// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeModuleSelectForm.pas (FormCreate, FormShow,
// vstModulesInitNode, vstModulesGetText, vstModulesCheckState,
// vstModulesNodeDblClick, vstModulesKeyDown, edFilterChange,
// mniSelectAllClick, mniSelectNoneClick, mniInvertSelectionClick,
// btnOKClick) and xEdit/xeModuleSelectForm.dfm (frmModuleSelect,
// vstModules with the Filename / File Order / ESM / Light / Load Order
// columns, edFilter, btnOK, the Cancel button, the selection menu items)

//! `TfrmModuleSelect` at startup: the modules of the data folder, the
//! active ones checked, with the filter and the bulk selection actions.
//! Step 5 finishes the dialog (the presets, the master display and the
//! scripted selection); the check state and `SimulateLoad` are here.

use eframe::egui;

use crate::startup::{ModuleRow, ModuleSelectState};
use crate::widgets::conflict::ConflictColors;
use crate::widgets::virtual_tree::{TreeCell, TreeColumn, TreeRow, TreeSource, TreeState, TreeView};
use xedit_core::interface::types::{ConflictAll, ConflictThis};

/// What the dialog asks the shell to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleSelectAction {
    /// `mrOk`: the plugins to load (`SimulateLoad`), as full paths.
    Ok(Vec<String>),
    Cancelled,
}

/// The module list as a [`TreeSource`]: a snapshot of the rows of the
/// current filter.
struct ModuleTree {
    rows: Vec<ModuleRow>,
    columns: Vec<TreeColumn>,
    selected: Option<usize>,
}

impl ModuleTree {
    fn new(state: &ModuleSelectState, selected: Option<usize>) -> Self {
        Self {
            rows: state.rows(),
            columns: vec![
                TreeColumn::new("Filename", 300.0),
                TreeColumn::new("File Order", 200.0),
                TreeColumn::new("ESM", 50.0),
                TreeColumn::new("Light", 50.0),
                TreeColumn::new("Load Order", 75.0),
            ],
            selected,
        }
    }
}

impl TreeSource for ModuleTree {
    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn columns(&self) -> &[TreeColumn] {
        &self.columns
    }

    fn row(&self, index: usize, out: &mut TreeRow) {
        let Some(module) = self.rows.get(index) else {
            return;
        };
        out.id = module.index as u64;
        out.depth = 0;
        out.icon = Some(if module.checked { '\u{2611}' } else { '\u{2610}' });
        let esm = if module.is_esm { "X" } else { "" };
        let light = if module.is_light { "X" } else { "" };
        out.cells = vec![
            TreeCell::new(module.name.clone()),
            TreeCell::new(module.file_order.clone()),
            TreeCell::new(esm),
            TreeCell::new(light),
            TreeCell::new(module.load_order.clone()),
        ];
        if module.masters_missing {
            out.cells[0].tooltip = Some("A master of this module is missing".to_owned());
            out.conflict_all = ConflictAll::caConflict;
            out.conflict_this = ConflictThis::ctConflictLoses;
        } else if self.selected == Some(index) {
            out.conflict_all = ConflictAll::caOverride;
        }
    }
}

/// Draws the dialog and returns the load list on OK.
pub fn show(ctx: &egui::Context, state: &mut ModuleSelectState, tree: &mut TreeState) -> Option<ModuleSelectAction> {
    let mut action = None;
    let mut open = true;
    let mut toggle_row: Option<usize> = None;
    egui::Window::new("Module Selection")
        .id(egui::Id::unique("frmModuleSelect"))
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(900.0, 600.0))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Filter:");
                if ui.text_edit_singleline(&mut state.filter).changed() {
                    state.error = None;
                }
                let mut regex = false;
                ui.add_enabled(false, egui::Checkbox::new(&mut regex, "RegEx"))
                    .on_disabled_hover_text(
                        "The port's module filter is the substring match (step 5 adds the regular expression)",
                    );
                ui.separator();
                if ui.button("Select All").clicked() {
                    state.select_all();
                }
                if ui.button("Select None").clicked() {
                    state.select_none();
                }
                if ui.button("Invert Selection").clicked() {
                    state.invert_selection();
                }
                ui.label(format!("{} selected", state.checked_names().len()));
            });
            ui.separator();
            // The list fills all the room above the buttons.
            let list_height = (ui.available_height() - 34.0).max(80.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), list_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let mut source = ModuleTree::new(state, tree.focused);
                    let output = TreeView {
                        id: ui.scope_id().with("module-tree"),
                        source: &mut source,
                        state: tree,
                        colors: &ConflictColors::default(),
                        header: true,
                    }
                    .show(ui);
                    if let Some(index) = output.double_clicked {
                        toggle_row = Some(index);
                    }
                    if let (Some(index), Some(crate::widgets::virtual_tree::TreeKeyAction::Activate(_))) =
                        (tree.focused, output.key)
                    {
                        toggle_row = Some(index);
                    }
                },
            );
            if let Some(message) = state.error.as_ref() {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    match state.load_order() {
                        Ok(plugins) if !plugins.is_empty() => action = Some(ModuleSelectAction::Ok(plugins)),
                        Ok(_) => state.error = Some("There are no modules to load.".to_owned()),
                        Err(message) => state.error = Some(message),
                    }
                }
                if ui.button("Cancel").clicked() {
                    action = Some(ModuleSelectAction::Cancelled);
                }
            });
        });
    // `vstModulesNodeDblClick` and `vstModulesKeyDown`: a double click or
    // Enter toggles the checked state of the row.
    if let Some(index) = toggle_row
        && let Some(row) = state.rows().get(index)
    {
        let module = row.index;
        let checked = state.checked.get(module).copied().unwrap_or(false);
        state.set_checked(module, !checked);
    }
    if !open && action.is_none() {
        action = Some(ModuleSelectAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_rows_carry_the_check_state() {
        // The adapter is only exercised through a real dialog; the row
        // mapping is the part the state machine tests cannot see.
        let tree = ModuleTree {
            rows: vec![ModuleRow {
                index: 3,
                name: "Update.esm".to_owned(),
                file_order: "2".to_owned(),
                is_esm: true,
                is_light: false,
                load_order: "3".to_owned(),
                masters_missing: false,
                checked: true,
            }],
            columns: Vec::new(),
            selected: None,
        };
        let mut row = TreeRow::default();
        TreeSource::row(&tree, 0, &mut row);
        assert_eq!(row.icon, Some('\u{2611}'));
        assert_eq!(row.cells[0].text, "Update.esm");
        assert_eq!(row.cells[2].text, "X");
    }
}
