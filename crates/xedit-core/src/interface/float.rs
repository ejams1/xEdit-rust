// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbFloatDef`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use super::def::request_storage;
use super::def::{
    Def, DefBase, DefKind, DefRef, NamedDef, NamedDefArgs, NamedDefBase, ValueDef, ValueDefBase, value_def_plumbing,
};
use super::element::{DataPtr, ElementArg, ElementRef};
use super::globals::IGNORE_STRING_VALUE;
use super::globals::{check_expected_bytes, is_internal_edit};
use super::misc::{EditError, Variant, length, shorten_text, variant_to_f64};
use super::types::{CallbackType, DefFlag, DefType, pascal_enum};
use crate::delphi::{
    HALF_MAX_VALUE, HALF_MIN_VALUE, HALF_NAN, HALF_NEG_INF, HALF_POS_INF, MAX_DOUBLE, MAX_SINGLE, float_to_half,
    float_to_str, float_to_str_f_fixed, half_to_float, round_to_ex, same_value, single_same_value, str_to_float,
};

pascal_enum! {
    FloatKind {
        fkHalf,
        fkSingle,
        fkDouble,
    }
}

pub type FloatNormalizer = Arc<dyn Fn(ElementArg, f64) -> f64 + Send + Sync>;

/// Upstream `wbFloatDigits`.
pub const FLOAT_DIGITS: i32 = 6;

/// `fdDigits` value that selects `FloatToStr` and no rounding. Upstream `Low(Integer)`.
pub const FLOAT_DIGITS_GENERAL: i32 = i32::MIN;

/// HalfEpsilon is a half-float bit pattern that upstream uses as a number.
const HALF_EPSILON: f64 = 0x1400 as f64;

/// The constructor arguments of `TwbFloatDef` after those of `TwbNamedDef`.
pub struct FloatDefArgs {
    pub scale: f64,
    /// Decimals to round to and to show. A negative value selects the default
    /// of the kind. [`FLOAT_DIGITS_GENERAL`] selects no rounding.
    pub digits: i32,
    pub normalizer: Option<FloatNormalizer>,
    pub default: f64,
    pub kind: FloatKind,
}

/// Upstream `TwbFloatDef`.
pub struct FloatDef {
    self_ref: Weak<FloatDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
    /// The bits of `fdDefault`.
    fd_default: AtomicU64,
    fd_scale: f64,
    fd_digits: i32,
    fd_normalizer: Option<FloatNormalizer>,
    fd_kind: FloatKind,
}

impl FloatDef {
    /// Port of `TwbFloatDef.Create`.
    pub fn create(args: NamedDefArgs, float: FloatDefArgs) -> Arc<Self> {
        let mut fd_digits = float.digits;
        if fd_digits != FLOAT_DIGITS_GENERAL && fd_digits < 0 {
            fd_digits = match float.kind {
                FloatKind::fkHalf => FLOAT_DIGITS / 2,
                FloatKind::fkSingle => FLOAT_DIGITS,
                FloatKind::fkDouble => FLOAT_DIGITS * 2,
            };
        }
        let (def, nd) = NamedDefBase::create(args);
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            vd: ValueDefBase::default(),
            fd_default: AtomicU64::new(float.default.to_bits()),
            fd_scale: float.scale,
            fd_digits,
            fd_normalizer: float.normalizer,
            fd_kind: float.kind,
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbFloatDef.Clone`.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(
            NamedDefBase::clone_args(source),
            FloatDefArgs {
                scale: source.fd_scale,
                digits: source.fd_digits,
                normalizer: source.fd_normalizer.clone(),
                default: source.fd_default(),
                kind: source.fd_kind,
            },
        );
        ValueDefBase::after_clone(&*this, source);
        this
    }

    pub fn fd_default(&self) -> f64 {
        f64::from_bits(self.fd_default.load(Ordering::Relaxed))
    }

    /// The bytes of a number in the layout of the kind.
    fn bits_of(&self, value: f64) -> Vec<u8> {
        match self.fd_kind {
            FloatKind::fkHalf => float_to_half(value).to_le_bytes().to_vec(),
            FloatKind::fkSingle => (value as f32).to_le_bytes().to_vec(),
            FloatKind::fkDouble => value.to_le_bytes().to_vec(),
        }
    }

    /// Upstream `HalfNaN`, `SingleNaN` and `DoubleNaN` (`0.0/0.0`).
    fn nan_bits(&self) -> Vec<u8> {
        match self.fd_kind {
            FloatKind::fkHalf => HALF_NAN.to_le_bytes().to_vec(),
            FloatKind::fkSingle => 0xFFC0_0000u32.to_le_bytes().to_vec(),
            FloatKind::fkDouble => 0xFFF8_0000_0000_0000u64.to_le_bytes().to_vec(),
        }
    }

    fn infinity_bits(&self, negative: bool) -> Vec<u8> {
        match self.fd_kind {
            FloatKind::fkHalf => if negative { HALF_NEG_INF } else { HALF_POS_INF }
                .to_le_bytes()
                .to_vec(),
            FloatKind::fkSingle => if negative { f32::NEG_INFINITY } else { f32::INFINITY }
                .to_le_bytes()
                .to_vec(),
            FloatKind::fkDouble => if negative { f64::NEG_INFINITY } else { f64::INFINITY }
                .to_le_bytes()
                .to_vec(),
        }
    }

    /// Upstream `HalfMaxValue`, `$7F7FFFFF` and `$7FEFFFFFFFFFFFFF`, or the
    /// minimum patterns.
    fn max_bits(&self, negative: bool) -> Vec<u8> {
        match (self.fd_kind, negative) {
            (FloatKind::fkHalf, false) => HALF_MAX_VALUE.to_le_bytes().to_vec(),
            (FloatKind::fkHalf, true) => HALF_MIN_VALUE.to_le_bytes().to_vec(),
            (FloatKind::fkSingle, false) => 0x7F7F_FFFFu32.to_le_bytes().to_vec(),
            (FloatKind::fkSingle, true) => 0xFF7F_FFFFu32.to_le_bytes().to_vec(),
            (FloatKind::fkDouble, false) => 0x7FEF_FFFF_FFFF_FFFFu64.to_le_bytes().to_vec(),
            (FloatKind::fkDouble, true) => 0xFFEF_FFFF_FFFF_FFFFu64.to_le_bytes().to_vec(),
        }
    }

    /// Port of `TwbFloatDef.FromValue`: zero, infinity and NaN as their
    /// patterns, a value beyond the range of the kind clamped to its
    /// extreme pattern, else the value rounded to the digits, divided by
    /// the scale, normalized and stored.
    pub fn from_value(&self, value: f64, data: DataPtr, element: ElementArg) -> Result<(), EditError> {
        let size = self.get_default_size(data, element) as usize;
        let (element_ref, mut bytes) = request_storage(element, size)?;
        let stored: Vec<u8> = if value == 0.0 || value.is_subnormal() {
            self.bits_of(0.0)
        } else if value.is_infinite() {
            self.infinity_bits(value.is_sign_negative())
        } else if value.is_nan() {
            self.nan_bits()
        } else {
            let (max, min_same) = match self.fd_kind {
                FloatKind::fkHalf => (
                    f64::from(HALF_MAX_VALUE),
                    single_same_value(value, f64::from(HALF_MIN_VALUE)) || value < f64::from(HALF_MIN_VALUE),
                ),
                FloatKind::fkSingle => (MAX_SINGLE, single_same_value(value, -MAX_SINGLE) || value < -MAX_SINGLE),
                FloatKind::fkDouble => (MAX_DOUBLE, same_value(value, -MAX_DOUBLE) || value < -MAX_DOUBLE),
            };
            let max_same = match self.fd_kind {
                FloatKind::fkDouble => same_value(value, max),
                _ => single_same_value(value, max),
            };
            if max_same || value > max {
                self.max_bits(false)
            } else if min_same {
                self.max_bits(true)
            } else {
                let mut value = value;
                if self.fd_digits >= 0 {
                    value = round_to_ex(value, -self.fd_digits);
                }
                if self.fd_scale != 1.0 {
                    value /= self.fd_scale;
                }
                if let Some(normalizer) = &self.fd_normalizer {
                    value = normalizer(element, value);
                }
                self.bits_of(value)
            }
        };
        let len = stored.len().min(bytes.len());
        bytes[..len].copy_from_slice(&stored[..len]);
        if self.nd.nd_terminator && !bytes.is_empty() {
            let last = bytes.len() - 1;
            bytes[last] = super::string::TERMINATOR;
        }
        element_ref.commit_storage(bytes);
        Ok(())
    }

    pub fn get_kind(&self) -> FloatKind {
        self.fd_kind
    }

    /// Port of the override of `SetDefaultNativeValue`, which sets `fdDefault`.
    pub fn set_default_float(self: Arc<Self>, value: f64) -> Arc<Self> {
        let this = if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        };
        this.fd_default.store(value.to_bits(), Ordering::Relaxed);
        this
    }

    fn len(data: DataPtr) -> i64 {
        data.map_or(0, |data| data.len() as i64)
    }

    /// Port of `ToValue`: the number that the data shows as, or NaN.
    pub fn to_value(&self, data: DataPtr, element: ElementArg) -> f64 {
        let size = self.get_default_size(data, element);
        if Self::len(data) < i64::from(size) {
            return f64::NAN;
        }
        let data = data.unwrap_or_default();
        let mut value = match self.fd_kind {
            FloatKind::fkHalf => {
                let bits = u16::from_le_bytes([data[0], data[1]]);
                // UPSTREAM-QUIRK: HalfMaxValue and HalfMinValue are half-float bit
                // patterns. Upstream returns them as numbers, 31743 and 1024.
                if bits == HALF_MAX_VALUE {
                    return f64::from(HALF_MAX_VALUE);
                } else if bits == HALF_MIN_VALUE {
                    return f64::from(HALF_MIN_VALUE);
                }
                f64::from(half_to_float(bits))
            }
            FloatKind::fkSingle => {
                let bits = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                if (bits == 0xFF7F_7FFF && self.def.def_flags.contains(DefFlag::dfFloatSometimesBroken))
                    || bits == 0x7F7F_FFFF
                {
                    return MAX_SINGLE;
                } else if bits == 0xFF7F_FFFF {
                    return -MAX_SINGLE;
                }
                f64::from(f32::from_bits(bits))
            }
            FloatKind::fkDouble => {
                let bits = u64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]);
                // UPSTREAM-QUIRK: the lowest double also reads as the highest.
                if bits == 0x7FEF_FFFF_FFFF_FFFF || bits == 0xFFEF_FFFF_FFFF_FFFF {
                    return MAX_DOUBLE;
                }
                f64::from_bits(bits)
            }
        };
        if value.is_infinite() || value.is_nan() {
            return value;
        }
        if value != 0.0 {
            let is_zero = match self.fd_kind {
                FloatKind::fkHalf | FloatKind::fkSingle => single_same_value(value, 0.0),
                // SameValue with the resolution of a double.
                FloatKind::fkDouble => value.abs() <= 1e-12,
            };
            if is_zero {
                value = 0.0;
            }
        }
        // Upstream reports a floating point exception as "<Error reading ...>" or
        // "<Error scaling/rounding ...>" on the progress output and returns NaN.
        // The progress output is not ported yet. The values are the same.
        if let Some(normalizer) = &self.fd_normalizer {
            value = normalizer(element, value);
        }
        if self.fd_scale != 1.0 {
            value *= self.fd_scale;
        }
        if value.is_nan() {
            return f64::NAN;
        }
        if self.fd_digits >= 0 {
            round_to_ex(value, -self.fd_digits)
        } else {
            value
        }
    }

    fn is_max(&self, value: f64) -> bool {
        // UPSTREAM-QUIRK: the third test upstream asks for fkHalf where fkDouble
        // was meant, so a double never shows as Default or Min.
        (self.fd_kind == FloatKind::fkHalf && (value == f64::from(HALF_MAX_VALUE) || value == MAX_DOUBLE))
            || (self.fd_kind == FloatKind::fkSingle && value == MAX_SINGLE)
    }

    fn is_min(&self, value: f64) -> bool {
        (self.fd_kind == FloatKind::fkHalf && (value == f64::from(HALF_MIN_VALUE) || value == -MAX_DOUBLE))
            || (self.fd_kind == FloatKind::fkSingle && value == -MAX_SINGLE)
    }

    /// Port of `ToStringInternal`.
    fn to_string_internal(&self, data: DataPtr, element: ElementArg, include_warnings: bool) -> String {
        let mut result = String::new();
        let len = Self::len(data);
        let default_size = i64::from(self.get_default_size(data, element));
        if len < default_size {
            if include_warnings && check_expected_bytes() {
                result = format!("<Error: Expected {default_size} bytes of data, found {len}>");
            }
        } else {
            let value = self.to_value(data, element);
            result = if value.is_nan() {
                "NaN".to_owned()
            } else if value == f64::INFINITY {
                "Inf".to_owned()
            } else if value == f64::NEG_INFINITY {
                "-Inf".to_owned()
            } else if self.is_max(value) {
                "Default".to_owned()
            } else if self.is_min(value) {
                "Min".to_owned()
            } else if self.fd_digits >= 0 {
                float_to_str_f_fixed(value, self.fd_digits as usize)
            } else {
                float_to_str(value)
            };
            if include_warnings && len > default_size && check_expected_bytes() {
                result.push_str(&format!(
                    " <Warning: Expected {default_size} bytes of data, found {len}>"
                ));
            }
        }
        self.used(element, &result);
        result
    }
}

impl Def for FloatDef {
    value_def_plumbing!(Def);

    fn get_def_type(&self) -> DefType {
        DefType::dtFloat
    }

    fn get_def_type_name(&self) -> String {
        "Float".to_owned()
    }

    fn as_float_def(&self) -> Option<&FloatDef> {
        Some(self)
    }
}

impl NamedDef for FloatDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for FloatDef {
    value_def_plumbing!(ValueDef);

    /// Port of the override of `SetDefaultNativeValue`, which sets `fdDefault`.
    fn apply_default_native_value(&self, value: Variant) {
        // Upstream fails for a value that is not a number.
        let default = match &value {
            Variant::Float(float) => *float,
            other => other.as_ordinal().unwrap_or(0) as f64,
        };
        self.fd_default.store(default.to_bits(), Ordering::Relaxed);
    }

    fn to_string(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = self.to_string_internal(data, element, true);
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToStr);
        }
        result
    }

    fn to_summary(&self, _depth: i32, data: DataPtr, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        let mut result = self.to_string_internal(data, element, false);
        if result.contains('.') {
            // Cuts trailing zeros, and the decimal point when nothing follows it.
            let chars: Vec<char> = result.chars().collect();
            let mut l = chars.len();
            while l > 1 {
                match chars[l - 1] {
                    '.' => {
                        l -= 1;
                        break;
                    }
                    '0' => l -= 1,
                    _ => break,
                }
            }
            result = chars[..l].iter().collect();
        }
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToSummary);
        }
        result = shorten_text(&result);
        if links_to.is_none()
            && !result.is_empty()
            && let Some(element) = element
        {
            *links_to = element.get_links_to();
        }
        result
    }

    fn to_sort_key(&self, data: DataPtr, element: ElementArg, _extended: bool) -> String {
        let mut result = String::new();
        let mut value = self.to_value(data, element);
        if value.is_nan() {
            result = " ".repeat(40);
        } else if value == f64::INFINITY {
            result = "+".repeat(40);
        } else if value == f64::NEG_INFINITY {
            result = "-".repeat(40);
        } else if value == 0.0 || value.is_subnormal() {
            value = 0.0;
        } else {
            if self.is_max(value) {
                result = format!("+{}", "9".repeat(39));
            } else if self.is_min(value) {
                result = format!("-{}", "9".repeat(39));
            }
            if result.is_empty() {
                // Upstream uses the smallest positive number of the kind. For a
                // half that is the bit pattern HalfEpsilon read as a number.
                let epsilon = match self.fd_kind {
                    FloatKind::fkHalf => HALF_EPSILON,
                    FloatKind::fkSingle => f64::from(f32::from_bits(1)),
                    FloatKind::fkDouble => f64::from_bits(1),
                };
                if value.abs() <= epsilon {
                    value = 0.0;
                }
            }
        }
        if result.is_empty() {
            let digits = if self.fd_digits >= 0 {
                self.fd_digits as usize
            } else {
                19
            };
            result = float_to_str_f_fixed(value.abs(), digits);
            let len = length(&result);
            if len < 39 {
                result = format!("{}{result}", "0".repeat(39 - len));
            }
            result.insert(0, if value < 0.0 { '-' } else { '+' });
        }
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToSortKey);
        }
        result
    }

    fn get_size(&self, data: DataPtr, element: ElementArg) -> i32 {
        match data {
            Some([]) => i32::from(self.nd.nd_terminator),
            _ => self.get_default_size(data, element),
        }
    }

    fn get_default_size(&self, _data: DataPtr, _element: ElementArg) -> i32 {
        let size = match self.fd_kind {
            FloatKind::fkHalf => 2,
            FloatKind::fkSingle => 4,
            FloatKind::fkDouble => 8,
        };
        size + i32::from(self.nd.nd_terminator)
    }

    fn to_edit_value(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = self.to_string_internal(data, element, false);
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToEditValue);
        }
        result
    }

    fn to_native_value(&self, data: DataPtr, element: ElementArg) -> Variant {
        let value = self.to_value(data, element);
        if value.is_nan() {
            Variant::Empty
        } else {
            Variant::Float(value)
        }
    }

    /// Port of `TwbFloatDef.FromEditValue`: the special texts `NaN`, `Inf`,
    /// `-Inf`, `Default`, `Max` and `Min`, an empty text as zero, else the
    /// number.
    fn from_edit_value(&self, data: DataPtr, element: ElementArg, value: &str) -> Result<(), EditError> {
        let size = self.get_default_size(data, element) as usize;
        let (element_ref, mut bytes) = request_storage(element, size)?;
        let mut text = value.to_owned();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut text, data, element, CallbackType::ctFromEditValue);
        }
        let text = text.trim();
        let special: Option<Vec<u8>> = if text == IGNORE_STRING_VALUE {
            return Ok(());
        } else if text.is_empty() {
            Some(self.bits_of(0.0))
        } else if text.eq_ignore_ascii_case("NaN") {
            Some(self.nan_bits())
        } else if text.eq_ignore_ascii_case("Inf") || text.eq_ignore_ascii_case("+Inf") {
            Some(self.infinity_bits(false))
        } else if text.eq_ignore_ascii_case("-Inf") {
            Some(self.infinity_bits(true))
        } else if text.eq_ignore_ascii_case("Default") || text.eq_ignore_ascii_case("Max") {
            Some(self.max_bits(false))
        } else if text.eq_ignore_ascii_case("Min") {
            Some(self.max_bits(true))
        } else {
            None
        };
        match special {
            Some(special) => {
                let len = special.len().min(bytes.len());
                bytes[..len].copy_from_slice(&special[..len]);
                if self.nd.nd_terminator && !bytes.is_empty() {
                    let last = bytes.len() - 1;
                    bytes[last] = super::string::TERMINATOR;
                }
                element_ref.commit_storage(bytes);
                Ok(())
            }
            None => {
                let number =
                    str_to_float(text).ok_or_else(|| format!("'{text}' is not a valid floating point value"))?;
                self.from_value(number, data, element)
            }
        }
    }

    fn from_native_value(&self, data: DataPtr, element: ElementArg, value: Variant) -> Result<(), EditError> {
        let number = match value {
            Variant::Empty => f64::NAN,
            other => variant_to_f64(&other)?,
        };
        self.from_value(number, data, element)
    }

    fn set_to_default(&self, data: DataPtr, element: ElementArg) -> Result<bool, EditError> {
        if self.set_to_default_callback(data, element) {
            return Ok(true);
        }
        if let Some(result) = self.set_to_default_edit_value(data, element) {
            return result;
        }
        let value = match self.to_native_value(data, element) {
            Variant::Float(value) => value,
            _ => 0.0,
        };
        let default = self.fd_default();
        let same = match self.fd_kind {
            FloatKind::fkHalf | FloatKind::fkSingle => single_same_value(value, default),
            FloatKind::fkDouble => same_value(value, default),
        };
        let changed = data.is_none() || !same;
        if changed {
            self.from_native_value(data, element, Variant::Float(default))?;
        }
        Ok(changed)
    }

    fn get_is_editable(&self, _data: DataPtr, _element: ElementArg) -> bool {
        !(self.def.def_internal_edit_only() && !is_internal_edit())
    }
}

impl DefKind for FloatDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::globals::test_lock;
    use super::super::types::ConflictPriority;
    use super::*;

    fn def_with(kind: FloatKind, digits: i32, scale: f64) -> Arc<FloatDef> {
        FloatDef::create(
            NamedDefArgs {
                priority: ConflictPriority::cpNormal,
                required: false,
                name: "Value".to_owned(),
                after_load: None,
                after_set: None,
                dont_show: None,
                get_cp: None,
                terminator: false,
            },
            FloatDefArgs {
                scale,
                digits,
                normalizer: None,
                default: 0.0,
                kind,
            },
        )
    }

    fn single() -> Arc<FloatDef> {
        def_with(FloatKind::fkSingle, -1, 1.0)
    }

    fn show(def: &FloatDef, value: f32) -> String {
        def.to_string(Some(&value.to_le_bytes()), None)
    }

    #[test]
    fn singles() {
        let _guard = test_lock();
        let def = single();
        assert_eq!(show(&def, 0.1), "0.100000");
        assert_eq!(show(&def, -14.5), "-14.500000");
        assert_eq!(show(&def, 1.0e-7), "0.000000");
        assert_eq!(show(&def, -1.0e-7), "0.000000");
        assert_eq!(show(&def, -0.0), "0.000000");
        assert_eq!(show(&def, f32::NAN), "NaN");
        assert_eq!(show(&def, f32::INFINITY), "Inf");
        assert_eq!(show(&def, f32::NEG_INFINITY), "-Inf");
        assert_eq!(show(&def, f32::MAX), "Default");
        assert_eq!(show(&def, f32::MIN), "Min");
        assert_eq!(show(&def, 1234567.9), "1234567.875000");
        // Rounding to six decimals overflows a 64-bit integer: the oracle
        // prints the x64 integer indefinite scaled back.
        assert_eq!(show(&def, 1.0e20), "-9223372013568.000000");
        assert_eq!(def.get_def_type_name(), "Float");
    }

    #[test]
    fn data_length() {
        let _guard = test_lock();
        let def = single();
        assert_eq!(
            def.to_string(Some(&[0, 0]), None),
            "<Error: Expected 4 bytes of data, found 2>"
        );
        assert_eq!(
            def.to_string(Some(&[0, 0, 0x80, 0x3F, 9]), None),
            "1.000000 <Warning: Expected 4 bytes of data, found 5>"
        );
        assert_eq!(def.to_edit_value(Some(&[0, 0]), None), "");
        assert_eq!(def.get_size(None, None), 4);
        assert_eq!(def.get_size(Some(&[]), None), 0);
        assert_eq!(def.to_native_value(Some(&[0, 0]), None), Variant::Empty);
        // The native value carries the rounding of `RoundToEx`.
        assert_eq!(
            def.to_native_value(Some(&1.5f32.to_le_bytes()), None),
            Variant::Float(round_to_ex(1.5, -6))
        );
    }

    #[test]
    fn kinds_digits_and_scale() {
        let _guard = test_lock();
        let half = def_with(FloatKind::fkHalf, -1, 1.0);
        assert_eq!(half.to_string(Some(&0x3C00u16.to_le_bytes()), None), "1.000");
        assert_eq!(half.to_string(Some(&0x7BFFu16.to_le_bytes()), None), "Default");
        assert_eq!(half.to_string(Some(&0x0400u16.to_le_bytes()), None), "Min");
        // The half with the value 1024 is not the Min bit pattern and is
        // rounded away from the Min value by `RoundToEx`.
        assert_eq!(half.to_string(Some(&0x6400u16.to_le_bytes()), None), "1024.000");
        let double = def_with(FloatKind::fkDouble, -1, 1.0);
        assert_eq!(double.to_string(Some(&0.1f64.to_le_bytes()), None), "0.100000000000");
        assert_eq!(double.get_default_size(None, None), 8);
        let percent = def_with(FloatKind::fkSingle, 2, 100.0);
        assert_eq!(show(&percent, 0.125), "12.50");
        let degrees = def_with(FloatKind::fkSingle, 0, 1.0);
        assert_eq!(show(&degrees, 2.5), "2");
        assert_eq!(show(&degrees, 3.5), "4");
    }

    #[test]
    fn summary_cuts_trailing_zeros() {
        let _guard = test_lock();
        let def = single();
        let summary = |value: f32| def.to_summary(0, Some(&value.to_le_bytes()), None, &mut None);
        assert_eq!(summary(1.5), "1.5");
        assert_eq!(summary(2.0), "2");
        assert_eq!(summary(0.0), "0");
        assert_eq!(summary(100.0), "100");
        assert_eq!(summary(f32::NAN), "NaN");
    }

    #[test]
    fn sort_keys() {
        let _guard = test_lock();
        let def = single();
        let key = |value: f32| def.to_sort_key(Some(&value.to_le_bytes()), None, false);
        assert_eq!(key(1.5), format!("+{}1.500000", "0".repeat(31)));
        assert_eq!(key(-1.5), format!("-{}1.500000", "0".repeat(31)));
        assert_eq!(key(0.0), format!("+{}0.000000", "0".repeat(31)));
        assert_eq!(key(f32::NAN), " ".repeat(40));
        assert_eq!(key(f32::INFINITY), "+".repeat(40));
        assert_eq!(key(f32::MAX), format!("+{}", "9".repeat(39)));
        assert_eq!(key(1.5).len(), 40);
    }
}
