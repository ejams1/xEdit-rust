// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDataFormatNifTypes.pas

//! The callbacks of `wbDataFormatNifTypes` that the transpiler does not
//! translate.

use xedit_core::delphi::{float_to_str, round};

use super::*;
use crate::data_format::{df_float_to_str, df_str_to_float, int_to_hex, split_string};
use crate::nif_math::{
    Matrix33, Quaternion, axis_angle_to_m33, axis_angle_to_quaternion, deg_to_rad, euler_to_m33, euler_to_quaternion,
    m33_to_axis_angle, m33_to_euler, quaternion_to_axis_angle, quaternion_to_euler, rad_to_deg,
};
use crate::variant::{str_to_int, str_to_int64};

fn not_an_integer(text: &str) -> DfError {
    DfError::new(format!("'{text}' is not a valid integer value"))
}

/// Upstream `GetTextHexColor`: hex digits separated by spaces become an
/// HTML colour (`#RRGGBB`).
pub fn get_text_hex_color(_t: &mut Tree, _e: El, a_text: &mut String) -> R<()> {
    if !a_text
        .chars()
        .all(|ch| ch.is_ascii_digit() || ('A'..='F').contains(&ch) || ch == ' ')
    {
        return Ok(());
    }
    *a_text = format!("#{}", a_text.replace(' ', ""));
    Ok(())
}

/// Upstream `SetTextHexColor`: an HTML colour sets the members.
pub fn set_text_hex_color(t: &mut Tree, e: El, a_text: &mut String) -> R<()> {
    if !a_text.starts_with('#') {
        return Ok(());
    }
    let mut text: String = a_text.chars().skip(1).collect();
    // Extend a short form: FFF is FFFFFF.
    if text.chars().count() == 3 {
        text = text.chars().flat_map(|ch| [ch, ch]).collect();
    }
    let chars: Vec<char> = text.chars().collect();
    let part = |start: usize| -> String { chars.iter().skip(start).take(2).collect() };
    t.set_edit_values(e, "R", &part(0))?;
    t.set_edit_values(e, "G", &part(2))?;
    t.set_edit_values(e, "B", &part(4))?;
    t.set_edit_values(e, "A", &part(6))?;
    a_text.clear();
    Ok(())
}

/// Upstream `ByteFloatGetText`: a byte as a float in -1..1.
pub fn byte_float_get_text(_t: &mut Tree, _e: El, a_text: &mut String) -> R<()> {
    let value = str_to_int(a_text).ok_or_else(|| not_an_integer(a_text))?;
    *a_text = df_float_to_str(f64::from(value) / 127.5 - 1.0);
    Ok(())
}

/// Upstream `ByteFloatSetText`.
pub fn byte_float_set_text(_t: &mut Tree, _e: El, a_text: &mut String) -> R<()> {
    let value = df_str_to_float(a_text)?;
    *a_text = round((value + 1.0) * 127.5).to_string();
    Ok(())
}

/// Upstream `GetTextHexByte`.
pub fn get_text_hex_byte(_t: &mut Tree, _e: El, a_text: &mut String) -> R<()> {
    let value = str_to_int(a_text).ok_or_else(|| not_an_integer(a_text))?;
    *a_text = int_to_hex(i64::from(value), 2);
    Ok(())
}

/// Upstream `SetTextHexByte`.
pub fn set_text_hex_byte(_t: &mut Tree, _e: El, a_text: &mut String) -> R<()> {
    let hex = format!("${a_text}");
    let value = str_to_int(&hex).ok_or_else(|| not_an_integer(&hex))?;
    *a_text = value.to_string();
    Ok(())
}

/// Upstream `GetTextHexFloat`: a float in 0..1 as a hex byte.
pub fn get_text_hex_float(_t: &mut Tree, _e: El, a_text: &mut String) -> R<()> {
    let value = df_str_to_float(a_text)?;
    if (0.0..=1.0).contains(&value) {
        *a_text = int_to_hex(round(value * 255.0), 2);
    }
    Ok(())
}

/// Upstream `SetTextHexFloat`.
pub fn set_text_hex_float(_t: &mut Tree, _e: El, a_text: &mut String) -> R<()> {
    let chars: Vec<char> = a_text.chars().collect();
    if chars.len() == 2 && chars[0] != '.' && chars[1] != '.' {
        let hex = format!("${a_text}");
        let value = str_to_int(&hex).ok_or_else(|| not_an_integer(&hex))?;
        *a_text = float_to_str(f64::from(value) / 255.0);
    }
    Ok(())
}

fn rotation_text(values: &[f64]) -> String {
    values
        .iter()
        .map(|&value| df_float_to_str(value))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Upstream `QuaternionGetText`: the rotation as an angle and an axis, or
/// as Euler angles.
pub fn quaternion_get_text(t: &mut Tree, e: El, a_text: &mut String) -> R<()> {
    if t.edit_values(e, "W")? == "Min"
        && t.edit_values(e, "X")? == "Min"
        && t.edit_values(e, "Y")? == "Min"
        && t.edit_values(e, "Z")? == "Min"
    {
        *a_text = "Min".to_owned();
        return Ok(());
    }
    let q = Quaternion::new(
        t.native_values(e, "W")?.to_f64()?,
        t.native_values(e, "X")?.to_f64()?,
        t.native_values(e, "Y")?.to_f64()?,
        t.native_values(e, "Z")?.to_f64()?,
    );
    *a_text = if rotation_euler() {
        let (x, y, z) = quaternion_to_euler(&q);
        rotation_text(&[rad_to_deg(x), rad_to_deg(y), rad_to_deg(z)])
    } else {
        let (a, x, y, z) = quaternion_to_axis_angle(&q);
        rotation_text(&[rad_to_deg(a), x, y, z])
    };
    Ok(())
}

/// Upstream `QuaternionSetText`.
pub fn quaternion_set_text(t: &mut Tree, e: El, a_text: &mut String) -> R<()> {
    if a_text == "Min" {
        for member in ["W", "X", "Y", "Z"] {
            t.set_edit_values(e, member, "Min")?;
        }
        a_text.clear();
        return Ok(());
    }
    let parts = split_string(a_text, " ");
    let q = if rotation_euler() {
        if parts.len() < 3 {
            return Ok(());
        }
        let x = deg_to_rad(df_str_to_float(&parts[0])?);
        let y = deg_to_rad(df_str_to_float(&parts[1])?);
        let z = deg_to_rad(df_str_to_float(&parts[2])?);
        euler_to_quaternion(x, y, z)
    } else {
        if parts.len() < 4 {
            return Ok(());
        }
        let a = deg_to_rad(df_str_to_float(&parts[0])?);
        let x = df_str_to_float(&parts[1])?;
        let y = df_str_to_float(&parts[2])?;
        let z = df_str_to_float(&parts[3])?;
        axis_angle_to_quaternion(a, x, y, z)
    };
    t.set_native_values(e, "W", Variant::Float(q.w()))?;
    t.set_native_values(e, "X", Variant::Float(q.x()))?;
    t.set_native_values(e, "Y", Variant::Float(q.y()))?;
    t.set_native_values(e, "Z", Variant::Float(q.z()))?;
    a_text.clear();
    Ok(())
}

fn matrix_member(i: usize, j: usize) -> String {
    format!("m{}{}", i + 1, j + 1)
}

/// Upstream `RotMatrix33_GetText`.
pub fn rot_matrix33_get_text(t: &mut Tree, e: El, a_text: &mut String) -> R<()> {
    let mut m: Matrix33 = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[j][i] = t.native_values(e, &matrix_member(i, j))?.to_f64()?;
        }
    }
    *a_text = if rotation_euler() {
        let (x, y, z) = m33_to_euler(&m);
        rotation_text(&[rad_to_deg(x), rad_to_deg(y), rad_to_deg(z)])
    } else {
        let (a, x, y, z) = m33_to_axis_angle(&m);
        rotation_text(&[rad_to_deg(a), x, y, z])
    };
    Ok(())
}

/// Upstream `RotMatrix33_SetText`.
pub fn rot_matrix33_set_text(t: &mut Tree, e: El, a_text: &mut String) -> R<()> {
    let parts = split_string(a_text, " ");
    let m = if rotation_euler() {
        if parts.len() < 3 {
            return Ok(());
        }
        let x = deg_to_rad(df_str_to_float(&parts[0])?);
        let y = deg_to_rad(df_str_to_float(&parts[1])?);
        let z = deg_to_rad(df_str_to_float(&parts[2])?);
        euler_to_m33(x, y, z)
    } else {
        if parts.len() < 4 {
            return Ok(());
        }
        let a = deg_to_rad(df_str_to_float(&parts[0])?);
        let x = df_str_to_float(&parts[1])?;
        let y = df_str_to_float(&parts[2])?;
        let z = df_str_to_float(&parts[3])?;
        axis_angle_to_m33(a, x, y, z)
    };
    for i in 0..3 {
        for j in 0..3 {
            t.set_native_values(e, &matrix_member(i, j), Variant::Float(m[j][i]))?;
        }
    }
    a_text.clear();
    Ok(())
}

#[allow(dead_code)]
fn parse_int64(text: &str) -> R<i64> {
    str_to_int64(text).ok_or_else(|| not_an_integer(text))
}
