// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsFO3.pas

//! The callbacks of `wbDefinitionsFO3.pas` that are ported by hand. The
//! ones that are not ported yet are stubs in `fo3_stubs.rs`.

// The stubs of the callbacks not ported yet; empty once every callback is ported.
#[allow(unused_imports)]
pub use super::fo3_stubs::*;

use xedit_core::delphi::round;
use xedit_core::interface::form_id_formater::actor_value_enum;
use xedit_core::interface::globals::{actor_template_hide, sort_flst};
use xedit_core::interface::misc::int_to_hex64;
use xedit_core::interface::string::to_comma_text;
use xedit_core::interface::*;

use super::common::{
    variant_int, wb_try_get_container_from_union, wb_try_get_container_ref_from_union_or_value, wb_try_get_main_record,
};
use crate::fo3::{
    WB_CONDITION_FUNCTIONS, WB_PERK_CONDITION, WB_PERK_ENTRY_POINTS, WB_PERK_FUNCTIONS, wb_condition_desc_from_index,
};
use crate::signatures::{PRKE, QUST};

/// Upstream `CmpW32` of `wbInterface`: -1, 0 or 1 for two unsigned values.
pub fn cmp_w32(a: u32, b: u32) -> i32 {
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Delphi `StrToIntDef(aString, 0)` for 64 bits: decimal, or hexadecimal
/// after `$`.
fn str_to_int64(text: &str) -> Option<i64> {
    let text = text.trim();
    match text.strip_prefix('$') {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// `Container.ElementNativeValues[aPath]` as an integer, 0 when missing.
fn container_int(container: &ElementRef, path: &str) -> i64 {
    container
        .as_container()
        .map_or(0, |container| variant_int(&container.get_element_native_value(path)))
}

/// Upstream `wbPERKFunctionParams`, which the transpiler does not write.
const WB_PERK_FUNCTION_PARAMS: [&str; 5] = ["None", "Float", "Float, Float", "Leveled Item", "Script"];

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

/// Upstream `wbConditionParam1Decider` and `wbConditionParam2Decider`: the
/// parameter type of the function of the condition.
fn condition_param_decider(a_element: ElementArg, second: bool) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let function = container_int(&container, "Function");
    let Some(desc) = wb_condition_desc_from_index(function as i32) else {
        return 0;
    };
    let param_type = if second { desc.param_type2 } else { desc.param_type1 };
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

/// Upstream `wbConditionVATSValueParam`: the value of parameter 1.
pub fn wb_condition_vats_value_param(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    container_int(&container, "Parameter #1") as i32
}

/// The stage lines of a quest for the quest stage callbacks: the text of
/// stage `a_int`, or the lines for the edit info when it is not found.
fn quest_stage_text(quest: &MainRecordRef, a_int: i64, a_type: CallbackType) -> Result<String, Vec<String>> {
    let mut edit_infos = Vec::new();
    if let Some(stages) = quest.get_element_by_name("Stages")
        && let Some(stages) = stages.as_container()
    {
        for index in 0..stages.get_element_count() {
            let Some(stage) = stages.get_element(index) else {
                continue;
            };
            let Some(stage) = stage.as_container() else { continue };
            let j = variant_int(&stage.get_element_native_value("INDX"));
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

/// Upstream `wbConditionQuestStageToStr`: the stage of the quest of
/// parameter 1.
pub fn wb_condition_quest_stage_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let result = match a_type {
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
    let main_record = main_record.get_winning_override();
    if main_record.get_signature() != QUST {
        return match a_type {
            CallbackType::ctCheck => format!("<Warning: \"{}\" is not a Quest record>", main_record.get_short_name()),
            // UPSTREAM-QUIRK: `Record` is capitalized here only.
            CallbackType::ctToStr => format!(
                "{a_int} <Warning: \"{}\" is not a Quest Record>",
                main_record.get_short_name()
            ),
            _ => result,
        };
    }
    if a_type == CallbackType::ctEditType {
        return "ComboBox".to_owned();
    }
    match quest_stage_text(&main_record, a_int, a_type) {
        Ok(text) => text,
        Err(mut edit_infos) => match a_type {
            CallbackType::ctCheck => format!(
                "<Warning: Quest Stage [{a_int}] not found in \"{}\">",
                main_record.get_name()
            ),
            CallbackType::ctToStr => format!(
                "{a_int} <Warning: Quest Stage [{a_int}] not found in \"{}\">",
                main_record.get_name()
            ),
            CallbackType::ctEditInfo => {
                edit_infos.sort_by_key(|text| text.to_lowercase());
                to_comma_text(&edit_infos)
            }
            _ => result,
        },
    }
}

/// The script of the record of parameter 1 for the variable name callbacks:
/// the base record of a reference, its winning override, and the record its
/// `SCRI` links to.
enum ParamScript {
    Unresolved,
    NoScri(MainRecordRef),
    NoScript(MainRecordRef),
    Script(MainRecordRef),
}

fn param_script(a_element: ElementArg) -> ParamScript {
    let Some(container) = wb_try_get_container_ref_from_union_or_value(a_element) else {
        return ParamScript::Unresolved;
    };
    let parameter = container
        .as_container()
        .and_then(|container| container.get_element_by_name("Parameter #1"));
    let Some(mut main_record) = wb_try_get_main_record(parameter.as_ref(), "") else {
        return ParamScript::Unresolved;
    };
    if let Some(base) = main_record.get_base_record() {
        main_record = base;
    }
    let main_record = main_record.get_winning_override();
    let Some(scri) = main_record.get_record_by_signature(Signature::new(b"SCRI")) else {
        return ParamScript::NoScri(main_record);
    };
    match scri.get_links_to().and_then(|linked| linked.into_main_record()) {
        Some(script) => ParamScript::Script(script),
        None => ParamScript::NoScript(main_record),
    }
}

/// The local variables of a script as (index, name).
fn local_variables(script: &MainRecordRef) -> Vec<(i64, String)> {
    let mut result = Vec::new();
    if let Some(variables) = script.get_element_by_name("Local Variables")
        && let Some(variables) = variables.as_container()
    {
        for index in 0..variables.get_element_count() {
            let Some(variable) = variables.get_element(index) else {
                continue;
            };
            let Some(variable) = variable.as_container() else {
                continue;
            };
            let j = variant_int(&variable.get_element_native_value("SLSD\\Index"));
            let s = match variable.get_element_native_value("SCVR") {
                Variant::Str(name) => name,
                _ => String::new(),
            };
            result.push((j, s));
        }
    }
    result
}

/// Upstream `wbConditionVariableNameToStr`: the local variable of the script
/// of parameter 1.
pub fn wb_condition_variable_name_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let result = match a_type {
        CallbackType::ctToEditValue | CallbackType::ctToSummary => a_int.to_string(),
        CallbackType::ctToStr => format!("{a_int} <Warning: Could not resolve Parameter 1>"),
        CallbackType::ctToSortKey => return int_to_hex64(a_int, 8),
        CallbackType::ctCheck => "<Warning: Could not resolve Parameter 1>".to_owned(),
        _ => String::new(),
    };
    let script = match param_script(a_element) {
        ParamScript::Unresolved => return result,
        ParamScript::NoScri(record) => {
            return match a_type {
                CallbackType::ctCheck => format!(
                    "<Warning: \"{}\" does not contain a SCRI subrecord>",
                    record.get_short_name()
                ),
                CallbackType::ctToStr => format!(
                    "{a_int} <Warning: \"{}\" does not contain a SCRI Sub-Record>",
                    record.get_short_name()
                ),
                _ => result,
            };
        }
        ParamScript::NoScript(record) => {
            return match a_type {
                CallbackType::ctCheck => {
                    format!(
                        "<Warning: \"{}\" does not have a valid script>",
                        record.get_short_name()
                    )
                }
                CallbackType::ctToStr => format!(
                    "{a_int} <Warning: \"{}\" does not have a valid script>",
                    record.get_short_name()
                ),
                _ => result,
            };
        }
        ParamScript::Script(script) => script,
    };
    let load_order = a_element
        .and_then(|element| element.get_file())
        .map_or(i32::MAX, |file| file.get_load_order());
    let script = script.get_highest_override_or_self(load_order);
    if a_type == CallbackType::ctEditType {
        return "ComboBox".to_owned();
    }
    let variables = local_variables(&script);
    if a_type != CallbackType::ctEditInfo
        && let Some((_, name)) = variables.iter().find(|(index, _)| *index == a_int)
    {
        return match a_type {
            CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => name.clone(),
            _ => String::new(),
        };
    }
    match a_type {
        CallbackType::ctCheck => format!(
            "<Warning: Variable Index [{a_int}] not found in \"{}\">",
            script.get_name()
        ),
        CallbackType::ctToStr => format!(
            "{a_int} <Warning: Variable Index [{a_int}] not found in \"{}\">",
            script.get_name()
        ),
        CallbackType::ctEditInfo => {
            let mut names: Vec<String> = variables.into_iter().map(|(_, name)| name).collect();
            names.sort_by_key(|name| name.to_lowercase());
            to_comma_text(&names)
        }
        _ => result,
    }
}

/// Upstream `wbConditionVariableNameToInt`: the index of the local variable
/// of that name.
pub fn wb_condition_variable_name_to_int(a_string: &str, a_element: ElementArg) -> i64 {
    if let Some(value) = str_to_int64(a_string) {
        return value;
    }
    let ParamScript::Script(script) = param_script(a_element) else {
        return 0;
    };
    let script = script.get_winning_override();
    local_variables(&script)
        .into_iter()
        .find(|(_, name)| name.eq_ignore_ascii_case(a_string.trim()))
        .map_or(0, |(index, _)| index)
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

/// Upstream `wbEPFDDecider`: the `EPFT` value, or 5 for the function that
/// takes an actor value.
pub fn wb_epfd_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = a_element.and_then(|element| element.get_container()) else {
        return 0;
    };
    if container.as_container().is_none() {
        return 0;
    }
    let mut result = container_int(&container, "EPFT") as i32;
    if result == 2 && container_int(&container, "..\\DATA\\Entry Point\\Function") == 5 {
        result = 5;
    }
    result
}

/// Upstream `wbStringToInt`.
pub fn wb_string_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    str_to_int64(a_string).unwrap_or(0)
}

/// The record of an element as upstream `GetElementFromUnion` and the walk up
/// the containers find it.
fn actor_record(a_element: ElementArg) -> Option<MainRecordRef> {
    let mut element = a_element?.clone();
    if element.get_element_type() == ElementType::etUnion {
        element = element.get_container()?;
        while element.get_element_type() == ElementType::etUnion {
            element = element.get_container()?;
        }
    }
    loop {
        if element.as_main_record().is_some() {
            return element.into_main_record();
        }
        element = element.get_container()?;
    }
}

/// Whether the actor of the element has `mask` set in `path`, when the
/// template elements are hidden.
fn actor_flag(a_element: ElementArg, path: &str, mask: i64) -> bool {
    if !actor_template_hide() {
        return false;
    }
    actor_record(a_element).is_some_and(|record| variant_int(&record.get_element_native_value(path)) & mask != 0)
}

/// Upstream `wbActorTemplateUseTraits`.
pub fn wb_actor_template_use_traits(a_element: ElementArg) -> bool {
    actor_flag(a_element, "ACBS\\Template Flags", 0x0000_0001)
}

/// Upstream `wbActorTemplateUseStats`.
pub fn wb_actor_template_use_stats(a_element: ElementArg) -> bool {
    actor_flag(a_element, "ACBS\\Template Flags", 0x0000_0002)
}

/// Upstream `wbActorTemplateUseFactions`.
pub fn wb_actor_template_use_factions(a_element: ElementArg) -> bool {
    actor_flag(a_element, "ACBS\\Template Flags", 0x0000_0004)
}

/// Upstream `wbActorTemplateUseActorEffectList`.
pub fn wb_actor_template_use_actor_effect_list(a_element: ElementArg) -> bool {
    actor_flag(a_element, "ACBS\\Template Flags", 0x0000_0008)
}

/// Upstream `wbActorTemplateUseAIData`.
pub fn wb_actor_template_use_ai_data(a_element: ElementArg) -> bool {
    actor_flag(a_element, "ACBS\\Template Flags", 0x0000_0010)
}

/// Upstream `wbActorTemplateUseAIPackages`.
pub fn wb_actor_template_use_ai_packages(a_element: ElementArg) -> bool {
    actor_flag(a_element, "ACBS\\Template Flags", 0x0000_0020)
}

/// Upstream `wbActorTemplateUseModelAnimation`.
pub fn wb_actor_template_use_model_animation(a_element: ElementArg) -> bool {
    actor_flag(a_element, "ACBS\\Template Flags", 0x0000_0040)
}

/// Upstream `wbActorTemplateUseBaseData`.
pub fn wb_actor_template_use_base_data(a_element: ElementArg) -> bool {
    actor_flag(a_element, "ACBS\\Template Flags", 0x0000_0080)
}

/// Upstream `wbActorTemplateUseInventory`.
pub fn wb_actor_template_use_inventory(a_element: ElementArg) -> bool {
    actor_flag(a_element, "ACBS\\Template Flags", 0x0000_0100)
}

/// Upstream `wbActorTemplateUseScript`.
pub fn wb_actor_template_use_script(a_element: ElementArg) -> bool {
    actor_flag(a_element, "ACBS\\Template Flags", 0x0000_0200)
}

/// Upstream `wbActorAutoCalcDontShow`.
pub fn wb_actor_auto_calc_dont_show(a_element: ElementArg) -> bool {
    actor_flag(a_element, "ACBS\\Flags", 0x0000_0010)
}

/// Upstream `wbActorTemplateUseStatsAutoCalc`.
pub fn wb_actor_template_use_stats_auto_calc(a_element: ElementArg) -> bool {
    actor_template_hide() && (wb_actor_template_use_stats(a_element) || wb_actor_auto_calc_dont_show(a_element))
}

/// Upstream `wbIdleAnam`: the animation group section and its flags.
pub fn wb_idle_anam(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    let section = a_int & !0xC0;
    let known = matches!(section, 0..=7 | 20 | 21);
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            let mut result = match section {
                0 => "Idle".to_owned(),
                1 => "Movement".to_owned(),
                2 => "Left Arm".to_owned(),
                3 => "Left Hand".to_owned(),
                4 => "Weapon".to_owned(),
                5 => "Weapon Up".to_owned(),
                6 => "Weapon Down".to_owned(),
                7 => "Special Idle".to_owned(),
                20 => "Whole Body".to_owned(),
                21 => "Upper Body".to_owned(),
                _ => format!("<Unknown: {section}>"),
            };
            if a_int & 0x80 == 0 {
                result.push_str(", Must return a file");
            }
            // UPSTREAM-QUIRK: `(aInt and $40) = 1` is never true, so
            // `, Loose Idle` is never added.
            result
        }
        CallbackType::ctToSortKey => int_to_hex64(a_int, 2),
        CallbackType::ctCheck if known => String::new(),
        CallbackType::ctCheck => format!("<Unknown: {section}>"),
        _ => String::new(),
    }
}

/// The native value of the `DATA` subrecord of the container of the
/// element, for the note deciders.
fn note_type(a_element: ElementArg) -> i64 {
    a_element
        .and_then(|element| element.get_container())
        .and_then(|container| {
            container
                .as_container()?
                .get_record_by_signature(Signature::new(b"DATA"))
        })
        .map_or(0, |data| variant_int(&data.get_native_value()))
}

/// Upstream `wbNOTETNAMDecide`: 1 for a voice note.
pub fn wb_notetnam_decide(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    i32::from(note_type(a_element) == 3)
}

/// Upstream `wbNOTESNAMDecide`: 1 for a voice note.
pub fn wb_notesnam_decide(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    i32::from(note_type(a_element) == 3)
}

/// Upstream `wbMGEFFAssocItemDecider`: the member for the `Archtype`.
pub fn wb_mgeff_assoc_item_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    const OFFSET_ARCHTYPE: usize = 56;
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 1;
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
        None => 1,
        Some(1) => 2,  // Script
        Some(18) => 3, // Bound Item
        Some(19) => 4, // Summon Creature
        Some(_) => 0,
    }
}

/// Upstream `wbNAVINVMIDecider`: 1 when flag `$20` is set.
pub fn wb_navinvmi_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    i32::from(container_int(&container, "Flags") & 0x20 == 32)
}

/// Upstream `wbFLSTLNAMIsSorted`: form lists are sorted unless their editor
/// ID ends in `OrderedList` or they are one of the ordered lists of
/// `WeaponModKits.esp`.
pub fn wb_flstlnam_is_sorted(a_container: ElementArg) -> bool {
    let Some(a_container) = a_container.and_then(|element| element.as_container()) else {
        return sort_flst();
    };
    const ORDERED_LIST: &str = "OrderedList";
    let mut result = sort_flst();
    if result && let Some(edid) = a_container.get_record_by_signature(Signature::new(b"EDID")) {
        let value = edid.get_value();
        let tail = if value.len() > ORDERED_LIST.len() {
            &value[value.len() - ORDERED_LIST.len()..]
        } else {
            value.as_str()
        };
        if tail.eq_ignore_ascii_case(ORDERED_LIST) {
            result = false;
        }
    }
    if result {
        let Some(main_record) = a_container.get_containing_main_record() else {
            return result;
        };
        let main_record = main_record.get_master_or_self();
        let Some(file) = main_record.get_file() else {
            return result;
        };
        if !file.get_name().eq_ignore_ascii_case("WeaponModKits.esp") {
            return result;
        }
        if matches!(
            main_record.get_form_id().object_id(),
            0x0130EB
                | 0x0130ED
                | 0x01522D
                | 0x01522E
                | 0x0158D5
                | 0x0158D6
                | 0x0158D7
                | 0x0158D8
                | 0x0158D9
                | 0x0158DA
                | 0x0158DC
                | 0x0158DD
                | 0x018E20
        ) {
            result = false;
        }
    }
    result
}

/// Upstream `wbPKDTSpecificFlagsDecider`: the package type plus one, 0 for
/// the short form of the subrecord.
pub fn wb_pkdt_specific_flags_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    if container.get_sub_record_header_size() == Some(8) {
        return 0;
    }
    container_int(&container, "Type") as i32 + 1
}

/// Whether the entry point function parameters have an `EPFT` outside
/// `values`.
fn epft_not_in(a_element: ElementArg, values: &[i64]) -> bool {
    let Some(element) = a_element else { return false };
    if element.get_name() != "Entry Point Function Parameters" || element.as_container().is_none() {
        return false;
    }
    !values.contains(&container_int(element, "EPFT"))
}

/// Upstream `wbEPFDDontShow`.
pub fn wb_epfd_dont_show(a_element: ElementArg) -> bool {
    epft_not_in(a_element, &[1, 2, 3])
}

/// Upstream `wbEPF2DontShow`.
pub fn wb_epf2_dont_show(a_element: ElementArg) -> bool {
    epft_not_in(a_element, &[4])
}

/// Upstream `wbPERKPRKCDontShow`: the run on value only shows for an entry
/// point effect.
pub fn wb_perkprkc_dont_show(a_element: ElementArg) -> bool {
    let Some(element) = a_element else { return false };
    if element.get_name() != "Effect" || element.as_container().is_none() {
        return false;
    }
    container_int(element, "PRKE\\Type") != 2
}

/// Upstream `wbTES4ONAMDontShow`: the overridden forms only show for a
/// master file.
pub fn wb_tes4_onam_dont_show(a_element: ElementArg) -> bool {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .is_some_and(|record| !record.get_flags().is_esm())
}

/// The ordinal at `path` from the element, `None` when it is not a
/// container or the value is empty.
fn element_ordinal(a_element: ElementArg, path: &str) -> Option<i64> {
    a_element?.as_container()?.get_element_native_value(path).as_ordinal()
}

/// The common start of the entry point callbacks: the text for an
/// unresolved value, or the sort key.
fn unresolved(a_int: i64, a_type: CallbackType, what: &str, sort_digits: usize) -> Result<String, String> {
    match a_type {
        CallbackType::ctToStr => Ok(format!("{a_int} <Warning: Could not resolve {what}>")),
        CallbackType::ctToSummary | CallbackType::ctToEditValue => Ok(a_int.to_string()),
        CallbackType::ctToSortKey => Err(int_to_hex64(a_int, sort_digits)),
        CallbackType::ctCheck => Ok(format!("<Warning: Could not resolve {what}>")),
        _ => Ok(String::new()),
    }
}

/// The text for a value that is out of range.
fn out_of_range(a_int: i64, a_type: CallbackType, warning: &str, result: String) -> String {
    match a_type {
        CallbackType::ctToStr => format!("{a_int} <Warning: {warning}>"),
        CallbackType::ctToSummary => a_int.to_string(),
        CallbackType::ctCheck => format!("<Warning: {warning}>"),
        _ => result,
    }
}

/// Upstream `wbPRKCToStr`: the run on caption of the entry point condition.
pub fn wb_prkc_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let result = match unresolved(a_int, a_type, "Entry Point", 2) {
        Ok(result) => result,
        Err(sort_key) => return sort_key,
    };
    let Some(entry_point) = element_ordinal(a_element, "..\\..\\..\\DATA\\Entry Point\\Entry Point") else {
        return result;
    };
    let Some(entry_point_def) = usize::try_from(entry_point)
        .ok()
        .and_then(|index| WB_PERK_ENTRY_POINTS.get(index))
    else {
        return out_of_range(a_int, a_type, &format!("Unknown Entry Point #{entry_point}"), result);
    };
    let condition = &WB_PERK_CONDITION[entry_point_def.condition as usize];
    let captions = [condition.caption1, condition.caption2, condition.caption3];
    match a_type {
        CallbackType::ctEditType => "ComboBox".to_owned(),
        CallbackType::ctEditInfo => {
            let mut names: Vec<String> = captions
                .iter()
                .filter(|caption| !caption.is_empty())
                .map(|caption| (*caption).to_owned())
                .collect();
            names.sort_by_key(|name| name.to_lowercase());
            to_comma_text(&names)
        }
        _ if a_int < 0 || a_int >= i64::from(condition.count) => {
            out_of_range(a_int, a_type, "Value out of Bounds for this Entry Point", result)
        }
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => match a_int {
            0..=2 => captions[a_int as usize].to_owned(),
            _ => result,
        },
        CallbackType::ctCheck => String::new(),
        _ => result,
    }
}

/// Upstream `wbPRKCToInt`. The exceptions upstream raises for a value it
/// cannot resolve become 0.
pub fn wb_prkc_to_int(a_string: &str, a_element: ElementArg) -> i64 {
    let s = a_string.trim();
    if let Ok(value) = s.parse::<i64>() {
        return value;
    }
    if s.is_empty() {
        return 0;
    }
    let Some(entry_point) = element_ordinal(a_element, "..\\..\\..\\DATA\\Entry Point\\Entry Point")
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| WB_PERK_ENTRY_POINTS.get(index))
    else {
        return 0;
    };
    let condition = &WB_PERK_CONDITION[entry_point.condition as usize];
    [condition.caption1, condition.caption2, condition.caption3]
        .iter()
        .position(|caption| caption.eq_ignore_ascii_case(a_string))
        .map_or(0, |index| index as i64)
}

/// Upstream `wbPerkDATAFunctionToStr`: the function of the entry point.
pub fn wb_perk_data_function_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let result = match unresolved(a_int, a_type, "Entry Point", 2) {
        Ok(result) => result,
        Err(sort_key) => return sort_key,
    };
    let Some(entry_point) = element_ordinal(a_element, "..\\Entry Point") else {
        return result;
    };
    let Some(entry_point_def) = usize::try_from(entry_point)
        .ok()
        .and_then(|index| WB_PERK_ENTRY_POINTS.get(index))
    else {
        return out_of_range(a_int, a_type, &format!("Unknown Entry Point #{entry_point}"), result);
    };
    match a_type {
        CallbackType::ctEditType => "ComboBox".to_owned(),
        CallbackType::ctEditInfo => {
            let mut names: Vec<String> = WB_PERK_FUNCTIONS
                .iter()
                .filter(|function| function.function_type == entry_point_def.function_type && !function.name.is_empty())
                .map(|function| function.name.to_owned())
                .collect();
            names.sort_by_key(|name| name.to_lowercase());
            to_comma_text(&names)
        }
        _ => {
            let Some(function) = usize::try_from(a_int)
                .ok()
                .and_then(|index| WB_PERK_FUNCTIONS.get(index))
            else {
                return out_of_range(a_int, a_type, "Unknown Function", result);
            };
            let matches = function.function_type == entry_point_def.function_type;
            match a_type {
                CallbackType::ctToStr if !matches => {
                    format!("{} <Warning: Value out of Bounds for this Entry Point>", function.name)
                }
                CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => {
                    function.name.to_owned()
                }
                CallbackType::ctCheck if !matches => "<Warning: Value out of Bounds for this Entry Point>".to_owned(),
                CallbackType::ctCheck => String::new(),
                _ => result,
            }
        }
    }
}

/// Upstream `wbPerkDATAFunctionToInt`. The exceptions upstream raises for
/// a value it cannot resolve become 0.
pub fn wb_perk_data_function_to_int(a_string: &str, a_element: ElementArg) -> i64 {
    let s = a_string.trim();
    if let Ok(value) = s.parse::<i64>() {
        return value;
    }
    let Some(entry_point) = element_ordinal(a_element, "..\\Entry Point")
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| WB_PERK_ENTRY_POINTS.get(index))
    else {
        return 0;
    };
    WB_PERK_FUNCTIONS
        .iter()
        .position(|function| {
            function.function_type == entry_point.function_type && function.name.eq_ignore_ascii_case(s)
        })
        .map_or(0, |index| index as i64)
}

/// Upstream `wbPerkEPFTToStr`: the parameter type of the function.
pub fn wb_perk_epft_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let result = match unresolved(a_int, a_type, "Function", 2) {
        Ok(result) => result,
        Err(sort_key) => return sort_key,
    };
    let Some(function) = element_ordinal(a_element, "..\\..\\DATA\\Entry Point\\Function") else {
        return result;
    };
    let Some(function_def) = usize::try_from(function)
        .ok()
        .and_then(|index| WB_PERK_FUNCTIONS.get(index))
    else {
        return out_of_range(a_int, a_type, &format!("Unknown Function #{function}"), result);
    };
    match a_type {
        CallbackType::ctEditType => "ComboBox".to_owned(),
        CallbackType::ctEditInfo => format!("\"{}\"", WB_PERK_FUNCTION_PARAMS[function_def.param_type as usize]),
        _ => {
            let Some(name) = usize::try_from(a_int)
                .ok()
                .and_then(|index| WB_PERK_FUNCTION_PARAMS.get(index))
            else {
                return out_of_range(a_int, a_type, "Unknown Function Param Type", result);
            };
            let matches = a_int == function_def.param_type as i64;
            match a_type {
                CallbackType::ctToStr if !matches => {
                    format!("{name} <Warning: Value out of Bounds for this Function>")
                }
                CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => (*name).to_owned(),
                // UPSTREAM-QUIRK: the check appends to the unresolved text.
                CallbackType::ctCheck if !matches => {
                    format!("{result} <Warning: Value out of Bounds for this Function>")
                }
                CallbackType::ctCheck => String::new(),
                _ => result,
            }
        }
    }
}

/// Upstream `wbPerkEPFTToInt`. The exceptions upstream raises for a value it
/// cannot resolve become 0.
pub fn wb_perk_epft_to_int(a_string: &str, a_element: ElementArg) -> i64 {
    let s = a_string.trim();
    if let Ok(value) = s.parse::<i64>() {
        return value;
    }
    let Some(function) = element_ordinal(a_element, "..\\..\\DATA\\Entry Point\\Function")
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| WB_PERK_FUNCTIONS.get(index))
    else {
        return 0;
    };
    match WB_PERK_FUNCTION_PARAMS
        .iter()
        .position(|name| name.eq_ignore_ascii_case(s))
    {
        Some(index) if index == function.param_type as usize => index as i64,
        _ => 0,
    }
}

/// Upstream `wbPerkDATAQuestStageToStr`.
pub fn wb_perk_data_quest_stage_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let result = match a_type {
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
    let main_record = main_record.get_winning_override();
    if main_record.get_signature() != QUST {
        return match a_type {
            CallbackType::ctToStr => format!(
                "{a_int} <Warning: \"{}\" is not a Quest record>",
                main_record.get_short_name()
            ),
            CallbackType::ctToSummary => a_int.to_string(),
            CallbackType::ctCheck => format!("<Warning: \"{}\" is not a Quest record>", main_record.get_short_name()),
            _ => result,
        };
    }
    if a_type == CallbackType::ctEditType {
        return "ComboBox".to_owned();
    }
    match quest_stage_text(&main_record, a_int, a_type) {
        Ok(text) => text,
        Err(mut edit_infos) => match a_type {
            CallbackType::ctToStr => format!(
                "{a_int} <Warning: Quest Stage [{a_int}] not found in \"{}\">",
                main_record.get_name()
            ),
            CallbackType::ctToSummary => a_int.to_string(),
            CallbackType::ctCheck => format!(
                "<Warning: Quest Stage [{a_int}] not found in \"{}\">",
                main_record.get_name()
            ),
            CallbackType::ctEditInfo => {
                edit_infos.sort_by_key(|text| text.to_lowercase());
                to_comma_text(&edit_infos)
            }
            _ => result,
        },
    }
}

// ----- the editing callbacks -----

use super::common::{
    add_member, cell_after_load_fallout3, condition_after_load_fallout3, efit_after_load, efsh_after_load_fallout3,
    element_at, element_count, embedded_script_after_load, fact_after_load_fallout3, flst_edid_after_set,
    gmst_edid_after_set, head_parts_after_set, info_after_load_fallout3, mgef_after_load_fallout3,
    mgef_archtype_after_set, mgef_assoc_item_after_set, npc_after_load_fallout3, pack_after_load_fallout3, path_exists,
    perk_data_function_after_set, perk_entry_point_after_set, perk_epft_after_set, refr_after_load_fallout3,
    remove_member, weap_after_load_fallout3, with_internal_edit,
};

/// Upstream `wbConditionsfterLoad`.
pub fn wb_conditionsfter_load(a_element: &ElementRef) {
    condition_after_load_fallout3(a_element);
}

/// Upstream `wbHeadPartsAfterSet`.
pub fn wb_head_parts_after_set(a_element: &ElementRef, _a_old_value: &Variant, _a_new_value: &Variant) {
    head_parts_after_set(a_element);
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
    mgef_assoc_item_after_set(a_element, a_new_value, "Archtype");
}

/// Upstream `wbMGEFArchtypeAfterSet`.
pub fn wb_mgef_archtype_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    mgef_archtype_after_set(a_element, a_old_value, a_new_value, &[(11, 48), (12, 49), (24, 47)]);
}

/// Upstream `wbPERKEntryPointAfterSet`.
pub fn wb_perk_entry_point_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    let entry_points: Vec<(usize, i64)> = WB_PERK_ENTRY_POINTS
        .iter()
        .map(|entry| (entry.condition as usize, entry.function_type as i64))
        .collect();
    let conditions: Vec<(i64, &str, &str)> = WB_PERK_CONDITION
        .iter()
        .map(|condition| (i64::from(condition.count), condition.caption2, condition.caption3))
        .collect();
    let functions: Vec<i64> = WB_PERK_FUNCTIONS
        .iter()
        .map(|function| function.function_type as i64)
        .collect();
    perk_entry_point_after_set(
        a_element,
        a_old_value,
        a_new_value,
        &entry_points,
        &conditions,
        &functions,
    );
}

/// Upstream `wbPerkDATAFunctionAfterSet`.
pub fn wb_perk_data_function_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    let param_types: Vec<i64> = WB_PERK_FUNCTIONS
        .iter()
        .map(|function| function.param_type as i64)
        .collect();
    perk_data_function_after_set(a_element, a_old_value, a_new_value, &param_types);
}

/// Upstream `wbPerkEPFTAfterSet`.
pub fn wb_perk_epft_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    perk_epft_after_set(a_element, a_old_value, a_new_value);
}

/// Upstream `wbMGEFAfterLoad`.
pub fn wb_mgef_after_load(a_element: &ElementRef) {
    mgef_after_load_fallout3(
        a_element,
        &[
            (1, -1),
            (2, -1),
            (3, -1),
            (13, -1),
            (16, -1),
            (17, -1),
            (18, -1),
            (19, -1),
            (30, -1),
            (31, -1),
            (32, -1),
            (33, -1),
            (11, 48),
            (12, 49),
            (24, 47),
        ],
    );
}

/// Upstream `wbPACKAfterLoad`.
pub fn wb_pack_after_load(a_element: &ElementRef) {
    pack_after_load_fallout3(a_element);
}

/// Upstream `wbNPCAfterLoad`.
pub fn wb_npc_after_load(a_element: &ElementRef) {
    npc_after_load_fallout3(a_element);
}

/// Upstream `wbREFRAfterLoad`.
pub fn wb_refr_after_load(a_element: &ElementRef) {
    refr_after_load_fallout3(a_element);
}

/// Upstream `wbINFOAfterLoad`.
pub fn wb_info_after_load(a_element: &ElementRef) {
    info_after_load_fallout3(a_element);
}

/// Upstream `wbCELLAfterLoad`.
pub fn wb_cell_after_load(a_element: &ElementRef) {
    cell_after_load_fallout3(a_element);
}

/// Upstream `wbEmbeddedScriptAfterLoad`.
pub fn wb_embedded_script_after_load(a_element: &ElementRef) {
    embedded_script_after_load(a_element);
}

/// Upstream `wbWEAPAfterLoad`.
pub fn wb_weap_after_load(a_element: &ElementRef) {
    weap_after_load_fallout3(a_element, false);
}

/// Upstream `wbEFSHAfterLoad`.
pub fn wb_efsh_after_load(a_element: &ElementRef) {
    efsh_after_load_fallout3(a_element);
}

/// Upstream `wbFACTAfterLoad`.
pub fn wb_fact_after_load(a_element: &ElementRef) {
    fact_after_load_fallout3(a_element);
}

/// Upstream `wbEFITAfterLoad`.
pub fn wb_efit_after_load(a_element: &ElementRef) {
    efit_after_load(a_element);
}

/// Upstream `wbWATRAfterLoad`: a legacy `DATA - Visual Data` becomes
/// `DNAM - Visual Data`.
pub fn wb_watr_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        if a_element.as_main_record().is_none() {
            return;
        }
        let record = a_element.clone();
        if !path_exists(&record, "DATA - Visual Data") {
            return;
        }
        let container = record.as_container().expect("a main record is a container");
        if container.get_element_by_name("DNAM - Visual Data").is_none() {
            add_member(&record, "DNAM");
        }
        let (Some(dnam), Some(data)) = (
            container.get_element_by_name("DNAM - Visual Data"),
            container.get_element_by_name("DATA - Visual Data"),
        ) else {
            return;
        };
        // UPSTREAM-QUIRK: the loop stops one member before the last.
        for index in 0..element_count(&data) - 1 {
            if let (Some(target), Some(source)) = (element_at(&dnam, index), element_at(&data, index)) {
                let _ = target.assign_from(&source);
            }
        }
        remove_member(&record, "DATA - Visual Data");
    });
}
