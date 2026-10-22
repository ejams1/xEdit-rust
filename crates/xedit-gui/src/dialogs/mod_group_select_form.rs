// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeModGroupSelectForm.pas (FormCreate, FormShow,
// FormKeyDown, FormClose, edFilterChange, vstModGroupsInitNode,
// vstModGroupsInitChildren, vstModGroupsGetText,
// vstModGroupsBeforeGetCheckState, vstModGroupsChecked,
// vstModGroupsCompareNodes, vstModGroupsPaintText,
// vstModGroupsNodeDblClick, vstModGroupsKeyDown, vstModGroupsKeyPress,
// mniSelectAllClick, mniSelectNoneClick, mniInvertSelectionClick,
// CollectSelected, DoSingleModGroupLoad, PresetLoad, PresetSave,
// PresetDelete, PresetsUpdate, GetPresetsSection, IsPresetDifferent,
// ShowModal) and xEdit/xeModGroupSelectForm.dfm

//! `TfrmModGroupSelect`: the mod groups of the loaded plugins with their
//! items and CRC32s, the filter, the bulk selection and the presets. The
//! dialog lists the groups, the selection is the names the shell hands to
//! `modgroups.select`.

use eframe::egui;

use xedit_session::CommandError;
use xedit_session::modgroups::{ItemInfo, ModGroupInfo};

use crate::dialogs::selection::{
    CheckState, DialogPlacement, NameFilter, PresetCombo, Question, affected_rows, show_question,
};
use crate::widgets::conflict::ConflictColors;
use crate::widgets::virtual_tree::{TreeCell, TreeColumn, TreeKeyAction, TreeRow, TreeSource, TreeState, TreeView};
use xedit_core::interface::types::{ConflictAll, ConflictThis};

/// `csLast`: the preset that loads `[ModGroups] Selection`.
pub const CS_LAST: &str = "<Last>";

/// The columns of `vstModGroups` in `xeModGroupSelectForm.dfm`.
pub const MOD_GROUP_COLUMNS: [(&str, f32); 6] = [
    ("Name", 494.0),
    ("Optional", 70.0),
    ("Target", 70.0),
    ("Source", 70.0),
    ("Forbidden", 70.0),
    ("Ignore LO", 70.0),
];

/// What the dialog asks the shell to do; no command runs here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModGroupSelectAction {
    /// `mrOk`: the selected names, for `modgroups.select`.
    Ok(Vec<String>),
    Cancelled,
    /// `PresetLoad`: the shell reads the preset's value (`settings.get`)
    /// and calls [`ModGroupSelectState::apply_preset`].
    PresetLoad(String),
    /// `PresetSave`: the shell selects the current names and writes the
    /// preset.
    PresetSave {
        preset: String,
        after_modal: bool,
    },
    /// `PresetDelete`.
    PresetDelete(String),
}

/// One visible row of the tree: a group, one of its items or one of an
/// item's CRC32s.
#[derive(Clone, Debug, PartialEq)]
pub struct ModGroupRow {
    pub row: usize,
    pub group: usize,
    pub item: Option<usize>,
    pub crc: Option<usize>,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub check: CheckState,
    pub disabled: bool,
    pub text: String,
    pub optional: String,
    pub target: String,
    pub source: String,
    pub forbidden: String,
    pub ignore_load_order: String,
    /// `mgfValid` or `mgifHasFile`: the row is struck out upstream.
    pub invalid: bool,
}

/// The `MessageDlg` the preset buttons ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModGroupQuestion {
    DeletePreset(String),
    OverwritePreset(String),
    ChangedPreset(String),
}

/// The selection dialog (`TfrmModGroupSelect`).
pub struct ModGroupSelectState {
    /// The groups `modgroups.list` returned (`wbModGroupsByName`).
    pub groups: Vec<ModGroupInfo>,
    /// The flattened rows.
    pub rows: Vec<ModGroupRow>,
    /// `edFilter`: the substring match, without a regular expression box.
    pub filter: NameFilter,
    pub preset: PresetCombo,
    pub loaded_preset: String,
    /// The value the loaded preset held, for `IsPresetDifferent`.
    loaded_value: Vec<String>,
    /// `pnlError`'s text without the `Error: ` prefix.
    pub error: Option<String>,
    pub min_select: usize,
    pub max_select: usize,
    /// The window caption (`Caption := ...` of the handler).
    pub caption: String,
    /// `PresetCategory <> ''`: the preset controls are shown.
    pub presets_visible: bool,
    /// `FilterFlag <> mgfNone`.
    pub filter_valid_only: bool,
    /// The groups whose items `InitChildren` expanded.
    pub expanded_groups: Vec<bool>,
    /// Per group and item, whether the CRC32s are shown.
    pub expanded_items: Vec<Vec<bool>>,
    pub question: Option<ModGroupQuestion>,
    pub placement: Option<DialogPlacement>,
    pub rect: Option<egui::Rect>,
    pub tree: TreeState,
}

impl ModGroupSelectState {
    /// The dialog of `mniViewModGroupsReloadClick`/`mniNavEditModGroupClick`:
    /// the groups the command listed, the saved selection checked and the
    /// preset combo filled (`PresetsUpdate`).
    // The parameters are the dialog's public fields of the upstream form
    // (`PresetCategory`, `SelectFlag`, `FilterFlag`, `MinSelect`,
    // `MaxSelect`, `Caption`), so they stay one constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        groups: Vec<ModGroupInfo>,
        presets: Vec<String>,
        last: bool,
        stored_selection: &[String],
        caption: impl Into<String>,
        filter_valid_only: bool,
        min_select: usize,
        max_select: usize,
    ) -> Self {
        let expanded_items = groups.iter().map(|group| vec![false; group.items.len()]).collect();
        let expanded_groups = vec![false; groups.len()];
        let mut state = Self {
            groups,
            rows: Vec::new(),
            filter: NameFilter::new(),
            preset: PresetCombo::new(Vec::new(), vec![CS_LAST.to_owned()]),
            loaded_preset: String::new(),
            loaded_value: Vec::new(),
            error: None,
            min_select,
            max_select,
            caption: caption.into(),
            presets_visible: true,
            filter_valid_only,
            expanded_groups,
            expanded_items,
            question: None,
            placement: None,
            rect: None,
            tree: TreeState::new(MOD_GROUP_COLUMNS.iter().map(|(_, width)| *width).collect()),
        };
        let mut items = Vec::new();
        if last {
            items.push(CS_LAST.to_owned());
        }
        items.extend(presets);
        state.preset.items = items;
        // `LoadModGroupsSelection` tags the saved selection; the edit flow
        // clears every tag before the dialog opens.
        for group in &mut state.groups {
            group.selected = stored_selection
                .iter()
                .any(|name| xedit_loadorder::ini_files::same_text(name, &group.name));
        }
        state.collect_selected();
        state
    }

    /// The dialog of the edit flow: no group is checked.
    pub fn for_edit(groups: Vec<ModGroupInfo>, caption: impl Into<String>) -> Self {
        let mut state = Self::new(groups, Vec::new(), false, &[], caption, false, 1, 1);
        state.presets_visible = false;
        for group in &mut state.groups {
            group.selected = false;
        }
        state.collect_selected();
        state
    }

    /// `CollectSelected`: the checked groups, then `MinSelect`/`MaxSelect`.
    pub fn collect_selected(&mut self) {
        let selected = self.checked_groups();
        self.error = if selected.len() < self.min_select {
            if selected.is_empty() {
                Some("No ModGroups selected".to_owned())
            } else {
                Some(format!("Less than {} ModGroups selected", self.min_select))
            }
        } else if selected.len() > self.max_select {
            if self.max_select == 1 {
                Some("More than 1 ModGroup selected".to_owned())
            } else {
                Some(format!("More than {} ModGroups selected", self.max_select))
            }
        } else {
            None
        };
        self.refresh_rows();
    }

    /// `SelectedModGroups.FilteredByFlag(SelectFlag)`: the checked groups'
    /// names in the list order.
    pub fn checked_groups(&self) -> Vec<String> {
        self.groups
            .iter()
            .filter(|group| group.selected)
            .map(|group| group.name.clone())
            .collect()
    }

    /// `vstModGroupsChecked`: a group's check box, then `CollectSelected`.
    pub fn toggle_group(&mut self, group: usize) {
        if let Some(entry) = self.groups.get_mut(group) {
            entry.selected = !entry.selected;
        }
        self.collect_selected();
    }

    /// `GetSelectedOrAll` of the bulk actions: the selected rows when two
    /// or more are selected, every enabled row otherwise. Only group rows
    /// change; item and CRC32 rows are disabled.
    fn bulk_scope(&self, selected: &[usize]) -> Vec<usize> {
        let enabled: Vec<bool> = self.rows.iter().map(|row| !row.disabled && row.depth == 0).collect();
        affected_rows(selected, &enabled)
    }

    fn set_scope(&mut self, selected: &[usize], check: impl Fn(bool) -> bool) {
        for row in self.bulk_scope(selected) {
            let group = self.rows[row].group;
            let checked = self.groups[group].selected;
            self.groups[group].selected = check(checked);
        }
        self.collect_selected();
    }

    /// `mniSelectAllClick`.
    pub fn select_all(&mut self, selected: &[usize]) {
        self.set_scope(selected, |_| true);
    }

    /// `mniSelectNoneClick`.
    pub fn select_none(&mut self, selected: &[usize]) {
        self.set_scope(selected, |_| false);
    }

    /// `mniInvertSelectionClick`.
    pub fn invert_selection(&mut self, selected: &[usize]) {
        self.set_scope(selected, |checked| !checked);
    }

    /// `DoSingleModGroupLoad`: only the row's group checked, then OK.
    pub fn select_single(&mut self, row: usize) {
        let Some(selected) = self.rows.get(row) else {
            return;
        };
        let group = selected.group;
        for entry in &mut self.groups {
            entry.selected = false;
        }
        self.groups[group].selected = true;
        self.collect_selected();
    }

    /// `edFilterChange`: a substring match of the group names; the tree is
    /// rebuilt.
    pub fn filter_changed(&mut self) {
        self.refresh_rows();
    }

    /// `vstModGroupsInitNode`/`InitChildren`: the visible rows.
    pub fn refresh_rows(&mut self) {
        let mut rows = Vec::new();
        for (group_index, group) in self.groups.iter().enumerate() {
            if self.filter_valid_only && !group.valid {
                continue;
            }
            if self.filter.is_active() && !self.filter.matches(&group.name) {
                continue;
            }
            let expanded = self.expanded_groups.get(group_index).copied().unwrap_or(false);
            rows.push(ModGroupRow {
                row: rows.len(),
                group: group_index,
                item: None,
                crc: None,
                depth: 0,
                has_children: !group.items.is_empty(),
                expanded,
                check: if group.selected {
                    CheckState::Checked
                } else {
                    CheckState::Unchecked
                },
                disabled: self.filter_valid_only && !group.valid,
                text: group.name.clone(),
                optional: String::new(),
                target: String::new(),
                source: String::new(),
                forbidden: String::new(),
                ignore_load_order: String::new(),
                invalid: !group.valid,
            });
            if !expanded {
                continue;
            }
            for (item_index, item) in group.items.iter().enumerate() {
                let item_expanded = self
                    .expanded_items
                    .get(group_index)
                    .and_then(|items| items.get(item_index))
                    .copied()
                    .unwrap_or(false);
                rows.push(item_row(group_index, item_index, item, item_expanded, rows.len()));
                if !item_expanded {
                    continue;
                }
                for (crc_index, crc) in item.crc32s.iter().enumerate() {
                    let checked = item
                        .current_crc32
                        .as_deref()
                        .is_some_and(|value| value.eq_ignore_ascii_case(crc));
                    rows.push(ModGroupRow {
                        row: rows.len(),
                        group: group_index,
                        item: Some(item_index),
                        crc: Some(crc_index),
                        depth: 2,
                        has_children: false,
                        expanded: false,
                        check: if checked {
                            CheckState::Checked
                        } else {
                            CheckState::Unchecked
                        },
                        disabled: true,
                        text: crc.clone(),
                        optional: String::new(),
                        target: String::new(),
                        source: String::new(),
                        forbidden: String::new(),
                        ignore_load_order: String::new(),
                        invalid: false,
                    });
                }
            }
        }
        self.rows = rows;
    }

    /// `toToggleOnDblClick`: a group expands its items, an item its CRC32s.
    /// The expansion lives in the state, so the row rebuild keeps it.
    pub fn toggle_expanded(&mut self, row_index: usize) -> bool {
        let Some(row) = self.rows.get(row_index) else {
            return false;
        };
        if !row.has_children {
            return false;
        }
        let (group, item) = (row.group, row.item);
        let expanded = match item {
            None => self.expanded_groups.get_mut(group).map(|entry| {
                *entry = !*entry;
                *entry
            }),
            Some(item) => self
                .expanded_items
                .get_mut(group)
                .and_then(|items| items.get_mut(item))
                .map(|entry| {
                    *entry = !*entry;
                    *entry
                }),
        };
        if expanded.is_none() {
            return false;
        }
        self.refresh_rows();
        true
    }

    /// `PresetsUpdate`, then a load: the preset's comma text is the names
    /// of the groups it checks (`<Last>` is the saved selection).
    pub fn apply_preset(&mut self, preset: &str, value: &str) {
        let names = xedit_loadorder::ini_files::parse_comma_text(value);
        for group in &mut self.groups {
            group.selected = names
                .iter()
                .any(|name| xedit_loadorder::ini_files::same_text(name, &group.name));
        }
        self.loaded_preset = preset.to_owned();
        self.loaded_value = names;
        self.collect_selected();
    }

    /// `IsPresetDifferent`: the checked names and the preset's names as
    /// sorted lists without duplicates, compared without regard to case.
    pub fn is_preset_different(&self) -> bool {
        // `aPreset.StartsWith('<')` exits upstream, `<Last>` included.
        if self.loaded_preset.is_empty() || self.loaded_preset.starts_with('<') {
            return false;
        }
        let normalise = |mut names: Vec<String>| {
            names.sort_by(|a, b| xedit_loadorder::mod_groups::compare_text(a, b));
            names.dedup_by(|a, b| xedit_loadorder::ini_files::same_text(a, b));
            names
        };
        let current = normalise(self.checked_groups());
        let preset = normalise(self.loaded_value.clone());
        current.len() != preset.len()
            || !xedit_loadorder::ini_files::same_text(
                &xedit_loadorder::ini_files::comma_text(&current),
                &xedit_loadorder::ini_files::comma_text(&preset),
            )
    }

    /// `PresetsUpdate` after a save/delete: the section names, `<Last>`
    /// first when the saved selection exists.
    pub fn set_presets(&mut self, presets: Vec<String>, last: bool) {
        let mut items = Vec::new();
        if last {
            items.push(CS_LAST.to_owned());
        }
        items.extend(presets);
        self.preset.items = items;
    }

    /// The `modgroups.select` request's names, when the dialog can confirm.
    pub fn confirm(&self) -> Result<Vec<String>, CommandError> {
        if let Some(error) = &self.error {
            return Err(CommandError::new("invalid_params", error.clone()));
        }
        Ok(self.checked_groups())
    }
}

/// The columns of one item row (`vstModGroupsGetText`).
fn item_row(group: usize, item_index: usize, item: &ItemInfo, expanded: bool, row: usize) -> ModGroupRow {
    ModGroupRow {
        row,
        group,
        item: Some(item_index),
        crc: None,
        depth: 1,
        has_children: !item.crc32s.is_empty(),
        expanded,
        check: CheckState::Unchecked,
        disabled: true,
        text: item.file_name.clone(),
        optional: if item.optional { "Optional" } else { "" }.to_owned(),
        target: if item.target { "Target" } else { "" }.to_owned(),
        source: if item.source { "Source" } else { "" }.to_owned(),
        forbidden: if item.forbidden { "Forbidden" } else { "" }.to_owned(),
        ignore_load_order: if item.ignore_load_order {
            "Always"
        } else if item.ignore_load_order_in_block {
            "in Block"
        } else {
            ""
        }
        .to_owned(),
        invalid: !item.has_file,
    }
}

/// The tree adapter.
struct ModGroupTree<'a> {
    rows: &'a [ModGroupRow],
    columns: Vec<TreeColumn>,
}

impl<'a> ModGroupTree<'a> {
    fn new(rows: &'a [ModGroupRow]) -> Self {
        Self {
            rows,
            columns: MOD_GROUP_COLUMNS
                .iter()
                .map(|(title, width)| TreeColumn::new(*title, *width))
                .collect(),
        }
    }
}

impl TreeSource for ModGroupTree<'_> {
    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn columns(&self) -> &[TreeColumn] {
        &self.columns
    }

    fn row(&self, index: usize, out: &mut TreeRow) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        out.id = row.row as u64;
        out.depth = row.depth;
        out.has_children = row.has_children;
        out.expanded = row.expanded;
        out.icon = match row.depth {
            0 => Some(row.check.glyph()),
            2 => Some(if row.check == CheckState::Checked {
                '\u{25C9}'
            } else {
                '\u{25CB}'
            }),
            _ => None,
        };
        out.cells = vec![
            TreeCell::new(row.text.clone()),
            TreeCell::new(row.optional.clone()),
            TreeCell::new(row.target.clone()),
            TreeCell::new(row.source.clone()),
            TreeCell::new(row.forbidden.clone()),
            TreeCell::new(row.ignore_load_order.clone()),
        ];
        if row.invalid {
            out.cells[0].tooltip = Some("The group or item is not valid".to_owned());
            out.conflict_all = ConflictAll::caConflict;
            out.conflict_this = ConflictThis::ctConflictLoses;
        }
    }
}

/// Draws the dialog and returns the shell action.
pub fn show(ctx: &egui::Context, state: &mut ModGroupSelectState) -> Option<ModGroupSelectAction> {
    let mut action = None;
    let mut open = true;
    let mut toggle_row = None;
    let mut double_row = None;
    let mut activate_row = None;
    let mut expand_row = None;
    let mut filter_focused = false;
    let mut window = egui::Window::new(if state.caption.is_empty() {
        "ModGroup Selection"
    } else {
        &state.caption
    })
    .id(egui::Id::unique("frmModGroupSelect"))
    .collapsible(false)
    .resizable(true)
    .default_size(crate::dialogs::selection::fit_size(ctx, egui::vec2(864.0, 615.0)))
    .open(&mut open);
    window = match state.placement {
        Some(placement) => placement.apply(ctx, window),
        None => window.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)),
    };
    let response = window.show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Filter:");
            let edit = ui.add(egui::TextEdit::singleline(&mut state.filter.text).desired_width(300.0));
            filter_focused = edit.has_focus();
            if edit.changed() {
                state.filter.recompile();
                state.filter_changed();
            }
            if state.presets_visible {
                ui.separator();
                ui.label("Preset:");
                egui::ComboBox::from_id_salt("modgroup-preset")
                    .width(200.0)
                    .selected_text(if state.preset.text.is_empty() {
                        "(none)"
                    } else {
                        &state.preset.text
                    })
                    .show_ui(ui, |ui| {
                        for item in state.preset.items.clone() {
                            ui.selectable_value(&mut state.preset.text, item.clone(), item);
                        }
                    });
                let selected = state.checked_groups().len();
                let can_load = state.preset.can_load();
                let save_reason = state.preset.save_reason(selected);
                let can_save = save_reason.is_none();
                let can_delete = state.preset.can_delete();
                if ui
                    .add_enabled(can_load, egui::Button::new("Load"))
                    .on_disabled_hover_text(state.preset.load_reason().unwrap_or_default())
                    .clicked()
                {
                    action = Some(ModGroupSelectAction::PresetLoad(state.preset.text.clone()));
                }
                if ui
                    .add_enabled(can_save, egui::Button::new("Save"))
                    .on_disabled_hover_text(save_reason.unwrap_or_default())
                    .clicked()
                {
                    let preset = state.preset.clean_name().unwrap_or_default();
                    if state.preset.items.contains(&preset) {
                        state.question = Some(ModGroupQuestion::OverwritePreset(preset));
                    } else {
                        action = Some(ModGroupSelectAction::PresetSave {
                            preset,
                            after_modal: false,
                        });
                    }
                }
                if ui
                    .add_enabled(can_delete, egui::Button::new("Delete"))
                    .on_disabled_hover_text(state.preset.delete_reason().unwrap_or_default())
                    .clicked()
                {
                    state.question = Some(ModGroupQuestion::DeletePreset(state.preset.text.clone()));
                }
            }
        });
        ui.separator();
        let list_height = (ui.available_height() - 34.0).max(80.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), list_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                let mut source = ModGroupTree::new(&state.rows);
                let output = TreeView {
                    id: ui.scope_id().with("modgroup-tree"),
                    source: &mut source,
                    state: &mut state.tree,
                    colors: &ConflictColors::default(),
                    header: true,
                }
                .show(ui);
                if let Some(row) = output.check_clicked {
                    toggle_row = Some(row);
                }
                if let Some(row) = output.double_clicked {
                    double_row = Some(row);
                }
                if let Some(row) = output.activated {
                    activate_row = Some(row);
                }
                if let Some(row) = output.expander_clicked {
                    expand_row = Some(row);
                }
                if let Some(TreeKeyAction::Toggle(row)) = output.key {
                    expand_row = Some(row);
                }
                if let Some(TreeKeyAction::Activate(row)) = output.key {
                    activate_row = Some(row);
                }
            },
        );
        if let Some(error) = &state.error {
            ui.colored_label(ui.visuals().error_fg_color, format!("Error: {error}"));
        } else {
            ui.label("");
        }
        ui.separator();
        ui.horizontal(|ui| {
            let enabled = state.error.is_none();
            if ui.add_enabled(enabled, egui::Button::new("OK")).clicked() {
                action = Some(ModGroupSelectAction::Ok(state.checked_groups()));
            }
            if ui.button("Cancel").clicked() {
                action = Some(ModGroupSelectAction::Cancelled);
            }
            ui.separator();
            if ui.button("Select All").clicked() {
                state.select_all(&state.tree.selected.clone());
            }
            if ui.button("Select None").clicked() {
                state.select_none(&state.tree.selected.clone());
            }
            if ui.button("Invert Selection").clicked() {
                state.invert_selection(&state.tree.selected.clone());
            }
            ui.label(format!("{} selected", state.checked_groups().len()));
        });
    });
    if let Some(response) = response {
        state.rect = Some(response.response.rect);
    }

    if let Some(row) = toggle_row
        && state.rows.get(row).is_some_and(|row| row.depth == 0)
    {
        let group = state.rows[row].group;
        state.toggle_group(group);
    }
    if let Some(row) = double_row {
        // `DoSingleModGroupLoad`.
        state.select_single(row);
        if state.error.is_none() {
            action = Some(ModGroupSelectAction::Ok(state.checked_groups()));
        }
    }
    if let Some(row) = expand_row.or(activate_row)
        && state.toggle_expanded(row)
    {
        state.tree.scroll_to = Some(row);
    }

    // `FormKeyDown`.
    if action.is_none() && state.question.is_none() && !filter_focused {
        let enter = ctx.input(|input| input.key_pressed(egui::Key::Enter));
        let ctrl = ctx.input(|input| input.modifiers.command);
        if enter {
            if ctrl || state.checked_groups().is_empty() {
                if let Some(row) = state.tree.focused {
                    state.select_single(row);
                }
                if state.error.is_none() {
                    action = Some(ModGroupSelectAction::Ok(state.checked_groups()));
                }
            } else if state.error.is_none() {
                action = Some(ModGroupSelectAction::Ok(state.checked_groups()));
            }
        } else if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            action = Some(ModGroupSelectAction::Cancelled);
        } else if crate::dialogs::selection::char_pressed(ctx, '*') {
            state.invert_selection(&state.tree.selected.clone());
        }
    }

    // The preset questions.
    if let Some(question) = state.question.clone() {
        let (text, default_yes) = match &question {
            ModGroupQuestion::DeletePreset(preset) => {
                (format!("Are you sure you want to delete preset \"{preset}\"?"), true)
            }
            ModGroupQuestion::OverwritePreset(preset) => (
                format!("Are you sure you want to overwrite existing preset \"{preset}\"?"),
                true,
            ),
            ModGroupQuestion::ChangedPreset(preset) => (
                format!(
                    "You have changed the selection after loading a preset. Do you want to update the preset \"{preset}\" with your new selection?"
                ),
                false,
            ),
        };
        match show_question(ctx, "modgroup-select-preset", &Question::new(text, default_yes)) {
            Some(true) => {
                state.question = None;
                action = Some(match question {
                    ModGroupQuestion::DeletePreset(preset) => ModGroupSelectAction::PresetDelete(preset),
                    ModGroupQuestion::OverwritePreset(preset) => ModGroupSelectAction::PresetSave {
                        preset,
                        after_modal: false,
                    },
                    ModGroupQuestion::ChangedPreset(preset) => ModGroupSelectAction::PresetSave {
                        preset,
                        after_modal: true,
                    },
                });
            }
            Some(false) => {
                state.question = None;
                if let ModGroupQuestion::ChangedPreset(_) = question {
                    action = Some(ModGroupSelectAction::Ok(state.checked_groups()));
                }
            }
            None => return None,
        }
    }

    if !open && action.is_none() {
        action = Some(ModGroupSelectAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, target: bool, source: bool, crcs: &[&str]) -> ItemInfo {
        ItemInfo {
            line: format!("[{target}{source}]{name}"),
            file_name: name.to_owned(),
            optional: false,
            target,
            source,
            forbidden: false,
            ignore_load_order: false,
            ignore_load_order_in_block: false,
            crc32s: crcs.iter().map(|crc| (*crc).to_owned()).collect(),
            load_order: Some(0),
            current_crc32: None,
            has_file: true,
            valid: true,
        }
    }

    fn group(name: &str, valid: bool, items: Vec<ItemInfo>) -> ModGroupInfo {
        ModGroupInfo {
            name: name.to_owned(),
            file: "Test.modgroups".to_owned(),
            valid,
            selected: false,
            messages: Vec::new(),
            items,
        }
    }

    fn state() -> ModGroupSelectState {
        let groups = vec![
            group(
                "Alpha",
                true,
                vec![item("A.esp", true, false, &[]), item("B.esp", false, true, &[])],
            ),
            group(
                "Beta",
                true,
                vec![item("C.esp", true, false, &[]), item("D.esp", false, true, &[])],
            ),
        ];
        ModGroupSelectState::new(
            groups,
            vec!["Mine".to_owned()],
            true,
            &["Alpha".to_owned()],
            "Reloading ModGroups - Which ModGroups do you want to activate?",
            true,
            0,
            usize::MAX,
        )
    }

    #[test]
    fn the_saved_selection_is_checked_and_the_presets_offer_last() {
        let state = state();
        assert_eq!(state.checked_groups(), vec!["Alpha"]);
        assert_eq!(state.preset.items, vec![CS_LAST.to_owned(), "Mine".to_owned()]);
        assert!(state.error.is_none());
        assert_eq!(state.rows.len(), 2);
        assert_eq!(state.rows[0].check, CheckState::Checked);
    }

    #[test]
    fn the_bulk_actions_follow_the_selected_or_all_rule() {
        let mut state = state();
        state.select_all(&[]);
        assert_eq!(state.checked_groups(), vec!["Alpha", "Beta"]);
        state.select_none(&[0, 1]);
        assert!(state.checked_groups().is_empty());
        state.invert_selection(&[1]);
        // One selected row is the "all" fallback.
        assert_eq!(state.checked_groups(), vec!["Alpha", "Beta"]);
    }

    #[test]
    fn the_filter_matches_group_names_only() {
        let mut state = state();
        state.filter.set_text("bet");
        state.filter_changed();
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].text, "Beta");
        state.filter.set_text("");
        state.filter_changed();
        assert_eq!(state.rows.len(), 2);
    }

    #[test]
    fn expanding_a_group_shows_its_items_and_crcs() {
        let mut state = state();
        state.toggle_expanded(0);
        assert_eq!(state.rows.len(), 4);
        assert_eq!(state.rows[1].text, "A.esp");
        assert!(!state.rows[1].target.is_empty());
        assert!(state.rows[1].disabled);
        // The item rows carry the checkbox state of the group's check.
        assert_eq!(state.rows[1].check, CheckState::Unchecked);
    }

    #[test]
    fn a_preset_load_checks_its_names_and_knows_when_it_differs() {
        let mut state = state();
        state.apply_preset("Mine", "Beta");
        assert_eq!(state.checked_groups(), vec!["Beta"]);
        assert!(!state.is_preset_different());
        state.toggle_group(0);
        assert!(state.is_preset_different());
        // The pseudo preset does not take part in the question.
        state.apply_preset(CS_LAST, "Alpha");
        assert!(!state.is_preset_different());
    }

    #[test]
    fn the_edit_flow_checks_nothing_and_lists_invalid_groups() {
        let groups = vec![group("Broken", false, vec![item("A.esp", true, true, &[])])];
        let mut state = ModGroupSelectState::for_edit(groups, "Which ModGroup do you want to edit?");
        assert!(state.checked_groups().is_empty());
        assert!(state.rows[0].invalid);
        assert!(!state.rows[0].disabled);
        assert_eq!(state.min_select, 1);
        assert_eq!(state.max_select, 1);
        state.select_all(&[]);
        assert_eq!(state.checked_groups(), vec!["Broken"]);
        // MaxSelect = 1: checking a second group is the dialog's error.
        state
            .groups
            .push(group("Second", true, vec![item("B.esp", true, true, &[])]));
        state.collect_selected();
        state.select_all(&[]);
        assert_eq!(state.error.as_deref(), Some("More than 1 ModGroup selected"));
    }

    #[test]
    fn the_crc_rows_are_checked_when_the_module_matches() {
        let groups = vec![group(
            "Alpha",
            true,
            vec![ItemInfo {
                current_crc32: Some("1A2B3C4D".to_owned()),
                ..item("A.esp", true, false, &["1A2B3C4D", "FFFFFFFF"])
            }],
        )];
        let mut state = ModGroupSelectState::new(groups, Vec::new(), false, &[], "Select", true, 0, usize::MAX);
        state.toggle_expanded(0);
        state.toggle_expanded(1);
        assert_eq!(state.rows.len(), 4);
        assert_eq!(state.rows[2].text, "1A2B3C4D");
        assert_eq!(state.rows[2].check, CheckState::Checked);
        assert_eq!(state.rows[3].check, CheckState::Unchecked);
    }
}
