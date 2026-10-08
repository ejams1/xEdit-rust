// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The reference index on synthetic Skyrim SE files: a master with a
//! keyword and a form list, and a plugin with a keyword, an override of the
//! master's list and a list of its own. The referenced-by lists after the
//! build, after edits that change and remove references, after a record is
//! deleted or copied, after a FormID change, with one thread and several,
//! and loaded from the reference cache.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use xedit_core::implementation::refs::{BuildOrLoadRefResult, build_or_load_refs};
use xedit_core::implementation::{FileImpl, MainRecordImpl, refcache, wb_file};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{Element, FormID, GameMode, MainRecord};
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

fn group_of(label: u32, group_type: i32, records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(&label.to_le_bytes());
    bytes.extend_from_slice(&(group_type as u32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

fn group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    group_of(u32::from_le_bytes(*label), 0, records)
}

fn header(flags: u32, record_count: u32, next_object_id: u32, masters: &[&str]) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.71f32.to_le_bytes());
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

fn keyword(form_id: u32, editor_id: &[u8]) -> Vec<u8> {
    main_record(b"KYWD", 0, form_id, &sub_record(b"EDID", editor_id))
}

fn form_list(form_id: u32, editor_id: &[u8], entries: &[u32]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    for entry in entries {
        data.extend(sub_record(b"LNAM", &entry.to_le_bytes()));
    }
    main_record(b"FLST", 0, form_id, &data)
}

fn master_bytes() -> Vec<u8> {
    let mut bytes = header(1, 4, 0x802, &[]);
    bytes.extend(group(b"KYWD", &keyword(0x800, b"KwMaster\0")));
    bytes.extend(group(b"FLST", &form_list(0x801, b"ListMaster\0", &[0x800])));
    bytes
}

fn plugin_bytes() -> Vec<u8> {
    let mut bytes = header(0, 8, 0x902, &["Master.esm"]);
    bytes.extend(group(b"KYWD", &keyword(0x0100_0900, b"KwPlugin\0")));
    let mut lists = form_list(0x801, b"ListMaster\0", &[0x800, 0x0100_0900]);
    lists.extend(form_list(0x0100_0901, b"ListPlugin\0", &[0x0100_0900, 0x800]));
    bytes.extend(group(b"FLST", &lists));
    // An interior cell 0x910 (2320: block 0, sub-block 2).
    let mut cell = sub_record(b"EDID", b"CellPlugin\0");
    cell.extend(sub_record(b"DATA", &1u16.to_le_bytes()));
    let sub_block = group_of(2, 3, &main_record(b"CELL", 0, 0x0100_0910, &cell));
    let block = group_of(0, 2, &sub_block);
    bytes.extend(group(b"CELL", &block));
    bytes
}

/// A fresh directory with the master and the plugin, loaded.
fn setup(test: &str) -> (PathBuf, Arc<FileImpl>, Arc<FileImpl>) {
    let dir = std::env::temp_dir().join(format!("xedit-refs-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Master.esm"), master_bytes()).unwrap();
    std::fs::write(dir.join("Plugin.esp"), plugin_bytes()).unwrap();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let plugin = wb_file(dir.join("Plugin.esp").to_str().unwrap(), i32::MAX, FileStates::empty()).unwrap();
    let master = plugin.masters()[0].clone();
    (dir, master, plugin)
}

fn session(files: Vec<Arc<FileImpl>>) -> Session {
    let mut session = Session::with_files(GameMode::gmSSE, files);
    session.allow_edit(true);
    session
}

fn record(file: &Arc<FileImpl>, form_id: u32) -> Arc<MainRecordImpl> {
    file.contained_record_by_load_order_form_id(FormID::from_cardinal(form_id))
        .unwrap_or_else(|| panic!("no record {form_id:08X} in {}", file.file_name()))
}

/// The referenced-by list of a record as `FormID@file`.
fn referenced_by(record: &Arc<MainRecordImpl>) -> Vec<String> {
    record
        .referenced_by()
        .iter()
        .map(|entry| {
            format!(
                "{}@{}",
                entry.get_load_order_form_id().to_string(false),
                entry.get_file().unwrap().get_name()
            )
        })
        .collect()
}

fn names(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            format!(
                "{}@{}",
                entry["form_id"].as_str().unwrap(),
                entry["file"].as_str().unwrap()
            )
        })
        .collect()
}

fn no_cache() {
    xedit_session::refs::set_cache_options(None, true, true, true);
    xedit_session::refs::init_cache_path();
}

#[test]
fn referenced_by_lists_after_the_build() {
    let _guard = test_lock();
    no_cache();
    let (dir, master, plugin) = setup("build");
    let mut session = session(vec![plugin.clone()]);
    let registry = Registry::standard();
    let result = registry
        .call(&mut session, "refs.get", json!({ "form_id": "00000800" }))
        .unwrap();
    // The master's list, its override in the plugin and the plugin's list,
    // by FormID and then by load order.
    assert_eq!(
        names(&result["referenced_by"]),
        ["00000801@Master.esm", "00000801@Plugin.esp", "01000901@Plugin.esp"]
    );
    assert_eq!(result["referenced_by_count"], 3);
    assert_eq!(result["master"]["file"], "Master.esm");
    // The keyword refers to nothing.
    assert_eq!(result["references"], json!([]));
    // An override keeps no list: its master does.
    assert_eq!(
        referenced_by(&record(&plugin, 0x0100_0900)),
        ["00000801@Plugin.esp", "01000901@Plugin.esp"]
    );
    let list = registry
        .call(
            &mut session,
            "refs.get",
            json!({ "form_id": "00000801", "file": "Plugin.esp", "offset": 0, "limit": 1 }),
        )
        .unwrap();
    assert_eq!(list["referenced_by_count"], 0);
    let references: Vec<&str> = list["references"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reference| reference["file_form_id"].as_str().unwrap())
        .collect();
    // As the plugin stores them: the master's keyword and its own.
    assert_eq!(references, ["00000800", "01000900"]);
    assert_eq!(list["references"][1]["record"]["editor_id"], "KwPlugin");
    assert!(master.refs_built() && plugin.refs_built());
    let built = registry.call(&mut session, "refs.build", json!({})).unwrap();
    assert!(
        built["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["result"] == "built")
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn edits_keep_the_lists() {
    let _guard = test_lock();
    no_cache();
    let (dir, master, plugin) = setup("edits");
    let mut session = session(vec![master.clone(), plugin.clone()]);
    session.ensure_refs().unwrap();
    let registry = Registry::standard();
    // The plugin's list refers to the master's list instead of the keyword.
    registry
        .call(
            &mut session,
            "elements.set",
            json!({ "form_id": "01000901", "path": "FormIDs\\[1]", "value": "00000801" }),
        )
        .unwrap();
    assert_eq!(
        referenced_by(&record(&master, 0x800)),
        ["00000801@Master.esm", "00000801@Plugin.esp"]
    );
    assert_eq!(referenced_by(&record(&master, 0x801)), ["01000901@Plugin.esp"]);
    // A deleted record leaves the lists.
    registry
        .call(
            &mut session,
            "records.delete",
            json!({ "form_id": "01000901", "file": "Plugin.esp" }),
        )
        .unwrap();
    assert_eq!(referenced_by(&record(&master, 0x801)), Vec::<String>::new());
    assert_eq!(referenced_by(&record(&plugin, 0x0100_0900)), ["00000801@Plugin.esp"]);
    // A copy as a new record refers to what the source refers to.
    let copied = registry
        .call(
            &mut session,
            "records.copy",
            json!({ "form_id": "00000801", "from": "Master.esm", "to": "Plugin.esp", "as_new": true }),
        )
        .unwrap();
    let new = copied["copy"]["form_id"].as_str().unwrap().to_owned();
    assert_eq!(
        referenced_by(&record(&master, 0x800)),
        [
            "00000801@Master.esm".to_owned(),
            "00000801@Plugin.esp".to_owned(),
            format!("{new}@Plugin.esp")
        ]
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn form_id_change_moves_the_list() {
    let _guard = test_lock();
    no_cache();
    let (dir, _master, plugin) = setup("change");
    let mut session = session(vec![plugin.clone()]);
    let registry = Registry::standard();
    registry
        .call(
            &mut session,
            "formids.change",
            json!({ "form_id": "01000900", "new_form_id": "01000A00" }),
        )
        .unwrap();
    // The referencing records refer to the new FormID and are listed again.
    assert_eq!(
        referenced_by(&record(&plugin, 0x0100_0A00)),
        ["00000801@Plugin.esp", "01000901@Plugin.esp"]
    );
    let result = registry
        .call(&mut session, "refs.get", json!({ "form_id": "01000901" }))
        .unwrap();
    let references: Vec<&str> = result["references"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reference| reference["file_form_id"].as_str().unwrap())
        .collect();
    assert_eq!(references, ["00000800", "01000A00"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn lists_do_not_depend_on_the_thread_count() {
    let _guard = test_lock();
    no_cache();
    let mut lists = Vec::new();
    for threads in [1, 4] {
        xedit_core::threads::set_threads(threads);
        let (dir, master, plugin) = setup(&format!("threads{threads}"));
        let mut session = session(vec![plugin.clone()]);
        session.ensure_refs().unwrap();
        lists.push(
            [
                record(&master, 0x800),
                record(&master, 0x801),
                record(&plugin, 0x0100_0900),
            ]
            .iter()
            .map(referenced_by)
            .collect::<Vec<_>>(),
        );
        std::fs::remove_dir_all(dir).ok();
    }
    xedit_core::threads::set_threads(0);
    assert_eq!(lists[0], lists[1]);
}

#[test]
fn cache_is_saved_and_loaded() {
    let _guard = test_lock();
    let (dir, master, plugin) = setup("cache");
    let cache = dir.join("cache");
    xedit_session::refs::set_cache_options(Some(cache.to_str().unwrap()), false, false, false);
    xedit_session::refs::init_cache_path();
    // Every file is big enough to be cached.
    xedit_core::interface::globals::set_cache_records_threshold(0);
    let results = build_or_load_refs(&[master.clone(), plugin.clone()], false).unwrap();
    assert_eq!(results, [BuildOrLoadRefResult::blrBuiltAndSaved; 2]);
    let built: Vec<Vec<String>> = [record(&master, 0x800), record(&plugin, 0x0100_0900)]
        .iter()
        .map(referenced_by)
        .collect();
    let path = refcache::cache_file_name(&plugin).unwrap();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.contains("_Plugin_esp_"), "{name}");
    assert!(name.ends_with(".refcache"), "{name}");
    let stream = refcache::decompress(&std::fs::read(&path).unwrap()).unwrap();
    let cached = refcache::decode(&stream, false).unwrap();
    // The keyword, the two lists and the cell.
    assert_eq!(cached.len(), 4);
    let list = cached
        .iter()
        .find(|record| record.form_id.to_cardinal() == 0x0100_0901)
        .unwrap();
    assert_eq!(
        list.references.iter().map(|id| id.to_cardinal()).collect::<Vec<_>>(),
        [0x800, 0x0100_0900]
    );
    assert_eq!(list.editor_id, "ListPlugin");

    // A new load reads the references from the cache.
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    let plugin = wb_file(dir.join("Plugin.esp").to_str().unwrap(), i32::MAX, FileStates::empty()).unwrap();
    let master = plugin.masters()[0].clone();
    let results = build_or_load_refs(&[master.clone(), plugin.clone()], false).unwrap();
    assert_eq!(results, [BuildOrLoadRefResult::blrLoaded; 2]);
    let loaded: Vec<Vec<String>> = [record(&master, 0x800), record(&plugin, 0x0100_0900)]
        .iter()
        .map(referenced_by)
        .collect();
    assert_eq!(loaded, built);
    assert_eq!(record(&plugin, 0x0100_0901).get_editor_id(), "ListPlugin");
    xedit_core::interface::globals::set_cache_records_threshold(500);
    no_cache();
    std::fs::remove_dir_all(dir).ok();
}

/// A Fallout 3 condition in the old 20 byte form with the "Run On Target"
/// flag: its `AfterLoad` resizes it to 28 bytes while it is built
/// (`SetDataSize` inside the init), which builds it again, so the condition
/// keeps its values and refers to the record of its parameter.
#[test]
fn fallout3_condition_resized_on_load_keeps_its_reference() {
    let _guard = test_lock();
    no_cache();
    let dir = std::env::temp_dir().join(format!("xedit-refs-ctda-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut ctda = vec![0x02, 0, 0, 0];
    ctda.extend_from_slice(&1.0f32.to_le_bytes());
    ctda.extend_from_slice(&72u16.to_le_bytes()); // GetIsID
    ctda.extend_from_slice(&[0, 0]);
    ctda.extend_from_slice(&0x0000_0801u32.to_le_bytes());
    ctda.extend_from_slice(&0u32.to_le_bytes());
    let mut idle = sub_record(b"EDID", b"IdleTest\0");
    idle.extend(sub_record(b"CTDA", &ctda));
    let mut plugin = header(1, 2, 0x802, &[]);
    plugin.extend(group(b"IDLE", &main_record(b"IDLE", 0, 0x800, &idle)));
    plugin.extend(group(
        b"NPC_",
        &main_record(b"NPC_", 0, 0x801, &sub_record(b"EDID", b"NpcTest\0")),
    ));
    std::fs::write(dir.join("Test.esm"), plugin).unwrap();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("fo3").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmFO3);
    let file = wb_file(dir.join("Test.esm").to_str().unwrap(), i32::MAX, FileStates::empty()).unwrap();
    let mut session = Session::with_files(GameMode::gmFO3, vec![file.clone()]);
    let registry = Registry::standard();
    let condition = registry
        .call(
            &mut session,
            "elements.get",
            json!({ "form_id": "00000800", "path": "Conditions\\[0]", "depth": 1 }),
        )
        .unwrap();
    let values: Vec<(String, String)> = condition["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|child| {
            (
                child["name"].as_str().unwrap().to_owned(),
                child["value"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(
        values.contains(&("Run On".to_owned(), "Target".to_owned())),
        "{values:?}"
    );
    assert!(
        values.contains(&("Type".to_owned(), "Equal To".to_owned())),
        "{values:?}"
    );
    session.ensure_refs().unwrap();
    assert_eq!(referenced_by(&record(&file, 0x801)), ["00000800@Test.esm"]);
    std::fs::remove_dir_all(dir).ok();
}

/// A master added to the plugin moves the FileIDs of its FormIDs: the
/// references the plugin's records keep follow (`MastersUpdated`), and the
/// referenced-by lists stay the same.
#[test]
fn masters_update_keeps_the_references() {
    let _guard = test_lock();
    no_cache();
    let dir = std::env::temp_dir().join(format!("xedit-refs-masters-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Master.esm"), master_bytes()).unwrap();
    std::fs::write(dir.join("Plugin.esp"), plugin_bytes()).unwrap();
    std::fs::write(dir.join("Extra.esm"), header(1, 0, 0x800, &[])).unwrap();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let extra = wb_file(dir.join("Extra.esm").to_str().unwrap(), i32::MAX, FileStates::empty()).unwrap();
    let plugin = wb_file(dir.join("Plugin.esp").to_str().unwrap(), i32::MAX, FileStates::empty()).unwrap();
    let master = plugin.masters()[0].clone();
    let mut session = session(vec![extra, plugin.clone()]);
    session.ensure_refs().unwrap();
    let before: Vec<Vec<String>> = [record(&master, 0x0100_0800), record(&plugin, 0x0200_0900)]
        .iter()
        .map(referenced_by)
        .collect();
    let registry = Registry::standard();
    let added = registry
        .call(
            &mut session,
            "masters.add",
            json!({ "file": "Plugin.esp", "masters": ["Extra.esm"] }),
        )
        .unwrap();
    assert_eq!(added["masters"], json!(["Extra.esm", "Master.esm"]));
    let after: Vec<Vec<String>> = [record(&master, 0x0100_0800), record(&plugin, 0x0200_0900)]
        .iter()
        .map(referenced_by)
        .collect();
    assert_eq!(after, before);
    // Extra.esm loads first: Master.esm has the load order FileID 01 and the
    // plugin 02. As the plugin stores them now, Master.esm is master 1 and
    // the plugin's own FormIDs take FileID 02 (they were 00 and 01).
    let references: Vec<String> = record(&plugin, 0x0200_0901)
        .references()
        .iter()
        .map(|form_id| form_id.to_string(false))
        .collect();
    assert_eq!(references, ["01000800", "02000900"]);
    std::fs::remove_dir_all(dir).ok();
}

/// An Oblivion magic effect code (`TwbChar4`) refers to the effect with the
/// code as its editor ID, also from the hardcoded file, which finds it in
/// the game master it compares to.
#[test]
fn oblivion_effect_codes_refer_to_the_effect() {
    let _guard = test_lock();
    no_cache();
    let dir = std::env::temp_dir().join(format!("xedit-refs-tes4-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Oblivion's record header has no version fields: 20 bytes.
    let record = |signature: &[u8; 4], form_id: u32, data: &[u8]| {
        let mut bytes = signature.to_vec();
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&form_id.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    };
    let tes4_group = |label: &[u8; 4], records: &[u8]| {
        let mut bytes = b"GRUP".to_vec();
        bytes.extend_from_slice(&((20 + records.len()) as u32).to_le_bytes());
        bytes.extend_from_slice(label);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(records);
        bytes
    };
    let mut hedr = 0.8f32.to_le_bytes().to_vec();
    hedr.extend_from_slice(&1u32.to_le_bytes());
    hedr.extend_from_slice(&0x2000u32.to_le_bytes());
    let mut plugin = record(b"TES4", 0, &sub_record(b"HEDR", &hedr));
    plugin.extend(tes4_group(
        b"MGEF",
        &record(b"MGEF", 0x1887, &sub_record(b"EDID", b"PARA\0")),
    ));
    std::fs::write(dir.join("Oblivion.esm"), plugin).unwrap();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    let mut session = Session::load("tes4", &[dir.join("Oblivion.esm").to_string_lossy().into_owned()]).unwrap();
    session.ensure_refs().unwrap();
    let files = xedit_core::implementation::masters::loaded_files();
    let master = files
        .iter()
        .find(|file| file.file_name().ends_with("Oblivion.esm"))
        .unwrap();
    let referenced = referenced_by(&record_of(master, 0x1887));
    // The hardcoded spell `DefaultMarksmanParalyzeSpell` [SPEL:00000137]
    // has the effect PARA.
    assert!(
        referenced.contains(&"00000137@Oblivion.exe".to_owned()),
        "{referenced:?}"
    );
    std::fs::remove_dir_all(dir).ok();
}

fn record_of(file: &Arc<FileImpl>, form_id: u32) -> Arc<MainRecordImpl> {
    record(file, form_id)
}
