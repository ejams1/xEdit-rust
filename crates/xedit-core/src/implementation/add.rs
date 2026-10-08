// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of the records and groups made by an edit: `TwbMainRecord.Create`
//! with a signature and FormID, the `TwbGroupRecord.Create` overloads that
//! make a group, `Add` of the file, the groups and the main records (the
//! child groups of a `CELL`, `DIAL`, `WRLD` and `QUST` record), `Assign` and
//! `CanAssign` of a main record, `SetEditorID`, `Remove` of a main record
//! with its overrides, and the overrides moved by `YouGotAMaster` and
//! `YouAreTheMaster`.
//!
//! State: the move of a record between the child groups of a cell
//! (`UpdateCellChildGroup`), `CollapseStorage` and the reference index
//! (`BuildRef`) are not ported; the editor ID index of a file does not
//! learn the records an edit adds.

use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock, Weak};

use crate::interface::def::Def;
use crate::interface::element::{Container, Element, ElementRef, File, MainRecord};
use crate::interface::form_id::FormID;
use crate::interface::globals::{
    GameMode, cell_size_factor, edit_allowed, fill_inoa, fill_inom, game_mode, is_internal_edit,
    size_of_main_record_struct, sort_sub_records, vwd_as_quest_children, wb_get_group_order,
};
use crate::interface::misc::{EditError, Variant};
use crate::interface::sub_record_group::RecordDef;
use crate::interface::types::{ASSIGN_ADD, ASSIGN_THIS, DefFlag, DefType, KnownSubRecord, PascalEnum, Signature};

use super::structs::{GroupRecordStruct, MainRecordStruct, MainRecordStructFlags};
use super::{
    ContainerBase, DataStorage, ElementBase, ElementImpl, FileImpl, GroupRecordImpl, InitOnce, MainRecordImpl,
};

/// The signatures of the placed records of a cell.
const PLACED: [&str; 14] = [
    "NAVM", "PGRD", "LAND", "REFR", "PGRE", "PMIS", "ACRE", "ACHR", "PARW", "PBEA", "PFLA", "PCON", "PBAR", "PHZD",
];

/// The placed records `SetPosition` moves.
const POSITIONED: [&[u8; 4]; 11] = [
    b"REFR", b"ACRE", b"ACHR", b"PGRE", b"PMIS", b"PARW", b"PBEA", b"PFLA", b"PCON", b"PBAR", b"PHZD",
];

/// Port of `wbSubBlockFromGridCell`.
pub fn sub_block_from_grid_cell(x: i32, y: i32) -> (i32, i32) {
    let part = |value: i32| {
        let mut result = value / 8;
        if value < 0 && value % 8 != 0 {
            result -= 1;
        }
        result
    };
    (part(x), part(y))
}

/// Port of `wbBlockFromSubBlock`.
pub fn block_from_sub_block(x: i32, y: i32) -> (i32, i32) {
    let part = |value: i32| {
        let mut result = value / 4;
        if value < 0 && value % 4 != 0 {
            result -= 1;
        }
        result
    };
    (part(x), part(y))
}

/// Port of `wbGridCellToGroupLabel`.
pub fn grid_cell_to_group_label(x: i32, y: i32) -> u32 {
    u32::from(y as i16 as u16) | (u32::from(x as i16 as u16) << 16)
}

/// Port of `wbGridCellToCenterPosition`.
pub fn grid_cell_to_center_position(x: i32, y: i32) -> (f64, f64, f64) {
    let factor = cell_size_factor();
    let center = |value: i32| {
        if value >= 0 {
            f64::from(value + 1) * factor - factor / 2.0
        } else {
            f64::from(value) * factor + factor / 2.0
        }
    };
    (center(x), center(y), 0.0)
}

// ----- the groups -----

impl GroupRecordImpl {
    /// The fields of a group made by an edit: no data in the file, the
    /// header in `gr_struct`.
    fn new_in(container: &ElementRef, file: &Arc<FileImpl>, group_type: i32, label: u32) -> Arc<Self> {
        let group = Arc::new_cyclic(|self_ref: &Weak<GroupRecordImpl>| GroupRecordImpl {
            self_ref: self_ref.clone(),
            base: ElementBase::new(Some(container)),
            container: ContainerBase::default(),
            file: Arc::downgrade(file),
            gr_struct: RwLock::new(GroupRecordStruct {
                group_size: size_of_main_record_struct() as u32,
                label,
                group_type,
                stamp: 0,
                unknown: 0,
            }),
            dc_base: 0,
            dc_end: 0,
            gr_sorted: std::sync::atomic::AtomicBool::new(false),
            gr_sorting: std::sync::atomic::AtomicBool::new(false),
        });
        if group_type == 0 {
            let order = wb_get_group_order(Signature::new(&label.to_le_bytes()));
            group.base.e_sort_order.store(order, Ordering::Relaxed);
            group.base.e_memory_order.store(order, Ordering::Relaxed);
        }
        if let Some(parent) = container.as_element_impl().and_then(ElementImpl::container_base) {
            parent.add_element(group.clone());
        }
        group
    }

    /// Port of `TwbGroupRecord.Create(aContainer, aSignature)`: a top level
    /// group, put into the order of the groups of the file.
    pub(crate) fn create_top(file: &Arc<FileImpl>, signature: Signature) -> Arc<Self> {
        let container: ElementRef = file.clone();
        let group = GroupRecordImpl::new_in(&container, file, 0, signature.to_int());
        group.set_modified(true);
        group.invalidate_storage();
        file.sort_groups();
        group
    }

    /// Port of `TwbGroupRecord.Create(aContainer, aType, aMainRecord)`: the
    /// group of the children of a record, in the group of the record (types
    /// 1, 6, 7 and the quest children) or in the cell children group (8, 9,
    /// 10).
    pub(crate) fn create_child(
        parent: &Arc<GroupRecordImpl>,
        group_type: i32,
        record: &Arc<MainRecordImpl>,
    ) -> Arc<Self> {
        let Some(file) = parent.file.upgrade() else {
            return GroupRecordImpl::new_in(
                &(parent.clone() as ElementRef),
                &record.file_impl().expect("a file"),
                group_type,
                record.mr_struct().form_id.to_cardinal(),
            );
        };
        let container: ElementRef = parent.clone();
        let group = GroupRecordImpl::new_in(&container, &file, group_type, record.mr_struct().form_id.to_cardinal());
        group.base.include_state(super::ElementState::esUnsaved);
        group.set_modified(true);
        group.invalidate_storage();
        parent.resort();
        group
    }

    /// Port of `TwbGroupRecord.Create(aContainer, aType, aLabel)`: a block or
    /// sub-block group (types 2 to 5).
    pub(crate) fn create_labelled(parent: &Arc<GroupRecordImpl>, group_type: i32, label: u32) -> Arc<Self> {
        let file = parent.file.upgrade().expect("a group is in a loaded file");
        let container: ElementRef = parent.clone();
        let group = GroupRecordImpl::new_in(&container, &file, group_type, label);
        group.set_modified(true);
        group.invalidate_storage();
        parent.resort();
        group
    }

    /// `Exclude(grStates, gsSorted); Sort`: the members changed, so the
    /// group sorts again.
    pub(crate) fn resort(&self) {
        self.gr_sorted.store(false, Ordering::Relaxed);
        self.sort();
    }

    /// Port of `FindChildGroup(aType, aLabel)`.
    pub(crate) fn find_child_group(&self, group_type: i32, label: u32) -> Option<Arc<GroupRecordImpl>> {
        self.container.elements().iter().find_map(|element| {
            let group = element.as_element_impl()?.group_record_impl()?;
            (group.group_type() == group_type && group.get_group_label() == label).then_some(group)
        })
    }

    /// Port of `ElementBySignature` on a group: the first record with the
    /// signature.
    fn record_by_signature(&self, signature: Signature) -> Option<Arc<MainRecordImpl>> {
        self.container.elements().iter().find_map(|element| {
            let record = element.as_element_impl()?.main_record_impl()?;
            (record.get_signature() == signature).then_some(record)
        })
    }

    /// Port of `TwbGroupRecord.Add`: a new record (or the copy of the one a
    /// master has, for `ROAD`, `LAND`, `PGRD` and a worldspace cell) with
    /// the signature of `name`, in this group or the child group it
    /// belongs to. A worldspace cell is named `CELL[P]` (persistent) or
    /// `CELL[x,y]`.
    pub(crate) fn add_impl_group(self: &Arc<Self>, name: &str, silent: bool) -> Result<Option<ElementRef>, EditError> {
        if name.chars().count() < 4 {
            return Ok(None);
        }
        let signature = Signature::from_str(&name.chars().take(4).collect::<String>())
            .map_err(|_| format!("{name} is not a signature"))?;
        let is = |expected: &[u8; 4]| signature == Signature::new(expected);
        let placed_in = |list: &[&[u8; 4]]| list.iter().any(|expected| is(expected));
        let group_type = self.group_type();
        match group_type {
            0 => {
                if signature != self.gr_struct().label_signature() {
                    return Ok(None);
                }
            }
            1 => {
                if !is(b"ROAD") && !is(b"CELL") {
                    return Ok(None);
                }
            }
            7 => {
                if !is(b"INFO") {
                    return Ok(None);
                }
            }
            6 => {
                let Some(cell) = self.children_of() else {
                    return Ok(None);
                };
                let child_type = if cell.get_is_persistent() { 8 } else { 9 };
                let group = match self.find_child_group(child_type, self.group_label()) {
                    Some(group) => group,
                    None => GroupRecordImpl::create_child(self, child_type, &cell),
                };
                return group.add_impl_group(name, silent);
            }
            8 => {
                if !placed_in(&[
                    b"REFR", b"ACRE", b"PGRE", b"PMIS", b"PARW", b"PBEA", b"PFLA", b"PCON", b"PBAR", b"PHZD", b"ACHR",
                ]) {
                    return Ok(None);
                }
            }
            9 => {
                if !placed_in(&[
                    b"LAND", b"PGRD", b"NAVM", b"REFR", b"PGRE", b"PMIS", b"PARW", b"PBEA", b"PFLA", b"PCON", b"PBAR",
                    b"PHZD", b"ACRE", b"ACHR",
                ]) {
                    return Ok(None);
                }
            }
            10 => {
                let allowed = if vwd_as_quest_children() {
                    placed_in(&[b"REFR", b"DLBR", b"DIAL", b"SCEN"])
                } else {
                    is(b"REFR")
                };
                if !allowed {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        }
        let Some(file) = self.file.upgrade() else {
            return Ok(None);
        };
        // A ROAD, LAND or PGRD that a master has is copied as an override.
        let single_child = |parent_signature: &[u8; 4], required_type: i32| -> Result<Option<ElementRef>, EditError> {
            if group_type != required_type {
                return Err(format!(
                    "{signature} can only be added to groups of type {required_type}"
                ));
            }
            let parent = self
                .children_of()
                .ok_or_else(|| "Can't find MainRecord for group".to_owned())?;
            if parent.get_signature() != Signature::new(parent_signature) {
                return Err(format!(
                    "Expected {} record, but found: {}",
                    Signature::new(parent_signature),
                    parent.get_signature()
                ));
            }
            for visible in parent.all_visible_for_file(&file) {
                if let Some(child) = visible.child_by_signature(signature) {
                    if child
                        .file_impl()
                        .is_some_and(|child_file| Arc::ptr_eq(&child_file, &file))
                    {
                        return Ok(Some(child as ElementRef));
                    }
                    let child: ElementRef = child;
                    let args = crate::interface::element::CopyArgs {
                        deep_copy: true,
                        ..Default::default()
                    };
                    return super::copy::copy_element_to_file(&child, &file, &args);
                }
            }
            Ok(None)
        };
        if is(b"ROAD")
            && let Some(found) = single_child(b"WRLD", 1)?
        {
            return Ok(Some(found));
        }
        if (is(b"LAND") || is(b"PGRD"))
            && let Some(found) = single_child(b"CELL", 9)?
        {
            return Ok(Some(found));
        }
        let is_world_cell = is(b"CELL") && group_type == 1;
        let mut persistent = false;
        let mut grid_cell = (0, 0);
        if is_world_cell {
            let params = name.chars().skip(4).collect::<String>();
            let params = params.trim();
            match params.strip_prefix('[').and_then(|params| params.strip_suffix(']')) {
                Some(params) => {
                    let parts: Vec<&str> = params.split(',').map(str::trim).collect();
                    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("P") {
                        persistent = true;
                    } else if parts.len() == 2 {
                        let parse = |text: &str| {
                            text.parse::<i32>()
                                .map_err(|_| format!("'{text}' is not a valid integer value"))
                        };
                        grid_cell = (parse(parts[0])?, parse(parts[1])?);
                    } else {
                        return Err(format!("Invalid Parameters: {name}"));
                    }
                }
                None => {
                    // The editor asks for the cell here
                    // (`wbGetCellDetailsForWorldspace`); without a user the
                    // add needs the parameters, as a silent add does.
                    return Err("To add a Worldspace CELL silently, parameters must be specified: CELL[P] for persistent world cell or CELL[x,y] for temporary cell".to_owned());
                }
            }
            let world = self
                .children_of()
                .ok_or_else(|| "Can't find MainRecord for group".to_owned())?;
            if world.get_signature() != Signature::new(b"WRLD") {
                return Err(format!("Expected WRLD record, but found: {}", world.get_signature()));
            }
            for visible in world.all_visible_for_file(&file) {
                let found = if persistent {
                    visible.child_by_signature(Signature::new(b"CELL"))
                } else {
                    visible.child_by_grid_cell(grid_cell)
                };
                if let Some(cell) = found {
                    if cell.file_impl().is_some_and(|cell_file| Arc::ptr_eq(&cell_file, &file)) {
                        return Ok(Some(cell as ElementRef));
                    }
                    let cell: ElementRef = cell;
                    let args = crate::interface::element::CopyArgs {
                        deep_copy: true,
                        ..Default::default()
                    };
                    return super::copy::copy_element_to_file(&cell, &file, &args);
                }
            }
        }
        // UPSTREAM-QUIRK: a non-silent add asks for the FormID
        // (`wbGetFormID`); without a user every add takes a new FormID.
        let _ = silent;
        let form_id = if is(b"PLYR") {
            FormID::from_cardinal(0x14)
        } else {
            file.new_form_id()?
        };
        if form_id.is_null() {
            return Ok(None);
        }
        let mut injected_master = None;
        if let Some(existing) = file.record_by_form_id(form_id, true, true) {
            if existing
                .file_impl()
                .is_some_and(|existing_file| Arc::ptr_eq(&existing_file, &file))
            {
                return Err(format!(
                    "FormID [{}] is already defined in file \"{}\"",
                    form_id.to_string(true),
                    file.get_name()
                ));
            }
            if existing.get_signature() != signature {
                return Err(format!(
                    "Existing record {} has different signature",
                    existing.get_name()
                ));
            }
            if file.is_new_record(form_id.file_id()) {
                injected_master = Some(existing);
            }
        }
        let mut group = self.clone();
        if is_world_cell && !persistent {
            let sub_block = sub_block_from_grid_cell(grid_cell.0, grid_cell.1);
            let block = block_from_sub_block(sub_block.0, sub_block.1);
            let label = grid_cell_to_group_label(block.0, block.1);
            group = match group.find_child_group(4, label) {
                Some(found) => found,
                None => GroupRecordImpl::create_labelled(&group, 4, label),
            };
            let label = grid_cell_to_group_label(sub_block.0, sub_block.1);
            group = match group.find_child_group(5, label) {
                Some(found) => found,
                None => GroupRecordImpl::create_labelled(&group, 5, label),
            };
        }
        let record = MainRecordImpl::create_new(&group, signature, form_id)?;
        if let Some(master) = injected_master {
            master.you_got_a_master(&record);
        }
        if is_world_cell {
            record.begin_update();
            let result = (|| {
                record.set_grid_cell(grid_cell)?;
                if persistent {
                    record.set_is_persistent(true);
                }
                Ok::<(), EditError>(())
            })();
            record.end_update();
            result?;
        }
        Ok(Some(record as ElementRef))
    }
}

// ----- the file -----

impl FileImpl {
    /// Port of `wbMergeSortPtr(@cntElements[1], ..., CompareSortOrder)`: the
    /// groups after the file header in the order of the groups.
    pub(crate) fn sort_groups(&self) {
        let mut elements = self.container.release_elements();
        if elements.len() > 1 {
            elements[1..].sort_by_key(|element| element.get_sort_order());
        }
        for element in elements {
            self.container.add_element(element);
        }
    }

    /// Port of `TwbFile.Add`: the top level group with the signature of
    /// `name`, made when the file lacks it.
    pub(crate) fn add_impl_file(self: &Arc<Self>, name: &str) -> Result<Option<ElementRef>, EditError> {
        if !self.is_element_editable() {
            return Err(format!("File \"{}\" is not editable", self.get_name()));
        }
        if name.chars().count() < 4 {
            return Ok(None);
        }
        let Ok(signature) = Signature::from_str(&name.chars().take(4).collect::<String>()) else {
            return Ok(None);
        };
        if let Some(group) = self.group_by_signature(signature) {
            return Ok(Some(group as ElementRef));
        }
        if wb_get_group_order(signature) < 0 {
            return Ok(None);
        }
        Ok(Some(GroupRecordImpl::create_top(self, signature) as ElementRef))
    }

    /// Port of `TwbFile.RemoveMainRecord`: the record leaves the records of
    /// the file, and the injected records of the file its FormID belongs to.
    pub(crate) fn remove_main_record(self: &Arc<Self>, record: &Arc<MainRecordImpl>) -> Result<(), EditError> {
        let form_id = record.get_fixed_form_id();
        if form_id.is_null() {
            return Ok(());
        }
        {
            let mut records = self.fl_records.write().unwrap();
            let position = records
                .iter()
                .position(|other| Arc::ptr_eq(other, record))
                .ok_or_else(|| {
                    format!(
                        "Can't remove FormID [{}] from file {}: FormID not registered",
                        form_id.to_string(true),
                        self.get_name()
                    )
                })?;
            records.remove(position);
        }
        let mut hardcoded = form_id.object_id() < 0x800;
        if hardcoded && self.get_allow_hardcoded_range_use() {
            hardcoded = form_id.is_hardcoded();
        }
        if hardcoded && self.get_load_order_file_id().full_slot() == 0 {
            hardcoded = false;
        }
        if !hardcoded && self.is_new_record(form_id.file_id()) {
            self.remove_index_keys(record);
            return Ok(());
        }
        let master = self.get_master_record_by_form_id(form_id, true, true);
        match master {
            Some(master) if !Arc::ptr_eq(&master, record) => master.remove_override(record),
            _ => {
                if let Some(master_file) = self.get_master_for_file_id(form_id.file_id()) {
                    master_file.remove_injected_main_record(record);
                }
            }
        }
        self.remove_index_keys(record);
        Ok(())
    }

    /// The `flRemoveKeysFromIndices(aRecord, aRecord.DeactivateIndexKeys)`
    /// at the end of `RemoveMainRecord`.
    fn remove_index_keys(&self, record: &Arc<MainRecordImpl>) {
        if self.fl_indices_active.load(Ordering::Acquire) {
            let keys = record.activate_index_keys();
            self.remove_keys_from_indices(record, &keys);
        }
    }

    /// Port of `RemoveInjectedMainRecord`.
    fn remove_injected_main_record(&self, record: &Arc<MainRecordImpl>) {
        self.fl_injected_records
            .write()
            .unwrap()
            .retain(|other| !Arc::ptr_eq(other, record));
    }
}

// ----- the main records -----

impl MainRecordImpl {
    /// Port of `TwbMainRecord.Create(aContainer, aSignature, aFormID)`: a new
    /// record in the group, with its required members. A persistent or
    /// visible-when-distant group sets the flag of the record, and an
    /// interior cell goes into the block and sub-block of its FormID.
    pub(crate) fn create_new(
        group: &Arc<GroupRecordImpl>,
        signature: Signature,
        form_id: FormID,
    ) -> Result<Arc<MainRecordImpl>, EditError> {
        let file = group
            .file
            .upgrade()
            .ok_or_else(|| "the group is not in a loaded file".to_owned())?;
        let mut flags = MainRecordStructFlags::default();
        if group.group_type() == 8 {
            flags.set_persistent(true);
        } else if group.group_type() == 10 {
            let quest_children = vwd_as_quest_children()
                && group
                    .parent_group()
                    .is_some_and(|parent| parent.gr_struct().label_signature() == Signature::new(b"QUST"));
            if !quest_children {
                flags.set_visible_when_distant(true);
            }
        }
        let mut container = group.clone();
        let mut is_interior = false;
        if signature == Signature::new(b"CELL") {
            let mut top = Some(group.clone());
            if let Some(current) = &top
                && current.group_type() == 3
            {
                top = current.parent_group();
            }
            if let Some(current) = &top
                && current.group_type() == 2
            {
                top = current.parent_group();
            }
            if let Some(top) = top
                && top.group_type() == 0
                && top.gr_struct().label_signature() == Signature::new(b"CELL")
            {
                let digits = format!("00{}", form_id.object_id());
                let digits = digits.as_bytes();
                let block = u32::from(digits[digits.len() - 1] - b'0');
                let sub_block = u32::from(digits[digits.len() - 2] - b'0');
                let block_group = match top.find_child_group(2, block) {
                    Some(found) => found,
                    None => GroupRecordImpl::create_labelled(&top, 2, block),
                };
                container = match block_group.find_child_group(3, sub_block) {
                    Some(found) => found,
                    None => GroupRecordImpl::create_labelled(&block_group, 3, sub_block),
                };
                is_interior = true;
            }
        }
        let version: u16 = match game_mode() {
            GameMode::gmSF1 => 582,
            GameMode::gmFO76 => 208,
            GameMode::gmFO4 | GameMode::gmFO4VR => 131,
            GameMode::gmSSE | GameMode::gmTES5VR | GameMode::gmEnderalSE => 44,
            GameMode::gmTES5 | GameMode::gmEnderal => 43,
            _ => 15,
        };
        let mr_struct = MainRecordStruct {
            signature,
            data_size: 0,
            flags,
            form_id: if game_mode() >= GameMode::gmTES4 {
                form_id
            } else {
                FormID::null()
            },
            vcs1: 0,
            version: if game_mode() >= GameMode::gmFO3 { version } else { 0 },
            vcs2: 0,
        };
        let container_ref: ElementRef = container.clone();
        let mr_def = crate::interface::constructors::find_record_def(signature);
        let record = Arc::new_cyclic(|self_ref: &Weak<MainRecordImpl>| MainRecordImpl {
            self_ref: self_ref.clone(),
            base: ElementBase::new(Some(&container_ref)),
            container: ContainerBase::default(),
            file: Arc::downgrade(&file),
            bytes: file.fl_bytes.clone(),
            mr_struct: RwLock::new(mr_struct),
            mr_def,
            dc_data_base: 0,
            dc_data_end: 0,
            mr_data_storage: std::sync::Mutex::new(DataStorage::Unloaded),
            mr_pinned_data: std::sync::OnceLock::new(),
            mr_init: InitOnce::new(),
            mr_editor_id: RwLock::new(String::new()),
            mr_full_name: RwLock::new(String::new()),
            mr_names_known: std::sync::atomic::AtomicBool::new(false),
            mr_builds: std::sync::atomic::AtomicU32::new(0),
            mr_master: RwLock::new(None),
            mr_overrides: RwLock::new(Vec::new()),
            mr_fixed_form_id: std::sync::atomic::AtomicU64::new(super::UNSET_FIXED_FORM_ID),
            mr_display_name: RwLock::new(None),
            mr_precombined: std::sync::OnceLock::new(),
            mr_ofst_removed: std::sync::atomic::AtomicBool::new(false),
            mr_duplicate: std::sync::atomic::AtomicBool::new(false),
        });
        let Some(mr_def) = record.mr_def.clone() else {
            return Err(format!("Error: unknown record type {signature}"));
        };
        container.container.add_element(record.clone());
        if let Err(error) = file.add_main_record(record.clone()) {
            let record_ref: ElementRef = record.clone();
            container.container.remove_element_by_identity(&record_ref);
            return Err(error);
        }
        record.begin_update();
        record.do_init();
        record.set_modified(true);
        record.invalidate_storage();
        let count = usize::try_from(mr_def.get_member_count()).unwrap_or(0);
        for index in 0..count {
            if mr_def.get_member(index).def_base().def_required() {
                record.assign(index as i32, None, false);
            }
        }
        if is_interior && let Some(data) = record.get_record_by_signature(Signature::new(b"DATA")) {
            let _ = data.set_edit_value("1");
        }
        if mr_def.get_is_reference()
            && let Some(cell) = container.children_of()
            && !cell.get_is_persistent()
            && let Some((x, y)) = cell.get_grid_cell()
        {
            record.set_position(grid_cell_to_center_position(x, y));
        }
        record.end_update();
        container.resort();
        Ok(record)
    }

    /// Port of `TwbMainRecord.SetPosition`: the position of a placed record.
    pub(crate) fn set_position(&self, position: (f64, f64, f64)) -> bool {
        let signature = self.get_signature();
        if !POSITIONED.iter().any(|expected| signature == Signature::new(expected)) {
            return false;
        }
        let Some(data) = self.get_record_by_signature(Signature::new(b"DATA")) else {
            return false;
        };
        let Some(data) = data.as_container().filter(|data| data.get_element_count() == 2) else {
            return false;
        };
        let Some(position_element) = data.get_element(0) else {
            return false;
        };
        let Some(values) = position_element
            .as_container()
            .filter(|values| values.get_element_count() == 3)
        else {
            return false;
        };
        position_element.begin_update();
        for (index, value) in [position.0, position.1, position.2].into_iter().enumerate() {
            if let Some(element) = values.get_element(index as i32) {
                let _ = element.set_native_value(Variant::Float(value));
            }
        }
        position_element.end_update();
        true
    }

    /// Port of `TwbMainRecord.SetGridCell`: the grid cell of an exterior
    /// cell, adding the subrecord that holds it.
    pub(crate) fn set_grid_cell(self: &Arc<Self>, grid_cell: (i32, i32)) -> Result<bool, EditError> {
        let Some(def) = self.mr_def.clone() else {
            return Ok(false);
        };
        if !def.get_contains_known_sub_record(KnownSubRecord::ksrGridCell) {
            return Ok(false);
        }
        self.do_init();
        let signature = def.known_sub_record_signatures()[KnownSubRecord::ksrGridCell.ord()];
        let record = match self.get_record_by_signature(signature) {
            Some(record) => record,
            None => {
                self.add(&signature.to_string(), true)?;
                let Some(record) = self.get_record_by_signature(signature) else {
                    return Ok(false);
                };
                record
            }
        };
        let Some(container) = record
            .as_container()
            .filter(|container| container.get_element_count() >= 2)
        else {
            return Ok(false);
        };
        record.begin_update();
        let result = container
            .set_element_native_value("X", Variant::Int(i64::from(grid_cell.0)))
            .and_then(|()| container.set_element_native_value("Y", Variant::Int(i64::from(grid_cell.1))));
        record.end_update();
        result?;
        Ok(true)
    }

    /// Port of `TwbMainRecord.SetEditorID`: the editor ID subrecord takes the
    /// value, added when it is missing.
    pub(crate) fn set_editor_id(self: &Arc<Self>, value: &str) -> Result<(), EditError> {
        let Some(def) = self.mr_def.clone() else {
            return Ok(());
        };
        if value == self.get_editor_id() {
            return Ok(());
        }
        self.do_init();
        let signature = def.known_sub_record_signatures()[KnownSubRecord::ksrEditorID.ord()];
        let self_ref: ElementRef = self.clone();
        let record = match self.get_record_by_signature(signature) {
            Some(record) => record,
            None => {
                let index = crate::interface::sub_record_group::RecordDef::get_member_index_for(
                    &*def,
                    Some(&self_ref),
                    signature,
                    None,
                );
                if index < 0 {
                    return Ok(());
                }
                self.assign(index, None, false);
                self.get_record_by_signature(signature)
                    .ok_or_else(|| format!("{} has no editor ID subrecord", self.get_name()))?
            }
        };
        def.set_editor_id(&record, value)
    }

    /// Port of `GetAllVisibleForFile`: the versions of the record in the
    /// file and its masters, from the file down.
    pub(crate) fn all_visible_for_file(&self, file: &Arc<FileImpl>) -> Vec<Arc<MainRecordImpl>> {
        let form_id = self.get_load_order_form_id();
        let mut result = Vec::new();
        if let Some(record) = file.contained_record_by_load_order_form_id(form_id) {
            result.push(record);
        }
        for master in file.masters().iter().rev() {
            if let Some(record) = master.contained_record_by_load_order_form_id(form_id) {
                result.push(record);
            }
        }
        result
    }

    /// Port of `GetChildBySignature`: the first record with the signature in
    /// the child group, or in its temporary children of a cell.
    pub(crate) fn child_by_signature(&self, signature: Signature) -> Option<Arc<MainRecordImpl>> {
        let group = self.child_group()?;
        if let Some(found) = group.record_by_signature(signature) {
            return Some(found);
        }
        if group.group_type() == 6 {
            return group
                .find_child_group(9, self.mr_struct().form_id.to_cardinal())?
                .record_by_signature(signature);
        }
        None
    }

    /// Port of `GetChildByGridCell`: the cell of a worldspace at the grid cell.
    pub(crate) fn child_by_grid_cell(&self, grid_cell: (i32, i32)) -> Option<Arc<MainRecordImpl>> {
        let group = self.child_group()?;
        if group.group_type() != 1 {
            return None;
        }
        let sub_block = sub_block_from_grid_cell(grid_cell.0, grid_cell.1);
        let block = block_from_sub_block(sub_block.0, sub_block.1);
        let group = group.find_child_group(4, grid_cell_to_group_label(block.0, block.1))?;
        let group = group.find_child_group(5, grid_cell_to_group_label(sub_block.0, sub_block.1))?;
        group.container.elements().iter().find_map(|element| {
            let cell = element.as_element_impl()?.main_record_impl()?;
            (cell.get_grid_cell() == Some(grid_cell)).then_some(cell)
        })
    }

    /// Port of `RemoveOverride`.
    pub(crate) fn remove_override(&self, record: &Arc<MainRecordImpl>) {
        self.mr_overrides
            .write()
            .unwrap()
            .retain(|other| other.upgrade().is_none_or(|other| !Arc::ptr_eq(&other, record)));
    }

    /// Port of `YouGotAMaster`: `master` becomes the master of this record
    /// and of its overrides.
    pub(crate) fn you_got_a_master(self: &Arc<Self>, master: &Arc<MainRecordImpl>) {
        if let Some(file) = self.file_impl()
            && !file.is_new_record(self.get_fixed_form_id().file_id())
            && let Some(injection_master) = file.get_master_for_file_id(self.get_fixed_form_id().file_id())
        {
            injection_master.remove_injected_main_record(self);
        }
        let mut overrides = vec![self.clone()];
        overrides.extend(self.overrides());
        master.you_are_the_master(&overrides);
        self.mr_overrides.write().unwrap().clear();
    }

    /// Port of `YouAreTheMaster`: this record, which was overridden by
    /// none, becomes the master of `overrides`.
    pub(crate) fn you_are_the_master(self: &Arc<Self>, overrides: &[Arc<MainRecordImpl>]) {
        *self.mr_master.write().unwrap() = None;
        let mut own = self.mr_overrides.write().unwrap();
        own.clear();
        for record in overrides {
            if Arc::ptr_eq(record, self) {
                continue;
            }
            *record.mr_master.write().unwrap() = Some(Arc::downgrade(self));
            own.push(Arc::downgrade(record));
        }
        drop(own);
        if let Some(file) = self.file_impl()
            && !file.is_new_record(self.mr_struct().form_id.file_id())
            && let Some(master_file) = file.get_master_for_file_id(self.mr_struct().form_id.file_id())
        {
            master_file.inject_main_record(self.clone());
        }
    }

    /// Port of `TwbMainRecord.Remove` before `TwbElement.Remove`: the child
    /// group goes, the record leaves its file, and an override of it takes
    /// its place as the master of the others.
    pub(crate) fn remove_from_file(self: &Arc<Self>) -> Result<(), EditError> {
        if let Some(group) = self.child_group() {
            group.remove();
        }
        if let Some(file) = self.file_impl() {
            file.remove_main_record(self)?;
        }
        match self.master() {
            Some(master) => master.remove_override(self),
            None => {
                let overrides = self.overrides();
                if let Some(first) = overrides.first() {
                    first.you_are_the_master(&overrides);
                }
            }
        }
        *self.mr_master.write().unwrap() = None;
        self.mr_overrides.write().unwrap().clear();
        self.clear_fixed_form_id();
        Ok(())
    }

    /// Port of `TwbMainRecord.CanAssignInternal`.
    pub(crate) fn can_assign_internal_impl(
        self: &Arc<Self>,
        index: i32,
        source: Option<&ElementRef>,
        check_dont_show: bool,
    ) -> bool {
        let Some(mr_def) = self.mr_def.clone() else {
            return false;
        };
        if !is_internal_edit() && (!edit_allowed() || mr_def.def_base().def_internal_edit_only()) {
            return false;
        }
        let known = mr_def.known_sub_record_signatures();
        let source_signature = source.and_then(|source| source.get_has_signature());
        if self.get_is_deleted()
            && index != ASSIGN_THIS
            && !(game_mode() >= GameMode::gmFO4 && source_signature == Some(known[KnownSubRecord::ksrBaseRecord.ord()]))
        {
            return false;
        }
        if self.get_is_partial_form()
            && index != ASSIGN_THIS
            && source_signature != Some(known[KnownSubRecord::ksrEditorID.ord()])
        {
            return false;
        }
        if !super::assign::parent_allows_edit(&**self) {
            return false;
        }
        if check_dont_show && self.get_dont_show() {
            return false;
        }
        let self_ref: ElementRef = self.clone();
        let member_count = mr_def.get_member_count();
        let Some(source) = source else {
            let mut result =
                index >= 0 && index < member_count && self.container.element_by_sort_order(index).is_none();
            if result && check_dont_show {
                result = !mr_def.get_member(index as usize).get_dont_show(Some(&self_ref));
            }
            if result
                && !is_internal_edit()
                && mr_def
                    .get_member(index as usize)
                    .def_base()
                    .def_flags
                    .contains(DefFlag::dfInternalEditOnly)
            {
                result = false;
            }
            return result;
        };
        let source_def = source.get_def();
        let source_def = source_def.as_deref().map(|def| def.as_dyn_def());
        if index == ASSIGN_THIS {
            return mr_def.equals(source_def);
        }
        let mut result = index >= 0 && index < member_count && {
            let member = mr_def.get_member(index as usize);
            member.can_assign(Some(&self_ref), ASSIGN_THIS, source_def)
                || member.can_assign(Some(&self_ref), ASSIGN_ADD, source_def)
        };
        if result && check_dont_show {
            result = !mr_def.get_member(index as usize).get_dont_show(Some(&self_ref));
        }
        result
    }

    /// Port of `TwbMainRecord.AssignInternal`: with `ASSIGN_THIS` the record
    /// takes the header flags, the version and every member of the source
    /// (or only its required members without one); with a member index the
    /// member is made or takes the value of the source.
    pub(crate) fn assign_internal_impl(
        self: &Arc<Self>,
        index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && !edit_allowed() {
            return Err(format!("{} can not be assigned.", self.get_name()));
        }
        let Some(mr_def) = self.mr_def.clone() else {
            return super::assign::container_assign_internal(&**self, index, source, only_sk);
        };
        let known = mr_def.known_sub_record_signatures();
        let source_signature = source.and_then(|source| source.get_has_signature());
        if self.get_is_deleted() && index != ASSIGN_THIS {
            let mut should_exit = true;
            if game_mode() >= GameMode::gmFO4 {
                should_exit = mr_def.get_known_sub_record_member_index(KnownSubRecord::ksrBaseRecord) != index;
                if !should_exit && let Some(signature) = source_signature {
                    should_exit = known[KnownSubRecord::ksrBaseRecord.ord()] != signature;
                }
            }
            if should_exit {
                return Ok(None);
            }
        }
        if self.get_is_partial_form() && index != ASSIGN_THIS {
            let mut should_exit = mr_def.get_known_sub_record_member_index(KnownSubRecord::ksrEditorID) != index;
            if !should_exit && let Some(signature) = source_signature {
                should_exit = known[KnownSubRecord::ksrEditorID.ord()] != signature;
            }
            if should_exit
                && (fill_inom() || fill_inoa())
                && self.get_signature() == Signature::new(b"DIAL")
                && index >= 0
                && index < mr_def.get_member_count()
            {
                let member_signature = mr_def.get_member(index as usize).get_default_signature();
                if (fill_inom() && member_signature == Signature::new(b"INOM"))
                    || (fill_inoa() && member_signature == Signature::new(b"INOA"))
                {
                    should_exit = false;
                }
            }
            if should_exit {
                return Ok(None);
            }
        }
        self.do_init();
        let self_ref: ElementRef = self.clone();
        let mut result = None;
        if index == ASSIGN_THIS {
            self.set_modified(true);
            self.invalidate_storage();
            self.container.release_elements();
            if let Some(source_record) = source.and_then(|source| source.as_element_impl()?.main_record_impl()) {
                let source_struct = source_record.mr_struct();
                self.make_header_writeable(|header| {
                    header.flags = source_struct.flags;
                    header.vcs1 = 0;
                    if game_mode() >= GameMode::gmFO3 {
                        header.version = source_struct.version;
                        header.vcs2 = 0;
                    }
                });
            }
            self.create_contained_in();
            self.create_record_header();
            match source.and_then(|source| source.as_container().map(|container| (source, container))) {
                Some((source, source_container)) => {
                    let own_master = self.get_master_or_self().get_element_id();
                    let _ = source;
                    for i in 0..source_container.get_element_count() {
                        let Some(element) = source_container.get_element(i) else {
                            continue;
                        };
                        let no_copy = element
                            .get_def()
                            .is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfNoCopyAsOverride));
                        if no_copy
                            && element
                                .get_containing_main_record()
                                .is_some_and(|record| record.get_master_or_self().get_element_id() == own_master)
                        {
                            continue;
                        }
                        self.assign(element.get_sort_order(), Some(&element), only_sk);
                    }
                }
                None => {
                    for i in 0..usize::try_from(mr_def.get_member_count()).unwrap_or(0) {
                        if mr_def.get_member(i).def_base().def_required() {
                            self.assign(i as i32, None, false);
                        }
                    }
                }
            }
        } else if index >= 0 && index < mr_def.get_member_count() {
            let mut member = mr_def.get_member(index as usize);
            let source_def = source.and_then(|source| source.get_def());
            let source_def = source_def.as_deref().map(|def| def.as_dyn_def());
            let is_add = source.is_none() || member.can_assign(Some(&self_ref), ASSIGN_THIS, source_def);
            let is_add_child =
                !is_add && source.is_some() && member.can_assign(Some(&self_ref), ASSIGN_ADD, source_def);
            if is_add || is_add_child {
                let existing = self.container.element_by_sort_order(index);
                let element = match existing {
                    Some(element) => element,
                    None => {
                        if member.get_def_type() == DefType::dtSubRecordUnion {
                            member = match source.and_then(|source| source.get_has_signature().map(|sig| (source, sig)))
                            {
                                Some((source, signature)) => member
                                    .as_record_def()
                                    .and_then(|union| {
                                        let data = source.as_data_container().map(|_| source);
                                        union.get_member_for(Some(&self_ref), signature, data)
                                    })
                                    .ok_or_else(|| {
                                        format!("{} has no member for {}", self.get_name(), source.get_name())
                                    })?,
                                None => member
                                    .as_record_def()
                                    .map(|union| union.get_member(0))
                                    .ok_or_else(|| format!("{} has an empty union member", self.get_name()))?,
                            };
                        }
                        let element =
                            super::sub_record::create_member_new(&self_ref, &self.container, &self.file, member)?;
                        if let Some(element_impl) = element.as_element_impl() {
                            element_impl.set_sort_and_memory_order(index);
                        }
                        element
                    }
                };
                if is_add && let Some(source) = source {
                    element.assign(ASSIGN_THIS, Some(source), only_sk);
                } else if is_add_child {
                    element.assign(ASSIGN_ADD, source, only_sk);
                }
                result = Some(element);
            }
        } else if index == -2
            && let Some(element) = self.container.element_by_sort_order(-2)
        {
            element.assign(ASSIGN_THIS, source, false);
            result = Some(element);
        }
        if sort_sub_records() {
            super::edit::sort_sub_records_of(&**self);
        }
        Ok(result)
    }

    /// Port of the child group part of `TwbMainRecord.Add`: a placed record
    /// of a cell, a response of a topic, a cell or road of a worldspace, or a
    /// child of a quest goes into the child group, made when it is missing.
    pub(crate) fn add_child(
        self: &Arc<Self>,
        name: &str,
        silent: bool,
    ) -> Result<Option<Option<ElementRef>>, EditError> {
        let prefix: String = name.chars().take(4).collect();
        let child = |signatures: &[&str]| {
            signatures
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&prefix))
        };
        let signature = self.mr_struct().signature;
        let group_type = if signature == Signature::new(b"CELL") && child(&PLACED) {
            6
        } else if signature == Signature::new(b"DIAL") && child(&["INFO"]) {
            7
        } else if signature == Signature::new(b"WRLD") && child(&["ROAD", "CELL"]) {
            1
        } else if vwd_as_quest_children() && signature == Signature::new(b"QUST") && child(&["DLBR", "DIAL", "SCEN"]) {
            10
        } else {
            return Ok(None);
        };
        let group = match self.child_group() {
            Some(group) => group,
            None => {
                let parent = self
                    .base
                    .container()
                    .and_then(|container| container.as_element_impl()?.group_record_impl())
                    .ok_or_else(|| format!("{} is not in a group", self.get_name()))?;
                GroupRecordImpl::create_child(&parent, group_type, self)
            }
        };
        Ok(Some(group.add_impl_group(name, silent)?))
    }
}
