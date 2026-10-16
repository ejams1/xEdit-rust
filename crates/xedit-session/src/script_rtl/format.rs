// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: the `Format` family of the Delphi RTL `System.SysUtils`
// (Format, FmtStr, StrFmt, StrLFmt, FormatBuf, FloatToStrF, FloatToStr,
// FormatFloat, CurrToStr/CurrToStrF, FormatCurr) as the JVCL
// JvInterpreter_SysUtils.pas registrations expose them, on the number
// semantics of xedit_core::delphi (FloatToStr, FloatToStrF, FloatToDecimal)

//! The Delphi `Format` family: the `%`-conversion language of
//! `System.SysUtils.Format` and the picture-string language of
//! `FormatFloat`.
//!
//! [`format_text`] is `Format(const Format: string; const Args: array of
//! const): string` over the interpreter's values, which is how the corpus
//! calls it (`Format('%s: %d', [name, count])`). The supported conversions
//! are the RTL's: `d`, `u`, `e`, `f`, `g`, `n`, `m`, `p`, `s`, `x`, `X` and
//! the literal `%%`, with the `Index:` / `-` / width / `.Precision`
//! modifiers. The Delphi number-to-digits step is the phase-5 port
//! (`xedit_core::delphi::float_to_str_f_fixed` and `format_general`), so
//! `%f` and `%g` round exactly as `FloatToStrF` does.
//!
//! The format settings are the en-US defaults the oracle runs with as a
//! plain Delphi process (`DecimalSeparator` `.`, `ThousandSeparator` `,`,
//! `CurrencyString` `$`, `CurrencyDecimals` 2); xEdit itself overrides none
//! of them (grep of the 4.1.5q `xEdit/` and `Core/` sources for
//! `ShortDateFormat`/`FormatSettings` finds no assignment).
//!
//! [`format_float`] is `FormatFloat`, the picture-string formatter: the
//! `0`/`#`/`.`/`,` picture characters, the `E+`/`E-` forms, quoted literals
//! and the `;`-separated positive/zero/negative sections. UPSTREAM-NOTE:
//! the RTL's handling of a picture with several `,` group positions, of a
//! `%` and of the `'`-quoted currency prefix is beyond what the corpus
//! uses; the port implements the documented rules and keeps the pictures a
//! script actually ships exact.
//!
//! The arguments reach the RTL as the `TVarRec`s `V2OA` builds from the
//! open array's variants (`JvInterpreter.pas:2376`): a `varInteger` or
//! `varSmallint` is `vtInteger`, a `varBoolean` `vtBoolean`, a `varDouble`
//! or `varCurrency` is cast to a `Currency` and passed as `vtCurrency`, and
//! everything else -- the strings, `varInt64`, `varByte`, `varDate`, `Null`,
//! an object -- is `vtVariant`. The RTL's `FormatBuf` then accepts only
//! `vtInteger` for `%d`/`%u`/`%x`, only `vtCurrency` for `%e`/`%f`/`%g`/
//! `%n`/`%m` and only `vtVariant` (through `VarToStr`) for `%s`, and raises
//! `EConvertError` with `Format '<format>' invalid or incompatible with
//! argument` otherwise, the format string cut to its first 31 characters
//! (`FormatError`'s buffer). Witnessed on the oracle by the `f-*` probes of
//! `cargo xtask parity script --phase probe` (2026-10-15): `Format('%d',
//! [TList.Count])` fails because `TList.Count` is a `NativeInt` (`varInt64`),
//! `Format('%s', [5])` and `Format('%f', [1])` fail, `Format('%.6f',
//! [1.23456789])` is `1.234600` (the four decimals of a `Currency`),
//! `Format('%.0f', [2.5])` is `2` and `[3.5]` is `4` (the currency's digits
//! round half to even), `Format('%f', [1e15])` is a `Type mismatch` (the
//! currency cast overflows, an `EVariantError` the expression handler
//! converts), `Format('%u', [-1])` is `4294967295`, and an unknown
//! conversion such as `%q` is the same `EConvertError`.

use xedit_script::values::{Value, ValueError};

use xedit_core::delphi;

/// The en-US format settings the oracle runs with.
pub struct FormatSettings {
    pub decimal_separator: char,
    pub thousand_separator: char,
    pub currency_string: &'static str,
    pub currency_decimals: i32,
}

/// `DefaultFormatSettings` as the oracle inherits it.
pub const SETTINGS: FormatSettings = FormatSettings {
    decimal_separator: '.',
    thousand_separator: ',',
    currency_string: "$",
    currency_decimals: 2,
};

/// `TFloatFormat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatFormat {
    General = 0,
    Exponent = 1,
    Fixed = 2,
    Number = 3,
    Currency = 4,
}

impl FloatFormat {
    pub fn from_code(code: i32) -> FloatFormat {
        match code {
            1 => FloatFormat::Exponent,
            2 => FloatFormat::Fixed,
            3 => FloatFormat::Number,
            4 => FloatFormat::Currency,
            _ => FloatFormat::General,
        }
    }
}

/// The `TVarRec` kind of an argument, as `V2OA` builds it
/// (`JvInterpreter.pas:2376-2460`).
enum VarRec<'a> {
    /// `vtInteger`: a `varInteger` or `varSmallint`.
    Integer(i32),
    /// `vtBoolean`.
    Boolean,
    /// `vtCurrency`: a `varDouble` cast to `Currency`, kept as the scaled
    /// integer (four decimals).
    Currency(i64),
    /// `vtVariant`: everything else, formatted through `VarToStr`.
    Variant(&'a Value),
}

/// `VarCast(V, V, varCurrency)` of a double: the value scaled by 10000 and
/// rounded to the nearest integer (half to even, the SSE conversion), or
/// the `EVariantOverflowError` beyond the `Currency` range -- an
/// `EVariantError`, which `Expression1` reports as `Type mismatch`.
fn currency_of(value: f64) -> Result<i64, ValueError> {
    // `MaxCurrency` is 922337203685477.5807 (the i64 range over 10000).
    const CURRENCY_LIMIT: f64 = i64::MAX as f64 / 10_000.0;
    if !value.is_finite() || value.abs() >= CURRENCY_LIMIT {
        return Err(ValueError::Variant);
    }
    // The product is formed in the x87's extended precision upstream, so
    // `0.00005 * 10000` is the exact `0.50000000000000002...` and rounds
    // up (probe `f-49`): the port scales the double's exact binary value
    // with 128-bit integers and rounds half to even.
    let bits = value.to_bits();
    let negative = (bits >> 63) != 0;
    let exponent = ((bits >> 52) & 0x7FF) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (mantissa, exponent) = if exponent == 0 {
        (fraction as i128, -1074)
    } else {
        ((fraction | (1u64 << 52)) as i128, exponent - 1075)
    };
    let scaled = mantissa * 10_000;
    let rounded: i128 = if exponent >= 0 {
        scaled << exponent
    } else {
        let shift = (-exponent) as u32;
        if shift >= 120 {
            0
        } else {
            let quotient = scaled >> shift;
            let remainder = scaled - (quotient << shift);
            let half = 1i128 << (shift - 1);
            if remainder > half || (remainder == half && quotient & 1 == 1) {
                quotient + 1
            } else {
                quotient
            }
        }
    };
    let rounded = if negative { -rounded } else { rounded };
    i64::try_from(rounded).map_err(|_| ValueError::Variant)
}

fn var_rec(value: &Value) -> Result<VarRec<'_>, ValueError> {
    Ok(match value {
        Value::Integer(v) => VarRec::Integer(*v),
        Value::Smallint(v) => VarRec::Integer(i32::from(*v)),
        Value::Bool(_) => VarRec::Boolean,
        Value::Double(v) => VarRec::Currency(currency_of(*v)?),
        other => VarRec::Variant(other),
    })
}

/// `FormatError` (`System.SysUtils`): the `EConvertError` of a conversion
/// the argument does not fit, naming the first 31 characters of the whole
/// format string.
fn format_error(format: &[u8]) -> ValueError {
    ValueError::external_class(
        "EConvertError",
        format!("Format '{}' invalid or incompatible with argument", excerpt(format)),
    )
}

/// `SArgumentMissing`, with the same excerpt.
fn argument_missing(format: &[u8]) -> ValueError {
    ValueError::external_class("EConvertError", format!("No argument for format '{}'", excerpt(format)))
}

fn excerpt(format: &[u8]) -> String {
    String::from_utf8_lossy(format).chars().take(31).collect()
}

/// `Format(Format, Args)`: the RTL's `FormatBuf` over the `TVarRec`s of
/// [`var_rec`]. A conversion the argument does not fit, an unknown
/// conversion character and a missing argument are the RTL's
/// `EConvertError`s ([`format_error`], [`argument_missing`]).
pub fn format_text(format: &[u8], args: &[Value]) -> Result<Vec<u8>, ValueError> {
    let mut out: Vec<u8> = Vec::new();
    let mut next_arg = 0usize;
    let mut i = 0usize;
    while i < format.len() {
        let c = format[i];
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        if i >= format.len() {
            // A dangling '%' is the end of the format (the RTL's loop
            // ends on it, with nothing written).
            break;
        }
        if format[i] == b'%' {
            out.push(b'%');
            i += 1;
            continue;
        }
        // `Index:` -- a decimal index followed by a colon.
        let mut index: Option<usize> = None;
        {
            let start = i;
            let mut j = i;
            while j < format.len() && format[j].is_ascii_digit() {
                j += 1;
            }
            if j > start && j < format.len() && format[j] == b':' {
                index = std::str::from_utf8(&format[start..j]).ok().and_then(|s| s.parse().ok());
                i = j + 1;
            }
        }
        let left_align = if i < format.len() && format[i] == b'-' {
            i += 1;
            true
        } else {
            false
        };
        // `*` takes the width (or the precision) from the next argument,
        // which must be a `vtInteger` (probe `f-81`).
        let star = |i: &mut usize, next_arg: &mut usize| -> Result<Option<usize>, ValueError> {
            if *i < format.len() && format[*i] == b'*' {
                *i += 1;
                let index = *next_arg;
                *next_arg += 1;
                return match args.get(index).map(var_rec).transpose()? {
                    Some(VarRec::Integer(v)) => Ok(Some(v.max(0) as usize)),
                    Some(_) => Err(format_error(format)),
                    None => Err(argument_missing(format)),
                };
            }
            Ok(read_int(format, i))
        };
        let width = star(&mut i, &mut next_arg)?;
        let precision = if i < format.len() && format[i] == b'.' {
            i += 1;
            Some(star(&mut i, &mut next_arg)?.unwrap_or(0))
        } else {
            None
        };
        let Some(&conversion) = format.get(i) else {
            break;
        };
        i += 1;
        let arg_index = match index {
            Some(index) => index,
            None => {
                let index = next_arg;
                next_arg += 1;
                index
            }
        };
        let Some(argument) = args.get(arg_index) else {
            return Err(argument_missing(format));
        };
        let text = convert(conversion, var_rec(argument)?, precision).ok_or_else(|| format_error(format))?;
        let padded = pad(&text, width, left_align);
        out.extend_from_slice(padded.as_bytes());
    }
    Ok(out)
}

fn read_int(format: &[u8], i: &mut usize) -> Option<usize> {
    let start = *i;
    while *i < format.len() && format[*i].is_ascii_digit() {
        *i += 1;
    }
    if *i == start {
        return None;
    }
    std::str::from_utf8(&format[start..*i])
        .ok()
        .and_then(|s| s.parse().ok())
}

fn pad(text: &str, width: Option<usize>, left_align: bool) -> String {
    let Some(width) = width else { return text.to_owned() };
    let length = text.chars().count();
    if length >= width {
        return text.to_owned();
    }
    let fill: String = std::iter::repeat_n(' ', width - length).collect();
    if left_align {
        format!("{text}{fill}")
    } else {
        format!("{fill}{text}")
    }
}

/// One conversion over its `TVarRec`; `None` is the kind the conversion
/// refuses. `%d`, `%u` and `%x` take `vtInteger` (`%u` and `%x` as the
/// 32-bit `Cardinal`), the float conversions take `vtCurrency` (whose
/// digits are the four decimals of the currency, rounded half to even to
/// the precision), `%s` takes `vtVariant` (`VarToStr`: a string, an
/// `Int64` or a `Byte` as text, `Null`, an object and the rest as nothing),
/// and `%p` nothing the interpreter builds. The conversion characters are
/// case-insensitive except that `%X` upper-cases the digits.
fn convert(conversion: u8, rec: VarRec<'_>, precision: Option<usize>) -> Option<String> {
    Some(match (conversion, rec) {
        (b'd' | b'D', VarRec::Integer(v)) => {
            let mut text = v.to_string();
            if let Some(precision) = precision {
                text = zero_pad(&text, precision);
            }
            text
        }
        (b'u' | b'U', VarRec::Integer(v)) => {
            let mut text = (v as u32).to_string();
            if let Some(precision) = precision {
                text = zero_pad(&text, precision);
            }
            text
        }
        // The RTL writes the hexadecimal digits in upper case for `%x` and
        // `%X` alike (probe `f-18`).
        (b'x' | b'X', VarRec::Integer(v)) => hex(u64::from(v as u32), precision, true),
        (b'e' | b'E', VarRec::Currency(c)) => currency_exponent(c),
        (b'f' | b'F', VarRec::Currency(c)) => currency_fixed(c, precision.unwrap_or(2), false),
        (b'n' | b'N', VarRec::Currency(c)) => currency_fixed(c, precision.unwrap_or(2), true),
        (b'm' | b'M', VarRec::Currency(c)) => {
            let digits = precision.unwrap_or(SETTINGS.currency_decimals.max(0) as usize);
            let fixed = currency_fixed(c, digits, true);
            let negative = fixed.starts_with('-');
            let body = if negative { &fixed[1..] } else { &fixed };
            if negative {
                format!("-{}{body}", SETTINGS.currency_string)
            } else {
                format!("{}{body}", SETTINGS.currency_string)
            }
        }
        (b'g' | b'G', VarRec::Currency(c)) => currency_general(c),
        (b's' | b'S', VarRec::Variant(value)) => {
            // The precision cuts the text to that many characters (probe
            // `f-78`).
            let text = String::from_utf8_lossy(&var_to_str(value)).into_owned();
            match precision {
                Some(precision) => text.chars().take(precision).collect(),
                None => text,
            }
        }
        _ => return None,
    })
}

/// `VarToStr` of the kinds that reach `%s` as `vtVariant`: a string as
/// it is, the integer kinds `V2OA` does not pass as `vtInteger` as text,
/// a date as `DateTimeToStr`, and `Null`, `Empty`, an object, a class, a
/// set or a pointer as nothing.
fn var_to_str(value: &Value) -> Vec<u8> {
    match value {
        Value::Str(bytes) => bytes.clone(),
        Value::Int64(v) => v.to_string().into_bytes(),
        Value::Byte(v) => v.to_string().into_bytes(),
        Value::Date(v) => {
            let format = format!(
                "{} {}",
                super::datetime::SETTINGS.short_date_format,
                super::datetime::SETTINGS.long_time_format
            );
            super::datetime::date_time_to_str(&format, *v).into_bytes()
        }
        _ => Vec::new(),
    }
}

/// The digits of a currency: `|c|` as a decimal string of at least five
/// digits (so the four decimals have an integer part) and the sign.
fn currency_digits(c: i64) -> (bool, String) {
    let negative = c < 0;
    let mut digits = c.unsigned_abs().to_string();
    while digits.len() < 5 {
        digits.insert(0, '0');
    }
    (negative, digits)
}

/// A currency rounded half to even to `decimals` places (`FloatToDecimal`
/// over the currency's own digits): the integer part and the decimals,
/// both as digit strings.
fn currency_round(c: i64, decimals: usize) -> (bool, String, String) {
    let (negative, digits) = currency_digits(c);
    let (integer, fraction) = digits.split_at(digits.len() - 4);
    if decimals >= 4 {
        let mut fraction = fraction.to_owned();
        while fraction.len() < decimals {
            fraction.push('0');
        }
        return (negative, integer.to_owned(), fraction);
    }
    // Round the scaled value at `decimals` places, half to even.
    let keep = 10i64.pow(decimals as u32);
    let drop = 10_000 / keep;
    let scaled = c.unsigned_abs() as i64;
    let quotient = scaled / drop;
    let remainder = scaled % drop;
    let rounded = if remainder * 2 > drop || (remainder * 2 == drop && quotient % 2 == 1) {
        quotient + 1
    } else {
        quotient
    };
    let mut text = rounded.to_string();
    while text.len() < decimals + 1 {
        text.insert(0, '0');
    }
    let (integer, fraction) = text.split_at(text.len() - decimals);
    let negative = negative && rounded != 0;
    (negative, integer.to_owned(), fraction.to_owned())
}

/// `%f` and `%n` of a currency.
fn currency_fixed(c: i64, decimals: usize, thousand: bool) -> String {
    let (negative, integer, fraction) = currency_round(c, decimals);
    let integer = if thousand { insert_thousands(&integer) } else { integer };
    let sign = if negative { "-" } else { "" };
    if decimals == 0 {
        format!("{sign}{integer}")
    } else {
        format!("{sign}{integer}{}{fraction}", SETTINGS.decimal_separator)
    }
}

/// `%e` of a currency: `FloatToText` with `fvCurrency` prints every digit
/// of the currency, so the mantissa carries 18 decimals whatever the
/// precision, and the exponent three digits with its sign.
fn currency_exponent(c: i64) -> String {
    let (negative, digits) = currency_digits(c);
    let trimmed = digits.trim_start_matches('0');
    let (mantissa, exponent) = if trimmed.is_empty() {
        ("0".to_owned(), 0i64)
    } else {
        (trimmed.to_owned(), trimmed.len() as i64 - 1 - 4)
    };
    let first = &mantissa[..1];
    let mut rest = mantissa[1..].to_owned();
    while rest.len() < 18 {
        rest.push('0');
    }
    rest.truncate(18);
    let sign = if exponent < 0 { '-' } else { '+' };
    let sign_prefix = if negative { "-" } else { "" };
    format!("{sign_prefix}{first}.{rest}E{sign}{:03}", exponent.abs())
}

/// `%g` of a currency: the general format over the currency's digits --
/// the decimals without their trailing zeros, no point when none remain.
fn currency_general(c: i64) -> String {
    let (negative, integer, fraction) = currency_round(c, 4);
    let fraction = fraction.trim_end_matches('0');
    let sign = if negative { "-" } else { "" };
    if fraction.is_empty() {
        format!("{sign}{integer}")
    } else {
        format!("{sign}{integer}{}{fraction}", SETTINGS.decimal_separator)
    }
}

fn zero_pad(text: &str, precision: usize) -> String {
    let negative = text.starts_with('-');
    let digits = if negative { &text[1..] } else { text };
    if digits.chars().count() >= precision {
        return text.to_owned();
    }
    let zeros: String = std::iter::repeat_n('0', precision - digits.chars().count()).collect();
    if negative {
        format!("-{zeros}{digits}")
    } else {
        format!("{zeros}{digits}")
    }
}

/// `%x`/`%X`: at least `precision` digits, uppercase on demand.
fn hex(value: u64, precision: Option<usize>, upper: bool) -> String {
    let text = if upper {
        format!("{value:X}")
    } else {
        format!("{value:x}")
    };
    match precision {
        Some(precision) if text.len() < precision => {
            let zeros: String = std::iter::repeat_n('0', precision - text.len()).collect();
            format!("{zeros}{text}")
        }
        _ => text,
    }
}

/// `FloatToText(ffFixed)`/`FloatToText(ffNumber)` for `%f` and `%n`.
fn float_fixed(value: f64, digits: usize, thousand: bool) -> String {
    let mut text = delphi::float_to_str_f_fixed(value, digits);
    if thousand {
        text = insert_thousands(&text);
    }
    text
}

/// `%m`: the currency format (symbol, grouped digits, `CurrencyDecimals`
/// decimals).
fn currency(value: f64, precision: Option<usize>) -> String {
    let digits = precision.unwrap_or(SETTINGS.currency_decimals.max(0) as usize);
    let fixed = float_fixed(value, digits, true);
    let negative = fixed.starts_with('-');
    let body = if negative { &fixed[1..] } else { &fixed };
    if negative {
        format!("-{}{body}", SETTINGS.currency_string)
    } else {
        format!("{}{body}", SETTINGS.currency_string)
    }
}

/// Inserts the thousands separators into the integer part of a formatted
/// number.
fn insert_thousands(text: &str) -> String {
    let (sign, body) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text),
    };
    let (integer, fraction) = match body.split_once(SETTINGS.decimal_separator) {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (body, None),
    };
    let mut grouped = String::new();
    let digits: Vec<char> = integer.chars().collect();
    for (index, c) in digits.iter().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(SETTINGS.thousand_separator);
        }
        grouped.push(*c);
    }
    match fraction {
        Some(fraction) => format!("{sign}{grouped}{}{fraction}", SETTINGS.decimal_separator),
        None => format!("{sign}{grouped}"),
    }
}

/// `FloatToStrF(Value, Format, Precision, Digits)`.
pub fn float_to_str_f(value: f64, format: FloatFormat, precision: i32, digits: i32) -> String {
    match format {
        FloatFormat::General => {
            if precision <= 0 {
                delphi::float_to_str(value)
            } else {
                delphi::float_to_str_f_general(value, precision as usize, digits)
            }
        }
        FloatFormat::Exponent => {
            // A precision of 0 or less is the default of 15 significant
            // digits (`FloatToStr`, the general format, uses it too).
            let precision = if precision <= 0 { 15 } else { precision as usize };
            delphi::float_to_str_f_exponent(value, precision, digits)
        }
        FloatFormat::Fixed => float_fixed(value, digits.max(0) as usize, false),
        FloatFormat::Number => float_fixed(value, digits.max(0) as usize, true),
        FloatFormat::Currency => {
            let text = currency(value, Some(digits.max(0) as usize));
            // ffCurrency with a precision of 0 keeps the default decimals.
            if precision == 0 { currency(value, None) } else { text }
        }
    }
}

/// `FormatFloat(const Format: string; Value: Extended)`: the picture-string
/// formatter.
///
/// UPSTREAM-NOTE: a picture is split on `;` into the positive, the negative
/// and the zero section (a missing negative section is the positive one with
/// a leading `-`, a missing zero section the positive one); within a section
/// `0` and `#` are digit placeholders around a single `.`, `,` is a grouping
/// separator, `E+`/`E-` switch to scientific notation with the exponent
/// padded to the digits after the `E`, quoted text is literal and a leading
/// `-`/`+` moves the sign. The port implements those rules; two corners the
/// RTL documents but the corpus never uses (several `,` before the `.`
/// scaling by powers of 1000, and the `%`-like extras) are not
/// implemented -- a picture needing them formats without the scaling.
pub fn format_float(picture: &str, value: f64) -> String {
    if value.is_nan() {
        return "Nan".to_owned();
    }
    let sections: Vec<&str> = split_sections(picture);
    let section = if value > 0.0 {
        sections.first().copied().unwrap_or("")
    } else if value < 0.0 {
        sections
            .get(1)
            .copied()
            .unwrap_or_else(|| sections.first().copied().unwrap_or(""))
    } else {
        sections
            .get(2)
            .or_else(|| sections.get(1))
            .or_else(|| sections.first())
            .copied()
            .unwrap_or("")
    };
    let negative = value < 0.0 && sections.len() < 2;
    let value = value.abs();
    let (literal_prefix, spec, literal_suffix) = split_literals(section);
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&literal_prefix);
    out.push_str(&format_picture(&spec, value));
    out.push_str(&literal_suffix);
    out
}

/// Splits a picture on `;` outside quoted text.
fn split_sections(picture: &str) -> Vec<&str> {
    let mut sections = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    for (index, c) in picture.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == ';' => {
                sections.push(&picture[start..index]);
                start = index + 1;
            }
            None => {}
        }
    }
    sections.push(&picture[start..]);
    sections
}

/// The leading/trailing literal text of a section: the numeric core runs
/// from its first digit placeholder through the last format character, and
/// an `E`/`e` belongs to it only as part of the `E+`/`E-` exponent form
/// (a bare `e` in `zero` is literal text).
fn split_literals(section: &str) -> (String, String, String) {
    let chars: Vec<char> = section.chars().collect();
    let core = chars.iter().position(|c| matches!(c, '0' | '#')).unwrap_or(chars.len());
    let mut end = core;
    while end < chars.len() {
        let c = chars[end];
        let exponent = matches!(c, 'E' | 'e') && matches!(chars.get(end + 1), Some('+') | Some('-'));
        if matches!(c, '0' | '#' | '.' | ',' | '+' | '-') || exponent {
            end += 1;
        } else {
            break;
        }
    }
    let mut prefix = String::new();
    let mut i = 0usize;
    while i < core {
        let c = chars[i];
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            while i < core && chars[i] != quote {
                prefix.push(chars[i]);
                i += 1;
            }
            i += 1;
        } else {
            prefix.push(c);
            i += 1;
        }
    }
    let mut suffix = String::new();
    let mut i = end;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            while i < chars.len() && chars[i] != quote {
                suffix.push(chars[i]);
                i += 1;
            }
            i += 1;
        } else {
            suffix.push(c);
            i += 1;
        }
    }
    (prefix, chars[core..end].iter().collect(), suffix)
}

/// The numeric part of a picture section.
fn format_picture(spec: &str, mut value: f64) -> String {
    // A section without a digit placeholder is all literal text.
    if !spec.contains(['0', '#']) {
        return String::new();
    }
    let spec: String = spec.chars().filter(|c| !matches!(c, '"' | '\'')).collect();
    // `E+`/`E-`: scientific notation with `digits` exponent digits.
    if let Some(at) = spec.find(['E', 'e']) {
        let mantissa_spec = &spec[..at];
        let exponent_spec = &spec[at + 1..];
        let exponent_digits = exponent_spec.chars().filter(|c| *c == '0').count().max(1);
        let decimals = mantissa_spec
            .split_once('.')
            .map(|(_, fraction)| fraction.chars().filter(|c| matches!(c, '0' | '#')).count())
            .unwrap_or(0);
        let text = format!("{:.*e}", decimals, value);
        let (mantissa, exponent) = text.split_once('e').expect("scientific format");
        let exponent: i32 = exponent.parse().expect("exponent");
        // `E-` prints the sign only for a negative exponent; `E+` prints it
        // always.
        let sign = if exponent < 0 || exponent_spec.contains('+') {
            if exponent < 0 { "-" } else { "+" }
        } else {
            ""
        };
        let magnitude = exponent.unsigned_abs().to_string();
        let zeros: String = std::iter::repeat_n('0', exponent_digits.saturating_sub(magnitude.len())).collect();
        return format!("{mantissa}E{sign}{zeros}{magnitude}");
    }
    let (integer_spec, fraction_spec) = match spec.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (spec.as_str(), None),
    };
    // The picture's scaling: every `,` but the last (which is the grouping
    // separator) divides by 1000.
    let commas = integer_spec.matches(',').count();
    if commas > 1 {
        value /= 1000f64.powi((commas - 1) as i32);
    }
    let fraction_digits = fraction_spec
        .map(|fraction| fraction.chars().filter(|c| matches!(c, '0' | '#')).count())
        .unwrap_or(0);
    let text = format!("{:.*}", fraction_digits, value);
    let (integer, fraction) = match text.split_once('.') {
        Some((integer, fraction)) => (integer.to_owned(), fraction.to_owned()),
        None => (text, String::new()),
    };
    // A `0` in the integer part is a minimum digit count; `#` shows leading
    // zeros only when no `0` follows them (`#,##0` of 5 is `5`, of 0 `0`).
    let min_integer = integer_spec.chars().filter(|c| *c == '0').count();
    let mut integer = integer;
    while integer.len() < min_integer {
        integer.insert(0, '0');
    }
    if integer == "0" && min_integer == 0 && fraction_digits > 0 {
        integer.clear();
    }
    let fraction = match fraction_spec {
        None => String::new(),
        Some(fraction_spec) => {
            // Trailing `#` positions drop a formatted zero.
            let trailing_hashes = fraction_spec.chars().rev().take_while(|c| *c == '#').count();
            let minimum = fraction_digits.saturating_sub(trailing_hashes);
            let mut fraction = fraction;
            while fraction.len() > minimum && fraction.ends_with('0') {
                fraction.pop();
            }
            fraction
        }
    };
    let grouped = if integer_spec.contains(',') && integer.len() > 3 {
        insert_thousands(&integer)
    } else {
        integer
    };
    let show_point = fraction_spec.is_some_and(|spec| !fraction.is_empty() || spec.contains('0'));
    if show_point {
        format!("{grouped}.{fraction}")
    } else {
        grouped
    }
}

/// `CurrToStr(Value)`: `CurrToStrF(Value, ffGeneral, 15)`-shaped for the
/// port's doubles.
pub fn curr_to_str(value: f64) -> String {
    delphi::float_to_str(value)
}

/// `StrToFloat`, with the RTL's `EConvertError` text on a bad value.
pub fn str_to_float(text: &str) -> Result<f64, String> {
    // `TextToFloat` reads the RTL's own `INF`/`NAN` spellings (probes
    // `x-149`, `x-150`).
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("inf") || trimmed.eq_ignore_ascii_case("+inf") {
        return Ok(f64::INFINITY);
    }
    if trimmed.eq_ignore_ascii_case("-inf") {
        return Ok(f64::NEG_INFINITY);
    }
    if trimmed.eq_ignore_ascii_case("nan") {
        return Ok(f64::NAN);
    }
    delphi::str_to_float(text).ok_or_else(|| format!("'{text}' is not a valid floating point value"))
}

/// `StrToInt`, with the RTL's `EConvertError` text on a bad value.
pub fn str_to_int(text: &str) -> Result<i32, String> {
    // `Val` skips the spaces before the number (not after: `'5 '` is
    // refused, probe `x-138`) and reads the `0x` prefix as well as `$`
    // (probes `x-137`, `x-140`); a hexadecimal up to `$FFFFFFFF` is
    // accepted and wraps into the `Integer` (`StrToInt('$FFFFFFFF')` is -1,
    // probe `x-071`).
    let body = text.trim_start();
    let body = match body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        Some(hex) => format!("${hex}"),
        None => body.to_owned(),
    };
    xedit_script::values::parse_int_literal(body.as_bytes())
        .and_then(|value| {
            i32::try_from(value).ok().or_else(|| {
                (body.starts_with('$') && (0..=0xFFFF_FFFF).contains(&value)).then_some(value as u32 as i32)
            })
        })
        .ok_or_else(|| format!("'{text}' is not a valid integer value"))
}

/// `IntToHex(Value, Digits)`.
pub fn int_to_hex(value: i64, digits: i32) -> String {
    let digits = digits.max(0) as usize;
    let mut text = format!("{value:X}");
    while text.len() < digits {
        text.insert(0, '0');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(format: &str, args: Vec<Value>) -> String {
        String::from_utf8_lossy(&format_text(format.as_bytes(), &args).unwrap()).into_owned()
    }

    fn error(format: &str, args: Vec<Value>) -> String {
        match format_text(format.as_bytes(), &args) {
            Err(ValueError::ExternalClass { class, message }) => format!("[{class}] {message}"),
            Err(ValueError::Variant) => "Type mismatch".to_owned(),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn int(v: i32) -> Value {
        Value::Integer(v)
    }

    fn string(v: &str) -> Value {
        Value::Str(v.as_bytes().to_vec())
    }

    /// The conversions the corpus uses (`%s`, `%d`, `%x`, `%f`, `%n`)
    /// plus the modifiers, against the RTL's shapes.
    #[test]
    fn format_conversions_follow_delphi() {
        assert_eq!(text("%s: %d", vec![string("Rec"), int(12)]), "Rec: 12");
        assert_eq!(text("%d,%d", vec![int(1), int(2)]), "1,2");
        assert_eq!(text("%.4d", vec![int(7)]), "0007");
        assert_eq!(text("%.8d", vec![int(123)]), "00000123");
        assert_eq!(text("%1:s", vec![string("a"), string("b")]), "b");
        assert_eq!(text("%0:s", vec![string("a"), string("b")]), "a");
        assert_eq!(text("%6s|", vec![string("ab")]), "    ab|");
        assert_eq!(text("%-6s|", vec![string("ab")]), "ab    |");
        // `2.25` is the currency `2.2500`, rounded half to even: `2.2`.
        assert_eq!(text("%1.1f", vec![Value::Double(2.25)]), "2.2");
        assert_eq!(text("%x", vec![int(255)]), "FF");
        assert_eq!(text("%X", vec![int(255)]), "FF");
        assert_eq!(text("%%d", vec![int(3)]), "%d");
        assert_eq!(text("%d%%", vec![int(3)]), "3%");
        // A missing argument is the RTL's conversion error.
        assert!(format_text(b"%d", &[]).is_err());
    }

    /// The `TVarRec` rules of `V2OA` and `FormatBuf`, each line an oracle
    /// probe of `cargo xtask parity script --phase probe` (`f-*`).
    #[test]
    fn format_arguments_are_the_oracle_var_recs() {
        let fail = "invalid or incompatible with argument";
        // `%d` takes `vtInteger` only: an `Int64` (`TList.Count`), a
        // string, a boolean, a double, a `Byte` are refused.
        assert_eq!(
            error("%d", vec![Value::Int64(5)]),
            format!("[EConvertError] Format '%d' {fail}")
        );
        assert_eq!(
            error("%d", vec![string("5")]),
            format!("[EConvertError] Format '%d' {fail}")
        );
        assert_eq!(
            error("%d", vec![Value::Bool(true)]),
            format!("[EConvertError] Format '%d' {fail}")
        );
        assert_eq!(
            error("%d", vec![Value::Double(1.5)]),
            format!("[EConvertError] Format '%d' {fail}")
        );
        assert_eq!(
            error("%d", vec![Value::Byte(5)]),
            format!("[EConvertError] Format '%d' {fail}")
        );
        assert_eq!(text("%d", vec![Value::Smallint(5)]), "5");
        // `%s` takes `vtVariant` only.
        assert_eq!(error("%s", vec![int(5)]), format!("[EConvertError] Format '%s' {fail}"));
        assert_eq!(
            error("%s", vec![Value::Double(1.5)]),
            format!("[EConvertError] Format '%s' {fail}")
        );
        assert_eq!(
            error("%s", vec![Value::Bool(true)]),
            format!("[EConvertError] Format '%s' {fail}")
        );
        assert_eq!(text("%s", vec![Value::Int64(4294967296)]), "4294967296");
        assert_eq!(text("%s", vec![Value::Null]), "");
        // The float conversions take `vtCurrency` only.
        assert_eq!(error("%f", vec![int(1)]), format!("[EConvertError] Format '%f' {fail}"));
        assert_eq!(
            error("%f", vec![Value::Int64(1)]),
            format!("[EConvertError] Format '%f' {fail}")
        );
        assert_eq!(
            error("%f", vec![Value::Date(1.0)]),
            format!("[EConvertError] Format '%f' {fail}")
        );
        // The currency's four decimals and its half-to-even rounding.
        assert_eq!(text("%.6f", vec![Value::Double(1.23456789)]), "1.234600");
        assert_eq!(text("%.2f", vec![Value::Double(2.675)]), "2.68");
        assert_eq!(text("%.0f", vec![Value::Double(2.5)]), "2");
        assert_eq!(text("%.0f", vec![Value::Double(3.5)]), "4");
        assert_eq!(text("%.4f", vec![Value::Double(0.00005)]), "0.0001");
        assert_eq!(text("%f", vec![Value::Double(0.00005)]), "0.00");
        assert_eq!(text("%f", vec![Value::Double(-1.00005)]), "-1.00");
        assert_eq!(text("%e", vec![Value::Double(1.5)]), "1.500000000000000000E+000");
        assert_eq!(text("%.3e", vec![Value::Double(1.5)]), "1.500000000000000000E+000");
        assert_eq!(
            text("%e", vec![Value::Double(123456789.0)]),
            "1.234567890000000000E+008"
        );
        assert_eq!(text("%g", vec![Value::Double(1.5)]), "1.5");
        assert_eq!(text("%g", vec![Value::Double(1234567890123.0)]), "1234567890123");
        assert_eq!(text("%n", vec![Value::Double(1234567.891)]), "1,234,567.89");
        assert_eq!(text("%m", vec![Value::Double(1234.5678)]), "$1,234.57");
        assert_eq!(text("%8.3f", vec![Value::Double(1.23456)]), "   1.235");
        // Beyond the currency range the cast is an `EVariantError`.
        assert_eq!(error("%f", vec![Value::Double(1e15)]), "Type mismatch");
        // `%u` and `%x` are 32-bit.
        assert_eq!(text("%u", vec![int(-1)]), "4294967295");
        assert_eq!(text("%x", vec![int(-1)]), "FFFFFFFF");
        assert_eq!(text("%*d", vec![int(4), int(5)]), "   5");
        assert_eq!(text("%.2s", vec![string("abcdef")]), "ab");
        // An unknown conversion and the 31-character excerpt.
        assert_eq!(error("%q", vec![int(1)]), format!("[EConvertError] Format '%q' {fail}"));
        assert_eq!(
            error(
                "%s and this format string is longer than thirty-one chars",
                vec![int(5)]
            ),
            format!("[EConvertError] Format '%s and this format string is lo' {fail}")
        );
        assert_eq!(
            error("%s %s this is a long format string over 31 chars", vec![string("a")]),
            "[EConvertError] No argument for format '%s %s this is a long format str'"
        );
        assert_eq!(
            error(
                "    %s   Males: %d   Females: %d",
                vec![string("r"), Value::Int64(1), Value::Int64(2)]
            ),
            format!("[EConvertError] Format '    %s   Males: %d   Females: %' {fail}")
        );
    }

    /// `%n` groups with the en-US thousand separator and honours the
    /// precision, which is the `%.0n` the corpus uses.
    #[test]
    fn number_and_currency_formats() {
        assert_eq!(text("%.0n", vec![Value::Double(1234567.0)]), "1,234,567");
        assert_eq!(text("%n", vec![Value::Double(1234.5)]), "1,234.50");
        assert_eq!(text("%m", vec![Value::Double(1234.5)]), "$1,234.50");
        // `-1.005` is the currency `-1.0050`, rounded half to even to `-1.00`.
        assert_eq!(text("%.2f", vec![Value::Double(-1.005)]), "-1.00");
        // The phase-5 `FloatToDecimal` tie cases, kept as anchors.
        assert_eq!(delphi::float_to_str_f_fixed(0.0078125, 6), "0.007813");
        assert_eq!(delphi::float_to_str_f_fixed(14060.7578125, 6), "14060.757812");
        // `FloatToStrF` shapes: fixed, general, exponent.
        assert_eq!(float_to_str_f(2.5, FloatFormat::Fixed, 99, 2), "2.50");
        assert_eq!(float_to_str_f(1234.5, FloatFormat::Number, 99, 2), "1,234.50");
        assert_eq!(float_to_str_f(1.5, FloatFormat::General, 0, 0), "1.5");
    }

    /// `FormatFloat`'s picture language: placeholders, the point, grouping,
    /// sections and the exponent forms.
    #[test]
    fn format_float_pictures() {
        assert_eq!(format_float("0", 5.0), "5");
        assert_eq!(format_float("0.00", 5.5), "5.50");
        assert_eq!(format_float("#.##", 1.5), "1.5");
        assert_eq!(format_float("#.##", 2.0), "2");
        assert_eq!(format_float("#,##0.00", 1234.5), "1,234.50");
        assert_eq!(format_float("0.###", 1.0), "1");
        assert_eq!(format_float("0.000", 1.0), "1.000");
        // Sections: positive;negative;zero.
        assert_eq!(format_float("0.0;(0.0);zero", -1.5), "(1.5)");
        assert_eq!(format_float("0.0;(0.0);zero", 0.0), "zero");
        // An exponent form pads the exponent to the picture's digits.
        assert_eq!(format_float("0.00E+00", 1234.5), "1.23E+03");
    }

    /// `IntToHex`/`StrToInt`/`StrToFloat` with their RTL error texts.
    #[test]
    fn number_text_helpers() {
        assert_eq!(int_to_hex(255, 4), "00FF");
        assert_eq!(int_to_hex(0x1_0000, 0), "10000");
        assert_eq!(str_to_int("42"), Ok(42));
        assert_eq!(str_to_int("$FF"), Ok(255));
        assert_eq!(str_to_int("x"), Err("'x' is not a valid integer value".to_owned()));
        assert_eq!(str_to_float("1.5"), Ok(1.5));
        assert_eq!(
            str_to_float("x"),
            Err("'x' is not a valid floating point value".to_owned())
        );
    }
}
