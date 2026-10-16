// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `session.load`: the session a `serve` or MCP client keeps open takes
//! another plugin list (and a tool mode's load settings), which is how the
//! quick clean modes run from a daemon.

use std::path::PathBuf;

use serde_json::json;
use xedit_session::{Registry, Session};

/// The Skyrim SE data folder, or `None` when the machine has no corpus.
fn sse_data() -> Option<PathBuf> {
    let data = PathBuf::from(std::env::var_os("XEDIT_SSE_DATA")?);
    data.join("Update.esm").is_file().then_some(data)
}

#[test]
fn a_session_loads_another_plugin_list_with_a_modes_settings() {
    let Some(data) = sse_data() else { return };
    // The corpus tests share the process-wide settings.
    let _lock = xedit_core::interface::globals::test_lock();
    let update = data.join("Update.esm");
    let mut session = Session::load("sse", &[]).expect("a session with no plugin");
    let mut session = {
        session.allow_edit(true);
        session
    };
    let registry = Registry::standard();

    // The dry run reports the plugins, the mode and its settings without
    // loading anything.
    let response = registry
        .call(
            &mut session,
            "session.load",
            json!({
                "plugins": [update.to_string_lossy()],
                "mode": "quickautoclean",
                "dry_run": true
            }),
        )
        .expect("a dry run");
    assert_eq!(response["dry_run"], true);
    assert_eq!(response["files"][0], "Update.esm");
    let settings = response["settings"]
        .as_array()
        .expect("the settings of the mode")
        .iter()
        .map(|setting| setting.as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert!(
        settings.iter().any(|setting| setting.contains("wbSimpleRecords")),
        "{settings:?}"
    );

    // The load itself: the session's plugins are the ones of the request.
    let response = registry
        .call(
            &mut session,
            "session.load",
            json!({
                "plugins": [update.to_string_lossy()],
                "mode": "quickautoclean"
            }),
        )
        .expect("the load");
    assert_eq!(response["dry_run"], false);
    let files: Vec<String> = response["files"]
        .as_array()
        .expect("the loaded files")
        .iter()
        .map(|file| file.as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(files.iter().any(|file| file == "Update.esm"), "{files:?}");

    // The quick clean mode loads as xEdit's does now, so `clean --quick` no
    // longer warns that the plugins were loaded generically.
    let response = registry
        .call(&mut session, "files.clean", json!({ "quick": true, "dry_run": true }))
        .expect("the quick clean count");
    let messages: Vec<String> = response["messages"]
        .as_array()
        .expect("the messages")
        .iter()
        .map(|message| message.as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(
        messages
            .iter()
            .all(|message| !message.contains("not loaded as the quick clean mode")),
        "{messages:?}"
    );
}

/// A session without a game cannot load: the mode is the session's.
#[test]
fn a_session_load_needs_a_session() {
    let mut session = Session::default();
    session.allow_edit(true);
    let error = Registry::standard()
        .call(&mut session, "session.load", json!({ "plugins": [] }))
        .expect_err("no game");
    assert_eq!(error.code, "no_session");
}

/// A plugin that is not there fails the load with `load_failed`: the load
/// clears the file list before it loads, so the session has no plugins
/// afterwards and the error says so.
#[test]
fn a_failed_load_leaves_the_session_empty() {
    let Some(data) = sse_data() else { return };
    let _lock = xedit_core::interface::globals::test_lock();
    let update = data.join("Update.esm");
    let mut session = Session::load("sse", &[update.to_string_lossy().into_owned()]).expect("the first load");
    session.allow_edit(true);
    let registry = Registry::standard();
    let error = registry
        .call(
            &mut session,
            "session.load",
            json!({ "plugins": [format!("{}/NotThere.esp", data.display())] }),
        )
        .expect_err("a plugin that is not there");
    assert_eq!(error.code, "load_failed");
    assert!(error.message.contains("no plugins now"), "{}", error.message);
    let info = registry
        .call(&mut session, "session.info", json!({}))
        .expect("the session");
    assert_eq!(info["files"].as_array().expect("the loaded files").len(), 0, "{info}");
}
