// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavCreateDeltaPatchClick)

//! `files.create_delta_patch`: "Create Delta Patch" of a module. The newer
//! version is copied into the data folder as `<name>.esu` and loaded in the
//! place of the module, at its load order, as a delta patch to compare
//! against it (`TLoaderThread.Create(..., [fsIsDeltaPatch])`); the session
//! then holds the delta patch, and `files.save` writes it.

use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::{FileBytes, is_module, wb_file_compare};
use xedit_core::interface::Element;
use xedit_core::interface::globals::data_path;
use xedit_core::interface::types::{FileState, FileStates};

use crate::{CommandError, Registry, Session};

/// `files.create_delta_patch`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateDeltaPatchRequest {
    /// The loaded module the delta patch is made against.
    pub file: String,
    /// The newer version of the module on disk (the selected file of the
    /// dialog).
    pub newer: String,
    /// The name of the delta patch without its extension (the InputQuery);
    /// the newer file's name when omitted.
    pub name: Option<String>,
    /// Report the patch file, write and load nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `files.create_delta_patch`: the response.
#[derive(Serialize, JsonSchema)]
pub struct CreateDeltaPatchResponse {
    /// The module the patch is made against.
    pub file: String,
    /// The path of the delta patch.
    pub patch: String,
    /// The newer version the patch was copied from.
    pub newer: String,
    /// The loaded files after the patch was loaded in place of the module:
    /// the patch and the masters it loads with.
    pub loaded: Vec<String>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

/// `files.create_delta_patch`: copy the newer version of a module to
/// `<data>\<name>.esu` and load it as a delta patch compared to the module
/// (`mniNavCreateDeltaPatchClick`). The release copies the file and hands it
/// to the loader thread; the port copies it and loads it in the session,
/// which `files.save` then writes.
fn files_create_delta_patch(
    session: &mut Session,
    request: CreateDeltaPatchRequest,
) -> Result<CreateDeltaPatchResponse, CommandError> {
    let old = session.file(Some(&request.file))?;
    if old.get_is_not_plugin() {
        return Err(CommandError::new(
            "invalid_params",
            "Delta patch can only be created for modules",
        ));
    }
    if !is_module(&request.newer) {
        return Err(CommandError::new(
            "invalid_params",
            "Delta patch can only be created for modules",
        ));
    }
    let name = request.name.clone().unwrap_or_else(|| {
        let newer = xedit_core::delphi::path_file_name(&request.newer);
        xedit_core::delphi::change_file_ext(newer, "")
    });
    let data = data_path();
    let separator = if data.ends_with(['\\', '/']) { "" } else { "\\" };
    let patch = format!("{data}{separator}{name}.esu");
    if Path::new(&patch).exists() {
        return Err(CommandError::new(
            "invalid_params",
            format!("A module called \"{name}.esu\" already exists."),
        ));
    }
    let mut loaded = Vec::new();
    if !request.dry_run {
        std::fs::copy(&request.newer, &patch)
            .map_err(|error| CommandError::new("io_failed", format!("copying {}: {error}", request.newer)))?;
        let mapped = xedit_io::MappedFile::open(std::path::Path::new(&patch))
            .map_err(|error| CommandError::new("io_failed", error.to_string()))?;
        // `TLoaderThread.Create(patch, old, old.LoadOrder, [fsIsDeltaPatch])`:
        // the patch loads in the place of the module, at its load order, as
        // a compare load that remembers the module.
        let mut states = FileStates::empty();
        states.include(FileState::fsIsDeltaPatch);
        xedit_core::interface::clear_files();
        let patch_file = wb_file_compare(
            &patch,
            old.load_order(),
            Some(&old.get_name()),
            states,
            FileBytes::Mapped(mapped),
        )
        .map_err(|error| CommandError::new("load_failed", error.to_string()))?;
        let mut files = patch_file.all_masters();
        files.push(patch_file.clone());
        session.files = files;
        loaded = session.files.iter().map(|file| file.get_name()).collect();
    }
    Ok(CreateDeltaPatchResponse {
        file: old.get_name(),
        patch,
        newer: request.newer,
        loaded,
        dry_run: request.dry_run,
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "files.create_delta_patch",
        "Copy a newer version of a module into the data folder as a .esu and load it as a delta patch against the module (Create Delta Patch).",
        true,
        files_create_delta_patch,
    );
}
