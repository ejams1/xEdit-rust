// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The string tables of a synthetic localized Skyrim SE plugin: a string
//! changed through its element and saved with the plugin, the plugin
//! delocalized and localized again, the localization editor's commands, and
//! the translate mode's refusals.

use std::path::{Path, PathBuf};
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

fn header(flags: u32, record_count: u32) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&record_count.to_le_bytes());
    hedr.extend_from_slice(&0x802u32.to_le_bytes());
    let mut data = sub_record(b"HEDR", &hedr);
    data.extend(sub_record(b"CNAM", b"xedit-rust\0"));
    data.extend(sub_record(b"INCC", &0u32.to_le_bytes()));
    main_record(b"TES4", flags, 0, &data)
}

/// A string game setting: its value is a localized string.
fn gmst(form_id: u32, editor_id: &[u8], value: &[u8]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"DATA", value));
    main_record(b"GMST", 0, form_id, &data)
}

/// `Loc.esm`, localized (ESM and localized flags) or not, with two string
/// game settings: the IDs or the texts.
fn plugin(localized: bool, first: &[u8], second: &[u8]) -> Vec<u8> {
    let mut bytes = header(if localized { 0x81 } else { 0x01 }, 3);
    let mut records = gmst(0x800, b"sFirst\0", first);
    records.extend(gmst(0x801, b"sSecond\0", second));
    bytes.extend(top_group(b"GMST", &records));
    bytes
}

/// A `.STRINGS` table (zero terminated) or a `.DLSTRINGS`/`.ILSTRINGS` one
/// (length prefixed).
fn table(entries: &[(u32, &str)], length_prefixed: bool) -> Vec<u8> {
    let mut directory = Vec::new();
    let mut data = Vec::new();
    for (id, text) in entries {
        directory.extend_from_slice(&id.to_le_bytes());
        directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
        if length_prefixed {
            data.extend_from_slice(&(text.len() as u32 + 1).to_le_bytes());
        }
        data.extend_from_slice(text.as_bytes());
        data.push(0);
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend(directory);
    bytes.extend(data);
    bytes
}

/// A data folder of its own with the tables of `Loc.esm`, set up as the
/// session's containers, and the plugin loaded from `bytes`.
fn load(name: &str, bytes: Vec<u8>, tables: &[(&str, Vec<u8>)]) -> (PathBuf, Arc<FileImpl>) {
    let dir = std::env::temp_dir().join(format!("xedit-localization-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Strings")).unwrap();
    for (table, data) in tables {
        std::fs::write(dir.join("Strings").join(table), data).unwrap();
    }
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    xedit_core::interface::globals::set_language("English");
    xedit_core::interface::globals::set_data_path(&format!("{}\\", dir.display()));
    xedit_core::container_handler::clear_containers();
    xedit_core::container_handler::add_folder(&dir);
    xedit_core::localization::install_localization_handler();
    // A first file takes load order 0, which xEdit does not (de)localize.
    let base = wb_file_from_bytes(
        "Base.esm",
        i32::MAX,
        FileStates::empty(),
        FileBytes::Owned(header(1, 0)),
    )
    .unwrap();
    xedit_core::interface::add_file(base);
    let file = wb_file_from_bytes("Loc.esm", i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap();
    xedit_core::interface::add_file(file.clone());
    (dir, file)
}

fn session(file: &Arc<FileImpl>) -> (Registry, Session) {
    let mut session = Session::with_files(GameMode::gmSSE, vec![file.clone()]);
    session.allow_edit(true);
    (Registry::standard(), session)
}

fn call(registry: &Registry, session: &mut Session, name: &str, params: Value) -> Value {
    registry
        .call(session, name, params)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn save(registry: &Registry, session: &mut Session, dir: &Path) -> (Vec<u8>, Value) {
    let output = dir.join("out").join("Loc.esm");
    let result = call(
        registry,
        session,
        "files.save",
        json!({ "file": "Loc.esm", "output": output, "backup": false }),
    );
    (std::fs::read(&output).unwrap(), result)
}

fn loaded_tables() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("Loc_English.STRINGS", table(&[(1, "First"), (2, "Second")], false)),
        ("Loc_English.DLSTRINGS", table(&[], true)),
        ("Loc_English.ILSTRINGS", table(&[], true)),
    ]
}

#[test]
fn a_string_set_through_its_element_is_saved_in_its_table() {
    let _guard = test_lock();
    let (dir, file) = load(
        "set",
        plugin(true, &1u32.to_le_bytes(), &2u32.to_le_bytes()),
        &loaded_tables(),
    );
    let (registry, mut session) = session(&file);
    let value = call(
        &registry,
        &mut session,
        "elements.get",
        json!({ "form_id": "01000800", "path": "DATA\\Name" }),
    );
    assert_eq!(value["value"], "First");
    call(
        &registry,
        &mut session,
        "elements.set",
        json!({ "form_id": "01000800", "path": "DATA\\Name", "value": "Changed" }),
    );
    let (bytes, result) = save(&registry, &mut session, &dir);
    // The element keeps its ID; the text changed in the table.
    assert_eq!(bytes, plugin(true, &1u32.to_le_bytes(), &2u32.to_le_bytes()));
    let strings = result["strings"].as_array().unwrap();
    assert_eq!(strings.len(), 1, "{result}");
    assert_eq!(strings[0]["table"], "Loc_English.STRINGS");
    let saved = std::fs::read(dir.join("out").join("Strings").join("Loc_English.STRINGS")).unwrap();
    assert_eq!(saved, table(&[(1, "Changed"), (2, "Second")], false));
    // Saved: the table is no longer modified.
    let (_, result) = save(&registry, &mut session, &dir);
    assert_eq!(result["strings"], json!([]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_string_takes_the_next_id_of_the_plugins_tables() {
    let _guard = test_lock();
    let (dir, file) = load(
        "add",
        plugin(true, &1u32.to_le_bytes(), &0u32.to_le_bytes()),
        &loaded_tables(),
    );
    let (registry, mut session) = session(&file);
    // The tables of a plugin load on the first lookup of a string with an
    // ID. ID 0 is no lookup, and `AddValue` makes new, empty tables for a
    // plugin whose tables are not loaded (an upstream quirk: saving them
    // would replace the plugin's tables), so a string is read first, as the
    // GUI reads the names of the records it lists.
    call(
        &registry,
        &mut session,
        "elements.get",
        json!({ "form_id": "01000800", "path": "DATA\\Name" }),
    );
    // ID 0 is no string: the text is a new string (`AddValue`).
    call(
        &registry,
        &mut session,
        "elements.set",
        json!({ "form_id": "01000801", "path": "DATA\\Name", "value": "New" }),
    );
    let (bytes, _) = save(&registry, &mut session, &dir);
    assert_eq!(bytes, plugin(true, &1u32.to_le_bytes(), &3u32.to_le_bytes()));
    let saved = std::fs::read(dir.join("out").join("Strings").join("Loc_English.STRINGS")).unwrap();
    assert_eq!(saved, table(&[(1, "First"), (2, "Second"), (3, "New")], false));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delocalizing_puts_the_texts_into_the_plugin() {
    let _guard = test_lock();
    let (dir, file) = load(
        "delocalize",
        plugin(true, &1u32.to_le_bytes(), &0u32.to_le_bytes()),
        &loaded_tables(),
    );
    let (registry, mut session) = session(&file);
    let dry = call(
        &registry,
        &mut session,
        "localization.delocalize",
        json!({ "file": "Loc.esm", "dry_run": true }),
    );
    assert_eq!(dry["localizable"], 2);
    assert_eq!(dry["localized"], true);
    let result = call(
        &registry,
        &mut session,
        "localization.delocalize",
        json!({ "file": "Loc.esm" }),
    );
    assert_eq!(result["localized"], false);
    let (bytes, saved) = save(&registry, &mut session, &dir);
    assert_eq!(bytes, plugin(false, b"First\0", b"\0"));
    assert_eq!(saved["strings"], json!([]));
    let error = registry
        .call(&mut session, "localization.delocalize", json!({ "file": "Loc.esm" }))
        .unwrap_err();
    assert_eq!(error.code, "invalid_state");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn localizing_makes_three_tables_with_the_ids_in_gathered_order() {
    let _guard = test_lock();
    let (dir, file) = load("localize", plugin(false, b"First\0", b"Second\0"), &[]);
    let (registry, mut session) = session(&file);
    let result = call(
        &registry,
        &mut session,
        "localization.localize",
        json!({ "file": "Loc.esm" }),
    );
    assert_eq!(result["localizable"], 2);
    assert_eq!(result["localized"], true);
    let (bytes, saved) = save(&registry, &mut session, &dir);
    // `GatherLStrings` walks the last element first, so the second game
    // setting gets ID 1.
    assert_eq!(bytes, plugin(true, &2u32.to_le_bytes(), &1u32.to_le_bytes()));
    let names: Vec<&str> = saved["strings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["table"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["Loc_English.DLSTRINGS", "Loc_English.ILSTRINGS", "Loc_English.STRINGS"]
    );
    let strings = |name: &str| std::fs::read(dir.join("out").join("Strings").join(name)).unwrap();
    assert_eq!(
        strings("Loc_English.STRINGS"),
        table(&[(1, "Second"), (2, "First")], false)
    );
    assert_eq!(strings("Loc_English.DLSTRINGS"), table(&[], true));
    assert_eq!(strings("Loc_English.ILSTRINGS"), table(&[], true));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_localization_editor_lists_sets_and_exports_strings() {
    let _guard = test_lock();
    let (dir, file) = load(
        "editor",
        plugin(true, &1u32.to_le_bytes(), &2u32.to_le_bytes()),
        &loaded_tables(),
    );
    let (registry, mut session) = session(&file);
    let files = call(&registry, &mut session, "localization.files", json!({}));
    let names: Vec<&str> = files["tables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|table| table["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["Loc_English.DLSTRINGS", "Loc_English.ILSTRINGS", "Loc_English.STRINGS"]
    );
    let strings = call(
        &registry,
        &mut session,
        "localization.strings",
        json!({ "table": "Loc_English.STRINGS" }),
    );
    assert_eq!(strings["strings"][1], json!({ "id": "00000002", "text": "Second" }));
    let get = call(
        &registry,
        &mut session,
        "localization.get",
        json!({ "form_id": "01000801", "path": "DATA\\Name" }),
    );
    assert_eq!(get["table"], "Loc_English.STRINGS");
    assert_eq!(get["id"], "00000002");
    let dry = call(
        &registry,
        &mut session,
        "localization.set",
        json!({ "table": "Loc_English.STRINGS", "id": "2", "text": "Two\nLines", "editor_text": true, "dry_run": true }),
    );
    assert_eq!(dry["new"], "Two\r\nLines\r\n");
    assert_eq!(dry["changed"], true);
    let unchanged = call(
        &registry,
        &mut session,
        "localization.get",
        json!({ "table": "Loc_English.STRINGS", "id": "2" }),
    );
    assert_eq!(unchanged["text"], "Second");
    call(
        &registry,
        &mut session,
        "localization.set",
        json!({ "table": "Loc_English.STRINGS", "id": "2", "text": "Two" }),
    );
    let value = call(
        &registry,
        &mut session,
        "elements.get",
        json!({ "form_id": "01000801", "path": "DATA\\Name" }),
    );
    assert_eq!(value["value"], "Two");
    let output = dir.join("export.txt");
    let export = call(
        &registry,
        &mut session,
        "localization.export",
        json!({ "table": "Loc_English.STRINGS", "output": output }),
    );
    assert_eq!(export["strings"], 2);
    assert_eq!(
        std::fs::read(&output).unwrap(),
        b"[00000001]\r\nFirst\r\n[00000002]\r\nTwo\r\n"
    );
    let error = registry
        .call(
            &mut session,
            "localization.get",
            json!({ "table": "Loc_English.STRINGS" }),
        )
        .unwrap_err();
    assert_eq!(error.code, "invalid_params");
    // A language switch would drop the unsaved change.
    let error = registry
        .call(&mut session, "localization.language", json!({ "language": "French" }))
        .unwrap_err();
    assert_eq!(error.code, "unsaved_strings");
    let switched = call(
        &registry,
        &mut session,
        "localization.language",
        json!({ "language": "French", "discard": true }),
    );
    assert_eq!(switched["discarded"], json!(["Loc_English.STRINGS"]));
    // No French tables: the strings are missing now.
    let value = call(
        &registry,
        &mut session,
        "elements.get",
        json!({ "form_id": "01000801", "path": "DATA\\Name" }),
    );
    assert_eq!(value["value"], "<Error: No strings file for lstring ID 00000002>");
    xedit_core::interface::globals::set_language("English");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_translate_mode_edits_only_translatable_elements() {
    let _guard = test_lock();
    let (dir, file) = load(
        "translate",
        plugin(true, &1u32.to_le_bytes(), &2u32.to_le_bytes()),
        &loaded_tables(),
    );
    let (registry, mut session) = session(&file);
    xedit_core::interface::globals::set_translation_mode(true);
    let delete = registry.call(&mut session, "records.delete", json!({ "form_id": "01000800" }));
    let editor_id = registry.call(
        &mut session,
        "elements.set",
        json!({ "form_id": "01000800", "path": "EDID", "value": "sRenamed" }),
    );
    let text = registry.call(
        &mut session,
        "elements.set",
        json!({ "form_id": "01000800", "path": "DATA\\Name", "value": "Translated" }),
    );
    xedit_core::interface::globals::set_translation_mode(false);
    assert_eq!(delete.unwrap_err().code, "translate_mode");
    assert_eq!(editor_id.unwrap_err().code, "translate_mode");
    assert!(text.is_ok(), "{text:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
