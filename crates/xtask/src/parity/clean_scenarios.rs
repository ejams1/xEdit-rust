// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity clean-scenarios`: the cleaning menu items of the
//! main form over a selection of plugins, with the options of the
//! "Cleaning" tab and the switches the quick auto clean mode of `parity
//! clean` can not reach.
//!
//! A scenario (`crates/xtask/oracle/cleaning/*.json`) names a game, the
//! plugins to load, the plugins to select (one or several file nodes), the
//! steps ("Undelete and Disable References", "Remove Identical to Master
//! records"), the UDR options (as the request fields of `files.clean`,
//! which the oracle reads from the `[Options]` section of its settings
//! file), extra switches (`-AllowMakePartial`) and the plugins to compare.
//! The GUI build of xEdit runs `crates/xtask/oracle/cleaning.pas` in its
//! script mode: the filter for cleaning, the selection (a click and
//! Ctrl+clicks on the file nodes), the clicks of the menu items and of
//! "LOOT Masterlist Entries" (`mniNavLOManagersDirtyInfoClick`), then the
//! plugins written out. The port runs `files.clean` with the same
//! selection and options, `files.dirty_info` and `files.save` as one
//! `xedit batch`. Compared: per step the closing count line and the record
//! lines of the log, the dirty information report and the saved bytes of
//! every plugin the scenario names. The oracle's results are cached in
//! `<cache>/<tag>/<MODE>-oracle-clean-scenarios/`; the key hashes the
//! plugins, the generated script and the scenario.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::gui::{self, GuiRun};
use super::oracle_save::{keep_compressed, load_list, oracle_key, text_hash, zstd_reader};
use super::{GIB, Game, Options, Runner};

/// The oracle script.
const SCRIPT: &str = include_str!("../../oracle/cleaning.pas");

/// Part of the cache key: changes to what the oracle run does go here.
const ORACLE_RECIPE: &str = "clean-scenarios v1";

/// How many differing lines an outcome lists.
const LISTED_DIFFERENCES: usize = 12;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    #[allow(dead_code)]
    description: String,
    game: String,
    /// The plugins, masters first.
    load: Vec<String>,
    /// The file nodes the clicks select, in any order (the handlers walk
    /// them in the order of the tree).
    select: Vec<String>,
    #[serde(default)]
    udr: bool,
    #[serde(default)]
    itm: bool,
    /// Fields of the `files.clean` request: the UDR options (`udr_xesp`,
    /// `udr_scale`, `udr_scale_value`, `udr_z`, `udr_z_value`, `udr_mstt`,
    /// `udr_mstt_value`) and `allow_make_partial`.
    #[serde(default)]
    options: Map<String, Value>,
    /// The plugins whose bytes are compared.
    #[serde(default)]
    save: Vec<String>,
    /// A plugin the port builds first over the loaded masters, which both
    /// sides then load after them: a case the vanilla plugins do not have
    /// (a deleted reference to a moveable static for `UDRSetMSTT`).
    #[serde(default)]
    prepare: Option<Prepare>,
    /// Further `[Options]` lines of the oracle's settings file (`FillPNAM`,
    /// `SimpleRecords`); the port gets the flag of each it has
    /// (`--fill-pnam`), and a scenario with one it has not runs on the
    /// oracle only.
    #[serde(default)]
    ini: Map<String, Value>,
}

/// The input plugin of a scenario that the port builds.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepare {
    /// The plugin's name; `commands` create it (`files.new`).
    file: String,
    /// The commands, as `xedit batch` takes them; the plugin is saved
    /// after them.
    commands: Vec<Value>,
}

impl Scenario {
    /// The `[Options]` lines of the oracle's settings file for the UDR
    /// options (`xeMainForm.pas` reads them in `FormShow`).
    fn settings_lines(&self) -> Result<String> {
        let mut lines = String::new();
        for (field, key) in [
            ("udr_xesp", "UDRSetXESP"),
            ("udr_scale", "UDRSetScale"),
            ("udr_z", "UDRSetZ"),
            ("udr_mstt", "UDRSetMSTT"),
        ] {
            if let Some(value) = self.options.get(field) {
                let value = value.as_bool().with_context(|| format!("{field} must be a boolean"))?;
                lines.push_str(&format!("{key}={}\r\n", u8::from(value)));
            }
        }
        for (field, key) in [("udr_scale_value", "UDRSetScaleValue"), ("udr_z_value", "UDRSetZValue")] {
            if let Some(value) = self.options.get(field) {
                let value = value.as_f64().with_context(|| format!("{field} must be a number"))?;
                lines.push_str(&format!("{key}={value}\r\n"));
            }
        }
        for (key, value) in &self.ini {
            let value = match value {
                Value::Bool(value) => u8::from(*value).to_string(),
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            lines.push_str(&format!(
                "{key}={value}
"
            ));
        }
        if let Some(value) = self.options.get("udr_mstt_value") {
            let text = value.as_str().context("udr_mstt_value must be hexadecimal text")?;
            let value = i64::from_str_radix(text, 16).context("udr_mstt_value must be hexadecimal text")?;
            lines.push_str(&format!("UDRSetMSTTValue={value}\r\n"));
        }
        Ok(lines)
    }

    fn switches(&self) -> Vec<String> {
        // `EditWarn` asks once per session unless `wbIKnowWhatImDoing`.
        let mut switches = vec!["-IKnowWhatImDoing".to_owned()];
        if self.options.get("allow_make_partial").and_then(Value::as_bool) == Some(true) {
            switches.push("-AllowMakePartial".to_owned());
        }
        switches
    }
}

#[derive(Serialize)]
struct ScenarioOutcome {
    scenario: String,
    game: &'static str,
    /// `equal`, `different`, `oracle-failed`, `port-failed` or
    /// `oracle-only`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// The oracle's counts: `udr/itm/nav`.
    counts: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    oracle_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_seconds: Option<f64>,
}

#[derive(Serialize)]
struct ScenarioReport<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    outcomes: &'a [ScenarioOutcome],
}

/// What the steps of one side logged.
#[derive(Debug, Default, PartialEq, Eq)]
struct Steps {
    /// The closing lines of the steps (`[Undeleting and Disabling
    /// References done]`, `[Removing "Identical to Master" records done]`)
    /// without the elapsed time, and the warnings after them.
    totals: Vec<String>,
    /// The record lines (`Undeleting:`, `Skipping:`, `Removing:`, `Can't
    /// remove:`, `Making Partial Form:`) in order.
    lines: Vec<String>,
    /// The dirty information report.
    dirty: Vec<String>,
}

fn strip_time(line: &str) -> &str {
    let bytes = line.as_bytes();
    if bytes.len() > 8 && bytes[0] == b'[' && bytes[3] == b':' && bytes[6] == b']' && bytes[7] == b' ' {
        &line[8..]
    } else {
        line
    }
}

/// A closing line without its `, Elapsed Time: ...`.
fn without_elapsed(line: &str) -> &str {
    line.find(", Elapsed Time:").map_or(line, |end| &line[..end])
}

fn steps_of<'a>(lines: impl Iterator<Item = &'a str>) -> Steps {
    let mut steps = Steps::default();
    let mut dirty = false;
    for line in lines {
        let raw = line.trim_end_matches(['\r', '\n']);
        if dirty {
            if strip_time(raw).len() != raw.len() || raw.starts_with("[cleaning] ") {
                dirty = false;
            } else {
                steps.dirty.push(raw.to_owned());
                continue;
            }
        }
        let line = strip_time(raw);
        if line == "LOOT Masterlist Entries" {
            dirty = true;
            steps.dirty.push(line.to_owned());
        } else if line.starts_with("[Undeleting and Disabling References done]")
            || line.starts_with("[Removing \"Identical to Master\" records done]")
            || line.starts_with("<Warning: Plugin contains")
        {
            steps.totals.push(without_elapsed(line).to_owned());
        } else if [
            "Undeleting: ",
            "Skipping: ",
            "Removing: ",
            "Can't remove: ",
            "Making Partial Form: ",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
        {
            steps.lines.push(line.to_owned());
        }
    }
    while steps.dirty.last().is_some_and(|line| line.trim().is_empty()) {
        steps.dirty.pop();
    }
    steps
}

/// The counts of a log for the report: `udr/itm/nav`.
fn counts_of(steps: &Steps) -> String {
    let number = |label: &str, prefix: &str| {
        steps
            .totals
            .iter()
            .find(|line| line.starts_with(prefix))
            .and_then(|line| {
                let rest = &line[line.find(label)? + label.len()..];
                rest.trim_start()
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse::<u64>()
                    .ok()
            })
            .unwrap_or(0)
    };
    format!(
        "{}/{}/{}",
        number("Undeleted Records:", "[Undeleting"),
        number("Removed Records:", "[Removing"),
        number("Plugin contains", "<Warning: Plugin contains")
    )
}

/// The actions of the oracle script.
fn actions_of(scenario: &Scenario) -> String {
    let mut actions = super::filter::cleaning_filter_pascal("cleaning");
    actions.push_str("CollapseFiles;\n      ");
    for (index, name) in scenario.select.iter().enumerate() {
        actions.push_str(&format!(
            "ClickRow({}, {});\n      ",
            pascal_string(name),
            if index == 0 { "False" } else { "True" }
        ));
    }
    if scenario.udr {
        actions.push_str(
            "AddMessage('[cleaning] udr');\n      frmMain.FindComponent('mniNavUndeleteAndDisableReferences').Click;\n      ",
        );
    }
    if scenario.itm {
        actions.push_str(
            "AddMessage('[cleaning] itm');\n      frmMain.FindComponent('mniNavRemoveIdenticalToMaster').Click;\n      ",
        );
    }
    actions.push_str(
        "AddMessage('[cleaning] dirty');\n      frmMain.FindComponent('mniNavLOManagersDirtyInfo').Click;\n      ",
    );
    for name in &scenario.save {
        let literal = pascal_string(name);
        actions.push_str(&format!(
            "fs := TFileStream.Create('{{{{WORK}}}}out\\' + {literal}, fmCreate);\n      try\n        FileWriteToStream(FileNamed({literal}), fs, 0);\n      finally\n        fs.Free;\n      end;\n      "
        ));
    }
    actions.push_str("AddMessage('[cleaning] end');\n      ");
    SCRIPT.replace("\r\n", "\n").replace("{{ACTIONS}}", &actions)
}

fn pascal_string(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn read_zst(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    zstd_reader(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn run_clean_scenarios(
    root: &Path,
    tag: &str,
    options: &Options,
    cache: PathBuf,
    scratch: PathBuf,
    oracle_dir: PathBuf,
) -> Result<()> {
    // The port builds the prepared plugins even for an oracle-only run.
    let port = Some(super::build_port(root)?);
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
    let dir = root.join("crates/xtask/oracle/cleaning");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    let mut outcomes = Vec::new();
    for path in paths {
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
        let outcome =
            check(&runner, game, &data, &stem, &scenario, &text, options.oracle_only).unwrap_or_else(|error| {
                ScenarioOutcome {
                    scenario: stem.clone(),
                    game: game.name,
                    status: "oracle-failed",
                    detail: Some(format!("  {error:#}")),
                    counts: String::new(),
                    oracle_seconds: None,
                    port_seconds: None,
                }
            });
        println!(
            "{:13} {} ({}, udr/itm/nav {})",
            outcome.status, outcome.scenario, outcome.game, outcome.counts
        );
        if let Some(detail) = &outcome.detail {
            println!("{detail}");
        }
        outcomes.push(outcome);
    }
    let equal = outcomes.iter().filter(|o| o.status == "equal").count();
    let report = ScenarioReport {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("clean-scenarios.json");
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

fn check(
    runner: &Runner,
    game: &'static Game,
    data: &Path,
    stem: &str,
    scenario: &Scenario,
    scenario_text: &str,
    oracle_only: bool,
) -> Result<ScenarioOutcome> {
    let mut outcome = ScenarioOutcome {
        scenario: stem.to_owned(),
        game: game.name,
        status: "oracle-only",
        detail: None,
        counts: String::new(),
        oracle_seconds: None,
        port_seconds: None,
    };
    let names: Vec<&str> = scenario.load.iter().map(String::as_str).collect();
    let mut plugins = load_list(game, data, &names)?;
    if let Some(prepare) = &scenario.prepare {
        plugins.push(prepare_plugin(runner, game, stem, &plugins, prepare)?);
    }
    let script = actions_of(scenario);
    let settings = scenario.settings_lines()?;
    let key = oracle_key(
        runner,
        &plugins,
        &format!("{ORACLE_RECIPE}\n{script}\n{settings}\n{}", text_hash(scenario_text)),
        gui::exe_name(game.mode),
    )?;
    let cache_dir = runner.cache.join(format!("{}-oracle-clean-scenarios", game.mode));
    fs::create_dir_all(&cache_dir)?;
    let file_stem = format!("{stem}.{key:016x}");
    let log_file = cache_dir.join(format!("{file_stem}.log.zst"));
    if !log_file.exists() {
        let exe = runner.oracle_dir.join(gui::exe_name(game.mode));
        ensure!(exe.exists(), "{} does not exist", exe.display());
        let work = runner.scratch.join("oracle-work").join(format!(
            "{}-clean-scenarios-{stem}.{}",
            game.mode,
            std::process::id()
        ));
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
            // The cleaning reads no reference information.
            build_refs: false,
            work: work.clone(),
            timeout: runner.oracle_timeout.unwrap_or(Duration::from_secs(120 * 60)),
            hang_timeout: Duration::from_secs(600),
            budget: &runner.budget,
            expected_peak,
            max_memory: runner.max_memory,
            extra_args: Vec::new(),
            discovery: false,
            tool_name: "Edit".to_owned(),
        };
        // The settings file next to the plugin list (`-P:`), as
        // `GuiRun::write_viewsettings` writes it for the auto modes, with
        // the UDR options of the scenario.
        let today = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs() / 86_400)
            + 25_569;
        let extras = gui::GuiExtras {
            files: vec![(
                PathBuf::from(format!("plugins.{}viewsettings", game.mode.to_ascii_lowercase())),
                format!(
                    "[Init]\r\nFirst64Start=0\r\n\r\n[DeveloperMessage]\r\nLastShownOn={today}\r\nVersion=0\r\n\r\n[Options]\r\nPatron=1\r\nShowTip=0\r\n{settings}"
                )
                .into_bytes(),
            )],
            args: scenario.switches(),
            ..Default::default()
        };
        let started = std::time::Instant::now();
        let result = match run.run_with(&extras) {
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
        if result.status.last().map(String::as_str) != Some("done") {
            let status = result.status.join("\n");
            let tail = result.log.lines().rev().take(15).collect::<Vec<_>>().join("\n");
            gui::remove_work(&work);
            anyhow::bail!("the oracle script stopped: {status}; the end of the log:\n{tail}");
        }
        for name in &scenario.save {
            keep_compressed(
                &result.out.join(name),
                &cache_dir.join(format!("{file_stem}.{name}.saved.zst")),
            )?;
        }
        let log_text = work.join("log.txt");
        fs::write(&log_text, &result.log)?;
        keep_compressed(&log_text, &log_file)?;
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
    let oracle_log = String::from_utf8_lossy(&read_zst(&log_file)?).into_owned();
    ensure!(
        oracle_log.contains("[cleaning] end"),
        "the oracle's log does not reach the end of the script; its end:\n{}",
        oracle_log.lines().rev().take(15).collect::<Vec<_>>().join("\n")
    );
    let oracle = steps_of(oracle_log.lines());
    outcome.counts = counts_of(&oracle);
    let Some(port) = runner.port.as_ref().filter(|_| !oracle_only) else {
        return Ok(outcome);
    };

    let port_dir = runner.scratch.join(format!("{}-clean-scenarios", game.mode));
    fs::create_dir_all(&port_dir)?;
    let private = private_data(&port_dir.join(format!("work-{stem}")), &plugins)?;
    let out_dir = port_dir.join(format!("{file_stem}.port"));
    if out_dir.exists() {
        fs::remove_dir_all(&out_dir)?;
    }
    fs::create_dir_all(&out_dir)?;
    let mut params = scenario.options.clone();
    params.insert("files".to_owned(), json!(scenario.select));
    params.insert("udr".to_owned(), json!(scenario.udr));
    params.insert("itm".to_owned(), json!(scenario.itm));
    let mut commands = vec![
        json!({ "command": "files.clean", "params": Value::Object(params) }),
        json!({ "command": "files.dirty_info", "params": {} }),
    ];
    for name in &scenario.save {
        commands.push(json!({
            "command": "files.save",
            "params": { "file": name, "output": out_dir.join(name), "backup": false },
        }));
    }
    let batch_file = port_dir.join(format!("{file_stem}.batch.json"));
    fs::write(&batch_file, serde_json::to_string(&commands)?)?;
    let port_log = port_dir.join(format!("{file_stem}.port.log"));
    let mut command = std::process::Command::new(port);
    command.args(["--json", "--edit", "--game", game.mode, "--dont-cache"]);
    for (key, value) in &scenario.ini {
        let on = value.as_bool().unwrap_or_else(|| value.as_str() == Some("1"));
        match key.as_str() {
            "FillPNAM" if on => {
                command.arg("--fill-pnam");
            }
            "FillPNAM" => {}
            other => anyhow::bail!("the port has no load setting for the oracle's {other}"),
        }
    }
    for plugin in &plugins {
        command
            .arg("--load")
            .arg(private.join(plugin.file_name().context("plugin without a name")?));
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
    let results = envelope["result"].as_array().cloned().unwrap_or_default();
    if envelope["ok"] != Value::Bool(true) || results.iter().any(|result| result["ok"] != Value::Bool(true)) {
        outcome.status = "port-failed";
        outcome.detail = Some(format!(
            "  {}, see {}",
            String::from_utf8_lossy(&output.stdout)
                .chars()
                .take(2000)
                .collect::<String>(),
            port_log.display()
        ));
        return Ok(outcome);
    }
    let mut messages: Vec<String> = Vec::new();
    for result in &results {
        for message in result["result"]["messages"].as_array().into_iter().flatten() {
            if let Some(message) = message.as_str() {
                messages.extend(message.split("\r\n").map(str::to_owned));
            }
        }
    }
    let port_steps = steps_of(messages.iter().map(String::as_str));

    let mut lines = Vec::new();
    if oracle.totals != port_steps.totals {
        lines.push(format!(
            "  totals: oracle {:?}\n          port   {:?}",
            oracle.totals, port_steps.totals
        ));
    }
    if oracle.lines != port_steps.lines {
        let mut shown = 0;
        for line in oracle.lines.iter().filter(|line| !port_steps.lines.contains(line)) {
            if shown < LISTED_DIFFERENCES {
                lines.push(format!("  only the oracle: {line}"));
            }
            shown += 1;
        }
        for line in port_steps.lines.iter().filter(|line| !oracle.lines.contains(line)) {
            if shown < LISTED_DIFFERENCES {
                lines.push(format!("  only the port: {line}"));
            }
            shown += 1;
        }
        if shown == 0 {
            lines.push("  the same record lines in another order".to_owned());
        }
    }
    if oracle.dirty != port_steps.dirty {
        let first = oracle
            .dirty
            .iter()
            .zip(&port_steps.dirty)
            .position(|(a, b)| a != b)
            .unwrap_or(oracle.dirty.len().min(port_steps.dirty.len()));
        lines.push(format!(
            "  dirty report line {}: oracle {:?}, port {:?}",
            first + 1,
            oracle.dirty.get(first),
            port_steps.dirty.get(first)
        ));
    }
    for name in &scenario.save {
        let oracle_bytes = read_zst(&cache_dir.join(format!("{file_stem}.{name}.saved.zst")))?;
        let port_file = out_dir.join(name);
        let port_bytes = fs::read(&port_file).with_context(|| format!("reading {}", port_file.display()))?;
        if oracle_bytes != port_bytes {
            let offset = oracle_bytes
                .iter()
                .zip(&port_bytes)
                .position(|(a, b)| a != b)
                .unwrap_or(oracle_bytes.len().min(port_bytes.len()));
            let oracle_copy = port_dir.join(format!("{file_stem}.{name}.oracle.saved"));
            fs::write(&oracle_copy, &oracle_bytes)?;
            lines.push(format!(
                "  {name}: saved bytes differ at {offset:#x} (oracle {} bytes, port {} bytes): {} {}",
                oracle_bytes.len(),
                port_bytes.len(),
                oracle_copy.display(),
                port_file.display()
            ));
        }
    }
    if lines.is_empty() {
        outcome.status = "equal";
    } else {
        outcome.status = "different";
        outcome.detail = Some(lines.join("\n"));
    }
    Ok(outcome)
}

/// Builds the scenario's prepared plugin with the port: the masters loaded
/// from a private data folder, the commands, and the save.
fn prepare_plugin(
    runner: &Runner,
    game: &'static Game,
    stem: &str,
    masters: &[PathBuf],
    prepare: &Prepare,
) -> Result<PathBuf> {
    let port = runner
        .port
        .as_ref()
        .context("no port to build the prepared plugin with")?;
    let dir = runner
        .scratch
        .join(format!("{}-clean-scenarios", game.mode))
        .join(format!("prepare-{stem}"));
    let private = private_data(&dir, masters)?;
    let out = dir.join("out");
    fs::create_dir_all(&out)?;
    let target = out.join(&prepare.file);
    let _ = fs::remove_file(&target);
    let mut commands = prepare.commands.clone();
    commands.push(json!({
        "command": "files.save",
        "params": { "file": prepare.file, "output": target, "backup": false },
    }));
    let batch = dir.join("prepare.json");
    fs::write(&batch, serde_json::to_string(&commands)?)?;
    let mut command = std::process::Command::new(port);
    command.args(["--json", "--edit", "--game", game.mode, "--dont-cache"]);
    for master in masters {
        command
            .arg("--load")
            .arg(private.join(master.file_name().context("plugin without a name")?));
    }
    let output = command
        .arg("batch")
        .arg(&batch)
        .stdin(std::process::Stdio::null())
        .stderr(File::create(dir.join("prepare.log"))?)
        .output()?;
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    let all_ok = envelope["ok"] == Value::Bool(true)
        && envelope["result"]
            .as_array()
            .is_some_and(|results| results.iter().all(|result| result["ok"] == Value::Bool(true)));
    ensure!(
        all_ok && target.is_file(),
        "the port could not build {}: {}",
        prepare.file,
        String::from_utf8_lossy(&output.stdout)
            .chars()
            .take(2000)
            .collect::<String>()
    );
    Ok(target)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_steps_of_a_log() {
        let log = "[00:01] [cleaning] udr
Undeleting: [REFR:0005F2E6] (in X)
[Undeleting and Disabling References done]  Processed Records: 18540, Undeleted Records: 11, Elapsed Time: 00:00
<Warning: Plugin contains 5 deleted NavMeshes which can not be undeleted>
[00:02] [cleaning] itm
Making Partial Form: [CELL:00000DC0]
[Removing \"Identical to Master\" records done]  Processed Records: 18540, Removed Records: 255, Elapsed Time: 00:00
[00:03] [cleaning] dirty

LOOT Masterlist Entries

  - name: 'A.esm'
    dirty:

[00:04] [cleaning] end";
        let steps = steps_of(log.lines());
        assert_eq!(steps.lines.len(), 2);
        assert_eq!(steps.totals.len(), 3);
        assert_eq!(
            steps.totals[0],
            "[Undeleting and Disabling References done]  Processed Records: 18540, Undeleted Records: 11"
        );
        assert_eq!(steps.dirty.len(), 4);
        assert_eq!(counts_of(&steps), "11/255/5");
    }

    #[test]
    fn writes_the_udr_options_as_settings() {
        let scenario: Scenario = serde_json::from_value(json!({
            "description": "", "game": "fo4", "load": [], "select": [],
            "options": {"udr_xesp": false, "udr_z_value": -20000.0, "udr_mstt_value": "0000001B", "allow_make_partial": true}
        }))
        .unwrap();
        let lines = scenario.settings_lines().unwrap();
        assert!(lines.contains("UDRSetXESP=0\r\n"));
        assert!(lines.contains("UDRSetZValue=-20000\r\n"));
        assert!(lines.contains("UDRSetMSTTValue=27\r\n"));
        assert_eq!(scenario.switches(), vec!["-IKnowWhatImDoing", "-AllowMakePartial"]);
    }
}
