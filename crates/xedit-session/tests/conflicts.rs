// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `conflicts.list` and `records.compare` on synthetic Skyrim SE plugins: a
//! global overridden by two plugins with different values, a global
//! overridden unchanged, a single record, two game settings of different
//! FormIDs compared by their editor ID, the same statuses on one thread and
//! on several, and the rows of the view of a record.

use std::sync::Arc;

use serde_json::{Value, json};
use xedit_core::implementation::{FileBytes, FileImpl, wb_file_from_bytes};
use xedit_core::interface::GameMode;
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
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

fn top_group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(label);
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

fn header(flags: u32, record_count: u32, masters: &[&str]) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&record_count.to_le_bytes());
    hedr.extend_from_slice(&0x900u32.to_le_bytes());
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

fn global(form_id: u32, editor_id: &[u8], value: f32) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"FNAM", b"f"));
    data.extend(sub_record(b"FLTV", &value.to_le_bytes()));
    main_record(b"GLOB", 0, form_id, &data)
}

fn gmst(form_id: u32, editor_id: &[u8], value: f32) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"DATA", &value.to_le_bytes()));
    main_record(b"GMST", 0, form_id, &data)
}

/// The master: three globals and a game setting.
fn master_bytes() -> Vec<u8> {
    let mut bytes = header(1, 4, &[]);
    bytes.extend(top_group(b"GMST", &gmst(0x800, b"fShared\0", 1.0)));
    let mut globals = global(0x801, b"Changed\0", 1.0);
    globals.extend(global(0x802, b"Unchanged\0", 2.0));
    globals.extend(global(0x803, b"Alone\0", 3.0));
    bytes.extend(top_group(b"GLOB", &globals));
    bytes
}

/// The first plugin: changes `Changed`, repeats `Unchanged`, and has a game
/// setting of its own FormID with the master's editor ID.
fn first_bytes() -> Vec<u8> {
    let mut bytes = header(0, 3, &["Master.esm"]);
    bytes.extend(top_group(b"GMST", &gmst(0x0100_0900, b"fShared\0", 4.0)));
    let mut globals = global(0x801, b"Changed\0", 5.0);
    globals.extend(global(0x802, b"Unchanged\0", 2.0));
    bytes.extend(top_group(b"GLOB", &globals));
    bytes
}

/// The second plugin: changes `Changed` again, to another value.
fn second_bytes() -> Vec<u8> {
    let mut bytes = header(0, 1, &["Master.esm"]);
    bytes.extend(top_group(b"GLOB", &global(0x801, b"Changed\0", 6.0)));
    bytes
}

fn load() -> Vec<Arc<FileImpl>> {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game_for_edit("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let mut files = Vec::new();
    for (name, bytes) in [
        ("Master.esm", master_bytes()),
        ("First.esp", first_bytes()),
        ("Second.esp", second_bytes()),
    ] {
        // The file joins upstream's `Files` as it loads.
        files.push(wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap());
    }
    files
}

fn conflicts(threads: usize) -> Value {
    xedit_core::threads::set_threads(threads);
    let files = load();
    let mut session = Session::with_files(GameMode::gmSSE, files);
    let result = Registry::standard()
        .call(&mut session, "conflicts.list", json!({ "include_single": true }))
        .unwrap();
    xedit_core::threads::set_threads(0);
    result
}

/// The `conflict_all/conflict_this` of the record of `form_id` in `file`.
fn status(result: &Value, file: &str, form_id: &str) -> String {
    let record = result["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["file"] == file && record["form_id"] == form_id)
        .unwrap_or_else(|| panic!("no record {form_id} in {file}"));
    format!(
        "{}/{}",
        record["conflict_all"].as_str().unwrap(),
        record["conflict_this"].as_str().unwrap()
    )
}

#[test]
fn statuses_of_the_records() {
    let _lock = test_lock();
    let result = conflicts(1);
    // Two overrides with different values: the last one wins.
    assert_eq!(status(&result, "Master.esm", "00000801"), "caConflict/ctMaster");
    assert_eq!(status(&result, "First.esp", "00000801"), "caConflict/ctConflictLoses");
    assert_eq!(status(&result, "Second.esp", "00000801"), "caConflict/ctConflictWins");
    // An override identical to its master.
    assert_eq!(status(&result, "Master.esm", "00000802"), "caNoConflict/ctMaster");
    assert_eq!(
        status(&result, "First.esp", "00000802"),
        "caNoConflict/ctIdenticalToMaster"
    );
    // A record without overrides.
    assert_eq!(status(&result, "Master.esm", "00000803"), "caOnlyOne/ctOnlyOne");
    // Game settings are compared by their editor ID, across FormIDs.
    assert_eq!(status(&result, "Master.esm", "00000800"), "caOverride/ctMaster");
    assert_eq!(status(&result, "First.esp", "01000900"), "caOverride/ctOverride");

    let files = result["files"].as_array().unwrap();
    let master = files.iter().find(|file| file["name"] == "Master.esm").unwrap();
    assert_eq!(master["records"], 4);
    assert_eq!(master["single"], 1);
    assert_eq!(master["conflict_all"], "caConflict");
    assert_eq!(master["conflict_this"], "ctMaster");
}

#[test]
fn filters_keep_the_statuses() {
    let _lock = test_lock();
    let files = load();
    let mut session = Session::with_files(GameMode::gmSSE, files);
    let registry = Registry::standard();
    let result = registry
        .call(
            &mut session,
            "conflicts.list",
            json!({ "files": ["First.esp"], "min_conflict_all": "caConflict" }),
        )
        .unwrap();
    assert_eq!(result["total"], 1);
    assert_eq!(status(&result, "First.esp", "00000801"), "caConflict/ctConflictLoses");
    let result = registry
        .call(
            &mut session,
            "conflicts.list",
            json!({ "conflict_this": ["ctIdenticalToMaster"] }),
        )
        .unwrap();
    assert_eq!(result["total"], 1);
    let error = registry
        .call(&mut session, "conflicts.list", json!({ "files": ["Missing.esp"] }))
        .unwrap_err();
    assert_eq!(error.code, "unknown_file");
}

#[test]
fn thread_count_does_not_change_the_result() {
    let _lock = test_lock();
    let serial = conflicts(1);
    let parallel = conflicts(4);
    assert_eq!(serial, parallel);
}

#[test]
fn compare_shows_the_rows_of_the_view() {
    let _lock = test_lock();
    let files = load();
    let mut session = Session::with_files(GameMode::gmSSE, files);
    let result = Registry::standard()
        .call(&mut session, "records.compare", json!({ "form_id": "00000801" }))
        .unwrap();
    assert_eq!(result["conflict_all"], "caConflict");
    let columns: Vec<String> = result["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| {
            format!(
                "{}={}",
                column["file"].as_str().unwrap(),
                column["conflict_this"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        columns,
        [
            "Master.esm=ctMaster",
            "First.esp=ctConflictLoses",
            "Second.esp=ctConflictWins"
        ]
    );
    let rows = result["rows"].as_array().unwrap();
    let value = rows
        .iter()
        .find(|row| row["name"] == "FLTV - Value")
        .unwrap_or_else(|| panic!("no value row in {rows:?}"));
    assert_eq!(value["conflict_all"], "caConflict");
    let cells: Vec<String> = value["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            format!(
                "{}={}",
                cell["value"].as_str().unwrap(),
                cell["conflict_this"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        cells,
        [
            "1.000000=ctMaster",
            "5.000000=ctConflictLoses",
            "6.000000=ctConflictWins"
        ]
    );
    // The editor ID is the same in every record.
    let editor_id = rows.iter().find(|row| row["name"] == "EDID - Editor ID").unwrap();
    assert_eq!(editor_id["conflict_all"], "caNoConflict");
}
