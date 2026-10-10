// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The tool modes of `xeInit.pas` on synthetic Skyrim SE plugins: the mode
//! list (`tool.modes`), the modes that change the module flags, mark the
//! header or write a sequence file, the check modes that count, the legacy
//! command line of a mod manager invocation, and the dry runs.

use std::sync::Arc;

use serde_json::json;

use xedit_core::implementation::{FileBytes, FileImpl, wb_file_from_bytes};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::{Element, File, FileStates, GameMode};
use xedit_session::tool_modes::{self, LegacyRun};
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

fn group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(label);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

/// A file header with the masters and the record count given.
fn header(flags: u32, masters: &[&str], record_count: u32, next_object_id: u32) -> Vec<u8> {
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
        data.extend(sub_record(b"DATA", &[0; 8]));
    }
    data.extend(sub_record(b"INCC", &0u32.to_le_bytes()));
    main_record(b"TES4", flags, 0, &data)
}

/// A master with one keyword, which the plugins override.
fn master() -> Vec<u8> {
    let mut bytes = header(1, &[], 2, 0x900);
    bytes.extend(group(
        b"KYWD",
        &main_record(b"KYWD", 0, 0x0000_0801, &sub_record(b"EDID", b"KeywordA\0")),
    ));
    bytes
}

/// A plugin that overrides the keyword of `master()` with the same data (an
/// identical-to-master record) and adds a start-game-enabled quest.
fn plugin(flags: u32, quest_flags: u16) -> Vec<u8> {
    let mut bytes = header(flags, &["Master.esm"], 3, 0x900);
    bytes.extend(group(
        b"KYWD",
        &main_record(b"KYWD", 0, 0x0000_0801, &sub_record(b"EDID", b"KeywordA\0")),
    ));
    // The `DNAM` of a quest: the flags first, which hold "Start Game
    // Enabled" as bit 0.
    let mut dnam = quest_flags.to_le_bytes().to_vec();
    dnam.extend_from_slice(&[0; 12]);
    let mut quest = sub_record(b"EDID", b"TestQuest\0");
    quest.extend(sub_record(b"DNAM", &dnam));
    bytes.extend(group(b"QUST", &main_record(b"QUST", 0, 0x0100_0802, &quest)));
    bytes
}

fn open(name: &str, bytes: Vec<u8>) -> Arc<FileImpl> {
    wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap()
}

/// A session over the master and the plugin, as `--game sse --load` makes.
fn open_session(plugin_bytes: Vec<u8>) -> Session {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let master = open("Master.esm", master());
    let plugin = open("Plugin.esp", plugin_bytes);
    xedit_core::interface::globals::set_data_path(
        &std::env::temp_dir()
            .join("xedit-tool-modes")
            .with_extension("")
            .to_string_lossy(),
    );
    Session::with_files(GameMode::gmSSE, vec![master, plugin])
}

fn call(session: &mut Session, params: serde_json::Value) -> serde_json::Value {
    Registry::standard()
        .call(session, "tool.run", params)
        .unwrap_or_else(|error| panic!("{error}"))
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xedit-tool-modes-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_mode_list_names_every_mode_with_its_switch() {
    let _guard = test_lock();
    let mut session = open_session(plugin(0, 1));
    let result = Registry::standard()
        .call(&mut session, "tool.modes", json!({}))
        .unwrap();
    let modes = result["modes"].as_array().unwrap();
    assert_eq!(modes.len(), 17, "the seventeen modes of xeInit.pas");
    let by_name = |name: &str| {
        modes
            .iter()
            .find(|mode| mode["mode"] == name)
            .unwrap_or_else(|| panic!("{name} is missing"))
            .clone()
    };
    assert_eq!(by_name("setesm")["switch"], "-setESM");
    assert_eq!(by_name("setesm")["plugin_mode"], true);
    assert_eq!(by_name("masterupdate")["supported"], false, "not a Skyrim SE mode");
    assert_eq!(by_name("onamupdate")["supported"], true);
    assert_eq!(by_name("checkforitm")["command"], "files.clean");
}

#[test]
fn setesm_changes_nothing_as_the_release_does() {
    let _guard = test_lock();
    let mut session = open_session(plugin(0, 1));
    session.allow_edit(true);
    let result = call(&mut session, json!({ "mode": "setesm" }));
    assert_eq!(result["files"], json!([]));
    assert!(result["note"].as_str().unwrap().contains("changes nothing"));
    // The flag is untouched.
    let plugin = session_file(&session);
    assert!(!plugin.get_is_esm());
}

#[test]
fn espify_clears_the_esm_flag_and_saves() {
    let _guard = test_lock();
    let dir = scratch("espify");
    let output = dir.join("Plugin.esp");
    let mut session = open_session(plugin(1, 1));
    session.allow_edit(true);
    // The plugin is an ESM-flagged `.esp`, which is what the mode clears.
    assert!(session_file(&session).get_is_esm());
    let result = call(
        &mut session,
        json!({ "mode": "clearesm", "output": output.to_string_lossy() }),
    );
    assert_eq!(result["changed"], true, "{result}");
    assert_eq!(result["files"][0]["saved"], true);
    // The saved file has the flag cleared, read back.
    let saved = std::fs::read(&output).unwrap();
    assert_eq!(saved[8] & 1, 0, "the ESM flag of the header is cleared");
}

#[test]
fn onamupdate_marks_the_header_of_a_plugin_with_masters() {
    let _guard = test_lock();
    let dir = scratch("onam");
    let output = dir.join("Plugin.esp");
    let mut session = open_session(plugin(0, 1));
    session.allow_edit(true);
    let result = call(
        &mut session,
        json!({ "mode": "onamupdate", "output": output.to_string_lossy() }),
    );
    assert_eq!(result["changed"], true, "{result}");
    assert_eq!(result["files"][0]["name"], "Plugin.esp");
    assert_eq!(result["files"][0]["saved"], true);
    let messages: Vec<&str> = result["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|line| line.as_str())
        .collect();
    assert!(
        messages.iter().any(|line| line.starts_with("Updating ONAM in: ")),
        "{messages:?}"
    );
}

#[test]
fn generateseq_writes_the_fixed_form_ids_of_the_quests() {
    let _guard = test_lock();
    let dir = scratch("seq");
    let mut session = open_session(plugin(0, 1));
    session.allow_edit(true);
    let result = call(
        &mut session,
        json!({
            "mode": "generateseq",
            "files": ["Plugin.esp"],
            "seq_path": dir.to_string_lossy()
        }),
    );
    let seq = &result["seq"][0];
    assert_eq!(seq["plugin"], "Plugin.esp");
    assert_eq!(seq["form_ids"], json!(["01000802"]));
    let path = dir.join("Plugin.seq");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes, 0x0100_0802u32.to_le_bytes(), "one FormID, little endian");

    // A quest that is not start-game-enabled is left out.
    let dir = scratch("seq-off");
    let mut session = open_session(plugin(0, 0));
    session.allow_edit(true);
    let result = call(
        &mut session,
        json!({
            "mode": "generateseq",
            "files": ["Plugin.esp"],
            "seq_path": dir.to_string_lossy()
        }),
    );
    // No sequence file, so the field is left out of the response.
    assert_eq!(result["seq"], serde_json::Value::Null);
    assert!(!dir.join("Plugin.seq").exists());
}

#[test]
fn the_itm_check_counts_the_records_identical_to_their_master() {
    let _guard = test_lock();
    let mut session = open_session(plugin(0, 1));
    session.allow_edit(true);
    let result = call(&mut session, json!({ "mode": "checkforitm" }));
    assert_eq!(result["count"], 1, "{result}");
    assert_eq!(result["exit_code"], 1, "the exit code is the count");
    let messages: Vec<&str> = result["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|line| line.as_str())
        .collect();
    assert!(
        messages
            .iter()
            .any(|line| line.starts_with("[Counting \"Identical to Master\" records done]")),
        "{messages:?}"
    );
    // The mode counts and changes nothing.
    let record = session_file(&session)
        .records()
        .into_iter()
        .find(|record| record.get_signature().to_string() == "KYWD")
        .unwrap();
    assert!(record.get_file().is_some());
}

#[test]
fn a_dry_run_reports_and_writes_nothing() {
    let _guard = test_lock();
    let dir = scratch("dry");
    let output = dir.join("Plugin.esp");
    let mut session = open_session(plugin(1, 1));
    session.allow_edit(true);
    let result = call(
        &mut session,
        json!({ "mode": "clearesm", "output": output.to_string_lossy(), "dry_run": true }),
    );
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["changed"], true, "what the run would change");
    assert_eq!(result["files"][0]["saved"], false);
    assert!(!output.exists(), "a dry run writes nothing");
    assert!(session_file(&session).get_is_esm(), "and changes nothing");
}

#[test]
fn a_mode_the_build_does_not_run_has_stable_errors() {
    let _guard = test_lock();
    let mut session = open_session(plugin(0, 1));
    let registry = Registry::standard();
    let error = registry
        .call(&mut session, "tool.run", json!({ "mode": "lodgen", "dry_run": true }))
        .unwrap_err();
    assert_eq!(error.code, "unsupported");
    assert!(error.message.contains("lodgen.generate"), "{}", error.message);
    let error = registry
        .call(&mut session, "tool.run", json!({ "mode": "nonsense", "dry_run": true }))
        .unwrap_err();
    assert_eq!(error.code, "invalid_params");
}

#[test]
fn the_legacy_command_line_of_a_mod_manager_is_read() {
    let dir = scratch("legacy");
    std::fs::write(dir.join("MyMod.esp"), b"").unwrap();
    let params: Vec<String> = [
        "-SSE",
        &format!("-D:{}", dir.display()),
        "-quickautoclean",
        "-autoexit",
        "-autoload",
        "MyMod.esp",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    let run: LegacyRun = tool_modes::parse_legacy(&params, "xedit.exe")
        .unwrap()
        .expect("a legacy command line");
    assert_eq!(run.game_tag.as_deref(), Some("sse"));
    assert_eq!(run.mode, "quick_auto_clean");
    assert!(run.auto_exit && run.auto_load);
    assert_eq!(run.modules, ["MyMod.esp"]);
    assert_eq!(run.plugins.len(), 1);
    assert!(run.plugins[0].ends_with("MyMod.esp"));

    // A command line of the CLI's own syntax is not one of xEdit's.
    let params: Vec<String> = ["--game", "sse", "--load", "x", "conflicts"]
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect();
    assert!(tool_modes::parse_legacy(&params, "xedit.exe").unwrap().is_none());

    // `-setesm` is a plugin mode: without a plugin it is refused.
    let params: Vec<String> = ["-SSE", "-setesm"].iter().map(|arg| (*arg).to_owned()).collect();
    let error = tool_modes::parse_legacy(&params, "xedit.exe").unwrap_err();
    assert!(error.contains("requires a valid plugin name"), "{error}");
}

/// The plugin of the session, out of the process-wide file list.
fn session_file(_session: &Session) -> Arc<FileImpl> {
    xedit_core::interface::files()
        .into_iter()
        .filter_map(|file| file.as_element_impl().and_then(|inner| inner.file_impl()))
        .find(|file| file.get_name().starts_with("Plugin"))
        .expect("the plugin")
}
