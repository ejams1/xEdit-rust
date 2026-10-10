// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeInit.pas (the mode selection, `wbAutoModes`,
// `wbPluginModes`, `wbAlwaysMode`, the settings each mode loads with),
// xEdit/xeMainForm.pas (SetAllToMaster, RestorePluginsFromMaster,
// UpdateAllOnam, GenerateSEQFileForFile, the auto mode dispatch of
// tmrGeneratorTimer)

//! The tool modes of xEdit (`xeInit.pas`) and what the modes that act do
//! (`xeMainForm.pas`): the sixteen modes of the release build plus `tmDump`,
//! the mode of `xDump.exe`.
//!
//! The executable of the release is named after its mode (`SSEEdit.exe`,
//! `SSEEditQuickAutoClean.exe`, `SSEEditLODGen.exe`), and a mode may be
//! asked for with a switch too (`-quickautoclean`, `-setesm`, `-masterupdate`
//! and the rest); `DetectAppMode` reads both. The mode decides which of the
//! mode's own settings apply *before* anything loads, so the CLI sets them
//! through [`set_on_load`] on the command line it was given and
//! `Session::load` applies them (`apply_on_load`).
//!
//! The modes that act over the loaded files save every file they changed,
//! as the GUI's `SaveChanged` does at the end of an auto mode.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::command_line::{find_cmd_line_param_next, find_cmd_line_param_switch, has_cmd_line_switch};
use xedit_core::delphi::change_file_ext;
use xedit_core::implementation::file_flags::ModuleFlag;
use xedit_core::implementation::{ElementImpl, ElementState, FileImpl, is_module};
use xedit_core::interface::globals::{self, GameMode, ToolMode, ToolSource};
use xedit_core::interface::{Container, Element, File, FileState};

use crate::save::{FilesSaveResponse, save_file};
use crate::{CommandError, NoParams, Registry, Session};

/// Upstream `wbAutoModes`: the modes that run without user interaction until
/// their final status.
pub const AUTO_MODES: [ToolMode; 12] = [
    ToolMode::tmOnamUpdate,
    ToolMode::tmMasterUpdate,
    ToolMode::tmMasterRestore,
    ToolMode::tmLODgen,
    ToolMode::tmESMify,
    ToolMode::tmESPify,
    ToolMode::tmSortAndCleanMasters,
    ToolMode::tmScript,
    ToolMode::tmCheckForErrors,
    ToolMode::tmCheckForITM,
    ToolMode::tmCheckForDR,
    ToolMode::tmGenerateSEQ,
];

/// Upstream `wbPluginModes`: the auto modes that need a plugin named on the
/// command line.
pub const PLUGIN_MODES: [ToolMode; 6] = [
    ToolMode::tmESMify,
    ToolMode::tmESPify,
    ToolMode::tmSortAndCleanMasters,
    ToolMode::tmCheckForErrors,
    ToolMode::tmCheckForITM,
    ToolMode::tmCheckForDR,
];

/// Upstream `wbAlwaysMode`: the modes every decoded game offers. The games
/// add to it (`xeInit.pas`): `tmMasterUpdate` and `tmMasterRestore` for
/// Fallout 3 and New Vegas, `tmOnamUpdate` for the Skyrim games, nothing
/// more for Fallout 4, Fallout 4 VR, Fallout 76 and Oblivion Remastered, and
/// for Starfield everything but `tmESMify`, `tmESPify` and `tmLODgen`.
pub const ALWAYS_MODES: [ToolMode; 11] = [
    ToolMode::tmView,
    ToolMode::tmEdit,
    ToolMode::tmTranslate,
    ToolMode::tmESMify,
    ToolMode::tmESPify,
    ToolMode::tmSortAndCleanMasters,
    ToolMode::tmLODgen,
    ToolMode::tmScript,
    ToolMode::tmCheckForITM,
    ToolMode::tmCheckForDR,
    ToolMode::tmCheckForErrors,
];

/// The modes the games of `xeInit.pas` support: `wbAlwaysMode` plus the
/// modes of the game.
pub fn supported_modes(mode: GameMode) -> Vec<ToolMode> {
    let mut modes = ALWAYS_MODES.to_vec();
    match mode {
        GameMode::gmFO3 | GameMode::gmFNV => {
            modes.push(ToolMode::tmMasterUpdate);
            modes.push(ToolMode::tmMasterRestore);
        }
        GameMode::gmTES5 | GameMode::gmTES5VR | GameMode::gmEnderal | GameMode::gmSSE | GameMode::gmEnderalSE => {
            modes.push(ToolMode::tmOnamUpdate);
        }
        GameMode::gmSF1 => {
            modes.retain(|mode| !matches!(mode, ToolMode::tmESMify | ToolMode::tmESPify | ToolMode::tmLODgen));
        }
        _ => {}
    }
    modes
}

/// The name of a tool mode as `GetEnumName` gives it, without its `tm`
/// prefix, and therefore also the switch of the mode (`-setesm`).
pub fn mode_name(mode: ToolMode) -> &'static str {
    match mode {
        ToolMode::tmView => "View",
        ToolMode::tmEdit => "Edit",
        ToolMode::tmDump => "Dump",
        ToolMode::tmExport => "Export",
        ToolMode::tmOnamUpdate => "OnamUpdate",
        ToolMode::tmMasterUpdate => "MasterUpdate",
        ToolMode::tmMasterRestore => "MasterRestore",
        ToolMode::tmLODgen => "LODgen",
        ToolMode::tmScript => "Script",
        ToolMode::tmTranslate => "Translate",
        ToolMode::tmESMify => "setESM",
        ToolMode::tmESPify => "clearESM",
        ToolMode::tmSortAndCleanMasters => "SortAndCleanMasters",
        ToolMode::tmCheckForErrors => "CheckForErrors",
        ToolMode::tmCheckForITM => "CheckForITM",
        ToolMode::tmCheckForDR => "CheckForDR",
        ToolMode::tmGenerateSEQ => "GenerateSEQ",
    }
}

/// Upstream `wbToolName` of the mode (`_DoInit`), which names the mode in
/// its messages and its title.
pub fn tool_name(mode: ToolMode) -> &'static str {
    match mode {
        ToolMode::tmESMify => "SettingESMflag",
        ToolMode::tmESPify => "ClearingESMflag",
        ToolMode::tmTranslate => "Trans",
        _ => mode_name(mode),
    }
}

/// The mode a name or switch asks for, as `DetectAppMode` compares it:
/// without case, and `setesm` is `tmESMify`. [`ToolMode::ALL`] holds the
/// order upstream lists the modes in.
pub fn mode_from_name(name: &str) -> Option<ToolMode> {
    let name = name.to_ascii_lowercase();
    // The names of `DetectAppMode`'s ToolModes list (the exe names and the
    // switches), then the other aliases `xeInit.pas` accepts.
    ToolMode::ALL.into_iter().find(|mode| {
        let candidate = mode_name(*mode).to_ascii_lowercase();
        candidate == name
    })
}

/// Port of `DetectAppMode` with `isMode` and `CheckForcedMode`: the game,
/// the tool mode and the tool source the command line and the executable
/// name ask for. `params` are the parameters without the program name and
/// `exe_name` is the program's file name, which upstream lowercases and
/// searches for a mode name.
///
/// The game mode of a game this build does not have (`tes4r`, `enderal`,
/// `enderalse`) is mapped to its game: the tag is what `--game` takes.
pub struct DetectedMode {
    /// The game the command line asks for (`-SSE`, `-FO4`, ...) or the
    /// executable name holds; `None` when neither names one, which upstream
    /// answers with the game selection dialog.
    pub game: Option<GameMode>,
    pub tool_mode: ToolMode,
    pub tool_source: ToolSource,
    /// Upstream `wbForcedModes`: what [`forced_modes`] found, before the
    /// mode lists are searched.
    pub forced_modes: String,
    /// The plugin of `-script:`, when the command line names one.
    pub script: Option<String>,
}

/// Upstream `CheckForcedMode`: a script named on the command line whose
/// extension ends in `pas` (`.FNVpas`) forces the game mode and the script
/// mode (`wbForcedModes`).
pub fn forced_modes(params: &[String]) -> Option<String> {
    // `wbFindCmdLineParam('script', s) or xeFindNextValidCmdLineFileName(1, s)`:
    // either the switch or the first parameter that is not a switch.
    // UPSTREAM-QUIRK: `Pos(s, wbForcedModes)` compares without case
    // folding, and the mode names are lowercase, so only a lowercase
    // extension (`.fnvpas`) selects the game.
    let script = find_cmd_line_param_switch(params, "script")
        .or_else(|| find_cmd_line_param_next(params, &mut 1).filter(|path| Path::new(path).is_file()))?;
    let extension = file_extension(&script);
    let upper = extension.to_ascii_uppercase();
    let at = upper.find("PAS")?;
    // `i = Length(s) - 2`: `pas` is the last three characters of the
    // extension, which then is the game name and `pas` (`.FNVpas`).
    if at != extension.len().saturating_sub(3) {
        return None;
    }
    Some(format!("{},script", &extension[1..extension.len() - 3]))
}

/// `ExtractFileExt` of a path: the extension with its dot, empty when it has
/// none.
fn file_extension(path: &str) -> String {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(index) if index > 0 => name[index..].to_owned(),
        _ => String::new(),
    }
}

/// Upstream `DetectAppMode`. The lists and their order are upstream's:
/// the switch wins over the executable name, and the executable name over
/// the default.
pub fn detect_app_mode(params: &[String], exe_name: &str) -> DetectedMode {
    const SOURCE_MODES: [(&str, ToolSource); 2] = [("plugins", ToolSource::tsPlugins), ("saves", ToolSource::tsSaves)];
    // The names `DetectAppMode` searches for, in its order. `tes4r` is not a
    // game this build has; `xfx` and `np` are the aliases of the release
    // executables (Fallout 4 VR and New Vegas) which upstream lists here.
    const GAME_MODES: [(&str, &str); 14] = [
        ("tes4r", "tes4"),
        ("tes5vr", "tes5vr"),
        ("fo4vr", "fo4vr"),
        ("tes3", "tes3"),
        ("tes4", "tes4"),
        ("tes5", "tes5"),
        ("enderalse", "enderalse"),
        ("enderal", "enderal"),
        ("sse", "sse"),
        ("fo3", "fo3"),
        ("fnv", "fnv"),
        ("fo4", "fo4"),
        ("fo76", "fo76"),
        ("sf1", "sf1"),
    ];
    let exe_name = exe_name.to_ascii_lowercase();
    let forced = forced_modes(params);
    let forced_text = forced.clone().unwrap_or_default();
    let is_forced = |name: &str| forced_text.contains(name);
    let matches = |name: &str| has_cmd_line_switch(params, name) || is_forced(name);

    let tool_source = SOURCE_MODES
        .iter()
        .find(|(name, _)| matches(name) || exe_name.contains(name))
        .map_or(ToolSource::tsPlugins, |(_, source)| *source);

    let game = GAME_MODES
        .iter()
        .find(|(name, _)| matches(name) || exe_name.contains(name))
        .and_then(|(_, tag)| game_of_tag(tag));

    // The tool modes of `DetectAppMode` in their order, then the aliases
    // (`qac` for `quickautoclean` and so on) which are not modes of their
    // own but ask for one.
    let tool_mode = TOOL_MODE_SWITCHES
        .iter()
        .find(|(names, _)| names.iter().any(|name| matches(name) || exe_name.contains(name)))
        .map_or(ToolMode::tmEdit, |(_, mode)| *mode);

    DetectedMode {
        game,
        tool_mode,
        tool_source,
        forced_modes: forced_text,
        script: find_cmd_line_param_switch(params, "script"),
    }
}

/// The switches of `DetectAppMode`'s ToolModes list and the mode each asks
/// for, in upstream's order: `sortandclean` and `sortandcleanmasters` are
/// both `tmSortAndCleanMasters`. The last entry is `edit`, which the
/// executable name of every xEdit contains.
const TOOL_MODE_SWITCHES: [(&[&str], ToolMode); 16] = [
    (&["view"], ToolMode::tmView),
    (&["lodgen"], ToolMode::tmLODgen),
    (&["script"], ToolMode::tmScript),
    (&["translate"], ToolMode::tmTranslate),
    (&["onamupdate"], ToolMode::tmOnamUpdate),
    (&["masterupdate"], ToolMode::tmMasterUpdate),
    (&["masterrestore"], ToolMode::tmMasterRestore),
    (&["setesm"], ToolMode::tmESMify),
    (&["clearesm"], ToolMode::tmESPify),
    (
        &["sortandcleanmasters", "sortandclean"],
        ToolMode::tmSortAndCleanMasters,
    ),
    (&["checkforerrors"], ToolMode::tmCheckForErrors),
    (&["checkforitm"], ToolMode::tmCheckForITM),
    (&["checkfordr"], ToolMode::tmCheckForDR),
    (&["export"], ToolMode::tmExport),
    (&["dump"], ToolMode::tmDump),
    (&["edit"], ToolMode::tmEdit),
];

/// The switches `xeInit.pas` reads for the edit mode's sub modes, which ask
/// for a tool mode of their own in `_DoInit`: `-quickautoclean` runs the
/// quick clean mode of `tmEdit`, `-agc` the ... The entry names the mode and
/// the sub mode it sets.
pub const EDIT_SUB_MODES: [(&[&str], &str); 6] = [
    (&["quickshowconflicts", "qsc"], "quick_show_conflicts"),
    (&["veryquickshowconflicts", "vqsc"], "very_quick_show_conflicts"),
    (&["autogamelink", "agl"], "auto_game_link"),
    (&["quickclean", "qc"], "quick_clean"),
    (&["quickautoclean", "qac"], "quick_auto_clean"),
    (&["autoload"], "auto_load"),
];

/// The `--game` tag of a game mode, as `DetectAppMode` writes it. The tags
/// the port does not have (`tes4r`) answer `None`.
pub fn game_of_tag(tag: &str) -> Option<GameMode> {
    let tag = tag.to_ascii_lowercase();
    // `gmTES4R` is not among the games the port defines plugins for, but it
    // is a game mode of `xeInit.pas`; the tag stays unmapped for now.
    const TAGS: [(&str, GameMode); 14] = [
        ("tes3", GameMode::gmTES3),
        ("tes4", GameMode::gmTES4),
        ("tes4r", GameMode::gmTES4R),
        ("fo3", GameMode::gmFO3),
        ("fnv", GameMode::gmFNV),
        ("tes5", GameMode::gmTES5),
        ("enderal", GameMode::gmEnderal),
        ("fo4", GameMode::gmFO4),
        ("sse", GameMode::gmSSE),
        ("tes5vr", GameMode::gmTES5VR),
        ("enderalse", GameMode::gmEnderalSE),
        ("fo4vr", GameMode::gmFO4VR),
        ("fo76", GameMode::gmFO76),
        ("sf1", GameMode::gmSF1),
    ];
    TAGS.iter().find(|(name, _)| *name == tag).map(|(_, mode)| *mode)
}

/// The settings the command line of `xeInit.pas` gives the load, which
/// [`apply_on_load`] applies. The tool mode is the one that matters most:
/// it selects the mode's own settings.
#[derive(Debug, Clone, Copy)]
pub struct LoadSwitches {
    pub tool_mode: ToolMode,
    pub tool_source: ToolSource,
    /// `-filteronam` and `-noFilteronam`.
    pub filter_onam: Option<bool>,
    /// `-FixPersistence` and `-NoFixPersistence`.
    pub fix_persistence: Option<bool>,
    /// `-alwayssaveonam`.
    pub always_save_onam: bool,
    /// `-FillPNAM` and `-NoFillPNAM`.
    pub fill_pnam: Option<bool>,
    /// `-sortinfo` and `-nosortinfo`.
    pub sort_info: Option<bool>,
    /// `-IKnowWhatImDoing`.
    pub i_know_what_im_doing: bool,
    /// `-fixup`, `-nofixup`, `-skipInternalEditing` and
    /// `-forceInternalEditing`.
    pub internal_edit: Option<bool>,
    /// `-showfixup` and `-hidefixup`.
    pub show_internal_edit: Option<bool>,
    /// `-nobuildrefs`.
    pub build_refs: Option<bool>,
    /// `-SimpleFormIDs`.
    pub simple_form_ids: bool,
    /// The switches `-IKnowWhatImDoing` and `-IKnowIllBreakMyGameWithThis`
    /// unlock.
    pub dangerous: DangerousSwitches,
}

impl Default for LoadSwitches {
    fn default() -> Self {
        Self {
            tool_mode: ToolMode::tmEdit,
            tool_source: ToolSource::tsPlugins,
            filter_onam: None,
            fix_persistence: None,
            always_save_onam: false,
            fill_pnam: None,
            sort_info: None,
            i_know_what_im_doing: false,
            internal_edit: None,
            show_internal_edit: None,
            build_refs: None,
            simple_form_ids: false,
            dangerous: DangerousSwitches::default(),
        }
    }
}

/// The switches of the load, which the CLI sets from its command line
/// before a session loads and `Session::load` applies.
static ON_LOAD: Mutex<Option<LoadSwitches>> = Mutex::new(None);

/// The switches `-IKnowWhatImDoing` unlocks (`xeInit.pas`), which the port
/// applies where it has the feature.
#[derive(Debug, Clone, Copy, Default)]
pub struct DangerousSwitches {
    pub allow_make_partial: bool,
    pub allow_master_files_edit: bool,
    pub allow_edit_hedr_version: bool,
    pub strip_empty_masters: bool,
    pub allow_esp_masters: bool,
    /// `-IKnowWhatImDoing -IKnowIllBreakMyGameWithThis`: the game master may
    /// be edited.
    pub allow_edit_game_master: bool,
}

/// The tool mode the session loads in.
static MODE_ON_LOAD: AtomicU8 = AtomicU8::new(ToolMode::tmEdit as u8);

/// Sets the settings the next sessions load with (`xeInit.pas`).
pub fn set_on_load(switches: LoadSwitches) {
    MODE_ON_LOAD.store(switches.tool_mode as u8, Ordering::Relaxed);
    *ON_LOAD.lock().unwrap() = Some(switches);
}

/// The tool mode the session loads in, for the parts of the load that read
/// it (`README` of `xeInit.pas`).
pub fn mode_on_load() -> ToolMode {
    match MODE_ON_LOAD.load(Ordering::Relaxed) {
        value if value == ToolMode::tmView as u8 => ToolMode::tmView,
        value if value == ToolMode::tmDump as u8 => ToolMode::tmDump,
        value if value == ToolMode::tmExport as u8 => ToolMode::tmExport,
        value if value == ToolMode::tmOnamUpdate as u8 => ToolMode::tmOnamUpdate,
        value if value == ToolMode::tmMasterUpdate as u8 => ToolMode::tmMasterUpdate,
        value if value == ToolMode::tmMasterRestore as u8 => ToolMode::tmMasterRestore,
        value if value == ToolMode::tmLODgen as u8 => ToolMode::tmLODgen,
        value if value == ToolMode::tmScript as u8 => ToolMode::tmScript,
        value if value == ToolMode::tmTranslate as u8 => ToolMode::tmTranslate,
        value if value == ToolMode::tmESMify as u8 => ToolMode::tmESMify,
        value if value == ToolMode::tmESPify as u8 => ToolMode::tmESPify,
        value if value == ToolMode::tmSortAndCleanMasters as u8 => ToolMode::tmSortAndCleanMasters,
        value if value == ToolMode::tmCheckForErrors as u8 => ToolMode::tmCheckForErrors,
        value if value == ToolMode::tmCheckForITM as u8 => ToolMode::tmCheckForITM,
        value if value == ToolMode::tmCheckForDR as u8 => ToolMode::tmCheckForDR,
        value if value == ToolMode::tmGenerateSEQ as u8 => ToolMode::tmGenerateSEQ,
        _ => ToolMode::tmEdit,
    }
}

/// Whether the session's definitions are built as `xDump.dpr` builds them
/// (`wbSimpleRecords` off among the settings) rather than as the editor does:
/// the export of the definitions (`tmExport`) walks the tree of the
/// definitions, and the two builds differ in it where a definition asks
/// `wbSimpleRecords` (`IfThen(wbSimpleRecords, wbByteArray(OFST, ...), ...)`
/// of the TES4 header). `Session::load` reads it.
pub fn loads_like_xdump() -> bool {
    mode_on_load() == ToolMode::tmExport
}

/// Port of the mode settings of `_DoInit`: what the tool mode changes before
/// the definitions are built and the plugins load, in the order upstream
/// applies them (the mode's own block, then `-alwayssaveonam`, `-filteronam`
/// and `-FixPersistence`, then `-nobuildrefs`). `Session::load` calls it.
pub fn apply_on_load() {
    let switches = *ON_LOAD.lock().unwrap();
    let Some(switches) = switches else { return };
    let mode = switches.tool_mode;
    // The switches `_DoInit` reads before the mode block, which the mode
    // block then overrides for the modes it names.
    if switches.i_know_what_im_doing {
        globals::set_i_know_what_im_doing(true);
        // The switches of `-IKnowWhatImDoing`'s block.
        globals::set_allow_make_partial(switches.dangerous.allow_make_partial);
        globals::set_allow_master_files_edit(switches.dangerous.allow_master_files_edit);
        globals::set_allow_edit_hedr_version(switches.dangerous.allow_edit_hedr_version);
        globals::set_strip_empty_masters(switches.dangerous.strip_empty_masters);
        if switches.dangerous.allow_esp_masters {
            globals::set_allow_esp_masters(true);
        }
        globals::set_allow_edit_game_master(switches.dangerous.allow_edit_game_master);
    }
    if let Some(value) = switches.internal_edit {
        globals::set_allow_internal_edit(value);
    }
    if let Some(value) = switches.show_internal_edit {
        globals::set_show_internal_edit(value);
    }
    if let Some(value) = switches.fill_pnam {
        globals::set_fill_pnam(value);
    }
    if let Some(value) = switches.sort_info {
        globals::set_sort_info(value);
    }
    if switches.simple_form_ids {
        globals::set_pretty_form_id(false);
    }

    // `case wbToolMode of`: the mode's own settings.
    match mode {
        ToolMode::tmLODgen => {
            globals::set_i_know_what_im_doing(true);
            globals::set_allow_internal_edit(false);
            globals::set_show_internal_edit(false);
            globals::set_build_refs(false);
        }
        ToolMode::tmScript => {
            globals::set_i_know_what_im_doing(true);
            globals::set_build_refs(true);
        }
        ToolMode::tmOnamUpdate | ToolMode::tmMasterUpdate | ToolMode::tmESMify => {
            globals::set_i_know_what_im_doing(true);
            globals::set_allow_internal_edit(false);
            globals::set_show_internal_edit(false);
            globals::set_build_refs(false);
            globals::set_master_update_filter_onam(mode == ToolMode::tmESMify);
            if mode == ToolMode::tmOnamUpdate {
                globals::set_always_save_onam(true);
                globals::set_always_save_onam_force(true);
            }
        }
        ToolMode::tmMasterRestore
        | ToolMode::tmESPify
        | ToolMode::tmCheckForDR
        | ToolMode::tmCheckForITM
        | ToolMode::tmCheckForErrors => {
            globals::set_i_know_what_im_doing(true);
            globals::set_allow_internal_edit(false);
            globals::set_show_internal_edit(false);
            globals::set_build_refs(false);
        }
        ToolMode::tmTranslate => {
            globals::set_translation_mode(true);
            globals::set_hide_unused(true);
            globals::set_hide_ignored(true);
            globals::set_hide_never_show(true);
        }
        _ => {}
    }

    // The switches `_DoInit` reads after the mode block, which override it.
    if switches.always_save_onam {
        globals::set_always_save_onam(true);
    }
    if let Some(value) = switches.filter_onam {
        globals::set_master_update_filter_onam(value);
    }
    if let Some(value) = switches.fix_persistence {
        globals::set_master_update_fix_persistence(value);
    }
    if let Some(value) = switches.build_refs {
        globals::set_build_refs(value);
    }
}

/// The message of the modes that count (`-checkforitm`, `-checkfordr`,
/// `-checkforerrors`), whose exit code is the count, at most 127.
fn capped_exit_code(count: u64) -> u32 {
    count.min(127) as u32
}

/// One loaded file a tool mode acted on.
#[derive(Serialize, JsonSchema)]
pub struct ToolModeFile {
    /// File name of the plugin.
    pub name: String,
    /// The mode changed this plugin in memory.
    pub changed: bool,
    /// A file was written for it.
    pub saved: bool,
    /// Path it was written to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Where the file that was there went, when one was backed up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    /// Size of the written file in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

impl ToolModeFile {
    fn new(file: &FileImpl) -> Self {
        Self {
            name: file.get_name(),
            changed: false,
            saved: false,
            output: None,
            backup: None,
            bytes: None,
        }
    }

    fn from_save(file: &FileImpl, saved: &FilesSaveResponse) -> Self {
        Self {
            name: file.get_name(),
            changed: true,
            saved: saved.written,
            output: Some(saved.output.clone()),
            backup: saved.backup.clone(),
            bytes: Some(saved.bytes),
        }
    }
}

/// A sequence file the `-generateseq` mode wrote.
#[derive(Serialize, JsonSchema)]
pub struct SeqFile {
    /// The plugin the sequence file is for.
    pub plugin: String,
    /// Path of the sequence file.
    pub path: String,
    /// FormIDs it holds, eight hexadecimal digits each.
    pub form_ids: Vec<String>,
}

/// `tool.modes`: one entry per tool mode of the release build.
#[derive(Serialize, JsonSchema)]
pub struct ToolModeEntry {
    /// The mode as its switch names it (`setesm`, `checkforitm`).
    pub mode: String,
    /// Upstream `wbToolName`.
    pub tool_name: String,
    /// The switch of the mode (`-setesm`).
    pub switch: String,
    /// Whether the mode runs to its final status without a user.
    pub auto: bool,
    /// Whether the mode needs a plugin named on the command line.
    pub plugin_mode: bool,
    /// Whether the mode is one of `wbAlwaysMode` plus the game's own.
    pub supported: bool,
    /// What the mode does over the loaded files.
    pub action: String,
    /// The command that runs it, when this build has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// What is not ported of the mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

fn mode_entries(mode: GameMode) -> Vec<ToolModeEntry> {
    let supported = supported_modes(mode);
    ToolMode::ALL
        .into_iter()
        .map(|tool| {
            let action_command = match tool {
                ToolMode::tmESMify
                | ToolMode::tmESPify
                | ToolMode::tmOnamUpdate
                | ToolMode::tmMasterUpdate
                | ToolMode::tmMasterRestore
                | ToolMode::tmSortAndCleanMasters
                | ToolMode::tmGenerateSEQ
                | ToolMode::tmExport => Some("tool.run"),
                ToolMode::tmCheckForErrors => Some("files.check"),
                ToolMode::tmCheckForITM | ToolMode::tmCheckForDR => Some("files.clean"),
                ToolMode::tmEdit | ToolMode::tmView | ToolMode::tmTranslate => Some("tool.run"),
                ToolMode::tmDump | ToolMode::tmLODgen | ToolMode::tmScript => None,
            };
            ToolModeEntry {
                mode: mode_name(tool).to_ascii_lowercase(),
                tool_name: tool_name(tool).to_owned(),
                switch: format!("-{}", mode_name(tool)),
                auto: AUTO_MODES.contains(&tool),
                plugin_mode: PLUGIN_MODES.contains(&tool),
                supported: supported.contains(&tool),
                action: mode_action(tool).0.to_owned(),
                command: action_command.map(str::to_owned),
                note: mode_action(tool).1.map(str::to_owned),
            }
        })
        .collect()
}

/// What a tool mode does, and what of it is not ported.
fn mode_action(mode: ToolMode) -> (&'static str, Option<&'static str>) {
    match mode {
        ToolMode::tmView => (
            "loads the plugins read-only (no element is editable and nothing is saved)",
            None,
        ),
        ToolMode::tmEdit => ("loads the plugins for editing; the commands do the work", None),
        ToolMode::tmDump => (
            "writes the element tree of a plugin as xDump does",
            Some("run it as `xedit dump --game <game> <plugin>`"),
        ),
        ToolMode::tmExport => (
            "writes the profile of the record definitions of the game (xdump -tmExport of xDump.dpr)",
            None,
        ),
        ToolMode::tmOnamUpdate => (
            "marks the header of every editable plugin with masters modified, so its ONAM list is rebuilt on save",
            None,
        ),
        ToolMode::tmMasterUpdate => (
            "sets the ESM flag of every loaded plugin that is not one, and rebuilds the ONAM list of the files that have masters (-filteronam)",
            None,
        ),
        ToolMode::tmMasterRestore => ("clears the ESM flag of the `.esp` files that have it", None),
        ToolMode::tmLODgen => (
            "generates the LOD of a load order",
            Some("phase 5 wires the `lodgen.generate` command; this build accepts the mode and runs nothing"),
        ),
        ToolMode::tmScript => (
            "runs a Pascal script through the JvInterpreter",
            Some("phase 6 adds the interpreter; this build accepts the mode and runs nothing"),
        ),
        ToolMode::tmTranslate => (
            "loads the plugins in the translate mode: only the translatable elements are compared and edited",
            None,
        ),
        ToolMode::tmESMify => (
            "sets the ESM flag of every loaded plugin that is not one (and marks the header of the files with masters, as -filteronam does)",
            None,
        ),
        ToolMode::tmESPify => (
            "clears the ESM flag of the `.esp` files that have it (RestorePluginsFromMaster)",
            None,
        ),
        ToolMode::tmSortAndCleanMasters => (
            "sorts the masters of every named module by load order and removes the unused ones",
            None,
        ),
        ToolMode::tmCheckForErrors => (
            "checks the last file of the load order for errors and exits with the count, at most 127",
            None,
        ),
        ToolMode::tmCheckForITM => (
            "removes the records identical to their master from the last file of the load order, counts them and saves nothing",
            None,
        ),
        ToolMode::tmCheckForDR => (
            "undeletes and disables the deleted references of the last file of the load order, counts them and saves nothing",
            None,
        ),
        ToolMode::tmGenerateSEQ => (
            "writes `<data>\\Seq\\<plugin>.seq` with the FormIDs of the start-game-enabled quests the plugin adds",
            None,
        ),
    }
}

fn tool_modes_list(session: &mut Session, _: NoParams) -> Result<ToolModesResponse, CommandError> {
    let mode = session.mode()?;
    let detected = detect_app_mode(&[], "xedit.exe");
    Ok(ToolModesResponse {
        tool_mode: mode_name(detected.tool_mode).to_owned(),
        tool_name: tool_name(detected.tool_mode).to_owned(),
        tool_source: match detected.tool_source {
            ToolSource::tsPlugins => "plugins".to_owned(),
            ToolSource::tsSaves => "saves".to_owned(),
        },
        modes: mode_entries(mode),
    })
}

#[derive(Serialize, JsonSchema)]
pub struct ToolModesResponse {
    /// The mode the executable name and the switches select.
    pub tool_mode: String,
    /// Upstream `wbToolName` of that mode.
    pub tool_name: String,
    /// Whether the session has plugins or saves loaded.
    pub tool_source: String,
    pub modes: Vec<ToolModeEntry>,
}

/// `tool.run`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolRunRequest {
    /// The tool mode: the switch of `xeInit.pas` without the dash (`setesm`,
    /// `clearesm`, `masterupdate`, `onamupdate`, `sortandcleanmasters`,
    /// `generateseq`, `checkforerrors`, `checkforitm`, `checkfordr`,
    /// `export`, `edit`), the name of the mode (`esmify`, `espify`,
    /// `masterrestore`, `quickclean`, `quickautoclean`, `dump`, `view`), or
    /// the sub mode name of the edit mode the legacy command line passes
    /// (`quick_clean`, `quick_auto_clean`).
    pub mode: String,
    /// The modules the mode works on, by plugin name, as the modules of
    /// `xeModulesToUse`; every loaded plugin when omitted.
    #[serde(default)]
    pub files: Vec<String>,
    /// Report what the mode would do, but change nothing and write nothing.
    #[serde(default)]
    pub dry_run: bool,
    /// Where the modes that save write the plugins; the loaded paths when
    /// omitted.
    pub output: Option<String>,
    /// Move an existing file at the output path to `<AppName>Edit Backups`
    /// before it is replaced. Defaults to true, as upstream.
    #[serde(default = "default_true")]
    pub backup: bool,
    /// `export`: the format of the profile, `raw` (the default) or
    /// `uespwiki`.
    pub format: Option<String>,
    /// `generateseq`: where the sequence files go; `<data path>\Seq` when
    /// omitted, as xEdit.
    pub seq_path: Option<String>,
}

fn default_true() -> bool {
    true
}

/// `tool.run`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ToolRunResponse {
    /// The mode that ran.
    pub mode: String,
    /// Upstream `wbToolName` of the mode.
    pub tool_name: String,
    /// Nothing was changed: the report is what a run would do.
    pub dry_run: bool,
    /// The mode changed something in memory that a save would write.
    pub changed: bool,
    /// Per plugin: whether the mode changed it and whether it was saved.
    pub files: Vec<ToolModeFile>,
    /// The sequence files the `-generateseq` mode wrote.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub seq: Vec<SeqFile>,
    /// Path of the file the `export` mode wrote.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// `export`: the structure of the definitions, as xDump writes it to
    /// stdout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The count the check modes exit with (`-checkforitm`, `-checkfordr`,
    /// `-checkforerrors`): the records found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    /// The exit code of the mode: the count, at most 127.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
    /// The lines xEdit writes to its message log.
    pub messages: Vec<String>,
    /// What the mode does not do in this build.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The loaded files a mode acts on, in load order (upstream's `Files[]`).
fn loaded_files(session: &Session) -> Result<Vec<std::sync::Arc<FileImpl>>, CommandError> {
    crate::conflicts::session_files(session)
}

/// The plugin a message names: `FileName`, which upstream logs.
fn file_path(file: &FileImpl) -> String {
    file.file_name().to_owned()
}

/// Port of `TfrmMain.SetAllToMaster`: the mode of `-setesm` and
/// `-masterupdate`. Every loaded file that is not an ESM gets the flag (and
/// is renamed to `.esm` on save); with `wbMasterUpdateFilterONAM` the header
/// of the files that are ESMs and have masters is marked modified, so its
/// ONAM list is rebuilt.
fn set_all_to_master(
    files: &[std::sync::Arc<FileImpl>],
    messages: &mut Vec<String>,
    dry_run: bool,
) -> Result<Vec<ToolModeFile>, CommandError> {
    let filter_onam = globals::master_update_filter_onam();
    let mut outcome = Vec::new();
    for file in files {
        let states = file.get_file_states();
        if !file.get_is_esm() && !states.contains(FileState::fsIsHardcoded) {
            messages.push(format!("Setting ESM Flag: {}", file_path(file)));
            if !dry_run {
                // `TwbFile.SetIsESM`: a file that is not editable fails.
                file.set_module_flag(ModuleFlag::Esm, true).map_err(edit_failed)?;
            }
            let mut entry = ToolModeFile::new(file);
            entry.changed = true;
            outcome.push(entry);
        } else if filter_onam && file.get_master_count(true) > 0 {
            // `wbMasterUpdateFilterONAM`: the header of a file with masters
            // is marked modified, so its ONAM list is rebuilt on save. The
            // GUI saves it with the rest (its own list of changed files is
            // not what `SaveChanged` walks).
            if !dry_run && let Some(header) = file.header() {
                header.mark_modified_recursive();
            }
            let mut entry = ToolModeFile::new(file);
            entry.changed = true;
            outcome.push(entry);
        }
    }
    Ok(outcome)
}

/// Port of `TfrmMain.RestorePluginsFromMaster`: the mode of `-clearesm` and
/// `-masterrestore`. The ESM flag of every loaded `.esp` that has it is
/// cleared; a file whose extension is `.esm` keeps it.
fn restore_plugins_from_master(
    files: &[std::sync::Arc<FileImpl>],
    messages: &mut Vec<String>,
    dry_run: bool,
) -> Result<Vec<ToolModeFile>, CommandError> {
    let mut outcome = Vec::new();
    for file in files {
        if file.get_is_esm()
            && !file.get_file_states().contains(FileState::fsIsHardcoded)
            && file_extension(&file.get_name()).eq_ignore_ascii_case(".esp")
        {
            messages.push(format!("Removing ESM Flag: {}", file_path(file)));
            if !dry_run {
                file.set_module_flag(ModuleFlag::Esm, false).map_err(edit_failed)?;
            }
            let mut entry = ToolModeFile::new(file);
            entry.changed = true;
            outcome.push(entry);
        }
    }
    Ok(outcome)
}

/// Port of `TfrmMain.UpdateAllOnam`: the mode of `-onamupdate`. The header
/// of every editable file that is not the game master, the hardcoded records
/// or an official module, and that has masters, is marked modified, so its
/// ONAM list is rebuilt on save.
fn update_all_onam(
    files: &[std::sync::Arc<FileImpl>],
    messages: &mut Vec<String>,
    dry_run: bool,
) -> Result<Vec<ToolModeFile>, CommandError> {
    let mut outcome = Vec::new();
    for file in files {
        let states = file.get_file_states();
        let excluded = states.contains(FileState::fsIsGameMaster)
            || states.contains(FileState::fsIsHardcoded)
            || states.contains(FileState::fsIsOfficial);
        if file.get_is_editable() && !excluded && file.get_master_count(true) > 0 {
            messages.push(format!("Updating ONAM in: {}", file_path(file)));
            if !dry_run && let Some(header) = file.header() {
                header.mark_modified_recursive();
            }
            let mut entry = ToolModeFile::new(file);
            entry.changed = true;
            outcome.push(entry);
        }
    }
    Ok(outcome)
}

/// Port of the `tmSortAndCleanMasters` branch of the auto mode dispatch:
/// `SortMasters` and `CleanMasters` of every module named on the command
/// line that is editable.
fn sort_and_clean_masters(
    session: &Session,
    modules: &[String],
    messages: &mut Vec<String>,
    dry_run: bool,
) -> Result<Vec<ToolModeFile>, CommandError> {
    let files = crate::conflicts::session_files(session)?;
    let mut outcome = Vec::new();
    for file in files {
        if !modules
            .iter()
            .any(|module| file.get_name().eq_ignore_ascii_case(module))
            || !file.get_is_editable()
        {
            continue;
        }
        messages.push(format!("Sorting and cleaning masters of: {}", file.get_name()));
        let mut entry = ToolModeFile::new(&file);
        if dry_run {
            let masters: Vec<String> = (0..file.get_master_count(false))
                .filter_map(|index| file.get_master(index, false))
                .map(|master| master.get_name())
                .collect();
            messages.push(format!("  masters: {}", masters.join(", ")));
        } else {
            file.sort_masters().map_err(edit_failed)?;
            file.clean_masters().map_err(edit_failed)?;
            entry.changed = true;
        }
        outcome.push(entry);
    }
    Ok(outcome)
}

fn edit_failed(error: xedit_core::interface::misc::EditError) -> CommandError {
    CommandError::new("edit_failed", error)
}

/// Port of `TfrmMain.GenerateSEQFileForFile` and of the `xeQuickSEQ` branch
/// of `tmrGeneratorTimer`: the sequence file of a plugin holds the FormIDs
/// of its start-game-enabled quests that are new, or that set the flag on a
/// master's quest. A plugin that needs none is skipped, as upstream logs.
fn generate_seq_file_for_file(
    file: &std::sync::Arc<FileImpl>,
    seq_path: &Path,
    messages: &mut Vec<String>,
    dry_run: bool,
) -> Result<Option<SeqFile>, CommandError> {
    if file.get_load_order() == 0 {
        return Ok(None);
    }
    let mut form_ids: Vec<xedit_core::interface::FormID> = Vec::new();
    // `aFile.GroupBySignature['QUST']`, walked in the order the group holds
    // its records (the order of the file), which for the records of a
    // plugin is not the FormID order of `records()`.
    let group = file.group_by_signature(xedit_core::interface::Signature::new(b"QUST"));
    let records: Vec<std::sync::Arc<xedit_core::implementation::MainRecordImpl>> = match group {
        Some(group) => (0..group.get_element_count())
            .filter_map(|index| group.get_element(index))
            .filter_map(|element| element.as_element_impl().and_then(|element| element.main_record_impl()))
            .collect(),
        None => Vec::new(),
    };
    for record in records {
        if record.get_signature().to_string() != "QUST" {
            continue;
        }
        // `MainRecord.ElementByPath['DNAM - General\Flags']`, tested with
        // `and 1`: the "Start Game Enabled" flag.
        let Some(flags) = record
            .get_element_by_path("DNAM\\Flags")
            .or_else(|| record.get_element_by_path("DNAM - General\\Flags"))
        else {
            continue;
        };
        let enabled = match flags.get_native_value() {
            xedit_core::interface::misc::Variant::Int(value) => value & 1 > 0,
            xedit_core::interface::misc::Variant::UInt(value) => value & 1 > 0,
            _ => false,
        };
        if !enabled {
            continue;
        }
        // New quests, or ones that set the flag on the master's quest.
        let master = record.master();
        let master_enabled = master.as_ref().is_some_and(|master| {
            master
                .get_element_by_path("DNAM\\Flags")
                .or_else(|| master.get_element_by_path("DNAM - General\\Flags"))
                .is_some_and(|flags| match flags.get_native_value() {
                    xedit_core::interface::misc::Variant::Int(value) => value & 1 > 0,
                    xedit_core::interface::misc::Variant::UInt(value) => value & 1 > 0,
                    _ => false,
                })
        });
        if master.is_none() || !master_enabled {
            form_ids.push(record.get_fixed_form_id());
        }
    }
    if form_ids.is_empty() {
        messages.push(format!("Skipped: {} doesn't need sequence file", file.get_name()));
        return Ok(None);
    }
    let path = seq_path.join(change_file_ext(&file.get_name(), ".seq"));
    if !dry_run {
        std::fs::create_dir_all(seq_path)
            .map_err(|error| CommandError::new("io", format!("{}: {error}", seq_path.display())))?;
        let mut bytes = Vec::with_capacity(form_ids.len() * 4);
        for form_id in &form_ids {
            bytes.extend_from_slice(&form_id.to_cardinal().to_le_bytes());
        }
        std::fs::write(&path, &bytes)
            .map_err(|error| CommandError::new("io", format!("{}: {error}", path.display())))?;
    }
    messages.push(format!("Created: {}", path.to_string_lossy()));
    Ok(Some(SeqFile {
        plugin: file.get_name(),
        path: path.to_string_lossy().into_owned(),
        form_ids: form_ids.iter().map(|form_id| form_id.to_string(false)).collect(),
    }))
}

/// The data folder the sequence files go to: `wbDataPath`, which the load
/// sets from the folder of the last loaded plugin.
fn default_seq_path() -> PathBuf {
    PathBuf::from(globals::data_path()).join("Seq")
}

/// Saves every file the mode left unsaved, as the `SaveChanged` at the end
/// of an auto mode does, and reports it per file. `files` is the report of
/// the mode itself, which is extended by the files the mode marked without
/// naming them (the headers of `-filteronam`).
fn save_changed(
    files: &mut Vec<ToolModeFile>,
    loaded: &[std::sync::Arc<FileImpl>],
    output: Option<&str>,
    backup: bool,
) -> Result<(), CommandError> {
    // `SaveChanged` saves the files that are editable and unsaved: the game
    // master and the hardcoded file are never written.
    let unsaved: Vec<&std::sync::Arc<FileImpl>> = loaded
        .iter()
        .filter(|file| file.get_is_editable() && file.element_base().has_state(ElementState::esUnsaved))
        .collect();
    // `output` names one file; a mode that changed several plugins saves
    // each of them where it was loaded from, as the GUI does.
    if output.is_some() && unsaved.len() > 1 {
        return Err(CommandError::new(
            "invalid_params",
            "output names one file, but the mode changed several plugins; save them to their own paths",
        ));
    }
    for file in unsaved {
        let saved = save_file(file, output.map(str::to_owned), false, backup)?;
        match files.iter_mut().find(|entry| entry.name == file.get_name()) {
            Some(entry) => *entry = ToolModeFile::from_save(file, &saved),
            None => files.push(ToolModeFile::from_save(file, &saved)),
        }
    }
    Ok(())
}

/// The sub mode of the edit mode a name asks for: the switch
/// (`-quickautoclean`), the mode name of `tool.run` (`quickautoclean`) or
/// the sub mode name itself (`quick_auto_clean`), which the legacy command
/// line carries (`parse_legacy`). `None` is the edit mode without a sub
/// mode.
pub fn edit_sub_mode_of(name: &str) -> Option<&'static str> {
    EDIT_SUB_MODES
        .iter()
        .find(|(names, sub)| {
            name.eq_ignore_ascii_case(sub) || names.iter().any(|name_of| name.eq_ignore_ascii_case(name_of))
        })
        .map(|(_, sub)| *sub)
}

/// The tool mode a name asks for, with the names `xeInit.pas` accepts for
/// its modes and the sub modes of the edit mode.
pub fn mode_of_name(name: &str) -> Option<ToolMode> {
    // The switch of a mode names it too (`-setesm`).
    let name = name.trim_start_matches(['-', '/']).to_ascii_lowercase();
    let alias = match name.as_str() {
        // The executable names of the modes (`SSEEditQuickAutoClean.exe`)
        // and the sub mode names of the edit mode, which the legacy command
        // line passes to `tool.run` (`parse_legacy`).
        "quickclean"
        | "qc"
        | "quickautoclean"
        | "qac"
        | "quick_clean"
        | "quick_auto_clean"
        | "quick_show_conflicts"
        | "very_quick_show_conflicts"
        | "auto_load"
        | "auto_game_link" => Some(ToolMode::tmEdit),
        "sortandclean" => Some(ToolMode::tmSortAndCleanMasters),
        "seq" | "generateseq" => Some(ToolMode::tmGenerateSEQ),
        "esmify" => Some(ToolMode::tmESMify),
        "espify" => Some(ToolMode::tmESPify),
        "check" => Some(ToolMode::tmCheckForErrors),
        "dump" => Some(ToolMode::tmDump),
        "export" => Some(ToolMode::tmExport),
        "view" => Some(ToolMode::tmView),
        "translate" => Some(ToolMode::tmTranslate),
        "masterupdate" => Some(ToolMode::tmMasterUpdate),
        "masterrestore" => Some(ToolMode::tmMasterRestore),
        "onamupdate" | "onam" => Some(ToolMode::tmOnamUpdate),
        "sortandcleanmasters" => Some(ToolMode::tmSortAndCleanMasters),
        "checkforerrors" => Some(ToolMode::tmCheckForErrors),
        "checkforitm" => Some(ToolMode::tmCheckForITM),
        "checkfordr" => Some(ToolMode::tmCheckForDR),
        "edit" => Some(ToolMode::tmEdit),
        "lodgen" => Some(ToolMode::tmLODgen),
        "script" => Some(ToolMode::tmScript),
        _ => None,
    };
    alias.or_else(|| mode_from_name(&name))
}

fn tool_run(session: &mut Session, request: ToolRunRequest) -> Result<ToolRunResponse, CommandError> {
    let mode = mode_of_name(&request.mode)
        .ok_or_else(|| CommandError::new("invalid_params", format!("{} is not a tool mode", request.mode)))?;
    let mut messages: Vec<String> = Vec::new();
    let mut response = ToolRunResponse {
        mode: mode_name(mode).to_ascii_lowercase(),
        tool_name: tool_name(mode).to_owned(),
        dry_run: request.dry_run,
        changed: false,
        files: Vec::new(),
        seq: Vec::new(),
        output: None,
        text: None,
        count: None,
        exit_code: None,
        messages: Vec::new(),
        note: None,
    };
    let loaded = loaded_files(session)?;
    // `xeModulesToUse`: the modules the mode works on.
    let modules: Vec<String> = if request.files.is_empty() {
        loaded.iter().map(|file| file.get_name()).collect()
    } else {
        request.files.clone()
    };
    match mode {
        ToolMode::tmESMify => {
            // UPSTREAM-QUIRK: the auto mode dispatch of xeMainForm.pas has
            // no branch for `tmESMify`, so the `-setesm` mode of xEdit
            // 4.1.5q changes nothing (it only loads with
            // `wbMasterUpdateFilterONAM := True`, set by `apply_on_load`).
            // The port follows the release: no file is flagged and nothing
            // is saved. `files flags --esm true` is the port of
            // `TwbFile.SetIsESM` and does what the mode's name says.
            globals::set_master_update_filter_onam(true);
            response.files = Vec::new();
            response.note = Some(
                "xEdit 4.1.5q's -setESM mode changes nothing: the auto mode dispatch of xeMainForm.pas has no branch for it (the load just sets -filteronam). Use `files flags --esm true` to set the flag, as TwbFile.SetIsESM does."
                    .to_owned(),
            );
        }
        ToolMode::tmMasterUpdate => {
            response.files = set_all_to_master(&loaded, &mut messages, request.dry_run)?;
            if !request.dry_run {
                save_changed(&mut response.files, &loaded, request.output.as_deref(), request.backup)?;
            }
        }
        ToolMode::tmESPify | ToolMode::tmMasterRestore => {
            response.files = restore_plugins_from_master(&loaded, &mut messages, request.dry_run)?;
            if !request.dry_run {
                save_changed(&mut response.files, &loaded, request.output.as_deref(), request.backup)?;
            }
        }
        ToolMode::tmOnamUpdate => {
            response.files = update_all_onam(&loaded, &mut messages, request.dry_run)?;
            if !request.dry_run {
                save_changed(&mut response.files, &loaded, request.output.as_deref(), request.backup)?;
            }
        }
        ToolMode::tmSortAndCleanMasters => {
            response.files = sort_and_clean_masters(session, &modules, &mut messages, request.dry_run)?;
            if !request.dry_run {
                save_changed(&mut response.files, &loaded, request.output.as_deref(), request.backup)?;
            }
        }
        ToolMode::tmGenerateSEQ => {
            let seq_path = request.seq_path.as_deref().map_or_else(default_seq_path, PathBuf::from);
            for file in &loaded {
                if !modules
                    .iter()
                    .any(|module| file.get_name().eq_ignore_ascii_case(module))
                {
                    continue;
                }
                if let Some(seq) = generate_seq_file_for_file(file, &seq_path, &mut messages, request.dry_run)? {
                    response.seq.push(seq);
                }
            }
        }
        ToolMode::tmCheckForErrors => {
            let request = crate::check::CheckRequest {
                files: Vec::new(),
                records: Vec::new(),
                record_file: None,
                last: true,
            };
            let checked = crate::check::files_check(session, request)?;
            messages.extend(checked.messages.iter().cloned());
            response.count = Some(checked.errors_found);
            response.exit_code = Some(u32::from(checked.exit_code));
        }
        ToolMode::tmCheckForITM | ToolMode::tmCheckForDR => {
            let itm = mode == ToolMode::tmCheckForITM;
            // `mniNavFilterForCleaningClick` then the menu item on
            // `JumpTo(Files[High(Files)].Header)`: the last file of the load
            // order, which is the hardcoded file of the game executable. The
            // mode counts (`AutoModeCheckForITM`/`AutoModeCheckForDR` keep
            // `Operation := 'Count'` and change nothing) and saves nothing
            // (`wbDontSave := True`); the count is what it reports and exits
            // with, at most 127.
            let target = loaded
                .last()
                .ok_or_else(|| CommandError::new("no_session", "no plugin loaded: pass --load"))?
                .clone();
            let clean_request = crate::clean::CleanRequest {
                file: None,
                itm,
                udr: !itm,
                quick: false,
                quick_auto_save: false,
                dry_run: true,
                output: None,
                backup: request.backup,
            };
            // The checks of the walk (`IsEditable`, `IsRemovable`) read
            // `wbEditAllowed`, as `files.clean` arranges for its dry run.
            let allowed = globals::edit_allowed();
            globals::set_edit_allowed(true);
            let cleaned = crate::clean::clean_file(&target, &loaded, &clean_request);
            globals::set_edit_allowed(allowed);
            let cleaned = cleaned?;
            messages.extend(cleaned.messages.iter().cloned());
            response.count = Some(if itm { cleaned.itm } else { cleaned.udr });
            response.exit_code = Some(capped_exit_code(response.count.unwrap_or(0)));
            response.files = loaded
                .last()
                .map(|file| {
                    vec![ToolModeFile {
                        name: file.get_name(),
                        changed: cleaned.unsaved,
                        saved: false,
                        output: None,
                        backup: None,
                        bytes: None,
                    }]
                })
                .unwrap_or_default();
        }
        ToolMode::tmEdit => {
            // The sub modes of the edit mode (`xeInit.pas`): the quick clean
            // modes run over the last loaded plugin, as `xedit clean --quick`
            // does.
            let sub = edit_sub_mode_of(&request.mode);
            if sub == Some("quick_clean") || sub == Some("quick_auto_clean") {
                let output = request.output.clone();
                // `xeInit.pas` sets `xeQuickCleanAutoSave` for
                // `-quickautoclean` (`-qac`) only: `-quickclean` (`-qc`)
                // cleans in memory and writes nothing.
                let auto_save = sub == Some("quick_auto_clean");
                let clean = crate::clean::CleanRequest {
                    file: loaded.last().map(|file| file.get_name()),
                    itm: true,
                    udr: true,
                    quick: true,
                    quick_auto_save: auto_save,
                    dry_run: request.dry_run,
                    output: output.clone(),
                    backup: auto_save && request.backup,
                };
                let cleaned = crate::clean::files_clean(session, clean)?;
                messages.extend(cleaned.messages.iter().cloned());
                response.files = loaded
                    .last()
                    .map(|file| {
                        vec![ToolModeFile {
                            name: file.get_name(),
                            changed: cleaned.itm + cleaned.udr > 0,
                            saved: cleaned.passes.iter().any(|pass| pass.saved.is_some()),
                            output: output.clone(),
                            backup: None,
                            bytes: None,
                        }]
                    })
                    .unwrap_or_default();
            } else if sub == Some("quick_show_conflicts") || sub == Some("very_quick_show_conflicts") {
                response.note = Some(
                    "-quickshowconflicts classifies the conflicts without comparing them; `xedit conflicts --quick-show-conflicts` does it"
                        .to_owned(),
                );
            } else {
                response.note = Some("the edit mode is the default: the commands of the CLI do the work".to_owned());
            }
        }
        ToolMode::tmExport => {
            let format = request.format.as_deref().unwrap_or("RAW");
            let exported =
                crate::export::export_definitions(session, format, request.output.as_deref(), request.dry_run)?;
            messages.push(format!(
                "{} the record definitions of {} to {}",
                if request.dry_run { "Would export" } else { "Exported" },
                globals::game_name(),
                exported.path
            ));
            response.output = Some(exported.path.clone());
            response.text = Some(exported.text.clone());
            response.count = Some(exported.profiles as u64);
        }
        ToolMode::tmView => {
            response.note = Some(
                "the view mode loads the plugins read-only: no element is editable and nothing is saved".to_owned(),
            );
        }
        ToolMode::tmTranslate => {
            response.note = Some(
                "the translate mode is a load setting (`xedit --translate`): only the translatable elements are compared and edited"
                    .to_owned(),
            );
        }
        ToolMode::tmDump => {
            return Err(CommandError::new(
                "unsupported",
                "the dump tool mode is `xedit dump --game <game> <plugin>`",
            ));
        }
        ToolMode::tmLODgen => {
            return Err(CommandError::new(
                "unsupported",
                "the LODGen tool mode runs the `lodgen.generate` command, which phase 5 wires",
            ));
        }
        ToolMode::tmScript => {
            return Err(CommandError::new(
                "unsupported",
                "the script tool mode runs `xedit script run`, which phase 6 adds",
            ));
        }
    }
    response.changed = response.files.iter().any(|file| file.changed) || !response.seq.is_empty();
    response.messages = messages;
    Ok(response)
}

#[derive(Debug)]
/// A tool mode invocation of xEdit's own command line (`xeInit.pas`): the
/// switches a mod manager passes (`SSEEdit.exe -quickautoclean -autoexit
/// -autoload <plugin>`) or the switches an executable name asks for.
pub struct LegacyRun {
    /// The game the switches and the executable name select.
    pub game_tag: Option<String>,
    pub tool_mode: ToolMode,
    /// The mode as `tool.run` names it.
    pub mode: String,
    /// The data folder of `-D:`, with a trailing separator.
    pub data_path: Option<String>,
    /// The plugins to load, in load order, as full paths.
    pub plugins: Vec<String>,
    /// The modules the mode works on (`xeModulesToUse`).
    pub modules: Vec<String>,
    /// The settings the session loads with.
    pub switches: LoadSwitches,
    /// `-O:`: where a mode that saves writes a plugin.
    pub output: Option<String>,
    /// The format of the export mode, when one is given as a plain
    /// parameter (`-export RAW`).
    pub format: Option<String>,
    /// `-R:`: a log file for the message log.
    pub log_file: Option<String>,
    /// `-autoload` (`xeAutoLoad`).
    pub auto_load: bool,
    /// `-autoexit` (`xeAutoExit`): accepted; the CLI has no window to close.
    pub auto_exit: bool,
    /// The switches that were read and that the port does not act on.
    pub ignored_switches: Vec<String>,
}

/// The switches of `xeInit.pas` that make a command line a legacy one: the
/// tool modes, the game modes and the parameters of `DoInitPath`. A command
/// line without one of them is the CLI's own syntax.
const LEGACY_TRIGGERS: &[&str] = &[
    // The tool modes and the sub modes of the edit mode.
    "view",
    "edit",
    "dump",
    "export",
    "lodgen",
    "script",
    "translate",
    "onamupdate",
    "masterupdate",
    "masterrestore",
    "setesm",
    "clearesm",
    "sortandclean",
    "sortandcleanmasters",
    "checkforerrors",
    "checkforitm",
    "checkfordr",
    "quickshowconflicts",
    "qsc",
    "veryquickshowconflicts",
    "vqsc",
    "autogamelink",
    "agl",
    "quickclean",
    "qc",
    "quickautoclean",
    "qac",
    "autoload",
    "autoexit",
    "generateseq",
    "quickedit",
    // The game modes.
    "tes3",
    "tes4",
    "tes4r",
    "fo3",
    "fnv",
    "tes5",
    "enderal",
    "sse",
    "tes5vr",
    "enderalse",
    "fo4",
    "fo4vr",
    "fo76",
    "sf1",
];

/// The parameters of `xeInit.pas` that are followed by a value (`-D:<data>`),
/// which make a command line legacy as well.
const LEGACY_PARAMETERS: &[&str] = &["D", "P", "O", "B", "T", "S", "M", "I", "G", "R", "C", "l"];

/// The parameters of a legacy command line that are not switches at all: the
/// format of the export mode (`-export RAW`), which `xeFindNextValidCmdLineModule`
/// would otherwise take for a plugin.
fn parameter_names(params: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    let mut index = 1usize;
    while let Some(value) = find_cmd_line_param_next(params, &mut index) {
        names.push(value);
    }
    names
}

/// Whether the command line holds a switch of xEdit itself; when it does it
/// is a legacy command line (`parse_legacy`), else the CLI's own syntax.
pub fn is_legacy(params: &[String]) -> bool {
    LEGACY_TRIGGERS.iter().any(|name| has_cmd_line_switch(params, name))
        || LEGACY_PARAMETERS
            .iter()
            .any(|name| find_cmd_line_param_switch(params, name).is_some())
}

/// The paths of the plugins a `plugins.txt` lists as active, in the order of
/// the file. The games of `wbSimplePluginsTxt` (Morrowind to Skyrim LE) name
/// only the active plugins; the later ones mark them with `*`. A `#` starts
/// a comment, and a name that does not resolve against the data folder is
/// reported.
fn read_plugin_list(path: &str, data_path: Option<&str>, simple: bool) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let mut plugins = Vec::new();
    let mut missing = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (active, name) = if simple {
            (true, line)
        } else if let Some(name) = line.strip_prefix('*') {
            (true, name)
        } else {
            (false, line)
        };
        if !active {
            continue;
        }
        let name = name.trim();
        match resolve_module(name, data_path) {
            Some(full) => plugins.push(full),
            None if data_path.is_none() => plugins.push(name.to_owned()),
            None => missing.push(name.to_owned()),
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "the plugins.txt {path} lists plugins that are not in the data folder: {}",
            missing.join(", ")
        ));
    }
    Ok(plugins)
}

/// The full path of a module name: as it is when the file exists, in the
/// data folder of `-D:` when it does not, and `None` when neither.
fn resolve_module(name: &str, data_path: Option<&str>) -> Option<String> {
    if std::path::Path::new(name).is_file() {
        return Some(name.to_owned());
    }
    let data = data_path?;
    let full = format!("{data}{}", name.trim_start_matches(['\\', '/']));
    std::path::Path::new(&full).is_file().then_some(full)
}

/// Port of `DetectAppMode` and the part of `DoInitPath` a tool mode needs
/// from xEdit's own command line: the game, the mode, the plugins to load
/// (`-P:` or the modules named as parameters) and the settings. `Ok(None)`
/// when the command line is the CLI's own syntax ([`is_legacy`]).
pub fn parse_legacy(params: &[String], exe_name: &str) -> Result<Option<LegacyRun>, String> {
    if !is_legacy(params) {
        return Ok(None);
    }
    let detected = detect_app_mode(params, exe_name);
    let mut ignored = Vec::new();
    // The parameters the port does not act on: the temporary folder, the
    // scripts folder, the game ini, the save folder and the code pages.
    for name in ["T", "S", "M", "I", "G"] {
        if find_cmd_line_param_switch(params, name).is_some() {
            ignored.push(format!("-{name}:"));
        }
    }
    let data_path = find_cmd_line_param_switch(params, "D").map(|path| {
        if path.ends_with(['\\', '/']) {
            path
        } else {
            format!("{path}\\")
        }
    });
    let names = parameter_names(params);
    // `xeFindNextValidCmdLineModule`: the parameters that name a module, of
    // which a plugin mode needs one.
    let modules: Vec<String> = names
        .iter()
        .filter(|name| is_module(name))
        .map(|name| {
            let full = resolve_module(name, data_path.as_deref()).unwrap_or_else(|| name.clone());
            std::path::Path::new(&full)
                .file_name()
                .map_or(full.clone(), |file| file.to_string_lossy().into_owned())
        })
        .collect();
    if PLUGIN_MODES.contains(&detected.tool_mode) && modules.is_empty() {
        return Err(format!(
            "{} mode requires a valid plugin name!",
            tool_name(detected.tool_mode)
        ));
    }
    let plugins = match find_cmd_line_param_switch(params, "P") {
        Some(list) => {
            let simple = matches!(
                detected.game,
                Some(GameMode::gmTES3)
                    | Some(GameMode::gmTES4)
                    | Some(GameMode::gmTES4R)
                    | Some(GameMode::gmFO3)
                    | Some(GameMode::gmFNV)
                    | Some(GameMode::gmTES5)
                    | Some(GameMode::gmEnderal)
            );
            read_plugin_list(&list, data_path.as_deref(), simple)?
        }
        None => modules
            .iter()
            .filter_map(|name| resolve_module(name, data_path.as_deref()))
            .collect(),
    };
    if plugins.is_empty() {
        return Err("no plugin to load: pass a plugin name, or a plugin list with -P:".to_owned());
    }
    let mut switches = LoadSwitches {
        tool_mode: detected.tool_mode,
        tool_source: detected.tool_source,
        ..Default::default()
    };
    let flag = |name: &str| has_cmd_line_switch(params, name);
    switches.i_know_what_im_doing = flag("IKnowWhatImDoing");
    switches.always_save_onam = flag("alwayssaveonam");
    switches.simple_form_ids = flag("SimpleFormIDs");
    switches.filter_onam = if flag("filteronam") {
        Some(true)
    } else if flag("noFilteronam") {
        Some(false)
    } else {
        None
    };
    switches.fix_persistence = if flag("FixPersistence") {
        Some(true)
    } else if flag("NoFixPersistence") {
        Some(false)
    } else {
        None
    };
    switches.fill_pnam = if flag("FillPNAM") {
        Some(true)
    } else if flag("NoFillPNAM") {
        Some(false)
    } else {
        None
    };
    switches.sort_info = if flag("sortinfo") {
        Some(true)
    } else if flag("nosortinfo") {
        Some(false)
    } else {
        None
    };
    switches.build_refs = flag("nobuildrefs").then_some(false);
    switches.internal_edit = if flag("fixup") || flag("forceInternalEditing") {
        Some(true)
    } else if flag("nofixup") || flag("skipInternalEditing") {
        Some(false)
    } else {
        None
    };
    switches.show_internal_edit = if flag("showfixup") {
        Some(true)
    } else if flag("hidefixup") {
        Some(false)
    } else {
        None
    };
    switches.dangerous = DangerousSwitches {
        allow_make_partial: flag("AllowMakePartial"),
        allow_master_files_edit: flag("AllowMasterFilesEdit"),
        allow_edit_hedr_version: flag("AllowEditHEDRVersion"),
        strip_empty_masters: flag("StripEmptyMasters"),
        allow_esp_masters: flag("AllowESPMaster"),
        allow_edit_game_master: flag("IKnowIllBreakMyGameWithThis"),
    };
    // The switches that are read and that the port does not act on, which
    // the result names so a caller can report them.
    for name in [
        "AllowMakePartial",
        "AllowMasterFilesEdit",
        "AllowEditHEDRVersion",
        "StripEmptyMasters",
        "StripMasters",
        "AllowESPMaster",
        "IKnowIllBreakMyGameWithThis",
        "TrackAllEditorID",
        "MoreInfoForIndex",
        "moreunknown",
        "reportinjected",
        "noreportinjected",
        "speed",
        "memory",
        "devmode",
        "exceptiontest",
        "resetsettings",
        "ItJustWorksTM",
        "ThisIsFine",
        "GiveMeTheRedPill",
        "IgnoreESL",
        "PseudoESL",
        "IgnoreMedium",
        "PseudoMedium",
        "IgnoreUpdate",
        "PseudoUpdate",
        "EnforceAllMasters",
        "dontremoveoffsetdata",
        "fixuppgrd",
        "moprofile",
        "AllowDirectSaves",
        "skipbsa",
        "forcebsa",
        "bsa",
        "allbsa",
    ] {
        if has_cmd_line_switch(params, name) {
            ignored.push(format!("-{name}"));
        }
    }
    let mode = mode_name(detected.tool_mode).to_ascii_lowercase();
    // `-generateseq:<plugin>` and `-quickedit:<plugin>` name a plugin.
    let quick_plugin =
        find_cmd_line_param_switch(params, "generateseq").or_else(|| find_cmd_line_param_switch(params, "quickedit"));
    let mut modules = modules;
    if let Some(name) = quick_plugin {
        modules = vec![name];
    }
    let format = names
        .iter()
        .find(|name| crate::export::ExportFormat::is_valid(name))
        .cloned();
    Ok(Some(LegacyRun {
        game_tag: detected.game.map(|game| crate::dump::game_tag(game).to_owned()),
        tool_mode: detected.tool_mode,
        mode: if detected.tool_mode == ToolMode::tmEdit {
            // `-quickautoclean` and `-quickclean` are the quick clean modes of
            // the edit mode; the mode name is what `tool.run` takes.
            EDIT_SUB_MODES
                .iter()
                .find(|(names, _)| names.iter().any(|name| has_cmd_line_switch(params, name)))
                .map_or_else(|| mode.clone(), |(_, sub)| (*sub).to_owned())
        } else {
            mode
        },
        data_path,
        plugins,
        modules,
        switches,
        output: find_cmd_line_param_switch(params, "O"),
        format,
        log_file: find_cmd_line_param_switch(params, "R"),
        auto_load: has_cmd_line_switch(params, "autoload"),
        auto_exit: has_cmd_line_switch(params, "autoexit"),
        ignored_switches: ignored,
    }))
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "tool.modes",
        "List the tool modes of xEdit (`xeInit.pas`) with the switch of each and what it does.",
        false,
        tool_modes_list,
    );
    registry.register(
        "tool.run",
        "Run a tool mode over the loaded plugins (xeInit.pas and the auto modes of xeMainForm.pas).",
        true,
        tool_run,
    );
}

/// The temporary data path helper of the tests.
#[cfg(test)]
mod tests {
    use super::*;

    fn params(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn a_switch_selects_the_mode() {
        let detected = detect_app_mode(&params(&["-SSE", "-quickautoclean"]), "xedit.exe");
        // `-quickautoclean` is not a mode of its own: the edit mode's sub
        // mode. `-SSE` is the game.
        assert_eq!(detected.tool_mode, ToolMode::tmEdit);
        assert_eq!(detected.game, Some(GameMode::gmSSE));

        let detected = detect_app_mode(&params(&["-FO4", "-setesm", "MyMod.esp"]), "xedit.exe");
        assert_eq!(detected.tool_mode, ToolMode::tmESMify);
        assert_eq!(detected.game, Some(GameMode::gmFO4));

        let detected = detect_app_mode(&params(&["-masterupdate"]), "FO4Edit.exe");
        assert_eq!(detected.tool_mode, ToolMode::tmMasterUpdate);
        assert_eq!(detected.game, Some(GameMode::gmFO4));
    }

    #[test]
    fn the_executable_name_selects_the_mode_and_the_game() {
        let detected = detect_app_mode(&[], "SSEEditQuickAutoClean.exe");
        assert_eq!(detected.game, Some(GameMode::gmSSE));
        assert_eq!(detected.tool_mode, ToolMode::tmEdit);

        let detected = detect_app_mode(&[], "FNVEdit.exe");
        assert_eq!(detected.game, Some(GameMode::gmFNV));
        assert_eq!(detected.tool_mode, ToolMode::tmEdit, "every xEdit name holds Edit");

        let detected = detect_app_mode(&[], "SSEEditLODGen.exe");
        assert_eq!(detected.tool_mode, ToolMode::tmLODgen);
    }

    #[test]
    fn the_script_extension_forces_the_game_and_the_script_mode() {
        let dir = std::env::temp_dir().join(format!("xedit-tool-modes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // `Pos(s, wbForcedModes)` is case sensitive, so the extension has to
        // be written as the game mode names it (`fnv`, not `FNV`).
        let script = dir.join("MyScript.fnvpas");
        std::fs::write(&script, "").unwrap();
        let detected = detect_app_mode(&params(&[&script.to_string_lossy()]), "xedit.exe");
        assert_eq!(detected.tool_mode, ToolMode::tmScript);
        assert_eq!(detected.game, Some(GameMode::gmFNV));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_mode_has_a_name_and_a_switch() {
        for mode in ToolMode::ALL {
            let name = mode_name(mode);
            assert!(!name.is_empty(), "{mode:?}");
            assert_eq!(mode_of_name(&name.to_ascii_lowercase()), Some(mode), "{name}");
            assert_eq!(mode_of_name(&format!("-{name}")), Some(mode), "{name}");
        }
        assert_eq!(mode_of_name("qac"), Some(ToolMode::tmEdit));
        assert_eq!(mode_of_name("nonsense"), None);
    }

    #[test]
    fn the_modes_of_a_game_follow_the_switch_settings_of_xe_init() {
        assert!(!supported_modes(GameMode::gmFO4).contains(&ToolMode::tmOnamUpdate));
        assert!(supported_modes(GameMode::gmSSE).contains(&ToolMode::tmOnamUpdate));
        assert!(supported_modes(GameMode::gmFNV).contains(&ToolMode::tmMasterUpdate));
        assert!(supported_modes(GameMode::gmTES4).contains(&ToolMode::tmESMify));
        assert!(!supported_modes(GameMode::gmSF1).contains(&ToolMode::tmESMify));
        assert!(!supported_modes(GameMode::gmSF1).contains(&ToolMode::tmLODgen));
    }
}
