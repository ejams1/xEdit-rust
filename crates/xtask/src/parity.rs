// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Differential parity harness.
//!
//! `cargo xtask parity dump` runs the oracle `xDump.exe` and the port's
//! `xedit dump` on every corpus file and compares the two outputs byte for
//! byte. Oracle output is cached outside the repository, zstd-compressed and
//! keyed by the release tag, the game and a hash of the input file. The port
//! output is compared while it is produced and kept only when it differs.
//! Every process runs under a memory cap, and the processes that run at once
//! stay within a memory budget (`memory`).
//!
//! `cargo xtask parity roundtrip` loads every corpus plugin with the port and
//! saves it to the cache; the saved bytes are compared with the input file.
//! The oracle of that check is the input itself, which holds where xEdit
//! writes a file it loaded unchanged.
//!
//! `cargo xtask parity bsarch` packs, unpacks and lists the archives of the
//! game folders with `BSArch.exe` and with the port's `bsarch` and compares
//! the results (`bsarch`).
//!
//! `cargo xtask parity oracle-save` has the GUI build of xEdit save every
//! corpus plugin (`oracle_save`, `gui`) and compares the port's save with
//! it; the round trip counts a save that equals a cached oracle save as
//! equal. `cargo xtask parity oracle-edit` runs the scripted edit sequences
//! of `crates/xtask/oracle/edits` on both and compares the saved files.
//!
//! `cargo xtask parity nif` loads, dumps and saves the NIF and material
//! files of the corpus archives with the port and with `Sniff.exe` (`nif`).
//! `cargo xtask parity sniff` runs the operations of Sniff on them with the
//! port's `sniff` and with `Sniff.exe` and compares what they write and
//! report (`sniff`).
//!
//! Environment:
//!
//! - `XEDIT_ORACLE_DIR`: unpacked release archive of the baseline tag.
//! - `XEDIT_<GAME>_DATA` (`XEDIT_FO4_DATA`, `XEDIT_SSE_DATA`, ... see
//!   `GAMES`): `Data` directory of each game.
//! - `XEDIT_PARITY_CACHE`: cache directory. Defaults to the user cache directory.
//! - `XEDIT_PARITY_SCRATCH`: where the saves of the port and the files
//!   kept for a difference go, so that two branches can run the round
//!   trip side by side and share the oracle cache. Defaults to the cache.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

use crate::memory::{self, Budget, GIB, Limit};

mod bsarch;
mod check;
mod clean;
mod conflicts;
mod filter;
mod gui;
mod hidden;
mod lodgen;
mod merged;
mod modgroups;
mod nif;
mod oracle_refs;
mod oracle_save;
mod script;
mod sniff;
mod strings;
mod tool_modes;

/// `parity check-dump`: the dump check runs `xDump -check` and
/// `xedit dump --check` in place of the dumps, cached apart (`<MODE>-check`).
static CHECK_DUMP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check_dump() -> bool {
    CHECK_DUMP.load(Ordering::Relaxed)
}

/// A game whose masters are in the corpus.
struct Game {
    /// Name on the command line of `cargo xtask parity`.
    name: &'static str,
    /// Game mode switch of the oracle and of `xedit dump --game`.
    mode: &'static str,
    data_var: &'static str,
    /// Lower-case names of the vanilla plugins. A trailing `*` matches any rest.
    vanilla: &'static [&'static str],
    /// Extensions of the saves and co-saves the oracle dumps (`-saves`); the
    /// saves are in the folder of `XEDIT_<NAME>_SAVES`.
    save_extensions: &'static [&'static str],
}

const SKYRIM_MASTERS: [&str; 5] = [
    "skyrim.esm",
    "update.esm",
    "dawnguard.esm",
    "hearthfires.esm",
    "dragonborn.esm",
];

const GAMES: &[Game] = &[
    Game {
        name: "fo4",
        mode: "FO4",
        data_var: "XEDIT_FO4_DATA",
        save_extensions: &["fos", "f4se"],
        vanilla: &["fallout4.esm", "dlc*", "cc*"],
    },
    Game {
        name: "sse",
        mode: "SSE",
        data_var: "XEDIT_SSE_DATA",
        save_extensions: &["ess", "skse"],
        vanilla: &[
            SKYRIM_MASTERS[0],
            SKYRIM_MASTERS[1],
            SKYRIM_MASTERS[2],
            SKYRIM_MASTERS[3],
            SKYRIM_MASTERS[4],
            "_resourcepack.esl",
            "cc*",
        ],
    },
    Game {
        name: "tes3",
        mode: "TES3",
        data_var: "XEDIT_TES3_DATA",
        save_extensions: &[],
        vanilla: &["morrowind.esm", "tribunal.esm", "bloodmoon.esm"],
    },
    Game {
        name: "tes4",
        mode: "TES4",
        data_var: "XEDIT_TES4_DATA",
        save_extensions: &["ess", "obse"],
        vanilla: &["oblivion.esm", "dlc*", "knights.esp"],
    },
    Game {
        name: "fo3",
        mode: "FO3",
        data_var: "XEDIT_FO3_DATA",
        save_extensions: &["fos", "fose"],
        vanilla: &[
            "fallout3.esm",
            "anchorage.esm",
            "thepitt.esm",
            "brokensteel.esm",
            "pointlookout.esm",
            "zeta.esm",
        ],
    },
    Game {
        name: "fnv",
        mode: "FNV",
        data_var: "XEDIT_FNV_DATA",
        save_extensions: &["fos", "nvse"],
        vanilla: &[
            "falloutnv.esm",
            "deadmoney.esm",
            "honesthearts.esm",
            "oldworldblues.esm",
            "lonesomeroad.esm",
            "gunrunnersarsenal.esm",
            "caravanpack.esm",
            "classicpack.esm",
            "mercenarypack.esm",
            "tribalpack.esm",
        ],
    },
    Game {
        name: "tes5",
        mode: "TES5",
        data_var: "XEDIT_TES5_DATA",
        save_extensions: &["ess", "skse"],
        vanilla: &SKYRIM_MASTERS,
    },
    Game {
        name: "tes5vr",
        mode: "TES5VR",
        data_var: "XEDIT_TES5VR_DATA",
        save_extensions: &[],
        vanilla: &[
            SKYRIM_MASTERS[0],
            SKYRIM_MASTERS[1],
            SKYRIM_MASTERS[2],
            SKYRIM_MASTERS[3],
            SKYRIM_MASTERS[4],
            "skyrimvr.esm",
        ],
    },
    Game {
        name: "fo4vr",
        mode: "FO4VR",
        data_var: "XEDIT_FO4VR_DATA",
        save_extensions: &[],
        vanilla: &["fallout4.esm", "fallout4_vr.esm"],
    },
    Game {
        name: "fo76",
        mode: "FO76",
        data_var: "XEDIT_FO76_DATA",
        save_extensions: &[],
        vanilla: &["seventysix.esm", "nw.esm"],
    },
    Game {
        name: "sf1",
        mode: "SF1",
        data_var: "XEDIT_SF1_DATA",
        save_extensions: &[],
        vanilla: &[
            "starfield.esm",
            "constellation.esm",
            "oldmars.esm",
            "blueprintships-*",
            "sfbgs*",
            "shatteredspace.esm",
        ],
    },
];

const PLUGIN_EXTENSIONS: &[&str] = &["esm", "esl", "esp"];

struct Options {
    /// `parity saves`: the saves of the games instead of their plugins.
    saves: bool,
    /// `parity roundtrip`: load and save each plugin, compare with the input.
    roundtrip: bool,
    /// `parity oracle-save`: compare the port's save with the GUI oracle's.
    oracle_save: bool,
    /// `parity oracle-edit`: the scripted edit sequences.
    oracle_edit: bool,
    /// `parity conflicts`: the conflict status of every record.
    conflicts: bool,
    /// `parity clean`: the quick auto clean mode on every plugin with
    /// masters.
    clean: bool,
    /// `parity check`: the `-CheckForErrors` mode on every plugin.
    check: bool,
    /// `parity conflicts --record <FormID>`: the oracle's probe of these
    /// records instead of the check.
    records: Vec<String>,
    /// `parity refs`: the reference index of each game's corpus.
    refs: bool,
    /// `parity modgroups`: the mod group scenarios.
    modgroups: bool,
    /// `parity merged`: the merged patch scenarios.
    merged: bool,
    /// `parity tool-modes`: the tool modes that work over the loaded files.
    tool_modes: bool,
    /// `parity filter`: the navigation tree filter scenarios.
    filter: bool,
    games: Vec<&'static Game>,
    /// Lower-case file names. Empty selects the whole corpus.
    files: Vec<String>,
    /// `parity tool-modes`: the tool modes to run; empty selects all.
    modes: Vec<String>,
    oracle_only: bool,
    jobs: usize,
    /// No `--game` was given.
    all_games: bool,
    /// Sum of the expected peaks of the processes that run at once.
    memory_budget: Option<u64>,
    /// Cap on the committed memory of one process.
    max_memory: Option<u64>,
    /// `--oracle-timeout`: the oracle is stopped after this long and its
    /// output so far is kept as a prefix.
    oracle_timeout: Option<Duration>,
}

/// One corpus file and where its outputs go.
struct Case {
    game: &'static Game,
    input: PathBuf,
    name: String,
    /// The data folder: the folder of a plugin, the game data of a save.
    data: PathBuf,
    saves: bool,
}

/// What every check shares.
struct Runner {
    oracle: PathBuf,
    port: Option<PathBuf>,
    cache: PathBuf,
    /// `XEDIT_PARITY_SCRATCH` (with the tag): the port's saves.
    scratch: PathBuf,
    /// `XEDIT_ORACLE_DIR`, for the GUI builds; empty when it is not set.
    oracle_dir: PathBuf,
    /// Content hashes of the masters, which many cases share.
    hashes: Mutex<std::collections::HashMap<PathBuf, u64>>,
    budget: Budget,
    max_memory: u64,
    oracle_timeout: Option<Duration>,
}

/// Expected peak of a dump whose peak was never measured, as a multiple of
/// the size of the plugin and its game master. Measured for the port:
/// `Fallout4.esm` 4.7 times, `DLCCoast.esm` with `Fallout4.esm` 3.9 times.
const UNMEASURED_PEAK_FACTOR: u64 = 6;

#[derive(Serialize)]
struct Outcome {
    game: &'static str,
    file: String,
    /// `equal`, `equal-prefix` (the oracle crashed and the port matches its
    /// output up to the crash), `equal-error` (the oracle stopped with an
    /// exception and the port stopped with the same message after the same
    /// output), `different`, `oracle-failed`, `port-failed` or `oracle-only`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    oracle_bytes: u64,
    port_bytes: u64,
    /// Peak committed memory of the process in this run, in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    oracle_peak: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_peak: Option<u64>,
}

#[derive(Serialize)]
struct Report<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    outcomes: &'a [Outcome],
}

pub fn run(root: &Path, tag: &str, args: &[&str]) -> Result<()> {
    // `parity bsarch` has its own options and cases: the archive tool against BSArch.exe.
    if let Some((&"bsarch", rest)) = args.split_first() {
        return bsarch::run(root, tag, rest);
    }
    if let Some((&"nif", rest)) = args.split_first() {
        return nif::run(root, tag, rest);
    }
    if let Some((&"sniff", rest)) = args.split_first() {
        return sniff::run(tag, rest);
    }
    if let Some((&"lodgen", rest)) = args.split_first() {
        return lodgen::run(root, tag, rest);
    }
    if let Some((&"script", rest)) = args.split_first() {
        return script::run(root, tag, rest);
    }
    let options = parse(args)?;
    // The round trip has no oracle binary: the input file is the oracle
    // (and the GUI oracle's saves, when they are cached).
    let oracle_dir = std::env::var_os("XEDIT_ORACLE_DIR").map(PathBuf::from);
    let gui_oracle = options.oracle_save
        || options.oracle_edit
        || options.conflicts
        || options.refs
        || options.clean
        || options.check
        || options.modgroups
        || options.merged
        || options.tool_modes;
    let oracle = if options.roundtrip || gui_oracle {
        PathBuf::new()
    } else {
        let oracle = PathBuf::from(required_var("XEDIT_ORACLE_DIR")?).join("xDump.exe");
        ensure!(oracle.exists(), "{} does not exist", oracle.display());
        oracle
    };
    if gui_oracle {
        ensure!(oracle_dir.is_some(), "environment variable XEDIT_ORACLE_DIR is not set");
    }
    let oracle_dir = oracle_dir.unwrap_or_default();
    let cache = cache_dir()?.join(tag);
    let scratch = match std::env::var_os("XEDIT_PARITY_SCRATCH") {
        Some(dir) => PathBuf::from(dir).join(tag),
        None => cache.clone(),
    };
    if options.oracle_edit {
        return oracle_save::run_edits(root, tag, &options, cache, scratch, oracle_dir);
    }
    if options.conflicts {
        return conflicts::run_conflicts(root, tag, &options, cache, scratch, oracle_dir);
    }
    if options.refs {
        return oracle_refs::run_refs(root, tag, &options, cache, scratch, oracle_dir);
    }
    if options.clean {
        return clean::run_clean(root, tag, &options, cache, scratch, oracle_dir);
    }
    if options.check {
        return check::run_check(root, tag, &options, cache, scratch, oracle_dir);
    }
    if options.modgroups {
        return modgroups::run_modgroups(root, tag, &options, cache, scratch, oracle_dir);
    }
    if options.merged {
        return merged::run_merged(root, tag, &options, cache, scratch, oracle_dir);
    }
    if options.tool_modes {
        return tool_modes::run_tool_modes(root, tag, &options, cache, scratch, oracle_dir);
    }
    if options.filter {
        return filter::run_filter(root, tag, &options, cache, scratch, oracle_dir);
    }

    let mut cases = Vec::new();
    for &game in &options.games {
        // Without `--game`, the games whose data directory is not set are
        // skipped and named, so a partial run is visible.
        let data = match (std::env::var_os(game.data_var), options.all_games) {
            (Some(data), _) => PathBuf::from(data),
            (None, true) => {
                println!("skipped       {}: {} is not set", game.name, game.data_var);
                continue;
            }
            (None, false) => bail!("environment variable {} is not set", game.data_var),
        };
        let before = cases.len();
        if options.saves {
            let var = format!("XEDIT_{}_SAVES", game.name.to_uppercase());
            let saves = match (std::env::var_os(&var), options.all_games) {
                (Some(saves), _) => PathBuf::from(saves),
                (None, true) => {
                    println!("skipped       {}: {var} is not set", game.name);
                    continue;
                }
                (None, false) => bail!("environment variable {var} is not set"),
            };
            for entry in fs::read_dir(&saves).with_context(|| format!("reading {}", saves.display()))? {
                let input = entry?.path();
                let name = input.file_name().unwrap().to_string_lossy().into_owned();
                let is_save = input
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| game.save_extensions.iter().any(|x| e.eq_ignore_ascii_case(x)));
                if is_save && (options.files.is_empty() || options.files.contains(&name.to_lowercase())) {
                    cases.push(Case {
                        game,
                        input,
                        name,
                        data: data.clone(),
                        saves: true,
                    });
                }
            }
            ensure!(cases.len() > before, "no save found in {}", saves.display());
            continue;
        }
        for entry in fs::read_dir(&data).with_context(|| format!("reading {}", data.display()))? {
            let input = entry?.path();
            let name = input.file_name().unwrap().to_string_lossy().into_owned();
            let lower = name.to_lowercase();
            let is_plugin = input
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| PLUGIN_EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)));
            let selected = if options.files.is_empty() {
                is_vanilla(game, &lower)
            } else {
                options.files.contains(&lower)
            };
            if is_plugin && selected {
                cases.push(Case {
                    game,
                    input,
                    name,
                    data: data.clone(),
                    saves: false,
                });
            }
        }
        // `--file` without `--game` looks in every game.
        ensure!(
            cases.len() > before || (options.all_games && !options.files.is_empty()),
            "no corpus file found in {}",
            data.display()
        );
    }
    ensure!(!cases.is_empty(), "no corpus file selected");
    // Largest first, so that the long runs start early.
    cases.sort_by_key(|case| std::cmp::Reverse(fs::metadata(&case.input).map(|m| m.len()).unwrap_or(0)));

    let port = if options.oracle_only {
        None
    } else {
        Some(build_port(root)?)
    };
    // Three quarters of the installed memory leaves room for the rest of
    // the machine.
    let budget = options
        .memory_budget
        .or_else(|| memory::physical_memory().map(|total| total / 4 * 3))
        .unwrap_or(24 * GIB);
    let runner = Runner {
        oracle,
        port,
        cache,
        scratch,
        oracle_dir,
        hashes: Mutex::new(std::collections::HashMap::new()),
        budget: Budget::new(budget),
        max_memory: options.max_memory.unwrap_or(budget),
        oracle_timeout: options.oracle_timeout,
    };
    println!(
        "memory budget {:.1} GiB, at most {:.1} GiB per process",
        budget as f64 / GIB as f64,
        runner.max_memory as f64 / GIB as f64
    );

    let next = AtomicUsize::new(0);
    let outcomes = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.min(cases.len()) {
            scope.spawn(|| {
                while let Some(case) = cases.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let checked = if options.roundtrip {
                        check_roundtrip(case, &runner)
                    } else if options.oracle_save {
                        oracle_save::check(case, &runner)
                    } else {
                        check(case, &runner)
                    };
                    let outcome = checked.unwrap_or_else(|error| Outcome {
                        game: case.game.name,
                        file: case.name.clone(),
                        status: "oracle-failed",
                        detail: Some(format!("{error:#}")),
                        oracle_bytes: 0,
                        port_bytes: 0,
                        oracle_peak: None,
                        port_peak: None,
                    });
                    println!("{:13} {} {}", outcome.status, outcome.game, outcome.file);
                    if let Some(detail) = &outcome.detail {
                        println!("{detail}");
                    }
                    outcomes.lock().unwrap().push(outcome);
                }
            });
        }
    });

    let mut outcomes = outcomes.into_inner().unwrap();
    outcomes.sort_by(|a, b| (a.game, &a.file).cmp(&(b.game, &b.file)));
    // A save the port refuses with an upstream message is as good as equal:
    // the oracle refuses the same file with the same text.
    let equal = outcomes
        .iter()
        .filter(|o| matches!(o.status, "equal" | "equal-prefix" | "equal-error" | "refused"))
        .count();
    let report = Report {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_name = if options.roundtrip {
        "roundtrip.json"
    } else if options.oracle_save {
        "oracle-save.json"
    } else if options.saves {
        "saves.json"
    } else if check_dump() {
        "check-dump.json"
    } else {
        "dump.json"
    };
    let report_file = report_dir.join(report_name);
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    if options.roundtrip {
        let count = |status: &str| outcomes.iter().filter(|o| o.status == status).count();
        println!(
            "{} equal, {} refused, {} ofst-dropped, {} header-edited, {} structure-edited, {} unsupported, {} different, {} failed of {}. Report: {}",
            count("equal"),
            count("refused"),
            count("ofst-dropped"),
            count("header-edited"),
            count("structure-edited"),
            count("unsupported"),
            count("different"),
            count("port-failed") + count("port-memory-limit"),
            outcomes.len(),
            report_file.display()
        );
    } else if options.oracle_save {
        let count = |status: &str| outcomes.iter().filter(|o| o.status == status).count();
        println!(
            "{} equal, {} equal-error, {} different, {} oracle-unsupported, {} oracle-failed, {} port-failed of {}. Report: {}",
            count("equal"),
            count("equal-error"),
            count("different"),
            count("oracle-unsupported"),
            count("oracle-failed"),
            count("port-failed") + count("port-memory-limit"),
            outcomes.len(),
            report_file.display()
        );
    } else {
        println!("{equal} of {} equal. Report: {}", outcomes.len(), report_file.display());
    }
    if !options.oracle_only {
        ensure!(equal == outcomes.len(), "parity does not hold");
    }
    Ok(())
}

/// How a save of the port ended.
enum PortSave {
    /// The saved file is written.
    Written,
    /// The port refused the save with an upstream message (`save_refused`).
    Refused(String),
    /// Anything else; the outcome says what.
    Stopped,
}

/// Has the port load `case` and save it to `saved`, as the round trip and
/// the oracle save do. A save that is not written sets the status and the
/// detail of `outcome` (`refused`, `unsupported`, `port-failed`,
/// `port-memory-limit` or `oracle-only` without a port).
fn port_save(case: &Case, runner: &Runner, saved: &Path, port_log: &Path, outcome: &mut Outcome) -> Result<PortSave> {
    let Some(port) = &runner.port else {
        outcome.status = "oracle-only";
        return Ok(PortSave::Stopped);
    };
    let _ = fs::remove_file(saved);
    let mut command = Command::new(port);
    command
        .args(["--json", "--edit", "--game", case.game.mode, "--load"])
        .arg(&case.input)
        .args(["save", "--no-backup", "--output"])
        .arg(saved)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(File::create(port_log)?);
    let peak_file = port_log.with_extension("peak");
    let (status, result, reached, peak) = run_limited(runner, case, &mut command, &peak_file, |child| {
        let mut text = String::new();
        child.stdout.take().unwrap().read_to_string(&mut text)?;
        Ok((text, true))
    })?;
    outcome.port_peak = peak;
    let text = result?;
    outcome.status = "port-failed";
    if reached && !status.success() {
        outcome.status = "port-memory-limit";
        outcome.detail = Some(format!(
            "  reached the cap of {:.1} GiB (--max-memory), see {}",
            runner.max_memory as f64 / GIB as f64,
            port_log.display()
        ));
        return Ok(PortSave::Stopped);
    }
    let envelope: serde_json::Value = match serde_json::from_str(text.trim()) {
        Ok(value) => value,
        Err(_) => {
            outcome.detail = Some(format!("  {status}, see {}", port_log.display()));
            return Ok(PortSave::Stopped);
        }
    };
    if envelope["ok"] != serde_json::Value::Bool(true) {
        let code = envelope["error"]["code"].as_str().unwrap_or("");
        let message = envelope["error"]["message"].as_str().unwrap_or("");
        outcome.status = match code {
            "save_refused" => "refused",
            "unsupported" => "unsupported",
            _ => "port-failed",
        };
        outcome.detail = Some(format!("  {code}: {message}"));
        return Ok(if code == "save_refused" {
            PortSave::Refused(message.to_owned())
        } else {
            PortSave::Stopped
        });
    }
    if envelope["result"]["written"] != serde_json::Value::Bool(true) {
        outcome.detail = Some(format!("  the port reported no written file: {}", text.trim()));
        return Ok(PortSave::Stopped);
    }
    outcome.port_bytes = fs::metadata(saved)?.len();
    Ok(PortSave::Written)
}

/// The round trip of one plugin: the port loads it and saves it into the
/// cache, and the saved bytes are compared with the input. `equal`,
/// `ofst-dropped` (the input without the offsets of its worldspaces, which
/// upstream drops too), `header-edited` (the same bytes after the file
/// header record, whose subrecords upstream edits on save: the `HEDR`
/// record count, `INCC`, the `ONAM` list, the flags), `different` (with the
/// first differing offset), `refused` (the port refused the save with an
/// upstream message, as the oracle would), `unsupported` (a save upstream
/// edits in a way the port cannot yet), `port-failed` or
/// `port-memory-limit`.
fn check_roundtrip(case: &Case, runner: &Runner) -> Result<Outcome> {
    let dir = runner.scratch.join(format!("{}-roundtrip", case.game.mode));
    fs::create_dir_all(&dir)?;
    let stem = format!("{}.{:016x}", case.name, content_hash(&case.input)?);
    let mut outcome = Outcome {
        game: case.game.name,
        file: case.name.clone(),
        status: "port-failed",
        detail: None,
        oracle_bytes: fs::metadata(&case.input)?.len(),
        port_bytes: 0,
        oracle_peak: None,
        port_peak: None,
    };
    let saved = dir.join(format!("{stem}.saved"));
    let port_log = dir.join(format!("{stem}.port.log"));
    match port_save(case, runner, &saved, &port_log, &mut outcome)? {
        PortSave::Written => {}
        PortSave::Stopped => return Ok(outcome),
        // A refusal is confirmed by the oracle's refusal with the same
        // message, when it is cached.
        PortSave::Refused(message) => {
            match oracle_save::cached(case, runner)? {
                Some(oracle_save::OracleSave::Error(expected)) if expected == message => {
                    outcome.detail = Some(format!("  the oracle refused the save too: {message}"));
                }
                Some(oracle_save::OracleSave::Error(expected)) => {
                    outcome.status = "different";
                    outcome.detail = Some(format!(
                        "  the oracle refused with: {expected}
  the port refused with:   {message}"
                    ));
                }
                Some(oracle_save::OracleSave::Saved(path)) => {
                    outcome.status = "different";
                    outcome.detail = Some(format!(
                        "  the port refused the save ({message}), the oracle saved it: {}",
                        path.display()
                    ));
                }
                None => {}
            }
            return Ok(outcome);
        }
    }
    // The GUI oracle's own save, when `parity oracle-save` has cached it,
    // decides: a save that equals it is equal whatever it changed in the
    // input, and one that differs from it is different.
    if let Some(oracle) = oracle_save::cached(case, runner)? {
        outcome.port_bytes = fs::metadata(&saved)?.len();
        let (status, detail) = oracle_save::compare(case, &oracle, &saved, &dir, &stem)?;
        outcome.status = status;
        outcome.detail = Some(detail);
        if status == "equal" {
            fs::remove_file(&saved)?;
        }
        return Ok(outcome);
    }
    outcome.port_bytes = fs::metadata(&saved)?.len();
    match first_byte_difference(&case.input, &saved)? {
        None => {
            outcome.status = "equal";
            fs::remove_file(&saved)?;
        }
        Some(offset) => {
            // xEdit drops the OFST subrecord of every worldspace it loads, so
            // a file with one saves smaller. A saved file that is the input
            // without those subrecords is what upstream writes, which the
            // oracle save of a later step confirms.
            let input = fs::read(&case.input)?;
            let saved_bytes = fs::read(&saved)?;
            let (expected, dropped) = match without_wrld_ofst(case, &input)? {
                Some((expected, dropped)) => (expected, dropped),
                None => (input, 0),
            };
            let ofst_note = if dropped > 0 {
                format!(" and without the OFST data of its worldspaces ({dropped} bytes)")
            } else {
                String::new()
            };
            if expected == saved_bytes {
                outcome.status = "ofst-dropped";
                outcome.detail = Some(format!(
                    "  the saved file is the input without the OFST data of its worldspaces ({dropped} bytes)"
                ));
                fs::remove_file(&saved)?;
                return Ok(outcome);
            }
            // The edits upstream makes to the file header on save leave the
            // rest of the file as it is.
            let header_size = main_record_header_size(case.game.mode);
            if let (Some(expected_body), Some(saved_body)) = (
                body_after_header(&expected, header_size),
                body_after_header(&saved_bytes, header_size),
            ) && expected_body == saved_body
            {
                outcome.status = "header-edited";
                outcome.detail = Some(format!(
                    "  the saved file is the input with the file header edited{ofst_note}: {}",
                    describe_header_edit(&expected, &saved_bytes, header_size)
                ));
                fs::remove_file(&saved)?;
                return Ok(outcome);
            }
            let (records_differ, record_report) = describe_record_differences(&expected, &saved_bytes, header_size);
            if !records_differ {
                // Every record is the input's: the file header and the
                // groups changed (an empty top level group dropped,
                // duplicated groups merged), as upstream changes them on
                // load.
                outcome.status = "structure-edited";
                outcome.detail = Some(format!(
                    "  every record is the input's{ofst_note}; the file header and the groups changed: {}; {}",
                    describe_header_edit(&expected, &saved_bytes, header_size),
                    describe_group_structure(&expected, &saved_bytes, header_size)
                ));
                fs::remove_file(&saved)?;
                return Ok(outcome);
            }
            outcome.status = "different";
            outcome.detail = Some(format!(
                "  first difference at byte {offset} (0x{offset:X}); input {} bytes, saved {} bytes\n  {record_report}\n  saved:  {}\n  log:    {}",
                outcome.oracle_bytes,
                outcome.port_bytes,
                saved.display(),
                port_log.display()
            ));
        }
    }
    Ok(outcome)
}

/// The main records of a plugin by FormID: the signature and the bytes of
/// each, found by walking the groups. A record that appears twice keeps
/// its first occurrence.
fn records_by_form_id(bytes: &[u8], header_size: usize) -> std::collections::HashMap<u32, (String, &[u8])> {
    fn walk<'a>(
        buf: &'a [u8],
        start: usize,
        end: usize,
        header_size: usize,
        out: &mut std::collections::HashMap<u32, (String, &'a [u8])>,
    ) -> Option<()> {
        let mut pos = start;
        while pos + header_size <= end {
            let signature = buf.get(pos..pos + 4)?;
            let size = u32::from_le_bytes(buf.get(pos + 4..pos + 8)?.try_into().ok()?) as usize;
            if signature == b"GRUP" {
                walk(
                    buf,
                    pos + header_size.clamp(20, 24),
                    (pos + size).min(end),
                    header_size,
                    out,
                )?;
                pos += size;
            } else {
                let form_id = u32::from_le_bytes(buf.get(pos + 12..pos + 16)?.try_into().ok()?);
                let record = buf.get(pos..pos + header_size + size)?;
                out.entry(form_id)
                    .or_insert_with(|| (String::from_utf8_lossy(signature).into_owned(), record));
                pos += header_size + size;
            }
        }
        Some(())
    }
    let mut out = std::collections::HashMap::new();
    // The group header is 24 bytes for every game but Oblivion (20) and
    // Morrowind, whose groups do not exist.
    let data_size = u32::from_le_bytes(bytes.get(4..8).map(|b| b.try_into().unwrap()).unwrap_or([0; 4])) as usize;
    let _ = walk(bytes, header_size + data_size, bytes.len(), header_size, &mut out);
    out
}

/// The number of groups of a plugin and of the empty ones among them.
fn describe_group_structure(input: &[u8], saved: &[u8], header_size: usize) -> String {
    fn count(bytes: &[u8], header_size: usize) -> (usize, usize) {
        fn walk(buf: &[u8], start: usize, end: usize, group_header: usize, counts: &mut (usize, usize)) -> Option<()> {
            let mut pos = start;
            while pos + group_header <= end {
                let signature = buf.get(pos..pos + 4)?;
                let size = u32::from_le_bytes(buf.get(pos + 4..pos + 8)?.try_into().ok()?) as usize;
                if signature == b"GRUP" {
                    counts.0 += 1;
                    if size == group_header {
                        counts.1 += 1;
                    }
                    walk(buf, pos + group_header, (pos + size).min(end), group_header, counts)?;
                    pos += size;
                } else {
                    pos += group_header + size;
                }
            }
            Some(())
        }
        let mut counts = (0, 0);
        let data_size = u32::from_le_bytes(bytes.get(4..8).map(|b| b.try_into().unwrap()).unwrap_or([0; 4])) as usize;
        let _ = walk(
            bytes,
            header_size + data_size,
            bytes.len(),
            header_size.clamp(20, 24),
            &mut counts,
        );
        counts
    }
    let (before, before_empty) = count(input, header_size);
    let (after, after_empty) = count(saved, header_size);
    format!("groups {before} ({before_empty} empty) -> {after} ({after_empty} empty)")
}

/// What differs between the records of the input and of the saved file:
/// whether any does, and a report with how many changed, were added or
/// dropped, by signature, and the first few of each.
fn describe_record_differences(input: &[u8], saved: &[u8], header_size: usize) -> (bool, String) {
    if header_size < 20 {
        return (true, "record differences are not analysed for this game".to_owned());
    }
    let before = records_by_form_id(input, header_size);
    let after = records_by_form_id(saved, header_size);
    let mut changed: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut examples: Vec<String> = Vec::new();
    let mut changed_total = 0usize;
    let mut form_ids: Vec<&u32> = before.keys().collect();
    form_ids.sort();
    for form_id in form_ids {
        let (signature, old) = &before[form_id];
        if let Some((_, new)) = after.get(form_id)
            && old != new
        {
            changed_total += 1;
            *changed.entry(signature.clone()).or_default() += 1;
            if examples.len() < 5 {
                examples.push(format!(
                    "{signature} [{form_id:08X}] {} -> {} bytes",
                    old.len(),
                    new.len()
                ));
            }
        }
    }
    let mut dropped: Vec<String> = before
        .iter()
        .filter(|(form_id, _)| !after.contains_key(form_id))
        .map(|(form_id, (signature, _))| format!("{signature} [{form_id:08X}]"))
        .collect();
    dropped.sort();
    let mut added: Vec<String> = after
        .iter()
        .filter(|(form_id, _)| !before.contains_key(form_id))
        .map(|(form_id, (signature, _))| format!("{signature} [{form_id:08X}]"))
        .collect();
    added.sort();
    let by_signature: Vec<String> = changed
        .iter()
        .map(|(signature, count)| format!("{signature} {count}"))
        .collect();
    let differs = changed_total > 0 || !dropped.is_empty() || !added.is_empty();
    let report = format!(
        "records changed: {changed_total} ({}), dropped: {} ({}), added: {} ({}); first changed: {}",
        by_signature.join(", "),
        dropped.len(),
        dropped.iter().take(3).cloned().collect::<Vec<_>>().join(", "),
        added.len(),
        added.iter().take(3).cloned().collect::<Vec<_>>().join(", "),
        examples.join("; ")
    );
    (differs, report)
}

/// The size of the main record header of a game (`wbSizeOfMainRecordStruct`).
fn main_record_header_size(mode: &str) -> usize {
    match mode.to_ascii_lowercase().as_str() {
        "tes3" => 16,
        "tes4" => 20,
        _ => 24,
    }
}

/// The bytes after the file header record: from the first group on.
fn body_after_header(bytes: &[u8], header_size: usize) -> Option<&[u8]> {
    let data_size = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    bytes.get(header_size + data_size..)
}

/// The subrecords of the file header record as `(signature, data)`.
fn header_sub_records(bytes: &[u8], header_size: usize) -> Vec<(String, Vec<u8>)> {
    let mut result = Vec::new();
    let Some(data_size) = bytes
        .get(4..8)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
    else {
        return result;
    };
    let Some(data) = bytes.get(header_size..header_size + data_size) else {
        return result;
    };
    let sub_header = if header_size == 16 { 8 } else { 6 };
    let mut pos = 0;
    while pos + sub_header <= data.len() {
        let signature = String::from_utf8_lossy(&data[pos..pos + 4]).into_owned();
        let size = if sub_header == 8 {
            u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize
        } else {
            u16::from_le_bytes([data[pos + 4], data[pos + 5]]) as usize
        };
        let end = (pos + sub_header + size).min(data.len());
        result.push((signature, data[pos + sub_header..end].to_vec()));
        pos = end;
    }
    result
}

/// What changed in the file header record: the flags, and the subrecords
/// added, removed or changed, with the values that matter.
fn describe_header_edit(input: &[u8], saved: &[u8], header_size: usize) -> String {
    let mut notes = Vec::new();
    let flags_at = if header_size == 16 { 12 } else { 8 };
    let flags = |bytes: &[u8]| {
        bytes
            .get(flags_at..flags_at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    if let (Some(before), Some(after)) = (flags(input), flags(saved))
        && before != after
    {
        notes.push(format!("flags {before:08X} -> {after:08X}"));
    }
    let before = header_sub_records(input, header_size);
    let after = header_sub_records(saved, header_size);
    let describe = |signature: &str, data: &[u8]| -> String {
        match signature {
            "HEDR" => format!(
                "{} records",
                data.get(4..8)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                    .unwrap_or(0)
            ),
            "INCC" => format!(
                "{} interior cells",
                data.get(..4)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                    .unwrap_or(0)
            ),
            "ONAM" => format!("{} entries", data.len() / 4),
            _ => format!("{} bytes", data.len()),
        }
    };
    let count = |list: &[(String, Vec<u8>)], signature: &str| list.iter().filter(|(s, _)| s == signature).count();
    let mut signatures: Vec<String> = before.iter().chain(&after).map(|(s, _)| s.clone()).collect();
    signatures.sort();
    signatures.dedup();
    for signature in signatures {
        let in_before = count(&before, &signature);
        let in_after = count(&after, &signature);
        let value_before = before
            .iter()
            .find(|(s, _)| *s == signature)
            .map(|(_, d)| describe(&signature, d));
        let value_after = after
            .iter()
            .find(|(s, _)| *s == signature)
            .map(|(_, d)| describe(&signature, d));
        match (in_before, in_after) {
            (0, _) => notes.push(format!("{signature} added ({})", value_after.unwrap_or_default())),
            (_, 0) => notes.push(format!("{signature} removed ({})", value_before.unwrap_or_default())),
            _ => {
                let same = before
                    .iter()
                    .filter(|(s, _)| *s == signature)
                    .map(|(_, d)| d)
                    .eq(after.iter().filter(|(s, _)| *s == signature).map(|(_, d)| d));
                if !same {
                    notes.push(format!(
                        "{signature} {} -> {}",
                        value_before.unwrap_or_default(),
                        value_after.unwrap_or_default()
                    ));
                }
            }
        }
    }
    if notes.is_empty() {
        "the header record changed".to_owned()
    } else {
        notes.join(", ")
    }
}

/// The input of `case` with the offset data upstream drops from every
/// uncompressed `WRLD` record on load removed, and the record and group
/// sizes adjusted, with the number of bytes dropped; `None` when the input
/// has none of it. That is the `OFST` subrecord (with its `XXXX` size
/// prefix) of `TwbMainRecord.Init` under `wbRemoveOffsetData`, and what
/// `wbWorldAfterLoad` removes: `CLSZ` for Fallout 4, Fallout 76 and
/// Starfield, `VISI` for Fallout 76, and the `RNAM` large references of
/// the game master for Skyrim, Fallout 4 and Fallout 76. Only for the games
/// with 24 byte record headers; Oblivion and Morrowind worldspaces keep
/// their offsets.
fn without_wrld_ofst(case: &Case, input: &[u8]) -> Result<Option<(Vec<u8>, u64)>> {
    let mode = case.game.mode.to_ascii_lowercase();
    if matches!(mode.as_str(), "tes3" | "tes4") {
        return Ok(None);
    }
    const HEADER: usize = 24;
    let mut dropped_signatures: Vec<&[u8; 4]> = vec![b"OFST"];
    if matches!(mode.as_str(), "fo4" | "fo4vr" | "fo76" | "sf1") {
        dropped_signatures.push(b"CLSZ");
    }
    if mode == "fo76" {
        dropped_signatures.push(b"VISI");
    }
    let game_master = match mode.as_str() {
        "tes5" | "tes5vr" | "sse" => Some("Skyrim.esm"),
        "fo4" | "fo4vr" => Some("Fallout4.esm"),
        "fo76" => Some("SeventySix.esm"),
        _ => None,
    };
    if game_master.is_some_and(|master| case.name.eq_ignore_ascii_case(master)) {
        dropped_signatures.push(b"RNAM");
    }
    let dropped_signatures: &[&[u8; 4]] = &dropped_signatures;

    fn u32_at(buf: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(buf.get(at..at + 4)?.try_into().ok()?))
    }

    /// The data of a WRLD record without the subrecords whose signature is
    /// in `signatures` (every occurrence), if it has any.
    fn drop_ofst(data: &[u8], signatures: &[&[u8; 4]]) -> Option<Vec<u8>> {
        let mut rest: Vec<u8> = Vec::with_capacity(data.len());
        let mut found = false;
        let mut pos = 0;
        let mut pending: Option<(usize, usize)> = None;
        while pos + 6 <= data.len() {
            let signature = &data[pos..pos + 4];
            let size = u16::from_le_bytes([data[pos + 4], data[pos + 5]]) as usize;
            if signature == b"XXXX" {
                pending = Some((pos, u32_at(data, pos + 6)? as usize));
                pos += 6 + size;
                continue;
            }
            let (start, real) = match pending.take() {
                Some((start, real)) if size == 0 => (start, real),
                _ => (pos, size),
            };
            let end = pos + 6 + real;
            if signatures.iter().any(|candidate| signature == *candidate) {
                found = true;
            } else {
                rest.extend_from_slice(data.get(start..end)?);
            }
            pos = end;
        }
        found.then_some(rest)
    }

    fn walk(
        buf: &[u8],
        start: usize,
        end: usize,
        out: &mut Vec<u8>,
        dropped: &mut u64,
        signatures: &[&[u8; 4]],
    ) -> Option<()> {
        let mut pos = start;
        while pos < end {
            let signature = buf.get(pos..pos + 4)?;
            let size = u32_at(buf, pos + 4)? as usize;
            if signature == b"GRUP" {
                let mark = out.len();
                out.extend_from_slice(buf.get(pos..pos + HEADER)?);
                let before = *dropped;
                walk(buf, pos + HEADER, pos + size, out, dropped, signatures)?;
                let inner = (*dropped - before) as usize;
                if inner > 0 {
                    out[mark + 4..mark + 8].copy_from_slice(&((size - inner) as u32).to_le_bytes());
                }
                pos += size;
            } else {
                let flags = u32_at(buf, pos + 8)?;
                let data = buf.get(pos + HEADER..pos + HEADER + size)?;
                if signature == b"WRLD"
                    && flags & 0x0004_0000 == 0
                    && let Some(rest) = drop_ofst(data, signatures)
                {
                    let mark = out.len();
                    out.extend_from_slice(&buf[pos..pos + HEADER]);
                    out[mark + 4..mark + 8].copy_from_slice(&(rest.len() as u32).to_le_bytes());
                    *dropped += (data.len() - rest.len()) as u64;
                    out.extend_from_slice(&rest);
                } else {
                    out.extend_from_slice(&buf[pos..pos + HEADER + size]);
                }
                pos += HEADER + size;
            }
        }
        Some(())
    }

    let mut out = Vec::with_capacity(input.len());
    let mut dropped = 0u64;
    if walk(input, 0, input.len(), &mut out, &mut dropped, dropped_signatures).is_none() || dropped == 0 {
        return Ok(None);
    }
    Ok(Some((out, dropped)))
}

/// The offset of the first byte where the two files differ, or `None` when
/// they are identical; a length difference counts at the shorter length.
fn first_byte_difference(a: &Path, b: &Path) -> Result<Option<u64>> {
    let mut a = BufReader::with_capacity(1 << 20, File::open(a)?);
    let mut b = BufReader::with_capacity(1 << 20, File::open(b)?);
    let mut offset = 0u64;
    loop {
        let buf_a = a.fill_buf()?;
        let buf_b = b.fill_buf()?;
        if buf_a.is_empty() || buf_b.is_empty() {
            return Ok((buf_a.len() != buf_b.len()).then_some(offset));
        }
        let len = buf_a.len().min(buf_b.len());
        if let Some(position) = buf_a[..len].iter().zip(&buf_b[..len]).position(|(x, y)| x != y) {
            return Ok(Some(offset + position as u64));
        }
        a.consume(len);
        b.consume(len);
        offset += len as u64;
    }
}

fn parse(args: &[&str]) -> Result<Options> {
    const USAGE: &str = "usage: cargo xtask parity dump|saves|roundtrip|oracle-save|oracle-edit|bsarch|conflicts|refs|clean|check|check-dump|modgroups|merged|filter|tool-modes|script [--game <game>]... [--file <name>]... [--mode <tool mode>]... \
                         [--record <FormID>]... [--oracle-only] [--jobs <n>] [--memory-budget <GiB>] \
                         [--max-memory <GiB>] [--oracle-timeout <minutes>]";
    let (mode, rest) = args.split_first().context(USAGE)?;
    let (saves, roundtrip, oracle_save, oracle_edit) = match *mode {
        "dump" | "conflicts" | "refs" | "clean" | "check" | "modgroups" | "merged" | "filter" | "tool-modes" => {
            (false, false, false, false)
        }
        "check-dump" => {
            CHECK_DUMP.store(true, Ordering::Relaxed);
            (false, false, false, false)
        }
        "saves" => (true, false, false, false),
        "roundtrip" => (false, true, false, false),
        "oracle-save" => (false, false, true, false),
        "oracle-edit" => (false, false, false, true),
        _ => bail!(USAGE),
    };
    let mut options = Options {
        saves,
        roundtrip,
        oracle_save,
        oracle_edit,
        conflicts: *mode == "conflicts",
        clean: *mode == "clean",
        check: *mode == "check",
        records: Vec::new(),
        refs: *mode == "refs",
        modgroups: *mode == "modgroups",
        merged: *mode == "merged",
        tool_modes: *mode == "tool-modes",
        filter: *mode == "filter",
        games: Vec::new(),
        files: Vec::new(),
        modes: Vec::new(),
        oracle_only: false,
        // The memory budget decides how many of them run at once.
        jobs: 3,
        all_games: false,
        memory_budget: None,
        max_memory: None,
        oracle_timeout: None,
    };
    let gib = |value: Option<&&str>| -> Result<u64> {
        let value: f64 = value.context(USAGE)?.parse()?;
        ensure!(value > 0.0, "a memory size must be positive");
        Ok((value * GIB as f64) as u64)
    };
    let mut rest = rest.iter();
    while let Some(&arg) = rest.next() {
        match arg {
            "--game" => {
                let name = rest.next().context(USAGE)?;
                let game = GAMES
                    .iter()
                    .find(|game| game.name == *name)
                    .with_context(|| format!("unknown game {name}"))?;
                options.games.push(game);
            }
            "--file" => options.files.push(rest.next().context(USAGE)?.to_lowercase()),
            "--mode" => options.modes.push(rest.next().context(USAGE)?.to_lowercase()),
            "--record" => options.records.push(rest.next().context(USAGE)?.to_uppercase()),
            "--oracle-only" => options.oracle_only = true,
            "--jobs" => options.jobs = rest.next().context(USAGE)?.parse::<usize>()?.max(1),
            "--memory-budget" => options.memory_budget = Some(gib(rest.next())?),
            "--max-memory" => options.max_memory = Some(gib(rest.next())?),
            "--oracle-timeout" => {
                let minutes: f64 = rest.next().context(USAGE)?.parse()?;
                ensure!(minutes > 0.0, "the oracle timeout must be positive");
                options.oracle_timeout = Some(Duration::from_secs_f64(minutes * 60.0));
            }
            _ => bail!(USAGE),
        }
    }
    if options.games.is_empty() {
        options.games = GAMES
            .iter()
            .filter(|game| !saves || !game.save_extensions.is_empty())
            .collect();
        options.all_games = true;
    }
    Ok(options)
}

fn required_var(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("environment variable {name} is not set"))
}

fn cache_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("XEDIT_PARITY_CACHE") {
        return Ok(PathBuf::from(dir));
    }
    let base = if cfg!(windows) {
        PathBuf::from(required_var("LOCALAPPDATA")?)
    } else if let Ok(dir) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(dir)
    } else {
        PathBuf::from(required_var("HOME")?).join(".cache")
    };
    Ok(base.join("xedit-rust").join("parity-cache"))
}

fn is_vanilla(game: &Game, lower_name: &str) -> bool {
    game.vanilla.iter().any(|pattern| match pattern.strip_suffix('*') {
        Some(prefix) => lower_name.starts_with(prefix),
        None => lower_name == *pattern,
    })
}

pub(super) fn build_port(root: &Path) -> Result<PathBuf> {
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "--package", "xedit-cli"])
        .current_dir(root)
        .status()?;
    ensure!(status.success(), "building xedit-cli failed");
    // Cargo builds into `CARGO_TARGET_DIR` when it is set, which lets two
    // harness runs use separate port binaries.
    let target = match std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_relative() => root.join(dir),
        Some(dir) => dir,
        None => root.join("target"),
    };
    Ok(target
        .join("release")
        .join(format!("xedit{}", std::env::consts::EXE_SUFFIX)))
}

/// FNV-1a over the file content. Identifies an input file in the cache.
fn content_hash(path: &Path) -> Result<u64> {
    let mut file = File::open(path)?;
    let mut buffer = vec![0u8; 1 << 20];
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hash);
        }
        for &byte in &buffer[..read] {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// The cached oracle output of one input file: plain text from before the
/// cache was compressed, or zstd.
struct OracleOutput {
    path: PathBuf,
    crashed: bool,
    /// The message of the exception that ended a crashed run, when the run
    /// recorded it (`<stem>.oracle.error`).
    error: Option<String>,
}

impl OracleOutput {
    fn find(dir: &Path, stem: &str) -> Option<Self> {
        let error = fs::read_to_string(dir.join(format!("{stem}.oracle.error")))
            .ok()
            .and_then(|line| oracle_error_message(&line).map(str::to_owned));
        [
            ("oracle.txt.zst", false),
            ("oracle.crashed.txt.zst", true),
            ("oracle.timeout.txt.zst", true),
            ("oracle.txt", false),
            ("oracle.crashed.txt", true),
        ]
        .into_iter()
        .map(|(suffix, crashed)| Self {
            path: dir.join(format!("{stem}.{suffix}")),
            crashed,
            error: error.clone(),
        })
        .find(|output| output.path.exists())
    }

    fn reader(&self) -> Result<Box<dyn BufRead>> {
        let file = File::open(&self.path).with_context(|| format!("opening {}", self.path.display()))?;
        Ok(if self.path.extension().is_some_and(|e| e == "zst") {
            Box::new(BufReader::with_capacity(1 << 20, zstd::Decoder::new(file)?))
        } else {
            Box::new(BufReader::with_capacity(1 << 20, file))
        })
    }

    /// Size of the uncompressed text.
    fn len(&self) -> Result<u64> {
        if self.path.extension().is_some_and(|e| e == "zst") {
            Ok(std::io::copy(&mut self.reader()?, &mut std::io::sink())?)
        } else {
            Ok(fs::metadata(&self.path)?.len())
        }
    }
}

/// Passes a reader through and copies everything read into a writer.
struct Tee<R, W> {
    reader: R,
    writer: W,
    count: u64,
}

impl<R: Read, W: Write> Read for Tee<R, W> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.reader.read(buf)?;
        self.writer.write_all(&buf[..read])?;
        self.count += read as u64;
        Ok(read)
    }
}

/// The expected peak memory of a process on `case`: the peak of its last
/// run, or an estimate from the file sizes before the first run.
fn expected_peak(case: &Case, peak_file: &Path) -> u64 {
    if let Some(peak) = fs::read_to_string(peak_file)
        .ok()
        .and_then(|text| text.trim().parse().ok())
    {
        return peak;
    }
    let size = |path: &Path| fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut inputs = size(&case.input);
    let master = case.input.with_file_name(case.game.vanilla[0]);
    if !case.name.eq_ignore_ascii_case(case.game.vanilla[0]) {
        inputs += size(&master);
    }
    (inputs * UNMEASURED_PEAK_FACTOR).max(GIB)
}

/// Runs `command` under the memory cap of the runner, after reserving its
/// expected peak in the budget. `consume` reads the output of the process
/// while it runs and decides whether it is still needed; the process is
/// killed when it is not. Returns the exit status, the result of `consume`
/// and whether the process reached the cap; the measured peak is written to
/// `peak_file`.
fn run_limited<T>(
    runner: &Runner,
    case: &Case,
    command: &mut Command,
    peak_file: &Path,
    consume: impl FnOnce(&mut Child) -> Result<(T, bool)>,
) -> Result<(ExitStatus, Result<T>, bool, Option<u64>)> {
    let expected = expected_peak(case, peak_file).min(runner.max_memory);
    let _reservation = runner.budget.reserve(expected);
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {:?}", command.get_program()))?;
    let limit = match Limit::apply(&child, runner.max_memory) {
        Ok(limit) => limit,
        Err(error) => {
            let _ = child.kill();
            return Err(error);
        }
    };
    let consumed = consume(&mut child);
    if !matches!(consumed, Ok((_, true))) {
        let _ = child.kill();
    }
    let status = child.wait()?;
    let peak = limit.peak();
    let reached = limit.reached();
    // Only a complete run measures the peak.
    if let (Some(peak), Ok((_, true))) = (peak, &consumed)
        && status.success()
    {
        fs::write(peak_file, peak.to_string())?;
    }
    Ok((status, consumed.map(|(value, _)| value), reached, peak))
}

fn check(case: &Case, runner: &Runner) -> Result<Outcome> {
    let dir = if case.saves {
        runner.cache.join(format!("{}-saves", case.game.mode))
    } else if check_dump() {
        runner.cache.join(format!("{}-check", case.game.mode))
    } else {
        runner.cache.join(case.game.mode)
    };
    fs::create_dir_all(&dir)?;
    let stem = format!("{}.{:016x}", case.name, content_hash(&case.input)?);
    let mut outcome = Outcome {
        game: case.game.name,
        file: case.name.clone(),
        status: "oracle-only",
        detail: None,
        oracle_bytes: 0,
        port_bytes: 0,
        oracle_peak: None,
        port_peak: None,
    };
    let oracle_out = match OracleOutput::find(&dir, &stem) {
        Some(output) => output,
        None => {
            outcome.oracle_peak = run_oracle(case, runner, &dir, &stem)?;
            OracleOutput::find(&dir, &stem).context("oracle output missing after the run")?
        }
    };
    let Some(port) = &runner.port else {
        outcome.oracle_bytes = oracle_out.len()?;
        return Ok(outcome);
    };

    // The port output is compared while it is produced and kept, compressed,
    // only when it differs.
    let port_out = dir.join(format!("{stem}.port.txt.zst"));
    let port_log = dir.join(format!("{stem}.port.log"));
    let mut command = Command::new(port);
    if case.saves {
        command
            .args(["saves", "dump", "--game", case.game.mode, "--data"])
            .arg(&case.data);
    } else {
        command.args(["dump", "--game", case.game.mode]);
        if check_dump() {
            command.arg("--check");
        }
    }
    command
        .arg(&case.input)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(File::create(&port_log)?);
    let mut port_bytes = 0;
    let (status, difference, reached, peak) = run_limited(
        runner,
        case,
        &mut command,
        &dir.join(format!("{stem}.port.peak")),
        |child| {
            let mut tee = Tee {
                reader: child.stdout.take().unwrap(),
                writer: zstd::Encoder::new(File::create(&port_out)?, 1)?,
                count: 0,
            };
            let difference = first_difference(
                oracle_out.reader()?,
                BufReader::with_capacity(1 << 20, &mut tee),
                oracle_out.crashed,
            );
            let difference = difference?;
            // Past the end of a crashed oracle run the port still has to
            // finish without an error.
            if difference.is_none() {
                tee.count += std::io::copy(&mut tee.reader, &mut std::io::sink())?;
            }
            port_bytes = tee.count;
            tee.writer.finish()?;
            // The port is not needed past the first difference.
            let needed = difference.is_none();
            Ok((difference, needed))
        },
    )?;
    outcome.port_bytes = port_bytes;
    outcome.port_peak = peak;
    let difference = difference?;
    if reached && (difference.is_some() || !status.success()) {
        outcome.status = "port-memory-limit";
        outcome.detail = Some(format!(
            "  reached the cap of {:.1} GiB (--max-memory), see {}",
            runner.max_memory as f64 / GIB as f64,
            port_log.display()
        ));
        return Ok(outcome);
    }
    if !status.success() && difference.is_none() {
        // An oracle run that stopped with an exception (a save whose header
        // magic is not the one its definitions expect) is matched by a port
        // run that stops with the same message after the same output.
        let port_error = last_line(&port_log)?;
        if let (Some(expected), Some(actual)) = (&oracle_out.error, port_error.strip_prefix("error: "))
            && expected == actual
        {
            outcome.status = "equal-error";
            outcome.detail = Some(format!("  both stopped with: {actual}"));
            fs::remove_file(&port_out)?;
            return Ok(outcome);
        }
        outcome.status = "port-failed";
        outcome.detail = Some(format!("  {status}, see {}", port_log.display()));
        return Ok(outcome);
    }
    match difference {
        None => {
            outcome.status = if oracle_out.crashed { "equal-prefix" } else { "equal" };
            fs::remove_file(&port_out)?;
        }
        Some(detail) => {
            outcome.status = "different";
            outcome.detail = Some(format!(
                "{detail}\n  oracle: {}\n  port:   {} (up to the difference)\n  log:    {}",
                oracle_out.path.display(),
                port_out.display(),
                port_log.display()
            ));
        }
    }
    Ok(outcome)
}

/// Runs the oracle on `case` and caches its output compressed. Returns the
/// peak memory of the run.
fn run_oracle(case: &Case, runner: &Runner, dir: &Path, stem: &str) -> Result<Option<u64>> {
    // Each harness process writes its own partial output and log, so that two
    // runs on the same file do not clobber each other; the first to finish
    // fills the cache.
    let partial = dir.join(format!("{stem}.oracle.{}.partial.zst", std::process::id()));
    let log = dir.join(format!("{stem}.oracle.{}.log", std::process::id()));
    let mut command = Command::new(&runner.oracle);
    command.arg(format!("-{}", case.game.mode));
    if case.saves {
        command.arg("-saves");
    }
    if check_dump() {
        command.arg("-check");
    }
    command
        .arg("-q")
        // The masters of a plugin that is not the game master, and the
        // plugins of a save, load from the data path; without it the oracle
        // fails on the hardcoded records.
        .arg(format!("-D:{}", case.data.display()))
        .arg(&case.input)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(File::create(&log)?);
    let (status, copied, reached, peak) = run_limited(
        runner,
        case,
        &mut command,
        &dir.join(format!("{stem}.oracle.peak")),
        |child| {
            let mut encoder = zstd::Encoder::new(File::create(&partial)?, 3)?;
            let mut stdout = child.stdout.take().unwrap();
            let start = Instant::now();
            let mut buffer = vec![0u8; 1 << 20];
            let mut timed_out = false;
            loop {
                let read = stdout.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                encoder.write_all(&buffer[..read])?;
                if runner.oracle_timeout.is_some_and(|timeout| start.elapsed() > timeout) {
                    timed_out = true;
                    let _ = child.kill();
                    break;
                }
            }
            encoder.finish()?;
            Ok((timed_out, true))
        },
    )
    .with_context(|| format!("running {}", runner.oracle.display()))?;
    let timed_out = copied?;
    // xDump exits with 0 after an exception. A complete run ends its log with
    // "All Done."; a run that died with an access violation (which the oracle
    // does on every Fallout 4 INFO with an alias condition) is kept as the
    // prefix the port has to match. A run that hit the memory cap says
    // nothing about the dump and is not kept.
    // The log has a line per record; only its end is read and kept.
    let log_tail = shrink_to_tail(&log, 64 * 1024)?;
    let last = log_tail.lines().last().unwrap_or_default();
    if reached {
        let _ = fs::remove_file(&partial);
        bail!(
            "oracle reached the memory cap of {:.1} GiB (--max-memory), last log line: {last}",
            runner.max_memory as f64 / GIB as f64
        );
    }
    // The output of an oracle run that was stopped at --oracle-timeout is
    // kept as the prefix the port has to match (the oracle needs hours for
    // a Skyrim LE save, which raises an exception per FormID).
    if timed_out {
        keep_partial(&partial, &dir.join(format!("{stem}.oracle.timeout.txt.zst")))?;
        return Ok(peak);
    }
    ensure!(status.success(), "oracle failed: {status}, last log line: {last}");
    // An I/O error (1450, insufficient system resources, under memory
    // pressure) says nothing about the dump: the run is not kept.
    if last.contains("Unexpected Error: <EInOutError:") {
        let _ = fs::remove_file(&partial);
        bail!("oracle did not finish: last log line: {last}");
    }
    if last.ends_with("All Done.") {
        keep_partial(&partial, &dir.join(format!("{stem}.oracle.txt.zst")))?;
    } else if last.contains("Unexpected Error") {
        keep_partial(&partial, &dir.join(format!("{stem}.oracle.crashed.txt.zst")))?;
        fs::write(dir.join(format!("{stem}.oracle.error")), last)?;
    } else {
        bail!("oracle did not finish: last log line: {last}");
    }
    Ok(peak)
}

/// Moves a finished oracle output into the cache, unless another run put it
/// there first.
fn keep_partial(partial: &Path, cached: &Path) -> Result<()> {
    if cached.exists() {
        fs::remove_file(partial)?;
        return Ok(());
    }
    fs::rename(partial, cached)?;
    Ok(())
}

/// The message of the exception that ended an oracle run, from the last line
/// of its log: `<time> Unexpected Error: <EClass: message>`.
fn oracle_error_message(line: &str) -> Option<&str> {
    let rest = line.trim_end().split_once("Unexpected Error: <")?.1;
    Some(rest.strip_suffix('>')?.split_once(": ")?.1)
}

/// The last non-empty line of a log.
fn last_line(path: &Path) -> Result<String> {
    use std::io::{Seek, SeekFrom};
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(64 * 1024)))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    let tail = String::from_utf8_lossy(&tail);
    Ok(tail
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .to_owned())
}

/// Cuts the file down to its last `keep` bytes, from a line start, and
/// returns them as text.
fn shrink_to_tail(path: &Path, keep: u64) -> Result<String> {
    use std::io::{Seek, SeekFrom};
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(keep)))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    drop(file);
    if len > keep {
        let start = tail.iter().position(|&byte| byte == b'\n').map_or(0, |n| n + 1);
        tail.drain(..start);
        fs::write(path, &tail)?;
    }
    Ok(String::from_utf8_lossy(&tail).into_owned())
}

/// Describes the first line that differs, or returns `None` for equal files.
/// With `prefix`, the port may continue past the end of the oracle output.
fn first_difference(mut oracle: impl BufRead, mut port: impl BufRead, prefix: bool) -> Result<Option<String>> {
    let (mut expected, mut actual) = (Vec::new(), Vec::new());
    let mut line = 0u64;
    loop {
        expected.clear();
        actual.clear();
        let read_expected = oracle.read_until(b'\n', &mut expected)?;
        let read_actual = port.read_until(b'\n', &mut actual)?;
        line += 1;
        if read_expected == 0 && (read_actual == 0 || prefix) {
            return Ok(None);
        }
        if expected != actual && !(prefix && read_expected < read_actual && actual.starts_with(&expected)) {
            let show = |bytes: &[u8]| {
                if bytes.is_empty() {
                    "<end of file>".to_owned()
                } else {
                    String::from_utf8_lossy(bytes).trim_end_matches(['\r', '\n']).to_owned()
                }
            };
            let mut detail = Vec::new();
            writeln!(detail, "  line {line}")?;
            writeln!(detail, "  - {}", show(&expected))?;
            write!(detail, "  + {}", show(&actual))?;
            return Ok(Some(String::from_utf8(detail)?));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_error_message_is_the_text_of_the_exception() {
        let line = r#"<00:00:00.271> Unexpected Error: <Exception: Expected header Magic FO3SAVEGAME, found TES4SAVEGAM in file "C:\Saves\Save 1.ess">"#;
        assert_eq!(
            oracle_error_message(line),
            Some(r#"Expected header Magic FO3SAVEGAME, found TES4SAVEGAM in file "C:\Saves\Save 1.ess""#)
        );
        assert_eq!(oracle_error_message("<00:00:01.000> All Done."), None);
    }

    fn diff_files(oracle: &Path, port: &Path, prefix: bool) -> Result<Option<String>> {
        first_difference(
            BufReader::new(File::open(oracle)?),
            BufReader::new(File::open(port)?),
            prefix,
        )
    }

    #[test]
    fn vanilla_patterns() {
        let fo4 = &GAMES[0];
        assert!(is_vanilla(fo4, "fallout4.esm"));
        assert!(is_vanilla(fo4, "dlccoast.esm"));
        assert!(is_vanilla(fo4, "ccbgsfo4001-pipboy(black).esl"));
        assert!(!is_vanilla(fo4, "mymod.esp"));
    }

    #[test]
    fn difference_reports_line() {
        let dir = std::env::temp_dir().join(format!("xtask-parity-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        fs::write(&a, "one\r\ntwo\r\n").unwrap();
        fs::write(&b, "one\r\ntwo\r\n").unwrap();
        assert_eq!(diff_files(&a, &b, false).unwrap(), None);
        fs::write(&b, "one\r\n2\r\n").unwrap();
        let detail = diff_files(&a, &b, false).unwrap().unwrap();
        assert!(detail.contains("line 2") && detail.contains("- two") && detail.contains("+ 2"));
        fs::write(&b, "one\r\n").unwrap();
        assert!(diff_files(&a, &b, false).unwrap().unwrap().contains("<end of file>"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn prefix_allows_the_port_to_continue() {
        let dir = std::env::temp_dir().join(format!("xtask-parity-prefix-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (oracle, port) = (dir.join("oracle"), dir.join("port"));
        // The oracle crashed in the middle of its third line.
        fs::write(&oracle, "one\r\ntwo\r\nthr").unwrap();
        fs::write(&port, "one\r\ntwo\r\nthree\r\nfour\r\n").unwrap();
        assert_eq!(diff_files(&oracle, &port, true).unwrap(), None);
        assert!(diff_files(&oracle, &port, false).unwrap().is_some());
        fs::write(&port, "one\r\ntwo\r\nfour\r\n").unwrap();
        assert!(diff_files(&oracle, &port, true).unwrap().unwrap().contains("line 3"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
