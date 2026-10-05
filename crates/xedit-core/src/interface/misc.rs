// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! Free functions of `wbInterface.pas` and helpers for Delphi string semantics.

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
