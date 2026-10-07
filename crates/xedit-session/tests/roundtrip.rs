// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The save path on a synthetic Skyrim SE plugin: a loaded file writes back
//! byte for byte, `PrepareSave` reports a record count the header does not
//! match, and a mutating command needs the edit flag.

use serde_json::json;
use xedit_core::implementation::{FileBytes, ResetModified, SaveError, wb_file_from_bytes};
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
/// one game setting.
fn plugin(record_count: u32) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&record_count.to_le_bytes());
    hedr.extend_from_slice(&0x801u32.to_le_bytes());
    let mut header_data = sub_record(b"HEDR", &hedr);
    header_data.extend(sub_record(b"CNAM", b"xedit-rust\0"));
    header_data.extend(sub_record(b"INCC", &0u32.to_le_bytes()));
    let mut gmst_data = sub_record(b"EDID", b"fRoundTrip\0");
    gmst_data.extend(sub_record(b"DATA", &1.5f32.to_le_bytes()));
    let mut bytes = main_record(b"TES4", 0, 0, &header_data);
    bytes.extend(group(b"GMST", &main_record(b"GMST", 0, 0x800, &gmst_data)));
    bytes
}

fn load(name: &str, bytes: Vec<u8>) -> std::sync::Arc<xedit_core::implementation::FileImpl> {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
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
fn header_record_count_off_is_reported_as_unsupported() {
    let _guard = test_lock();
    let file = load("RoundTripCount.esp", plugin(7));
    match file.write_to_bytes(ResetModified::rmSetInternal) {
        Err(SaveError::Unsupported(message)) => assert!(message.contains("HEDR holds 7"), "{message}"),
        other => panic!("expected an unsupported save, got {other:?}"),
    }
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
