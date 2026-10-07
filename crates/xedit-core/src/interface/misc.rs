// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! Free functions of `wbInterface.pas` and helpers for Delphi string semantics.

use std::sync::{Arc, RwLock};

/// A variable of a definition unit: `wbFoo: IwbStructDef` becomes
/// `static WB_FOO: Global<Option<Arc<StructDef>>>`. Reads clone the value.
pub struct Global<T>(RwLock<Option<T>>);

impl<T: Clone + Default> Global<T> {
    pub const fn new() -> Self {
        Global(RwLock::new(None))
    }

    pub fn get(&self) -> T {
        self.0.read().unwrap().clone().unwrap_or_default()
    }

    pub fn set(&self, value: T) {
        *self.0.write().unwrap() = Some(value);
    }
}

impl<T: Clone + Default> Global<Vec<T>> {
    /// Upstream `SetLength` on the variable.
    pub fn set_length(&self, length: i32) {
        let mut guard = self.0.write().unwrap();
        guard
            .get_or_insert_with(Vec::new)
            .resize(usize::try_from(length).unwrap_or(0), T::default());
    }

    /// Upstream `variable[index] := value`.
    pub fn set_at(&self, index: i32, value: T) {
        let mut guard = self.0.write().unwrap();
        let index = usize::try_from(index).expect("a non-negative index");
        guard.get_or_insert_with(Vec::new)[index] = value;
    }
}

impl<T: Clone + Default> Default for Global<T> {
    fn default() -> Self {
        Self::new()
    }
}

use super::element::ElementArg;

/// A value that upstream passes as a Delphi `Variant`.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Variant {
    /// `Unassigned`, the result of `VarClear`.
    #[default]
    Empty,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
    /// A byte array (`TBytes`).
    Bytes(Vec<u8>),
}

macro_rules! variant_from {
    ($($ty:ty => $variant:ident),* $(,)?) => {
        $(
            impl From<$ty> for Variant {
                fn from(value: $ty) -> Self {
                    Variant::$variant(value.into())
                }
            }
        )*
    };
}

variant_from!(bool => Bool, i8 => Int, i16 => Int, i32 => Int, i64 => Int, u8 => UInt, u16 => UInt, u32 => UInt, u64 => UInt, f32 => Float, f64 => Float, String => Str, &str => Str);

/// The receiver of progress messages, upstream `_wbProgressCallback`.
pub type ProgressCallback = Arc<dyn Fn(&str) + Send + Sync>;

static PROGRESS_CALLBACK: RwLock<Option<ProgressCallback>> = RwLock::new(None);

pub fn set_progress_callback(callback: Option<ProgressCallback>) {
    *PROGRESS_CALLBACK.write().unwrap() = callback;
}

/// Port of `wbProgress`: sends a status message to the progress callback.
pub fn progress(status: &str) {
    let callback = PROGRESS_CALLBACK.read().unwrap().clone();
    if let Some(callback) = callback {
        callback(status);
    }
}

/// The lookup of localized strings, upstream `wbLocalizationHandler`. The
/// implementation is the port of `wbLocalization.pas`.
pub trait LocalizationHandler: Send + Sync {
    /// Port of `GetValue`: whether the string `id` was found, and its text or
    /// the error text to show in its place.
    fn get_value(&self, id: u32, element: ElementArg) -> (bool, String);
}

static LOCALIZATION_HANDLER: RwLock<Option<Arc<dyn LocalizationHandler>>> = RwLock::new(None);

pub fn set_localization_handler(handler: Option<Arc<dyn LocalizationHandler>>) {
    *LOCALIZATION_HANDLER.write().unwrap() = handler;
}

/// `wbLocalizationHandler.GetValue`. Nothing is found while no handler is set.
pub fn localization_get_value(id: u32, element: ElementArg) -> (bool, String) {
    let handler = LOCALIZATION_HANDLER.read().unwrap().clone();
    match handler {
        Some(handler) => handler.get_value(id, element),
        None => (false, String::new()),
    }
}

impl Variant {
    /// The value when `VarIsOrdinal` holds, converted as `Int64(variant)`.
    pub fn as_ordinal(&self) -> Option<i64> {
        match self {
            Variant::Bool(value) => Some(if *value { -1 } else { 0 }),
            Variant::Int(value) => Some(*value),
            Variant::UInt(value) => Some(*value as i64),
            Variant::Empty | Variant::Float(_) | Variant::Str(_) | Variant::Bytes(_) => None,
        }
    }
}

/// Port of `StrToIntDef` with a 32-bit result: decimal digits with an optional
/// sign, or hexadecimal digits after `$` or `0x`.
pub fn str_to_int_def(text: &str, default: i32) -> i32 {
    let text = text.trim_start_matches(' ');
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let hex = digits
        .strip_prefix('$')
        .or_else(|| digits.strip_prefix("0x"))
        .or_else(|| digits.strip_prefix("0X"));
    let parsed = match hex {
        // Eight hexadecimal digits fill the 32 bits, sign bit included.
        Some(hex) if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
            u32::from_str_radix(hex, 16).ok().map(|value| {
                let value = value as i32;
                if negative { value.wrapping_neg() } else { value }
            })
        }
        Some(_) => None,
        None if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            let sign = if negative { "-" } else { "" };
            format!("{sign}{digits}").parse::<i32>().ok()
        }
        None => None,
    };
    parsed.unwrap_or(default)
}

/// Length of a Delphi `string`: the number of UTF-16 code units.
pub fn length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Delphi `SetLength` on a string that gets shorter: keeps the first `units`
/// UTF-16 code units.
// A cut inside a surrogate pair drops the whole character, where Delphi keeps
// an unpaired surrogate.
pub fn truncate(text: &mut String, units: usize) {
    let mut seen = 0;
    for (index, character) in text.char_indices() {
        seen += character.len_utf16();
        if seen > units {
            text.truncate(index);
            return;
        }
    }
}

/// Upstream default of the `aPlaceholder` parameter of `ShortenText`.
pub const SHORTEN_TEXT_PLACEHOLDER: &str = "\u{2026}";

/// Port of `ShortenText` with its default parameters.
pub fn shorten_text(text: &str) -> String {
    shorten_text_to(text, 64, SHORTEN_TEXT_PLACEHOLDER)
}

/// Port of `ShortenText`.
pub fn shorten_text_to(text: &str, width: usize, placeholder: &str) -> String {
    // UPSTREAM-QUIRK: the comment upstream says that all whitespace collapses to
    // single spaces, but the pattern '\t\n\v\f\r' is not a character class. It
    // matches only that exact sequence of five characters.
    let mut result = text.replace("\t\n\u{b}\u{c}\r", " ");
    if length(&result) > width {
        truncate(&mut result, width.saturating_sub(length(placeholder)));
        result.push_str(placeholder);
    }
    result
}

/// Port of `IntToHex64`: the 64-bit two's complement of `value` in upper-case
/// hexadecimal with at least `digits` digits.
pub fn int_to_hex64(value: i64, digits: usize) -> String {
    format!("{:0digits$X}", value as u64)
}

/// Port of `wbReadInteger24`: three bytes, most significant first.
// UPSTREAM-QUIRK: upstream copies eight bytes from a four-byte buffer into the
// result, so its upper half is whatever follows the buffer on the stack. The
// port returns the value with a zero upper half.
pub fn read_integer24(data: &[u8]) -> i64 {
    (i64::from(data[0]) << 16) | (i64::from(data[1]) << 8) | i64::from(data[2])
}

/// Port of `ReadIntegerCounterSize`. The two least significant bits of the
/// first byte give the length of the counter.
pub fn read_integer_counter_size(data: Option<&[u8]>) -> i64 {
    match data.and_then(|data| data.first()) {
        Some(first) => match first & 3 {
            1 => 2,
            2 => 4,
            _ => 1,
        },
        None => 1,
    }
}

/// Port of `ReadIntegerCounter`: a count of 6, 14 or 30 bits.
pub fn read_integer_counter(data: Option<&[u8]>) -> i64 {
    let Some(data) = data.filter(|data| !data.is_empty()) else {
        return 0;
    };
    let raw = match data[0] & 3 {
        0 => i64::from(data[0]),
        1 => i64::from(u16::from_le_bytes([data[0], data[1]])),
        2 => i64::from(u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
        // Not supposed to exist: zeroed out by the engine.
        _ => 0,
    };
    raw >> 2
}

/// Delphi `ToUpperInvariant` for one character. Characters whose upper case
/// is more than one character stay as they are.
fn to_upper_invariant(text: &str) -> String {
    text.chars()
        .map(|character| {
            let mut upper = character.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(single), None) => single,
                _ => character,
            }
        })
        .collect()
}

/// Port of `wbGetUnknownIntString`: `<Unknown: 5 $5>`. A value of eight
/// hexadecimal digits that reads as an upper-case signature also shows the
/// signature.
pub fn get_unknown_int_string(int: i64) -> String {
    let mut result = format!("<Unknown: {int}");
    if super::globals::extended_int_unknowns() {
        let hex = int_to_hex64(int, 16);
        let hex = hex.trim_start_matches('0');
        if !hex.is_empty() {
            result.push_str(" $");
            result.push_str(hex);
        }
        if hex.len() == 8 {
            let signature = super::types::Signature::from_int(int as u32).to_string();
            let upper = to_upper_invariant(&signature);
            if signature == upper {
                result.push(' ');
                result.push_str(&upper);
            }
        }
    }
    result.push('>');
    result
}

/// The message of the exception upstream raises when an edit fails.
pub type EditError = String;

impl Variant {
    /// Port of `VarIsOrdinal`.
    pub fn is_ordinal(&self) -> bool {
        matches!(self, Variant::Bool(_) | Variant::Int(_) | Variant::UInt(_))
    }

    /// Port of `VarSameValue`: two variants with the same value, numbers
    /// compared as numbers.
    pub fn same_value(&self, other: &Variant) -> bool {
        match (self.as_number(), other.as_number()) {
            (Some(a), Some(b)) => a == b,
            _ => match (self, other) {
                (Variant::Str(a), Variant::Str(b)) => a == b,
                (Variant::Str(a), other) | (other, Variant::Str(a)) => other
                    .as_number()
                    .is_some_and(|number| variant_to_f64(&Variant::Str(a.clone())) == Ok(number)),
                _ => self == other,
            },
        }
    }

    /// The value as a number, for the comparisons of Delphi variants.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Variant::Bool(value) => Some(if *value { -1.0 } else { 0.0 }),
            Variant::Int(value) => Some(*value as f64),
            Variant::UInt(value) => Some(*value as f64),
            Variant::Float(value) => Some(*value),
            Variant::Empty | Variant::Str(_) | Variant::Bytes(_) => None,
        }
    }
}

/// Port of `StrToInt64`: a decimal integer, or a hexadecimal one after `$`
/// or `0x`. The error is the message of the `EConvertError`.
pub fn str_to_int64(text: &str) -> Result<i64, EditError> {
    let trimmed = text.trim_matches(' ');
    let (negative, rest) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let hex = rest
        .strip_prefix('$')
        .or_else(|| rest.strip_prefix("0x"))
        .or_else(|| rest.strip_prefix("0X"));
    let parsed = match hex {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => rest.parse::<u64>().ok(),
    };
    match parsed {
        Some(value) if !rest.is_empty() => {
            let value = value as i64;
            Ok(if negative { value.wrapping_neg() } else { value })
        }
        _ => Err(format!("'{text}' is not a valid integer value")),
    }
}

/// A Delphi `Variant` converted to `Int64`: the number, a boolean as 0 or
/// 1, a float rounded, a string parsed as an integer; `Unassigned` is 0.
pub fn variant_to_i64(value: &Variant) -> Result<i64, EditError> {
    match value {
        Variant::Empty => Ok(0),
        Variant::Bool(value) => Ok(i64::from(*value)),
        Variant::Int(value) => Ok(*value),
        Variant::UInt(value) => Ok(*value as i64),
        Variant::Float(value) => Ok(crate::delphi::round(*value)),
        Variant::Str(text) => str_to_int64(text),
        Variant::Bytes(_) => Err("Could not convert variant of type (Array Byte) into type (Int64)".to_owned()),
    }
}

/// A Delphi `Variant` converted to `Extended`.
pub fn variant_to_f64(value: &Variant) -> Result<f64, EditError> {
    match value {
        Variant::Empty => Ok(0.0),
        Variant::Bool(value) => Ok(if *value { 1.0 } else { 0.0 }),
        Variant::Int(value) => Ok(*value as f64),
        Variant::UInt(value) => Ok(*value as f64),
        Variant::Float(value) => Ok(*value),
        Variant::Str(text) => {
            crate::delphi::str_to_float(text).ok_or_else(|| format!("'{text}' is not a valid floating point value"))
        }
        Variant::Bytes(_) => Err("Could not convert variant of type (Array Byte) into type (Double)".to_owned()),
    }
}

/// A Delphi `Variant` converted to `string`.
pub fn variant_to_string(value: &Variant) -> String {
    match value {
        Variant::Empty => String::new(),
        Variant::Bool(value) => if *value { "True" } else { "False" }.to_owned(),
        Variant::Int(value) => value.to_string(),
        Variant::UInt(value) => value.to_string(),
        Variant::Float(value) => crate::delphi::float_to_str(*value),
        Variant::Str(text) => text.clone(),
        Variant::Bytes(bytes) => bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// Port of `WriteIntegerCounter`: a count of 6, 14 or 30 bits in as many
/// bytes as it needs, most significant byte first. Returns the bytes.
// UPSTREAM-QUIRK: the three byte case shifts the fourth byte of the value
// (always zero there) instead of the third, so a count that needs three
// bytes loses its high bits.
pub fn write_integer_counter(value: i64) -> Vec<u8> {
    let bytes = value.to_le_bytes();
    if bytes[3] > 0 {
        vec![(bytes[3] << 2) | 3, bytes[2], bytes[1], bytes[0]]
    } else if bytes[2] > 0 {
        vec![(bytes[3] << 2) | 2, bytes[1], bytes[0]]
    } else if bytes[1] > 0 {
        vec![(bytes[1] << 2) | 1, bytes[0]]
    } else {
        vec![bytes[0] << 2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_counts_utf16_units() {
        assert_eq!(length("abc"), 3);
        assert_eq!(length("é"), 1);
        assert_eq!(length("\u{1F600}"), 2);
    }

    #[test]
    fn truncate_keeps_units() {
        let mut text = "aé\u{1F600}b".to_owned();
        truncate(&mut text, 10);
        assert_eq!(text, "aé\u{1F600}b");
        truncate(&mut text, 4);
        assert_eq!(text, "aé\u{1F600}");
        truncate(&mut text, 3);
        assert_eq!(text, "aé");
        truncate(&mut text, 0);
        assert_eq!(text, "");
    }

    #[test]
    fn unknown_int_string() {
        let _guard = super::super::globals::test_lock();
        assert_eq!(get_unknown_int_string(0), "<Unknown: 0>");
        assert_eq!(get_unknown_int_string(5), "<Unknown: 5 $5>");
        assert_eq!(get_unknown_int_string(-1), "<Unknown: -1 $FFFFFFFFFFFFFFFF>");
        assert_eq!(
            get_unknown_int_string(i64::from(u32::from_le_bytes(*b"EDID"))),
            "<Unknown: 1145652293 $44494445 EDID>"
        );
        assert_eq!(
            get_unknown_int_string(i64::from(u32::from_le_bytes(*b"edid"))),
            "<Unknown: 1684628581 $64696465>"
        );
        // The signature text ends at a zero byte.
        assert_eq!(get_unknown_int_string(0x4100_0000), "<Unknown: 1090519040 $41000000 >");
        super::super::globals::set_extended_int_unknowns(false);
        assert_eq!(get_unknown_int_string(5), "<Unknown: 5>");
    }

    #[test]
    fn str_to_int_def_forms() {
        assert_eq!(str_to_int_def("42", -1), 42);
        assert_eq!(str_to_int_def("-42", -1), -42);
        assert_eq!(str_to_int_def("$1F", 0), 31);
        assert_eq!(str_to_int_def("0x10", 0), 16);
        assert_eq!(str_to_int_def("$FFFFFFFF", 0), -1);
        assert_eq!(str_to_int_def("$100000000", 7), 7);
        assert_eq!(str_to_int_def("", 7), 7);
        assert_eq!(str_to_int_def("12a", 7), 7);
        assert_eq!(str_to_int_def("99999999999", 7), 7);
        assert_eq!(Variant::UInt(5).as_ordinal(), Some(5));
        assert_eq!(Variant::Str("5".to_owned()).as_ordinal(), None);
    }

    #[test]
    fn integer_helpers() {
        assert_eq!(int_to_hex64(-1, 4), "FFFFFFFFFFFFFFFF");
        assert_eq!(int_to_hex64(0x1AB, 5), "001AB");
        assert_eq!(int_to_hex64(0x1AB, 1), "1AB");
        assert_eq!(read_integer24(&[0x01, 0x02, 0x03]), 0x01_0203);
        assert_eq!(read_integer_counter_size(None), 1);
        assert_eq!(read_integer_counter_size(Some(&[0b11])), 1);
        assert_eq!(read_integer_counter(Some(&[0b11, 0xFF])), 0);
        assert_eq!(read_integer_counter(Some(&[0b0101, 0x01])), 0x41);
        assert_eq!(read_integer_counter(None), 0);
    }

    #[test]
    fn shorten_text_cuts_at_width() {
        assert_eq!(shorten_text("short"), "short");
        let long = "x".repeat(65);
        let shortened = shorten_text(&long);
        assert_eq!(length(&shortened), 64);
        assert!(shortened.ends_with('\u{2026}'));
        assert_eq!(shorten_text(&"x".repeat(64)), "x".repeat(64));
        assert_eq!(shorten_text("a\tb\nc"), "a\tb\nc");
        assert_eq!(shorten_text("a\t\n\u{b}\u{c}\rb"), "a b");
    }
}
