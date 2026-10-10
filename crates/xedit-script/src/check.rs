// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptHost.pas (the uses namespace
// stripping of TxejviScript.Create and the unit source resolution of
// JvInterpreterProgramGetUnitSource)

//! Checking a script: compile it and every unit it uses, the way
//! `TJvInterpreterUnit.Compile` accepts them -- routine bodies are only
//! scanned for their balanced `end`, not parsed, so a statement-level error
//! surfaces when a function runs, as in the interpreter (the full statement
//! grammar is [`crate::interpreter::parse`], which the evaluator uses).
//!
//! The resolution follows `JvInterpreterProgramGetUnitSource`
//! (`xejviScriptHost.pas`): a unit name is looked up as `<name>.pas` in the
//! scripts folder; a name with no file there is a unit compiled into the
//! interpreter's host (the handler answers `unit <name>; end.`, an empty
//! stub), so it parses to nothing. The real `xEditAPI.pas` is such a name at
//! run time (the host stubs it even when the file exists); the check command
//! parses the file when the folder has one, because the declaration-only
//! reference unit is part of the corpus gate and its declarations are what a
//! later step binds identifiers against.
//!
//! Only the main script goes through the namespace stripping of
//! `TxejviScript.Create`; the units loaded with it are parsed as they are,
//! as upstream parses them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::ast::{ItemNode, Module};
use crate::error::line_of;
use crate::interpreter;

/// One failure: the file it was raised in (the script or a unit it uses),
/// the line (1-based, counted as `GetLineByPos` counts it; 0 for a failure
/// before a source was read) and the interpreter's message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    pub file: String,
    pub line: i64,
    pub message: String,
}

/// The result of checking one script: empty `errors` means it parses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptCheck {
    pub file: String,
    pub errors: Vec<ScriptError>,
}

impl ScriptCheck {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Checks one script file with its used units resolved from `scripts_folder`.
pub fn check_file(path: &Path, scripts_folder: &Path) -> ScriptCheck {
    let file = path.to_string_lossy().into_owned();
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return ScriptCheck {
                file: file.clone(),
                errors: vec![ScriptError {
                    file,
                    line: 0,
                    message: format!("could not read: {error}"),
                }],
            };
        }
    };
    let source = strip_namespaces(&bytes);
    let mut errors = Vec::new();
    let mut started = HashSet::new();
    match interpreter::parse_compile(&source) {
        Err(error) => errors.push(ScriptError {
            file: file.clone(),
            line: line_of(&source, error.pos),
            message: error.message,
        }),
        Ok(module) => {
            // The unit that is being checked is registered when it reads its
            // first unit, so a cycle back to it stops (`ReadUnit`'s
            // `UnitExists` guard, `JvInterpreter.pas:8032`).
            started.insert(module.name.to_ascii_lowercase());
            check_uses(&module, path, &source, scripts_folder, &mut started, &mut errors);
        }
    }
    ScriptCheck { file, errors }
}

fn check_uses(
    module: &Module,
    module_path: &Path,
    source: &[u8],
    scripts_folder: &Path,
    started: &mut HashSet<String>,
    errors: &mut Vec<ScriptError>,
) {
    for item in &module.items {
        let ItemNode::Uses(uses) = &item.node else {
            continue;
        };
        for unit in &uses.units {
            let key = unit.name.to_ascii_lowercase();
            if !started.insert(key) {
                continue;
            }
            let Some(unit_path) = find_unit_file(scripts_folder, &unit.name) else {
                // A unit of the interpreter's host: the empty stub.
                continue;
            };
            if same_file(&unit_path, module_path) {
                continue;
            }
            let file = unit_path.to_string_lossy().into_owned();
            let bytes = match std::fs::read(&unit_path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    errors.push(ScriptError {
                        file,
                        line: 0,
                        message: format!("could not read: {error}"),
                    });
                    continue;
                }
            };
            match interpreter::parse_compile(&bytes) {
                Err(error) => errors.push(ScriptError {
                    file,
                    line: line_of(&bytes, error.pos),
                    message: error.message,
                }),
                Ok(unit_module) => {
                    check_uses(&unit_module, &unit_path, &bytes, scripts_folder, started, errors);
                }
            }
        }
    }
    let _ = (module_path, source);
}

/// The port of `TxejviScript.Create`'s `TPerlRegEx` block
/// (`xejviScriptHost.pas:342`): `^\s*uses\s+(.+?);` with `preCaseLess`,
/// `preSingleLine` and `preMultiLine`; inside every match the five Delphi
/// namespace prefixes (`system.`, `vcl.`, `winapi.`, `data.`, `web.`) are
/// removed with `rfReplaceAll, rfIgnoreCase`. The scan continues after the
/// replacement, as `MatchAgain` does with `Start := i + Length(s)`.
pub fn strip_namespaces(source: &[u8]) -> Vec<u8> {
    const PREFIXES: [&[u8]; 5] = [b"system.", b"vcl.", b"winapi.", b"data.", b"web."];
    let mut text = source.to_vec();
    let mut start = 0usize;
    while start < text.len() {
        // `^` with `preMultiLine`: the start of the subject or after '\n'.
        if start != 0 && text[start - 1] != b'\n' {
            start += 1;
            continue;
        }
        let mut i = start;
        while i < text.len() && is_space(text[i]) {
            i += 1;
        }
        if i + 4 > text.len() || !text[i..i + 4].eq_ignore_ascii_case(b"uses") {
            start += 1;
            continue;
        }
        let mut j = i + 4;
        let whitespace_start = j;
        while j < text.len() && is_space(text[j]) {
            j += 1;
        }
        if j == whitespace_start {
            start += 1;
            continue;
        }
        // `(.+?)` up to the first ';' (`preSingleLine` lets `.` cross lines).
        let Some(semicolon) = text[j..].iter().position(|&c| c == b';') else {
            break;
        };
        let match_end = j + semicolon + 1;
        let matched = text[start..match_end].to_vec();
        let replaced = remove_prefixes(&matched, &PREFIXES);
        if replaced != matched {
            let mut next = Vec::with_capacity(text.len());
            next.extend_from_slice(&text[..start]);
            next.extend_from_slice(&replaced);
            next.extend_from_slice(&text[match_end..]);
            text = next;
        }
        start += replaced.len().max(1);
    }
    text
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn remove_prefixes(matched: &[u8], prefixes: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(matched.len());
    let mut i = 0usize;
    'outer: while i < matched.len() {
        for prefix in prefixes {
            if matched.len() - i >= prefix.len() && matched[i..i + prefix.len()].eq_ignore_ascii_case(prefix) {
                i += prefix.len();
                continue 'outer;
            }
        }
        out.push(matched[i]);
        i += 1;
    }
    out
}

/// `wbScriptsPath + UnitName + '.pas'` (`xejviScriptHost.pas:450`). The
/// lookup follows the Windows file system (case-insensitive); the fallback
/// scan keeps the same behaviour on a case-sensitive file system.
fn find_unit_file(folder: &Path, unit_name: &str) -> Option<PathBuf> {
    let candidate = folder.join(format!("{unit_name}.pas"));
    if candidate.is_file() {
        return Some(candidate);
    }
    let wanted = format!("{unit_name}.pas");
    let entries = std::fs::read_dir(folder).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.eq_ignore_ascii_case(&wanted) {
            return Some(entry.path());
        }
    }
    None
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
