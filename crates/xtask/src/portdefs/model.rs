// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The symbols and types of the parsed Pascal units, as far as the
//! transpiler needs them to resolve overloaded calls.

use std::collections::{HashMap, HashSet};

use crate::pascal::ast::{Decl, Expr, Param, Routine, RoutineKind, TypeDecl, TypeRef, Unit};
use crate::pascal::lexer::{Token, TokenKind};
use crate::pascal::parser::Parser;

/// The type of an expression or a parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Str,
    /// `TwbSignature`
    Sig,
    Bool,
    Int,
    Float,
    /// The literal `nil`.
    Nil,
    /// A type by its lower-case name: an enumeration, interface, class,
    /// callback type or record.
    Named(String),
    /// A dynamic or open array.
    Array(Box<Ty>),
    /// `array of const`
    ArrayOfConst,
    /// The literal `[]`.
    EmptyList,
    /// A routine that is named without being called.
    Routine(String),
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Enum(Vec<String>),
    /// An interface with its parent, lower case.
    Interface(Option<String>),
    /// A class with its parent and the interfaces it implements, lower case.
    Class(Vec<String>),
    /// A procedural type with its signature.
    Callback(Box<Routine>),
    Alias(Ty),
    /// `set of T` with the lower-case name of `T`.
    Set(String),
    /// `^T` with the name of `T` as declared.
    Pointer(String),
    Other,
}

#[derive(Debug, Clone)]
pub struct TypeInfo {
    /// The name as declared.
    pub name: String,
    pub kind: TypeKind,
}

/// One overload of a routine.
#[derive(Debug, Clone)]
pub struct RoutineSig {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub unit: String,
    pub line: u32,
}

#[derive(Default)]
pub struct Symbols {
    /// Lower-case type name to its declaration.
    pub types: HashMap<String, TypeInfo>,
    /// Lower-case routine name to its overloads, in declaration order.
    pub routines: HashMap<String, Vec<RoutineSig>>,
    /// Lower-case name of a variable, constant or enumeration value to its type.
    pub values: HashMap<String, Ty>,
    /// Lower-case interface or class name to its functions and properties:
    /// lower-case member name to the name of its type.
    pub members: HashMap<String, HashMap<String, String>>,
    /// Lower-case names of the constants among `values`.
    pub consts: HashSet<String>,
}

fn lower(name: &str) -> String {
    name.to_ascii_lowercase()
}

impl Symbols {
    /// Adds the interface declarations of `unit`, and its implementation
    /// declarations when `with_implementation`.
    pub fn add_unit(&mut self, unit: &Unit, with_implementation: bool) {
        // Types first, so that the types of values resolve.
        for decl in unit.interface.iter().chain(
            with_implementation
                .then_some(&unit.implementation)
                .into_iter()
                .flatten(),
        ) {
            if let Decl::Type(type_decl) = decl {
                self.add_type(type_decl);
            }
        }
        for decl in &unit.interface {
            self.add_decl(decl, &unit.name);
        }
        if with_implementation {
            for decl in &unit.implementation {
                self.add_decl(decl, &unit.name);
            }
        }
    }

    fn add_decl(&mut self, decl: &Decl, unit: &str) {
        match decl {
            Decl::Type(_) => {}
            Decl::Const(var) | Decl::Var(var) => {
                let ty = match (&var.type_ref, &var.value) {
                    (Some(type_ref), _) => self.ty_of_type_ref(type_ref),
                    (None, Some(value)) => self.literal_ty(value),
                    (None, None) => Ty::Unknown,
                };
                for name in &var.names {
                    self.values.insert(lower(name), ty.clone());
                    if matches!(decl, Decl::Const(_)) {
                        self.consts.insert(lower(name));
                    }
                }
            }
            Decl::Routine(routine) => {
                // A method implementation is not a routine that definitions call.
                if routine.name.contains('.') || routine.kind == RoutineKind::Operator {
                    return;
                }
                let overloads = self.routines.entry(lower(&routine.name)).or_default();
                // The implementation repeats the header of an interface declaration.
                let repeated = overloads.iter().any(|known| {
                    known.params.len() == routine.params.len()
                        && known
                            .params
                            .iter()
                            .zip(&routine.params)
                            .all(|(a, b)| a.type_ref == b.type_ref && a.name.eq_ignore_ascii_case(&b.name))
                });
                if !repeated {
                    overloads.push(RoutineSig {
                        name: routine.name.clone(),
                        params: routine.params.clone(),
                        return_type: routine.return_type.clone(),
                        unit: unit.to_owned(),
                        line: routine.line,
                    });
                }
            }
        }
    }

    fn add_type(&mut self, decl: &TypeDecl) {
        let tokens = &decl.tokens;
        let word = |index: usize, word: &str| tokens.get(index).is_some_and(|token| token.is_word(word));
        let symbol = |index: usize, symbol: &str| tokens.get(index).is_some_and(|token| token.is_symbol(symbol));
        // The names in parentheses after `class` or `interface`.
        let parents = |start: usize| -> Vec<String> {
            let mut names = Vec::new();
            if symbol(start, "(") {
                let mut index = start + 1;
                while index < tokens.len() && !tokens[index].is_symbol(")") {
                    if let Some(name) = tokens[index].ident() {
                        names.push(lower(name));
                    }
                    index += 1;
                }
            }
            names
        };
        let kind = if !decl.enum_values.is_empty() {
            for value in &decl.enum_values {
                self.values.insert(lower(value), Ty::Named(lower(&decl.name)));
            }
            TypeKind::Enum(decl.enum_values.clone())
        } else if word(0, "interface") {
            // A forward declaration must not replace the full one.
            if tokens.len() == 1 && self.types.contains_key(&lower(&decl.name)) {
                return;
            }
            TypeKind::Interface(parents(1).into_iter().next())
        } else if word(0, "class") && !word(1, "of") {
            if tokens.len() == 1 && self.types.contains_key(&lower(&decl.name)) {
                return;
            }
            TypeKind::Class(parents(1))
        } else if (word(0, "reference") && word(1, "to")) || word(0, "function") || word(0, "procedure") {
            match callback_signature(tokens) {
                Some(routine) => TypeKind::Callback(Box::new(routine)),
                None => TypeKind::Other,
            }
        } else if (word(0, "array") && word(1, "of") && tokens.len() == 3)
            || (word(0, "tarray") && symbol(1, "<") && tokens.len() == 4)
        {
            // `array of T` and `TArray<T>` have the element type as third token.
            match tokens[2].ident() {
                Some(element) => TypeKind::Alias(Ty::Array(Box::new(self.ty_of_name(element)))),
                None => TypeKind::Other,
            }
        } else if symbol(0, "^") && tokens.len() == 2 && tokens[1].ident().is_some() {
            TypeKind::Pointer(tokens[1].ident().unwrap_or_default().to_owned())
        } else if word(0, "set") && word(1, "of") && tokens.len() == 3 && tokens[2].ident().is_some() {
            TypeKind::Set(lower(tokens[2].ident().unwrap_or_default()))
        } else if tokens.len() == 1 && tokens[0].ident().is_some() {
            TypeKind::Alias(self.ty_of_name(tokens[0].ident().unwrap_or_default()))
        } else {
            TypeKind::Other
        };
        if matches!(kind, TypeKind::Interface(_) | TypeKind::Class(_)) {
            self.members.insert(lower(&decl.name), scan_members(tokens));
            for mut method in scan_methods(tokens) {
                if method.kind == RoutineKind::Constructor {
                    method.return_type = Some(TypeRef::Named(decl.name.clone()));
                }
                self.routines
                    .entry(format!("{}.{}", lower(&decl.name), lower(&method.name)))
                    .or_default()
                    .push(RoutineSig {
                        name: format!("{}.{}", decl.name, method.name),
                        params: method.params,
                        return_type: method.return_type,
                        unit: String::new(),
                        line: method.line,
                    });
            }
        }
        self.types.insert(
            lower(&decl.name),
            TypeInfo {
                name: decl.name.clone(),
                kind,
            },
        );
    }

    /// The type that a type name stands for.
    pub fn ty_of_name(&self, name: &str) -> Ty {
        let name = lower(name.rsplit('.').next().unwrap_or(name));
        match name.as_str() {
            "string" | "ansistring" | "unicodestring" | "widestring" | "char" | "ansichar" | "widechar" => Ty::Str,
            "twbsignature" => Ty::Sig,
            "boolean" => Ty::Bool,
            "integer" | "cardinal" | "int64" | "uint64" | "byte" | "word" | "smallint" | "shortint" | "nativeint"
            | "nativeuint" | "longint" | "longword" => Ty::Int,
            "extended" | "double" | "single" => Ty::Float,
            "variant" => Ty::Unknown,
            _ => match self.types.get(&name) {
                Some(TypeInfo {
                    kind: TypeKind::Alias(ty),
                    ..
                }) => ty.clone(),
                _ => Ty::Named(name),
            },
        }
    }

    pub fn ty_of_type_ref(&self, type_ref: &TypeRef) -> Ty {
        match type_ref {
            TypeRef::Named(name) => self.ty_of_name(name),
            TypeRef::ArrayOf(element) => Ty::Array(Box::new(self.ty_of_type_ref(element))),
            TypeRef::ArrayOfConst => Ty::ArrayOfConst,
            TypeRef::Generic { name, args } if name.eq_ignore_ascii_case("TArray") && args.len() == 1 => {
                Ty::Array(Box::new(self.ty_of_type_ref(&args[0])))
            }
            TypeRef::Generic { .. } | TypeRef::Other(_) => Ty::Unknown,
        }
    }

    /// The type of a constant without declared type.
    fn literal_ty(&self, value: &Expr) -> Ty {
        match value {
            Expr::Str(_) => Ty::Str,
            Expr::Int { .. } => Ty::Int,
            Expr::Float(_) => Ty::Float,
            Expr::Ident(name) if name.eq_ignore_ascii_case("true") || name.eq_ignore_ascii_case("false") => Ty::Bool,
            Expr::Unary { operand, .. } | Expr::Paren(operand) => self.literal_ty(operand),
            Expr::Binary { left, .. } => self.literal_ty(left),
            _ => Ty::Unknown,
        }
    }

    /// Whether the interface or class `name` is `ancestor` or descends from it.
    pub fn descends_from(&self, name: &str, ancestor: &str) -> bool {
        let mut pending = vec![lower(name)];
        let mut seen = Vec::new();
        while let Some(current) = pending.pop() {
            if current == ancestor {
                return true;
            }
            if seen.contains(&current) {
                continue;
            }
            match self.types.get(&current).map(|info| &info.kind) {
                Some(TypeKind::Interface(Some(parent))) => pending.push(parent.clone()),
                Some(TypeKind::Class(parents)) => pending.extend(parents.iter().cloned()),
                _ => {}
            }
            seen.push(current);
        }
        false
    }

    /// The type of the function or property `member` of the interface or
    /// class `name`, looking through its ancestors.
    pub fn member_ty(&self, name: &str, member: &str) -> Option<Ty> {
        let member = lower(member);
        let mut pending = vec![lower(name)];
        let mut seen = Vec::new();
        while let Some(current) = pending.pop() {
            if seen.contains(&current) {
                continue;
            }
            if let Some(type_name) = self.members.get(&current).and_then(|members| members.get(&member)) {
                return Some(self.ty_of_name(type_name));
            }
            match self.types.get(&current).map(|info| &info.kind) {
                Some(TypeKind::Interface(Some(parent))) => pending.push(parent.clone()),
                Some(TypeKind::Class(parents)) => pending.extend(parents.iter().cloned()),
                _ => {}
            }
            seen.push(current);
        }
        None
    }

    /// The lower-case name of the interface or class that declares the
    /// method `member`: `name` or one of its ancestors.
    pub fn method_owner(&self, name: &str, member: &str) -> Option<String> {
        let member = lower(member);
        let mut pending = vec![lower(name)];
        let mut seen = Vec::new();
        while let Some(current) = pending.pop() {
            if seen.contains(&current) {
                continue;
            }
            if self.routines.contains_key(&format!("{current}.{member}")) {
                return Some(current);
            }
            match self.types.get(&current).map(|info| &info.kind) {
                Some(TypeKind::Interface(Some(parent))) => pending.push(parent.clone()),
                Some(TypeKind::Class(parents)) => pending.extend(parents.iter().cloned()),
                _ => {}
            }
            seen.push(current);
        }
        None
    }

    pub fn callback(&self, name: &str) -> Option<&Routine> {
        match self.types.get(&lower(name)).map(|info| &info.kind) {
            Some(TypeKind::Callback(routine)) => Some(routine),
            _ => None,
        }
    }
}

/// The functions and properties in the tokens of an interface or class
/// body, each with the name of its type. Of overloads the first one counts.
fn scan_members(tokens: &[Token]) -> HashMap<String, String> {
    let mut members = HashMap::new();
    let mut index = 0;
    while index < tokens.len() {
        let declares = tokens[index].is_word("function") || tokens[index].is_word("property");
        let Some(name) = tokens.get(index + 1).and_then(Token::ident).filter(|_| declares) else {
            index += 1;
            continue;
        };
        // Skip the parameters of a function or the index of a property.
        let mut cursor = index + 2;
        if tokens
            .get(cursor)
            .is_some_and(|token| token.is_symbol("(") || token.is_symbol("["))
        {
            let mut depth = 0;
            while cursor < tokens.len() {
                let token = &tokens[cursor];
                cursor += 1;
                if token.is_symbol("(") || token.is_symbol("[") {
                    depth += 1;
                } else if token.is_symbol(")") || token.is_symbol("]") {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
        }
        if tokens.get(cursor).is_some_and(|token| token.is_symbol(":"))
            && let Some(type_name) = tokens.get(cursor + 1).and_then(Token::ident)
        {
            members.entry(lower(name)).or_insert_with(|| type_name.to_owned());
        }
        index = cursor;
    }
    members
}

/// The methods and constructors in the tokens of an interface or class body.
fn scan_methods(tokens: &[Token]) -> Vec<Routine> {
    let mut methods = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let starts = ["function", "procedure", "constructor"]
            .iter()
            .any(|word| tokens[index].is_word(word));
        if !starts || tokens.get(index + 1).and_then(Token::ident).is_none() {
            index += 1;
            continue;
        }
        // The header ends at the first `;` outside of parentheses.
        let mut end = index;
        let mut depth = 0;
        while end < tokens.len() {
            let token = &tokens[end];
            if token.is_symbol("(") || token.is_symbol("[") {
                depth += 1;
            } else if token.is_symbol(")") || token.is_symbol("]") {
                depth -= 1;
            } else if token.is_symbol(";") && depth == 0 {
                break;
            }
            end += 1;
        }
        let line = tokens[index].line;
        let mut header = tokens[index..end].to_vec();
        header.push(Token {
            kind: TokenKind::Symbol(";"),
            line,
        });
        header.push(Token {
            kind: TokenKind::Eof,
            line,
        });
        if let Ok(routine) = Parser::new(header).parse_routine_header() {
            methods.push(routine);
        }
        index = end.max(index + 1);
    }
    methods
}

/// Parses the tokens of a procedural type: `reference to function(...): T`,
/// `function(...): T` or `procedure(...) of object`.
fn callback_signature(tokens: &[Token]) -> Option<Routine> {
    let start = if tokens.first()?.is_word("reference") { 2 } else { 0 };
    let is_function = tokens.get(start)?.is_word("function");
    // Reuse the routine parser: give the type a name and a closing `;`.
    let line = tokens[0].line;
    let mut header = vec![
        tokens[start].clone(),
        Token {
            kind: TokenKind::Ident("Callback".to_owned()),
            line,
        },
    ];
    for token in &tokens[start + 1..] {
        if token.is_word("of") {
            break;
        }
        header.push(token.clone());
    }
    header.push(Token {
        kind: TokenKind::Symbol(";"),
        line,
    });
    header.push(Token {
        kind: TokenKind::Eof,
        line,
    });
    let routine = Parser::new(header).parse_routine_header().ok()?;
    (routine.kind
        == if is_function {
            RoutineKind::Function
        } else {
            RoutineKind::Procedure
        })
    .then_some(routine)
}
