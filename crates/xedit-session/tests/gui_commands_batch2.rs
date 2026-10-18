// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The second batch of issue #78: the cell child and Visible When Distant
//! items, the reference changes, the circular leveled list check, Copy
//! Idles, the delta patch, the two Oblivion helpers and the Log Analyzer.

use std::sync::Arc;

use serde_json::json;
use xedit_core::implementation::{FileBytes, FileImpl, wb_file_from_bytes};
use xedit_core::interface::globals::{set_data_path, test_lock};
use xedit_core::interface::types::{FileState, FileStates};
use xedit_core::interface::{Container, Element, GameMode, MainRecord};
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

fn session(game: GameMode, files: &[Arc<FileImpl>]) -> (Registry, Session) {
    let mut session = Session::with_files(game, files.to_vec());
    session.allow_edit(true);
    (Registry::standard(), session)
}

fn call(registry: &Registry, session: &mut Session, name: &str, params: serde_json::Value) -> serde_json::Value {
    registry
        .call(session, name, params)
        .unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn stat(form_id: u32, editor_id: &[u8], model: &str) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    let mut path = model.as_bytes().to_vec();
    path.push(0);
    data.extend(sub_record(b"MODL", &path));
    main_record(b"STAT", 0, form_id, &data)
}

fn reference(form_id: u32, base: u32) -> Vec<u8> {
    main_record(b"REFR", 0, form_id, &sub_record(b"NAME", &base.to_le_bytes()))
}

/// An interior cell with references in its temporary children group.
fn interior_cell(form_id: u32, references: &[u8]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", b"TestCell\0");
    data.extend(sub_record(b"DATA", &1u16.to_le_bytes()));
    let mut cell = main_record(b"CELL", 0, form_id, &data);
    cell.extend(group(form_id, 6, &group(form_id, 9, references)));
    top_group(
        b"CELL",
        &group(form_id >> 16 & 0xFF, 2, &group(form_id & 0xFF, 3, &cell)),
    )
}

/// An exterior cell of a worldspace, with references in its temporary
/// children group.
fn exterior_cell(world: u32, form_id: u32, references: &[u8]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", b"TestCell\0");
    data.extend(sub_record(b"DATA", &0u16.to_le_bytes()));
    let mut cell = main_record(b"CELL", 0, form_id, &data);
    cell.extend(group(form_id, 6, &group(form_id, 9, references)));
    let mut world_data = sub_record(b"EDID", b"TestWorld\0");
    world_data.extend(sub_record(b"DATA", &[0; 8]));
    let world_record = main_record(b"WRLD", 0, world, &world_data);
    let mut world_records = world_record;
    world_records.extend(group(world, 1, &group(0, 4, &group(0, 5, &cell))));
    top_group(b"WRLD", &world_records)
}

fn reference_plugin(exterior: bool) -> Vec<Arc<FileImpl>> {
    let master = header(1, 0, 0x800, &[]);
    let mut plugin = header(0, 4, 0x901, &["Skyrim.esm"]);
    plugin.extend(top_group(b"STAT", &stat(0x0100_0900, b"VwdStatic\0", "test\\vwd.nif")));
    let refs = {
        let mut refs = reference(0x0100_0801, 0x0100_0900);
        refs.extend(reference(0x0100_0802, 0x0100_0900));
        refs
    };
    if exterior {
        plugin.extend(exterior_cell(0x0100_0800, 0x0100_0803, &refs));
    } else {
        plugin.extend(interior_cell(0x0100_0800, &refs));
    }
    load_game("sse", vec![("Skyrim.esm", master), ("Plugin.esp", plugin)])
}

#[test]
fn makes_a_reference_persistent_and_visible_when_distant() {
    let _guard = test_lock();
    let files = reference_plugin(false);
    let (registry, mut session) = session(GameMode::gmSSE, &files);
    let record = files[1]
        .records()
        .into_iter()
        .find(|record| record.get_signature().to_string() == "REFR")
        .unwrap();
    assert!(!record.get_is_persistent());
    let group_type = || {
        record
            .get_container()
            .and_then(|group| {
                group
                    .as_element_impl()?
                    .group_record_impl()
                    .map(|group| group.group_type())
            })
            .expect("the children group")
    };
    let result = call(
        &registry,
        &mut session,
        "records.set_persistent",
        json!({ "form_id": "01000801", "file": "Plugin.esp", "persistent": true }),
    );
    assert_eq!(result["changed"], true);
    assert!(record.get_is_persistent());
    assert_eq!(group_type(), 8);
    let vwd = call(
        &registry,
        &mut session,
        "records.set_visible_when_distant",
        json!({ "form_id": "01000801", "file": "Plugin.esp", "visible_when_distant": true }),
    );
    assert_eq!(vwd["changed"], true);
    assert!(record.get_is_visible_when_distant());
    // Persistent takes the persistent children group while the flag is set.
    assert_eq!(group_type(), 8);
    let not_vwd = call(
        &registry,
        &mut session,
        "records.set_visible_when_distant",
        json!({ "form_id": "01000801", "file": "Plugin.esp", "visible_when_distant": false }),
    );
    assert_eq!(not_vwd["changed"], true);
    assert!(!record.get_is_visible_when_distant());
    assert_eq!(group_type(), 8);
    let temporary = call(
        &registry,
        &mut session,
        "records.set_persistent",
        json!({ "form_id": "01000801", "file": "Plugin.esp", "persistent": false }),
    );
    assert_eq!(temporary["changed"], true);
    assert!(!record.get_is_persistent());
    assert_eq!(group_type(), 9);
    // Not persistent: Skyrim keeps VWD references in the temporary group
    // (wbVWDInTemporary); the flag alone changes no group.
    let vwd = call(
        &registry,
        &mut session,
        "records.set_visible_when_distant",
        json!({ "form_id": "01000801", "file": "Plugin.esp", "visible_when_distant": true }),
    );
    assert_eq!(vwd["changed"], true);
    assert!(record.get_is_visible_when_distant());
    assert_eq!(group_type(), 9);
    let not_vwd = call(
        &registry,
        &mut session,
        "records.set_visible_when_distant",
        json!({ "form_id": "01000801", "file": "Plugin.esp", "visible_when_distant": false }),
    );
    assert_eq!(not_vwd["changed"], true);
    assert!(!record.get_is_visible_when_distant());
    assert_eq!(group_type(), 9);
}

fn vwd_folder() -> TempDir {
    let folder = TempDir::new(format!("xedit-vwd-{}", std::process::id()));
    std::fs::create_dir_all(folder.join("meshes").join("test")).unwrap();
    std::fs::write(folder.join("meshes").join("test").join("vwd_far.nif"), b"nif").unwrap();
    set_data_path(&format!("{}\\", folder.display()));
    xedit_core::container_handler::clear_containers();
    xedit_core::container_handler::add_folder(folder.path());
    folder
}

#[test]
fn sets_vwd_on_the_references_with_a_vwd_mesh() {
    let _guard = test_lock();
    let folder = vwd_folder();
    let files = reference_plugin(true);
    let (registry, mut session) = session(GameMode::gmSSE, &files);
    let result = call(
        &registry,
        &mut session,
        "references.set_vwd_auto",
        json!({ "files": ["Plugin.esp"] }),
    );
    assert_eq!(result["changed"].as_array().unwrap().len(), 2);
    for record in files[1].records() {
        if record.get_signature().to_string() == "REFR" {
            assert!(record.get_is_visible_when_distant(), "{}", record.get_name());
        }
    }
    drop(folder);
    xedit_core::container_handler::clear_containers();
}

#[test]
fn copies_the_vwd_references_into_a_plugin() {
    let _guard = test_lock();
    let folder = vwd_folder();
    let files = reference_plugin(true);
    let mut target = header(0, 0, 0x800, &["Skyrim.esm"]);
    let bytes = std::mem::take(&mut target);
    let target_file = wb_file_from_bytes("Target.esp", 2, FileStates::empty(), FileBytes::Owned(bytes)).unwrap();
    let mut all = files.clone();
    all.push(target_file.clone());
    let (registry, mut session) = session(GameMode::gmSSE, &all);
    let result = call(
        &registry,
        &mut session,
        "references.copy_vwd_auto",
        json!({ "files": ["Plugin.esp"], "to": "Target.esp" }),
    );
    assert_eq!(result["copied"].as_array().unwrap().len(), 2);
    let copied: Vec<_> = target_file
        .records()
        .into_iter()
        .filter(|record| record.get_signature().to_string() == "REFR")
        .collect();
    assert_eq!(copied.len(), 2);
    assert!(copied.iter().all(|record| record.get_is_visible_when_distant()));
    drop(folder);
    xedit_core::container_handler::clear_containers();
}

fn gmst(form_id: u32, editor_id: &[u8], value: f32) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"DATA", &value.to_le_bytes()));
    main_record(b"GMST", 0, form_id, &data)
}

fn reference_change_files() -> Vec<Arc<FileImpl>> {
    // The master's game setting, a plugin that overrides it, and a form ID
    // list that refers to the master's record.
    let mut master = header(1, 3, 0x820, &[]);
    let mut settings = gmst(0x800, b"fMaster\0", 1.5);
    settings.extend(gmst(0x801, b"fOther\0", 3.5));
    master.extend(top_group(b"GMST", &settings));
    let mut list = sub_record(b"EDID", b"MasterList\0");
    list.extend(sub_record(b"LNAM", &0x800u32.to_le_bytes()));
    master.extend(top_group(b"FLST", &main_record(b"FLST", 0, 0x810, &list)));
    let first = {
        let mut data = sub_record(b"EDID", b"FirstList\0");
        data.extend(sub_record(b"LNAM", &0x800u32.to_le_bytes()));
        let mut bytes = header(0, 2, 0x820, &["Skyrim.esm"]);
        bytes.extend(top_group(b"GMST", &gmst(0x800, b"fFirst\0", 2.5)));
        bytes.extend(top_group(b"FLST", &main_record(b"FLST", 0, 0x810, &data)));
        bytes
    };
    let second = {
        let mut data = sub_record(b"EDID", b"SecondList\0");
        data.extend(sub_record(b"LNAM", &0x800u32.to_le_bytes()));
        let mut bytes = header(0, 1, 0x811, &["Skyrim.esm"]);
        bytes.extend(top_group(b"FLST", &main_record(b"FLST", 0, 0x810, &data)));
        bytes
    };
    load_game(
        "sse",
        vec![("Skyrim.esm", master), ("First.esp", first), ("Second.esp", second)],
    )
}

#[test]
fn changes_the_references_of_a_record() {
    let _guard = test_lock();
    let files = reference_change_files();
    let (registry, mut session) = session(GameMode::gmSSE, &files);
    let result = call(
        &registry,
        &mut session,
        "references.change",
        json!({ "form_id": "00000800", "file": "First.esp", "new_form_id": "00000801" }),
    );
    assert_eq!(result["updated"], 2, "{result}");
    let list = files[1]
        .records()
        .into_iter()
        .find(|record| record.get_signature().to_string() == "FLST")
        .expect("the list");
    let entry = list.get_element_by_path("FormIDs\\[0]").expect("the entry");
    assert_eq!(entry.get_native_value().as_ordinal(), Some(0x801));
    // The record itself keeps its FormID.
    let setting = files[0].records()[0].clone();
    assert_eq!(setting.get_load_order_form_id().to_cardinal(), 0x800);
}

#[test]
fn changes_the_references_of_a_batch() {
    let _guard = test_lock();
    let files = reference_change_files();
    let (registry, mut session) = session(GameMode::gmSSE, &files);
    let result = call(
        &registry,
        &mut session,
        "references.change_batch",
        json!({ "file": "First.esp",
                "pairs": [{ "old_form_id": "00000800", "new_form_id": "00000801" }] }),
    );
    assert_eq!(result["pairs"][0]["updated"], 1, "{result}");
    assert_eq!(result["pairs"][0]["renamed"], true);
    let list = files[1]
        .records()
        .into_iter()
        .find(|record| record.get_signature().to_string() == "FLST")
        .expect("the list");
    let entry = list.get_element_by_path("FormIDs\\[0]").expect("the entry");
    assert_eq!(entry.get_native_value().as_ordinal(), Some(0x801));
}

fn leveled_item(form_id: u32, editor_id: &[u8], entries: &[u32]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"OBND", &[0; 12]));
    data.extend(sub_record(b"LVLD", &[0]));
    data.extend(sub_record(b"LVLF", &[1]));
    data.extend(sub_record(b"LLCT", &[entries.len() as u8]));
    for reference in entries {
        let mut entry = Vec::new();
        entry.extend_from_slice(&1u16.to_le_bytes());
        entry.extend_from_slice(&[0, 0]);
        entry.extend_from_slice(&reference.to_le_bytes());
        entry.extend_from_slice(&1u16.to_le_bytes());
        entry.extend_from_slice(&[0, 0]);
        data.extend(sub_record(b"LVLO", &entry));
    }
    main_record(b"LVLI", 0, form_id, &data)
}

#[test]
fn reports_a_circular_leveled_list() {
    let _guard = test_lock();
    let mut master = header(1, 3, 0x810, &[]);
    let mut lists = leveled_item(0x800, b"ListA\0", &[0x801]);
    lists.extend(leveled_item(0x801, b"ListB\0", &[0x800]));
    lists.extend(leveled_item(0x802, b"ListC\0", &[0x800]));
    master.extend(top_group(b"LVLI", &lists));
    let files = load_game("sse", vec![("Skyrim.esm", master)]);
    let (registry, mut session) = session(GameMode::gmSSE, &files);
    let result = call(&registry, &mut session, "leveled_lists.check_circular", json!({}));
    assert_eq!(result["checked"], 3);
    assert_eq!(result["cycles"].as_array().unwrap().len(), 1, "{result}");
    let cycle = result["cycles"][0].as_str().unwrap();
    assert!(cycle.contains("ListA") && cycle.contains("ListB"), "{cycle}");
}

/// A group of Oblivion: the 20-byte header of the games before Skyrim.
fn tes4_group(label: u32, group_type: i32, records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((20 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(&label.to_le_bytes());
    bytes.extend_from_slice(&(group_type as u32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

fn tes4_top_group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    tes4_group(u32::from_le_bytes(*label), 0, records)
}

/// A record of Oblivion: the 20-byte header of the games before Skyrim
/// (signature, size, flags, FormID, VCS1).
fn tes4_record(signature: &[u8; 4], flags: u32, form_id: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&form_id.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn tes4_header(flags: u32, record_count: u32, next_object_id: u32, masters: &[&str]) -> Vec<u8> {
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
    tes4_record(b"TES4", flags, 0, &data)
}

/// A model path of a TES4 record (the Model structure's `MODL`).
fn model_record(signature: &[u8; 4], form_id: u32, editor_id: &[u8], path: &str) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    let mut model = path.as_bytes().to_vec();
    model.push(0);
    data.extend(sub_record(b"MODL", &model));
    tes4_record(signature, 0, form_id, &data)
}

#[test]
fn copies_the_idles_of_a_model_folder() {
    let _guard = test_lock();
    let oblivion = tes4_header(1, 0, 0x800, &[]);
    let mut plugin = tes4_header(0, 2, 0x802, &["Oblivion.esm"]);
    let mut idles = model_record(b"IDLE", 0x0100_0800, b"IdleA\0", "test\\idle_a.nif");
    idles.extend(model_record(b"IDLE", 0x0100_0801, b"IdleB\0", "test\\idle_b.nif"));
    plugin.extend(tes4_top_group(b"IDLE", &idles));
    let target = tes4_header(1, 0, 0x800, &[]);
    let files = load_game(
        "tes4",
        vec![
            ("Oblivion.esm", oblivion),
            ("Plugin.esp", plugin),
            ("Target.esp", target),
        ],
    );
    let (registry, mut session) = session(GameMode::gmTES4, &files);
    let result = call(
        &registry,
        &mut session,
        "records.copy_idles",
        json!({ "path": "test", "new_path": "copy", "to": "Target.esp" }),
    );
    assert_eq!(result["idles"].as_array().unwrap().len(), 2, "{result}");
    let copies: Vec<_> = files[2].records();
    assert_eq!(copies.len(), 2);
    for copy in copies {
        let model = copy.get_element_edit_value("MODL\\MODL");
        assert!(model.starts_with("copy\\"), "{model}");
    }
}

fn script(form_id: u32, editor_id: &[u8]) -> Vec<u8> {
    tes4_record(b"SCPT", 0, form_id, &sub_record(b"EDID", editor_id))
}

fn npc(form_id: u32, editor_id: &[u8], script_id: Option<u32>) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    if let Some(script_id) = script_id {
        data.extend(sub_record(b"SCRI", &script_id.to_le_bytes()));
    }
    tes4_record(b"NPC_", 0, form_id, &data)
}

#[test]
fn fixes_the_bandit_scripts() {
    let _guard = test_lock();
    let mut mmm = tes4_header(1, 8, 0x820, &[]);
    let mut scripts = script(0x800, b"CSNPC\0");
    scripts.extend(script(0x801, b"CSNPCBoss\0"));
    scripts.extend(script(0x802, b"CSNPCBandit\0"));
    scripts.extend(script(0x803, b"CSNPCBanditBoss\0"));
    mmm.extend(tes4_top_group(b"SCPT", &scripts));
    let mut npcs = npc(0x810, b"TestBandit\0", Some(0x800));
    npcs.extend(npc(0x811, b"TestBanditBoss\0", None));
    npcs.extend(npc(0x812, b"AdventurerBandit\0", None));
    npcs.extend(npc(0x813, b"TestRaider\0", Some(0x800)));
    mmm.extend(tes4_top_group(b"NPC_", &npcs));
    let files = load_game("tes4", vec![("Mart's Monster Mod.esm", mmm)]);
    let (registry, mut session) = session(GameMode::gmTES4, &files);
    let result = call(&registry, &mut session, "records.bandit_fix", json!({}));
    assert_eq!(result["changed"].as_array().unwrap().len(), 3, "{result}");
    let changed = |editor_id: &str| {
        let record = files[0]
            .records()
            .into_iter()
            .find(|record| record.get_editor_id() == editor_id)
            .expect("the NPC");
        record
            .get_element_native_value("SCRI")
            .as_ordinal()
            .filter(|value| *value != 0)
            .map_or_else(String::new, |value| format!("{:08X}", value as u32))
    };
    assert_eq!(changed("TestBandit"), "00000802");
    assert_eq!(changed("TestBanditBoss"), "00000803");
    assert_eq!(changed("AdventurerBandit"), "");
    assert_eq!(changed("TestRaider"), "00000802");
}

#[test]
fn changes_the_race_leveled_list_entries() {
    let _guard = test_lock();
    let mut master = tes4_header(1, 4, 0x830, &[]);
    let race = tes4_record(b"RACE", 0, 0x800, &sub_record(b"EDID", b"TestRace\0"));
    master.extend(tes4_top_group(b"RACE", &race));
    let mut lists = tes4_record(b"LVLI", 0, 0x820, &sub_record(b"EDID", b"Loot\0"));
    // The release looks the list up under `EditorID + Race`.
    lists.extend(tes4_record(b"LVLI", 0, 0x821, &sub_record(b"EDID", b"LootTestRace\0")));
    master.extend(tes4_top_group(b"LVLI", &lists));
    let mut data = sub_record(b"EDID", b"Bandit\0");
    data.extend(sub_record(b"RNAM", &0x800u32.to_le_bytes()));
    let mut item = Vec::new();
    item.extend_from_slice(&0x820u32.to_le_bytes());
    item.extend_from_slice(&1i32.to_le_bytes());
    data.extend(sub_record(b"CNTO", &item));
    master.extend(tes4_top_group(b"NPC_", &tes4_record(b"NPC_", 0, 0x810, &data)));
    let files = load_game("tes4", vec![("Oblivion.esm", master)]);
    let (registry, mut session) = session(GameMode::gmTES4, &files);
    let result = call(&registry, &mut session, "records.race_leveled_lists", json!({}));
    assert_eq!(result["changed"].as_array().unwrap().len(), 1, "{result}");
    let npc = files[0]
        .records()
        .into_iter()
        .find(|record| record.get_editor_id() == "Bandit")
        .expect("the NPC");
    let entry = npc.get_element_by_path("Items\\[0]").expect("the item");
    let reference = entry
        .as_container()
        .expect("the item")
        .get_element(0)
        .expect("the reference");
    assert_eq!(
        reference
            .get_native_value()
            .as_ordinal()
            .map(|value| format!("{:08X}", value as u32)),
        Some("00000821".to_owned())
    );
}

fn delta_patch_files(folder: &TempDir) -> (Vec<Arc<FileImpl>>, std::path::PathBuf) {
    set_data_path(&format!("{}\\", folder.display()));
    let oblivion = tes4_header(1, 0, 0x800, &[]);
    std::fs::write(folder.join("Oblivion.esm"), &oblivion).unwrap();
    let old = tes4_header(0, 1, 0x801, &["Oblivion.esm"]);
    let mut old = old;
    old.extend(tes4_top_group(
        b"GMST",
        &tes4_record(b"GMST", 0, 0x800, &{
            let mut data = sub_record(b"EDID", b"fOld\0");
            data.extend(sub_record(b"DATA", &1.5f32.to_le_bytes()));
            data
        }),
    ));
    let newer = tes4_header(0, 0, 0x800, &["Oblivion.esm"]);
    let newer_path = folder.join("Newer.esp");
    std::fs::write(&newer_path, &newer).unwrap();
    let files = load_game("tes4", vec![("Oblivion.esm", oblivion), ("Old.esp", old)]);
    set_data_path(&format!("{}\\", folder.display()));
    (files, newer_path)
}

#[test]
fn creates_a_delta_patch() {
    let _guard = test_lock();
    let folder = TempDir::new(format!("xedit-delta-{}", std::process::id()));
    let (files, newer_path) = delta_patch_files(&folder);
    let (registry, mut session) = session(GameMode::gmTES4, &files);
    let result = call(
        &registry,
        &mut session,
        "files.create_delta_patch",
        json!({ "file": "Old.esp", "newer": newer_path.display().to_string(), "name": "Patch" }),
    );
    let patch = std::path::PathBuf::from(result["patch"].as_str().unwrap());
    assert!(patch.exists(), "{}", patch.display());
    let loaded = xedit_core::interface::files();
    let patch_file = loaded
        .iter()
        .find(|file| file.get_name().eq_ignore_ascii_case("Patch.esu"))
        .expect("the delta patch is loaded");
    assert!(patch_file.get_file_states().contains(FileState::fsIsDeltaPatch));
    assert!(patch_file.get_file_states().contains(FileState::fsIsCompareLoad));
    assert!(
        xedit_core::interface::files()
            .iter()
            .any(|file| file.get_name().eq_ignore_ascii_case("Patch.esu"))
    );
}

#[test]
fn analyzes_a_papyrus_log() {
    let _guard = test_lock();
    let files = reference_change_files();
    let (registry, mut session) = session(GameMode::gmSSE, &files);
    let folder = TempDir::new(format!("xedit-papyrus-{}", std::process::id()));
    let log = folder.join("Papyrus.0.log");
    std::fs::write(
        &log,
        "[10/18/2026 - 10:20:30AM] error: (00000800): the record is broken\n\
         [10/18/2026 - 10:20:31AM] warning: (00000800): careful\n\
         [10/18/2026 - 10:20:32AM] error: (00ABCDEF): unknown form\n\
         [10/18/2026 - 10:20:33AM] error: [None].SomeScript: the save game is broken\n",
    )
    .unwrap();
    let result = call(
        &registry,
        &mut session,
        "logs.analyze",
        json!({ "log": "papyrus", "file": log.display().to_string() }),
    );
    assert_eq!(result["entries"].as_array().unwrap().len(), 2, "{result}");
    let known = &result["entries"][0];
    assert_eq!(known["form_id"], "00000800");
    assert_eq!(known["errors"], 1);
    assert_eq!(known["warnings"], 1);
    assert_eq!(known["record"]["editor_id"], "fMaster");
    let saved = &result["entries"][1];
    assert_eq!(saved["form_id"], "FF000000");
    assert_eq!(saved["load_order"], 255);
    assert_eq!(result["unknown_form_ids"], 1);
    assert_eq!(result["plugins"].as_array().unwrap().len(), 2);
}

#[test]
fn analyzes_a_runtime_script_profiler_log() {
    let _guard = test_lock();
    let files = reference_change_files();
    let (registry, mut session) = session(GameMode::gmSSE, &files);
    let folder = TempDir::new(format!("xedit-profiler-{}", std::process::id()));
    let log = folder.join("RuntimeScriptProfiler.log");
    // The time of the profiler starts at column 18, after the other fields.
    std::fs::write(&log, "00000800\t123456785.0 rest\n00000800\t1234567812.5 rest\n").unwrap();
    let result = call(
        &registry,
        &mut session,
        "logs.analyze",
        json!({ "log": "runtime_script_profiler", "file": log.display().to_string() }),
    );
    let entry = &result["entries"][0];
    assert_eq!(entry["form_id"], "00000800");
    assert_eq!(entry["frames_executed"], 2);
    assert_eq!(entry["total_time_ms"], 17.5);
    assert_eq!(entry["max_execution_time_ms"], 12.5);
    assert_eq!(entry["text"], "");
}
