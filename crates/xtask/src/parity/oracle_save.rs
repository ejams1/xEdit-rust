// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The oracle's own saves.
//!
//! `cargo xtask parity oracle-save` has the GUI build of xEdit load every
//! corpus plugin with its masters and write it with `FileWriteToStream`
//! (`oracle/save.pas`), which is the `WriteToStream` with `PrepareSave`
//! that the GUI's save runs. The saved bytes are cached zstd-compressed as
//! `<cache>/<tag>/<MODE>-oracle-save/<file>.<key>.saved.zst`, or the
//! message of the exception the save raised as `<file>.<key>.error`; the
//! key hashes the plugin, its masters and the script. The port saves the
//! same plugin (`xedit save`, as the round trip) and the two files are
//! compared byte for byte: `equal` whatever the save changed in the input,
//! `equal-error` when both refused the save with the same message,
//! `different` otherwise.
//!
//! `cargo xtask parity oracle-edit` runs the scripted edit sequences of
//! `crates/xtask/oracle/edits/*.json` on both: the port runs the commands
//! as an `xedit batch`, the oracle a Pascal script generated from the same
//! commands (`edit_script`), and every plugin the sequence saves is
//! compared.

use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::Value;

use super::gui::{self, GuiRun};
use super::{
    Case, GIB, Game, Options, Outcome, PortSave, Runner, UNMEASURED_PEAK_FACTOR, content_hash,
    describe_record_differences, main_record_header_size, port_save,
};

/// The script of the plain save.
const SAVE_SCRIPT: &str = include_str!("../../oracle/save.pas");

/// How long a GUI run may take unless `--oracle-timeout` says otherwise.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120 * 60);

/// A GUI run that used no CPU for this long is stuck.
const HANG_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The cached result of the oracle on one plugin.
pub(super) enum OracleSave {
    /// The saved plugin, zstd-compressed.
    Saved(PathBuf),
    /// The message of the exception the save raised.
    Error(String),
}

impl OracleSave {
    fn find(dir: &Path, stem: &str) -> Result<Option<Self>> {
        let saved = dir.join(format!("{stem}.saved.zst"));
        if saved.exists() {
            return Ok(Some(Self::Saved(saved)));
        }
        let error = dir.join(format!("{stem}.error"));
        if error.exists() {
            return Ok(Some(Self::Error(fs::read_to_string(error)?.trim().to_owned())));
        }
        Ok(None)
    }
}

/// The masters named in the file header of a plugin, in their order.
fn header_masters(path: &Path, header_size: usize) -> Result<Vec<String>> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut head = vec![0u8; header_size];
    file.read_exact(&mut head)?;
    let data_size = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
    let mut data = vec![0u8; data_size];
    file.read_exact(&mut data)?;
    // Morrowind's subrecords have a four byte size; the later games two,
    // with `XXXX` giving the size of the next one.
    let wide = header_size == 16;
    let mut masters = Vec::new();
    let mut pos = 0;
    let mut next_size = None;
    while pos + if wide { 8 } else { 6 } <= data.len() {
        let signature = &data[pos..pos + 4];
        let (size, start) = if wide {
            (
                u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize,
                pos + 8,
            )
        } else {
            let size = u16::from_le_bytes(data[pos + 4..pos + 6].try_into().unwrap()) as usize;
            (next_size.take().unwrap_or(size), pos + 6)
        };
        let body = data.get(start..start + size).context("truncated file header")?;
        if signature == b"XXXX" && size == 4 {
            next_size = Some(u32::from_le_bytes(body.try_into().unwrap()) as usize);
        } else if signature == b"MAST" {
            let end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
            masters.push(String::from_utf8_lossy(&body[..end]).into_owned());
        }
        pos = start + size;
    }
    Ok(masters)
}

/// The file of `name` in `data`, found without regard to case.
pub(super) fn find_in(data: &Path, name: &str) -> Result<PathBuf> {
    let direct = data.join(name);
    if direct.exists() {
        // Keep the spelling of the folder listing, which the GUI shows.
        for entry in fs::read_dir(data)? {
            let path = entry?.path();
            if path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
            {
                return Ok(path);
            }
        }
        return Ok(direct);
    }
    bail!("master {name} is not in {}", data.display())
}

/// The plugins the GUI needs for `names`: their masters, recursively, each
/// before the plugins that need it, then the plugins themselves.
pub(super) fn load_list(game: &Game, data: &Path, names: &[&str]) -> Result<Vec<PathBuf>> {
    fn visit(game: &Game, data: &Path, name: &str, seen: &mut Vec<String>, out: &mut Vec<PathBuf>) -> Result<()> {
        if seen.iter().any(|s| s.eq_ignore_ascii_case(name)) {
            return Ok(());
        }
        seen.push(name.to_owned());
        let path = find_in(data, name)?;
        for master in header_masters(&path, main_record_header_size(game.mode))? {
            visit(game, data, &master, seen, out)?;
        }
        out.push(path);
        Ok(())
    }
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for name in names {
        visit(game, data, name, &mut seen, &mut out)?;
    }
    Ok(out)
}

/// The cache key of an oracle run: the contents of the plugins it loads,
/// the script and the executable's name.
pub(super) fn oracle_key(runner: &Runner, plugins: &[PathBuf], script: &str, exe: &str) -> Result<u64> {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |value: u64| {
        for byte in value.to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for plugin in plugins {
        let known = runner.hashes.lock().unwrap().get(plugin).copied();
        let value = match known {
            Some(value) => value,
            None => {
                let value = content_hash(plugin)?;
                runner.hashes.lock().unwrap().insert(plugin.clone(), value);
                value
            }
        };
        mix(value);
    }
    mix(text_hash(script));
    mix(text_hash(exe));
    Ok(hash)
}

/// FNV-1a of a text with its line endings normalised to LF: a checkout with
/// `core.autocrlf` turns the embedded scripts into CRLF, and the key of a
/// cached oracle run must not depend on that.
fn text_hash(text: &str) -> u64 {
    text.replace("\r\n", "\n")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

/// A string as a Pascal literal.
fn pascal_string(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The plain-save script for one plugin.
fn save_script(name: &str) -> String {
    fill_save_script(SAVE_SCRIPT, name)
}

/// `template` with the plugin filled in, and LF line endings.
fn fill_save_script(template: &str, name: &str) -> String {
    let literal = pascal_string(name);
    template
        .replace("\r\n", "\n")
        .replace("'{{FILE}}'", &literal)
        .replace("{{FILE}}", name)
}

/// Where the oracle saves of a game are cached.
fn oracle_dir(runner: &Runner, game: &Game) -> PathBuf {
    runner.cache.join(format!("{}-oracle-save", game.mode))
}

/// The plain save of `case` by the oracle, when it is cached.
pub(super) fn cached(case: &Case, runner: &Runner) -> Result<Option<OracleSave>> {
    let dir = oracle_dir(runner, case.game);
    if !dir.exists() {
        return Ok(None);
    }
    let plugins = load_list(case.game, &case.data, &[&case.name])?;
    let key = oracle_key(
        runner,
        &plugins,
        &save_script(&case.name),
        gui::exe_name(case.game.mode),
    )?;
    OracleSave::find(&dir, &format!("{}.{key:016x}", case.name))
}

/// The offset of the first byte where two streams differ, or `None` when
/// they are identical; a length difference counts at the shorter length.
fn first_difference(a: impl Read, b: impl Read) -> Result<Option<u64>> {
    use std::io::BufRead;
    let mut a = BufReader::with_capacity(1 << 20, a);
    let mut b = BufReader::with_capacity(1 << 20, b);
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

pub(super) fn zstd_reader(path: &Path) -> Result<impl Read> {
    Ok(zstd::Decoder::new(
        File::open(path).with_context(|| format!("opening {}", path.display()))?,
    )?)
}

/// Compares the port's save of `case` (`saved`) with the oracle's: `equal`
/// or `different` with a detail. A difference keeps `saved` and writes the
/// oracle's bytes next to it as `<stem>.oracle.saved`.
pub(super) fn compare(
    case: &Case,
    oracle: &OracleSave,
    saved: &Path,
    dir: &Path,
    stem: &str,
) -> Result<(&'static str, String)> {
    let oracle_path = match oracle {
        OracleSave::Saved(path) => path,
        OracleSave::Error(message) => {
            return Ok((
                "different",
                format!(
                    "  the oracle refused the save ({message}), the port saved {}",
                    saved.display()
                ),
            ));
        }
    };
    match first_difference(zstd_reader(oracle_path)?, File::open(saved)?)? {
        None => {
            // What the save changed in the input, which the oracle confirms.
            let note = match first_difference(zstd_reader(oracle_path)?, File::open(&case.input)?)? {
                None => "  equal to the oracle save, which equals the input".to_owned(),
                Some(offset) => format!(
                    "  equal to the oracle save, which differs from the input from byte {offset} (input {} bytes, saved {} bytes)",
                    fs::metadata(&case.input)?.len(),
                    fs::metadata(saved)?.len()
                ),
            };
            Ok(("equal", note))
        }
        Some(offset) => {
            let mut oracle_bytes = Vec::new();
            zstd_reader(oracle_path)?.read_to_end(&mut oracle_bytes)?;
            let oracle_copy = dir.join(format!("{stem}.oracle.saved"));
            fs::write(&oracle_copy, &oracle_bytes)?;
            let port_bytes = fs::read(saved)?;
            let (_, report) =
                describe_record_differences(&oracle_bytes, &port_bytes, main_record_header_size(case.game.mode));
            Ok((
                "different",
                format!(
                    "  first difference to the oracle save at byte {offset} (0x{offset:X}); oracle {} bytes, port {} bytes\n  {report} (dropped: only in the oracle save, added: only in the port's)\n  oracle: {}\n  port:   {}",
                    oracle_bytes.len(),
                    port_bytes.len(),
                    oracle_copy.display(),
                    saved.display()
                ),
            ))
        }
    }
}

/// The expected peak of a GUI run: its last peak, or an estimate from the
/// size of the plugins it loads.
fn expected_peak(peak_file: &Path, plugins: &[PathBuf]) -> u64 {
    if let Some(peak) = fs::read_to_string(peak_file)
        .ok()
        .and_then(|text| text.trim().parse().ok())
    {
        return peak;
    }
    let size: u64 = plugins
        .iter()
        .map(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
        .sum();
    (size * UNMEASURED_PEAK_FACTOR).max(2 * GIB)
}

/// Runs the GUI oracle with `script` on `plugins` (masters first) and
/// returns the status lines and the output folder; `work` is removed by
/// the caller.
pub(super) fn run_gui(
    runner: &Runner,
    game: &Game,
    plugins: Vec<PathBuf>,
    script: String,
    build_refs: bool,
    work: PathBuf,
    peak_file: &Path,
) -> Result<gui::GuiResult> {
    let exe = runner.oracle_dir.join(gui::exe_name(game.mode));
    ensure!(exe.exists(), "{} does not exist", exe.display());
    let expected_peak = expected_peak(peak_file, &plugins);
    let run = GuiRun {
        exe: &exe,
        mode: game.mode,
        star_plugins_txt: !gui::simple_plugins_txt(game.mode),
        plugins,
        script,
        build_refs,
        work,
        timeout: runner.oracle_timeout.unwrap_or(DEFAULT_TIMEOUT),
        hang_timeout: HANG_TIMEOUT,
        budget: &runner.budget,
        expected_peak,
        max_memory: runner.max_memory,
    };
    let result = run.run();
    if let Ok(result) = &result
        && let Some(peak) = result.peak
    {
        fs::write(peak_file, peak.to_string())?;
    }
    if result.is_err() {
        gui::remove_work(&run.work);
    }
    result
}

/// Compresses a file the oracle wrote into the cache, unless another run
/// put it there first.
pub(super) fn keep_compressed(source: &Path, cached: &Path) -> Result<()> {
    if cached.exists() {
        return Ok(());
    }
    let partial = cached.with_extension(format!("{}.partial", std::process::id()));
    let mut encoder = zstd::Encoder::new(File::create(&partial)?, 3)?;
    std::io::copy(&mut File::open(source)?, &mut encoder)?;
    encoder.finish()?;
    if cached.exists() {
        fs::remove_file(&partial)?;
    } else {
        fs::rename(&partial, cached)?;
    }
    Ok(())
}

/// Moves the outcome of a script into the cache: the file it saved as
/// `<stem><suffix>.saved.zst`, or its error as `<stem><suffix>.error`.
fn keep_result(status: &[String], saved: &Path, dir: &Path, stem: &str) -> Result<()> {
    let last = status.last().map(String::as_str).unwrap_or("");
    if last == "done" {
        ensure!(
            saved.exists(),
            "the script reported done but wrote no {}",
            saved.display()
        );
        keep_compressed(saved, &dir.join(format!("{stem}.saved.zst")))
    } else if let Some(message) = last.strip_prefix("error: ") {
        fs::write(dir.join(format!("{stem}.error")), message)?;
        Ok(())
    } else {
        bail!("the script ended without a result: {}", status.join(" / "))
    }
}

/// `parity oracle-save` for one plugin.
pub(super) fn check(case: &Case, runner: &Runner) -> Result<Outcome> {
    if case.game.mode == "TES3" {
        // `xeInit.pas`: Morrowind's tool modes are `[tmView]`, so the GUI
        // shows "Application Morrowind does not currently support Script"
        // and saves no Morrowind plugin in any mode.
        return Ok(Outcome {
            game: case.game.name,
            file: case.name.clone(),
            status: "oracle-unsupported",
            detail: Some("  the 4.1.5q GUI opens Morrowind in the view mode only and saves nothing".to_owned()),
            oracle_bytes: 0,
            port_bytes: 0,
            oracle_peak: None,
            port_peak: None,
        });
    }
    let dir = oracle_dir(runner, case.game);
    fs::create_dir_all(&dir)?;
    let plugins = load_list(case.game, &case.data, &[&case.name])?;
    let script = save_script(&case.name);
    let key = oracle_key(runner, &plugins, &script, gui::exe_name(case.game.mode))?;
    let stem = format!("{}.{key:016x}", case.name);
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
    let oracle = match OracleSave::find(&dir, &stem)? {
        Some(oracle) => oracle,
        None => {
            let work = runner
                .scratch
                .join("oracle-work")
                .join(format!("{stem}.{}", std::process::id()));
            let peak_file = dir.join(format!("{stem}.oracle.peak"));
            let result = run_gui(runner, case.game, plugins, script, false, work.clone(), &peak_file)?;
            outcome.oracle_peak = result.peak;
            let kept = keep_result(&result.status, &result.out.join("saved"), &dir, &stem);
            fs::write(dir.join(format!("{stem}.oracle.log")), result.status.join("\n"))?;
            gui::remove_work(&work);
            kept?;
            OracleSave::find(&dir, &stem)?.context("the oracle save is missing after the run")?
        }
    };
    if let OracleSave::Saved(path) = &oracle {
        outcome.oracle_bytes = fs::metadata(path)?.len();
    }
    if runner.port.is_none() {
        return Ok(outcome);
    }
    let port_dir = runner.scratch.join(format!("{}-oracle-save", case.game.mode));
    fs::create_dir_all(&port_dir)?;
    let port_stem = format!("{}.{:016x}", case.name, content_hash(&case.input)?);
    let saved = port_dir.join(format!("{port_stem}.port.saved"));
    let port_log = port_dir.join(format!("{port_stem}.port.log"));
    match port_save(case, runner, &saved, &port_log, &mut outcome)? {
        PortSave::Written => {
            let (status, detail) = compare(case, &oracle, &saved, &port_dir, &port_stem)?;
            outcome.status = status;
            outcome.detail = Some(detail);
            if status == "equal" {
                fs::remove_file(&saved)?;
            }
        }
        PortSave::Refused(message) => match &oracle {
            OracleSave::Error(expected) if *expected == message => {
                outcome.status = "equal-error";
                outcome.detail = Some(format!("  both refused the save: {message}"));
            }
            OracleSave::Error(expected) => {
                outcome.status = "different";
                outcome.detail = Some(format!(
                    "  the oracle refused with: {expected}\n  the port refused with:   {message}"
                ));
            }
            OracleSave::Saved(path) => {
                outcome.status = "different";
                outcome.detail = Some(format!(
                    "  the port refused the save ({message}), the oracle saved it: {}",
                    path.display()
                ));
            }
        },
        PortSave::Stopped => {}
    }
    Ok(outcome)
}

/// A scripted edit sequence (`crates/xtask/oracle/edits/<name>.json`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditSequence {
    /// What the sequence covers.
    #[allow(dead_code, reason = "documentation in the file")]
    description: String,
    /// The game, as `--game` of the harness names it.
    game: String,
    /// The oracle builds the reference information after loading, which a
    /// FormID change needs (`ReferencedBy`). Building it initialises every
    /// record of the loaded files, so a saved file then carries the fix-ups
    /// of all its records, where the port initialises only the records a
    /// command touches.
    #[serde(default)]
    build_refs: bool,
    /// The plugins to load from the game's data folder; their masters load
    /// with them.
    load: Vec<String>,
    /// The commands, as an `xedit batch` takes them.
    commands: Vec<Value>,
    /// The plugins whose saved bytes are compared.
    save: Vec<String>,
}

/// The text of a JSON string parameter.
fn text_param<'a>(params: &'a Value, name: &str) -> Result<&'a str> {
    params[name]
        .as_str()
        .with_context(|| format!("parameter {name} must be a string"))
}

fn bool_param(params: &Value, name: &str) -> bool {
    params[name].as_bool().unwrap_or(false)
}

/// Fails on a parameter the translation does not know, so that a sequence
/// never runs something on the port that the oracle skips.
fn check_params(params: &Value, known: &[&str]) -> Result<()> {
    if let Some(object) = params.as_object() {
        for key in object.keys() {
            ensure!(
                known.contains(&key.as_str()),
                "parameter {key} is not translated for the oracle"
            );
        }
    }
    Ok(())
}

fn pascal_bool(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

/// One command of an edit sequence as statements of the script API, which
/// do what the session command does (see `oracle/edit.pas`).
fn edit_step(index: usize, command: &Value) -> Result<String> {
    let name = command["command"].as_str().context("a step has no command")?;
    let params = &command["params"];
    let q = pascal_string;
    let file = || text_param(params, "file").map(q);
    let form_id = || text_param(params, "form_id").map(q);
    let mut code = format!("      log.Add({});\n", q(&format!("step {index}: {name}")));
    let body = match name {
        "elements.set" => {
            check_params(params, &["form_id", "file", "path", "value"])?;
            let record = format!("RecordIn({}, {})", file()?, form_id()?);
            let path = q(text_param(params, "path")?);
            // The session command fails when the element is not there after
            // the set; the setters of the script API do nothing then.
            let check = format!(
                "
      PathElement({record}, {path});"
            );
            let set = match &params["value"] {
                Value::String(value) => format!("SetElementEditValues({record}, {path}, {});", q(value)),
                Value::Null => format!("SetToDefault(PathElement({record}, {path}));"),
                Value::Bool(value) => format!("SetElementNativeValues({record}, {path}, {});", pascal_bool(*value)),
                Value::Number(value) => format!("SetElementNativeValues({record}, {path}, {value});"),
                _ => bail!("step {index}: value must be a string, number, boolean or null"),
            };
            set + &check
        }
        "elements.add" => {
            check_params(params, &["form_id", "file", "path", "name"])?;
            let record = format!("RecordIn({}, {})", file()?, form_id()?);
            let container = match params["path"].as_str().filter(|path| !path.is_empty()) {
                Some(path) => format!("PathElement({record}, {})", q(path)),
                None => record,
            };
            let added = q(text_param(params, "name")?);
            format!(
                "el := Add({container}, {added}, True);\n      if not Assigned(el) then raise Exception.Create('can not add ' + {added});"
            )
        }
        "elements.remove" => {
            check_params(params, &["form_id", "file", "path"])?;
            format!(
                "Remove(PathElement(RecordIn({}, {}), {}));",
                file()?,
                form_id()?,
                q(text_param(params, "path")?)
            )
        }
        "records.copy" => {
            check_params(
                params,
                &[
                    "form_id",
                    "from",
                    "to",
                    "as_new",
                    "deep",
                    "prefix",
                    "suffix",
                    "prefix_remove",
                    "suffix_remove",
                ],
            )?;
            let as_new = pascal_bool(bool_param(params, "as_new"));
            let deep = pascal_bool(bool_param(params, "deep"));
            let affix = |name: &str| q(params[name].as_str().unwrap_or(""));
            // `CopyInto` of the GUI: the element (`aDeepCopy` of
            // `wbCopyElementToFile` copies its contents), or with `deep` the
            // child group of the record.
            format!(
                "r := RecordIn({from}, {id});
      f := FileNamed({to});
      AddRequiredElementMasters(r, f, {as_new}, True);
      if {deep} and Assigned(ChildGroup(r)) then
        el := wbCopyElementToFileWithPrefixAndSuffix(ChildGroup(r), f, {as_new}, True, {pr}, {sr}, {p}, {s})
      else
        el := wbCopyElementToFileWithPrefixAndSuffix(r, f, {as_new}, True, {pr}, {sr}, {p}, {s});
      if not Assigned(el) then raise Exception.Create('the copy failed');",
                from = q(text_param(params, "from")?),
                id = form_id()?,
                to = q(text_param(params, "to")?),
                pr = affix("prefix_remove"),
                sr = affix("suffix_remove"),
                p = affix("prefix"),
                s = affix("suffix"),
            )
        }
        "records.delete" => {
            check_params(params, &["form_id", "file"])?;
            format!("Remove(OwnRecord({}, {}));", file()?, form_id()?)
        }
        "masters.add" => {
            check_params(params, &["file", "masters", "sort"])?;
            let masters = params["masters"].as_array().context("masters must be an array")?;
            let mut text = "sl := TStringList.Create;\n      try\n".to_owned();
            for master in masters {
                text.push_str(&format!(
                    "        sl.Add({});\n",
                    q(master.as_str().context("a master must be a string")?)
                ));
            }
            let sort = params["sort"].as_bool().unwrap_or(true);
            text.push_str(&format!(
                "        AddMastersIfMissing(FileNamed({}), sl, {});\n      finally\n        sl.Free;\n      end;",
                file()?,
                pascal_bool(sort)
            ));
            text
        }
        "masters.sort" => {
            check_params(params, &["file"])?;
            format!("SortMasters(FileNamed({}));", file()?)
        }
        "masters.clean" => {
            check_params(params, &["file"])?;
            format!("CleanMasters(FileNamed({}));", file()?)
        }
        "formids.change" => {
            check_params(params, &["form_id", "file", "new_form_id"])?;
            format!(
                "ChangeFormID(RecordIn({}, {}), StrToInt({}), False);",
                file()?,
                form_id()?,
                q(&format!("${}", text_param(params, "new_form_id")?))
            )
        }
        "formids.renumber" => {
            check_params(params, &["file", "start"])?;
            format!(
                "Renumber({}, StrToInt({}));",
                file()?,
                q(&format!("${}", text_param(params, "start")?))
            )
        }
        "files.flags" => {
            check_params(params, &["file", "esm", "medium", "light"])?;
            let mut text = format!("f := FileNamed({});", file()?);
            // The order of `files.flags`: esm, medium, light.
            for (flag, setter) in [("esm", "SetIsESM"), ("medium", "SetIsMedium"), ("light", "SetIsLight")] {
                if let Some(value) = params[flag].as_bool() {
                    text.push_str(&format!("\n      {setter}(f, {});", pascal_bool(value)));
                }
            }
            text
        }
        _ => bail!("step {index}: {name} is not translated for the oracle"),
    };
    code.push_str("      ");
    code.push_str(&body);
    code.push('\n');
    Ok(code)
}

/// The script of an edit sequence: the steps, then a write of every plugin
/// to compare to `out\<name>`.
fn edit_script(sequence: &EditSequence) -> Result<String> {
    const TEMPLATE: &str = include_str!("../../oracle/edit.pas");
    let mut steps = String::new();
    for (index, command) in sequence.commands.iter().enumerate() {
        let name = command["command"].as_str().unwrap_or("");
        ensure!(
            sequence.build_refs || !name.starts_with("formids."),
            "step {index}: {name} needs the reference information (\"build_refs\": true)"
        );
        steps.push_str(&edit_step(index, command)?);
    }
    let mut saves = String::new();
    for name in &sequence.save {
        let literal = pascal_string(name);
        saves.push_str(&format!("      log.Add({});\n", pascal_string(&format!("save {name}"))));
        saves.push_str(&format!(
            "      fs := TFileStream.Create('{{{{WORK}}}}out\\' + {literal}, fmCreate);\n"
        ));
        saves.push_str(&format!(
            "      try\n        FileWriteToStream(FileNamed({literal}), fs, 0);\n      finally\n        fs.Free;\n      end;\n"
        ));
    }
    Ok(TEMPLATE
        .replace("\r\n", "\n")
        .replace("{{STEPS}}\n", &steps)
        .replace("{{SAVES}}\n", &saves))
}

/// `parity oracle-edit`: every sequence of `crates/xtask/oracle/edits`, or
/// the ones `--file` names (by file stem).
pub(super) fn run_edits(
    root: &Path,
    tag: &str,
    options: &Options,
    cache: PathBuf,
    scratch: PathBuf,
    oracle_dir: PathBuf,
) -> Result<()> {
    let edits = root.join("crates/xtask/oracle/edits");
    let mut sequences = Vec::new();
    for entry in fs::read_dir(&edits).with_context(|| format!("reading {}", edits.display()))? {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if !options.files.is_empty() && !options.files.contains(&stem.to_lowercase()) {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let sequence: EditSequence =
            serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        let game = super::GAMES
            .iter()
            .find(|game| game.name == sequence.game)
            .with_context(|| format!("{}: unknown game {}", path.display(), sequence.game))?;
        if !options.all_games && !options.games.iter().any(|g| g.name == game.name) {
            continue;
        }
        sequences.push((stem, text, sequence, game));
    }
    sequences.sort_by(|a, b| a.0.cmp(&b.0));
    ensure!(
        !sequences.is_empty(),
        "no edit sequence selected in {}",
        edits.display()
    );
    let port = if options.oracle_only {
        None
    } else {
        Some(super::build_port(root)?)
    };
    let budget = options
        .memory_budget
        .or_else(|| crate::memory::physical_memory().map(|total| total / 4 * 3))
        .unwrap_or(24 * GIB);
    let runner = Runner {
        oracle: PathBuf::new(),
        port,
        cache,
        scratch,
        oracle_dir,
        hashes: std::sync::Mutex::new(std::collections::HashMap::new()),
        budget: crate::memory::Budget::new(budget),
        max_memory: options.max_memory.unwrap_or(budget),
        oracle_timeout: options.oracle_timeout,
    };
    let mut outcomes = Vec::new();
    for (stem, text, sequence, game) in &sequences {
        match check_edit(&runner, stem, text, sequence, game) {
            Ok(mut list) => outcomes.append(&mut list),
            Err(error) => outcomes.push(Outcome {
                game: game.name,
                file: stem.clone(),
                status: "oracle-failed",
                detail: Some(format!("  {error:#}")),
                oracle_bytes: 0,
                port_bytes: 0,
                oracle_peak: None,
                port_peak: None,
            }),
        }
    }
    for outcome in &outcomes {
        println!("{:13} {} {}", outcome.status, outcome.game, outcome.file);
        if let Some(detail) = &outcome.detail {
            println!("{detail}");
        }
    }
    let equal = outcomes.iter().filter(|o| o.status == "equal").count();
    let report = super::Report {
        tag,
        equal,
        total: outcomes.len(),
        outcomes: &outcomes,
    };
    let report_dir = root.join("target/parity");
    fs::create_dir_all(&report_dir)?;
    let report_file = report_dir.join("oracle-edit.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    println!(
        "{equal} of {} saved files equal. Report: {}",
        outcomes.len(),
        report_file.display()
    );
    if !options.oracle_only {
        ensure!(equal == outcomes.len(), "parity does not hold");
    }
    Ok(())
}

/// One edit sequence: the oracle's saves (cached), the port's batch, and an
/// outcome per saved plugin, named `<sequence>/<plugin>`.
fn check_edit(
    runner: &Runner,
    stem: &str,
    text: &str,
    sequence: &EditSequence,
    game: &'static Game,
) -> Result<Vec<Outcome>> {
    let data = PathBuf::from(super::required_var(game.data_var)?);
    let names: Vec<&str> = sequence.load.iter().map(String::as_str).collect();
    let plugins = load_list(game, &data, &names)?;
    let script = edit_script(sequence)?;
    let key = oracle_key(runner, &plugins, &format!("{text}{script}"), gui::exe_name(game.mode))?;
    let dir = runner.cache.join(format!("{}-oracle-edit", game.mode));
    fs::create_dir_all(&dir)?;
    let oracle_stem = format!("{stem}.{key:016x}");
    let status_file = dir.join(format!("{oracle_stem}.oracle.log"));
    let mut oracle_peak = None;
    if !status_file.exists() {
        let work = runner
            .scratch
            .join("oracle-work")
            .join(format!("{oracle_stem}.{}", std::process::id()));
        let peak_file = dir.join(format!("{oracle_stem}.oracle.peak"));
        let result = run_gui(
            runner,
            game,
            plugins,
            script,
            sequence.build_refs,
            work.clone(),
            &peak_file,
        )?;
        oracle_peak = result.peak;
        if result.status.last().is_some_and(|last| last == "done") {
            for name in &sequence.save {
                keep_compressed(
                    &result.out.join(name),
                    &dir.join(format!("{oracle_stem}.{name}.saved.zst")),
                )?;
            }
        }
        fs::write(&status_file, result.status.join("\n"))?;
        gui::remove_work(&work);
    }
    let status: Vec<String> = fs::read_to_string(&status_file)?.lines().map(str::to_owned).collect();
    let mut outcomes: Vec<Outcome> = sequence
        .save
        .iter()
        .map(|name| Outcome {
            game: game.name,
            file: format!("{stem}/{name}"),
            status: "oracle-only",
            detail: None,
            oracle_bytes: 0,
            port_bytes: 0,
            oracle_peak,
            port_peak: None,
        })
        .collect();
    if status.last().is_none_or(|last| last != "done") {
        for outcome in &mut outcomes {
            outcome.status = "oracle-failed";
            outcome.detail = Some(format!("  the oracle script stopped: {}", status.join(" / ")));
        }
        return Ok(outcomes);
    }
    let Some(port) = &runner.port else {
        return Ok(outcomes);
    };
    let port_dir = runner.scratch.join(format!("{}-oracle-edit", game.mode));
    fs::create_dir_all(&port_dir)?;
    let mut batch = sequence.commands.clone();
    let saved: Vec<PathBuf> = sequence
        .save
        .iter()
        .map(|name| port_dir.join(format!("{stem}.{name}.port.saved")))
        .collect();
    for (name, path) in sequence.save.iter().zip(&saved) {
        let _ = fs::remove_file(path);
        batch.push(serde_json::json!({
            "command": "files.save",
            "params": {"file": name, "output": path, "backup": false},
        }));
    }
    let batch_file = port_dir.join(format!("{stem}.batch.json"));
    fs::write(&batch_file, serde_json::to_string_pretty(&batch)?)?;
    let port_log = port_dir.join(format!("{stem}.port.log"));
    let mut command = std::process::Command::new(port);
    command.args(["--json", "--edit", "--game", game.mode]);
    for plugin in &sequence.load {
        command.arg("--load").arg(find_in(&data, plugin)?);
    }
    let output = command
        .arg("batch")
        .arg(&batch_file)
        .stdin(std::process::Stdio::null())
        .stderr(File::create(&port_log)?)
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(text.trim()).unwrap_or(Value::Null);
    let steps = envelope["result"].as_array().cloned().unwrap_or_default();
    let failed = steps.iter().position(|step| step["ok"] != Value::Bool(true));
    if envelope["ok"] != Value::Bool(true) || failed.is_some() || steps.len() != batch.len() {
        let message = match failed {
            Some(index) => format!(
                "step {index} ({}) failed: {}",
                steps[index]["command"], steps[index]["error"]
            ),
            None => format!("{}, see {}", text.trim(), port_log.display()),
        };
        for outcome in &mut outcomes {
            outcome.status = "port-failed";
            outcome.detail = Some(format!("  {message}"));
        }
        return Ok(outcomes);
    }
    for ((outcome, name), path) in outcomes.iter_mut().zip(&sequence.save).zip(&saved) {
        let oracle = dir.join(format!("{oracle_stem}.{name}.saved.zst"));
        outcome.oracle_bytes = fs::metadata(&oracle)?.len();
        outcome.port_bytes = fs::metadata(path)?.len();
        let case = Case {
            game,
            input: find_in(&data, name)?,
            name: name.clone(),
            data: data.clone(),
            saves: false,
        };
        let (status, detail) = compare(
            &case,
            &OracleSave::Saved(oracle),
            path,
            &port_dir,
            &format!("{stem}.{name}"),
        )?;
        outcome.status = status;
        outcome.detail = Some(detail);
        if status == "equal" {
            fs::remove_file(path)?;
        }
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut bytes = signature.to_vec();
        bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    #[test]
    fn masters_are_read_from_the_file_header() {
        let mut data = sub_record(b"HEDR", &[0; 12]);
        data.extend(sub_record(b"MAST", b"Fallout4.esm\0"));
        data.extend(sub_record(b"DATA", &[0; 8]));
        data.extend(sub_record(b"MAST", b"DLCRobot.esm\0"));
        data.extend(sub_record(b"DATA", &[0; 8]));
        let mut bytes = b"TES4".to_vec();
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend(data);
        let dir = std::env::temp_dir().join(format!("xtask-masters-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Test.esp");
        fs::write(&path, bytes).unwrap();
        let masters = header_masters(&path, 24).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(masters, ["Fallout4.esm", "DLCRobot.esm"]);
    }

    #[test]
    fn line_endings_do_not_change_the_key() {
        let lf = "begin\n  Save('{{FILE}}');\nend.\n";
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(fill_save_script(lf, "A.esm"), fill_save_script(&crlf, "A.esm"));
        assert_eq!(text_hash(lf), text_hash(&crlf));
        assert_ne!(text_hash(lf), text_hash("begin\nend.\n"));
    }

    #[test]
    fn steps_become_script_calls() {
        let step = |command: Value| edit_step(0, &command).unwrap();
        let set = step(serde_json::json!({"command": "elements.set", "params":
            {"form_id": "000E3778", "file": "It's.esm", "path": "DATA\\Value", "value": "12"}}));
        assert!(set.contains(r"SetElementEditValues(RecordIn('It''s.esm', '000E3778'), 'DATA\Value', '12');"));
        assert!(set.contains(r"PathElement(RecordIn('It''s.esm', '000E3778'), 'DATA\Value');"));
        let native = step(serde_json::json!({"command": "elements.set", "params":
            {"form_id": "00000800", "file": "A.esm", "path": "FLTV", "value": 1.5}}));
        assert!(native.contains("SetElementNativeValues(RecordIn('A.esm', '00000800'), 'FLTV', 1.5);"));
        let copy = step(serde_json::json!({"command": "records.copy", "params":
            {"form_id": "00000800", "from": "A.esm", "to": "B.esp", "as_new": true}}));
        assert!(copy.contains("AddRequiredElementMasters(r, f, True, True);"));
        assert!(copy.contains("wbCopyElementToFileWithPrefixAndSuffix(r, f, True, True, '', '', '', '')"));
        let renumber =
            step(serde_json::json!({"command": "formids.renumber", "params": {"file": "B.esp", "start": "000A00"}}));
        assert!(renumber.contains("Renumber('B.esp', StrToInt('$000A00'));"));
    }

    #[test]
    fn untranslated_steps_and_parameters_are_refused() {
        let unknown = serde_json::json!({"command": "files.save", "params": {}});
        assert!(edit_step(0, &unknown).is_err());
        let parameter = serde_json::json!({"command": "formids.change", "params":
            {"form_id": "01000800", "file": "B.esp", "new_form_id": "01000900", "overrides": true}});
        assert!(edit_step(0, &parameter).is_err());
    }
}
