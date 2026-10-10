// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity filter`: the filter of the navigation tree.
//!
//! A scenario names a game, the plugins to load and a list of filter
//! configurations of `TfrmFilterOptions`. The GUI build of xEdit runs
//! `crates/xtask/oracle/filter.pas` in its script mode: the harness
//! generates one block per configuration that sets every filter variable
//! (as `Apply filter for cleaning.pas` does) and calls `ApplyFilter`, which
//! skips the options dialog (`FilterPreset`). The filter writes its counts
//! to the message log of the main form: `[file] Filtered n of m records`
//! per partly filtered file and the closing `Done: Applying Filter, [Pass
//! 1] Processed Records: ..., [Pass 2] Processed Records: ..., Remaining
//! unfiltered nodes: ...`, which the harness reads per configuration.
//!
//! The port runs the same configurations as one `xedit batch` (and
//! `refs.build_reachable` first when the scenario builds the reachable
//! information, which the oracle clicks `mniNavBuildReachable` for) and
//! compares the two passes, the nodes left and the records each file lost.
//! The oracle's log is cached in
//! `<cache>/<tag>/<MODE>-oracle-filter/<scenario>.<key>/`; the key hashes
//! the plugins, the generated script and the scenario.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::gui::{self, GuiRun};
use super::oracle_save::{load_list, oracle_key, text_hash};
use super::{GIB, Game, Options, Runner};

/// The oracle script.
const FILTER_SCRIPT: &str = include_str!("../../oracle/filter.pas");

/// Part of the cache key: changes to what the oracle run does go here.
const ORACLE_RECIPE: &str = "filter v1";

/// How many differing lines an outcome lists.
const LISTED_DIFFERENCES: usize = 12;

/// One filter configuration of a scenario: the fields of the filter dialog
/// (`TfrmFilterOptions`) with the Pascal names of the conflict statuses.
/// Every field is optional; the generator writes every variable for every
/// configuration, the ones a configuration leaves out at their defaults
/// (`FilterLoadPreset('')` of the dialog), because the variables keep their
/// values between two filters in the same session.
#[derive(Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
struct Config {
    /// The name the log and the report show.
    name: String,
    #[serde(default)]
    conflict_all: Option<Vec<String>>,
    #[serde(default)]
    conflict_this: Option<Vec<String>>,
    #[serde(default)]
    by_inject_status: Option<bool>,
    #[serde(default)]
    by_not_reachable_status: Option<bool>,
    #[serde(default)]
    by_references_injected_status: Option<bool>,
    #[serde(default)]
    by_editor_id: Option<String>,
    #[serde(default)]
    by_element_value: Option<String>,
    #[serde(default)]
    by_name: Option<String>,
    #[serde(default)]
    by_base_editor_id: Option<String>,
    #[serde(default)]
    by_base_name: Option<String>,
    #[serde(default)]
    scaled_actors: bool,
    #[serde(default)]
    by_signature: Option<String>,
    #[serde(default)]
    by_base_signature: Option<String>,
    #[serde(default)]
    by_persistent: bool,
    #[serde(default = "default_true")]
    persistent: bool,
    #[serde(default)]
    unnecessary_persistent: bool,
    #[serde(default)]
    master_is_temporary: bool,
    #[serde(default)]
    is_master: bool,
    #[serde(default)]
    persistent_pos_changed: bool,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    by_vwd: Option<bool>,
    #[serde(default)]
    by_has_vwd_mesh: Option<bool>,
    #[serde(default)]
    by_has_precombined_mesh: Option<bool>,
    #[serde(default)]
    regex_comparison: bool,
    #[serde(default)]
    flatten_blocks: bool,
    #[serde(default)]
    flatten_cell_childs: bool,
    #[serde(default)]
    assign_pers_wrld_child: bool,
    #[serde(default = "default_true")]
    inherit_conflict_by_parent: bool,
}

fn default_true() -> bool {
    true
}

impl Config {
    /// The Pascal statements of the configuration: every variable of the
    /// filter dialog, then `ApplyFilter`.
    fn pascal(&self) -> String {
        let mut text = String::new();
        let bool_value = |value: bool| if value { "True" } else { "False" };
        let set = |values: &Option<Vec<String>>| match values {
            Some(values) if !values.is_empty() => format!("[{}]", values.join(", ")),
            _ => "[]".to_owned(),
        };
        text.push_str(&format!(
            "FilterConflictAll := {};\n      ",
            bool_value(self.conflict_all.is_some())
        ));
        text.push_str(&format!("FilterConflictAllSet := {};\n      ", set(&self.conflict_all)));
        text.push_str(&format!(
            "FilterConflictThis := {};\n      ",
            bool_value(self.conflict_this.is_some())
        ));
        text.push_str(&format!(
            "FilterConflictThisSet := {};\n      ",
            set(&self.conflict_this)
        ));
        text.push_str(&format!(
            "FilterByInjectStatus := {};\n      FilterInjectStatus := {};\n      ",
            bool_value(self.by_inject_status.is_some()),
            bool_value(self.by_inject_status.unwrap_or(true))
        ));
        text.push_str(&format!(
            "FilterByNotReachableStatus := {};\n      FilterNotReachableStatus := {};\n      ",
            bool_value(self.by_not_reachable_status.is_some()),
            bool_value(self.by_not_reachable_status.unwrap_or(true))
        ));
        text.push_str(&format!(
            "FilterByReferencesInjectedStatus := {};\n      FilterReferencesInjectedStatus := {};\n      ",
            bool_value(self.by_references_injected_status.is_some()),
            bool_value(self.by_references_injected_status.unwrap_or(true))
        ));
        text.push_str(&format!(
            "FilterByEditorID := {};\n      FilterEditorID := '{}';\n      ",
            bool_value(self.by_editor_id.is_some()),
            self.by_editor_id.as_deref().unwrap_or("")
        ));
        text.push_str(&format!(
            "FilterByName := {};\n      FilterName := '{}';\n      ",
            bool_value(self.by_name.is_some()),
            self.by_name.as_deref().unwrap_or("")
        ));
        text.push_str(&format!(
            "FilterByBaseEditorID := {};\n      FilterBaseEditorID := '{}';\n      ",
            bool_value(self.by_base_editor_id.is_some()),
            self.by_base_editor_id.as_deref().unwrap_or("")
        ));
        text.push_str(&format!(
            "FilterByBaseName := {};\n      FilterBaseName := '{}';\n      ",
            bool_value(self.by_base_name.is_some()),
            self.by_base_name.as_deref().unwrap_or("")
        ));
        text.push_str(&format!(
            "FilterByElementValue := {};\n      FilterElementValue := '{}';\n      ",
            bool_value(self.by_element_value.is_some()),
            self.by_element_value.as_deref().unwrap_or("")
        ));
        text.push_str(&format!(
            "FilterByRegexComparison := {};\n      ",
            bool_value(self.regex_comparison)
        ));
        text.push_str(&format!(
            "FilterScaledActors := {};\n      ",
            bool_value(self.scaled_actors)
        ));
        text.push_str(&format!(
            "FilterBySignature := {};\n      FilterSignatures := '{}';\n      ",
            bool_value(self.by_signature.is_some()),
            self.by_signature.as_deref().unwrap_or("")
        ));
        text.push_str(&format!(
            "FilterByBaseSignature := {};\n      FilterBaseSignatures := '{}';\n      ",
            bool_value(self.by_base_signature.is_some()),
            self.by_base_signature.as_deref().unwrap_or("")
        ));
        text.push_str(&format!(
            "FilterByPersistent := {};\n      FilterPersistent := {};\n      ",
            bool_value(self.by_persistent),
            bool_value(self.persistent)
        ));
        text.push_str(&format!(
            "FilterUnnecessaryPersistent := {};\n      FilterMasterIsTemporary := {};\n      FilterIsMaster := {};\n      FilterPersistentPosChanged := {};\n      ",
            bool_value(self.unnecessary_persistent),
            bool_value(self.master_is_temporary),
            bool_value(self.is_master),
            bool_value(self.persistent_pos_changed)
        ));
        text.push_str(&format!("FilterDeleted := {};\n      ", bool_value(self.deleted)));
        text.push_str(&format!(
            "FilterByVWD := {};\n      FilterVWD := {};\n      ",
            bool_value(self.by_vwd.is_some()),
            bool_value(self.by_vwd.unwrap_or(true))
        ));
        text.push_str(&format!(
            "FilterByHasVWDMesh := {};\n      FilterHasVWDMesh := {};\n      ",
            bool_value(self.by_has_vwd_mesh.is_some()),
            bool_value(self.by_has_vwd_mesh.unwrap_or(true))
        ));
        text.push_str(&format!(
            "FilterByHasPrecombinedMesh := {};\n      FilterHasPrecombinedMesh := {};\n      ",
            bool_value(self.by_has_precombined_mesh.is_some()),
            bool_value(self.by_has_precombined_mesh.unwrap_or(true))
        ));
        text.push_str(&format!(
            "FlattenBlocks := {};\n      ",
            bool_value(self.flatten_blocks)
        ));
        text.push_str(&format!(
            "FlattenCellChilds := {};\n      ",
            bool_value(self.flatten_cell_childs)
        ));
        text.push_str(&format!(
            "AssignPersWrldChild := {};\n      ",
            bool_value(self.assign_pers_wrld_child)
        ));
        text.push_str(&format!(
            "InheritConflictByParent := {};\n      ",
            bool_value(self.inherit_conflict_by_parent)
        ));
        text.push_str(&format!("AddMessage('[filter] {}');\n      ", self.name));
        text.push_str("ApplyFilter;\n      ");
        text
    }

    /// The options of the port's `filter.apply` request, as the same fields.
    fn options(&self) -> Value {
        json!({
            "conflict_all": self.conflict_all,
            "conflict_this": self.conflict_this,
            "by_inject_status": self.by_inject_status,
            "by_not_reachable_status": self.by_not_reachable_status,
            "by_references_injected_status": self.by_references_injected_status,
            "by_editor_id": self.by_editor_id,
            "by_element_value": self.by_element_value,
            "by_name": self.by_name,
            "by_base_editor_id": self.by_base_editor_id,
            "by_base_name": self.by_base_name,
            "scaled_actors": self.scaled_actors,
            "by_signature": self.by_signature,
            "by_base_signature": self.by_base_signature,
            "by_persistent": self.by_persistent,
            "persistent": self.persistent,
            "unnecessary_persistent": self.unnecessary_persistent,
            "master_is_temporary": self.master_is_temporary,
            "is_master": self.is_master,
            "persistent_pos_changed": self.persistent_pos_changed,
            "deleted": self.deleted,
            "by_vwd": self.by_vwd,
            "by_has_vwd_mesh": self.by_has_vwd_mesh,
            "by_has_precombined_mesh": self.by_has_precombined_mesh,
            "regex_comparison": self.regex_comparison,
            "flatten_blocks": self.flatten_blocks,
            "flatten_cell_childs": self.flatten_cell_childs,
            "assign_pers_wrld_child": self.assign_pers_wrld_child,
            "inherit_conflict_by_parent": self.inherit_conflict_by_parent,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    #[allow(dead_code)]
    description: String,
    game: String,
    /// The plugins, masters first.
    load: Vec<String>,
    /// Build the reachable information before the filters
    /// (`mniNavBuildReachableClick`, which the script clicks).
    #[serde(default)]
    build_reachable: bool,
    configs: Vec<Config>,
}

/// What one configuration left, the oracle's or the port's.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
struct Counts {
    pass1: i64,
    pass2: i64,
    unfiltered: i64,
    /// `(file, filtered, records)` per partly filtered file, in load order.
    files: Vec<(String, i64, i64)>,
}

#[derive(Serialize)]
struct FilterOutcome {
    scenario: String,
    game: &'static str,
    /// `equal`, `different`, `oracle-failed`, `port-failed` or `oracle-only`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// The configurations compared.
    configs: usize,
    /// Configurations whose counts differ.
    different: usize,
    /// Nodes the first passes walked, by the oracle.
    pass1: i64,
    oracle_seconds: Option<f64>,
    port_seconds: Option<f64>,
}

#[derive(Serialize)]
struct FilterReport<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    outcomes: &'a [FilterOutcome],
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

/// A number after `label` in `line`.
fn number_after(line: &str, label: &str) -> Option<i64> {
    let rest = &line[line.find(label)? + label.len()..];
    let digits: String = rest.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The base name of a path as the log writes it.
fn base_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_owned()
}

/// The counts of every configuration, in order, from the message log of the
/// GUI (or of the port, which writes the same lines).
fn counts_of(log: &str, names: &[String]) -> Vec<Counts> {
    let mut result: Vec<Counts> = names.iter().map(|_| Counts::default()).collect();
    let mut current: Option<usize> = None;
    for line in log.lines() {
        let line = strip_time(line.trim_end());
        // `%.0n` writes the counts with the thousands separator of the
        // machine's format settings (`1,234`), which the port writes too; a
        // count without one is not changed.
        let line = &numbers_without_separators(line);
        if let Some(name) = line.strip_prefix("[filter] ") {
            let name = name.trim();
            current = names.iter().position(|wanted| wanted == name);
            if let Some(index) = current {
                result[index] = Counts::default();
            }
            continue;
        }
        let Some(index) = current else { continue };
        if let Some(rest) = line.strip_prefix('[')
            && let Some((path, tail)) = rest.split_once("] Filtered ")
            && let Some(filtered) = number_after(tail, "")
            && let Some(records) = number_after(&tail[tail.find(" of ").unwrap_or(0)..], "of ")
        {
            result[index].files.push((base_name(path), filtered, records));
            continue;
        }
        if line.starts_with("Done: Applying Filter") {
            result[index].pass1 = number_after(line, "[Pass 1] Processed Records:").unwrap_or(0);
            result[index].pass2 = number_after(line, "[Pass 2] Processed Records:").unwrap_or(0);
            result[index].unfiltered = number_after(line, "Remaining unfiltered nodes:").unwrap_or(0);
        }
    }
    result
}

/// The named numbers `Filtered n of m` of one line, without the separator
/// the GUI writes with its format settings (`1,234`).
fn numbers_without_separators(text: &str) -> String {
    text.replace(',', "")
}

pub(super) fn run_filter(
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
    let dir = root.join("crates/xtask/oracle/filters");
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
        let game = super::GAMES
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
            Err(error) => FilterOutcome {
                scenario: stem.clone(),
                game: game.name,
                status: "oracle-failed",
                detail: Some(format!("  {error:#}")),
                configs: scenario.configs.len(),
                different: 0,
                pass1: 0,
                oracle_seconds: None,
                port_seconds: None,
            },
        };
        println!(
            "{:13} {} ({}, {} configs, {} different, {} nodes in pass 1)",
            outcome.status, outcome.scenario, outcome.game, outcome.configs, outcome.different, outcome.pass1
        );
        if let Some(detail) = &outcome.detail {
            println!("{detail}");
        }
        outcomes.push(outcome);
    }
    let equal = outcomes.iter().filter(|o| o.status == "equal").count();
    let report = FilterReport {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("filter.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    println!(
        "{equal} of {} scenarios equal. Report: {}",
        outcomes.len(),
        report_file.display()
    );
    if !options.oracle_only {
        let failed = outcomes.iter().filter(|o| o.status != "equal").count();
        ensure!(failed == 0, "parity does not hold");
    }
    Ok(())
}

/// The script of a scenario: the reachable build and the configurations.
fn script_of(scenario: &Scenario) -> String {
    let mut actions = String::new();
    if scenario.build_reachable {
        actions.push_str(
            "AddMessage('[filter] build reachable');\n      frmMain.FindComponent('mniNavBuildReachable').Click;\n      ",
        );
    }
    for config in &scenario.configs {
        actions.push_str(&config.pascal());
    }
    FILTER_SCRIPT.replace("\r\n", "\n").replace("{{ACTIONS}}", &actions)
}

fn check(
    runner: &Runner,
    game: &'static Game,
    data: &Path,
    stem: &str,
    scenario: &Scenario,
    scenario_text: &str,
) -> Result<FilterOutcome> {
    let mut outcome = FilterOutcome {
        scenario: stem.to_owned(),
        game: game.name,
        status: "oracle-only",
        detail: None,
        configs: scenario.configs.len(),
        different: 0,
        pass1: 0,
        oracle_seconds: None,
        port_seconds: None,
    };
    let names: Vec<&str> = scenario.load.iter().map(String::as_str).collect();
    let plugins = load_list(game, data, &names)?;
    let script = script_of(scenario);
    let key = oracle_key(
        runner,
        &plugins,
        &format!("{ORACLE_RECIPE}\n{script}\n{}", text_hash(scenario_text)),
        gui::exe_name(game.mode),
    )?;
    let cache_dir = runner.cache.join(format!("{}-oracle-filter", game.mode));
    fs::create_dir_all(&cache_dir)?;
    let file_stem = format!("{stem}.{key:016x}");
    let log_file = cache_dir.join(format!("{file_stem}.log.zst"));
    if !log_file.exists() {
        let exe = runner.oracle_dir.join(gui::exe_name(game.mode));
        ensure!(exe.exists(), "{} does not exist", exe.display());
        let work =
            runner
                .scratch
                .join("oracle-work")
                .join(format!("{}-filter-{stem}.{}", game.mode, std::process::id()));
        let peak_file = cache_dir.join(format!("{file_stem}.oracle.peak"));
        let expected_peak = fs::read_to_string(&peak_file)
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(4 * GIB);
        let run = GuiRun {
            exe: &exe,
            mode: game.mode,
            star_plugins_txt: !gui::simple_plugins_txt(game.mode),
            plugins: plugins.clone(),
            script,
            // The script mode builds the reference information on load
            // unless `-nobuildrefs`, as the edit mode does: the filters that
            // read the references (the unnecessary-persistent and the
            // references-injected ones) need it. A scenario that builds the
            // reachable information clicks `mniNavBuildReachable`, which
            // builds the references itself, so the load skips them there.
            build_refs: !scenario.build_reachable,
            work: work.clone(),
            timeout: runner.oracle_timeout.unwrap_or(Duration::from_secs(120 * 60)),
            hang_timeout: Duration::from_secs(600),
            budget: &runner.budget,
            expected_peak,
            max_memory: runner.max_memory,
        };
        let started = std::time::Instant::now();
        let result = run.run_with(&Default::default());
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                gui::remove_work(&work);
                return Err(error);
            }
        };
        outcome.oracle_seconds = Some(started.elapsed().as_secs_f64());
        let log_text = work.join("log.txt");
        fs::write(&log_text, &result.log)?;
        super::oracle_save::keep_compressed(&log_text, &log_file)?;
        fs::write(
            cache_dir.join(format!("{file_stem}.oracle.seconds")),
            format!("{:.1}", started.elapsed().as_secs_f64()),
        )?;
        gui::remove_work(&work);
    } else {
        outcome.oracle_seconds = fs::read_to_string(cache_dir.join(format!("{file_stem}.oracle.seconds")))
            .ok()
            .and_then(|text| text.trim().parse().ok());
    }
    let oracle_log = super::oracle_save::zstd_reader(&log_file).and_then(|mut reader| {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut reader, &mut text)?;
        Ok(text)
    })?;
    let wants: Vec<String> = scenario.configs.iter().map(|config| config.name.clone()).collect();
    let oracle_counts = counts_of(&oracle_log, &wants);
    let found = oracle_log.matches("[filter] ").count();
    ensure!(
        found >= scenario.configs.len(),
        "the oracle ran {} of {} filters; the end of the log:\n{}",
        found - usize::from(scenario.build_reachable),
        scenario.configs.len(),
        oracle_log.lines().rev().take(15).collect::<Vec<_>>().join("\n")
    );
    ensure!(
        oracle_counts.iter().any(|counts| counts.pass1 > 0),
        "the oracle logged no filter of the scenario; the end of the log:\n{}",
        oracle_log.lines().rev().take(15).collect::<Vec<_>>().join("\n")
    );
    outcome.pass1 = oracle_counts.iter().map(|counts| counts.pass1).sum();
    let Some(port) = &runner.port else {
        return Ok(outcome);
    };

    // The port runs the same configurations as one batch, from a folder of
    // links to the plugins only, as the GUI's private data folder: no
    // archives, no strings files (an lstring then shows
    // `<Error: No strings file for lstring ID ...>` on both sides).
    let port_dir = runner.scratch.join(format!("{}-filter", game.mode));
    fs::create_dir_all(&port_dir)?;
    let private = private_data(&port_dir.join(format!("work-{stem}")), &plugins)?;
    let mut commands: Vec<Value> = Vec::new();
    if scenario.build_reachable {
        commands.push(json!({ "command": "refs.build_reachable", "params": { "build_refs": true } }));
    }
    for config in &scenario.configs {
        commands.push(json!({
            "command": "filter.apply",
            "params": { "options": config.options(), "list_records": false },
        }));
    }
    let batch_file = port_dir.join(format!("{file_stem}.batch.json"));
    fs::write(&batch_file, serde_json::to_string(&commands)?)?;
    let port_log = port_dir.join(format!("{file_stem}.port.log"));
    let mut command = std::process::Command::new(port);
    command
        .args(["--json", "--edit", "--game", game.mode, "--dont-cache"])
        .arg("--load");
    let loaded: Vec<PathBuf> = plugins
        .iter()
        .map(|plugin| private.join(plugin.file_name().unwrap_or_else(|| std::ffi::OsStr::new("plugin"))))
        .collect();
    command.arg(&loaded[0]);
    for plugin in &loaded[1..] {
        command.arg("--load").arg(plugin);
    }
    let started = std::time::Instant::now();
    let output = command
        .arg("batch")
        .arg(&batch_file)
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
    let results = envelope["result"].as_array().cloned().unwrap_or_default();
    let mut port_counts: Vec<Counts> = Vec::new();
    for result in &results {
        if result["command"] == Value::String("refs.build_reachable".to_owned()) {
            if result["ok"] != Value::Bool(true) {
                outcome.status = "port-failed";
                outcome.detail = Some(format!("  refs.build_reachable: {}", result["error"]));
                return Ok(outcome);
            }
            continue;
        }
        if result["ok"] != Value::Bool(true) {
            outcome.status = "port-failed";
            outcome.detail = Some(format!("  filter.apply: {}", result["error"]));
            return Ok(outcome);
        }
        let result = &result["result"];
        let mut counts = Counts {
            pass1: result["pass1"].as_i64().unwrap_or(0),
            pass2: result["pass2"].as_i64().unwrap_or(0),
            unfiltered: result["unfiltered"].as_i64().unwrap_or(0),
            files: Vec::new(),
        };
        for file in result["files"].as_array().into_iter().flatten() {
            if file["logged"] != Value::Bool(true) {
                continue;
            }
            counts.files.push((
                file["name"].as_str().unwrap_or_default().to_owned(),
                file["filtered"].as_i64().unwrap_or(0),
                file["records"].as_i64().unwrap_or(0),
            ));
        }
        port_counts.push(counts);
    }
    ensure!(
        port_counts.len() == oracle_counts.len(),
        "the port ran {} of the {} filters",
        port_counts.len(),
        oracle_counts.len()
    );

    let mut lines = Vec::new();
    let mut different = 0;
    for (index, (oracle, port)) in oracle_counts.iter().zip(&port_counts).enumerate() {
        if oracle == port {
            continue;
        }
        different += 1;
        if lines.len() < LISTED_DIFFERENCES {
            let name = &scenario.configs[index].name;
            lines.push(format!(
                "  {name}: oracle ({} / {} / {}) {}\n         port   ({} / {} / {}) {}",
                oracle.pass1,
                oracle.pass2,
                oracle.unfiltered,
                describe_files(&oracle.files),
                port.pass1,
                port.pass2,
                port.unfiltered,
                describe_files(&port.files)
            ));
        }
    }
    outcome.different = different;
    if different > LISTED_DIFFERENCES {
        lines.push(format!(
            "  ... {} more configurations differ",
            different - LISTED_DIFFERENCES
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

/// A data folder with only `plugins` in it, as hard links where the file
/// system allows them (the private data folder of the oracle run).
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

/// The files of a configuration for a report.
fn describe_files(files: &[(String, i64, i64)]) -> String {
    if files.is_empty() {
        "no filtered file line".to_owned()
    } else {
        files
            .iter()
            .map(|(name, filtered, records)| format!("{name} {filtered}/{records}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_counts_of_a_log() {
        let log = "[00:01] [filter] cleaning
[00:02] [D:\\Data\\Skyrim.esm] Filtered 1,234 of 9,999 records
[00:09] Done: Applying Filter, [Pass 1] Processed Records: 19264, [Pass 2] Processed Records: 18541, Remaining unfiltered nodes: 18541, Elapsed Time: 00:09
[00:10] [filter] conflicts
[00:11] Done: Applying Filter, [Pass 1] Processed Records: 21, [Pass 2] Processed Records: 20, Remaining unfiltered nodes: 19, Elapsed Time: 00:01";
        let counts = counts_of(log, &["cleaning".to_owned(), "conflicts".to_owned()]);
        assert_eq!(counts.len(), 2);
        assert_eq!(counts[0].pass1, 19264);
        assert_eq!(counts[0].pass2, 18541);
        assert_eq!(counts[0].unfiltered, 18541);
        assert_eq!(counts[0].files, vec![("Skyrim.esm".to_owned(), 1234, 9999)]);
        assert_eq!(counts[1].pass1, 21);
        assert_eq!(counts[1].files.len(), 0);
    }

    #[test]
    fn writes_every_variable_of_a_configuration() {
        let config: Config = serde_json::from_value(json!({"name": "one", "flatten_blocks": true})).unwrap();
        let pascal = config.pascal();
        assert!(pascal.contains("FilterConflictAll := False;"));
        assert!(pascal.contains("FilterConflictAllSet := [];"));
        assert!(pascal.contains("FlattenBlocks := True;"));
        assert!(pascal.contains("InheritConflictByParent := True;"));
        assert!(pascal.contains("AddMessage('[filter] one');"));
        assert!(pascal.ends_with("ApplyFilter;\n      "));
    }
}
