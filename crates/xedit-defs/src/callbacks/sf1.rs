// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsSF1.pas

//! The callbacks of `wbDefinitionsSF1.pas` that are ported by hand. The
//! ones that are not ported yet are stubs in `sf1_stubs.rs`.

pub use super::sf1_stubs::*;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use xedit_core::container_handler::open_resource_last;
use xedit_core::interface::misc::{int_to_hex64, progress};
use xedit_core::interface::string::to_comma_text;
use xedit_core::interface::*;

use super::common::wb_try_get_containing_main_record;
use crate::signatures::QUST;

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
