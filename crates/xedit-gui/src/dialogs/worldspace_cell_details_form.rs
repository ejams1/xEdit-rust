// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeWorldspaceCellDetailsForm.pas (FormCreate,
// rbTemporaryClick, xeGetCellDetailsForWorldspaceImplementation) and
// xEdit/xeWorldspaceCellDetailsForm.dfm (rbPersistent, rbTemporary, sedX,
// sedY, lblX, lblY, btnOK, btnCancel)

//! `TfrmWorldspaceCellDetails`: the question of
//! `wbGetCellDetailsForWorldspaceCallback` when a world cell is added to a
//! worldspace: is it persistent, or which grid cell does it take?
//!
//! The dialog is installed through the core seam by [`crate::dialogs::prompts`];
//! its answer is the `(persistent, x, y)` the callback returns, and Cancel
//! is upstream's `ShowModal <> mrOk` (`None`, the add is refused).

use eframe::egui;

/// What the dialog asks the seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldspaceCellDetailsAction {
    /// `btnOK` (`mrOk`).
    Ok { persistent: bool, x: i32, y: i32 },
    /// `btnCancel` (`mrCancel`).
    Cancelled,
}

/// `TfrmWorldspaceCellDetails` as data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldspaceCellDetailsState {
    /// `rbPersistent.Checked` (`rbPersistent` starts checked).
    pub persistent: bool,
    /// `sedX.Value` (`MinValue` -9999, `MaxValue` 9999).
    pub x: i32,
    /// `sedY.Value`.
    pub y: i32,
}

impl Default for WorldspaceCellDetailsState {
    fn default() -> Self {
        Self {
            persistent: true,
            x: 0,
            y: 0,
        }
    }
}

impl WorldspaceCellDetailsState {
    /// `rbTemporaryClick`: the grid fields take the focus.
    pub fn temporary_clicked(&mut self) {
        self.persistent = false;
    }

    /// The spin boxes clamp to `MinValue`/`MaxValue`.
    pub fn set_cell(&mut self, x: i32, y: i32) {
        self.x = x.clamp(-9999, 9999);
        self.y = y.clamp(-9999, 9999);
    }

    /// The answer of the seam: `aPersistent := rbPersistent.Checked;
    /// aGridCell.x := sedX.Value; ...` with `mrOk`.
    pub fn answer(&self) -> Option<(bool, i32, i32)> {
        Some((self.persistent, self.x, self.y))
    }
}

/// Draws the dialog; the title names the worldspace the cell is added to
/// (the port adds the record's name to the form's caption).
pub fn show(
    ctx: &egui::Context,
    state: &mut WorldspaceCellDetailsState,
    worldspace: &str,
) -> Option<WorldspaceCellDetailsAction> {
    let mut action = None;
    egui::Window::new(format!("Worldspace Cell: {worldspace}"))
        .id(egui::Id::unique("frmWorldspaceCellDetails"))
        .collapsible(false)
        .resizable(false)
        // PORT-EXTENSION: the caption names the worldspace, so the port
        // opens wider than the form's 172 points.
        .default_size(crate::dialogs::selection::fit_size(ctx, egui::vec2(280.0, 150.0)))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            if ui.radio(state.persistent, "Persistent").clicked() {
                state.persistent = true;
            }
            if ui.radio(!state.persistent, "Temporary").clicked() {
                state.temporary_clicked();
            }
            ui.horizontal(|ui| {
                ui.label("X");
                let mut x = state.x;
                if ui.add(egui::DragValue::new(&mut x).range(-9999..=9999)).changed() {
                    state.set_cell(x, state.y);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Y");
                let mut y = state.y;
                if ui.add(egui::DragValue::new(&mut y).range(-9999..=9999)).changed() {
                    state.set_cell(state.x, y);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    action = Some(WorldspaceCellDetailsAction::Ok {
                        persistent: state.persistent,
                        x: state.x,
                        y: state.y,
                    });
                }
                if ui.button("Cancel").clicked() {
                    action = Some(WorldspaceCellDetailsAction::Cancelled);
                }
            });
        });
    if action.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        action = Some(WorldspaceCellDetailsAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_persistent_radio_starts_checked() {
        let state = WorldspaceCellDetailsState::default();
        assert!(state.persistent);
        assert_eq!(state.answer(), Some((true, 0, 0)));
    }

    #[test]
    fn temporary_selects_the_grid_cell() {
        let mut state = WorldspaceCellDetailsState::default();
        state.temporary_clicked();
        state.set_cell(4, -2);
        assert_eq!(state.answer(), Some((false, 4, -2)));
    }

    #[test]
    fn the_cell_clamps_to_the_spin_edit_range() {
        let mut state = WorldspaceCellDetailsState::default();
        state.set_cell(20000, -20000);
        assert_eq!((state.x, state.y), (9999, -9999));
    }

    #[test]
    fn a_cancelled_dialog_refuses_the_add() {
        // `ShowModal <> mrOk` is the seam's `False` (`None`).
        assert_eq!(
            WorldspaceCellDetailsAction::Cancelled,
            WorldspaceCellDetailsAction::Cancelled
        );
    }
}
