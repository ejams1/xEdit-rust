// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The master editing of `TwbFile` on synthetic Skyrim SE plugins:
//! `AddMasters` and `AddMastersIfMissing` with and without the sort, the
//! FormID remap of `MastersUpdated` (the record FormIDs, the FormIDs in the
//! elements, the labels of child groups), `SortMasters`, `CleanMasters`, and
//! the `masters.*` commands.

use std::sync::Arc;

use serde_json::json;

use xedit_core::implementation::{FileBytes, FileImpl, ResetModified, wb_file_from_bytes};
use xedit_core::interface::globals::{set_edit_allowed, test_lock};
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{Container, Element, GameMode, MainRecord, Variant};
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

fn group_of(label: [u8; 4], group_type: u32, records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(&label);
    bytes.extend_from_slice(&group_type.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

fn group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    group_of(*label, 0, records)
}

/// A file header with the masters and the record count given.
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
    data.extend(sub_record(b"INCC", &0u32.to_le_bytes()));
    main_record(b"TES4", flags, 0, &data)
}

/// A master without masters of its own with one keyword.
fn master(form_id: u32, editor_id: &[u8]) -> Vec<u8> {
    let mut bytes = header(1, &[], 2);
    bytes.extend(group(
        b"KYWD",
        &main_record(b"KYWD", 0, form_id, &sub_record(b"EDID", editor_id)),
    ));
    bytes
}

/// A form list whose entries are the FormIDs given.
fn form_list(form_id: u32, entries: &[u32]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", b"TestList\0");
    for entry in entries {
        data.extend(sub_record(b"LNAM", &entry.to_le_bytes()));
    }
    main_record(b"FLST", 0, form_id, &data)
}

/// A plugin with the masters given and one form list.
fn plugin(masters: &[&str], list_form_id: u32, entries: &[u32]) -> Vec<u8> {
    let mut bytes = header(0, masters, 2);
    bytes.extend(group(b"FLST", &form_list(list_form_id, entries)));
    bytes
}

/// Loads the masters and the plugin in the order given, `MasterB.esm`
/// first so that it sorts before `MasterA.esm`.
fn load(plugin_bytes: Vec<u8>, master_b_first: bool) -> Arc<FileImpl> {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let open = |name: &str, bytes: Vec<u8>| {
        wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap()
    };
    let a = || open("MasterA.esm", master(0x0000_0801, b"KeywordA\0"));
    let b = || open("MasterB.esm", master(0x0000_0802, b"KeywordB\0"));
    if master_b_first {
        b();
        a();
    } else {
        a();
        b();
    }
    open("Plugin.esp", plugin_bytes)
}

fn master_names(file: &FileImpl) -> Vec<String> {
    file.masters().iter().map(|master| master.get_name()).collect()
}

/// The file FormIDs of the form list: its own and its entries.
fn list_form_ids(file: &FileImpl) -> (u32, Vec<Variant>) {
    let record = file
        .records()
        .into_iter()
        .find(|record| record.get_signature().to_string() == "FLST")
        .unwrap();
    let form_ids = record.get_element_by_name("FormIDs").unwrap();
    let container = form_ids.as_container().unwrap();
    let entries = (0..container.get_element_count())
        .map(|index| container.get_element(index).unwrap().get_native_value())
        .collect();
    (record.form_id().to_cardinal(), entries)
}

#[test]
fn add_masters_moves_the_own_form_ids() {
    let _guard = test_lock();
    let file = load(
        plugin(&["MasterA.esm"], 0x0100_0803, &[0x0000_0801, 0x0100_0803]),
        false,
    );
    set_edit_allowed(true);
    file.add_masters_if_missing(&["MasterB.esm"], false, true).unwrap();
    assert_eq!(master_names(&file), ["MasterA.esm", "MasterB.esm"]);
    // The record of the plugin moves past the new master in the file; its
    // load order FormID (slot 2) stays.
    let (form_id, entries) = list_form_ids(&file);
    assert_eq!(form_id, 0x0200_0803);
    assert_eq!(entries, [Variant::UInt(0x0000_0801), Variant::UInt(0x0200_0803)]);
    let record = file.records()[0].clone();
    assert_eq!(record.get_load_order_form_id().to_cardinal(), 0x0200_0803);
    let saved = file.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    set_edit_allowed(false);
    assert_eq!(
        saved,
        plugin(
            &["MasterA.esm", "MasterB.esm"],
            0x0200_0803,
            &[0x0000_0801, 0x0200_0803]
        )
    );
}

#[test]
fn add_masters_if_missing_sorts_and_clean_masters_undoes_it() {
    let _guard = test_lock();
    let original = plugin(&["MasterA.esm"], 0x0100_0803, &[0x0000_0801, 0x0100_0803]);
    let file = load(original.clone(), true);
    set_edit_allowed(true);
    // An existing master and the file itself are not added again.
    file.add_master_if_missing("MasterA.esm", true, true).unwrap();
    assert_eq!(master_names(&file), ["MasterA.esm"]);
    file.add_masters_if_missing(&["MasterB.esm", "Plugin.esp"], true, true)
        .unwrap();
    // MasterB.esm loads first, so the sort puts it first: the FormIDs of
    // MasterA.esm move to index 1, those of the plugin to index 2.
    assert_eq!(master_names(&file), ["MasterB.esm", "MasterA.esm"]);
    let (form_id, entries) = list_form_ids(&file);
    assert_eq!(form_id, 0x0200_0803);
    assert_eq!(entries[0], Variant::UInt(0x0100_0801));
    let saved = file.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert_eq!(
        saved,
        plugin(
            &["MasterB.esm", "MasterA.esm"],
            0x0200_0803,
            &[0x0100_0801, 0x0200_0803]
        )
    );
    // MasterB.esm is not used: cleaning removes it and the FormIDs move
    // back, which gives the input again.
    file.clean_masters().unwrap();
    assert_eq!(master_names(&file), ["MasterA.esm"]);
    let saved = file.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    set_edit_allowed(false);
    assert_eq!(saved, original);
}

#[test]
fn sort_masters_follows_the_load_order() {
    let _guard = test_lock();
    let file = load(
        plugin(
            &["MasterA.esm", "MasterB.esm"],
            0x0200_0803,
            &[0x0000_0801, 0x0100_0802],
        ),
        true,
    );
    set_edit_allowed(true);
    file.sort_masters().unwrap();
    assert_eq!(master_names(&file), ["MasterB.esm", "MasterA.esm"]);
    let saved = file.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    set_edit_allowed(false);
    assert_eq!(
        saved,
        plugin(
            &["MasterB.esm", "MasterA.esm"],
            0x0200_0803,
            &[0x0100_0801, 0x0000_0802]
        )
    );
}

#[test]
fn clean_masters_keeps_used_masters() {
    let _guard = test_lock();
    let original = plugin(&["MasterA.esm", "MasterB.esm"], 0x0200_0803, &[0x0100_0802]);
    let file = load(original.clone(), false);
    set_edit_allowed(true);
    file.clean_masters().unwrap();
    assert_eq!(master_names(&file), ["MasterB.esm"]);
    let saved = file.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    assert_eq!(saved, plugin(&["MasterB.esm"], 0x0100_0803, &[0x0000_0802]));
    // Nothing more to remove.
    file.clean_masters().unwrap();
    assert_eq!(master_names(&file), ["MasterB.esm"]);
    set_edit_allowed(false);
}

#[test]
fn add_masters_refuses_files_that_are_not_loaded() {
    let _guard = test_lock();
    let file = load(plugin(&["MasterA.esm"], 0x0100_0803, &[]), false);
    set_edit_allowed(true);
    let error = file.add_masters_if_missing(&["Missing.esm"], true, true).unwrap_err();
    assert_eq!(
        error,
        "[AddMAddMastersIfMissingasters] Requested file to add is not loaded: \"Missing.esm\""
    );
    let error = file.add_masters(&["Missing.esm"], true).unwrap_err();
    assert_eq!(
        error,
        "[AddMasters] Requested file to add is not loaded: \"Missing.esm\""
    );
    // Names that are not modules are skipped.
    file.add_masters(&["readme.txt"], true).unwrap();
    assert_eq!(master_names(&file), ["MasterA.esm"]);
    set_edit_allowed(false);
    // Without the edit flag the file is not editable.
    let error = file.add_masters(&["MasterB.esm"], true).unwrap_err();
    assert_eq!(error, "File \"Plugin.esp\" is not editable");
}

#[test]
fn a_plugin_without_masters_gets_the_master_files() {
    let _guard = test_lock();
    let file = load(plugin(&[], 0x0000_0803, &[0x0000_0803]), false);
    set_edit_allowed(true);
    file.add_masters_if_missing(&["MasterA.esm"], true, true).unwrap();
    assert_eq!(master_names(&file), ["MasterA.esm"]);
    let saved = file.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    set_edit_allowed(false);
    assert_eq!(saved, plugin(&["MasterA.esm"], 0x0100_0803, &[0x0100_0803]));
}

#[test]
fn child_group_labels_follow_the_masters() {
    let _guard = test_lock();
    // A topic of the plugin with a response in its child group.
    let topic = 0x0100_0810u32;
    let mut bytes = header(0, &["MasterA.esm"], 4);
    let mut dial = main_record(b"DIAL", 0, topic, &sub_record(b"EDID", b"TestTopic\0"));
    dial.extend(group_of(
        topic.to_le_bytes(),
        7,
        &main_record(b"INFO", 0, 0x0100_0811, &[]),
    ));
    bytes.extend(group(b"DIAL", &dial));
    let file = load(bytes, false);
    set_edit_allowed(true);
    file.add_masters_if_missing(&["MasterB.esm"], true, true).unwrap();
    let saved = file.write_to_bytes(ResetModified::rmSetInternal).unwrap();
    set_edit_allowed(false);
    let find = |pattern: &[u8]| saved.windows(pattern.len()).position(|window| window == pattern);
    // The topic, its group label and the response moved to index 2.
    assert!(find(&0x0200_0810u32.to_le_bytes()).is_some());
    assert!(find(&0x0200_0811u32.to_le_bytes()).is_some());
    assert!(find(&topic.to_le_bytes()).is_none());
    let groups: Vec<(u32, u32)> = saved
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == b"GRUP")
        .map(|(offset, _)| {
            let at = |i: usize| u32::from_le_bytes(saved[offset + i..offset + i + 4].try_into().unwrap());
            (at(8), at(12))
        })
        .collect();
    assert!(groups.contains(&(0x0200_0810, 7)), "{groups:x?}");
}
#[test]
fn masters_commands_add_sort_and_clean() {
    let _guard = test_lock();
    let file = load(plugin(&["MasterA.esm"], 0x0100_0803, &[0x0000_0801]), true);
    let registry = Registry::standard();
    let mut session = Session::with_files(GameMode::gmSSE, vec![file.clone()]);
    // A mutating command needs the edit flag unless it is a dry run.
    let error = registry
        .call(&mut session, "masters.add", json!({ "masters": ["MasterB.esm"] }))
        .unwrap_err();
    assert_eq!(error.code, "edit_required");
    let dry = registry
        .call(
            &mut session,
            "masters.add",
            json!({ "masters": ["MasterB.esm"], "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["masters"], json!(["MasterB.esm", "MasterA.esm"]));
    assert_eq!(dry["changed"], true);
    assert_eq!(master_names(&file), ["MasterA.esm"]);
    session.allow_edit(true);
    let added = registry
        .call(
            &mut session,
            "masters.add",
            json!({ "masters": ["MasterB.esm"], "sort": false }),
        )
        .unwrap();
    assert_eq!(added["old_masters"], json!(["MasterA.esm"]));
    assert_eq!(added["masters"], json!(["MasterA.esm", "MasterB.esm"]));
    let sorted = registry.call(&mut session, "masters.sort", json!({})).unwrap();
    assert_eq!(sorted["masters"], json!(["MasterB.esm", "MasterA.esm"]));
    let dry = registry
        .call(&mut session, "masters.clean", json!({ "dry_run": true }))
        .unwrap();
    assert_eq!(dry["masters"], json!(["MasterA.esm"]));
    assert_eq!(master_names(&file), ["MasterB.esm", "MasterA.esm"]);
    let cleaned = registry.call(&mut session, "masters.clean", json!({})).unwrap();
    assert_eq!(cleaned["masters"], json!(["MasterA.esm"]));
    assert_eq!(cleaned["changed"], true);
    let error = registry
        .call(&mut session, "masters.add", json!({ "masters": ["Nope.esm"] }))
        .unwrap_err();
    assert_eq!(error.code, "edit_failed");
    session.allow_edit(false);
}
