// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `patch.merged` on synthetic Skyrim SE plugins: two plugins change the
//! entries of a leveled list of the game master in different ways, and the
//! merged patch keeps both changes.

use std::sync::Arc;

use serde_json::json;

use xedit_core::implementation::{FileBytes, FileImpl, wb_file_from_bytes};
use xedit_core::interface::globals::{set_data_path, test_lock};
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{Container, GameMode, Variant};
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

fn header(flags: u32, masters: &[&str], record_count: u32) -> Vec<u8> {
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
        data.extend(sub_record(b"DATA", &[0; 8]));
    }
    main_record(b"TES4", flags, 0, &data)
}

/// A leveled item whose entries are (level, reference) pairs.
fn leveled_item(entries: &[(u16, u32)]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", b"TestList\0");
    data.extend(sub_record(b"OBND", &[0; 12]));
    data.extend(sub_record(b"LVLD", &[0]));
    data.extend(sub_record(b"LVLF", &[1]));
    data.extend(sub_record(b"LLCT", &[entries.len() as u8]));
    for (level, reference) in entries {
        let mut entry = Vec::new();
        entry.extend_from_slice(&level.to_le_bytes());
        entry.extend_from_slice(&[0, 0]);
        entry.extend_from_slice(&reference.to_le_bytes());
        entry.extend_from_slice(&1u16.to_le_bytes());
        entry.extend_from_slice(&[0, 0]);
        data.extend(sub_record(b"LVLO", &entry));
    }
    main_record(b"LVLI", 0, 0x800, &data)
}

fn plugin(flags: u32, masters: &[&str], entries: &[(u16, u32)]) -> Vec<u8> {
    let mut bytes = header(flags, masters, 1);
    bytes.extend(group(b"LVLI", &leveled_item(entries)));
    bytes
}

fn load() -> (Vec<Arc<FileImpl>>, Session) {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game_for_edit("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let folder = std::env::temp_dir().join(format!("xedit-patch-test-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    set_data_path(&format!("{}\\", folder.display()));
    let master = plugin(1, &[], &[(1, 0xA01), (1, 0xA02), (5, 0xA03)]);
    // The first plugin removes A01 and adds A04, the second adds A05.
    let first = plugin(0, &["Skyrim.esm"], &[(1, 0xA02), (5, 0xA03), (2, 0xA04)]);
    let second = plugin(0, &["Skyrim.esm"], &[(1, 0xA01), (1, 0xA02), (5, 0xA03), (3, 0xA05)]);
    let mut files = Vec::new();
    for (name, bytes) in [("Skyrim.esm", master), ("First.esp", first), ("Second.esp", second)] {
        files.push(wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap());
    }
    let mut session = Session::with_files(GameMode::gmSSE, files.clone());
    session.allow_edit(true);
    (files, session)
}

/// The references of the entries of the patch's leveled list.
fn patch_entries(session: &mut Session) -> Vec<u64> {
    let _ = session;
    let file = xedit_core::interface::files()
        .into_iter()
        .find(|file| file.get_name() == "Merged.esp")
        .and_then(|file| file.as_element_impl()?.file_impl())
        .unwrap();
    // The game master is the patch's master twice, and its FormIDs point to
    // the second entry, as in xEdit's patches.
    let record = file.records().into_iter().next().expect("the patch overrides the list");
    assert_eq!(record.form_id().to_cardinal(), 0x0100_0800);
    let entries = record.get_element_by_name("Leveled List Entries").unwrap();
    let container = entries.as_container().unwrap();
    (0..container.get_element_count())
        .map(|index| {
            let entry = container.get_element(index).unwrap();
            let lvlo = entry.as_container().unwrap().get_element(0).unwrap();
            let reference = lvlo.as_container().unwrap().get_element(2).unwrap();
            match reference.get_native_value() {
                Variant::UInt(value) => value,
                Variant::Int(value) => value as u64,
                other => panic!("{other:?}"),
            }
        })
        .collect()
}

#[test]
fn the_patch_keeps_the_changes_of_both_plugins() {
    let _lock = test_lock();
    let (_files, mut session) = load();
    let registry = Registry::standard();

    let dry = registry
        .call(
            &mut session,
            "patch.merged",
            json!({ "file": "Merged", "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["records"][0]["form_id"], "00000800");
    assert_eq!(dry["records"][0]["lists"][0]["entries"], 4);
    assert_eq!(dry["records"][0]["lists"][0]["winning_entries"], 4);
    assert!(dry["warning"].as_str().unwrap().contains("unsupported"));
    assert!(
        !xedit_core::interface::files()
            .iter()
            .any(|file| file.get_name() == "Merged.esp")
    );

    let made = registry
        .call(&mut session, "patch.merged", json!({ "file": "Merged.esp" }))
        .unwrap();
    assert_eq!(made["file"], "Merged.esp");
    assert_eq!(made["unsaved"], true);
    // The game master twice: `AddNewFile` adds it, and so does the list of
    // every loaded file (UPSTREAM-QUIRK); the clean keeps both, and drops
    // the plugins, as every FormID of the patch is the game master's.
    assert_eq!(made["masters"], json!(["Skyrim.esm", "Skyrim.esm"]));
    let mut references = patch_entries(&mut session);
    references.sort_unstable();
    assert_eq!(references, [0x0100_0A02, 0x0100_0A03, 0x0100_0A04, 0x0100_0A05]);

    let again = registry
        .call(&mut session, "patch.merged", json!({ "file": "Merged" }))
        .unwrap_err();
    assert_eq!(again.code, "file_exists");
}

#[test]
fn a_dry_run_needs_no_edit_flag_and_the_name_is_checked() {
    let _lock = test_lock();
    let (_files, mut session) = load();
    session.allow_edit(false);
    let registry = Registry::standard();
    let error = registry
        .call(&mut session, "patch.merged", json!({ "file": "Merged" }))
        .unwrap_err();
    assert_eq!(error.code, "edit_required");
    let error = registry
        .call(&mut session, "patch.merged", json!({ "file": "a|b", "dry_run": true }))
        .unwrap_err();
    assert_eq!(error.code, "invalid_params");
}
