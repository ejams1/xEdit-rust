// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeModuleSelectForm.pas (FormCreate, ShowModal,
// vstModulesInitNode, vstModulesGetText, PresetLoad, PresetSave,
// PresetDelete, PresetsUpdate, GetPresetsSection, IsPresetDifferent,
// SimulateLoad and the OnUpdate handlers of the preset actions),
// xeMainForm.pas (PresetCategory := 'ActiveModules' of the module selection),
// Core/wbLoadOrder.pas (wbModulesByLoadOrder of the two dialogs)

//! `modules.*`: the modules the module selection dialog and the mod group
//! editor's Insert list show (`wbModulesByLoadOrder`, each with the flags
//! the dialogs read), and the selection presets of the module selection
//! dialog (`TfrmModuleSelect` with `PresetCategory = 'ActiveModules'`) kept
//! in xEdit's settings file. The dialog's check state is a parameter: the
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
pub use xedit_loadorder::load_order::{ModuleInfo, Modules};

use crate::{CommandError, Registry, Session};

/// The modules the selection dialog lists, and the data folder
/// `DoInitPath` found them in.
///
/// Port of `wbLoadModules` as the GUI's dialog reads it before a session
/// exists: the game is set up (`xeInit.pas` `_DoInitPath` and the
/// definitions), the data folder is found (`CheckAppPath`, the Steam
/// library, the registry), the game's own plugin list and settings file are
/// set, and the modules of the data folder are scanned. The dialog's check
/// state and `SimulateLoad` are the GUI's; this only opens the list.
///
/// This is not a command: it is the data source of a dialog, as
/// [`Modules::load`] is for a loaded session.
pub struct SelectableModules {
    pub game: xedit_core::interface::globals::GameMode,
    /// The data folder with its trailing separator.
    pub data_path: String,
    /// The module list (not `Debug`: it holds the loaded files).
    pub modules: Modules,
}

/// Opens the module list of a game for the selection dialog.
pub fn selectable_modules(game: &str) -> Result<SelectableModules, CommandError> {
    use xedit_core::interface::globals;
    let mode =
        crate::dump::setup_game_for_edit(game).map_err(|message| CommandError::new("invalid_params", message))?;
    let folder = crate::game_paths::discover_game_folder(mode);
    if folder.is_empty() {
        return Err(CommandError::new(
            "game_folder",
            crate::game_paths::missing_install_message(mode),
        ));
    }
    let data_path = crate::game_paths::data_path_of(&folder, mode);
    if !Path::new(&data_path).is_dir() {
        return Err(CommandError::new("game_folder", format!("{data_path} is not a folder")));
    }
    // `DoInitPath`: the data folder, the game's own plugin list (unless a
    // command line named one), the inis of `My Games` and the settings
    // file, which the module scan and the dialog read.
    if globals::data_path().is_empty() {
        globals::set_data_path(&data_path);
    }
    if globals::plugins_file_name().is_empty() {
        globals::set_plugins_file_name(&crate::game_paths::default_plugins_file(mode, &data_path));
    }
    crate::resources::init_game_paths(mode, &data_path);
    let settings = crate::game_paths::settings_file_name(
        crate::game_paths::app_name(mode),
        "Edit",
        &globals::program_path(),
        &globals::plugins_file_name(),
    );
    globals::set_settings_file_name(&settings);
    let modules = Modules::load_data_folder(&data_path);
    Ok(SelectableModules {
        game: mode,
        data_path,
        modules,
    })
}

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

/// One module of the dialogs' trees: `TwbModuleInfo` with the fields the
/// module selection's columns read (`vstModulesGetText`) and the flags the
/// mod group editor's Insert filters on.
#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ModuleEntry {
    /// The position of the module in `wbModulesByLoadOrder`, the order the
    /// dialog lists its nodes; the positions are of the whole list, so a
    /// filter leaves gaps.
    pub index: usize,
    /// `miName`: the file name without `.ghost`.
    pub name: String,
    /// `miOriginalName`: the name of the file in the data folder (with
    /// `.ghost`).
    pub original_name: String,
    /// `miLoadOrder`: the load order of the module's loaded file, `None`
    /// when it is not loaded (upstream `High(Integer)`), which the dialog's
    /// load order column shows only below 10000.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_order: Option<i32>,
    /// `miFileID`: the load order FileID of the loaded module (`01`,
    /// `FE 003`), `None` for a module that is not loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    /// `wbDataPath + miOriginalName`, the module's file.
    pub path: String,
    /// The size of the module's file in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// `mfValid`: the module's header could be read (a plugin; the game's
    /// executable is not valid).
    pub valid: bool,
    /// `mfHasFile`/`mfLoaded`: the module's file is loaded in the session.
    pub loaded: bool,
    /// `mfHasESMFlag`: the ESM flag of the file header (the dialog's `ESM`
    /// column).
    pub has_esm_flag: bool,
    /// `mfIsESM`: the ESM extension of the games from Fallout 4 on, or the
    /// ESM flag (how the load order sorts the module).
    pub is_esm: bool,
    /// `mfHasESMExtension`: the module's extension is `.esm` or `.esl` in
    /// the games from Fallout 4 on.
    pub has_esm_extension: bool,
    /// `mfHasLightFlag`: the light (ESL) flag, or an `.esl` extension (the
    /// dialog's light column, `wbLightName`).
    pub is_light: bool,
    /// `mfHasMediumFlag`: the medium flag (Starfield).
    pub is_medium: bool,
    /// `mfHasUpdateFlag`: the update (overlay) flag.
    pub is_update: bool,
    /// `mfHasLocalizedFlag`: the localized flag of the file header.
    pub is_localized: bool,
    /// `mfHasBlueprintFlag`: the Starfield blueprint flag.
    pub has_blueprint: bool,
    /// `mfGhost`: a `.ghost` file whose original is not in the data folder.
    pub is_ghost: bool,
    /// `mfIsHardcoded`: the game's executable, which the hardcoded file
    /// loads as.
    pub is_hardcoded: bool,
    /// `mfMastersMissing`: one of the module's masters is not in the data
    /// folder (or lacks masters itself). The dialog disables such a node.
    pub masters_missing: bool,
    /// `mfActive`: the module is active (`mfActiveInPluginsTxt` or one of
    /// `wbLoadModules`' implicit actives), the check of the dialog's
    /// `mfActive` selection.
    pub active: bool,
    /// `mfActiveInPluginsTxt`: the plugin list names the module active.
    pub active_in_plugins_txt: bool,
    /// `mfHasIndex`: the module has an index the load order sort reads.
    pub has_index: bool,
    /// `miDateTime`: the write time of the module's file as a Delphi
    /// `TDateTime` (days since 1899-12-30), which the dialogs' `[Time:...]`
    /// text of `LoadOrderDescription` formats and the load order sorts by.
    pub date_time: f64,
    /// `miOfficialIndex`: the place of the game master, the game's
    /// executable and its official DLC, `None` for every other module
    /// (upstream `High(Integer)`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub official_index: Option<i32>,
    /// `miCCIndex`: the place of the module in the game's Creation Club
    /// list from 1, `None` otherwise (upstream `High(Integer)`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cc_index: Option<i32>,
    /// `miPluginsTxtIndex`: the module's line in the plugin list, `None`
    /// when the list does not name it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugins_txt_index: Option<i32>,
    /// `miLoadOrderTxtIndex`: the module's line in `loadorder.txt`, `None`
    /// when the file does not name it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_order_txt_index: Option<i32>,
    /// `miMasterNames`: the file names the module's file header lists.
    pub masters: Vec<String>,
    /// `GetCRC32`: the module's CRC32, `None` when the file can not be
    /// read or its value is not valid (`TwbCRC32.IsValid`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crc32: Option<u32>,
}

/// An index that upstream keeps at `High(Integer)` for a module it does not
/// assign.
fn index_or_none(value: i32) -> Option<i32> {
    (value != i32::MAX).then_some(value)
}

fn module_entry(index: usize, module: &xedit_loadorder::load_order::ModuleInfo) -> ModuleEntry {
    use xedit_core::interface::File;
    ModuleEntry {
        index,
        name: module.name.clone(),
        original_name: module.original_name.clone(),
        load_order: (module.load_order < 10000).then_some(module.load_order),
        file_id: module
            .file
            .as_ref()
            .map(|file| file.get_load_order_file_id().to_string()),
        path: module.file_path(),
        size: module.file_size(),
        valid: module.valid,
        loaded: module.has_file(),
        has_esm_flag: module.has_esm_flag,
        is_esm: module.is_esm,
        has_esm_extension: module.has_esm_extension,
        is_light: module.is_light,
        is_medium: module.is_medium,
        is_update: module.is_update,
        is_localized: module.is_localized,
        has_blueprint: module.has_blueprint,
        is_ghost: module.is_ghost,
        is_hardcoded: module.is_hardcoded,
        masters_missing: module.masters_missing,
        active: module.active,
        active_in_plugins_txt: module.active_in_plugins_txt,
        has_index: module.has_index,
        date_time: module.date_time,
        official_index: index_or_none(module.official_index),
        cc_index: index_or_none(module.cc_index),
        plugins_txt_index: index_or_none(module.plugins_txt_index),
        load_order_txt_index: index_or_none(module.load_order_txt_index),
        masters: module.master_names.clone(),
        crc32: module.get_crc32(),
    }
}

/// `modules.list`: the modules the module selection dialog and the mod
/// group editor's Insert list build their trees from
/// (`wbModulesByLoadOrder`), in the load order the dialogs list them.
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModulesListRequest {
    /// Keep only the valid modules, as both dialogs do
    /// (`FilteredByFlag(mfValid)`): false lists every module of the data
    /// folder, including the game's executable and a plugin whose header
    /// did not read.
    #[serde(default = "default_true")]
    pub valid: bool,
    /// Keep only the active modules (`FilteredByFlag(mfActive)`).
    #[serde(default)]
    pub active: bool,
    /// Keep only the modules whose masters are in the data folder
    /// (`not mfMastersMissing`); the dialogs disable the others instead.
    #[serde(default)]
    pub masters_present: bool,
}

fn default_true() -> bool {
    true
}

/// `modules.list`: the response.
#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ModulesListResponse {
    /// The data folder the modules were read from (`wbDataPath`, with its
    /// trailing separator).
    pub data_path: String,
    /// `wbModulesByLoadOrder` in load order, filtered by the request.
    pub modules: Vec<ModuleEntry>,
}

fn modules_list(session: &mut Session, request: ModulesListRequest) -> Result<ModulesListResponse, CommandError> {
    let modules = load_modules(session)?;
    let data_path = data_path();
    let modules = modules
        .modules
        .iter()
        .enumerate()
        .filter(|(_, module)| !request.valid || module.valid)
        .filter(|(_, module)| !request.active || module.active)
        .filter(|(_, module)| !request.masters_present || !module.masters_missing)
        .map(|(index, module)| module_entry(index, module))
        .collect();
    Ok(ModulesListResponse { data_path, modules })
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

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetsRequest {}

/// The presets of the module selection dialog (`PresetsUpdate`).
#[derive(Serialize, Deserialize, JsonSchema)]
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
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetLoadRequest {
    /// The name of the preset, or `<plugins.txt>` for the modules the plugin
    /// list activates (and the official ones, which xEdit always activates).
    pub preset: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
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
#[derive(Serialize, Deserialize, JsonSchema)]
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

#[derive(Serialize, Deserialize, JsonSchema)]
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
#[derive(Serialize, Deserialize, JsonSchema)]
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

#[derive(Serialize, Deserialize, JsonSchema)]
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
        "modules.list",
        "List the modules the module selection dialog and the mod group editor's Insert list show (wbModulesByLoadOrder) with their header flags, masters, load order and file facts.",
        false,
        modules_list,
    );
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
