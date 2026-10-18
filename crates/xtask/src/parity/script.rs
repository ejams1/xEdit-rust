// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity script`: the phase-6 gate over the script corpus.
//!
//! The corpus is the `Edit Scripts` folder of the 4.1.5q oracle
//! (`XEDIT_SCRIPTS`, default `<XEDIT_ORACLE_DIR>/Edit Scripts`): 150 `.pas`
//! scripts, of which `xEditAPI.pas` only declares the API and never runs.
//! Three phases:
//!
//! - **parse**: every `.pas` of the folder. The oracle compiles each script
//!   (the GUI's script mode, `-script:`, `DoRunScript` -> `ApplyScript` ->
//!   `TxeScriptHost.CreateScript`, which is where `JvInterpreter` compiles)
//!   and the port checks it (`xedit script check`). "The same outcome" is
//!   whether the script compiled: the oracle's run reaches
//!   [`SCRIPT_END_LINE`] (posted after the script ran), or shows the
//!   `Applying script` line (`PerformLongAction` begins after the compile
//!   and before any statement runs), or stops on a dialog of the script's
//!   own (`Compile` runs the main block first: `Bookmark.pas` shows its
//!   bookmark list before that line exists). Only the application's own
//!   exception dialog -- caption [`APP_DIALOG_VERSION`], `Error in unit
//!   'X' on line N : ...` for a compile error or an access violation when
//!   the compiler faults -- means the script did not compile; the port
//!   exits 0 for a file it parses.
//! - **run**: the form-free corpus (`crates/xtask/oracle/scripts/runs.json`),
//!   each case naming the game and the plugins to load. The oracle runs the
//!   script on the hidden GUI with a private data folder and the script's
//!   folder as `wbScriptsPath` (upstream sets it to the folder of the
//!   script before running it, which is where `uses` units resolve from);
//!   the port runs `xedit script run`. "The same output" is: every file the
//!   script wrote (mapped below), the message log lines of the script
//!   section (`Start: Applying script` .. [`SCRIPT_END_LINE`], without the
//!   `[mm:ss] ` prefixes and the elapsed time), and the bytes of every
//!   plugin the case lists (`close_after` closes the GUI after the script,
//!   so its `SaveChanged` writes what the script changed into the private
//!   data folder, and the port's saved plugins are compared with them).
//! - **probe**: the language probes of `crates/xtask/oracle/scripts/probes/`
//!   (every `.pas` there whose name does not start with `Unit`; the `Unit*`
//!   files are the units some probes `uses`). Each is a small script that
//!   establishes one interpreter behaviour -- a variant conversion, an
//!   array bound, a `Format` argument, the expression stack, a routine
//!   terminator, a unit's encoding, an exception's class and line -- and
//!   both sides run it over the probe master the harness writes (the empty
//!   game master of the parse phase plus two `GMST` records, so `Process`
//!   runs). The comparison is the run phase's log comparison; nothing is
//!   written. A probe the oracle ends on a dialog (its exception dialog for
//!   a native fault, say) is `oracle-dialog`, a class and not a difference.
//!
//! Selection semantics, headless (derived from `DoRunScript` and
//! `ApplyScript`, and recorded again in `runs.json`): the script mode calls
//! `SelectRootNodes(vstNav)`, which selects **every root node** of the
//! navigation tree (every loaded file), and `ApplyScript` walks the
//! selection with `vstNav.GetLast(StartNode)` and `GetPrevious`, so `Process`
//! runs for every element whose `ElementType` is in `ScriptProcessElements`
//! (default `[etMainRecord]`) of every loaded file, the deepest last
//! descendant of a file's subtree first and the file's own node last. A
//! case that loads plugins therefore runs a `Process`-driven script over
//! every main record of them, in that order.
//!
//! Files a run leaves are captured by where they were written, because the
//! script chooses:
//!
//! - `out/`: `wbOutputPath` (the harness passes `-O:<work>out\`; upstream
//!   defaults it to the data path).
//! - `script/`: the run folder itself, which is `ScriptsPath()` (the folder
//!   of the script file) and the process's working directory.
//! - `program/`: next to the executable (`ProgramPath()`, the private `bin`
//!   folder; the copy of the executable and the GUI's own log are not
//!   captured).
//! - `data/`: the private data folder, less the plugins that went in; a
//!   plugin the case lists in `save` is captured as saved bytes instead.
//!
//! The oracle's result is cached under
//! `<cache>/<tag>/<MODE>-oracle-script/<stem>.<key>/`: `result.json` (the
//! class, the dialogs, the seconds, the peak, the file manifest), `log.txt`
//! and the captured files zstd-compressed. The key hashes the run recipe,
//! the script text, the units it `uses`, the plugins (masters included) and
//! the executable's name, so a change to any of them runs the oracle again.
//!
//! The port side is `port-pending` until the port CLI has the `script`
//! commands (phase 6 steps 2 and 4); it is probed once per run and never
//! faked. What it will run, per case:
//!
//! - parse: `xedit script check <file>` (exit 0 = parsed, else the
//!   `file:line: message` lines), with `XEDIT_SCRIPTS` set to the scripts
//!   folder so the units a script `uses` resolve there.
//! - run: `xedit --json --edit --game <MODE> --allow-edit-game-master
//!   --program-path <run>\bin\ --load <run>\Data\\<plugin>... script run
//!   <run>\<name> --scripts <run> [--build-refs]`, from the run folder as
//!   the working directory: the script and its units sit in the run folder
//!   itself (the oracle's `-script:<work>\<name>.pas`), the plugins are
//!   private copies below its `Data` (the oracle's `-D:` folder, from which
//!   the port derives its data path, so the string tables and `DataPath`
//!   messages see the same folder the oracle does), `bin` stands in for the
//!   oracle's program folder (`ProgramPath`), `--allow-edit-game-master` is
//!   the oracle's `-IKnowIllBreakMyGameWithThis`, `--build-refs` its load's
//!   `wbBuildRefs` for the cases that read `ReferencedBy`, and
//!   `result.messages` stands in for the message log. The comparison maps
//!   each side's run folder to `<root>` and the wall clock of a save suffix
//!   to `<time>` (see [`normalize_lines`]), because the two runs' absolute
//!   paths and clock digits differ by construction.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use super::gui::{self, GuiExtras, GuiRun, SCRIPT_END_LINE};
use super::oracle_save::{expected_peak, load_list, oracle_key};
use super::{GIB, Game, Runner, content_hash};

/// Part of the cache key and of the port's scratch folder: changes to what
/// a run captures or compares go here.
const RECIPE: &str = "script v6";

/// The switches every corpus run gets beyond the shared ones of `GuiRun`:
/// with `-IKnowWhatImDoing` (which `GuiRun` passes) this lets a script edit
/// the game master of the private copy (`TwbFile.GetIsEditable`:
/// `not (fsIsGameMaster in flStates) or wbAllowEditGameMaster`), which the
/// mutating cases need on both sides.
const EXTRA_ARGS: &[&str] = &["-IKnowIllBreakMyGameWithThis"];

/// The no-op script of the reference cache run of a `refcache` case: the
/// load builds the references and writes the cache; the script does
/// nothing.
const PREP_SCRIPT: &str = "refcache-prep.pas";
const PREP_SCRIPT_TEXT: &str =
    "unit RefCachePrep;\n\nfunction Initialize: integer;\nbegin\n  Result := 0;\nend;\n\nend.\n";

/// The reference cache files of a folder, by name.
fn cache_files_of(folder: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    if !folder.is_dir() {
        return Ok(files);
    }
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        if path.is_file() {
            let name = path
                .file_name()
                .context("a cache file without a name")?
                .to_string_lossy()
                .into_owned();
            files.push((name, fs::read(&path)?));
        }
    }
    files.sort();
    Ok(files)
}

/// The API declarations of the corpus: it parses but never runs.
const API_SCRIPT: &str = "xEditAPI.pas";

/// The version in the GUI's own window title: the caption of the
/// application's exception dialog (a compile error, a fault of the
/// compiler), which tells it from a message box of the script.
const APP_DIALOG_VERSION: &str = "4.1.5q";

/// The units the host compiles in: `xejviScriptHost.pas`
/// (`GetUnitSource`) answers `xEditAPI`, `UITypes` and every unit
/// registered with the JvInterpreter adapter with an empty stub, so a file
/// of the scripts folder is only read for the others. The names of the
/// namespace forms are stripped the same way (`system.`, `vcl.`,
/// `winapi.`, `data.`, `web.`).
const BUILT_IN_UNITS: &[&str] = &[
    "system",
    "sysutils",
    "classes",
    "dialogs",
    "windows",
    "graphics",
    "controls",
    "buttons",
    "stdctrls",
    "comctrls",
    "extctrls",
    "forms",
    "menus",
    "math",
    "variants",
    "strutils",
    "filectrl",
    "jsondataobjects",
    "shellapi",
    "clipbrd",
    "uitypes",
    "xeditapi",
];

/// How many differing entries an outcome lists before it summarizes.
const LISTED_DIFFERENCES: usize = 12;

const USAGE: &str = "usage: cargo xtask parity script [--script <name part>]... [--skip <name part>]... [--sample <n>] [--game <game>]... \
                     [--phase parse|run|probe]... [--oracle-only] [--refresh-oracle] [--keep] [--list] [--jobs <n>] \
                     [--memory-budget <GiB>] [--max-memory <GiB>] [--oracle-timeout <minutes>]";

struct Options {
    /// `--script`: scripts whose name holds one of these (without regard to
    /// case); empty selects the whole corpus.
    scripts: Vec<String>,
    /// `--skip`: scripts whose name holds one of these are left out (the
    /// slow ones, while the rest of the corpus is gone over).
    skip: Vec<String>,
    /// `--sample <n>`: the first `n` scripts of each phase (of the corpus
    /// order), for a quick run while porting.
    sample: Option<usize>,
    games: Vec<&'static Game>,
    all_games: bool,
    /// `--phase`: `parse`, `run`, `probe` or all three.
    parse: bool,
    run: bool,
    probe: bool,
    oracle_only: bool,
    refresh_oracle: bool,
    keep: bool,
    list: bool,
    jobs: usize,
    memory_budget: Option<u64>,
    max_memory: Option<u64>,
    oracle_timeout: Option<Duration>,
}

fn parse(args: &[&str]) -> Result<Options> {
    let mut options = Options {
        scripts: Vec::new(),
        skip: Vec::new(),
        sample: None,
        games: Vec::new(),
        all_games: false,
        parse: false,
        run: false,
        probe: false,
        oracle_only: false,
        refresh_oracle: false,
        keep: false,
        list: false,
        jobs: 3,
        memory_budget: None,
        max_memory: None,
        oracle_timeout: None,
    };
    let mut rest = args.iter();
    while let Some(&arg) = rest.next() {
        match arg {
            "--script" => {
                let name = rest.next().context(USAGE)?;
                options.scripts.push(name.to_lowercase());
            }
            "--skip" => {
                let name = rest.next().context(USAGE)?;
                options.skip.push(name.to_lowercase());
            }
            "--sample" => options.sample = Some(rest.next().context(USAGE)?.parse::<usize>()?),
            "--game" => {
                let name = rest.next().context(USAGE)?;
                let game = super::GAMES
                    .iter()
                    .find(|game| game.name == *name)
                    .with_context(|| format!("unknown game {name}"))?;
                options.games.push(game);
            }
            "--phase" => match rest.next().context(USAGE)?.to_lowercase().as_str() {
                "parse" => options.parse = true,
                "run" => options.run = true,
                "probe" => options.probe = true,
                other => bail!("unknown phase {other}; {USAGE}"),
            },
            "--oracle-only" => options.oracle_only = true,
            "--refresh-oracle" => options.refresh_oracle = true,
            "--keep" => options.keep = true,
            "--list" => options.list = true,
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
        options.games = super::GAMES.iter().collect();
        options.all_games = true;
    }
    if !options.parse && !options.run && !options.probe {
        options.parse = true;
        options.run = true;
        options.probe = true;
    }
    Ok(options)
}

fn gib(value: Option<&&str>) -> Result<u64> {
    let value: f64 = value.context(USAGE)?.parse()?;
    ensure!(value > 0.0, "a memory size must be positive");
    Ok((value * GIB as f64) as u64)
}

/// The folder the corpus is read from (`XEDIT_SCRIPTS`, default the
/// oracle's `Edit Scripts`).
fn scripts_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("XEDIT_SCRIPTS") {
        return Ok(PathBuf::from(dir));
    }
    let oracle = super::required_var("XEDIT_ORACLE_DIR")?;
    Ok(PathBuf::from(oracle).join("Edit Scripts"))
}

/// The file name of the game master of a mode (`xeInit.pas`,
/// `wbGameMasterEsm`), the plugin the module selection of the GUI forces
/// into every load.
fn game_master(mode: &str) -> &'static str {
    match mode {
        "TES3" => "Morrowind.esm",
        "TES4" => "Oblivion.esm",
        "FO3" => "Fallout3.esm",
        "FNV" => "FalloutNV.esm",
        "FO4" | "FO4VR" => "Fallout4.esm",
        "FO76" => "SeventySix.esm",
        "SF1" => "Starfield.esm",
        _ => "Skyrim.esm",
    }
}

/// A `TES4` header record: a valid plugin with no records, the game master
/// the parse cases load. Compiling a script does not read any record, so
/// the parse corpus runs on an empty one instead of a copy of the game's
/// master.
fn minimal_master(records: u32) -> Vec<u8> {
    let mut data = Vec::new();
    // HEDR: version 1.7 (Skyrim SE and later), records, next object ID.
    data.extend_from_slice(b"HEDR");
    data.extend_from_slice(&12u16.to_le_bytes());
    data.extend_from_slice(&1.7f32.to_le_bytes());
    data.extend_from_slice(&records.to_le_bytes());
    data.extend_from_slice(&0x0000_0800u32.to_le_bytes());
    // CNAM: the author, with the terminating zero counted.
    let author = b"xEdit parity\0";
    data.extend_from_slice(b"CNAM");
    data.extend_from_slice(&(author.len() as u16).to_le_bytes());
    data.extend_from_slice(author);
    let mut file = Vec::new();
    file.extend_from_slice(b"TES4");
    file.extend_from_slice(&(data.len() as u32).to_le_bytes());
    // The record flags of a master carry the ESM bit (`Skyrim.esm` has
    // 0x81); the later games keep the flag in a `DATA` subrecord, which the
    // Skyrim definitions refuse in the file header record.
    file.extend_from_slice(&0x81u32.to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes()); // FormID
    file.extend_from_slice(&0u32.to_le_bytes()); // version control
    file.extend_from_slice(&44u16.to_le_bytes()); // record version (Skyrim SE)
    file.extend_from_slice(&0u16.to_le_bytes()); // unknown
    file.extend_from_slice(&data);
    file
}

/// The probe master: the empty master with a `GMST` top group of two game
/// settings, so a probe's `Process` runs (over the two records, the last
/// first) and the closing line carries `Processed Records: 2`.
fn probe_master() -> Vec<u8> {
    fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut bytes = signature.to_vec();
        bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    }
    fn gmst(form_id: u32, editor_id: &[u8]) -> Vec<u8> {
        let mut data = sub_record(b"EDID", editor_id);
        data.extend(sub_record(b"DATA", &1.5f32.to_le_bytes()));
        let mut bytes = b"GMST".to_vec();
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes()); // flags
        bytes.extend_from_slice(&form_id.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes()); // version control
        bytes.extend_from_slice(&44u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&data);
        bytes
    }
    let records = [gmst(0x800, b"fProbeOne\0"), gmst(0x801, b"fProbeTwo\0")].concat();
    let mut file = minimal_master(2);
    file.extend_from_slice(b"GRUP");
    file.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    file.extend_from_slice(b"GMST");
    file.extend_from_slice(&0i32.to_le_bytes()); // group type: top
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&records);
    file
}

/// Writes the empty game master of `game` (or the probe master) into the
/// scratch folder and returns its path.
fn synthetic_master(runner: &Runner, game: &Game, probe: bool) -> Result<PathBuf> {
    let dir = runner.scratch.join(if probe { "synthetic-probe" } else { "synthetic" });
    fs::create_dir_all(&dir)?;
    let path = dir.join(game_master(game.mode));
    if !path.exists() {
        fs::write(&path, if probe { probe_master() } else { minimal_master(0) })?;
    }
    Ok(path)
}

/// The plugins a task loads, masters first: the empty master of a parse
/// case, the fixture master of a probe case (copied under the game
/// master's name by the staging of each side), or the case's plugins of the
/// game's data folder with their masters.
fn task_plugins(runner: &Runner, task: &Task) -> Result<Vec<PathBuf>> {
    if task.synthetic_master {
        return Ok(vec![synthetic_master(runner, task.game, task.phase == "probe")?]);
    }
    if let Some(master) = &task.master {
        let dir = runner.scratch.join("fixtures").join(&task.stem);
        fs::create_dir_all(&dir)?;
        let path = dir.join(game_master(task.game.mode));
        let bytes = fs::read(master).with_context(|| format!("reading {}", master.display()))?;
        if fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
            fs::write(&path, &bytes)?;
        }
        let mut paths = vec![path];
        for plugin in &task.plugins {
            let name = plugin.file_name().context("a fixture plugin without a name")?;
            let path = dir.join(name);
            let bytes = fs::read(plugin).with_context(|| format!("reading {}", plugin.display()))?;
            if fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
                fs::write(&path, &bytes)?;
            }
            paths.push(path);
        }
        return Ok(paths);
    }
    let names: Vec<&str> = task.load.iter().map(String::as_str).collect();
    load_list(task.game, &task.data, &names)
}

/// The class of a script: what it needs to run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    /// Runs headless: the run corpus.
    Headless,
    /// `TForm` or `ShowModal`: builds a form (the port's shim refuses it
    /// headless, and the oracle waits on it).
    Form,
    /// `InputQuery` or `InputBox`: asks for input.
    Interactive,
    /// The API declarations: parses, never runs.
    Api,
}

impl Class {
    fn name(self) -> &'static str {
        match self {
            Class::Headless => "headless",
            Class::Form => "form",
            Class::Interactive => "interactive",
            Class::Api => "api",
        }
    }
}

/// [`Class`] from the text of a script, as step 1's `corpus.json` records
/// it (the same greps: a `TForm` or a `ShowModal` builds a form, an
/// `InputQuery` or an `InputBox` asks, `xEditAPI` never runs).
fn class_of(name: &str, text: &str) -> Class {
    if name.eq_ignore_ascii_case(API_SCRIPT) {
        return Class::Api;
    }
    let lower = without_comments(text).to_lowercase();
    if word_in(&lower, "tform") || word_in(&lower, "showmodal") {
        return Class::Form;
    }
    if word_in(&lower, "inputquery") || word_in(&lower, "inputbox") {
        return Class::Interactive;
    }
    Class::Headless
}

/// `needle` (lower case) as a whole word of the lower case `text`:
/// `GetFormVersion` must not class a script as one that builds a `TForm`.
fn word_in(text: &str, needle: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(needle).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(is_word)
            && !text[at + needle.len()..].chars().next().is_some_and(is_word)
    })
}

/// The classification of step 1 (`crates/xtask/oracle/scripts/corpus.json`),
/// when it is there: it overrides the greps of [`class_of`].
#[derive(Deserialize)]
struct CorpusFile {
    #[serde(default)]
    scripts: Vec<CorpusEntry>,
}

#[derive(Deserialize)]
struct CorpusEntry {
    name: String,
    /// The class of a step 1 file that names one (`headless`, `form`,
    /// `interactive`, `api`).
    #[serde(default)]
    class: Option<String>,
    /// The step 1 file of the phase records the two greps instead: a script
    /// can be both a form and a dialog script, and the `api` flag marks
    /// `xEditAPI.pas`.
    #[serde(default)]
    form: bool,
    #[serde(default)]
    dialogs: bool,
    #[serde(default)]
    api: bool,
}

impl CorpusEntry {
    fn class_name(&self) -> &'static str {
        match &self.class {
            Some(class) => match class.to_lowercase().as_str() {
                "form" => "form",
                "interactive" => "interactive",
                "api" => "api",
                _ => "headless",
            },
            None if self.api => "api",
            None if self.form => "form",
            None if self.dialogs => "interactive",
            None => "headless",
        }
    }
}

/// The classes of `corpus.json` (step 1), by lower-case script name: it
/// overrides the greps of [`class_of`]. A file that cannot be read at all
/// leaves the greps in charge.
fn corpus_classes(root: &Path) -> std::collections::HashMap<String, String> {
    let path = root.join("crates/xtask/oracle/scripts/corpus.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return std::collections::HashMap::new();
    };
    match serde_json::from_str::<CorpusFile>(&text) {
        Ok(file) => file
            .scripts
            .into_iter()
            .map(|entry| (entry.name.to_lowercase(), entry.class_name().to_owned()))
            .collect(),
        Err(error) => {
            println!(
                "note: {} is not read ({error}); the greps class the scripts",
                path.display()
            );
            std::collections::HashMap::new()
        }
    }
}

/// The probes file (`crates/xtask/oracle/scripts/probes.json`): the probes
/// whose oracle outcome the gate accepts with a reason (a native fault, a
/// hang, a time-dependent value, a documented divergence).
#[derive(Deserialize)]
struct ProbesFile {
    #[allow(dead_code, reason = "documentation in the file")]
    description: String,
    #[serde(default)]
    accept: std::collections::HashMap<String, String>,
}

/// The parse case (`crates/xtask/oracle/scripts/parse.json`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParseCase {
    #[allow(dead_code, reason = "documentation in the file")]
    description: String,
    /// The game the oracle runs the parse corpus in; the scripts are
    /// game-agnostic to compile, and the empty master keeps the runs short.
    game: String,
    /// Scripts the oracle cannot answer but the port may parse anyway, with
    /// the reason: the outcome is `known-difference`, which is documented
    /// and does not fail the check (an upstream bug of the interpreter the
    /// port is not required to reproduce).
    #[serde(default)]
    accept: std::collections::BTreeMap<String, String>,
}

/// The run cases (`crates/xtask/oracle/scripts/runs.json`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunsFile {
    #[allow(dead_code, reason = "documentation in the file")]
    description: String,
    defaults: RunDefaults,
    cases: Vec<RunCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunDefaults {
    game: String,
    /// The plugins loaded unless a case names its own, masters first.
    #[serde(default)]
    load: Vec<String>,
    /// The oracle builds the reference information after loading (the
    /// script mode does by default); off unless a script reads
    /// `ReferencedBy`.
    #[serde(default)]
    build_refs: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunCase {
    script: String,
    #[serde(default)]
    game: Option<String>,
    #[serde(default)]
    load: Option<Vec<String>>,
    #[serde(default)]
    build_refs: Option<bool>,
    /// Close the GUI once the script is done, which saves the plugins the
    /// script changed into the private data folder.
    #[serde(default)]
    close_after: bool,
    /// The plugins whose saved bytes are compared (a subset of the loaded
    /// ones); with them `close_after` is implied.
    #[serde(default)]
    save: Vec<String>,
    /// The recorded dialogs the script shows, in order: the oracle GUI is
    /// answered with them (`gui::Answer`) and the port's headless VCL model
    /// consumes the same JSON (`script.run`'s `answers`). One case file
    /// drives both sides.
    #[serde(default)]
    answers: Vec<super::gui::Answer>,
    /// A script that is slow over the loaded plugins.
    #[serde(default)]
    timeout_minutes: Option<f64>,
    /// A synthetic game master of the repository (`tests/fixtures/...`)
    /// loaded in place of the game's, under the game master's name, by both
    /// sides: a fixture case needs no game data. `load` then names the game
    /// master alone.
    #[serde(default)]
    master: Option<String>,
    /// Fixture plugins of the repository loaded after the fixture `master`
    /// (a case of a script that works on overrides, such as `Merge
    /// overrides into master.pas`), by their file names. Needs `master`.
    #[serde(default)]
    plugins: Vec<String>,
    /// The references come from the reference cache: each side first loads
    /// the plugins with a no-op script and the reference build (which
    /// writes its cache files, for a file above `wbCacheRecordsThreshold`
    /// records), then runs the case with those files in its cache folder,
    /// so the records' references are read from the cache. Needs
    /// `build_refs`.
    #[serde(default)]
    refcache: bool,
    /// Load the empty master the harness writes instead of the case's
    /// plugins: a probe that reads no record needs no game data.
    #[serde(default)]
    synthetic_master: bool,
    /// Extra files copied into the private data folder of both sides, as
    /// `<path in the repository>:<path under Data>` (separators either way,
    /// such as `tests/fixtures/parity/atlas/textures/t/one.dds:textures/t/one.dds`):
    /// a probe that reads resources (`wbContainerHandler` lookups) needs
    /// them where the game's files would be.
    #[serde(default)]
    data_files: Vec<String>,
    /// The load settings of the case, as `xeInit.pas` reads them from its
    /// command line (`PseudoESL`, `IgnoreESL`, `TrackAllEditorID`, ...): the
    /// GUI gets the switch (`-PseudoESL`), the port the flag of
    /// `crates/xedit-cli` for it (`--pseudo-light`). A name with no flag
    /// fails the case.
    #[serde(default)]
    switches: Vec<String>,
    /// A second case of the same script (the `MessageDlg` path of a script
    /// whose other case builds the references): appended to the case's
    /// stem, so the two runs keep separate folders and report lines.
    #[serde(default)]
    variant: Option<String>,
    /// The reason the case is accepted when it differs: a divergence
    /// `docs/PLAN.md` documents (an upstream bug that loses data, which the
    /// port does not reproduce). The outcome is then `known-difference`; a
    /// case that turns out equal stays `equal`.
    #[serde(default)]
    accept: Option<String>,
    /// What the case is for, when it is not obvious.
    #[serde(default)]
    #[allow(dead_code, reason = "documentation in the file")]
    note: Option<String>,
}

/// One case to run: the parse of a script, or a run of one.
struct Task {
    /// `parse`, `run` or `probe`.
    phase: &'static str,
    /// The file name of the script.
    script: String,
    /// The folder the script and its units are read from: the corpus
    /// folder, `crates/xtask/oracle/scripts/probes` for the probe phase, or
    /// a probe of the repository run as a case whose `script` is
    /// `probes/<name>.pas`.
    scripts_dir: PathBuf,
    class: Class,
    game: &'static Game,
    data: PathBuf,
    /// The plugins to load, masters first.
    load: Vec<String>,
    build_refs: bool,
    close_after: bool,
    save: Vec<String>,
    /// The recorded dialog answers, given to the oracle GUI and to the
    /// port's `script.run` alike.
    answers: Vec<super::gui::Answer>,
    timeout: Duration,
    /// Parse cases load the empty game master instead of the case's
    /// plugins.
    synthetic_master: bool,
    /// The fixture master of a probe case, loaded as the game master.
    master: Option<PathBuf>,
    /// The fixture plugins loaded after the fixture master.
    plugins: Vec<PathBuf>,
    /// The case's `data_files`: (source in the repository, path under Data).
    data_files: Vec<(PathBuf, String)>,
    /// The references are loaded from a cache a first run wrote.
    refcache: bool,
    /// The load settings of the case (`-PseudoESL` for the GUI, the port's
    /// flag for the same setting).
    switches: Vec<String>,
    /// The reason `parse.json` accepts a script the oracle refuses, a probe
    /// `probes.json` accepts, or a run case `runs.json` accepts.
    accepted: Option<String>,
    #[allow(dead_code, reason = "read where a case loads the game's plugins")]
    stem: String,
}

/// The files a run left, keyed by the relative path the two sides map to.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
struct CapturedFile {
    path: String,
    size: u64,
    hash: u64,
}

/// What the oracle's run of a case left, cached.
#[derive(Serialize, Deserialize, Default)]
struct OracleResult {
    /// `done` (the script ran to the end), `interactive` (a dialog ended
    /// the run), `compile-error` (a dialog and no `Applying script` line:
    /// the script did not compile) or `failed`.
    status: String,
    #[serde(default)]
    dialogs: Vec<String>,
    #[serde(default)]
    files: Vec<CapturedFile>,
    #[serde(default)]
    saved: Vec<CapturedFile>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Serialize)]
struct ScriptOutcome {
    phase: &'static str,
    script: String,
    class: &'static str,
    game: &'static str,
    /// The parse phase: `equal` (the oracle compiled it and the port parsed
    /// it), `equal-error` (both refused it), `different`, `oracle-failed`,
    /// `port-failed` or `port-pending`. The run phase: `equal`,
    /// `equal-log-loss` (the oracle's memo lost log lines; the port's log is
    /// the superset in order with the first, last and `Done:` lines equal,
    /// see [`memo_loss`]), `different`, `known-difference` (the oracle
    /// refused a script `parse.json` accepts, with the reason),
    /// `oracle-interactive` (a dialog; deferred, the port can not run it
    /// headless either), `form` (not run, see `class_of`), `oracle-failed`,
    /// `port-failed` or `port-pending`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// Run phase: the files compared.
    files: usize,
    /// Run phase: the message-log lines compared.
    log_lines: usize,
    oracle_seconds: Option<f64>,
    port_seconds: Option<f64>,
}

#[derive(Serialize)]
struct ScriptReport<'a> {
    tag: &'a str,
    equal: usize,
    total: usize,
    outcomes: &'a [ScriptOutcome],
}

/// Whether the port CLI has `script check` / `script run` yet (phase 6
/// steps 2 and 4 add them). Probed once per run, each on its own; a side
/// that is not there reports `port-pending`, never a faked result.
fn port_has_command(port: &Path, command: &str) -> bool {
    std::process::Command::new(port)
        .args(["script", command, "--help"])
        .stdin(std::process::Stdio::null())
        .output()
        .map(|output| {
            output.status.success() && !String::from_utf8_lossy(&output.stdout).contains("unrecognized subcommand")
        })
        .unwrap_or(false)
}

pub(super) fn run(root: &Path, tag: &str, args: &[&str]) -> Result<()> {
    let options = parse(args)?;
    let oracle_dir = PathBuf::from(super::required_var("XEDIT_ORACLE_DIR")?);
    ensure!(oracle_dir.is_dir(), "{} does not exist", oracle_dir.display());
    let cache = super::cache_dir()?.join(tag);
    let scratch = match std::env::var_os("XEDIT_PARITY_SCRATCH") {
        Some(dir) => PathBuf::from(dir).join(tag),
        None => cache.clone(),
    };
    let scripts = scripts_dir()?;
    ensure!(scripts.is_dir(), "{} does not exist", scripts.display());
    let port = if options.oracle_only {
        None
    } else {
        Some(super::build_port(root)?)
    };
    let has_check = port.as_deref().is_some_and(|port| port_has_command(port, "check"));
    let has_run = port.as_deref().is_some_and(|port| port_has_command(port, "run"));
    let budget = options
        .memory_budget
        .or_else(|| crate::memory::physical_memory().map(|total| total / 4 * 3))
        .unwrap_or(24 * GIB);
    let runner = Runner {
        oracle: PathBuf::new(),
        port: port.clone(),
        cache,
        scratch: scratch.clone(),
        oracle_dir,
        hashes: std::sync::Mutex::new(std::collections::HashMap::new()),
        budget: crate::memory::Budget::new(budget),
        max_memory: options.max_memory.unwrap_or(budget),
        oracle_timeout: options.oracle_timeout,
    };
    fs::create_dir_all(&scratch)?;

    let cases_dir = root.join("crates/xtask/oracle/scripts");
    let parse_text = fs::read_to_string(cases_dir.join("parse.json"))
        .with_context(|| format!("reading {}", cases_dir.join("parse.json").display()))?;
    let parse_case: ParseCase = serde_json::from_str(&parse_text)?;
    let runs_text = fs::read_to_string(cases_dir.join("runs.json"))
        .with_context(|| format!("reading {}", cases_dir.join("runs.json").display()))?;
    let runs: RunsFile = serde_json::from_str(&runs_text)?;
    let classes = corpus_classes(root);

    let find_game = |name: &str| -> Result<&'static Game> {
        super::GAMES
            .iter()
            .find(|game| game.name == name)
            .with_context(|| format!("unknown game {name}"))
    };
    let selected = |name: &str| -> bool {
        let lower = name.to_lowercase();
        if options.skip.iter().any(|text| lower.contains(text.as_str())) {
            return false;
        }
        options.scripts.is_empty() || options.scripts.iter().any(|text| lower.contains(text.as_str()))
    };
    let game_selected = |game: &Game| options.all_games || options.games.iter().any(|g| g.name == game.name);

    // The parse corpus: every `.pas` of the scripts folder.
    let mut names: Vec<String> = fs::read_dir(&scripts)
        .with_context(|| format!("reading {}", scripts.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pas"))
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort_by_key(|name| name.to_lowercase());

    let mut tasks: Vec<Task> = Vec::new();
    if options.parse && game_selected(find_game(&parse_case.game)?) {
        let game = find_game(&parse_case.game)?;
        // The parse runs load an empty master of their own, so the game's
        // data folder is only needed for the executable of Fallout 3 and
        // New Vegas; the parse case names a game that needs none.
        let data = game_data_or_none(game).unwrap_or_default();
        let mut chosen: Vec<&String> = names.iter().filter(|name| selected(name)).collect();
        if let Some(sample) = options.sample {
            chosen.truncate(sample);
        }
        for name in chosen {
            let text = fs::read_to_string(scripts.join(name)).unwrap_or_default();
            let class = class_from(&classes, name, &text);
            tasks.push(Task {
                phase: "parse",
                script: name.clone(),
                scripts_dir: scripts.clone(),
                class,
                game,
                data: data.clone(),
                load: vec![game_master(game.mode).to_owned()],
                build_refs: false,
                close_after: false,
                save: Vec::new(),
                answers: Vec::new(),
                timeout: Duration::from_secs(10 * 60),
                data_files: Vec::new(),
                synthetic_master: true,
                master: None,
                plugins: Vec::new(),
                refcache: false,
                switches: Vec::new(),
                accepted: parse_case.accept.get(name.as_str()).cloned(),
                stem: format!("parse-{name}"),
            });
        }
    }
    if options.run {
        // A `--script` text selects a case by its script name, or by
        // `<script>-<variant>` for one case of a script that has several.
        let case_name = |case: &RunCase| match &case.variant {
            Some(variant) => format!("{}-{variant}", case.script),
            None => case.script.clone(),
        };
        let mut chosen: Vec<&RunCase> = runs.cases.iter().filter(|case| selected(&case_name(case))).collect();
        if let Some(sample) = options.sample {
            chosen.truncate(sample);
        }
        for case in chosen {
            let name = case.game.as_deref().unwrap_or(&runs.defaults.game);
            let game = find_game(name)?;
            if !game_selected(game) {
                continue;
            }
            // A fixture case loads its own master and needs no game data;
            // the game's data folder only matters for the executable of
            // Fallout 3 and New Vegas.
            let master = case.master.as_ref().map(|master| root.join(master));
            ensure!(
                case.plugins.is_empty() || master.is_some(),
                "{}: fixture plugins need a fixture master",
                case.script
            );
            let plugins: Vec<PathBuf> = case.plugins.iter().map(|plugin| root.join(plugin)).collect();
            let data_files = case
                .data_files
                .iter()
                .map(|entry| {
                    let (source, path) = entry
                        .split_once(':')
                        .with_context(|| format!("{}: data_files needs <source>:<path under Data>", case.script))?;
                    let source = root.join(source);
                    ensure!(source.is_file(), "{}: {} is not a file", case.script, source.display());
                    Ok((source, path.replace('/', "\\")))
                })
                .collect::<Result<Vec<_>>>()?;
            let data = match (game_data_or_none(game), &master) {
                (Some(data), _) => data,
                (None, Some(_)) => PathBuf::new(),
                (None, None) => {
                    println!("skipped       {}: {} is not set", case.script, game.data_var);
                    continue;
                }
            };
            // A machine may have the variable set to a folder that no longer
            // holds the game (an emptied install): the case is skipped and
            // named, as a mode skips a game whose variable is unset.
            let load = match &case.load {
                Some(load) => load.clone(),
                None if master.is_some() => vec![game_master(game.mode).to_owned()],
                None if game.name == runs.defaults.game => runs.defaults.load.clone(),
                None => vec![game_master(game.mode).to_owned()],
            };
            if let Some(master) = &master {
                ensure!(
                    load.len() == 1 && load[0].eq_ignore_ascii_case(game_master(game.mode)),
                    "{}: a case with a fixture master loads the game master alone",
                    case.script
                );
                ensure!(master.is_file(), "{}: {} is not a file", case.script, master.display());
                for plugin in &plugins {
                    ensure!(plugin.is_file(), "{}: {} is not a file", case.script, plugin.display());
                }
            } else {
                let missing = load
                    .iter()
                    .find(|name| super::oracle_save::find_in(&data, name).is_err());
                if let Some(name) = missing {
                    println!("skipped       {}: {name} is not in {}", case.script, data.display());
                    continue;
                }
            }
            // A probe of the repository (`probes/<name>.pas`) is read from
            // the cases folder, the corpus scripts from the scripts folder.
            let (scripts_dir, script) = match case.script.strip_prefix("probes/") {
                Some(name) => (cases_dir.join("probes"), name.to_owned()),
                None => (scripts.clone(), case.script.clone()),
            };
            let text = fs::read_to_string(scripts_dir.join(&script)).unwrap_or_default();
            let class = class_from(&classes, &script, &text);
            let close_after = case.close_after || !case.save.is_empty();
            let build_refs = case.build_refs.unwrap_or(runs.defaults.build_refs);
            ensure!(
                build_refs || !case.refcache,
                "{}: a case with refcache needs build_refs",
                case.script
            );
            tasks.push(Task {
                phase: "run",
                stem: {
                    // The run folder of a case: two cases of the same script
                    // have to differ, or they overwrite each other's files
                    // (`--jobs` runs them at once).
                    let mut stem = format!("run-{script}");
                    if case.refcache {
                        stem.push_str("-refcache");
                    }
                    if !case.switches.is_empty() {
                        stem.push('-');
                        stem.push_str(&case.switches.join("-"));
                    }
                    if let Some(variant) = &case.variant {
                        stem.push('-');
                        stem.push_str(variant);
                    }
                    stem
                },
                script,
                scripts_dir,
                class,
                game,
                data,
                load,
                build_refs,
                refcache: case.refcache,
                switches: case.switches.clone(),
                close_after,
                save: case.save.clone(),
                answers: case.answers.clone(),
                timeout: case.timeout_minutes.map_or(Duration::from_secs(10 * 60), |minutes| {
                    Duration::from_secs_f64(minutes * 60.0)
                }),
                data_files,
                synthetic_master: case.synthetic_master,
                master,
                plugins,
                accepted: case.accept.clone(),
            });
        }
    }
    if options.probe && game_selected(find_game(&parse_case.game)?) {
        let game = find_game(&parse_case.game)?;
        let data = game_data_or_none(game).unwrap_or_default();
        let probes = root.join("crates/xtask/oracle/scripts/probes");
        let accept_text = fs::read_to_string(cases_dir.join("probes.json"))
            .with_context(|| format!("reading {}", cases_dir.join("probes.json").display()))?;
        let accept: ProbesFile = serde_json::from_str(&accept_text)?;
        let mut names: Vec<String> = fs::read_dir(&probes)
            .with_context(|| format!("reading {}", probes.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.to_lowercase().ends_with(".pas") && !name.starts_with("Unit"))
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        let mut chosen: Vec<&String> = names.iter().filter(|name| selected(name)).collect();
        if let Some(sample) = options.sample {
            chosen.truncate(sample);
        }
        for name in chosen {
            tasks.push(Task {
                phase: "probe",
                script: name.clone(),
                scripts_dir: probes.clone(),
                class: Class::Headless,
                game,
                data: data.clone(),
                load: vec![game_master(game.mode).to_owned()],
                build_refs: false,
                close_after: false,
                save: Vec::new(),
                answers: Vec::new(),
                timeout: Duration::from_secs(10 * 60),
                data_files: Vec::new(),
                synthetic_master: true,
                master: None,
                plugins: Vec::new(),
                refcache: false,
                switches: Vec::new(),
                accepted: accept.accept.get(name.as_str()).cloned(),
                stem: format!("probe-{name}"),
            });
        }
    }
    if options.list {
        for task in &tasks {
            println!(
                "{:5} {:10} {} ({})",
                task.phase,
                task.class.name(),
                task.script,
                task.game.name
            );
        }
        println!("{} cases", tasks.len());
        return Ok(());
    }
    ensure!(!tasks.is_empty(), "no case selected");

    // The port's parse side: one `script check` per script, from the
    // scripts folder (the units a script `uses` resolve there).
    let port_parses = if let Some(port) = &port
        && has_check
    {
        Some(port_check(port, &tasks)?)
    } else {
        None
    };

    let queue = std::sync::Mutex::new(tasks.into_iter().enumerate().collect::<Vec<_>>().into_iter());
    let done = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.max(1) {
            let queue = &queue;
            let done = &done;
            let runner = &runner;
            let options = &options;
            let port_parses = &port_parses;
            scope.spawn(move || {
                loop {
                    let next = queue.lock().unwrap().next();
                    let Some((index, task)) = next else { break };
                    let mut outcome =
                        check(runner, options, has_check, has_run, port_parses, &task).unwrap_or_else(|error| {
                            ScriptOutcome {
                                phase: task.phase,
                                script: task.script.clone(),
                                class: task.class.name(),
                                game: task.game.name,
                                status: "oracle-failed",
                                detail: Some(format!("  {error:#}")),
                                files: 0,
                                log_lines: 0,
                                oracle_seconds: None,
                                port_seconds: None,
                            }
                        });
                    // A probe `probes.json` accepts, or a run case with
                    // `accept`: whatever the outcome, the reason is the
                    // record.
                    if matches!(task.phase, "probe" | "run")
                        && let Some(reason) = &task.accepted
                        && !matches!(outcome.status, "equal" | "equal-error")
                    {
                        outcome.detail = Some(format!(
                            "  accepted ({}): {reason}{}",
                            outcome.status,
                            outcome
                                .detail
                                .as_deref()
                                .map(|d| format!(
                                    "
{d}"
                                ))
                                .unwrap_or_default()
                        ));
                        outcome.status = "known-difference";
                    }
                    let mut text = format!(
                        "{:14} {} {} {} ({} files, {} log lines)",
                        outcome.status, outcome.phase, outcome.script, outcome.game, outcome.files, outcome.log_lines
                    );
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
    let mut outcomes = done.into_inner().unwrap();
    outcomes.sort_by_key(|(index, _)| *index);
    let outcomes: Vec<ScriptOutcome> = outcomes.into_iter().map(|(_, outcome)| outcome).collect();

    let equal = outcomes
        .iter()
        .filter(|o| {
            matches!(
                o.status,
                "equal" | "equal-log-loss" | "equal-error" | "known-difference"
            )
        })
        .count();
    let report = ScriptReport {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("script.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    let mut by_status: Vec<(&&str, usize)> = Vec::new();
    for outcome in &outcomes {
        match by_status.iter_mut().find(|(status, _)| **status == outcome.status) {
            Some((_, count)) => *count += 1,
            None => by_status.push((&outcome.status, 1)),
        }
    }
    for (status, count) in &by_status {
        println!("{count} {status}");
    }
    println!(
        "{equal} of {} cases equal. Report: {}",
        outcomes.len(),
        report_file.display()
    );
    let failed = outcomes
        .iter()
        .filter(|o| matches!(o.status, "different" | "oracle-failed" | "port-failed"))
        .count();
    ensure!(failed == 0, "parity does not hold");
    Ok(())
}

fn game_data_or_none(game: &Game) -> Option<PathBuf> {
    std::env::var_os(game.data_var).map(PathBuf::from)
}

/// The class of a script: `corpus.json` when it has one, else the greps.
fn class_from(classes: &std::collections::HashMap<String, String>, name: &str, text: &str) -> Class {
    match classes.get(&name.to_lowercase()).map(String::as_str) {
        Some("headless") => Class::Headless,
        Some("form") => Class::Form,
        Some("interactive") => Class::Interactive,
        Some("api") => Class::Api,
        _ => class_of(name, text),
    }
}

/// `parity script` for one case.
#[allow(clippy::too_many_arguments)]
fn check(
    runner: &Runner,
    options: &Options,
    has_check: bool,
    has_run: bool,
    port_parses: &Option<std::collections::HashMap<String, String>>,
    task: &Task,
) -> Result<ScriptOutcome> {
    let mut outcome = ScriptOutcome {
        phase: task.phase,
        script: task.script.clone(),
        class: task.class.name(),
        game: task.game.name,
        status: "oracle-only",
        detail: None,
        files: 0,
        log_lines: 0,
        oracle_seconds: None,
        port_seconds: None,
    };
    // A script that needs a form or asks for input does not run headless:
    // it is deferred unless it was named with `--script`, or unless the case
    // carries recorded `answers` (issue #26), which both sides run with.
    if task.phase == "run" && task.class != Class::Headless && options.scripts.is_empty() && task.answers.is_empty() {
        outcome.status = match task.class {
            Class::Form => "form",
            _ => "oracle-interactive",
        };
        outcome.detail = Some(format!(
            "  the corpus classes \"{}\" as {}; name it with --script to run it anyway",
            task.script,
            task.class.name()
        ));
        return Ok(outcome);
    }
    let (oracle, oracle_dir) = oracle_run(runner, options, task, &mut outcome)?;
    if task.phase == "parse" {
        return parse_outcome(has_check, port_parses, task, &oracle, outcome);
    }
    if task.phase == "probe" && oracle.status == "compile-error" {
        // The application's exception dialog of a compile error carries
        // `Error in unit 'X' on line N : <message>.`; the port's `script
        // check` reports the same failure as `file:line: message`, and the
        // two compare on their `line : message` part.
        return match &runner.port {
            Some(port) if has_check => Ok(probe_compile_error(port, task, &oracle, outcome)),
            _ => {
                outcome.status = "port-pending";
                Ok(outcome)
            }
        };
    }
    if task.phase == "probe" && !matches!(oracle.status.as_str(), "done" | "aborted") {
        // A probe that ends on a dialog of the script's own, or that the
        // oracle never finishes, has no log shape the port could match: its
        // class is recorded with the dialog.
        outcome.status = "oracle-dialog";
        outcome.detail = Some(format!(
            "  the oracle ended {}: {}",
            oracle.status,
            oracle.note.clone().unwrap_or_else(|| oracle.dialogs.join("; "))
        ));
        return Ok(outcome);
    }
    if !matches!(oracle.status.as_str(), "done" | "aborted") {
        // A dialog a corpus script shows (an `InputQuery` even in a script
        // the greps called headless, a message box) ends the run: the class
        // is reported and nothing is compared, which is not a difference.
        if oracle.status == "interactive" {
            outcome.status = "oracle-interactive";
            outcome.detail = Some(format!("  the run stopped on a dialog: {}", oracle.dialogs.join("; ")));
        } else {
            outcome.status = "oracle-failed";
            outcome.detail = Some(format!(
                "  the oracle run ended {}: {}",
                oracle.status,
                oracle.note.clone().unwrap_or_else(|| oracle.dialogs.join("; "))
            ));
        }
        return Ok(outcome);
    }
    outcome.files = oracle.files.len() + oracle.saved.len();
    if let Ok(log) = fs::read_to_string(oracle_dir.join("log.txt")) {
        // The lines the port has to reproduce, for the report even while
        // the port side is still pending.
        outcome.log_lines = script_lines(&log).len();
    }
    if oracle.status == "aborted" {
        outcome.detail = Some("  the script aborted at runtime; the abort is compared".to_owned());
    }
    if !has_run {
        outcome.status = "port-pending";
        outcome.detail = Some("  the port CLI has no `script run` command yet (phase 6 step 4)".to_owned());
        return Ok(outcome);
    }
    let Some(port) = &runner.port else {
        outcome.status = "oracle-only";
        return Ok(outcome);
    };
    port_run(runner, port, options, task, &oracle, &oracle_dir, outcome)
}

/// The oracle's run of a case: from the cache, or the GUI. Returns the
/// result and the folder its files, log and manifest are cached under.
fn oracle_run(
    runner: &Runner,
    options: &Options,
    task: &Task,
    outcome: &mut ScriptOutcome,
) -> Result<(OracleResult, PathBuf)> {
    let plugins = task_plugins(runner, task)?;
    let (script_text, units) = script_and_units(&task.scripts_dir, &task.script)?;
    let mut recipe = format!(
        "{RECIPE}\n{}\n{}\n{}\n{}",
        task.phase,
        task.game.mode,
        if task.build_refs { "refs" } else { "norefs" },
        // The run command line, beyond the shared switches of `GuiRun`.
        EXTRA_ARGS.join(",")
    );
    if task.close_after {
        recipe.push_str("\nclose");
        recipe.push_str(&task.save.join(","));
        // Bumped when what a run captures for the saved plugins changes.
        recipe.push_str("\nsave v2");
    }
    if !task.answers.is_empty() {
        recipe.push_str("\nanswers ");
        recipe.push_str(&serde_json::to_string(&task.answers)?);
    }
    if !task.switches.is_empty() {
        recipe.push_str(
            "
switches ",
        );
        recipe.push_str(&task.switches.join(","));
    }
    if task.refcache {
        recipe.push_str("\nrefcache");
    }
    for (source, path) in &task.data_files {
        recipe.push_str(path);
        recipe.push_str(&format!("{:016x}", super::nif::fnv(&fs::read(source)?)));
    }
    // The texts go in as bytes (a probe may carry a byte order mark or a
    // UTF-16 encoding, which the oracle's `TStringList.LoadFromFile` reads
    // and the port has to read the same way), hashed so an encoding change
    // is a new key.
    recipe.push_str(&format!("{:016x}", super::nif::fnv(&script_text)));
    for (name, text) in &units {
        recipe.push_str(name);
        recipe.push_str(&format!("{:016x}", super::nif::fnv(text)));
    }
    let key = oracle_key(runner, &plugins, &recipe, gui::exe_name(task.game.mode))?;
    let dir = runner.cache.join(format!("{}-oracle-script", task.game.mode));
    let stem = format!("{}.{key:016x}", task.stem);
    let dir = dir.join(&stem);
    let result_file = dir.join("result.json");
    if result_file.exists() && !options.refresh_oracle {
        outcome.oracle_seconds = fs::read_to_string(dir.join("seconds"))
            .ok()
            .and_then(|text| text.trim().parse().ok());
        let result = read_result(&dir)?;
        return Ok((result, dir));
    }
    fs::create_dir_all(&dir)?;
    let work = runner
        .scratch
        .join("oracle-work")
        .join(format!("{}.{}", stem, std::process::id()));
    let exe = runner.oracle_dir.join(gui::exe_name(task.game.mode));
    ensure!(exe.exists(), "{} does not exist", exe.display());
    let peak_file = dir.join("peak");
    let out = work.join("out");
    let mut extras = GuiExtras {
        close_after: task.close_after,
        answers: task.answers.clone(),
        ..GuiExtras::default()
    };
    extras.files.push((PathBuf::from(&task.script), script_text.clone()));
    for (name, text) in &units {
        extras.files.push((PathBuf::from(format!("{name}.pas")), text.clone()));
    }
    for (source, path) in &task.data_files {
        let bytes = fs::read(source).with_context(|| format!("reading {}", source.display()))?;
        extras.files.push((PathBuf::from(format!("Data\\{path}")), bytes));
    }
    let run = GuiRun {
        exe: &exe,
        mode: task.game.mode,
        star_plugins_txt: !gui::simple_plugins_txt(task.game.mode),
        plugins: plugins.clone(),
        script: String::new(),
        build_refs: task.build_refs,
        work: work.clone(),
        timeout: runner.oracle_timeout.unwrap_or(task.timeout),
        // A script that works uses the CPU; a `Mode: Silent` script that
        // raised is idle without a line in the log (`wbProgressLock`
        // suppresses them), and the dialog rule below would not see it
        // either, so the idle watch has to end the run.
        hang_timeout: Duration::from_secs(120),
        budget: &runner.budget,
        expected_peak: expected_peak(&peak_file, &plugins),
        max_memory: runner.max_memory,
        extra_args: EXTRA_ARGS
            .iter()
            .map(|arg| (*arg).to_owned())
            .chain(task.switches.iter().map(|name| format!("-{name}")))
            .collect(),
        discovery: false,
        tool_name: "Edit".to_owned(),
    };
    let started = std::time::Instant::now();
    if task.refcache {
        // The cache files a first load with the reference build writes
        // (`-C:` is the run folder's `cache`), put into the cache folder of
        // the case's run.
        let prep_work = runner
            .scratch
            .join("oracle-work")
            .join(format!("{}.{}.prep", stem, std::process::id()));
        let prep = GuiRun {
            work: prep_work.clone(),
            extra_args: EXTRA_ARGS.iter().map(|arg| (*arg).to_owned()).collect(),
            discovery: false,
            tool_name: "Edit".to_owned(),
            build_refs: true,
            plugins: plugins.clone(),
            script: String::new(),
            ..run
        };
        let prep_extras = GuiExtras {
            files: vec![(PathBuf::from(PREP_SCRIPT), PREP_SCRIPT_TEXT.as_bytes().to_vec())],
            ..GuiExtras::default()
        };
        let prepared = prep.run_script(PREP_SCRIPT, &prep_extras, Some(&prep_work.join("out")));
        let cache_files = prepared.and_then(|result| {
            ensure!(
                result.ended,
                "the reference build run did not reach its end: {}",
                gui::tail(&result.log, 5)
            );
            cache_files_of(&prep_work.join("cache"))
        });
        gui::remove_work(&prep_work);
        let cache_files = cache_files.context("the reference cache of the oracle")?;
        ensure!(!cache_files.is_empty(), "the oracle wrote no reference cache file");
        for (name, bytes) in cache_files {
            extras.files.push((PathBuf::from("cache").join(name), bytes));
        }
    }
    let result = run.run_script(&task.script, &extras, Some(&out));
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            gui::remove_work(&work);
            // A `Mode:` keyword suppresses the mode's progress lines (the
            // `Start:`/`Aborted:` pair among them, `wbProgressLock`), so a
            // script that raises shows as an idle GUI here, not as a line.
            let hint = if String::from_utf8_lossy(&script_text).contains("Mode:") {
                " (the script has a `Mode:` keyword, so its runtime lines are suppressed: an idle GUI can be a runtime failure)"
            } else {
                ""
            };
            if task.phase == "probe" {
                // A probe the oracle never finishes (a native stack
                // exhaustion, a hang on a NUL character) is its own
                // witnessed outcome: cached as `failed` with the reason, so
                // the next run does not wait for it again (`--refresh-oracle`
                // runs it anew).
                outcome.oracle_seconds = Some(started.elapsed().as_secs_f64());
                let captured = OracleResult {
                    status: "failed".to_owned(),
                    note: Some(format!("the oracle run failed{hint}: {error:#}")),
                    ..OracleResult::default()
                };
                fs::write(dir.join("log.txt"), "")?;
                fs::write(&result_file, serde_json::to_string_pretty(&captured)?)?;
                return Ok((captured, dir));
            }
            return Err(error).with_context(|| format!("the oracle run failed{hint}"));
        }
    };
    outcome.oracle_seconds = Some(started.elapsed().as_secs_f64());
    if let Some(peak) = result.peak {
        fs::write(&peak_file, peak.to_string())?;
    }
    fs::write(dir.join("seconds"), format!("{:.1}", started.elapsed().as_secs_f64()))?;
    let log = result.saved_log.clone().unwrap_or_else(|| result.log.clone());
    fs::write(dir.join("log.txt"), &log)?;
    let compiled = log.contains("Applying script");
    let aborted = log.contains(gui::SCRIPT_ABORT_LINE);
    // The application's own exception dialog (the GUI's title carries the
    // version): what a compile error shows, `Error in unit 'X' on line N :
    // ...` (`JvResources.pas`), or an access violation when the compiler
    // itself faults (`xEditAPI.pas`). A message box or form of the script
    // itself has the default caption of its kind (`Information`, `Confirm`)
    // or a window class of its own, and proves that the compile finished:
    // `Bookmark.pas` shows its bookmark list from its main block, which
    // `TJvInterpreterProgram.Compile` runs before `ApplyScript` posts the
    // `Applying script` line.
    let application_dialog = result
        .dialogs
        .iter()
        .find(|dialog| dialog.starts_with("[#32770]") && dialog_title(dialog).contains(APP_DIALOG_VERSION))
        .cloned();
    let mut captured = OracleResult {
        status: if result.ended {
            "done".to_owned()
        } else if aborted {
            // The script compiled and ran, and raised outside its own
            // handler: `ApplyScript` aborts and the mode never reaches its
            // closing line.
            "aborted".to_owned()
        } else if application_dialog.is_some() {
            "compile-error".to_owned()
        } else if !result.dialogs.is_empty() {
            "interactive".to_owned()
        } else if !compiled {
            "failed".to_owned()
        } else {
            // Compiled, idle, no line: a `Mode: Silent` script between its
            // statements. The 120 s idle watch ends the run before this.
            "interactive".to_owned()
        },
        dialogs: result.dialogs.clone(),
        ..OracleResult::default()
    };
    if let Some(dialog) = application_dialog {
        captured.note = Some(dialog);
    }
    if task.phase == "run" {
        let skip = plugin_names(&plugins, task.game.mode);
        let (files, saved) = capture_work(&work, &result.data, &skip, &task.save)?;
        keep_files(&work, &result.data, &dir, &files, false)?;
        keep_files(&work, &result.data, &dir, &saved, true)?;
        captured.files = files;
        captured.saved = saved;
    }
    fs::write(&result_file, serde_json::to_string_pretty(&captured)?)?;
    gui::remove_work(&work);
    Ok((captured, dir))
}

/// The units a script `uses`, by name, with their bytes.
type UnitSources = Vec<(String, Vec<u8>)>;

/// The script text and the texts of the units it `uses` that are not
/// compiled into the host, by unit name. The texts are the files' bytes:
/// their encoding (a byte order mark, UTF-16) is part of what both sides
/// read.
fn script_and_units(scripts: &Path, name: &str) -> Result<(Vec<u8>, UnitSources)> {
    let text = fs::read(scripts.join(name)).with_context(|| format!("reading {name}"))?;
    let mut units = Vec::new();
    for unit in used_units(&String::from_utf8_lossy(&text)) {
        let stripped = strip_namespace(&unit);
        if BUILT_IN_UNITS
            .iter()
            .any(|built_in| *built_in == stripped.to_lowercase())
        {
            continue;
        }
        let path = scripts.join(format!("{stripped}.pas"));
        if path.exists() {
            units.push((stripped.to_owned(), fs::read(&path)?));
        }
    }
    units.sort();
    Ok((text, units))
}

/// `text` without its comments (`//`, `{ }` and `(* *)`), so that a `uses`
/// in one does not name a unit. String literals are kept.
fn without_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'/') {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
        } else if bytes[at] == b'{' {
            while at < bytes.len() && bytes[at] != b'}' {
                at += 1;
            }
            at += 1;
        } else if bytes[at] == b'(' && bytes.get(at + 1) == Some(&b'*') {
            at += 2;
            while at + 1 < bytes.len() && !(bytes[at] == b'*' && bytes[at + 1] == b')') {
                at += 1;
            }
            at += 2;
        } else if bytes[at] == b'\'' {
            out.push(bytes[at]);
            at += 1;
            while at < bytes.len() {
                out.push(bytes[at]);
                let done = bytes[at] == b'\'';
                at += 1;
                if done {
                    break;
                }
            }
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The unit names of the `uses` clauses of a script.
fn used_units(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let clean = without_comments(text);
    let text = clean.as_str();
    let lower = text.to_lowercase();
    let mut at = 0;
    while let Some(found) = lower[at..].find("uses") {
        let start = at + found;
        at = start + 4;
        // A word, not `causes` or a name that ends in `uses`.
        if start > 0 && (lower.as_bytes()[start - 1].is_ascii_alphanumeric() || lower.as_bytes()[start - 1] == b'_') {
            continue;
        }
        let Some(end) = lower[at..].find(';') else { break };
        let clause = &text[at..at + end];
        // A uses clause holds unit names separated by commas (and line
        // breaks); a `uses` in a comment or a string does not matter here,
        // a missing file is skipped.
        if clause.contains('{') || clause.contains('(') || clause.contains('\'') {
            continue;
        }
        for part in clause.split(',') {
            let name = part.trim();
            if !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                names.push(name.to_owned());
            }
        }
        at += end;
    }
    names.sort();
    names.dedup();
    names
}

/// A unit name without its namespace, as `xejviScriptHost.pas` strips it
/// (`system.`, `vcl.`, `winapi.`, `data.`, `web.`).
fn strip_namespace(name: &str) -> &str {
    let lower = name.to_lowercase();
    for prefix in ["system.", "vcl.", "winapi.", "data.", "web."] {
        if lower.starts_with(prefix) {
            return &name[prefix.len()..];
        }
    }
    name
}

/// The entry names of the plugins of a load: what a run's data folder holds
/// before the script writes anything into it.
fn plugin_names(plugins: &[PathBuf], mode: &str) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = plugins
        .iter()
        .filter_map(|plugin| plugin.file_name())
        .map(|name| name.to_string_lossy().to_lowercase())
        .collect();
    if let Some(exe) = gui::hardcoded_exe(mode) {
        names.insert(exe.to_lowercase());
    }
    names
}

/// The files a run left, split into what the script wrote and the plugins
/// the close saved (`save`).
fn capture_work(
    work: &Path,
    data: &Path,
    skip: &BTreeSet<String>,
    save: &[String],
) -> Result<(Vec<CapturedFile>, Vec<CapturedFile>)> {
    let mut files = Vec::new();
    let mut saved = Vec::new();
    let roots = [
        (work.join("out"), "out"),
        (work.to_path_buf(), "script"),
        (work.join("bin"), "program"),
        (data.to_path_buf(), "data"),
    ];
    for (root, prefix) in &roots {
        if !root.is_dir() {
            continue;
        }
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let lower = name.to_lowercase();
                if path.is_dir() {
                    // The GUI's own folders, not output of the script.
                    if *prefix == "script" && RUN_FOLDERS.contains(&lower.as_str()) {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }
                let relative = path.strip_prefix(root).unwrap_or(&path);
                let mapped = format!("{prefix}/{}", relative.to_string_lossy().replace('\\', "/"));
                // The machinery of the run, not output of the script.
                let machinery = match *prefix {
                    "out" => false,
                    "script" => {
                        lower == "oracle.pas"
                            || lower == "plugins.txt"
                            || lower == "game.ini"
                            || lower.starts_with("plugins.") && lower.ends_with("viewsettings")
                            || lower.ends_with(".pas")
                            || skip.contains(&lower)
                    }
                    "program" => is_gui_artifact(&lower) || skip.contains(&lower) || is_windows_executable(&path),
                    _ => skip.contains(&lower),
                };
                if machinery {
                    continue;
                }
                // A plugin the close saved is captured on its own, under
                // the name of the case: the GUI writes it as
                // `<name>.save.<time>` and queues the rename over the
                // plugin for its shutdown, which the harness never reaches.
                if *prefix == "data" && save.iter().any(|plugin| saved_matches(plugin, &lower)) {
                    continue;
                }
                files.push(capture(&path, &mapped)?);
            }
        }
    }
    saved.extend(capture_saved(data, save)?);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    saved.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((files, saved))
}

/// The title of a recorded dialog (`[class] "title": texts`); a file
/// dialog (`Open`, `Save As`) carries paths that must not be read as the
/// application's.
fn dialog_title(dialog: &str) -> &str {
    dialog
        .split_once("] \"")
        .and_then(|(_, rest)| rest.split('"').next())
        .unwrap_or_default()
}

/// Whether a data folder entry is one of the plugins of a case's `save`
/// list, or the `<name>.save.<time>` file the GUI writes its save to.
fn saved_matches(plugin: &str, lower_name: &str) -> bool {
    let plugin = plugin.to_lowercase();
    lower_name == plugin
        || lower_name
            .strip_prefix(&plugin)
            .is_some_and(|rest| rest.starts_with(".save."))
}

/// The file a case's `save` plugin is in after the close: the plugin
/// itself when the GUI renamed its save over it (the rename is queued for
/// the shutdown, which the harness never reaches), else the newest
/// `<name>.save.<time>` the GUI wrote.
fn saved_source(data: &Path, plugin: &str) -> Result<Option<PathBuf>> {
    let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in fs::read_dir(data)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        if saved_matches(plugin, &name) {
            let modified = fs::metadata(&path)?
                .modified()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            candidates.push((modified, path));
        }
    }
    candidates.sort_by_key(|(modified, _)| *modified);
    Ok(candidates.pop().map(|(_, path)| path))
}

/// The plugins of a case's `save` list as the close left them, under the
/// name of the case.
fn capture_saved(data: &Path, save: &[String]) -> Result<Vec<CapturedFile>> {
    let mut saved = Vec::new();
    for plugin in save {
        if let Some(path) = saved_source(data, plugin)? {
            saved.push(capture(&path, &format!("saved/{plugin}"))?);
        }
    }
    Ok(saved)
}

/// The folders the GUI's run folder holds beside what a script writes: the
/// private `Data`, `bin` (the program), `out` (the output path, walked of
/// its own), `mygames` and the temporary, reference cache and backup
/// folders.
const RUN_FOLDERS: &[&str] = &["bin", "data", "out", "temp", "cache", "backup", "mygames"];

/// The files the GUI itself writes next to its program: its message log
/// and the exception log every caught exception appends to (machine
/// specific addresses, never part of a script's output).
fn is_gui_artifact(lower_name: &str) -> bool {
    lower_name.ends_with("_log.txt") || lower_name.ends_with("exception.log")
}

fn is_windows_executable(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("dll"))
}

/// One captured file: its size and content hash.
fn capture(path: &Path, mapped: &str) -> Result<CapturedFile> {
    Ok(CapturedFile {
        path: mapped.to_owned(),
        size: fs::metadata(path)?.len(),
        hash: content_hash(path)?,
    })
}

/// The file of a run folder a mapped path names: `out/` below the run
/// folder's `out`, `script/` in the run folder itself, `program/` in its
/// `bin`, `data/` in its `Data` (the GUI's private folders).
fn mapped_source(work: &Path, mapped: &str) -> PathBuf {
    let (folder, rest) = mapped.split_once('/').unwrap_or(("script", mapped));
    let root = match folder {
        "out" => work.join("out"),
        "program" => work.join("bin"),
        "data" => work.join("Data"),
        _ => work.to_path_buf(),
    };
    root.join(rest.replace('/', "\\"))
}

/// Copies the bytes of the captured files into the cache, zstd-compressed:
/// `<dir>/files/<path>.zst`, or `files/saved/<plugin>.zst` for the saved
/// plugins (`data` is the private data folder they live in).
fn keep_files(work: &Path, data: &Path, dir: &Path, files: &[CapturedFile], saved: bool) -> Result<()> {
    for file in files {
        let source = if saved {
            let name = file.path.rsplit('/').next().unwrap_or(&file.path).to_owned();
            match saved_source(data, &name)? {
                Some(path) => path,
                None => continue,
            }
        } else {
            mapped_source(work, &file.path)
        };
        if !source.exists() {
            continue;
        }
        let cached = if saved {
            let name = file.path.rsplit('/').next().unwrap_or(&file.path);
            dir.join("files").join("saved").join(format!("{name}.zst"))
        } else {
            dir.join("files").join(format!("{}.zst", file.path))
        };
        if let Some(parent) = cached.parent() {
            fs::create_dir_all(parent)?;
        }
        super::oracle_save::keep_compressed(&source, &cached)?;
    }
    Ok(())
}

fn read_result(dir: &Path) -> Result<OracleResult> {
    let text = fs::read_to_string(dir.join("result.json"))?;
    Ok(serde_json::from_str(&text)?)
}

/// The parse outcome: the oracle compiled the script and the port parsed
/// it, or both refused it.
fn parse_outcome(
    has_check: bool,
    port_parses: &Option<std::collections::HashMap<String, String>>,
    task: &Task,
    oracle: &OracleResult,
    mut outcome: ScriptOutcome,
) -> Result<ScriptOutcome> {
    // `aborted` compiled too: the script ran and raised at runtime.
    let compiled = matches!(oracle.status.as_str(), "done" | "interactive" | "aborted");
    if !has_check {
        outcome.status = "port-pending";
        outcome.detail = Some(format!(
            "  the oracle {} it; the port CLI has no `script check` command yet (phase 6 step 2)",
            if compiled { "compiled" } else { "refused" }
        ));
        return Ok(outcome);
    }
    let result = port_parses
        .as_ref()
        .and_then(|parse| parse.get(&task.script.to_lowercase()))
        .cloned();
    let Some(result) = result else {
        outcome.status = "port-failed";
        outcome.detail = Some("  the port reported no result for the script".to_owned());
        return Ok(outcome);
    };
    match (compiled, result.as_str()) {
        (true, "ok") => outcome.status = "equal",
        (false, "error") => {
            outcome.status = "equal-error";
            outcome.detail = Some(format!(
                "  both refused the script; the oracle: {}",
                oracle.dialogs.join("; ")
            ));
        }
        (true, _) => {
            outcome.status = "different";
            outcome.detail = Some(format!("  the oracle compiled the script, the port: {result}"));
        }
        (false, _) => match &task.accepted {
            Some(reason) => {
                outcome.status = "known-difference";
                outcome.detail = Some(format!(
                    "  the oracle refused the script ({}) and `parse.json` accepts it: {reason}",
                    oracle.note.clone().unwrap_or_else(|| oracle.dialogs.join("; "))
                ));
            }
            None => {
                outcome.status = "different";
                outcome.detail = Some(format!(
                    "  the oracle refused the script ({}), the port: {}",
                    oracle.note.clone().unwrap_or_else(|| oracle.dialogs.join("; ")),
                    result
                ));
            }
        },
    }
    Ok(outcome)
}

/// A probe the oracle refused at compile time: `xedit script check` on the
/// port, whose first failure's `line: message` must be the dialog's `on
/// line N : message.` (the dialog's text ends with the period the
/// application's exception handler appends).
fn probe_compile_error(port: &Path, task: &Task, oracle: &OracleResult, mut outcome: ScriptOutcome) -> ScriptOutcome {
    let note = oracle.note.clone().unwrap_or_default();
    let Some(at) = note.find(" on line ") else {
        outcome.status = "oracle-failed";
        outcome.detail = Some(format!("  the oracle's dialog has no compile error: {note}"));
        return outcome;
    };
    let expected = note[at + " on line ".len()..].trim_end_matches('.').trim().to_owned();
    let output = std::process::Command::new(port)
        .arg("script")
        .arg("check")
        .arg(task.scripts_dir.join(&task.script))
        .env("XEDIT_SCRIPTS", &task.scripts_dir)
        .stdin(std::process::Stdio::null())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            outcome.status = "port-failed";
            outcome.detail = Some(format!("  running {} script check: {error}", port.display()));
            return outcome;
        }
    };
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        outcome.status = "different";
        outcome.detail = Some(format!(
            "  the oracle refused the script (line {expected}), the port parsed it"
        ));
        return outcome;
    }
    // `<file>:<line>: <message>`: the file path may hold colons, so the
    // line is the last `:<digits>:` group of the first failure line.
    let first = text.lines().find(|line| !line.trim().is_empty()).unwrap_or_default();
    let port_text = match first.rsplit_once(": ") {
        Some((head, message)) => match head.rsplit_once(':') {
            Some((_, line)) => format!("{} : {message}", line.trim()),
            None => first.to_owned(),
        },
        None => first.to_owned(),
    };
    if port_text == expected {
        outcome.status = "equal-error";
        outcome.detail = Some(format!("  both refused the script: line {expected}"));
    } else {
        outcome.status = "different";
        outcome.detail = Some(format!(
            "  the oracle refused the script with line {expected}, the port with {port_text}"
        ));
    }
    outcome
}

/// `result.json` of the port is not defined yet: the parse side runs
/// `xedit script check <file>` for every parse script once and keeps
/// `ok` or `error` (with the message) per script.
fn port_check(port: &Path, tasks: &[Task]) -> Result<std::collections::HashMap<String, String>> {
    let mut results = std::collections::HashMap::new();
    for task in tasks.iter().filter(|task| task.phase == "parse") {
        let output = std::process::Command::new(port)
            .arg("script")
            .arg("check")
            .arg(task.scripts_dir.join(&task.script))
            .env("XEDIT_SCRIPTS", &task.scripts_dir)
            .stdin(std::process::Stdio::null())
            .output()
            .with_context(|| format!("running {} script check", port.display()))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value = if output.status.success() {
            "ok".to_owned()
        } else {
            let first = text
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("<no message>")
                .trim()
                .to_owned();
            format!("error ({first})")
        };
        results.insert(task.script.to_lowercase(), value);
    }
    Ok(results)
}

/// The port's run of a case, compared with the oracle's.
#[allow(clippy::too_many_arguments)]
fn port_run(
    runner: &Runner,
    port: &Path,
    options: &Options,
    task: &Task,
    oracle: &OracleResult,
    oracle_dir: &Path,
    mut outcome: ScriptOutcome,
) -> Result<ScriptOutcome> {
    let dir = runner.scratch.join(format!("{}-port", task.game.mode)).join(&task.stem);
    // A folder kept from an earlier run of a different recipe must not leak
    // files into this run's capture.
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    let out = dir.join("out");
    let data = dir.join("Data");
    let bin = dir.join("bin");
    fs::create_dir_all(&out)?;
    fs::create_dir_all(&data)?;
    fs::create_dir_all(&bin)?;
    let (text, units) = script_and_units(&task.scripts_dir, &task.script)?;
    // The script and the units it `uses` live in the run folder itself, as
    // the oracle's do (`-script:<work>\<name>.pas`), so `ScriptsPath()` is
    // the run folder on both sides.
    fs::write(dir.join(&task.script), &text)?;
    for (name, text) in &units {
        fs::write(dir.join(format!("{name}.pas")), text)?;
    }
    // The plugins load from a private data folder, as the oracle's copies
    // do: the port derives its data path from the loaded plugin's folder, so
    // string tables, `DataPath` messages, the DDS scan and the save paths all
    // see a folder without the game's `Strings` files, exactly as the
    // oracle's private Data does.
    let loaded = task_plugins(runner, task)?;
    let mut plugins = Vec::new();
    for source in loaded {
        let name = source.file_name().context("plugin without a name")?;
        fs::copy(&source, data.join(name))?;
        // The oracle's `-D:` value ends `Data\\` and its log paths repeat
        // the separator (`<work>\Data\\Skyrim.esm`); the port takes its data
        // path from the loaded plugin's folder, so the mode names the copies
        // with the same doubled separator and the printed paths compare.
        plugins.push(PathBuf::from(format!(
            "{}\\\\{}",
            data.display(),
            name.to_string_lossy()
        )));
    }
    // The case's own files below `Data` (fixture textures a probe reads
    // through the container handler), as the oracle's private folder has
    // them.
    for (source, path) in &task.data_files {
        let target = data.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, &target).with_context(|| format!("copying {} to {}", source.display(), target.display()))?;
    }
    // The oracle's `-M:` and `-I:`: a My Games folder of the run with an
    // empty game ini, so no ini of the user's names archives or a language.
    let my_games = dir.join("mygames");
    fs::create_dir_all(&my_games)?;
    fs::create_dir_all(dir.join("temp"))?;
    fs::write(my_games.join("game.ini"), "")?;
    // The same recorded dialogs the oracle GUI is answered with, as the
    // `script.run` request's `answers`; the port's VCL model consumes them
    // in order.
    let answers_file = dir.join("answers.json");
    if !task.answers.is_empty() {
        // `{{WORK}}` stands for the run folder, as in the oracle's answers:
        // an answer naming a path the script writes under it gets this
        // side's folder, so the two sides' files compare.
        let work_text = dir.display().to_string();
        let answers: Vec<_> = task
            .answers
            .iter()
            .cloned()
            .map(|mut answer| {
                if let Some(text) = &mut answer.text {
                    *text = text.replace("{{WORK}}", &work_text);
                }
                answer
            })
            .collect();
        let answers = serde_json::to_string(&answers)?;
        fs::write(&answers_file, answers)?;
    }
    let cache = dir.join("cache");
    let port_command = |script: &str| {
        let mut command = std::process::Command::new(port);
        command.args(["--json", "--edit", "--game", task.game.mode]);
        if task.refcache {
            // The cache folder a first run fills (the oracle's `-C:`).
            command.arg("--cache-path").arg(format!("{}\\", cache.display()));
        } else {
            command.arg("--dont-cache");
        }
        for name in &task.switches {
            // The switch of `xeInit.pas` and the flag of the port for the
            // same load setting.
            command.arg(match name.as_str() {
                "PseudoESL" | "PseudoLight" | "PseudoSmall" => "--pseudo-light",
                "IgnoreESL" | "IgnoreLight" | "IgnoreSmall" => "--ignore-light",
                "PseudoMedium" => "--pseudo-medium",
                "IgnoreMedium" => "--ignore-medium",
                "PseudoUpdate" => "--pseudo-update",
                "IgnoreUpdate" => "--ignore-update",
                "dontremoveoffsetdata" => "--dont-remove-offset-data",
                "reportinjected" => "--report-injected",
                "noreportinjected" => "--no-report-injected",
                "TrackAllEditorID" => "--track-all-editor-id",
                "moreunknown" => "--more-unknown",
                "EnforceAllMasters" => "--enforce-all-masters",
                "speed" => "--speed-over-memory",
                "memory" => "--memory-over-speed",
                "FillPNAM" => "--fill-pnam",
                other => panic!("no port flag for the switch -{other}"),
            });
        }
        command
            // `-IKnowIllBreakMyGameWithThis` on the oracle's side: without it
            // the private copy's game master is not editable and the mutating
            // cases do not save.
            .arg("--allow-edit-game-master")
            // The folder standing in for the oracle's `bin` (`ProgramPath()`,
            // where a script's `ProgramPath + 'Edit Scripts\...'` lands,
            // and fails to, as on the oracle).
            .arg("--program-path")
            .arg(format!("{}\\", bin.display()))
            .arg("--my-games")
            .arg(format!("{}\\", my_games.display()))
            .arg("--game-ini")
            .arg(my_games.join("game.ini"))
            // The GUI's `-T:`: the temporary folder of the run, which a
            // script's `wbTempPath` reports.
            .arg("--temp-path")
            .arg(format!("{}\\temp\\", dir.display()))
            // The GUI's `xeSettingsFileName` (`wbSettingsFileName`): no
            // `<AppName>Edit.ini` beside its program, so the plugin list's
            // name with `.<app>viewsettings` (`-P:<work>\plugins.txt`).
            .arg("--settings")
            .arg(dir.join(format!("plugins.{}viewsettings", task.game.mode.to_lowercase())))
            .env("XEDIT_SCRIPTS", &dir);
        for plugin in &plugins {
            command.arg("--load").arg(plugin);
        }
        command
            .arg("script")
            .arg("run")
            .arg(dir.join(script))
            .arg("--scripts")
            .arg(&dir);
        if !task.answers.is_empty() {
            // The recorded answers are a `script run` option.
            command.arg("--answers").arg(&answers_file);
        }
        if task.build_refs {
            // The oracle's script mode builds the references at its load
            // unless `-nobuildrefs` was passed; the cases that read them say
            // so.
            command.arg("--build-refs");
        }
        if !task.close_after {
            // The oracle's GUI is not closed, so its `SaveChanged` never
            // runs: the port keeps what the script changed in memory too
            // (an aborted run's save lines are part of the compared log,
            // and a saved file would be one of the run's data files).
            command.arg("--no-save");
        }
        command.current_dir(&dir).stdin(std::process::Stdio::null());
        command
    };
    if task.refcache {
        // The first run writes the cache files the case's run then reads.
        fs::create_dir_all(&cache)?;
        fs::write(dir.join(PREP_SCRIPT), PREP_SCRIPT_TEXT)?;
        let output = port_command(PREP_SCRIPT).output()?;
        let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
        ensure!(
            envelope["ok"] == serde_json::Value::Bool(true),
            "the port's reference build run failed: {}",
            String::from_utf8_lossy(&output.stdout)
                .chars()
                .take(400)
                .collect::<String>()
        );
        ensure!(
            !cache_files_of(&cache)?.is_empty(),
            "the port wrote no reference cache file into {}",
            cache.display()
        );
    }
    let mut command = port_command(&task.script);
    let started = std::time::Instant::now();
    let output = command.output()?;
    outcome.port_seconds = Some(started.elapsed().as_secs_f64());
    let report = dir.join("port.json");
    fs::write(&report, &output.stdout)?;
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    if envelope["ok"] != serde_json::Value::Bool(true) {
        outcome.status = "port-failed";
        outcome.detail = Some(format!(
            "  {}, see {}",
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .chars()
                .take(400)
                .collect::<String>(),
            report.display()
        ));
        return Ok(outcome);
    }
    let messages = envelope["result"]["messages"]
        .as_array()
        .map(|lines| {
            lines
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let oracle_log = fs::read_to_string(oracle_dir.join("log.txt"))?;
    // Paths of two different runs stand for each other: the oracle's work
    // folder and the port's run folder become `<root>`, and the wall clock
    // of a save suffix becomes `<time>`.
    let oracle_root = work_root_of(&oracle_log);
    let port_root = dir.display().to_string();
    let oracle_lines = normalize_lines(script_lines(&oracle_log), oracle_root.as_deref());
    let port_lines = normalize_lines(script_lines(&messages), Some(&port_root));
    let mut differences: Vec<String> = Vec::new();
    if oracle_lines != port_lines {
        let first = oracle_lines
            .iter()
            .zip(&port_lines)
            .position(|(a, b)| a != b)
            .unwrap_or(oracle_lines.len().min(port_lines.len()));
        differences.push(format!(
            "  log lines: oracle {}, port {}; first difference at line {first}:\n    oracle: {}\n    port:   {}",
            oracle_lines.len(),
            port_lines.len(),
            oracle_lines.get(first).map_or("<end>", String::as_str),
            port_lines.get(first).map_or("<end>", String::as_str)
        ));
    }
    // The files: every file the oracle captured, by the path both sides map
    // to, plus the port's own. Files are hashed in place (the port writes
    // into its own folder).
    let skip = plugin_names(&plugins, task.game.mode);
    // Probes compare their log lines alone: the phase writes nothing for the
    // comparison to hold, and the oracle's capture is run-phase only anyway
    // (a probe of the corpus that writes its own scratch files, `streams.pas`,
    // would otherwise differ on files only the port captured).
    // The file differences are kept apart from the log's: a memo loss
    // excuses missing log lines only, never a written or saved file that
    // differs (the `equal-log-loss` status once covered both, so a saved
    // plugin whose bytes differed read as equal whenever the memo had lost
    // lines; found by the fix-sse-filters branch on `Add Prefix Plus Name To
    // EDID.pas`, whose 850 emptied records never showed).
    let mut file_differences = Vec::new();
    if task.phase == "run" {
        let (port_files, port_saved) = capture_port(&dir, &skip, &task.save)?;
        compare_files(&oracle.files, &port_files, "written", &mut file_differences);
        compare_files(&oracle.saved, &port_saved, "saved", &mut file_differences);
    }
    outcome.files = oracle.files.len() + oracle.saved.len();
    outcome.log_lines = oracle_lines.len();
    if differences.is_empty() && file_differences.is_empty() {
        outcome.status = "equal";
    } else if file_differences.is_empty()
        && let Some(lost) = memo_loss(&oracle_lines, &port_lines)
    {
        outcome.status = "equal-log-loss";
        outcome.detail = Some(format!(
            "  the oracle's memo of the run lost {} lines in {} runs; every line it kept is the port's, in order, and the first, last and Done lines are equal",
            port_lines.len() - oracle_lines.len(),
            lost
        ));
    } else {
        if !file_differences.is_empty()
            && let Some(lost) = memo_loss(&oracle_lines, &port_lines)
        {
            differences.push(format!(
                "  the oracle's memo of the run lost {} lines in {} runs (every line it kept is the port's); the files differ:",
                port_lines.len() - oracle_lines.len(),
                lost
            ));
        }
        differences.extend(file_differences);
        outcome.status = "different";
        let extra = differences.len().saturating_sub(LISTED_DIFFERENCES);
        differences.truncate(LISTED_DIFFERENCES);
        if extra > 0 {
            differences.push(format!("  ... {extra} more differences"));
        }
        outcome.detail = Some(differences.join("\n"));
    }
    if !options.keep {
        // Keep the port's folder only when something differed.
        if outcome.status == "equal" {
            let _ = fs::remove_dir_all(&dir);
        }
    }
    Ok(outcome)
}

/// The files a port run left: its output folder, the run folder itself (the
/// script folder), the private data folder less the plugins that went in,
/// and its `bin` (the program folder).
fn capture_port(
    dir: &Path,
    skip: &BTreeSet<String>,
    save: &[String],
) -> Result<(Vec<CapturedFile>, Vec<CapturedFile>)> {
    let mut files = Vec::new();
    let mut saved = Vec::new();
    let roots = [
        (dir.join("out"), "out"),
        (dir.to_path_buf(), "script"),
        (dir.join("Data"), "data"),
        (dir.join("bin"), "program"),
    ];
    for (root, prefix) in &roots {
        if !root.is_dir() {
            continue;
        }
        let mut stack = vec![root.clone()];
        while let Some(folder) = stack.pop() {
            for entry in fs::read_dir(&folder)? {
                let path = entry?.path();
                if path.is_dir() {
                    // The private folders of the run are walked of their own,
                    // not as part of the run folder.
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                    if *prefix == "script" && RUN_FOLDERS.contains(&name.as_str()) {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let lower = name.to_lowercase();
                // A plugin the case saves is compared under the name of the
                // case, as the oracle's capture maps it; the port's copies
                // live in the private data folder and the check comes before
                // the machinery of the load, which every plugin is part of.
                if *prefix == "data"
                    && let Some(plugin) = save.iter().find(|plugin| saved_matches(plugin, &lower))
                {
                    saved.push(capture(&path, &format!("saved/{plugin}"))?);
                    continue;
                }
                let relative = path.strip_prefix(root).unwrap_or(&path);
                let mapped = format!("{prefix}/{}", relative.to_string_lossy().replace('\\', "/"));
                // The machinery of the run, not output of the script: the
                // port's report, the script and its units, the plugins that
                // went in, and the same program artifacts the oracle drops.
                let machinery = match *prefix {
                    "out" => false,
                    "script" => {
                        lower == "port.json"
                            || lower == "answers.json"
                            || lower.ends_with(".pas")
                            || skip.contains(&lower)
                    }
                    "program" => is_gui_artifact(&lower) || skip.contains(&lower) || is_windows_executable(&path),
                    _ => skip.contains(&lower),
                };
                if machinery {
                    continue;
                }
                files.push(capture(&path, &mapped)?);
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    saved.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((files, saved))
}

/// Compares two captured file sets and names what differs.
fn compare_files(oracle: &[CapturedFile], port: &[CapturedFile], what: &str, differences: &mut Vec<String>) {
    for file in oracle {
        match port.iter().find(|other| other.path == file.path) {
            Some(other) if other.hash == file.hash && other.size == file.size => {}
            Some(other) => differences.push(format!(
                "  {what} file {}: oracle {} bytes (hash {:016x}), port {} bytes (hash {:016x})",
                file.path, file.size, file.hash, other.size, other.hash
            )),
            None => differences.push(format!("  {what} file {} only in the oracle run", file.path)),
        }
    }
    for file in port {
        if !oracle.iter().any(|other| other.path == file.path) {
            differences.push(format!("  {what} file {} only in the port run", file.path));
        }
    }
}

/// The lines of the script section of a message log: from the
/// `Start: Applying script` line through [`SCRIPT_END_LINE`], without the
/// `[mm:ss] ` prefixes and the elapsed time of the `Done:` line. A script
/// whose text has a `Mode:` keyword runs with `bShowMessages` off, and the
/// GUI then suppresses every progress line of the run with
/// `wbProgressLock` -- the `Start:`, the `Done:` and the script's own
/// `AddMessage` lines among them, which reach the memo through the same
/// callback -- so the section is the closing line alone.
fn script_lines(log: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in log.lines() {
        let raw = line.trim_end_matches(['\r', '\n']);
        // The oracle's memo (SynEdit) gives a line only up to a NUL in it
        // (`gui::main_form_log`), so a line is compared up to its first NUL
        // (`log-nul.pas`); the rest of it cannot be read from the oracle.
        let raw = raw.split('\0').next().unwrap_or_default();
        let stripped = strip_time(raw);
        let text = match stripped.split_once(", Elapsed Time:") {
            Some((before, _)) => before.to_owned(),
            None => stripped.to_owned(),
        };
        if text == SCRIPT_END_LINE {
            lines.push(text);
            break;
        }
        if text.starts_with("Start: Applying script") {
            inside = true;
        }
        if inside {
            lines.push(text);
        }
    }
    lines
}

/// A line without the `[mm:ss] ` (or `[h:mm:ss] `) of `wbProgress`.
fn strip_time(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix('[')
        && let Some(end) = rest.find("] ")
        && end >= 5
        && rest[..end].chars().all(|c| c.is_ascii_digit() || c == ':')
    {
        return &rest[end + 2..];
    }
    line
}

/// The work folder of an oracle log (`Using Backup Path: <work>\backup\`),
/// the absolute path whose occurrences a comparison maps to `<root>`. The
/// recorded path may carry a doubled trailing separator (`xeInit` appends
/// one to the `-B:` value), so the root is what precedes `\backup`.
fn work_root_of(log: &str) -> Option<String> {
    for line in log.lines() {
        let line = strip_time(line.trim_end_matches(['\r', '\n']));
        if let Some(rest) = line.strip_prefix("Using Backup Path: ")
            && let Some(at) = rest.find("\\backup")
        {
            return Some(rest[..at].to_owned());
        }
    }
    None
}

/// Normalizes a log section for the comparison: the run's own absolute path
/// becomes `<root>` (the oracle's lines carry its work folder, the port's its
/// run folder), and the digits of a `.save.<timestamp>` suffix become
/// `<time>` (the two runs saved at different moments of the clock).
fn normalize_lines(lines: Vec<String>, root: Option<&str>) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let line = match root {
            Some(root) if !root.is_empty() => replace_ci(&line, root, "<root>"),
            _ => line,
        };
        out.push(normalize_save_time(line));
    }
    out
}

/// `text.replace(needle, replacement)` without regard to case: the two runs
/// print the same base folder, but a side could differ in the case of a path
/// component.
fn replace_ci(text: &str, needle: &str, replacement: &str) -> String {
    // ASCII case folding keeps the byte lengths equal, so the found offsets
    // in the folded copy are the offsets in the text.
    let folded = fold_ascii(text);
    let folded_needle = fold_ascii(needle);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = folded[at..].find(&folded_needle) {
        out.push_str(&text[at..at + found]);
        out.push_str(replacement);
        at += found + needle.len();
    }
    out.push_str(&text[at..]);
    out
}

/// A copy of `text` with ASCII letters lower-cased; byte lengths are
/// preserved.
fn fold_ascii(text: &str) -> String {
    text.chars().map(|c| c.to_ascii_lowercase()).collect()
}

/// The digits of every `.save.<yyyy_mm_dd_hh_nn_ss>` suffix become
/// `<time>`, so the `Saving:` and `Queued renaming of save` lines of a
/// cached oracle log and of a port run compare.
fn normalize_save_time(mut line: String) -> String {
    let mut from = 0;
    while let Some(found) = line[from..].find(".save.") {
        let start = from + found + ".save.".len();
        let mut end = start;
        while let Some(c) = line[end..].chars().next() {
            if !(c.is_ascii_digit() || c == '_') {
                break;
            }
            end += c.len_utf8();
        }
        if end > start {
            line.replace_range(start..end, "<time>");
            from = start + "<time>".len();
        } else {
            from = start;
        }
    }
    line
}

/// Whether the oracle's lines are the port's with runs left out, under the
/// memo-loss rule: the port is the superset in order, and the first, last and
/// `Done:` lines are equal. `Some(runs)` is the class `parity check` calls
/// `equal-log-loss`; the oracle's log is the GUI memo, which loses `AddMessage`
/// lines to its `PostAddMessage` queue. The rule cannot tell a lost line from
/// a port line the oracle never had -- the class says so by its name -- so the
/// guards are the lines that must match.
fn memo_loss(oracle: &[String], port: &[String]) -> Option<usize> {
    if oracle.len() >= port.len() || oracle.first() != port.first() || oracle.last() != port.last() {
        return None;
    }
    let done = |lines: &[String]| {
        lines
            .iter()
            .find(|line| line.starts_with("Done: Applying script"))
            .cloned()
    };
    if done(oracle) != done(port) {
        return None;
    }
    lost_blocks(oracle, port)
}

/// Whether `oracle` is `port` with runs of lines left out, and how many
/// runs: the lines a log lost (`parity check`'s helper, the same rule).
fn lost_blocks(oracle: &[String], port: &[String]) -> Option<usize> {
    let mut blocks = 0;
    let mut in_gap = false;
    let mut next = oracle.iter().peekable();
    for line in port {
        if next.peek().is_some_and(|expected| *expected == line) {
            next.next();
            in_gap = false;
        } else {
            if !in_gap {
                blocks += 1;
            }
            in_gap = true;
        }
    }
    (next.peek().is_none() && blocks > 0).then_some(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_classes_by_its_dialogs() {
        assert_eq!(class_of("a.pas", "var f: TForm;"), Class::Form);
        assert_eq!(class_of("a.pas", "f.ShowModal;"), Class::Form);
        assert_eq!(class_of("a.pas", "s := InputQuery('x', 'y', s);"), Class::Interactive);
        assert_eq!(class_of("a.pas", "AddMessage('ok');"), Class::Headless);
        assert_eq!(class_of("a.pas", "fv := GetFormVersion(rec);"), Class::Headless);
        assert_eq!(class_of("a.pas", "{ a TForm in a comment }"), Class::Headless);
        assert_eq!(class_of("xEditAPI.pas", "unit xEditAPI; end."), Class::Api);
    }

    #[test]
    fn unit_names_of_uses_clauses() {
        assert_eq!(used_units("uses\n  SysUtils, Classes;\n"), vec!["Classes", "SysUtils"]);
        assert_eq!(used_units("// uses nothing;\n"), Vec::<String>::new());
        assert_eq!(
            used_units("uses xEditAPI, Classes, SysUtils, StrUtils, Windows;"),
            vec!["Classes", "StrUtils", "SysUtils", "Windows", "xEditAPI"]
        );
        assert_eq!(used_units("causes a, b;"), Vec::<String>::new());
    }

    #[test]
    fn namespaces_are_stripped_as_the_host_strips_them() {
        assert_eq!(strip_namespace("Vcl.Graphics"), "Graphics");
        assert_eq!(strip_namespace("System.SysUtils"), "SysUtils");
        assert_eq!(strip_namespace("StrUtils"), "StrUtils");
    }

    #[test]
    fn the_script_section_of_a_log() {
        let log = "[00:00] Loading files...\n[00:05] Start: Applying script \"a\"\nhello\n[00:06] Done: Applying script \"a\", Processed Records: 1, Elapsed Time: 00:01\nYou can close this application now.\n";
        assert_eq!(
            script_lines(log),
            vec![
                "Start: Applying script \"a\"",
                "hello",
                "Done: Applying script \"a\", Processed Records: 1",
                "You can close this application now."
            ]
        );
        // A `Mode: Silent` script: every progress line of the run is
        // suppressed, so the section is the closing line.
        let silent = "[00:00] Loading files...
Setting a bookmark
[00:06] You can close this application now.
";
        assert_eq!(script_lines(silent), vec!["You can close this application now."]);
        assert_eq!(script_lines("[00:00] Loading files...\n"), Vec::<String>::new());
    }

    #[test]
    fn the_two_runs_paths_and_clock_normalize() {
        let oracle = concat!(
            "Using Backup Path: M:\\cache\\oracle-work\\run-x.abc.123\\backup\\\r\n",
            "[00:06] Start: Applying script \"q\"\r\n",
            "[00:07] Queued renaming of save \"M:\\cache\\oracle-work\\run-x.abc.123\\Data\\\\Skyrim.esm.save.2026_10_10_16_57_47\" to \"M:\\cache\\oracle-work\\run-x.abc.123\\Data\\\\Skyrim.esm\" on shutdown.\r\n",
            "[00:08] Exception in unit UserScript line 27: [EFCreateError] Cannot create file \"M:\\cache\\oracle-work\\run-x.abc.123\\bin\\Edit Scripts\\cells.txt\". The system cannot find the path specified\r\n",
            "You can close this application now.\r\n",
        );
        let root = work_root_of(oracle).unwrap();
        assert_eq!(root, "M:\\cache\\oracle-work\\run-x.abc.123");
        // The `-B:` value's doubled trailing separator does not confuse it.
        assert_eq!(
            work_root_of("Using Backup Path: M:\\cache\\w\\backup\\\\\r\n").unwrap(),
            "M:\\cache\\w"
        );
        let oracle_lines = normalize_lines(script_lines(oracle), Some(&root));
        assert_eq!(
            oracle_lines,
            vec![
                "Start: Applying script \"q\"".to_owned(),
                "Queued renaming of save \"<root>\\Data\\\\Skyrim.esm.save.<time>\" to \"<root>\\Data\\\\Skyrim.esm\" on shutdown."
                    .to_owned(),
                "Exception in unit UserScript line 27: [EFCreateError] Cannot create file \"<root>\\bin\\Edit Scripts\\cells.txt\". The system cannot find the path specified"
                    .to_owned(),
                "You can close this application now.".to_owned(),
            ]
        );
        let port = "[00:06] Start: Applying script \"q\"\n\
                    [00:07] Queued renaming of save \"M:\\scratch\\SSE-port\\run-x\\Data\\\\Skyrim.esm.save.2026_10_10_21_02_08\" to \"M:\\scratch\\SSE-port\\run-x\\Data\\\\Skyrim.esm\" on shutdown.\n\
                    [00:08] Exception in unit UserScript line 27: [EFCreateError] Cannot create file \"M:\\scratch\\SSE-port\\run-x\\bin\\Edit Scripts\\cells.txt\". The system cannot find the path specified\n\
                    You can close this application now.\n";
        let port_lines = normalize_lines(script_lines(port), Some("M:\\scratch\\SSE-port\\run-x"));
        assert_eq!(oracle_lines, port_lines);
        // A path outside either root is left alone; a `.save.` without
        // digits is too.
        assert_eq!(
            normalize_save_time("Saving: x.save.txt".to_owned()),
            "Saving: x.save.txt"
        );
        assert_eq!(
            normalize_save_time("Saving: Skyrim.esm.save.2026_10_10_16_57_47".to_owned()),
            "Saving: Skyrim.esm.save.<time>"
        );
        assert_eq!(replace_ci("A:\\B\\C", "a:\\b", "<root>"), "<root>\\C");
    }

    #[test]
    fn a_memo_truncated_log_only_passes_the_superset_rule_with_equal_guards() {
        let mut port: Vec<String> = (0..50).map(|i| format!("body {i}")).collect();
        port.insert(0, "Start: Applying script \"x\"".to_owned());
        port.push("Done: Applying script \"x\", Processed Records: 50".to_owned());
        port.push("You can close this application now.".to_owned());
        let mut oracle = port.clone();
        oracle.drain(30..40);
        assert_eq!(memo_loss(&oracle, &port), Some(1));
        let mut other_done = port.clone();
        let at = other_done.len() - 2;
        other_done[at] = "Done: Applying script \"x\", Processed Records: 51".to_owned();
        assert_eq!(memo_loss(&other_done, &port), None);
        let mut other_start = port.clone();
        other_start[0] = "Start: Applying script \"y\"".to_owned();
        assert_eq!(memo_loss(&other_start, &port), None);
        assert_eq!(memo_loss(&port, &port), None);
    }

    #[test]
    fn a_minimal_master_has_a_tes4_header() {
        let master = minimal_master(0);
        assert_eq!(&master[..4], b"TES4");
        let size = u32::from_le_bytes(master[4..8].try_into().unwrap()) as usize;
        assert_eq!(master.len(), 24 + size);
        assert_eq!(
            u32::from_le_bytes(master[8..12].try_into().unwrap()) & 1,
            1,
            "the ESM flag"
        );
        assert!(master.windows(4).any(|window| window == b"HEDR"));
        assert!(!master.windows(4).any(|window| window == b"DATA"));
    }
}
