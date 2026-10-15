// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapter.pas (the Dummy unit's
// functions and constants, the TwbVector and TwbGridCell records and their
// constructors of the wbInterface unit, and the wbInterface external unit)
// and Core/wbHelpers.pas (FullPathToFilename), Core/wbInterface.pas (the
// TwbVersion.ToCardinal of `wbVersionNumber`, the record definitions
// GetRecordDefNames lists) and Core/wbImplementation.pas
// (wbRemoveDuplicateStrings' TStringList.Find use).

//! The registrations of `xejviScriptAdapter.pas` itself: the `Dummy` unit's
//! functions and constants and the `wbInterface` unit's `TwbVector` and
//! `TwbGridCell` records with their constructors.
//!
//! The `et*`, `ca*` and `ct*` constants of this unit belong to the step-4
//! run loop (its filter hooks name them); they are registered in
//! `script.rs` and are not repeated here (`Adapter::add_const` asserts on a
//! duplicate identifier). Everything else of the unit is here: the 14
//! `gm*`, the 14 `es*`, the 17 `dt*`, the 6 `cs*`, the 5 `res*` and the 30
//! `at*` constants with the ordinals of the upstream enumerations, the
//! `wbInterface` external unit, and the unit's 10 functions and 2 records.

use std::cell::RefCell;
use std::rc::Rc;

use xedit_core::interface::constructors::record_defs;
use xedit_core::interface::def::NamedDef;
use xedit_core::interface::globals::{self, GameMode};
use xedit_core::interface::sub_record::SignatureDef;
use xedit_io::encoding::lower_case;

use xedit_script::adapter::{Adapter, Args};
use xedit_script::values::{ParamType, RecordField, RecordInstance, Value, VarType};

use crate::script_classes::ScriptClasses;
use crate::script_strings::{borrow_list_mut, list_of_or_error, text_arg};

/// `cInterface` of `xejviScriptAdapter.pas`: the unit the other adapters
/// register their functions under (an external unit, `AddExtUnit`).
pub const WB_INTERFACE_UNIT: &str = "wbInterface";

/// `cTwbVector`/`cTwbGridCell`.
pub const C_TWB_VECTOR: &str = "TwbVector";
pub const C_TWB_GRID_CELL: &str = "TwbGridCell";

/// `VersionString` of `Core/wbInterface.pas:57`: the release the port
/// targets, 4.1.5q. `wbVersionNumber` is
/// `TwbVersion.ToCardinal` of it: `(Major shl 24) or (Minor shl 16) or
/// (Release shl 8)`, plus `Ord(Build) - Ord('a')` for a single lowercase
/// letter build.
pub const WB_VERSION: (u32, u32, u32, char) = (4, 1, 5, 'q');

/// `TwbVersion.ToCardinal` (`Core/wbInterface.pas:23683`).
pub fn version_to_cardinal() -> u32 {
    let (major, minor, release, build) = WB_VERSION;
    let mut result = ((major & 0xFF) << 24) | ((minor & 0xFF) << 16) | ((release & 0xFF) << 8);
    if build.is_ascii_lowercase() {
        result += (build as u32) - ('a' as u32);
    }
    result
}

/// The ordinal of `TwbElementState` (`Core/wbInterface.pas:861`) and its
/// alias: upstream registers `esDeciding` with the ordinal of `esResolving`
/// (`xejviScriptAdapter.pas:249`), so both names carry 11.
const ELEMENT_STATES: &[(&str, i32)] = &[
    ("esModified", 0),
    ("esInternalModified", 1),
    ("esUnsaved", 2),
    ("esSortKeyValid", 3),
    ("esExtendedSortKeyValid", 4),
    ("esHidden", 5),
    ("esParentHidden", 6),
    ("esParentHiddenChecked", 7),
    ("esNotReachable", 8),
    ("esReachable", 9),
    ("esTagged", 10),
    ("esResolving", 11),
    ("esNotSuitableToAddTo", 12),
    // The name of older releases, registered with `Ord(esResolving)`.
    ("esDeciding", 11),
];

/// The ordinal of `TwbDefType` (`Core/wbInterface.pas:634`). Only the names
/// the unit registers; `dtFlag`, `dtGuid`, `dtResolvable` and the others
/// stay unregistered as upstream.
const DEF_TYPES: &[(&str, i32)] = &[
    ("dtRecord", 0),
    ("dtSubRecord", 1),
    ("dtSubRecordArray", 2),
    ("dtSubRecordStruct", 3),
    ("dtSubRecordUnion", 4),
    ("dtString", 5),
    ("dtLString", 6),
    ("dtLenString", 7),
    ("dtByteArray", 8),
    ("dtInteger", 9),
    ("dtIntegerFormater", 10),
    ("dtFloat", 13),
    ("dtArray", 15),
    ("dtStruct", 16),
    ("dtUnion", 17),
    ("dtEmpty", 19),
    ("dtStructChapter", 20),
];

/// The ordinal of `TwbContainerState` (`Core/wbInterface.pas:1382`).
const CONTAINER_STATES: &[(&str, i32)] = &[
    ("csInit", 0),
    ("csInitOnce", 1),
    ("csInitDone", 2),
    ("csInitializing", 3),
    ("csRefsBuild", 5),
    ("csAsCreatedEmpty", 6),
];

/// The ordinal of `TwbAssetType` (`Core/wbBSArchive.pas:437`).
const ASSET_TYPES: &[(&str, i32)] = &[
    ("atNone", 0),
    ("atMesh", 1),
    ("atTexture", 2),
    ("atMaterial", 3),
    ("atGeometry", 4),
    ("atSound", 5),
    ("atVoice", 6),
    ("atMusic", 7),
    ("atScript", 8),
    ("atSource", 9),
    ("atSourceSSE", 10),
    ("atStrings", 11),
    ("atSpeedTree", 12),
    ("atVideo", 13),
    ("atLODSettings", 14),
    ("atDistantLOD", 15),
    ("atInterface", 16),
    ("atProgram", 17),
    ("atMenus", 18),
    ("atFont", 19),
    ("atFacegen", 20),
    ("atLSData", 21),
    ("atShaders", 22),
    ("atGrass", 24),
    // The unit spells it `atPrevis`; the enumeration is `atPreVis`.
    ("atPrevis", 25),
    ("atSeq", 26),
    ("atDialogueViews", 27),
    ("atBookArt", 28),
    ("atIcon", 29),
    ("atSplash", 30),
];

/// The deprecated `TGameResourceType` names, registered with the ordinals
/// of the `TwbAssetType` values they alias (`xejviScriptAdapter.pas:304`).
const RESOURCE_TYPES: &[(&str, i32)] = &[
    ("resMesh", 1),
    ("resTexture", 2),
    ("resMaterial", 3),
    ("resSound", 5),
    ("resMusic", 7),
];

/// `FullPathToFilename` (`Core/wbHelpers.pas:508`): path separators
/// spelled out for a file name -- ` \ ` and ` \` lose the space, `"` and
/// `:` and `/` become `'`, `-` and ` `, and trailing spaces are dropped.
pub fn full_path_to_filename(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let mut index = chars.len();
    while index >= 1 {
        let at = index - 1;
        if chars.len() > index + 1 && chars[at] == ' ' && chars[at + 1] == '\\' && chars[at + 2] == ' ' {
            chars.remove(at + 1);
            chars.remove(at);
        } else if chars.len() > index && chars[at] == ' ' && chars[at + 1] == '\\' {
            chars.remove(at);
        } else if chars[at] == '"' {
            chars[at] = '\'';
        } else if chars[at] == ':' {
            chars[at] = '-';
        } else if chars[at] == '/' {
            chars[at] = ' ';
        }
        index -= 1;
    }
    let mut result: String = chars.into_iter().collect();
    while result.ends_with(' ') {
        result.pop();
    }
    result
}

/// The `Dummy` unit and the `wbInterface` unit of `xejviScriptAdapter.pas`.
pub fn register(adapter: &mut Adapter, classes: &ScriptClasses) {
    let c_unit = crate::script::C_UNIT;

    //=== The wbInterface unit: AddExtUnit and the two records ===============

    adapter.add_ext_unit(WB_INTERFACE_UNIT);

    // `TwbVector`: x, y, z at offsets 0, 4 and 8 as `varSingle`. The port
    // has no `varSingle`; the fields are doubles (documented divergence,
    // `values.rs`).
    let vector_def = adapter.add_rec(
        WB_INTERFACE_UNIT,
        C_TWB_VECTOR,
        12,
        vec![
            RecordField {
                name: "x".to_owned(),
                offset: 0,
                typ: VarType::Double,
                data_type: None,
            },
            RecordField {
                name: "y".to_owned(),
                offset: 4,
                typ: VarType::Double,
                data_type: None,
            },
            RecordField {
                name: "z".to_owned(),
                offset: 8,
                typ: VarType::Double,
                data_type: None,
            },
        ],
    );
    // `TwbGridCell`: x and y as `varInteger`.
    let grid_cell_def = adapter.add_rec(
        WB_INTERFACE_UNIT,
        C_TWB_GRID_CELL,
        8,
        vec![
            RecordField {
                name: "x".to_owned(),
                offset: 0,
                typ: VarType::Integer,
                data_type: None,
            },
            RecordField {
                name: "y".to_owned(),
                offset: 4,
                typ: VarType::Integer,
                data_type: None,
            },
        ],
    );

    {
        let def = vector_def.clone();
        adapter.add_function(
            WB_INTERFACE_UNIT,
            "wbVector",
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let mut record = RecordInstance::new(def.clone());
                for (index, argument) in args.values.iter().take(3).enumerate() {
                    record.values[index] = xedit_script::values::var_as_type(argument, VarType::Double)?;
                }
                *value = Value::Record(Rc::new(RefCell::new(record)));
                Ok(())
            }),
            3,
            vec![
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
            ],
            VarType::Record,
        );
    }
    {
        let def = grid_cell_def.clone();
        adapter.add_function(
            WB_INTERFACE_UNIT,
            "wbGridCell",
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let mut record = RecordInstance::new(def.clone());
                for (index, argument) in args.values.iter().take(2).enumerate() {
                    record.values[index] = xedit_script::values::var_as_type(argument, VarType::Integer)?;
                }
                *value = Value::Record(Rc::new(RefCell::new(record)));
                Ok(())
            }),
            2,
            vec![ParamType::value(VarType::Empty), ParamType::value(VarType::Empty)],
            VarType::Record,
        );
    }

    //=== The constants =======================================================

    for mode in GameMode::ALL {
        adapter.add_const(c_unit, mode_name(mode), Value::Integer(mode as i32));
    }
    for (name, ordinal) in ELEMENT_STATES.iter().chain(DEF_TYPES).chain(CONTAINER_STATES) {
        adapter.add_const(c_unit, name, Value::Integer(*ordinal));
    }
    for (name, ordinal) in ASSET_TYPES.iter().chain(RESOURCE_TYPES) {
        adapter.add_const(c_unit, name, Value::Integer(*ordinal));
    }

    //=== The Dummy unit's functions ==========================================

    // `_Assigned`: `Assigned(V2O(Args.Values[0]))`.
    adapter.add_function(
        c_unit,
        "Assigned",
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Bool(lookup_assigned(&args.values[0]));
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Empty,
    );

    // `ObjectToElement`: `IwbElement(Pointer(V2O(...)))` under a swallowed
    // `except`. An element value is handed back unchanged; anything else
    // leaves the result unassigned (upstream's cast of a foreign object is
    // undefined, so the port does not fabricate one).
    adapter.add_function(
        c_unit,
        "ObjectToElement",
        Rc::new(|value: &mut Value, args: &mut Args| {
            if crate::script::ScriptRunState::element_of(&args.values[0]).is_some() {
                *value = args.values[0].clone();
            } else {
                *value = Value::Empty;
            }
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Empty,
    );

    // `_FullPathToFilename`.
    adapter.add_function(
        c_unit,
        "FullPathToFilename",
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Str(full_path_to_filename(&text_arg(&args.values[0])?).into_bytes());
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Empty,
    );

    // `EnableSkyrimSaveFormat`: `wbTestWrite := True; wbForceNewHeader :=
    // True`.
    adapter.add_function(
        c_unit,
        "EnableSkyrimSaveFormat",
        Rc::new(|_value: &mut Value, _args: &mut Args| {
            globals::set_test_write(true);
            globals::set_force_new_header(true);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );

    // `GetRecordDefNames`: `DefaultSignature + ' - ' + GetName` for every
    // record definition of the loaded game definitions.
    adapter.add_function(
        c_unit,
        "GetRecordDefNames",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let Some(list) = crate::script_strings::list_of(&args.values[0]) else {
                return Ok(());
            };
            let mut list = borrow_list_mut(&list);
            for (_, def) in record_defs() {
                let name = format!(
                    "{} - {}",
                    String::from_utf8_lossy(&def.get_default_signature().0),
                    def.get_name()
                );
                list.add(name)?;
            }
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Empty,
    );

    // `wbFilterStrings(slIn, slOut, Filter)`: `Pos(Filter,
    // Lowercase(slIn[i])) > 0` over the first list's lines.
    adapter.add_function(
        c_unit,
        "wbFilterStrings",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let input = list_of_or_error(&args.values[0])?;
            let output = list_of_or_error(&args.values[1])?;
            let filter = lower_case(&text_arg(&args.values[2])?);
            let lines = {
                let list = crate::script_strings::borrow_list(&input);
                (0..list.count())
                    .map(|index| list.string_at(index as i64))
                    .collect::<Vec<_>>()
            };
            let mut output = borrow_list_mut(&output);
            for line in lines {
                if lower_case(&line).contains(&filter) {
                    output.add(line)?;
                }
            }
            Ok(())
        }),
        3,
        vec![
            ParamType::value(VarType::Empty),
            ParamType::value(VarType::Empty),
            ParamType::value(VarType::Empty),
        ],
        VarType::Empty,
    );

    // `wbRemoveDuplicateStrings(sl)`: from the end, `sl.Find(sl[i], j)`
    // and a `Delete(i)` when `j <> i`. `Find` always sets its index (the
    // insertion point when the string is not there), so the upstream
    // `j <> -1` test never fires; on an unsorted list the binary search
    // still runs (`TStringList.Find` has no unsorted fallback).
    adapter.add_function(
        c_unit,
        "wbRemoveDuplicateStrings",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let Some(list) = crate::script_strings::list_of(&args.values[0]) else {
                return Ok(());
            };
            let mut list = borrow_list_mut(&list);
            for index in (0..list.count()).rev() {
                let text = list.string_at(index as i64);
                let (_, found_at) = list.find(&text);
                if found_at != index {
                    list.delete(index as i64)?;
                }
            }
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Empty,
    );

    // `wbGetVersionNumber`: `VersionString.ToCardinal`.
    adapter.add_function(
        c_unit,
        "wbVersionNumber",
        Rc::new(|value: &mut Value, _args: &mut Args| {
            *value = Value::Int64(i64::from(version_to_cardinal()));
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );

    let _ = classes;
}

/// `Assigned(V2O(Value))`: whether the value holds an object.
/// `Assigned(V)` reads the variant's pointer field: anything whose payload
/// is not zero is assigned -- an object, a non-empty string, a non-zero
/// number (`Assigned(5)` and `Assigned('x')` are `True`, probes `x-219`,
/// `x-220`); `nil`, `Null`, `Unassigned`, 0 and `''` are not.
fn lookup_assigned(value: &Value) -> bool {
    match value {
        Value::Empty | Value::Null | Value::Pointer(0) => false,
        Value::Str(bytes) => !bytes.is_empty(),
        Value::Double(v) | Value::Date(v) => v.to_bits() != 0,
        Value::Bool(v) => *v,
        other => other.raw().is_some_and(|raw| raw != 0) || other.is_object() || other.is_record() || other.is_array(),
    }
}

/// `GetEnumName` of `TwbGameMode` (`gm` + the name), which upstream's
/// `AddConst` writes by hand.
fn mode_name(mode: GameMode) -> &'static str {
    match mode {
        GameMode::gmTES3 => "gmTES3",
        GameMode::gmTES4 => "gmTES4",
        GameMode::gmTES4R => "gmTES4R",
        GameMode::gmFO3 => "gmFO3",
        GameMode::gmFNV => "gmFNV",
        GameMode::gmTES5 => "gmTES5",
        GameMode::gmEnderal => "gmEnderal",
        GameMode::gmFO4 => "gmFO4",
        GameMode::gmSSE => "gmSSE",
        GameMode::gmTES5VR => "gmTES5VR",
        GameMode::gmEnderalSE => "gmEnderalSE",
        GameMode::gmFO4VR => "gmFO4VR",
        GameMode::gmFO76 => "gmFO76",
        GameMode::gmSF1 => "gmSF1",
    }
}
