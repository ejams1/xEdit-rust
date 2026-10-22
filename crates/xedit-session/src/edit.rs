// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The commands that change the structure of a plugin: `elements.add`,
//! `elements.remove`, `records.copy` (xEdit's "Copy as override into" and
//! "Copy as new record into", `CopyInto`) and `records.delete` (xEdit's
//! "Remove").

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::ElementImpl;
use xedit_core::implementation::FileImpl;
use xedit_core::implementation::copy::{
    CanOverwriteAction, add_required_masters_of, copy_element_to_record, copy_into_file, copy_into_masters,
    missing_masters, required_masters, set_can_overwrite_callback,
};
use xedit_core::interface::globals::{edit_allowed, set_edit_allowed};
use xedit_core::interface::misc::Variant;
use xedit_core::interface::types::ASSIGN_THIS;
use xedit_core::interface::{Container, CopyArgs, Element, ElementRef, MainRecord, MainRecordRef};

use crate::commands::{ElementNode, RecordSummary, node_of, parse_form_id, summary_of};
use crate::{CommandError, Registry, Session};

/// The element a path names in a record (the record itself for no path).
fn element_of(record: &MainRecordRef, path: Option<&str>) -> Result<ElementRef, CommandError> {
    match path.filter(|path| !path.is_empty()) {
        Some(path) => record.get_element_by_path(path).ok_or_else(|| {
            CommandError::new(
                "unknown_element",
                format!("{} has no element at {path}", record.get_name()),
            )
        }),
        None => Ok(record.clone()),
    }
}

/// Runs a check of the editor (`IsEditable`, `IsRemovable`) as it would go
/// with editing allowed, so that a dry run reports what the edit would do.
pub(crate) fn with_edit_allowed<T>(check: impl FnOnce() -> T) -> T {
    let allowed = edit_allowed();
    set_edit_allowed(true);
    let result = check();
    set_edit_allowed(allowed);
    result
}

fn edit_failed(error: String) -> CommandError {
    CommandError::new("edit_failed", error)
}

/// The `Abort` of `GeneralProgress` once the wait form was cancelled.
fn cancelled() -> CommandError {
    CommandError::new("cancelled", "the operation was cancelled")
}

/// `elements.add`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsAddRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// What to add, as xEdit's `Add`: the name or signature of a member of
    /// the record (`EDID`, `Model`), the signature of a child record of a
    /// cell, topic, worldspace or quest (`REFR`, `INFO`, `CELL[3,-2]` or
    /// `CELL[P]` for a worldspace cell), or anything for an entry of an
    /// array (a position adds at that position).
    pub name: String,
    /// Path of the container inside the record to add to, with `\` between
    /// the names; the record itself when omitted.
    pub path: Option<String>,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// Report the container, but add nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `elements.add`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ElementsAddResponse {
    /// Full path of the element added, or of the container for a dry run,
    /// in the upstream `FullPath` shape (` \ [00] File.esm \ [1] GRUP ...
    /// \ [0] Record`, `wbImplementation.pas:19641-19652`).
    pub path: String,
    /// The element added (or found, when the member was there already).
    pub element: Option<ElementNode>,
    /// The record added, when the element is a new record (a child of a
    /// cell, topic, worldspace or quest).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<RecordSummary>,
    /// The plugin that now holds a change to save.
    pub file: String,
}

fn elements_add(session: &mut Session, request: ElementsAddRequest) -> Result<ElementsAddResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("elements.add")?;
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let record_ref: ElementRef = record.clone();
    let container: ElementRef = match request.path.as_deref().filter(|path| !path.is_empty()) {
        Some(path) => record.get_element_by_path(path).ok_or_else(|| {
            CommandError::new(
                "unknown_element",
                format!("{} has no element at {path}", record.get_name()),
            )
        })?,
        None => record_ref.clone(),
    };
    let Some(container_ref) = container.as_container() else {
        return Err(CommandError::new(
            "invalid_params",
            format!("{} has no elements to add to", container.get_full_path()),
        ));
    };
    let file = record.get_file().map(|file| file.get_name()).unwrap_or_default();
    if request.dry_run {
        return Ok(ElementsAddResponse {
            path: container.get_full_path(),
            element: None,
            record: None,
            file,
        });
    }
    let added = container_ref.add(&request.name, true).map_err(edit_failed)?;
    let Some(added) = added else {
        return Err(CommandError::new(
            "edit_failed",
            format!("{} can not add {}", container.get_full_path(), request.name),
        ));
    };
    let new_record: Option<MainRecordRef> = added.clone().into_main_record();
    let file = added.get_file().map(|file| file.get_name()).unwrap_or(file);
    Ok(ElementsAddResponse {
        path: added.get_full_path(),
        element: Some(node_of(&added, Some(1))),
        record: new_record.as_ref().map(summary_of),
        file,
    })
}

/// `elements.remove`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsRemoveRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Path of the element inside the record, with `\` between the names.
    pub path: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// Report the element, but remove nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `elements.remove`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ElementsRemoveResponse {
    /// Full path the element had.
    pub path: String,
    /// The element as it was.
    pub element: ElementNode,
    /// Whether the element was removed; false for a dry run.
    pub removed: bool,
    /// The plugin that now holds a change to save.
    pub file: String,
}

fn elements_remove(
    session: &mut Session,
    request: ElementsRemoveRequest,
) -> Result<ElementsRemoveResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("elements.remove")?;
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let element = record.get_element_by_path(&request.path).ok_or_else(|| {
        CommandError::new(
            "unknown_element",
            format!("{} has no element at {}", record.get_name(), request.path),
        )
    })?;
    // The editor offers `Remove` only for an element that is removable.
    if !with_edit_allowed(|| element.get_is_removable()) {
        return Err(CommandError::new(
            "not_removable",
            format!("{} can not be removed", element.get_full_path()),
        ));
    }
    let response = ElementsRemoveResponse {
        path: element.get_full_path(),
        element: node_of(&element, Some(0)),
        removed: !request.dry_run,
        file: record.get_file().map(|file| file.get_name()).unwrap_or_default(),
    };
    if !request.dry_run {
        element.remove();
    }
    Ok(response)
}

/// `records.copy`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsCopyRequest {
    /// Load order FormID of the record to copy as hexadecimal digits.
    pub form_id: String,
    /// Plugin to copy into.
    pub to: String,
    /// Plugin the record to copy is seen from (its version there is
    /// copied); the last loaded plugin when omitted.
    pub from: Option<String>,
    /// Copy as a new record with a new FormID of the target, not as an
    /// override.
    #[serde(default)]
    pub as_new: bool,
    /// Copy the records of the child group too (the placed records of a
    /// cell, the responses of a topic, the cells of a worldspace).
    #[serde(default)]
    pub deep: bool,
    /// Copy over a record the target has already, as xEdit's "Copy as
    /// override (with overwriting) into..." and "Deep copy as override
    /// (with overwriting) into..." do (`aAllowOverwrite`), answering "Yes
    /// to All" to the question whether to overwrite each one. Ignored with
    /// `as_new`, as upstream ignores it.
    #[serde(default)]
    pub overwrite: bool,
    /// With `overwrite`: answer "Yes to All" to xEdit's question whether to
    /// remove the record of the target instead of copying a source that is
    /// flagged as deleted (`coDelete`); without it the deleted record is
    /// copied over, the default answer of the dialog.
    #[serde(default)]
    pub remove_deleted: bool,
    /// Text removed from the start of the editor ID of the copy.
    #[serde(default)]
    pub prefix_remove: String,
    /// Text removed from the end of the editor ID of the copy.
    #[serde(default)]
    pub suffix_remove: String,
    /// Text put before the editor ID of the copy.
    #[serde(default)]
    pub prefix: String,
    /// Text put after the editor ID of the copy.
    #[serde(default)]
    pub suffix: String,
    /// Copy the record as the wrapper of its leveled list (`AsWrapper`, "Copy
    /// as wrapper"): a new record of the target with `wrapper_editor_id`,
    /// then an override of the record whose single leveled list entry
    /// points to the wrapper with level and count 1 (`CopyInto`'s AsWrapper
    /// branch). The GUI offers the item for `LVLI`, `LVLC`, `LVSP` and
    /// `LVLN` only.
    #[serde(default)]
    pub as_wrapper: bool,
    /// The editor ID of the wrapper copy, as the GUI's InputQuery asks it.
    /// Required with `as_wrapper`, and different from the record's own.
    pub wrapper_editor_id: Option<String>,
    /// Adjust the spawn rate of the copied leveled list (`AsSpawnRate`, "Copy
    /// as spawn rate override"): `AfterCopyAdjustSpawnRate` writes every
    /// leveled list entry nine times with the counts 1, 1, 2, 2, 2, 2, 2,
    /// 3, 3.
    #[serde(default)]
    pub as_spawn_rate: bool,
    /// Copy as a disabled override (`AfterCopyDisable`, "Copy as disabled
    /// override into"): the copy's `InitiallyDisabled` flag is set. The
    /// release's callback does nothing else.
    #[serde(default)]
    pub disable: bool,
    /// Report what the copy needs, but copy nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `records.copy`: the response.
#[derive(Serialize, JsonSchema)]
pub struct RecordsCopyResponse {
    /// The record copied.
    pub source: RecordSummary,
    /// The copy in the target plugin; for a dry run the override that
    /// exists already, if any. None when the copy removed the record of the
    /// target (`remove_deleted`).
    pub copy: Option<RecordSummary>,
    /// Whether the target had the record already (copied over only with
    /// `overwrite`).
    pub existed: bool,
    /// What the copy over existing records did, as xEdit logs it:
    /// `Overwriting "<target>" with "<source>"`, `Removing "<target>"` and
    /// `Skipping "<source>"`, for the record and, with `deep`, every record
    /// of its child group the target had.
    pub overwrites: Vec<String>,
    /// The masters the copy needs.
    pub required_masters: Vec<String>,
    /// The required masters the target lacks, which the copy adds.
    pub missing_masters: Vec<String>,
    /// The plugin copied into.
    pub file: String,
    /// The wrapper record made by `as_wrapper`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrapper: Option<RecordSummary>,
}

fn names(files: &[Arc<FileImpl>]) -> Vec<String> {
    files.iter().map(|file| file.get_name()).collect()
}

/// Clears `_wbCanOverwriteCallback` when the copy is done, as the
/// `finally` of `CopyInto` does.
struct OverwriteCallbackGuard;

impl Drop for OverwriteCallbackGuard {
    fn drop(&mut self) {
        set_can_overwrite_callback(None);
    }
}

/// The counts `AfterCopyAdjustSpawnRate` writes for every leveled list
/// entry, nine copies in that order.
const SPAWN_RATE_COUNTS: [i64; 9] = [1, 1, 2, 2, 2, 2, 2, 3, 3];

/// Port of `AfterCopyAdjustSpawnRate` (`xeMainForm.pas`): every entry of
/// `Leveled List Entries` whose name is `LVLO - Leveled List Entry` is
/// added again once per count of [`SPAWN_RATE_COUNTS`], and the copy takes
/// that count.
///
/// UPSTREAM-QUIRK: the elements of the array are `TwbSubRecordStruct`s
/// named `Leveled List Entry` (`TwbSubRecordStruct.GetName`), so the
/// `SameText` of the release never matches and it adds nothing: in 4.1.5q
/// "Copy as spawn rate override" copies the record as a plain override.
/// The port follows the release.
fn after_copy_adjust_spawn_rate(record: &MainRecordRef) -> Result<(), CommandError> {
    let entries = record
        .get_element_by_name("Leveled List Entries")
        .ok_or_else(|| edit_failed(format!("{} has no Leveled List Entries", record.get_name())))?;
    let source_entries = {
        let container = entries
            .as_container()
            .ok_or_else(|| edit_failed(format!("{} has no Leveled List Entries container", record.get_name())))?;
        let mut source_entries = Vec::new();
        for index in 0..container.get_element_count() {
            if let Some(entry) = container.get_element(index)
                && entry.get_name().eq_ignore_ascii_case("LVLO - Leveled List Entry")
            {
                source_entries.push(entry);
            }
        }
        source_entries
    };
    for entry in source_entries {
        for count in SPAWN_RATE_COUNTS {
            let Some(copy) = entries.assign(ASSIGN_THIS, Some(&entry), false) else {
                continue;
            };
            if let Some(count_element) = copy
                .as_container()
                .and_then(|container| container.get_element_by_name("Count"))
            {
                let _ = count_element.set_native_value(Variant::Int(count));
            }
        }
    }
    Ok(())
}

fn edit_ref(name: impl Into<String>) -> CommandError {
    CommandError::new("edit_failed", name)
}

/// Whether the release offers the wrapper and spawn rate items for the
/// record (`mniNavCopyAsWrapper.Visible`): the leveled list signatures, not
/// in Fallout 76 or Morrowind.
fn wrapper_offered(signature: &str) -> bool {
    matches!(signature, "LVLB" | "LVLC" | "LVLI" | "LVLN" | "LVLP" | "LVSC" | "LVSP")
        && !matches!(
            xedit_core::interface::globals::game_mode(),
            xedit_core::interface::globals::GameMode::gmFO76 | xedit_core::interface::globals::GameMode::gmTES3
        )
}

/// The `AsWrapper` branch of `CopyInto`: a new record of the target with the
/// wrapper's editor ID, then an override of the record cleared and given a
/// single leveled list entry that points to the wrapper with level and
/// count 1.
fn copy_as_wrapper(
    source: &MainRecordRef,
    source_ref: &ElementRef,
    target: &Arc<FileImpl>,
    wrapper_editor_id: &str,
) -> Result<(MainRecordRef, MainRecordRef), CommandError> {
    if wrapper_editor_id.is_empty() || wrapper_editor_id.eq_ignore_ascii_case(&source.get_editor_id()) {
        return Err(CommandError::new(
            "invalid_params",
            "as_wrapper needs a wrapper_editor_id different from the record's own",
        ));
    }
    let original_editor_id = source.get_editor_id();
    let wrapper_args = CopyArgs {
        as_new: true,
        deep_copy: true,
        ..CopyArgs::default()
    };
    let wrapper = copy_into_file(source_ref, target, &wrapper_args)
        .map_err(edit_failed)?
        .and_then(|copy| copy.into_main_record())
        .ok_or_else(|| edit_ref("the wrapper record was not copied"))?;
    wrapper.set_editor_id(wrapper_editor_id).map_err(edit_failed)?;
    let override_args = CopyArgs {
        as_new: false,
        deep_copy: false,
        ..CopyArgs::default()
    };
    let copied = copy_into_file(source_ref, target, &override_args)
        .map_err(edit_failed)?
        .and_then(|copy| copy.into_main_record())
        .ok_or_else(|| edit_ref("the wrapper's override was not copied"))?;
    copied.assign(ASSIGN_THIS, None, false);
    let entries = match copied.get_element_by_name("Leveled List Entries") {
        Some(entries) => entries,
        None => copied
            .add("Leveled List Entries", true)
            .map_err(edit_failed)?
            .ok_or_else(|| edit_ref("the wrapper's override did not add Leveled List Entries"))?,
    };
    let entries_container = entries
        .as_container()
        .ok_or_else(|| edit_ref("the wrapper's override has no Leveled List Entries container"))?;
    let entry = entries_container
        .get_element(0)
        .ok_or_else(|| edit_ref("the wrapper's override has no leveled list entry"))?;
    let lvlo = if xedit_core::interface::globals::is_oblivion() {
        entry
    } else {
        entry
            .as_container()
            .and_then(|container| container.get_element(0))
            .ok_or_else(|| edit_ref("the wrapper's leveled list entry has no union member"))?
    };
    let lvlo_container = lvlo
        .as_container()
        .ok_or_else(|| edit_ref("the wrapper's leveled list entry is no structure"))?;
    let reference = lvlo_container
        .get_element(2)
        .ok_or_else(|| edit_ref("the wrapper's leveled list entry has no reference"))?;
    reference
        .set_edit_value(&wrapper.get_edit_value())
        .map_err(edit_failed)?;
    if let Some(count) = lvlo_container.get_element_by_name("Count") {
        count.set_edit_value("1").map_err(edit_failed)?;
    }
    if let Some(level) = lvlo_container.get_element_by_name("Level") {
        level.set_edit_value("1").map_err(edit_failed)?;
    }
    copied.set_editor_id(&original_editor_id).map_err(edit_failed)?;
    Ok((copied, wrapper))
}

fn records_copy(session: &mut Session, request: RecordsCopyRequest) -> Result<RecordsCopyResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("records.copy")?;
    if request.as_wrapper && request.as_spawn_rate {
        return Err(CommandError::new(
            "invalid_params",
            "as_wrapper and as_spawn_rate are different copies of the release's CopyInto",
        ));
    }
    if (request.as_wrapper || request.as_spawn_rate) && (request.as_new || request.deep) {
        return Err(CommandError::new(
            "invalid_params",
            "as_wrapper and as_spawn_rate assert not AsNew and not DeepCopy upstream",
        ));
    }
    if request.disable && (request.as_new || request.deep || request.as_wrapper || request.as_spawn_rate) {
        return Err(CommandError::new(
            "invalid_params",
            "disable is AfterCopyDisable, which upstream allows only without AsNew, AsWrapper, AsSpawnRate and DeepCopy",
        ));
    }
    if request.as_wrapper
        && request
            .wrapper_editor_id
            .as_deref()
            .is_none_or(|editor_id| editor_id.is_empty())
    {
        return Err(CommandError::new(
            "invalid_params",
            "as_wrapper needs wrapper_editor_id",
        ));
    }
    let source = session.record(&request.form_id, request.from.as_deref())?;
    let target = session.file(Some(&request.to))?;
    if (request.as_wrapper || request.as_spawn_rate) && !wrapper_offered(&source.get_signature().to_string()) {
        return Err(CommandError::new(
            "invalid_params",
            format!(
                "{} is not a leveled list the release offers the item for (LVLB, LVLC, LVLI, LVLN, LVLP, LVSC, LVSP)",
                source.get_name()
            ),
        ));
    }
    let source_ref: ElementRef = source.clone();
    let required = copy_into_masters(&source_ref, &target, request.as_new, request.deep);
    let missing = missing_masters(&required, &target).map_err(edit_failed)?;
    let existing = if request.as_new {
        None
    } else {
        target.contained_record_by_load_order_form_id(source.get_load_order_form_id())
    };
    let mut response = RecordsCopyResponse {
        source: summary_of(&source),
        copy: existing.clone().map(|record| summary_of(&(record as MainRecordRef))),
        existed: existing.is_some(),
        overwrites: Vec::new(),
        required_masters: names(&required),
        missing_masters: names(&missing),
        file: target.get_name(),
        wrapper: None,
    };
    if !with_edit_allowed(|| target.get_is_editable()) {
        return Err(CommandError::new(
            "not_editable",
            format!("File \"{}\" is not editable", target.get_name()),
        ));
    }
    if request.dry_run {
        return Ok(response);
    }
    add_required_masters_of(&required, &target).map_err(edit_failed)?;
    if request.as_wrapper {
        // `CopyInto`'s `AsWrapper` branch, with the GUI's "single element"
        // case (its prefix and suffix are empty).
        let (override_record, wrapper) = copy_as_wrapper(
            &source,
            &source_ref,
            &target,
            request.wrapper_editor_id.as_deref().unwrap_or_default(),
        )?;
        response.copy = Some(summary_of(&override_record));
        response.wrapper = Some(summary_of(&wrapper));
        return Ok(response);
    }
    let overwrite = request.overwrite && !request.as_new && !request.as_spawn_rate;
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let _guard = OverwriteCallbackGuard;
    if overwrite {
        // The answers of `CopyInto`'s dialogs: "Yes to All" to the
        // overwrite, and to the removal of a deleted source's target when
        // `remove_deleted` (else "No to All"), with its log lines.
        let remove_deleted = request.remove_deleted;
        let log = log.clone();
        set_can_overwrite_callback(Some(Arc::new(move |target: &ElementRef, source: &ElementRef| {
            let deleted = source.as_main_record().is_some_and(|record| record.get_is_deleted());
            let action = if deleted && remove_deleted {
                CanOverwriteAction::Delete
            } else {
                CanOverwriteAction::Copy
            };
            let line = match action {
                CanOverwriteAction::Copy => {
                    format!(
                        "Overwriting \"{}\" with \"{}\"",
                        target.get_full_path(),
                        source.get_full_path()
                    )
                }
                CanOverwriteAction::Delete => format!("Removing \"{}\"", target.get_full_path()),
                CanOverwriteAction::Skip => format!("Skipping \"{}\"", source.get_full_path()),
            };
            xedit_core::interface::misc::progress(&line);
            log.lock().unwrap().push(line);
            action
        })));
    }
    let args = CopyArgs {
        as_new: request.as_new,
        deep_copy: request.deep,
        prefix_remove: request.prefix_remove,
        suffix_remove: request.suffix_remove,
        prefix: request.prefix,
        suffix: request.suffix,
        allow_overwrite: overwrite,
    };
    let copy = copy_into_file(&source_ref, &target, &args).map_err(edit_failed)?;
    response.overwrites = std::mem::take(&mut *log.lock().unwrap());
    // With `deep` the result is the child group, whose record is the copy.
    let copy = copy.and_then(|copy| {
        let group_owner = copy
            .as_element_impl()
            .and_then(|element| element.group_record_impl())
            .and_then(|group| group.children_of());
        match group_owner {
            Some(record) => Some(record as MainRecordRef),
            None => copy.into_main_record(),
        }
    });
    let removed = copy.as_ref().is_some_and(|record| record.get_container().is_none());
    let copy = match copy {
        Some(copy) if !removed => copy,
        Some(_) => {
            response.copy = None;
            return Ok(response);
        }
        None if overwrite => {
            response.copy = None;
            return Ok(response);
        }
        None => {
            return Err(edit_failed(format!(
                "{} was not copied into {}",
                source.get_name(),
                target.get_name()
            )));
        }
    };
    if request.as_spawn_rate {
        after_copy_adjust_spawn_rate(&copy)?;
    }
    if request.disable {
        copy.set_is_initially_disabled(true);
    }
    response.copy = Some(summary_of(&copy));
    Ok(response)
}

/// `records.delete`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsDeleteRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Plugin whose version of the record is removed; the only loaded
    /// plugin when omitted.
    pub file: Option<String>,
    /// Report the record, but remove nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `records.delete`: the response.
#[derive(Serialize, JsonSchema)]
pub struct RecordsDeleteResponse {
    /// The record removed.
    pub record: RecordSummary,
    /// The records of its child group, which go with it.
    pub child_records: usize,
    /// Whether the record was removed; false for a dry run.
    pub removed: bool,
}

/// The main records below a group, at any depth.
fn count_records(element: &ElementRef) -> usize {
    let Some(container) = element.as_container() else {
        return 0;
    };
    (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .map(|child| match child.get_element_type() {
            xedit_core::interface::ElementType::etMainRecord => 1,
            xedit_core::interface::ElementType::etGroupRecord => count_records(&child),
            _ => 0,
        })
        .sum()
}

fn records_delete(session: &mut Session, request: RecordsDeleteRequest) -> Result<RecordsDeleteResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("records.delete")?;
    let file = session.file(request.file.as_deref())?;
    let form_id = parse_form_id(&request.form_id)?;
    let record = file.contained_record_by_load_order_form_id(form_id).ok_or_else(|| {
        CommandError::new(
            "unknown_record",
            format!(
                "{} has no record with FormID {}",
                file.get_name(),
                form_id.to_string(false)
            ),
        )
    })?;
    let record_ref: MainRecordRef = record.clone();
    if !with_edit_allowed(|| record_ref.get_is_removable()) {
        return Err(CommandError::new(
            "not_removable",
            format!("{} can not be removed", record_ref.get_name()),
        ));
    }
    let child_records = record
        .child_group()
        .map_or(0, |group| count_records(&(group as ElementRef)));
    let response = RecordsDeleteResponse {
        record: summary_of(&record_ref),
        child_records,
        removed: !request.dry_run,
    };
    if !request.dry_run {
        record_ref.remove();
    }
    Ok(response)
}

/// `records.set_deleted`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsSetDeletedRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Plugin whose version of the record changes; the only loaded plugin
    /// when omitted.
    pub file: Option<String>,
    /// `true` marks the record deleted (`TwbMainRecord.Delete`: it keeps
    /// its place, its child group and the records that refer to it, loses
    /// its contents, and from Fallout 4 on a placed record keeps its base
    /// record); `false` undeletes it (it takes the contents of its master
    /// again). Defaults to `true`.
    #[serde(default = "default_true")]
    pub deleted: bool,
    /// Report the record, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

fn default_true() -> bool {
    true
}

/// `records.set_deleted`: the response.
#[derive(Serialize, JsonSchema)]
pub struct RecordsSetDeletedResponse {
    /// The record, after the change.
    pub record: RecordSummary,
    /// The deleted flag before the call.
    pub was_deleted: bool,
    /// Whether the flag changed (false for a dry run and when the record
    /// already had the asked state). The plugin then needs saving.
    pub changed: bool,
}

/// `records.set_deleted`: `IsDeleted := deleted` of the record, as the
/// script function `SetIsDeleted` sets it (`TwbMainRecord.SetIsDeleted`,
/// which runs `Delete` or the undelete). This is not `records.delete`,
/// which removes the record from the plugin (`Remove`).
fn records_set_deleted(
    session: &mut Session,
    request: RecordsSetDeletedRequest,
) -> Result<RecordsSetDeletedResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("records.set_deleted")?;
    let file = session.file(request.file.as_deref())?;
    let form_id = parse_form_id(&request.form_id)?;
    let record = file.contained_record_by_load_order_form_id(form_id).ok_or_else(|| {
        CommandError::new(
            "unknown_record",
            format!(
                "{} has no record with FormID {}",
                file.get_name(),
                form_id.to_string(false)
            ),
        )
    })?;
    let record_ref: MainRecordRef = record.clone();
    if !with_edit_allowed(|| record_ref.get_is_editable()) {
        return Err(CommandError::new(
            "not_editable",
            format!("{} can not be edited", record_ref.get_name()),
        ));
    }
    let was_deleted = record.get_is_deleted();
    let changed = was_deleted != request.deleted && !request.dry_run;
    if changed {
        crate::clean::run_on_large_stack(|| record.set_is_deleted(request.deleted))?.map_err(edit_failed)?;
    }
    Ok(RecordsSetDeletedResponse {
        record: summary_of(&record_ref),
        was_deleted,
        changed,
    })
}

/// `elements.clear`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsClearRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Path of the element inside the record, with `\` between the names.
    pub path: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// Report the element, but clear nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `elements.clear`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ElementsClearResponse {
    /// Full path of the element.
    pub path: String,
    /// The element as it was.
    pub element: ElementNode,
    /// Whether the element is clearable (`IsClearable`); the command
    /// refuses one that is not, as the menu item is hidden for it.
    pub clearable: bool,
    /// Whether the elements were removed; false for a dry run.
    pub cleared: bool,
    /// The plugin that now holds a change to save.
    pub file: String,
}

/// `elements.clear`: `Element.Clear` of the focused element of the view
/// (`mniViewClear`): the elements of a variable-size array that are all
/// removable go from the last. Every other element is a no-op upstream and
/// not clearable.
fn elements_clear(session: &mut Session, request: ElementsClearRequest) -> Result<ElementsClearResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("elements.clear")?;
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let element = element_of(&record, Some(&request.path))?;
    let clearable = with_edit_allowed(|| element.get_is_clearable());
    if !clearable {
        return Err(CommandError::new(
            "not_clearable",
            format!("{} can not be cleared", element.get_full_path()),
        ));
    }
    let response = ElementsClearResponse {
        path: element.get_full_path(),
        element: node_of(&element, Some(0)),
        clearable,
        cleared: !request.dry_run,
        file: record.get_file().map(|file| file.get_name()).unwrap_or_default(),
    };
    if !request.dry_run {
        element.clear();
    }
    Ok(response)
}

/// `elements.mark_modified`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsMarkModifiedRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Path of the element inside the record, with `\` between the names;
    /// the record itself when omitted.
    pub path: Option<String>,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// Report the element, but mark nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `elements.mark_modified`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ElementsMarkModifiedResponse {
    /// Full path of the element.
    pub path: String,
    /// Whether the elements were marked; false for a dry run.
    pub modified: bool,
    /// The plugin whose change the save now writes.
    pub file: String,
}

/// `elements.mark_modified`: `Element.MarkModifiedRecursive(AllElementTypes)`
/// over the selection (`mniNavMarkModified`, and `mniRefByMarkModified` for
/// a selection of referencing records): every element below it, and it,
/// is marked so the save writes it.
fn elements_mark_modified(
    session: &mut Session,
    request: ElementsMarkModifiedRequest,
) -> Result<ElementsMarkModifiedResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("elements.mark_modified")?;
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let element = element_of(&record, request.path.as_deref())?;
    let response = ElementsMarkModifiedResponse {
        path: element.get_full_path(),
        modified: !request.dry_run,
        file: record.get_file().map(|file| file.get_name()).unwrap_or_default(),
    };
    if !request.dry_run {
        element.mark_modified_recursive();
    }
    Ok(response)
}

/// `files.mark_modified_headers`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesMarkModifiedHeadersRequest {
    /// Report the files, but mark nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `files.mark_modified_headers`: the response.
#[derive(Serialize, JsonSchema)]
pub struct FilesMarkModifiedHeadersResponse {
    /// The plugins whose header was (or would be) marked.
    pub files: Vec<String>,
    /// Whether the headers were marked; false for a dry run.
    pub modified: bool,
}

/// `files.mark_modified_headers`: `MarkHeaderModified` of every editable
/// file without ONAM (`mniMarkallfileswithoutONAMasmodified`): the save
/// writes the header of each, which is how a plugin gets its ONAM list.
fn files_mark_modified_headers(
    session: &mut Session,
    request: FilesMarkModifiedHeadersRequest,
) -> Result<FilesMarkModifiedHeadersResponse, CommandError> {
    session.mode()?;
    let mut files = Vec::new();
    for file in &session.files {
        // UPSTREAM-QUIRK: the release's item marks the headers of every
        // plugin in a loop without a progress message, so the check is the
        // port's own. It is before the header is marked, so a cancelled
        // command leaves the files after it as they were.
        xedit_core::interface::wait_form::poll_cancel().map_err(|_| cancelled())?;
        if !with_edit_allowed(|| file.get_is_editable()) || file.get_is_not_plugin() {
            continue;
        }
        let Some(header) = file.header() else {
            return Err(edit_ref(format!(
                "Unexpected error reading file \"{}\"",
                file.file_name()
            )));
        };
        if header.get_element_by_path("ONAM").is_some() {
            continue;
        }
        files.push(file.get_name());
        if !request.dry_run {
            header.set_modified(true);
        }
    }
    Ok(FilesMarkModifiedHeadersResponse {
        files,
        modified: !request.dry_run,
    })
}

/// The direction of `elements.change_member`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemberDirection {
    #[default]
    Next,
    Previous,
}

/// `elements.change_member`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsChangeMemberRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Path of the element inside the record, with `\` between the names.
    pub path: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// The member the element switches to.
    #[serde(default)]
    pub direction: MemberDirection,
    /// Report whether the member can change, but change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `elements.change_member`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ElementsChangeMemberResponse {
    /// Full path of the element after the change (before it for a dry run).
    pub path: String,
    /// The element after the change; the element itself when its member
    /// does not change.
    pub element: ElementNode,
    /// Whether the member changed.
    pub changed: bool,
    /// Whether the element can change its member (`CanChangeMember`),
    /// before the change.
    pub can_change: bool,
    /// The plugin that now holds a change to save.
    pub file: String,
}

/// `elements.change_member`: `Element.NextMember` and
/// `Element.PreviousMember` (`mniViewNextMember` and
/// `mniViewPreviousMember`): the element of a union takes the member after
/// or before its own, wrapping around, and keeps what its members share.
fn elements_change_member(
    session: &mut Session,
    request: ElementsChangeMemberRequest,
) -> Result<ElementsChangeMemberResponse, CommandError> {
    // The view hides Next Member and Previous Member in the translate mode
    // (`mniViewNextMember.Visible := not wbTranslationMode and ...`).
    crate::commands::refuse_in_translate_mode("elements.change_member")?;
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let element = element_of(&record, Some(&request.path))?;
    let can_change = with_edit_allowed(|| element.get_can_change_member());
    let file = record.get_file().map(|file| file.get_name()).unwrap_or_default();
    if request.dry_run {
        return Ok(ElementsChangeMemberResponse {
            path: element.get_full_path(),
            element: node_of(&element, Some(0)),
            changed: false,
            can_change,
            file,
        });
    }
    let member = match request.direction {
        MemberDirection::Next => element.next_member(),
        MemberDirection::Previous => element.previous_member(),
    }
    .unwrap_or_else(|| element.clone());
    let changed = member.get_element_id() != element.get_element_id();
    Ok(ElementsChangeMemberResponse {
        path: member.get_full_path(),
        element: node_of(&member, Some(0)),
        changed,
        can_change,
        file,
    })
}

/// How `elements.copy_to_records` copies the element.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CopyToRecordsMode {
    /// `PerformDrop`: the element of the same path in every target record
    /// takes the value of the source (`mniViewCopyToSelectedRecords`).
    #[default]
    Assign,
    /// `wbCopyElementToRecord(aSource, aRecord, False, True)`: the element
    /// is copied into every target record, with the containers it needs
    /// (`mniViewCopyMultipleToSelectedRecords`).
    Copy,
}

/// `elements.copy_to_records`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsCopyToRecordsRequest {
    /// Load order FormID of the source record as hexadecimal digits.
    pub form_id: String,
    /// Plugin the source record is seen from; the last loaded plugin when
    /// omitted.
    pub file: Option<String>,
    /// Path of the source element, with `\` between the names; the record
    /// itself when omitted.
    pub path: Option<String>,
    /// Plugins of the target records: the other view columns of the same
    /// record. Their record has the FormID of the source.
    pub targets: Vec<String>,
    /// Assign the same-path element, or copy the element into the record.
    #[serde(default)]
    pub mode: CopyToRecordsMode,
    /// With `mode`: `copy`, copy as a new record with a new FormID.
    #[serde(default)]
    pub as_new: bool,
    /// With `mode`: `copy`, copy the containers of the element (`aDeepCopy`,
    /// True in the menu item).
    #[serde(default = "default_true")]
    pub deep: bool,
    /// With `mode`: `copy` and a source record that has no element at
    /// `path`, remove the target's element at the path instead, as the
    /// menu item does for a row the focused record does not have.
    #[serde(default)]
    pub remove_missing: bool,
    /// Report what each target would do, but change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// What `elements.copy_to_records` did with one target.
#[derive(Serialize, JsonSchema)]
pub struct CopyToRecordTarget {
    /// The plugin of the target record.
    pub file: String,
    /// What the copy did: `assigned`, `copied`, `removed` or `skipped`.
    pub action: String,
    /// Full path of the target element, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Why the target was skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `elements.copy_to_records`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ElementsCopyToRecordsResponse {
    /// The record the source element is in.
    pub source: RecordSummary,
    /// What was done with each target, in the order given.
    pub targets: Vec<CopyToRecordTarget>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

fn target_record(
    session: &Session,
    form_id: xedit_core::interface::FormID,
    file: &str,
) -> Result<Option<MainRecordRef>, CommandError> {
    let target_file = session.file(Some(file))?;
    Ok(target_file
        .contained_record_by_load_order_form_id(form_id)
        .map(|record| record as MainRecordRef))
}

/// `elements.copy_to_records`: the element of the source record copied into
/// the records of the target plugins, the other view columns
/// (`mniViewCopyToSelectedRecords` through `PerformDrop`'s
/// `TargetElement.Assign(wbAssignThis, SourceElement, False)`, and
/// `mniViewCopyMultipleToSelectedRecords` through `wbCopyElementToRecord`).
fn elements_copy_to_records(
    session: &mut Session,
    request: ElementsCopyToRecordsRequest,
) -> Result<ElementsCopyToRecordsResponse, CommandError> {
    // The release's handler checks `wbEditAllowed` only, not the translate
    // mode.
    let source_record = session.record(&request.form_id, request.file.as_deref())?;
    let source_element = element_of(&source_record, request.path.as_deref());
    let source_element = match source_element {
        Ok(element) => Some(element),
        Err(error) if matches!(request.mode, CopyToRecordsMode::Copy) && request.remove_missing => {
            let _ = error;
            None
        }
        Err(error) => return Err(error),
    };
    let form_id = source_record.get_load_order_form_id();
    let required = source_element.as_ref().map(|element| {
        required_masters(
            element,
            request.as_new && matches!(request.mode, CopyToRecordsMode::Copy),
        )
    });
    let mut targets = Vec::new();
    for name in &request.targets {
        // UPSTREAM-QUIRK: the release's item copies the element into every
        // selected record in a loop without a progress message, so the check
        // is the port's own. It is before the target is changed, so a
        // cancelled command leaves the targets after it as they were.
        xedit_core::interface::wait_form::poll_cancel().map_err(|_| cancelled())?;
        let Some(target) = target_record(session, form_id, name)? else {
            targets.push(CopyToRecordTarget {
                file: name.clone(),
                action: "skipped".to_owned(),
                path: None,
                reason: Some("the plugin has no record with this FormID".to_owned()),
            });
            continue;
        };
        let target_path = request.path.as_deref();
        match request.mode {
            CopyToRecordsMode::Assign => {
                let target_element = element_of(&target, target_path);
                let Ok(target_element) = target_element else {
                    targets.push(CopyToRecordTarget {
                        file: name.clone(),
                        action: "skipped".to_owned(),
                        path: None,
                        reason: Some("the record has no element at the path".to_owned()),
                    });
                    continue;
                };
                let source = source_element.as_ref().expect("assign mode has a source");
                if target_element.get_element_id() == source.get_element_id() {
                    targets.push(CopyToRecordTarget {
                        file: name.clone(),
                        action: "skipped".to_owned(),
                        path: Some(target_element.get_full_path()),
                        reason: Some("the source and the target are the same element".to_owned()),
                    });
                    continue;
                }
                if !request.dry_run {
                    if let Some(required) = &required {
                        let target_file = session.file(Some(name))?;
                        let missing = missing_masters(required, &target_file).map_err(edit_failed)?;
                        add_required_masters_of(&missing, &target_file).map_err(edit_failed)?;
                    }
                    target_element.assign(ASSIGN_THIS, Some(source), false);
                }
                targets.push(CopyToRecordTarget {
                    file: name.clone(),
                    action: "assigned".to_owned(),
                    path: Some(target_element.get_full_path()),
                    reason: None,
                });
            }
            CopyToRecordsMode::Copy => match &source_element {
                Some(source) => {
                    if request.dry_run {
                        targets.push(CopyToRecordTarget {
                            file: name.clone(),
                            action: "copied".to_owned(),
                            path: Some(source.get_full_path()),
                            reason: None,
                        });
                    } else {
                        let target_impl = target.as_element_impl().and_then(ElementImpl::main_record_impl);
                        let result = match target_impl {
                            Some(target_impl) => {
                                copy_element_to_record(source, &target_impl, request.as_new, request.deep)
                            }
                            None => Err("the target record has no implementation".to_owned()),
                        };
                        match result {
                            Ok(Some(_)) => targets.push(CopyToRecordTarget {
                                file: name.clone(),
                                action: "copied".to_owned(),
                                path: Some(source.get_full_path()),
                                reason: None,
                            }),
                            Ok(None) => targets.push(CopyToRecordTarget {
                                file: name.clone(),
                                action: "skipped".to_owned(),
                                path: None,
                                reason: Some("the copy gave no element".to_owned()),
                            }),
                            Err(error) => targets.push(CopyToRecordTarget {
                                file: name.clone(),
                                action: "skipped".to_owned(),
                                path: None,
                                reason: Some(error),
                            }),
                        }
                    }
                }
                None => {
                    let target_element = target_path.and_then(|path| target.get_element_by_path(path));
                    match target_element {
                        Some(element) if with_edit_allowed(|| element.get_is_removable()) => {
                            if !request.dry_run {
                                element.remove();
                            }
                            targets.push(CopyToRecordTarget {
                                file: name.clone(),
                                action: "removed".to_owned(),
                                path: Some(element.get_full_path()),
                                reason: None,
                            });
                        }
                        _ => targets.push(CopyToRecordTarget {
                            file: name.clone(),
                            action: "skipped".to_owned(),
                            path: None,
                            reason: Some("the record has no removable element at the path".to_owned()),
                        }),
                    }
                }
            },
        }
    }
    Ok(ElementsCopyToRecordsResponse {
        source: summary_of(&source_record),
        targets,
        dry_run: request.dry_run,
    })
}

/// `elements.remove_from_records`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsRemoveFromRecordsRequest {
    /// Load order FormID of the source record as hexadecimal digits.
    pub form_id: String,
    /// Plugin the source record is seen from; the last loaded plugin when
    /// omitted.
    pub file: Option<String>,
    /// Path of the focused row's element, with `\` between the names.
    pub path: String,
    /// Plugins of the target records: the other view columns of the same
    /// record.
    pub targets: Vec<String>,
    /// Report what each target would do, but remove nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `elements.remove_from_records`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ElementsRemoveFromRecordsResponse {
    /// The record the focused row's element is in.
    pub source: RecordSummary,
    /// What was done with each target, in the order given.
    pub targets: Vec<CopyToRecordTarget>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

/// `elements.remove_from_records`: `Element.Remove` of the focused row's
/// element in every record of the view (`mniViewRemoveFromSelected`), each
/// one that is removable.
fn elements_remove_from_records(
    session: &mut Session,
    request: ElementsRemoveFromRecordsRequest,
) -> Result<ElementsRemoveFromRecordsResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("elements.remove_from_records")?;
    let source_record = session.record(&request.form_id, request.file.as_deref())?;
    let form_id = source_record.get_load_order_form_id();
    let mut targets = Vec::new();
    for name in &request.targets {
        // UPSTREAM-QUIRK: the release's item removes the element from every
        // selected record in a loop without a progress message, so the check
        // is the port's own. It is before the element is removed, so a
        // cancelled command leaves the targets after it as they were.
        xedit_core::interface::wait_form::poll_cancel().map_err(|_| cancelled())?;
        let Some(target) = target_record(session, form_id, name)? else {
            targets.push(CopyToRecordTarget {
                file: name.clone(),
                action: "skipped".to_owned(),
                path: None,
                reason: Some("the plugin has no record with this FormID".to_owned()),
            });
            continue;
        };
        let Some(element) = target.get_element_by_path(&request.path) else {
            targets.push(CopyToRecordTarget {
                file: name.clone(),
                action: "skipped".to_owned(),
                path: None,
                reason: Some("the record has no element at the path".to_owned()),
            });
            continue;
        };
        if !with_edit_allowed(|| element.get_is_removable()) {
            targets.push(CopyToRecordTarget {
                file: name.clone(),
                action: "skipped".to_owned(),
                path: Some(element.get_full_path()),
                reason: Some("the element is not removable".to_owned()),
            });
            continue;
        }
        let path = element.get_full_path();
        if !request.dry_run {
            element.remove();
        }
        targets.push(CopyToRecordTarget {
            file: name.clone(),
            action: "removed".to_owned(),
            path: Some(path),
            reason: None,
        });
    }
    Ok(ElementsRemoveFromRecordsResponse {
        source: summary_of(&source_record),
        targets,
        dry_run: request.dry_run,
    })
}

/// `records.set_persistent`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsSetPersistentRequest {
    /// Load order FormID of the reference as hexadecimal digits.
    pub form_id: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// `true` makes the reference persistent (the persistent children group
    /// of its cell), `false` temporary. Defaults to `true`.
    #[serde(default = "default_true")]
    pub persistent: bool,
    /// Report the record, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `records.set_persistent`: the response.
#[derive(Serialize, JsonSchema)]
pub struct RecordsSetPersistentResponse {
    /// The record after the change.
    pub record: RecordSummary,
    /// The flag before the call.
    pub was_persistent: bool,
    /// Whether the flag changed (false for a dry run and when the record
    /// already had the asked state).
    pub changed: bool,
}

fn reference_of(session: &Session, form_id: &str, file: Option<&str>) -> Result<MainRecordRef, CommandError> {
    let record = session.record(form_id, file)?;
    if record.get_signature().to_string() != "REFR" {
        return Err(CommandError::new(
            "not_reference",
            format!("{} is not a reference (REFR)", record.get_name()),
        ));
    }
    Ok(record)
}

/// `records.set_persistent`: `IsPersistent := persistent` of a reference
/// (`mniNavCellChildPers` and `mniNavCellChildTemp`): the flag changes and
/// the reference moves to the persistent, temporary or visible distant
/// children group of its cell (`UpdateCellChildGroup`).
fn records_set_persistent(
    session: &mut Session,
    request: RecordsSetPersistentRequest,
) -> Result<RecordsSetPersistentResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("records.set_persistent")?;
    let record = reference_of(session, &request.form_id, request.file.as_deref())?;
    let was_persistent = record.get_is_persistent();
    let changed = was_persistent != request.persistent && !request.dry_run;
    if changed {
        record.set_is_persistent(request.persistent);
    }
    Ok(RecordsSetPersistentResponse {
        record: summary_of(&record),
        was_persistent,
        changed,
    })
}

/// `records.set_visible_when_distant`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsSetVisibleWhenDistantRequest {
    /// Load order FormID of the reference as hexadecimal digits.
    pub form_id: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// `true` sets the Visible When Distant flag, `false` clears it.
    /// Defaults to `true`.
    #[serde(default = "default_true")]
    pub visible_when_distant: bool,
    /// Report the record, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `records.set_visible_when_distant`: the response.
#[derive(Serialize, JsonSchema)]
pub struct RecordsSetVisibleWhenDistantResponse {
    /// The record after the change.
    pub record: RecordSummary,
    /// The flag before the call.
    pub was_visible_when_distant: bool,
    /// Whether the flag changed (false for a dry run and when the record
    /// already had the asked state).
    pub changed: bool,
}

/// `records.set_visible_when_distant`: `IsVisibleWhenDistant := value` of a
/// reference (`mniNavCellChildVWD` and `mniNavCellChildNotVWD`, and
/// `mniRefByVWD` and `mniRefByNotVWD` for the referencing records): the
/// flag changes and the reference moves to the visible distant or temporary
/// children group of its cell.
fn records_set_visible_when_distant(
    session: &mut Session,
    request: RecordsSetVisibleWhenDistantRequest,
) -> Result<RecordsSetVisibleWhenDistantResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("records.set_visible_when_distant")?;
    let record = reference_of(session, &request.form_id, request.file.as_deref())?;
    let was_visible_when_distant = record.get_is_visible_when_distant();
    let changed = was_visible_when_distant != request.visible_when_distant && !request.dry_run;
    if changed {
        record.set_is_visible_when_distant(request.visible_when_distant);
    }
    Ok(RecordsSetVisibleWhenDistantResponse {
        record: summary_of(&record),
        was_visible_when_distant,
        changed,
    })
}

/// Adds the structure commands to the registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "elements.add",
        "Add a member, an array entry or a child record to a record (xEdit's Add).",
        true,
        elements_add,
    );
    registry.register(
        "elements.remove",
        "Remove an element of a record by path (xEdit's Remove).",
        true,
        elements_remove,
    );
    registry.register(
        "elements.clear",
        "Remove every element of a clearable array value (xEdit's Clear).",
        true,
        elements_clear,
    );
    registry.register(
        "elements.mark_modified",
        "Mark an element and everything below it modified (MarkModifiedRecursive), so the save writes them.",
        true,
        elements_mark_modified,
    );
    registry.register(
        "elements.change_member",
        "Switch the union member of an element to the next or previous member (Next Member, Previous Member).",
        true,
        elements_change_member,
    );
    registry.register(
        "elements.copy_to_records",
        "Copy the focused element into the records of the other view columns, by assign or by copy (PerformDrop, wbCopyElementToRecord).",
        true,
        elements_copy_to_records,
    );
    registry.register(
        "elements.remove_from_records",
        "Remove the focused row's element from every record of the view (mniViewRemoveFromSelected).",
        true,
        elements_remove_from_records,
    );
    registry.register(
        "files.mark_modified_headers",
        "Mark the header of every editable file without ONAM modified (mniMarkallfileswithoutONAMasmodified).",
        true,
        files_mark_modified_headers,
    );
    registry.register(
        "records.copy",
        "Copy a record into a plugin as an override or as a new record (CopyInto).",
        true,
        records_copy,
    );
    registry.register(
        "records.delete",
        "Remove a record and its child group from a plugin (xEdit's Remove).",
        true,
        records_delete,
    );
    registry.register(
        "records.set_deleted",
        "Mark a record deleted (TwbMainRecord.Delete: it keeps its place, child group and references) or undelete it, as SetIsDeleted does; unlike records.delete it stays in the plugin.",
        true,
        records_set_deleted,
    );
    registry.register(
        "records.set_persistent",
        "Make a reference persistent or temporary, moving it to the child group of its cell (Cell Child > Persistent/Temporary).",
        true,
        records_set_persistent,
    );
    registry.register(
        "records.set_visible_when_distant",
        "Set or clear the Visible When Distant flag of a reference, moving it to the child group of its cell (Cell Child > Visible When Distant, and the referencing records' item).",
        true,
        records_set_visible_when_distant,
    );
}
