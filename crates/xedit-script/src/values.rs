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
    /// A raised `EJvInterpreterError`.
    Interp { code: i32, name1: String, name2: String },
}

impl ValueError {
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
    /// `Args.Obj.Free` (`JvInterpreter.pas:3976`). A Rust object cannot be
    /// freed through a shared handle; this is the adapter's signal to mark
    /// itself released. Upstream frees the Delphi object here.
    fn free(&mut self) {}
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
}

impl VariantArray {
    pub fn new(values: Vec<Value>) -> Self {
        VariantArray { low: 0, values }
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
            Value::Smallint(_) | Value::Integer(_) | Value::Byte(_) | Value::Int64(_) | Value::Bool(_)
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
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Empty, Value::Empty) | (Value::Null, Value::Null) => true,
            (Value::Smallint(a), Value::Smallint(b)) => a == b,
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Byte(a), Value::Byte(b)) => a == b,
            (Value::Int64(a), Value::Int64(b)) => a == b,
            (Value::Double(a), Value::Double(b)) | (Value::Date(a), Value::Date(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => a == b,
            (Value::Pointer(a), Value::Pointer(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
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
            VarType::Integer | VarType::Smallint | VarType::Byte | VarType::Int64 => Ok(zero_value(typ)),
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
            } else {
                Err(ValueError::Variant)
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

pub fn to_i64(value: &Value) -> Result<i64, ValueError> {
    match value {
        Value::Smallint(v) => Ok(i64::from(*v)),
        Value::Integer(v) => Ok(i64::from(*v)),
        Value::Byte(v) => Ok(i64::from(*v)),
        Value::Int64(v) => Ok(*v),
        Value::Bool(v) => Ok(i64::from(*v)),
        Value::Set(v) => Ok(i64::from(*v)),
        Value::Double(v) | Value::Date(v) => f64_to_i64(*v),
        Value::Empty | Value::Null => Ok(0),
        Value::Str(bytes) => match str_to_i64(bytes) {
            Some(v) => Ok(v),
            None => Err(ValueError::Variant),
        },
        _ => Err(ValueError::Variant),
    }
}

pub fn to_i32(value: &Value) -> Result<i32, ValueError> {
    let v = to_i64(value)?;
    i32::try_from(v).map_err(|_| ValueError::Variant)
}

pub fn to_i16(value: &Value) -> Result<i16, ValueError> {
    let v = to_i64(value)?;
    i16::try_from(v).map_err(|_| ValueError::Variant)
}

pub fn to_u8(value: &Value) -> Result<u8, ValueError> {
    let v = to_i64(value)?;
    u8::try_from(v).map_err(|_| ValueError::Variant)
}

pub fn to_f64(value: &Value) -> Result<f64, ValueError> {
    match value {
        Value::Smallint(v) => Ok(f64::from(*v)),
        Value::Integer(v) => Ok(f64::from(*v)),
        Value::Byte(v) => Ok(f64::from(*v)),
        Value::Int64(v) => Ok(*v as f64),
        Value::Double(v) | Value::Date(v) => Ok(*v),
        Value::Bool(v) => Ok(if *v { 1.0 } else { 0.0 }),
        Value::Empty | Value::Null => Ok(0.0),
        Value::Str(bytes) => match std::str::from_utf8(bytes)
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
        {
            Some(v) => Ok(v),
            None => Err(ValueError::Variant),
        },
        _ => Err(ValueError::Variant),
    }
}

pub fn to_bool(value: &Value) -> Result<bool, ValueError> {
    match value {
        Value::Bool(v) => Ok(*v),
        Value::Empty | Value::Null => Ok(false),
        Value::Integer(0) | Value::Smallint(0) | Value::Byte(0) | Value::Int64(0) => Ok(false),
        Value::Integer(_) | Value::Smallint(_) | Value::Byte(_) | Value::Int64(_) => Ok(true),
        Value::Str(bytes) => {
            if bytes.eq_ignore_ascii_case(b"true") {
                Ok(true)
            } else if bytes.eq_ignore_ascii_case(b"false") {
                Ok(false)
            } else {
                Err(ValueError::Variant)
            }
        }
        _ => Err(ValueError::Variant),
    }
}

/// `VarToStr`-shaped conversion for concatenation and `varString` slots.
pub fn to_str(value: &Value) -> Result<Vec<u8>, ValueError> {
    match value {
        Value::Str(bytes) => Ok(bytes.clone()),
        Value::Smallint(v) => Ok(v.to_string().into_bytes()),
        Value::Integer(v) => Ok(v.to_string().into_bytes()),
        Value::Byte(v) => Ok(v.to_string().into_bytes()),
        Value::Int64(v) => Ok(v.to_string().into_bytes()),
        Value::Double(v) | Value::Date(v) => Ok(float_to_str_delphi(*v)),
        Value::Bool(true) => Ok(b"True".to_vec()),
        Value::Bool(false) => Ok(b"False".to_vec()),
        Value::Empty | Value::Null => Ok(Vec::new()),
        _ => Err(ValueError::Variant),
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
    std::str::from_utf8(text).ok().and_then(|s| s.parse().ok())
}

fn str_to_i64(bytes: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    if let Some(hex) = text.strip_prefix('$') {
        i64::from_str_radix(hex.trim(), 16).ok()
    } else {
        text.parse().ok()
    }
}

/// The variant-to-integer conversion of a typed slot assignment: the value
/// rounds to the nearest whole number (`VarI4FromR8`; the port rounds half
/// to even, as the RTL `Round` does, see `xedit_core::delphi::round`), and a
/// value outside the target's range is a variant error.
fn f64_to_i64(value: f64) -> Result<i64, ValueError> {
    if !value.is_finite() || value < i64::MIN as f64 || value >= i64::MAX as f64 {
        return Err(ValueError::Variant);
    }
    Ok(value.round_ties_even() as i64)
}

/// `FloatToStr` for the values a script turns into text: 15 significant
/// digits, plain notation inside `1e-4 .. 1e15`, otherwise one digit before
/// the point and an `E` exponent (`1E16`, `1E-5`).
pub fn float_to_str_delphi(value: f64) -> Vec<u8> {
    if value.is_nan() {
        return b"Nan".to_vec();
    }
    if value.is_infinite() {
        return if value > 0.0 { b"Inf".to_vec() } else { b"-Inf".to_vec() };
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

//=== Comparison =============================================================

/// `VarCmp` for the pairs the interpreter's expression stack can produce.
/// `Null` propagates (a comparison with `Null` is `Null`); `Empty` coerces
/// to the other side's zero default; numbers compare as `Double` when either
/// is one (a `varBoolean` counts 0/1, the `Ord(V = True)` of `Var2Type`);
/// strings compare byte-wise (the locale case-sensitive collation upstream),
/// and every other pairing is a variant error.
pub fn variant_compare(left: &Value, right: &Value) -> Result<Ordering, ValueError> {
    if left.is_null() || right.is_null() {
        return Err(ValueError::Variant);
    }
    let left = coerce_empty(left, right);
    let right = coerce_empty(right, &left);
    let (left, right) = (left.as_ref(), right.as_ref());
    match (left, right) {
        (Value::Str(a), Value::Str(b)) => Ok(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Ok(a.cmp(b)),
        (a, b) if is_numeric(a) && is_numeric(b) => {
            if matches!(a, Value::Double(_) | Value::Date(_)) || matches!(b, Value::Double(_) | Value::Date(_)) {
                let a = to_f64(a)?;
                let b = to_f64(b)?;
                a.partial_cmp(&b).ok_or(ValueError::Variant)
            } else {
                Ok(to_i64(a)?.cmp(&to_i64(b)?))
            }
        }
        (Value::VariantArray(a), Value::VariantArray(b)) => Ok((Rc::as_ptr(a) as usize).cmp(&(Rc::as_ptr(b) as usize))),
        _ => Err(ValueError::Variant),
    }
}

fn is_numeric(value: &Value) -> bool {
    matches!(
        value,
        Value::Smallint(_)
            | Value::Integer(_)
            | Value::Byte(_)
            | Value::Int64(_)
            | Value::Double(_)
            | Value::Date(_)
            | Value::Bool(_)
    )
}

/// An `Empty`/`Null` operand takes the other side's zero default (COM's
/// `VarCmp` coercion).
fn coerce_empty<'a>(value: &'a Value, other: &Value) -> std::borrow::Cow<'a, Value> {
    if value.is_empty() {
        std::borrow::Cow::Owned(match other.var_type() {
            VarType::Str => Value::Str(Vec::new()),
            VarType::Double | VarType::Date => Value::Double(0.0),
            VarType::Bool => Value::Bool(false),
            _ => Value::Integer(0),
        })
    } else {
        std::borrow::Cow::Borrowed(value)
    }
}

//=== Operators ==============================================================

/// `not Expression(TTyp)` (`JvInterpreter.pas:5785`): logical for a boolean,
/// bitwise for an integer.
pub fn unary_not(value: &Value) -> Result<Value, ValueError> {
    match value {
        Value::Bool(v) => Ok(Value::Bool(!v)),
        Value::Smallint(v) => Ok(Value::Smallint(!v)),
        Value::Integer(v) => Ok(Value::Integer(!v)),
        Value::Byte(v) => Ok(Value::Byte(!v)),
        Value::Int64(v) => Ok(Value::Int64(!v)),
        Value::Set(v) => Ok(Value::Set(!v)),
        _ => Err(ValueError::Variant),
    }
}

/// Unary minus (`JvInterpreter.pas:5741`).
pub fn unary_minus(value: &Value) -> Result<Value, ValueError> {
    match value {
        Value::Smallint(v) => Ok(Value::Smallint(v.checked_neg().ok_or(ValueError::Variant)?)),
        Value::Integer(v) => Ok(Value::Integer(v.checked_neg().ok_or(ValueError::Variant)?)),
        Value::Byte(v) => Ok(Value::Integer(-(i32::from(*v)))),
        Value::Int64(v) => Ok(Value::Int64(v.checked_neg().ok_or(ValueError::Variant)?)),
        Value::Double(v) => Ok(Value::Double(-v)),
        Value::Date(v) => Ok(Value::Date(-v)),
        Value::Empty | Value::Null => Ok(Value::Integer(0)),
        _ => Err(ValueError::Variant),
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
            _ => variant_add(left, right),
        },
        BinOp::Minus => match left {
            // The `varSet` difference (`:5748`).
            Value::Set(a) => Ok(Value::Set(*a & !to_i32(right)?)),
            _ => variant_sub(left, right),
        },
        BinOp::Mul => variant_mul(left, right),
        // `/` is always a double division (`:5759`).
        BinOp::Div => {
            let a = to_f64(left)?;
            let b = to_f64(right)?;
            if b == 0.0 {
                return Err(ValueError::External("Division by zero".to_owned()));
            }
            Ok(Value::Double(a / b))
        }
        BinOp::IntDiv => {
            let a = to_i64(left)?;
            let b = to_i64(right)?;
            if b == 0 {
                return Err(ValueError::External("Division by zero".to_owned()));
            }
            let quotient = a.checked_div(b).ok_or(ValueError::Variant)?;
            Ok(int_result(left, right, quotient))
        }
        BinOp::Mod => {
            let a = to_i64(left)?;
            let b = to_i64(right)?;
            if b == 0 {
                return Err(ValueError::External("Division by zero".to_owned()));
            }
            let remainder = a.checked_rem(b).ok_or(ValueError::Variant)?;
            Ok(int_result(left, right, remainder))
        }
        BinOp::Or | BinOp::And | BinOp::Xor => match (left, right) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(match op {
                BinOp::Or => *a || *b,
                BinOp::And => *a && *b,
                _ => *a != *b,
            })),
            _ => {
                let a = to_i64(left)?;
                let b = to_i64(right)?;
                let result = match op {
                    BinOp::Or => a | b,
                    BinOp::And => a & b,
                    _ => a ^ b,
                };
                Ok(int_result(left, right, result))
            }
        },
        BinOp::Shl => {
            let a = to_i32(left)?;
            let b = to_i32(right)?;
            Ok(Value::Integer(a.wrapping_shl(b as u32)))
        }
        BinOp::Shr => {
            // A logical shift, as the 32-bit `shr` is (`$FFFFFFFF shr 1` is
            // `$7FFFFFFF`).
            let a = to_i32(left)?;
            let b = to_i32(right)?;
            Ok(Value::Integer(((a as u32) >> (b as u32 & 31)) as i32))
        }
        BinOp::Equ | BinOp::NotEqu => compare_equality(op, left, right),
        BinOp::Greater | BinOp::Less | BinOp::EquGreater | BinOp::EquLess => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            let ordering = variant_compare(left, right)?;
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
    let raw_left = matches!(
        left,
        Value::Object(_) | Value::Class(_) | Value::Set(_) | Value::Pointer(_)
    );
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
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    let ordering = variant_compare(left, right)?;
    let equal = ordering == Ordering::Equal;
    Ok(Value::Bool(if op == BinOp::Equ { equal } else { !equal }))
}

/// `Tmp + Expression(TTyp)` (`JvInterpreter.pas:5732`): a string operand
/// concatenates, numbers add in the wider kind, `Null` propagates. Two
/// `Integer`-sized operands compute in `Integer` -- an overflow is the
/// variant error `VarAdd` raises -- and an `Int64` operand widens the pair
/// to `Int64`; a boolean does not take part in arithmetic (only the
/// concatenation above reads it, as text).
fn variant_add(left: &Value, right: &Value) -> Result<Value, ValueError> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    if left.is_str() || right.is_str() {
        let a = to_str(left)?;
        let b = to_str(right)?;
        let mut result = a;
        result.extend_from_slice(&b);
        return Ok(Value::Str(result));
    }
    if matches!(left, Value::Double(_) | Value::Date(_)) || matches!(right, Value::Double(_) | Value::Date(_)) {
        return Ok(Value::Double(to_f64(left)? + to_f64(right)?));
    }
    if matches!(left, Value::Bool(_)) || matches!(right, Value::Bool(_)) {
        return Err(ValueError::Variant);
    }
    if matches!(left, Value::Int64(_)) || matches!(right, Value::Int64(_)) {
        let a = to_i64(left)?;
        let b = to_i64(right)?;
        return Ok(Value::Int64(a.checked_add(b).ok_or(ValueError::Variant)?));
    }
    let a = to_i32(left)?;
    let b = to_i32(right)?;
    Ok(Value::Integer(a.checked_add(b).ok_or(ValueError::Variant)?))
}

/// `Tmp - Expression(TTyp)` (`JvInterpreter.pas:5753`).
fn variant_sub(left: &Value, right: &Value) -> Result<Value, ValueError> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    if left.is_str() || right.is_str() {
        return Err(ValueError::Variant);
    }
    if matches!(left, Value::Double(_) | Value::Date(_)) || matches!(right, Value::Double(_) | Value::Date(_)) {
        return Ok(Value::Double(to_f64(left)? - to_f64(right)?));
    }
    if matches!(left, Value::Bool(_)) || matches!(right, Value::Bool(_)) {
        return Err(ValueError::Variant);
    }
    if matches!(left, Value::Int64(_)) || matches!(right, Value::Int64(_)) {
        let a = to_i64(left)?;
        let b = to_i64(right)?;
        return Ok(Value::Int64(a.checked_sub(b).ok_or(ValueError::Variant)?));
    }
    let a = to_i32(left)?;
    let b = to_i32(right)?;
    Ok(Value::Integer(a.checked_sub(b).ok_or(ValueError::Variant)?))
}

/// `PopExp * Expression(TTyp)` (`JvInterpreter.pas:5712`).
fn variant_mul(left: &Value, right: &Value) -> Result<Value, ValueError> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    if matches!(left, Value::Double(_) | Value::Date(_)) || matches!(right, Value::Double(_) | Value::Date(_)) {
        return Ok(Value::Double(to_f64(left)? * to_f64(right)?));
    }
    if matches!(left, Value::Bool(_)) || matches!(right, Value::Bool(_)) || left.is_str() || right.is_str() {
        return Err(ValueError::Variant);
    }
    if matches!(left, Value::Int64(_)) || matches!(right, Value::Int64(_)) {
        let a = to_i64(left)?;
        let b = to_i64(right)?;
        return Ok(Value::Int64(a.checked_mul(b).ok_or(ValueError::Variant)?));
    }
    let a = to_i32(left)?;
    let b = to_i32(right)?;
    Ok(Value::Integer(a.checked_mul(b).ok_or(ValueError::Variant)?))
}

/// The result kind of an integer operation: `Integer` when it fits and
/// neither operand is an `Int64`, `Int64` otherwise (the variant promotion
/// of `VarI4`/`VarI8`).
fn int_result(left: &Value, right: &Value, value: i64) -> Value {
    if !matches!(left, Value::Int64(_))
        && !matches!(right, Value::Int64(_))
        && let Ok(v) = i32::try_from(value)
    {
        return Value::Integer(v);
    }
    Value::Int64(value)
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
        // Two `Integer` operands compute in `Integer`; the overflow of
        // `VarAdd` is a variant error, and an `Int64` operand widens the
        // pair instead.
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Integer(i32::MAX), &five),
            Err(ValueError::Variant)
        );
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Int64(3_000_000_000), &five).unwrap(),
            Value::Int64(3_000_000_005)
        );
        // A boolean takes no part in arithmetic.
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Bool(true), &five),
            Err(ValueError::Variant)
        );
        // Set union and difference (`:5727`, `:5748`).
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Set(0b01), &Value::Set(0b10)).unwrap(),
            Value::Set(0b11)
        );
        assert_eq!(
            binary_op(BinOp::Minus, &Value::Set(0b11), &Value::Set(0b01)).unwrap(),
            Value::Set(0b10)
        );
        // String concatenation converts the other operand.
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Str(b"a".to_vec()), &Value::Integer(1)).unwrap(),
            Value::Str(b"a1".to_vec())
        );
        assert_eq!(
            binary_op(BinOp::Plus, &Value::Str(b"x".to_vec()), &Value::Bool(true)).unwrap(),
            Value::Str(b"xTrue".to_vec())
        );
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
        // A boolean counts 0/1 next to numbers.
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Bool(true), &Value::Integer(1)).unwrap(),
            Value::Bool(true)
        );
        // A pointer-like left compares raw: two distinct objects differ, an
        // object and a nil pointer do not match.
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Pointer(0), &Value::Pointer(0)).unwrap(),
            Value::Bool(true)
        );
        // Mismatched kinds are variant errors.
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Str(b"1".to_vec()), &Value::Integer(1)),
            Err(ValueError::Variant)
        );
        // Null propagates through comparisons.
        assert_eq!(
            binary_op(BinOp::Equ, &Value::Null, &Value::Integer(1)).unwrap(),
            Value::Null
        );
    }
}
