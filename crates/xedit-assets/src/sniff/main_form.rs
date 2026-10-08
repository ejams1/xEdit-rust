// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/frmMain.pas (FormCreate, btnProcessClick: the
// list of operations, the command line and the automation mode),
// Core/wbCommandLine.pas (wbFindCmdLineParam), Core/wbTaskProgress.pas
// (CalcThreads, the worker threads)

//! The non-GUI part of Sniff's main form: the operations it offers, the
//! command line of its automation mode (`-OP:<title>`) and the run itself,
//! which collects the files of the input folder or archive, runs the
//! processor on them on several threads and adds the summary line.
//!
//! Upstream the threads take the files in order and add their messages as
//! they finish, so the order of the messages depends on the threads. The
//! port runs `ProcessFile` on the threads and commits each result (the
//! messages, the counts, the written file) in file order, which is what
//! upstream gives on one thread; the output does not depend on the thread
//! count.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use xedit_io::archive::{Archive, is_archive};

use crate::data_format::{DfError, R};
use crate::sniff::processor::{
    FileStatus, MemIniFile, Proc, ProcContext, ProcFileObject, ProcInput, ProcManager, ProcOptionInfo, StopContext,
    Storage, contains_text, same_text,
};
use crate::sniff::procs;

/// `sSniffVersion`.
pub const SNIFF_VERSION: &str = "1.9.2";
/// `sSniffCaption`.
pub const SNIFF_CAPTION: &str = "S'Lanter's NIF Helper";

/// The groups of the list of operations, in order.
pub const GROUPS: &[&str] = &["NIF", "Report", "Animation", "Collision", "Shader"];

/// `wbFindCmdLineParam` with the switch characters `-` and `/`, ignoring
/// case: the value of `-<switch>:<value>`, an empty value for a bare
/// `-<switch>`, or `None`.
pub fn find_cmd_line_param(args: &[String], switch: &str) -> Option<String> {
    for arg in args {
        let Some(rest) = arg.strip_prefix(['-', '/']) else {
            continue;
        };
        let prefix_len = switch.len() + 1;
        if rest.len() >= prefix_len
            && rest.is_char_boundary(prefix_len)
            && rest[..switch.len()].eq_ignore_ascii_case(switch)
            && rest.as_bytes()[switch.len()] == b':'
        {
            return Some(rest[prefix_len..].to_owned());
        }
        if rest.eq_ignore_ascii_case(switch) {
            return Some(String::new());
        }
    }
    None
}

/// The settings of a run that the main form takes from its controls, the
/// settings ini (`[Main]`) and the command line.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// `-OP:`: the title of the operation, any case.
    pub operation: String,
    /// `-I:`: a folder or an archive.
    pub input: String,
    /// `-O:`: the folder of the outputs.
    pub output: String,
    /// `-P:`: only paths that contain this text, any case.
    pub path_contains: Option<String>,
    /// `-subdir:`: include the subfolders of an input folder.
    pub subdir: Option<bool>,
    /// `-skip:`: skip a file the processor fails on, instead of stopping.
    pub skip_on_errors: Option<bool>,
    /// `-all:`: write the unchanged files too.
    pub copy_all: Option<bool>,
    /// `-threads:`: 0 for the CPU count less one.
    pub threads: Option<i32>,
    /// Process every file but write nothing.
    pub dry_run: bool,
    /// Receives each output (the path under the output folder and the
    /// bytes) instead of the file being written.
    pub sink: Option<OutputSink>,
}

/// A receiver of the outputs of a run (`RunOptions::sink`).
#[derive(Clone)]
pub struct OutputSink(pub std::sync::Arc<dyn Fn(&str, &[u8]) + Send + Sync>);

impl std::fmt::Debug for OutputSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OutputSink")
    }
}

impl RunOptions {
    /// The options of the command line (`FormCreate`); the settings file and
    /// the log file are the caller's.
    pub fn from_command_line(args: &[String]) -> RunOptions {
        let flag = |name: &str| find_cmd_line_param(args, name).map(|value| same_text(&value, "yes"));
        RunOptions {
            operation: find_cmd_line_param(args, "OP").unwrap_or_default(),
            input: find_cmd_line_param(args, "I").unwrap_or_default(),
            output: find_cmd_line_param(args, "O").unwrap_or_default(),
            path_contains: find_cmd_line_param(args, "P"),
            subdir: flag("subdir"),
            skip_on_errors: flag("skip"),
            copy_all: flag("all"),
            threads: find_cmd_line_param(args, "threads")
                .and_then(|value| crate::variant::str_to_int(&value))
                .filter(|&count| count >= 0),
            dry_run: false,
            sink: None,
        }
    }
}

/// The result of a run.
#[derive(Debug)]
pub struct RunReport {
    /// The title of the operation.
    pub operation: String,
    /// `Manager.Messages`, ending with the summary line.
    pub messages: Vec<String>,
    /// `ModifiedCount`.
    pub modified: usize,
    /// `ProcessedCount`.
    pub processed: usize,
    /// Each file with what happened to it, in file order.
    pub files: Vec<(String, FileStatus)>,
    /// The time the run took.
    pub elapsed: Duration,
}

/// Why a run did not happen or stopped.
#[derive(Debug)]
pub enum RunError {
    /// No operation has the title (upstream then opens the GUI).
    UnknownOperation(String),
    /// The operation's unit is not ported yet: the title and why.
    NotPorted(String, String),
    /// A message upstream shows in a dialog before any file
    /// (`SniffMessage`): a missing input or output folder, or the
    /// exception of `OnStart`.
    Message(String),
    /// A file failed and skipping on errors is off: the messages so far,
    /// ending with the error line.
    Aborted { messages: Vec<String>, error: String },
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::UnknownOperation(name) => write!(f, "Unknown operation: {name}"),
            RunError::NotPorted(name, why) => write!(f, "{name}: {why}"),
            RunError::Message(message) => f.write_str(message),
            RunError::Aborted { error, .. } => f.write_str(error),
        }
    }
}

/// The operations of the main form, in the order of `FormCreate` (the
/// group of each is its `GroupID`).
pub fn create_procs() -> Vec<Box<dyn Proc>> {
    procs::all()
}

/// The description of an operation: its title, group, games, extensions
/// and the settings it reads, with their defaults.
#[derive(Debug, Clone)]
pub struct ProcInfo {
    pub title: &'static str,
    pub group: &'static str,
    pub supported_games: Vec<&'static str>,
    pub extensions: String,
    pub no_output: bool,
    pub storage_section: String,
    pub options: Vec<ProcOptionInfo>,
}

/// The operations with what each reads from its settings section.
pub fn proc_infos() -> Vec<ProcInfo> {
    create_procs()
        .into_iter()
        .map(|mut proc| {
            let section = proc.base().storage_section();
            let storage = Storage::recording(section.clone());
            proc.on_show(&storage);
            let mut options = storage.recorded();
            options.insert(
                0,
                ProcOptionInfo {
                    name: "ProcessedFiles".to_owned(),
                    kind: crate::sniff::processor::OptionKind::String,
                    default: proc.base().extension_names(),
                },
            );
            proc.on_hide();
            let base = proc.base();
            ProcInfo {
                title: base.title,
                group: base.group,
                supported_games: base.supported_games.iter().map(|game| game.name()).collect(),
                extensions: base.extension_names(),
                no_output: base.no_output,
                storage_section: section,
                options,
            }
        })
        .collect()
}

/// `CalcThreads`: one core left for the system, at least two threads.
fn calc_threads(cores: i32) -> i32 {
    (cores - 1).max(2)
}

/// `System.CPUCount`.
fn cpu_count() -> i32 {
    std::thread::available_parallelism().map_or(1, |count| count.get() as i32)
}

/// `TDirectory.GetFiles(Path, '*.*', SearchOption)`: the files in the order
/// the file system lists them (NTFS sorts names by their upper case), each
/// folder's subfolders walked where they are met.
fn get_files(dir: &str, recursive: bool, out: &mut Vec<String>) -> std::io::Result<()> {
    let mut entries: Vec<(String, bool)> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| {
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            (entry.file_name().to_string_lossy().into_owned(), is_dir)
        })
        .collect();
    entries.sort_by(|(a, _), (b, _)| {
        let a: Vec<u16> = a.to_uppercase().encode_utf16().collect();
        let b: Vec<u16> = b.to_uppercase().encode_utf16().collect();
        a.cmp(&b)
    });
    for (name, is_dir) in entries {
        let path = if dir.ends_with(['\\', '/']) {
            format!("{dir}{name}")
        } else {
            format!("{dir}\\{name}")
        };
        if is_dir {
            if recursive {
                get_files(&path, recursive, out)?;
            }
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// `IncludeTrailingPathDelimiter`.
fn include_trailing_path_delimiter(path: &str) -> String {
    if path.ends_with('\\') {
        path.to_owned()
    } else {
        format!("{path}\\")
    }
}

/// `TTimeSpan.ToString`: `[d.]hh:mm:ss[.fffffff]`.
pub fn time_span_to_string(elapsed: Duration) -> String {
    let ticks = elapsed.as_nanos() / 100;
    let days = ticks / 864_000_000_000;
    let hours = ticks / 36_000_000_000 % 24;
    let minutes = ticks / 600_000_000 % 60;
    let seconds = ticks / 10_000_000 % 60;
    let sub = ticks % 10_000_000;
    let mut text = String::new();
    if days != 0 {
        text.push_str(&format!("{days}."));
    }
    text.push_str(&format!("{hours:02}:{minutes:02}:{seconds:02}"));
    if sub != 0 {
        text.push_str(&format!(".{sub:07}"));
    }
    text
}

/// The automation mode of the main form: `FormCreate` with `-OP:` and
/// `btnProcessClick`. `settings` is the settings ini (`-S:`).
pub fn run(settings: Option<MemIniFile>, options: &RunOptions) -> Result<RunReport, RunError> {
    let mut procs = create_procs();
    let index = procs
        .iter()
        .position(|proc| same_text(&options.operation, proc.base().title))
        .ok_or_else(|| {
            match procs::PROCS
                .iter()
                .find(|entry| same_text(&options.operation, entry.title))
            {
                Some(entry) => RunError::NotPorted(entry.title.to_owned(), entry.pending.to_owned()),
                None => RunError::UnknownOperation(options.operation.clone()),
            }
        })?;
    let mut proc = procs.swap_remove(index);

    let main = |name: &str| settings.as_ref().map(|ini| ini.read_string("Main", name, ""));
    let main_bool = |name: &str, default: bool| {
        settings
            .as_ref()
            .map_or(default, |ini| ini.read_bool("Main", name, default))
    };
    // The controls: the .dfm defaults, the settings, then the command line.
    let subdir = options.subdir.unwrap_or_else(|| main_bool("InputSubDir", true));
    let skip_on_errors = options
        .skip_on_errors
        .unwrap_or_else(|| main_bool("SkipOnErrors", false));
    let copy_all = options.copy_all.unwrap_or_else(|| main_bool("OutputAll", false));
    let path_contains = match &options.path_contains {
        Some(path) => path.clone(),
        None => main("PathContains").unwrap_or_default(),
    };
    let threads_text = match options.threads {
        Some(count) => count.to_string(),
        None => main("Threads").unwrap_or_default(),
    };

    // lvProcsSelectItem: the extensions of the settings, then OnShow.
    let section = proc.base().storage_section();
    if let Some(ini) = &settings {
        let ext = ini.read_string(&section, "ProcessedFiles", &proc.base().extension_names());
        if !same_text(&ext, &proc.base().extension_names()) {
            proc.base_mut().set_extension_names(&ext);
        }
    }
    {
        let storage = Storage::new(section, settings.as_ref());
        proc.on_show(&storage);
    }
    let result = run_proc(
        proc.as_mut(),
        settings,
        options,
        subdir,
        skip_on_errors,
        copy_all,
        &path_contains,
        &threads_text,
    );
    proc.on_hide();
    result
}

#[allow(clippy::too_many_arguments)]
fn run_proc(
    proc: &mut dyn Proc,
    settings: Option<MemIniFile>,
    options: &RunOptions,
    subdir: bool,
    skip_on_errors: bool,
    copy_all: bool,
    path_contains: &str,
    threads_text: &str,
) -> Result<RunReport, RunError> {
    // btnProcessClick
    let input = options.input.trim();
    let input_is_archive = is_archive(&options.input) && std::path::Path::new(&options.input).is_file();
    if input.is_empty() || (!std::path::Path::new(&options.input).is_dir() && !input_is_archive) {
        return Err(RunError::Message(
            "Input directory/archive not found or invalid".to_owned(),
        ));
    }
    if !proc.base().no_output && (options.output.trim().is_empty() || !std::path::Path::new(&options.output).is_dir()) {
        return Err(RunError::Message("Output directory not found".to_owned()));
    }
    let input_directory = if is_archive(&options.input) {
        options.input.clone()
    } else {
        include_trailing_path_delimiter(&options.input)
    };

    let mut manager = ProcManager::new(settings);
    manager.initialize_processing();
    manager.output_directory = include_trailing_path_delimiter(&options.output);
    manager.copy_all = copy_all;
    manager.skip_on_errors = skip_on_errors;
    manager.dry_run = options.dry_run;
    manager.sink = options.sink.clone();

    proc.on_start().map_err(|error| RunError::Message(error.0))?;

    let start = Instant::now();
    let path = crate::sniff::processor::trim(path_contains).to_owned();

    // Collecting the files.
    let mut input = ProcInput {
        archive: None,
        input_directory,
    };
    let mut names: Vec<(String, Option<usize>)> = Vec::new();
    if is_archive(&input.input_directory) {
        let archive =
            Archive::open(std::path::Path::new(&input.input_directory)).map_err(|error| RunError::Message(error.0))?;
        for (index, entry) in archive.files().iter().enumerate() {
            if !proc.base().is_accepted_file(&entry.name) {
                continue;
            }
            if !path.is_empty() && !contains_text(&entry.name, &path) {
                continue;
            }
            names.push((entry.name.clone(), Some(index)));
        }
        input.archive = Some(archive);
    } else {
        let mut files = Vec::new();
        get_files(&input.input_directory, subdir, &mut files).map_err(|error| RunError::Message(error.to_string()))?;
        for full in files {
            let name = full[input.input_directory.len().min(full.len())..].to_owned();
            if !proc.base().is_accepted_file(&name) {
                continue;
            }
            if !path.is_empty() && !contains_text(&name, &path) {
                continue;
            }
            names.push((name, None));
        }
    }

    // The thread count: the processor's own, else the user's up to the CPU
    // count, 0 for automatic.
    let mut threads = proc.base().threads;
    if threads == 0 {
        threads = crate::variant::str_to_int(threads_text).unwrap_or(0);
        if threads > cpu_count() {
            threads = cpu_count();
        }
    }
    if threads == 0 {
        threads = calc_threads(cpu_count()).max(1);
    }
    let threads = (threads.max(1) as usize).min(names.len().max(1));

    let mut files_report = Vec::new();
    let input = &input;
    let objects: Vec<ProcFileObject> = names
        .iter()
        .map(|(name, entry)| ProcFileObject {
            input,
            file_name: name.clone(),
            file_entry: entry.and_then(|index| input.archive.as_ref().map(|archive| &archive.files()[index])),
        })
        .collect();

    let abort = process_all(&*proc, &mut manager, objects, threads, &mut files_report);
    if let Some(error) = abort {
        manager.add_message(format!("\r\nError: \"{error}"));
        return Err(RunError::Aborted {
            messages: std::mem::take(&mut manager.messages),
            error,
        });
    }

    // OnStop: its exceptions do not matter.
    let proc_log = std::mem::take(&mut manager.proc_log);
    let _ = proc.on_stop(&mut StopContext {
        messages: &mut manager.messages,
        proc_log: &proc_log,
        dry_run: options.dry_run,
    });

    let elapsed = start.elapsed();
    manager.add_message(format!(
        "Done. Updated {} files out of {}, elapsed time {}.",
        manager.modified_count,
        manager.processed_count,
        time_span_to_string(elapsed)
    ));
    Ok(RunReport {
        operation: proc.base().title.to_owned(),
        messages: manager.messages,
        modified: manager.modified_count,
        processed: manager.processed_count,
        files: files_report,
        elapsed,
    })
}

/// One processed file waiting for its turn to be committed.
struct Done<'a> {
    file: ProcFileObject<'a>,
    ctx: ProcContext,
    result: R<Vec<u8>>,
}

/// Runs `ProcessFile` on `threads` threads and commits the results in file
/// order with `TProcManager.Process`. Returns the error line of the file
/// that stopped the run (`<file>: <message>`), if one did.
fn process_all<'a>(
    proc: &dyn Proc,
    manager: &mut ProcManager,
    objects: Vec<ProcFileObject<'a>>,
    threads: usize,
    report: &mut Vec<(String, FileStatus)>,
) -> Option<String> {
    let count = objects.len();
    if count == 0 {
        return None;
    }
    // The files are taken in order; a worker runs at most `window` files
    // ahead of the commit, so the outputs waiting in memory stay few.
    let window = threads * 4;
    let slots: Vec<Mutex<Option<ProcFileObject<'a>>>> = objects.into_iter().map(|o| Mutex::new(Some(o))).collect();
    let next = AtomicUsize::new(0);
    let state = Mutex::new((BTreeMap::<usize, Done<'a>>::new(), 0usize, false));
    let ready = Condvar::new();

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    if index >= count {
                        break;
                    }
                    // Wait for the commit to come close enough.
                    {
                        let mut guard = state.lock().unwrap();
                        while index >= guard.1 + window && !guard.2 {
                            guard = ready.wait(guard).unwrap();
                        }
                        if guard.2 {
                            break;
                        }
                    }
                    let Some(mut file) = slots[index].lock().unwrap().take() else {
                        break;
                    };
                    let mut ctx = ProcContext::default();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        proc.process_file(&mut file, &mut ctx)
                    }))
                    .unwrap_or_else(|panic| {
                        let message = panic
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                            .unwrap_or_else(|| "panic".to_owned());
                        Err(DfError::new(message))
                    });
                    let mut guard = state.lock().unwrap();
                    guard.0.insert(index, Done { file, ctx, result });
                    ready.notify_all();
                }
            });
        }

        // The commit, in file order, on this thread.
        let mut abort = None;
        for index in 0..count {
            let done = {
                let mut guard = state.lock().unwrap();
                loop {
                    if let Some(done) = guard.0.remove(&index) {
                        break done;
                    }
                    guard = ready.wait(guard).unwrap();
                }
            };
            let name = done.file.file_name.clone();
            match manager.process(proc, &done.file, done.ctx, done.result) {
                Ok(status) => report.push((name, status)),
                Err(error) => {
                    abort = Some(format!("{name}: {}", error.0));
                }
            }
            let mut guard = state.lock().unwrap();
            guard.1 = index + 1;
            if abort.is_some() {
                guard.2 = true;
            }
            ready.notify_all();
            drop(guard);
            if abort.is_some() {
                break;
            }
        }
        abort
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_switches() {
        let args: Vec<String> = [
            "-S:a.ini",
            "-OP:Update bounds",
            "/i:C:\\in",
            "-subdir:no",
            "-skip:YES",
            "-threads:4",
        ]
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect();
        let options = RunOptions::from_command_line(&args);
        assert_eq!(options.operation, "Update bounds");
        assert_eq!(options.input, "C:\\in");
        assert_eq!(options.subdir, Some(false));
        assert_eq!(options.skip_on_errors, Some(true));
        assert_eq!(options.copy_all, None);
        assert_eq!(options.threads, Some(4));
        assert_eq!(find_cmd_line_param(&args, "s").as_deref(), Some("a.ini"));
        assert_eq!(find_cmd_line_param(&args, "P"), None);
    }

    #[test]
    fn time_spans() {
        assert_eq!(time_span_to_string(Duration::from_millis(1500)), "00:00:01.5000000");
        assert_eq!(time_span_to_string(Duration::from_secs(3661)), "01:01:01");
        assert_eq!(time_span_to_string(Duration::from_secs(90000)), "1.01:00:00");
    }

    #[test]
    fn every_operation_has_a_unique_title() {
        let infos = proc_infos();
        for (index, info) in infos.iter().enumerate() {
            assert!(GROUPS.contains(&info.group), "{} has no group", info.title);
            assert!(
                infos[..index].iter().all(|other| !same_text(other.title, info.title)),
                "{} twice",
                info.title
            );
        }
    }
}
