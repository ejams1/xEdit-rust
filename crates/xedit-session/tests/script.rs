// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `script.*` commands: the script folder of `script.list`, its
//! listing of the `*.pas` files with their line counts and the scripts of
//! the oracle's `Edit Scripts` when `XEDIT_ORACLE_DIR` is set.

use serde_json::json;
use xedit_session::{Registry, Session};

/// A folder of its own for a test, made fresh.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xedit-script-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn list_reports_the_pas_scripts_of_the_folder() {
    let dir = temp_dir("list");
    std::fs::write(dir.join("b script.pas"), "begin\nend.\n").unwrap();
    std::fs::write(dir.join("a script.pas"), "begin\nend.\nprogram Foobar;\n").unwrap();
    std::fs::write(dir.join("_newscript_.pas"), "begin\nend.\n").unwrap();
    std::fs::write(dir.join("notes.txt"), "not a script\n").unwrap();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub").join("nested.pas"), "begin\nend.\n").unwrap();

    let result = Registry::standard()
        .call(
            &mut Session::default(),
            "script.list",
            json!({ "scripts": dir.display().to_string() }),
        )
        .unwrap();

    assert_eq!(result["folder"], dir.display().to_string());
    let scripts = result["scripts"].as_array().unwrap();
    let names: Vec<&str> = scripts.iter().map(|script| script["name"].as_str().unwrap()).collect();
    // Sorted as the form's list is, the placeholder included, no other
    // extension and no subdirectory.
    assert_eq!(names, ["_newscript_.pas", "a script.pas", "b script.pas"]);
    let a = scripts.iter().find(|script| script["name"] == "a script.pas").unwrap();
    assert_eq!(a["lines"], 3);

    // The `XEDIT_SCRIPTS` environment variable is the default the CLI
    // leaves in place; the request's folder wins over it (they cannot be
    // tested together, since the tests share the process).
    assert_eq!(
        xedit_session::script::scripts_folder(Some(&dir.display().to_string())),
        dir
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_folder_is_an_io_error() {
    let error = Registry::standard()
        .call(
            &mut Session::default(),
            "script.list",
            json!({ "scripts": "Z:\\no-such-folder" }),
        )
        .unwrap_err();
    assert_eq!(error.code, "io");
}

/// The scripts of the oracle's `Edit Scripts`, when `XEDIT_ORACLE_DIR` is
/// set: the 4.1.5q corpus of 150 `*.pas` files (`xEditAPI.pas` and the
/// form's `_newscript_.pas` among them). Does nothing without the
/// environment variable.
#[test]
fn the_oracle_corpus_has_150_scripts() {
    let Some(oracle) = std::env::var_os("XEDIT_ORACLE_DIR") else {
        return;
    };
    let folder = std::path::PathBuf::from(oracle).join("Edit Scripts");
    let scripts = xedit_session::script::list_scripts(&folder).unwrap();
    assert_eq!(scripts.len(), 150);
    assert!(scripts.iter().any(|script| script.name == "xEditAPI.pas"));
    let api = scripts.iter().find(|script| script.name == "xEditAPI.pas").unwrap();
    assert_eq!(api.lines, 1633);
}
