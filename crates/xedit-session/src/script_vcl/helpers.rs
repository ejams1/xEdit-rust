// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter_Controls.pas
// (the hint and cursor helpers), JvInterpreter_Forms.pas (IsAccel,
// KeysToShiftState, KeyDataToShiftState) and JvInterpreter_Menus.pas (the
// shortcut codec)

//! The pure helpers of the VCL units: the ones that answer without a window.
//!
//! Each behaviour is ported from the unit's handler and the Delphi function
//! it calls, with the values the 4.1.5q oracle produced for probes of it
//! (captured 2026-10-10 by running a probe script through the hidden GUI;
//! see the step's PLAN note). The tables of [`super::tables`] carry the
//! oracle's key-name and constant data these functions read.

use std::cell::RefCell;
use std::rc::Rc;

use xedit_script::adapter::{Adapter, Args, ExternalCallFn};
use xedit_script::values::{Value, ValueError, to_i64, to_str};

use super::Vcl;

/// The shape of one native helper.
type NativeFunction = fn(&mut Value, &mut Args) -> Result<(), ValueError>;

/// The native implementation of a unit function, when it has one: the pure
/// helpers of `Controls`, `Forms` and `Menus`. Everything else a script can
/// call (`InputQuery`, `MessageDlg`, `Screen`, `NewItem`, ...) is answered by
/// the [`Vcl`] seam.
pub(crate) fn native_function(unit: &str, name: &str) -> Option<NativeFunction> {
    Some(match (unit, name) {
        ("Controls", "CursorToString") => cursor_to_string,
        ("Controls", "StringToCursor") => string_to_cursor,
        ("Controls", "CursorToIdent") => cursor_to_ident,
        ("Controls", "IdentToCursor") => ident_to_cursor,
        ("Controls", "GetShortHint") => get_short_hint,
        ("Controls", "GetLongHint") => get_long_hint,
        ("Forms", "IsAccel") => is_accel,
        ("Forms", "KeysToShiftState") => keys_to_shift_state,
        ("Forms", "KeyDataToShiftState") => key_data_to_shift_state,
        ("Menus", "ShortCut") => short_cut,
        ("Menus", "ShortCutToKey") => short_cut_to_key,
        ("Menus", "ShortCutToText") => short_cut_to_text,
        ("Menus", "TextToShortCut") => text_to_short_cut,
        _ => return None,
    })
}

/// The text of `Args.Values[i]` the way the handlers coerce it (`string(...)`).
fn text_at(args: &Args, index: usize) -> Result<String, ValueError> {
    Ok(String::from_utf8_lossy(&to_str(&args.values[index])?).into_owned())
}

/// `Boolean(Args.Values[i])`.
fn int_at(args: &Args, index: usize) -> Result<i32, ValueError> {
    let value = to_i64(&args.values[index])?;
    Ok(value as i32)
}

//=== Vcl.Controls: the hint helpers ========================================

/// `GetShortHint` (`Controls.pas`): the text before the first `|`, the whole
/// string without one.
fn get_short_hint(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let hint = text_at(args, 0)?;
    let short = match hint.find('|') {
        Some(bar) => &hint[..bar],
        None => hint.as_str(),
    };
    *value = Value::Str(short.as_bytes().to_vec());
    Ok(())
}

/// `GetLongHint`: the text after the first `|`, the whole string without one.
fn get_long_hint(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let hint = text_at(args, 0)?;
    let long = match hint.find('|') {
        Some(bar) => &hint[bar + 1..],
        None => hint.as_str(),
    };
    *value = Value::Str(long.as_bytes().to_vec());
    Ok(())
}

//=== Vcl.Controls: the cursor table =========================================

/// The `cr*` idents of the `Controls` unit's cursor list, in the order the
/// oracle answers a lookup (captured 2026-10-10). `crNone` is *not* in the
/// table: `StringToCursor('crNone')` falls through to `StrToInt` and raises
/// (`'crNone' is not a valid integer value`), as the oracle does. `crSize`
/// and `crSizeAll` are the same cursor; the value-to-name direction answers
/// `crSizeAll` (the later declaration wins the reverse lookup).
const CURSORS: &[(&str, i32)] = &[
    ("crDefault", 0),
    ("crArrow", -2),
    ("crCross", -3),
    ("crIBeam", -4),
    ("crSizeNESW", -6),
    ("crSizeNS", -7),
    ("crSizeNWSE", -8),
    ("crSizeWE", -9),
    ("crUpArrow", -10),
    ("crHourGlass", -11),
    ("crDrag", -12),
    ("crNoDrop", -13),
    ("crHSplit", -14),
    ("crVSplit", -15),
    ("crMultiDrag", -16),
    ("crSQLWait", -17),
    ("crNo", -18),
    ("crAppStart", -19),
    ("crHelp", -20),
    ("crHandPoint", -21),
    ("crSize", -22),
    ("crSizeAll", -22),
];

/// `IdentToCursor`: the cursor of a `cr*` ident, case-insensitively (the
/// list's own comparison).
fn cursor_of_ident(ident: &str) -> Option<i32> {
    CURSORS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(ident))
        .map(|(_, cursor)| *cursor)
}

/// `CursorToIdent`'s name of a cursor: the last declaration of a value
/// answers (`crSizeAll` for -22), a cursor without one has no name.
fn ident_of_cursor(cursor: i32) -> Option<&'static str> {
    CURSORS
        .iter()
        .filter(|(_, value)| *value == cursor)
        .map(|(name, _)| *name)
        .next_back()
}

/// `CursorToString`: the ident, or the number as text when there is none
/// (`CursorToString(-23)` is `-23`).
fn cursor_to_string(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let cursor = int_at(args, 0)?;
    let text = ident_of_cursor(cursor).map_or_else(|| cursor.to_string(), |name| name.to_owned());
    *value = Value::Str(text.into_bytes());
    Ok(())
}

/// `StringToCursor`: the ident's cursor, else `StrToInt` of the text -- which
/// raises `EConvertError` for a text that is not a number.
fn string_to_cursor(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let ident = text_at(args, 0)?;
    let cursor = match cursor_of_ident(&ident) {
        Some(cursor) => cursor,
        None => str_to_int(&ident)?,
    };
    *value = Value::Integer(cursor);
    Ok(())
}

/// `StrToInt` (`System.SysUtils`): an optional sign, decimal digits, or `$`
/// and hexadecimal digits; anything else is the `EConvertError` the oracle
/// raises (`'<text>' is not a valid integer value`).
fn str_to_int(text: &str) -> Result<i32, ValueError> {
    let bad = || ValueError::external_class("EConvertError", format!("'{text}' is not a valid integer value"));
    let trimmed = text.trim_matches(|c: char| c == ' ' || c == '\t');
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let value = match digits.strip_prefix('$') {
        Some(hex) if !hex.is_empty() => i64::from_str_radix(hex, 16).map_err(|_| bad())?,
        Some(_) => return Err(bad()),
        None => digits.parse::<i64>().map_err(|_| bad())?,
    };
    let value = if negative { -value } else { value };
    i32::try_from(value).map_err(|_| bad())
}

/// `IdentToCursor`: the ident's cursor into the second argument, `True` when
/// it is in the list (the argument is left alone when it is not).
fn ident_to_cursor(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let ident = text_at(args, 0)?;
    match cursor_of_ident(&ident) {
        Some(cursor) => {
            if let Some(target) = args.values.get_mut(1) {
                *target = Value::Integer(cursor);
            }
            *value = Value::Bool(true);
        }
        None => *value = Value::Bool(false),
    }
    Ok(())
}

/// `CursorToIdent`: the cursor's ident into the second argument, `True` when
/// there is one.
///
/// UPSTREAM-QUIRK: the JVCL handler passes `string(TVarData(Args.Values[1]).vString)`
/// -- a cast of the variant's string *field* -- to the function's `var Ident:
/// string` parameter, so what the function writes never reaches the caller's
/// variable and the caller's string comes back empty (the oracle prints an
/// empty string for a cargo that has an ident). The port keeps the observable:
/// the boolean is real, the string argument becomes empty.
fn cursor_to_ident(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let cursor = int_at(args, 0)?;
    let known = ident_of_cursor(cursor).is_some();
    if let Some(target) = args.values.get_mut(1) {
        *target = Value::Str(Vec::new());
    }
    *value = Value::Bool(known);
    Ok(())
}

//=== Vcl.Forms: the shift-state helpers =====================================

/// `IsAccel` (`Forms.pas`): the key's character equals the character behind
/// the first `&` that is not part of an `&&`, case-insensitively.
fn is_accel(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let key = int_at(args, 0)?;
    let text = text_at(args, 1)?;
    let bytes = text.as_bytes();
    let wanted = (key & 0xFF) as u8;
    let mut found = false;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'&' {
            if bytes.get(at + 1) == Some(&b'&') {
                at += 1;
            } else if let Some(next) = bytes.get(at + 1) {
                found = next.eq_ignore_ascii_case(&wanted);
                break;
            }
        }
        at += 1;
    }
    *value = Value::Bool(found);
    Ok(())
}

/// The `TShiftState` set of a mouse key state (`KeysToShiftState`): the
/// Windows mask bits, `MK_SHIFT`/`MK_CONTROL` mapped to `ssShift`/`ssCtrl`.
fn keys_to_shift_state(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let keys = int_at(args, 0)?;
    let mut set = 0i32;
    if keys & 0x0004 != 0 {
        set |= SS_SHIFT;
    }
    if keys & 0x0008 != 0 {
        set |= SS_CTRL;
    }
    if keys & 0x0001 != 0 {
        set |= SS_LEFT;
    }
    if keys & 0x0002 != 0 {
        set |= SS_RIGHT;
    }
    if keys & 0x0010 != 0 {
        set |= SS_MIDDLE;
    }
    *value = Value::Set(set);
    Ok(())
}

/// `KeyDataToShiftState`: the real key state of Shift, Ctrl and Alt (the
/// argument is unused); headless and in the oracle's hidden GUI the keys are
/// up, so the answer is the empty set.
fn key_data_to_shift_state(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let _ = args;
    let mut set = 0i32;
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
        // SAFETY: `GetKeyState` reads the calling thread's key state and has
        // no preconditions.
        unsafe {
            if GetKeyState(i32::from(VK_SHIFT)) < 0 {
                set |= SS_SHIFT;
            }
            if GetKeyState(i32::from(VK_CONTROL)) < 0 {
                set |= SS_CTRL;
            }
            if GetKeyState(i32::from(VK_MENU)) < 0 {
                set |= SS_ALT;
            }
        }
    }
    *value = Value::Set(set);
    Ok(())
}

/// `TShiftState`'s set bits (delphi `System.Classes`): `ssShift`, `ssAlt`,
/// `ssCtrl` in that order.
const SS_SHIFT: i32 = 1;
const SS_ALT: i32 = 2;
const SS_CTRL: i32 = 4;
const SS_LEFT: i32 = 8;
const SS_RIGHT: i32 = 16;
const SS_MIDDLE: i32 = 32;

//=== Vcl.Menus: the shortcut codec ==========================================

/// `scShift`/`scCtrl`/`scAlt` of `Menus.pas`.
const SC_SHIFT: i32 = 0x2000;
const SC_CTRL: i32 = 0x4000;
const SC_ALT: i32 = 0x8000;

/// `ShortCut(Key, Shift)`: the modifiers of the `TShiftState` set as the
/// `sc*` bits, or-ed with the key.
///
/// UPSTREAM-QUIRK: a key above 255 gives 0 -- without the modifiers:
/// `ShortCut(300, 0)` and `ShortCut(300, [ssCtrl])` are both 0, while
/// `ShortCut(255, 0)` is 255 (oracle probes of 2026-10-10). The argument is
/// a `Word`, so the guard is the function's own, not a conversion's.
fn short_cut(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let key = int_at(args, 0)?;
    let shift = int_at(args, 1)?;
    if !(0..=255).contains(&key) {
        *value = Value::Integer(0);
        return Ok(());
    }
    let mut short_cut = 0i32;
    if shift & SS_SHIFT != 0 {
        short_cut |= SC_SHIFT;
    }
    if shift & SS_CTRL != 0 {
        short_cut |= SC_CTRL;
    }
    if shift & SS_ALT != 0 {
        short_cut |= SC_ALT;
    }
    *value = Value::Integer(short_cut | key);
    Ok(())
}

/// `ShortCutToKey(ShortCut, var Key, var Shift)`: the low byte into `Key`,
/// the `sc*` bits as the `TShiftState` set into `Shift`.
fn short_cut_to_key(_value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let short_cut = int_at(args, 0)?;
    if let Some(key) = args.values.get_mut(1) {
        *key = Value::Integer(short_cut & 0xFF);
    }
    let mut shift = 0i32;
    if short_cut & SC_SHIFT != 0 {
        shift |= SS_SHIFT;
    }
    if short_cut & SC_CTRL != 0 {
        shift |= SS_CTRL;
    }
    if short_cut & SC_ALT != 0 {
        shift |= SS_ALT;
    }
    if let Some(target) = args.values.get_mut(2) {
        *target = Value::Set(shift);
    }
    Ok(())
}

/// `ShortCutToText`: `Ctrl+`/`Shift+`/`Alt+` in front of the key's name, the
/// whole text empty when the key has no name (a shortcut of modifiers alone
/// is the empty string).
fn short_cut_to_text(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let short_cut = int_at(args, 0)?;
    *value = Value::Str(short_cut_to_text_of(short_cut).into_bytes());
    Ok(())
}

fn short_cut_to_text_of(short_cut: i32) -> String {
    let key = short_cut & 0xFF;
    let Some(name) = super::tables::KEY_NAMES
        .iter()
        .find(|(code, _)| *code == key)
        .map(|(_, name)| *name)
    else {
        return String::new();
    };
    // The words are prefixed in the order Shift, Alt, Ctrl, which puts them
    // in the text as `Ctrl+Alt+Shift+<key>` (the oracle: `57409` is
    // `Ctrl+Alt+Shift+A`, `[ssShift, ssAlt]` is `Alt+Shift+A`).
    let mut text = String::from(name);
    if short_cut & SC_SHIFT != 0 {
        text = format!("Shift+{text}");
    }
    if short_cut & SC_ALT != 0 {
        text = format!("Alt+{text}");
    }
    if short_cut & SC_CTRL != 0 {
        text = format!("Ctrl+{text}");
    }
    text
}

/// `TextToShortCut`: the modifier words (`Ctrl+`, `Shift+`, `Alt+`, any
/// order, repeated), then the key's name; a text the name table does not
/// hold is 0.
fn text_to_short_cut(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let text = text_at(args, 0)?;
    *value = Value::Integer(text_to_short_cut_of(&text));
    Ok(())
}

fn text_to_short_cut_of(text: &str) -> i32 {
    let mut rest = text;
    let mut short_cut = 0i32;
    loop {
        let lower = rest.to_ascii_lowercase();
        let (modifier, length) = if lower.starts_with("ctrl+") {
            (SC_CTRL, 5)
        } else if lower.starts_with("shift+") {
            (SC_SHIFT, 6)
        } else if lower.starts_with("alt+") {
            (SC_ALT, 4)
        } else {
            break;
        };
        short_cut |= modifier;
        rest = &rest[length..];
    }
    match super::tables::KEY_PARSE
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(rest))
    {
        Some((_, key)) => short_cut | key,
        None => 0,
    }
}

/// Registers the `external` seam of the run's adapter: upstream declares
/// `MessageBox` as an import of `user32` (`JvInterpreter_Windows.pas`), a
/// function that shows a modal window -- the port routes it through the same
/// seam as every other form, so the GUI phase answers it with the real API
/// call and headless it refuses with the clear error.
///
/// A function a script declares `external` itself (`AddExtFun` of
/// `InterpretFunction`, issue #25) is called in its library
/// ([`crate::script_extfun::call_dll`]), as upstream's `CallDll` does.
pub(crate) fn register_external(adapter: &mut Adapter, vcl: Rc<RefCell<dyn Vcl>>) {
    let call: ExternalCallFn = Rc::new(move |function, args| {
        if function.unit_name != "Windows" {
            return crate::script_extfun::call_dll(function, args);
        }
        let mut value = Value::Empty;
        vcl.borrow_mut()
            .function(&function.unit_name, &function.identifier, &mut value, args)?;
        Ok(value)
    });
    adapter.external_call = Some(call);
}
