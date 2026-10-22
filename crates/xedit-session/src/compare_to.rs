// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavCompareToClick)

//! `files.compare_to`: the navigation tree's "Compare to...". The chosen
//! file is copied into the data folder under a name that is free and loaded
//! in the place of the focused plugin, at that plugin's load order, as a
//! compare load (`TLoaderThread.Create(CompareFile, _File.FileName,
//! _File.LoadOrder, States)`): the compare file takes the load order of
//! the plugin it compares to, and the plugin becomes its last master
//! (`AddMaster(flCompareTo)`), so the records of the two files are
//! compared with each other.
//!
//! UPSTREAM-QUIRK: upstream copies the file under its own name when that
//! name is free and appends `IntToHex(i, 3)` only when it is taken
//! (`xeMainForm.pas:4007-4014`); the port always appends the number, so
//! the copy never overwrites a file of the data folder. Upstream also
//! copies to the save folder a file that is not a module
//! (`xeMainForm.pas:3999-4001`); the port copies to the data folder
//! always, because a compare load has to sit in the folder of the plugin
//! it compares to (its masters resolve to the same files there).

use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::masters::extract_file_ext;
use xedit_core::implementation::{FileBytes, FileImpl, is_module, wb_file_compare};
use xedit_core::interface::globals::{data_path, save_path};
use xedit_core::interface::types::{FileState, FileStates};
use xedit_core::interface::{Element, File};

use crate::{CommandError, Registry, Session};

/// `files.compare_to`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareToRequest {
    /// The loaded plugin the chosen file is compared to (the file the
    /// navigation tree has focused).
    pub file: String,
    /// The file to compare to, on the disk.
    pub path: String,
    /// Report the copy the command would make, but load nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `files.compare_to`: the response.
#[derive(Serialize, JsonSchema)]
pub struct CompareToResponse {
    /// The plugin the compare file is loaded in the place of.
    pub file: String,
    /// The path of the copy of the compare file in the data folder.
    pub copy: String,
    /// The loaded files after the compare file was loaded: the compare
    /// file and the masters it loads with.
    pub loaded: Vec<String>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

/// The folder the chosen file is copied into: the data folder of a
/// module, the save folder of anything else (`xeMainForm.pas:3999-4001`).
fn target_folder(path: &str) -> String {
    if is_module(path) {
        data_path()
    } else {
        let save = save_path();
        if save.is_empty() { data_path() } else { save }
    }
}

/// The free name of the copy in the folder: the name of the chosen file
/// with `IntToHex(i, 3)` appended, for the first free number of 0..255
/// (`xeMainForm.pas:4007-4014`).
fn free_copy(folder: &str, chosen: &str) -> Option<String> {
    let name = xedit_core::delphi::path_file_name(chosen);
    let stem = xedit_core::delphi::change_file_ext(name, "");
    let extension = extract_file_ext(name);
    let separator = if folder.ends_with(['\\', '/']) { "" } else { "\\" };
    for index in 0..=255u32 {
        // `IntToHex(i, 3)`: the number is written in hex, in three digits.
        let candidate = format!("{folder}{separator}{stem}{index:03x}{extension}");
        if !Path::new(&candidate).exists() {
            return Some(candidate);
        }
    }
    None
}

/// `files.compare_to`: copy the chosen file into the folder of the plugin
/// it compares to and load it in the place of that plugin, at its load
/// order, as a compare load (`mniNavCompareToClick`).
fn files_compare_to(session: &mut Session, request: CompareToRequest) -> Result<CompareToResponse, CommandError> {
    let old = session.file(Some(&request.file))?;
    if !Path::new(&request.path).is_file() {
        return Err(CommandError::new(
            "invalid_params",
            format!("{} does not exist", request.path),
        ));
    }
    let folder = target_folder(&request.path);
    let Some(copy) = free_copy(&folder, &request.path) else {
        return Err(CommandError::new(
            "io_failed",
            format!("Could not copy {} into {folder}", request.path),
        ));
    };
    if request.dry_run {
        return Ok(CompareToResponse {
            file: old.get_name(),
            copy,
            loaded: Vec::new(),
            dry_run: true,
        });
    }
    std::fs::copy(&request.path, &copy)
        .map_err(|error| CommandError::new("io_failed", format!("copying {}: {error}", request.path)))?;
    let mapped = xedit_io::MappedFile::open(Path::new(&copy))
        .map_err(|error| CommandError::new("io_failed", error.to_string()))?;
    // `TLoaderThread.Create(CompareFile, _File.FileName, _File.LoadOrder,
    // States)`: the compare file loads in the place of the plugin, at its
    // load order, and remembers the plugin as the file it compares to. The
    // copy is temporary (`fsIsTemporary`), because upstream deletes it when
    // the form closes.
    let file = wb_file_compare(
        &copy,
        old.load_order(),
        Some(&old.get_name()),
        FileStates::of(&[FileState::fsIsTemporary]),
        FileBytes::Mapped(mapped),
    )
    .map_err(|error| CommandError::new("load_failed", error.to_string()))?;
    // The session keeps its files: the compare file is added to them, with
    // the load order of the plugin it compares to, and the plugin stays
    // loaded as the last master of the compare file (`AddMaster(flCompareTo)`).
    let mut files: Vec<std::sync::Arc<FileImpl>> = session.files.clone();
    files.push(file);
    session.files = files;
    let loaded = session.files.iter().map(|file| file.get_name()).collect();
    Ok(CompareToResponse {
        file: old.get_name(),
        copy,
        loaded,
        dry_run: false,
    })
}

/// The compare file of a loaded file (`flCompareTo`), which the compare
/// load keeps.
pub fn compare_to_of(file: &FileImpl) -> Option<String> {
    file.get_compare_to_name()
}

/// Whether a file is a compare load (`fsIsCompareLoad`).
pub fn is_compare_load(file: &FileImpl) -> bool {
    file.get_file_states().contains(FileState::fsIsCompareLoad)
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "files.compare_to",
        "Copy a file into the data folder and load it in the place of a plugin, at that plugin's load order, as a compare load (Compare to...).",
        true,
        files_compare_to,
    );
}
