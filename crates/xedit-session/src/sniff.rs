// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/frmMain.pas (the automation mode),
// Sniff/SniffProcessor.pas

//! The commands of Sniff: `sniff.run` runs an operation on the NIF, KF and
//! material files of a folder or an archive as Sniff's automation mode does
//! (`-OP:`), and `sniff.list` lists the operations with their settings.
//! Neither needs a loaded plugin.

use std::collections::BTreeMap;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_assets::sniff::main_form::{RunError, RunOptions, proc_infos, run};
use xedit_assets::sniff::processor::{FileStatus, MemIniFile, OptionKind, same_text, string_list_file_bytes};
use xedit_assets::sniff::procs::PROCS;

use crate::save::write_atomically;
use crate::{CommandError, NoParams, Registry, Session};

/// `sniff.run`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SniffRunRequest {
    /// The title of the operation, any case, such as `Update tangents and
    /// binormals` or `Universal tweaker` (`sniff.list` names them all).
    pub operation: String,
    /// A folder, or an archive (BSA, BA2), with the files to process.
    pub input: String,
    /// The folder that exists to write the changed files to, below their
    /// paths in the input. Needed unless the operation only reports.
    pub output: Option<String>,
    /// A settings ini in Sniff's form: the section named after the
    /// operation's title without spaces (`[Universaltweaker]`) holds its
    /// settings, `[Main]` the defaults of the other parameters.
    pub settings: Option<String>,
    /// Settings of the operation's section, as name and value, over the
    /// ones of `settings` (`{"sPath": "Alpha", "sValue": "0.5"}`); the
    /// names and defaults are in `sniff.list`. `ProcessedFiles` sets the
    /// file masks (`*.nif, *.kf`).
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    /// Only the files whose path holds this text, any case (`-P:`).
    pub path_contains: Option<String>,
    /// Include the subfolders of an input folder (`-subdir:`); default true.
    pub subdir: Option<bool>,
    /// Report a file that fails and go on, instead of stopping at it
    /// (`-skip:`); default false.
    pub skip_on_errors: Option<bool>,
    /// Write the unchanged files too (`-all:`); default false.
    pub copy_all: Option<bool>,
    /// Threads that process files; 0 or none for the CPU count less one.
    /// The results are the same for every count.
    pub threads: Option<i32>,
    /// A file to write the messages to, as Sniff's `-LOG:` writes them.
    pub log: Option<String>,
    /// Process every file and report what would change, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// A file the run wrote or skipped.
#[derive(Serialize, JsonSchema)]
pub struct SniffFile {
    /// The path below the input (and the output).
    pub file: String,
    /// `updated`, `unchanged` (written because `copy_all`) or `skipped`.
    pub status: String,
    /// The error of a skipped file.
    pub error: Option<String>,
}

/// `sniff.run`: the response.
#[derive(Serialize, JsonSchema)]
pub struct SniffRunResponse {
    /// The title of the operation.
    pub operation: String,
    /// The messages of the run as Sniff shows them: what the operation
    /// reports, a line per written or skipped file, and the summary line.
    pub messages: Vec<String>,
    /// The files written (or that would be): changed ones.
    pub updated: usize,
    /// The files processed.
    pub processed: usize,
    /// The files written or skipped, in the order of the input.
    pub files: Vec<SniffFile>,
    /// The error that stopped the run (`<file>: <message>`), when a file
    /// failed and `skip_on_errors` was off. The files before it are done.
    pub aborted: Option<String>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

fn sniff_run(_: &mut Session, request: SniffRunRequest) -> Result<SniffRunResponse, CommandError> {
    let mut settings = match &request.settings {
        Some(path) => {
            if !Path::new(path).is_file() {
                return Err(CommandError::new(
                    "io",
                    format!("{path}: the settings file does not exist"),
                ));
            }
            MemIniFile::load(Path::new(path))
        }
        None => MemIniFile::default(),
    };
    // The options go to the section of the operation.
    let title = PROCS
        .iter()
        .find(|entry| same_text(entry.title, &request.operation))
        .map(|entry| entry.title)
        .ok_or_else(|| {
            CommandError::new(
                "invalid_params",
                format!("Unknown operation: {}; sniff.list names them", request.operation),
            )
        })?;
    let section = title.replace(' ', "");
    for (name, value) in &request.options {
        settings.write_string(&section, name, value);
    }
    let options = RunOptions {
        operation: request.operation.clone(),
        input: request.input.clone(),
        output: request.output.clone().unwrap_or_default(),
        path_contains: Some(request.path_contains.clone().unwrap_or_default()),
        subdir: Some(request.subdir.unwrap_or(true)),
        skip_on_errors: Some(request.skip_on_errors.unwrap_or(false)),
        copy_all: Some(request.copy_all.unwrap_or(false)),
        threads: Some(request.threads.unwrap_or(0).max(0)),
        dry_run: request.dry_run,
        sink: None,
    };
    let (messages, report, aborted) = match run(Some(settings), &options) {
        Ok(report) => (report.messages.clone(), Some(report), None),
        Err(RunError::Aborted { messages, error }) => (messages, None, Some(error)),
        Err(RunError::NotPorted(name, why)) => {
            return Err(CommandError::new("unsupported", format!("{name}: {why}")));
        }
        Err(error) => return Err(CommandError::new("invalid_params", error.to_string())),
    };
    if let Some(log) = &request.log
        && !request.dry_run
    {
        write_atomically(Path::new(log), &string_list_file_bytes(&messages))?;
    }
    let files = report
        .as_ref()
        .map(|report| {
            report
                .files
                .iter()
                .filter_map(|(file, status)| {
                    let (status, error) = match status {
                        FileStatus::Updated => ("updated", None),
                        FileStatus::Unchanged => ("unchanged", None),
                        FileStatus::Skipped(error) => ("skipped", Some(error.clone())),
                        FileStatus::Untouched => return None,
                    };
                    Some(SniffFile {
                        file: file.clone(),
                        status: status.to_owned(),
                        error,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SniffRunResponse {
        operation: title.to_owned(),
        updated: report.as_ref().map_or(0, |report| report.modified),
        processed: report.as_ref().map_or(0, |report| report.processed),
        messages,
        files,
        aborted,
        dry_run: request.dry_run,
    })
}

/// A setting of an operation.
#[derive(Serialize, JsonSchema)]
pub struct SniffSetting {
    /// The name in the operation's section of the settings ini.
    pub name: String,
    /// `bool` (0 or 1), `integer` or `string`.
    pub kind: String,
    /// The default: the one of Sniff's control.
    pub default: String,
}

/// An operation of Sniff.
#[derive(Serialize, JsonSchema)]
pub struct SniffOperation {
    /// The title (`operation` of `sniff.run`).
    pub title: String,
    /// The group of the list: NIF, Report, Animation, Collision or Shader.
    pub group: String,
    /// The games it is meant for.
    pub games: Vec<String>,
    /// The file masks it processes by default.
    pub files: String,
    /// Whether it only reports and writes no file.
    pub report_only: bool,
    /// The section of the settings ini.
    pub section: String,
    /// Its settings with their defaults.
    pub settings: Vec<SniffSetting>,
    /// Why it can not run yet, for an operation not ported.
    pub not_ported: Option<String>,
}

/// `sniff.list`: the response.
#[derive(Serialize, JsonSchema)]
pub struct SniffListResponse {
    /// The operations in the order of Sniff's list.
    pub operations: Vec<SniffOperation>,
}

fn sniff_list(_: &mut Session, _: NoParams) -> Result<SniffListResponse, CommandError> {
    let mut infos = proc_infos().into_iter();
    let operations = PROCS
        .iter()
        .map(|entry| {
            if entry.create.is_some()
                && let Some(info) = infos.next()
            {
                return SniffOperation {
                    title: info.title.to_owned(),
                    group: info.group.to_owned(),
                    games: info.supported_games.iter().map(|game| (*game).to_owned()).collect(),
                    files: info.extensions,
                    report_only: info.no_output,
                    section: info.storage_section,
                    settings: info
                        .options
                        .into_iter()
                        .map(|option| SniffSetting {
                            name: option.name,
                            kind: match option.kind {
                                OptionKind::Bool => "bool",
                                OptionKind::Integer => "integer",
                                OptionKind::String => "string",
                            }
                            .to_owned(),
                            default: option.default,
                        })
                        .collect(),
                    not_ported: None,
                };
            }
            SniffOperation {
                title: entry.title.to_owned(),
                group: entry.group.to_owned(),
                games: Vec::new(),
                files: String::new(),
                report_only: false,
                section: entry.title.replace(' ', ""),
                settings: Vec::new(),
                not_ported: Some(entry.pending.to_owned()),
            }
        })
        .collect();
    Ok(SniffListResponse { operations })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "sniff.list",
        "List the operations of Sniff with their settings and defaults.",
        false,
        sniff_list,
    );
    registry.register(
        "sniff.run",
        "Run an operation of Sniff on the NIF, KF and material files of a folder or archive (Sniff's automation mode, -OP:). Needs --edit unless --dry-run.",
        true,
        sniff_run,
    );
}
