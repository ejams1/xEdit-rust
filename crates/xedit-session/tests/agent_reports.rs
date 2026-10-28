// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regressions of the problems agents met while using the CLI on a
//! Fallout 4 load order, on synthetic Fallout 4 plugins.

use std::sync::Arc;

use serde_json::{Value, json};
use xedit_core::implementation::{FileBytes, FileImpl, ResetModified, wb_file_from_bytes};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{File, GameMode};
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
    bytes.extend_from_slice(&131u16.to_le_bytes());
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
    hedr.extend_from_slice(&1.0f32.to_le_bytes());
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
    main_record(b"TES4", flags, 0, &data)
}

fn load_game(files: Vec<(&str, Vec<u8>)>) -> Vec<Arc<FileImpl>> {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    let mode = xedit_session::dump::setup_game_for_edit("fo4").unwrap();
    xedit_session::save::apply_edit_settings(mode);
    let mut result = Vec::new();
    for (name, bytes) in files {
        let file = wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap();
        xedit_core::interface::add_file(file.clone());
        result.push(file);
    }
    result
}

fn session(files: &[Arc<FileImpl>]) -> (Registry, Session) {
    let mut session = Session::with_files(GameMode::gmFO4, files.to_vec());
    session.allow_edit(true);
    (Registry::standard(), session)
}

fn call(registry: &Registry, session: &mut Session, name: &str, params: Value) -> Value {
    registry
        .call(session, name, params)
        .unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

/// The data of every subrecord with the signature in the record of a saved
/// file, in file order.
fn sub_records_of(bytes: &[u8], form_id: u32, signature: &[u8; 4]) -> Vec<Vec<u8>> {
    let mut offset = 0;
    while offset + 24 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &bytes[offset..offset + 4] == b"GRUP" {
            offset += 24;
            continue;
        }
        let id = u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
        if id == form_id && &bytes[offset..offset + 4] != b"TES4" {
            let mut found = Vec::new();
            let mut at = offset + 24;
            let end = offset + 24 + size;
            while at + 6 <= end {
                let length = u16::from_le_bytes(bytes[at + 4..at + 6].try_into().unwrap()) as usize;
                if &bytes[at..at + 4] == signature {
                    found.push(bytes[at + 6..at + 6 + length].to_vec());
                }
                at += 6 + length;
            }
            return found;
        }
        offset += 24 + size;
    }
    panic!("the saved file has no record [{form_id:08X}]");
}

/// A `PHWT` of 16 floats and the 108 bytes of its trailing unknown member,
/// as the human race of Fallout 4 has them.
fn phwt(seed: u8) -> Vec<u8> {
    let mut data = Vec::new();
    for i in 0..16u8 {
        data.extend_from_slice(&(f32::from(i) / 16.0).to_le_bytes());
    }
    data.extend((0..108u8).map(|i| i.wrapping_mul(3).wrapping_add(seed)));
    data
}

/// `records.copy` of a race copies each phoneme weight subrecord whole,
/// with the unknown bytes after its floats.
#[test]
fn copying_a_race_keeps_the_unknown_bytes_of_its_phoneme_weights() {
    let _guard = test_lock();
    let mut race = sub_record(b"EDID", b"HumanRace\0");
    race.extend(sub_record(b"PHWT", &phwt(1)));
    race.extend(sub_record(b"PHWT", &phwt(2)));
    let mut master = header(1, 1, 0x801, &[]);
    master.extend(top_group(b"RACE", &main_record(b"RACE", 0, 0x800, &race)));
    let patch = header(0, 0, 0x800, &["Fallout4.esm"]);
    let files = load_game(vec![("Fallout4.esm", master), ("Patch.esp", patch)]);
    let (registry, mut session) = session(&files);
    call(
        &registry,
        &mut session,
        "records.copy",
        json!({ "form_id": "00000800", "from": "Fallout4.esm", "to": "Patch.esp" }),
    );
    let saved = files[1].write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert_eq!(sub_records_of(&saved, 0x800, b"PHWT"), vec![phwt(1), phwt(2)]);
}

/// Fallout 4 with a base object, a furniture and an actor; a light plugin
/// with an interior cell that holds a persistent reference; an empty patch.
fn cell_files() -> Vec<Arc<FileImpl>> {
    let mut master = header(1, 3, 0x810, &[]);
    master.extend(top_group(
        b"STAT",
        &main_record(b"STAT", 0, 0x801, &sub_record(b"EDID", b"TestStatic\0")),
    ));
    master.extend(top_group(
        b"FURN",
        &main_record(b"FURN", 0, 0x803, &sub_record(b"EDID", b"TestStand\0")),
    ));
    master.extend(top_group(
        b"NPC_",
        &main_record(b"NPC_", 0, 0x802, &sub_record(b"EDID", b"TestActor\0")),
    ));
    let mut cell = sub_record(b"EDID", b"LightCell\0");
    cell.extend(sub_record(b"DATA", &1u16.to_le_bytes()));
    let cell = main_record(b"CELL", 0, 0x0100_0699, &cell);
    // A load door whose teleport leads to another door of the plugin, with
    // a position, as the agents' reference was.
    let position = |x: f32| {
        let mut data = Vec::new();
        for value in [x, 2000.0, 128.0, 0.0, 0.0, 1.5] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        data
    };
    let mut door = sub_record(b"NAME", &0x801u32.to_le_bytes());
    let mut xtel = 0x0100_069Au32.to_le_bytes().to_vec();
    xtel.extend_from_slice(&[0; 32]);
    door.extend(sub_record(b"XTEL", &xtel));
    door.extend(sub_record(b"DATA", &position(1000.0)));
    let reference = main_record(b"REFR", 0x400, 0x0100_069B, &door);
    let mut other_door = sub_record(b"NAME", &0x801u32.to_le_bytes());
    let mut back = 0x0100_069Bu32.to_le_bytes().to_vec();
    back.extend_from_slice(&[0; 32]);
    other_door.extend(sub_record(b"XTEL", &back));
    other_door.extend(sub_record(b"DATA", &position(-1000.0)));
    let mut persistent_records = reference;
    persistent_records.extend(main_record(b"REFR", 0x400, 0x0100_069A, &other_door));
    let persistent = group(0x0100_0699, 8, &persistent_records);
    let children = group(0x0100_0699, 6, &persistent);
    let mut sub_block_records = cell;
    sub_block_records.extend(children);
    let sub_block = group(9, 3, &sub_block_records);
    let block = group(9, 2, &sub_block);
    let mut light = header(0x200, 2, 0x6A0, &["Fallout4.esm"]);
    light.extend(top_group(b"CELL", &block));
    let patch = header(0, 0, 0x800, &["Fallout4.esm"]);
    load_game(vec![
        ("Fallout4.esm", master),
        ("Light.esp", light),
        ("Patch.esp", patch),
    ])
}

fn listed_file(listed: &Value, name: &str) -> Value {
    listed["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["name"] == name)
        .unwrap_or_else(|| panic!("{name} is listed"))
        .clone()
}

/// `files.list` gives a light plugin the slot xEdit shows (`[FE 000]`)
/// beside its position in the load order.
#[test]
fn files_list_gives_the_slot_of_a_light_plugin() {
    let _guard = test_lock();
    let files = cell_files();
    let (registry, mut session) = session(&files);
    let listed = call(&registry, &mut session, "files.list", json!({}));
    assert_eq!(listed_file(&listed, "Light.esp")["file_id"], "FE 000");
    assert_eq!(listed_file(&listed, "Fallout4.esm")["file_id"], "00");
}

/// The signatures of the records in a saved plugin, in file order.
fn saved_signatures(bytes: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut offset = 0;
    while offset + 24 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &bytes[offset..offset + 4] == b"GRUP" {
            offset += 24;
            continue;
        }
        found.push(String::from_utf8_lossy(&bytes[offset..offset + 4]).into_owned());
        offset += 24 + size;
    }
    found
}

/// The same copy into a plugin `files.new` made, saved: the reference is
/// in the saved file, not only its cell.
#[test]
fn a_reference_copied_into_a_new_plugin_is_saved() {
    let _guard = test_lock();
    let files = cell_files();
    let (registry, mut session) = session(&files);
    let dir = xedit_io::temp::TempDir::new("xedit-agent-reports-new-plugin");
    xedit_core::interface::globals::set_data_path(&format!("{}\\", dir.path().display()));
    call(&registry, &mut session, "files.new", json!({ "file": "NewPatch.esp" }));
    let copied = call(
        &registry,
        &mut session,
        "records.copy",
        json!({ "form_id": "FE00069B", "from": "Light.esp", "to": "NewPatch.esp" }),
    );
    assert_eq!(copied["copy"]["file"], "NewPatch.esp", "{copied}");
    let output = dir.path().join("NewPatch.esp");
    call(
        &registry,
        &mut session,
        "files.save",
        json!({ "file": "NewPatch.esp", "output": output.to_string_lossy(), "backup": false }),
    );
    let saved = std::fs::read(&output).unwrap();
    assert_eq!(saved_signatures(&saved), vec!["TES4", "CELL", "REFR"]);
}

/// A persistent reference of a light plugin's interior cell is copied as
/// an override with its cell, and the commands that name a file refuse a
/// record that file does not hold.
#[test]
fn copying_a_persistent_reference_of_a_light_plugin_copies_the_reference() {
    let _guard = test_lock();
    let files = cell_files();
    let (registry, mut session) = session(&files);
    let copied = call(
        &registry,
        &mut session,
        "records.copy",
        json!({ "form_id": "FE00069B", "from": "Light.esp", "to": "Patch.esp" }),
    );
    assert_eq!(copied["copy"]["signature"], "REFR", "{copied}");
    assert_eq!(copied["copy"]["file"], "Patch.esp", "{copied}");
    let listed = call(&registry, &mut session, "records.list", json!({ "file": "Patch.esp" }));
    let signatures: Vec<&str> = listed["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["signature"].as_str().unwrap())
        .collect();
    assert!(signatures.contains(&"CELL") && signatures.contains(&"REFR"), "{listed}");
    let got = call(
        &registry,
        &mut session,
        "records.get",
        json!({ "form_id": "FE00069B", "file": "Patch.esp", "depth": 0 }),
    );
    assert_eq!(got["file"], "Patch.esp");
    for (command, params) in [
        ("records.get", json!({ "form_id": "00000801", "file": "Patch.esp" })),
        (
            "elements.set",
            json!({ "form_id": "00000801", "file": "Patch.esp", "path": "EDID", "value": "Changed" }),
        ),
    ] {
        let error = registry.call(&mut session, command, params).unwrap_err();
        assert_eq!(error.code, "not_in_file", "{command}: {error:?}");
    }
    let master = call(
        &registry,
        &mut session,
        "records.get",
        json!({ "form_id": "00000801", "file": "Fallout4.esm" }),
    );
    assert_eq!(master["editor_id"], "TestStatic");
}

/// `elements.set` and `elements.assign` add a missing member named by its
/// display name (`PFRN - Power Armor Stand`), as by its signature.
#[test]
fn a_missing_member_is_added_by_its_display_name() {
    let _guard = test_lock();
    let files = cell_files();
    let (registry, mut session) = session(&files);
    call(
        &registry,
        &mut session,
        "records.copy",
        json!({ "form_id": "00000802", "from": "Fallout4.esm", "to": "Patch.esp" }),
    );
    let set = call(
        &registry,
        &mut session,
        "elements.set",
        json!({ "form_id": "00000802", "file": "Patch.esp", "path": "PFRN - Power Armor Stand", "value": "00000803" }),
    );
    assert_eq!(set["added"], true, "{set}");
    assert_eq!(set["new"]["native"], 0x803, "{set}");
    // Assign: another plugin's copy takes the member from the patch.
    call(
        &registry,
        &mut session,
        "records.copy",
        json!({ "form_id": "00000802", "from": "Fallout4.esm", "to": "Light.esp" }),
    );
    let assigned = call(
        &registry,
        &mut session,
        "elements.assign",
        json!({
            "form_id": "00000802", "file": "Light.esp", "path": "PFRN - Power Armor Stand",
            "source": { "form_id": "00000802", "file": "Patch.esp", "path": "PFRN" }
        }),
    );
    assert_eq!(assigned["can_assign"], true, "{assigned}");
    let got = call(
        &registry,
        &mut session,
        "elements.get",
        json!({ "form_id": "00000802", "file": "Light.esp", "path": "PFRN" }),
    );
    assert_eq!(got["native"], 0x803, "{got}");
}

fn count_rows(rows: &Value) -> usize {
    rows.as_array()
        .map_or(0, |rows| rows.iter().map(|row| 1 + count_rows(&row["children"])).sum())
}

/// `records.compare` bounds its rows with `limit` and `depth`, and
/// `compact` leaves the paths and edit values out of the cells.
#[test]
fn compare_rows_are_bounded() {
    let _guard = test_lock();
    let mut race = sub_record(b"EDID", b"HumanRace\0");
    for seed in 0..4 {
        race.extend(sub_record(b"PHWT", &phwt(seed)));
    }
    let mut master = header(1, 1, 0x801, &[]);
    master.extend(top_group(b"RACE", &main_record(b"RACE", 0, 0x800, &race)));
    let patch = header(0, 0, 0x800, &["Fallout4.esm"]);
    let files = load_game(vec![("Fallout4.esm", master), ("Patch.esp", patch)]);
    let (registry, mut session) = session(&files);
    call(
        &registry,
        &mut session,
        "records.copy",
        json!({ "form_id": "00000800", "from": "Fallout4.esm", "to": "Patch.esp" }),
    );
    let full = call(
        &registry,
        &mut session,
        "records.compare",
        json!({ "form_id": "00000800" }),
    );
    assert!(full.get("truncated").is_none());
    let limited = call(
        &registry,
        &mut session,
        "records.compare",
        json!({ "form_id": "00000800", "limit": 3, "compact": true }),
    );
    assert_eq!(count_rows(&limited["rows"]), 3);
    assert_eq!(limited["truncated"]["listed_rows"], 3);
    assert_eq!(limited["truncated"]["total_rows"], count_rows(&full["rows"]));
    assert_eq!(limited["truncated"]["truncated"], true);
    assert!(limited["rows"][0]["cells"][0].get("path").is_none(), "{limited}");
    let top = call(
        &registry,
        &mut session,
        "records.compare",
        json!({ "form_id": "00000800", "depth": 1 }),
    );
    assert_eq!(count_rows(&top["rows"]), full["rows"].as_array().unwrap().len());
}

/// A master the folder of the plugin lacks loads from a master search path.
#[test]
fn masters_load_from_the_master_search_paths() {
    let _guard = test_lock();
    let data = xedit_io::temp::TempDir::new("xedit-agent-reports-masters-data");
    let mods = xedit_io::temp::TempDir::new("xedit-agent-reports-masters-mod");
    let mut master = header(1, 1, 0x801, &[]);
    master.extend(top_group(
        b"STAT",
        &main_record(b"STAT", 0, 0x800, &sub_record(b"EDID", b"TestStatic\0")),
    ));
    std::fs::write(data.path().join("Fallout4.esm"), master).unwrap();
    let plugin_path = mods.path().join("Mod.esp");
    std::fs::write(&plugin_path, header(0, 0, 0x800, &["Fallout4.esm"])).unwrap();
    load_game(Vec::new());
    let plugin_path = plugin_path.to_string_lossy().into_owned();
    xedit_core::interface::globals::set_master_search_paths(Vec::new());
    assert!(xedit_core::implementation::wb_file(&plugin_path, i32::MAX, FileStates::empty()).is_err());
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::globals::set_master_search_paths(vec![data.path().to_string_lossy().into_owned()]);
    let loaded = xedit_core::implementation::wb_file(&plugin_path, i32::MAX, FileStates::empty());
    xedit_core::interface::globals::set_master_search_paths(Vec::new());
    let plugin = loaded.expect("the master loads from the search path");
    let master = plugin.get_master(0, false).expect("the master");
    assert_eq!(master.get_name(), "Fallout4.esm");
    xedit_core::implementation::clear_files_map();
}
