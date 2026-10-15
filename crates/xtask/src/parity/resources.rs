// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity resources`: the archives a load finds and what the
//! container handler then resolves (issue #32).
//!
//! Each scenario of `crates/xtask/oracle/resources/*.json` names a game, the
//! plugins to load (their masters load with them), the archives of the
//! game's data folder to link into the private data folder, synthetic loose
//! files, the game ini and custom ini, the command-line switches
//! (`-skipbsa`, `-forcebsa`) and the probes. The GUI oracle runs
//! `oracle/resources.pas` in its script mode with the probes filled in; the
//! port runs the same script through the same command line (`xedit -FO4
//! -D:... -P:... -M:... -I:... -script:...`), so both load in the script
//! tool mode with the same inis and plugin list. Compared: the probe's
//! output (the container list, each probed resource with the containers
//! that hold it and the CRC32 of the data that wins, the listings of a
//! container below a folder, localized values, the sort keys of file and
//! folder hashes) and the loader's resource lines (`Loading Resources.`,
//! `Version:`, `Skipped.`, `Can't find`, `Setting Resource Path.` and the
//! cache lines), with each side's data folder written `<data>\`.
//!
//! The oracle's outputs are cached in
//! `<cache>/<tag>/<MODE>-oracle-resources/<scenario>.<key>/`; the key
//! hashes the plugins, the archives (name, size and time), the script and
//! the scenario.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::gui::{self, GuiExtras};
use super::modgroups::Text;
use super::oracle_save::{load_list, oracle_key, run_gui_with};
use super::{GAMES, GIB, Game, Options, Runner};

/// The probe script.
const RESOURCES_SCRIPT: &str = include_str!("../../oracle/resources.pas");

/// A loose file of a scenario, written below the private data folder.
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct LooseFile {
    /// The path below the data folder.
    path: String,
    /// The text of the file (written as UTF-8).
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    #[allow(dead_code, reason = "documentation in the file")]
    description: String,
    game: String,
    /// The plugins to load, masters first; their masters load with them.
    load: Vec<String>,
    /// The archives of the game's data folder linked into the private one;
    /// a name ending in `*` takes every archive whose name starts with the
    /// rest.
    #[serde(default)]
    archives: Vec<String>,
    /// Loose files of the private data folder.
    #[serde(default)]
    files: Vec<LooseFile>,
    /// The game ini (`-I:`); empty when omitted.
    #[serde(default)]
    game_ini: Option<Text>,
    /// The custom ini (`<My Games>\<game>Custom.ini`); none when omitted.
    #[serde(default)]
    custom_ini: Option<Text>,
    /// Switches of both command lines (`-skipbsa`, `-forcebsa`).
    #[serde(default)]
    switches: Vec<String>,
    /// The probes: `{"containers": true}`, `{"resource": name}`,
    /// `{"listing": [index, folder, shown]}`, `{"value": [file, FormID,
    /// path]}`, `{"key": [file, FormID, path]}`.
    probes: Vec<Value>,
}

#[derive(Serialize)]
struct ResourcesOutcome {
    scenario: String,
    game: &'static str,
    /// `equal`, `different`, `oracle-failed` or `port-failed`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// Lines of the probe's output and of the loader compared.
    probe_lines: usize,
    loader_lines: usize,
}

#[derive(Serialize)]
struct ResourcesReport<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    outcomes: &'a [ResourcesOutcome],
}

/// The custom ini of a game in the My Games folder (`DoInitPath`).
fn custom_ini_name(game: &Game) -> &'static str {
    match game.mode {
        "FO3" | "FNV" => "FalloutCustom.ini",
        "FO4" => "Fallout4Custom.ini",
        "FO4VR" => "Fallout4VRCustom.ini",
        "FO76" => "Fallout76Custom.ini",
        "SF1" => "StarfieldCustom.ini",
        "TES4" => "OblivionCustom.ini",
        "TES3" => "MorrowindCustom.ini",
        _ => "SkyrimCustom.ini",
    }
}

/// A Pascal string literal.
fn pascal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The probe calls of a scenario, in place of `{{PROBES}}`.
fn probe_calls(probes: &[Value]) -> Result<String> {
    let mut calls = String::new();
    let text = |value: &Value| -> Result<String> {
        Ok(match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
    };
    for probe in probes {
        let object = probe.as_object().context("a probe is an object")?;
        ensure!(object.len() == 1, "a probe has one key: {probe}");
        let (kind, args) = object.iter().next().unwrap();
        let call = match kind.as_str() {
            "containers" => "ProbeContainers;".to_owned(),
            "resource" => format!("ProbeResource({});", pascal(&text(args)?)),
            "listing" => {
                let args = args.as_array().context("listing takes [index, folder, shown]")?;
                ensure!(args.len() == 3, "listing takes [index, folder, shown]");
                format!(
                    "ProbeListing({}, {}, {});",
                    args[0].as_u64().context("listing index")?,
                    pascal(&text(&args[1])?),
                    args[2].as_u64().context("listing count")?
                )
            }
            "value" | "key" => {
                let args = args.as_array().context("takes [file, FormID, path]")?;
                ensure!(args.len() == 3, "{kind} takes [file, FormID, path]");
                format!(
                    "{}({}, {}, {});",
                    if kind == "value" { "ProbeValue" } else { "ProbeKey" },
                    pascal(&text(&args[0])?),
                    pascal(&text(&args[1])?),
                    pascal(&text(&args[2])?)
                )
            }
            other => bail!("unknown probe {other}"),
        };
        calls.push_str("      ");
        calls.push_str(&call);
        calls.push('\n');
    }
    Ok(calls)
}

/// The archives of the game's data folder a scenario names.
fn scenario_archives(scenario: &Scenario, data: &Path) -> Result<Vec<PathBuf>> {
    archives_named(&scenario.archives, data)
}

/// The archives of a data folder that `patterns` name: a name, or the
/// start of names followed by `*`, ignoring case; each once, in the order
/// of the patterns and then of the names.
pub(super) fn archives_named(patterns: &[String], data: &Path) -> Result<Vec<PathBuf>> {
    let mut all: Vec<String> = fs::read_dir(data)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            lower.ends_with(".ba2") || lower.ends_with(".bsa")
        })
        .collect();
    all.sort_by_key(|name| name.to_ascii_lowercase());
    let mut out = Vec::new();
    for pattern in patterns {
        let lower = pattern.to_ascii_lowercase();
        let matched: Vec<&String> = match lower.strip_suffix('*') {
            Some(prefix) => all
                .iter()
                .filter(|name| name.to_ascii_lowercase().starts_with(prefix))
                .collect(),
            None => all.iter().filter(|name| name.to_ascii_lowercase() == lower).collect(),
        };
        ensure!(!matched.is_empty(), "no archive {pattern} in {}", data.display());
        for name in matched {
            let path = data.join(name);
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// The lines of a run's log that are the loader's resource lines, without
/// the GUI's `[mm:ss] Background Loader: ` and with the data folder as
/// `<data>\`.
fn loader_lines(log: &str, data: &Path) -> Vec<String> {
    let data_text = format!("{}\\", gui::windows_path(data));
    log.lines()
        .filter_map(|line| {
            let line = line.trim_matches(|c: char| c.is_whitespace() || c == '\0');
            let line = match line.find("] Background Loader: ") {
                Some(at) if line.starts_with('[') => &line[at + "] Background Loader: ".len()..],
                _ => line,
            };
            let resource = line.ends_with("] Loading Resources.")
                || line.contains("] Version: ")
                || line.ends_with("] Skipped.")
                || line.contains("] Could not be loaded. <Error: ")
                || line.starts_with("Warning: <Can't find ")
                || line.ends_with("] Setting Resource Path.")
                || line == "Start building resources cache..."
                || line == "...resources cache finished building";
            resource.then(|| replace_ignore_case(line, &data_text, "<data>\\"))
        })
        .collect()
}

/// `text` with every `from` replaced, ignoring ASCII case.
fn replace_ignore_case(text: &str, from: &str, to: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let needle = from.to_ascii_lowercase();
    let mut out = String::new();
    let mut rest = 0;
    while let Some(at) = lower[rest..].find(&needle) {
        out.push_str(&text[rest..rest + at]);
        out.push_str(to);
        rest += at + needle.len();
    }
    out.push_str(&text[rest..]);
    out
}

/// The probe's output with the data folder as `<data>\`.
fn probe_lines(path: &Path, data: &Path) -> Result<Vec<String>> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let data_text = format!("{}\\", gui::windows_path(data));
    Ok(text
        .lines()
        .map(|line| replace_ignore_case(line, &data_text, "<data>\\"))
        .collect())
}

/// What one side of a scenario gave.
struct SideOutput {
    status: String,
    probe: Vec<String>,
    loader: Vec<String>,
}

pub(super) fn run_resources(
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
    let dir = root.join("crates/xtask/oracle/resources");
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
        let outcome = check(&runner, game, &data, &stem, &scenario, &text).unwrap_or_else(|error| ResourcesOutcome {
            scenario: stem.clone(),
            game: game.name,
            status: "oracle-failed",
            detail: Some(format!("  {error:#}")),
            probe_lines: 0,
            loader_lines: 0,
        });
        println!(
            "{:13} {} ({}, {} probe lines, {} loader lines)",
            outcome.status, outcome.scenario, outcome.game, outcome.probe_lines, outcome.loader_lines
        );
        if let Some(detail) = &outcome.detail {
            println!("{detail}");
        }
        outcomes.push(outcome);
    }
    let equal = outcomes.iter().filter(|o| o.status == "equal").count();
    let report = ResourcesReport {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("resources.json");
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

/// The files of a run folder both sides share: the inis below `mygames`,
/// the loose files below `Data` and the links of the archives.
fn run_files(
    scenario: &Scenario,
    game: &Game,
    archives: &[PathBuf],
) -> (Vec<(PathBuf, Vec<u8>)>, Vec<(PathBuf, PathBuf)>) {
    let mut files = Vec::new();
    files.push((
        PathBuf::from("mygames").join("game.ini"),
        scenario
            .game_ini
            .as_ref()
            .map(Text::text)
            .unwrap_or_default()
            .into_bytes(),
    ));
    if let Some(custom) = &scenario.custom_ini {
        files.push((
            PathBuf::from("mygames").join(custom_ini_name(game)),
            custom.text().into_bytes(),
        ));
    }
    for file in &scenario.files {
        files.push((
            PathBuf::from("Data").join(file.path.replace('\\', "/")),
            file.text.clone().into_bytes(),
        ));
    }
    let links = archives
        .iter()
        .map(|path| (PathBuf::from("Data").join(path.file_name().unwrap()), path.clone()))
        .collect();
    (files, links)
}

fn check(
    runner: &Runner,
    game: &'static Game,
    data: &Path,
    stem: &str,
    scenario: &Scenario,
    text: &str,
) -> Result<ResourcesOutcome> {
    let names: Vec<&str> = scenario.load.iter().map(String::as_str).collect();
    let plugins = load_list(game, data, &names)?;
    let archives = scenario_archives(scenario, data)?;
    let script = RESOURCES_SCRIPT.replace("{{PROBES}}", &probe_calls(&scenario.probes)?);
    let mut archive_text = String::new();
    for archive in &archives {
        let meta = fs::metadata(archive)?;
        archive_text.push_str(&format!(
            "{}:{}:{:?}\n",
            archive.file_name().unwrap().to_string_lossy(),
            meta.len(),
            meta.modified().ok()
        ));
    }
    let key = oracle_key(
        runner,
        &plugins,
        &format!("{text}{script}{archive_text}"),
        gui::exe_name(game.mode),
    )?;
    let cache_dir = runner
        .cache
        .join(format!("{}-oracle-resources", game.mode))
        .join(format!("{stem}.{key:016x}"));
    let (files, links) = run_files(scenario, game, &archives);
    if !cache_dir.join("status").exists() {
        run_oracle(
            runner,
            game,
            &plugins,
            script.clone(),
            scenario,
            &files,
            &links,
            &cache_dir,
            stem,
        )?;
    }
    let oracle = read_cached(&cache_dir)?;
    let mut outcome = ResourcesOutcome {
        scenario: stem.to_owned(),
        game: game.name,
        status: "oracle-failed",
        detail: None,
        probe_lines: oracle.probe.len(),
        loader_lines: oracle.loader.len(),
    };
    if oracle.status != "done" {
        outcome.detail = Some(format!("  the oracle script stopped: {}", oracle.status));
        return Ok(outcome);
    }
    let Some(port) = &runner.port else {
        outcome.status = "oracle-only";
        return Ok(outcome);
    };
    let port_output = match run_port(runner, port, game, stem, scenario, &plugins, &files, &links, &script) {
        Ok(output) => output,
        Err(error) => {
            outcome.status = "port-failed";
            outcome.detail = Some(format!("  {error:#}"));
            return Ok(outcome);
        }
    };
    if port_output.status != "done" {
        outcome.status = "port-failed";
        outcome.detail = Some(format!("  the port's script stopped: {}", port_output.status));
        return Ok(outcome);
    }
    let first_difference = |what: &str, a: &[String], b: &[String]| -> Option<String> {
        let index = a
            .iter()
            .zip(b)
            .position(|(x, y)| x != y)
            .or_else(|| (a.len() != b.len()).then(|| a.len().min(b.len())))?;
        Some(format!(
            "  {what} line {}: oracle {:?}, port {:?} ({} and {} lines)",
            index + 1,
            a.get(index).map_or("<end>", String::as_str),
            b.get(index).map_or("<end>", String::as_str),
            a.len(),
            b.len()
        ))
    };
    let differences: Vec<String> = [
        first_difference("probe", &oracle.probe, &port_output.probe),
        first_difference("loader", &oracle.loader, &port_output.loader),
    ]
    .into_iter()
    .flatten()
    .collect();
    if differences.is_empty() {
        outcome.status = "equal";
    } else {
        outcome.status = "different";
        outcome.detail = Some(differences.join("\n"));
    }
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
fn run_oracle(
    runner: &Runner,
    game: &Game,
    plugins: &[PathBuf],
    script: String,
    scenario: &Scenario,
    files: &[(PathBuf, Vec<u8>)],
    links: &[(PathBuf, PathBuf)],
    cache_dir: &Path,
    stem: &str,
) -> Result<()> {
    let work =
        runner
            .scratch
            .join("oracle-work")
            .join(format!("{}-resources-{stem}.{}", game.mode, std::process::id()));
    let extras = GuiExtras {
        files: files.to_vec(),
        links: links.to_vec(),
        args: scenario.switches.clone(),
        ..Default::default()
    };
    let peak_file = runner
        .cache
        .join(format!("{}-oracle-resources", game.mode))
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
    let data = work.join("Data");
    let last = result.status.last().cloned().unwrap_or_else(|| "no status".to_owned());
    fs::write(partial.join("loader.txt"), loader_lines(&result.log, &data).join("\n"))?;
    if last == "done" {
        fs::write(
            partial.join("probe.txt"),
            probe_lines(&result.out.join("resources.txt"), &data)?.join("\n"),
        )?;
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

fn read_cached(cache_dir: &Path) -> Result<SideOutput> {
    let lines = |name: &str| -> Vec<String> {
        fs::read_to_string(cache_dir.join(name))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    };
    Ok(SideOutput {
        status: fs::read_to_string(cache_dir.join("status"))?.trim().to_owned(),
        probe: lines("probe.txt"),
        loader: lines("loader.txt"),
    })
}

/// The port's run of a scenario: a run folder of its own laid out as the
/// GUI's (`Data` with the plugins, the archives and the loose files,
/// `mygames` with the inis, the plugin list, the script), and the GUI's
/// command line given to `xedit` (its legacy command line), so that the
/// port loads in the script tool mode as the oracle does.
#[allow(clippy::too_many_arguments)]
fn run_port(
    runner: &Runner,
    port: &Path,
    game: &Game,
    stem: &str,
    scenario: &Scenario,
    plugins: &[PathBuf],
    files: &[(PathBuf, Vec<u8>)],
    links: &[(PathBuf, PathBuf)],
    script: &str,
) -> Result<SideOutput> {
    let work = runner.scratch.join(format!("{}-resources", game.mode)).join(stem);
    if work.exists() {
        fs::remove_dir_all(&work).with_context(|| format!("removing {}", work.display()))?;
    }
    let data = work.join("Data");
    for dir in [&data, &work.join("out"), &work.join("mygames").join("Saves")] {
        fs::create_dir_all(dir)?;
    }
    let mut plugins_txt = String::new();
    for plugin in plugins {
        let name = plugin.file_name().context("plugin without a name")?;
        if fs::hard_link(plugin, data.join(name)).is_err() {
            fs::copy(plugin, data.join(name))?;
        }
        if !gui::simple_plugins_txt(game.mode) {
            plugins_txt.push('*');
        }
        plugins_txt.push_str(&name.to_string_lossy());
        plugins_txt.push_str("\r\n");
    }
    fs::write(work.join("plugins.txt"), plugins_txt)?;
    for (path, source) in links {
        let target = work.join(path);
        if fs::hard_link(source, &target).is_err() {
            fs::copy(source, &target)?;
        }
    }
    for (path, bytes) in files {
        let target = work.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, bytes)?;
    }
    let work_text = format!("{}\\", gui::windows_path(&work));
    fs::write(work.join("resources.pas"), script.replace("{{WORK}}", &work_text))?;
    fs::create_dir_all(work.join("bin"))?;
    let log = work.join("port.log");
    let mut command = std::process::Command::new(port);
    command
        .arg(format!("-{}", game.mode))
        .arg(format!("-D:{}\\", gui::windows_path(&data)))
        .arg(format!("-P:{}", gui::windows_path(&work.join("plugins.txt"))))
        .arg(format!("-M:{work_text}mygames\\"))
        .arg(format!("-I:{work_text}mygames\\game.ini"))
        .arg(format!("-S:{work_text}"))
        .arg(format!("-script:{work_text}resources.pas"))
        .arg("-IKnowWhatImDoing")
        .arg("-nobuildrefs");
    for switch in &scenario.switches {
        command.arg(switch);
    }
    let output = command
        .current_dir(&work)
        .stdin(std::process::Stdio::null())
        .stderr(File::create(&log)?)
        .output()?;
    let status = fs::read_to_string(work.join("status.txt"))
        .map(|text| text.lines().last().unwrap_or_default().to_owned())
        .unwrap_or_else(|_| {
            format!(
                "no status (exit {:?}): {}",
                output.status.code(),
                super::gui::tail(&String::from_utf8_lossy(&output.stdout), 5)
            )
        });
    let stderr = fs::read(&log).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())?;
    let probe = if status == "done" {
        probe_lines(&work.join("out").join("resources.txt"), &data)?
    } else {
        Vec::new()
    };
    Ok(SideOutput {
        status,
        probe,
        loader: loader_lines(&stderr, &data),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_lines_drop_the_gui_prefix_and_the_data_folder() {
        let log = "[00:00] Background Loader: starting...\n[00:01] Background Loader: [A - Main.ba2] Loading Resources.\n[00:01] Background Loader: [A - Main.ba2] Version: 8\n[00:02] Background Loader: [C:\\x\\Data\\] Setting Resource Path.\n[00:02] Background Loader: loading \"A.esm\"...";
        assert_eq!(
            loader_lines(log, Path::new("C:\\X\\Data")),
            [
                "[A - Main.ba2] Loading Resources.",
                "[A - Main.ba2] Version: 8",
                "[<data>\\] Setting Resource Path."
            ]
        );
    }

    #[test]
    fn probes_become_calls() {
        let calls = probe_calls(&[
            serde_json::json!({"containers": true}),
            serde_json::json!({"resource": "strings\\it's.strings"}),
            serde_json::json!({"listing": [1, "strings", 3]}),
            serde_json::json!({"value": ["A.esm", "01000800", "FULL"]}),
        ])
        .unwrap();
        assert!(calls.contains("ProbeContainers;"));
        assert!(calls.contains("ProbeResource('strings\\it''s.strings');"));
        assert!(calls.contains("ProbeListing(1, 'strings', 3);"));
        assert!(calls.contains("ProbeValue('A.esm', '01000800', 'FULL');"));
    }
}
