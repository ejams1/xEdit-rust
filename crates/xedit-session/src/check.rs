// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavCheckForErrorsClick)

//! `files.check`: "Check for Errors" on files or records of the session.
//! The check itself is `xedit_analysis::check`.
//!
//! The `-CheckForErrors` tool mode checks the last loaded plugin and loads
//! the plugins without the internal edits of the load
//! (`wbAllowInternalEdit` off, `xeInit.pas`); `xedit check` loads that way
//! (`commands::set_check_on_load`). A session loaded otherwise checks the
//! records as the edit mode's menu item does, after the fixes of the load.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_analysis::check::{self, CheckError, CheckTarget, CheckedRecord};
use xedit_core::interface::globals::{
    build_refs, display_shorter_names, edit_allowed, set_display_shorter_names, set_edit_allowed,
};
use xedit_core::interface::{Element, ElementRef, File, FileState};

use crate::{CommandError, Registry, Session};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckRequest {
    /// Names of loaded files to check, each as its file node of the
    /// navigation tree, in the order given. When neither `files` nor
    /// `records` is given, the plugins loaded with `--load`, in their order
    /// (with one plugin, what `-CheckForErrors <plugin>` checks).
    #[serde(default)]
    pub files: Vec<String>,
    /// Load order FormIDs of main records to check after the files, each
    /// as its record node.
    #[serde(default)]
    pub records: Vec<String>,
    /// The loaded file whose version of `records` is checked; the last
    /// loaded plugin when omitted.
    pub record_file: Option<String>,
    /// Check the file node of the last file of the load order instead, as
    /// the `-CheckForErrors` mode does: with only a game master loaded that
    /// is the file of the hardcoded records (`[00] <game>.exe`).
    #[serde(default)]
    pub last: bool,
}

/// An error of one element.
#[derive(Serialize, JsonSchema)]
pub struct ElementError {
    /// The path of the element, from its record's signature (`Path`).
    pub path: String,
    /// The error (`Check`).
    pub error: String,
}

impl From<CheckError> for ElementError {
    fn from(error: CheckError) -> Self {
        ElementError {
            path: error.path,
            error: error.error,
        }
    }
}

/// A main record with errors.
#[derive(Serialize, JsonSchema)]
pub struct RecordErrors {
    /// The record as the message log names it.
    pub name: String,
    /// The load order FormID, as eight hexadecimal digits.
    pub form_id: String,
    pub signature: String,
    /// The plugin that holds this version of the record.
    pub file: String,
    pub errors: Vec<ElementError>,
}

impl From<CheckedRecord> for RecordErrors {
    fn from(record: CheckedRecord) -> Self {
        RecordErrors {
            name: record.name,
            form_id: record.form_id,
            signature: record.signature,
            file: record.file,
            errors: record.errors.into_iter().map(ElementError::from).collect(),
        }
    }
}

#[derive(Serialize, JsonSchema)]
pub struct CheckResponse {
    /// Main records checked ("Processed Records").
    pub checked: u64,
    /// Records with errors ("Errors found").
    pub errors_found: u64,
    /// The exit code of the `-CheckForErrors` mode: the errors found, at
    /// most 127.
    pub exit_code: u8,
    /// The records with errors, in the order of the check.
    pub records: Vec<RecordErrors>,
    /// Errors of elements in no record.
    pub other_errors: Vec<ElementError>,
    /// The lines xEdit adds to its message log, without the times and the
    /// elapsed time: the start, a line per checked node, each record with
    /// errors followed by its errors (`    <path> -> <error>`), the counts.
    pub messages: Vec<String>,
}

pub(crate) fn files_check(session: &mut Session, request: CheckRequest) -> Result<CheckResponse, CommandError> {
    session.mode()?;
    let mut targets: Vec<CheckTarget> = Vec::new();
    for name in &request.files {
        let file = session.file(Some(name)).or_else(|_| {
            crate::conflicts::session_files(session)?
                .into_iter()
                .find(|file| file.get_name().eq_ignore_ascii_case(name))
                .ok_or_else(|| CommandError::new("unknown_file", format!("{name} is not loaded")))
        })?;
        targets.push(file as ElementRef);
    }
    for form_id in &request.records {
        let record = session.record(form_id, request.record_file.as_deref())?;
        targets.push(record as ElementRef);
    }
    // The GUI builds the references when it loads the plugins, unless the
    // mode builds none (`wbBuildRefs`); a check reads them (the story
    // manager check of a quest).
    if build_refs() {
        session.ensure_refs()?;
    }
    if request.last {
        if let Some(file) = crate::conflicts::session_files(session)?.pop() {
            targets.push(file as ElementRef);
        }
    } else if request.files.is_empty() && request.records.is_empty() {
        targets.extend(
            session
                .files
                .iter()
                .filter(|file| !file.get_file_states().contains(FileState::fsIsHardcoded))
                .map(|file| file.clone() as ElementRef),
        );
    }
    // The GUI offers "Check for Errors" only where editing is allowed
    // (`wbEditAllowed`), so the records it builds while it checks get the
    // fixes of their init (`wbBeginInternalEdit`); nothing is saved.
    let allowed = edit_allowed();
    set_edit_allowed(true);
    // The GUI names the records in its message log with the shorter names.
    let shorter_names = display_shorter_names();
    set_display_shorter_names(true);
    let report = crate::clean::run_on_large_stack(|| check::check_for_errors(&targets));
    set_display_shorter_names(shorter_names);
    set_edit_allowed(allowed);
    let report = report?;
    Ok(CheckResponse {
        checked: report.checked,
        errors_found: report.errors_found,
        exit_code: report.exit_code(),
        records: report.records.into_iter().map(RecordErrors::from).collect(),
        other_errors: report.other_errors.into_iter().map(ElementError::from).collect(),
        messages: report.messages,
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "files.check",
        "Check files or records for errors: unresolved and wrongly typed FormIDs, missing required members, deleted or partial records with data, out of range values and the checks of the definitions (mniNavCheckForErrorsClick, CheckForErrorsLinear).",
        false,
        files_check,
    );
}
