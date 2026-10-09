// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavCreateMergedPatchClick,
// AddNewFile, AddNewFileName, GetSanitizedFilename,
// IsValidWindowsFileName)

//! `patch.merged`: xEdit's "Create Merged Patch". The command makes the new
//! plugin as the GUI's handler does (`AddNewFile`: an `.esp` in the data
//! folder with the game master as its master, then every loaded file as a
//! master), runs the merge of `xedit_analysis::merged_patch`, and removes
//! the masters the patch does not need (`CleanMasters`). The patch is a
//! loaded plugin of the session afterwards; `save` writes it at once, as
//! `xedit patch merged` does.
//!
//! The GUI asks for the file name (`InputQuery`) and, for the games from
//! Skyrim on, whether to go on with a merge it calls unsupported; here the
//! name is a parameter and the question comes back as `warning`.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_analysis::merged_patch::{self, MergedPatchReport};
use xedit_core::implementation::FileImpl;
use xedit_core::interface::Element;
use xedit_core::interface::globals::game_name2;

use crate::commands::refuse_in_translate_mode;
use crate::conflicts::session_files;
use crate::new_file::{add_new_file, new_file_name};
use crate::save::{FilesSaveResponse, save_file};
use crate::{CommandError, Registry, Session};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchMergedRequest {
    /// File name of the new patch, with or without its extension (`.esp`,
    /// `.esm` and `.esl` are dropped and `.esp` is used, as xEdit does); it
    /// is made in the data folder and must not exist there.
    pub file: String,
    /// Report what the patch would merge, but make no plugin.
    #[serde(default)]
    pub dry_run: bool,
    /// Save the patch after the merge (`files.save`), to `output` or into
    /// the data folder.
    #[serde(default)]
    pub save: bool,
    /// Where `save` writes the patch; the data folder when omitted.
    pub output: Option<String>,
    /// Move an existing file at the output path to `<AppName>Edit Backups`
    /// before `save` replaces it. Defaults to true, as upstream.
    #[serde(default = "default_true")]
    pub backup: bool,
}

fn default_true() -> bool {
    true
}

/// A list of a merged record.
#[derive(Serialize, JsonSchema)]
pub struct PatchList {
    /// The list, as xEdit's handler names it (`Leveled List Entries`).
    pub name: String,
    /// Entries of the merged list.
    pub entries: usize,
    /// Entries of the list of the winning override.
    pub winning_entries: usize,
}

/// A record the patch overrides.
#[derive(Serialize, JsonSchema)]
pub struct PatchRecord {
    /// Load order FormID as eight hexadecimal digits.
    pub form_id: String,
    pub signature: String,
    /// The record as xEdit names it.
    pub name: String,
    /// The plugin of the winning override, whose version the patch copies.
    pub winning_file: String,
    /// The lists the patch merged in the record.
    pub lists: Vec<PatchList>,
}

#[derive(Serialize, JsonSchema)]
pub struct PatchMergedResponse {
    /// File name of the patch.
    pub file: String,
    /// Nothing was made: the records are what the patch would merge.
    pub dry_run: bool,
    /// The question xEdit asks before the merge in this game, which calls
    /// the merged patch unsupported there; the command goes on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Records with two overrides or more whose lists were compared.
    pub checked: usize,
    /// The records the patch overrides, in the order they were merged.
    pub records: Vec<PatchRecord>,
    /// The masters of the patch after `CleanMasters` (empty for a dry run).
    pub masters: Vec<String>,
    /// The lines xEdit writes to its message log: faulty ordered lists and
    /// the errors of records that could not be merged.
    pub messages: Vec<String>,
    /// The patch has changes in memory that are not saved.
    pub unsaved: bool,
    /// The save, with `save`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved: Option<FilesSaveResponse>,
}

fn patch_merged(session: &mut Session, request: PatchMergedRequest) -> Result<PatchMergedResponse, CommandError> {
    let mode = session.mode()?;
    refuse_in_translate_mode("patch.merged")?;
    let files = session_files(session)?;
    let name = new_file_name(&request.file, false)?;
    let warning = merged_patch::unsupported_warning(mode, &game_name2());
    if request.dry_run {
        let report = merged_patch::merge_into(&files, None);
        return Ok(response(name, true, warning, report, Vec::new(), false, None));
    }

    let target = create_patch_file(&name, &files)?;
    session.files.push(target.clone());
    let report = merged_patch::merge_into(&files, Some(&target));
    target
        .clean_masters()
        .map_err(|message| CommandError::new("edit_failed", message))?;
    let masters = target.masters().iter().map(|file| file.get_name()).collect();
    let saved = if request.save {
        Some(save_file(&target, request.output, false, request.backup)?)
    } else {
        None
    };
    let unsaved = saved.is_none();
    Ok(response(name, false, warning, report, masters, unsaved, saved))
}

/// The new file of the handler: `AddNewFile(TargetFile, False, False)`
/// and `AddMasters` with every loaded file, one per load order.
fn create_patch_file(name: &str, files: &[Arc<FileImpl>]) -> Result<Arc<FileImpl>, CommandError> {
    let target = add_new_file(name, files, false, false)?;
    let mut masters = Vec::new();
    let mut last_load_order = -1;
    for file in files {
        if file.load_order() > last_load_order {
            last_load_order = file.load_order();
            masters.push(file.get_name());
        }
    }
    target
        .add_masters(&masters, false)
        .map_err(|message| CommandError::new("edit_failed", message))?;
    Ok(target)
}

fn response(
    file: String,
    dry_run: bool,
    warning: Option<String>,
    report: MergedPatchReport,
    masters: Vec<String>,
    unsaved: bool,
    saved: Option<FilesSaveResponse>,
) -> PatchMergedResponse {
    PatchMergedResponse {
        file,
        dry_run,
        warning,
        checked: report.checked,
        records: report
            .records
            .into_iter()
            .map(|record| PatchRecord {
                form_id: format!("{:08X}", record.form_id),
                signature: record.signature.to_string(),
                name: record.name,
                winning_file: record.winning_file,
                lists: record
                    .lists
                    .into_iter()
                    .map(|list| PatchList {
                        name: list.name,
                        entries: list.entries,
                        winning_entries: list.winning_entries,
                    })
                    .collect(),
            })
            .collect(),
        masters,
        messages: report.messages,
        unsaved,
        saved,
    }
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "patch.merged",
        "Create a merged patch: a new plugin whose overrides merge the leveled lists, container items, factions, keywords and the other lists that several plugins change (mniNavCreateMergedPatchClick).",
        true,
        patch_merged,
    );
}
