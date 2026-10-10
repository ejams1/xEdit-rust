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
//! Conversion of an argument whose type does not fit the conversion follows
//! the variant coercion the interpreter applies before the RTL sees the
//! `TVarRec` (`%d` of a string parses it, of a boolean gives 0/1, of a float
//! rounds half to even through `Round`), rather than the EConvertError the
//! Delphi formatter would raise for a type it refuses.

use xedit_script::values::{Value, to_f64, to_i64, to_str};

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

/// `Format(Format, Args)`.
///
/// An invalid format string raises the RTL's `EConvertError` text
/// (`Invalid format string`? the RTL raises `EConvertError.CreateResFmt`
/// with `SInvalidFormat`); a conversion character that is not one of the
/// RTL's is a literal upstream, so the port keeps it as one.
pub fn format_text(format: &[u8], args: &[Value]) -> Result<Vec<u8>, String> {
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
            // A dangling '%' is invalid; the RTL raises.
            return Err("Invalid format string".to_owned());
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
        let width = read_int(format, &mut i);
        let precision = if i < format.len() && format[i] == b'.' {
            i += 1;
            Some(read_int(format, &mut i).unwrap_or(0))
        } else {
            None
        };
        let Some(&conversion) = format.get(i) else {
            return Err("Invalid format string".to_owned());
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
        let argument = args.get(arg_index);
        let text = convert(conversion, argument, precision)?;
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

fn convert(conversion: u8, argument: Option<&Value>, precision: Option<usize>) -> Result<String, String> {
    let Some(argument) = argument else {
        // The RTL raises `EConvertError` for a missing argument
        // (`SArgumentMissing`).
        return Err("No argument for format '%'".to_owned());
    };
    let value = argument.clone();
    Ok(match conversion {
        b'd' => {
            let mut text = to_i64(&value)
                .map_err(|_| "Invalid argument for format 'd'".to_owned())?
                .to_string();
            if let Some(precision) = precision {
                text = zero_pad(&text, precision);
            }
            text
        }
        b'u' => {
            let value = to_i64(&value).map_err(|_| "Invalid argument for format 'u'".to_owned())? as u64;
            let mut text = value.to_string();
            if let Some(precision) = precision {
                text = zero_pad(&text, precision);
            }
            text
        }
        b'x' => hex(
            to_i64(&value).map_err(|_| "Invalid argument for format 'x'".to_owned())? as u64,
            precision,
            false,
        ),
        b'X' => hex(
            to_i64(&value).map_err(|_| "Invalid argument for format 'X'".to_owned())? as u64,
            precision,
            true,
        ),
        b'e' => float_exponent(
            to_f64(&value).map_err(|_| "Invalid argument for format 'e'".to_owned())?,
            precision.unwrap_or(15),
            0,
        ),
        b'f' => float_fixed(
            to_f64(&value).map_err(|_| "Invalid argument for format 'f'".to_owned())?,
            precision.unwrap_or(2),
            false,
        ),
        b'n' => float_fixed(
            to_f64(&value).map_err(|_| "Invalid argument for format 'n'".to_owned())?,
            precision.unwrap_or(2),
            true,
        ),
        b'm' => currency(
            to_f64(&value).map_err(|_| "Invalid argument for format 'm'".to_owned())?,
            precision,
        ),
        b'g' => delphi::format_general(
            to_f64(&value).map_err(|_| "Invalid argument for format 'g'".to_owned())?,
            precision.unwrap_or(15),
        ),
        b's' => {
            // For `%s` the RTL ignores the precision.
            let _ = precision;
            String::from_utf8_lossy(&to_str(&value).map_err(|_| "Invalid argument for format 's'".to_owned())?)
                .into_owned()
        }
        b'p' => format!("{:08X}", to_i64(&value).unwrap_or(0) as u64 & 0xFFFF_FFFF),
        other => {
            // The RTL treats an unknown conversion character as a literal.
            format!("%{}", other as char)
        }
    })
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

/// `%e`: `digits` significant digits and an exponent of at least three
/// digits with an explicit sign (the RTL's shape: `1.23E+002`).
fn float_exponent(value: f64, digits: usize, exponent_digits: usize) -> String {
    if value.is_nan() {
        return "Nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "Inf".to_owned()
        } else {
            "-Inf".to_owned()
        };
    }
    let digits = digits.clamp(0, 17);
    // The mantissa digit count the RTL keeps is `Precision` when given,
    // else the shortest exact representation capped at 15.
    let text = format!("{:.*e}", digits.saturating_sub(1).min(17), value);
    let (mantissa, exponent) = text.split_once('e').expect("scientific format");
    let exponent: i32 = exponent.parse().expect("exponent");
    let sign = if exponent < 0 { '-' } else { '+' };
    let magnitude = exponent.unsigned_abs().to_string();
    let exponent_digits = exponent_digits.max(3);
    let zeros: String = std::iter::repeat_n('0', exponent_digits.saturating_sub(magnitude.len())).collect();
    format!("{mantissa}E{sign}{zeros}{magnitude}")
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
                delphi::format_general(value, precision as usize)
            }
        }
        FloatFormat::Exponent => float_exponent(value, precision.max(0) as usize, digits.max(0) as usize),
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
    delphi::str_to_float(text).ok_or_else(|| format!("'{text}' is not a valid floating point value"))
}

/// `StrToInt`, with the RTL's `EConvertError` text on a bad value.
pub fn str_to_int(text: &str) -> Result<i32, String> {
    xedit_script::values::parse_int_literal(text.as_bytes())
        .and_then(|value| i32::try_from(value).ok())
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
        assert_eq!(text("%1.1f", vec![Value::Double(2.25)]), "2.3");
        assert_eq!(text("%x", vec![int(255)]), "ff");
        assert_eq!(text("%X", vec![int(255)]), "FF");
        assert_eq!(text("%%d", vec![int(3)]), "%d");
        assert_eq!(text("%d%%", vec![int(3)]), "3%");
        // Unknown conversions are literal, as the RTL leaves them.
        assert_eq!(text("%q", vec![int(3)]), "%q");
        // A missing argument is the RTL's conversion error.
        assert!(format_text(b"%d", &[]).is_err());
    }

    /// `%n` groups with the en-US thousand separator and honours the
    /// precision, which is the `%.0n` the corpus uses.
    #[test]
    fn number_and_currency_formats() {
        assert_eq!(text("%.0n", vec![Value::Double(1234567.0)]), "1,234,567");
        assert_eq!(text("%n", vec![Value::Double(1234.5)]), "1,234.50");
        assert_eq!(text("%m", vec![Value::Double(1234.5)]), "$1,234.50");
        // `-1.005` is the binary `-1.00499999999999989...`, so the RTL
        // (and the port) round it to `-1.00`.
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
