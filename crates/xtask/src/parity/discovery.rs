// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity discovery`: what `DoInitPath` of `xeInit.pas` finds
//! for a game when the command line names neither `-D:` nor `-P:`.
//!
//! The GUI build and the port both run their `-CheckForITM` mode without
//! `-D:` and `-P:`: the data folder comes from the folder of the program,
//! the Steam library and the registry, the plugin list from the game's own
//! `Plugins.txt` in the local application data folder, and the save folder
//! from `SLocalSavePath` of the game's ini below `My Games`. Both print the
//! `Using ...` lines of the main form's startup log (`xeMainForm.pas:5200`)
//! and exit with the ITM count of the last loaded plugin.
//!
//! Everything but the data folder, the inis and the plugin list stays
//! private: the temporary, cache, backup, scripts, My Games and save
//! folders are the run's own (`-T:`, `-C:`, `-B:`, `-S:`, `-M:`, `-G:`), an
//! empty `<APP><Tool>.ini` in the program folder keeps the GUI from
//! creating a settings file next to the discovered `Plugins.txt`, and the
//! port runs from a copy of its own binary, so its program folder is the
//! run's program folder too.
//!
//! The paths the case compares are therefore the machine's (the registry
//! key, `%LOCALAPPDATA%`, `%USERPROFILE%`), which is the point of it: the
//! comparison is only meaningful on a machine where the game is installed
//! and has been started once.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::gui::{self, GuiRun};
use super::{Game, Options};

/// The prefixes of the startup log the case compares: what `DoInitPath`
/// found and what the load active list is.
const PREFIXES: &[&str] = &[
    "Using ",
    "Loading active plugin list:",
    "Warning: Could not find plugin list",
];

#[derive(Serialize)]
struct DiscoveryOutcome {
    game: &'static str,
    /// `equal`, `different`, `oracle-failed` or `port-failed`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// The lines both sides wrote, with the run folder of each side mapped
    /// to `<root>`.
    lines: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oracle_exit_code: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_exit_code: Option<u32>,
}

#[derive(Serialize)]
struct DiscoveryReport<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    outcomes: &'a [DiscoveryOutcome],
}

/// The game's own `Plugins.txt` in the local application data folder
/// (`DoInitPath`): the folder is `wbGameName2`, which the port's
/// `game_paths::game_name2` holds; a game with no entry here is one the
/// release has no local folder for.
fn plugins_file(mode: &str) -> Option<std::path::PathBuf> {
    let folder = match mode {
        "TES3" => "Morrowind",
        "TES4" => "Oblivion",
        "FO3" => "Fallout3",
        "FNV" => "FalloutNV",
        "TES5" => "Skyrim",
        "TES5VR" => "Skyrim VR",
        "SSE" => "Skyrim Special Edition",
        "FO4" => "Fallout4",
        "FO4VR" => "Fallout4VR",
        "FO76" => "Fallout76",
        "SF1" => "Starfield",
        _ => return None,
    };
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(std::path::PathBuf::from(local).join(folder).join("Plugins.txt"))
}

/// The lines of a log the case compares, each side's run folder replaced by
/// `<root>` (the two runs are in folders of their own).
fn compared_lines(log: &str, work: &Path) -> Vec<String> {
    let work_text = format!("{}", work.display());
    let mut lines = Vec::new();
    for line in log.lines() {
        let line = line.trim_end();
        let line = line.replace(&work_text, "<root>");
        let line = line.replace(&work_text.replace('\\', "/"), "<root>");
        if PREFIXES.iter().any(|prefix| line.starts_with(prefix)) {
            lines.push(line);
        }
    }
    lines
}

pub(super) fn run_discovery(
    root: &Path,
    tag: &str,
    options: &Options,
    cache: PathBuf,
    scratch: PathBuf,
    oracle_dir: PathBuf,
) -> Result<()> {
    let port = if options.oracle_only {
        None
    } else {
        Some(super::build_port(root)?)
    };
    let mut outcomes = Vec::new();
    for &game in &options.games {
        let Some(data_var) = std::env::var_os(game.data_var).map(PathBuf::from) else {
            ensure!(options.all_games, "environment variable {} is not set", game.data_var);
            println!("skipped       {}: {} is not set", game.name, game.data_var);
            continue;
        };
        if game.mode == "TES3" {
            println!(
                "skipped       {}: the GUI runs Morrowind in its view mode only",
                game.name
            );
            continue;
        }
        // The case is about a game whose own `Plugins.txt` exists: without
        // one the GUI loads no module and the mode has nothing to check.
        match plugins_file(game.mode) {
            Some(path) if path.is_file() => {}
            Some(path) => {
                println!("skipped       {}: {} does not exist", game.name, path.display());
                continue;
            }
            None => {
                println!(
                    "skipped       {}: the game has no local application data folder",
                    game.name
                );
                continue;
            }
        }
        let _ = &data_var;
        let outcome = check(
            runner(&cache, &scratch, &oracle_dir, port.clone(), options)?,
            game,
            options,
        )
        .unwrap_or_else(|error| DiscoveryOutcome {
            game: game.name,
            status: "oracle-failed",
            detail: Some(format!("  {error:#}")),
            lines: Vec::new(),
            oracle_exit_code: None,
            port_exit_code: None,
        });
        let mut text = format!("{:12} {}", outcome.status, outcome.game);
        if let Some(detail) = &outcome.detail {
            text.push('\n');
            text.push_str(detail);
        }
        println!("{text}");
        outcomes.push(outcome);
    }
    let equal = outcomes.iter().filter(|o| o.status == "equal").count();
    let report = DiscoveryReport {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("discovery.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    println!(
        "{equal} of {} discovery runs equal. Report: {}",
        outcomes.len(),
        report_file.display()
    );
    if !options.oracle_only {
        let failed = outcomes
            .iter()
            .filter(|outcome| !matches!(outcome.status, "equal" | "oracle-unsupported"))
            .count();
        ensure!(failed == 0, "parity does not hold");
    }
    Ok(())
}

/// The piece of `Runner` the case needs: the port binary, the folders and
/// the limits. The rest of the harness is about plugins of a corpus.
struct DiscoveryRunner {
    port: Option<PathBuf>,
    scratch: PathBuf,
    max_memory: u64,
}

fn runner(
    cache: &Path,
    scratch: &Path,
    _oracle_dir: &Path,
    port: Option<PathBuf>,
    options: &Options,
) -> Result<DiscoveryRunner> {
    let _ = cache;
    let budget = options
        .memory_budget
        .or_else(|| crate::memory::physical_memory().map(|total| total / 4 * 3))
        .unwrap_or(24 * super::GIB);
    Ok(DiscoveryRunner {
        port,
        scratch: scratch.to_path_buf(),
        max_memory: options.max_memory.unwrap_or(budget),
    })
}

fn check(runner: DiscoveryRunner, game: &'static Game, options: &Options) -> Result<DiscoveryOutcome> {
    let mut outcome = DiscoveryOutcome {
        game: game.name,
        status: "different",
        detail: None,
        lines: Vec::new(),
        oracle_exit_code: None,
        port_exit_code: None,
    };
    // The module a plugin mode needs on the command line, which must be in
    // the data folder it finds: the game master (or the name `--file`
    // gives).
    // The first vanilla name of a game is its master, which is in the data
    // folder the case discovers whatever the `Plugins.txt` lists.
    let master = options
        .files
        .first()
        .cloned()
        .or_else(|| game.vanilla.first().map(|name| (*name).to_owned()))
        .with_context(|| format!("{} has no vanilla master in the corpus table", game.name))?;
    // The oracle: the GUI in its `-CheckForITM` mode, discovering
    // everything itself.
    let exe = {
        let exe_name = gui::exe_name(game.mode);
        let exe = std::env::var_os("XEDIT_ORACLE_DIR")
            .map(PathBuf::from)
            .map(|dir| dir.join(exe_name))
            .context("environment variable XEDIT_ORACLE_DIR is not set")?;
        ensure!(exe.exists(), "{} does not exist", exe.display());
        exe
    };
    let oracle_work = runner.scratch.join(format!("discovery-oracle-{}", game.mode));
    let port_work = runner.scratch.join(format!("discovery-port-{}", game.mode));
    let run = GuiRun {
        exe: &exe,
        mode: game.mode,
        star_plugins_txt: !gui::simple_plugins_txt(game.mode),
        plugins: Vec::new(),
        script: String::new(),
        build_refs: false,
        work: oracle_work.clone(),
        timeout: options.oracle_timeout.unwrap_or(Duration::from_secs(30 * 60)),
        hang_timeout: Duration::from_secs(600),
        budget: &crate::memory::Budget::new(runner.max_memory),
        expected_peak: 4 * super::GIB,
        max_memory: runner.max_memory,
        extra_args: Vec::new(),
        discovery: true,
        tool_name: "CheckForITM".to_owned(),
    };
    let result = run.run_tool_mode(&["-CheckForITM", &master, "-autoexit"])?;
    outcome.oracle_exit_code = result.exit_code;
    let oracle_lines = compared_lines(&result.log, &oracle_work);
    let Some(port) = &runner.port else {
        outcome.status = "oracle-only";
        outcome.lines = oracle_lines;
        return Ok(outcome);
    };
    // The port: the same mode of the same command line, from a copy of its
    // binary in a program folder of its own, so that its `wbProgramPath` is
    // the run's folder.
    let bin = port_work.join("bin");
    fs::create_dir_all(&bin)?;
    let port_exe = bin.join(port.file_name().context("the port binary without a name")?);
    fs::copy(port, &port_exe)?;
    fs::write(bin.join(format!("{}{}.ini", game.mode, "CheckForITM")), "")?;
    let work_text = format!("{}\\", port_work.display());
    let args: Vec<String> = [
        format!("-{}", game.mode),
        "-checkforitm".to_owned(),
        master.clone(),
        "-autoexit".to_owned(),
        format!("-T:{work_text}temp\\"),
        format!("-C:{work_text}cache\\"),
        format!("-B:{work_text}backup\\"),
        format!("-S:{work_text}"),
        format!("-M:{work_text}mygames\\"),
        format!("-I:{work_text}mygames\\game.ini"),
        format!("-G:{work_text}mygames\\Saves\\"),
    ]
    .to_vec();
    for dir in ["temp", "cache", "backup", "mygames\\Saves"] {
        fs::create_dir_all(port_work.join(dir))?;
    }
    fs::write(port_work.join("mygames\\game.ini"), "")?;
    let log_file = runner.scratch.join(format!("discovery-{}.port.log", game.mode));
    let output = std::process::Command::new(&port_exe)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stderr(File::create(&log_file)?)
        .output()?;
    outcome.port_exit_code = output.status.code().map(|code| code as u32);
    let port_lines = compared_lines(&String::from_utf8_lossy(&output.stdout), &port_work);
    let mut differences = Vec::new();
    for line in &oracle_lines {
        if !port_lines.contains(line) {
            differences.push(format!("  only the oracle: {line}"));
        }
    }
    for line in &port_lines {
        if !oracle_lines.contains(line) {
            differences.push(format!("  only the port: {line}"));
        }
    }
    if outcome.oracle_exit_code != outcome.port_exit_code {
        differences.push(format!(
            "  exit code: oracle {:?}, port {:?}",
            outcome.oracle_exit_code, outcome.port_exit_code
        ));
    }
    outcome.lines = oracle_lines;
    if differences.is_empty() {
        outcome.status = "equal";
    } else {
        outcome.status = "different";
        outcome.detail = Some(differences.join("\n"));
    }
    gui::remove_work(&oracle_work);
    Ok(outcome)
}
