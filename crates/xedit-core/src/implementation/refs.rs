// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of the reference paths of the elements: `BuildRef` with
//! `AddReferencedFromID` and `CanContainFormIDs`, which collect the FormIDs
//! a main record refers to (`TwbMainRecord.DoBuildRef` without the
//! referenced-by lists), and `CompareExchangeFormID`, which replaces one
//! load order FormID by another in every element of a record.
//!
//! The port has no reference index yet: upstream keeps `mrReferencedBy` for
//! every record from the `BuildRef` of all loaded files, and the FormID
//! change of `xeMainForm` (`ShowChangeReferencedBy`) calls
//! `CompareExchangeFormID` on the records in that list. Until phase 4 builds
//! the index, [`ReferenceScan`] finds the same records on demand: a record
//! refers to a target when a FormID its `BuildRef` collects resolves, as
//! `DoBuildRef` resolves it, to the target or one of its overrides. The scan
//! looks only at the files that can see the FormID of a target (the file of
//! the FormID and the files that have it as a master) and skips a record
//! whose data does not hold the FormID's four bytes before it builds its
//! elements. Phase 4 replaces the scan by the index behind the same call.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::interface::def::Def;
use crate::interface::element::{Element, ElementRef, MainRecord};
use crate::interface::form_id::FormID;
use crate::interface::misc::{EditError, Variant};
use crate::interface::types::{DefFlag, ElementType};

use super::{ElementImpl, FileImpl, MainRecordImpl, edit, value};

thread_local! {
    /// Port of `mrTmpRefFormIDs`: the FormIDs collected by the `BuildRef`
    /// of the main record that builds on this thread (`mrsBuildingRef`).
    static BUILDING_REFS: RefCell<Option<BTreeSet<u32>>> = const { RefCell::new(None) };
}

/// Port of `AddReferencedFromID`: `TwbElement` hands the FormID up to its
/// main record, which keeps it while it builds its references.
pub(crate) fn add_referenced_from_id(form_id: FormID) {
    if form_id.is_null() {
        return;
    }
    BUILDING_REFS.with(|refs| {
        if let Some(refs) = refs.borrow_mut().as_mut() {
            refs.insert(form_id.to_cardinal());
        }
    });
}

/// Whether the element is the record header or the contained-in element
/// of a main record, which hold no references (`TwbRecordHeaderStruct`,
/// `TwbContainedInElement`).
fn is_header_element(element: &dyn ElementImpl) -> bool {
    element.value_impl().is_some_and(|value| {
        value.vb.record_header.load(std::sync::atomic::Ordering::Relaxed)
            || (element.get_sort_order() == -2 && value.vb.dont_save.load(std::sync::atomic::Ordering::Relaxed))
    })
}

fn def_flag(element: &dyn ElementImpl, flag: DefFlag) -> bool {
    element
        .get_def()
        .is_some_and(|def| def.def_base().def_flags.contains(flag))
}

fn value_def_flag(element: &dyn ElementImpl, flag: DefFlag) -> bool {
    element
        .get_value_def()
        .is_some_and(|def| def.def_base().def_flags.contains(flag))
}

/// Port of `CanContainFormIDs` of every element class.
pub(crate) fn can_contain_form_ids(element: &dyn ElementImpl) -> bool {
    match element.get_element_type() {
        ElementType::etMainRecord => true,
        ElementType::etSubRecord | ElementType::etSubRecordArray | ElementType::etSubRecordStruct => {
            def_flag(element, DefFlag::dfCanContainFormID)
        }
        ElementType::etValue
        | ElementType::etStruct
        | ElementType::etStructChapter
        | ElementType::etArray
        | ElementType::etUnion => !is_header_element(element) && value_def_flag(element, DefFlag::dfCanContainFormID),
        ElementType::etStringListTerminator => false,
        _ => true,
    }
}

fn children(element: &dyn ElementImpl) -> Vec<ElementRef> {
    if let Some(container) = element.as_container() {
        // `DoInit`.
        container.get_element_count();
    }
    element.container_base().map(|base| base.elements()).unwrap_or_default()
}

/// The data of a subrecord or value, as `GetDataBasePtr` gives it.
fn data_of(element: &dyn ElementImpl) -> Option<&[u8]> {
    element.as_data_container().and_then(|data| data.get_data())
}

/// Port of `BuildRef` of the elements below a main record:
/// `TwbContainer.BuildRef`, `TwbSubRecord.BuildRef` and
/// `TwbValueBase.BuildRef` (the value definition after the elements).
fn build_ref(element: &dyn ElementImpl) {
    if is_header_element(element) {
        return;
    }
    match element.get_element_type() {
        ElementType::etStringListTerminator => return,
        ElementType::etMainRecord | ElementType::etFile | ElementType::etGroupRecord => return,
        _ => {}
    }
    if def_flag(element, DefFlag::dfExcludeFromBuildRef) || value_def_flag(element, DefFlag::dfExcludeFromBuildRef) {
        return;
    }
    let self_ref = element.self_element_ref();
    let is_sub_record = element.get_element_type() == ElementType::etSubRecord;
    if is_sub_record && element.get_def().is_some() {
        element.as_container().map(|container| container.get_element_count());
        if let Some(value_def) = element.get_value_def() {
            value_def.build_ref(data_of(element), self_ref.as_ref());
        }
    }
    for child in children(element) {
        if let Some(child) = child.as_element_impl()
            && can_contain_form_ids(child)
        {
            build_ref(child);
        }
    }
    let is_value = matches!(
        element.get_element_type(),
        ElementType::etValue
            | ElementType::etStruct
            | ElementType::etStructChapter
            | ElementType::etArray
            | ElementType::etUnion
    );
    if is_value && let Some(value_def) = element.get_value_def() {
        value_def.build_ref(data_of(element), self_ref.as_ref());
    }
}

impl MainRecordImpl {
    /// Port of `TwbMainRecord.BuildRef` through `DoBuildRef`: the FormIDs
    /// of the files the record's elements refer to, sorted, as the record's
    /// file stores them (`mrReferences`). A record without a definition or
    /// whose definition is excluded refers to nothing.
    pub fn build_ref(self: &Arc<Self>) -> Vec<FormID> {
        let Some(def) = &self.mr_def else { return Vec::new() };
        if def.def_base().def_flags.contains(DefFlag::dfExcludeFromBuildRef) {
            return Vec::new();
        }
        let outer = BUILDING_REFS.with(|refs| refs.borrow_mut().replace(BTreeSet::new()));
        self.do_init();
        for element in self.container.elements() {
            if let Some(element) = element.as_element_impl()
                && can_contain_form_ids(element)
            {
                build_ref(element);
            }
        }
        let collected = BUILDING_REFS.with(|refs| std::mem::replace(&mut *refs.borrow_mut(), outer));
        collected
            .unwrap_or_default()
            .into_iter()
            .map(FormID::from_cardinal)
            .collect()
    }

    /// Port of `TwbMainRecord.CompareExchangeFormID`: every element that can
    /// hold a FormID replaces the load order FormID `old` by `new`. Returns
    /// whether anything changed.
    pub fn compare_exchange_form_id(self: &Arc<Self>, old: FormID, new: FormID) -> Result<bool, EditError> {
        self.do_init();
        container_compare_exchange(&**self, old, new)
    }

    /// The process of `DoBuildRef` for one collected FormID: the record it
    /// resolves to through the master of the record's file its FileID
    /// names, or through the file itself for a FileID past the masters.
    pub fn resolve_reference(&self, form_id: FormID) -> Option<Arc<MainRecordImpl>> {
        let file = self.file_impl()?;
        let masters = file.masters();
        let target = if crate::interface::globals::complex_file_file_id() {
            // The slot among the masters of the module type of the FileID.
            match usize::try_from(file.get_master_index_for_file_id(form_id.file_id())) {
                Ok(index) if index < masters.len() => masters[index].clone(),
                _ => file,
            }
        } else {
            let slot = usize::try_from(form_id.file_id().full_slot()).ok()?.min(masters.len());
            if slot == masters.len() {
                file
            } else {
                masters[slot].clone()
            }
        };
        let form_id = form_id.change_file_id(target.get_file_file_id());
        target.record_by_form_id(form_id, true, true)
    }
}

/// Port of `TwbContainer.CompareExchangeFormID`: the elements that can hold
/// FormIDs, inside one update.
fn container_compare_exchange(element: &dyn ElementImpl, old: FormID, new: FormID) -> Result<bool, EditError> {
    let elements = children(element);
    edit::begin_update(element);
    let mut result = Ok(false);
    for child in elements {
        let Some(child) = child.as_element_impl() else { continue };
        if !can_contain_form_ids(child) {
            continue;
        }
        match compare_exchange(child, old, new) {
            Ok(changed) => result = result.map(|any| any || changed),
            Err(error) => {
                result = Err(error);
                break;
            }
        }
    }
    edit::end_update(element);
    result
}

/// Port of `CompareExchangeFormID` of the elements below a main record:
/// a subrecord, a union and a value replace the FormID in their elements
/// and then in their own data through the resolved definition (with the
/// `AfterSet` callbacks for `dfAfterSetOnIDUpdate`); the other containers
/// only in their elements.
fn compare_exchange(element: &dyn ElementImpl, old: FormID, new: FormID) -> Result<bool, EditError> {
    if is_header_element(element) {
        return Ok(false);
    }
    let own_data = match element.get_element_type() {
        ElementType::etSubRecord => {
            if element.get_def().is_none() {
                return Ok(false);
            }
            true
        }
        ElementType::etValue | ElementType::etUnion => true,
        ElementType::etStringListTerminator => return Ok(false),
        _ => false,
    };
    if !own_data {
        return container_compare_exchange(element, old, new);
    }
    edit::begin_update(element);
    let result = (|| {
        let mut result = container_compare_exchange(element, old, new)?;
        let old_value = element.get_native_value();
        let self_ref = element.self_element_ref();
        if let Some(value_def) = element.get_value_def() {
            let resolved = value::resolve(value_def, data_of(element), self_ref.as_ref());
            if resolved.compare_exchange_form_id(data_of(element), self_ref.as_ref(), old, new)? {
                element.set_modified(true);
                result = true;
                let new_value: Variant = element.get_native_value();
                if def_flag(element, DefFlag::dfAfterSetOnIDUpdate) {
                    element.do_after_set(&old_value, &new_value);
                }
            }
        }
        Ok(result)
    })();
    edit::end_update(element);
    result
}

/// The records that refer to a set of records, found by scanning the files
/// that can see their FormIDs: the stand-in for upstream's `ReferencedBy`
/// until the reference index of phase 4 exists.
pub struct ReferenceScan {
    /// The referencing records by the master (`MasterOrSelf`) they refer to,
    /// keyed by its element ID, in file and FormID order.
    referenced_by: HashMap<usize, Vec<Arc<MainRecordImpl>>>,
}

impl ReferenceScan {
    /// Finds the records of every loaded file that refer to one of
    /// `targets` or to one of their overrides.
    pub fn new(targets: &[Arc<MainRecordImpl>]) -> Self {
        let masters: Vec<Arc<MainRecordImpl>> = targets.iter().map(|record| record.master_or_self_impl()).collect();
        let master_ids: HashSet<usize> = masters.iter().map(|master| master.get_element_id()).collect();
        let mut referenced_by: HashMap<usize, Vec<Arc<MainRecordImpl>>> = HashMap::new();
        let mut files: Vec<Arc<FileImpl>> = super::FILES_MAP.read().unwrap().clone();
        files.sort_by_key(|file| file.load_order());
        for file in files {
            // The FormIDs of the targets as this file stores them; a file
            // that cannot see a target's file cannot refer to it.
            let needles: HashSet<u32> = masters
                .iter()
                .filter_map(|master| {
                    let form_id = master.get_load_order_form_id();
                    FileImpl::load_order_form_id_to_file_form_id(&file, form_id)
                })
                .map(FormID::to_cardinal)
                .collect();
            if needles.is_empty() {
                continue;
            }
            for record in file.records() {
                if !record.may_hold_any(&needles) {
                    continue;
                }
                let was_initialized = record.container.element_count() > 0;
                let mut found: Vec<usize> = Vec::new();
                for form_id in record.build_ref() {
                    if !needles.contains(&form_id.to_cardinal()) {
                        continue;
                    }
                    let Some(target) = record.resolve_reference(form_id) else {
                        continue;
                    };
                    let id = target.master_or_self_impl().get_element_id();
                    if master_ids.contains(&id) && !found.contains(&id) {
                        found.push(id);
                    }
                }
                for id in found {
                    referenced_by.entry(id).or_default().push(record.clone());
                }
                if !was_initialized {
                    record.reset();
                }
            }
        }
        ReferenceScan { referenced_by }
    }

    /// The records that refer to `record` or to its master or overrides
    /// (`MasterOrSelf.ReferencedBy`).
    pub fn referenced_by(&self, record: &Arc<MainRecordImpl>) -> Vec<Arc<MainRecordImpl>> {
        let id = record.master_or_self_impl().get_element_id();
        self.referenced_by.get(&id).cloned().unwrap_or_default()
    }
}

impl MainRecordImpl {
    /// Whether the record may refer to one of the file FormIDs `needles`:
    /// a modified record always may, an unmodified one only when its data
    /// holds the four bytes of one of them.
    fn may_hold_any(&self, needles: &HashSet<u32>) -> bool {
        if self.base.has_state(super::ElementState::esModified) {
            return true;
        }
        let compressed;
        let data: &[u8] = if self.mr_struct().flags.is_compressed() {
            match self.decompress_uncached() {
                Some(data) => {
                    compressed = data;
                    &compressed
                }
                // Undecidable: let the elements decide.
                None => return true,
            }
        } else {
            match self.raw_data() {
                Some(data) => data,
                None => return false,
            }
        };
        data.windows(4)
            .any(|window| needles.contains(&u32::from_le_bytes([window[0], window[1], window[2], window[3]])))
    }

    /// The decompressed data of a compressed record, without keeping it.
    fn decompress_uncached(&self) -> Option<Vec<u8>> {
        let raw = self.raw_data()?;
        let length = u32::from_le_bytes(raw.get(..4)?.try_into().ok()?) as usize;
        let mut data = vec![0u8; length];
        if length > 0 {
            xedit_io::CompressionType::ZLib
                .decompress(raw.get(4..)?, &mut data)
                .ok()?;
        }
        Some(data)
    }
}
