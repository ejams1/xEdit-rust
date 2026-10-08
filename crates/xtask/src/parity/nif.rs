// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity nif`: the NIF and material files of the corpus
//! archives, loaded and saved by the port and by the oracle.
//!
//! The oracle is `Sniff.exe` of the release in its automation mode
//! (`-OP:<operation> -I:<archive> -O:<folder> -S:<settings> -LOG:<file>`),
//! which reads the archive itself and processes its files on all CPUs:
//!
//! - the dump: the operation `Convert to and from JSON` writes every NIF
//!   (`*.nif`, `*.kf`) as `ToJSON`, with six decimals and rotations as an
//!   angle and an axis, in the ANSI code page of the system.
//! - the save: the operation `Universal tweaker` sets `Num Blocks` of the
//!   `NiHeader`, which saving recomputes (`UpdateHeader`), so each NIF is
//!   written exactly as a load and save writes it. A material has no such
//!   element, and the tweaker reports the missing element as changed
//!   (its `EditValues` leaves the caller's string in the result), so each
//!   material is written as loaded too.
//!
//! The oracle's outputs are reduced to a hash per file and cached in
//! `<cache>/<tag>/nif-oracle`, keyed by the archive's name, size and a hash
//! of its first and last megabyte. The port runs in this process on the
//! same files, read with the port's archive reader. A file whose output
//! differs is written to `<scratch>/<tag>/nif-diff` with the oracle's
//! output for it, which Sniff makes again for that file alone (`-P:`).

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};

use xedit_assets::data_format::FLOAT_DECIMAL_DIGITS;
use xedit_assets::data_format_material::MaterialFile;
use xedit_assets::data_format_nif::NifFile;
use xedit_assets::data_format_nif_types::ROTATION_EULER;
use xedit_io::archive::Archive;
use xedit_io::encoding::Encoding;

use super::{GAMES, Game, cache_dir, required_var};

const USAGE: &str = "usage: cargo xtask parity nif [--game <game>]... [--archive <name part>]... [--file <path part>] [--threads <n>] [--keep]";

/// The file kinds of the check.
fn kind(path: &str) -> Option<Kind> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".nif") || lower.ends_with(".kf") {
        Some(Kind::Nif)
    } else if lower.ends_with(".bgsm") {
        Some(Kind::Bgsm)
    } else if lower.ends_with(".bgem") {
        Some(Kind::Bgem)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Nif,
    Bgsm,
    Bgem,
}

/// The output of one side for one file: the hash of the bytes, or the
/// message of the exception.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Output {
    Hash(u64),
    Error(String),
}

/// FNV-1a.
fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

struct Options {
    games: Vec<&'static Game>,
    archives: Vec<String>,
    file: Option<String>,
    threads: usize,
    keep: bool,
}

fn parse(args: &[&str]) -> Result<Options> {
    let mut options = Options {
        games: Vec::new(),
        archives: Vec::new(),
        file: None,
        threads: std::thread::available_parallelism().map_or(4, |count| count.get()),
        keep: false,
    };
    let mut rest = args.iter();
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
            "--archive" => options.archives.push(rest.next().context(USAGE)?.to_lowercase()),
            "--file" => options.file = Some(rest.next().context(USAGE)?.to_lowercase().replace('/', "\\")),
            "--threads" => options.threads = rest.next().context(USAGE)?.parse::<usize>()?.max(1),
            "--keep" => options.keep = true,
            _ => bail!(USAGE),
        }
    }
    if options.games.is_empty() {
        options.games = GAMES.iter().collect();
    }
    Ok(options)
}

/// The cache key of an archive: its name, size and a hash of its first and
/// last megabyte.
fn archive_key(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let chunk = 1 << 20;
    let mut head = vec![0u8; chunk.min(size as usize)];
    file.read_exact(&mut head)?;
    let mut tail = vec![0u8; chunk.min(size as usize)];
    file.seek(SeekFrom::End(-(tail.len() as i64)))?;
    file.read_exact(&mut tail)?;
    head.extend_from_slice(&tail);
    let name = path.file_name().unwrap_or_default().to_string_lossy().replace(' ', "_");
    Ok(format!("{name}-{size}-{:016x}", fnv(&head)))
}

/// The oracle's outputs of one archive: per lower-case path, the dump and
/// the save.
#[derive(Default)]
struct OracleOutputs {
    json: HashMap<String, Output>,
    save: HashMap<String, Output>,
}

fn write_cache(path: &Path, outputs: &OracleOutputs) -> Result<()> {
    let mut text = String::new();
    for (side, map) in [("json", &outputs.json), ("save", &outputs.save)] {
        let sorted: BTreeMap<&String, &Output> = map.iter().collect();
        for (name, output) in sorted {
            match output {
                Output::Hash(hash) => text.push_str(&format!("{side}\t{name}\t{hash:016x}\n")),
                Output::Error(message) => {
                    text.push_str(&format!(
                        "{side}\t{name}\tERR\t{}\n",
                        message.replace(['\t', '\n', '\r'], " ")
                    ));
                }
            }
        }
    }
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, text)?;
    Ok(())
}

fn read_cache(path: &Path) -> Result<OracleOutputs> {
    let mut outputs = OracleOutputs::default();
    for line in fs::read_to_string(path)?.lines() {
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let output = if parts[2] == "ERR" {
            Output::Error(parts.get(3).unwrap_or(&"").to_string())
        } else {
            Output::Hash(u64::from_str_radix(parts[2], 16)?)
        };
        let map = if parts[0] == "json" {
            &mut outputs.json
        } else {
            &mut outputs.save
        };
        map.insert(parts[1].to_owned(), output);
    }
    Ok(outputs)
}

/// The settings file of a Sniff run.
fn settings(operation: &str) -> &'static str {
    match operation {
        "json" => {
            "[Main]\r\nPopupWarning=0\r\n[ConverttoandfromJSON]\r\nbToJson=1\r\nsDigits=6\r\niRotation=0\r\nProcessedFiles=*.nif, *.kf\r\n"
        }
        _ => {
            "[Main]\r\nPopupWarning=0\r\n[Universaltweaker]\r\nProcessedFiles=*.nif, *.kf, *.bgsm, *.bgem\r\nbReportOnly=0\r\nsBlocks=NiHeader\r\nbDescendants=0\r\nsPath=Num Blocks\r\niValueMode=0\r\nsValue=99999\r\nbOldValueCheck=0\r\nsOldPath=\r\niOldValueMode=0\r\nsOldValue=\r\n"
        }
    }
}

fn windows_path(path: &Path) -> String {
    path.display().to_string().replace('/', "\\")
}

/// Runs Sniff on the archive: `operation` is `json` or `save`. The outputs
/// are written under `out`; the log is returned.
fn run_sniff(
    sniff: &Path,
    work: &Path,
    archive: &Path,
    operation: &str,
    out: &Path,
    path_filter: Option<&str>,
    threads: usize,
) -> Result<String> {
    fs::create_dir_all(work)?;
    fs::create_dir_all(out)?;
    let exe = work.join("Sniff.exe");
    if !exe.exists() {
        fs::copy(sniff, &exe)?;
    }
    let ini = work.join(format!("{operation}.ini"));
    fs::write(&ini, settings(operation))?;
    let log = work.join(format!("{operation}.log"));
    let _ = fs::remove_file(&log);
    let title = if operation == "json" {
        "Convert to and from JSON"
    } else {
        "Universal tweaker"
    };
    let mut command = Command::new(&exe);
    command
        .current_dir(work)
        .arg(format!("-S:{}", windows_path(&ini)))
        .arg(format!("-OP:{title}"))
        .arg(format!("-I:{}", windows_path(archive)))
        .arg(format!("-O:{}", windows_path(out)))
        .arg(format!("-LOG:{}", windows_path(&log)))
        .arg("-skip:yes")
        .arg(format!("-threads:{threads}"));
    if let Some(filter) = path_filter {
        command.arg(format!("-P:{filter}"));
    }
    let mut child = command.spawn().context("starting Sniff")?;
    let size = fs::metadata(archive)?.len();
    let timeout = Duration::from_secs(300 + size / 2_000_000);
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Sniff ended with {status}");
            break;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Sniff did not finish within {} s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let bytes = fs::read(&log).with_context(|| format!("Sniff wrote no log {}", log.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The outputs below `dir` by lower-case archive path, hashed.
fn hash_outputs(dir: &Path, suffix: &str, map: &mut HashMap<String, Output>) -> Result<()> {
    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(dir)?.to_string_lossy().replace('/', "\\");
        let mut name = relative.to_lowercase();
        if !suffix.is_empty() {
            match name.strip_suffix(suffix) {
                Some(stripped) => name = stripped.to_owned(),
                None => continue,
            }
        }
        let bytes = fs::read(entry.path())?;
        map.insert(name, Output::Hash(fnv(&bytes)));
    }
    Ok(())
}

/// The files that Sniff skipped with an error: `Skipped: <file>: <message>`.
fn skipped(log: &str, map: &mut HashMap<String, Output>) {
    for line in log.lines() {
        if let Some(rest) = line.strip_prefix("Skipped: ") {
            // The path ends at the first `: ` after the extension.
            let lower = rest.to_lowercase();
            let end = [".nif: ", ".kf: ", ".bgsm: ", ".bgem: "]
                .iter()
                .filter_map(|ext| lower.find(ext).map(|index| index + ext.len() - 2))
                .min();
            if let Some(end) = end {
                map.insert(lower[..end].to_owned(), Output::Error(rest[end + 2..].to_owned()));
            }
        }
    }
}

/// The oracle's outputs of an archive, from the cache or made now.
fn oracle(sniff: &Path, archive: &Path, cache: &Path, scratch: &Path, options: &Options) -> Result<OracleOutputs> {
    let key = archive_key(archive)?;
    let cached = cache.join(format!("{key}.tsv"));
    let work = scratch.join("sniff-work").join(&key);
    if cached.exists() {
        let mut outputs = read_cache(&cached)?;
        if rerun_crashes(sniff, &work, archive, &mut outputs)? {
            write_cache(&cached, &outputs)?;
        }
        let _ = fs::remove_dir_all(&work);
        return Ok(outputs);
    }
    let mut outputs = OracleOutputs::default();
    let start = Instant::now();
    for operation in ["json", "save"] {
        let out = work.join(format!("out-{operation}"));
        let _ = fs::remove_dir_all(&out);
        let log = run_sniff(sniff, &work, archive, operation, &out, None, options.threads)?;
        let map = if operation == "json" {
            &mut outputs.json
        } else {
            &mut outputs.save
        };
        hash_outputs(&out, if operation == "json" { ".json" } else { "" }, map)?;
        skipped(&log, map);
        let _ = fs::remove_dir_all(&out);
    }
    rerun_crashes(sniff, &work, archive, &mut outputs)?;
    let _ = fs::remove_dir_all(&work);
    println!(
        "oracle        {} ({} dumps, {} saves, {:.0} s)",
        archive.display(),
        outputs.json.len(),
        outputs.save.len(),
        start.elapsed().as_secs_f64()
    );
    write_cache(&cached, &outputs)?;
    Ok(outputs)
}

/// Whether an exception of Sniff is a crash of its threads rather than an
/// error of the file: an access violation or an invalid pointer.
fn is_crash(message: &str) -> bool {
    message.starts_with("Access violation") || message.contains("Invalid pointer operation")
}

/// Runs the files whose output is a crash again, one at a time on one
/// thread: Sniff's threads share state, and a file that crashed among
/// others is read fine on its own. Returns whether an output changed.
fn rerun_crashes(sniff: &Path, work: &Path, archive: &Path, outputs: &mut OracleOutputs) -> Result<bool> {
    let mut changed = false;
    for operation in ["json", "save"] {
        let map = if operation == "json" {
            &mut outputs.json
        } else {
            &mut outputs.save
        };
        let crashed: Vec<String> = map
            .iter()
            .filter(|(_, output)| matches!(output, Output::Error(message) if is_crash(message)))
            .map(|(name, _)| name.clone())
            .collect();
        for name in crashed {
            let out = work.join(format!("rerun-{operation}"));
            let _ = fs::remove_dir_all(&out);
            let log = run_sniff(sniff, work, archive, operation, &out, Some(&name), 1)?;
            let mut rerun = HashMap::new();
            hash_outputs(&out, if operation == "json" { ".json" } else { "" }, &mut rerun)?;
            skipped(&log, &mut rerun);
            if let Some(output) = rerun.remove(&name) {
                println!("oracle rerun  {name}: {output:?}");
                map.insert(name, output);
                changed = true;
            }
            let _ = fs::remove_dir_all(&out);
        }
    }
    Ok(changed)
}

/// The port's dump and save of one file. A material has no dump.
/// The bytes of a port output, or the message of its exception.
type PortOutput = Result<Vec<u8>, String>;

fn port(kind: Kind, data: &[u8]) -> (Option<PortOutput>, PortOutput) {
    match kind {
        Kind::Nif => {
            let json = (|| {
                let mut nif = NifFile::new()?;
                nif.load_from_data(data)?;
                nif.to_json(false)
            })()
            // Sniff writes the text in the ANSI code page, with `?` for a
            // character outside of it.
            .map(|text| Encoding::Mbcs(0).get_bytes(&text))
            .map_err(|error| error.0);
            let save = (|| {
                let mut nif = NifFile::new()?;
                nif.load_from_data(data)?;
                nif.save_to_data()
            })()
            .map_err(|error| error.0);
            (Some(json), save)
        }
        Kind::Bgsm | Kind::Bgem => {
            let save = (|| {
                let mut file = if kind == Kind::Bgsm {
                    MaterialFile::new_bgsm()?
                } else {
                    MaterialFile::new_bgem()?
                };
                file.load_from_data(data)?;
                file.save_to_data()
            })()
            .map_err(|error| error.0);
            (None, save)
        }
    }
}

fn as_output(result: &Result<Vec<u8>, String>) -> Output {
    match result {
        Ok(bytes) => Output::Hash(fnv(bytes)),
        Err(message) => Output::Error(message.clone()),
    }
}

/// The outcome of one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Outcome {
    /// Dump and save equal the oracle's.
    Equal,
    /// The same exception in the oracle and the port.
    EqualError,
    DumpDifferent,
    SaveDifferent,
    PortFailed,
    OracleFailed,
    /// The oracle has no output for the file.
    OracleMissing,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Outcome::Equal => "equal",
            Outcome::EqualError => "equal-error",
            Outcome::DumpDifferent => "dump-different",
            Outcome::SaveDifferent => "save-different",
            Outcome::PortFailed => "port-failed",
            Outcome::OracleFailed => "oracle-failed",
            Outcome::OracleMissing => "oracle-missing",
        }
    }
}

fn compare(port: &Output, oracle: Option<&Output>) -> Option<Outcome> {
    match (port, oracle) {
        (_, None) => Some(Outcome::OracleMissing),
        (a, Some(b)) if a == b => None,
        (Output::Error(_), Some(Output::Hash(_))) => Some(Outcome::PortFailed),
        (Output::Hash(_), Some(Output::Error(_))) => Some(Outcome::OracleFailed),
        (Output::Error(_), Some(Output::Error(_))) => Some(Outcome::PortFailed),
        _ => Some(Outcome::SaveDifferent),
    }
}

pub fn run(_root: &Path, tag: &str, args: &[&str]) -> Result<()> {
    let options = parse(args)?;
    let sniff = PathBuf::from(required_var("XEDIT_ORACLE_DIR")?).join("Sniff.exe");
    ensure!(sniff.exists(), "{} does not exist", sniff.display());
    let cache = cache_dir()?.join(tag).join("nif-oracle");
    let scratch = match std::env::var_os("XEDIT_PARITY_SCRATCH") {
        Some(dir) => PathBuf::from(dir).join(tag),
        None => cache_dir()?.join(tag),
    };
    // The settings of the oracle's dump.
    FLOAT_DECIMAL_DIGITS.store(6, Ordering::Relaxed);
    ROTATION_EULER.store(false, Ordering::Relaxed);

    let mut totals: BTreeMap<Outcome, usize> = BTreeMap::new();
    let mut report = String::new();
    for &game in &options.games {
        let Some(data) = std::env::var_os(game.data_var).map(PathBuf::from) else {
            println!("skipped       {}: {} is not set", game.name, game.data_var);
            continue;
        };
        let mut archives: Vec<PathBuf> = fs::read_dir(&data)
            .with_context(|| format!("reading {}", data.display()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                (name.ends_with(".bsa") || name.ends_with(".ba2"))
                    && (options.archives.is_empty() || options.archives.iter().any(|part| name.contains(part)))
            })
            .collect();
        archives.sort();
        for archive_path in archives {
            let archive = match Archive::open(&archive_path) {
                Ok(archive) => archive,
                Err(error) => {
                    println!("skipped       {}: {error}", archive_path.display());
                    continue;
                }
            };
            let mut names: Vec<String> = archive
                .files()
                .filter(|name| kind(name).is_some())
                .filter(|name| options.file.as_ref().is_none_or(|part| name.contains(part.as_str())))
                .map(str::to_owned)
                .collect();
            if names.is_empty() {
                continue;
            }
            names.sort();
            let oracle = oracle(&sniff, &archive_path, &cache.join(game.name), &scratch, &options)?;

            let start = Instant::now();
            let next = AtomicUsize::new(0);
            let results: Mutex<Vec<(String, Outcome, String)>> = Mutex::new(Vec::new());
            std::thread::scope(|scope| {
                for _ in 0..options.threads {
                    scope.spawn(|| {
                        loop {
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            let Some(name) = names.get(index) else { break };
                            let kind = kind(name).unwrap_or(Kind::Nif);
                            let data = match archive.read(name) {
                                Ok(Some(data)) => data,
                                other => {
                                    let message = format!("{other:?}");
                                    results
                                        .lock()
                                        .unwrap()
                                        .push((name.clone(), Outcome::PortFailed, message));
                                    continue;
                                }
                            };
                            let (json, save) = port(kind, &data);
                            let mut outcome = Outcome::Equal;
                            let mut detail = String::new();
                            if let Some(json) = &json {
                                let port_json = as_output(json);
                                if let Some(found) = compare(&port_json, oracle.json.get(name)) {
                                    outcome = if found == Outcome::SaveDifferent {
                                        Outcome::DumpDifferent
                                    } else {
                                        found
                                    };
                                    detail = format!("dump: port {port_json:?}, oracle {:?}", oracle.json.get(name));
                                }
                            }
                            if outcome == Outcome::Equal {
                                let port_save = as_output(&save);
                                if let Some(found) = compare(&port_save, oracle.save.get(name)) {
                                    outcome = found;
                                    detail = format!("save: port {port_save:?}, oracle {:?}", oracle.save.get(name));
                                } else if matches!(port_save, Output::Error(_)) {
                                    outcome = Outcome::EqualError;
                                    detail = format!("{port_save:?}");
                                }
                            }
                            if outcome != Outcome::Equal && outcome != Outcome::EqualError {
                                keep_difference(&scratch, game.name, &archive_path, name, json.as_ref(), &save);
                            }
                            results.lock().unwrap().push((name.clone(), outcome, detail));
                        }
                    });
                }
            });
            let mut results = results.into_inner().unwrap();
            results.sort();
            let mut counts: BTreeMap<Outcome, usize> = BTreeMap::new();
            for (_, outcome, _) in &results {
                *counts.entry(*outcome).or_default() += 1;
                *totals.entry(*outcome).or_default() += 1;
            }
            let summary: Vec<String> = counts
                .iter()
                .map(|(outcome, count)| format!("{count} {}", outcome.name()))
                .collect();
            let line = format!(
                "{:<6} {}: {} files: {} ({:.0} s)",
                game.name,
                archive_path.file_name().unwrap_or_default().to_string_lossy(),
                results.len(),
                summary.join(", "),
                start.elapsed().as_secs_f64()
            );
            println!("{line}");
            report.push_str(&line);
            report.push('\n');
            let mut shown: HashMap<Outcome, usize> = HashMap::new();
            for (name, outcome, detail) in &results {
                if *outcome == Outcome::Equal {
                    continue;
                }
                let count = shown.entry(*outcome).or_default();
                *count += 1;
                if *count <= 5 || *outcome == Outcome::EqualError && *count <= 20 {
                    let line = format!("    {} {name}: {detail}", outcome.name());
                    println!("{line}");
                    report.push_str(&line);
                    report.push('\n');
                }
            }
            if !options.keep {
                // The oracle's output of the differences on request only.
            }
        }
    }
    let summary: Vec<String> = totals
        .iter()
        .map(|(outcome, count)| format!("{count} {}", outcome.name()))
        .collect();
    let line = format!("total: {}", summary.join(", "));
    println!("{line}");
    report.push_str(&line);
    report.push('\n');
    fs::create_dir_all(&scratch)?;
    fs::write(scratch.join("nif-report.txt"), report)?;
    Ok(())
}

/// Writes the port's outputs of a file that differs to the scratch folder.
fn keep_difference(
    scratch: &Path,
    game: &str,
    archive: &Path,
    name: &str,
    json: Option<&Result<Vec<u8>, String>>,
    save: &Result<Vec<u8>, String>,
) {
    let dir = scratch.join("nif-diff").join(game).join(
        archive
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .replace(' ', "_"),
    );
    let path = dir.join(name.replace('\\', "/"));
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Some(Ok(json)) = json {
        let _ = fs::write(path.with_extension(format!("{}.port.json", ext(name))), json);
    }
    if let Ok(save) = save {
        let _ = fs::write(path.with_extension(format!("{}.port", ext(name))), save);
    }
}

fn ext(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or_default()
}
