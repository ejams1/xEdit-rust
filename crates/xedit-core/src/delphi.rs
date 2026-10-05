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
    fn round_and_parse() {
        assert_eq!(round(0.5), 0);
        assert_eq!(round(1.5), 2);
        assert_eq!(round(-2.5), -2);
        assert_eq!(str_to_float("1.25"), Some(1.25));
        assert_eq!(str_to_float("abc"), None);
        assert_eq!(str_to_float("inf"), None);
    }
}
