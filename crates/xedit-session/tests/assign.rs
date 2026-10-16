// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The assign, move and copy-over commands of the parity batch on synthetic
//! Skyrim SE plugins: an entry of an unsorted array moved, removed and put
//! back, assigned from another record and made from a template, a copy over
//! a record the target has already, a deleted source that removes the
//! target's record, and the module templates of "Create New File".

use std::sync::Arc;

use serde_json::json;
use xedit_core::implementation::{FileBytes, FileImpl, ResetModified, wb_file_from_bytes};
use xedit_core::interface::GameMode;
use xedit_core::interface::globals::{set_data_path, test_lock};
use xedit_core::interface::types::FileStates;
use xedit_io::temp::TempDir;
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

/// A deleted game setting (`frDeleted` of the record header flags).
fn deleted_gmst(form_id: u32, editor_id: &[u8], value: f32) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"DATA", &value.to_le_bytes()));
    main_record(b"GMST", 0x20, form_id, &data)
}

/// A form ID list with one `LNAM` entry per FormID.
fn flst(form_id: u32, editor_id: &[u8], entries: &[u32]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    for entry in entries {
        data.extend(sub_record(b"LNAM", &entry.to_le_bytes()));
    }
    main_record(b"FLST", 0, form_id, &data)
}

/// The game master (its name is the one `files.new` adds): two game
/// settings and a form ID list of two entries.
fn master_bytes() -> Vec<u8> {
    let mut bytes = header(1, 3, 0x820, &[]);
    let mut gmsts = gmst(0x800, b"fMaster\0", 1.5);
    gmsts.extend(gmst(0x801, b"fOther\0", 2.5));
    bytes.extend(top_group(b"GMST", &gmsts));
    bytes.extend(top_group(b"FLST", &flst(0x810, b"MasterList\0", &[0x800, 0x801])));
    bytes
}

/// A plugin of the master: an override's source, a form ID list of its own
/// and a game setting it flags as deleted.
fn plugin_bytes() -> Vec<u8> {
    let mut bytes = header(0, 3, 0x820, &["Skyrim.esm"]);
    let mut gmsts = gmst(0x0100_0800, b"fPlugin\0", 2.5);
    gmsts.extend(deleted_gmst(0x0100_0801, b"fDeleted\0", 3.5));
    bytes.extend(top_group(b"GMST", &gmsts));
    bytes.extend(top_group(
        b"FLST",
        &flst(0x0100_0810, b"PluginList\0", &[0x800, 0x801, 0x0100_0800]),
    ));
    bytes
}

struct Files {
    master: Arc<FileImpl>,
    plugin: Arc<FileImpl>,
}

fn load() -> Files {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let open = |name: &str, bytes: Vec<u8>| {
        wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap()
    };
    let master = open("Skyrim.esm", master_bytes());
    xedit_core::interface::add_file(master.clone());
    let plugin = open("Plugin.esp", plugin_bytes());
    Files { master, plugin }
}

fn session(files: &Files) -> (Registry, Session) {
    let mut session = Session::with_files(GameMode::gmSSE, vec![files.master.clone(), files.plugin.clone()]);
    session.allow_edit(true);
    (Registry::standard(), session)
}

/// The `LNAM` values of the record in a saved file, in file order: `None`
/// for an entry the save wrote without the four bytes of a form ID.
fn lnam_entries(bytes: &[u8], form_id: u32) -> Vec<Option<u32>> {
    let mut offset = 0;
    while offset + 24 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &bytes[offset..offset + 4] == b"GRUP" {
            offset += 24;
            continue;
        }
        let id = u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
        if &bytes[offset..offset + 4] == b"FLST" && id == form_id {
            let mut values = Vec::new();
            let mut at = offset + 24;
            let end = offset + 24 + size;
            while at + 6 <= end {
                let length = u16::from_le_bytes(bytes[at + 4..at + 6].try_into().unwrap()) as usize;
                if &bytes[at..at + 4] == b"LNAM" {
                    values.push((length == 4).then(|| u32::from_le_bytes(bytes[at + 6..at + 10].try_into().unwrap())));
                }
                at += 6 + length;
            }
            return values;
        }
        offset += 24 + size;
    }
    panic!("the saved file has no FLST [{form_id:08X}]");
}

fn saved_plugin(files: &Files) -> Vec<u8> {
    files.plugin.write_to_bytes(ResetModified::rmSetInternal).unwrap()
}

#[test]
fn moves_entries_of_an_unsorted_array() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    // The second entry moves down: [0800, 01000800, 0801] in the file.
    let moved = registry
        .call(
            &mut session,
            "elements.move",
            json!({ "form_id": "01000810", "file": "Plugin.esp", "path": "FormIDs\\[1]", "direction": "down" }),
        )
        .unwrap();
    assert_eq!(moved["can_move_down"], true);
    assert_eq!(moved["can_move_up"], true);
    assert_eq!(moved["moved"], true);
    let saved = saved_plugin(&files);
    assert_eq!(
        lnam_entries(&saved, 0x0100_0810),
        vec![Some(0x800), Some(0x0100_0800), Some(0x801)]
    );
    // The last entry moves up again.
    registry
        .call(
            &mut session,
            "elements.move",
            json!({ "form_id": "01000810", "file": "Plugin.esp", "path": "FormIDs\\[2]", "direction": "up" }),
        )
        .unwrap();
    let saved = saved_plugin(&files);
    assert_eq!(
        lnam_entries(&saved, 0x0100_0810),
        vec![Some(0x800), Some(0x801), Some(0x0100_0800)]
    );
    // The first entry is at the start of the array and can not move up.
    let top = registry
        .call(
            &mut session,
            "elements.move",
            json!({ "form_id": "01000810", "file": "Plugin.esp", "path": "FormIDs\\[0]", "direction": "up" }),
        )
        .unwrap();
    assert_eq!(top["can_move_up"], false);
    assert_eq!(top["moved"], false);
}

#[test]
fn inserts_an_entry_removed_first_at_a_position() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    // `InsertElement` after `RemoveElement`: the first entry goes in at
    // position 2, where the subrecord array keeps its data.
    let moved = registry
        .call(
            &mut session,
            "elements.move",
            json!({ "form_id": "01000810", "file": "Plugin.esp", "path": "FormIDs\\[0]", "position": 2 }),
        )
        .unwrap();
    assert_eq!(moved["moved"], true);
    let saved = saved_plugin(&files);
    assert_eq!(
        lnam_entries(&saved, 0x0100_0810),
        vec![Some(0x801), Some(0x0100_0800), Some(0x800)]
    );
}

#[test]
fn assigns_an_entry_of_another_record() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    // The master's first list entry is appended to the plugin's list.
    let dry = registry
        .call(
            &mut session,
            "elements.assign",
            json!({ "form_id": "01000810", "file": "Plugin.esp", "path": "FormIDs", "index": "add",
                    "source": { "form_id": "00000810", "file": "Skyrim.esm", "path": "FormIDs\\[0]" },
                    "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["can_assign"], true);
    assert_eq!(dry["element"], json!(null));
    let assigned = registry
        .call(
            &mut session,
            "elements.assign",
            json!({ "form_id": "01000810", "file": "Plugin.esp", "path": "FormIDs", "index": "add",
                    "source": { "form_id": "00000810", "file": "Skyrim.esm", "path": "FormIDs\\[0]" } }),
        )
        .unwrap();
    assert_eq!(assigned["can_assign"], true);
    assert_eq!(assigned["file"], "Plugin.esp");
    let saved = saved_plugin(&files);
    assert_eq!(
        lnam_entries(&saved, 0x0100_0810),
        vec![Some(0x800), Some(0x801), Some(0x0100_0800), Some(0x800)]
    );
}

#[test]
fn lists_the_templates_and_assigns_one() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    // The templates of the array for an added entry: the element of the array.
    let templates = registry
        .call(
            &mut session,
            "elements.templates",
            json!({ "form_id": "01000810", "file": "Plugin.esp", "path": "FormIDs", "index": "add" }),
        )
        .unwrap();
    assert_eq!(templates["templates"], json!(["FormID"]));
    // The template makes an entry of its definition, without data: an empty
    // form ID, which the save writes as four zero bytes.
    let assigned = registry
        .call(
            &mut session,
            "elements.assign",
            json!({ "form_id": "01000810", "file": "Plugin.esp", "path": "FormIDs", "index": "add",
                    "template": "FormID" }),
        )
        .unwrap();
    assert_eq!(assigned["can_assign"], true);
    let saved = saved_plugin(&files);
    // The template made the fourth entry; its data is the definition's, not
    // a copied form ID, so only the first three are asserted.
    let entries = lnam_entries(&saved, 0x0100_0810);
    assert_eq!(entries.len(), 4);
    assert_eq!(&entries[..3], &[Some(0x800), Some(0x801), Some(0x0100_0800)]);
    // A template the element does not offer is refused by name.
    let error = registry
        .call(
            &mut session,
            "elements.assign",
            json!({ "form_id": "01000810", "file": "Plugin.esp", "path": "FormIDs", "index": "add",
                    "template": "Nope" }),
        )
        .unwrap_err();
    assert_eq!(error.code, "unknown_template");
}

#[test]
fn copies_over_a_record_the_target_has() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    // The copy makes the override of the master's record in the plugin.
    let copied = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00000800", "from": "Skyrim.esm", "to": "Plugin.esp" }),
        )
        .unwrap();
    assert_eq!(copied["existed"], false);
    assert_eq!(copied["copy"]["form_id"], "00000800");
    // The override is edited, then copied over from the master: without
    // `overwrite` the existing override is returned unchanged.
    registry
        .call(
            &mut session,
            "elements.set",
            json!({ "form_id": "00000800", "file": "Plugin.esp", "path": "DATA\\Float", "value": "9.0" }),
        )
        .unwrap();
    let again = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00000800", "from": "Skyrim.esm", "to": "Plugin.esp", "dry_run": true }),
        )
        .unwrap();
    assert_eq!(again["existed"], true);
    assert_eq!(again["overwrites"], json!([]));
    let over = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00000800", "from": "Skyrim.esm", "to": "Plugin.esp",
                    "deep": true, "overwrite": true }),
        )
        .unwrap();
    assert_eq!(over["existed"], true);
    assert_eq!(over["overwrites"].as_array().unwrap().len(), 1);
    let line = over["overwrites"][0].as_str().unwrap();
    assert!(line.starts_with("Overwriting \""), "{line}");
    let get = |session: &mut Session, file: &str| {
        registry
            .call(
                session,
                "elements.get",
                json!({ "form_id": "00000800", "file": file, "path": "DATA\\Float" }),
            )
            .unwrap()["value"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let source_value = get(&mut session, "Skyrim.esm");
    assert_eq!(
        get(&mut session, "Plugin.esp"),
        source_value,
        "the master's value is back"
    );
}

#[test]
fn removes_the_target_of_a_deleted_source() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    // The plugin's own record is flagged as deleted; a copy of it over its
    // own version with `remove_deleted` removes it (the deleted record of
    // the source wins the question, as the GUI answers it).
    let over = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "01000801", "from": "Plugin.esp", "to": "Plugin.esp",
                    "deep": true, "overwrite": true, "remove_deleted": true }),
        )
        .unwrap();
    assert_eq!(over["existed"], true);
    assert_eq!(over["copy"], json!(null));
    let lines: Vec<&str> = over["overwrites"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|line| line.as_str())
        .collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("Removing \""), "{}", lines[0]);
    // The saved plugin has no record with the FormID of the removed one.
    let saved = saved_plugin(&files);
    let mut offset = 0;
    let mut found = false;
    while offset + 24 <= saved.len() {
        let size = u32::from_le_bytes(saved[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &saved[offset..offset + 4] == b"GRUP" {
            offset += 24;
            continue;
        }
        let id = u32::from_le_bytes(saved[offset + 12..offset + 16].try_into().unwrap());
        if &saved[offset..offset + 4] == b"GMST" && id == 0x0100_0801 {
            found = true;
        }
        offset += 24 + size;
    }
    assert!(!found, "the removed record is still saved");
}

#[test]
fn makes_a_new_file_from_a_module_template() {
    let _guard = test_lock();
    let files = load();
    let (registry, mut session) = session(&files);
    let dir = TempDir::new("xedit-rust-assign-test");
    set_data_path(&format!("{}\\", dir.display()));
    // A dry run reports the name and the extension of the template.
    let dry = registry
        .call(
            &mut session,
            "files.new",
            json!({ "file": "Templated", "template": "esm-light", "dry_run": true }),
        )
        .unwrap();
    assert_eq!(dry["file"], "Templated.esm");
    assert_eq!(dry["dry_run"], true);
    // The real call makes it in the data folder with the header of the
    // template (the ESM and light flags) and the game master as its master;
    // like the GUI's, the file reaches the disk with its first save.
    let made = registry
        .call(
            &mut session,
            "files.new",
            json!({ "file": "Templated", "template": "esm-light" }),
        )
        .unwrap();
    assert_eq!(made["file"], "Templated.esm");
    assert_eq!(made["masters"], json!(["Skyrim.esm"]));
    let output = format!("{}\\Templated.esm", dir.display());
    let saved = registry
        .call(
            &mut session,
            "files.save",
            json!({ "file": "Templated.esm", "output": output, "backup": false }),
        )
        .unwrap();
    assert_eq!(saved["written"], true);
    let bytes = std::fs::read(dir.join("Templated.esm")).unwrap();
    assert_eq!(&bytes[..4], b"TES4");
    let flags = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert_eq!(flags & 0x1, 0x1, "the ESM flag of the template");
    assert_eq!(flags & 0x200, 0x200, "the light flag of the template");
    // A template the game does not offer is refused, and so is a template
    // together with the flags it stands for.
    let error = registry
        .call(
            &mut session,
            "files.new",
            json!({ "file": "Medium", "template": "esp-medium" }),
        )
        .unwrap_err();
    assert_eq!(error.code, "invalid_params");
    let error = registry
        .call(
            &mut session,
            "files.new",
            json!({ "file": "Both", "template": "esp", "light": true }),
        )
        .unwrap_err();
    assert_eq!(error.code, "invalid_params");
}
