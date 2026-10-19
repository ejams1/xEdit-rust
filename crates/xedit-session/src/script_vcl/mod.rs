// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter_Dialogs.pas,
// JvInterpreter_Windows.pas, JvInterpreter_Graphics.pas,
// JvInterpreter_Controls.pas, JvInterpreter_Buttons.pas,
// JvInterpreter_StdCtrls.pas, JvInterpreter_ComCtrls.pas,
// JvInterpreter_ExtCtrls.pas, JvInterpreter_Forms.pas and
// JvInterpreter_Menus.pas (the ten VCL-backed JVCL interpreter units) and
// xEdit/JvI/xejviScriptAdapterMiscVcl.pas

//! The VCL surface of the script dialect and the form shim (phase 6 step 9).
//!
//! The ten VCL-backed JVCL units register the classes, constants, properties,
//! methods and helpers a script that builds or drives a form needs, and
//! `xejviScriptAdapterMiscVcl.pas` adds the binary file streams, the
//! `IniFiles` classes, the `IOUtils` directory and file helpers and a handful
//! of members of the JVCL classes. This module registers that surface exactly
//! as the units do (class by class, member by member; the tables of
//! [`tables`] are transcribed from their `RegisterJvInterpreterAdapter` calls
//! and the constant values are captured from the 4.1.5q oracle).
//!
//! **The seam.** A member or function that needs a window, a canvas or a
//! dialog goes through the [`Vcl`] trait instead of being answered here.
//! Since issue #26 a script run passes [`interactive::VclModel`]: a headless
//! object model that keeps the form and control objects a script builds
//! (properties, events and synthetic handles) and answers its dialogs from
//! the recorded answers of `script.run` (`pending_interaction` when none is
//! recorded). [`NoVcl`] remains the refusing implementation: it raises one
//! clear error naming the VCL surface and the script ([`VCL_ERROR_CLASS`]),
//! which is also what the model refuses where it has nothing to answer (the
//! four `JvInterpreterFm` form-runner functions). Phase 7 renders the same
//! model with a real window layer; nothing else in this module changes when
//! it does.
//!
//! **What answers headless.** The pure helpers and value types of the units:
//! `TPoint`/`TRect` and `Point`/`Rect`/`Bounds` with the two `TRect`
//! accessors ([`windows`]), the `Controls` hint and cursor helpers and the
//! `Forms` shift-state helpers, and the `Menus` shortcut codec
//! ([`helpers`]) -- all unit-tested against values captured from the oracle.
//! The `IniFiles` classes and the `IOUtils` helpers of
//! `xejviScriptAdapterMiscVcl.pas` are answered too ([`misc_vcl`]): they are
//! ordinary files, not windows. Everything else refuses through the seam.
//!
//! **The one registration the tables do not carry**: `TBinaryReader`,
//! `TBinaryWriter` and `TBytesStream` are `xejviScriptAdapterMiscVcl.pas`
//! registrations over the stream objects of the JVCL `Classes` unit (phase 6
//! step 8). They need no window: their constructors and members are bound
//! with that unit's stream object model
//! (`crate::script_rtl::classes::register_binary_streams`), not through the
//! seam, and the `streams.pas` probe witnesses every typed read and write
//! against the oracle; [`misc_vcl::ledger_keys`] carries their ledger keys.

pub mod dfm;
pub mod helpers;
pub mod interactive;
pub mod misc_vcl;
pub mod tables;
pub mod windows;

use std::cell::RefCell;
use std::rc::Rc;

use xedit_script::adapter::{Adapter, AdapterGetFn, AdapterSetFn, Args};
use xedit_script::values::{ClassDef, ParamType, Value, VarType};

use crate::script::ScriptRunState;

/// Re-exported so the units of this step and their handlers name one seam
/// (the trait lives with the RTL units that need its four form functions,
/// `crate::script_rtl`).
pub use crate::script_rtl::{MemberKind, NoVcl, VCL_ERROR_CLASS, Vcl};

/// The Delphi class the parent of a table row and the port do not register
/// themselves, with the unit upstream registers it in. Created only when the
/// unit that owns the class is not registered (a build before phase 6 step 8
/// landed registers them); a class another unit registered is adopted.
const ANCHORS: &[(&str, &str)] = &[
    ("TObject", "System"),
    ("TPersistent", "Classes"),
    ("TComponent", "Classes"),
    ("TCollectionItem", "Classes"),
    ("TCollection", "Classes"),
    ("TMemoryStream", "Classes"),
    ("TStringList", "Classes"),
    ("Exception", "SysUtils"),
];

/// The registered class named `name`, creating it (and its ancestors) when it
/// is not there yet. A class of [`tables::CLASSES`] gets its parent from the
/// table -- the nearest registered ancestor of the Delphi hierarchy, so `is`
/// checks see the same answers the oracle's RTTI gives; a class of
/// [`ANCHORS`] the unit that owns it; anything else has no parent.
pub(crate) fn class_of(adapter: &mut Adapter, unit: &str, name: &str) -> Rc<ClassDef> {
    if let Some(class) = adapter.find_class(name.as_bytes()) {
        return class;
    }
    if let Some(row) = tables::CLASSES.iter().find(|row| row.1 == name) {
        let parent = class_of(adapter, row.0, row.2);
        return adapter.add_class(row.0, name, Some(parent));
    }
    if let Some((_, anchor_unit)) = ANCHORS.iter().find(|(anchor, _)| *anchor == name) {
        return adapter.add_class(anchor_unit, name, None);
    }
    adapter.add_class(unit, name, None)
}

/// The parent of a row: the row's parent name, resolved; the root of an
/// anchor chain has none.
fn parent_of(adapter: &mut Adapter, unit: &str, parent: &str) -> Option<Rc<ClassDef>> {
    if parent.is_empty() {
        None
    } else {
        Some(class_of(adapter, unit, parent))
    }
}

/// The parameter list of a table row: `.` is `varEmpty`, `&` `varByRef`,
/// `n` `varNull`, `i` `varInteger`, `b` `varBoolean` (the five the VCL units
/// declare).
pub(crate) fn params(encoded: &str) -> Vec<ParamType> {
    encoded
        .chars()
        .map(|code| match code {
            '.' => ParamType::value(VarType::Empty),
            '&' => ParamType {
                typ: VarType::Empty,
                by_ref: true,
                by_const: false,
            },
            'n' => ParamType::value(VarType::Null),
            'i' => ParamType::value(VarType::Integer),
            'b' => ParamType::value(VarType::Bool),
            unknown => unreachable!("param code {unknown:?}"),
        })
        .collect()
}

/// One member handler that asks the seam.
pub(crate) fn member_handler(
    vcl: &Rc<RefCell<dyn Vcl>>,
    class: &'static str,
    member: &'static str,
    kind: MemberKind,
) -> AdapterGetFn {
    let vcl = vcl.clone();
    Rc::new(move |value: &mut Value, args: &mut Args| vcl.borrow_mut().member(class, member, kind, value, args))
}

/// One property writer that asks the seam. The assigned value reaches a
/// handler as the `Value` parameter (`TJvInterpreterAdapter.SetValue`), which
/// the adapter passes as the function's `&Value`.
pub(crate) fn setter_handler(
    vcl: &Rc<RefCell<dyn Vcl>>,
    class: &'static str,
    member: &'static str,
    kind: MemberKind,
) -> AdapterSetFn {
    let vcl = vcl.clone();
    Rc::new(move |value: &Value, args: &mut Args| {
        let mut assigned = value.clone();
        vcl.borrow_mut().member(class, member, kind, &mut assigned, args)
    })
}

/// One unit function handler that asks the seam.
fn function_handler(vcl: &Rc<RefCell<dyn Vcl>>, unit: &'static str, function: &'static str) -> AdapterGetFn {
    let vcl = vcl.clone();
    Rc::new(move |value: &mut Value, args: &mut Args| vcl.borrow_mut().function(unit, function, value, args))
}

/// Registers the ten VCL-backed units and `xejviScriptAdapterMiscVcl.pas` on
/// `adapter`, in the order `xejviScriptAdapter.pas`'s `Init` calls them
/// (`Dialogs`, `Windows`, `Graphics`, `Controls`, `Buttons`, `StdCtrls`,
/// `ComCtrls`, `ExtCtrls`, `Forms`, `Menus`, then the misc family's own
/// `xejviScriptAdapterMiscVcl`, which upstream registers at the tail of
/// `xejviScriptAdapterMisc`).
///
/// Every registration of the tables is bound: the pure helpers and record
/// types answer here, everything that needs a window goes through `vcl` (the
/// headless [`NoVcl`] refuses with [`VCL_ERROR_CLASS`]).
pub fn register(adapter: &mut Adapter, state: &Rc<RefCell<ScriptRunState>>, vcl: &Rc<RefCell<dyn Vcl>>) {
    // The classes first: the members of a class need its descriptor, and the
    // member tables of the later units attach to classes the earlier ones
    // registered (`TBitBtn` is a `TButton` of the `StdCtrls` unit).
    for (unit, name, parent) in tables::CLASSES {
        if adapter.find_class(name.as_bytes()).is_none() {
            let parent = parent_of(adapter, unit, parent);
            adapter.add_class(unit, name, parent);
        }
    }

    for (unit, name, value) in tables::CONSTANTS {
        adapter.add_const(unit, name, Value::Integer(*value as i32));
    }

    for (_, class, member, kind, count, encoded) in tables::MEMBERS {
        let class_def = class_of(adapter, "", class);
        let params = params(encoded);
        match kind {
            'g' => adapter.add_get(
                class_def,
                member,
                member_handler(vcl, class, member, MemberKind::Get),
                *count,
                params,
                VarType::Empty,
            ),
            's' => adapter.add_set(
                class_def,
                member,
                setter_handler(vcl, class, member, MemberKind::Set),
                *count,
                params,
            ),
            'i' => adapter.add_iget(
                class_def,
                member,
                member_handler(vcl, class, member, MemberKind::IndexedGet),
                *count,
                params,
                VarType::Empty,
            ),
            'I' => adapter.add_iset(
                class_def,
                member,
                setter_handler(vcl, class, member, MemberKind::IndexedSet),
                *count,
                params,
            ),
            unknown => unreachable!("member kind {unknown:?}"),
        }
    }

    for (unit, name, count, encoded) in tables::FUNCTIONS {
        // `Point`, `Rect` and `Bounds` are registered by [`windows::register`]
        // with the record definitions they build; the rest of the `Windows`
        // unit registers no function.
        if *unit == "Windows" {
            continue;
        }
        let func: AdapterGetFn = match helpers::native_function(unit, name) {
            Some(native) => Rc::new(native),
            None => function_handler(vcl, unit, name),
        };
        adapter.add_function(unit, name, func, *count, params(encoded), VarType::Empty);
    }

    for (unit, event, event_class) in tables::HANDLERS {
        adapter.add_handler(unit, event, event_class);
    }

    windows::register(adapter, vcl);
    misc_vcl::register(adapter, state, vcl);

    // The classes an implementation that builds objects needs: the `Rc`s of
    // the ten units' `AddClass` rows (`ClassDef::is_a` compares pointers,
    // so a `TForm.Create` object must answer the class the member
    // registrations were made against) and the RTL classes an object model
    // hands out as values (the `Classes` unit's `TStringList`, for `Items`
    // and `Lines`). Registered after the RTL units, so `find_class` sees
    // them.
    let mut classes: std::collections::HashMap<String, std::rc::Rc<xedit_script::values::ClassDef>> =
        std::collections::HashMap::new();
    for (unit, name, _) in tables::CLASSES {
        classes.insert((*name).to_owned(), class_of(adapter, unit, name));
    }
    // The control classes `xejviScriptAdapterMiscVcl.pas` adds (`TCheckListBox`,
    // `TLabeledEdit` and their parents), built through the seam like the
    // ten units' classes.
    for name in misc_vcl::CONTROL_CLASSES {
        if let Some(found) = adapter.find_class(name.as_bytes()) {
            classes.insert((*name).to_owned(), found);
        }
    }
    for anchor in [
        "TObject",
        "TPersistent",
        "TComponent",
        "TCollectionItem",
        "TCollection",
        "TMemoryStream",
        "TStringList",
        "Exception",
    ] {
        if let Some(found) = adapter.find_class(anchor.as_bytes()) {
            classes.insert(anchor.to_owned(), found);
        }
    }
    vcl.borrow_mut().install_classes(classes);
}

//=== The ledger ==============================================================

/// The port-side half of `cargo xtask check`'s script section: one ledger key
/// for every registration the tables and the two hand-registered units make,
/// built the way `cargo xtask sync` builds it (`crates/xtask/src/main.rs`,
/// `script_registrations`), with the counts the ledger must carry. The
/// constants of [`ANCHORS`] and the classes of the VCL units are declaration
/// entries (their key ends in `:class`).
#[cfg(test)]
pub(crate) fn ledger_keys() -> std::collections::BTreeMap<String, usize> {
    // The scanner reads the registration's *raw* unit argument, so the key
    // carries the `cControls`-shaped constant, not its value.
    let unit_file = |unit: &str| -> &'static str {
        match unit {
            "Dialogs" => "JvInterpreter_Dialogs",
            "Windows" => "JvInterpreter_Windows",
            "Graphics" => "JvInterpreter_Graphics",
            "Controls" => "JvInterpreter_Controls",
            "Buttons" => "JvInterpreter_Buttons",
            "StdCtrls" => "JvInterpreter_StdCtrls",
            "ComCtrls" => "JvInterpreter_ComCtrls",
            "ExtCtrls" => "JvInterpreter_ExtCtrls",
            "Forms" => "JvInterpreter_Forms",
            "Menus" => "JvInterpreter_Menus",
            "IH_Controls" => "JvInterpreter_Controls",
            other => unreachable!("unit {other:?}"),
        }
    };
    let unit_const = |unit: &str| -> String {
        if unit == "IH_Controls" {
            unit.to_owned()
        } else {
            format!("c{unit}")
        }
    };
    let mut keys = std::collections::BTreeMap::new();
    let mut add = |key: String| *keys.entry(key).or_insert(0) += 1;
    for (unit, name, _) in tables::CLASSES {
        add(format!("{}:{}.{name}:class", unit_file(unit), unit_const(unit)));
    }
    for (unit, name, _) in tables::CONSTANTS {
        add(format!("{}:{}.{name}:const", unit_file(unit), unit_const(unit)));
    }
    for (stem, class, member, kind, _, _) in tables::MEMBERS {
        let kind = match kind {
            'g' => "get",
            's' => "set",
            'i' => "iget",
            'I' => "iset",
            other => unreachable!("member kind {other:?}"),
        };
        add(format!("{stem}:{class}.{member}:{kind}"));
    }
    for (unit, name, _, _) in tables::FUNCTIONS {
        add(format!("{}:{}.{name}:function", unit_file(unit), unit_const(unit)));
    }
    // The hand registrations of the `Windows` unit (`windows.rs`): the two
    // records, their accessors, `AddExtUnit` and `AddExtFun` (the key of a
    // `recget`/`recset` carries the record type the scanner read, the one of
    // `AddExtUnit` the raw unit argument).
    for key in [
        "JvInterpreter_Windows:cWindows.TPoint:rec",
        "JvInterpreter_Windows:cWindows.TRect:rec",
        "JvInterpreter_Windows:TRect.TopLeft:recget",
        "JvInterpreter_Windows:TRect.TopLeft:recset",
        "JvInterpreter_Windows:TRect.BottomRight:recget",
        "JvInterpreter_Windows:TRect.BottomRight:recset",
        "JvInterpreter_Windows:cWindows:extunit",
        "JvInterpreter_Windows:cWindows.MessageBox:extfun",
    ] {
        add(key.to_owned());
    }
    for (unit, event, _) in tables::HANDLERS {
        // The sync scanner reads the raw argument of the registration, which
        // is the `cControls`-shaped constant, not its value.
        let constant = format!("c{}", unit);
        add(format!("{}:{constant}.{event}:handler", unit_file(unit)));
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use xedit_script::Interpreter;
    use xedit_script::values::ValueError;

    #[test]
    fn the_headless_shim_refuses_with_the_class_and_the_script() {
        let mut vcl = NoVcl::new("Weather Editor");
        let mut value = Value::Empty;
        let mut args = Args::new();
        let error = vcl
            .member("TForm", "ShowModal", MemberKind::Get, &mut value, &mut args)
            .unwrap_err();
        match error {
            ValueError::ExternalClass { class, message } => {
                assert_eq!(class, "EVclUnavailable");
                assert_eq!(
                    message,
                    "TForm.ShowModal is not available without a GUI (script \"Weather Editor\")"
                );
            }
            other => panic!("{other:?}"),
        }
        let error = vcl
            .function("Dialogs", "InputQuery", &mut value, &mut args)
            .unwrap_err();
        match error {
            ValueError::ExternalClass { class, message } => {
                assert_eq!(class, "EVclUnavailable");
                assert_eq!(
                    message,
                    "InputQuery is not available without a GUI (script \"Weather Editor\")"
                );
            }
            other => panic!("{other:?}"),
        }
        for error in [
            vcl.make_form("f.dfm").unwrap_err(),
            vcl.run_form("f.dfm").unwrap_err(),
            vcl.run_form_modal("f.dfm").unwrap_err(),
            vcl.run_unit("u.pas").unwrap_err(),
        ] {
            match error {
                ValueError::ExternalClass { class, message } => {
                    assert_eq!(class, "EVclUnavailable");
                    assert!(message.contains("script \"Weather Editor\""), "{message}");
                }
                other => panic!("{other:?}"),
            }
        }
    }

    /// The registration counts against the source census: the ten units'
    /// `RegisterJvInterpreterAdapter` calls register 1,127 members (820 `Get`,
    /// 288 `Set`, 11 `IGet`, 8 `ISet`), 103 classes, 782 constants, 44
    /// functions and 41 handlers; `xejviScriptAdapterMiscVcl.pas` adds its
    /// 103 (`misc_vcl`), and `_Windows`' records, `AddExtUnit` and
    /// `AddExtFun` are registered by hand.
    #[test]
    fn the_registration_counts_match_the_census() {
        assert_eq!(tables::CLASSES.len(), 103);
        assert_eq!(tables::CONSTANTS.len(), 782);
        assert_eq!(tables::MEMBERS.len(), 1127);
        assert_eq!(tables::FUNCTIONS.len(), 44);
        assert_eq!(tables::HANDLERS.len(), 41);
        let members = |kind: char| tables::MEMBERS.iter().filter(|row| row.3 == kind).count();
        assert_eq!(members('g'), 820);
        assert_eq!(members('s'), 288);
        assert_eq!(members('i'), 11);
        assert_eq!(members('I'), 8);
        // The `Menus` key-name table of the oracle: 127 codes carry a name,
        // 108 distinct names parse back (the aliases collapse).
        assert_eq!(tables::KEY_NAMES.len(), 127);
        assert_eq!(tables::KEY_PARSE.len(), 108);
    }

    /// The port-side half of `cargo xtask check`'s script section: every
    /// `covered` entry of the eleven units this step ports is registered by
    /// the port, and the port registers nothing of those units the ledger
    /// does not list at all (since issue #26 the entries are `covered` --
    /// the headless model of [`interactive`] -- or `presentation`, the
    /// renderer-only members).
    #[test]
    fn every_covered_ledger_entry_of_the_units_is_registered() {
        let ledger = include_str!("../../../../coverage/ledger.toml");
        let units = [
            "JvInterpreter_Dialogs",
            "JvInterpreter_Windows",
            "JvInterpreter_Graphics",
            "JvInterpreter_Controls",
            "JvInterpreter_Buttons",
            "JvInterpreter_StdCtrls",
            "JvInterpreter_ComCtrls",
            "JvInterpreter_ExtCtrls",
            "JvInterpreter_Forms",
            "JvInterpreter_Menus",
            "xejviScriptAdapterMiscVcl",
        ];
        let mut covered = std::collections::BTreeSet::new();
        let mut listed = std::collections::BTreeSet::new();
        let mut key: Option<String> = None;
        for line in ledger.lines() {
            if let Some(rest) = line.strip_prefix("[script.\"") {
                key = Some(rest.trim_end_matches("\"]").to_owned());
                continue;
            }
            if let Some(status) = line.strip_prefix("status = \"") {
                let status = status.trim_end_matches('"');
                let Some(current) = key.take() else { continue };
                if !units.iter().any(|unit| current.starts_with(&format!("{unit}:"))) {
                    continue;
                }
                listed.insert(current.clone());
                if status == "covered" {
                    covered.insert(current);
                }
            }
        }
        assert!(!covered.is_empty(), "the ledger's script section was not read");

        let mut registered: std::collections::BTreeSet<String> = ledger_keys().into_keys().collect();
        registered.extend(super::misc_vcl::ledger_keys());

        let missing: Vec<&String> = covered.difference(&registered).collect();
        let extra: Vec<&String> = registered.difference(&listed).collect();
        assert!(missing.is_empty(), "covered entries not registered: {missing:?}");
        assert!(extra.is_empty(), "registered names not in the ledger: {extra:?}");
    }

    /// The classes and their parents resolve to one descriptor each (a member
    /// registered by a later unit attaches to the class an earlier one made:
    /// `TBitBtn` is the `StdCtrls` unit's `TButton`).
    #[test]
    fn the_classes_resolve_once_with_their_parents() {
        let mut interpreter = Interpreter::new();
        let adapter = &mut interpreter.adapter;
        for (unit, name, _) in tables::CLASSES {
            class_of(adapter, unit, name);
        }
        let count = tables::CLASSES
            .iter()
            .filter(|(_, name, _)| adapter.find_class(name.as_bytes()).is_some())
            .count();
        assert_eq!(count, tables::CLASSES.len());
        let t_bit_btn = adapter.find_class(b"TBitBtn").expect("TBitBtn");
        assert_eq!(
            t_bit_btn.parent.as_ref().map(|parent| parent.name.as_str()),
            Some("TButton")
        );
        let t_form = adapter.find_class(b"TForm").expect("TForm");
        assert_eq!(
            t_form.parent.as_ref().map(|parent| parent.name.as_str()),
            Some("TCustomForm")
        );
        // `TForm` is a `TWinControl` and a `TComponent`; `TFont` is not.
        let t_win_control = adapter.find_class(b"TWinControl").expect("TWinControl");
        assert!(t_form.is_a(&t_win_control));
        let t_font = adapter.find_class(b"TFont").expect("TFont");
        assert!(!t_font.is_a(&t_win_control));
    }
}
