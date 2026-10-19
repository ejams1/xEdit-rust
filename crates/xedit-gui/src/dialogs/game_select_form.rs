// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeGameSelectForm.pas (FormCreate, FormKeyDown,
// edSearchChange, ListBox1DblClick, TListBox.DrawItem) and
// xEdit/xeGameSelectForm.dfm (frmGameSelect, Label1, Label2, ListBox1,
// edSearch, btnOK)

//! `TfrmGameSelect`: the modes the executable does not name, picked from a
//! list. The shell shows it at startup when no game mode was given on the
//! command line (step 5 finishes it; this version is the form the startup
//! flow needs).

use eframe::egui;

use crate::dialogs::selection::row_matches;
use crate::startup::{GAME_MODES, GameSelectState};
use xedit_core::interface::globals::GameMode;

/// What the dialog asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameSelectAction {
    Chosen(GameMode),
    Cancelled,
}

/// Draws the dialog. `state` keeps the search text and the selection, and
/// `error` the reason a choice was refused (a mode the port cannot load).
pub fn show(ctx: &egui::Context, state: &mut GameSelectState, error: &mut Option<String>) -> Option<GameSelectAction> {
    let mut action = None;
    let mut open = true;
    let mut search_focused = false;
    let mut window = egui::Window::new("Select Game Mode")
        .id(egui::Id::unique("frmGameSelect"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open);
    window = match state.placement {
        Some(placement) => placement.apply(ctx, window),
        None => window.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)),
    };
    let response = window.show(ctx, |ui| {
        ui.set_width(360.0);
        ui.label("Search");
        let response = ui.text_edit_singleline(&mut state.search);
        search_focused = response.has_focus();
        if response.changed() {
            state.search_changed();
            *error = None;
        }
        ui.add_space(4.0);
        ui.label("Select the game mode to start with:");
        let visual = ui.visuals().clone();
        let search = state.search.clone();
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for (index, (tag, mode, supported)) in GAME_MODES.iter().enumerate() {
                let selected = state.selected == Some(index);
                let matches = row_matches(tag, &search);
                let mut text = egui::RichText::new(*tag);
                if !supported {
                    text = text.weak();
                }
                let button = egui::Button::new(text).selected(selected);
                let response = ui.add_sized([ui.available_width(), 20.0], button);
                if matches && !selected {
                    // `TListBox.DrawItem`: a matching row is highlighted.
                    ui.painter().rect_filled(
                        response.rect,
                        egui::CornerRadius::ZERO,
                        visual.selection.bg_fill.gamma_multiply(0.25),
                    );
                }
                if *supported {
                    response.clone().on_hover_text(mode_tag(*mode));
                } else {
                    response
                        .clone()
                        .on_hover_text("Oblivion Remastered: the definitions are not ported (issue #80)");
                }
                if response.clicked() {
                    state.selected = Some(index);
                    *error = None;
                }
                if response.double_clicked() {
                    state.selected = Some(index);
                    match state.confirm() {
                        Ok(mode) => action = Some(GameSelectAction::Chosen(mode)),
                        Err(refusal) => *error = Some(refusal.message),
                    }
                }
            }
        });
        if let Some(message) = error.as_ref() {
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                match state.confirm() {
                    Ok(mode) => action = Some(GameSelectAction::Chosen(mode)),
                    Err(refusal) => *error = Some(refusal.message),
                }
            }
            if ui.button("Cancel").clicked() {
                action = Some(GameSelectAction::Cancelled);
            }
        });
    });
    if let Some(response) = response {
        state.rect = Some(response.response.rect);
    }
    // `FormKeyDown`: Enter is OK, Escape cancels unless `NoEscape`, and
    // neither while the search box has the focus (`edSearch.Focused`).
    let typing = search_focused;
    if action.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Escape)) && !typing && !state.no_escape {
        action = Some(GameSelectAction::Cancelled);
    }
    if !typing && ctx.input(|input| input.key_pressed(egui::Key::Enter)) && action.is_none() {
        match state.confirm() {
            Ok(mode) => action = Some(GameSelectAction::Chosen(mode)),
            Err(refusal) => *error = Some(refusal.message),
        }
    }
    if !open && action.is_none() {
        action = Some(GameSelectAction::Cancelled);
    }
    action
}

/// The game mode name for a tooltip (`wbGameName2`).
fn mode_tag(mode: GameMode) -> String {
    xedit_session::game_paths::game_name2(mode).to_owned()
}
