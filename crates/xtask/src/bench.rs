// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask bench`: the timings of the phase 8 gate.
//!
//! The runner times the release build of the port's `xedit` binary on the
//! five gate workloads of `docs/PLAN.md` (load, reference build, conflict
//! scan, clean, archive pack) and the same operation on the Delphi build
//! of the baseline tag, so that the two columns of the gate table are
//! measured the same way. Every case repeats its measurement three times
//! and the median wall time is kept; the peak memory of the port comes from
//! the job object the parity harness already uses.
//!
//! The numbers of a run are only meaningful on a quiet machine: a build
//! running beside the benchmark steals the cores the measurement is about.
//! Every record therefore carries a `provisional` flag, which is on by
//! default and turned off with `--final`.
//!
//! ```text
//! cargo xtask bench [--list] [--case <name>]... [--final] [--out <file>]
//!                   [--repeats <n>]
//! ```
//!
//! Environment:
//!
//! - `XEDIT_FO4_DATA`, `XEDIT_SSE_DATA`: the data folders of the games the
//!   cases use. A case whose game is not installed is skipped and named.
//! - `XEDIT_ORACLE_DIR`: the unpacked release archive of the baseline tag,
//!   for the Delphi column.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

use xedit_io::dds::Dxgi;

use crate::memory::Limit;

/// How often a case repeats its measurement.
const REPEATS: usize = 3;

/// The name of the profile the published numbers use.
const PROFILE: &str = "release";

/// One timed run.
#[derive(Debug, Clone, Serialize)]
struct Run {
    /// Wall time of the run in seconds.
    seconds: f64,
    /// Peak memory of the process in bytes, where the platform reports it.
    #[serde(skip_serializing_if = "Option::is_none")]
    peak_bytes: Option<u64>,
    /// Exit code of the process.
    code: Option<i32>,
}

/// The measurements of one case under one configuration.
#[derive(Debug, Clone, Serialize)]
struct Measurement {
    /// The runs, in the order they ran.
    runs: Vec<Run>,
    /// The median wall time of the runs, in seconds.
    median_seconds: f64,
    /// The largest peak memory of the runs, in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    peak_bytes: Option<u64>,
}

/// The configuration of one measurement: the vector kernels on or off and
/// the thread count the port was given.
#[derive(Debug, Clone, Serialize)]
struct Variant {
    /// `XEDIT_SIMD`: `default` (the kernels of the machine) or `off` (the
    /// scalar route).
    simd: &'static str,
    /// The thread count the port was given: `default` (one per CPU) or the
    /// number.
    threads: &'static str,
}

/// One measurement of the port, under one configuration.
#[derive(Debug, Clone, Serialize)]
struct PortMeasurement {
    /// The configuration of the measurement.
    variant: Variant,
    /// The measurement itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    measurement: Option<Measurement>,
}

/// One case of the gate table.
#[derive(Debug, Clone, Serialize)]
struct Case {
    /// The name of the case, as `--case` names it.
    name: &'static str,
    /// The gate workload the case belongs to.
    workload: &'static str,
    /// What the case does, for the report.
    description: &'static str,
    /// The game the corpus is of.
    game: &'static str,
    /// The port's measurements, one per configuration.
    port: Vec<PortMeasurement>,
    /// The Delphi build's measurement of the same operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    delphi: Option<Measurement>,
    /// Why the Delphi column has no number, when it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    delphi_limitation: Option<String>,
    /// Notes on the case itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

/// The whole run.
#[derive(Debug, Serialize)]
struct Report {
    /// Schema of the file.
    schema: u32,
    /// The date of the run.
    date: String,
    /// The commit the port was built from.
    commit: String,
    /// The profile the port's binary was built with.
    profile: String,
    /// Whether the numbers are provisional.
    provisional: bool,
    /// The machine the numbers were taken on.
    machine: Machine,
    /// The cases.
    cases: Vec<Case>,
    /// Notes on the run as a whole.
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

/// The machine the numbers were taken on.
#[derive(Debug, Serialize)]
struct Machine {
    /// The CPU model, as the machine reports it.
    cpu: String,
    /// The number of logical processors.
    logical_cpus: usize,
    /// The vector instruction set the port's kernels run on.
    simd: String,
    /// The operating system.
    os: String,
}

/// The options of the run.
struct Options {
    /// The cases to run; every case when empty.
    cases: Vec<String>,
    /// Whether to print the list of cases and stop.
    list: bool,
    /// Whether the numbers are final.
    final_numbers: bool,
    /// Where the JSON goes; `docs/history/bench/` when not given.
    out: Option<PathBuf>,
    /// How often a case repeats its measurement.
    repeats: usize,
}

/// The port's side of a case.
enum PortWork {
    /// `xedit` loads the plugins and reports the load order.
    Load { game: &'static str, plugins: Vec<PathBuf> },
    /// `xedit refs build` builds the reference index.
    RefsBuild {
        game: &'static str,
        plugins: Vec<PathBuf>,
        cache: bool,
    },
    /// `xedit call conflicts.list` lists the conflicts.
    Conflicts { game: &'static str, plugins: Vec<PathBuf> },
    /// `xedit clean --quick --dry-run` counts what a quick auto clean
    /// would remove, the operation the Delphi build does in its quick
    /// auto clean mode.
    Clean { game: &'static str, plugin: PathBuf },
    /// `xedit archive pack` packs a folder.
    Pack {
        format: &'static str,
        source: PathBuf,
        compress: bool,
    },
    /// `xedit archive extract` unpacks an archive. The archive is made
    /// before the measurement, so that the timing is of the extraction
    /// alone.
    Extract {
        format: &'static str,
        source: PathBuf,
        compress: bool,
    },
}

/// The Delphi build's side of a case.
enum DelphiWork {
    /// `xDump.exe` loads the plugin and its masters; the time is the one
    /// the oracle's own clock writes when the load ends.
    Load { game: &'static str, plugin: PathBuf },
    /// The GUI build in its script mode, which the parity harness drives.
    Gui {
        game: &'static str,
        plugins: Vec<PathBuf>,
        script: &'static str,
        build_refs: bool,
    },
    /// The GUI build in its quick auto clean mode.
    QuickClean {
        game: &'static str,
        plugins: Vec<PathBuf>,
        target: String,
    },
    /// `BSArch.exe` packs a folder.
    Pack {
        format: &'static str,
        source: PathBuf,
        compress: bool,
    },
    /// `BSArch.exe` unpacks an archive.
    Unpack {
        format: &'static str,
        source: PathBuf,
        compress: bool,
    },
}

/// One case: the port's workload and the Delphi workload beside it.
struct Workload {
    name: &'static str,
    workload: &'static str,
    description: &'static str,
    game: &'static str,
    port: PortWork,
    delphi: Option<DelphiWork>,
    delphi_limitation: Option<String>,
    notes: Option<String>,
}

/// Runs `cargo xtask bench`.
pub fn run(root: &Path, args: &[&str]) -> Result<()> {
    let options = parse(args)?;
    let cases = all_cases(root)?;
    if options.list {
        for case in &cases {
            println!("{:<28} {:<16} {}", case.name, case.workload, case.description);
        }
        return Ok(());
    }
    let selected: Vec<&Workload> = if options.cases.is_empty() {
        cases.iter().collect()
    } else {
        let mut selected = Vec::new();
        for name in &options.cases {
            let case = cases
                .iter()
                .find(|case| case.name == name)
                .with_context(|| format!("no bench case named {name}"))?;
            selected.push(case);
        }
        selected
    };
    ensure!(!selected.is_empty(), "no bench case selected");

    let port = crate::parity::build_port_with_profile(root, PROFILE)?;
    let machine = machine_info();
    let mut results = Vec::new();
    for case in selected {
        println!("--- {} ({})", case.name, case.workload);
        let port = match measure_port(&port, case, options.repeats) {
            Ok(measurements) => {
                for entry in &measurements {
                    let seconds = entry
                        .measurement
                        .as_ref()
                        .map(|m| format!("{:.3} s", m.median_seconds))
                        .unwrap_or_else(|| "failed".to_owned());
                    println!(
                        "    port    simd={:<7} threads={:<7} {}",
                        entry.variant.simd, entry.variant.threads, seconds
                    );
                }
                measurements
            }
            Err(error) => {
                println!("    port    failed: {error:#}");
                Vec::new()
            }
        };
        let (delphi, limitation) = match measure_delphi(root, case, options.repeats) {
            Ok(measurement) => {
                println!(
                    "    delphi  {:.3} s (median of {})",
                    measurement.median_seconds, options.repeats
                );
                (Some(measurement), None)
            }
            Err(error) => {
                println!("    delphi  not measured: {error:#}");
                (None, Some(format!("{error:#}")))
            }
        };
        // The limitation of the case, when it has one, and the reason the
        // Delphi column has no number, when it has none.
        let limitation = match (case.delphi_limitation.as_deref(), limitation) {
            (Some(declared), Some(reason)) => Some(format!("{declared}; the column has no number: {reason}")),
            (Some(declared), None) => Some(declared.to_owned()),
            (None, Some(reason)) => Some(reason),
            (None, None) => None,
        };
        results.push(Case {
            name: case.name,
            workload: case.workload,
            description: case.description,
            game: case.game,
            port,
            delphi,
            delphi_limitation: limitation,
            notes: case.notes.clone(),
        });
    }

    let notes = run_notes(&results, options.final_numbers);
    let report = Report {
        schema: 1,
        date: date(),
        commit: git_commit(root)?,
        profile: PROFILE.to_owned(),
        provisional: !options.final_numbers,
        machine,
        cases: results,
        notes: Some(notes),
    };
    let out = match &options.out {
        Some(path) => path.clone(),
        None => {
            let dir = root.join("docs/history/bench");
            fs::create_dir_all(&dir)?;
            dir.join(format!("{}-{}.json", date(), report.commit))
        }
    };
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out, serde_json::to_vec_pretty(&report)?).with_context(|| format!("writing {}", out.display()))?;
    println!("report: {}", out.display());
    Ok(())
}

/// The note of the run as a whole: what the numbers mean and where the
/// Delphi column is not the same work as the port's column.
fn run_notes(cases: &[Case], final_numbers: bool) -> String {
    let mut notes = String::new();
    if final_numbers {
        notes.push_str("The numbers are final: the machine was quiet while the run was measured.");
    } else {
        notes.push_str(
            "The numbers are provisional: the run shared the machine with other builds, so they are a proof that the runner works and not a baseline. The final numbers are taken with `--final` on a quiet machine and replace this file.",
        );
    }
    let limited: Vec<&str> = cases
        .iter()
        .filter_map(|case| case.delphi_limitation.as_deref())
        .collect();
    if !limited.is_empty() {
        notes.push_str(" The Delphi column of these cases is not the same work as the port's column: ");
        notes.push_str(&limited.join("; "));
        notes.push('.');
    }
    notes
}

/// The options of the run.
fn parse(args: &[&str]) -> Result<Options> {
    let mut options = Options {
        cases: Vec::new(),
        list: false,
        final_numbers: false,
        out: None,
        repeats: REPEATS,
    };
    let mut rest = args;
    while let Some((first, tail)) = rest.split_first() {
        rest = tail;
        match *first {
            "--list" => options.list = true,
            "--final" => options.final_numbers = true,
            "--provisional" => options.final_numbers = false,
            "--case" => {
                let (value, tail) = rest.split_first().context("--case needs a name")?;
                rest = tail;
                options.cases.push((*value).to_owned());
            }
            "--out" => {
                let (value, tail) = rest.split_first().context("--out needs a path")?;
                rest = tail;
                options.out = Some(PathBuf::from(value));
            }
            "--repeats" => {
                let (value, tail) = rest.split_first().context("--repeats needs a count")?;
                rest = tail;
                options.repeats = value.parse().context("the repeat count is a number")?;
                ensure!(options.repeats > 0, "the repeat count is at least one");
            }
            other => bail!("unknown bench option {other}"),
        }
    }
    Ok(options)
}

/// The data folder of a game, `None` when it is not set.
fn data_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

/// The cases of the gate table.
///
/// The corpora are the ones the gate names: Fallout 4 with a DLC that has
/// BA2 string tables, a ten plugin load order, Skyrim SE with `Update.esm`
/// and a DLC for the conflicts.
fn all_cases(root: &Path) -> Result<Vec<Workload>> {
    let fo4 = data_dir("XEDIT_FO4_DATA");
    let sse = data_dir("XEDIT_SSE_DATA");
    let pack_source = pack_source(root)?;
    let mut cases = Vec::new();

    // Load.
    if let Some(data) = &fo4 {
        cases.push(Workload {
            name: "load-fo4",
            workload: "load",
            description: "load Fallout4.esm with the DLC that has BA2 string tables",
            game: "fo4",
            port: PortWork::Load {
                game: "fo4",
                plugins: vec![data.join("Fallout4.esm"), data.join("DLCworkshop03.esm")],
            },
            delphi: Some(DelphiWork::Load {
                game: "fo4",
                plugin: data.join("Fallout4.esm"),
            }),
            delphi_limitation: Some(
                "the Delphi column is the load of the game master by xDump.exe, which is the plugin \
                 with the string tables in its archive; the port's column loads the game master and \
                 the DLC, so the two columns are not the same work"
                    .to_owned(),
            ),
            notes: None,
        });
        // A ten plugin load order: the game master, the six DLC and three
        // of the Creation Club plugins.
        let mut plugins = vec![data.join("Fallout4.esm")];
        for name in [
            "DLCCoast.esm",
            "DLCNukaWorld.esm",
            "DLCRobot.esm",
            "DLCworkshop01.esm",
            "DLCworkshop02.esm",
            "DLCworkshop03.esm",
            "ccBGSFO4001-PipBoy(Black).esm",
            "ccBGSFO4004-PipBoy(Camo02).esm",
            "ccBGSFO4006-PipBoy(Chrome).esm",
        ] {
            if data.join(name).is_file() {
                plugins.push(data.join(name));
            }
        }
        cases.push(Workload {
            name: "load-fo4-order10",
            workload: "load",
            description: "load a ten plugin load order of Fallout 4",
            game: "fo4",
            port: PortWork::Load {
                game: "fo4",
                plugins: plugins.clone(),
            },
            delphi: Some(DelphiWork::Load {
                game: "fo4",
                plugin: data.join("Fallout4.esm"),
            }),
            delphi_limitation: Some(
                "the Delphi build has no ten plugin load order in the corpus: the column is the load \
                 of the game master alone, so the two columns are not the same work"
                    .to_owned(),
            ),
            notes: Some(format!("{} plugins", plugins.len())),
        });
    }
    if let Some(data) = &sse {
        cases.push(Workload {
            name: "load-sse",
            workload: "load",
            description: "load Skyrim.esm and Update.esm",
            game: "sse",
            port: PortWork::Load {
                game: "sse",
                plugins: vec![data.join("Skyrim.esm"), data.join("Update.esm")],
            },
            delphi: Some(DelphiWork::Load {
                game: "sse",
                plugin: data.join("Update.esm"),
            }),
            delphi_limitation: Some(
                "the Delphi column is the load of Update.esm by xDump.exe, which loads the game master \
                 with it; the port's column loads the game master and Update.esm, so the two columns \
                 are the same work"
                    .to_owned(),
            ),
            notes: None,
        });
    }

    // Reference build.
    if let Some(data) = &sse {
        cases.push(Workload {
            name: "refs-build-sse",
            workload: "reference build",
            description: "build the reference index of Skyrim.esm and Update.esm with the cache off",
            game: "sse",
            port: PortWork::RefsBuild {
                game: "sse",
                plugins: vec![data.join("Skyrim.esm"), data.join("Update.esm")],
                cache: false,
            },
            delphi: Some(DelphiWork::Gui {
                game: "sse",
                plugins: vec![data.join("Skyrim.esm"), data.join("Update.esm")],
                script: REFS_SCRIPT,
                build_refs: true,
            }),
            delphi_limitation: Some(
                "the Delphi column is the GUI build in its script mode: it includes the start of the \
                 program and the load of the plugins, so it is larger than the reference build alone"
                    .to_owned(),
            ),
            notes: None,
        });
    }

    // Conflict scan.
    if let Some(data) = &sse {
        cases.push(Workload {
            name: "conflicts-sse",
            workload: "conflict scan",
            description: "list the conflicts of Skyrim.esm, Update.esm and Dawnguard.esm",
            game: "sse",
            port: PortWork::Conflicts {
                game: "sse",
                plugins: vec![
                    data.join("Skyrim.esm"),
                    data.join("Update.esm"),
                    data.join("Dawnguard.esm"),
                ],
            },
            delphi: Some(DelphiWork::Gui {
                game: "sse",
                plugins: vec![
                    data.join("Skyrim.esm"),
                    data.join("Update.esm"),
                    data.join("Dawnguard.esm"),
                ],
                script: CONFLICTS_SCRIPT,
                build_refs: true,
            }),
            delphi_limitation: Some(
                "the Delphi column is the GUI build in its script mode: it includes the start of the \
                 program and the load of the plugins, so it is larger than the conflict scan alone"
                    .to_owned(),
            ),
            notes: None,
        });
    }

    // Clean.
    if let Some(data) = &sse {
        cases.push(Workload {
            name: "clean-sse",
            workload: "clean",
            description: "quick clean dry run of Dawnguard.esm on Skyrim SE",
            game: "sse",
            port: PortWork::Clean {
                game: "sse",
                plugin: data.join("Dawnguard.esm"),
            },
            delphi: Some(DelphiWork::QuickClean {
                game: "sse",
                plugins: vec![
                    data.join("Skyrim.esm"),
                    data.join("Update.esm"),
                    data.join("Dawnguard.esm"),
                ],
                target: "Dawnguard.esm".to_owned(),
            }),
            delphi_limitation: Some(
                "the Delphi column is the GUI build in its quick auto clean mode: it includes the \
                 start of the program, the load of the plugins and the save of the cleaned plugin, so \
                 it is larger than the dry run alone"
                    .to_owned(),
            ),
            notes: None,
        });
    }
    if let Some(data) = &fo4 {
        cases.push(Workload {
            name: "clean-fo4",
            workload: "clean",
            description: "quick clean dry run of DLCCoast.esm on Fallout 4",
            game: "fo4",
            port: PortWork::Clean {
                game: "fo4",
                plugin: data.join("DLCCoast.esm"),
            },
            delphi: Some(DelphiWork::QuickClean {
                game: "fo4",
                plugins: vec![data.join("Fallout4.esm"), data.join("DLCCoast.esm")],
                target: "DLCCoast.esm".to_owned(),
            }),
            delphi_limitation: Some(
                "the Delphi column is the GUI build in its quick auto clean mode: it includes the \
                 start of the program, the load of the plugins and the save of the cleaned plugin, so \
                 it is larger than the dry run alone"
                    .to_owned(),
            ),
            notes: None,
        });
    }

    // Archive pack.
    if fo4.is_some() {
        let texture_source = texture_source(root)?;
        cases.push(Workload {
            name: "pack-ba2-gnrl",
            workload: "archive pack",
            description: "pack a folder into a BA2 in the general format",
            game: "fo4",
            port: PortWork::Pack {
                format: "fo4",
                source: pack_source.clone(),
                compress: false,
            },
            delphi: Some(DelphiWork::Pack {
                format: "fo4",
                source: pack_source.clone(),
                compress: false,
            }),
            delphi_limitation: None,
            notes: None,
        });
        cases.push(Workload {
            name: "pack-ba2-dx10",
            workload: "archive pack",
            description: "pack a folder of textures into a BA2 in the DX10 format",
            game: "fo4",
            port: PortWork::Pack {
                format: "fo4dds",
                source: texture_source.clone(),
                compress: false,
            },
            delphi: Some(DelphiWork::Pack {
                format: "fo4dds",
                source: texture_source.clone(),
                compress: false,
            }),
            delphi_limitation: None,
            notes: None,
        });
    }
    if sse.is_some() {
        cases.push(Workload {
            name: "pack-bsa-sse",
            workload: "archive pack",
            description: "pack a folder into a compressed SSE BSA",
            game: "sse",
            port: PortWork::Pack {
                format: "sse",
                source: pack_source.clone(),
                compress: true,
            },
            delphi: Some(DelphiWork::Pack {
                format: "sse",
                source: pack_source.clone(),
                compress: true,
            }),
            delphi_limitation: None,
            notes: None,
        });
    }

    // Archive extract.
    if sse.is_some() {
        cases.push(Workload {
            name: "extract-bsa-sse",
            workload: "archive extract",
            description: "unpack a compressed SSE BSA",
            game: "sse",
            port: PortWork::Extract {
                format: "sse",
                source: pack_source.clone(),
                compress: true,
            },
            delphi: Some(DelphiWork::Unpack {
                format: "sse",
                source: pack_source.clone(),
                compress: true,
            }),
            delphi_limitation: None,
            notes: None,
        });
    }
    Ok(cases)
}

/// The folder the archive pack cases pack: a fixed set of files, so that
/// both builds pack the same bytes.
///
/// The folder is made of the corpus of the game where there is one (the
/// loose files of the game, a slice of it so that the run is not measured
/// on a copy of the whole install) and of files made up for the cases the
/// corpus does not cover.
fn pack_source(root: &Path) -> Result<PathBuf> {
    let source = root.join("target/bench/pack-source");
    // The marker file beside the folder tells a complete folder from a half
    // made one. It is not in the folder, so that it is not packed.
    let marker = root.join("target/bench/pack-source.marker");
    if marker.is_file() {
        return Ok(source);
    }
    let _ = fs::remove_dir_all(&source);
    for dir in ["meshes", "meshes/sub", "textures/t", "strings", "scripts"] {
        fs::create_dir_all(source.join(dir))?;
    }
    // A deterministic pseudo random generator, so that the folder is the
    // same on every machine and every run.
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
    for (name, data) in [
        ("meshes/a.nif", text(7000)),
        ("meshes/b.nif", text(5000)),
        ("meshes/sub/c.nif", text(9000)),
        ("strings/p_en.strings", text(2000)),
        ("scripts/s.pex", random(4000)),
    ] {
        let path = source.join(name);
        fs::write(path, data)?;
    }
    // The textures are real DDS files: a BA2 in the DX10 format holds only
    // textures, and a file that is not one is refused.
    for (name, format, width, height, mips) in [
        ("textures/t/one.dds", Dxgi::BC1_UNORM, 256, 256, 9),
        ("textures/t/two.dds", Dxgi::BC3_UNORM, 128, 128, 8),
    ] {
        let mut file = vec![0u8; xedit_io::dds::MAX_HEADER_SIZE];
        xedit_io::dds::set_up_header(&mut file, format, width, height, mips, false, false);
        let size = xedit_io::dds::header_size(&file);
        file.truncate(size);
        let bits = usize::from(xedit_io::dds::bits_per_pixel(format));
        file.extend(random(width as usize * height as usize * bits as usize / 8 / 4));
        let path = source.join(name);
        fs::write(path, file)?;
    }
    fs::write(&marker, b"the pack source of the bench runner\n")?;
    Ok(source)
}

/// The source folder of the DX10 case: only the textures of the general
/// folder, because a BA2 in the DX10 format refuses anything else.
fn texture_source(root: &Path) -> Result<PathBuf> {
    let general = pack_source(root)?;
    let source = root.join("target/bench/pack-source-dds");
    let marker = root.join("target/bench/pack-source-dds.marker");
    if marker.is_file() {
        return Ok(source);
    }
    let _ = fs::remove_dir_all(&source);
    copy_folder(&general.join("textures"), &source.join("textures"))?;
    fs::write(&marker, b"the texture source of the bench runner\n")?;
    Ok(source)
}

/// Copies a folder and everything in it.
fn copy_folder(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// The script the Delphi build runs for the reference build. It is the
/// script of the parity harness (`oracle/refs.pas`), which walks the
/// referenced-by list of every master record: the same walk the reference
/// build costs.
const REFS_SCRIPT: &str = include_str!("../oracle/refs.pas");

/// The script the Delphi build runs for the conflict scan. It is the script
/// of the parity harness (`oracle/conflicts.pas`).
const CONFLICTS_SCRIPT: &str = include_str!("../oracle/conflicts.pas");

/// The measurements of the port's side of a case, one per configuration.
///
/// The configurations are the vector kernels on and off (`XEDIT_SIMD`) and
/// the thread count the port was given: the default (one per CPU) and one
/// thread, where the command takes a thread count.
fn measure_port(port: &Path, case: &Workload, repeats: usize) -> Result<Vec<PortMeasurement>> {
    let mut results = Vec::new();
    for variant in variants_of(&case.port) {
        let measurement = measure_port_variant(port, case, repeats, &variant)?;
        results.push(PortMeasurement { variant, measurement });
    }
    Ok(results)
}

/// The configurations a workload runs under.
fn variants_of(work: &PortWork) -> Vec<Variant> {
    // The archive pack takes its own thread count, so the thread variants
    // of the other workloads do not apply to it.
    let threads: &[&str] = match work {
        PortWork::Pack { .. } | PortWork::Extract { .. } => &["default"],
        _ => &["default", "1"],
    };
    let mut variants = Vec::new();
    for simd in ["default", "off"] {
        for threads in threads {
            variants.push(Variant { simd, threads });
        }
    }
    variants
}

/// The measurements of the port's side of a case under one configuration.
fn measure_port_variant(
    port: &Path,
    case: &Workload,
    repeats: usize,
    variant: &Variant,
) -> Result<Option<Measurement>> {
    let mut runs = Vec::new();
    for _ in 0..repeats {
        let run = match &case.port {
            PortWork::Load { game, plugins } => {
                let mut command = Command::new(port);
                command.arg("--json").arg("--game").arg(game);
                for plugin in plugins {
                    command.arg("--load").arg(plugin);
                }
                add_threads(&mut command, variant);
                command.arg("call").arg("session.info");
                timed(command, variant)?
            }
            PortWork::RefsBuild { game, plugins, cache } => {
                let mut command = Command::new(port);
                command.arg("--json").arg("--game").arg(game);
                for plugin in plugins {
                    command.arg("--load").arg(plugin);
                }
                if !*cache {
                    command.arg("--dont-cache");
                }
                add_threads(&mut command, variant);
                command.arg("refs").arg("build");
                timed(command, variant)?
            }
            PortWork::Conflicts { game, plugins } => {
                let mut command = Command::new(port);
                command.arg("--json").arg("--game").arg(game);
                for plugin in plugins {
                    command.arg("--load").arg(plugin);
                }
                add_threads(&mut command, variant);
                command
                    .arg("call")
                    .arg("conflicts.list")
                    .arg("--params")
                    .arg(r#"{"include_single": false}"#);
                timed(command, variant)?
            }
            PortWork::Clean { game, plugin } => {
                let mut command = Command::new(port);
                command.arg("--json").arg("--game").arg(game).arg("--load").arg(plugin);
                add_threads(&mut command, variant);
                // The quick auto clean of the Delphi build: the same
                // operation, in a dry run so that nothing is written.
                command.arg("clean").arg("--quick").arg("--dry-run");
                timed(command, variant)?
            }
            PortWork::Pack {
                format,
                source,
                compress,
            } => {
                let out = source.with_extension(format!("bench-{format}.out"));
                let _ = fs::remove_file(&out);
                let mut command = Command::new(port);
                command
                    .arg("--json")
                    .arg("--edit")
                    .arg("archive")
                    .arg("pack")
                    .arg(&out)
                    .arg(source)
                    .arg("--format")
                    .arg(format);
                if *compress {
                    command.arg("--compress");
                }
                timed(command, variant)?
            }
            PortWork::Extract {
                format,
                source,
                compress,
            } => {
                // The archive is made before the measurement, so that the
                // timing is of the extraction alone.
                let archive = source.with_extension(format!("bench-{format}.out"));
                let _ = fs::remove_file(&archive);
                let mut pack = Command::new(port);
                pack.arg("--json")
                    .arg("--edit")
                    .arg("archive")
                    .arg("pack")
                    .arg(&archive)
                    .arg(source)
                    .arg("--format")
                    .arg(format);
                if *compress {
                    pack.arg("--compress");
                }
                let packed = timed(pack, variant)?;
                ensure!(packed.code == Some(0), "packing the archive failed");
                let output = source.with_extension(format!("bench-{format}-unpacked"));
                let _ = fs::remove_dir_all(&output);
                fs::create_dir_all(&output)?;
                let mut command = Command::new(port);
                command
                    .arg("--json")
                    .arg("--edit")
                    .arg("archive")
                    .arg("extract")
                    .arg(&archive)
                    .arg(&output);
                let extracted = timed(command, variant)?;
                ensure!(extracted.code == Some(0), "extracting the archive failed");
                extracted
            }
        };
        runs.push(run);
    }
    Ok(Some(summarize(runs)))
}

/// Adds the thread count of a variant to a command, when the variant names
/// one.
fn add_threads(command: &mut Command, variant: &Variant) {
    if variant.threads != "default" {
        command.arg("--threads").arg(variant.threads);
    }
}

/// The time the oracle's own clock writes when the load of a plugin ends.
///
/// `xDump.exe` writes the time since its start on every line of its error
/// output (`FormatDateTime('<hh:nn:ss.zzz>', Now - StartTime)`), and the
/// line that ends the load is the one that says so.
fn load_seconds(log: &str) -> Option<f64> {
    for line in log.lines() {
        let rest = line.strip_prefix('<')?;
        let (time, _) = rest.split_once('>')?;
        if !line.contains("Finished loading record") {
            continue;
        }
        let mut parts = time.split(':');
        let hours: f64 = parts.next()?.parse().ok()?;
        let minutes: f64 = parts.next()?.parse().ok()?;
        let seconds: f64 = parts.next()?.parse().ok()?;
        return Some(hours * 3600.0 + minutes * 60.0 + seconds);
    }
    None
}

/// The measurements of the Delphi side of a case.
fn measure_delphi(root: &Path, case: &Workload, repeats: usize) -> Result<Measurement> {
    let oracle_dir = std::env::var_os("XEDIT_ORACLE_DIR")
        .map(PathBuf::from)
        .context("environment variable XEDIT_ORACLE_DIR is not set")?;
    let mut runs = Vec::new();
    for index in 0..repeats {
        let run = match &case.delphi {
            Some(DelphiWork::Load { game, plugin }) => {
                let exe = oracle_dir.join("xDump.exe");
                ensure!(exe.is_file(), "{} does not exist", exe.display());
                let data = plugin.parent().context("a plugin without a folder")?;
                // `xDump.exe` writes the time of every step on its error
                // output; the line that ends the load is the one the
                // measurement is of. The group list is empty, so the dump
                // itself writes nothing: the run is of the load alone.
                let mut command = Command::new(&exe);
                command
                    .arg(format!("-{}", game.to_uppercase()))
                    .arg("-q")
                    .arg("-dg:ZZZZ")
                    .arg(format!("-D:{}", data.display()))
                    .arg(plugin);
                command.stdout(Stdio::null()).stderr(Stdio::piped());
                let output = command.output()?;
                let log = String::from_utf8_lossy(&output.stderr).into_owned();
                let seconds =
                    load_seconds(&log).with_context(|| format!("no load time in the log of {}", exe.display()))?;
                Run {
                    seconds,
                    peak_bytes: None,
                    code: output.status.code(),
                }
            }
            Some(DelphiWork::Gui {
                game,
                plugins,
                script,
                build_refs,
            }) => {
                let exe = oracle_dir.join(gui_exe_name(game));
                ensure!(exe.is_file(), "{} does not exist", exe.display());
                let work = root.join(format!("target/bench/delphi-{}-{}", case.name, index));
                let run = crate::parity::gui::GuiRun {
                    exe: &exe,
                    mode: gui_mode(game),
                    star_plugins_txt: !crate::parity::gui::simple_plugins_txt(gui_mode(game)),
                    plugins: plugins.clone(),
                    script: (*script).to_owned(),
                    build_refs: *build_refs,
                    work,
                    timeout: std::time::Duration::from_secs(60 * 60),
                    hang_timeout: std::time::Duration::from_secs(600),
                    budget: memory_budget(),
                    expected_peak: 4 * crate::memory::GIB,
                    max_memory: 16 * crate::memory::GIB,
                    extra_args: Vec::new(),
                    discovery: false,
                    tool_name: "Edit".to_owned(),
                };
                let started = Instant::now();
                let result = run.run()?;
                let seconds = started.elapsed().as_secs_f64();
                crate::parity::gui::remove_work(run.work.as_path());
                Run {
                    seconds,
                    peak_bytes: result.peak,
                    code: Some(0),
                }
            }
            Some(DelphiWork::QuickClean { game, plugins, target }) => {
                let exe = oracle_dir.join(gui_exe_name(game));
                ensure!(exe.is_file(), "{} does not exist", exe.display());
                let work = root.join(format!("target/bench/delphi-{}-{}", case.name, index));
                let run = crate::parity::gui::GuiRun {
                    exe: &exe,
                    mode: gui_mode(game),
                    star_plugins_txt: !crate::parity::gui::simple_plugins_txt(gui_mode(game)),
                    plugins: plugins.clone(),
                    script: String::new(),
                    build_refs: false,
                    work,
                    timeout: std::time::Duration::from_secs(60 * 60),
                    hang_timeout: std::time::Duration::from_secs(600),
                    budget: memory_budget(),
                    expected_peak: 4 * crate::memory::GIB,
                    max_memory: 16 * crate::memory::GIB,
                    extra_args: Vec::new(),
                    discovery: false,
                    tool_name: "Edit".to_owned(),
                };
                let started = Instant::now();
                let result = run.run_quick_clean(target)?;
                let seconds = started.elapsed().as_secs_f64();
                crate::parity::gui::remove_work(run.work.as_path());
                Run {
                    seconds,
                    peak_bytes: result.peak,
                    code: Some(0),
                }
            }
            Some(DelphiWork::Pack {
                format,
                source,
                compress,
            }) => {
                let exe = oracle_dir.join("BSArch.exe");
                ensure!(exe.is_file(), "{} does not exist", exe.display());
                let out = oracle_archive(source, format);
                let _ = fs::remove_file(&out);
                let mut command = Command::new(&exe);
                command.arg("pack").arg(source).arg(&out).arg(format!("-{}", format));
                if *compress {
                    command.arg("-z");
                }
                command.stdout(Stdio::null()).stderr(Stdio::null());
                timed(command, &default_variant())?
            }
            Some(DelphiWork::Unpack {
                format,
                source,
                compress,
            }) => {
                let exe = oracle_dir.join("BSArch.exe");
                ensure!(exe.is_file(), "{} does not exist", exe.display());
                // The archive is made before the measurement, so that the
                // timing is of the unpack alone.
                let archive = oracle_archive(source, format);
                let _ = fs::remove_file(&archive);
                let mut pack = Command::new(&exe);
                pack.arg("pack").arg(source).arg(&archive).arg(format!("-{}", format));
                if *compress {
                    pack.arg("-z");
                }
                pack.stdout(Stdio::null()).stderr(Stdio::null());
                let packed = pack.status()?;
                ensure!(packed.success(), "the oracle could not pack the archive");
                let folder = source.with_extension(format!("bench-{format}-oracle-unpacked"));
                let _ = fs::remove_dir_all(&folder);
                fs::create_dir_all(&folder)?;
                let mut command = Command::new(&exe);
                command.arg("unpack").arg(&archive).arg(&folder);
                command.stdout(Stdio::null()).stderr(Stdio::null());
                timed(command, &default_variant())?
            }
            None => bail!("this case has no Delphi counterpart"),
        };
        runs.push(run);
    }
    Ok(summarize(runs))
}

/// The configuration the Delphi build runs under: it has no vector switch
/// and no thread count of its own.
fn default_variant() -> Variant {
    Variant {
        simd: "default",
        threads: "default",
    }
}

/// The path of the archive the Delphi build makes of a source folder. The
/// oracle's archives are kept beside the port's, so that the two sides do
/// not write over each other.
fn oracle_archive(source: &Path, format: &str) -> PathBuf {
    source.with_extension(format!("bench-{format}.oracle.out"))
}

/// The memory budget the oracle runs share. The Delphi build is the one
/// that needs the room: it holds the whole load order in one process.
fn memory_budget() -> &'static crate::memory::Budget {
    static BUDGET: std::sync::OnceLock<crate::memory::Budget> = std::sync::OnceLock::new();
    BUDGET.get_or_init(|| {
        let total = crate::memory::physical_memory()
            .map(|total| total / 4 * 3)
            .unwrap_or(24 * crate::memory::GIB);
        crate::memory::Budget::new(total)
    })
}

/// The GUI executable of a game.
fn gui_exe_name(game: &str) -> &'static str {
    match game {
        "fo4" => "xFOEdit.exe",
        _ => "xTESEdit.exe",
    }
}

/// The mode of the GUI of a game: the switch of the command line and the
/// name of its settings file (`FO4`, `SSE`). The tag of the corpus is
/// lower case (`fo4`), the GUI needs the mode in upper case.
fn gui_mode(game: &str) -> &'static str {
    match game {
        "fo4" => "FO4",
        "sse" => "SSE",
        _ => "TES5",
    }
}

/// Runs a command and times it, with the peak memory of the process where
/// the platform reports it.
fn timed(mut command: Command, variant: &Variant) -> Result<Run> {
    if variant.simd == "off" {
        // The scalar route of the vector kernels, as the parity harness
        // runs a case with `XEDIT_SIMD=off`.
        command.env("XEDIT_SIMD", "off");
    }
    let started = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("running {}", command.get_program().to_string_lossy()))?;
    let limit = Limit::apply(&child, u64::MAX);
    let status = child.wait()?;
    let seconds = started.elapsed().as_secs_f64();
    let peak = limit.ok().and_then(|limit| limit.peak());
    Ok(Run {
        seconds,
        peak_bytes: peak,
        code: status.code(),
    })
}

/// The median of the runs and the largest peak memory among them.
fn summarize(runs: Vec<Run>) -> Measurement {
    let mut seconds: Vec<f64> = runs.iter().map(|run| run.seconds).collect();
    seconds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = if seconds.is_empty() {
        0.0
    } else if seconds.len() % 2 == 1 {
        seconds[seconds.len() / 2]
    } else {
        (seconds[seconds.len() / 2 - 1] + seconds[seconds.len() / 2]) / 2.0
    };
    let peak = runs.iter().filter_map(|run| run.peak_bytes).max();
    Measurement {
        runs,
        median_seconds: median,
        peak_bytes: peak,
    }
}

/// The CPU model, the number of logical processors and the vector
/// instruction set the kernels would run on.
fn machine_info() -> Machine {
    Machine {
        cpu: cpu_model(),
        logical_cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        simd: format!("{:?}", xedit_io::simd::detect()),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    }
}

/// The model of the CPU, as the machine reports it.
#[cfg(windows)]
fn cpu_model() -> String {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
    };
    /// The key of the CPU's description and the name of its model.
    const KEY: &str = r"HARDWARE\DESCRIPTION\System\CentralProcessor\0";
    const VALUE: &str = "ProcessorNameString";
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }
    let key_path = wide(KEY);
    let value_name = wide(VALUE);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: the paths are owned here and the handle is closed before the
    // function returns.
    unsafe {
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, key_path.as_ptr(), 0, KEY_READ, &raw mut key) != 0 {
            return "unknown".to_owned();
        }
        let mut size = 0u32;
        let mut kind = 0u32;
        let read = RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &raw mut kind,
            std::ptr::null_mut(),
            &raw mut size,
        );
        if read != 0 || size == 0 {
            RegCloseKey(key);
            return "unknown".to_owned();
        }
        let mut buffer = vec![0u16; size as usize / 2 + 1];
        let read = RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &raw mut kind,
            buffer.as_mut_ptr().cast(),
            &raw mut size,
        );
        RegCloseKey(key);
        if read != 0 {
            return "unknown".to_owned();
        }
        let text = String::from_utf16_lossy(&buffer);
        match text.find('\0') {
            Some(end) => text[..end].trim().to_owned(),
            None => text.trim().to_owned(),
        }
    }
}

/// The model of the CPU, as the machine reports it.
#[cfg(not(windows))]
fn cpu_model() -> String {
    "unknown".to_owned()
}

/// The date of the run, as `YYYY-MM-DD`.
fn date() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = now / 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The civil date of a count of days since the Unix epoch.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// The commit the working tree is at.
fn git_commit(root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .context("running git rev-parse")?;
    ensure!(output.status.success(), "git rev-parse failed");
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
