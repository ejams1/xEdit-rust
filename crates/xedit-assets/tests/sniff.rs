// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Sniff operations on synthetic meshes in a temporary folder: the run
//! of the automation mode (file collection, output, messages, summary),
//! the settings, the dry run, and a few processors end to end. The parity
//! with `Sniff.exe` on the game meshes is `cargo xtask parity sniff`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use xedit_assets::data_format_nif::{
    NifFile, NifVersion, add_block, block, block_add_child, blocks_count, set_nif_version,
};
use xedit_assets::sniff::main_form::{RunError, RunOptions, run};
use xedit_assets::sniff::processor::{FileStatus, MemIniFile};
use xedit_assets::variant::Variant;

/// The runs share the process-wide float format of the JSON converter.
static LOCK: Mutex<()> = Mutex::new(());

/// A Fallout 3 mesh: a root node with an unnamed child node, an unnamed
/// triangle shape with three vertices and a material, and an unused
/// extra data block.
fn mesh() -> Vec<u8> {
    let mut nif = NifFile::new().unwrap();
    let tree = &mut nif.tree;
    set_nif_version(tree, NifVersion::Fo3).unwrap();
    let root = add_block(tree, "NiNode").unwrap();
    tree.set_edit_values(root, "Name", "Scene Root").unwrap();
    block_add_child(tree, root, "NiNode").unwrap();
    let shape = block_add_child(tree, root, "NiTriShape").unwrap();
    let data = add_block(tree, "NiTriShapeData").unwrap();
    let data_index = tree.index(data).unwrap();
    tree.set_native_values(shape, "Data", Variant::Int(i64::from(data_index)))
        .unwrap();
    tree.set_native_values(data, "Num Vertices", Variant::Int(3)).unwrap();
    tree.set_native_values(data, "Has Vertices", Variant::Int(1)).unwrap();
    let vertices = tree.elements(data, "Vertices").unwrap().unwrap();
    tree.set_count(vertices, 3).unwrap();
    for (index, text) in [
        "0.000000 0.000000 0.000000",
        "1.000000 0.000000 0.000000",
        "0.000000 2.000000 0.000000",
    ]
    .iter()
    .enumerate()
    {
        let vertex = tree.item(vertices, index as i32).unwrap();
        tree.set_edit_value(vertex, text).unwrap();
    }
    let extra = add_block(tree, "NiStringExtraData").unwrap();
    tree.set_edit_values(extra, "Name", "UPB").unwrap();
    let material = add_block(tree, "NiMaterialProperty").unwrap();
    tree.set_edit_values(material, "Alpha", "1.000000").unwrap();
    let properties = tree.elements(shape, "Properties").unwrap().unwrap();
    let entry = tree.add(properties).unwrap();
    let material_index = tree.index(material).unwrap();
    tree.set_native_value(entry, Variant::Int(i64::from(material_index)))
        .unwrap();
    nif.save_to_data().unwrap()
}

/// A folder with the mesh at `meshes\a\b.nif`, a text file and an output
/// folder.
fn folders(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("xedit-sniff-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let input = base.join("in");
    let output = base.join("out");
    std::fs::create_dir_all(input.join("meshes").join("a")).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(input.join("meshes").join("a").join("b.nif"), mesh()).unwrap();
    std::fs::write(input.join("readme.txt"), "not a mesh").unwrap();
    (input, output)
}

fn options(operation: &str, input: &Path, output: &Path) -> RunOptions {
    RunOptions {
        operation: operation.to_owned(),
        input: input.display().to_string(),
        output: output.display().to_string(),
        threads: Some(2),
        ..RunOptions::default()
    }
}

fn load(path: &Path) -> NifFile {
    let mut nif = NifFile::new().unwrap();
    nif.load_from_data(&std::fs::read(path).unwrap()).unwrap();
    nif
}

#[test]
fn update_bounds_writes_the_changed_mesh() {
    let _lock = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (input, output) = folders("bounds");
    let report = run(None, &options("update BOUNDS", &input, &output)).unwrap();
    assert_eq!(report.operation, "Update bounds");
    assert_eq!(report.processed, 1);
    assert_eq!(report.modified, 1);
    assert_eq!(report.files, vec![("meshes\\a\\b.nif".to_owned(), FileStatus::Updated)]);
    assert_eq!(report.messages[0], "Updated: meshes\\a\\b.nif");
    assert!(report.messages[1].starts_with("Done. Updated 1 files out of 1, elapsed time 00:00:"));
    let mut nif = load(&output.join("meshes").join("a").join("b.nif"));
    let tree = &mut nif.tree;
    let data = block(tree, 3).unwrap();
    assert_eq!(tree.edit_values(data, "Bounding Sphere\\Radius").unwrap(), "1.118034");
}

#[test]
fn set_missing_names_names_after_the_file() {
    let _lock = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (input, output) = folders("names");
    let report = run(None, &options("Set missing names", &input, &output)).unwrap();
    assert_eq!(report.modified, 1);
    let mut nif = load(&output.join("meshes").join("a").join("b.nif"));
    let tree = &mut nif.tree;
    let names: Vec<String> = (0..3)
        .map(|index| {
            let b = block(tree, index).unwrap();
            tree.edit_values(b, "Name").unwrap()
        })
        .collect();
    assert_eq!(names, vec!["b", "b:0", "b:1"]);
}

#[test]
fn settings_dry_run_and_report_only() {
    let _lock = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (input, output) = folders("tweaker");
    // The tweaker's defaults set the material's alpha to 0.8; the report
    // writes nothing.
    let settings = MemIniFile::from_text("[Universaltweaker]\r\nbReportOnly=1\r\n");
    let report = run(Some(settings), &options("Universal tweaker", &input, &output)).unwrap();
    assert_eq!(report.modified, 0);
    assert_eq!(
        &report.messages[..3],
        [
            "meshes\\a\\b.nif".to_owned(),
            "\t5 NiMaterialProperty\\Alpha: Changed from \"1.000000\" to \"0.800000\"".to_owned(),
            String::new()
        ]
    );
    assert!(!output.join("meshes").exists());

    // A dry run reports the update and writes nothing.
    let mut dry = options("Universal tweaker", &input, &output);
    dry.dry_run = true;
    let report = run(None, &dry).unwrap();
    assert_eq!(report.modified, 1);
    assert!(!output.join("meshes").exists());

    // Unknown, not ported and failing operations.
    assert!(matches!(
        run(None, &options("No such thing", &input, &output)),
        Err(RunError::UnknownOperation(_))
    ));
    assert!(matches!(
        run(None, &options("Optimize mesh", &input, &output)),
        Err(RunError::NotPorted(..))
    ));
    let settings = MemIniFile::from_text("[Universaltweaker]\r\nsPath=\r\n");
    match run(Some(settings), &options("Universal tweaker", &input, &output)) {
        Err(RunError::Message(message)) => assert_eq!(message, "Field path can not be empty"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn fixer_and_checks_report_the_mesh() {
    let _lock = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (input, output) = folders("fixer");
    let log = output.join("fixer.log");
    let settings = MemIniFile::from_text(&format!(
        "[Universalfixer]\r\nbSaveLog=1\r\nsLogFile={}\r\n",
        log.display()
    ));
    let report = run(Some(settings), &options("Universal fixer", &input, &output)).unwrap();
    assert_eq!(report.modified, 1);
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.starts_with("meshes\\a\\b.nif\r\n"), "{text}");
    assert!(
        text.contains("\t4 NiStringExtraData: Removed unused \"UPB\" extra data\r\n"),
        "{text}"
    );
    let nif = load(&output.join("meshes").join("a").join("b.nif"));
    let mut tree = nif.tree;
    assert_eq!(blocks_count(&mut tree).unwrap(), 5);

    let (input, output) = folders("checks");
    let report = run(None, &options("Check for errors", &input, &output)).unwrap();
    assert_eq!(report.modified, 0);
    assert_eq!(report.messages[0], "meshes\\a\\b.nif");
    assert!(
        report
            .messages
            .contains(&"\t4 NiStringExtraData: Unused block not referenced from the root scenegparh".to_owned()),
        "{:?}",
        report.messages
    );
}

#[test]
fn a_failing_file_stops_or_is_skipped() {
    let _lock = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (input, output) = folders("broken");
    std::fs::write(
        input.join("meshes").join("a").join("a.nif"),
        b"not a NIF file, but longer than its magic",
    )
    .unwrap();
    let mut skip = options("Update bounds", &input, &output);
    skip.skip_on_errors = Some(true);
    let report = run(None, &skip).unwrap();
    assert_eq!(report.processed, 2);
    assert_eq!(report.messages[0], "Skipped: meshes\\a\\a.nif: Not a valid NIF file");
    match run(None, &options("Update bounds", &input, &output)) {
        Err(RunError::Aborted { messages, error }) => {
            assert_eq!(error, "meshes\\a\\a.nif: Not a valid NIF file");
            assert_eq!(
                messages,
                vec!["\r\nError: \"meshes\\a\\a.nif: Not a valid NIF file".to_owned()]
            );
        }
        other => panic!("{other:?}"),
    }
}
