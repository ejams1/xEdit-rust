// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `sniff.*` commands: the list of operations with their settings,
//! a run with settings and its dry run, the log file and the errors.

use std::sync::Mutex;

use serde_json::json;

use xedit_assets::data_format_nif::{NifFile, NifVersion, add_block, block_add_child, set_nif_version};
use xedit_session::{Registry, Session};

static LOCK: Mutex<()> = Mutex::new(());

/// A Fallout 3 mesh: a root node and an unnamed child node.
fn mesh() -> Vec<u8> {
    let mut nif = NifFile::new().unwrap();
    let tree = &mut nif.tree;
    set_nif_version(tree, NifVersion::Fo3).unwrap();
    let root = add_block(tree, "NiNode").unwrap();
    tree.set_edit_values(root, "Name", "Scene Root").unwrap();
    block_add_child(tree, root, "NiNode").unwrap();
    nif.save_to_data().unwrap()
}

#[test]
fn list_names_every_operation() {
    let mut session = Session::default();
    let result = Registry::standard()
        .call(&mut session, "sniff.list", json!({}))
        .unwrap();
    let operations = result["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 50);
    let tweaker = operations.iter().find(|op| op["title"] == "Universal tweaker").unwrap();
    assert_eq!(tweaker["section"], "Universaltweaker");
    assert!(
        tweaker["settings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|setting| setting["name"] == "sPath" && setting["default"] == "Alpha")
    );
    let mopp = operations.iter().find(|op| op["title"] == "Update MOPP code").unwrap();
    assert!(mopp["not_ported"].as_str().unwrap().contains("NifMopp.dll"));
    // It is the only one: every other operation has its processor and the
    // settings of its frame.
    assert_eq!(
        operations.iter().filter(|op| !op["not_ported"].is_null()).count(),
        1
    );
    let textures = operations.iter().find(|op| op["title"] == "Find textures").unwrap();
    assert_eq!(textures["files"], "*.dds");
    // It writes the files it copies unless `bReportOnly` is on.
    assert_eq!(textures["report_only"], false);
    assert!(
        textures["settings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|setting| setting["name"] == "bHeaderDump" && setting["default"] == "0")
    );
    let skeleton = operations.iter().find(|op| op["title"] == "Add blocks from skeleton").unwrap();
    assert!(
        skeleton["settings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|setting| setting["name"] == "sNames" && setting["default"] == "Weapon,HeadAnims")
    );
}

#[test]
fn run_with_options_dry_run_and_log() {
    let _lock = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let base = std::env::temp_dir().join(format!("xedit-sniff-session-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (input, output) = (base.join("in"), base.join("out"));
    std::fs::create_dir_all(&input).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(input.join("x.nif"), mesh()).unwrap();
    let log = base.join("log.txt");
    let params = |dry_run: bool| {
        json!({
            "operation": "set missing names",
            "input": input.display().to_string(),
            "output": output.display().to_string(),
            "options": {"bRenameRoot": "0"},
            "log": log.display().to_string(),
            "dry_run": dry_run,
        })
    };
    let registry = Registry::standard();
    let mut session = Session::default();

    // Without --edit only the dry run runs, and writes nothing.
    let error = registry.call(&mut session, "sniff.run", params(false)).unwrap_err();
    assert_eq!(error.code, "edit_required");
    let result = registry.call(&mut session, "sniff.run", params(true)).unwrap();
    assert_eq!(result["operation"], "Set missing names");
    assert_eq!(result["updated"], 1);
    assert_eq!(result["files"][0]["file"], "x.nif");
    assert!(!output.join("x.nif").exists() && !log.exists());

    session.allow_edit(true);
    let result = registry.call(&mut session, "sniff.run", params(false)).unwrap();
    assert_eq!(result["updated"], 1);
    session.allow_edit(false);
    let mut nif = NifFile::new().unwrap();
    nif.load_from_data(&std::fs::read(output.join("x.nif")).unwrap())
        .unwrap();
    let tree = &mut nif.tree;
    let root = xedit_assets::data_format_nif::block(tree, 0).unwrap();
    let child = xedit_assets::data_format_nif::block(tree, 1).unwrap();
    // The root kept its name (bRenameRoot=0).
    assert_eq!(tree.edit_values(root, "Name").unwrap(), "Scene Root");
    assert_eq!(tree.edit_values(child, "Name").unwrap(), "x:0");
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.starts_with("Updated: x.nif\r\nDone. Updated 1 files out of 1, elapsed time "));

    // The errors.
    let mut bad = params(true);
    bad["operation"] = json!("No such thing");
    assert_eq!(
        registry.call(&mut session, "sniff.run", bad).unwrap_err().code,
        "invalid_params"
    );
    let mut not_ported = params(true);
    not_ported["operation"] = json!("Update MOPP code");
    assert_eq!(
        registry.call(&mut session, "sniff.run", not_ported).unwrap_err().code,
        "unsupported"
    );
    let _ = std::fs::remove_dir_all(&base);
}
