// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Behaviour of the Delphi run-time library that xEdit output depends on.
//!
//! The oracle is a 64-bit build, where `Extended` is a 64-bit float.

/// Significant digits that `FloatToDecimal` produces before it rounds to the
/// requested number of decimals.
// Not confirmed against the oracle yet. The parity harness decides.
const FLOAT_TO_DECIMAL_PRECISION: usize = 18;

/// Port of `ParamStr(0)`: the path of the running program.
pub fn exe_path() -> String {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_default()
}

/// Port of `ExtractFilePath`: the directory part of a path, with its
/// trailing separator.
pub fn extract_file_path(path: &str) -> String {
    match path.rfind(['\\', '/', ':']) {
        Some(index) => path[..=index].to_owned(),
        None => String::new(),
    }
}

/// Port of `TFile.ReadAllLines`.
///
/// Panics when the file cannot be read, as the Delphi exception would.
pub fn read_all_lines(path: &str) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("reading {path}: {error}"));
    text.lines().map(str::to_owned).collect()
}

/// Port of `FloatToStrF(value, ffFixed, 99, digits)` with `.` as the decimal
/// separator, which is what xEdit sets.
///
/// Delphi converts the value to a string of decimal digits and then rounds
/// that string half up, where Rust formatting rounds the binary value half to
/// even. 0.0078125 with 6 decimals is `0.007813` in Delphi.
pub fn float_to_str_f_fixed(value: f64, digits: usize) -> String {
    if value.is_nan() {
        return "NAN".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-INF" } else { "INF" }.to_owned();
    }
    // Mantissa digits and decimal exponent: value = 0.d1d2d3... * 10^exponent.
    let formatted = format!("{:.*e}", FLOAT_TO_DECIMAL_PRECISION - 1, value.abs());
    let (mantissa, exponent) = formatted.split_once('e').expect("exponent format has an exponent");
    let mut decimal: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    let mut exponent: i64 = exponent.parse::<i64>().expect("exponent is a number") + 1;
    if value == 0.0 {
        exponent = 0;
    }

    // Round half up to `digits` decimals: keep `exponent + digits` digits.
    let keep = exponent + digits as i64;
    if keep < 0 {
        decimal.clear();
    } else if (keep as usize) < decimal.len() {
        let round_up = decimal[keep as usize] >= b'5';
        decimal.truncate(keep as usize);
        if round_up {
            let mut index = decimal.len();
            loop {
                if index == 0 {
                    decimal.insert(0, b'1');
                    exponent += 1;
                    break;
                }
                index -= 1;
                if decimal[index] == b'9' {
                    decimal[index] = b'0';
                } else {
                    decimal[index] += 1;
                    break;
                }
            }
        }
    }
    let is_zero = decimal.iter().all(|&digit| digit == b'0');
    if is_zero {
        exponent = 0;
    }

    let mut result = String::new();
    // A value that rounds to zero has no sign.
    if value < 0.0 && !is_zero {
        result.push('-');
    }
    let digit_at = |position: i64| -> char {
        if position >= 0 && (position as usize) < decimal.len() {
            char::from(decimal[position as usize])
        } else {
            '0'
        }
    };
    if exponent <= 0 {
        result.push('0');
    } else {
        for position in 0..exponent {
            result.push(digit_at(position));
        }
    }
    if digits > 0 {
        result.push('.');
        for position in exponent..exponent + digits as i64 {
            result.push(digit_at(position));
        }
    }
    result
}

/// Port of `FloatToStr`: up to 15 significant digits, in scientific notation
/// when the value is below 0.00001 or has more than 15 digits before the point.
pub fn float_to_str(value: f64) -> String {
    if value.is_nan() {
        return "NAN".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-INF" } else { "INF" }.to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let formatted = format!("{:.14e}", value.abs());
    let (mantissa, exponent) = formatted.split_once('e').expect("exponent format has an exponent");
    let exponent: i32 = exponent.parse().expect("exponent is a number");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut result = String::new();
    if value < 0.0 {
        result.push('-');
    }
    if (-5..15).contains(&exponent) {
        if exponent < 0 {
            result.push_str("0.");
            result.push_str(&"0".repeat((-exponent - 1) as usize));
            result.push_str(digits);
        } else {
            let point = exponent as usize + 1;
            if digits.len() <= point {
                result.push_str(digits);
                result.push_str(&"0".repeat(point - digits.len()));
            } else {
                result.push_str(&digits[..point]);
                result.push('.');
                result.push_str(&digits[point..]);
            }
        }
    } else {
        result.push_str(&digits[..1]);
        if digits.len() > 1 {
            result.push('.');
            result.push_str(&digits[1..]);
        }
        result.push('E');
        result.push_str(&exponent.to_string());
    }
    result
}

/// Upstream `HalfMaxValue`: the bits of the largest finite half-float.
pub const HALF_MAX_VALUE: u16 = 0x7BFF;
/// Upstream `HalfMinValue`.
pub const HALF_MIN_VALUE: u16 = 0x0400;
/// Delphi `MaxSingle`.
pub const MAX_SINGLE: f64 = f32::MAX as f64;
/// Delphi `MaxDouble`.
pub const MAX_DOUBLE: f64 = f64::MAX;

/// Port of `HalfToFloat`: the exact value of an IEEE half-float.
pub fn half_to_float(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from(bits >> 10) & 0x1F;
    let mantissa = u32::from(bits) & 0x3FF;
    let single = match (exponent, mantissa) {
        (0, 0) => sign,
        (0, _) => {
            // Subnormal: shift the mantissa up until its leading bit is implicit.
            let shift = mantissa.leading_zeros() - 21;
            let mantissa = (mantissa << shift) & 0x3FF;
            sign | ((113 - shift) << 23) | (mantissa << 13)
        }
        (0x1F, _) => sign | 0x7F80_0000 | (mantissa << 13),
        _ => sign | ((exponent + 112) << 23) | (mantissa << 13),
    };
    f32::from_bits(single)
}

/// Port of `IntPower`.
pub fn int_power(base: f64, exponent: i32) -> f64 {
    let mut y = exponent.unsigned_abs();
    let mut base = base;
    let mut result = 1.0;
    while y > 0 {
        while y & 1 == 0 {
            y >>= 1;
            base *= base;
        }
        y -= 1;
        result *= base;
    }
    if exponent < 0 { 1.0 / result } else { result }
}

/// Port of xEdit `RoundToEx`: rounds to the decimal position `digit`, where -6
/// keeps six decimals. `None` when the scaled value does not fit an `Int64`,
/// which raises a floating point exception in Delphi.
pub fn round_to_ex(value: f64, digit: i32) -> Option<f64> {
    let factor = int_power(10.0, digit);
    let scaled = value / factor;
    // 2^63. NaN does not fit either.
    if scaled.is_nan() || scaled.abs() >= 9_223_372_036_854_775_808.0 {
        return None;
    }
    Some(round(scaled) as f64 * factor)
}

/// Port of xEdit `SingleSameValue`: compares as single precision floats with a
/// relative resolution of 0.0000005.
pub fn single_same_value(a: f64, b: f64) -> bool {
    const SINGLE_RESOLUTION: f32 = 0.000_000_5;
    let (a, b) = (a as f32, b as f32);
    (a - b).abs() <= (a.abs().min(b.abs()) * SINGLE_RESOLUTION).max(SINGLE_RESOLUTION)
}

/// Port of `Round`: rounds half to even, as the default FPU rounding mode does.
pub fn round(value: f64) -> i64 {
    value.round_ties_even() as i64
}

/// Port of `StrToFloat` with `.` as the decimal separator.
pub fn str_to_float(text: &str) -> Option<f64> {
    let text = text.trim_matches(' ');
    let valid = !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.' | b'e' | b'E'));
    if valid { text.parse().ok() } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_format() {
        assert_eq!(float_to_str_f_fixed(0.1, 6), "0.100000");
        assert_eq!(float_to_str_f_fixed(1.0, 6), "1.000000");
        assert_eq!(float_to_str_f_fixed(-14.5, 2), "-14.50");
        assert_eq!(float_to_str_f_fixed(123456.789, 3), "123456.789");
        assert_eq!(float_to_str_f_fixed(0.0, 6), "0.000000");
        assert_eq!(float_to_str_f_fixed(42.0, 0), "42");
        assert_eq!(float_to_str_f_fixed(0.000001, 6), "0.000001");
        assert_eq!(float_to_str_f_fixed(f64::from(0.1f32), 6), "0.100000");
    }

    #[test]
    fn ties_round_half_up() {
        assert_eq!(float_to_str_f_fixed(0.0078125, 6), "0.007813");
        assert_eq!(float_to_str_f_fixed(0.5, 0), "1");
        assert_eq!(float_to_str_f_fixed(2.5, 0), "3");
        assert_eq!(float_to_str_f_fixed(-0.125, 2), "-0.13");
        assert_eq!(float_to_str_f_fixed(0.999_999_9, 6), "1.000000");
        assert_eq!(float_to_str_f_fixed(9.9996, 3), "10.000");
    }

    #[test]
    fn small_values_lose_their_sign() {
        assert_eq!(float_to_str_f_fixed(-0.000_000_1, 6), "0.000000");
        assert_eq!(float_to_str_f_fixed(-0.0, 6), "0.000000");
        assert_eq!(float_to_str_f_fixed(0.000_000_4, 6), "0.000000");
        assert_eq!(float_to_str_f_fixed(0.000_000_6, 6), "0.000001");
    }

    #[test]
    fn special_values() {
        assert_eq!(float_to_str_f_fixed(f64::NAN, 6), "NAN");
        assert_eq!(float_to_str_f_fixed(f64::INFINITY, 6), "INF");
        assert_eq!(float_to_str_f_fixed(f64::NEG_INFINITY, 6), "-INF");
    }

    #[test]
    fn general_format() {
        assert_eq!(float_to_str(0.0), "0");
        assert_eq!(float_to_str(1.5), "1.5");
        assert_eq!(float_to_str(-100.0), "-100");
        assert_eq!(float_to_str(0.1), "0.1");
        assert_eq!(float_to_str(0.00001), "0.00001");
        assert_eq!(float_to_str(0.000001), "1E-6");
        assert_eq!(float_to_str(1e20), "1E20");
        assert_eq!(float_to_str(1.5e-10), "1.5E-10");
        assert_eq!(float_to_str(123456789012345.0), "123456789012345");
        assert_eq!(float_to_str(1234567890123456.0), "1.23456789012346E15");
    }

    #[test]
    fn halves() {
        assert_eq!(half_to_float(0x3C00), 1.0);
        assert_eq!(half_to_float(0xC000), -2.0);
        assert_eq!(half_to_float(0x7BFF), 65504.0);
        assert_eq!(half_to_float(0x0001), 5.960_464_5e-8);
        assert_eq!(half_to_float(0x03FF), 6.097_555e-5);
        assert_eq!(half_to_float(0x0400), 6.103_515_6e-5);
        assert!(half_to_float(0x7C00).is_infinite());
        assert!(half_to_float(0x7FFF).is_nan());
        assert_eq!(half_to_float(0x8000).to_bits(), 0x8000_0000);
    }

    #[test]
    fn rounding_helpers() {
        assert_eq!(int_power(10.0, 3), 1000.0);
        assert_eq!(int_power(10.0, -6), 1.0 / 1_000_000.0);
        assert_eq!(int_power(2.0, 0), 1.0);
        assert_eq!(round_to_ex(1.2345678, -3), Some(1.235));
        assert_eq!(round_to_ex(1e20, -6), None);
        assert_eq!(round_to_ex(-0.0, -6), Some(0.0));
        assert!(single_same_value(4.0e-7, 0.0));
        assert!(!single_same_value(6.0e-7, 0.0));
    }

    #[test]
    fn round_and_parse() {
        assert_eq!(round(0.5), 0);
        assert_eq!(round(1.5), 2);
        assert_eq!(round(-2.5), -2);
        assert_eq!(str_to_float("1.25"), Some(1.25));
        assert_eq!(str_to_float("abc"), None);
        assert_eq!(str_to_float("inf"), None);
    }
}
