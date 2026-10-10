// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity clean`: the quick auto clean mode.
//!
//! For every corpus plugin that has masters, the GUI build of xEdit runs
//! its quick auto clean mode on a private copy of the plugin and its
//! masters (`-quickautoclean -autoexit -autoload <plugin>`, see
//! `gui::GuiRun::run_quick_clean`), and the port runs `xedit clean --quick`
//! on the same files (a folder of hard links to the plugins only, as the
//! GUI's private data folder: no archives and no strings files). The check
//! compares the saved bytes (or that neither side saved), and per pass the
//! counts of the filter, the UDR and the ITM removal and the records each
//! names in the message log. The oracle's result is cached as
//! `<cache>/<tag>/<MODE>-oracle-clean/<file>.<key>.saved.zst` (or
//! `.unchanged`) with its log as `.log.zst`; the key hashes the plugin and
//! its masters.

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
const ORACLE_RECIPE: &str = "quickautoclean v1";

/// How many differing lines an outcome lists.
const LISTED_DIFFERENCES: usize = 12;

#[derive(Serialize)]
struct CleanOutcome {
    game: &'static str,
    file: String,
    /// `equal`, `different`, `oracle-failed`, `oracle-unsupported`,
    /// `port-failed` or `oracle-only`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// The oracle's counts per pass: `udr/itm/nav`.
    passes: Vec<String>,
    /// Whether the oracle saved the plugin.
    saved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    oracle_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_seconds: Option<f64>,
}

#[derive(Serialize)]
struct CleanReport<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    outcomes: &'a [CleanOutcome],
}

/// The counts of one pass and the records its lines name, as either side
/// reports them.
#[derive(Debug, Default, PartialEq, Eq)]
struct Pass {
    /// `[Pass 1]`, `[Pass 2]` and the remaining nodes of the filter.
    filter: (u64, u64, u64),
    /// Processed and undeleted.
    udr: (u64, u64),
    nav: u64,
    /// Processed and removed.
    itm: (u64, u64),
    /// The `Undeleting:`, `Skipping:`, `Removing:` and `Can't remove:`
    /// lines in order.
    lines: Vec<String>,
}

/// A number after `label` in `line`.
fn number_after(line: &str, label: &str) -> Option<u64> {
    let rest = &line[line.find(label)? + label.len()..];
    let digits: String = rest.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// A line without the `[mm:ss] ` of `wbProgress`.
fn strip_time(line: &str) -> &str {
    let bytes = line.as_bytes();
    if bytes.len() > 8 && bytes[0] == b'[' && bytes[3] == b':' && bytes[6] == b']' && bytes[7] == b' ' {
        &line[8..]
    } else {
        line
    }
}

/// The passes of a message log, the oracle's or the port's.
fn passes_of<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<Pass> {
    let mut passes: Vec<Pass> = Vec::new();
    for line in lines {
        let line = strip_time(line.trim_end());
        if line.starts_with("Done: Applying Filter") {
            passes.push(Pass {
                filter: (
                    number_after(line, "[Pass 1] Processed Records:").unwrap_or(0),
                    number_after(line, "[Pass 2] Processed Records:").unwrap_or(0),
                    number_after(line, "Remaining unfiltered nodes:").unwrap_or(0),
                ),
                ..Default::default()
            });
            continue;
        }
        let Some(pass) = passes.last_mut() else { continue };
        if line.starts_with("[Undeleting and Disabling References done]") {
            pass.udr = (
                number_after(line, "Processed Records:").unwrap_or(0),
                number_after(line, "Undeleted Records:").unwrap_or(0),
            );
        } else if line.starts_with("[Removing \"Identical to Master\" records done]") {
            pass.itm = (
                number_after(line, "Processed Records:").unwrap_or(0),
                number_after(line, "Removed Records:").unwrap_or(0),
            );
        } else if line.starts_with("<Warning: Plugin contains") && line.contains("deleted NavMeshes") {
            pass.nav = number_after(line, "Plugin contains").unwrap_or(0);
        } else if ["Undeleting: ", "Skipping: ", "Removing: ", "Can't remove: "]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        {
            pass.lines.push(line.to_owned());
        }
    }
    passes
}

/// The plugins of a game the check cleans: the vanilla plugins (or those
/// named with `--file`) that have masters.
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
        if is_plugin && selected && load_list(game, data, &[&name])?.len() > 1 {
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

pub(super) fn run_clean(
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
    let mut outcomes = Vec::new();
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
    // budget); the port runs of a plugin follow its oracle run.
    let queue = std::sync::Mutex::new(cases.into_iter().enumerate().collect::<Vec<_>>().into_iter());
    let done = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().unwrap().next();
                    let Some((index, (game, data, name))) = next else { break };
                    let outcome = check(&runner, game, &data, &name).unwrap_or_else(|error| CleanOutcome {
                        game: game.name,
                        file: name.clone(),
                        status: "oracle-failed",
                        detail: Some(format!("  {error:#}")),
                        passes: Vec::new(),
                        saved: false,
                        oracle_seconds: None,
                        port_seconds: None,
                    });
                    let mut text = format!(
                        "{:13} {}/{} (passes {}{})",
                        outcome.status,
                        outcome.game,
                        outcome.file,
                        outcome.passes.join(", "),
                        if outcome.saved { ", saved" } else { "" }
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
    let mut done = done.into_inner().unwrap();
    done.sort_by_key(|(index, _)| *index);
    outcomes.extend(done.into_iter().map(|(_, outcome)| outcome));
    let equal = outcomes.iter().filter(|o| o.status == "equal").count();
    let report = CleanReport {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("clean.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    println!(
        "{equal} of {} plugins equal. Report: {}",
        outcomes.len(),
        report_file.display()
    );
    if !options.oracle_only {
        let failed = outcomes
            .iter()
            .filter(|o| o.status != "equal" && o.status != "oracle-unsupported")
            .count();
        ensure!(failed == 0, "parity does not hold");
    }
    Ok(())
}

fn check(runner: &Runner, game: &'static Game, data: &Path, name: &str) -> Result<CleanOutcome> {
    let mut outcome = CleanOutcome {
        game: game.name,
        file: name.to_owned(),
        status: "oracle-only",
        detail: None,
        passes: Vec::new(),
        saved: false,
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
    let dir = runner.cache.join(format!("{}-oracle-clean", game.mode));
    fs::create_dir_all(&dir)?;
    let stem = format!("{name}.{key:016x}");
    let saved_file = dir.join(format!("{stem}.saved.zst"));
    let unchanged_file = dir.join(format!("{stem}.unchanged"));
    let log_file = dir.join(format!("{stem}.log.zst"));
    if !log_file.exists() {
        let exe = runner.oracle_dir.join(exe_name);
        ensure!(exe.exists(), "{} does not exist", exe.display());
        let work =
            runner
                .scratch
                .join("oracle-work")
                .join(format!("{}-clean-{name}.{}", game.mode, std::process::id()));
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
            timeout: runner.oracle_timeout.unwrap_or(Duration::from_secs(120 * 60)),
            hang_timeout: Duration::from_secs(600),
            budget: &runner.budget,
            expected_peak,
            max_memory: runner.max_memory,
            extra_args: Vec::new(),
        };
        let started = std::time::Instant::now();
        let result = run.run_quick_clean(name);
        let result = match result {
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
        ensure!(
            result.log.contains("Quick Clean mode finished."),
            "the quick clean mode did not finish; the end of the log:\n{}",
            result.log.lines().rev().take(10).collect::<Vec<_>>().join("\n")
        );
        if result.saved {
            keep_compressed(&result.plugin, &saved_file)?;
        } else {
            fs::write(&unchanged_file, "")?;
        }
        let log_text = work.join("log.txt");
        fs::write(&log_text, &result.log)?;
        keep_compressed(&log_text, &log_file)?;
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
    let oracle_passes = passes_of(oracle_log.lines());
    outcome.saved = saved_file.exists();
    outcome.passes = oracle_passes
        .iter()
        .map(|pass| format!("{}/{}/{}", pass.udr.1, pass.itm.1, pass.nav))
        .collect();
    let Some(port) = &runner.port else {
        return Ok(outcome);
    };

    let port_dir = runner.scratch.join(format!("{}-clean", game.mode));
    fs::create_dir_all(&port_dir)?;
    let private = private_data(&port_dir.join(format!("work-{name}")), &plugins)?;
    let port_saved = port_dir.join(format!("{stem}.port.saved"));
    let _ = fs::remove_file(&port_saved);
    let port_log = port_dir.join(format!("{stem}.port.log"));
    let started = std::time::Instant::now();
    let output = std::process::Command::new(port)
        .args(["--json", "--edit", "--game", game.mode, "--load"])
        .arg(find_in(&private, name)?)
        .args(["clean", "--quick", "--no-backup", "--output"])
        .arg(&port_saved)
        .stdin(std::process::Stdio::null())
        .stderr(File::create(&port_log)?)
        .output()?;
    outcome.port_seconds = Some(started.elapsed().as_secs_f64());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    if envelope["ok"] != Value::Bool(true) {
        outcome.status = "port-failed";
        outcome.detail = Some(format!(
            "  {}, see {}",
            String::from_utf8_lossy(&output.stdout).trim(),
            port_log.display()
        ));
        return Ok(outcome);
    }
    let result = &envelope["result"];
    let messages: Vec<&str> = result["messages"]
        .as_array()
        .map(|lines| lines.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let port_passes = passes_of(messages.into_iter());

    let mut lines = Vec::new();
    if port_passes.len() != oracle_passes.len() {
        lines.push(format!(
            "  passes: oracle {}, port {}",
            oracle_passes.len(),
            port_passes.len()
        ));
    }
    for (index, (oracle, port)) in oracle_passes.iter().zip(&port_passes).enumerate() {
        let pass = index + 1;
        if oracle.filter != port.filter {
            lines.push(format!(
                "  pass {pass} filter: oracle {:?}, port {:?}",
                oracle.filter, port.filter
            ));
        }
        if oracle.udr != port.udr || oracle.nav != port.nav {
            lines.push(format!(
                "  pass {pass} UDR (processed, undeleted) nav: oracle {:?} {}, port {:?} {}",
                oracle.udr, oracle.nav, port.udr, port.nav
            ));
        }
        if oracle.itm != port.itm {
            lines.push(format!(
                "  pass {pass} ITM (processed, removed): oracle {:?}, port {:?}",
                oracle.itm, port.itm
            ));
        }
        if oracle.lines != port.lines {
            let mut shown = 0;
            for line in oracle.lines.iter().filter(|line| !port.lines.contains(line)) {
                if shown < LISTED_DIFFERENCES {
                    lines.push(format!("  pass {pass} only the oracle: {line}"));
                }
                shown += 1;
            }
            for line in port.lines.iter().filter(|line| !oracle.lines.contains(line)) {
                if shown < LISTED_DIFFERENCES {
                    lines.push(format!("  pass {pass} only the port: {line}"));
                }
                shown += 1;
            }
            if shown == 0 {
                lines.push(format!("  pass {pass}: the same lines in another order"));
            }
        }
    }
    match (outcome.saved, port_saved.exists()) {
        (true, true) => {
            let oracle_bytes = read_all(&saved_file)?;
            let port_bytes = fs::read(&port_saved)?;
            if oracle_bytes != port_bytes {
                let offset = oracle_bytes
                    .iter()
                    .zip(&port_bytes)
                    .position(|(a, b)| a != b)
                    .unwrap_or(oracle_bytes.len().min(port_bytes.len()));
                let oracle_copy = port_dir.join(format!("{stem}.oracle.saved"));
                fs::write(&oracle_copy, &oracle_bytes)?;
                lines.push(format!(
                    "  saved bytes differ at {offset:#x} (oracle {} bytes, port {} bytes): {} {}",
                    oracle_bytes.len(),
                    port_bytes.len(),
                    oracle_copy.display(),
                    port_saved.display()
                ));
            }
        }
        (false, false) => {}
        (oracle, port) => lines.push(format!("  saved: oracle {oracle}, port {port}")),
    }
    if lines.is_empty() {
        outcome.status = "equal";
        let _ = fs::remove_file(&port_saved);
    } else {
        outcome.status = "different";
        outcome.detail = Some(lines.join("\n"));
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_passes_of_a_log() {
        let log = "[00:02] Start: Applying Filter
[00:11] Done: Applying Filter, [Pass 1] Processed Records: 19264, [Pass 2] Processed Records: 18541, Remaining unfiltered nodes: 18541, Elapsed Time: 00:09
Undeleting: [REFR:0005F2E6] (in X)
Skipping: [NAVM:000F0660] (in X)
[Undeleting and Disabling References done]  Processed Records: 18540, Undeleted Records: 11, Elapsed Time: 00:00
<Warning: Plugin contains 5 deleted NavMeshes which can not be undeleted>
Removing: [NAVM:00041B98] (in X)
[Removing \"Identical to Master\" records done]  Processed Records: 18540, Removed Records: 255, Elapsed Time: 00:00";
        let passes = passes_of(log.lines());
        assert_eq!(passes.len(), 1);
        assert_eq!(passes[0].filter, (19264, 18541, 18541));
        assert_eq!(passes[0].udr, (18540, 11));
        assert_eq!(passes[0].nav, 5);
        assert_eq!(passes[0].itm, (18540, 255));
        assert_eq!(passes[0].lines.len(), 3);
    }
}
