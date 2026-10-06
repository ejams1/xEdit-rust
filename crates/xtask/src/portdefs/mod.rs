// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask port-defs`: the transpiler from the Pascal definition calls
//! to the Rust builder API.
//!
//! State: the reader, the symbol table, the overload resolution and the Rust
//! output for the builder functions of `wbInterface.pas` exist.
//! `cargo xtask port-defs emit <upstream checkout> <file>` writes them, and
//! `cargo xtask port-defs resolve <upstream checkout> [<unit>]` reports the
//! calls that do not resolve. The output for the definition units is next.

// The Rust output will use the parts that nothing reads yet.
#![allow(dead_code)]

pub mod emit;
pub mod model;
pub mod resolve;

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};

use self::model::{Symbols, Ty};
use self::resolve::{Resolver, Scope};
use crate::pascal::ast::{Decl, Expr, Routine, Stmt, Unit};
use crate::pascal::parser::parse_unit;

pub const DEFINES: &[&str] = &["WIN64", "MSWINDOWS", "CPUX64"];

/// Reads a unit of the upstream checkout. The sources are in code page 1252.
pub fn read_unit(upstream: &Path, relative: &str) -> Result<Unit> {
    let path = upstream.join(relative);
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let source: String = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => error.into_bytes().iter().map(|&byte| char::from(byte)).collect(),
    };
    parse_unit(&source, DEFINES).with_context(|| format!("parsing {relative}"))
}

pub fn run(args: &[&str]) -> Result<()> {
    match args {
        ["resolve", upstream] => resolve_builders(Path::new(upstream)),
        ["resolve", upstream, unit] => resolve_definitions(Path::new(upstream), unit),
        ["emit", upstream, out] => emit_builders(Path::new(upstream), Path::new(out)),
        ["emit-signatures", upstream, out] => emit_signatures(Path::new(upstream), Path::new(out)),
        ["emit-unit", upstream, unit, out, stubs] => {
            emit_unit(Path::new(upstream), unit, Path::new(out), Path::new(stubs), None)
        }
        ["emit-unit", upstream, unit, out, stubs, ported] => emit_unit(
            Path::new(upstream),
            unit,
            Path::new(out),
            Path::new(stubs),
            Some(Path::new(ported)),
        ),
        _ => bail!(
            "usage: cargo xtask port-defs resolve <upstream checkout> [<definition unit>]\n       cargo xtask port-defs emit <upstream checkout> <output file>"
        ),
    }
}

/// Whether the routine builds a definition: its name starts with `wb` and it
/// returns a definition interface.
pub fn is_builder(routine: &Routine, symbols: &Symbols) -> bool {
    let returns_def = match routine
        .return_type
        .as_ref()
        .map(|type_ref| symbols.ty_of_type_ref(type_ref))
    {
        Some(Ty::Named(name)) => symbols.descends_from(&name, "iwbdef"),
        _ => false,
    };
    routine.name.to_ascii_lowercase().starts_with("wb") && !routine.name.contains('.') && returns_def
}

/// Calls `visit` for every expression in `statements`, outer calls first.
pub fn walk_statements(statements: &[Stmt], visit: &mut dyn FnMut(&Expr)) {
    for statement in statements {
        match statement {
            Stmt::Empty | Stmt::Raise(None) => {}
            Stmt::Assign { target, value, .. } => {
                walk_expr(target, visit);
                walk_expr(value, visit);
            }
            Stmt::Expr { expr, .. } | Stmt::Raise(Some(expr)) => walk_expr(expr, visit),
            Stmt::Block(inner) => walk_statements(inner, visit),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                walk_expr(condition, visit);
                walk_statements(std::slice::from_ref(then_branch), visit);
                if let Some(else_branch) = else_branch {
                    walk_statements(std::slice::from_ref(else_branch), visit);
                }
            }
            Stmt::For { from, to, body, .. } => {
                walk_expr(from, visit);
                walk_expr(to, visit);
                walk_statements(std::slice::from_ref(body), visit);
            }
            Stmt::ForIn { collection, body, .. } => {
                walk_expr(collection, visit);
                walk_statements(std::slice::from_ref(body), visit);
            }
            Stmt::While { condition, body } => {
                walk_expr(condition, visit);
                walk_statements(std::slice::from_ref(body), visit);
            }
            Stmt::Repeat { body, until } => {
                walk_statements(body, visit);
                walk_expr(until, visit);
            }
            Stmt::Case {
                selector,
                arms,
                else_branch,
            } => {
                walk_expr(selector, visit);
                for arm in arms {
                    walk_statements(std::slice::from_ref(&arm.body), visit);
                }
                if let Some(else_branch) = else_branch {
                    walk_statements(else_branch, visit);
                }
            }
            Stmt::With { targets, body } => {
                for target in targets {
                    walk_expr(target, visit);
                }
                walk_statements(std::slice::from_ref(body), visit);
            }
            Stmt::Try {
                body,
                handlers,
                except,
                finally,
            } => {
                walk_statements(body, visit);
                for handler in handlers {
                    walk_statements(std::slice::from_ref(&handler.body), visit);
                }
                for part in [except, finally].into_iter().flatten() {
                    walk_statements(part, visit);
                }
            }
            Stmt::Var(var) => {
                if let Some(value) = &var.value {
                    walk_expr(value, visit);
                }
            }
        }
    }
}

pub fn walk_expr(expr: &Expr, visit: &mut dyn FnMut(&Expr)) {
    visit(expr);
    match expr {
        Expr::Call { callee, args } | Expr::Index { base: callee, args } => {
            walk_expr(callee, visit);
            for arg in args {
                walk_expr(arg, visit);
            }
        }
        Expr::Member { base, .. } | Expr::Deref(base) | Expr::Paren(base) => walk_expr(base, visit),
        Expr::Unary { operand, .. } => walk_expr(operand, visit),
        Expr::Binary { left, right, .. } | Expr::Range(left, right) => {
            walk_expr(left, visit);
            walk_expr(right, visit);
        }
        Expr::List(items) => {
            for item in items {
                walk_expr(item, visit);
            }
        }
        Expr::Inherited(Some(inner)) => walk_expr(inner, visit),
        _ => {}
    }
}

/// The local scope of a routine: its parameters and variables.
pub fn routine_scope(routine: &Routine, symbols: &Symbols) -> Scope {
    let mut scope = Scope::default();
    for param in &routine.params {
        let ty = match &param.type_ref {
            Some(type_ref) => symbols.ty_of_type_ref(type_ref),
            None => Ty::Unknown,
        };
        scope.insert(&param.name, ty);
    }
    if let Some(return_type) = &routine.return_type {
        scope.insert("Result", symbols.ty_of_type_ref(return_type));
    }
    if let Some(body) = &routine.body {
        for decl in &body.decls {
            if let Decl::Var(var) | Decl::Const(var) = decl {
                let ty = match &var.type_ref {
                    Some(type_ref) => symbols.ty_of_type_ref(type_ref),
                    None => Ty::Unknown,
                };
                for name in &var.names {
                    scope.insert(name, ty.clone());
                }
            }
        }
        add_inline_vars(&body.statements, &Resolver { symbols }, &mut scope);
    }
    scope
}

/// Adds the inline `var` and `const` declarations in `statements` to `scope`,
/// in source order so that an inferred type can depend on an earlier one.
fn add_inline_vars(statements: &[Stmt], resolver: &Resolver, scope: &mut Scope) {
    for statement in statements {
        match statement {
            Stmt::Var(var) => {
                let ty = match (&var.type_ref, &var.value) {
                    (Some(type_ref), _) => resolver.symbols.ty_of_type_ref(type_ref),
                    (None, Some(value)) => resolver.ty_of(value, scope),
                    (None, None) => Ty::Unknown,
                };
                for name in &var.names {
                    scope.insert(name, ty.clone());
                }
            }
            Stmt::Block(inner) | Stmt::Repeat { body: inner, .. } => add_inline_vars(inner, resolver, scope),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                add_inline_vars(std::slice::from_ref(then_branch), resolver, scope);
                if let Some(else_branch) = else_branch {
                    add_inline_vars(std::slice::from_ref(else_branch), resolver, scope);
                }
            }
            Stmt::For { body, .. } | Stmt::ForIn { body, .. } | Stmt::While { body, .. } | Stmt::With { body, .. } => {
                add_inline_vars(std::slice::from_ref(body), resolver, scope)
            }
            Stmt::Case { arms, else_branch, .. } => {
                for arm in arms {
                    add_inline_vars(std::slice::from_ref(&arm.body), resolver, scope);
                }
                if let Some(else_branch) = else_branch {
                    add_inline_vars(else_branch, resolver, scope);
                }
            }
            Stmt::Try {
                body, except, finally, ..
            } => {
                add_inline_vars(body, resolver, scope);
                for part in [except, finally].into_iter().flatten() {
                    add_inline_vars(part, resolver, scope);
                }
            }
            _ => {}
        }
    }
}

/// Resolves every call of a known routine in the bodies of the builder
/// functions of `wbInterface.pas`.
fn resolve_builders(upstream: &Path) -> Result<()> {
    let unit = read_unit(upstream, "Core/wbInterface.pas")?;
    let mut symbols = Symbols::default();
    symbols.add_unit(&unit, true);
    let resolver = Resolver { symbols: &symbols };

    let mut builders = 0;
    let mut resolved = 0;
    let mut failures = Vec::new();
    for decl in &unit.implementation {
        let Decl::Routine(routine) = decl else { continue };
        let Some(body) = &routine.body else { continue };
        if !is_builder(routine, &symbols) {
            continue;
        }
        builders += 1;
        let scope = routine_scope(routine, &symbols);
        walk_statements(&body.statements, &mut |expr| {
            if let Expr::Call { callee, args } = expr
                && let Expr::Ident(name) = &**callee
                && symbols.routines.contains_key(&name.to_ascii_lowercase())
            {
                match resolver.resolve_call(name, args, &scope) {
                    Ok(_) => resolved += 1,
                    Err(error) => failures.push(format!("{} (line {}): {error}", routine.name, routine.line)),
                }
            }
        });
    }
    println!(
        "{builders} builder functions, {resolved} calls resolved, {} failed",
        failures.len()
    );
    for failure in &failures {
        println!("  {failure}");
    }
    ensure!(failures.is_empty(), "{} call(s) did not resolve", failures.len());
    Ok(())
}

/// Writes the Rust functions for the builder functions of `wbInterface.pas`
/// and reports the builder functions that the transpiler cannot write.
fn emit_builders(upstream: &Path, out: &Path) -> Result<()> {
    let unit = read_unit(upstream, "Core/wbInterface.pas")?;
    let mut symbols = Symbols::default();
    symbols.add_unit(&unit, true);
    let emitter = emit::Emitter::new(&symbols, "wbInterface");

    let mut functions = Vec::new();
    let mut failures = Vec::new();
    for decl in &unit.implementation {
        let Decl::Routine(routine) = decl else { continue };
        if !is_builder(routine, &symbols) {
            continue;
        }
        match emitter.routine(routine) {
            Ok(function) => functions.push(function),
            Err(error) => failures.push(format!("{} (line {}): {error}", routine.name, routine.line)),
        }
    }
    let mut text = String::from(GENERATED_HEADER);
    for function in &functions {
        text.push('\n');
        text.push_str(function);
    }
    std::fs::write(out, text).with_context(|| format!("writing {}", out.display()))?;
    println!("{} functions written, {} not written", functions.len(), failures.len());
    for failure in &failures {
        println!("  {failure}");
    }
    let written: HashSet<&str> = functions
        .iter()
        .filter_map(|function| function.split("pub fn ").nth(1)?.split('(').next())
        .collect();
    println!("referenced and not written:");
    for (name, what) in emitter.references.borrow().iter() {
        if !written.contains(name.as_str()) {
            println!("  {name}: {what}");
        }
    }
    Ok(())
}

const GENERATED_HEADER: &str = "\
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas
// Generated by `cargo xtask port-defs emit`. Do not edit.

//! The builder functions of `wbInterface.pas`, one Rust function per
//! Pascal overload. See `crates/xtask/src/portdefs/emit.rs` for the naming.

#![allow(clippy::all, unused_variables, unused_mut, unused_assignments, unused_parens)]

use std::sync::Arc;

use super::*;
use super::globals::*;
";

/// Writes the signature constants of `wbDefinitionsSignatures.pas`.
fn emit_signatures(upstream: &Path, out: &Path) -> Result<()> {
    let unit = read_unit(upstream, "Core/wbDefinitionsSignatures.pas")?;
    let mut text = String::from(SIGNATURES_HEADER);
    let mut count = 0;
    for decl in &unit.interface {
        let Decl::Const(var) = decl else { continue };
        let Some(Expr::Str(value)) = &var.value else { continue };
        if value.len() != 4 || !value.is_ascii() {
            bail!("the signature {} is not four ASCII characters", var.names[0]);
        }
        for name in &var.names {
            text.push_str(&format!(
                "pub const {name}: Signature = Signature::new(b\"{}\");
",
                emit::byte_string(value)
            ));
            count += 1;
        }
    }
    std::fs::write(out, text).with_context(|| format!("writing {}", out.display()))?;
    println!("{count} signatures written");
    Ok(())
}

const SIGNATURES_HEADER: &str = "\
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsSignatures.pas
// Generated by `cargo xtask port-defs emit-signatures`. Do not edit.

//! The signature constants of every game.

use xedit_core::interface::Signature;

";

/// The symbol table for a definition unit: `wbInterface`, the signatures,
/// `wbDefinitionsCommon` and the unit itself with its implementation.
fn definition_symbols(upstream: &Path, unit_name: &str) -> Result<(Symbols, Unit)> {
    let interface = read_unit(upstream, "Core/wbInterface.pas")?;
    let signatures = read_unit(upstream, "Core/wbDefinitionsSignatures.pas")?;
    let mut symbols = Symbols::default();
    // `TProc` of `System.SysUtils`, the type of the resources loaded handlers.
    symbols.add_unit(
        &crate::pascal::parser::parse_unit(
            "unit System; interface type TProc = reference to procedure; implementation end.",
            DEFINES,
        )?,
        false,
    );
    symbols.add_unit(&interface, false);
    symbols.add_unit(&signatures, false);
    if unit_name != "wbDefinitionsCommon" {
        let common = read_unit(upstream, "Core/wbDefinitionsCommon.pas")?;
        symbols.add_unit(&common, false);
    }
    // Starfield uses the reflection definitions.
    if unit_name == "wbDefinitionsSF1" {
        let reflection = read_unit(upstream, "Core/wbDefinitionsReflection.pas")?;
        symbols.add_unit(&reflection, false);
    }
    let unit = read_unit(upstream, &format!("Core/{unit_name}.pas"))?;
    symbols.add_unit(&unit, true);
    Ok((symbols, unit))
}

/// Whether the routine of a definition unit is written by the transpiler:
/// every routine with a body that is not a method and not a callback. The
/// callbacks are ported by hand.
fn is_definition_routine(routine: &Routine, symbols: &Symbols) -> bool {
    if routine.name.contains('.') || routine.body.is_none() {
        return false;
    }
    let sig = model::RoutineSig {
        name: routine.name.clone(),
        params: routine.params.clone(),
        return_type: routine.return_type.clone(),
        unit: String::new(),
        line: routine.line,
    };
    let callback = symbols.types.values().find(|info| match &info.kind {
        // `TProc` is only the type of the resources loaded handlers; a
        // procedure without arguments such as `DefineSF1` is not a callback.
        model::TypeKind::Callback(callback) if !info.name.eq_ignore_ascii_case("TProc") => {
            resolve::callback_matches(&sig, callback)
        }
        _ => false,
    });
    if let Some(info) = callback
        && std::env::var_os("PORTDEFS_DEBUG").is_some()
    {
        eprintln!("callback {} as {}", routine.name, info.name);
    }
    callback.is_none()
}

/// Writes the Rust module for a definition unit and the stubs of the
/// callbacks it names, and reports what the transpiler cannot write.
fn emit_unit(upstream: &Path, unit_name: &str, out: &Path, stubs: &Path, ported: Option<&Path>) -> Result<()> {
    let (symbols, unit) = definition_symbols(upstream, unit_name)?;
    let emitter = emit::Emitter::new(&symbols, unit_name);
    let mut failures = Vec::new();
    // The functions of the file that is written by hand need no stub.
    // The functions of the file that is written by hand, and for a game
    // unit the functions and stubs of Common, need no stub.
    let mut ported_files: Vec<std::path::PathBuf> = ported.map(Path::to_path_buf).into_iter().collect();
    if unit_name != "wbDefinitionsCommon"
        && let Some(dir) = stubs.parent()
    {
        ported_files.push(dir.join("common.rs"));
        ported_files.push(dir.join("common_stubs.rs"));
        if unit_name == "wbDefinitionsSF1" {
            ported_files.push(dir.join("reflection.rs"));
            ported_files.push(dir.join("reflection_stubs.rs"));
        }
    }
    let mut ported: HashSet<String> = HashSet::new();
    for path in &ported_files {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        ported.extend(
            text.lines()
                .filter_map(|line| line.strip_prefix("pub fn ")?.split('(').next().map(str::to_owned)),
        );
    }

    let mut text = format!("{GENERATED_UNIT_HEADER}\n// Ported from xEdit: Core/{unit_name}.pas\n\n");
    text.push_str(&unit_imports(unit_name));
    for decl in unit.interface.iter().chain(&unit.implementation) {
        match decl {
            Decl::Type(type_decl) => match emitter.unit_type(&type_decl.name) {
                Ok(Some(item)) => {
                    text.push('\n');
                    text.push_str(&item);
                }
                Ok(None) => {}
                Err(error) => failures.push(format!("{} (line {}): {error}", type_decl.name, type_decl.line)),
            },
            Decl::Const(var) => {
                for name in &var.names {
                    match emitter.unit_const(name, var) {
                        Ok(Some(item)) => {
                            text.push('\n');
                            text.push_str(&item);
                        }
                        Ok(None) => {}
                        Err(error) => failures.push(format!("{name} (line {}): {error}", var.line)),
                    }
                }
            }
            _ => {}
        }
    }
    for decl in unit.interface.iter().chain(&unit.implementation) {
        let Decl::Var(var) = decl else { continue };
        for name in &var.names {
            match emitter.unit_static(name) {
                Ok(item) => {
                    text.push('\n');
                    text.push_str(&item);
                }
                Err(error) => failures.push(format!("{name} (line {}): {error}", var.line)),
            }
        }
    }
    let mut written = 0;
    for decl in &unit.implementation {
        let Decl::Routine(routine) = decl else { continue };
        if !is_definition_routine(routine, &symbols) {
            continue;
        }
        // A routine that is written by hand is not generated.
        if emitter.rust_name(routine).is_some_and(|name| ported.contains(&name)) {
            continue;
        }
        match emitter.routine(routine) {
            Ok(function) => {
                text.push('\n');
                text.push_str(&function);
                written += 1;
            }
            Err(error) => {
                let signature = emitter.hand_signature(routine).unwrap_or_default();
                failures.push(format!(
                    "{} (line {}): {error}
    {signature}",
                    routine.name, routine.line
                ));
            }
        }
    }
    std::fs::write(out, &text).with_context(|| format!("writing {}", out.display()))?;

    // The Common unit also stubs the Common callbacks that only the game
    // units name, so that each has one stub, in `common_stubs.rs`.
    let mut stub_callbacks: Vec<(String, (String, String, u32), &str)> = emitter
        .callbacks
        .borrow()
        .iter()
        .map(|(rust, callback)| (rust.clone(), callback.clone(), unit_name))
        .collect();
    if unit_name == "wbDefinitionsCommon" {
        let mut common_callbacks = BTreeMap::new();
        for game_unit in GAME_UNITS {
            let (game_symbols, game) = definition_symbols(upstream, game_unit)?;
            let game_emitter = emit::Emitter::new(&game_symbols, game_unit);
            for decl in &game.implementation {
                if let Decl::Routine(routine) = decl
                    && is_definition_routine(routine, &game_symbols)
                {
                    let _ = game_emitter.routine(routine);
                }
            }
            common_callbacks.extend(game_emitter.common_callbacks.borrow().clone());
        }
        for (rust, callback) in common_callbacks {
            if !emitter.callbacks.borrow().contains_key(&rust) {
                stub_callbacks.push((rust, callback, unit_name));
            }
        }
        stub_callbacks.sort_by(|a, b| a.0.cmp(&b.0));
    }

    let mut stub_text = format!("{GENERATED_UNIT_HEADER}\n// Ported from xEdit: Core/{unit_name}.pas\n\n");
    stub_text.push_str(STUBS_IMPORTS);
    let mut stub_count = 0;
    for (rust, (pascal, callback_type, line), stub_unit) in &stub_callbacks {
        if ported.contains(rust) {
            continue;
        }
        match emitter.callback_stub(rust, pascal, callback_type, *line, stub_unit) {
            Some(stub) => {
                stub_text.push('\n');
                stub_text.push_str(&stub);
                stub_count += 1;
            }
            None => failures.push(format!("{pascal}: callback type {callback_type} has no stub form")),
        }
    }
    std::fs::write(stubs, stub_text).with_context(|| format!("writing {}", stubs.display()))?;

    println!(
        "{unit_name}: {written} routines and {stub_count} callback stubs written, {} failures",
        failures.len()
    );
    for failure in &failures {
        println!("  {failure}");
    }
    let written: HashSet<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("pub fn ")?.split('(').next())
        .collect();
    println!("referenced and not written:");
    for (name, what) in emitter.references.borrow().iter() {
        if !emitter.callbacks.borrow().contains_key(name)
            && !written.contains(name.as_str())
            && !ported.contains(name)
            && !what.starts_with("wbInterface:")
            && !(what.starts_with("wbDefinitionsCommon:") && unit_name != "wbDefinitionsCommon")
        {
            println!("  {name}: {what}");
        }
    }
    Ok(())
}

const GENERATED_UNIT_HEADER: &str = "\
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
";

/// The game definition units, whose Common callbacks the Common unit stubs.
const GAME_UNITS: &[&str] = &[
    "wbDefinitionsTES3",
    "wbDefinitionsTES4",
    "wbDefinitionsFO3",
    "wbDefinitionsFNV",
    "wbDefinitionsTES5",
    "wbDefinitionsFO4",
    "wbDefinitionsFO76",
    "wbDefinitionsSF1",
    "wbDefinitionsReflection",
];

/// The module name of a definition unit: `wbDefinitionsFO4` is `fo4`.
pub fn unit_module(unit_name: &str) -> String {
    unit_name
        .strip_prefix("wbDefinitions")
        .unwrap_or(unit_name)
        .to_ascii_lowercase()
}

fn unit_imports(unit_name: &str) -> String {
    let module = unit_module(unit_name);
    let mut text = String::from(
        "\
// Generated by `cargo xtask port-defs emit-unit`. Do not edit.

#![allow(clippy::all, unused_variables, unused_mut, unused_assignments, unused_parens, unused_imports, unreachable_code)]

use std::sync::Arc;

use xedit_core::delphi::*;
use xedit_core::interface::*;
use xedit_core::interface::globals::*;

use crate::signatures::*;
",
    );
    if module != "common" {
        text.push_str("use crate::callbacks::common::*;\nuse crate::common::*;\n");
    }
    if module == "sf1" {
        text.push_str("use crate::callbacks::reflection::*;\nuse crate::reflection::*;\n");
    }
    text.push_str(&format!("use crate::callbacks::{module}::*;\n"));
    text
}

const STUBS_IMPORTS: &str = "\
// Generated by `cargo xtask port-defs emit-unit`. Do not edit.

//! Stubs of the callbacks that are not ported yet. Each panics when called.

#![allow(clippy::all, unused_variables, unused_imports)]

use std::sync::Arc;

use xedit_core::interface::*;
";

/// Resolves every call of a builder function in a definition unit such as
/// `wbDefinitionsFO4`.
fn resolve_definitions(upstream: &Path, unit_name: &str) -> Result<()> {
    let interface = read_unit(upstream, "Core/wbInterface.pas")?;
    let signatures = read_unit(upstream, "Core/wbDefinitionsSignatures.pas")?;
    let common = read_unit(upstream, "Core/wbDefinitionsCommon.pas")?;
    let unit = read_unit(upstream, &format!("Core/{unit_name}.pas"))?;
    let mut symbols = Symbols::default();
    symbols.add_unit(&interface, false);
    symbols.add_unit(&signatures, false);
    symbols.add_unit(&common, false);
    symbols.add_unit(&unit, true);
    let resolver = Resolver { symbols: &symbols };

    let mut resolved = 0;
    let mut failures = Vec::new();
    for decl in &unit.implementation {
        let Decl::Routine(routine) = decl else { continue };
        let Some(body) = &routine.body else { continue };
        let scope = routine_scope(routine, &symbols);
        walk_statements(&body.statements, &mut |expr| {
            if let Expr::Call { callee, args } = expr
                && let Expr::Ident(name) = &**callee
                && name.to_ascii_lowercase().starts_with("wb")
                && symbols.routines.contains_key(&name.to_ascii_lowercase())
            {
                match resolver.resolve_call(name, args, &scope) {
                    Ok(_) => resolved += 1,
                    Err(error) => {
                        let unknown: Vec<String> = args
                            .iter()
                            .filter(|arg| resolver.ty_of(arg, &scope) == Ty::Unknown)
                            .map(|arg| format!("{arg:?}").chars().take(160).collect())
                            .collect();
                        failures.push(format!(
                            "{} (line {}): {error} unknown: {unknown:?}",
                            routine.name, routine.line
                        ));
                    }
                }
            }
        });
    }
    for failure in &failures {
        println!("{failure}");
    }
    println!("{unit_name}: {resolved} calls resolved, {} failed", failures.len());
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for failure in &failures {
        let message = failure.split("): ").nth(1).unwrap_or(failure);
        *kinds.entry(message.to_owned()).or_default() += 1;
    }
    let mut kinds: Vec<(usize, String)> = kinds.into_iter().map(|(message, count)| (count, message)).collect();
    kinds.sort_by(|a, b| b.cmp(a));
    for (count, message) in kinds.iter().take(40) {
        println!("  {count:5} {message}");
    }
    ensure!(failures.is_empty(), "{} call(s) did not resolve", failures.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "unit wbTest;
        interface
        type
          TwbSignature = array[0..3] of AnsiChar;
          TwbConflictPriority = (cpIgnore, cpNormal);
          IwbDef = interface end;
          IwbNamedDef = interface(IwbDef) end;
          IwbValueDef = interface(IwbNamedDef) end;
          IwbIntegerDef = interface(IwbValueDef) end;
          IwbRecordMemberDef = interface(IwbNamedDef) end;
          IwbSubRecordDef = interface(IwbRecordMemberDef) end;
          IwbIntegerDefFormater = interface(IwbDef) end;
          TwbIntToStrCallback = reference to function(aInt: Int64; const aElement: IwbElement): string;
          TwbStrToIntCallback = reference to function(const aString: string): Int64;
          TwbSignatures = array of TwbSignature;
        const
          DATA: TwbSignature = 'DATA';
        var
          wbEDID: IwbRecordMemberDef;
        function wbInteger(const aSignature: TwbSignature; const aName: string;
          const aFormater: IwbIntegerDefFormater = nil; aPriority: TwbConflictPriority = cpNormal): IwbSubRecordDef; overload;
        function wbInteger(const aName: string; const aFormater: IwbIntegerDefFormater = nil;
          aPriority: TwbConflictPriority = cpNormal): IwbIntegerDef; overload;
        function wbInteger(const aName: string; const aToStr: TwbIntToStrCallback;
          const aToInt: TwbStrToIntCallback = nil): IwbIntegerDef; overload;
        function wbEnum(const aNames: array of string): IwbIntegerDefFormater;
        function wbStruct(const aName: string; const aMembers: array of IwbValueDef): IwbValueDef;
        function wbFormID: IwbIntegerDefFormater;
        function wbSigs(const aSignatures: TwbSignatures): IwbValueDef;
        function ToStr(aInt: Int64; const aElement: IwbElement): string;
        function Helper(a: Integer): Integer;
        implementation
        end.";

    fn check(call: &str) -> Result<usize> {
        let unit = parse_unit(SOURCE, DEFINES).unwrap();
        let mut symbols = Symbols::default();
        symbols.add_unit(&unit, false);
        let resolver = Resolver { symbols: &symbols };
        let Expr::Call { callee, args } = crate::pascal::parser::parse_expr(call).unwrap() else {
            panic!("not a call");
        };
        let Expr::Ident(name) = *callee else {
            panic!("not a routine")
        };
        resolver
            .resolve_call(&name, &args, &Scope::default())
            .map(|(resolved, _)| resolved.overload)
    }

    #[test]
    fn overloads_resolve_by_argument_types() {
        assert_eq!(check("wbInteger(DATA, 'Value')").unwrap(), 0);
        assert_eq!(check("wbInteger('Value')").unwrap(), 1);
        assert_eq!(check("wbInteger('Value', wbEnum(['A', 'B']), cpNormal)").unwrap(), 1);
        assert_eq!(check("wbInteger('Value', nil, cpIgnore)").unwrap(), 1);
        // A routine name fits a callback type with the same parameters.
        assert_eq!(check("wbInteger('Value', ToStr)").unwrap(), 2);
        // A routine without parameters is called.
        assert_eq!(check("wbInteger('Value', wbFormID)").unwrap(), 1);
        // A string of four characters is a signature, but a string comes first.
        assert_eq!(check("wbInteger('DATA', 'Value')").unwrap(), 0);
        assert_eq!(check("wbStruct('S', [wbInteger('A'), wbStruct('B', [])])").unwrap(), 0);
        assert_eq!(check("wbSigs([DATA, 'XXXX'])").unwrap(), 0);
    }

    #[test]
    fn calls_that_do_not_fit_fail() {
        assert!(check("wbInteger(1, 2)").is_err());
        assert!(check("wbInteger('Value', Helper)").is_err());
        assert!(check("wbStruct('S', [wbEnum([])])").is_err());
        assert!(
            check("wbInteger('Value', nil)")
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
    }
}
