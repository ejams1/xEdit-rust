// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask port-defs`: the transpiler from the Pascal definition calls
//! to the Rust builder API.
//!
//! State: the reader, the symbol table and the overload resolution exist.
//! `cargo xtask port-defs resolve <upstream checkout>` resolves every call in
//! the bodies of the builder functions of `wbInterface.pas` and reports the
//! calls that do not resolve. The Rust output is not written yet.

// The Rust output will use the parts that nothing reads yet.
#![allow(dead_code)]

pub mod model;
pub mod resolve;

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
        _ => bail!("usage: cargo xtask port-defs resolve <upstream checkout> [<definition unit>]"),
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
