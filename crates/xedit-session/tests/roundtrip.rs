// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The save path on a synthetic Skyrim SE plugin: a loaded file writes back
//! byte for byte, `PrepareSave` corrects the record count of the header,
//! an edited element is written from its storage, `elements.set` changes a
//! value and adds a missing member, and a mutating command needs the edit
//! flag.

use serde_json::json;
use xedit_core::implementation::{FileBytes, ResetModified, wb_file_from_bytes};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{Container, GameMode, MainRecord};
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

fn group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(label);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

/// A plugin without masters: the header with `record_count` in `HEDR`, and
/// one game setting with the editor ID and the value given.
fn plugin_with(record_count: u32, editor_id: &[u8], value: f32) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&record_count.to_le_bytes());
    hedr.extend_from_slice(&0x801u32.to_le_bytes());
    let mut header_data = sub_record(b"HEDR", &hedr);
    header_data.extend(sub_record(b"CNAM", b"xedit-rust\0"));
    header_data.extend(sub_record(b"INCC", &0u32.to_le_bytes()));
    let mut gmst_data = sub_record(b"EDID", editor_id);
    gmst_data.extend(sub_record(b"DATA", &value.to_le_bytes()));
    let mut bytes = main_record(b"TES4", 0, 0, &header_data);
    bytes.extend(group(b"GMST", &main_record(b"GMST", 0, 0x800, &gmst_data)));
    bytes
}

fn plugin(record_count: u32) -> Vec<u8> {
    plugin_with(record_count, b"fRoundTrip\0", 1.5)
}

fn load(name: &str, bytes: Vec<u8>) -> std::sync::Arc<xedit_core::implementation::FileImpl> {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(xedit_core::interface::GameMode::gmSSE);
    wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap()
}

#[test]
fn unmodified_plugin_writes_back_byte_for_byte() {
    let _guard = test_lock();
    // The header, the group and the record: three counted, two in HEDR.
    let bytes = plugin(2);
    let file = load("RoundTrip.esp", bytes.clone());
    let loaded_crc = file.crc32();
    let saved = file.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert_eq!(saved, bytes);
    assert_eq!(file.crc32(), loaded_crc);
}

#[test]
fn header_record_count_is_corrected_on_save() {
    let _guard = test_lock();
    let file = load("RoundTripCount.esp", plugin(7));
    let saved = file.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert_eq!(saved, plugin(2));
}

#[test]
fn edited_elements_are_written() {
    let _guard = test_lock();
    let file = load("RoundTripEdit.esp", plugin(2));
    xedit_core::interface::globals::set_edit_allowed(true);
    let record = file.records().into_iter().next().unwrap();
    let data = record.get_element_by_path("DATA\\Float").unwrap();
    assert_eq!(data.get_edit_value(), "1.500000");
    data.set_edit_value("2.25").unwrap();
    assert_eq!(data.get_edit_value(), "2.250000");
    // A longer editor ID resizes its subrecord.
    let edid = record.get_element_by_path("EDID").unwrap();
    edid.set_edit_value("fRoundTripEdited").unwrap();
    assert_eq!(record.get_editor_id(), "fRoundTripEdited");
    let saved = file.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    xedit_core::interface::globals::set_edit_allowed(false);
    assert_eq!(saved, plugin_with(2, b"fRoundTripEdited\0", 2.25));
}

#[test]
fn elements_set_changes_and_adds() {
    let _guard = test_lock();
    let file = load("RoundTripSet.esp", plugin(2));
    let registry = Registry::standard();
    let mut session = Session::with_files(GameMode::gmSSE, vec![file.clone()]);
    session.allow_edit(true);
    let result = registry
        .call(
            &mut session,
            "elements.set",
            json!({ "form_id": "00000800", "path": "DATA\\Float", "value": "3.5" }),
        )
        .unwrap();
    assert_eq!(result["old"]["value"], "1.500000");
    assert_eq!(result["new"]["value"], "3.500000");
    assert_eq!(result["changed"], true);
    // A native value through the same command.
    let result = registry
        .call(
            &mut session,
            "elements.set",
            json!({ "form_id": "00000800", "path": "DATA\\Float", "value": 4.0 }),
        )
        .unwrap();
    assert_eq!(result["new"]["value"], "4.000000");
    // A member the record lacks is added by its name (`ElementEditValues`):
    // the file header has no SNAM description yet.
    xedit_core::interface::globals::set_edit_allowed(true);
    let header = file.header().unwrap();
    assert!(header.get_element_by_path("SNAM").is_none());
    header.set_element_edit_value("SNAM", "made by the test").unwrap();
    let snam = header.get_element_by_path("SNAM").unwrap();
    assert_eq!(snam.get_edit_value(), "made by the test");
    xedit_core::interface::globals::set_edit_allowed(false);
    // The save carries the edits, and the header count stays right.
    let saved = registry
        .call(&mut session, "files.save", json!({ "dry_run": true }))
        .unwrap();
    assert_eq!(saved["changed"], true);
    assert_eq!(saved["written"], false);
    session.allow_edit(false);
    let bytes = file.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert!(
        bytes.windows(4).any(|w| w == b"SNAM"),
        "the saved file has the added SNAM"
    );
    assert!(
        bytes.windows(4).any(|w| w == 4.0f32.to_le_bytes()),
        "the saved file has the new value"
    );
}

#[test]
fn mutating_command_needs_the_edit_flag() {
    let _guard = test_lock();
    let registry = Registry::standard();
    assert_eq!(registry.mutates("files.save"), Some(true));
    let mut session = Session::default();
    let error = registry.call(&mut session, "files.save", json!({})).unwrap_err();
    assert_eq!(error.code, "edit_required");
    // A dry run passes the gate and fails later, on the missing session.
    let error = registry
        .call(&mut session, "files.save", json!({ "dry_run": true }))
        .unwrap_err();
    assert_eq!(error.code, "no_session");
}
