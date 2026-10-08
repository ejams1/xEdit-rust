// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Behaviour of the Delphi run-time library that xEdit output depends on.
//!
//! The oracle is a 64-bit build, where `Extended` is a 64-bit float.

/// Port of `TFloatRec`: the significant digits of a value, already rounded
/// to the requested precision and decimals, with `value = 0.d1d2... *
/// 10^exponent`. No digits means zero.
struct FloatRec {
    exponent: i32,
    negative: bool,
    digits: Vec<u8>,
}

/// Significant digits a `Double` build of `FloatToText` keeps:
/// `CMaxExtPrecision` without ten byte extendeds.
const MAX_EXT_PRECISION: i32 = 17;

/// The tables of `Power10` in the 64-bit RTL (`System.Power10` at
/// 0x40CDF0 in `Sniff.exe`): each power of ten as a pair of doubles, a
/// high part of 26 significant bits and the rest, so that a product with
/// them keeps about 80 bits until its last rounding.
/// `10^i`, `i` in 0..32.
const POW10_TAB0: [(u64, u64); 32] = [
    (0x3ff0000000000000, 0x0000000000000000),
    (0x4024000000000000, 0x0000000000000000),
    (0x4059000000000000, 0x0000000000000000),
    (0x408f400000000000, 0x0000000000000000),
    (0x40c3880000000000, 0x0000000000000000),
    (0x40f86a0000000000, 0x0000000000000000),
    (0x412e848000000000, 0x0000000000000000),
    (0x416312d000000000, 0x0000000000000000),
    (0x4197d78400000000, 0x0000000000000000),
    (0x41cdcd6500000000, 0x0000000000000000),
    (0x4202a05f20000000, 0x0000000000000000),
    (0x42374876e8000000, 0x0000000000000000),
    (0x426d1a94a0000000, 0x40b0000000000000),
    (0x42a2309ce4000000, 0x40e4000000000000),
    (0x42d6bcc41c000000, 0x4124800000000000),
    (0x430c6bf524000000, 0x4151a00000000000),
    (0x4341c37934000000, 0x419f040000000000),
    (0x4376345784000000, 0x41bd8a0000000000),
    (0x43abc16d64000000, 0x41fa764000000000),
    (0x43e158e460000000, 0x421227a000000000),
    (0x4415af1d78000000, 0x4246b18800000000),
    (0x444b1ae4d4000000, 0x4297177a80000000),
    (0x4480f0cf04000000, 0x42d26eac90000000),
    (0x44b52d02c4000000, 0x430f0a57b4000000),
    (0x44ea784378000000, 0x432d99db42000000),
    (0x45208b2a2c000000, 0x433401484a000000),
    (0x4554adf4b4000000, 0x43a99019a5c80000),
    (0x4589d971e4000000, 0x43bfd0803ce80000),
    (0x45c027e72c000000, 0x4418f89409844000),
    (0x45f431e0f8000000, 0x444736b90be55000),
    (0x46293e5938000000, 0x446a08ce9dbd4800),
    (0x465f8def88000000, 0x44416048a5934000),
];
/// `10^(32 i)`, `i` in 0..8.
const POW10_TAB1: [(u64, u64); 8] = [
    (0x3ff0000000000000, 0x0000000000000000),
    (0x4693b8b5b4000000, 0x44d056e16b3be040),
    (0x4d384f03e8000000, 0x4b73ff9f4daa797f),
    (0x53ddf67560000000, 0x5226c59b14a2c5d0),
    (0x5a827748f8000000, 0x58c301d319bf8cde),
    (0x6126c2d424000000, 0x5f66ffcc2f54aef7),
    (0x67cc0e1ef0000000, 0x660a724eaad50b78),
    (0x6e714a52dc000000, 0x6ccfe33cc92f82bd),
];
/// `10^-i`, `i` in 0..32.
const POW10_NEG_TAB0: [(u64, u64); 32] = [
    (0x3ff0000000000000, 0x0000000000000000),
    (0x3fb9999998000000, 0x3df999999999999a),
    (0x3f847ae144000000, 0x3ddd70a3d70a3d71),
    (0x3f50624dd0000000, 0x3da78d4fdf3b645a),
    (0x3f1a36e2e8000000, 0x3d68e219652bd3c3),
    (0x3ee4f8b588000000, 0x3d1c6d1e108c3f3e),
    (0x3eb0c6f7a0000000, 0x3ce6bdb1a6d698fe),
    (0x3e7ad7f298000000, 0x3cc5e57a42bc3d33),
    (0x3e45798ee0000000, 0x3c918461cefcfdc2),
    (0x3e112e0be8000000, 0x3c236b4a59731681),
    (0x3ddb7cdfd8000000, 0x3c1d7bdbab7d6ae7),
    (0x3da5fd7fe0000000, 0x3be7964955fdef1f),
    (0x3d71979980000000, 0x3bb2dea11197f27f),
    (0x3d3c25c268000000, 0x3b525da07099432d),
    (0x3d06849b84000000, 0x3b55095cd80f5385),
    (0x3cd203af9c000000, 0x3b273ab0acd90f9d),
    (0x3c9cd2b294000000, 0x3aeec44de15b4c2f),
    (0x3c670ef544000000, 0x3ab236a4b44909bf),
    (0x3c32725dd0000000, 0x3a7d243aba0e75fe),
    (0x3bfd83c94c000000, 0x3a4db69561a52b32),
    (0x3bc79ca10c000000, 0x39f248446baa23d3),
    (0x3b92e3b408000000, 0x39e074da7beed3f7),
    (0x3b5e392010000000, 0x39575ee5962a6499),
    (0x3b282db340000000, 0x3922b25144eeb6e1),
    (0x3af357c298000000, 0x393a88ea76a58925),
    (0x3abef2d0f4000000, 0x38fda7dd8aa27508),
    (0x3a88c240c4000000, 0x38b5d962776a54d9),
    (0x3a53ce9a34000000, 0x38a791e07e48775f),
    (0x3a1fb0f6bc000000, 0x38628300ca0d8bcb),
    (0x39e95a5efc000000, 0x3835359a3b3e096f),
    (0x39b4484bfc000000, 0x38075e14fc31a126),
    (0x398039d664000000, 0x37c89687f9e901d6),
];
/// `10^(-32 i)`, `i` in 0..8.
const POW10_NEG_TAB1: [(u64, u64); 8] = [
    (0x3ff0000000000000, 0x0000000000000000),
    (0x3949f623d4000000, 0x378a8a732974cfbc),
    (0x32a50ffd44000000, 0x30de94e7a694fc8e),
    (0x2c0116805c000000, 0x2a57fd75539b11dc),
    (0x255bba08cc000000, 0x23ac64bce4a0ac7d),
    (0x1eb67e9c10000000, 0x1d03db73a09359ed),
    (0x18123ff06c000000, 0x16675423cc067b63),
    (0x116d9ca79c000000, 0x0fa894629d7b49f1),
];
/// `10^256` as a pair.
const POW10_256: (u64, u64) = (0x75154fdd7c000000, 0x736b9df9de8dddbc);
/// `10^-256` as a pair scaled by 2^850, and the scale `2^-850`.
const POW10_NEG_256: (u64, u64) = (0x3fe8062864000000, 0x3e158de864e7ea44);
const POW10_NEG_256_SCALE: u64 = 0x0ad0000000000000;

/// The product of `value` and a table pair (at 0x40CD70): `value` split
/// into a high part of 26 bits and the rest, the partial products added
/// from the smallest.
fn mul_pair(value: f64, (hi, lo): (u64, u64)) -> f64 {
    let (hi, lo) = (f64::from_bits(hi), f64::from_bits(lo));
    let vh = f64::from_bits(value.to_bits() & 0xffff_ffff_f800_0000);
    let vl = value - vh;
    if lo == 0.0 {
        vh * hi + vl * hi
    } else {
        ((vl * lo + vh * lo) + vl * hi) + vh * hi
    }
}

/// Port of `Power10` of the 64-bit RTL, from the machine code of
/// `Sniff.exe`: `value * 10^power` as the product of up to four table
/// pairs (`mul_pair`), negative powers by the reciprocal tables. The
/// result is near the correctly rounded one but not always it, and
/// `FloatToDecimal` turns its last bits into digits (`1.05E-5` from
/// `0.000014 * 0.75` gives `0.000011` with six decimals, found by `parity
/// sniff` on the vertices of Skyrim meshes).
fn power10(value: f64, power: i32) -> f64 {
    let mut result = value;
    if power > 0 {
        if power >= 632 {
            return f64::INFINITY;
        }
        if power & 0x1F != 0 {
            result = mul_pair(result, POW10_TAB0[(power & 0x1F) as usize]);
        }
        let p = power >> 5;
        if p != 0 {
            if p & 7 != 0 {
                result = mul_pair(result, POW10_TAB1[(p & 7) as usize]);
            }
            for _ in 0..(p >> 3) {
                result = mul_pair(result, POW10_256);
            }
        }
    } else if power < 0 {
        let p = -power;
        if p >= 632 {
            return 0.0;
        }
        if p & 0x1F != 0 {
            result = mul_pair(result, POW10_NEG_TAB0[(p & 0x1F) as usize]);
        }
        let p = p >> 5;
        if p != 0 {
            if p & 7 != 0 {
                result = mul_pair(result, POW10_NEG_TAB1[(p & 7) as usize]);
            }
            for _ in 0..(p >> 3) {
                result = mul_pair(result, POW10_NEG_256) * f64::from_bits(POW10_NEG_256_SCALE);
            }
        }
    }
    result
}

/// Port of `ExtToDecimal` in the `PUREPASCAL` `FloatToDecimal` of a 64-bit
/// build, where `Extended` is a `Double`. The value is finite.
///
/// The decimal exponent is estimated from the binary exponent alone, so it
/// is one too small when the value crosses a power of ten inside its
/// binade. The value is scaled by `Power10` into an 18 digit integer with
/// banker's rounding and divided by ten when the estimate was too small.
/// Both steps round in double precision, so the last digits are not those
/// of the exact value (`14060.7578125` becomes `140607578124999984`) and
/// the rounding to `decimals` that follows, half up on the digit string,
/// sees those digits. `precision` is at most 18.
fn float_to_decimal(value: f64, precision: i32, decimals: i32) -> FloatRec {
    let bits = value.to_bits();
    let mut negative = bits >> 63 != 0;
    let mut exp = ((bits >> 52) & 0x7FF) as i32;
    let value = value.abs();
    if exp == 0 && value == 0.0 {
        return FloatRec {
            exponent: 0,
            negative: false,
            digits: Vec::new(),
        };
    }
    if exp == 0 {
        // Denormalized: the exponent of the leading mantissa bit.
        let mut n = value.to_bits() as i64;
        while n & 0x0008_0000_0000_0000 == 0 {
            exp -= 1;
            n <<= 1;
        }
    }
    exp -= 0x3FF;
    // `exp10 = exp2 * log10(2)`, with `log10(2) * 2^16 ~= 19728`, taking the
    // high 16 bits of the product sign extended.
    exp = ((exp * 19728) >> 16) + 1;
    let mut exponent = exp;
    let mut scaled = power10(value, 18 - exp).round_ties_even();
    if scaled >= 1e18 {
        scaled /= 10.0;
        exponent += 1;
    }
    // `GetBcdBytes`: `Round` to an `Int64`, then the digits in pairs.
    let mut int = scaled.round_ties_even() as i64;
    let mut digits = vec![b'0'; 18];
    for index in 0..9 {
        let pair = (int % 100) as u8;
        digits[16 - index * 2] = b'0' + pair / 10;
        digits[17 - index * 2] = b'0' + pair % 10;
        int /= 100;
    }

    if exponent + decimals < 0 {
        return FloatRec {
            exponent: 0,
            negative: false,
            digits: Vec::new(),
        };
    }
    let mut j = (exponent + decimals).min(precision);
    if j >= 18 || digits[j as usize] < b'5' {
        // Round down: cut at `j` and drop the trailing zeroes.
        digits.truncate(j.min(18) as usize);
        while digits.last() == Some(&b'0') {
            digits.pop();
        }
        if digits.is_empty() {
            negative = false;
        }
    } else {
        // Round up with carry.
        digits.truncate(j as usize);
        loop {
            if j == 0 {
                digits = vec![b'1'];
                exponent += 1;
                break;
            }
            j -= 1;
            digits[j as usize] += 1;
            if digits[j as usize] <= b'9' {
                break;
            }
            digits.truncate(j as usize);
        }
    }
    FloatRec {
        exponent,
        negative,
        digits,
    }
}

/// The digits of a `FloatRec` in order, `'0'` once they run out, as
/// `GetDigit` in `InternalFloatToText`.
struct DigitSource<'a> {
    digits: &'a [u8],
    position: usize,
}

impl DigitSource<'_> {
    fn next(&mut self) -> char {
        match self.digits.get(self.position) {
            Some(&digit) => {
                self.position += 1;
                char::from(digit)
            }
            None => '0',
        }
    }

    fn has_more(&self) -> bool {
        self.position < self.digits.len()
    }
}

/// Port of `FormatNumber` in `InternalFloatToText` for `ffFixed`, with `.`
/// as the decimal separator: the integer digits, then `digits` decimals.
fn format_fixed_rec(rec: &FloatRec, digits: i32) -> String {
    let mut result = String::new();
    if rec.negative {
        result.push('-');
    }
    let mut source = DigitSource {
        digits: &rec.digits,
        position: 0,
    };
    let mut remaining = digits.min(MAX_EXT_PRECISION);
    let mut k = rec.exponent;
    if k > 0 {
        while k > 0 {
            result.push(source.next());
            k -= 1;
        }
    } else {
        result.push('0');
    }
    if remaining != 0 {
        result.push('.');
        while k < 0 && remaining > 0 {
            result.push('0');
            k += 1;
            remaining -= 1;
        }
        while remaining > 0 {
            result.push(source.next());
            remaining -= 1;
        }
    }
    result
}

/// Port of the `ffGeneral` branch of `InternalFloatToText`: plain notation
/// when the exponent is within `-3..=precision`, otherwise one digit, the
/// rest after the point and an `E` exponent padded to `digits` places when
/// `digits` is at most 4.
fn format_general_rec(rec: &FloatRec, precision: i32, digits: i32) -> String {
    let mut result = String::new();
    if rec.negative {
        result.push('-');
    }
    let mut source = DigitSource {
        digits: &rec.digits,
        position: 0,
    };
    let mut count = rec.exponent;
    let use_e_notation = count > precision || count < -3;
    if use_e_notation {
        count = 1;
    }
    if count > 0 {
        while count > 0 {
            result.push(source.next());
            count -= 1;
        }
        if source.has_more() {
            result.push('.');
            while source.has_more() {
                result.push(source.next());
            }
        }
        if use_e_notation {
            // `FormatExponent` without the plus sign of the other formats.
            let min_count = if digits > 4 { 0 } else { digits as usize };
            result.push('E');
            let mut exponent = rec.exponent - 1;
            if rec.digits.is_empty() {
                exponent = 0;
            } else if exponent < 0 {
                exponent = -exponent;
                result.push('-');
            }
            result.push_str(&format!("{exponent:0min_count$}"));
        }
    } else {
        result.push('0');
        if !rec.digits.is_empty() {
            result.push('.');
            for _ in 0..-count {
                result.push('0');
            }
            while source.has_more() {
                result.push(source.next());
            }
        }
    }
    result
}

/// `INF`, `-INF` or `NAN` for a value that is not finite, as `FloatToText`
/// writes them.
fn special_value(value: f64) -> Option<String> {
    if value.is_nan() {
        Some("NAN".to_owned())
    } else if value.is_infinite() {
        Some(if value < 0.0 { "-INF" } else { "INF" }.to_owned())
    } else {
        None
    }
}

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

/// Port of `ExtractFileName`.
pub fn path_file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Port of `ChangeFileExt`: the path with its extension (the part from the
/// last `.` of the file name) replaced by `extension`.
pub fn change_file_ext(path: &str, extension: &str) -> String {
    let name_start = path.rfind(['\\', '/', ':']).map_or(0, |index| index + 1);
    let stem_end = path[name_start..].rfind('.').map_or(path.len(), |dot| name_start + dot);
    format!("{}{extension}", &path[..stem_end])
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
/// The precision of 99 clamps to 17 significant digits. The value becomes
/// decimal digits through `FloatToDecimal`, whose scaling in double
/// precision decides how a tie rounds: 0.0078125 with 6 decimals is
/// `0.007813`, but 14060.7578125 is `14060.757812`.
pub fn float_to_str_f_fixed(value: f64, digits: usize) -> String {
    if let Some(special) = special_value(value) {
        return special;
    }
    let digits = i32::try_from(digits).unwrap_or(i32::MAX);
    let rec = float_to_decimal(value, MAX_EXT_PRECISION, digits);
    // `FloatToText` switches to the general format when the integer part
    // has more digits than the precision.
    if rec.exponent > MAX_EXT_PRECISION {
        format_general_rec(&rec, MAX_EXT_PRECISION, digits)
    } else {
        format_fixed_rec(&rec, digits)
    }
}

/// Port of `FloatToStr`: up to 15 significant digits, in scientific notation
/// when the value is below 0.0001 or has more than 15 digits before the point.
pub fn float_to_str(value: f64) -> String {
    float_to_text_general(value, 15, 0)
}

/// Port of `Format('%.*g', [precision, value])`: up to `precision`
/// significant digits, with the exponent of the scientific notation padded
/// to three places.
pub fn format_general(value: f64, precision: usize) -> String {
    float_to_text_general(value, precision, 3)
}

/// Port of `FloatToText(value, fvExtended, ffGeneral, precision, digits)`.
fn float_to_text_general(value: f64, precision: usize, digits: i32) -> String {
    if let Some(special) = special_value(value) {
        return special;
    }
    let precision = i32::try_from(precision).unwrap_or(i32::MAX).clamp(2, MAX_EXT_PRECISION);
    // `CGenExpDigits`: the general format rounds to the precision only.
    let rec = float_to_decimal(value, precision, 9999);
    format_general_rec(&rec, precision, digits)
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

/// Port of `FloatToHalf` for a double: the exponent range is checked on the
/// double, then the single conversion runs.
pub fn float_to_half(value: f64) -> u16 {
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7FF) as i32 - 1023;
    if exponent < -126 {
        0
    } else if exponent > 127 {
        if exponent == 1024 && bits & 0x000F_FFFF_FFFF_FFFF != 0 {
            HALF_NAN
        } else if bits & 0x8000_0000_0000_0000 == 0 {
            HALF_POS_INF
        } else {
            HALF_NEG_INF
        }
    } else {
        single_to_half(value as f32)
    }
}

/// Upstream `HalfNaN`, `HalfPosInf` and `HalfNegInf`.
pub const HALF_NAN: u16 = 0x7FFF;
pub const HALF_POS_INF: u16 = 0x7C00;
pub const HALF_NEG_INF: u16 = 0xFC00;

/// Port of `FloatToHalf` for a single: Jeroen van der Zijp's table
/// conversion, computed instead of tabled. The mantissa is truncated, not
/// rounded, as the tables do.
pub fn single_to_half(value: f32) -> u16 {
    let bits = value.to_bits();
    let exp_sign = bits >> 23;
    let exponent = (exp_sign & 0xFF) as i32 - 127;
    let sign = ((exp_sign & 0x100) << 7) as u16;
    let mantissa = bits & 0x007F_FFFF;
    let (base, shift): (u16, u32) = if exponent < -24 {
        // Too small: signed zero.
        (sign, 24)
    } else if exponent < -14 {
        // Subnormal half: the leading bit of the single is explicit.
        let base = sign | (0x0400u16 >> (-14 - exponent) as u32);
        (base, (-1 - exponent) as u32)
    } else if exponent <= 15 {
        (sign | (((exponent + 15) as u16) << 10), 13)
    } else if exponent < 128 {
        // Too large: infinity.
        (sign | 0x7C00, 24)
    } else {
        // Infinity or NaN: the mantissa bits that fit stay.
        (sign | 0x7C00, 13)
    };
    base.wrapping_add((mantissa >> shift) as u16)
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

/// Port of the `Single` overload of `IntPower`: the base is squared in
/// double precision, the result is rounded to single after every
/// multiplication and the reciprocal is taken in double.
pub fn int_power_single(base: f32, exponent: i32) -> f32 {
    let mut y = exponent.unsigned_abs();
    let mut base = f64::from(base);
    let mut result = 1.0_f32;
    while y > 0 {
        while y & 1 == 0 {
            y >>= 1;
            base *= base;
        }
        y -= 1;
        result = (f64::from(result) * base) as f32;
    }
    if exponent < 0 {
        (1.0 / f64::from(result)) as f32
    } else {
        result
    }
}

/// Port of xEdit `RoundToEx`: rounds to the decimal position `digit`, where -6
/// keeps six decimals.
///
/// UPSTREAM-QUIRK: `IntPower(10, ADigit)` binds to the `Single` overload, so
/// the factor is the single-precision `10^digit` (for example
/// `9.99999997e-7` for six decimals); the value is divided by it, rounded,
/// and multiplied by it again in double precision, which puts the result
/// one unit off correct rounding for values near a tie.
///
/// UPSTREAM-QUIRK: `TwbFloatDef.ToValue` masks the floating point
/// exceptions, so `Round` of a scaled value outside the `Int64` range gives
/// the x64 integer indefinite, `-2^63`, and the oracle prints
/// `-9223372013568` for such a single with six digits.
pub fn round_to_ex(value: f64, digit: i32) -> f64 {
    let factor = f64::from(int_power_single(10.0, digit));
    let scaled = value / factor;
    // 2^63. NaN does not fit either.
    let rounded = if scaled.is_nan() || scaled.abs() >= 9_223_372_036_854_775_808.0 {
        i64::MIN
    } else {
        round(scaled)
    };
    rounded as f64 * factor
}

/// Port of xEdit `SingleSameValue`: compares as single precision floats with a
/// relative resolution of 0.0000005.
pub fn single_same_value(a: f64, b: f64) -> bool {
    const SINGLE_RESOLUTION: f32 = 0.000_000_5;
    let (a, b) = (a as f32, b as f32);
    (a - b).abs() <= (a.abs().min(b.abs()) * SINGLE_RESOLUTION).max(SINGLE_RESOLUTION)
}

/// Port of `SameValue` for doubles with the default epsilon.
pub fn same_value(a: f64, b: f64) -> bool {
    const DOUBLE_RESOLUTION: f64 = 1E-15;
    (a - b).abs() <= (a.abs().min(b.abs()) * DOUBLE_RESOLUTION).max(DOUBLE_RESOLUTION)
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
    fn power10_rounds_as_the_rtl() {
        // The products of six decimal texts and 0.75 that `Sniff.exe`'s
        // `Apply transformation` writes (a probe of 239 vertices); the
        // first two round up only with the RTL's paired tables.
        for (value, text) in [
            (1.4e-5, "0.000011"),
            (1.8e-5, "0.000014"),
            (2.6e-5, "0.000020"),
            (5.4e-5, "0.000040"),
            (1.1e-4, "0.000083"),
        ] {
            assert_eq!(float_to_str_f_fixed(value * 0.75, 6), text, "{value}");
        }
    }

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
        assert_eq!(float_to_str(0.0001), "0.0001");
        assert_eq!(float_to_str(0.00001), "1E-5");
        assert_eq!(float_to_str(1e20), "1E20");
        assert_eq!(float_to_str(1.5e-10), "1.5E-10");
        assert_eq!(float_to_str(123456789012345.0), "123456789012345");
        assert_eq!(float_to_str(1234567890123456.0), "1.23456789012346E15");
        // `Format('%g')` pads the exponent to three places.
        assert_eq!(format_general(1e20, 5), "1E020");
        assert_eq!(format_general(100.0, 5), "100");
        assert_eq!(format_general(123456.0, 5), "1.2346E005");
    }

    #[test]
    #[allow(clippy::excessive_precision)]
    fn ties_follow_the_double_scaling() {
        // Exact ties in binary that the oracle rounds down because the
        // decimal exponent estimate is one too small and the scaled value
        // is divided by ten in double precision.
        assert_eq!(float_to_str_f_fixed(-14060.7578125, 6), "-14060.757812");
        assert_eq!(float_to_str_f_fixed(f64::from(-14060.7578125f32), 6), "-14060.757812");
        // The precision clamps to 17 digits.
        assert_eq!(float_to_str_f_fixed(123456789012.345678, 6), "123456789012.345680");
        assert_eq!(float_to_str_f_fixed(1e18, 6), "1E18");
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
        assert_eq!(format!("{:.3}", round_to_ex(1.2345678, -3)), "1.235");
        // The oracle prints 9823.924804 and 14043.040040 for these singles.
        assert_eq!(format!("{:.6}", round_to_ex(9823.9248046875, -6)), "9823.924804");
        assert_eq!(format!("{:.6}", round_to_ex(14043.0400390625, -6)), "14043.040040");
        assert_eq!(format!("{:.6}", round_to_ex(451.0703125, -6)), "451.070313");
        assert_eq!(format!("{:.6}", round_to_ex(3515.0703125, -6)), "3515.070312");
        // The 18 digit mantissa is scaled in double precision: the oracle
        // prints 4294953215.999999 for this water height.
        assert_eq!(
            float_to_str_f_fixed(round_to_ex(4294953216.0, -6), 6),
            "4294953215.999999"
        );
        // Out of the `Int64` range: the oracle prints -9223372013568.
        assert_eq!(float_to_str_f_fixed(round_to_ex(1e20, -6), 6), "-9223372013568.000000");
        assert_eq!(
            float_to_str_f_fixed(round_to_ex(f64::INFINITY, -6), 6),
            "-9223372013568.000000"
        );
        assert_eq!(round_to_ex(-0.0, -6), 0.0);
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
