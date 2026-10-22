// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeLocalizePluginForm.pas (FormCreate,
// cbTranslationClick, FormClose) and xEdit/xeLocalizePluginForm.dfm
// (Label1, gbTranslation with clbFrom and clbTo, cbTranslation, btnOK
// "Localize", btnCancel)

//! `TfrmLocalizePlugin`: the Localize confirmation and the translation
//! pair lists of `mniNavLocalizationSwitchClick`; the action is
//! `localization.localize`.
//!
//! UPSTREAM-QUIRK: the release's DFM hides `cbTranslation` and
//! `gbTranslation` (`Visible = False`), and the code that fills the lists
//! lives in the main form, so 4.1.5q never reaches the translation pair
//! checks of `FormClose` (`:61-82`). The port draws the check box and the
//! lists so that documented path is reachable.

use eframe::egui;

/// What the dialog asks the shell to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalizePluginAction {
    /// `localization.localize` with the translation pairs.
    Localize {
        translate_from: Vec<String>,
        translate_to: Vec<String>,
    },
    /// `btnCancel`.
    Cancelled,
}

/// `TfrmLocalizePlugin`'s state.
#[derive(Clone, Debug, Default)]
pub struct LocalizePluginState {
    /// The file names `localization.languages` lists (`clbFrom`/`clbTo`).
    pub tables: Vec<String>,
    /// `clbFrom.Checked`.
    pub from_checked: Vec<bool>,
    /// `clbTo.Checked`.
    pub to_checked: Vec<bool>,
    /// `cbTranslation.Checked`.
    pub translation: bool,
    /// The `MessageBox` of `FormClose` (`Action := caNone` keeps the form
    /// open).
    pub error: Option<String>,
    /// The text `Label1` shows.
    pub label: String,
}

impl LocalizePluginState {
    /// The dialog with the translation tables `localization.languages`
    /// returned.
    pub fn new(tables: Vec<String>) -> Self {
        Self {
            from_checked: vec![false; tables.len()],
            to_checked: vec![false; tables.len()],
            tables,
            translation: false,
            error: None,
            label: "Do you want to localize this plugin?".to_owned(),
        }
    }

    /// `FormClose`'s checks: no table in both lists, and as many from as to.
    pub fn confirm(&mut self) -> Option<LocalizePluginAction> {
        if !self.translation {
            return Some(LocalizePluginAction::Localize {
                translate_from: Vec::new(),
                translate_to: Vec::new(),
            });
        }
        let mut balance: i32 = 0;
        for index in 0..self.tables.len() {
            let from = self.from_checked.get(index).copied().unwrap_or(false);
            let to = self.to_checked.get(index).copied().unwrap_or(false);
            if from && to {
                self.error = Some("Translation files should not match".to_owned());
                return None;
            }
            if from {
                balance += 1;
            }
            if to {
                balance -= 1;
            }
        }
        if balance != 0 {
            self.error = Some("Translation files should come in pairs".to_owned());
            return None;
        }
        Some(LocalizePluginAction::Localize {
            translate_from: self
                .tables
                .iter()
                .zip(&self.from_checked)
                .filter(|(_, checked)| **checked)
                .map(|(table, _)| table.clone())
                .collect(),
            translate_to: self
                .tables
                .iter()
                .zip(&self.to_checked)
                .filter(|(_, checked)| **checked)
                .map(|(table, _)| table.clone())
                .collect(),
        })
    }
}

/// Draws the dialog.
pub fn show(ctx: &egui::Context, state: &mut LocalizePluginState) -> Option<LocalizePluginAction> {
    let mut action = None;
    let mut open = true;
    egui::Window::new("Localize plugin")
        .id(egui::Id::unique("frmLocalizePlugin"))
        .collapsible(false)
        .resizable(false)
        .default_size(egui::vec2(600.0, 380.0))
        .default_pos(egui::pos2(60.0, 60.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(&state.label);
            if ui.checkbox(&mut state.translation, "Translation").changed() {
                state.error = None;
            }
            ui.add_enabled_ui(state.translation, |ui| {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label("From language");
                            for index in 0..state.tables.len() {
                                let table = state.tables[index].clone();
                                let mut checked = state.from_checked[index];
                                if ui.checkbox(&mut checked, table).changed() {
                                    state.from_checked[index] = checked;
                                    state.error = None;
                                }
                            }
                        });
                        ui.vertical(|ui| {
                            ui.label("To language");
                            for index in 0..state.tables.len() {
                                let table = state.tables[index].clone();
                                let mut checked = state.to_checked[index];
                                if ui.checkbox(&mut checked, table).changed() {
                                    state.to_checked[index] = checked;
                                    state.error = None;
                                }
                            }
                        });
                    });
                });
            });
            if let Some(message) = state.error.clone() {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Localize").clicked() {
                    action = state.confirm();
                }
                if ui.button("Cancel").clicked() {
                    action = Some(LocalizePluginAction::Cancelled);
                }
            });
        });
    if !open && action.is_none() && state.error.is_none() {
        action = Some(LocalizePluginAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_translation_the_dialog_localizes_without_pairs() {
        let mut state = LocalizePluginState::new(vec!["A_English.STRINGS".to_owned()]);
        assert_eq!(
            state.confirm(),
            Some(LocalizePluginAction::Localize {
                translate_from: Vec::new(),
                translate_to: Vec::new()
            })
        );
    }

    #[test]
    fn a_table_in_both_lists_is_refused() {
        let mut state = LocalizePluginState::new(vec!["A_English.STRINGS".to_owned()]);
        state.translation = true;
        state.from_checked[0] = true;
        state.to_checked[0] = true;
        assert_eq!(state.confirm(), None);
        assert_eq!(state.error.as_deref(), Some("Translation files should not match"));
    }

    #[test]
    fn the_lists_must_balance() {
        let mut state = LocalizePluginState::new(vec!["A_English.STRINGS".to_owned(), "B_English.STRINGS".to_owned()]);
        state.translation = true;
        state.from_checked[0] = true;
        assert_eq!(state.confirm(), None);
        assert_eq!(state.error.as_deref(), Some("Translation files should come in pairs"));
        state.to_checked[1] = true;
        assert_eq!(
            state.confirm(),
            Some(LocalizePluginAction::Localize {
                translate_from: vec!["A_English.STRINGS".to_owned()],
                translate_to: vec!["B_English.STRINGS".to_owned()]
            })
        );
    }
}
