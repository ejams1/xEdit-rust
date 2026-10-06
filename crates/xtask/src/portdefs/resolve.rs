// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Types of Pascal expressions and resolution of overloaded calls.

use std::collections::HashMap;

use anyhow::{Result, bail};

use super::model::{RoutineSig, Symbols, Ty, TypeKind};
use crate::pascal::ast::{Expr, Param, Routine, TypeRef};

/// The local names of a routine body: parameters and variables.
#[derive(Default, Clone)]
pub struct Scope {
    locals: HashMap<String, Ty>,
}

impl Scope {
    pub fn insert(&mut self, name: &str, ty: Ty) {
        self.locals.insert(name.to_ascii_lowercase(), ty);
    }

    pub fn get(&self, name: &str) -> Option<&Ty> {
        self.locals.get(&name.to_ascii_lowercase())
    }
}

/// The overload that a call resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// Index into the overloads of the routine.
    pub overload: usize,
}

pub struct Resolver<'a> {
    pub symbols: &'a Symbols,
}

impl<'a> Resolver<'a> {
    /// The type of `expr`. `Ty::Unknown` when the transpiler does not know it.
    pub fn ty_of(&self, expr: &Expr, scope: &Scope) -> Ty {
        match expr {
            Expr::Str(_) => Ty::Str,
            Expr::Int { .. } => Ty::Int,
            Expr::Float(_) => Ty::Float,
            Expr::Nil => Ty::Nil,
            Expr::Paren(inner) => self.ty_of(inner, scope),
            Expr::List(items) => match items.first() {
                None => Ty::EmptyList,
                Some(first) => Ty::Array(Box::new(self.ty_of(first, scope))),
            },
            Expr::Ident(name) => {
                let lower = name.to_ascii_lowercase();
                if lower == "true" || lower == "false" {
                    return Ty::Bool;
                }
                if lower == "pi" {
                    return Ty::Float;
                }
                if let Some(ty) = scope.get(name) {
                    return ty.clone();
                }
                if let Some(ty) = self.symbols.values.get(&lower) {
                    return ty.clone();
                }
                if self.symbols.routines.contains_key(&lower) {
                    return Ty::Routine(lower);
                }
                Ty::Unknown
            }
            Expr::Call { callee, args } => match &**callee {
                Expr::Ident(name) => {
                    let lower = name.to_ascii_lowercase();
                    // The functions that the compiler provides.
                    match lower.as_str() {
                        "length" | "high" | "low" | "ord" | "pred" | "succ" => return Ty::Int,
                        "assigned" => return Ty::Bool,
                        "inttostr" | "format" | "paramstr" | "extractfilepath" => return Ty::Str,
                        "fileexists" => return Ty::Bool,
                        _ => {}
                    }
                    // A cast such as Integer(x) or IwbFoo(x).
                    if !self.symbols.routines.contains_key(&lower)
                        && (self.symbols.types.contains_key(&lower)
                            || self.symbols.ty_of_name(name) != Ty::Named(lower))
                    {
                        return self.symbols.ty_of_name(name);
                    }
                    match self.resolve_call(name, args, scope) {
                        Ok((_, sig)) => self.return_ty(sig),
                        Err(_) => Ty::Unknown,
                    }
                }
                // A method call: the result type of the method.
                Expr::Member { base, name } => self.member_ty(base, name, scope),
                _ => Ty::Unknown,
            },
            Expr::Member { base, name } => self.member_ty(base, name, scope),
            Expr::Index { base, .. } => match self.ty_of(base, scope) {
                Ty::Array(element) => *element,
                _ => Ty::Unknown,
            },
            Expr::Binary { op, left, right } => match op.as_str() {
                "as" => match &**right {
                    Expr::Ident(name) => self.symbols.ty_of_name(name),
                    _ => Ty::Unknown,
                },
                "=" | "<>" | "<" | ">" | "<=" | ">=" | "in" | "is" => Ty::Bool,
                "/" => Ty::Float,
                _ => {
                    let left = self.ty_of(left, scope);
                    if left == Ty::Unknown {
                        self.ty_of(right, scope)
                    } else {
                        left
                    }
                }
            },
            Expr::Unary { op, operand } => {
                if op == "@" {
                    // The address of a value is of the pointer type declared
                    // for its type, such as `PwbKnownSubRecordSignatures`.
                    match self.ty_of(operand, scope) {
                        Ty::Named(pointee) => self
                            .symbols
                            .types
                            .iter()
                            .find(|(_, info)| {
                                matches!(&info.kind, TypeKind::Pointer(target) if target.eq_ignore_ascii_case(&pointee))
                            })
                            .map_or(Ty::Unknown, |(name, _)| Ty::Named(name.clone())),
                        _ => Ty::Unknown,
                    }
                } else {
                    self.ty_of(operand, scope)
                }
            }
            _ => Ty::Unknown,
        }
    }

    /// The type of the function or property `name` of the value `base`.
    fn member_ty(&self, base: &Expr, name: &str, scope: &Scope) -> Ty {
        let base_ty = match self.ty_of(base, scope) {
            // A function without arguments that is called without parentheses.
            Ty::Routine(routine) => self.implicit_call_ty(&routine).unwrap_or(Ty::Unknown),
            other => other,
        };
        match base_ty {
            Ty::Named(type_name) => self.symbols.member_ty(&type_name, name).unwrap_or(Ty::Unknown),
            _ => Ty::Unknown,
        }
    }

    fn return_ty(&self, sig: &RoutineSig) -> Ty {
        match &sig.return_type {
            Some(type_ref) => self.symbols.ty_of_type_ref(type_ref),
            None => Ty::Unknown,
        }
    }

    /// How well a value of type `from` fits a parameter of type `to`: 0 for the
    /// same type, a higher number for a conversion, `None` when it does not fit.
    pub fn assign_cost(&self, from: &Ty, to: &Ty, expr: Option<&Expr>, scope: &Scope) -> Option<u32> {
        if from == to {
            return Some(0);
        }
        match (from, to) {
            (Ty::Unknown, _) | (_, Ty::Unknown) => Some(2),
            (Ty::Int, Ty::Float) => Some(1),
            // A string literal of four characters is a signature.
            (Ty::Str, Ty::Sig) => match expr {
                Some(Expr::Str(text)) if text.chars().count() == 4 => Some(1),
                _ => None,
            },
            (Ty::Sig, Ty::Str) => Some(1),
            (Ty::Nil, Ty::Named(_) | Ty::Array(_)) => Some(0),
            (Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Sig, Ty::Named(name)) if name == "variant" => Some(1),
            (Ty::EmptyList, Ty::Array(_) | Ty::ArrayOfConst) => Some(0),
            (Ty::EmptyList, Ty::Named(name)) => {
                // An empty set of an enumeration.
                name.starts_with("twb").then_some(1)
            }
            (Ty::Array(_), Ty::ArrayOfConst) => Some(0),
            (Ty::Array(from_element), Ty::Array(to_element)) => {
                // The elements of a list literal are checked one by one.
                if let Some(Expr::List(items)) = expr {
                    let mut total = 0;
                    for item in items {
                        let item_ty = self.ty_of(item, scope);
                        let item_ty = if item_ty == Ty::Unknown {
                            (**from_element).clone()
                        } else {
                            item_ty
                        };
                        total = total.max(self.assign_cost(&item_ty, to_element, Some(item), scope)?);
                    }
                    Some(total)
                } else {
                    self.assign_cost(from_element, to_element, None, scope)
                }
            }
            (Ty::Named(from_name), Ty::Named(to_name)) => self.symbols.descends_from(from_name, to_name).then_some(1),
            (Ty::Routine(routine), Ty::Named(to_name)) => {
                if let Some(callback) = self.symbols.callback(to_name) {
                    let overloads = self.symbols.routines.get(routine)?;
                    return overloads.iter().any(|sig| callback_matches(sig, callback)).then_some(0);
                }
                // A routine without required parameters is called.
                let result = self.implicit_call_ty(routine)?;
                self.assign_cost(&result, to, None, scope).map(|cost| cost + 1)
            }
            (Ty::Routine(routine), _) => {
                let result = self.implicit_call_ty(routine)?;
                self.assign_cost(&result, to, None, scope).map(|cost| cost + 1)
            }
            _ => None,
        }
    }

    /// The result type of calling the routine without arguments, if it has an
    /// overload that allows that.
    fn implicit_call_ty(&self, routine: &str) -> Option<Ty> {
        self.symbols
            .routines
            .get(routine)?
            .iter()
            .find(|sig| sig.params.iter().all(|param| param.default.is_some()))
            .map(|sig| self.return_ty(sig))
    }

    /// Resolves a call of the routine `name` with `args` to one overload.
    pub fn resolve_call(&self, name: &str, args: &[Expr], scope: &Scope) -> Result<(Resolved, &'a RoutineSig)> {
        let Some(overloads) = self.symbols.routines.get(&name.to_ascii_lowercase()) else {
            bail!("unknown routine {name}");
        };
        let arg_types: Vec<Ty> = args.iter().map(|arg| self.ty_of(arg, scope)).collect();
        let mut best: Vec<(u32, usize)> = Vec::new();
        for (index, sig) in overloads.iter().enumerate() {
            if args.len() > sig.params.len() || sig.params[args.len()..].iter().any(|param| param.default.is_none()) {
                continue;
            }
            let cost = args
                .iter()
                .zip(&arg_types)
                .zip(&sig.params)
                .try_fold(0u32, |total, ((arg, ty), param)| {
                    let to = self.param_ty(param);
                    self.assign_cost(ty, &to, Some(arg), scope).map(|cost| total + cost)
                });
            if let Some(cost) = cost {
                best.push((cost, index));
            }
        }
        best.sort();
        // Delphi prefers `Integer` for integer arguments over the other
        // integer types when nothing else tells the overloads apart, unless
        // a literal does not fit in it.
        if let [(cost, _), (next, _), ..] = best.as_slice()
            && cost == next
        {
            let tied: Vec<usize> = best
                .iter()
                .filter(|(c, _)| c == cost)
                .map(|(_, index)| *index)
                .collect();
            // A routine of a unit hides the routines of the units it uses, so
            // among overloads from different units the last unit wins (the
            // overloads are in the order their units were added).
            let last_unit = tied.iter().map(|&index| &overloads[index].unit).max_by_key(|unit| {
                overloads
                    .iter()
                    .rposition(|overload| &overload.unit == *unit)
                    .unwrap_or(0)
            });
            if let Some(last_unit) = last_unit {
                let own: Vec<usize> = tied
                    .iter()
                    .copied()
                    .filter(|&index| &overloads[index].unit == last_unit)
                    .collect();
                if let [index] = own.as_slice()
                    && own.len() < tied.len()
                {
                    return Ok((Resolved { overload: *index }, &overloads[*index]));
                }
            }
            let wide = args.iter().any(|arg| match arg {
                Expr::Int { digits, hex: true } => u64::from_str_radix(digits, 16).is_ok_and(|v| v > i32::MAX as u64),
                Expr::Int { digits, hex: false } => digits.parse::<u64>().is_ok_and(|v| v > i32::MAX as u64),
                _ => false,
            });
            let preferred_type = if wide { "cardinal" } else { "integer" };
            let integers = |index: usize| {
                overloads[index]
                    .params
                    .iter()
                    .filter(|param| matches!(&param.type_ref, Some(TypeRef::Named(name)) if name.eq_ignore_ascii_case(preferred_type)))
                    .count()
            };
            let most = tied.iter().map(|&index| integers(index)).max().unwrap_or(0);
            let preferred: Vec<usize> = tied.iter().copied().filter(|&index| integers(index) == most).collect();
            if let [index] = preferred.as_slice()
                && most > 0
            {
                return Ok((Resolved { overload: *index }, &overloads[*index]));
            }
        }
        match best.as_slice() {
            [] => bail!(
                "no overload of {name} takes ({})",
                arg_types
                    .iter()
                    .map(|ty| format!("{ty:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            [(cost, _), (next, _), ..] if cost == next => bail!(
                "call of {name} is ambiguous between {} overloads for ({})",
                best.iter().filter(|(c, _)| c == cost).count(),
                arg_types
                    .iter()
                    .map(|ty| format!("{ty:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            [(_, index), ..] => Ok((Resolved { overload: *index }, &overloads[*index])),
        }
    }

    pub fn param_ty(&self, param: &Param) -> Ty {
        match &param.type_ref {
            Some(type_ref) => self.symbols.ty_of_type_ref(type_ref),
            None => Ty::Unknown,
        }
    }
}

/// Whether the routine can be assigned to the callback type: same kind and
/// the same parameter types.
pub fn callback_matches(sig: &RoutineSig, callback: &Routine) -> bool {
    let same_types = |a: &Param, b: &Param| match (&a.type_ref, &b.type_ref) {
        (Some(a), Some(b)) => format!("{a:?}").eq_ignore_ascii_case(&format!("{b:?}")),
        (None, None) => true,
        _ => false,
    };
    let same_return = match (&sig.return_type, &callback.return_type) {
        (Some(a), Some(b)) => format!("{a:?}").eq_ignore_ascii_case(&format!("{b:?}")),
        (None, None) => true,
        _ => false,
    };
    same_return
        && sig.params.len() == callback.params.len()
        && sig.params.iter().zip(&callback.params).all(|(a, b)| same_types(a, b))
}
