// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter_System.pas (the
// `System` unit: TObject and TInterfacedObject methods, the variant helpers,
// the string/math functions of the Delphi RTL, the array helpers of
// JvInterpreter.pas and the twenty-four variant type constants)

//! `JvInterpreter_System.pas`: the `System` unit of the script dialect.
//!
//! The registrations are bound one for one, including the two the upstream
//! file carries as quirks: `High` of a native variant array returns the
//! *low* bound (`JvInterpreter_System.pas:516` reads `VarArrayLowBound`, a
//! copy-paste that shipped) and the `Move` handler, which copies raw memory
//! the port's value model has none of, raises upstream's own
//! `NotImplemented` text.
//!
//! `TObject.ClassInfo` (the RTTI pointer) and `InstanceSize` are the two
//! members the port cannot answer faithfully: the value model carries no
//! RTTI blocks or instance sizes, so a script sees the nil pointer and 0.
//! `TInterfacedObject.RefCount` is 0 for a freshly created Delphi object.

use xedit_script::adapter::{Adapter, Args};
use xedit_script::error::{IE_ARRAY_INDEX_OUT_OF_BOUNDS, IE_TYPE_MISMATCH, RS_SORRY_FOR_ONE_DIMENSIONAL_ARRAYS_ONLY};
use xedit_script::values::{ArrayInstance, Value, ValueError, VarType, VariantArray, to_i32, to_str};

use super::{
    BYREF, DELPHI_VAR_ANY, DELPHI_VAR_ARRAY, DELPHI_VAR_CLASS, DELPHI_VAR_OBJECT, DELPHI_VAR_POINTER,
    DELPHI_VAR_RECORD, DELPHI_VAR_SET, DELPHI_VAR_STRING, DELPHI_VAR_TYPE_MASK, DELPHI_VAR_USTRING, EMPTY, INTEGER,
    STR_OR_ARRAY_BYREF, arg, arg_char, arg_f64, arg_i32, arg_str, constant, method, text_value,
};

/// `cSystem = 'System'` (`JvInterpreter_System.pas:552`).
const UNIT: &str = "System";

thread_local! {
    /// Delphi's `RandSeed`.
    static RAND_SEED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// `Randomize`: seeds from the tick count, as the RTL does.
pub(crate) fn randomize() {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u32)
        .unwrap_or(0);
    RAND_SEED.with(|cell| cell.set(seed | 1));
}

/// `Random(Number)`: `RandSeed := RandSeed * $08088405 + 1;` and the high
/// 32 bits of `RandSeed * Number` (the RTL's `_Random`).
pub(crate) fn random(number: i32) -> i32 {
    RAND_SEED.with(|cell| {
        let seed = cell.get().wrapping_mul(0x0808_8405).wrapping_add(1);
        cell.set(seed);
        ((u64::from(seed) * u64::from(number as u32)) >> 32) as i32
    })
}

/// The parameterless `Random`: the seed after its step, as a fraction of
/// 2^32 (`Math.RandG` draws through it).
pub(crate) fn random_unit() -> f64 {
    RAND_SEED.with(|cell| {
        let seed = cell.get().wrapping_mul(0x0808_8405).wrapping_add(1);
        cell.set(seed);
        f64::from(seed) / 4_294_967_296.0
    })
}

/// Registers the unit.
pub fn register(adapter: &mut Adapter) {
    let object = adapter.add_class(UNIT, "TObject", None);
    method(adapter, &object, "ClassType", &[], |value, args| {
        // `C2V(TObject(Args.Obj).ClassType)`.
        *value = match args.object() {
            Some(object) => Value::Class(object.borrow().class()),
            None => Value::Empty,
        };
        Ok(())
    });
    method(adapter, &object, "ClassName", &[], |value, args| {
        *value = match args.object() {
            Some(object) => text_value(object.borrow().class().name.clone().into_bytes()),
            None => text_value(Vec::new()),
        };
        Ok(())
    });
    method(adapter, &object, "ClassNameIs", &[EMPTY], |value, args| {
        let name = arg_str(args, 0);
        *value = Value::Bool(match args.object() {
            Some(object) => object.borrow().class().name.as_bytes().eq_ignore_ascii_case(&name),
            None => false,
        });
        Ok(())
    });
    method(adapter, &object, "ClassParent", &[], |value, args| {
        // `C2V(ClassParent)`: a class without a parent is the nil class
        // pointer upstream; the port has no nil class value, so it is
        // `Unassigned` (the step-6 convention for a nil object result).
        *value = args
            .object()
            .and_then(|object| object.borrow().class().parent.clone())
            .map_or(Value::Empty, Value::Class);
        Ok(())
    });
    method(adapter, &object, "ClassInfo", &[], |value, _args| {
        // The RTTI pointer; the port carries no RTTI, so it is nil.
        *value = Value::Pointer(0);
        Ok(())
    });
    method(adapter, &object, "InstanceSize", &[], |value, _args| {
        *value = Value::Integer(0);
        Ok(())
    });
    method(adapter, &object, "InheritsFrom", &[EMPTY], |value, args| {
        // `TObject(Args.Obj).InheritsFrom(V2C(Args.Values[0]))`.
        *value = Value::Bool(match (args.object(), arg(args, 0).as_class()) {
            (Some(object), Some(class)) => object.borrow().class().is_a(class),
            _ => false,
        });
        Ok(())
    });

    let interfaced = adapter.add_class(UNIT, "TInterfacedObject", Some(object));
    method(adapter, &interfaced, "RefCount", &[], |value, _args| {
        // A freshly created `TInterfacedObject` has not been `_AddRef`ed.
        *value = Value::Integer(0);
        Ok(())
    });

    // The functions, in the registration order of the unit.
    function_(adapter, "Move", &[EMPTY, BYREF, EMPTY], |_value, _args| {
        // `Move(Source, Dest, Count)` copies raw memory; the value model has
        // none, and a script passing variant values here is broken upstream
        // as well.
        Err(ValueError::not_implemented("Move"))
    });
    function_(adapter, "ParamCount", &[EMPTY], |value, _args| {
        *value = Value::Integer(std::env::args_os().count().saturating_sub(1) as i32);
        Ok(())
    });
    function_(adapter, "ParamStr", &[EMPTY], |value, args| {
        let index = arg_i32(args, 0)?;
        let text = if index < 0 {
            String::new()
        } else {
            std::env::args_os()
                .nth(index as usize)
                .map(|arg| arg.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        *value = text_value(text.into_bytes());
        Ok(())
    });
    function_(adapter, "Randomize", &[], |_value, _args| {
        randomize();
        Ok(())
    });
    function_(adapter, "Random", &[INTEGER], |value, args| {
        *value = Value::Integer(random(arg_i32(args, 0)?));
        Ok(())
    });
    function_(adapter, "UpCase", &[EMPTY], |value, args| {
        // `UpCase(string(Args.Values[0])[1])`: the RTL's Latin-1 table.
        let ch = arg_char(args, 0);
        let upper = if ch.is_ascii_lowercase() {
            ch.to_ascii_uppercase()
        } else {
            ch
        };
        *value = text_value(vec![upper]);
        Ok(())
    });
    function_(adapter, "VarType", &[EMPTY], |value, args| {
        *value = Value::Integer(delphi_var_type(arg(args, 0)));
        Ok(())
    });
    function_(adapter, "VarAsType", &[EMPTY, EMPTY], |value, args| {
        let target = arg_i32(args, 1)?;
        *value = var_as_type(arg(args, 0), target)?;
        Ok(())
    });
    function_(adapter, "VarIsEmpty", &[EMPTY], |value, args| {
        *value = Value::Bool(arg(args, 0).is_empty());
        Ok(())
    });
    function_(adapter, "VarIsNull", &[EMPTY], |value, args| {
        *value = Value::Bool(arg(args, 0).is_null());
        Ok(())
    });
    function_(adapter, "VarToStr", &[EMPTY], |value, args| {
        *value = Value::Str(to_str(arg(args, 0))?);
        Ok(())
    });
    function_(adapter, "VarFromDateTime", &[EMPTY], |value, args| {
        *value = Value::Date(arg_f64(args, 0)?);
        Ok(())
    });
    function_(adapter, "VarToDateTime", &[EMPTY], |value, args| {
        *value = Value::Double(arg_f64(args, 0)?);
        Ok(())
    });
    function_(adapter, "VarArrayCreate", &[EMPTY, EMPTY], |value, args| {
        *value = var_array_create(args)?;
        Ok(())
    });
    function_(adapter, "VarArrayOf", &[EMPTY], |value, args| {
        *value = Value::VariantArray(std::rc::Rc::new(std::cell::RefCell::new(VariantArray::new(
            args.open_array(0)?,
        ))));
        Ok(())
    });
    function_(adapter, "VarArrayDimCount", &[EMPTY], |value, args| {
        *value = Value::Integer(var_array_dims(arg(args, 0))? as i32);
        Ok(())
    });
    function_(adapter, "VarArrayLowBound", &[EMPTY, EMPTY], |value, args| {
        let array = variant_array(arg(args, 0))?;
        let array = array.borrow();
        *value = Value::Integer(array.low as i32);
        Ok(())
    });
    function_(adapter, "VarArrayHighBound", &[EMPTY, EMPTY], |value, args| {
        let array = variant_array(arg(args, 0))?;
        let array = array.borrow();
        *value = Value::Integer((array.low + array.values.len() as i64 - 1) as i32);
        Ok(())
    });
    function_(adapter, "VarIsArray", &[EMPTY], |value, args| {
        // `VarIsArray` tests the `varArray` bit; the interpreter's own array
        // type is a custom variant type without it (the `Length`/`High`/
        // `Low` handlers test `= varArray` separately), so only a native
        // variant array answers true.
        *value = Value::Bool(matches!(arg(args, 0), Value::VariantArray(_)));
        Ok(())
    });
    function_(adapter, "ord", &[EMPTY], |value, args| {
        // `if VarType(V) = varString then Ord(V[1]) else Integer(V)`; the
        // port removes the parenthesis around `ord` the upstream line has.
        *value = match arg(args, 0) {
            Value::Str(bytes) => Value::Integer(i32::from(bytes.first().copied().unwrap_or(0))),
            other => Value::Integer(to_i32(other)?),
        };
        Ok(())
    });
    function_(adapter, "Chr", &[EMPTY], |value, args| {
        *value = text_value(vec![arg_i32(args, 0)? as u8]);
        Ok(())
    });
    function_(adapter, "Abs", &[EMPTY], |value, args| {
        // `if VarType(V) = varInteger then Abs(Integer(V)) else
        // Abs(Extended(V))`: an exactly-`varInteger` argument stays an
        // integer, everything else (an `Int64` included) goes through
        // `Extended`.
        *value = match arg(args, 0) {
            Value::Integer(v) => Value::Integer(v.wrapping_abs()),
            other => Value::Double(xedit_script::values::to_f64(other)?.abs()),
        };
        Ok(())
    });
    function_(adapter, "Length", &[EMPTY], |value, args| {
        *value = Value::Integer(length(arg(args, 0))? as i32);
        Ok(())
    });
    function_(adapter, "Copy", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let source = to_str(arg(args, 0))?;
        let index = arg_i32(args, 1)?;
        let count = arg_i32(args, 2)?;
        *value = Value::Str(copy_str(&source, index, count));
        Ok(())
    });
    function_(adapter, "Round", &[EMPTY], |value, args| {
        // `Integer(Round(...))` is a bits-cast of the `Int64` result.
        *value = Value::Integer(xedit_core::delphi::round(arg_f64(args, 0)?) as i32);
        Ok(())
    });
    function_(adapter, "Trunc", &[EMPTY], |value, args| {
        *value = Value::Integer(arg_f64(args, 0)?.trunc() as i32);
        Ok(())
    });
    function_(adapter, "Pos", &[EMPTY, EMPTY], |value, args| {
        let needle = to_str(arg(args, 0))?;
        let haystack = to_str(arg(args, 1))?;
        *value = Value::Integer(pos(&needle, &haystack) as i32);
        Ok(())
    });
    function_(adapter, "Delete", &[BYREF, EMPTY, EMPTY], |value, args| {
        let mut text = arg_str(args, 0);
        delphi_delete(&mut text, arg_i32(args, 1)?, arg_i32(args, 2)?);
        // The handler writes the changed string back through the `var`
        // parameter *and* the result (both are the same text upstream).
        args.values[0] = Value::Str(text.clone());
        *value = Value::Str(text);
        Ok(())
    });
    function_(adapter, "Insert", &[EMPTY, BYREF, EMPTY], |value, args| {
        let source = to_str(arg(args, 0))?;
        let mut text = arg_str(args, 1);
        delphi_insert(&source, &mut text, arg_i32(args, 2)?);
        args.values[1] = Value::Str(text.clone());
        *value = Value::Str(text);
        Ok(())
    });
    function_(adapter, "Sqr", &[EMPTY], |value, args| {
        let x = arg_f64(args, 0)?;
        *value = Value::Double(x * x);
        Ok(())
    });
    function_(adapter, "Sqrt", &[EMPTY], |value, args| {
        *value = Value::Double(arg_f64(args, 0)?.sqrt());
        Ok(())
    });
    function_(adapter, "Exp", &[EMPTY], |value, args| {
        *value = Value::Double(arg_f64(args, 0)?.exp());
        Ok(())
    });
    function_(adapter, "Ln", &[EMPTY], |value, args| {
        *value = Value::Double(arg_f64(args, 0)?.ln());
        Ok(())
    });
    function_(adapter, "Sin", &[EMPTY], |value, args| {
        *value = Value::Double(xedit_assets::nif_math::sin(arg_f64(args, 0)?));
        Ok(())
    });
    function_(adapter, "Cos", &[EMPTY], |value, args| {
        *value = Value::Double(xedit_assets::nif_math::cos(arg_f64(args, 0)?));
        Ok(())
    });
    function_(adapter, "Tan", &[EMPTY], |value, args| {
        // `Tan(X) = Sin(X)/Cos(X)` (`:485`, because `Math` is not linked in
        // every Delphi SKU).
        let x = arg_f64(args, 0)?;
        *value = Value::Double(xedit_assets::nif_math::sin(x) / xedit_assets::nif_math::cos(x));
        Ok(())
    });
    function_(adapter, "ArcTan", &[EMPTY], |value, args| {
        *value = Value::Double(xedit_assets::nif_math::arc_tan(arg_f64(args, 0)?));
        Ok(())
    });
    function_(adapter, "SetLength", &[STR_OR_ARRAY_BYREF, INTEGER], |_value, args| {
        args.values[0] = set_length(args.values[0].clone(), arg_i32(args, 1)?)?;
        Ok(())
    });
    function_(adapter, "High", &[EMPTY], |value, args| {
        *value = Value::Integer(high_or_low(args, true)? as i32);
        Ok(())
    });
    function_(adapter, "Low", &[EMPTY], |value, args| {
        *value = Value::Integer(high_or_low(args, false)? as i32);
        Ok(())
    });
    function_(adapter, "DeleteFromArray", &[EMPTY, EMPTY], |_value, args| {
        let array = interpreter_array(arg(args, 0))?;
        let index = arg_i32(args, 1)? as i64;
        let mut array = array.borrow_mut();
        if array.begin.len() > 1 {
            return Err(ValueError::External(
                RS_SORRY_FOR_ONE_DIMENSIONAL_ARRAYS_ONLY.to_owned(),
            ));
        }
        if index < array.begin[0] || index > array.end[0] {
            return Err(ValueError::interp(IE_ARRAY_INDEX_OUT_OF_BOUNDS));
        }
        let offset = (index - array.begin[0]) as usize;
        array.values.remove(offset);
        array.end[0] -= 1;
        Ok(())
    });
    function_(adapter, "InsertIntoArray", &[EMPTY, EMPTY, EMPTY], |_value, args| {
        let array = interpreter_array(arg(args, 0))?;
        let index = arg_i32(args, 1)? as i64;
        let item = arg(args, 2).clone();
        let mut array = array.borrow_mut();
        if array.begin.len() > 1 {
            return Err(ValueError::External(
                RS_SORRY_FOR_ONE_DIMENSIONAL_ARRAYS_ONLY.to_owned(),
            ));
        }
        if index < array.begin[0] || index > array.end[0] {
            return Err(ValueError::interp(IE_ARRAY_INDEX_OUT_OF_BOUNDS));
        }
        let offset = (index - array.begin[0]) as usize;
        let slot = match array.item_type {
            VarType::Empty => Value::Null,
            typ => zero_value(typ),
        };
        array.values.insert(offset, slot);
        let mut stored = array.values[offset].clone();
        xedit_script::values::jv_var_assignment(&mut stored, &item)?;
        array.values[offset] = stored;
        array.end[0] += 1;
        Ok(())
    });

    // The variant type constants, in the registration order of the unit.
    constant(adapter, UNIT, "varEmpty", 0);
    constant(adapter, UNIT, "varSmallint", 2);
    constant(adapter, UNIT, "varInteger", 3);
    constant(adapter, UNIT, "varSingle", 4);
    constant(adapter, UNIT, "varCurrency", 6);
    constant(adapter, UNIT, "varDouble", 5);
    constant(adapter, UNIT, "varDate", 7);
    constant(adapter, UNIT, "varOleStr", 8);
    constant(adapter, UNIT, "varDispatch", 9);
    constant(adapter, UNIT, "varError", 10);
    constant(adapter, UNIT, "varBoolean", 11);
    constant(adapter, UNIT, "varVariant", 12);
    constant(adapter, UNIT, "varUnknown", 13);
    constant(adapter, UNIT, "varByte", 17);
    constant(adapter, UNIT, "varStrArg", 72);
    constant(adapter, UNIT, "varString", DELPHI_VAR_STRING);
    constant(adapter, UNIT, "varAny", DELPHI_VAR_ANY);
    constant(adapter, UNIT, "varTypeMask", DELPHI_VAR_TYPE_MASK);
    constant(adapter, UNIT, "varArray", DELPHI_VAR_ARRAY);
    constant(adapter, UNIT, "varByRef", 0x4000);
    constant(adapter, UNIT, "varShortInt", 16);
    constant(adapter, UNIT, "varWord", 18);
    constant(adapter, UNIT, "varLongWord", 19);
    constant(adapter, UNIT, "varInt64", 20);
}

/// A `System` function, named apart from the test helper of module scope.
fn function_(
    adapter: &mut Adapter,
    identifier: &str,
    params: &[xedit_script::values::ParamType],
    func: impl Fn(&mut Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    super::function(adapter, UNIT, identifier, params, func);
}

/// The Delphi variant type code of a value (`VarType(V)`).
fn delphi_var_type(value: &Value) -> i32 {
    match value {
        Value::Empty => 0,
        Value::Null => 1,
        Value::Smallint(_) => 2,
        Value::Integer(_) => 3,
        Value::Byte(_) => 17,
        Value::Int64(_) => 20,
        Value::Double(_) => 5,
        Value::Date(_) => 7,
        // The interpreter's strings are Unicode strings, so `VarType` of
        // one is `varUString` (`$102`) -- the oracle's answer for
        // `VarType('x')` (probe of 2026-10-10), and the same answer the
        // `Variants` unit's own `VarType` of `xejviScriptAdapterMisc.pas`
        // gives. The `varString`/`varUString` *constants* keep Delphi's
        // codes (`$100`/`$102`).
        Value::Str(_) => DELPHI_VAR_USTRING,
        Value::Bool(_) => 11,
        Value::Set(_) => DELPHI_VAR_SET,
        Value::Pointer(_) => DELPHI_VAR_POINTER,
        Value::Object(_) => DELPHI_VAR_OBJECT,
        Value::Class(_) => DELPHI_VAR_CLASS,
        Value::Record(_) => DELPHI_VAR_RECORD,
        Value::Array(_) => DELPHI_VAR_ARRAY,
        // `VarArrayCreate` builds a `varVariant` array: `varArray or
        // varVariant`.
        Value::VariantArray(_) => 0x2000 | 12,
    }
}

/// `VarAsType(V, T)`: `T` is the numeric Delphi type code.
fn var_as_type(value: &Value, code: i32) -> Result<Value, ValueError> {
    let typ = match code {
        0 => VarType::Empty,
        1 => VarType::Null,
        2 => VarType::Smallint,
        3 => VarType::Integer,
        4..=6 => VarType::Double,
        7 => VarType::Date,
        11 => VarType::Bool,
        12 => VarType::Variant,
        16..=20 => VarType::Int64,
        DELPHI_VAR_STRING => VarType::Str,
        DELPHI_VAR_OBJECT => VarType::Object,
        DELPHI_VAR_CLASS => VarType::Class,
        DELPHI_VAR_RECORD => VarType::Record,
        DELPHI_VAR_POINTER => VarType::Pointer,
        DELPHI_VAR_SET => VarType::Set,
        DELPHI_VAR_ARRAY => VarType::Array,
        _ => return Err(ValueError::Variant),
    };
    xedit_script::values::var_as_type(value, typ)
}

/// `VarArrayCreate`: the bounds come as a one-dimensional array of pairs
/// (`[0, 5]` parses to a native variant array), the type as its Delphi code.
fn var_array_create(args: &Args) -> Result<Value, ValueError> {
    let values: Vec<Value> = match arg(args, 0) {
        Value::VariantArray(array) => array.borrow().values.clone(),
        Value::Array(array) => array.borrow().values.clone(),
        _ => return Err(ValueError::Variant),
    };
    let mut bounds: Vec<i64> = Vec::with_capacity(values.len());
    for value in &values {
        bounds.push(i64::from(to_i32(value)?));
    }
    if !bounds.len().is_multiple_of(2) {
        return Err(ValueError::External(
            "The size of bounds array must be even!".to_owned(),
        ));
    }
    let typ = match arg_i32(args, 1)? {
        0 => VarType::Empty,
        1 => VarType::Null,
        2 => VarType::Smallint,
        3 => VarType::Integer,
        4..=6 => VarType::Double,
        7 => VarType::Date,
        11 => VarType::Bool,
        12 => VarType::Variant,
        16..=20 => VarType::Int64,
        DELPHI_VAR_STRING => VarType::Str,
        _ => VarType::Variant,
    };
    // One dimension as the port models it; a multi-dimensional request keeps
    // its total element count with the first dimension's bounds (the corpus
    // never creates one, and `VarArrayDimCount` reports 1).
    let low = bounds.first().copied().unwrap_or(0);
    let count: i64 = bounds.chunks(2).map(|pair| (pair[1] - pair[0] + 1).max(0)).product();
    let values = (0..count)
        .map(|_| match typ {
            VarType::Empty | VarType::Variant | VarType::Null => Value::Empty,
            typ => zero_value(typ),
        })
        .collect();
    Ok(Value::VariantArray(std::rc::Rc::new(std::cell::RefCell::new(
        VariantArray { low, values },
    ))))
}

/// The native variant array of an argument, or the `EVariantError` a
/// `VarArray*` routine raises for anything else.
fn variant_array(value: &Value) -> Result<&std::rc::Rc<std::cell::RefCell<VariantArray>>, ValueError> {
    match value {
        Value::VariantArray(array) => Ok(array),
        _ => Err(ValueError::Variant),
    }
}

/// `VarArrayDimCount` of the port's arrays: always one.
fn var_array_dims(value: &Value) -> Result<i64, ValueError> {
    match value {
        Value::VariantArray(_) => Ok(1),
        _ => Err(ValueError::Variant),
    }
}

/// The interpreter's own array of an argument (`PJvInterpreterArrayRec`).
fn interpreter_array(value: &Value) -> Result<&std::rc::Rc<std::cell::RefCell<ArrayInstance>>, ValueError> {
    match value {
        Value::Array(array) => Ok(array),
        _ => Err(ValueError::Variant),
    }
}

/// `JvInterpreter_Length` (`:371`): a native variant array, then the
/// interpreter's array, then Delphi's `Length` of a string.
fn length(value: &Value) -> Result<usize, ValueError> {
    match value {
        Value::VariantArray(array) => {
            let array = array.borrow();
            check_one_dimensional(1)?;
            Ok(array.values.len())
        }
        Value::Array(array) => {
            let array = array.borrow();
            check_one_dimensional(array.begin.len())?;
            Ok(array.values.len())
        }
        Value::Str(bytes) => Ok(bytes.len()),
        _ => Err(ValueError::Variant),
    }
}

/// `RsESorryForOneDimensionalArraysOnly`, raised as an `EJVCLException` by
/// the array helpers.
fn check_one_dimensional(dimensions: usize) -> Result<(), ValueError> {
    if dimensions > 1 {
        return Err(ValueError::External(
            RS_SORRY_FOR_ONE_DIMENSIONAL_ARRAYS_ONLY.to_owned(),
        ));
    }
    Ok(())
}

/// `JvInterpreter_SetLength` (`:500`): a string grows with zero bytes (the
/// RTL's `SetLength(string, N)` leaves the new characters zeroed), an
/// interpreter array through `JvInterpreterArraySetLength`. The returned
/// value is what the caller stores back into the variable's slot (for an
/// array the shared record is already mutated).
fn set_length(value: Value, size: i32) -> Result<Value, ValueError> {
    let size = size.max(0) as usize;
    match value {
        Value::Array(ref array) => {
            let mut array = array.borrow_mut();
            check_one_dimensional(array.begin.len())?;
            let old = array.values.len();
            if old > size {
                array.values.truncate(size);
                array.end[0] -= (old - size) as i64;
            } else if old < size {
                for _ in old..size {
                    let slot = match array.item_type {
                        VarType::Empty => Value::Null,
                        typ => zero_value(typ),
                    };
                    array.values.push(slot);
                }
                array.end[0] += (size - old) as i64;
            }
            drop(array);
            Ok(value)
        }
        // Anything else takes the string arm upstream, which rewrites the
        // variant as a string of the new length.
        Value::Str(mut bytes) => {
            bytes.resize(size, 0);
            Ok(Value::Str(bytes))
        }
        Value::Empty | Value::Null => Ok(Value::Str(vec![0; size])),
        _ => Err(ValueError::interp(IE_TYPE_MISMATCH)),
    }
}

/// The `High`/`Low` handlers (`:510`, `:524`): a native variant array
/// answers `VarArrayLowBound` for *both* (the `High` copy-paste bug), the
/// interpreter's array its `BeginPos[0]`/`EndPos[0]`.
fn high_or_low(args: &Args, high: bool) -> Result<i64, ValueError> {
    match arg(args, 0) {
        Value::VariantArray(array) => {
            let array = array.borrow();
            check_one_dimensional(1)?;
            // UPSTREAM-QUIRK: `High` reads `VarArrayLowBound` too (`:516`),
            // so both bounds report the low bound.
            let _ = high;
            Ok(array.low)
        }
        Value::Array(array) => {
            let array = array.borrow();
            check_one_dimensional(array.begin.len())?;
            Ok(if high { array.end[0] } else { array.begin[0] })
        }
        _ => Err(ValueError::Variant),
    }
}

/// Delphi `Copy(S, Index, Count)`: the substring from `Index` (1-based), with
/// an `Index` below 1 clamping to 1 and reducing `Count` by the difference
/// (the RTL's `_LStrCopy`), and a range past the end shortening the result.
fn copy_str(source: &[u8], index: i32, count: i32) -> Vec<u8> {
    // `Copy(S, 0, N)` is `Copy(S, 1, N)`: the RTL clamps the index to 1 and
    // keeps the count (the oracle's `Copy('abcdef', 0, 3)` is `abc`, as is
    // `Copy('abcdef', -1, 3)`; the parity-23 probe `edid-quick-init.pas`).
    let index = index.max(1);
    if count <= 0 || index as usize > source.len() {
        return Vec::new();
    }
    let start = index as usize - 1;
    let end = (start + count as usize).min(source.len());
    source[start..end].to_vec()
}

/// Delphi `Delete(S, Index, Count)` with the RTL's clamping.
fn delphi_delete(text: &mut Vec<u8>, index: i32, count: i32) {
    if count <= 0 || index as usize > text.len() {
        return;
    }
    let start = (index.max(1) - 1) as usize;
    let end = (start + count as usize).min(text.len());
    text.drain(start..end);
}

/// Delphi `Insert(Source, S, Index)` with the RTL's clamping.
fn delphi_insert(source: &[u8], text: &mut Vec<u8>, index: i32) {
    let at = (index.max(1) - 1) as usize;
    let at = at.min(text.len());
    text.splice(at..at, source.iter().copied());
}

/// Delphi `Pos`: the 1-based position of `needle` in `haystack`, 0 when it
/// does not occur (an empty needle is 1).
fn pos(needle: &[u8], haystack: &[u8]) -> usize {
    if needle.is_empty() {
        return 1;
    }
    if needle.len() > haystack.len() {
        return 0;
    }
    match haystack.windows(needle.len()).position(|window| window == needle) {
        Some(index) => index + 1,
        None => 0,
    }
}

/// The zero value of a typed slot (the interpreter arrays of
/// `crate::script_rtl` mirror `values::zero_value`, which is private).
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
        VarType::Pointer => Value::Pointer(0),
        VarType::Object | VarType::Class | VarType::Null => Value::Null,
        VarType::Empty | VarType::Variant | VarType::Record | VarType::Array => Value::Null,
    }
}
