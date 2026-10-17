// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The quick init of a record leaves `Init` at the first subrecord past
//! `QuickInitLimit` (issue #86, Fallout 76's `SeventySix.esm`): an error
//! past the limit is reported by the full init only, with the record's
//! names, and the name an official file caches is the record's own.

use xedit_core::implementation::{FileBytes, wb_file_from_bytes};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{File, FormID};

fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn main_record(signature: &[u8; 4], form_id: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
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
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

/// A game master (an official file, whose records cache their names) with
/// one class: `EDID` and `FULL` (the quick init limit), `DESC` past the
/// limit, then a subrecord the definition does not know.
fn master_bytes() -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&1u32.to_le_bytes());
    hedr.extend_from_slice(&0x801u32.to_le_bytes());
    let mut bytes = main_record(b"TES4", 1, &sub_record(b"HEDR", &hedr));
    let mut class = sub_record(b"EDID", b"TestClass\0");
    class.extend(sub_record(b"FULL", b"Test Class\0"));
    class.extend(sub_record(b"DESC", b"A class\0"));
    class.extend(sub_record(b"ZZZZ", &[1, 2, 3, 4]));
    bytes.extend(top_group(b"CLAS", &main_record(b"CLAS", 0x800, &class)));
    bytes
}

#[test]
fn a_record_named_first_keeps_its_names_past_an_error_beyond_the_quick_init_limit() {
    let _guard = test_lock();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    let file = wb_file_from_bytes("Skyrim.esm", 0, FileStates::empty(), FileBytes::Owned(master_bytes())).unwrap();
    let record = file
        .get_record_by_form_id(FormID::from_cardinal(0x800), false, false)
        .unwrap()
        .expect("the class");

    // The display name is read first, as the dump's line of the record
    // does: it runs the quick init.
    assert_eq!(record.get_display_name(true), "Test Class");
    let name = "CLAS - Class [00000800] <TestClass> \"Test Class\"";
    assert_eq!(record.get_name(), name);
    // The full init reports the subrecord it does not know and keeps the
    // name.
    assert!(record.as_container().unwrap().get_element_count() > 0);
    assert_eq!(record.get_name(), name);
    xedit_core::implementation::clear_files_map();
}
