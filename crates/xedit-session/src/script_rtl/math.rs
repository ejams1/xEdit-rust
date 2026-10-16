// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter_Math.pas (the
// `Math` unit: 56 functions of Delphi's Math.pas, the eleven constants and
// the `ArcCotH` registration that binds `ArcCot`'s handler) over the Delphi
// `Math` semantics and the FPU-accurate transcendentals of
// `xedit_assets::nif_math` and `xedit_core::delphi`

//! `JvInterpreter_Math.pas`: the `Math` unit of the script dialect.
//!
//! The formula-semantics of Delphi's `Math.pas` are reproduced where they
//! are observable: `Hypot` scales by the larger operand before squaring
//! (Delphi's overflow guard), `Power` special-cases an integer exponent
//! through `IntPower`, `Log10`/`Log2`/`LogN` divide `Ln`s rather than call
//! the log2 of the hardware, and the degree helpers keep Delphi's exact
//! factors. `Sin`/`Cos`/`Tan`/`ArcSin`/`ArcCos`/`ArcTan`/`ArcTan2` and the
//! degree/radian conversions go through `nif_math`, the port of the FPU
//! machine code the Sniff phase matched.
//!
//! Two registrations are not mathematical: `ClearExceptions` clears the FPU
//! exception flags, which the port's value model does not carry, so it does
//! nothing; `RandG` draws from `System.Random` with the polar method and
//! caches the second value, which is the documented shape of the RTL
//! routine.

use std::cell::Cell;

use xedit_assets::nif_math as nif;
use xedit_script::adapter::Adapter;
use xedit_script::values::Value;

use super::{EMPTY, arg_f64, arg_i32};

/// `cMath = 'Math'` (`JvInterpreter_Math.pas:342`).
const UNIT: &str = "Math";

/// `PI` (`Math.pas`).
const PI: f64 = std::f64::consts::PI;
/// `MaxSingle` = `3.40e+38` as the RTL's constant has it (the exact
/// single `f32::MAX`; parity #39 checked the printed digits against the
/// oracle).
const MAX_SINGLE: f64 = f32::MAX as f64;
/// `MinSingle`: the smallest *normal* single, `1.1754943508222875e-38`
/// (the old `1.5e-45` of Math.pas is not the released 4.1.5q's; parity #39
/// probed it).
const MIN_SINGLE: f64 = f32::MIN_POSITIVE as f64;
/// `MinDouble`: the smallest normal double, `2.2250738585072014e-308`
/// (`MinExtended` on the 64-bit build is the same value; parity #39 probed
/// both).
const MIN_DOUBLE: f64 = f64::MIN_POSITIVE;
/// `MaxComp`.
const MAX_COMP: f64 = 9.223_372_036_854_776e18;
/// `MinComp`.
const MIN_COMP: f64 = -9.223_372_036_854_776e18;

thread_local! {
    /// `RandG`'s cached second normal deviate (`GaussSave`/`GaussDone` of
    /// `Math.pas`).
    static GAUSS_SAVE: Cell<f64> = const { Cell::new(0.0) };
    static GAUSS_DONE: Cell<bool> = const { Cell::new(false) };
}

/// Registers the unit.
pub fn register(adapter: &mut Adapter) {
    // The constants, in the registration order of the unit. `MinExtended`
    // is `MinDouble` on the 64-bit build the oracle is (`SizeOf(Extended)`
    // is 8 there), and the `MaxExtended` registration is commented out.
    consts(adapter, "PI", PI);
    consts(adapter, "Infinity", f64::INFINITY);
    consts(adapter, "MaxComp", MAX_COMP);
    consts(adapter, "MaxDouble", f64::MAX);
    consts(adapter, "MaxSingle", MAX_SINGLE);
    consts(adapter, "MinComp", MIN_COMP);
    consts(adapter, "MinDouble", MIN_DOUBLE);
    consts(adapter, "MinExtended", MIN_DOUBLE);
    consts(adapter, "MinSingle", MIN_SINGLE);
    // The Math NaN is the RTL's real indefinite `0xFFF8...` (parity #39);
    // `f64::NAN` is `0x7FF8...` and showed as a byte difference in the
    // probe's `.dat`.
    consts(adapter, "NaN", xedit_core::delphi::nan());
    consts(adapter, "NegInfinity", f64::NEG_INFINITY);

    // The functions, in the registration order of the unit.
    unary(adapter, "ArcCos", nif::arc_cos);
    unary(adapter, "ArcCosh", |x| (x + (x * x - 1.0).sqrt()).ln());
    unary(adapter, "ArcCot", |x| nif::arc_tan(1.0 / x));
    // UPSTREAM-QUIRK: `ArcCotH` binds `ArcCot`'s handler (`:376`).
    unary(adapter, "ArcCotH", |x| nif::arc_tan(1.0 / x));
    unary(adapter, "ArcCsc", |x| nif::arc_sin(1.0 / x));
    unary(adapter, "ArcCscH", |x| ((1.0 + (1.0 + x * x).sqrt()) / x).ln());
    unary(adapter, "ArcSec", |x| nif::arc_cos(1.0 / x));
    unary(adapter, "ArcSecH", |x| (((1.0 - x * x).sqrt() + 1.0) / x).ln());
    unary(adapter, "ArcSin", nif::arc_sin);
    unary(adapter, "ArcSinh", |x| (x + (x * x + 1.0).sqrt()).ln());
    binary(adapter, "ArcTan2", nif::arc_tan2);
    unary(adapter, "ArcTanh", |x| 0.5 * ((1.0 + x) / (1.0 - x)).ln());
    // `Math.Ceil`/`Floor` narrow to `Integer`: `Result := Trunc(X); if
    // Frac(X) > 0 then Inc(Result)` (and `< 0`/`Dec` for `Floor`), so an
    // out-of-range value gives the RTL's truncated low 32 bits (`1e19`
    // gives 0, `1e10+0.5` gives 1410065409) and NaN gives 0 (parity #39
    // probed every one).
    unary_int(adapter, "Ceil", |x| {
        let mut result = xedit_core::delphi::trunc(x) as i32;
        if x.fract() > 0.0 {
            result = result.wrapping_add(1);
        }
        result
    });
    super::function(adapter, UNIT, "ClearExceptions", &[], |_value, _args| {
        // `Math.ClearExceptions` clears the FPU exception flags; the port
        // has no FPU exception state to clear.
        Ok(())
    });
    unary(adapter, "Cosecant", |x| 1.0 / nif::sin(x));
    unary(adapter, "Cosh", |x| (x.exp() + (-x).exp()) / 2.0);
    unary(adapter, "Cot", |x| nif::cos(x) / nif::sin(x));
    unary(adapter, "Cotan", |x| 1.0 / (nif::sin(x) / nif::cos(x)));
    unary(adapter, "CotH", |x| {
        ((x.exp() + (-x).exp()) / 2.0) / ((x.exp() - (-x).exp()) / 2.0)
    });
    unary(adapter, "Csc", |x| 1.0 / nif::sin(x));
    unary(adapter, "CscH", |x| 1.0 / ((x.exp() - (-x).exp()) / 2.0));
    unary(adapter, "CycleToDeg", |x| x * 360.0);
    unary(adapter, "CycleToGrad", |x| x * 400.0);
    unary(adapter, "CycleToRad", |x| x * 2.0 * PI);
    unary(adapter, "DegToCycle", |x| x / 360.0);
    unary(adapter, "DegToGrad", |x| x / 0.9);
    unary(adapter, "DegToRad", nif::deg_to_rad);
    unary_int(adapter, "Floor", |x| {
        let mut result = xedit_core::delphi::trunc(x) as i32;
        if x.fract() < 0.0 {
            result = result.wrapping_sub(1);
        }
        result
    });
    unary(adapter, "GradToCycle", |x| x / 400.0);
    unary(adapter, "GradToDeg", |x| x * 0.9);
    unary(adapter, "GradToRad", |x| x * PI / 200.0);
    binary(adapter, "Hypot", delphi_hypot);
    binary(adapter, "IntPower", |x, e| xedit_core::delphi::int_power(x, e as i32));
    unary_bool(adapter, "IsInfinite", |x| x.is_infinite());
    unary_bool(adapter, "IsNan", |x| x.is_nan());
    binary(adapter, "Ldexp", |x, p| x * 2f64.powi(p as i32));
    unary(adapter, "LnXP1", |x| (1.0 + x).ln());
    unary(adapter, "Log10", |x| x.ln() / 10f64.ln());
    unary(adapter, "Log2", |x| x.ln() / 2f64.ln());
    binary(adapter, "LogN", |base, x| x.ln() / base.ln());
    binary_int(adapter, "Max", |a, b| a.max(b));
    binary_int(adapter, "Min", |a, b| a.min(b));
    binary(adapter, "Power", delphi_power);
    unary(adapter, "RadToCycle", |x| x / (2.0 * PI));
    unary(adapter, "RadToDeg", nif::rad_to_deg);
    unary(adapter, "RadToGrad", |x| x * 200.0 / PI);
    binary(adapter, "RandG", rand_g);
    binary_int(adapter, "RandomRange", |from, to| {
        if from > to {
            super::system::random(from - to) + to
        } else {
            super::system::random(to - from) + from
        }
    });
    unary(adapter, "Sec", |x| 1.0 / nif::cos(x));
    unary(adapter, "Secant", |x| 1.0 / nif::cos(x));
    unary(adapter, "SecH", |x| 1.0 / ((x.exp() + (-x).exp()) / 2.0));
    unary(adapter, "Sinh", |x| (x.exp() - (-x).exp()) / 2.0);
    super::function(
        adapter,
        UNIT,
        "SLNDepreciation",
        &[EMPTY, EMPTY, EMPTY],
        |value, args| {
            let cost = arg_f64(args, 0)?;
            let salvage = arg_f64(args, 1)?;
            let life = arg_i32(args, 2)?;
            *value = Value::Double((cost - salvage) / f64::from(life));
            Ok(())
        },
    );
    super::function(
        adapter,
        UNIT,
        "SYDDepreciation",
        &[EMPTY, EMPTY, EMPTY, EMPTY],
        |value, args| {
            let cost = arg_f64(args, 0)?;
            let salvage = arg_f64(args, 1)?;
            let life = arg_i32(args, 2)?;
            let period = arg_i32(args, 3)?;
            *value = Value::Double(
                (cost - salvage) * f64::from(life - period + 1) * 2.0 / (f64::from(life) * f64::from(life + 1)),
            );
            Ok(())
        },
    );
    unary(adapter, "Tan", |x| nif::sin(x) / nif::cos(x));
    unary(adapter, "Tanh", |x| {
        ((x.exp() - (-x).exp()) / 2.0) / ((x.exp() + (-x).exp()) / 2.0)
    });
}

/// `AddConst` of a floating point constant.
fn consts(adapter: &mut Adapter, identifier: &str, value: f64) {
    adapter.add_const(UNIT, identifier, Value::Double(value));
}

/// One `Extended` argument, one `Extended` (double) result.
fn unary(adapter: &mut Adapter, identifier: &str, func: fn(f64) -> f64) {
    super::function(adapter, UNIT, identifier, &[EMPTY], move |value, args| {
        *value = Value::Double(func(arg_f64(args, 0)?));
        Ok(())
    });
}

/// One argument, an integer result (`Ceil`/`Floor`).
fn unary_int(adapter: &mut Adapter, identifier: &str, func: fn(f64) -> i32) {
    super::function(adapter, UNIT, identifier, &[EMPTY], move |value, args| {
        *value = Value::Integer(func(arg_f64(args, 0)?));
        Ok(())
    });
}

/// One argument, a boolean result (`IsInfinite`/`IsNan`).
fn unary_bool(adapter: &mut Adapter, identifier: &str, func: fn(f64) -> bool) {
    super::function(adapter, UNIT, identifier, &[EMPTY], move |value, args| {
        *value = Value::Bool(func(arg_f64(args, 0)?));
        Ok(())
    });
}

/// Two arguments, one `Extended` result.
fn binary(adapter: &mut Adapter, identifier: &str, func: fn(f64, f64) -> f64) {
    super::function(adapter, UNIT, identifier, &[EMPTY, EMPTY], move |value, args| {
        *value = Value::Double(func(arg_f64(args, 0)?, arg_f64(args, 1)?));
        Ok(())
    });
}

/// Two integer arguments, an integer result (`Max`/`Min`/`RandomRange`).
fn binary_int(adapter: &mut Adapter, identifier: &str, func: fn(i32, i32) -> i32) {
    super::function(adapter, UNIT, identifier, &[EMPTY, EMPTY], move |value, args| {
        *value = Value::Integer(func(arg_i32(args, 0)?, arg_i32(args, 1)?));
        Ok(())
    });
}

/// `Math.Hypot`: the scaled square root that does not overflow where the
/// naive `Sqrt(X*X + Y*Y)` would.
fn delphi_hypot(x: f64, y: f64) -> f64 {
    if x.abs() > y.abs() {
        let temp = y / x;
        x.abs() * (1.0 + temp * temp).sqrt()
    } else if y == 0.0 {
        0.0
    } else {
        let temp = x / y;
        y.abs() * (1.0 + temp * temp).sqrt()
    }
}

/// `Math.Power`: an integer exponent goes through `IntPower`, everything
/// else through `Exp(Exponent * Ln(Base))`.
fn delphi_power(base: f64, exponent: f64) -> f64 {
    if exponent == 0.0 {
        1.0
    } else if base == 0.0 && exponent > 0.0 {
        0.0
    } else if exponent.fract() == 0.0 && exponent.abs() <= f64::from(i32::MAX) {
        xedit_core::delphi::int_power(base, exponent as i32)
    } else {
        (exponent * base.ln()).exp()
    }
}

/// `Math.RandG`: the polar method (Marsaglia) drawing two `Random` values at
/// a time and caching the second; the cache is a unit global upstream too.
fn rand_g(mean: f64, std_dev: f64) -> f64 {
    if GAUSS_DONE.with(Cell::get) {
        GAUSS_DONE.with(|cell| cell.set(false));
        return mean + std_dev * GAUSS_SAVE.with(Cell::get);
    }
    let (v1, v2, s) = loop {
        let v1 = 2.0 * super::system::random_unit() - 1.0;
        let v2 = 2.0 * super::system::random_unit() - 1.0;
        let s = v1 * v1 + v2 * v2;
        if s > 0.0 && s < 1.0 {
            break (v1, v2, s);
        }
    };
    let scale = (-2.0 * s.ln() / s).sqrt();
    GAUSS_SAVE.with(|cell| cell.set(v2 * scale));
    GAUSS_DONE.with(|cell| cell.set(true));
    mean + std_dev * v1 * scale
}
