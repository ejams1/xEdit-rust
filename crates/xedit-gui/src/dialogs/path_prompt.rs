// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (the `odModule` and `odCSV` open
// dialogs of mniNavCompareToClick, mniNavCreateDeltaPatchClick and
// mniNavBatchChangeReferencingRecordsClick)

//! The stand-in for the standard open dialog (`TOpenDialog`) of the three
//! navigation items that ask for a file of the disk: "Compare to...", the
//! newer version of a delta patch and the CSV file of a batch reference
//! change.
//!
//! UPSTREAM-QUIRK: upstream opens the Windows common file dialog, which
//! lets the user walk the whole disk. The port has no native file dialog,
//! so it shows the title of the dialog, a text box with the path and a
//! list of the files of the folder the dialog starts in; a path that is
//! typed in full is accepted as well, so every file of the disk can still
//! be reached.

use eframe::egui;

use crate::dialogs::selection::DialogPlacement;

/// What the dialog answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathPromptAction {
    /// `Execute`: the path of the chosen file.
    Ok(String),
    /// `Cancel`.
    Cancelled,
}

/// The open dialog as data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathPromptState {
    /// `odModule.Title` (the title of the open dialog).
    pub caption: String,
    /// The line under the title, which names the file the item acts on.
    pub message: String,
    /// `odModule.InitialDir`.
    pub initial_dir: String,
    /// `odModule.DefaultExt`: the extension the list offers.
    pub extension: String,
    /// The files of `initial_dir` the list offers.
    pub candidates: Vec<String>,
    /// The `File name` box of the open dialog.
    pub value: String,
    /// The error of the last answer.
    pub error: Option<String>,
    pub placement: Option<DialogPlacement>,
    pub rect: Option<egui::Rect>,
}

impl PathPromptState {
    /// A dialog over a title and a folder; the candidates are the files of
    /// the folder that carry the extension.
    pub fn new(
        caption: impl Into<String>,
        message: impl Into<String>,
        initial_dir: impl Into<String>,
        extension: impl Into<String>,
    ) -> Self {
        Self {
            caption: caption.into(),
            message: message.into(),
            initial_dir: initial_dir.into(),
            extension: extension.into(),
            candidates: Vec::new(),
            value: String::new(),
            error: None,
            placement: None,
            rect: None,
        }
    }

    /// `odModule` fills the list with the files of the folder that carry
    /// the extension.
    pub fn set_candidates(&mut self, candidates: Vec<String>) {
        self.candidates = candidates;
    }

    /// The path the box answers: the text of the `File name` box, which is
    /// the full path when the user typed one.
    pub fn path(&self) -> String {
        let value = self.value.trim();
        if value.is_empty() {
            String::new()
        } else if value.contains('\\') || value.contains('/') || value.contains(':') {
            value.to_owned()
        } else {
            format!("{}{}", self.initial_dir, value)
        }
    }
}

/// Draws the dialog.
pub fn show(ctx: &egui::Context, state: &mut PathPromptState) -> Option<PathPromptAction> {
    let mut action = None;
    let mut open = true;
    let mut focused = false;
    let mut chosen = None;
    let mut window = egui::Window::new(if state.caption.is_empty() {
        "Open"
    } else {
        &state.caption
    })
    .id(egui::Id::unique("frmOpenDialog"))
    .collapsible(false)
    .resizable(true)
    .default_size(crate::dialogs::selection::fit_size(ctx, egui::vec2(520.0, 420.0)))
    .open(&mut open);
    window = match state.placement {
        Some(placement) => placement.apply(ctx, window),
        None => window.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)),
    };
    let response = window.show(ctx, |ui| {
        if !state.message.is_empty() {
            ui.label(&state.message);
        }
        ui.label(format!("Look in: {}", state.initial_dir));
        let height = (ui.available_height() - 90.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("open-dialog-files")
            .max_height(height)
            .show(ui, |ui| {
                for candidate in &state.candidates {
                    if ui.selectable_label(false, candidate).clicked() {
                        chosen = Some(candidate.clone());
                    }
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("File name:");
            let edit = ui.add(egui::TextEdit::singleline(&mut state.value).desired_width(280.0));
            focused = edit.has_focus();
        });
        if let Some(error) = &state.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.horizontal(|ui| {
            if ui.button("Open").clicked() {
                if state.path().is_empty() {
                    state.error = Some("Please select a file".to_owned());
                } else {
                    action = Some(PathPromptAction::Ok(state.path()));
                }
            }
            if ui.button("Cancel").clicked() {
                action = Some(PathPromptAction::Cancelled);
            }
        });
    });
    if let Some(response) = response {
        state.rect = Some(response.response.rect);
    }
    if let Some(chosen) = chosen {
        state.value = chosen;
    }
    if action.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        action = Some(PathPromptAction::Cancelled);
    }
    if !focused && action.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
        if state.path().is_empty() {
            state.error = Some("Please select a file".to_owned());
        } else {
            action = Some(PathPromptAction::Ok(state.path()));
        }
    }
    if !open && action.is_none() {
        action = Some(PathPromptAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_without_a_path_is_taken_from_the_folder() {
        let mut state = PathPromptState::new("Open", "", "C:\\Data\\", ".esp");
        state.value = "Fallout4.esp".to_owned();
        assert_eq!(state.path(), "C:\\Data\\Fallout4.esp");
    }

    #[test]
    fn a_full_path_is_kept() {
        let mut state = PathPromptState::new("Open", "", "C:\\Data\\", ".esp");
        state.value = "D:\\Other\\Thing.esp".to_owned();
        assert_eq!(state.path(), "D:\\Other\\Thing.esp");
    }

    #[test]
    fn an_empty_box_has_no_path() {
        let state = PathPromptState::new("Open", "", "C:\\Data\\", ".esp");
        assert_eq!(state.path(), "");
    }
}
