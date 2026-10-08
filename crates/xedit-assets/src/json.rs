// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The JSON documents of the data format units, as the `JsonDataObjects`
//! library that upstream uses builds and prints them: objects keep their
//! keys in insertion order, setting a key that exists replaces its value in
//! place, and the readable form indents with tabs. The accessors read values
//! the way the library's `S[]`, `I[]`, `F[]`, `B[]`, `O[]` and `A[]`
//! properties do, with the same conversions and the same errors.

use xedit_core::delphi::{float_to_str, str_to_float};

use crate::data_format::DfError;
use crate::variant::str_to_int64;

/// The type a number gets when the library parses it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Number {
    Int(i64),
    Long(i64),
    ULong(u64),
    Float(f64),
}

/// The library's number parser: an integer of up to 9 digits is an
/// `Integer`, of up to 19 a `Long` (a `ULong` when it does not fit), of 20 a
/// `ULong`; a fraction, an exponent or an integer too large is a `Float`.
fn parse_number(text: &str) -> Number {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let float = || Number::Float(text.parse::<f64>().unwrap_or(0.0));
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return float();
    }
    let count = digits.len();
    if count <= 9 {
        let value: i64 = digits.parse().unwrap_or(0);
        return Number::Int(if negative { -value } else { value });
    }
    let Ok(value) = digits.parse::<u64>() else {
        return float();
    };
    if negative {
        if value > i64::MAX as u64 {
            return Number::Float(-(value as f64));
        }
        let value = -(value as i64);
        return if i32::try_from(value).is_ok() {
            Number::Int(value)
        } else {
            Number::Long(value)
        };
    }
    if count == 20 || value > i64::MAX as u64 {
        Number::ULong(value)
    } else {
        Number::Long(value as i64)
    }
}

fn cast_error(from: &str, into: &str) -> DfError {
    DfError::new(format!("Cannot cast {from} into {into}"))
}

fn index_error(index: usize) -> DfError {
    DfError::new(format!("List index out of bounds ({index})"))
}

fn not_a_float(text: &str) -> DfError {
    DfError::new(format!("'{text}' is not a valid floating point value"))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// A number with its text as written.
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn object() -> Json {
        Json::Obj(Vec::new())
    }

    pub fn array() -> Json {
        Json::Arr(Vec::new())
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Json::Obj(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Json::Arr(_))
    }

    /// `O.S[Name] := Value`, `O.O[Name] := Value`, `O.A[Name] := Value`:
    /// replaces the value of an existing key in place, else appends.
    pub fn set(&mut self, name: &str, value: Json) {
        if let Json::Obj(entries) = self {
            match entries.iter_mut().find(|(key, _)| key == name) {
                Some(entry) => entry.1 = value,
                None => entries.push((name.to_owned(), value)),
            }
        }
    }

    /// `A.Add(Value)`.
    pub fn push(&mut self, value: Json) {
        if let Json::Arr(items) = self {
            items.push(value);
        }
    }

    /// The first value with the key, as `FindItem` finds it.
    pub fn get(&self, name: &str) -> Option<&Json> {
        match self {
            Json::Obj(entries) => entries.iter().find(|(key, _)| key == name).map(|(_, value)| value),
            _ => None,
        }
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut Json> {
        match self {
            Json::Obj(entries) => entries.iter_mut().find(|(key, _)| key == name).map(|(_, value)| value),
            _ => None,
        }
    }

    /// The keys, in order, repeated keys included (`Names[i]`).
    pub fn names(&self) -> Vec<String> {
        match self {
            Json::Obj(entries) => entries.iter().map(|(key, _)| key.clone()).collect(),
            _ => Vec::new(),
        }
    }

    /// The name of the value's type in the library's cast errors; `null`
    /// is a nil object there.
    fn type_name(&self) -> &'static str {
        match self {
            Json::Null | Json::Obj(_) => "Object",
            Json::Bool(_) => "Bool",
            Json::Str(_) => "String",
            Json::Arr(_) => "Array",
            Json::Num(text) => match parse_number(text) {
                Number::Int(_) => "Integer",
                Number::Long(_) => "Long",
                Number::ULong(_) => "ULong",
                Number::Float(_) => "Float",
            },
        }
    }

    /// `TJsonDataValue.Value`: the value as a string.
    pub fn value_str(&self) -> Result<String, DfError> {
        match self {
            Json::Str(text) => Ok(text.clone()),
            Json::Num(text) => Ok(match parse_number(text) {
                Number::Int(value) | Number::Long(value) => value.to_string(),
                Number::ULong(value) => value.to_string(),
                Number::Float(value) => float_to_str(value),
            }),
            Json::Bool(true) => Ok("true".to_owned()),
            Json::Bool(false) => Ok("false".to_owned()),
            _ => Err(cast_error(self.type_name(), "String")),
        }
    }

    /// `TJsonDataValue.IntValue`.
    pub fn value_int(&self) -> Result<i32, DfError> {
        match self {
            Json::Str(text) => match str_to_int64(text).and_then(|value| i32::try_from(value).ok()) {
                Some(value) => Ok(value),
                None => Ok(str_to_float(text).ok_or_else(|| not_a_float(text))?.trunc() as i64 as i32),
            },
            Json::Num(text) => Ok(match parse_number(text) {
                Number::Int(value) | Number::Long(value) => value as i32,
                Number::ULong(value) => value as i32,
                Number::Float(value) => value.trunc() as i64 as i32,
            }),
            Json::Bool(value) => Ok(i32::from(*value)),
            _ => Err(cast_error(self.type_name(), "Integer")),
        }
    }

    /// `TJsonDataValue.FloatValue`.
    pub fn value_float(&self) -> Result<f64, DfError> {
        match self {
            Json::Str(text) => str_to_float(text).ok_or_else(|| not_a_float(text)),
            Json::Num(text) => Ok(match parse_number(text) {
                Number::Int(value) | Number::Long(value) => value as f64,
                Number::ULong(value) => value as f64,
                Number::Float(value) => value,
            }),
            Json::Bool(value) => Ok(f64::from(u8::from(*value))),
            _ => Err(cast_error(self.type_name(), "Float")),
        }
    }

    /// `TJsonDataValue.BoolValue`.
    pub fn value_bool(&self) -> Result<bool, DfError> {
        match self {
            Json::Str(text) => Ok(text == "true"),
            Json::Num(text) => Ok(match parse_number(text) {
                Number::Int(value) | Number::Long(value) => value != 0,
                Number::ULong(value) => value != 0,
                Number::Float(value) => value != 0.0,
            }),
            Json::Bool(value) => Ok(*value),
            _ => Err(cast_error(self.type_name(), "Bool")),
        }
    }

    /// `O.S[Name]`: empty when the key is missing.
    pub fn s(&self, name: &str) -> Result<String, DfError> {
        self.get(name).map_or(Ok(String::new()), Json::value_str)
    }

    /// `O.I[Name]`: 0 when the key is missing.
    pub fn i(&self, name: &str) -> Result<i32, DfError> {
        self.get(name).map_or(Ok(0), Json::value_int)
    }

    /// `O.F[Name]`: 0 when the key is missing.
    pub fn f(&self, name: &str) -> Result<f64, DfError> {
        self.get(name).map_or(Ok(0.0), Json::value_float)
    }

    /// `O.B[Name]`: false when the key is missing.
    pub fn b(&self, name: &str) -> Result<bool, DfError> {
        self.get(name).map_or(Ok(false), Json::value_bool)
    }

    /// `A.S[Index]`.
    pub fn s_at(&self, index: usize) -> Result<String, DfError> {
        self.index(index).ok_or_else(|| index_error(index))?.value_str()
    }

    /// `O.O[Name]` or `O.A[Name]`: adds an empty object or array when the
    /// key is missing.
    fn container_mut(&mut self, name: &str, array: bool) -> Result<Option<&mut Json>, DfError> {
        if self.get(name).is_none() {
            self.set(name, if array { Json::array() } else { Json::object() });
        }
        match self.get_mut(name) {
            Some(value) => container_value(value, array),
            None => Ok(None),
        }
    }

    /// `O.O[Name]`.
    pub fn o_mut(&mut self, name: &str) -> Result<Option<&mut Json>, DfError> {
        self.container_mut(name, false)
    }

    /// `O.A[Name]`.
    pub fn a_mut(&mut self, name: &str) -> Result<Option<&mut Json>, DfError> {
        self.container_mut(name, true)
    }

    /// `A.O[Index]`.
    pub fn o_at_mut(&mut self, index: usize) -> Result<Option<&mut Json>, DfError> {
        let Json::Arr(items) = self else {
            return Err(index_error(index));
        };
        container_value(items.get_mut(index).ok_or_else(|| index_error(index))?, false)
    }

    /// `A.A[Index]`.
    pub fn a_at_mut(&mut self, index: usize) -> Result<Option<&mut Json>, DfError> {
        let Json::Arr(items) = self else {
            return Err(index_error(index));
        };
        container_value(items.get_mut(index).ok_or_else(|| index_error(index))?, true)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn index(&self, index: usize) -> Option<&Json> {
        match self {
            Json::Arr(items) => items.get(index),
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Json::Arr(items) => items.len(),
            Json::Obj(entries) => entries.len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `ToJSON(aCompact)`.
    pub fn to_text(&self, compact: bool) -> String {
        let mut out = String::new();
        write_value(self, compact, 0, &mut out);
        if !compact {
            out.push('\n');
        }
        out
    }

    pub fn parse(text: &str) -> Result<Json, DfError> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            pos: 0,
        };
        parser.skip_space();
        let value = parser.value()?;
        parser.skip_space();
        if parser.pos != parser.bytes.len() {
            return Err(parser.error("unexpected data after the value"));
        }
        Ok(value)
    }
}

/// `ObjectValue` and `ArrayValue`: `null` is a nil object, which reads as
/// no object and fails as an array.
fn container_value(value: &mut Json, array: bool) -> Result<Option<&mut Json>, DfError> {
    match (value, array) {
        (Json::Null, false) => Ok(None),
        (value @ Json::Obj(_), false) | (value @ Json::Arr(_), true) => Ok(Some(value)),
        (value, true) => Err(cast_error(value.type_name(), "Array")),
        (value, false) => Err(cast_error(value.type_name(), "Object")),
    }
}

fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push('\t');
    }
}

fn write_value(value: &Json, compact: bool, depth: usize, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Num(text) => out.push_str(text),
        Json::Str(text) => write_string(text, out),
        Json::Arr(items) => {
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
                    out.push('\n');
                    indent(depth + 1, out);
                }
                write_value(item, compact, depth + 1, out);
            }
            if !compact {
                out.push('\n');
                indent(depth, out);
            }
            out.push(']');
        }
        Json::Obj(entries) => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                if !compact {
                    out.push('\n');
                    indent(depth + 1, out);
                }
                write_string(key, out);
                out.push(':');
                if !compact {
                    out.push(' ');
                }
                write_value(item, compact, depth + 1, out);
            }
            if !compact {
                out.push('\n');
                indent(depth, out);
            }
            out.push('}');
        }
    }
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> DfError {
        DfError::new(format!("JSON: {message} at position {}", self.pos))
    }

    fn skip_space(&mut self) {
        while self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\r' | b'\n') {
            self.pos += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.bytes.get(self.pos) == Some(&byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Json, DfError> {
        match self.bytes.get(self.pos) {
            Some(b'{') => {
                self.pos += 1;
                let mut entries: Vec<(String, Json)> = Vec::new();
                self.skip_space();
                if self.eat(b'}') {
                    return Ok(Json::Obj(entries));
                }
                loop {
                    self.skip_space();
                    let key = self.string()?;
                    self.skip_space();
                    if !self.eat(b':') {
                        return Err(self.error("expected ':'"));
                    }
                    self.skip_space();
                    // The library keeps a repeated key; lookups find the first.
                    let value = self.value()?;
                    entries.push((key, value));
                    self.skip_space();
                    if self.eat(b',') {
                        continue;
                    }
                    if self.eat(b'}') {
                        return Ok(Json::Obj(entries));
                    }
                    return Err(self.error("expected ',' or '}'"));
                }
            }
            Some(b'[') => {
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_space();
                if self.eat(b']') {
                    return Ok(Json::Arr(items));
                }
                loop {
                    self.skip_space();
                    items.push(self.value()?);
                    self.skip_space();
                    if self.eat(b',') {
                        continue;
                    }
                    if self.eat(b']') {
                        return Ok(Json::Arr(items));
                    }
                    return Err(self.error("expected ',' or ']'"));
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') if self.bytes[self.pos..].starts_with(b"true") => {
                self.pos += 4;
                Ok(Json::Bool(true))
            }
            Some(b'f') if self.bytes[self.pos..].starts_with(b"false") => {
                self.pos += 5;
                Ok(Json::Bool(false))
            }
            Some(b'n') if self.bytes[self.pos..].starts_with(b"null") => {
                self.pos += 4;
                Ok(Json::Null)
            }
            Some(byte) if *byte == b'-' || byte.is_ascii_digit() => {
                let start = self.pos;
                while self.pos < self.bytes.len()
                    && matches!(self.bytes[self.pos], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.pos += 1;
                }
                Ok(Json::Num(
                    String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned(),
                ))
            }
            _ => Err(self.error("unexpected character")),
        }
    }

    fn string(&mut self) -> Result<String, DfError> {
        if !self.eat(b'"') {
            return Err(self.error("expected a string"));
        }
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&byte) = self.bytes.get(self.pos) else {
                return Err(self.error("unterminated string"));
            };
            self.pos += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(&escape) = self.bytes.get(self.pos) else {
                        return Err(self.error("unterminated string"));
                    };
                    self.pos += 1;
                    let ch = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hex = self
                                .bytes
                                .get(self.pos..self.pos + 4)
                                .and_then(|hex| std::str::from_utf8(hex).ok())
                                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                                .ok_or_else(|| self.error("invalid \\u escape"))?;
                            self.pos += 4;
                            let mut code = hex;
                            if (0xD800..0xDC00).contains(&hex)
                                && self.bytes.get(self.pos..self.pos + 2) == Some(b"\\u")
                                && let Some(low) = self
                                    .bytes
                                    .get(self.pos + 2..self.pos + 6)
                                    .and_then(|hex| std::str::from_utf8(hex).ok())
                                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                                    .filter(|low| (0xDC00..0xE000).contains(low))
                            {
                                self.pos += 6;
                                code = 0x10000 + ((hex - 0xD800) << 10) + (low - 0xDC00);
                            }
                            char::from_u32(code).unwrap_or('\u{FFFD}')
                        }
                        _ => return Err(self.error("invalid escape")),
                    };
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
                }
                byte => out.push(byte),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_form() {
        let mut root = Json::object();
        let mut inner = Json::object();
        inner.set("A", Json::Str("x\\y".to_owned()));
        root.set("Block", inner);
        root.set("Empty", Json::array());
        assert_eq!(
            root.to_text(false),
            "{\n\t\"Block\": {\n\t\t\"A\": \"x\\\\y\"\n\t},\n\t\"Empty\": []\n}\n"
        );
    }

    #[test]
    fn set_replaces_in_place() {
        let mut root = Json::object();
        root.set("A", Json::Str("1".to_owned()));
        root.set("B", Json::Str("2".to_owned()));
        root.set("A", Json::Str("3".to_owned()));
        assert_eq!(root.to_text(true), "{\"A\":\"3\",\"B\":\"2\"}");
    }

    #[test]
    fn parses_back() {
        let text = "{\"a\": [\"1\", {\"b\": true}], \"c\": -1.5e3}";
        let value = Json::parse(text).unwrap();
        assert_eq!(value.get("c").unwrap().value_float().unwrap(), -1500.0);
        assert!(
            value
                .get("a")
                .unwrap()
                .index(1)
                .unwrap()
                .get("b")
                .unwrap()
                .value_bool()
                .unwrap()
        );
    }

    #[test]
    fn reads_like_the_library() {
        let mut value = Json::parse(
            r#"{"a": ["x"], "n": null, "i": 12, "l": 1234567890123, "f": 1.50, "t": true, "d": "1", "d": "2"}"#,
        )
        .unwrap();
        assert_eq!(value.s("a").unwrap_err().0, "Cannot cast Array into String");
        assert_eq!(value.s("n").unwrap_err().0, "Cannot cast Object into String");
        assert_eq!(value.s("i").unwrap(), "12");
        assert_eq!(value.s("f").unwrap(), "1.5");
        assert_eq!(value.s("t").unwrap(), "true");
        assert_eq!(value.s("missing").unwrap(), "");
        assert_eq!(value.s("d").unwrap(), "1");
        assert_eq!(value.i("f").unwrap(), 1);
        assert!(value.o_mut("n").unwrap().is_none());
        assert_eq!(value.a_mut("n").unwrap_err().0, "Cannot cast Object into Array");
        assert_eq!(value.o_mut("l").unwrap_err().0, "Cannot cast Long into Object");
        assert_eq!(value.o_mut("i").unwrap_err().0, "Cannot cast Integer into Object");
        assert!(value.o_mut("new").unwrap().is_some());
        assert_eq!(value.s("new").unwrap_err().0, "Cannot cast Object into String");
        assert_eq!(value.names().len(), 9);
    }
}
