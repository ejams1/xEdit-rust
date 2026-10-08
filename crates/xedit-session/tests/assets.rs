// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `assets.*` commands on NIF and material files built in the test:
//! the dumps, the block list, a save, an edit and its save, and a file
//! built back from its JSON dump.

use std::path::PathBuf;

use serde_json::{Value, json};

use xedit_assets::data_format_material::MaterialFile;
use xedit_assets::data_format_nif::{
    NifFile, NifVersion, add_block, block_add_child, block_add_extra_data, set_nif_version,
};
use xedit_session::{Registry, Session};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xedit-assets-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A small Fallout 4 NIF: a root node with an extra data block and a child
/// node.
fn test_nif() -> Vec<u8> {
    let mut nif = NifFile::new().unwrap();
    let tree = &mut nif.tree;
    tree.set_to_default(nif.root).unwrap();
    set_nif_version(tree, NifVersion::Fo4).unwrap();
    let root = add_block(tree, "BSFadeNode").unwrap();
    tree.set_edit_values(root, "Name", "Root").unwrap();
    let bsx = block_add_extra_data(tree, root, "BSXFlags").unwrap();
    tree.set_edit_values(bsx, "Flags", "Havok").unwrap();
    let child = block_add_child(tree, root, "NiNode").unwrap();
    tree.set_edit_values(child, "Name", "Child").unwrap();
    tree.set_edit_values(child, "Transform\\Scale", "2.5").unwrap();
    nif.save_to_data().unwrap()
}

fn call(session: &mut Session, name: &str, params: Value) -> Value {
    Registry::standard()
        .call(session, name, params)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

#[test]
fn nif_dump_blocks_save_set_and_json() {
    let dir = temp_dir("nif");
    let file = dir.join("test.nif");
    let bytes = test_nif();
    std::fs::write(&file, &bytes).unwrap();
    let file = file.to_string_lossy().into_owned();
    let mut session = Session::default();

    let blocks = call(&mut session, "assets.blocks", json!({ "file": file }));
    assert_eq!(blocks["nif_version"], "nfFO4");
    let names: Vec<&str> = blocks["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|block| block["block_type"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["NiHeader", "BSFadeNode", "BSXFlags", "NiNode", "NiFooter"]);
    assert_eq!(blocks["blocks"][1]["name"], "Root");

    let text = call(&mut session, "assets.dump", json!({ "file": file }));
    let text = text["text"].as_str().unwrap();
    assert!(text.starts_with("NIF\r\n\tNiHeader\r\n"), "{text}");
    assert!(text.contains("\t\tFlags: Havok\r\n"), "{text}");
    assert!(text.contains("\t\t\tScale: 2.500000\r\n"), "{text}");

    let json_dump = call(&mut session, "assets.dump", json!({ "file": file, "format": "json" }));
    let json_text = json_dump["text"].as_str().unwrap();
    assert!(json_text.contains("\"0 BSFadeNode\": {"), "{json_text}");
    assert!(json_text.contains("\"1 BSXFlags \\\"BSX\\\"\""), "{json_text}");

    // A save needs the edit flag or a dry run.
    let output = dir.join("saved.nif").to_string_lossy().into_owned();
    let error = Registry::standard()
        .call(&mut session, "assets.save", json!({ "file": file, "output": output }))
        .unwrap_err();
    assert_eq!(error.code, "edit_required");
    let dry = call(
        &mut session,
        "assets.save",
        json!({ "file": file, "output": output, "dry_run": true }),
    );
    assert_eq!(dry["equal_to_input"], true);
    assert_eq!(dry["written"], false);
    assert!(!std::path::Path::new(&output).exists());

    session.allow_edit(true);
    let saved = call(&mut session, "assets.save", json!({ "file": file, "output": output }));
    assert_eq!(saved["written"], true);
    assert_eq!(std::fs::read(&output).unwrap(), bytes);

    // An edit by block index and one by block path.
    let edited = dir.join("edited.nif").to_string_lossy().into_owned();
    let set = call(
        &mut session,
        "assets.set",
        json!({
            "file": file,
            "edits": [
                { "block": "2", "path": "Transform\\Scale", "value": "1" },
                { "block": "Child", "path": "Name", "value": "Renamed" },
            ],
            "output": edited,
        }),
    );
    assert_eq!(set["changes"][0]["old"], "2.500000");
    assert_eq!(set["changes"][0]["new"], "1.000000");
    assert_eq!(set["changes"][1]["new"], "Renamed");
    let reread = call(&mut session, "assets.blocks", json!({ "file": edited }));
    assert_eq!(reread["blocks"][3]["name"], "Renamed");

    // The JSON dump builds the same file again.
    let json_file = dir.join("test.nif.json");
    std::fs::write(&json_file, json_text).unwrap();
    let rebuilt = dir.join("rebuilt.nif").to_string_lossy().into_owned();
    call(
        &mut session,
        "assets.from-json",
        json!({ "file": json_file.to_string_lossy(), "output": rebuilt }),
    );
    assert_eq!(std::fs::read(&rebuilt).unwrap(), bytes);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn material_dump_and_save() {
    let dir = temp_dir("material");
    let file = dir.join("test.bgsm");
    let mut material = MaterialFile::new_bgsm().unwrap();
    material.tree.set_to_default(material.root).unwrap();
    material
        .tree
        .set_edit_values(material.root, "Textures\\Diffuse", "test\\a_d.dds")
        .unwrap();
    let bytes = material.save_to_data().unwrap();
    std::fs::write(&file, &bytes).unwrap();
    let file = file.to_string_lossy().into_owned();
    let mut session = Session::default();

    let text = call(&mut session, "assets.dump", json!({ "file": file }));
    assert_eq!(text["kind"], "bgsm");
    assert!(text["text"].as_str().unwrap().contains("\t\tDiffuse: test/a_d.dds\r\n"));

    // Materials have no JSON writer upstream.
    let error = Registry::standard()
        .call(&mut session, "assets.dump", json!({ "file": file, "format": "json" }))
        .unwrap_err();
    assert_eq!(error.code, "load_failed");

    let saved = call(
        &mut session,
        "assets.save",
        json!({ "file": file, "output": "unused.bgsm", "dry_run": true }),
    );
    assert_eq!(saved["equal_to_input"], true);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_kind_is_an_error() {
    let error = Registry::standard()
        .call(&mut Session::default(), "assets.dump", json!({ "file": "x.unknown" }))
        .unwrap_err();
    assert_eq!(error.code, "invalid_params");
}
