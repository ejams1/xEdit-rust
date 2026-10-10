// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter.pas (the runtime:
// TJvInterpreterExpression, TJvInterpreterFunction and TJvInterpreterUnit --
// Run/Compile, InterpretUnit/ReadUnit/InterpretUses, InterpretFunction,
// ExecFunction/InFunction, the Interpret* statements, Expression1/Expression2/
// InternalGetValue/InternalSetValue, GetElement/SetElement,
// UpdateExceptionPos/GetLastErrorLocation, DoOnStatement and CallFunction/
// CallFunctionEx)

//! The interpreter runtime: the evaluator over the syntax tree.
//!
//! Upstream the parser and the evaluator are one pass: `TJvInterpreterUnit`
//! walks the token stream, registers units, functions and variables, and
//! `ExecFunction` re-parses a routine body and runs its `Interpret*`
//! statements when the routine is called. This module keeps the semantics
//! and moves the tokens to [`crate::ast`]: [`compile`](Interpreter::compile)
//! walks a [`Module`] the way `InterpretUnit` walks tokens (the compile
//! acceptance of [`crate::interpreter::parse_compile`], so a statement error
//! inside a body surfaces at first execution, as upstream), and
//! [`exec_function`](Interpreter::exec_function) runs the parsed body, with
//! the same control flow the interpreter's flags model: `Break`, `Continue`
//! and `Exit` are [`Interpreter`] state, so they cross a routine call
//! boundary exactly as upstream's `FBreak`/`FContinue`/`FExit` do.
//!
//! The interfaces:
//!
//! * [`Interpreter::compile`] takes the script text (the script host strips
//!   the Delphi namespace prefixes first, see [`crate::check`]) and resolves
//!   `uses` units through [`Interpreter::set_unit_source`], the host's
//!   `OnGetUnitSource` (`JvInterpreter.pas:1032`).
//! * [`Interpreter::call_function`]/[`call_function_ex`](Interpreter::call_function_ex)
//!   are `CallFunction`/`CallFunctionEx` (`:8434`), how the host runs a
//!   script's `Initialize`/`Process`/`Finalize`; [`run_main`](Interpreter::run_main)
//!   is `TJvInterpreterUnit.Run` with its `main` convention
//!   (`ieMainUndefined`, `:8280`).
//! * The adapter is a public field: the host registers everything before
//!   compiling. `OnStatement` is [`Interpreter::on_statement`], fired where
//!   `DoOnStatement` fires (the top of every `InterpretStatement` and every
//!   statement-separating `;` of a `begin`/`try`/`repeat` list).
//! * The error state of the last failure is [`Interpreter::last_error`], and
//!   [`Interpreter::last_error_location`] is the `TxejviScript.GetLastErrorLocation`
//!   text (`xejviScriptHost.pas:430`).
//!
//! Positions: upstream reports byte offsets into the source where the parser
//! stood (`PosBeg`/`CurPos` of the *current token*). The tree carries spans,
//! and the runtime reconstructs the current token with the tokenizer
//! ([`Interpreter::token_after`]): `PosBeg` is the start of the token that
//! follows the construct, `CurPos` its end -- for example the undeclared
//! identifier of `Foo(1);` is reported at the `;` (`JvInterpreter.pas:6128`
//! with `PosBeg` after `ReadArgs` consumed the following token), and a
//! variant error of `1 + 'a';` at the byte after the `;` (`Expression1`'s
//! handler uses `CurPos`, `:5884`).
//!
//! Not ported here (host seams of the later steps): the RTTI property and
//! event surface (`GetValueRTTI`/`SetMethodProp`, VCL forms), the
//! `-script:`-driven run loop of `xeMainForm.pas`, and the OLE automation
//! branch, which the xEdit build compiles out.

// The interpreter's error travels as a value (`FlowError::Error(ExecError)`)
// the port moves around; boxing it would hide the structure the port mirrors.
#![allow(clippy::result_large_err)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::adapter::{self, Adapter, Args, FunctionDesc, VarList};
use crate::ast::{
    Arg, ArgValue, Block, CaseArm, ConstDecl, Expr, ExprNode, Handler, ItemNode, LocalDecl, Module, OnClause,
    RoutineDecl, Section, Span, Stmt, StmtNode, TypeDef, TypeRef, UsesUnit, VarGroup,
};
use crate::error::{
    self, Error, IE_ARRAY_NOT_ENOUGH_PARAMS, IE_ARRAY_REQUIRED, IE_ARRAY_TOO_MANY_PARAMS, IE_BOOLEAN_REQUIRED,
    IE_CLASS_REQUIRED, IE_EXPECTED, IE_INTEGER_REQUIRED, IE_INTERNAL, IE_MAIN_UNDEFINED, IE_RAISE,
    IE_RECORD_NOT_DEFINED, IE_ROC_REQUIRED, IE_TYPE_MISMATCH, IE_UNIT_NOT_FOUND, IE_UNKNOWN_IDENTIFIER,
    RS_RANGE_CHECK_ERROR, RS_UNKNOWN_RECORD_TYPE,
};
use crate::interpreter;
use crate::interpreter_parser::Tokenizer;
use crate::values::{
    self, ClassDef, DataType, ParamType, RecordField, ScriptObject, Value, ValueError, VarType, VariantArray,
};

/// The error state of one failure: the port of `EJvInterpreterError`
/// (`JvInterpreter.pas:1095`), including the fields `UpdateExceptionPos`
/// (`:5363`) fills and `FLastError` keeps for the host.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecError {
    /// `FErrCode`, the `ie*` code.
    pub code: i32,
    /// `FErrPos`: the 0-based source offset; `None` is upstream's `-1`
    /// before `UpdateExceptionPos` fills it from `CurPos`.
    pub pos: Option<i64>,
    pub name1: String,
    pub name2: String,
    /// `E.Message`: the formatted message, first the `Format` of the code,
    /// then `Error in unit '...' on line n : ...` once the unit is known.
    pub message: String,
    /// `FErrMessage`: the message before the unit and line were prefixed.
    pub err_message: String,
    /// `FErrUnitName`.
    pub unit_name: String,
    /// `FErrLine`, 1-based; -1 while unknown.
    pub line: i64,
    /// `FExceptionPos`.
    pub exception_pos: bool,
    /// Whether this is a non-interpreter exception reported through the
    /// `ieExternal` text (`:5390`).
    pub external: bool,
}

impl ExecError {
    /// `EJvInterpreterError.Create` (`JvInterpreter.pas:1478`):
    /// `Message := Format(LoadStr2(ErrCode), [ErrName1, ErrName2])`.
    pub fn new(code: i32, pos: Option<i64>, name1: &str, name2: &str) -> ExecError {
        let message = error::format_message(code, name1, name2);
        ExecError {
            code,
            pos,
            name1: name1.to_owned(),
            name2: name2.to_owned(),
            err_message: message.clone(),
            message,
            unit_name: String::new(),
            line: -1,
            exception_pos: false,
            external: false,
        }
    }

    /// A non-interpreter exception (`EZeroDivide`, `ERangeError`,
    /// `EJVCLException`): `FLastError.FErrCode := ieExternal` and the
    /// exception's own message as the text (`:5390`).
    pub fn external(message: &str) -> ExecError {
        ExecError {
            code: error::IE_EXTERNAL,
            pos: None,
            name1: String::new(),
            name2: String::new(),
            err_message: message.to_owned(),
            message: message.to_owned(),
            unit_name: String::new(),
            line: -1,
            exception_pos: false,
            external: true,
        }
    }

    /// `FLastError.Clear` (`JvInterpreter.pas:1504`): the cleared error. The
    /// object is always assigned upstream, so a cleared one prints its unit
    /// as empty and its line as -1 (`GetLastErrorLocation`).
    pub fn cleared() -> ExecError {
        ExecError {
            code: -1,
            pos: None,
            name1: String::new(),
            name2: String::new(),
            message: String::new(),
            err_message: String::new(),
            unit_name: String::new(),
            line: -1,
            exception_pos: false,
            external: false,
        }
    }

    /// A front-end [`Error`] (a parse or tokenizer failure) as the
    /// interpreter reports it: the code and the formatted text.
    pub fn from_parse_error(error: &Error) -> ExecError {
        ExecError {
            code: error.code,
            pos: (error.pos >= 0).then_some(error.pos),
            name1: String::new(),
            name2: String::new(),
            message: error.message.clone(),
            err_message: error.message.clone(),
            unit_name: String::new(),
            line: -1,
            exception_pos: false,
            external: false,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// A script exception travelling from `raise` to `except`
/// (`raise V2O(V)`, `JvInterpreter.pas:7670`): the object and the class the
/// `on E: T` clauses test (`E is ExceptionClass`, `:7487`) with the message
/// `UpdateExceptionPos` reports.
pub struct RaisedException {
    pub class: Rc<ClassDef>,
    pub object: Rc<RefCell<dyn ScriptObject>>,
    pub message: String,
}

/// What an evaluation steps into.
pub enum FlowError {
    /// A raised `EJvInterpreterError` (or the external wrap of another
    /// exception).
    Error(ExecError),
    /// A raised script exception object.
    Raise(RaisedException),
    /// A bare `raise;` (`:7663`): internally `EJvInterpreterError(ieRaise)`,
    /// which only the `except` handling of a `try` consumes (by re-raising
    /// the original; outside one it surfaces as code 4, "Re-raising an
    /// exception only allowed in exception handler").
    ReRaise,
}

impl From<ExecError> for FlowError {
    fn from(error: ExecError) -> FlowError {
        FlowError::Error(error)
    }
}

/// The `EJvInterpreterError` object an `except on E` handler binds for an
/// interpreter error (`O2V(E)`, `JvInterpreter.pas:7493`): its class is the
/// built-in `EJvInterpreterError`, whose parent is the registered
/// `Exception`, so `on E: Exception` (and any ancestor of `Exception`)
/// matches, as Delphi's RTTI makes it match upstream.
pub struct InterpExceptionObject {
    pub error: ExecError,
    class: Rc<ClassDef>,
}

impl ScriptObject for InterpExceptionObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn message(&self) -> Option<&str> {
        Some(&self.error.message)
    }
}

struct Frame {
    locals: VarList,
}

/// The host's `FOnGetValue` (`JvInterpreter.pas:867`).
pub type OnGetValueHook = Box<dyn FnMut(&str, &mut Value, &mut Args) -> Result<bool, ExecError>>;
/// The host's `FOnSetValue`.
pub type OnSetValueHook = Box<dyn FnMut(&str, &Value, &mut Args) -> Result<bool, ExecError>>;
/// `TJvInterpreterProgram.OnStatement` (`:1063`).
pub type OnStatementHook = Box<dyn FnMut(usize) -> Result<(), ExecError>>;
/// `OnGetUnitSource` (`:1032`): resolves a `uses` name to its source.
pub type UnitSourceHook = Box<dyn FnMut(&str) -> Option<Vec<u8>>>;

/// The interpreter over one compiled script: `TJvInterpreterUnit` (`:1013`)
/// on top of `TJvInterpreterFunction` (`:945`) and `TJvInterpreterExpression`
/// (`:846`).
pub struct Interpreter {
    /// `FAdapter` (`:862`). Register everything through it before
    /// [`compile`](Interpreter::compile).
    pub adapter: Adapter,
    /// `FSharedAdapter` (`:863`): the `GlobalJvInterpreterAdapter` fallback.
    pub shared_adapter: Option<Rc<RefCell<Adapter>>>,
    /// `FOnGetValue` (`:867`): the host's last lookup hook, after both
    /// adapters (`:6332`).
    pub on_get_value: Option<OnGetValueHook>,
    /// `FOnSetValue`.
    pub on_set_value: Option<OnSetValueHook>,
    /// `FOnStatement` of `TJvInterpreterProgram` (`:1063`): fired where
    /// `DoOnStatement` fires, with the source offset; an error aborts the
    /// script (the xEdit host ticks and may break here,
    /// `xejviScriptHost.pas:289`).
    pub on_statement: Option<OnStatementHook>,
    /// `OnGetUnitSource` (`:1032`).
    pub unit_source: Option<UnitSourceHook>,

    compiled: bool,
    /// The source passed to [`compile`](Interpreter::compile), kept for the
    /// `if not Compiled then Compile` of `CallFunctionEx` (`:8448`).
    source: Option<Rc<Vec<u8>>>,
    main_unit: Option<String>,
    /// `FCurUnitName` (`:947`).
    cur_unit: String,
    /// `FParser.Source` (`:860`): the source of the unit being interpreted
    /// or run, the file positions and line numbers refer to.
    cur_source: Rc<Vec<u8>>,
    /// `FFunctionStack` (`:952`): one frame per active call.
    frames: Vec<Frame>,
    /// `FBreak`/`FContinue`/`FExit` (`:948-950`); they intentionally live on
    /// the interpreter, so they leak out of a called routine exactly as
    /// upstream's do.
    break_flag: bool,
    continue_flag: bool,
    exit_flag: bool,
    /// `FLastError` (`:868`), always present; [`ExecError::cleared`] is
    /// `Clear`.
    last_error: ExecError,
    /// `FBaseErrLine` (`:865`).
    base_err_line: i64,
    /// `CurPos` (`:914`): the tokenizer position the last construct left,
    /// the fallback position of errors raised without one.
    cur_pos: usize,
    interp_error_class: Option<Rc<ClassDef>>,
}

impl Default for Interpreter {
    fn default() -> Self {
        Interpreter::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            adapter: Adapter::new(),
            shared_adapter: None,
            on_get_value: None,
            on_set_value: None,
            on_statement: None,
            unit_source: None,
            compiled: false,
            source: None,
            main_unit: None,
            cur_unit: String::new(),
            cur_source: Rc::new(Vec::new()),
            frames: Vec::new(),
            break_flag: false,
            continue_flag: false,
            exit_flag: false,
            last_error: ExecError::cleared(),
            base_err_line: 0,
            cur_pos: 0,
            interp_error_class: None,
        }
    }

    /// The host's `OnGetUnitSource`: the source of a `uses` unit, or `None`
    /// for `ieUnitNotFound`. The script host answers built-in units with an
    /// empty stub and reads the rest from the scripts folder
    /// (`xejviScriptHost.pas:439`); the runtime stays free of both.
    pub fn set_unit_source(&mut self, hook: impl FnMut(&str) -> Option<Vec<u8>> + 'static) {
        self.unit_source = Some(Box::new(hook));
    }

    /// `FLastError` (`TxejviScript.GetLastErrorLocation` reads its unit and
    /// line, `xejviScriptHost.pas:432`).
    pub fn last_error(&self) -> &ExecError {
        &self.last_error
    }

    /// `TxejviScript.GetLastErrorLocation` (`xejviScriptHost.pas:430`):
    /// `'unit ' + ErrUnitName + ' line ' + IntToStr(ErrLine)`. Upstream's
    /// `LastError` object is always assigned, so a cleared one gives
    /// `unit  line -1`.
    pub fn last_error_location(&self) -> String {
        format!("unit {} line {}", self.last_error.unit_name, self.last_error.line)
    }

    /// `TJvInterpreterUnit.FunctionExists` (`JvInterpreter.pas:8488`).
    pub fn function_exists(&self, unit_name: &str, function_name: &str) -> bool {
        self.adapter
            .find_fun_desc(unit_name, function_name.as_bytes(), b"")
            .is_some()
    }

    /// `TJvInterpreterUnit.Compile` (`JvInterpreter.pas:8291`): registers the
    /// script and every unit it `uses`. The acceptance is
    /// [`interpreter::parse_compile`], so routine bodies are registered, not
    /// parsed -- a statement-level error surfaces at first execution.
    pub fn compile(&mut self, source: &[u8]) -> Result<(), ExecError> {
        let source = Rc::new(source.to_vec());
        self.source = Some(source.clone());
        self.cur_source = source.clone();
        self.cur_unit = String::new();
        self.compiled = false;
        let module = match interpreter::parse_compile(&source) {
            Ok(module) => module,
            Err(error) => {
                let mut exec = ExecError::from_parse_error(&error);
                self.update_exception_pos(&mut exec, &self.cur_unit.clone());
                return Err(exec);
            }
        };
        self.main_unit = Some(module.name.clone());
        match self.compile_module(&module, &source) {
            Ok(()) => {
                self.compiled = true;
                Ok(())
            }
            Err(flow) => {
                let mut exec = self.flow_to_error(flow);
                self.update_exception_pos(&mut exec, &self.cur_unit.clone());
                Err(exec)
            }
        }
    }

    /// `InterpretUnit` (`JvInterpreter.pas:8090`): the items in source
    /// order, then `AddSrcUnit` with the unit's source and its uses list.
    fn compile_module(&mut self, module: &Module, source: &Rc<Vec<u8>>) -> Result<(), FlowError> {
        self.cur_unit = module.name.clone();
        self.cur_source = source.clone();
        let mut uses_names: Vec<String> = Vec::new();
        for item in &module.items {
            match &item.node {
                ItemNode::Uses(clause) => {
                    for unit in &clause.units {
                        uses_names.push(unit.name.clone());
                        self.read_unit(unit)?;
                    }
                }
                ItemNode::Routine(decl) => self.register_routine(decl, item.section, source)?,
                ItemNode::Var(group) => self.register_unit_vars(group)?,
                ItemNode::Const(decl) => self.register_unit_const(decl)?,
                ItemNode::Type(decl) => self.register_type(decl)?,
            }
        }
        self.adapter.add_src_unit(&module.name, source, &uses_names.join(","));
        Ok(())
    }

    /// `InterpretUses`/`ReadUnit` (`JvInterpreter.pas:8062`, `:8026`).
    fn read_unit(&mut self, unit: &UsesUnit) -> Result<(), FlowError> {
        if self.adapter.unit_exists(unit.name.as_bytes()) {
            return Ok(());
        }
        // The cycle guard: `AddSrcUnit(FCurUnitName, '', '')` registers the
        // unit being interpreted with an empty source before reading
        // (`:8034`).
        let guard_unit = self.cur_unit.clone();
        self.adapter.add_src_unit(&guard_unit, b"", "");
        let source = self.unit_source.as_mut().and_then(|hook| hook(&unit.name));
        let Some(source) = source else {
            return Err(FlowError::Error(ExecError::new(
                IE_UNIT_NOT_FOUND,
                Some(unit.span.start as i64),
                &unit.name,
                "",
            )));
        };
        let source = Rc::new(source);
        let module = match interpreter::parse_compile(&source) {
            Ok(module) => module,
            Err(error) => {
                let mut exec = ExecError::from_parse_error(&error);
                self.update_exception_pos(&mut exec, &unit.name);
                return Err(FlowError::Error(exec));
            }
        };
        let saved_unit = std::mem::replace(&mut self.cur_unit, unit.name.clone());
        let saved_source = std::mem::replace(&mut self.cur_source, source.clone());
        let result = self.compile_module(&module, &source);
        let result = result.map_err(|flow| {
            let mut exec = self.flow_to_error(flow);
            self.update_exception_pos(&mut exec, &self.cur_unit.clone());
            FlowError::Error(exec)
        });
        self.cur_unit = saved_unit;
        self.cur_source = saved_source;
        result
    }

    /// `InterpretFunction` (`JvInterpreter.pas:7948`) as registration: an
    /// `interface` declaration registers nothing (upstream rewinds the
    /// position), an `external` declaration an `AddExtFun`, everything else
    /// a [`FunctionDesc`] whose body `ExecFunction` parses later.
    fn register_routine(
        &mut self,
        decl: &RoutineDecl,
        section: Section,
        source: &Rc<Vec<u8>>,
    ) -> Result<(), FlowError> {
        if section == Section::Interface {
            return Ok(());
        }
        let header = &decl.header;
        let params = &header.params;
        let param_types: Vec<ParamType> = params
            .iter()
            .map(|param| {
                let typ = values::type_name_2_var_type(param.type_name.as_bytes());
                match param.mode {
                    crate::ast::ParamMode::Var => ParamType::by_ref(typ),
                    crate::ast::ParamMode::Const => ParamType::by_const(typ),
                    crate::ast::ParamMode::Value => ParamType::value(typ),
                }
            })
            .collect();
        let (res_typ, res_typ_name, res_data_type) = match &header.result {
            Some(result_ref) => {
                let data_type = self.data_type_from_ref(result_ref);
                let mut typ = data_type.get_typ();
                // `if FunctionDesc.FResTyp = 0 then FResTyp := varVariant`
                // (`:7938`): a function of an unknown result type returns an
                // untyped variant.
                if typ == VarType::Empty {
                    typ = VarType::Variant;
                }
                (typ, type_ref_name(result_ref), Some(data_type))
            }
            None => (VarType::Empty, String::new(), None),
        };
        if let Some(external) = &decl.external {
            // `InterpretFunction`'s `external` path (`:7962`): the DLL is a
            // literal or an identifier resolved with `GetValue` now.
            let dll = match &external.dll.node {
                ExprNode::Str(bytes) => Value::Str(bytes.clone()),
                ExprNode::Ident { name, args, indexed } => {
                    let value = self.internal_get_value(None, name, args, *indexed, external.dll.span)?;
                    let pos = self.cur_pos;
                    match values::to_str(&value) {
                        Ok(bytes) => Value::Str(bytes),
                        Err(error) => return Err(FlowError::Error(self.value_error(error, pos))),
                    }
                }
                _ => Value::Str(Vec::new()),
            };
            let function_name = external
                .name
                .as_ref()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .unwrap_or_default();
            self.adapter.add_ext_fun(
                &self.cur_unit,
                &header.name,
                dll,
                &function_name,
                external.index.unwrap_or(-1),
                params.len() as i32,
                param_types,
                res_typ,
            );
            return Ok(());
        }
        let desc = FunctionDesc::new(
            self.cur_unit.clone(),
            header.name.clone(),
            header.class.clone().unwrap_or_default(),
            decl.body_pos,
            source.clone(),
            params.len(),
            param_types,
            params.iter().map(|param| param.type_name.clone()).collect(),
            params.iter().map(|param| param.name.clone()).collect(),
            res_typ,
            res_typ_name,
            res_data_type,
            Rc::new(decl.clone()),
        );
        self.adapter.add_src_function(desc);
        Ok(())
    }

    /// `InterpretVar` at unit level (`AddSrcVar`, `JvInterpreter.pas:8119`).
    fn register_unit_vars(&mut self, group: &VarGroup) -> Result<(), FlowError> {
        let data_type = self.data_type_from_ref(&group.ty);
        let type_name = type_ref_name(&group.ty);
        for name in &group.names {
            let value = data_type.init();
            let vtyp = ParamType::value(data_type.get_typ());
            let result =
                self.adapter
                    .add_src_var(&self.cur_unit, name, &type_name, vtyp, &value, Some(data_type.clone()));
            if let Err(error) = result {
                return Err(FlowError::Error(self.value_error(error, self.cur_pos)));
            }
        }
        Ok(())
    }

    /// `InterpretConst` at unit level (`:8120`): the value is a full
    /// `Expression1` at compile time.
    fn register_unit_const(&mut self, decl: &ConstDecl) -> Result<(), FlowError> {
        let value = self.expression1(&decl.value)?;
        let pos = self.cur_pos;
        let result = self.adapter.add_src_var(
            &self.cur_unit,
            &decl.name,
            "",
            ParamType::default(),
            &value,
            Some(DataType::Simple(value.var_type())),
        );
        if let Err(error) = result {
            return Err(FlowError::Error(self.value_error(error, pos)));
        }
        Ok(())
    }

    /// `InterpretType` (`:8126`): records and classes; an alias or an enum
    /// is the front end's extension for the declaration-only `xEditAPI.pas`
    /// and fails here the way `InterpretType` fails (`:8160`).
    fn register_type(&mut self, decl: &crate::ast::TypeDecl) -> Result<(), FlowError> {
        match &decl.def {
            TypeDef::Record(record) => {
                let mut fields = Vec::new();
                let mut offset = 0i64;
                for group in &record.fields {
                    let data_type = self.data_type_from_ref(&group.ty);
                    for name in &group.names {
                        fields.push(RecordField {
                            name: name.clone(),
                            offset,
                            typ: VarType::Empty,
                            data_type: Some(data_type.clone()),
                        });
                        // `AddField`: `Inc(RecordSize, SizeOf(TVarData))`
                        // (`:8661`).
                        offset += 16;
                    }
                }
                self.adapter.add_rec(&self.cur_unit, &decl.name, offset, fields);
                Ok(())
            }
            TypeDef::Class(_) => {
                self.adapter.add_src_class(&self.cur_unit, &decl.name);
                Ok(())
            }
            TypeDef::Alias(_) | TypeDef::Enum { .. } => {
                let pos = self
                    .token_after(decl.span.start)
                    .and_then(|(_, end, _)| self.token_after(end));
                let pos = pos.map(|(start, _, _)| start).unwrap_or(decl.span.start);
                Err(FlowError::Error(self.expected_at(pos, "Class Declaration")))
            }
        }
    }

    /// `ParseDataType` (`JvInterpreter.pas:7689`) as the runtime's type
    /// descriptor: a registered record, a simple type, or an array of them.
    fn data_type_from_ref(&self, ty: &TypeRef) -> DataType {
        match ty {
            TypeRef::Named(name) => {
                if let Some(def) = self.adapter.get_rec(name.as_bytes()) {
                    DataType::Record(def)
                } else {
                    DataType::Simple(values::type_name_2_var_type(name.as_bytes()))
                }
            }
            TypeRef::Array { ranges, element } => DataType::Array {
                ranges: ranges.clone(),
                item_type: element_item_type(element),
                element: Box::new(self.data_type_from_ref(element)),
            },
        }
    }

    //=== Calling =============================================================

    /// `CallFunction` (`JvInterpreter.pas:8434`): the host's entry, with the
    /// parameters as values.
    pub fn call_function(&mut self, function_name: &str, params: &[Value]) -> Result<Value, ExecError> {
        let mut args = Args::new();
        args.values = params.to_vec();
        self.call_function_ex(None, function_name, &mut args)
    }

    /// `CallFunctionEx` (`JvInterpreter.pas:8440`): finds the routine of a
    /// unit (an empty name for any) and runs it; `args` carries any `var`
    /// parameters the host passes by name (`FVarNames`), which the routine
    /// writes back into `args.values`.
    pub fn call_function_ex(
        &mut self,
        unit_name: Option<&str>,
        function_name: &str,
        args: &mut Args,
    ) -> Result<Value, ExecError> {
        self.auto_compile()?;
        let unit = unit_name.unwrap_or("");
        let Some(desc) = self.adapter.find_fun_desc(unit, function_name.as_bytes(), b"") else {
            return Err(ExecError::new(IE_UNKNOWN_IDENTIFIER, None, function_name, ""));
        };
        // The simple init of `CallFunctionEx` (`:8470`).
        self.break_flag = false;
        self.continue_flag = false;
        self.last_error = ExecError::cleared();
        let result = self.exec_function(&desc, args);
        self.flow_result(result)
    }

    /// `TJvInterpreterUnit.Run` for the unit form (`JvInterpreter.pas:8254`):
    /// the `main` routine of the compiled unit; without one,
    /// `ieMainUndefined` (`:8280`).
    pub fn run_main(&mut self) -> Result<Value, ExecError> {
        self.auto_compile()?;
        let desc = self
            .main_unit
            .clone()
            .and_then(|unit| self.adapter.find_fun_desc(&unit, b"main", b""));
        let Some(desc) = desc else {
            return Err(ExecError::new(IE_MAIN_UNDEFINED, None, "", ""));
        };
        let mut args = Args::new();
        self.break_flag = false;
        self.continue_flag = false;
        self.last_error = ExecError::cleared();
        let result = self.exec_function(&desc, &mut args);
        self.flow_result(result)
    }

    fn auto_compile(&mut self) -> Result<(), ExecError> {
        if self.compiled {
            return Ok(());
        }
        match self.source.clone() {
            Some(source) => self.compile(&source),
            None => Err(ExecError::new(IE_INTERNAL, None, "no source to compile", "")),
        }
    }

    fn flow_result(&mut self, result: Result<Value, FlowError>) -> Result<Value, ExecError> {
        match result {
            Ok(value) => Ok(value),
            Err(flow) => Err(self.flow_to_error(flow)),
        }
    }

    fn flow_to_error(&self, flow: FlowError) -> ExecError {
        match flow {
            FlowError::Error(exec) => exec,
            FlowError::ReRaise => ExecError::new(IE_RAISE, Some(self.cur_pos as i64), "", ""),
            FlowError::Raise(raised) => {
                let mut exec = ExecError::external(&raised.message);
                exec.pos = Some(self.cur_pos as i64);
                exec
            }
        }
    }

    /// `ExecFunction` (`JvInterpreter.pas:8397`): switch the current unit,
    /// run the body, and report every error with the routine's unit
    /// (`UpdateExceptionPos(E, FCurUnitName)`, `:8419`).
    fn exec_function(&mut self, desc: &Rc<FunctionDesc>, args: &mut Args) -> Result<Value, FlowError> {
        let mut switched = false;
        let mut old_unit = String::new();
        let mut old_source = self.cur_source.clone();
        if !self.cur_unit.eq_ignore_ascii_case(&desc.unit_name) {
            old_unit = std::mem::replace(&mut self.cur_unit, desc.unit_name.clone());
            old_source = std::mem::replace(&mut self.cur_source, desc.source.clone());
            switched = true;
        }
        let result = self.in_function(desc, args);
        let result = result.map_err(|flow| self.update_flow(flow, &desc.unit_name));
        if switched {
            self.cur_unit = old_unit;
            self.cur_source = old_source;
        }
        result
    }

    /// `InFunction` (`JvInterpreter.pas:6436`): the parameter copies, the
    /// local sections, the body, and the write-back of `var` parameters.
    fn in_function(&mut self, desc: &Rc<FunctionDesc>, args: &mut Args) -> Result<Value, FlowError> {
        // `CheckNotSupportedFunctionParameters` (`:6543`).
        for value in &args.values {
            if matches!(value, Value::Array(_) | Value::Record(_)) {
                let pos = self.cur_pos;
                let error = ValueError::not_implemented(error::message_text(402));
                return Err(FlowError::Error(self.value_error(error, pos)));
            }
        }
        self.frames.push(Frame { locals: VarList::new() });
        if let Err(flow) = self.enter_function(desc, args) {
            self.frames.pop();
            self.exit_flag = false;
            return Err(flow);
        }
        self.exit_flag = false;
        let body = match desc.routine_body() {
            Ok(body) => body,
            Err(error) => {
                self.frames.pop();
                self.exit_flag = false;
                return Err(FlowError::Error(ExecError::from_parse_error(&error)));
            }
        };
        for local in &body.locals {
            let result = match local {
                LocalDecl::Var(group) => self.add_frame_vars(group),
                LocalDecl::Const(decl) => self.add_frame_const(decl),
            };
            if let Err(flow) = result {
                self.leave_function(desc, args, false);
                self.exit_flag = false;
                return Err(flow);
            }
        }
        match self.exec_block(&body.block) {
            Ok(()) => {
                let value = self.leave_function(desc, args, true);
                self.exit_flag = false;
                Ok(value)
            }
            Err(flow) => {
                self.leave_function(desc, args, false);
                self.exit_flag = false;
                Err(flow)
            }
        }
    }

    /// `EnterFunction` (`JvInterpreter.pas:6443`): the `var` parameters copy
    /// their value (`JvInterpreterVarCopy`), the by-value ones assign
    /// (`JvInterpreterVarAssignment`, which refuses arrays), and a routine
    /// with a result gets its `Result` variable initialized through the
    /// result's `DataType`.
    fn enter_function(&mut self, desc: &Rc<FunctionDesc>, args: &mut Args) -> Result<(), FlowError> {
        let has_refs = desc.param_types.iter().take(args.values.len()).any(|typ| typ.by_ref);
        args.has_vars = args.has_vars || has_refs;
        for index in 0..args.values.len() {
            let by_ref = desc.param_types.get(index).is_some_and(|typ| typ.by_ref);
            let param_name = desc.param_names.get(index).cloned().unwrap_or_default();
            let typ = desc.param_types.get(index).copied().unwrap_or_default();
            let mut value = Value::Null;
            if by_ref {
                values::jv_var_copy(&mut value, &args.values[index]);
            } else {
                let pos = self.cur_pos;
                if let Err(error) = values::jv_var_assignment(&mut value, &args.values[index]) {
                    return Err(FlowError::Error(self.value_error(error, pos)));
                }
            }
            let result = {
                let frame = self.frames.last_mut().expect("frame");
                frame
                    .locals
                    .add_var("", &param_name, "", typ, &value, Some(DataType::Simple(typ.typ)))
            };
            if let Err(error) = result {
                let pos = self.cur_pos;
                return Err(FlowError::Error(self.value_error(error, pos)));
            }
        }
        if desc.res_typ != VarType::Empty {
            let init = desc.res_data_type.as_ref().map(DataType::init).unwrap_or(Value::Empty);
            let res_typ = desc.res_typ;
            let data_type = desc.res_data_type.clone();
            let result = {
                let frame = self.frames.last_mut().expect("frame");
                frame
                    .locals
                    .add_var("", "Result", "", ParamType::value(res_typ), &init, data_type)
            };
            if let Err(error) = result {
                let pos = self.cur_pos;
                return Err(FlowError::Error(self.value_error(error, pos)));
            }
        }
        Ok(())
    }

    /// `LeaveFunction` (`JvInterpreter.pas:6488`): on success the `var`
    /// parameters read the local value back by parameter name (`:6505`) and
    /// `Result` becomes the call's value (`ResTyp > 0`; a procedure leaves
    /// the previous `FVResult` untouched, which the port keeps as
    /// [`Value::Empty`]).
    fn leave_function(&mut self, desc: &Rc<FunctionDesc>, args: &mut Args, ok: bool) -> Value {
        if ok {
            for index in 0..args.values.len() {
                let by_ref = desc.param_types.get(index).is_some_and(|typ| typ.by_ref);
                let Some(param_name) = desc.param_names.get(index) else {
                    continue;
                };
                if !by_ref || param_name.is_empty() {
                    continue;
                }
                if let Some(value) = self
                    .frames
                    .last()
                    .and_then(|frame| frame.locals.value_of("", param_name.as_bytes()))
                {
                    args.values[index] = value.clone();
                }
            }
        }
        let result = if ok {
            self.frames
                .last()
                .and_then(|frame| frame.locals.value_of("", b"Result"))
                .cloned()
                .unwrap_or(Value::Empty)
        } else {
            Value::Empty
        };
        self.frames.pop();
        result
    }

    /// `InterpretVar`'s body for function locals (`:7369`).
    fn add_frame_vars(&mut self, group: &VarGroup) -> Result<(), FlowError> {
        let data_type = self.data_type_from_ref(&group.ty);
        let type_name = type_ref_name(&group.ty);
        let names = group.names.clone();
        for name in names {
            let value = data_type.init();
            let result = {
                let frame = self.frames.last_mut().expect("frame");
                frame.locals.add_var(
                    "",
                    &name,
                    &type_name,
                    ParamType::value(data_type.get_typ()),
                    &value,
                    Some(data_type.clone()),
                )
            };
            if let Err(error) = result {
                let pos = self.cur_pos;
                return Err(FlowError::Error(self.value_error(error, pos)));
            }
        }
        Ok(())
    }

    /// `InterpretConst` for function locals (`:7410`).
    fn add_frame_const(&mut self, decl: &ConstDecl) -> Result<(), FlowError> {
        let value = self.expression1(&decl.value)?;
        let result = {
            let frame = self.frames.last_mut().expect("frame");
            frame.locals.add_var(
                "",
                &decl.name,
                "",
                ParamType::default(),
                &value,
                Some(DataType::Simple(value.var_type())),
            )
        };
        if let Err(error) = result {
            let pos = self.cur_pos;
            return Err(FlowError::Error(self.value_error(error, pos)));
        }
        Ok(())
    }

    //=== The lookup chain ====================================================

    /// `TJvInterpreterFunction.GetValue` (`JvInterpreter.pas:6596`) and
    /// `TJvInterpreterUnit.GetValue` (`:8317`): the local variables, then
    /// the adapter and the shared adapter, then the host hook, then the
    /// script functions of the current unit (or of the unit a member access
    /// reached through).
    fn get_value(
        &mut self,
        identifier: &[u8],
        value: &mut Value,
        args: &mut Args,
        call_pos: usize,
    ) -> Result<bool, FlowError> {
        let local = match self.frames.last_mut() {
            Some(frame) => frame.locals.get_value(identifier, value, args),
            None => false,
        };
        if local {
            return Ok(true);
        }
        match self.adapter.get_value(identifier, value, args) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(error) => return Err(FlowError::Error(self.value_error(error, call_pos))),
        }
        if let Some(shared) = self.shared_adapter.clone() {
            match shared.borrow_mut().get_value(identifier, value, args) {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(error) => return Err(FlowError::Error(self.value_error(error, call_pos))),
            }
        }
        if let Some(hook) = &mut self.on_get_value
            && hook(&String::from_utf8_lossy(identifier), value, args)?
        {
            return Ok(true);
        }
        let unit = match &args.obj {
            None => self.cur_unit.clone(),
            Some(obj) => match adapter::src_unit_name(obj) {
                Some(unit_name) => unit_name,
                // A form or class receiver: the RTTI class-method surface of
                // the VCL shim (phase 6 step 9).
                None => return Ok(false),
            },
        };
        let Some(desc) = self.adapter.find_fun_desc(&unit, identifier, b"") else {
            return Ok(false);
        };
        let count = desc.param_count as i32;
        let param_types = desc.param_types.clone();
        if let Err(error) = self.adapter.check_args(args, count, &param_types) {
            return Err(FlowError::Error(self.value_error(error, call_pos)));
        }
        let result = self.exec_function(&desc, args)?;
        *value = result;
        Ok(true)
    }

    /// `TJvInterpreterFunction.SetValue` (`JvInterpreter.pas:6618`): the
    /// locals, then the adapter chain. Upstream's `SetValue` wrapper reports
    /// adapter errors at `PosBeg` (`:6346`).
    fn set_value(
        &mut self,
        identifier: &str,
        value: &Value,
        args: &mut Args,
        err_pos: usize,
    ) -> Result<bool, FlowError> {
        let local = match self.frames.last_mut() {
            Some(frame) => frame.locals.set_value(identifier.as_bytes(), value, args),
            None => Ok(false),
        };
        match local {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
        }
        match self.adapter.set_value(identifier.as_bytes(), value, args) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
        }
        if let Some(shared) = self.shared_adapter.clone() {
            match shared.borrow_mut().set_value(identifier.as_bytes(), value, args) {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
            }
        }
        if let Some(hook) = &mut self.on_set_value
            && hook(identifier, value, args)?
        {
            return Ok(true);
        }
        Ok(false)
    }

    /// `UpdateVarParams` (`JvInterpreter.pas:6060` and `:6493`): every
    /// argument that named a variable of a `var` parameter is written back;
    /// a failing write is ignored, as upstream's commented-out raise leaves
    /// it.
    fn update_var_params(&mut self, args: &mut Args) -> Result<(), FlowError> {
        let count = args.values.len();
        for index in 0..count {
            let by_ref = args.types.get(index).is_some_and(|typ| typ.by_ref);
            let Some(name) = args.var_name(index).map(str::to_owned) else {
                continue;
            };
            if name.is_empty() || !by_ref {
                continue;
            }
            let value = args.values[index].clone();
            let mut plain = Args::new();
            let pos = self.cur_pos;
            let _ = self.set_value(&name, &value, &mut plain, pos)?;
        }
        Ok(())
    }

    //=== Expressions =========================================================

    /// `Expression1` (`JvInterpreter.pas:5626`): a full expression. The
    /// variant-error conversion of its handler happens at the operator sites
    /// (see [`values::binary_op`]).
    fn expression1(&mut self, expr: &Expr) -> Result<Value, FlowError> {
        self.eval_expr(expr)
    }

    /// `Expression2` (`JvInterpreter.pas:5888`): the expression with the
    /// boolean or integer requirement of its context.
    fn expression2(&mut self, expr: &Expr, expected: VarType) -> Result<Value, FlowError> {
        let value = self.expression1(expr)?;
        if value.var_type() == expected {
            return Ok(value);
        }
        let pos = Some(expr.span.start as i64);
        match expected {
            VarType::Integer => {
                // `Expression2` accepts `varSmallint`, `varShortInt`,
                // `varByte`, `varWord`, `varUInt32`, `varInt64` and
                // `varUInt64` when the value fits an `Integer`
                // (`JvInterpreter.pas:5905-5913`) -- the 64-bit literal
                // acceptance of the upstream `whatsnew.md` note ("certain
                // scripts with `for` loops or `case` statements might fail
                // with `Integer required` error in 64 bit"): a literal above
                // `High(Integer)` is a `varInt64` (`ParseToken`:
                // `Val(FTokenStr, ValueInt64, Stub)`, `:5565`) and is
                // refused here, with the visible `ieIntegerRequired` message.
                if matches!(value, Value::Smallint(_) | Value::Byte(_) | Value::Int64(_))
                    && let Ok(fit) = values::to_i32(&value)
                {
                    return Ok(Value::Integer(fit));
                }
                Err(FlowError::Error(ExecError::new(IE_INTEGER_REQUIRED, pos, "", "")))
            }
            VarType::Bool => Err(FlowError::Error(ExecError::new(IE_BOOLEAN_REQUIRED, pos, "", ""))),
            // `JvInterpreterError(ieUnknown, ErrPos)` (`:5921`); the
            // interpreter only ever asks for the two types above.
            _ => Err(FlowError::Error(ExecError::new(1, pos, "", ""))),
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, FlowError> {
        match &expr.node {
            // `ParseToken` (`JvInterpreter.pas:5559`): an `Integer` while it
            // fits, `Int64` otherwise.
            ExprNode::Int(text) => match values::parse_int_literal(text) {
                Some(value) => match i32::try_from(value) {
                    Ok(small) => Ok(Value::Integer(small)),
                    Err(_) => Ok(Value::Int64(value)),
                },
                None => Ok(Value::Integer(0)),
            },
            ExprNode::Float(text) => Ok(Value::Double(values::parse_float_literal(text).unwrap_or(0.0))),
            ExprNode::Str(bytes) => Ok(Value::Str(bytes.clone())),
            ExprNode::Bool(value) => Ok(Value::Bool(*value)),
            ExprNode::Set { items } => self.eval_set(items),
            ExprNode::Unary { op, operand } => {
                let value = self.eval_expr(operand)?;
                let result = match op {
                    // Unary plus is the value itself (`:5720`).
                    crate::ast::UnOp::Plus => Ok(value),
                    crate::ast::UnOp::Minus => values::unary_minus(&value),
                    crate::ast::UnOp::Not => values::unary_not(&value),
                };
                match result {
                    Ok(value) => Ok(value),
                    Err(error) => {
                        let pos = self.following_token_end(expr.span.end);
                        Err(FlowError::Error(self.value_error(error, pos)))
                    }
                }
            }
            ExprNode::Binary { op, left, right } => {
                let left_value = self.eval_expr(left)?;
                let right_value = self.eval_expr(right)?;
                match values::binary_op(*op, &left_value, &right_value) {
                    Ok(value) => Ok(value),
                    Err(error) => {
                        // The `EVariantError` handler of `Expression1`
                        // reports the failure at `CurPos` (`:5884`), the
                        // position after the token that followed the
                        // expression.
                        let pos = self.following_token_end(expr.span.end);
                        Err(FlowError::Error(self.value_error(error, pos)))
                    }
                }
            }
            ExprNode::Ident { name, args, indexed } => self.internal_get_value(None, name, args, *indexed, expr.span),
            ExprNode::Member {
                base,
                name,
                args,
                indexed,
            } => {
                let receiver = self.eval_expr(base)?;
                // `JvInterpreterError(ieROCRequired, PosBeg)` at the member
                // name (`:6146`); upstream also accepts `varDispatch` and
                // `varUnknown`, which have no port values.
                if !matches!(receiver.var_type(), VarType::Object | VarType::Class | VarType::Record) {
                    let pos = self.member_name_pos(base.span.end);
                    return Err(FlowError::Error(ExecError::new(
                        IE_ROC_REQUIRED,
                        Some(pos as i64),
                        "",
                        "",
                    )));
                }
                self.internal_get_value(Some(receiver), name, args, *indexed, expr.span)
            }
        }
    }

    /// `SetExpression1` (`JvInterpreter.pas:5927`): `[a, b]`. An identifier
    /// contributes `1 shl value`, a plain integer its own value, and a
    /// non-ordinal identifier is `ieIntegerRequired` at its position
    /// (`:5944`).
    fn eval_set(&mut self, items: &[Expr]) -> Result<Value, FlowError> {
        let mut result: i32 = 0;
        for item in items {
            match &item.node {
                ExprNode::Int(text) => {
                    let value = values::parse_int_literal(text).unwrap_or(0);
                    match i32::try_from(value) {
                        Ok(small) => result |= small,
                        Err(_) => {
                            let pos = self.following_token_end(item.span.end);
                            return Err(FlowError::Error(ExecError::new(
                                IE_TYPE_MISMATCH,
                                Some(pos as i64),
                                "",
                                "",
                            )));
                        }
                    }
                }
                ExprNode::Ident { name, args, indexed } => {
                    let value = self.internal_get_value(None, name, args, *indexed, item.span)?;
                    if !value.is_ordinal() {
                        let pos = self.following_token_start(item.span.end);
                        return Err(FlowError::Error(ExecError::new(
                            IE_INTEGER_REQUIRED,
                            Some(pos as i64),
                            "",
                            "",
                        )));
                    }
                    let pos = self.cur_pos;
                    match values::to_i32(&value) {
                        Ok(index) => result |= 1i32.wrapping_shl(index as u32),
                        Err(error) => return Err(FlowError::Error(self.value_error(error, pos))),
                    }
                }
                _ => {}
            }
        }
        Ok(Value::Set(result))
    }

    /// The argument list of a call: [`Arg`]s to values, an open array to the
    /// `VarArrayCreate` value `ReadOpenArray` builds
    /// (`JvInterpreter.pas:5967`), and the first-token names the `var`
    /// write-back needs (`FVarNames`, `:6007`).
    fn build_call_args(&mut self, arg_exprs: &[Arg], indexed: bool) -> Result<Args, FlowError> {
        let mut args = Args::new();
        args.indexed = indexed;
        for (index, arg) in arg_exprs.iter().enumerate() {
            let value = match &arg.value {
                ArgValue::Expr(expr) => self.expression1(expr)?,
                ArgValue::OpenArray(items) => {
                    let mut values = Vec::with_capacity(items.len());
                    for item in items {
                        values.push(self.expression1(item)?);
                    }
                    Value::VariantArray(Rc::new(RefCell::new(VariantArray::new(values))))
                }
            };
            args.values.push(value);
            args.set_var_name(index, arg.name.clone());
        }
        Ok(args)
    }

    /// `InternalGetValue` (`JvInterpreter.pas:6053`): resolve a name (of a
    /// receiver, or of the current unit), run the lookup chain, fix a record
    /// value's type, write back `var` parameters and read the element of an
    /// indexed form.
    fn internal_get_value(
        &mut self,
        receiver: Option<Value>,
        name: &str,
        arg_exprs: &[Arg],
        indexed: bool,
        span: Span,
    ) -> Result<Value, FlowError> {
        let mut args = self.build_call_args(arg_exprs, indexed)?;
        args.obj = receiver;
        let call_end = self.following_token_end(span.end);
        let call_start = self.following_token_start(span.end);
        self.cur_pos = call_end;
        let mut value = Value::Null;
        if !self.get_value(name.as_bytes(), &mut value, &mut args, call_end)? {
            return Err(FlowError::Error(ExecError::new(
                IE_UNKNOWN_IDENTIFIER,
                Some(call_start as i64),
                name,
                "",
            )));
        }
        if value.is_record() {
            // `if not (FAdapter.SetRecord(...)) then
            // JvInterpreterErrorN(ieRecordNotDefined, -1, RsEUnknownRecordType)`
            // (`:6111`).
            let ok = self.adapter.set_record(&value)
                || self
                    .shared_adapter
                    .as_ref()
                    .is_some_and(|shared| shared.borrow().set_record(&value));
            if !ok {
                return Err(FlowError::Error(ExecError::new(
                    IE_RECORD_NOT_DEFINED,
                    Some(call_end as i64),
                    RS_UNKNOWN_RECORD_TYPE,
                    "",
                )));
            }
        }
        if args.has_vars {
            self.update_var_params(&mut args)?;
        }
        if args.indexed && !args.return_indexed {
            let mut element = Value::Null;
            if !self.get_element(&value, &mut element, &mut args, call_end)? {
                return Err(FlowError::Error(ExecError::new(
                    IE_ARRAY_REQUIRED,
                    Some(call_start as i64),
                    "",
                    "",
                )));
            }
            return Ok(element);
        }
        Ok(value)
    }

    /// `TJvInterpreterExpression.GetElement` (`JvInterpreter.pas:6158`): the
    /// `[...]` form of a string (1-based characters), an interpreter array,
    /// a default indexed property, or a native variant array. A variable of
    /// another kind raises `ieArrayRequired` at `CurPos` (`:6235`).
    fn get_element(
        &mut self,
        variable: &Value,
        value: &mut Value,
        args: &mut Args,
        err_pos: usize,
    ) -> Result<bool, FlowError> {
        if args.values.is_empty() {
            return Ok(false);
        }
        match variable {
            Value::Str(bytes) => {
                if args.values.len() > 1 {
                    return Err(FlowError::Error(ExecError::new(
                        IE_ARRAY_TOO_MANY_PARAMS,
                        Some(err_pos as i64),
                        "",
                        "",
                    )));
                }
                if bytes.is_empty() {
                    return Err(FlowError::Error(
                        ExecError::external(RS_RANGE_CHECK_ERROR).with_pos(err_pos),
                    ));
                }
                let index = match values::to_i32(&args.values[0]) {
                    Ok(index) => index,
                    Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
                };
                if index < 1 || index as usize > bytes.len() {
                    return Err(FlowError::Error(
                        ExecError::external(RS_RANGE_CHECK_ERROR).with_pos(err_pos),
                    ));
                }
                *value = Value::Str(vec![bytes[index as usize - 1]]);
                Ok(true)
            }
            Value::Array(array) => {
                let array = array.borrow();
                if args.values.len() > array.begin.len() {
                    return Err(FlowError::Error(ExecError::new(
                        IE_ARRAY_TOO_MANY_PARAMS,
                        Some(err_pos as i64),
                        "",
                        "",
                    )));
                }
                if args.values.len() < array.begin.len() {
                    return Err(FlowError::Error(ExecError::new(
                        IE_ARRAY_NOT_ENOUGH_PARAMS,
                        Some(err_pos as i64),
                        "",
                        "",
                    )));
                }
                let mut indexes = Vec::with_capacity(args.values.len());
                for bound in &args.values {
                    match values::to_i32(bound) {
                        Ok(index) => indexes.push(i64::from(index)),
                        Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
                    }
                }
                match values::array_offset(&array.begin, &array.end, &indexes) {
                    Ok(offset) => {
                        *value = values::array_get(&array, offset);
                        Ok(true)
                    }
                    Err(error) => Err(FlowError::Error(self.value_error(error, err_pos))),
                }
            }
            Value::Object(_) | Value::Class(_) => {
                let result = match self.adapter.get_element(variable, value, args) {
                    Ok(result) => result,
                    Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
                };
                if result {
                    return Ok(true);
                }
                if let Some(shared) = self.shared_adapter.clone() {
                    match shared.borrow_mut().get_element(variable, value, args) {
                        Ok(result) => return Ok(result),
                        Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
                    }
                }
                Ok(false)
            }
            Value::VariantArray(array) => {
                let array = array.borrow();
                if args.values.len() > 1 {
                    return Err(FlowError::Error(ExecError::new(
                        IE_ARRAY_TOO_MANY_PARAMS,
                        Some(err_pos as i64),
                        "",
                        "",
                    )));
                }
                if args.values.is_empty() {
                    return Err(FlowError::Error(ExecError::new(
                        IE_ARRAY_NOT_ENOUGH_PARAMS,
                        Some(err_pos as i64),
                        "",
                        "",
                    )));
                }
                let index = match values::to_i32(&args.values[0]) {
                    Ok(index) => i64::from(index),
                    Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
                };
                if index < array.low || index >= array.low + array.values.len() as i64 {
                    return Err(FlowError::Error(
                        self.value_error(ValueError::interp(error::IE_ARRAY_INDEX_OUT_OF_BOUNDS), err_pos),
                    ));
                }
                *value = array.values[(index - array.low) as usize].clone();
                Ok(true)
            }
            _ => Err(FlowError::Error(ExecError::new(
                IE_ARRAY_REQUIRED,
                Some(err_pos as i64),
                "",
                "",
            ))),
        }
    }

    /// `TJvInterpreterExpression.SetElement` (`JvInterpreter.pas:6239`): the
    /// indexed assignment. Strings take the first character of the value
    /// (`string(Value)[1]`, `:6255`); the caller writes a changed string
    /// back.
    fn set_element(
        &mut self,
        variable: &mut Value,
        value: &Value,
        args: &mut Args,
        err_pos: usize,
    ) -> Result<bool, FlowError> {
        if args.values.is_empty() {
            return Ok(false);
        }
        match variable {
            Value::Str(bytes) => {
                if args.values.len() > 1 {
                    return Err(FlowError::Error(ExecError::new(
                        IE_ARRAY_TOO_MANY_PARAMS,
                        Some(err_pos as i64),
                        "",
                        "",
                    )));
                }
                let index = match values::to_i32(&args.values[0]) {
                    Ok(index) => index,
                    Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
                };
                let replacement = match values::to_str(value) {
                    Ok(text) => text,
                    Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
                };
                let Some(first) = replacement.first().copied() else {
                    return Err(FlowError::Error(
                        ExecError::external(RS_RANGE_CHECK_ERROR).with_pos(err_pos),
                    ));
                };
                if index < 1 || index as usize > bytes.len() {
                    return Err(FlowError::Error(
                        ExecError::external(RS_RANGE_CHECK_ERROR).with_pos(err_pos),
                    ));
                }
                bytes[index as usize - 1] = first;
                Ok(true)
            }
            Value::Array(array) => {
                let mut array = array.borrow_mut();
                if args.values.len() > array.begin.len() {
                    return Err(FlowError::Error(ExecError::new(
                        IE_ARRAY_TOO_MANY_PARAMS,
                        Some(err_pos as i64),
                        "",
                        "",
                    )));
                }
                if args.values.len() < array.begin.len() {
                    return Err(FlowError::Error(ExecError::new(
                        IE_ARRAY_NOT_ENOUGH_PARAMS,
                        Some(err_pos as i64),
                        "",
                        "",
                    )));
                }
                let mut indexes = Vec::with_capacity(args.values.len());
                for bound in &args.values {
                    match values::to_i32(bound) {
                        Ok(index) => indexes.push(i64::from(index)),
                        Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
                    }
                }
                match values::array_offset(&array.begin, &array.end, &indexes) {
                    Ok(offset) => match values::array_set(&mut array, offset, value) {
                        Ok(()) => Ok(true),
                        Err(error) => Err(FlowError::Error(self.value_error(error, err_pos))),
                    },
                    Err(error) => Err(FlowError::Error(self.value_error(error, err_pos))),
                }
            }
            Value::Object(_) | Value::Class(_) => {
                let result = match self.adapter.set_element(variable, value, args) {
                    Ok(result) => result,
                    Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
                };
                if result {
                    return Ok(true);
                }
                if let Some(shared) = self.shared_adapter.clone() {
                    match shared.borrow_mut().set_element(variable, value, args) {
                        Ok(result) => return Ok(result),
                        Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
                    }
                }
                Ok(false)
            }
            Value::VariantArray(array) => {
                let mut array = array.borrow_mut();
                if args.values.len() > 1 {
                    return Err(FlowError::Error(ExecError::new(
                        IE_ARRAY_TOO_MANY_PARAMS,
                        Some(err_pos as i64),
                        "",
                        "",
                    )));
                }
                if args.values.is_empty() {
                    return Err(FlowError::Error(ExecError::new(
                        IE_ARRAY_NOT_ENOUGH_PARAMS,
                        Some(err_pos as i64),
                        "",
                        "",
                    )));
                }
                let index = match values::to_i32(&args.values[0]) {
                    Ok(index) => i64::from(index),
                    Err(error) => return Err(FlowError::Error(self.value_error(error, err_pos))),
                };
                if index < array.low || index >= array.low + array.values.len() as i64 {
                    return Err(FlowError::Error(
                        self.value_error(ValueError::interp(error::IE_ARRAY_INDEX_OUT_OF_BOUNDS), err_pos),
                    ));
                }
                let low = array.low;
                let at = (index - low) as usize;
                array.values[at] = value.clone();
                Ok(true)
            }
            _ => Err(FlowError::Error(ExecError::new(
                IE_ARRAY_REQUIRED,
                Some(err_pos as i64),
                "",
                "",
            ))),
        }
    }

    //=== Statements ==========================================================

    /// `InterpretStatement` (`JvInterpreter.pas:6646`) with `DoOnStatement`
    /// first (`:6648`). `Break`, `Continue` and `Exit` set the interpreter's
    /// flags, which the enclosing block or loop answers.
    fn exec_statement(&mut self, stmt: &Stmt) -> Result<(), FlowError> {
        self.statement_hook(stmt.span.start)?;
        match &stmt.node {
            StmtNode::Assign { target, value } => self.exec_assign(target, value),
            StmtNode::Expr(expr) => {
                self.expression1(expr)?;
                Ok(())
            }
            StmtNode::Block(block) => self.exec_block(block),
            StmtNode::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let condition = self.expression2(cond, VarType::Bool)?;
                let truthy = matches!(condition, Value::Bool(true));
                if truthy {
                    self.exec_statement(then_branch)
                } else if let Some(else_branch) = else_branch {
                    self.exec_statement(else_branch)
                } else {
                    Ok(())
                }
            }
            StmtNode::While { cond, body } => self.exec_while(stmt, cond, body),
            StmtNode::Repeat { body, cond } => self.exec_repeat(body, cond),
            StmtNode::For {
                var,
                from,
                to,
                down,
                body,
            } => self.exec_for(var, from, to, *down, body),
            StmtNode::Case {
                selector,
                arms,
                else_arm,
            } => self.exec_case(selector, arms, else_arm),
            StmtNode::Try { body, handler } => self.exec_try(body, handler),
            StmtNode::Raise { value } => self.exec_raise(value),
            StmtNode::Break => {
                self.break_flag = true;
                Ok(())
            }
            StmtNode::Continue => {
                self.continue_flag = true;
                Ok(())
            }
            StmtNode::Exit => {
                self.exit_flag = true;
                Ok(())
            }
            // The hook above is all `InterpretStatement` does for the no-op
            // arms (`:6658-6667`).
            StmtNode::Empty => Ok(()),
            // A `function` inside a block is the front end's acceptance;
            // `InterpretStatement` reports the token (`:6687`).
            StmtNode::LocalRoutine(_) => Err(FlowError::Error(self.expected_at(stmt.span.start, "';'"))),
        }
    }

    /// `InterpretBegin` (`JvInterpreter.pas:7042`): a statement list, with
    /// the `DoOnStatement` of every separating `;` (`:7056`) and the flag
    /// exit (`:7066`).
    fn exec_block(&mut self, block: &Block) -> Result<(), FlowError> {
        for stmt in &block.stmts {
            self.exec_statement(stmt)?;
            if self.break_flag || self.continue_flag || self.exit_flag {
                return Ok(());
            }
            self.hook_statement_separators(stmt.span.end)?;
        }
        Ok(())
    }

    /// `InterpretIdentifier` (`JvInterpreter.pas:7015`) with
    /// `InternalSetValue` (`:6993`): the value first, then the receiver for
    /// an indexed or member target. The event-assignment path
    /// (`InternalSetValue`'s method-property branch, `:6904`) needs RTTI and
    /// belongs to the VCL shim.
    fn exec_assign(&mut self, target: &Expr, value: &Expr) -> Result<(), FlowError> {
        match &target.node {
            ExprNode::Ident {
                name,
                args,
                indexed: false,
            } if args.is_empty() => {
                let value_value = self.expression1(value)?;
                let pos = self.following_token_start(value.span.end);
                let mut call_args = Args::new();
                call_args.assignment = true;
                if !self.set_value(name, &value_value, &mut call_args, pos)? {
                    return Err(FlowError::Error(ExecError::new(
                        IE_UNKNOWN_IDENTIFIER,
                        Some(pos as i64),
                        name,
                        "",
                    )));
                }
                Ok(())
            }
            ExprNode::Ident { name, args, indexed } => self.assign_target(None, name, args, *indexed, value),
            ExprNode::Member {
                base,
                name,
                args,
                indexed,
            } => {
                let receiver = self.eval_expr(base)?;
                if !matches!(receiver.var_type(), VarType::Object | VarType::Class | VarType::Record) {
                    let pos = self.member_name_pos(base.span.end);
                    return Err(FlowError::Error(ExecError::new(
                        IE_ROC_REQUIRED,
                        Some(pos as i64),
                        "",
                        "",
                    )));
                }
                self.assign_target(Some(receiver), name, args, *indexed, value)
            }
            _ => Err(FlowError::Error(self.expected_at(target.span.start, "';'"))),
        }
    }

    /// The indexed or member arm of `InternalSetValue` (`:6985-7012`): the
    /// target's arguments evaluate first (they were parsed before the
    /// `:=`), then the value; an indexed target reads the element and writes
    /// it back when it is a string or an array.
    fn assign_target(
        &mut self,
        receiver: Option<Value>,
        name: &str,
        arg_exprs: &[Arg],
        indexed: bool,
        value: &Expr,
    ) -> Result<(), FlowError> {
        let mut target_args = self.build_call_args(arg_exprs, indexed)?;
        target_args.obj = receiver;
        let value_value = self.expression1(value)?;
        let pos = self.following_token_start(value.span.end);
        if indexed {
            let mut variable = Value::Null;
            if self.get_value(name.as_bytes(), &mut variable, &mut target_args, self.cur_pos)? {
                if !self.set_element(&mut variable, &value_value, &mut target_args, self.cur_pos)? {
                    return Err(FlowError::Error(ExecError::new(
                        IE_ARRAY_REQUIRED,
                        Some(pos as i64),
                        "",
                        "",
                    )));
                }
                // A changed string writes back; `VarIsArray` is true only
                // for a *native* variant array -- the interpreter's own
                // arrays carry the custom `varArray` type, which lacks the
                // `$2000` array flag (`System.varArray`), so an element
                // write stays inside the shared array record and no
                // write-back happens (a write-back would raise the
                // array-to-array assignment error, `JvInterpreter.pas:6985-7001`).
                if (variable.is_str() || matches!(variable, Value::VariantArray(_)))
                    && !self.set_value(name, &variable, &mut target_args, pos)?
                {
                    return Err(FlowError::Error(ExecError::new(
                        IE_UNKNOWN_IDENTIFIER,
                        Some(pos as i64),
                        name,
                        "",
                    )));
                }
                Ok(())
            } else {
                if !self.set_value(name, &value_value, &mut target_args, pos)? {
                    return Err(FlowError::Error(ExecError::new(
                        IE_UNKNOWN_IDENTIFIER,
                        Some(pos as i64),
                        name,
                        "",
                    )));
                }
                Ok(())
            }
        } else {
            if !self.set_value(name, &value_value, &mut target_args, pos)? {
                return Err(FlowError::Error(ExecError::new(
                    IE_UNKNOWN_IDENTIFIER,
                    Some(pos as i64),
                    name,
                    "",
                )));
            }
            Ok(())
        }
    }

    /// `InterpretWhile` (`JvInterpreter.pas:7113`): the condition each pass,
    /// `ieBooleanRequired` at the position after `while` (`:7128`), and the
    /// flag resets of the body arm.
    fn exec_while(&mut self, stmt: &Stmt, cond: &Expr, body: &Stmt) -> Result<(), FlowError> {
        let while_pos = stmt.span.start + 5; // the `while` keyword
        loop {
            let condition = self.expression1(cond)?;
            let truthy = match condition {
                Value::Bool(value) => value,
                _ => {
                    return Err(FlowError::Error(ExecError::new(
                        IE_BOOLEAN_REQUIRED,
                        Some(while_pos as i64),
                        "",
                        "",
                    )));
                }
            };
            if !truthy {
                break;
            }
            self.continue_flag = false;
            self.break_flag = false;
            self.exec_statement(body)?;
            if self.break_flag || self.exit_flag {
                break;
            }
        }
        self.continue_flag = false;
        self.break_flag = false;
        Ok(())
    }

    /// `InterpretRepeat` (`JvInterpreter.pas:7154`): the body then the
    /// `until` condition, `ieBooleanRequired` at `CurPos` (`:7182`).
    fn exec_repeat(&mut self, body: &Block, cond: &Expr) -> Result<(), FlowError> {
        loop {
            for stmt in &body.stmts {
                self.exec_statement(stmt)?;
                if self.break_flag || self.exit_flag {
                    self.continue_flag = false;
                    self.break_flag = false;
                    return Ok(());
                }
                self.hook_statement_separators(stmt.span.end)?;
            }
            let condition = self.expression1(cond)?;
            let truthy = match condition {
                Value::Bool(value) => value,
                _ => {
                    let pos = self.following_token_end(cond.span.end);
                    return Err(FlowError::Error(ExecError::new(
                        IE_BOOLEAN_REQUIRED,
                        Some(pos as i64),
                        "",
                        "",
                    )));
                }
            };
            if truthy {
                break;
            }
        }
        self.continue_flag = false;
        self.break_flag = false;
        Ok(())
    }

    /// `InterpretFor` (`JvInterpreter.pas:7203`): both bounds through
    /// `Expression2(varInteger)` -- the 64-bit acceptance of
    /// [`Interpreter::expression2`] -- and the loop variable written through
    /// `SetValue` each pass (`:7240`).
    fn exec_for(&mut self, var: &str, from: &Expr, to: &Expr, down: bool, body: &Stmt) -> Result<(), FlowError> {
        let from_value = self.expression2(from, VarType::Integer)?;
        let to_value = self.expression2(to, VarType::Integer)?;
        let (begin, end) = match (from_value, to_value) {
            (Value::Integer(begin), Value::Integer(end)) => (begin, end),
            _ => unreachable!("expression2 returned an integer"),
        };
        let iterations: Box<dyn Iterator<Item = i32>> = if down {
            Box::new((end..=begin).rev())
        } else {
            Box::new(begin..=end)
        };
        for index in iterations {
            let mut args = Args::new();
            let pos = self.cur_pos;
            if !self.set_value(var, &Value::Integer(index), &mut args, pos)? {
                return Err(FlowError::Error(ExecError::new(
                    IE_UNKNOWN_IDENTIFIER,
                    Some(body.span.start as i64),
                    var,
                    "",
                )));
            }
            self.continue_flag = false;
            self.break_flag = false;
            self.exec_statement(body)?;
            if self.break_flag || self.exit_flag {
                break;
            }
        }
        self.continue_flag = false;
        self.break_flag = false;
        Ok(())
    }

    /// `InterpretCase` (`JvInterpreter.pas:7283`): the selector and every
    /// label through `Expression2(varInteger)`, arms in source order, `else`
    /// after them.
    fn exec_case(&mut self, selector: &Expr, arms: &[CaseArm], else_arm: &Option<Box<Stmt>>) -> Result<(), FlowError> {
        let selector_value = self.expression2(selector, VarType::Integer)?;
        let selector_int = match selector_value {
            Value::Integer(value) => value,
            _ => unreachable!("expression2 returned an integer"),
        };
        for arm in arms {
            let mut ranges: Vec<(i32, i32)> = Vec::with_capacity(arm.labels.len());
            for label in &arm.labels {
                let lo = self.expression2(&label.lo, VarType::Integer)?;
                let lo = match lo {
                    Value::Integer(value) => value,
                    _ => unreachable!(),
                };
                let hi = match &label.hi {
                    Some(hi) => match self.expression2(hi, VarType::Integer)? {
                        Value::Integer(value) => value,
                        _ => unreachable!(),
                    },
                    None => lo,
                };
                ranges.push((lo, hi));
            }
            // `InCase` (`:7288`).
            if ranges.iter().any(|(lo, hi)| selector_int >= *lo && selector_int <= *hi) {
                return self.exec_statement(&arm.body);
            }
        }
        if let Some(else_arm) = else_arm {
            return self.exec_statement(else_arm);
        }
        Ok(())
    }

    /// `InterpretTry` (`JvInterpreter.pas:7435`) with `DoFinallyExcept`
    /// (`:7560`): the `finally` block runs for a normal exit, a flag exit
    /// and an exception (which then re-raises); the `except` handler only
    /// for an exception.
    fn exec_try(&mut self, body: &Block, handler: &Handler) -> Result<(), FlowError> {
        match self.exec_block(body) {
            Ok(()) => self.do_finally_except(handler, None),
            Err(flow) => match handler {
                Handler::Finally(finally) => {
                    self.run_finally(finally)?;
                    Err(flow)
                }
                Handler::Except { .. } => self.exec_except(handler, flow),
            },
        }
    }

    /// `DoFinallyExcept(nil)` / the flag exit of `InterpretTry` (`:7628`).
    fn do_finally_except(&mut self, handler: &Handler, _error: Option<FlowError>) -> Result<(), FlowError> {
        match handler {
            Handler::Finally(finally) => self.run_finally(finally),
            // `if E = nil then SkipToEnd` (`:7577`).
            Handler::Except { .. } => Ok(()),
        }
    }

    /// The `finally` block with the `FExit` save-and-merge of
    /// `DoFinallyExcept` (`:7562`).
    fn run_finally(&mut self, finally: &Block) -> Result<(), FlowError> {
        let old_exit = self.exit_flag;
        self.exit_flag = false;
        let result = self.exec_block(finally);
        self.exit_flag = self.exit_flag || old_exit;
        result
    }

    /// `DoFinallyExcept`'s `except` arm (`JvInterpreter.pas:7575-7595`):
    /// `InterpretExcept` runs; a bare `raise;` inside it re-raises the
    /// original exception, another `EJvInterpreterError` is swallowed (the
    /// upstream wrapper's `on E1: EJvInterpreterError` catches every code
    /// and only acts on `ieRaise`), anything else replaces the original.
    fn exec_except(&mut self, handler: &Handler, error: FlowError) -> Result<(), FlowError> {
        let Handler::Except { ons, body, else_arm } = handler else {
            unreachable!("except handler")
        };
        let outcome = self.interpret_except(ons, body, else_arm, &error);
        match outcome {
            Ok(()) => {
                // `FLastError.Clear` (`:7585`).
                self.last_error = ExecError::cleared();
                Ok(())
            }
            Err(FlowError::ReRaise) => Err(error),
            Err(FlowError::Error(_)) => Ok(()),
            Err(other) => Err(other),
        }
    }

    /// `InterpretExcept` (`JvInterpreter.pas:7457`): the `on` chain with its
    /// class test, or the plain body for any exception.
    fn interpret_except(
        &mut self,
        ons: &[OnClause],
        body: &Option<Block>,
        else_arm: &Option<Box<Stmt>>,
        error: &FlowError,
    ) -> Result<(), FlowError> {
        if let Some(plain) = body {
            return self.exec_block(plain);
        }
        for on in ons {
            let class = self.resolve_exception_class(on)?;
            if self.exception_matches(error, &class) {
                let object_value = self.exception_object_value(error);
                let bound = on.var.clone();
                if let Some(name) = &bound {
                    // `LocalVars.AddVar('', ExceptionVarName,
                    // ExceptionClassName, varObject, O2V(E), ...)` (`:7492`).
                    let result = {
                        let frame = self.frames.last_mut().expect("frame");
                        frame.locals.add_var(
                            "",
                            name,
                            &on.class,
                            ParamType::value(VarType::Object),
                            &object_value,
                            Some(DataType::Simple(VarType::Object)),
                        )
                    };
                    if let Err(error) = result {
                        let pos = self.cur_pos;
                        return Err(FlowError::Error(self.value_error(error, pos)));
                    }
                }
                let result = self.exec_statement(&on.body);
                if let Some(name) = &bound
                    && let Some(frame) = self.frames.last_mut()
                {
                    frame.locals.delete_var("", name.as_bytes());
                }
                result?;
                return Ok(());
            }
        }
        match else_arm {
            Some(arm) => self.exec_statement(arm),
            // No `on` matched and no `else`: `ReRaiseException := True`
            // (`:7533`).
            None => Err(FlowError::ReRaise),
        }
    }

    /// `On1`'s class resolution (`JvInterpreter.pas:7463`): the class name
    /// through `GetValue`, `ieUnknownIdentifier` or `ieClassRequired` at the
    /// token after it (`PosBeg`, `:7481`).
    fn resolve_exception_class(&mut self, on: &OnClause) -> Result<Rc<ClassDef>, FlowError> {
        let pos = self.exception_class_token_pos(on);
        let mut args = Args::new();
        let mut value = Value::Null;
        if !self.get_value(on.class.as_bytes(), &mut value, &mut args, pos)? {
            return Err(FlowError::Error(ExecError::new(
                IE_UNKNOWN_IDENTIFIER,
                Some(pos as i64),
                &on.class,
                "",
            )));
        }
        if value.var_type() != VarType::Class {
            return Err(FlowError::Error(ExecError::new(
                IE_CLASS_REQUIRED,
                Some(pos as i64),
                "",
                "",
            )));
        }
        Ok(value.as_class().expect("checked").clone())
    }

    /// The token after the `on` clause's class name: where `On1` stands when
    /// `GetValue` runs (`:7480`).
    fn exception_class_token_pos(&self, on: &OnClause) -> usize {
        let Some((_, mut at, _)) = self.token_after(on.span.start) else {
            return on.span.start;
        };
        // `on` then the name (or `Name:` then the class name).
        let first = match self.token_after(at) {
            Some((start, end, _)) => (start, end),
            None => return on.span.start,
        };
        if on.var.is_some() {
            // first = the bound name; skip ':' to the class name.
            let colon = match self.token_after(first.1) {
                Some((_, end, _)) => end,
                None => return first.1,
            };
            let class_token = match self.token_after(colon) {
                Some((_, end, _)) => end,
                None => return first.1,
            };
            at = class_token;
        } else {
            at = first.1;
        }
        self.token_after(at).map(|(start, _, _)| start).unwrap_or(at)
    }

    /// `E is ExceptionClass` (`JvInterpreter.pas:7487`): a raised object
    /// through its class chain, an interpreter error through the built-in
    /// `EJvInterpreterError` (a Delphi `Exception`).
    fn exception_matches(&mut self, error: &FlowError, class: &Rc<ClassDef>) -> bool {
        match error {
            FlowError::Raise(raised) => raised.class.is_a(class),
            FlowError::Error(_) => self.interp_error_class().is_a(class),
            FlowError::ReRaise => false,
        }
    }

    /// The object an `on E` clause binds (`O2V(E)`).
    fn exception_object_value(&mut self, error: &FlowError) -> Value {
        match error {
            FlowError::Raise(raised) => Value::Object(raised.object.clone()),
            _ => {
                let class = self.interp_error_class();
                let exec = match error {
                    FlowError::Error(exec) => exec.clone(),
                    _ => ExecError::cleared(),
                };
                Value::Object(Rc::new(RefCell::new(InterpExceptionObject { error: exec, class })))
            }
        }
    }

    /// The `EJvInterpreterError` class, parented to the registered
    /// `Exception` when there is one, so `on E: Exception` matches an
    /// interpreter error as Delphi's RTTI does upstream.
    fn interp_error_class(&mut self) -> Rc<ClassDef> {
        if let Some(class) = &self.interp_error_class {
            return class.clone();
        }
        let parent = self.adapter.find_class(b"Exception");
        let class = Rc::new(ClassDef {
            unit: String::new(),
            name: "EJvInterpreterError".to_owned(),
            parent,
        });
        self.interp_error_class = Some(class.clone());
        class
    }

    /// `InterpretRaise` (`JvInterpreter.pas:7655`): a bare `raise;` is the
    /// internal `ieRaise` error; `raise X` needs an object and records the
    /// external error state (`UpdateExceptionPos(Exception(V2O(V)), '')`,
    /// `:7669`).
    fn exec_raise(&mut self, value: &Option<Expr>) -> Result<(), FlowError> {
        let Some(expr) = value else {
            return Err(FlowError::ReRaise);
        };
        let raised = self.expression1(expr)?;
        if raised.var_type() != VarType::Object {
            let pos = self.following_token_start(expr.span.end);
            return Err(FlowError::Error(ExecError::new(
                IE_CLASS_REQUIRED,
                Some(pos as i64),
                "",
                "",
            )));
        }
        let object = raised.as_object().expect("checked").clone();
        let (class, message) = {
            let borrowed = object.borrow();
            (borrowed.class(), borrowed.message().unwrap_or_default().to_owned())
        };
        let mut exec = ExecError::external(&message);
        exec.pos = Some(self.cur_pos as i64);
        self.update_exception_pos(&mut exec, "");
        Err(FlowError::Raise(RaisedException { class, object, message }))
    }

    //=== Hooks, tokens and errors ============================================

    /// `DoOnStatement` (`JvInterpreter.pas:6640`): the statement hook of
    /// `TJvInterpreterProgram` (`:8534`).
    fn statement_hook(&mut self, pos: usize) -> Result<(), FlowError> {
        if let Some(hook) = &mut self.on_statement {
            hook(pos)?;
        }
        Ok(())
    }

    /// The `DoOnStatement` of every `;` between statements
    /// (`InterpretBegin`'s ttSemicolon arm, `:7056`); the tokenizer stops
    /// at the first non-`;` token, so only real separators count (a `;`
    /// inside a remark is not a token).
    fn hook_statement_separators(&mut self, pos: usize) -> Result<(), FlowError> {
        let mut at = pos;
        while let Some((start, end, text)) = self.token_after(at) {
            if text != b";" {
                return Ok(());
            }
            self.statement_hook(start)?;
            at = end;
        }
        Ok(())
    }

    /// The next token of the current source from a byte offset: the runtime
    /// reconstructs upstream's current token (`FTokenStr` at `CurPos`) with
    /// it.
    fn token_after(&self, pos: usize) -> Option<(usize, usize, Vec<u8>)> {
        let mut tokenizer = Tokenizer::new(self.cur_source.as_slice());
        tokenizer.set_pos(pos);
        let token = tokenizer.token().ok()?;
        if token.text.is_empty() {
            return None;
        }
        Some((token.start, token.end, token.text))
    }

    /// Upstream's `PosBeg`: the start of the token the parse reached.
    fn following_token_start(&self, pos: usize) -> usize {
        self.token_after(pos).map(|(start, _, _)| start).unwrap_or(pos)
    }

    /// Upstream's `CurPos`: the end of the token the parse reached.
    fn following_token_end(&self, pos: usize) -> usize {
        self.token_after(pos).map(|(_, end, _)| end).unwrap_or(pos)
    }

    /// The start of a member's name token, the `PosBeg` of the
    /// `ieROCRequired` a non-object receiver raises (`:6146`).
    fn member_name_pos(&self, base_end: usize) -> usize {
        let Some((_, dot_end, text)) = self.token_after(base_end) else {
            return base_end;
        };
        if text != b"." {
            return base_end;
        }
        self.token_after(dot_end).map(|(start, _, _)| start).unwrap_or(base_end)
    }

    /// `ErrorExpected` (`JvInterpreter.pas:5448`): the message with the
    /// token found at `pos` (the current token upstream).
    fn expected_at(&self, pos: usize, exp: &str) -> ExecError {
        match self.token_after(pos) {
            Some((_, _, text)) => ExecError::new(
                IE_EXPECTED,
                Some(pos as i64),
                exp,
                &format!("'{}'", String::from_utf8_lossy(&text)),
            ),
            // `LoadStr2(irEndOfFile)` for the empty token (`:5453`).
            None => ExecError::new(IE_EXPECTED, Some(pos as i64), exp, error::message_text(304)),
        }
    }

    /// A [`ValueError`] as the interpreter's error at a position.
    fn value_error(&self, error: ValueError, pos: usize) -> ExecError {
        match error {
            ValueError::Variant => ExecError::new(IE_TYPE_MISMATCH, Some(pos as i64), "", ""),
            ValueError::External(message) => {
                let mut exec = ExecError::external(&message);
                exec.pos = Some(pos as i64);
                exec
            }
            ValueError::Interp { code, name1, name2 } => ExecError::new(code, Some(pos as i64), &name1, &name2),
        }
    }

    /// `UpdateExceptionPos` (`JvInterpreter.pas:5363`): the position, the
    /// unit and the line are filled once; the message becomes
    /// `Error in unit '...' on line n : ...` (the `ieExternal` text for a
    /// non-interpreter exception); `FLastError` keeps the result.
    fn update_exception_pos(&mut self, exec: &mut ExecError, unit: &str) {
        if !exec.exception_pos {
            if exec.pos.is_none() {
                exec.pos = Some(self.cur_pos as i64);
            }
            if exec.unit_name.is_empty() {
                exec.unit_name = unit.to_owned();
            }
            if !exec.unit_name.is_empty() {
                // `GetLineByPos(FParser.Source, E.FErrPos) + BaseErrLine + 1`
                // (`JvInterpreter.pas:5376`); [`error::line_of`] already
                // counts the "first line has number 1".
                exec.line = error::line_of(&self.cur_source, exec.pos.unwrap_or(0)) + self.base_err_line;
                exec.message = if exec.external {
                    error::format_external_error_pos(&exec.unit_name, exec.line, &exec.err_message)
                } else {
                    error::format_error_pos(&exec.unit_name, exec.line, &exec.err_message)
                };
                exec.exception_pos = true;
            }
        }
        if exec.external {
            if !self.last_error.exception_pos {
                self.last_error = exec.clone();
            }
        } else {
            self.last_error = exec.clone();
        }
    }

    /// The `except on E` arms of `ExecFunction`/`InFunction`
    /// (`UpdateExceptionPos(E, unit)` before the state is restored): the
    /// error message gains the routine's unit and line. A raised object takes
    /// the external path (`:5390`).
    fn update_flow(&mut self, flow: FlowError, unit: &str) -> FlowError {
        match flow {
            FlowError::Error(mut exec) => {
                self.update_exception_pos(&mut exec, unit);
                FlowError::Error(exec)
            }
            FlowError::Raise(raised) => {
                if !self.last_error.exception_pos {
                    let mut exec = ExecError::external(&raised.message);
                    exec.pos = Some(self.cur_pos as i64);
                    self.update_exception_pos(&mut exec, unit);
                }
                FlowError::Raise(raised)
            }
            FlowError::ReRaise => FlowError::ReRaise,
        }
    }
}

impl ExecError {
    fn with_pos(mut self, pos: usize) -> ExecError {
        self.pos = Some(pos as i64);
        self
    }
}

/// `ParseDataType`'s array item type: `TypeName2VarTyp` of the element
/// type's first token (`JvInterpreter.pas:7789`), `varEmpty` for a nested
/// array (whose first token is `array`).
fn element_item_type(element: &TypeRef) -> VarType {
    match element {
        TypeRef::Named(name) => values::type_name_2_var_type(name.as_bytes()),
        TypeRef::Array { .. } => VarType::Empty,
    }
}

/// The type name a declared type is registered under: `ParseDataType` keeps
/// the first token (`TypName := Token`, `JvInterpreter.pas:7691`).
fn type_ref_name(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Named(name) => name.clone(),
        TypeRef::Array { .. } => "array".to_owned(),
    }
}
