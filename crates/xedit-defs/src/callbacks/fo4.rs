// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsFO4.pas

//! The callbacks of `wbDefinitionsFO4.pas` that are ported by hand. The
//! ones that are not ported yet are stubs in `fo4_stubs.rs`.

// The stubs of the callbacks not ported yet; empty once every callback is ported.
#[allow(unused_imports)]
pub use super::fo4_stubs::*;

use std::sync::{Arc, Mutex};

use xedit_core::delphi::{float_to_str_f_fixed, path_file_name, str_to_float};
use xedit_core::interface::globals::more_info_for_decider;
use xedit_core::interface::misc::{int_to_hex64, progress, str_to_int_def};
use xedit_core::interface::string::to_comma_text;
use xedit_core::interface::*;

use super::common::{
    collision_layer_links_to, index_key_from_ordinal, variant_int, wb_try_get_container_from_union,
    wb_try_get_container_ref_from_union_or_value, wb_try_get_containing_main_record, wb_try_get_main_record,
};
use crate::common::{wb_idx_addon_node, wb_idx_collision_layer};
use crate::fo4::{
    TConditionParameterType, WB_ACTOR_PROPERTY_ENUM, WB_ARMOR_PROPERTY_ENUM, WB_CONDITION_FUNCTIONS,
    WB_EVENT_FUNCTION_ENUM, WB_EVENT_MEMBER_ENUM, WB_WEAPON_PROPERTY_ENUM, wb_condition_desc_from_index,
};
use crate::signatures::{ANAM, NAME, PRKE, QUST};

/// `Container.ElementNativeValues[aPath]` as an integer, 0 when missing.
fn container_int(container: &ElementRef, path: &str) -> i64 {
    container
        .as_container()
        .map_or(0, |container| variant_int(&container.get_element_native_value(path)))
}

/// Upstream `CombineVarRecs`.
pub fn combine_var_recs(a: &[VarRec], b: &[VarRec]) -> Vec<VarRec> {
    a.iter().chain(b).cloned().collect()
}

/// Upstream `MakeVarRecs`.
pub fn make_var_recs(a: &[VarRec]) -> Vec<VarRec> {
    a.to_vec()
}

/// Upstream `GetObjectModPropertyEnum`. Not ported yet: it needs the
/// element tree.
pub fn get_object_mod_property_enum(_a_element: Option<Arc<dyn Element>>) -> Option<Arc<EnumDef>> {
    todo!("port GetObjectModPropertyEnum from wbDefinitionsFO4.pas line 2392")
}

/// Upstream `CmpW32`: -1, 0 or 1 for two unsigned values.
pub fn cmp_w32(a: u32, b: u32) -> i32 {
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Upstream anonymous routine at line 7895 of `wbDefinitionsFO4.pas`: the
/// `ADDN` index key.
pub fn define_fo4_anonymous_7895(a_main_record: &MainRecordRef, a_index_keys: &mut IndexKeys) {
    index_key_from_ordinal(a_main_record, a_index_keys, "DATA", wb_idx_addon_node());
}

/// Upstream anonymous routine at line 9638 of `wbDefinitionsFO4.pas`: the
/// `COLL` index key.
pub fn define_fo4_anonymous_9638(a_main_record: &MainRecordRef, a_index_keys: &mut IndexKeys) {
    index_key_from_ordinal(a_main_record, a_index_keys, "BNAM", wb_idx_collision_layer());
}

/// Upstream anonymous routine at line 11711 of `wbDefinitionsFO4.pas`: the
/// collision layer of `XTRI`.
pub fn define_fo4_anonymous_11711(a_element: ElementArg) -> Option<ElementRef> {
    collision_layer_links_to(a_element)
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
        kind @ 1..=7 => kind as i32,
        11 => 8,
        12 => 9,
        13 => 10,
        14 => 11,
        15 => 12,
        16 => 13,
        17 => 14,
        _ => 0,
    }
}

/// Upstream `wbBOOKTeachesDecider`: 1 for a skill, 2 for a spell, 3 for a perk.
pub fn wb_book_teaches_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let flags = container_int(&container, "Flags");
    if flags & 0x01 != 0 {
        1
    } else if flags & 0x04 != 0 {
        2
    } else if flags & 0x10 != 0 {
        3
    } else {
        0
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

/// Upstream `wbMGEFAssocItemDecider`: the member for the `Archetype`.
pub fn wb_mgef_assoc_item_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    const OFFSET_ARCHTYPE: usize = 56;
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let archtype = match container
        .as_container()
        .and_then(|container| container.get_element_by_name("Archetype"))
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
        Some(45) => 9, // Damage Type
        Some(46) => 9, // Immunity
        _ => 0,
    }
}

/// Upstream `wbREFRRecordFlagsDecider`: the member for the signature of
/// the base record of the reference.
pub fn wb_refr_record_flags_decider(a_element: ElementArg) -> i32 {
    let Some(main_record) = wb_try_get_containing_main_record(a_element) else {
        return 0;
    };
    let name = main_record.get_element_by_signature(NAME);
    let Some(base) = wb_try_get_main_record(name.as_ref(), "") else {
        return 0;
    };
    match base.get_signature().0.as_slice() {
        b"ACTI" | b"STAT" | b"SCOL" | b"TREE" => 1,
        b"CONT" | b"TERM" => 2,
        b"DOOR" => 3,
        b"LIGH" => 4,
        b"MSTT" => 5,
        b"ADDN" => 6,
        b"SCRL" | b"AMMO" | b"ARMO" | b"BOOK" | b"INGR" | b"KEYM" | b"MISC" | b"FURN" | b"WEAP" | b"ALCH" => 7,
        _ => 0,
    }
}

/// Upstream `wbINFOGroupDecider`: 1 for a record with flag `$40`.
pub fn wb_info_group_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match a_element.and_then(|element| element.get_containing_main_record()) {
        Some(main_record) if main_record.get_flags().0 & 0x40 != 0 => 1,
        _ => 0,
    }
}

/// Upstream `wbAECHDataDecider`: the member for the `KNAM` type of the effect chain.
pub fn wb_aech_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let Some(container) = container.get_container() else {
        return 0;
    };
    let Some(knam) = container
        .as_container()
        .and_then(|container| container.get_element_by_signature(Signature::new(b"KNAM")))
    else {
        return 0;
    };
    match knam.get_edit_value().as_str() {
        "BSOverdrive" => 0,
        "BSStateVariableFilter" => 1,
        "BSDelayEffect" => 2,
        _ => 0,
    }
}

/// Upstream `wbSNDRDataDecider`: 1 for an auto weapon sound descriptor.
pub fn wb_sndr_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let Some(container) = container.get_container() else {
        return 0;
    };
    let Some(cnam) = container
        .as_container()
        .and_then(|container| container.get_element_by_signature(Signature::new(b"CNAM")))
    else {
        return 0;
    };
    if cnam.get_edit_value() == "AutoWeapon" { 1 } else { 0 }
}

/// Upstream `wbOMODDataFunctionTypeDecider`.
pub fn wb_omod_data_function_type_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    match container_int(&container, "Value Type") {
        2 => 1,
        4 => 3,
        5 => 2,
        _ => 0,
    }
}

/// Upstream `wbOMODDataPropertyValue1Decider`.
pub fn wb_omod_data_property_value1_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    match container_int(&container, "Value Type") {
        0 => 1,
        1 => 2,
        2 => 3,
        4 | 6 => 4,
        5 => {
            let property = container
                .as_container()
                .map(|container| container.get_element_edit_value("Property"))
                .unwrap_or_default();
            match property.as_str() {
                "SoundLevel" => 6,
                "StaggerValue" => 7,
                "HitBehaviour" => 8,
                _ => 5,
            }
        }
        _ => 0,
    }
}

/// Upstream `wbOMODDataPropertyValue2Decider`.
pub fn wb_omod_data_property_value2_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    match container_int(&container, "Value Type") {
        0 | 4 => 1,
        1 | 6 => 2,
        2 => 3,
        _ => 0,
    }
}

/// Upstream `wbCELLCombinedRefsCounter`: half the count, which counts each
/// member of the struct.
pub fn wb_cell_combined_refs_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    match a_element.and_then(|element| element.get_container()) {
        Some(container) if container.as_container().is_some() => {
            (container_int(&container, "References Count") / 2) as u32
        }
        _ => 0,
    }
}

/// Upstream `wbConditionFunctionToStr`: the name of the condition function.
pub fn wb_condition_function_to_str(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    let desc = wb_condition_desc_from_index(a_int as i32);
    let mut result = match a_type {
        CallbackType::ctEditType => "ComboBox".to_owned(),
        CallbackType::ctToSortKey => int_to_hex64(a_int, 8),
        CallbackType::ctCheck if desc.is_none() => format!("<Unknown: {a_int}>"),
        CallbackType::ctEditInfo => {
            let mut names: Vec<String> = WB_CONDITION_FUNCTIONS
                .iter()
                .map(|function| function.name.to_owned())
                .collect();
            names.sort_by_key(|name| name.to_lowercase());
            to_comma_text(&names)
        }
        _ => String::new(),
    };
    match desc {
        Some(desc) => {
            if matches!(
                a_type,
                CallbackType::ctToEditValue | CallbackType::ctToStr | CallbackType::ctToSummary
            ) {
                result = desc.name.to_owned();
            }
        }
        None => match a_type {
            CallbackType::ctToEditValue | CallbackType::ctToSummary => result = a_int.to_string(),
            CallbackType::ctToStr => result = format!("<Unknown: {a_int}>"),
            _ => {}
        },
    }
    result
}

/// Upstream `wbConditionFunctionToInt`.
pub fn wb_condition_function_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    for function in WB_CONDITION_FUNCTIONS {
        if function.name.eq_ignore_ascii_case(a_string) {
            return i64::from(function.index);
        }
    }
    a_string.trim().parse().unwrap_or(0)
}

/// Upstream `wbConditionParam1Decider`.
pub fn wb_condition_param1_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let Some(desc) = wb_condition_desc_from_index(container_int(&container, "Function") as i32) else {
        return 0;
    };
    let mut param_type = desc.param_type1;
    let param_flag = container_int(&container, "Type");
    if matches!(
        param_type,
        TConditionParameterType::ptReference | TConditionParameterType::ptActor | TConditionParameterType::ptPackage
    ) {
        if param_flag & 0x02 > 0 {
            // Except for this function when Run On = Quest Alias: then the
            // alias is parameter 3 and the package is parameter 1.
            if !(container_int(&container, "Run On") == 5 && desc.name == "GetIsCurrentPackage") {
                param_type = TConditionParameterType::ptAlias;
            }
        } else if param_flag & 0x08 > 0 {
            param_type = TConditionParameterType::ptPackdata;
        }
    }
    param_type as i32 + 1
}

/// Upstream `wbConditionParam2Decider`.
pub fn wb_condition_param2_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let Some(desc) = wb_condition_desc_from_index(container_int(&container, "Function") as i32) else {
        return 0;
    };
    let mut param_type = desc.param_type2;
    let param_flag = container_int(&container, "Type");
    if matches!(
        param_type,
        TConditionParameterType::ptReference | TConditionParameterType::ptActor | TConditionParameterType::ptPackage
    ) {
        if param_flag & 0x02 > 0 {
            param_type = TConditionParameterType::ptAlias;
        } else if param_flag & 0x08 > 0 {
            param_type = TConditionParameterType::ptPackdata;
        }
    }
    param_type as i32 + 1
}

/// Upstream `wbConditionEventToStr`: the event function and member.
pub fn wb_condition_event_to_str(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    let event_function = a_int & 0xFFFF;
    let event_member = a_int >> 16;
    let (Some(function_enum), Some(member_enum)) = (WB_EVENT_FUNCTION_ENUM.get(), WB_EVENT_MEMBER_ENUM.get()) else {
        return String::new();
    };
    match a_type {
        CallbackType::ctEditType => "ComboBox".to_owned(),
        CallbackType::ctToSortKey => int_to_hex64(a_int, 8),
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => format!(
            "{}:{}",
            function_enum.to_edit_value(event_function, None),
            member_enum.to_edit_value(event_member, None)
        ),
        CallbackType::ctCheck => {
            let mut s1 = function_enum.check(event_function, None);
            if !s1.is_empty() {
                s1 = format!("EventFunction{s1}");
            }
            let mut s2 = member_enum.check(event_member, None);
            if !s2.is_empty() {
                s2 = format!("EventMember{s2}");
            }
            if !s1.is_empty() || !s2.is_empty() {
                format!("{s1}:{s2}")
            } else {
                String::new()
            }
        }
        CallbackType::ctEditInfo => {
            let members = member_enum.get_edit_info(None);
            let mut names = Vec::new();
            for index in 0..function_enum.get_name_count() {
                for member in &members {
                    names.push(format!("{}:{member}", function_enum.get_name_of(i64::from(index))));
                }
            }
            names.sort_by_key(|name| name.to_lowercase());
            to_comma_text(&names)
        }
        _ => String::new(),
    }
}

/// The winning quest override except for partial forms, as the quest stage
/// callbacks pick it.
fn quest_override(main_record: MainRecordRef) -> MainRecordRef {
    let winning = main_record.get_winning_override();
    if winning.get_flags().0 & 0x0000_4000 == 0 {
        winning
    } else if main_record.get_flags().0 & 0x0000_4000 != 0 {
        main_record.get_master_or_self()
    } else {
        main_record
    }
}

/// The stage text lookup shared by the quest stage callbacks: the result for
/// the stage `a_int` of the quest, or `None` when it is not found.
fn quest_stage_text(main_record: &MainRecordRef, a_int: i64, a_type: CallbackType) -> Result<String, Vec<String>> {
    let mut edit_infos = Vec::new();
    if let Some(stages) = main_record.get_element_by_name("Stages")
        && let Some(stages) = stages.as_container()
    {
        for index in 0..stages.get_element_count() {
            let Some(stage) = stages.get_element(index) else {
                continue;
            };
            let Some(stage) = stage.as_container() else { continue };
            let j = variant_int(&stage.get_element_native_value("INDX\\Stage Index"));
            let s = stage
                .get_element_by_path("Log Entries\\Log Entry\\CNAM")
                .map(|entry| entry.get_value())
                .unwrap_or_default();
            let s = s.trim();
            let mut t = format!("{j:0>3}");
            if !s.is_empty() {
                t = format!("{t} {s}");
            }
            if a_type == CallbackType::ctEditInfo {
                edit_infos.push(t);
            } else if j == a_int {
                return Ok(match a_type {
                    CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => t,
                    _ => String::new(),
                });
            }
        }
    }
    Err(edit_infos)
}

/// Upstream `wbConditionQuestStageToStr`.
pub fn wb_condition_quest_stage_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let mut result = match a_type {
        CallbackType::ctToSortKey => return int_to_hex64(a_int, 8),
        CallbackType::ctToEditValue | CallbackType::ctToSummary => a_int.to_string(),
        CallbackType::ctCheck => "<Warning: Could not resolve Parameter 1>".to_owned(),
        CallbackType::ctToStr => format!("{a_int} <Warning: Could not resolve Parameter 1>"),
        _ => String::new(),
    };
    let Some(container) = wb_try_get_container_ref_from_union_or_value(a_element) else {
        return result;
    };
    let parameter = container
        .as_container()
        .and_then(|container| container.get_element_by_name("Parameter #1"));
    let Some(main_record) = wb_try_get_main_record(parameter.as_ref(), "") else {
        return result;
    };
    let main_record = quest_override(main_record);
    if main_record.get_signature() != QUST {
        match a_type {
            CallbackType::ctCheck => {
                result = format!("<Warning: \"{}\" is not a Quest record>", main_record.get_short_name());
            }
            CallbackType::ctToStr => {
                result = format!(
                    "{a_int} <Warning: \"{}\" is not a Quest record>",
                    main_record.get_short_name()
                );
            }
            _ => {}
        }
        return result;
    }
    if a_type == CallbackType::ctEditType {
        return "ComboBox".to_owned();
    }
    match quest_stage_text(&main_record, a_int, a_type) {
        Ok(text) => text,
        Err(mut edit_infos) => match a_type {
            CallbackType::ctCheck => {
                format!(
                    "<Warning: Quest Stage [{a_int}] not found in \"{}\">",
                    main_record.get_name()
                )
            }
            CallbackType::ctToStr => {
                format!(
                    "{a_int} <Warning: Quest Stage [{a_int}] not found in \"{}\">",
                    main_record.get_name()
                )
            }
            CallbackType::ctEditInfo => {
                edit_infos.sort_by_key(|text| text.to_lowercase());
                to_comma_text(&edit_infos)
            }
            _ => result,
        },
    }
}

/// Upstream `wbPerkDATAQuestStageToStr`.
pub fn wb_perk_data_quest_stage_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let mut result = match a_type {
        CallbackType::ctToStr => format!("{a_int} <Warning: Could not resolve Quest>"),
        CallbackType::ctToSummary | CallbackType::ctToEditValue => a_int.to_string(),
        CallbackType::ctToSortKey => return int_to_hex64(a_int, 8),
        CallbackType::ctCheck => "<Warning: Could not resolve Quest>".to_owned(),
        _ => String::new(),
    };
    let Some(container) = wb_try_get_container_ref_from_union_or_value(a_element) else {
        return result;
    };
    let parameter = container
        .as_container()
        .and_then(|container| container.get_element_by_name("Quest"));
    let Some(main_record) = wb_try_get_main_record(parameter.as_ref(), "") else {
        return result;
    };
    let main_record = quest_override(main_record);
    if main_record.get_signature() != QUST {
        match a_type {
            CallbackType::ctToStr => {
                result = format!(
                    "{a_int} <Warning: \"{}\" is not a Quest record>",
                    main_record.get_short_name()
                );
            }
            CallbackType::ctToSummary => result = a_int.to_string(),
            CallbackType::ctCheck => {
                result = format!("<Warning: \"{}\" is not a Quest record>", main_record.get_short_name());
            }
            _ => {}
        }
        return result;
    }
    if a_type == CallbackType::ctEditType {
        return "ComboBox".to_owned();
    }
    match quest_stage_text(&main_record, a_int, a_type) {
        Ok(text) => text,
        Err(mut edit_infos) => match a_type {
            CallbackType::ctToStr => {
                format!(
                    "{a_int} <Warning: Quest Stage [{a_int}] not found in \"{}\">",
                    main_record.get_name()
                )
            }
            CallbackType::ctToSummary => a_int.to_string(),
            CallbackType::ctCheck => {
                format!(
                    "<Warning: Quest Stage [{a_int}] not found in \"{}\">",
                    main_record.get_name()
                )
            }
            CallbackType::ctEditInfo => {
                edit_infos.sort_by_key(|text| text.to_lowercase());
                to_comma_text(&edit_infos)
            }
            _ => result,
        },
    }
}

/// Upstream `wbStringToInt`.
pub fn wb_string_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    i64::from(str_to_int_def(a_string, 0))
}

/// Upstream `wbIntToHexStr`.
pub fn wb_int_to_hex_str(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToSortKey | CallbackType::ctToEditValue => {
            int_to_hex64(a_int, 8)
        }
        _ => String::new(),
    }
}

/// Upstream `wbHexStrToInt`: the hexadecimal number before a space or colon.
pub fn wb_hex_str_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    let end = a_string.find(' ').or_else(|| a_string.find(':'));
    let text = match end {
        Some(end) => &a_string[..end],
        None => a_string,
    };
    i64::from_str_radix(text.trim(), 16).unwrap_or(0)
}

/// Upstream `wbCLFMColorToStr`: a float when the `FNAM` flag is set, the
/// `rgba(...)` bytes otherwise.
pub fn wb_clfm_color_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let flags = a_element
        .and_then(|element| element.get_container())
        .and_then(|container| {
            container
                .as_container()?
                .get_element_by_signature(Signature::new(b"FNAM"))
        })
        .map_or(0, |fnam| variant_int(&fnam.get_native_value()));
    let text = if flags & 2 != 0 {
        float_to_str_f_fixed(f64::from(f32::from_bits(a_int as u32)), 6)
    } else {
        format!(
            "rgba({}, {}, {}, {})",
            a_int & 0xFF,
            (a_int >> 8) & 0xFF,
            (a_int >> 16) & 0xFF,
            (a_int >> 24) & 0xFF
        )
    };
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => text,
        CallbackType::ctToSortKey => int_to_hex64(a_int, 8),
        _ => String::new(),
    }
}

/// Upstream `wbCLFMColorToInt`.
pub fn wb_clfm_color_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    if a_string.len() >= 5 && a_string[..5].eq_ignore_ascii_case("rgba(") {
        let end = a_string.find(')').unwrap_or(a_string.len());
        let parts: Vec<&str> = a_string[5..end].split(',').collect();
        if parts.len() != 4 {
            return 0;
        }
        let mut result: i64 = 0;
        for (index, part) in parts.iter().enumerate() {
            let byte = str_to_int_def(part, 0) as u8;
            result |= i64::from(byte) << (8 * index);
        }
        return result;
    }
    let value = str_to_float(a_string).unwrap_or(0.0) as f32;
    i64::from(value.to_bits() as i32)
}

/// Upstream `GetObjectModPropertyEnum`: the property enumeration for the
/// form type of the object modification.
fn object_mod_property_enum(a_element: ElementArg) -> Option<Arc<EnumDef>> {
    let main_record = a_element?.get_containing_main_record()?;
    let mut signature = main_record.get_signature();
    if signature == Signature::new(b"OMOD")
        && let Some(data) = main_record.get_element_by_signature(Signature::new(b"DATA"))
        && let Some(data) = data.as_container()
    {
        let form_type = variant_int(&data.get_element_native_value("Form Type")) as u32;
        signature = Signature(form_type.to_le_bytes());
    }
    match signature.0.as_slice() {
        b"ARMO" => WB_ARMOR_PROPERTY_ENUM.get(),
        b"WEAP" => WB_WEAPON_PROPERTY_ENUM.get(),
        b"NPC_" => WB_ACTOR_PROPERTY_ENUM.get(),
        _ => None,
    }
}

/// Upstream `wbObjectModPropertyToStr`.
pub fn wb_object_mod_property_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let Some(prop_enum) = object_mod_property_enum(a_element) else {
        return match a_type {
            CallbackType::ctToStr
            | CallbackType::ctToSummary
            | CallbackType::ctToSortKey
            | CallbackType::ctToEditValue => a_int.to_string(),
            _ => String::new(),
        };
    };
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            prop_enum.to_string(a_int, a_element, a_type == CallbackType::ctToSummary)
        }
        CallbackType::ctToSortKey => prop_enum.to_sort_key(a_int, a_element),
        CallbackType::ctCheck => prop_enum.check(a_int, a_element),
        CallbackType::ctToEditValue => prop_enum.to_edit_value(a_int, a_element),
        CallbackType::ctEditType => "ComboBox".to_owned(),
        CallbackType::ctEditInfo => to_comma_text(&prop_enum.get_edit_info(a_element)),
        _ => String::new(),
    }
}

/// Upstream `wbObjectModPropertyToInt`.
pub fn wb_object_mod_property_to_int(a_string: &str, a_element: ElementArg) -> i64 {
    match object_mod_property_enum(a_element) {
        Some(prop_enum) => prop_enum
            .find_name(a_string)
            .unwrap_or_else(|| i64::from(str_to_int_def(a_string, 0))),
        None => i64::from(str_to_int_def(a_string, 0)),
    }
}

/// Upstream `wbCombinedMeshIDToStr`: the precombined mesh file of the cell.
pub fn wb_combined_mesh_id_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let result = int_to_hex64(a_int, 8);
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => {
            let Some(cell) = a_element.and_then(|element| element.get_containing_main_record()) else {
                return result;
            };
            let cell = cell.get_master_or_self();
            let master_folder = match cell.get_file() {
                Some(file) if file.get_load_order() > 0 => format!("{}\\", file.get_name()),
                _ => String::new(),
            };
            format!(
                "Precombined\\{master_folder}{}_{result}_OC.nif",
                cell.get_form_id()
                    .change_file_id(FileID::create_full(0))
                    .to_string(false)
            )
        }
        CallbackType::ctCheck => String::new(),
        _ => result,
    }
}

/// Upstream `wbCombinedMeshIDToInt`: the hexadecimal number between the
/// first two underscores of the file name.
pub fn wb_combined_mesh_id_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    let file_name = path_file_name(a_string);
    let Some((_, rest)) = file_name.split_once('_') else {
        return 0;
    };
    let Some((hex, _)) = rest.split_once('_') else { return 0 };
    if hex.len() != 8 {
        return 0;
    }
    i64::from_str_radix(hex, 16).unwrap_or(0)
}

/// Upstream `wbConditionQuestOverlay`: a null quest stands for the quest the
/// condition belongs to.
pub fn wb_condition_quest_overlay(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> i64 {
    if a_int != 0
        || !matches!(
            a_type,
            CallbackType::ctCheck
                | CallbackType::ctLinksTo
                | CallbackType::ctToSortKey
                | CallbackType::ctToStr
                | CallbackType::ctToSummary
        )
    {
        return a_int;
    }
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return a_int;
    };
    let native = |signature: &[u8; 4]| {
        main_record
            .get_element_by_signature(Signature::new(signature))
            .map_or(a_int, |element| variant_int(&element.get_native_value()))
    };
    match main_record.get_signature().0.as_slice() {
        b"QUST" => i64::from(main_record.get_fixed_form_id().to_cardinal()),
        b"SCEN" => native(b"PNAM"),
        b"PACK" => native(b"QNAM"),
        b"INFO" => {
            // The DIAL of the INFO.
            let Some(group) = main_record
                .get_container()
                .and_then(|container| container.as_element_impl()?.group_record_impl())
            else {
                return a_int;
            };
            match group.children_of() {
                Some(dial) if dial.get_signature() == Signature::new(b"DIAL") => dial
                    .get_element_by_signature(Signature::new(b"QNAM"))
                    .map_or(a_int, |element| variant_int(&element.get_native_value())),
                _ => a_int,
            }
        }
        _ => a_int,
    }
}

/// Upstream `TFaceGenFeature`: the entries of a race feature by race and
/// sex, as the face callbacks cache them.
struct FaceGenFeature {
    race_id: String,
    female: bool,
    entries: Vec<(u32, String)>,
}

/// Upstream `FaceMorphs`, `TintLayers` and `MorphValues`.
static FACE_MORPHS: Mutex<Vec<FaceGenFeature>> = Mutex::new(Vec::new());
static TINT_LAYERS: Mutex<Vec<FaceGenFeature>> = Mutex::new(Vec::new());
static MORPH_VALUES: Mutex<Vec<FaceGenFeature>> = Mutex::new(Vec::new());

/// The race of the actor that holds the element (its winning override) and
/// whether the actor is female.
fn actor_race(a_element: ElementArg) -> Option<(MainRecordRef, bool)> {
    let actor = a_element?.get_containing_main_record()?;
    let female = actor.get_element_edit_value("ACBS\\Flags\\Female") == "1";
    let race = actor
        .get_element_by_signature(Signature::new(b"RNAM"))?
        .get_links_to()?
        .into_main_record()?;
    Some((race.get_winning_override(), female))
}

/// The entries of the feature for the race and sex, from the cache or built
/// with `build` for both sexes.
fn cached_entries(
    cache: &Mutex<Vec<FaceGenFeature>>,
    race: &MainRecordRef,
    female: bool,
    build: impl Fn(&MainRecordRef, bool) -> Vec<(u32, String)>,
) -> Option<Vec<(u32, String)>> {
    let race_id = race.get_editor_id();
    let mut cache = cache.lock().unwrap();
    let find = |cache: &Vec<FaceGenFeature>| {
        cache
            .iter()
            .position(|feature| feature.female == female && feature.race_id == race_id)
    };
    let position = match find(&cache) {
        Some(position) => position,
        None => {
            // Cache not found, fill with data from RACE.
            for female2 in [false, true] {
                cache.push(FaceGenFeature {
                    race_id: race_id.clone(),
                    female: female2,
                    entries: build(race, female2),
                });
            }
            find(&cache)?
        }
    };
    Some(cache[position].entries.clone())
}

/// The elements of the container at `name` of the record, as containers.
fn container_entries(record: &MainRecordRef, name: &str) -> Vec<ElementRef> {
    let Some(container) = record.get_element_by_name(name) else {
        return Vec::new();
    };
    let Some(container) = container.as_container() else {
        return Vec::new();
    };
    (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .collect()
}

fn child_entries(entry: &ElementRef, name: &str) -> Vec<ElementRef> {
    let Some(container) = entry.as_container().and_then(|entry| entry.get_element_by_name(name)) else {
        return Vec::new();
    };
    let Some(container) = container.as_container() else {
        return Vec::new();
    };
    (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .collect()
}

fn edit_value(entry: &ElementRef, path: &str) -> String {
    entry
        .as_container()
        .map(|entry| entry.get_element_edit_value(path))
        .unwrap_or_default()
}

fn native_u32(entry: &ElementRef, path: &str) -> u32 {
    entry
        .as_container()
        .map_or(0, |entry| variant_int(&entry.get_element_native_value(path)) as u32)
}

/// The shared result text of the face callbacks.
fn face_feature_text(
    a_int: i64,
    a_type: CallbackType,
    a_element: ElementArg,
    cache: &Mutex<Vec<FaceGenFeature>>,
    build: impl Fn(&MainRecordRef, bool) -> Vec<(u32, String)>,
    hex: bool,
    what: &str,
) -> String {
    let index_text = if hex { int_to_hex64(a_int, 8) } else { a_int.to_string() };
    let mut result = match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => index_text.clone(),
        CallbackType::ctToSortKey => return int_to_hex64(a_int, 8),
        CallbackType::ctCheck => format!("<Warning: Could not resolve {what} index {index_text}>"),
        _ => String::new(),
    };
    let Some((race, female)) = actor_race(a_element) else {
        return result;
    };
    let Some(entries) = cached_entries(cache, &race, female, build) else {
        return result;
    };
    let index = a_int as u32;
    let entry_name = entries
        .iter()
        .find(|(entry_index, _)| *entry_index == index)
        .map(|(_, name)| name.clone())
        .unwrap_or_default();
    // The capitalized form of the kind in the messages.
    let kind = match what {
        "tint layer" => "Tint Layer Index",
        "face morph" => "Face morph index",
        _ => "Morph index",
    };
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            if !entry_name.is_empty() {
                result = format!("{index_text} {entry_name}");
            } else {
                result = index_text.clone();
                if a_type == CallbackType::ctToStr {
                    result.push_str(&format!(" <{kind} [{index_text}] not found in {}>", race.get_name()));
                }
            }
        }
        CallbackType::ctCheck => {
            result = if entry_name.is_empty() {
                format!("<{kind} [{index_text}] not found in {}>", race.get_name())
            } else {
                String::new()
            };
        }
        CallbackType::ctEditType => result = "ComboBox".to_owned(),
        CallbackType::ctEditInfo => {
            result = entries
                .iter()
                .map(|(entry_index, name)| {
                    let index_text = if hex {
                        int_to_hex64(i64::from(*entry_index), 8)
                    } else {
                        entry_index.to_string()
                    };
                    format!("\"{index_text} {name}\"")
                })
                .collect::<Vec<_>>()
                .join(",");
        }
        _ => {}
    }
    result
}

/// Upstream `wbTintLayerToStr`: the tint group and option of the index.
pub fn wb_tint_layer_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    face_feature_text(
        a_int,
        a_type,
        a_element,
        &TINT_LAYERS,
        |race, female| {
            let name = if female {
                "Female Tint Layers"
            } else {
                "Male Tint Layers"
            };
            let mut entries = Vec::new();
            for group in container_entries(race, name) {
                if group.as_container().is_none() {
                    continue;
                }
                for option in child_entries(&group, "Options") {
                    if option.as_container().is_none() {
                        continue;
                    }
                    entries.push((
                        native_u32(&option, "TETI\\Index"),
                        format!("{} - {}", edit_value(&group, "TTGP"), edit_value(&option, "TTGP")),
                    ));
                }
            }
            entries
        },
        false,
        "tint layer",
    )
}

/// Upstream `wbFaceMorphToStr`: the face morph of the index.
pub fn wb_face_morph_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    face_feature_text(
        a_int,
        a_type,
        a_element,
        &FACE_MORPHS,
        |race, female| {
            let name = if female {
                "Female Face Morphs"
            } else {
                "Male Face Morphs"
            };
            let mut entries = Vec::new();
            for entry in container_entries(race, name) {
                if entry.as_container().is_none() {
                    break;
                }
                entries.push((native_u32(&entry, "FMRI"), edit_value(&entry, "FMRN")));
            }
            entries
        },
        true,
        "face morph",
    )
}

/// Upstream `wbMorphValueToStr`: the morph group preset or morph value of
/// the index.
pub fn wb_morph_value_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    face_feature_text(
        a_int,
        a_type,
        a_element,
        &MORPH_VALUES,
        |race, female| {
            let name = if female {
                "Female Morph Groups"
            } else {
                "Male Morph Groups"
            };
            let mut entries = Vec::new();
            // Iterate over the morph groups.
            for group in container_entries(race, name) {
                if group.as_container().is_none() {
                    break;
                }
                let group_name = edit_value(&group, "MPGN");
                // Iterate over the morph group presets.
                for preset in child_entries(&group, "Morph Presets") {
                    if preset.as_container().is_none() {
                        continue;
                    }
                    entries.push((
                        native_u32(&preset, "MPPI"),
                        format!("{group_name} - {}", edit_value(&preset, "MPPN")),
                    ));
                }
            }
            // Append the morph values, the same for both sexes.
            for value in container_entries(race, "Morph Values") {
                if value.as_container().is_none() {
                    continue;
                }
                entries.push((
                    native_u32(&value, "MSID"),
                    format!("{}/{}", edit_value(&value, "MSM0"), edit_value(&value, "MSM1")),
                ));
            }
            entries
        },
        true,
        "morph",
    )
}

// ----- the editing callbacks -----

use super::common::{
    aech_type_after_set, cell_combined_refs_after_set, cell_data_after_set, cell_xclw_get_conflict_priority,
    check_morph_key_order, condition_event_to_int, efit_after_load, flst_edid_after_set, flst_lnam_is_sorted,
    gmst_edid_after_set, lle_after_load, mgef_archtype_after_set, mgef_assoc_item_after_set,
    package_data_input_value_type_after_set, refr_after_load_lock, replace_bodt_with_bod2, set_native,
    with_forced_internal_edit, with_internal_edit,
};

/// Upstream `wbConditionEventToInt`.
pub fn wb_condition_event_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    condition_event_to_int(a_string, WB_EVENT_FUNCTION_ENUM.get(), WB_EVENT_MEMBER_ENUM.get())
}

/// Upstream `wbGMSTEDIDAfterSet`.
pub fn wb_gmstedid_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    gmst_edid_after_set(a_element, a_old_value, a_new_value);
}

/// Upstream `wbFLSTEDIDAfterSet`.
pub fn wb_flstedid_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    flst_edid_after_set(a_element, a_old_value, a_new_value);
}

/// Upstream `wbMGEFAssocItemAfterSet`.
pub fn wb_mgef_assoc_item_after_set(a_element: &ElementRef, _a_old_value: &Variant, a_new_value: &Variant) {
    mgef_assoc_item_after_set(a_element, a_new_value, "Archetype");
}

/// Upstream `wbMGEFAV2WeightAfterSet`.
pub fn wb_mgefav2_weight_after_set(a_element: &ElementRef, _a_old_value: &Variant, a_new_value: &Variant) {
    mgef_assoc_item_after_set(a_element, a_new_value, "Archetype");
}

/// Upstream `wbMGEFArchtypeAfterSet`.
pub fn wb_mgef_archtype_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    mgef_archtype_after_set(
        a_element,
        a_old_value,
        a_new_value,
        &[(6, 0), (7, 1), (8, 0), (11, 54), (21, 53), (24, 1), (38, 1), (42, 1)],
    );
}

/// Upstream `wbFLSTLNAMIsSorted`.
pub fn wb_flstlnam_is_sorted(a_container: ElementArg) -> bool {
    flst_lnam_is_sorted(a_container)
}

/// Upstream `wbAECHTypeAfterSet`.
pub fn wb_aech_type_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    aech_type_after_set(a_element, a_old_value, a_new_value);
}

/// Upstream `wbARMOAfterLoad`.
pub fn wb_armo_after_load(a_element: &ElementRef) {
    replace_bodt_with_bod2(a_element);
}

/// Upstream `wbARMAAfterLoad`.
pub fn wb_arma_after_load(a_element: &ElementRef) {
    replace_bodt_with_bod2(a_element);
}

/// Upstream `wbNPCAfterLoad`.
pub fn wb_npc_after_load(a_element: &ElementRef) {
    check_morph_key_order(a_element);
}

/// Upstream `wbREFRAfterLoad`.
pub fn wb_refr_after_load(a_element: &ElementRef) {
    refr_after_load_lock(a_element, false);
}

/// Upstream `wbCELLXCLWGetConflictPriority`.
pub fn wb_cellxclw_get_conflict_priority(a_element: ElementArg, a_conflict_priority: &mut ConflictPriority) {
    cell_xclw_get_conflict_priority(a_element, a_conflict_priority);
}

/// Upstream `wbCELLDATAAfterSet`.
pub fn wb_celldata_after_set(a_element: &ElementRef, _a_old_value: &Variant, _a_new_value: &Variant) {
    cell_data_after_set(a_element);
}

/// Upstream `wbEFITAfterLoad`.
pub fn wb_efit_after_load(a_element: &ElementRef) {
    efit_after_load(a_element);
}

/// Upstream `wbLLEAfterLoad`.
pub fn wb_lle_after_load(a_element: &ElementRef) {
    lle_after_load(a_element);
}

/// Upstream `wbPackageDataInputValueTypeAfterSet`.
pub fn wb_package_data_input_value_type_after_set(
    a_element: &ElementRef,
    a_old_value: &Variant,
    a_new_value: &Variant,
) {
    package_data_input_value_type_after_set(a_element, a_old_value, a_new_value);
}

/// Upstream `wbCELLCombinedRefsAfterSet`.
pub fn wb_cell_combined_refs_after_set(a_element: &ElementRef, _a_old_value: &Variant, _a_new_value: &Variant) {
    with_forced_internal_edit(|| cell_combined_refs_after_set(a_element));
}

/// Upstream `wbSCENBehaviorEnumAfterLoad`: at most 3.
pub fn wb_scen_behavior_enum_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        if variant_int(&a_element.get_native_value()) > 3 {
            set_native(a_element, 3i64);
        }
    });
}
