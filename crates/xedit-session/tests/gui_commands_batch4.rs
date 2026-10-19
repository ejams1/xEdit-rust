// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The command-layer gaps of phase 7 step 1b: the module list of the module
//! selection dialog and the mod group editor (`modules.list`), the signature
//! lists of the filter options dialog (`signatures.list`), the settings file
//! commands (`settings.get`/`settings.set`), the request's compare file
//! (`session.load` with `compare_to`), and the two core callback seams
//! (`wbGetFormID` and `wbGetCellDetailsForWorldspace`) that the GUI dialogs
//! install.

use std::sync::Arc;

use serde_json::{Value, json};
use xedit_core::implementation::{FileBytes, FileImpl, wb_file, wb_file_from_bytes};
use xedit_core::interface::globals::{
    set_creation_club_content_file_name, set_data_path, set_plugins_file_name, test_lock,
};
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{File, GameMode, MainRecord, clear_files};
use xedit_io::temp::TempDir;
use xedit_session::resources::{ResourceOptions, set_options};
use xedit_session::{Registry, Session};

fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

/// A plugin whose file header lists `masters`, with `flags` in its record
/// header.
fn plugin(flags: u32, masters: &[&str]) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&0u32.to_le_bytes());
    hedr.extend_from_slice(&0x0000_0800u32.to_le_bytes());
    let mut data = sub_record(b"HEDR", &hedr);
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        data.extend(sub_record(b"MAST", &name));
        data.extend(sub_record(b"DATA", &[0u8; 8]));
    }
    let mut bytes = b"TES4".to_vec();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&44u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend(data);
    bytes
}

fn call(session: &mut Session, name: &str, params: Value) -> Value {
    Registry::standard()
        .call(session, name, params)
        .unwrap_or_else(|error| panic!("{name}: {error:?}"))
}

fn error(session: &mut Session, name: &str, params: Value) -> xedit_session::CommandError {
    Registry::standard().call(session, name, params).unwrap_err()
}

/// The modules of a Skyrim SE data folder: the game master and an official
/// DLC, an active and an inactive plugin from the plugin list, a light
/// module, a plugin with a missing master, and a ghost.
fn module_setup(test: &str) -> (TempDir, Session) {
    let dir = TempDir::new(format!("xedit-gui4-{test}-{}", std::process::id()));
    for (name, flags, masters) in [
        ("Skyrim.esm", 0x1, &[][..]),
        ("Dawnguard.esm", 0x1, &["Skyrim.esm"][..]),
        ("Active.esp", 0, &["Skyrim.esm"][..]),
        ("Inactive.esp", 0, &["Skyrim.esm"][..]),
        ("Light.esl", 0x200, &["Skyrim.esm"][..]),
        ("Missing.esp", 0, &["MissingMaster.esm"][..]),
        ("Ghost.esp.ghost", 0, &["Skyrim.esm"][..]),
    ] {
        std::fs::write(dir.join(name), plugin(flags, masters)).unwrap();
    }
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    clear_files();
    let mode = xedit_session::dump::setup_game_for_edit("sse").unwrap();
    assert_eq!(mode, GameMode::gmSSE);
    xedit_session::save::apply_edit_settings(mode);
    let files: Vec<Arc<FileImpl>> = ["Skyrim.esm", "Dawnguard.esm", "Active.esp", "Inactive.esp"]
        .iter()
        .enumerate()
        .map(|(order, name)| wb_file(dir.join(name).to_str().unwrap(), order as i32, FileStates::empty()).unwrap())
        .collect();
    set_data_path(&format!("{}\\", dir.display()));
    std::fs::write(
        dir.join("plugins.txt"),
        "*Active.esp\r\nInactive.esp\r\n*Missing.esp\r\n",
    )
    .unwrap();
    set_plugins_file_name(dir.join("plugins.txt").to_str().unwrap());
    // No Creation Club list: only the game master and the official DLC are
    // implicit actives.
    set_creation_club_content_file_name("");
    xedit_session::modgroups::set_file_options(
        Some(dir.join("global.modgroups").to_str().unwrap()),
        Some(dir.join("settings.ini").to_str().unwrap()),
    );
    let mut session = Session::with_files(mode, files);
    session.allow_edit(true);
    (dir, session)
}

fn module<'a>(response: &'a Value, name: &str) -> &'a Value {
    response["modules"]
        .as_array()
        .expect("the modules")
        .iter()
        .find(|module| module["name"] == name)
        .unwrap_or_else(|| panic!("{name} is not listed: {response}"))
}

#[test]
fn the_module_list_holds_the_dialog_facts_in_load_order() {
    let _guard = test_lock();
    let (_dir, mut session) = module_setup("list");
    let response = call(&mut session, "modules.list", json!({}));
    let modules = response["modules"].as_array().expect("the modules");
    let names: Vec<&str> = modules.iter().map(|module| module["name"].as_str().unwrap()).collect();
    // `wbModulesByLoadOrder`: the game master, the official DLC, the ESM
    // module, then the plugin list's modules and the one it does not name.
    assert_eq!(
        names,
        [
            "Skyrim.esm",
            "Dawnguard.esm",
            "Light.esl",
            "Active.esp",
            "Inactive.esp",
            "Missing.esp",
            "Ghost.esp"
        ]
    );
    // The dialogs list the valid modules only: the game's executable is a
    // module of the data folder but not a plugin.
    assert!(modules.iter().all(|module| module["valid"] == true));
    assert!(modules.iter().all(|module| module["is_hardcoded"] == false));

    let skyrim = module(&response, "Skyrim.esm");
    assert_eq!(skyrim["is_esm"], true);
    assert_eq!(skyrim["has_esm_flag"], true);
    assert_eq!(skyrim["active"], true);
    assert_eq!(skyrim["loaded"], true);
    assert_eq!(skyrim["load_order"], 0);
    assert!(skyrim["file_id"].is_string(), "{skyrim}");
    assert_eq!(skyrim["official_index"], i32::MIN);
    assert!(skyrim["size"].as_u64().unwrap() > 0);
    assert!(skyrim["path"].as_str().unwrap().ends_with("Skyrim.esm"));

    // The light module: no ESM bit in its header, the `.esl` extension
    // makes it an ESM and the light column's flag.
    let light = module(&response, "Light.esl");
    assert_eq!(light["is_light"], true);
    assert_eq!(light["has_esm_flag"], false);
    assert_eq!(light["has_esm_extension"], true);
    assert_eq!(light["is_esm"], true);

    // The plugin list's active mark, and the module the list names but whose
    // master is missing (`mfMastersMissing` is not active).
    assert_eq!(module(&response, "Active.esp")["active_in_plugins_txt"], true);
    assert_eq!(module(&response, "Inactive.esp")["active"], false);
    let missing = module(&response, "Missing.esp");
    assert_eq!(missing["masters_missing"], true);
    assert_eq!(missing["active"], false);
    assert_eq!(missing["masters"], json!(["MissingMaster.esm"]));

    // A ghost keeps its file name, without the extension the dialogs show.
    let ghost = module(&response, "Ghost.esp");
    assert_eq!(ghost["is_ghost"], true);
    assert_eq!(ghost["original_name"], "Ghost.esp.ghost");
    assert!(ghost["path"].as_str().unwrap().ends_with("Ghost.esp.ghost"));

    // `FilteredByFlag(mfActive)`: the game master, the official DLC and the
    // plugin list's active module.
    let response = call(&mut session, "modules.list", json!({ "active": true }));
    let names: Vec<&str> = response["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|module| module["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Skyrim.esm", "Dawnguard.esm", "Active.esp"]);

    // `not mfMastersMissing`.
    let response = call(&mut session, "modules.list", json!({ "masters_present": true }));
    assert!(
        response["modules"]
            .as_array()
            .unwrap()
            .iter()
            .all(|module| module["name"] != "Missing.esp")
    );

    // The whole data folder: the game's executable is a module of its own,
    // invalid and not a plugin (`mfIsHardcoded`).
    let response = call(&mut session, "modules.list", json!({ "valid": false }));
    let executable = module(&response, "SkyrimSE.exe");
    assert_eq!(executable["is_hardcoded"], true);
    assert_eq!(executable["valid"], false);
    assert_eq!(executable["official_index"], i32::MIN + 1);
    assert!(response["modules"].as_array().unwrap().len() > 7);
}

#[test]
fn the_signature_lists_follow_the_definitions() {
    let _guard = test_lock();
    let (_dir, mut session) = module_setup("signatures");
    let response = call(&mut session, "signatures.list", json!({}));
    let records = response["records"].as_array().expect("the record signatures");
    let base = response["base_records"].as_array().expect("the base record signatures");
    assert!(records.len() > 100, "{} signatures", records.len());
    assert!(!base.is_empty());
    assert!(base.len() < records.len());

    // `SIGNATURE - Name` with `ReferenceSignatureCount`.
    let weapon = records.iter().find(|entry| entry["signature"] == "WEAP").expect("WEAP");
    assert_eq!(weapon["name"], "Weapon");
    assert_eq!(weapon["text"], "WEAP - Weapon");
    assert!(weapon["reference_signature_count"].as_i64().unwrap() > 0);
    assert!(base.iter().any(|entry| entry["signature"] == "WEAP"));

    // A game setting is no base of a reference, so only the record list
    // holds it.
    assert!(records.iter().any(|entry| entry["signature"] == "GMST"));
    assert!(!base.iter().any(|entry| entry["signature"] == "GMST"));

    // Both lists are sorted by the entry text (`Sorted := True`), and the
    // base list is exactly the records with a reference signature.
    let texts: Vec<&str> = records.iter().map(|entry| entry["text"].as_str().unwrap()).collect();
    let mut sorted = texts.clone();
    sorted.sort_by(|a, b| xedit_loadorder::mod_groups::compare_text(a, b));
    assert_eq!(texts, sorted);
    let expected: Vec<&str> = records
        .iter()
        .filter(|entry| entry["reference_signature_count"].as_i64().unwrap() > 0)
        .map(|entry| entry["signature"].as_str().unwrap())
        .collect();
    let listed: Vec<&str> = base.iter().map(|entry| entry["signature"].as_str().unwrap()).collect();
    assert_eq!(listed, expected);
}

fn settings_setup(test: &str) -> (TempDir, Session) {
    let dir = TempDir::new(format!("xedit-settings-{test}-{}", std::process::id()));
    let settings = dir.join("xedit.ini");
    std::fs::write(
        &settings,
        "; the GUI's settings\r\n\
         [frmMain]\r\n\
         Left=100\r\n\
         Top=200\r\n\
         Width=1000\r\n\
         Height=800\r\n\
         WindowState=1\r\n\
         pnlNavWidth=250\r\n\
         vstNavColumnWidth0=300\r\n\
         \r\n\
         [ColorConflictThis]\r\n\
         ctConflictWins=-16777216\r\n\
         \r\n\
         [Nav]\r\n\
         FilesSort=1\r\n\
         INFOSort=0\r\n\
         \r\n\
         [View]\r\n\
         HodeNoConflict=1\r\n\
         StickViewNodeLabel=\r\n",
    )
    .unwrap();
    xedit_session::modgroups::set_file_options(None, Some(settings.to_str().unwrap()));
    let mut session = Session::default();
    session.allow_edit(true);
    (dir, session)
}

#[test]
fn the_settings_file_reads_as_the_gui_reads_it() {
    let _guard = test_lock();
    let (_dir, mut session) = settings_setup("read");

    // The section names, in file order, when no section is named.
    let response = call(&mut session, "settings.get", json!({}));
    assert_eq!(response["exists"], true);
    assert!(response["settings"].as_str().unwrap().ends_with("xedit.ini"));
    assert_eq!(
        response["sections"],
        json!(["frmMain", "ColorConflictThis", "Nav", "View"])
    );
    assert_eq!(response["values"], json!([]));

    // A form's window placement and splitter keys, `ReadInteger`.
    let response = call(
        &mut session,
        "settings.get",
        json!({ "section": "frmMain", "value_type": "integer" }),
    );
    let values = response["values"].as_array().expect("the keys");
    let value_of = |key: &str| {
        values
            .iter()
            .find(|entry| entry["key"] == key)
            .unwrap_or_else(|| panic!("{key} is not listed"))
            .clone()
    };
    assert_eq!(value_of("Left")["value"], 100);
    assert_eq!(value_of("Left")["raw"], "100");
    assert_eq!(value_of("WindowState")["value"], 1);
    assert_eq!(value_of("pnlNavWidth")["value"], 250);
    assert_eq!(value_of("vstNavColumnWidth0")["value"], 300);
    assert_eq!(values.len(), 7);

    // `[ColorConflictThis]` holds the `TColor` of the enum name.
    let response = call(
        &mut session,
        "settings.get",
        json!({ "section": "ColorConflictThis", "key": "ctConflictWins", "value_type": "integer" }),
    );
    assert_eq!(response["values"][0]["value"], -16777216);

    // `[Nav]` and `[View]`, `ReadInteger` and `ReadBool`.
    let response = call(
        &mut session,
        "settings.get",
        json!({ "section": "Nav", "key": "FilesSort", "value_type": "integer" }),
    );
    assert_eq!(response["values"][0]["value"], 1);
    let response = call(
        &mut session,
        "settings.get",
        json!({ "section": "View", "key": "HodeNoConflict", "value_type": "bool" }),
    );
    assert_eq!(response["values"][0]["value"], true);
    // Without a type the value is the raw text, as `ReadString` gives it.
    let response = call(
        &mut session,
        "settings.get",
        json!({ "section": "View", "key": "StickViewNodeLabel" }),
    );
    assert_eq!(response["values"][0]["raw"], "");
    assert!(response["values"][0].get("value").is_none());

    // A missing key reads the default, as `StrToIntDef` does.
    let response = call(
        &mut session,
        "settings.get",
        json!({ "section": "frmMain", "key": "Missing", "value_type": "integer", "default": 7 }),
    );
    assert_eq!(response["values"][0]["value"], 7);
    let response = call(
        &mut session,
        "settings.get",
        json!({ "section": "NoSuchSection", "value_type": "bool", "default": true }),
    );
    assert_eq!(response["values"], json!([]));

    let refused = error(&mut session, "settings.get", json!({ "value_type": "date" }));
    assert_eq!(refused.code, "invalid_params");
}

#[test]
fn the_settings_file_writes_as_the_gui_writes_it() {
    let _guard = test_lock();
    let (dir, mut session) = settings_setup("write");
    let path = dir.join("xedit.ini");

    // A dry run reports the change without writing.
    let response = call(
        &mut session,
        "settings.set",
        json!({ "section": "View", "key": "LastUsedFilter", "value": "MyFilter", "dry_run": true }),
    );
    assert_eq!(response["changed"], true);
    assert_eq!(response["written"], false);
    assert!(response["old"].is_null());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("LastUsedFilter"));

    // The write: a new key of an existing section, a changed key, a boolean
    // as `1`/`0`, and a float as `FloatToStr` in a section added at the end.
    let response = call(
        &mut session,
        "settings.set",
        json!({ "section": "View", "key": "LastUsedFilter", "value": "MyFilter" }),
    );
    assert_eq!(response["written"], true);
    let response = call(
        &mut session,
        "settings.set",
        json!({ "section": "frmMain", "key": "WindowState", "value": 2, "value_type": "integer" }),
    );
    assert_eq!(response["old"], "1");
    assert_eq!(response["new"], "2");
    assert_eq!(response["changed"], true);
    let response = call(
        &mut session,
        "settings.set",
        json!({ "section": "View", "key": "IncludeScriptsFromSubDir", "value": true, "value_type": "bool" }),
    );
    assert_eq!(response["new"], "1");
    let response = call(
        &mut session,
        "settings.set",
        json!({ "section": "frmRichEdit", "key": "Zoom", "value": 2.5, "value_type": "float" }),
    );
    assert_eq!(response["new"], "2.5");

    // The file is written as `TMemIniFile.UpdateFile` writes it: the
    // comment dropped, the sections in order with a blank line after each,
    // the new section last.
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("; the GUI's settings"), "{text}");
    assert!(text.contains("LastUsedFilter=MyFilter"), "{text}");
    assert!(text.contains("WindowState=2"), "{text}");
    assert!(text.contains("IncludeScriptsFromSubDir=1"), "{text}");
    assert!(text.contains("[frmRichEdit]\r\nZoom=2.5\r\n\r\n"), "{text}");
    assert!(text.find("[frmMain]").unwrap() < text.find("[frmRichEdit]").unwrap());

    // The write needs --edit unless it is a dry run.
    session.allow_edit(false);
    let refused = error(
        &mut session,
        "settings.set",
        json!({ "section": "View", "key": "LastUsedFilter", "value": "Other" }),
    );
    assert_eq!(refused.code, "edit_required");
    let response = call(
        &mut session,
        "settings.set",
        json!({ "section": "View", "key": "LastUsedFilter", "value": "Other", "dry_run": true }),
    );
    assert_eq!(response["changed"], true);
    assert_eq!(response["written"], false);
    session.allow_edit(true);

    // A settings path in a folder that is not there yet is created, as
    // FormCreate's ForceDirectories does.
    let nested = dir.join("new").join("deeper").join("xedit.ini");
    xedit_session::modgroups::set_file_options(None, Some(nested.to_str().unwrap()));
    let response = call(
        &mut session,
        "settings.set",
        json!({ "section": "View", "key": "LastUsedFilter", "value": "Nested" }),
    );
    assert_eq!(response["written"], true);
    // A CRLF after each line, as `TStrings.SaveToFile` writes them.
    let crlf: String = [char::from(13), char::from(10)].iter().collect();
    let expected = ["[View]", "LastUsedFilter=Nested", "", ""].join(&crlf);
    assert_eq!(std::fs::read_to_string(&nested).unwrap(), expected);

    // A value of the wrong kind is refused rather than written.
    let refused = error(
        &mut session,
        "settings.set",
        json!({ "section": "View", "key": "FilesSort", "value": "many", "value_type": "integer" }),
    );
    assert_eq!(refused.code, "invalid_params");
}

/// A data folder with a master, a plugin and a compare file next to them,
/// and the resources of a session of their own.
fn compare_setup(test: &str) -> (TempDir, Vec<String>) {
    let dir = TempDir::new(format!("xedit-compare-{test}-{}", std::process::id()));
    let data = dir.join("Data");
    let my_games = dir.join("My Games");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&my_games).unwrap();
    for (name, flags, masters) in [
        ("Skyrim.esm", 0x1, &[][..]),
        ("Base.esp", 0, &["Skyrim.esm"][..]),
        ("Mod.esp", 0, &["Skyrim.esm"][..]),
        ("Compare.esp", 0, &["Skyrim.esm"][..]),
        ("Compare2.esp", 0, &["Skyrim.esm"][..]),
        ("Other.esp", 0, &["Skyrim.esm"][..]),
    ] {
        std::fs::write(data.join(name), plugin(flags, masters)).unwrap();
    }
    set_options(ResourceOptions {
        my_games: Some(format!("{}\\", my_games.display())),
        ..Default::default()
    });
    let plugins = ["Base.esp", "Mod.esp"]
        .iter()
        .map(|name| data.join(name).to_str().unwrap().to_owned())
        .collect();
    (dir, plugins)
}

#[test]
fn a_session_load_takes_the_requests_compare_file() {
    let _guard = test_lock();
    let (dir, plugins) = compare_setup("load");
    let data = dir.join("Data");
    let mut session = Session::load("sse", &plugins).expect("the first load");
    session.allow_edit(true);

    // The request's compare file: it loads with the last plugin's load
    // order and is compared with it.
    let compare = data.join("Compare.esp");
    let response = call(
        &mut session,
        "session.load",
        json!({ "plugins": plugins, "compare_to": compare.to_str().unwrap() }),
    );
    assert_eq!(response["compare_to"], compare.to_str().unwrap());
    assert_eq!(response["files"].as_array().unwrap().len(), 3);
    let info = call(&mut session, "session.info", json!({}));
    let files: Vec<&str> = info["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file.as_str().unwrap())
        .collect();
    assert!(files.contains(&"Compare.esp"), "{files:?}");
    let compare_file = xedit_core::interface::files()
        .into_iter()
        .find(|file| file.get_name() == "Compare.esp")
        .expect("the compare load");
    assert_eq!(compare_file.get_compare_to_name().as_deref(), Some("Mod.esp"));
    // The compare file takes the load order of the plugin it compares to
    // (Skyrim.esm is the master of both plugins, then Base.esp, Mod.esp; the
    // absolute slot counts the loads of the whole test process).
    let compared_to = wb_file(&plugins[1], i32::MAX, FileStates::empty()).expect("the last plugin");
    assert_eq!(compare_file.get_load_order(), compared_to.get_load_order());

    // The request's file wins over the process's `--compare-to`, which the
    // load consumes (it does not apply to this load or a later one).
    let compare2 = data.join("Compare2.esp");
    xedit_session::commands::set_compare_to_on_load(Some(data.join("Other.esp").to_str().unwrap()));
    let response = call(
        &mut session,
        "session.load",
        json!({ "plugins": plugins, "compare_to": compare2.to_str().unwrap() }),
    );
    assert_eq!(response["compare_to"], compare2.to_str().unwrap());
    let info = call(&mut session, "session.info", json!({}));
    let files: Vec<&str> = info["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file.as_str().unwrap())
        .collect();
    assert!(
        files.contains(&"Compare2.esp") && !files.contains(&"Other.esp"),
        "{files:?}"
    );
    let response = call(&mut session, "session.load", json!({ "plugins": plugins }));
    assert!(response.get("compare_to").is_none(), "{response}");

    // A compare file outside the folder of the last plugin is refused, as
    // the masters would resolve to other files.
    let elsewhere = dir.join("Elsewhere.esp");
    std::fs::write(&elsewhere, plugin(0, &["Skyrim.esm"])).unwrap();
    let refused = error(
        &mut session,
        "session.load",
        json!({ "plugins": plugins, "compare_to": elsewhere.to_str().unwrap() }),
    );
    assert_eq!(refused.code, "load_failed");
    assert!(
        refused.message.contains("has to sit in the folder"),
        "{}",
        refused.message
    );
}

// ----- the core callback seams ------------------------------------------

/// A plugin with a WRLD (whose children group is empty), a STAT group with
/// one record, and the world record's FormID.
fn world_plugin() -> (Vec<u8>, u32) {
    let world_form_id: u32 = 0x0000_0800;

    let mut world = sub_record(b"EDID", b"TestWorld\0");
    world.extend(sub_record(b"DATA", &[0; 8]));
    let mut world_record = b"WRLD".to_vec();
    world_record.extend_from_slice(&(world.len() as u32).to_le_bytes());
    world_record.extend_from_slice(&0u32.to_le_bytes());
    world_record.extend_from_slice(&world_form_id.to_le_bytes());
    world_record.extend_from_slice(&0u32.to_le_bytes());
    world_record.extend_from_slice(&44u16.to_le_bytes());
    world_record.extend_from_slice(&0u16.to_le_bytes());
    world_record.extend(world);
    // The world's children group (type 1, the label is the world's FormID),
    // which an add of a world cell places its block and sub-block in.
    let mut children = b"GRUP".to_vec();
    children.extend_from_slice(&24u32.to_le_bytes());
    children.extend_from_slice(&world_form_id.to_le_bytes());
    children.extend_from_slice(&1i32.to_le_bytes());
    children.extend_from_slice(&0u32.to_le_bytes());
    children.extend_from_slice(&0u32.to_le_bytes());
    world_record.extend(children);

    let mut world_group = b"GRUP".to_vec();
    world_group.extend_from_slice(&((24 + world_record.len()) as u32).to_le_bytes());
    world_group.extend_from_slice(b"WRLD");
    world_group.extend_from_slice(&0i32.to_le_bytes());
    world_group.extend_from_slice(&0u32.to_le_bytes());
    world_group.extend_from_slice(&0u32.to_le_bytes());
    world_group.extend(world_record);

    // A top-level STAT group (type 0) with one record, which an add of a
    // STAT joins.
    let mut stat = sub_record(b"EDID", b"TestStatic\0");
    stat.extend(sub_record(b"MODL", b"test\\stat.nif\0"));
    let mut stat_record = b"STAT".to_vec();
    stat_record.extend_from_slice(&(stat.len() as u32).to_le_bytes());
    stat_record.extend_from_slice(&0u32.to_le_bytes());
    stat_record.extend_from_slice(&0x0000_0900u32.to_le_bytes());
    stat_record.extend_from_slice(&0u32.to_le_bytes());
    stat_record.extend_from_slice(&44u16.to_le_bytes());
    stat_record.extend_from_slice(&0u16.to_le_bytes());
    stat_record.extend(stat);
    let mut stat_group = b"GRUP".to_vec();
    stat_group.extend_from_slice(&((24 + stat_record.len()) as u32).to_le_bytes());
    stat_group.extend_from_slice(b"STAT");
    stat_group.extend_from_slice(&0i32.to_le_bytes());
    stat_group.extend_from_slice(&0u32.to_le_bytes());
    stat_group.extend_from_slice(&0u32.to_le_bytes());
    stat_group.extend(stat_record);

    let header = plugin(0x1, &[]);
    let mut bytes = header;
    bytes.extend(stat_group);
    bytes.extend(world_group);
    (bytes, world_form_id)
}

/// The fixture plugin of the seams.
fn load_world() -> Arc<FileImpl> {
    let (bytes, _) = world_plugin();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    let _ = xedit_session::dump::setup_game_for_edit("sse").unwrap();
    xedit_core::interface::globals::set_edit_allowed(true);
    xedit_core::interface::globals::set_allow_internal_edit(true);
    clear_files();
    wb_file_from_bytes("World.esp", 0, FileStates::empty(), FileBytes::Owned(bytes)).expect("the fixture plugin")
}

/// The world's children group: a group of type 1 next to the record in its
/// group (the child groups are siblings of the record, as upstream's are).
fn children_group(record: &xedit_core::interface::ElementRef) -> xedit_core::interface::ElementRef {
    let container = record.get_container().expect("the group of the record");
    let container = container.as_container().expect("a group");
    (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .find(|element| {
            element
                .as_element_impl()
                .and_then(|element| element.group_record_impl())
                .is_some_and(|group| group.group_type() == 1)
        })
        .expect("the children group")
}

#[test]
fn the_worldspace_cell_seam_defaults_to_the_silent_error() {
    let _guard = test_lock();
    let file = load_world();
    assert!(!xedit_core::interface::globals::has_get_cell_details_for_worldspace_callback());
    let world = file
        .records()
        .into_iter()
        .find(|record| record.get_signature().to_string() == "WRLD")
        .expect("the world record");
    let world: xedit_core::interface::ElementRef = world;
    let group = children_group(&world);
    // Without the callback a world cell without parameters fails with the
    // silent add's message, as it did before the seam.
    let error = match group.as_container().unwrap().add("CELL", false) {
        Err(error) => error,
        Ok(_) => panic!("the parameters are required"),
    };
    assert!(error.contains("To add a Worldspace CELL silently"), "{error}");

    // With a callback installed the dialog's answer is used, and a
    // cancelled dialog adds nothing.
    use std::sync::atomic::{AtomicUsize, Ordering};
    use xedit_core::interface::globals::{
        GetCellDetailsForWorldspaceCallback, set_get_cell_details_for_worldspace_callback,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let callback: GetCellDetailsForWorldspaceCallback = Arc::new(move |worldspace| {
        assert_eq!(worldspace.get_signature().to_string(), "WRLD");
        counter.fetch_add(1, Ordering::Relaxed);
        Some((false, 3, -2))
    });
    set_get_cell_details_for_worldspace_callback(Some(callback));
    let added = group
        .as_container()
        .unwrap()
        .add("CELL", false)
        .expect("the dialog answers")
        .expect("the cell");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let record = added
        .as_element_impl()
        .and_then(|element| element.main_record_impl())
        .expect("a record");
    assert!(!record.get_is_persistent());
    assert_eq!(record.get_grid_cell(), Some((3, -2)));

    // A cancelled dialog (`None`) adds nothing and reports no error.
    let cancelled: GetCellDetailsForWorldspaceCallback = Arc::new(|_| None);
    set_get_cell_details_for_worldspace_callback(Some(cancelled));
    let added = group.as_container().unwrap().add("CELL", false).expect("no error");
    assert!(added.is_none());
    set_get_cell_details_for_worldspace_callback(None);
}

#[test]
fn the_form_id_seam_asks_the_installed_callback() {
    let _guard = test_lock();
    let file = load_world();
    // A record to add to: the STAT group, whose group type takes a new STAT.
    let stat = file
        .records()
        .into_iter()
        .find(|record| record.get_signature().to_string() == "STAT")
        .expect("the static record");
    let stat: xedit_core::interface::ElementRef = stat;
    let group = stat.get_container().expect("the group of the record");
    let group = group.as_container().expect("a group");

    // The default: no callback, a new record takes a new FormID.
    assert!(!xedit_core::interface::globals::has_get_form_id_callback());
    let before = file.get_next_object_id();
    let added = group.add("STAT", false).expect("the add").expect("the record");
    let record = added
        .as_element_impl()
        .and_then(|element| element.main_record_impl())
        .expect("a record");
    assert_eq!(record.get_signature().to_string(), "STAT");
    assert_eq!(record.get_form_id().file_id(), file.get_file_file_id());
    assert_ne!(file.get_next_object_id(), before, "the FormID came from the file");

    // An installed callback names the FormID; it is given the container the
    // record is added to (upstream's `wbGetFormID(Self)`).
    use xedit_core::interface::globals::{GetFormIDCallback, set_get_form_id_callback};
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = seen.clone();
    let callback: GetFormIDCallback = Arc::new(move |element| {
        recorder.lock().unwrap().push(element.get_name());
        // A load order FormID of the file itself (`$00000ABC`).
        xedit_core::interface::FormID::from_cardinal(0x0000_0ABC)
    });
    set_get_form_id_callback(Some(callback));
    let added = group.add("STAT", false).expect("the add").expect("the record");
    let record = added
        .as_element_impl()
        .and_then(|element| element.main_record_impl())
        .expect("a record");
    assert_eq!(record.get_form_id().to_string(false), "00000ABC");
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert!(!seen[0].is_empty(), "the callback saw the element");

    // A callback that returns a null FormID (the dialog was cancelled)
    // adds nothing.
    let cancelled: GetFormIDCallback = Arc::new(|_| xedit_core::interface::FormID::null());
    set_get_form_id_callback(Some(cancelled));
    let added = group.add("STAT", false).expect("no error");
    assert!(added.is_none());
    set_get_form_id_callback(None);
}
