// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter.pas (the value model
// of the interpreter: the extended variant types, the conversion and copy
// helpers of the implementation section, the array record, and the variant
// operators of TJvInterpreterExpression.Expression) and
// External/jvcl/jvcl/run/JvInterpreter_Types.pas and
// External/jvcl/jvcl/run/JvInterpreterConst.pas (the type tables the model
// keeps)

//! The `Variant` value model of the interpreter.
//!
//! Upstream a script value is a Delphi `Variant` with six extra variant
//! types registered by `TCustomVariantType` descendants
//! (`JvInterpreter.pas:1417-1445`): `varRecord` (a `TJvInterpreterRecHolder`
//! pointer), `varObject` (a `TObject`), `varClass` (a `TClass`),
//! `varPointer`, `varSet` (an `Integer` in the pointer slot, `S2V` at
//! `:1590`) and `varArray` (a `PJvInterpreterArrayRec`). This module models
//! the same values as [`Value`], the conversions of the implementation
//! section (`JvInterpreterVarAsType`, `Var2Type`, `JvInterpreterVarCopy`,
//! `JvInterpreterVarAssignment`, `V2O` ... `P2R`, `S2V`/`V2S`,
//! `TypeName2VarTyp`, `Typ2Size`) and the variant operators the expression
//! evaluator applies (`Expression`, `:5710-5836`), so the evaluator
//! (`crate::eval`) never touches Delphi semantics directly.
//!
//! What is *not* modelled: `varOleStr`/`varDispatch`/`varUnknown` (the OLE
//! automation and interface paths, compiled out without
//! `JvInterpreter_OLEAUTO` and unreachable without the VCL), `varSingle` and
//! `varCurrency` as distinct variants (a single-precision field read comes
//! back as [`Value::Double`] holding the single's value, `Single` fields are
//! written by rounding to `f32` first) and `varDate` keeps its own kind but
//! carries `f64` days as upstream does.
//!
//! Error model: a failed operation is a [`ValueError`], either a Delphi
//! `EVariantError` (the `Expression1` handler turns it into `ieTypeMistmatch`
//! at the position the parse reached, `:5883`) or a raised
//! `EJvInterpreterError` with its code and names (the evaluator adds the
//! position). The string comparisons of variant equality use the user
//! locale's case-sensitive collation upstream (`VarCmp`); this port compares
//! bytes, exact for the ASCII corpus, and the difference is owed to
//! `cargo xtask parity script`.

use std::any::Any;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use crate::ast::BinOp;
use crate::error::{
    IE_ARRAY_BAD_DIMENSION, IE_ARRAY_BAD_RANGE, IE_ARRAY_INDEX_OUT_OF_BOUNDS, IE_ROC_REQUIRED,
    RS_ARRAY_TO_ARRAY_ASSIGNMENT, RS_NOT_IMPLEMENTED,
};

/// A JvInterpreter error raised by a value operation: the code and the two
/// names `EJvInterpreterError` formats its message from
/// (`JvInterpreter.pas:1478`). The position is the evaluator's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueError {
    /// A Delphi `EVariantError`: the `Expression1` handler converts it to
    /// `ieTypeMistmatch` at `CurPos` (`JvInterpreter.pas:5883`).
    Variant,
    /// A non-interpreter exception with its own message (`EZeroDivide`,
    /// `ERangeError`, `EJVCLException`): reported through the `ieExternal`
    /// path (`UpdateExceptionPos`, `:5390`).
    External(String),
    /// A non-interpreter exception whose Delphi class the host knows, so the
    /// `Exception in unit ...: [<class>] <message>` line names it
    /// (`ERegularExpressionError` of the script-callable `TPerlRegEx`, the
    /// `EJsonCastException`/`EJsonParserException` of `JsonDataObjects`).
    /// [`ValueError::External`] leaves the class to the host's default of
    /// `Exception`.
    ExternalClass { class: &'static str, message: String },
    /// A raised `EJvInterpreterError`.
    Interp { code: i32, name1: String, name2: String },
}

impl ValueError {
    /// A host exception with a Delphi class name.
    pub fn external_class(class: &'static str, message: String) -> Self {
        ValueError::ExternalClass { class, message }
    }

    pub fn interp(code: i32) -> Self {
        ValueError::Interp {
            code,
            name1: String::new(),
            name2: String::new(),
        }
    }

    pub fn interp_n(code: i32, name1: impl Into<String>) -> Self {
        ValueError::Interp {
            code,
            name1: name1.into(),
            name2: String::new(),
        }
    }

    /// `NotImplemented` (`JvInterpreter.pas:1620`): the message is
    /// `Msg + RsENotImplemented` with no separating space, raised as
    /// `ieInternal` with the name carrying the whole text.
    pub fn not_implemented(message: &str) -> Self {
        ValueError::Interp {
            code: crate::error::IE_INTERNAL,
            name1: format!("{message}{RS_NOT_IMPLEMENTED}"),
            name2: String::new(),
        }
    }

    /// `JvInterpreterError(ieROCRequired, -1)` for a value that is no record
    /// holder (`V2R`, `:1575`).
    pub fn roc_required() -> Self {
        ValueError::interp(IE_ROC_REQUIRED)
    }
}

/// The type of a value or a declared variable (`TVarType`). The custom
/// variant types of `JvInterpreter.pas` (`varRecord` and the five others,
/// `:1417-1445`) become their own kinds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VarType {
    /// `varEmpty` -- unassigned.
    #[default]
    Empty,
    /// `varNull`.
    Null,
    /// `varSmallint`.
    Smallint,
    /// `varInteger`.
    Integer,
    /// `varByte`.
    Byte,
    /// `varInt64` -- an integer literal that does not fit an `Integer`
    /// (`ParseToken`, `:5561-5570`).
    Int64,
    /// `varDouble`.
    Double,
    /// `varDate` (`TDateTime`).
    Date,
    /// `varString` (also `AnsiString`, `char`, `PChar` in
    /// `TypeName2VarTyp`).
    Str,
    /// `varBoolean`.
    Bool,
    /// `varVariant` -- the declared type of an untyped variable: no coercion
    /// on assignment.
    Variant,
    /// `varPointer`.
    Pointer,
    /// `varObject`.
    Object,
    /// `varClass`.
    Class,
    /// `varSet` -- an `Integer` (or `Word`) of set bits.
    Set,
    /// `varRecord`.
    Record,
    /// `varArray` (the interpreter's own arrays).
    Array,
}

/// One parameter type: a [`VarType`] with the mode flags `ReadFunctionHeader`
/// ORs into the type word (`FParamTypes[iBeg] or varByRef`, `:7885`) and
/// `CheckArgs` tests (`ParamTypes[I] and varByRef`, `:3906`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ParamType {
    pub typ: VarType,
    pub by_ref: bool,
    pub by_const: bool,
}

impl ParamType {
    pub fn value(typ: VarType) -> Self {
        ParamType {
            typ,
            by_ref: false,
            by_const: false,
        }
    }

    pub fn by_ref(typ: VarType) -> Self {
        ParamType {
            typ,
            by_ref: true,
            by_const: false,
        }
    }

    pub fn by_const(typ: VarType) -> Self {
        ParamType {
            typ,
            by_ref: false,
            by_const: true,
        }
    }

    /// `GetDefine`'s prefix (`:2772`).
    pub fn mode_prefix(&self) -> &'static str {
        if self.by_ref {
            "Var "
        } else if self.by_const {
            "Const "
        } else {
            ""
        }
    }
}

/// `TypeName2VarTyp` (`JvInterpreter.pas:1650`): the variant type a Delphi
/// type name maps to; every unknown name is `varEmpty` (0), which callers
/// test against (`TypeCast`'s `Result := VT <> varEmpty`, `:4438`).
#[allow(clippy::if_same_then_else)] // the arms keep the per-name order of the upstream case (`:1657`)
pub fn type_name_2_var_type(name: &[u8]) -> VarType {
    if name.eq_ignore_ascii_case(b"AnsiString") {
        VarType::Str
    } else if name.eq_ignore_ascii_case(b"boolean") || name.eq_ignore_ascii_case(b"bool") {
        VarType::Bool
    } else if name.eq_ignore_ascii_case(b"byte") {
        VarType::Byte
    } else if name.eq_ignore_ascii_case(b"char") {
        VarType::Str
    } else if name.eq_ignore_ascii_case(b"dword") {
        VarType::Integer
    } else if name.eq_ignore_ascii_case(b"double") {
        VarType::Double
    } else if name.eq_ignore_ascii_case(b"integer") {
        VarType::Integer
    } else if name.eq_ignore_ascii_case(b"longint") {
        VarType::Integer
    } else if name.eq_ignore_ascii_case(b"longbool") {
        VarType::Bool
    } else if name.eq_ignore_ascii_case(b"PChar") {
        VarType::Str
    } else if name.eq_ignore_ascii_case(b"string") || name.eq_ignore_ascii_case(b"ShortString") {
        VarType::Str
    } else if name.eq_ignore_ascii_case(b"smallint") {
        VarType::Smallint
    } else if name.eq_ignore_ascii_case(b"TObject") {
        VarType::Object
    } else if name.eq_ignore_ascii_case(b"tdatetime") {
        VarType::Date
    } else if name.eq_ignore_ascii_case(b"word") {
        VarType::Smallint
    } else if name.eq_ignore_ascii_case(b"wordbool") {
        VarType::Bool
    } else {
        VarType::Empty
    }
}

/// `Typ2Size` (`JvInterpreter.pas:1628`): the byte size of a variant type;
/// 0 for the types the record-holder code never sizes. `TVarData` is 16, the
/// value `AddField` steps a record field by (`:8661`), and every object
/// reference is a pointer.
pub fn typ_2_size(typ: VarType) -> usize {
    match typ {
        VarType::Integer => 4,
        VarType::Double => 8,
        VarType::Byte => 1,
        VarType::Smallint => 2,
        VarType::Date => 8,
        VarType::Empty | VarType::Variant | VarType::Object | VarType::Class | VarType::Pointer | VarType::Null => 8,
        VarType::Str | VarType::Bool | VarType::Set | VarType::Record | VarType::Array | VarType::Int64 => 8,
    }
}

/// A classic-type object of a script: what `varObject` carries. The class is
/// the `TClass` `AddClass` registered (`is` checks run against it), and
/// adapters downcast [`as_any_mut`](ScriptObject::as_any_mut) to their own
/// types.
pub trait ScriptObject {
    fn class(&self) -> Rc<ClassDef>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// `E.Message` of an exception object, for the `ieExternal` error path
    /// (`UpdateExceptionPos`, `JvInterpreter.pas:5391` reads `E.Message`).
    fn message(&self) -> Option<&str> {
        None
    }
    /// The event property `name` as the object stores it: the script routine
    /// assigned to it. Upstream this is the `tkMethod` property of the real
    /// object (`GetPropInfo(FCurrArgs.Obj.ClassInfo, Identifier)`, a
    /// `TMethod` thunk over `NewEvent`); only the headless VCL object model
    /// (`script_vcl::interactive`) answers.
    fn event_function(&self, _name: &str) -> Option<String> {
        None
    }
    /// Store the routine `function` into the event property `name`; `None`
    /// clears it (`X.OnClick := nil`). `true` when this object has such a
    /// slot (`SetMethodProp(FCurrArgs.Obj, PropInf, Method)`,
    /// `JvInterpreter.pas:6932`; the `nil` arm is `:6936-6948`).
    fn set_event(&mut self, _name: &str, _function: Option<&str>) -> bool {
        false
    }
    /// `E.ClassName` of an exception object whose Delphi class is not a
    /// registered script class: the handler's `E` is the real exception
    /// upstream, so `E.ClassName` is `EVariantTypeCastError` for the error
    /// of a typed assignment (probe `x-raise-04`) although `on E:
    /// EVariantTypeCastError` is an unknown class name to the interpreter.
    fn exception_class_name(&self) -> Option<&str> {
        None
    }
    /// The identity two values of this object compare by when they are not
    /// the same wrapper: an `IInterface` variant compares by the interface
    /// pointer upstream, and the host hands out a fresh wrapper for every
    /// element value, so an element wrapper answers its element's id
    /// (`sl.IndexOfObject(base)` finds the element added earlier). `None`
    /// keeps the wrapper identity (`Rc::ptr_eq`).
    fn identity(&self) -> Option<u64> {
        None
    }
    /// `Args.Obj.Free` (`JvInterpreter.pas:3976`). A Rust object cannot be
    /// freed through a shared handle; this is the adapter's signal to mark
    /// itself released. Upstream frees the Delphi object here.
    fn free(&mut self) {}
    /// Whether the value is an `IInterface` (`varUnknown`) rather than a
    /// `varObject`: the element wrappers of a run. `VarType` answers 13 for
    /// them, `=` refuses them and `<>` compares them raw.
    fn is_interface(&self) -> bool {
        false
    }
    /// The object as its `TStrings` lines, when it is one
    /// ([`crate::strings::StringList`]): what a host function that takes a
    /// `TStrings` parameter reads, whatever class registered the value.
    fn string_list(&self) -> Option<&dyn crate::strings::StringList> {
        None
    }
    /// The object as its `TStrings` lines, for the functions that fill one.
    fn string_list_mut(&mut self) -> Option<&mut dyn crate::strings::StringList> {
        None
    }
    /// The object as its `TList` entries, when it is one
    /// ([`crate::lists::List`]): what a host function that fills a `TList`
    /// appends to.
    fn list(&self) -> Option<&dyn crate::lists::List> {
        None
    }
    /// The object as its `TList` entries, for the functions that fill one.
    fn list_mut(&mut self) -> Option<&mut dyn crate::lists::List> {
        None
    }
}

/// A class as `AddClass` registers it (`FClassList`): the name, the unit and
/// the parent chain `is` checks walk. Delphi resolves the chain from RTTI;
/// this port takes the parent at registration.
#[derive(Debug)]
pub struct ClassDef {
    pub unit: String,
    pub name: String,
    pub parent: Option<Rc<ClassDef>>,
}

impl ClassDef {
    /// `Args.Obj is FClassType` / `TClass(Args.Obj) = FClassType`
    /// (`JvInterpreter.pas:3960-3963`): the identity check walks the chain
    /// (Delphi's `TObject.InheritsFrom` returns true for the class itself).
    pub fn is_a(&self, class: &Rc<ClassDef>) -> bool {
        let mut current: Option<&ClassDef> = Some(self);
        while let Some(c) = current {
            if std::ptr::eq(c, class.as_ref()) {
                return true;
            }
            current = c.parent.as_deref();
        }
        false
    }
}

/// A record type (`TJvInterpreterRecord`): the fields in declaration order
/// with the byte offset and variant type `AddRec`'s `RFD` gives
/// (`JvInterpreter.pas:3615`) or `AddField` grows for a script record
/// (`:8653`: every field a `TVarData`, 16 bytes). The port keeps the values
/// per field instead of a byte buffer, keyed by index.
#[derive(Clone, Debug)]
pub struct RecordDef {
    pub unit: String,
    pub name: String,
    /// `SizeOf(Rec^)` for an adapter record, `FieldCount * SizeOf(TVarData)`
    /// for a script record.
    pub size: i64,
    pub fields: Vec<RecordField>,
}

#[derive(Clone, Debug)]
pub struct RecordField {
    pub name: String,
    /// The `RFD` offset (metadata; the port addresses fields by index) or the
    /// running size a script record's `AddField` assigns.
    pub offset: i64,
    /// The field's variant type: `varEmpty` for every script-record field and
    /// for adapter fields declared `varEmpty` (a full variant slot), a typed
    /// slot otherwise (`AddRec`'s `RFD` `Typ`).
    pub typ: VarType,
    /// The initializer a script record's field carries (`InterpretVar`'s
    /// `DT.Init`, `:7398`).
    pub data_type: Option<DataType>,
}

/// A record instance (`TJvInterpreterRecHolder`'s `Rec` data): the values of
/// the fields of its [`RecordDef`], in field order.
#[derive(Debug)]
pub struct RecordInstance {
    pub def: Rc<RecordDef>,
    pub values: Vec<Value>,
}

impl RecordInstance {
    /// `NewRecord` (`JvInterpreter.pas:8665`): a `varString` field is `''`, a
    /// `varEmpty` field starts `Null` and runs its `DataType.Init`, every
    /// other typed field zero (upstream reads uninitialized memory for them;
    /// the port zeroes).
    pub fn new(def: Rc<RecordDef>) -> RecordInstance {
        let values = def
            .fields
            .iter()
            .map(|field| match field.typ {
                VarType::Str => Value::Str(Vec::new()),
                VarType::Empty => match &field.data_type {
                    Some(data_type) => data_type.init(),
                    None => Value::Null,
                },
                typ => zero_value(typ),
            })
            .collect();
        RecordInstance { def, values }
    }
}

/// `GetRecord`'s field lookup (`JvInterpreter.pas:4232`): the fields are
/// scanned in order with `Cmp` (case-insensitive).
pub fn record_field_index(def: &RecordDef, name: &[u8]) -> Option<usize> {
    def.fields
        .iter()
        .position(|field| field.name.as_bytes().eq_ignore_ascii_case(name))
}

/// `GetRecord` (`JvInterpreter.pas:4223`): a field read, by the field's
/// declared variant type. `varSingle` and `varOleStr` keep their slot shape:
/// a single-precision value comes back as a double (the port has no
/// `varSingle`), a `WideString` as the UTF-8 bytes of its text.
pub fn record_read_field(instance: &RecordInstance, index: usize) -> Value {
    instance.values[index].clone()
}

/// `SetRecord` (`JvInterpreter.pas:4690`): a field write with the field
/// type's implicit conversion (`PInteger(Rec + Offset)^ := Value` and the
/// other assignments).
pub fn record_write_field(instance: &mut RecordInstance, index: usize, value: &Value) -> Result<(), ValueError> {
    let field = &instance.def.fields[index];
    let converted = match field.typ {
        VarType::Empty => value.clone(),
        VarType::Smallint => Value::Smallint(to_i16(value)?),
        VarType::Integer => Value::Integer(to_i32(value)?),
        VarType::Byte => Value::Byte(to_u8(value)?),
        VarType::Int64 => Value::Int64(to_i64(value)?),
        VarType::Double | VarType::Date => Value::Double(to_f64(value)?),
        VarType::Str => Value::Str(to_str(value)?),
        VarType::Bool => Value::Bool(to_bool(value)?),
        VarType::Set => Value::Set(to_i32(value)?),
        VarType::Object | VarType::Class | VarType::Pointer => value.clone(),
        VarType::Null => Value::Null,
        VarType::Variant => value.clone(),
        VarType::Record | VarType::Array => return Err(ValueError::Variant),
    };
    instance.values[index] = converted;
    Ok(())
}

/// The default value of a typed slot: what a zero-filled `TVarData` (or a
/// zeroed `PInteger`) reads as.
fn zero_value(typ: VarType) -> Value {
    match typ {
        VarType::Smallint => Value::Smallint(0),
        VarType::Integer => Value::Integer(0),
        VarType::Byte => Value::Byte(0),
        VarType::Int64 => Value::Int64(0),
        VarType::Double | VarType::Date => Value::Double(0.0),
        VarType::Str => Value::Str(Vec::new()),
        VarType::Bool => Value::Bool(false),
        VarType::Set => Value::Set(0),
        VarType::Object | VarType::Class | VarType::Null => Value::Null,
        VarType::Pointer => Value::Pointer(0),
        VarType::Empty | VarType::Variant | VarType::Record | VarType::Array => Value::Null,
    }
}

/// `IJvInterpreterDataType` (`JvInterpreter.pas:404`): the initializer of a
/// declared type. `InterpretVar` calls `DT.Init(Value)` once per declared
/// variable (`:7398`), a function's `Result` starts from
/// `ResDataType.Init` (`:7476`) and so does a script record's field
/// (`AddField`, `:8653`).
#[derive(Clone, Debug)]
pub enum DataType {
    /// `TJvInterpreterSimpleDataType` (`:8739`).
    Simple(VarType),
    /// `TJvInterpreterRecordDataType` (`:8697`): `Init` is `NewRecord`.
    Record(Rc<RecordDef>),
    /// `TJvInterpreterArrayDataType` (`:8715`): `Init` is
    /// `JvInterpreterArrayInit`.
    Array {
        ranges: Vec<(i64, i64)>,
        item_type: VarType,
        element: Box<DataType>,
    },
}

impl DataType {
    /// `GetTyp` (`JvInterpreter.pas:8705`, `:8728`, `:8747`).
    pub fn get_typ(&self) -> VarType {
        match self {
            DataType::Simple(typ) => *typ,
            DataType::Record(_) => VarType::Empty,
            DataType::Array { .. } => VarType::Array,
        }
    }

    /// `Init`: `V := Null; TVarData(V).VType := varEmpty; if (FTyp <> 0) and
    /// (FTyp <> varObject) then V := Var2Type(V, FTyp)` (`:8752`) -- the
    /// port's `var_2_type` from `Empty` (a `varObject` variable stays
    /// unassigned, as upstream leaves it).
    pub fn init(&self) -> Value {
        match self {
            DataType::Simple(VarType::Empty) | DataType::Simple(VarType::Object) => Value::Empty,
            DataType::Simple(typ) => var_2_type(&Value::Empty, *typ).unwrap_or(Value::Empty),
            DataType::Record(def) => Value::Record(Rc::new(RefCell::new(RecordInstance::new(def.clone())))),
            DataType::Array {
                ranges,
                item_type,
                element,
            } => match array_init(ranges, *item_type, Some(element)) {
                Ok(array) => Value::Array(Rc::new(RefCell::new(array))),
                Err(_) => Value::Empty,
            },
        }
    }
}

/// An interpreter array (`PJvInterpreterArrayRec`, `JvInterpreter.pas:506`):
/// up to `JvInterpreter_MAX_ARRAY_DIMENSION` (10) dimensions with inclusive
/// bounds, an item type and one value slot per element (the port's stand-in
/// for the record's memory block; `varString` elements keep upstream's one
/// string per slot).
#[derive(Debug)]
pub struct ArrayInstance {
    pub begin: Vec<i64>,
    pub end: Vec<i64>,
    pub item_type: VarType,
    pub values: Vec<Value>,
}

/// `GetArraySize` (`JvInterpreter.pas:2073`).
pub fn array_size(begin: &[i64], end: &[i64]) -> usize {
    begin
        .iter()
        .zip(end)
        .map(|(b, e)| (*e - *b + 1).max(0) as usize)
        .product()
}

/// `GetArrayOffset` (`JvInterpreter.pas:2086`): the row-major element offset,
/// bounds-checked per index with `ieArrayIndexOutOfBounds`.
pub fn array_offset(begin: &[i64], end: &[i64], element: &[i64]) -> Result<usize, ValueError> {
    let mut result: i64 = 0;
    let mut last_dim: i64 = 1;
    for a in 0..begin.len() {
        let index = element.get(a).copied().unwrap_or(0);
        if index < begin[a] || index > end[a] {
            return Err(ValueError::interp(IE_ARRAY_INDEX_OUT_OF_BOUNDS));
        }
        result += last_dim * (index - begin[a]);
        last_dim *= end[a] - begin[a] + 1;
    }
    Ok(result as usize)
}

/// `JvInterpreterArrayInit` (`JvInterpreter.pas:2105`): the dimension and
/// range checks, `varEmpty` elements through the element `DataType.Init`,
/// typed elements zero-filled (upstream zero-fills too).
pub fn array_init(
    ranges: &[(i64, i64)],
    item_type: VarType,
    element: Option<&DataType>,
) -> Result<ArrayInstance, ValueError> {
    let dimension = ranges.len();
    if !(1..=32).contains(&dimension) {
        return Err(ValueError::interp(IE_ARRAY_BAD_DIMENSION));
    }
    for (b, e) in ranges {
        if !(b <= e) && !(dimension == 1 && *b == 0 && *e == -1) {
            return Err(ValueError::interp(IE_ARRAY_BAD_RANGE));
        }
    }
    let begin: Vec<i64> = ranges.iter().map(|(b, _)| *b).collect();
    let end: Vec<i64> = ranges.iter().map(|(_, e)| *e).collect();
    let values = (0..array_size(&begin, &end))
        .map(|_| match item_type {
            VarType::Empty => match element {
                Some(data_type) => data_type.init(),
                None => Value::Null,
            },
            // UPSTREAM-QUIRK: `JvInterpreterArrayGetElement` reads only
            // `varInteger`, `varDouble`, `varByte`, `varSmallint`, `varDate`,
            // `varString`, `varEmpty` and `varObject` elements (`:2240`),
            // and `JvInterpreterArraySetElement` writes the same set, so an
            // `array of Boolean` (or of `Int64`, a set, ...) reads
            // `Unassigned` whatever was stored (probe `a-17`).
            typ if !array_element_kind_is_handled(typ) => Value::Empty,
            typ => zero_value(typ),
        })
        .collect();
    Ok(ArrayInstance {
        begin,
        end,
        item_type,
        values,
    })
}

/// The item types `JvInterpreterArrayGetElement`/`SetElement` handle
/// (`JvInterpreter.pas:2240`, `:2201`); `Variant` is the port's spelling of
/// the `varEmpty` item of `array of Variant`.
fn array_element_kind_is_handled(typ: VarType) -> bool {
    matches!(
        typ,
        VarType::Integer
            | VarType::Double
            | VarType::Byte
            | VarType::Smallint
            | VarType::Date
            | VarType::Str
            | VarType::Empty
            | VarType::Variant
            | VarType::Object
    )
}

/// `JvInterpreterArrayGetElement` (`JvInterpreter.pas:2226`): the element
/// read by the item type.
pub fn array_get(array: &ArrayInstance, offset: usize) -> Value {
    array.values[offset].clone()
}

/// `JvInterpreterArraySetElement` (`JvInterpreter.pas:2188`): the element
/// write with the item type's implicit conversion
/// (`PInteger(P)^ := Value`, `PByte(P)^ := Value`, `varString` through
/// `VarAsType(Value, varString)` into the string list, `varEmpty` through
/// `JvInterpreterVarAssignment`, `varObject` through `V2O`).
pub fn array_set(array: &mut ArrayInstance, offset: usize, value: &Value) -> Result<(), ValueError> {
    if !array_element_kind_is_handled(array.item_type) {
        // UPSTREAM-QUIRK: the write of an unhandled item type is the empty
        // `else` of `JvInterpreterArraySetElement` (`:2213`): nothing.
        return Ok(());
    }
    let converted = match array.item_type {
        VarType::Integer => Value::Integer(to_i32(value)?),
        VarType::Double | VarType::Date => Value::Double(to_f64(value)?),
        VarType::Byte => Value::Byte(to_u8(value)?),
        VarType::Smallint => Value::Smallint(to_i16(value)?),
        VarType::Str => Value::Str(var_as_type(value, VarType::Str)?.into_str()),
        VarType::Empty => {
            let mut dest = array.values[offset].clone();
            jv_var_assignment(&mut dest, value)?;
            dest
        }
        VarType::Object => match value {
            Value::Object(_) => value.clone(),
            Value::Null | Value::Pointer(0) => Value::Null,
            _ => return Err(ValueError::Variant),
        },
        VarType::Int64 => Value::Int64(to_i64(value)?),
        VarType::Bool => Value::Bool(to_bool(value)?),
        VarType::Set => Value::Set(to_i32(value)?),
        VarType::Variant => value.clone(),
        VarType::Class | VarType::Pointer | VarType::Record | VarType::Array | VarType::Null => {
            return Err(ValueError::Variant);
        }
    };
    array.values[offset] = converted;
    Ok(())
}

/// A native variant array: what `ReadArgs.ReadOpenArray`
/// (`JvInterpreter.pas:5967`) builds with `VarArrayCreate([0, I - 1],
/// varVariant)`, the shape an argument like `[a, b]` arrives in.
#[derive(Debug)]
pub struct VariantArray {
    pub low: i64,
    pub values: Vec<Value>,
    /// The element type `VarArrayCreate` was given: `varVariant` (12) for
    /// an open array, `varOleStr` (8) for a `TArray<string>` result such as
    /// `TDirectory.GetFiles` (`VarType` is `varArray or` this, probe
    /// `r-04`).
    pub element_code: i32,
}

impl VariantArray {
    pub fn new(values: Vec<Value>) -> Self {
        VariantArray {
            low: 0,
            values,
            element_code: 12,
        }
    }

    pub fn of_strings(values: Vec<Value>) -> Self {
        VariantArray {
            low: 0,
            values,
            element_code: 8,
        }
    }
}

/// A script value.
#[derive(Clone)]
pub enum Value {
    /// `varEmpty` -- `Unassigned`.
    Empty,
    /// `varNull` -- `Null`.
    Null,
    Smallint(i16),
    Integer(i32),
    Byte(u8),
    /// `varWord`: what `VarType` answers with (a `TVarType` is a `Word`,
    /// so `VarType(VarType(x))` is 18, probe `a-17`); no declared type maps
    /// to it (`word` is `varSmallint` to `TypeName2VarTyp`).
    Word(u16),
    /// `varInt64` -- what an integer literal above `High(Integer)` becomes
    /// (`ParseToken`, `JvInterpreter.pas:5565`).
    Int64(i64),
    Double(f64),
    Date(f64),
    Str(Vec<u8>),
    Bool(bool),
    /// `varSet` (`S2V`, `:1590`).
    Set(i32),
    /// `varPointer`.
    Pointer(usize),
    /// `varObject`, an adapter object.
    Object(Rc<RefCell<dyn ScriptObject>>),
    /// `varClass`, a registered class (`C2V`, `:1530`).
    Class(Rc<ClassDef>),
    /// `varRecord`, a record holder (`R2V`, `:1569`).
    Record(Rc<RefCell<RecordInstance>>),
    /// `varArray`, an interpreter array (`JvInterpreterArrayInit`).
    Array(Rc<RefCell<ArrayInstance>>),
    /// The `VarArrayCreate` result of an open-array argument.
    VariantArray(Rc<RefCell<VariantArray>>),
}

impl Value {
    /// The value's variant type (`TVarData.VType`).
    pub fn var_type(&self) -> VarType {
        match self {
            Value::Empty => VarType::Empty,
            Value::Null => VarType::Null,
            Value::Smallint(_) => VarType::Smallint,
            Value::Integer(_) => VarType::Integer,
            Value::Byte(_) => VarType::Byte,
            // No declared type is a `Word`; the nearest for the typed-slot
            // rules is the 16-bit `Smallint`.
            Value::Word(_) => VarType::Smallint,
            Value::Int64(_) => VarType::Int64,
            Value::Double(_) => VarType::Double,
            Value::Date(_) => VarType::Date,
            Value::Str(_) => VarType::Str,
            Value::Bool(_) => VarType::Bool,
            Value::Set(_) => VarType::Set,
            Value::Pointer(_) => VarType::Pointer,
            Value::Object(_) => VarType::Object,
            Value::Class(_) => VarType::Class,
            Value::Record(_) => VarType::Record,
            Value::Array(_) => VarType::Array,
            Value::VariantArray(_) => VarType::Array,
        }
    }

    /// `VarIsStr`/`VarIsEmpty`-style predicates adapters use.
    pub fn is_str(&self) -> bool {
        matches!(self, Value::Str(_))
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Value::Empty)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// `VarIsArray`: both the interpreter's arrays and native variant arrays.
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_) | Value::VariantArray(_))
    }

    /// `VarIsOrdinal` (`SetExpression1` tests it, `JvInterpreter.pas:5943`).
    pub fn is_ordinal(&self) -> bool {
        matches!(
            self,
            Value::Smallint(_) | Value::Integer(_) | Value::Byte(_) | Value::Word(_) | Value::Int64(_) | Value::Bool(_)
        )
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    pub fn is_record(&self) -> bool {
        matches!(self, Value::Record(_))
    }

    /// `varString` as bytes.
    pub fn as_str(&self) -> Option<&[u8]> {
        match self {
            Value::Str(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub fn into_str(self) -> Vec<u8> {
        match self {
            Value::Str(bytes) => bytes,
            _ => Vec::new(),
        }
    }

    pub fn as_object(&self) -> Option<&Rc<RefCell<dyn ScriptObject>>> {
        match self {
            Value::Object(object) => Some(object),
            _ => None,
        }
    }

    pub fn as_class(&self) -> Option<&Rc<ClassDef>> {
        match self {
            Value::Class(class) => Some(class),
            _ => None,
        }
    }

    pub fn as_record(&self) -> Option<&Rc<RefCell<RecordInstance>>> {
        match self {
            Value::Record(record) => Some(record),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Rc<RefCell<ArrayInstance>>> {
        match self {
            Value::Array(array) => Some(array),
            _ => None,
        }
    }

    /// The raw pointer-sized payload of the pointer-like kinds and the
    /// numeric payload of the integer kinds: what the `ttEqu`/`ttNotEqu`
    /// special cases read out of `TVarData(...).VInteger`
    /// (`JvInterpreter.pas:5794`, `:5807`).
    pub fn raw(&self) -> Option<i64> {
        match self {
            Value::Smallint(v) => Some(*v as i64),
            Value::Integer(v) => Some(*v as i64),
            Value::Byte(v) => Some(*v as i64),
            Value::Word(v) => Some(*v as i64),
            Value::Int64(v) => Some(*v),
            Value::Bool(v) => Some(*v as i64),
            Value::Set(v) => Some(*v as i64),
            Value::Pointer(v) => Some(*v as i64),
            Value::Object(o) => Some(Rc::as_ptr(o) as *const () as i64),
            Value::Class(c) => Some(Rc::as_ptr(c) as *const () as i64),
            Value::Record(r) => Some(Rc::as_ptr(r) as *const () as i64),
            Value::Array(a) => Some(Rc::as_ptr(a) as *const () as i64),
            Value::VariantArray(a) => Some(Rc::as_ptr(a) as *const () as i64),
            Value::Empty | Value::Null | Value::Double(_) | Value::Date(_) | Value::Str(_) => None,
        }
    }

    /// `P2V(nil)`/`nil` comparisons: whether the value is a nil pointer-like.
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Pointer(0) | Value::Null)
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Empty => write!(f, "Empty"),
            Value::Null => write!(f, "Null"),
            Value::Smallint(v) => write!(f, "Smallint({v})"),
            Value::Integer(v) => write!(f, "Integer({v})"),
            Value::Byte(v) => write!(f, "Byte({v})"),
            Value::Word(v) => write!(f, "Word({v})"),
            Value::Int64(v) => write!(f, "Int64({v})"),
            Value::Double(v) => write!(f, "Double({v:?})"),
            Value::Date(v) => write!(f, "Date({v:?})"),
            Value::Str(bytes) => write!(f, "Str({:?})", String::from_utf8_lossy(bytes)),
            Value::Bool(v) => write!(f, "Bool({v})"),
            Value::Set(v) => write!(f, "Set(${v:X})"),
            Value::Pointer(v) => write!(f, "Pointer({v:#X})"),
            Value::Object(o) => write!(f, "Object({})", o.borrow().class().name),
            Value::Class(c) => write!(f, "Class({})", c.name),
            Value::Record(r) => write!(f, "Record({})", r.borrow().def.name),
            Value::Array(_) => write!(f, "Array"),
            Value::VariantArray(_) => write!(f, "VariantArray"),
        }
    }
}

/// Value identity for tests and equality checks: the scalar kinds compare by
/// value, the reference kinds (`Object`, `Class`, `Record`, `Array`,
/// `VariantArray`) by handle.
/// Two object values are equal when they are one wrapper, or when both
/// wrappers answer the same [`ScriptObject::identity`].
pub fn same_object(a: &Rc<RefCell<dyn ScriptObject>>, b: &Rc<RefCell<dyn ScriptObject>>) -> bool {
    if Rc::ptr_eq(a, b) {
        return true;
    }
    match (a.try_borrow(), b.try_borrow()) {
        (Ok(a), Ok(b)) => matches!((a.identity(), b.identity()), (Some(x), Some(y)) if x == y),
        _ => false,
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Empty, Value::Empty) | (Value::Null, Value::Null) => true,
            (Value::Smallint(a), Value::Smallint(b)) => a == b,
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Byte(a), Value::Byte(b)) => a == b,
            (Value::Word(a), Value::Word(b)) => a == b,
            (Value::Int64(a), Value::Int64(b)) => a == b,
            (Value::Double(a), Value::Double(b)) | (Value::Date(a), Value::Date(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => a == b,
            (Value::Pointer(a), Value::Pointer(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => same_object(a, b),
            (Value::Class(a), Value::Class(b)) => Rc::ptr_eq(a, b),
            (Value::Record(a), Value::Record(b)) => Rc::ptr_eq(a, b),
            (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b),
            (Value::VariantArray(a), Value::VariantArray(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

//=== Conversions =============================================================

/// `JvInterpreterVarAsType` (`JvInterpreter.pas:2466`): the cast with the
/// interpreter's own rules -- `Empty` and `Null` take the target's default
/// (`''`, `0`, `False`, `0.0`, `Null`), an array keeps its data and only
/// changes type, a cast to `varEmpty` returns the value itself, everything
/// else is a `VarAsType`.
pub fn var_as_type(value: &Value, typ: VarType) -> Result<Value, ValueError> {
    if value.is_empty() || value.is_null() {
        return match typ {
            VarType::Str => Ok(Value::Str(Vec::new())),
            // `Result := 0` (`:2473`): a Delphi literal in a `Variant` takes
            // the smallest type that holds it, so the slot reads as a
            // `Byte` (`i := Null; VarType(i)` is 17, probe `v-slot-18`).
            VarType::Integer | VarType::Smallint | VarType::Byte | VarType::Int64 => Ok(Value::Byte(0)),
            VarType::Bool => Ok(Value::Bool(false)),
            VarType::Double | VarType::Date => Ok(Value::Double(0.0)),
            VarType::Variant => Ok(Value::Null),
            VarType::Set => Ok(Value::Set(0)),
            VarType::Pointer => Ok(Value::Pointer(0)),
            VarType::Empty => Ok(Value::Empty),
            VarType::Object | VarType::Class | VarType::Null => Ok(Value::Null),
            VarType::Record | VarType::Array => Err(ValueError::Variant),
        };
    }
    match typ {
        VarType::Array => {
            if value.is_array() {
                Ok(value.clone())
            } else if value.is_nil() {
                // `a := nil` stores the nil pointer; the next `a[0]` is
                // then `Array type required` (probe `a-28`).
                Ok(value.clone())
            } else {
                // `VarAsType` into a custom type (probe `a-29`).
                Err(ValueError::external_class(
                    "EVariantTypeCastError",
                    "Invalid variant type conversion".to_owned(),
                ))
            }
        }
        VarType::Empty => Ok(value.clone()),
        VarType::Smallint => Ok(Value::Smallint(to_i16(value)?)),
        VarType::Integer => Ok(Value::Integer(to_i32(value)?)),
        VarType::Byte => Ok(Value::Byte(to_u8(value)?)),
        VarType::Int64 => Ok(Value::Int64(to_i64(value)?)),
        VarType::Double => Ok(Value::Double(to_f64(value)?)),
        VarType::Date => Ok(Value::Date(to_f64(value)?)),
        VarType::Str => Ok(Value::Str(to_str(value)?)),
        VarType::Bool => Ok(Value::Bool(to_bool(value)?)),
        VarType::Set => Ok(Value::Set(to_i32(value)?)),
        VarType::Pointer => match value {
            Value::Pointer(_) => Ok(value.clone()),
            Value::Null | Value::Empty => Ok(Value::Pointer(0)),
            _ => Err(ValueError::Variant),
        },
        VarType::Variant => Ok(value.clone()),
        VarType::Object | VarType::Class | VarType::Record | VarType::Null => {
            if value.var_type() == typ {
                Ok(value.clone())
            } else {
                Err(ValueError::Variant)
            }
        }
    }
}

/// `TypeCast`'s retag (`JvInterpreter.pas:4433`, `TVarData(Value).VType :=
/// VT`): a double cast to an integer kind reads the low bits of its IEEE
/// payload (`Integer(1.5)` is 0), an integer cast to a double reads its
/// bits as a double, a nil pointer is 0, and the conversions that are
/// well-defined (a numeric string, a boolean, an integer into a wider or
/// narrower integer) convert; what would fail keeps the value, as a retag
/// never raises.
pub fn retag(value: &Value, typ: VarType) -> Value {
    match (value, typ) {
        (Value::Double(v) | Value::Date(v), VarType::Integer) => Value::Integer(v.to_bits() as u32 as i32),
        (Value::Double(v) | Value::Date(v), VarType::Smallint) => Value::Smallint(v.to_bits() as u16 as i16),
        (Value::Double(v) | Value::Date(v), VarType::Byte) => Value::Byte(v.to_bits() as u8),
        (Value::Double(v) | Value::Date(v), VarType::Int64) => Value::Int64(v.to_bits() as i64),
        (Value::Integer(v), VarType::Double) => Value::Double(f64::from_bits(*v as u32 as u64)),
        (Value::Smallint(v), VarType::Double) => Value::Double(f64::from_bits(*v as u16 as u64)),
        (Value::Byte(v), VarType::Double) => Value::Double(f64::from_bits(u64::from(*v))),
        (Value::Int64(v), VarType::Double) => Value::Double(f64::from_bits(*v as u64)),
        (Value::Pointer(0) | Value::Null | Value::Empty, VarType::Integer | VarType::Smallint | VarType::Byte) => {
            Value::Byte(0)
        }
        // A conversion that fails is a reinterpreted payload upstream
        // (`Integer('x')` is the low bits of a string pointer, `Double('x')`
        // those bits as a double): the port stands in the target's zero,
        // so the script continues as it does there (probes `x-048`,
        // `x-050`, `x-053`).
        _ => var_as_type(value, typ).unwrap_or_else(|_| match typ {
            VarType::Integer => Value::Integer(0),
            VarType::Smallint => Value::Smallint(0),
            VarType::Byte => Value::Byte(0),
            VarType::Int64 => Value::Int64(0),
            VarType::Double | VarType::Date => Value::Double(0.0),
            VarType::Bool => Value::Bool(false),
            VarType::Str => Value::Str(Vec::new()),
            _ => value.clone(),
        }),
    }
}

/// `Var2Type` (`JvInterpreter.pas:2564`): `JvInterpreterVarAsType` plus the
/// explicit `varBoolean`-to-`varInteger` rule (`Ord(V = True)`, `:2585`).
pub fn var_2_type(value: &Value, typ: VarType) -> Result<Value, ValueError> {
    let result = var_as_type(value, typ)?;
    if typ == VarType::Integer && matches!(value, Value::Bool(_)) {
        return Ok(Value::Integer(i32::from(matches!(value, Value::Bool(true)))));
    }
    Ok(result)
}

/// `JvInterpreterVarCopy` (`JvInterpreter.pas:2533`): an array or record
/// copies its handle (the `TVarData` wholesale), everything else the value.
pub fn jv_var_copy(dest: &mut Value, source: &Value) {
    *dest = source.clone();
}

/// `JvInterpreterVarAssignment` (`JvInterpreter.pas:2500`): the assignment
/// an interpreter variable receives. An array raises
/// `NotImplemented(RsArrayToArrayAssignment)`; a record copies field by
/// field into a record destination. Upstream reads the destination's record
/// holder without checking it and faults on a non-record destination
/// (`:2514`); the port assigns the value instead.
pub fn jv_var_assignment(dest: &mut Value, source: &Value) -> Result<(), ValueError> {
    match source {
        Value::Array(_) => Err(ValueError::not_implemented(RS_ARRAY_TO_ARRAY_ASSIGNMENT)),
        Value::Record(source_record) => {
            if let Value::Record(dest_record) = dest {
                // A `var` record parameter and its argument share the holder
                // (FindFirst's `var F: TSearchRec`); a self-assignment is a
                // no-op, and borrowing the same `RefCell` twice would panic.
                if Rc::ptr_eq(source_record, dest_record) {
                    return Ok(());
                }
                let source = source_record.borrow();
                let mut target = dest_record.borrow_mut();
                let count = source.values.len().min(target.values.len());
                for index in 0..count {
                    let mut value = target.values[index].clone();
                    jv_var_assignment(&mut value, &source.values[index])?;
                    target.values[index] = value;
                }
                Ok(())
            } else {
                *dest = source.clone();
                Ok(())
            }
        }
        _ => {
            *dest = source.clone();
            Ok(())
        }
    }
}

/// `V2R` (`JvInterpreter.pas:1575`) / `P2R` (`:1583`): the record holder of
/// a `varRecord` value; anything else raises `ieROCRequired`.
pub fn v2r(value: &Value) -> Result<Rc<RefCell<RecordInstance>>, ValueError> {
    match value {
        Value::Record(record) => Ok(record.clone()),
        _ => Err(ValueError::roc_required()),
    }
}

/// `S2V` (`JvInterpreter.pas:1590`): an integer as a set value.
pub fn s2v(value: i32) -> Value {
    Value::Set(value)
}

/// `V2S` (`JvInterpreter.pas:1596`): the integer of a set value; a variant
/// array (a `[...]` constant passed where a set belongs) is `or`-ed from
/// `1 shl element` over its elements.
pub fn v2s(value: &Value) -> Result<i32, ValueError> {
    match value {
        Value::Set(v) => Ok(*v),
        Value::Smallint(v) => Ok(i32::from(*v)),
        Value::Integer(v) => Ok(*v),
        Value::Byte(v) => Ok(i32::from(*v)),
        Value::Int64(v) => Ok(*v as i32),
        Value::VariantArray(array) => {
            let mut result: i32 = 0;
            for element in &array.borrow().values {
                let index = to_i32(element)?;
                result |= 1i32.wrapping_shl(index as u32);
            }
            Ok(result)
        }
        _ => Ok(0),
    }
}

//=== Numeric conversions (the implicit conversions of typed slots) ==========
//
// The conversions are Delphi's `VarAsType` over the interpreter's values
// (`JvInterpreterVarAsType`, `JvInterpreter.pas:2466`, and the `Integer(V)`
// reads of the RTL handlers). A conversion that fails raises Delphi's own
// variant error, with its class and text: `EVariantTypeCastError` `Could
// not convert variant of type (UnicodeString) into type (Integer)` for a
// string that is no number, `EVariantOverflowError` `Overflow while
// converting variant of type (Int64) into type (Integer)` for a value out
// of the target's range, `EVariantTypeCastError` `Invalid variant type
// conversion` for a custom kind (an object, an array). Inside an expression
// `Expression1`'s handler turns any of them into `Type mismatch`
// (`Interpreter::expression1`); in a typed assignment or a statement call
// the error reaches the log with its class, as the oracle prints it
// (probes `v-slot-03`, `v-slot-12`, `x-006`, `x-038`, 2026-10-15).

thread_local! {
    static PENDING_VARIANT_ERROR: RefCell<Option<ValueError>> = const { RefCell::new(None) };
}

/// Records a variant error an argument reader found (an `IInterface(V)`
/// cast of a value that is no interface, which upstream raises inside the
/// handler); the adapter raises it when the handler returns
/// ([`take_pending_variant_error`]).
pub fn set_pending_variant_error(error: ValueError) {
    PENDING_VARIANT_ERROR.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(error);
        }
    });
}

/// The error [`set_pending_variant_error`] recorded, as a `Result`.
pub fn take_pending_variant_error() -> Result<(), ValueError> {
    match PENDING_VARIANT_ERROR.with(|slot| slot.borrow_mut().take()) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// `VarTypeAsText` of a value, for the conversion messages.
pub fn delphi_type_name(value: &Value) -> &'static str {
    match value {
        Value::Empty => "Empty",
        Value::Null => "Null",
        Value::Smallint(_) => "Smallint",
        Value::Integer(_) => "Integer",
        Value::Byte(_) => "Byte",
        Value::Word(_) => "Word",
        Value::Int64(_) => "Int64",
        Value::Double(_) => "Double",
        Value::Date(_) => "Date",
        Value::Str(_) => "UnicodeString",
        Value::Bool(_) => "Boolean",
        Value::Set(_) | Value::Pointer(_) | Value::Object(_) | Value::Class(_) | Value::Record(_) => "",
        Value::Array(_) | Value::VariantArray(_) => "",
    }
}

/// `EVariantTypeCastError`: `Could not convert variant of type (X) into
/// type (Y)`, or `Invalid variant type conversion` for a custom kind.
pub fn cast_error(value: &Value, target: &str) -> ValueError {
    let source = match value {
        // An `IInterface` element is a `varUnknown` (probe `x-236`).
        Value::Object(object) if object.borrow().is_interface() => "Unknown",
        other => delphi_type_name(other),
    };
    if source.is_empty() {
        return ValueError::external_class("EVariantTypeCastError", "Invalid variant type conversion".to_owned());
    }
    ValueError::external_class(
        "EVariantTypeCastError",
        format!("Could not convert variant of type ({source}) into type ({target})"),
    )
}

/// `EVariantOverflowError`: `Overflow while converting variant of type (X)
/// into type (Y)`.
pub fn overflow_error(value: &Value, target: &str) -> ValueError {
    ValueError::external_class(
        "EVariantOverflowError",
        format!(
            "Overflow while converting variant of type ({}) into type ({target})",
            delphi_type_name(value)
        ),
    )
}

/// The whole-number reading of a value for an integer slot: the integer
/// kinds as they are, a boolean as Delphi's `-1`/`0`, a double rounded
/// half to even (`VarR8ToI4`), a string through [`str_to_number`] (a
/// hexadecimal `$` form or a decimal, rounded when fractional), `Empty` and
/// `Null` as 0 (`JvInterpreterVarAsType`), a nil pointer as 0.
pub fn to_i64(value: &Value) -> Result<i64, ValueError> {
    match value {
        Value::Smallint(v) => Ok(i64::from(*v)),
        Value::Integer(v) => Ok(i64::from(*v)),
        Value::Byte(v) => Ok(i64::from(*v)),
        Value::Word(v) => Ok(i64::from(*v)),
        Value::Int64(v) => Ok(*v),
        Value::Bool(v) => Ok(if *v { -1 } else { 0 }),
        Value::Set(v) => Ok(i64::from(*v)),
        Value::Double(v) | Value::Date(v) => f64_to_i64(*v).ok_or_else(|| overflow_error(value, "Int64")),
        // `Unassigned` converts to 0; `Null` does not convert at all
        // (`NullStrictConvert`: `IntToStr(Null)` is the cast error, probe
        // `x-257`); the typed-slot rule for both is `var_as_type`'s.
        Value::Empty => Ok(0),
        Value::Null => Err(cast_error(value, "Int64")),
        Value::Str(bytes) => match str_to_number(bytes) {
            Some(Number::Int(v)) => Ok(v),
            Some(Number::Float(v)) => f64_to_i64(v).ok_or_else(|| overflow_error(value, "Int64")),
            None => Err(cast_error(value, "Int64")),
        },
        Value::Pointer(0) => Ok(0),
        // `i := <element>` is the cast error `(Unknown) into type (Integer)`
        // (probe `x-236`); `IntToStr(<element>)` is `Type mismatch` (probe
        // `x-234`) and the function refuses the interface itself.
        _ => Err(cast_error(value, "Int64")),
    }
}

/// The `Integer` reading: [`to_i64`] within the `Integer` range.
pub fn to_i32(value: &Value) -> Result<i32, ValueError> {
    let v = to_i64(value).map_err(|error| retarget(error, "Integer"))?;
    i32::try_from(v).map_err(|_| overflow_error(value, "Integer"))
}

pub fn to_i16(value: &Value) -> Result<i16, ValueError> {
    let v = to_i64(value).map_err(|error| retarget(error, "Smallint"))?;
    i16::try_from(v).map_err(|_| overflow_error(value, "Smallint"))
}

pub fn to_u8(value: &Value) -> Result<u8, ValueError> {
    let v = to_i64(value).map_err(|error| retarget(error, "Byte"))?;
    u8::try_from(v).map_err(|_| overflow_error(value, "Byte"))
}

pub fn to_u16(value: &Value) -> Result<u16, ValueError> {
    let v = to_i64(value).map_err(|error| retarget(error, "Word"))?;
    u16::try_from(v).map_err(|_| overflow_error(value, "Word"))
}

pub fn to_u32(value: &Value) -> Result<u32, ValueError> {
    let v = to_i64(value).map_err(|error| retarget(error, "Cardinal"))?;
    u32::try_from(v).map_err(|_| overflow_error(value, "Cardinal"))
}

/// The message of a failed [`to_i64`] names `Int64`; the narrower readers
/// name their own target.
fn retarget(error: ValueError, target: &str) -> ValueError {
    match error {
        ValueError::ExternalClass { class, message } => ValueError::ExternalClass {
            class,
            message: message.replace("into type (Int64)", &format!("into type ({target})")),
        },
        other => other,
    }
}

/// The `Double` reading: a boolean is `-1`/`0`, a string is parsed as the
/// OLE automation `VarR8FromStr` parses it ([`str_to_f64`]: a leading
/// currency sign and grouping commas are accepted), `Empty` and `Null` are
/// 0.
pub fn to_f64(value: &Value) -> Result<f64, ValueError> {
    match value {
        Value::Smallint(v) => Ok(f64::from(*v)),
        Value::Integer(v) => Ok(f64::from(*v)),
        Value::Byte(v) => Ok(f64::from(*v)),
        Value::Word(v) => Ok(f64::from(*v)),
        Value::Int64(v) => Ok(*v as f64),
        Value::Double(v) | Value::Date(v) => Ok(*v),
        Value::Bool(v) => Ok(if *v { -1.0 } else { 0.0 }),
        Value::Empty | Value::Pointer(0) => Ok(0.0),
        Value::Null => Err(cast_error(value, "Double")),
        Value::Str(bytes) => str_to_f64(bytes).ok_or_else(|| cast_error(value, "Double")),
        _ => Err(cast_error(value, "Double")),
    }
}

/// The `Boolean` reading: a number is `<> 0`, the strings `True`/`False`
/// (and the `-1`/`0` texts) convert, anything else is the cast error.
pub fn to_bool(value: &Value) -> Result<bool, ValueError> {
    match value {
        Value::Bool(v) => Ok(*v),
        Value::Empty => Ok(false),
        Value::Null => Err(cast_error(value, "Boolean")),
        Value::Integer(0) | Value::Smallint(0) | Value::Byte(0) | Value::Word(0) | Value::Int64(0) => Ok(false),
        Value::Integer(_) | Value::Smallint(_) | Value::Byte(_) | Value::Word(_) | Value::Int64(_) => Ok(true),
        Value::Double(v) | Value::Date(v) => Ok(*v != 0.0),
        Value::Str(bytes) => {
            let text = String::from_utf8_lossy(bytes);
            let text = text.trim();
            if text.eq_ignore_ascii_case("true") {
                Ok(true)
            } else if text.eq_ignore_ascii_case("false") {
                Ok(false)
            } else if let Some(v) = xedit_io::fpu::parse_f64(text) {
                Ok(v != 0.0)
            } else {
                Err(cast_error(value, "Boolean"))
            }
        }
        _ => Err(cast_error(value, "Boolean")),
    }
}

/// `VarToStr`-shaped conversion for concatenation and `varString` slots.
pub fn to_str(value: &Value) -> Result<Vec<u8>, ValueError> {
    match value {
        Value::Str(bytes) => Ok(bytes.clone()),
        Value::Smallint(v) => Ok(v.to_string().into_bytes()),
        Value::Integer(v) => Ok(v.to_string().into_bytes()),
        Value::Byte(v) => Ok(v.to_string().into_bytes()),
        Value::Word(v) => Ok(v.to_string().into_bytes()),
        Value::Int64(v) => Ok(v.to_string().into_bytes()),
        Value::Double(v) | Value::Date(v) => Ok(float_to_str_delphi(*v)),
        Value::Bool(true) => Ok(b"True".to_vec()),
        Value::Bool(false) => Ok(b"False".to_vec()),
        Value::Empty | Value::Pointer(0) => Ok(Vec::new()),
        // The string parameters of the RTL are `OleStr`s to the conversion
        // (`sl.Add(Null)` is `Could not convert variant of type (Null) into
        // type (OleStr)`, probe `x-289`).
        Value::Null => Err(cast_error(value, "OleStr")),
        Value::Object(object) if object.borrow().is_interface() => Err(ValueError::external_class(
            "EVariantTypeCastError",
            "Could not convert variant of type (Unknown) into type (OleStr)".to_owned(),
        )),
        _ => Err(cast_error(value, "OleStr")),
    }
}

/// `Val` on an integer token (`ParseToken`, `JvInterpreter.pas:5561`): a `$`
/// prefixes a hexadecimal value.
pub fn parse_int_literal(text: &[u8]) -> Option<i64> {
    let (digits, radix) = match text.first() {
        Some(b'$') => (&text[1..], 16),
        _ => (text, 10),
    };
    std::str::from_utf8(digits)
        .ok()
        .and_then(|s| i64::from_str_radix(s, radix).ok())
}

/// `TextToFloat` on a double token (`ParseToken`, `JvInterpreter.pas:5577`).
pub fn parse_float_literal(text: &[u8]) -> Option<f64> {
    std::str::from_utf8(text).ok().and_then(xedit_io::fpu::parse_f64)
}

/// A number read from a string.
pub enum Number {
    Int(i64),
    Float(f64),
}

/// The string-to-integer reading of a typed slot (`VarAsType(S,
/// varInteger)`): a `$` hexadecimal, a whole decimal, else the decimal
/// reading of [`str_to_f64`] (so `'1.5'` lands as `2` after rounding),
/// with the spaces around it ignored.
pub fn str_to_number(bytes: &[u8]) -> Option<Number> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    if let Some(hex) = text.strip_prefix('$') {
        return i64::from_str_radix(hex.trim(), 16).ok().map(Number::Int);
    }
    if let Ok(v) = text.parse::<i64>() {
        return Some(Number::Int(v));
    }
    str_to_f64(bytes).map(Number::Float)
}

/// The string-to-double reading of the variant arithmetic
/// (`VarR8FromStr`, the OLE automation parser): spaces around the number,
/// a leading `$` currency sign and grouping commas are accepted, an
/// exponent too; anything else is no number (`'x' + 5` is `Type mismatch`,
/// `'$10' + 1` is `11`, `'1e2' + 1` is `101`, probes `v-04`, `v-66`,
/// `v-67`).
pub fn str_to_f64(bytes: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    let text = text.strip_prefix('$').unwrap_or(text).trim();
    let text: String = text.chars().filter(|c| *c != ',').collect();
    if text.is_empty() || text.eq_ignore_ascii_case("inf") || text.eq_ignore_ascii_case("nan") || text.contains("inf") {
        return None;
    }
    xedit_io::fpu::parse_f64(&text)
}

/// The variant-to-integer conversion of a typed slot assignment: the value
/// rounds to the nearest whole number (`VarI4FromR8`; the port rounds half
/// to even, as the RTL `Round` does, see `xedit_core::delphi::round`), and a
/// value outside the `Int64` range is `None`.
fn f64_to_i64(value: f64) -> Option<i64> {
    if !value.is_finite() || value < i64::MIN as f64 || value >= i64::MAX as f64 {
        return None;
    }
    Some(value.round_ties_even() as i64)
}

/// `FloatToStr` for the values a script turns into text: 15 significant
/// digits, plain notation inside `1e-4 .. 1e15`, otherwise one digit before
/// the point and an `E` exponent (`1E16`, `1E-5`); the RTL's `INF`, `-INF`
/// and `NAN` for the non-finite values (`VarToStr(1 / 0)` is `INF`, probe
/// `v-56`).
pub fn float_to_str_delphi(value: f64) -> Vec<u8> {
    if value.is_nan() {
        return b"NAN".to_vec();
    }
    if value.is_infinite() {
        return if value > 0.0 { b"INF".to_vec() } else { b"-INF".to_vec() };
    }
    if value == value.trunc() && value.abs() < 1e15 {
        return format!("{}", value as i64).into_bytes();
    }
    let negative = value < 0.0;
    let magnitude = value.abs();
    // `%.14e` gives 15 significant digits with the round-half-even of the
    // formatting; split it into digits and an exponent.
    let text = format!("{magnitude:.14e}");
    let (mantissa, exponent) = text.split_once('e').expect("scientific format");
    let exponent: i32 = exponent.parse().expect("exponent");
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut result = Vec::new();
    if negative {
        result.push(b'-');
    }
    if (-4..15).contains(&exponent) && exponent >= 0 {
        // Plain notation with the point inside or after the digits.
        let point = (exponent + 1) as usize;
        for (i, digit) in digits.bytes().enumerate() {
            if i == point {
                result.push(b'.');
            }
            result.push(digit);
        }
        if point > digits.len() {
            result.resize(result.len() + point - digits.len(), b'0');
        }
    } else if (-4..0).contains(&exponent) {
        // Plain notation with leading zeros after the point.
        result.extend_from_slice(b"0.");
        result.resize(result.len() + (-exponent - 1) as usize, b'0');
        result.extend_from_slice(digits.as_bytes());
    } else {
        result.push(digits.as_bytes()[0]);
        if digits.len() > 1 {
            result.push(b'.');
            result.extend_from_slice(&digits.as_bytes()[1..]);
        }
        result.push(b'E');
        result.extend_from_slice(exponent.to_string().as_bytes());
    }
    result
}

//=== The variant arithmetic ================================================
//
// Delphi's `Variant` operators as the oracle computes them (the `v-*`
// probes of `cargo xtask parity script --phase probe`, 2026-10-15), which
// `Expression` applies to its operands (`JvInterpreter.pas:5710-5836`):
//
// - `Null` on either side of an arithmetic operator is `Null`.
// - Two strings concatenate under `+`; a string beside a number or under
//   any other operator is read as a `Double` ([`str_to_f64`]), and one
//   that is no number is the `EVariantTypeCastError` `Expression1` reports
//   as `Type mismatch` (`'1' + 5` is the double `6`, `'x' + 5` fails).
// - A boolean is the integer `-1`/`0` beside an integer (`True + 1` is
//   `0`), and a double beside a double, a string or another boolean
//   (`True + True` is the double `-2`).
// - Two `Integer`s compute in `Integer`; a sum, difference or product
//   that overflows is a `Double` (`2147483647 + 1` is the double
//   `2147483648`). An `Int64` beside an integer computes in `Int64`.
// - `/` is a double division without a zero check (`1 / 0` is `INF`);
//   `div` and `mod` round their operands to integers (`1.5 div 1` is `2`)
//   and raise `EDivByZero` `Division by zero`.
// - A comparison of two strings is the string order; of a string and a
//   number the number order after the string is read as a double (`'10' >
//   9`, `'5' = 5`, `'x' = 5` fails); a boolean counts `-1`/`0` (`True = 1`
//   is `False`). `Null` equals only `Null` and orders below everything
//   (`Null < 1` is `True`, `Null > 1` is `False`).

/// A numeric operand of the arithmetic.
#[derive(Clone, Copy)]
enum Num {
    Int(i32),
    Int64(i64),
    Double(f64),
}

/// `+`, `-` and `*` of Delphi's variants.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arith {
    Add,
    Sub,
    Mul,
}

/// The arithmetic reading of an operand beside `other`.
fn arith_operand(value: &Value, other: &Value) -> Result<Num, ValueError> {
    Ok(match value {
        Value::Smallint(v) => Num::Int(i32::from(*v)),
        Value::Integer(v) => Num::Int(*v),
        Value::Byte(v) => Num::Int(i32::from(*v)),
        Value::Word(v) => Num::Int(i32::from(*v)),
        Value::Int64(v) => Num::Int64(*v),
        Value::Double(v) | Value::Date(v) => Num::Double(*v),
        Value::Empty => Num::Int(0),
        Value::Bool(v) => {
            let n = if *v { -1 } else { 0 };
            match other {
                Value::Smallint(_)
                | Value::Integer(_)
                | Value::Byte(_)
                | Value::Word(_)
                | Value::Int64(_)
                | Value::Empty => Num::Int(n),
                _ => Num::Double(f64::from(n)),
            }
        }
        Value::Str(bytes) => Num::Double(str_to_f64(bytes).ok_or_else(|| cast_error(value, "Double"))?),
        _ => return Err(cast_error(value, "Double")),
    })
}

fn arith(op: Arith, left: &Value, right: &Value) -> Result<Value, ValueError> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    if op == Arith::Add && left.is_str() && right.is_str() {
        let mut result = left.as_str().unwrap_or_default().to_vec();
        result.extend_from_slice(right.as_str().unwrap_or_default());
        return Ok(Value::Str(result));
    }
    let a = arith_operand(left, right)?;
    let b = arith_operand(right, left)?;
    let double = |a: f64, b: f64| match op {
        Arith::Add => a + b,
        Arith::Sub => a - b,
        Arith::Mul => a * b,
    };
    Ok(match (a, b) {
        (Num::Double(a), b) => Num::Double(double(a, num_f64(b))),
        (a, Num::Double(b)) => Num::Double(double(num_f64(a), b)),
        (Num::Int(a), Num::Int(b)) => {
            let result = match op {
                Arith::Add => a.checked_add(b),
                Arith::Sub => a.checked_sub(b),
                Arith::Mul => a.checked_mul(b),
            };
            match result {
                Some(v) => Num::Int(v),
                None => Num::Double(double(f64::from(a), f64::from(b))),
            }
        }
        (a, b) => {
            let (a, b) = (num_i64(a), num_i64(b));
            let result = match op {
                Arith::Add => a.checked_add(b),
                Arith::Sub => a.checked_sub(b),
                Arith::Mul => a.checked_mul(b),
            };
            match result {
                Some(v) => Num::Int64(v),
                None => Num::Double(double(a as f64, b as f64)),
            }
        }
    }
    .into_value())
}

fn num_f64(n: Num) -> f64 {
    match n {
        Num::Int(v) => f64::from(v),
        Num::Int64(v) => v as f64,
        Num::Double(v) => v,
    }
}

fn num_i64(n: Num) -> i64 {
    match n {
        Num::Int(v) => i64::from(v),
        Num::Int64(v) => v,
        Num::Double(v) => v.round_ties_even() as i64,
    }
}

impl Num {
    fn into_value(self) -> Value {
        match self {
            Num::Int(v) => Value::Integer(v),
            Num::Int64(v) => Value::Int64(v),
            Num::Double(v) => Value::Double(v),
        }
    }
}

/// `EDivByZero`'s `Division by zero`.
fn div_by_zero() -> ValueError {
    ValueError::external_class("EDivByZero", "Division by zero".to_owned())
}

/// The integer reading of an operand of `div`, `mod`, `shl`, `shr`, `and`,
/// `or` and `xor`: a double or a numeric string rounded, a boolean as
/// `-1`/`0`.
fn int_operand(value: &Value) -> Result<(i64, bool), ValueError> {
    Ok(match value {
        Value::Int64(v) => (*v, true),
        other => (to_i64(other)?, false),
    })
}

/// `Integer` when it fits and neither operand was an `Int64`, else
/// `Int64`.
fn int_value(value: i64, wide: bool) -> Value {
    match (wide, i32::try_from(value)) {
        (false, Ok(v)) => Value::Integer(v),
        _ => Value::Int64(value),
    }
}

//=== Comparison =============================================================

/// `VarCmp` for the pairs the interpreter's expression stack can produce:
/// two strings compare byte-wise (the locale-free `VarCompareValue` of
/// Unicode strings upstream); otherwise both sides read as doubles (a
/// string that is no number is the cast error, a boolean `-1`/`0`,
/// `Empty` 0). `Null` is handled by the callers.
pub fn variant_compare(left: &Value, right: &Value) -> Result<Ordering, ValueError> {
    if left.is_null() || right.is_null() {
        return Err(ValueError::Variant);
    }
    match (left, right) {
        (Value::Str(a), Value::Str(b)) => Ok(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Ok(a.cmp(b)),
        (Value::VariantArray(a), Value::VariantArray(b)) => Ok((Rc::as_ptr(a) as usize).cmp(&(Rc::as_ptr(b) as usize))),
        (a, b) => {
            let a = to_f64(a)?;
            let b = to_f64(b)?;
            // The RTL's real comparison tests `<` and `>` and answers equal
            // otherwise, under the masked exceptions of the 4.1.5q build, so
            // a NaN compares equal to everything (`NaN = 1` is `True`,
            // `NaN < 1` and `NaN > 1` are `False`; probe `float-invalid`).
            Ok(a.partial_cmp(&b).unwrap_or(Ordering::Equal))
        }
    }
}

//=== Operators ==============================================================

/// `not Expression(TTyp)` (`JvInterpreter.pas:5785`): logical for a boolean,
/// bitwise for an integer (a double or a numeric string rounded first);
/// `Null` stays `Null`.
pub fn unary_not(value: &Value) -> Result<Value, ValueError> {
    match value {
        Value::Bool(v) => Ok(Value::Bool(!v)),
        Value::Null => Ok(Value::Null),
        Value::Smallint(v) => Ok(Value::Smallint(!v)),
        Value::Integer(v) => Ok(Value::Integer(!v)),
        Value::Byte(v) => Ok(Value::Byte(!v)),
        Value::Int64(v) => Ok(Value::Int64(!v)),
        Value::Set(v) => Ok(Value::Set(!v)),
        Value::Str(_) => Err(ValueError::Variant),
        other => {
            let (v, wide) = int_operand(other)?;
            Ok(int_value(!v, wide))
        }
    }
}

/// Unary minus (`JvInterpreter.pas:5741`): a string negates as a double
/// (`-'5'` is the double `-5`), a boolean as its integer, an `Integer`
/// whose negation overflows becomes a double, `Null` stays `Null`.
pub fn unary_minus(value: &Value) -> Result<Value, ValueError> {
    match value {
        Value::Smallint(v) => Ok(Value::Integer(-(i32::from(*v)))),
        Value::Integer(v) => Ok(match v.checked_neg() {
            Some(n) => Value::Integer(n),
            None => Value::Double(-f64::from(*v)),
        }),
        Value::Byte(v) => Ok(Value::Integer(-(i32::from(*v)))),
        Value::Int64(v) => Ok(match v.checked_neg() {
            Some(n) => Value::Int64(n),
            None => Value::Double(-(*v as f64)),
        }),
        Value::Double(v) => Ok(Value::Double(-v)),
        Value::Date(v) => Ok(Value::Date(-v)),
        // `-True` is the `Smallint` `-1` (probes `x-188`, `x-197`).
        Value::Bool(v) => Ok(Value::Smallint(if *v { -1 } else { 0 })),
        Value::Str(bytes) => Ok(Value::Double(
            -str_to_f64(bytes).ok_or_else(|| cast_error(value, "Double"))?,
        )),
        Value::Null => Ok(Value::Null),
        Value::Empty => Ok(Value::Integer(0)),
        _ => Err(cast_error(value, "Double")),
    }
}

/// The variant operators of `Expression` (`JvInterpreter.pas:5710-5836`),
/// applied to the popped left operand and the evaluated right operand.
pub fn binary_op(op: BinOp, left: &Value, right: &Value) -> Result<Value, ValueError> {
    match op {
        BinOp::Plus => match left {
            // The `varSet` union (`:5727`): only a set on the *left* takes
            // this arm, as upstream tests `Tmp` only.
            Value::Set(a) => Ok(Value::Set(*a | to_i32(right)?)),
            _ => arith(Arith::Add, left, right),
        },
        BinOp::Minus => match left {
            // The `varSet` difference (`:5748`).
            Value::Set(a) => Ok(Value::Set(*a & !to_i32(right)?)),
            _ => arith(Arith::Sub, left, right),
        },
        BinOp::Mul => arith(Arith::Mul, left, right),
        // `/` is always a double division (`:5759`), without a zero check:
        // the floating point exceptions are masked, so `1 / 0` is `INF`.
        BinOp::Div => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            let a = num_f64(arith_operand(left, right)?);
            let b = num_f64(arith_operand(right, left)?);
            Ok(Value::Double(a / b))
        }
        BinOp::IntDiv | BinOp::Mod => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            let (a, wide_a) = int_operand(left)?;
            let (b, wide_b) = int_operand(right)?;
            if b == 0 {
                return Err(div_by_zero());
            }
            let result = if op == BinOp::IntDiv {
                a.checked_div(b)
            } else {
                a.checked_rem(b)
            };
            Ok(int_value(result.ok_or(ValueError::Variant)?, wide_a || wide_b))
        }
        BinOp::Or | BinOp::And | BinOp::Xor => match (left, right) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(match op {
                BinOp::Or => *a || *b,
                BinOp::And => *a && *b,
                _ => *a != *b,
            })),
            _ => {
                if left.is_null() || right.is_null() {
                    // The three-valued rules: `Null or True` is `True`,
                    // `Null and False` is `False`, the rest is `Null`
                    // (probe `x-176`).
                    let other = if left.is_null() { right } else { left };
                    return Ok(match (op, other) {
                        (BinOp::Or, Value::Bool(true)) => Value::Bool(true),
                        (BinOp::And, Value::Bool(false)) => Value::Bool(false),
                        _ => Value::Null,
                    });
                }
                let (a, wide_a) = int_operand(left)?;
                let (b, wide_b) = int_operand(right)?;
                let result = match op {
                    BinOp::Or => a | b,
                    BinOp::And => a & b,
                    _ => a ^ b,
                };
                Ok(int_value(result, wide_a || wide_b))
            }
        },
        BinOp::Shl => {
            let (a, _) = int_operand(left)?;
            let (b, _) = int_operand(right)?;
            Ok(Value::Integer((a as i32).wrapping_shl(b as u32)))
        }
        BinOp::Shr => {
            // A logical shift, as the 32-bit `shr` is (`$FFFFFFFF shr 1` is
            // `$7FFFFFFF`).
            let (a, _) = int_operand(left)?;
            let (b, _) = int_operand(right)?;
            Ok(Value::Integer(((a as i32 as u32) >> (b as u32 & 31)) as i32))
        }
        BinOp::Equ | BinOp::NotEqu => compare_equality(op, left, right),
        BinOp::Greater | BinOp::Less | BinOp::EquGreater | BinOp::EquLess => {
            // `Null` orders below everything and equals itself.
            let ordering = match (left.is_null(), right.is_null()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => variant_compare(left, right)?,
            };
            Ok(Value::Bool(match op {
                BinOp::Greater => ordering == Ordering::Greater,
                BinOp::Less => ordering == Ordering::Less,
                BinOp::EquGreater => ordering != Ordering::Less,
                _ => ordering != Ordering::Greater,
            }))
        }
    }
}

/// `ttEqu`/`ttNotEqu` (`JvInterpreter.pas:5787-5816`): a pointer-like left
/// operand (object, class, set, pointer; `varUnknown` too for `<>`)
/// compares its raw integer payload, everything else compares as values.
fn compare_equality(op: BinOp, left: &Value, right: &Value) -> Result<Value, ValueError> {
    // Only these kinds take the raw comparison upstream (`:5792-5793`);
    // records and arrays fall through to the general variant comparison,
    // which rejects them.
    let raw_left = match left {
        // `varUnknown` (an `IInterface` element) is in the `<>` list only
        // (`:5807`); under `=` it goes to the variant comparison, which
        // refuses it (`e = nil` is `Type mismatch`, probe `x-228`).
        Value::Object(object) if object.borrow().is_interface() => op == BinOp::NotEqu,
        Value::Object(_) | Value::Class(_) | Value::Set(_) | Value::Pointer(_) => true,
        _ => false,
    };
    if raw_left {
        let a = left.raw().ok_or(ValueError::Variant)?;
        // The right side reads `.VInteger` raw upstream; a nil comparison
        // (`obj = nil`) compares against the pointer value 0.
        let b = match right {
            Value::Null => 0,
            other => other.raw().unwrap_or(i64::MIN),
        };
        let equal = a == b;
        return Ok(Value::Bool(if op == BinOp::Equ { equal } else { !equal }));
    }
    // `Null = Null` is `True`, `Null = 1` is `False` (`NullEqualityRule`).
    if matches!(left, Value::Object(_)) {
        return Err(ValueError::Variant);
    }
    let equal = match (left.is_null(), right.is_null()) {
        (true, true) => true,
        (true, false) | (false, true) => false,
        (false, false) => variant_compare(left, right)? == Ordering::Equal,
    };
    Ok(Value::Bool(if op == BinOp::Equ { equal } else { !equal }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_names() {
        assert_eq!(type_name_2_var_type(b"integer"), VarType::Integer);
        assert_eq!(type_name_2_var_type(b"Integer"), VarType::Integer);
        assert_eq!(type_name_2_var_type(b"char"), VarType::Str);
        assert_eq!(type_name_2_var_type(b"tdatetime"), VarType::Date);
        assert_eq!(type_name_2_var_type(b"word"), VarType::Smallint);
        assert_eq!(type_name_2_var_type(b"TStringList"), VarType::Empty);
        // The first-character switch upstream (`:1657`) is an optimization:
        // `AnsiString` matches under 'A' only case-insensitively.
        assert_eq!(type_name_2_var_type(b"ansistring"), VarType::Str);
    }

    #[test]
    fn record_self_assignment_is_a_noop() {
        // A `var` record parameter aliases its argument, so `UpdateVarParams`
        // assigns the record to itself (FindFirst's `var F: TSearchRec`).
        let def = Rc::new(RecordDef {
            unit: String::new(),
            name: "TSearchRec".to_string(),
            size: 8,
            fields: vec![
                RecordField {
                    name: "Time".to_string(),
                    offset: 0,
                    typ: VarType::Integer,
                    data_type: None,
                },
                RecordField {
                    name: "Name".to_string(),
                    offset: 4,
                    typ: VarType::Str,
                    data_type: None,
                },
            ],
        });
        let record = Rc::new(RefCell::new(RecordInstance::new(def)));
        record.borrow_mut().values[0] = Value::Integer(42);
        record.borrow_mut().values[1] = Value::Str(b"parity.btt".to_vec());
        let mut dest = Value::Record(record.clone());
        let source = dest.clone();
        jv_var_assignment(&mut dest, &source).unwrap();
        let borrowed = record.borrow();
        assert_eq!(borrowed.values[0], Value::Integer(42));
        assert_eq!(borrowed.values[1], Value::Str(b"parity.btt".to_vec()));
    }

    #[test]
    fn float_text_matches_float_to_str() {
        assert_eq!(float_to_str_delphi(5.0), b"5");
        assert_eq!(float_to_str_delphi(-3.0), b"-3");
        assert_eq!(float_to_str_delphi(2.5), b"2.5");
        assert_eq!(float_to_str_delphi(0.1), b"0.1");
        assert_eq!(float_to_str_delphi(1e16), b"1E16");
        assert_eq!(float_to_str_delphi(0.00001), b"1E-5");
        assert_eq!(float_to_str_delphi(0.0001), b"0.0001");
        assert_eq!(float_to_str_delphi(1.0 / 3.0), b"0.333333333333333");
    }

    #[test]
    fn arithmetic_promotion_and_sets() {
        let five = Value::Integer(5);
        let two_point_five = Value::Double(2.5);
        assert_eq!(
            binary_op(BinOp::Plus, &five, &Value::Integer(6)).unwrap(),
            Value::Integer(11)
        );
        assert_eq!(
            binary_op(BinOp::Plus, &five, &two_point_five).unwrap(),
            Value::Double(7.5)
        );
        // Two `Integer` operands compute in `Integer`; a sum that overflows
        // is a double (`2147483647 + 1` is the double `2147483648`, probe
        // `v-52`), and an `Int64` operand widens the pair instead.
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Integer(i32::MAX), &five).unwrap(),
            Value::Double(2_147_483_652.0)
        );
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Int64(3_000_000_000), &five).unwrap(),
            Value::Int64(3_000_000_005)
        );
        // A boolean is `-1` next to an integer (`True + 1` is `0`, probe
        // `v-06`) and a double next to another boolean (`True + True` is
        // the double `-2`, probe `v-08`).
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Bool(true), &five).unwrap(),
            Value::Integer(4)
        );
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Bool(true), &Value::Bool(true)).unwrap(),
            Value::Double(-2.0)
        );
        // A numeric string reads as a double, one that is no number is the
        // variant cast error (`'1' + 5` is the double `6`, `'x' + 5` fails,
        // probes `v-01`, `v-04`).
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Str(b"1".to_vec()), &five).unwrap(),
            Value::Double(6.0)
        );
        assert!(matches!(
            binary_op(BinOp::Plus, &Value::Str(b"x".to_vec()), &five),
            Err(ValueError::ExternalClass {
                class: "EVariantTypeCastError",
                ..
            })
        ));
        assert!(matches!(
            binary_op(BinOp::Plus, &Value::Str(b"x".to_vec()), &Value::Bool(true)),
            Err(ValueError::ExternalClass {
                class: "EVariantTypeCastError",
                ..
            })
        ));
        // A NaN compares equal to everything (probe `float-invalid`).
        let nan = binary_op(BinOp::Div, &Value::Double(0.0), &Value::Double(0.0)).unwrap();
        assert!(matches!(nan, Value::Double(v) if v.is_nan()));
        assert_eq!(variant_compare(&nan, &Value::Integer(1)).unwrap(), Ordering::Equal);
        assert_eq!(variant_compare(&Value::Double(1.0), &nan).unwrap(), Ordering::Equal);
        // `/` is a double division without a zero check; `div` by zero is
        // `EDivByZero` (probes `v-56`, `v-57`).
        assert_eq!(
            binary_op(BinOp::Div, &Value::Integer(1), &Value::Integer(0)).unwrap(),
            Value::Double(f64::INFINITY)
        );
        assert!(matches!(
            binary_op(BinOp::IntDiv, &Value::Integer(1), &Value::Integer(0)),
            Err(ValueError::ExternalClass {
                class: "EDivByZero",
                ..
            })
        ));
        // Set union and difference (`:5727`, `:5748`).
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Set(0b01), &Value::Set(0b10)).unwrap(),
            Value::Set(0b11)
        );
        assert_eq!(
            binary_op(BinOp::Minus, &Value::Set(0b11), &Value::Set(0b01)).unwrap(),
            Value::Set(0b10)
        );
        // A string beside a number is read as a number, so `'a' + 1` is the
        // variant cast error, not `a1` (probe `v-04`); two strings
        // concatenate.
        assert!(matches!(
            binary_op(BinOp::Plus, &Value::Str(b"a".to_vec()), &Value::Integer(1)),
            Err(ValueError::ExternalClass {
                class: "EVariantTypeCastError",
                ..
            })
        ));
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Str(b"a".to_vec()), &Value::Str(b"1".to_vec())).unwrap(),
            Value::Str(b"a1".to_vec())
        );
        // `'x' + True` is the variant cast error too (probe `v-05`).
        assert!(matches!(
            binary_op(BinOp::Plus, &Value::Str(b"x".to_vec()), &Value::Bool(true)),
            Err(ValueError::ExternalClass {
                class: "EVariantTypeCastError",
                ..
            })
        ));
        // `div` keeps the integer kind; `/` is a double.
        assert_eq!(
            binary_op(BinOp::IntDiv, &Value::Integer(7), &Value::Integer(2)).unwrap(),
            Value::Integer(3)
        );
        assert_eq!(
            binary_op(BinOp::Div, &Value::Integer(1), &Value::Integer(2)).unwrap(),
            Value::Double(0.5)
        );
        assert_eq!(
            binary_op(BinOp::Mod, &Value::Integer(-7), &Value::Integer(2)).unwrap(),
            Value::Integer(-1)
        );
        // A logical `shr`, masked by 31.
        assert_eq!(
            binary_op(BinOp::Shr, &Value::Integer(-1), &Value::Integer(1)).unwrap(),
            Value::Integer(0x7FFFFFFF)
        );
        // Booleans are logical.
        assert_eq!(
            binary_op(BinOp::And, &Value::Bool(true), &Value::Bool(false)).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            binary_op(BinOp::Or, &Value::Integer(0b01), &Value::Integer(0b10)).unwrap(),
            Value::Integer(0b11)
        );
    }

    #[test]
    fn the_64_bit_literal_shapes() {
        // `ParseToken`: a literal that does not fit an `Integer` is an Int64
        // (`JvInterpreter.pas:5565`); `1` and `$FF` stay integers.
        assert_eq!(parse_int_literal(b"1"), Some(1));
        assert_eq!(parse_int_literal(b"$FF"), Some(255));
        assert_eq!(parse_int_literal(b"3000000000"), Some(3_000_000_000));
        assert_eq!(parse_int_literal(b"$FFFFFFFF"), Some(4_294_967_295));
    }

    #[test]
    fn comparisons() {
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Integer(1), &Value::Double(1.0)).unwrap(),
            Value::Bool(true)
        );
        // Strings compare byte-wise and case-sensitively.
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Str(b"a".to_vec()), &Value::Str(b"A".to_vec())).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            binary_op(BinOp::Less, &Value::Str(b"A".to_vec()), &Value::Str(b"a".to_vec())).unwrap(),
            Value::Bool(true)
        );
        // A boolean counts `-1`/`0` next to numbers (`True = 1` is `False`,
        // probe `v-38`); `Null` equals only `Null` and orders below
        // everything (probes `v-41`, `v-42`, `v-75`).
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Bool(true), &Value::Integer(1)).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Bool(true), &Value::Integer(-1)).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Null, &Value::Null).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Null, &Value::Integer(1)).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            binary_op(BinOp::Less, &Value::Null, &Value::Integer(1)).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Str(b"5".to_vec()), &Value::Integer(5)).unwrap(),
            Value::Bool(true)
        );
        // A pointer-like left compares raw: two distinct objects differ, an
        // object and a nil pointer do not match.
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Pointer(0), &Value::Pointer(0)).unwrap(),
            Value::Bool(true)
        );
        // A numeric string compares as a number (`1 = '1'` is `True`, probe
        // `v-33`); one that is no number is the variant cast error.
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Str(b"1".to_vec()), &Value::Integer(1)).unwrap(),
            Value::Bool(true)
        );
        assert!(matches!(
            binary_op(BinOp::Equ, &Value::Str(b"x".to_vec()), &Value::Integer(1)),
            Err(ValueError::ExternalClass {
                class: "EVariantTypeCastError",
                ..
            })
        ));
        // `Null` compares as a value: equal to `Null` only, below the rest
        // (probes `v-41`, `v-42`, `v-76`).
        assert_eq!(
            binary_op(BinOp::Greater, &Value::Null, &Value::Integer(1)).unwrap(),
            Value::Bool(false)
        );
    }
}
