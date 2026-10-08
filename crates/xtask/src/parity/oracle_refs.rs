// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The reference index against the oracle.
//!
//! `cargo xtask parity refs` loads the corpus plugins of each game into the
//! GUI build of xEdit in its script mode, which builds the reference
//! information of every loaded file as the GUI does on load, and has
//! `oracle/refs.pas` write the referenced-by list (`ReferencedByCount`,
//! `ReferencedByIndex`) of every master record. The port loads the same
//! plugins in the oracle's load order and writes the same lists with
//! `xedit refs dump`; the two are compared record by record and entry by
//! entry, with the files named instead of numbered. The oracle also writes
//! its reference cache files (`-C:`), which are kept and compared with the
//! port's cache files after decompression; the port then runs once more
//! with its cache files in place, so the load path of the cache gives the
//! same lists.
//!
//! The oracle's output is cached as
//! `<cache>/<tag>/<MODE>-oracle-refs/refs.<key>.txt.zst`, with the status
//! lines as `.status` and its cache files in the folder `.refcache`; the
//! key hashes the plugins and the script. The plugins are given to the port
//! as links (copies on another volume) in a folder of their own, so it sees
//! what the oracle sees: the plugins without their archives and strings.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail, ensure};

use super::{GIB, Game, Options, Outcome, PLUGIN_EXTENSIONS, Runner, is_vanilla};
use crate::memory::Limit;

/// The script of the referenced-by lists.
const REFS_SCRIPT: &str = include_str!("../../oracle/refs.pas");

/// The referenced-by lists of one side: per master record (its load order
/// FormID and file) the entries (load order FormID and file), with the
/// files as indices into `names`.
#[derive(Default)]
struct Index {
    names: Vec<String>,
    lists: HashMap<(u32, u16), Vec<(u32, u16)>>,
    /// The masters in the order of the output.
    order: Vec<(u32, u16)>,
    entries: usize,
}

/// The interned lower-case file names shared by both sides.
#[derive(Default)]
struct Names(HashMap<String, u16>, Vec<String>);

impl Names {
    fn id(&mut self, name: &str) -> u16 {
        let key = name.to_lowercase();
        if let Some(id) = self.0.get(&key) {
            return *id;
        }
        let id = self.1.len() as u16;
        self.0.insert(key, id);
        self.1.push(name.to_owned());
        id
    }

    fn name(&self, id: u16) -> &str {
        &self.1[id as usize]
    }
}

/// Reads the output of `refs.pas` or `xedit refs dump`.
fn read_index(reader: impl BufRead, names: &mut Names) -> Result<Index> {
    let mut index = Index::default();
    let mut current: Option<(u32, u16)> = None;
    // `<FormID>@<file>`.
    let entry = |text: &str, names: &mut Names| -> Result<(u32, u16)> {
        let (form_id, file) = text
            .split_once('@')
            .with_context(|| format!("not FormID@file: {text}"))?;
        let form_id = &form_id[form_id.len().saturating_sub(8)..];
        let form_id = u32::from_str_radix(form_id, 16).with_context(|| format!("not a FormID: {form_id}"))?;
        Ok((form_id, names.id(file)))
    };
    for line in reader.lines() {
        let line = line?;
        let line = line.trim_end_matches('\r');
        if let Some(name) = line.strip_prefix("F ") {
            names.id(name);
            index.names.push(name.to_owned());
        } else if let Some(rest) = line.strip_prefix("R ") {
            let (record, _count) = rest.rsplit_once(' ').context("a record line without a count")?;
            let key = entry(record, names)?;
            index.order.push(key);
            index.lists.entry(key).or_default();
            current = Some(key);
        } else if line.starts_with('|') {
            let key = current.context("entries before a record line")?;
            for text in line.split('|').skip(1) {
                let value = entry(text, names)?;
                index
                    .lists
                    .get_mut(&key)
                    .expect("added with the record line")
                    .push(value);
                index.entries += 1;
            }
        } else if !line.is_empty() {
            bail!("unexpected line: {line}");
        }
    }
    Ok(index)
}

/// The comparison of two indices, as a status and a detail.
fn compare(oracle: &Index, port: &Index, names: &Names) -> (bool, String) {
    let entry = |(form_id, file): (u32, u16)| format!("{form_id:08X}:{}", names.name(file));
    let mut only_oracle = 0usize;
    let mut different = 0usize;
    let mut examples = Vec::new();
    for key in &oracle.order {
        let expected = &oracle.lists[key];
        match port.lists.get(key) {
            None => {
                only_oracle += 1;
                if examples.len() < 10 {
                    examples.push(format!(
                        "  {} referenced by {} records in the oracle, none in the port",
                        entry(*key),
                        expected.len()
                    ));
                }
            }
            Some(actual) if actual != expected => {
                different += 1;
                if examples.len() < 10 {
                    let position = expected
                        .iter()
                        .zip(actual)
                        .position(|(a, b)| a != b)
                        .unwrap_or(expected.len().min(actual.len()));
                    examples.push(format!(
                        "  {}: oracle {} entries, port {}; first difference at entry {position}: oracle {}, port {}",
                        entry(*key),
                        expected.len(),
                        actual.len(),
                        expected.get(position).map_or("-".to_owned(), |e| entry(*e)),
                        actual.get(position).map_or("-".to_owned(), |e| entry(*e)),
                    ));
                }
            }
            Some(_) => {}
        }
    }
    let only_port: Vec<_> = port
        .order
        .iter()
        .filter(|key| !oracle.lists.contains_key(key))
        .collect();
    for key in only_port.iter().take(10usize.saturating_sub(examples.len())) {
        examples.push(format!(
            "  {} referenced by {} records in the port, none in the oracle",
            entry(**key),
            port.lists[key].len()
        ));
    }
    let equal = only_oracle == 0 && different == 0 && only_port.is_empty();
    let mut detail = format!(
        "  {} files; oracle {} referenced records with {} entries, port {} with {}; {} records differ, {} only in the oracle, {} only in the port",
        oracle.names.len(),
        oracle.order.len(),
        oracle.entries,
        port.order.len(),
        port.entries,
        different,
        only_oracle,
        only_port.len()
    );
    for example in examples {
        detail.push('\n');
        detail.push_str(&example);
    }
    (equal, detail)
}

/// The cache file name without the program's CRC32, which differs between
/// the oracle and the port.
fn cache_key(name: &str) -> Option<&str> {
    name.split_once('_').map(|(_, rest)| rest)
}

/// Compares the oracle's cache files with the port's, after decompression.
fn compare_caches(oracle_dir: &Path, port_dir: &Path) -> Result<(usize, usize, Vec<String>)> {
    let mut port_files: HashMap<String, PathBuf> = HashMap::new();
    if port_dir.exists() {
        for entry in fs::read_dir(port_dir)? {
            let path = entry?.path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if let Some(key) = cache_key(&name) {
                port_files.insert(key.to_lowercase(), path);
            }
        }
    }
    let (mut equal, mut total, mut notes) = (0, 0, Vec::new());
    let mut oracle_files: Vec<PathBuf> = if oracle_dir.exists() {
        fs::read_dir(oracle_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<_>>()?
    } else {
        Vec::new()
    };
    oracle_files.sort();
    for path in oracle_files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some(key) = cache_key(&name) else { continue };
        total += 1;
        let Some(port) = port_files.remove(&key.to_lowercase()) else {
            notes.push(format!("  cache {key}: the port wrote none"));
            continue;
        };
        let oracle_data = decompress_lz4f(&fs::read(&path)?)?;
        let port_data = decompress_lz4f(&fs::read(&port)?)?;
        if oracle_data == port_data {
            equal += 1;
        } else {
            let offset = oracle_data
                .iter()
                .zip(&port_data)
                .position(|(a, b)| a != b)
                .unwrap_or(oracle_data.len().min(port_data.len()));
            notes.push(format!(
                "  cache {key}: oracle {} bytes, port {}, first difference at byte {offset} (0x{offset:X}){}",
                oracle_data.len(),
                port_data.len(),
                describe_cache_difference(&oracle_data, &port_data)
            ));
        }
    }
    for key in port_files.keys() {
        notes.push(format!("  cache {key}: only the port wrote one"));
    }
    Ok((equal, total, notes))
}

/// A field of a cache record: its name and its byte range.
type CacheField = (&'static str, usize, usize);

/// The fields of one record of a cache stream (`SaveRefsToStream`), and
/// where the record ends.
fn cache_record_fields(data: &[u8], mut at: usize, names: bool) -> Option<(Vec<CacheField>, usize)> {
    let u32_at = |at: usize| -> Option<u32> { Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?)) };
    let mut fields = Vec::new();
    let mut field = |name: &'static str, start: usize, end: usize| {
        fields.push((name, start, end));
        end
    };
    at = field("FormID", at, at + 4);
    let count = u32_at(at)? as usize;
    at = field("references", at, at + 4 + 4 * count);
    let editor_id = u32_at(at)? as usize;
    at = field("editor ID", at, at + 4 + 2 * editor_id);
    let full_name = u32_at(at)? as usize;
    at = field("full name", at, at + 4 + 2 * full_name);
    at = field("base record", at, at + 4);
    let checked = *data.get(at)? != 0;
    let has = checked && *data.get(at + 1)? != 0;
    let end = at + 1 + usize::from(checked) + if has { 8 } else { 0 };
    at = field("grid cell", at, end);
    if names {
        for name in ["name", "short name"] {
            let length = u32_at(at)? as usize;
            at = field(name, at, at + 4 + 2 * length);
        }
    }
    (at <= data.len()).then_some((fields, at))
}

/// The first record and field where two cache streams differ.
fn describe_cache_difference(oracle: &[u8], port: &[u8]) -> String {
    // A stream with the names of the game master, or without.
    for names in [false, true] {
        let (mut a, mut b) = (4, 4);
        let mut index = 0;
        while let (Some((fields_a, end_a)), Some((fields_b, end_b))) = (
            cache_record_fields(oracle, a, names),
            cache_record_fields(port, b, names),
        ) {
            for ((name, start_a, stop_a), (_, start_b, stop_b)) in fields_a.iter().zip(&fields_b) {
                if oracle[*start_a..*stop_a] != port[*start_b..*stop_b] {
                    let form_id = u32::from_le_bytes(oracle[fields_a[0].1..fields_a[0].1 + 4].try_into().unwrap());
                    return format!("; record {index} [{form_id:08X}], field {name}");
                }
            }
            if end_a >= oracle.len() || end_b >= port.len() {
                break;
            }
            (a, b, index) = (end_a, end_b, index + 1);
        }
    }
    String::new()
}

/// The stream of an LZ4 frame.
fn decompress_lz4f(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    lz4_flex::frame::FrameDecoder::new(data).read_to_end(&mut out)?;
    Ok(out)
}

/// Links (or copies) the plugins into `folder` and returns the new paths.
fn link_plugins(plugins: &[PathBuf], folder: &Path) -> Result<Vec<PathBuf>> {
    if folder.exists() {
        fs::remove_dir_all(folder).with_context(|| format!("removing {}", folder.display()))?;
    }
    fs::create_dir_all(folder)?;
    let mut linked = Vec::new();
    for plugin in plugins {
        let target = folder.join(plugin.file_name().context("plugin without a name")?);
        if fs::hard_link(plugin, &target).is_err() {
            fs::copy(plugin, &target).with_context(|| format!("copying {}", plugin.display()))?;
        }
        linked.push(target);
    }
    Ok(linked)
}

/// Runs `xedit refs dump` on `plugins` (in load order) into `out`.
fn run_port(runner: &Runner, game: &Game, plugins: &[PathBuf], cache: &Path, out: &Path, log: &Path) -> Result<()> {
    let port = runner.port.as_ref().context("no port binary")?;
    let mut command = Command::new(port);
    command.arg("--game").arg(game.name).arg("--cache-path").arg(cache);
    for plugin in plugins {
        command.arg("--load").arg(plugin);
    }
    command
        .args(["refs", "dump"])
        .stdin(Stdio::null())
        .stdout(File::create(out)?)
        .stderr(File::create(log)?);
    let _reservation = runner.budget.reserve((8 * GIB).min(runner.max_memory));
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {}", port.display()))?;
    let limit = match Limit::apply(&child, runner.max_memory) {
        Ok(limit) => limit,
        Err(error) => {
            let _ = child.kill();
            return Err(error);
        }
    };
    let status = child.wait()?;
    ensure!(!limit.reached(), "the port reached the memory cap");
    if !status.success() {
        let text = fs::read_to_string(log).unwrap_or_default();
        let tail: Vec<&str> = text.lines().rev().take(5).collect();
        bail!(
            "the port failed ({status}): {}",
            tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
        );
    }
    Ok(())
}

/// The oracle's output for `plugins`, from the cache or a GUI run: the
/// lists, the status lines and the folder of its cache files.
fn oracle_output(runner: &Runner, game: &Game, plugins: &[PathBuf]) -> Result<(PathBuf, Vec<String>, PathBuf)> {
    let dir = runner.cache.join(format!("{}-oracle-refs", game.mode));
    fs::create_dir_all(&dir)?;
    let script = REFS_SCRIPT.replace("\r\n", "\n");
    let key = super::oracle_save::oracle_key(runner, plugins, &script, super::gui::exe_name(game.mode))?;
    let stem = format!("refs.{key:016x}");
    let refs = dir.join(format!("{stem}.txt.zst"));
    let status_file = dir.join(format!("{stem}.status"));
    let caches = dir.join(format!("{stem}.refcache"));
    if refs.exists() && status_file.exists() {
        let status = fs::read_to_string(&status_file)?.lines().map(str::to_owned).collect();
        return Ok((refs, status, caches));
    }
    let work = runner
        .scratch
        .join("oracle-work")
        .join(format!("{}-refs-{}", game.mode, std::process::id()));
    let result = super::oracle_save::run_gui(
        runner,
        game,
        plugins.to_vec(),
        script,
        true,
        work.clone(),
        &dir.join(format!("{stem}.peak")),
    )?;
    let kept = (|| -> Result<()> {
        let last = result.status.last().map(String::as_str).unwrap_or("");
        ensure!(last == "done", "the script did not finish: {last}");
        let parts: usize = result
            .status
            .iter()
            .find_map(|line| line.strip_prefix("parts: "))
            .context("the script wrote no part count")?
            .parse()?;
        let partial = refs.with_extension(format!("zst.{}.partial", std::process::id()));
        let mut encoder = zstd::Encoder::new(File::create(&partial)?, 3)?;
        for part in 0..parts {
            let path = result.out.join(format!("refs{part}.txt"));
            // `TStringList.SaveToFile` writes ANSI text with CRLF.
            std::io::copy(&mut File::open(&path)?, &mut encoder)?;
        }
        encoder.finish()?;
        if caches.exists() {
            fs::remove_dir_all(&caches)?;
        }
        fs::create_dir_all(&caches)?;
        let written = work.join("cache");
        if written.exists() {
            for entry in fs::read_dir(&written)? {
                let path = entry?.path();
                fs::copy(&path, caches.join(path.file_name().unwrap()))?;
            }
        }
        fs::write(&status_file, result.status.join("\n"))?;
        fs::rename(&partial, &refs)?;
        Ok(())
    })();
    super::gui::remove_work(&work);
    kept?;
    Ok((refs, result.status, caches))
}

/// `parity refs` for one game.
fn check_game(runner: &Runner, game: &'static Game, data: &Path) -> Result<Outcome> {
    let mut names = Vec::new();
    for entry in fs::read_dir(data).with_context(|| format!("reading {}", data.display()))? {
        let path = entry?.path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let is_plugin = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| PLUGIN_EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)));
        if is_plugin && is_vanilla(game, &name.to_lowercase()) {
            names.push(name);
        }
    }
    names.sort();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let plugins = super::oracle_save::load_list(game, data, &name_refs)?;
    let label = format!("{} plugins", plugins.len());
    let outcome = |status: &'static str, detail: String| Outcome {
        game: game.name,
        file: label.clone(),
        status,
        detail: Some(detail),
        oracle_bytes: 0,
        port_bytes: 0,
        oracle_peak: None,
        port_peak: None,
    };
    let (refs, status, oracle_caches) = oracle_output(runner, game, &plugins)?;
    if runner.port.is_none() {
        return Ok(outcome("oracle-only", format!("  {}", refs.display())));
    }
    let mut interned = Names::default();
    let oracle = read_index(BufReader::new(zstd::Decoder::new(File::open(&refs)?)?), &mut interned)?;
    // The port loads the plugins in the oracle's load order; the hardcoded
    // file loads with the game master.
    let by_name: HashMap<String, &PathBuf> = plugins
        .iter()
        .map(|path| (path.file_name().unwrap().to_string_lossy().to_lowercase(), path))
        .collect();
    let ordered: Vec<PathBuf> = oracle
        .names
        .iter()
        .filter_map(|name| by_name.get(&name.to_lowercase()).map(|path| (*path).clone()))
        .collect();
    ensure!(
        ordered.len() == plugins.len(),
        "the oracle loaded {} of the {} plugins: {}",
        ordered.len(),
        plugins.len(),
        status.join(" | ")
    );
    let work = runner.scratch.join(format!("{}-refs", game.mode));
    let linked = link_plugins(&ordered, &work.join("data"))?;
    let cache = work.join("cache");
    if cache.exists() {
        fs::remove_dir_all(&cache)?;
    }
    let mut details = Vec::new();
    let mut all_equal = true;
    // The first run builds and saves the cache, the second loads it.
    for (run, label) in [(0, "built"), (1, "loaded from the cache")] {
        let out = work.join(format!("port{run}.txt"));
        let log = work.join(format!("port{run}.log"));
        run_port(runner, game, &linked, &cache, &out, &log)?;
        let port = read_index(BufReader::new(File::open(&out)?), &mut interned)?;
        let (equal, detail) = compare(&oracle, &port, &interned);
        all_equal &= equal;
        details.push(format!(
            "  references {label}: {}",
            if equal { "equal" } else { "different" }
        ));
        details.push(detail);
        if equal {
            let _ = fs::remove_file(&out);
        }
    }
    let (equal, total, notes) = compare_caches(&oracle_caches, &cache)?;
    details.push(format!("  cache files: {equal} of {total} equal after decompression"));
    details.extend(notes);
    let caches_equal = equal == total;
    let status = if all_equal && caches_equal {
        "equal"
    } else {
        "different"
    };
    Ok(outcome(status, details.join("\n")))
}

/// `cargo xtask parity refs`.
pub(super) fn run_refs(
    root: &Path,
    tag: &str,
    options: &Options,
    cache: PathBuf,
    scratch: PathBuf,
    oracle_dir: PathBuf,
) -> Result<()> {
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
        hashes: std::sync::Mutex::new(HashMap::new()),
        budget: crate::memory::Budget::new(budget),
        max_memory: options.max_memory.unwrap_or(budget),
        oracle_timeout: options.oracle_timeout,
    };
    let mut outcomes = Vec::new();
    for &game in &options.games {
        let data = match (std::env::var_os(game.data_var), options.all_games) {
            (Some(data), _) => PathBuf::from(data),
            (None, true) => {
                println!("skipped       {}: {} is not set", game.name, game.data_var);
                continue;
            }
            (None, false) => bail!("environment variable {} is not set", game.data_var),
        };
        if game.mode == "TES3" {
            // Upstream builds no references for Morrowind (`wbBuildRefs`).
            println!("skipped       {}: no reference information in Morrowind", game.name);
            continue;
        }
        let outcome = check_game(&runner, game, &data).unwrap_or_else(|error| Outcome {
            game: game.name,
            file: "corpus".to_owned(),
            status: "oracle-failed",
            detail: Some(format!("  {error:#}")),
            oracle_bytes: 0,
            port_bytes: 0,
            oracle_peak: None,
            port_peak: None,
        });
        println!("{:13} {} {}", outcome.status, outcome.game, outcome.file);
        if let Some(detail) = &outcome.detail {
            println!("{detail}");
        }
        outcomes.push(outcome);
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
    let report_file = report_dir.join("refs.json");
    fs::write(&report_file, serde_json::to_string_pretty(&report)?)?;
    println!("{equal} of {} equal. Report: {}", outcomes.len(), report_file.display());
    if !options.oracle_only {
        ensure!(equal == outcomes.len(), "parity does not hold");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_formats() {
        let text = "F Skyrim.esm\r\nF Update.esm\r\nR 00000007@Skyrim.esm 3\r\n|00000014@Skyrim.esm|0100A1B2@Update.esm\r\n|0100A1B2@Update.esm\r\n";
        let mut names = Names::default();
        let index = read_index(text.as_bytes(), &mut names).unwrap();
        assert_eq!(index.entries, 3);
        let (skyrim, update) = (names.id("skyrim.esm"), names.id("update.esm"));
        assert_eq!(
            index.lists[&(7, skyrim)],
            vec![(0x14, skyrim), (0x0100_A1B2, update), (0x0100_A1B2, update)]
        );
        let (equal, _) = compare(&index, &index, &names);
        assert!(equal);
        // File names compare without case; the line endings do not matter.
        let other = "F Skyrim.esm\nF UPDATE.ESM\nR 00000007@SKYRIM.ESM 3\n|00000014@Skyrim.esm|0100A1B2@Update.esm|0100A1B2@Update.esm\n";
        let other = read_index(other.as_bytes(), &mut names).unwrap();
        assert!(compare(&index, &other, &names).0);
        let short = "F Skyrim.esm\nR 00000007@Skyrim.esm 1\n|00000014@Skyrim.esm\n";
        let short = read_index(short.as_bytes(), &mut names).unwrap();
        let (equal, detail) = compare(&index, &short, &names);
        assert!(!equal);
        assert!(detail.contains("first difference at entry 1"), "{detail}");
    }

    #[test]
    fn cache_names_without_the_program() {
        assert_eq!(
            cache_key("1A2B3C4D_Skyrim_esm_DEADBEEF_g1252_t1252_l1252_English.refcache"),
            Some("Skyrim_esm_DEADBEEF_g1252_t1252_l1252_English.refcache")
        );
    }
}
