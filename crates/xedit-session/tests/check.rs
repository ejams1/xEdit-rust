// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `files.check` ("Check for Errors") on synthetic Skyrim SE plugins: an
//! unresolved FormID, a FormID of the wrong record type, a deleted record
//! with data, an object ID beyond a light module's range, and the counts
//! and message lines of the log.

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
    hedr.extend_from_slice(&0x2000u32.to_le_bytes());
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

fn gmst(flags: u32, form_id: u32, editor_id: &[u8], value: f32) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"DATA", &value.to_le_bytes()));
    main_record(b"GMST", flags, form_id, &data)
}

/// An outfit whose items are the FormIDs given.
fn outfit(form_id: u32, editor_id: &[u8], items: &[u32]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    let ids: Vec<u8> = items.iter().flat_map(|id| id.to_le_bytes()).collect();
    data.extend(sub_record(b"INAM", &ids));
    main_record(b"OTFT", 0, form_id, &data)
}

/// The master: a game setting.
fn master_bytes() -> Vec<u8> {
    let mut bytes = header(1, 2, &[]);
    bytes.extend(top_group(b"GMST", &gmst(0, 0x800, b"fMaster\0", 1.5)));
    bytes
}

/// A plugin with a clean game setting, a deleted one with data, and an
/// outfit with an unresolved item and a game setting as an item.
fn plugin_bytes() -> Vec<u8> {
    let mut bytes = header(0, 4, &["Master.esm"]);
    let mut settings = gmst(0, 0x0100_0800, b"fClean\0", 2.5);
    settings.extend(gmst(0x20, 0x0100_0801, b"fDeleted\0", 3.5));
    bytes.extend(top_group(b"GMST", &settings));
    bytes.extend(top_group(
        b"OTFT",
        &outfit(0x0100_0802, b"TestOutfit\0", &[0x0100_0ABC, 0x0000_0800]),
    ));
    bytes
}

/// A light plugin with a record beyond the 4096 object IDs of a light
/// module.
fn light_bytes() -> Vec<u8> {
    let mut bytes = header(0x200, 2, &["Master.esm"]);
    bytes.extend(top_group(b"GMST", &gmst(0, 0x0100_1000, b"fLight\0", 1.0)));
    bytes
}

fn load(plugin: (&str, Vec<u8>)) -> (Vec<Arc<FileImpl>>, Session) {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game_for_edit("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let mut files = Vec::new();
    for (name, bytes) in [("Master.esm", master_bytes()), plugin] {
        let file = wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap();
        xedit_core::interface::add_file(file.clone());
        files.push(file);
    }
    let session = Session::with_files(GameMode::gmSSE, vec![files[1].clone()]);
    (files, session)
}

fn messages(result: &Value) -> Vec<String> {
    result["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn reports_the_errors_of_a_plugin() {
    let _lock = test_lock();
    let (_files, mut session) = load(("Plugin.esp", plugin_bytes()));
    let result = Registry::standard()
        .call(&mut session, "files.check", json!({}))
        .unwrap();
    let lines = messages(&result);
    assert_eq!(lines[0], "Start: Checking for Errors");
    assert_eq!(lines[1], "Checking for Errors in [01] Plugin.esp");
    // The file header, two settings and the outfit.
    assert_eq!(result["checked"], 4);
    assert_eq!(result["errors_found"], 2);
    assert_eq!(result["exit_code"], 2);
    let records = result["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["form_id"], "01000801");
    let deleted = records[0]["errors"][0]["error"].as_str().unwrap();
    assert!(
        deleted.starts_with("Record marked as deleted, but contains: "),
        "{deleted}"
    );
    assert_eq!(records[0]["errors"][0]["path"], "GMST");
    assert_eq!(records[1]["form_id"], "01000802");
    let errors: Vec<(&str, &str)> = records[1]["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|error| (error["path"].as_str().unwrap(), error["error"].as_str().unwrap()))
        .collect();
    assert_eq!(
        errors,
        [
            (
                "OTFT \\ INAM - Items \\ Item",
                "Found a GMST reference, expected: ARMO,LVLI"
            ),
            (
                "OTFT \\ INAM - Items \\ Item",
                "[01000ABC] <Error: Could not be resolved>"
            ),
        ]
    );
    // Each record line is followed by its errors.
    assert_eq!(lines[2], records[0]["name"].as_str().unwrap());
    assert_eq!(lines[3], format!("    GMST -> {deleted}"));
    assert_eq!(
        lines.last().unwrap(),
        "Done: Checking for Errors, Processed Records: 4, Errors found: 2"
    );
}

#[test]
fn checks_one_record_and_a_light_module() {
    let _lock = test_lock();
    let (_files, mut session) = load(("Plugin.esp", plugin_bytes()));
    let result = Registry::standard()
        .call(&mut session, "files.check", json!({ "records": ["01000800"] }))
        .unwrap();
    assert_eq!(result["checked"], 1);
    assert_eq!(result["errors_found"], 0);

    let (_files, mut session) = load(("Light.esp", light_bytes()));
    let result = Registry::standard()
        .call(&mut session, "files.check", json!({ "files": ["Light.esp"] }))
        .unwrap();
    assert_eq!(result["errors_found"], 1);
    assert_eq!(
        result["records"][0]["errors"][0]["error"],
        "ObjectID 001000 is invalid for a light module."
    );
}

#[test]
fn an_unknown_file_is_an_error() {
    let _lock = test_lock();
    let (_files, mut session) = load(("Plugin.esp", plugin_bytes()));
    let error = Registry::standard()
        .call(&mut session, "files.check", json!({ "files": ["Missing.esp"] }))
        .unwrap_err();
    assert_eq!(error.code, "unknown_file");
}

#[test]
fn the_thread_count_does_not_change_the_result() {
    let _lock = test_lock();
    let (_files, mut session) = load(("Plugin.esp", plugin_bytes()));
    let many = Registry::standard()
        .call(&mut session, "files.check", json!({}))
        .unwrap();
    xedit_core::threads::set_threads(1);
    let one = Registry::standard()
        .call(&mut session, "files.check", json!({}))
        .unwrap();
    xedit_core::threads::set_threads(0);
    assert_eq!(many, one);
}
