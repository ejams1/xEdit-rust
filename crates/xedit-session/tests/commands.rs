// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The inspection commands on a plugin of an installed game. The test does
//! nothing when `XEDIT_SSE_DATA` is not in the environment.

use std::path::Path;

use serde_json::json;
use xedit_core::interface::globals::test_lock;
use xedit_session::{Registry, Session};

#[test]
fn inspects_update_esm() {
    let _guard = test_lock();
    let Some(data) = std::env::var("XEDIT_SSE_DATA").ok() else {
        return;
    };
    let path = format!("{data}/Update.esm");
    if !Path::new(&path).exists() {
        return;
    }
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    let registry = Registry::standard();
    let mut session = Session::load("sse", &[path]).unwrap();

    let info = registry.call(&mut session, "session.info", json!({})).unwrap();
    assert_eq!(info["game"], "sse");
    assert_eq!(info["files"][0], "Skyrim.esm");
    assert_eq!(info["files"][2], "Update.esm");

    let files = registry.call(&mut session, "files.list", json!({})).unwrap();
    let update = &files["files"][2];
    assert_eq!(update["name"], "Update.esm");
    assert_eq!(update["masters"], json!(["Skyrim.esm"]));
    assert_eq!(update["load_order"], 1);
    assert!(update["record_count"].as_u64().unwrap() > 10_000);

    let found = registry
        .call(
            &mut session,
            "records.find",
            json!({ "editor_id": "help_dlcdawnguard", "signature": "MESG" }),
        )
        .unwrap();
    assert_eq!(found["total"], 1);
    assert_eq!(found["records"][0]["form_id"], "01003274");

    let listed = registry
        .call(&mut session, "records.list", json!({ "signature": "GMST", "limit": 2 }))
        .unwrap();
    assert_eq!(listed["records"].as_array().unwrap().len(), 2);
    assert!(listed["total"].as_u64().unwrap() > 2);

    let record = registry
        .call(
            &mut session,
            "records.get",
            json!({ "form_id": "01003274", "depth": 1 }),
        )
        .unwrap();
    assert_eq!(record["editor_id"], "Help_DLCDawnguard");
    assert_eq!(record["elements"][0]["name"], "Record Header");
    assert!(record["elements"][0]["children"][0]["children"].is_null());

    let element = registry
        .call(
            &mut session,
            "elements.get",
            json!({ "form_id": "01003274", "path": "DNAM" }),
        )
        .unwrap();
    assert_eq!(element["value"], "Message Box");
    assert_eq!(element["native"], 1);

    let missing = registry
        .call(
            &mut session,
            "elements.get",
            json!({ "form_id": "01003274", "path": "NOPE" }),
        )
        .unwrap_err();
    assert_eq!(missing.code, "unknown_element");
}

#[test]
fn commands_need_a_session() {
    let error = Registry::standard()
        .call(&mut Session::default(), "session.info", json!({}))
        .unwrap_err();
    assert_eq!(error.code, "no_session");
}
