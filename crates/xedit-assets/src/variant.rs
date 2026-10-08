// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The parts of the Delphi `Variant` that the data format units rely on:
//! native values of elements, with the conversions and comparisons that
//! Delphi applies when a variant meets an integer, a float or a string.

use std::cmp::Ordering;

use xedit_core::delphi::{float_to_str, str_to_float};

use crate::data_format::DfError;

/// A native value. `Empty` is `Unassigned`: what a lookup of a missing
/// element returns, which converts to 0 and to the empty string.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Variant {
    #[default]
    Empty,
    Int(i64),
    UInt(u64),
    Float(f64),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
}

impl Variant {
    pub fn is_empty(&self) -> bool {
        matches!(self, Variant::Empty)
    }

    /// Variant to integer: a float rounds half to even, a string is parsed
    /// as `StrToInt`, `True` is -1.
    pub fn to_i64(&self) -> Result<i64, DfError> {
        Ok(match self {
            Variant::Empty => 0,
            Variant::Int(value) => *value,
            Variant::UInt(value) => *value as i64,
            Variant::Float(value) => value.round_ties_even() as i64,
            Variant::Bool(value) => -i64::from(*value),
            Variant::Str(text) => match str_to_int64(text) {
                Some(value) => value,
                None => match str_to_float(text) {
                    Some(value) => value.round_ties_even() as i64,
                    None => {
                        return Err(DfError::new(
                            "Could not convert variant of type (UnicodeString) into type (Int64)",
                        ));
                    }
                },
            },
            Variant::Bytes(_) => return Err(DfError::new("Invalid variant type conversion")),
        })
    }

    pub fn to_u64(&self) -> Result<u64, DfError> {
        match self {
            Variant::UInt(value) => Ok(*value),
            _ => Ok(self.to_i64()? as u64),
        }
    }

    pub fn to_i32(&self) -> Result<i32, DfError> {
        Ok(self.to_i64()? as i32)
    }

    pub fn to_f64(&self) -> Result<f64, DfError> {
        Ok(match self {
            Variant::Empty => 0.0,
            Variant::Int(value) => *value as f64,
            Variant::UInt(value) => *value as f64,
            Variant::Float(value) => *value,
            Variant::Bool(value) => -f64::from(u8::from(*value)),
            Variant::Str(text) => match str_to_float(text) {
                Some(value) => value,
                None => {
                    return Err(DfError::new(
                        "Could not convert variant of type (UnicodeString) into type (Double)",
                    ));
                }
            },
            Variant::Bytes(_) => return Err(DfError::new("Invalid variant type conversion")),
        })
    }

    pub fn to_bool(&self) -> Result<bool, DfError> {
        Ok(match self {
            Variant::Bool(value) => *value,
            Variant::Str(text) if text.eq_ignore_ascii_case("true") => true,
            Variant::Str(text) if text.eq_ignore_ascii_case("false") => false,
            other => other.to_f64()? != 0.0,
        })
    }

    /// Variant to string, as `string(V)`: integers in decimal, floats as
    /// `FloatToStr`, booleans as `True` and `False`.
    pub fn to_str(&self) -> Result<String, DfError> {
        Ok(match self {
            Variant::Empty => String::new(),
            Variant::Int(value) => value.to_string(),
            Variant::UInt(value) => value.to_string(),
            Variant::Float(value) => float_to_str(*value),
            Variant::Bool(true) => "True".to_owned(),
            Variant::Bool(false) => "False".to_owned(),
            Variant::Str(text) => text.clone(),
            Variant::Bytes(_) => return Err(DfError::new("Invalid variant type conversion")),
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, DfError> {
        match self {
            Variant::Bytes(bytes) => Ok(bytes.clone()),
            Variant::Empty => Ok(Vec::new()),
            _ => Err(DfError::new("Invalid variant type conversion")),
        }
    }

    /// Delphi's comparison of two variants: numbers compare as numbers, a
    /// string against a number is converted to a number, two strings
    /// compare as strings.
    pub fn compare(&self, other: &Variant) -> Result<Ordering, DfError> {
        match (self, other) {
            (Variant::Str(a), Variant::Str(b)) => Ok(a.cmp(b)),
            (Variant::Int(a), Variant::Int(b)) => Ok(a.cmp(b)),
            (Variant::UInt(a), Variant::UInt(b)) => Ok(a.cmp(b)),
            (Variant::Int(a), Variant::UInt(b)) => Ok(i128::from(*a).cmp(&i128::from(*b))),
            (Variant::UInt(a), Variant::Int(b)) => Ok(i128::from(*a).cmp(&i128::from(*b))),
            (Variant::Empty, Variant::Empty) => Ok(Ordering::Equal),
            (a, b) => {
                let (a, b) = (a.to_f64()?, b.to_f64()?);
                Ok(a.partial_cmp(&b).unwrap_or(Ordering::Less))
            }
        }
    }
}

/// `StrToInt64`: decimal or `$` hexadecimal, with an optional sign.
pub fn str_to_int64(text: &str) -> Option<i64> {
    let text = text.trim_matches(' ');
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let value = if let Some(hex) = digits
        .strip_prefix('$')
        .or_else(|| digits.strip_prefix("0x"))
        .or_else(|| digits.strip_prefix("0X"))
    {
        if hex.is_empty() {
            return None;
        }
        u64::from_str_radix(hex, 16).ok()? as i64
    } else {
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let value: u64 = digits.parse().ok()?;
        if value > i64::MAX as u64 && !(negative && value == 1 << 63) {
            return None;
        }
        value as i64
    };
    Some(if negative { value.wrapping_neg() } else { value })
}

/// `StrToInt`: as `StrToInt64`, but the value must fit 32 bits. A
/// hexadecimal value is read as 32 bits and may set the sign bit.
pub fn str_to_int(text: &str) -> Option<i32> {
    let trimmed = text.trim_matches(' ');
    let unsigned = trimmed.trim_start_matches(['-', '+']);
    if let Some(hex) = unsigned.strip_prefix('$') {
        let value = u32::from_str_radix(hex, 16).ok()? as i32;
        return Some(if trimmed.starts_with('-') {
            value.wrapping_neg()
        } else {
            value
        });
    }
    i32::try_from(str_to_int64(text)?).ok()
}

macro_rules! int_from {
    ($($ty:ty),*) => {$(
        impl From<$ty> for Variant {
            fn from(value: $ty) -> Self {
                Variant::Int(i64::from(value))
            }
        }
    )*};
}
int_from!(i8, u8, i16, u16, i32, u32, i64);

impl From<u64> for Variant {
    fn from(value: u64) -> Self {
        Variant::UInt(value)
    }
}

impl From<usize> for Variant {
    fn from(value: usize) -> Self {
        Variant::Int(value as i64)
    }
}

impl From<f64> for Variant {
    fn from(value: f64) -> Self {
        Variant::Float(value)
    }
}

impl From<f32> for Variant {
    fn from(value: f32) -> Self {
        Variant::Float(f64::from(value))
    }
}

impl From<bool> for Variant {
    fn from(value: bool) -> Self {
        Variant::Bool(value)
    }
}

impl From<String> for Variant {
    fn from(value: String) -> Self {
        Variant::Str(value)
    }
}

impl From<&str> for Variant {
    fn from(value: &str) -> Self {
        Variant::Str(value.to_owned())
    }
}

impl From<Vec<u8>> for Variant {
    fn from(value: Vec<u8>) -> Self {
        Variant::Bytes(value)
    }
}

/// Comparison with an integer, as the definition callbacks write it
/// (`e.NativeValues['..\Type'] = 3`). A value that cannot be converted
/// compares unequal.
impl PartialEq<i64> for Variant {
    fn eq(&self, other: &i64) -> bool {
        self.compare(&Variant::Int(*other)).is_ok_and(Ordering::is_eq)
    }
}

impl PartialOrd<i64> for Variant {
    fn partial_cmp(&self, other: &i64) -> Option<Ordering> {
        self.compare(&Variant::Int(*other)).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        assert!(Variant::Empty == 0);
        assert!(Variant::Empty < 1);
        assert_eq!(Variant::Empty.to_str().unwrap(), "");
    }

    #[test]
    fn str_to_int_forms() {
        assert_eq!(str_to_int64("-12"), Some(-12));
        assert_eq!(str_to_int64("$FF"), Some(255));
        assert_eq!(str_to_int64("1.5"), None);
        assert_eq!(str_to_int("$FFFFFFFF"), Some(-1));
    }

    #[test]
    fn float_rounds_to_int() {
        assert_eq!(Variant::Float(2.5).to_i64().unwrap(), 2);
        assert_eq!(Variant::Float(3.5).to_i64().unwrap(), 4);
    }
}
