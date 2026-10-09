// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `files.clean` on synthetic Skyrim SE plugins: the records identical to
//! their master removed with the groups they leave empty, a deleted
//! reference undeleted and disabled, the dry run, the quick mode with its
//! save, and the edit gate.

use std::sync::Arc;

use serde_json::{Value, json};
use xedit_core::implementation::{FileBytes, FileImpl, ResetModified, wb_file_from_bytes};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{Container, File, GameMode};
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

fn stat(form_id: u32) -> Vec<u8> {
    let mut data = sub_record(b"EDID", b"TestStatic\0");
    data.extend(sub_record(b"OBND", &[0; 12]));
    main_record(b"STAT", 0, form_id, &data)
}

fn reference(form_id: u32, base: u32, position: [f32; 3]) -> Vec<u8> {
    let mut data = sub_record(b"NAME", &base.to_le_bytes());
    let mut placement = Vec::new();
    for value in position.iter().chain(&[0.0; 3]) {
        placement.extend_from_slice(&value.to_le_bytes());
    }
    data.extend(sub_record(b"DATA", &placement));
    main_record(b"REFR", 0, form_id, &data)
}

fn deleted_reference(form_id: u32) -> Vec<u8> {
    main_record(b"REFR", 0x20, form_id, &[])
}

/// The interior cell `[00000803]` (block 1, sub-block 5 of its object ID)
/// with its temporary references.
fn cell_with(references: &[u8]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", b"TestCell\0");
    data.extend(sub_record(b"DATA", &1u16.to_le_bytes()));
    let mut cell = main_record(b"CELL", 0, 0x803, &data);
    cell.extend(group(0x803, 6, &group(0x803, 9, references)));
    top_group(b"CELL", &group(1, 2, &group(5, 3, &cell)))
}

fn master_bytes() -> Vec<u8> {
    let mut bytes = header(1, 6, 0x806, &[]);
    let mut settings = gmst(0x800, b"fSame\0", 1.5);
    settings.extend(gmst(0x801, b"fChanged\0", 2.0));
    bytes.extend(top_group(b"GMST", &settings));
    bytes.extend(top_group(b"STAT", &stat(0x802)));
    let mut references = reference(0x804, 0x802, [1.0, 2.0, 3.0]);
    references.extend(reference(0x805, 0x802, [4.0, 5.0, 6.0]));
    bytes.extend(cell_with(&references));
    bytes
}

/// A dirty plugin: a game setting identical to the master's and one
/// changed, the cell identical with an identical reference and a deleted
/// one.
fn dirty_bytes() -> Vec<u8> {
    let mut bytes = header(0, 5, 0x800, &["Master.esm"]);
    let mut settings = gmst(0x800, b"fSame\0", 1.5);
    settings.extend(gmst(0x801, b"fChanged\0", 3.0));
    bytes.extend(top_group(b"GMST", &settings));
    let mut references = reference(0x804, 0x802, [1.0, 2.0, 3.0]);
    references.extend(deleted_reference(0x805));
    bytes.extend(cell_with(&references));
    bytes
}

/// A plugin whose cell and reference are identical to the master's.
fn identical_bytes() -> Vec<u8> {
    let mut bytes = header(0, 2, 0x800, &["Master.esm"]);
    bytes.extend(cell_with(&reference(0x804, 0x802, [1.0, 2.0, 3.0])));
    bytes
}

fn load(plugin: Vec<u8>) -> (Vec<Arc<FileImpl>>, Session) {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game_for_edit("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let mut files = Vec::new();
    for (name, bytes) in [("Master.esm", master_bytes()), ("Plugin.esp", plugin)] {
        files.push(wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap());
    }
    let mut session = Session::with_files(GameMode::gmSSE, files.clone());
    session.allow_edit(true);
    (files, session)
}

/// The FormIDs of the main records of a saved file, groups skipped.
fn records_in(bytes: &[u8]) -> Vec<(String, u32, u32)> {
    let mut result = Vec::new();
    let mut offset = 0;
    while offset + 24 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &bytes[offset..offset + 4] == b"GRUP" {
            offset += 24;
            continue;
        }
        let flags = u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap());
        let id = u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
        result.push((
            String::from_utf8_lossy(&bytes[offset..offset + 4]).into_owned(),
            id,
            flags,
        ));
        offset += 24 + size;
    }
    result
}

fn groups_in(bytes: &[u8]) -> usize {
    let mut count = 0;
    let mut offset = 0;
    while offset + 24 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &bytes[offset..offset + 4] == b"GRUP" {
            count += 1;
            offset += 24;
        } else {
            offset += 24 + size;
        }
    }
    count
}

fn form_ids(step: &Value) -> Vec<String> {
    step["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            format!(
                "{}:{}",
                record["signature"].as_str().unwrap(),
                record["form_id"].as_str().unwrap_or("")
            )
        })
        .collect()
}

#[test]
fn removes_identical_records_and_undeletes_references() {
    let _guard = test_lock();
    let (files, mut session) = load(dirty_bytes());
    let registry = Registry::standard();

    // The dry run counts and changes nothing.
    let dry = registry
        .call(
            &mut session,
            "files.clean",
            json!({ "file": "Plugin.esp", "itm": true, "udr": true, "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["udr"], 1);
    assert_eq!(dry["itm"], 2, "{dry:#}");
    assert_eq!(files[1].get_record_count(), 5);

    let result = registry
        .call(
            &mut session,
            "files.clean",
            json!({ "file": "Plugin.esp", "itm": true, "udr": true }),
        )
        .unwrap();
    let pass = &result["passes"][0];
    assert_eq!(form_ids(&pass["udr"]), ["REFR:00000805"]);
    // The identical game setting and the identical reference go; the cell
    // keeps the undeleted reference, so it stays with its groups. The top
    // groups are in the order of the navigation tree ("Cell" before "Game
    // Setting"), walked from the last.
    assert_eq!(form_ids(&pass["itm"]), ["GMST:00000800", "REFR:00000804"]);
    assert_eq!(result["unsaved"], true);

    let saved = files[1].write_to_bytes(ResetModified::rmSetInternal).unwrap();
    let records = records_in(&saved);
    let ids: Vec<u32> = records.iter().map(|(_, id, _)| *id).collect();
    assert_eq!(ids, [0, 0x801, 0x803, 0x805]);
    // The reference is no longer deleted and is initially disabled.
    let (_, _, flags) = &records[3];
    assert_eq!(flags & 0x20, 0);
    assert_ne!(flags & 0x800, 0);
    let record = files[1]
        .records()
        .into_iter()
        .find(|record| record.mr_struct().form_id.to_cardinal() == 0x805)
        .unwrap();
    assert_eq!(record.get_element_native_value(r"NAME").as_ordinal(), Some(0x802));
    let xedit_core::interface::misc::Variant::Float(z) = record.get_element_native_value(r"DATA\Position\Z") else {
        panic!("no position");
    };
    assert!((z + 30000.0).abs() < 0.01, "{z}");
    assert_eq!(
        record.get_element_native_value(r"XESP\Reference").as_ordinal(),
        Some(0x14)
    );
}

#[test]
fn removes_a_cell_with_the_groups_it_leaves_empty() {
    let _guard = test_lock();
    let (files, mut session) = load(identical_bytes());
    let registry = Registry::standard();
    let result = registry
        .call(
            &mut session,
            "files.clean",
            json!({ "itm": false, "udr": false, "quick": true, "dry_run": true, "file": "Plugin.esp" }),
        )
        .unwrap();
    // The reference only: the groups are not empty until it is removed.
    assert_eq!(result["itm"], 1, "{result:#}");
    let result = registry
        .call(
            &mut session,
            "files.clean",
            json!({ "file": "Plugin.esp", "itm": true }),
        )
        .unwrap();
    // The reference, the temporary children, the cell (with its children
    // group), the sub-block, the block and the top group, as the GUI
    // counts them.
    let step = &result["passes"][0]["itm"];
    assert_eq!(step["count"], 6, "{result:#}");
    assert_eq!(files[1].get_record_count(), 0);
    let saved = files[1].write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert_eq!(records_in(&saved).len(), 1);
    assert_eq!(groups_in(&saved), 0);
}

#[test]
fn the_quick_mode_saves_and_cleans_again() {
    let _guard = test_lock();
    let (_files, mut session) = load(dirty_bytes());
    let registry = Registry::standard();
    let dir = std::env::temp_dir().join(format!("xedit-clean-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let output = dir.join("Plugin.esp");
    let result = registry
        .call(
            &mut session,
            "files.clean",
            json!({ "file": "Plugin.esp", "quick": true, "output": output.to_string_lossy(), "backup": false }),
        )
        .unwrap();
    let passes = result["passes"].as_array().unwrap();
    // The first pass changed the plugin and saved it; the second found
    // nothing and saved nothing.
    assert_eq!(passes.len(), 2, "{result:#}");
    assert_eq!(passes[0]["saved"]["written"], true);
    assert!(passes[1].get("saved").is_none());
    assert_eq!(passes[1]["udr"]["count"], 0);
    assert_eq!(passes[1]["itm"]["count"], 0);
    assert_eq!(result["itm"], 2);
    assert_eq!(result["udr"], 1);
    assert_eq!(result["unsaved"], false);
    let saved = std::fs::read(&output).unwrap();
    let ids: Vec<u32> = records_in(&saved).iter().map(|(_, id, _)| *id).collect();
    assert_eq!(ids, [0, 0x801, 0x803, 0x805]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cleaning_needs_the_edit_flag() {
    let _guard = test_lock();
    let (_files, mut session) = load(dirty_bytes());
    session.allow_edit(false);
    let registry = Registry::standard();
    let error = registry
        .call(
            &mut session,
            "files.clean",
            json!({ "file": "Plugin.esp", "itm": true }),
        )
        .unwrap_err();
    assert_eq!(error.code, "edit_required");
    let error = registry
        .call(
            &mut session,
            "files.clean",
            json!({ "file": "Plugin.esp", "dry_run": true }),
        )
        .unwrap_err();
    assert_eq!(error.code, "invalid_params");
    let dry = registry
        .call(
            &mut session,
            "files.clean",
            json!({ "file": "Plugin.esp", "udr": true, "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["udr"], 1);
}
