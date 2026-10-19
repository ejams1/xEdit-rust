// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeViewElementsForm.pas (FormCreate, FormShow,
// FormClose, FormKeyDown, AddElement, btnCopyToClipboard,
// btnCompareClick, mniCompareConfClick, FindDialog1Find, MemoChange,
// MemoKeyDown, ShowModal) and xEdit/xeViewElementsForm.dfm (pcView,
// pnlButtons with LiteCopyButton, btnCompare, LiteCompareButton, btnOK and
// btnCancel, PopupMenu1 with mniCompareConf, dlgCompareTool, FindDialog1)

//! `TfrmViewElements`: one memo per plugin that holds the element, with the
//! Copy, Compare and Save buttons, the Ctrl+F find and the compare tool
//! command line. The element's text is `elements.get` (`node.value`), the
//! Save is `elements.set` for every edited tab; the dialog only returns
//! actions.

use std::path::{Path, PathBuf};

use eframe::egui;

use xedit_session::commands::ElementDetail;

/// What the dialog asks the shell to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewElementsAction {
    /// `ShowModal`'s `mrOk`: `elements.set` for every edited memo.
    Save { updates: Vec<ElementUpdate> },
    /// `btnCancelClick`.
    Closed,
}

/// One edited element the shell writes back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementUpdate {
    pub form_id: String,
    /// The element's path (`elements.get`'s `path`).
    pub path: String,
    pub file: Option<String>,
    pub text: String,
}

/// One page of `pcView`: a memo per plugin (`AddElement`).
#[derive(Clone, Debug, PartialEq)]
pub struct ElementTab {
    /// `TabSheet.Caption`: the file's name, with the `*` a modified memo
    /// adds.
    pub caption: String,
    /// The file name without the `*`.
    pub file: String,
    /// The text the element had when the page was made.
    pub text: String,
    /// The text the memo holds now.
    pub edited: String,
    /// `Memo.ReadOnly := not aEditable`.
    pub editable: bool,
    /// `Memo.Modified`.
    pub modified: bool,
    /// Where a Save writes: the record and element the page shows.
    pub form_id: String,
    pub path: String,
}

impl ElementTab {
    /// `AddElement(aElement, aFocused, aEditable)`: a page for one plugin's
    /// view of the element.
    pub fn from_detail(detail: &ElementDetail, file: String, editable: bool, form_id: String) -> Self {
        let text = if detail.node.value.is_empty() {
            detail.node.summary.clone()
        } else {
            detail.node.value.clone()
        };
        Self {
            caption: file.clone(),
            file,
            text: text.clone(),
            edited: text,
            editable,
            modified: false,
            form_id,
            path: detail.path.clone(),
        }
    }

    /// `MemoChange` and the `*` of `btnCompareClick`.
    pub fn set_edited(&mut self, text: String) {
        self.modified = text != self.text;
        self.edited = text;
        let base = self.file.clone();
        self.caption = if self.modified { format!("{base}*") } else { base };
    }
}

/// `TfrmViewElements`'s state.
#[derive(Clone, Debug)]
pub struct ViewElementsState {
    pub tabs: Vec<ElementTab>,
    /// `pcView.TabIndex`.
    pub active: usize,
    /// `[External] CompareCommandLine`.
    pub compare_cmdline: String,
    /// `edSearch`'s text (the find dialog's FindText).
    pub find: String,
    /// `frMatchCase`.
    pub find_match_case: bool,
    /// `FSelPos`: where the next `frFindNext` starts.
    pub find_pos: usize,
    /// The "Could not find" message of the find dialog.
    pub find_error: Option<String>,
    /// The result of the last compare run (`MessageBox` on failure).
    pub compare_message: Option<String>,
    /// `[frmViewElements]` placement.
    pub placement: crate::settings::WindowPlacement,
    /// Whether the placement was read from the settings already.
    pub placement_loaded: bool,
    /// A find to focus in the active memo this frame.
    pub select: Option<(usize, usize)>,
}

impl Default for ViewElementsState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            compare_cmdline: "bcompare.exe %1 %2".to_owned(),
            find: String::new(),
            find_match_case: false,
            find_pos: 0,
            find_error: None,
            compare_message: None,
            placement: crate::settings::WindowPlacement::default(),
            placement_loaded: false,
            select: None,
        }
    }
}

impl ViewElementsState {
    /// `ShowModal`: `mrAbort` without a page.
    pub fn can_show(&self) -> bool {
        !self.tabs.is_empty()
    }

    /// `btnCopyToClipboard`: the active memo's text.
    pub fn active_text(&self) -> Option<String> {
        self.tabs.get(self.active).map(|tab| tab.edited.clone())
    }

    /// `ShowModal`'s `mrOk`: the edited pages. A page that cannot be edited
    /// is left alone (`if eMemo.Modified`).
    pub fn save_updates(&self) -> Vec<ElementUpdate> {
        self.tabs
            .iter()
            .filter(|tab| tab.editable && tab.modified)
            .map(|tab| ElementUpdate {
                form_id: tab.form_id.clone(),
                path: tab.path.clone(),
                file: (!tab.file.is_empty()).then(|| tab.file.clone()),
                text: tab.edited.clone(),
            })
            .collect()
    }

    /// `FindDialog1Find`: from `find_pos` on a `frFindNext`, else from the
    /// start; the case folding of `frMatchCase`. Returns where the match is.
    pub fn find(&mut self, find_next: bool) -> Option<(usize, usize)> {
        let tab = self.tabs.get(self.active)?;
        let text = &tab.edited;
        let start = if find_next {
            self.find_pos + self.find.chars().count()
        } else {
            0
        };
        let haystack: String = text.chars().skip(start).collect();
        let (haystack, needle) = if self.find_match_case {
            (haystack, self.find.clone())
        } else {
            (haystack.to_lowercase(), self.find.to_lowercase())
        };
        if needle.is_empty() {
            return None;
        }
        match haystack.find(&needle) {
            Some(byte_offset) => {
                let char_offset = haystack[..byte_offset].chars().count();
                let position = start + char_offset;
                self.find_pos = position;
                self.find_error = None;
                Some((position, position + self.find.chars().count()))
            }
            None => {
                self.find_error = Some(if find_next {
                    format!("There are no further occurences of \"{}\".", self.find)
                } else {
                    format!("Could not find \"{}\".", self.find)
                });
                None
            }
        }
    }

    /// `btnCompareClick`'s file names: `<program folder><caption>.txt`, the
    /// `*` of a modified caption dropped.
    pub fn compare_paths(&self, program_folder: &Path) -> Option<(usize, usize, PathBuf, PathBuf)> {
        if self.tabs.len() < 2 {
            return None;
        }
        let mut index = self.active;
        if index == 0 {
            index += 1;
        }
        let second = index;
        let first = index - 1;
        let path_of = |tab: &ElementTab| {
            let caption = tab.caption.trim_end_matches('*');
            program_folder.join(format!("{caption}.txt"))
        };
        Some((first, second, path_of(&self.tabs[first]), path_of(&self.tabs[second])))
    }

    /// `btnCompareClick`: writes the two memos, runs the tool (`%1` and
    /// `%2` are the temp files), reads the files back into the editable
    /// memos and deletes them.
    pub fn run_compare(&mut self) {
        let folder = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let Some((first, second, first_path, second_path)) = self.compare_paths(&folder) else {
            return;
        };
        let Some(first_text) = self.tabs.get(first).map(|tab| tab.edited.clone()) else {
            return;
        };
        let Some(second_text) = self.tabs.get(second).map(|tab| tab.edited.clone()) else {
            return;
        };
        let write = |path: &Path, text: &str| std::fs::write(path, text);
        if let Err(error) = write(&first_path, &first_text).and_then(|()| write(&second_path, &second_text)) {
            self.compare_message = Some(format!("Could not write the comparison files: {error}"));
            return;
        }
        let command = self
            .compare_cmdline
            .replace("%1", &format!("\"{}\"", first_path.display()))
            .replace("%2", &format!("\"{}\"", second_path.display()));
        let outcome = run_command_line(&command);
        match outcome {
            Ok(()) => {
                let read = |path: &Path| std::fs::read_to_string(path).ok();
                for (index, path) in [(first, &first_path), (second, &second_path)] {
                    let Some(text) = read(path) else {
                        continue;
                    };
                    if let Some(tab) = self.tabs.get_mut(index)
                        && tab.editable
                        && tab.edited != text
                    {
                        tab.set_edited(text);
                    }
                }
                self.compare_message = None;
            }
            Err(message) => {
                self.compare_message = Some(format!("Could not execute command line\r{}\rError: {message}", command));
            }
        }
        let _ = std::fs::remove_file(&first_path);
        let _ = std::fs::remove_file(&second_path);
    }
}

/// `CreateProcess` + `WaitForSingleObject(INFINITE)` of the compare tool: the
/// command line split on its spaces outside quotes.
fn run_command_line(command: &str) -> Result<(), String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in command.chars() {
        match ch {
            '"' => quoted = !quoted,
            ' ' if !quoted => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    let Some((program, arguments)) = parts.split_first() else {
        return Ok(());
    };
    let status = std::process::Command::new(program)
        .args(arguments)
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("the program exited with {status}"));
    }
    Ok(())
}

/// Draws the dialog. Returns the save the shell runs (`elements.set`).
pub fn show(ctx: &egui::Context, state: &mut ViewElementsState) -> Option<ViewElementsAction> {
    if !state.can_show() {
        return Some(ViewElementsAction::Closed);
    }
    let mut action = None;
    let mut open = true;
    let mut copy = false;
    let mut compare = false;
    let mut find_next = false;
    let mut find_first = false;
    let placement = state.placement;
    let left = if placement.left > 0 { placement.left } else { 60 };
    let top = if placement.top > 0 { placement.top } else { 60 };
    let mut window = egui::Window::new("View Elements")
        .id(egui::Id::unique("frmViewElements"))
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(placement.width as f32, placement.height as f32))
        .default_pos(egui::pos2(left as f32, top as f32))
        .open(&mut open);
    if placement.state == crate::settings::WindowState::Maximized {
        window = window.default_size(egui::vec2(1180.0, 760.0));
    }
    window.show(ctx, |ui| {
        // `pcView`'s tabs.
        let mut switch_to = None;
        ui.horizontal(|ui| {
            for (index, tab) in state.tabs.iter().enumerate() {
                if ui.selectable_label(state.active == index, &tab.caption).clicked() {
                    switch_to = Some(index);
                }
            }
        });
        if let Some(index) = switch_to {
            state.active = index;
            state.find_pos = 0;
            state.find_error = None;
        }
        ui.separator();
        if let Some(tab) = state.tabs.get(state.active) {
            let mut edited = tab.edited.clone();
            let editable = tab.editable;
            let id = egui::Id::unique("view-elements-memo");
            let output = egui::TextEdit::multiline(&mut edited)
                .id(id)
                .desired_rows(18)
                .desired_width(f32::INFINITY)
                .interactive(editable)
                .show(ui);
            if editable
                && edited != tab.edited
                && let Some(tab) = state.tabs.get_mut(state.active)
            {
                tab.set_edited(edited);
            }
            // `MemoKeyDown`: Ctrl+A is egui's own; Ctrl+F starts the find.
            if output.response.has_focus()
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F))
            {
                find_first = true;
            }
            // A found range is selected in the memo.
            if let Some((start, end)) = state.select.take() {
                let mut edit_state = output.state.clone();
                edit_state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(start),
                    egui::text::CCursor::new(end),
                )));
                edit_state.store(ui.ctx(), id);
            }
        }
        // `FindDialog1`.
        if find_first || find_next || !state.find.is_empty() {
            ui.horizontal(|ui| {
                ui.label("Find:");
                ui.add(egui::TextEdit::singleline(&mut state.find).desired_width(220.0));
                ui.checkbox(&mut state.find_match_case, "Match case");
                if ui.button("Find").clicked() {
                    find_first = true;
                }
                if ui.button("Find next").clicked() {
                    find_next = true;
                }
            });
        }
        if let Some(message) = state.find_error.clone() {
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
        if let Some(message) = state.compare_message.clone() {
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
        ui.separator();
        // `PopupMenu1`'s "Comparison program".
        ui.horizontal(|ui| {
            ui.label("Compare command line:");
            ui.add(egui::TextEdit::singleline(&mut state.compare_cmdline).desired_width(320.0));
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    action = Some(ViewElementsAction::Closed);
                }
                let has_edits = state.tabs.iter().any(|tab| tab.editable);
                if has_edits && ui.button("Save").clicked() {
                    action = Some(ViewElementsAction::Save {
                        updates: state.save_updates(),
                    });
                }
                if ui
                    .add_enabled(state.tabs.len() >= 2, egui::Button::new("Compare"))
                    .clicked()
                {
                    compare = true;
                }
                if ui.button("Copy").clicked() {
                    copy = true;
                }
            });
        });
    });
    // The buttons act after the window, on the state.
    if (find_first || find_next)
        && let Some((start, end)) = state.find(find_next)
    {
        state.select = Some((start, end));
    }
    if copy && let Some(text) = state.active_text() {
        ctx.copy_text(text);
    }
    if compare {
        state.run_compare();
    }
    if !open && action.is_none() {
        action = Some(ViewElementsAction::Closed);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use xedit_session::commands::ElementNode;

    fn detail(path: &str, value: &str) -> ElementDetail {
        ElementDetail {
            path: path.to_owned(),
            node: ElementNode {
                name: "DATA".to_owned(),
                display_name: None,
                value: value.to_owned(),
                summary: String::new(),
                native: serde_json::Value::Null,
                children: Vec::new(),
            },
        }
    }

    fn state() -> ViewElementsState {
        ViewElementsState {
            tabs: vec![
                ElementTab::from_detail(
                    &detail("DATA", "first"),
                    "Skyrim.esm".to_owned(),
                    true,
                    "0000003C".to_owned(),
                ),
                ElementTab::from_detail(
                    &detail("DATA", "second"),
                    "Update.esm".to_owned(),
                    false,
                    "0000003C".to_owned(),
                ),
            ],
            ..ViewElementsState::default()
        }
    }

    #[test]
    fn an_edited_tab_gets_the_star_and_the_save_update() {
        let mut state = state();
        assert!(state.save_updates().is_empty());
        state.tabs[0].set_edited("changed".to_owned());
        assert_eq!(state.tabs[0].caption, "Skyrim.esm*");
        let updates = state.save_updates();
        assert_eq!(
            updates,
            vec![ElementUpdate {
                form_id: "0000003C".to_owned(),
                path: "DATA".to_owned(),
                file: Some("Skyrim.esm".to_owned()),
                text: "changed".to_owned()
            }]
        );
        // A read-only page never saves.
        state.tabs[1].set_edited("x".to_owned());
        assert_eq!(state.save_updates().len(), 1);
    }

    #[test]
    fn the_find_starts_over_and_then_continues() {
        let mut state = state();
        state.tabs[0].set_edited("one two one".to_owned());
        state.find = "one".to_owned();
        assert_eq!(state.find(false), Some((0, 3)));
        assert_eq!(state.find(true), Some((8, 11)));
        assert!(state.find(true).is_none());
        assert_eq!(
            state.find_error.as_deref(),
            Some("There are no further occurences of \"one\".")
        );
        state.find = "ONE".to_owned();
        state.find_pos = 0;
        assert_eq!(state.find(false), Some((0, 3)));
        state.find_match_case = true;
        assert!(state.find(false).is_none());
        assert_eq!(state.find_error.as_deref(), Some("Could not find \"ONE\"."));
    }

    #[test]
    fn the_compare_needs_two_pages_and_names_the_dropped_star() {
        let mut state = state();
        state.active = 0;
        state.tabs[1].set_edited("second".to_owned());
        let (first, second, first_path, second_path) = state.compare_paths(Path::new("M:\\bin")).unwrap();
        assert_eq!((first, second), (0, 1));
        assert!(first_path.ends_with("Skyrim.esm.txt"));
        assert!(second_path.ends_with("Update.esm.txt"));
        state.tabs.truncate(1);
        assert!(state.compare_paths(Path::new("M:\\bin")).is_none());
    }

    #[test]
    fn an_empty_page_list_refuses_to_open() {
        let state = ViewElementsState::default();
        assert!(!state.can_show());
    }
}
