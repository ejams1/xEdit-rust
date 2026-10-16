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
use xedit_core::implementation::FileImpl;
use xedit_core::implementation::copy::{
    CanOverwriteAction, copy_into_file, copy_into_masters, missing_masters, set_can_overwrite_callback,
};
use xedit_core::interface::globals::{edit_allowed, set_edit_allowed};
use xedit_core::interface::{CopyArgs, Element, ElementRef, MainRecord, MainRecordRef};

use crate::commands::{ElementNode, RecordSummary, node_of, parse_form_id, summary_of};
use crate::{CommandError, Registry, Session};

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

fn records_copy(session: &mut Session, request: RecordsCopyRequest) -> Result<RecordsCopyResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("records.copy")?;
    let source = session.record(&request.form_id, request.from.as_deref())?;
    let target = session.file(Some(&request.to))?;
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
    xedit_core::implementation::copy::add_required_masters_of(&required, &target).map_err(edit_failed)?;
    let overwrite = request.overwrite && !request.as_new;
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
}
