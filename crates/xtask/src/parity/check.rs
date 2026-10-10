// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity check`: "Check for Errors".
//!
//! For every corpus plugin, the GUI build of xEdit runs its
//! `-CheckForErrors` tool mode on a private copy of the plugin and its
//! masters (`gui::GuiRun::run_check_for_errors`): it loads them without the
//! internal edits of the load, checks every element of the plugin's file
//! node, writes the errors to its message log and exits with the number of
//! records with errors (at most 127). The port runs `xedit check` on the
//! same files (a folder of hard links to the plugins only, as the GUI's
//! private data folder: no archives and no strings files). The check
//! compares the lines of the log from `Start: Checking for Errors` to the
//! `Done:` line with its counts (without the times, the elapsed time and the
//! `still checking` lines the GUI writes while it is busy), line by line,
//! and the exit code. The oracle's log is cached as
//! `<cache>/<tag>/<MODE>-oracle-check/<file>.<key>.log.zst` with its exit
//! code in `.exit`; the key hashes the plugin and its masters.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;

use super::gui::{self, GuiRun};
use super::oracle_save::{find_in, keep_compressed, load_list, oracle_key, zstd_reader};
use super::{GIB, Game, Options, Runner, is_vanilla};

/// Part of the cache key: changes to what the oracle run does go here.
const ORACLE_RECIPE: &str = "checkforerrors v1";

/// How many differing lines an outcome lists.
const LISTED_DIFFERENCES: usize = 12;

#[derive(Serialize)]
struct CheckOutcome {
    game: &'static str,
    file: String,
    /// `equal`, `equal-log-loss` (the oracle's log lost lines, see
    /// `lost_blocks`), `different`, `oracle-failed`, `oracle-unsupported`,
    /// `port-failed` or `oracle-only`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// The oracle's "Processed Records".
    checked: u64,
    /// The oracle's "Errors found".
    errors_found: u64,
    /// The error lines of the oracle's log (`    <path> -> <error>`).
    error_lines: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    oracle_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_seconds: Option<f64>,
}

#[derive(Serialize)]
struct CheckReport<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    records_checked: u64,
    errors_found: u64,
    error_lines: usize,
    outcomes: &'a [CheckOutcome],
}

/// A line without the `[mm:ss] ` (or `[h:mm:ss] `) of `wbProgress`.
fn strip_time(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix('[')
        && let Some(end) = rest.find("] ")
        && end >= 5
        && rest[..end].chars().all(|c| c.is_ascii_digit() || c == ':')
    {
        return &rest[end + 2..];
    }
    line
}

/// The lines of "Check for Errors" in a message log: from the start line
/// to the `Done:` line, without the times, the `still ...` lines and the
/// elapsed time.
fn check_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut result = Vec::new();
    let mut inside = false;
    for line in lines {
        let line = strip_time(line.trim_end_matches(['\r', '\n']));
        if line == "Start: Checking for Errors" {
            inside = true;
            result.clear();
        }
        if !inside {
            continue;
        }
        if line.starts_with("still checking for Errors") {
            continue;
        }
        if let Some(done) = line.strip_prefix("Done: Checking for Errors") {
            let done = done.split(", Elapsed Time:").next().unwrap_or(done);
            result.push(format!("Done: Checking for Errors{done}"));
            break;
        }
        result.push(line.to_owned());
    }
    result
}

/// The counts of the `Done:` line.
fn counts(lines: &[String]) -> (u64, u64) {
    let number_after = |line: &str, label: &str| -> u64 {
        line.find(label)
            .map(|at| {
                line[at + label.len()..]
                    .trim_start()
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
            })
            .and_then(|digits| digits.parse().ok())
            .unwrap_or(0)
    };
    match lines.last() {
        Some(done) if done.starts_with("Done: ") => (
            number_after(done, "Processed Records:"),
            number_after(done, "Errors found:"),
        ),
        _ => (0, 0),
    }
}

/// The plugins of a game the check runs on: the vanilla plugins, or those
/// named with `--file`.
fn corpus(game: &Game, data: &Path, options: &Options) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(data).with_context(|| format!("reading {}", data.display()))? {
        let path = entry?.path();
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let lower = name.to_lowercase();
        let is_plugin = super::PLUGIN_EXTENSIONS
            .iter()
            .any(|ext| lower.ends_with(&format!(".{ext}")));
        let selected = if options.files.is_empty() {
            is_vanilla(game, &lower)
        } else {
            options.files.contains(&lower)
        };
        if is_plugin && selected {
            names.push(name);
        }
    }
    names.sort_by_key(|name| name.to_lowercase());
    Ok(names)
}

/// A data folder with only `plugins` in it, as hard links where the file
/// system allows them.
fn private_data(dir: &Path, plugins: &[PathBuf]) -> Result<PathBuf> {
    let data = dir.join("Data");
    if data.exists() {
        fs::remove_dir_all(&data)?;
    }
    fs::create_dir_all(&data)?;
    for plugin in plugins {
        let target = data.join(plugin.file_name().context("plugin without a name")?);
        if fs::hard_link(plugin, &target).is_err() {
            fs::copy(plugin, &target).with_context(|| format!("copying {}", plugin.display()))?;
        }
    }
    Ok(data)
}

fn read_all(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    zstd_reader(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn run_check(
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
    let budget = options
        .memory_budget
        .or_else(|| crate::memory::physical_memory().map(|total| total / 4 * 3))
        .unwrap_or(24 * GIB);
    let runner = Runner {
        oracle: PathBuf::new(),
        port,
        cache,
        scratch,
        oracle_dir,
        hashes: std::sync::Mutex::new(std::collections::HashMap::new()),
        budget: crate::memory::Budget::new(budget),
        max_memory: options.max_memory.unwrap_or(budget),
        oracle_timeout: options.oracle_timeout,
    };
    let mut cases = Vec::new();
    for &game in &options.games {
        let Some(data) = std::env::var_os(game.data_var).map(PathBuf::from) else {
            ensure!(options.all_games, "environment variable {} is not set", game.data_var);
            println!("skipped       {}: {} is not set", game.name, game.data_var);
            continue;
        };
        for name in corpus(game, &data, options)? {
            cases.push((game, data.clone(), name));
        }
    }
    // The oracle runs of `--jobs` plugins at once (within the memory
    // budget); the port run of a plugin follows its oracle run.
    let queue = std::sync::Mutex::new(cases.into_iter().enumerate().collect::<Vec<_>>().into_iter());
    let done = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().unwrap().next();
                    let Some((index, (game, data, name))) = next else { break };
                    let outcome = check(&runner, game, &data, &name).unwrap_or_else(|error| CheckOutcome {
                        game: game.name,
                        file: name.clone(),
                        status: "oracle-failed",
                        detail: Some(format!("  {error:#}")),
                        checked: 0,
                        errors_found: 0,
                        error_lines: 0,
                        oracle_seconds: None,
                        port_seconds: None,
                    });
                    let mut text = format!(
                        "{:13} {}/{} ({} records, {} with errors, {} error lines)",
                        outcome.status,
                        outcome.game,
                        outcome.file,
                        outcome.checked,
                        outcome.errors_found,
                        outcome.error_lines
                    );
                    if let Some(detail) = &outcome.detail {
                        text.push('\n');
                        text.push_str(detail);
                    }
                    println!("{text}");
                    done.lock().unwrap().push((index, outcome));
                }
            });
        }
    });
    let mut outcomes = done.into_inner().unwrap();
    outcomes.sort_by_key(|(index, _)| *index);
    let outcomes: Vec<CheckOutcome> = outcomes.into_iter().map(|(_, outcome)| outcome).collect();
    let equal = outcomes
        .iter()
        .filter(|o| o.status == "equal" || o.status == "equal-log-loss")
        .count();
    let report = CheckReport {
        tag,
        equal,
        total: outcomes.len(),
        records_checked: outcomes.iter().map(|o| o.checked).sum(),
        errors_found: outcomes.iter().map(|o| o.errors_found).sum(),
        error_lines: outcomes.iter().map(|o| o.error_lines).sum(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("check.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    println!(
        "{equal} of {} plugins equal ({} records checked, {} with errors, {} error lines). Report: {}",
        outcomes.len(),
        report.records_checked,
        report.errors_found,
        report.error_lines,
        report_file.display()
    );
    if !options.oracle_only {
        let failed = outcomes
            .iter()
            .filter(|o| !matches!(o.status, "equal" | "equal-log-loss" | "oracle-unsupported"))
            .count();
        ensure!(failed == 0, "parity does not hold");
    }
    Ok(())
}

fn check(runner: &Runner, game: &'static Game, data: &Path, name: &str) -> Result<CheckOutcome> {
    let mut outcome = CheckOutcome {
        game: game.name,
        file: name.to_owned(),
        status: "oracle-only",
        detail: None,
        checked: 0,
        errors_found: 0,
        error_lines: 0,
        oracle_seconds: None,
        port_seconds: None,
    };
    // The 4.1.5q GUI runs Morrowind in its view mode only.
    if game.mode == "TES3" {
        outcome.status = "oracle-unsupported";
        return Ok(outcome);
    }
    let plugins = load_list(game, data, &[name])?;
    let exe_name = gui::exe_name(game.mode);
    let key = oracle_key(runner, &plugins, ORACLE_RECIPE, exe_name)?;
    let dir = runner.cache.join(format!("{}-oracle-check", game.mode));
    fs::create_dir_all(&dir)?;
    let stem = format!("{name}.{key:016x}");
    let log_file = dir.join(format!("{stem}.log.zst"));
    let exit_file = dir.join(format!("{stem}.exit"));
    if !log_file.exists() {
        let exe = runner.oracle_dir.join(exe_name);
        ensure!(exe.exists(), "{} does not exist", exe.display());
        let work =
            runner
                .scratch
                .join("oracle-work")
                .join(format!("{}-check-{name}.{}", game.mode, std::process::id()));
        let peak_file = dir.join(format!("{stem}.oracle.peak"));
        let expected_peak = fs::read_to_string(&peak_file)
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(2 * GIB);
        let run = GuiRun {
            exe: &exe,
            mode: game.mode,
            star_plugins_txt: !gui::simple_plugins_txt(game.mode),
            plugins: plugins.clone(),
            script: String::new(),
            build_refs: false,
            work: work.clone(),
            timeout: runner.oracle_timeout.unwrap_or(Duration::from_secs(240 * 60)),
            hang_timeout: Duration::from_secs(600),
            budget: &runner.budget,
            expected_peak,
            max_memory: runner.max_memory,
            extra_args: Vec::new(),
        };
        let started = std::time::Instant::now();
        let result = match run.run_check_for_errors(name) {
            Ok(result) => result,
            Err(error) => {
                gui::remove_work(&work);
                return Err(error);
            }
        };
        outcome.oracle_seconds = Some(started.elapsed().as_secs_f64());
        if let Some(peak) = result.peak {
            fs::write(&peak_file, peak.to_string())?;
        }
        let lines = check_lines(result.log.lines());
        ensure!(
            lines
                .last()
                .is_some_and(|line| line.starts_with("Done: Checking for Errors")),
            "the check did not finish; the end of the log:\n{}",
            result.log.lines().rev().take(10).collect::<Vec<_>>().join("\n")
        );
        let log_text = work.join("log.txt");
        fs::write(&log_text, &result.log)?;
        keep_compressed(&log_text, &log_file)?;
        fs::write(
            &exit_file,
            result.exit_code.map(|code| code.to_string()).unwrap_or_default(),
        )?;
        fs::write(
            dir.join(format!("{stem}.oracle.seconds")),
            format!("{:.1}", started.elapsed().as_secs_f64()),
        )?;
        gui::remove_work(&work);
    } else {
        outcome.oracle_seconds = fs::read_to_string(dir.join(format!("{stem}.oracle.seconds")))
            .ok()
            .and_then(|text| text.trim().parse().ok());
    }
    let oracle_log = String::from_utf8_lossy(&read_all(&log_file)?).into_owned();
    let oracle_lines = check_lines(oracle_log.lines());
    let oracle_exit: Option<u64> = fs::read_to_string(&exit_file)
        .ok()
        .and_then(|text| text.trim().parse().ok());
    (outcome.checked, outcome.errors_found) = counts(&oracle_lines);
    outcome.error_lines = oracle_lines.iter().filter(|line| line.starts_with("    ")).count();
    let Some(port) = &runner.port else {
        return Ok(outcome);
    };

    let port_dir = runner.scratch.join(format!("{}-check", game.mode));
    fs::create_dir_all(&port_dir)?;
    let private = private_data(&port_dir.join(format!("work-{name}")), &plugins)?;
    let port_log = port_dir.join(format!("{stem}.port.log"));
    let started = std::time::Instant::now();
    let output = std::process::Command::new(port)
        .args(["--json", "--game", game.mode, "--load"])
        .arg(find_in(&private, name)?)
        .args(["check", "--last"])
        .stdin(std::process::Stdio::null())
        .stderr(File::create(&port_log)?)
        .output()?;
    outcome.port_seconds = Some(started.elapsed().as_secs_f64());
    let _ = fs::remove_dir_all(port_dir.join(format!("work-{name}")));
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    if envelope["ok"] != Value::Bool(true) {
        outcome.status = "port-failed";
        let stdout = String::from_utf8_lossy(&output.stdout);
        outcome.detail = Some(format!(
            "  {}, see {}",
            stdout.trim().chars().take(400).collect::<String>(),
            port_log.display()
        ));
        return Ok(outcome);
    }
    let result = &envelope["result"];
    let port_lines: Vec<String> = result["messages"]
        .as_array()
        .map(|lines| lines.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let port_exit = result["exit_code"].as_u64();

    let mut lines = Vec::new();
    if oracle_exit.is_some() && oracle_exit != port_exit {
        lines.push(format!("  exit code: oracle {oracle_exit:?}, port {port_exit:?}"));
    }
    if oracle_lines != port_lines {
        let first = oracle_lines
            .iter()
            .zip(&port_lines)
            .position(|(a, b)| a != b)
            .unwrap_or(oracle_lines.len().min(port_lines.len()));
        lines.push(format!(
            "  lines: oracle {}, port {}; first difference at line {first}:",
            oracle_lines.len(),
            port_lines.len()
        ));
        lines.push(format!(
            "    oracle: {}",
            oracle_lines.get(first).map_or("<end>", String::as_str)
        ));
        lines.push(format!(
            "    port:   {}",
            port_lines.get(first).map_or("<end>", String::as_str)
        ));
        let mut oracle_only: Vec<&String> = Vec::new();
        let mut port_only: Vec<&String> = Vec::new();
        {
            let mut counts: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
            for line in &oracle_lines {
                *counts.entry(line).or_default() += 1;
            }
            for line in &port_lines {
                *counts.entry(line).or_default() -= 1;
            }
            for line in &oracle_lines {
                if let Some(count) = counts.get_mut(line.as_str())
                    && *count > 0
                {
                    *count -= 1;
                    oracle_only.push(line);
                }
            }
            let mut counts: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
            for line in &port_lines {
                *counts.entry(line).or_default() += 1;
            }
            for line in &oracle_lines {
                *counts.entry(line).or_default() -= 1;
            }
            for line in &port_lines {
                if let Some(count) = counts.get_mut(line.as_str())
                    && *count > 0
                {
                    *count -= 1;
                    port_only.push(line);
                }
            }
        }
        lines.push(format!(
            "  {} lines only in the oracle, {} only in the port",
            oracle_only.len(),
            port_only.len()
        ));
        for line in oracle_only.iter().take(LISTED_DIFFERENCES) {
            lines.push(format!("  only the oracle: {line}"));
        }
        for line in port_only.iter().take(LISTED_DIFFERENCES) {
            lines.push(format!("  only the port:   {line}"));
        }
        let port_text = port_dir.join(format!("{stem}.port.txt"));
        let oracle_text = port_dir.join(format!("{stem}.oracle.txt"));
        fs::write(&port_text, port_lines.join("\n") + "\n")?;
        fs::write(&oracle_text, oracle_lines.join("\n") + "\n")?;
        lines.push(format!("  {} {}", oracle_text.display(), port_text.display()));
    }
    if lines.is_empty() {
        outcome.status = "equal";
    } else if oracle_exit == port_exit
        && counts(&oracle_lines) == counts(&port_lines)
        && let Some(blocks) = lost_blocks(&oracle_lines, &port_lines)
    {
        // The GUI posts every line of its log as a window message
        // (`PostAddMessage`); a burst of lines that fills the message queue
        // of its thread loses the lines that do not fit.
        outcome.status = "equal-log-loss";
        outcome.detail = Some(format!(
            "  the oracle's log lost {} lines in {} runs; every line it kept is the port's, in order, and the counts and exit code are equal",
            port_lines.len() - oracle_lines.len(),
            blocks
        ));
    } else {
        outcome.status = "different";
        outcome.detail = Some(lines.join("\n"));
    }
    Ok(outcome)
}

/// Whether `oracle` is `port` with runs of lines left out, and how many
/// runs: the lines a log lost.
fn lost_blocks(oracle: &[String], port: &[String]) -> Option<usize> {
    let mut blocks = 0;
    let mut in_gap = false;
    let mut next = oracle.iter().peekable();
    for line in port {
        if next.peek().is_some_and(|expected| *expected == line) {
            next.next();
            in_gap = false;
        } else {
            if !in_gap {
                blocks += 1;
            }
            in_gap = true;
        }
    }
    (next.peek().is_none() && blocks > 0).then_some(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_lines_of_a_check() {
        let log = "[00:00] Background Loader: finished
[00:01] Start: Checking for Errors
[00:01] Checking for Errors in [02] Dawnguard.esm
[00:03] [NPC_:02003AB1] <DLC1Serana> \"Serana\"
[00:03]     NPC_ \\ DATA - Data -> wrong
[00:12] still checking for Errors in [02] Dawnguard.esm
[1:00:13] [NPC_:02003AB2] <X>
[1:00:13]     NPC_ \\ DATA - Data -> wrong
[1:00:14] Done: Checking for Errors, Processed Records: 12, Errors found: 2, Elapsed Time: 1:00:13
[1:00:14] --= All Done =--";
        let lines = check_lines(log.lines());
        assert_eq!(
            lines,
            [
                "Start: Checking for Errors",
                "Checking for Errors in [02] Dawnguard.esm",
                "[NPC_:02003AB1] <DLC1Serana> \"Serana\"",
                "    NPC_ \\ DATA - Data -> wrong",
                "[NPC_:02003AB2] <X>",
                "    NPC_ \\ DATA - Data -> wrong",
                "Done: Checking for Errors, Processed Records: 12, Errors found: 2",
            ]
        );
        assert_eq!(counts(&lines), (12, 2));
    }
}
