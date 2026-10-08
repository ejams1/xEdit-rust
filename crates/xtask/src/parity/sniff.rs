// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity sniff`: the operations of Sniff run on the corpus
//! archives by the port and by the oracle, compared file by file.
//!
//! Each case is an operation with the settings of its section in the
//! settings ini (`CASES`). For every archive of the games the operation
//! supports that holds files it processes, `Sniff.exe` of the release runs
//! in its automation mode (`-OP:<title> -I:<archive> -O:<folder> -S:<ini>
//! -LOG:<file> -skip:yes -threads:<n>`) and the port runs in this process
//! with the same settings, its outputs hashed as they come. Compared are:
//!
//! - the output of each file (an FNV-1a hash of the bytes written, or none),
//! - the error of each file Sniff skipped (`Skipped: <file>: <message>`;
//!   two access violations count as the same error, as the port names no
//!   address),
//! - the other lines of the log (the `Updated:` and `Unchanged:` lines and
//!   what the processor reports), as a multiset: Sniff's threads add their
//!   lines in the order they finish, the port in file order,
//! - the counts of the summary line, and the log file a processor writes
//!   itself (`{log}` in the settings is the path of that file).
//!
//! Sniff's threads share state and rarely fail a file with an access
//! violation that does not repeat when the file runs alone; such a file is
//! run again on its own (`-P:<file> -threads:1`) before it counts. The
//! oracle's results are cached in `<cache>/<tag>/sniff-oracle/<case>/`,
//! keyed by the archive and the settings. With `--sample <n>` the first `n`
//! files of each archive are unpacked into a folder, which is the input of
//! both (the loose file path of Sniff); without it the archive is.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use xedit_assets::sniff::main_form::{OutputSink, RunError, RunOptions, run as port_run};
use xedit_assets::sniff::processor::{GameType, MemIniFile, string_list_file_bytes, string_list_lines};
use xedit_assets::sniff::procs::PROCS;
use xedit_io::archive::Archive;
use xedit_io::encoding::ansi_string;

use super::nif::{archive_key, fnv, windows_path};
use super::{GAMES, Game, cache_dir, required_var};

const USAGE: &str = "usage: cargo xtask parity sniff [--case <name part>]... [--game <game>]... \
                     [--archive <name part>]... [--sample <n>] [--threads <n>] [--keep <n>] [--list]";

/// A run of an operation with its settings.
struct Case {
    /// The name of the case on the command line and in the report.
    name: &'static str,
    /// The title of the operation (`-OP:`).
    operation: &'static str,
    /// The values of the operation's section of the settings ini. `{log}`
    /// is replaced with the path of a log file the processor writes.
    settings: &'static [(&'static str, &'static str)],
}

/// The cases of the check: every ported operation with its defaults, and
/// with the settings that take other paths through it.
const CASES: &[Case] = &[
    Case {
        name: "tangents",
        operation: "Update tangents and binormals",
        settings: &[],
    },
    Case {
        name: "tangents-add",
        operation: "Update tangents and binormals",
        settings: &[("bAddIfMissing", "1"), ("bFaceNormals", "1")],
    },
    Case {
        name: "bounds",
        operation: "Update bounds",
        settings: &[],
    },
    Case {
        name: "replace-assets",
        operation: "Search and replace assets",
        settings: &[
            ("sReplacements", "textures\\#13#10tex\\#13#10.dds#13#10.DDS#13#10"),
            ("bFixAbsolute", "1"),
        ],
    },
    Case {
        name: "replace-assets-regexp",
        operation: "Search and replace assets",
        settings: &[
            (
                "sReplacements",
                "^(.+)_(d|n)\\.dds$#13#10$1_\\u2.dds#13#10#13#10data\\#13#10",
            ),
            ("bRegExp", "1"),
            ("bReportOnly", "1"),
        ],
    },
    Case {
        name: "json",
        operation: "Convert to and from JSON",
        settings: &[("bToJson", "1"), ("sDigits", "8"), ("iRotation", "1")],
    },
    Case {
        name: "tweaker",
        operation: "Universal tweaker",
        settings: &[],
    },
    Case {
        name: "tweaker-report",
        operation: "Universal tweaker",
        settings: &[
            ("bReportOnly", "1"),
            ("sBlocks", "NiAVObject"),
            ("bDescendants", "1"),
            ("sPath", "Name"),
            ("iValueMode", "3"),
            ("sValue", "x$1"),
            ("bOldValueCheck", "1"),
            ("iOldValueMode", "10"),
            ("sOldValue", "^(\\w+)Node"),
        ],
    },
    Case {
        name: "tweaker-math",
        operation: "Universal tweaker",
        settings: &[
            ("sBlocks", "NiAVObject"),
            ("bDescendants", "1"),
            ("sPath", "Transform\\Scale"),
            ("iValueMode", "2"),
            ("sValue", "1.5"),
            ("bOldValueCheck", "1"),
            ("sOldPath", "Flags"),
            ("iOldValueMode", "8"),
            ("sOldValue", "2"),
        ],
    },
    Case {
        name: "tweaker-array",
        operation: "Universal tweaker",
        settings: &[
            ("sBlocks", "BSShaderTextureSet"),
            ("sPath", "Textures\\[*]"),
            ("iValueMode", "4"),
            ("sValue", "x\\"),
            ("bOldValueCheck", "1"),
            ("iOldValueMode", "6"),
            ("sOldValue", "textures\\"),
        ],
    },
    Case {
        name: "fixer",
        operation: "Universal fixer",
        settings: &[("bSaveLog", "1"), ("sLogFile", "{log}")],
    },
    Case {
        name: "apply-transform",
        operation: "Apply transformation",
        settings: &[],
    },
    Case {
        name: "apply-transform-all",
        operation: "Apply transformation",
        settings: &[
            ("bSkipSkinned", "0"),
            ("bSkipAnimated", "0"),
            ("bSkipCollision", "0"),
            ("bSkipRoot", "0"),
            ("bSkipControllerManager", "0"),
        ],
    },
    Case {
        name: "adjust-transform",
        operation: "Adjust transformation",
        settings: &[("sPosZ", "10.5"), ("sScale", "2")],
    },
    Case {
        name: "adjust-transform-names",
        operation: "Adjust transformation",
        settings: &[
            ("sNames", "Bip01, Scene Root"),
            ("bExactMatch", "0"),
            ("iMode", "3"),
            ("sRotY", "45"),
            ("sPosX", "-3"),
        ],
    },
    Case {
        name: "attach-parent",
        operation: "Attach parent NiNode",
        settings: &[],
    },
    Case {
        name: "remove-nodes",
        operation: "Remove nodes",
        settings: &[("sNames", "EditorMarker"), ("bExactMatch", "0")],
    },
    Case {
        name: "remove-nodes-type",
        operation: "Remove nodes",
        settings: &[("iMode", "2"), ("sType", "NiStringExtraData")],
    },
    Case {
        name: "remove-unused",
        operation: "Remove unused nodes",
        settings: &[],
    },
    Case {
        name: "convert-block",
        operation: "Convert block type",
        settings: &[("sNodeFrom", "NiNode"), ("sNodeTo", "BSFadeNode"), ("bRoot", "1")],
    },
    Case {
        name: "convert-block-all",
        operation: "Convert block type",
        settings: &[("sNodeFrom", "BSFadeNode"), ("sNodeTo", "NiNode")],
    },
    Case {
        name: "unskin",
        operation: "Unskin mesh",
        settings: &[],
    },
    Case {
        name: "missing-names",
        operation: "Set missing names",
        settings: &[],
    },
    Case {
        name: "fix-kf",
        operation: "Fix 3DS exported KF",
        settings: &[],
    },
    Case {
        name: "merge-properties",
        operation: "Merge properties",
        settings: &[],
    },
    Case {
        name: "merge-properties-named",
        operation: "Merge properties",
        settings: &[
            ("bIgnoreName", "0"),
            (
                "sBlocks",
                "NiMaterialProperty,NiAlphaProperty,NiTexturingProperty,BSShaderPPLightingProperty,BSLightingShaderProperty",
            ),
        ],
    },
    Case {
        name: "lod-node",
        operation: "Add NiLODNode",
        settings: &[],
    },
    Case {
        name: "lod-node-screen",
        operation: "Add NiLODNode",
        settings: &[("sLODData", "NiScreenLODData"), ("sProportions", "0.5#13#10#13#100.25")],
    },
    Case {
        name: "bounding-box",
        operation: "Add bounding box",
        settings: &[("sCenterZ", "12.5"), ("sExtentX", "4"), ("sFlags", "4")],
    },
    Case {
        name: "root-collision",
        operation: "Add RootCollisionNode",
        settings: &[],
    },
    Case {
        name: "copy-priorities",
        operation: "Copy anim priorities",
        settings: &[("sSourceDirectory", "{source}")],
    },
    Case {
        name: "remove-controlled",
        operation: "Remove controlled blocks",
        settings: &[],
    },
    Case {
        name: "remove-controlled-others",
        operation: "Remove controlled blocks",
        settings: &[
            ("sNames", "Bip01 Spine, Tail"),
            ("bExactMatch", "0"),
            ("bNotMatching", "1"),
        ],
    },
    Case {
        name: "quadratic-to-linear",
        operation: "Quadratic to linear anim",
        settings: &[("sNames", "Bip01"), ("bExactMatch", "0")],
    },
    Case {
        name: "optimize-kf",
        operation: "Optimize Animations",
        settings: &[],
    },
    Case {
        name: "havok-settings",
        operation: "Update Havok settings",
        settings: &[
            ("sMass", "5"),
            ("sFriction", "0.25"),
            ("sRestitution", "0.4"),
            ("sRadius", " 0.1 "),
        ],
    },
    Case {
        name: "inertia",
        operation: "Update Havok inertia",
        settings: &[],
    },
    Case {
        name: "inertia-penetration",
        operation: "Update Havok inertia",
        settings: &[
            ("bPenetrationUpdate", "1"),
            ("bPenetrationStaticsUpdate", "1"),
            ("sDepthMult", "0.3"),
            ("sMult", "\"1 Head=4\",\"2 Body=\""),
        ],
    },
    Case {
        name: "ragdoll",
        operation: "Update ragdoll constraint",
        settings: &[],
    },
    Case {
        name: "ragdoll-convert",
        operation: "Update ragdoll constraint",
        settings: &[("bConvert", "1")],
    },
    Case {
        name: "havok-material",
        operation: "Search for Havok material",
        settings: &[],
    },
    Case {
        name: "havok-material-replace",
        operation: "Search for Havok material",
        settings: &[
            ("iGame", "1"),
            ("sMaterialSearch", "FO_HAV_MAT_STONE"),
            ("sMaterialReplace", "fo_hav_mat_metal"),
            ("bSkipRoot", "1"),
        ],
    },
    Case {
        name: "shader-flags",
        operation: "Update shader flags",
        settings: &[("iGame", "1"), ("iFlags", "3"), ("iFlags2", "16")],
    },
    Case {
        name: "shader-flags-report",
        operation: "Update shader flags",
        settings: &[("iGame", "0"), ("iMode", "2"), ("iFlags", "4096"), ("bReportOnly", "1")],
    },
    Case {
        name: "walls-reflection",
        operation: "Real Time Reflections - NVSE",
        settings: &[("sNormalIntensity", "0.5")],
    },
    Case {
        name: "check-errors",
        operation: "Check for errors",
        settings: &[("ProcessedFiles", "*.nif, *.kf")],
    },
    Case {
        name: "check-errors-all",
        operation: "Check for errors",
        settings: &[
            ("ProcessedFiles", "*.nif, *.kf"),
            ("Check NiAlphaProperty", "1"),
            ("Clamped tiling UVs", "1"),
            ("Optional checks", "1"),
            ("Repeated denegerate tris in strips", "1"),
            ("Unsupported mesh formats", "1"),
        ],
    },
    Case {
        name: "transform-info",
        operation: "Transform information",
        settings: &[],
    },
    Case {
        name: "transform-info-no-scale",
        operation: "Transform information",
        settings: &[("bRotation", "0"), ("bSkipEmpty", "0")],
    },
    Case {
        name: "havok-info",
        operation: "Havok information",
        settings: &[(
            "sFields",
            "\"Inertia Tensor\",Friction,\"Motion System\",\"Penetration Depth\"",
        )],
    },
    Case {
        name: "havok-info-same-line",
        operation: "Havok information",
        settings: &[
            ("bSameLine", "1"),
            ("sFields", "Restitution,\"Max Linear Velocity\",\"Inertia Tensor\""),
        ],
    },
    Case {
        name: "unwelded",
        operation: "Find unwelded vertices",
        settings: &[],
    },
    Case {
        name: "unwelded-report",
        operation: "Find unwelded vertices",
        settings: &[("sDistance", "0.01"), ("bSkipSame", "1"), ("bReportVertices", "1")],
    },
    Case {
        name: "draw-calls",
        operation: "Find excessive draw calls",
        settings: &[("sCallsNum", "3")],
    },
    Case {
        name: "find-uvs",
        operation: "Find UVs",
        settings: &[("sUMax", "1"), ("sVMax", "1.5")],
    },
    Case {
        name: "soft-particles",
        operation: "Vanilla Plus Particles - NVSE",
        settings: &[],
    },
];

/// The game of the harness for a game of Sniff.
fn harness_game(game: GameType) -> &'static str {
    match game {
        GameType::Tes3 => "tes3",
        GameType::Tes4 => "tes4",
        GameType::Fo3 => "fo3",
        GameType::Fnv => "fnv",
        GameType::Tes5 => "tes5",
        GameType::Sse => "sse",
        GameType::Fo4 => "fo4",
    }
}

struct Options {
    cases: Vec<String>,
    games: Vec<String>,
    archives: Vec<String>,
    sample: Option<usize>,
    threads: usize,
    keep: usize,
    list: bool,
}

fn parse(args: &[&str]) -> Result<Options> {
    let mut options = Options {
        cases: Vec::new(),
        games: Vec::new(),
        archives: Vec::new(),
        sample: None,
        threads: std::thread::available_parallelism().map_or(4, |count| count.get()),
        keep: 3,
        list: false,
    };
    let mut rest = args.iter();
    while let Some(&arg) = rest.next() {
        match arg {
            "--case" => options.cases.push(rest.next().context(USAGE)?.to_lowercase()),
            "--game" => options.games.push(rest.next().context(USAGE)?.to_lowercase()),
            "--archive" => options.archives.push(rest.next().context(USAGE)?.to_lowercase()),
            "--sample" => options.sample = Some(rest.next().context(USAGE)?.parse()?),
            "--threads" => options.threads = rest.next().context(USAGE)?.parse::<usize>()?.max(1),
            "--keep" => options.keep = rest.next().context(USAGE)?.parse()?,
            "--list" => options.list = true,
            _ => bail!(USAGE),
        }
    }
    Ok(options)
}

/// The settings ini of a case, with `{log}` as `log`.
fn settings_text(case: &Case, log: &Path, source: &Path) -> String {
    let section = case.operation.replace(' ', "");
    let mut text = format!("[Main]\r\nPopupWarning=0\r\n[{section}]\r\n");
    for (name, value) in case.settings {
        let value = value
            .replace("{log}", &windows_path(log))
            .replace("{source}", &windows_path(source));
        text.push_str(&format!("{name}={value}\r\n"));
    }
    text
}

/// What one side gave for one input (an archive or a sample folder).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
struct Results {
    /// The hash of each output, by lower-case path.
    outputs: BTreeMap<String, u64>,
    /// The error of each skipped file, by lower-case path.
    errors: BTreeMap<String, String>,
    /// The other lines of the log, sorted.
    log: Vec<String>,
    /// The lines of the processor's own log file, sorted.
    extra: Vec<String>,
    /// The counts of the summary line: updated and processed.
    updated: usize,
    processed: usize,
}

/// Whether an error of Sniff is a crash of its threads rather than of the
/// file: an access violation or an invalid pointer.
fn is_crash(message: &str) -> bool {
    message.starts_with("Access violation") || message.contains("Invalid pointer operation")
}

/// Two errors are the same; two access violations are.
fn same_error(a: &str, b: &str) -> bool {
    a == b || (a.starts_with("Access violation") && b.starts_with("Access violation"))
}

/// The file of a `Skipped: <file>: <message>` line: the path ends at the
/// first `: ` after one of the extensions.
fn split_skipped<'a>(rest: &'a str, extensions: &[String]) -> Option<(&'a str, &'a str)> {
    let lower = rest.to_lowercase();
    let end = extensions
        .iter()
        .filter_map(|ext| {
            let pattern = format!(".{ext}: ");
            lower.find(&pattern).map(|index| index + pattern.len() - 2)
        })
        .min()?;
    Some((&rest[..end], &rest[end + 2..]))
}

/// Reads the lines of a log into `results`: the skipped files, the summary
/// line and the rest.
fn read_log(lines: &[String], extensions: &[String], results: &mut Results) {
    for line in lines {
        if let Some(rest) = line.strip_prefix("Skipped: ")
            && let Some((name, message)) = split_skipped(rest, extensions)
        {
            results.errors.insert(name.to_lowercase(), message.to_owned());
            continue;
        }
        if let Some(rest) = line.strip_prefix("Done. Updated ") {
            // `Done. Updated %d files out of %d, elapsed time %s.`
            let mut numbers = rest
                .split(|c: char| !c.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .map(|part| part.parse::<usize>().unwrap_or(0));
            results.updated = numbers.next().unwrap_or(0);
            results.processed = numbers.next().unwrap_or(0);
            continue;
        }
        results.log.push(line.clone());
    }
    results.log.sort();
}

/// The lines of a text file as `TStringList.LoadFromFile` reads it.
fn file_lines(path: &Path) -> Vec<String> {
    match fs::read(path) {
        Ok(bytes) => string_list_lines(&ansi_string(&bytes)),
        Err(_) => Vec::new(),
    }
}

/// The outputs below `dir` by lower-case relative path, hashed.
fn hash_outputs(dir: &Path, outputs: &mut BTreeMap<String, u64>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(dir)?.to_string_lossy().replace('/', "\\");
        outputs.insert(relative.to_lowercase(), fnv(&fs::read(entry.path())?));
    }
    Ok(())
}

/// Runs Sniff on `input` and returns what it gave.
#[allow(clippy::too_many_arguments)]
fn run_sniff(
    sniff: &Path,
    work: &Path,
    case: &Case,
    source: &Path,
    input: &Path,
    path_filter: Option<&str>,
    threads: usize,
    extensions: &[String],
) -> Result<Results> {
    fs::create_dir_all(work)?;
    let exe = work.join("Sniff.exe");
    if !exe.exists() {
        fs::copy(sniff, &exe)?;
    }
    let out = work.join("oracle-out");
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out)?;
    let extra = work.join("oracle-extra.log");
    let _ = fs::remove_file(&extra);
    let ini = work.join("oracle.ini");
    fs::write(&ini, settings_text(case, &extra, source))?;
    let log = work.join("oracle.log");
    let _ = fs::remove_file(&log);
    let mut command = Command::new(&exe);
    command
        .current_dir(work)
        .arg(format!("-S:{}", windows_path(&ini)))
        .arg(format!("-OP:{}", case.operation))
        .arg(format!("-I:{}", windows_path(input)))
        .arg(format!("-O:{}", windows_path(&out)))
        .arg(format!("-LOG:{}", windows_path(&log)))
        .arg("-skip:yes")
        .arg("-all:no")
        .arg(if input.is_dir() { "-subdir:yes" } else { "-subdir:no" })
        .arg(format!("-threads:{threads}"));
    if let Some(filter) = path_filter {
        command.arg(format!("-P:{filter}"));
    }
    let mut child = command.spawn().context("starting Sniff")?;
    let size = if input.is_file() { fs::metadata(input)?.len() } else { 0 };
    let timeout = Duration::from_secs(600 + size / 1_000_000);
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
    ensure!(
        log.exists(),
        "Sniff wrote no log {} (an error before the run?)",
        log.display()
    );
    let mut results = Results::default();
    read_log(&file_lines(&log), extensions, &mut results);
    hash_outputs(&out, &mut results.outputs)?;
    results.extra = file_lines(&extra);
    results.extra.sort();
    let _ = fs::remove_dir_all(&out);
    Ok(results)
}

/// Runs the files whose error is a crash again, each alone on one thread,
/// and puts what they give in place of the crash.
fn rerun_crashes(
    sniff: &Path,
    work: &Path,
    case: &Case,
    source: &Path,
    input: &Path,
    extensions: &[String],
    results: &mut Results,
) -> Result<()> {
    let crashed: Vec<String> = results
        .errors
        .iter()
        .filter(|(_, message)| is_crash(message))
        .map(|(name, _)| name.clone())
        .collect();
    for name in crashed {
        let rerun = run_sniff(sniff, work, case, source, input, Some(&name), 1, extensions)?;
        let error = rerun.errors.get(&name).cloned();
        println!("oracle rerun  {name}: {}", error.as_deref().unwrap_or("no error"));
        results.errors.remove(&name);
        if let Some(error) = error {
            results.errors.insert(name.clone(), error);
        }
        if let Some(hash) = rerun.outputs.get(&name) {
            results.outputs.insert(name.clone(), *hash);
        }
        results.log.extend(rerun.log);
        results.log.sort();
        results.extra.extend(rerun.extra);
        results.extra.sort();
        results.updated += rerun.updated;
    }
    Ok(())
}

/// Runs the port on `input` with the settings of the case.
fn run_port(
    work: &Path,
    case: &Case,
    source: &Path,
    input: &Path,
    threads: usize,
    extensions: &[String],
) -> Result<Results> {
    let out = work.join("port-out");
    fs::create_dir_all(&out)?;
    let extra = work.join("port-extra.log");
    let _ = fs::remove_file(&extra);
    let settings = MemIniFile::from_text(&settings_text(case, &extra, source));
    let outputs: Arc<Mutex<BTreeMap<String, u64>>> = Arc::default();
    let sink_outputs = outputs.clone();
    let options = RunOptions {
        operation: case.operation.to_owned(),
        input: windows_path(input),
        output: windows_path(&out),
        path_contains: Some(String::new()),
        subdir: Some(true),
        skip_on_errors: Some(true),
        copy_all: Some(false),
        threads: Some(threads as i32),
        dry_run: false,
        sink: Some(OutputSink(Arc::new(move |name: &str, data: &[u8]| {
            sink_outputs.lock().unwrap().insert(name.to_lowercase(), fnv(data));
        }))),
    };
    let report = match port_run(Some(settings), &options) {
        Ok(report) => report,
        Err(RunError::Aborted { error, .. }) => bail!("the port stopped: {error}"),
        Err(error) => bail!("the port did not run: {error}"),
    };
    // The log as Sniff writes and reads it: in the ANSI code page.
    let lines = string_list_lines(&ansi_string(&string_list_file_bytes(&report.messages)));
    let mut results = Results::default();
    read_log(&lines, extensions, &mut results);
    results.outputs = std::mem::take(&mut *outputs.lock().unwrap());
    results.extra = file_lines(&extra);
    results.extra.sort();
    Ok(results)
}

/// The outcome of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    Equal,
    EqualError,
    Unchanged,
    OutputDifferent,
    PortOnly,
    OracleOnly,
    ErrorDifferent,
    PortFailed,
    OracleFailed,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Outcome::Equal => "equal",
            Outcome::EqualError => "equal-error",
            Outcome::Unchanged => "unchanged",
            Outcome::OutputDifferent => "output-different",
            Outcome::PortOnly => "port-only-output",
            Outcome::OracleOnly => "oracle-only-output",
            Outcome::ErrorDifferent => "error-different",
            Outcome::PortFailed => "port-failed",
            Outcome::OracleFailed => "oracle-failed",
        }
    }
}

/// The differences of two sorted line lists: the lines only one side has.
fn line_differences(port: &[String], oracle: &[String]) -> (Vec<String>, Vec<String>) {
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for line in port {
        *counts.entry(line).or_default() += 1;
    }
    for line in oracle {
        *counts.entry(line).or_default() -= 1;
    }
    let mut only_port = Vec::new();
    let mut only_oracle = Vec::new();
    for (line, count) in counts {
        for _ in 0..count.max(0) {
            only_port.push(line.to_owned());
        }
        for _ in 0..(-count).max(0) {
            only_oracle.push(line.to_owned());
        }
    }
    only_port.sort();
    only_oracle.sort();
    (only_port, only_oracle)
}

/// Unpacks the first `count` files of `archive` the operation processes
/// into `dir`, once.
fn sample_folder(archive: &Archive, dir: &Path, count: usize, extensions: &[String]) -> Result<()> {
    let done = dir.join(".complete");
    if done.exists() {
        return Ok(());
    }
    let _ = fs::remove_dir_all(dir);
    let mut taken = 0;
    for entry in archive.files() {
        if taken >= count {
            break;
        }
        let lower = entry.name.to_lowercase();
        if !extensions.iter().any(|ext| lower.ends_with(&format!(".{ext}"))) {
            continue;
        }
        let path = dir.join(entry.name.replace('\\', "/"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            &path,
            archive.unpack_entry(entry).map_err(|error| anyhow::anyhow!(error.0))?,
        )?;
        taken += 1;
    }
    fs::create_dir_all(dir)?;
    fs::write(done, "")?;
    Ok(())
}

/// The source folder of the operations that copy from the files of the same
/// path in another folder (`Copy anim priorities`): the files of the
/// archive with the priorities of their controlled blocks set to 33 by the
/// port's universal tweaker. Made once.
fn prepare_source(archive: &Archive, dir: &Path, extensions: &[String], threads: usize) -> Result<()> {
    let done = dir.join(".complete");
    if done.exists() {
        return Ok(());
    }
    let raw = dir.with_extension("raw");
    sample_folder(archive, &raw, usize::MAX, extensions)?;
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir)?;
    let settings = MemIniFile::from_text(
        "[Universaltweaker]\r\nProcessedFiles=*.kf\r\nsBlocks=NiControllerSequence\r\nsPath=Controlled Blocks\\[*]\\Priority\r\nsValue=33\r\n",
    );
    let options = RunOptions {
        operation: "Universal tweaker".to_owned(),
        input: windows_path(&raw),
        output: windows_path(dir),
        path_contains: Some(String::new()),
        subdir: Some(true),
        skip_on_errors: Some(true),
        copy_all: Some(false),
        threads: Some(threads as i32),
        dry_run: false,
        sink: None,
    };
    if let Err(error) = port_run(Some(settings), &options) {
        bail!("preparing {}: {error}", dir.display());
    }
    fs::write(done, "")?;
    Ok(())
}

pub fn run(tag: &str, args: &[&str]) -> Result<()> {
    let options = parse(args)?;
    if options.list {
        for case in CASES {
            println!("{:<24} {}", case.name, case.operation);
        }
        return Ok(());
    }
    let sniff = PathBuf::from(required_var("XEDIT_ORACLE_DIR")?).join("Sniff.exe");
    ensure!(sniff.exists(), "{} does not exist", sniff.display());
    let cache = cache_dir()?.join(tag).join("sniff-oracle");
    let scratch = match std::env::var_os("XEDIT_PARITY_SCRATCH") {
        Some(dir) => PathBuf::from(dir).join(tag),
        None => cache_dir()?.join(tag),
    };

    let mut totals: BTreeMap<Outcome, usize> = BTreeMap::new();
    let mut log_differences = 0;
    let mut report = String::new();
    let say = |line: String, report: &mut String| {
        println!("{line}");
        report.push_str(&line);
        report.push('\n');
    };

    for case in CASES {
        if !options.cases.is_empty() && !options.cases.iter().any(|part| case.name.contains(part.as_str())) {
            continue;
        }
        let entry = PROCS
            .iter()
            .find(|entry| entry.title == case.operation)
            .with_context(|| format!("no operation {}", case.operation))?;
        let proc = (entry
            .create
            .with_context(|| format!("{} is not ported", case.operation))?)();
        let extensions = proc.base().extensions.clone();
        let games: Vec<&'static Game> = proc
            .base()
            .supported_games
            .iter()
            .filter_map(|game| GAMES.iter().find(|known| known.name == harness_game(*game)))
            .filter(|game| options.games.is_empty() || options.games.iter().any(|name| name == game.name))
            .collect();
        let settings_key = fnv(settings_text(case, Path::new("{log}"), Path::new("{source}")).as_bytes());

        for game in games {
            let Some(data) = std::env::var_os(game.data_var).map(PathBuf::from) else {
                say(
                    format!("skipped       {}: {} is not set", game.name, game.data_var),
                    &mut report,
                );
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
                        say(
                            format!("skipped       {}: {error}", archive_path.display()),
                            &mut report,
                        );
                        continue;
                    }
                };
                let files = archive
                    .files()
                    .iter()
                    .filter(|entry| {
                        let lower = entry.name.to_lowercase();
                        extensions.iter().any(|ext| lower.ends_with(&format!(".{ext}")))
                    })
                    .count();
                if files == 0 {
                    continue;
                }
                let archive_name = archive_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .replace(' ', "_");
                let mut key = format!("{}-{settings_key:016x}", archive_key(&archive_path)?);
                let input = match options.sample {
                    Some(count) => {
                        key.push_str(&format!("-sample{count}"));
                        let dir = scratch
                            .join("sniff-sample")
                            .join(game.name)
                            .join(&archive_name)
                            .join(count.to_string());
                        sample_folder(&archive, &dir, count, &extensions)?;
                        dir
                    }
                    None => archive_path.clone(),
                };
                // The source folder of the operations that copy from one.
                let source = scratch.join("sniff-source").join(game.name).join(&archive_name);
                if case.settings.iter().any(|(_, value)| value.contains("{source}")) {
                    prepare_source(&archive, &source, &extensions, options.threads)?;
                }
                drop(archive);
                let work = scratch.join("sniff-work").join(case.name).join(&archive_name);

                // The oracle, from the cache or run now.
                let cached = cache.join(case.name).join(game.name).join(format!("{key}.json"));
                let start = Instant::now();
                let oracle = if cached.exists() {
                    serde_json::from_slice::<Results>(&fs::read(&cached)?)?
                } else {
                    let mut results =
                        run_sniff(&sniff, &work, case, &source, &input, None, options.threads, &extensions)?;
                    rerun_crashes(&sniff, &work, case, &source, &input, &extensions, &mut results)?;
                    fs::create_dir_all(cached.parent().unwrap())?;
                    fs::write(&cached, serde_json::to_vec(&results)?)?;
                    say(
                        format!(
                            "oracle        {} {} ({} outputs, {} errors, {:.0} s)",
                            case.name,
                            archive_path.display(),
                            results.outputs.len(),
                            results.errors.len(),
                            start.elapsed().as_secs_f64()
                        ),
                        &mut report,
                    );
                    results
                };

                let start = Instant::now();
                let port = match run_port(&work, case, &source, &input, options.threads, &extensions) {
                    Ok(port) => port,
                    Err(error) => {
                        say(
                            format!("port failed   {} {}: {error:#}", case.name, archive_path.display()),
                            &mut report,
                        );
                        *totals.entry(Outcome::PortFailed).or_default() += 1;
                        continue;
                    }
                };

                // File by file.
                let mut names: Vec<&String> = oracle
                    .outputs
                    .keys()
                    .chain(oracle.errors.keys())
                    .chain(port.outputs.keys())
                    .chain(port.errors.keys())
                    .collect();
                names.sort();
                names.dedup();
                let mut counts: BTreeMap<Outcome, usize> = BTreeMap::new();
                let mut shown: BTreeMap<Outcome, usize> = BTreeMap::new();
                let mut details = Vec::new();
                for name in names {
                    let outcome = match (
                        port.errors.get(name),
                        oracle.errors.get(name),
                        port.outputs.get(name),
                        oracle.outputs.get(name),
                    ) {
                        (Some(a), Some(b), _, _) if same_error(a, b) => Outcome::EqualError,
                        (Some(_), Some(_), _, _) => Outcome::ErrorDifferent,
                        (Some(_), None, _, _) => Outcome::PortFailed,
                        (None, Some(_), _, _) => Outcome::OracleFailed,
                        (None, None, Some(a), Some(b)) if a == b => Outcome::Equal,
                        (None, None, Some(_), Some(_)) => Outcome::OutputDifferent,
                        (None, None, Some(_), None) => Outcome::PortOnly,
                        (None, None, None, Some(_)) => Outcome::OracleOnly,
                        (None, None, None, None) => Outcome::Unchanged,
                    };
                    *counts.entry(outcome).or_default() += 1;
                    if !matches!(outcome, Outcome::Equal | Outcome::EqualError | Outcome::Unchanged) {
                        let seen = shown.entry(outcome).or_default();
                        *seen += 1;
                        if *seen <= options.keep.max(3) {
                            details.push(format!(
                                "    {} {name}: port {:?} {:?}, oracle {:?} {:?}",
                                outcome.name(),
                                port.outputs.get(name),
                                port.errors.get(name),
                                oracle.outputs.get(name),
                                oracle.errors.get(name)
                            ));
                        }
                    }
                }
                // The files that the summary counts but that wrote nothing.
                let silent = oracle.processed.saturating_sub(counts.values().sum());
                if silent > 0 {
                    *counts.entry(Outcome::Unchanged).or_default() += silent;
                }
                for (outcome, count) in &counts {
                    *totals.entry(*outcome).or_default() += count;
                }

                let (log_port, log_oracle) = line_differences(&port.log, &oracle.log);
                let (extra_port, extra_oracle) = line_differences(&port.extra, &oracle.extra);
                let counts_differ = port.updated != oracle.updated || port.processed != oracle.processed;
                let log_differs = !log_port.is_empty()
                    || !log_oracle.is_empty()
                    || !extra_port.is_empty()
                    || !extra_oracle.is_empty()
                    || counts_differ;
                if log_differs {
                    log_differences += 1;
                }
                let summary: Vec<String> = counts
                    .iter()
                    .map(|(outcome, count)| format!("{count} {}", outcome.name()))
                    .collect();
                say(
                    format!(
                        "{:<22} {:<5} {}: {} files: {}; log {} ({} lines){} ({:.0} s)",
                        case.name,
                        game.name,
                        archive_path.file_name().unwrap_or_default().to_string_lossy(),
                        files,
                        summary.join(", "),
                        if log_differs { "different" } else { "equal" },
                        oracle.log.len() + oracle.extra.len(),
                        if counts_differ {
                            format!(
                                ", counts port {}/{} oracle {}/{}",
                                port.updated, port.processed, oracle.updated, oracle.processed
                            )
                        } else {
                            String::new()
                        },
                        start.elapsed().as_secs_f64()
                    ),
                    &mut report,
                );
                for line in details {
                    say(line, &mut report);
                }
                for (label, lines) in [
                    ("log port only", &log_port),
                    ("log oracle only", &log_oracle),
                    ("log file port only", &extra_port),
                    ("log file oracle only", &extra_oracle),
                ] {
                    for line in lines.iter().take(options.keep.max(3)) {
                        say(format!("    {label}: {}", line.replace('\t', "\\t")), &mut report);
                    }
                }
            }
        }
    }
    let summary: Vec<String> = totals
        .iter()
        .map(|(outcome, count)| format!("{count} {}", outcome.name()))
        .collect();
    say(
        format!("total: {}; logs different: {log_differences}", summary.join(", ")),
        &mut report,
    );
    fs::create_dir_all(&scratch)?;
    fs::write(scratch.join("sniff-report.txt"), report)?;
    Ok(())
}
