// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of the copy path of `wbImplementation.pas`: `wbCopyElementToFile`,
//! `wbCopyElementToRecord`, `TwbElement.CopyInto` with
//! `ReportRequiredMasters` and `AddRequiredMasters`, `AddIfMissing`, and the
//! `AddIfMissingInternal` of the file, the group records and the main
//! records (the copy of a main record as an override or as a new record).
//! The `AddIfMissingInternal` of the subrecords and values are next to them.
//!
//! State: `aAllowOverwrite` with a deep copy (`wbCanOverwrite`) and the
//! copy of a partial form (`MakePartialForm`) are not ported.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::interface::element::{CopyArgs, Element, ElementRef, File, FileRef, MainRecord};
use crate::interface::globals::{
    allow_make_partial, copy_is_running, edit_allowed, is_internal_edit, is_starfield, set_copy_is_running,
    vwd_as_quest_children, wb_get_group_order,
};
use crate::interface::misc::{EditError, progress};
use crate::interface::types::{ASSIGN_THIS, DefFlag, ElementType, FileState, KnownSubRecord, PascalEnum, Signature};

use super::assign::{contains_reflection, contains_unmapped_form_id};
use super::{ElementImpl, FileImpl, GroupRecordImpl, MainRecordImpl};

/// The generations of `TwbFilesSet`: an element that reported its masters
/// to a set does not report them again.
static FILES_SET_GENERATION: AtomicU32 = AtomicU32::new(0);

/// Port of `TwbFilesSet`: the files a copy needs as masters, with the
/// generation the elements remember (`eReportMastersGen`).
pub struct FilesSet {
    files: Vec<Arc<FileImpl>>,
    generation: u32,
}

impl Default for FilesSet {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesSet {
    pub fn new() -> Self {
        let generation = (FILES_SET_GENERATION.fetch_add(1, Ordering::Relaxed) + 1) & 0x7FFF_FFFF;
        FilesSet {
            files: Vec::new(),
            generation,
        }
    }

    /// Port of `Add`: whether the file was not in the set.
    pub fn add(&mut self, file: Arc<FileImpl>) -> bool {
        if self.files.iter().any(|known| Arc::ptr_eq(known, &file)) {
            return false;
        }
        self.files.push(file);
        true
    }

    pub fn files(&self) -> &[Arc<FileImpl>] {
        &self.files
    }
}

/// The generation test at the top of every `ReportRequiredMasters`, and the
/// `dfDontAssign` tests of `TwbElement` and `TwbContainer`.
fn report_skip(element: &dyn ElementImpl, masters: &FilesSet, recursive: bool) -> bool {
    let generation = element.element_base().e_report_masters_gen.load(Ordering::Relaxed);
    if generation & 0x7FFF_FFFF == masters.generation && (!recursive || generation & 0x8000_0000 != 0) {
        return true;
    }
    let dont_assign = |flags: crate::interface::types::DefFlags| flags.contains(DefFlag::dfDontAssign);
    element
        .get_def()
        .is_some_and(|def| dont_assign(def.def_base().def_flags.get()))
        || element
            .get_value_def()
            .is_some_and(|def| dont_assign(def.def_base().def_flags.get()))
}

/// Port of `TwbElement.ReportRequiredMasters`: the file of the record the
/// element links to.
fn element_report_required_masters(element: &dyn ElementImpl, masters: &mut FilesSet, recursive: bool) {
    if report_skip(element, masters, recursive) {
        return;
    }
    if let Some(linked) = element.get_links_to()
        && let Some(file) = reference_file(&linked)
    {
        masters.add(file);
    }
    let mut generation = masters.generation;
    if recursive {
        generation |= 0x8000_0000;
    }
    element
        .element_base()
        .e_report_masters_gen
        .store(generation, Ordering::Relaxed);
}

/// Port of `TwbContainer.ReportRequiredMasters`: the element itself, then
/// the elements that can hold FormIDs.
pub(crate) fn container_report_required_masters(
    element: &dyn ElementImpl,
    masters: &mut FilesSet,
    as_new: bool,
    recursive: bool,
    initial: bool,
) {
    if report_skip(element, masters, recursive) {
        return;
    }
    if let Some(container) = element.as_container() {
        container.get_element_count();
    }
    element_report_required_masters(element, masters, recursive);
    if recursive || (initial && element.get_element_type() != ElementType::etGroupRecord) {
        let children = element.container_base().map(|base| base.elements()).unwrap_or_default();
        for child in children {
            if let Some(child) = child.as_element_impl()
                && child.can_contain_form_ids()
            {
                child.report_required_masters(masters, as_new, recursive, false);
            }
        }
    }
}

/// Port of `TwbMainRecord.ReportRequiredMasters`: a record of the game
/// master or of the hardcoded file needs that file only; an override needs
/// the file of the record it overrides.
pub(crate) fn main_record_report_required_masters(
    record: &MainRecordImpl,
    masters: &mut FilesSet,
    as_new: bool,
    recursive: bool,
    initial: bool,
) {
    let generation = record.base.e_report_masters_gen.load(Ordering::Relaxed);
    if generation & 0x7FFF_FFFF == masters.generation && (!recursive || generation & 0x8000_0000 != 0) {
        return;
    }
    if let Some(file) = record.file_impl() {
        let states = file.get_file_states();
        if states.contains(FileState::fsIsHardcoded) || states.contains(FileState::fsIsGameMaster) {
            masters.add(file);
            return;
        }
    }
    if !as_new && let Some(file) = record_reference_file(record) {
        masters.add(file);
    }
    container_report_required_masters(record, masters, as_new, recursive, initial);
}

/// Port of `TwbMainRecord.GetReferenceFile`: the file the FormID of the
/// record belongs to.
fn record_reference_file(record: &MainRecordImpl) -> Option<Arc<FileImpl>> {
    let file = record.file_impl()?;
    file.master_for_file_id_or_self(record.mr_struct().form_id.file_id())
}

/// Port of `GetReferenceFile`: for a record the file its FormID belongs
/// to, for a file the file itself, else the reference file of the
/// container.
pub(crate) fn reference_file(element: &ElementRef) -> Option<Arc<FileImpl>> {
    let element_impl = element.as_element_impl()?;
    if let Some(record) = element_impl.main_record_impl() {
        return record_reference_file(&record);
    }
    if let Some(file) = element_impl.file_impl() {
        return Some(file);
    }
    reference_file(&element.get_container()?)
}

/// The masters a copy of `element` needs (`ReportRequiredMasters`, as
/// `CopyInto` asks for them).
pub fn required_masters(element: &ElementRef, as_new: bool) -> Vec<Arc<FileImpl>> {
    let mut masters = FilesSet::new();
    if let Some(element) = element.as_element_impl() {
        element.report_required_masters(&mut masters, as_new, true, false);
    }
    masters.files
}

/// The first part of `AddRequiredMasters`: the masters of `masters` the
/// target lacks, in load order. A master that loads after the target is
/// refused.
pub fn missing_masters(masters: &[Arc<FileImpl>], target: &Arc<FileImpl>) -> Result<Vec<Arc<FileImpl>>, EditError> {
    // A sorted list of the names without duplicates, as `TStringList` with
    // `Sorted` and `dupIgnore` keeps them (ignoring case).
    let mut missing: Vec<Arc<FileImpl>> = Vec::new();
    let mut by_load_order = masters.to_vec();
    by_load_order.sort_by_key(|file| file.load_order());
    for file in by_load_order {
        if !missing
            .iter()
            .any(|known| known.get_name().eq_ignore_ascii_case(&file.get_name()))
        {
            missing.push(file);
        }
    }
    let own: Vec<String> = target.masters().iter().map(|master| master.get_name()).collect();
    missing.retain(|file| {
        !own.iter().any(|name| name.eq_ignore_ascii_case(&file.get_name()))
            && !file.get_name().eq_ignore_ascii_case(&target.get_name())
    });
    missing.sort_by_key(|file| file.get_name().to_ascii_lowercase());
    for file in &missing {
        if file.load_order() >= target.load_order() {
            return Err(format!(
                "The required master \"{}\" can not be added to \"{}\" as it has a higher load order",
                file.get_name(),
                target.get_name()
            ));
        }
    }
    missing.sort_by_key(|file| file.load_order());
    Ok(missing)
}

/// Port of `AddRequiredMasters`: the masters the copy needs that the target
/// file lacks are added, in load order; they must all load before it.
pub fn add_required_masters(masters: &FilesSet, target: &Arc<FileImpl>) -> Result<(), EditError> {
    let missing = missing_masters(masters.files(), target)?;
    if missing.is_empty() {
        return Ok(());
    }
    target.add_masters_if_missing_for_copy(&missing)
}

/// Port of `TwbElement.CopyInto`: the masters the element needs are added
/// to the file, then the element is copied with its containers; with
/// `deep_copy` a record takes its child group along.
/// UPSTREAM-QUIRK: only the masters of the element itself are reported,
/// not those of the records of a child group copied with it.
pub(crate) fn copy_into(
    element: &dyn ElementImpl,
    file: &FileRef,
    args: &CopyArgs,
) -> Result<Option<ElementRef>, EditError> {
    let target = file
        .as_element_impl()
        .and_then(ElementImpl::file_impl)
        .ok_or_else(|| "the target is not a loaded file".to_owned())?;
    let mut masters = FilesSet::new();
    element.report_required_masters(&mut masters, args.as_new, true, false);
    add_required_masters(&masters, &target)?;
    let copy_args = CopyArgs {
        deep_copy: true,
        allow_overwrite: false,
        ..args.clone()
    };
    if args.deep_copy
        && let Some(record) = element.main_record_impl()
        && let Some(group) = record.child_group()
    {
        let group: ElementRef = group;
        let result = copy_element_to_file(&group, &target, &copy_args)?;
        return Ok(result
            .and_then(|group| group.as_element_impl()?.group_record_impl())
            .and_then(|group| group.children_of())
            .map(|record| record as ElementRef));
    }
    let this = element
        .self_element_ref()
        .ok_or_else(|| "the element is being created".to_owned())?;
    copy_element_to_file(&this, &target, &copy_args)
}

/// Port of `wbCopyElementToFile`: the containers of the element are copied
/// into the file first (without their contents), then the element is added
/// to the copy of its container.
pub fn copy_element_to_file(
    source: &ElementRef,
    file: &Arc<FileImpl>,
    args: &CopyArgs,
) -> Result<Option<ElementRef>, EditError> {
    set_copy_is_running(copy_is_running() + 1);
    let result = (|| {
        let Some(mut container) = source.get_container() else {
            return Ok(Some(file.clone() as ElementRef));
        };
        if let Some(record) = container.as_element_impl().and_then(ElementImpl::main_record_impl) {
            container = record.highest_override_visible_for_file(file);
        }
        let container_args = CopyArgs {
            as_new: false,
            deep_copy: false,
            allow_overwrite: false,
            ..args.clone()
        };
        match copy_element_to_file(&container, file, &container_args)? {
            Some(target) => target.add_if_missing(source, args),
            None => Ok(None),
        }
    })();
    set_copy_is_running(copy_is_running() - 1);
    result
}

/// Port of `wbCopyElementToRecord`: the element copied into the record,
/// with the containers between them.
pub fn copy_element_to_record(
    source: &ElementRef,
    record: &Arc<MainRecordImpl>,
    as_new: bool,
    deep_copy: bool,
) -> Result<Option<ElementRef>, EditError> {
    if source.get_element_type() == ElementType::etMainRecord {
        if source.get_element_id() == record.get_element_id() {
            return Ok(None);
        }
        return Ok(Some(record.clone() as ElementRef));
    }
    let container = source
        .get_container()
        .ok_or_else(|| "[wbCopyElementToRecord] not Assigned(Container)".to_owned())?;
    match copy_element_to_record(&container, record, false, false)? {
        Some(target) => target.add_if_missing(
            source,
            &CopyArgs {
                as_new,
                deep_copy,
                ..CopyArgs::default()
            },
        ),
        None => Ok(None),
    }
}

/// Port of `TwbElement.AddIfMissing`: the update counter around
/// `AddIfMissingInternal`.
pub(crate) fn add_if_missing(
    element: &dyn ElementImpl,
    source: &ElementRef,
    args: &CopyArgs,
) -> Result<Option<ElementRef>, EditError> {
    super::edit::begin_update(element);
    let result = element.add_if_missing_internal(source, args);
    super::edit::end_update(element);
    result
}

/// Port of `RemovePrefix`.
fn remove_prefix(text: &str, prefix: &str) -> String {
    if !text.is_empty()
        && !prefix.is_empty()
        && text.len() >= prefix.len()
        && text.is_char_boundary(prefix.len())
        && text[..prefix.len()].eq_ignore_ascii_case(prefix)
    {
        text[prefix.len()..].to_owned()
    } else {
        text.to_owned()
    }
}

/// Port of `RemoveSuffix`.
fn remove_suffix(text: &str, suffix: &str) -> String {
    let start = text.len().wrapping_sub(suffix.len());
    if !text.is_empty()
        && !suffix.is_empty()
        && text.len() >= suffix.len()
        && text.is_char_boundary(start)
        && text[start..].eq_ignore_ascii_case(suffix)
    {
        text[..start].to_owned()
    } else {
        text.to_owned()
    }
}

// ----- the file -----

impl FileImpl {
    /// Port of `GetMasterForFileID(aFileID, aNew, True)` without the complex
    /// FileIDs: the file itself for its own FileID, else the master at the
    /// slot.
    pub(crate) fn master_for_file_id_or_self(
        self: &Arc<Self>,
        file_id: crate::interface::form_id::FileID,
    ) -> Option<Arc<FileImpl>> {
        if self.is_new_record(file_id) {
            return Some(self.clone());
        }
        let slot = file_id.full_slot();
        self.masters().get(usize::try_from(slot).ok()?).cloned()
    }

    /// The `AddMastersIfMissing(sl)` call of `AddRequiredMasters`, with the
    /// defaults of its other arguments (sorted, not silent).
    fn add_masters_if_missing_for_copy(self: &Arc<Self>, missing: &[Arc<FileImpl>]) -> Result<(), EditError> {
        let names: Vec<String> = missing.iter().map(|file| file.get_name()).collect();
        self.add_masters_if_missing(&names, true, false)
    }

    /// Port of `GetGroupBySignature`: the top level group with the label.
    pub fn group_by_signature(&self, signature: Signature) -> Option<Arc<GroupRecordImpl>> {
        self.container.elements().iter().find_map(|element| {
            let group = element.as_element_impl()?.group_record_impl()?;
            (group.group_type() == 0 && group.gr_struct().label_signature() == signature).then_some(group)
        })
    }

    /// Port of `TwbFile.AddIfMissingInternal`: the top level group of the
    /// source group, created when the file lacks it; with a deep copy, its
    /// records are copied too.
    pub(crate) fn add_if_missing_internal_impl(
        self: &Arc<Self>,
        source: &ElementRef,
        args: &CopyArgs,
    ) -> Result<Option<ElementRef>, EditError> {
        if !self.is_element_editable() {
            return Err(format!("File \"{}\" is not editable", self.get_name()));
        }
        let group = source
            .as_element_impl()
            .and_then(ElementImpl::group_record_impl)
            .ok_or_else(|| "Only group records can be added to files".to_owned())?;
        if group.group_type() != 0 {
            return Err("Only top level group records can be added to files".to_owned());
        }
        let signature = group.gr_struct().label_signature();
        if wb_get_group_order(signature) < 0 {
            return Err(format!("{signature}is not a valid group label"));
        }
        let result = match self.group_by_signature(signature) {
            Some(existing) => existing,
            None => GroupRecordImpl::create_top(self, signature),
        };
        if args.deep_copy {
            let deep = CopyArgs {
                deep_copy: true,
                ..args.clone()
            };
            for child in group.container.elements() {
                result.add_if_missing(&child, &deep)?;
            }
        }
        Ok(Some(result as ElementRef))
    }
}

// ----- the groups -----

/// The signatures of the placed records a group of the persistent (8) or
/// visible-when-distant (10) children of a cell may hold.
const PLACED: [&[u8; 4]; 11] = [
    b"REFR", b"ACHR", b"ACRE", b"PGRE", b"PMIS", b"PARW", b"PBEA", b"PFLA", b"PCON", b"PBAR", b"PHZD",
];

impl GroupRecordImpl {
    /// The group of the children with the type and label among the
    /// elements, as the loops of `AddIfMissingInternal` look for it.
    fn child_group_of_type(&self, group_type: i32, label: Option<u32>) -> Option<Arc<GroupRecordImpl>> {
        self.container.elements().iter().find_map(|element| {
            let group = element.as_element_impl()?.group_record_impl()?;
            (group.group_type() == group_type && label.is_none_or(|label| group.group_label() == label))
                .then_some(group)
        })
    }

    /// The copy of every element of `source` into `target`, for a deep copy.
    fn copy_children(target: &ElementRef, source: &Arc<GroupRecordImpl>, args: &CopyArgs) -> Result<(), EditError> {
        if !args.deep_copy {
            return Ok(());
        }
        target.begin_update();
        let result = (|| {
            for child in source.container.elements() {
                target.add_if_missing(&child, args)?;
            }
            Ok(())
        })();
        target.end_update();
        result
    }

    /// The case of a group of the children of a record (`WRLD`, `CELL`,
    /// `DIAL`, `QUST`): the record copied as an override (or new) with a
    /// deep copy, then its child group.
    fn copy_children_group(
        self: &Arc<Self>,
        file: &Arc<FileImpl>,
        source: &Arc<GroupRecordImpl>,
        group_type: i32,
        args: &CopyArgs,
    ) -> Result<Option<ElementRef>, EditError> {
        let record = source
            .children_of()
            .ok_or_else(|| format!("Can't find record for {}", source.get_name()))?;
        let record = record.highest_override_visible_for_file(file);
        let record_args = CopyArgs {
            deep_copy: true,
            allow_overwrite: false,
            ..args.clone()
        };
        let record_ref: ElementRef = record;
        let target = self.add_if_missing_internal_impl(&record_ref, &record_args)?;
        let Some(target) = target.and_then(|target| target.as_element_impl()?.main_record_impl()) else {
            return Ok(None);
        };
        let group = match target.child_group() {
            Some(group) => group,
            None => GroupRecordImpl::create_child(self, group_type, &target),
        };
        let group_ref: ElementRef = group;
        GroupRecordImpl::copy_children(&group_ref, source, args)?;
        Ok(Some(group_ref))
    }

    /// The case of a block or sub-block group: found by type and label, or
    /// made, then filled with a deep copy.
    fn copy_block_group(
        self: &Arc<Self>,
        source: &Arc<GroupRecordImpl>,
        group_type: i32,
        label: u32,
        args: &CopyArgs,
    ) -> Result<Option<ElementRef>, EditError> {
        let group = match self.child_group_of_type(group_type, Some(label)) {
            Some(group) => group,
            None => GroupRecordImpl::create_labelled(self, group_type, label),
        };
        let group_ref: ElementRef = group;
        GroupRecordImpl::copy_children(&group_ref, source, args)?;
        Ok(Some(group_ref))
    }

    /// Port of `TwbGroupRecord.AddIfMissingInternal`: the group or record
    /// the source is, found in or added to this group by the rules of its
    /// group type.
    pub(crate) fn add_if_missing_internal_impl(
        self: &Arc<Self>,
        source: &ElementRef,
        args: &CopyArgs,
    ) -> Result<Option<ElementRef>, EditError> {
        let Some(file) = self.file.upgrade() else {
            return Ok(None);
        };
        let label_signature = self.gr_struct().label_signature();
        let source_group = source.as_element_impl().and_then(ElementImpl::group_record_impl);
        let source_record = source.as_element_impl().and_then(ElementImpl::main_record_impl);
        let cant_add_top = |group: &Arc<GroupRecordImpl>| {
            format!(
                "Can't add {} to top level group with signature {label_signature}",
                group.get_name()
            )
        };
        let cant_add = |name: String| format!("Can't add {name} to {}", self.get_name());
        let only_main_records = || format!("Only main records can be added to {}", self.get_name());
        let wrong_signature = |record: &Arc<MainRecordImpl>| {
            format!(
                "Can't add main record with signature {} to {}",
                record.get_signature(),
                self.get_name()
            )
        };
        match self.group_type() {
            0 => {
                let child_groups: &[(&[u8; 4], i32)] = &[(b"DIAL", 7), (b"WRLD", 1)];
                for (signature, group_type) in child_groups {
                    if label_signature == Signature::new(signature)
                        && let Some(group) = &source_group
                    {
                        if group.group_type() != *group_type {
                            return Err(cant_add_top(group));
                        }
                        return self.copy_children_group(&file, group, *group_type, args);
                    }
                }
                if vwd_as_quest_children()
                    && label_signature == Signature::new(b"QUST")
                    && let Some(group) = &source_group
                {
                    if group.group_type() != 10 {
                        return Err(cant_add_top(group));
                    }
                    return self.copy_children_group(&file, group, 10, args);
                }
                if label_signature == Signature::new(b"CELL")
                    && let Some(group) = &source_group
                {
                    if group.group_type() != 2 || group.group_label() > 9 {
                        return Err(cant_add_top(group));
                    }
                    return self.copy_block_group(group, 2, group.group_label(), args);
                }
                let record =
                    source_record.ok_or_else(|| "Only main records can be added to top level groups".to_owned())?;
                if record.get_signature() != label_signature {
                    return Err(format!(
                        "Can't add main record with signature {} to top level group with signature {label_signature}",
                        record.get_signature()
                    ));
                }
                self.copy_main_record(&file, source, &record, args)
            }
            1 => {
                if let Some(group) = &source_group {
                    if group.group_type() == 4 {
                        return self.copy_block_group(group, 4, group.group_label(), args);
                    }
                    if group.group_type() != 6 {
                        return Err(cant_add_top(group));
                    }
                    return self.copy_children_group(&file, group, 6, args);
                }
                let record = source_record.ok_or_else(only_main_records)?;
                let signature = record.get_signature();
                if signature != Signature::new(b"CELL") && signature != Signature::new(b"ROAD") {
                    return Err(wrong_signature(&record));
                }
                if args.as_new {
                    return Err(format!("Can't copy record {} as new record.", record.get_name()));
                }
                self.copy_main_record(&file, source, &record, args)
            }
            2 | 4 => {
                let Some(group) = &source_group else {
                    return Err(cant_add(source.get_name()));
                };
                if group.group_type() != self.group_type() + 1 {
                    return Err(cant_add(group.get_name()));
                }
                self.copy_block_group(group, group.group_type(), group.group_label(), args)
            }
            3 | 5 => {
                if let Some(group) = &source_group {
                    if group.group_type() != 6 {
                        return Err(cant_add_top(group));
                    }
                    return self.copy_children_group(&file, group, 6, args);
                }
                let record = source_record.ok_or_else(only_main_records)?;
                if record.get_signature() != Signature::new(b"CELL") {
                    return Err(wrong_signature(&record));
                }
                if args.as_new {
                    return Err(format!("Can't copy record {} as new record.", record.get_name()));
                }
                self.copy_main_record(&file, source, &record, args)
            }
            6 => {
                let Some(group) = &source_group else {
                    return Err(cant_add(source.get_name()));
                };
                if !matches!(group.group_type(), 8..=10) {
                    return Err(cant_add(group.get_name()));
                }
                let target = match self.child_group_of_type(group.group_type(), None) {
                    Some(existing) => existing,
                    None => {
                        let cell = self
                            .children_of()
                            .ok_or_else(|| format!("Can't find record for {}", self.get_name()))?;
                        GroupRecordImpl::create_child(self, group.group_type(), &cell)
                    }
                };
                let target: ElementRef = target;
                GroupRecordImpl::copy_children(&target, group, args)?;
                Ok(Some(target))
            }
            7 => {
                let record = source_record.ok_or_else(only_main_records)?;
                if record.get_signature() != Signature::new(b"INFO") {
                    return Err(wrong_signature(&record));
                }
                self.copy_main_record(&file, source, &record, args)
            }
            8..=10 => {
                if vwd_as_quest_children()
                    && let Some(group) = &source_group
                {
                    if group.group_type() != 7 {
                        return Err(cant_add_top(group));
                    }
                    return self.copy_children_group(&file, group, 7, args);
                }
                let record = source_record.ok_or_else(only_main_records)?;
                let signature = record.get_signature();
                let is = |expected: &[u8; 4]| signature == Signature::new(expected);
                let placed = PLACED.iter().any(|expected| is(expected));
                let quest_child =
                    vwd_as_quest_children() && self.group_type() == 10 && (is(b"DLBR") || is(b"DIAL") || is(b"SCEN"));
                let temporary_child = self.group_type() == 9 && (is(b"PGRD") || is(b"LAND") || is(b"NAVM"));
                if !placed && !(quest_child || temporary_child) {
                    return Err(wrong_signature(&record));
                }
                self.copy_main_record(&file, source, &record, args)
            }
            other => Err(format!(
                "TwbGroupRecord.AddIfMissingInternal is not implemented for GroupType {other}"
            )),
        }
    }

    /// Port of `CopyMainRecord` in `TwbGroupRecord.AddIfMissingInternal`:
    /// the record in this group with the FormID of the source (an override)
    /// or a new FormID, made when the file lacks it and filled from the
    /// source with a deep copy.
    fn copy_main_record(
        self: &Arc<Self>,
        file: &Arc<FileImpl>,
        source_element: &ElementRef,
        source: &Arc<MainRecordImpl>,
        args: &CopyArgs,
    ) -> Result<Option<ElementRef>, EditError> {
        let source_ref: ElementRef = source.clone();
        if is_starfield() {
            if source.get_load_order_form_id().to_cardinal() == 0x25 {
                return Ok(None);
            }
            if contains_reflection(&*source_ref) {
                progress(&format!(
                    "Error adding [{}] to [{}]: Source contains Reflection and can not be copied",
                    source_element.get_full_path(),
                    self.get_full_path()
                ));
                return Ok(None);
            }
        }
        if contains_unmapped_form_id(&*source_ref) {
            let states = file.get_file_states();
            let game_master_first = file
                .masters()
                .first()
                .is_some_and(|master| master.get_file_states().contains(FileState::fsIsGameMaster));
            if !states.contains(FileState::fsIsGameMaster)
                && !states.contains(FileState::fsIsHardcoded)
                && !game_master_first
            {
                progress(&format!(
                    "Error adding [{}] to [{}]: Source contains Unmapped FormID and can not be copied into a module which does not have the game master as its first master",
                    source_element.get_full_path(),
                    self.get_full_path()
                ));
                return Ok(None);
            }
        }
        let mut existing = None;
        let form_id = if args.as_new {
            file.new_form_id()?
        } else {
            let load_order_form_id = source.get_load_order_form_id();
            existing = file.contained_record_by_load_order_form_id(load_order_form_id);
            match &existing {
                Some(record) => record.get_fixed_form_id(),
                None => File::load_order_form_id_to_file_form_id(&**file, load_order_form_id, true)?,
            }
        };
        let (result, is_new) = match existing {
            Some(existing) => {
                if !args.allow_overwrite || !args.deep_copy {
                    return Ok(Some(existing as ElementRef));
                }
                return Err(format!(
                    "overwriting {} in {} (wbCanOverwrite) is not ported yet",
                    existing.get_name(),
                    file.get_name()
                ));
            }
            None => (MainRecordImpl::create_new(self, source.get_signature(), form_id)?, true),
        };
        if args.deep_copy {
            if source.get_is_partial_form() || (is_new && allow_make_partial() && result.get_can_be_partial()) {
                return Err(format!(
                    "copying {} as a partial form (MakePartialForm) is not ported yet",
                    source.get_name()
                ));
            }
            if !(result.get_is_partial_form() || result.get_is_deleted()) {
                result.assign(ASSIGN_THIS, Some(source_element), false);
            }
            let editor_id = source.get_editor_id();
            if !result.get_is_deleted() && !editor_id.is_empty() {
                let editor_id = remove_prefix(&editor_id, &args.prefix_remove);
                let editor_id = remove_suffix(&editor_id, &args.suffix_remove);
                if crate::interface::globals::begin_internal_edit(true) {
                    let set = result.set_editor_id(&format!("{}{editor_id}{}", args.prefix, args.suffix));
                    crate::interface::globals::end_internal_edit();
                    set?;
                }
            }
        }
        if !args.as_new && source.get_is_master() {
            let result_order = file.load_order();
            let source_file = source.file_impl();
            let source_order = source_file.as_ref().map_or(-1, |file| file.load_order());
            let compare_load = file.get_file_states().contains(FileState::fsIsCompareLoad);
            if result_order < source_order || (result_order == source_order && !compare_load) {
                source.you_got_a_master(&result);
            }
        }
        // `if Assigned(Result) and (csRefsBuild in Result._File.ContainerStates)
        // then Result.BuildRef`.
        if file.refs_built() {
            result.build_ref();
        }
        Ok(Some(result as ElementRef))
    }
}

// ----- the main records -----

impl MainRecordImpl {
    /// Port of `TwbMainRecord.AddIfMissingInternal`: the member of the
    /// record the source is, made from the source when the record lacks it,
    /// or taking its value.
    pub(crate) fn add_if_missing_internal_impl(
        self: &Arc<Self>,
        source: &ElementRef,
        args: &CopyArgs,
    ) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && !edit_allowed() {
            return Err(format!("{} can not be assigned.", self.get_name()));
        }
        let Some(mr_def) = self.mr_def.clone() else {
            return Ok(None);
        };
        let source_signature = source.get_has_signature();
        let known = mr_def.known_sub_record_signatures();
        if self.get_is_deleted() {
            let base_record = crate::interface::globals::game_mode() >= crate::interface::globals::GameMode::gmFO4
                && source_signature == Some(known[KnownSubRecord::ksrBaseRecord.ord()]);
            if !base_record {
                return Ok(None);
            }
        }
        if self.get_is_partial_form() && source_signature != Some(known[KnownSubRecord::ksrEditorID.ord()]) {
            return Ok(None);
        }
        self.do_init();
        let sort_order = source.get_sort_order();
        let result = self.container.element_by_sort_order(sort_order);
        match result {
            Some(result) => {
                result.assign(ASSIGN_THIS, Some(source), !args.deep_copy);
                Ok(Some(result))
            }
            None => {
                self.assign(sort_order, Some(source), !args.deep_copy);
                let result = self.container.element_by_sort_order(sort_order);
                super::edit::sort_sub_records_of(&**self);
                Ok(result)
            }
        }
    }
}
