// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Rust output of the transpiler.
//!
//! Rust has no overloads and no default parameters. Every Pascal overload
//! becomes one Rust function with its own name and with all parameters, and
//! every call names the function it resolves to and passes every argument.
//!
//! A value of a Pascal interface type can be `nil`, and the definitions pass
//! `nil` members on purpose. Every such value is an `Option<Arc<...>>` here.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Result, anyhow, bail, ensure};

use super::model::{RoutineSig, Symbols, Ty, TypeKind};
use super::resolve::{Resolver, Scope};
use crate::pascal::ast::{Decl, Expr, Param, Routine, Stmt, TypeRef, VarDecl};
use crate::pascal::lexer::{Token, TokenKind};
use crate::pascal::parser::Parser;

/// The Rust types of the interfaces that are not the struct with the name
/// without its `Iwb` prefix: the traits, and the interfaces whose classes
/// are one struct on the Rust side.
const TYPES: &[(&str, &str)] = &[
    ("iwbdef", "dyn Def"),
    ("iwbnameddef", "dyn NamedDef"),
    ("iwbvaluedef", "dyn ValueDef"),
    ("iwbintegerdef", "dyn IntegerDefInterface"),
    ("iwbintegerdefformater", "dyn IntegerDefFormater"),
    ("iwbsignaturedef", "dyn SignatureDef"),
    ("iwbrecordmemberdef", "dyn RecordMemberDef"),
    ("iwbrecorddef", "dyn RecordDef"),
    ("iwbresolvabledef", "dyn ResolvableDef"),
    ("iwbstringdefformater", "dyn StringDefFormater"),
    ("iwbemptydef", "dyn EmptyDefInterface"),
    ("iwbelement", "dyn Element"),
    ("iwbcontainer", "dyn Container"),
    ("iwbcontainerelementref", "dyn Container"),
    ("iwbdatacontainer", "dyn DataContainer"),
    ("iwbmainrecord", "dyn MainRecord"),
    ("iwbfile", "dyn File"),
    ("iwbchar4", "dyn IntegerDefFormater"),
    ("iwbrefid", "dyn IntegerDefFormater"),
    ("iwbformid", "FormIDDefFormater"),
    ("iwbformidchecked", "FormIDDefFormater"),
    ("iwbkey2data6enumdef", "EnumDef"),
    ("iwbdata6key2enumdef", "EnumDef"),
    ("iwblstringdef", "StringDef"),
    ("iwblstringkcdef", "StringDef"),
    ("iwbstringlcdef", "StringDef"),
    ("iwbstringkcdef", "StringDef"),
    ("iwbstringscriptdef", "StringDef"),
    ("iwbstringmgefcodedef", "StringDef"),
    ("iwbstructcdef", "StructDef"),
    ("iwbstructzdef", "StructDef"),
    ("iwbstructlzdef", "StructDef"),
];

/// The Rust parameters and result of each callback type, as the aliases in
/// `xedit-core` declare them.
const CALLBACKS: &[(&str, &str)] = &[
    ("tproc", "()"),
    ("twbsubrecordforvaluecallback", "(v: Option<Arc<dyn ValueDef>>)"),
    ("twbaddinfocallback", "(a_main_record: &MainRecordRef) -> String"),
    ("twbafterloadcallback", "(a_element: &ElementRef)"),
    (
        "twbaftersetcallback",
        "(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant)",
    ),
    (
        "twbcountcallback",
        "(a_base_ptr: DataPtr, a_element: ElementArg) -> u32",
    ),
    ("twbdontshowcallback", "(a_element: ElementArg) -> bool"),
    ("twbfloatnormalizer", "(a_element: ElementArg, a_float: f64) -> f64"),
    (
        "twbgetconflictpriority",
        "(a_element: ElementArg, a_conflict_priority: &mut ConflictPriority)",
    ),
    ("twbintegerdefformateruniondecider", "(a_element: ElementArg) -> i32"),
    (
        "twbintoverlaycallback",
        "(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> i64",
    ),
    (
        "twbinttostrcallback",
        "(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String",
    ),
    ("twbisremovablecallback", "(a_element: ElementArg) -> bool"),
    ("twbissortedcallback", "(a_container: ElementArg) -> bool"),
    ("twblinkstocallback", "(a_element: ElementArg) -> Option<ElementRef>"),
    (
        "twbsettodefaultcallback",
        "(a_base_ptr: DataPtr, a_element: ElementArg) -> bool",
    ),
    (
        "twbshouldincludecallback",
        "(a_base_ptr: DataPtr, a_array: ElementArg) -> bool",
    ),
    ("twbstrtointcallback", "(a_string: &str, a_element: ElementArg) -> i64"),
    (
        "twbstructsizecallback",
        "(a_base_ptr: DataPtr, a_element: ElementArg) -> u32",
    ),
    (
        "twbtostrcallback",
        "(a_value: &mut String, a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType)",
    ),
    ("twbuniondecider", "(a_base_ptr: DataPtr, a_element: ElementArg) -> i32"),
    ("twbruniondecider", "(a_container: ElementArg) -> i32"),
    (
        "twbmainrecordgetformidcallback",
        "(a_main_record: &MainRecordRef) -> Option<FormID>",
    ),
    (
        "twbmainrecordseteditoridcallback",
        "(a_sub_record: &ElementRef, a_editor_id: &str)",
    ),
    (
        "twbmainrecordidentitycallback",
        "(a_main_record: &MainRecordRef) -> String",
    ),
    (
        "twbmainrecordgeteditoridcallback",
        "(a_sub_record: &ElementRef) -> String",
    ),
    (
        "twbmainrecordgetgridcellcallback",
        "(a_sub_record: &ElementRef) -> Option<GridCell>",
    ),
    (
        "twbbuildindexkeyscallback",
        "(a_main_record: &MainRecordRef, a_index_keys: &mut IndexKeys)",
    ),
];

/// The methods whose Rust name is not the Pascal name in snake case: the
/// lower-case interface that declares the method or `""` for any, the
/// method, the overload index, and the Rust name.
const METHODS: &[(&str, &str, usize, &str)] = &[
    ("", "includeflag", 0, "include_flag_when"),
    ("", "setcountpath", 1, "set_count_paths"),
    ("", "setcountpathonvalue", 1, "set_count_paths_on_value"),
    (
        "iwbsubrecordwithstructdef",
        "setsummarydelimiteronvalue",
        0,
        "set_summary_delimiter_on_struct",
    ),
    (
        "iwbsubrecordwitharraydef",
        "setsummarydelimiteronvalue",
        0,
        "set_summary_delimiter_on_array",
    ),
];

const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl",
    "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super",
    "trait", "true", "type", "unsafe", "use", "where", "while", "async", "await", "box", "final", "override", "priv",
    "virtual", "yield", "try", "gen", "abstract", "become", "do", "macro", "typeof", "unsized",
];

/// The Rust spelling of a Pascal identifier: `wbFormIDCk` is `wb_form_id_ck`.
pub fn snake(name: &str) -> String {
    let chars: Vec<char> = name.trim_start_matches('&').chars().collect();
    let mut out = String::new();
    for (index, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && index > 0 {
            let prev = chars[index - 1];
            let next_is_lower = chars.get(index + 1).is_some_and(|next| next.is_ascii_lowercase());
            if prev.is_ascii_lowercase() || prev.is_ascii_digit() || (prev.is_ascii_uppercase() && next_is_lower) {
                out.push('_');
            }
        }
        out.push(c.to_ascii_lowercase());
    }
    if KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

/// The Rust type of a Pascal scalar type name.
fn scalar(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "integer" | "longint" | "twbnamedindex" => "i32",
        "cardinal" | "longword" => "u32",
        "int64" => "i64",
        "uint64" => "u64",
        "byte" => "u8",
        "word" => "u16",
        "smallint" => "i16",
        "shortint" => "i8",
        "nativeint" => "isize",
        "nativeuint" => "usize",
        "extended" | "double" => "f64",
        "single" => "f32",
        "boolean" => "bool",
        _ => return None,
    })
}

/// What an expression has to become.
#[derive(Debug, Clone)]
struct Target {
    ty: Ty,
    /// A `String` or `Vec` instead of a `&str` or slice.
    owned: bool,
    /// The Rust type of a number.
    num: Option<&'static str>,
}

/// An emitted expression.
struct Val {
    code: String,
    ty: Ty,
    owned: bool,
    num: Option<&'static str>,
    /// The expression is a literal.
    literal: bool,
}

impl Val {
    fn new(code: impl Into<String>, ty: Ty) -> Self {
        Val {
            code: code.into(),
            ty,
            owned: false,
            num: None,
            literal: false,
        }
    }

    fn owned(mut self) -> Self {
        self.owned = true;
        self
    }

    fn num(mut self, num: Option<&'static str>) -> Self {
        self.num = num;
        self
    }
}

#[derive(Clone)]
struct Local {
    target: Target,
    is_param: bool,
    /// The Rust name, from the declaration. Pascal ignores case.
    rust: String,
}

/// The names of one routine body.
struct Context {
    scope: Scope,
    locals: HashMap<String, Local>,
    result: Option<Target>,
    /// The Rust name of the routine, for naming its anonymous routines.
    routine: String,
    /// The anonymous routines assigned to inline variables: lower-case name
    /// to the signature.
    closures: HashMap<String, RoutineSig>,
}

pub struct Emitter<'a> {
    pub symbols: &'a Symbols,
    /// The unit whose routines are written.
    pub unit: &'a str,
    /// Lower-case routine name and overload index to the Rust function name.
    pub names: HashMap<(String, usize), String>,
    /// The Rust functions that the output calls, with what they stand for.
    pub references: RefCell<BTreeMap<String, String>>,
    /// The callbacks that the output names: Rust name to the Pascal name,
    /// the lower-case callback type and the line.
    pub callbacks: RefCell<BTreeMap<String, (String, String, u32)>>,
    /// The callbacks of `wbDefinitionsCommon` that a game unit names, in the
    /// same form. Their stubs belong to the Common unit.
    pub common_callbacks: RefCell<BTreeMap<String, (String, String, u32)>>,
}

impl<'a> Emitter<'a> {
    pub fn new(symbols: &'a Symbols, unit: &'a str) -> Self {
        Emitter {
            symbols,
            unit,
            names: overload_names(symbols),
            references: RefCell::default(),
            callbacks: RefCell::default(),
            common_callbacks: RefCell::default(),
        }
    }

    fn resolver(&self) -> Resolver<'a> {
        Resolver { symbols: self.symbols }
    }

    /// The Rust name of an interface, class, enumeration, set or callback type.
    fn named(&self, name: &str) -> Result<String> {
        let lower = name.to_ascii_lowercase();
        if lower == "variant" {
            return Ok("Variant".to_owned());
        }
        if lower == "tvarrec" {
            return Ok("VarRec".to_owned());
        }
        let Some(info) = self.symbols.types.get(&lower) else {
            bail!("type {name} is not known");
        };
        let stripped = || {
            let declared = info.name.as_str();
            declared
                .strip_prefix("Twb")
                .or_else(|| declared.strip_prefix("Iwb"))
                .unwrap_or(declared)
                .to_owned()
        };
        Ok(match &info.kind {
            TypeKind::Enum(_) | TypeKind::Set(_) | TypeKind::Record(_) => stripped(),
            TypeKind::Callback(_) => format!("Option<{}>", stripped()),
            TypeKind::Pointer(pointee) => {
                format!("Option<&'static {}>", pointee.strip_prefix("Twb").unwrap_or(pointee))
            }
            TypeKind::Interface(_) | TypeKind::Class(_) => format!("Option<Arc<{}>>", self.pointee(&lower)),
            _ => bail!("type {name} has no Rust form"),
        })
    }

    /// The Rust type behind the `Arc` of an interface or class.
    fn pointee(&self, lower: &str) -> String {
        if let Some((_, rust)) = TYPES.iter().find(|(name, _)| *name == lower) {
            return (*rust).to_owned();
        }
        // The interfaces that only narrow the setters of a subrecord.
        if lower.starts_with("iwbsubrecordwith") {
            return "SubRecordDef".to_owned();
        }
        let declared = self.symbols.types.get(lower).map_or(lower, |info| info.name.as_str());
        declared
            .strip_prefix("Twb")
            .or_else(|| declared.strip_prefix("Iwb"))
            .unwrap_or(declared)
            .to_owned()
    }

    fn is_kind(&self, name: &str, test: impl Fn(&TypeKind) -> bool) -> bool {
        self.symbols.types.get(name).is_some_and(|info| test(&info.kind))
    }

    fn is_interface(&self, name: &str) -> bool {
        self.is_kind(name, |kind| matches!(kind, TypeKind::Interface(_) | TypeKind::Class(_)))
    }

    fn is_callback(&self, name: &str) -> bool {
        self.is_kind(name, |kind| matches!(kind, TypeKind::Callback(_)))
    }

    fn is_set(&self, name: &str) -> bool {
        self.is_kind(name, |kind| matches!(kind, TypeKind::Set(_)))
    }

    /// The target that a declared type stands for.
    fn target(&self, type_ref: &TypeRef, owned: bool) -> Target {
        let num = match type_ref {
            TypeRef::Named(name) => scalar(name),
            _ => None,
        };
        Target {
            ty: self.symbols.ty_of_type_ref(type_ref),
            owned,
            num,
        }
    }

    /// The Rust type of a target.
    fn rust_ty(&self, target: &Target) -> Result<String> {
        Ok(match &target.ty {
            Ty::Str if target.owned => "String".to_owned(),
            Ty::Str => "&str".to_owned(),
            Ty::Sig => "Signature".to_owned(),
            Ty::Bool => "bool".to_owned(),
            Ty::Int => target.num.unwrap_or("i32").to_owned(),
            Ty::Float => target.num.unwrap_or("f64").to_owned(),
            Ty::Named(name) => self.named(name)?,
            Ty::Array(element) => {
                let element = self.rust_ty(&Target {
                    ty: (**element).clone(),
                    owned: target.owned,
                    num: None,
                })?;
                if target.owned {
                    format!("Vec<{element}>")
                } else {
                    format!("&[{element}]")
                }
            }
            Ty::ArrayOfConst => "&[VarRec]".to_owned(),
            other => bail!("type {other:?} has no Rust form"),
        })
    }

    // ----- routines -----

    /// The Rust name of the overload that a routine implements.
    pub fn rust_name(&self, routine: &Routine) -> Option<String> {
        let lower = routine.name.to_ascii_lowercase();
        let overloads = self.symbols.routines.get(&lower)?;
        let index = overloads
            .iter()
            .position(|sig| same_params(&sig.params, &routine.params))?;
        self.names.get(&(lower, index)).cloned()
    }

    /// The Rust signature of a routine that is written by hand, with the
    /// Rust name of its overload.
    pub fn hand_signature(&self, routine: &Routine) -> Result<String> {
        let lower = routine.name.to_ascii_lowercase();
        let overloads = self
            .symbols
            .routines
            .get(&lower)
            .ok_or_else(|| anyhow!("routine is not in the symbol table"))?;
        let index = overloads
            .iter()
            .position(|sig| same_params(&sig.params, &routine.params))
            .ok_or_else(|| anyhow!("no declaration matches the implementation"))?;
        self.signature(&self.names[&(lower, index)], &overloads[index])
    }

    /// The Rust signature of a routine, for a function that is written by hand.
    pub fn signature(&self, function: &str, sig: &RoutineSig) -> Result<String> {
        let mut params = Vec::new();
        for param in &sig.params {
            let type_ref = param
                .type_ref
                .as_ref()
                .ok_or_else(|| anyhow!("untyped parameter {}", param.name))?;
            params.push(format!(
                "{}: {}",
                snake(&param.name),
                self.rust_ty(&self.target(type_ref, false))?
            ));
        }
        let returns = match &sig.return_type {
            Some(type_ref) => format!(" -> {}", self.rust_ty(&self.target(type_ref, true))?),
            None => String::new(),
        };
        Ok(format!("pub fn {function}({}){returns}", params.join(", ")))
    }

    /// The Rust function for one Pascal routine.
    pub fn routine(&self, routine: &Routine) -> Result<String> {
        let lower = routine.name.to_ascii_lowercase();
        let overloads = self
            .symbols
            .routines
            .get(&lower)
            .ok_or_else(|| anyhow!("routine is not in the symbol table"))?;
        let index = overloads
            .iter()
            .position(|sig| same_params(&sig.params, &routine.params))
            .ok_or_else(|| anyhow!("no declaration matches the implementation"))?;
        let sig = &overloads[index];
        let name = &self.names[&(lower, index)];
        let body = routine.body.as_ref().ok_or_else(|| anyhow!("no body"))?;
        if body.is_asm {
            bail!("assembler body");
        }

        let mut cx = Context {
            scope: super::routine_scope(routine, self.symbols),
            locals: HashMap::new(),
            result: None,
            routine: name.clone(),
            closures: HashMap::new(),
        };
        let mut params = Vec::new();
        // The defaults are in the declaration, the names in the implementation.
        for param in &routine.params {
            if !matches!(param.modifier.as_str(), "" | "const") {
                bail!("{} parameter {}", param.modifier, param.name);
            }
            let type_ref = param
                .type_ref
                .as_ref()
                .ok_or_else(|| anyhow!("untyped parameter {}", param.name))?;
            let target = self.target(type_ref, false);
            params.push(format!("{}: {}", snake(&param.name), self.rust_ty(&target)?));
            cx.locals.insert(
                param.name.to_ascii_lowercase(),
                Local {
                    target,
                    is_param: true,
                    rust: snake(&param.name),
                },
            );
        }
        let mut out = String::new();
        out.push_str(&format!(
            "/// Upstream `{}`, line {} of `{}.pas`.\n",
            sig.name, routine.line, sig.unit
        ));
        out.push_str(&format!("pub fn {name}({})", params.join(", ")));
        if let Some(return_type) = &routine.return_type {
            let target = self.target(return_type, true);
            out.push_str(&format!(" -> {}", self.rust_ty(&target)?));
            cx.result = Some(target);
        }
        out.push_str(" {\n");

        // A body that only assigns the result is the expression alone.
        if let ([Stmt::Assign { target, value, .. }], Some(result), true) =
            (body.statements.as_slice(), &cx.result, body.decls.is_empty())
            && matches!(target, Expr::Ident(name) if name.eq_ignore_ascii_case("result"))
        {
            out.push_str(&format!("    {}\n}}\n", self.expr_to(value, result, &cx)?));
            return Ok(out);
        }

        let assigned = assigned_names(&body.statements);
        for param in &routine.params {
            let lower = param.name.to_ascii_lowercase();
            if assigned.contains(&lower) {
                let name = snake(&param.name);
                let local = cx
                    .locals
                    .get_mut(&lower)
                    .ok_or_else(|| anyhow!("parameter {name} is not local"))?;
                match local.target.ty {
                    Ty::Str => {
                        local.target.owned = true;
                        local.is_param = false;
                        out.push_str(&format!("    let mut {name}: String = {name}.to_owned();\n"));
                    }
                    Ty::Array(_) => bail!("assignment to the array parameter {}", param.name),
                    _ => out.push_str(&format!("    let mut {name} = {name};\n")),
                }
            }
        }
        for decl in &body.decls {
            match decl {
                Decl::Var(var) => {
                    let type_ref = var.type_ref.as_ref().ok_or_else(|| anyhow!("variable without type"))?;
                    let target = self.target(type_ref, true);
                    let rust_ty = self.rust_ty(&target)?;
                    let initial = self.initial_value(&target)?;
                    for name in &var.names {
                        out.push_str(&format!("    let mut {}: {rust_ty} = {initial};\n", snake(name)));
                        cx.locals.insert(
                            name.to_ascii_lowercase(),
                            Local {
                                target: target.clone(),
                                is_param: false,
                                rust: snake(name),
                            },
                        );
                    }
                }
                _ => bail!("local declaration that is not a variable"),
            }
        }
        if let Some(result) = &cx.result {
            out.push_str(&format!(
                "    let mut result: {} = Default::default();\n",
                self.rust_ty(result)?
            ));
        }
        self.statements(&body.statements, 1, &mut cx, &mut out)?;
        if cx.result.is_some() {
            out.push_str("    result\n");
        }
        out.push_str("}\n");
        Ok(out)
    }

    // ----- statements -----

    fn statements(&self, statements: &[Stmt], depth: usize, cx: &mut Context, out: &mut String) -> Result<()> {
        for statement in statements {
            self.statement(statement, depth, cx, out)?;
        }
        Ok(())
    }

    fn statement(&self, statement: &Stmt, depth: usize, cx: &mut Context, out: &mut String) -> Result<()> {
        let pad = "    ".repeat(depth);
        match statement {
            Stmt::Empty => {}
            Stmt::Block(inner) => self.statements(inner, depth, cx, out)?,
            Stmt::Assign { target, value, .. } => {
                if let Expr::Ident(name) = target
                    && let Some(setter) = self.unit_var_setter(name, cx)
                {
                    let decl = &self.symbols.value_decls[&name.to_ascii_lowercase()];
                    let type_ref = decl.type_ref.as_ref().ok_or_else(|| anyhow!("{name} has no type"))?;
                    // The setters of the `wbInterface` globals borrow strings.
                    let target = self.target(type_ref, !setter.starts_with("set_"));
                    out.push_str(&format!("{pad}{setter}({});\n", self.expr_to(value, &target, cx)?));
                    return Ok(());
                }
                // `wbKnownSubRecordSignatures[ksrRole] := 'SIGN'`, which only
                // Morrowind does.
                if let Expr::Index { base, args } = target
                    && let [index] = args.as_slice()
                    && let Expr::Ident(name) = &**base
                    && name.eq_ignore_ascii_case("wbKnownSubRecordSignatures")
                    && let Expr::Ident(role) = index
                    && let Expr::Str(signature) = value
                {
                    out.push_str(&format!(
                        "{pad}set_known_sub_record_signature(KnownSubRecord::{role}, Signature::new(b\"{signature}\"));\n"
                    ));
                    return Ok(());
                }
                if let Expr::Index { base, args } = target
                    && let [index] = args.as_slice()
                    && let Expr::Ident(name) = &**base
                    && let Some(setter) = self.unit_var_setter(name, cx)
                {
                    let decl = &self.symbols.value_decls[&name.to_ascii_lowercase()];
                    let type_ref = decl.type_ref.as_ref().ok_or_else(|| anyhow!("{name} has no type"))?;
                    let Ty::Array(element) = self.symbols.ty_of_type_ref(type_ref) else {
                        bail!("index into {name}, which is not an array")
                    };
                    let element = Target {
                        ty: *element,
                        owned: false,
                        num: None,
                    };
                    let index = self.expr(index, cx)?;
                    out.push_str(&format!(
                        "{pad}{setter}_at({}, {});\n",
                        index.code,
                        self.expr_to(value, &element, cx)?
                    ));
                    return Ok(());
                }
                // The index is computed before the element is borrowed.
                if let Expr::Index { base, args } = target
                    && let [index] = args.as_slice()
                {
                    let (place, target) = self.place(base, cx)?;
                    let Ty::Array(element) = target.ty else {
                        bail!("index into a value that is not an array")
                    };
                    let element = Target {
                        ty: *element,
                        owned: true,
                        num: None,
                    };
                    let index = self.expr(index, cx)?;
                    out.push_str(&format!(
                        "{pad}{{\n{pad}    let index = ({}) as usize;\n{pad}    {place}[index] = {};\n{pad}}}\n",
                        index.code,
                        self.expr_to(value, &element, cx)?
                    ));
                    return Ok(());
                }
                let (place, target) = self.place(target, cx)?;
                out.push_str(&format!("{pad}{place} = {};\n", self.expr_to(value, &target, cx)?));
            }
            Stmt::Expr { expr, .. } => match expr {
                Expr::Ident(name) if name.eq_ignore_ascii_case("exit") => {
                    out.push_str(&format!(
                        "{pad}return{};\n",
                        if cx.result.is_some() { " result" } else { "" }
                    ));
                }
                Expr::Call { callee, args } if matches!(&**callee, Expr::Ident(name) if name.eq_ignore_ascii_case("exit")) =>
                {
                    let result = cx
                        .result
                        .clone()
                        .ok_or_else(|| anyhow!("Exit with value in a procedure"))?;
                    let [value] = args.as_slice() else {
                        bail!("Exit with {} arguments", args.len())
                    };
                    out.push_str(&format!("{pad}return {};\n", self.expr_to(value, &result, cx)?));
                }
                Expr::Call { callee, args } if matches!(&**callee, Expr::Ident(name) if name.eq_ignore_ascii_case("inc") || name.eq_ignore_ascii_case("dec")) =>
                {
                    let Expr::Ident(name) = &**callee else { unreachable!() };
                    let operator = if name.eq_ignore_ascii_case("inc") { "+=" } else { "-=" };
                    let (place, target) = self.place(&args[0], cx)?;
                    let amount = match args.get(1) {
                        Some(amount) => self.expr_to(amount, &target, cx)?,
                        None => "1".to_owned(),
                    };
                    out.push_str(&format!("{pad}{place} {operator} {amount};\n"));
                }
                Expr::Call { callee, args } if matches!(&**callee, Expr::Ident(name) if name.eq_ignore_ascii_case("setlength")) =>
                {
                    let length = self.expr(&args[1], cx)?;
                    if let Expr::Ident(name) = &args[0]
                        && let Some(setter) = self.unit_var_setter(name, cx)
                    {
                        out.push_str(&format!("{pad}{setter}_length({});\n", length.code));
                        return Ok(());
                    }
                    let (place, _) = self.place(&args[0], cx)?;
                    out.push_str(&format!(
                        "{pad}{place}.resize(({}) as usize, Default::default());\n",
                        length.code
                    ));
                }
                _ => {
                    let value = self.expr(expr, cx)?;
                    out.push_str(&format!("{pad}{};\n", value.code));
                }
            },
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                out.push_str(&format!("{pad}if {} {{\n", self.condition(condition, cx)?));
                self.statement(then_branch, depth + 1, cx, out)?;
                if let Some(else_branch) = else_branch {
                    out.push_str(&format!("{pad}}} else {{\n"));
                    self.statement(else_branch, depth + 1, cx, out)?;
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::For {
                variable,
                from,
                to,
                down,
                body,
            } => {
                // `for var i := ...` declares the variable in the loop.
                let local = cx
                    .locals
                    .get(&variable.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or_else(|| Local {
                        target: Target {
                            ty: Ty::Int,
                            owned: false,
                            num: Some("i32"),
                        },
                        is_param: false,
                        rust: snake(variable),
                    });
                cx.locals.insert(variable.to_ascii_lowercase(), local.clone());
                cx.scope.insert(variable, Ty::Int);
                let variable = &local.rust;
                let from = self.expr_to(from, &local.target, cx)?;
                let to = self.expr_to(to, &local.target, cx)?;
                let range = if *down {
                    format!("(({to})..=({from})).rev()")
                } else {
                    format!("({from})..=({to})")
                };
                out.push_str(&format!("{pad}for {variable} in {range} {{\n"));
                self.statement(body, depth + 1, cx, out)?;
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::While { condition, body } => {
                out.push_str(&format!("{pad}while {} {{\n", self.condition(condition, cx)?));
                self.statement(body, depth + 1, cx, out)?;
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::Var(var) if matches!(&var.value, Some(Expr::Anonymous(_))) => {
                let Some(Expr::Anonymous(routine)) = &var.value else {
                    unreachable!()
                };
                let [name] = var.names.as_slice() else {
                    bail!("inline variable with several names")
                };
                // A closure in a variable of a callback type is passed on as
                // a callback, so it is written by hand like an anonymous
                // routine that is passed directly.
                let typed_callback = var.type_ref.as_ref().is_some_and(
                    |type_ref| matches!(type_ref, TypeRef::Named(type_name) if self.is_callback(&type_name.to_ascii_lowercase())),
                );
                let translated = if typed_callback {
                    Err(anyhow!("a closure of a callback type is written by hand"))
                } else {
                    self.closure(routine, cx, depth)
                };
                let closure = match translated {
                    Ok(closure) => closure,
                    // A closure that cannot be translated is written by hand
                    // when it is a callback; a plain closure is left out, and
                    // the closures that call it become hand-written as well.
                    Err(error) => {
                        let callback = var.type_ref.as_ref().and_then(|type_ref| match type_ref {
                            TypeRef::Named(type_name) if self.is_callback(&type_name.to_ascii_lowercase()) => {
                                Some(type_name.clone())
                            }
                            _ => None,
                        });
                        let Some(callback) = callback else {
                            out.push_str(&format!(
                                "{pad}// `{name}` is not translated ({error}); the callbacks that call it are written by hand.
"
                            ));
                            return Ok(());
                        };
                        let function = format!("{}_anonymous_{}", cx.routine, routine.line);
                        self.callbacks.borrow_mut().insert(
                            function.clone(),
                            (
                                format!("anonymous routine in {}", cx.routine),
                                callback.to_ascii_lowercase(),
                                routine.line,
                            ),
                        );
                        out.push_str(&format!(
                            "{pad}let {}: {} = Some(Arc::new({function}));\n",
                            snake(name),
                            self.named(&callback)?
                        ));
                        let target = Target {
                            ty: Ty::Named(callback.to_ascii_lowercase()),
                            owned: false,
                            num: None,
                        };
                        cx.scope.insert(name, target.ty.clone());
                        cx.locals.insert(
                            name.to_ascii_lowercase(),
                            Local {
                                target,
                                is_param: false,
                                rust: snake(name),
                            },
                        );
                        return Ok(());
                    }
                };
                out.push_str(&format!("{pad}let {} = {closure};\n", snake(name)));
                cx.closures.insert(
                    name.to_ascii_lowercase(),
                    RoutineSig {
                        name: name.clone(),
                        params: routine.params.clone(),
                        return_type: routine.return_type.clone(),
                        unit: String::new(),
                        line: routine.line,
                    },
                );
            }
            Stmt::Try {
                body,
                handlers,
                except,
                finally: None,
            } if handlers.is_empty() && except.as_ref().is_some_and(Vec::is_empty) => {
                // UPSTREAM-QUIRK: `try ... except end` swallows every
                // exception upstream. The body runs without that net here.
                self.statements(body, depth, cx, out)?;
            }
            Stmt::Case {
                selector,
                arms,
                else_branch,
            } => {
                let selector = self.expr(selector, cx)?;
                let target = Target {
                    ty: selector.ty.clone(),
                    owned: false,
                    num: selector.num,
                };
                out.push_str(&format!(
                    "{pad}match {} {{
",
                    selector.code
                ));
                for arm in arms {
                    let labels = arm
                        .labels
                        .iter()
                        .map(|label| match label {
                            Expr::Range(low, high) => Ok(format!(
                                "{}..={}",
                                self.expr_to(low, &target, cx)?,
                                self.expr_to(high, &target, cx)?
                            )),
                            other => self.expr_to(other, &target, cx),
                        })
                        .collect::<Result<Vec<_>>>()?;
                    out.push_str(&format!(
                        "{pad}    {} => {{
",
                        labels.join(" | ")
                    ));
                    self.statement(&arm.body, depth + 2, cx, out)?;
                    out.push_str(&format!(
                        "{pad}    }}
"
                    ));
                }
                out.push_str(&format!(
                    "{pad}    _ => {{
"
                ));
                if let Some(else_branch) = else_branch {
                    self.statements(else_branch, depth + 2, cx, out)?;
                }
                out.push_str(&format!(
                    "{pad}    }}
{pad}}}
"
                ));
            }
            Stmt::Raise(exception) => {
                // `raise Exception.Create('message')`
                let message = match exception.as_ref() {
                    Some(Expr::Call { callee, args }) if matches!(&**callee, Expr::Member { name, .. } if name.eq_ignore_ascii_case("create")) => {
                        match args.first() {
                            Some(Expr::Str(text)) => format!("{text:?}"),
                            _ => "\"exception\"".to_owned(),
                        }
                    }
                    _ => "\"exception\"".to_owned(),
                };
                out.push_str(&format!("{pad}panic!({message});\n"));
            }
            Stmt::Var(var) => {
                let target = match (&var.type_ref, &var.value) {
                    (Some(type_ref), _) => self.target(type_ref, true),
                    (None, Some(Expr::List(items))) if !items.is_empty() => {
                        let types = items
                            .iter()
                            .map(|item| self.expr(item, cx).map(|value| value.ty))
                            .collect::<Result<Vec<_>>>()?;
                        let element = match self.common_interface(&types) {
                            Some(common) => Ty::Named(common),
                            None => types[0].clone(),
                        };
                        Target {
                            ty: Ty::Array(Box::new(element)),
                            owned: true,
                            num: None,
                        }
                    }
                    (None, Some(value)) => {
                        let value = self.expr(value, cx)?;
                        Target {
                            ty: value.ty,
                            owned: true,
                            num: value.num,
                        }
                    }
                    (None, None) => bail!("inline variable without type and value"),
                };
                let [name] = var.names.as_slice() else {
                    bail!("inline variable with several names")
                };
                let value = match &var.value {
                    Some(value) => self.expr_to(value, &target, cx)?,
                    None => self.initial_value(&target)?,
                };
                out.push_str(&format!(
                    "{pad}let mut {}: {} = {value};\n",
                    snake(name),
                    self.rust_ty(&target)?,
                ));
                cx.scope.insert(name, target.ty.clone());
                cx.locals.insert(
                    name.to_ascii_lowercase(),
                    Local {
                        target,
                        is_param: false,
                        rust: snake(name),
                    },
                );
            }
            other => bail!("statement {} is not supported", statement_name(other)),
        }
        Ok(())
    }

    /// The Rust function that assigns the unit variable `name`, when it is one.
    fn unit_var_setter(&self, name: &str, cx: &Context) -> Option<String> {
        let lower = name.to_ascii_lowercase();
        if lower == "result" || cx.locals.contains_key(&lower) || !self.symbols.value_decls.contains_key(&lower) {
            return None;
        }
        if self.symbols.consts.contains(&lower) {
            return None;
        }
        Some(
            if self.symbols.value_units.get(&lower).map(String::as_str) == Some("wbinterface") {
                let code = format!("set_{}", snake(strip_wb(name)));
                self.references
                    .borrow_mut()
                    .insert(code.clone(), format!("setter of {name}"));
                code
            } else {
                format!("{}.set", static_name(&self.declared_name(name)))
            },
        )
    }

    /// The nearest interface that all of the interface types descend from.
    fn common_interface(&self, types: &[Ty]) -> Option<String> {
        let names: Vec<&str> = types
            .iter()
            .map(|ty| match ty {
                Ty::Named(name) if self.is_interface(name) => Some(name.as_str()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        let mut candidate = names[0].to_owned();
        loop {
            if names.iter().all(|name| self.symbols.descends_from(name, &candidate)) {
                return Some(candidate);
            }
            match self.symbols.types.get(&candidate).map(|info| &info.kind) {
                Some(TypeKind::Interface(Some(parent))) => candidate = parent.clone(),
                _ => return None,
            }
        }
    }

    /// The value of a variable before the body assigns it.
    fn initial_value(&self, target: &Target) -> Result<String> {
        if let Ty::Named(name) = &target.ty
            && let Some(TypeKind::Enum(values)) = self.symbols.types.get(name).map(|info| &info.kind)
            && let Some(first) = values.first()
        {
            return Ok(format!("{}::{first}", self.named(name)?));
        }
        Ok("Default::default()".to_owned())
    }

    /// A Rust closure for an anonymous routine, with the locals of the
    /// enclosing routine in scope.
    fn closure(&self, routine: &Routine, outer: &Context, depth: usize) -> Result<String> {
        let body = routine
            .body
            .as_ref()
            .ok_or_else(|| anyhow!("anonymous routine without body"))?;
        let mut cx = Context {
            scope: outer.scope.clone(),
            locals: outer.locals.clone(),
            result: None,
            routine: outer.routine.clone(),
            closures: outer.closures.clone(),
        };
        let mut params = Vec::new();
        for param in &routine.params {
            if !matches!(param.modifier.as_str(), "" | "const") {
                bail!("{} parameter {}", param.modifier, param.name);
            }
            let type_ref = param
                .type_ref
                .as_ref()
                .ok_or_else(|| anyhow!("untyped parameter {}", param.name))?;
            let target = self.target(type_ref, false);
            cx.scope.insert(&param.name, target.ty.clone());
            params.push(format!("{}: {}", snake(&param.name), self.rust_ty(&target)?));
            cx.locals.insert(
                param.name.to_ascii_lowercase(),
                Local {
                    target,
                    is_param: true,
                    rust: snake(&param.name),
                },
            );
        }
        let pad = "    ".repeat(depth);
        let mut out = format!("|{}|", params.join(", "));
        if let Some(return_type) = &routine.return_type {
            let target = self.target(return_type, true);
            out.push_str(&format!(" -> {}", self.rust_ty(&target)?));
            cx.result = Some(target);
        }
        out.push_str(" {\n");
        for decl in &body.decls {
            let Decl::Var(var) = decl else {
                bail!("local declaration that is not a variable")
            };
            let type_ref = var.type_ref.as_ref().ok_or_else(|| anyhow!("variable without type"))?;
            let target = self.target(type_ref, true);
            let rust_ty = self.rust_ty(&target)?;
            let initial = self.initial_value(&target)?;
            for name in &var.names {
                out.push_str(&format!("{pad}    let mut {}: {rust_ty} = {initial};\n", snake(name)));
                cx.scope.insert(name, target.ty.clone());
                cx.locals.insert(
                    name.to_ascii_lowercase(),
                    Local {
                        target: target.clone(),
                        is_param: false,
                        rust: snake(name),
                    },
                );
            }
        }
        if let Some(result) = &cx.result {
            out.push_str(&format!(
                "{pad}    let mut result: {} = Default::default();\n",
                self.rust_ty(result)?
            ));
        }
        self.statements(&body.statements, depth + 1, &mut cx, &mut out)?;
        if cx.result.is_some() {
            out.push_str(&format!("{pad}    result\n"));
        }
        out.push_str(&format!("{pad}}}"));
        Ok(out)
    }

    fn condition(&self, condition: &Expr, cx: &Context) -> Result<String> {
        self.expr_to(
            condition,
            &Target {
                ty: Ty::Bool,
                owned: false,
                num: None,
            },
            cx,
        )
    }

    /// The place that an assignment writes, with what it holds.
    fn place(&self, target: &Expr, cx: &Context) -> Result<(String, Target)> {
        match target {
            Expr::Ident(name) if name.eq_ignore_ascii_case("result") => {
                let result = cx.result.clone().ok_or_else(|| anyhow!("Result in a procedure"))?;
                Ok(("result".to_owned(), result))
            }
            Expr::Ident(name) => match cx.locals.get(&name.to_ascii_lowercase()) {
                Some(local) => Ok((local.rust.clone(), local.target.clone())),
                None => bail!("assignment to {name}, which is not local"),
            },
            Expr::Index { base, args } => {
                let (place, target) = self.place(base, cx)?;
                let Ty::Array(element) = target.ty else {
                    bail!("index into a value that is not an array")
                };
                let [index] = args.as_slice() else {
                    bail!("index with several values")
                };
                let index = self.expr(index, cx)?;
                Ok((
                    format!("{place}[({}) as usize]", index.code),
                    Target {
                        ty: *element,
                        owned: true,
                        num: None,
                    },
                ))
            }
            other => bail!("assignment to {}", expr_name(other)),
        }
    }

    // ----- expressions -----

    /// The expression as a value of `target`.
    fn expr_to(&self, expr: &Expr, target: &Target, cx: &Context) -> Result<String> {
        match (expr, &target.ty) {
            (Expr::Paren(inner), _) => return self.expr_to(inner, target, cx),
            (Expr::Str(text), Ty::Sig) if text.chars().count() == 4 && text.is_ascii() => {
                return Ok(format!("Signature::new(b\"{}\")", byte_string(text)));
            }
            (Expr::List(items), Ty::Array(element)) => {
                let element = Target {
                    ty: (**element).clone(),
                    owned: target.owned,
                    num: None,
                };
                let items = items
                    .iter()
                    .map(|item| self.expr_to(item, &element, cx))
                    .collect::<Result<Vec<_>>>()?;
                return Ok(if target.owned {
                    format!("vec![{}]", items.join(", "))
                } else {
                    format!("&[{}]", items.join(", "))
                });
            }
            (Expr::List(items), Ty::ArrayOfConst) => {
                let mut out = Vec::new();
                for item in items {
                    let value = self.expr(item, cx)?;
                    out.push(match value.ty {
                        Ty::Int => format!("VarRec::Int(({}) as i64)", value.code),
                        Ty::Str if value.owned => format!("VarRec::Str({})", value.code),
                        Ty::Str => format!("VarRec::Str({}.to_owned())", value.code),
                        Ty::Bool => format!("VarRec::Bool({})", value.code),
                        other => bail!("{other:?} in an array of const"),
                    });
                }
                return Ok(format!("&[{}]", out.join(", ")));
            }
            (Expr::List(items), Ty::Named(name)) if self.is_set(name) => {
                let TypeKind::Set(element) = &self.symbols.types[name].kind else {
                    unreachable!()
                };
                let element = Target {
                    ty: Ty::Named(element.clone()),
                    owned: false,
                    num: None,
                };
                let items = items
                    .iter()
                    .map(|item| self.expr_to(item, &element, cx))
                    .collect::<Result<Vec<_>>>()?;
                return Ok(format!("EnumSet::of(&[{}])", items.join(", ")));
            }
            // An anonymous routine is a function that is written by hand.
            (Expr::Anonymous(routine), Ty::Named(callback)) if self.is_callback(callback) => {
                let function = format!("{}_anonymous_{}", cx.routine, routine.line);
                self.callbacks.borrow_mut().insert(
                    function.clone(),
                    (
                        format!("anonymous routine in {}", cx.routine),
                        callback.to_ascii_lowercase(),
                        routine.line,
                    ),
                );
                return Ok(format!("Some(Arc::new({function}))"));
            }
            // A routine that is named where a callback is expected.
            (Expr::Ident(name), Ty::Named(callback)) if self.is_callback(callback) && cx.scope.get(name).is_none() => {
                if let Some(function) = self.callback_function(name, callback) {
                    return Ok(format!("Some(Arc::new({function}))"));
                }
            }
            _ => {}
        }
        let value = self.expr(expr, cx)?;
        self.convert(value, target)
    }

    /// The Rust function behind a routine that is passed as the callback type.
    fn callback_function(&self, name: &str, callback_type: &str) -> Option<String> {
        let lower = name.to_ascii_lowercase();
        let overloads = self.symbols.routines.get(&lower)?;
        let callback = self.symbols.callback(callback_type)?;
        let index = overloads
            .iter()
            .position(|sig| super::resolve::callback_matches(sig, callback))?;
        let function = self.names[&(lower, index)].clone();
        self.references
            .borrow_mut()
            .insert(function.clone(), format!("callback {}", overloads[index].name));
        let callbacks = if overloads[index].unit.eq_ignore_ascii_case(self.unit) {
            Some(&self.callbacks)
        } else if overloads[index].unit.eq_ignore_ascii_case("wbDefinitionsCommon") {
            Some(&self.common_callbacks)
        } else {
            None
        };
        if let Some(callbacks) = callbacks {
            callbacks.borrow_mut().insert(
                function.clone(),
                (
                    overloads[index].name.clone(),
                    callback_type.to_ascii_lowercase(),
                    overloads[index].line,
                ),
            );
        }
        Some(function)
    }

    /// A stub of the callback `rust` that panics when called, or `None` when
    /// the callback type is not in [`CALLBACKS`].
    pub fn callback_stub(
        &self,
        rust: &str,
        pascal: &str,
        callback_type: &str,
        line: u32,
        unit: &str,
    ) -> Option<String> {
        let (_, signature) = CALLBACKS.iter().find(|(name, _)| *name == callback_type)?;
        Some(format!(
            "/// Upstream `{pascal}`, line {line} of `{unit}.pas`.\npub fn {rust}{} {{\n    todo!(\"port {pascal} from {unit}.pas line {line}\")\n}}\n",
            signature.replace("a_", "_a_")
        ))
    }

    /// The Rust type for an enumeration or record type of a definition unit.
    pub fn unit_type(&self, name: &str) -> Result<Option<String>> {
        let lower = name.to_ascii_lowercase();
        let info = self
            .symbols
            .types
            .get(&lower)
            .ok_or_else(|| anyhow!("{name} is not a type"))?;
        if !matches!(info.kind, TypeKind::Enum(_) | TypeKind::Record(_)) {
            return Ok(None);
        }
        let rust = self.named(name)?;
        Ok(match &info.kind {
            TypeKind::Enum(values) => {
                let mut text = format!(
                    "/// Upstream `{name}`.\n#[allow(non_camel_case_types)]\n#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]\npub enum {rust} {{\n"
                );
                for (index, value) in values.iter().enumerate() {
                    if index == 0 {
                        text.push_str("    #[default]\n");
                    }
                    text.push_str(&format!("    {value},\n"));
                }
                text.push_str("}\n");
                Some(text)
            }
            TypeKind::Record(fields) => {
                let mut text = format!(
                    "/// Upstream `{name}`.\n#[derive(Debug, Clone, PartialEq, Default)]\npub struct {rust} {{\n"
                );
                for (field, type_ref) in fields {
                    let target = self.target(type_ref, true);
                    let rust_ty = match self.rust_ty(&target)?.as_str() {
                        "String" => "&'static str".to_owned(),
                        other => other.to_owned(),
                    };
                    text.push_str(&format!("    pub {}: {rust_ty},\n", snake(field)));
                }
                text.push_str("}\n");
                Some(text)
            }
            _ => None,
        })
    }

    /// The constant for a typed constant of a definition unit: a value or an
    /// array of records.
    pub fn unit_const(&self, name: &str, decl: &VarDecl) -> Result<Option<String>> {
        let Some(type_ref) = &decl.type_ref else {
            // An untyped constant is inlined where it is used.
            return Ok(None);
        };
        let rust_name = snake(name).to_ascii_uppercase();
        let cx = Context::empty();
        if let Some(value) = &decl.value {
            let target = self.target(type_ref, false);
            let rust_ty = self.rust_ty(&target)?;
            let code = self.expr_to(value, &target, &cx)?;
            return Ok(Some(format!(
                "/// Upstream `{name}`.\npub const {rust_name}: {rust_ty} = {code};\n"
            )));
        }
        // `TwbKnownSubRecordSignatures = ('NAME', '____', ...)`: the known
        // subrecord signatures of a record type, one per role.
        if let TypeRef::Named(type_name) = type_ref
            && type_name.eq_ignore_ascii_case("TwbKnownSubRecordSignatures")
        {
            let signatures: Vec<String> = decl
                .value_tokens
                .iter()
                .filter_map(|token| match &token.kind {
                    TokenKind::Str(text) => Some(format!("Signature::new(b\"{text}\")")),
                    _ => None,
                })
                .collect();
            ensure!(signatures.len() == 5, "constant {name} does not have five signatures");
            return Ok(Some(format!(
                "/// Upstream `{name}`.\npub const {rust_name}: KnownSubRecordSignatures = [{}];\n",
                signatures.join(", ")
            )));
        }
        // `array[lo..hi] of TRecord = ((Field: value; ...), ...)`
        let Ty::Array(element) = self.symbols.ty_of_type_ref(type_ref) else {
            bail!("constant {name} is not an expression and not an array");
        };
        let Ty::Named(record_name) = &*element else {
            bail!("constant {name} is not an array of records");
        };
        let Some(TypeKind::Record(fields)) = self.symbols.types.get(record_name).map(|info| &info.kind) else {
            bail!("constant {name} is not an array of records");
        };
        let record_rust = self.named(record_name)?;
        let mut text = format!("/// Upstream `{name}`.\npub const {rust_name}: &[{record_rust}] = &[\n");
        for entry in record_entries(&decl.value_tokens)? {
            text.push_str(&format!("    {record_rust} {{ "));
            let mut parts = Vec::new();
            for (field, field_type) in fields {
                let target = self.target(field_type, false);
                let code = match entry.iter().find(|(given, _)| given.eq_ignore_ascii_case(field)) {
                    Some((_, tokens)) => {
                        let mut tokens = tokens.clone();
                        tokens.push(Token {
                            kind: TokenKind::Eof,
                            line: 0,
                        });
                        let expr = Parser::new(tokens).expr()?;
                        self.expr_to(&expr, &target, &cx)?
                    }
                    None => self.zero_value(&target)?,
                };
                parts.push(format!("{}: {code}", snake(field)));
            }
            text.push_str(&parts.join(", "));
            text.push_str(" },\n");
        }
        text.push_str("];\n");
        Ok(Some(text))
    }

    /// The value of a field that a record constant leaves out.
    fn zero_value(&self, target: &Target) -> Result<String> {
        Ok(match &target.ty {
            Ty::Int => "0".to_owned(),
            Ty::Float => "0.0".to_owned(),
            Ty::Bool => "false".to_owned(),
            Ty::Str => "\"\"".to_owned(),
            Ty::Named(name) => {
                if let Some(TypeKind::Enum(values)) = self.symbols.types.get(name).map(|info| &info.kind)
                    && let Some(first) = values.first()
                {
                    format!("{}::{first}", self.named(name)?)
                } else {
                    bail!("no constant zero value for {name}")
                }
            }
            other => bail!("no constant zero value for {other:?}"),
        })
    }

    /// The static for a variable of a definition unit.
    pub fn unit_static(&self, name: &str) -> Result<String> {
        let lower = name.to_ascii_lowercase();
        let decl = self
            .symbols
            .value_decls
            .get(&lower)
            .ok_or_else(|| anyhow!("{name} is not a unit variable"))?;
        let type_ref = decl.type_ref.as_ref().ok_or_else(|| anyhow!("{name} has no type"))?;
        let target = self.target(type_ref, true);
        let rust_ty = self.rust_ty(&target)?;
        if decl.value.is_some() || !decl.value_tokens.is_empty() {
            bail!("{name} has an initial value");
        }
        Ok(format!(
            "/// Upstream `{name}`.\npub static {}: Global<{rust_ty}> = Global::new();\n",
            static_name(name)
        ))
    }

    /// Converts an emitted value to `target`.
    fn convert(&self, value: Val, target: &Target) -> Result<String> {
        let code = value.code;
        Ok(match (&value.ty, &target.ty) {
            (Ty::Nil, Ty::Array(_)) if target.owned => "Vec::new()".to_owned(),
            (Ty::Nil, Ty::Array(_) | Ty::ArrayOfConst) => "&[]".to_owned(),
            (Ty::Nil, Ty::Str) if target.owned => "String::new()".to_owned(),
            (Ty::Nil, Ty::Str) => "\"\"".to_owned(),
            (Ty::Nil, _) => "None".to_owned(),
            (Ty::Int | Ty::Float | Ty::Bool | Ty::Str, Ty::Named(name)) if name == "variant" => {
                format!("Variant::from({code})")
            }
            (Ty::Sig, Ty::Named(name)) if name == "variant" => format!("Variant::from({code}.to_string())"),
            (Ty::Unknown, _) | (_, Ty::Unknown) => code,
            (Ty::Str, Ty::Str) => match (value.owned, target.owned) {
                (true, false) => format!("&{code}"),
                (false, true) => format!("{code}.to_owned()"),
                _ => code,
            },
            (Ty::Sig, Ty::Str) if target.owned => format!("{code}.to_string()"),
            (Ty::Sig, Ty::Str) => format!("&{code}.to_string()"),
            (Ty::Int, Ty::Float) if value.literal => format!("{code}.0"),
            (Ty::Int, Ty::Float) => format!("(({code}) as {})", target.num.unwrap_or("f64")),
            (Ty::Int, Ty::Int) | (Ty::Float, Ty::Float) => match (value.num, target.num) {
                (Some(from), Some(to)) if from != to => format!("(({code}) as {to})"),
                _ => code,
            },
            (Ty::EmptyList, Ty::Array(_)) if target.owned => "Vec::new()".to_owned(),
            (Ty::EmptyList, Ty::Array(_) | Ty::ArrayOfConst) => "&[]".to_owned(),
            // A `TVarRecs` value is passed where `array of const` is expected.
            (Ty::Array(element), Ty::ArrayOfConst) if matches!(&**element, Ty::Named(name) if name == "tvarrec") => {
                if value.owned {
                    format!("&{code}")
                } else {
                    code
                }
            }
            (Ty::EmptyList, Ty::Named(name)) if self.is_set(name) => "EnumSet::empty()".to_owned(),
            (Ty::Array(from), Ty::Array(to)) => {
                let same = from == to || self.rust_element(from) == self.rust_element(to);
                match (same, value.owned, target.owned) {
                    // Owned strings are `Vec<String>`, borrowed ones `&[&str]`.
                    (true, true, false) if **to == Ty::Str => {
                        format!("&{code}.iter().map(String::as_str).collect::<Vec<_>>()")
                    }
                    (true, false, true) if **to == Ty::Str => {
                        format!("{code}.iter().map(|item| (*item).to_owned()).collect::<Vec<_>>()")
                    }
                    (true, true, false) => format!("&{code}"),
                    (true, false, true) => format!("{code}.to_vec()"),
                    (true, _, _) => code,
                    (false, _, _) if **from == Ty::Str && **to == Ty::Str => code,
                    (false, _, owned) => {
                        let element = self.convert(
                            Val::new("item.clone()", (**from).clone()),
                            &Target {
                                ty: (**to).clone(),
                                owned: true,
                                num: None,
                            },
                        )?;
                        format!(
                            "{}{code}.iter().map(|item| {element}).collect::<Vec<_>>()",
                            if owned { "" } else { "&" }
                        )
                    }
                }
            }
            (Ty::Named(from), Ty::Named(to)) if from != to && self.is_interface(to) => {
                let to_rust = self.pointee(to);
                if self
                    .symbols
                    .types
                    .get(from)
                    .is_some_and(|info| matches!(info.kind, TypeKind::Interface(_)))
                    && self.pointee(from) == to_rust
                {
                    code
                } else {
                    format!("{code}.map(|def| def as Arc<{to_rust}>)")
                }
            }
            (Ty::Routine(name), _) => {
                // A routine without arguments is called.
                let call = self.call(name, &[], &Context::empty())?;
                self.convert(call, target)?
            }
            (from, to) if from == to => code,
            (Ty::Bool | Ty::Int | Ty::Float | Ty::Str | Ty::Sig, Ty::Named(_)) | (Ty::Named(_), _) => code,
            (from, to) => bail!("no conversion from {from:?} to {to:?}"),
        })
    }

    fn rust_element(&self, element: &Ty) -> String {
        match element {
            Ty::Named(name) if self.is_interface(name) => self.pointee(name),
            other => format!("{other:?}"),
        }
    }

    fn expr(&self, expr: &Expr, cx: &Context) -> Result<Val> {
        Ok(match expr {
            Expr::Str(text) => {
                let mut value = Val::new(format!("{text:?}"), Ty::Str);
                value.literal = true;
                value
            }
            Expr::Int { digits, hex } => {
                let mut value = Val::new(if *hex { format!("0x{digits}") } else { digits.clone() }, Ty::Int);
                value.literal = !*hex;
                value
            }
            Expr::Float(text) => {
                let text = if text.contains('.') || text.contains(['e', 'E']) {
                    text.clone()
                } else {
                    format!("{text}.0")
                };
                Val::new(text, Ty::Float)
            }
            Expr::Nil => Val::new("None", Ty::Nil),
            Expr::Paren(inner) => {
                let inner = self.expr(inner, cx)?;
                Val {
                    code: format!("({})", inner.code),
                    literal: false,
                    ..inner
                }
            }
            Expr::Ident(name) => self.ident(name, cx)?,
            Expr::Call { callee, args } => match &**callee {
                Expr::Ident(name) => self.call(name, args, cx)?,
                Expr::Member { base, name } => self.method(base, name, args, cx)?,
                other => bail!("call of {}", expr_name(other)),
            },
            Expr::Member { base, name } => self.method(base, name, &[], cx)?,
            Expr::Index { base, args } => {
                let base = self.expr(base, cx)?;
                let Ty::Array(element) = base.ty else {
                    bail!("index into a value that is not an array")
                };
                let [index] = args.as_slice() else {
                    bail!("index with several values")
                };
                let index = self.expr(index, cx)?;
                let copy = matches!(*element, Ty::Int | Ty::Float | Ty::Bool | Ty::Sig);
                let borrowed_str = *element == Ty::Str && !base.owned;
                let mut value = Val::new(
                    format!(
                        "{}[({}) as usize]{}",
                        base.code,
                        index.code,
                        if copy || borrowed_str { "" } else { ".clone()" }
                    ),
                    *element,
                );
                value.owned = !borrowed_str;
                if value.ty == Ty::Int {
                    value.num = Some("i32");
                }
                value
            }
            Expr::Unary { op, operand } => {
                let operand = self.value(operand, cx)?;
                match op.as_str() {
                    "not" if operand.ty == Ty::Bool => Val::new(format!("!({})", operand.code), Ty::Bool),
                    "not" => Val::new(format!("!({})", operand.code), operand.ty).num(operand.num),
                    "-" => Val::new(format!("-({})", operand.code), operand.ty).num(operand.num),
                    "+" => operand,
                    "@" => Val::new(
                        format!("Some(&{})", operand.code.trim_end_matches(".clone()")),
                        Ty::Unknown,
                    ),
                    other => bail!("operator {other}"),
                }
            }
            Expr::Binary { op, left, right } => self.binary(op, left, right, cx)?,
            Expr::List(items) if items.is_empty() => Val::new("&[]", Ty::EmptyList),
            Expr::List(items) => {
                let first = self.expr(&items[0], cx)?;
                let element = Target {
                    ty: first.ty.clone(),
                    owned: false,
                    num: first.num,
                };
                let items = items
                    .iter()
                    .map(|item| self.expr_to(item, &element, cx))
                    .collect::<Result<Vec<_>>>()?;
                Val::new(format!("&[{}]", items.join(", ")), Ty::Array(Box::new(first.ty)))
            }
            other => bail!("expression {} is not supported", expr_name(other)),
        })
    }

    /// The expression as a value: a routine named without arguments, as an
    /// operand, is its call.
    fn value(&self, expr: &Expr, cx: &Context) -> Result<Val> {
        let value = self.expr(expr, cx)?;
        if let Ty::Routine(name) = &value.ty {
            return self.call(name, &[], &Context::empty());
        }
        Ok(value)
    }

    fn ident(&self, name: &str, cx: &Context) -> Result<Val> {
        let lower = name.to_ascii_lowercase();
        if lower == "true" || lower == "false" {
            return Ok(Val::new(lower, Ty::Bool));
        }
        if lower == "pi" {
            return Ok(Val::new("std::f64::consts::PI", Ty::Float).num(Some("f64")));
        }
        if lower == "result" {
            let result = cx.result.clone().ok_or_else(|| anyhow!("Result in a procedure"))?;
            return Ok(self.read("result", &result, true));
        }
        if let Some(local) = cx.locals.get(&lower) {
            return Ok(self.read(&local.rust, &local.target, !local.is_param));
        }
        // A closure named without arguments is its call.
        if let Some(sig) = cx.closures.get(&lower) {
            let sig = sig.clone();
            return self.call_sig(&snake(&sig.name), &sig, &[], cx);
        }
        if let Some(ty) = self.symbols.values.get(&lower) {
            // A value of an enumeration.
            if let Ty::Named(type_name) = ty
                && let Some(info) = self.symbols.types.get(type_name)
                && let TypeKind::Enum(values) = &info.kind
                && let Some(value) = values.iter().find(|value| value.eq_ignore_ascii_case(name))
            {
                return Ok(Val::new(format!("{}::{value}", self.named(type_name)?), ty.clone()));
            }
            let in_interface = self.symbols.value_units.get(&lower).map(String::as_str) == Some("wbinterface");
            if !in_interface {
                // A signature constant keeps its name; another constant is
                // written by hand; a variable is a static.
                if self.symbols.consts.contains(&lower) {
                    if *ty == Ty::Sig {
                        return Ok(Val::new(name.to_owned(), Ty::Sig));
                    }
                    let code = snake(name).to_ascii_uppercase();
                    self.references
                        .borrow_mut()
                        .insert(code.clone(), format!("constant {name}"));
                    return Ok(Val::new(code, ty.clone()));
                }
                return Ok(Val::new(format!("{}.get()", static_name(&self.declared_name(name))), ty.clone()).owned());
            }
            // The globals of `wbInterface` lose their `wb` prefix on the Rust side.
            let name = strip_wb(name);
            if self.symbols.consts.contains(&lower) {
                let code = snake(name).to_ascii_uppercase();
                self.references
                    .borrow_mut()
                    .insert(code.clone(), format!("constant {name}"));
                return Ok(Val::new(code, ty.clone()));
            }
            // A variable of a unit is read through its getter.
            let code = snake(name);
            self.references
                .borrow_mut()
                .insert(code.clone(), format!("variable {name}"));
            return Ok(Val::new(format!("{code}()"), ty.clone()).owned());
        }
        if self.symbols.routines.contains_key(&lower) {
            return Ok(Val::new(String::new(), Ty::Routine(lower)));
        }
        bail!("identifier {name} is not known")
    }

    /// Reads a local, a parameter or the result.
    fn read(&self, code: &str, target: &Target, owned: bool) -> Val {
        // Shared values are cloned on every read; owned strings and arrays
        // of locals are cloned when they are read as owned values.
        let shared = match &target.ty {
            Ty::Named(name) => self.is_interface(name) || self.is_callback(name),
            _ => false,
        };
        let cloned_when_owned = owned && matches!(target.ty, Ty::Str | Ty::Array(_));
        let mut value = Val::new(
            if shared || cloned_when_owned {
                format!("{code}.clone()")
            } else {
                code.to_owned()
            },
            target.ty.clone(),
        );
        value.owned = owned;
        value.num = target.num.or(match target.ty {
            Ty::Int => Some("i32"),
            _ => None,
        });
        value
    }

    fn binary(&self, op: &str, left: &Expr, right: &Expr, cx: &Context) -> Result<Val> {
        if op == "as" {
            let Expr::Ident(name) = right else {
                bail!("as with a type that is not a name")
            };
            return self.call(name, std::slice::from_ref(left), cx);
        }
        let a = self.value(left, cx)?;
        let b = self.value(right, cx)?;
        let bool_target = Target {
            ty: Ty::Bool,
            owned: false,
            num: None,
        };
        Ok(match op {
            "+" if a.ty == Ty::Str || b.ty == Ty::Str => {
                Val::new(format!("format!(\"{{}}{{}}\", {}, {})", a.code, b.code), Ty::Str).owned()
            }
            "+" if matches!(a.ty, Ty::Array(_)) => {
                let ty = a.ty.clone();
                let slice = |value: &Val| {
                    if value.owned {
                        format!("&{}[..]", value.code)
                    } else {
                        format!("&{}[..]", value.code.trim_start_matches('&'))
                    }
                };
                let b = match (&b.ty, &ty) {
                    (Ty::EmptyList, _) => return Ok(a),
                    _ => b,
                };
                Val::new(format!("[{}, {}].concat()", slice(&a), slice(&b)), ty).owned()
            }
            "/" => {
                let float = Target {
                    ty: Ty::Float,
                    owned: false,
                    num: Some("f64"),
                };
                let a = self.convert(a, &float)?;
                let b = self.convert(b, &float)?;
                Val::new(format!("({a} / {b})"), Ty::Float).num(Some("f64"))
            }
            "+" | "-" | "*" => {
                let num = a.num.or(b.num);
                let ty = if a.ty == Ty::Float || b.ty == Ty::Float {
                    Ty::Float
                } else {
                    a.ty.clone()
                };
                Val::new(format!("({} {op} {})", a.code, b.code), ty).num(num)
            }
            "div" => Val::new(format!("({} / {})", a.code, b.code), Ty::Int).num(a.num.or(b.num)),
            "mod" => Val::new(format!("({} % {})", a.code, b.code), Ty::Int).num(a.num.or(b.num)),
            "shl" => Val::new(format!("({} << {})", a.code, b.code), Ty::Int).num(a.num),
            "shr" => Val::new(format!("({} >> {})", a.code, b.code), Ty::Int).num(a.num),
            "and" | "or" if a.ty == Ty::Bool => {
                let operator = if op == "and" { "&&" } else { "||" };
                let b = self.convert(b, &bool_target)?;
                Val::new(format!("({} {operator} {b})", a.code), Ty::Bool)
            }
            "and" | "or" | "xor" => {
                let operator = match op {
                    "and" => "&",
                    "or" => "|",
                    _ => "^",
                };
                Val::new(format!("({} {operator} {})", a.code, b.code), a.ty.clone()).num(a.num.or(b.num))
            }
            "=" | "<>" => {
                let operator = if op == "=" { "==" } else { "!=" };
                match (&a.ty, &b.ty) {
                    (_, Ty::Nil) => Val::new(
                        format!("{}.{}()", a.code, if op == "=" { "is_none" } else { "is_some" }),
                        Ty::Bool,
                    ),
                    _ => {
                        let target = Target {
                            ty: a.ty.clone(),
                            owned: a.owned,
                            num: a.num,
                        };
                        let b = self.convert(b, &target)?;
                        Val::new(format!("({} {operator} {b})", a.code), Ty::Bool)
                    }
                }
            }
            "<" | ">" | "<=" | ">=" => {
                let target = Target {
                    ty: a.ty.clone(),
                    owned: a.owned,
                    num: a.num,
                };
                let b = self.convert(b, &target)?;
                Val::new(format!("({} {op} {b})", a.code), Ty::Bool)
            }
            "in" => Val::new(
                format!("{}.contains(&({}))", b.code.trim_start_matches('&'), a.code),
                Ty::Bool,
            ),
            other => bail!("operator {other}"),
        })
    }

    /// A call of a routine or of a function that the compiler provides.
    fn call(&self, name: &str, args: &[Expr], cx: &Context) -> Result<Val> {
        let lower = name.to_ascii_lowercase();
        match (lower.as_str(), args) {
            ("assigned", [value]) => {
                let value = self.expr(value, cx)?;
                return Ok(Val::new(
                    format!("{}.is_some()", value.code.trim_end_matches(".clone()")),
                    Ty::Bool,
                ));
            }
            ("length", [value]) => {
                let value = self.expr(value, cx)?;
                return Ok(Val::new(format!("({}.len() as i32)", value.code), Ty::Int).num(Some("i32")));
            }
            ("high", [value]) => {
                let value = self.expr(value, cx)?;
                return Ok(Val::new(format!("({}.len() as i32 - 1)", value.code), Ty::Int).num(Some("i32")));
            }
            ("low", [_]) => return Ok(Val::new("0", Ty::Int)),
            ("ord", [Expr::Str(text)]) if text.chars().count() == 1 => {
                return Ok(Val::new(
                    format!("({} as i32)", u32::from(text.chars().next().unwrap_or_default())),
                    Ty::Int,
                )
                .num(Some("i32")));
            }
            ("ord", [value]) => {
                let value = self.expr(value, cx)?;
                return Ok(Val::new(format!("(({}) as i32)", value.code), Ty::Int).num(Some("i32")));
            }
            ("sametext", [a, b]) => {
                let a = self.expr(a, cx)?;
                let b = self.expr(b, cx)?;
                return Ok(Val::new(
                    format!("{}.eq_ignore_ascii_case(&{})", a.code, b.code),
                    Ty::Bool,
                ));
            }
            ("paramstr", [Expr::Int { digits, .. }]) if digits == "0" => {
                return Ok(Val::new("exe_path()", Ty::Str).owned());
            }
            ("extractfilepath", [path]) => {
                let path = self.expr(path, cx)?;
                return Ok(Val::new(format!("extract_file_path(&{})", path.code), Ty::Str).owned());
            }
            ("fileexists", [path]) => {
                let path = self.expr(path, cx)?;
                return Ok(Val::new(
                    format!("std::path::Path::new(&{}).exists()", path.code),
                    Ty::Bool,
                ));
            }
            ("assert", [condition, ..]) => {
                return Ok(Val::new(
                    format!("assert!({})", self.condition(condition, cx)?),
                    Ty::Unknown,
                ));
            }
            ("ifthen", [condition, a, b]) if self.resolver().resolve_call(name, args, &cx.scope).is_err() => {
                // `IfThen` of `System.StrUtils`.
                let condition = self.condition(condition, cx)?;
                let a = self.expr(a, cx)?;
                let target = Target {
                    ty: a.ty.clone(),
                    owned: true,
                    num: a.num,
                };
                let b = self.expr_to(b, &target, cx)?;
                let a = self.convert(a, &target)?;
                return Ok(Val::new(format!("(if {condition} {{ {a} }} else {{ {b} }})"), target.ty).owned());
            }
            ("pred", [value]) => {
                let value = self.expr(value, cx)?;
                return Ok(Val::new(format!("({} - 1)", value.code), Ty::Int).num(value.num));
            }
            ("succ", [value]) => {
                let value = self.expr(value, cx)?;
                return Ok(Val::new(format!("({} + 1)", value.code), Ty::Int).num(value.num));
            }
            ("inttostr", [value]) => {
                let value = self.expr(value, cx)?;
                return Ok(Val::new(format!("({}).to_string()", value.code), Ty::Str).owned());
            }
            _ => {}
        }
        // A cast to a number type.
        if let (Some(num), [value]) = (scalar(name), args)
            && !self.symbols.routines.contains_key(&lower)
        {
            let value = self.expr(value, cx)?;
            let ty = if num.starts_with('f') { Ty::Float } else { Ty::Int };
            return Ok(Val::new(format!("(({}) as {num})", value.code), ty).num(Some(num)));
        }
        // A closure of the routine.
        if let Some(sig) = cx.closures.get(&lower) {
            // The Rust name comes from the declaration: Pascal ignores case.
            let sig = sig.clone();
            return self.call_sig(&snake(&sig.name), &sig, args, cx);
        }
        // A cast to an interface that is the same type on the Rust side.
        if let [value] = args
            && self.is_interface(&lower)
        {
            let value = self.expr(value, cx)?;
            return match &value.ty {
                Ty::Named(from) if self.is_interface(from) && self.pointee(from) == self.pointee(&lower) => Ok(Val {
                    ty: Ty::Named(lower),
                    ..value
                }),
                other => bail!("cast from {other:?} to {name}"),
            };
        }
        if !self.symbols.routines.contains_key(&lower) {
            bail!("call of {name}, which is not a known routine");
        }
        let (resolved, sig) = self.resolver().resolve_call(name, args, &cx.scope).map_err(|error| {
            let unknown: Vec<String> = args
                .iter()
                .filter(|arg| self.resolver().ty_of(arg, &cx.scope) == Ty::Unknown)
                .map(|arg| format!("{arg:?}").chars().take(200).collect())
                .collect();
            anyhow!("{error}; unknown: {unknown:?}")
        })?;
        let mut function = self.names[&(lower.clone(), resolved.overload)].clone();
        if sig.unit.eq_ignore_ascii_case("wbinterface") && lower.starts_with("wbis") {
            function = function[3..].to_owned();
        }
        self.references.borrow_mut().insert(
            function.clone(),
            format!("{}: {} line {}", sig.unit, sig.name, sig.line),
        );
        self.call_sig(&function, sig, args, cx)
    }

    /// The call of `function` with the arguments and the defaults of `sig`.
    fn call_sig(&self, function: &str, sig: &RoutineSig, args: &[Expr], cx: &Context) -> Result<Val> {
        let mut out = Vec::new();
        for (index, param) in sig.params.iter().enumerate() {
            if !matches!(param.modifier.as_str(), "" | "const") {
                bail!("call of {} with a {} parameter", sig.name, param.modifier);
            }
            let type_ref = param
                .type_ref
                .as_ref()
                .ok_or_else(|| anyhow!("untyped parameter of {}", sig.name))?;
            let target = self.target(type_ref, false);
            let code = match (args.get(index), &param.default) {
                (Some(arg), _) => self.expr_to(arg, &target, cx)?,
                // A default is an expression of the unit that declares the routine.
                (None, Some(default)) => self.expr_to(default, &target, &Context::empty())?,
                (None, None) => bail!("missing argument {} of {}", param.name, sig.name),
            };
            out.push(code);
        }
        let mut value = match &sig.return_type {
            Some(type_ref) => {
                let target = self.target(type_ref, true);
                Val::new(String::new(), target.ty).num(target.num)
            }
            None => Val::new(String::new(), Ty::Unknown),
        };
        value.code = format!("{function}({})", out.join(", "));
        value.owned = true;
        Ok(value)
    }

    /// A method call, a property read or a constructor call.
    fn method(&self, base: &Expr, name: &str, args: &[Expr], cx: &Context) -> Result<Val> {
        // `TwbFoo.Create(...)`
        if let Expr::Ident(class) = base
            && cx.scope.get(class).is_none()
            && self.is_kind(&class.to_ascii_lowercase(), |kind| matches!(kind, TypeKind::Class(_)))
        {
            let class_lower = class.to_ascii_lowercase();
            let owner = self
                .symbols
                .method_owner(&class_lower, name)
                .ok_or_else(|| anyhow!("{class} has no constructor {name}"))?;
            let qualified = format!("{owner}.{}", name.to_ascii_lowercase());
            let (resolved, sig) = self.resolver().resolve_call(&qualified, args, &cx.scope)?;
            // The function is named after the class that is created.
            let owner_function = &self.names[&(qualified, resolved.overload)];
            let owner_prefix = snake(sig.name.split('.').next().unwrap_or_default());
            let function = format!("{}{}", snake(class), &owner_function[owner_prefix.len()..]);
            self.references.borrow_mut().insert(
                function.clone(),
                format!("constructor {class}.{name}: {}", self.signature(&function, sig)?),
            );
            let mut value = self.call_sig(&function, sig, args, cx)?;
            value.ty = Ty::Named(class_lower);
            return Ok(value);
        }
        // `TFile.ReadAllLines`
        if let Expr::Ident(class) = base
            && class.eq_ignore_ascii_case("tfile")
            && name.eq_ignore_ascii_case("readalllines")
            && let [path] = args
        {
            let path = self.expr(path, cx)?;
            return Ok(Val::new(format!("read_all_lines(&{})", path.code), Ty::Array(Box::new(Ty::Str))).owned());
        }
        // A method of a list variable of `wbInterface` is a free function.
        if let Expr::Ident(variable) = base
            && cx.scope.get(variable).is_none()
            && let Some(ty) = self.symbols.values.get(&variable.to_ascii_lowercase())
            && !matches!(ty, Ty::Named(type_name) if self.is_interface(type_name))
            && self
                .symbols
                .value_units
                .get(&variable.to_ascii_lowercase())
                .map(String::as_str)
                == Some("wbinterface")
        {
            let function = format!("{}_{}", snake(strip_wb(variable)), snake(name));
            self.references
                .borrow_mut()
                .insert(function.clone(), format!("method {name} of {variable}"));
            let args = args
                .iter()
                .map(|arg| self.expr(arg, cx).map(|value| value.code))
                .collect::<Result<Vec<_>>>()?;
            return Ok(Val::new(format!("{function}({})", args.join(", ")), Ty::Unknown));
        }
        let mut receiver = self.expr(base, cx)?;
        // A function without arguments that is called without parentheses.
        if let Ty::Routine(routine) = &receiver.ty {
            receiver = self.call(&routine.clone(), &[], cx)?;
        }
        let Ty::Named(type_name) = &receiver.ty else {
            bail!("member {name} of a value of type {:?}", receiver.ty)
        };
        // A field of a record, through a pointer or not.
        let record = match self.symbols.types.get(type_name).map(|info| &info.kind) {
            Some(TypeKind::Record(fields)) => Some((fields, false)),
            Some(TypeKind::Pointer(pointee)) => match self.symbols.types.get(&pointee.to_ascii_lowercase()) {
                Some(info) => match &info.kind {
                    TypeKind::Record(fields) => Some((fields, true)),
                    _ => None,
                },
                None => None,
            },
            _ => None,
        };
        if let Some((fields, through_pointer)) = record {
            let (_, field_type) = fields
                .iter()
                .find(|(field, _)| field.eq_ignore_ascii_case(name))
                .ok_or_else(|| anyhow!("{type_name} has no field {name}"))?;
            let target = self.target(field_type, false);
            let base_code = if through_pointer {
                format!("{}.expect(\"nil\")", receiver.code)
            } else {
                receiver.code
            };
            let mut value = Val::new(format!("{base_code}.{}", snake(name)), target.ty).num(target.num);
            value.owned = false;
            return Ok(value);
        }
        if !self.is_interface(type_name) {
            bail!("member {name} of {type_name}");
        }
        // The type that declares the method.
        let Some(owner) = self.symbols.method_owner(type_name, name) else {
            // A property is read through its getter.
            let ty = self
                .symbols
                .member_ty(type_name, name)
                .ok_or_else(|| anyhow!("{type_name} has no member {name}"))?;
            if !args.is_empty() {
                bail!("indexed property {name}");
            }
            let mut value = Val::new(format!("{}.as_ref().unwrap().get_{}()", receiver.code, snake(name)), ty);
            value.owned = true;
            return Ok(value);
        };
        let qualified = format!("{owner}.{name}");
        let (resolved, sig) = self.resolver().resolve_call(&qualified, args, &cx.scope)?;
        let rust_name = METHODS
            .iter()
            .find(|(declaring, method, overload, _)| {
                (declaring.is_empty() || *declaring == owner)
                    && name.eq_ignore_ascii_case(method)
                    && *overload == resolved.overload
            })
            .map_or_else(|| snake(name), |(_, _, _, rust)| (*rust).to_owned());
        let mut call = self.call_sig(&rust_name, sig, args, cx)?;
        // A setter returns `Self` on the Rust side, whatever interface it
        // is declared to return.
        if let Ty::Named(returned) = &call.ty
            && self.symbols.descends_from(type_name, returned)
        {
            call.ty = receiver.ty.clone();
        }
        let returns_interface = matches!(&call.ty, Ty::Named(name) if self.is_interface(name));
        let mut value = if returns_interface {
            Val::new(format!("{}.map(|def| def.{})", receiver.code, call.code), call.ty)
        } else {
            Val::new(format!("{}.as_ref().unwrap().{}", receiver.code, call.code), call.ty)
        };
        value.owned = true;
        value.num = call.num;
        Ok(value)
    }
}

impl Context {
    fn empty() -> Self {
        Context {
            scope: Scope::default(),
            locals: HashMap::new(),
            result: None,
            routine: String::new(),
            closures: HashMap::new(),
        }
    }
}

/// Whether two parameter lists declare the same types in the same order.
fn same_params(a: &[Param], b: &[Param]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| a.type_ref == b.type_ref && a.name.eq_ignore_ascii_case(&b.name))
}

/// The lower-case names that the statements assign to.
fn assigned_names(statements: &[Stmt]) -> HashSet<String> {
    fn visit(statement: &Stmt, out: &mut HashSet<String>) {
        match statement {
            Stmt::Assign {
                target: Expr::Ident(name),
                ..
            } => {
                out.insert(name.to_ascii_lowercase());
            }
            Stmt::Expr {
                expr: Expr::Call { callee, args },
                ..
            } => {
                if let Expr::Ident(name) = &**callee
                    && ["inc", "dec", "setlength"].contains(&name.to_ascii_lowercase().as_str())
                    && let Some(Expr::Ident(target)) = args.first()
                {
                    out.insert(target.to_ascii_lowercase());
                }
            }
            Stmt::Block(inner) | Stmt::Repeat { body: inner, .. } => inner.iter().for_each(|inner| visit(inner, out)),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                visit(then_branch, out);
                if let Some(else_branch) = else_branch {
                    visit(else_branch, out);
                }
            }
            Stmt::For { body, .. } | Stmt::ForIn { body, .. } | Stmt::While { body, .. } | Stmt::With { body, .. } => {
                visit(body, out)
            }
            _ => {}
        }
    }
    let mut out = HashSet::new();
    statements.iter().for_each(|statement| visit(statement, &mut out));
    out
}

fn statement_name(statement: &Stmt) -> &'static str {
    match statement {
        Stmt::Empty => "empty",
        Stmt::Assign { .. } => "assignment",
        Stmt::Expr { .. } => "call",
        Stmt::Block(_) => "block",
        Stmt::If { .. } => "if",
        Stmt::For { .. } => "for",
        Stmt::ForIn { .. } => "for in",
        Stmt::While { .. } => "while",
        Stmt::Repeat { .. } => "repeat",
        Stmt::Case { .. } => "case",
        Stmt::With { .. } => "with",
        Stmt::Try { .. } => "try",
        Stmt::Raise(_) => "raise",
        Stmt::Var(_) => "inline variable",
    }
}

fn expr_name(expr: &Expr) -> &'static str {
    match expr {
        Expr::Ident(_) => "identifier",
        Expr::Int { .. } => "integer",
        Expr::Float(_) => "float",
        Expr::Str(_) => "string",
        Expr::Nil => "nil",
        Expr::Call { .. } => "call",
        Expr::Member { .. } => "member",
        Expr::Index { .. } => "index",
        Expr::Deref(_) => "dereference",
        Expr::Unary { .. } => "unary operator",
        Expr::Binary { .. } => "binary operator",
        Expr::List(_) => "list",
        Expr::Range(..) => "range",
        Expr::Paren(_) => "parentheses",
        Expr::Generic { .. } => "generic type",
        Expr::Inherited(_) => "inherited",
        Expr::Anonymous(_) => "anonymous routine",
    }
}

/// The Rust function name of every overload of every routine.
///
/// A routine without overloads keeps its name. An overload is named after
/// the fewest and earliest of its parameters that select it: of the overloads
/// that have all of these parameters, it is the one with the fewest
/// parameters that not every overload has. `wbInteger` with a signature and
/// an `aToStr` callback is `wb_integer_signature_to_str`, and `wbInteger`
/// with a name and a formater is `wb_integer`.
pub fn overload_names(symbols: &Symbols) -> HashMap<(String, usize), String> {
    let base_of = |sig: &RoutineSig| match sig.name.split_once('.') {
        Some((class, method)) => format!("{}_{}", snake(class), snake(method)),
        None => snake(&sig.name),
    };
    // The names of the routines without overloads are taken.
    let mut used: HashSet<String> = symbols
        .routines
        .values()
        .filter(|overloads| overloads.len() == 1)
        .map(|overloads| base_of(&overloads[0]))
        .collect();
    let mut names = HashMap::new();
    let mut keys: Vec<&String> = symbols.routines.keys().collect();
    keys.sort();
    for lower in keys {
        let overloads = &symbols.routines[lower];
        let base = base_of(&overloads[0]);
        if overloads.len() == 1 {
            names.insert((lower.clone(), 0), base);
            continue;
        }
        let mut all: Vec<Vec<String>> = overloads
            .iter()
            .map(|sig| sig.params.iter().map(|param| param_word(&param.name)).collect())
            .collect();
        // Overloads with the same parameter names are told apart by the types.
        let same_names = all
            .iter()
            .enumerate()
            .any(|(i, own)| all.iter().enumerate().any(|(j, other)| i != j && own == other));
        if same_names {
            all = overloads
                .iter()
                .map(|sig| sig.params.iter().map(type_word).collect())
                .collect();
        }
        // The parameters that not every overload has.
        let varying: Vec<Vec<String>> = all
            .iter()
            .map(|own| {
                own.iter()
                    .filter(|word| !all.iter().all(|other| other.contains(word)))
                    .cloned()
                    .collect()
            })
            .collect();
        for index in 0..overloads.len() {
            let name = selecting(index, &varying)
                .into_iter()
                .map(|words| {
                    if words.is_empty() {
                        base.clone()
                    } else {
                        format!("{base}_{}", words.join("_"))
                    }
                })
                .find(|name| !used.contains(name))
                .unwrap_or_else(|| format!("{base}_v{}", index + 1));
            used.insert(name.clone());
            names.insert((lower.clone(), index), name);
        }
    }
    names
}

/// The inside of a Rust byte string literal for ASCII text.
pub fn byte_string(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'"' => "\\\"".to_owned(),
            b'\\' => "\\\\".to_owned(),
            0x20..=0x7e => char::from(byte).to_string(),
            _ => format!("\\x{byte:02x}"),
        })
        .collect()
}

/// The entries of `((Field: value; ...), (...))`: each entry is the fields
/// it gives, with the tokens of each value.
/// One entry of a record constant: the fields it gives with their tokens.
type RecordEntry = Vec<(String, Vec<Token>)>;

fn record_entries(tokens: &[Token]) -> Result<Vec<RecordEntry>> {
    let mut entries = Vec::new();
    let mut index = 0;
    let expect = |index: &mut usize, symbol: &str| -> Result<()> {
        if tokens.get(*index).is_some_and(|token| token.is_symbol(symbol)) {
            *index += 1;
            Ok(())
        } else {
            bail!("expected `{symbol}` in a record constant at token {}", *index)
        }
    };
    expect(&mut index, "(")?;
    while index < tokens.len() && !tokens[index].is_symbol(")") {
        expect(&mut index, "(")?;
        let mut entry = Vec::new();
        while index < tokens.len() && !tokens[index].is_symbol(")") {
            let field = tokens[index]
                .ident()
                .ok_or_else(|| anyhow!("expected a field name in a record constant"))?
                .to_owned();
            index += 1;
            expect(&mut index, ":")?;
            let start = index;
            let mut depth = 0;
            while index < tokens.len() {
                let token = &tokens[index];
                if token.is_symbol("(") || token.is_symbol("[") {
                    depth += 1;
                } else if token.is_symbol(")") || token.is_symbol("]") {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                } else if token.is_symbol(";") && depth == 0 {
                    break;
                }
                index += 1;
            }
            entry.push((field, tokens[start..index].to_vec()));
            if tokens.get(index).is_some_and(|token| token.is_symbol(";")) {
                index += 1;
            }
        }
        expect(&mut index, ")")?;
        entries.push(entry);
        if tokens.get(index).is_some_and(|token| token.is_symbol(",")) {
            index += 1;
        }
    }
    Ok(entries)
}

/// The static of a unit variable: `wbFoo` is `WB_FOO`.
fn static_name(name: &str) -> String {
    snake(name).to_ascii_uppercase()
}

impl Emitter<'_> {
    /// The name of a unit value as it is declared. Pascal ignores case, so a
    /// use may spell it differently.
    fn declared_name<'n>(&self, name: &'n str) -> std::borrow::Cow<'n, str> {
        let lower = name.to_ascii_lowercase();
        match self.symbols.value_decls.get(&lower) {
            Some(decl) => decl
                .names
                .iter()
                .find(|declared| declared.eq_ignore_ascii_case(name))
                .map_or(std::borrow::Cow::Borrowed(name), |declared| {
                    std::borrow::Cow::Owned(declared.clone())
                }),
            None => std::borrow::Cow::Borrowed(name),
        }
    }
}

/// `wbFoo` is `Foo`.
fn strip_wb(name: &str) -> &str {
    match name.strip_prefix("wb") {
        Some(rest) if rest.starts_with(|c: char| c.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

/// The type of a parameter as a word: `IwbValueDef` is `value_def`.
fn type_word(param: &Param) -> String {
    fn word(type_ref: &TypeRef) -> String {
        match type_ref {
            TypeRef::Named(name) => {
                let name = name.rsplit('.').next().unwrap_or(name);
                snake(
                    name.strip_prefix("Iwb")
                        .or_else(|| name.strip_prefix("Twb"))
                        .unwrap_or(name),
                )
            }
            TypeRef::ArrayOf(element) => format!("array_of_{}", word(element)),
            TypeRef::ArrayOfConst => "array_of_const".to_owned(),
            TypeRef::Generic { name, .. } => snake(name),
            TypeRef::Other(_) => "other".to_owned(),
        }
    }
    match &param.type_ref {
        Some(type_ref) => word(type_ref),
        None => "untyped".to_owned(),
    }
}

/// `aSignature` is `signature`.
fn param_word(name: &str) -> String {
    let mut chars = name.chars();
    let stripped = match (chars.next(), chars.next()) {
        (Some('a'), Some(second)) if second.is_ascii_uppercase() => &name[1..],
        _ => name,
    };
    snake(stripped)
}

/// The sets of up to three words of `varying[index]` that select the
/// overload `index`, the best first: the set whose last word comes earliest,
/// then the smallest.
fn selecting(index: usize, varying: &[Vec<String>]) -> Vec<Vec<String>> {
    let own = &varying[index];
    let selects = |positions: &[usize]| {
        let having: Vec<usize> = (0..varying.len())
            .filter(|&other| {
                positions
                    .iter()
                    .all(|&position| varying[other].contains(&own[position]))
            })
            .collect();
        having
            .iter()
            .all(|&other| other == index || varying[other].len() > own.len())
    };
    let mut found: Vec<Vec<usize>> = Vec::new();
    if selects(&[]) {
        found.push(Vec::new());
    }
    for a in 0..own.len() {
        if selects(&[a]) {
            found.push(vec![a]);
        }
        for b in a + 1..own.len() {
            if selects(&[a, b]) {
                found.push(vec![a, b]);
            }
            for c in b + 1..own.len() {
                if selects(&[a, b, c]) {
                    found.push(vec![a, b, c]);
                }
            }
        }
    }
    found.sort_by_key(|positions| (positions.last().map_or(0, |last| last + 1), positions.len()));
    found
        .into_iter()
        .map(|positions| positions.into_iter().map(|position| own[position].clone()).collect())
        .collect()
}
