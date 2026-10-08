// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The save behaviour that `cargo xtask parity oracle-save` found missing
//! when it compared the port's saves with the GUI oracle's, on synthetic
//! plugins: a sorted array of subrecords sorts when its record is rebuilt,
//! a reference set persistent moves to the persistent children of its
//! cell, and the empty children group of a quest whose label has a FileID
//! beyond the masters takes the record's FormID.

use std::sync::Arc;

use serde_json::json;
use xedit_core::implementation::{FileBytes, FileImpl, ResetModified, wb_file_from_bytes};
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

fn main_record(signature: &[u8; 4], flags: u32, form_id: u32, version: u16, data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&form_id.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&version.to_le_bytes());
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

fn header(version: u16, record_count: u32) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&record_count.to_le_bytes());
    hedr.extend_from_slice(&0x900u32.to_le_bytes());
    let mut data = sub_record(b"HEDR", &hedr);
    data.extend(sub_record(b"CNAM", b"xedit-rust\0"));
    data.extend(sub_record(b"INCC", &1u32.to_le_bytes()));
    main_record(b"TES4", 1, 0, version, &data)
}

fn open(game: &str, mode: GameMode, name: &str, bytes: Vec<u8>) -> Arc<FileImpl> {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game(game).unwrap();
    xedit_session::save::apply_edit_settings(mode);
    let file = wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap();
    xedit_core::interface::add_file(file.clone());
    file
}

/// The main records and groups of a saved file in file order, as
/// `(signature or GRUP, FormID or label, group type)`.
fn layout(bytes: &[u8]) -> Vec<(String, u32, i32)> {
    let mut out = Vec::new();
    let data_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let mut offset = 24 + data_size;
    while offset + 24 <= bytes.len() {
        let signature = String::from_utf8_lossy(&bytes[offset..offset + 4]).into_owned();
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let id = u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
        if signature == "GRUP" {
            let label = u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap());
            out.push((signature, label, id as i32));
            offset += 24;
        } else {
            out.push((signature, id, -1));
            offset += 24 + size;
        }
    }
    out
}

/// The data of the subrecords with the signature in the first record with
/// the FormID.
fn sub_records_of(bytes: &[u8], form_id: u32, wanted: &[u8; 4]) -> Vec<Vec<u8>> {
    let data_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let mut offset = 24 + data_size;
    while offset + 24 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &bytes[offset..offset + 4] == b"GRUP" {
            offset += 24;
            continue;
        }
        let id = u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
        if id == form_id {
            let data = &bytes[offset + 24..offset + 24 + size];
            let mut result = Vec::new();
            let mut pos = 0;
            while pos + 6 <= data.len() {
                let length = u16::from_le_bytes(data[pos + 4..pos + 6].try_into().unwrap()) as usize;
                if &data[pos..pos + 4] == wanted {
                    result.push(data[pos + 6..pos + 6 + length].to_vec());
                }
                pos += 6 + length;
            }
            return result;
        }
        offset += 24 + size;
    }
    Vec::new()
}

fn lvlo(level: i16, reference: u32, count: i16) -> Vec<u8> {
    let mut data = level.to_le_bytes().to_vec();
    data.extend_from_slice(&[0, 0]);
    data.extend_from_slice(&reference.to_le_bytes());
    data.extend_from_slice(&count.to_le_bytes());
    data.extend_from_slice(&[0, 0]);
    sub_record(b"LVLO", &data)
}

#[test]
fn a_sorted_array_of_subrecords_sorts_when_its_record_is_rebuilt() {
    let _guard = test_lock();
    // `Leveled List Entries` is a sorted array (`wbRArrayS`); the entries
    // are stored with level 5 before level 1.
    let mut data = sub_record(b"EDID", b"TestList\0");
    data.extend(sub_record(b"LLCT", &[2]));
    data.extend(lvlo(5, 0x0000_000F, 1));
    data.extend(lvlo(1, 0x0000_000F, 2));
    let mut bytes = header(44, 1);
    bytes.extend(top_group(b"LVLI", &main_record(b"LVLI", 0, 0x800, 44, &data)));
    let file = open("sse", GameMode::gmSSE, "Sorted.esm", bytes);
    let mut session = Session::with_files(GameMode::gmSSE, vec![file.clone()]);
    session.allow_edit(true);
    let registry = Registry::standard();
    // An unmodified record is copied as loaded.
    let saved = file.write_to_bytes(ResetModified::rmNo).unwrap();
    let levels = |bytes: &[u8]| -> Vec<u8> { sub_records_of(bytes, 0x800, b"LVLO").iter().map(|d| d[0]).collect() };
    assert_eq!(levels(&saved), [5, 1]);
    registry
        .call(
            &mut session,
            "elements.set",
            json!({ "form_id": "00000800", "path": "EDID", "value": "TestListEdited" }),
        )
        .unwrap();
    let saved = file.write_to_bytes(ResetModified::rmNo).unwrap();
    assert_eq!(levels(&saved), [1, 5]);
}

#[test]
fn a_reference_set_persistent_moves_to_the_persistent_children() {
    let _guard = test_lock();
    let cell = {
        let mut data = sub_record(b"EDID", b"TestCell\0");
        data.extend(sub_record(b"DATA", &1u16.to_le_bytes()));
        main_record(b"CELL", 0, 0x801, 44, &data)
    };
    let reference = {
        let mut data = sub_record(b"NAME", &0x0000_0007u32.to_le_bytes());
        data.extend(sub_record(b"DATA", &[0; 24]));
        main_record(b"REFR", 0, 0x802, 44, &data)
    };
    let children = group(0x801, 6, &group(0x801, 9, &reference));
    let mut cells = cell.clone();
    cells.extend(children);
    let mut bytes = header(44, 6);
    bytes.extend(top_group(b"CELL", &group(1, 2, &group(0, 3, &cells))));
    let file = open("sse", GameMode::gmSSE, "Persistent.esm", bytes);
    xedit_core::interface::globals::set_edit_allowed(true);
    let record = file
        .records()
        .into_iter()
        .find(|record| record.get_signature().to_string() == "REFR")
        .unwrap();
    record.set_is_persistent(true);
    xedit_core::interface::globals::set_edit_allowed(false);
    let saved = file.write_to_bytes(ResetModified::rmNo).unwrap();
    let layout = layout(&saved);
    let position = layout.iter().position(|entry| entry.0 == "REFR").unwrap();
    // The reference follows a persistent children group of the cell; the
    // emptied temporary group is gone.
    assert_eq!(layout[position - 1], ("GRUP".to_owned(), 0x801, 8));
    assert!(!layout.iter().any(|entry| entry.0 == "GRUP" && entry.2 == 9));
}

#[test]
fn the_empty_children_group_of_a_quest_takes_its_formid() {
    let _guard = test_lock();
    // Fallout4.esm has a quest followed by an empty quest children group
    // labelled with FileID 01 in a file without masters; `GetGroupLabel`
    // reads it as the file's own FileID, so the group belongs to the quest,
    // and the save writes the quest's FormID into the label
    // (`ClampFormID`).
    let quest = main_record(b"QUST", 0, 0x89C, 131, &sub_record(b"EDID", b"TestQuest\0"));
    let mut records = quest;
    records.extend(group(0x0100_089C, 10, &[]));
    let mut bytes = header(131, 3);
    bytes.extend(top_group(b"QUST", &records));
    let file = open("fo4", GameMode::gmFO4, "Quest.esm", bytes);
    let saved = file.write_to_bytes(ResetModified::rmNo).unwrap();
    let layout = layout(&saved);
    assert!(layout.contains(&("GRUP".to_owned(), 0x0000_089C, 10)), "{layout:x?}");
}
