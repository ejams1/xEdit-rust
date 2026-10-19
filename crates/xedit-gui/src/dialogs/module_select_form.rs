// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeModuleSelectForm.pas (FormCreate, FormShow,
// FormKeyDown, FormClose, edFilterChange, cbRegExFilterClick,
// vstModulesInitNode, vstModulesInitChildren, vstModulesGetText,
// vstModulesCheckState, vstModulesChecked, vstModulesHeaderClick,
// vstModulesCompareNodes, vstModulesPaintText, vstModulesNodeDblClick,
// vstModulesKeyDown, vstModulesKeyPress, mniSelectAllClick,
// mniSelectNoneClick, mniInvertSelectionClick, DoSingleModuleLoad,
// acPresetLoadUpdate, acPresetSaveUpdate, acPresetDeleteUpdate,
// acPresetLoadExecute, acPresetSaveExecute, acPresetDeleteExecute,
// btnOKClick, ShowModal) and xEdit/xeModuleSelectForm.dfm

//! `TfrmModuleSelect`: the module tree with its eight columns, the filter
//! with its regular expression mode, the selection actions, the presets and
//! the simulated load. The state machine (check states, simulation, presets,
//! filter) is `crate::startup::ModuleSelectState`; this module draws it and
//! returns what the shell must run as a command.

use eframe::egui;

use crate::dialogs::selection::{CheckState, Question, show_question};
use crate::startup::{MODULE_COLUMNS, ModuleRow, ModuleSelectQuestion, ModuleSelectState};
use crate::widgets::conflict::ConflictColors;
use crate::widgets::virtual_tree::{TreeCell, TreeColumn, TreeKeyAction, TreeRow, TreeSource, TreeView};
use xedit_core::interface::types::{ConflictAll, ConflictThis};

/// What the dialog asks the shell to do. None of these calls a command:
/// `main_form` runs `modules.preset_*` and the load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleSelectAction {
    /// `mrOk`: the plugins to load (`state.load_order()`, or the module
    /// name for the mod group editor's Insert).
    Ok,
    Cancelled,
    /// `PresetLoad` and `PresetDeleteExecute`; `PresetSaveExecute`.
    PresetLoad(String),
    PresetDelete(String),
    PresetSave {
        preset: String,
        after_modal: bool,
    },
    /// `PresetSave(LoadedPreset, True)` after OK: ask whether the changed
    /// selection updates the preset, then finish the OK.
    AskChangedPreset(String),
}

/// The tree of the dialog: the visible rows of the state.
struct ModuleTree<'a> {
    rows: &'a [ModuleRow],
    columns: Vec<TreeColumn>,
}

impl<'a> ModuleTree<'a> {
    fn new(rows: &'a [ModuleRow]) -> Self {
        Self {
            rows,
            columns: MODULE_COLUMNS
                .iter()
                .map(|(title, width)| TreeColumn::new(*title, *width))
                .collect(),
        }
    }
}

impl TreeSource for ModuleTree<'_> {
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
        // `vstModulesInitNode` gives the root children a check box; a
        // master row carries none (`ctNone`).
        out.icon = (!row.master).then(|| row.check.glyph());
        out.cells = vec![
            TreeCell::new(row.name.clone()),
            TreeCell::new(row.file_order.clone()),
            TreeCell::new(row.esm.clone()),
            TreeCell::new(row.light.clone()),
            TreeCell::new(row.load_order.clone()),
            TreeCell::new(row.file_id.clone()),
            TreeCell::new(row.update.clone()),
            TreeCell::new(row.medium.clone()),
        ];
        if row.ghost {
            out.cells[0].tooltip = Some("A .ghost file whose original is not in the data folder".to_owned());
        }
        if row.masters_missing {
            out.cells[0].tooltip = Some("A master of this module is missing".to_owned());
            out.conflict_all = ConflictAll::caConflict;
            out.conflict_this = ConflictThis::ctConflictLoses;
        } else if row.disabled {
            out.conflict_all = ConflictAll::caUnknown;
        }
    }
}

/// Draws the dialog; `main_form` runs the action and feeds the command's
/// response back into the state.
pub fn show(ctx: &egui::Context, state: &mut ModuleSelectState) -> Option<ModuleSelectAction> {
    let mut action = None;
    let mut open = true;
    let mut toggle_row: Option<usize> = None;
    let mut header_click: Option<usize> = None;
    let mut double_row: Option<usize> = None;
    let mut activate_row: Option<usize> = None;
    let mut expand_row: Option<usize> = None;
    let mut ask_ok = false;
    let mut single_load = false;
    let mut filter_focused = false;
    let mut preset_focused = false;
    let mut window = egui::Window::new("Module Selection")
        .id(egui::Id::unique("frmModuleSelect"))
        .collapsible(false)
        .resizable(true)
        .default_size(crate::dialogs::selection::fit_size(ctx, egui::vec2(898.0, 613.0)))
        .open(&mut open);
    window = match state.placement {
        Some(placement) => placement.apply(ctx, window),
        None => window.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)),
    };
    let response = window.show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Filter:");
            let edit = ui.add(egui::TextEdit::singleline(&mut state.filter.text).desired_width(280.0));
            filter_focused = edit.has_focus();
            if edit.changed() {
                state.filter.recompile();
                state.filter_changed();
            }
            let mut regex = state.filter.regex;
            if ui.checkbox(&mut regex, "RegEx").changed() {
                state.filter.set_regex(regex);
                state.filter_changed();
            }
            if let Some(reason) = state.filter.invalid_reason() {
                ui.colored_label(ui.visuals().warn_fg_color, reason);
            }
            if state.presets_available {
                ui.separator();
                ui.label("Preset:");
                let combo = egui::ComboBox::from_id_salt("module-preset")
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
                preset_focused = combo.response.has_focus();
                let selected = state.checked_names().len();
                let can_load = state.preset.can_load();
                let save_reason = state.preset.save_reason(selected);
                let can_save = save_reason.is_none();
                let can_delete = state.preset.can_delete();
                if ui
                    .add_enabled(can_load, egui::Button::new("Load"))
                    .on_disabled_hover_text(state.preset.load_reason().unwrap_or_default())
                    .clicked()
                {
                    action = Some(ModuleSelectAction::PresetLoad(state.preset.text.clone()));
                }
                if ui
                    .add_enabled(can_save, egui::Button::new("Save"))
                    .on_disabled_hover_text(save_reason.unwrap_or_default())
                    .clicked()
                {
                    let preset = state.preset.clean_name().unwrap_or_default();
                    if state.preset.items.contains(&preset) {
                        state.question = Some(ModuleSelectQuestion::OverwritePreset(preset));
                    } else {
                        action = Some(ModuleSelectAction::PresetSave {
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
                    state.question = Some(ModuleSelectQuestion::DeletePreset(state.preset.text.clone()));
                }
            }
        });
        ui.separator();
        let list_height = (ui.available_height() - 34.0).max(80.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), list_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                let mut source = ModuleTree::new(&state.rows);
                let output = TreeView {
                    id: ui.scope_id().with("module-tree"),
                    source: &mut source,
                    state: &mut state.tree,
                    colors: &ConflictColors::default(),
                    header: true,
                }
                .show(ui);
                header_click = output.header_clicked;
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
            // `btnOK.Enabled := Error = ''`.
            let enabled = state.error.is_none();
            if ui.add_enabled(enabled, egui::Button::new("OK")).clicked() {
                ask_ok = true;
            }
            if ui.button("Cancel").clicked() {
                action = Some(ModuleSelectAction::Cancelled);
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
            ui.label(format!("{} selected", state.checked_names().len()));
        });
    });
    if let Some(response) = response {
        state.rect = Some(response.response.rect);
    }

    // `vstModulesHeaderClick`.
    if let Some(column) = header_click {
        state.header_click(column);
    }
    if let Some(row) = toggle_row {
        state.toggle_row(row);
    }
    if let Some(row) = double_row {
        // A double click on a module row checks it and confirms.
        if state.rows.get(row).is_some_and(|row| !row.master) {
            state.select_single(Some(row), &state.tree.selected.clone());
            if state.error.is_none() {
                action = Some(ModuleSelectAction::Ok);
            }
        } else {
            state.toggle_expanded(row);
        }
    }
    if let Some(row) = expand_row.or(activate_row) {
        // `vstModulesInitChildren`: the expander and Enter show a module's
        // masters.
        if state.toggle_expanded(row) {
            state.tree.scroll_to = Some(row);
        }
    }

    // `FormKeyDown`.
    let enter = ctx.input(|input| input.key_pressed(egui::Key::Enter));
    let ctrl = ctx.input(|input| input.modifiers.command);
    if action.is_none() && enter && !filter_focused && !preset_focused && !state.question.is_some() {
        if ctrl || state.checked_names().is_empty() {
            single_load = true;
        } else if state.error.is_none() {
            ask_ok = true;
        }
    }
    if action.is_none() && !filter_focused && !preset_focused {
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            action = Some(ModuleSelectAction::Cancelled);
        } else if crate::dialogs::selection::char_pressed(ctx, '*') {
            // `VK_MULTIPLY`.
            state.invert_selection(&state.tree.selected.clone());
        }
    }
    // `edFilterKeyDown`: Down moves the focus to the first visible row; the
    // port leaves the text box when the tree is clicked, as egui does.
    if single_load {
        state.select_single(state.tree.focused, &state.tree.selected.clone());
        if state.error.is_none() {
            action = Some(ModuleSelectAction::Ok);
        }
    }

    // The preset questions (`PresetDelete` and `PresetSave`'s overwrite
    // question).
    if let Some(question) = state.question.clone() {
        let (text, default_yes) = match &question {
            ModuleSelectQuestion::DeletePreset(preset) => {
                (format!("Are you sure you want to delete preset \"{preset}\"?"), true)
            }
            ModuleSelectQuestion::OverwritePreset(preset) => (
                format!("Are you sure you want to overwrite existing preset \"{preset}\"?"),
                true,
            ),
            ModuleSelectQuestion::ChangedPreset(preset) => (
                format!(
                    "You have changed the selection after loading a preset. Do you want to update the preset \"{preset}\" with your new selection?"
                ),
                false,
            ),
        };
        match show_question(ctx, "module-select-preset", &Question::new(text, default_yes)) {
            Some(true) => {
                state.question = None;
                action = Some(match question {
                    ModuleSelectQuestion::DeletePreset(preset) => ModuleSelectAction::PresetDelete(preset),
                    ModuleSelectQuestion::OverwritePreset(preset) => ModuleSelectAction::PresetSave {
                        preset,
                        after_modal: false,
                    },
                    ModuleSelectQuestion::ChangedPreset(preset) => ModuleSelectAction::PresetSave {
                        preset,
                        after_modal: true,
                    },
                });
            }
            Some(false) => {
                state.question = None;
                if let ModuleSelectQuestion::ChangedPreset(_) = question {
                    // The answer "No" still ends the close that asked it.
                    action = Some(ModuleSelectAction::Ok);
                }
            }
            None => {
                // The question owns the keyboard while it is open.
                return None;
            }
        }
    }

    if ask_ok {
        // `ShowModal`: `btnOKClick` loads the focused module when nothing is
        // checked.
        if state.checked_names().is_empty() {
            state.select_single(state.tree.focused, &state.tree.selected.clone());
        }
        if state.error.is_none() {
            if !state.loaded_preset.is_empty() {
                action = Some(ModuleSelectAction::AskChangedPreset(state.loaded_preset.clone()));
            } else {
                action = Some(ModuleSelectAction::Ok);
            }
        }
    }
    if !open && action.is_none() {
        action = Some(ModuleSelectAction::Cancelled);
    }
    action
}

/// The columns of the tree, for the tests of the adapter.
pub fn columns() -> Vec<TreeColumn> {
    MODULE_COLUMNS
        .iter()
        .map(|(title, width)| TreeColumn::new(*title, *width))
        .collect()
}

/// The check glyph of a state, for the tests.
pub fn glyph(state: CheckState) -> char {
    state.glyph()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::{ModuleNode, ModuleSelectMode};

    fn node(node: usize, name: &str) -> ModuleNode {
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
            valid: true,
            loaded: false,
            masters_missing: false,
            active: true,
            active_in_plugins_txt: true,
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

    fn state() -> ModuleSelectState {
        ModuleSelectState::from_nodes(
            "C:\\Data\\".to_owned(),
            vec![node(0, "Skyrim.esm"), node(1, "Mod.esp")],
            ModuleSelectMode::Active,
            &[],
        )
    }

    #[test]
    fn the_tree_rows_carry_the_check_state_and_columns() {
        let state = state();
        let tree = ModuleTree::new(&state.rows);
        let mut row = TreeRow::default();
        TreeSource::row(&tree, 0, &mut row);
        assert_eq!(row.icon, Some('\u{2611}'));
        assert_eq!(row.cells[0].text, "Skyrim.esm");
        assert_eq!(row.cells[2].text, "ESM");
        assert_eq!(row.cells[5].text, "00");
        // A master row has no check box.
        let mut source = state;
        source.modules[1].master_names = vec!["Skyrim.esm".to_owned()];
        source.modules[1].masters = vec![Some(0)];
        source.modules[1].expanded = true;
        source.refresh_rows();
        let tree = ModuleTree::new(&source.rows);
        TreeSource::row(&tree, 2, &mut row);
        assert_eq!(row.icon, None);
        assert_eq!(row.depth, 1);
        assert_eq!(row.cells[0].text, "Skyrim.esm");
    }

    #[test]
    fn the_glyph_of_a_mixed_check_is_the_port_marker() {
        assert_eq!(glyph(CheckState::Checked), '\u{2611}');
        assert_eq!(glyph(CheckState::Mixed), '\u{25A3}');
        assert_eq!(glyph(CheckState::Unchecked), '\u{2610}');
    }

    #[test]
    fn the_columns_are_the_dfm_ones() {
        let columns = columns();
        assert_eq!(columns.len(), 8);
        assert_eq!(columns[5].title, "FormID Prefix");
        assert_eq!(columns[6].title, "Update");
    }
}
