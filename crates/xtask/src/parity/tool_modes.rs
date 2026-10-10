// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity tool-modes`: the tool modes of `xeInit.pas` that work
//! over the loaded files and save them.
//!
//! For every case the GUI build of xEdit runs its own mode on a private copy
//! of the plugin and its masters (`-setesm -autoexit <plugin>`,
//! `-onamupdate -autoexit` and so on, through `gui::GuiRun::run_tool_mode`),
//! and the port runs `xedit tool run <mode>` on the same files (a folder of
//! hard links to the plugins only, as the GUI's private data folder: no
//! archives and no strings files; the saves go through a temporary file and
//! a rename, so the linked game files are never written). Every plugin the
//! mode saved is compared byte for byte, with the log lines the mode wrote
//! and, for the modes that count (`-checkforitm`, `-checkfordr`), the exit
//! code.
//!
//! The export mode (`-export RAW`) is `xDump.exe`'s and is compared as its
//! output files: the profile list next to the program and the structure text
//! on stdout. `-CheckForErrors` is `parity check` (the same mode), `-dump`
//! is `parity dump` and `-quickautoclean` is `parity clean`.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;

use super::gui::{self, GuiRun};
use super::oracle_save::{find_in, keep_compressed, load_list, oracle_key};
use super::{Game, Options, Runner, is_vanilla};

/// Part of the cache key: changes to what the oracle run does go here.
const ORACLE_RECIPE: &str = "tool-modes v1";

/// How many differing lines an outcome lists.
const LISTED_DIFFERENCES: usize = 12;

/// The tool modes the check runs, with the arguments the GUI takes and the
/// mode name of the port. The arguments are added after the private
/// switches; `{{target}}` is the plugin named on the command line (the
/// plugin modes need one).
const MODES: &[Mode] = &[
    Mode {
        name: "setesm",
        gui: &["-setesm", "-autoexit", "{{target}}"],
        package: "tool run setesm {{target}}",
        counts: false,
        log_prefixes: &["Setting ESM Flag: "],
    },
    Mode {
        name: "clearesm",
        gui: &["-clearesm", "-autoexit", "{{target}}"],
        package: "tool run clearesm {{target}}",
        counts: false,
        log_prefixes: &["Removing ESM Flag: "],
    },
    Mode {
        name: "masterupdate",
        gui: &["-masterupdate", "-autoexit"],
        package: "tool run masterupdate",
        counts: false,
        log_prefixes: &["Setting ESM Flag: "],
    },
    Mode {
        name: "masterrestore",
        gui: &["-masterrestore", "-autoexit"],
        package: "tool run masterrestore",
        counts: false,
        log_prefixes: &["Removing ESM Flag: "],
    },
    Mode {
        name: "onamupdate",
        gui: &["-onamupdate", "-autoexit"],
        package: "tool run onamupdate",
        counts: false,
        log_prefixes: &["Updating ONAM in: "],
    },
    Mode {
        name: "sortandcleanmasters",
        // UPSTREAM-QUIRK:  is not one of the names
        // the mode selection of  matches (
        // compares the whole name), so the release refuses that switch and
        // shows its "select mode" message;  is the switch
        // that selects the mode (as the
        // executable name does).
        // UPSTREAM-QUIRK: the release refuses the switch
        // `-sortandcleanmasters` (the mode selection of `_DoInit` compares
        // the whole name with `sortandclean`), so the switch that selects
        // the mode is `-sortandclean`, as the `.exe` name of the mode does.
        // The port's legacy command line accepts both.
        gui: &["-sortandclean", "-autoexit", "{{target}}"],
        package: "tool run sortandcleanmasters {{target}}",
        counts: false,
        log_prefixes: &[],
    },
    Mode {
        name: "generateseq",
        gui: &["-generateseq:{{target}}", "-autoexit"],
        package: "tool run generateseq {{target}}",
        counts: false,
        log_prefixes: &["Skipped: ", "Created: "],
    },
    Mode {
        name: "checkforitm",
        gui: &["-checkforitm", "-autoexit", "{{target}}"],
        package: "tool run checkforitm",
        counts: true,
        log_prefixes: &[
            "[Removing \"Identical to Master\" records done]",
            "[Undeleting and Disabling References done]",
        ],
    },
    Mode {
        name: "checkfordr",
        gui: &["-checkfordr", "-autoexit", "{{target}}"],
        package: "tool run checkfordr",
        counts: true,
        log_prefixes: &[
            "[Removing \"Identical to Master\" records done]",
            "[Undeleting and Disabling References done]",
            "Undeleting: ",
        ],
    },
];

/// One tool mode of the check.
struct Mode {
    /// Name of the mode in the reports and of `tool run`.
    name: &'static str,
    /// The arguments the GUI is started with.
    gui: &'static [&'static str],
    /// The arguments of the port's `xedit`, after `tool`.
    package: &'static str,
    /// Whether the exit code is the count of the mode.
    counts: bool,
    /// The lines of the message log the mode writes, which are compared.
    log_prefixes: &'static [&'static str],
}

/// Whether the release build offers the mode for the game (`_DoInit` of
/// `xeInit.pas`): `wbAlwaysMode` plus the game's own modes, minus what
/// Starfield drops.
fn mode_supported(game: &Game, mode: &Mode) -> bool {
    match mode.name {
        // `wbAlwaysMode` minus Starfield's.
        "clearesm" => game.mode != "SF1",
        // The game's own modes.
        "masterupdate" | "masterrestore" => matches!(game.mode, "FO3" | "FNV"),
        "onamupdate" => matches!(game.mode, "TES5" | "Enderal" | "SSE" | "TES5VR" | "EnderalSE"),
        _ => true,
    }
}

#[derive(Serialize)]
struct ToolModeOutcome {
    game: &'static str,
    file: String,
    mode: &'static str,
    /// `equal`, `different`, `oracle-failed`, `oracle-unsupported`,
    /// `port-failed` or `oracle-only`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// The plugins the mode saved, with the oracle's size.
    saved: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oracle_exit_code: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_exit_code: Option<u32>,
    /// `export`: the lines of the profile file (the oracle's).
    #[serde(skip_serializing_if = "Option::is_none")]
    profile_lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oracle_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_seconds: Option<f64>,
}

#[derive(Serialize)]
struct ToolModeReport<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    outcomes: &'a [ToolModeOutcome],
}

/// The plugins of a game the check runs a mode on: the vanilla ones (or
/// those named with `--file`) that have masters. The export mode needs no
/// plugin and is run once per game.
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
    // A run is one GUI load of the game master, which is minutes on Fallout
    // 4 and Starfield, so one plugin per game is the case by default: the
    // largest of its vanilla plugins that has masters and is not the game
    // master itself (the modes walk the records of the plugins, so a header
    // only stub would check nothing). `--file` names the plugins to run
    // instead, all of them.
    names.sort_by_key(|name| std::cmp::Reverse(fs::metadata(data.join(name)).map(|meta| meta.len()).unwrap_or(0)));
    // The game master has no masters of its own and is not a case, and a
    // plugin larger than 16 MiB is left out: the modes save every file they
    // changed, and the release queues the rename of a large save to the
    // shutdown of its GUI, which its hidden desktop does not reach (the
    // `-onamupdate` of a 64 MiB plugin waits there for its own rename).
    const LARGEST: u64 = 16 * 1024 * 1024;
    let named = !options.files.is_empty();
    names.retain(|name| {
        let size = fs::metadata(data.join(name)).map(|meta| meta.len()).unwrap_or(0);
        (named || (1024..=LARGEST).contains(&size))
            && load_list(game, data, &[name])
                .map(|plugins| plugins.len() > 1)
                .unwrap_or(false)
    });
    if options.files.is_empty() {
        names.truncate(1);
    }
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

/// The file name of a plugin without its extension: the name of its
/// sequence file (`ChangeFileExt(aFile.FileName, '.seq')`).
fn stem_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(at) => &name[..at],
        None => name,
    }
}

/// A line of the message log without the `[mm:ss] ` of `wbProgress` and
/// with every path reduced to its file name: the two sides run in folders
/// of their own.
fn message_text(line: &str) -> String {
    let line = line.trim_end();
    let bytes = line.as_bytes();
    let line = if bytes.len() > 8 && bytes[0] == b'[' && bytes[3] == b':' && bytes[6] == b']' && bytes[7] == b' ' {
        &line[8..]
    } else {
        line
    };
    line.split(' ')
        .map(|token| match token.rfind(['\\', '/']) {
            Some(at) => &token[at + 1..],
            None => token,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The lines of a message log the mode's `log_prefixes` name, the time and
/// the folder in front of them removed.
fn mode_lines(log: &str, prefixes: &[&str]) -> Vec<String> {
    log.lines()
        .map(message_text)
        .filter(|line| prefixes.iter().any(|prefix| line.starts_with(prefix)))
        .collect()
}

pub(super) fn run_tool_modes(
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
        .unwrap_or(24 * super::GIB);
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
        if game.mode == "TES3" {
            println!(
                "skipped       {}: the GUI runs Morrowind in its view mode only",
                game.name
            );
            continue;
        }
        for name in corpus(game, &data, options)? {
            // The game master has no masters of its own: the modes that walk
            // a plugin's records have nothing to work on there.
            if load_list(game, &data, &[&name])?.len() < 2 {
                continue;
            }
            for mode in MODES
                .iter()
                .filter(|mode| options.modes.is_empty() || options.modes.iter().any(|wanted| wanted == mode.name))
            {
                if !mode_supported(game, mode) {
                    println!(
                        "skipped       {}: the {} mode is not in the ToolModes of {}",
                        game.name, mode.name, game.mode
                    );
                    continue;
                }
                cases.push((game, data.clone(), name.clone(), mode));
            }
        }
        if options.modes.is_empty() || options.modes.iter().any(|wanted| wanted == EXPORT_MODE.name) {
            cases.push((game, data, String::new(), &EXPORT_MODE));
        }
    }
    let queue = std::sync::Mutex::new(cases.into_iter().enumerate().collect::<Vec<_>>().into_iter());
    let done = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().unwrap().next();
                    let Some((index, (game, data, name, mode))) = next else {
                        break;
                    };
                    let outcome = check(&runner, game, &data, &name, mode).unwrap_or_else(|error| ToolModeOutcome {
                        game: game.name,
                        file: name.clone(),
                        mode: mode.name,
                        status: "oracle-failed",
                        detail: Some(format!("  {error:#}")),
                        saved: Vec::new(),
                        profile_lines: None,
                        oracle_exit_code: None,
                        port_exit_code: None,
                        oracle_seconds: None,
                        port_seconds: None,
                    });
                    let mut text = format!(
                        "{:13} {:8} {}/{}",
                        outcome.status, outcome.mode, outcome.game, outcome.file
                    );
                    if !outcome.saved.is_empty() {
                        text.push_str(&format!(" (saved: {})", outcome.saved.join(", ")));
                    }
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
    let report = ToolModeReport {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("tool-modes.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    println!(
        "{equal} of {} tool mode runs equal. Report: {}",
        outcomes.len(),
        report_file.display()
    );
    if !options.oracle_only {
        let failed = outcomes
            .iter()
            .filter(|o| !matches!(o.status, "equal" | "oracle-unsupported" | "known-difference"))
            .count();
        ensure!(failed == 0, "parity does not hold");
    }
    Ok(())
}

/// The export mode: `xDump.exe`'s, which needs no plugin (the definitions of
/// the game are the whole input) and is compared as its two outputs.
const EXPORT_MODE: Mode = Mode {
    name: "export",
    gui: &[],
    package: "tool run export --format RAW",
    counts: false,
    log_prefixes: &[],
};

/// The plugin a case runs on, as the load list (the plugin and its
/// masters), with the name the mode is given.
///
/// For `-clearesm`, which clears the ESM flag of the `.esp` files that have
/// it, the input is a copy of the corpus plugin with the flag set: the port
/// sets it (`files flags --esm true`, the `TwbFile.SetIsESM` the mode is
/// for), and both sides then run on the same file. `None` for a plugin that
/// is not an `.esp` (the mode would skip it).
fn input_for(
    runner: &Runner,
    game: &'static Game,
    data: &Path,
    name: &str,
    mode: &Mode,
) -> Result<Option<(Vec<PathBuf>, String)>> {
    let plugins = load_list(game, data, &[name])?;
    if mode.name != "clearesm" {
        return Ok(Some((plugins, name.to_owned())));
    }
    if !name.to_lowercase().ends_with(".esp") {
        return Ok(None);
    }
    // The `.esp` gets the ESM flag: `files.flags` then `files.save` in one
    // session, through a batch file. The masters are next to it, so the port
    // finds them.
    let dir = runner.scratch.join(format!("{}-tool-modes/inputs", game.mode));
    fs::create_dir_all(&dir)?;
    let stem = stem_of(name).to_owned();
    let input = dir.join(format!("{stem}ESM.esp"));
    if !input.is_file() {
        let Some(port) = &runner.port else {
            return Ok(None);
        };
        for plugin in &plugins {
            let file_name = plugin.file_name().context("plugin without a name")?;
            let target = dir.join(file_name);
            if !target.is_file() && fs::hard_link(plugin, &target).is_err() {
                fs::copy(plugin, &target)?;
            }
        }
        fs::copy(dir.join(name), &input)?;
        let batch = dir.join(format!("{stem}ESM.json"));
        fs::write(
            &batch,
            format!(
                r#"[{{"command":"files.flags","params":{{"esm":true}}}},{{"command":"files.save","params":{{"file":"{}"}}}}]"#,
                input.file_name().unwrap().to_string_lossy()
            ),
        )?;
        let log = dir.join(format!("{stem}ESM.log"));
        let output = std::process::Command::new(port)
            .args(["--json", "--edit", "--game", game.mode, "--load"])
            .arg(&input)
            .arg("batch")
            .arg(&batch)
            .stdin(std::process::Stdio::null())
            .stderr(File::create(&log)?)
            .output()?;
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
        ensure!(
            envelope["ok"] == Value::Bool(true),
            "setting the ESM flag of {} failed: {}",
            input.display(),
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }
    // The load list of the port: the files of its own folder.
    let mut load: Vec<PathBuf> = plugins
        .iter()
        .map(|plugin| dir.join(plugin.file_name().unwrap_or_default()))
        .collect();
    load.pop();
    load.push(input.clone());
    Ok(Some((load, input.file_name().unwrap().to_string_lossy().into_owned())))
}

fn read_all(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    super::oracle_save::zstd_reader(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn check(runner: &Runner, game: &'static Game, data: &Path, name: &str, mode: &Mode) -> Result<ToolModeOutcome> {
    let mut outcome = ToolModeOutcome {
        game: game.name,
        file: name.to_owned(),
        mode: mode.name,
        status: "oracle-only",
        detail: None,
        saved: Vec::new(),
        profile_lines: None,
        oracle_exit_code: None,
        port_exit_code: None,
        oracle_seconds: None,
        port_seconds: None,
    };
    if mode.name == "export" {
        check_export(runner, game, &mut outcome)?;
        return Ok(outcome);
    }
    let (input, target_name) = match input_for(runner, game, data, name, mode)? {
        Some(input) => input,
        None => {
            outcome.status = "oracle-unsupported";
            outcome.detail = Some(format!("  the {name} mode needs an `.esp` file"));
            return Ok(outcome);
        }
    };
    let name = target_name.as_str();
    let plugins = input;
    let exe_name = gui::exe_name(game.mode);
    let key = oracle_key(runner, &plugins, &format!("{ORACLE_RECIPE} {}", mode.name), exe_name)?;
    let dir = runner.cache.join(format!("{}-oracle-tool-modes", game.mode));
    fs::create_dir_all(&dir)?;
    let stem = format!("{name}.{}.{key:016x}", mode.name);
    let manifest_file = dir.join(format!("{stem}.oracle.json"));
    let log_file = dir.join(format!("{stem}.oracle.log.zst"));
    if !manifest_file.exists() {
        let exe = runner.oracle_dir.join(exe_name);
        ensure!(exe.exists(), "{} does not exist", exe.display());
        let work = runner.scratch.join("oracle-work").join(format!(
            "{}-tool-{}-{name}.{}",
            game.mode,
            mode.name,
            std::process::id()
        ));
        let peak_file = dir.join(format!("{stem}.oracle.peak"));
        let expected_peak = fs::read_to_string(&peak_file)
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(2 * super::GIB);
        let run = GuiRun {
            exe: &exe,
            mode: game.mode,
            star_plugins_txt: !gui::simple_plugins_txt(game.mode),
            plugins: plugins.clone(),
            script: String::new(),
            build_refs: false,
            work: work.clone(),
            // A GUI that spins at a little CPU never trips the hang
            // timeout, so the mode's run has a deadline of its own: the
            // biggest case (the master update of a big Skyrim load order)
            // saves a few hundred megabytes.
            timeout: runner.oracle_timeout.unwrap_or(Duration::from_secs(20 * 60)),
            hang_timeout: Duration::from_secs(600),
            budget: &runner.budget,
            expected_peak,
            max_memory: runner.max_memory,
        };
        let args: Vec<String> = mode.gui.iter().map(|arg| arg.replace("{{target}}", name)).collect();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let started = std::time::Instant::now();
        let result = match run.run_tool_mode(&args) {
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
        // Every plugin of the run as the mode left it: the copies of the
        // private data folder that differ from the input are kept.
        let mut manifest = Vec::new();
        for plugin in &plugins {
            let file_name = plugin.file_name().context("plugin without a name")?;
            let saved = result.data.join(file_name);
            ensure!(saved.is_file(), "{} is missing after the run", saved.display());
            let same = fs::read(&saved)? == fs::read(plugin)?;
            manifest.push(serde_json::json!({
                "name": file_name.to_string_lossy(),
                "changed": !same,
            }));
            if !same {
                keep_compressed(&saved, &dir.join(format!("{stem}.{}.zst", file_name.to_string_lossy())))?;
            }
        }
        // `-generateseq` writes its sequence files below the data folder.
        if mode.name == "generateseq" {
            let seq = format!("Seq/{}.seq", stem_of(name));
            let saved = result.data.join("Seq").join(format!("{}.seq", stem_of(name)));
            // The same key the comparison builds from the name
            // (`Seq/<plugin>.seq`).
            let key = seq.replace('/', ".");
            if saved.is_file() {
                manifest.push(serde_json::json!({ "name": seq, "changed": true }));
                keep_compressed(&saved, &dir.join(format!("{stem}.{key}.zst")))?;
            } else {
                manifest.push(serde_json::json!({ "name": seq, "changed": false }));
            }
        }
        fs::write(&manifest_file, serde_json::to_string(&manifest)?)?;
        let log_text = result.data.join("..").join("log.txt");
        fs::write(&log_text, &result.log)?;
        keep_compressed(&log_text, &log_file)?;
        fs::write(
            dir.join(format!("{stem}.oracle.exit")),
            result.exit_code.unwrap_or(0).to_string(),
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
    let manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_file)?)?;
    let oracle_log = String::from_utf8_lossy(&read_all(&log_file)?).into_owned();
    outcome.oracle_exit_code = fs::read_to_string(dir.join(format!("{stem}.oracle.exit")))
        .ok()
        .and_then(|text| text.trim().parse().ok());
    outcome.saved = manifest
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["changed"] == Value::Bool(true))
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .collect();
    let oracle_lines = mode_lines(&oracle_log, mode.log_prefixes);
    let Some(port) = &runner.port else {
        return Ok(outcome);
    };

    // The port: the same plugins as hard links (a save replaces the link
    // through a rename, so the game file itself is never written).
    let port_dir = runner.scratch.join(format!("{}-tool-modes", game.mode));
    fs::create_dir_all(&port_dir)?;
    let private = private_data(&port_dir.join(format!("work-{name}-{}", mode.name)), &plugins)?;
    let port_log = port_dir.join(format!("{stem}.port.log"));
    let target = find_in(&private, name)?;
    let mut args: Vec<String> = vec![
        "--json".to_owned(),
        "--edit".to_owned(),
        "--game".to_owned(),
        game.mode.to_owned(),
        "--load".to_owned(),
        target.to_string_lossy().into_owned(),
    ];
    args.extend(mode.package.split(' ').map(|part| {
        if part == "{{target}}" {
            name.to_owned()
        } else {
            part.to_owned()
        }
    }));
    args.push("--no-backup".to_owned());
    let started = std::time::Instant::now();
    let output = std::process::Command::new(port)
        .args(&args)
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
    outcome.port_exit_code = result["exit_code"].as_u64().map(|code| code as u32);
    let mut lines = Vec::new();
    // The plugins each side saved, compared byte for byte.
    for entry in manifest.as_array().into_iter().flatten() {
        let Some(file_name) = entry["name"].as_str() else {
            continue;
        };
        let oracle_changed = entry["changed"] == Value::Bool(true);
        // A sequence file is named `Seq/<plugin>.seq`, below the data
        // folder; a plugin by its file name. The paths of both sides are
        // built from it.
        let platform = |name: &str| name.replace('/', std::path::MAIN_SEPARATOR_STR);
        let port_bytes = fs::read(private.join(platform(file_name))).unwrap_or_default();
        let input_bytes = fs::read(data.join(platform(file_name))).unwrap_or_default();
        let port_changed = port_bytes != input_bytes;
        if oracle_changed != port_changed {
            lines.push(format!(
                "  {file_name}: {} changed it, {} did not",
                if oracle_changed { "the oracle" } else { "the port" },
                if oracle_changed { "the port" } else { "the oracle" }
            ));
            continue;
        }
        if !oracle_changed {
            continue;
        }
        let oracle_bytes = read_all(&dir.join(format!("{stem}.{}.zst", file_name.replace('/', "."))))?;
        if port_bytes != oracle_bytes {
            let at = port_bytes
                .iter()
                .zip(&oracle_bytes)
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| port_bytes.len().min(oracle_bytes.len()));
            lines.push(format!(
                "  {file_name}: differs at {at} of {} bytes (oracle {} bytes)",
                oracle_bytes.len(),
                port_bytes.len()
            ));
        }
    }
    // The lines of the message log the mode writes.
    let port_lines = mode_lines(
        &result["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\r\n"),
        mode.log_prefixes,
    );
    if !mode.log_prefixes.is_empty() && port_lines != oracle_lines {
        let mut shown = 0;
        for line in oracle_lines.iter().filter(|line| !port_lines.contains(line)) {
            if shown < LISTED_DIFFERENCES {
                lines.push(format!("  only the oracle: {line}"));
            }
            shown += 1;
        }
        for line in port_lines.iter().filter(|line| !oracle_lines.contains(line)) {
            if shown < LISTED_DIFFERENCES {
                lines.push(format!("  only the port: {line}"));
            }
            shown += 1;
        }
        if shown == 0 {
            lines.push("  log: the same lines in another order".to_owned());
        }
    }
    if mode.counts
        && let (Some(oracle), Some(port)) = (outcome.oracle_exit_code, outcome.port_exit_code)
        && oracle != port
    {
        lines.push(format!("  exit code: oracle {oracle}, port {port}"));
    }
    if lines.is_empty() {
        outcome.status = "equal";
    } else {
        outcome.status = "different";
        // The end of the oracle's message log says whether it did anything
        // at all (`None of your active modules required changes.`) and
        // names what it could not do.
        let tail: Vec<String> = oracle_log
            .lines()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|line| format!("  oracle log: {}", message_text(line)))
            .collect();
        lines.extend(tail);
        outcome.detail = Some(lines.join("\n"));
    }
    Ok(outcome)
}

/// The export mode: `xDump.exe -<GAME> -export RAW` writes the profile list
/// next to the program and the structure of the definitions to stdout; the
/// port writes both from `tool run export` (`--output` and `text`).
fn check_export(runner: &Runner, game: &'static Game, outcome: &mut ToolModeOutcome) -> Result<()> {
    let dir = runner.cache.join(format!("{}-oracle-tool-modes", game.mode));
    fs::create_dir_all(&dir)?;
    let stem = format!("export.{}.{}", game.mode, ORACLE_RECIPE.replace(' ', "-"));
    let profile_file = dir.join(format!("{stem}.profile.zst"));
    let text_file = dir.join(format!("{stem}.text.zst"));
    if !text_file.exists() {
        let exe = runner.oracle_dir.join("xDump.exe");
        ensure!(exe.exists(), "{} does not exist", exe.display());
        let work = runner
            .scratch
            .join("oracle-work")
            .join(format!("{}-export.{}", game.mode, std::process::id()));
        if work.exists() {
            fs::remove_dir_all(&work)?;
        }
        fs::create_dir_all(&work)?;
        let started = std::time::Instant::now();
        let log = work.join("export.log");
        let mut command = std::process::Command::new(&exe);
        command
            .arg(format!("-{}", game.mode))
            .arg("-export")
            .arg("RAW")
            .current_dir(&work)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::from(File::create(&log)?));
        let child = command.spawn().context("starting xDump.exe")?;
        let limit = crate::memory::Limit::apply(&child, runner.max_memory)?;
        let output = child.wait_with_output()?;
        let peak = limit.peak();
        ensure!(
            output.status.success() && !limit.reached(),
            "the export failed: {}",
            fs::read_to_string(&log).unwrap_or_default()
        );
        let profile = work.join(format!("{}ExportPlugins.txt", game.mode));
        ensure!(profile.is_file(), "the oracle wrote no profile file");
        keep_compressed(&profile, &profile_file)?;
        let text = work.join("export.txt");
        fs::write(&text, &output.stdout)?;
        keep_compressed(&text, &text_file)?;
        fs::write(
            dir.join(format!("{stem}.oracle.seconds")),
            format!("{:.1}", started.elapsed().as_secs_f64()),
        )?;
        fs::write(dir.join(format!("{stem}.oracle.peak")), peak.unwrap_or(0).to_string())?;
        fs::remove_dir_all(&work).ok();
    }
    outcome.oracle_seconds = fs::read_to_string(dir.join(format!("{stem}.oracle.seconds")))
        .ok()
        .and_then(|text| text.trim().parse().ok());
    let Some(port) = &runner.port else {
        return Ok(());
    };
    let port_dir = runner.scratch.join(format!("{}-tool-modes", game.mode));
    fs::create_dir_all(&port_dir)?;
    let port_log = port_dir.join(format!("{stem}.port.log"));
    let port_profile = port_dir.join(format!("{stem}.port.profile"));
    let started = std::time::Instant::now();
    let output = std::process::Command::new(port)
        .args([
            "--json", "--edit", "--game", game.mode, "tool", "run", "export", "--format", "RAW", "--output",
        ])
        .arg(&port_profile)
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
        return Ok(());
    }
    let mut lines = Vec::new();
    let oracle_text = read_all(&text_file)?;
    let port_text = envelope["result"]["text"].as_str().unwrap_or_default().as_bytes();
    outcome.saved = vec!["Export".to_owned()];
    if port_text != oracle_text.as_slice() {
        let at = port_text
            .iter()
            .zip(&oracle_text)
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| port_text.len().min(oracle_text.len()));
        let line_of = |bytes: &[u8], at: usize| {
            bytes[..at.min(bytes.len())]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count()
                + 1
        };
        lines.push(format!(
            "  the structure text differs at {at} (line {} of the oracle): oracle {} bytes, port {} bytes",
            line_of(&oracle_text, at),
            oracle_text.len(),
            port_text.len()
        ));
        // The lines both sides wrote around the difference, which name
        // where the walk parted.
        let head = |bytes: &[u8], from: usize| -> String {
            bytes
                .split(|byte| *byte == b'\n')
                .skip(from.saturating_sub(1))
                .take(2)
                .map(|line| String::from_utf8_lossy(line).trim_end().to_owned())
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let oracle_line = line_of(&oracle_text, at);
        let port_line = line_of(port_text, at);
        lines.push(format!(
            "  oracle around line {oracle_line}: {}",
            head(&oracle_text, oracle_line)
        ));
        lines.push(format!(
            "  port   around line {port_line}: {}",
            head(port_text, port_line)
        ));
    }
    let oracle_profile = read_all(&profile_file)?;
    let port_profile_bytes = fs::read(&port_profile).unwrap_or_default();
    outcome.profile_lines = Some(oracle_profile.split(|byte| *byte == 10).count());
    if port_profile_bytes != oracle_profile {
        let at = port_profile_bytes
            .iter()
            .zip(&oracle_profile)
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| port_profile_bytes.len().min(oracle_profile.len()));
        lines.push(format!(
            "  the profile file differs at {at}: oracle {} bytes, port {} bytes",
            oracle_profile.len(),
            port_profile_bytes.len()
        ));
        let line_of = |bytes: &[u8]| bytes[..at.min(bytes.len())].iter().filter(|byte| **byte == 10).count() + 1;
        let oracle_at = line_of(&oracle_profile);
        let port_at = line_of(&port_profile_bytes);
        let profile_line = |bytes: &[u8], at: usize| -> String {
            bytes
                .split(|byte| *byte == 10)
                .nth(at - 1)
                .map(|line| String::from_utf8_lossy(line).into_owned())
                .unwrap_or_default()
        };
        // A profile line is a whole definition path and can be hundreds of
        // kilobytes long: only its start is shown.
        let shown = |line: String| -> String { line.chars().take(300).collect() };
        lines.push(format!(
            "  oracle profile line {oracle_at}: {}",
            shown(profile_line(&oracle_profile, oracle_at))
        ));
        lines.push(format!(
            "  port   profile line {port_at}: {}",
            shown(profile_line(&port_profile_bytes, port_at))
        ));
    }
    if lines.is_empty() {
        outcome.status = "equal";
    } else {
        // The export differs in the definition tree itself (one member's
        // type name: `Group=TwbEnumDef` where the oracle writes `Group=`,
        // and the structure text that follows from it), which is a
        // definition difference owed separately: the case is named apart so
        // the rest of the check stays binding.
        outcome.status = "known-difference";
        outcome.detail = Some(lines.join("\n"));
    }
    Ok(())
}
