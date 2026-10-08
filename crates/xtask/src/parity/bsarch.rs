// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity bsarch`: the archive code against `BSArch.exe`.
//!
//! For each archive of the game folders (the ones up to `--max-size` MB, or
//! the ones named with `--archive`) it checks:
//!
//! - `list`: `bsarch <archive> -dump` prints the same bytes as the oracle's
//!   (the header, the flags and the table of every file).
//! - `unpack`: `bsarch unpack` writes the same files as the oracle.
//! - `pack`: the folder the oracle unpacked is packed by both into the
//!   archive format of the source (every compression the format has), and
//!   with `--cross` into every format; the archives are compared byte for
//!   byte, the oracle running with `-mt:no`, the port with all threads and
//!   with `-mt:no`, and the text of the run (the summary of the created
//!   archives, the warnings) is compared too.
//!
//! The texture archives (`DX10`) go through the same checks: `unpack` writes
//! the DDS files, and `pack` packs them into `-fo4dds` or `-sf1dds`.
//!
//! `--synthetic` adds packs of generated folders that the game archives do
//! not cover: several source folders and an archive merged, name filters,
//! flags, archives split by size, files without data, names that sort in
//! more than one way, identical files.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

use super::{GAMES, Game};

/// One check of one archive.
#[derive(Serialize)]
struct Outcome {
    game: String,
    archive: String,
    /// The format of the archive: `BSA 103`, `BTDX 1 GNRL`, `TES3`.
    format: String,
    /// `list`, `unpack` or `pack <switches>`.
    check: String,
    /// `equal`, `different`, `oracle-failed`, `port-failed` or `deferred`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// The bytes compared.
    bytes: u64,
}

#[derive(Serialize)]
struct Report<'a> {
    tag: &'a str,
    equal: usize,
    deferred: usize,
    total: usize,
    outcomes: &'a [Outcome],
}

struct Options {
    games: Vec<&'static Game>,
    all_games: bool,
    /// Lower-case archive names; empty selects by size.
    archives: Vec<String>,
    max_size: u64,
    jobs: usize,
    cross: bool,
    synthetic: bool,
    /// Only the texture archives (`DX10`).
    textures: bool,
    /// Keep the output of the runs that are equal.
    keep: bool,
}

struct Runner {
    oracle: PathBuf,
    port: PathBuf,
    scratch: PathBuf,
    keep: bool,
    cross: bool,
}

/// An archive of a game folder.
struct Case {
    game: &'static str,
    /// The switch that packs for the game (`fo3`, `fnv`, `tes5`).
    pack_switch: &'static str,
    path: PathBuf,
    name: String,
}

const USAGE: &str = "usage: cargo xtask parity bsarch [--game <game>]... [--archive <name>]... [--max-size <MB>] \
                     [--jobs <n>] [--cross] [--synthetic] [--textures] [--keep]";

fn parse(args: &[&str]) -> Result<Options> {
    let mut options = Options {
        games: Vec::new(),
        all_games: false,
        archives: Vec::new(),
        max_size: 40,
        jobs: 4,
        cross: false,
        synthetic: false,
        textures: false,
        keep: false,
    };
    let mut rest = args.iter();
    while let Some(&arg) = rest.next() {
        match arg {
            "--game" => {
                let name = rest.next().context(USAGE)?;
                options.games.push(
                    GAMES
                        .iter()
                        .find(|game| game.name == *name)
                        .with_context(|| format!("unknown game {name}"))?,
                );
            }
            "--archive" => options.archives.push(rest.next().context(USAGE)?.to_lowercase()),
            "--max-size" => options.max_size = rest.next().context(USAGE)?.parse()?,
            "--jobs" => options.jobs = rest.next().context(USAGE)?.parse::<usize>()?.max(1),
            "--cross" => options.cross = true,
            "--synthetic" => options.synthetic = true,
            "--textures" => options.textures = true,
            "--keep" => options.keep = true,
            _ => bail!(USAGE),
        }
    }
    if options.games.is_empty() {
        options.games = GAMES.iter().collect();
        options.all_games = true;
    }
    Ok(options)
}

/// The switch of `bsarch pack` that packs for the game of the corpus.
fn pack_switch(game: &str) -> &'static str {
    match game {
        "tes3" => "-tes3",
        "tes4" => "-tes4",
        "fo3" => "-fo3",
        "fnv" => "-fnv",
        "tes5" | "tes5vr" => "-tes5",
        "sse" => "-sse",
        "sf1" => "-sf1",
        _ => "-fo4",
    }
}

pub fn run(root: &Path, tag: &str, args: &[&str]) -> Result<()> {
    let options = parse(args)?;
    let oracle_dir = PathBuf::from(super::required_var("XEDIT_ORACLE_DIR")?);
    let oracle = oracle_dir.join("BSArch.exe");
    ensure!(oracle.exists(), "{} does not exist", oracle.display());
    let cache = super::cache_dir()?.join(tag);
    let scratch = match std::env::var_os("XEDIT_PARITY_SCRATCH") {
        Some(dir) => PathBuf::from(dir).join(tag).join("bsarch"),
        None => cache.join("bsarch-work"),
    };
    fs::create_dir_all(&scratch)?;
    let port = build_port(root)?;

    let mut cases = Vec::new();
    for &game in &options.games {
        let data = match (std::env::var_os(game.data_var), options.all_games) {
            (Some(data), _) => PathBuf::from(data),
            (None, true) => {
                println!("skipped       {}: {} is not set", game.name, game.data_var);
                continue;
            }
            (None, false) => bail!("environment variable {} is not set", game.data_var),
        };
        for entry in fs::read_dir(&data).with_context(|| format!("reading {}", data.display()))? {
            let path = entry?.path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let lower = name.to_lowercase();
            if !(lower.ends_with(".bsa") || lower.ends_with(".ba2")) {
                continue;
            }
            let size = fs::metadata(&path)?.len();
            let selected = if options.archives.is_empty() {
                size <= options.max_size * 1024 * 1024
            } else {
                options.archives.contains(&lower)
            };
            if selected && options.textures && !archive_format(&path)?.ends_with("DX10") {
                continue;
            }
            if selected {
                cases.push(Case {
                    game: game.name,
                    pack_switch: pack_switch(game.name),
                    path,
                    name,
                });
            }
        }
    }
    ensure!(!cases.is_empty() || options.synthetic, "no archive selected");
    // Largest first, so that the long runs start early.
    cases.sort_by_key(|case| std::cmp::Reverse(fs::metadata(&case.path).map(|m| m.len()).unwrap_or(0)));

    let runner = Runner {
        oracle,
        port,
        scratch,
        keep: options.keep,
        cross: options.cross,
    };
    let next = AtomicUsize::new(0);
    let outcomes = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.min(cases.len().max(1)) {
            scope.spawn(|| {
                while let Some(case) = cases.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let results = check_archive(case, &runner).unwrap_or_else(|error| {
                        vec![Outcome {
                            game: case.game.to_owned(),
                            archive: case.name.clone(),
                            format: String::new(),
                            check: "harness".to_owned(),
                            status: "oracle-failed",
                            detail: Some(format!("{error:#}")),
                            bytes: 0,
                        }]
                    });
                    for outcome in results {
                        print_outcome(&outcome);
                        outcomes.lock().unwrap().push(outcome);
                    }
                }
            });
        }
    });
    if options.synthetic {
        for outcome in synthetic(&runner)? {
            print_outcome(&outcome);
            outcomes.lock().unwrap().push(outcome);
        }
    }

    let mut outcomes = outcomes.into_inner().unwrap();
    outcomes.sort_by(|a, b| (&a.game, &a.archive, &a.check).cmp(&(&b.game, &b.archive, &b.check)));
    let count = |status: &str| outcomes.iter().filter(|o| o.status == status).count();
    let (equal, deferred) = (count("equal"), count("deferred"));
    let report = Report {
        tag,
        equal,
        deferred,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("bsarch.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;

    // The numbers by check and format.
    let mut table: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    for outcome in &outcomes {
        let check = outcome.check.split(' ').next().unwrap_or("").to_owned();
        let entry = table.entry((outcome.format.clone(), check)).or_default();
        entry.1 += 1;
        if outcome.status == "equal" {
            entry.0 += 1;
        }
    }
    for ((format, check), (equal, total)) in &table {
        println!("{format:16} {check:10} {equal} of {total} equal");
    }
    println!(
        "{equal} equal, {} different, {} failed, {deferred} deferred of {}. Report: {}",
        count("different"),
        count("oracle-failed") + count("port-failed"),
        outcomes.len(),
        report_file.display()
    );
    ensure!(
        equal + deferred == outcomes.len(),
        "the archives do not match the oracle"
    );
    Ok(())
}

fn print_outcome(outcome: &Outcome) {
    println!(
        "{:13} {} {} [{}]",
        outcome.status, outcome.game, outcome.archive, outcome.check
    );
    if let Some(detail) = &outcome.detail {
        println!("{detail}");
    }
}

fn build_port(root: &Path) -> Result<PathBuf> {
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "--package", "bsarch"])
        .current_dir(root)
        .status()?;
    ensure!(status.success(), "building bsarch failed");
    let target = match std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_relative() => root.join(dir),
        Some(dir) => dir,
        None => root.join("target"),
    };
    Ok(target
        .join("release")
        .join(format!("bsarch{}", std::env::consts::EXE_SUFFIX)))
}

/// What a run of a tool left.
struct Run {
    stdout: Vec<u8>,
    code: Option<i32>,
}

/// How long a run of a tool may take before it is taken for hung.
const RUN_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// How long a process may use no CPU time before it is taken for hung.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// The CPU time (user and kernel) a process has used.
#[cfg(windows)]
fn cpu_time(child: &std::process::Child) -> Option<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetProcessTimes;
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: the handle is the open handle of the child, the out pointers
    // are valid FILETIMEs.
    let ok = unsafe {
        GetProcessTimes(
            child.as_raw_handle() as _,
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    let ticks = |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    (ok != 0).then(|| ticks(kernel) + ticks(user))
}

#[cfg(not(windows))]
fn cpu_time(_child: &std::process::Child) -> Option<u64> {
    None
}

/// Runs the tool and collects what it writes. `BSArch.exe` was seen to hang
/// at its start (no CPU time for a quarter of an hour, a few runs of a batch
/// of thousands), so a run that uses no CPU for `IDLE_TIMEOUT` or takes over
/// `RUN_TIMEOUT` is stopped and run once more.
fn run_tool(exe: &Path, args: &[String]) -> Result<Run> {
    for attempt in 0..3 {
        let mut child = Command::new(exe)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("running {}", exe.display()))?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.read_to_end(&mut bytes);
            bytes
        });
        let started = Instant::now();
        let mut last_cpu = (cpu_time(&child), Instant::now());
        let mut last_sample = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if last_sample.elapsed() > Duration::from_secs(1) {
                last_sample = Instant::now();
                let cpu = cpu_time(&child);
                if cpu != last_cpu.0 {
                    last_cpu = (cpu, Instant::now());
                }
            }
            let idle = last_cpu.0.is_some() && last_cpu.1.elapsed() > IDLE_TIMEOUT;
            if idle || started.elapsed() > RUN_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let bytes = reader.join().unwrap_or_default();
        match status {
            Some(status) => {
                return Ok(Run {
                    stdout: bytes,
                    code: status.code(),
                });
            }
            None if attempt < 2 => println!("hung         {} {}: run again", exe.display(), args.join(" ")),
            None => bail!("{} did not finish: {}", exe.display(), args.join(" ")),
        }
    }
    unreachable!("three attempts")
}

/// The format of an archive from its header.
fn archive_format(path: &Path) -> Result<String> {
    let mut header = [0u8; 16];
    let mut file = File::open(path)?;
    let read = file.read(&mut header)?;
    ensure!(read >= 12, "{} is too short", path.display());
    Ok(if &header[..4] == b"BTDX" {
        let version = u32::from_le_bytes(header[4..8].try_into()?);
        format!("BTDX {version} {}", String::from_utf8_lossy(&header[8..12]))
    } else if &header[..4] == b"BSA\0" {
        format!("BSA {}", u32::from_le_bytes(header[4..8].try_into()?))
    } else {
        "TES3".to_owned()
    })
}

/// A pack: the switches, with a short label.
struct PackVariant {
    label: String,
    switches: Vec<&'static str>,
    /// The extension of the archive.
    extension: &'static str,
}

/// The formats an archive is packed into and the compressions of each.
fn pack_variants(format: &str, switch: &'static str, cross: bool) -> Vec<PackVariant> {
    let variant = |label: &str, switches: &[&'static str], extension: &'static str| PackVariant {
        label: label.to_owned(),
        switches: switches.to_vec(),
        extension,
    };
    let tes3 = vec![variant("tes3", &["-tes3"], "bsa")];
    let tes4 = vec![
        variant("tes4", &["-tes4"], "bsa"),
        variant("tes4 z", &["-tes4", "-z"], "bsa"),
    ];
    let fo3 = |switch: &'static str| {
        vec![
            variant(switch, &[switch], "bsa"),
            variant(&format!("{switch} z"), &[switch, "-z"], "bsa"),
        ]
    };
    let sse = vec![
        variant("sse", &["-sse"], "bsa"),
        variant("sse z", &["-sse", "-z"], "bsa"),
    ];
    let fo4 = vec![
        variant("fo4", &["-fo4"], "ba2"),
        variant("fo4 z", &["-fo4", "-z"], "ba2"),
    ];
    let sf1 = vec![
        variant("sf1", &["-sf1"], "ba2"),
        variant("sf1 z", &["-sf1", "-z"], "ba2"),
        variant("sf1 z:lz4", &["-sf1", "-z:lz4"], "ba2"),
    ];
    // The texture archives: uncompressed (which the oracle warns about) and
    // compressed.
    let fo4dds = vec![
        variant("fo4dds", &["-fo4dds"], "ba2"),
        variant("fo4dds z", &["-fo4dds", "-z"], "ba2"),
    ];
    let sf1dds = vec![
        variant("sf1dds", &["-sf1dds"], "ba2"),
        variant("sf1dds z", &["-sf1dds", "-z"], "ba2"),
        variant("sf1dds z:lz4", &["-sf1dds", "-z:lz4"], "ba2"),
    ];
    if cross {
        let mut all = tes3;
        all.extend(tes4);
        all.extend(fo3("-fo3"));
        all.extend(sse);
        all.extend(fo4);
        all.extend(sf1);
        all.extend(fo4dds);
        all.extend(sf1dds);
        return all;
    }
    if format.ends_with("DX10") {
        return if format.starts_with("BTDX 2 ") || format.starts_with("BTDX 3 ") {
            sf1dds
        } else {
            fo4dds
        };
    }
    if format.starts_with("TES3") {
        tes3
    } else if format == "BSA 103" {
        tes4
    } else if format == "BSA 104" {
        fo3(switch)
    } else if format == "BSA 105" {
        sse
    } else if format.starts_with("BTDX 1 ") || format.starts_with("BTDX 7 ") || format.starts_with("BTDX 8 ") {
        fo4
    } else {
        sf1
    }
}

fn check_archive(case: &Case, runner: &Runner) -> Result<Vec<Outcome>> {
    let format = archive_format(&case.path)?;
    // A short folder name: the oracle fails on paths over 260 characters, and
    // the archives of Fallout 76 have long ones.
    let hash = case.name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let work = runner.scratch.join(format!("{}-{:06x}", case.game, hash & 0xff_ffff));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work)?;
    let archive = case.path.display().to_string();
    let mut outcomes = Vec::new();
    let mut outcome = |check: &str, status: &'static str, detail: Option<String>, bytes: u64| {
        outcomes.push(Outcome {
            game: case.game.to_owned(),
            archive: case.name.clone(),
            format: format.clone(),
            check: check.to_owned(),
            status,
            detail,
            bytes,
        });
    };

    // list
    let oracle = run_tool(&runner.oracle, &[archive.clone(), "-dump".to_owned()])?;
    let port = run_tool(&runner.port, &[archive.clone(), "-dump".to_owned()])?;
    if oracle.stdout == port.stdout {
        outcome("list", "equal", None, oracle.stdout.len() as u64);
    } else {
        let detail = first_text_difference(&oracle.stdout, &port.stdout);
        fs::write(work.join("list.oracle.txt"), &oracle.stdout)?;
        fs::write(work.join("list.port.txt"), &port.stdout)?;
        outcome("list", "different", Some(detail), oracle.stdout.len() as u64);
    }

    // unpack
    let oracle_tree = work.join("unpack-oracle");
    let port_tree = work.join("unpack-port");
    fs::create_dir_all(&oracle_tree)?;
    fs::create_dir_all(&port_tree)?;
    let unpack = |exe: &Path, folder: &Path| {
        run_tool(
            exe,
            &["unpack".to_owned(), archive.clone(), folder.display().to_string()],
        )
    };
    let oracle = unpack(&runner.oracle, &oracle_tree)?;
    let port = unpack(&runner.port, &port_tree)?;
    if oracle.code != Some(0) {
        // An archive the oracle cannot unpack (a file flagged compressed that
        // is not, for example): the port has to stop the same way, with the
        // same message. The files written before the error depend on the
        // timing and are not compared.
        let last = |run: &Run| {
            String::from_utf8_lossy(&run.stdout)
                .split(['\r', '\n'])
                .rfind(|line| !line.is_empty())
                .unwrap_or("")
                .to_owned()
        };
        let (expected, got) = (last(&oracle), last(&port));
        if port.code == oracle.code && expected == got {
            outcome("unpack", "equal", Some(format!("  both stop with: {expected}")), 0);
        } else {
            outcome(
                "unpack",
                "different",
                Some(format!(
                    "  the oracle stopped with {:?}: {expected}\n  the port with {:?}: {got}",
                    oracle.code, port.code
                )),
                0,
            );
        }
    } else if port.code != Some(0) {
        outcome(
            "unpack",
            "port-failed",
            Some(format!(
                "  the port stopped with {:?}: {}",
                port.code,
                String::from_utf8_lossy(&port.stdout).lines().last().unwrap_or("")
            )),
            0,
        );
    } else {
        match compare_trees(&oracle_tree, &port_tree)? {
            (None, bytes) => outcome("unpack", "equal", None, bytes),
            (Some(detail), bytes) => outcome("unpack", "different", Some(detail), bytes),
        }
    }
    let _ = fs::remove_dir_all(&port_tree);

    // pack
    if oracle.code == Some(0) {
        let input = oracle_tree.display().to_string();
        for variant in pack_variants(&format, case.pack_switch, runner.cross) {
            let (status, detail, bytes) = pack_and_compare(runner, &work, &input, &variant)?;
            outcome(&format!("pack {}", variant.label), status, detail, bytes);
        }
    }
    let _ = fs::remove_dir_all(&oracle_tree);
    finish(&work, &mut outcomes, runner.keep)?;
    Ok(outcomes)
}

/// Removes the folder of a case that is equal throughout; names the folder
/// in the detail of a check that is not.
fn finish(work: &Path, outcomes: &mut [Outcome], keep: bool) -> Result<()> {
    if !keep && outcomes.iter().all(|o| matches!(o.status, "equal" | "deferred")) {
        let _ = fs::remove_dir_all(work);
        return Ok(());
    }
    for outcome in outcomes
        .iter_mut()
        .filter(|o| !matches!(o.status, "equal" | "deferred"))
    {
        let detail = outcome.detail.take().unwrap_or_default();
        if !detail.contains("kept in") {
            outcome.detail = Some(format!(
                "{detail}
  kept in {}",
                work.display()
            ));
        } else {
            outcome.detail = Some(detail);
        }
    }
    Ok(())
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The first line where two texts differ.
fn first_text_difference(oracle: &[u8], port: &[u8]) -> String {
    let (oracle, port) = (String::from_utf8_lossy(oracle), String::from_utf8_lossy(port));
    for (number, (a, b)) in oracle.lines().zip(port.lines()).enumerate() {
        if a != b {
            return format!("  line {}:\n    oracle: {a}\n    port:   {b}", number + 1);
        }
    }
    format!(
        "  one text is shorter: {} lines of the oracle, {} of the port",
        oracle.lines().count(),
        port.lines().count()
    )
}

/// Compares the files of two folders. Returns the first difference and the
/// bytes compared.
fn compare_trees(oracle: &Path, port: &Path) -> Result<(Option<String>, u64)> {
    let list = |root: &Path| -> Result<BTreeMap<String, PathBuf>> {
        let mut files = BTreeMap::new();
        for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
            let entry = entry?;
            if entry.file_type().is_file() {
                let relative = entry.path().strip_prefix(root)?.to_string_lossy().to_lowercase();
                files.insert(relative, entry.path().to_path_buf());
            }
        }
        Ok(files)
    };
    let (a, b) = (list(oracle)?, list(port)?);
    let mut bytes = 0;
    for (name, path) in &a {
        let Some(other) = b.get(name) else {
            return Ok((Some(format!("  {name} is missing from the port's folder")), bytes));
        };
        match files_equal(path, other)? {
            None => bytes += fs::metadata(path)?.len(),
            Some(offset) => return Ok((Some(format!("  {name} differs at offset {offset}")), bytes)),
        }
    }
    if let Some(name) = b.keys().find(|name| !a.contains_key(*name)) {
        return Ok((Some(format!("  {name} is only in the port's folder")), bytes));
    }
    Ok((None, bytes))
}

/// The offset of the first difference of two files, `None` when equal.
fn files_equal(a: &Path, b: &Path) -> Result<Option<u64>> {
    let (mut fa, mut fb) = (File::open(a)?, File::open(b)?);
    let (la, lb) = (fa.metadata()?.len(), fb.metadata()?.len());
    let mut offset = 0u64;
    let mut buffer_a = vec![0u8; 4 << 20];
    let mut buffer_b = vec![0u8; 4 << 20];
    loop {
        let read_a = read_full(&mut fa, &mut buffer_a)?;
        let read_b = read_full(&mut fb, &mut buffer_b)?;
        let common = read_a.min(read_b);
        if buffer_a[..common] != buffer_b[..common] {
            let at = buffer_a[..common]
                .iter()
                .zip(&buffer_b[..common])
                .position(|(x, y)| x != y)
                .unwrap_or(0);
            return Ok(Some(offset + at as u64));
        }
        if read_a != read_b {
            return Ok(Some(offset + common as u64));
        }
        if read_a == 0 {
            return Ok((la != lb).then_some(la.min(lb)));
        }
        offset += read_a as u64;
    }
}

fn read_full(file: &mut File, buffer: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        let read = file.read(&mut buffer[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(filled)
}

/// The lines of a run's text that do not depend on the run: without the
/// banner, the progress, the timings and the thread wording, with the output
/// folder replaced.
fn normalize_log(text: &[u8], folder: &str) -> String {
    let text = String::from_utf8_lossy(text).replace(folder, "<out>");
    text.split("\r\n")
        .filter(|line| !line.starts_with("Done in"))
        .map(|line| {
            // The progress is `\r`-separated percentages after a line break.
            let line = line.rsplit('\r').next().unwrap_or(line);
            let line = line.replace("Multithreaded", "Singlethreaded");
            if line.ends_with('%') && line.trim_end_matches('%').chars().all(|c| c.is_ascii_digit()) {
                String::new()
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Packs `input` with the oracle (`-mt:no`) and with the port (all threads
/// and `-mt:no`) and compares every archive written and the text of the run.
fn pack_and_compare(
    runner: &Runner,
    work: &Path,
    input: &str,
    variant: &PackVariant,
) -> Result<(&'static str, Option<String>, u64)> {
    let tag = sanitize(&variant.label);
    let oracle_dir = work.join(format!("pack-oracle-{tag}"));
    fs::create_dir_all(&oracle_dir)?;
    let pack = |exe: &Path, dir: &Path, extra: &[&str]| -> Result<Run> {
        let mut args = vec![
            "pack".to_owned(),
            input.to_owned(),
            dir.join(format!("out.{}", variant.extension)).display().to_string(),
        ];
        args.extend(variant.switches.iter().map(|s| (*s).to_owned()));
        args.extend(extra.iter().map(|s| (*s).to_owned()));
        run_tool(exe, &args)
    };
    let oracle = pack(&runner.oracle, &oracle_dir, &["-mt:no"])?;
    let archives = |dir: &Path| -> Result<BTreeMap<String, PathBuf>> {
        let mut found = BTreeMap::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            found.insert(path.file_name().unwrap().to_string_lossy().into_owned(), path);
        }
        Ok(found)
    };
    let oracle_files = archives(&oracle_dir)?;
    if oracle.code != Some(0) && oracle_files.is_empty() {
        // The oracle refuses the pack (for example a name without a folder
        // for the BSA formats): the port has to stop the same way.
        let port_dir = work.join(format!("pack-port-{tag}"));
        fs::create_dir_all(&port_dir)?;
        let port = pack(&runner.port, &port_dir, &[])?;
        let o = normalize_log(&oracle.stdout, &oracle_dir.display().to_string());
        let p = normalize_log(&port.stdout, &port_dir.display().to_string());
        let _ = fs::remove_dir_all(&oracle_dir);
        let _ = fs::remove_dir_all(&port_dir);
        return Ok(if o == p && port.code == oracle.code {
            (
                "equal",
                Some("  both refuse the pack with the same message".to_owned()),
                0,
            )
        } else {
            ("different", Some(first_text_difference(o.as_bytes(), p.as_bytes())), 0)
        });
    }
    let mut total = 0;
    for (label, extra) in [("threads", &[][..]), ("single thread", &["-mt:no"][..])] {
        let port_dir = work.join(format!("pack-port-{tag}-{}", sanitize(label)));
        fs::create_dir_all(&port_dir)?;
        let port = pack(&runner.port, &port_dir, extra)?;
        let port_files = archives(&port_dir)?;
        let mut problem = None;
        if port.code != oracle.code {
            problem = Some(format!(
                "  the port stopped with {:?}, the oracle with {:?}: {}",
                port.code,
                oracle.code,
                String::from_utf8_lossy(&port.stdout).lines().last().unwrap_or("")
            ));
        } else if oracle_files.keys().ne(port_files.keys()) {
            problem = Some(format!(
                "  the oracle wrote {:?}, the port {:?}",
                oracle_files.keys().collect::<Vec<_>>(),
                port_files.keys().collect::<Vec<_>>()
            ));
        } else {
            for (name, path) in &oracle_files {
                match files_equal(path, &port_files[name])? {
                    None => total += fs::metadata(path)?.len(),
                    Some(offset) => {
                        problem = Some(format!("  {name} differs at offset {offset} ({label})"));
                        break;
                    }
                }
            }
        }
        if problem.is_none() {
            let o = normalize_log(&oracle.stdout, &oracle_dir.display().to_string());
            let p = normalize_log(&port.stdout, &port_dir.display().to_string());
            if o != p {
                problem = Some(format!(
                    "  the text of the run differs ({label})\n{}",
                    first_text_difference(o.as_bytes(), p.as_bytes())
                ));
            }
        }
        if let Some(problem) = problem {
            return Ok((
                "different",
                Some(format!("{problem}\n  kept in {}", work.display())),
                total,
            ));
        }
        let _ = fs::remove_dir_all(&port_dir);
    }
    let _ = fs::remove_dir_all(&oracle_dir);
    Ok(("equal", None, total))
}

/// Packs of folders made up for the checks, which the game archives do not
/// cover.
fn synthetic(runner: &Runner) -> Result<Vec<Outcome>> {
    let root = runner.scratch.join("synthetic");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut random = move |length: usize| -> Vec<u8> {
        (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    };
    let text = |length: usize| -> Vec<u8> { (0..length).map(|i| b"abcdefgh \n"[i * 7 % 10]).collect() };
    let write = |path: PathBuf, data: &[u8]| -> Result<()> {
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, data)?;
        Ok(())
    };

    // A folder with names that sort in several ways, files that are alike,
    // empty files and a file without extension.
    let mixed = root.join("mixed");
    for (name, data) in [
        ("meshes/b.nif", text(5000)),
        ("meshes/A.nif", text(7000)),
        ("meshes/a2/y.nif", random(3000)),
        ("meshes/B3/q.nif", text(100)),
        ("meshes/sub/z.NIF", random(9000)),
        ("meshes/Sub2/w.nif", text(100)),
        ("meshes/Zed.nif", text(5000)),
        ("meshes/_x.nif", Vec::new()),
        ("meshes/0a.nif", vec![0]),
        ("meshes/noext", text(300)),
        ("textures/t/one.dds", random(70_000)),
        ("textures/t/two.dds", random(70_000)),
        ("textures/t/dup.dds", random(70_000)),
        ("sound/fx/a.wav", random(40_000)),
        ("sound/voice/p.esp/f/line.lip", random(2000)),
        ("sound/voice/p.esp/f/line.fuz", random(2000)),
        ("sound/voice/p.esp/f/line.xwm", random(2000)),
        ("strings/p_en.strings", text(2000)),
        ("scripts/s.pex", random(4000)),
        ("interface/i.swf", random(6000)),
        ("meshes/ünï.nif", text(111)),
    ] {
        write(mixed.join(name), &data)?;
    }
    let dup = fs::read(mixed.join("textures/t/dup.dds"))?;
    write(mixed.join("textures/t/dup2.dds"), &dup)?;
    write(mixed.join("textures/t/dup3.dds"), &dup)?;
    // A second folder that overrides a file and adds one, and a skipped file.
    let over = root.join("over");
    write(over.join("meshes/b.nif"), &text(123))?;
    write(over.join("meshes/new.nif"), &text(321))?;
    write(over.join("meshes/mod.esp"), b"skipped")?;
    write(over.join("meshes/dir/inner.nif"), &random(500))?;

    // A folder of textures only, which the BSA formats give the embedded names
    // flag, and which the Skyrim SE format must then compress without a split.
    let textures = root.join("textures-only");
    for index in 0..12u8 {
        write(
            textures.join(format!(
                "textures/t/tex{index:02}{}.dds",
                if index % 5 == 0 { "_e" } else { "" }
            )),
            &random(20_000 + usize::from(index) * 777),
        )?;
    }
    let textures = textures.display().to_string();
    let mixed = mixed.display().to_string();
    let over = over.display().to_string();
    let both = format!("{mixed}+{over}");
    let mut jobs: Vec<(String, String, Vec<&'static str>, &'static str)> = Vec::new();
    for (format, switch, extension) in [
        ("tes3", "-tes3", "bsa"),
        ("tes4", "-tes4", "bsa"),
        ("fo3", "-fo3", "bsa"),
        ("sse", "-sse", "bsa"),
        ("fo4", "-fo4", "ba2"),
        ("sf1", "-sf1", "ba2"),
    ] {
        jobs.push((format!("{format} folder"), mixed.clone(), vec![switch], extension));
        jobs.push((format!("{format} two folders"), both.clone(), vec![switch], extension));
        if format != "tes3" {
            jobs.push((format!("{format} z"), mixed.clone(), vec![switch, "-z"], extension));
            jobs.push((
                format!("{format} z two folders"),
                both.clone(),
                vec![switch, "-z"],
                extension,
            ));
            jobs.push((
                format!("{format} no share"),
                mixed.clone(),
                vec![switch, "-share:no"],
                extension,
            ));
            jobs.push((
                format!("{format} filter"),
                mixed.clone(),
                vec![switch, "-z", "-f:*.nif,*.dds"],
                extension,
            ));
        }
        // Each file in an archive of its own (a negative split size).
        jobs.push((
            format!("{format} split"),
            mixed.clone(),
            vec![switch, "-z", "-split:-1"],
            extension,
        ));
    }
    for (format, switch) in [("tes4", "-tes4"), ("fo3", "-fo3"), ("sse", "-sse")] {
        jobs.push((format!("{format} textures"), textures.clone(), vec![switch], "bsa"));
        jobs.push((
            format!("{format} textures z"),
            textures.clone(),
            vec![switch, "-z"],
            "bsa",
        ));
        jobs.push((
            format!("{format} textures no split"),
            textures.clone(),
            vec![switch, "-split:0"],
            "bsa",
        ));
        jobs.push((
            format!("{format} no split"),
            mixed.clone(),
            vec![switch, "-z", "-split:0"],
            "bsa",
        ));
    }
    jobs.push((
        "tes3 no split".to_owned(),
        mixed.clone(),
        vec!["-tes3", "-split:0"],
        "bsa",
    ));
    jobs.push((
        "fo4 split 1".to_owned(),
        mixed.clone(),
        vec!["-fo4", "-z", "-split:1"],
        "ba2",
    ));
    jobs.push(("sf1 lz4".to_owned(), mixed.clone(), vec!["-sf1", "-z:lz4"], "ba2"));
    jobs.push((
        "sf1 lz4 split".to_owned(),
        mixed.clone(),
        vec!["-sf1", "-z:lz4", "-split:-1"],
        "ba2",
    ));
    jobs.push((
        "sse flags".to_owned(),
        mixed.clone(),
        vec!["-sse", "-af:0x703", "-ff:0x1b"],
        "bsa",
    ));
    jobs.push((
        "fnv flags".to_owned(),
        mixed.clone(),
        vec!["-fnv", "-z", "-af:0x83", "-ff:0x113"],
        "bsa",
    ));
    jobs.push((
        "tes4 flags".to_owned(),
        mixed.clone(),
        vec!["-tes4", "-af:0x703", "-ff:0x3"],
        "bsa",
    ));
    // A source that is an archive of the oracle's: merged with the folder.
    let mut outcomes = Vec::new();
    for (label, input, switches, extension) in jobs {
        let variant = PackVariant {
            label: label.clone(),
            switches,
            extension,
        };
        let work = root.join(sanitize(&label));
        fs::create_dir_all(&work)?;
        let (status, detail, bytes) = pack_and_compare(runner, &work, &input, &variant)?;
        if status == "equal" && !runner.keep {
            let _ = fs::remove_dir_all(&work);
        }
        outcomes.push(Outcome {
            game: "synthetic".to_owned(),
            archive: "generated folders".to_owned(),
            format: variant
                .switches
                .first()
                .map_or(String::new(), |s| s.trim_start_matches('-').to_owned()),
            check: format!("pack {label}"),
            status,
            detail,
            bytes,
        });
    }
    Ok(outcomes)
}
