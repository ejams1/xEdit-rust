// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The first batch of issue #78: the menu items of the main form that had no
//! typed command. The tests drive the commands over synthetic plugins and
//! check the effect of the release's handlers: `Element.Clear`,
//! `MarkModifiedRecursive`, `MarkHeaderModified`, `NextMember` and
//! `PreviousMember`, `PerformDrop`/`wbCopyElementToRecord`,
//! `Element.Remove` over a view row, and `CopyInto`'s AsWrapper, AsSpawnRate
//! and `AfterCopyDisable` branches.

use std::sync::Arc;

use serde_json::json;
use xedit_core::implementation::write::ElementState;
use xedit_core::implementation::{FileBytes, FileImpl, wb_file_from_bytes};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{Container, ElementRef, GameMode, MainRecord, Variant};
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
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
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
    data.extend(sub_record(b"INCC", &0u32.to_le_bytes()));
    main_record(b"TES4", flags, 0, &data)
}

fn gmst(form_id: u32, editor_id: &[u8], value: f32) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"DATA", &value.to_le_bytes()));
    main_record(b"GMST", 0, form_id, &data)
}

/// A leveled item whose entries are (level, count, reference) triples.
fn leveled_item(form_id: u32, editor_id: &[u8], entries: &[(u16, u16, u32)]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"OBND", &[0; 12]));
    data.extend(sub_record(b"LVLD", &[0]));
    data.extend(sub_record(b"LVLF", &[1]));
    data.extend(sub_record(b"LLCT", &[entries.len() as u8]));
    for (level, count, reference) in entries {
        let mut entry = Vec::new();
        entry.extend_from_slice(&level.to_le_bytes());
        entry.extend_from_slice(&[0, 0]);
        entry.extend_from_slice(&reference.to_le_bytes());
        entry.extend_from_slice(&count.to_le_bytes());
        entry.extend_from_slice(&[0, 0]);
        data.extend(sub_record(b"LVLO", &entry));
    }
    main_record(b"LVLI", 0, form_id, &data)
}

fn load_game(game: &str, files: Vec<(&str, Vec<u8>)>) -> Vec<Arc<FileImpl>> {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    let mode = xedit_session::dump::setup_game(game).unwrap();
    xedit_session::save::apply_edit_settings(mode);
    let mut result = Vec::new();
    for (order, (name, bytes)) in files.into_iter().enumerate() {
        result.push(wb_file_from_bytes(name, order as i32, FileStates::empty(), FileBytes::Owned(bytes)).unwrap());
    }
    result
}

fn session(files: &[Arc<FileImpl>]) -> (Registry, Session) {
    let mut session = Session::with_files(GameMode::gmSSE, files.to_vec());
    session.allow_edit(true);
    (Registry::standard(), session)
}

fn call(registry: &Registry, session: &mut Session, name: &str, params: serde_json::Value) -> serde_json::Value {
    registry
        .call(session, name, params)
        .unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn modified(element: &ElementRef) -> bool {
    element
        .as_element_impl()
        .expect("an element")
        .element_base()
        .has_state(ElementState::esModified)
}

/// Skyrim: the master with a game setting, and a plugin overriding it.
fn gmst_files() -> Vec<Arc<FileImpl>> {
    let mut master = header(1, 1, 0x801, &[]);
    master.extend(top_group(b"GMST", &gmst(0x800, b"fMaster\0", 1.5)));
    let plugin = {
        let mut bytes = header(0, 1, 0x801, &["Skyrim.esm"]);
        bytes.extend(top_group(b"GMST", &gmst(0x800, b"fPlugin\0", 2.5)));
        bytes
    };
    load_game("sse", vec![("Skyrim.esm", master), ("Plugin.esp", plugin)])
}

#[test]
fn marks_an_element_modified() {
    let _guard = test_lock();
    let files = gmst_files();
    let (registry, mut session) = session(&files);
    let record = files[1].records()[0].clone();
    assert!(!modified(&(record.clone() as ElementRef)));
    let result = call(
        &registry,
        &mut session,
        "elements.mark_modified",
        json!({ "form_id": "00000800", "file": "Plugin.esp" }),
    );
    assert_eq!(result["modified"], true);
    let record_ref: ElementRef = record.clone();
    assert!(modified(&record_ref));
    let data = record.get_element_by_path("DATA").expect("DATA");
    assert!(modified(&data));
}

#[test]
fn marks_the_headers_of_files_without_onam() {
    let _guard = test_lock();
    let files = gmst_files();
    let (registry, mut session) = session(&files);
    let dry = call(
        &registry,
        &mut session,
        "files.mark_modified_headers",
        json!({ "dry_run": true }),
    );
    assert_eq!(dry["files"], json!(["Plugin.esp"]));
    assert_eq!(dry["modified"], false);
    let header = files[1].header().expect("the header");
    assert!(!modified(&(header.clone() as ElementRef)));
    call(&registry, &mut session, "files.mark_modified_headers", json!({}));
    assert!(modified(&(header as ElementRef)));
}

#[test]
fn a_cancelled_run_leaves_the_headers_as_they_were() {
    let _guard = test_lock();
    let files = gmst_files();
    let (registry, mut session) = session(&files);
    // The user cancelled: the headers are not marked.
    xedit_core::interface::globals::set_force_terminate(true);
    let error = registry
        .call(&mut session, "files.mark_modified_headers", json!({}))
        .unwrap_err();
    xedit_core::interface::globals::set_force_terminate(false);
    assert_eq!(error.code, "cancelled");
    let header = files[1].header().expect("the header");
    assert!(!modified(&(header.clone() as ElementRef)));
}

/// Skyrim: a cell of the plugin with the water current velocities, whose
/// "Velocity Count" is a union member of a subrecord structure.
fn cell_files() -> Vec<Arc<FileImpl>> {
    let master = header(1, 0, 0x800, &[]);
    let mut data = sub_record(b"EDID", b"TestCell\0");
    data.extend(sub_record(b"DATA", &1u16.to_le_bytes()));
    // `XWCN` (the union member of "Water Current Velocities") and one
    // `XWCU` entry of a vector and a float.
    data.extend(sub_record(b"XWCN", &1u32.to_le_bytes()));
    data.extend(sub_record(b"XWCU", &[0u8; 16]));
    let cell = main_record(b"CELL", 0, 0x0100_0800, &data);
    let mut plugin = header(0, 1, 0x801, &["Skyrim.esm"]);
    plugin.extend(top_group(b"CELL", &cell));
    load_game("sse", vec![("Skyrim.esm", master), ("Plugin.esp", plugin)])
}

#[test]
fn switches_the_member_of_a_union() {
    let _guard = test_lock();
    let files = cell_files();
    let (registry, mut session) = session(&files);
    let record = files[1].records()[0].clone();
    let structure = record
        .get_element_by_path("Water Current Velocities")
        .expect("the water current structure");
    let member = structure
        .as_container()
        .expect("the structure is a container")
        .get_element(0)
        .expect("the union member");
    let before = member.get_name();
    assert!(before.contains("XWCN"), "{before}");
    let dry = call(
        &registry,
        &mut session,
        "elements.change_member",
        json!({ "form_id": "01000800", "file": "Plugin.esp", "path": "Water Current Velocities\\[0]",
                "dry_run": true }),
    );
    assert_eq!(dry["can_change"], true);
    assert_eq!(dry["changed"], false);
    let next = call(
        &registry,
        &mut session,
        "elements.change_member",
        json!({ "form_id": "01000800", "file": "Plugin.esp", "path": "Water Current Velocities\\[0]",
                "direction": "next" }),
    );
    assert_eq!(next["changed"], true);
    let switched = structure
        .as_container()
        .expect("the structure is a container")
        .get_element(0)
        .expect("the union member");
    assert!(switched.get_name().contains("XWCS"), "{}", switched.get_name());
    let previous = call(
        &registry,
        &mut session,
        "elements.change_member",
        json!({ "form_id": "01000800", "file": "Plugin.esp", "path": "Water Current Velocities\\[0]",
                "direction": "previous" }),
    );
    assert_eq!(previous["changed"], true);
    let back = structure
        .as_container()
        .expect("the structure is a container")
        .get_element(0)
        .expect("the union member");
    assert!(back.get_name().contains("XWCN"), "{}", back.get_name());
}

/// Fallout 4: a cell with a combined reference index, whose "Meshes" is a
/// value array of two combined mesh IDs.
fn xcri_files() -> Vec<Arc<FileImpl>> {
    let master = header(1, 0, 0x800, &[]);
    let mut data = sub_record(b"EDID", b"TestCell\0");
    let mut xcri = Vec::new();
    xcri.extend_from_slice(&2u32.to_le_bytes());
    xcri.extend_from_slice(&0u32.to_le_bytes());
    xcri.extend_from_slice(&0x0100_0A01u32.to_le_bytes());
    xcri.extend_from_slice(&0x0100_0A02u32.to_le_bytes());
    data.extend(sub_record(b"XCRI", &xcri));
    let cell = main_record(b"CELL", 0, 0x0100_0800, &data);
    let mut plugin = header(0, 1, 0x801, &["Fallout4.esm"]);
    plugin.extend(top_group(b"CELL", &cell));
    load_game("fo4", vec![("Fallout4.esm", master), ("Plugin.esp", plugin)])
}

#[test]
fn clears_a_value_array() {
    let _guard = test_lock();
    let files = xcri_files();
    let (registry, mut session) = session(&files);
    let record = files[1].records()[0].clone();
    let meshes = record
        .get_element_by_path("XCRI\\Meshes")
        .expect("the combined mesh array");
    let container = meshes.as_container().expect("the array is a container");
    assert_eq!(container.get_element_count(), 2);
    let dry = call(
        &registry,
        &mut session,
        "elements.clear",
        json!({ "form_id": "01000800", "file": "Plugin.esp", "path": "XCRI\\Meshes",
                "dry_run": true }),
    );
    assert_eq!(dry["clearable"], true);
    assert_eq!(dry["cleared"], false);
    assert_eq!(container.get_element_count(), 2);
    let cleared = call(
        &registry,
        &mut session,
        "elements.clear",
        json!({ "form_id": "01000800", "file": "Plugin.esp", "path": "XCRI\\Meshes" }),
    );
    assert_eq!(cleared["cleared"], true);
    assert_eq!(container.get_element_count(), 0);
    // The menu item is hidden for an element that is not clearable.
    let error = registry
        .call(
            &mut session,
            "elements.clear",
            json!({ "form_id": "01000800", "file": "Plugin.esp", "path": "EDID" }),
        )
        .expect_err("EDID is not clearable");
    assert_eq!(error.code, "not_clearable");
}

/// Skyrim: the master's leveled item and a plugin with the same record at
/// the same load order FormID.
fn leveled_files() -> Vec<Arc<FileImpl>> {
    let mut master = header(1, 1, 0x801, &[]);
    master.extend(top_group(
        b"LVLI",
        &leveled_item(0x800, b"TestList\0", &[(1, 1, 0xA01), (2, 1, 0xA02), (3, 2, 0xA03)]),
    ));
    let mut plugin = header(0, 1, 0x801, &["Skyrim.esm"]);
    plugin.extend(top_group(
        b"LVLI",
        &leveled_item(0x800, b"TestList\0", &[(1, 1, 0xA01)]),
    ));
    load_game("sse", vec![("Skyrim.esm", master), ("Plugin.esp", plugin)])
}

fn entries_of(record: &xedit_core::implementation::MainRecordImpl) -> Vec<(u32, u32)> {
    let entries = record.get_element_by_name("Leveled List Entries").expect("entries");
    let container = entries.as_container().expect("a container");
    (0..container.get_element_count())
        .map(|index| {
            let entry = container.get_element(index).expect("an entry");
            let lvlo = entry
                .as_container()
                .expect("an entry is a container")
                .get_element(0)
                .expect("the LVLO member");
            let lvlo = lvlo.as_container().expect("the LVLO structure");
            let reference = match lvlo.get_element(2).expect("the reference").get_native_value() {
                Variant::UInt(value) => value as u32,
                Variant::Int(value) => value as u32,
                other => panic!("{other:?}"),
            };
            let count = match lvlo.get_element_by_name("Count").expect("Count").get_native_value() {
                Variant::UInt(value) => value as u32,
                Variant::Int(value) => value as u32,
                other => panic!("{other:?}"),
            };
            (reference, count)
        })
        .collect()
}

#[test]
fn copies_a_leveled_list_as_a_wrapper() {
    let _guard = test_lock();
    let files = leveled_files();
    let (registry, mut session) = session(&files);
    let result = call(
        &registry,
        &mut session,
        "records.copy",
        json!({ "form_id": "00000800", "from": "Skyrim.esm", "to": "Plugin.esp",
                "as_wrapper": true, "wrapper_editor_id": "TestWrapper" }),
    );
    assert_eq!(result["wrapper"]["editor_id"], "TestWrapper");
    let override_record = files[1]
        .contained_record_by_load_order_form_id(xedit_core::interface::FormID::from_cardinal(0x800))
        .expect("the override");
    assert_eq!(override_record.get_editor_id(), "TestList");
    let entries = entries_of(&override_record);
    assert_eq!(entries.len(), 1);
    let wrapper = files[1]
        .records()
        .into_iter()
        .find(|record| record.get_editor_id() == "TestWrapper")
        .expect("the wrapper record");
    assert_eq!(entries[0].0, wrapper.get_form_id().to_cardinal());
    assert_eq!(entries[0].1, 1);
    let level = {
        let entries = override_record
            .get_element_by_name("Leveled List Entries")
            .expect("entries");
        let container = entries.as_container().expect("a container");
        let lvlo = container
            .get_element(0)
            .expect("an entry")
            .as_container()
            .expect("an entry")
            .get_element(0)
            .expect("the LVLO member");
        match lvlo
            .as_container()
            .expect("the LVLO structure")
            .get_element_by_name("Level")
            .expect("Level")
            .get_native_value()
        {
            Variant::UInt(value) => value as u32,
            Variant::Int(value) => value as u32,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(level, 1);
}

#[test]
fn copies_a_leveled_list_as_a_spawn_rate_override() {
    let _guard = test_lock();
    let files = leveled_files();
    let (registry, mut session) = session(&files);
    call(
        &registry,
        &mut session,
        "records.copy",
        json!({ "form_id": "00000800", "from": "Skyrim.esm", "to": "Plugin.esp",
                "as_spawn_rate": true, "overwrite": false }),
    );
    let copy = files[1]
        .contained_record_by_load_order_form_id(xedit_core::interface::FormID::from_cardinal(0x800))
        .expect("the copy");
    // UPSTREAM-QUIRK: `AfterCopyAdjustSpawnRate` takes the entries of
    // "Leveled List Entries" whose name is 'LVLO - Leveled List Entry', but
    // the elements of the array are `TwbSubRecordStruct`s named "Leveled
    // List Entry", so no entry matches and the release's item copies the
    // record as a plain override. The port follows it; the entry count and
    // count values of the copy are the source's.
    let entries = entries_of(&copy);
    assert_eq!(entries, vec![(0xA01, 1)]);
}

#[test]
fn copies_as_a_disabled_override() {
    let _guard = test_lock();
    let files = leveled_files();
    let (registry, mut session) = session(&files);
    let response = call(
        &registry,
        &mut session,
        "records.copy",
        json!({ "form_id": "00000800", "from": "Skyrim.esm", "to": "Plugin.esp",
                "disable": true, "overwrite": true }),
    );
    assert_eq!(response["copy"]["file"], "Plugin.esp");
    let copy = files[1]
        .contained_record_by_load_order_form_id(xedit_core::interface::FormID::from_cardinal(0x800))
        .expect("the copy");
    assert!(copy.get_is_initially_disabled());
}

/// Skyrim: the master's game setting and two plugins overriding it.
fn override_files() -> Vec<Arc<FileImpl>> {
    let mut master = header(1, 1, 0x801, &[]);
    master.extend(top_group(b"GMST", &gmst(0x800, b"fMaster\0", 1.5)));
    let first = {
        let mut bytes = header(0, 1, 0x801, &["Skyrim.esm"]);
        bytes.extend(top_group(b"GMST", &gmst(0x800, b"fFirst\0", 2.5)));
        bytes
    };
    let second = {
        let mut bytes = header(0, 1, 0x801, &["Skyrim.esm"]);
        bytes.extend(top_group(b"GMST", &gmst(0x800, b"fSecond\0", 3.5)));
        bytes
    };
    load_game(
        "sse",
        vec![("Skyrim.esm", master), ("First.esp", first), ("Second.esp", second)],
    )
}

fn record_value(record: &xedit_core::implementation::MainRecordImpl) -> f64 {
    let data = record.get_element_by_path("DATA").expect("DATA");
    let value = data
        .as_container()
        .expect("the DATA subrecord")
        .get_element(0)
        .expect("the value of DATA");
    match value.get_native_value() {
        Variant::Float(value) => value,
        other => panic!("{other:?}"),
    }
}

#[test]
fn copies_an_element_to_the_other_view_columns() {
    let _guard = test_lock();
    let files = override_files();
    let (registry, mut session) = session(&files);
    let first = files[1].records()[0].clone();
    let result = call(
        &registry,
        &mut session,
        "elements.copy_to_records",
        json!({ "form_id": "00000800", "file": "Second.esp", "path": "DATA", "targets": ["First.esp"] }),
    );
    assert_eq!(result["targets"][0]["action"], "assigned");
    assert!((record_value(&first) - 3.5).abs() < 1e-4);
    let copy = call(
        &registry,
        &mut session,
        "elements.copy_to_records",
        json!({ "form_id": "00000800", "file": "Second.esp", "path": "EDID", "targets": ["First.esp"],
                "mode": "copy" }),
    );
    assert_eq!(copy["targets"][0]["action"], "copied");
    assert_eq!(first.get_editor_id(), "fSecond");
}

/// Skyrim: the master's form ID list and two plugins overriding it, each
/// with two entries.
fn flst_files() -> Vec<Arc<FileImpl>> {
    let flst = |form_id: u32, editor_id: &[u8], entries: &[u32]| {
        let mut data = sub_record(b"EDID", editor_id);
        for entry in entries {
            data.extend(sub_record(b"LNAM", &entry.to_le_bytes()));
        }
        main_record(b"FLST", 0, form_id, &data)
    };
    let mut master = header(1, 1, 0x811, &[]);
    master.extend(top_group(b"FLST", &flst(0x810, b"MasterList\0", &[0x800, 0x801])));
    let first = {
        let mut bytes = header(0, 1, 0x811, &["Skyrim.esm"]);
        bytes.extend(top_group(b"FLST", &flst(0x810, b"FirstList\0", &[0x800, 0x801])));
        bytes
    };
    let second = {
        let mut bytes = header(0, 1, 0x811, &["Skyrim.esm"]);
        bytes.extend(top_group(b"FLST", &flst(0x810, b"SecondList\0", &[0x800, 0x802])));
        bytes
    };
    load_game(
        "sse",
        vec![("Skyrim.esm", master), ("First.esp", first), ("Second.esp", second)],
    )
}

#[test]
fn removes_an_element_from_the_other_view_columns() {
    let _guard = test_lock();
    let files = flst_files();
    let (registry, mut session) = session(&files);
    let first = files[1].records()[0].clone();
    let entries = first.get_element_by_path("FormIDs").expect("FormIDs");
    assert_eq!(entries.as_container().expect("a container").get_element_count(), 2);
    let result = call(
        &registry,
        &mut session,
        "elements.remove_from_records",
        json!({ "form_id": "00000810", "file": "Second.esp", "path": "FormIDs\\[1]",
                "targets": ["First.esp"] }),
    );
    assert_eq!(result["targets"][0]["action"], "removed");
    let entries = first.get_element_by_path("FormIDs").expect("FormIDs");
    assert_eq!(entries.as_container().expect("a container").get_element_count(), 1);
}
