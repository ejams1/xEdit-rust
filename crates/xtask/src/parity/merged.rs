// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity merged`: the merged patch (phase 4 step 8).
//!
//! Each scenario of `crates/xtask/oracle/merged/*.json` names a game, the
//! plugins to load and the name of the patch, and optionally mod group
//! files with a settings file and the menu items that activate them, as the
//! scenarios of `parity modgroups` do. The GUI build of xEdit runs
//! `oracle/merged.pas` in its script mode: the script clicks the menu items
//! and then "Create Merged Patch" (`mniNavCreateMergedPatch`), the harness
//! answers the handler's dialogs (the warning of the games from Skyrim on,
//! the file name `InputQuery` asks for), and the script writes the patch as
//! the GUI saves it. The port loads the same plugins in the order the GUI
//! loaded them, runs the scenario's commands and `patch.merged` with `save`,
//! and the two patches are compared byte for byte.
//!
//! The oracle's outputs are cached in
//! `<cache>/<tag>/<MODE>-oracle-merged/<scenario>.<key>/`; the key hashes
//! the plugins, the script and the scenario.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::gui::{self, Answer, GuiExtras};
use super::modgroups::{GLOBAL_FILE, Text, put_files, settings_name};
use super::oracle_save::{
    find_in, first_difference, keep_compressed, load_list, oracle_key, run_gui_with, text_hash, zstd_reader,
};
use super::{GAMES, GIB, Game, Options, Runner, describe_record_differences, main_record_header_size};

/// The oracle script.
const MERGED_SCRIPT: &str = include_str!("../../oracle/merged.pas");

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct ScenarioAnswer {
    class: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    keys: Vec<u16>,
    button: Option<String>,
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    #[allow(dead_code)]
    description: String,
    game: String,
    /// The plugins, masters first.
    load: Vec<String>,
    /// Commands the port runs first, as `xedit batch` takes them, on the
    /// plugins of `load`, to make the plugins the scenario adds to them:
    /// each plugin a `files.new` makes is saved and loaded after `load`, in
    /// the order they were made.
    #[serde(default)]
    make: Vec<Value>,
    /// The name of the patch as typed into the GUI's `InputQuery`.
    patch: String,
    /// Mod group files by name, as in `parity modgroups`.
    #[serde(default)]
    files: BTreeMap<String, Text>,
    /// The settings file of the GUI (`Plugins.<app>viewsettings`).
    settings: Option<Text>,
    /// The menu items the GUI clicks before "Create Merged Patch".
    #[serde(default)]
    menu: Vec<String>,
    /// The dialogs the GUI shows, in order, and how they are answered: those
    /// of the menu items, then those of the merged patch.
    #[serde(default)]
    answers: Vec<ScenarioAnswer>,
    /// The commands the port runs before `patch.merged`, as `xedit batch`
    /// takes them.
    #[serde(default)]
    port: Vec<Value>,
}

/// The result for one scenario.
#[derive(Serialize)]
struct MergedOutcome {
    scenario: String,
    game: &'static str,
    /// `equal`, `different`, `oracle-failed`, `port-failed` or `oracle-only`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// Files loaded, by the oracle.
    files: usize,
    /// Size of the oracle's patch.
    oracle_bytes: u64,
    /// Size of the port's patch.
    port_bytes: u64,
    /// Records with two overrides or more the port compared.
    checked: u64,
    /// Records the port's patch overrides.
    records: u64,
    /// Lines the port wrote to the log (faulty ordered lists, errors).
    messages: usize,
}

#[derive(Serialize)]
struct MergedReport<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    outcomes: &'a [MergedOutcome],
}

/// The name the GUI gives the patch (`AddNewFile`): the trimmed name
/// without a plugin extension, with `.esp`.
fn patch_file_name(typed: &str) -> String {
    let name = typed.trim();
    let lower = name.to_lowercase();
    let base = if [".esp", ".esm", ".esl"].iter().any(|ext| lower.ends_with(ext)) {
        &name[..name.len() - 4]
    } else {
        name
    };
    format!("{base}.esp")
}

pub(super) fn run_merged(
    root: &Path,
    tag: &str,
    options: &Options,
    cache: PathBuf,
    scratch: PathBuf,
    oracle_dir: PathBuf,
) -> Result<()> {
    // The port makes the plugins of the scenarios with `make`, also for
    // `--oracle-only`.
    let maker = super::build_port(root)?;
    let port = if options.oracle_only { None } else { Some(maker.clone()) };
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
    let dir = root.join("crates/xtask/oracle/merged");
    let mut names: Vec<PathBuf> = fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    names.sort();
    let mut outcomes = Vec::new();
    for path in names {
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        if !options.files.is_empty() && !options.files.contains(&stem.to_lowercase()) {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let scenario: Scenario = serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        let game = GAMES
            .iter()
            .find(|game| game.name == scenario.game)
            .with_context(|| format!("{stem}: unknown game {}", scenario.game))?;
        if !options.all_games && !options.games.iter().any(|selected| selected.name == game.name) {
            continue;
        }
        let Some(data) = std::env::var_os(game.data_var).map(PathBuf::from) else {
            println!("skipped       {stem}: {} is not set", game.data_var);
            continue;
        };
        let outcome = match check(&runner, &maker, game, &data, &stem, &scenario, &text) {
            Ok(outcome) => outcome,
            Err(error) => MergedOutcome {
                scenario: stem.clone(),
                game: game.name,
                status: "oracle-failed",
                detail: Some(format!("  {error:#}")),
                files: 0,
                oracle_bytes: 0,
                port_bytes: 0,
                checked: 0,
                records: 0,
                messages: 0,
            },
        };
        println!(
            "{:13} {} ({}, {} files, {} records checked, {} merged, {} messages, oracle {} bytes, port {} bytes)",
            outcome.status,
            outcome.scenario,
            outcome.game,
            outcome.files,
            outcome.checked,
            outcome.records,
            outcome.messages,
            outcome.oracle_bytes,
            outcome.port_bytes
        );
        if let Some(detail) = &outcome.detail {
            println!("{detail}");
        }
        outcomes.push(outcome);
    }
    let equal = outcomes.iter().filter(|o| o.status == "equal").count();
    let report = MergedReport {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("merged.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    println!(
        "{equal} of {} scenarios equal. Report: {}",
        outcomes.len(),
        report_file.display()
    );
    if runner.port.is_some() {
        let failed = outcomes.iter().filter(|o| o.status != "equal").count();
        ensure!(failed == 0, "parity does not hold");
    }
    Ok(())
}

fn check(
    runner: &Runner,
    maker: &Path,
    game: &'static Game,
    data: &Path,
    stem: &str,
    scenario: &Scenario,
    scenario_text: &str,
) -> Result<MergedOutcome> {
    let mut outcome = MergedOutcome {
        scenario: stem.to_owned(),
        game: game.name,
        status: "oracle-only",
        detail: None,
        files: 0,
        oracle_bytes: 0,
        port_bytes: 0,
        checked: 0,
        records: 0,
        messages: 0,
    };
    let names: Vec<&str> = scenario.load.iter().map(String::as_str).collect();
    let mut plugins = load_list(game, data, &names)?;
    if !scenario.make.is_empty() {
        let made = make_plugins(runner, maker, game, stem, &plugins, &scenario.make)
            .with_context(|| format!("{stem}: making the plugins"))?;
        plugins.extend(made);
    }
    let files = put_files(&scenario.files, scenario.settings.as_ref(), game, &plugins)?;
    let patch = patch_file_name(&scenario.patch);

    let mut actions: String = scenario
        .menu
        .iter()
        .map(|item| format!("frmMain.FindComponent('{item}').Click;\n      "))
        .collect();
    actions.push_str("frmMain.FindComponent('mniNavCreateMergedPatch').Click;");
    let script = MERGED_SCRIPT
        .replace("\r\n", "\n")
        .replace("{{ACTIONS}}", &actions)
        .replace("{{FILE}}", &patch.replace('\'', "''"));
    let key = oracle_key(
        runner,
        &plugins,
        &format!("{script}\n{}", text_hash(scenario_text)),
        gui::exe_name(game.mode),
    )?;
    let cache_dir = runner
        .cache
        .join(format!("{}-oracle-merged", game.mode))
        .join(format!("{stem}.{key:016x}"));
    if !cache_dir.join("status").exists() {
        run_oracle(runner, game, &plugins, script, scenario, &files, &cache_dir, stem)?;
    }
    let status = fs::read_to_string(cache_dir.join("status"))?;
    if status.trim() != "done" {
        outcome.status = "oracle-failed";
        outcome.detail = Some(format!("  the oracle script stopped: {}", status.trim()));
        return Ok(outcome);
    }
    let loaded: Vec<String> = fs::read_to_string(cache_dir.join("files.txt"))?
        .lines()
        .map(str::to_owned)
        .filter(|name| !name.is_empty())
        .collect();
    outcome.files = loaded.len();
    let oracle_saved = cache_dir.join("saved.zst");
    let mut oracle_bytes = Vec::new();
    std::io::Read::read_to_end(&mut zstd_reader(&oracle_saved)?, &mut oracle_bytes)?;
    outcome.oracle_bytes = oracle_bytes.len() as u64;
    let Some(port) = &runner.port else {
        return Ok(outcome);
    };

    let dir = runner.scratch.join(format!("{}-merged", game.mode)).join(stem);
    let saved = dir.join(format!("{patch}.port.saved"));
    let result = match run_port(
        runner, port, game, scenario, &plugins, &files, &loaded, &patch, &dir, &saved,
    ) {
        Ok(result) => result,
        Err(error) => {
            outcome.status = "port-failed";
            outcome.detail = Some(format!("  {error:#}"));
            return Ok(outcome);
        }
    };
    outcome.checked = result["checked"].as_u64().unwrap_or(0);
    outcome.records = result["records"].as_array().map_or(0, |records| records.len() as u64);
    let messages: Vec<String> = result["messages"]
        .as_array()
        .map(|lines| lines.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    outcome.messages = messages.len();
    outcome.port_bytes = fs::metadata(&saved)?.len();
    let mut lines = Vec::new();
    if let Some(offset) = first_difference(oracle_bytes.as_slice(), File::open(&saved)?)? {
        let oracle_copy = dir.join(format!("{patch}.oracle.saved"));
        fs::write(&oracle_copy, &oracle_bytes)?;
        let port_bytes = fs::read(&saved)?;
        let (_, report) = describe_record_differences(&oracle_bytes, &port_bytes, main_record_header_size(game.mode));
        lines.push(format!(
            "  first difference at byte {offset} (0x{offset:X}); oracle {} bytes, port {} bytes\n  {report} (dropped: only in the oracle's patch, added: only in the port's)\n  oracle: {}\n  port:   {}",
            oracle_bytes.len(),
            port_bytes.len(),
            oracle_copy.display(),
            saved.display()
        ));
    }
    // The handler's own log lines: the faulty ordered lists.
    let log = fs::read_to_string(cache_dir.join("log.txt")).unwrap_or_default();
    let oracle_messages = log_messages(&log);
    let port_messages: Vec<String> = messages
        .into_iter()
        .filter(|line| line.starts_with("Error: Can't merge"))
        .collect();
    if oracle_messages != port_messages {
        lines.push(format!(
            "  log lines differ:\n    oracle {oracle_messages:?}\n    port   {port_messages:?}"
        ));
    }
    if lines.is_empty() {
        outcome.status = "equal";
        let _ = fs::remove_file(&saved);
    } else {
        outcome.status = "different";
        outcome.detail = Some(lines.join("\n"));
    }
    Ok(outcome)
}

/// Runs the `make` commands of a scenario on the port and returns the
/// plugins they made, saved into the scratch folder with their dates one
/// minute apart in the order they were made (the games up to New Vegas load
/// their plugins by date).
fn make_plugins(
    runner: &Runner,
    port: &Path,
    game: &Game,
    stem: &str,
    plugins: &[PathBuf],
    make: &[Value],
) -> Result<Vec<PathBuf>> {
    let dir = runner
        .scratch
        .join(format!("{}-merged", game.mode))
        .join(stem)
        .join("make");
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    let made_dir = dir.join("made");
    fs::create_dir_all(&made_dir)?;
    let data = super::conflicts::private_data(&dir, plugins)?;
    let mut commands = make.to_vec();
    let mut made = Vec::new();
    for command in make {
        if command["command"] == "files.new" {
            let name = command["params"]["file"].as_str().context("files.new without a file")?;
            let name = patch_file_name(name);
            let output = made_dir.join(&name);
            commands.push(json!({
                "command": "files.save",
                "params": { "file": name, "output": output, "backup": false }
            }));
            made.push(output);
        }
    }
    let batch = dir.join("make.json");
    fs::write(&batch, serde_json::to_string_pretty(&commands)?)?;
    let mut command = std::process::Command::new(port);
    command.args(["--json", "--edit", "--game", game.mode]);
    for plugin in plugins {
        command
            .arg("--load")
            .arg(data.join(plugin.file_name().context("plugin without a name")?));
    }
    command.arg("batch").arg(&batch);
    let log = dir.join("make.log");
    let output = command
        .stdin(std::process::Stdio::null())
        .stderr(File::create(&log)?)
        .output()?;
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    ensure!(
        envelope["ok"] == Value::Bool(true),
        "{}, see {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        log.display()
    );
    for result in envelope["result"].as_array().context("the batch gave no results")? {
        ensure!(
            result["ok"] == Value::Bool(true),
            "{} failed: {}",
            result["command"],
            result["error"]
        );
    }
    let start = std::time::SystemTime::now();
    for (index, path) in made.iter().enumerate() {
        File::options()
            .write(true)
            .open(path)?
            .set_modified(start + std::time::Duration::from_secs(60 * (index as u64 + 1)))?;
    }
    Ok(made)
}

/// The lines of the handler about faulty ordered lists in the GUI's
/// message log, between the script's markers.
fn log_messages(log: &str) -> Vec<String> {
    let mut inside = false;
    let mut messages = Vec::new();
    for line in log.lines() {
        if line.contains("[merged] action start") {
            inside = true;
            continue;
        }
        if line.contains("[merged] action end") {
            break;
        }
        if inside && line.starts_with("Error: Can't merge") {
            messages.push(line.to_owned());
        }
    }
    messages
}

#[allow(clippy::too_many_arguments)]
fn run_oracle(
    runner: &Runner,
    game: &Game,
    plugins: &[PathBuf],
    script: String,
    scenario: &Scenario,
    files: &[(String, PathBuf, Vec<u8>)],
    cache_dir: &Path,
    stem: &str,
) -> Result<()> {
    let work = runner
        .scratch
        .join("oracle-work")
        .join(format!("{}-merged-{stem}.{}", game.mode, std::process::id()));
    let extras = GuiExtras {
        files: files
            .iter()
            .map(|(_, path, bytes)| (path.clone(), bytes.clone()))
            .collect(),
        answers: scenario
            .answers
            .iter()
            .map(|answer| Answer {
                class: answer.class.clone(),
                title: answer.title.clone(),
                keys: answer.keys.clone(),
                button: answer.button.clone(),
                text: answer.text.clone(),
            })
            .collect(),
        close_after: false,
    };
    let peak_file = runner
        .cache
        .join(format!("{}-oracle-merged", game.mode))
        .join(format!("{stem}.oracle.peak"));
    fs::create_dir_all(peak_file.parent().context("no cache folder")?)?;
    let result = run_gui_with(
        runner,
        game,
        plugins.to_vec(),
        script,
        false,
        work.clone(),
        &peak_file,
        &extras,
    )?;
    let partial = cache_dir.with_extension(format!("partial{}", std::process::id()));
    if partial.exists() {
        fs::remove_dir_all(&partial)?;
    }
    fs::create_dir_all(&partial)?;
    fs::write(partial.join("log.txt"), &result.log)?;
    let last = result.status.last().cloned().unwrap_or_else(|| "no status".to_owned());
    if last == "done" {
        keep_compressed(&result.out.join("saved"), &partial.join("saved.zst"))?;
        fs::copy(result.out.join("files.txt"), partial.join("files.txt"))?;
    }
    fs::write(partial.join("status"), last)?;
    gui::remove_work(&work);
    if cache_dir.exists() {
        fs::remove_dir_all(&partial)?;
    } else {
        fs::rename(&partial, cache_dir)?;
    }
    Ok(())
}

/// Runs the scenario on the port and returns the result of `patch.merged`.
#[allow(clippy::too_many_arguments)]
fn run_port(
    runner: &Runner,
    port: &Path,
    game: &Game,
    scenario: &Scenario,
    plugins: &[PathBuf],
    files: &[(String, PathBuf, Vec<u8>)],
    loaded: &[String],
    patch: &str,
    dir: &Path,
    saved: &Path,
) -> Result<Value> {
    let _ = runner;
    let data = super::conflicts::private_data(dir, plugins)?;
    let global = dir.join("global.modgroups");
    let settings = dir.join(settings_name(game.mode));
    for path in [&global, &settings, &saved.to_path_buf()] {
        let _ = fs::remove_file(path);
    }
    for (name, _, bytes) in files {
        let path = if name == GLOBAL_FILE {
            global.clone()
        } else if *name == settings_name(game.mode) {
            settings.clone()
        } else {
            data.join(name)
        };
        fs::write(path, bytes)?;
    }
    let mut commands = scenario.port.clone();
    commands.push(json!({
        "command": "patch.merged",
        "params": { "file": patch, "save": true, "output": saved, "backup": false }
    }));
    let batch = dir.join("batch.json");
    fs::write(&batch, serde_json::to_string_pretty(&commands)?)?;
    let mut command = std::process::Command::new(port);
    command.args(["--json", "--edit", "--game", game.mode]);
    for name in loaded {
        let lower = name.to_lowercase();
        if lower.ends_with(".exe") || name.eq_ignore_ascii_case(patch) {
            continue;
        }
        command.arg("--load").arg(find_in(&data, name)?);
    }
    command
        .arg("--modgroups-file")
        .arg(&global)
        .arg("--settings")
        .arg(&settings)
        .arg("batch")
        .arg(&batch);
    let log = dir.join("port.log");
    let output = command
        .stdin(std::process::Stdio::null())
        .stderr(File::create(&log)?)
        .output()?;
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    if envelope["ok"] != Value::Bool(true) {
        bail!(
            "{}, see {}",
            String::from_utf8_lossy(&output.stdout).trim(),
            log.display()
        );
    }
    let results = envelope["result"].as_array().context("the batch gave no results")?;
    for result in results {
        if result["ok"] != Value::Bool(true) {
            bail!("{} failed: {}", result["command"], result["error"]);
        }
    }
    let last = results.last().context("the batch gave no results")?;
    Ok(last["result"].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_patch_gets_the_gui_name() {
        assert_eq!(patch_file_name(" Merged Patch "), "Merged Patch.esp");
        assert_eq!(patch_file_name("Merged.esm"), "Merged.esp");
    }

    #[test]
    fn faulty_list_lines_are_taken_between_the_markers() {
        let log = "Error: Can't merge x\n[merged] action start\nAdding\nError: Can't merge faulty ordered list A\n[merged] action end";
        assert_eq!(log_messages(log), ["Error: Can't merge faulty ordered list A"]);
    }
}
