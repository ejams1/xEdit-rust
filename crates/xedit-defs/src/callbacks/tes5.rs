// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsTES5.pas

//! The callbacks of `wbDefinitionsTES5.pas` that are ported by hand. The
//! ones that are not ported yet are stubs in `tes5_stubs.rs`.

pub use super::tes5_stubs::*;

use xedit_core::interface::globals::more_info_for_decider;
use xedit_core::interface::misc::progress;
use xedit_core::interface::*;

use super::common::wb_try_get_container_from_union;
use crate::signatures::{ANAM, NAME, PRKE};

/// Upstream `CombineVarRecs`.
pub fn combine_var_recs(a: &[VarRec], b: &[VarRec]) -> Vec<VarRec> {
    a.iter().chain(b).cloned().collect()
}

/// Upstream `MakeVarRecs`.
pub fn make_var_recs(a: &[VarRec]) -> Vec<VarRec> {
    a.to_vec()
}

/// Upstream `CmpW32`: -1, 0 or 1 for two unsigned values.
pub fn cmp_w32(a: u32, b: u32) -> i32 {
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// `Container.ElementNativeValues[aPath]` as an integer, 0 when missing.
fn container_int(container: &ElementRef, path: &str) -> i64 {
    container
        .as_container()
        .map_or(0, |container| match container.get_element_native_value(path) {
            Variant::Float(float) => float.round() as i64,
            Variant::Str(text) => text.trim().parse().unwrap_or(0),
            other => other.as_ordinal().unwrap_or(0),
        })
}

/// Upstream `wbTypeDecider`: the `Type` value of the container.
pub fn wb_type_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    match container
        .as_container()
        .and_then(|container| container.get_element_by_name("Type"))
    {
        Some(element) => element.get_native_value().as_ordinal().unwrap_or(0) as i32,
        None => {
            if more_info_for_decider() {
                progress(&format!(
                    "\"{}\" does not contain an element named Type",
                    container.get_name()
                ));
            }
            0
        }
    }
}

/// Upstream `wbScriptPropertyDecider`.
pub fn wb_script_property_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    match container_int(&container, "Type") {
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 5,
        11 => 6,
        12 => 7,
        13 => 8,
        14 => 9,
        15 => 10,
        _ => 0,
    }
}

/// Upstream `wbPerkDATADecider`: the `Type` of the `PRKE` subrecord.
pub fn wb_perk_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(prke) = a_element
        .and_then(|element| element.get_container())
        .and_then(|container| container.as_container()?.get_record_by_signature(PRKE))
    else {
        return 0;
    };
    let Some(kind) = prke.as_container().and_then(|prke| prke.get_element_by_name("Type")) else {
        return 0;
    };
    kind.get_native_value().as_ordinal().unwrap_or(0) as i32
}

/// Upstream `wbEPFDDecider`: the `EPFT` value, or 8 for the functions that
/// take an actor value.
pub fn wb_epfd_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = a_element.and_then(|element| element.get_container()) else {
        return 0;
    };
    if container.as_container().is_none() {
        return 0;
    }
    let mut result = container_int(&container, "EPFT") as i32;
    if result == 2
        && matches!(
            container_int(&container, "..\\DATA\\Entry Point\\Function"),
            5 | 12 | 13 | 14
        )
    {
        result = 8;
    }
    result
}

/// Upstream `wbBOOKTeachesDecider`: 1 for a skill book, 2 for a spell book.
///
/// UPSTREAM-QUIRK: upstream also renames the union definition to `Skill` or
/// `Spell` as a side effect; definition names are immutable here.
pub fn wb_book_teaches_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let flags = container_int(&container, "Flags");
    if flags & 0x1 != 0 {
        1
    } else if flags & 0x4 != 0 {
        2
    } else {
        0
    }
}

/// Upstream `wbMGEFAssocItemDecider`: the member for the `Archtype`, read
/// from the element, or from the data at offset 56 of a proper structure.
pub fn wb_mgef_assoc_item_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    const OFFSET_ARCHTYPE: usize = 56;
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let archtype = match container
        .as_container()
        .and_then(|container| container.get_element_by_name("Archtype"))
    {
        Some(element) => element.get_native_value().as_ordinal(),
        None => match (container.as_data_container(), a_base_ptr) {
            (Some(_), Some(data)) if data.len() >= OFFSET_ARCHTYPE + 4 => Some(i64::from(u32::from_le_bytes(
                data[OFFSET_ARCHTYPE..OFFSET_ARCHTYPE + 4].try_into().unwrap(),
            ))),
            _ => None,
        },
    };
    match archtype {
        Some(12) => 1, // Light
        Some(17) => 2, // Bound Item
        Some(18) => 3, // Summon Creature
        Some(25) => 4, // Guide
        Some(34) => 8, // Peak Mod
        Some(35) => 5, // Cloak
        Some(36) => 6, // Werewolf
        Some(39) => 7, // Enhance Weapon
        Some(40) => 4, // Spawn Hazard
        Some(46) => 6, // Vampire Lord
        _ => 0,
    }
}

/// Upstream `wbPubPackCNAMDecider`: the member for the `ANAM` type name.
pub fn wb_pub_pack_cnam_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(anam) = a_element
        .and_then(|element| element.get_container())
        .and_then(|container| container.as_container()?.get_record_by_signature(ANAM))
    else {
        return 0;
    };
    let ctype = match anam.get_native_value() {
        Variant::Str(text) => text,
        _ => String::new(),
    };
    match ctype.as_str() {
        "Bool" => 1,
        "Int" => 2,
        "Float" | "ObjectList" => 3,
        _ => 0,
    }
}

/// Upstream `wbREFRRecordFlagsDecider`: the member for the signature of the
/// base record of the reference.
pub fn wb_refr_record_flags_decider(a_element: ElementArg) -> i32 {
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return 0;
    };
    let Some(name) = main_record.get_element_by_signature(NAME) else {
        return 0;
    };
    let Some(base) = name.get_links_to().and_then(|links_to| links_to.into_main_record()) else {
        return 0;
    };
    match base.get_signature().0.as_slice() {
        b"ACTI" => 1,
        b"ADDN" | b"ARTO" | b"ASPC" | b"FLOR" | b"FURN" | b"IDLM" | b"SOUN" | b"TACT" | b"TXST" => 2,
        b"ALCH" | b"AMMO" | b"APPA" | b"ARMO" | b"BOOK" | b"INGR" | b"KEYM" | b"MISC" | b"SCRL" | b"SLGM" | b"WEAP" => {
            3
        }
        b"CONT" => 4,
        b"DOOR" => 5,
        b"LIGH" => 6,
        b"MSTT" => 7,
        b"STAT" => 8,
        b"TREE" => 9,
        _ => 0,
    }
}
