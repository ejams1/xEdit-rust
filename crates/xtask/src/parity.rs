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
//! Environment:
//!
//! - `XEDIT_ORACLE_DIR`: unpacked release archive of the baseline tag.
//! - `XEDIT_<GAME>_DATA` (`XEDIT_FO4_DATA`, `XEDIT_SSE_DATA`, ... see
//!   `GAMES`): `Data` directory of each game.
//! - `XEDIT_PARITY_CACHE`: cache directory. Defaults to the user cache directory.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

use crate::memory::{self, Budget, GIB, Limit};

/// A game whose masters are in the corpus.
struct Game {
    /// Name on the command line of `cargo xtask parity`.
    name: &'static str,
    /// Game mode switch of the oracle and of `xedit dump --game`.
    mode: &'static str,
    data_var: &'static str,
    /// Lower-case names of the vanilla plugins. A trailing `*` matches any rest.
    vanilla: &'static [&'static str],
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
        vanilla: &["fallout4.esm", "dlc*", "cc*"],
    },
    Game {
        name: "sse",
        mode: "SSE",
        data_var: "XEDIT_SSE_DATA",
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
        vanilla: &["morrowind.esm", "tribunal.esm", "bloodmoon.esm"],
    },
    Game {
        name: "tes4",
        mode: "TES4",
        data_var: "XEDIT_TES4_DATA",
        vanilla: &["oblivion.esm", "dlc*", "knights.esp"],
    },
    Game {
        name: "fo3",
        mode: "FO3",
        data_var: "XEDIT_FO3_DATA",
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
        vanilla: &SKYRIM_MASTERS,
    },
    Game {
        name: "tes5vr",
        mode: "TES5VR",
        data_var: "XEDIT_TES5VR_DATA",
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
        vanilla: &["fallout4.esm", "fallout4_vr.esm"],
    },
    Game {
        name: "fo76",
        mode: "FO76",
        data_var: "XEDIT_FO76_DATA",
        vanilla: &["seventysix.esm", "nw.esm"],
    },
    Game {
        name: "sf1",
        mode: "SF1",
        data_var: "XEDIT_SF1_DATA",
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
    games: Vec<&'static Game>,
    /// Lower-case file names. Empty selects the whole corpus.
    files: Vec<String>,
    oracle_only: bool,
    jobs: usize,
    /// No `--game` was given.
    all_games: bool,
    /// Sum of the expected peaks of the processes that run at once.
    memory_budget: Option<u64>,
    /// Cap on the committed memory of one process.
    max_memory: Option<u64>,
}

/// One corpus file and where its outputs go.
struct Case {
    game: &'static Game,
    input: PathBuf,
    name: String,
}

/// What every check shares.
struct Runner {
    oracle: PathBuf,
    port: Option<PathBuf>,
    cache: PathBuf,
    budget: Budget,
    max_memory: u64,
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
    /// output up to the crash), `different`, `oracle-failed`, `port-failed`
    /// or `oracle-only`.
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
    let options = parse(args)?;
    let oracle = PathBuf::from(required_var("XEDIT_ORACLE_DIR")?).join("xDump.exe");
    ensure!(oracle.exists(), "{} does not exist", oracle.display());
    let cache = cache_dir()?.join(tag);

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
                cases.push(Case { game, input, name });
            }
        }
        ensure!(cases.len() > before, "no corpus file found in {}", data.display());
    }
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
        budget: Budget::new(budget),
        max_memory: options.max_memory.unwrap_or(budget),
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
                    let outcome = check(case, &runner).unwrap_or_else(|error| Outcome {
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
    let equal = outcomes
        .iter()
        .filter(|o| o.status == "equal" || o.status == "equal-prefix")
        .count();
    let report = Report {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("dump.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    println!("{equal} of {} equal. Report: {}", outcomes.len(), report_file.display());
    if !options.oracle_only {
        ensure!(equal == outcomes.len(), "dump parity does not hold");
    }
    Ok(())
}

fn parse(args: &[&str]) -> Result<Options> {
    const USAGE: &str = "usage: cargo xtask parity dump [--game <game>]... [--file <name>]... [--oracle-only] \
                         [--jobs <n>] [--memory-budget <GiB>] [--max-memory <GiB>]";
    let ["dump", rest @ ..] = args else { bail!(USAGE) };
    let mut options = Options {
        games: Vec::new(),
        files: Vec::new(),
        oracle_only: false,
        // The memory budget decides how many of them run at once.
        jobs: 3,
        all_games: false,
        memory_budget: None,
        max_memory: None,
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
            "--oracle-only" => options.oracle_only = true,
            "--jobs" => options.jobs = rest.next().context(USAGE)?.parse::<usize>()?.max(1),
            "--memory-budget" => options.memory_budget = Some(gib(rest.next())?),
            "--max-memory" => options.max_memory = Some(gib(rest.next())?),
            _ => bail!(USAGE),
        }
    }
    if options.games.is_empty() {
        options.games = GAMES.iter().collect();
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

fn build_port(root: &Path) -> Result<PathBuf> {
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "--package", "xedit-cli"])
        .current_dir(root)
        .status()?;
    ensure!(status.success(), "building xedit-cli failed");
    Ok(root
        .join("target/release")
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
}

impl OracleOutput {
    fn find(dir: &Path, stem: &str) -> Option<Self> {
        [
            ("oracle.txt.zst", false),
            ("oracle.crashed.txt.zst", true),
            ("oracle.txt", false),
            ("oracle.crashed.txt", true),
        ]
        .into_iter()
        .map(|(suffix, crashed)| Self {
            path: dir.join(format!("{stem}.{suffix}")),
            crashed,
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
    let dir = runner.cache.join(case.game.mode);
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
    command
        .args(["dump", "--game", case.game.mode])
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
    let partial = dir.join(format!("{stem}.oracle.partial.zst"));
    let log = dir.join(format!("{stem}.oracle.log"));
    let mut command = Command::new(&runner.oracle);
    command
        .arg(format!("-{}", case.game.mode))
        .arg("-q")
        // The masters of a plugin that is not the game master load from the
        // data path; without it the oracle fails on the hardcoded records.
        .arg(format!(
            "-D:{}",
            case.input
                .parent()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default()
        ))
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
            std::io::copy(&mut child.stdout.take().unwrap(), &mut encoder)?;
            encoder.finish()?;
            Ok(((), true))
        },
    )
    .with_context(|| format!("running {}", runner.oracle.display()))?;
    copied?;
    // xDump exits with 0 after an exception. A complete run ends its log with
    // "All Done."; a run that died with an access violation (which the oracle
    // does on every Fallout 4 INFO with an alias condition) is kept as the
    // prefix the port has to match. A run that hit the memory cap says
    // nothing about the dump and is not kept.
    let log_text = String::from_utf8_lossy(&fs::read(&log)?).into_owned();
    let last = log_text.lines().last().unwrap_or_default();
    if reached {
        let _ = fs::remove_file(&partial);
        bail!(
            "oracle reached the memory cap of {:.1} GiB (--max-memory), last log line: {last}",
            runner.max_memory as f64 / GIB as f64
        );
    }
    ensure!(status.success(), "oracle failed: {status}, last log line: {last}");
    if last.ends_with("All Done.") {
        fs::rename(&partial, dir.join(format!("{stem}.oracle.txt.zst")))?;
    } else if last.contains("Unexpected Error") {
        fs::rename(&partial, dir.join(format!("{stem}.oracle.crashed.txt.zst")))?;
    } else {
        bail!("oracle did not finish: last log line: {last}");
    }
    Ok(peak)
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
