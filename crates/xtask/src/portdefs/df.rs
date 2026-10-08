// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask port-defs emit-df`: the transpiler for the data format
//! units (`wbDataFormatNifTypes`, `wbDataFormatNif`).
//!
//! These units declare NIF blocks with the builder functions of
//! `wbDataFormat` (`dfStruct`, `dfArray`, ...) and their own `wb*`
//! builders, and attach event callbacks that are mostly one-line version
//! checks. The transpiler writes three kinds of routines to Rust:
//!
//! - builders: functions that return a definition. Each overload becomes a
//!   function of its own (the one with the most parameters keeps the base
//!   name, the others get their parameter names appended), because the
//!   overloads do not always delegate with plain defaults (`dfBool` with
//!   events drops them).
//! - `wbDefine*` procedures, which register NiObjects.
//! - callbacks: the event procedures and the helpers they call, in a small
//!   typed subset (version checks, `NativeValues` lookups and comparisons,
//!   `with nif(e) do`, assignments to the `var` parameter). A callback
//!   outside of the subset gets a stub that returns an error, and is
//!   ported by hand into the hand file, whose `pub fn`s are never generated.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use super::emit::snake;
use super::read_unit;
use crate::pascal::ast::{Decl, Expr, Param, Routine, RoutineKind, Stmt, TypeRef, Unit, VarDecl};

const HEADER: &str = "\
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
";

/// The parameter types of the builder functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PKind {
    Str,
    /// `const aEvents: array of const`
    Events,
    /// The value map of `dfFlags` and `dfEnum`: `array of const`.
    Values,
    Defs,
    DataType,
    Int,
    Char,
    Bool,
    Decider,
    Def,
}

#[derive(Debug, Clone)]
struct BuilderSig {
    rust: String,
    params: Vec<(String, PKind)>,
}

/// The kind of a callback, by its signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CbKind {
    /// `function(const e: TdfElement): Boolean`
    Enabled,
    /// `function(const e: TdfElement): Integer`, a decider or a helper.
    IntFn,
    /// `procedure(const e: TdfElement; var aValue: Variant)`
    Value,
    /// `procedure(const e: TdfElement; var aText: string)`
    Text,
    /// `procedure(const e: TdfElement; var aCount: Integer)`
    Count,
    /// `procedure(const e: TdfElement; const aDataStart: Pointer; aDataSize: Integer)`
    AfterLoad,
    /// `procedure(const e: TdfElement)`
    Proc,
    /// `function(const e: TdfElement): TdfElement`
    LinksTo,
}

#[derive(Debug, Clone)]
struct CallbackSig {
    rust: String,
    kind: CbKind,
    /// The Pascal names of the parameters.
    params: Vec<String>,
    line: u32,
}

/// The types of the callback subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum T {
    Bool,
    Int,
    Float,
    Str,
    Var,
    El,
    OptEl,
    /// `nif(e)`.
    Nif,
    /// `TwbNifVersion`.
    NifEnum,
    /// The data start of `OnAfterLoad`, which is only compared with `nil`.
    Ptr,
}

const DEF_TYPES: &[&str] = &[
    "tdfdef",
    "tdfstructdef",
    "tdfchardef",
    "tdfcharsdef",
    "tdfarraydef",
    "twbnirefdef",
    "twbnifblockdef",
    "tdfintegerdef",
    "tdfenumdef",
    "tdfflagsdef",
    "tdffloatdef",
    "tdfmergedef",
    "tdfuniondef",
    "tdfvaluenuiondef",
    "tdfvalueuniondef",
    "tdfbytesdef",
];

fn lower(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn type_name(type_ref: &Option<TypeRef>) -> String {
    match type_ref {
        Some(TypeRef::Named(name)) => lower(name.rsplit('.').next().unwrap_or(name)),
        Some(TypeRef::ArrayOfConst) => "array of const".to_owned(),
        Some(TypeRef::ArrayOf(inner)) => format!("array of {}", type_name(&Some((**inner).clone()))),
        _ => String::new(),
    }
}

fn is_builder(routine: &Routine) -> bool {
    routine.kind == RoutineKind::Function
        && !routine.name.contains('.')
        && DEF_TYPES.contains(&type_name(&routine.return_type).as_str())
}

fn pkind(param: &Param) -> Result<PKind> {
    let name = lower(&param.name);
    Ok(match type_name(&param.type_ref).as_str() {
        "string" => PKind::Str,
        "array of const" if name == "aevents" => PKind::Events,
        "array of const" => PKind::Values,
        "tdfdefs" => PKind::Defs,
        "tdfdatatype" => PKind::DataType,
        "integer" | "cardinal" => PKind::Int,
        "ansichar" | "char" => PKind::Char,
        "boolean" => PKind::Bool,
        "tdfondecideevent" => PKind::Decider,
        other if DEF_TYPES.contains(&other) => PKind::Def,
        other => bail!("parameter {} of type {other}", param.name),
    })
}

fn rust_param_type(kind: PKind) -> &'static str {
    match kind {
        PKind::Str => "&str",
        PKind::Events => "&[Event]",
        PKind::Values => "&[(i64, &str)]",
        PKind::Defs => "Vec<Def>",
        PKind::DataType => "DataType",
        PKind::Int => "i32",
        PKind::Char => "u8",
        PKind::Bool => "bool",
        PKind::Decider => "Option<OnDecide>",
        PKind::Def => "Def",
    }
}

fn callback_kind(routine: &Routine) -> Option<CbKind> {
    let first = routine.params.first()?;
    if type_name(&first.type_ref) != "tdfelement" || routine.name.contains('.') {
        return None;
    }
    let rest: Vec<(String, String, String)> = routine.params[1..]
        .iter()
        .map(|param| (lower(&param.modifier), lower(&param.name), type_name(&param.type_ref)))
        .collect();
    let ret = type_name(&routine.return_type);
    match (routine.kind, ret.as_str(), rest.as_slice()) {
        (RoutineKind::Function, "boolean", []) => Some(CbKind::Enabled),
        (RoutineKind::Function, "integer" | "word" | "cardinal", []) => Some(CbKind::IntFn),
        (RoutineKind::Function, "tdfelement", []) => Some(CbKind::LinksTo),
        (RoutineKind::Procedure, _, []) => Some(CbKind::Proc),
        (RoutineKind::Procedure, _, [(modifier, _, ty)]) if modifier == "var" && ty == "variant" => Some(CbKind::Value),
        (RoutineKind::Procedure, _, [(modifier, _, ty)]) if modifier == "var" && ty == "string" => Some(CbKind::Text),
        (RoutineKind::Procedure, _, [(modifier, _, ty)]) if modifier == "var" && ty == "integer" => Some(CbKind::Count),
        (RoutineKind::Procedure, _, [(_, _, a), (_, _, b)]) if a == "pointer" && b == "integer" => {
            Some(CbKind::AfterLoad)
        }
        _ => None,
    }
}

fn callback_signature(sig: &CallbackSig) -> String {
    let p = |index: usize| snake(&sig.params[index]);
    match sig.kind {
        CbKind::Enabled => format!("pub fn {}(t: &mut Tree, {}: El) -> R<bool>", sig.rust, p(0)),
        CbKind::IntFn => format!("pub fn {}(t: &mut Tree, {}: El) -> R<i32>", sig.rust, p(0)),
        CbKind::LinksTo => format!("pub fn {}(t: &mut Tree, {}: El) -> R<Option<El>>", sig.rust, p(0)),
        CbKind::Proc => format!("pub fn {}(t: &mut Tree, {}: El) -> R<()>", sig.rust, p(0)),
        CbKind::Value => format!(
            "pub fn {}(t: &mut Tree, {}: El, {}: &mut Variant) -> R<()>",
            sig.rust,
            p(0),
            p(1)
        ),
        CbKind::Text => format!(
            "pub fn {}(t: &mut Tree, {}: El, {}: &mut String) -> R<()>",
            sig.rust,
            p(0),
            p(1)
        ),
        CbKind::Count => format!(
            "pub fn {}(t: &mut Tree, {}: El, {}: &mut i32) -> R<()>",
            sig.rust,
            p(0),
            p(1)
        ),
        CbKind::AfterLoad => format!(
            "pub fn {}(t: &mut Tree, {}: El, {}: bool, {}: i32) -> R<()>",
            sig.rust,
            p(0),
            p(1),
            p(2)
        ),
    }
}

/// The routines of the hand file: every `pub fn` name.
fn hand_names(path: &Path) -> Result<HashSet<String>> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    Ok(text
        .lines()
        .filter_map(|line| {
            line.trim_start()
                .strip_prefix("pub fn ")?
                .split(['(', '<'])
                .next()
                .map(str::to_owned)
        })
        .collect())
}

/// The builders of a unit, by lower-case name, with their Rust names.
fn unit_builders(unit: &Unit, table: &mut HashMap<String, Vec<BuilderSig>>) -> Result<()> {
    let mut groups: BTreeMap<String, Vec<&Routine>> = BTreeMap::new();
    for decl in unit.interface.iter().chain(&unit.implementation) {
        if let Decl::Routine(routine) = decl
            && is_builder(routine)
        {
            let overloads = groups.entry(lower(&routine.name)).or_default();
            // The implementation repeats the interface header.
            let repeated = overloads.iter().any(|known| {
                known.params.len() == routine.params.len()
                    && known
                        .params
                        .iter()
                        .zip(&routine.params)
                        .all(|(a, b)| a.name.eq_ignore_ascii_case(&b.name) && a.type_ref == b.type_ref)
            });
            if !repeated {
                overloads.push(routine);
            }
        }
    }
    for (name, overloads) in groups {
        let base = snake(&overloads[0].name);
        let max = overloads.iter().map(|routine| routine.params.len()).max().unwrap_or(0);
        let mut taken = false;
        let mut sigs = Vec::new();
        for routine in &overloads {
            let params = routine
                .params
                .iter()
                .map(|param| Ok((param.name.clone(), pkind(param)?)))
                .collect::<Result<Vec<_>>>()
                .with_context(|| format!("{} (line {})", routine.name, routine.line))?;
            let rust = if routine.params.len() == max && !taken {
                taken = true;
                base.clone()
            } else {
                let words: Vec<String> = params
                    .iter()
                    .map(|(name, _)| snake(name.strip_prefix('a').unwrap_or(name)))
                    .collect();
                if words.is_empty() {
                    format!("{base}_0")
                } else {
                    format!("{base}_{}", words.join("_"))
                }
            };
            sigs.push(BuilderSig { rust, params });
        }
        table.insert(name, sigs);
    }
    Ok(())
}

/// The shape of an argument, for overload resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Str,
    DataType,
    EmptyList,
    List,
    Int,
    Bool,
    Def,
    FnRef,
    Nil,
    Unknown,
}

fn fits(shape: Shape, kind: PKind) -> bool {
    match shape {
        Shape::Str => matches!(kind, PKind::Str | PKind::Char),
        Shape::DataType => kind == PKind::DataType,
        Shape::EmptyList => matches!(kind, PKind::Events | PKind::Values | PKind::Defs),
        Shape::List => matches!(kind, PKind::Events | PKind::Values | PKind::Defs),
        Shape::Int => kind == PKind::Int,
        Shape::Bool => kind == PKind::Bool,
        Shape::Def => kind == PKind::Def,
        Shape::FnRef => kind == PKind::Decider,
        Shape::Nil => kind == PKind::Decider,
        Shape::Unknown => true,
    }
}

/// The local scope of a builder: parameter kinds by lower-case name and
/// the nested builders.
#[derive(Default, Clone)]
struct BScope {
    params: HashMap<String, PKind>,
    nested: HashMap<String, BuilderSig>,
}

struct Emitter {
    builders: HashMap<String, Vec<BuilderSig>>,
    callbacks: HashMap<String, CallbackSig>,
    /// Unit constants: lower-case name to Rust name and type.
    consts: HashMap<String, (String, T)>,
    /// The callbacks a definition refers to.
    referenced: RefCell<BTreeSet<String>>,
}

/// The Rust literal of a Pascal string.
fn rust_str(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            ch if (ch as u32) < 0x20 || (ch as u32) > 0x7e => {
                let _ = write!(out, "\\u{{{:x}}}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn int_literal(digits: &str, hex: bool) -> Result<i64> {
    if hex {
        u64::from_str_radix(digits, 16)
            .map(|value| value as i64)
            .map_err(|error| anyhow!("{error}"))
    } else {
        digits.parse::<i64>().map_err(|error| anyhow!("{error}"))
    }
}

fn data_type(name: &str) -> Option<&'static str> {
    Some(match lower(name).as_str() {
        "dtnone" => "DataType::None",
        "dtstruct" => "DataType::Struct",
        "dtarray" => "DataType::Array",
        "dtunion" => "DataType::Union",
        "dtmerge" => "DataType::Merge",
        "dtbytes" => "DataType::Bytes",
        "dtchars" => "DataType::Chars",
        "dtu8" => "DataType::U8",
        "dts8" => "DataType::S8",
        "dtu16" => "DataType::U16",
        "dts16" => "DataType::S16",
        "dtu32" => "DataType::U32",
        "dts32" => "DataType::S32",
        "dtu64" => "DataType::U64",
        "dts64" => "DataType::S64",
        "dtfloat16" => "DataType::Float16",
        "dtfloat32" => "DataType::Float32",
        _ => return None,
    })
}

fn event_variant(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match lower(name).as_str() {
        "df_oncreate" => ("Create", "on_create"),
        "df_ondestroy" => ("Destroy", "on_destroy"),
        "df_ongetenabled" => ("GetEnabled", "on_get_enabled"),
        "df_ondecide" => ("Decide", "on_decide"),
        "df_onafterload" => ("AfterLoad", "on_after_load"),
        "df_onbeforesave" => ("BeforeSave", "on_before_save"),
        "df_ongetvalue" => ("GetValue", "on_get_value"),
        "df_onsetvalue" => ("SetValue", "on_set_value"),
        "df_ongettext" => ("GetText", "on_get_text"),
        "df_onsettext" => ("SetText", "on_set_text"),
        "df_ongetcount" => ("GetCount", "on_get_count"),
        "df_onsetcount" => ("SetCount", "on_set_count"),
        "df_onlinksto" => ("LinksTo", "on_links_to"),
        _ => return None,
    })
}

/// The event field a property or a `SetOn*` method sets.
fn event_field(name: &str) -> Option<&'static str> {
    Some(match lower(name).as_str() {
        "oncreate" | "setoncreate" => "on_create",
        "ondestroy" | "setondestroy" => "on_destroy",
        "ongetenabled" | "setonenabled" => "on_get_enabled",
        "ondecide" | "setondecide" => "on_decide",
        "onafterload" | "setonafterload" => "on_after_load",
        "onbeforesave" | "setonbeforesave" => "on_before_save",
        "ongetvalue" | "setongetvalue" => "on_get_value",
        "onsetvalue" | "setonsetvalue" => "on_set_value",
        "ongettext" | "setongettext" => "on_get_text",
        "onsettext" | "setonsettext" => "on_set_text",
        "ongetcount" | "setongetcount" => "on_get_count",
        "onsetcount" | "setonsetcount" => "on_set_count",
        "onlinksto" | "setonlinksto" => "on_links_to",
        _ => return None,
    })
}

fn strip_paren(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(inner) => strip_paren(inner),
        other => other,
    }
}

impl Emitter {
    fn fn_ref(&self, expr: &Expr) -> Result<String> {
        let name = match strip_paren(expr) {
            Expr::Unary { op, operand } if op == "@" => match &**operand {
                Expr::Ident(name) => name.clone(),
                other => bail!("callback reference {other:?}"),
            },
            Expr::Ident(name) => name.clone(),
            other => bail!("callback reference {other:?}"),
        };
        let sig = self
            .callbacks
            .get(&lower(&name))
            .ok_or_else(|| anyhow!("unknown callback {name}"))?;
        self.referenced.borrow_mut().insert(lower(&name));
        Ok(sig.rust.clone())
    }

    fn shape(&self, expr: &Expr, scope: &BScope) -> Shape {
        match strip_paren(expr) {
            Expr::Str(_) => Shape::Str,
            Expr::Binary { op, left, .. } if op == "+" => match self.shape(left, scope) {
                Shape::Str => Shape::Str,
                Shape::Int => Shape::Int,
                other => other,
            },
            Expr::Binary { .. } => Shape::Int,
            Expr::Int { .. } => Shape::Int,
            Expr::Unary { op, .. } if op == "-" => Shape::Int,
            Expr::Unary { op, .. } if op == "@" => Shape::FnRef,
            Expr::Nil => Shape::Nil,
            Expr::List(items) if items.is_empty() => Shape::EmptyList,
            Expr::List(_) => Shape::List,
            Expr::Ident(name) => {
                let lower_name = lower(name);
                if data_type(name).is_some() {
                    Shape::DataType
                } else if lower_name == "true" || lower_name == "false" {
                    Shape::Bool
                } else if let Some(kind) = scope.params.get(&lower_name) {
                    match kind {
                        PKind::Str | PKind::Char => Shape::Str,
                        PKind::DataType => Shape::DataType,
                        PKind::Events | PKind::Values | PKind::Defs => Shape::List,
                        PKind::Int => Shape::Int,
                        PKind::Bool => Shape::Bool,
                        PKind::Decider => Shape::FnRef,
                        PKind::Def => Shape::Def,
                    }
                } else if let Some((_, ty)) = self.consts.get(&lower_name) {
                    match ty {
                        T::Str => Shape::Str,
                        _ => Shape::Int,
                    }
                } else if self.callbacks.contains_key(&lower_name) {
                    Shape::FnRef
                } else if self.builders.contains_key(&lower_name) || scope.nested.contains_key(&lower_name) {
                    Shape::Def
                } else {
                    Shape::Unknown
                }
            }
            Expr::Call { .. } | Expr::Member { .. } => Shape::Def,
            _ => Shape::Unknown,
        }
    }

    /// A string argument as a `&str` expression.
    fn str_arg(&self, expr: &Expr, scope: &BScope) -> Result<String> {
        let parts = self.str_parts(expr, scope)?;
        if parts.len() == 1 {
            return Ok(parts.into_iter().next().unwrap_or_default());
        }
        let placeholders = "{}".repeat(parts.len());
        Ok(format!("&format!(\"{placeholders}\", {})", parts.join(", ")))
    }

    fn str_parts(&self, expr: &Expr, scope: &BScope) -> Result<Vec<String>> {
        match strip_paren(expr) {
            Expr::Str(text) => Ok(vec![rust_str(text)]),
            Expr::Binary { op, left, right } if op == "+" => {
                let mut parts = self.str_parts(left, scope)?;
                parts.extend(self.str_parts(right, scope)?);
                Ok(parts)
            }
            Expr::Ident(name) => {
                let lower_name = lower(name);
                if scope.params.contains_key(&lower_name) {
                    Ok(vec![snake(name)])
                } else if let Some((rust, T::Str)) = self.consts.get(&lower_name) {
                    Ok(vec![rust.clone()])
                } else {
                    bail!("string {name}")
                }
            }
            other => bail!("string expression {other:?}"),
        }
    }

    fn int_arg(&self, expr: &Expr, scope: &BScope) -> Result<String> {
        match strip_paren(expr) {
            Expr::Int { digits, hex } => Ok(int_literal(digits, *hex)?.to_string()),
            Expr::Unary { op, operand } if op == "-" => Ok(format!("-{}", self.int_arg(operand, scope)?)),
            // `Int64(n)`, `Cardinal(n)`: a typecast of a literal.
            Expr::Call { callee, args }
                if args.len() == 1
                    && matches!(&**callee, Expr::Ident(name) if matches!(lower(name).as_str(), "int64" | "cardinal" | "integer")) =>
            {
                self.int_arg(&args[0], scope)
            }
            Expr::Ident(name) => {
                let lower_name = lower(name);
                if scope.params.contains_key(&lower_name) {
                    Ok(snake(name))
                } else if let Some((rust, _)) = self.consts.get(&lower_name) {
                    Ok(rust.clone())
                } else {
                    bail!("integer {name}")
                }
            }
            Expr::Binary { op, left, right } => {
                let op = match op.as_str() {
                    "+" => "+",
                    "-" => "-",
                    "*" => "*",
                    "shl" => "<<",
                    "shr" => ">>",
                    "or" => "|",
                    "and" => "&",
                    other => bail!("integer operator {other}"),
                };
                Ok(format!(
                    "({} {op} {})",
                    self.int_arg(left, scope)?,
                    self.int_arg(right, scope)?
                ))
            }
            other => bail!("integer expression {other:?}"),
        }
    }

    fn arg(&self, expr: &Expr, kind: PKind, scope: &BScope) -> Result<String> {
        let expr = strip_paren(expr);
        match kind {
            PKind::Str => self.str_arg(expr, scope),
            PKind::Char => match expr {
                Expr::Str(text) if text.chars().count() == 1 => {
                    Ok(format!("{}u8", text.chars().next().map_or(0, |ch| ch as u32)))
                }
                Expr::Ident(name) if scope.params.contains_key(&lower(name)) => Ok(snake(name)),
                other => bail!("character {other:?}"),
            },
            PKind::DataType => match expr {
                Expr::Ident(name) => match data_type(name) {
                    Some(rust) => Ok(rust.to_owned()),
                    None if scope.params.contains_key(&lower(name)) => Ok(snake(name)),
                    None => bail!("data type {name}"),
                },
                other => bail!("data type {other:?}"),
            },
            PKind::Int => Ok(format!("{} as i32", self.int_arg(expr, scope)?)),
            PKind::Bool => match expr {
                Expr::Ident(name) if lower(name) == "true" => Ok("true".to_owned()),
                Expr::Ident(name) if lower(name) == "false" => Ok("false".to_owned()),
                Expr::Ident(name) if scope.params.contains_key(&lower(name)) => Ok(snake(name)),
                other => bail!("boolean {other:?}"),
            },
            PKind::Decider => match expr {
                Expr::Nil => Ok("None".to_owned()),
                Expr::Ident(name) if scope.params.contains_key(&lower(name)) => Ok(snake(name)),
                other => Ok(format!("Some({})", self.fn_ref(other)?)),
            },
            PKind::Events => match expr {
                Expr::List(items) => {
                    if items.len() % 2 != 0 {
                        bail!("odd events list");
                    }
                    let mut out = Vec::new();
                    for pair in items.chunks(2) {
                        let Expr::Ident(event) = &pair[0] else {
                            bail!("event {:?}", pair[0]);
                        };
                        let (variant, _) = event_variant(event).ok_or_else(|| anyhow!("event {event}"))?;
                        out.push(format!("Event::{variant}({})", self.fn_ref(&pair[1])?));
                    }
                    Ok(format!("&[{}]", out.join(", ")))
                }
                Expr::Ident(name) if scope.params.contains_key(&lower(name)) => Ok(snake(name)),
                other => bail!("events {other:?}"),
            },
            PKind::Values => match expr {
                Expr::List(items) => {
                    if items.len() % 2 != 0 {
                        bail!("odd values list");
                    }
                    let mut out = Vec::new();
                    for pair in items.chunks(2) {
                        out.push(format!(
                            "({}, {})",
                            self.int_arg(&pair[0], scope)?,
                            self.str_arg(&pair[1], scope)?
                        ));
                    }
                    Ok(format!("&[{}]", out.join(", ")))
                }
                Expr::Ident(name) if scope.params.contains_key(&lower(name)) => Ok(snake(name)),
                other => bail!("values {other:?}"),
            },
            PKind::Defs => match expr {
                Expr::List(items) => {
                    let defs = items
                        .iter()
                        .map(|item| self.def_expr(item, scope))
                        .collect::<Result<Vec<_>>>()?;
                    Ok(format!("vec![{}]", defs.join(", ")))
                }
                Expr::Ident(name) if scope.params.contains_key(&lower(name)) => Ok(snake(name)),
                other => bail!("definitions {other:?}"),
            },
            PKind::Def => self.def_expr(expr, scope),
        }
    }

    /// The call of a `wbDataFormat` builder, completed to its full form.
    fn df_call(&self, name: &str, args: &[Expr], scope: &BScope) -> Result<Option<String>> {
        let shapes: Vec<Shape> = args.iter().map(|arg| self.shape(arg, scope)).collect();
        let a = |index: usize, kind: PKind| self.arg(&args[index], kind, scope);
        let lname = lower(name);
        let code = match (lname.as_str(), args.len()) {
            ("dfstruct", 3) => format!(
                "df_struct({}, {}, {})",
                a(0, PKind::Str)?,
                a(1, PKind::Defs)?,
                a(2, PKind::Events)?
            ),
            ("dfstruct", 2) => format!("df_struct({}, {}, &[])", a(0, PKind::Str)?, a(1, PKind::Defs)?),
            ("dfarray", 5) => format!(
                "df_array({}, {}, {}, {}, {})",
                a(0, PKind::Str)?,
                a(1, PKind::Def)?,
                a(2, PKind::Int)?,
                a(3, PKind::Str)?,
                a(4, PKind::Events)?
            ),
            ("dfarray", 4) => format!(
                "df_array({}, {}, {}, {}, &[])",
                a(0, PKind::Str)?,
                a(1, PKind::Def)?,
                a(2, PKind::Int)?,
                a(3, PKind::Str)?
            ),
            ("dfarray", 3) if shapes[2] == Shape::Str => format!(
                "df_array({}, {}, 0, {}, &[])",
                a(0, PKind::Str)?,
                a(1, PKind::Def)?,
                a(2, PKind::Str)?
            ),
            ("dfarray", 3) => format!(
                "df_array({}, {}, {}, \"\", &[])",
                a(0, PKind::Str)?,
                a(1, PKind::Def)?,
                a(2, PKind::Int)?
            ),
            ("dfunion", 3) => format!(
                "df_union({}, {}, {})",
                a(0, PKind::Decider)?,
                a(1, PKind::Defs)?,
                a(2, PKind::Events)?
            ),
            ("dfunion", 2) if matches!(shapes[0], Shape::FnRef | Shape::Nil) => {
                format!("df_union({}, {}, &[])", a(0, PKind::Decider)?, a(1, PKind::Defs)?)
            }
            ("dfunion", 2) => format!("df_union(None, {}, {})", a(0, PKind::Defs)?, a(1, PKind::Events)?),
            ("dfunion", 1) => format!("df_union(None, {}, &[])", a(0, PKind::Defs)?),
            ("dfvalueunion", 4) => format!(
                "df_value_union({}, {}, {}, {})",
                a(0, PKind::DataType)?,
                a(1, PKind::Decider)?,
                a(2, PKind::Defs)?,
                a(3, PKind::Events)?
            ),
            ("dfvalueunion", 3) => format!(
                "df_value_union({}, {}, {}, &[])",
                a(0, PKind::DataType)?,
                a(1, PKind::Decider)?,
                a(2, PKind::Defs)?
            ),
            ("dfmerge", 4) => format!(
                "df_merge({}, {}, {}, {})",
                a(0, PKind::Str)?,
                a(1, PKind::Defs)?,
                a(2, PKind::Str)?,
                a(3, PKind::Events)?
            ),
            ("dfmerge", 3) => format!(
                "df_merge({}, {}, \"\", {})",
                a(0, PKind::Str)?,
                a(1, PKind::Defs)?,
                a(2, PKind::Events)?
            ),
            ("dfmerge", 2) => format!("df_merge({}, {}, \"\", &[])", a(0, PKind::Str)?, a(1, PKind::Defs)?),
            ("dfinteger", 4) => format!(
                "df_integer({}, {}, {}, {})",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?,
                a(2, PKind::Str)?,
                a(3, PKind::Events)?
            ),
            ("dfinteger", 3) if shapes[2] == Shape::Str => format!(
                "df_integer({}, {}, {}, &[])",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?,
                a(2, PKind::Str)?
            ),
            ("dfinteger", 3) => format!(
                "df_integer({}, {}, \"\", {})",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?,
                a(2, PKind::Events)?
            ),
            ("dfinteger", 2) => format!(
                "df_integer({}, {}, \"\", &[])",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?
            ),
            ("dfhexinteger", 2) => format!("df_hex_integer({}, {})", a(0, PKind::Str)?, a(1, PKind::DataType)?),
            ("dfflags", 5) => format!(
                "df_flags({}, {}, {}, {}, {})",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?,
                a(2, PKind::Values)?,
                a(3, PKind::Str)?,
                a(4, PKind::Events)?
            ),
            ("dfflags", 3) => format!(
                "df_flags({}, {}, {}, \"\", &[])",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?,
                a(2, PKind::Values)?
            ),
            ("dfenum", 5) => format!(
                "df_enum({}, {}, {}, {}, {})",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?,
                a(2, PKind::Values)?,
                a(3, PKind::Str)?,
                a(4, PKind::Events)?
            ),
            ("dfenum", 4) => format!(
                "df_enum({}, {}, {}, {}, &[])",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?,
                a(2, PKind::Values)?,
                a(3, PKind::Str)?
            ),
            ("dfenum", 3) => format!(
                "df_enum({}, {}, {}, \"\", &[])",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?,
                a(2, PKind::Values)?
            ),
            ("dfbool", 4) => format!(
                "df_bool({}, {}, {}, {})",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?,
                a(2, PKind::Str)?,
                a(3, PKind::Events)?
            ),
            ("dfbool", 3) if shapes[2] == Shape::Str => format!(
                "df_bool({}, {}, {}, &[])",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?,
                a(2, PKind::Str)?
            ),
            // UPSTREAM-QUIRK: the overload with events passes none on.
            ("dfbool", 3) => {
                a(2, PKind::Events)?;
                format!(
                    "/* UPSTREAM-QUIRK: events dropped */ df_bool({}, {}, \"\", &[])",
                    a(0, PKind::Str)?,
                    a(1, PKind::DataType)?
                )
            }
            ("dfbool", 2) => format!("df_bool({}, {}, \"\", &[])", a(0, PKind::Str)?, a(1, PKind::DataType)?),
            ("dffloat", 4) => format!(
                "df_float({}, {}, {}, {})",
                a(0, PKind::Str)?,
                a(1, PKind::DataType)?,
                a(2, PKind::Str)?,
                a(3, PKind::Events)?
            ),
            ("dffloat", 3) => format!(
                "df_float({}, DataType::Float32, {}, {})",
                a(0, PKind::Str)?,
                a(1, PKind::Str)?,
                a(2, PKind::Events)?
            ),
            ("dffloat", 2) if shapes[1] == Shape::DataType => {
                format!("df_float({}, {}, \"\", &[])", a(0, PKind::Str)?, a(1, PKind::DataType)?)
            }
            ("dffloat", 2) if shapes[1] == Shape::Str => {
                format!(
                    "df_float({}, DataType::Float32, {}, &[])",
                    a(0, PKind::Str)?,
                    a(1, PKind::Str)?
                )
            }
            ("dffloat", 2) => format!(
                "df_float({}, DataType::Float32, \"\", {})",
                a(0, PKind::Str)?,
                a(1, PKind::Events)?
            ),
            ("dffloat", 1) => format!("df_float({}, DataType::Float32, \"\", &[])", a(0, PKind::Str)?),
            ("dfbytes", 3) => format!(
                "df_bytes({}, {}, {})",
                a(0, PKind::Str)?,
                a(1, PKind::Int)?,
                a(2, PKind::Events)?
            ),
            ("dfbytes", 2) => format!("df_bytes({}, {}, &[])", a(0, PKind::Str)?, a(1, PKind::Int)?),
            ("dfchars", 6) => format!(
                "df_chars({}, {}, {}, {}, {}, {})",
                a(0, PKind::Str)?,
                a(1, PKind::Int)?,
                a(2, PKind::Str)?,
                a(3, PKind::Char)?,
                a(4, PKind::Bool)?,
                a(5, PKind::Events)?
            ),
            _ if lname.starts_with("df") => bail!("{name} with {} arguments", args.len()),
            _ => return Ok(None),
        };
        Ok(Some(code))
    }

    /// A builder call resolved to an overload.
    fn builder_call(&self, name: &str, args: &[Expr], scope: &BScope) -> Result<String> {
        let lname = lower(name);
        let candidates: Vec<BuilderSig> = match scope.nested.get(&lname) {
            Some(sig) => vec![sig.clone()],
            None => self
                .builders
                .get(&lname)
                .cloned()
                .ok_or_else(|| anyhow!("unknown function {name}"))?,
        };
        let shapes: Vec<Shape> = args.iter().map(|arg| self.shape(arg, scope)).collect();
        let sig = candidates
            .iter()
            .find(|sig| {
                sig.params.len() == args.len()
                    && sig
                        .params
                        .iter()
                        .zip(&shapes)
                        .all(|((_, kind), shape)| fits(*shape, *kind))
            })
            .ok_or_else(|| anyhow!("no overload of {name} for {shapes:?}"))?;
        let args = sig
            .params
            .iter()
            .zip(args)
            .map(|((_, kind), arg)| self.arg(arg, *kind, scope))
            .collect::<Result<Vec<_>>>()?;
        Ok(format!("{}({})", sig.rust, args.join(", ")))
    }

    /// An expression that builds a definition.
    fn def_expr(&self, expr: &Expr, scope: &BScope) -> Result<String> {
        match strip_paren(expr) {
            Expr::Call { callee, args } => match &**callee {
                Expr::Ident(name) => match self.df_call(name, args, scope)? {
                    Some(code) => Ok(code),
                    None => self.builder_call(name, args, scope),
                },
                Expr::Member { base, name } => {
                    // A constructor: `TdfStructDef.Create(aName, dtStruct, aDefs)`.
                    if let Expr::Ident(class) = &**base
                        && name.eq_ignore_ascii_case("Create")
                    {
                        let kind = match lower(class).as_str() {
                            "tdfstructdef" => "DefKind::Struct",
                            "twbnifblockdef" => "DefKind::NifBlock",
                            "twbnirefdef" => "DefKind::NiRef { template: String::new(), ptr: false }",
                            "tdfintegerdef" => "DefKind::Integer",
                            "tdffloatdef" => "DefKind::Float",
                            other => bail!("constructor of {other}"),
                        };
                        if args.len() != 3 {
                            bail!("constructor with {} arguments", args.len());
                        }
                        return Ok(format!(
                            "Def::new({kind}, {}, {}, {})",
                            self.arg(&args[0], PKind::Str, scope)?,
                            self.arg(&args[1], PKind::DataType, scope)?,
                            self.arg(&args[2], PKind::Defs, scope)?
                        ));
                    }
                    let base = self.def_expr(base, scope)?;
                    if name.eq_ignore_ascii_case("SetDefaultValue") {
                        return Ok(format!(
                            "{base}.set_default_value({})",
                            self.arg(&args[0], PKind::Str, scope)?
                        ));
                    }
                    let field = event_field(name).ok_or_else(|| anyhow!("method {name}"))?;
                    let method = match field {
                        "on_get_enabled" => "set_on_enabled".to_owned(),
                        other => format!("set_{other}"),
                    };
                    Ok(format!("{base}.{method}({})", self.fn_ref(&args[0])?))
                }
                other => bail!("call of {other:?}"),
            },
            Expr::Ident(name) => {
                let lname = lower(name);
                if scope.params.get(&lname) == Some(&PKind::Def) {
                    Ok(snake(name))
                } else if self.builders.contains_key(&lname) || scope.nested.contains_key(&lname) {
                    self.builder_call(name, &[], scope)
                } else {
                    bail!("definition {name}")
                }
            }
            other => bail!("definition expression {other:?}"),
        }
    }

    /// The statements of a builder.
    fn builder_statements(&self, statements: &[Stmt], scope: &BScope, depth: usize, out: &mut String) -> Result<()> {
        for statement in statements {
            self.builder_statement(statement, scope, depth, out)?;
        }
        Ok(())
    }

    fn builder_statement(&self, statement: &Stmt, scope: &BScope, depth: usize, out: &mut String) -> Result<()> {
        let pad = "    ".repeat(depth);
        match statement {
            Stmt::Empty => Ok(()),
            Stmt::Block(inner) => self.builder_statements(inner, scope, depth, out),
            Stmt::Assign { target, value, .. } => match target {
                Expr::Ident(name) if name.eq_ignore_ascii_case("Result") => {
                    writeln!(out, "{pad}result = {};", self.def_expr(value, scope)?)?;
                    Ok(())
                }
                Expr::Member { base, name } => {
                    let is_result = match &**base {
                        Expr::Ident(base) => base.eq_ignore_ascii_case("Result"),
                        Expr::Call { callee, args } => {
                            matches!(&**callee, Expr::Ident(_))
                                && args.len() == 1
                                && matches!(&args[0], Expr::Ident(arg) if arg.eq_ignore_ascii_case("Result"))
                        }
                        _ => false,
                    };
                    if !is_result {
                        bail!("assignment to {target:?}");
                    }
                    let lname = lower(name);
                    if let Some(field) = event_field(&lname) {
                        writeln!(out, "{pad}result.events.{field} = Some({});", self.fn_ref(value)?)?;
                    } else if lname == "delimiter" {
                        writeln!(
                            out,
                            "{pad}result.set_delimiter({});",
                            self.arg(value, PKind::Str, scope)?
                        )?;
                    } else if lname == "ptr" {
                        writeln!(out, "{pad}result.set_ptr({});", self.arg(value, PKind::Bool, scope)?)?;
                    } else if lname == "template" {
                        writeln!(
                            out,
                            "{pad}result.set_template({});",
                            self.arg(value, PKind::Str, scope)?
                        )?;
                    } else if lname == "defaultvalue" {
                        writeln!(
                            out,
                            "{pad}result.default_value = ({}).to_owned();",
                            self.arg(value, PKind::Str, scope)?
                        )?;
                    } else if lname == "size" {
                        writeln!(out, "{pad}result.size = {};", self.arg(value, PKind::Int, scope)?)?;
                    } else {
                        bail!("property {name}");
                    }
                    Ok(())
                }
                other => bail!("assignment to {other:?}"),
            },
            Stmt::Expr { expr, .. } => match expr {
                Expr::Call { callee, args } => match &**callee {
                    Expr::Member { base, name }
                        if matches!(&**base, Expr::Ident(base) if base.eq_ignore_ascii_case("Result"))
                            && name.eq_ignore_ascii_case("AssignEvents") =>
                    {
                        writeln!(
                            out,
                            "{pad}result.assign_events({});",
                            self.arg(&args[0], PKind::Events, scope)?
                        )?;
                        Ok(())
                    }
                    _ => bail!("statement {expr:?}"),
                },
                _ => bail!("statement {expr:?}"),
            },
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                writeln!(out, "{pad}if {} {{", self.builder_condition(condition, scope)?)?;
                self.builder_statement(then_branch, scope, depth + 1, out)?;
                match else_branch {
                    Some(else_branch) => {
                        writeln!(out, "{pad}}} else {{")?;
                        self.builder_statement(else_branch, scope, depth + 1, out)?;
                        writeln!(out, "{pad}}}")?;
                    }
                    None => writeln!(out, "{pad}}}")?,
                }
                Ok(())
            }
            Stmt::Raise(Some(expr)) => {
                // A definition error is a programming error.
                let message = match expr {
                    Expr::Call { args, .. } if args.len() == 1 => self.str_arg(&args[0], scope)?,
                    _ => "\"exception\"".to_owned(),
                };
                writeln!(out, "{pad}panic!(\"{{}}\", {message});")?;
                Ok(())
            }
            other => bail!("statement {other:?}"),
        }
    }

    fn builder_condition(&self, condition: &Expr, scope: &BScope) -> Result<String> {
        match strip_paren(condition) {
            Expr::Call { callee, args } if matches!(&**callee, Expr::Ident(name) if lower(name) == "sametext") => {
                Ok(format!(
                    "same_text({}, {})",
                    self.str_arg(&args[0], scope)?,
                    self.str_arg(&args[1], scope)?
                ))
            }
            Expr::Binary { op, left, right } if op == "=" || op == "<>" => {
                let rust = if op == "=" { "==" } else { "!=" };
                Ok(format!(
                    "{} {rust} {}",
                    self.str_arg(left, scope)?,
                    self.str_arg(right, scope)?
                ))
            }
            other => bail!("condition {other:?}"),
        }
    }

    /// A builder function and its nested builders.
    fn builder(&self, routine: &Routine, sig: &BuilderSig, depth: usize) -> Result<String> {
        let mut scope = BScope::default();
        for (name, kind) in &sig.params {
            scope.params.insert(lower(name), *kind);
        }
        let pad = "    ".repeat(depth);
        let mut out = String::new();
        let params: Vec<String> = sig
            .params
            .iter()
            .map(|(name, kind)| format!("{}: {}", snake(name), rust_param_type(*kind)))
            .collect();
        let visibility = if depth == 0 { "pub " } else { "" };
        writeln!(out, "{pad}/// Upstream `{}` (line {}).", routine.name, routine.line)?;
        writeln!(out, "{pad}{visibility}fn {}({}) -> Def {{", sig.rust, params.join(", "))?;
        let body = routine.body.as_ref().ok_or_else(|| anyhow!("no body"))?;
        for decl in &body.decls {
            match decl {
                Decl::Routine(nested) if is_builder(nested) => {
                    let params = nested
                        .params
                        .iter()
                        .map(|param| Ok((param.name.clone(), pkind(param)?)))
                        .collect::<Result<Vec<_>>>()?;
                    let nested_sig = BuilderSig {
                        rust: snake(&nested.name),
                        params,
                    };
                    scope.nested.insert(lower(&nested.name), nested_sig.clone());
                    out.push_str(&self.builder(nested, &nested_sig, depth + 1)?);
                }
                other => bail!("declaration {other:?}"),
            }
        }
        writeln!(out, "{pad}    let mut result: Def;")?;
        self.builder_statements(&body.statements, &scope, depth + 1, &mut out)?;
        writeln!(out, "{pad}    result")?;
        writeln!(out, "{pad}}}")?;
        Ok(out)
    }

    /// A `wbDefine*` procedure.
    fn define(&self, routine: &Routine) -> Result<String> {
        let scope = BScope::default();
        let mut out = String::new();
        writeln!(out, "/// Upstream `{}` (line {}).", routine.name, routine.line)?;
        writeln!(
            out,
            "pub fn {}(infos: &mut NiObjectInfos) -> R<()> {{",
            snake(&routine.name)
        )?;
        let body = routine.body.as_ref().ok_or_else(|| anyhow!("no body"))?;
        if !body.decls.is_empty() {
            bail!("declarations in {}", routine.name);
        }
        for statement in &body.statements {
            match statement {
                // The version variables are constants in Rust.
                Stmt::Assign {
                    target: Expr::Ident(name),
                    ..
                } if self.consts.contains_key(&lower(name)) => {}
                Stmt::Expr { expr, .. } => match expr {
                    Expr::Ident(name) if lower(name).starts_with("wbdefine") => {
                        writeln!(out, "    {}(infos)?;", snake(name))?;
                    }
                    Expr::Call { callee, args } if matches!(&**callee, Expr::Ident(name) if lower(name) == "wbniobject") =>
                    {
                        let def = self.def_expr(&args[0], &scope)?;
                        let (inherit, is_abstract, index) = match args.len() {
                            1 => ("\"\"".to_owned(), "false".to_owned(), "0".to_owned()),
                            3 => (
                                self.arg(&args[1], PKind::Str, &scope)?,
                                self.arg(&args[2], PKind::Bool, &scope)?,
                                "0".to_owned(),
                            ),
                            4 => (
                                self.arg(&args[1], PKind::Str, &scope)?,
                                self.arg(&args[2], PKind::Bool, &scope)?,
                                self.int_arg(&args[3], &scope)?,
                            ),
                            count => bail!("wbNiObject with {count} arguments"),
                        };
                        writeln!(
                            out,
                            "    wb_ni_object(infos, {def}, {inherit}, {is_abstract}, {index})?;"
                        )?;
                    }
                    other => bail!("statement {other:?}"),
                },
                Stmt::Empty => {}
                other => bail!("statement {other:?}"),
            }
        }
        writeln!(out, "    Ok(())")?;
        writeln!(out, "}}")?;
        Ok(out)
    }
}

// ---- callbacks ----

/// A typed Rust expression.
struct V {
    code: String,
    ty: T,
}

impl V {
    fn new(code: impl Into<String>, ty: T) -> V {
        V { code: code.into(), ty }
    }
}

struct CScope<'a> {
    sig: &'a CallbackSig,
    /// Local variables by lower-case name: Rust name and type.
    locals: HashMap<String, (String, T)>,
    /// Inside `with nif(e) do`.
    with_nif: bool,
}

impl CScope<'_> {
    fn element_param(&self) -> String {
        lower(&self.sig.params[0])
    }
}

/// `func(args)suffix`, with the arguments bound first when they use the
/// tree, which a method of the tree borrows mutably for the call.
fn tcall(func: &str, args: &[String], suffix: &str) -> String {
    let uses_tree = args
        .iter()
        .any(|arg| arg.contains("t.") || arg.contains("(t,") || arg.contains("(t)"));
    if !uses_tree {
        return format!("{func}({}){suffix}", args.join(", "));
    }
    let mut out = String::from("{ ");
    let mut names = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        if arg == "t" || arg.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            names.push(arg.clone());
        } else {
            let _ = write!(out, "let a{index} = {arg}; ");
            names.push(format!("a{index}"));
        }
    }
    let _ = write!(out, "{func}({}){suffix} }}", names.join(", "));
    out
}

fn cmp_op(op: &str) -> Option<&'static str> {
    Some(match op {
        "=" => "==",
        "<>" => "!=",
        "<" => "<",
        ">" => ">",
        "<=" => "<=",
        ">=" => ">=",
        _ => return None,
    })
}

fn flip(op: &str) -> &'static str {
    match op {
        "<" => ">",
        ">" => "<",
        "<=" => ">=",
        ">=" => "<=",
        "==" => "==",
        _ => "!=",
    }
}

fn ordering_method(op: &str) -> &'static str {
    match op {
        "==" => "is_eq",
        "!=" => "is_ne",
        "<" => "is_lt",
        ">" => "is_gt",
        "<=" => "is_le",
        _ => "is_ge",
    }
}

fn nif_version_const(name: &str) -> Option<&'static str> {
    Some(match lower(name).as_str() {
        "nfunknown" => "NifVersion::Unknown",
        "nftes3" => "NifVersion::Tes3",
        "nftes4" => "NifVersion::Tes4",
        "nffo3" => "NifVersion::Fo3",
        "nftes5" => "NifVersion::Tes5",
        "nfsse" => "NifVersion::Sse",
        "nffo4" => "NifVersion::Fo4",
        _ => return None,
    })
}

impl Emitter {
    /// Converts a value to `ty`.
    fn conv(&self, value: V, ty: T) -> Result<String> {
        Ok(match (value.ty, ty) {
            (a, b) if a == b => value.code,
            (T::Var, T::Int) => format!("({}).to_i64()?", value.code),
            (T::Var, T::Float) => format!("({}).to_f64()?", value.code),
            (T::Var, T::Bool) => format!("({}).to_bool()?", value.code),
            (T::Var, T::Str) => format!("({}).to_str()?", value.code),
            (T::Int, T::Var) => format!("Variant::Int({})", value.code),
            (T::Float, T::Var) => format!("Variant::Float({})", value.code),
            (T::Bool, T::Var) => format!("Variant::Bool({})", value.code),
            (T::Str, T::Var) => format!("Variant::Str(({}).to_string())", value.code),
            (T::Int, T::Float) => format!("({}) as f64", value.code),
            (T::El, T::OptEl) => format!("Some({})", value.code),
            (T::OptEl, T::El) => format!("req({})?", value.code),
            (from, to) => bail!("conversion from {from:?} to {to:?}"),
        })
    }

    /// An element expression.
    fn el(&self, expr: &Expr, scope: &CScope) -> Result<String> {
        let value = self.cexpr(expr, scope)?;
        self.conv(value, T::El)
    }

    fn path_arg(&self, args: &[Expr], scope: &CScope) -> Result<String> {
        if args.len() != 1 {
            bail!("path with {} arguments", args.len());
        }
        let value = self.cexpr(&args[0], scope)?;
        match value.ty {
            T::Str => Ok(format!("&{}", value.code)),
            other => bail!("path of type {other:?}"),
        }
    }

    fn cexpr(&self, expr: &Expr, scope: &CScope) -> Result<V> {
        match expr {
            Expr::Paren(inner) => {
                let inner = self.cexpr(inner, scope)?;
                Ok(V::new(format!("({})", inner.code), inner.ty))
            }
            Expr::Int { digits, hex } => Ok(V::new(format!("{}i64", int_literal(digits, *hex)?), T::Int)),
            Expr::Float(text) => Ok(V::new(format!("{text}f64"), T::Float)),
            Expr::Str(text) => Ok(V::new(rust_str(text), T::Str)),
            Expr::Nil => Ok(V::new("None", T::OptEl)),
            Expr::Ident(name) => self.ident(name, scope),
            Expr::Unary { op, operand } => {
                let value = self.cexpr(operand, scope)?;
                match (op.as_str(), value.ty) {
                    ("not", T::Bool) => Ok(V::new(format!("!{}", value.code), T::Bool)),
                    ("not", T::Int) => Ok(V::new(format!("!{}", value.code), T::Int)),
                    ("-", T::Int) => Ok(V::new(format!("-{}", value.code), T::Int)),
                    ("-", T::Float) => Ok(V::new(format!("-{}", value.code), T::Float)),
                    other => bail!("unary {other:?}"),
                }
            }
            Expr::Binary { op, left, right } => self.binary(op, left, right, scope),
            Expr::Index { base, args } => self.index(base, args, scope),
            Expr::Member { base, name } => self.member(base, name, &[], scope),
            Expr::Call { callee, args } => match &**callee {
                Expr::Member { base, name } => self.member(base, name, args, scope),
                Expr::Ident(name) => self.call(name, args, scope),
                other => bail!("call of {other:?}"),
            },
            other => bail!("expression {other:?}"),
        }
    }

    fn ident(&self, name: &str, scope: &CScope) -> Result<V> {
        let lname = lower(name);
        let sig = scope.sig;
        if lname == "result" {
            return match sig.kind {
                CbKind::Enabled => Ok(V::new("result", T::Bool)),
                CbKind::IntFn => Ok(V::new("result", T::Int)),
                CbKind::LinksTo => Ok(V::new("result", T::OptEl)),
                _ => bail!("Result in a procedure"),
            };
        }
        if let Some((rust, ty)) = scope.locals.get(&lname) {
            return Ok(V::new(rust.clone(), *ty));
        }
        if lname == scope.element_param() {
            return Ok(V::new(snake(name), T::El));
        }
        if let Some(param) = sig.params.get(1)
            && lname == lower(param)
        {
            return match sig.kind {
                CbKind::Value => Ok(V::new(format!("(*{})", snake(name)), T::Var)),
                CbKind::Text => Ok(V::new(format!("{}.as_str()", snake(name)), T::Str)),
                CbKind::Count => Ok(V::new(format!("i64::from(*{})", snake(name)), T::Int)),
                CbKind::AfterLoad => Ok(V::new(snake(name), T::Ptr)),
                _ => bail!("parameter {name}"),
            };
        }
        if sig.kind == CbKind::AfterLoad
            && let Some(param) = sig.params.get(2)
            && lname == lower(param)
        {
            return Ok(V::new(format!("i64::from({})", snake(name)), T::Int));
        }
        if scope.with_nif {
            match lname.as_str() {
                "version" => return Ok(V::new("nif_version(t)", T::Int)),
                "userversion" => return Ok(V::new("nif_user_version(t)", T::Int)),
                "userversion2" => return Ok(V::new("nif_user_version2(t)", T::Int)),
                "nifversion" => return Ok(V::new("t.nif.nif_version", T::NifEnum)),
                _ => {}
            }
        }
        if lname == "true" || lname == "false" {
            return Ok(V::new(lname, T::Bool));
        }
        if lname == "nil" {
            return Ok(V::new("None", T::OptEl));
        }
        if let Some(rust) = nif_version_const(name) {
            return Ok(V::new(rust, T::NifEnum));
        }
        if let Some((rust, ty)) = self.consts.get(&lname) {
            return Ok(V::new(rust.clone(), *ty));
        }
        // A function called without parentheses.
        if self.callbacks.contains_key(&lname) {
            return self.call(name, &[], scope);
        }
        bail!("identifier {name}")
    }

    fn binary(&self, op: &str, left: &Expr, right: &Expr, scope: &CScope) -> Result<V> {
        let op = op.to_ascii_lowercase();
        if op == "in" {
            let value = self.cexpr(left, scope)?;
            let Expr::List(items) = strip_paren(right) else {
                bail!("in {right:?}");
            };
            let items = items
                .iter()
                .map(|item| {
                    let item = self.cexpr(item, scope)?;
                    self.conv(item, T::Int)
                })
                .collect::<Result<Vec<_>>>()?;
            let value = self.conv(value, T::Int)?;
            return Ok(V::new(format!("[{}].contains(&({value}))", items.join(", ")), T::Bool));
        }
        let mut l = self.cexpr(left, scope)?;
        let mut r = self.cexpr(right, scope)?;
        if let Some(cmp) = cmp_op(&op) {
            let code = match (l.ty, r.ty) {
                (T::Ptr, T::OptEl) if r.code == "None" && cmp == "==" => format!("!{}", l.code),
                (T::Ptr, T::OptEl) if r.code == "None" && cmp == "!=" => l.code.clone(),
                (T::Int, T::Int) | (T::Float, T::Float) | (T::NifEnum, T::NifEnum) | (T::Bool, T::Bool) => {
                    format!("{} {cmp} {}", l.code, r.code)
                }
                (T::Int, T::Float) => format!("({}) as f64 {cmp} {}", l.code, r.code),
                (T::Float, T::Int) => format!("{} {cmp} ({}) as f64", l.code, r.code),
                (T::Var, T::Int) => format!("{} {cmp} {}", l.code, r.code),
                (T::Int, T::Var) => format!("{} {} {}", r.code, flip(cmp), l.code),
                (T::Var, T::Var) => format!("({}).compare(&{})?.{}()", l.code, r.code, ordering_method(cmp)),
                (T::Str, T::Str) if cmp == "==" || cmp == "!=" => format!("{} {cmp} {}", l.code, r.code),
                (T::OptEl, T::OptEl) | (T::El, T::OptEl) | (T::OptEl, T::El) if cmp == "==" || cmp == "!=" => {
                    let l = self.conv(l, T::OptEl)?;
                    let r = self.conv(r, T::OptEl)?;
                    format!("{l} {cmp} {r}")
                }
                (a, b) => bail!("comparison of {a:?} and {b:?}"),
            };
            return Ok(V::new(format!("({code})"), T::Bool));
        }
        // A variant meets an integer in integer arithmetic: the native
        // values these callbacks read are integers.
        if matches!(
            op.as_str(),
            "and" | "or" | "+" | "-" | "*" | "div" | "mod" | "shl" | "shr"
        ) && (l.ty == T::Var || r.ty == T::Var)
            && (l.ty == T::Int || r.ty == T::Int || (l.ty == T::Var && r.ty == T::Var))
        {
            if l.ty == T::Var {
                l = V::new(self.conv(l, T::Int)?, T::Int);
            }
            if r.ty == T::Var {
                r = V::new(self.conv(r, T::Int)?, T::Int);
            }
        }
        match (op.as_str(), l.ty, r.ty) {
            ("and", T::Bool, T::Bool) => Ok(V::new(format!("({} && {})", l.code, r.code), T::Bool)),
            ("or", T::Bool, T::Bool) => Ok(V::new(format!("({} || {})", l.code, r.code), T::Bool)),
            ("and", T::Int, T::Int) => Ok(V::new(format!("({} & {})", l.code, r.code), T::Int)),
            ("or", T::Int, T::Int) => Ok(V::new(format!("({} | {})", l.code, r.code), T::Int)),
            ("+", T::Str, T::Str) => Ok(V::new(format!("format!(\"{{}}{{}}\", {}, {})", l.code, r.code), T::Str)),
            ("+" | "-" | "*", T::Int, T::Int) => Ok(V::new(format!("({} {op} {})", l.code, r.code), T::Int)),
            ("div", T::Int, T::Int) => Ok(V::new(format!("({} / {})", l.code, r.code), T::Int)),
            ("mod", T::Int, T::Int) => Ok(V::new(format!("({} % {})", l.code, r.code), T::Int)),
            ("shl", T::Int, T::Int) => Ok(V::new(format!("({} << {})", l.code, r.code), T::Int)),
            ("shr", T::Int, T::Int) => Ok(V::new(format!("({} >> {})", l.code, r.code), T::Int)),
            (op, a, b) => bail!("operator {op} on {a:?} and {b:?}"),
        }
    }

    fn index(&self, base: &Expr, args: &[Expr], scope: &CScope) -> Result<V> {
        // `x.NativeValues['p']` and the other indexed properties.
        if let Expr::Member { base: owner, name } = base {
            let owner = self.el(owner, scope)?;
            let path = self.path_arg(args, scope)?;
            return match lower(name).as_str() {
                "nativevalues" => Ok(V::new(tcall("t.native_values", &[owner, path], "?"), T::Var)),
                "editvalues" => Ok(V::new(tcall("t.edit_values", &[owner, path], "?"), T::Str)),
                "elements" => Ok(V::new(tcall("t.elements", &[owner, path], "?"), T::OptEl)),
                other => bail!("indexed property {other}"),
            };
        }
        // `x[i]`
        let element = self.el(base, scope)?;
        if args.len() != 1 {
            bail!("index with {} arguments", args.len());
        }
        let index = self.cexpr(&args[0], scope)?;
        let index = self.conv(index, T::Int)?;
        Ok(V::new(
            tcall("t.item", &[element, format!("({index}) as i32")], "?"),
            T::El,
        ))
    }

    fn member(&self, base: &Expr, name: &str, args: &[Expr], scope: &CScope) -> Result<V> {
        let lname = lower(name);
        // `nif(e).X`
        if let Expr::Call { callee, .. } = strip_paren(base)
            && matches!(&**callee, Expr::Ident(callee) if lower(callee) == "nif")
        {
            return match lname.as_str() {
                "version" => Ok(V::new("nif_version(t)", T::Int)),
                "userversion" => Ok(V::new("nif_user_version(t)", T::Int)),
                "userversion2" => Ok(V::new("nif_user_version2(t)", T::Int)),
                "nifversion" => Ok(V::new("t.nif.nif_version", T::NifEnum)),
                other => bail!("nif member {other}"),
            };
        }
        if !args.is_empty() {
            bail!("method {name}");
        }
        let owner = self.el(base, scope)?;
        match lname.as_str() {
            "nativevalue" => Ok(V::new(tcall("t.native_value", &[owner], "?"), T::Var)),
            "editvalue" => Ok(V::new(tcall("t.edit_value", &[owner], "?"), T::Str)),
            "index" => Ok(V::new(
                format!("i64::from({})", tcall("t.index", &[owner], "?")),
                T::Int,
            )),
            "count" => Ok(V::new(format!("i64::from({})", tcall("t.count", &[owner], "")), T::Int)),
            "datasize" => Ok(V::new(
                format!("i64::from({})", tcall("t.data_size", &[owner], "?")),
                T::Int,
            )),
            "userdata" => Ok(V::new(
                format!("i64::from({})", tcall("t.user_data", &[owner], "")),
                T::Int,
            )),
            "parent" => Ok(V::new(tcall("t.parent", &[owner], ""), T::OptEl)),
            "linksto" => Ok(V::new(tcall("t.links_to", &[owner], "?"), T::OptEl)),
            other => bail!("member {other}"),
        }
    }

    fn call(&self, name: &str, args: &[Expr], scope: &CScope) -> Result<V> {
        let lname = lower(name);
        match lname.as_str() {
            "nifblk" => {
                let element = self.el(&args[0], scope)?;
                return Ok(V::new(tcall("nifblk_r", &["t".to_owned(), element], "?"), T::El));
            }
            "nif" => return Ok(V::new("", T::Nif)),
            "cardinal" | "integer" | "word" | "int64" | "byte" => {
                let value = self.cexpr(&args[0], scope)?;
                let value = self.conv(value, T::Int)?;
                let cast = match lname.as_str() {
                    "cardinal" => "u32",
                    "integer" => "i32",
                    "word" => "u16",
                    "byte" => "u8",
                    _ => "i64",
                };
                return Ok(V::new(format!("i64::from(({value}) as {cast})"), T::Int));
            }
            "inttostr" => {
                let value = self.cexpr(&args[0], scope)?;
                let value = self.conv(value, T::Int)?;
                return Ok(V::new(format!("({value}).to_string()"), T::Str));
            }
            "assigned" => {
                let value = self.cexpr(&args[0], scope)?;
                let value = self.conv(value, T::OptEl)?;
                return Ok(V::new(format!("({value}).is_some()"), T::Bool));
            }
            "sametext" => {
                let a = self.cexpr(&args[0], scope)?;
                let b = self.cexpr(&args[1], scope)?;
                if a.ty != T::Str || b.ty != T::Str {
                    bail!("SameText of {:?} and {:?}", a.ty, b.ty);
                }
                return Ok(V::new(format!("same_text(&{}, &{})", a.code, b.code), T::Bool));
            }
            "wbisniobject" => {
                let element = self.el(&args[0], scope)?;
                let template = self.cexpr(&args[1], scope)?;
                if template.ty != T::Str {
                    bail!("wbIsNiObject template");
                }
                return Ok(V::new(
                    tcall(
                        "wb_is_ni_object_el",
                        &["t".to_owned(), element, format!("&{}", template.code)],
                        "",
                    ),
                    T::Bool,
                ));
            }
            "wbinttonifversion" => {
                let value = self.cexpr(&args[0], scope)?;
                let value = self.conv(value, T::Int)?;
                return Ok(V::new(format!("wb_int_to_nif_version(({value}) as u32)"), T::Str));
            }
            "wbnifversiontoint" => {
                let value = self.cexpr(&args[0], scope)?;
                if value.ty != T::Str {
                    bail!("wbNifVersionToInt of {:?}", value.ty);
                }
                return Ok(V::new(
                    format!("i64::from(wb_nif_version_to_int(&{}))", value.code),
                    T::Int,
                ));
            }
            _ => {}
        }
        let sig = self
            .callbacks
            .get(&lname)
            .ok_or_else(|| anyhow!("unknown function {name}"))?;
        if args.len() != 1 {
            bail!("call of {name} with {} arguments", args.len());
        }
        let element = self.el(&args[0], scope)?;
        let ty = match sig.kind {
            CbKind::Enabled => T::Bool,
            CbKind::IntFn => T::Int,
            CbKind::LinksTo => T::OptEl,
            _ => bail!("call of procedure {name} as a function"),
        };
        let code = match ty {
            T::Int => format!("i64::from({})", tcall(&sig.rust, &["t".to_owned(), element], "?")),
            _ => tcall(&sig.rust, &["t".to_owned(), element], "?"),
        };
        Ok(V::new(code, ty))
    }

    fn cstatements(&self, statements: &[Stmt], scope: &mut CScope, depth: usize, out: &mut String) -> Result<()> {
        for statement in statements {
            self.cstatement(statement, scope, depth, out)?;
        }
        Ok(())
    }

    fn exit_code(&self, scope: &CScope) -> &'static str {
        match scope.sig.kind {
            CbKind::Enabled | CbKind::LinksTo => "return Ok(result);",
            CbKind::IntFn => "return Ok(result as i32);",
            _ => "return Ok(());",
        }
    }

    fn cstatement(&self, statement: &Stmt, scope: &mut CScope, depth: usize, out: &mut String) -> Result<()> {
        let pad = "    ".repeat(depth);
        match statement {
            Stmt::Empty => {}
            Stmt::Block(inner) => self.cstatements(inner, scope, depth, out)?,
            Stmt::With { targets, body } => {
                let is_nif = targets.len() == 1
                    && matches!(&targets[0], Expr::Call { callee, .. } if matches!(&**callee, Expr::Ident(name) if lower(name) == "nif"));
                if !is_nif {
                    bail!("with {targets:?}");
                }
                let outer = scope.with_nif;
                scope.with_nif = true;
                self.cstatement(body, scope, depth, out)?;
                scope.with_nif = outer;
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let condition = self.cexpr(condition, scope)?;
                let condition = self.conv(condition, T::Bool)?;
                writeln!(out, "{pad}if {condition} {{")?;
                self.cstatement(then_branch, scope, depth + 1, out)?;
                match else_branch {
                    Some(else_branch) => {
                        writeln!(out, "{pad}}} else {{")?;
                        self.cstatement(else_branch, scope, depth + 1, out)?;
                        writeln!(out, "{pad}}}")?;
                    }
                    None => writeln!(out, "{pad}}}")?,
                }
            }
            Stmt::Var(var) => self.local(var, scope, depth, out)?,
            Stmt::For {
                variable,
                from,
                to,
                down,
                body,
            } => {
                let from = self.cexpr(from, scope)?;
                let from = self.conv(from, T::Int)?;
                let to = self.cexpr(to, scope)?;
                let to = self.conv(to, T::Int)?;
                let rust = snake(variable);
                let range = if *down {
                    format!("(({to})..=({from})).rev()")
                } else {
                    format!("({from})..=({to})")
                };
                writeln!(out, "{pad}for {rust} in {range} {{")?;
                let outer = scope.locals.insert(lower(variable), (rust, T::Int));
                self.cstatement(body, scope, depth + 1, out)?;
                match outer {
                    Some(outer) => scope.locals.insert(lower(variable), outer),
                    None => scope.locals.remove(&lower(variable)),
                };
                writeln!(out, "{pad}}}")?;
            }
            Stmt::Assign { target, value, .. } => self.assign(target, value, scope, depth, out)?,
            Stmt::Expr { expr, .. } => match expr {
                Expr::Ident(name) if lower(name) == "exit" => writeln!(out, "{pad}{}", self.exit_code(scope))?,
                Expr::Member { base, name } => {
                    if let Expr::Call { callee, .. } = strip_paren(base)
                        && matches!(&**callee, Expr::Ident(callee) if lower(callee) == "nif")
                        && lower(name) == "updatenifversion"
                    {
                        writeln!(out, "{pad}update_nif_version(t)?;")?;
                    } else {
                        bail!("statement {expr:?}");
                    }
                }
                Expr::Call { callee, args } => match &**callee {
                    Expr::Member { base, name } if lower(name) == "doexception" => {
                        let owner = self.el(base, scope)?;
                        let message = self.cexpr(&args[0], scope)?;
                        if message.ty != T::Str {
                            bail!("DoException message");
                        }
                        writeln!(out, "{pad}return Err(t.exception({owner}, &{}));", message.code)?;
                    }
                    Expr::Ident(name) if lower(name) == "exit" => writeln!(out, "{pad}{}", self.exit_code(scope))?,
                    Expr::Ident(name) if matches!(lower(name).as_str(), "inc" | "dec") => {
                        let target = self.place(&args[0], scope)?;
                        let amount = match args.get(1) {
                            Some(amount) => {
                                let amount = self.cexpr(amount, scope)?;
                                self.conv(amount, T::Int)?
                            }
                            None => "1".to_owned(),
                        };
                        let op = if lower(name) == "inc" { "+=" } else { "-=" };
                        if target.starts_with('*') {
                            writeln!(out, "{pad}{target} {op} ({amount}) as i32;")?;
                        } else {
                            writeln!(out, "{pad}{target} {op} {amount};")?;
                        }
                    }
                    Expr::Member { base, name } if lower(name) == "delete" => {
                        let owner = self.el(base, scope)?;
                        let index = self.cexpr(&args[0], scope)?;
                        let index = self.conv(index, T::Int)?;
                        writeln!(
                            out,
                            "{pad}{};",
                            tcall("t.delete", &[owner, format!("({index}) as i32")], "?")
                        )?;
                    }
                    _ => bail!("statement {expr:?}"),
                },
                other => bail!("statement {other:?}"),
            },
            other => bail!("statement {other:?}"),
        }
        Ok(())
    }

    /// A variable that `Inc` and `Dec` change: a local integer or the count.
    fn place(&self, expr: &Expr, scope: &CScope) -> Result<String> {
        let Expr::Ident(name) = strip_paren(expr) else {
            bail!("place {expr:?}");
        };
        let lname = lower(name);
        if let Some((rust, T::Int)) = scope.locals.get(&lname) {
            return Ok(rust.clone());
        }
        if scope.sig.kind == CbKind::Count
            && let Some(param) = scope.sig.params.get(1)
            && lname == lower(param)
        {
            return Ok(format!("*{}", snake(name)));
        }
        bail!("place {name}")
    }

    fn local(&self, var: &VarDecl, scope: &mut CScope, depth: usize, out: &mut String) -> Result<()> {
        let pad = "    ".repeat(depth);
        if var.names.len() != 1 {
            bail!("several variables in one declaration");
        }
        let name = &var.names[0];
        let rust = snake(name);
        let (ty, code) = match (&var.type_ref, &var.value) {
            (_, Some(value)) => {
                let value = self.cexpr(value, scope)?;
                let ty = value.ty;
                (ty, value.code)
            }
            (Some(type_ref), None) => match type_name(&Some(type_ref.clone())).as_str() {
                "integer" | "word" | "cardinal" | "byte" | "int64" => (T::Int, "0".to_owned()),
                "boolean" => (T::Bool, "false".to_owned()),
                "string" => (T::Str, "String::new()".to_owned()),
                "tdfelement" => (T::OptEl, "None::<El>".to_owned()),
                other => bail!("local of type {other}"),
            },
            (None, None) => bail!("local without type"),
        };
        let code = if ty == T::Str {
            format!("({code}).to_string()")
        } else {
            code
        };
        if ty == T::Int {
            writeln!(out, "{pad}let mut {rust}: i64 = {code};")?;
        } else {
            writeln!(out, "{pad}let mut {rust} = {code};")?;
        }
        let shown_ty = if ty == T::Str { T::Str } else { ty };
        let rust_ref = if ty == T::Str { format!("{rust}.as_str()") } else { rust };
        scope.locals.insert(lower(name), (rust_ref, shown_ty));
        Ok(())
    }

    fn assign(&self, target: &Expr, value: &Expr, scope: &mut CScope, depth: usize, out: &mut String) -> Result<()> {
        let pad = "    ".repeat(depth);
        let sig = scope.sig;
        match target {
            Expr::Ident(name) => {
                let lname = lower(name);
                let value = self.cexpr(value, scope)?;
                if lname == "result" {
                    let ty = match sig.kind {
                        CbKind::Enabled => T::Bool,
                        CbKind::IntFn => T::Int,
                        CbKind::LinksTo => T::OptEl,
                        _ => bail!("Result in a procedure"),
                    };
                    writeln!(out, "{pad}result = {};", self.conv(value, ty)?)?;
                    return Ok(());
                }
                if let Some((rust, ty)) = scope.locals.get(&lname).cloned() {
                    let rust = rust.trim_end_matches(".as_str()").to_owned();
                    let code = self.conv(value, ty)?;
                    let code = if ty == T::Str {
                        format!("({code}).to_string()")
                    } else {
                        code
                    };
                    writeln!(out, "{pad}{rust} = {code};")?;
                    return Ok(());
                }
                if let Some(param) = sig.params.get(1)
                    && lname == lower(param)
                {
                    let rust = snake(name);
                    match sig.kind {
                        CbKind::Value => writeln!(out, "{pad}*{rust} = {};", self.conv(value, T::Var)?)?,
                        CbKind::Text => {
                            writeln!(out, "{pad}*{rust} = ({}).to_string();", self.conv(value, T::Str)?)?;
                        }
                        CbKind::Count => writeln!(out, "{pad}*{rust} = ({}) as i32;", self.conv(value, T::Int)?)?,
                        _ => bail!("assignment to {name}"),
                    }
                    return Ok(());
                }
                bail!("assignment to {name}")
            }
            Expr::Index { base, args } => {
                let Expr::Member { base: owner, name } = &**base else {
                    bail!("assignment to {target:?}");
                };
                let owner = self.el(owner, scope)?;
                let path = self.path_arg(args, scope)?;
                let value = self.cexpr(value, scope)?;
                match lower(name).as_str() {
                    "nativevalues" => {
                        let value = self.conv(value, T::Var)?;
                        writeln!(
                            out,
                            "{pad}{};",
                            tcall("t.set_native_values", &[owner, path, value], "?")
                        )?
                    }
                    "editvalues" => {
                        let value = format!("&({}).to_string()", self.conv(value, T::Str)?);
                        writeln!(out, "{pad}{};", tcall("t.set_edit_values", &[owner, path, value], "?"))?
                    }
                    other => bail!("assignment to {other}"),
                }
                Ok(())
            }
            Expr::Member { base, name } => {
                let owner = self.el(base, scope)?;
                let value = self.cexpr(value, scope)?;
                match lower(name).as_str() {
                    "nativevalue" => {
                        let value = self.conv(value, T::Var)?;
                        writeln!(out, "{pad}{};", tcall("t.set_native_value", &[owner, value], "?"))?
                    }
                    "editvalue" => {
                        let value = format!("&({}).to_string()", self.conv(value, T::Str)?);
                        writeln!(out, "{pad}{};", tcall("t.set_edit_value", &[owner, value], "?"))?
                    }
                    "userdata" => {
                        let value = format!("({}) as i32", self.conv(value, T::Int)?);
                        writeln!(out, "{pad}{};", tcall("t.set_user_data", &[owner, value], ""))?
                    }
                    "count" => {
                        let value = format!("({}) as i32", self.conv(value, T::Int)?);
                        writeln!(out, "{pad}{};", tcall("t.set_count", &[owner, value], "?"))?
                    }
                    other => bail!("assignment to {other}"),
                }
                Ok(())
            }
            other => bail!("assignment to {other:?}"),
        }
    }

    fn callback(&self, routine: &Routine, sig: &CallbackSig) -> Result<String> {
        let mut scope = CScope {
            sig,
            locals: HashMap::new(),
            with_nif: false,
        };
        let body = routine.body.as_ref().ok_or_else(|| anyhow!("no body"))?;
        let mut out = String::new();
        writeln!(out, "/// Upstream `{}` (line {}).", routine.name, routine.line)?;
        writeln!(out, "{} {{", callback_signature(sig))?;
        match sig.kind {
            CbKind::Enabled => writeln!(out, "    let mut result = false;")?,
            CbKind::IntFn => writeln!(out, "    let mut result: i64 = 0;")?,
            CbKind::LinksTo => writeln!(out, "    let mut result: Option<El> = None;")?,
            _ => {}
        }
        for decl in &body.decls {
            match decl {
                Decl::Var(var) => self.local(var, &mut scope, 1, &mut out)?,
                other => bail!("declaration {other:?}"),
            }
        }
        self.cstatements(&body.statements, &mut scope, 1, &mut out)?;
        match sig.kind {
            CbKind::Enabled | CbKind::LinksTo => writeln!(out, "    Ok(result)")?,
            CbKind::IntFn => writeln!(out, "    Ok(result as i32)")?,
            _ => writeln!(out, "    Ok(())")?,
        }
        writeln!(out, "}}")?;
        Ok(out)
    }
}

/// The unit constants that are integers or strings.
fn unit_consts(unit: &Unit, consts: &mut HashMap<String, (String, T)>, out: Option<&mut String>) {
    let mut text = String::new();
    for decl in unit.implementation.iter() {
        let Decl::Const(var) = decl else { continue };
        if var.type_ref.is_some() {
            continue;
        }
        let Some(value) = &var.value else { continue };
        for name in &var.names {
            let rust = snake(name).to_ascii_uppercase();
            let scope = BScope::default();
            let emitter = Emitter {
                builders: HashMap::new(),
                callbacks: HashMap::new(),
                consts: consts.clone(),
                referenced: RefCell::new(BTreeSet::new()),
            };
            if let Ok(code) = emitter.int_arg(value, &scope) {
                let _ = writeln!(text, "pub const {rust}: i64 = {code};");
                consts.insert(lower(name), (rust, T::Int));
            } else if let Expr::Str(string) = value {
                let _ = writeln!(text, "pub const {rust}: &str = {};", rust_str(string));
                consts.insert(lower(name), (rust, T::Str));
            }
        }
    }
    if let Some(out) = out {
        out.push_str(&text);
    }
}

/// `cargo xtask port-defs emit-df <upstream> <unit> <out> <stubs> <hand>`.
pub fn emit_df(upstream: &Path, unit_name: &str, out: &Path, stubs: &Path, hand: &Path) -> Result<()> {
    let types_unit = read_unit(upstream, "Core/wbDataFormatNifTypes.pas")?;
    let unit = read_unit(upstream, &format!("Core/{unit_name}.pas"))?;
    let mut hand_written = HashSet::new();
    for path in hand.to_string_lossy().split(';') {
        hand_written.extend(hand_names(Path::new(path))?);
    }
    let generated = generate(&types_unit, &unit, unit_name, &hand_written)?;
    std::fs::write(out, &generated.text).with_context(|| format!("writing {}", out.display()))?;
    std::fs::write(stubs, &generated.stubs).with_context(|| format!("writing {}", stubs.display()))?;
    println!(
        "{unit_name}: {} routines written, {} callback stubs, {} failures",
        generated.written,
        generated.stub_count,
        generated.failures.len()
    );
    for failure in &generated.failures {
        println!("  {failure}");
    }
    if !generated.unresolved.is_empty() {
        println!("unresolved callbacks: {}", generated.unresolved.join(", "));
    }
    Ok(())
}

/// The output of the transpiler for one unit.
pub struct Generated {
    pub text: String,
    pub stubs: String,
    pub written: usize,
    pub stub_count: usize,
    pub failures: Vec<String>,
    /// Callbacks a definition names that the unit does not define.
    pub unresolved: Vec<String>,
}

/// Transpiles `unit`; `types_unit` is `wbDataFormatNifTypes`, whose builders
/// every data format unit may call. The routines in `hand_written` (Rust
/// names) are not generated.
pub fn generate(types_unit: &Unit, unit: &Unit, unit_name: &str, hand_written: &HashSet<String>) -> Result<Generated> {
    let mut builders = HashMap::new();
    unit_builders(types_unit, &mut builders)?;
    if unit_name != "wbDataFormatNifTypes" {
        unit_builders(unit, &mut builders)?;
    }

    let mut consts = HashMap::new();
    // The NIF version variables, which `wbDefineNif` sets once.
    if unit_name == "wbDataFormatNif" {
        for decl in &unit.implementation {
            if let Decl::Var(var) = decl {
                for name in &var.names {
                    let lname = lower(name);
                    if lname.starts_with('v') && lname[1..].bytes().all(|byte| byte.is_ascii_digit()) && lname.len() > 1
                    {
                        consts.insert(lname, (name.to_ascii_uppercase(), T::Int));
                    }
                }
            }
        }
    }
    let mut const_text = String::new();
    unit_consts(unit, &mut consts, Some(&mut const_text));

    let mut callbacks = HashMap::new();
    let mut callback_routines = Vec::new();
    for decl in &unit.implementation {
        if let Decl::Routine(routine) = decl
            && !is_builder(routine)
            && routine.body.is_some()
            && let Some(kind) = callback_kind(routine)
        {
            let sig = CallbackSig {
                rust: snake(&routine.name),
                kind,
                params: routine.params.iter().map(|param| param.name.clone()).collect(),
                line: routine.line,
            };
            callbacks.insert(lower(&routine.name), sig);
            callback_routines.push(routine);
        }
    }
    // The NifTypes callbacks and builders are visible to the Nif unit
    // through its imports; their names only have to resolve.
    if unit_name != "wbDataFormatNifTypes" {
        for decl in &types_unit.implementation {
            if let Decl::Routine(routine) = decl
                && !is_builder(routine)
                && let Some(kind) = callback_kind(routine)
            {
                callbacks.entry(lower(&routine.name)).or_insert(CallbackSig {
                    rust: snake(&routine.name),
                    kind,
                    params: routine.params.iter().map(|param| param.name.clone()).collect(),
                    line: routine.line,
                });
            }
        }
    }

    let emitter = Emitter {
        builders,
        callbacks,
        consts,
        referenced: RefCell::new(BTreeSet::new()),
    };

    let mut text = format!(
        "{HEADER}\n// Ported from xEdit: Core/{unit_name}.pas\n// Generated by `cargo xtask port-defs emit-df`. Do not edit.\n\n"
    );
    text.push_str(
        "#![allow(clippy::all, unused_mut, unused_variables, unused_parens, unused_imports, unused_assignments, non_snake_case, dead_code)]\n\n",
    );
    text.push_str("use super::*;\n\n");
    text.push_str(&const_text);

    let mut failures = Vec::new();
    let mut written = 0;
    let mut stubbed: Vec<&CallbackSig> = Vec::new();
    let mut generated_callbacks: HashSet<String> = HashSet::new();

    // Builders, in source order; each overload once.
    let mut emitted_builders: HashSet<String> = HashSet::new();
    for decl in &unit.implementation {
        let Decl::Routine(routine) = decl else { continue };
        if routine.body.is_none() {
            continue;
        }
        if is_builder(routine) {
            let sigs = emitter.builders.get(&lower(&routine.name)).cloned().unwrap_or_default();
            let Some(sig) = sigs.iter().find(|sig| {
                sig.params.len() == routine.params.len()
                    && sig
                        .params
                        .iter()
                        .zip(&routine.params)
                        .all(|((name, _), param)| name.eq_ignore_ascii_case(&param.name))
            }) else {
                failures.push(format!("{} (line {}): no signature", routine.name, routine.line));
                continue;
            };
            if hand_written.contains(&sig.rust) || !emitted_builders.insert(sig.rust.clone()) {
                continue;
            }
            match emitter.builder(routine, sig, 0) {
                Ok(code) => {
                    text.push('\n');
                    text.push_str(&code);
                    written += 1;
                }
                Err(error) => failures.push(format!("{} (line {}): {error}", routine.name, routine.line)),
            }
        } else if routine.kind == RoutineKind::Procedure
            && routine.params.is_empty()
            && lower(&routine.name).starts_with("wbdefine")
        {
            let rust = snake(&routine.name);
            if hand_written.contains(&rust) {
                continue;
            }
            match emitter.define(routine) {
                Ok(code) => {
                    text.push('\n');
                    text.push_str(&code);
                    written += 1;
                }
                Err(error) => failures.push(format!("{} (line {}): {error}", routine.name, routine.line)),
            }
        }
    }

    for routine in &callback_routines {
        let sig = &emitter.callbacks[&lower(&routine.name)];
        if hand_written.contains(&sig.rust) {
            continue;
        }
        match emitter.callback(routine, sig) {
            Ok(code) => {
                text.push('\n');
                text.push_str(&code);
                written += 1;
                generated_callbacks.insert(sig.rust.clone());
            }
            Err(error) => {
                failures.push(format!("{} (line {}): {error}", routine.name, routine.line));
                stubbed.push(sig);
            }
        }
    }
    let mut stub_text = format!(
        "{HEADER}\n// Ported from xEdit: Core/{unit_name}.pas\n// Generated by `cargo xtask port-defs emit-df`. Do not edit.\n\n//! The callbacks that are not ported yet. Each fails when called.\n\n#![allow(unused_variables, unused_imports, dead_code)]\n\nuse super::*;\n"
    );
    for sig in &stubbed {
        let _ = write!(
            stub_text,
            "\n{} {{\n    Err(DfError::new(\"not ported: {} line {}\"))\n}}\n",
            callback_signature(sig),
            sig.rust,
            sig.line
        );
    }
    let unresolved: Vec<String> = emitter
        .referenced
        .borrow()
        .iter()
        .filter(|name| !emitter.callbacks.contains_key(*name))
        .cloned()
        .collect();
    Ok(Generated {
        text,
        stubs: stub_text,
        written,
        stub_count: stubbed.len(),
        failures,
        unresolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pascal::parser::parse_unit;

    const TYPES: &str = "unit wbDataFormatNifTypes;
interface
uses wbDataFormat;
function wbVector3(const aName, aDefaultValue: string; const aEvents: array of const): TdfDef; overload;
function wbVector3(const aName: string): TdfDef; overload;
implementation
function wbVector3(const aName, aDefaultValue: string; const aEvents: array of const): TdfDef;
begin
  Result := dfMerge(aName, [dfFloat('X'), dfFloat('Y'), dfFloat('Z')], aDefaultValue, aEvents);
end;
function wbVector3(const aName: string): TdfDef;
begin
  Result := wbVector3(aName, '', []);
end;
end.
";

    const NIF: &str = "unit wbDataFormatNif;
interface
implementation
var
  v10100: Cardinal;
const
  VF_UV = 1 shl 5;
function EnSince10100(const e: TdfElement): Boolean; begin Result := nif(e).Version >= v10100; end;
function EnUV(const e: TdfElement): Boolean; begin with nif(e) do Result := (Version >= v10100) and (e.NativeValues['..\\Flags'] and VF_UV <> 0); end;
procedure SetCount(const e: TdfElement; var aCount: Integer); begin aCount := e.NativeValues['..\\Num'] * 2; end;
procedure Unknown(const e: TdfElement); begin e.Frobnicate; end;
function wbNifBlock(const aName: string; const aDefs: TdfDefs; const aEvents: array of const): TwbNifBlockDef; overload;
begin
  Result := TwbNifBlockDef.Create(aName, dtStruct, aDefs);
  Result.AssignEvents(aEvents);
end;
function wbNifBlock(const aName: string; const aDefs: TdfDefs): TwbNifBlockDef; overload;
begin
  Result := wbNifBlock(aName, aDefs, []);
end;
procedure wbDefineThing;
begin
  wbNiObject(wbNifBlock('Thing', [
    wbVector3('Position'),
    dfBool('Visible', dtU8, [DF_OnGetEnabled, @EnSince10100]),
    dfArray('Items', dfInteger('Item', dtU16), 0, '', [DF_OnGetCount, @SetCount])
  ]), 'NiObject', False);
end;
end.
";

    #[test]
    fn transpiles_builders_defines_and_callbacks() {
        let types = parse_unit(TYPES, &[]).unwrap();
        let nif = parse_unit(NIF, &[]).unwrap();
        let generated = generate(&types, &nif, "wbDataFormatNif", &HashSet::new()).unwrap();
        let text = &generated.text;
        assert_eq!(generated.failures.len(), 1, "{:?}", generated.failures);
        assert!(text.contains("pub const VF_UV: i64 = (1 << 5);"), "{text}");
        assert!(text.contains("result = (nif_version(t) >= V10100);"), "{text}");
        assert!(
            text.contains("&\"..\\\\Flags\")?).to_i64()? & VF_UV) != 0i64)"),
            "{text}"
        );
        assert!(
            text.contains("*a_count = (((t.native_values(e, &\"..\\\\Num\")?).to_i64()? * 2i64)) as i32;"),
            "{text}"
        );
        // dfBool with events drops them, as upstream.
        assert!(
            text.contains("/* UPSTREAM-QUIRK: events dropped */ df_bool(\"Visible\", DataType::U8, \"\", &[])"),
            "{text}"
        );
        assert!(text.contains("wb_vector3_name(\"Position\")"), "{text}");
        assert!(text.contains("df_array(\"Items\", df_integer(\"Item\", DataType::U16, \"\", &[]), 0 as i32, \"\", &[Event::GetCount(set_count)])"), "{text}");
        assert!(
            text.contains("wb_ni_object(infos, wb_nif_block_name_defs(\"Thing\""),
            "{text}"
        );
        // A callback outside of the subset is stubbed.
        assert_eq!(generated.stub_count, 1);
        assert!(
            generated.stubs.contains("pub fn unknown(t: &mut Tree, e: El) -> R<()>"),
            "{}",
            generated.stubs
        );
    }
}
