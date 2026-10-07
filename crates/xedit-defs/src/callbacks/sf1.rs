// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsSF1.pas

//! The callbacks of `wbDefinitionsSF1.pas` that are ported by hand. The
//! ones that are not ported yet are stubs in `sf1_stubs.rs`.

// The stubs of the callbacks not ported yet; empty once every callback is ported.
#[allow(unused_imports)]
pub use super::sf1_stubs::*;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use xedit_core::container_handler::open_resource_last;
use xedit_core::delphi::{float_to_str_f_fixed, format_general, round, str_to_float};
use xedit_core::interface::constructors::radians_to_degrees_scale;
use xedit_core::interface::globals::more_info_for_decider;
use xedit_core::interface::misc::{int_to_hex64, progress, str_to_int_def};
use xedit_core::interface::string::to_comma_text;
use xedit_core::interface::*;

use super::common::{
    index_key_from_ordinal, variant_int, wb_to_string_from_links_to_main_record_name, wb_try_get_container_from_union,
    wb_try_get_container_ref_from_union_or_value, wb_try_get_containing_main_record, wb_try_get_main_record,
};
use crate::common::{wb_idx_addon_node, wb_idx_collision_layer};
use crate::sf1::{
    TConditionParameterType, WB_CONDITION_FUNCTIONS, WB_EVENT_FUNCTION_ENUM, WB_EVENT_MEMBER_ENUM,
    wb_condition_desc_from_index,
};
use crate::signatures::{NAME, PRKE, QUST, SPQU};

/// Upstream `CmpW32` of `wbInterface`: -1, 0 or 1 for two unsigned values.
pub fn cmp_w32(a: u32, b: u32) -> i32 {
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Upstream `csPropertyCount`.
pub const CS_PROPERTY_COUNT: &str = "Property Count";
/// Upstream `csIncludeCount`.
pub const CS_INCLUDE_COUNT: &str = "Include Count";

/// One entry of upstream `wbWwiseGUIDs`: the name and object path of a Wwise
/// object of the sound bank info.
#[derive(Clone)]
struct WwiseObject {
    name: String,
    object_path: String,
}

/// Upstream `wbWwiseGUIDs`, by the GUID in upper case with braces.
static WWISE_GUIDS: RwLock<Option<HashMap<String, WwiseObject>>> = RwLock::new(None);

/// `TJsonObject.S[aName]`: the string of a member, empty when it is missing.
/// A number or a boolean reads as its JSON text.
fn json_string(object: &serde_json::Map<String, serde_json::Value>, name: &str) -> String {
    match object.get(name) {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// `TJsonBaseObject.Iterate`: every object and array, the container first,
/// then its members in order.
fn json_iterate(value: &serde_json::Value, visit: &mut impl FnMut(&serde_json::Map<String, serde_json::Value>)) {
    match value {
        serde_json::Value::Object(object) => {
            visit(object);
            for member in object.values() {
                json_iterate(member, visit);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                json_iterate(item, visit);
            }
        }
        _ => {}
    }
}

/// `StringToGUID` followed by the key of the dictionary: the GUID in upper
/// case. `None` for text that is not a GUID in braces.
fn guid_key(text: &str) -> Option<String> {
    let inner = text.strip_prefix('{')?.strip_suffix('}')?;
    let parts: Vec<&str> = inner.split('-').collect();
    let lengths = [8, 4, 4, 4, 12];
    if parts.len() != 5
        || parts
            .iter()
            .zip(lengths)
            .any(|(part, length)| part.len() != length || !part.chars().all(|c| c.is_ascii_hexdigit()))
    {
        return None;
    }
    Some(text.to_ascii_uppercase())
}

/// Upstream anonymous routine at line 8816 of `wbDefinitionsSF1.pas`: the
/// resources loaded handler that indexes the Wwise sound bank info by GUID.
pub fn define_sf1_anonymous_8816() {
    let data = open_resource_last("sound\\soundbanks\\soundbanksinfo.json").unwrap_or_default();
    if data.is_empty() {
        progress("Warning: Could not find Wwise Soundbank Info.");
        return;
    }
    progress("Loading Wwise Soundbank Info...");
    let root: serde_json::Value = match serde_json::from_slice(&data) {
        Ok(root) => root,
        Err(error) => {
            progress(&format!(
                "Error: Loading Wwise Soundbank Info failed: [EJsonParserException] {error}"
            ));
            return;
        }
    };
    progress("Building Wwise GUID Index...");
    let mut guids: HashMap<String, WwiseObject> = HashMap::new();
    json_iterate(&root, &mut |object| {
        let guid = json_string(object, "GUID");
        if guid.is_empty() {
            return;
        }
        // UPSTREAM-QUIRK: `StringToGUID` raises on text that is not a GUID,
        // which ends the indexing; the sound bank info has none such.
        let Some(key) = guid_key(&guid) else { return };
        let entry = WwiseObject {
            name: json_string(object, "Name"),
            object_path: json_string(object, "ObjectPath"),
        };
        match guids.get(&key) {
            None => {
                guids.insert(key, entry);
            }
            // An object with a name replaces one without.
            Some(existing) if !entry.name.is_empty() && existing.name.is_empty() => {
                guids.insert(key, entry);
            }
            Some(existing) if !entry.name.is_empty() && existing.name != entry.name => {
                // UPSTREAM-QUIRK: the warning prints the signature `LNAM`
                // where the new name was meant.
                progress(&format!(
                    "Warning: Multiple names for GUID {guid}: [{}] <> [LNAM]",
                    existing.name
                ));
            }
            Some(_) => {}
        }
    });
    progress(&format!("Indexed {} GUIDs successfully.", guids.len()));
    *WWISE_GUIDS.write().unwrap() = Some(guids);
}

/// Upstream `wbWwiseGuidToStr`: the name and object path of the Wwise object
/// of the GUID.
pub fn wb_wwise_guid_to_str(a_value: &mut String, _a_base_ptr: DataPtr, _a_element: ElementArg, a_type: CallbackType) {
    let guids = WWISE_GUIDS.read().unwrap();
    let Some(guids) = guids.as_ref() else { return };
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => {
            if a_value.is_empty() {
                return;
            }
            if a_value == "{00000000-0000-0000-0000-000000000000}" {
                a_value.clear();
                return;
            }
            let Some(object) = guid_key(a_value).and_then(|key| guids.get(&key)) else {
                return;
            };
            if !object.name.is_empty() {
                if a_type == CallbackType::ctToSummary {
                    *a_value = object.name.clone();
                    return;
                }
                *a_value = format!("{} {a_value}", object.name);
            }
            if !object.object_path.is_empty() {
                let mut path = object.object_path.clone();
                if a_type == CallbackType::ctToEditValue && path.chars().count() > 64 {
                    path = format!("{}...", path.chars().take(61).collect::<String>());
                }
                *a_value = format!("{a_value} \"{path}\"");
            }
        }
        CallbackType::ctFromEditValue => {
            if a_value.is_empty() {
                return;
            }
            let Some(start) = a_value.find('{') else { return };
            a_value.drain(..start);
            let Some(end) = a_value.find('}') else { return };
            a_value.truncate(end + 1);
        }
        CallbackType::ctEditType => *a_value = "ComboBox".to_owned(),
        _ => {}
    }
}

/// Upstream `wbWwiseGUID` for a subrecord. The static edit info of the GUIDs
/// is only for the editor and is not set.
pub fn wb_wwise_guid_signature(
    a_signature: Signature,
    a_name: &str,
    a_priority: ConflictPriority,
    a_required: bool,
    a_dont_show: Option<DontShowCallback>,
    a_get_cp: Option<GetConflictPriority>,
) -> Option<Arc<SubRecordDef>> {
    wb_guid_signature(a_signature, a_name, a_priority, a_required, a_dont_show, a_get_cp)
        .map(|def| def.set_to_str(Some(Arc::new(wb_wwise_guid_to_str))))
}

/// Upstream `wbWwiseGUID` for a value. The static edit info of the GUIDs is
/// only for the editor and is not set.
pub fn wb_wwise_guid(
    a_name: &str,
    a_priority: ConflictPriority,
    a_required: bool,
    a_dont_show: Option<DontShowCallback>,
    a_get_cp: Option<GetConflictPriority>,
) -> Option<Arc<GuidDef>> {
    wb_guid(a_name, a_priority, a_required, a_dont_show, a_get_cp)
        .map(|def| def.set_to_str(Some(Arc::new(wb_wwise_guid_to_str))))
}

/// Upstream `wbQuestStageToStr`: the stage of `a_quest` with its log entry.
pub fn wb_quest_stage_to_str(
    a_stage_index: i64,
    _a_element: ElementArg,
    a_type: CallbackType,
    a_source_name: &str,
    a_quest: Option<MainRecordRef>,
    a_allow_none: bool,
) -> String {
    let none = a_allow_none && a_stage_index < 0;
    let mut result = String::new();
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            result = a_stage_index.to_string();
            if none {
                return format!("{result} NONE");
            }
            if a_type == CallbackType::ctToStr {
                result.push_str(&format!(" <Warning: Could not resolve {a_source_name}>"));
            }
        }
        CallbackType::ctToEditValue => {
            result = a_stage_index.to_string();
            if none {
                return result;
            }
        }
        CallbackType::ctToSortKey => return int_to_hex64(a_stage_index, 8),
        CallbackType::ctCheck => {
            if none {
                return result;
            }
            result = format!("<Warning: Could not resolve {a_source_name}>");
        }
        _ => {}
    }
    let Some(quest) = a_quest else { return result };
    if quest.get_signature() != QUST {
        return match a_type {
            CallbackType::ctToStr => format!(
                "{a_stage_index} <Warning: \"{}\" is not a Quest record>",
                quest.get_short_name()
            ),
            CallbackType::ctToSummary => a_stage_index.to_string(),
            CallbackType::ctCheck => format!("<Warning: \"{}\" is not a Quest record>", quest.get_short_name()),
            _ => result,
        };
    }
    if a_type == CallbackType::ctEditType {
        return "ComboBox".to_owned();
    }
    let edit_info = a_type == CallbackType::ctEditInfo;
    let mut edit_infos = Vec::new();
    if a_allow_none && edit_info {
        edit_infos.push("-1 NONE".to_owned());
    }
    if let Some(stages) = quest.get_element_by_name("Stages")
        && let Some(stages) = stages.as_container()
    {
        for index in 0..stages.get_element_count() {
            let Some(stage) = stages.get_element(index) else {
                continue;
            };
            let Some(stage) = stage.as_container() else { continue };
            let Some(stage_index) = stage.get_element_native_value("INDX\\Stage Index").as_ordinal() else {
                continue;
            };
            let stage_index = i64::from(stage_index as i32);
            if !edit_info && stage_index != a_stage_index {
                continue;
            }
            let mut text = format!("{stage_index:0>3}");
            let log_entry = stage
                .get_element_by_path("Log Entries\\Log Entry\\NAM2")
                .map(|entry| entry.get_value())
                .unwrap_or_default();
            let log_entry = log_entry.trim_matches(|c: char| c <= ' ');
            if !log_entry.is_empty() {
                text = format!("{text} {log_entry}");
            }
            if edit_info {
                edit_infos.push(text);
            } else {
                return match a_type {
                    CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => text,
                    _ => String::new(),
                };
            }
        }
    }
    match a_type {
        CallbackType::ctToStr => format!(
            "{a_stage_index} <Warning: Quest Stage [{a_stage_index}] not found in \"{}\">",
            quest.get_name()
        ),
        CallbackType::ctToSummary => a_stage_index.to_string(),
        CallbackType::ctCheck => format!(
            "<Warning: Quest Stage [{a_stage_index}] not found in \"{}\">",
            quest.get_name()
        ),
        CallbackType::ctEditInfo => {
            edit_infos.sort_by_key(|text| text.to_lowercase());
            to_comma_text(&edit_infos)
        }
        _ => result,
    }
}

/// Upstream `wbResolveSnapTemplateNodeFromReference`: the node with the ID of
/// the snap template of the base record of the reference.
pub fn wb_resolve_snap_template_node_from_reference(
    a_reference: Option<&MainRecordRef>,
    a_node_id: i64,
) -> Option<ElementRef> {
    let base_record = a_reference?.get_base_record()?;
    let snap_template = base_record
        .get_element_by_signature(Signature::new(b"SNTP"))?
        .get_links_to()?
        .into_main_record()?;
    let nodes = snap_template.get_element_by_name("Nodes")?;
    let nodes = nodes.as_container()?;
    for index in 0..nodes.get_element_count() {
        let Some(node) = nodes.get_element(index) else { continue };
        let Some(container) = node.as_container() else { continue };
        if container.get_element_native_value("Node ID").as_ordinal() == Some(a_node_id) {
            return Some(node);
        }
    }
    None
}

/// Upstream `wbLinksToNodeId`: the snap template node with the ID of the
/// element, of the reference that holds it or that `a_reference_path` links
/// to.
pub fn wb_links_to_node_id(a_reference_path: &str) -> Option<LinksToCallback> {
    let reference_path = a_reference_path.to_owned();
    Some(Arc::new(move |a_element: ElementArg| -> Option<ElementRef> {
        let main_record = if reference_path == "..." {
            wb_try_get_containing_main_record(a_element)?
        } else {
            a_element?
                .get_container()?
                .as_container()?
                .get_element_by_path(&reference_path)?
                .get_links_to()?
                .into_main_record()?
        };
        let node_id = a_element?.get_native_value().as_ordinal()?;
        wb_resolve_snap_template_node_from_reference(Some(&main_record), node_id)
    }))
}

/// `Container.ElementNativeValues[aPath]` as an integer, 0 when missing.
fn container_int(container: &ElementRef, path: &str) -> i64 {
    container
        .as_container()
        .map_or(0, |container| variant_int(&container.get_element_native_value(path)))
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

/// Upstream `wbConditionFunctionToInt`.
pub fn wb_condition_function_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    for function in WB_CONDITION_FUNCTIONS {
        if function.name.eq_ignore_ascii_case(a_string) {
            return i64::from(function.index);
        }
    }
    a_string.trim().parse().unwrap_or(0)
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

/// Upstream `wbHexStrToInt`: the hexadecimal number before a space or colon.
pub fn wb_hex_str_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    let end = a_string.find(' ').or_else(|| a_string.find(':'));
    let text = match end {
        Some(end) => &a_string[..end],
        None => a_string,
    };
    i64::from_str_radix(text.trim(), 16).unwrap_or(0)
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

/// Upstream `wbStringToInt`.
pub fn wb_string_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    i64::from(str_to_int_def(a_string, 0))
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

/// Upstream `wbINFOGroupDecider`: 1 for a record with flag `$40`.
pub fn wb_info_group_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match a_element.and_then(|element| element.get_containing_main_record()) {
        Some(main_record) if main_record.get_flags().0 & 0x40 != 0 => 1,
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

/// Upstream `wbConditionParam1Decider`: the parameter type of the function
/// of the condition, with the alias and packdata flags applied.
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
            let run_on = container_int(&container, "Run On");
            if run_on == 14 && desc.name == "GetDistance" {
                param_type = TConditionParameterType::ptAlias;
            } else if !(run_on == 5 && desc.name == "GetIsCurrentPackage") {
                // Except for this function when Run On = Quest Alias: then the
                // alias is parameter 3 and the package is parameter 1.
                param_type = TConditionParameterType::ptAlias;
            }
        } else if param_flag & 0x08 > 0 {
            param_type = TConditionParameterType::ptPackdata;
        }
    }
    param_type as i32 + 1
}

/// The quest a record of a group of the children of a quest belongs to.
fn parent_quest(main_record: &MainRecordRef) -> Option<i64> {
    let group = main_record.get_container()?.as_element_impl()?.group_record_impl()?;
    let quest = group.children_of()?;
    (quest.get_signature() == QUST).then(|| i64::from(quest.get_fixed_form_id().to_cardinal()))
}

/// Upstream `wbConditionQuestOverlay`: a null quest stands for the quest the
/// condition belongs to.
pub fn wb_condition_quest_overlay(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> i64 {
    if a_int != 0
        || !matches!(
            a_type,
            CallbackType::ctCheck
                | CallbackType::ctToStr
                | CallbackType::ctToSummary
                | CallbackType::ctToSortKey
                | CallbackType::ctLinksTo
        )
    {
        return a_int;
    }
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return a_int;
    };
    let native = |record: &MainRecordRef, signature: &[u8; 4]| {
        record
            .get_element_by_signature(Signature::new(signature))
            .map(|element| variant_int(&element.get_native_value()))
    };
    match main_record.get_signature().0.as_slice() {
        b"QUST" => i64::from(main_record.get_fixed_form_id().to_cardinal()),
        b"SCEN" => native(&main_record, b"PNAM")
            .or_else(|| parent_quest(&main_record))
            .unwrap_or(a_int),
        b"PACK" => native(&main_record, b"QNAM").unwrap_or(a_int),
        b"INFO" => {
            // The DIAL of the INFO.
            let dial = main_record
                .get_container()
                .and_then(|container| container.as_element_impl()?.group_record_impl())
                .and_then(|group| group.children_of());
            match dial {
                Some(dial) if dial.get_signature() == Signature::new(b"DIAL") => {
                    let dial: MainRecordRef = dial;
                    native(&dial, b"QNAM").or_else(|| parent_quest(&dial)).unwrap_or(a_int)
                }
                _ => a_int,
            }
        }
        _ => a_int,
    }
}

/// Upstream `wbConditionQuestStageToStr`: the stage of the quest of
/// parameter 1.
pub fn wb_condition_quest_stage_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let quest = super::common::wb_try_get_container_ref_from_union_or_value(a_element).and_then(|container| {
        let parameter = container
            .as_container()
            .and_then(|container| container.get_element_by_name("Parameter #1"));
        wb_try_get_main_record(parameter.as_ref(), "")
    });
    wb_quest_stage_to_str(a_int, a_element, a_type, "Quest in Parameter #1", quest, true)
}

/// Upstream `wbPubPackCNAMDecider`: the member for the `ANAM` type name of
/// the container of the union.
pub fn wb_pub_pack_cnam_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(anam) = wb_try_get_container_from_union(a_element)
        .and_then(|container| container.get_container())
        .and_then(|container| {
            container
                .as_container()?
                .get_record_by_signature(Signature::new(b"ANAM"))
        })
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

/// Upstream `wbObjectModPropertiesDecider`: the member for the signature of
/// the record.
pub fn wb_object_mod_properties_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(main_record) = wb_try_get_containing_main_record(a_element) else {
        return 0;
    };
    match main_record.get_signature().0.as_slice() {
        b"WEAP" => 1,
        b"ARMO" => 2,
        b"NPC_" => 3,
        _ => 0,
    }
}

/// The `Archetype` of a magic effect, from the element or at offset 80 of
/// the data, for the associated item deciders.
fn mgef_archetype(a_base_ptr: DataPtr, a_element: ElementArg) -> Option<i64> {
    const OFFSET_ARCHTYPE: usize = 80;
    let container = wb_try_get_container_from_union(a_element)?;
    match container
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
    }
}

/// Upstream `wbMGEFAssocItemDecider1`.
pub fn wb_mgef_assoc_item_decider1(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match mgef_archetype(a_base_ptr, a_element) {
        Some(12) => 1,  // Light
        Some(17) => 2,  // Bound Item
        Some(18) => 3,  // Summon Creature
        Some(25) => 4,  // Guide
        Some(26) => 5,  // Unknown 26
        Some(34) => 8,  // Peak Mod
        Some(35) => 5,  // Cloak
        Some(39) => 7,  // Enhance Weapon
        Some(40) => 4,  // Spawn Hazard
        Some(45) => 9,  // Damage Type
        Some(46) => 6,  // Immunity
        Some(54) => 10, // TrackDamage
        Some(55) => 11, // GravWielder
        _ => 0,
    }
}

/// Upstream `wbMGEFAssocItemDecider2`: 1 for the value modifier archetypes.
pub fn wb_mgef_assoc_item_decider2(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    i32::from(matches!(
        mgef_archetype(a_base_ptr, a_element),
        Some(0 | 4 | 5 | 32 | 34 | 48 | 50)
    ))
}

/// Upstream `wbMGEFAssocItemDecider3`: 1 for the dual value modifier.
pub fn wb_mgef_assoc_item_decider3(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    i32::from(mgef_archetype(a_base_ptr, a_element) == Some(5))
}

/// Upstream `wbVLMSTypeDecider`.
pub fn wb_vlms_type_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    match container_int(&container, "Type") {
        3 => 1,
        5 => 2,
        _ => 0,
    }
}

/// Upstream `wbINNRTargetDecider`: the member for the `UNAM` target type.
pub fn wb_innr_target_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return 0;
    };
    let target = main_record
        .get_element_by_signature(Signature::new(b"UNAM"))
        .map_or(0, |unam| variant_int(&unam.get_native_value()));
    match target {
        0x22 => 1, // Armor
        0x24 => 2, // Container
        0x2E => 3, // Flora
        0x2F => 4, // Furniture
        0x30 => 5, // Weapon
        0x32 => 6, // Actor
        _ => 0,
    }
}

/// The component name of the `BFCB` of the component that holds the union,
/// for the component data deciders.
fn component_name(a_element: ElementArg) -> Option<String> {
    let container = wb_try_get_container_from_union(a_element)?
        .get_container()?
        .get_container()?;
    let container = container.as_container()?;
    if container.get_element_count() < 2 {
        return None;
    }
    Some(container.get_element(0)?.get_edit_value())
}

/// Upstream `wbBFCDATADecider`.
pub fn wb_bfcdata_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match component_name(a_element).as_deref() {
        Some("BGSStarDataComponent_Component") => 1,
        Some("BGSOrbitedDataComponent_Component") => 2,
        Some("BGSOrbitalDataComponent_Component") => 3,
        Some("UniqueOverlayList_Component") => 4,
        Some("UniquePatternPlacementInfo_Component") => 5,
        Some("BGSOverlayDesignatedPlacementInfo_Component") => 6,
        _ => 0,
    }
}

/// Upstream `wbBFCDAT2Decider`.
pub fn wb_bfcdat2_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match component_name(a_element).as_deref() {
        Some("BlockHeightAdjustment_Component") => 1,
        Some("SurfaceTreePatternSwapInfo_Component") => 2,
        Some("BGSBlockEditorMetaData_Component") => 3,
        _ => 0,
    }
}

/// Upstream `wbTMLMTypeDontShow`.
pub fn wb_tmlm_type_dont_show(a_element: ElementArg) -> bool {
    let Some(container) = a_element.and_then(|element| element.as_container()) else {
        return true;
    };
    !matches!(
        variant_int(&container.get_element_native_value("ISET\\Type")),
        0 | 1 | 5
    )
}

/// Upstream `wbTMLMTypeUnionDecider`.
pub fn wb_tmlm_type_union_decider(a_container: ElementArg) -> i32 {
    let kind = a_container
        .and_then(|container| container.as_container())
        .map_or(0, |container| {
            variant_int(&container.get_element_native_value("...\\ISET\\Type"))
        });
    match kind {
        0 => 0, // Display Text
        1 => 1, // Submenu
        5 => 2, // DataSlate
        _ => 3,
    }
}

/// Upstream `wbQuestAliasExternalAliasLinksTo`.
pub fn wb_quest_alias_external_alias_links_to(a_element: ElementArg) -> Option<ElementRef> {
    if !xedit_core::interface::globals::resolve_alias() {
        return None;
    }
    let element = a_element?;
    let alias = element.get_native_value().as_ordinal()?;
    let container = element.get_container()?;
    let quest = container
        .as_container()?
        .get_element_by_signature(Signature::new(b"ALEQ"));
    super::common::wb_alias_links_to(alias, quest.as_ref())
}

/// Upstream `wbSameQuestAliasLinksTo`.
pub fn wb_same_quest_alias_links_to(a_element: ElementArg) -> Option<ElementRef> {
    if !xedit_core::interface::globals::resolve_alias() {
        return None;
    }
    let element = a_element?;
    let alias = element.get_native_value().as_ordinal()?;
    let quest: Option<ElementRef> = element.get_containing_main_record().map(|record| record as ElementRef);
    super::common::wb_alias_links_to(alias, quest.as_ref())
}

/// Upstream `wbStarIDToStr`: the star ID with the name of the star.
pub fn wb_star_id_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let result = match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary if a_int == -1 => {
            if a_type == CallbackType::ctToSummary {
                String::new()
            } else {
                "Universe".to_owned()
            }
        }
        CallbackType::ctToStr => format!("{a_int} <Warning: Could not resolve Star>"),
        CallbackType::ctToSummary => a_int.to_string(),
        CallbackType::ctToEditValue if a_int == -1 => "Universe".to_owned(),
        CallbackType::ctToEditValue => a_int.to_string(),
        CallbackType::ctToSortKey => return int_to_hex64(a_int, 8),
        CallbackType::ctCheck => {
            let links = a_element.and_then(|element| element.get_links_to());
            return if a_int == -1 || links.is_some() {
                String::new()
            } else {
                format!("<Warning: Could not resolve Star [{a_int}]>")
            };
        }
        _ => String::new(),
    };
    if a_int == -1 && !matches!(a_type, CallbackType::ctEditType | CallbackType::ctEditInfo) {
        return result;
    }
    if a_type == CallbackType::ctEditType {
        return String::new();
    }
    let Some(element) = a_element else { return result };
    let star_id = variant_int(&element.get_native_value());
    let Some(links_to) = element.get_links_to() else {
        return result;
    };
    let Some(container) = wb_try_get_container_from_union(Some(&links_to)) else {
        return result;
    };
    let name = container
        .as_container()
        .and_then(|container| container.get_element_by_path("ANAM"))
        .map(|anam| anam.get_value())
        .unwrap_or_default();
    format!("{star_id} ({name})")
}

/// Upstream `wbStrToStarID`.
pub fn wb_str_to_star_id(a_string: &str, _a_element: ElementArg) -> i64 {
    if a_string == "None" || a_string == "Universe" {
        return -1;
    }
    let s = a_string.trim_matches(|c: char| c <= ' ');
    let digits: String = s.chars().take_while(|c| *c == '-' || c.is_ascii_digit()).collect();
    i64::from(str_to_int_def(&digits, -1))
}

/// Upstream `wbBIOMScaleToStr`: the scale of a biome shown as its inverse in
/// hundredths. Editing it is not ported.
pub fn wb_biom_scale_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    if a_type != CallbackType::ctToStr {
        return;
    }
    let Some(element) = a_element else { return };
    let value = match element.get_native_value() {
        Variant::Float(value) => value,
        other => other.as_ordinal().unwrap_or(0) as f64,
    };
    if value != 0.0 {
        *a_value = float_to_str_f_fixed(0.01 / value, 1);
    }
}

/// Upstream closure `wbRaceOverrideDontShow` of `DefineSF1`: hides a race
/// override member unless its flag is set in the active overrides.
pub fn define_sf1_wb_race_override_dont_show(a_flag: u8) -> Option<DontShowCallback> {
    Some(Arc::new(move |a_element: ElementArg| -> bool {
        let Some(container) = a_element.and_then(|element| element.as_container()) else {
            return false;
        };
        // `Unknown 6` is doubled because of the union.
        let Some(active_overrides) = container
            .get_element_native_value("...\\ONA2\\General\\General\\Active Overrides")
            .as_ordinal()
        else {
            return false;
        };
        active_overrides & (1 << a_flag) == 0
    }))
}

/// Upstream closure `wbLonLanFunc` of `DefineSF1`: shows a longitude or a
/// latitude in radians as degrees, minutes and seconds with the direction.
/// Editing the value is not ported.
pub fn define_sf1_wb_lon_lan_func(a_is_lat: bool) -> Option<ToStrCallback> {
    let full: i64 = if a_is_lat { 180 } else { 360 };
    let half = full / 2;
    let (negative, positive) = if a_is_lat { ('S', 'N') } else { ('W', 'E') };
    Some(Arc::new(
        move |a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType| {
            if !matches!(a_type, CallbackType::ctToStr | CallbackType::ctToSummary) {
                return;
            }
            let Some(Variant::Float(value)) = a_element.map(|element| element.get_native_value()) else {
                return;
            };
            let mut coord = value * radians_to_degrees_scale();
            while coord > half as f64 {
                coord -= full as f64;
            }
            while coord < -(half as f64) {
                coord += full as f64;
            }
            let mut degrees = coord.trunc() as i64;
            let mut minutes = ((coord - degrees as f64).abs() * 60.0).trunc() as i64;
            let mut seconds = round(((coord - degrees as f64).abs() * 60.0 - minutes as f64) * 60.0);
            if seconds == 60 {
                seconds = 0;
                minutes += 1;
                if minutes == 60 {
                    degrees += 1;
                    minutes = 0;
                }
            }
            let direction = if coord >= 0.0 { positive } else { negative };
            *a_value = format!("{}\u{00B0}{minutes}'{seconds}\"{direction}", degrees.abs());
        },
    ))
}

/// Upstream `wbPerkActivityTypes` of `DefineSF1`.
const WB_PERK_ACTIVITY_TYPES: [&str; 30] = [
    "Actor Value",
    "Apply Magic Effect",
    "Barter",
    "Bleedout",
    "Build Workshop Item",
    "Consume",
    "Craft",
    "Cripple Limb",
    "Destroy Ship",
    "Dock Ship",
    "Grav Jump",
    "Harvest",
    "Kill",
    "Land Planet",
    "Location Discovered",
    "Lockpick",
    "LootContainer",
    "Lose Enemy",
    "Player Pickpocket",
    "Produce",
    "Reload Weapon",
    "Research Completed",
    "Scan Planet",
    "Scan Surface",
    "ShipBuilder",
    "ShipCollection",
    "Speech Challenge",
    "Sprint",
    "Take Actor Damage",
    "Take Hit Damage",
];

/// `RecordFromIndexByKey` of the file of the element.
fn record_from_index(element: &ElementRef, index: i32, key: &str) -> Option<ElementRef> {
    let file = element.get_file()?;
    file.get_record_from_index_by_key(index, key)
        .map(|record| record as ElementRef)
}

/// The record of the index that an ordinal value is the key of, for the
/// links-to callbacks of `wbIdxStarID` and `wbIdxCollisionLayer`.
fn ordinal_links_to(a_element: ElementArg, index: i32) -> Option<ElementRef> {
    let element = a_element?;
    let key = element.get_native_value().as_ordinal()?;
    record_from_index(element, index, &key.to_string())
}

/// The record of the index that a string value is the key of.
fn string_links_to(element: &ElementRef, index: i32) -> Option<ElementRef> {
    let Variant::Str(key) = element.get_native_value() else {
        return None;
    };
    record_from_index(element, index, &key)
}

/// `ElementValues[aPath]` of a container: empty without the element.
fn element_value(container: &ElementRef, path: &str) -> String {
    container
        .as_container()
        .and_then(|container| container.get_element_by_path(path))
        .map(|element| element.get_value())
        .unwrap_or_default()
}

/// `ElementValues['...\MNAM']` of the container of the union, `None`
/// without one.
fn union_mnam(a_element: ElementArg) -> Option<String> {
    let container = wb_try_get_container_from_union(a_element)?;
    Some(element_value(&container, "...\\MNAM"))
}

/// Upstream `wbIdxAVMByType` of `DefineSF1`: the index of an `AVMD` type.
fn avm_index_by_type(kind: i64) -> Option<i32> {
    match kind {
        1 => Some(wb_named_index("SimpleGroup", true)),
        2 => Some(wb_named_index("ComplexGroup", true)),
        3 => Some(wb_named_index("Modulation", true)),
        _ => None,
    }
}

/// Upstream anonymous routine at line 2493 of `wbDefinitionsSF1.pas`: the
/// star of the star ID (`wbStarSystemLookup`).
pub fn define_sf1_anonymous_2493(a_element: ElementArg) -> Option<ElementRef> {
    ordinal_links_to(a_element, wb_named_index("StarID", true))
}

/// Upstream anonymous routine at line 4990 of `wbDefinitionsSF1.pas`: the
/// reference only shows when the condition runs on a reference.
pub fn define_sf1_anonymous_4990(a_element: ElementArg) -> bool {
    let Some(element) = a_element.filter(|element| element.as_container().is_some()) else {
        return true;
    };
    element_value(element, "..\\Run On") != "Reference"
}

/// Upstream anonymous routine at line 5367 of `wbDefinitionsSF1.pas`: the
/// member for the perk activity type.
pub fn define_sf1_anonymous_5367(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    let activity_type = element
        .get_container()
        .and_then(|container| container.get_container())
        .and_then(|container| container.as_container()?.get_element(0))
        .map(|element| element.get_value())
        .unwrap_or_default();
    WB_PERK_ACTIVITY_TYPES
        .iter()
        .position(|name| name.eq_ignore_ascii_case(&activity_type))
        .map_or(0, |index| index as i32 + 1)
}

/// Upstream anonymous routine at line 6020 of `wbDefinitionsSF1.pas`
/// (`wbAngleToStr`): an angle in radians as degrees. Editing the value is
/// not ported.
pub fn define_sf1_anonymous_6020(
    a_value: &mut String,
    _a_base_ptr: DataPtr,
    a_element: ElementArg,
    a_type: CallbackType,
) {
    const FULL: f64 = 360.0;
    if !matches!(a_type, CallbackType::ctToStr | CallbackType::ctToSummary) {
        return;
    }
    let Some(Variant::Float(value)) = a_element.map(|element| element.get_native_value()) else {
        return;
    };
    let mut angle = value * radians_to_degrees_scale();
    while angle > FULL {
        angle -= FULL;
    }
    while angle < -FULL {
        angle += FULL;
    }
    *a_value = format!("{}\u{00B0}", format_general(angle, 15));
}

/// Upstream anonymous routine at line 7405 of `wbDefinitionsSF1.pas`: the
/// stage of the quest of the entry.
pub fn define_sf1_anonymous_7405(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let quest = wb_try_get_container_ref_from_union_or_value(a_element).and_then(|container| {
        let element = container.as_container()?.get_element_by_name("Quest");
        wb_try_get_main_record(element.as_ref(), "")
    });
    wb_quest_stage_to_str(a_int, a_element, a_type, "Quest", quest, false)
}

/// Upstream anonymous routine at line 8798 of `wbDefinitionsSF1.pas`: the
/// stage of the quest of the `SPQU` of the record.
pub fn define_sf1_anonymous_8798(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let quest = wb_try_get_containing_main_record(a_element).and_then(|record| {
        let element = record.get_element_by_signature(SPQU);
        wb_try_get_main_record(element.as_ref(), "")
    });
    wb_quest_stage_to_str(a_int, a_element, a_type, "Quest", quest, true)
}

/// Upstream anonymous routine at line 9243 of `wbDefinitionsSF1.pas`: the
/// collision layer of the index.
pub fn define_sf1_anonymous_9243(a_element: ElementArg) -> Option<ElementRef> {
    ordinal_links_to(a_element, wb_idx_collision_layer())
}

/// Upstream anonymous routine at line 9708 of `wbDefinitionsSF1.pas`: the
/// `ADDN` index key.
pub fn define_sf1_anonymous_9708(a_main_record: &MainRecordRef, a_index_keys: &mut IndexKeys) {
    index_key_from_ordinal(a_main_record, a_index_keys, "DATA", wb_idx_addon_node());
}

/// Upstream anonymous routine at line 10083 of `wbDefinitionsSF1.pas`: the
/// `AVMD` of the entry type, in the index of the `MNAM` type.
pub fn define_sf1_anonymous_10083(a_element: ElementArg) -> Option<ElementRef> {
    let element = a_element?;
    let container = element.as_container()?;
    let Variant::Str(name) = element.get_native_value() else {
        return None;
    };
    if name.is_empty() {
        return None;
    }
    element.get_file()?;
    let kind = container.get_element_native_value("...\\MNAM").as_ordinal()?;
    record_from_index(element, avm_index_by_type(kind)?, &name)
}

/// Upstream anonymous routine at line 10364 of `wbDefinitionsSF1.pas`: the
/// `AVMD` of a complex group name without a value.
pub fn define_sf1_anonymous_10364(a_element: ElementArg) -> Option<ElementRef> {
    let container = wb_try_get_container_from_union(a_element)?;
    if element_value(&container, "...\\MNAM") != "Complex Group" {
        return None;
    }
    if container.as_container()?.get_element_exists("..\\VNAM") {
        return None;
    }
    let element = a_element?;
    string_links_to(element, wb_named_index("SimpleGroup", true))
        .or_else(|| string_links_to(element, wb_named_index("ComplexGroup", true)))
        .or_else(|| string_links_to(element, wb_named_index("Modulation", true)))
}

/// Upstream anonymous routine at line 10392 of `wbDefinitionsSF1.pas`.
pub fn define_sf1_anonymous_10392(
    a_value: &mut String,
    a_base_ptr: DataPtr,
    a_element: ElementArg,
    a_type: CallbackType,
) {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return;
    };
    if element_value(&container, "...\\MNAM") != "Complex Group" {
        return;
    }
    if container
        .as_container()
        .is_some_and(|container| container.get_element_exists("..\\VNAM"))
    {
        return;
    }
    wb_to_string_from_links_to_main_record_name(a_value, a_base_ptr, a_element, a_type);
}

/// Upstream anonymous routine at line 10407 of `wbDefinitionsSF1.pas`: the
/// `AVMD` a `<Kind>_<Name>` value names.
pub fn define_sf1_anonymous_10407(a_element: ElementArg) -> Option<ElementRef> {
    if union_mnam(a_element)? != "Complex Group" {
        return None;
    }
    let element = a_element?;
    let Variant::Str(name) = element.get_native_value() else {
        return None;
    };
    // `Pos` is 1-based: the prefix has at least ten characters.
    let underline = name.find('_')?;
    if underline < 10 {
        return None;
    }
    let index = match &name[..underline] {
        "SimpleGroup" => wb_named_index("SimpleGroup", true),
        "ComplexGroup" => wb_named_index("ComplexGroup", true),
        "Modulation" => wb_named_index("Modulation", true),
        _ => return None,
    };
    let rest = &name[underline + 1..];
    if rest.is_empty() {
        return None;
    }
    record_from_index(element, index, rest)
}

/// Upstream anonymous routine at line 10451 of `wbDefinitionsSF1.pas`.
pub fn define_sf1_anonymous_10451(
    a_value: &mut String,
    a_base_ptr: DataPtr,
    a_element: ElementArg,
    a_type: CallbackType,
) {
    if union_mnam(a_element).as_deref() != Some("Complex Group") {
        return;
    }
    wb_to_string_from_links_to_main_record_name(a_value, a_base_ptr, a_element, a_type);
}

/// Upstream anonymous routine at line 10462 of `wbDefinitionsSF1.pas`.
pub fn define_sf1_anonymous_10462(a_element: ElementArg) -> bool {
    union_mnam(a_element).is_some_and(|mnam| mnam == "Modulation")
}

/// Upstream anonymous routine at line 10470 of `wbDefinitionsSF1.pas`.
pub fn define_sf1_anonymous_10470(a_element: ElementArg) -> bool {
    union_mnam(a_element).is_some_and(|mnam| mnam != "Modulation")
}

/// Upstream anonymous routine at line 10500 of `wbDefinitionsSF1.pas`: the
/// index key of the `AVMD` by its `MNAM` type.
pub fn define_sf1_anonymous_10500(a_main_record: &MainRecordRef, a_index_keys: &mut IndexKeys) {
    let Some(kind) = a_main_record.get_element_native_value("MNAM").as_ordinal() else {
        return;
    };
    let name = a_main_record.get_element_edit_value("TNAM");
    if name.is_empty() {
        return;
    }
    if let Some(index) = avm_index_by_type(kind) {
        a_index_keys.set_key(index, &name);
    }
}

/// Upstream anonymous routine at line 10716 of `wbDefinitionsSF1.pas`: the
/// member for the bone modifier type.
pub fn define_sf1_anonymous_10716(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = a_element.and_then(|element| element.as_container()) else {
        return 0;
    };
    let kind = container.get_element_edit_value("...\\Type");
    ["LookAtChain", "MorphDriver", "PoseDeformer", "SpringBone"]
        .iter()
        .position(|name| name.eq_ignore_ascii_case(&kind))
        .map_or(0, |index| index as i32 + 1)
}

/// Upstream anonymous routine at line 11367 of `wbDefinitionsSF1.pas`: the
/// `COLL` index key.
pub fn define_sf1_anonymous_11367(a_main_record: &MainRecordRef, a_index_keys: &mut IndexKeys) {
    index_key_from_ordinal(a_main_record, a_index_keys, "BNAM", wb_idx_collision_layer());
}

/// The ordinal of a subrecord of the container for the record union
/// deciders, -1 without one.
fn record_union_type(a_container: ElementArg, signature: &str) -> i32 {
    a_container
        .and_then(|container| container.as_container())
        .and_then(|container| container.get_element_native_value(signature).as_ordinal())
        .map_or(-1, |kind| kind as i32)
}

/// Upstream anonymous routine at line 12787 of `wbDefinitionsSF1.pas`.
pub fn define_sf1_anonymous_12787(a_container: ElementArg) -> i32 {
    record_union_type(a_container, "TNAM")
}

/// Upstream anonymous routine at line 12846 of `wbDefinitionsSF1.pas`.
pub fn define_sf1_anonymous_12846(a_container: ElementArg) -> i32 {
    record_union_type(a_container, "BNAM")
}

/// Upstream anonymous routine at line 13034 of `wbDefinitionsSF1.pas`.
pub fn define_sf1_anonymous_13034(a_element: ElementArg) -> Option<ElementRef> {
    string_links_to(a_element?, wb_named_index("SimpleGroup", true))
}

/// Upstream anonymous routine at line 13965 of `wbDefinitionsSF1.pas`.
pub fn define_sf1_anonymous_13965(a_element: ElementArg) -> Option<ElementRef> {
    ordinal_links_to(a_element, wb_named_index("StarID", true))
}

/// Upstream anonymous routine at line 18163 of `wbDefinitionsSF1.pas`: the
/// star ID index key.
pub fn define_sf1_anonymous_18163(a_main_record: &MainRecordRef, a_index_keys: &mut IndexKeys) {
    index_key_from_ordinal(a_main_record, a_index_keys, "DNAM", wb_named_index("StarID", true));
}

/// Upstream anonymous routine at line 5930 of `wbDefinitionsSF1.pas`
/// (`wbLinksToBluePrintComponent`): the blueprint component item with the
/// part ID. The base form components are found by the name of their
/// definition, which is unique in `DefineSF1`.
pub fn define_sf1_anonymous_5930(a_element: ElementArg) -> Option<ElementRef> {
    let element = a_element?;
    let value = variant_int(&element.get_native_value());
    if value < 0 {
        return None;
    }
    let mut container = element.get_container();
    while let Some(current) = container.clone() {
        let def = current.get_def()?;
        if def.get_def_type() == DefType::dtRecord {
            return None;
        }
        if def.get_def_type() == DefType::dtSubRecordArray && def.get_name() == "Base Form Components" {
            break;
        }
        container = current.get_container();
    }
    let container = container?;
    let container = container.as_container()?;
    for array_index in 0..container.get_element_count() {
        let Some(component) = container.get_element(array_index) else {
            continue;
        };
        let Some(component) = component.as_container() else {
            continue;
        };
        let is_blueprint = component
            .get_element_by_signature(Signature::new(b"BFCB"))
            .is_some_and(|bfcb| bfcb.get_value() == "Blueprint_Component");
        if !is_blueprint {
            continue;
        }
        let Some(items) = component.get_element_by_path("Component Data\\BUO4 - Blue Print Components") else {
            continue;
        };
        let Some(items) = items.as_container() else { continue };
        let count = i64::from(items.get_element_count());
        let mut item_index = value.min(count - 1);
        let mut move_by = 0;
        loop {
            let item = items.get_element(i32::try_from(item_index).ok()?)?;
            // Upstream repeats the same item forever without a part ID.
            let part = item.as_container()?.get_element_by_name("Part ID")?;
            let diff = value - variant_int(&part.get_native_value());
            if diff == 0 {
                return Some(item);
            }
            if diff < 0 {
                if move_by > 0 {
                    return None;
                }
                move_by = -1;
            } else {
                if move_by < 0 {
                    return None;
                }
                move_by = 1;
            }
            item_index += move_by;
            if item_index < 0 || item_index >= count {
                return None;
            }
        }
    }
    None
}

// ----- the editing callbacks -----

use super::common::{
    as_container_ref, cell_data_after_set, condition_event_to_int, container_of, element_at, flst_edid_after_set,
    flst_lnam_is_sorted, gmst_edid_after_set, package_data_input_value_type_after_set, set_native, set_path_native,
};

/// Upstream `wbConditionEventToInt`.
pub fn wb_condition_event_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    condition_event_to_int(a_string, WB_EVENT_FUNCTION_ENUM.get(), WB_EVENT_MEMBER_ENUM.get())
}

/// The body of the type `AfterSet` callbacks that replace a member by the
/// template of the new type: the member at `sort_order` is removed; the
/// assignment from a template (`GetAssignTemplates`) comes with the copy
/// step of the write path.
fn replace_member_by_template(container: &ElementRef, sort_order: i32) {
    if let Some(member) = container
        .as_container()
        .and_then(|c| c.get_element_by_sort_order(sort_order))
    {
        member.remove();
    }
    xedit_core::interface::misc::progress(&format!(
        "<Warning: the member {sort_order} of {} is not rebuilt from its template; that comes with the copy step>",
        container.get_full_path()
    ));
}

/// Upstream `wbTMLMTypeAfterSet`.
pub fn wb_tmlm_type_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if !(a_old_value.is_ordinal() && a_new_value.is_ordinal()) || a_old_value.same_value(a_new_value) {
        return;
    }
    let Some(parent) = container_of(a_element).and_then(|c| container_of(&c)) else {
        return;
    };
    if let Some(data) = parent.as_container().and_then(|c| c.get_element_by_sort_order(5)) {
        data.remove();
    }
    if matches!(variant_int(&a_element.get_native_value()), 0 | 1 | 5) {
        replace_member_by_template(&parent, 5);
    }
}

/// Upstream `wbGPOFTypeAfterSetCallback`.
pub fn wb_gpof_type_after_set_callback(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if !(a_old_value.is_ordinal() && a_new_value.is_ordinal()) || a_old_value.same_value(a_new_value) {
        return;
    }
    if let Some(container) = container_of(a_element) {
        replace_member_by_template(&container, 1);
    }
}

/// Upstream `wbGPOGTypeAfterSetCallback`.
pub fn wb_gpog_type_after_set_callback(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    wb_gpof_type_after_set_callback(a_element, a_old_value, a_new_value);
}

/// Upstream `wbSCENTimelineTypeAfterSet`.
pub fn wb_scen_timeline_type_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if !(a_old_value.is_ordinal() && a_new_value.is_ordinal()) || a_old_value.same_value(a_new_value) {
        return;
    }
    if let Some(container) = container_of(a_element) {
        replace_member_by_template(&container, 2);
    }
}

/// Upstream `wbGMSTEDIDAfterSet`.
pub fn wb_gmstedid_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    gmst_edid_after_set(a_element, a_old_value, a_new_value);
}

/// Upstream `wbFLSTEDIDAfterSet`.
pub fn wb_flstedid_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    flst_edid_after_set(a_element, a_old_value, a_new_value);
}

/// Upstream `wbMGEFArchtypeAfterSet`: the members that depend on the
/// archetype are zeroed, by index because unions label them.
pub fn wb_mgef_archtype_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    let Some(container) = as_container_ref(a_element) else {
        return;
    };
    let Some(parent) = container_of(&container) else { return };
    if let Some(first) = element_at(&parent, 0) {
        set_native(&first, 0i64);
    }
    if let Some(weight) = element_at(&parent, 17) {
        set_native(&weight, 0i64);
    }
    set_path_native(&container, "..\\Second AV Weight", 0.0f64);
    if !matches!(variant_int(a_new_value), 0 | 4 | 5 | 32 | 34 | 48 | 50)
        && let Some(member) = element_at(&parent, 14)
    {
        set_native(&member, 0i64);
    }
}

/// Upstream `wbFLSTLNAMIsSorted`.
pub fn wb_flstlnam_is_sorted(a_container: ElementArg) -> bool {
    flst_lnam_is_sorted(a_container)
}

/// Upstream `wbCELLDATAAfterSet`.
pub fn wb_celldata_after_set(a_element: &ElementRef, _a_old_value: &Variant, _a_new_value: &Variant) {
    cell_data_after_set(a_element);
}

/// Upstream `wbPackageDataInputValueTypeAfterSet`.
pub fn wb_package_data_input_value_type_after_set(
    a_element: &ElementRef,
    a_old_value: &Variant,
    a_new_value: &Variant,
) {
    package_data_input_value_type_after_set(a_element, a_old_value, a_new_value);
}

/// The perk activity callbacks of Starfield keep a virtual JSON view of
/// the activity data (`wbPerkActivityLoadVirtualJSON`); the reflection JSON
/// is not ported, so they do nothing beyond the upstream early exits.
pub fn define_sf1_anonymous_5247(_a_element: &ElementRef) {}

/// Upstream anonymous `AfterSet` of the perk activity (`ATAV`).
pub fn define_sf1_anonymous_5256(_a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) || xedit_core::interface::globals::is_internal_edit() {
        return;
    }
    xedit_core::interface::misc::progress("<Warning: the perk activity JSON of Starfield is not ported>");
}

/// Upstream anonymous `AfterSet` of the virtual perk activity (`NULL`).
pub fn define_sf1_anonymous_5271(_a_element: &ElementRef, _a_old_value: &Variant, _a_new_value: &Variant) {
    if xedit_core::interface::globals::is_internal_edit() {
        return;
    }
    xedit_core::interface::misc::progress("<Warning: the perk activity JSON of Starfield is not ported>");
}

/// Upstream anonymous `AfterSet` of the virtual perk stream ID: only a
/// master update triggers it, which is not ported.
pub fn define_sf1_anonymous_5287(_a_element: &ElementRef, _a_old_value: &Variant, _a_new_value: &Variant) {}
