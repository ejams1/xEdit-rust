// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsTES5.pas

//! The callbacks of `wbDefinitionsTES5.pas` that are ported by hand. The
//! ones that are not ported yet are stubs in `tes5_stubs.rs`.

pub use super::tes5_stubs::*;

use std::sync::Mutex;

use xedit_core::delphi::{change_file_ext, path_file_name, round};
use xedit_core::interface::form_id_formater::actor_value_enum;
use xedit_core::interface::globals::more_info_for_decider;
use xedit_core::interface::misc::{int_to_hex64, progress};
use xedit_core::interface::string::to_comma_text;
use xedit_core::interface::*;

use super::common::{
    variant_int, wb_try_get_container_from_union, wb_try_get_container_ref_from_union_or_value, wb_try_get_main_record,
};
use crate::signatures::{ANAM, NAME, PRKE, QUST};
use crate::tes5::{
    TConditionParameterType, WB_CONDITION_FUNCTIONS, WB_EVENT_FUNCTION_ENUM, WB_EVENT_MEMBER_ENUM,
    wb_condition_desc_from_index,
};

/// Delphi `StrToIntDef(aString, 0)` for 64 bits: decimal, or hexadecimal
/// after `$`.
fn str_to_int64(text: &str) -> Option<i64> {
    let text = text.trim();
    match text.strip_prefix('$') {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

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
        .map_or(0, |container| variant_int(&container.get_element_native_value(path)))
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

/// Upstream `wbConditionFunctionToStr`: the name of the condition function.
pub fn wb_condition_function_to_str(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    let desc = wb_condition_desc_from_index(a_int as i32);
    match a_type {
        CallbackType::ctEditType => "ComboBox".to_owned(),
        CallbackType::ctToSortKey => int_to_hex64(a_int, 8),
        CallbackType::ctCheck => match desc {
            Some(_) => String::new(),
            None => format!("<Unknown: {a_int}>"),
        },
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => match desc {
            Some(desc) => desc.name.to_owned(),
            None if matches!(a_type, CallbackType::ctToSummary | CallbackType::ctToEditValue) => a_int.to_string(),
            None => format!("<Unknown: {a_int}>"),
        },
        CallbackType::ctEditInfo => {
            let mut names: Vec<String> = WB_CONDITION_FUNCTIONS
                .iter()
                .map(|function| function.name.to_owned())
                .collect();
            names.sort_by_key(|name| name.to_lowercase());
            to_comma_text(&names)
        }
        _ => String::new(),
    }
}

/// Upstream `wbConditionFunctionToInt`.
pub fn wb_condition_function_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    for function in WB_CONDITION_FUNCTIONS {
        if function.name.eq_ignore_ascii_case(a_string) {
            return i64::from(function.index);
        }
    }
    str_to_int64(a_string).unwrap_or(0)
}

/// The parameter type of the function of the condition, with the alias and
/// packdata flags applied to the reference types.
fn condition_param_decider(a_element: ElementArg, second: bool) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let function = container_int(&container, "Function");
    let Some(desc) = wb_condition_desc_from_index(function as i32) else {
        return 0;
    };
    let mut param_type = if second { desc.param_type2 } else { desc.param_type1 };
    let param_flag = container_int(&container, "Type");
    if matches!(
        param_type,
        TConditionParameterType::ptReference | TConditionParameterType::ptActor | TConditionParameterType::ptPackage
    ) {
        if param_flag & 0x02 > 0 {
            // 'use aliases' is set
            param_type = TConditionParameterType::ptAlias;
        } else if param_flag & 0x08 > 0 {
            // 'use packdata' is set
            param_type = TConditionParameterType::ptPackdata;
        }
    }
    param_type as i32 + 1
}

/// Upstream `wbConditionParam1Decider`.
pub fn wb_condition_param1_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    condition_param_decider(a_element, false)
}

/// Upstream `wbConditionParam2Decider`.
pub fn wb_condition_param2_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    condition_param_decider(a_element, true)
}

/// Upstream `wbConditionVATSValueParamDecider`: the value of parameter 1.
pub fn wb_condition_vats_value_param_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    container_int(&container, "Parameter #1") as i32
}

/// Upstream `wbConditionEventToStr`: the event function and member.
pub fn wb_condition_event_to_str(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    let event_function = a_int & 0xFFFF;
    let event_member = a_int >> 16;
    let function_enum = WB_EVENT_FUNCTION_ENUM.get();
    let member_enum = WB_EVENT_MEMBER_ENUM.get();
    let (Some(function_enum), Some(member_enum)) = (function_enum, member_enum) else {
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

/// Upstream `wbConditionQuestStageToStr`: the stage of the quest of
/// parameter 1.
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
    let Some(main_record) = wb_try_get_main_record(
        container
            .as_container()
            .and_then(|container| container.get_element_by_name("Parameter #1"))
            .as_ref(),
        "",
    ) else {
        return result;
    };
    let main_record = main_record.get_winning_override();
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
    let mut edit_infos: Option<Vec<(String, i64)>> = match a_type {
        CallbackType::ctEditType => return "ComboBox".to_owned(),
        CallbackType::ctEditInfo => Some(Vec::new()),
        _ => None,
    };
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
            if let Some(edit_infos) = &mut edit_infos {
                edit_infos.push((t.clone(), j));
            }
            if j == a_int {
                match a_type {
                    CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => result = t,
                    CallbackType::ctCheck => result = String::new(),
                    _ => {}
                }
                return result;
            }
        }
    }
    match a_type {
        CallbackType::ctCheck => {
            result = format!(
                "<Warning: Quest Stage [{a_int}] not found in \"{}\">",
                main_record.get_name()
            );
        }
        CallbackType::ctToStr => {
            result = format!(
                "{a_int} <Warning: Quest Stage [{a_int}] not found in \"{}\">",
                main_record.get_name()
            );
        }
        CallbackType::ctEditInfo => {
            let mut edit_infos = edit_infos.unwrap_or_default();
            edit_infos.sort_by_key(|(text, _)| text.to_lowercase());
            let names: Vec<String> = edit_infos.into_iter().map(|(text, _)| text).collect();
            result = to_comma_text(&names);
        }
        _ => {}
    }
    result
}

/// Upstream `wbStringToInt`.
pub fn wb_string_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    str_to_int64(a_string).unwrap_or(0)
}

/// Upstream `TFaceGenFeature`: the tint layers of a race and sex, cached
/// by `wbTintLayerToStr`.
struct FaceGenFeature {
    race_id: String,
    female: bool,
    entries: Vec<(u32, String)>,
}

/// Upstream `TintLayers`: the cache of the tint layers by race.
static TINT_LAYERS: Mutex<Vec<FaceGenFeature>> = Mutex::new(Vec::new());

/// The tint layers of the race from its `Head Data`.
fn tint_layer_entries(race: &MainRecordRef, female: bool) -> Vec<(u32, String)> {
    let path = if female {
        "Head Data\\Female Head Data\\Tint Masks"
    } else {
        "Head Data\\Male Head Data\\Tint Masks"
    };
    let Some(container) = race.get_element_by_path(path) else {
        return Vec::new();
    };
    let Some(container) = container.as_container() else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for index in 0..container.get_element_count() {
        // Should never be false.
        let Some(entry) = container.get_element(index) else {
            continue;
        };
        let Some(entry) = entry.as_container() else { continue };
        let layer_index = variant_int(&entry.get_element_native_value("Tint Layer\\TINI")) as u32;
        let mut name = entry.get_element_edit_value("Tint Layer\\TINP");
        // Add the texture name.
        if !name.is_empty() {
            name = format!("[{name}] ");
        }
        let texture = entry.get_element_edit_value("Tint Layer\\TINT");
        name.push_str(&change_file_ext(path_file_name(&texture), ""));
        entries.push((layer_index, name));
    }
    entries
}

/// Upstream `wbTintLayerToStr`: the name of the tint layer of the race of
/// the actor.
pub fn wb_tint_layer_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let mut result = match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => a_int.to_string(),
        CallbackType::ctToSortKey => return int_to_hex64(a_int, 8),
        CallbackType::ctCheck => format!("<Warning: Could not resolve tint layer index {a_int}>"),
        _ => String::new(),
    };
    let Some(actor) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return result;
    };
    let female = actor.get_element_edit_value("ACBS\\Flags\\Female") == "1";
    let Some(race) = actor
        .get_element_by_signature(Signature::new(b"RNAM"))
        .and_then(|element| element.get_links_to())
        .and_then(|element| element.into_main_record())
    else {
        return result;
    };
    let race = race.get_winning_override();
    let race_id = race.get_editor_id();
    let entries = {
        let mut cache = TINT_LAYERS.lock().unwrap();
        let cached = cache
            .iter()
            .position(|feature| feature.female == female && feature.race_id == race_id);
        let position = match cached {
            Some(position) => position,
            None => {
                // Cache not found, fill with data from RACE.
                for female2 in [false, true] {
                    cache.push(FaceGenFeature {
                        race_id: race_id.clone(),
                        female: female2,
                        entries: tint_layer_entries(&race, female2),
                    });
                }
                match cache
                    .iter()
                    .position(|feature| feature.female == female && feature.race_id == race_id)
                {
                    Some(position) => position,
                    None => return result,
                }
            }
        };
        cache[position].entries.clone()
    };
    let index = a_int as u32;
    let entry_name = entries
        .iter()
        .find(|(layer_index, _)| *layer_index == index)
        .map(|(_, name)| name.clone())
        .unwrap_or_default();
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            if !entry_name.is_empty() {
                result = format!("{a_int} {entry_name}");
            } else {
                result = a_int.to_string();
                if a_type == CallbackType::ctToStr {
                    result.push_str(&format!(
                        " <Tint Layer Index [{a_int}] not found in {}>",
                        race.get_name()
                    ));
                }
            }
        }
        CallbackType::ctCheck => {
            result = if entry_name.is_empty() {
                format!("<Tint Layer Index [{a_int}] not found in {}>", race.get_name())
            } else {
                String::new()
            };
        }
        CallbackType::ctEditType => result = "ComboBox".to_owned(),
        CallbackType::ctEditInfo => {
            result = entries
                .iter()
                .map(|(layer_index, name)| format!("\"{layer_index} {name}\""))
                .collect::<Vec<_>>()
                .join(",");
        }
        _ => {}
    }
    result
}

/// Upstream `wbEPFDActorValueToStr`: the actor value stored as a float.
pub fn wb_epfd_actor_value_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let as_float = f32::from_bits(a_int as u32);
    let a_int = round(f64::from(as_float));
    let Some(actor_values) = actor_value_enum() else {
        return String::new();
    };
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            actor_values.to_string(a_int, a_element, a_type == CallbackType::ctToSummary)
        }
        CallbackType::ctToSortKey => actor_values.to_sort_key(a_int, a_element),
        CallbackType::ctCheck => actor_values.check(a_int, a_element),
        CallbackType::ctToEditValue => actor_values.to_edit_value(a_int, a_element),
        CallbackType::ctEditType => "ComboBox".to_owned(),
        CallbackType::ctEditInfo => to_comma_text(&actor_values.get_edit_info(a_element)),
        _ => String::new(),
    }
}

/// Upstream `wbEPFDActorValueToInt`: the actor value stored as a float.
pub fn wb_epfd_actor_value_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    let value = actor_value_enum()
        .and_then(|actor_values| actor_values.find_name(a_string))
        .unwrap_or_else(|| a_string.trim().parse().unwrap_or(0));
    i64::from((value as f32).to_bits())
}

/// Upstream `wbPerkDATAQuestStageToStr`: the stage of the quest of the perk entry.
pub fn wb_perk_data_quest_stage_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let mut result = match a_type {
        CallbackType::ctToEditValue | CallbackType::ctToSummary => a_int.to_string(),
        CallbackType::ctToStr => format!("{a_int} <Warning: Could not resolve Quest>"),
        CallbackType::ctToSortKey => return int_to_hex64(a_int, 8),
        CallbackType::ctCheck => "<Warning: Could not resolve Quest>".to_owned(),
        _ => String::new(),
    };
    let Some(container) = wb_try_get_container_ref_from_union_or_value(a_element) else {
        return result;
    };
    let Some(main_record) = wb_try_get_main_record(
        container
            .as_container()
            .and_then(|container| container.get_element_by_name("Quest"))
            .as_ref(),
        "",
    ) else {
        return result;
    };
    let main_record = main_record.get_winning_override();
    if main_record.get_signature() != QUST {
        match a_type {
            CallbackType::ctToStr => {
                result = format!(
                    "{a_int} <Warning: \"{}\" is not a Quest record>",
                    main_record.get_short_name()
                );
            }
            CallbackType::ctCheck => {
                result = format!("<Warning: \"{}\" is not a Quest record>", main_record.get_short_name());
            }
            _ => {}
        }
        return result;
    }
    let mut edit_infos: Option<Vec<String>> = match a_type {
        CallbackType::ctEditType => return "ComboBox".to_owned(),
        CallbackType::ctEditInfo => Some(Vec::new()),
        _ => None,
    };
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
            if let Some(edit_infos) = &mut edit_infos {
                edit_infos.push(t.clone());
            }
            if j == a_int {
                match a_type {
                    CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => result = t,
                    CallbackType::ctCheck => result = String::new(),
                    _ => {}
                }
                return result;
            }
        }
    }
    match a_type {
        CallbackType::ctToStr => {
            result = format!(
                "{a_int} <Warning: Quest Stage [{a_int}] not found in \"{}\">",
                main_record.get_name()
            );
        }
        CallbackType::ctCheck => {
            result = format!(
                "<Warning: Quest Stage [{a_int}] not found in \"{}\">",
                main_record.get_name()
            );
        }
        CallbackType::ctEditInfo => {
            let mut edit_infos = edit_infos.unwrap_or_default();
            edit_infos.sort_by_key(|text| text.to_lowercase());
            result = to_comma_text(&edit_infos);
        }
        _ => {}
    }
    result
}
