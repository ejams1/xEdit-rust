// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The third batch of issue #78: the module selection presets and BSArchPro's
//! asset editor and list.

use std::sync::Arc;

use serde_json::{Value, json};
use xedit_core::implementation::{FileImpl, wb_file};
use xedit_core::interface::globals::{
    set_creation_club_content_file_name, set_data_path, set_plugins_file_name, test_lock,
};
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{GameMode, clear_files};
use xedit_io::temp::TempDir;
use xedit_session::{Registry, Session};

fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

/// A plugin whose header lists `masters`.
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

/// A data folder of Skyrim with `Active.esp` in the plugin list, and xEdit's
/// settings and mod group files pointing into it.
fn module_setup(test: &str) -> (TempDir, Session) {
    let dir = TempDir::new(format!("xedit-modules-{test}-{}", std::process::id()));
    for (name, flags) in [
        ("Skyrim.esm", 1),
        ("Dawnguard.esm", 1),
        ("Active.esp", 0),
        ("Inactive.esp", 0),
    ] {
        let masters: &[&str] = if name == "Skyrim.esm" { &[] } else { &["Skyrim.esm"] };
        std::fs::write(dir.join(name), plugin(flags, masters)).unwrap();
    }
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    clear_files();
    xedit_session::dump::setup_game_for_edit("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let files: Vec<Arc<FileImpl>> = ["Skyrim.esm", "Dawnguard.esm", "Active.esp", "Inactive.esp"]
        .iter()
        .map(|name| wb_file(dir.join(name).to_str().unwrap(), i32::MAX, FileStates::empty()).unwrap())
        .collect();
    set_data_path(&format!("{}\\", dir.display()));
    std::fs::write(dir.join("plugins.txt"), "*Active.esp\r\n").unwrap();
    set_plugins_file_name(dir.join("plugins.txt").to_str().unwrap());
    set_creation_club_content_file_name(dir.join("missing.ccc").to_str().unwrap());
    xedit_session::modgroups::set_file_options(
        Some(dir.join("global.modgroups").to_str().unwrap()),
        Some(dir.join("settings.ini").to_str().unwrap()),
    );
    let mut session = Session::with_files(GameMode::gmSSE, files);
    session.allow_edit(true);
    (dir, session)
}

#[test]
fn module_presets_load_save_and_delete_as_the_selection_dialog() {
    let _guard = test_lock();
    let (dir, mut session) = module_setup("presets");
    // The official Dawnguard is active too; Inactive.esp is not.
    assert_eq!(
        call(&mut session, "modules.presets", json!({}))["modules"],
        json!(["Skyrim.esm", "Dawnguard.esm", "Active.esp"])
    );
    assert_eq!(
        call(&mut session, "modules.presets", json!({}))["presets"],
        json!(["<plugins.txt>"])
    );
    let saved = call(&mut session, "modules.preset_save", json!({ "preset": "Mine" }));
    assert_eq!(saved["written"], json!(true));
    assert_eq!(saved["value"], json!("Skyrim.esm,Dawnguard.esm,Active.esp"));
    assert_eq!(
        std::fs::read_to_string(dir.join("settings.ini")).unwrap(),
        "[frmModuleSelect_ActiveModules_Presets]\r\nMine=Skyrim.esm,Dawnguard.esm,Active.esp\r\n\r\n"
    );
    assert_eq!(
        call(&mut session, "modules.presets", json!({}))["presets"],
        json!(["<plugins.txt>", "Mine"])
    );
    // `PresetLoad` checks the preset's names; `SimulateLoad` filters the
    // checked valid modules in load order.
    let mine = call(&mut session, "modules.preset_load", json!({ "preset": "Mine" }));
    assert_eq!(mine["preset"], json!("Mine"));
    assert_eq!(mine["modules"], json!(["Skyrim.esm", "Dawnguard.esm", "Active.esp"]));
    assert_eq!(mine["preset_differs"], json!(false));
    assert_eq!(mine["error"], Value::Null);
    // A save from another check state answers a preset that names other
    // modules; `<plugins.txt>` ignores the preset and checks the modules the
    // plugin list activates.
    let other = call(
        &mut session,
        "modules.preset_save",
        json!({ "preset": "Other", "modules": ["Active.esp", "Inactive.esp"] }),
    );
    assert_eq!(other["value"], json!("Active.esp,Inactive.esp"));
    let loaded = call(&mut session, "modules.preset_load", json!({ "preset": "Other" }));
    assert_eq!(loaded["modules"], json!(["Active.esp", "Inactive.esp"]));
    let list = call(
        &mut session,
        "modules.preset_load",
        json!({ "preset": "<plugins.txt>" }),
    );
    assert_eq!(list["preset"], json!(""));
    assert_eq!(list["modules"], json!(["Skyrim.esm", "Dawnguard.esm", "Active.esp"]));
    assert_eq!(list["preset_differs"], json!(false));
    // An existing preset is only overwritten when the question is answered;
    // after a modal load the default is No.
    let no = call(
        &mut session,
        "modules.preset_save",
        json!({ "preset": "Mine", "after_modal": true }),
    );
    assert_eq!(no["written"], json!(false));
    let yes = call(
        &mut session,
        "modules.preset_save",
        json!({ "preset": "Mine", "after_modal": true, "confirm": true, "modules": ["Inactive.esp"] }),
    );
    assert_eq!(yes["written"], json!(true));
    assert_eq!(yes["value"], json!("Inactive.esp"));
    // The action is disabled (and the command refuses) for names it can not
    // write and for an empty selection.
    assert_eq!(
        error(&mut session, "modules.preset_save", json!({ "preset": "Bad=Name" })).code,
        "invalid_params"
    );
    assert_eq!(
        error(&mut session, "modules.preset_save", json!({ "preset": "<Bad>" })).code,
        "invalid_params"
    );
    assert_eq!(
        error(
            &mut session,
            "modules.preset_save",
            json!({ "preset": "Empty", "modules": [] })
        )
        .code,
        "invalid_params"
    );
    // A dry run reports the write but leaves the settings file alone.
    let dry = call(
        &mut session,
        "modules.preset_save",
        json!({ "preset": "Dry", "dry_run": true }),
    );
    assert_eq!(dry["written"], json!(true));
    assert!(
        !std::fs::read_to_string(dir.join("settings.ini"))
            .unwrap()
            .contains("Dry")
    );
    // Unknown names are errors; the question of a delete defaults to Yes.
    assert_eq!(
        error(&mut session, "modules.preset_load", json!({ "preset": "Nope" })).code,
        "unknown_preset"
    );
    let kept = call(
        &mut session,
        "modules.preset_delete",
        json!({ "preset": "Mine", "confirm": false }),
    );
    assert_eq!(kept["deleted"], json!(false));
    let deleted = call(&mut session, "modules.preset_delete", json!({ "preset": "Mine" }));
    assert_eq!(deleted["deleted"], json!(true));
    assert_eq!(
        error(
            &mut session,
            "modules.preset_delete",
            json!({ "preset": "<plugins.txt>" })
        )
        .code,
        "invalid_params"
    );
    assert_eq!(
        call(&mut session, "modules.presets", json!({}))["presets"],
        json!(["<plugins.txt>", "Other"])
    );
}

fn bsarch_session() -> Session {
    let mut session = Session::with_files(GameMode::gmSSE, Vec::new());
    session.allow_edit(true);
    session
}

fn write_list(session: &mut Session, dir: &TempDir, name: &str, assets: Value) -> String {
    let list = dir.join(name);
    let result = call(
        session,
        "bsarchpro.list_save",
        json!({ "list": list.to_str().unwrap(), "assets": assets }),
    );
    result["list"].as_str().unwrap().to_owned()
}

fn asset_names(session: &mut Session, list: &str) -> Vec<String> {
    call(session, "bsarchpro.list_load", json!({ "list": list }))["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|asset| asset[1].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn saves_and_loads_an_asset_list_as_the_gui() {
    let _guard = test_lock();
    let dir = TempDir::new(format!("xedit-bsarch-list-{}", std::process::id()));
    let mut session = bsarch_session();
    let list = dir.join("list");
    let saved = call(
        &mut session,
        "bsarchpro.list_save",
        json!({
            "list": list.to_str().unwrap(),
            "packing": { "ArchiveType": 5, "ArchiveTarget": 1, "ArchiveFileName": "Data.ba2",
                         "ArchiveFlags": 3, "FileFlags": 4, "AutodetectFlags": false,
                         "MultiThreaded": false, "SharedData": false, "Split": 2,
                         "CompressionType": "zlib" },
            "assets": [[1, "meshes\\a.nif"], [0, "sound\\b.wav", "src\\b.wav", "Source.ba2"]]
        }),
    );
    // `mniSaveListClick` gives the path the .bsarch extension.
    let path = format!("{}.bsarch", list.display());
    assert_eq!(saved["list"], json!(path));
    assert_eq!(saved["assets"], json!(2));
    assert_eq!(saved["written"], json!(true));
    assert!(list.with_extension("bsarch").exists());
    let loaded = call(&mut session, "bsarchpro.list_load", json!({ "list": path }));
    assert_eq!(loaded["packing"]["ArchiveType"], json!(5));
    assert_eq!(loaded["packing"]["ArchiveFileName"], json!("Data.ba2"));
    assert_eq!(loaded["packing"]["AutodetectFlags"], json!(false));
    assert_eq!(loaded["packing"]["Split"], json!(2));
    // `TAsset.ToJson` leaves out a source file name equal to the asset name.
    assert_eq!(
        loaded["assets"],
        json!([
            [1, "meshes\\a.nif", ""],
            [0, "sound\\b.wav", "src\\b.wav", "Source.ba2"]
        ])
    );
    // A plain JSON list is read too; the settings it lacks get `LoadList`'s
    // defaults (booleans false, strings empty, Split -1, baNone the highest
    // archive type).
    let plain = dir.join("plain.json");
    std::fs::write(
        &plain,
        r#"{"Packing":{"ArchiveType":5,"Split":3},"Assets":[[0,"meshes\\a.nif"]]}"#,
    )
    .unwrap();
    let loaded = call(
        &mut session,
        "bsarchpro.list_load",
        json!({ "list": plain.to_str().unwrap() }),
    );
    assert_eq!(loaded["packing"]["ArchiveType"], json!(5));
    assert_eq!(loaded["packing"]["Split"], json!(3));
    assert_eq!(loaded["packing"]["AutodetectFlags"], json!(false));
    assert_eq!(loaded["packing"]["ArchiveFileName"], json!(""));
    assert_eq!(loaded["assets"], json!([[0, "meshes\\a.nif", ""]]));
    std::fs::write(&plain, r#"{"Assets":[]}"#).unwrap();
    let loaded = call(
        &mut session,
        "bsarchpro.list_load",
        json!({ "list": plain.to_str().unwrap() }),
    );
    // Without Packing the form's `FormCreate` defaults apply.
    assert_eq!(loaded["packing"]["ArchiveType"], json!(8));
    assert_eq!(loaded["packing"]["AutodetectFlags"], json!(true));
    assert_eq!(loaded["packing"]["MultiThreaded"], json!(true));
    assert_eq!(loaded["packing"]["SharedData"], json!(true));
    assert_eq!(loaded["packing"]["Split"], json!(-1));
    assert!(
        loaded["packing"]["ArchiveFileName"]
            .as_str()
            .unwrap()
            .ends_with("New.ba2")
    );
    // A dry run writes nothing.
    let dry = call(
        &mut session,
        "bsarchpro.list_save",
        json!({ "list": dir.join("dry").to_str().unwrap(), "dry_run": true }),
    );
    assert_eq!(dry["written"], json!(false));
    assert!(!dir.join("dry.bsarch").exists());
}

#[test]
fn compress_sets_the_flag_by_do_not_compress() {
    let _guard = test_lock();
    let dir = TempDir::new(format!("xedit-bsarch-compress-{}", std::process::id()));
    let mut session = bsarch_session();
    let list = write_list(
        &mut session,
        &dir,
        "list",
        json!([
            [1, "sound\\test.wav"],
            [0, "sound\\line.fuz"],
            [0, "sound\\anim.hkx"],
            [1, "meshes\\a.nif"],
            [0, "music\\a.xwm"],
            [0, "strings\\a.strings"],
            [1, "interface\\a.swf"]
        ]),
    );
    let result = call(
        &mut session,
        "bsarchpro.compress",
        json!({ "list": list, "dry_run": true }),
    );
    assert_eq!(result["compressed"], json!(4));
    assert_eq!(result["uncompressed"], json!(3));
    let result = call(&mut session, "bsarchpro.compress", json!({ "list": list }));
    assert_eq!(result["compressed"], json!(4));
    let assets = call(&mut session, "bsarchpro.list_load", json!({ "list": list }))["assets"].clone();
    let flags: Vec<i64> = assets
        .as_array()
        .unwrap()
        .iter()
        .map(|asset| asset[0].as_i64().unwrap())
        .collect();
    assert_eq!(flags, [0, 1, 1, 1, 0, 0, 1]);
}

#[test]
fn edits_one_asset_name_trimmed() {
    let _guard = test_lock();
    let dir = TempDir::new(format!("xedit-bsarch-edit-{}", std::process::id()));
    let mut session = bsarch_session();
    let list = write_list(&mut session, &dir, "list", json!([[1, "old.nif"]]));
    let edited = call(
        &mut session,
        "bsarchpro.edit",
        json!({ "list": list, "index": 0, "name": "  new.nif  " }),
    );
    assert_eq!(edited["changed"], json!(true));
    assert_eq!(edited["asset_name"], json!("new.nif"));
    // Empty text keeps the name.
    let kept = call(
        &mut session,
        "bsarchpro.edit",
        json!({ "list": list, "index": 0, "name": "   " }),
    );
    assert_eq!(kept["changed"], json!(false));
    assert_eq!(kept["asset_name"], json!("new.nif"));
    assert_eq!(asset_names(&mut session, &list), ["new.nif"]);
    assert_eq!(
        error(
            &mut session,
            "bsarchpro.edit",
            json!({ "list": list, "index": 4, "name": "x" })
        )
        .code,
        "invalid_params"
    );
}

#[test]
fn replaces_prepends_and_appends_in_asset_names() {
    let _guard = test_lock();
    let dir = TempDir::new(format!("xedit-bsarch-replace-{}", std::process::id()));
    let mut session = bsarch_session();
    let list = write_list(
        &mut session,
        &dir,
        "list",
        json!([
            [1, "Textures\\A.DDS"],
            [1, "Textures\\B.dds"],
            [1, "Other.esp"],
            [1, "Other2.esp"]
        ]),
    );
    // `rfReplaceAll` and `rfIgnoreCase`.
    let result = call(
        &mut session,
        "bsarchpro.replace",
        json!({ "list": list, "mode": "replace", "search": "textures\\", "replace_text": "New\\",
                "selected": [0, 1, 2] }),
    );
    assert_eq!(result["changed"], json!(2));
    assert_eq!(
        asset_names(&mut session, &list),
        ["New\\A.DDS", "New\\B.dds", "Other.esp", "Other2.esp"]
    );
    let result = call(
        &mut session,
        "bsarchpro.replace",
        json!({ "list": list, "mode": "prepend", "search": "pre\\", "selected": [2] }),
    );
    assert_eq!(result["changed"], json!(1));
    call(
        &mut session,
        "bsarchpro.replace",
        json!({ "list": list, "mode": "append", "search": ".bak", "selected": [3] }),
    );
    assert_eq!(
        asset_names(&mut session, &list),
        ["New\\A.DDS", "New\\B.dds", "pre\\Other.esp", "Other2.esp.bak"]
    );
    // The replace mode requires the search text; the file is unchanged.
    assert_eq!(
        error(
            &mut session,
            "bsarchpro.replace",
            json!({ "list": list, "mode": "replace", "search": "", "selected": [0] })
        )
        .code,
        "invalid_params"
    );
    assert_eq!(asset_names(&mut session, &list)[0], "New\\A.DDS");
}

#[test]
fn removes_selected_assets_with_the_clear_question() {
    let _guard = test_lock();
    let dir = TempDir::new(format!("xedit-bsarch-remove-{}", std::process::id()));
    let mut session = bsarch_session();
    let list = write_list(
        &mut session,
        &dir,
        "list",
        json!([[1, "a.nif"], [1, "b.nif"], [1, "c.nif"]]),
    );
    let no = call(
        &mut session,
        "bsarchpro.remove_selected",
        json!({ "list": list, "selected": [0], "confirm": false }),
    );
    assert_eq!(no["removed"], json!(0));
    assert_eq!(no["remaining"], json!(3));
    let one = call(
        &mut session,
        "bsarchpro.remove_selected",
        json!({ "list": list, "selected": [0] }),
    );
    assert_eq!(one["removed"], json!(1));
    assert_eq!(one["remaining"], json!(2));
    assert_eq!(asset_names(&mut session, &list), ["b.nif", "c.nif"]);
    // The whole list selected asks `btnClearList`'s question too.
    let kept = call(
        &mut session,
        "bsarchpro.remove_selected",
        json!({ "list": list, "selected": [0, 1], "clear_confirm": false }),
    );
    assert_eq!(kept["removed"], json!(0));
    assert_eq!(kept["cleared"], json!(false));
    assert_eq!(asset_names(&mut session, &list), ["b.nif", "c.nif"]);
    let cleared = call(
        &mut session,
        "bsarchpro.remove_selected",
        json!({ "list": list, "selected": [0, 1] }),
    );
    assert_eq!(cleared["removed"], json!(2));
    assert_eq!(cleared["remaining"], json!(0));
    assert_eq!(cleared["cleared"], json!(true));
    assert!(asset_names(&mut session, &list).is_empty());
    assert_eq!(
        error(
            &mut session,
            "bsarchpro.remove_selected",
            json!({ "list": list, "selected": [7] })
        )
        .code,
        "invalid_params"
    );
}

#[test]
fn removes_the_unselected_assets() {
    let _guard = test_lock();
    let dir = TempDir::new(format!("xedit-bsarch-unselected-{}", std::process::id()));
    let mut session = bsarch_session();
    let list = write_list(
        &mut session,
        &dir,
        "list",
        json!([[1, "a.nif"], [1, "b.nif"], [1, "c.nif"], [1, "d.nif"]]),
    );
    let no = call(
        &mut session,
        "bsarchpro.remove_unselected",
        json!({ "list": list, "selected": [1], "confirm": false }),
    );
    assert_eq!(no["removed"], json!(0));
    assert_eq!(no["remaining"], json!(4));
    let result = call(
        &mut session,
        "bsarchpro.remove_unselected",
        json!({ "list": list, "selected": [1] }),
    );
    assert_eq!(result["removed"], json!(3));
    assert_eq!(result["remaining"], json!(1));
    assert_eq!(asset_names(&mut session, &list), ["b.nif"]);
}

#[test]
fn finds_the_assets_with_identical_data() {
    let _guard = test_lock();
    let dir = TempDir::new(format!("xedit-bsarch-identical-{}", std::process::id()));
    let mut session = bsarch_session();
    for (name, contents) in [
        ("a.bin", "same"),
        ("b.bin", "same"),
        ("c.bin", "diff"),
        ("d.bin", "diff"),
        ("e.bin", "same"),
    ] {
        std::fs::write(dir.join(name), contents).unwrap();
    }
    let assets: Vec<Value> = ["a.bin", "b.bin", "c.bin", "d.bin", "e.bin"]
        .iter()
        .map(|name| json!([1, name, dir.join(name).to_str().unwrap()]))
        .collect();
    let list = write_list(&mut session, &dir, "list", Value::Array(assets));
    let result = call(&mut session, "bsarchpro.find_identical", json!({ "list": list }));
    assert_eq!(result["duplicate_files"], json!(3));
    assert_eq!(result["duplicate_size"], json!(12));
    assert_eq!(result["size_text"], json!("12 Bytes"));
    assert_eq!(result["groups"].as_array().unwrap().len(), 2, "{result}");
    assert_eq!(result["groups"][0]["data_size"], json!(4));
    assert_eq!(result["groups"][0]["assets"].as_array().unwrap().len(), 3);
    assert_eq!(result["groups"][1]["assets"].as_array().unwrap().len(), 2);
    let hash = result["groups"][0]["lookup_hash"].as_str().unwrap();
    assert_eq!(hash.len(), 16);
    assert_eq!(
        result["title"],
        json!(
            "Duplicate Files: 3\nDuplicate Size: 12 Bytes (uncompressed saved space if all identical files will end up in the same archive with Shared Data option)"
        )
    );
    assert_eq!(result["lines"][0], json!("Duplicate Files: 3"));
    assert_eq!(result["lines"][2], json!(""));
    // Different data of the same size stays apart (the lookup hash decides).
    let list = write_list(
        &mut session,
        &dir,
        "distinct",
        json!([
            [1, "one.bin", dir.join("a.bin").to_str().unwrap()],
            [1, "two.bin", dir.join("c.bin").to_str().unwrap()]
        ]),
    );
    let none = call(&mut session, "bsarchpro.find_identical", json!({ "list": list }));
    assert!(none["groups"].as_array().unwrap().is_empty());
    assert_eq!(none["message"], json!("No identical assets found"));
    assert_eq!(none["title"], Value::Null);
    // A missing source file reports the read error as the dialog does.
    let list = write_list(
        &mut session,
        &dir,
        "missing",
        json!([[1, "x.nif", dir.join("missing.nif").to_str().unwrap()]]),
    );
    assert_eq!(
        error(&mut session, "bsarchpro.find_identical", json!({ "list": list })).code,
        "io"
    );
}

#[test]
fn bsarchpro_commands_need_edit_or_dry_run() {
    let _guard = test_lock();
    let dir = TempDir::new(format!("xedit-bsarch-gate-{}", std::process::id()));
    let mut session = Session::with_files(GameMode::gmSSE, Vec::new());
    let path = dir.join("list.bsarch");
    std::fs::write(&path, "{}").unwrap();
    let registry = Registry::standard();
    for (name, params) in [
        ("bsarchpro.list_save", json!({ "list": path.to_str().unwrap() })),
        ("bsarchpro.compress", json!({ "list": path.to_str().unwrap() })),
        (
            "bsarchpro.edit",
            json!({ "list": path.to_str().unwrap(), "index": 0, "name": "x" }),
        ),
        (
            "bsarchpro.replace",
            json!({ "list": path.to_str().unwrap(), "mode": "prepend", "search": "x" }),
        ),
        (
            "bsarchpro.remove_selected",
            json!({ "list": path.to_str().unwrap(), "selected": [0] }),
        ),
        (
            "bsarchpro.remove_unselected",
            json!({ "list": path.to_str().unwrap(), "selected": [0] }),
        ),
    ] {
        let blocked = registry.call(&mut session, name, params.clone()).unwrap_err();
        assert_eq!(blocked.code, "edit_required", "{name}");
        let mut dry = params;
        dry["dry_run"] = json!(true);
        // The command may still refuse its parameters; only the edit gate
        // must be gone.
        let result = registry.call(&mut session, name, dry);
        assert!(
            result.as_ref().err().is_none_or(|error| error.code != "edit_required"),
            "{name}: {result:?}"
        );
    }
}
