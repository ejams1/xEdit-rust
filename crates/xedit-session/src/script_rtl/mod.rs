// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter_System.pas,
// JvInterpreter_SysUtils.pas, JvInterpreter_Math.pas,
// JvInterpreter_Classes.pas and JvInterpreterFm.pas (the non-VCL RTL
// backbone of the 15 JVCL interpreter units xejviScriptAdapter.pas
// registers; the VCL-backed units belong to phase 6 step 9)

//! The JVCL RTL units of the script dialect: `System`, `SysUtils`, `Math`,
//! `Classes` and the four registrations `JvInterpreterFm.pas` makes itself.
//!
//! These are the Delphi RTL functions and classes a shipped script names
//! without any xEdit unit in its `uses` clause (`IntToStr`, `Format`,
//! `TStringList`, `TFileStream`, the `Math` helpers, the exception classes).
//! [`register`] binds them on an interpreter's adapter exactly as the
//! `RegisterJvInterpreterAdapter` of each unit registers them, including the
//! quirks the upstream files carry (`High` returning the low bound of a
//! variant array, `ArcCotH` binding `ArcCot`'s handler, the commented-out
//! registrations staying unregistered).
//!
//! The Delphi-side dependencies route to the ports that already exist:
//! `xedit_core::delphi` for the number and text semantics
//! (`FloatToStr`/`FloatToStrF`, `Round`, `StrToFloat`), `nif_math` for the
//! FPU-accurate `Sin`/`Cos`/`ArcTan`/`ArcSin`/`ArcCos`/`ArcTan2` and the
//! degree conversions, `xedit_io::encoding` for the locale comparisons
//! (`AnsiCompareText` is `CompareStringW`), and the phase 5 Sniff port's
//! `xedit_assets::sniff::processor` for the `TStrings` text semantics
//! (`DelimitedText`/`CommaText`, the `SaveToFile` byte shape) and the
//! `TMemIniFile` class. [`format`] is the Delphi `Format` family (the
//! `%d`/`%s`/`%f`/`%n`/`%e`/`%g`/`%m`/`%p`/`%x` conversions with the
//! index/width/precision modifiers) and [`datetime`] the `TDateTime` codec
//! the date functions share; both are new here (no earlier phase needed
//! them) and unit-tested against the Delphi rules they implement.
//!
//! What stays out of this step, and where it goes:
//!
//! * The ten VCL-backed units (`_Dialogs`, `_Windows`, `_Graphics`,
//!   `_Controls`, `_Buttons`, `_StdCtrls`, `_ComCtrls`, `_ExtCtrls`,
//!   `_Forms`, `_Menus`) are phase 6 step 9; `xejviScriptAdapterMiscVcl` is
//!   step 7's.
//! * The VCL calls the units of this step make themselves go through the
//!   [`Vcl`] seam, which step 9 implements: `JvInterpreterFm.pas`'s four
//!   registrations (the form makers and `JvInterpreterRunUnit`), the
//!   `TComponent` streaming methods of `TStream`, `TComponent`'s RTTI
//!   properties (`ComponentState`, `ComponentStyle`) and the `Exception`
//!   forms the port cannot model. Headless, [`NoVcl`] answers with a clear
//!   error naming the function.
//! * `TList`/`TStrings` items and the `TStream` buffers carry script values
//!   where upstream stores raw pointers: the interpreter's value model has
//!   no untyped memory, so a `TList` item is the value a script passed to
//!   `Add` (an element object, a string, a number), `Items[i]` hands it back
//!   unchanged, and `ReadBuffer` reads into a `var` string the way upstream's
//!   `ReadBuffer` fills its local buffer. `TStream.Read`/`FileRead` follow
//!   the same shape (upstream reads into the *variant's payload memory*,
//!   which is garbage by construction).
//! * A file API failure that upstream reports as `RaiseLastOSError` carries
//!   the port's `Cannot open file "<path>". <os error>` text instead
//!   (`TFileStream.Create`; the phase 5 data format adapter documents the
//!   same difference).

pub mod classes;
pub mod datetime;
pub mod fm;
pub mod format;
pub mod math;
pub mod system;
pub mod sysutils;

use std::cell::RefCell;
use std::rc::Rc;

use xedit_script::adapter::{Adapter, AdapterGetFn, AdapterSetFn, Args};
use xedit_script::values::{ClassDef, ParamType, Value, ValueError, VarType};

use crate::script::ScriptRunState;

/// The Delphi variant type codes of the interpreter's own value kinds
/// (`varObject`, `varRecord`, `varClass`, `varPointer`, `varSet`,
/// `varArray`).
///
/// Upstream these are `TCustomVariantType` codes the RTL allocates at
/// startup (`JvInterpreter.pas:1357-1392`), so they are not compile-time
/// constants and the JVCL units register no constant for them; a script can
/// see them only through `VarType(V)`, and cannot hold them in a portable
/// comparison. The port assigns fixed codes above the standard range; the
/// values are otherwise unobservable (`VarIsArray`'s `and varArray` bit test
/// sees [`DELPHI_VAR_STRING`] and up, none of which carry the `$2000` bit).
pub const DELPHI_VAR_OBJECT: i32 = 0x0103;
pub const DELPHI_VAR_RECORD: i32 = 0x0104;
pub const DELPHI_VAR_CLASS: i32 = 0x0105;
pub const DELPHI_VAR_POINTER: i32 = 0x0106;
pub const DELPHI_VAR_SET: i32 = 0x0107;
pub const DELPHI_VAR_ARRAY: i32 = 0x0108;
/// `varString` -- the Delphi code of the interpreter's `Str` value
/// (`varUString` in a Unicode Delphi; `varString` is the ANSI string code
/// the JVCL `System` unit registers).
pub const DELPHI_VAR_STRING: i32 = 0x0100;
/// `varAny`.
pub const DELPHI_VAR_ANY: i32 = 0x0101;
/// `varTypeMask`.
pub const DELPHI_VAR_TYPE_MASK: i32 = 0x0FFF;

/// The VCL seam: what the non-VCL units of this step cannot do without the
/// form shim of phase 6 step 9. The four functions are `JvInterpreterFm.pas`'s
/// own registrations; step 9 implements them (and extends the trait with the
/// surface its own units need).
pub trait Vcl {
    /// `JvInterpreterMakeForm`: build a form from a DFM file.
    fn make_form(&mut self, file_name: &str) -> Result<Value, ValueError>;
    /// `JvInterpreterRunForm`: build the form and `Show` it.
    fn run_form(&mut self, file_name: &str) -> Result<Value, ValueError>;
    /// `JvInterpreterRunFormModal`: build the form and run `ShowModal`.
    fn run_form_modal(&mut self, file_name: &str) -> Result<i64, ValueError>;
    /// `JvInterpreterRunUnit`: run a unit's main block in a fresh
    /// interpreter.
    fn run_unit(&mut self, file_name: &str) -> Result<Value, ValueError>;
}

/// The headless default of the [`Vcl`] seam: every call fails with the same
/// clear error, naming the function a script called. Phase 6 step 9 replaces
/// it with the form shim (the GUI phase passes the real one).
#[derive(Default)]
pub struct NoVcl;

impl NoVcl {
    fn unavailable(function: &str) -> ValueError {
        ValueError::External(format!("{function}: forms are not available in this build"))
    }
}

impl Vcl for NoVcl {
    fn make_form(&mut self, _file_name: &str) -> Result<Value, ValueError> {
        Err(NoVcl::unavailable("JvInterpreterMakeForm"))
    }

    fn run_form(&mut self, _file_name: &str) -> Result<Value, ValueError> {
        Err(NoVcl::unavailable("JvInterpreterRunForm"))
    }

    fn run_form_modal(&mut self, _file_name: &str) -> Result<i64, ValueError> {
        Err(NoVcl::unavailable("JvInterpreterRunFormModal"))
    }

    fn run_unit(&mut self, _file_name: &str) -> Result<Value, ValueError> {
        Err(NoVcl::unavailable("JvInterpreterRunUnit"))
    }
}

/// Registers every unit of this step on `adapter`, in the order
/// `xejviScriptAdapter.pas`'s `Init` calls them (the identifier lists sort
/// afterwards, so the order is only visible through the duplicate-name
/// tie-breaks the sorted lists resolve by registration order).
pub fn register(adapter: &mut Adapter, state: &Rc<RefCell<ScriptRunState>>, vcl: Rc<RefCell<dyn Vcl>>) {
    fm::register(adapter, vcl);
    system::register(adapter);
    sysutils::register(adapter);
    classes::register(adapter, state);
    math::register(adapter);
}

//=== Registration helpers ====================================================

/// `varEmpty` -- an untyped parameter (the `[varEmpty]` of nearly every
/// upstream registration).
pub(crate) const EMPTY: ParamType = ParamType {
    typ: VarType::Empty,
    by_ref: false,
    by_const: false,
};

/// `varEmpty or varByRef` -- a `var` parameter.
pub(crate) const BYREF: ParamType = ParamType {
    typ: VarType::Empty,
    by_ref: true,
    by_const: false,
};

/// `varInteger`.
pub(crate) const INTEGER: ParamType = ParamType {
    typ: VarType::Integer,
    by_ref: false,
    by_const: false,
};

/// `varString`.
pub(crate) const STRING: ParamType = ParamType {
    typ: VarType::Str,
    by_ref: false,
    by_const: false,
};

/// `varNull` -- what `AddISet(...)` declares for its value (the one `varNull`
/// registration of the JVCL units, `TList`/`TCollection`/`TStrings` `Items`).
pub(crate) const NULL: ParamType = ParamType {
    typ: VarType::Null,
    by_ref: false,
    by_const: false,
};

/// `varString or varArray or varByRef` -- `SetLength`'s first parameter
/// (`JvInterpreter_System.pas:623`); `CheckArgs` only reads the `varByRef`
/// bit, so the port records `Str` and notes the mask.
pub(crate) const STR_OR_ARRAY_BYREF: ParamType = ParamType {
    typ: VarType::Str,
    by_ref: true,
    by_const: false,
};

/// `AddFunction`: every registration of the five units returns `varEmpty`
/// (a procedure result a script must not use; the handlers assign `Value`
/// where the Delphi function had a result).
pub(crate) fn function(
    adapter: &mut Adapter,
    unit: &str,
    identifier: &str,
    params: &[ParamType],
    func: impl Fn(&mut Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    adapter.add_function(
        unit,
        identifier,
        Rc::new(func) as AdapterGetFn,
        params.len() as i32,
        params.to_vec(),
        VarType::Empty,
    );
}

/// `AddGet`: a class method or property reader.
pub(crate) fn method(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    identifier: &str,
    params: &[ParamType],
    func: impl Fn(&mut Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    adapter.add_get(
        class.clone(),
        identifier,
        Rc::new(func) as AdapterGetFn,
        params.len() as i32,
        params.to_vec(),
        VarType::Empty,
    );
}

/// `AddSet`: a property writer. Upstream every `AddSet` of the five units
/// passes a parameter count of 0 with `[varEmpty]` as its type list (the
/// assigned value reaches the handler through `Value`, not `Args.Values`;
/// `TJvInterpreterAdapter.SetValue` sets `Count := 1` only for the *index*
/// arguments of an `AddISet`).
pub(crate) fn setter(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    identifier: &str,
    params: &[ParamType],
    func: impl Fn(&Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    adapter.add_set(
        class.clone(),
        identifier,
        Rc::new(func) as AdapterSetFn,
        0,
        params.to_vec(),
    );
}

/// `AddIGet`: an indexed property reader.
pub(crate) fn index_get(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    identifier: &str,
    params: &[ParamType],
    func: impl Fn(&mut Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    adapter.add_iget(
        class.clone(),
        identifier,
        Rc::new(func) as AdapterGetFn,
        params.len() as i32,
        params.to_vec(),
        VarType::Empty,
    );
}

/// `AddISet`: an indexed property writer.
pub(crate) fn index_set(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    identifier: &str,
    params: &[ParamType],
    func: impl Fn(&Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    adapter.add_iset(
        class.clone(),
        identifier,
        Rc::new(func) as AdapterSetFn,
        params.len() as i32,
        params.to_vec(),
    );
}

/// `AddIDGet`: the default indexed property reader.
pub(crate) fn default_get(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    params: &[ParamType],
    func: impl Fn(&mut Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    adapter.add_id_get(
        class.clone(),
        Rc::new(func) as AdapterGetFn,
        params.len() as i32,
        params.to_vec(),
        VarType::Empty,
    );
}

/// `AddIDSet`: the default indexed property writer.
pub(crate) fn default_set(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    params: &[ParamType],
    func: impl Fn(&Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    adapter.add_id_set(
        class.clone(),
        Rc::new(func) as AdapterSetFn,
        params.len() as i32,
        params.to_vec(),
    );
}

/// `AddConst` of an integer value.
pub(crate) fn constant(adapter: &mut Adapter, unit: &str, identifier: &str, value: i32) {
    adapter.add_const(unit, identifier, Value::Integer(value));
}

//=== Argument helpers ========================================================

/// `Args.Values[i]` of a call whose registration guarantees the count.
pub(crate) fn arg(args: &Args, index: usize) -> &Value {
    &args.values[index]
}

/// The text of `Args.Values[i]`, the way Delphi coerces a variant argument to
/// a `string` parameter: through `VarToStr`, so numbers become their decimal
/// text.
pub(crate) fn arg_text(args: &Args, index: usize) -> Result<String, ValueError> {
    Ok(String::from_utf8_lossy(&xedit_script::values::to_str(arg(args, index))?).into_owned())
}

/// The string argument's bytes as they are (no number coercion), for the
/// functions whose upstream body indexes `string(Args.Values[0])[1]`.
pub(crate) fn arg_str(args: &Args, index: usize) -> Vec<u8> {
    match arg(args, index) {
        Value::Str(bytes) => bytes.clone(),
        other => xedit_script::values::to_str(other).unwrap_or_default(),
    }
}

/// The first character of the string argument, as upstream's
/// `string(Args.Values[i])[1]` reads it; empty for an empty string.
pub(crate) fn arg_char(args: &Args, index: usize) -> u8 {
    arg_str(args, index).first().copied().unwrap_or(0)
}

/// `Integer(Args.Values[i])` -- the variant-to-integer cast.
pub(crate) fn arg_i32(args: &Args, index: usize) -> Result<i32, ValueError> {
    xedit_script::values::to_i32(arg(args, index))
}

/// `Int64(Args.Values[i])`-shaped widening used by the `TStream` offsets.
pub(crate) fn arg_i64(args: &Args, index: usize) -> Result<i64, ValueError> {
    xedit_script::values::to_i64(arg(args, index))
}

/// `Extended(Args.Values[i])`.
pub(crate) fn arg_f64(args: &Args, index: usize) -> Result<f64, ValueError> {
    xedit_script::values::to_f64(arg(args, index))
}

/// `Boolean(Args.Values[i])`.
pub(crate) fn arg_bool(args: &Args, index: usize) -> Result<bool, ValueError> {
    xedit_script::values::to_bool(arg(args, index))
}

/// A `string` result.
pub(crate) fn text_value(text: impl Into<Vec<u8>>) -> Value {
    Value::Str(text.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::ScriptRunState;
    use xedit_script::Interpreter;

    /// The five units this step ports, with the ledger prefix, the source
    /// file name and the constant each registration's unit argument is.
    const UNITS: &[(&str, &str, &str)] = &[
        ("System", "JvInterpreter_System", "cSystem"),
        ("SysUtils", "JvInterpreter_SysUtils", "cSysUtils"),
        ("Math", "JvInterpreter_Math", "cMath"),
        ("Classes", "JvInterpreter_Classes", "cClasses"),
        ("JvInterpreterFm", "JvInterpreterFm", "cJvInterpreterFm"),
    ];

    fn register_all() -> Interpreter {
        let mut interpreter = Interpreter::new();
        let element = interpreter.adapter.add_class("", "IwbElement", None);
        let state = Rc::new(RefCell::new(ScriptRunState::new(Vec::new(), element)));
        let vcl: Rc<RefCell<dyn Vcl>> = Rc::new(RefCell::new(NoVcl));
        register(&mut interpreter.adapter, &state, vcl);
        interpreter
    }

    /// The ledger key of a registration, built the way `cargo xtask sync`
    /// builds it (`crates/xtask/src/main.rs::script_registrations`): the
    /// unit argument constant, the owner (a class type or the unit
    /// constant) and the identifier -- the `AddIDGet`/`AddIDSet` kinds name
    /// no identifier, so the ledger records their handler, which the
    /// adapter does not keep; both sides use the `<handler>` placeholder.
    fn ledger_key(unit: &str, kind: &str, class: Option<&str>, identifier: Option<&str>) -> String {
        let (_, file, constant) = UNITS.iter().find(|(name, _, _)| *name == unit).expect("known unit");
        let owner = match kind {
            "get" | "set" | "iget" | "iset" | "idget" | "idset" => class.unwrap_or_default(),
            _ => constant,
        };
        let name = match kind {
            "idget" | "idset" => "<handler>",
            _ => identifier.unwrap_or_default(),
        };
        format!("{file}:{owner}.{name}:{kind}")
    }

    /// Every live registration of the ledger's five JVCL units of this step
    /// is bound by [`register`], and the port binds nothing the ledger does
    /// not list: the port-side half of `cargo xtask check`'s script section.
    #[test]
    fn every_ledger_entry_of_the_jvcl_units_is_registered() {
        let ledger = include_str!("../../../../coverage/ledger.toml");
        let mut expected: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut key: Option<String> = None;
        for line in ledger.lines() {
            if let Some(rest) = line.strip_prefix("[script.\"") {
                key = Some(rest.trim_end_matches("\"]").to_owned());
                continue;
            }
            if let Some(status) = line.strip_prefix("status = \"") {
                let status = status.trim_end_matches('"');
                let Some(current) = key.take() else { continue };
                let unit = current.split(':').next().unwrap_or_default().to_owned();
                if !UNITS.iter().any(|(_, file, _)| *file == unit) {
                    continue;
                }
                if status == "excluded" {
                    continue;
                }
                let mut current = current;
                // The `AddIDGet`/`AddIDSet` keys carry the handler name;
                // the adapter keeps no name for them.
                if current.ends_with(":idget") || current.ends_with(":idset") {
                    let kind = current.rsplit(':').next().unwrap().to_owned();
                    let head = current
                        .rsplit_once('.')
                        .map(|(head, _)| head.to_owned())
                        .unwrap_or(current.clone());
                    current = format!("{head}.<handler>:{kind}");
                }
                expected.insert(current);
            } else if key.is_none() {
                // A header line for another kind resets nothing.
            }
        }
        assert!(!expected.is_empty(), "the ledger's script section was not read");

        let interpreter = register_all();
        let registered: std::collections::BTreeSet<String> = interpreter
            .adapter
            .registrations()
            .iter()
            .filter(|entry| UNITS.iter().any(|(name, _, _)| *name == entry.unit))
            .map(|entry| {
                ledger_key(
                    &entry.unit,
                    entry.kind,
                    entry.class.as_deref(),
                    entry.identifier.as_deref(),
                )
            })
            .collect();

        let missing: Vec<&String> = expected.difference(&registered).collect();
        let extra: Vec<&String> = registered.difference(&expected).collect();
        assert!(missing.is_empty(), "ledger entries not registered: {missing:?}");
        assert!(extra.is_empty(), "registered names not in the ledger: {extra:?}");
    }

    /// The per-unit registration counts, against the ledger's live entries
    /// of each unit (the ledger being the mechanical census of the upstream
    /// files): 76 `System`, 210 `SysUtils`, 67 `Math`, 191 `Classes` and the
    /// 4 of `JvInterpreterFm`, the commented-out registrations excluded.
    #[test]
    fn the_registration_counts_match_the_ledger() {
        let interpreter = register_all();
        let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for entry in interpreter.adapter.registrations() {
            if UNITS.iter().any(|(name, _, _)| *name == entry.unit) {
                *counts.entry(entry.unit.clone()).or_default() += 1;
            }
        }
        assert_eq!(counts.get("System"), Some(&76));
        assert_eq!(counts.get("SysUtils"), Some(&210));
        assert_eq!(counts.get("Math"), Some(&67));
        assert_eq!(counts.get("Classes"), Some(&191));
        assert_eq!(counts.get("JvInterpreterFm"), Some(&4));
    }
}
