// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: the `AddClass` registrations the phase 6 adapter steps
// share -- `TStrings` and `TStringList` of External/jvcl/jvcl/run/
// JvInterpreter_Classes.pas and `TEncoding` of JvInterpreter_SysUtils.pas
// (registered by xejviScriptAdapterMisc.pas), the `TJsonBaseObject`,
// `TJsonArray` and `TJsonObject` of xejviScriptAdapterMiscJson.pas and the
// `TPerlRegEx` of xejviScriptAdapterMiscRegEx.pas.

//! The script classes the adapter steps register once and share.
//!
//! `AddGet`/`AddSet`/`AddIGet`/`AddISet` entries answer a receiver by class
//! identity (an object receiver by inheritance, a class-value receiver by
//! equality, `class_matches` of `crates/xedit-script`), so every step that
//! registers methods on a class must use the *same* [`ClassDef`] instance the
//! step that hands out that class's objects used -- a second `AddClass` of
//! the same name would make the object and the methods unmatchable.
//! [`ScriptClasses`] is that single holder: `script.rs` builds it before the
//! per-unit registrars run and every registrar takes it.
//!
//! The classes a *later* step owns are registered here already, because the
//! step that needs them first cannot wait for it: `TStrings`/`TStringList`
//! belong to the JVCL `Classes` unit (step 8) and `TEncoding` to `SysUtils`
//! (step 8), but the `TStrings` surface of `xejviScriptAdapterMisc.pas` is
//! step 7's, so step 7 registers them and their base methods (the early
//! slice of the two JVCL-class units in `script_strings.rs`). Step 8 extends
//! [`ScriptClasses`] and that slice instead of registering a second class of
//! the same name (the JVCL units are not part of the phase 6 script ledger,
//! so no ledger entry depends on which step registered what).
//!
//! The element classes of the run loop (`IwbElement` and the derived
//! interfaces the record, file and container adapters add) are separate:
//! they are created per run in `script.run`, since their objects carry the
//! run's [`ElementRef`](xedit_core::interface::ElementRef).

use std::rc::Rc;

use xedit_script::adapter::Adapter;
use xedit_script::values::ClassDef;

/// The shared script classes, as the registrars of the Misc family need
/// them.
pub struct ScriptClasses {
    /// `TStrings` (`Classes`).
    pub t_strings: Rc<ClassDef>,
    /// `TStringList` (`Classes`), a `TStrings`.
    pub t_string_list: Rc<ClassDef>,
    /// `TEncoding` (`SysUtils`).
    pub t_encoding: Rc<ClassDef>,
    /// `TJsonBaseObject` (`JsonDataObjects`).
    pub t_json_base_object: Rc<ClassDef>,
    /// `TJsonArray` (`JsonDataObjects`), a `TJsonBaseObject`.
    pub t_json_array: Rc<ClassDef>,
    /// `TJsonObject` (`JsonDataObjects`), a `TJsonBaseObject`.
    pub t_json_object: Rc<ClassDef>,
    /// `TPerlRegEx` (`RegularExpressionsCore`).
    pub t_perl_regex: Rc<ClassDef>,
}

impl ScriptClasses {
    /// Registers the classes (`AddClass` order follows the units' own: the
    /// JVCL `Classes` unit registers `TStrings` before `TStringList`, the
    /// JVCL `SysUtils` unit `TEncoding`, `xejviScriptAdapterMiscJson` its
    /// three classes and `xejviScriptAdapterMiscRegEx` its one).
    pub fn register(adapter: &mut Adapter) -> ScriptClasses {
        let t_strings = adapter.add_class("Classes", "TStrings", None);
        let t_string_list = adapter.add_class("Classes", "TStringList", Some(t_strings.clone()));
        let t_encoding = adapter.add_class("SysUtils", "TEncoding", None);
        let t_json_base_object = adapter.add_class("JsonDataObjects", "TJsonBaseObject", None);
        let t_json_array = adapter.add_class("JsonDataObjects", "TJsonArray", Some(t_json_base_object.clone()));
        let t_json_object = adapter.add_class("JsonDataObjects", "TJsonObject", Some(t_json_base_object.clone()));
        let t_perl_regex = adapter.add_class("RegularExpressionsCore", "TPerlRegEx", None);
        ScriptClasses {
            t_strings,
            t_string_list,
            t_encoding,
            t_json_base_object,
            t_json_array,
            t_json_object,
            t_perl_regex,
        }
    }
}
