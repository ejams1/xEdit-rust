// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsTES4.pas

//! The callbacks of `wbDefinitionsTES4.pas` that are ported by hand. The
//! ones that are not ported yet are stubs in `tes4_stubs.rs`.

// The stubs of the callbacks not ported yet; empty once every callback is ported.
#[allow(unused_imports)]
pub use super::tes4_stubs::*;

use xedit_core::interface::misc::int_to_hex64;
use xedit_core::interface::string::to_comma_text;
use xedit_core::interface::*;

use super::common::{
    variant_int, wb_try_get_container_from_union, wb_try_get_container_ref_from_union_or_value,
    wb_try_get_container_with_valid_main_record, wb_try_get_containing_main_record, wb_try_get_main_record,
};
use crate::signatures::QUST;
use crate::tes4::{WB_CONDITION_FUNCTIONS, wb_condition_desc_from_index};

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

/// Upstream `wbConditionFunctionToInt`.
pub fn wb_condition_function_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    for function in WB_CONDITION_FUNCTIONS {
        if function.name.eq_ignore_ascii_case(a_string) {
            return i64::from(function.index);
        }
    }
    str_to_int64(a_string).unwrap_or(0)
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
            CallbackType::ctToStr => format!(
                "{a_int} <Warning: \"{}\" is not a Quest record>",
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
                    "{a_int} <Warning: \"{}\" does not contain a SCRI subrecord>",
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

/// Upstream `wbIdleAnam`: the animation group section and its flag.
pub fn wb_idle_anam(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    let section = a_int & !0x80;
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            let mut result = match section {
                0 => "Lower Body".to_owned(),
                1 => "Left Arm".to_owned(),
                2 => "Left Hand".to_owned(),
                3 => "Right Arm".to_owned(),
                4 => "Special Idle".to_owned(),
                5 => "Whole Body".to_owned(),
                6 => "Upper Body".to_owned(),
                _ => format!("<Unknown: {section}>"),
            };
            if a_int & 0x80 == 0 {
                result.push_str(", Must return a file");
            }
            result
        }
        CallbackType::ctToSortKey => int_to_hex64(a_int, 2),
        CallbackType::ctCheck if (0..=6).contains(&section) => String::new(),
        CallbackType::ctCheck => format!("<Unknown: {section}>"),
        _ => String::new(),
    }
}

/// Upstream `wbCalcPGRRSize`: the connection count of the point of the path
/// grid that the element belongs to.
pub fn wb_calc_pgrr_size(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    let Some(container) = element.get_container() else {
        return 0;
    };
    let count = container
        .as_container()
        .map_or(0, |container| container.get_element_count());
    // `ExtractCountFromLabel`: one past the number after `#` in the name.
    let name = element.get_name();
    let index = match name.find('#') {
        None => count,
        Some(at) => name[at + 1..].trim().parse::<i32>().map_or(count, |number| number + 1),
    };
    let Some(main_record) = container.get_container().and_then(|record| record.into_main_record()) else {
        return 0;
    };
    main_record
        .get_record_by_signature(Signature::new(b"PGRP"))
        .and_then(|pgrp| pgrp.as_container()?.get_element(index - 1))
        .and_then(|point| point.as_container()?.get_element(3))
        .map_or(0, |connections| variant_int(&connections.get_native_value()) as u32)
}

/// Upstream `wbMGEFFAssocItemDecider`: the associated item from the bits 16,
/// 17, 18 and 24 of the flags, read through their sort key as upstream does.
pub fn wb_mgeff_assoc_item_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let Some(flags) = container
        .as_container()
        .and_then(|container| container.get_element_by_name("Flags"))
    else {
        return 0;
    };
    let Some(def) = flags.get_value_def() else { return 0 };
    let data = flags.as_data_container().and_then(|data| data.get_data());
    let sort_key = def.to_sort_key(data, Some(&flags), false);
    let bit = |index: usize| sort_key.as_bytes().get(index) == Some(&b'1');
    if bit(16) {
        1
    } else if bit(17) {
        2
    } else if bit(18) {
        3
    } else if bit(24) {
        4
    } else {
        0
    }
}

/// Upstream `wbMISCActorValueDecider`: 1 for a record with the flags `$C0`.
pub fn wb_misc_actor_value_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(main_record) = wb_try_get_containing_main_record(a_element) else {
        return 0;
    };
    i32::from(main_record.get_flags().0 & 0xC0 == 0xC0)
}

/// Upstream `wbPACKPKDTDecider`: 0 for the short form of the subrecord.
pub fn wb_packpkdt_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 1;
    };
    if container.get_sub_record_header_size() == Some(4) {
        0
    } else {
        1
    }
}

/// Upstream `wbXLOCFillerDecider`: 1 for the long form of the subrecord.
pub fn wb_xloc_filler_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    i32::from(container.get_sub_record_header_size() == Some(16))
}

// ----- the editing callbacks -----

use super::common::{
    add_member, as_container_ref, element_at, element_count, path_int, path_native, remove_member, set_path_native,
    with_internal_edit,
};

/// Upstream `wbCELLAfterLoad`: an interior cell gets lighting and loses
/// regions; an exterior cell loses its climate, gets the "has water" flag
/// and keeps only the regions that are regions.
pub fn wb_cell_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        let interior = path_int(&record, "DATA") & 1 != 0;
        if interior {
            add_member(&record, "XCLL");
            remove_member(&record, "XCLR");
        } else {
            remove_member(&record, "XCCM");
            if path_int(&record, "DATA") & 2 == 0 {
                let data = path_int(&record, "DATA") | 2;
                set_path_native(&record, "DATA", data);
            }
            if let Some(regions) = record
                .as_container()
                .and_then(|c| c.get_element_by_signature(Signature::new(b"XCLR")))
                .and_then(|regions| as_container_ref(&regions))
            {
                for index in (0..element_count(&regions)).rev() {
                    let is_region = element_at(&regions, index)
                        .and_then(|region| region.get_links_to())
                        .and_then(|target| target.into_main_record())
                        .is_some_and(|target| target.get_signature() == Signature::new(b"REGN"));
                    if !is_region {
                        regions.as_container().and_then(|c| c.remove_element_at(index, false));
                    }
                }
                if element_count(&regions) < 1 {
                    regions.remove();
                }
            }
        }
    });
}

/// Upstream `wbEFITAfterLoad`: the actor value of an effect item follows a
/// magic effect that uses one.
pub fn wb_efit_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(container) = as_container_ref(a_element) else {
            return;
        };
        if element_count(&container) < 1 {
            return;
        }
        let name = container
            .as_container()
            .and_then(|c| c.get_element_by_name("Magic effect name"));
        let Some(mgef) = wb_try_get_main_record(name.as_ref(), "MGEF") else {
            return;
        };
        let mgef: ElementRef = mgef;
        if path_int(&mgef, "DATA - Data\\Flags") & 0x0100_0000 == 0 {
            return;
        }
        let actor_value = path_native(&mgef, "DATA - Data\\Assoc. Item");
        if matches!(actor_value, Variant::Empty) {
            return;
        }
        if !actor_value.same_value(&path_native(&container, "Actor Value")) {
            set_path_native(&container, "Actor Value", actor_value);
        }
    });
}

/// Upstream `wbLVLAfterLoad`: the legacy `DATA` is dropped and the "calculate
/// from all levels" bit of the chance moves into the flags.
pub fn wb_lvl_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        remove_member(&record, "DATA");
        let chance = path_int(&record, "LVLD");
        if chance & 0x80 != 0 {
            set_path_native(&record, "LVLD", chance & !0x80);
            let flags = path_int(&record, "LVLF") | 0x01;
            set_path_native(&record, "LVLF", flags);
        }
    });
}

/// Upstream `wbMGEFAfterLoad`: flag fixes for five effects of Oblivion.esm.
pub fn wb_mgef_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(main_record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = main_record.clone();
        let Some(file) = main_record.get_file() else { return };
        if !file.get_name().eq_ignore_ascii_case("Oblivion.esm") {
            return;
        }
        let editor_id = main_record.get_editor_id();
        if ["RSFI", "RSFR", "RSPA", "RSSH"]
            .iter()
            .any(|id| editor_id.eq_ignore_ascii_case(id))
        {
            let flags = path_int(&record, "DATA - Data\\Flags") | 0x8;
            set_path_native(&record, "DATA - Data\\Flags", flags);
        }
        if editor_id.eq_ignore_ascii_case("REAN") {
            let flags = path_int(&record, "DATA - Data\\Flags") & !0x20000;
            set_path_native(&record, "DATA - Data\\Flags", flags);
        }
    });
}

/// Upstream `wbPGRDAfterLoad`: a path grid gets its `PGAG`, is compressed,
/// and loses the unused connections of its points.
pub fn wb_pgrd_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(main_record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = main_record.clone();
        let by_signature = |signature: &[u8; 4]| {
            record
                .as_container()
                .and_then(|c| c.get_element_by_signature(Signature::new(signature)))
                .and_then(|element| as_container_ref(&element))
        };
        let Some(points) = by_signature(b"PGRP") else { return };
        if !record.as_container().is_some_and(|c| c.get_element_exists("PGAG"))
            && let Some(pgag) = add_member(&record, "PGAG")
        {
            let _ = pgag.set_data_size((element_count(&points) + 7) / 8);
        }
        main_record.set_is_compressed(true);
        let Some(connections) = by_signature(b"PGRR") else {
            return;
        };
        if element_count(&points) < element_count(&connections) {
            return;
        }
        let mut first_removed = false;
        for index in (0..element_count(&connections)).rev() {
            let Some(connection) = element_at(&connections, index).and_then(|c| as_container_ref(&c)) else {
                continue;
            };
            let mut removed = false;
            let mut j = element_count(&connection);
            while j > 0 {
                j -= 1;
                let Some(entry) = element_at(&connection, j) else { break };
                if variant_int(&entry.get_native_value()) == 65535 {
                    if !first_removed {
                        first_removed = true;
                        connections.mark_modified_recursive();
                    }
                    entry.remove();
                    removed = true;
                } else {
                    break;
                }
            }
            if removed && let Some(point) = element_at(&points, index).and_then(|p| as_container_ref(&p)) {
                set_path_native(&point, "Connections", i64::from(element_count(&connection)));
            }
        }
    });
}

/// Upstream `wbPGRIPointerAfterLoad`: duplicate inter-cell connections are
/// dropped.
pub fn wb_pgri_pointer_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(connections) = as_container_ref(a_element) else {
            return;
        };
        let mut keys: Vec<String> = Vec::new();
        for index in (0..element_count(&connections)).rev() {
            let Some(connection) = element_at(&connections, index) else {
                continue;
            };
            let key = connection.get_sort_key(true);
            if keys.contains(&key) {
                connections
                    .as_container()
                    .and_then(|c| c.remove_element_at(index, true));
            } else {
                keys.push(key);
            }
        }
    });
}

/// Upstream `wbREFRAfterLoad`: `XPCI` is dropped.
pub fn wb_refr_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = as_container_ref(a_element) else {
            return;
        };
        if element_count(&record) < 1 {
            return;
        }
        remove_member(&record, "XPCI");
    });
}
