// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The inspection commands (the session, its files, records and elements
//! as JSON) and `elements.set`, the first command that changes plugin data.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use xedit_core::implementation::{ElementImpl, FileImpl, MainRecordImpl, wb_file};
use xedit_core::interface::globals::{GameMode, data_path, game_name};
use xedit_core::interface::misc::Variant;
use xedit_core::interface::{
    Element, ElementRef, FileRef, FileStates, FormID, MainRecordRef, clear_files, record_by_load_order_form_id,
};

use crate::dump::{game_tag, load_hardcoded, setup_game_for_edit};
use crate::{CommandError, NoParams, Registry, Session};

/// Whether [`Session::load`] turns on the PNAM fill (`wbFillPNAM`).
static FILL_PNAM_ON_LOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether [`Session::load`] loads as the quick clean modes do.
static QUICK_CLEAN_ON_LOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Port of the settings `xeInit.pas` gives the quick clean modes
/// (`-quickclean`, `-quickautoclean`) before anything loads: the full
/// record definitions (`wbSimpleRecords := False`), the internal edits of
/// the load (`wbAllowInternalEdit := True`), no topic response lists
/// (`wbFillINOM`, `wbFillINOA` off), the worldspace offsets dropped
/// (`wbRemoveOffsetData`) and, where the game sorts the responses of a
/// topic, the PNAM fill (`wbFillPNAM`). The sessions loaded
/// afterwards use them; `xedit clean --quick` sets them.
pub fn set_quick_clean_on_load(value: bool) {
    QUICK_CLEAN_ON_LOAD.store(value, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn quick_clean_on_load() -> bool {
    QUICK_CLEAN_ON_LOAD.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether [`Session::load`] loads as the `-CheckForErrors` mode does.
static CHECK_ON_LOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Port of the settings `xeInit.pas` gives the `-CheckForErrors` mode (and
/// `-CheckForITM`, `-CheckForDR`, `-MasterRestore`, `-clearESM`) before
/// anything loads: no internal edits of the load (`wbAllowInternalEdit :=
/// False`; the edits of `wbEditAllowed` still apply) and no reference
/// information (`wbBuildRefs := False`, which some checks read). The
/// sessions loaded afterwards use them; `xedit check` sets them.
pub fn set_check_on_load(value: bool) {
    CHECK_ON_LOAD.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// Port of `-FillPNAM` and of the `wbFillPNAM := True` of the quick clean
/// modes in `xeInit.pas`: the sessions loaded afterwards give a topic
/// response without `PNAM` the `PNAM` of the response it follows when it is
/// built (`TwbMainRecord.Init`), as xEdit does in those modes. It changes
/// the comparison of responses with masters that have no `PNAM` (the game
/// masters of Oblivion to Skyrim), so it must be set before the load.
pub fn set_fill_pnam_on_load(value: bool) {
    FILL_PNAM_ON_LOAD.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// Whether [`Session::load`] loads in the translate tool mode.
static TRANSLATE_ON_LOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The language [`Session::load`] sets in place of the game's default.
static LANGUAGE_ON_LOAD: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Port of the translate tool mode (`tmTranslate`, `-translate` or an
/// executable named `...Trans...`): `xeInit.pas` sets `wbTranslationMode`,
/// which makes every element that is not translatable ignored by the
/// comparison of records (`GetConflictPriority`), hides the unused, ignored
/// and never shown fields, and loads the archives for the strings; the main
/// form then offers none of the commands that change the structure of a
/// plugin (`refuse_in_translate_mode`), only the values of the translatable
/// elements and the localization commands. The sessions loaded afterwards
/// use it.
pub fn set_translate_on_load(value: bool) {
    TRANSLATE_ON_LOAD.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// Port of `-l:<language>`: the language of the string tables, in place of
/// the default of the game, for the sessions loaded afterwards.
pub fn set_language_on_load(language: Option<&str>) {
    *LANGUAGE_ON_LOAD.lock().unwrap() = language.map(str::to_owned);
}

/// The `--compare-to` file of the next load (the GUI's "Compare to..."): the
/// file loads as a compare load of the last plugin of `--load`, takes its
/// load order, and is compared with it record by record.
static COMPARE_TO_ON_LOAD: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Sets the path of the compare load of the sessions loaded afterwards.
pub fn set_compare_to_on_load(path: Option<&str>) {
    *COMPARE_TO_ON_LOAD.lock().unwrap() = path.map(str::to_owned);
}

/// The `if wbTranslationMode then Exit` of the menu handlers that change
/// the structure of a plugin (and the menu items the translate mode hides):
/// the command is refused in the translate mode.
pub(crate) fn refuse_in_translate_mode(command: &str) -> Result<(), CommandError> {
    if xedit_core::interface::globals::translation_mode() {
        return Err(CommandError::new(
            "translate_mode",
            format!("{command} is not available in the translate mode"),
        ));
    }
    Ok(())
}

impl Session {
    /// Loads the plugins of a game in the order given, with their masters.
    pub fn load(game: &str, plugins: &[String]) -> Result<Self, String> {
        Self::load_with(game, |_mode| Ok(plugins.to_vec()))
    }

    /// Port of the module selection of a legacy command line: the plugins
    /// are `wbModulesByLoadOrder`'s active modules with their masters
    /// (`SimulateLoad`), read after the game is set up because
    /// `wbLoadModules` needs its tables (the game master, the official DLC,
    /// the Creation Club list, `Plugins.txt`).
    pub fn load_legacy(game: &str) -> Result<Self, String> {
        Self::load_with(game, |_mode| crate::tool_modes::legacy_load_order())
    }

    /// [`Self::load`] with the plugin list computed once the game is set
    /// up (`wbLoadModules`).
    fn load_with(game: &str, plugins: impl FnOnce(GameMode) -> Result<Vec<String>, String>) -> Result<Self, String> {
        // `xeInit.pas` sets `wbStartTime := Now` right before the loader
        // thread starts: the `[mm:ss]` prefixes of the message log count
        // from the load's start.
        crate::script::reset_progress_start();
        // `xDump.dpr` builds its definitions with settings of its own, which
        // the export of the definitions has to see (`wbSimpleRecords` among
        // them); every other mode loads them as the editor does.
        let xdump = crate::tool_modes::loads_like_xdump();
        // `-saves` (`_DoInit`'s tool source): the definitions of the saves
        // of the game rather than those of its plugins, which the export of
        // the save definitions walks (`xDump.exe -saves -export`).
        let saves = crate::tool_modes::on_load_tool_source() == xedit_core::interface::globals::ToolSource::tsSaves;
        let mode = if xdump {
            if saves {
                crate::dump::setup_game_for_save_definitions(game)?
            } else {
                crate::dump::setup_game(game)?
            }
        } else {
            setup_game_for_edit(game)?
        };
        // The editor's settings apply before the plugins load: the load
        // itself edits records under `wbAllowInternalEdit`.
        crate::save::apply_edit_settings(mode);
        // `TfrmMain.FormCreate` sets `wbDisplayShorterNames`, so the editor
        // names a record `EditorID "Full Name" [SIG:FormID]`; xDump keeps
        // the long form its dumps compare.
        if !xdump {
            xedit_core::interface::globals::set_display_shorter_names(true);
        }
        // `wbLoadModules` reads the tables the setup just filled (the game
        // master, the official DLC, the Creation Club list) and the data
        // folder and plugin list of the command line.
        let plugins = plugins(mode)?;
        // The data folder of the session: the folder of the plugins.
        let data = plugins
            .last()
            .map(|path| xedit_core::delphi::extract_file_path(path))
            .unwrap_or_default();
        // `DoInitPath`: the folder of the game's inis and the inis; then the
        // `wbLoadBSAs` of the settings file and of `-skipbsa`/`-forcebsa`.
        if !xdump {
            crate::resources::init_game_paths(mode, &data);
            crate::resources::apply_load_settings();
        }
        // `xeInit.pas`: the tool mode's own settings, which decide what the
        // load does (the internal edits, the reference information, the
        // translate mode, the archives and so on).
        crate::tool_modes::apply_on_load();
        // `-l:`, else the language of the game inis.
        if let Some(language) = LANGUAGE_ON_LOAD.lock().unwrap().as_deref() {
            xedit_core::interface::globals::set_language(language);
        } else if !xdump && let Some(language) = crate::resources::game_ini_language(mode) {
            xedit_core::interface::globals::set_language(&language);
        }
        let translate = TRANSLATE_ON_LOAD.load(std::sync::atomic::Ordering::Relaxed);
        xedit_core::interface::globals::set_translation_mode(translate);
        if translate {
            use xedit_core::interface::globals::{set_hide_ignored, set_hide_never_show, set_hide_unused};
            set_hide_unused(true);
            set_hide_ignored(true);
            set_hide_never_show(true);
            // `tmTranslate`: needed for localization.
            if mode >= GameMode::gmTES5 {
                xedit_core::interface::globals::set_load_bsas(true);
            }
        }
        // `-FillPNAM`: the responses of a topic get a `PNAM` when they are
        // built (`wbFillPNAM`).
        xedit_core::interface::globals::set_fill_pnam(FILL_PNAM_ON_LOAD.load(std::sync::atomic::Ordering::Relaxed));
        if quick_clean_on_load() {
            // `xeInit.pas` for `-quickclean` and `-quickautoclean`: the PNAM
            // fill where the game sorts the responses, the internal edits
            // of the load (`wbAllowInternalEdit`) also in Oblivion, no
            // `INOM` and `INOA` lists on the topics, the offsets of the
            // worldspaces dropped, and the full definitions
            // (`wbSimpleRecords := False`, applied by `setup_game_for_edit`).
            use xedit_core::interface::globals::{
                can_sort_info, set_allow_internal_edit, set_fill_inoa, set_fill_inom, set_fill_pnam,
                set_remove_offset_data,
            };
            if can_sort_info() {
                set_fill_pnam(true);
            }
            set_allow_internal_edit(true);
            set_fill_inom(false);
            set_fill_inoa(false);
            set_remove_offset_data(true);
        }
        // `wbBuildRefs`: the GUI builds the references on load, except in
        // Morrowind and in the modes that only check.
        let check = CHECK_ON_LOAD.load(std::sync::atomic::Ordering::Relaxed);
        xedit_core::interface::globals::set_build_refs(!check && mode != GameMode::gmTES3);
        if check {
            // `xeInit.pas` for `tmCheckForErrors`: the load makes no
            // internal edits and builds no references. The mode edits
            // (`wbEditAllowed`), so the records the load builds (the file
            // headers) get the fixes of their init all the same; the CLI's
            // edit gate is set again after the load.
            xedit_core::interface::globals::set_allow_internal_edit(false);
            xedit_core::interface::globals::set_edit_allowed(true);
            xedit_core::interface::globals::set_load_bsas(false);
        }
        clear_files();
        let mut files = Vec::new();
        for path in &plugins {
            // Each plugin takes the next free slot after its masters.
            let file = wb_file(path, i32::MAX, FileStates::empty()).map_err(|error| error.to_string())?;
            files.push(file);
        }
        // The `--compare-to` file (the GUI's "Compare to..."): it is loaded
        // in the place of the last plugin of `--load`, which it takes the
        // load order of, and compared with it. As the GUI copies the chosen
        // file into the data folder, the compare load has to sit in the
        // folder of that plugin, so that the masters resolve to the same
        // files.
        if let Some(path) = COMPARE_TO_ON_LOAD.lock().unwrap().take() {
            let compare_to = files
                .last()
                .ok_or_else(|| "--compare-to needs a plugin loaded with --load".to_owned())?;
            let data = plugins
                .last()
                .map(|path| xedit_core::delphi::extract_file_path(path))
                .unwrap_or_default();
            let folder = xedit_core::delphi::extract_file_path(&path);
            if !folder.eq_ignore_ascii_case(&data) {
                return Err(format!(
                    "the --compare-to file has to sit in the folder of the plugin it compares to ({data}): {path}"
                ));
            }
            let file =
                xedit_core::implementation::wb_file_compare_to(&path, compare_to.load_order(), &compare_to.get_name())
                    .map_err(|error| error.to_string())?;
            files.push(file);
        }
        if let (Some(file), Some(path)) = (files.last(), plugins.last()) {
            if xdump {
                crate::resources::load_dump_resources(file, path, &data, mode);
            } else {
                // `TLoaderThread.Execute`: the archives of the game inis and
                // of every module, the data folder, `wbLoaderDone`.
                crate::resources::load_session_resources(&data)?;
            }
            load_hardcoded()?;
        }
        Ok(Self {
            game: Some(mode),
            files,
            edit_allowed: false,
            dirty_info: Vec::new(),
        })
    }

    pub(crate) fn mode(&self) -> Result<GameMode, CommandError> {
        self.game
            .ok_or_else(|| CommandError::new("no_session", "no game loaded: pass --game and --load"))
    }

    /// The loaded file with the name, or the only loaded file when no name
    /// is given.
    pub(crate) fn file(&self, name: Option<&str>) -> Result<Arc<FileImpl>, CommandError> {
        self.mode()?;
        match name {
            Some(name) => self
                .files
                .iter()
                .find(|file| file.get_name().eq_ignore_ascii_case(name))
                .cloned()
                .ok_or_else(|| CommandError::new("unknown_file", format!("{name} is not loaded"))),
            None => match self.files.as_slice() {
                [file] => Ok(file.clone()),
                [] => Err(CommandError::new("no_session", "no plugin loaded: pass --load")),
                _ => Err(CommandError::new(
                    "ambiguous_file",
                    "several plugins are loaded: pass file",
                )),
            },
        }
    }

    pub(crate) fn record(&self, form_id: &str, file: Option<&str>) -> Result<MainRecordRef, CommandError> {
        let form_id = parse_form_id(form_id)?;
        let seen_from: Option<FileRef> = match file {
            Some(name) => Some(self.file(Some(name))? as FileRef),
            None => {
                self.mode()?;
                self.files.last().map(|file| file.clone() as FileRef)
            }
        };
        record_by_load_order_form_id(form_id, seen_from.as_ref()).ok_or_else(|| {
            CommandError::new(
                "unknown_record",
                format!("no record with FormID {}", form_id.to_string(false)),
            )
        })
    }
}

/// A load order FormID as eight hexadecimal digits.
pub(crate) fn parse_form_id(text: &str) -> Result<FormID, CommandError> {
    let digits = text.trim().trim_start_matches("0x");
    u32::from_str_radix(digits, 16)
        .map(FormID::from_cardinal)
        .map_err(|_| CommandError::new("invalid_params", format!("{text} is not a hexadecimal FormID")))
}

/// `session.info`.
#[derive(Serialize, JsonSchema)]
pub struct SessionInfo {
    /// Game tag as given to `--game`.
    pub game: String,
    /// Upstream game name, such as `Skyrim` or `Fallout4`.
    pub game_name: String,
    /// Data folder the plugins load from.
    pub data_path: String,
    /// Names of the loaded plugins in load order.
    pub files: Vec<String>,
}

fn session_info(session: &mut Session, _: NoParams) -> Result<SessionInfo, CommandError> {
    let mode = session.mode()?;
    Ok(SessionInfo {
        game: game_tag(mode).to_owned(),
        game_name: game_name(),
        data_path: data_path(),
        files: all_files(session).iter().map(|file| file.get_name()).collect(),
    })
}

/// `session.load`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionLoadRequest {
    /// The plugins to load, in load order, as full paths (a name resolves
    /// against the data folder of the session). Masters load with them, as
    /// they do for `--load`.
    pub plugins: Vec<String>,
    /// The tool mode whose load settings apply, as `xeInit.pas` applies them
    /// before anything loads (`quickclean`, `quickautoclean`,
    /// `checkforerrors`, `translate`, `edit`). The settings of the mode are
    /// part of the result; `edit` (the default) applies none of its own.
    pub mode: Option<String>,
    /// Reload although a loaded plugin has unsaved changes, which the reload
    /// discards.
    #[serde(default)]
    pub force: bool,
    /// Report what the load would do (the plugins, the mode and its
    /// settings, the changes a reload discards), but keep the session as it
    /// is.
    #[serde(default)]
    pub dry_run: bool,
}

/// `session.load`: the response.
#[derive(Serialize, JsonSchema)]
pub struct SessionLoadResponse {
    /// The game tag of the session, which a load does not change.
    pub game: String,
    /// The tool mode the load applies, when `mode` named one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    pub dry_run: bool,
    /// The plugins the load would put in the session, in load order, with
    /// their masters.
    pub files: Vec<String>,
    /// The plugins whose unsaved changes the reload discards.
    pub discarded: Vec<String>,
    /// The load settings the mode applies, as `xeInit.pas` sets them
    /// (`wbFillPNAM`, `wbAllowInternalEdit`, `wbRemoveOffsetData`,
    /// `wbSimpleRecords` and the rest).
    pub settings: Vec<String>,
    /// What the load does not do in this build.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `session.load`: loads another plugin list (or the same one with another
/// mode's settings) into the running session. The session is the one
/// `serve`/`mcp` keep open, where the plugins are otherwise fixed at
/// startup; the edit gate the operator set stays as it is.
fn session_load(session: &mut Session, request: SessionLoadRequest) -> Result<SessionLoadResponse, CommandError> {
    use xedit_core::implementation::ElementImpl;
    use xedit_core::interface::globals::{ToolMode, can_sort_info};

    let game = session.mode()?;
    // The mode's settings, as `xeInit.pas` reads them (`_DoInit`): the tool
    // mode, and the quick clean and check modes of the edit mode.
    let sub = request
        .mode
        .as_deref()
        .and_then(crate::tool_modes::edit_sub_mode_of)
        .map(str::to_owned);
    let tool_mode = match request.mode.as_deref() {
        None => ToolMode::tmEdit,
        Some(name) => crate::tool_modes::mode_of_name(name).ok_or_else(|| {
            CommandError::new(
                "invalid_params",
                format!(
                    "{name} is not a tool mode: use the sub mode of the edit mode (quickclean, quickautoclean), a mode of `tool.modes`, or omit it"
                ),
            )
        })?,
    };
    let quick = matches!(sub.as_deref(), Some("quick_clean" | "quick_auto_clean"));
    let translate = tool_mode == ToolMode::tmTranslate;
    let check = tool_mode == ToolMode::tmCheckForErrors;
    let mut settings = Vec::new();
    if quick {
        settings.push(if can_sort_info() {
            "wbFillPNAM := True".to_owned()
        } else {
            "wbFillPNAM unchanged (the game does not sort the responses)".to_owned()
        });
        settings.push("wbAllowInternalEdit := True".to_owned());
        settings.push("wbFillINOM := False, wbFillINOA := False".to_owned());
        settings.push("wbRemoveOffsetData := True".to_owned());
        settings.push("wbSimpleRecords := False (the full record definitions)".to_owned());
    }
    if translate {
        settings.push("wbTranslationMode := True".to_owned());
        settings.push("wbHideUnused, wbHideIgnored, wbHideNeverShow := True".to_owned());
        settings.push("wbLoadBSAs := True (the games from Skyrim on)".to_owned());
    }
    if check {
        settings.push("wbAllowInternalEdit := False, wbBuildRefs := False, wbLoadBSAs := False".to_owned());
    }
    // A reload drops what a loaded plugin has not saved. The files of the
    // session, not every file of the process.
    let discarded: Vec<String> = session
        .files
        .iter()
        .filter(|file| {
            file.element_base()
                .has_state(xedit_core::implementation::ElementState::esUnsaved)
        })
        .map(|file| file.get_name())
        .collect();
    if !discarded.is_empty() && !request.force {
        return Err(CommandError::new(
            "invalid_state",
            format!(
                "{} has unsaved changes, which a load discards: save them first, or load with force",
                discarded.join(", ")
            ),
        ));
    }
    if request.dry_run {
        // Without loading, the report names the plugins as they were given;
        // the masters a plugin lists load with it, as they do for a real
        // load, whose report names every file.
        return Ok(SessionLoadResponse {
            game: crate::dump::game_tag(game).to_owned(),
            mode: request.mode.clone(),
            dry_run: true,
            files: request
                .plugins
                .iter()
                .map(|path| {
                    std::path::Path::new(path)
                        .file_name()
                        .map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned())
                })
                .collect(),
            discarded,
            settings,
            note: None,
        });
    }
    // The settings of the mode apply to the definitions and the load, so
    // they are set before `Session::load` runs, exactly as the CLI sets
    // them before its own load.
    crate::tool_modes::set_on_load(crate::tool_modes::LoadSwitches {
        tool_mode,
        ..Default::default()
    });
    crate::commands::set_quick_clean_on_load(quick);
    crate::commands::set_translate_on_load(translate);
    crate::commands::set_check_on_load(check);
    let edit_allowed = session.edit_allowed;
    let reloaded = match Session::load(crate::dump::game_tag(game), &request.plugins) {
        Ok(reloaded) => reloaded,
        Err(message) => {
            // `Session::load` clears the process-wide file list before it
            // loads, so a failed load leaves the session with no plugin at
            // all; the caller has to load again.
            session.files.clear();
            return Err(CommandError::new(
                "load_failed",
                format!("{message} (the session has no plugins now)"),
            ));
        }
    };
    let files: Vec<String> = reloaded.files.iter().map(|file| file.get_name()).collect();
    *session = reloaded;
    session.edit_allowed = edit_allowed;
    Ok(SessionLoadResponse {
        game: crate::dump::game_tag(game).to_owned(),
        mode: request.mode.clone(),
        dry_run: false,
        files,
        discarded,
        settings,
        note: None,
    })
}

/// Every file of the session, including the masters that loaded with the
/// plugins and the hardcoded records, in load order.
fn all_files(session: &Session) -> Vec<FileRef> {
    let _ = session;
    let mut files = xedit_core::interface::files();
    files.sort_by_key(|file| file.get_load_order());
    files
}

/// One entry of `files.list`.
#[derive(Serialize, JsonSchema)]
pub struct FileInfo {
    pub name: String,
    pub load_order: i32,
    /// Master files in the order of the file header.
    pub masters: Vec<String>,
    pub record_count: usize,
    pub is_esm: bool,
    pub is_localized: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct FileList {
    pub files: Vec<FileInfo>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesListRequest {
    /// `edFileNameFilter` of the navigation tree, "Filter by filename": list
    /// only the files whose name contains this text, compared without case.
    pub name_filter: Option<String>,
    /// `cbRegExFilter` of that box: read the filter as a regular expression
    /// (case-insensitive, as the GUI's `TPerlRegEx` with `preCaseLess`).
    #[serde(default)]
    pub regex: bool,
}

/// Whether the file name filter of the navigation tree keeps a name:
/// `edFileNameFilterChange` (a substring without case, or the whole text as
/// a `preCaseLess` regular expression). A bad expression fails with
/// `invalid_params` where the GUI colours its edit box.
fn name_filter_matches(filter: &str, regex: bool, name: &str) -> Result<bool, CommandError> {
    if filter.is_empty() {
        return Ok(true);
    }
    if regex {
        let mut compiled = xedit_core::perl_regex::PerlRegEx::new();
        compiled.set_regex(filter.to_owned());
        compiled.set_options(1 << xedit_core::perl_regex::OPTION_CASE_LESS);
        compiled.set_subject(name.to_owned());
        compiled.match_again().map_err(|error| {
            CommandError::new(
                "invalid_params",
                format!("the file name filter is not a regular expression: {}", error.message()),
            )
        })
    } else {
        Ok(name.to_lowercase().contains(&filter.to_lowercase()))
    }
}

fn files_list(session: &mut Session, request: FilesListRequest) -> Result<FileList, CommandError> {
    session.mode()?;
    let filter = request.name_filter.as_deref().unwrap_or_default();
    let mut files = Vec::new();
    for file in all_files(session) {
        if !name_filter_matches(filter, request.regex, &file.get_name())? {
            continue;
        }
        files.push(FileInfo {
            name: file.get_name(),
            load_order: file.get_load_order(),
            masters: (0..file.get_master_count(false))
                .filter_map(|index| file.get_master(index, false))
                .map(|master| master.get_name())
                .collect(),
            record_count: file.get_record_count() as usize,
            is_esm: file.get_is_esm(),
            is_localized: file.get_is_localized(),
        });
    }
    Ok(FileList { files })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsSetHiddenRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// The record and every override of it (`overrides` on the master's
    /// record), as the view's "Unhide all" does; without it the state of
    /// the record version given changes only.
    #[serde(default)]
    pub overrides: bool,
    /// Hide the record (`true`, `Hide`), or show it again (`false`, `Show`).
    #[serde(default = "default_true")]
    pub hidden: bool,
}

/// `serde` default of the `hidden` field.
fn default_true() -> bool {
    true
}

#[derive(Serialize, JsonSchema)]
pub struct RecordsSetHiddenResponse {
    /// Whether the records are hidden now.
    pub hidden: bool,
    /// The records whose state changed, each as `[file] Name`.
    pub changed: Vec<String>,
}

/// Port of `mniNavHiddenClick`/`mniViewHeaderHiddenClick` (`Hide`/`Show`)
/// and of the view's "Unhide all": the hidden state of a record is session
/// state like the `records.compare` alignment, so this changes no plugin
/// data and no file.
fn records_set_hidden(
    session: &mut Session,
    request: RecordsSetHiddenRequest,
) -> Result<RecordsSetHiddenResponse, CommandError> {
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let record = record
        .as_element_impl()
        .and_then(ElementImpl::main_record_impl)
        .ok_or_else(|| CommandError::new("internal", "the record is not a main record of a file"))?;
    let mut records = Vec::new();
    if request.overrides {
        let master = record.master_or_self_impl();
        records.push(master.clone());
        records.extend(master.overrides());
    } else {
        records.push(record);
    }
    let mut changed = Vec::new();
    for record in records {
        let element: ElementRef = record.clone();
        let was_hidden = xedit_core::implementation::sortable::is_hidden(&element);
        if was_hidden == request.hidden {
            continue;
        }
        let base = record.element_base();
        if request.hidden {
            base.hide();
        } else {
            base.show();
        }
        changed.push(format!(
            "[{}] {}",
            record.get_file().map(|file| file.get_name()).unwrap_or_default(),
            record.get_name()
        ));
    }
    Ok(RecordsSetHiddenResponse {
        hidden: request.hidden,
        changed,
    })
}

/// A record in a list.
#[derive(Serialize, JsonSchema)]
pub struct RecordSummary {
    /// Load order FormID as eight hexadecimal digits.
    pub form_id: String,
    pub signature: String,
    pub editor_id: String,
    /// The name xEdit shows for the record.
    pub name: String,
    /// File that holds this version of the record.
    pub file: String,
}

pub(crate) fn summary_of(record: &MainRecordRef) -> RecordSummary {
    RecordSummary {
        form_id: record.get_load_order_form_id().to_string(false),
        signature: record.get_signature().to_string(),
        editor_id: record.get_editor_id(),
        name: record.get_name(),
        file: record.get_file().map(|file| file.get_name()).unwrap_or_default(),
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsListRequest {
    /// Name of the loaded plugin; the only loaded plugin when omitted.
    pub file: Option<String>,
    /// Keep only records with this signature, such as `NPC_`.
    pub signature: Option<String>,
    /// Records to skip.
    #[serde(default)]
    pub offset: usize,
    /// Records to return at most; 100 when omitted.
    pub limit: Option<usize>,
}

#[derive(Serialize, JsonSchema)]
pub struct RecordList {
    pub records: Vec<RecordSummary>,
    /// Records that match before `offset` and `limit` apply.
    pub total: usize,
}

fn records_of(file: &Arc<FileImpl>, signature: Option<&str>) -> Vec<Arc<MainRecordImpl>> {
    file.records()
        .into_iter()
        .filter(|record| signature.is_none_or(|signature| record.get_signature().to_string() == signature))
        .collect()
}

fn records_list(session: &mut Session, request: RecordsListRequest) -> Result<RecordList, CommandError> {
    let file = session.file(request.file.as_deref())?;
    let records = records_of(&file, request.signature.as_deref());
    let total = records.len();
    let records = records
        .into_iter()
        .skip(request.offset)
        .take(request.limit.unwrap_or(100))
        .map(|record| summary_of(&(record as MainRecordRef)))
        .collect();
    Ok(RecordList { records, total })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsFindRequest {
    /// Name of the loaded plugin; the only loaded plugin when omitted.
    pub file: Option<String>,
    /// Keep only records with this signature.
    pub signature: Option<String>,
    /// Text the editor ID contains, compared without case.
    pub editor_id: Option<String>,
    /// Text the name contains, compared without case.
    pub name: Option<String>,
    /// Records to return at most; 100 when omitted.
    pub limit: Option<usize>,
}

fn records_find(session: &mut Session, request: RecordsFindRequest) -> Result<RecordList, CommandError> {
    let file = session.file(request.file.as_deref())?;
    let editor_id = request.editor_id.as_deref().map(str::to_ascii_lowercase);
    let name = request.name.as_deref().map(str::to_ascii_lowercase);
    let matches: Vec<RecordSummary> = records_of(&file, request.signature.as_deref())
        .into_iter()
        .map(|record| summary_of(&(record as MainRecordRef)))
        .filter(|record| {
            editor_id
                .as_deref()
                .is_none_or(|text| record.editor_id.to_ascii_lowercase().contains(text))
                && name
                    .as_deref()
                    .is_none_or(|text| record.name.to_ascii_lowercase().contains(text))
        })
        .collect();
    let total = matches.len();
    let records = matches.into_iter().take(request.limit.unwrap_or(100)).collect();
    Ok(RecordList { records, total })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsGetRequest {
    /// Load order FormID as hexadecimal digits.
    pub form_id: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// Levels of child elements to include; every level when omitted.
    pub depth: Option<usize>,
}

/// An element of the tree.
#[derive(Serialize, JsonSchema)]
pub struct ElementNode {
    pub name: String,
    /// Name as xEdit displays it, when it differs from `name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Value as xEdit displays it; empty for a container.
    pub value: String,
    /// Summary of a container, as the `[S]:` text of a dump.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Native value: number, boolean, string or bytes as hexadecimal.
    #[serde(skip_serializing_if = "Value::is_null")]
    pub native: Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ElementNode>,
}

fn native_json(variant: Variant) -> Value {
    match variant {
        Variant::Empty => Value::Null,
        Variant::Bool(value) => Value::from(value),
        Variant::Int(value) => Value::from(value),
        Variant::UInt(value) => Value::from(value),
        Variant::Float(value) => Value::from(value),
        Variant::Str(value) => Value::from(value),
        Variant::Bytes(bytes) => Value::from(
            bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    }
}

pub(crate) fn node_of(element: &ElementRef, depth: Option<usize>) -> ElementNode {
    let name = element.get_name();
    let display_name = element.get_display_name(true);
    let value = element.get_value();
    let summary = if value.is_empty() {
        element.get_summary()
    } else {
        String::new()
    };
    // `wbFlagsAsArray`: the flags of a flags value are elements of their
    // own in the GUI's tree and in the view (`ValueDoInit`).
    let flags = xedit_core::implementation::flag::flags_as_array_of(element);
    let children = match (element.as_container(), depth, flags) {
        (Some(_), Some(0), _) => Vec::new(),
        (Some(_), depth, Some(flags)) => flags
            .iter()
            .map(|flag| node_of(flag, depth.map(|depth| depth - 1)))
            .collect(),
        (Some(container), depth, None) => (0..container.get_element_count())
            .filter_map(|index| container.get_element(index))
            .map(|child| node_of(&child, depth.map(|depth| depth - 1)))
            .collect(),
        (None, _, _) => Vec::new(),
    };
    ElementNode {
        display_name: (display_name != name).then_some(display_name),
        name,
        value,
        summary,
        native: native_json(element.get_native_value()),
        children,
    }
}

#[derive(Serialize, JsonSchema)]
pub struct RecordDetail {
    #[serde(flatten)]
    pub summary: RecordSummary,
    /// Elements of the record, starting with the record header.
    pub elements: Vec<ElementNode>,
}

fn records_get(session: &mut Session, request: RecordsGetRequest) -> Result<RecordDetail, CommandError> {
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let elements = (0..record.get_element_count())
        .filter_map(|index| record.get_element(index))
        .map(|element| node_of(&element, request.depth))
        .collect();
    Ok(RecordDetail {
        summary: summary_of(&record),
        elements,
    })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsGetRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Path of the element inside the record, with `\` between the names,
    /// such as `DATA\Health` or `ACBS\Flags\Female`.
    pub path: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// Levels of child elements to include; every level when omitted.
    pub depth: Option<usize>,
}

#[derive(Serialize, JsonSchema)]
pub struct ElementDetail {
    /// Full path of the element, in the upstream `FullPath` shape
    /// (`wbImplementation.pas:19641-19652`).
    pub path: String,
    #[serde(flatten)]
    pub node: ElementNode,
}

fn elements_get(session: &mut Session, request: ElementsGetRequest) -> Result<ElementDetail, CommandError> {
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let element = record.get_element_by_path(&request.path).ok_or_else(|| {
        CommandError::new(
            "unknown_element",
            format!("{} has no element at {}", record.get_name(), request.path),
        )
    })?;
    Ok(ElementDetail {
        path: element.get_full_path(),
        node: node_of(&element, request.depth),
    })
}

/// `elements.set`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsSetRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Path of the element inside the record, with `\\` between the names.
    /// A missing last element is added when the record's definition has it
    /// (`ElementEditValues`), and a flag of a flags value is set by its name.
    pub path: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// The value: a string is set as the edit value (the text xEdit shows
    /// in its editor, such as `1.5`, `Dawnguard "Dawnguard" [QUST:0200C97A]`
    /// or `0000000000000001` for flags), a number or a boolean as the native
    /// value, and `null` sets the element to its default.
    pub value: Value,
    /// Report what would change, but change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `elements.set`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ElementsSetResponse {
    /// Full path of the element, in the upstream `FullPath` shape
    /// (`wbImplementation.pas:19641-19652`).
    pub path: String,
    /// The element before the change.
    pub old: ElementNode,
    /// The element after the change; the same as `old` for a dry run.
    pub new: ElementNode,
    /// Whether the value differs after the change.
    pub changed: bool,
    /// Whether the element was added to the record by this call.
    pub added: bool,
    /// The plugin that now holds a change to save.
    pub file: String,
}

fn elements_set(session: &mut Session, request: ElementsSetRequest) -> Result<ElementsSetResponse, CommandError> {
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let existing = record.get_element_by_path(&request.path);
    // The translate mode shows (and so edits) only the translatable
    // elements: the others are ignored (`GetConflictPriority`) and hidden
    // from the view (`InitConflictStatus`), and no member can be added.
    if xedit_core::interface::globals::translation_mode()
        && existing.as_ref().is_none_or(|element| {
            element.get_conflict_priority() == xedit_core::interface::types::ConflictPriority::cpIgnore
        })
    {
        return Err(CommandError::new(
            "translate_mode",
            format!(
                "{} \\ {} is not translatable, which the translate mode does not edit",
                record.get_name(),
                request.path
            ),
        ));
    }
    let old = match &existing {
        Some(element) => node_of(element, Some(0)),
        None => {
            // The element may be added; the request must name a member of
            // the record for that.
            if request.value.is_null() {
                return Err(CommandError::new(
                    "unknown_element",
                    format!("{} has no element at {}", record.get_name(), request.path),
                ));
            }
            ElementNode {
                name: String::new(),
                display_name: None,
                value: String::new(),
                summary: String::new(),
                native: Value::Null,
                children: Vec::new(),
            }
        }
    };
    let file = record.get_file().map(|file| file.get_name()).unwrap_or_default();
    if request.dry_run {
        let element = existing.ok_or_else(|| {
            CommandError::new(
                "unknown_element",
                format!("{} has no element at {}", record.get_name(), request.path),
            )
        })?;
        if !element.get_is_editable() && !request.value.is_null() {
            return Err(CommandError::new(
                "not_editable",
                format!("{} can not be edited", element.get_full_path()),
            ));
        }
        return Ok(ElementsSetResponse {
            path: element.get_full_path(),
            new: node_of(&element, Some(0)),
            old,
            changed: false,
            added: false,
            file,
        });
    }
    let edit = |error: String| CommandError::new("edit_failed", error);
    match (&existing, &request.value) {
        (Some(element), Value::Null) => element.set_to_default().map_err(edit)?,
        (Some(element), Value::String(text)) => element.set_edit_value(text).map_err(edit)?,
        (Some(element), value) => element.set_native_value(native_of(value)?).map_err(edit)?,
        (None, Value::String(text)) => record.set_element_edit_value(&request.path, text).map_err(edit)?,
        (None, value) => record
            .set_element_native_value(&request.path, native_of(value)?)
            .map_err(edit)?,
    }
    let element = record.get_element_by_path(&request.path).ok_or_else(|| {
        CommandError::new(
            "unknown_element",
            format!("{} has no element at {}", record.get_name(), request.path),
        )
    })?;
    let new = node_of(&element, Some(0));
    Ok(ElementsSetResponse {
        path: element.get_full_path(),
        changed: new.value != old.value || new.native != old.native,
        added: existing.is_none(),
        old,
        new,
        file,
    })
}

/// A JSON value as the native value of an element.
fn native_of(value: &Value) -> Result<Variant, CommandError> {
    match value {
        Value::Bool(value) => Ok(Variant::Bool(*value)),
        Value::Number(number) => {
            if let Some(value) = number.as_u64() {
                Ok(Variant::UInt(value))
            } else if let Some(value) = number.as_i64() {
                Ok(Variant::Int(value))
            } else {
                Ok(Variant::Float(number.as_f64().unwrap_or(0.0)))
            }
        }
        Value::String(text) => Ok(Variant::Str(text.clone())),
        Value::Null => Ok(Variant::Empty),
        _ => Err(CommandError::new(
            "invalid_params",
            "value must be a string, a number, a boolean or null",
        )),
    }
}

/// Adds the inspection commands to the registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "session.info",
        "Report the game and the loaded plugins.",
        false,
        session_info,
    );
    registry.register(
        "session.load",
        "Load another plugin list into the running session (or the same one with a tool mode's load settings, as xeInit.pas applies them): the way a serve or MCP session runs the quick clean modes. Discards unsaved changes unless force is set; needs --edit unless dry_run.",
        true,
        session_load,
    );
    registry.register(
        "files.list",
        "List the loaded files with their masters.",
        false,
        files_list,
    );
    registry.register("records.list", "List the records of a plugin.", false, records_list);
    registry.register(
        "records.find",
        "Find records by editor ID or name.",
        false,
        records_find,
    );
    registry.register("records.get", "Read a record with its elements.", false, records_get);
    registry.register(
        "elements.get",
        "Read an element of a record by path.",
        false,
        elements_get,
    );
    registry.register(
        "records.set_hidden",
        "Hide a record or show it again, as the navigation tree's Hide and the view's Unhide all do (element state esHidden); comparisons, the filter and the record lists leave hidden records out.",
        false,
        records_set_hidden,
    );
    registry.register(
        "elements.set",
        "Set the value of an element of a record by path, adding a missing member.",
        true,
        elements_set,
    );
}
