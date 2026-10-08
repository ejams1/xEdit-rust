// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/frmMain.pas (FormCreate and btnProcessClick in
// the automation mode)

//! `sniff`: the operations of Sniff (`S'Lanter's NIF Helper`) on the NIF,
//! KF and material files of a folder or an archive, with the arguments of
//! its automation mode:
//!
//! ```text
//! sniff -OP:<operation> -I:<folder|archive> -O:<folder> [-S:<settings ini>] [-LOG:<file>]
//!       [-P:<path part>] [-subdir:yes|no] [-skip:yes|no] [-all:yes|no] [-threads:<n>]
//! sniff <operation> -I:... (the same, the operation as the first argument)
//! sniff -list (the operations and the settings each reads)
//! ```
//!
//! The settings of an operation are read from the section of the settings
//! ini named after its title without spaces (`[Universaltweaker]`); the
//! settings ini and the log file are relative to the folder of the
//! program, as upstream's. The messages of the run are printed and, with
//! `-LOG:`, written to the log file as Sniff writes it.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use xedit_assets::sniff::main_form::{
    RunError, RunOptions, SNIFF_CAPTION, SNIFF_VERSION, find_cmd_line_param, proc_infos, run,
};
use xedit_assets::sniff::processor::{MemIniFile, string_list_file_bytes};
use xedit_assets::sniff::procs::PROCS;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `TPath.Combine(ExtractFilePath(ParamStr(0)), s)` for a relative path.
fn beside_program(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_exe() {
        Ok(exe) => exe.parent().map_or_else(|| path.to_path_buf(), |dir| dir.join(path)),
        Err(_) => path.to_path_buf(),
    }
}

fn usage() {
    println!("{SNIFF_CAPTION} {SNIFF_VERSION} (xEdit-rust)");
    println!();
    println!("sniff -OP:<operation> -I:<folder|archive> -O:<folder> [-S:<settings ini>] [-LOG:<file>]");
    println!("      [-P:<path part>] [-subdir:yes|no] [-skip:yes|no] [-all:yes|no] [-threads:<n>]");
    println!("sniff -list");
    println!();
    println!("Operations:");
    for entry in PROCS {
        if entry.create.is_some() {
            println!("  {:<10} {}", entry.group, entry.title);
        } else {
            println!("  {:<10} {} ({})", entry.group, entry.title, entry.pending);
        }
    }
}

fn list() {
    for info in proc_infos() {
        println!("{} [{}]", info.title, info.group);
        println!("  games: {}", info.supported_games.join(", "));
        println!("  files: {}", info.extensions);
        if info.no_output {
            println!("  reports only");
        }
        println!("  settings section: [{}]", info.storage_section);
        for option in info.options {
            println!("    {}={}", option.name, option.default);
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if find_cmd_line_param(&args, "list").is_some() {
        list();
        return ExitCode::SUCCESS;
    }
    let mut options = RunOptions::from_command_line(&args);
    if options.operation.is_empty()
        && let Some(first) = args.first().filter(|arg| !arg.starts_with(['-', '/']))
    {
        options.operation = first.clone();
    }
    if options.operation.is_empty() {
        usage();
        return ExitCode::FAILURE;
    }

    // The settings ini: `-S:`, else `sniff.ini` beside the program.
    let settings_path = match find_cmd_line_param(&args, "S").filter(|value| !value.is_empty()) {
        Some(path) => beside_program(&path),
        None => beside_program("sniff.ini"),
    };
    let settings = MemIniFile::load(&settings_path);

    match run(Some(settings), &options) {
        Ok(report) => {
            for message in &report.messages {
                println!("{message}");
            }
            // The log file of the automation mode.
            if let Some(log) = find_cmd_line_param(&args, "LOG").filter(|value| !value.is_empty()) {
                // Upstream ignores a failure to write it.
                let _ = std::fs::write(beside_program(&log), string_list_file_bytes(&report.messages));
            }
            ExitCode::SUCCESS
        }
        Err(RunError::Aborted { messages, error }) => {
            for message in &messages {
                println!("{message}");
            }
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
