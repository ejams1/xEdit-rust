// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeLocalizationForm.pas (FormCreate,
// vetStringsInitNode, vetStringsInitChildren, vetStringsGetText,
// vetStringsChange, btnSaveClick, vetStringsPaintText, memoTextChange,
// pmuStringsPopup, mniFileExportClick, EditValue) and
// xEdit/xeLocalizationForm.dfm (Panel1 with vetStrings, Splitter1,
// Panel2 with memoText and pnlControls with btnSave, pmuStrings with
// Export/Import)

//! `TfrmLocalization`: the string tables and their strings on the left, the
//! text of the selected string on the right, and the Save and Export
//! actions. The tables are `localization.files`, the strings
//! `localization.strings`, the text `localization.get`, Save
//! `localization.set` and Export `localization.export`.

use eframe::egui;

use xedit_session::localization::{LocalizationStringsResponse, StringEntry, TableInfo};

use crate::dialogs::viewers::{SplitOrientation, SplitViewer};
use crate::widgets::conflict::ConflictColors;
use crate::widgets::virtual_tree::{TreeCell, TreeColumn, TreeRow, TreeSource, TreeState};

/// What the dialog asks the shell to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalizationAction {
    /// `FormCreate`: the loaded string tables.
    Files,
    /// `vetStringsInitChildren`: the strings of a table.
    Strings { table: String },
    /// `mniFileExportClick`: `localization.export`.
    Export { table: String, output: Option<String> },
    /// `btnSaveClick`: `localization.set` of the edited text.
    Set { table: String, id: String, text: String },
    /// `mniFileImport` has no handler in the release's DFM (the menu item
    /// is drawn without an `OnClick`); the port draws it disabled.
    Closed,
}

/// One row of the strings tree.
#[derive(Clone, Debug, PartialEq, Eq)]
enum StringsRow {
    Table(usize),
    String { table: usize, entry: usize },
}

/// `TfrmLocalization`'s state.
#[derive(Clone, Debug, Default)]
pub struct LocalizationState {
    pub tables: Vec<TableInfo>,
    /// The strings of every table that was expanded (loaded on demand).
    pub strings: Vec<Option<Vec<StringEntry>>>,
    pub expanded: Vec<bool>,
    /// `vetStrings.FocusedNode`: the table index and, for a string, the
    /// string ID.
    pub selected: Option<(usize, Option<String>)>,
    /// `memoText.Lines.Text`.
    pub memo: String,
    /// `memoText.Modified`.
    pub memo_modified: bool,
    /// `pnlControls.Visible`: the Save button.
    pub save_visible: bool,
    /// The save path the export dialog proposed (`dlgExport.FileName`).
    pub export_path: String,
    pub error: Option<String>,
    pub tree: TreeState,
    pub split: f32,
    pub listed: bool,
}

impl LocalizationState {
    /// `FormCreate`.
    pub fn new() -> Self {
        Self {
            tree: TreeState::new(vec![100.0, 225.0]),
            split: 329.0,
            ..Self::default()
        }
    }

    /// The first `localization.files` the form shows.
    pub fn show(&mut self) -> Option<LocalizationAction> {
        if self.listed {
            return None;
        }
        self.listed = true;
        Some(LocalizationAction::Files)
    }

    /// `vetStrings.RootNodeCount := wbLocalizationHandler.Count`.
    pub fn apply_files(&mut self, tables: Vec<TableInfo>) {
        self.expanded = vec![false; tables.len()];
        self.strings = vec![None; tables.len()];
        self.tables = tables;
        self.selected = None;
        self.memo.clear();
        self.memo_modified = false;
        self.save_visible = false;
        self.error = None;
    }

    /// `vetStringsInitChildren`'s answer: the strings of one table.
    pub fn apply_strings(&mut self, table: &str, response: LocalizationStringsResponse) {
        if let Some(index) = self.tables.iter().position(|info| info.name == table)
            && let Some(slot) = self.strings.get_mut(index)
        {
            *slot = Some(response.strings);
            if let Some(info) = self.tables.get_mut(index) {
                info.strings = response.table.strings;
                info.modified = response.table.modified;
            }
        }
    }

    fn rows(&self) -> Vec<StringsRow> {
        let mut rows = Vec::new();
        for table in 0..self.tables.len() {
            rows.push(StringsRow::Table(table));
            if self.expanded.get(table).copied().unwrap_or(false) {
                for entry in 0..self
                    .strings
                    .get(table)
                    .and_then(|slot| slot.as_ref())
                    .map_or(0, Vec::len)
                {
                    rows.push(StringsRow::String { table, entry });
                }
            }
        }
        rows
    }

    /// `vetStringsInitNode`/`vetStringsChange`: a table node expands, a
    /// string node fills the memo (`Data.lFile[Data.ID]`).
    pub fn select(&mut self, row: usize) -> Option<LocalizationAction> {
        let rows = self.rows();
        self.memo_modified = false;
        self.save_visible = false;
        match rows.get(row).cloned() {
            Some(StringsRow::Table(table)) => {
                self.selected = Some((table, None));
                self.memo.clear();
                let expanded = self.expanded.get(table).copied().unwrap_or(false);
                if let Some(slot) = self.expanded.get_mut(table) {
                    *slot = !expanded;
                }
                if expanded {
                    None
                } else if self.strings.get(table).and_then(|slot| slot.as_ref()).is_none() {
                    Some(LocalizationAction::Strings {
                        table: self.tables.get(table)?.name.clone(),
                    })
                } else {
                    None
                }
            }
            Some(StringsRow::String { table, entry }) => {
                let info = self.tables.get(table)?;
                let string = self.strings.get(table)?.as_ref()?.get(entry)?;
                self.selected = Some((table, Some(string.id.clone())));
                self.memo = string.text.clone();
                self.export_path = format!("{}.txt", info.name);
                None
            }
            None => None,
        }
    }

    /// `memoTextChange`: the Save button appears once the memo changed and a
    /// string is focused.
    pub fn memo_changed(&mut self) {
        self.memo_modified = true;
        if matches!(self.selected, Some((_, Some(_)))) {
            self.save_visible = true;
        }
    }

    /// `btnSaveClick`: the request for the edited string.
    pub fn save(&mut self) -> Option<LocalizationAction> {
        let (table, id) = self.selected.clone()?;
        let id = id?;
        let table = self.tables.get(table)?.name.clone();
        Some(LocalizationAction::Set {
            table,
            id,
            text: self.memo.clone(),
        })
    }

    /// `btnSaveClick`'s tail: the modified flag cleared, the Save hidden and
    /// the row's text updated.
    pub fn apply_save(&mut self, table: &str, id: &str, text: String) {
        if let Some((table_index, Some(selected_id))) = self.selected.clone()
            && selected_id == id
            && self.tables.get(table_index).is_some_and(|info| info.name == table)
            && let Some(strings) = self.strings.get_mut(table_index).and_then(|slot| slot.as_mut())
            && let Some(entry) = strings.iter_mut().find(|entry| entry.id == id)
        {
            entry.text = text;
        }
        if let Some(table) = self.tables.iter_mut().find(|info| info.name == table) {
            table.modified = true;
        }
        self.memo_modified = false;
        self.save_visible = false;
    }

    /// `mniFileExportClick`: the table of the focused node.
    pub fn export(&self) -> Option<LocalizationAction> {
        let (table, _) = self.selected.clone()?;
        let info = self.tables.get(table)?;
        Some(LocalizationAction::Export {
            table: info.name.clone(),
            output: (!self.export_path.is_empty()).then(|| self.export_path.clone()),
        })
    }
}

/// The `vetStrings` source.
struct StringsTree<'a> {
    rows: &'a [StringsRow],
    tables: &'a [TableInfo],
    expanded: &'a [bool],
    strings: &'a [Option<Vec<StringEntry>>],
    columns: Vec<TreeColumn>,
}

impl StringsTree<'_> {
    fn table_of(&self, index: usize) -> Option<&TableInfo> {
        self.tables.get(index)
    }
}

impl TreeSource for StringsTree<'_> {
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
        match row {
            StringsRow::Table(table) => {
                let Some(info) = self.table_of(*table) else {
                    return;
                };
                out.id = *table as u64;
                out.depth = 0;
                out.has_children = info.strings > 0;
                out.expanded = self.expanded.get(*table).copied().unwrap_or(false);
                out.cells = vec![TreeCell::new(info.name.clone()), TreeCell::new("")];
                if info.modified {
                    // `vetStringsPaintText`: a modified table is bold; the
                    // widget paints a marker instead (UPSTREAM-QUIRK).
                    out.cells[0].tooltip = Some("This table has unsaved changes".to_owned());
                    out.icon = Some('\u{25CF}');
                }
            }
            StringsRow::String { table, entry } => {
                let Some(info) = self.table_of(*table) else {
                    return;
                };
                let Some(string) = self
                    .strings
                    .get(*table)
                    .and_then(|slot| slot.as_ref())
                    .and_then(|strings| strings.get(*entry))
                else {
                    return;
                };
                out.id = (1_000_000 + index) as u64;
                out.depth = 1;
                out.cells = vec![TreeCell::new(string.id.clone()), TreeCell::new(string.text.clone())];
                let _ = info;
            }
        }
    }
}

/// Draws the editor.
pub fn show(ctx: &egui::Context, state: &mut LocalizationState) -> Option<LocalizationAction> {
    if let Some(action) = state.show() {
        return Some(action);
    }
    let mut action = None;
    let mut open = true;
    egui::Window::new("Localization Editor")
        .id(egui::Id::unique("frmLocalization"))
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(840.0, 560.0))
        .default_pos(egui::pos2(60.0, 60.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let rows = {
                let mut rows = Vec::new();
                for table in 0..state.tables.len() {
                    rows.push(StringsRow::Table(table));
                    if state.expanded.get(table).copied().unwrap_or(false) {
                        for entry in 0..state
                            .strings
                            .get(table)
                            .and_then(|slot| slot.as_ref())
                            .map_or(0, Vec::len)
                        {
                            rows.push(StringsRow::String { table, entry });
                        }
                    }
                }
                rows
            };
            let columns = vec![TreeColumn::new("String ID", 100.0), TreeColumn::new("Text", 225.0)];
            let selected = {
                let (tree, split) = (&mut state.tree, &mut state.split);
                let mut source = StringsTree {
                    rows: &rows,
                    tables: &state.tables,
                    expanded: &state.expanded,
                    strings: &state.strings,
                    columns,
                };
                SplitViewer {
                    id: "vetStrings",
                    source: &mut source,
                    state: tree,
                    memo: &state.memo,
                    orientation: SplitOrientation::TreeLeft,
                    split,
                    colors: &ConflictColors::default(),
                    header: true,
                }
                .show(ui)
            };
            if let Some(row) = selected.clicked.or(selected.double_clicked) {
                action = state.select(row);
            }
            if selected.clicked.is_some() && selected.clicked == selected.double_clicked {
                // A double click on a table toggles twice; keep the expand.
            }
            // `memoText`: the text of the selected string.
            ui.separator();
            let mut memo = state.memo.clone();
            let response = egui::TextEdit::multiline(&mut memo)
                .id(egui::Id::unique("localization-memo"))
                .desired_rows(6)
                .desired_width(f32::INFINITY)
                .show(ui);
            if memo != state.memo {
                state.memo = memo;
                state.memo_changed();
            }
            let _ = response;
            ui.horizontal(|ui| {
                if state.save_visible && ui.button("Save").clicked() {
                    action = state.save();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        action = Some(LocalizationAction::Closed);
                    }
                    if ui.button("Export to file").clicked() {
                        action = state.export();
                    }
                    ui.add_enabled(false, egui::Button::new("Import from file"))
                        .on_disabled_hover_text("Import has no handler in the release's form");
                });
            });
            if let Some(message) = state.error.clone() {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
        });
    if !open && action.is_none() {
        action = Some(LocalizationAction::Closed);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(name: &str, strings: usize, modified: bool) -> TableInfo {
        TableInfo {
            index: 0,
            name: name.to_owned(),
            path: name.to_owned(),
            strings,
            modified,
            next_id: "00000001".to_owned(),
        }
    }

    fn state() -> LocalizationState {
        let mut state = LocalizationState::new();
        state.apply_files(vec![table("Skyrim_English.STRINGS", 2, false)]);
        state
    }

    #[test]
    fn expanding_a_table_loads_its_strings_and_selecting_one_fills_the_memo() {
        let mut state = state();
        assert_eq!(
            state.select(0),
            Some(LocalizationAction::Strings {
                table: "Skyrim_English.STRINGS".to_owned()
            })
        );
        state.apply_strings(
            "Skyrim_English.STRINGS",
            LocalizationStringsResponse {
                table: table("Skyrim_English.STRINGS", 2, false),
                strings: vec![
                    StringEntry {
                        id: "00000001".to_owned(),
                        text: "one".to_owned(),
                    },
                    StringEntry {
                        id: "00000002".to_owned(),
                        text: "two".to_owned(),
                    },
                ],
            },
        );
        assert!(state.expanded[0]);
        assert_eq!(state.select(2), None);
        assert_eq!(state.memo, "two");
        assert_eq!(state.selected, Some((0, Some("00000002".to_owned()))));
    }

    #[test]
    fn the_save_button_follows_the_memo() {
        let mut state = state();
        state.select(0);
        state.apply_strings(
            "Skyrim_English.STRINGS",
            LocalizationStringsResponse {
                table: table("Skyrim_English.STRINGS", 1, false),
                strings: vec![StringEntry {
                    id: "00000001".to_owned(),
                    text: "one".to_owned(),
                }],
            },
        );
        state.select(1);
        assert!(!state.save_visible);
        state.memo = "changed".to_owned();
        state.memo_changed();
        assert!(state.save_visible);
        assert_eq!(
            state.save(),
            Some(LocalizationAction::Set {
                table: "Skyrim_English.STRINGS".to_owned(),
                id: "00000001".to_owned(),
                text: "changed".to_owned()
            })
        );
        state.apply_save("Skyrim_English.STRINGS", "00000001", "changed".to_owned());
        assert!(!state.save_visible);
        assert!(state.tables[0].modified);
        assert_eq!(state.strings[0].as_ref().unwrap()[0].text, "changed");
    }

    #[test]
    fn the_export_names_the_focused_table() {
        let mut state = state();
        state.select(0);
        assert_eq!(
            state.export(),
            Some(LocalizationAction::Export {
                table: "Skyrim_English.STRINGS".to_owned(),
                output: None
            })
        );
    }
}
