// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity discovery`: what `DoInitPath` of `xeInit.pas` finds
//! for a game when the command line names neither `-D:` nor `-P:`, and the
//! module list `wbLoadModules` builds from it (issue #83).
//!
//! The first case of a game is the original one: the GUI build and the port
//! both run their `-CheckForITM` mode without `-D:` and `-P:`: the data
//! folder comes from the folder of the program, the Steam library and the
//! registry, the plugin list from the game's own `Plugins.txt` in the local
//! application data folder, and the save folder from `SLocalSavePath` of the
//! game's ini below `My Games`. Both print the `Using ...` lines of the main
//! form's startup log (`xeMainForm.pas:5200`) and exit with the ITM count of
//! the last loaded plugin.
//!
//! The module-list cases build a data folder of their own (a synthetic
//! plugin file per module, so no game install is needed) and run the GUI's
//! script mode with `-autoload`: the loaded files are read from the
//! `[<file>] Loading file` progress lines of both runs and compared with
//! the `Using ...` lines. Three cases:
//!
//! - `implicit-actives` (SSE and FO4): the plugin list names no official or
//!   Creation Club module, and the release still loads the game master,
//!   `Update.esm` of the Skyrim games, the official DLC and the modules of
//!   the game's `.ccc` list.
//! - `loadorder` (Skyrim LE): a `loadorder.txt` next to the plugin list,
//!   which the game reads (`wbGameMode in [gmTES4R, gmTES5, gmEnderal]`).
//! - `missing-masters` (FO4): a plugin list that marks active a module whose
//!   master is not there; the module is not loaded.
//!
//! Everything but the data folder, the inis and the plugin list stays
//! private: the temporary, cache, backup, scripts, My Games and save
//! folders are the run's own (`-T:`, `-C:`, `-B:`, `-S:`, `-M:`, `-G:`), an
//! empty `<APP><Tool>.ini` in the program folder keeps the GUI from
//! creating a settings file next to the discovered `Plugins.txt`, and the
//! port runs from a copy of its own binary, so its program folder is the
//! run's program folder too.
//!
//! The paths the first case compares are therefore the machine's (the
//! registry key, `%LOCALAPPDATA%`, `%USERPROFILE%`), which is the point of
//! it: the comparison is only meaningful on a machine where the game is
//! installed and has been started once. The module-list cases compare only
//! their own folders.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::gui::{self, GuiExtras, GuiRun};
use super::{Game, Options};

/// The prefixes of the startup log the cases compare: what `DoInitPath`
/// found and what the load active list is.
const PREFIXES: &[&str] = &[
    "Using ",
    "Loading active plugin list:",
    "Warning: Could not find plugin list",
];

/// The script both sides run in the module-list cases: the module selection
/// is answered by `-autoload`, the script only ends the run (an empty
/// `Process` is absent, so no record is walked), and the GUI posts its
/// closing line.
const DISCOVERY_SCRIPT: &str =
    "unit DiscoveryCase;\r\n\r\nfunction Initialize: Integer;\r\nbegin\r\n  Result := 0;\r\nend;\r\n\r\nend.\r\n";

#[derive(Serialize)]
struct DiscoveryOutcome {
    game: &'static str,
    /// `discovery` for the original case, else the module-list case.
    case: &'static str,
    /// `equal`, `different`, `oracle-failed` or `port-failed`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// The lines both sides wrote, with the run folder of each side mapped
    /// to `<root>`.
    lines: Vec<String>,
    /// The files the runs loaded, in load order (`[<file>] Loading file`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    loaded: Vec<String>,
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
        let line = line.replace(&work_text.replace('/', "\\"), "<root>");
        if PREFIXES.iter().any(|prefix| line.starts_with(prefix)) {
            lines.push(line);
        }
    }
    lines
}

/// The module a `[<file>] Loading file` progress line names: the release
/// writes `[mm:ss] [<file>] Loading file` to its message log
/// (`TwbFile.flProgress`), the port `[<file>] Loading file` to stderr. The
/// hardcoded file (the game executable's `.exe`) is left out: the release
/// loads it right after the first module and the port after all of them,
/// and it is not a module of the list the case compares.
fn loaded_files(log: &str) -> Vec<String> {
    log.lines()
        .filter_map(|line| {
            let line = line.trim_end();
            let rest = line.strip_suffix(" Loading file")?;
            let start = rest.rfind('[')?;
            let end = rest.rfind(']')?;
            let name = rest.get(start + 1..end)?;
            (!name.to_ascii_lowercase().ends_with(".exe")).then(|| name.to_owned())
        })
        .collect()
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
        let data_var = std::env::var_os(game.data_var).map(PathBuf::from);
        // The original case: the game's own plugin list and data folder.
        let runnable =
            data_var.is_some() && game.mode != "TES3" && plugins_file(game.mode).is_some_and(|path| path.is_file());
        if runnable {
            let outcome = check(
                runner(&cache, &scratch, &oracle_dir, port.clone(), options)?,
                game,
                options,
            )
            .unwrap_or_else(|error| failed(game.name, "discovery", &error));
            print_outcome(&outcome);
            outcomes.push(outcome);
        } else if game.mode == "TES3" && data_var.is_some() {
            println!(
                "skipped       {}: the GUI runs Morrowind in its view mode only",
                game.name
            );
        } else if data_var.is_none() {
            ensure!(options.all_games, "environment variable {} is not set", game.data_var);
            println!("skipped       {}: {} is not set", game.name, game.data_var);
        } else if let Some(path) = plugins_file(game.mode) {
            println!("skipped       {}: {} does not exist", game.name, path.display());
        } else {
            println!(
                "skipped       {}: the game has no local application data folder",
                game.name
            );
        }
        // The module-list cases build their own data folder and plugin
        // list, so they need no game install.
        for case in MODULE_CASES
            .iter()
            .filter(|case| case.mode.eq_ignore_ascii_case(game.mode))
        {
            let outcome = check_module_case(
                runner(&cache, &scratch, &oracle_dir, port.clone(), options)?,
                game,
                case,
                options,
            )
            .unwrap_or_else(|error| failed(game.name, case.name, &error));
            print_outcome(&outcome);
            outcomes.push(outcome);
        }
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

fn failed(game: &'static str, case: &'static str, error: &anyhow::Error) -> DiscoveryOutcome {
    DiscoveryOutcome {
        game,
        case,
        status: "oracle-failed",
        detail: Some(format!("  {error:#}")),
        lines: Vec::new(),
        loaded: Vec::new(),
        oracle_exit_code: None,
        port_exit_code: None,
    }
}

fn print_outcome(outcome: &DiscoveryOutcome) {
    let mut text = format!("{:12} {} {}", outcome.status, outcome.game, outcome.case);
    if let Some(detail) = &outcome.detail {
        text.push('\n');
        text.push_str(detail);
    }
    println!("{text}");
}

/// The piece of `Runner` the cases need: the port binary, the folders and
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
        case: "discovery",
        status: "different",
        detail: None,
        lines: Vec::new(),
        loaded: Vec::new(),
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
    // `-autoload`: the module selection of the GUI is answered by the GUI
    // itself, as the tool-modes check does (a dialog a hidden desktop cannot
    // answer ends the run instead).
    let result = run.run_tool_mode(&["-autoload", "-CheckForITM", &master, "-autoexit"])?;
    outcome.oracle_exit_code = result.exit_code;
    let oracle_lines = compared_lines(&result.log, &oracle_work);
    let oracle_loaded = loaded_files(&result.log);
    let Some(port) = &runner.port else {
        outcome.status = "oracle-only";
        outcome.lines = oracle_lines;
        outcome.loaded = oracle_loaded;
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
    let port_loaded = loaded_files(&fs::read_to_string(&log_file).unwrap_or_default());
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
    if oracle_loaded != port_loaded {
        differences.push(format!(
            "  loaded files differ:\n    oracle {:?}\n    port   {:?}",
            oracle_loaded, port_loaded
        ));
    }
    // The exit code is not compared: the harness ends the GUI at its own
    // `--= All Done =--` line, so what it reads there is not the mode's own
    // exit code (`ExitCode := CheckResult`). `parity tool-modes` compares
    // the count of the check modes where the oracle exits by itself.
    outcome.lines = oracle_lines;
    outcome.loaded = oracle_loaded;
    if differences.is_empty() {
        outcome.status = "equal";
    } else {
        outcome.status = "different";
        outcome.detail = Some(differences.join("\n"));
    }
    gui::remove_work(&oracle_work);
    Ok(outcome)
}

/// A module-list case: a data folder of synthetic plugins (the header flags
/// and the masters of each), the game's Creation Club list, the plugin list
/// and, where the game reads one, a `loadorder.txt`, all under the run's
/// own folder.
struct ModuleCase {
    /// The name of the case in the report.
    name: &'static str,
    /// The game mode switch.
    mode: &'static str,
    /// `(file name, header record flags, masters)` of each module.
    modules: &'static [(&'static str, u32, &'static [&'static str])],
    /// `(file name, text)` of the Creation Club list next to the data
    /// folder.
    ccc: Option<(&'static str, &'static str)>,
    /// The plugin list.
    plugins: &'static str,
    /// The `loadorder.txt` next to the plugin list.
    loadorder: Option<&'static str>,
}

const MODULE_CASES: &[ModuleCase] = &[
    ModuleCase {
        name: "implicit-actives",
        mode: "SSE",
        modules: &[
            ("Skyrim.esm", 0x1, &[]),
            ("Update.esm", 0x1, &["Skyrim.esm"]),
            ("Dawnguard.esm", 0x1, &["Skyrim.esm"]),
            ("HearthFires.esm", 0x1, &["Skyrim.esm"]),
            ("Dragonborn.esm", 0x1, &["Skyrim.esm"]),
            ("ccTestA.esl", 0x201, &["Skyrim.esm"]),
            ("ccTestB.esl", 0x201, &["Skyrim.esm"]),
            ("Mod.esp", 0, &["Skyrim.esm"]),
        ],
        ccc: Some(("Skyrim.ccc", "ccTestA.esl\r\nccTestB.esl\r\n")),
        plugins: "# the list names no official or Creation Club module\r\n*Mod.esp\r\n",
        loadorder: None,
    },
    ModuleCase {
        name: "implicit-actives",
        mode: "FO4",
        modules: &[
            ("Fallout4.esm", 0x1, &[]),
            ("DLCRobot.esm", 0x1, &["Fallout4.esm"]),
            ("ccTestA.esl", 0x201, &["Fallout4.esm"]),
            ("ccTestB.esl", 0x201, &["Fallout4.esm"]),
            ("Mod.esp", 0, &["Fallout4.esm"]),
        ],
        ccc: Some(("Fallout4.ccc", "ccTestA.esl\r\nccTestB.esl\r\n")),
        plugins: "# the list names no official or Creation Club module\r\n*Mod.esp\r\n",
        loadorder: None,
    },
    ModuleCase {
        name: "loadorder",
        mode: "TES5",
        modules: &[
            ("Skyrim.esm", 0x1, &[]),
            ("Update.esm", 0x1, &["Skyrim.esm"]),
            ("Dawnguard.esm", 0x1, &["Skyrim.esm"]),
            ("HearthFires.esm", 0x1, &["Skyrim.esm"]),
            ("Dragonborn.esm", 0x1, &["Skyrim.esm"]),
            ("ModA.esp", 0, &["Skyrim.esm"]),
            ("ModB.esp", 0, &["Skyrim.esm"]),
            ("Extra.esp", 0, &["Skyrim.esm"]),
        ],
        ccc: None,
        plugins: "Skyrim.esm\r\nModA.esp\r\nModB.esp\r\n",
        loadorder: Some("Skyrim.esm\r\nModA.esp\r\nExtra.esp\r\nModB.esp\r\n"),
    },
    ModuleCase {
        name: "missing-masters",
        mode: "FO4",
        modules: &[
            ("Fallout4.esm", 0x1, &[]),
            ("Good.esp", 0, &["Fallout4.esm"]),
            ("Broken.esp", 0, &["Missing.esm"]),
        ],
        ccc: None,
        plugins: "*Good.esp\r\n*Broken.esp\r\n",
        loadorder: None,
    },
];

/// A `TES4` header record whose `MAST` subrecords name `masters`. An empty
/// plugin the release and the port load the same.
fn plugin_bytes(flags: u32, masters: &[&str]) -> Vec<u8> {
    fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut bytes = signature.to_vec();
        bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    }
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&0u32.to_le_bytes());
    hedr.extend_from_slice(&0x0000_0800u32.to_le_bytes());
    let mut data = sub_record(b"HEDR", &hedr);
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        data.extend(sub_record(b"MAST", &name));
        data.extend(sub_record(b"DATA", &[0u8; 8]));
    }
    let mut bytes = b"TES4".to_vec();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&44u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend(data);
    bytes
}

fn check_module_case(
    runner: DiscoveryRunner,
    game: &'static Game,
    case: &'static ModuleCase,
    options: &Options,
) -> Result<DiscoveryOutcome> {
    let mut outcome = DiscoveryOutcome {
        game: game.name,
        case: case.name,
        status: "different",
        detail: None,
        lines: Vec::new(),
        loaded: Vec::new(),
        oracle_exit_code: None,
        port_exit_code: None,
    };
    let exe = {
        let exe = std::env::var_os("XEDIT_ORACLE_DIR")
            .map(PathBuf::from)
            .map(|dir| dir.join(gui::exe_name(case.mode)))
            .context("environment variable XEDIT_ORACLE_DIR is not set")?;
        ensure!(exe.exists(), "{} does not exist", exe.display());
        exe
    };
    // The plugin files the GUI's `-D:` folder takes, dated in list order by
    // the GUI's run folder.
    let files_dir = runner
        .scratch
        .join(format!("discovery-case-{}", case.name))
        .join(case.mode);
    let _ = fs::remove_dir_all(&files_dir);
    fs::create_dir_all(&files_dir)?;
    let mut plugins = Vec::new();
    for (name, flags, masters) in case.modules {
        let path = files_dir.join(name);
        fs::write(&path, plugin_bytes(*flags, masters))?;
        plugins.push(path);
    }
    let mut extras = GuiExtras::default();
    extras
        .files
        .push((PathBuf::from("plugins.txt"), case.plugins.as_bytes().to_vec()));
    if let Some(loadorder) = case.loadorder {
        extras
            .files
            .push((PathBuf::from("loadorder.txt"), loadorder.as_bytes().to_vec()));
    }
    if let Some((name, text)) = case.ccc {
        extras.files.push((PathBuf::from(name), text.as_bytes().to_vec()));
    }
    let oracle_work = runner
        .scratch
        .join(format!("discovery-oracle-{}-{}", case.mode, case.name));
    let run = GuiRun {
        exe: &exe,
        mode: case.mode,
        star_plugins_txt: !gui::simple_plugins_txt(case.mode),
        plugins: plugins.clone(),
        script: DISCOVERY_SCRIPT.to_owned(),
        build_refs: false,
        work: oracle_work.clone(),
        timeout: options.oracle_timeout.unwrap_or(Duration::from_secs(30 * 60)),
        hang_timeout: Duration::from_secs(600),
        budget: &crate::memory::Budget::new(runner.max_memory),
        expected_peak: 4 * super::GIB,
        max_memory: runner.max_memory,
        extra_args: vec!["-autoload".to_owned()],
        discovery: false,
        tool_name: "Script".to_owned(),
    };
    let result = run.run_script("oracle.pas", &extras, None)?;
    ensure!(result.ended, "the oracle's script did not end: {:?}", result.dialogs);
    let oracle_lines = compared_lines(&result.log, &oracle_work);
    let oracle_loaded = loaded_files(&result.log);
    let Some(port) = &runner.port else {
        outcome.status = "oracle-only";
        outcome.lines = oracle_lines;
        outcome.loaded = oracle_loaded;
        gui::remove_work(&oracle_work);
        let _ = fs::remove_dir_all(&files_dir);
        return Ok(outcome);
    };
    // The port: the same data folder, plugin list, loadorder.txt and
    // Creation Club list, in a run folder of its own.
    let port_work = runner
        .scratch
        .join(format!("discovery-port-{}-{}", case.mode, case.name));
    let bin = port_work.join("bin");
    let data = port_work.join("Data");
    for dir in [
        &bin,
        &data,
        &port_work.join("temp"),
        &port_work.join("cache"),
        &port_work.join("backup"),
    ] {
        fs::create_dir_all(dir)?;
    }
    fs::create_dir_all(port_work.join("mygames\\Saves"))?;
    fs::write(port_work.join("mygames\\game.ini"), "")?;
    for (name, flags, masters) in case.modules {
        fs::write(data.join(name), plugin_bytes(*flags, masters))?;
    }
    fs::write(port_work.join("plugins.txt"), case.plugins)?;
    if let Some(loadorder) = case.loadorder {
        fs::write(port_work.join("loadorder.txt"), loadorder)?;
    }
    if let Some((name, text)) = case.ccc {
        fs::write(port_work.join(name), text)?;
    }
    fs::write(port_work.join("oracle.pas"), DISCOVERY_SCRIPT)?;
    let port_exe = bin.join(port.file_name().context("the port binary without a name")?);
    fs::copy(port, &port_exe)?;
    let work_text = format!("{}\\", port_work.display());
    let args: Vec<String> = [
        format!("-{}", case.mode),
        "-autoload".to_owned(),
        "-nobuildrefs".to_owned(),
        format!("-script:{work_text}oracle.pas"),
        format!("-D:{work_text}Data\\"),
        format!("-P:{work_text}plugins.txt"),
        format!("-T:{work_text}temp\\"),
        format!("-C:{work_text}cache\\"),
        format!("-B:{work_text}backup\\"),
        format!("-S:{work_text}"),
        format!("-M:{work_text}mygames\\"),
        format!("-I:{work_text}mygames\\game.ini"),
        format!("-G:{work_text}mygames\\Saves\\"),
    ]
    .to_vec();
    let log_file = runner
        .scratch
        .join(format!("discovery-{}-{}.port.log", case.mode, case.name));
    let output = std::process::Command::new(&port_exe)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stderr(File::create(&log_file)?)
        .output()?;
    outcome.port_exit_code = output.status.code().map(|code| code as u32);
    let port_lines = compared_lines(&String::from_utf8_lossy(&output.stdout), &port_work);
    let port_loaded = loaded_files(&fs::read_to_string(&log_file).unwrap_or_default());
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
    if oracle_loaded != port_loaded {
        differences.push(format!(
            "  loaded files differ:\n    oracle {:?}\n    port   {:?}",
            oracle_loaded, port_loaded
        ));
    }
    outcome.lines = oracle_lines;
    outcome.loaded = oracle_loaded;
    if differences.is_empty() {
        outcome.status = "equal";
    } else {
        outcome.status = "different";
        outcome.detail = Some(differences.join("\n"));
    }
    gui::remove_work(&oracle_work);
    gui::remove_work(&port_work);
    let _ = fs::remove_dir_all(&files_dir);
    Ok(outcome)
}
