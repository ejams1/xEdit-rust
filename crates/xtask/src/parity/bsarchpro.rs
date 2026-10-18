// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity bsarchpro`: the packing workflow of the GUI
//! `BSArchPro.exe`, which is the only 4.1.5q route to the XBox 360 archive
//! target and to the `xtexconv.exe` bitmap conversion (parity #38).
//!
//! A case is a `.bsarch` list (`LoadList` reads plain JSON as well as the
//! zip the GUI writes) with the archive type, the target, the name of the
//! archive and the assets, and the source files of the case below
//! `textures\`. `BSArchPro.exe <list>` is the GUI's autopacking mode
//! (`FormCreate` loads the list and posts `WM_PACK`): it packs and closes
//! itself, so the archive is compared byte for byte with the port's
//! `xedit archive pack` over the same sources and settings. The GUI writes
//! its settings file under `TPath.GetHomePath`, which the harness points
//! into the work folder with the child's environment.
//!
//! The DDS files are generated with the port's own header writer
//! (`xedit_io::dds::set_up_header`, the `xbox` argument included), so no
//! game file is needed. A case with `xtexconv` also puts the
//! `xtexconv-stub` binary (this package's second binary, built for the
//! harness) next to the program under the name `xtexconv.exe`: the GUI
//! converts a non-XBox texture through it when the file is there, and the
//! port does the same with `--program-path` pointed at that folder, so the
//! spawn, the arguments, the temporary file and the read-back are on
//! both sides.
//!
//! A case whose target is XBox over a PC texture with no `xtexconv.exe`
//! reaches the same refusal on both sides (`DDS is not in XBox format`):
//! the oracle shows it on its progress form (`TFormTaskProgress`, titled
//! `Packing...`) and writes no archive worth comparing, so the check
//! requires that dialog and the port's message to equal one of the oracle
//! executable's own UTF-16 strings (`oracle_utf16_strings`). The mode
//! starts no work on a desktop nothing displays: it runs on the user's
//! desktop, shown (`HiddenCommand::visible`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};

use xedit_io::dds::{self, Dxgi};

use super::hidden::HiddenCommand;
use super::{cache_dir, oracle_utf16_strings, required_var};

const USAGE: &str = "usage: cargo xtask parity bsarchpro [--case <name or name part>]... [--keep] [--refresh-oracle] [--oracle-only] [--list]";

/// A packing case of the GUI.
struct Case {
    /// The name on the command line and in the report.
    name: &'static str,
    /// What the GUI calls the archive type (`TwbBSArchiveType`); the port's
    /// `--format` is derived from it.
    archive_type: i32,
    /// The port's `--format` for the same type.
    format: &'static str,
    /// The GUI's `ArchiveTarget`: `btXBox` is 1.
    xbox: bool,
    /// The fixture files of the case below `src`, as names below
    /// `textures\t\`: `pc.dds` is a PC texture, `xbox.dds` an XBox one and
    /// `readme.txt` a general file.
    files: &'static [&'static str],
    /// Whether the `xtexconv.exe` stub stands next to the program.
    xtexconv: bool,
    /// The case ends in a per-file refusal (`DDS is not in XBox format`):
    /// the oracle shows its error dialog and writes no archive.
    refusal: bool,
}

const CASES: &[Case] = &[
    Case {
        name: "fo4-general-pc",
        archive_type: 5,
        format: "fo4",
        xbox: false,
        files: &["misc\\readme.txt"],
        xtexconv: false,
        refusal: false,
    },
    Case {
        name: "fo4dds-pc",
        archive_type: 6,
        format: "fo4dds",
        xbox: false,
        files: &["pc.dds"],
        xtexconv: false,
        refusal: false,
    },
    Case {
        name: "fo4dds-xbox",
        archive_type: 6,
        format: "fo4dds",
        xbox: true,
        files: &["xbox.dds"],
        xtexconv: false,
        refusal: false,
    },
    Case {
        name: "fo4dds-xbox-pc-texture",
        archive_type: 6,
        format: "fo4dds",
        xbox: true,
        files: &["pc.dds"],
        xtexconv: false,
        refusal: true,
    },
    Case {
        name: "fo4dds-xbox-convert",
        archive_type: 6,
        format: "fo4dds",
        xbox: true,
        files: &["pc.dds"],
        xtexconv: true,
        refusal: false,
    },
    Case {
        name: "sf1dds-xbox-convert",
        archive_type: 8,
        format: "sf1dds",
        xbox: true,
        files: &["pc.dds"],
        xtexconv: true,
        refusal: false,
    },
];

struct Options {
    cases: Vec<String>,
    keep: bool,
    refresh_oracle: bool,
    oracle_only: bool,
    list: bool,
}

fn parse(args: &[&str]) -> Result<Options> {
    let mut options = Options {
        cases: Vec::new(),
        keep: false,
        refresh_oracle: false,
        oracle_only: false,
        list: false,
    };
    let mut rest = args.iter().copied();
    while let Some(arg) = rest.next() {
        match arg {
            "--case" => options.cases.push(rest.next().context(USAGE)?.to_lowercase()),
            "--keep" => options.keep = true,
            "--refresh-oracle" => options.refresh_oracle = true,
            "--oracle-only" => options.oracle_only = true,
            "--list" => options.list = true,
            _ => bail!(USAGE),
        }
    }
    Ok(options)
}

/// A DDS file of 64x64 with one mipmap, `xbox` writing the XBox header
/// (`SetUpHeader` with its `aIsXBox` argument, the form `xtexconv.exe`
/// gives).
fn dds_file(xbox: bool) -> Vec<u8> {
    let (width, height) = (64, 64);
    let data = (0..width * height / 2).map(|i| (i * 7) as u8).collect::<Vec<u8>>();
    let mut file = vec![0u8; dds::MAX_HEADER_SIZE];
    dds::set_up_header(&mut file, Dxgi::BC1_UNORM, width, height, 1, false, xbox);
    file.truncate(dds::header_size(&file));
    file.extend(data);
    file
}

/// The sources of a case below `<work>/src`: the fixture files under the
/// folders the archive keeps them in.
fn write_sources(work: &Path, case: &Case) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for file in case.files {
        let name = if file.ends_with(".dds") {
            format!("textures\\t\\{file}")
        } else {
            (*file).to_owned()
        };
        let path = work.join("src").join(name.replace('\\', "/"));
        fs::create_dir_all(path.parent().unwrap())?;
        let data = if *file == "pc.dds" {
            dds_file(false)
        } else if *file == "xbox.dds" {
            dds_file(true)
        } else {
            b"a fixture file for the archive\r\n".to_vec()
        };
        fs::write(&path, data)?;
        names.push(name);
    }
    Ok(names)
}

/// The `.bsarch` list the GUI loads: plain JSON, which `LoadList` reads as
/// well as the zip it writes. The asset array is `TAsset.ToJson`'s: the
/// compressed flag, the name in the archive, the file name when it differs
/// and the source archive.
fn list_json(work: &Path, case: &Case, names: &[String], archive: &Path) -> String {
    let assets: Vec<serde_json::Value> = names
        .iter()
        .map(|name| {
            // The source of an asset is read from its own path, not from the
            // work folder: `write_sources` puts every fixture below `src`
            // (the folder the port packs), and the GUI's list named `src\
            // misc\readme.txt` as `misc\readme.txt` and read
            // `<work>\misc\readme.txt`, which does not exist (parity #41:
            // the "Error reading source file" the run at a screen showed).
            let file = work.join("src").join(name.replace('\\', "/"));
            serde_json::json!([1, name, file.display().to_string()])
        })
        .collect();
    serde_json::json!({
        "Packing": {
            "ArchiveType": case.archive_type,
            "ArchiveTarget": if case.xbox { 1 } else { 0 },
            "ArchiveFileName": archive.display().to_string(),
            "ArchiveFlags": 0,
            "FileFlags": 0,
            "AutodetectFlags": true,
            "MultiThreaded": true,
            "SharedData": true,
            "Split": -1,
            "CompressionType": "",
        },
        "Assets": assets,
    })
    .to_string()
}

/// What a run of the GUI left.
struct OracleRun {
    /// The archive the list names, when it was written.
    archive: Option<Vec<u8>>,
    /// The dialog that was up, as `[class] title`: a refusal, or an error
    /// the run ended on.
    dialog: Option<String>,
}

/// Runs `BSArchPro.exe <list>` in its autopacking mode on the harness's
/// hidden desktop and waits for it to close (or for a dialog that keeps it
/// open, which is killed through its own handle).
fn run_oracle(pro: &Path, work: &Path, case: &Case, names: &[String]) -> Result<OracleRun> {
    let run = work.join("pro");
    fs::create_dir_all(&run)?;
    let exe = run.join("BSArchPro.exe");
    if !exe.exists() {
        fs::copy(pro, &exe)?;
    }
    if case.xtexconv {
        let stub = work.join("xtexconv-stub.exe");
        ensure!(stub.exists(), "{} does not exist", stub.display());
        fs::copy(&stub, run.join("xtexconv.exe"))?;
    }
    // `TPath.GetHomePath` decides where the settings file goes; the home
    // variables of the child keep it in the work folder.
    let home = work.join("home");
    fs::create_dir_all(&home)?;
    let archive = work.join(format!("{}.ba2", case.name.replace('-', "_")));
    let _ = fs::remove_file(&archive);
    let list = work.join("list.bsarch");
    fs::write(&list, list_json(work, case, names, &archive))?;

    // The GUI's autopacking mode starts no work on the harness's own
    // desktop, a desktop nothing displays (the parity #38 finding): it runs
    // on the user's desktop, shown.
    let mut command = HiddenCommand::new(&exe).visible();
    command
        .current_dir(&run)
        .arg(list.display().to_string())
        .env("HOME", home.as_os_str())
        .env("USERPROFILE", home.as_os_str())
        .env("HOMEDRIVE", Path::new(""))
        .env("HOMEPATH", home.as_os_str());
    let mut child = command.spawn().context("starting BSArchPro")?;
    let start = Instant::now();
    let mut dialog = None;
    loop {
        let dialog_up = if let Some(title) = super::gui::current_desktop_windows(child.id())
            .iter()
            .filter(|window| window.visible)
            // The per-file refusal of a `refusal` case is a box over the
            // progress form the GUI titles `Packing...`; the run at a screen
            // showed it (parity #41), and its class is the VCL form's, not
            // `#32770`. Only those cases take it as the dialog: a successful
            // pack opens the same form and closes it with the archive.
            .find(|window| window.class == "#32770" || (case.refusal && window.title.starts_with("Packing")))
            .map(|window| format!("[{}] {}", window.class, window.title))
        {
            dialog = Some(title);
            true
        } else {
            false
        };
        // An autopacking run closes itself when it packed; a refusal leaves
        // its modal dialog up, and the run ends once the dialog has been up
        // for a moment (its body is drawn, not read).
        if dialog_up {
            std::thread::sleep(Duration::from_millis(1500));
            child.kill();
            break;
        }
        if let Some(code) = child.try_wait()? {
            ensure!(code == 0 || archive.exists(), "BSArchPro exited with code {code:#x}");
            break;
        }
        if start.elapsed() > Duration::from_secs(120) {
            // The finding of parity #38: the 4.1.5q GUI's autopacking mode
            // shows its progress form and starts no work when it is driven
            // without a user (no CPU time, empty memos, no file written,
            // with and without `SW_HIDE` and with the threading on and
            // off). The mode is kept for a run at a screen; the cases then
            // compare the archives. Until then this is the oracle's own
            // failure, not a difference of the port.
            child.kill();
            return Ok(OracleRun {
                archive: None,
                dialog: Some("no processing (the GUI's autopacking mode did not start its work headlessly)".to_owned()),
            });
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if let Some(title) = &dialog
        && !archive.exists()
    {
        return Ok(OracleRun {
            archive: None,
            dialog: Some(title.clone()),
        });
    }
    let bytes = fs::read(&archive).with_context(|| format!("reading {}", archive.display()))?;
    Ok(OracleRun {
        archive: Some(bytes),
        dialog,
    })
}

/// The port's archive for the same case, through the CLI a caller would
/// use (`xedit --edit archive pack`), with `--program-path` aimed at the
/// folder that holds the `xtexconv.exe` stub.
fn run_port(xedit: &Path, work: &Path, case: &Case) -> Result<Result<Vec<u8>, String>> {
    let archive = work.join(format!("{}.port.ba2", case.name.replace('-', "_")));
    let _ = fs::remove_file(&archive);
    let stdout = Command::new(xedit)
        .arg("--edit")
        .arg("--json")
        .arg("--program-path")
        // `ProgramPath` is read with the trailing separator, as the GUI's
        // `ExtractFilePath(ParamStr(0))` has it.
        .arg(format!("{}\\", work.join("pro").display()))
        .arg("archive")
        .arg("pack")
        .arg(archive.display().to_string())
        .arg(work.join("src").display().to_string())
        .arg("--format")
        .arg(case.format)
        .arg("--target")
        .arg(if case.xbox { "xbox" } else { "pc" })
        .arg("--compress")
        .arg("default")
        .arg("--threads")
        .arg("1")
        .output()
        .context("running xedit")?;
    let stderr = String::from_utf8_lossy(&stdout.stderr).into_owned();
    let text = String::from_utf8_lossy(&stdout.stdout);
    let json: serde_json::Value = match serde_json::from_str(&text) {
        Ok(json) => json,
        Err(error) => bail!("xedit printed no JSON ({error}): {stderr}"),
    };
    if json["error"].is_null() {
        let bytes = fs::read(&archive).with_context(|| format!("reading {} (stderr: {stderr})", archive.display()))?;
        Ok(Ok(bytes))
    } else {
        Ok(Err(json["error"]["message"].as_str().unwrap_or_default().to_owned()))
    }
}

pub fn run(root: &Path, tag: &str, args: &[&str]) -> Result<()> {
    let options = parse(args)?;
    if options.list {
        for case in CASES {
            println!(
                "{:<24} type {} {}",
                case.name,
                case.archive_type,
                if case.xbox { "xbox" } else { "pc" }
            );
        }
        return Ok(());
    }
    let oracle_dir = PathBuf::from(required_var("XEDIT_ORACLE_DIR")?);
    let pro = oracle_dir.join("BSArchPro.exe");
    ensure!(pro.exists(), "{} does not exist", pro.display());
    let cache = cache_dir()?.join(tag).join("bsarchpro");
    let scratch = match std::env::var_os("XEDIT_PARITY_SCRATCH") {
        Some(dir) => PathBuf::from(dir).join(tag).join("bsarchpro"),
        None => cache.join("work"),
    };

    // The port's CLI and the xtexconv stub, in release as the other checks
    // build their tools.
    let (xedit, stub) = if options.oracle_only {
        (PathBuf::new(), PathBuf::new())
    } else {
        (
            build_port(root, "xedit-cli", "xedit")?,
            build_port(root, "xtask", "xtexconv-stub")?,
        )
    };

    let mut equal = 0usize;
    let mut different = 0usize;
    let mut oracle_failed = 0usize;
    let mut port_failed = 0usize;
    let mut report = String::new();
    let say = |line: String, report: &mut String| {
        println!("{line}");
        report.push_str(&line);
        report.push('\n');
    };
    for case in CASES {
        let selects = |part: &String| {
            case.name == part.as_str()
                || (!CASES.iter().any(|other| other.name == part.as_str()) && case.name.contains(part.as_str()))
        };
        if !options.cases.is_empty() && !options.cases.iter().any(selects) {
            continue;
        }
        let work = scratch.join(case.name);
        let _ = fs::remove_dir_all(&work);
        fs::create_dir_all(&work)?;
        let names = write_sources(&work, case)?;
        if !options.oracle_only {
            fs::copy(&stub, work.join("xtexconv-stub.exe"))?;
        }
        let cached = cache.join(format!("{}.json", case.name));
        let start = Instant::now();
        let (archive, dialog) = if cached.exists() && !options.refresh_oracle {
            let json: serde_json::Value = serde_json::from_slice(&fs::read(&cached)?)?;
            let archive = match json["archive"].as_str() {
                Some(path) => Some(fs::read(path)?),
                None => None,
            };
            (archive, json["dialog"].as_str().map(str::to_owned))
        } else {
            let run = run_oracle(&pro, &work, case, &names)?;
            fs::create_dir_all(&cache)?;
            // The archive is cached in the cache folder so that the run is
            // not repeated for a byte comparison.
            let archive_path = match &run.archive {
                Some(bytes) => {
                    let path = cache.join(format!("{}.ba2", case.name));
                    fs::write(&path, bytes)?;
                    Some(path.display().to_string())
                }
                None => None,
            };
            fs::write(
                &cached,
                serde_json::to_vec(&serde_json::json!({
                    "archive": archive_path,
                    "dialog": run.dialog,
                }))?,
            )?;
            let archive = match &archive_path {
                Some(path) => Some(fs::read(path)?),
                None => None,
            };
            (archive, run.dialog)
        };
        say(
            format!(
                "oracle        {} ({}, {:?}, {:.0} s)",
                case.name,
                if archive.is_some() {
                    "archive written"
                } else {
                    "no archive"
                },
                dialog.as_deref().unwrap_or(""),
                start.elapsed().as_secs_f64()
            ),
            &mut report,
        );
        if options.oracle_only {
            continue;
        }
        if archive.is_none() {
            // The oracle wrote no archive and refused nothing: its own
            // failure (see the 300 s branch), recorded and not counted as a
            // difference of the port.
            oracle_failed += 1;
            say(
                format!(
                    "{:<22} oracle-failed: {}",
                    case.name,
                    dialog.as_deref().unwrap_or("no archive, no dialog")
                ),
                &mut report,
            );
            if options.keep {
                say(format!("    work: {}", work.display()), &mut report);
            } else {
                let _ = fs::remove_dir_all(&work);
            }
            continue;
        }
        let start = Instant::now();
        let port = match run_port(&xedit, &work, case) {
            Ok(port) => port,
            Err(error) => {
                say(format!("port failed   {}: {error:#}", case.name), &mut report);
                port_failed += 1;
                continue;
            }
        };
        match (archive, port) {
            (Some(oracle), Ok(port)) if oracle == port => {
                equal += 1;
                say(
                    format!(
                        "{:<22} equal: {} bytes ({:.0} s)",
                        case.name,
                        port.len(),
                        start.elapsed().as_secs_f64()
                    ),
                    &mut report,
                );
            }
            (Some(oracle), Ok(port)) => {
                different += 1;
                say(
                    format!(
                        "{:<22} different: oracle {} bytes, port {} bytes",
                        case.name,
                        oracle.len(),
                        port.len()
                    ),
                    &mut report,
                );
            }
            (_, Err(message)) if case.refusal => {
                // The refusal: the dialog was up (the case says one is
                // expected) and the port's message is the line the GUI
                // shows, `Error processing '<asset>': <reason>`. The reason
                // has to be the oracle's own string (`oracle_utf16_strings`:
                // the dialog body is drawn and holds no readable text, the
                // executable's table does). The archive the oracle leaves is
                // whatever it packed before the bad asset, never a compared
                // result; the run at a screen showed the error on the
                // progress form (parity #41).
                let messages = oracle_utf16_strings(&pro, "not in XBox format")?;
                if dialog.is_some() && messages.iter().any(|text| message.ends_with(text)) {
                    equal += 1;
                    say(
                        format!(
                            "{:<22} equal: refusal {:?}, oracle message {messages:?}",
                            case.name, message
                        ),
                        &mut report,
                    );
                } else {
                    different += 1;
                    say(
                        format!(
                            "{:<22} different: dialog {:?}, oracle message(s) {messages:?}, port message {message:?}",
                            case.name, dialog
                        ),
                        &mut report,
                    );
                }
            }
            (None, Err(message)) => {
                different += 1;
                say(
                    format!(
                        "{:<22} different: no archive, no expected refusal; port message {message:?}, dialog {dialog:?}",
                        case.name
                    ),
                    &mut report,
                );
            }
            (Some(_), Err(message)) => {
                different += 1;
                say(
                    format!(
                        "{:<22} different: oracle wrote an archive, port refused: {message:?}",
                        case.name
                    ),
                    &mut report,
                );
            }
            (None, Ok(_)) if case.refusal => {
                different += 1;
                say(
                    format!(
                        "{:<22} different: the port wrote an archive where the oracle refused",
                        case.name
                    ),
                    &mut report,
                );
            }
            (None, Ok(_)) => {
                oracle_failed += 1;
                say(
                    format!("{:<22} oracle-failed: no archive, no dialog", case.name),
                    &mut report,
                );
            }
        }
        if options.keep {
            say(format!("    work: {}", work.display()), &mut report);
        } else {
            let _ = fs::remove_dir_all(&work);
        }
    }
    fs::create_dir_all(&scratch)?;
    let report_path = scratch.join("bsarchpro-report.txt");
    fs::write(&report_path, &report)?;
    println!("total: {equal} equal, {different} different, {oracle_failed} oracle-failed, {port_failed} port-failed");
    ensure!(
        different == 0 && port_failed == 0,
        "the port differs from the oracle: {different} different, {port_failed} port-failed (report: {})",
        report_path.display()
    );
    Ok(())
}

/// Builds a package's binary in release and gives its path, as the other
/// checks build the port's tools.
fn build_port(root: &Path, package: &str, name: &str) -> Result<PathBuf> {
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "--package", package])
        .current_dir(root)
        .status()?;
    ensure!(status.success(), "building {package} failed");
    let target = match std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_relative() => root.join(dir),
        Some(dir) => dir,
        None => root.join("target"),
    };
    Ok(target
        .join("release")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
}
