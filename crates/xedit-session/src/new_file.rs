// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (AddNewFile, AddNewFileName,
// AddNewFileWithDialog, GetSanitizedFilename, IsValidWindowsFileName)

//! `files.new`: a new empty plugin in the data folder, as the GUI's
//! `AddNewFile(aFile, aIsLight, aIsMedium)` makes it after its
//! `InputQuery` (the "<new file>" entry of the copy commands, and the
//! merged patch): the name without its extension, `.esl` for a light
//! module and `.esp` otherwise, the slot after the last loaded file, and
//! the game master as its master. With `template` it is "Create New File"
//! (`AddNewFileWithDialog`, `AddNewFile(aFile, aTemplate)`): the module
//! template the dialog offers gives the extension and the header flags
//! (`TwbFile.CreateNew` with a `PwbModuleInfo`), and the load order is after
//! the highest of the loaded files. The new plugin exists in memory until
//! `files.save` writes it.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::FileImpl;
use xedit_core::implementation::new_file::{ModuleTemplate, module_templates, wb_new_file, wb_new_file_from_template};
use xedit_core::interface::Element;
use xedit_core::interface::globals::{
    always_load_game_master, data_path, game_master_esm, is_light_supported, is_medium_supported, pseudo_light,
    pseudo_medium,
};

use crate::commands::refuse_in_translate_mode;
use crate::conflicts::session_files;
use crate::{CommandError, Registry, Session};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesNewRequest {
    /// File name of the new plugin; a `.esp`, `.esm` or `.esl` extension
    /// is dropped and `.esl` (light) or `.esp` used, as xEdit does. It is
    /// made in the data folder and must not exist there.
    pub file: String,
    /// A light module (ESL flag, `.esl`), where the game has them.
    #[serde(default)]
    pub light: bool,
    /// A medium module (Starfield).
    #[serde(default)]
    pub medium: bool,
    /// The module template of xEdit's "Create New File" dialog to make the
    /// file from, in place of `light` and `medium`: the extension (`.esp`
    /// or `.esm`) and the header flags. The game offers the templates its
    /// modules can have (no light ones before Skyrim SE, no medium or update
    /// ones before Starfield, and Starfield only `esp` and the `esm` ones).
    pub template: Option<FileTemplate>,
    /// Check the name and report the file, but make nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// A module template of "Create New File": the extension, then the header
/// flags (`<new file>.esp` with `<ESM>` is `esp-esm`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FileTemplate {
    Esp,
    EspEsm,
    EspLight,
    EspEsmLight,
    EspMedium,
    EspEsmMedium,
    EspUpdate,
    EspEsmUpdate,
    Esm,
    EsmLight,
    EsmMedium,
    EsmUpdate,
}

impl FileTemplate {
    /// The module template of the core.
    pub fn module_template(self) -> ModuleTemplate {
        let (esm_extension, esm, light, medium, update) = match self {
            FileTemplate::Esp => (false, false, false, false, false),
            FileTemplate::EspEsm => (false, true, false, false, false),
            FileTemplate::EspLight => (false, false, true, false, false),
            FileTemplate::EspEsmLight => (false, true, true, false, false),
            FileTemplate::EspMedium => (false, false, false, true, false),
            FileTemplate::EspEsmMedium => (false, true, false, true, false),
            FileTemplate::EspUpdate => (false, false, false, false, true),
            FileTemplate::EspEsmUpdate => (false, true, false, false, true),
            FileTemplate::Esm => (true, true, false, false, false),
            FileTemplate::EsmLight => (true, true, true, false, false),
            FileTemplate::EsmMedium => (true, true, false, true, false),
            FileTemplate::EsmUpdate => (true, true, false, false, true),
        };
        ModuleTemplate {
            esm_extension,
            esm,
            light,
            medium,
            update,
        }
    }
}

#[derive(Serialize, JsonSchema)]
pub struct FilesNewResponse {
    /// File name of the new plugin.
    pub file: String,
    /// Where `files.save` writes it by default.
    pub path: String,
    /// Its load order.
    pub load_order: i32,
    /// Its masters.
    pub masters: Vec<String>,
    /// Nothing was made.
    pub dry_run: bool,
}

/// Port of `IsValidWindowsFileName`.
fn is_valid_windows_file_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| "\\/*?:\"<>|".contains(c))
}

/// Port of `GetSanitizedFilename`: trimmed, without a plugin extension.
fn sanitized_file_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return String::new();
    }
    let extension = xedit_core::implementation::masters::extract_file_ext(name);
    if [".esp", ".esm", ".esl"]
        .iter()
        .any(|ext| extension.eq_ignore_ascii_case(ext))
    {
        name[..name.len() - extension.len()].to_owned()
    } else {
        name.to_owned()
    }
}

/// The name `AddNewFile(aFile, aIsLight, ...)` gives the file, checked as
/// `AddNewFileName` checks it: the sanitized name with `.esl` or `.esp`.
pub(crate) fn new_file_name(requested: &str, light: bool) -> Result<String, CommandError> {
    new_file_name_with(requested, if light { ".esl" } else { ".esp" })
}

/// [`new_file_name`] with the extension given (a template's).
fn new_file_name_with(requested: &str, extension: &str) -> Result<String, CommandError> {
    let base = sanitized_file_name(requested);
    if base.is_empty() {
        return Err(CommandError::new("invalid_params", "the file name is empty"));
    }
    let name = format!("{base}{extension}");
    if !is_valid_windows_file_name(&name) {
        return Err(CommandError::new(
            "invalid_params",
            format!(
                "The specified filename:\r\n\r\n{name}\r\n\r\nContains one or more invalid characters:\r\n\r\n\\/*?:\"<>|"
            ),
        ));
    }
    if std::path::Path::new(&format!("{}{name}", data_path())).exists() {
        return Err(CommandError::new("file_exists", "A file of that name exists already."));
    }
    if session_loaded(&name) {
        return Err(CommandError::new("file_exists", format!("{name} is loaded already")));
    }
    Ok(name)
}

fn session_loaded(name: &str) -> bool {
    xedit_core::interface::files()
        .iter()
        .any(|file| file.get_name().eq_ignore_ascii_case(name))
}

/// Port of `AddNewFileName` and the rest of `AddNewFile`: the file in the
/// data folder with the load order after the last of `files` (the loaded
/// files in load order), and the game master as its master
/// (`wbAlwaysLoadGameMaster`).
pub(crate) fn add_new_file(
    name: &str,
    files: &[Arc<FileImpl>],
    light: bool,
    medium: bool,
) -> Result<Arc<FileImpl>, CommandError> {
    let edit_failed = |message: String| CommandError::new("edit_failed", message);
    let load_order = files.last().map_or(0, |file| file.load_order() + 1);
    let file = wb_new_file(&format!("{}{name}", data_path()), load_order, light, medium).map_err(edit_failed)?;
    if always_load_game_master() {
        file.add_masters_if_missing(&[game_master_esm()], true, false)
            .map_err(edit_failed)?;
    }
    Ok(file)
}

/// Port of `AddNewFileName(aFileName, aTemplate)` and the rest of
/// `AddNewFile(aFile, aTemplate)`: the load order after the highest of the
/// loaded files, and the game master as master.
fn add_new_file_from_template(
    name: &str,
    files: &[Arc<FileImpl>],
    template: &ModuleTemplate,
) -> Result<Arc<FileImpl>, CommandError> {
    let edit_failed = |message: String| CommandError::new("edit_failed", message);
    let load_order = files.iter().map(|file| file.load_order() + 1).max().unwrap_or(0);
    let file =
        wb_new_file_from_template(&format!("{}{name}", data_path()), load_order, template).map_err(edit_failed)?;
    if always_load_game_master() {
        file.add_masters_if_missing(&[game_master_esm()], true, false)
            .map_err(edit_failed)?;
    }
    Ok(file)
}

fn files_new(session: &mut Session, request: FilesNewRequest) -> Result<FilesNewResponse, CommandError> {
    session.mode()?;
    refuse_in_translate_mode("files.new")?;
    if let Some(template) = request.template {
        if request.light || request.medium {
            return Err(CommandError::new(
                "invalid_params",
                "a template gives the flags of the module: leave light and medium out",
            ));
        }
        let module = template.module_template();
        if !module_templates().contains(&module) {
            return Err(CommandError::new(
                "invalid_params",
                format!(
                    "Create New File offers no {} template for this game",
                    template_name(template)
                ),
            ));
        }
        let name = new_file_name_with(&request.file, module.extension())?;
        let files = session_files(session)?;
        if request.dry_run {
            return Ok(FilesNewResponse {
                path: format!("{}{name}", data_path()),
                file: name,
                load_order: files.iter().map(|file| file.load_order() + 1).max().unwrap_or(0),
                masters: vec![game_master_esm()],
                dry_run: true,
            });
        }
        let file = add_new_file_from_template(&name, &files, &module)?;
        session.files.push(file.clone());
        return Ok(FilesNewResponse {
            file: name,
            path: file.file_name().to_owned(),
            load_order: file.load_order(),
            masters: file.masters().iter().map(|master| master.get_name()).collect(),
            dry_run: false,
        });
    }
    if request.light && request.medium {
        return Err(CommandError::new(
            "invalid_params",
            "a module is light or medium, not both",
        ));
    }
    if request.light && !(is_light_supported() || pseudo_light()) {
        return Err(CommandError::new("invalid_params", "the game has no light modules"));
    }
    if request.medium && !(is_medium_supported() || pseudo_medium()) {
        return Err(CommandError::new("invalid_params", "the game has no medium modules"));
    }
    let name = new_file_name(&request.file, request.light)?;
    let files = session_files(session)?;
    if request.dry_run {
        return Ok(FilesNewResponse {
            path: format!("{}{name}", data_path()),
            file: name,
            load_order: files.last().map_or(0, |file| file.load_order() + 1),
            masters: vec![game_master_esm()],
            dry_run: true,
        });
    }
    let file = add_new_file(&name, &files, request.light, request.medium)?;
    session.files.push(file.clone());
    Ok(FilesNewResponse {
        file: name,
        path: file.file_name().to_owned(),
        load_order: file.load_order(),
        masters: file.masters().iter().map(|master| master.get_name()).collect(),
        dry_run: false,
    })
}

/// The name of a template as requests give it.
fn template_name(template: FileTemplate) -> String {
    serde_json::to_value(template)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "files.new",
        "Make a new empty plugin in the data folder, with the game master as its master (AddNewFile); files.save writes it.",
        true,
        files_new,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_sanitized_as_xedit_does() {
        assert_eq!(sanitized_file_name("  Merged Patch.esm "), "Merged Patch");
        assert_eq!(sanitized_file_name("Merged.ESP"), "Merged");
        assert_eq!(sanitized_file_name("Merged.txt"), "Merged.txt");
        assert!(!is_valid_windows_file_name("a|b.esp"));
        assert!(is_valid_windows_file_name("Merged Patch.esp"));
    }
}
