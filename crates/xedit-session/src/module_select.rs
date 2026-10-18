// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeModuleSelectForm.pas (PresetLoad, PresetSave,
// PresetDelete, PresetsUpdate, GetPresetsSection, IsPresetDifferent,
// SimulateLoad and the OnUpdate handlers of the preset actions),
// xeMainForm.pas (PresetCategory := 'ActiveModules' of the module selection)

//! `modules.*`: the selection presets of the module selection dialog
//! (`TfrmModuleSelect` with `PresetCategory = 'ActiveModules'`) kept in
//! xEdit's settings file. The dialog's check state is a parameter: the
//! modules its tree has checked, the active ones when a command omits it.
//! `SimulateLoad` derives the filtered modules the preset commands write —
//! the checked valid modules in simulated load order, with the masters they
//! need loaded before them — so a preset is the load order the dialog would
//! return, not the tree's raw check marks.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::interface::globals::data_path;
use xedit_loadorder::ini_files::{MemIniFile, comma_text, parse_comma_text, same_text};
use xedit_loadorder::load_order::Modules;

use crate::{CommandError, Registry, Session};

/// Port of `TfrmModuleSelect.GetPresetsSection` for the module selection
/// menu item: `Name + '_' + PresetCategory + '_Presets'`.
const PRESETS_SECTION: &str = "frmModuleSelect_ActiveModules_Presets";

/// The pseudo preset `csPluginsTxt`, inserted first by `PresetsUpdate`
/// (the dialog always selects active modules, so `SelectFlag = mfActive`).
const PLUGINS_TXT: &str = "<plugins.txt>";

/// The modules the selection dialog lists: `wbModulesByLoadOrder` without
/// the invalid ones (`AllModules`, `FilterFlag = mfValid`).
fn load_modules(session: &Session) -> Result<Modules, CommandError> {
    session.mode()?;
    let files = xedit_core::implementation::masters::loaded_files();
    Ok(Modules::load(&data_path(), &files))
}

fn io_error(error: std::io::Error, path: &Path) -> CommandError {
    CommandError::new("io", format!("{}: {error}", path.display()))
}

/// The settings file of the GUI (`frmMain.Settings`), shared with the mod
/// group commands (`xeSettingsFileName`).
fn settings_file() -> PathBuf {
    crate::modgroups::settings_file()
}

fn open_settings() -> Result<(PathBuf, MemIniFile), CommandError> {
    let path = settings_file();
    let ini = MemIniFile::open(&path).map_err(|error| io_error(error, &path))?;
    Ok((path, ini))
}

/// The check state of the tree: the named modules checked when `names` is
/// given, else every active module (`SelectFlag := mfActive`).
fn checked_modules(modules: &Modules, names: Option<&[String]>) -> Result<Vec<bool>, CommandError> {
    let mut checked = vec![false; modules.modules.len()];
    match names {
        None => {
            for (index, module) in modules.modules.iter().enumerate() {
                checked[index] = module.active;
            }
        }
        Some(names) => {
            for name in names {
                let index = modules
                    .modules
                    .iter()
                    .position(|module| module.valid && same_text(&module.name, name))
                    .ok_or_else(|| {
                        CommandError::new(
                            "unknown_module",
                            format!("\"{name}\" is not a valid module of the data folder"),
                        )
                    })?;
                checked[index] = true;
            }
        }
    }
    Ok(checked)
}

/// `FilteredModules` of the dialog's `SimulateLoad`: the checked valid
/// modules, in the load order `SimulateLoad` computes, each after the
/// masters it needs. A module the check state does not name (an automatically
/// loaded master, or the game master `wbAlwaysLoadGameMaster` force-loads) is
/// not filtered in.
fn filtered_modules(modules: &Modules, checked: &[bool]) -> Result<Vec<usize>, CommandError> {
    let order = modules
        .simulate_load(checked)
        .map_err(|error| CommandError::new("load_order", error))?;
    Ok(order
        .into_iter()
        .filter(|&index| checked[index] && modules.modules[index].valid)
        .collect())
}

fn module_names(modules: &Modules, filtered: &[usize]) -> Vec<String> {
    filtered
        .iter()
        .map(|&index| modules.modules[index].name.clone())
        .collect()
}

/// `IsPresetDifferent`: whether the filtered modules and the preset hold
/// the same names (both a sorted list without duplicates, compared without
/// regard to case).
fn is_preset_different(filtered: &[String], value: &str) -> bool {
    let list = |names: Vec<String>| {
        let mut names = names;
        names.sort_by(|a, b| xedit_loadorder::mod_groups::compare_text(a, b));
        names.dedup_by(|a, b| same_text(a, b));
        names
    };
    let current = list(filtered.to_vec());
    let preset = list(parse_comma_text(value));
    current.len() != preset.len() || !same_text(&comma_text(&current), &comma_text(&preset))
}

fn unknown_preset(preset: &str) -> CommandError {
    CommandError::new("unknown_preset", format!("there is no preset named \"{preset}\""))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetsRequest {}

/// The presets of the module selection dialog (`PresetsUpdate`).
#[derive(Serialize, JsonSchema)]
pub struct PresetsResponse {
    /// The preset names in the order the combo box holds them: the settings
    /// section's keys with `<plugins.txt>` first.
    pub presets: Vec<String>,
    /// `FilteredModules` of the current check state (the active valid
    /// modules in simulated load order), what a save would write now.
    pub modules: Vec<String>,
    /// The settings section the presets are in (`GetPresetsSection`).
    pub section: String,
    /// The settings file (`xeSettingsFileName`).
    pub settings: String,
}

fn modules_presets(session: &mut Session, _: PresetsRequest) -> Result<PresetsResponse, CommandError> {
    let modules = load_modules(session)?;
    let (path, ini) = open_settings()?;
    let mut presets = ini.read_section(PRESETS_SECTION);
    presets.insert(0, PLUGINS_TXT.to_owned());
    let filtered = filtered_modules(&modules, &checked_modules(&modules, None)?)?;
    Ok(PresetsResponse {
        presets,
        modules: module_names(&modules, &filtered),
        section: PRESETS_SECTION.to_owned(),
        settings: path.display().to_string(),
    })
}

/// `PresetLoad`: the preset's modules checked, then `SimulateLoad`. The
/// command is read-only: the dialog only changes its in-memory check state
/// and `LoadedPreset`; nothing is written to the settings file.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetLoadRequest {
    /// The name of the preset, or `<plugins.txt>` for the modules the plugin
    /// list activates (and the official ones, which xEdit always activates).
    pub preset: String,
}

#[derive(Serialize, JsonSchema)]
pub struct PresetLoadResponse {
    /// The preset that was loaded (`LoadedPreset`; empty for
    /// `<plugins.txt>`, which `IsPresetDifferent` skips).
    pub preset: String,
    /// `FilteredModules`: the checked valid modules in simulated load order.
    pub modules: Vec<String>,
    /// The dialog's panel error when the selection can not be confirmed
    /// (`No modules selected`; the form refuses OK then).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// `IsPresetDifferent`: whether the selection differs from the preset,
    /// the question the dialog asks on close. False for `<plugins.txt>`.
    pub preset_differs: bool,
}

fn modules_preset_load(session: &mut Session, request: PresetLoadRequest) -> Result<PresetLoadResponse, CommandError> {
    let modules = load_modules(session)?;
    let (_, ini) = open_settings()?;
    let mut value = String::new();
    let mut loaded_preset = false;
    let checked = if request.preset == PLUGINS_TXT {
        // `if (mfActiveInPluginsTxt in miFlags) or (miOfficialIndex <
        // High(Integer)) then Include(miFlags, SelectFlag)`.
        let mut checked = vec![false; modules.modules.len()];
        for (index, module) in modules.modules.iter().enumerate() {
            checked[index] = module.valid && (module.active_in_plugins_txt || module.official_index < i32::MAX);
        }
        checked
    } else {
        if request.preset.is_empty() || request.preset.starts_with('<') {
            return Err(CommandError::new(
                "unknown_preset",
                format!("\"{}\" is not a preset the module selection loads", request.preset),
            ));
        }
        if !ini.value_exists(PRESETS_SECTION, &request.preset) {
            return Err(unknown_preset(&request.preset));
        }
        value = ini.read_string(PRESETS_SECTION, &request.preset, "");
        // `sl.CommaText := ...; sl.Sorted := True; sl.Find(miName, j)`.
        let mut names = parse_comma_text(&value);
        names.sort_by(|a, b| xedit_loadorder::mod_groups::compare_text(a, b));
        let mut checked = vec![false; modules.modules.len()];
        for (index, module) in modules.modules.iter().enumerate() {
            checked[index] = module.valid && names.iter().any(|name| same_text(name, &module.name));
        }
        loaded_preset = true;
        checked
    };
    let filtered = filtered_modules(&modules, &checked)?;
    let names = module_names(&modules, &filtered);
    let preset_differs = loaded_preset && is_preset_different(&names, &value);
    Ok(PresetLoadResponse {
        preset: if loaded_preset { request.preset } else { String::new() },
        error: names.is_empty().then(|| "No modules selected".to_owned()),
        modules: names,
        preset_differs,
    })
}

/// `PresetSave`: `FilteredModules` written as a comma text. The action is
/// disabled for a name that contains `=`, has surrounding whitespace, is
/// empty, starts with `<`, or when no module is selected; the command
/// refuses the same names. The overwrite question is asked only when the
/// preset exists ("Are you sure you want to overwrite existing preset
/// ...?", or the changed-selection question with `after_modal`); `confirm`
/// is its answer, its default button when omitted (Yes, No with
/// `after_modal`).
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetSaveRequest {
    /// The name of the preset to write.
    pub preset: String,
    /// The checked modules of the dialog's tree; the active modules when
    /// omitted.
    pub modules: Option<Vec<String>>,
    /// The save the dialog runs on close after a preset was loaded (the
    /// question about the changed selection, with No as its default
    /// button).
    #[serde(default)]
    pub after_modal: bool,
    /// The answer to the question; the dialog's default button when
    /// omitted.
    pub confirm: Option<bool>,
    /// Report what would be written, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct PresetSaveResponse {
    pub preset: String,
    /// The value written, the filtered modules as a comma text.
    pub value: String,
    /// Whether the preset was written (the question can stop it).
    pub written: bool,
    /// Whether the preset holds another value than before.
    pub changed: bool,
    pub section: String,
    pub settings: String,
    pub dry_run: bool,
}

fn modules_preset_save(session: &mut Session, request: PresetSaveRequest) -> Result<PresetSaveResponse, CommandError> {
    let name = request.preset.replace('=', "");
    let name = name.trim();
    if request.preset != name || name.is_empty() || name.starts_with('<') {
        return Err(CommandError::new(
            "invalid_params",
            format!("\"{}\" is not a preset name a save writes", request.preset),
        ));
    }
    let modules = load_modules(session)?;
    let (path, ini) = open_settings()?;
    let checked = checked_modules(&modules, request.modules.as_deref())?;
    let filtered = filtered_modules(&modules, &checked)?;
    if filtered.is_empty() {
        return Err(CommandError::new(
            "invalid_params",
            "No modules selected: the dialog can not save an empty selection",
        ));
    }
    let value = comma_text(&module_names(&modules, &filtered));
    let exists = ini.value_exists(PRESETS_SECTION, &request.preset);
    let confirm = request.confirm.unwrap_or(!request.after_modal);
    if exists && !confirm {
        return Ok(PresetSaveResponse {
            preset: request.preset,
            value,
            written: false,
            changed: false,
            section: PRESETS_SECTION.to_owned(),
            settings: path.display().to_string(),
            dry_run: request.dry_run,
        });
    }
    let changed = ini.read_string(PRESETS_SECTION, &request.preset, "") != value;
    if !request.dry_run {
        let mut ini = ini;
        ini.write_string(PRESETS_SECTION, &request.preset, &value);
        ini.update_file().map_err(|error| io_error(error, &path))?;
    }
    Ok(PresetSaveResponse {
        preset: request.preset,
        value,
        written: true,
        changed,
        section: PRESETS_SECTION.to_owned(),
        settings: path.display().to_string(),
        dry_run: request.dry_run,
    })
}

/// `PresetDelete`: the question "Are you sure you want to delete preset
/// ...?", then `DeleteKey` and `UpdateFile` (the combo box is refilled from
/// the section, which the next `modules.presets` shows).
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetDeleteRequest {
    /// The name of the preset to delete.
    pub preset: String,
    /// The answer to the question; the dialog's default button (Yes) when
    /// omitted.
    pub confirm: Option<bool>,
    /// Report what would be deleted, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct PresetDeleteResponse {
    pub preset: String,
    /// Whether the preset was deleted (the question can stop it).
    pub deleted: bool,
    pub section: String,
    pub settings: String,
    pub dry_run: bool,
}

fn modules_preset_delete(
    session: &mut Session,
    request: PresetDeleteRequest,
) -> Result<PresetDeleteResponse, CommandError> {
    if request.preset.is_empty() || request.preset.starts_with('<') {
        return Err(CommandError::new(
            "invalid_params",
            format!("\"{}\" is not a preset name a delete removes", request.preset),
        ));
    }
    session.mode()?;
    let (path, ini) = open_settings()?;
    if !ini.value_exists(PRESETS_SECTION, &request.preset) {
        return Err(unknown_preset(&request.preset));
    }
    if !request.confirm.unwrap_or(true) {
        return Ok(PresetDeleteResponse {
            preset: request.preset,
            deleted: false,
            section: PRESETS_SECTION.to_owned(),
            settings: path.display().to_string(),
            dry_run: request.dry_run,
        });
    }
    if !request.dry_run {
        let mut ini = ini;
        ini.delete_key(PRESETS_SECTION, &request.preset);
        ini.update_file().map_err(|error| io_error(error, &path))?;
    }
    Ok(PresetDeleteResponse {
        preset: request.preset,
        deleted: true,
        section: PRESETS_SECTION.to_owned(),
        settings: path.display().to_string(),
        dry_run: request.dry_run,
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "modules.presets",
        "List the selection presets of the module selection dialog and what the current selection would save (PresetsUpdate).",
        false,
        modules_presets,
    );
    registry.register(
        "modules.preset_load",
        "Load a selection preset of the module selection dialog and report the filtered modules (PresetLoad, SimulateLoad).",
        false,
        modules_preset_load,
    );
    registry.register(
        "modules.preset_save",
        "Write the filtered modules of the module selection dialog as a preset (PresetSave).",
        true,
        modules_preset_save,
    );
    registry.register(
        "modules.preset_delete",
        "Delete a selection preset of the module selection dialog (PresetDelete).",
        true,
        modules_preset_delete,
    );
}
