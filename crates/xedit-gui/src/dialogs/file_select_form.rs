// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeFileSelectForm.pas (FormCreate, FormKeyDown,
// CheckListBox1DblClick, edSearchChange, TCheckListBox.DrawItem,
// SelectAll1Click, SelectNone1Click, InvertSelection1Click) and
// xEdit/xeFileSelectForm.dfm

//! `TfrmFileSelect`: the owner-drawn searchable check list with the bulk
//! actions and the "Backup plugins" option. The dialog is a reusable list:
//! the save and dump flows fill it with their items; the shell fills it
//! with the loaded files.

use eframe::egui;

use crate::dialogs::selection::{CheckItem, DialogPlacement, affected_rows, char_pressed, first_match, row_matches};

/// What the dialog returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileSelectAction {
    /// `btnOK.Click`: the checked items in list order.
    Ok {
        items: Vec<String>,
        backup: bool,
    },
    Cancelled,
}

/// `TfrmFileSelect` as data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileSelectState {
    pub items: Vec<CheckItem>,
    /// `edSearch.Text`.
    pub search: String,
    /// `CheckListBox1.ItemIndex`.
    pub item_index: Option<usize>,
    /// `cbBackup.Checked` (the DFM default is checked).
    pub backup: bool,
    /// `cbBackup.Visible`; the save flow shows it, a caller may not.
    pub backup_visible: bool,
    /// `NoEscape`.
    pub no_escape: bool,
    /// `Caption`.
    pub caption: String,
    pub placement: Option<DialogPlacement>,
    pub rect: Option<egui::Rect>,
}

impl FileSelectState {
    /// A dialog over the items of a caller, none checked.
    pub fn new(caption: impl Into<String>, items: Vec<String>) -> Self {
        Self {
            items: items.into_iter().map(|item| CheckItem::new(item, false)).collect(),
            backup: true,
            backup_visible: true,
            caption: caption.into(),
            ..Self::default()
        }
    }

    /// The checked items in list order.
    pub fn checked(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.checked)
            .map(|item| item.text.clone())
            .collect()
    }

    /// `edSearchChange`: the first entry that contains the search text is
    /// `ItemIndex`; none clears it.
    pub fn search_changed(&mut self) {
        let texts: Vec<String> = self.items.iter().map(|item| item.text.clone()).collect();
        self.item_index = first_match(&texts, &self.search);
    }

    /// `TListBox.DrawItem`/`TCheckListBox.DrawItem`: the row is highlighted
    /// when the search text matches it and it is not selected.
    pub fn highlighted(&self, index: usize) -> bool {
        self.items
            .get(index)
            .is_some_and(|item| !item.selected && row_matches(&item.text, &self.search))
    }

    fn scope(&self) -> Vec<usize> {
        let enabled: Vec<bool> = self.items.iter().map(|item| item.enabled).collect();
        let selected: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.selected)
            .map(|(index, _)| index)
            .collect();
        if selected.len() >= 2 {
            affected_rows(&selected, &enabled)
        } else {
            // `CheckListBox1.SelCount < 2`.
            affected_rows(&[], &enabled)
        }
    }

    /// `SelectAll1Click`.
    pub fn select_all(&mut self) {
        for index in self.scope() {
            self.items[index].checked = true;
        }
    }

    /// `SelectNone1Click`.
    pub fn select_none(&mut self) {
        for index in self.scope() {
            self.items[index].checked = false;
        }
    }

    /// `InvertSelection1Click`.
    pub fn invert_selection(&mut self) {
        for index in self.scope() {
            self.items[index].checked = !self.items[index].checked;
        }
    }

    /// A click on a row: `ItemIndex` follows it, and Ctrl/Shift extend the
    /// list selection (`MultiSelect`).
    pub fn click(&mut self, index: usize, modifiers: egui::Modifiers) {
        if modifiers.command {
            if let Some(item) = self.items.get_mut(index) {
                item.selected = !item.selected;
            }
        } else if modifiers.shift {
            let anchor = self.item_index.unwrap_or(index);
            let (from, to) = if anchor <= index {
                (anchor, index)
            } else {
                (index, anchor)
            };
            for (row, item) in self.items.iter_mut().enumerate() {
                item.selected = row >= from && row <= to;
            }
        } else {
            for item in &mut self.items {
                item.selected = false;
            }
            if let Some(item) = self.items.get_mut(index) {
                item.selected = true;
            }
        }
        self.item_index = Some(index);
    }

    /// `CheckListBox1DblClick`: clear the checks, check the clicked row and
    /// answer OK.
    pub fn double_click(&mut self, index: usize) {
        self.select_none();
        if let Some(item) = self.items.get_mut(index) {
            item.checked = true;
        }
    }
}

/// Draws the dialog.
pub fn show(ctx: &egui::Context, state: &mut FileSelectState) -> Option<FileSelectAction> {
    let mut action = None;
    let mut open = true;
    let mut search_focused = false;
    let mut clicked = None;
    let mut double_clicked = None;
    let mut window = egui::Window::new(if state.caption.is_empty() {
        "Select Files"
    } else {
        &state.caption
    })
    .id(egui::Id::unique("frmFileSelect"))
    .collapsible(false)
    .resizable(true)
    .default_size(crate::dialogs::selection::fit_size(ctx, egui::vec2(480.0, 620.0)))
    .open(&mut open);
    window = match state.placement {
        Some(placement) => placement.apply(ctx, window),
        None => window.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)),
    };
    let response = window.show(ctx, |ui| {
        ui.label("Search");
        let edit = ui.add(egui::TextEdit::singleline(&mut state.search));
        search_focused = edit.has_focus();
        if edit.changed() {
            state.search_changed();
        }
        ui.separator();
        let height = (ui.available_height() - 40.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("file-select-items")
            .max_height(height)
            .show(ui, |ui| {
                for index in 0..state.items.len() {
                    let (text, highlighted) = {
                        let item = &state.items[index];
                        (item.text.clone(), state.highlighted(index))
                    };
                    let selected = state.items[index].selected;
                    let mut checked = state.items[index].checked;
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut checked, "").changed() {
                            state.items[index].checked = checked;
                        }
                        let mut label = egui::RichText::new(&text);
                        if highlighted {
                            label = label.background_color(ui.visuals().selection.bg_fill.gamma_multiply(0.4));
                        }
                        let response = ui.selectable_label(selected, label);
                        if response.clicked() {
                            clicked = Some(index);
                        }
                        if response.double_clicked() {
                            double_clicked = Some(index);
                        }
                    });
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            if state.backup_visible && ui.checkbox(&mut state.backup, "Backup plugins").changed() {
                // `cbBackup` is presentation state of the caller.
            }
            if ui.button("OK").clicked() {
                action = Some(FileSelectAction::Ok {
                    items: state.checked(),
                    backup: state.backup,
                });
            }
            if ui.button("Cancel").clicked() {
                action = Some(FileSelectAction::Cancelled);
            }
        });
    });
    if let Some(response) = response {
        state.rect = Some(response.response.rect);
    }

    if let Some(index) = clicked {
        let modifiers = ctx.input(|input| input.modifiers);
        state.click(index, modifiers);
    }
    if let Some(index) = double_clicked {
        state.double_click(index);
        action = Some(FileSelectAction::Ok {
            items: state.checked(),
            backup: state.backup,
        });
    }
    // `FormKeyDown`, unless the search box has the focus.
    if action.is_none() && !search_focused {
        if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
            action = Some(FileSelectAction::Ok {
                items: state.checked(),
                backup: state.backup,
            });
        } else if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            if !state.no_escape {
                action = Some(FileSelectAction::Cancelled);
            }
        } else if ctx.input(|input| input.key_pressed(egui::Key::Minus)) {
            state.select_none();
        } else if ctx.input(|input| input.key_pressed(egui::Key::Plus)) {
            state.select_all();
        } else if char_pressed(ctx, '*') {
            state.invert_selection();
        }
    }
    if !open && action.is_none() {
        action = Some(FileSelectAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> FileSelectState {
        FileSelectState::new(
            "Which files?",
            vec!["Fallout4.esm".to_owned(), "DLCRobot.esm".to_owned()],
        )
    }

    fn none() -> egui::Modifiers {
        egui::Modifiers::NONE
    }

    fn ctrl() -> egui::Modifiers {
        egui::Modifiers {
            ctrl: true,
            command: true,
            ..egui::Modifiers::NONE
        }
    }

    #[test]
    fn the_search_selects_the_first_containing_row() {
        let mut state = state();
        state.search = "robot".to_owned();
        state.search_changed();
        assert_eq!(state.item_index, Some(1));
        assert!(state.highlighted(1));
        state.search = "nothing".to_owned();
        state.search_changed();
        assert_eq!(state.item_index, None);
        state.search = "  ".to_owned();
        state.search_changed();
        assert_eq!(state.item_index, None);
    }

    #[test]
    fn the_bulk_actions_follow_the_selected_or_all_rule() {
        let mut state = state();
        state.select_all();
        assert_eq!(state.checked(), vec!["Fallout4.esm", "DLCRobot.esm"]);
        state.click(0, none());
        state.click(1, ctrl());
        // Two selected rows: only those change.
        state.items[1].checked = false;
        state.select_none();
        assert_eq!(state.checked(), Vec::<String>::new());
        state.items[1].checked = true;
        state.invert_selection();
        assert_eq!(state.checked(), vec!["Fallout4.esm"]);
        // With one selected row the whole list changes.
        state.click(0, none());
        state.select_all();
        assert_eq!(state.checked().len(), 2);
    }

    #[test]
    fn a_double_click_checks_one_row_and_answers_ok() {
        let mut state = state();
        state.select_all();
        state.double_click(1);
        assert_eq!(state.checked(), vec!["DLCRobot.esm"]);
        assert!(state.item_index.is_none() || state.item_index == Some(0));
    }

    #[test]
    fn a_disabled_row_is_never_changed() {
        let mut state = state();
        state.items[1].enabled = false;
        state.select_all();
        assert_eq!(state.checked(), vec!["Fallout4.esm"]);
        state.invert_selection();
        assert!(!state.items[1].checked);
    }
}
