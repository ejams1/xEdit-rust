// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeInit.pas (DetectAppMode's GameModes list and
// TfrmGameSelect at 667-682), xEdit/xeGameSelectForm.pas (FormKeyDown,
// edSearchChange, ListBox1DblClick), xEdit/xeMainForm.pas (DoInit's source
// selection at 5365-5495: wbModulesByLoadOrder, TfrmModuleSelect with
// `PresetCategory = 'ActiveModules'`, SimulateLoad and the load list),
// xEdit/xeModuleSelectForm.pas (FormCreate, FormShow, vstModulesInitNode,
// vstModulesInitChildren, vstModulesGetText, vstModulesCheckState,
// vstModulesCompareNodes, vstModulesHeaderClick, GetSelectedOrAll,
// mniSelectAllClick, mniSelectNoneClick, mniInvertSelectionClick,
// edFilterChange, DoSingleModuleLoad, SimulateLoad, PresetLoad, PresetSave,
// PresetDelete, PresetsUpdate, GetPresetsSection, IsPresetDifferent) and
// Core/wbLoadOrder.pas (LoadOrderDescription, TwbModuleInfos.SimulateLoad,
// TwbFileID.Create* and ToString)

//! The startup flow of the shell: the game selection of
//! `TfrmGameSelect`, the module selection of `TfrmModuleSelect` over the
//! modules of the game's data folder, and the load that follows.
//!
//! The state here is data and the state machines over it; the drawing is in
//! `dialogs/game_select_form.rs` and `dialogs/module_select_form.rs`, and
//! the load itself runs on a worker thread in `main_form.rs`.
//!
//! The module list comes from `modules.list` (`wbModulesByLoadOrder`); the
//! dialog opened before a session exists is served by a session over no
//! files (`Session::with_files`), which the command layer accepts. A game
//! whose data folder can be scanned without a session is still opened
//! through `session::module_select::selectable_modules`, which is the
//! helper the command's data source reads as well.

use eframe::egui;
use xedit_core::interface::globals::{
    GameMode, always_load_game_master, ignore_light, ignore_medium, ignore_update, is_light_supported,
    is_medium_supported, pseudo_light, pseudo_medium, pseudo_update,
};
use xedit_session::CommandError;
use xedit_session::module_select::{
    ModuleEntry as ListEntry, ModulesListResponse, SelectableModules, selectable_modules,
};

use crate::dialogs::selection::{CheckState, DialogPlacement, NameFilter, PresetCombo};
use crate::widgets::virtual_tree::TreeState;

/// The columns of `vstModules` in `xeModuleSelectForm.dfm`: Filename 300,
/// File Order 200, ESM 50, Light 50, Load Order 75, FormID Prefix 90,
/// Update 70, Med 50.
pub const MODULE_COLUMNS: [(&str, f32); 8] = [
    ("Filename", 300.0),
    ("File Order", 200.0),
    ("ESM", 50.0),
    ("Light", 50.0),
    ("Load Order", 75.0),
    ("FormID Prefix", 90.0),
    ("Update", 70.0),
    ("Med", 50.0),
];

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
    /// `NoEscape`: the dialog of a startup step that can not be cancelled.
    pub no_escape: bool,
    pub placement: Option<DialogPlacement>,
    /// The rectangle of the drawn window, for the placement write.
    pub rect: Option<egui::Rect>,
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
        self.selected = crate::dialogs::selection::first_match(
            &GAME_MODES
                .iter()
                .map(|(tag, _, _)| (*tag).to_owned())
                .collect::<Vec<_>>(),
            &self.search,
        );
    }

    /// Whether a row matches the search (the highlight of
    /// `TListBox.DrawItem`).
    pub fn matches(&self, tag: &str) -> bool {
        crate::dialogs::selection::row_matches(tag, &self.search)
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

/// One module of the dialog's tree: the fields of `TwbModuleInfo` its
/// columns, check state and simulation read.
#[derive(Clone, Debug, PartialEq)]
pub struct ModuleNode {
    /// `miIndex`: the position in `wbModulesByLoadOrder` (the command's
    /// `index`), which ties equal sorts.
    pub index: usize,
    /// The position of the node in [`ModuleSelectState::modules`].
    pub node: usize,
    pub name: String,
    pub original_name: String,
    pub path: String,
    /// `miLoadOrder` of the loaded module, `None` when it is not loaded.
    pub load_order: Option<i32>,
    /// `miFileID` of the loaded module.
    pub file_id: Option<String>,
    /// The simulated `miLoadOrder` (`SimulateLoad`), `None` before a run.
    pub simulated_load_order: Option<i32>,
    /// The simulated `miFileID` (`SimulateLoad`), `XX` when invalid.
    pub simulated_file_id: String,
    pub has_esm_flag: bool,
    pub is_esm: bool,
    pub is_light: bool,
    pub is_medium: bool,
    pub is_update: bool,
    pub is_ghost: bool,
    pub is_hardcoded: bool,
    pub valid: bool,
    pub loaded: bool,
    pub masters_missing: bool,
    pub active: bool,
    pub active_in_plugins_txt: bool,
    pub official_index: Option<i32>,
    pub cc_index: Option<i32>,
    pub plugins_txt_index: Option<i32>,
    pub load_order_txt_index: Option<i32>,
    pub date_time: f64,
    pub has_blueprint: bool,
    /// `miMasters`: the node of each master in [`ModuleSelectState::modules`],
    /// `None` for a master that is not in the list.
    pub masters: Vec<Option<usize>>,
    pub master_names: Vec<String>,
    /// `SelectFlag in miFlags`.
    pub checked: bool,
    /// The node is disabled: not `FilterFlag`, masters missing, or the tag
    /// of the mod group editor's Insert list.
    pub disabled: bool,
    /// `HideFlag in miFlags`.
    pub hidden: bool,
    /// `ivsHasChildren`.
    pub expanded: bool,
}

impl ModuleNode {
    /// `mfIsGameMaster`: `miOfficialIndex = Low(Integer)`.
    pub fn is_game_master(&self) -> bool {
        self.official_index == Some(i32::MIN)
    }

    /// `CheckStateForModule` for a root child of the tree.
    pub fn check_state(&self, mix_loaded: bool) -> CheckState {
        if self.checked {
            CheckState::Checked
        } else if mix_loaded && self.loaded {
            CheckState::Mixed
        } else {
            CheckState::Unchecked
        }
    }

    /// `LoadOrderDescription` (`wbLoadOrder.pas`).
    pub fn load_order_description(&self) -> String {
        let mut text = String::new();
        if self.has_blueprint && xedit_core::interface::globals::game_mode() == GameMode::gmSF1 {
            text.push_str("[BP]");
        }
        // `_UpdateIndex` is -1 for the Skyrim family; Enderal Special
        // Edition loads its Update.esm last (`High(Integer) - 1`).
        match self.official_index {
            Some(i32::MIN) => text.push_str("[GameMaster]"),
            Some(index) if index == i32::MIN + 1 => text.push_str("[Hardcoded]"),
            Some(index) if index == -1 || index == i32::MAX - 1 => text.push_str("[Update]"),
            Some(index) if index >= 0 => text.push_str(&format!("[DLC:{index}]")),
            _ => {}
        }
        if let Some(index) = self.cc_index {
            text.push_str(&format!("[CC:{index}]"));
        }
        if text.is_empty() {
            if self.is_esm && xedit_core::interface::globals::game_mode() != GameMode::gmTES4R {
                text.push_str("[ESM]");
            }
            if let Some(index) = self.plugins_txt_index {
                text.push_str(&format!("[Plugins.txt:{index}]"));
            }
            if let Some(index) = self.load_order_txt_index {
                text.push_str(&format!("[LoadOrder.txt:{index}]"));
            }
            if text.is_empty() || text == "[ESM]" {
                text.push_str(&format!(
                    "[Time:{}]",
                    xedit_session::script_rtl::datetime::date_time_to_str("yyyy-mm-dd hh:mm:ss", self.date_time)
                ));
            }
        }
        text
    }
}

/// One visible row of `vstModules`: a module or one of its masters.
#[derive(Clone, Debug, PartialEq)]
pub struct ModuleRow {
    /// The row's identity for the tree.
    pub row: usize,
    /// The module the row shows (the master for a master row).
    pub node: usize,
    pub master: bool,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub check: CheckState,
    pub disabled: bool,
    pub name: String,
    pub file_order: String,
    pub esm: String,
    pub light: String,
    pub load_order: String,
    pub file_id: String,
    pub update: String,
    pub medium: String,
    pub ghost: bool,
    pub masters_missing: bool,
}

/// How the module dialog was opened (`TfrmModuleSelect`'s public flags).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleSelectMode {
    /// `DoInit`'s source selection: `SelectFlag = mfActive`,
    /// `FilterFlag = mfValid`, `PresetCategory = 'ActiveModules'`.
    Active,
    /// The mod group editor's Insert: `SelectFlag = mfTagged`,
    /// `FilterFlag = mfEphemeralModGroupTagged`, no presets,
    /// `MinSelect = MaxSelect = 1`, and the modules already in the group
    /// are tagged out (`TagContainedModules(True)`).
    Insert,
}

/// The module selection (`TfrmModuleSelect`): every module of the data
/// folder, the check state, the filter, the presets and the simulated load.
pub struct ModuleSelectState {
    pub data_path: String,
    /// The modules in `wbModulesByLoadOrder` order.
    pub modules: Vec<ModuleNode>,
    /// The flattened rows of the tree, in display order.
    pub rows: Vec<ModuleRow>,
    /// `edFilter` with `cbRegExFilter`.
    pub filter: NameFilter,
    /// `cbPreset` (`PresetsUpdate`).
    pub preset: PresetCombo,
    /// `LoadedPreset`.
    pub loaded_preset: String,
    /// `PresetCategory <> ''`: the presets are offered.
    pub presets_available: bool,
    /// `pnlError`'s text without the `Error: ` prefix.
    pub error: Option<String>,
    pub min_select: usize,
    pub max_select: usize,
    /// `SelectFlag = mfActive`, which also makes the `mfLoaded` rows mixed.
    pub mix_loaded: bool,
    /// `Header.SortColumn` and `Header.SortDirection`.
    pub sort_column: usize,
    pub sort_descending: bool,
    pub mode: ModuleSelectMode,
    /// The question the preset buttons asked, waiting for its answer.
    pub question: Option<ModuleSelectQuestion>,
    pub placement: Option<DialogPlacement>,
    /// The rectangle of the drawn window, for the placement write.
    pub rect: Option<egui::Rect>,
    /// `vstModules`: the selection and focus of the rows.
    pub tree: TreeState,
}

/// A `MessageDlg` the preset buttons of the module dialog ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleSelectQuestion {
    /// `PresetDelete`: "Are you sure you want to delete preset ...?"
    DeletePreset(String),
    /// `PresetSave` of a preset that exists: "Are you sure you want to
    /// overwrite existing preset ...?"
    OverwritePreset(String),
    /// `PresetSave(..., True)` after OK: the changed-selection question.
    ChangedPreset(String),
}

impl ModuleSelectState {
    /// `wbModulesByLoadOrder`: the modules of the game's data folder, read
    /// after `DoInitPath` finds it and the game's tables are set up. This
    /// primes the game globals for `modules.list`, which the shell then
    /// reads; it is also the fallback when the command can not run.
    pub fn open(game: GameMode, mode: ModuleSelectMode, contained: &[String]) -> Result<Self, CommandError> {
        let tag = xedit_session::dump::game_tag(game);
        let selectable = selectable_modules(tag)?;
        Ok(Self::from_selectable(selectable, mode, contained))
    }

    /// The state over the modules of `selectable_modules` (no session
    /// exists yet; the same scan the command performs).
    pub fn from_selectable(selectable: SelectableModules, mode: ModuleSelectMode, contained: &[String]) -> Self {
        let nodes: Vec<ModuleNode> = selectable
            .modules
            .modules
            .iter()
            .enumerate()
            .map(|(index, module)| {
                let masters = module
                    .master_names
                    .iter()
                    .map(|name| selectable.modules.by_name(name))
                    .collect();
                ModuleNode {
                    index,
                    node: index,
                    name: module.name.clone(),
                    original_name: module.original_name.clone(),
                    path: module.file_path(),
                    load_order: (module.load_order != i32::MAX).then_some(module.load_order),
                    file_id: module.file.as_ref().map(|file| {
                        use xedit_core::interface::File as _;
                        file.get_load_order_file_id().to_string()
                    }),
                    simulated_load_order: None,
                    simulated_file_id: "XX".to_owned(),
                    has_esm_flag: module.has_esm_flag,
                    is_esm: module.is_esm,
                    is_light: module.is_light,
                    is_medium: module.is_medium,
                    is_update: module.is_update,
                    is_ghost: module.is_ghost,
                    is_hardcoded: module.is_hardcoded,
                    valid: module.valid,
                    loaded: module.has_file(),
                    masters_missing: module.masters_missing,
                    active: module.active,
                    active_in_plugins_txt: module.active_in_plugins_txt,
                    official_index: (module.official_index != i32::MAX).then_some(module.official_index),
                    cc_index: (module.cc_index != i32::MAX).then_some(module.cc_index),
                    plugins_txt_index: (module.plugins_txt_index != i32::MAX).then_some(module.plugins_txt_index),
                    load_order_txt_index: (module.load_order_txt_index != i32::MAX)
                        .then_some(module.load_order_txt_index),
                    date_time: module.date_time,
                    has_blueprint: module.has_blueprint,
                    masters,
                    master_names: module.master_names.clone(),
                    checked: false,
                    disabled: false,
                    hidden: false,
                    expanded: false,
                }
            })
            .collect();
        Self::from_nodes(selectable.data_path, nodes, mode, contained)
    }

    /// The state over the response of `modules.list`.
    pub fn from_modules_list(response: ModulesListResponse, mode: ModuleSelectMode, contained: &[String]) -> Self {
        // The command's indices are of the whole list; a master is looked
        // up by name among the listed modules.
        let nodes: Vec<ModuleNode> = response
            .modules
            .iter()
            .enumerate()
            .map(|(position, module)| Self::node_from_entry(position, module))
            .collect();
        Self::from_nodes(response.data_path, nodes, mode, contained)
    }

    fn node_from_entry(node: usize, module: &ListEntry) -> ModuleNode {
        // `masters` is resolved against the whole list once it is built; a
        // name that is not there stays `None` (a master the list does not
        // have, as `miMasters[i]` is unassigned upstream).
        ModuleNode {
            index: module.index,
            node,
            name: module.name.clone(),
            original_name: module.original_name.clone(),
            path: module.path.clone(),
            load_order: module.load_order,
            file_id: module.file_id.clone(),
            simulated_load_order: None,
            simulated_file_id: "XX".to_owned(),
            has_esm_flag: module.has_esm_flag,
            is_esm: module.is_esm,
            is_light: module.is_light,
            is_medium: module.is_medium,
            is_update: module.is_update,
            is_ghost: module.is_ghost,
            is_hardcoded: module.is_hardcoded,
            valid: module.valid,
            loaded: module.loaded,
            masters_missing: module.masters_missing,
            active: module.active,
            active_in_plugins_txt: module.active_in_plugins_txt,
            official_index: module.official_index,
            cc_index: module.cc_index,
            plugins_txt_index: module.plugins_txt_index,
            load_order_txt_index: module.load_order_txt_index,
            date_time: module.date_time,
            has_blueprint: module.has_blueprint,
            masters: Vec::new(),
            master_names: module.masters.clone(),
            checked: false,
            disabled: false,
            hidden: false,
            expanded: false,
        }
    }

    pub fn from_nodes(
        data_path: String,
        mut nodes: Vec<ModuleNode>,
        mode: ModuleSelectMode,
        contained: &[String],
    ) -> Self {
        // `miMasters`: the node of each master, by name.
        for node in 0..nodes.len() {
            let names = nodes[node].master_names.clone();
            let masters: Vec<Option<usize>> = names
                .iter()
                .map(|name| nodes.iter().position(|other| other.name.eq_ignore_ascii_case(name)))
                .collect();
            nodes[node].masters = masters;
        }
        let (min_select, max_select, presets_available, mix_loaded) = match mode {
            ModuleSelectMode::Active => (0, usize::MAX, true, true),
            ModuleSelectMode::Insert => (1, 1, false, false),
        };
        let mut state = Self {
            data_path,
            rows: Vec::new(),
            filter: NameFilter::new(),
            preset: PresetCombo::default(),
            loaded_preset: String::new(),
            presets_available,
            error: None,
            min_select,
            max_select,
            mix_loaded,
            sort_column: 1,
            sort_descending: false,
            mode,
            question: None,
            placement: None,
            modules: nodes,
            rect: None,
            tree: TreeState::new(MODULE_COLUMNS.iter().map(|(_, width)| *width).collect()),
        };
        // `SelectFlag = mfActive` checks the active modules; the Insert
        // dialog starts with nothing checked.
        if mode == ModuleSelectMode::Active {
            for node in &mut state.modules {
                node.checked = node.active && node.valid;
            }
        }
        state.apply_flags(contained);
        state.simulate_and_refresh();
        state
    }

    /// `vstModulesInitNode`'s disabled and hidden states: `FilterFlag`,
    /// `mfMastersMissing`, `HideFlag` and the mod group editor's tag.
    fn apply_flags(&mut self, contained: &[String]) {
        for node in &mut self.modules {
            let tagged_out = self.mode == ModuleSelectMode::Insert
                && contained.iter().any(|name| name.eq_ignore_ascii_case(&node.name));
            node.disabled = !node.valid || node.masters_missing || tagged_out;
            if node.disabled {
                // `if InitialStates * [ivsHidden, ivsDisabled] <> [] then
                // Exclude(mndModule.miFlags, SelectFlag)`.
                node.checked = false;
            }
        }
    }

    /// `vstModulesChecked` and `vstModulesNodeDblClick`/`vstModulesKeyDown`:
    /// toggling a root row's check box, then `SimulateLoad`.
    pub fn toggle_row(&mut self, row_index: usize) {
        let Some(row) = self.rows.get(row_index) else {
            return;
        };
        // A double click loads the module; the check box of a child does
        // nothing (`DoSingleModuleLoad`).
        if row.master {
            return;
        }
        let node = row.node;
        if self.modules[node].disabled {
            return;
        }
        self.modules[node].checked = !self.modules[node].checked;
        self.simulate_and_refresh();
    }

    /// `DoSingleModuleLoad` without a node: checks the focused root row (or
    /// the one row selected with several selected) and simulates.
    pub fn select_single(&mut self, focus: Option<usize>, selected: &[usize]) {
        for node in &mut self.modules {
            node.checked = false;
        }
        let roots: Vec<usize> = selected
            .iter()
            .filter_map(|&row| self.rows.get(row))
            .filter(|row| !row.master && !row.disabled)
            .map(|row| row.node)
            .collect();
        if roots.len() > 1 && self.max_select > 1 {
            for node in roots {
                self.modules[node].checked = true;
            }
        } else if let Some(node) = roots.first().copied().or_else(|| {
            focus
                .and_then(|row| self.rows.get(row))
                .filter(|row| !row.master && !row.disabled)
                .map(|row| row.node)
        }) {
            self.modules[node].checked = true;
        }
        self.simulate_and_refresh();
    }

    /// `GetSelectedOrAll` of the bulk actions: the selected rows when two
    /// or more are selected, every enabled root row otherwise.
    fn bulk_scope(&self, selected: &[usize]) -> Vec<usize> {
        let enabled: Vec<bool> = self.rows.iter().map(|row| !row.disabled && !row.master).collect();
        crate::dialogs::selection::affected_rows(selected, &enabled)
    }

    /// `mniSelectAllClick`.
    pub fn select_all(&mut self, selected: &[usize]) {
        for row in self.bulk_scope(selected) {
            let node = self.rows[row].node;
            self.modules[node].checked = true;
        }
        self.simulate_and_refresh();
    }

    /// `mniSelectNoneClick`.
    pub fn select_none(&mut self, selected: &[usize]) {
        for row in self.bulk_scope(selected) {
            let node = self.rows[row].node;
            self.modules[node].checked = false;
        }
        self.simulate_and_refresh();
    }

    /// `mniInvertSelectionClick`.
    pub fn invert_selection(&mut self, selected: &[usize]) {
        for row in self.bulk_scope(selected) {
            let node = self.rows[row].node;
            self.modules[node].checked = !self.modules[node].checked;
        }
        self.simulate_and_refresh();
    }

    /// `edFilterChange` and `cbRegExFilterClick`: the rows that stay
    /// visible, then `SimulateLoad` (which the caller does).
    pub fn filter_changed(&mut self) {
        self.error = None;
        self.refresh_rows();
    }

    /// `vstModulesHeaderClick`: a click on the sort column flips the
    /// direction, another column starts ascending.
    pub fn header_click(&mut self, column: usize) {
        if self.sort_column == column {
            self.sort_descending = !self.sort_descending;
        } else {
            self.sort_column = column;
            self.sort_descending = false;
        }
        self.refresh_rows();
    }

    /// `SimulateLoad` and the row rebuild, which every change runs.
    pub fn simulate_and_refresh(&mut self) {
        let result = self.simulate();
        match result {
            Ok(()) => {
                let filtered = self.filtered_modules();
                self.error = if filtered.len() < self.min_select {
                    if filtered.is_empty() {
                        Some("No modules selected".to_owned())
                    } else {
                        Some(format!("Less than {} modules selected", self.min_select))
                    }
                } else if filtered.len() > self.max_select {
                    if self.max_select == 1 {
                        Some("More than 1 module selected".to_owned())
                    } else {
                        Some(format!("More than {} modules selected", self.max_select))
                    }
                } else {
                    None
                };
            }
            Err(message) => self.error = Some(message),
        }
        self.refresh_rows();
    }

    /// `TwbModuleInfos.SimulateLoad`: every module's `miLoadOrder` and
    /// `miFileID` for the checked selection, the masters first, with the
    /// full/medium/light slots of `TwbFileID`.
    fn simulate(&mut self) -> Result<(), String> {
        for node in &mut self.modules {
            node.simulated_load_order = None;
            node.simulated_file_id = "XX".to_owned();
        }
        let mut loaded = vec![false; self.modules.len()];
        let mut loading = vec![false; self.modules.len()];
        let mut order = Vec::new();
        for index in 0..self.modules.len() {
            let force = always_load_game_master() && self.modules[index].is_game_master();
            if self.modules[index].checked || force {
                Self::load_module(&self.modules, index, &mut loaded, &mut loading, &mut order)?;
            }
        }
        // `SimulateLoad` assigns `miLoadOrder` and `miFileID` as it loads,
        // which is the order it pushes into `NewLoadOrder`.
        let mut slots = Slots::new();
        for (position, &node) in order.iter().enumerate() {
            self.modules[node].simulated_load_order = Some(position as i32);
            let file_id = slot_text(&self.modules[node], &mut slots)?;
            self.modules[node].simulated_file_id = file_id;
        }
        Ok(())
    }

    /// `SimulateLoad`'s `Load` (`wbLoadOrder.pas`).
    fn load_module(
        modules: &[ModuleNode],
        index: usize,
        loaded: &mut [bool],
        loading: &mut [bool],
        order: &mut Vec<usize>,
    ) -> Result<(), String> {
        if loaded[index] {
            return Ok(());
        }
        if loading[index] {
            return Err(format!(
                "Modules contain circular references. Can't load \"{}\"",
                modules[index].name
            ));
        }
        loading[index] = true;
        for (master, name) in modules[index].masters.iter().zip(&modules[index].master_names) {
            match master {
                Some(master) => Self::load_module(modules, *master, loaded, loading, order)?,
                None => {
                    return Err(format!(
                        "Module \"{}\" requires master \"{name}\" which can not be found",
                        modules[index].name
                    ));
                }
            }
        }
        loading[index] = false;
        loaded[index] = true;
        order.push(index);
        Ok(())
    }

    /// The modules `SimulateLoad` returns as `SelectedModules`.
    pub fn selected_modules(&self) -> Vec<usize> {
        let mut modules: Vec<(i32, usize)> = self
            .modules
            .iter()
            .enumerate()
            .filter_map(|(node, module)| module.simulated_load_order.map(|order| (order, node)))
            .collect();
        modules.sort_unstable();
        modules.into_iter().map(|(_, node)| node).collect()
    }

    /// `FilteredModules`: the checked valid modules in simulated order.
    pub fn filtered_modules(&self) -> Vec<usize> {
        self.selected_modules()
            .into_iter()
            .filter(|&node| self.modules[node].checked && self.modules[node].valid)
            .collect()
    }

    /// The names of [`ModuleSelectState::filtered_modules`].
    pub fn filtered_names(&self) -> Vec<String> {
        self.filtered_modules()
            .into_iter()
            .map(|node| self.modules[node].name.clone())
            .collect()
    }

    /// The checked modules' names, in the list's order (the `modules`
    /// parameter of `modules.preset_save`).
    pub fn checked_names(&self) -> Vec<String> {
        self.modules
            .iter()
            .filter(|module| module.checked && module.valid)
            .map(|module| module.name.clone())
            .collect()
    }

    /// `SimulateLoad` and `FilteredModules` over a fresh simulation: the
    /// plugins to load, with the masters they need, as full paths.
    pub fn load_order(&mut self) -> Result<Vec<String>, String> {
        self.simulate_and_refresh();
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        Ok(self
            .filtered_modules()
            .into_iter()
            .map(|node| self.modules[node].path.clone())
            .collect())
    }

    /// The module names of a successful OK, for the mod group editor's
    /// Insert (`SelectedModules[0]` with `MaxSelect = 1`).
    pub fn selected_names(&mut self) -> Result<Vec<String>, String> {
        self.simulate_and_refresh();
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        Ok(self
            .filtered_modules()
            .into_iter()
            .map(|node| self.modules[node].name.clone())
            .collect())
    }

    /// `vstModulesInitNode`/`vstModulesInitChildren` and `CompareNodes`:
    /// the visible rows in the sort order.
    pub fn refresh_rows(&mut self) {
        let mut visible: Vec<usize> = Vec::new();
        for (node, module) in self.modules.iter().enumerate() {
            if module.hidden || !module.valid {
                continue;
            }
            if self.filter.is_active() && !self.filter.matches(&module.name) {
                continue;
            }
            visible.push(node);
        }
        // `CompareNodes`; column 1 has no case of its own, so it sorts by
        // the module index (`CmpI32(mndIndex)`).
        let column = self.sort_column;
        let descending = self.sort_descending;
        visible.sort_by(|&a, &b| {
            let (left, right) = (&self.modules[a], &self.modules[b]);
            let ordering = match column {
                0 => left.name.to_uppercase().cmp(&right.name.to_uppercase()),
                2 => left.has_esm_flag.cmp(&right.has_esm_flag),
                3 => left.is_light.cmp(&right.is_light),
                4 => left
                    .simulated_load_order
                    .unwrap_or(i32::MAX)
                    .cmp(&right.simulated_load_order.unwrap_or(i32::MAX)),
                5 => left
                    .simulated_file_id
                    .to_uppercase()
                    .cmp(&right.simulated_file_id.to_uppercase()),
                6 => left.is_update.cmp(&right.is_update),
                7 => left.is_medium.cmp(&right.is_medium),
                _ => std::cmp::Ordering::Equal,
            };
            let ordering = if descending { ordering.reverse() } else { ordering };
            ordering.then(left.index.cmp(&right.index))
        });
        self.rows.clear();
        for node in visible {
            let module = &self.modules[node];
            self.rows.push(ModuleRow {
                row: self.rows.len(),
                node,
                master: false,
                depth: 0,
                has_children: !module.masters.is_empty(),
                expanded: module.expanded,
                check: module.check_state(self.mix_loaded),
                disabled: module.disabled,
                name: module.name.clone(),
                file_order: module.load_order_description(),
                esm: if module.has_esm_flag {
                    "ESM".to_owned()
                } else {
                    String::new()
                },
                light: if module.is_light {
                    "Light".to_owned()
                } else {
                    String::new()
                },
                load_order: module
                    .simulated_load_order
                    .filter(|order| *order < 10000)
                    .map(|order| order.to_string())
                    .unwrap_or_default(),
                file_id: if module.simulated_load_order.is_some_and(|order| order < 10000) {
                    module.simulated_file_id.clone()
                } else {
                    String::new()
                },
                update: if module.is_update {
                    "Update".to_owned()
                } else {
                    String::new()
                },
                medium: if module.is_medium {
                    "Med".to_owned()
                } else {
                    String::new()
                },
                ghost: module.is_ghost,
                masters_missing: module.masters_missing,
            });
            if module.expanded {
                for (master, name) in module.masters.iter().zip(&module.master_names) {
                    let (check, disabled) = match master {
                        Some(master) => (self.modules[*master].check_state(self.mix_loaded), true),
                        None => (CheckState::Unchecked, true),
                    };
                    let shown = master.map(|master| self.modules[master].name.clone());
                    self.rows.push(ModuleRow {
                        row: self.rows.len(),
                        node: master.unwrap_or(node),
                        master: true,
                        depth: 1,
                        has_children: false,
                        expanded: false,
                        check,
                        disabled,
                        name: shown.unwrap_or_else(|| name.clone()),
                        file_order: String::new(),
                        esm: String::new(),
                        light: String::new(),
                        load_order: String::new(),
                        file_id: String::new(),
                        update: String::new(),
                        medium: String::new(),
                        ghost: false,
                        masters_missing: master.is_none(),
                    });
                }
            }
        }
    }

    /// `vstModulesInitChildren` + `toToggleOnDblClick`: expands or collapses
    /// a module's masters.
    pub fn toggle_expanded(&mut self, row_index: usize) -> bool {
        let Some(row) = self.rows.get(row_index) else {
            return false;
        };
        if row.master || !row.has_children {
            return false;
        }
        let node = row.node;
        self.modules[node].expanded = !self.modules[node].expanded;
        self.refresh_rows();
        true
    }

    /// `PresetsUpdate`: the combo box of `modules.presets`.
    pub fn set_presets(&mut self, presets: Vec<String>) {
        self.preset.items = presets;
        self.preset.pseudo = vec!["<plugins.txt>".to_owned()];
        if !self.preset.items.iter().any(|item| item == "<plugins.txt>") {
            self.preset.items.insert(0, "<plugins.txt>".to_owned());
        }
        if self.preset.text.is_empty() {
            self.preset.text = "<plugins.txt>".to_owned();
        }
    }

    /// `PresetLoad` with the response of `modules.preset_load`: the named
    /// modules are checked, the rest cleared; `LoadedPreset` remembers the
    /// name (empty for `<plugins.txt>`).
    pub fn apply_preset_load(&mut self, preset: &str, modules: &[String]) {
        self.loaded_preset = if preset.starts_with('<') {
            String::new()
        } else {
            preset.to_owned()
        };
        for node in &mut self.modules {
            node.checked = false;
        }
        for name in modules {
            if let Some(node) = self
                .modules
                .iter_mut()
                .find(|node| node.valid && node.name.eq_ignore_ascii_case(name))
            {
                node.checked = true;
            }
        }
        self.simulate_and_refresh();
    }

    /// `<plugins.txt>`: `(mfActiveInPluginsTxt in miFlags) or (miOfficialIndex
    /// < High(Integer))`.
    pub fn apply_plugins_txt_preset(&mut self) {
        self.loaded_preset.clear();
        for node in &mut self.modules {
            node.checked = node.valid && (node.active_in_plugins_txt || node.official_index.is_some());
        }
        self.simulate_and_refresh();
    }
}

/// The `TwbFileID` slots of `SimulateLoad`.
struct Slots {
    full: i32,
    medium: i32,
    light: i32,
    max_full: i32,
    max_medium: i32,
    max_light: i32,
}

impl Slots {
    fn new() -> Self {
        // `TwbFileID.MaxFullSlot` and friends.
        let light = pseudo_light() || is_light_supported();
        let medium = pseudo_medium() || is_medium_supported();
        let mut max_full = 0xFE;
        if light {
            max_full -= 1;
            if medium {
                max_full -= 1;
            }
        }
        Self {
            full: 0,
            medium: 0,
            light: 0,
            max_full,
            max_medium: if medium { 0xFF } else { -1 },
            max_light: if light { 0xFFF } else { -1 },
        }
    }
}

/// The `miFileID` assignment of `SimulateLoad` for one module, in the order
/// the load assigns them, including `TwbFileID.ToString`.
fn slot_text(module: &ModuleNode, slots: &mut Slots) -> Result<String, String> {
    // `if not (wbPseudoLight or wbPseudoUpdate) then ...`: with either
    // pseudo mode every id stays invalid.
    if pseudo_light() || pseudo_update() {
        return Ok("XX".to_owned());
    }
    if module.is_update && !ignore_update() {
        return Ok("XX".to_owned());
    }
    if module.is_light && !ignore_light() {
        if slots.light > slots.max_light {
            return Err("Too many light modules".to_owned());
        }
        let slot = slots.light;
        slots.light += 1;
        return Ok(format!("FE {slot:03X}"));
    }
    if module.is_medium && !ignore_medium() {
        if slots.medium > slots.max_medium {
            return Err("Too many heavy modules".to_owned());
        }
        let slot = slots.medium;
        slots.medium += 1;
        return Ok(format!("FD {slot:02X}"));
    }
    if slots.full > slots.max_full {
        return Err("Too many full modules".to_owned());
    }
    let slot = slots.full;
    slots.full += 1;
    Ok(format!("{slot:02X}"))
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
        match ModuleSelectState::open(game, ModuleSelectMode::Active, &[]) {
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
        state.search = "SE".to_owned();
        state.search_changed();
        assert_eq!(state.selected, Some(6)); // enderalse
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

    fn node(node: usize, name: &str, active: bool, valid: bool) -> ModuleNode {
        ModuleNode {
            index: node,
            node,
            name: name.to_owned(),
            original_name: name.to_owned(),
            path: format!("C:\\Data\\{name}"),
            load_order: None,
            file_id: None,
            simulated_load_order: None,
            simulated_file_id: "XX".to_owned(),
            has_esm_flag: name.ends_with(".esm"),
            is_esm: name.ends_with(".esm"),
            is_light: false,
            is_medium: false,
            is_update: false,
            is_ghost: false,
            is_hardcoded: false,
            valid,
            loaded: false,
            masters_missing: false,
            active,
            active_in_plugins_txt: active,
            official_index: None,
            cc_index: None,
            plugins_txt_index: None,
            load_order_txt_index: None,
            date_time: 0.0,
            has_blueprint: false,
            masters: Vec::new(),
            master_names: Vec::new(),
            checked: false,
            disabled: false,
            hidden: false,
            expanded: false,
        }
    }

    fn state_with(modules: Vec<ModuleNode>) -> ModuleSelectState {
        ModuleSelectState::from_nodes("C:\\Data\\".to_owned(), modules, ModuleSelectMode::Active, &[])
    }

    #[test]
    fn the_module_state_machines_toggle_every_valid_module() {
        let mut state = state_with(vec![
            node(0, "Skyrim.esm", true, true),
            node(1, "Update.esm", true, true),
            node(2, "Mod.esp", false, true),
            node(3, "Broken.esp", false, false),
        ]);
        // `SelectFlag = mfActive`: only the active modules start checked.
        assert_eq!(state.checked_names(), vec!["Skyrim.esm", "Update.esm"]);
        // The invalid module is never listed.
        assert_eq!(state.rows.len(), 3);
        state.select_all(&[]);
        assert_eq!(state.checked_names().len(), 3);
        state.select_none(&[]);
        assert!(state.checked_names().is_empty());
        state.invert_selection(&[]);
        assert_eq!(state.checked_names().len(), 3);
        state.toggle_row(2); // Mod.esp, row index 2
        assert_eq!(state.checked_names(), vec!["Skyrim.esm", "Update.esm"]);
        // `edFilterChange`: a substring match, without regard to case.
        state.filter.set_text("mod");
        state.refresh_rows();
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].name, "Mod.esp");
        assert!(state.rows[0].check == CheckState::Unchecked);
        state.filter.set_text("nothing-matches");
        state.refresh_rows();
        assert!(state.rows.is_empty());
    }

    #[test]
    fn the_simulated_load_assigns_order_and_file_ids() {
        let mut game_master = node(0, "Fallout4.esm", true, true);
        game_master.official_index = Some(i32::MIN);
        let state = state_with(vec![
            game_master,
            node(1, "DLCRobot.esm", true, true),
            node(2, "Mod.esp", true, true),
        ]);
        // The state starts with the active modules checked and simulates.
        let names = state.filtered_names();
        assert_eq!(names, vec!["Fallout4.esm", "DLCRobot.esm", "Mod.esp"]);
        assert_eq!(state.modules[0].simulated_load_order, Some(0));
        assert_eq!(state.modules[2].simulated_load_order, Some(2));
        let order: Vec<String> = state.rows.iter().map(|row| row.file_id.clone()).collect();
        assert_eq!(order, vec!["00", "01", "02"]);
        let order_column: Vec<String> = state.rows.iter().map(|row| row.load_order.clone()).collect();
        assert_eq!(order_column, vec!["0", "1", "2"]);
        assert!(state.error.is_none());
    }

    #[test]
    fn an_unchecked_master_loads_before_its_dependent() {
        let mut dependent = node(1, "Patch.esp", true, true);
        dependent.master_names = vec!["Fallout4.esm".to_owned()];
        let mut game_master = node(0, "Fallout4.esm", false, true);
        game_master.official_index = Some(i32::MIN);
        let mut state = state_with(vec![game_master, dependent]);
        state.modules[1].masters = vec![Some(0)];
        state.simulate_and_refresh();
        // The game master is force-loaded (`wbAlwaysLoadGameMaster`) and
        // filtered out because it is not checked.
        assert_eq!(state.filtered_names(), vec!["Patch.esp"]);
        assert_eq!(state.modules[0].simulated_load_order, Some(0));
        assert_eq!(state.modules[1].simulated_load_order, Some(1));
        assert!(state.modules[0].simulated_file_id == "00");
        assert!(state.modules[1].simulated_file_id == "01");
    }

    #[test]
    fn a_missing_master_is_an_error_and_disables_its_module() {
        let mut dependent = node(0, "Patch.esp", true, true);
        dependent.master_names = vec!["Missing.esm".to_owned()];
        dependent.masters_missing = true;
        let mut state = state_with(vec![dependent]);
        assert!(state.error.is_none());
        assert!(state.modules[0].disabled);
        assert!(state.checked_names().is_empty());
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].name, "Patch.esp");
        // The filter is on the node name, not on the master rows.
        state.modules[0].expanded = true;
        state.refresh_rows();
        assert_eq!(state.rows.len(), 2);
        assert_eq!(state.rows[1].name, "Missing.esm");
        assert!(state.rows[1].masters_missing);
    }

    #[test]
    fn a_circular_master_list_is_refused_with_the_upstream_text() {
        let mut one = node(0, "One.esp", true, true);
        one.master_names = vec!["Two.esp".to_owned()];
        let mut two = node(1, "Two.esp", true, true);
        two.master_names = vec!["One.esp".to_owned()];
        let mut state = state_with(vec![one, two]);
        state.modules[0].masters = vec![Some(1)];
        state.modules[1].masters = vec![Some(0)];
        state.simulate_and_refresh();
        assert_eq!(
            state.error.as_deref(),
            Some("Modules contain circular references. Can't load \"One.esp\"")
        );
    }

    #[test]
    fn the_insert_mode_disables_the_contained_modules_and_limits_the_selection() {
        let mut state = ModuleSelectState::from_nodes(
            "C:\\Data\\".to_owned(),
            vec![
                node(0, "A.esp", true, true),
                node(1, "B.esp", true, true),
                node(2, "Broken.esp", false, false),
            ],
            ModuleSelectMode::Insert,
            &["A.esp".to_owned()],
        );
        assert!(!state.presets_available);
        assert!(state.modules[0].disabled);
        assert!(!state.modules[1].disabled);
        assert!(state.checked_names().is_empty());
        state.toggle_row(1);
        assert_eq!(state.selected_names().unwrap(), vec!["B.esp"]);
        state.toggle_row(0);
        assert_eq!(state.checked_names(), vec!["B.esp"]);
        // MaxSelect = 1: a second checked module is the dialog's error.
        state.modules[0].disabled = false;
        state.modules[0].checked = true;
        state.simulate_and_refresh();
        assert_eq!(state.error.as_deref(), Some("More than 1 module selected"));
    }

    #[test]
    fn a_header_click_sorts_and_flips_the_direction() {
        let a = node(0, "B.esp", false, true);
        let b = node(1, "A.esm", false, true);
        let mut c = node(2, "C.esp", false, true);
        c.has_esm_flag = true;
        let mut state = state_with(vec![a, b, c]);
        // `Header.SortColumn := 1` has no `CompareNodes` case of its own, so
        // it falls back to `CmpI32(mndIndex)`: the list order.
        assert_eq!(
            state.rows.iter().map(|row| row.name.clone()).collect::<Vec<_>>(),
            vec!["B.esp", "A.esm", "C.esp"]
        );
        state.header_click(0);
        assert!(!state.sort_descending);
        assert_eq!(
            state.rows.iter().map(|row| row.name.clone()).collect::<Vec<_>>(),
            vec!["A.esm", "B.esp", "C.esp"]
        );
        state.header_click(0);
        assert!(state.sort_descending);
        assert_eq!(
            state.rows.iter().map(|row| row.name.clone()).collect::<Vec<_>>(),
            vec!["C.esp", "B.esp", "A.esm"]
        );
        state.header_click(2);
        assert!(!state.sort_descending);
        let esm: Vec<bool> = state.rows.iter().map(|row| !row.esm.is_empty()).collect();
        assert_eq!(esm, vec![false, true, true]);
    }

    #[test]
    fn the_plugins_txt_preset_checks_the_plugin_list_and_official_modules() {
        let mut state = state_with(vec![
            node(0, "Fallout4.esm", true, true),
            node(1, "Mod.esp", true, true),
            node(2, "Implicit.esp", false, true),
        ]);
        state.modules[2].active = true;
        state.modules[2].active_in_plugins_txt = false;
        state.modules[0].active_in_plugins_txt = false;
        state.modules[0].official_index = Some(i32::MIN);
        state.apply_plugins_txt_preset();
        assert_eq!(state.checked_names(), vec!["Fallout4.esm", "Mod.esp"]);
    }

    #[test]
    fn a_preset_load_checks_the_named_modules() {
        let mut state = state_with(vec![node(0, "A.esp", false, true), node(1, "B.esp", false, true)]);
        state.apply_preset_load("Mine", &["b.ESP".to_owned()]);
        assert_eq!(state.checked_names(), vec!["B.esp"]);
        assert_eq!(state.loaded_preset, "Mine");
        state.apply_preset_load("<plugins.txt>", &[]);
        assert!(state.loaded_preset.is_empty());
    }

    #[test]
    fn the_load_order_description_is_upstreams() {
        let mut module = node(0, "Update.esm", false, true);
        module.is_update = true;
        module.official_index = Some(-1);
        assert_eq!(module.load_order_description(), "[Update]");
        module.official_index = Some(i32::MIN);
        assert_eq!(module.load_order_description(), "[GameMaster]");
        module.official_index = None;
        module.is_update = false;
        module.plugins_txt_index = Some(4);
        assert_eq!(module.load_order_description(), "[ESM][Plugins.txt:4]");
        module.is_esm = false;
        module.plugins_txt_index = None;
        assert!(module.load_order_description().starts_with("[Time:"));
    }

    #[test]
    fn the_file_id_strings_are_the_twb_file_id_ones() {
        let mut light = node(0, "Light.esl", false, true);
        light.is_light = true;
        light.checked = true;
        let mut full = node(1, "Full.esp", false, true);
        full.is_esm = false;
        full.checked = true;
        let mut state = state_with(vec![light, full]);
        state.simulate_and_refresh();
        if is_light_supported() && !pseudo_light() {
            assert_eq!(state.modules[0].simulated_file_id, "FE 000");
            assert_eq!(state.modules[1].simulated_file_id, "00");
        } else {
            assert_eq!(state.modules[0].simulated_file_id, "XX");
        }
    }
}
