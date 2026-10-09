// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (CheckForErrorsLinear,
// CheckForErrors, mniNavCheckForErrorsClick)

//! "Check for Errors" of the main form, out of the GUI: every element below
//! the selected nodes is asked for its `Check` (`Element::get_check`), and
//! each error is reported under the name of the record that holds it, as
//! the message log shows it:
//!
//! ```text
//! Checking for Errors in [01] Dawnguard.esm
//! [NPC_:02003AB1] <DLC1Serana> "Serana"
//!     NPC_ \ Record Header \ Record Flags -> <Unknown: 9>
//! Done: Checking for Errors, Processed Records: 12345, Errors found: 1
//! ```
//!
//! Upstream walks the elements on one thread (`CheckForErrorsLinear`). The
//! errors of one main record depend on that record alone, so the records
//! are checked on the worker threads in batches, each built while it is
//! walked and reset afterwards, and the results are put together in the
//! order of the walk, with upstream's counting: "Errors found" counts the
//! records whose first error starts a new record line, "Processed Records"
//! every main record walked. The output does not depend on the thread
//! count. The walk of a selection of several nodes runs the nodes one
//! after the other, as the GUI does.
//!
//! The tool mode `-CheckForErrors` (`tmCheckForErrors`, phase 4 step 9)
//! checks the file node of the last loaded plugin and exits with the number
//! of errors found, at most 127 ([`CheckReport::exit_code`]); it loads the
//! plugins without the internal edits of the load (`wbAllowInternalEdit`
//! off) and without building the references.

use std::sync::Arc;

use xedit_core::implementation::check::check_children;
use xedit_core::implementation::{ElementImpl, FileImpl, MainRecordImpl, pin_record, trim_initialized_records};
use xedit_core::interface::globals::game_exe_name;
use xedit_core::interface::misc::capture_progress;
use xedit_core::interface::{Element, ElementRef, File, FileState, MainRecord};
use xedit_core::threads;

/// One error of an element: what the log shows after the indent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckError {
    /// `Path` of the element: from the signature of its record, or the
    /// names of its containers when it is in no record.
    pub path: String,
    /// The text of `Check`.
    pub error: String,
}

/// A main record with errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedRecord {
    /// `Name` of the record, the line the log shows before its errors.
    pub name: String,
    /// The load order FormID, as eight hexadecimal digits.
    pub form_id: String,
    pub signature: String,
    /// The file name of the plugin that holds the record.
    pub file: String,
    pub errors: Vec<CheckError>,
}

/// The result of [`check_for_errors`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// `CheckedCount`: the main records walked ("Processed Records").
    pub checked: u64,
    /// `ErrorsCount`: the records with errors ("Errors found"), counted as
    /// upstream counts them (a record line that starts).
    pub errors_found: u64,
    /// The records with errors, in the order of the walk. A record that
    /// shows up twice in the log (its errors interrupted by an element
    /// outside every record) is listed twice.
    pub records: Vec<CheckedRecord>,
    /// Errors of elements that are in no main record (none of upstream's
    /// file and group elements has a check of its own).
    pub other_errors: Vec<CheckError>,
    /// The lines the GUI adds to its message log, without the times and
    /// without the elapsed time of the last line.
    pub messages: Vec<String>,
}

impl CheckReport {
    /// The exit code of the tool mode (`CheckResult`): the errors found, at
    /// most 127.
    pub fn exit_code(&self) -> u8 {
        self.errors_found.min(127) as u8
    }
}

/// A node of the navigation tree that "Check for Errors" runs on: its
/// element (a file, a group or a main record).
pub type CheckTarget = ElementRef;

/// Port of `TwbFile.GetName` (the port's file names are the plain file
/// names): `[<load order FileID>] <file name>`, the game's executable for
/// the hardcoded file.
pub fn file_display_name(file: &FileImpl) -> String {
    let name = if file.get_file_states().contains(FileState::fsIsHardcoded) {
        game_exe_name()
    } else {
        file.get_name()
    };
    format!("[{}] {name}", file.get_load_order_file_id())
}

/// The `Name` of a node's element as the log shows it.
fn element_display_name(element: &ElementRef) -> String {
    match element.as_element_impl().and_then(ElementImpl::file_impl) {
        Some(file) => file_display_name(&file),
        None => element.get_name(),
    }
}

/// What the check of a main record gives, in order: the errors of its
/// elements and the messages its builds log.
enum RecordEvent {
    Error(CheckError),
    Message(String),
}

/// The errors of every element of a main record (the record first, then
/// its elements depth first in their order) with the messages logged while
/// it is built, and its name when it has an error.
///
/// Upstream builds the record twice: `GetCheck` holds the record's own
/// `IwbContainerElementRef` while it reads the elements, and releasing that
/// last reference resets the record (`DoReset`, unless it was modified), so
/// the walk of its elements builds it again, logging the errors of the
/// build (`Errors were found in: ...`) again.
fn record_errors(record: &Arc<MainRecordImpl>) -> (Vec<RecordEvent>, String) {
    let element: ElementRef = record.clone();
    let mut events = Vec::new();
    let (error, messages) = capture_progress(|| element.get_check());
    events.extend(messages.into_iter().map(RecordEvent::Message));
    if !error.is_empty() {
        events.push(RecordEvent::Error(CheckError {
            path: element.get_path(),
            error,
        }));
    }
    record.reset();
    let (errors, messages) = capture_progress(|| {
        let mut errors = Vec::new();
        for child in check_children(&element) {
            walk_record(&child, &mut errors);
        }
        errors
    });
    events.extend(messages.into_iter().map(RecordEvent::Message));
    events.extend(errors.into_iter().map(RecordEvent::Error));
    let name = if events.iter().any(|event| matches!(event, RecordEvent::Error(_))) {
        record.get_name()
    } else {
        String::new()
    };
    (events, name)
}

fn walk_record(element: &ElementRef, errors: &mut Vec<CheckError>) {
    let error = element.get_check();
    if !error.is_empty() {
        errors.push(CheckError {
            path: element.get_path(),
            error,
        });
    }
    for child in check_children(element) {
        walk_record(&child, errors);
    }
}

/// [`record_errors`] with the record built for the walk and reset
/// afterwards (a modified record keeps its elements).
fn check_record(record: &Arc<MainRecordImpl>) -> (Vec<RecordEvent>, String) {
    let _read = threads::read_guard();
    let pin = pin_record(record);
    let result = record_errors(record);
    drop(pin);
    record.reset();
    trim_initialized_records(KEPT_RECORDS, None);
    result
}

/// The records a batch of the walk holds.
const RECORDS_PER_BATCH: usize = 512;

/// The other records whose elements stay built while the records are
/// checked (as the dump keeps them).
const KEPT_RECORDS: usize = 1024;

/// The errors of `records`, on the worker threads, in their order. When
/// two builds needed each other during the batch, it is checked again on
/// the calling thread, so the result does not depend on the thread count.
fn check_batch(records: &[Arc<MainRecordImpl>], messages: &mut Vec<String>) -> Vec<(Vec<RecordEvent>, String)> {
    let cycles = threads::init_cycles();
    let checked: Vec<_> = match threads::pool() {
        Some(pool) if records.len() > 1 => pool.install(|| {
            use rayon::prelude::*;
            records.par_iter().with_max_len(1).map(check_record).collect()
        }),
        _ => records.iter().map(check_record).collect(),
    };
    if threads::init_cycles() == cycles {
        return checked;
    }
    messages.push(
        "Warning: two records needed each other while they were built; checking the records again on one thread"
            .to_owned(),
    );
    for record in records {
        record.reset();
    }
    trim_initialized_records(0, None);
    records.iter().map(check_record).collect()
}

/// The state of `CheckForErrorsLinear` that runs through the walk: the
/// record of the last error (`LastRecord`) and the counts.
struct Linear<'a> {
    report: &'a mut CheckReport,
    last_record: Option<usize>,
}

impl Linear<'_> {
    /// An error of an element: a record line first when it is the first
    /// error of its record since the last one.
    fn error(&mut self, record: Option<(&Arc<MainRecordImpl>, &str)>, error: CheckError) {
        let id = record.map(|(record, _)| Arc::as_ptr(record) as usize);
        match record {
            Some((record, name)) if id != self.last_record => {
                self.report.errors_found += 1;
                self.report.messages.push(name.to_owned());
                self.report.records.push(CheckedRecord {
                    name: name.to_owned(),
                    form_id: record.get_load_order_form_id().to_string(false),
                    signature: record.get_signature().to_string(),
                    file: record.file_impl().map(|file| file.get_name()).unwrap_or_default(),
                    errors: Vec::new(),
                });
            }
            _ => {}
        }
        self.report
            .messages
            .push(format!("    {} -> {}", error.path, error.error));
        match (record, self.report.records.last_mut()) {
            (Some(_), Some(entry)) => entry.errors.push(error),
            _ => self.report.other_errors.push(error),
        }
        // `Result := aElement.ContainingMainRecord`: an error outside every
        // record clears the last record.
        self.last_record = id;
    }
}

/// The walk of `CheckForErrorsLinear`: the elements outside the main
/// records are visited in order on the calling thread, and the main
/// records are collected into batches that are checked on the worker
/// threads before the walk goes on past them where it has to (a group of
/// the responses of a topic: building the topic sorts that group, which
/// upstream walks after the topic, in its new order).
struct Walker<'a> {
    linear: Linear<'a>,
    pending: Vec<Arc<MainRecordImpl>>,
}

impl Walker<'_> {
    fn visit(&mut self, element: &ElementRef) {
        let element_impl = element.as_element_impl();
        if let Some(record) = element_impl.and_then(ElementImpl::main_record_impl) {
            // `recSkipped`: a record the load skipped for its duplicate
            // FormID is not in upstream's tree.
            if !record.is_skipped_duplicate() {
                self.pending.push(record);
                if self.pending.len() >= RECORDS_PER_BATCH {
                    self.flush();
                }
            }
            return;
        }
        if element_impl
            .and_then(ElementImpl::group_record_impl)
            .is_some_and(|group| group.group_type() == 7)
        {
            self.flush();
        }
        let error = element.get_check();
        if !error.is_empty() {
            self.flush();
            // A main record is never visited here, so this error is outside
            // every record.
            self.linear.error(
                None,
                CheckError {
                    path: element.get_path(),
                    error,
                },
            );
        }
        for child in check_children(element) {
            self.visit(&child);
        }
    }

    /// Checks the records collected so far and adds their errors in order.
    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let records = std::mem::take(&mut self.pending);
        let mut warnings = Vec::new();
        let results = check_batch(&records, &mut warnings);
        for (record, (events, name)) in records.iter().zip(results) {
            for event in events {
                match event {
                    RecordEvent::Error(error) => self.linear.error(Some((record, &name)), error),
                    RecordEvent::Message(message) => self.linear.report.messages.push(message),
                }
            }
            self.linear.report.checked += 1;
        }
        self.linear.report.messages.extend(warnings);
    }
}

/// Port of `CheckForErrorsLinear(aElement, nil)`: every element below
/// `element` checked, the errors and counts added to `report`.
pub fn check_for_errors_linear(element: &ElementRef, report: &mut CheckReport) {
    let mut walker = Walker {
        linear: Linear {
            report,
            last_record: None,
        },
        pending: Vec::new(),
    };
    walker.visit(element);
    walker.flush();
}

/// Port of `mniNavCheckForErrorsClick` over `targets` (the selected nodes
/// in their order): `CheckForErrorsLinear` of each, with the messages of
/// `PerformLongAction`.
pub fn check_for_errors(targets: &[CheckTarget]) -> CheckReport {
    let mut report = CheckReport::default();
    report.messages.push("Start: Checking for Errors".to_owned());
    for target in targets {
        report
            .messages
            .push(format!("Checking for Errors in {}", element_display_name(target)));
        check_for_errors_linear(target, &mut report);
    }
    report.messages.push(format!(
        "Done: Checking for Errors, Processed Records: {}, Errors found: {}",
        report.checked, report.errors_found
    ));
    report
}

/// Port of `TfrmMain.CheckForErrors` (the tree form, which the GUI no
/// longer calls): the errors below `element` with their containers named,
/// children from the last, indented by depth. Returns whether an error was
/// found; `lines` gets what `wbProgress` would show (the empty progress
/// calls show nothing).
pub fn check_for_errors_tree(indent: usize, element: &ElementRef, lines: &mut Vec<String>) -> bool {
    let error = element.get_check();
    let mut result = !error.is_empty();
    if result {
        lines.push(format!(
            "{}{} -> {error}",
            " ".repeat(indent * 2),
            element_display_name(element)
        ));
    }
    for child in check_children(element).iter().rev() {
        result = check_for_errors_tree(indent + 1, child, lines) || result;
    }
    if result && error.is_empty() {
        lines.push(format!(
            "{}Above errors were found in :{}",
            " ".repeat(indent * 2),
            element_display_name(element)
        ));
    }
    result
}

/// The file nodes of the plugins, in load order, as targets.
pub fn file_targets(files: &[Arc<FileImpl>]) -> Vec<CheckTarget> {
    files.iter().map(|file| file.clone() as ElementRef).collect()
}

/// One line of the xDump check: the indent, the name and the error, or the
/// container below which errors were found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DumpCheckLine {
    /// `<indent><Name> -> <Error>`.
    Error { indent: usize, name: String, error: String },
    /// `<indent>Above errors were found in: <Name>`.
    Above { indent: usize, name: String },
}

impl DumpCheckLine {
    /// The line as `xDump.exe -check` writes it, without its line break.
    pub fn text(&self) -> String {
        match self {
            DumpCheckLine::Error { indent, name, error } => format!("{}{name} -> {error}", " ".repeat(indent * 2)),
            DumpCheckLine::Above { indent, name } => {
                format!("{}Above errors were found in: {name}", " ".repeat(indent * 2))
            }
        }
    }
}

/// The xDump check of an element and its children, from the last child
/// (`CheckForErrors` of `xDump.dpr`). Returns whether an error was found.
fn dump_check_tree(indent: usize, element: &ElementRef, lines: &mut Vec<DumpCheckLine>) -> bool {
    let error = element.get_check();
    let mut result = !error.is_empty();
    if result {
        lines.push(DumpCheckLine::Error {
            indent,
            name: element.get_name(),
            error: error.clone(),
        });
    }
    for child in check_children(element).iter().rev() {
        result = dump_check_tree(indent + 1, child, lines) || result;
    }
    if result && error.is_empty() {
        lines.push(DumpCheckLine::Above {
            indent,
            name: element.get_name(),
        });
    }
    result
}

/// Port of `CheckForErrors` of `xDump.dpr` (`xDump -check`) on a file: the
/// errors of every element, children from the last, each container with
/// errors below it named after them. `progress` gets the `Checking:` line
/// of each top group, as xDump reports it.
///
/// The main records are checked first, on the worker threads in batches
/// (each built while it is checked and reset afterwards), and the lines are
/// put together in xDump's order afterwards, so the output does not depend
/// on the thread count.
pub fn dump_check(file: &ElementRef, progress: &mut dyn FnMut(&str)) -> Vec<DumpCheckLine> {
    // The records in any order; the walk below reads their results.
    let mut records = Vec::new();
    collect_records(file, &mut records);
    let mut results: std::collections::HashMap<usize, (Vec<DumpCheckLine>, bool)> = std::collections::HashMap::new();
    for batch in records.chunks(RECORDS_PER_BATCH) {
        let cycles = threads::init_cycles();
        let checked: Vec<(Vec<DumpCheckLine>, bool)> = match threads::pool() {
            Some(pool) if batch.len() > 1 => pool.install(|| {
                use rayon::prelude::*;
                batch.par_iter().with_max_len(1).map(dump_check_record).collect()
            }),
            _ => batch.iter().map(dump_check_record).collect(),
        };
        let checked = if threads::init_cycles() == cycles {
            checked
        } else {
            for record in batch {
                record.reset();
            }
            trim_initialized_records(0, None);
            batch.iter().map(dump_check_record).collect()
        };
        for (record, result) in batch.iter().zip(checked) {
            results.insert(Arc::as_ptr(record) as usize, result);
        }
    }
    let mut lines = Vec::new();
    dump_check_walk(0, file, &results, &mut lines, progress);
    lines
}

fn collect_records(element: &ElementRef, records: &mut Vec<Arc<MainRecordImpl>>) {
    if let Some(record) = element.as_element_impl().and_then(ElementImpl::main_record_impl) {
        if !record.is_skipped_duplicate() {
            records.push(record);
        }
        return;
    }
    for child in check_children(element) {
        collect_records(&child, records);
    }
}

/// The lines of one main record, at the indent of a record in a top group
/// (relative indents are added by the walk), built and reset.
fn dump_check_record(record: &Arc<MainRecordImpl>) -> (Vec<DumpCheckLine>, bool) {
    let _read = threads::read_guard();
    let pin = pin_record(record);
    let mut lines = Vec::new();
    let element: ElementRef = record.clone();
    let found = dump_check_tree(0, &element, &mut lines);
    drop(pin);
    record.reset();
    trim_initialized_records(KEPT_RECORDS, None);
    (lines, found)
}

fn dump_check_walk(
    indent: usize,
    element: &ElementRef,
    results: &std::collections::HashMap<usize, (Vec<DumpCheckLine>, bool)>,
    lines: &mut Vec<DumpCheckLine>,
    progress: &mut dyn FnMut(&str),
) -> bool {
    if let Some(record) = element.as_element_impl().and_then(ElementImpl::main_record_impl) {
        let Some((record_lines, found)) = results.get(&(Arc::as_ptr(&record) as usize)) else {
            return false;
        };
        lines.extend(record_lines.iter().map(|line| match line {
            DumpCheckLine::Error {
                indent: depth,
                name,
                error,
            } => DumpCheckLine::Error {
                indent: indent + depth,
                name: name.clone(),
                error: error.clone(),
            },
            DumpCheckLine::Above { indent: depth, name } => DumpCheckLine::Above {
                indent: indent + depth,
                name: name.clone(),
            },
        }));
        return *found;
    }
    let error = element.get_check();
    let mut result = !error.is_empty();
    if result {
        lines.push(DumpCheckLine::Error {
            indent,
            name: element_display_name(element),
            error: error.clone(),
        });
    }
    if let Some(group) = element.as_element_impl().and_then(ElementImpl::group_record_impl)
        && group.group_type() == 0
    {
        progress(&format!("Checking: {}", element.get_name()));
    }
    for child in check_children(element).iter().rev() {
        result = dump_check_walk(indent + 1, child, results, lines, progress) || result;
    }
    if result && error.is_empty() {
        lines.push(DumpCheckLine::Above {
            indent,
            name: element_display_name(element),
        });
    }
    result
}
