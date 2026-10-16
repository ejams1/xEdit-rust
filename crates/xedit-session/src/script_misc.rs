// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterMisc.pas (the constants of
// System, Math, Types, SysUtils, ExtCtrls, StdCtrls, Forms, Menus, Controls
// and Windows; the StrUtils, Variants, SysUtils, System, Windows, ShellApi,
// FileCtrl, Clipbrd and Math functions; the TEncoding and TStrings
// registrations) with Delphi's System.StrUtils, System.VarUtils,
// System.SysUtils, System.Math and Winapi.Winapi semantics.

//! `xejviScriptAdapterMisc.pas`: the string, file, path, math and shell
//! helpers of the misc family, and the `TStrings`/`TEncoding` registrations
//! it makes.
//!
//! The `TStrings`/`TStringList` and `TEncoding` objects themselves are
//! `script_strings.rs`; this module registers the unit's entries over them.
//! The Windows-only functions (the clipboard, `ShellExecute*`,
//! `CreateProcessWait`, `GetKeyState`) call the system APIs of a Windows
//! build and degrade to a documented stub elsewhere; the platform is
//! Windows first (decision 4 of `docs/PLAN.md`).
//!
//! Divergences recorded for the parity harness:
//! - `SelectDirectory` shows the VCL directory dialog, which is the step-9
//!   form shim; headless it raises the same single-argument error the form
//!   classes of step 9 will raise.
//! - `GetKeyState`, `ShellExecute` and the clipboard read the desktop's real
//!   state; a hidden parity run compares what the oracle's hidden window
//!   sees.
//! - `GetClipboardText`/`SetClipboardText` never open a dialog; the oracle's
//!   work on the same desktop clipboard.

use std::rc::Rc;
use std::sync::atomic::{AtomicI32, Ordering};

use xedit_core::delphi::{int_power, round, same_value, str_to_float};
use xedit_core::interface::globals;
use xedit_io::collate::ansi_compare_text;
use xedit_io::encoding::lower_case;

use xedit_script::adapter::{Adapter, Args};
use xedit_script::ast::BinOp;
use xedit_script::values::{ParamType, Value, ValueError, VarType, VariantArray};

use crate::script_classes::ScriptClasses;
use crate::script_strings::{
    EncodingKind, borrow_list, borrow_list_mut, encoding_of, list_of_or_error, receiver, text_arg, text_arg_lossy,
};

/// The class name of a plain Delphi `Exception`.
pub const EXCEPTION_CLASS: &str = "Exception";

//=== Constants ===============================================================

/// `TVarType` (`System.pas`): the `var*` codes of a variant.
const VAR_TYPES: &[(&str, i32)] = &[
    ("varEmpty", 0x0000),
    ("varNull", 0x0001),
    ("varSmallInt", 0x0002),
    ("varInteger", 0x0003),
    ("varSingle", 0x0004),
    ("varDouble", 0x0005),
    ("varCurrency", 0x0006),
    ("varDate", 0x0007),
    ("varOleStr", 0x0008),
    ("varDispatch", 0x0009),
    ("varError", 0x000A),
    ("varBoolean", 0x000B),
    ("varVariant", 0x000C),
    ("varUnknown", 0x000D),
    ("varShortInt", 0x0010),
    ("varByte", 0x0011),
    ("varWord", 0x0012),
    ("varLongWord", 0x0013),
    ("varInt64", 0x0014),
    ("varUInt64", 0x0015),
    ("varStrArg", 0x0048),
    ("varString", 0x0100),
    ("varAny", 0x0101),
    ("varUString", 0x0102),
    ("varTypeMask", 0x0FFF),
    ("varByRef", 0x4000),
];

/// `System.Math`'s `TValueSign` trio, `TFPUPrecisionMode` and
/// `TRoundingMode` (`System.Math.pas:441`, `:650`): `NegativeValue = -1`,
/// `ZeroValue = 0`, `PositiveValue = 1`; `pmSingle = 0`, `pmReserved = 1`,
/// `pmDouble = 2`, `pmExtended = 3`; `rmNearest = 0`, `rmDown = 1`,
/// `rmUp = 2`, `rmTruncate = 3`.
const MATH_CONSTS: &[(&str, i32)] = &[
    ("NegativeValue", -1),
    ("ZeroValue", 0),
    ("PositiveValue", 1),
    ("pmSingle", 0),
    ("pmReserved", 1),
    ("pmDouble", 2),
    ("pmExtended", 3),
    ("rmNearest", 0),
    ("rmDown", 1),
    ("rmUp", 2),
    ("rmTruncate", 3),
];

/// `System.Types`' `TValueRelationship`: `LessThanValue = -1`,
/// `EqualsValue = 0`, `GreaterThanValue = 1`.
const TYPES_CONSTS: &[(&str, i32)] = &[("LessThanValue", -1), ("EqualsValue", 0), ("GreaterThanValue", 1)];

/// `System.SysUtils`' `TReplaceFlag` (`rfReplaceAll = 0`,
/// `rfIgnoreCase = 1`), `TFileMode`'s `fmCreate = $FF00` and the integer
/// bounds.
const SYSUTILS_CONSTS: &[(&str, i32)] = &[
    ("rfReplaceAll", 0),
    ("rfIgnoreCase", 1),
    ("fmCreate", 0xFF00),
    ("LowInteger", i32::MIN),
    ("HighInteger", i32::MAX),
];

/// `Vcl.ExtCtrls`' `TLabelPosition = (lpAbove, lpBelow, lpLeft, lpRight)`.
const EXTCTRLS_CONSTS: &[(&str, i32)] = &[("lpAbove", 0), ("lpBelow", 1), ("lpLeft", 2), ("lpRight", 3)];

/// `Vcl.StdCtrls`' `TCheckBoxState = (cbUnchecked, cbChecked, cbGrayed)`.
const STDCTRLS_CONSTS: &[(&str, i32)] = &[("cbUnchecked", 0), ("cbChecked", 1), ("cbGrayed", 2)];

/// `Vcl.Forms`' `TPosition`'s `poMainFormCenter` (6) and
/// `TPopupMode = (pmNone, pmAuto, pmExplicit)`.
const FORMS_CONSTS: &[(&str, i32)] = &[("poMainFormCenter", 6), ("pmNone", 0), ("pmAuto", 1), ("pmExplicit", 2)];

/// `Vcl.Menus`' `TMenuAnimation = (maAutomatic, maManual, maNone)`.
const MENUS_CONSTS: &[(&str, i32)] = &[("maAutomatic", 0), ("maManual", 1)];

/// `Vcl.Controls`' `TAnchorKind = (akLeft, akTop, akRight, akBottom)`.
const CONTROLS_CONSTS: &[(&str, i32)] = &[("akLeft", 0), ("akTop", 1), ("akRight", 2), ("akBottom", 3)];

/// `Winapi.Windows`' `SW_*` show-window codes.
const WINDOWS_CONSTS: &[(&str, i32)] = &[
    ("SW_HIDE", 0),
    ("SW_SHOWNORMAL", 1),
    ("SW_SHOWMINIMIZED", 2),
    ("SW_SHOWMAXIMIZED", 3),
    ("SW_SHOWNOACTIVATE", 4),
    ("SW_SHOW", 5),
    ("SW_MINIMIZE", 6),
    ("SW_SHOWMINNOACTIVE", 7),
    ("SW_SHOWNA", 8),
    ("SW_RESTORE", 9),
    ("SW_SHOWDEFAULT", 10),
];

//=== The variant type of a script value ======================================

/// The custom variant types of `JvInterpreter.pas` (`varRecord` and the
/// five others): the RTL assigns their codes at run time from `$1000` up, so
/// no registered constant names them; the port fixes an order for the ones
/// its own `var*` mapping needs.
// The oracle's codes of the custom variant types (see
// `script_rtl::DELPHI_VAR_*`).
const VAR_RECORD: i32 = crate::script_rtl::DELPHI_VAR_RECORD;
const VAR_OBJECT: i32 = crate::script_rtl::DELPHI_VAR_OBJECT;
const VAR_CLASS: i32 = crate::script_rtl::DELPHI_VAR_CLASS;
const VAR_POINTER: i32 = crate::script_rtl::DELPHI_VAR_POINTER;
const VAR_SET: i32 = crate::script_rtl::DELPHI_VAR_SET;
const VAR_ARRAY: i32 = crate::script_rtl::DELPHI_VAR_ARRAY;

/// `VarType(V)`: the Delphi variant-type code of a script value.
/// `Iterface`-carrying values (the elements of a run) are `varUnknown`, as
/// upstream's `O2V` gives them; the port's other objects are the custom
/// `varObject` of `JvInterpreter.pas`. A script string is a
/// `UnicodeString`, so `varUString` (`$102`) as the Delphi 2009+ RTL gives
/// it.
pub fn delphi_var_type(value: &Value) -> i32 {
    match value {
        Value::Empty => 0x0000,
        Value::Null => 0x0001,
        Value::Smallint(_) => 0x0002,
        Value::Integer(_) => 0x0003,
        Value::Byte(_) => 0x0011,
        Value::Word(_) => 0x0012,
        Value::Int64(_) => 0x0014,
        Value::Double(_) => 0x0005,
        Value::Date(_) => 0x0007,
        Value::Str(_) => 0x0102,
        Value::Bool(_) => 0x000B,
        Value::Set(_) => VAR_SET,
        Value::Pointer(_) => VAR_POINTER,
        Value::Object(_) => {
            if crate::script::ScriptRunState::element_of(value).is_some() {
                0x000D
            } else {
                VAR_OBJECT
            }
        }
        Value::Class(_) => VAR_CLASS,
        Value::Record(_) => VAR_RECORD,
        Value::Array(_) => VAR_ARRAY,
        Value::VariantArray(array) => 0x2000 | array.borrow().element_code,
    }
}

/// `System.VarUtils.VarTypeAsText`.
///
/// OWED: the RTL's name table for the codes outside `varEmpty`..`varUnknown`
/// and `varShortInt`..`varUInt64` is not reproduced from source; `varString`
/// and `varUString` are given their Delphi type names and every other code
/// `Unknown`. No script of the corpus calls it.
pub fn var_type_as_text(ty: i32) -> &'static str {
    match ty & 0x0FFF {
        0x0000 => "Empty",
        0x0001 => "Null",
        0x0002 => "Smallint",
        0x0003 => "Integer",
        0x0004 => "Single",
        0x0005 => "Double",
        0x0006 => "Currency",
        0x0007 => "Date",
        0x0008 => "OleStr",
        0x0009 => "Dispatch",
        0x000A => "Error",
        0x000B => "Boolean",
        0x000C => "Variant",
        0x000D => "Unknown",
        0x0010 => "ShortInt",
        0x0011 => "Byte",
        0x0012 => "Word",
        0x0013 => "LongWord",
        0x0014 => "Int64",
        0x0015 => "UInt64",
        0x0100 => "String",
        0x0101 => "Any",
        0x0102 => "UnicodeString",
        VAR_RECORD => "Record",
        VAR_OBJECT => "Object",
        VAR_CLASS => "Class",
        VAR_POINTER => "Pointer",
        VAR_SET => "Set",
        VAR_ARRAY => "Array",
        _ => "Unknown",
    }
}

//=== The FPU state the Math unit reports =====================================

/// `GetPrecisionMode`'s state: the port does not touch the FPU and reports
/// the x64 SSE default, `pmDouble` (2).
static PRECISION_MODE: AtomicI32 = AtomicI32::new(2);
/// `GetRoundMode`'s state: `rmNearest` (0).
static ROUND_MODE: AtomicI32 = AtomicI32::new(0);

//=== Functions ===============================================================

/// `System.StrUtils.ContainsStr`: `Pos(ASubStr, AStr) > 0`, case
/// significant.
fn contains_str(text: &str, sub: &str) -> bool {
    text.contains(sub)
}

/// `System.StrUtils.ContainsText`: the case-insensitive `ContainsStr`
/// (Delphi compares through the locale's lower case).
fn contains_text(text: &str, sub: &str) -> bool {
    lower_case(text).contains(&lower_case(sub))
}

/// `System.StrUtils.StartsStr`/`EndsStr` and their `Text` forms. The
/// argument order is upstream's, where the substring comes *first*
/// (`StartsStr(const ASubText, AText)`, unlike `ContainsStr(const AText,
/// ASubText)`).
fn starts_str(sub: &str, text: &str) -> bool {
    text.starts_with(sub)
}

fn ends_str(sub: &str, text: &str) -> bool {
    text.ends_with(sub)
}

/// `System.StrUtils.DupeString`: the text `count` times; a count of zero or
/// less gives `''`.
fn dupe_string(text: &str, count: i32) -> String {
    if count <= 0 {
        return String::new();
    }
    text.repeat(count as usize)
}

/// `System.StrUtils.LeftStr`/`RightStr`/`MidStr` over Delphi's `Copy`
/// clamping.
fn left_str(text: &str, count: i32) -> String {
    if count <= 0 {
        return String::new();
    }
    text.chars().take(count as usize).collect()
}

fn right_str(text: &str, count: i32) -> String {
    if count <= 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let start = chars.len().saturating_sub(count as usize);
    chars[start..].iter().collect()
}

fn mid_str(text: &str, start: i32, count: i32) -> String {
    if count <= 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let start = (start.max(1) as usize - 1).min(chars.len());
    let end = chars.len().min(start.saturating_add(count as usize));
    chars[start..end].iter().collect()
}

/// `System.StrUtils.ReverseString`.
fn reverse_string(text: &str) -> String {
    text.chars().rev().collect()
}

/// `System.StrUtils.SplitString(S, Delimiters, Limit = 0)`: the string
/// split on any character of `Delimiters`; empty pieces are dropped, and a
/// `Limit` of zero or one gives the whole string as one piece.
fn split_string(text: &str, delimiters: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let delimiters: Vec<char> = delimiters.chars().collect();
    let mut result: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if delimiters.contains(&ch) {
            if !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

/// `System.StrUtils.StuffString(S, Start, Count, Insert)`: `Copy(S, 1,
/// Start-1) + Insert + Copy(S, Start+Count, MaxInt)`.
fn stuff_string(text: &str, start: u32, count: u32, insert: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start = (start.max(1) as usize - 1).min(chars.len());
    let end = chars.len().min(start.saturating_add(count as usize));
    let mut out: String = chars[..start].iter().collect();
    out.push_str(insert);
    out.extend(&chars[end..]);
    out
}

/// `System.StrUtils.IfThen` as `xejviScriptAdapterMisc.pas` calls it: for
/// two arguments of the same variant type, the typed `IfThen`; for mixed
/// types the variant itself. Two equal types outside the list leave the
/// result unassigned (the `case` has no `else`).
fn if_then(condition: bool, yes: &Value, no: &Value) -> Value {
    let yes_type = delphi_var_type(yes);
    let no_type = delphi_var_type(no);
    if yes_type == no_type {
        match yes_type {
            0x0100 | 0x0102 => {
                return Value::Str(
                    if condition {
                        text_arg_lossy(yes)
                    } else {
                        text_arg_lossy(no)
                    }
                    .into_bytes(),
                );
            }
            0x0003 | 0x0002 | 0x0011 | 0x0014 => {
                let value = if condition { yes } else { no };
                return Value::Integer(xedit_script::values::to_i32(value).unwrap_or(0));
            }
            0x0004 | 0x0005 => {
                let value = if condition { yes } else { no };
                return Value::Double(xedit_script::values::to_f64(value).unwrap_or(0.0));
            }
            _ => return Value::Empty,
        }
    }
    if condition { yes.clone() } else { no.clone() }
}

/// `System.StrUtils.IndexStr(AStr, AValues)`: the index of the first exact
/// match; -1 when there is none. The array and `TStringList` overloads
/// differ only in what they search.
fn index_str(text: &str, values: &[String]) -> i32 {
    values
        .iter()
        .position(|value| value == text)
        .map_or(-1, |index| index as i32)
}

/// `System.StrUtils.IndexText(AStr, AValues)`: the same, case-insensitive
/// through the locale comparison (`SameText`).
fn index_text(text: &str, values: &[String]) -> i32 {
    values
        .iter()
        .position(|value| ansi_compare_text(value, text) == std::cmp::Ordering::Equal)
        .map_or(-1, |index| index as i32)
}

/// The array or `TStringList` of the second argument of the `Index*` and
/// `Match*` helpers. `Err` is the empty array for an argument that is
/// neither (both upstream branches fail and the result stays -1/False).
fn values_argument(value: &Value) -> Vec<String> {
    if let Value::Array(array) = value {
        return array.borrow().values.iter().map(text_arg_lossy).collect();
    }
    if let Value::VariantArray(array) = value {
        return array.borrow().values.iter().map(text_arg_lossy).collect();
    }
    if let Some(list) = crate::script_strings::list_of(value) {
        let list = borrow_list(&list);
        return (0..list.count()).map(|index| list.string_at(index as i64)).collect();
    }
    Vec::new()
}

/// `IntToHex64`: at least `digits` hex digits, upper case, the two's
/// complement of a negative value.
fn int_to_hex64(value: i64, digits: i32) -> String {
    let text = format!("{:X}", value as u64);
    let digits = digits.max(0) as usize;
    if text.len() >= digits {
        text
    } else {
        format!("{}{text}", "0".repeat(digits - text.len()))
    }
}

/// `StrToInt64`: the `Val`-shaped conversion with the RTL's `EConvertError`.
fn str_to_int64(text: &str) -> Result<i64, ValueError> {
    let trimmed = text.trim();
    let (negative, rest) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let value = if let Some(hex) = rest.strip_prefix('$').or_else(|| rest.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else {
        rest.parse::<i64>()
            .ok()
            .map(|value| if negative { -value } else { value })
    };
    value.ok_or_else(|| ValueError::external_class("EConvertError", format!("'{}' is not a valid integer value", text)))
}

/// `StrToInt64Def(Text, Default)`: the conversion's failure gives the
/// default.
fn str_to_int64_def(text: &str, default: i64) -> i64 {
    str_to_int64(text).unwrap_or(default)
}

/// `StrToFloatDef(Text, Default)`: the locale's decimal point is `.` in
/// the port (`xedit_core::delphi::str_to_float`).
fn str_to_float_def(text: &str, default: f64) -> f64 {
    str_to_float(text).unwrap_or(default)
}

/// `System.SysUtils.IncludeTrailingBackslash`/`ExcludeTrailingBackslash`:
/// a single `\` is appended unless the path ends in one of the two path
/// delimiters, and both are removed again.
fn include_trailing_backslash(path: &str) -> String {
    if path.ends_with(['\\', '/']) {
        path.to_owned()
    } else {
        format!("{path}\\")
    }
}

fn exclude_trailing_backslash(path: &str) -> String {
    match path.strip_suffix(['\\', '/']) {
        Some(stripped) if !stripped.is_empty() => stripped.to_owned(),
        _ => path.to_owned(),
    }
}

/// `System.SysUtils.ForceDirectories`: `True` when the directory exists
/// afterwards.
fn force_directories(path: &str) -> bool {
    std::fs::create_dir_all(path).is_ok() || std::path::Path::new(path).is_dir()
}

/// `System.SysUtils.StringReplace` with `TReplaceFlags`: without
/// `rfReplaceAll` only the first occurrence is replaced; `rfIgnoreCase`
/// compares case-insensitively (the locale's lower case).
fn string_replace(text: &str, find: &str, replace: &str, flags: i32) -> String {
    let replace_all = flags & 1 != 0;
    let ignore_case = flags & 2 != 0;
    if find.is_empty() {
        return text.to_owned();
    }
    if !ignore_case {
        if replace_all {
            return text.replace(find, replace);
        }
        return match text.find(find) {
            Some(at) => {
                let mut out = String::with_capacity(text.len());
                out.push_str(&text[..at]);
                out.push_str(replace);
                out.push_str(&text[at + find.len()..]);
                out
            }
            None => text.to_owned(),
        };
    }
    let lowered_text = lower_case(text);
    let lowered_find = lower_case(find);
    let mut out = String::new();
    let mut rest = 0usize;
    while let Some(at) = lowered_text[rest..].find(&lowered_find) {
        let start = rest + at;
        out.push_str(&text[rest..start]);
        out.push_str(replace);
        rest = start + lowered_find.len();
        if !replace_all {
            break;
        }
    }
    out.push_str(&text[rest.min(text.len())..]);
    out
}

/// `System.SysUtils.SameText`: the case-insensitive locale comparison.
fn same_text(left: &str, right: &str) -> bool {
    ansi_compare_text(left, right) == std::cmp::Ordering::Equal
}

//=== The registry ============================================================

fn value_param() -> ParamType {
    ParamType::value(VarType::Empty)
}

fn function(
    adapter: &mut Adapter,
    unit_name: &str,
    identifier: &str,
    param_count: i32,
    func: xedit_script::adapter::AdapterGetFn,
) {
    adapter.add_function(
        unit_name,
        identifier,
        func,
        param_count,
        (0..param_count).map(|_| value_param()).collect(),
        VarType::Empty,
    );
}

/// Registers `xejviScriptAdapterMisc.pas`.
pub fn register(adapter: &mut Adapter, classes: &ScriptClasses) {
    //=== The constants =======================================================

    for (name, value) in VAR_TYPES
        .iter()
        .chain(MATH_CONSTS)
        .chain(TYPES_CONSTS)
        .chain(SYSUTILS_CONSTS)
        .chain(EXTCTRLS_CONSTS)
        .chain(STDCTRLS_CONSTS)
        .chain(FORMS_CONSTS)
        .chain(MENUS_CONSTS)
        .chain(CONTROLS_CONSTS)
        .chain(WINDOWS_CONSTS)
    {
        let unit_name = const_unit(name);
        adapter.add_const(unit_name, name, Value::Integer(*value));
    }
    adapter.add_const("System", "MaxInt", Value::Integer(i32::MAX));
    adapter.add_const("System", "MinInt", Value::Integer(i32::MIN));

    //=== StrUtils ============================================================

    function(
        adapter,
        "StrUtils",
        "ContainsStr",
        2,
        Rc::new(|value, args| {
            *value = Value::Bool(contains_str(&text_arg(&args.values[0])?, &text_arg(&args.values[1])?));
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "ContainsText",
        2,
        Rc::new(|value, args| {
            *value = Value::Bool(contains_text(&text_arg(&args.values[0])?, &text_arg(&args.values[1])?));
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "DupeString",
        2,
        Rc::new(|value, args| {
            *value = Value::Str(
                dupe_string(
                    &text_arg(&args.values[0])?,
                    xedit_script::values::to_i32(&args.values[1])?,
                )
                .into_bytes(),
            );
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "EndsStr",
        2,
        Rc::new(|value, args| {
            *value = Value::Bool(ends_str(&text_arg(&args.values[0])?, &text_arg(&args.values[1])?));
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "EndsText",
        2,
        Rc::new(|value, args| {
            *value = Value::Bool(ends_str(
                &lower_case(&text_arg(&args.values[0])?),
                &lower_case(&text_arg(&args.values[1])?),
            ));
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "IfThen",
        3,
        Rc::new(|value, args| {
            let condition = xedit_script::values::to_bool(&args.values[0])?;
            *value = if_then(condition, &args.values[1], &args.values[2]);
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "IndexStr",
        2,
        Rc::new(|value, args| {
            let values = values_argument(&args.values[1]);
            *value = Value::Integer(index_str(&text_arg(&args.values[0])?, &values));
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "IndexText",
        2,
        Rc::new(|value, args| {
            let values = values_argument(&args.values[1]);
            *value = Value::Integer(index_text(&text_arg(&args.values[0])?, &values));
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "LeftStr",
        2,
        Rc::new(|value, args| {
            *value = Value::Str(
                left_str(
                    &text_arg(&args.values[0])?,
                    xedit_script::values::to_i32(&args.values[1])?,
                )
                .into_bytes(),
            );
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "MatchStr",
        2,
        Rc::new(|value, args| {
            let values = values_argument(&args.values[1]);
            *value = Value::Bool(index_str(&text_arg(&args.values[0])?, &values) != -1);
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "MatchText",
        2,
        Rc::new(|value, args| {
            let values = values_argument(&args.values[1]);
            *value = Value::Bool(index_text(&text_arg(&args.values[0])?, &values) != -1);
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "MidStr",
        3,
        Rc::new(|value, args| {
            *value = Value::Str(
                mid_str(
                    &text_arg(&args.values[0])?,
                    xedit_script::values::to_i32(&args.values[1])?,
                    xedit_script::values::to_i32(&args.values[2])?,
                )
                .into_bytes(),
            );
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "ReverseString",
        1,
        Rc::new(|value, args| {
            *value = Value::Str(reverse_string(&text_arg(&args.values[0])?).into_bytes());
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "RightStr",
        2,
        Rc::new(|value, args| {
            *value = Value::Str(
                right_str(
                    &text_arg(&args.values[0])?,
                    xedit_script::values::to_i32(&args.values[1])?,
                )
                .into_bytes(),
            );
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "StartsStr",
        2,
        Rc::new(|value, args| {
            *value = Value::Bool(starts_str(&text_arg(&args.values[0])?, &text_arg(&args.values[1])?));
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "StartsText",
        2,
        Rc::new(|value, args| {
            *value = Value::Bool(starts_str(
                &lower_case(&text_arg(&args.values[0])?),
                &lower_case(&text_arg(&args.values[1])?),
            ));
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "SplitString",
        2,
        Rc::new(|value, args| {
            let pieces = split_string(&text_arg(&args.values[0])?, &text_arg(&args.values[1])?);
            *value = Value::VariantArray(Rc::new(std::cell::RefCell::new(VariantArray::new(
                pieces.into_iter().map(|piece| Value::Str(piece.into_bytes())).collect(),
            ))));
            Ok(())
        }),
    );
    function(
        adapter,
        "StrUtils",
        "StuffString",
        4,
        Rc::new(|value, args| {
            *value = Value::Str(
                stuff_string(
                    &text_arg(&args.values[0])?,
                    xedit_script::values::to_i32(&args.values[1])?.max(0) as u32,
                    xedit_script::values::to_i32(&args.values[2])?.max(0) as u32,
                    &text_arg(&args.values[3])?,
                )
                .into_bytes(),
            );
            Ok(())
        }),
    );

    //=== Variants ============================================================

    function(
        adapter,
        "Variants",
        "VarType",
        1,
        Rc::new(|value, args| {
            *value = Value::Word(delphi_var_type(&args.values[0]) as u16);
            Ok(())
        }),
    );
    function(
        adapter,
        "Variants",
        "VarTypeAsText",
        1,
        Rc::new(|value, args| {
            let ty = xedit_script::values::to_i32(&args.values[0])?;
            *value = Value::Str(var_type_as_text(ty).as_bytes().to_vec());
            Ok(())
        }),
    );

    //=== SysUtils ============================================================

    // `Inc(Args.Values[0])`/`Dec`: the by-reference argument's value moves
    // by one (Delphi's variant `Inc`; `xejviScriptAdapterMisc.pas:854`
    // registers them with `varByRef`, so the change lands in the caller's
    // variable and not in a copy of it).
    adapter.add_function(
        "SysUtils",
        "Inc",
        Rc::new(|_value, args| {
            // `Args.Values[0] := Args.Values[0] + N`: the variant `+`, so a
            // string increment reads as a double (`Inc(i, 'x')` is `Could
            // not convert ... into type (Double)`, probe `x-028`).
            let step = args.values.get(1).cloned().unwrap_or(Value::Integer(1));
            args.values[0] = xedit_script::values::binary_op(BinOp::Plus, &args.values[0], &step)?;
            Ok(())
        }),
        1,
        vec![ParamType::by_ref(VarType::Empty)],
        VarType::Empty,
    );
    adapter.add_function(
        "SysUtils",
        "Dec",
        Rc::new(|_value, args| {
            let step = args.values.get(1).cloned().unwrap_or(Value::Integer(1));
            args.values[0] = xedit_script::values::binary_op(BinOp::Minus, &args.values[0], &step)?;
            Ok(())
        }),
        1,
        vec![ParamType::by_ref(VarType::Empty)],
        VarType::Empty,
    );
    function(
        adapter,
        "SysUtils",
        "Succ",
        1,
        Rc::new(|value, args| {
            *value = Value::Integer(xedit_script::values::to_i32(&args.values[0])?.wrapping_add(1));
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "Pred",
        1,
        Rc::new(|value, args| {
            *value = Value::Integer(xedit_script::values::to_i32(&args.values[0])?.wrapping_sub(1));
            Ok(())
        }),
    );
    // `Single(Frac(...))`: the result is a single upstream (the port keeps
    // it a double of the single's value, documented in `values.rs`).
    function(
        adapter,
        "SysUtils",
        "Frac",
        1,
        Rc::new(|value, args| {
            let number = xedit_script::values::to_f64(&args.values[0])?;
            // UPSTREAM-QUIRK (parity #39, probes/numerics.pas): the oracle's
            // `Frac` of NaN, INF and -INF prints `0.0000000000000000E+0`,
            // where `Int` of the same values prints NAN and INF.
            *value = Value::Double(if number.is_finite() {
                f64::from((number.fract()) as f32)
            } else {
                0.0
            });
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "Int",
        1,
        Rc::new(|value, args| {
            let number = xedit_script::values::to_f64(&args.values[0])?;
            *value = Value::Double(f64::from((number.trunc()) as f32));
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "SameText",
        2,
        Rc::new(|value, args| {
            *value = Value::Bool(same_text(&text_arg(&args.values[0])?, &text_arg(&args.values[1])?));
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "SameValue",
        2,
        Rc::new(|value, args| {
            let left = xedit_script::values::to_f64(&args.values[0])?;
            let right = xedit_script::values::to_f64(&args.values[1])?;
            *value = Value::Bool(same_value(left, right));
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "StringReplace",
        4,
        Rc::new(|value, args| {
            // `f := V2S(Args.Values[3])` (`xejviScriptAdapterMisc.pas:593`):
            // a set literal in an argument position arrives as an array of
            // ordinals (`1 shl` each, `V2S`'s variant-array branch), and a
            // set value or an integer passes through.
            let flags = xedit_script::values::v2s(&args.values[3])?;
            *value = Value::Str(
                string_replace(
                    &text_arg(&args.values[0])?,
                    &text_arg(&args.values[1])?,
                    &text_arg(&args.values[2])?,
                    flags,
                )
                .into_bytes(),
            );
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "IntToHex64",
        2,
        Rc::new(|value, args| {
            *value = Value::Str(
                int_to_hex64(
                    xedit_script::values::to_i64(&args.values[0])?,
                    xedit_script::values::to_i32(&args.values[1])?,
                )
                .into_bytes(),
            );
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "StrToInt64",
        1,
        Rc::new(|value, args| {
            *value = Value::Int64(str_to_int64(&text_arg(&args.values[0])?)?);
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "StrToInt64Def",
        2,
        Rc::new(|value, args| {
            *value = Value::Int64(str_to_int64_def(
                &text_arg(&args.values[0])?,
                xedit_script::values::to_i64(&args.values[1])?,
            ));
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "StrToFloatDef",
        2,
        Rc::new(|value, args| {
            *value = Value::Double(str_to_float_def(
                &text_arg(&args.values[0])?,
                xedit_script::values::to_f64(&args.values[1])?,
            ));
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "DirectoryExists",
        1,
        Rc::new(|value, args| {
            *value = Value::Bool(std::path::Path::new(&text_arg(&args.values[0])?).is_dir());
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "FileExists",
        1,
        Rc::new(|value, args| {
            *value = Value::Bool(std::path::Path::new(&text_arg(&args.values[0])?).is_file());
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "ForceDirectories",
        1,
        Rc::new(|value, args| {
            *value = Value::Bool(force_directories(&text_arg(&args.values[0])?));
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "IncludeTrailingBackslash",
        1,
        Rc::new(|value, args| {
            *value = Value::Str(include_trailing_backslash(&text_arg(&args.values[0])?).into_bytes());
            Ok(())
        }),
    );
    function(
        adapter,
        "SysUtils",
        "ExcludeTrailingBackslash",
        1,
        Rc::new(|value, args| {
            *value = Value::Str(exclude_trailing_backslash(&text_arg(&args.values[0])?).into_bytes());
            Ok(())
        }),
    );

    //=== System ==============================================================

    function(
        adapter,
        "System",
        "StringOfChar",
        2,
        Rc::new(|value, args| {
            let text = text_arg(&args.values[0])?;
            if text.is_empty() {
                // The `if Length(...) > 0` leaves the result unassigned.
                *value = Value::Empty;
                return Ok(());
            }
            let count = xedit_script::values::to_i32(&args.values[1])?.max(0) as usize;
            let ch = text.chars().next().unwrap_or(' ');
            *value = Value::Str(ch.to_string().repeat(count).into_bytes());
            Ok(())
        }),
    );

    //=== Windows, ShellApi, FileCtrl, Clipbrd ================================

    function(
        adapter,
        "Windows",
        "CopyFile",
        3,
        Rc::new(|value, args| {
            *value = Value::Bool(copy_file(
                &text_arg(&args.values[0])?,
                &text_arg(&args.values[1])?,
                xedit_script::values::to_bool(&args.values[2])?,
            ));
            Ok(())
        }),
    );
    function(
        adapter,
        "Windows",
        "Sleep",
        1,
        Rc::new(|_value, args| {
            let milliseconds = xedit_script::values::to_i64(&args.values[0])?.max(0) as u64;
            std::thread::sleep(std::time::Duration::from_millis(milliseconds));
            Ok(())
        }),
    );
    function(
        adapter,
        "Windows",
        "GetKeyState",
        1,
        Rc::new(|value, args| {
            let key = xedit_script::values::to_i32(&args.values[0])?;
            *value = Value::Integer(i32::from(get_key_state(key)));
            Ok(())
        }),
    );
    function(
        adapter,
        "Windows",
        "CreateProcessWait",
        4,
        Rc::new(|value, args| {
            let file = text_arg(&args.values[0])?;
            let parameters = text_arg(&args.values[1])?;
            // `wShowWindow := Args.Values[2]` and `Cardinal(Args.Values[3])`:
            // the Word and Cardinal conversions of the variant, which
            // overflow where the value does not fit the slot.
            let show = xedit_script::values::to_u16(&args.values[2])?;
            let timeout = xedit_script::values::to_u32(&args.values[3])?;
            *value = Value::Integer(create_process_wait(&file, &parameters, show, timeout)?);
            Ok(())
        }),
    );
    function(
        adapter,
        "ShellApi",
        "ShellExecute",
        6,
        Rc::new(|value, args| {
            let verb = text_arg(&args.values[1])?;
            let file = text_arg(&args.values[2])?;
            let parameters = text_arg(&args.values[3])?;
            let directory = text_arg(&args.values[4])?;
            let show = xedit_script::values::to_i32(&args.values[5])?;
            *value = Value::Int64(shell_execute(&verb, &file, &parameters, &directory, show));
            Ok(())
        }),
    );
    function(
        adapter,
        "ShellApi",
        "ShellExecuteWait",
        6,
        Rc::new(|value, args| {
            let verb = text_arg(&args.values[1])?;
            let file = text_arg(&args.values[2])?;
            let parameters = text_arg(&args.values[3])?;
            let directory = text_arg(&args.values[4])?;
            let show = xedit_script::values::to_i32(&args.values[5])?;
            *value = Value::Integer(shell_execute_wait(&verb, &file, &parameters, &directory, show)?);
            Ok(())
        }),
    );
    function(
        adapter,
        "FileCtrl",
        "SelectDirectory",
        4,
        Rc::new(|value, args| {
            let _ = (value, args);
            // `SelectDirectory` is `Vcl.FileCtrl`'s directory dialog. Headless
            // there is no dialog; the form shim of phase 6 step 9 is the seam
            // that fills this in, with the same error the form classes give.
            Err(ValueError::external_class(
                EXCEPTION_CLASS,
                "SelectDirectory requires the GUI; the VCL form shim is phase 6 step 9".to_owned(),
            ))
        }),
    );
    function(
        adapter,
        "Vcl.Clipbrd",
        "GetClipboardText",
        0,
        Rc::new(|value, _args| {
            *value = Value::Str(get_clipboard_text().into_bytes());
            Ok(())
        }),
    );
    function(
        adapter,
        "Vcl.Clipbrd",
        "SetClipboardText",
        1,
        Rc::new(|_value, args| {
            let text = text_arg(&args.values[0])?;
            set_clipboard_text(&text);
            Ok(())
        }),
    );

    //=== Math ================================================================

    function(
        adapter,
        "Math",
        "CompareValue",
        3,
        Rc::new(|value, args| {
            let a = xedit_script::values::to_f64(&args.values[0])?;
            let b = xedit_script::values::to_f64(&args.values[1])?;
            let epsilon = xedit_script::values::to_f64(&args.values[2])?;
            *value = Value::Integer(compare_value(a, b, epsilon));
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "EnsureRange",
        3,
        Rc::new(|value, args| {
            let a = xedit_script::values::to_f64(&args.values[0])?;
            let min = xedit_script::values::to_f64(&args.values[1])?;
            let max = xedit_script::values::to_f64(&args.values[2])?;
            *value = Value::Double(ensure_range(a, min, max));
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "InRange",
        3,
        Rc::new(|value, args| {
            let a = xedit_script::values::to_f64(&args.values[0])?;
            let min = xedit_script::values::to_f64(&args.values[1])?;
            let max = xedit_script::values::to_f64(&args.values[2])?;
            *value = Value::Bool(a >= min && a <= max);
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "FMod",
        2,
        Rc::new(|value, args| {
            let a = xedit_script::values::to_f64(&args.values[0])?;
            let b = xedit_script::values::to_f64(&args.values[1])?;
            *value = Value::Double(a - (a / b).trunc() * b);
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "IsZero",
        2,
        Rc::new(|value, args| {
            let a = xedit_script::values::to_f64(&args.values[0])?.abs();
            let epsilon = xedit_script::values::to_f64(&args.values[1])?;
            // `if Epsilon = 0 then Epsilon := DoubleResolution`.
            let epsilon = if epsilon == 0.0 { 1E-12 } else { epsilon };
            *value = Value::Bool(a <= epsilon);
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "Lerp",
        3,
        Rc::new(|value, args| {
            let a = xedit_script::values::to_f64(&args.values[0])?;
            let b = xedit_script::values::to_f64(&args.values[1])?;
            let t = xedit_script::values::to_f64(&args.values[2])?;
            *value = Value::Double(a + (b - a) * ensure_range(t, 0.0, 1.0));
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "LerpUnclamped",
        3,
        Rc::new(|value, args| {
            let a = xedit_script::values::to_f64(&args.values[0])?;
            let b = xedit_script::values::to_f64(&args.values[1])?;
            let t = xedit_script::values::to_f64(&args.values[2])?;
            *value = Value::Double(a + (b - a) * t);
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "InverseLerp",
        3,
        Rc::new(|value, args| {
            let a = xedit_script::values::to_f64(&args.values[0])?;
            let b = xedit_script::values::to_f64(&args.values[1])?;
            let v = xedit_script::values::to_f64(&args.values[2])?;
            *value = Value::Double(if same_value(a, b) {
                0.0
            } else {
                ensure_range((v - a) / (b - a), 0.0, 1.0)
            });
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "RoundTo",
        2,
        Rc::new(|value, args| {
            let a = xedit_script::values::to_f64(&args.values[0])?;
            let digit = xedit_script::values::to_i32(&args.values[1])?;
            *value = Value::Double(round_to(a, digit));
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "SimpleRoundTo",
        2,
        Rc::new(|value, args| {
            let a = xedit_script::values::to_f64(&args.values[0])?;
            let digit = xedit_script::values::to_i32(&args.values[1])?;
            *value = Value::Double(simple_round_to(a, digit));
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "Sign",
        1,
        Rc::new(|value, args| {
            let a = xedit_script::values::to_f64(&args.values[0])?;
            *value = Value::Integer(if a == 0.0 {
                0
            } else if a < 0.0 {
                -1
            } else {
                1
            });
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "GetPrecisionMode",
        0,
        Rc::new(|value, _args| {
            *value = Value::Integer(PRECISION_MODE.load(Ordering::Relaxed));
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "SetPrecisionMode",
        1,
        Rc::new(|value, args| {
            let mode = xedit_script::values::to_i32(&args.values[0])?;
            let previous = PRECISION_MODE.swap(mode, Ordering::Relaxed);
            *value = Value::Integer(previous);
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "GetRoundMode",
        0,
        Rc::new(|value, _args| {
            *value = Value::Integer(ROUND_MODE.load(Ordering::Relaxed));
            Ok(())
        }),
    );
    function(
        adapter,
        "Math",
        "SetRoundMode",
        1,
        Rc::new(|value, args| {
            let mode = xedit_script::values::to_i32(&args.values[0])?;
            let previous = ROUND_MODE.swap(mode, Ordering::Relaxed);
            *value = Value::Integer(previous);
            Ok(())
        }),
    );

    //=== TStrings ============================================================

    // `TStrings_Read_Delimiter`/`Write_Delimiter`.
    adapter.add_get(
        classes.t_strings.clone(),
        "Delimiter",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let list = receiver(args)?;
            *value = Value::Str(borrow_list(&list).delimiter().to_string().into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        classes.t_strings.clone(),
        "Delimiter",
        Rc::new(|value: &Value, args: &mut Args| {
            // `if length(string(Value)) > 0 then ...Delimiter := string(Value)[1]`.
            let text = text_arg(value)?;
            if let Some(ch) = text.chars().next() {
                let list = receiver(args)?;
                borrow_list_mut(&list).set_delimiter(ch);
            }
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_set(
        classes.t_strings.clone(),
        "StrictDelimiter",
        Rc::new(|value: &Value, args: &mut Args| {
            let strict = xedit_script::values::to_bool(value)?;
            let list = receiver(args)?;
            borrow_list_mut(&list).set_strict_delimiter(strict);
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_set(
        classes.t_strings.clone(),
        "NameValueSeparator",
        Rc::new(|value: &Value, args: &mut Args| {
            let text = text_arg(value)?;
            if let Some(ch) = text.chars().next() {
                let list = receiver(args)?;
                borrow_list_mut(&list).set_name_value_separator(ch);
            }
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_get(
        classes.t_strings.clone(),
        "DelimitedText",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let list = receiver(args)?;
            *value = Value::Str(borrow_list(&list).delimited_text().into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        classes.t_strings.clone(),
        "DelimitedText",
        Rc::new(|value: &Value, args: &mut Args| {
            let text = text_arg(value)?;
            let list = receiver(args)?;
            borrow_list_mut(&list).set_delimited_text(&text)?;
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_iget(
        classes.t_strings.clone(),
        "ValueFromIndex",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let index = xedit_script::values::to_i32(&args.values[0])?;
            let list = receiver(args)?;
            *value = Value::Str(borrow_list(&list).value_from_index(i64::from(index)).into_bytes());
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_iset(
        classes.t_strings.clone(),
        "ValueFromIndex",
        Rc::new(|value: &Value, args: &mut Args| {
            let index = xedit_script::values::to_i32(&args.values[0])?;
            let text = text_arg(value)?;
            let list = receiver(args)?;
            borrow_list_mut(&list).set_value_from_index(i64::from(index), &text);
            Ok(())
        }),
        1,
        vec![value_param()],
    );

    // `TStringList_Read_CaseSensitive`/`Write_CaseSensitive`: registered on
    // `TStrings` as upstream, a hard cast to `TStringList` inside.
    adapter.add_get(
        classes.t_strings.clone(),
        "CaseSensitive",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let list = receiver(args)?;
            *value = Value::Bool(borrow_list(&list).case_sensitive());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        classes.t_strings.clone(),
        "CaseSensitive",
        Rc::new(|value: &Value, args: &mut Args| {
            let case_sensitive = xedit_script::values::to_bool(value)?;
            let list = receiver(args)?;
            borrow_list_mut(&list).set_case_sensitive(case_sensitive);
            Ok(())
        }),
        0,
        vec![value_param()],
    );

    // `TStrings_AddPair`.
    adapter.add_get(
        classes.t_strings.clone(),
        "AddPair",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let name = text_arg(&args.values[0])?;
            let pair = text_arg(&args.values[1])?;
            let list = receiver(args)?;
            borrow_list_mut(&list).add_pair(&name, &pair)?;
            // `Value := O2V(Strings)` -- the receiver, for chaining.
            *value = args.obj.clone().unwrap_or(Value::Null);
            Ok(())
        }),
        2,
        vec![value_param(), value_param()],
        VarType::Empty,
    );

    // `TStrings_Contains`/`ContainsName`/`ContainsObject`.
    adapter.add_get(
        classes.t_strings.clone(),
        "Contains",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let text = text_arg(&args.values[0])?;
            let list = receiver(args)?;
            *value = Value::Bool(borrow_list(&list).contains(&text));
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_get(
        classes.t_strings.clone(),
        "ContainsName",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let text = text_arg(&args.values[0])?;
            let list = receiver(args)?;
            *value = Value::Bool(borrow_list(&list).contains_name(&text));
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_get(
        classes.t_strings.clone(),
        "ContainsObject",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let list = receiver(args)?;
            *value = Value::Bool(borrow_list(&list).index_of_object(&args.values[0]) >= 0);
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );

    // `TStrings_InsertObject`: the `Objects[]` write of the base unit is not
    // part of the slice, so the index's object goes in here.
    adapter.add_get(
        classes.t_strings.clone(),
        "InsertObject",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let index = xedit_script::values::to_i32(&args.values[0])?;
            let text = text_arg(&args.values[1])?;
            let obj = args.values[2].clone();
            let list = receiver(args)?;
            {
                let mut list = borrow_list_mut(&list);
                list.insert(i64::from(index), text)?;
                list.set_object_at(i64::from(index), obj);
            }
            Ok(())
        }),
        3,
        vec![value_param(), value_param(), value_param()],
        VarType::Empty,
    );

    // `TStrings_LoadFromFile`/`SaveToFile`: one or two arguments, the
    // second a `TEncoding`.
    adapter.add_get(
        classes.t_strings.clone(),
        "LoadFromFile",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let file_name = text_arg(&args.values[0])?;
            let encoding = args.values.get(1).and_then(encoding_of);
            let list = receiver(args)?;
            borrow_list_mut(&list).load_from_file(&file_name, encoding)
        }),
        -1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_get(
        classes.t_strings.clone(),
        "SaveToFile",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let file_name = text_arg(&args.values[0])?;
            let encoding = args.values.get(1).and_then(encoding_of);
            let list = receiver(args)?;
            borrow_list_mut(&list).save_to_file(&file_name, encoding)
        }),
        -1,
        vec![value_param()],
        VarType::Empty,
    );

    // `TStrings_SetStrings`.
    adapter.add_get(
        classes.t_strings.clone(),
        "SetStrings",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let other = list_of_or_error(&args.values[0])?;
            let list = receiver(args)?;
            if Rc::ptr_eq(&other, &list) {
                return Ok(());
            }
            let source = borrow_list(&other).snapshot();
            borrow_list_mut(&list).assign(&source);
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );

    // `TStringList_Difference`/`Intersection`/`SymmetricDifference`/`Union`:
    // `StringSetOp` of `xejviScriptAdapterMisc.pas:465` -- the receiver is
    // made `dupIgnore` and `Sorted`, the two lists dedupe into sorted
    // copies, the receiver is emptied and filled by the operation.
    for (identifier, operation) in [
        ("Difference", 'D'),
        ("Intersection", 'I'),
        ("SymmetricDifference", 'S'),
        ("Union", 'U'),
    ] {
        adapter.add_get(
            classes.t_strings.clone(),
            identifier,
            Rc::new(move |_value: &mut Value, args: &mut Args| {
                let other = list_of_or_error(&args.values[0])?;
                let list = receiver(args)?;
                let other_values = if Rc::ptr_eq(&other, &list) {
                    borrow_list(&list).snapshot()
                } else {
                    borrow_list(&other).snapshot()
                };
                set_operation(&list, &other_values, operation);
                Ok(())
            }),
            1,
            vec![value_param()],
            VarType::Empty,
        );
    }

    let _ = EncodingKind::Default;
}

/// `StringSetOp` (`xejviScriptAdapterMisc.pas:465`): the receiver becomes
/// the sorted deduped operation of the two sets.
fn set_operation(
    left: &Rc<std::cell::RefCell<dyn xedit_script::values::ScriptObject>>,
    right: &crate::script_strings::StringListObject,
    operation: char,
) {
    let mut left_list = borrow_list_mut(left);
    left_list.set_duplicates(crate::script_strings::DUP_IGNORE);
    left_list.set_sorted(true);
    let set_a = deduped_sorted(&left_list.strings());
    let set_b = deduped_sorted(&right.strings());
    left_list.clear();
    match operation {
        'D' => {
            for text in &set_a {
                if !set_b.contains(text) {
                    let _ = left_list.append(text.clone());
                }
            }
        }
        'I' => {
            for text in &set_a {
                if set_b.contains(text) {
                    let _ = left_list.append(text.clone());
                }
            }
        }
        'S' => {
            for text in set_a.iter().chain(set_b.iter()) {
                if !left_list.strings().contains(text) {
                    let _ = left_list.append(text.clone());
                }
            }
            for text in &set_a {
                if set_b.contains(text) {
                    let index = left_list.strings().iter().position(|entry| entry == text);
                    if let Some(index) = index {
                        let _ = left_list.delete(index as i64);
                    }
                }
            }
        }
        _ => {
            for text in set_a.iter().chain(set_b.iter()) {
                let _ = left_list.append(text.clone());
            }
        }
    }
}

/// `TStringList` with `dupIgnore` and `Sorted`: the strings in
/// `CompareStrings` order without duplicates.
fn deduped_sorted(strings: &[String]) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for text in strings {
        if !result
            .iter()
            .any(|entry| ansi_compare_text(entry, text) == std::cmp::Ordering::Equal)
        {
            result.push(text.clone());
        }
    }
    result.sort_by(|a, b| ansi_compare_text(a, b));
    result
}

/// The `unit_name` each constant of the unit registers under (`System`,
/// `Math`, `Types`, `SysUtils`, `ExtCtrls`, `StdCtrls`, `Forms`, `Menus`,
/// `Controls`, `Windows`).
fn const_unit(name: &str) -> &'static str {
    if VAR_TYPES.iter().any(|(entry, _)| *entry == name) {
        "System"
    } else if MATH_CONSTS.iter().any(|(entry, _)| *entry == name) {
        "Math"
    } else if TYPES_CONSTS.iter().any(|(entry, _)| *entry == name) {
        "Types"
    } else if SYSUTILS_CONSTS.iter().any(|(entry, _)| *entry == name) {
        "SysUtils"
    } else if EXTCTRLS_CONSTS.iter().any(|(entry, _)| *entry == name) {
        "ExtCtrls"
    } else if STDCTRLS_CONSTS.iter().any(|(entry, _)| *entry == name) {
        "StdCtrls"
    } else if FORMS_CONSTS.iter().any(|(entry, _)| *entry == name) {
        "Forms"
    } else if MENUS_CONSTS.iter().any(|(entry, _)| *entry == name) {
        "Menus"
    } else if CONTROLS_CONSTS.iter().any(|(entry, _)| *entry == name) {
        "Controls"
    } else {
        "Windows"
    }
}

//=== Math helpers ============================================================

/// `System.Math.CompareValue(A, B, Epsilon)`: `EqualsValue` when
/// `SameValue`, else the sign of the difference.
fn compare_value(a: f64, b: f64, epsilon: f64) -> i32 {
    if same_value_with_epsilon(a, b, epsilon) {
        0
    } else if a < b {
        -1
    } else {
        1
    }
}

/// `System.Math.SameValue(A, B, Epsilon)`: `Epsilon = 0` computes
/// `Max(Min(|A|, |B|) * DoubleResolution, DoubleResolution)` with
/// `DoubleResolution = 1E-15 * FuzzFactor(1000)`.
fn same_value_with_epsilon(a: f64, b: f64, epsilon: f64) -> bool {
    let epsilon = if epsilon == 0.0 {
        (a.abs().min(b.abs()) * 1E-12).max(1E-12)
    } else {
        epsilon
    };
    if a > b { (a - b) <= epsilon } else { (b - a) <= epsilon }
}

/// `System.Math.EnsureRange`.
fn ensure_range(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// The exact decimal factors of `System.Math.RoundTo`
/// (`System.Math.pas:940`): `(10^digit, 10^-digit)` for `digit` -20..20.
fn round_to_factors(digit: i32) -> (f64, f64) {
    let power = int_power(10.0, digit.abs());
    if digit < 0 {
        (power.recip(), power)
    } else {
        (power, power.recip())
    }
}

/// `System.Math.RoundTo` (`System.Math.pas:935`): scales by the exact
/// decimal factor, rounds with `Round` (half to even) when the scaled value
/// fits, and scales back.
fn round_to(value: f64, digit: i32) -> f64 {
    const ROUND_MAX: f64 = 4503599627370496.0;
    let (out_factor, in_factor) = round_to_factors(digit);
    let mut scaled = value * in_factor;
    if scaled.abs() < ROUND_MAX {
        scaled = round(scaled) as f64;
    }
    scaled * out_factor
}

/// `System.Math.SimpleRoundTo` (`System.Math.pas:1023`): `Int(x ± 0.5)`
/// scaled, the asymmetric arithmetic rounding.
fn simple_round_to(value: f64, digit: i32) -> f64 {
    let factor = int_power(10.0, digit);
    if value < 0.0 {
        ((value / factor) - 0.5).trunc() * factor
    } else {
        ((value / factor) + 0.5).trunc() * factor
    }
}

/// `Winapi.Windows.CopyFile`.
fn copy_file(source: &str, destination: &str, fail_if_exists: bool) -> bool {
    if fail_if_exists && std::path::Path::new(destination).exists() {
        return false;
    }
    std::fs::copy(source, destination).is_ok()
}

/// `Winapi.Windows.CreateProcessWait`
/// (`JvInterpreter_CreateProcessWait`): upstream fills a `TStartUpInfo`
/// with `cb`, `STARTF_USESHOWWINDOW or STARTF_FORCEONFEEDBACK` and the
/// show state, starts the file with the parameter string as its command
/// line, waits up to `timeout` milliseconds, reports `GetExitCodeProcess`
/// whatever the wait returned -- `STILL_ACTIVE` (259) while the child runs,
/// which the wait never terminates -- and raises
/// `Exception.Create('CreateProcess failed, error code <code>')` when
/// `CreateProcess` fails.
///
/// The file is `lpApplicationName`, so Windows resolves a name without a
/// path against the current directory alone -- no search path -- exactly as
/// the oracle does (`CreateProcessWait('cmd.exe', ...)` fails with code 2
/// where a full path runs; `sys-process.pas` compares both).
fn create_process_wait(file: &str, parameters: &str, show: u16, timeout: u32) -> Result<i32, ValueError> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            CreateProcessW, GetExitCodeProcess, NORMAL_PRIORITY_CLASS, PROCESS_INFORMATION, STARTF_FORCEONFEEDBACK,
            STARTF_USESHOWWINDOW, STARTUPINFOW, WaitForSingleObject,
        };
        let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(std::iter::once(0)).collect() };
        let application = wide(file);
        // `CreateProcess` writes into the command line buffer, so it is its
        // own mutable copy.
        let mut command_line = wide(parameters);
        let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        startup.dwFlags = STARTF_USESHOWWINDOW | STARTF_FORCEONFEEDBACK;
        startup.wShowWindow = show;
        let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the two buffers are NUL-terminated and live for the call
        // (the command line is mutable for it); no environment block and no
        // current directory are given, as upstream passes nil for both.
        let ok = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                NORMAL_PRIORITY_CLASS,
                std::ptr::null(),
                std::ptr::null(),
                &startup,
                &mut process,
            )
        };
        if ok == 0 {
            let code = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return Err(ValueError::external_class(
                EXCEPTION_CLASS,
                format!("CreateProcess failed, error code {code}"),
            ));
        }
        // SAFETY: the handles are the child's, closed once its code is read.
        // `GetExitCodeProcess` answers `STILL_ACTIVE` (259) while the child
        // runs and the process is never terminated, as upstream reads it.
        let mut exit_code: u32 = 0;
        unsafe {
            WaitForSingleObject(process.hProcess, timeout);
            GetExitCodeProcess(process.hProcess, &mut exit_code);
            CloseHandle(process.hThread);
            CloseHandle(process.hProcess);
        }
        Ok(exit_code as i32)
    }
    #[cfg(not(windows))]
    {
        let _ = (file, parameters, show, timeout);
        Err(ValueError::external_class(
            EXCEPTION_CLASS,
            "CreateProcessWait is Windows only".to_owned(),
        ))
    }
}

/// `Winapi.ShellApi.ShellExecute`: the HINSTANCE-shaped result as an
/// integer (32 or greater is success, the `SE_ERR_*` codes below).
fn shell_execute(verb: &str, file: &str, parameters: &str, directory: &str, show: i32) -> i64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(std::iter::once(0)).collect() };
        let verb = wide(verb);
        let file = wide(file);
        let parameters = wide(parameters);
        let directory = wide(directory);
        // SAFETY: every pointer is a NUL-terminated buffer that lives for
        // the call; a null HWND is what the adapter passes upstream.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                parameters.as_ptr(),
                directory.as_ptr(),
                show,
            )
        };
        result as i64
    }
    #[cfg(not(windows))]
    {
        let _ = (verb, file, parameters, directory, show);
        0
    }
}

/// `Winapi.ShellApi.ShellExecuteWait`: `SEE_MASK_NOCLOSEPROCESS`, wait
/// without a timeout, then the exit code; a failure raises the upstream
/// `Exception`.
fn shell_execute_wait(verb: &str, file: &str, parameters: &str, directory: &str, show: i32) -> Result<i32, ValueError> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
        let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(std::iter::once(0)).collect() };
        let mut verb = wide(verb);
        let mut file = wide(file);
        let mut parameters = wide(parameters);
        let mut directory = wide(directory);
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_mut_ptr();
        info.lpFile = file.as_mut_ptr();
        info.lpParameters = parameters.as_mut_ptr();
        info.lpDirectory = directory.as_mut_ptr();
        info.nShow = show;
        // SAFETY: `info` is a fully initialized structure that lives for the
        // call.
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            let error = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return Err(ValueError::external_class(
                EXCEPTION_CLASS,
                format!("ShellExecute failed, error code {error}"),
            ));
        }
        let code = wait_hprocess(info.hProcess);
        Ok(code)
    }
    #[cfg(not(windows))]
    {
        let _ = (verb, file, parameters, directory, show);
        Err(ValueError::external_class(
            EXCEPTION_CLASS,
            "ShellExecuteWait is Windows only".to_owned(),
        ))
    }
}

/// `WaitForSingleObject(hProcess, INFINITE)` then `GetExitCodeProcess`.
#[cfg(windows)]
fn wait_hprocess(process: windows_sys::Win32::Foundation::HANDLE) -> i32 {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    // SAFETY: `process` is a process handle of the current call.
    unsafe {
        WaitForSingleObject(process, INFINITE);
        let mut code: u32 = STILL_ACTIVE as u32;
        GetExitCodeProcess(process, &mut code);
        CloseHandle(process);
        code as i32
    }
}

/// `Winapi.Windows.GetKeyState`.
fn get_key_state(key: i32) -> i16 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
        // SAFETY: `GetKeyState` reads the calling thread's input state.
        unsafe { GetKeyState(key) }
    }
    #[cfg(not(windows))]
    {
        let _ = key;
        0
    }
}

/// `Clipboard.AsText` (the read side of `Vcl.Clipbrd`).
fn get_clipboard_text() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
        use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
        use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
        // SAFETY: the clipboard is opened and closed around the read; the
        // locked buffer is NUL-terminated UTF-16.
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return String::new();
            }
            let handle = GetClipboardData(u32::from(CF_UNICODETEXT));
            let mut text = String::new();
            if !handle.is_null() {
                let pointer = GlobalLock(handle) as *const u16;
                if !pointer.is_null() {
                    let len = GlobalSize(handle) / 2;
                    let mut units: Vec<u16> = Vec::with_capacity(len);
                    for index in 0..len {
                        let unit = *pointer.add(index);
                        if unit == 0 {
                            break;
                        }
                        units.push(unit);
                    }
                    text = String::from_utf16_lossy(&units);
                    GlobalUnlock(handle);
                }
            }
            CloseClipboard();
            text
        }
    }
    #[cfg(not(windows))]
    {
        String::new()
    }
}

/// `Clipboard.AsText := s` (an empty string clears the clipboard).
fn set_clipboard_text(text: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
        };
        use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
        use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
        // SAFETY: the clipboard is opened and closed around the write; the
        // moveable block is handed to the system on success.
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return;
            }
            EmptyClipboard();
            if !text.is_empty() {
                let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let bytes = units.len() * std::mem::size_of::<u16>();
                let block = GlobalAlloc(GMEM_MOVEABLE, bytes);
                if !block.is_null() {
                    let pointer = GlobalLock(block) as *mut u16;
                    if !pointer.is_null() {
                        std::ptr::copy_nonoverlapping(units.as_ptr(), pointer, units.len());
                        GlobalUnlock(block);
                        SetClipboardData(u32::from(CF_UNICODETEXT), block);
                    }
                }
            }
            CloseClipboard();
        }
    }
    #[cfg(not(windows))]
    {
        let _ = text;
    }
}

/// The unit the `Text`/`Delimiter` constants of `TStrings` are not part of,
/// referenced so the imported `globals` stays in use on every platform.
#[allow(dead_code)]
fn unused_globals() -> bool {
    globals::game_mode() == globals::game_mode()
}
