// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity conflicts`: the conflict status of every record.
//!
//! For each game the GUI build of xEdit loads the vanilla plugins of the
//! corpus together and runs `oracle/conflicts.pas`, which writes the
//! `ConflictAll` and `ConflictThis` of every main record of every loaded
//! file as `ConflictAllForMainRecord` and `ConflictThisForMainRecord`
//! report them (`TfrmMain.ConflictLevelForMainRecord`): one line per record
//! that is not a single record, and a count per file of the single records.
//! The output is cached zstd-compressed as
//! `<cache>/<tag>/<MODE>-oracle-conflicts/<set>.<key>.txt.zst`; the key
//! hashes the plugins and the script. The port loads the same plugins in
//! the load order the GUI reports and runs `conflicts.list`; every record
//! is compared.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;

use super::gui;
use super::oracle_save::{find_in, keep_compressed, load_list, oracle_key, run_gui, zstd_reader};
use super::{GIB, Game, Options, Runner, is_vanilla};

/// The oracle script.
const CONFLICTS_SCRIPT: &str = include_str!("../../oracle/conflicts.pas");

/// The probe of single records (`--record`).
const PROBE_SCRIPT: &str = include_str!("../../oracle/conflicts-probe.pas");

/// The names of `TConflictAll` and `TConflictThis` in ordinal order, as the
/// script writes them and the port names them.
const CONFLICT_ALL: [&str; 7] = [
    "caUnknown",
    "caOnlyOne",
    "caNoConflict",
    "caConflictBenign",
    "caOverride",
    "caConflict",
    "caConflictCritical",
];
const CONFLICT_THIS: [&str; 12] = [
    "ctUnknown",
    "ctIgnored",
    "ctNotDefined",
    "ctIdenticalToMaster",
    "ctOnlyOne",
    "ctHiddenByModGroup",
    "ctMaster",
    "ctConflictBenign",
    "ctOverride",
    "ctIdenticalToMasterWinsConflict",
    "ctConflictWins",
    "ctConflictLoses",
];

/// How many differing records an outcome lists.
const LISTED_DIFFERENCES: usize = 20;

/// The result for one game.
#[derive(Serialize)]
struct ConflictOutcome {
    game: &'static str,
    /// `equal`, `different`, `oracle-failed`, `oracle-unsupported`,
    /// `port-failed` or `oracle-only`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    files: usize,
    /// Records of the loaded files, single records included.
    records: u64,
    /// Records that are not single records, by the oracle.
    listed: usize,
    /// Records whose status differs (or that only one side lists).
    different: usize,
    /// Records by oracle status, `<ConflictAll>/<ConflictThis>`.
    by_status: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oracle_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_seconds: Option<f64>,
}

#[derive(Serialize)]
struct ConflictReport<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    outcomes: &'a [ConflictOutcome],
}

/// What the oracle wrote: the files in load order, the single record count
/// and the record count of each file, and the other records.
#[derive(Default)]
struct Statuses {
    files: Vec<String>,
    /// File (lower case) to (single records, records).
    counts: BTreeMap<String, (u64, u64)>,
    /// (file lower case, FormID) to (signature, ConflictAll, ConflictThis).
    records: BTreeMap<(String, String), (String, String, String)>,
}

fn read_oracle(path: &Path) -> Result<Statuses> {
    let mut statuses = Statuses::default();
    for line in BufReader::new(zstd_reader(path)?).lines() {
        let line = line?;
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["file", name, _] => statuses.files.push((*name).to_owned()),
            ["singles", name, singles, count] => {
                statuses
                    .counts
                    .insert(name.to_lowercase(), (singles.parse()?, count.parse()?));
            }
            [file, form_id, signature, all, this] => {
                let all = CONFLICT_ALL
                    .get(all.parse::<usize>()?)
                    .with_context(|| format!("bad ConflictAll in {line}"))?;
                let this = CONFLICT_THIS
                    .get(this.parse::<usize>()?)
                    .with_context(|| format!("bad ConflictThis in {line}"))?;
                statuses.records.insert(
                    (file.to_lowercase(), form_id.to_uppercase()),
                    ((*signature).to_owned(), (*all).to_owned(), (*this).to_owned()),
                );
            }
            _ => anyhow::bail!("unexpected line in {}: {line}", path.display()),
        }
    }
    Ok(statuses)
}

fn read_port(result: &Value) -> Result<Statuses> {
    let mut statuses = Statuses::default();
    for file in result["files"].as_array().context("no files in the port's result")? {
        let name = file["name"].as_str().context("file without a name")?;
        statuses.files.push(name.to_owned());
        statuses.counts.insert(
            name.to_lowercase(),
            (
                file["single"].as_u64().context("file without single")?,
                file["records"].as_u64().context("file without records")?,
            ),
        );
    }
    for record in result["records"]
        .as_array()
        .context("no records in the port's result")?
    {
        let text = |name: &str| record[name].as_str().map(str::to_owned).unwrap_or_default();
        statuses.records.insert(
            (text("file").to_lowercase(), text("form_id").to_uppercase()),
            (text("signature"), text("conflict_all"), text("conflict_this")),
        );
    }
    Ok(statuses)
}

/// The vanilla plugins of a game in its data folder (or the plugins named
/// with `--file`), with their masters, masters first.
fn corpus(game: &Game, data: &Path, options: &Options) -> Result<Vec<PathBuf>> {
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
    // The game master first, then the rest by name; the GUI orders them
    // itself and reports its order.
    names.sort_by_key(|name| (!name.eq_ignore_ascii_case(game.vanilla[0]), name.to_lowercase()));
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    load_list(game, data, &names)
}

/// A data folder with only `plugins` in it, as hard links where the file
/// system allows them (copies otherwise), like the GUI's private folder.
fn private_data(dir: &Path, plugins: &[PathBuf]) -> Result<PathBuf> {
    let data = dir.join("Data");
    fs::create_dir_all(&data)?;
    let wanted: std::collections::HashSet<String> = plugins
        .iter()
        .filter_map(|plugin| plugin.file_name().map(|name| name.to_string_lossy().to_lowercase()))
        .collect();
    for entry in fs::read_dir(&data)? {
        let path = entry?.path();
        let name = path.file_name().map(|name| name.to_string_lossy().to_lowercase());
        if name.is_none_or(|name| !wanted.contains(&name)) {
            fs::remove_file(&path)?;
        }
    }
    for plugin in plugins {
        let target = data.join(plugin.file_name().context("plugin without a name")?);
        let source = fs::metadata(plugin)?;
        let same = fs::metadata(&target)
            .is_ok_and(|current| current.len() == source.len() && current.modified().ok() == source.modified().ok());
        if same {
            continue;
        }
        let _ = fs::remove_file(&target);
        if fs::hard_link(plugin, &target).is_err() {
            fs::copy(plugin, &target).with_context(|| format!("copying {}", plugin.display()))?;
        }
    }
    Ok(data)
}

/// `--record`: runs the probe script on the records and prints what the
/// oracle reports for each (not cached).
fn probe(runner: &Runner, game: &'static Game, data: &Path, options: &Options) -> Result<()> {
    let plugins = corpus(game, data, options)?;
    let script = PROBE_SCRIPT
        .replace("\r\n", "\n")
        .replace("{{RECORDS}}", &options.records.join(","));
    let work = runner
        .scratch
        .join("oracle-work")
        .join(format!("{}-probe.{}", game.mode, std::process::id()));
    let dir = runner.scratch.join(format!("{}-conflicts", game.mode));
    fs::create_dir_all(&dir)?;
    let result = run_gui(
        runner,
        game,
        plugins,
        script,
        false,
        work.clone(),
        &dir.join("probe.peak"),
    )?;
    let text = fs::read_to_string(result.out.join("probe.txt")).unwrap_or_default();
    println!("{}", result.status.join(" / "));
    println!("{text}");
    gui::remove_work(&work);
    Ok(())
}

pub(super) fn run_conflicts(
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
    for &game in &options.games {
        let Some(data) = std::env::var_os(game.data_var).map(PathBuf::from) else {
            ensure!(options.all_games, "environment variable {} is not set", game.data_var);
            println!("skipped       {}: {} is not set", game.name, game.data_var);
            continue;
        };
        if !options.records.is_empty() {
            probe(&runner, game, &data, options)?;
            continue;
        }
        let outcome = match check_game(&runner, game, &data, options) {
            Ok(outcome) => outcome,
            Err(error) => ConflictOutcome {
                game: game.name,
                status: "oracle-failed",
                detail: Some(format!("  {error:#}")),
                files: 0,
                records: 0,
                listed: 0,
                different: 0,
                by_status: BTreeMap::new(),
                oracle_seconds: None,
                port_seconds: None,
            },
        };
        println!(
            "{:13} {} ({} files, {} records, {} listed, {} different)",
            outcome.status, outcome.game, outcome.files, outcome.records, outcome.listed, outcome.different
        );
        if let Some(detail) = &outcome.detail {
            println!("{detail}");
        }
        outcomes.push(outcome);
    }
    let equal = outcomes.iter().filter(|o| o.status == "equal").count();
    let report = ConflictReport {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("conflicts.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    println!(
        "{equal} of {} games equal. Report: {}",
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

fn check_game(runner: &Runner, game: &'static Game, data: &Path, options: &Options) -> Result<ConflictOutcome> {
    let mut outcome = ConflictOutcome {
        game: game.name,
        status: "oracle-only",
        detail: None,
        files: 0,
        records: 0,
        listed: 0,
        different: 0,
        by_status: BTreeMap::new(),
        oracle_seconds: None,
        port_seconds: None,
    };
    // The 4.1.5q GUI runs Morrowind in its view mode only, without scripts.
    if game.mode == "TES3" {
        outcome.status = "oracle-unsupported";
        return Ok(outcome);
    }
    let plugins = corpus(game, data, options)?;
    ensure!(!plugins.is_empty(), "no plugin of {} selected", game.name);
    let script = CONFLICTS_SCRIPT.replace("\r\n", "\n");
    let key = oracle_key(runner, &plugins, &script, gui::exe_name(game.mode))?;
    let dir = runner.cache.join(format!("{}-oracle-conflicts", game.mode));
    fs::create_dir_all(&dir)?;
    let set = if options.files.is_empty() {
        "vanilla"
    } else {
        "selected"
    };
    let stem = format!("{set}.{key:016x}");
    let cached = dir.join(format!("{stem}.txt.zst"));
    let error_file = dir.join(format!("{stem}.error"));
    if !cached.exists() && !error_file.exists() {
        let work = runner
            .scratch
            .join("oracle-work")
            .join(format!("{}-conflicts.{}", game.mode, std::process::id()));
        let peak_file = dir.join(format!("{stem}.oracle.peak"));
        let started = std::time::Instant::now();
        let result = run_gui(runner, game, plugins.clone(), script, false, work.clone(), &peak_file)?;
        outcome.oracle_seconds = Some(started.elapsed().as_secs_f64());
        match result.status.last().map(String::as_str) {
            Some("done") => keep_compressed(&result.out.join("conflicts.txt"), &cached)?,
            last => {
                let message = last.unwrap_or("no status").trim_start_matches("error: ").to_owned();
                fs::write(&error_file, message)?;
            }
        }
        fs::write(
            dir.join(format!("{stem}.oracle.seconds")),
            format!("{:.1}", started.elapsed().as_secs_f64()),
        )?;
        gui::remove_work(&work);
    }
    if error_file.exists() {
        outcome.status = "oracle-failed";
        outcome.detail = Some(format!(
            "  the oracle script stopped: {}",
            fs::read_to_string(&error_file)?.trim()
        ));
        return Ok(outcome);
    }
    if outcome.oracle_seconds.is_none() {
        outcome.oracle_seconds = fs::read_to_string(dir.join(format!("{stem}.oracle.seconds")))
            .ok()
            .and_then(|text| text.trim().parse().ok());
    }
    let oracle = read_oracle(&cached)?;
    outcome.files = oracle.files.len();
    outcome.records = oracle.counts.values().map(|(_, count)| count).sum();
    outcome.listed = oracle.records.len();
    for (_, all, this) in oracle.records.values() {
        *outcome.by_status.entry(format!("{all}/{this}")).or_default() += 1;
    }
    let singles: u64 = oracle.counts.values().map(|(singles, _)| singles).sum();
    if singles > 0 {
        outcome
            .by_status
            .insert("caOnlyOne/ctOnlyOne".to_owned(), singles as usize);
    }
    let Some(port) = &runner.port else {
        return Ok(outcome);
    };

    // The port loads the plugins in the GUI's load order (the hardcoded
    // file, the game's executable, loads with them) from a folder that has
    // only the plugins, as the GUI's private data folder: no archive and no
    // strings file, so the localized strings resolve the same on both
    // sides.
    let port_dir = runner.scratch.join(format!("{}-conflicts", game.mode));
    let private_data = private_data(&port_dir, &plugins)?;
    let mut command = std::process::Command::new(port);
    command.args(["--json", "--game", game.mode]);
    for name in &oracle.files {
        if name.to_lowercase().ends_with(".exe") {
            continue;
        }
        command.arg("--load").arg(find_in(&private_data, name)?);
    }
    let port_log = port_dir.join(format!("{stem}.port.log"));
    let started = std::time::Instant::now();
    let output = command
        .args(["call", "conflicts.list", "--params", r#"{"include_single": false}"#])
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
    let port = read_port(&envelope["result"])?;

    let mut lines = Vec::new();
    let oracle_files: Vec<String> = oracle.files.iter().map(|f| f.to_lowercase()).collect();
    let port_files: Vec<String> = port.files.iter().map(|f| f.to_lowercase()).collect();
    if oracle_files != port_files {
        lines.push(format!(
            "  load order differs: oracle {:?}, port {:?}",
            oracle.files, port.files
        ));
    }
    for (file, counts) in &oracle.counts {
        match port.counts.get(file) {
            Some(port_counts) if port_counts == counts => {}
            other => lines.push(format!(
                "  {file}: oracle {} single of {} records, port {:?}",
                counts.0, counts.1, other
            )),
        }
    }
    let mut different = 0;
    for (key, value) in &oracle.records {
        match port.records.get(key) {
            Some(port_value) if port_value == value => {}
            other => {
                different += 1;
                if different <= LISTED_DIFFERENCES {
                    lines.push(format!(
                        "  {} [{}:{}] oracle {}/{}, port {}",
                        key.0,
                        value.0,
                        key.1,
                        value.1,
                        value.2,
                        other.map_or("not listed (single)".to_owned(), |(_, all, this)| format!(
                            "{all}/{this}"
                        ))
                    ));
                }
            }
        }
    }
    for (key, value) in &port.records {
        if !oracle.records.contains_key(key) {
            different += 1;
            if different <= LISTED_DIFFERENCES {
                lines.push(format!(
                    "  {} [{}:{}] oracle single, port {}/{}",
                    key.0, value.0, key.1, value.1, value.2
                ));
            }
        }
    }
    outcome.different = different;
    if lines.is_empty() {
        outcome.status = "equal";
    } else {
        outcome.status = "different";
        if different > LISTED_DIFFERENCES {
            lines.push(format!("  ... {} more", different - LISTED_DIFFERENCES));
        }
        outcome.detail = Some(lines.join("\n"));
    }
    Ok(outcome)
}
