// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterMiscJson.pas (the
// JsonDataObjects surface: TJsonBaseObject, TJsonArray and TJsonObject with
// their accessors, the JDO configuration functions and the jdt* constants)
// over the semantics of the vendored library
// (External/JsonDataObjects/Source/JsonDataObjects.pas: TJsonDataValue's
// conversions and IsNull, TJsonArray/TJsonObject's accessors and
// auto-creation, Assign/Clone, the TJsonOutputWriter's compact and readable
// forms, StrToJSONStr's escaping, DoubleToText and the reader's parse).

//! `TJsonObject`/`TJsonArray` for scripts.
//!
//! The dome: every `TJsonObject`/`TJsonArray` a script holds is a shared
//! handle to a [`JsonNode`] (so `Obj.A['x']` and a later write through the
//! returned array see each other's changes, as upstream's pointers do), and
//! child handles alias the same node as their parent's storage.
//!
//! UPSTREAM-QUIRK: a JSON `null` is a `jdtObject` with a nil pointer
//! upstream (`VarTypeToJsonDataType(varNull) = jdtObject`); the port keeps
//! the same `NodeKind::Null` with the `jdtObject` code, so `O.O['null']`
//! reads nil, `O.A['null']` is a cast error and `Types['null']` is
//! `jdtObject`, as the library gives them.
//!
//! Owed to the parity harness (no corpus script reaches them, recorded in
//! the map): `TJsonDataValue.DateTime`'s local-to-UTC conversion (the port
//! formats the value without the system's bias), `LoadFromResource` over
//! `wbContainerHandler` (ported, exercised only by a game-data run), the
//! `jdtUtcDateTime` kind (the adapter registers no constant for it and the
//! port folds it into `jdtDateTime`), and `D[]`'s parsing of a date string
//! (the port's subset of `JSONToDateTime`).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use xedit_core::container_handler::open_resource_data;
use xedit_core::delphi::float_to_str;

use xedit_script::adapter::{Adapter, Args};
use xedit_script::values::{ClassDef, ParamType, ScriptObject, Value, ValueError, VarType};

use crate::script_classes::ScriptClasses;
use crate::script_strings::{EncodingKind, borrow_list_mut, encoding_of, list_of_or_error, receiver, text_arg};

/// The exception classes of `JsonDataObjects` and their texts.
pub const E_JSON_CAST_CLASS: &str = "EJsonCastException";
pub const E_JSON_PARSER_CLASS: &str = "EJsonParserException";
pub const E_JSON_CLASS: &str = "EJsonException";

/// `TJsonDataType` (`JsonDataObjects.pas:286`): `jdtUtcDateTime` holds 7 and
/// the adapter registers no constant for it, so `jdtBool` is 8.
pub const JDT_NONE: i32 = 0;
pub const JDT_STRING: i32 = 1;
pub const JDT_INT: i32 = 2;
pub const JDT_LONG: i32 = 3;
pub const JDT_ULONG: i32 = 4;
pub const JDT_FLOAT: i32 = 5;
pub const JDT_DATETIME: i32 = 6;
pub const JDT_UTC_DATETIME: i32 = 7;
pub const JDT_BOOL: i32 = 8;
pub const JDT_ARRAY: i32 = 9;
pub const JDT_OBJECT: i32 = 10;

/// `TJsonBaseObject.DataTypeNames`.
pub fn data_type_name(data_type: i32) -> &'static str {
    match data_type {
        JDT_NONE => "null",
        JDT_STRING => "String",
        JDT_INT => "Integer",
        JDT_LONG => "Long",
        JDT_ULONG => "ULong",
        JDT_FLOAT => "Float",
        JDT_DATETIME => "DateTime",
        JDT_UTC_DATETIME => "UTC-DateTime",
        JDT_BOOL => "Bool",
        JDT_ARRAY => "Array",
        _ => "Object",
    }
}

fn cast_error(from: &str, into: &str) -> ValueError {
    ValueError::external_class(E_JSON_CAST_CLASS, format!("Cannot cast {from} into {into}"))
}

fn unsupported_variant_type(var_type: i32) -> ValueError {
    ValueError::external_class(E_JSON_CAST_CLASS, format!("VarType {var_type} is not supported"))
}

fn list_index_error(index: i64) -> ValueError {
    // `ListError(@SListIndexError, Index)` raises `EStringListError`.
    ValueError::external_class("EStringListError", format!("List index out of bounds ({index})"))
}

fn parser_error(message: String) -> ValueError {
    ValueError::external_class(E_JSON_PARSER_CLASS, message)
}

//=== The node model ==========================================================

/// One JSON value. `Null` is the library's "null" (upstream a `jdtObject`
/// with a nil pointer).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum NodeKind {
    #[default]
    None,
    Null,
    Str(String),
    Int(i32),
    Long(i64),
    ULong(u64),
    Float(f64),
    DateTime(f64),
    UtcDateTime(f64),
    Bool(bool),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

/// A shared handle to one JSON value.
pub type Node = Rc<RefCell<NodeKind>>;

impl NodeKind {
    /// `TJsonDataValue.Typ`.
    fn data_type(&self) -> i32 {
        match self {
            NodeKind::None => JDT_NONE,
            // `varNull` converts to `jdtObject`: the null is a nil object.
            NodeKind::Null | NodeKind::Object(_) => JDT_OBJECT,
            NodeKind::Str(_) => JDT_STRING,
            NodeKind::Int(_) => JDT_INT,
            NodeKind::Long(_) => JDT_LONG,
            NodeKind::ULong(_) => JDT_ULONG,
            NodeKind::Float(_) => JDT_FLOAT,
            NodeKind::DateTime(_) => JDT_DATETIME,
            NodeKind::UtcDateTime(_) => JDT_UTC_DATETIME,
            NodeKind::Bool(_) => JDT_BOOL,
            NodeKind::Array(_) => JDT_ARRAY,
        }
    }

    /// `TJsonDataValue.IsNull`: a null and an unset value.
    fn is_null(&self) -> bool {
        matches!(self, NodeKind::Null | NodeKind::None)
    }

    fn name(&self) -> &'static str {
        data_type_name(self.data_type())
    }
}

//=== The configuration =======================================================

/// `JsonSerializationConfig` (`JsonDataObjects.pas:881`): the unit's
/// process-wide setting a script writes with `SetJDO*`.
#[derive(Clone, Debug)]
pub struct JsonConfig {
    pub indent_char: char,
    pub line_break: String,
    pub null_converts_to_value_types: bool,
    pub use_utc_time: bool,
    pub escape_all_non_ascii_chars: bool,
}

impl Default for JsonConfig {
    fn default() -> Self {
        JsonConfig {
            indent_char: '\t',
            line_break: "\n".to_owned(),
            null_converts_to_value_types: false,
            use_utc_time: true,
            escape_all_non_ascii_chars: false,
        }
    }
}

/// The unit's `JsonSerializationConfig` variable.
static CONFIG: Mutex<Option<JsonConfig>> = Mutex::new(None);

fn config() -> JsonConfig {
    CONFIG.lock().unwrap().clone().unwrap_or_default()
}

fn set_config(update: impl FnOnce(&mut JsonConfig)) {
    let mut guard = CONFIG.lock().unwrap();
    let mut current = guard.clone().unwrap_or_default();
    update(&mut current);
    *guard = Some(current);
}

/// The shared classes a JSON handle hands to its children.
pub struct JsonDom {
    pub base: Rc<ClassDef>,
    pub array: Rc<ClassDef>,
    pub object: Rc<ClassDef>,
}

/// One `TJsonObject`/`TJsonArray` value.
pub struct JsonValueObject {
    dom: Rc<JsonDom>,
    node: Node,
    class: Rc<ClassDef>,
}

impl ScriptObject for JsonValueObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn node_value(dom: &Rc<JsonDom>, node: Node) -> Value {
    let class = if matches!(&*node.borrow(), NodeKind::Array(_)) {
        dom.array.clone()
    } else {
        dom.object.clone()
    };
    Value::Object(Rc::new(RefCell::new(JsonValueObject {
        dom: dom.clone(),
        node,
        class,
    })))
}

/// The handle behind a value, if it is a JSON object.
fn json_of(value: &Value) -> Option<Rc<RefCell<dyn ScriptObject>>> {
    let object = value.as_object()?;
    if object.borrow().as_any().is::<JsonValueObject>() {
        Some(object.clone())
    } else {
        None
    }
}

fn json_receiver(args: &Args) -> Result<Rc<RefCell<dyn ScriptObject>>, ValueError> {
    match &args.obj {
        Some(object) => json_of(object),
        None => None,
    }
    .ok_or_else(|| cast_error("nil", "TJsonObject"))
}

fn dom_of(object: &Rc<RefCell<dyn ScriptObject>>) -> Rc<JsonDom> {
    object
        .borrow()
        .as_any()
        .downcast_ref::<JsonValueObject>()
        .expect("a JSON object")
        .dom
        .clone()
}

fn node_of(object: &Rc<RefCell<dyn ScriptObject>>) -> Node {
    object
        .borrow()
        .as_any()
        .downcast_ref::<JsonValueObject>()
        .expect("a JSON object")
        .node
        .clone()
}

fn node_of_value(value: &Value) -> Option<Node> {
    let object = json_of(value)?;
    let node = node_of(&object);
    Some(node)
}

//=== Value conversions =======================================================

/// `VarTypeToJsonDataType` (`:1495`): the kind a variant converts to; a
/// type without a mapping is `EJsonCastException('VarType %d is not
/// supported')`. `varNull` is the nil object of the library.
fn data_type_of_value(value: &Value) -> Result<NodeKind, ValueError> {
    match value {
        Value::Null => Ok(NodeKind::Null),
        Value::Str(bytes) => Ok(NodeKind::Str(String::from_utf8_lossy(bytes).into_owned())),
        Value::Smallint(v) => Ok(NodeKind::Int(i32::from(*v))),
        Value::Integer(v) => Ok(NodeKind::Int(*v)),
        Value::Byte(v) => Ok(NodeKind::Int(i32::from(*v))),
        Value::Int64(v) => Ok(NodeKind::Long(*v)),
        Value::Double(v) => Ok(NodeKind::Float(*v)),
        Value::Date(v) => Ok(NodeKind::DateTime(*v)),
        Value::Bool(v) => Ok(NodeKind::Bool(*v)),
        other => Err(unsupported_variant_type(crate::script_misc::delphi_var_type(other))),
    }
}

/// `TJsonDataValue.VariantValue` (`:2228`): the value as a script value. A
/// container is `ErrorUnsupportedVariantType(varArray)`/`varObject` unless
/// it is nil.
fn variant_of_node(kind: &NodeKind, _null_converts: bool) -> Result<Value, ValueError> {
    Ok(match kind {
        NodeKind::None => Value::Empty,
        NodeKind::Null => Value::Null,
        NodeKind::Str(text) => Value::Str(text.clone().into_bytes()),
        NodeKind::Int(v) => Value::Integer(*v),
        NodeKind::Long(v) => Value::Int64(*v),
        // The port has no `varUInt64`; a `ULong` reads as an `Int64` of the
        // same bits.
        NodeKind::ULong(v) => Value::Int64(*v as i64),
        NodeKind::Float(v) | NodeKind::DateTime(v) | NodeKind::UtcDateTime(v) => Value::Double(*v),
        NodeKind::Bool(v) => Value::Bool(*v),
        NodeKind::Array(_) => {
            return Err(unsupported_variant_type(0x2000));
        }
        NodeKind::Object(_) => {
            return Err(unsupported_variant_type(0x1001));
        }
    })
}

/// `TJsonDataValue.Value` (`:2300`): the string reading with the library's
/// auto-casts.
fn string_of_node(kind: &NodeKind, null_converts: bool) -> Result<String, ValueError> {
    match kind {
        NodeKind::None => Ok(String::new()),
        NodeKind::Str(text) => Ok(text.clone()),
        NodeKind::Int(v) => Ok(v.to_string()),
        NodeKind::Long(v) => Ok(v.to_string()),
        NodeKind::ULong(v) => Ok(v.to_string()),
        NodeKind::Float(v) => Ok(float_to_str(*v)),
        NodeKind::DateTime(v) | NodeKind::UtcDateTime(v) => Ok(date_text(*v, config().use_utc_time)),
        NodeKind::Bool(v) => Ok(if *v { "true" } else { "false" }.to_owned()),
        NodeKind::Object(_) => {
            if null_converts {
                Ok(String::new())
            } else {
                Err(cast_error(kind.name(), "String"))
            }
        }
        _ => Err(cast_error(kind.name(), "String")),
    }
}

/// `TJsonDataValue.IntValue`.
fn int_of_node(kind: &NodeKind, null_converts: bool) -> Result<i32, ValueError> {
    match kind {
        NodeKind::None => Ok(0),
        NodeKind::Str(text) => match text.trim().parse::<i32>() {
            Ok(value) => Ok(value),
            Err(_) => str_to_float_or(text).map(|value| value.trunc() as i32),
        },
        NodeKind::Int(v) => Ok(*v),
        NodeKind::Long(v) => Ok(*v as i32),
        NodeKind::ULong(v) => Ok(*v as i32),
        NodeKind::Float(v) | NodeKind::DateTime(v) | NodeKind::UtcDateTime(v) => Ok(v.trunc() as i32),
        NodeKind::Bool(v) => Ok(i32::from(*v)),
        NodeKind::Object(_) => {
            if null_converts {
                Ok(0)
            } else {
                Err(cast_error(kind.name(), "Integer"))
            }
        }
        _ => Err(cast_error(kind.name(), "Integer")),
    }
}

/// `TJsonDataValue.FloatValue`.
fn float_of_node(kind: &NodeKind, null_converts: bool) -> Result<f64, ValueError> {
    match kind {
        NodeKind::None => Ok(0.0),
        NodeKind::Str(text) => str_to_float_or(text),
        NodeKind::Int(v) => Ok(f64::from(*v)),
        NodeKind::Long(v) => Ok(*v as f64),
        NodeKind::ULong(v) => Ok(*v as f64),
        NodeKind::Float(v) | NodeKind::DateTime(v) | NodeKind::UtcDateTime(v) => Ok(*v),
        NodeKind::Bool(v) => Ok(if *v { 1.0 } else { 0.0 }),
        NodeKind::Object(_) => {
            if null_converts {
                Ok(0.0)
            } else {
                Err(cast_error(kind.name(), "Float"))
            }
        }
        _ => Err(cast_error(kind.name(), "Float")),
    }
}

/// `TJsonDataValue.BoolValue`.
fn bool_of_node(kind: &NodeKind, null_converts: bool) -> Result<bool, ValueError> {
    match kind {
        NodeKind::None => Ok(false),
        NodeKind::Str(text) => Ok(text == "true"),
        NodeKind::Int(v) => Ok(*v != 0),
        NodeKind::Long(v) => Ok(*v != 0),
        NodeKind::ULong(v) => Ok(*v != 0),
        NodeKind::Float(v) | NodeKind::DateTime(v) | NodeKind::UtcDateTime(v) => Ok(*v != 0.0),
        NodeKind::Bool(v) => Ok(*v),
        NodeKind::Object(_) => {
            if null_converts {
                Ok(false)
            } else {
                Err(cast_error(kind.name(), "Bool"))
            }
        }
        _ => Err(cast_error(kind.name(), "Bool")),
    }
}

/// `StrToFloat(string(FValue.S), JSONFormatSettings)`.
fn str_to_float_or(text: &str) -> Result<f64, ValueError> {
    use xedit_core::delphi::str_to_float;
    str_to_float(text).ok_or_else(|| {
        ValueError::external_class(
            "EConvertError",
            format!("'{}' is not a valid floating point value", text.trim()),
        )
    })
}

/// `TJsonDataValue.DateTimeValue`.
fn date_of_node(kind: &NodeKind, null_converts: bool) -> Result<f64, ValueError> {
    match kind {
        NodeKind::None => Ok(0.0),
        NodeKind::Str(text) => parse_iso_date(text),
        NodeKind::Int(v) => Ok(f64::from(*v)),
        NodeKind::Long(v) => Ok(*v as f64),
        NodeKind::ULong(v) => Ok(*v as f64),
        NodeKind::Float(v) | NodeKind::DateTime(v) | NodeKind::UtcDateTime(v) => Ok(*v),
        NodeKind::Bool(v) => Ok(if *v { 1.0 } else { 0.0 }),
        NodeKind::Object(_) => {
            if null_converts {
                Ok(0.0)
            } else {
                Err(cast_error(kind.name(), "DateTime"))
            }
        }
        _ => Err(cast_error(kind.name(), "DateTime")),
    }
}

//=== The object and array accessors ==========================================

/// `TJsonObject.FindItem`: the first entry with the name.
fn find_index(entries: &[(String, Node)], name: &str) -> Option<usize> {
    entries.iter().position(|(key, _)| key == name)
}

/// `TJsonObject.GetObjectValue`: nil for an unset or null value, the
/// object otherwise, a cast error for anything else.
fn object_value_of(node: &Node) -> Result<Option<Node>, ValueError> {
    let borrowed = node.borrow();
    match &*borrowed {
        NodeKind::None | NodeKind::Null => Ok(None),
        NodeKind::Object(_) => Ok(Some(node.clone())),
        other => Err(cast_error(other.name(), "Object")),
    }
}

/// `TJsonDataValue.ArrayValue`: nil for an unset value, the array
/// otherwise, a cast error for anything else (a null included).
fn array_value_of(node: &Node) -> Result<Option<Node>, ValueError> {
    let borrowed = node.borrow();
    match &*borrowed {
        NodeKind::None => Ok(None),
        NodeKind::Array(_) => Ok(Some(node.clone())),
        other => Err(cast_error(other.name(), "Array")),
    }
}

/// `TJsonObject.RequireItem`: the existing entry's node, else a new
/// `jdtNone` one appended.
fn require_item(entries: &mut Vec<(String, Node)>, name: &str) -> Node {
    if let Some(index) = find_index(entries, name) {
        return entries[index].1.clone();
    }
    let node: Node = Rc::new(RefCell::new(NodeKind::None));
    entries.push((name.to_owned(), node.clone()));
    node
}

/// The object's entries (`TJsonObject` receiver), as a copy of the handles.
fn object_entries(object: &Rc<RefCell<dyn ScriptObject>>) -> Result<Vec<(String, Node)>, ValueError> {
    let node = node_of(object);
    let borrowed = node.borrow();
    match &*borrowed {
        NodeKind::Object(entries) => Ok(entries.clone()),
        other => Err(cast_error(other.name(), "Object")),
    }
}

/// The array's items (`TJsonArray` receiver).
fn array_items(object: &Rc<RefCell<dyn ScriptObject>>) -> Result<Vec<Node>, ValueError> {
    let node = node_of(object);
    let borrowed = node.borrow();
    match &*borrowed {
        NodeKind::Array(items) => Ok(items.clone()),
        other => Err(cast_error(other.name(), "Array")),
    }
}

fn set_node(object: &Rc<RefCell<dyn ScriptObject>>, kind: NodeKind) {
    *node_of(object).borrow_mut() = kind;
}

//=== Serialization ===========================================================

/// `StrToJSONStr`'s escape of one string, without the surrounding quotes.
fn write_json_string(text: &str, escape_all_non_ascii: bool, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\u{0}'..='\u{7}' | '\u{b}' | '\u{e}'..='\u{1f}' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            ch if escape_all_non_ascii && (ch as u32) >= 0x80 => {
                let mut buffer = [0u16; 2];
                for unit in ch.encode_utf16(&mut buffer) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
            ch => out.push(ch),
        }
    }
}

/// `DoubleToText`: `FloatToText(ffGeneral, 15)` with a decimal separator
/// added when there is none, so the value stays a Float on a re-read.
fn double_text(value: f64) -> String {
    let text = float_to_str(value);
    if text.contains('.') { text } else { format!("{text}.0") }
}

/// `TJsonBaseObject.DateTimeToJSON` and `DateTimeToISO8601`: the ISO 8601
/// text of a `TDateTime`.
///
/// OWED: upstream converts the local time to UTC for `UseUtcTime` through
/// the system's time zone (`TzSpecificLocalTimeToSystemTime`); the port
/// formats the value as it stands (the difference is the time zone's bias).
fn date_text(value: f64, use_utc_time: bool) -> String {
    let _ = use_utc_time;
    let (year, month, day, hour, minute, second, millis) = decode_date_time(value);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis}Z")
}

/// `DecodeDate`/`DecodeTime` of a `TDateTime` (days since 1899-12-30).
fn decode_date_time(value: f64) -> (i64, i64, i64, i64, i64, i64, i64) {
    let days = value.trunc() as i64;
    let fraction = value - value.trunc();
    let millis_total = (fraction * 86_400_000.0).round() as i64;
    let (mut day, mut millis) = (days, millis_total);
    if millis < 0 {
        day -= 1;
        millis += 86_400_000;
    }
    if millis >= 86_400_000 {
        day += 1;
        millis -= 86_400_000;
    }
    // The civil-from-days algorithm; day 0 is 1899-12-30 and 1970-01-01 is
    // day 25569, the algorithm's epoch.
    let day = day - 25_569;
    let z = day + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    let hour = millis / 3_600_000;
    let minute = (millis / 60_000) % 60;
    let second = (millis / 1000) % 60;
    let millis = millis % 1000;
    (year, m, d, hour, minute, second, millis)
}

/// `TJsonBaseObject.JSONToDateTime`'s ISO 8601 subset: `yyyy-mm-dd`, an
/// optional `Thh:nn:ss.zzz` and an optional trailing `Z` or offset.
fn parse_iso_date(text: &str) -> Result<f64, ValueError> {
    let text = text.trim();
    let invalid = || ValueError::external_class("EConvertError", format!("'{text}' is not a valid date and time"));
    let bytes = text.as_bytes();
    if bytes.len() < 10 {
        return Err(invalid());
    }
    let number = |from: usize, to: usize| -> Result<i64, ValueError> {
        text.get(from..to)
            .and_then(|part| part.parse::<i64>().ok())
            .ok_or_else(invalid)
    };
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let mut millis = 0i64;
    if bytes.len() > 11 {
        let hour = number(11, 13)?;
        let minute = number(14, 16)?;
        let second = number(17, 19)?;
        millis = ((hour * 60 + minute) * 60 + second) * 1000;
        if bytes.len() > 20 && bytes[19] == b'.' {
            let rest: String = text[20..].chars().take_while(char::is_ascii_digit).collect();
            if !rest.is_empty() {
                let fraction: f64 = format!("0.{rest}").parse().unwrap_or(0.0);
                millis += (fraction * 1000.0).round() as i64;
            }
        }
    }
    // 1970-01-01 is the 25569th day of the `TDateTime` epoch.
    let days = days_from_civil(year, month, day) + 25_569;
    Ok(days as f64 + millis as f64 / 86_400_000.0)
}

/// The inverse of [`decode_date_time`]'s date part.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe
}

/// The readable form's line structure (`TJsonOutputWriter`): compact output
/// has no whitespace, the readable one puts one entry per line at the
/// `IndentChar` levels and ends with the `LineBreak`.
fn write_node(node: &Node, compact: bool, level: usize, config: &JsonConfig, out: &mut String) {
    let kind = node.borrow().clone();
    match &kind {
        NodeKind::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                if !compact {
                    out.push_str(&config.line_break);
                    push_indent(level + 1, config, out);
                }
                write_node(item, compact, level + 1, config, out);
            }
            if !compact {
                out.push_str(&config.line_break);
                push_indent(level, config, out);
            }
            out.push(']');
        }
        NodeKind::Object(entries) => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (index, (name, value)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                if !compact {
                    out.push_str(&config.line_break);
                    push_indent(level + 1, config, out);
                }
                out.push('"');
                write_json_string(name, config.escape_all_non_ascii_chars, out);
                out.push('"');
                out.push(':');
                if !compact {
                    out.push(' ');
                }
                write_node(value, compact, level + 1, config, out);
            }
            if !compact {
                out.push_str(&config.line_break);
                push_indent(level, config, out);
            }
            out.push('}');
        }
        other => out.push_str(&scalar_text(other, config)),
    }
}

fn push_indent(level: usize, config: &JsonConfig, out: &mut String) {
    for _ in 0..level {
        out.push(config.indent_char);
    }
}

fn scalar_text(kind: &NodeKind, config: &JsonConfig) -> String {
    match kind {
        NodeKind::None | NodeKind::Null => "null".to_owned(),
        NodeKind::Str(text) => {
            let mut out = String::from("\"");
            write_json_string(text, config.escape_all_non_ascii_chars, &mut out);
            out.push('"');
            out
        }
        NodeKind::Int(v) => v.to_string(),
        NodeKind::Long(v) => v.to_string(),
        NodeKind::ULong(v) => v.to_string(),
        NodeKind::Float(v) => double_text(*v),
        NodeKind::DateTime(v) | NodeKind::UtcDateTime(v) => date_text(*v, config.use_utc_time),
        NodeKind::Bool(v) => if *v { "true" } else { "false" }.to_owned(),
        NodeKind::Array(_) | NodeKind::Object(_) => String::new(),
    }
}

/// The complete text of a document: `ToJSON(Compact)`.
fn to_json(node: &Node, compact: bool) -> String {
    let config = config();
    let mut out = String::new();
    write_node(node, compact, 0, &config, &mut out);
    if !compact {
        out.push_str(&config.line_break);
    }
    out
}

//=== Parsing =================================================================

/// `TJsonBaseObject.Parse`'s reader: the whole text is the document; an
/// error is an `EJsonParserException`.
fn parse_json(text: &str) -> Result<NodeKind, ValueError> {
    let mut parser = Parser {
        chars: text.chars().collect(),
        pos: 0,
    };
    parser.skip_space();
    let value = parser.value()?;
    parser.skip_space();
    if parser.pos != parser.chars.len() {
        return Err(parser_error(format!(
            "Expected end of file but found '{}'",
            parser.chars[parser.pos]
        )));
    }
    Ok(value)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn skip_space(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos] <= ' ' {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Result<NodeKind, ValueError> {
        self.skip_space();
        let Some(ch) = self.chars.get(self.pos).copied() else {
            return Err(parser_error(
                "Unexpected end of file where value was expected".to_owned(),
            ));
        };
        match ch {
            '{' => self.object(),
            '[' => self.array(),
            '"' => Ok(NodeKind::Str(self.string()?)),
            't' => self.literal("true", NodeKind::Bool(true)),
            'f' => self.literal("false", NodeKind::Bool(false)),
            'n' => self.literal("null", NodeKind::Null),
            _ => self.number(),
        }
    }

    fn literal(&mut self, text: &str, value: NodeKind) -> Result<NodeKind, ValueError> {
        let word: String = self.chars[self.pos..].iter().take(text.len()).collect();
        if word == text {
            self.pos += text.len();
            Ok(value)
        } else {
            Err(parser_error(format!("Expected value but found {word}")))
        }
    }

    fn object(&mut self) -> Result<NodeKind, ValueError> {
        self.pos += 1;
        let mut entries: Vec<(String, Node)> = Vec::new();
        self.skip_space();
        if self.chars.get(self.pos) == Some(&'}') {
            self.pos += 1;
            return Ok(NodeKind::Object(entries));
        }
        loop {
            self.skip_space();
            if self.chars.get(self.pos) != Some(&'"') {
                return Err(parser_error("Expected '\"{\"' but found value".to_owned()));
            }
            let name = self.string()?;
            self.skip_space();
            if self.chars.get(self.pos) != Some(&':') {
                return Err(parser_error("Expected '\":\"' but found value".to_owned()));
            }
            self.pos += 1;
            let value = self.value()?;
            // The library keeps a repeated key; lookups find the first.
            entries.push((name, Rc::new(RefCell::new(value))));
            self.skip_space();
            match self.chars.get(self.pos) {
                Some(',') => self.pos += 1,
                Some('}') => {
                    self.pos += 1;
                    return Ok(NodeKind::Object(entries));
                }
                _ => return Err(parser_error("Expected '\"}\"' but found value".to_owned())),
            }
        }
    }

    fn array(&mut self) -> Result<NodeKind, ValueError> {
        self.pos += 1;
        let mut items: Vec<Node> = Vec::new();
        self.skip_space();
        if self.chars.get(self.pos) == Some(&']') {
            self.pos += 1;
            return Ok(NodeKind::Array(items));
        }
        loop {
            let value = self.value()?;
            items.push(Rc::new(RefCell::new(value)));
            self.skip_space();
            match self.chars.get(self.pos) {
                Some(',') => self.pos += 1,
                Some(']') => {
                    self.pos += 1;
                    return Ok(NodeKind::Array(items));
                }
                _ => return Err(parser_error("Expected '\"]\"' but found value".to_owned())),
            }
        }
    }

    fn string(&mut self) -> Result<String, ValueError> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let Some(ch) = self.chars.get(self.pos).copied() else {
                return Err(parser_error("String not closed".to_owned()));
            };
            self.pos += 1;
            match ch {
                '"' => return Ok(out),
                '\\' => {
                    let Some(escape) = self.chars.get(self.pos).copied() else {
                        return Err(parser_error("String not closed".to_owned()));
                    };
                    self.pos += 1;
                    match escape {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let hex: String = self.chars[self.pos..].iter().take(4).collect();
                            let code = u32::from_str_radix(&hex, 16)
                                .map_err(|_| parser_error(format!("Invalid hex number \"{hex}\"")))?;
                            self.pos += 4;
                            let mut code = code;
                            if (0xD800..0xDC00).contains(&code) {
                                let low_hex: String = self
                                    .chars
                                    .get(self.pos..self.pos + 1)
                                    .into_iter()
                                    .flatten()
                                    .copied()
                                    .collect();
                                if low_hex == "\\" {
                                    let low_hex: String = self.chars[self.pos + 2..].iter().take(4).collect();
                                    if let Ok(low) = u32::from_str_radix(&low_hex, 16)
                                        && (0xDC00..0xE000).contains(&low)
                                    {
                                        self.pos += 6;
                                        code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                    }
                                }
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err(parser_error("Invalid character in string".to_owned())),
                    }
                }
                ch => out.push(ch),
            }
        }
    }

    fn number(&mut self) -> Result<NodeKind, ValueError> {
        let start = self.pos;
        let mut is_float = false;
        if self.chars.get(self.pos) == Some(&'-') {
            self.pos += 1;
        }
        while let Some(ch) = self.chars.get(self.pos).copied() {
            match ch {
                '0'..='9' => self.pos += 1,
                '.' | 'e' | 'E' | '+' | '-' => {
                    is_float = true;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        if text.is_empty() {
            return Err(parser_error(format!(
                "Expected value but found '{}'",
                self.chars.get(self.pos).copied().unwrap_or(' ')
            )));
        }
        if is_float {
            return text
                .parse::<f64>()
                .map(NodeKind::Float)
                .map_err(|_| parser_error(format!("Expected value but found {text}")));
        }
        // The library types an integer of up to 9 digits as `jdtInt`, of up
        // to 19 as `jdtLong`, and a longer one as `jdtULong` or a float.
        let negative = text.starts_with('-');
        let digits = text.trim_start_matches('-');
        if digits.len() <= 9 {
            return text
                .parse::<i32>()
                .map(NodeKind::Int)
                .map_err(|_| parser_error(format!("Expected value but found {text}")));
        }
        match digits.parse::<i64>() {
            Ok(value) => Ok(if negative {
                NodeKind::Long(value)
            } else if i32::try_from(value).is_ok() {
                NodeKind::Int(value as i32)
            } else {
                NodeKind::Long(value)
            }),
            Err(_) => match digits.parse::<u64>() {
                Ok(value) if !negative && digits.len() >= 20 => Ok(NodeKind::ULong(value)),
                Ok(value) => Ok(NodeKind::Long(value as i64)),
                Err(_) => text
                    .parse::<f64>()
                    .map(NodeKind::Float)
                    .map_err(|_| parser_error(format!("Expected value but found {text}"))),
            },
        }
    }
}

//=== Registration ============================================================

fn value_param() -> ParamType {
    ParamType::value(VarType::Empty)
}

fn function(adapter: &mut Adapter, identifier: &str, param_count: i32, func: xedit_script::adapter::AdapterGetFn) {
    adapter.add_function(
        "JsonDataObjects",
        identifier,
        func,
        param_count,
        (0..param_count.max(0)).map(|_| value_param()).collect(),
        VarType::Empty,
    );
}

fn get(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    identifier: &str,
    param_count: i32,
    func: xedit_script::adapter::AdapterGetFn,
) {
    adapter.add_get(
        class.clone(),
        identifier,
        func,
        param_count,
        (0..param_count.max(0)).map(|_| value_param()).collect(),
        VarType::Empty,
    );
}

/// Registers `xejviScriptAdapterMiscJson.pas`.
pub fn register(adapter: &mut Adapter, classes: &ScriptClasses) {
    let dom = Rc::new(JsonDom {
        base: classes.t_json_base_object.clone(),
        array: classes.t_json_array.clone(),
        object: classes.t_json_object.clone(),
    });

    //=== The constants =======================================================

    for (name, value) in [
        ("jdtNone", JDT_NONE),
        ("jdtString", JDT_STRING),
        ("jdtInt", JDT_INT),
        ("jdtLong", JDT_LONG),
        ("jdtULong", JDT_ULONG),
        ("jdtFloat", JDT_FLOAT),
        ("jdtDateTime", JDT_DATETIME),
        ("jdtBool", JDT_BOOL),
        ("jdtArray", JDT_ARRAY),
        ("jdtObject", JDT_OBJECT),
    ] {
        adapter.add_const("JsonDataObjects", name, Value::Integer(value));
    }

    //=== The SetJDO* configuration functions =================================

    function(
        adapter,
        "SetJDOIndentChar",
        1,
        Rc::new(|_value, args| {
            let text = text_arg(&args.values[0]);
            if let Some(ch) = text.chars().next() {
                set_config(|config| config.indent_char = ch);
            }
            Ok(())
        }),
    );
    function(
        adapter,
        "SetJDOLineBreak",
        1,
        Rc::new(|_value, args| {
            set_config(|config| config.line_break = text_arg(&args.values[0]));
            Ok(())
        }),
    );
    function(
        adapter,
        "SetJDONullConvertsToValueTypes",
        1,
        Rc::new(|_value, args| {
            let null_converts = xedit_script::values::to_bool(&args.values[0])?;
            set_config(|config| config.null_converts_to_value_types = null_converts);
            Ok(())
        }),
    );
    function(
        adapter,
        "SetJDOUseUtcTime",
        1,
        Rc::new(|_value, args| {
            let use_utc = xedit_script::values::to_bool(&args.values[0])?;
            set_config(|config| config.use_utc_time = use_utc);
            Ok(())
        }),
    );

    //=== TJsonBaseObject =====================================================

    get(
        adapter,
        &dom.base,
        "FromJSON",
        1,
        Rc::new(|_value, args| {
            let object = json_receiver(args)?;
            let text = text_arg(&args.values[0]);
            let parsed = parse_json(&text)?;
            set_node(&object, parsed);
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.base,
        "LoadFromFile",
        -1,
        Rc::new(|_value, args| {
            let object = json_receiver(args)?;
            let file_name = text_arg(&args.values[0]);
            let utf8_without_bom = args.values.get(1).map_or(Ok(true), xedit_script::values::to_bool)?;
            let _ = utf8_without_bom;
            let bytes = std::fs::read(&file_name).map_err(|error| {
                ValueError::external_class(
                    "EFOpenError",
                    format!(
                        "Cannot open file \"{file_name}\". {}",
                        crate::script_strings::os_error_text(&error)
                    ),
                )
            })?;
            let text = decode_stream_bytes(&bytes, None);
            let parsed = parse_json(&text)?;
            set_node(&object, parsed);
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.base,
        "LoadFromResource",
        -1,
        Rc::new(|_value, args| {
            let object = json_receiver(args)?;
            // `wbContainerHandler.OpenResourceData('', ResName)` or
            // `(ResType, ResName)`.
            let (container, name) = if args.values.len() > 1 {
                (text_arg(&args.values[0]), text_arg(&args.values[1]))
            } else {
                (String::new(), text_arg(&args.values[0]))
            };
            let bytes = open_resource_data(&container, &name);
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let parsed = parse_json(&text)?;
            set_node(&object, parsed);
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.base,
        "LoadFromStream",
        -1,
        Rc::new(|_value, _args| {
            // The stream classes (`TMemoryStream`, `TFileStream`) are the JVCL
            // `Classes` unit's, phase 6 step 8; until then no script can hold
            // one.
            Err(ValueError::external_class(
                E_JSON_CLASS,
                "LoadFromStream needs a TStream; the JVCL Classes stream classes are phase 6 step 8".to_owned(),
            ))
        }),
    );
    // `Parse` is a class function of `TJsonBaseObject` called through the
    // class (`TJsonObject.Parse`) as often as through an instance; the
    // receiver is the class upstream (`TJsonBaseObject(Args.Obj)`), so the
    // handler needs none of it and the parsed document's own shape decides
    // the class of the result.
    get(
        adapter,
        &dom.base,
        "Parse",
        1,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let text = text_arg(&args.values[0]);
                let parsed = parse_document(&text)?;
                *value = match parsed {
                    Some(node) => node_value(&dom, node),
                    None => Value::Null,
                };
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.base,
        "SaveToFile",
        -1,
        Rc::new(|_value, args| {
            let object = json_receiver(args)?;
            let file_name = text_arg(&args.values[0]);
            let compact = args.values.get(1).map_or(Ok(true), xedit_script::values::to_bool)?;
            let encoding = args.values.get(2).and_then(encoding_of);
            let utf8_without_bom = args.values.get(3).map_or(Ok(true), xedit_script::values::to_bool)?;
            let text = to_json(&node_of(&object), compact);
            let bytes = encode_stream_bytes(&text, encoding, utf8_without_bom);
            std::fs::write(&file_name, bytes).map_err(|error| {
                ValueError::external_class(
                    "EFCreateError",
                    format!(
                        "Cannot create file \"{file_name}\". {}",
                        crate::script_strings::os_error_text(&error)
                    ),
                )
            })
        }),
    );
    get(
        adapter,
        &dom.base,
        "SaveToLines",
        1,
        Rc::new(|_value, args| {
            let lines = list_of_or_error(&args.values[0])?;
            let object = json_receiver(args)?;
            // The writer forces the readable form for a `TStrings` and adds one
            // line per line without the line breaks.
            let text = to_json(&node_of(&object), false);
            let mut lines = borrow_list_mut(&lines);
            lines.clear();
            for line in text.split('\n') {
                let line = line.strip_suffix('\r').unwrap_or(line);
                if line.is_empty() && text.ends_with('\n') {
                    continue;
                }
                if line.is_empty() {
                    continue;
                }
                lines.append(line.to_owned())?;
            }
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.base,
        "SaveToStream",
        -1,
        Rc::new(|_value, _args| {
            Err(ValueError::external_class(
                E_JSON_CLASS,
                "SaveToStream needs a TStream; the JVCL Classes stream classes are phase 6 step 8".to_owned(),
            ))
        }),
    );
    get(
        adapter,
        &dom.base,
        "ToJSON",
        -1,
        Rc::new(|value, args| {
            let object = json_receiver(args)?;
            let compact = args.values.first().map_or(Ok(true), xedit_script::values::to_bool)?;
            *value = Value::Str(to_json(&node_of(&object), compact).into_bytes());
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.base,
        "ToString",
        0,
        Rc::new(|value, args| {
            let object = json_receiver(args)?;
            *value = Value::Str(to_json(&node_of(&object), true).into_bytes());
            Ok(())
        }),
    );

    //=== TJsonArray ==========================================================

    get(
        adapter,
        &dom.array,
        "Create",
        0,
        Rc::new({
            let dom = dom.clone();
            move |value, _args| {
                let node: Node = Rc::new(RefCell::new(NodeKind::Array(Vec::new())));
                *value = node_value(&dom, node);
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.array,
        "Add",
        1,
        Rc::new(|_value, args| {
            let object = json_receiver(args)?;
            // `VarTypeToJsonDataType(VarType(AValue))` first: the type check runs
            // before the item is added.
            let kind = data_type_of_value(&args.values[0])?;
            let node = node_of(&object);
            if let NodeKind::Array(items) = &mut *node.borrow_mut() {
                items.push(Rc::new(RefCell::new(kind)));
            }
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.array,
        "AddArray",
        0,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let object = json_receiver(args)?;
                let node: Node = Rc::new(RefCell::new(NodeKind::Array(Vec::new())));
                if let NodeKind::Array(items) = &mut *node_of(&object).borrow_mut() {
                    items.push(node.clone());
                }
                *value = node_value(&dom, node);
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.array,
        "AddObject",
        0,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let object = json_receiver(args)?;
                let node: Node = Rc::new(RefCell::new(NodeKind::Object(Vec::new())));
                if let NodeKind::Array(items) = &mut *node_of(&object).borrow_mut() {
                    items.push(node.clone());
                }
                *value = node_value(&dom, node);
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.array,
        "Assign",
        1,
        Rc::new(|_value, args| {
            let object = json_receiver(args)?;
            let source = json_of(&args.values[0]).ok_or_else(|| cast_error("nil", "Array"))?;
            let source_kind = node_of(&source).borrow().clone();
            if !matches!(source_kind, NodeKind::Array(_)) {
                return Err(cast_error(source_kind.name(), "Array"));
            }
            set_node(&object, source_kind);
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.array,
        "Clear",
        0,
        Rc::new(|_value, args| {
            let object = json_receiver(args)?;
            set_node(&object, NodeKind::Array(Vec::new()));
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.array,
        "Count",
        0,
        Rc::new(|value, args| {
            let object = json_receiver(args)?;
            *value = Value::Integer(array_items(&object)?.len() as i32);
            Ok(())
        }),
    );
    adapter.add_set(
        dom.array.clone(),
        "Count",
        Rc::new(|value: &Value, args: &mut Args| {
            let count = xedit_script::values::to_i32(value)?.max(0) as usize;
            let object = json_receiver(args)?;
            let node = node_of(&object);
            if let NodeKind::Array(items) = &mut *node.borrow_mut() {
                if count < items.len() {
                    items.truncate(count);
                } else {
                    while items.len() < count {
                        // New items are "null" (`SetCount`).
                        items.push(Rc::new(RefCell::new(NodeKind::Null)));
                    }
                }
            }
            Ok(())
        }),
        1,
        vec![value_param()],
    );
    get(
        adapter,
        &dom.array,
        "Delete",
        1,
        Rc::new(|_value, args| {
            let index = xedit_script::values::to_i64(&args.values[0])?;
            let object = json_receiver(args)?;
            let node = node_of(&object);
            if let NodeKind::Array(items) = &mut *node.borrow_mut() {
                if index < 0 || index >= items.len() as i64 {
                    return Err(list_index_error(index));
                }
                items.remove(index as usize);
            }
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.array,
        "Extract",
        1,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let index = xedit_script::values::to_i64(&args.values[0])?;
                let object = json_receiver(args)?;
                let node = node_of(&object);
                let extracted = {
                    let mut borrowed = node.borrow_mut();
                    let NodeKind::Array(items) = &mut *borrowed else {
                        return Ok(());
                    };
                    if index < 0 || index >= items.len() as i64 {
                        return Err(list_index_error(index));
                    }
                    let item = items.remove(index as usize);
                    let kind = item.borrow().clone();
                    match kind {
                        NodeKind::None | NodeKind::Null | NodeKind::Array(_) | NodeKind::Object(_) => Some(item),
                        _ => None,
                    }
                };
                *value = match extracted {
                    Some(item) if matches!(&*item.borrow(), NodeKind::Null) => Value::Null,
                    Some(item) => node_value(&dom, item),
                    None => Value::Null,
                };
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.array,
        "ExtractArray",
        1,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let index = xedit_script::values::to_i64(&args.values[0])?;
                let object = json_receiver(args)?;
                let node = node_of(&object);
                let extracted = {
                    let mut borrowed = node.borrow_mut();
                    let NodeKind::Array(items) = &mut *borrowed else {
                        return Ok(());
                    };
                    if index < 0 || index >= items.len() as i64 {
                        return Err(list_index_error(index));
                    }
                    let item = items.remove(index as usize);
                    if !matches!(&*item.borrow(), NodeKind::Array(_)) {
                        return Err(cast_error("Object", "Array"));
                    }
                    item
                };
                *value = node_value(&dom, extracted);
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.array,
        "ExtractObject",
        1,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let index = xedit_script::values::to_i64(&args.values[0])?;
                let object = json_receiver(args)?;
                let node = node_of(&object);
                let extracted = {
                    let mut borrowed = node.borrow_mut();
                    let NodeKind::Array(items) = &mut *borrowed else {
                        return Ok(());
                    };
                    if index < 0 || index >= items.len() as i64 {
                        return Err(list_index_error(index));
                    }
                    let item = items.remove(index as usize);
                    if matches!(&*item.borrow(), NodeKind::Null) {
                        *value = Value::Null;
                        return Ok(());
                    }
                    if !matches!(&*item.borrow(), NodeKind::Object(_)) {
                        return Err(cast_error("Array", "Object"));
                    }
                    item
                };
                *value = node_value(&dom, extracted);
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.array,
        "Insert",
        2,
        Rc::new(|_value, args| {
            let index = xedit_script::values::to_i64(&args.values[0])?;
            let kind = data_type_of_value(&args.values[1])?;
            let object = json_receiver(args)?;
            let node = node_of(&object);
            if let NodeKind::Array(items) = &mut *node.borrow_mut() {
                if index < 0 || index > items.len() as i64 {
                    return Err(list_index_error(index));
                }
                items.insert(index as usize, Rc::new(RefCell::new(kind)));
            }
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.array,
        "InsertArray",
        1,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let index = xedit_script::values::to_i64(&args.values[0])?;
                let object = json_receiver(args)?;
                let node: Node = Rc::new(RefCell::new(NodeKind::Array(Vec::new())));
                {
                    let parent = node_of(&object);
                    let mut borrowed = parent.borrow_mut();
                    if let NodeKind::Array(items) = &mut *borrowed {
                        if index < 0 || index > items.len() as i64 {
                            return Err(list_index_error(index));
                        }
                        items.insert(index as usize, node.clone());
                    }
                }
                *value = node_value(&dom, node);
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.array,
        "InsertObject",
        -1,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                if args.values.is_empty() {
                    return Err(ValueError::interp(xedit_script::error::IE_NOT_ENOUGH_PARAMS));
                }
                if args.values.len() > 2 {
                    return Err(ValueError::interp(xedit_script::error::IE_TOO_MANY_PARAMS));
                }
                let index = xedit_script::values::to_i64(&args.values[0])?;
                let object = json_receiver(args)?;
                if args.values.len() == 2 {
                    let source = json_of(&args.values[1]).ok_or_else(|| cast_error("nil", "Object"))?;
                    let source_node = node_of(&source);
                    if !matches!(&*source_node.borrow(), NodeKind::Object(_)) {
                        return Err(cast_error("Array", "Object"));
                    }
                    let parent = node_of(&object);
                    let mut borrowed = parent.borrow_mut();
                    if let NodeKind::Array(items) = &mut *borrowed {
                        let count = items.len() as i64;
                        if index < 0 || index >= count {
                            return Err(list_index_error(index));
                        }
                        items.insert(index as usize, source_node);
                    }
                    return Ok(());
                }
                let node: Node = Rc::new(RefCell::new(NodeKind::Object(Vec::new())));
                {
                    let parent = node_of(&object);
                    let mut borrowed = parent.borrow_mut();
                    if let NodeKind::Array(items) = &mut *borrowed {
                        if index < 0 || index > items.len() as i64 {
                            return Err(list_index_error(index));
                        }
                        items.insert(index as usize, node.clone());
                    }
                }
                *value = node_value(&dom, node);
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.array,
        "IsNull",
        1,
        Rc::new(|value, args| {
            let index = xedit_script::values::to_i64(&args.values[0])?;
            let object = json_receiver(args)?;
            let items = array_items(&object)?;
            let item = items
                .get(index.max(0) as usize)
                .ok_or_else(|| list_index_error(index))?;
            *value = Value::Bool(item.borrow().is_null());
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.array,
        "Parse",
        1,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let text = text_arg(&args.values[0]);
                let parsed = parse_document(&text)?;
                *value = match parsed {
                    Some(node) => node_value(&dom, node),
                    None => Value::Null,
                };
                Ok(())
            }
        }),
    );

    // The indexed accessors: A/B/D/F/I/L/O/S/U and Types.
    for (identifier, access) in [
        ("A", Access::Array),
        ("B", Access::Bool),
        ("D", Access::Date),
        ("F", Access::Float),
        ("I", Access::Int),
        ("L", Access::Long),
        ("O", Access::Object),
        ("S", Access::String),
        ("U", Access::ULong),
    ] {
        array_accessor(adapter, &dom, identifier, access);
    }
    get(
        adapter,
        &dom.array,
        "Types",
        1,
        Rc::new(|value, args| {
            let index = xedit_script::values::to_i64(&args.values[0])?;
            let object = json_receiver(args)?;
            let items = array_items(&object)?;
            let item = items
                .get(index.max(0) as usize)
                .ok_or_else(|| list_index_error(index))?;
            *value = Value::Integer(item.borrow().data_type());
            Ok(())
        }),
    );

    // The array's default property (V[Index]).
    adapter.add_id_get(
        dom.array.clone(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            let index = xedit_script::values::to_i64(&args.values[0])?;
            let object = json_receiver(args)?;
            let items = array_items(&object)?;
            let item = items
                .get(index.max(0) as usize)
                .ok_or_else(|| list_index_error(index))?;
            let kind = item.borrow().clone();
            *value = variant_of_node(&kind, config().null_converts_to_value_types)?;
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_id_set(
        dom.array.clone(),
        Rc::new(|value: &Value, args: &mut Args| {
            let index = xedit_script::values::to_i64(&args.values[0])?;
            let kind = data_type_of_value(value)?;
            let object = json_receiver(args)?;
            let items = array_items(&object)?;
            let item = items
                .get(index.max(0) as usize)
                .ok_or_else(|| list_index_error(index))?;
            *item.borrow_mut() = kind;
            Ok(())
        }),
        1,
        vec![value_param()],
    );

    //=== TJsonObject =========================================================

    get(
        adapter,
        &dom.object,
        "Create",
        0,
        Rc::new({
            let dom = dom.clone();
            move |value, _args| {
                let node: Node = Rc::new(RefCell::new(NodeKind::Object(Vec::new())));
                *value = node_value(&dom, node);
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.object,
        "Assign",
        1,
        Rc::new(|_value, args| {
            let object = json_receiver(args)?;
            let source = json_of(&args.values[0]).ok_or_else(|| cast_error("nil", "Object"))?;
            let source_kind = node_of(&source).borrow().clone();
            if !matches!(source_kind, NodeKind::Object(_)) {
                return Err(cast_error(source_kind.name(), "Object"));
            }
            set_node(&object, source_kind);
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.object,
        "Clear",
        0,
        Rc::new(|_value, args| {
            let object = json_receiver(args)?;
            set_node(&object, NodeKind::Object(Vec::new()));
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.object,
        "Contains",
        1,
        Rc::new(|value, args| {
            let name = text_arg(&args.values[0]);
            let object = json_receiver(args)?;
            let entries = object_entries(&object)?;
            *value = Value::Bool(find_index(&entries, &name).is_some());
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.object,
        "Count",
        0,
        Rc::new(|value, args| {
            let object = json_receiver(args)?;
            *value = Value::Integer(object_entries(&object)?.len() as i32);
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.object,
        "Delete",
        1,
        Rc::new(|_value, args| {
            let index = xedit_script::values::to_i64(&args.values[0])?;
            let object = json_receiver(args)?;
            let node = node_of(&object);
            if let NodeKind::Object(entries) = &mut *node.borrow_mut() {
                if index < 0 || index >= entries.len() as i64 {
                    return Err(list_index_error(index));
                }
                entries.remove(index as usize);
            }
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.object,
        "Extract",
        1,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let name = text_arg(&args.values[0]);
                let object = json_receiver(args)?;
                let node = node_of(&object);
                let extracted = {
                    let mut borrowed = node.borrow_mut();
                    let NodeKind::Object(entries) = &mut *borrowed else {
                        return Ok(());
                    };
                    let Some(index) = find_index(entries, &name) else {
                        return Ok(());
                    };
                    let (_, item) = entries.remove(index);
                    let kind = item.borrow().clone();
                    match kind {
                        NodeKind::None | NodeKind::Null | NodeKind::Array(_) | NodeKind::Object(_) => Some(item),
                        _ => None,
                    }
                };
                *value = match extracted {
                    Some(item) if matches!(&*item.borrow(), NodeKind::Null) => Value::Null,
                    Some(item) => node_value(&dom, item),
                    None => Value::Null,
                };
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.object,
        "ExtractArray",
        1,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let name = text_arg(&args.values[0]);
                let object = json_receiver(args)?;
                let node = node_of(&object);
                let extracted = {
                    let mut borrowed = node.borrow_mut();
                    let NodeKind::Object(entries) = &mut *borrowed else {
                        return Ok(());
                    };
                    let Some(index) = find_index(entries, &name) else {
                        *value = Value::Null;
                        return Ok(());
                    };
                    let (_, item) = entries.remove(index);
                    if !matches!(&*item.borrow(), NodeKind::Array(_)) {
                        *value = Value::Null;
                        return Ok(());
                    }
                    item
                };
                *value = node_value(&dom, extracted);
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.object,
        "ExtractObject",
        1,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let name = text_arg(&args.values[0]);
                let object = json_receiver(args)?;
                let node = node_of(&object);
                let extracted = {
                    let mut borrowed = node.borrow_mut();
                    let NodeKind::Object(entries) = &mut *borrowed else {
                        return Ok(());
                    };
                    let Some(index) = find_index(entries, &name) else {
                        *value = Value::Null;
                        return Ok(());
                    };
                    let (_, item) = entries.remove(index);
                    if matches!(&*item.borrow(), NodeKind::Null) {
                        *value = Value::Null;
                        return Ok(());
                    }
                    if !matches!(&*item.borrow(), NodeKind::Object(_)) {
                        *value = Value::Null;
                        return Ok(());
                    }
                    item
                };
                *value = node_value(&dom, extracted);
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.object,
        "IndexOf",
        1,
        Rc::new(|value, args| {
            let name = text_arg(&args.values[0]);
            let object = json_receiver(args)?;
            let entries = object_entries(&object)?;
            *value = Value::Integer(find_index(&entries, &name).map_or(-1, |index| index as i32));
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.object,
        "IsNull",
        1,
        Rc::new(|value, args| {
            let name = text_arg(&args.values[0]);
            let object = json_receiver(args)?;
            let entries = object_entries(&object)?;
            *value = Value::Bool(match find_index(&entries, &name) {
                Some(index) => entries[index].1.borrow().is_null(),
                None => true,
            });
            Ok(())
        }),
    );
    get(
        adapter,
        &dom.object,
        "Names",
        1,
        Rc::new(|value, args| {
            let index = xedit_script::values::to_i64(&args.values[0])?;
            let object = json_receiver(args)?;
            let entries = object_entries(&object)?;
            let name = entries
                .get(index.max(0) as usize)
                .ok_or_else(|| list_index_error(index))?
                .0
                .clone();
            *value = Value::Str(name.into_bytes());
            Ok(())
        }),
    );
    adapter.add_iget(
        dom.object.clone(),
        "Names",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let index = xedit_script::values::to_i64(&args.values[0])?;
            let object = json_receiver(args)?;
            let entries = object_entries(&object)?;
            let name = entries
                .get(index.max(0) as usize)
                .ok_or_else(|| list_index_error(index))?
                .0
                .clone();
            *value = Value::Str(name.into_bytes());
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    get(
        adapter,
        &dom.object,
        "Parse",
        1,
        Rc::new({
            let dom = dom.clone();
            move |value, args| {
                let text = text_arg(&args.values[0]);
                let parsed = parse_document(&text)?;
                *value = match parsed {
                    Some(node) => node_value(&dom, node),
                    None => Value::Null,
                };
                Ok(())
            }
        }),
    );
    get(
        adapter,
        &dom.object,
        "Remove",
        1,
        Rc::new(|_value, args| {
            let name = text_arg(&args.values[0]);
            let object = json_receiver(args)?;
            let node = node_of(&object);
            if let NodeKind::Object(entries) = &mut *node.borrow_mut()
                && let Some(index) = find_index(entries, &name)
            {
                entries.remove(index);
            }
            Ok(())
        }),
    );

    for (identifier, access) in [
        ("A", Access::Array),
        ("B", Access::Bool),
        ("D", Access::Date),
        ("F", Access::Float),
        ("I", Access::Int),
        ("L", Access::Long),
        ("O", Access::Object),
        ("S", Access::String),
        ("U", Access::ULong),
    ] {
        object_accessor(adapter, &dom, identifier, access);
    }
    adapter.add_iget(
        dom.object.clone(),
        "Types",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let name = text_arg(&args.values[0]);
            let object = json_receiver(args)?;
            let entries = object_entries(&object)?;
            *value = Value::Integer(match find_index(&entries, &name) {
                Some(index) => entries[index].1.borrow().data_type(),
                None => JDT_NONE,
            });
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );

    // The object's default property (V[name]): a read of a missing name is
    // `Unassigned` (the helper's `jdtNone`), a write creates the entry.
    adapter.add_id_get(
        dom.object.clone(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            let name = text_arg(&args.values[0]);
            let object = json_receiver(args)?;
            let entries = object_entries(&object)?;
            *value = match find_index(&entries, &name) {
                Some(index) => {
                    let kind = entries[index].1.borrow().clone();
                    variant_of_node(&kind, config().null_converts_to_value_types)?
                }
                None => Value::Empty,
            };
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_id_set(
        dom.object.clone(),
        Rc::new(|value: &Value, args: &mut Args| {
            let name = text_arg(&args.values[0]);
            let kind = data_type_of_value(value)?;
            let object = json_receiver(args)?;
            let node = node_of(&object);
            if let NodeKind::Object(entries) = &mut *node.borrow_mut() {
                let item = require_item(entries, &name);
                *item.borrow_mut() = kind;
            }
            Ok(())
        }),
        1,
        vec![value_param()],
    );
}

/// `TJsonBaseObject.Parse`: nil for an empty or whitespace-only text, else
/// the parsed document (`[` starts an array, anything else an object).
fn parse_document(text: &str) -> Result<Option<Node>, ValueError> {
    let trimmed = text.trim_start_matches(|ch: char| ch <= ' ');
    if trimmed.is_empty() {
        return Ok(None);
    }
    let kind = parse_json(text)?;
    Ok(Some(Rc::new(RefCell::new(kind))))
}

/// The `GetBytes` side of the stream readers: the byte order mark decides
/// the encoding when none is given (`GetStreamBytes`).
fn decode_stream_bytes(bytes: &[u8], encoding: Option<EncodingKind>) -> String {
    if let Some(kind) = encoding {
        let rest = bytes.strip_prefix(kind.preamble()).unwrap_or(bytes);
        return kind.string(rest);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return EncodingKind::Unicode.string(rest);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    EncodingKind::Default.string(bytes)
}

/// `SaveToStream`: `Utf8WithoutBOM` (default True) with a UTF-8 or no
/// encoding writes UTF-8 without a mark; every other encoding writes its
/// preamble.
fn encode_stream_bytes(text: &str, encoding: Option<EncodingKind>, utf8_without_bom: bool) -> Vec<u8> {
    let without_bom = utf8_without_bom && matches!(encoding, None | Some(EncodingKind::Utf8));
    let kind = if without_bom {
        EncodingKind::Utf8
    } else {
        encoding.unwrap_or(EncodingKind::Default)
    };
    let mut bytes = if without_bom {
        Vec::new()
    } else {
        kind.preamble().to_vec()
    };
    bytes.extend(kind.bytes(text));
    bytes
}

/// The property a `A[i]`/`S[name]` accessor reads and writes.
#[derive(Clone, Copy, PartialEq)]
pub enum Access {
    Array,
    Bool,
    Date,
    Float,
    Int,
    Long,
    Object,
    String,
    ULong,
}

impl Access {
    /// The `data_type` the property names (the setter overload the variant
    /// picks); kept for the accessors of the later steps.
    #[allow(dead_code)]
    fn key(self) -> i32 {
        match self {
            Access::Array => JDT_ARRAY,
            Access::Bool => JDT_BOOL,
            Access::Date => JDT_DATETIME,
            Access::Float => JDT_FLOAT,
            Access::Int => JDT_INT,
            Access::Long => JDT_LONG,
            Access::Object => JDT_OBJECT,
            Access::String => JDT_STRING,
            Access::ULong => JDT_ULONG,
        }
    }

    /// The generic readers of `TJsonDataValue`.
    fn read(self, kind: &NodeKind, null_converts: bool) -> Result<Value, ValueError> {
        match self {
            Access::String => Ok(Value::Str(string_of_node(kind, null_converts)?.into_bytes())),
            Access::Int => Ok(Value::Integer(int_of_node(kind, null_converts)?)),
            Access::Long => Ok(Value::Int64(match kind {
                NodeKind::Int(v) => i64::from(*v),
                NodeKind::Long(v) => *v,
                NodeKind::ULong(v) => *v as i64,
                _ => i64::from(int_of_node(kind, null_converts)?),
            })),
            Access::ULong => Ok(Value::Int64(int_of_node(kind, null_converts)? as i64)),
            Access::Float => Ok(Value::Double(float_of_node(kind, null_converts)?)),
            Access::Date => Ok(Value::Double(date_of_node(kind, null_converts)?)),
            Access::Bool => Ok(Value::Bool(bool_of_node(kind, null_converts)?)),
            Access::Array | Access::Object => Ok(Value::Null),
        }
    }

    /// The generic writers of `TJsonDataValue`.
    fn write(self, value: &Value) -> Result<NodeKind, ValueError> {
        let text = || -> String { text_arg(value) };
        Ok(match self {
            Access::String => NodeKind::Str(text()),
            Access::Int => NodeKind::Int(
                xedit_script::values::to_i32(value).unwrap_or_else(|_| text().trim().parse::<i32>().unwrap_or(0)),
            ),
            Access::Long => NodeKind::Long(xedit_script::values::to_i64(value).unwrap_or(0)),
            Access::ULong => NodeKind::ULong(xedit_script::values::to_i64(value).unwrap_or(0) as u64),
            Access::Float => NodeKind::Float(xedit_script::values::to_f64(value)?),
            Access::Date => NodeKind::DateTime(xedit_script::values::to_f64(value)?),
            Access::Bool => NodeKind::Bool(xedit_script::values::to_bool(value)?),
            Access::Array | Access::Object => data_type_of_value(value)?,
        })
    }
}

/// `TJsonArray`'s `A/B/D/F/I/L/O/S/U` indexed properties.
fn array_accessor(adapter: &mut Adapter, dom: &Rc<JsonDom>, identifier: &str, access: Access) {
    adapter.add_iget(
        dom.array.clone(),
        identifier,
        Rc::new(move |value: &mut Value, args: &mut Args| {
            let index = xedit_script::values::to_i64(&args.values[0])?;
            let object = json_receiver(args)?;
            let items = array_items(&object)?;
            let item = items
                .get(index.max(0) as usize)
                .ok_or_else(|| list_index_error(index))?
                .clone();
            match access {
                Access::Array => {
                    let Some(node) = array_value_of(&item)? else {
                        *value = Value::Null;
                        return Ok(());
                    };
                    *value = node_value(&dom_of(&object), node);
                }
                Access::Object => {
                    let Some(node) = object_value_of(&item)? else {
                        *value = Value::Null;
                        return Ok(());
                    };
                    *value = node_value(&dom_of(&object), node);
                }
                access => {
                    let kind = item.borrow().clone();
                    *value = access.read(&kind, config().null_converts_to_value_types)?;
                }
            }
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_iset(
        dom.array.clone(),
        identifier,
        Rc::new(move |value: &Value, args: &mut Args| {
            let index = xedit_script::values::to_i64(&args.values[0])?;
            let object = json_receiver(args)?;
            let items = array_items(&object)?;
            let item = items
                .get(index.max(0) as usize)
                .ok_or_else(|| list_index_error(index))?
                .clone();
            let kind = match access {
                Access::Array | Access::Object => {
                    if let Some(source) = node_of_value(value) {
                        let wanted = if access == Access::Array { JDT_ARRAY } else { JDT_OBJECT };
                        let source_kind = source.borrow().clone();
                        if source_kind.data_type() == wanted {
                            source_kind
                        } else {
                            data_type_of_value(value)?
                        }
                    } else {
                        data_type_of_value(value)?
                    }
                }
                access => access.write(value)?,
            };
            *item.borrow_mut() = kind;
            Ok(())
        }),
        1,
        vec![value_param()],
    );
}

/// `TJsonObject`'s `A/B/D/F/I/L/O/S/U` named properties: reading a missing
/// name creates an empty array/object (`GetArray`/`GetObject`); every other
/// reader of a missing name gives the type's zero (`GetString` `''`, ...).
fn object_accessor(adapter: &mut Adapter, dom: &Rc<JsonDom>, identifier: &str, access: Access) {
    adapter.add_iget(
        dom.object.clone(),
        identifier,
        Rc::new(move |value: &mut Value, args: &mut Args| {
            let name = text_arg(&args.values[0]);
            let object = json_receiver(args)?;
            let entries = object_entries(&object)?;
            let item = match find_index(&entries, &name) {
                Some(index) => entries[index].1.clone(),
                None => match access {
                    Access::Array | Access::Object => {
                        // `GetArray`/`GetObject` create the entry.
                        let kind = if access == Access::Array {
                            NodeKind::Array(Vec::new())
                        } else {
                            NodeKind::Object(Vec::new())
                        };
                        let node: Node = Rc::new(RefCell::new(kind));
                        let parent = node_of(&object);
                        if let NodeKind::Object(parent_entries) = &mut *parent.borrow_mut() {
                            parent_entries.push((name, node.clone()));
                        }
                        node
                    }
                    _ => {
                        // A missing name reads as the type's zero.
                        *value = access.read(&NodeKind::None, false)?;
                        return Ok(());
                    }
                },
            };
            match access {
                Access::Array => {
                    let Some(node) = array_value_of(&item)? else {
                        *value = Value::Null;
                        return Ok(());
                    };
                    *value = node_value(&dom_of(&object), node);
                }
                Access::Object => {
                    let Some(node) = object_value_of(&item)? else {
                        *value = Value::Null;
                        return Ok(());
                    };
                    *value = node_value(&dom_of(&object), node);
                }
                access => {
                    let kind = item.borrow().clone();
                    *value = access.read(&kind, config().null_converts_to_value_types)?;
                }
            }
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_iset(
        dom.object.clone(),
        identifier,
        Rc::new(move |value: &Value, args: &mut Args| {
            let name = text_arg(&args.values[0]);
            let object = json_receiver(args)?;
            let kind = match access {
                Access::Array | Access::Object => {
                    let wanted = if access == Access::Array { JDT_ARRAY } else { JDT_OBJECT };
                    if let Some(source) = node_of_value(value) {
                        let source_kind = source.borrow().clone();
                        if source_kind.data_type() == wanted {
                            source_kind
                        } else {
                            return Ok(());
                        }
                    } else {
                        return Ok(());
                    }
                }
                access => access.write(value)?,
            };
            let parent = node_of(&object);
            if let NodeKind::Object(entries) = &mut *parent.borrow_mut() {
                let item = require_item(entries, &name);
                *item.borrow_mut() = kind;
            }
            Ok(())
        }),
        1,
        vec![value_param()],
    );
}

/// Keeps `receiver` referenced: the default-property registrations of the
/// later steps (the VCL shim) use it for their receivers.
#[allow(dead_code)]
fn receiver_is_kept_referenced() {
    let _ = receiver;
}
