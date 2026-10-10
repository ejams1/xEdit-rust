// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xeScriptForm.pas (ReadScriptsList), xeInit.pas (wbScriptsPath)

//! The commands of phase 6: `script.list` lists the Pascal scripts of the
//! scripts folder, the corpus `script.check` and `script.run` work on. The
//! folder is upstream's `wbScriptsPath`: the `--scripts` folder, else
//! `XEDIT_SCRIPTS`, else the oracle's `Edit Scripts`.
//!
//! Only `script.list` is registered here so far; `script.check` (the parser)
//! and `script.run` (the host and run loop) are later steps of phase 6.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_io::collate::ansi_compare_text;

use crate::{CommandError, Registry, Session};

/// `script.list`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptListRequest {
    /// The folder with the `*.pas` scripts: the `Edit Scripts` folder. When
    /// omitted, the `XEDIT_SCRIPTS` environment variable, else the oracle's
    /// `Edit Scripts` (the `XEDIT_ORACLE_DIR` folder), else the `Edit
    /// Scripts` folder beside this program.
    pub scripts: Option<String>,
}

/// One script of the folder.
#[derive(Serialize, JsonSchema)]
pub struct ScriptInfo {
    /// File name of the script, `<Unit>.pas`: a script's unit name matches
    /// its file name, so the file name is the unit a `uses` of another
    /// script resolves through the folder (`_newscript_.pas`, the form's
    /// template, is the one exception).
    pub name: String,
    /// Lines of the file.
    pub lines: u32,
}

/// `script.list`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ScriptListResponse {
    /// The folder that was listed.
    pub folder: String,
    /// Every `*.pas` script of the folder, by name.
    pub scripts: Vec<ScriptInfo>,
}

/// The folder the scripts are read from: the one named by the caller, else
/// `XEDIT_SCRIPTS`, else the oracle's `Edit Scripts`, else this program's
/// (upstream's `wbScriptsPath` default of `xeInit.pas`, which `-S:` sets).
pub fn scripts_folder(explicit: Option<&str>) -> PathBuf {
    if let Some(folder) = explicit {
        return PathBuf::from(folder);
    }
    if let Some(folder) = std::env::var_os("XEDIT_SCRIPTS") {
        return PathBuf::from(folder);
    }
    if let Some(oracle) = std::env::var_os("XEDIT_ORACLE_DIR") {
        return PathBuf::from(oracle).join("Edit Scripts");
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.join("Edit Scripts")))
        .unwrap_or_else(|| PathBuf::from("Edit Scripts"))
}

/// Lists the `*.pas` scripts of `folder` as `ReadScriptsList` fills the
/// script combo box: the top folder only, sorted as a `TStringList`. Unlike
/// the combo box, the `_newscript_` placeholder of the form is listed too:
/// it is one of the shipped scripts of the corpus (`script.check` parses
/// every file of the folder).
pub fn list_scripts(folder: &Path) -> Result<Vec<ScriptInfo>, CommandError> {
    let entries =
        std::fs::read_dir(folder).map_err(|error| CommandError::new("io", format!("{}: {error}", folder.display())))?;
    let mut scripts = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| CommandError::new("io", format!("{}: {error}", folder.display())))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pas"))
        {
            continue;
        }
        let name = match entry.file_name().into_string() {
            Ok(name) => name,
            // A name the file system holds that is not valid UTF-8 is not a
            // unit name the parser accepts either.
            Err(_) => continue,
        };
        let text =
            std::fs::read(&path).map_err(|error| CommandError::new("io", format!("{}: {error}", path.display())))?;
        let lines = String::from_utf8_lossy(&text).lines().count();
        scripts.push(ScriptInfo {
            name,
            lines: u32::try_from(lines).unwrap_or(u32::MAX),
        });
    }
    scripts.sort_by(|a, b| ansi_compare_text(&a.name, &b.name));
    Ok(scripts)
}

fn script_list(_session: &mut Session, request: ScriptListRequest) -> Result<ScriptListResponse, CommandError> {
    let folder = scripts_folder(request.scripts.as_deref());
    let scripts = list_scripts(&folder)?;
    Ok(ScriptListResponse {
        folder: folder.display().to_string(),
        scripts,
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "script.list",
        "List the *.pas scripts of the scripts folder (XEDIT_SCRIPTS, the oracle's Edit Scripts by default).",
        false,
        script_list,
    );
}
