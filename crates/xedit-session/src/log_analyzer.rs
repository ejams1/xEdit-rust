// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeLogAnalyzerForm.pas

//! `logs.analyze`: the Log Analyzer. It reads the game's Papyrus log or the
//! RuntimeScriptProfiler log of Oblivion and lists the forms the log names,
//! grouped by the plugin of their load order slot, with the errors and
//! warnings (Papyrus) or the frames executed and the execution times
//! (RuntimeScriptProfiler). The form selection, the trees and the memo are
//! presentation; the file the dialog picks is the `file` parameter.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::interface::globals::{data_path, my_games_the_game_path};
use xedit_core::interface::{FormID, record_by_load_order_form_id};

use crate::commands::{RecordSummary, summary_of};
use crate::{CommandError, Registry, Session};

/// The logs the analyzer reads (`TLogType`).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogType {
    /// Oblivion's RuntimeScriptProfiler log.
    RuntimeScriptProfiler,
    /// The Papyrus logs of Skyrim and the games after it.
    #[default]
    Papyrus,
}

/// `logs.analyze`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogsAnalyzeRequest {
    /// The log to read.
    #[serde(default)]
    pub log: LogType,
    /// The log file; the game's default when omitted
    /// (`RuntimeScriptProfiler.log` in the data folder, or
    /// `<My Games>\<game>\Logs\Script\Papyrus.0.log`).
    pub file: Option<String>,
    /// Read only the first this many megabytes of the log, as the "first"
    /// edit of the form (50 when omitted).
    pub max_size_mb: Option<u32>,
}

/// One form of the log.
#[derive(Serialize, JsonSchema)]
pub struct LogEntryResult {
    /// The load order FormID as eight hexadecimal digits.
    pub form_id: String,
    /// The load order slot of the FormID; 255 for a message about the saved
    /// game.
    pub load_order: u8,
    /// The record the FormID resolves to, when it is in a loaded plugin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<RecordSummary>,
    /// The log lines of the form, joined by a blank line (empty for the
    /// RuntimeScriptProfiler, whose analyzer leaves the text unset).
    pub text: String,
    /// Papyrus: the errors of the form.
    pub errors: u64,
    /// Papyrus: the warnings of the form.
    pub warnings: u64,
    /// RuntimeScriptProfiler: the frames the script ran in.
    pub frames_executed: u64,
    /// RuntimeScriptProfiler: the total execution time in milliseconds.
    pub total_time_ms: f64,
    /// RuntimeScriptProfiler: the longest execution in milliseconds.
    pub max_execution_time_ms: f64,
}

/// One plugin of the log, the totals of its forms.
#[derive(Serialize, JsonSchema)]
pub struct LogPluginResult {
    /// The load order slot of the plugin.
    pub load_order: u8,
    /// The tree's text: the file name, `Unknown`, or `[FF] Saved game`.
    pub label: String,
    /// The file of the plugin when a form of it resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub entries: usize,
    pub errors: u64,
    pub warnings: u64,
    pub frames_executed: u64,
    pub total_time_ms: f64,
    pub max_execution_time_ms: f64,
}

/// `logs.analyze`: the response.
#[derive(Serialize, JsonSchema)]
pub struct LogsAnalyzeResponse {
    /// The log read.
    pub log: LogType,
    /// The path of the log.
    pub file: String,
    /// The forms, by load order slot and FormID, as the sorted tree shows
    /// them.
    pub entries: Vec<LogEntryResult>,
    /// The plugins, by load order.
    pub plugins: Vec<LogPluginResult>,
    /// The FormIDs the analyzer ignored because no loaded record has them.
    pub unknown_form_ids: u64,
    /// The message log lines of the analysis.
    pub messages: Vec<String>,
}

/// The accumulation of the release's `TLogEntry`.
#[derive(Default)]
struct Counts {
    load_order: u8,
    record: Option<RecordSummary>,
    text: String,
    value1: f64,
    value2: f64,
    value3: f64,
}

/// Port of `TTexStream.ReadLn`: the lines of `data`, split at CR, LF or the
/// pair, within the first `limit` bytes.
fn lines(data: &str, limit: Option<usize>) -> Vec<String> {
    let data = match limit {
        Some(limit) if data.len() > limit => {
            let mut end = limit;
            while end > 0 && !data.is_char_boundary(end) {
                end -= 1;
            }
            &data[..end]
        }
        _ => data,
    };
    let mut result = Vec::new();
    let mut current = String::new();
    let mut chars = data.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' | '\n' => {
                result.push(std::mem::take(&mut current));
                if let Some(next) = chars.peek()
                    && (*next == '\r' || *next == '\n')
                    && *next != c
                {
                    chars.next();
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

/// `IsHexStr`.
fn is_hex_str(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
}

/// Port of `BracketedFormIDFromString`: the eight hex digits between the
/// brackets, such as `(0012ABCD)`.
fn bracketed_form_id(text: &str) -> String {
    let bytes: Vec<char> = text.chars().collect();
    for index in 0..bytes.len().saturating_sub(9) {
        if bytes[index] == '(' && bytes[index + 9] == ')' {
            let candidate: String = bytes[index + 1..index + 9].iter().collect();
            if is_hex_str(&candidate) {
                return candidate;
            }
        }
    }
    String::new()
}

/// One entry of the analysis, with its FormID.
fn entry_id(form_id: &str) -> FormID {
    FormID::from_str_def(form_id, 0)
}

fn parse_papyrus_line(line: &str, entries: &mut Vec<(FormID, Counts)>, unknown: &mut u64, messages: &mut Vec<String>) {
    let txt: String = line.chars().skip(26).collect::<String>().trim().to_string();
    let is_error = txt.starts_with("error:");
    let is_warning = txt.starts_with("warning:");
    if !(is_error || is_warning) {
        return;
    }
    let s = bracketed_form_id(&txt);
    let form_id = if s.is_empty() {
        if txt.contains("[None].")
            || txt.contains("in the type table in save")
            || txt.contains("referenced by the save game")
        {
            FormID::from_cardinal(0xFF00_0000)
        } else {
            return;
        }
    } else {
        entry_id(&s)
    };
    if let Some((_, counts)) = entries.iter_mut().find(|(known, _)| *known == form_id) {
        if is_error {
            counts.value1 += 1.0;
        } else if is_warning {
            counts.value2 += 1.0;
        }
        if !counts.text.contains(&txt) {
            counts.text.push_str("\r\n\r\n");
            counts.text.push_str(&txt);
        }
        return;
    }
    let record = record_by_load_order_form_id(form_id, None);
    let load_order = form_id.file_id().full_slot();
    if load_order != 0xFF && record.is_none() {
        if *unknown == 0 {
            messages.push(format!(
                "Unknown FormID [{}], changed load order? All other unknown forms will be ignored.",
                s
            ));
        }
        *unknown += 1;
        return;
    }
    entries.push((
        form_id,
        Counts {
            load_order: u8::try_from(load_order).unwrap_or(0xFF),
            record: record.as_ref().map(summary_of),
            text: txt,
            value1: u64::from(is_error) as f64,
            value2: u64::from(is_warning) as f64,
            value3: 0.0,
        },
    ));
}

fn parse_profiler_line(line: &str, entries: &mut Vec<(FormID, Counts)>, unknown: &mut u64, messages: &mut Vec<String>) {
    let s: String = line.chars().take(8).collect();
    if !is_hex_str(&s) {
        return;
    }
    let form_id = entry_id(&s);
    let rest: String = line.chars().skip(17).collect();
    let time_text: String = rest.split(' ').next().unwrap_or_default().to_string();
    let time: f64 = time_text.trim().parse().unwrap_or(0.0);
    if let Some((_, counts)) = entries.iter_mut().find(|(known, _)| *known == form_id) {
        counts.value1 += 1.0;
        counts.value2 += time;
        if time > counts.value3 {
            counts.value3 = time;
        }
        return;
    }
    let record = record_by_load_order_form_id(form_id, None);
    let load_order = form_id.file_id().full_slot();
    if load_order != 0xFF && record.is_none() {
        if *unknown == 0 {
            messages.push(format!(
                "Unknown FormID [{}], changed load order? All other unknown forms will be ignored.",
                s
            ));
        }
        *unknown += 1;
        return;
    }
    entries.push((
        form_id,
        Counts {
            load_order: u8::try_from(load_order).unwrap_or(0xFF),
            record: record.as_ref().map(summary_of),
            // UPSTREAM-QUIRK: `txt` is never set in
            // `ParseRuntimeScriptProfilerData`, so the release shows no text.
            text: String::new(),
            value1: 1.0,
            value2: time,
            value3: time,
        },
    ));
}

fn logs_analyze(session: &mut Session, request: LogsAnalyzeRequest) -> Result<LogsAnalyzeResponse, CommandError> {
    session.mode()?;
    let file = request.file.clone().unwrap_or_else(|| match request.log {
        LogType::RuntimeScriptProfiler => {
            let data = data_path();
            let data = data.trim_end_matches(['\\', '/']);
            format!("{data}\\RuntimeScriptProfiler.log")
        }
        LogType::Papyrus => format!("{}Logs\\Script\\Papyrus.0.log", my_games_the_game_path()),
    });
    let mut messages = Vec::new();
    let data = match std::fs::read_to_string(&file) {
        Ok(data) => data,
        Err(_) => {
            messages.push("File does not exist".to_owned());
            return Ok(LogsAnalyzeResponse {
                log: request.log,
                file,
                entries: Vec::new(),
                plugins: Vec::new(),
                unknown_form_ids: 0,
                messages,
            });
        }
    };
    messages.push("Processing log...".to_owned());
    let limit = request.max_size_mb.unwrap_or(50) as usize * 1024 * 1024;
    let mut entries: Vec<(FormID, Counts)> = Vec::new();
    let mut unknown = 0;
    match request.log {
        LogType::Papyrus => {
            let mut entry = String::new();
            for line in lines(&data, Some(limit)) {
                if line.len() > 25 && line.starts_with('[') && line.as_bytes().get(1).is_some_and(u8::is_ascii_digit) {
                    if !entry.is_empty() {
                        parse_papyrus_line(&entry, &mut entries, &mut unknown, &mut messages);
                    }
                    entry = line;
                } else {
                    if !entry.is_empty() {
                        entry.push_str("\r\n");
                    }
                    entry.push_str(&line);
                }
            }
            if !entry.is_empty() {
                parse_papyrus_line(&entry, &mut entries, &mut unknown, &mut messages);
            }
        }
        LogType::RuntimeScriptProfiler => {
            for line in lines(&data, Some(limit)) {
                if line.len() > 9 && line.as_bytes().get(8) == Some(&b'\t') {
                    parse_profiler_line(&line, &mut entries, &mut unknown, &mut messages);
                }
            }
        }
    }
    // `BuildPluginsList`: the totals per load order slot, the slots sorted.
    let mut plugins: HashMap<u8, LogPluginResult> = HashMap::new();
    for (_, counts) in &entries {
        let plugin = plugins.entry(counts.load_order).or_insert_with(|| LogPluginResult {
            load_order: counts.load_order,
            label: if counts.load_order == 0xFF {
                "[FF] Saved game".to_owned()
            } else {
                counts
                    .record
                    .as_ref()
                    .map(|record| record.file.clone())
                    .unwrap_or_else(|| "Unknown".to_owned())
            },
            file: counts.record.as_ref().map(|record| record.file.clone()),
            entries: 0,
            errors: 0,
            warnings: 0,
            frames_executed: 0,
            total_time_ms: 0.0,
            max_execution_time_ms: 0.0,
        });
        plugin.entries += 1;
        match request.log {
            LogType::Papyrus => {
                plugin.errors += counts.value1 as u64;
                plugin.warnings += counts.value2 as u64;
            }
            LogType::RuntimeScriptProfiler => {
                plugin.frames_executed += counts.value1 as u64;
                plugin.total_time_ms += counts.value2;
                plugin.max_execution_time_ms += counts.value3;
            }
        }
    }
    let mut plugins: Vec<LogPluginResult> = plugins.into_values().collect();
    plugins.sort_by_key(|plugin| plugin.load_order);
    entries.sort_by_key(|(form_id, counts)| (counts.load_order, form_id.to_cardinal()));
    let entries: Vec<LogEntryResult> = entries
        .into_iter()
        .map(|(form_id, counts)| {
            let (errors, warnings, frames_executed, total_time_ms, max_execution_time_ms) = match request.log {
                LogType::Papyrus => (counts.value1 as u64, counts.value2 as u64, 0, 0.0, 0.0),
                LogType::RuntimeScriptProfiler => (0, 0, counts.value1 as u64, counts.value2, counts.value3),
            };
            LogEntryResult {
                form_id: form_id.to_string(false),
                load_order: counts.load_order,
                record: counts.record,
                text: counts.text,
                errors,
                warnings,
                frames_executed,
                total_time_ms,
                max_execution_time_ms,
            }
        })
        .collect();
    if unknown > 0 {
        messages.push(format!("Ignored {unknown} unknown FormIDs"));
    }
    messages.push("Done.".to_owned());
    messages.push("Hint: double click on FormID to jump to that record".to_owned());
    Ok(LogsAnalyzeResponse {
        log: request.log,
        file,
        entries,
        plugins,
        unknown_form_ids: unknown,
        messages,
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "logs.analyze",
        "Read the game's Papyrus or RuntimeScriptProfiler log and list the forms it names (the Log Analyzer).",
        false,
        logs_analyze,
    );
}
