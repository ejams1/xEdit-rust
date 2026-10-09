// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Add, remove and copy on synthetic Skyrim SE plugins: a record copied as
//! an override and as a new record, a member and a child record added and
//! removed, a record deleted, a cell copied with its child group, and the
//! saved bytes of each.

use std::sync::Arc;

use serde_json::json;
use xedit_core::implementation::{FileBytes, FileImpl, ResetModified, wb_file_from_bytes};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{Container, Element, File, GameMode, MainRecord};
use xedit_session::{Registry, Session};

fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn main_record(signature: &[u8; 4], flags: u32, form_id: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&form_id.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&44u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn group(label: u32, group_type: i32, records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(&label.to_le_bytes());
    bytes.extend_from_slice(&group_type.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

fn top_group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    group(u32::from_le_bytes(*label), 0, records)
}

fn header(flags: u32, record_count: u32, next_object_id: u32, masters: &[&str]) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&record_count.to_le_bytes());
    hedr.extend_from_slice(&next_object_id.to_le_bytes());
    let mut data = sub_record(b"HEDR", &hedr);
    data.extend(sub_record(b"CNAM", b"xedit-rust\0"));
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        data.extend(sub_record(b"MAST", &name));
        data.extend(sub_record(b"DATA", &0u64.to_le_bytes()));
    }
    data.extend(sub_record(b"INCC", &0u32.to_le_bytes()));
    main_record(b"TES4", flags, 0, &data)
}

fn gmst(form_id: u32, editor_id: &[u8], value: f32) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"DATA", &value.to_le_bytes()));
    main_record(b"GMST", 0, form_id, &data)
}

/// The interior cell `[00000801]`: block 9, sub-block 4 of its object ID.
fn cell() -> Vec<u8> {
    let mut data = sub_record(b"EDID", b"TestCell\0");
    data.extend(sub_record(b"DATA", &1u16.to_le_bytes()));
    main_record(b"CELL", 0, 0x801, &data)
}

/// The master: a game setting and an interior cell without children.
fn master_bytes() -> Vec<u8> {
    let mut bytes = header(1, 5, 0x802, &[]);
    bytes.extend(top_group(b"GMST", &gmst(0x800, b"fMaster\0", 1.5)));
    let cells = group(9, 2, &group(4, 3, &cell()));
    bytes.extend(top_group(b"CELL", &cells));
    bytes
}

/// A plugin of the master with a game setting of its own.
fn plugin_bytes() -> Vec<u8> {
    let mut bytes = header(0, 2, 0x801, &["Master.esm"]);
    bytes.extend(top_group(b"GMST", &gmst(0x0100_0800, b"fPlugin\0", 2.5)));
    bytes
}

/// A plugin without masters.
fn lone_bytes() -> Vec<u8> {
    header(0, 0, 0x800, &[])
}

struct Files {
    master: Arc<FileImpl>,
    plugin: Arc<FileImpl>,
    lone: Arc<FileImpl>,
}

fn load() -> Files {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let open = |name: &str, bytes: Vec<u8>| {
        wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap()
    };
    let master = open("Master.esm", master_bytes());
    xedit_core::interface::add_file(master.clone());
    let plugin = open("Plugin.esp", plugin_bytes());
    let lone = open("Lone.esp", lone_bytes());
    Files { master, plugin, lone }
}

fn session(files: &Files) -> (Registry, Session) {
    let mut session = Session::with_files(
        GameMode::gmSSE,
        vec![files.master.clone(), files.plugin.clone(), files.lone.clone()],
    );
    session.allow_edit(true);
    (Registry::standard(), session)
}

/// The bytes of the record with the FormID in a saved file, header
/// included.
fn record_in(bytes: &[u8], signature: &[u8; 4], form_id: u32) -> Option<Vec<u8>> {
    let mut offset = 0;
    while offset + 24 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &bytes[offset..offset + 4] == b"GRUP" {
            offset += 24;
            continue;
        }
        let id = u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
        if &bytes[offset..offset + 4] == signature && id == form_id {
            return Some(bytes[offset..offset + 24 + size].to_vec());
        }
        offset += 24 + size;
    }
    None
}

#[test]
fn copies_a_record_as_an_override() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    let dry = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00000800", "from": "Master.esm", "to": "Plugin.esp", "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["existed"], false);
    assert_eq!(dry["required_masters"], json!(["Master.esm"]));
    assert_eq!(dry["missing_masters"], json!([]));
    assert_eq!(files.plugin.get_record_count(), 1);
    let result = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00000800", "from": "Master.esm", "to": "Plugin.esp" }),
        )
        .unwrap();
    assert_eq!(result["copy"]["form_id"], "00000800");
    assert_eq!(result["copy"]["file"], "Plugin.esp");
    assert_eq!(result["copy"]["editor_id"], "fMaster");
    assert_eq!(files.plugin.get_record_count(), 2);
    // The copy is the winning override of the master's record.
    let source = files.master.records()[0].clone();
    let winner = source.get_winning_override();
    assert_eq!(winner.get_file().unwrap().get_name(), "Plugin.esp");
    // A second copy finds the override.
    let again = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00000800", "from": "Master.esm", "to": "Plugin.esp", "dry_run": true }),
        )
        .unwrap();
    assert_eq!(again["existed"], true);
    // The saved override has the bytes of the source record.
    let saved = files.plugin.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    let copied = record_in(&saved, b"GMST", 0x800).expect("the override is saved");
    assert_eq!(copied, gmst(0x800, b"fMaster\0", 1.5));
    // The header counts the new record.
    assert_eq!(&saved[..4], b"TES4");
    assert!(record_in(&saved, b"GMST", 0x0100_0800).is_some());
}

#[test]
fn copies_a_record_as_a_new_record() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    let result = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00000800", "from": "Master.esm", "to": "Plugin.esp", "as_new": true,
                    "suffix": "_New" }),
        )
        .unwrap();
    // The next object ID of the plugin is 0x801, and the plugin has a
    // record, so the next one after the copy is 0x802.
    assert_eq!(result["copy"]["form_id"], "01000801");
    assert_eq!(result["copy"]["editor_id"], "fMaster_New");
    assert_eq!(
        files
            .plugin
            .header()
            .unwrap()
            .get_element_native_value(r"HEDR\Next Object ID")
            .as_ordinal(),
        Some(0x802)
    );
    let saved = files.plugin.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    let copied = record_in(&saved, b"GMST", 0x0100_0801).expect("the new record is saved");
    assert_eq!(copied, gmst(0x0100_0801, b"fMaster_New\0", 1.5));
}

#[test]
fn a_copy_adds_the_masters_it_needs() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    let dry = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00000800", "from": "Master.esm", "to": "Lone.esp", "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["missing_masters"], json!(["Master.esm"]));
    assert_eq!(files.lone.masters().len(), 0);
    let copied = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00000800", "from": "Master.esm", "to": "Lone.esp" }),
        )
        .unwrap();
    assert_eq!(copied["copy"]["form_id"], "00000800");
    let masters: Vec<String> = files.lone.masters().iter().map(|master| master.get_name()).collect();
    assert_eq!(masters, ["Master.esm"]);
    let saved = files.lone.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert!(saved.windows(11).any(|window| window == b"Master.esm\0"));
    assert_eq!(record_in(&saved, b"GMST", 0x800), Some(gmst(0x800, b"fMaster\0", 1.5)));
    // A master that loads after the target is refused.
    let error = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "01000800", "from": "Plugin.esp", "to": "Master.esm", "dry_run": true }),
        )
        .unwrap_err();
    assert!(error.message.contains("higher load order"), "{}", error.message);
}

#[test]
fn adds_and_removes_a_member() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    // The editor ID exists already: `Add` returns it.
    let added = registry
        .call(
            &mut session,
            "elements.add",
            json!({ "form_id": "01000800", "file": "Plugin.esp", "name": "EDID" }),
        )
        .unwrap();
    assert_eq!(added["element"]["value"], "fPlugin");
    // A required member can not be removed.
    let error = registry
        .call(
            &mut session,
            "elements.remove",
            json!({ "form_id": "01000800", "file": "Plugin.esp", "path": "EDID" }),
        )
        .unwrap_err();
    assert_eq!(error.code, "not_removable");
    // The cell lacks a name: added, set, saved, then removed again.
    let added = registry
        .call(
            &mut session,
            "elements.add",
            json!({ "form_id": "00000801", "file": "Master.esm", "name": "FULL" }),
        )
        .unwrap();
    assert!(added["path"].as_str().unwrap().ends_with("FULL - Name"), "{added}");
    registry
        .call(
            &mut session,
            "elements.set",
            json!({ "form_id": "00000801", "file": "Master.esm", "path": "FULL", "value": "Test Cell" }),
        )
        .unwrap();
    let saved = files.master.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert!(saved.windows(10).any(|window| window == b"Test Cell\0"));
    let dry = registry
        .call(
            &mut session,
            "elements.remove",
            json!({ "form_id": "00000801", "file": "Master.esm", "path": "FULL", "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["removed"], false);
    let removed = registry
        .call(
            &mut session,
            "elements.remove",
            json!({ "form_id": "00000801", "file": "Master.esm", "path": "FULL" }),
        )
        .unwrap();
    assert_eq!(removed["removed"], true);
    let saved = files.master.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert!(!saved.windows(4).any(|window| window == b"FULL"));
    // The cell keeps its editor ID and data; the load added its required
    // lighting template (`LTMP`), which the modified cell now saves.
    let saved_cell = record_in(&saved, b"CELL", 0x801).unwrap();
    assert!(saved_cell.windows(4).any(|window| window == b"LTMP"));
    assert!(saved_cell.starts_with(&cell()[..4]));
}

#[test]
fn deletes_a_record() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    let dry = registry
        .call(
            &mut session,
            "records.delete",
            json!({ "form_id": "01000800", "file": "Plugin.esp", "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["removed"], false);
    assert_eq!(files.plugin.get_record_count(), 1);
    registry
        .call(
            &mut session,
            "records.delete",
            json!({ "form_id": "01000800", "file": "Plugin.esp" }),
        )
        .unwrap();
    assert_eq!(files.plugin.get_record_count(), 0);
    // The empty group leaves the file on save: the header is all that is left.
    let saved = files.plugin.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert!(record_in(&saved, b"GMST", 0x0100_0800).is_none());
    assert!(!saved.windows(4).any(|window| window == b"GRUP"));
    // The file header is not removable.
    let error = registry
        .call(
            &mut session,
            "records.delete",
            json!({ "form_id": "00000000", "file": "Plugin.esp" }),
        )
        .unwrap_err();
    assert!(
        matches!(error.code.as_str(), "unknown_record" | "not_removable"),
        "{error}"
    );
}

#[test]
fn adds_a_placed_record_to_a_cell_and_copies_the_cell_with_it() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    let added = registry
        .call(
            &mut session,
            "elements.add",
            json!({ "form_id": "00000801", "file": "Master.esm", "name": "REFR" }),
        )
        .unwrap();
    assert_eq!(added["record"]["signature"], "REFR");
    assert_eq!(added["record"]["form_id"], "00000802");
    assert_eq!(added["file"], "Master.esm");
    let cell = files
        .master
        .records()
        .into_iter()
        .find(|record| record.get_signature().to_string() == "CELL")
        .unwrap();
    let children = cell.child_group().expect("the cell has a child group now");
    assert_eq!(children.group_type(), 6);
    let temporary = children.get_element(0).unwrap();
    assert!(temporary.get_name().starts_with("GRUP Cell Temporary Children of"));
    // The saved master has the reference after its cell, in the groups of
    // the children.
    let saved = files.master.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    let cell_at = saved.windows(4).position(|window| window == b"CELL").unwrap();
    let refr_at = saved.windows(4).position(|window| window == b"REFR").unwrap();
    assert!(refr_at > cell_at);
    let reloaded = wb_file_from_bytes("Reloaded.esm", i32::MAX, FileStates::empty(), FileBytes::Owned(saved)).unwrap();
    assert_eq!(reloaded.get_record_count(), 3);
    // The cell copied with its children into the plugin.
    let copied = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00000801", "from": "Master.esm", "to": "Plugin.esp", "deep": true }),
        )
        .unwrap();
    assert_eq!(copied["copy"]["signature"], "CELL");
    assert_eq!(files.plugin.get_record_count(), 3);
    let plugin_cell = files
        .plugin
        .records()
        .into_iter()
        .find(|record| record.get_signature().to_string() == "CELL")
        .unwrap();
    let group = plugin_cell.child_group().expect("the copy has a child group");
    assert_eq!(group.get_element_count(), 1);
}

/// A record of an installed game copied as an override keeps every value:
/// the script properties of Nazeem (unions decided again from the values
/// assigned before them) and his factions (a subrecord array made from its
/// definition). Does nothing without `XEDIT_SSE_DATA`.
#[test]
fn copies_a_skyrim_npc_as_an_override() {
    let _guard = test_lock();
    let Some(data) = std::env::var("XEDIT_SSE_DATA").ok() else {
        return;
    };
    let skyrim = format!("{data}/Skyrim.esm");
    let update = format!("{data}/Update.esm");
    if !std::path::Path::new(&update).exists() {
        return;
    }
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    let mut session = Session::load("sse", &[skyrim, update]).unwrap();
    session.allow_edit(true);
    let registry = Registry::standard();
    // The copy assigns the localized strings of the NPC as text, so they
    // become new strings of Update.esm's tables; its tables are loaded
    // first, as the GUI has them once it shows a name of the plugin
    // (`AddValue` makes empty tables for a plugin whose tables are not
    // loaded, which would hide Update.esm's own strings).
    registry.call(&mut session, "localization.files", json!({})).unwrap();
    let copied = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00013BBF", "from": "Skyrim.esm", "to": "Update.esm" }),
        )
        .unwrap();
    assert_eq!(copied["copy"]["file"], "Update.esm");
    let source = registry
        .call(
            &mut session,
            "elements.get",
            json!({ "form_id": "00013BBF", "file": "Skyrim.esm", "path": "VMAD" }),
        )
        .unwrap();
    let copy = registry
        .call(
            &mut session,
            "elements.get",
            json!({ "form_id": "00013BBF", "file": "Update.esm", "path": "VMAD" }),
        )
        .unwrap();
    assert_eq!(copy["children"], source["children"]);
    for path in ["Factions", "Items", "AIDT"] {
        let source = registry
            .call(
                &mut session,
                "elements.get",
                json!({ "form_id": "00013BBF", "file": "Skyrim.esm", "path": path }),
            )
            .unwrap();
        let copy = registry
            .call(
                &mut session,
                "elements.get",
                json!({ "form_id": "00013BBF", "file": "Update.esm", "path": path }),
            )
            .unwrap();
        assert_eq!(copy["children"], source["children"], "{path}");
    }
}
