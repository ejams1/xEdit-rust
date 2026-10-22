// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeScriptForm.pas (FormCreate, FormShow,
// ReadScriptsList, DoScriptSelectionChange, btnSaveClick, EditorKeyDown,
// EditorKeyPress, EditorKeyUp, edFilterChange, edFilterKeyDown,
// chkScriptsSubDirClick, cmbScripts*, FormKeyDown, FormClose, Indent,
// Dedent, UpdateCaretPos) and xEdit/xeScriptForm.dfm (pnlTop with
// edFilter, cmbScripts, chkScriptsSubDir and btnSave; Editor; pnlStatus
// with lblPosition; pnlBottom with OK and Cancel)

//! `TfrmScript`: the script combo over the scripts folder, the filter, the
//! subdirectory switch, and the Pascal editor with the release's
//! highlighter and Tab behaviour. The list is `script.list`, the text is
//! `script.read`, Save is `script.save`, and OK hands the editor text to
//! `script.run`; the dialog itself only returns actions.

use eframe::egui;

use xedit_session::script::{ScriptInfo, ScriptListResponse, ScriptReadResponse, ScriptSaveResponse};

/// `sNewScript`.
pub const S_NEW_SCRIPT: &str = "<new script>";
/// `sNewScriptName`: the template the "<new script>" entry loads.
pub const S_NEW_SCRIPT_NAME: &str = "_newscript_";
/// `sScriptExt`.
pub const S_SCRIPT_EXT: &str = ".pas";

/// What the dialog asks the shell to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptFormAction {
    /// `ReadScriptsList`: `script.list` over the folder.
    List {
        folder: Option<String>,
        subdirectories: bool,
    },
    /// `DoScriptSelectionChange`: `script.read` of the chosen script.
    Read { file: String },
    /// `btnSaveClick`: `script.save`.
    Save { file: String, text: String },
    /// The "modified, save it?" question answered Yes before switching.
    SaveThenRead { file: String, text: String, read: String },
    /// The "modified, save it?" question answered Yes before OK.
    SaveThenOk { file: String, text: String, name: String },
    /// `FormClose` with `mrOk`: run the editor text (`script.run`).
    Ok { name: String, text: String },
    /// `mrCancel`.
    Cancelled,
}

/// The question a pending selection change or OK raised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptConfirm {
    /// `DoScriptSelectionChange`'s "The previous script (\"<name>\") has
    /// been modified. Do you want to save it before loading the new
    /// script?".
    Switch { index: usize },
    /// `FormClose`'s "The script has been modified. Do you want to save
    /// it?".
    Ok,
}

/// `TfrmScript`'s state.
#[derive(Clone, Debug)]
pub struct ScriptFormState {
    /// `Path`: the scripts folder, from `script.list`'s response.
    pub folder: Option<String>,
    /// The folder named on the list request (the `scripts` parameter).
    pub folder_request: Option<String>,
    /// The listed scripts (`cmbScripts.Items` minus the placeholder).
    pub listed: Vec<ScriptInfo>,
    /// `edFilter.Text`.
    pub filter: String,
    /// `chkScriptsSubDir.Checked`.
    pub subdirectories: bool,
    /// `[View] LastUsedScript`.
    pub last_used: String,
    /// `cmbScripts.ItemIndex` into [`ScriptFormState::items`].
    pub selected: Option<usize>,
    /// `Editor.Lines.Text`.
    pub editor: String,
    /// `Editor.Modified`.
    pub modified: bool,
    /// `SaveOverride`: the script name the editor was loaded from, or the
    /// name the new script was saved under.
    pub save_override: String,
    /// `lblPosition.Caption`.
    pub position: String,
    /// The file the "<new script>" entry is saved as (`dlgSave.FileName`).
    pub new_script_path: String,
    pub confirm: Option<ScriptConfirm>,
    pub error: Option<String>,
    /// Whether the first list has been asked for (`FormShow`).
    pub listed_once: bool,
}

impl ScriptFormState {
    /// `FormCreate` and `FormShow`'s inputs: the last used script and the
    /// subdirectory switch from the settings.
    pub fn new(folder: Option<String>, last_used: String, subdirectories: bool) -> Self {
        Self {
            folder: None,
            folder_request: folder,
            listed: Vec::new(),
            filter: String::new(),
            subdirectories,
            last_used,
            selected: None,
            editor: String::new(),
            modified: false,
            save_override: String::new(),
            position: "Line:1 Col:1".to_owned(),
            new_script_path: String::new(),
            confirm: None,
            error: None,
            listed_once: false,
        }
    }

    /// `FormShow`: the first `ReadScriptsList`.
    pub fn show(&mut self) -> Option<ScriptFormAction> {
        if self.listed_once {
            return None;
        }
        self.listed_once = true;
        Some(ScriptFormAction::List {
            folder: self.folder_request.clone(),
            subdirectories: self.subdirectories,
        })
    }

    /// `edFilterChange` and `chkScriptsSubDirClick`.
    pub fn relist(&mut self) -> ScriptFormAction {
        ScriptFormAction::List {
            folder: self.folder_request.clone(),
            subdirectories: self.subdirectories,
        }
    }

    /// `cmbScripts.Items`: "<new script>" first, then the subdirectory
    /// scripts, then the top folder's, each sorted as a `TStringList`
    /// (case-insensitive), then the filter (`ContainsText`).
    pub fn items(&self) -> Vec<String> {
        let mut subdirectory: Vec<String> = Vec::new();
        let mut top: Vec<String> = Vec::new();
        for script in &self.listed {
            // `ChangeFileExt(f, '')`: the combo lists the names without the
            // extension; `_newscript_` is the template of "<new script>".
            let name = script
                .name
                .strip_suffix(S_SCRIPT_EXT)
                .or_else(|| script.name.strip_suffix(".PAS"))
                .unwrap_or(&script.name);
            if name.eq_ignore_ascii_case(S_NEW_SCRIPT_NAME) {
                continue;
            }
            if name.contains('\\') {
                subdirectory.push(name.to_owned());
            } else {
                top.push(name.to_owned());
            }
        }
        subdirectory.sort_by(|a, b| text_compare(a, b));
        top.sort_by(|a, b| text_compare(a, b));
        subdirectory.extend(top);
        let filter = self.filter.trim().to_lowercase();
        let mut items = Vec::with_capacity(subdirectory.len() + 1);
        items.push(S_NEW_SCRIPT.to_owned());
        items.extend(
            subdirectory
                .into_iter()
                .filter(|name| filter.is_empty() || name.to_lowercase().contains(&filter)),
        );
        items
    }

    /// The `cmbScripts.Text` of the selection.
    pub fn current(&self) -> Option<String> {
        self.selected.and_then(|index| self.items().get(index).cloned())
    }

    /// The file name a listed item loads: `<item>.pas`, or the template for
    /// "<new script>".
    pub fn file_of(item: &str) -> String {
        if item == S_NEW_SCRIPT {
            format!("{S_NEW_SCRIPT_NAME}{S_SCRIPT_EXT}")
        } else {
            format!("{item}{S_SCRIPT_EXT}")
        }
    }

    /// The path a listed item saves to: `<folder><item>.pas` (the new
    /// script keeps the path the save dialog gave).
    pub fn save_path(&self, item: &str) -> String {
        if item == S_NEW_SCRIPT {
            return self.new_script_path.clone();
        }
        format!("{}{}", self.folder.clone().unwrap_or_default(), Self::file_of(item))
    }

    /// `ReadScriptsList`'s tail: keep the current text, else the last used
    /// script, else the first item, and read it when the selection changed.
    pub fn apply_list(&mut self, response: ScriptListResponse) -> Option<ScriptFormAction> {
        // `CurrentSelection := cmbScripts.Text` before the list is refilled.
        let current = self.current().unwrap_or_default();
        self.folder = Some(with_separator(&response.folder));
        self.listed = response.scripts;
        let target = if current.is_empty() {
            self.last_used.clone()
        } else {
            current
        };
        let items = self.items();
        let index = items.iter().position(|item| *item == target);
        let changed = index.is_none() || Some(index.unwrap()) != self.selected;
        let index = index.unwrap_or(0);
        self.selected = Some(index);
        if changed {
            return self.select_now(index);
        }
        None
    }

    /// `DoScriptSelectionChange` without the question: remember the item and
    /// ask for its text.
    fn select_now(&mut self, index: usize) -> Option<ScriptFormAction> {
        let item = self.items().get(index).cloned()?;
        self.selected = Some(index);
        self.save_override = item.clone();
        self.error = None;
        Some(ScriptFormAction::Read {
            file: Self::file_of(&item),
        })
    }

    /// `cmbScriptsSelect`/`DoScriptSelectionChange`: when the editor has
    /// unsaved text, ask first.
    pub fn select(&mut self, index: usize) -> Option<ScriptFormAction> {
        if Some(index) == self.selected {
            return None;
        }
        if self.modified && !self.editor.trim().is_empty() {
            self.confirm = Some(ScriptConfirm::Switch { index });
            return None;
        }
        self.select_now(index)
    }

    /// The answer of the "save it before loading the new script?" question.
    pub fn confirm_switch(&mut self, save: bool) -> Option<ScriptFormAction> {
        let Some(ScriptConfirm::Switch { index }) = self.confirm.take() else {
            return None;
        };
        let target = self.items().get(index).cloned()?;
        if save {
            let file = self.save_path(&self.save_override);
            if file.is_empty() {
                self.error = Some("Choose the file the new script is saved as.".to_owned());
                return None;
            }
            return Some(ScriptFormAction::SaveThenRead {
                file,
                text: self.editor.clone(),
                read: Self::file_of(&target),
            });
        }
        self.select_now(index)
    }

    /// `DoScriptSelectionChange`'s `LoadFromFile`: the tab replacement and
    /// the caret reset.
    pub fn apply_read(&mut self, response: ScriptReadResponse) {
        self.editor = response.text.replace('\t', "  ");
        self.modified = false;
        self.position = "Line:1 Col:1".to_owned();
        self.error = None;
        if self.save_override.is_empty() {
            self.save_override = response.script;
        }
    }

    /// `btnSaveClick`: the request for the selected item.
    pub fn save(&mut self) -> Option<ScriptFormAction> {
        let item = self.current()?;
        let file = self.save_path(&item);
        if file.is_empty() {
            self.error = Some("Choose the file the new script is saved as.".to_owned());
            return None;
        }
        Some(ScriptFormAction::Save {
            file,
            text: self.editor.clone(),
        })
    }

    /// `btnSaveClick`'s tail: `lblPosition` "Saved: <file>", the modified
    /// flag cleared, and a new script joins the combo
    /// (`cmbScripts.Items.Add(s2)`).
    pub fn apply_save(&mut self, response: ScriptSaveResponse) {
        let path = std::path::Path::new(&response.file);
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.position = format!("Saved: {file_name}");
        self.modified = false;
        self.error = None;
        if self.current().as_deref() == Some(S_NEW_SCRIPT) {
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !stem.is_empty() {
                if !self.listed.iter().any(|script| script.name == stem) {
                    self.listed.push(ScriptInfo {
                        name: stem.clone(),
                        lines: response.text.lines().count() as u32,
                    });
                }
                let name = stem;
                if let Some(index) = self.items().iter().position(|item| *item == name) {
                    self.selected = Some(index);
                }
                self.save_override = name;
            }
        }
    }

    /// `FormClose` with `mrOk`: with unsaved text, ask first; else run.
    pub fn ok(&mut self) -> Option<ScriptFormAction> {
        let item = self.current()?;
        if self.modified {
            self.confirm = Some(ScriptConfirm::Ok);
            return None;
        }
        Some(ScriptFormAction::Ok {
            name: item,
            text: self.editor.clone(),
        })
    }

    /// The answer of the OK question.
    pub fn confirm_ok(&mut self, save: bool) -> Option<ScriptFormAction> {
        if self.confirm != Some(ScriptConfirm::Ok) {
            return None;
        }
        self.confirm = None;
        let item = self.current()?;
        if save {
            let file = self.save_path(&item);
            if file.is_empty() {
                self.error = Some("Choose the file the new script is saved as.".to_owned());
                return None;
            }
            return Some(ScriptFormAction::SaveThenOk {
                file,
                text: self.editor.clone(),
                name: item,
            });
        }
        Some(ScriptFormAction::Ok {
            name: item,
            text: self.editor.clone(),
        })
    }

    /// `ReadScriptsList`'s filter over the combo: `edFilterKeyDown`'s Enter
    /// moves to the combo and opens it (the caller only has to refilter).
    pub fn filter_changed(&mut self) -> ScriptFormAction {
        self.relist()
    }

    /// `UpdateCaretPos` from a character offset.
    pub fn update_caret(&mut self, char_index: usize) {
        let (line, column) = caret_position(&self.editor, char_index);
        self.position = format!("Line:{line} Col:{column}");
    }
}

/// A folder with a trailing separator, as `Path` has one
/// (`wbScriptsPath` ends with a backslash).
fn with_separator(folder: &str) -> String {
    if folder.ends_with(['\\', '/']) {
        folder.to_owned()
    } else {
        format!("{folder}\\")
    }
}

/// `TStringList.Sort`: case-insensitive, falling back to the exact text.
fn text_compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

/// `UpdateCaretPos`: the 1-based line and column of a character offset.
pub fn caret_position(text: &str, char_index: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for (index, ch) in text.chars().enumerate() {
        if index >= char_index {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// `Indent(aText, '  ')`: every non-empty line gets the prefix; trailing
/// whitespace is trimmed.
pub fn indent_text(text: &str, prefix: &str) -> String {
    text.trim_end()
        .split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                line.to_owned()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `Dedent(aText, '  ')`: the prefix comes off every non-empty line that has
/// it, else the line is trimmed on the left.
pub fn dedent_text(text: &str, prefix: &str) -> String {
    text.trim_end()
        .split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                line.to_owned()
            } else if let Some(rest) = line.strip_prefix(prefix) {
                rest.to_owned()
            } else {
                line.trim_start().to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replaces the characters `[start, end)` of `text`.
pub fn replace_range(text: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut result = String::with_capacity(text.len() + replacement.len());
    for (index, ch) in text.chars().enumerate() {
        if index == start {
            result.push_str(replacement);
        }
        if index < start || index >= end {
            result.push(ch);
        }
    }
    if start >= text.chars().count() {
        result.push_str(replacement);
    }
    result
}

/// `EditorKeyDown`'s Tab: the selection indents or dedents, else two spaces
/// go in at the caret. Returns the new text and the selected character
/// range.
pub fn tab_edit(text: &str, start: usize, end: usize, shift: bool) -> (String, usize, usize) {
    if start == end {
        let new_text = replace_range(text, start, start, "  ");
        return (new_text, start + 2, start + 2);
    }
    let (start, end) = (start.min(end), start.max(end));
    let selected: String = text.chars().skip(start).take(end - start).collect();
    let replacement = if shift {
        dedent_text(&selected, "  ")
    } else {
        indent_text(&selected, "  ")
    };
    let new_text = replace_range(text, start, end, &replacement);
    let new_end = start + replacement.chars().count();
    (new_text, start, new_end)
}

/// The editor's layout: `TSynPasSyn`'s token classes with the port's theme
/// (keywords bold blue, comments green, strings red).
fn pascal_layout(ui: &egui::Ui, text: &str, wrap_width: f32) -> std::sync::Arc<egui::Galley> {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let default = ui.visuals().text_color();
    let keyword = egui::Color32::from_rgb(0, 0, 160);
    let comment = egui::Color32::from_rgb(0, 128, 0);
    let string = egui::Color32::from_rgb(160, 0, 0);
    let push = |job: &mut egui::text::LayoutJob, text: &str, color: egui::Color32, bold: bool| {
        if text.is_empty() {
            return;
        }
        let mut format = egui::TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        };
        if bold {
            format.color = egui::Color32::from_rgb(0, 0, 96);
        }
        job.append(text, 0.0, format);
    };
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let (start, ch) = (index, chars[index]);
        if ch.is_whitespace() {
            let mut end = index;
            while end < chars.len() && chars[end].is_whitespace() {
                end += 1;
            }
            push(&mut job, &chars[index..end].iter().collect::<String>(), default, false);
            index = end;
        } else if ch == '/' && chars.get(index + 1) == Some(&'/') {
            let end = chars[index..]
                .iter()
                .position(|c| *c == '\n')
                .map_or(chars.len(), |at| index + at);
            push(&mut job, &chars[index..end].iter().collect::<String>(), comment, false);
            index = end;
        } else if ch == '{' {
            let end = chars[index..]
                .iter()
                .position(|c| *c == '}')
                .map_or(chars.len(), |at| index + at + 1);
            push(&mut job, &chars[index..end].iter().collect::<String>(), comment, false);
            index = end;
        } else if ch == '\'' {
            let mut end = index + 1;
            while end < chars.len() {
                if chars[end] == '\'' {
                    if chars.get(end + 1) == Some(&'\'') {
                        end += 2;
                        continue;
                    }
                    end += 1;
                    break;
                }
                end += 1;
            }
            push(&mut job, &chars[index..end].iter().collect::<String>(), string, false);
            index = end;
        } else if ch.is_ascii_digit() {
            let mut end = index;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '.') {
                end += 1;
            }
            push(&mut job, &chars[index..end].iter().collect::<String>(), string, false);
            index = end;
        } else if ch.is_alphabetic() || ch == '_' || ch == '&' {
            let mut end = index;
            while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_' || chars[end] == '&') {
                end += 1;
            }
            let word: String = chars[index..end].iter().collect();
            let plain = word.trim_start_matches('&');
            let is_keyword = PASCAL_KEYWORDS.contains(&plain.to_lowercase().as_str());
            push(&mut job, &word, if is_keyword { keyword } else { default }, is_keyword);
            index = end;
        } else {
            push(&mut job, &ch.to_string(), default, false);
            index = start + 1;
        }
    }
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// `TSynPasSyn`'s keyword list (the reserved words the highlighter bolds).
const PASCAL_KEYWORDS: &[&str] = &[
    "absolute",
    "and",
    "array",
    "as",
    "asm",
    "begin",
    "case",
    "class",
    "const",
    "constructor",
    "destructor",
    "div",
    "do",
    "downto",
    "else",
    "end",
    "except",
    "exports",
    "file",
    "finalization",
    "finally",
    "for",
    "function",
    "goto",
    "if",
    "implementation",
    "in",
    "inherited",
    "initialization",
    "inline",
    "interface",
    "is",
    "label",
    "library",
    "mod",
    "nil",
    "not",
    "object",
    "of",
    "or",
    "out",
    "packed",
    "procedure",
    "program",
    "property",
    "raise",
    "record",
    "repeat",
    "resourcestring",
    "set",
    "shl",
    "shr",
    "string",
    "then",
    "threadvar",
    "to",
    "try",
    "type",
    "unit",
    "until",
    "uses",
    "var",
    "while",
    "with",
    "xor",
];

/// Draws the dialog and returns the command the shell runs.
pub fn show(ctx: &egui::Context, state: &mut ScriptFormState) -> Option<ScriptFormAction> {
    // FormShow's first list.
    if let Some(action) = state.show() {
        return Some(action);
    }
    let mut action = None;
    let mut open = true;
    egui::Window::new("Apply Script")
        .id(egui::Id::unique("frmScript"))
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(760.0, 640.0))
        .default_pos(egui::pos2(60.0, 60.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("&Filter");
                if ui
                    .add(egui::TextEdit::singleline(&mut state.filter).desired_width(360.0))
                    .changed()
                {
                    action = Some(state.filter_changed());
                }
                ui.add_space(8.0);
                ui.label("&Script");
                let items = state.items();
                let current = state
                    .selected
                    .and_then(|index| items.get(index).cloned())
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("cmbScripts")
                    .selected_text(current)
                    .width(360.0)
                    .show_ui(ui, |ui| {
                        for (index, item) in items.iter().enumerate() {
                            if ui.selectable_label(state.selected == Some(index), item).clicked() {
                                action = state.select(index);
                            }
                        }
                    });
                if ui.button("&Save").clicked() {
                    action = state.save();
                }
            });
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut state.subdirectories, "&Include scripts from subdirectories")
                    .changed()
                {
                    action = Some(state.relist());
                }
                if !state.new_script_path.is_empty() || state.current().as_deref() == Some(S_NEW_SCRIPT) {
                    ui.label("Save as:");
                    ui.add(egui::TextEdit::singleline(&mut state.new_script_path).desired_width(320.0));
                }
            });
            if let Some(message) = state.error.clone() {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
            // `Editor`: the Pascal highlighter and the Tab behaviour.
            let editor_id = egui::Id::unique("script-editor");
            let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, wrap_width: f32| {
                pascal_layout(ui, buffer.as_str(), wrap_width)
            };
            let height = (ui.available_height() - 60.0).max(120.0);
            let output = egui::TextEdit::multiline(&mut state.editor)
                .id(editor_id)
                .code_editor()
                .desired_rows(20)
                .desired_width(f32::INFINITY)
                .layouter(&mut layouter)
                .min_size(egui::vec2(ui.available_width(), height))
                .show(ui);
            let editor_response = output.response.clone();
            let focused = editor_response.has_focus();
            if focused && let Some(range) = output.cursor_range {
                let start = range.primary.index.0;
                let end = range.secondary.index.0;
                state.update_caret(start);
                let (shift, tab) = ui.input_mut(|input| {
                    let shift = input.key_pressed(egui::Key::Tab) && input.modifiers.shift;
                    let tab = shift || input.key_pressed(egui::Key::Tab);
                    if tab {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
                        input.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab);
                    }
                    (shift, tab)
                });
                if tab {
                    let before = state.editor.clone();
                    let (text, new_start, new_end) = tab_edit(&before, start, end, shift);
                    state.editor = text;
                    if state.editor != before {
                        state.modified = true;
                    }
                    let mut edit_state = output.state.clone();
                    edit_state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(new_start),
                        egui::text::CCursor::new(new_end),
                    )));
                    edit_state.store(ui.ctx(), editor_id);
                }
            }
            if editor_response.changed() {
                state.modified = true;
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(&state.position);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        action = Some(ScriptFormAction::Cancelled);
                    }
                    if ui.button("OK").clicked() {
                        action = state.ok();
                    }
                });
            });
        });
    // The two "modified, save it?" questions (`MessageDlg`).
    if let Some(confirm) = state.confirm {
        let mut answered = None;
        let caption = match confirm {
            ScriptConfirm::Switch { .. } => format!(
                "The previous script (\"{}\") has been modified. Do you want to save it before loading the new script?",
                state.save_override
            ),
            ScriptConfirm::Ok => "The script has been modified. Do you want to save it?".to_owned(),
        };
        let mut open = true;
        egui::Window::new("Confirm")
            .id(egui::Id::unique("frmScriptConfirm"))
            .collapsible(false)
            .resizable(false)
            .default_pos(egui::pos2(60.0, 60.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(caption.clone());
                ui.horizontal(|ui| {
                    if ui.button("Yes").clicked() {
                        answered = Some(true);
                    }
                    if ui.button("No").clicked() {
                        answered = Some(false);
                    }
                    if ui.button("Cancel").clicked() {
                        state.confirm = None;
                        answered = None;
                    }
                });
            });
        if let Some(save) = answered {
            action = match confirm {
                ScriptConfirm::Switch { .. } => state.confirm_switch(save),
                ScriptConfirm::Ok => state.confirm_ok(save),
            };
        }
        if !open {
            state.confirm = None;
        }
    }
    if !open && state.confirm.is_none() && action.is_none() {
        action = Some(ScriptFormAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ScriptFormState {
        let mut state = ScriptFormState::new(Some("M:\\scripts".to_owned()), String::new(), false);
        state.apply_list(ScriptListResponse {
            folder: "M:\\scripts".to_owned(),
            scripts: vec![
                ScriptInfo {
                    name: "B.pas".to_owned(),
                    lines: 1,
                },
                ScriptInfo {
                    name: "a.pas".to_owned(),
                    lines: 1,
                },
                ScriptInfo {
                    name: "_newscript_.pas".to_owned(),
                    lines: 1,
                },
                ScriptInfo {
                    name: "Sub\\C.pas".to_owned(),
                    lines: 1,
                },
            ],
        });
        state
    }

    #[test]
    fn the_combo_lists_the_new_script_then_subdirectories_then_names() {
        let state = state();
        assert_eq!(state.items(), vec!["<new script>", "Sub\\C", "a", "B"]);
        assert_eq!(state.folder.as_deref(), Some("M:\\scripts\\"));
    }

    #[test]
    fn the_filter_is_a_case_insensitive_substring() {
        let mut state = state();
        state.filter = "sub".to_owned();
        assert_eq!(state.items(), vec!["<new script>", "Sub\\C"]);
        state.filter = "B".to_owned();
        // `ContainsText` is a substring match: "Sub\C" holds the b too.
        assert_eq!(state.items(), vec!["<new script>", "Sub\\C", "B"]);
        state.filter = "nothing".to_owned();
        assert_eq!(state.items(), vec!["<new script>"]);
    }

    #[test]
    fn apply_list_keeps_the_last_used_script_or_falls_back_to_the_first() {
        let mut state = ScriptFormState::new(Some("M:\\scripts".to_owned()), "a".to_owned(), false);
        let action = state.apply_list(ScriptListResponse {
            folder: "M:\\scripts".to_owned(),
            scripts: vec![ScriptInfo {
                name: "a.pas".to_owned(),
                lines: 3,
            }],
        });
        assert_eq!(
            action,
            Some(ScriptFormAction::Read {
                file: "a.pas".to_owned()
            })
        );
        assert_eq!(state.save_override, "a");
        // A second list with the same selection asks for nothing.
        let action = state.apply_list(ScriptListResponse {
            folder: "M:\\scripts".to_owned(),
            scripts: vec![ScriptInfo {
                name: "a.pas".to_owned(),
                lines: 3,
            }],
        });
        assert_eq!(action, None);
    }

    #[test]
    fn switching_a_modified_script_asks_first() {
        let mut state = state();
        state.selected = Some(3);
        state.save_override = "B".to_owned();
        state.editor = "program B;".to_owned();
        state.modified = true;
        assert_eq!(state.select(2), None);
        assert_eq!(state.confirm, Some(ScriptConfirm::Switch { index: 2 }));
        // Yes saves the previous script and reads the new one.
        assert_eq!(
            state.confirm_switch(true),
            Some(ScriptFormAction::SaveThenRead {
                file: "M:\\scripts\\B.pas".to_owned(),
                text: "program B;".to_owned(),
                read: "a.pas".to_owned()
            })
        );
        assert!(state.confirm.is_none());
        // No reads the new script without saving.
        state.modified = true;
        state.confirm = Some(ScriptConfirm::Switch { index: 2 });
        assert_eq!(
            state.confirm_switch(false),
            Some(ScriptFormAction::Read {
                file: "a.pas".to_owned()
            })
        );
    }

    #[test]
    fn the_read_replaces_tabs_and_clears_modified() {
        let mut state = state();
        state.modified = true;
        state.apply_read(ScriptReadResponse {
            file: "M:\\scripts\\a.pas".to_owned(),
            script: "a".to_owned(),
            text: "begin\n\tx := 1;\nend.".to_owned(),
            lines: 3,
        });
        assert_eq!(state.editor, "begin\n  x := 1;\nend.");
        assert!(!state.modified);
        assert_eq!(state.position, "Line:1 Col:1");
    }

    #[test]
    fn save_uses_the_item_name_and_a_new_script_joins_the_list() {
        let mut state = state();
        state.selected = Some(2);
        assert_eq!(
            state.save(),
            Some(ScriptFormAction::Save {
                file: "M:\\scripts\\a.pas".to_owned(),
                text: String::new()
            })
        );
        // "<new script>" saves to the path the dialog gave.
        state.selected = Some(0);
        assert!(state.save().is_none());
        assert!(state.error.is_some());
        state.new_script_path = "M:\\scripts\\Fresh.pas".to_owned();
        assert_eq!(
            state.save(),
            Some(ScriptFormAction::Save {
                file: "M:\\scripts\\Fresh.pas".to_owned(),
                text: String::new()
            })
        );
        state.apply_save(ScriptSaveResponse {
            file: "M:\\scripts\\Fresh.pas".to_owned(),
            backup: None,
            bytes: 10,
            written: true,
            text: "text".to_owned(),
        });
        assert_eq!(state.position, "Saved: Fresh.pas");
        assert!(state.items().iter().any(|item| item == "Fresh"));
        assert_eq!(state.current().as_deref(), Some("Fresh"));
        assert_eq!(state.save_override, "Fresh");
    }

    #[test]
    fn ok_asks_about_unsaved_text_and_reports_the_editor_text() {
        let mut state = state();
        state.selected = Some(3);
        let action = state.ok().unwrap();
        assert_eq!(
            action,
            ScriptFormAction::Ok {
                name: "B".to_owned(),
                text: String::new()
            }
        );
        state.modified = true;
        state.editor = "changed".to_owned();
        assert_eq!(state.ok(), None);
        assert_eq!(state.confirm, Some(ScriptConfirm::Ok));
        assert_eq!(
            state.confirm_ok(false),
            Some(ScriptFormAction::Ok {
                name: "B".to_owned(),
                text: "changed".to_owned()
            })
        );
        state.confirm = Some(ScriptConfirm::Ok);
        assert_eq!(
            state.confirm_ok(true),
            Some(ScriptFormAction::SaveThenOk {
                file: "M:\\scripts\\B.pas".to_owned(),
                text: "changed".to_owned(),
                name: "B".to_owned()
            })
        );
    }

    #[test]
    fn the_tab_indents_and_dedents_like_the_form() {
        assert_eq!(indent_text("a\n\nb", "  "), "  a\n\n  b");
        assert_eq!(dedent_text("  a\nb\n    c", "  "), "a\nb\n  c");
        let (text, start, end) = tab_edit("x\ny\nz", 0, 5, false);
        assert_eq!(text, "  x\n  y\n  z");
        assert_eq!((start, end), (0, 11));
        let (text, ..) = tab_edit("  x", 3, 3, false);
        assert_eq!(text, "  x  ");
        let (text, ..) = tab_edit(&text, 3, 5, true);
        // The selection "  " dedents to "", which is what the form's
        // SelText replacement does.
        assert_eq!(text, "  x");
    }

    #[test]
    fn the_caret_is_one_based() {
        assert_eq!(caret_position("", 0), (1, 1));
        assert_eq!(caret_position("abc", 2), (1, 3));
        assert_eq!(caret_position("a\nbc", 4), (2, 3));
    }
}
