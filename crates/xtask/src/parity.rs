// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Differential parity harness.
//!
//! `cargo xtask parity dump` runs the oracle `xDump.exe` and the port's
//! `xedit dump` on every corpus file and compares the two outputs byte for
//! byte. Oracle output is cached outside the repository, keyed by the release
//! tag, the game and a hash of the input file.
//!
//! Environment:
//!
//! - `XEDIT_ORACLE_DIR`: unpacked release archive of the baseline tag.
//! - `XEDIT_FO4_DATA`, `XEDIT_SSE_DATA`: `Data` directory of each game.
//! - `XEDIT_PARITY_CACHE`: cache directory. Defaults to the user cache directory.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

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
            "skyrim.esm",
            "update.esm",
            "dawnguard.esm",
            "hearthfires.esm",
            "dragonborn.esm",
            "_resourcepack.esl",
            "cc*",
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
}

/// One corpus file and where its outputs go.
struct Case {
    game: &'static Game,
    input: PathBuf,
    name: String,
}

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
        let data = PathBuf::from(required_var(game.data_var)?);
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

    let next = AtomicUsize::new(0);
    let outcomes = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.min(cases.len()) {
            scope.spawn(|| {
                while let Some(case) = cases.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let outcome = check(case, &oracle, port.as_deref(), &cache).unwrap_or_else(|error| Outcome {
                        game: case.game.name,
                        file: case.name.clone(),
                        status: "oracle-failed",
                        detail: Some(format!("{error:#}")),
                        oracle_bytes: 0,
                        port_bytes: 0,
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
    const USAGE: &str =
        "usage: cargo xtask parity dump [--game fo4|sse]... [--file <name>]... [--oracle-only] [--jobs <n>]";
    let ["dump", rest @ ..] = args else { bail!(USAGE) };
    let mut options = Options {
        games: Vec::new(),
        files: Vec::new(),
        oracle_only: false,
        // Three dumps of up to 4.5 GB each next to the oracle fit in 32 GB.
        jobs: 3,
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
            _ => bail!(USAGE),
        }
    }
    if options.games.is_empty() {
        options.games = GAMES.iter().collect();
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

fn check(case: &Case, oracle: &Path, port: Option<&Path>, cache: &Path) -> Result<Outcome> {
    let dir = cache.join(case.game.mode);
    fs::create_dir_all(&dir)?;
    let stem = format!("{}.{:016x}", case.name, content_hash(&case.input)?);
    let oracle_out = dir.join(format!("{stem}.oracle.txt"));
    let oracle_crashed = dir.join(format!("{stem}.oracle.crashed.txt"));
    if !oracle_out.exists() && !oracle_crashed.exists() {
        run_oracle(case, oracle, &dir, &stem, &oracle_out, &oracle_crashed)?;
    }
    // A crashed oracle run is valid up to the last record it wrote.
    let crashed = !oracle_out.exists();
    let oracle_out = if crashed { oracle_crashed } else { oracle_out };
    let oracle_bytes = fs::metadata(&oracle_out)?.len();
    let mut outcome = Outcome {
        game: case.game.name,
        file: case.name.clone(),
        status: "oracle-only",
        detail: None,
        oracle_bytes,
        port_bytes: 0,
    };
    let Some(port) = port else { return Ok(outcome) };

    let port_out = dir.join(format!("{stem}.port.txt"));
    let port_log = dir.join(format!("{stem}.port.log"));
    let status = Command::new(port)
        .args(["dump", "--game", case.game.mode])
        .arg(&case.input)
        .stdin(Stdio::null())
        .stdout(File::create(&port_out)?)
        .stderr(File::create(&port_log)?)
        .status()?;
    outcome.port_bytes = fs::metadata(&port_out)?.len();
    if !status.success() {
        outcome.status = "port-failed";
        outcome.detail = Some(format!("  {status}, see {}", port_log.display()));
        return Ok(outcome);
    }
    match first_difference(&oracle_out, &port_out, crashed)? {
        None => outcome.status = if crashed { "equal-prefix" } else { "equal" },
        Some(detail) => {
            outcome.status = "different";
            outcome.detail = Some(format!(
                "{detail}\n  oracle: {}\n  port:   {}",
                oracle_out.display(),
                port_out.display()
            ));
        }
    }
    Ok(outcome)
}

fn run_oracle(
    case: &Case,
    oracle: &Path,
    dir: &Path,
    stem: &str,
    oracle_out: &Path,
    oracle_crashed: &Path,
) -> Result<()> {
    let partial = dir.join(format!("{stem}.oracle.partial"));
    let log = dir.join(format!("{stem}.oracle.log"));
    let status = Command::new(oracle)
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
        .stdout(File::create(&partial)?)
        .stderr(File::create(&log)?)
        .status()
        .with_context(|| format!("running {}", oracle.display()))?;
    // xDump exits with 0 after an exception. A complete run ends its log with
    // "All Done."; a run that died with an access violation (which the oracle
    // does on every Fallout 4 INFO with an alias condition) is kept as the
    // prefix the port has to match.
    let log_text = String::from_utf8_lossy(&fs::read(&log)?).into_owned();
    let last = log_text.lines().last().unwrap_or_default();
    ensure!(status.success(), "oracle failed: {status}, last log line: {last}");
    if last.ends_with("All Done.") {
        fs::rename(&partial, oracle_out)?;
    } else if last.contains("Unexpected Error") {
        fs::rename(&partial, oracle_crashed)?;
    } else {
        bail!("oracle did not finish: last log line: {last}");
    }
    Ok(())
}

/// Describes the first line that differs, or returns `None` for equal files.
/// With `prefix`, the port may continue past the end of the oracle output.
fn first_difference(oracle: &Path, port: &Path, prefix: bool) -> Result<Option<String>> {
    let mut oracle = BufReader::with_capacity(1 << 20, File::open(oracle)?);
    let mut port = BufReader::with_capacity(1 << 20, File::open(port)?);
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
        assert_eq!(first_difference(&a, &b, false).unwrap(), None);
        fs::write(&b, "one\r\n2\r\n").unwrap();
        let detail = first_difference(&a, &b, false).unwrap().unwrap();
        assert!(detail.contains("line 2") && detail.contains("- two") && detail.contains("+ 2"));
        fs::write(&b, "one\r\n").unwrap();
        assert!(
            first_difference(&a, &b, false)
                .unwrap()
                .unwrap()
                .contains("<end of file>")
        );
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
        assert_eq!(first_difference(&oracle, &port, true).unwrap(), None);
        assert!(first_difference(&oracle, &port, false).unwrap().is_some());
        fs::write(&port, "one\r\ntwo\r\nfour\r\n").unwrap();
        assert!(
            first_difference(&oracle, &port, true)
                .unwrap()
                .unwrap()
                .contains("line 3")
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
