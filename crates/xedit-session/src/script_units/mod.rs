// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterElement.pas,
// xejviScriptAdapterRecord.pas, xejviScriptAdapterFile.pas,
// xejviScriptAdapterContainer.pas and xejviScriptAdapterResource.pas

//! The registrations of the five element-level adapter units of phase 6 step
//! 5: `xejviScriptAdapterElement.pas` (the `IwbElement` functions),
//! `xejviScriptAdapterRecord.pas` (`IwbMainRecord` and `IwbGroupRecord`),
//! `xejviScriptAdapterFile.pas` (`IwbFile`), `xejviScriptAdapterContainer.pas`
//! (`IwbContainer`) and `xejviScriptAdapterResource.pas` (the resource
//! handlers, the asset helpers, the hashes and the grid-cell maths).
//!
//! Every function of the five units is registered with the upstream name, the
//! upstream parameter list and the upstream value behaviour; the ones whose
//! engine work belongs to a later phase-6 step (the RTTI form surface of the
//! VCL shim, `TwbDefs`' assign templates, the element states the port does
//! not model) are noted in `docs/PLAN.md` under step 5. An element argument
//! is the [`ScriptElement`](crate::script::ScriptElement) value convention of
//! step 4.
//!
//! Shared pieces of the same five units the other adapter steps need:
//! [`record_def`] (the `TwbVector`/`TwbGridCell` record types the record,
//! resource and misc units read and write) and [`string_list_class`] (the
//! `TStringList` value the `TStrings` parameters take).

use std::rc::Rc;

use xedit_core::interface::{ElementRef, ElementType, FileRef, MainRecordRef, Signature};
use xedit_script::adapter::{Adapter, Args};
use xedit_script::error::{IE_INTEGER_REQUIRED, IE_NOT_ENOUGH_PARAMS, IE_TOO_MANY_PARAMS, IE_TYPE_MISMATCH};
use xedit_script::strings::ScriptStringList;
use xedit_script::values::{
    ClassDef, ParamType, RecordDef, RecordField, ScriptObject, Value, ValueError, VarType, to_bool, to_i64, to_str,
};

use crate::script::{ScriptRunState, signature_text};

pub(crate) mod container;
pub(crate) mod element;
pub(crate) mod file;
pub(crate) mod record;
pub(crate) mod resource;

/// Registers the five units over the run's element values.
pub(crate) fn register_units(adapter: &mut Adapter, state: &Rc<std::cell::RefCell<ScriptRunState>>) {
    let strings = string_list_class(adapter);
    let lists = list_class(adapter);
    element::register(adapter, state, &strings);
    record::register(adapter, state);
    file::register(adapter, state, &strings);
    container::register(adapter, state);
    resource::register(adapter, state, &strings, &lists);
}

/// Registers one `cUnit` function with the upstream name and signature
/// (every `AddFunction` of the five units has `varEmpty` as its result type).
pub(crate) fn add(
    adapter: &mut Adapter,
    name: &str,
    param_count: i32,
    params: Vec<ParamType>,
    func: impl Fn(&mut Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    adapter.add_function(C_UNIT, name, Rc::new(func), param_count, params, VarType::Empty);
}

/// One `varEmpty` parameter (`[varEmpty]`).
pub(crate) fn value_param() -> Vec<ParamType> {
    vec![ParamType::value(VarType::Empty)]
}

/// `[varEmpty, varEmpty]`, ...
pub(crate) fn params(types: &[VarType]) -> Vec<ParamType> {
    types.iter().copied().map(ParamType::value).collect()
}

//=== argument helpers (the `Supports` casts of the Pascal) ==================

/// The unit the adapter units register the shared functions under (`cUnit`
/// of `xejviScriptAdapter*.pas`, `'Dummy'` upstream): a name with no file,
/// which `uses` never resolves, so the functions are callable from any
/// script.
pub(crate) const C_UNIT: &str = "";

/// `Args.Values[i]` as an element, `None` for another kind of value.
pub(crate) fn element_at(args: &Args, index: usize) -> Option<ElementRef> {
    args.values.get(index).and_then(ScriptRunState::element_of)
}

/// `Supports(IInterface(Args.Values[i]), IwbMainRecord, ...)`.
pub(crate) fn main_record_at(args: &Args, index: usize) -> Option<MainRecordRef> {
    element_at(args, index)?.into_main_record()
}

/// The implementation of a file reference, for the calls that need the
/// `FileImpl` API rather than the `File` interface.
pub(crate) fn file_impl_of(file: &FileRef) -> Option<std::sync::Arc<xedit_core::implementation::FileImpl>> {
    file.as_element_impl().and_then(|element| element.file_impl())
}

/// `Supports(IInterface(Args.Values[i]), IwbFile, ...)`.
pub(crate) fn file_at(args: &Args, index: usize) -> Option<FileRef> {
    let element = element_at(args, index)?;
    if element.get_element_type() != ElementType::etFile {
        return None;
    }
    element.get_file()
}

/// `string(Args.Values[i])`: the variant as an `AnsiString`.
pub(crate) fn string_at(args: &Args, index: usize) -> String {
    args.values
        .get(index)
        .and_then(|value| to_str(value).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// `Integer(Args.Values[i])`; the caller reports the cast failure the way the
/// upstream code does (a raised `EVariantError` becomes the interpreter's
/// type mismatch).
pub(crate) fn integer_at(args: &Args, index: usize) -> Result<i64, ValueError> {
    args.values
        .get(index)
        .map(to_i64)
        .unwrap_or(Ok(0))
        .map_err(|_| ValueError::interp(IE_INTEGER_REQUIRED))
}

/// `Boolean(Args.Values[i])`.
pub(crate) fn boolean_at(args: &Args, index: usize) -> Result<bool, ValueError> {
    args.values
        .get(index)
        .map(to_bool)
        .unwrap_or(Ok(false))
        .map_err(|_| ValueError::interp(IE_TYPE_MISMATCH))
}

/// `StrToSignature(Args.Values[i])`: the four characters of the value, the
/// first four of a longer one (`StrToSignature` pads with `#0` when short).
pub(crate) fn signature_at(args: &Args, index: usize) -> Signature {
    let text = string_at(args, index);
    let bytes = text.as_bytes();
    let mut signature = [0u8; 4];
    for (slot, byte) in signature.iter_mut().zip(bytes) {
        *slot = *byte;
    }
    Signature::new(&signature)
}

/// The `Args.Count` guard of the variable-arity functions
/// (`AddMasterIfMissing`, `AddMastersIfMissing`, `ResourceList`,
/// `CreateHashTES4`): upstream reports `ieNotEnoughParams`/`ieTooManyParams`.
pub(crate) fn check_arg_count(args: &Args, min: usize, max: usize) -> Result<(), ValueError> {
    if args.values.len() < min {
        return Err(ValueError::interp(IE_NOT_ENOUGH_PARAMS));
    }
    if args.values.len() > max {
        return Err(ValueError::interp(IE_TOO_MANY_PARAMS));
    }
    Ok(())
}

//=== the native value of an element ========================================

/// An element's native value (`xedit_core`'s `Variant`) as the script value
/// the interpreter moves around. A byte array (an `etByteArray` element's
/// native value) becomes a `varString` of the bytes, the shape the port's
/// JSON dump uses for it too.
pub(crate) fn native_to_value(native: xedit_core::interface::misc::Variant) -> Value {
    use xedit_core::interface::misc::Variant;
    match native {
        Variant::Empty => Value::Null,
        Variant::Bool(value) => Value::Bool(value),
        Variant::Int(value) => int_value(value),
        Variant::UInt(value) => u_int_value(value),
        Variant::Float(value) => Value::Double(value),
        Variant::Str(value) => Value::Str(value.into_bytes()),
        Variant::Bytes(bytes) => Value::Str(bytes),
    }
}

fn int_value(value: i64) -> Value {
    if i32::try_from(value).is_ok() {
        Value::Integer(value as i32)
    } else {
        Value::Int64(value)
    }
}

fn u_int_value(value: u64) -> Value {
    if i32::try_from(value).is_ok() {
        Value::Integer(value as i32)
    } else {
        Value::Int64(value as i64)
    }
}

/// A script value as an element's native value (`SetNativeValue`): the cast
/// of a variant the Pascal does, where a value the port cannot cast is the
/// interpreter's type mismatch.
pub(crate) fn value_to_native(value: &Value) -> Result<xedit_core::interface::misc::Variant, ValueError> {
    use xedit_core::interface::misc::Variant;
    Ok(match value {
        Value::Empty | Value::Null => Variant::Empty,
        Value::Bool(value) => Variant::Bool(*value),
        Value::Smallint(value) => Variant::Int(i64::from(*value)),
        Value::Integer(value) => Variant::Int(i64::from(*value)),
        Value::Byte(value) => Variant::UInt(u64::from(*value)),
        Value::Int64(value) => Variant::Int(*value),
        Value::Double(value) | Value::Date(value) => Variant::Float(*value),
        Value::Str(bytes) => Variant::Str(String::from_utf8_lossy(bytes).into_owned()),
        Value::Set(value) => Variant::Int(i64::from(*value)),
        _ => return Err(ValueError::interp(IE_TYPE_MISMATCH)),
    })
}

//=== the `TStrings` parameter ===============================================

/// The `TStrings(V2O(...))` cast of a parameter: the string list a value
/// carries, `None` for a value of another class (upstream's unchecked cast
/// has no defined behaviour there, the port leaves the function a no-op).
pub(crate) fn strings_at(args: &Args, index: usize) -> Option<Rc<std::cell::RefCell<dyn ScriptObject>>> {
    let Value::Object(object) = args.values.get(index)? else {
        return None;
    };
    object.borrow().string_list()?;
    Some(object.clone())
}

/// Appends the lines to the string list of `Args.Values[index]`
/// (`lStrings.Add`), doing nothing when the value is not a `TStrings`.
pub(crate) fn strings_add(args: &Args, index: usize, lines: Vec<Vec<u8>>) {
    let Some(object) = strings_at(args, index) else {
        return;
    };
    if let Some(list) = object.borrow_mut().string_list_mut() {
        for line in lines {
            list.add(line);
        }
    }
}

/// The `TList(V2O(...))` cast of a parameter (`wbGetSiblingRecords`,
/// `wbFindREFRsByBase`): the list a value carries.
pub(crate) fn list_at(args: &Args, index: usize) -> Option<Rc<std::cell::RefCell<dyn ScriptObject>>> {
    let Value::Object(object) = args.values.get(index)? else {
        return None;
    };
    object.borrow().list()?;
    Some(object.clone())
}

/// Appends an element to the `TList` of `Args.Values[index]`.
pub(crate) fn list_add(args: &Args, index: usize, value: Value) {
    let Some(Value::Object(object)) = args.values.get(index) else {
        return;
    };
    if let Some(list) = object.borrow_mut().list_mut() {
        list.add(value);
    }
}

//=== the `TList` class ======================================================

/// The `TList` class of the `Classes` unit as far as `wbGetSiblingRecords`
/// and `wbFindREFRsByBase` need it: `Create`, `Add`, `Count`, `Get(i)`,
/// `Clear` and `Free`. A later step that ports the JVCL `Classes` unit
/// extends this class instead of registering a second one.
pub(crate) fn list_class(adapter: &mut Adapter) -> Rc<ClassDef> {
    match adapter.find_class(b"TList") {
        Some(class) => class,
        None => {
            let class = adapter.add_class("Classes", "TList", None);
            {
                let class = class.clone();
                adapter.add_get(
                    class.clone(),
                    "Create",
                    Rc::new(move |value: &mut Value, _args: &mut Args| {
                        *value = xedit_script::lists::list_value(class.clone());
                        Ok(())
                    }),
                    0,
                    value_param(),
                    VarType::Empty,
                );
            }
            let instance = value_param;
            adapter.add_get(
                class.clone(),
                "Add",
                Rc::new(|value: &mut Value, args: &mut Args| {
                    *value = Value::Integer(0);
                    if let Some(Value::Object(object)) = &args.obj
                        && let Some(list) = object.borrow_mut().list_mut()
                    {
                        let item = args.values.first().cloned().unwrap_or(Value::Empty);
                        *value = Value::Integer(list.add(item));
                    }
                    Ok(())
                }),
                1,
                instance(),
                VarType::Empty,
            );
            adapter.add_get(
                class.clone(),
                "Count",
                Rc::new(|value: &mut Value, args: &mut Args| {
                    *value = Value::Integer(0);
                    if let Some(Value::Object(object)) = &args.obj
                        && let Some(list) = object.borrow_mut().list()
                    {
                        *value = Value::Integer(list.count());
                    }
                    Ok(())
                }),
                0,
                instance(),
                VarType::Empty,
            );
            // `lst[i]`: the indexed get of the class (the port's dispatch
            // calls `Get` with the index).
            adapter.add_get(
                class.clone(),
                "Get",
                Rc::new(|value: &mut Value, args: &mut Args| {
                    *value = Value::Null;
                    if let Some(Value::Object(object)) = &args.obj
                        && let Some(list) = object.borrow_mut().list()
                    {
                        let index = integer_at(args, 0)? as i32;
                        if let Some(item) = list.get(index) {
                            *value = item.clone();
                        }
                    }
                    Ok(())
                }),
                1,
                instance(),
                VarType::Empty,
            );
            adapter.add_get(
                class.clone(),
                "Clear",
                Rc::new(|_value: &mut Value, args: &mut Args| {
                    if let Some(Value::Object(object)) = &args.obj
                        && let Some(list) = object.borrow_mut().list_mut()
                    {
                        list.clear();
                    }
                    Ok(())
                }),
                0,
                instance(),
                VarType::Empty,
            );
            class
        }
    }
}

//=== the shared record types ===============================================

/// `cTwbVector`/`cTwbGridCell`: the record types `xejviScriptAdapter.pas`
/// registers once and the record, resource and misc units read and write
/// (`R2V(cTwbVector, Rec)`). Registered here so a run of any of those units
/// finds them; `Adapter::add_rec` returns the one already registered.
///
/// The upstream fields are declared `varSingle`/`varInteger`
/// (`RFD('x', 0, varSingle)`); the port has no `varSingle` and reads a
/// single-precision field back as a double, so the fields are `Double` there
/// and `Integer` for the grid cell, and a `Single` write rounds the value to
/// `f32` at the point the value is read (`vector_of`).
pub(crate) fn record_def(adapter: &mut Adapter, name: &str) -> Rc<RecordDef> {
    match name {
        "TwbVector" => adapter.add_rec(
            "Dummy",
            "TwbVector",
            12,
            vec![
                field("x", 0, VarType::Double),
                field("y", 4, VarType::Double),
                field("z", 8, VarType::Double),
            ],
        ),
        _ => adapter.add_rec(
            "Dummy",
            "TwbGridCell",
            8,
            vec![field("x", 0, VarType::Integer), field("y", 4, VarType::Integer)],
        ),
    }
}

fn field(name: &str, offset: i64, typ: VarType) -> RecordField {
    RecordField {
        name: name.to_owned(),
        offset,
        typ,
        data_type: None,
    }
}

/// A `TwbVector` value (`wbVector2Var`).
pub(crate) fn vector_value(def: Rc<RecordDef>, x: f32, y: f32, z: f32) -> Value {
    record_value(
        def,
        vec![
            Value::Double(f64::from(x)),
            Value::Double(f64::from(y)),
            Value::Double(f64::from(z)),
        ],
    )
}

/// A `TwbGridCell` value (`wbGridCell2Var`).
pub(crate) fn grid_cell_value(def: Rc<RecordDef>, x: i32, y: i32) -> Value {
    record_value(def, vec![Value::Integer(x), Value::Integer(y)])
}

fn record_value(def: Rc<RecordDef>, values: Vec<Value>) -> Value {
    let mut record = xedit_script::values::RecordInstance::new(def);
    record.values = values;
    Value::Record(Rc::new(std::cell::RefCell::new(record)))
}

/// `V2R(Value)^` for a vector: the three singles of the record value.
pub(crate) fn vector_of(value: &Value) -> Option<(f32, f32, f32)> {
    let Value::Record(record) = value else {
        return None;
    };
    let record = record.borrow();
    if record.def.fields.len() < 3 {
        return None;
    }
    let component = |index: usize| {
        xedit_script::values::to_f64(&record.values[index])
            .ok()
            .map(|value| value as f32)
    };
    Some((component(0)?, component(1)?, component(2)?))
}

/// `V2R(Value)^` for a grid cell.
pub(crate) fn grid_cell_of(value: &Value) -> Option<(i32, i32)> {
    let Value::Record(record) = value else {
        return None;
    };
    let record = record.borrow();
    if record.def.fields.len() < 2 {
        return None;
    }
    let component = |index: usize| to_i64(&record.values[index]).ok().map(|value| value as i32);
    Some((component(0)?, component(1)?))
}

//=== the `TStringList` class ===============================================

/// The `TStringList` class of the `Classes` unit as far as the `TStrings`
/// parameters of these units need it, with `Create`, `Add`, `AddObject`,
/// `Count`, `Strings[]`, `Text`, `Clear`, `Delete`, `Insert`, `IndexOf` and
/// the `Sorted`/`Duplicates` pair. A later step that ports the JVCL `Classes`
/// unit adds its remaining members to this class (`Adapter::find_class`)
/// instead of registering a second one.
///
/// `TwbFastStringList` (`wbInterface.pas:5046`, `class(TStringList)` with
/// `UseLocale := False` and a `Clear(aFreeObjects)`) is registered by
/// `xejviScriptAdapterResource.pas` and is created here as a child of the
/// same class with the same members plus its `Create`; its `UseLocale` and
/// `CreateSorted`/`Clear(aFreeObjects)` members are owed to the `Classes`
/// step.
pub(crate) fn string_list_class(adapter: &mut Adapter) -> Rc<ClassDef> {
    let class = match adapter.find_class(b"TStringList") {
        Some(class) => class,
        None => {
            let class = adapter.add_class("Classes", "TStringList", None);
            register_string_list_members(adapter, &class);
            class
        }
    };
    if adapter.find_class(b"TwbFastStringList").is_none() {
        let fast = adapter.add_class(C_UNIT, "TwbFastStringList", Some(class.clone()));
        register_string_list_create(adapter, &fast);
    }
    class
}

/// `TwbFastStringList_Create` (`xejviScriptAdapterResource.pas:119`):
/// `AddGet(TwbFastStringList, 'Create', ..., 0, [varEmpty], varEmpty)`.
/// The object is the `Classes` unit's `StringListObject`, so a fast list
/// answers the same surface as `TStringList.Create` (a script's `sl.Clear`
/// etc.; the two models became one with the `Classes` step).
fn register_string_list_create(adapter: &mut Adapter, class: &Rc<ClassDef>) {
    let class = class.clone();
    adapter.add_get(
        class.clone(),
        "Create",
        Rc::new(move |value: &mut Value, _args: &mut Args| {
            *value = Value::Object(Rc::new(std::cell::RefCell::new(
                crate::script_strings::StringListObject::new(class.clone()),
            )));
            Ok(())
        }),
        0,
        value_param(),
        VarType::Empty,
    );
}

fn register_string_list_members(adapter: &mut Adapter, class: &Rc<ClassDef>) {
    let instance = || vec![ParamType::value(VarType::Empty)];
    let object_get = |adapter: &mut Adapter,
                      name: &str,
                      count: i32,
                      params: Vec<ParamType>,
                      func: xedit_script::adapter::AdapterGetFn| {
        adapter.add_get(class.clone(), name, func, count, params, VarType::Empty);
    };
    register_string_list_create(adapter, class);
    object_get(
        adapter,
        "Add",
        1,
        instance(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Integer(0);
            if let Some(object) = &args.obj
                && let Value::Object(object) = object
                && let Some(list) = object.borrow_mut().string_list_mut()
            {
                let text = string_at(args, 0);
                *value = Value::Integer(list.add(text.into_bytes()));
            }
            Ok(())
        }),
    );
    object_get(
        adapter,
        "AddObject",
        2,
        instance(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Integer(0);
            if let Some(Value::Object(object)) = &args.obj
                && let Some(list) = object.borrow_mut().string_list_mut()
            {
                let text = string_at(args, 0);
                let object = args.values.get(1).cloned().unwrap_or(Value::Empty);
                *value = Value::Integer(list.add_object(text.into_bytes(), object));
            }
            Ok(())
        }),
    );
    object_get(
        adapter,
        "Clear",
        0,
        instance(),
        Rc::new(|_value: &mut Value, args: &mut Args| {
            if let Some(Value::Object(object)) = &args.obj
                && let Some(list) = object.borrow_mut().string_list_mut()
            {
                list.clear();
            }
            Ok(())
        }),
    );
    object_get(
        adapter,
        "Delete",
        1,
        instance(),
        Rc::new(|_value: &mut Value, args: &mut Args| {
            if let Some(Value::Object(object)) = &args.obj
                && let Some(list) = object.borrow_mut().string_list_mut()
            {
                list.delete(integer_at(args, 0)? as i32);
            }
            Ok(())
        }),
    );
    object_get(
        adapter,
        "IndexOf",
        1,
        instance(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Integer(-1);
            if let Some(Value::Object(object)) = &args.obj
                && let Some(list) = object.borrow_mut().string_list()
            {
                *value = Value::Integer(list.index_of(string_at(args, 0).as_bytes()));
            }
            Ok(())
        }),
    );
    // `TStrings.Count`, the `IwbFastStringList`-style `Get(Index)` and
    // `TStrings.Text` are the properties the adapter functions and the
    // corpus scripts read; the JVCL `_Classes.pas` registers the rest.
    object_get(
        adapter,
        "Count",
        0,
        instance(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Integer(0);
            if let Some(Value::Object(object)) = &args.obj
                && let Some(list) = object.borrow_mut().string_list()
            {
                *value = Value::Integer(list.lines().len() as i32);
            }
            Ok(())
        }),
    );
    // `TStrings.Strings` is an *indexed* property upstream: `_Classes.pas`
    // registers `AddIGet`/`AddISet` (`JvInterpreter_Classes.pas:1268-1269`)
    // with the `AddIDGet`/`AddIDSet` default (`:1270-1271`), so
    // `sl.Strings[i]` goes through the indexed dispatch
    // (`JvInterpreter.pas:4462-4475`) and `sl[i]` through `GetID`/`SetID`
    // (`:4875-4889`); the getter reads the index from `Args.Values[0]`, the
    // setter takes its value from the `Value` parameter
    // (`TStrings_Read_Strings`/`TStrings_Write_Strings`, `:583`/`:590`).
    // There is no plain get for the name, as upstream: the call form
    // `sl.Strings(0)` is not registered either.
    let strings_get: xedit_script::adapter::AdapterGetFn = Rc::new(|value: &mut Value, args: &mut Args| {
        *value = Value::Str(Vec::new());
        if let Some(Value::Object(object)) = &args.obj
            && let Some(list) = object.borrow_mut().string_list()
        {
            let index = integer_at(args, 0)? as i32;
            if let Some(line) = usize::try_from(index).ok().and_then(|index| list.lines().get(index)) {
                *value = Value::Str(line.clone());
            }
        }
        Ok(())
    });
    adapter.add_iget(
        class.clone(),
        "Strings",
        strings_get.clone(),
        1,
        instance(),
        VarType::Empty,
    );
    adapter.add_id_get(class.clone(), strings_get, 1, instance(), VarType::Empty);
    let strings_set: xedit_script::adapter::AdapterSetFn = Rc::new(|value: &Value, args: &mut Args| {
        if let Some(Value::Object(object)) = &args.obj
            && let Some(list) = object.borrow_mut().string_list_mut()
        {
            let index = integer_at(args, 0)? as i32;
            let text = to_str(value).unwrap_or_default();
            list.set_line(index, text);
        }
        Ok(())
    });
    adapter.add_iset(class.clone(), "Strings", strings_set.clone(), 1, instance());
    adapter.add_id_set(class.clone(), strings_set, 1, instance());
    object_get(
        adapter,
        "Text",
        0,
        instance(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Str(Vec::new());
            if let Some(Value::Object(object)) = &args.obj
                && let Some(list) = object.borrow_mut().string_list()
            {
                let mut text = Vec::new();
                for line in list.lines() {
                    text.extend_from_slice(line);
                    text.extend_from_slice(b"\r\n");
                }
                *value = Value::Str(text);
            }
            Ok(())
        }),
    );
    object_get(
        adapter,
        "Sorted",
        0,
        instance(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Bool(false);
            if let Some(Value::Object(object)) = &args.obj
                && let Some(list) = object.borrow_mut().string_list()
            {
                *value = Value::Bool(list.sorted());
            }
            Ok(())
        }),
    );
    adapter.add_set(
        class.clone(),
        "Sorted",
        Rc::new(|_value: &Value, args: &mut Args| {
            if let Some(Value::Object(object)) = &args.obj
                && let Some(list) = object.borrow_mut().as_any_mut().downcast_mut::<ScriptStringList>()
            {
                list.is_sorted = boolean_at(args, 0)?;
            }
            Ok(())
        }),
        0,
        instance(),
    );
    object_get(
        adapter,
        "Duplicates",
        0,
        instance(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Integer(0);
            if let Some(Value::Object(object)) = &args.obj
                && let Some(list) = object.borrow_mut().string_list()
            {
                *value = Value::Integer(list.duplicates().as_int());
            }
            Ok(())
        }),
    );
    adapter.add_set(
        class.clone(),
        "Duplicates",
        Rc::new(|_value: &Value, args: &mut Args| {
            if let Some(Value::Object(object)) = &args.obj
                && let Some(list) = object.borrow_mut().as_any_mut().downcast_mut::<ScriptStringList>()
            {
                list.dups = xedit_script::strings::Duplicates::from_int(integer_at(args, 0)?);
            }
            Ok(())
        }),
        0,
        instance(),
    );
}

/// The `wbGridCellToGroupLabel`-style `Signature`-to-text of an element
/// (`crate::script`'s, re-exported so the unit modules name one helper).
#[allow(dead_code)]
pub(crate) fn signature_text_of(signature: Signature) -> String {
    signature_text(signature)
}
