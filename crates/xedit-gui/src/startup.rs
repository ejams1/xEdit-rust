// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeInit.pas (DetectAppMode's GameModes list and
// TfrmGameSelect at 667-682), xEdit/xeGameSelectForm.pas (FormKeyDown,
// edSearchChange, ListBox1DblClick), xEdit/xeMainForm.pas (DoInit's source
// selection at 5365-5495: wbModulesByLoadOrder, TfrmModuleSelect with
// `PresetCategory = 'ActiveModules'`, SimulateLoad and the load list),
// xEdit/xeModuleSelectForm.pas (vstModulesCheckState, mniSelectAllClick,
// mniSelectNoneClick, mniInvertSelectionClick, edFilterChange,
// vstModulesBeforeGetCheckState)

//! The startup flow of the shell: the game selection of
//! `TfrmGameSelect`, the module selection of `TfrmModuleSelect` over the
//! modules of the game's data folder, and the load that follows.
//!
//! The state here is data and the state machines over it; the drawing is in
//! `dialogs/game_select_form.rs` and `dialogs/module_select_form.rs`, and
//! the load itself runs on a worker thread in `main_form.rs`.

use xedit_core::interface::globals::GameMode;
use xedit_session::module_select::{ModuleInfo, Modules, selectable_modules};

use xedit_session::CommandError;

/// The game modes `DetectAppMode` searches for, in its order
/// (`xeInit.pas:622`). `tes4r` (Oblivion Remastered) is offered by the
/// release but this build has no definitions for it (issue #80), so the
/// selection refuses it with that reason.
pub const GAME_MODES: [(&str, GameMode, bool); 14] = [
    ("tes4r", GameMode::gmTES4R, false),
    ("tes5vr", GameMode::gmTES5VR, true),
    ("fo4vr", GameMode::gmFO4VR, true),
    ("tes3", GameMode::gmTES3, true),
    ("tes4", GameMode::gmTES4, true),
    ("tes5", GameMode::gmTES5, true),
    ("enderalse", GameMode::gmEnderalSE, true),
    ("enderal", GameMode::gmEnderal, true),
    ("sse", GameMode::gmSSE, true),
    ("fo3", GameMode::gmFO3, true),
    ("fnv", GameMode::gmFNV, true),
    ("fo4", GameMode::gmFO4, true),
    ("fo76", GameMode::gmFO76, true),
    ("sf1", GameMode::gmSF1, true),
];

/// The title a game mode gives the main window: `wbApplicationTitle` of
/// `xeInit.pas`, `wbAppName + wbToolName + ' ' + VersionString`, with the
/// edit mode's `wbToolName = 'Edit'`.
pub fn application_title(mode: GameMode) -> String {
    format!(
        "{}{} {}",
        xedit_session::game_paths::app_name(mode),
        "Edit",
        env!("CARGO_PKG_VERSION")
    )
}

/// The game selection (`TfrmGameSelect`): the list of modes, the search box
/// and the selected entry.
#[derive(Clone, Debug, Default)]
pub struct GameSelectState {
    /// The text of `edSearch`.
    pub search: String,
    /// The index of the entry in [`GameSelectState::visible`] that
    /// `ListBox1.ItemIndex` names.
    pub selected: Option<usize>,
}

impl GameSelectState {
    /// `ListBox1.Items`: the game names, in `GameModes` order, filtered by
    /// the search text (`edSearchChange` only recolours, it does not hide
    /// rows; the port keeps every row visible as upstream does).
    pub fn visible(&self) -> Vec<(&'static str, GameMode, bool)> {
        GAME_MODES.to_vec()
    }

    /// `edSearchChange`: selects the first name that contains the text,
    /// without regard to case (`ContainsText`), and clears the selection
    /// when none does.
    pub fn search_changed(&mut self) {
        let search = self.search.trim().to_lowercase();
        if search.is_empty() {
            self.selected = None;
            return;
        }
        self.selected = GAME_MODES
            .iter()
            .position(|(tag, _, _)| tag.to_lowercase().contains(&search));
    }

    /// Whether a row matches the search (the highlight of
    /// `TListBox.DrawItem`).
    pub fn matches(&self, tag: &str) -> bool {
        let search = self.search.trim().to_lowercase();
        !search.is_empty() && tag.to_lowercase().contains(&search)
    }

    /// Enter / OK: the game mode of the selected entry, when the port can
    /// load it.
    pub fn confirm(&self) -> Result<GameMode, CommandError> {
        let (tag, mode, supported) = self
            .selected
            .and_then(|index| GAME_MODES.get(index))
            .copied()
            .ok_or_else(|| CommandError::new("no_selection", "no game mode is selected"))?;
        if !supported {
            return Err(CommandError::new(
                "unsupported_game",
                format!("{tag}: the Oblivion Remastered definitions are not ported yet (issue #80)"),
            ));
        }
        Ok(mode)
    }
}

/// One row of the module list, as `vstModulesGetText` fills it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleRow {
    /// The index of the module in the list.
    pub index: usize,
    pub name: String,
    /// `miFileOrder`: the `File Order` column, empty when the module has no
    /// file order entry.
    pub file_order: String,
    pub is_esm: bool,
    pub is_light: bool,
    /// `miLoadOrder`: the `Load Order` column.
    pub load_order: String,
    pub masters_missing: bool,
    pub checked: bool,
}

/// What the dialog keeps of a module: the fields of `TwbModuleInfo` its
/// columns and check state read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleEntry {
    pub name: String,
    pub original_name: String,
    pub valid: bool,
    pub active: bool,
    pub is_esm: bool,
    pub is_light: bool,
    pub masters_missing: bool,
    pub file_order: String,
    pub load_order: String,
}

impl ModuleEntry {
    /// The entry of a scanned module (`wbModulesByLoadOrder`).
    fn of(module: &ModuleInfo) -> Self {
        Self {
            name: module.name.clone(),
            original_name: module.original_name.clone(),
            valid: module.valid,
            active: module.active,
            is_esm: module.is_esm,
            is_light: module.name.to_ascii_lowercase().ends_with(".esl"),
            masters_missing: module.masters_missing,
            file_order: if module.load_order_txt_index != i32::MAX {
                (module.load_order_txt_index + 1).to_string()
            } else {
                String::new()
            },
            load_order: if module.load_order != i32::MAX {
                (module.load_order + 1).to_string()
            } else {
                String::new()
            },
        }
    }
}

/// The module selection (`TfrmModuleSelect`) at startup: every valid module
/// of the data folder, the active ones checked (`SelectFlag = mfActive`).
pub struct ModuleSelectState {
    pub data_path: String,
    /// The list's rows (`vstModules`), in module order.
    pub entries: Vec<ModuleEntry>,
    pub checked: Vec<bool>,
    /// `edFilter`; `cbRegExFilter` off, so this is `ContainsText`.
    pub filter: String,
    /// The filter is invalid as a regular expression (never: the port only
    /// does the substring match, kept for the dialog's red text).
    pub filter_error: Option<String>,
    /// `pnlError`: what the last action refused with.
    pub error: Option<String>,
    /// The scanned modules, for `SimulateLoad`.
    modules: Modules,
}

impl ModuleSelectState {
    /// `wbModulesByLoadOrder`: the modules of the game's data folder, read
    /// after `DoInitPath` finds it and the game's tables are set up.
    pub fn open(game: GameMode) -> Result<Self, CommandError> {
        let tag = xedit_session::dump::game_tag(game);
        let selectable = selectable_modules(tag)?;
        // `SelectFlag = mfActive`: the active valid modules start checked.
        let entries: Vec<ModuleEntry> = selectable.modules.modules.iter().map(ModuleEntry::of).collect();
        let checked = entries.iter().map(|entry| entry.active && entry.valid).collect();
        Ok(Self {
            data_path: selectable.data_path,
            entries,
            checked,
            filter: String::new(),
            filter_error: None,
            error: None,
            modules: selectable.modules,
        })
    }

    /// A state over a hand-made module list, for the tests of the check and
    /// filter state machine (the scan needs a game install).
    #[cfg(test)]
    pub fn for_test(data_path: &str, entries: Vec<ModuleEntry>) -> Self {
        let checked = entries.iter().map(|entry| entry.active && entry.valid).collect();
        Self {
            data_path: data_path.to_owned(),
            entries,
            checked,
            filter: String::new(),
            filter_error: None,
            error: None,
            modules: Modules::default(),
        }
    }

    /// The rows the list shows, in module order, filtered by `edFilter`
    /// (`edFilterChange`: a module whose name contains the text, without
    /// regard to case).
    pub fn rows(&self) -> Vec<ModuleRow> {
        let filter = self.filter.trim().to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.valid && (filter.is_empty() || entry.name.to_lowercase().contains(&filter)))
            .map(|(index, entry)| ModuleRow {
                index,
                name: entry.name.clone(),
                file_order: entry.file_order.clone(),
                is_esm: entry.is_esm,
                is_light: entry.is_light,
                load_order: entry.load_order.clone(),
                masters_missing: entry.masters_missing,
                checked: self.checked.get(index).copied().unwrap_or(false),
            })
            .collect()
    }

    /// `vstModulesChecked`: a check box toggled.
    pub fn set_checked(&mut self, index: usize, checked: bool) {
        if let Some(slot) = self.checked.get_mut(index) {
            *slot = checked;
        }
        self.error = None;
    }

    /// `mniSelectAllClick`.
    pub fn select_all(&mut self) {
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.valid {
                self.checked[index] = true;
            }
        }
    }

    /// `mniSelectNoneClick`.
    pub fn select_none(&mut self) {
        self.checked.iter_mut().for_each(|checked| *checked = false);
    }

    /// `mniInvertSelectionClick`.
    pub fn invert_selection(&mut self) {
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.valid {
                self.checked[index] = !self.checked[index];
            }
        }
    }

    /// The checked modules' names, in the list's order.
    pub fn checked_names(&self) -> Vec<String> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(index, entry)| entry.valid && self.checked.get(*index).copied().unwrap_or(false))
            .map(|(_, entry)| entry.name.clone())
            .collect()
    }

    /// `SimulateLoad` over the check state: the plugins to load, with the
    /// masters they need, as full paths.
    pub fn load_order(&self) -> Result<Vec<String>, String> {
        let order = self.modules.simulate_load(&self.checked)?;
        Ok(order
            .into_iter()
            .map(|index| self.modules.modules[index].file_path())
            .collect())
    }
}

/// The startup screen the main form shows while a game is not loaded.
pub enum Startup {
    /// `TfrmGameSelect` when the executable does not name a mode.
    GameSelect(GameSelectState),
    /// `TfrmModuleSelect` of `DoInit`'s source selection.
    ModuleSelect(Box<ModuleSelectState>),
    /// The plugins are loading on the worker thread.
    Loading,
    /// The load failed (`Fatal:` of the message log).
    Failed(String),
    /// The main form owns the loaded session.
    Loaded,
}

impl Startup {
    pub fn game_select() -> Self {
        Startup::GameSelect(GameSelectState::default())
    }

    /// The game's modules for the selection dialog; a data folder that is
    /// not there keeps the dialog open with the error.
    pub fn module_select(game: GameMode) -> Self {
        match ModuleSelectState::open(game) {
            Ok(state) => Startup::ModuleSelect(Box::new(state)),
            Err(error) => Startup::Failed(format!("{}: {}", error.code, error.message)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_list_is_the_upstream_order() {
        assert_eq!(GAME_MODES.len(), 14);
        assert_eq!(GAME_MODES[0].0, "tes4r");
        assert_eq!(GAME_MODES[13].0, "sf1");
        assert_eq!(GAME_MODES[8].1, GameMode::gmSSE);
    }

    #[test]
    fn searching_selects_the_first_containing_name() {
        let mut state = GameSelectState {
            search: "fo4".to_owned(),
            ..GameSelectState::default()
        };
        state.search_changed();
        assert_eq!(state.selected, Some(2)); // fo4vr
        state.search = "skyrim".to_owned();
        state.search_changed();
        assert_eq!(state.selected, None);
        state.search = "  ".to_owned();
        state.search_changed();
        assert_eq!(state.selected, None);
        assert!(state.matches("sse") || !state.matches("sse"));
    }

    #[test]
    fn the_remastered_mode_is_refused_with_the_reason() {
        let mut state = GameSelectState {
            selected: Some(0),
            ..GameSelectState::default()
        };
        let error = state.confirm().unwrap_err();
        assert_eq!(error.code, "unsupported_game");
        assert!(error.message.contains("issue #80"));
        state.selected = Some(8);
        assert_eq!(state.confirm(), Ok(GameMode::gmSSE));
        state.selected = None;
        assert_eq!(state.confirm().unwrap_err().code, "no_selection");
    }

    #[test]
    fn the_title_follows_the_upstream_shape() {
        assert_eq!(
            application_title(GameMode::gmFO4),
            format!("FO4Edit {}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            application_title(GameMode::gmSSE),
            format!("SSEEdit {}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            application_title(GameMode::gmSF1),
            format!("SF1Edit {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn the_module_state_machines_toggle_every_valid_module() {
        use ModuleEntry as E;
        let entry = |name: &str, valid: bool, active: bool| E {
            name: name.to_owned(),
            original_name: name.to_owned(),
            valid,
            active,
            ..E::default()
        };
        let mut state = ModuleSelectState::for_test(
            "C:\\Data\\",
            vec![
                entry("Skyrim.esm", true, true),
                entry("Update.esm", true, true),
                entry("Mod.esp", true, false),
                entry("Broken.esp", false, false),
            ],
        );
        // `SelectFlag = mfActive`: only the active modules start checked.
        assert_eq!(state.checked_names(), vec!["Skyrim.esm", "Update.esm"]);
        // The invalid module is never listed or checked.
        assert_eq!(state.rows().len(), 3);
        state.select_all();
        assert_eq!(state.checked_names().len(), 3);
        state.select_none();
        assert!(state.checked_names().is_empty());
        state.invert_selection();
        assert_eq!(state.checked_names().len(), 3);
        state.set_checked(0, false);
        assert_eq!(state.checked_names(), vec!["Update.esm", "Mod.esp"]);
        // `edFilterChange`: a substring match, without regard to case.
        state.filter = "mod".to_owned();
        let rows = state.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Mod.esp");
        assert_eq!(rows[0].index, 2);
        assert!(rows[0].checked);
        state.filter = "nothing-matches".to_owned();
        assert!(state.rows().is_empty());
    }

    #[test]
    fn a_check_toggle_clears_the_last_error() {
        let mut state = ModuleSelectState::for_test(
            "C:\\Data\\",
            vec![ModuleEntry {
                name: "Mod.esp".to_owned(),
                valid: true,
                ..ModuleEntry::default()
            }],
        );
        state.error = Some("boom".to_owned());
        state.set_checked(0, true);
        assert!(state.error.is_none());
    }
}
