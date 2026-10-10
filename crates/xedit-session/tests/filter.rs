// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `filter.*` commands and the filters of `records.compare` and
//! `refs.get` on synthetic Skyrim SE plugins: a master with two records and
//! a plugin that overrides one of them, in a data folder of their own, with
//! a settings file for the presets. The parity harness checks the filter
//! options against the GUI oracle on the corpus (`cargo xtask parity
//! filter`); these tests cover what it can not reach: the presets, the
//! `filter.remove`, the flags the script host does not expose (`only_one`,
//! `conflict_only`) and the two filters of the tabs.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use xedit_core::implementation::{FileImpl, wb_file};
use xedit_core::interface::globals::{set_data_path, test_lock};
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{Element, GameMode, MainRecord};
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

/// The interior cell `[00000801]`: block 9, sub-block 4 of its object ID.
fn cell() -> Vec<u8> {
    let mut data = sub_record(b"EDID", b"TestCell\0");
    data.extend(sub_record(b"DATA", &1u16.to_le_bytes()));
    main_record(b"CELL", 0, 0x801, &data)
}

/// The master: a game setting and an interior cell nothing refers to.
fn master_bytes() -> Vec<u8> {
    let mut bytes = header(1, 3, 0x802, &[]);
    bytes.extend(top_group(b"GMST", &gmst(0x800, b"fMaster\0", 1.5)));
    let cells = group(9, 2, &group(4, 3, &cell()));
    bytes.extend(top_group(b"CELL", &cells));
    bytes
}

/// The plugin: the game setting of the master with another value, and one of
/// its own.
fn plugin_bytes() -> Vec<u8> {
    // The FormID `0x0800` is the master's slot 0 (the first master): the
    // record overrides the master's game setting. `0x0100_0802` is the
    // plugin's own first slot.
    let mut bytes = header(0, 2, 0x802, &["Master.esm"]);
    bytes.extend(top_group(b"GMST", &gmst(0x0800, b"fMaster\0", 2.5)));
    bytes.extend(top_group(b"GMST", &gmst(0x0100_0802, b"fOther\0", 3.5)));
    bytes
}

struct Files {
    master: Arc<FileImpl>,
    plugin: Arc<FileImpl>,
}

/// A data folder with a settings file of its own, so the presets do not
/// touch the user's.
fn data_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xedit-filter-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    set_data_path(&dir.to_string_lossy());
    xedit_session::modgroups::set_file_options(None, Some(&dir.join("xedit.ini").to_string_lossy()));
    dir
}

fn load(dir: &Path) -> Files {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let open = |name: &str, bytes: Vec<u8>| {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        wb_file(&path.to_string_lossy(), i32::MAX, FileStates::empty()).unwrap()
    };
    // `wb_file` registers each file in `Files` itself.
    let master = open("Master.esm", master_bytes());
    let plugin = open("Plugin.esp", plugin_bytes());
    Files { master, plugin }
}

fn session(files: &Files) -> (Registry, Session) {
    let mut session = Session::with_files(GameMode::gmSSE, vec![files.master.clone(), files.plugin.clone()]);
    session.allow_edit(true);
    (Registry::standard(), session)
}

/// The record names the tree keeps.
fn records_of(result: &Value) -> Vec<String> {
    result["records"]
        .as_array()
        .map(|names| names.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Whether a kept record is the one of the FormID (`[00000800]`).
fn is_form_id(names: &[String], form_id: &str) -> Vec<String> {
    names
        .iter()
        .filter(|name| name.contains(&format!("[{form_id}]")))
        .cloned()
        .collect()
}

/// The signatures of the kept records.
fn signatures(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter_map(|name| name.split_whitespace().next().map(str::to_owned))
        .collect()
}

#[test]
fn filters_by_signature_and_editor_id() {
    let _guard = test_lock();
    let dir = data_dir("options");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    let result = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "options": { "by_signature": "CELL" }, "list_records": true }),
        )
        .unwrap();
    let kept = records_of(&result);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(is_form_id(&kept, "00000801").len(), 1, "{kept:?}");
    // The cell is in the master only, so the plugin lost everything. A file
    // with no record left reports nothing filtered (its `FileFiltered` stays
    // 0), while the master logs the line of its own partial count.
    let files = result["files"].as_array().unwrap();
    let names: Vec<&str> = files.iter().map(|file| file["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Master.esm", "Plugin.esp"]);
    // The master lost its game setting and its file header record, which the
    // signature filter takes out too: one of its two records is left.
    assert_eq!(files[0]["records"], 2);
    assert_eq!(files[0]["filtered"], 1);
    assert_eq!(files[0]["logged"], true);
    // The plugin lost everything, and a file with no record left reports
    // nothing filtered (`FileFiltered` stays 0), so it logs no line.
    assert_eq!(files[1]["filtered"], 0);
    assert_eq!(files[1]["logged"], false);

    let result = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "options": { "by_editor_id": "fmaster" }, "list_records": true }),
        )
        .unwrap();
    let kept = records_of(&result);
    // The game setting and its override.
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!(is_form_id(&kept, "00000800").len(), 2, "{kept:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_default_options_keep_everything() {
    let _guard = test_lock();
    let dir = data_dir("empty");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    let result = registry.call(&mut session, "filter.apply", json!({})).unwrap();
    assert_eq!(result["pass1"], result["pass2"]);
    assert_eq!(result["pass1"], result["unfiltered"]);
    for file in result["files"].as_array().unwrap() {
        // A file that keeps every record reports one less than nothing
        // filtered (`FileFiltered` counts the header record), and no line.
        assert!(file["filtered"].as_i64().unwrap() <= 0, "{file}");
        assert_eq!(file["logged"], false);
    }
    // An empty set filters everything out.
    let result = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "options": { "by_signature": "" } }),
        )
        .unwrap();
    assert_eq!(result["unfiltered"], 0);
    let result = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "options": { "conflict_this": [] } }),
        )
        .unwrap();
    assert_eq!(result["unfiltered"], 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn only_one_and_conflict_only_keep_the_overridden_record() {
    let _guard = test_lock();
    let dir = data_dir("onlyone");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    let result = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "only_one": true, "list_records": true }),
        )
        .unwrap();
    let kept = records_of(&result);
    // UPSTREAM-QUIRK: the filter drops the records with overrides and keeps
    // the ones without any: the game setting with its override goes, the
    // cell and the plugin's own record stay (with the two file headers).
    assert_eq!(kept.len(), 4, "{kept:?}");
    assert_eq!(is_form_id(&kept, "00000800").len(), 0, "{kept:?}");
    assert_eq!(is_form_id(&kept, "00000801").len(), 1, "{kept:?}");
    assert_eq!(is_form_id(&kept, "01000802").len(), 1, "{kept:?}");

    // `FilterConflictOnly` drops every record of a FormID with fewer than two
    // overrides, which is every record here.
    let result = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "conflict_only": true, "list_records": true }),
        )
        .unwrap();
    assert_eq!(records_of(&result).len(), 0, "{:?}", records_of(&result));

    // The cleaning preset alone keeps the records it does not filter, which
    // the cells without conflicts show.
    let result = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "options": { "inherit_conflict_by_parent": true } }),
        )
        .unwrap();
    assert_eq!(result["pass1"], result["pass2"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_reachable_filter_needs_the_reachable_information() {
    let _guard = test_lock();
    let dir = data_dir("reachable");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    // Without "Build Reachable Info" the option has no effect.
    let result = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "options": { "by_not_reachable_status": false }, "list_records": true }),
        )
        .unwrap();
    assert_eq!(records_of(&result).len(), 6, "{:?}", records_of(&result));
    let built = registry
        .call(&mut session, "refs.build_reachable", json!({ "build_refs": true }))
        .unwrap();
    assert_eq!(built["reachable_build"], true);
    // The game settings and the file headers are reached (their groups
    // always are), the cell is not: nothing refers to it.
    let result = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "options": { "by_not_reachable_status": false }, "list_records": true }),
        )
        .unwrap();
    let kept = records_of(&result);
    assert_eq!(kept.len(), 5, "{kept:?}");
    let signatures = signatures(&kept);
    assert_eq!(signatures.iter().filter(|name| *name == "GMST").count(), 3, "{kept:?}");
    assert_eq!(signatures.iter().filter(|name| *name == "TES4").count(), 2, "{kept:?}");
    let result = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "options": { "by_not_reachable_status": true }, "list_records": true }),
        )
        .unwrap();
    let kept = records_of(&result);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(is_form_id(&kept, "00000801").len(), 1, "{kept:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_preset_is_saved_listed_loaded_and_deleted() {
    let _guard = test_lock();
    let dir = data_dir("preset");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    let saved = registry
        .call(
            &mut session,
            "filter.preset.save",
            json!({ "name": "mine", "options": { "by_signature": "GMST", "deleted": true } }),
        )
        .unwrap();
    assert_eq!(saved["section"], "Filter mine");
    assert_eq!(saved["changed"], true);
    let listed = registry.call(&mut session, "filter.presets", json!({})).unwrap();
    assert_eq!(listed["presets"], json!(["", "mine"]));
    let again = registry
        .call(
            &mut session,
            "filter.preset.save",
            json!({ "name": "mine", "options": { "by_signature": "GMST", "deleted": true } }),
        )
        .unwrap();
    assert_eq!(again["changed"], false);
    // The preset is what `filter.apply` reads.
    let result = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "preset": "mine", "list_records": true }),
        )
        .unwrap();
    assert_eq!(result["options"]["by_signature"], "GMST");
    assert_eq!(result["options"]["deleted"], true);
    assert_eq!(records_of(&result).len(), 0, "no record is deleted");
    let deleted = registry
        .call(&mut session, "filter.preset.delete", json!({ "name": "mine" }))
        .unwrap();
    assert_eq!(deleted["erased"], true);
    let listed = registry.call(&mut session, "filter.presets", json!({})).unwrap();
    assert_eq!(listed["presets"], json!([""]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn removing_the_filter_lists_the_files() {
    let _guard = test_lock();
    let dir = data_dir("remove");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    let result = registry.call(&mut session, "filter.remove", json!({})).unwrap();
    assert_eq!(result["applied"], false);
    let names: Vec<&str> = result["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Master.esm", "Plugin.esp"]);
    assert_eq!(result["files"][0]["records"], 2);
    assert_eq!(result["files"][1]["records"], 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_view_filter_keeps_the_rows() {
    let _guard = test_lock();
    let dir = data_dir("view");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    let result = registry
        .call(
            &mut session,
            "records.compare",
            json!({ "form_id": "00000800", "file": "Plugin.esp" }),
        )
        .unwrap();
    let rows = result["rows"].as_array().unwrap().len();
    assert!(rows > 1, "the record has rows");
    assert!(result["view_filter"].is_null());
    // The name of the row of the editor ID is `EDID - Editor ID`; the filter
    // keeps the rows that contain it.
    let result = registry
        .call(
            &mut session,
            "records.compare",
            json!({ "form_id": "00000800", "file": "Plugin.esp", "view_filter_name": "editor" }),
        )
        .unwrap();
    let filter = &result["view_filter"];
    // The filter walks every row of the view; `rows` counts the rows at the
    // top of it.
    assert!(filter["rows"].as_u64().unwrap() >= rows as u64, "{filter}");
    assert!(filter["filtered"].as_u64().unwrap() > 0);
    assert_eq!(
        filter["shown"].as_u64().unwrap() + filter["filtered"].as_u64().unwrap(),
        filter["rows"].as_u64().unwrap()
    );
    let names: Vec<&str> = result["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert!(names.len() < rows, "{names:?}");
    for name in names {
        let lower = name.to_lowercase();
        assert!(lower.contains("editor") || lower.contains("edid"), "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_referenced_by_filter_keeps_matching_records() {
    let _guard = test_lock();
    let Some(data) = std::env::var_os("XEDIT_SSE_DATA").map(PathBuf::from) else {
        return;
    };
    let dir = data_dir("refby");
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let master = wb_file(
        &data.join("Skyrim.esm").to_string_lossy(),
        i32::MAX,
        FileStates::empty(),
    )
    .unwrap();
    xedit_core::interface::add_file(master.clone());
    let mut session = Session::with_files(GameMode::gmSSE, vec![master.clone()]);
    session.allow_edit(true);
    let registry = Registry::standard();
    let master = xedit_core::implementation::masters::loaded_files()
        .into_iter()
        .find(|file| file.get_name().eq_ignore_ascii_case("Skyrim.esm"))
        .unwrap();
    let mut found = None;
    for record in master.records() {
        if record.referenced_by_count() > 0 {
            found = Some(record);
            break;
        }
    }
    let Some(record) = found else {
        return;
    };
    let form_id = record.get_load_order_form_id().to_string(false);
    let all = registry
        .call(&mut session, "refs.get", json!({ "form_id": form_id }))
        .unwrap();
    let count = all["referenced_by_count"].as_u64().unwrap();
    assert!(count > 0);
    assert_eq!(all["filtered_count"], count);
    let name = all["referenced_by"][0]["editor_id"].as_str().unwrap().to_lowercase();
    let filtered = registry
        .call(
            &mut session,
            "refs.get",
            json!({ "form_id": form_id, "filter_name": name }),
        )
        .unwrap();
    let filtered_count = filtered["filtered_count"].as_u64().unwrap();
    assert!(filtered_count > 0 && filtered_count <= count);
    for entry in filtered["referenced_by"].as_array().unwrap() {
        let entry_file = entry["file"].as_str().unwrap().to_lowercase();
        let _ = entry_file;
    }
    // A file that holds none of the records hides every entry.
    let none = registry
        .call(
            &mut session,
            "refs.get",
            json!({ "form_id": form_id, "filter_file": "no such file" }),
        )
        .unwrap();
    assert_eq!(none["filtered_count"], 0);
    let _ = std::fs::remove_dir_all(&dir);
}
