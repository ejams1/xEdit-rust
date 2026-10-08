// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (ConflictLevelForMainRecord,
// ConflictLevelForContainer, ConflictLevelForNodeDatas,
// ConflictLevelForChildNodeDatas, InitChildren, InitNodes,
// InitConflictStatus, NodeDatasForMainRecord, NodeDatasForContainer,
// InheritStateFromChildren, vstViewGetText)

//! Conflict detection: the conflict code of the main form, moved out of the
//! GUI.
//!
//! The GUI compares the records of a FormID (the master and its overrides)
//! as the columns of its view tab: `NodeDatasForMainRecord` makes one
//! column per record, `InitChildren` decides the rows below a node (the
//! members of a record, the entries of an array; sorted arrays are merged
//! by their sort keys and unsorted arrays aligned with `TDiff`),
//! `InitNodes` fills the cells of a row, and `ConflictLevelForNodeDatas`
//! classifies a row: `TConflictAll` for the row, `TConflictThis` for each
//! cell. `ConflictLevelForMainRecord` runs this over the whole record
//! (`ConflictLevelForChildNodeDatas`) and keeps the result on every record
//! of the FormID, which is what the navigation tree colours and the
//! scripts read (`ConflictAllForMainRecord`, `ConflictThisForMainRecord`).
//! `InitConflictStatus` does the same over the tree of the view tab, which
//! [`ConflictContext::view_for_main_record`] builds for `xedit compare`.
//!
//! The differences to upstream:
//!
//! - Upstream keeps the result on the records (`TwbMainRecord.ConflictAll`
//!   and `ConflictThis`, the cache that `ConflictLevelForMainRecord` reads
//!   first); here a [`ConflictContext`] holds it, so that the records of
//!   different FormIDs can be classified on different threads and the
//!   results merged ([`conflict_statuses`]). The GMST and DFOB records are
//!   grouped by editor ID across FormIDs, so their result depends on the
//!   order they are asked for; they are classified on one thread, in the
//!   order of the files and records.
//! - Upstream's `InitChildren` sets the `SortOrder` of the elements it
//!   aligns; the view containers here keep their own sort orders, so a
//!   comparison changes nothing in the element tree.
//! - The mod groups of `NodeDatasForMainRecord` are a hook
//!   ([`ModGroupFilter`]) that phase 4 step 6 fills; without one the
//!   records are taken as upstream does with no mod group defined.
//! - `IsHidden` (records the user hid in the GUI) and the compare-to load
//!   (`fsCompareToHasSameMasters`) do not exist in the port yet: no record is
//!   hidden and no file is a compare-to file.

use std::cell::OnceCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use xedit_core::implementation::sortable::{self, Sortable};
use xedit_core::implementation::{ElementImpl, FileImpl, MainRecordImpl, pin_record, trim_initialized_records};
use xedit_core::interface::globals::{align_array_elements, align_array_limit, hide_ignored, translation_mode};
use xedit_core::interface::types::{ConflictAll, ConflictPriority, ConflictThis, DefFlag, DefType, ElementType};
use xedit_core::interface::{Container, Element, ElementRef, File, FileState, MainRecord, Signature};
use xedit_core::threads;

use crate::diff::{ChangeKind, Diff};

/// Port of `TViewNodeFlag` as bits of [`ViewNodeFlags`].
pub mod view_node_flag {
    pub const DONT_SHOW: u8 = 1;
    pub const IGNORE: u8 = 1 << 1;
    pub const USE_SORT_ORDER: u8 = 1 << 2;
    pub const IS_SORTED: u8 = 1 << 3;
    pub const IS_ALIGNED: u8 = 1 << 4;
    pub const IS_PARTIAL_FORM: u8 = 1 << 5;
}

/// Port of `TViewNodeFlags`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewNodeFlags(u8);

impl ViewNodeFlags {
    pub fn contains(self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    pub fn include(&mut self, flag: u8) {
        self.0 |= flag;
    }

    /// `ViewNodeFlags * [a, b] <> []`.
    pub fn any(self, flags: u8) -> bool {
        self.0 & flags != 0
    }
}

/// A container of the view: an element with its elements as the GUI holds
/// them (`sortable::view_elements`) and the sort orders the view gives them.
pub struct ViewContainer {
    element: ElementRef,
    elements: OnceCell<Vec<ElementRef>>,
    sort_orders: OnceCell<Vec<i32>>,
    /// The first element of each sort order, built on the first lookup
    /// after the sort orders were set.
    by_sort_order: OnceCell<HashMap<i32, usize>>,
}

impl ViewContainer {
    pub fn new(element: ElementRef) -> Self {
        ViewContainer {
            element,
            elements: OnceCell::new(),
            sort_orders: OnceCell::new(),
            by_sort_order: OnceCell::new(),
        }
    }

    pub fn element(&self) -> &ElementRef {
        &self.element
    }

    /// Upstream `Elements[]`.
    pub fn elements(&self) -> &[ElementRef] {
        self.elements.get_or_init(|| sortable::view_elements(&self.element))
    }

    /// Upstream `ElementCount`.
    pub fn element_count(&self) -> usize {
        self.elements().len()
    }

    pub fn element_type(&self) -> ElementType {
        self.element.get_element_type()
    }

    /// Upstream `AdditionalElementCount`.
    pub fn additional_element_count(&self) -> i32 {
        self.element
            .as_container()
            .map_or(0, |container| container.get_additional_element_count())
    }

    fn sort_orders(&self) -> &[i32] {
        self.sort_orders
            .get_or_init(|| self.elements().iter().map(|element| element.get_sort_order()).collect())
    }

    /// Upstream `IwbElement.SortOrder := aValue` on the element at `index`.
    fn set_sort_order(&mut self, index: usize, order: i32) {
        self.sort_orders();
        if let Some(orders) = self.sort_orders.get_mut() {
            orders[index] = order;
        }
        self.by_sort_order = OnceCell::new();
    }

    /// Port of `TwbContainer.GetElementBySortOrder`: the element whose sort
    /// order is `sort_order` less the additional elements.
    pub fn element_by_sort_order(&self, sort_order: i32) -> Option<&ElementRef> {
        let sort_order = sort_order - self.additional_element_count();
        let index = self.by_sort_order.get_or_init(|| {
            let mut map = HashMap::new();
            for (index, &order) in self.sort_orders().iter().enumerate() {
                map.entry(order).or_insert(index);
            }
            map
        });
        index.get(&sort_order).map(|&index| &self.elements()[index])
    }

    /// `Supports(Container, IwbSortableContainer)`.
    pub fn sortable(&self) -> Option<Sortable> {
        sortable::sortable(&self.element)
    }

    /// The record definition of a main record or subrecord structure
    /// (`Container.Def as IwbRecordDef`): its member count.
    fn record_member_count(&self) -> i32 {
        self.element
            .get_def()
            .and_then(|def| def.as_record_def().map(|record_def| record_def.get_member_count()))
            .unwrap_or(0)
    }
}

/// Port of `TViewNodeData`: one cell of the view, the element of one record
/// in one row.
pub struct ViewNodeData {
    pub element: Option<ElementRef>,
    pub container: Option<ViewContainer>,
    pub conflict_all: ConflictAll,
    pub conflict_this: ConflictThis,
    pub flags: ViewNodeFlags,
}

impl Default for ViewNodeData {
    /// A zeroed record, as the virtual tree hands out.
    fn default() -> Self {
        ViewNodeData {
            element: None,
            container: None,
            conflict_all: ConflictAll::caUnknown,
            conflict_this: ConflictThis::ctUnknown,
            flags: ViewNodeFlags::default(),
        }
    }
}

/// Port of `TVirtualNodeInitStates` as far as `InitNodes` sets them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InitialStates {
    /// `ivsDisabled`: no record has an element in this row.
    pub disabled: bool,
    /// `ivsHasChildren`.
    pub has_children: bool,
}

/// The mod groups' part of `NodeDatasForMainRecord`: with mod groups
/// enabled, the records of a FormID that a mod group hides are left out of
/// the comparison (and get `ctHiddenByModGroup`). Phase 4 step 6 ports
/// `wbModGroups` behind this hook.
pub trait ModGroupFilter: Send + Sync {
    /// Called with the records of a FormID in load order when there are more
    /// than two (`ModGroupsEnabled and (Length(MainRecords) > 2)`); keeps the
    /// records the comparison shows.
    fn filter(&self, records: &mut Vec<Arc<MainRecordImpl>>);
}

/// The options of the main form that the conflict code reads.
#[derive(Clone, Default)]
pub struct ConflictOptions {
    /// `OnlyShowMasterAndLeafs`: compare only the master and the overrides
    /// no other override has as a master (`MasterAndLeafs`).
    pub only_master_and_leafs: bool,
    /// `xeQuickShowConflicts` (the `-quickshowconflicts` mode): a FormID
    /// with exactly one override is an override without comparing them.
    pub quick_show_conflicts: bool,
    /// `ComparingSiblings`: the view compares records of different FormIDs.
    pub comparing_siblings: bool,
    /// `HideNoConflict` of the view: rows without a conflict are hidden.
    pub hide_no_conflict: bool,
    /// `ModGroupsEnabled` with the mod groups; `None` is no mod group.
    pub mod_groups: Option<Arc<dyn ModGroupFilter>>,
}

/// The conflict status of a main record: upstream `TwbMainRecord.ConflictAll`
/// and `ConflictThis`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConflictStatus {
    pub all: ConflictAll,
    pub this: ConflictThis,
}

impl Default for ConflictStatus {
    fn default() -> Self {
        ConflictStatus {
            all: ConflictAll::caUnknown,
            this: ConflictThis::ctUnknown,
        }
    }
}

/// The state of a comparison: the options, the loaded files in the order of
/// upstream's `Files`, the statuses found so far (upstream keeps them on the
/// records) and the messages the GUI would log.
pub struct ConflictContext<'a> {
    options: &'a ConflictOptions,
    files: &'a [Arc<FileImpl>],
    statuses: HashMap<usize, ConflictStatus>,
    /// Messages upstream posts to the message log (`PostAddMessage`).
    pub messages: Vec<String>,
}

fn record_id(record: &MainRecordImpl) -> usize {
    record.get_element_id()
}

fn is_gmst_or_dfob(signature: Signature) -> bool {
    matches!(signature.0.as_slice(), b"GMST" | b"DFOB")
}

fn master_or_self(record: &Arc<MainRecordImpl>) -> Arc<MainRecordImpl> {
    record.master().unwrap_or_else(|| record.clone())
}

fn element_as_record(element: &ElementRef) -> Option<Arc<MainRecordImpl>> {
    element.as_element_impl().and_then(ElementImpl::main_record_impl)
}

/// `CompareStr` of two strings: by their UTF-16 code units.
fn compare_str(a: &str, b: &str) -> std::cmp::Ordering {
    if a.is_ascii() && b.is_ascii() {
        a.as_bytes().cmp(b.as_bytes())
    } else {
        a.encode_utf16().cmp(b.encode_utf16())
    }
}

/// A key of a `TwbFastStringListCS`: ordered as `CompareStr` orders it.
#[derive(Clone, PartialEq, Eq)]
struct Key(String);

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Key {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        compare_str(&self.0, &other.0)
    }
}

/// The files in the order of upstream's `Files`: the load order, the
/// hardcoded file (the game's executable) after the game master it loads
/// with.
pub fn files_in_load_order(files: Vec<Arc<FileImpl>>) -> Vec<Arc<FileImpl>> {
    let mut files = files;
    files.sort_by_key(|file| {
        (
            file.load_order(),
            file.get_file_states().contains(FileState::fsIsHardcoded),
        )
    });
    files
}

impl<'a> ConflictContext<'a> {
    pub fn new(options: &'a ConflictOptions, files: &'a [Arc<FileImpl>]) -> Self {
        ConflictContext {
            options,
            files,
            statuses: HashMap::new(),
            messages: Vec::new(),
        }
    }

    /// Upstream `TwbMainRecord.ConflictAll` and `ConflictThis`.
    pub fn status(&self, record: &MainRecordImpl) -> ConflictStatus {
        self.statuses.get(&record_id(record)).copied().unwrap_or_default()
    }

    fn set_status(&mut self, record: &MainRecordImpl, status: ConflictStatus) {
        self.statuses.insert(record_id(record), status);
    }

    /// The statuses found so far, by the identity of the record.
    pub fn into_statuses(self) -> (HashMap<usize, ConflictStatus>, Vec<String>) {
        (self.statuses, self.messages)
    }

    /// Port of `TfrmMain.ConflictLevelForMainRecord`.
    pub fn conflict_level_for_main_record(&mut self, record: &Arc<MainRecordImpl>) -> ConflictStatus {
        let mut result = self.status(record);
        if result.all > ConflictAll::caUnknown {
            return result;
        }

        let master = master_or_self(record);
        if master.overrides().is_empty() && !translation_mode() && !is_gmst_or_dfob(master.get_signature()) {
            result = ConflictStatus {
                all: ConflictAll::caOnlyOne,
                this: ConflictThis::ctOnlyOne,
            };
            self.set_status(record, result);
            return result;
        }

        let injected = master.is_injected() && !is_gmst_or_dfob(record.get_signature());
        let mut node_datas = self.node_datas_for_main_record(record);
        if node_datas.len() == 1 && !translation_mode() {
            result.all = ConflictAll::caOnlyOne;
            node_datas[0].conflict_all = ConflictAll::caOnlyOne;
            node_datas[0].conflict_this = ConflictThis::ctOnlyOne;
        } else if node_datas.len() == 2 {
            if self.options.quick_show_conflicts {
                result.all = ConflictAll::caOverride;
                node_datas[0].conflict_all = ConflictAll::caOverride;
                node_datas[1].conflict_all = ConflictAll::caOverride;
                node_datas[0].conflict_this = ConflictThis::ctMaster;
                node_datas[1].conflict_this = ConflictThis::ctOverride;
            } else if is_compare_to_same(&node_datas) {
                result.all = ConflictAll::caNoConflict;
                node_datas[0].conflict_all = ConflictAll::caNoConflict;
                node_datas[1].conflict_all = ConflictAll::caNoConflict;
                node_datas[0].conflict_this = ConflictThis::ctMaster;
                node_datas[1].conflict_this = ConflictThis::ctIdenticalToMaster;
            } else {
                result.all = self.conflict_level_for_child_node_datas(&mut node_datas, false, injected);
                // Upstream calls `IsCompareToSame` again here and drops its
                // result.
            }
        } else {
            result.all = self.conflict_level_for_child_node_datas(&mut node_datas, false, injected);
        }

        for node_data in &node_datas {
            if let Some(element) = &node_data.element
                && let Some(record) = element_as_record(element)
            {
                self.set_status(
                    &record,
                    ConflictStatus {
                        all: result.all,
                        this: node_data.conflict_this,
                    },
                );
            }
        }

        // `Fix`: every record of the FormID gets the result; one that was
        // not compared (left out by a mod group) is hidden by the mod group.
        let fix = |context: &mut Self, record: &MainRecordImpl| {
            let mut status = context.status(record);
            status.all = result.all;
            if status.this == ConflictThis::ctUnknown {
                status.this = ConflictThis::ctHiddenByModGroup;
            }
            context.set_status(record, status);
        };
        fix(self, &master);
        for record in master.overrides() {
            fix(self, &record);
        }

        result.this = self.status(record).this;
        result
    }

    /// Port of `TfrmMain.ConflictLevelForContainer`: a main record as
    /// `ConflictLevelForMainRecord`, any other container compared with the
    /// element at its path in every other file.
    pub fn conflict_level_for_container(&mut self, container: &ElementRef) -> ConflictStatus {
        if let Some(record) = element_as_record(container) {
            return self.conflict_level_for_main_record(&record);
        }
        let mut result = ConflictStatus::default();
        let mut node_datas = self.node_datas_for_container(container);
        if node_datas.len() == 1 {
            result.all = ConflictAll::caOnlyOne;
            node_datas[0].conflict_all = ConflictAll::caOnlyOne;
            node_datas[0].conflict_this = ConflictThis::ctOnlyOne;
        } else if self.options.quick_show_conflicts && node_datas.len() == 2 {
            result.all = ConflictAll::caOverride;
            node_datas[0].conflict_all = ConflictAll::caOverride;
            node_datas[1].conflict_all = ConflictAll::caOverride;
            node_datas[0].conflict_this = ConflictThis::ctMaster;
            node_datas[1].conflict_this = ConflictThis::ctOverride;
        } else {
            result.all = self.conflict_level_for_child_node_datas(&mut node_datas, false, false);
        }
        for node_data in &node_datas {
            if let Some(element) = &node_data.element
                && element.get_element_id() == container.get_element_id()
            {
                result.this = node_data.conflict_this;
            }
        }
        result
    }

    /// Port of `TfrmMain.NodeDatasForContainer`: the element at the path of
    /// `container` in every loaded file that is not a plugin.
    ///
    /// UPSTREAM-QUIRK: upstream asks `IsNotPlugin` of each file, which keeps
    /// the hardcoded file and the saves and leaves out every plugin, so the
    /// container itself is found only in such a file.
    pub fn node_datas_for_container(&mut self, container: &ElementRef) -> Vec<ViewNodeData> {
        let file_path = container.get_file().map(|file| file.get_path()).unwrap_or_default();
        let mut path = container.get_path();
        if let Some(rest) = path.strip_prefix(&format!("{file_path} \\ ")) {
            path = rest.to_owned();
        }
        // `GetPath` to `ByPath`: " \ " becomes "\".
        let path = path.replace(" \\ ", "\\");
        let mut result = Vec::new();
        for file in self.files {
            if !file.get_is_not_plugin() {
                continue;
            }
            if let Some(element) = file.get_element_by_path(&path) {
                let mut node_data = ViewNodeData {
                    element: Some(element.clone()),
                    ..Default::default()
                };
                let view = ViewContainer::new(element);
                if view.element_count() >= 1 {
                    node_data.container = Some(view);
                }
                result.push(node_data);
            }
        }
        result
    }

    /// The records of the top group of `file` for `signature`
    /// (`GroupBySignature`), in the order of the group.
    fn group_records(file: &FileImpl, signature: Signature) -> Vec<Arc<MainRecordImpl>> {
        let label = signature.to_int();
        let group = (0..file.get_element_count())
            .filter_map(|index| file.get_element(index))
            .find(|element| {
                element
                    .as_element_impl()
                    .and_then(ElementImpl::group_record_impl)
                    .is_some_and(|group| group.group_type() == 0 && group.group_label() == label)
            });
        let Some(group) = group else {
            return Vec::new();
        };
        let Some(group) = group.as_container() else {
            return Vec::new();
        };
        (0..group.get_element_count())
            .filter_map(|index| group.get_element(index))
            .filter_map(|element| element_as_record(&element))
            .collect()
    }

    /// Port of `TfrmMain.NodeDatasForMainRecord`: the columns of the view of
    /// a record. GMST and DFOB records are compared by editor ID, the first
    /// of each file; a NAVI record and a file header with the records of the
    /// files of the same load order; every other record with the master and
    /// the overrides of its FormID.
    pub fn node_datas_for_main_record(&mut self, record: &Arc<MainRecordImpl>) -> Vec<ViewNodeData> {
        let signature = record.get_signature();
        let mut records: Vec<Arc<MainRecordImpl>> = Vec::new();
        if is_gmst_or_dfob(signature) {
            let editor_id = record.get_editor_id();
            for file in self.files {
                if let Some(found) = Self::group_records(file, signature)
                    .into_iter()
                    .find(|other| other.get_editor_id().eq_ignore_ascii_case(&editor_id))
                {
                    records.push(found);
                }
            }
        } else if signature.0 == *b"NAVI" {
            let form_id = record.form_id();
            let load_order = record.get_file().map_or(-1, |file| file.get_load_order());
            for file in self.files {
                if file.load_order() == load_order
                    && let Some(found) = Self::group_records(file, signature)
                        .into_iter()
                        .find(|other| other.form_id() == form_id)
                {
                    records.push(found);
                }
            }
        } else if signature.0 == *b"TES4" {
            let own_file = record.get_file();
            let load_order = own_file.as_ref().map_or(-1, |file| file.get_load_order());
            let own_is_exe = own_file.as_ref().is_some_and(|file| is_exe(&file.get_name()));
            for file in self.files {
                if file.load_order() != load_order {
                    continue;
                }
                // The header of the executable shows only itself, and the
                // others leave it out.
                if own_is_exe != is_exe(file.file_name()) {
                    continue;
                }
                if let Some(header) = file.header() {
                    records.push(header);
                }
            }
        } else {
            let master = master_or_self(record);
            if self.options.only_master_and_leafs {
                records = master_and_leafs(&master);
            } else {
                records.push(master.clone());
                records.extend(master.overrides());
            }
        }

        if let Some(mod_groups) = &self.options.mod_groups
            && records.len() > 2
        {
            mod_groups.filter(&mut records);
        }

        // `IsHidden`: the port has no hidden records yet.
        if records.is_empty() {
            records.push(record.clone());
        }

        let mut result = Vec::with_capacity(records.len());
        let master_signature = records[0].get_signature();
        for record in records {
            let element: ElementRef = record.clone();
            let mut node_data = ViewNodeData {
                element: Some(element.clone()),
                ..Default::default()
            };
            if record.get_element_count() != 0 && record.get_signature() == master_signature {
                node_data.container = Some(ViewContainer::new(element));
            }
            result.push(node_data);
        }
        result
    }

    /// Port of `TfrmMain.ConflictLevelForChildNodeDatas`: the rows below the
    /// cells `node_datas`, compared row by row; returns the highest
    /// `TConflictAll` of the rows and raises the `ConflictThis` of each cell
    /// to the highest of its column.
    pub fn conflict_level_for_child_node_datas(
        &mut self,
        node_datas: &mut [ViewNodeData],
        sibling_compare: bool,
        injected: bool,
    ) -> ConflictAll {
        let mut result = match node_datas.len() {
            0 => ConflictAll::caUnknown,
            1 => {
                if !translation_mode() {
                    node_datas[0].conflict_this = ConflictThis::ctOnlyOne;
                }
                ConflictAll::caOnlyOne
            }
            _ => ConflictAll::caNoConflict,
        };
        if translation_mode() {
            if result < ConflictAll::caOnlyOne {
                return result;
            }
        } else if result < ConflictAll::caNoConflict {
            return result;
        }

        let child_count = self.init_children(node_datas);
        for index in 0..child_count {
            let mut children: Vec<ViewNodeData> = (0..node_datas.len()).map(|_| ViewNodeData::default()).collect();
            let states = init_nodes(&mut children, node_datas, index as i32);
            if !states.disabled {
                let conflict_all = if states.has_children {
                    self.conflict_level_for_child_node_datas(&mut children, sibling_compare, injected)
                } else {
                    conflict_level_for_node_datas(&mut children, sibling_compare, injected)
                };
                if conflict_all > result {
                    result = conflict_all;
                }
                for (parent, child) in node_datas.iter_mut().zip(&children) {
                    if child.conflict_this > parent.conflict_this {
                        parent.conflict_this = child.conflict_this;
                    }
                }
            } else {
                let mut conflict_this = ConflictThis::ctNotDefined;
                let element = node_datas.iter().find_map(|node_data| node_data.container.as_ref());
                if let Some(container) = element
                    && matches!(
                        container.element_type(),
                        ElementType::etMainRecord | ElementType::etSubRecordStruct
                    )
                {
                    let member_count = container.record_member_count();
                    let additional = container.additional_element_count();
                    let member = index as i32 - additional;
                    if index as i32 >= additional
                        && member < member_count
                        && let Some(def) = container.element.get_def()
                        && let Some(record_def) = def.as_record_def()
                    {
                        let member_def = record_def.get_member(member as usize);
                        if translation_mode()
                            && (!member_def.def_base().def_flags.contains(DefFlag::dfTranslatable)
                                || member_def.get_conflict_priority(None) == ConflictPriority::cpIgnore)
                        {
                            conflict_this = ConflictThis::ctIgnored;
                        }
                    }
                }
                if element.is_none() && translation_mode() {
                    conflict_this = ConflictThis::ctIgnored;
                }
                for node_data in node_datas.iter_mut() {
                    if conflict_this > node_data.conflict_this {
                        node_data.conflict_this = conflict_this;
                    }
                }
            }
        }
        result
    }

    /// Port of `TfrmMain.InitChildren`: the number of rows below the cells
    /// `node_datas`. For sorted containers the elements are matched by their
    /// sort keys and take the index of their key as their sort order; the
    /// entries of alignable arrays are aligned with `TDiff` and take their
    /// aligned position as their sort order.
    pub fn init_children(&mut self, node_datas: &mut [ViewNodeData]) -> usize {
        let node_count = node_datas.len();
        let mut sorted_count = 0;
        let mut alignable_count = 0;
        let mut non_sorted_count = 0;
        let mut first_container: Option<usize> = None;
        for (index, node_data) in node_datas.iter().enumerate() {
            if first_container.is_none() {
                first_container = node_data.container.as_ref().map(|_| index);
            }
            if let Some(container) = &node_data.container {
                match container.sortable() {
                    Some(sortable) => {
                        if sortable.sorted {
                            sorted_count += 1;
                        } else if sortable.alignable {
                            alignable_count += 1;
                        }
                    }
                    None => non_sorted_count += 1,
                }
            }
        }

        let kinds =
            usize::from(sorted_count > 0) + usize::from(alignable_count > 0) + usize::from(non_sorted_count > 0);
        if kinds > 1 {
            if let Some(index) = first_container
                && let Some(container) = &node_datas[index].container
            {
                let record_name = container
                    .element
                    .get_containing_main_record()
                    .map(|record| record.get_name())
                    .unwrap_or_default();
                self.messages.push(format!(
                    "Warning: Comparing a mix of sorted, unsorted, and/or alignable entries for \"{}\" in \"{}\"",
                    container.element.get_path(),
                    record_name
                ));
            }
            sorted_count = 0;
            alignable_count = 0;
        }

        let mut child_count: usize = 0;
        if sorted_count > 0 {
            // The keys of each container with a counter for repeated keys,
            // and all keys in `CompareStr` order.
            let mut all_keys: BTreeSet<Key> = BTreeSet::new();
            let mut keys_of: Vec<Option<HashMap<String, usize>>> = Vec::with_capacity(node_count);
            for node_data in node_datas.iter_mut() {
                node_data.flags.include(view_node_flag::IS_SORTED);
                let Some(container) = node_data.container.as_ref().filter(|c| c.sortable().is_some()) else {
                    keys_of.push(None);
                    continue;
                };
                let mut keys = HashMap::new();
                let mut dup_counter = 0;
                let mut last_sort_key = String::new();
                for (index, element) in container.elements().iter().enumerate() {
                    let sort_key = sortable::display_sort_key(element, false);
                    if last_sort_key == sort_key {
                        dup_counter += 1;
                    } else {
                        dup_counter = 0;
                        last_sort_key = sort_key.clone();
                    }
                    let sort_key = format!("{sort_key}<{dup_counter:04X}>");
                    // UPSTREAM-QUIRK: the list of a container does not accept
                    // duplicates (`dupError`), which a key repeated apart from
                    // its neighbours would raise; the first one is kept.
                    keys.entry(sort_key.clone()).or_insert(index);
                    all_keys.insert(Key(sort_key));
                }
                keys_of.push(Some(keys));
            }
            child_count = all_keys.len();
            for (order, key) in all_keys.iter().enumerate() {
                for (node_data, keys) in node_datas.iter_mut().zip(&keys_of) {
                    if let Some(keys) = keys
                        && let Some(&index) = keys.get(&key.0)
                        && let Some(container) = &mut node_data.container
                    {
                        container.set_sort_order(index, order as i32);
                    }
                }
            }
        } else {
            let mut all_keys: Option<Vec<String>> = (align_array_elements() && alignable_count > 1).then(Vec::new);
            for node_data in node_datas.iter() {
                let Some(container) = &node_data.container else {
                    continue;
                };
                match container.element_type() {
                    ElementType::etMainRecord | ElementType::etSubRecordStruct => {
                        child_count =
                            (container.record_member_count() + container.additional_element_count()).max(0) as usize;
                        if container.element_count() > child_count {
                            let record_name = container
                                .element
                                .get_containing_main_record()
                                .map(|record| record.get_name())
                                .unwrap_or_default();
                            self.messages.push(format!(
                                "Error: Container.ElementCount {{{}}} > aChildCount {{{}}} for {} in {}",
                                container.element_count(),
                                child_count,
                                container.element.get_path(),
                                record_name
                            ));
                            for (index, element) in container.elements().iter().enumerate() {
                                self.messages.push(format!("  #{index}: {}", element.get_name()));
                            }
                        }
                    }
                    ElementType::etSubRecordArray | ElementType::etSubRecord | ElementType::etArray => {
                        if container.element_count() as i64 > i64::from(align_array_limit()) {
                            all_keys = None;
                        }
                        if let Some(all_keys) = &mut all_keys {
                            for element in container.elements() {
                                all_keys.push(sortable::display_sort_key(element, false));
                            }
                        }
                        child_count = child_count.max(container.element_count());
                    }
                    ElementType::etStruct
                    | ElementType::etValue
                    | ElementType::etUnion
                    | ElementType::etStructChapter => {
                        child_count = child_count.max(container.element_count());
                    }
                    _ => {}
                }
            }
            if let Some(all_keys) = all_keys {
                let all_keys: Vec<Key> = all_keys
                    .into_iter()
                    .map(Key)
                    .collect::<BTreeSet<Key>>()
                    .into_iter()
                    .collect();
                if all_keys.len() > 1 {
                    child_count = child_count.max(self.align(node_datas, &all_keys));
                    for node_data in node_datas.iter_mut() {
                        node_data.flags.include(view_node_flag::USE_SORT_ORDER);
                        node_data.flags.include(view_node_flag::IS_ALIGNED);
                    }
                }
            }
        }
        child_count
    }

    /// The alignment of `InitChildren`: the entries of each container are
    /// diffed against the alignment of the containers before it, by the
    /// index of their sort key in `all_keys`; each entry takes its aligned
    /// position as its sort order. Returns the length of the alignment.
    fn align(&mut self, node_datas: &mut [ViewNodeData], all_keys: &[Key]) -> usize {
        let key_index = |key: String| -> i32 {
            all_keys
                .binary_search_by(|probe| compare_str(&probe.0, &key))
                .map_or(-1, |index| index as i32)
        };
        // `KeyedElements`: per container, the index of the element at each
        // aligned position.
        let mut keyed: Vec<Vec<Option<usize>>> = vec![Vec::new(); node_datas.len()];
        let mut left_keys: Vec<i32> = Vec::new();
        let mut have_first = false;
        let mut child_count = 0;
        for (i, node_data) in node_datas.iter().enumerate() {
            let Some(container) = &node_data.container else {
                continue;
            };
            if container.element_count() == 0 {
                continue;
            }
            if !have_first {
                have_first = true;
                left_keys = container
                    .elements()
                    .iter()
                    .map(|element| key_index(sortable::display_sort_key(element, false)))
                    .collect();
                keyed[i] = (0..container.element_count()).map(Some).collect();
                continue;
            }
            let right_keys: Vec<i32> = container
                .elements()
                .iter()
                .map(|element| key_index(sortable::display_sort_key(element, false)))
                .collect();
            let diff = Diff::execute(&left_keys, &right_keys, false);
            let count = diff.count();
            let compares = diff.compares();
            for previous in keyed.iter_mut().take(i) {
                if previous.is_empty() {
                    continue;
                }
                previous.resize(count, None);
                for k in (0..count).rev() {
                    let compare = compares[k];
                    if matches!(compare.kind, ChangeKind::None | ChangeKind::Delete) {
                        let old = compare.old_index1 as usize;
                        if old != k {
                            previous[k] = previous[old];
                            previous[old] = None;
                        }
                    }
                }
            }
            let mut merged = vec![0; count];
            let mut own = vec![None; count];
            for k in (0..count).rev() {
                let compare = compares[k];
                if matches!(compare.kind, ChangeKind::None | ChangeKind::Add) {
                    own[k] = Some(compare.old_index2 as usize);
                    merged[k] = compare.int2;
                } else {
                    merged[k] = compare.int1;
                }
            }
            keyed[i] = own;
            left_keys = merged;
            child_count = child_count.max(count);
        }
        for (node_data, keyed) in node_datas.iter_mut().zip(&keyed) {
            let Some(container) = &mut node_data.container else {
                continue;
            };
            for (position, index) in keyed.iter().enumerate() {
                if let Some(index) = index {
                    container.set_sort_order(*index, position as i32);
                }
            }
        }
        child_count
    }

    /// Port of `TfrmMain.InitConflictStatus` over a view tree: the leaves
    /// classified with `ConflictLevelForNodeDatas`, the conflict of every
    /// node raised to the highest of its children, and the visibility of the
    /// rows.
    fn init_conflict_status(
        &self,
        node: &mut ViewNode,
        injected: bool,
        root: bool,
        parent_containers: &[Option<ElementRef>],
    ) {
        let column_count = node.datas.len();
        if node.children.is_empty() {
            node.datas[0].conflict_all =
                conflict_level_for_node_datas(&mut node.datas, self.options.comparing_siblings, injected);
        } else {
            let containers: Vec<Option<ElementRef>> = node
                .datas
                .iter()
                .map(|data| data.container.as_ref().map(|container| container.element.clone()))
                .collect();
            for child in &mut node.children {
                self.init_conflict_status(child, injected, false, &containers);
                for i in 0..column_count {
                    // `InheritConflict`.
                    if child.datas[i].conflict_all > node.datas[i].conflict_all {
                        node.datas[i].conflict_all = child.datas[i].conflict_all;
                    }
                    if child.datas[i].conflict_this > node.datas[i].conflict_this {
                        node.datas[i].conflict_this = child.datas[i].conflict_this;
                    }
                }
            }
        }

        let mut has_element = false;
        let mut conflict_all = ConflictAll::caUnknown;
        let mut conflict_this = ConflictThis::ctUnknown;
        for data in &node.datas {
            has_element |= data.element.is_some();
            conflict_all = conflict_all.max(data.conflict_all);
            conflict_this = conflict_this.max(data.conflict_this);
        }
        if !has_element && translation_mode() {
            conflict_this = ConflictThis::ctIgnored;
        }
        if matches!(conflict_all, ConflictAll::caUnknown | ConflictAll::caOnlyOne) && self.options.comparing_siblings {
            conflict_all = ConflictAll::caNoConflict;
        }
        for data in &mut node.datas {
            data.conflict_all = conflict_all;
        }

        if root {
            return;
        }
        let mut dont_show = false;
        for data in &node.datas {
            if data.flags.contains(view_node_flag::DONT_SHOW) {
                dont_show = true;
            }
            if data.container.is_some() {
                dont_show = false;
                break;
            }
        }
        node.visible = match conflict_this {
            ConflictThis::ctUnknown => !dont_show && !translation_mode(),
            ConflictThis::ctIgnored => !hide_ignored(),
            ConflictThis::ctNotDefined => {
                let is_record_container = |element: &ElementRef| {
                    matches!(
                        element.get_element_type(),
                        ElementType::etMainRecord | ElementType::etSubRecordStruct
                    )
                };
                // The member of the record or subrecord structure above the
                // node, by the position of the node.
                let member_of = |element: &ElementRef| {
                    let additional = element
                        .as_container()
                        .map_or(0, |container| container.get_additional_element_count());
                    let member = node.index as i32 - additional;
                    let def = element.get_def()?;
                    let record_def = def.as_record_def()?;
                    (node.index as i32 >= additional && member < record_def.get_member_count())
                        .then(|| record_def.get_member(member as usize))
                };
                let mut element = parent_containers.iter().find_map(Clone::clone);
                if let Some(parent) = element.clone()
                    && is_record_container(&parent)
                    && let Some(member_def) = member_of(&parent)
                {
                    if translation_mode()
                        && (!member_def.def_base().def_flags.contains(DefFlag::dfTranslatable)
                            || member_def.get_conflict_priority(None) == ConflictPriority::cpIgnore)
                    {
                        conflict_this = ConflictThis::ctIgnored;
                        for data in &mut node.datas {
                            data.conflict_this = conflict_this;
                        }
                    }
                    if conflict_this != ConflictThis::ctIgnored && member_def.get_has_dont_show() {
                        dont_show = true;
                        for container in parent_containers {
                            element = container.clone();
                            if let Some(container) = &element {
                                dont_show = member_def.get_dont_show(Some(container));
                                if !dont_show {
                                    break;
                                }
                            }
                        }
                    }
                }
                if element.is_none() && translation_mode() {
                    conflict_this = ConflictThis::ctIgnored;
                }
                if conflict_this == ConflictThis::ctNotDefined
                    && let Some(parent) = parent_containers.iter().find_map(Clone::clone)
                    && is_record_container(&parent)
                    && member_of(&parent)
                        .is_some_and(|member_def| member_def.get_conflict_priority(None) == ConflictPriority::cpIgnore)
                {
                    conflict_this = ConflictThis::ctIgnored;
                }
                (conflict_this != ConflictThis::ctIgnored || !hide_ignored()) && !dont_show
            }
            _ => !dont_show,
        };
        if node.visible && self.options.hide_no_conflict {
            if column_count > 1 {
                if self.options.comparing_siblings {
                    if conflict_all < ConflictAll::caConflictBenign {
                        node.visible = false;
                    }
                } else if conflict_this < ConflictThis::ctOverride {
                    node.visible = false;
                }
            } else if !has_element {
                node.visible = false;
            }
        }
    }

    /// The tree of the view tab for `record` (`SetActiveRecord`): one column
    /// per record of `NodeDatasForMainRecord`, the rows of every level built
    /// as the virtual tree builds them when `InitConflictStatus` walks it,
    /// and the conflict status of every node.
    pub fn view_for_main_record(&mut self, record: &Arc<MainRecordImpl>) -> ViewNode {
        let active_master = master_or_self(record);
        let datas = self.node_datas_for_main_record(record);
        let mut root = ViewNode {
            datas,
            children: Vec::new(),
            index: 0,
            visible: true,
        };
        if let Some(def) = active_master.get_def()
            && let Some(record_def) = def.as_record_def()
        {
            let count = record_def.get_member_count() + active_master.get_additional_element_count();
            for index in 0..count.max(0) {
                let child = self.build_view_node(&mut root.datas, index as usize, false);
                root.children.push(child);
            }
            let injected = active_master.is_injected() && !is_gmst_or_dfob(active_master.get_signature());
            self.init_conflict_status(&mut root, injected, true, &[]);
        }
        root
    }

    /// A node of the view below `parent` at `index`, with its children when
    /// it has any (`vstViewInitNode`, `vstViewInitChildren`).
    fn build_view_node(&mut self, parent: &mut [ViewNodeData], index: usize, _parent_initialized: bool) -> ViewNode {
        let mut datas: Vec<ViewNodeData> = (0..parent.len()).map(|_| ViewNodeData::default()).collect();
        let states = init_nodes(&mut datas, parent, index as i32);
        let mut node = ViewNode {
            datas,
            children: Vec::new(),
            index,
            visible: true,
        };
        if states.has_children {
            let child_count = self.init_children(&mut node.datas);
            for child_index in 0..child_count {
                let child = self.build_view_node(&mut node.datas, child_index, true);
                node.children.push(child);
            }
        }
        node
    }
}

/// The conflict status of every main record of `files` (in the order of
/// upstream's `Files`, see [`files_in_load_order`]), as
/// `ConflictLevelForMainRecord` gives it when it is asked for every record
/// of every file in that order, which is what a script that walks the files
/// with `ConflictAllForMainRecord` sees.
///
/// The records of one FormID form a group that upstream classifies at once
/// and keeps on all of them; the groups are independent, so they are
/// classified on the worker threads ([`threads::pool`]) in batches, each
/// record built while its group is compared and reset afterwards. The GMST,
/// DFOB, NAVI and TES4 records, whose groups are not their FormID's (see
/// `NodeDatasForMainRecord`) and depend on which record asks first, are
/// classified afterwards on the calling thread in file order. When two
/// builds needed each other during a batch (`init_cycles`), the batch is
/// classified again on the calling thread, so the result does not depend on
/// the thread count.
pub fn conflict_statuses(files: &[Arc<FileImpl>], options: &ConflictOptions) -> ConflictResults {
    let mut results = ConflictResults::default();
    let mut serial = Vec::new();
    let mut groups: Vec<Arc<MainRecordImpl>> = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    for file in files {
        for record in file.records() {
            let signature = record.get_signature();
            if is_gmst_or_dfob(signature) || signature.0 == *b"NAVI" || signature.0 == *b"TES4" {
                serial.push(record);
                continue;
            }
            let master = master_or_self(&record);
            if master.overrides().is_empty() && !translation_mode() {
                results.statuses.insert(
                    record_id(&record),
                    ConflictStatus {
                        all: ConflictAll::caOnlyOne,
                        this: ConflictThis::ctOnlyOne,
                    },
                );
                continue;
            }
            if seen.insert(record_id(&master)) {
                groups.push(record);
            }
        }
    }

    for batch in groups.chunks(GROUPS_PER_BATCH) {
        let cycles = threads::init_cycles();
        let classified: Vec<(HashMap<usize, ConflictStatus>, Vec<String>)> = match threads::pool() {
            Some(pool) => pool.install(|| {
                use rayon::prelude::*;
                batch
                    .par_iter()
                    .with_max_len(1)
                    .map(|record| classify_group(record, options, files))
                    .collect()
            }),
            None => batch
                .iter()
                .map(|record| classify_group(record, options, files))
                .collect(),
        };
        let classified = if threads::init_cycles() == cycles {
            classified
        } else {
            // Two builds needed each other: what the threads saw meanwhile
            // could depend on timing, so the batch runs again on one thread
            // from records that are not built.
            results.messages.push(
                "Warning: two records needed each other while they were built; comparing the records again on one thread"
                    .to_owned(),
            );
            for record in batch {
                let master = master_or_self(record);
                master.reset();
                for record in master.overrides() {
                    record.reset();
                }
            }
            trim_initialized_records(0, None);
            batch
                .iter()
                .map(|record| classify_group(record, options, files))
                .collect()
        };
        for (statuses, messages) in classified {
            results.statuses.extend(statuses);
            results.messages.extend(messages);
        }
    }

    let mut context = ConflictContext::new(options, files);
    for record in &serial {
        let _read = threads::read_guard();
        context.conflict_level_for_main_record(record);
        record.reset();
        trim_initialized_records(KEPT_RECORDS, None);
    }
    let (statuses, messages) = context.into_statuses();
    results.statuses.extend(statuses);
    results.messages.extend(messages);
    results
}

/// The FormID groups a batch of [`conflict_statuses`] holds.
const GROUPS_PER_BATCH: usize = 1024;

/// The other records whose elements stay built while the groups are
/// compared (as the dump keeps them, `trim_initialized_records`).
const KEPT_RECORDS: usize = 1024;

/// Classifies the FormID group of `record`, with the records of the group
/// built while they are compared and reset afterwards.
fn classify_group(
    record: &Arc<MainRecordImpl>,
    options: &ConflictOptions,
    files: &[Arc<FileImpl>],
) -> (HashMap<usize, ConflictStatus>, Vec<String>) {
    let _read = threads::read_guard();
    let master = master_or_self(record);
    let mut members = vec![master.clone()];
    members.extend(master.overrides());
    let pins: Vec<_> = members.iter().map(pin_record).collect();
    let mut context = ConflictContext::new(options, files);
    context.conflict_level_for_main_record(record);
    drop(pins);
    for member in &members {
        member.reset();
    }
    trim_initialized_records(KEPT_RECORDS, None);
    context.into_statuses()
}

/// The result of [`conflict_statuses`].
#[derive(Default)]
pub struct ConflictResults {
    /// The status of every main record, by the identity of the record
    /// (`Element::get_element_id`).
    pub statuses: HashMap<usize, ConflictStatus>,
    /// The messages upstream would log while it compares.
    pub messages: Vec<String>,
}

impl ConflictResults {
    /// The status of `record`; `caUnknown` for a record that was not compared.
    pub fn status(&self, record: &MainRecordImpl) -> ConflictStatus {
        self.statuses.get(&record_id(record)).copied().unwrap_or_default()
    }
}

/// A node of the view tab: its cells, its rows and whether the view shows
/// it.
pub struct ViewNode {
    pub datas: Vec<ViewNodeData>,
    pub children: Vec<ViewNode>,
    /// `Node.Index`: the position below its parent.
    pub index: usize,
    /// `vstView.IsVisible[Node]` after `InitConflictStatus`.
    pub visible: bool,
}

impl ViewNode {
    /// Port of `vstViewGetText` for the name column (column 0, with no
    /// focused column): the display name of the first element of the row,
    /// with " (sorted)" or " (aligned)" for a merged or aligned row; for a
    /// row without an element, the name of the member of the record or
    /// subrecord structure above it.
    pub fn name(&self, parent: Option<&ViewNode>) -> String {
        let use_suffix = self.datas.len() == 1;
        if let Some(element) = self.datas.iter().find_map(|data| data.element.as_ref()) {
            let mut text = element.get_display_name(use_suffix);
            if self.datas[0].flags.contains(view_node_flag::IS_SORTED) {
                text.push_str(" (sorted)");
            } else if self.datas[0].flags.contains(view_node_flag::IS_ALIGNED) {
                text.push_str(" (aligned)");
            }
            return text;
        }
        let Some(parent) = parent else {
            return String::new();
        };
        let Some(container) = parent.datas.iter().find_map(|data| data.container.as_ref()) else {
            return String::new();
        };
        let element = &container.element;
        if !matches!(
            element.get_element_type(),
            ElementType::etMainRecord | ElementType::etSubRecordStruct
        ) {
            return String::new();
        }
        let additional = container.additional_element_count();
        let member = self.index as i32 - additional;
        if let Some(def) = element.get_def()
            && let Some(record_def) = def.as_record_def()
            && self.index as i32 >= additional
            && member < record_def.get_member_count()
        {
            let member_def = record_def.get_member(member as usize);
            if member_def.get_def_type() == DefType::dtSubRecord {
                return format!(
                    "{} - {}",
                    displayable(member_def.get_default_signature()),
                    member_def.get_name()
                );
            }
            return member_def.get_name().to_owned();
        }
        element.get_name()
    }

    /// Port of `vstViewGetText` for the cell of `column`: the value of the
    /// element (empty for an ignored one under `wbHideIgnored`), or its
    /// summary when it has no value.
    pub fn cell_text(&self, column: usize) -> String {
        let Some(element) = &self.datas[column].element else {
            return String::new();
        };
        if element.get_conflict_priority() == ConflictPriority::cpIgnore && hide_ignored() {
            return String::new();
        }
        let value = element.get_value();
        if value.is_empty() { element.get_summary() } else { value }
    }
}

/// Port of `Displayable`: the control characters of a signature as letters.
fn displayable(signature: Signature) -> String {
    let mut bytes = signature.0;
    for byte in &mut bytes {
        if *byte < 32 {
            *byte += b'a';
        }
    }
    Signature(bytes).to_string()
}

fn is_exe(file_name: &str) -> bool {
    file_name.to_ascii_lowercase().ends_with(".exe")
}

/// Port of `TwbMainRecord.GetMasterAndLeafs`: the master and the overrides
/// that no later override has as a master.
fn master_and_leafs(master: &Arc<MainRecordImpl>) -> Vec<Arc<MainRecordImpl>> {
    let overrides = master.overrides();
    let mut result = vec![master_or_self(master)];
    match overrides.len() {
        0 => {}
        1 => result.push(overrides[0].clone()),
        _ => {
            let mut masters: HashSet<String> = HashSet::new();
            for record in &overrides {
                if let Some(file) = record.get_file() {
                    for index in (0..file.get_master_count(true)).rev() {
                        if let Some(master_file) = file.get_master(index, true) {
                            masters.insert(master_file.get_name().to_lowercase());
                        }
                    }
                }
            }
            for record in overrides {
                let name = record.get_file().map(|file| file.get_name()).unwrap_or_default();
                // UPSTREAM-QUIRK: `TStringList.IndexOf` on a list that is not
                // case sensitive.
                if !masters.contains(&name.to_lowercase()) {
                    result.push(record);
                }
            }
        }
    }
    result
}

/// Port of `IsCompareToSame` in `ConflictLevelForMainRecord`: the override
/// in a compare-to file with the same masters is unchanged against its
/// master. The port loads no compare-to files yet
/// (`fsCompareToHasSameMasters`), so this is never the case.
fn is_compare_to_same(_node_datas: &[ViewNodeData]) -> bool {
    // Upstream needs the override's file loaded as a compare-to file with
    // the same masters (`fsCompareToHasSameMasters`), both records
    // unmodified, the master in the file compared to, and their data equal
    // (`ContentEquals`). The port loads no compare-to files yet.
    false
}

/// Port of `TfrmMain.InitNodes`: the cells of the row `index` below the
/// cells `parent_datas`. Returns whether no record has an element in the
/// row and whether the row has rows below it.
pub fn init_nodes(node_datas: &mut [ViewNodeData], parent_datas: &[ViewNodeData], index: i32) -> InitialStates {
    for (node_data, parent) in node_datas.iter_mut().zip(parent_datas) {
        if let Some(container) = &parent.container {
            let by_sort_order = parent.flags.contains(view_node_flag::USE_SORT_ORDER)
                || container.sortable().is_some_and(|sortable| sortable.sorted);
            node_data.element = if by_sort_order {
                container.element_by_sort_order(index).cloned()
            } else {
                match container.element_type() {
                    ElementType::etMainRecord | ElementType::etSubRecordStruct => {
                        container.element_by_sort_order(index).cloned()
                    }
                    ElementType::etSubRecordArray
                    | ElementType::etArray
                    | ElementType::etStruct
                    | ElementType::etSubRecord
                    | ElementType::etValue
                    | ElementType::etUnion
                    | ElementType::etStructChapter => usize::try_from(index)
                        .ok()
                        .and_then(|index| container.elements().get(index).cloned()),
                    _ => None,
                }
            };
        }
        if let Some(element) = &node_data.element
            && element.get_dont_show()
        {
            node_data.element = None;
            node_data.flags.include(view_node_flag::DONT_SHOW);
        }
        if node_data.element.is_none()
            && let Some(container) = &parent.container
            && container.element_type() == ElementType::etMainRecord
            && element_as_record(&container.element).is_some_and(|record| record.get_is_partial_form())
        {
            node_data.flags.include(view_node_flag::IGNORE);
            node_data.flags.include(view_node_flag::IS_PARTIAL_FORM);
        }
    }

    let mut states = InitialStates {
        disabled: true,
        has_children: false,
    };
    for (node_data, parent) in node_datas.iter_mut().zip(parent_datas) {
        if node_data.element.is_some() {
            states.disabled = false;
        } else {
            if parent.flags.contains(view_node_flag::IGNORE)
                || parent.element.as_ref().is_some_and(|element| {
                    matches!(
                        element.get_conflict_priority(),
                        ConflictPriority::cpIgnore | ConflictPriority::cpNormalIgnoreEmpty
                    )
                })
            {
                node_data.flags.include(view_node_flag::IGNORE);
            }
            if parent.flags.contains(view_node_flag::IS_PARTIAL_FORM) {
                node_data.flags.include(view_node_flag::IS_PARTIAL_FORM);
            }
        }
        if node_data.container.is_none()
            && let Some(element) = &node_data.element
            && is_container(element)
        {
            node_data.container = Some(ViewContainer::new(element.clone()));
        }
        if let Some(container) = &node_data.container
            && (container.element_count() > 0 || container.element_type() == ElementType::etSubRecordStruct)
        {
            states.has_children = true;
        }
    }
    states
}

/// `Supports(Element, IwbContainerElementRef)`: every element but a flag
/// and the terminator of a string list is a container.
fn is_container(element: &ElementRef) -> bool {
    element.as_container().is_some()
        && !matches!(
            element.get_element_type(),
            ElementType::etFlag | ElementType::etStringListTerminator
        )
}

/// Port of `TfrmMain.ConflictLevelForNodeDatas`: the `TConflictAll` of one
/// row and the `TConflictThis` of each of its cells.
pub fn conflict_level_for_node_datas(
    node_datas: &mut [ViewNodeData],
    sibling_compare: bool,
    injected: bool,
) -> ConflictAll {
    let node_count = node_datas.len();
    let mut found_any = false;
    let mut master_position = 0;
    let mut overall_conflict_this = ConflictThis::ctUnknown;

    let mut active_count = 0;
    let mut first_node: Option<usize> = None;
    if node_count == 1 {
        active_count = 1;
        first_node = Some(0);
    } else {
        for (index, node_data) in node_datas.iter_mut().enumerate() {
            if node_data.flags.any(view_node_flag::DONT_SHOW | view_node_flag::IGNORE) {
                node_data.conflict_this = ConflictThis::ctNotDefined;
                if node_data.element.is_some() && node_data.flags.contains(view_node_flag::IGNORE) {
                    node_data.conflict_this = ConflictThis::ctIgnored;
                }
            } else {
                active_count += 1;
                if first_node.is_none() {
                    first_node = Some(index);
                }
            }
        }
    }

    match active_count {
        0 => return ConflictAll::caUnknown,
        1 => {
            let first = first_node.expect("one active node");
            let node_data = &mut node_datas[first];
            node_data.conflict_this = match &node_data.element {
                Some(element) if element.get_conflict_priority() == ConflictPriority::cpIgnore => {
                    ConflictThis::ctIgnored
                }
                Some(_) => ConflictThis::ctOnlyOne,
                None => ConflictThis::ctNotDefined,
            };
            return ConflictAll::caOnlyOne;
        }
        _ => {}
    }

    let first_node = first_node.expect("an active node");
    let mut last_index = node_count - 1;
    let mut last_element = node_datas[last_index].element.clone();
    while last_element.is_none()
        && node_datas[last_index].flags.contains(view_node_flag::IS_PARTIAL_FORM)
        && last_index > 0
    {
        last_index -= 1;
        last_element = node_datas[last_index].element.clone();
    }
    let mut first_element = node_datas[first_node].element.clone();

    let mut unique_values: HashSet<String> = HashSet::new();
    let mut priority = ConflictPriority::cpNormal;
    for i in 0..node_count {
        let Some(element) = node_datas[i].element.clone() else {
            continue;
        };
        found_any = true;
        priority = element.get_conflict_priority();
        if priority == ConflictPriority::cpNormalIgnoreEmpty {
            first_element = Some(element.clone());
            master_position = i;
            for j in (i..node_count).rev() {
                last_element = node_datas[j].element.clone();
                if last_element.is_some() {
                    break;
                }
            }
        }
        if sortable::conflict_priority_can_change(&element) {
            for node_data in &node_datas[i + 1..] {
                if let Some(element) = &node_data.element {
                    let this_priority = element.get_conflict_priority();
                    if this_priority > priority {
                        priority = this_priority;
                    }
                }
            }
        }
        break;
    }

    if sibling_compare && priority > ConflictPriority::cpBenign {
        priority = ConflictPriority::cpBenign;
    }
    if injected && priority >= ConflictPriority::cpNormal {
        priority = ConflictPriority::cpCritical;
    }

    let first_element_not_ignored = if priority > ConflictPriority::cpIgnore
        && first_element
            .as_ref()
            .is_none_or(|element| element.get_conflict_priority() == ConflictPriority::cpIgnore)
    {
        None
    } else {
        first_element.clone()
    };

    let mut element_types: HashSet<ElementType> = HashSet::new();
    let mut def_types: HashSet<DefType> = HashSet::new();
    let mut optional_and_missing = false;
    // `DisplaySortKey[True]` of each element, read once.
    let keys: Vec<Option<String>> = node_datas
        .iter()
        .map(|node_data| {
            node_data
                .element
                .as_ref()
                .map(|element| sortable::display_sort_key(element, true))
        })
        .collect();
    let last_key = last_element
        .as_ref()
        .map(|element| sortable::display_sort_key(element, true));
    let first_not_ignored_key = first_element_not_ignored
        .as_ref()
        .map(|element| sortable::display_sort_key(element, true));

    for i in 0..node_count {
        let element = node_datas[i].element.clone();
        let mut this_priority;
        if let Some(element) = &element {
            element_types.insert(element.get_element_type());
            match element.get_value_def() {
                Some(value_def) => def_types.insert(value_def.get_def_type()),
                None => def_types.insert(DefType::dtEmpty),
            };
            optional_and_missing = optional_and_missing || sortable::is_optional_and_missing(element);
            this_priority = element.get_conflict_priority();
            if this_priority != ConflictPriority::cpIgnore {
                unique_values.insert(keys[i].clone().unwrap_or_default());
            }
        } else if node_datas[i].flags.contains(view_node_flag::IS_PARTIAL_FORM) {
            this_priority = ConflictPriority::cpIgnore;
        } else {
            def_types.insert(DefType::dtEmpty);
            this_priority = priority;
            if !node_datas[i].flags.contains(view_node_flag::IGNORE)
                && priority != ConflictPriority::cpNormalIgnoreEmpty
            {
                unique_values.insert(String::new());
            }
        }

        let conflict_this = if (this_priority == ConflictPriority::cpNormalIgnoreEmpty && element.is_none())
            || this_priority == ConflictPriority::cpIgnore
        {
            ConflictThis::ctIgnored
        } else if sibling_compare {
            ConflictThis::ctOnlyOne
        } else if i == master_position {
            if element.is_some() {
                ConflictThis::ctMaster
            } else {
                ConflictThis::ctUnknown
            }
        } else {
            let same_as_last = i == node_count - 1
                || !((element.is_some() != last_element.is_some())
                    || (element.is_some() && keys[i].as_deref() != last_key.as_deref()));
            let mut same_as_first = !((element.is_some() != first_element_not_ignored.is_some())
                || (element.is_some() && keys[i].as_deref() != first_not_ignored_key.as_deref()));
            if !same_as_first
                && this_priority == ConflictPriority::cpBenignIfAdded
                && same_as_last
                && first_element_not_ignored.is_none()
            {
                this_priority = ConflictPriority::cpBenign;
                priority = ConflictPriority::cpBenign;
                same_as_first = true;
            }
            if same_as_first {
                ConflictThis::ctIdenticalToMaster
            } else if same_as_last {
                ConflictThis::ctConflictWins
            } else {
                ConflictThis::ctConflictLoses
            }
        };
        node_datas[i].conflict_this = conflict_this;

        if this_priority == ConflictPriority::cpBenign && node_datas[i].conflict_this > ConflictThis::ctConflictBenign {
            node_datas[i].conflict_this = ConflictThis::ctConflictBenign;
        }
        if this_priority == ConflictPriority::cpOverride && node_datas[i].conflict_this > ConflictThis::ctOverride {
            node_datas[i].conflict_this = ConflictThis::ctOverride;
        }
        if node_datas[i].conflict_this > overall_conflict_this {
            overall_conflict_this = node_datas[i].conflict_this;
        }
    }

    let mut result = match unique_values.len() {
        0 | 1 => ConflictAll::caNoConflict,
        2 => {
            let element = node_datas[0].element.clone();
            let mut compare_index = node_count - 1;
            let mut compare_element = node_datas[compare_index].element.clone();
            while compare_element.is_none()
                && node_datas[compare_index]
                    .flags
                    .contains(view_node_flag::IS_PARTIAL_FORM)
                && compare_index > 0
            {
                compare_index -= 1;
                compare_element = node_datas[compare_index].element.clone();
            }
            if (element.is_some() != compare_element.is_some())
                || (element.is_some() && keys[0].as_deref() != keys[compare_index].as_deref())
                || (unique_values.contains("")
                    && compare_element.is_some()
                    && keys[compare_index].as_deref() != Some(""))
            {
                ConflictAll::caOverride
            } else {
                ConflictAll::caConflict
            }
        }
        _ => ConflictAll::caConflict,
    };

    if sibling_compare && result > ConflictAll::caConflictBenign {
        result = ConflictAll::caConflictBenign;
    }

    if !found_any {
        for node_data in node_datas.iter_mut() {
            node_data.conflict_this = ConflictThis::ctNotDefined;
        }
    }

    if result > ConflictAll::caNoConflict {
        match priority {
            ConflictPriority::cpBenign => result = ConflictAll::caConflictBenign,
            ConflictPriority::cpOverride => result = ConflictAll::caOverride,
            ConflictPriority::cpCritical => {
                unique_values.remove("");
                if unique_values.len() > 1 {
                    result = ConflictAll::caConflictCritical;
                }
            }
            _ => {}
        }
    }

    if priority > ConflictPriority::cpBenign && overall_conflict_this > ConflictThis::ctOverride {
        let last = &mut node_datas[node_count - 1];
        if last.conflict_this < ConflictThis::ctOverride {
            last.conflict_this = if last.conflict_this == ConflictThis::ctIdenticalToMaster {
                ConflictThis::ctIdenticalToMasterWinsConflict
            } else {
                ConflictThis::ctConflictWins
            };
        }
    }

    if matches!(
        result,
        ConflictAll::caNoConflict | ConflictAll::caOverride | ConflictAll::caConflict
    ) {
        for (i, node_data) in node_datas.iter_mut().enumerate() {
            match node_data.conflict_this {
                ConflictThis::ctIdenticalToMaster => {
                    if matches!(result, ConflictAll::caOverride | ConflictAll::caConflict) && i == node_count - 1 {
                        node_data.conflict_this = ConflictThis::ctIdenticalToMasterWinsConflict;
                    }
                }
                ConflictThis::ctConflictWins => match result {
                    ConflictAll::caNoConflict => node_data.conflict_this = ConflictThis::ctIdenticalToMaster,
                    ConflictAll::caOverride => node_data.conflict_this = ConflictThis::ctOverride,
                    _ => {}
                },
                _ => {}
            }
        }
    }

    if result < ConflictAll::caConflict
        && node_datas
            .iter()
            .any(|node_data| node_data.conflict_this >= ConflictThis::ctIdenticalToMasterWinsConflict)
    {
        result = ConflictAll::caConflict;
    }

    if result > ConflictAll::caNoConflict
        && optional_and_missing
        && element_types
            .iter()
            .all(|t| matches!(t, ElementType::etArray | ElementType::etStruct | ElementType::etValue))
        && def_types.contains(&DefType::dtEmpty)
    {
        let others: Vec<DefType> = def_types.iter().copied().filter(|t| *t != DefType::dtEmpty).collect();
        let simple = |t: &DefType| {
            (DefType::dtString..=DefType::dtInteger).contains(t)
                || matches!(t, DefType::dtFloat | DefType::dtArray | DefType::dtStruct)
        };
        if others.len() == 1 && others.iter().all(simple) {
            for node_data in node_datas.iter() {
                if let Some(element) = &node_data.element
                    && !sortable::content_is_all_zero(element)
                {
                    return result;
                }
            }
            result = ConflictAll::caNoConflict;
            for node_data in node_datas.iter_mut() {
                if node_data.conflict_this > ConflictThis::ctIdenticalToMaster {
                    node_data.conflict_this = ConflictThis::ctIdenticalToMaster;
                }
                if node_data.conflict_all > ConflictAll::caNoConflict {
                    node_data.conflict_all = ConflictAll::caNoConflict;
                }
            }
        }
    }

    result
}
