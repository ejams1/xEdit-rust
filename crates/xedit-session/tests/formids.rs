// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! FormID changes and module flags on synthetic Skyrim SE files: a master
//! with a keyword and a form list, and a plugin with new records, an
//! override of the master's list and an interior cell. `formids.change`
//! moves a record to a new FormID and updates the records that refer to
//! it in both files, `formids.renumber` renumbers the plugin's new records,
//! an interior cell moves to the block of its new object ID, and
//! `files.flags` sets the ESL flag.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use xedit_core::implementation::{FileImpl, ResetModified, wb_file};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{Element, FormID, GameMode, MainRecord};
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

fn group_of(label: u32, group_type: i32, records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(&label.to_le_bytes());
    bytes.extend_from_slice(&(group_type as u32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

fn group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    group_of(u32::from_le_bytes(*label), 0, records)
}

fn header(flags: u32, record_count: u32, next_object_id: u32, masters: &[&str]) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.71f32.to_le_bytes());
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

fn keyword(form_id: u32, editor_id: &[u8]) -> Vec<u8> {
    main_record(b"KYWD", 0, form_id, &sub_record(b"EDID", editor_id))
}

fn form_list(form_id: u32, editor_id: &[u8], entries: &[u32]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    for entry in entries {
        data.extend(sub_record(b"LNAM", &entry.to_le_bytes()));
    }
    main_record(b"FLST", 0, form_id, &data)
}

fn master_bytes() -> Vec<u8> {
    let mut bytes = header(1, 4, 0x802, &[]);
    bytes.extend(group(b"KYWD", &keyword(0x800, b"KwMaster\0")));
    bytes.extend(group(b"FLST", &form_list(0x801, b"ListMaster\0", &[0x800])));
    bytes
}

fn plugin_bytes() -> Vec<u8> {
    let mut bytes = header(0, 8, 0x902, &["Master.esm"]);
    bytes.extend(group(b"KYWD", &keyword(0x0100_0900, b"KwPlugin\0")));
    let mut lists = form_list(0x801, b"ListMaster\0", &[0x800, 0x0100_0900]);
    lists.extend(form_list(0x0100_0901, b"ListPlugin\0", &[0x0100_0900, 0x800]));
    bytes.extend(group(b"FLST", &lists));
    // An interior cell 0x910 (2320: block 0, sub-block 2).
    let mut cell = sub_record(b"EDID", b"CellPlugin\0");
    cell.extend(sub_record(b"DATA", &1u16.to_le_bytes()));
    let sub_block = group_of(2, 3, &main_record(b"CELL", 0, 0x0100_0910, &cell));
    let block = group_of(0, 2, &sub_block);
    bytes.extend(group(b"CELL", &block));
    bytes
}

/// A fresh directory with the master and the plugin, loaded.
fn setup(test: &str) -> (PathBuf, Arc<FileImpl>, Arc<FileImpl>) {
    let dir = std::env::temp_dir().join(format!("xedit-formids-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Master.esm"), master_bytes()).unwrap();
    std::fs::write(dir.join("Plugin.esp"), plugin_bytes()).unwrap();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let plugin = wb_file(dir.join("Plugin.esp").to_str().unwrap(), i32::MAX, FileStates::empty()).unwrap();
    let master = plugin.masters()[0].clone();
    (dir, master, plugin)
}

fn session(files: Vec<Arc<FileImpl>>) -> Session {
    let mut session = Session::with_files(GameMode::gmSSE, files);
    session.allow_edit(true);
    session
}

fn lnam(file: &Arc<FileImpl>, form_id: u32) -> Vec<String> {
    let record = file
        .contained_record_by_load_order_form_id(FormID::from_cardinal(form_id))
        .unwrap_or_else(|| panic!("no record {form_id:08X} in {}", file.file_name()));
    let mut values = Vec::new();
    collect_lnam(&(record as xedit_core::interface::ElementRef), &mut values);
    values
}

fn collect_lnam(element: &xedit_core::interface::ElementRef, values: &mut Vec<String>) {
    if element
        .get_record_signature()
        .is_some_and(|signature| signature.to_string() == "LNAM")
    {
        values.push(element.get_edit_value());
        return;
    }
    if let Some(container) = element.as_container() {
        for index in 0..container.get_element_count() {
            if let Some(child) = container.get_element(index) {
                collect_lnam(&child, values);
            }
        }
    }
}

fn ids(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| {
            let open = value.rfind('[').map_or(0, |index| index + 1);
            let close = value.rfind(']').unwrap_or(value.len());
            value[open..close].to_owned()
        })
        .collect()
}

#[test]
fn change_moves_the_record_and_updates_the_references() {
    let _guard = test_lock();
    let (dir, master, plugin) = setup("change");
    let mut session = session(vec![plugin.clone()]);
    let registry = Registry::standard();
    let result = registry
        .call(
            &mut session,
            "formids.change",
            json!({ "form_id": "01000900", "new_form_id": "01000A00" }),
        )
        .unwrap();
    assert_eq!(result["old_form_id"], "01000900");
    assert_eq!(result["new_form_id"], "01000A00");
    assert_eq!(result["changed"], true);
    // The plugin's own list and its override of the master's list.
    assert_eq!(result["referenced_by"].as_array().unwrap().len(), 2);
    assert_eq!(result["references_updated"], 2);
    assert!(
        plugin
            .contained_record_by_load_order_form_id(FormID::from_cardinal(0x0100_0900))
            .is_none()
    );
    let moved = plugin
        .contained_record_by_load_order_form_id(FormID::from_cardinal(0x0100_0A00))
        .unwrap();
    assert_eq!(moved.get_editor_id(), "KwPlugin");
    assert_eq!(ids(&lnam(&plugin, 0x0100_0901)), ["01000A00", "00000800"]);
    assert_eq!(ids(&lnam(&plugin, 0x0000_0801)), ["00000800", "01000A00"]);
    assert_eq!(ids(&lnam(&master, 0x0000_0801)), ["00000800"]);
    // The records stay sorted by FormID.
    let order: Vec<u32> = plugin
        .records()
        .iter()
        .map(|record| record.get_fixed_form_id().to_cardinal())
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted);

    // The saved file holds the new FormID and the new references.
    let saved = plugin.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    std::fs::write(dir.join("Saved.esp"), &saved).unwrap();
    let reloaded = wb_file(dir.join("Saved.esp").to_str().unwrap(), i32::MAX, FileStates::empty()).unwrap();
    assert!(
        reloaded
            .contained_record_by_load_order_form_id(FormID::from_cardinal(0x0100_0A00))
            .is_some()
    );
    assert_eq!(ids(&lnam(&reloaded, 0x0100_0901)), ["01000A00", "00000800"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn change_of_a_master_record_takes_its_overrides_along() {
    let _guard = test_lock();
    let (dir, master, plugin) = setup("master");
    let mut session = session(vec![master.clone(), plugin.clone()]);
    let registry = Registry::standard();
    let result = registry
        .call(
            &mut session,
            "formids.change",
            json!({ "form_id": "00000801", "file": "Master.esm", "new_form_id": "00000850", "overrides": true }),
        )
        .unwrap();
    assert_eq!(result["renumbered"], json!(["Master.esm", "Plugin.esp"]));
    let record = master
        .contained_record_by_load_order_form_id(FormID::from_cardinal(0x850))
        .unwrap();
    let overrides = record.overrides();
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].get_file().unwrap().get_name(), "Plugin.esp");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn change_to_a_taken_form_id_fails() {
    let _guard = test_lock();
    let (dir, _master, plugin) = setup("taken");
    let mut session = session(vec![plugin]);
    let error = Registry::standard()
        .call(
            &mut session,
            "formids.change",
            json!({ "form_id": "01000900", "new_form_id": "01000901" }),
        )
        .unwrap_err();
    assert_eq!(error.code, "edit_failed");
    assert!(
        error.message.contains("is already present in file"),
        "{}",
        error.message
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn renumber_from_a_start_form_id() {
    let _guard = test_lock();
    let (dir, master, plugin) = setup("renumber");
    let mut session = session(vec![plugin.clone()]);
    let registry = Registry::standard();
    let dry = registry
        .call(
            &mut session,
            "formids.renumber",
            json!({ "start": "000D00", "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["changed"], false);
    assert_eq!(dry["changes"].as_array().unwrap().len(), 3);
    assert!(
        plugin
            .contained_record_by_load_order_form_id(FormID::from_cardinal(0x0100_0900))
            .is_some()
    );

    let result = registry
        .call(&mut session, "formids.renumber", json!({ "start": "000D00" }))
        .unwrap();
    let changes: Vec<(String, String)> = result["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| {
            (
                change["old_form_id"].as_str().unwrap().to_owned(),
                change["new_form_id"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        changes,
        [
            ("01000900".to_owned(), "01000D00".to_owned()),
            ("01000901".to_owned(), "01000D01".to_owned()),
            ("01000910".to_owned(), "01000D02".to_owned()),
        ]
    );
    assert_eq!(result["next_object_id"], "000D04");
    assert_eq!(ids(&lnam(&plugin, 0x0100_0D01)), ["01000D00", "00000800"]);
    assert_eq!(ids(&lnam(&plugin, 0x0000_0801)), ["00000800", "01000D00"]);
    assert_eq!(ids(&lnam(&master, 0x0000_0801)), ["00000800"]);
    assert_eq!(plugin.get_next_object_id(), 0xD04);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn interior_cell_moves_to_the_block_of_its_object_id() {
    let _guard = test_lock();
    let (dir, _master, plugin) = setup("cell");
    xedit_core::interface::globals::set_edit_allowed(true);
    let cell = plugin
        .contained_record_by_load_order_form_id(FormID::from_cardinal(0x0100_0910))
        .unwrap();
    // 2321: block 1, sub-block 2.
    cell.set_load_order_form_id(FormID::from_cardinal(0x0100_0911)).unwrap();
    let saved = plugin.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    xedit_core::interface::globals::set_edit_allowed(false);
    // The CELL top group holds one block group labelled 1 with one
    // sub-block group labelled 2.
    let cell_top = find_top_group(&saved, b"CELL").unwrap();
    let block = &saved[cell_top + 24..];
    assert_eq!(&block[..4], b"GRUP");
    assert_eq!(u32::from_le_bytes(block[8..12].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(block[12..16].try_into().unwrap()), 2);
    let sub_block = &block[24..];
    assert_eq!(u32::from_le_bytes(sub_block[8..12].try_into().unwrap()), 2);
    assert_eq!(u32::from_le_bytes(sub_block[12..16].try_into().unwrap()), 3);
    assert_eq!(&sub_block[24..28], b"CELL");
    assert_eq!(u32::from_le_bytes(sub_block[36..40].try_into().unwrap()), 0x0100_0911);
    std::fs::remove_dir_all(dir).ok();
}

/// The offset of the top level group with the label.
fn find_top_group(bytes: &[u8], label: &[u8; 4]) -> Option<usize> {
    // Skip the file header record.
    let mut offset = 24 + u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    while offset + 24 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &bytes[offset + 8..offset + 12] == label {
            return Some(offset);
        }
        offset += size;
    }
    None
}

#[test]
fn flags_set_the_light_flag() {
    let _guard = test_lock();
    let (dir, _master, plugin) = setup("flags");
    let mut session = session(vec![plugin.clone()]);
    let registry = Registry::standard();
    let dry = registry
        .call(&mut session, "files.flags", json!({ "light": true, "dry_run": true }))
        .unwrap();
    assert_eq!(dry["before"]["light"], false);
    assert_eq!(dry["after"]["light"], true);
    assert_eq!(dry["light_compatible"], true);
    assert!(!plugin.get_is_light());
    let result = registry
        .call(&mut session, "files.flags", json!({ "light": true }))
        .unwrap();
    assert_eq!(result["changed"], true);
    assert!(plugin.get_is_light());
    let saved = plugin.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert_eq!(u32::from_le_bytes(saved[8..12].try_into().unwrap()) & 0x200, 0x200);
    // Medium is not a Skyrim flag: nothing changes.
    let result = registry
        .call(&mut session, "files.flags", json!({ "medium": true }))
        .unwrap();
    assert_eq!(result["changed"], false);
    assert_eq!(result["supported"], json!(["esm", "localized", "light"]));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn renumber_needs_the_edit_flag() {
    let _guard = test_lock();
    let (dir, _master, plugin) = setup("gate");
    let mut session = Session::with_files(GameMode::gmSSE, vec![plugin]);
    let error = Registry::standard()
        .call(&mut session, "formids.renumber", json!({ "start": "000D00" }))
        .unwrap_err();
    assert_eq!(error.code, "edit_required");
    // A dry run reports what the command would do with the edit flag.
    let dry = Registry::standard()
        .call(
            &mut session,
            "formids.renumber",
            json!({ "start": "000D00", "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["changes"].as_array().unwrap().len(), 3);
    assert!(!xedit_core::interface::globals::edit_allowed());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn change_into_a_file_that_is_not_a_master_adds_it() {
    let _guard = test_lock();
    let dir = std::env::temp_dir().join(format!("xedit-formids-addmaster-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut other = header(1, 2, 0x801, &[]);
    other.extend(group(b"KYWD", &keyword(0x800, b"KwOther\0")));
    std::fs::write(dir.join("Other.esm"), other).unwrap();
    std::fs::write(dir.join("Master.esm"), master_bytes()).unwrap();
    std::fs::write(dir.join("Plugin.esp"), plugin_bytes()).unwrap();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let other = wb_file(dir.join("Other.esm").to_str().unwrap(), i32::MAX, FileStates::empty()).unwrap();
    let plugin = wb_file(dir.join("Plugin.esp").to_str().unwrap(), i32::MAX, FileStates::empty()).unwrap();
    let mut session = session(vec![other, plugin.clone()]);
    let result = Registry::standard()
        .call(
            &mut session,
            "formids.change",
            json!({ "form_id": "02000900", "file": "Plugin.esp", "new_form_id": "00000A00" }),
        )
        .unwrap();
    assert_eq!(result["masters_added"], json!(["Other.esm"]));
    let masters: Vec<String> = plugin.masters().iter().map(|master| master.get_name()).collect();
    assert_eq!(masters, ["Other.esm", "Master.esm"]);
    let record = plugin
        .contained_record_by_load_order_form_id(FormID::from_cardinal(0xA00))
        .unwrap();
    assert_eq!(record.get_editor_id(), "KwPlugin");
    // The plugin's list refers to the keyword by its new FormID.
    assert_eq!(ids(&lnam(&plugin, 0x0200_0901)), ["00000A00", "01000800"]);
    std::fs::remove_dir_all(dir).ok();
}
