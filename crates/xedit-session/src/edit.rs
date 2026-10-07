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
use xedit_core::implementation::copy::{missing_masters, required_masters};
use xedit_core::interface::globals::{edit_allowed, set_edit_allowed};
use xedit_core::interface::{CopyArgs, Element, ElementRef, FileRef, MainRecordRef};

use crate::commands::{ElementNode, RecordSummary, node_of, parse_form_id, summary_of};
use crate::{CommandError, Registry, Session};

/// Runs a check of the editor (`IsEditable`, `IsRemovable`) as it would go
/// with editing allowed, so that a dry run reports what the edit would do.
fn with_edit_allowed<T>(check: impl FnOnce() -> T) -> T {
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
    /// Full path of the element added, or of the container for a dry run.
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
    /// exists already, if any.
    pub copy: Option<RecordSummary>,
    /// Whether the target had the record already (an override is not
    /// copied over).
    pub existed: bool,
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

fn records_copy(session: &mut Session, request: RecordsCopyRequest) -> Result<RecordsCopyResponse, CommandError> {
    let source = session.record(&request.form_id, request.from.as_deref())?;
    let target = session.file(Some(&request.to))?;
    let source_ref: ElementRef = source.clone();
    let required = required_masters(&source_ref, request.as_new);
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
    let args = CopyArgs {
        as_new: request.as_new,
        deep_copy: request.deep,
        prefix_remove: request.prefix_remove,
        suffix_remove: request.suffix_remove,
        prefix: request.prefix,
        suffix: request.suffix,
        allow_overwrite: false,
    };
    let target_ref: FileRef = target.clone();
    let copy = source_ref.copy_into(&target_ref, &args).map_err(edit_failed)?;
    let copy = copy.and_then(|copy| copy.into_main_record()).ok_or_else(|| {
        edit_failed(format!(
            "{} was not copied into {}",
            source.get_name(),
            target.get_name()
        ))
    })?;
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
}
