// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The parallel dump writes what the serial dump writes: a synthetic Skyrim
//! SE plugin whose form lists show the names of thousands of keywords (each
//! keyword record is built by whichever thread reads it first) dumps to the
//! same bytes on one thread and on several, also when the records were
//! built before.

use xedit_core::implementation::{FileBytes, wb_file_from_bytes};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
use xedit_core::threads::set_threads;
use xedit_session::dump::write_dump;

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

const KEYWORDS: u32 = 6000;
const LISTS: u32 = 400;

/// Keywords `[00000800]` on, and form lists after them that each list 40
/// keywords spread over the whole range, some of them missing.
fn plugin() -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&(KEYWORDS + LISTS).to_le_bytes());
    hedr.extend_from_slice(&(0x800 + KEYWORDS + LISTS).to_le_bytes());
    let mut header = sub_record(b"HEDR", &hedr);
    header.extend(sub_record(b"CNAM", b"xedit-rust\0"));
    let mut keywords = Vec::new();
    for index in 0..KEYWORDS {
        let mut data = sub_record(b"EDID", format!("Keyword{index}\0").as_bytes());
        data.extend(sub_record(b"CNAM", &index.to_le_bytes()));
        keywords.extend(main_record(b"KYWD", 0x800 + index, &data));
    }
    let mut lists = Vec::new();
    for index in 0..LISTS {
        let mut data = sub_record(b"EDID", format!("List{index}\0").as_bytes());
        for entry in 0..40u32 {
            // Some entries point past the keywords, to a record that
            // does not exist.
            let target = 0x800 + (index * 97 + entry * 211) % (KEYWORDS + 50);
            data.extend(sub_record(b"LNAM", &target.to_le_bytes()));
        }
        lists.extend(main_record(b"FLST", 0x800 + KEYWORDS + index, &data));
    }
    let mut bytes = main_record(b"TES4", 0, &header);
    bytes.extend(group(b"KYWD", &keywords));
    bytes.extend(group(b"FLST", &lists));
    bytes
}

fn dump(file: &xedit_core::implementation::FileImpl, threads: usize) -> Vec<u8> {
    set_threads(threads);
    let mut out = Vec::new();
    write_dump(file, &mut out).unwrap();
    set_threads(0);
    out
}

#[test]
fn one_thread_and_several_write_the_same_dump() {
    let _guard = test_lock();
    xedit_core::implementation::clear_files_map();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    let file = wb_file_from_bytes(
        "Parallel.esp",
        i32::MAX,
        FileStates::empty(),
        FileBytes::Owned(plugin()),
    )
    .unwrap();
    // The parallel dump first, while no record has been built.
    let parallel = dump(&file, 8);
    let serial = dump(&file, 1);
    assert!(serial.len() > 500_000, "the dump has {} bytes", serial.len());
    let text = String::from_utf8_lossy(&serial);
    assert!(text.contains("<Keyword5999>"), "the form lists show the keyword names");
    assert!(parallel == serial, "the parallel dump differs from the serial one");
    assert!(dump(&file, 3) == serial, "the dump on 3 threads differs");
}

fn load(threads: usize) -> std::sync::Arc<xedit_core::implementation::FileImpl> {
    set_threads(threads);
    xedit_core::implementation::clear_files_map();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(xedit_core::interface::GameMode::gmSSE);
    let file = wb_file_from_bytes(
        "Parallel.esp",
        i32::MAX,
        FileStates::empty(),
        FileBytes::Owned(plugin()),
    )
    .unwrap();
    set_threads(0);
    file
}

#[test]
fn a_plugin_scanned_on_one_thread_and_on_several_saves_and_dumps_the_same() {
    let _guard = test_lock();
    let serial = load(1);
    let serial_saved = serial
        .write_to_bytes(xedit_core::implementation::ResetModified::rmSetInternal)
        .unwrap();
    let serial_dump = dump(&serial, 1);
    let parallel = load(8);
    assert_eq!(parallel.records().len(), serial.records().len());
    let parallel_saved = parallel
        .write_to_bytes(xedit_core::implementation::ResetModified::rmSetInternal)
        .unwrap();
    assert!(parallel_saved == serial_saved, "the saved bytes differ");
    assert!(dump(&parallel, 8) == serial_dump, "the dumps differ");
}
