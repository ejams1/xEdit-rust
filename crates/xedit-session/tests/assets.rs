// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `assets.*` commands on NIF and material files built in the test:
//! the dumps, the block list, a save, an edit and its save, and a file
//! built back from its JSON dump.

use serde_json::{Value, json};

use xedit_assets::data_format_material::MaterialFile;
use xedit_assets::data_format_nif::{
    NifFile, NifVersion, add_block, block, block_add_child, block_add_extra_data, block_get_strips,
    block_get_triangles, block_set_triangles, block_type, blocks_count, set_nif_version,
};
use xedit_assets::nif_math::stripify_triangles;
use xedit_assets::variant::Variant;
use xedit_io::temp::TempDir;
use xedit_session::{Registry, Session};

fn temp_dir(name: &str) -> TempDir {
    TempDir::new(format!("xedit-assets-test-{}-{name}", std::process::id()))
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
}

#[test]
fn unknown_kind_is_an_error() {
    let error = Registry::standard()
        .call(&mut Session::default(), "assets.dump", json!({ "file": "x.unknown" }))
        .unwrap_err();
    assert_eq!(error.code, "invalid_params");
}

/// An Oblivion mesh: a root node with a rendered triangle shape whose data
/// holds four vertices and two triangles that form a strip.
fn spell_nif() -> Vec<u8> {
    let mut nif = NifFile::new().unwrap();
    let tree = &mut nif.tree;
    set_nif_version(tree, NifVersion::Tes4).unwrap();
    let root = add_block(tree, "NiNode").unwrap();
    tree.set_edit_values(root, "Name", "Scene Root").unwrap();
    let shape = block_add_child(tree, root, "NiTriShape").unwrap();
    let data = add_block(tree, "NiTriShapeData").unwrap();
    let data_index = tree.index(data).unwrap();
    tree.set_native_values(shape, "Data", Variant::Int(i64::from(data_index)))
        .unwrap();
    tree.set_native_values(data, "Num Vertices", Variant::Int(4)).unwrap();
    tree.set_native_values(data, "Has Vertices", Variant::Int(1)).unwrap();
    let vertices = tree.elements(data, "Vertices").unwrap().unwrap();
    tree.set_count(vertices, 4).unwrap();
    for (index, text) in [
        "0.000000 0.000000 0.000000",
        "1.000000 0.000000 0.000000",
        "0.000000 2.000000 0.000000",
        "1.000000 2.000000 0.000000",
    ]
    .iter()
    .enumerate()
    {
        let vertex = tree.item(vertices, index as i32).unwrap();
        tree.set_edit_value(vertex, text).unwrap();
    }
    // `NiTriShapeData_EnTriangles`: below 10.0.1.2 the array is always on,
    // from there it needs `Has Triangles`.
    tree.set_native_values(data, "Has Triangles", Variant::Int(1)).unwrap();
    block_set_triangles(tree, data, &[[0, 1, 2], [2, 1, 3]], None).unwrap();
    // An Oblivion shape counts as rendered with a NiMaterialProperty.
    let material = add_block(tree, "NiMaterialProperty").unwrap();
    tree.set_edit_values(material, "Alpha", "1.000000").unwrap();
    let properties = tree.elements(shape, "Properties").unwrap().unwrap();
    let entry = tree.add(properties).unwrap();
    let material_index = tree.index(material).unwrap();
    tree.set_native_value(entry, Variant::Int(i64::from(material_index)))
        .unwrap();
    nif.save_to_data().unwrap()
}

/// The spells of `assets.optimize`: the default options, `stripify`
/// (`SpellStripify`), `triangulate` (`SpellTriangulate`), the dry run and
/// the unknown option.
#[test]
fn optimize_runs_the_mesh_spells() {
    let dir = temp_dir("optimize");
    let file = dir.join("spell.nif");
    let bytes = spell_nif();
    std::fs::write(&file, &bytes).unwrap();
    let file = file.to_string_lossy().into_owned();
    let output = dir.join("stripified.nif").to_string_lossy().into_owned();
    let mut session = Session::default();
    let registry = Registry::standard();

    // A mutating command needs the edit flag; the dry run writes nothing.
    let error = registry
        .call(
            &mut session,
            "assets.optimize",
            json!({ "file": file, "output": output }),
        )
        .unwrap_err();
    assert_eq!(error.code, "edit_required");
    let dry = call(
        &mut session,
        "assets.optimize",
        json!({ "file": file, "output": output, "dry_run": true }),
    );
    assert_eq!(dry["options"], json!(["vertex-cache", "overdraw"]));
    assert_eq!(dry["written"], false);
    assert!(!std::path::Path::new(&output).exists());

    // `stripify` alone is SpellStripify's stripify pass: the shape and its
    // data become the strips blocks, and the strips are the ones
    // `wbNifMath.StripifyTriangles` gives for the same triangles.
    session.allow_edit(true);
    let stripified = call(
        &mut session,
        "assets.optimize",
        json!({ "file": file, "output": output, "options": ["stripify"] }),
    );
    assert_eq!(stripified["changed"], true);
    assert_eq!(stripified["written"], true);
    assert_eq!(stripified["equal_to_input"], false);
    let mut nif = NifFile::new().unwrap();
    nif.load_from_data(&std::fs::read(&output).unwrap()).unwrap();
    let tree = &mut nif.tree;
    let mut types = Vec::new();
    for index in 0..blocks_count(tree).unwrap() {
        let b = block(tree, index).unwrap();
        types.push(block_type(tree, b));
    }
    assert_eq!(
        types,
        ["NiNode", "NiTriStrips", "NiTriStripsData", "NiMaterialProperty"]
    );
    let data = block(tree, 2).unwrap();
    let strips = block_get_strips(tree, data, None).unwrap();
    assert_eq!(strips.len(), 1);
    assert_eq!(stripify_triangles(&[[0, 1, 2], [2, 1, 3]]), strips[0]);

    // The stripified file triangulates back with `triangulate`
    // (SpellTriangulate).
    let flat = dir.join("flat.nif").to_string_lossy().into_owned();
    let triangulated = call(
        &mut session,
        "assets.optimize",
        json!({ "file": output, "output": flat, "options": ["triangulate"] }),
    );
    assert_eq!(triangulated["changed"], true);
    let mut nif = NifFile::new().unwrap();
    nif.load_from_data(&std::fs::read(&flat).unwrap()).unwrap();
    let tree = &mut nif.tree;
    let mut types = Vec::new();
    for index in 0..blocks_count(tree).unwrap() {
        let b = block(tree, index).unwrap();
        types.push(block_type(tree, b));
    }
    assert_eq!(types, ["NiNode", "NiTriShape", "NiTriShapeData", "NiMaterialProperty"]);
    let data = block(tree, 2).unwrap();
    let mut tris = block_get_triangles(tree, data, None).unwrap();
    tris.sort();
    // The second triangle keeps the winding the strip gives it.
    assert_eq!(tris, [[0, 1, 2], [1, 3, 2]]);

    // An option the spells do not know.
    let error = registry
        .call(
            &mut session,
            "assets.optimize",
            json!({ "file": file, "output": flat, "options": ["sort"], "dry_run": true }),
        )
        .unwrap_err();
    assert_eq!(error.code, "invalid_params");
    assert!(error.message.contains("unknown optimization sort"));
}
