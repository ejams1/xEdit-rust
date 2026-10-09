// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity modgroups`: mod groups (phase 4 step 6).
//!
//! Each scenario of `crates/xtask/oracle/modgroups/*.json` names a game,
//! the plugins to load, the mod group files to put next to them (and the
//! program's own `<AppName><ToolName>.modgroups`), the settings file with
//! the saved selection, the menu items the GUI clicks with the dialogs the
//! harness answers, and the commands the port runs for the same action.
//! The GUI build of xEdit runs `oracle/modgroups.pas` in its script mode:
//! the script clicks the menu items (the script mode itself never
//! activates mod groups), and once the action and the reload that ends it
//! are done writes the conflict status of every record, as
//! `parity conflicts` does. The port runs the scenario's commands as an
//! `xedit batch` and then `conflicts.list` with the saved selection. The
//! check compares the conflict statuses, every mod group file and the
//! settings file after the run byte for byte, and the validation messages
//! the reload writes to the log.
//!
//! The oracle's outputs are cached in
//! `<cache>/<tag>/<MODE>-oracle-modgroups/<scenario>.<key>/`; the key
//! hashes the plugins, the script and the scenario.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::conflicts::{self, LISTED_DIFFERENCES, Statuses};
use super::gui::{self, Answer, GuiExtras};
use super::oracle_save::{find_in, keep_compressed, load_list, oracle_key, run_gui_with, text_hash};
use super::{GAMES, GIB, Game, Options, Runner};

/// The oracle script.
const MODGROUPS_SCRIPT: &str = include_str!("../../oracle/modgroups.pas");

/// The name a scenario gives the program's own mod group file.
const GLOBAL_FILE: &str = "<global>";

/// The text of a file of a scenario: the exact text, or lines that are
/// written with CRLF after each, as xEdit writes them.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum Text {
    Raw(String),
    Lines(Vec<String>),
}

impl Text {
    fn text(&self) -> String {
        match self {
            Text::Raw(text) => text.clone(),
            Text::Lines(lines) => lines.iter().map(|line| format!("{line}\r\n")).collect(),
        }
    }
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct ScenarioAnswer {
    class: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    keys: Vec<u16>,
    button: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    #[allow(dead_code)]
    description: String,
    game: String,
    /// The plugins, masters first.
    load: Vec<String>,
    /// Mod group files by name (`<plugin>.modgroups` in the data folder, or
    /// `<global>`). `{{CRC:<plugin>}}` is the CRC32 of the plugin.
    #[serde(default)]
    files: BTreeMap<String, Text>,
    /// The settings file of the GUI (`Plugins.<app>viewsettings`).
    settings: Option<Text>,
    /// The menu items of the main form the GUI clicks, in order.
    menu: Vec<String>,
    /// The dialogs the GUI shows, in order, and how they are answered.
    #[serde(default)]
    answers: Vec<ScenarioAnswer>,
    /// The commands the port runs for the same action, as `xedit batch`
    /// takes them; `conflicts.list` with the saved selection follows.
    #[serde(default)]
    port: Vec<Value>,
}

/// The result for one scenario.
#[derive(Serialize)]
struct ModGroupsOutcome {
    scenario: String,
    game: &'static str,
    /// `equal`, `different`, `oracle-failed`, `port-failed` or `oracle-only`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// Records of the loaded files.
    records: u64,
    /// Records that are not single records, by the oracle.
    listed: usize,
    /// Records the mod groups hide, by the oracle.
    hidden: usize,
    /// Records whose status differs.
    different: usize,
    /// Files compared (mod group files and the settings file).
    files: usize,
    /// Validation messages compared.
    messages: usize,
}

#[derive(Serialize)]
struct ModGroupsReport<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    outcomes: &'a [ModGroupsOutcome],
}

/// The settings file of the GUI next to its plugin list (`-P:`), as
/// `xeInit` names it: `Plugins.<app>viewsettings` with the case of the list.
fn settings_name(mode: &str) -> String {
    format!("plugins.{}viewsettings", mode.to_lowercase())
}

/// What a run leaves: the mod group files and the settings file by name,
/// the validation messages of the reloads, the conflict statuses.
struct RunOutput {
    files: BTreeMap<String, Option<Vec<u8>>>,
    messages: Vec<String>,
    statuses: Statuses,
}

/// The validation lines (`ShowValidationMessages`) of the GUI's message
/// log between the script's markers.
fn log_messages(log: &str) -> Vec<String> {
    let mut inside = false;
    let mut messages = Vec::new();
    for line in log.lines() {
        if line.contains("[modgroups] action start") {
            inside = true;
            continue;
        }
        if line.contains("[modgroups] action end") {
            break;
        }
        if inside && (line.starts_with("ModGroup \"") || line.starts_with(" - ")) {
            messages.push(line.to_owned());
        }
    }
    messages
}

pub(super) fn run_modgroups(
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
    let dir = root.join("crates/xtask/oracle/modgroups");
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
        let outcome = match check(&runner, game, &data, &stem, &scenario, &text) {
            Ok(outcome) => outcome,
            Err(error) => ModGroupsOutcome {
                scenario: stem.clone(),
                game: game.name,
                status: "oracle-failed",
                detail: Some(format!("  {error:#}")),
                records: 0,
                listed: 0,
                hidden: 0,
                different: 0,
                files: 0,
                messages: 0,
            },
        };
        println!(
            "{:13} {} ({}, {} records, {} listed, {} hidden by mod groups, {} different, {} files, {} messages)",
            outcome.status,
            outcome.scenario,
            outcome.game,
            outcome.records,
            outcome.listed,
            outcome.hidden,
            outcome.different,
            outcome.files,
            outcome.messages
        );
        if let Some(detail) = &outcome.detail {
            println!("{detail}");
        }
        outcomes.push(outcome);
    }
    let equal = outcomes.iter().filter(|o| o.status == "equal").count();
    let report = ModGroupsReport {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("modgroups.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    println!(
        "{equal} of {} scenarios equal. Report: {}",
        outcomes.len(),
        report_file.display()
    );
    if !runner.port.is_none() {
        let failed = outcomes.iter().filter(|o| o.status != "equal").count();
        ensure!(failed == 0, "parity does not hold");
    }
    Ok(())
}

/// The scenario's text of a file with the CRC32s of the plugins filled in.
fn fill_crcs(text: &str, plugins: &[PathBuf]) -> Result<String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{CRC:") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 6..];
        let end = after.find("}}").context("unterminated {{CRC:")?;
        let name = &after[..end];
        let plugin = plugins
            .iter()
            .find(|plugin| {
                plugin
                    .file_name()
                    .is_some_and(|file| file.to_string_lossy().eq_ignore_ascii_case(name))
            })
            .with_context(|| format!("{{{{CRC:{name}}}}} names a plugin that is not loaded"))?;
        let crc = crc32(&fs::read(plugin)?);
        out.push_str(&format!("{crc:08X}"));
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The standard CRC-32 (as `TwbHash.CRC32`).
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// The files a scenario puts down: the path in the GUI's run folder, the
/// path in the port's folder and the bytes.
fn scenario_files(scenario: &Scenario, game: &Game, plugins: &[PathBuf]) -> Result<Vec<(String, PathBuf, Vec<u8>)>> {
    let mut files = Vec::new();
    for (name, text) in &scenario.files {
        let bytes = fill_crcs(&text.text(), plugins)?.into_bytes();
        let gui_path = if name == GLOBAL_FILE {
            // `wbModGroupFileName` of the script mode, next to the program.
            PathBuf::from("bin").join(format!("{}Script.modgroups", game.mode))
        } else {
            PathBuf::from("Data").join(name)
        };
        files.push((name.clone(), gui_path, bytes));
    }
    if let Some(settings) = &scenario.settings {
        files.push((
            settings_name(game.mode),
            PathBuf::from(settings_name(game.mode)),
            settings.text().into_bytes(),
        ));
    }
    Ok(files)
}

/// The names of the files the check compares after a run: every mod group
/// file the scenario has, the global one and the settings file.
fn compared_names(scenario: &Scenario, game: &Game) -> Vec<String> {
    let mut names: Vec<String> = scenario.files.keys().cloned().collect();
    if !names.iter().any(|name| name == GLOBAL_FILE) {
        names.push(GLOBAL_FILE.to_owned());
    }
    names.push(settings_name(game.mode));
    names
}

fn check(
    runner: &Runner,
    game: &'static Game,
    data: &Path,
    stem: &str,
    scenario: &Scenario,
    scenario_text: &str,
) -> Result<ModGroupsOutcome> {
    let mut outcome = ModGroupsOutcome {
        scenario: stem.to_owned(),
        game: game.name,
        status: "oracle-only",
        detail: None,
        records: 0,
        listed: 0,
        hidden: 0,
        different: 0,
        files: 0,
        messages: 0,
    };
    let names: Vec<&str> = scenario.load.iter().map(String::as_str).collect();
    let plugins = load_list(game, data, &names)?;
    let files = scenario_files(scenario, game, &plugins)?;
    let compared = compared_names(scenario, game);

    let actions: String = scenario
        .menu
        .iter()
        .map(|item| format!("frmMain.FindComponent('{item}').Click;\n      "))
        .collect();
    let script = MODGROUPS_SCRIPT.replace("\r\n", "\n").replace("{{ACTIONS}}", &actions);
    let key = oracle_key(
        runner,
        &plugins,
        &format!("{script}\n{}", text_hash(scenario_text)),
        gui::exe_name(game.mode),
    )?;
    let cache_dir = runner
        .cache
        .join(format!("{}-oracle-modgroups", game.mode))
        .join(format!("{stem}.{key:016x}"));
    if !cache_dir.join("status").exists() {
        run_oracle(
            runner, game, &plugins, script, scenario, &files, &compared, &cache_dir, stem,
        )?;
    }
    let status = fs::read_to_string(cache_dir.join("status"))?;
    if status.trim() != "done" {
        outcome.status = "oracle-failed";
        outcome.detail = Some(format!("  the oracle script stopped: {}", status.trim()));
        return Ok(outcome);
    }
    let oracle = read_output(&cache_dir, &compared)?;
    outcome.records = oracle.statuses.counts.values().map(|(_, count)| count).sum();
    outcome.listed = oracle.statuses.records.len();
    outcome.hidden = oracle
        .statuses
        .records
        .values()
        .filter(|(_, _, this)| this == "ctHiddenByModGroup")
        .count();
    outcome.files = oracle.files.len();
    outcome.messages = oracle.messages.len();
    let Some(port) = &runner.port else {
        return Ok(outcome);
    };

    let port_output = match run_port(runner, port, game, stem, scenario, &plugins, &files, &compared, &oracle) {
        Ok(output) => output,
        Err(error) => {
            outcome.status = "port-failed";
            outcome.detail = Some(format!("  {error:#}"));
            return Ok(outcome);
        }
    };
    let (different, mut lines) = conflicts::compare(&oracle.statuses, &port_output.statuses);
    outcome.different = different;
    if different > LISTED_DIFFERENCES {
        lines.push(format!("  ... {} more", different - LISTED_DIFFERENCES));
    }
    for (name, bytes) in &oracle.files {
        let port_bytes = port_output.files.get(name).cloned().flatten();
        if port_bytes != *bytes {
            lines.push(format!(
                "  {name} differs:\n    oracle {}\n    port   {}",
                describe(bytes.as_deref()),
                describe(port_bytes.as_deref())
            ));
        }
    }
    if oracle.messages != port_output.messages {
        lines.push(format!(
            "  validation messages differ:\n    oracle {:?}\n    port   {:?}",
            oracle.messages, port_output.messages
        ));
    }
    if lines.is_empty() {
        outcome.status = "equal";
    } else {
        outcome.status = "different";
        outcome.detail = Some(lines.join("\n"));
    }
    Ok(outcome)
}

/// A file's bytes for a report: its text with the line ends shown.
fn describe(bytes: Option<&[u8]>) -> String {
    match bytes {
        None => "(no file)".to_owned(),
        Some(bytes) => format!("{:?}", String::from_utf8_lossy(bytes)),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_oracle(
    runner: &Runner,
    game: &Game,
    plugins: &[PathBuf],
    script: String,
    scenario: &Scenario,
    files: &[(String, PathBuf, Vec<u8>)],
    compared: &[String],
    cache_dir: &Path,
    stem: &str,
) -> Result<()> {
    let work =
        runner
            .scratch
            .join("oracle-work")
            .join(format!("{}-modgroups-{stem}.{}", game.mode, std::process::id()));
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
            })
            .collect(),
        close_after: false,
    };
    let peak_file = runner
        .cache
        .join(format!("{}-oracle-modgroups", game.mode))
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
    fs::create_dir_all(partial.join("files"))?;
    fs::write(partial.join("log.txt"), &result.log)?;
    let last = result.status.last().cloned().unwrap_or_else(|| "no status".to_owned());
    if last == "done" {
        keep_compressed(&result.out.join("conflicts.txt"), &partial.join("conflicts.txt.zst"))?;
        for name in compared {
            let source = gui_path(game, &work, name);
            if source.exists() {
                fs::copy(&source, partial.join("files").join(file_key(name)))?;
            }
        }
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

/// Where the GUI's run folder has the file of a scenario name.
fn gui_path(game: &Game, work: &Path, name: &str) -> PathBuf {
    if name == GLOBAL_FILE {
        work.join("bin").join(format!("{}Script.modgroups", game.mode))
    } else if name == settings_name(game.mode) {
        work.join(name)
    } else {
        work.join("Data").join(name)
    }
}

/// The name a file of a scenario is kept under.
fn file_key(name: &str) -> String {
    if name == GLOBAL_FILE {
        "global.modgroups".to_owned()
    } else {
        name.to_owned()
    }
}

fn read_output(cache_dir: &Path, compared: &[String]) -> Result<RunOutput> {
    let mut files = BTreeMap::new();
    for name in compared {
        let path = cache_dir.join("files").join(file_key(name));
        files.insert(name.clone(), fs::read(&path).ok());
    }
    let log = fs::read_to_string(cache_dir.join("log.txt")).unwrap_or_default();
    Ok(RunOutput {
        files,
        messages: log_messages(&log),
        statuses: conflicts::read_oracle(&cache_dir.join("conflicts.txt.zst"))?,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_port(
    runner: &Runner,
    port: &Path,
    game: &Game,
    stem: &str,
    scenario: &Scenario,
    plugins: &[PathBuf],
    files: &[(String, PathBuf, Vec<u8>)],
    compared: &[String],
    oracle: &RunOutput,
) -> Result<RunOutput> {
    let dir = runner.scratch.join(format!("{}-modgroups", game.mode)).join(stem);
    let data = conflicts::private_data(&dir, plugins)?;
    let global = dir.join("global.modgroups");
    let settings = dir.join(settings_name(game.mode));
    for path in [&global, &settings] {
        let _ = fs::remove_file(path);
    }
    let port_path = |name: &str| -> PathBuf {
        if name == GLOBAL_FILE {
            global.clone()
        } else if name == settings_name(game.mode) {
            settings.clone()
        } else {
            data.join(name)
        }
    };
    for (name, _, bytes) in files {
        fs::write(port_path(name), bytes)?;
    }
    let mut commands = scenario.port.clone();
    commands.push(json!({ "command": "conflicts.list", "params": { "saved_mod_groups": true } }));
    let batch = dir.join("batch.json");
    fs::write(&batch, serde_json::to_string_pretty(&commands)?)?;
    let mut command = std::process::Command::new(port);
    command.args(["--json", "--edit", "--game", game.mode]);
    for name in &oracle.statuses.files {
        if name.to_lowercase().ends_with(".exe") {
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
    let mut messages = Vec::new();
    for result in results {
        if result["ok"] != Value::Bool(true) {
            bail!("{} failed: {}", result["command"], result["error"]);
        }
        if let Some(lines) = result["result"]["validation_messages"].as_array() {
            messages.extend(lines.iter().filter_map(Value::as_str).map(str::to_owned));
        }
    }
    let last = results.last().context("the batch gave no results")?;
    let statuses = conflicts::read_port(&last["result"])?;
    let mut out_files = BTreeMap::new();
    for name in compared {
        out_files.insert(name.clone(), fs::read(port_path(name)).ok());
    }
    Ok(RunOutput {
        files: out_files,
        messages,
        statuses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn validation_lines_are_taken_between_the_markers() {
        let log = "a\n[modgroups] action start\nModGroup \"A\" is invalid:\n - Error: x\nother\n[modgroups] action end\nModGroup \"B\" is invalid:";
        assert_eq!(log_messages(log), ["ModGroup \"A\" is invalid:", " - Error: x"]);
    }
}
