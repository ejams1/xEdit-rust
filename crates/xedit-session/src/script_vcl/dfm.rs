// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreterFm.pas
// (JvInterpreterReadComponentRes, which converts a text DFM to a resource
// stream and reads it with the VCL's TReader)

//! The text form of a Delphi `.dfm` file, as `JvInterpreterFm.pas` reads it
//! to build a form (`JvInterpreterReadComponentRes` -> `TReader`).
//!
//! [`parse`] turns the text form into a [`Component`] tree: the root
//! component, its design-time properties and the nested components. The VCL
//! object model applies the tree in `interactive.rs`; the parser knows no
//! VCL classes, so the whole grammar is testable on its own.
//!
//! What a text DFM holds (the `ObjectTextToBinary` grammar) and what this
//! parser reads:
//!
//! * `object`/`inherited`/`inline <name>: <class>` ... `end` components,
//!   nested as written;
//! * `name = value` properties, a name with dots (`Font.Height`) kept whole;
//! * string expressions -- `'text'` with `''`, `#13`/`#$0D` character
//!   codes, adjacent parts, a `+` continuation across a line break;
//! * integers (`42`, `-7`, `$FF`) and floats (`1.5`, `-2E-3`);
//! * identifiers (`True`, `False`, `nil`, an enum value such as `bsSingle`);
//! * sets (`[fsBold, fsItalic]`, `[]`, and the one-byte `[...]` forms);
//! * string lists (`( 'a' 'b' )`, written one per line by the VCL);
//! * collections (`< item ... end item ... end >`) and binary values
//!   (`{ 01 02 }`).
//!
//! The binary `TPF0` form is not read here: upstream converts a text DFM to
//! binary and reads both through `TReader`, while the port reads the text
//! form only (the form scripts of the corpus ship text DFMs and the fixture
//! is text); a `TPF0` stream is refused with a message naming it.

/// One property value of a DFM component.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A string expression.
    Str(Vec<u8>),
    /// An integer (`42`, `-7`, `$FF`).
    Integer(i64),
    /// A floating point value (`1.5`, `-2E-3`).
    Float(f64),
    /// `True`/`False`, `nil` and an enum value's identifier.
    Ident(String),
    /// A set value, with the identifiers as written.
    Set(Vec<String>),
    /// A string list (`TStrings` property value).
    List(Vec<Vec<u8>>),
    /// A collection (`< item ... end >`): each item is a component without a
    /// name or class.
    Collection(Vec<Component>),
    /// A binary property value.
    Binary(Vec<u8>),
}

/// One component of a DFM: its class, its name (empty for a collection
/// item), its properties in file order and its nested components.
#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    pub class_name: String,
    pub name: String,
    pub properties: Vec<(String, Value)>,
    pub children: Vec<Component>,
}

/// The streaming header of a binary DFM (`TPF0`).
const BINARY_HEADER: &[u8] = b"TPF0";

/// Parses the text form of a DFM. A binary (`TPF0`) stream is refused.
pub fn parse(text: &[u8]) -> Result<Component, String> {
    if text.starts_with(BINARY_HEADER) {
        return Err("binary DFM (TPF0) is not read by the headless form runner".to_owned());
    }
    let mut parser = Parser { text, pos: 0 };
    // A UTF-8 byte order mark precedes some files; TStringList.LoadFromFile
    // would take it as text, and the first token is unaffected.
    if parser.text.starts_with(&[0xEF, 0xBB, 0xBF]) {
        parser.pos = 3;
    }
    parser.skip_space();
    let component = parser.parse_component()?;
    parser.skip_space();
    if parser.pos != parser.text.len() {
        return Err(parser.error("unexpected text after the root component"));
    }
    Ok(component)
}

struct Parser<'a> {
    text: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> String {
        // The line number of the position, as a compiler would report it.
        let line = self.text[..self.pos.min(self.text.len())]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1;
        format!("DFM line {line}: {message}")
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek();
        if byte.is_some() {
            self.pos += 1;
        }
        byte
    }

    fn skip_space(&mut self) {
        while let Some(byte) = self.peek() {
            if matches!(byte, b' ' | b'\t' | b'\r' | b'\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// The next word (a run of letters, digits and `_`). The position is
    /// restored when the run is empty.
    fn word(&mut self) -> Option<String> {
        let start = self.pos;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return None;
        }
        Some(String::from_utf8_lossy(&self.text[start..self.pos]).into_owned())
    }

    /// The next word without consuming it.
    fn peek_word(&mut self) -> Option<String> {
        let pos = self.pos;
        let word = self.word();
        self.pos = pos;
        word
    }

    /// The next identifier (letters, digits, `_`, `.` and `-` inside a
    /// class name such as `TButton`); a property name with dots is one
    /// identifier.
    fn identifier(&mut self) -> Option<String> {
        let start = self.pos;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.') {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return None;
        }
        Some(String::from_utf8_lossy(&self.text[start..self.pos]).into_owned())
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        self.skip_space();
        match self.bump() {
            Some(found) if found == byte => Ok(()),
            Some(found) => Err(self.error(&format!("'{}' expected, '{}' found", byte as char, found as char))),
            None => Err(self.error(&format!("'{}' expected, end of file found", byte as char))),
        }
    }

    /// `object Name: Class ... end`, `inherited ...` or `inline ...`; the
    /// root component's name and class are read the same way (the name may
    /// be empty, `object : TForm`).
    fn parse_component(&mut self) -> Result<Component, String> {
        self.skip_space();
        let keyword = self
            .word()
            .ok_or_else(|| self.error("a component expected (object, inherited or inline)"))?;
        if !matches!(keyword.to_ascii_lowercase().as_str(), "object" | "inherited" | "inline") {
            return Err(self.error(&format!("'object' expected, '{keyword}' found")));
        }
        self.skip_space();
        let name = self.identifier().unwrap_or_default();
        self.skip_space();
        self.expect(b':')?;
        self.skip_space();
        let class_name = self
            .identifier()
            .ok_or_else(|| self.error("a class name expected after ':'"))?;
        let mut component = Component {
            class_name,
            name,
            properties: Vec::new(),
            children: Vec::new(),
        };
        loop {
            self.skip_space();
            let Some(word) = self.peek_word() else {
                return Err(self.error("'end' expected, end of file found"));
            };
            let lower = word.to_ascii_lowercase();
            if lower == "end" {
                let _ = self.word();
                return Ok(component);
            }
            if matches!(lower.as_str(), "object" | "inherited" | "inline") {
                component.children.push(self.parse_component()?);
                continue;
            }
            let property = self
                .identifier()
                .ok_or_else(|| self.error("a property name expected"))?;
            self.skip_space();
            self.expect(b'=')?;
            let value = self.parse_value()?;
            component.properties.push((property, value));
        }
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        self.skip_space();
        match self.peek() {
            Some(b'\'') | Some(b'#') => Ok(Value::Str(self.parse_string_expression()?)),
            Some(b'[') => self.parse_set(),
            Some(b'(') => self.parse_list(),
            Some(b'<') => self.parse_collection(),
            Some(b'{') => self.parse_binary(),
            Some(b'$') | Some(b'-') | Some(b'0'..=b'9') => self.parse_number(),
            _ => {
                let word = self
                    .identifier()
                    .ok_or_else(|| self.error("a property value expected"))?;
                Ok(Value::Ident(word))
            }
        }
    }

    /// A string expression: one or more `'...'`, `#nn` and `#$nn` parts,
    /// optionally split with a trailing `+`.
    fn parse_string_expression(&mut self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut first = true;
        let mut after_plus = false;
        loop {
            if first || after_plus {
                self.skip_space();
                first = false;
                after_plus = false;
            }
            match self.peek() {
                Some(b'\'') => {
                    let _ = self.bump();
                    loop {
                        match self.bump() {
                            Some(b'\'') => {
                                if self.peek() == Some(b'\'') {
                                    // A doubled quote is one quote.
                                    let _ = self.bump();
                                    out.push(b'\'');
                                } else {
                                    break;
                                }
                            }
                            Some(byte) => out.push(byte),
                            None => return Err(self.error("unterminated string")),
                        }
                    }
                }
                Some(b'#') => {
                    let _ = self.bump();
                    let code = if self.peek() == Some(b'$') {
                        let _ = self.bump();
                        let start = self.pos;
                        while self.peek().is_some_and(|byte| byte.is_ascii_hexdigit()) {
                            self.pos += 1;
                        }
                        let text = String::from_utf8_lossy(&self.text[start..self.pos]).into_owned();
                        u32::from_str_radix(&text, 16).map_err(|_| self.error("a character code expected after '#'"))?
                    } else {
                        let start = self.pos;
                        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                            self.pos += 1;
                        }
                        let text = String::from_utf8_lossy(&self.text[start..self.pos]).into_owned();
                        text.parse::<u32>()
                            .map_err(|_| self.error("a character code expected after '#'"))?
                    };
                    match char::from_u32(code) {
                        Some(character) => {
                            let mut buffer = [0u8; 4];
                            out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                        }
                        None => return Err(self.error(&format!("invalid character code {code}"))),
                    }
                }
                _ => break,
            }
            let after_part = self.pos;
            // Peek past the whitespace that follows the part: a `+` there
            // continues the expression, an adjacent part (no whitespace)
            // does too, and a whitespace alone separates list entries.
            let mut probe = self.pos;
            while probe < self.text.len() && self.text[probe].is_ascii_whitespace() {
                probe += 1;
            }
            if probe < self.text.len() && self.text[probe] == b'+' {
                self.pos = probe + 1;
                after_plus = true;
                continue;
            }
            if probe == after_part && matches!(self.peek(), Some(b'\'' | b'#')) {
                continue;
            }
            break;
        }
        Ok(out)
    }

    fn parse_set(&mut self) -> Result<Value, String> {
        self.expect(b'[')?;
        let mut members = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                Some(b']') => {
                    let _ = self.bump();
                    return Ok(Value::Set(members));
                }
                None => return Err(self.error("']' expected, end of file found")),
                _ => {
                    let member = self
                        .identifier()
                        .or_else(|| self.word())
                        .ok_or_else(|| self.error("a set member expected"))?;
                    members.push(member);
                    self.skip_space();
                    if self.peek() == Some(b',') {
                        let _ = self.bump();
                    }
                }
            }
        }
    }

    fn parse_list(&mut self) -> Result<Value, String> {
        self.expect(b'(')?;
        let mut lines = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                Some(b')') => {
                    let _ = self.bump();
                    return Ok(Value::List(lines));
                }
                None => return Err(self.error("')' expected, end of file found")),
                _ => lines.push(self.parse_string_expression()?),
            }
        }
    }

    /// `< item ... end item ... end >`: each `item` is a component whose
    /// name and class are empty (the collection's item class).
    fn parse_collection(&mut self) -> Result<Value, String> {
        self.expect(b'<')?;
        let mut items = Vec::new();
        loop {
            self.skip_space();
            if self.peek() == Some(b'>') {
                let _ = self.bump();
                return Ok(Value::Collection(items));
            }
            let Some(word) = self.word() else {
                return Err(self.error("'item' or '>' expected, end of file found"));
            };
            match word.to_ascii_lowercase().as_str() {
                "item" => {
                    let mut item = Component {
                        class_name: String::new(),
                        name: String::new(),
                        properties: Vec::new(),
                        children: Vec::new(),
                    };
                    loop {
                        self.skip_space();
                        let Some(word) = self.peek_word() else {
                            return Err(self.error("'end' expected, end of file found"));
                        };
                        let lower = word.to_ascii_lowercase();
                        if lower == "end" {
                            let _ = self.word();
                            break;
                        }
                        if matches!(lower.as_str(), "object" | "inherited" | "inline") {
                            item.children.push(self.parse_component()?);
                            continue;
                        }
                        let property = self
                            .identifier()
                            .ok_or_else(|| self.error("a property name expected"))?;
                        self.skip_space();
                        self.expect(b'=')?;
                        let value = self.parse_value()?;
                        item.properties.push((property, value));
                    }
                    items.push(item);
                }
                other => return Err(self.error(&format!("'item' or '>' expected, '{other}' found"))),
            }
        }
    }

    fn parse_binary(&mut self) -> Result<Value, String> {
        self.expect(b'{')?;
        let mut bytes = Vec::new();
        let mut high: Option<u8> = None;
        loop {
            match self.bump() {
                Some(b'}') => {
                    if high.is_some() {
                        return Err(self.error("an odd number of hexadecimal digits"));
                    }
                    return Ok(Value::Binary(bytes));
                }
                Some(byte) if byte.is_ascii_hexdigit() => {
                    let digit = (byte as char).to_digit(16).expect("hex digit") as u8;
                    match high.take() {
                        Some(first) => bytes.push(first << 4 | digit),
                        None => high = Some(digit),
                    }
                }
                Some(byte) if byte.is_ascii_whitespace() => {}
                Some(byte) => return Err(self.error(&format!("unexpected '{}' in a binary value", byte as char))),
                None => return Err(self.error("'}' expected, end of file found")),
            }
        }
    }

    fn parse_number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            let _ = self.bump();
        }
        if self.peek() == Some(b'$') {
            let _ = self.bump();
            let digits = self.pos;
            while self.peek().is_some_and(|byte| byte.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            let text = String::from_utf8_lossy(&self.text[digits..self.pos]).into_owned();
            let value = i64::from_str_radix(&text, 16).map_err(|_| self.error("an invalid hexadecimal number"))?;
            let negative = self.text[start] == b'-';
            return Ok(Value::Integer(if negative { -value } else { value }));
        }
        let mut float = false;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.peek() == Some(b'.') {
            float = true;
            let _ = self.bump();
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'E' | b'e')) {
            float = true;
            let _ = self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                let _ = self.bump();
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let text = String::from_utf8_lossy(&self.text[start..self.pos]).into_owned();
        if float {
            text.parse::<f64>()
                .map(Value::Float)
                .map_err(|_| self.error(&format!("an invalid number '{text}'")))
        } else {
            text.parse::<i64>()
                .map(Value::Integer)
                .map_err(|_| self.error(&format!("an invalid number '{text}'")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop<'a>(component: &'a Component, name: &str) -> &'a Value {
        &component
            .properties
            .iter()
            .find(|(property, _)| property == name)
            .unwrap_or_else(|| panic!("no property {name}"))
            .1
    }

    /// The shape `JvInterpreterMakeForm` reads: the root, its properties and
    /// a nested control with the properties the modal loop needs.
    #[test]
    fn a_text_dfm_is_read() {
        let text = b"object frmFixture: TForm\r\n\
  Caption = 'Fixture Form'\r\n\
  Left = 10\r\n\
  ClientWidth = 320\r\n\
  KeyPreview = True\r\n\
  object Button1: TButton\r\n\
    Caption = 'OK'\r\n\
    ModalResult = 1\r\n\
    Default = True\r\n\
  end\r\n\
end\r\n";
        let component = parse(text).expect("the DFM parses");
        assert_eq!(component.class_name, "TForm");
        assert_eq!(component.name, "frmFixture");
        assert_eq!(prop(&component, "Caption"), &Value::Str(b"Fixture Form".to_vec()));
        assert_eq!(prop(&component, "Left"), &Value::Integer(10));
        assert_eq!(prop(&component, "KeyPreview"), &Value::Ident("True".to_owned()));
        assert_eq!(component.children.len(), 1);
        let button = &component.children[0];
        assert_eq!(button.class_name, "TButton");
        assert_eq!(button.name, "Button1");
        assert_eq!(prop(button, "ModalResult"), &Value::Integer(1));
    }

    /// The string grammar: doubled quotes, character codes, adjacent parts
    /// and a `+` continuation.
    #[test]
    fn strings_and_character_codes_are_read() {
        let text = b"object A: TForm\r\n  Caption = 'it''s'#13#10'ok'\r\n  Text = 'a' +\r\n    'b'\r\nend\r\n";
        let component = parse(text).expect("the DFM parses");
        assert_eq!(prop(&component, "Caption"), &Value::Str(b"it's\r\nok".to_vec()));
        assert_eq!(prop(&component, "Text"), &Value::Str(b"ab".to_vec()));
    }

    /// A string list, a set, a collection and a binary value.
    #[test]
    fn lists_sets_collections_and_binary_parse() {
        let text = b"object A: TForm\r\n\
  Items.Strings = (\r\n    'one'\r\n    'two')\r\n\
  Style = [csDropDown, csOwnerDrawFixed]\r\n\
  Columns = <\r\n    item\r\n      Caption = 'A'\r\n      Width = 70\r\n    end>\r\n\
  Picture.Data = { 0102 FF }\r\n\
end\r\n";
        let component = parse(text).expect("the DFM parses");
        assert_eq!(
            prop(&component, "Items.Strings"),
            &Value::List(vec![b"one".to_vec(), b"two".to_vec()])
        );
        assert_eq!(
            prop(&component, "Style"),
            &Value::Set(vec!["csDropDown".to_owned(), "csOwnerDrawFixed".to_owned()])
        );
        let Value::Collection(items) = prop(&component, "Columns") else {
            panic!("a collection expected");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].properties[0],
            ("Caption".to_owned(), Value::Str(b"A".to_vec()))
        );
        assert_eq!(prop(&component, "Picture.Data"), &Value::Binary(vec![1, 2, 0xFF]));
    }

    #[test]
    fn numbers_read_as_integers_and_floats() {
        let text =
            b"object A: TForm\r\n  Tag = -3\r\n  AlphaBlendValue = $FF\r\n  Scale = 1.5\r\n  Bias = -2E-3\r\nend\r\n";
        let component = parse(text).expect("the DFM parses");
        assert_eq!(prop(&component, "Tag"), &Value::Integer(-3));
        assert_eq!(prop(&component, "AlphaBlendValue"), &Value::Integer(255));
        assert_eq!(prop(&component, "Scale"), &Value::Float(1.5));
        assert_eq!(prop(&component, "Bias"), &Value::Float(-0.002));
    }

    /// A binary stream is refused with a message naming it, and a broken
    /// text names its line.
    #[test]
    fn a_binary_dfm_and_broken_text_are_refused() {
        assert!(parse(b"TPF0\x00\x01").unwrap_err().contains("binary DFM"));
        let error = parse(b"object A: TForm\r\n  Caption = 'x\r\nend\r\n").unwrap_err();
        assert!(error.contains("unterminated string"), "{error}");
        let error = parse(b"object A TForm\r\nend\r\n").unwrap_err();
        assert!(error.contains("':' expected"), "{error}");
    }
}
