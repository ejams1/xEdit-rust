// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter.pas (TJvInterpreterAdapter
// with its registration methods and the GetValue/SetValue/GetElement/SetElement
// dispatch, TJvInterpreterArgs, TJvInterpreterVarList, the identifier lists
// with their sorted Find, TJvInterpreterFunctionDesc and TJvInterpreterSrcUnit)

//! The adapter model: how the interpreter reaches the world.
//!
//! `TJvInterpreterAdapter` (`JvInterpreter.pas:586`) holds every registration
//! the host made -- functions, constants, object methods and properties,
//! indexed and default-indexed accessors, record types, classes, external
//! units, event handlers -- and answers the interpreter's identifier lookups
//! in the exact order of its `GetValue` (`:3939`) and `SetValue` (`:4573`).
//! [`Adapter`] ports that class; the per-kind registration lists keep the
//! sorted-list `Find` (`TJvInterpreterIdentifierList.Find`, `:2913`) with its
//! `AnsiStrIComp` case-insensitive ordering, including the method-list
//! tie-break that orders a base class's method before a derived class's
//! (`SortMethodIdentifier`, `:8627`).
//!
//! Where upstream stores a Delphi function pointer, the port stores an
//! [`Rc`]'d closure (`-> Result<(), ValueError>`); where upstream passes a
//! `Data: Pointer` through to `CheckAction`, the port has no seam because
//! xEdit never overrides `CheckAction` (grep of `xEdit/` 2026-10-10) and a
//! Rust closure captures what it needs. `AddIntfGet` and the other
//! `IUnknown` registrations are not ported: they need `varUnknown` values,
//! which do not exist without the VCL/COM world, and no xEdit adapter
//! registers one. The OLE automation path (`JvInterpreter_OLEAUTO`) is
//! compiled out of the xEdit build and not ported either.
//!
//! The script-level source functions and variables live here too, because
//! upstream keeps them in the adapter: [`FunctionDesc`] is what `Compile`
//! registers per routine and `ExecFunction` runs, [`VarList`] is
//! `TJvInterpreterVarList` (globals of every unit and function locals share
//! its semantics, newest entry first, `Cmp` name matching, typed coercion on
//! assignment).

use std::any::Any;
use std::cell::{RefCell, RefMut};
use std::cmp::Ordering;
use std::rc::Rc;

use crate::ast::{RoutineBody, RoutineDecl};
use crate::error::{
    self, Error, IE_EVENT_NOT_REGISTERED, IE_IDENTIFIER_REDECLARED, IE_NOT_ENOUGH_PARAMS, IE_TOO_MANY_PARAMS,
    RS_SORRY_FOR_ONE_DIMENSIONAL_ARRAYS_ONLY,
};
use crate::interpreter;
use crate::values::{
    self, ClassDef, DataType, ParamType, RecordDef, RecordField, RecordInstance, ScriptObject, Value, ValueError,
    VarType,
};

/// `prArgsNoCheck` (`JvInterpreter.pas:1220`): a registration that takes any
/// number of arguments.
pub const PR_ARGS_NO_CHECK: i32 = -1;

/// `Cmp` (`JvInterpreter.pas:1733`) as an `Ordering`: case-insensitive
/// equality for the ASCII names every registration uses.
fn cmp(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// `AnsiStrIComp`: the case-insensitive ordinal comparison the identifier
/// lists sort and search with. Upstream compares through the user locale's
/// tables; for the ASCII names of every registration that is the
/// case-folded byte comparison (a punctuation byte such as `_` sorts above
/// the letters here and below them in the locale's linguistic order -- the
/// lists are self-consistent either way, and the order is not observable
/// outside them).
fn ansi_str_i_comp(a: &[u8], b: &[u8]) -> Ordering {
    let n = a.len().min(b.len());
    for i in 0..n {
        let ca = a[i].to_ascii_uppercase();
        let cb = b[i].to_ascii_uppercase();
        if ca != cb {
            return ca.cmp(&cb);
        }
    }
    a.len().cmp(&b.len())
}

/// `TJvInterpreterIdentifierList.Find` (`JvInterpreter.pas:2913`): the
/// binary search over the [`ansi_str_i_comp`] ordering. A hit with
/// `duplicates_accept` continues leftwards, so `index` is the leftmost equal
/// entry -- the lists the scan-after-find callers use (methods, properties)
/// are the `dupAccept` ones; the rest land on the probed entry, as upstream's
/// `L := I` leaves it. Without a hit, `index` is the insertion point.
fn list_find<T>(items: &[T], key: impl Fn(&T) -> &[u8], identifier: &[u8], duplicates_accept: bool) -> (bool, usize) {
    let mut result = false;
    let mut l: i64 = 0;
    let mut h: i64 = items.len() as i64 - 1;
    while l <= h {
        let i = (l + h) >> 1;
        let c = ansi_str_i_comp(key(&items[i as usize]), identifier);
        if c == Ordering::Less {
            l = i + 1;
        } else {
            h = i - 1;
            if c == Ordering::Equal {
                result = true;
                if !duplicates_accept {
                    l = i;
                }
            }
        }
    }
    (result, l.max(0) as usize)
}

/// `SortIdentifier` (`JvInterpreter.pas:2941`).
fn sort_identifier<T>(items: &mut [T], key: impl Fn(&T) -> &[u8]) {
    items.sort_by(|a, b| ansi_str_i_comp(key(a), key(b)));
}

/// `SortMethodIdentifier` (`JvInterpreter.pas:8627`): by identifier first,
/// then by class -- an entry whose class inherits the other's sorts before
/// it. Upstream's tie-break is not a total order (two unrelated classes
/// compare "greater" both ways and `InheritsFrom` is true for the class
/// itself); the port makes identical and unrelated classes equal, so the
/// stable sort keeps registration order there, and a derived class's entry
/// precedes its base's.
fn sort_method<T>(items: &mut [T], key: impl Fn(&T) -> &[u8], class_of: impl Fn(&T) -> Option<&Rc<ClassDef>>) {
    items.sort_by(|a, b| {
        let by_name = ansi_str_i_comp(key(a), key(b));
        if by_name != Ordering::Equal {
            return by_name;
        }
        match (class_of(a), class_of(b)) {
            (Some(a), Some(b)) => {
                if Rc::ptr_eq(a, b) {
                    Ordering::Equal
                } else if a.is_a(b) {
                    Ordering::Less
                } else if b.is_a(a) {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            }
            _ => Ordering::Equal,
        }
    });
}

/// `TJvInterpreterArgs` (`JvInterpreter.pas:249`): one call's arguments, its
/// receiver and its state. Upstream's fixed 33-slot arrays are `Vec`s here;
/// `FVarNames` is the private per-argument call-site variable name that
/// `var`/`out` parameters are written back through
/// (`InternalGetValue.UpdateVarParams`, `:6060`).
#[derive(Clone)]
pub struct Args {
    pub identifier: String,
    pub values: Vec<Value>,
    /// The declared parameter types of the callee, set by
    /// [`Adapter::check_args`] (`CheckArgs`: `Args.Types := ParamTypes`).
    pub types: Vec<ParamType>,
    pub has_result: bool,
    pub assignment: bool,
    /// The receiver of a method or record access, with its variant type in
    /// `obj_type`.
    pub obj: Option<Value>,
    pub indexed: bool,
    pub return_indexed: bool,
    /// `FHasVars`: some argument is a `var` parameter of a named variable, so
    /// the caller must write the values back after the call.
    pub has_vars: bool,
    var_names: Vec<Option<String>>,
}

impl Args {
    pub fn new() -> Self {
        Args {
            identifier: String::new(),
            values: Vec::new(),
            types: Vec::new(),
            has_result: false,
            assignment: false,
            obj: None,
            indexed: false,
            return_indexed: false,
            has_vars: false,
            var_names: Vec::new(),
        }
    }

    /// `TJvInterpreterArgs.Clear` (`JvInterpreter.pas:5302`).
    pub fn clear(&mut self) {
        self.values.clear();
        self.obj = None;
        self.has_vars = false;
        self.indexed = false;
        self.return_indexed = false;
        self.var_names.clear();
    }

    /// The call-site variable an argument names (upstream `FVarNames[I]`).
    pub fn set_var_name(&mut self, index: usize, name: Option<String>) {
        if self.var_names.len() <= index {
            self.var_names.resize(index + 1, None);
        }
        self.var_names[index] = name;
    }

    pub fn var_name(&self, index: usize) -> Option<&str> {
        self.var_names.get(index).and_then(|name| name.as_deref())
    }

    /// `Args.ObjTyp` (`TVarData(Args.Obj).VType`; `0` for no receiver).
    pub fn obj_type(&self) -> VarType {
        self.obj.as_ref().map(Value::var_type).unwrap_or(VarType::Empty)
    }

    /// `P2R` (`JvInterpreter.pas:1583`): the record holder behind
    /// `Args.Obj`, the receiver every `AddRec*` function works on.
    pub fn record_mut(&mut self) -> Result<RefMut<'_, RecordInstance>, ValueError> {
        match &self.obj {
            Some(Value::Record(record)) => Ok(record.borrow_mut()),
            _ => Err(ValueError::roc_required()),
        }
    }

    /// `V2O(Args.Obj)` for an adapter object.
    pub fn object(&self) -> Option<&Rc<RefCell<dyn ScriptObject>>> {
        self.obj.as_ref().and_then(Value::as_object)
    }

    /// `TJvInterpreterArgs.OpenArray` (`JvInterpreter.pas:5313`) with
    /// `V2OA` (`:2367`): the element values of an open-array argument. A
    /// one-dimensional interpreter array contributes its slots in storage
    /// order (`V2OA` indexes from 0), a native variant array its elements.
    /// The `TVarRec` currency conversion `V2OA` applies to double elements
    /// belongs to the Delphi call machinery, not the values.
    pub fn open_array(&mut self, index: usize) -> Result<Vec<Value>, ValueError> {
        let Some(value) = self.values.get(index) else {
            return Err(ValueError::Variant);
        };
        match value {
            Value::Array(array) => {
                let array = array.borrow();
                if array.begin.len() > 1 {
                    return Err(ValueError::External(
                        RS_SORRY_FOR_ONE_DIMENSIONAL_ARRAYS_ONLY.to_owned(),
                    ));
                }
                Ok(array.values.clone())
            }
            Value::VariantArray(array) => Ok(array.borrow().values.clone()),
            _ => Err(ValueError::Variant),
        }
    }
}

impl Default for Args {
    fn default() -> Self {
        Args::new()
    }
}

/// The closure shape of `TJvInterpreterAdapterGetValue`: `procedure(var
/// Value: Variant; Args: TJvInterpreterArgs)` (`JvInterpreter.pas:233`).
pub type AdapterGetFn = Rc<dyn Fn(&mut Value, &mut Args) -> Result<(), ValueError>>;

/// The closure shape of `TJvInterpreterAdapterSetValue`
/// (`JvInterpreter.pas:234`).
pub type AdapterSetFn = Rc<dyn Fn(&Value, &mut Args) -> Result<(), ValueError>>;

/// The host's `CallDll` (`JvInterpreter.pas:2035`): how an `external`
/// routine is called.
pub type ExternalCallFn = Rc<dyn Fn(&ExtFunction, &mut Args) -> Result<Value, ValueError>>;

/// One `FOnGetList` entry (`JvInterpreter.pas:610`),
/// `procedure(Sender, Identifier, var Value, Args, var Done)`.
pub type OnGetHandler = Rc<dyn Fn(&str, &mut Value, &mut Args) -> Result<bool, ValueError>>;

/// One `FOnSetList` entry.
pub type OnSetHandler = Rc<dyn Fn(&str, &Value, &mut Args) -> Result<bool, ValueError>>;

/// `TJvInterpreterMethod` (`JvInterpreter.pas:428`): one registered
/// function, property reader or indexed reader.
#[derive(Clone)]
pub struct Method {
    pub identifier: String,
    pub unit_name: String,
    /// `FClassType`: the receiver class for methods and properties.
    pub class_type: Option<Rc<ClassDef>>,
    pub func: Option<AdapterGetFn>,
    /// `FParamCount`: `-1` is `prArgsNoCheck`.
    pub param_count: i32,
    pub param_types: Vec<ParamType>,
    pub res_typ: VarType,
}

/// `TJvInterpreterMethod` as a writer (`AddSet*`): no result.
#[derive(Clone)]
pub struct SetMethod {
    pub identifier: String,
    pub unit_name: String,
    pub class_type: Option<Rc<ClassDef>>,
    pub func: Option<AdapterSetFn>,
    pub param_count: i32,
    pub param_types: Vec<ParamType>,
}

/// `TJvInterpreterRecMethod` (`JvInterpreter.pas:483`) of `FRecordGetList`.
#[derive(Clone)]
pub struct RecGetMethod {
    pub record: Rc<RecordDef>,
    pub identifier: String,
    pub func: AdapterGetFn,
    pub param_count: i32,
    pub param_types: Vec<ParamType>,
    pub res_typ: VarType,
}

/// `TJvInterpreterRecMethod` of `FRecordSetList`.
#[derive(Clone)]
pub struct RecSetMethod {
    pub record: Rc<RecordDef>,
    pub identifier: String,
    pub func: AdapterSetFn,
    pub param_count: i32,
    pub param_types: Vec<ParamType>,
}

/// `TJvInterpreterConst` (`JvInterpreter.pas:463`): `AddConst`.
#[derive(Clone)]
pub struct ConstEntry {
    pub unit_name: String,
    pub identifier: String,
    pub value: Value,
}

/// `TJvInterpreterSrcUnit` (`JvInterpreter.pas:417`): `AddSrcUnit`, one per
/// unit of script source (and the host's stubs).
#[derive(Clone)]
pub struct SrcUnitEntry {
    pub identifier: String,
    pub source: Rc<Vec<u8>>,
    pub uses_list: Vec<String>,
}

/// The `varObject` of a source unit (`O2V(TJvInterpreterSrcUnit)`,
/// `GetSrcUnit`, `JvInterpreter.pas:4393`): member access on it dispatches
/// into the unit's functions and variables.
pub struct SrcUnitObject {
    pub name: String,
    class: Rc<ClassDef>,
}

impl ScriptObject for SrcUnitObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The name of a [`SrcUnitObject`] value, for the unit-qualified dispatch
/// (`GetValue`'s `Args.Obj is TJvInterpreterSrcUnit`, `JvInterpreter.pas:8334`).
pub fn src_unit_name(value: &Value) -> Option<String> {
    let Value::Object(object) = value else {
        return None;
    };
    let object = object.borrow();
    object
        .as_any()
        .downcast_ref::<SrcUnitObject>()
        .map(|unit| unit.name.clone())
}

/// `FExtUnitList` entries (`AddExtUnit`, `JvInterpreter.pas:3254`).
#[derive(Clone)]
pub struct ExtUnitEntry {
    pub identifier: String,
}

/// `TJvInterpreterExtFunction` (`JvInterpreter.pas:528`): a routine declared
/// with an `external` directive (`AddExtFun`, `:3721`). Calling one needs
/// the host -- the DLL loading of `CallDll` (`:2035`) -- through
/// [`Adapter::external_call`].
#[derive(Clone)]
pub struct ExtFunction {
    pub unit_name: String,
    pub identifier: String,
    pub dll: Value,
    pub function_name: String,
    pub function_index: i64,
    pub param_count: i32,
    pub param_types: Vec<ParamType>,
    pub res_typ: VarType,
}

/// `TJvInterpreterEventDesc` (`JvInterpreter.pas:538`): `AddHandler`. The
/// `EventClass`/`Code` pair is the VCL method-pointer thunk the form shim
/// (phase 6 step 9) provides; the port keeps the registered type name.
#[derive(Clone)]
pub struct EventDesc {
    pub unit_name: String,
    pub identifier: String,
    pub event_class: String,
}

/// `FEventList` entries (`AddEvent`, `JvInterpreter.pas:3822`): a property
/// of the class holds a script event.
#[derive(Clone)]
pub struct EventEntry {
    pub unit_name: String,
    pub class: Rc<ClassDef>,
    pub identifier: String,
}

/// What `NewEvent` (`JvInterpreter.pas:5235`) resolves an event type to: the
/// registration plus the script function a fired event calls back through
/// `CallFunctionEx` (`TJvInterpreterEvent.CallFunction`, `:2893`). The
/// method-pointer storage in the control is the form shim's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventHandle {
    pub event_class: String,
    pub unit_name: String,
    pub function_name: String,
}

/// `TJvInterpreterFunctionDesc` (`JvInterpreter.pas:284`): one registered
/// source routine. `body_pos` is where `ExecFunction` re-parses the body
/// (`FPosBeg := CurPos`, `:7959`); the first execution parses and caches it
/// (`[`routine_body`](FunctionDesc::routine_body)`), and a parse failure is
/// cached and re-raised per call, as upstream re-parses and fails per call.
pub struct FunctionDesc {
    pub unit_name: String,
    pub identifier: String,
    /// `FClassIdentifier`: the `TClass.` prefix of a method declaration.
    pub class_identifier: String,
    pub body_pos: Option<usize>,
    pub source: Rc<Vec<u8>>,
    pub param_count: usize,
    pub param_types: Vec<ParamType>,
    pub param_type_names: Vec<String>,
    pub param_names: Vec<String>,
    pub res_typ: VarType,
    pub res_typ_name: String,
    pub res_data_type: Option<DataType>,
    pub decl: Rc<RoutineDecl>,
    body: RefCell<BodyCache>,
}

enum BodyCache {
    NotParsed,
    Parsed(Rc<RoutineBody>),
    Failed(Error),
}

impl FunctionDesc {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        unit_name: String,
        identifier: String,
        class_identifier: String,
        body_pos: Option<usize>,
        source: Rc<Vec<u8>>,
        param_count: usize,
        param_types: Vec<ParamType>,
        param_type_names: Vec<String>,
        param_names: Vec<String>,
        res_typ: VarType,
        res_typ_name: String,
        res_data_type: Option<DataType>,
        decl: Rc<RoutineDecl>,
    ) -> FunctionDesc {
        FunctionDesc {
            unit_name,
            identifier,
            class_identifier,
            body_pos,
            source,
            param_count,
            param_types,
            param_type_names,
            param_names,
            res_typ,
            res_typ_name,
            res_data_type,
            decl,
            body: RefCell::new(BodyCache::NotParsed),
        }
    }

    /// The body `ExecFunction` runs: the routine's `var`/`const` sections and
    /// `begin ... end` block, parsed from `body_pos` on first use (the
    /// full-parse tree when `Compile` already has it).
    pub fn routine_body(&self) -> Result<Rc<RoutineBody>, Error> {
        let mut cache = self.body.borrow_mut();
        match &*cache {
            BodyCache::Parsed(body) => return Ok(body.clone()),
            BodyCache::Failed(error) => return Err(error.clone()),
            BodyCache::NotParsed => {}
        }
        let parsed = match (&self.decl.body, self.body_pos) {
            (Some(block), _) => Ok(Rc::new(RoutineBody {
                locals: self.decl.locals.clone(),
                block: block.clone(),
            })),
            (None, Some(pos)) => interpreter::parse_body(&self.source, pos).map(Rc::new),
            (None, None) => Err(Error::new(error::IE_INTERNAL, -1, "no body", "")),
        };
        match parsed {
            Ok(body) => {
                *cache = BodyCache::Parsed(body.clone());
                Ok(body)
            }
            Err(error) => {
                *cache = BodyCache::Failed(error.clone());
                Err(error)
            }
        }
    }
}

/// `TJvInterpreterClass` (`JvInterpreter.pas:452`) as `FSrcClassList` holds
/// it (`AddSrcClass`, `:3850`): a `type X = class(Y)` of script source. The
/// class fields are the form instances of the VCL shim (phase 6 step 9); the
/// port registers the name.
#[derive(Clone)]
pub struct SrcClassEntry {
    pub unit_name: String,
    pub identifier: String,
}

/// `FSrcVarList` (`JvInterpreter.pas:616`): unit variables and constants
/// (`AddSrcVar`) and, through `InterpretVar`'s `AddVarFunc`, the locals of a
/// running function. One entry is `TJvInterpreterVar` (`:353`); the list
/// keeps `TJvInterpreterVarList` (`:365`) semantics -- newest entry first,
/// the last added of a name wins, `Cmp` matching, and the typed coercion of
/// `SetValue` (`:2694`).
#[derive(Clone)]
pub struct VarEntry {
    pub unit_name: String,
    pub identifier: String,
    pub typ: String,
    pub vtyp: ParamType,
    pub value: Value,
    pub data_type: Option<DataType>,
}

#[derive(Default)]
pub struct VarList {
    entries: Vec<VarEntry>,
}

impl VarList {
    pub fn new() -> Self {
        VarList::default()
    }

    pub fn entries(&self) -> &[VarEntry] {
        &self.entries
    }

    /// `AddVar` (`JvInterpreter.pas:2614`): a redeclared name of the same
    /// unit raises `ieIdentifierRedeclared`; the entry goes to the front.
    pub fn add_var(
        &mut self,
        unit_name: &str,
        identifier: &str,
        typ: &str,
        vtyp: ParamType,
        value: &Value,
        data_type: Option<DataType>,
    ) -> Result<(), ValueError> {
        if self.find_var(unit_name, identifier.as_bytes()).is_some() {
            return Err(ValueError::interp_n(IE_IDENTIFIER_REDECLARED, identifier));
        }
        let mut copied = Value::Null;
        values::jv_var_copy(&mut copied, value);
        self.entries.insert(
            0,
            VarEntry {
                unit_name: unit_name.to_owned(),
                identifier: identifier.to_owned(),
                typ: typ.to_owned(),
                vtyp,
                value: copied,
                data_type,
            },
        );
        Ok(())
    }

    /// `FindVar` (`JvInterpreter.pas:2630`): from the front (newest first),
    /// `Cmp` on both names; an empty `unit_name` matches any unit.
    pub fn find_var(&self, unit_name: &str, identifier: &[u8]) -> Option<usize> {
        self.entries.iter().position(|entry| {
            cmp(entry.identifier.as_bytes(), identifier)
                && (unit_name.is_empty() || cmp(entry.unit_name.as_bytes(), unit_name.as_bytes()))
        })
    }

    /// `DeleteVar` (`JvInterpreter.pas:2645`).
    pub fn delete_var(&mut self, unit_name: &str, identifier: &[u8]) {
        if let Some(index) = self.find_var(unit_name, identifier) {
            self.entries.remove(index);
        }
    }

    pub fn value_of(&self, unit_name: &str, identifier: &[u8]) -> Option<&Value> {
        self.find_var(unit_name, identifier)
            .map(|index| &self.entries[index].value)
    }

    /// `TJvInterpreterVarList.GetValue` (`JvInterpreter.pas:2664`): a bare
    /// name any unit, a member of a source-unit object its unit's name.
    pub fn get_value(&self, identifier: &[u8], value: &mut Value, args: &Args) -> bool {
        let found = match &args.obj {
            None => self.find_var("", identifier),
            Some(obj) => match src_unit_name(obj) {
                Some(unit_name) => self.find_var(&unit_name, identifier),
                None => None,
            },
        };
        match found {
            Some(index) => {
                values::jv_var_copy(value, &self.entries[index].value);
                true
            }
            None => false,
        }
    }

    /// `TJvInterpreterVarList.SetValue` (`JvInterpreter.pas:2694`): a bare
    /// name of any unit and only without a receiver; a typed variable
    /// (`VTyp` neither `varEmpty` nor `varVariant`) coerces through
    /// `JvInterpreterVarAsType` when the assigned type differs.
    pub fn set_value(&mut self, identifier: &[u8], value: &Value, args: &Args) -> Result<bool, ValueError> {
        let Some(index) = self.find_var("", identifier) else {
            return Ok(false);
        };
        if args.obj.is_some() {
            return Ok(false);
        }
        let entry = &mut self.entries[index];
        let vtyp = entry.vtyp;
        if vtyp.typ != VarType::Empty && vtyp.typ != VarType::Variant && value.var_type() != vtyp.typ {
            let converted = values::var_as_type(value, vtyp.typ)?;
            values::jv_var_assignment(&mut entry.value, &converted)?;
        } else {
            values::jv_var_assignment(&mut entry.value, value)?;
        }
        Ok(true)
    }
}

/// The interpreter's adapter: every registration and the dispatch that
/// answers `GetValue`/`SetValue`/`GetElement`/`SetElement`.
#[derive(Default)]
pub struct Adapter {
    src_unit_list: Vec<SrcUnitEntry>,
    ext_unit_list: Vec<ExtUnitEntry>,
    get_list: Vec<Method>,
    set_list: Vec<SetMethod>,
    iget_list: Vec<Method>,
    iset_list: Vec<SetMethod>,
    id_get_list: Vec<Method>,
    id_set_list: Vec<SetMethod>,
    class_list: Vec<Rc<ClassDef>>,
    const_list: Vec<ConstEntry>,
    function_list: Vec<Method>,
    record_list: Vec<Rc<RecordDef>>,
    record_get_list: Vec<RecGetMethod>,
    record_set_list: Vec<RecSetMethod>,
    src_function_list: Vec<Rc<FunctionDesc>>,
    ext_function_list: Vec<ExtFunction>,
    event_handler_list: Vec<EventDesc>,
    event_list: Vec<EventEntry>,
    /// `FSrcClassList` (`JvInterpreter.pas:617`): script `type X = class`
    /// declarations (`AddSrcClass`).
    src_class_list: Vec<SrcClassEntry>,
    /// `FSrcVarList` (`JvInterpreter.pas:616`): unit variables and constants
    /// (`AddSrcVar`) and, through `InterpretVar`'s `AddVarFunc`, the locals
    /// of a running function.
    pub src_var_list: VarList,
    sorted: bool,
    /// `DisableExternalFunctions` (`JvInterpreter.pas:811`).
    pub disable_external_functions: bool,
    /// The host's `CallDll` (`JvInterpreter.pas:2035`): how an `external`
    /// routine is called. A host without DLL loading leaves it `None`, and
    /// the call fails with a clear message (owed to the host steps).
    pub external_call: Option<ExternalCallFn>,
    /// `FOnGetList` (`JvInterpreter.pas:610`): adapter-level fallbacks.
    pub on_get_list: Vec<OnGetHandler>,
    /// `FOnSetList`.
    pub on_set_list: Vec<OnSetHandler>,
    src_unit_class: Option<Rc<ClassDef>>,
}

impl Adapter {
    pub fn new() -> Self {
        Adapter {
            src_unit_class: Some(Rc::new(ClassDef {
                unit: String::new(),
                name: "TJvInterpreterSrcUnit".to_owned(),
                parent: None,
            })),
            ..Adapter::default()
        }
    }

    //=== Registration ========================================================

    /// `AddFunction` (`JvInterpreter.pas:3590`).
    pub fn add_function(
        &mut self,
        unit_name: &str,
        identifier: &str,
        func: AdapterGetFn,
        param_count: i32,
        param_types: Vec<ParamType>,
        res_typ: VarType,
    ) {
        self.function_list.push(Method {
            identifier: identifier.to_owned(),
            unit_name: unit_name.to_owned(),
            class_type: None,
            func: Some(func),
            param_count,
            param_types,
            res_typ,
        });
        self.sorted = false;
    }

    /// `AddConst` (`JvInterpreter.pas:3701`).
    pub fn add_const(&mut self, unit_name: &str, identifier: &str, value: Value) {
        self.const_list.push(ConstEntry {
            unit_name: unit_name.to_owned(),
            identifier: identifier.to_owned(),
            value,
        });
        self.sorted = false;
    }

    /// `AddClass` (`JvInterpreter.pas:3269`). Upstream takes the `TClass` and
    /// reads the hierarchy from RTTI; the port registers the name and lets
    /// the caller name the parent (the class identity `is` checks use).
    pub fn add_class(&mut self, unit_name: &str, name: &str, parent: Option<Rc<ClassDef>>) -> Rc<ClassDef> {
        let class = Rc::new(ClassDef {
            unit: unit_name.to_owned(),
            name: name.to_owned(),
            parent,
        });
        self.class_list.push(class.clone());
        self.sorted = false;
        class
    }

    /// `AddGet` (`JvInterpreter.pas:3289`).
    pub fn add_get(
        &mut self,
        class: Rc<ClassDef>,
        identifier: &str,
        func: AdapterGetFn,
        param_count: i32,
        param_types: Vec<ParamType>,
        res_typ: VarType,
    ) {
        self.get_list.push(Method {
            identifier: identifier.to_owned(),
            unit_name: class.unit.clone(),
            class_type: Some(class),
            func: Some(func),
            param_count,
            param_types,
            res_typ,
        });
        self.sorted = false;
    }

    /// `AddSet` (`JvInterpreter.pas:3457`).
    pub fn add_set(
        &mut self,
        class: Rc<ClassDef>,
        identifier: &str,
        func: AdapterSetFn,
        param_count: i32,
        param_types: Vec<ParamType>,
    ) {
        self.set_list.push(SetMethod {
            identifier: identifier.to_owned(),
            unit_name: class.unit.clone(),
            class_type: Some(class),
            func: Some(func),
            param_count,
            param_types,
        });
        self.sorted = false;
    }

    /// `AddIGet` (`JvInterpreter.pas:3314`).
    pub fn add_iget(
        &mut self,
        class: Rc<ClassDef>,
        identifier: &str,
        func: AdapterGetFn,
        param_count: i32,
        param_types: Vec<ParamType>,
        res_typ: VarType,
    ) {
        self.iget_list.push(Method {
            identifier: identifier.to_owned(),
            unit_name: class.unit.clone(),
            class_type: Some(class),
            func: Some(func),
            param_count,
            param_types,
            res_typ,
        });
        self.sorted = false;
    }

    /// `AddISet` (`JvInterpreter.pas:3480`).
    pub fn add_iset(
        &mut self,
        class: Rc<ClassDef>,
        identifier: &str,
        func: AdapterSetFn,
        param_count: i32,
        param_types: Vec<ParamType>,
    ) {
        self.iset_list.push(SetMethod {
            identifier: identifier.to_owned(),
            unit_name: class.unit.clone(),
            class_type: Some(class),
            func: Some(func),
            param_count,
            param_types,
        });
        self.sorted = false;
    }

    /// `AddIDGet` (`JvInterpreter.pas:3339`): a default indexed property,
    /// matched by class only -- the list has no identifiers and is scanned
    /// linearly (`GetID`, `:4825`).
    pub fn add_id_get(
        &mut self,
        class: Rc<ClassDef>,
        func: AdapterGetFn,
        param_count: i32,
        param_types: Vec<ParamType>,
        res_typ: VarType,
    ) {
        self.id_get_list.push(Method {
            identifier: String::new(),
            unit_name: class.unit.clone(),
            class_type: Some(class),
            func: Some(func),
            param_count,
            param_types,
            res_typ,
        });
    }

    /// `AddIDSet` (`JvInterpreter.pas:3503`).
    pub fn add_id_set(
        &mut self,
        class: Rc<ClassDef>,
        func: AdapterSetFn,
        param_count: i32,
        param_types: Vec<ParamType>,
    ) {
        self.id_set_list.push(SetMethod {
            identifier: String::new(),
            unit_name: class.unit.clone(),
            class_type: Some(class),
            func: Some(func),
            param_count,
            param_types,
        });
    }

    /// `AddRec` (`JvInterpreter.pas:3615`).
    pub fn add_rec(&mut self, unit_name: &str, identifier: &str, size: i64, fields: Vec<RecordField>) -> Rc<RecordDef> {
        let def = Rc::new(RecordDef {
            unit: unit_name.to_owned(),
            name: identifier.to_owned(),
            size,
            fields,
        });
        self.record_list.push(def.clone());
        def
    }

    /// `AddRecGet` (`JvInterpreter.pas:3651`): the record-field readers of a
    /// record type. The lookup of the record by name (`GetRec`,
    /// `:3878`) happens at the call site; the port takes the registered type.
    pub fn add_rec_get(
        &mut self,
        record: Rc<RecordDef>,
        identifier: &str,
        func: AdapterGetFn,
        param_count: i32,
        param_types: Vec<ParamType>,
        res_typ: VarType,
    ) {
        self.record_get_list.push(RecGetMethod {
            record,
            identifier: identifier.to_owned(),
            func,
            param_count,
            param_types,
            res_typ,
        });
    }

    /// `AddRecSet` (`JvInterpreter.pas:3677`).
    pub fn add_rec_set(
        &mut self,
        record: Rc<RecordDef>,
        identifier: &str,
        func: AdapterSetFn,
        param_count: i32,
        param_types: Vec<ParamType>,
    ) {
        self.record_set_list.push(RecSetMethod {
            record,
            identifier: identifier.to_owned(),
            func,
            param_count,
            param_types,
        });
    }

    /// `AddExtUnit` (`JvInterpreter.pas:3254`): a unit name the host itself
    /// implements, so `uses` of it reads no file.
    pub fn add_ext_unit(&mut self, identifier: &str) {
        self.ext_unit_list.push(ExtUnitEntry {
            identifier: identifier.to_owned(),
        });
    }

    /// `AddSrcUnit` (`JvInterpreter.pas:3207`): registers a unit's source;
    /// an entry whose source is already set is left alone, so the first
    /// definition wins and the cycle guard's empty registration fills later.
    pub fn add_src_unit(&mut self, identifier: &str, source: &[u8], uses_list: &str) {
        if let Some(entry) = self
            .src_unit_list
            .iter_mut()
            .find(|entry| cmp(entry.identifier.as_bytes(), identifier.as_bytes()))
        {
            if !entry.source.is_empty() {
                return;
            }
            entry.identifier = identifier.to_owned();
            entry.source = Rc::new(source.to_vec());
            entry.uses_list = parse_uses_list(uses_list);
            return;
        }
        self.src_unit_list.push(SrcUnitEntry {
            identifier: identifier.to_owned(),
            source: Rc::new(source.to_vec()),
            uses_list: parse_uses_list(uses_list),
        });
    }

    /// `AddSrcFun` (`JvInterpreter.pas:3753`): the entry `FindFunDesc`
    /// (`:3919`) later finds by backwards scan.
    pub fn add_src_function(&mut self, desc: FunctionDesc) {
        self.src_function_list.push(Rc::new(desc));
    }

    /// `AddSrcClass` (`JvInterpreter.pas:3850`): a script
    /// `type X = class(Y)` declaration. The class fields are the form
    /// instances of the VCL shim (phase 6 step 9); the port registers the
    /// name for the event-assignment path.
    pub fn add_src_class(&mut self, unit_name: &str, identifier: &str) {
        self.src_class_list.push(SrcClassEntry {
            unit_name: unit_name.to_owned(),
            identifier: identifier.to_owned(),
        });
    }

    /// `GetSrcClass` (`JvInterpreter.pas:3855`): a linear `IndexOf('', ...)`.
    pub fn get_src_class(&self, identifier: &[u8]) -> Option<&SrcClassEntry> {
        self.src_class_list
            .iter()
            .find(|entry| cmp(entry.identifier.as_bytes(), identifier))
    }

    /// `AddSrcVar` (`JvInterpreter.pas:3844`).
    pub fn add_src_var(
        &mut self,
        unit_name: &str,
        identifier: &str,
        typ: &str,
        vtyp: ParamType,
        value: &Value,
        data_type: Option<DataType>,
    ) -> Result<(), ValueError> {
        self.src_var_list
            .add_var(unit_name, identifier, typ, vtyp, value, data_type)
    }

    /// `AddExtFun` (`JvInterpreter.pas:3721`): an `external` declaration.
    #[allow(clippy::too_many_arguments)]
    pub fn add_ext_fun(
        &mut self,
        unit_name: &str,
        identifier: &str,
        dll: Value,
        function_name: &str,
        function_index: i64,
        param_count: i32,
        param_types: Vec<ParamType>,
        res_typ: VarType,
    ) {
        self.ext_function_list.push(ExtFunction {
            unit_name: unit_name.to_owned(),
            identifier: identifier.to_owned(),
            dll,
            function_name: function_name.to_owned(),
            function_index,
            param_count,
            param_types,
            res_typ,
        });
    }

    /// `AddHandler` (`JvInterpreter.pas:3799`).
    pub fn add_handler(&mut self, unit_name: &str, identifier: &str, event_class: &str) {
        self.event_handler_list.push(EventDesc {
            unit_name: unit_name.to_owned(),
            identifier: identifier.to_owned(),
            event_class: event_class.to_owned(),
        });
    }

    /// `AddEvent` (`JvInterpreter.pas:3822`).
    pub fn add_event(&mut self, unit_name: &str, class: Rc<ClassDef>, identifier: &str) {
        self.event_list.push(EventEntry {
            unit_name: unit_name.to_owned(),
            class,
            identifier: identifier.to_owned(),
        });
    }

    //=== Lookups =============================================================

    /// `Sort` (`JvInterpreter.pas:5275`): every `Find`-searched list, once,
    /// after any add to one of them invalidated `FSorted`.
    fn ensure_sorted(&mut self) {
        if self.sorted {
            return;
        }
        sort_identifier(&mut self.const_list, |entry| entry.identifier.as_bytes());
        sort_identifier(&mut self.class_list, |class| class.name.as_bytes());
        sort_method(
            &mut self.function_list,
            |m| m.identifier.as_bytes(),
            |m| m.class_type.as_ref(),
        );
        sort_method(
            &mut self.get_list,
            |m| m.identifier.as_bytes(),
            |m| m.class_type.as_ref(),
        );
        sort_method(
            &mut self.set_list,
            |m| m.identifier.as_bytes(),
            |m| m.class_type.as_ref(),
        );
        sort_method(
            &mut self.iget_list,
            |m| m.identifier.as_bytes(),
            |m| m.class_type.as_ref(),
        );
        sort_method(
            &mut self.iset_list,
            |m| m.identifier.as_bytes(),
            |m| m.class_type.as_ref(),
        );
        self.sorted = true;
    }

    /// `UnitExists` (`JvInterpreter.pas:5214`): a source unit or an external
    /// one.
    pub fn unit_exists(&self, identifier: &[u8]) -> bool {
        self.src_unit_list
            .iter()
            .any(|entry| cmp(entry.identifier.as_bytes(), identifier))
            || self
                .ext_unit_list
                .iter()
                .any(|entry| cmp(entry.identifier.as_bytes(), identifier))
    }

    /// `GetRec` (`JvInterpreter.pas:3878`): a linear `Cmp` scan.
    pub fn get_rec(&self, identifier: &[u8]) -> Option<Rc<RecordDef>> {
        self.record_list
            .iter()
            .find(|def| cmp(def.name.as_bytes(), identifier))
            .cloned()
    }

    /// The `FClassList.Find` of `GetClass`(`JvInterpreter.pas:4310`).
    pub fn find_class(&mut self, identifier: &[u8]) -> Option<Rc<ClassDef>> {
        self.ensure_sorted();
        let (found, index) = list_find(&self.class_list, |class| class.name.as_bytes(), identifier, false);
        if found {
            Some(self.class_list[index].clone())
        } else {
            None
        }
    }

    /// `FindFunDesc` (`JvInterpreter.pas:3919`): the newest definition of a
    /// name in the unit wins (backwards scan); without one in the unit, a
    /// classless global of any unit answers a unit-qualified lookup.
    pub fn find_fun_desc(
        &self,
        unit_name: &str,
        identifier: &[u8],
        class_identifier: &[u8],
    ) -> Option<Rc<FunctionDesc>> {
        for desc in self.src_function_list.iter().rev() {
            if cmp(desc.identifier.as_bytes(), identifier)
                && (class_identifier.is_empty() || cmp(desc.class_identifier.as_bytes(), class_identifier))
                && (unit_name.is_empty() || cmp(desc.unit_name.as_bytes(), unit_name.as_bytes()))
            {
                return Some(desc.clone());
            }
        }
        if !unit_name.is_empty() && class_identifier.is_empty() {
            self.find_fun_desc("", identifier, b"")
        } else {
            None
        }
    }

    /// `CheckArgs` (`JvInterpreter.pas:3891`): the count check and the
    /// `FHasVars` computation of `var` arguments.
    pub fn check_args(&self, args: &mut Args, param_count: i32, param_types: &[ParamType]) -> Result<(), ValueError> {
        if param_count == PR_ARGS_NO_CHECK {
            return Ok(());
        }
        if args.values.len() as i32 > param_count {
            return Err(ValueError::interp(IE_TOO_MANY_PARAMS));
        }
        if (args.values.len() as i32) < param_count {
            return Err(ValueError::interp(IE_NOT_ENOUGH_PARAMS));
        }
        args.has_vars = false;
        args.types = param_types.to_vec();
        for index in 0..args.values.len() {
            if args.var_name(index).is_some_and(|name| !name.is_empty())
                && args.types.get(index).is_some_and(|typ| typ.by_ref)
            {
                args.has_vars = true;
                break;
            }
        }
        Ok(())
    }

    /// `NewEvent` (`JvInterpreter.pas:5235`): resolve an event type to its
    /// registered handler; an unregistered type raises
    /// `ieEventNotRegistered` (`:6863`).
    pub fn new_event(
        &self,
        unit_name: &str,
        function_name: &str,
        event_type: &[u8],
    ) -> Result<EventHandle, ValueError> {
        for desc in &self.event_handler_list {
            if cmp(desc.identifier.as_bytes(), event_type) {
                return Ok(EventHandle {
                    event_class: desc.event_class.clone(),
                    unit_name: unit_name.to_owned(),
                    function_name: function_name.to_owned(),
                });
            }
        }
        Err(ValueError::interp_n(
            IE_EVENT_NOT_REGISTERED,
            String::from_utf8_lossy(event_type).into_owned(),
        ))
    }

    /// `IsEvent` (`JvInterpreter.pas:5257`): whether an identifier of an
    /// object names an event property.
    pub fn is_event(&self, obj: &Value, identifier: &[u8]) -> bool {
        let Some(object) = obj.as_object() else {
            return false;
        };
        let class = object.borrow().class();
        self.event_list
            .iter()
            .any(|entry| class.is_a(&entry.class) && cmp(entry.identifier.as_bytes(), identifier))
    }

    /// `TJvInterpreterAdapter.SetRecord` (`JvInterpreter.pas:4962`): resolve
    /// a record value's type; the port's holders always carry their
    /// registered definition, so this reports whether it is one.
    pub fn set_record(&self, value: &Value) -> bool {
        match value {
            Value::Record(record) => {
                let def = record.borrow().def.clone();
                self.record_list.iter().any(|entry| Rc::ptr_eq(entry, &def))
            }
            _ => false,
        }
    }

    //=== GetValue ============================================================

    /// `TJvInterpreterAdapter.GetValue` (`JvInterpreter.pas:3939`).
    pub fn get_value(&mut self, identifier: &[u8], value: &mut Value, args: &mut Args) -> Result<bool, ValueError> {
        self.ensure_sorted();

        if args.indexed {
            if let Some(obj) = args.obj.clone() {
                match obj.var_type() {
                    VarType::Record => {
                        if self.get_record(&obj, identifier, value, args)? {
                            args.return_indexed = false;
                            return Ok(true);
                        }
                    }
                    VarType::Object | VarType::Class => {
                        if self.i_get_method(identifier, value, args)? {
                            return Ok(true);
                        }
                        // Try the property as a plain method with the index
                        // arguments dropped (`Args.Count := 0`, `:4467`).
                        let values = std::mem::take(&mut args.values);
                        let result = self.get_method(identifier, value, args);
                        args.values = values;
                        if result? {
                            return Ok(true);
                        }
                    }
                    _ => {}
                }
            }
        } else if let Some(obj) = args.obj.clone() {
            match obj.var_type() {
                VarType::Object | VarType::Class => {
                    if self.get_method(identifier, value, args)? {
                        return Ok(true);
                    }
                }
                VarType::Record if self.get_record(&obj, identifier, value, args)? => {
                    return Ok(true);
                }
                _ => {}
            }
        } else {
            if self.get_class(identifier, value, args)? {
                return Ok(true);
            }
            if self.get_const(identifier, value, args)? {
                return Ok(true);
            }
            if self.get_fun(identifier, value, args)? {
                return Ok(true);
            }
            if self.get_ext_fun(identifier, value, args)? {
                return Ok(true);
            }
            if self.type_cast(identifier, value, args) {
                return Ok(true);
            }
        }

        // Source variables, then source units, then the OnGet chain.
        if self.src_var_list.get_value(identifier, value, args) {
            return Ok(true);
        }
        if !matches!(args.obj_type(), VarType::Object | VarType::Class) && self.get_src_unit(identifier, value, args)? {
            return Ok(true);
        }
        for handler in &self.on_get_list {
            let handler = handler.clone();
            if handler(&String::from_utf8_lossy(identifier), value, args)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `GetMethod`'s `FGetList` scan (`JvInterpreter.pas:3953-3972`): from
    /// the leftmost sorted hit forward while the identifier still matches,
    /// the first entry whose class the receiver is (or, for a class-value
    /// receiver, whose class it equals) answers. `Free` frees the receiver
    /// (`:3973`).
    fn get_method(&self, identifier: &[u8], value: &mut Value, args: &mut Args) -> Result<bool, ValueError> {
        let (found, index) = list_find(&self.get_list, |m| m.identifier.as_bytes(), identifier, true);
        if found {
            for method in &self.get_list[index..] {
                if !cmp(method.identifier.as_bytes(), identifier) {
                    break;
                }
                if method.func.is_none() || !class_matches(args, method.class_type.as_ref()) {
                    continue;
                }
                args.identifier = String::from_utf8_lossy(identifier).into_owned();
                self.check_args(args, method.param_count, &method.param_types)?;
                let func = method.func.clone().expect("checked");
                func(value, args)?;
                return Ok(true);
            }
        }
        if cmp(identifier, b"Free")
            && let Some(object) = args.obj.as_ref().and_then(Value::as_object)
        {
            object.borrow_mut().free();
            args.obj = None;
            *value = Value::Null;
            return Ok(true);
        }
        Ok(false)
    }

    /// `IGetMethod` (`JvInterpreter.pas:4011`): `AddIGet` readers, matched
    /// by class and identifier; a hit marks `ReturnIndexed`.
    fn i_get_method(&self, identifier: &[u8], value: &mut Value, args: &mut Args) -> Result<bool, ValueError> {
        let (found, index) = list_find(&self.iget_list, |m| m.identifier.as_bytes(), identifier, true);
        if found {
            for method in &self.iget_list[index..] {
                if !cmp(method.identifier.as_bytes(), identifier) {
                    break;
                }
                if method.func.is_none() || !class_matches(args, method.class_type.as_ref()) {
                    continue;
                }
                args.identifier = String::from_utf8_lossy(identifier).into_owned();
                self.check_args(args, method.param_count, &method.param_types)?;
                let func = method.func.clone().expect("checked");
                func(value, args)?;
                args.return_indexed = true;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `GetRecord` (`JvInterpreter.pas:4223`): a field by name, then the
    /// record's `AddRecGet` methods.
    fn get_record(
        &self,
        receiver: &Value,
        identifier: &[u8],
        value: &mut Value,
        args: &mut Args,
    ) -> Result<bool, ValueError> {
        let record = values::v2r(receiver)?;
        let def = record.borrow().def.clone();
        if let Some(index) = values::record_field_index(&def, identifier) {
            *value = values::record_read_field(&record.borrow(), index);
            return Ok(true);
        }
        for method in &self.record_get_list {
            if Rc::ptr_eq(&method.record, &def) && cmp(method.identifier.as_bytes(), identifier) {
                args.identifier = String::from_utf8_lossy(identifier).into_owned();
                self.check_args(args, method.param_count, &method.param_types)?;
                let func = method.func.clone();
                func(value, args)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `GetConst` (`JvInterpreter.pas:4279`): `nil`, `Null`, then the list.
    fn get_const(&self, identifier: &[u8], value: &mut Value, args: &mut Args) -> Result<bool, ValueError> {
        if cmp(identifier, b"nil") {
            *value = Value::Pointer(0);
            return Ok(true);
        }
        if cmp(identifier, b"Null") {
            *value = Value::Null;
            return Ok(true);
        }
        let (found, index) = list_find(&self.const_list, |entry| entry.identifier.as_bytes(), identifier, false);
        if found {
            args.identifier = String::from_utf8_lossy(identifier).into_owned();
            *value = self.const_list[index].value.clone();
            return Ok(true);
        }
        Ok(false)
    }

    /// `GetClass` (`JvInterpreter.pas:4305`): the class value, or the one
    /// typecast `TClass(value)` -- upstream retags the value to `varObject`
    /// unless it is already a `varClass` (the port keeps the value).
    fn get_class(&self, identifier: &[u8], value: &mut Value, args: &mut Args) -> Result<bool, ValueError> {
        let (found, index) = list_find(&self.class_list, |class| class.name.as_bytes(), identifier, false);
        if !found {
            return Ok(false);
        }
        let class = self.class_list[index].clone();
        match args.values.len() {
            0 => {
                *value = Value::Class(class);
                Ok(true)
            }
            1 => {
                *value = args.values[0].clone();
                Ok(true)
            }
            _ => Err(ValueError::interp(IE_TOO_MANY_PARAMS)),
        }
    }

    /// `GetFun` (`JvInterpreter.pas:4330`).
    fn get_fun(&self, identifier: &[u8], value: &mut Value, args: &mut Args) -> Result<bool, ValueError> {
        let (found, index) = list_find(&self.function_list, |m| m.identifier.as_bytes(), identifier, false);
        if !found {
            return Ok(false);
        }
        let method = &self.function_list[index];
        if !cmp(method.identifier.as_bytes(), identifier) {
            return Ok(false);
        }
        args.identifier = String::from_utf8_lossy(identifier).into_owned();
        self.check_args(args, method.param_count, &method.param_types)?;
        let func = method.func.clone().expect("registered function");
        func(value, args)?;
        Ok(true)
    }

    /// `GetExtFun` (`JvInterpreter.pas:4349`).
    fn get_ext_fun(&self, identifier: &[u8], value: &mut Value, args: &mut Args) -> Result<bool, ValueError> {
        if self.disable_external_functions {
            return Ok(false);
        }
        for function in &self.ext_function_list {
            if cmp(function.identifier.as_bytes(), identifier) {
                args.identifier = String::from_utf8_lossy(identifier).into_owned();
                self.check_args(args, function.param_count, &function.param_types)?;
                let Some(call) = &self.external_call else {
                    return Err(ValueError::not_implemented("Calling external DLL functions"));
                };
                let call = call.clone();
                *value = call(function, args)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `TypeCast` (`JvInterpreter.pas:4433`): an identifier that is a type
    /// name casts its argument. Upstream retags the value without
    /// converting (`TVarData(Value).VType := VT`); the port converts
    /// through [`values::var_as_type`], because a retag of a different kind
    /// is a reinterpreted payload upstream (garbage for `Double(2)` and
    /// friends, which no script relies on).
    fn type_cast(&self, identifier: &[u8], value: &mut Value, args: &mut Args) -> bool {
        let typ = values::type_name_2_var_type(identifier);
        if typ == VarType::Empty {
            return false;
        }
        let Some(first) = args.values.first().cloned() else {
            // Upstream retags the stale `Values[0]`; a bare type name with
            // no argument is not a value.
            return false;
        };
        *value = values::var_as_type(&first, typ).unwrap_or(first);
        true
    }

    /// `GetSrcUnit` (`JvInterpreter.pas:4381`).
    fn get_src_unit(&self, identifier: &[u8], value: &mut Value, args: &mut Args) -> Result<bool, ValueError> {
        for unit in &self.src_unit_list {
            if cmp(unit.identifier.as_bytes(), identifier) {
                self.check_args(args, 0, &[])?;
                *value = Value::Object(Rc::new(RefCell::new(SrcUnitObject {
                    name: unit.identifier.clone(),
                    class: self.src_unit_class.clone().expect("built in"),
                })));
                return Ok(true);
            }
        }
        Ok(false)
    }

    //=== SetValue ============================================================

    /// `TJvInterpreterAdapter.SetValue` (`JvInterpreter.pas:4573`).
    pub fn set_value(&mut self, identifier: &[u8], value: &Value, args: &mut Args) -> Result<bool, ValueError> {
        self.ensure_sorted();

        if args.indexed {
            if matches!(args.obj_type(), VarType::Object | VarType::Class)
                && self.iset_method(identifier, value, args)?
            {
                return Ok(true);
            }
        } else if let Some(obj) = args.obj.clone() {
            match obj.var_type() {
                VarType::Object | VarType::Class => {
                    if self.set_method(identifier, value, args)? {
                        return Ok(true);
                    }
                }
                VarType::Record if self.set_record_method(&obj, identifier, value, args)? => {
                    return Ok(true);
                }
                _ => {}
            }
        }

        if self.src_var_list.set_value(identifier, value, args)? {
            return Ok(true);
        }
        for handler in &self.on_set_list {
            let handler = handler.clone();
            if handler(&String::from_utf8_lossy(identifier), value, args)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `SetMethod` (`JvInterpreter.pas:4589`): a *linear* scan (not the
    /// sorted Find), as upstream iterates the whole `FSetList`.
    fn set_method(&self, identifier: &[u8], value: &Value, args: &mut Args) -> Result<bool, ValueError> {
        for method in &self.set_list {
            if method.func.is_none()
                || !cmp(method.identifier.as_bytes(), identifier)
                || !class_matches(args, method.class_type.as_ref())
            {
                continue;
            }
            args.identifier = String::from_utf8_lossy(identifier).into_owned();
            self.check_args(args, method.param_count, &method.param_types)?;
            let func = method.func.clone().expect("checked");
            func(value, args)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// `ISetMethod` (`JvInterpreter.pas:4606`): the sorted Find of
    /// `FISetList`; a hit marks `ReturnIndexed`.
    fn iset_method(&self, identifier: &[u8], value: &Value, args: &mut Args) -> Result<bool, ValueError> {
        let (found, index) = list_find(&self.iset_list, |m| m.identifier.as_bytes(), identifier, true);
        if !found {
            return Ok(false);
        }
        for method in &self.iset_list[index..] {
            if !cmp(method.identifier.as_bytes(), identifier) {
                break;
            }
            if method.func.is_none() || !class_matches(args, method.class_type.as_ref()) {
                continue;
            }
            args.identifier = String::from_utf8_lossy(identifier).into_owned();
            self.check_args(args, method.param_count, &method.param_types)?;
            let func = method.func.clone().expect("checked");
            func(value, args)?;
            args.return_indexed = true;
            return Ok(true);
        }
        Ok(false)
    }

    /// `SetRecord` (`JvInterpreter.pas:4690`).
    fn set_record_method(
        &self,
        receiver: &Value,
        identifier: &[u8],
        value: &Value,
        args: &mut Args,
    ) -> Result<bool, ValueError> {
        let record = values::v2r(receiver)?;
        let def = record.borrow().def.clone();
        if let Some(index) = values::record_field_index(&def, identifier) {
            values::record_write_field(&mut record.borrow_mut(), index, value)?;
            return Ok(true);
        }
        for method in &self.record_set_list {
            if Rc::ptr_eq(&method.record, &def) && cmp(method.identifier.as_bytes(), identifier) {
                args.identifier = String::from_utf8_lossy(identifier).into_owned();
                self.check_args(args, method.param_count, &method.param_types)?;
                let func = method.func.clone();
                func(value, args)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    //=== GetElement / SetElement ============================================

    /// `TJvInterpreterAdapter.GetElement` (`JvInterpreter.pas:4822`): the
    /// default indexed property of an object value.
    pub fn get_element(&mut self, variable: &Value, value: &mut Value, args: &mut Args) -> Result<bool, ValueError> {
        let Some(object) = variable.as_object() else {
            return Ok(false);
        };
        let class = object.borrow().class();
        for method in &self.id_get_list {
            if method.func.is_none() || !method.class_type.as_ref().is_some_and(|entry| class.is_a(entry)) {
                continue;
            }
            args.obj = Some(variable.clone());
            self.check_args(args, method.param_count, &method.param_types)?;
            let func = method.func.clone().expect("checked");
            func(value, args)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// `TJvInterpreterAdapter.SetElement` (`JvInterpreter.pas:4892`).
    pub fn set_element(&mut self, variable: &Value, value: &Value, args: &mut Args) -> Result<bool, ValueError> {
        let Some(object) = variable.as_object() else {
            return Ok(false);
        };
        let class = object.borrow().class();
        for method in &self.id_set_list {
            if method.func.is_none() || !method.class_type.as_ref().is_some_and(|entry| class.is_a(entry)) {
                continue;
            }
            args.obj = Some(variable.clone());
            self.check_args(args, method.param_count, &method.param_types)?;
            let func = method.func.clone().expect("checked");
            func(value, args)?;
            return Ok(true);
        }
        Ok(false)
    }
}

/// The `(Args.Obj is FClassType)` / `(TClass(Args.Obj) = FClassType)` test
/// (`JvInterpreter.pas:3960`): an object receiver is checked against the
/// entry's class by class identity and inheritance, a class-value receiver
/// by equality.
fn class_matches(args: &Args, class: Option<&Rc<ClassDef>>) -> bool {
    let Some(class) = class else {
        return false;
    };
    match &args.obj {
        Some(Value::Object(object)) => object.borrow().class().is_a(class),
        Some(Value::Class(obj_class)) => Rc::ptr_eq(obj_class, class),
        _ => false,
    }
}

/// `Trim(SubStrBySeparator(UsesList, I, ','))` (`JvInterpreter.pas:3244`).
fn parse_uses_list(uses_list: &str) -> Vec<String> {
    uses_list
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(identifier: &str) -> Method {
        Method {
            identifier: identifier.to_owned(),
            unit_name: String::new(),
            class_type: None,
            func: None,
            param_count: 0,
            param_types: Vec::new(),
            res_typ: VarType::Empty,
        }
    }

    #[test]
    fn sorted_find_matches_case_insensitively() {
        let mut items = vec![
            entry("Zebra"),
            entry("apple"),
            entry("Banana"),
            entry("cherry"),
            entry("Apricot"),
        ];
        sort_identifier(&mut items, |m| m.identifier.as_bytes());
        let names: Vec<&str> = items.iter().map(|m| m.identifier.as_str()).collect();
        assert_eq!(names, vec!["apple", "Apricot", "Banana", "cherry", "Zebra"]);
        // A Find is case-insensitive (`AnsiStrIComp`).
        let (found, index) = list_find(&items, |m| m.identifier.as_bytes(), b"banana", false);
        assert!(found);
        assert_eq!(items[index].identifier, "Banana");
        let (found, index) = list_find(&items, |m| m.identifier.as_bytes(), b"APPLE", false);
        assert!(found);
        // `dupIgnore` lands on the probed entry; the leftmost of equals is
        // `dupAccept`'s.
        assert_eq!(items[index].identifier, "apple");
        let (found, index) = list_find(&items, |m| m.identifier.as_bytes(), b"apricot", true);
        assert!(found);
        assert_eq!(items[index].identifier, "Apricot");
        let (found, index) = list_find(&items, |m| m.identifier.as_bytes(), b"blueberry", false);
        assert!(!found);
        // `banana` < `blueberry` (`'a' < 'l'`), so the insertion point is
        // after `Banana`.
        assert_eq!(index, 3);
    }

    #[test]
    fn duplicate_names_scan_from_the_leftmost_entry() {
        let mut items = vec![entry("Find"), entry("find"), entry("Fetch")];
        sort_identifier(&mut items, |m| m.identifier.as_bytes());
        assert_eq!(items[0].identifier, "Fetch");
        let (found, index) = list_find(&items, |m| m.identifier.as_bytes(), b"FIND", true);
        assert!(found);
        assert_eq!(index, 1);
        // Both equal entries are adjacent, so the forward scan sees both.
        assert!(cmp(items[index].identifier.as_bytes(), b"find"));
        assert!(cmp(items[index + 1].identifier.as_bytes(), b"find"));
    }

    #[test]
    fn method_sort_puts_a_derived_class_first() {
        let base = Rc::new(ClassDef {
            unit: String::new(),
            name: "TBase".to_owned(),
            parent: None,
        });
        let derived = Rc::new(ClassDef {
            unit: String::new(),
            name: "TDerived".to_owned(),
            parent: Some(base.clone()),
        });
        let mut items = vec![
            Method {
                class_type: Some(base.clone()),
                ..entry("Run")
            },
            Method {
                class_type: Some(derived.clone()),
                ..entry("run")
            },
        ];
        sort_method(&mut items, |m| m.identifier.as_bytes(), |m| m.class_type.as_ref());
        // The entry of the derived class sorts before the base's, so a
        // derived receiver's forward scan finds it first (`:8637`).
        assert!(Rc::ptr_eq(items[0].class_type.as_ref().unwrap(), &derived));
        assert!(Rc::ptr_eq(items[1].class_type.as_ref().unwrap(), &base));
    }

    #[test]
    fn var_list_coerces_to_the_declared_type() {
        let mut list = VarList::new();
        list.add_var(
            "U",
            "count",
            "integer",
            ParamType::value(VarType::Integer),
            &Value::Empty,
            None,
        )
        .unwrap();
        let args = Args::new();
        // An integer variable takes an Int64 in range, an out-of-range one
        // is a variant error.
        assert!(list.set_value(b"Count", &Value::Int64(7), &args).unwrap());
        assert_eq!(list.value_of("", b"count"), Some(&Value::Integer(7)));
        assert_eq!(
            list.set_value(b"count", &Value::Int64(3_000_000_000), &args),
            Err(ValueError::Variant)
        );
        // A `varString` variable takes a number as text.
        list.add_var(
            "U",
            "name",
            "string",
            ParamType::value(VarType::Str),
            &Value::Empty,
            None,
        )
        .unwrap();
        assert!(list.set_value(b"name", &Value::Integer(12), &args).unwrap());
        assert_eq!(list.value_of("", b"name"), Some(&Value::Str(b"12".to_vec())));
        // A redeclaration of the same unit raises, other units may shadow.
        assert_eq!(
            list.add_var("U", "count", "", ParamType::default(), &Value::Empty, None),
            Err(ValueError::interp_n(IE_IDENTIFIER_REDECLARED, "count"))
        );
        assert!(
            list.add_var("V", "count", "", ParamType::default(), &Value::Empty, None)
                .is_ok()
        );
        // The newest entry answers a bare name (`Insert(0, ...)`).
        assert_eq!(list.value_of("", b"count"), Some(&Value::Empty));
    }
}
