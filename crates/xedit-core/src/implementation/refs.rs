// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of the reference index: `BuildRef` of every element with
//! `AddReferencedFromID` and `CanContainFormIDs`, which collect the FormIDs
//! a main record refers to; `TwbMainRecord.BuildRef`, `DoBuildRef` and
//! `UpdateRefs`, which keep them in `mrReferences` and add the record to the
//! referenced-by list (`mrReferencedBy`, `AddReferencedBy`,
//! `RemoveReferencedBy`, `SortReferencedBy`) of every record it refers to;
//! `TwbFile.BuildRef` and `BuildOrLoadRef` with the reference cache file
//! (`refcache`); and `CompareExchangeFormID`, which replaces one load order
//! FormID by another in every element of a record.
//!
//! Upstream builds the references of the loaded files on load
//! (`TfrmMain`'s loader, one file per thread with `USE_PARALLEL_BUILD_REFS`,
//! `AddReferencedBy` under a lock and the lists sorted when first read). The
//! port builds them when a command first needs them ([`build_or_load_refs`]):
//! the FormIDs of every record of the files to build are collected on the
//! worker threads, record by record, and then added to the lists in file
//! order on the calling thread. A referenced-by list is sorted by the load
//! order FormID and the load order of the file of each record when it is
//! read, as upstream, so the result does not depend on the thread count. An
//! edit keeps the index right: a changed record builds its references again
//! (`UpdateRefs` from `ElementChanged` and `SetParentModified`), a new or
//! copied record builds them once its file has them, a removed record takes
//! its references back (`DoBuildRef(True)`), and a record whose FormID
//! changes hands its list to the override that becomes the master.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::interface::def::Def;
use crate::interface::element::{ElementRef, File, MainRecord};
use crate::interface::form_id::{FileID, FormID};
use crate::interface::globals::{complex_file_file_id, dont_cache, dont_cache_load, dont_cache_save};
use crate::interface::misc::{EditError, Variant};
use crate::interface::types::{DefFlag, ElementType, FileState};
use crate::threads;

use super::{
    ElementImpl, ElementState, FileImpl, MainRecordImpl, edit, pin_record, refcache, trim_initialized_records, value,
};

thread_local! {
    /// Port of `mrTmpRefFormIDs`: the FormIDs collected by the `BuildRef`
    /// of the main record that builds on this thread (`mrsBuildingRef`).
    static BUILDING_REFS: RefCell<Option<BTreeSet<u32>>> = const { RefCell::new(None) };
    /// Port of the `threadvar` `_FileRefsBuilding`: a file builds the
    /// references of its records, which are reset once built.
    static FILE_REFS_BUILDING: Cell<bool> = const { Cell::new(false) };
}

/// The reference state of a main record.
#[derive(Default)]
pub struct RecordRefs {
    /// Port of `csRefsBuild`: the references were built or loaded.
    built: bool,
    /// Port of `cntRefsBuildAt >= eGeneration`: the record did not change
    /// since. Cleared when the record is marked modified.
    current: bool,
    /// Port of `mrsBuildingRef`.
    building: bool,
    /// Port of `mrReferences`: the FormIDs the record refers to as its file
    /// stores them, sorted.
    references: Vec<FormID>,
}

/// The records that refer to a main record.
#[derive(Default)]
pub struct ReferencedBy {
    /// Port of `mrReferencedBy`. The records stay alive through their
    /// files; a record dropped from the tree drops out of the list.
    list: Vec<Weak<MainRecordImpl>>,
    /// Port of `mrsReferencedByUnsorted`.
    unsorted: bool,
}

/// Port of `TwbBuildOrLoadRefResult`.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildOrLoadRefResult {
    blrNone,
    blrBuilt,
    blrBuiltAndSaved,
    blrLoaded,
}

impl BuildOrLoadRefResult {
    /// The message the loader of `xeMainForm` logs for the result.
    pub fn message(self, only_load: bool) -> &'static str {
        match self {
            BuildOrLoadRefResult::blrBuilt => "Done building reference info.",
            BuildOrLoadRefResult::blrBuiltAndSaved => "Done building and saving reference info.",
            BuildOrLoadRefResult::blrLoaded => "Done loading reference info.",
            BuildOrLoadRefResult::blrNone if only_load => "No cached reference info available.",
            BuildOrLoadRefResult::blrNone => "No reference info built or loaded.",
        }
    }
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
        value.vb.record_header.load(Ordering::Relaxed)
            || (element.get_sort_order() == -2 && value.vb.dont_save.load(Ordering::Relaxed))
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

/// The sort key of `CompareReferencedBy` (`TwbFormID.Compare` and `CmpW32`
/// compare without sign).
fn referenced_by_key(record: &MainRecordImpl) -> (u32, u32) {
    let load_order = record.file_impl().map_or(u32::MAX, |file| file.load_order() as u32);
    (record.get_load_order_form_id().to_cardinal(), load_order)
}

impl ReferencedBy {
    /// Port of `SortReferencedBy`, a stable merge sort as `wbMergeSortPtr`,
    /// with `CompareReferencedBy`: by the load order FormID, then by the
    /// load order of the file; a record that is gone sorts last. The keys are
    /// read once per entry (`sort_by_cached_key` is stable as well).
    fn sort(&mut self) {
        self.unsorted = false;
        if self.list.len() > 1 {
            self.list.sort_by_cached_key(|entry| {
                entry
                    .upgrade()
                    .map_or((u32::MAX, u32::MAX), |entry| referenced_by_key(&entry))
            });
        }
    }

    /// Port of `FindReferencedBy`: the binary search for the first entry
    /// with the key of `record`.
    /// UPSTREAM-QUIRK: the search trusts the sort, which a record whose
    /// FormID changed after the sort no longer follows; such an entry may
    /// not be found.
    fn find(&self, record: &MainRecordImpl) -> Option<usize> {
        let key = referenced_by_key(record);
        let (mut low, mut high) = (0isize, self.list.len() as isize - 1);
        let mut found = false;
        while low <= high {
            let middle = (low + high) >> 1;
            let entry = self.list[middle as usize]
                .upgrade()
                .map_or((u32::MAX, u32::MAX), |entry| referenced_by_key(&entry));
            match entry.cmp(&key) {
                std::cmp::Ordering::Less => low = middle + 1,
                ordering => {
                    high = middle - 1;
                    if ordering == std::cmp::Ordering::Equal {
                        found = true;
                        low = middle;
                    }
                }
            }
        }
        found.then_some(low as usize)
    }
}

impl MainRecordImpl {
    /// The FormIDs the elements of the record refer to, sorted, as the
    /// record's file stores them: the walk of `TwbMainRecord.DoBuildRef`
    /// (`inherited BuildRef`) without the bookkeeping. A record without a
    /// definition or whose definition is excluded refers to nothing.
    pub fn collect_references(self: &Arc<Self>) -> Vec<FormID> {
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

    /// The file of the record (`_File`).
    pub fn record_file(&self) -> Option<Arc<FileImpl>> {
        self.file_impl()
    }

    /// Port of `mrReferences` (`GetReference`, `ReferencesCount`): the
    /// FormIDs the record refers to as its file stores them, empty until
    /// the references are built.
    pub fn references(&self) -> Vec<FormID> {
        self.mr_refs.lock().unwrap().references.clone()
    }

    /// Port of `csRefsBuild` of a main record.
    pub fn refs_built(&self) -> bool {
        self.mr_refs.lock().unwrap().built
    }

    /// The `Inc(eGeneration)` of `SetModified`: the references are to be
    /// built again.
    pub(crate) fn mark_refs_stale(&self) {
        self.mr_refs.lock().unwrap().current = false;
    }

    /// Port of `TwbMainRecord.BuildRef`: the references are built again
    /// when the record changed since they were built.
    pub fn build_ref(self: &Arc<Self>) {
        let Some(def) = &self.mr_def else { return };
        if def.def_base().def_flags.contains(DefFlag::dfExcludeFromBuildRef) {
            return;
        }
        {
            let refs = self.mr_refs.lock().unwrap();
            if refs.built && refs.current {
                return;
            }
        }
        self.do_build_ref(false);
        if FILE_REFS_BUILDING.with(Cell::get) && !self.base.has_state(ElementState::esModified) {
            self.reset();
        }
    }

    /// Port of `UpdateRefs`: a record whose references were built builds
    /// them again after a change.
    /// UPSTREAM-QUIRK: upstream also runs this while the record's own init
    /// changes it (the `AfterLoad` fixes of a record whose references came
    /// from the cache), and walks the elements built so far; the port waits
    /// for the next change or `BuildRef`, which sees the whole record.
    pub fn update_refs(self: &Arc<Self>) {
        if !self.refs_built() || self.mr_init.is_running_here() {
            return;
        }
        self.build_ref();
    }

    /// Port of `DoBuildRef`: the references are collected again (or, with
    /// `remove`, dropped), and the record leaves the referenced-by lists of
    /// the records it no longer refers to and joins the lists of the new
    /// ones. Returns whether any list changed.
    pub(crate) fn do_build_ref(self: &Arc<Self>, remove: bool) -> bool {
        let Some(def) = &self.mr_def else { return false };
        if def.def_base().def_flags.contains(DefFlag::dfExcludeFromBuildRef) {
            return false;
        }
        {
            let mut refs = self.mr_refs.lock().unwrap();
            if refs.building {
                return false;
            }
            refs.building = true;
        }
        let new = if remove {
            let mut refs = self.mr_refs.lock().unwrap();
            refs.built = false;
            refs.current = false;
            Vec::new()
        } else {
            let new = self.collect_references();
            let mut refs = self.mr_refs.lock().unwrap();
            refs.built = true;
            refs.current = true;
            new
        };
        let old = std::mem::take(&mut self.mr_refs.lock().unwrap().references);
        let changed = self.apply_references(&old, &new, |form_id| self.resolve_reference(form_id));
        let mut refs = self.mr_refs.lock().unwrap();
        refs.references = new;
        refs.building = false;
        changed
    }

    /// The merge of `DoBuildRef`: `ProcessRef` for every FormID only in
    /// `new` (added) or only in `old` (removed), both sorted.
    fn apply_references(
        self: &Arc<Self>,
        old: &[FormID],
        new: &[FormID],
        mut resolve: impl FnMut(FormID) -> Option<Arc<MainRecordImpl>>,
    ) -> bool {
        let mut changed = false;
        let mut process = |form_id: FormID, add: bool| {
            changed = true;
            if let Some(target) = resolve(form_id) {
                if add {
                    target.add_referenced_by(self);
                } else {
                    target.remove_referenced_by(self);
                }
            }
        };
        let (mut i, mut j) = (0, 0);
        while i < new.len() && j < old.len() {
            match new[i].to_cardinal().cmp(&old[j].to_cardinal()) {
                std::cmp::Ordering::Equal => {
                    i += 1;
                    j += 1;
                }
                std::cmp::Ordering::Less => {
                    process(new[i], true);
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    process(old[j], false);
                    j += 1;
                }
            }
        }
        for form_id in &new[i..] {
            process(*form_id, true);
        }
        for form_id in &old[j..] {
            process(*form_id, false);
        }
        changed
    }

    /// The process of `ProcessRef` for one collected FormID: the record it
    /// resolves to through the master of the record's file its FileID
    /// names, or through the file itself for a FileID past the masters.
    /// With `wbComplexFileFileID` the masters of the FileID's module type
    /// are counted. The masters of a compare load end with the file it
    /// compares to, as upstream's.
    pub fn resolve_reference(&self, form_id: FormID) -> Option<Arc<MainRecordImpl>> {
        let file = self.file_impl()?;
        let mut masters = file.masters();
        // Upstream's compare load (the hardcoded file) has the file it
        // compares to as its last master; the port keeps it apart.
        masters.extend(file.compare_to_file());
        let target = if complex_file_file_id() {
            let file_id = form_id.file_id();
            if !file_id.is_valid() || (file_id.is_full_slot() && file_id.full_slot() > FileID::max_full_slot()) {
                return None;
            }
            // The slot among the masters of the module type of the FileID; a
            // slot past them is the file itself.
            match usize::try_from(file.get_master_index_for_file_id(file_id)) {
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

    /// Port of `AddReferencedBy`: the master keeps the list.
    pub fn add_referenced_by(self: &Arc<Self>, record: &Arc<MainRecordImpl>) {
        if let Some(master) = self.master() {
            return master.add_referenced_by(record);
        }
        let mut list = self.mr_referenced_by.lock().unwrap();
        list.list.push(Arc::downgrade(record));
        list.unsorted = true;
    }

    /// Port of `RemoveReferencedBy`.
    pub fn remove_referenced_by(self: &Arc<Self>, record: &Arc<MainRecordImpl>) {
        if let Some(master) = self.master() {
            return master.remove_referenced_by(record);
        }
        let mut list = self.mr_referenced_by.lock().unwrap();
        if list.unsorted {
            list.sort();
        }
        if let Some(index) = list.find(record) {
            list.list.remove(index);
            if list.list.is_empty() {
                list.list = Vec::new();
            }
        }
    }

    /// Port of `ReferencedByCount`: the master's.
    pub fn referenced_by_count(&self) -> usize {
        if let Some(master) = self.master() {
            return master.referenced_by_count();
        }
        self.mr_referenced_by.lock().unwrap().list.len()
    }

    /// Port of `ReferencedBy[i]` for every `i`: the records that refer to
    /// the master of this record or to one of its overrides, sorted by
    /// their load order FormID and the load order of their file. A record
    /// that refers twice (two FormIDs that resolve to the same record) is
    /// listed twice, as upstream.
    pub fn referenced_by(&self) -> Vec<Arc<MainRecordImpl>> {
        if let Some(master) = self.master() {
            return master.referenced_by();
        }
        let mut list = self.mr_referenced_by.lock().unwrap();
        if list.unsorted {
            list.sort();
        }
        list.list.iter().filter_map(Weak::upgrade).collect()
    }

    /// The list of `mrReferencedBy`, taken for `YouAreTheMaster`.
    pub(crate) fn take_referenced_by(&self) -> ReferencedBy {
        std::mem::take(&mut *self.mr_referenced_by.lock().unwrap())
    }

    /// The `mrReferencedBy := aReferencedBy` of `YouAreTheMaster`.
    pub(crate) fn set_referenced_by(&self, list: ReferencedBy) {
        *self.mr_referenced_by.lock().unwrap() = list;
    }

    /// Port of `cntRefsBuildAt < eGeneration`: the record changed since its
    /// references were built.
    pub(crate) fn refs_out_of_date(&self) -> bool {
        !self.mr_refs.lock().unwrap().current
    }

    /// The part of `MastersUpdated` for `mrReferences`: every FormID is
    /// fixed up, and the list sorted again when one changed. `None` when the
    /// references are not built, else whether one changed.
    pub(crate) fn update_references(&self, fixup: impl Fn(FormID) -> FormID) -> Option<bool> {
        let mut refs = self.mr_refs.lock().unwrap();
        if !refs.built {
            return None;
        }
        let mut found = false;
        for form_id in refs.references.iter_mut() {
            let new = fixup(*form_id);
            if new != *form_id {
                found = true;
                *form_id = new;
            }
        }
        if found {
            refs.references.sort_by_key(|form_id| form_id.to_cardinal());
        }
        Some(found)
    }

    /// The `LoadRefsFromStream` part of a record: the references from the
    /// cache, as the file stores them.
    pub(crate) fn set_references_from_cache(&self, references: Vec<FormID>) {
        let mut refs = self.mr_refs.lock().unwrap();
        refs.references = references;
        refs.built = true;
        refs.current = true;
    }

    /// Port of `TwbMainRecord.CompareExchangeFormID`: every element that can
    /// hold a FormID replaces the load order FormID `old` by `new`, and the
    /// references are built again when they were. Returns whether anything
    /// changed.
    pub fn compare_exchange_form_id(self: &Arc<Self>, old: FormID, new: FormID) -> Result<bool, EditError> {
        self.do_init();
        let result = container_compare_exchange(&**self, old, new);
        // `if csRefsBuild in cntStates then BuildRef`: another record may
        // have the new FormID already.
        if self.refs_built() {
            self.build_ref();
        }
        result
    }
}

impl FileImpl {
    /// The main records of the element tree that are not in `flRecords`:
    /// the file header and the records with the null FormID, which
    /// `AddMainRecord` does not keep. A record skipped on load as a
    /// duplicate is not in upstream's tree.
    pub(crate) fn unlisted_records(&self) -> Vec<Arc<MainRecordImpl>> {
        fn walk(elements: Vec<ElementRef>, out: &mut Vec<Arc<MainRecordImpl>>) {
            for element in elements {
                let Some(element) = element.as_element_impl() else {
                    continue;
                };
                if let Some(record) = element.main_record_impl() {
                    if record.get_fixed_form_id().is_null() && !record.mr_duplicate.load(Ordering::Relaxed) {
                        out.push(record);
                    }
                } else if let Some(group) = element.group_record_impl() {
                    walk(group.container.elements(), out);
                }
            }
        }
        let mut out = Vec::new();
        walk(self.container.elements(), &mut out);
        out
    }

    /// Adds a state to `flStates`.
    pub(crate) fn include_file_state(&self, state: FileState) {
        self.fl_states.write().unwrap().include(state);
    }

    /// Port of `fsRefsBuild`: the references of the file were built or
    /// loaded.
    pub fn refs_built(&self) -> bool {
        self.get_file_states().contains(FileState::fsRefsBuild)
    }

    /// Port of `TwbFile.BuildRef`: the references of a file whose
    /// references are built already are built again for the records that
    /// changed since; the others are built (or loaded from the cache).
    pub fn build_ref(self: &Arc<Self>) -> Result<BuildOrLoadRefResult, String> {
        if self.get_file_states().contains(FileState::fsIsDeltaPatch) {
            return Ok(BuildOrLoadRefResult::blrNone);
        }
        Ok(build_or_load_refs(std::slice::from_ref(self), false)?[0])
    }

    /// The `inherited BuildRef` of a file whose references are built: every
    /// record builds its references again if it changed.
    fn rebuild_changed_refs(self: &Arc<Self>) {
        let outer = FILE_REFS_BUILDING.with(|building| building.replace(true));
        for record in self.unlisted_records().into_iter().chain(self.records()) {
            record.build_ref();
        }
        FILE_REFS_BUILDING.with(|building| building.set(outer));
    }
}

/// What the build of one record leaves for the apply and the cache.
pub(crate) struct Collected {
    pub references: Vec<FormID>,
    targets: Vec<Option<Arc<MainRecordImpl>>>,
    /// The time the build took, which decides whether the cache is saved.
    elapsed: Duration,
    /// The record values the cache file keeps that need the elements.
    pub cache: refcache::RecordCacheData,
}

/// The records whose builds may stay built while the workers build the
/// references, per thread: the records the definitions read often.
const KEPT_RECORDS_PER_THREAD: usize = 256;

/// The records of one batch; the batches run one after the other, and a
/// batch is built again on one thread when two of its builds needed each
/// other (`threads::init_cycles`).
const BATCH_RECORDS: usize = 4096;

/// Builds the references of one record for [`build_or_load_refs`]: the
/// record is built, its FormIDs collected and resolved, the values of the
/// cache read, and the record reset again unless it is modified (the
/// `_FileRefsBuilding` reset of `TwbMainRecord.BuildRef`).
fn collect_one(record: &Arc<MainRecordImpl>, kept: usize) -> Collected {
    let start = Instant::now();
    let _pin = pin_record(record);
    let (references, cache) = {
        let _read = threads::read_guard();
        let references = record.collect_references();
        let cache = refcache::RecordCacheData::read(record);
        (references, cache)
    };
    let targets = references
        .iter()
        .map(|form_id| record.resolve_reference(*form_id))
        .collect();
    if !record.base.has_state(ElementState::esModified) {
        record.reset();
    }
    drop(_pin);
    trim_initialized_records(kept, None);
    Collected {
        references,
        targets,
        elapsed: start.elapsed(),
        cache,
    }
}

/// Builds the references of `records` on the worker threads (all of them
/// on the calling thread without workers); the result is in the order of
/// `records` and does not depend on the thread count.
fn collect_all(records: &[Arc<MainRecordImpl>]) -> Vec<Collected> {
    let Some(pool) = threads::pool() else {
        // One thread, with the stack of the workers: a record resolves
        // deeply through the definitions.
        return std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(threads::STACK_SIZE)
                .spawn_scoped(scope, || {
                    records
                        .iter()
                        .map(|record| collect_one(record, KEPT_RECORDS_PER_THREAD))
                        .collect()
                })
                .expect("the reference thread")
                .join()
                .expect("the reference thread panicked")
        });
    };
    let kept = KEPT_RECORDS_PER_THREAD * threads::threads();
    let mut result = Vec::with_capacity(records.len());
    for batch in records.chunks(BATCH_RECORDS) {
        let cycles = threads::init_cycles();
        let collected: Vec<Collected> = pool.install(|| {
            batch
                .par_iter()
                .with_max_len(8)
                .map(|record| collect_one(record, kept))
                .collect()
        });
        if threads::init_cycles() == cycles {
            result.extend(collected);
            continue;
        }
        // Two builds needed each other: what they saw of each other could
        // depend on timing, so the batch is built again on this thread
        // from records that are not built.
        eprintln!(
            "Warning: two records needed each other while they were built; building the references of the batch again on one thread"
        );
        for record in batch {
            record.reset();
        }
        trim_initialized_records(0, None);
        result.extend(batch.iter().map(|record| collect_one(record, KEPT_RECORDS_PER_THREAD)));
    }
    result
}

/// Port of `BuildOrLoadRef` for several files at once, as the loader of
/// `xeMainForm` runs it for every loaded file: the references of a file are
/// loaded from its cache file when there is one, else built (unless
/// `only_load`) and saved to the cache when the file has more than
/// `wbCacheRecordsThreshold` records or took more than 2 seconds. The
/// records of all the files to build are built on the worker threads (see
/// the module documentation). A file whose references are built already
/// builds those of its changed records again. Returns the result of each
/// file.
pub fn build_or_load_refs(files: &[Arc<FileImpl>], only_load: bool) -> Result<Vec<BuildOrLoadRefResult>, String> {
    let mut results = vec![BuildOrLoadRefResult::blrNone; files.len()];
    // The files to build, with the cache file to save when they qualify.
    let mut to_build: Vec<(usize, Option<std::path::PathBuf>)> = Vec::new();
    let mut loaded: Vec<(usize, Vec<refcache::CachedRecord>)> = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let states = file.get_file_states();
        let modified = file.base.has_state(ElementState::esModified);
        let internal = file.base.has_state(ElementState::esInternalModified);
        if !dont_cache() && !states.contains(FileState::fsRefsBuild) && (!modified || internal) {
            let cache_file = refcache::cache_file_name(file);
            if !dont_cache_load() && cache_file.as_ref().is_some_and(|path| path.is_file()) {
                let cache_file = cache_file.expect("checked above");
                file.include_file_state(FileState::fsRefsBuild);
                let records = refcache::load(file, &cache_file)?;
                loaded.push((index, records));
                results[index] = BuildOrLoadRefResult::blrLoaded;
            } else if !only_load {
                file.include_file_state(FileState::fsRefsBuild);
                to_build.push((index, cache_file.filter(|_| !dont_cache_save())));
                results[index] = BuildOrLoadRefResult::blrBuilt;
            }
        } else if !only_load {
            if states.contains(FileState::fsRefsBuild) {
                file.rebuild_changed_refs();
            } else {
                file.include_file_state(FileState::fsRefsBuild);
                to_build.push((index, None));
            }
            results[index] = BuildOrLoadRefResult::blrBuilt;
        }
    }

    // The `inherited BuildRef` of `TwbFile` walks the element tree, which
    // holds main records besides `flRecords`: the file header and a record
    // with the null FormID (the transient types of a Fallout 4 header, a
    // navmesh without a FormID). Their references are built in either case
    // and are not cached.
    let headers: Vec<Arc<MainRecordImpl>> = loaded
        .iter()
        .map(|(index, _)| *index)
        .chain(to_build.iter().map(|(index, _)| *index))
        .flat_map(|index| files[index].unlisted_records())
        .collect();

    // The references from the caches: resolved on the workers, added on
    // this thread.
    for (index, records) in loaded {
        let file = &files[index];
        let file_records = file.records();
        let resolve = |(record, cached): (&Arc<MainRecordImpl>, &refcache::CachedRecord)| {
            cached
                .references
                .iter()
                .map(|form_id| record.resolve_reference(*form_id))
                .collect::<Vec<_>>()
        };
        let targets: Vec<Vec<Option<Arc<MainRecordImpl>>>> = match threads::pool() {
            Some(pool) => pool.install(|| file_records.par_iter().zip(records.par_iter()).map(resolve).collect()),
            None => file_records.iter().zip(records.iter()).map(resolve).collect(),
        };
        for ((record, cached), targets) in file_records.iter().zip(records).zip(targets) {
            record.set_references_from_cache(cached.references);
            record.set_names_from_cache(cached.editor_id, cached.full_name);
            for target in targets.into_iter().flatten() {
                target.add_referenced_by(record);
            }
        }
    }

    // The records of the files to build, in file order, and the headers.
    let records: Vec<Arc<MainRecordImpl>> = to_build
        .iter()
        .flat_map(|(index, _)| files[*index].records())
        .chain(headers.iter().cloned())
        .collect();
    let collected = collect_all(&records);
    let mut collected = collected.into_iter();
    for (index, cache_file) in &to_build {
        let file = &files[*index];
        let file_records = file.records();
        let mut elapsed = Duration::ZERO;
        let mut cache = Vec::with_capacity(file_records.len());
        for record in &file_records {
            let (references, record_cache, record_elapsed) =
                record.apply_collected(collected.next().expect("one result per record"));
            elapsed += record_elapsed;
            cache.push((references, record_cache));
        }
        if let Some(cache_file) = cache_file
            && (file_records.len() > crate::interface::globals::cache_records_threshold() as usize
                || elapsed > refcache::CACHE_TIME_THRESHOLD)
            && refcache::save(file, cache_file, &file_records, &cache).is_ok()
        {
            // Errors while saving the cache are ignored, as upstream.
            results[*index] = BuildOrLoadRefResult::blrBuiltAndSaved;
        }
    }
    for header in &headers {
        header.apply_collected(collected.next().expect("one result per header"));
    }
    Ok(results)
}

impl MainRecordImpl {
    /// The references a worker collected, added to the lists: the record
    /// joins the lists of the records it refers to and keeps the FormIDs.
    /// Returns the FormIDs, the values of the cache and the time the build
    /// took.
    fn apply_collected(self: &Arc<Self>, collected: Collected) -> (Vec<FormID>, refcache::RecordCacheData, Duration) {
        let Collected {
            references,
            targets,
            elapsed,
            cache,
        } = collected;
        let old = std::mem::take(&mut self.mr_refs.lock().unwrap().references);
        self.apply_references(&old, &references, |form_id| {
            // The targets resolved on the worker, by the position in
            // `references`; a FormID only in `old` resolves now.
            match references.binary_search_by_key(&form_id.to_cardinal(), |id| id.to_cardinal()) {
                Ok(position) => targets[position].clone(),
                Err(_) => self.resolve_reference(form_id),
            }
        });
        self.set_references_from_cache(references.clone());
        (references, cache, elapsed)
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
