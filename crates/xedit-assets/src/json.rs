// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The JSON documents of the data format units, as the `JsonDataObjects`
//! library that upstream uses builds and prints them: objects keep their
//! keys in insertion order, setting a key that exists replaces its value in
//! place, and the readable form indents with tabs.

use crate::data_format::DfError;

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

    pub fn get(&self, name: &str) -> Option<&Json> {
        match self {
            Json::Obj(entries) => entries.iter().find(|(key, _)| key == name).map(|(_, value)| value),
            _ => None,
        }
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

    /// The value as `S[...]` reads it: a string, a number or a boolean as
    /// text, empty for anything else.
    pub fn as_str(&self) -> String {
        match self {
            Json::Str(text) | Json::Num(text) => text.clone(),
            Json::Bool(true) => "true".to_owned(),
            Json::Bool(false) => "false".to_owned(),
            _ => String::new(),
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Json::Bool(value) => *value,
            Json::Num(text) => text.parse::<f64>().is_ok_and(|value| value != 0.0),
            Json::Str(text) => text.eq_ignore_ascii_case("true"),
            _ => false,
        }
    }

    pub fn as_f64(&self) -> f64 {
        match self {
            Json::Num(text) | Json::Str(text) => text.parse().unwrap_or(0.0),
            Json::Bool(value) => f64::from(u8::from(*value)),
            _ => 0.0,
        }
    }

    pub fn as_i64(&self) -> i64 {
        match self {
            Json::Num(text) | Json::Str(text) => text
                .parse::<i64>()
                .unwrap_or_else(|_| text.parse::<f64>().map_or(0, |value| value as i64)),
            Json::Bool(value) => i64::from(*value),
            _ => 0,
        }
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
                    let value = self.value()?;
                    match entries.iter_mut().find(|(name, _)| *name == key) {
                        Some(entry) => entry.1 = value,
                        None => entries.push((key, value)),
                    }
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
        assert_eq!(value.get("c").unwrap().as_f64(), -1500.0);
        assert!(value.get("a").unwrap().index(1).unwrap().get("b").unwrap().as_bool());
    }
}
