// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavAddMastersClick,
// mniNavSortMastersClick, mniNavCleanMastersClick)

//! The master commands of the navigation menu: `masters.add` runs
//! `AddMastersIfMissing` as "Add Masters..." does, `masters.sort` runs
//! `SortMasters` and `masters.clean` runs `CleanMasters`. Each rewrites the
//! FormIDs of the plugin to follow its masters (`MastersUpdated`). A dry run
//! reports the master list the command would leave and changes nothing.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::FileImpl;
use xedit_core::interface::{Element, new_used_masters};

use crate::{CommandError, Registry, Session};

/// `masters.add`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MastersAddRequest {
    /// Name of the loaded plugin to change; the only loaded plugin when omitted.
    pub file: Option<String>,
    /// File names of loaded plugins to add as masters, such as
    /// `Dawnguard.esm`. Masters the plugin has already are skipped; under
    /// Starfield the masters of each one are added too.
    pub masters: Vec<String>,
    /// Sort the masters by load order afterwards (`aSortMasters`). Defaults
    /// to true, as upstream.
    #[serde(default = "default_true")]
    pub sort: bool,
    /// Report the master list the command would leave, but change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

fn default_true() -> bool {
    true
}

/// `masters.clean` and `masters.sort`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MastersRequest {
    /// Name of the loaded plugin to change; the only loaded plugin when omitted.
    pub file: Option<String>,
    /// Report the master list the command would leave, but change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// The response of the master commands.
#[derive(Serialize, JsonSchema)]
pub struct MastersResponse {
    /// Name of the plugin.
    pub file: String,
    /// The masters before the command, in the order of the file header.
    pub old_masters: Vec<String>,
    /// The masters after the command (for a dry run, the masters it would
    /// leave).
    pub masters: Vec<String>,
    /// Whether the master list changes.
    pub changed: bool,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

fn names(files: &[Arc<FileImpl>]) -> Vec<String> {
    files.iter().map(|file| file.get_name()).collect()
}

fn response(file: &FileImpl, old: Vec<String>, masters: Vec<String>, dry_run: bool) -> MastersResponse {
    MastersResponse {
        file: file.get_name(),
        changed: old != masters,
        old_masters: old,
        masters,
        dry_run,
    }
}

fn edit_failed(message: String) -> CommandError {
    CommandError::new("edit_failed", message)
}

fn masters_add(session: &mut Session, request: MastersAddRequest) -> Result<MastersResponse, CommandError> {
    let file = session.file(request.file.as_deref())?;
    let old = names(&file.masters());
    if request.dry_run {
        let mut masters = file.masters();
        masters.extend(file.masters_to_add(&request.masters).map_err(edit_failed)?);
        if request.sort {
            masters.sort_by_key(|master| master.load_order());
        }
        return Ok(response(&file, old, names(&masters), true));
    }
    file.add_masters_if_missing(&request.masters, request.sort, false)
        .map_err(edit_failed)?;
    Ok(response(&file, old, names(&file.masters()), false))
}

fn masters_sort(session: &mut Session, request: MastersRequest) -> Result<MastersResponse, CommandError> {
    let file = session.file(request.file.as_deref())?;
    let old = names(&file.masters());
    if request.dry_run {
        return Ok(response(&file, old, names(&file.masters_in_load_order()), true));
    }
    file.sort_masters().map_err(edit_failed)?;
    Ok(response(&file, old, names(&file.masters()), false))
}

fn masters_clean(session: &mut Session, request: MastersRequest) -> Result<MastersResponse, CommandError> {
    let file = session.file(request.file.as_deref())?;
    let old = names(&file.masters());
    if request.dry_run {
        let mut used = new_used_masters();
        file.find_used_masters(&mut used);
        let keep = file.masters_to_keep(&used);
        let masters: Vec<Arc<FileImpl>> = file
            .masters()
            .into_iter()
            .zip(keep)
            .filter_map(|(master, keep)| keep.then_some(master))
            .collect();
        return Ok(response(&file, old, names(&masters), true));
    }
    file.clean_masters().map_err(edit_failed)?;
    Ok(response(&file, old, names(&file.masters()), false))
}

/// Adds the master commands to the registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "masters.add",
        "Add loaded plugins as masters of a plugin (AddMastersIfMissing), sorted by load order unless sort is false.",
        true,
        masters_add,
    );
    registry.register(
        "masters.sort",
        "Sort the masters of a plugin by load order (SortMasters).",
        true,
        masters_sort,
    );
    registry.register(
        "masters.clean",
        "Remove the masters no FormID of a plugin points to (CleanMasters).",
        true,
        masters_clean,
    );
}
