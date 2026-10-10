// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Repository maintenance tasks.
//!
//! `cargo xtask check` validates license headers, `upstream-map.toml` and
//! `coverage/ledger.toml`.
//!
//! `cargo xtask sync <upstream checkout> [commit]` re-scans an upstream xEdit
//! checkout. It adds new units and new GUI actions, switches, modes and script
//! functions as `pending`, keeps every existing entry, and lists entries that
//! upstream no longer has.
//!
//! `cargo xtask parity dump` and `parity saves` compare the port with the oracle. See `parity`.
//!
//! `cargo xtask pascal-check <file>...` parses upstream Pascal units with the
//! reader of the definition transpiler and reports the files that fail.

mod memory;
mod parity;
mod pascal;
mod portdefs;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

const MAP_FILE: &str = "upstream-map.toml";
const LEDGER_FILE: &str = "coverage/ledger.toml";
const HEADER: &str = "Mozilla Public";

/// Upstream directories whose Pascal units are tracked in the map.
const UNIT_DIRS: &[&str] = &["Core", "xEdit", "xDump", "BSArch", "Sniff", "Tools"];

const UNIT_STATUSES: &[&str] = &["pending", "ported", "replaced", "not-ported"];
const LEDGER_STATUSES: &[&str] = &["pending", "covered", "presentation", "excluded"];

#[derive(Serialize, Deserialize, Default)]
struct UpstreamMap {
    upstream: Upstream,
    /// Upstream unit path to its port state.
    units: BTreeMap<String, Unit>,
}

#[derive(Serialize, Deserialize, Default)]
struct Upstream {
    repository: String,
    /// Release tag the port is level with. Its published binaries are the parity oracle.
    tag: String,
    /// Commit of `tag`.
    commit: String,
}

#[derive(Serialize, Deserialize)]
struct Unit {
    /// One of `UNIT_STATUSES`.
    status: String,
    /// Rust file or directory that holds the port. Required for `ported`.
    #[serde(skip_serializing_if = "Option::is_none")]
    rust: Option<String>,
    /// Why the unit is `replaced` or `not-ported`, or which upstream change is still to merge.
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

/// Everything a user or script can reach in upstream xEdit, by kind.
#[derive(Serialize, Deserialize, Default)]
struct Ledger {
    /// Event bindings in form files, keyed `file:component:event`.
    gui: BTreeMap<String, Entry>,
    /// Command-line switches, keyed by lower-case name.
    switch: BTreeMap<String, Entry>,
    game_mode: BTreeMap<String, Entry>,
    tool_mode: BTreeMap<String, Entry>,
    /// Script host registrations, keyed `unit:owner.name:kind`.
    script: BTreeMap<String, Entry>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    /// One of `LEDGER_STATUSES`.
    status: String,
    /// Session command that covers the entry. Required for `covered`.
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    /// Upstream handler or location, for orientation.
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

impl Entry {
    fn pending(source: Option<String>) -> Self {
        Self {
            status: "pending".to_owned(),
            command: None,
            source,
            note: None,
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    match args.as_slice() {
        ["check"] => {
            std::env::set_current_dir(&root)?;
            check()
        }
        ["sync", upstream, rest @ ..] if rest.len() <= 1 => {
            let upstream = fs::canonicalize(upstream).with_context(|| format!("opening {upstream}"))?;
            std::env::set_current_dir(&root)?;
            sync(&upstream, rest.first().copied())
        }
        ["port-defs", rest @ ..] => portdefs::run(rest),
        ["pascal-check", files @ ..] if !files.is_empty() => {
            let files: Vec<String> = files.iter().map(|file| (*file).to_owned()).collect();
            pascal::parser::check_files(&files)
        }
        ["parity", rest @ ..] => {
            let root = fs::canonicalize(&root)?;
            std::env::set_current_dir(&root)?;
            let map: UpstreamMap = load(MAP_FILE)?;
            parity::run(&root, &map.upstream.tag, rest)
        }
        _ => bail!(
            "usage: cargo xtask check | cargo xtask sync <upstream checkout> [commit] | cargo xtask parity dump|saves [options]"
        ),
    }
}

fn load<T: Default + for<'de> Deserialize<'de>>(path: &str) -> Result<T> {
    if !Path::new(path).exists() {
        return Ok(T::default());
    }
    toml::from_str(&fs::read_to_string(path)?).with_context(|| format!("parsing {path}"))
}

fn save<T: Serialize>(path: &str, about: &str, value: &T) -> Result<()> {
    let body = toml::to_string(value)?;
    fs::write(
        path,
        format!("# {about}\n# Maintained with `cargo xtask sync`. Edit statuses by hand.\n\n{body}"),
    )?;
    Ok(())
}

fn check() -> Result<()> {
    let mut problems = Vec::new();

    for entry in WalkDir::new("crates") {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "rs") {
            let text = fs::read_to_string(entry.path())?;
            if !text.lines().take(3).any(|line| line.contains(HEADER)) {
                problems.push(format!("{}: missing MPL-2.0 header", entry.path().display()));
            }
        }
    }

    let map: UpstreamMap = load(MAP_FILE)?;
    if map.upstream.commit.is_empty() {
        problems.push(format!("{MAP_FILE}: upstream.commit is not set"));
    }
    for (path, unit) in &map.units {
        if !UNIT_STATUSES.contains(&unit.status.as_str()) {
            problems.push(format!("{MAP_FILE}: {path}: unknown status {}", unit.status));
        }
        match (&unit.rust, unit.status.as_str()) {
            (Some(rust), _) if !Path::new(rust).exists() => {
                problems.push(format!("{MAP_FILE}: {path}: {rust} does not exist"));
            }
            (None, "ported") => problems.push(format!("{MAP_FILE}: {path}: ported without a rust path")),
            (_, "replaced" | "not-ported") if unit.note.is_none() => {
                problems.push(format!("{MAP_FILE}: {path}: {} needs a note", unit.status));
            }
            _ => {}
        }
    }

    let ledger: Ledger = load(LEDGER_FILE)?;
    let kinds = [
        ("gui", &ledger.gui),
        ("switch", &ledger.switch),
        ("game_mode", &ledger.game_mode),
        ("tool_mode", &ledger.tool_mode),
        ("script", &ledger.script),
    ];
    for (kind, entries) in kinds {
        for (key, entry) in entries {
            if !LEDGER_STATUSES.contains(&entry.status.as_str()) {
                problems.push(format!("{LEDGER_FILE}: {kind} {key}: unknown status {}", entry.status));
            }
            if entry.status == "covered" && entry.command.is_none() {
                problems.push(format!("{LEDGER_FILE}: {kind} {key}: covered without a command"));
            }
            if entry.status == "excluded" && entry.note.is_none() {
                problems.push(format!("{LEDGER_FILE}: {kind} {key}: excluded needs a note"));
            }
        }
        let open = entries.values().filter(|e| e.status == "pending").count();
        println!("{kind}: {} entries, {open} pending", entries.len());
    }
    let open = map.units.values().filter(|u| u.status == "pending").count();
    println!("units: {} tracked, {open} pending", map.units.len());

    for problem in &problems {
        eprintln!("{problem}");
    }
    ensure!(problems.is_empty(), "{} problem(s) found", problems.len());
    Ok(())
}

fn sync(upstream: &Path, commit: Option<&str>) -> Result<()> {
    ensure!(
        upstream.join("xEdit.dpr").exists(),
        "{} is not an xEdit checkout",
        upstream.display()
    );

    let mut map: UpstreamMap = load(MAP_FILE)?;
    if let Some(commit) = commit {
        map.upstream.commit = commit.to_owned();
    }
    let sources = pascal_sources(upstream)?;
    let mut added = 0;
    for path in sources.keys() {
        if !map.units.contains_key(path) {
            map.units.insert(
                path.clone(),
                Unit {
                    status: "pending".to_owned(),
                    rust: None,
                    note: None,
                },
            );
            added += 1;
        }
    }
    for path in map.units.keys().filter(|path| !sources.contains_key(*path)) {
        println!("unit removed upstream: {path}");
    }
    println!("units: {added} added");
    save(MAP_FILE, "Upstream xEdit units and the state of their Rust port.", &map)?;

    let mut ledger: Ledger = load(LEDGER_FILE)?;
    let merge = |kind: &str, entries: &mut BTreeMap<String, Entry>, found: BTreeMap<String, Option<String>>| {
        for key in entries.keys().filter(|key| !found.contains_key(*key)) {
            println!("{kind} removed upstream: {key}");
        }
        let before = entries.len();
        for (key, source) in found {
            entries.entry(key).or_insert_with(|| Entry::pending(source));
        }
        println!("{kind}: {} added", entries.len() - before);
    };
    merge("gui", &mut ledger.gui, gui_bindings(upstream)?);
    merge("switch", &mut ledger.switch, switches(&sources));
    merge(
        "game_mode",
        &mut ledger.game_mode,
        enum_values(&sources, "TwbGameMode")?,
    );
    merge(
        "tool_mode",
        &mut ledger.tool_mode,
        enum_values(&sources, "TwbToolMode")?,
    );
    merge("script", &mut ledger.script, script_registrations(&sources));
    save(
        LEDGER_FILE,
        "Everything reachable in upstream xEdit and the command that covers it.",
        &ledger,
    )
}

/// Reads a file that may be UTF-8 or a legacy single-byte encoding.
fn read_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

fn relative(upstream: &Path, path: &Path) -> Result<String> {
    Ok(path.strip_prefix(upstream)?.to_string_lossy().replace('\\', "/"))
}

/// Tracked Pascal sources, keyed by path relative to the upstream root.
fn pascal_sources(upstream: &Path) -> Result<BTreeMap<String, String>> {
    let mut files: Vec<PathBuf> = fs::read_dir(upstream)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| has_extension(path, &["dpr"]))
        .collect();
    for dir in UNIT_DIRS {
        for entry in WalkDir::new(upstream.join(dir)) {
            let entry = entry?;
            if has_extension(entry.path(), &["pas", "dpr", "inc"]) {
                files.push(entry.into_path());
            }
        }
    }
    files
        .iter()
        .map(|path| Ok((relative(upstream, path)?, read_text(path)?)))
        .collect()
}

/// Event bindings (`OnClick = handler`) of every component in the form files.
fn gui_bindings(upstream: &Path) -> Result<BTreeMap<String, Option<String>>> {
    let object = Regex::new(r"^(?:object|inherited|inline)\s+(\w+)\s*:")?;
    let event = Regex::new(r"^(On\w+)\s*=\s*(\w+)$")?;
    let mut found = BTreeMap::new();
    for dir in UNIT_DIRS {
        for entry in WalkDir::new(upstream.join(dir)) {
            let entry = entry?;
            if !has_extension(entry.path(), &["dfm"]) {
                continue;
            }
            let file = relative(upstream, entry.path())?;
            // Component names of the enclosing objects; collection items push an empty name.
            let mut stack: Vec<String> = Vec::new();
            for line in read_text(entry.path())?.lines() {
                let line = line.trim();
                if let Some(captures) = object.captures(line) {
                    stack.push(captures[1].to_owned());
                } else if line == "item" {
                    stack.push(String::new());
                } else if line == "end" || line == "end>" {
                    stack.pop();
                } else if let Some(captures) = event.captures(line) {
                    let component = stack.iter().rev().find(|name| !name.is_empty());
                    if let Some(component) = component {
                        found.insert(
                            format!("{file}:{component}:{}", &captures[1]),
                            Some(captures[2].to_owned()),
                        );
                    }
                }
            }
        }
    }
    Ok(found)
}

/// Command-line switches read with a literal name.
fn switches(sources: &BTreeMap<String, String>) -> BTreeMap<String, Option<String>> {
    let switch = Regex::new(r"(?i)\b(?:FindCmdLineSwitch|wbFindCmdLineParam)\(\s*'([^']+)'").unwrap();
    let mut found = BTreeMap::new();
    for (path, text) in sources {
        for captures in switch.captures_iter(text) {
            found
                .entry(captures[1].to_lowercase())
                .or_insert_with(|| Some(path.clone()));
        }
    }
    found
}

/// Values of the Pascal enumeration `name`.
fn enum_values(sources: &BTreeMap<String, String>, name: &str) -> Result<BTreeMap<String, Option<String>>> {
    let declaration = Regex::new(&format!(r"\b{name}\s*=\s*\(([^)]*)\)"))?;
    for (path, text) in sources {
        if let Some(captures) = declaration.captures(text) {
            return Ok(captures[1]
                .split(',')
                .map(|value| (value.trim().to_owned(), Some(path.clone())))
                .collect());
        }
    }
    bail!("enumeration {name} not found upstream")
}

/// The registration kinds of `TJvInterpreterAdapter` the `xejvi*` adapter
/// units call. The longer names come first (`RecGet` before `Rec`), so a
/// kind is never cut short. `AddExtUnit` declares a script unit rather than
/// a member of one and takes a single argument, so it has a pattern of its
/// own.
const REGISTRATION_KINDS: &str = "Function|Const|Class|IDGet|IDSet|IGet|ISet|Get|Set|RecGet|Rec|Handler";

/// Functions, properties and methods registered with the script interpreter.
fn script_registrations(sources: &BTreeMap<String, String>) -> BTreeMap<String, Option<String>> {
    let registration = Regex::new(&format!(
        r"\bAdd({REGISTRATION_KINDS})\(\s*'?([\w.]+)'?\s*,\s*'?([\w.]+)'?(?:\s*,\s*'?([\w.]+)'?)?"
    ))
    .unwrap();
    let ext_unit = Regex::new(r"\bAddExtUnit\(\s*'?([\w.]+)'?\s*\)").unwrap();
    let mut found = BTreeMap::new();
    for (path, text) in sources.iter().filter(|(path, _)| path.starts_with("xEdit/JvI/")) {
        let unit = Path::new(path).file_stem().unwrap().to_string_lossy();
        let text = without_comments(text);
        for captures in registration.captures_iter(&text) {
            let kind = captures[1].to_lowercase();
            let key = if kind == "recget" {
                // `AddRecGet(UnitName, RecordType, Identifier, Proc, ...)`:
                // the record type owns the method, so the methods of one
                // record type stay apart.
                format!("{unit}:{}.{}:{kind}", &captures[3], &captures[4])
            } else {
                format!("{unit}:{}.{}:{kind}", &captures[2], &captures[3])
            };
            // The source is the handler routine where the call has one:
            // the fourth argument of the kinds registered as
            // `(Class, 'Name', Proc, ...)`, the third of `AddIDGet` and
            // `AddIDSet`, which name no identifier and are
            // `(Class, Proc, ParamCount, ...)`. The other kinds (a
            // constant, a class, a record, a record method, `AddExtUnit`)
            // register a declaration, and the unit file is the source.
            let source = match kind.as_str() {
                "function" | "get" | "set" | "iget" | "iset" | "handler" => captures.get(4),
                "idget" | "idset" => captures.get(3),
                _ => None,
            }
            .map(|handler| handler.as_str().to_owned())
            .or_else(|| Some(path.clone()));
            found.insert(key, source);
        }
        for captures in ext_unit.captures_iter(&text) {
            found.insert(format!("{unit}:{}:extunit", &captures[1]), Some(path.clone()));
        }
    }
    found
}

/// `text` with its comments removed: `//` runs to the end of the line,
/// `{...}` and `(*...*)` are blocks, and a quoted string is kept as it is
/// (a registration a script writes in a string is not a registration, and
/// the commented-out ones of the adapter units are not either).
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                out.push(c);
                while let Some(c) = chars.next() {
                    out.push(c);
                    if c == '\'' {
                        // `''` is an escaped quote inside the literal.
                        if chars.peek() == Some(&'\'') {
                            out.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push(c);
                        break;
                    }
                }
            }
            '{' => {
                out.push(' ');
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                }
            }
            '(' if chars.peek() == Some(&'*') => {
                out.push(' ');
                chars.next();
                let mut previous = '\0';
                for c in chars.by_ref() {
                    if previous == '*' && c == ')' {
                        break;
                    }
                    previous = c;
                }
            }
            _ => out.push(c),
        }
    }
    out
}
