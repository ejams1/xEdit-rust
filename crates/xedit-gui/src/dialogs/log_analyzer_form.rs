// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeLogAnalyzerForm.pas (FormCreate, FormShow,
// btnFileSelectClick, btnAnalyzeClick, vstFormsInitNode,
// vstFormsInitChildren, vstFormsGetText, vstFormsChange,
// vstFormsDblClick, vstFormsCompareNodes, vstFormsHeaderClick) and
// xEdit/xeLogAnalyzerForm.dfm (pnlTop with edLogFile, btnFileSelect,
// btnAnalyze, edLogSize and the MB label; pnlClient with vstForms,
// Splitter1 and memoText)

//! `TfrmLogAnalyzer`: the log file edit with its Analyze button, the tree
//! of the plugin totals and the log lines of each form, and the memo of the
//! selected entry. The analysis itself is `logs.analyze`; the dialog keeps
//! the response and builds the tree from it.

use eframe::egui;

use xedit_session::log_analyzer::{LogEntryResult, LogPluginResult, LogType, LogsAnalyzeResponse};

use crate::dialogs::viewers::{SplitOrientation, SplitViewer};
use crate::widgets::conflict::ConflictColors;
use crate::widgets::virtual_tree::{TreeCell, TreeColumn, TreeRow, TreeSource, TreeState};

/// What the dialog asks the shell to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogAnalyzerAction {
    /// `btnAnalyzeClick`: `logs.analyze` over the file and the size.
    Analyze {
        file: String,
        max_size_mb: u32,
    },
    /// `vstFormsDblClick`: the record to show (`entries[].record`).
    JumpTo {
        form_id: String,
        file: String,
    },
    Closed,
}

/// One row of the flattened tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogRow {
    Plugin(usize),
    Entry { plugin: usize, entry: usize },
}

/// `TfrmLogAnalyzer`'s state.
#[derive(Clone, Debug)]
pub struct LogAnalyzerState {
    /// `ltLog`.
    pub log: LogType,
    /// `edLogFile.Text`.
    pub file: String,
    /// `edLogSize.Text`: the MB read (0 means the whole file,
    /// `StrToIntDef(edLogSize.Text, 0)`).
    pub max_size_mb: String,
    pub analysis: Option<LogsAnalyzeResponse>,
    /// `memoText.Lines`.
    pub messages: Vec<String>,
    /// The memo of the selected entry (`vstFormsChange`).
    pub memo: String,
    pub expanded: Vec<bool>,
    pub selected: Option<usize>,
    /// `vstForms.Header.SortColumn` and `SortDirection`.
    pub sort_column: usize,
    pub sort_descending: bool,
    pub error: Option<String>,
    pub tree: TreeState,
    /// `vstForms.Height`, the splitter above the memo.
    pub split: f32,
}

impl LogAnalyzerState {
    /// `FormCreate` and `FormShow`: the default file of the log type, the
    /// hints and the column titles. `l_data_path` and
    /// `l_my_games_the_game_path` are the session's; without a response the
    /// suggestion comes from the command's own defaults when Analyze runs.
    pub fn new(log: LogType) -> Self {
        let mut state = Self {
            log,
            file: String::new(),
            max_size_mb: "50".to_owned(),
            analysis: None,
            messages: Vec::new(),
            memo: String::new(),
            expanded: Vec::new(),
            selected: None,
            sort_column: 0,
            sort_descending: false,
            error: None,
            tree: TreeState::new(vec![100.0, 469.0, 100.0, 100.0]),
            split: 265.0,
        };
        match log {
            LogType::RuntimeScriptProfiler => {
                state.messages.push("RuntimeScriptProfiler by ShadeMe".to_owned());
                state
                    .messages
                    .push("http://www.oblivion.nexusmods.com/mods/41863".to_owned());
            }
            LogType::Papyrus => {
                state.messages.push("How to enable logging".to_owned());
                state
                    .messages
                    .push("http://www.creationkit.com/FAQ:_My_Script_Doesn't_Work!".to_owned());
                state.messages.push("Papyrus errors description".to_owned());
                state
                    .messages
                    .push("http://www.creationkit.com/Papyrus_Runtime_Errors".to_owned());
            }
        }
        state
    }

    /// `FormShow`'s `edLogFile.Text` from the session paths.
    pub fn set_default_file(&mut self, data_path: &str, my_games_path: &str) {
        self.file = match self.log {
            LogType::RuntimeScriptProfiler => {
                let data = data_path.trim_end_matches(['\\', '/']);
                format!("{data}\\RuntimeScriptProfiler.log")
            }
            LogType::Papyrus => format!("{my_games_path}Logs\\Script\\Papyrus.0.log"),
        };
    }

    /// The header texts `FormShow` changes: `Value1`/`Value2` become the
    /// counters of the log type.
    pub fn value_titles(&self) -> [&'static str; 2] {
        match self.log {
            LogType::RuntimeScriptProfiler => ["Frames Executed", "Total Time, ms"],
            LogType::Papyrus => ["Errors", "Warnings"],
        }
    }

    /// `btnAnalyzeClick`'s action, or the "file does not exist" memo line.
    pub fn analyze(&mut self) -> Option<LogAnalyzerAction> {
        if self.file.is_empty() || !std::path::Path::new(&self.file).is_file() {
            self.messages.push("File does not exist".to_owned());
            return None;
        }
        // `MaxLogSize := StrToIntDef(edLogSize.Text, 0)*1024*1024`.
        let megabytes = self.max_size_mb.trim().parse::<u32>().unwrap_or(0);
        self.messages.clear();
        self.messages.push("Processing log...".to_owned());
        self.memo.clear();
        self.analysis = None;
        self.expanded.clear();
        self.selected = None;
        self.error = None;
        Some(LogAnalyzerAction::Analyze {
            file: self.file.clone(),
            max_size_mb: megabytes,
        })
    }

    /// The response of `logs.analyze`: the trees, the memo and the counts.
    pub fn apply_analysis(&mut self, analysis: LogsAnalyzeResponse) {
        self.expanded = vec![false; analysis.plugins.len()];
        self.messages = analysis.messages.clone();
        self.error = None;
        self.analysis = Some(analysis);
    }

    fn plugins(&self) -> &[LogPluginResult] {
        self.analysis
            .as_ref()
            .map(|analysis| analysis.plugins.as_slice())
            .unwrap_or(&[])
    }

    fn entries(&self) -> &[LogEntryResult] {
        self.analysis
            .as_ref()
            .map(|analysis| analysis.entries.as_slice())
            .unwrap_or(&[])
    }

    /// The entries of a plugin in the tree's current sort order
    /// (`vstFormsCompareNodes` and `vstFormsHeaderClick`).
    fn plugin_entries(&self, plugin: usize) -> Vec<usize> {
        let Some(target) = self.plugins().get(plugin) else {
            return Vec::new();
        };
        let mut indices: Vec<usize> = self
            .entries()
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.load_order == target.load_order)
            .map(|(index, _)| index)
            .collect();
        let entries = self.entries();
        indices.sort_by(|&left, &right| {
            let (a, b) = (&entries[left], &entries[right]);
            let ordering = match self.sort_column {
                0 => a.form_id.cmp(&b.form_id),
                1 => a
                    .record
                    .as_ref()
                    .map(|record| &record.name)
                    .cloned()
                    .unwrap_or_default()
                    .to_lowercase()
                    .cmp(
                        &b.record
                            .as_ref()
                            .map(|record| &record.name)
                            .cloned()
                            .unwrap_or_default()
                            .to_lowercase(),
                    ),
                2 => a.errors.cmp(&b.errors).then(a.frames_executed.cmp(&b.frames_executed)),
                _ => a
                    .warnings
                    .cmp(&b.warnings)
                    .then(a.total_time_ms.total_cmp(&b.total_time_ms)),
            };
            if self.sort_descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
        indices
    }

    /// The visible rows: a plugin node, then its entries when expanded.
    pub fn rows(&self) -> Vec<LogRow> {
        let mut rows = Vec::new();
        for plugin in 0..self.plugins().len() {
            rows.push(LogRow::Plugin(plugin));
            if self.expanded.get(plugin).copied().unwrap_or(false) {
                for entry in self.plugin_entries(plugin) {
                    rows.push(LogRow::Entry { plugin, entry });
                }
            }
        }
        rows
    }

    /// `vstFormsHeaderClick`: the same column flips the direction, a new
    /// one sorts ascending.
    pub fn header_clicked(&mut self, column: usize) {
        if self.sort_column == column {
            self.sort_descending = !self.sort_descending;
        } else {
            self.sort_column = column;
            self.sort_descending = false;
        }
    }

    /// `vstFormsChange`: the memo of the selected row. The Papyrus memo is
    /// the entry's log lines. The profiler shows the execution time and the
    /// record's `SCTX`, which the command does not return (UPSTREAM-QUIRK:
    /// the memo keeps the time line alone).
    pub fn select(&mut self, row: usize) {
        self.selected = Some(row);
        let rows = self.rows();
        self.memo = match rows.get(row).copied() {
            Some(LogRow::Plugin(plugin)) => self
                .plugins()
                .get(plugin)
                .map(|plugin| plugin.label.clone())
                .unwrap_or_default(),
            Some(LogRow::Entry { entry, .. }) => {
                let entry = &self.entries()[entry];
                match self.log {
                    LogType::RuntimeScriptProfiler => format!(
                        "; RuntimeScriptProfiler - max execution time: {} ms",
                        format_number(entry.max_execution_time_ms)
                    ),
                    LogType::Papyrus => entry.text.clone(),
                }
            }
            None => String::new(),
        };
    }

    /// `vstFormsDblClick`: the record the row's entry names.
    pub fn double_clicked(&self, row: usize) -> Option<LogAnalyzerAction> {
        let rows = self.rows();
        let Some(LogRow::Entry { entry, .. }) = rows.get(row).copied() else {
            return None;
        };
        let entry = &self.entries()[entry];
        let record = entry.record.as_ref()?;
        Some(LogAnalyzerAction::JumpTo {
            form_id: record.form_id.clone(),
            file: record.file.clone(),
        })
    }
}

/// `Format('%.0n', [Round(value) + 0.0])`: the thousands separator.
fn format_number(value: f64) -> String {
    let rounded = value.round() as i64;
    let negative = rounded < 0;
    let digits = rounded.unsigned_abs().to_string();
    let mut result = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    if negative {
        result.insert(0, '-');
    }
    result
}

/// The `vstForms` source over the current state.
struct LogTree<'a> {
    rows: &'a [LogRow],
    analysis: Option<&'a LogsAnalyzeResponse>,
    expanded: &'a [bool],
    log: LogType,
    columns: Vec<TreeColumn>,
}

impl LogTree<'_> {
    fn plugins(&self) -> &[LogPluginResult] {
        self.analysis.map(|analysis| analysis.plugins.as_slice()).unwrap_or(&[])
    }

    fn entries(&self) -> &[LogEntryResult] {
        self.analysis.map(|analysis| analysis.entries.as_slice()).unwrap_or(&[])
    }

    fn counts(&self, plugin: &LogPluginResult, entry: Option<&LogEntryResult>) -> (String, String) {
        match self.log {
            LogType::Papyrus => (
                format_number(match entry {
                    Some(entry) => entry.errors as f64,
                    None => plugin.errors as f64,
                }),
                format_number(match entry {
                    Some(entry) => entry.warnings as f64,
                    None => plugin.warnings as f64,
                }),
            ),
            LogType::RuntimeScriptProfiler => (
                format_number(match entry {
                    Some(entry) => entry.frames_executed as f64,
                    None => plugin.frames_executed as f64,
                }),
                format_number(match entry {
                    Some(entry) => entry.total_time_ms,
                    None => plugin.total_time_ms,
                }),
            ),
        }
    }
}

impl TreeSource for LogTree<'_> {
    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn columns(&self) -> &[TreeColumn] {
        &self.columns
    }

    fn row(&self, index: usize, out: &mut TreeRow) {
        let Some(row) = self.rows.get(index).copied() else {
            return;
        };
        match row {
            LogRow::Plugin(plugin) => {
                let Some(entry) = self.plugins().get(plugin) else {
                    return;
                };
                let (value1, value2) = self.counts(entry, None);
                out.id = plugin as u64;
                out.depth = 0;
                out.has_children = entry.entries > 0;
                out.expanded = self.expanded.get(plugin).copied().unwrap_or(false);
                out.cells = vec![
                    TreeCell::new(""),
                    TreeCell::new(entry.label.clone()),
                    TreeCell::new(value1),
                    TreeCell::new(value2),
                ];
            }
            LogRow::Entry { entry, .. } => {
                let Some(entry) = self.entries().get(entry) else {
                    return;
                };
                let plugin = self
                    .plugins()
                    .iter()
                    .find(|plugin| plugin.load_order == entry.load_order);
                let (value1, value2) = match plugin {
                    Some(plugin) => self.counts(plugin, Some(entry)),
                    None => (String::new(), String::new()),
                };
                out.id = (1_000_000 + index) as u64;
                out.depth = 1;
                out.cells = vec![
                    // UPSTREAM-QUIRK: `FormID.ToString(True)` of the release
                    // spaces an ESL FormID (`FE 001 ABC`); the command
                    // reports the eight-digit load order FormID.
                    TreeCell::new(entry.form_id.clone()),
                    TreeCell::new(
                        entry
                            .record
                            .as_ref()
                            .map(|record| record.name.clone())
                            .unwrap_or_default(),
                    ),
                    TreeCell::new(value1),
                    TreeCell::new(value2),
                ];
            }
        }
    }
}

impl LogAnalyzerState {
    /// `vstFormsInitNode`'s `ivsHasChildren` toggle: a double click on a
    /// plugin row expands or collapses it.
    pub fn toggle_row(&mut self, row: usize) {
        let rows = self.rows();
        if let Some(LogRow::Plugin(plugin)) = rows.get(row).copied()
            && let Some(slot) = self.expanded.get_mut(plugin)
        {
            *slot = !*slot;
        }
    }
}

/// Draws the analyzer. Returns the command the shell runs.
pub fn show(ctx: &egui::Context, state: &mut LogAnalyzerState) -> Option<LogAnalyzerAction> {
    let mut action = None;
    let mut open = true;
    egui::Window::new("Log Analyzer")
        .id(egui::Id::unique("frmLogAnalyzer"))
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(900.0, 600.0))
        .default_pos(egui::pos2(60.0, 60.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Log file:");
                ui.add(egui::TextEdit::singleline(&mut state.file).desired_width(400.0));
                // `btnFileSelect` (the file open dialog of step 5).
                if ui.button("...").on_hover_text("Choose the log file").clicked() {
                    state.error = Some(
                        "The file selection dialog arrives with phase 7 step 5; type the path instead.".to_owned(),
                    );
                }
                if ui.button("Analyze").clicked() {
                    action = state.analyze();
                }
                ui.label("MB:");
                ui.add(egui::TextEdit::singleline(&mut state.max_size_mb).desired_width(40.0));
            });
            if let Some(message) = state.error.clone() {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
            ui.separator();
            let [value1, value2] = state.value_titles();
            let columns = vec![
                TreeColumn::new("FormID", 100.0),
                TreeColumn::new("Name", 469.0),
                TreeColumn::new(value1, 100.0),
                TreeColumn::new(value2, 100.0),
            ];
            let selected = {
                let rows = state.rows();
                let (analysis, expanded, tree, memo, log, split) = (
                    state.analysis.as_ref(),
                    state.expanded.as_slice(),
                    &mut state.tree,
                    state.memo.as_str(),
                    state.log,
                    &mut state.split,
                );
                let mut source = LogTree {
                    rows: &rows,
                    analysis,
                    expanded,
                    log,
                    columns,
                };
                SplitViewer {
                    id: "vstForms",
                    source: &mut source,
                    state: tree,
                    memo,
                    orientation: SplitOrientation::TreeTop,
                    split,
                    colors: &ConflictColors::default(),
                    header: true,
                }
                .show(ui)
            };
            if let Some(row) = selected.clicked {
                state.select(row);
            }
            if let Some(row) = selected.double_clicked {
                // `toToggleOnDblClick`: a plugin row expands, an entry row
                // jumps to its record.
                if matches!(state.rows().get(row), Some(LogRow::Plugin(_))) {
                    state.toggle_row(row);
                } else if let Some(jump) = state.double_clicked(row) {
                    action = Some(jump);
                }
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("log-analyzer-messages")
                .max_height(80.0)
                .show(ui, |ui| {
                    for line in &state.messages {
                        ui.label(line);
                    }
                });
            ui.horizontal(|ui| {
                for (column, title) in ["FormID", "Name", value1, value2].iter().enumerate() {
                    if ui.small_button(*title).clicked() {
                        state.header_clicked(column);
                    }
                }
            });
        });
    if !open && action.is_none() {
        action = Some(LogAnalyzerAction::Closed);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response() -> LogsAnalyzeResponse {
        serde_json::from_value(json!({
            "log": "papyrus",
            "file": "Papyrus.0.log",
            "entries": [
                { "form_id": "0000003C", "load_order": 0, "text": "error: boom",
                  "errors": 2, "warnings": 0, "frames_executed": 0,
                  "total_time_ms": 0.0, "max_execution_time_ms": 0.0,
                  "record": { "form_id": "0000003C", "signature": "QUST",
                              "editor_id": "MQ01", "name": "Main Quest", "file": "Skyrim.esm" } },
                { "form_id": "00000042", "load_order": 0, "text": "warning: hmm",
                  "errors": 0, "warnings": 1, "frames_executed": 0,
                  "total_time_ms": 0.0, "max_execution_time_ms": 0.0 }
            ],
            "plugins": [
                { "load_order": 0, "label": "Skyrim.esm", "file": "Skyrim.esm",
                  "entries": 2, "errors": 2, "warnings": 1,
                  "frames_executed": 0, "total_time_ms": 0.0, "max_execution_time_ms": 0.0 },
                { "load_order": 255, "label": "[FF] Saved game",
                  "entries": 0, "errors": 0, "warnings": 0,
                  "frames_executed": 0, "total_time_ms": 0.0, "max_execution_time_ms": 0.0 }
            ],
            "unknown_form_ids": 0,
            "messages": ["Processing log...", "Done."]
        }))
        .unwrap()
    }

    #[test]
    fn the_tree_starts_with_the_plugin_nodes_collapsed() {
        let mut state = LogAnalyzerState::new(LogType::Papyrus);
        state.apply_analysis(response());
        let rows = state.rows();
        assert_eq!(rows, vec![LogRow::Plugin(0), LogRow::Plugin(1)]);
        assert_eq!(state.tree.row_height, 18.0);
    }

    #[test]
    fn an_expanded_plugin_lists_its_entries_sorted_by_form_id() {
        let mut state = LogAnalyzerState::new(LogType::Papyrus);
        state.apply_analysis(response());
        state.toggle_row(0);
        let rows = state.rows();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[1], LogRow::Entry { plugin: 0, entry: 0 });
        assert_eq!(rows[2], LogRow::Entry { plugin: 0, entry: 1 });
        // `vstFormsHeaderClick`: the same column flips, a new column starts
        // ascending.
        state.header_clicked(0);
        assert!(state.sort_descending);
        let rows = state.rows();
        assert_eq!(rows[1], LogRow::Entry { plugin: 0, entry: 1 });
        state.header_clicked(1);
        assert_eq!(state.sort_column, 1);
        assert!(!state.sort_descending);
    }

    #[test]
    fn selecting_a_row_fills_the_memo() {
        let mut state = LogAnalyzerState::new(LogType::Papyrus);
        state.apply_analysis(response());
        state.toggle_row(0);
        state.select(0);
        assert_eq!(state.memo, "Skyrim.esm");
        state.select(1);
        assert_eq!(state.memo, "error: boom");
        state.select(2);
        assert_eq!(state.memo, "warning: hmm");
    }

    #[test]
    fn a_profiler_entry_shows_its_time_line() {
        let mut state = LogAnalyzerState::new(LogType::RuntimeScriptProfiler);
        let mut analysis = response();
        analysis.log = LogType::RuntimeScriptProfiler;
        state.apply_analysis(analysis);
        state.toggle_row(0);
        state.select(1);
        assert!(state.memo.contains("max execution time"));
        assert_eq!(state.value_titles(), ["Frames Executed", "Total Time, ms"]);
    }

    #[test]
    fn a_missing_file_is_the_upstream_memo_line() {
        let mut state = LogAnalyzerState::new(LogType::Papyrus);
        state.file = "M:\\no-such-log.txt".to_owned();
        assert_eq!(state.analyze(), None);
        assert_eq!(state.messages.last().unwrap(), "File does not exist");
    }

    #[test]
    fn analyze_reports_the_file_and_size() {
        let path = std::env::temp_dir().join("xedit-gui-log-analyzer-test.log");
        std::fs::write(&path, "").unwrap();
        let mut state = LogAnalyzerState::new(LogType::Papyrus);
        state.file = path.display().to_string();
        state.max_size_mb = "12".to_owned();
        let action = state.analyze();
        assert_eq!(
            action,
            Some(LogAnalyzerAction::Analyze {
                file: path.display().to_string(),
                max_size_mb: 12
            })
        );
        assert_eq!(state.messages, vec!["Processing log..."]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_double_click_answers_the_record_to_jump_to() {
        let mut state = LogAnalyzerState::new(LogType::Papyrus);
        state.apply_analysis(response());
        state.toggle_row(0);
        assert_eq!(
            state.double_clicked(1),
            Some(LogAnalyzerAction::JumpTo {
                form_id: "0000003C".to_owned(),
                file: "Skyrim.esm".to_owned()
            })
        );
        // An entry without a loaded record has nothing to jump to.
        assert_eq!(state.double_clicked(2), None);
        assert_eq!(state.double_clicked(0), None);
    }

    #[test]
    fn the_counts_use_the_thousands_separator() {
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(999.4), "999");
        assert_eq!(format_number(1234567.0), "1,234,567");
    }
}
