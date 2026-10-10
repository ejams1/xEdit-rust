// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `script.*` commands: the script folder of `script.list`, its
//! listing of the `*.pas` files with their line counts, the scripts of the
//! oracle's `Edit Scripts` when `XEDIT_ORACLE_DIR` is set, and the run loop
//! of `script.run` -- `Initialize`/`Process`/`Finalize` over the selected
//! elements, the `FunctionIsEmpty` skip, the `ScriptProcessElements`/
//! `ScriptProcessSignatures` filter hooks, the self-termination, and the
//! `Aborted: Applying script` text of a script that raises.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use xedit_core::implementation::{FileBytes, FileImpl, wb_file_from_bytes};
use xedit_core::interface::globals::GameMode;
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
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

//=== script.run: the run loop ================================================

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

fn group(label: u32, group_type: i32, records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(&label.to_le_bytes());
    bytes.extend_from_slice(&group_type.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

fn header(flags: u32, record_count: u32, next_object_id: u32, masters: &[&str]) -> Vec<u8> {
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
        data.extend(sub_record(b"DATA", &0u64.to_le_bytes()));
    }
    data.extend(sub_record(b"INCC", &0u32.to_le_bytes()));
    main_record(b"TES4", flags, 0, &data)
}

/// A game setting with an editor ID.
fn gmst(form_id: u32, editor_id: &[u8]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"DATA", &1.5f32.to_le_bytes()));
    main_record(b"GMST", 0, form_id, &data)
}

fn open(name: &str, bytes: Vec<u8>) -> Arc<FileImpl> {
    wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap()
}

/// A session over a master with two game settings and a plugin with one
/// override and one of its own: four main records in all.
fn open_session() -> Session {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let mut master = header(1, 2, 0x802, &[]);
    master.extend(group(
        u32::from_le_bytes(*b"GMST"),
        0,
        &[gmst(0x800, b"fMaster\0"), gmst(0x801, b"fOther\0")].concat(),
    ));
    let mut plugin = header(0, 2, 0x802, &["Master.esm"]);
    plugin.extend(group(
        u32::from_le_bytes(*b"GMST"),
        0,
        &[gmst(0x0100_0800, b"fOverride\0"), gmst(0x0100_0801, b"fPlugin\0")].concat(),
    ));
    let master = open("Master.esm", master);
    let plugin = open("Plugin.esp", plugin);
    let mut session = Session::with_files(GameMode::gmSSE, vec![master, plugin]);
    session.allow_edit(true);
    session
}

/// A scripts folder of its own for a test, holding the script.
fn script_folder(name: &str, file: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xedit-script-run-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(file), text).unwrap();
    dir
}

fn run(session: &mut Session, folder: &Path, file: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut params = json!({
        "file": file,
        "scripts": folder.display().to_string(),
        "save": false,
    });
    for (key, value) in extra.as_object().unwrap() {
        params[key] = value.clone();
    }
    Registry::standard()
        .call(session, "script.run", params)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// The messages without the `[mm:ss]` prefixes the lines of the action carry.
fn lines(result: &serde_json::Value) -> Vec<String> {
    result["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| {
            let line = line.as_str().unwrap();
            match line.strip_prefix('[') {
                Some(rest) => match rest.split_once("] ") {
                    Some((time, text)) if time.contains(':') => text.to_owned(),
                    _ => line.to_owned(),
                },
                None => line.to_owned(),
            }
        })
        .collect()
}

#[test]
fn the_loop_runs_initialize_process_and_finalize() {
    let _guard = test_lock();
    let folder = script_folder(
        "loop",
        "Loop.pas",
        "unit Loop;\n\
         function Initialize: Integer;\n\
         begin\n\
           AddMessage('initialize');\n\
         end;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           AddMessage('process ' + Name(e));\n\
         end;\n\
         function Finalize: Integer;\n\
         begin\n\
           AddMessage('finalize');\n\
           Result := 0;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Loop.pas", json!({}));

    assert_eq!(result["script"], "Loop");
    assert_eq!(result["aborted"], false);
    assert_eq!(
        result["processed"], 6,
        "every main record of the loaded files, the file headers included"
    );
    let lines = lines(&result);
    assert_eq!(lines[0], "Start: Applying script \"Loop\"");
    assert_eq!(lines[1], "initialize");
    // The walk of one file runs backwards -- `GetLast`, then `GetPrevious` to
    // the file node -- so the last record of a file is processed first, the
    // file header after its records (the header is the first node of the
    // file), and the files in load order.
    assert_eq!(lines[2], "process GMST - Game Setting [00000801] <fOther>");
    assert_eq!(lines[3], "process GMST - Game Setting [00000800] <fMaster>");
    assert_eq!(lines[4], "process TES4 - Main File Header [00000000]");
    assert_eq!(lines[5], "process GMST - Game Setting [01000801] <fPlugin>");
    assert_eq!(lines[6], "process GMST - Game Setting [01000800] <fOverride>");
    assert_eq!(lines[7], "process TES4 - Main File Header [00000000]");
    assert_eq!(lines[8], "finalize");
    assert_eq!(
        lines[9],
        "Done: Applying script \"Loop\", Processed Records: 6, Elapsed Time: 00:00"
    );
    assert_eq!(lines[10], "You can close this application now.");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_routine_of_only_result_zero_is_empty_and_skipped() {
    let _guard = test_lock();
    // `FunctionIsEmpty` reads the text: `begin Result := 0; end` does nothing
    // and is not called, whatever it holds.
    let folder = script_folder(
        "empty",
        "Empty.pas",
        "unit Empty;\n\
         function Initialize: Integer;\n\
         begin\n\
           Result := 0;\n\
         end;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
         end;\n\
         function Finalize: Integer;\n\
         begin\n\
           Result := 0;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Empty.pas", json!({}));
    assert_eq!(result["processed"], 0);
    assert_eq!(result["aborted"], false);
    let lines = lines(&result);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(
        lines[1],
        "Done: Applying script \"Empty\", Processed Records: 0, Elapsed Time: 00:00"
    );
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_self_terminated_initialize_stops_the_run_with_the_upstream_message() {
    let _guard = test_lock();
    let folder = script_folder(
        "terminate",
        "Terminate.pas",
        "unit Terminate;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           AddMessage('process');\n\
         end;\n\
         function Initialize: Integer;\n\
         begin\n\
           AddMessage('initialize');\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Terminate.pas", json!({}));
    assert_eq!(result["self_terminated"], 1);
    assert_eq!(result["processed"], 0, "Process never ran");
    let lines = lines(&result);
    assert_eq!(lines[1], "initialize");
    assert_eq!(lines[2], "Script terminated itself, Result=1");
    // The run finished (the `Exit` of the anonymous method), so the closing
    // line of `DoRunScript` follows.
    assert_eq!(lines[4], "You can close this application now.");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn an_error_aborts_the_script_with_the_upstream_text() {
    let _guard = test_lock();
    let folder = script_folder(
        "error",
        "Planted.pas",
        "unit Planted;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           AddMessage('before');\n\
           NoSuchIdentifier;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Planted.pas", json!({}));
    assert_eq!(result["aborted"], true);
    let error = result["error"].as_str().unwrap();
    assert!(error.contains("Undeclared Identifier 'NoSuchIdentifier'"), "{error}");
    assert!(
        result["error_location"]
            .as_str()
            .unwrap()
            .starts_with("unit Planted line "),
        "{:?}",
        result["error_location"]
    );
    let lines = lines(&result);
    assert_eq!(lines[1], "before");
    assert!(
        lines[2].starts_with(
            "Exception in unit Planted line 1: [EJvInterpreterError] Error in unit 'Planted' on line 1 : Undeclared Identifier"
        ),
        "{:?}",
        lines[2]
    );
    assert_eq!(lines[3], "Aborted: Applying script \"Planted\"");
    assert_eq!(lines.len(), 4, "an aborted run never reaches the closing line");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_script_process_elements_hook_selects_the_files() {
    let _guard = test_lock();
    let folder = script_folder(
        "files",
        "Files.pas",
        "unit Files;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           AddMessage(Name(e));\n\
         end;\n\
         function Initialize: Integer;\n\
         begin\n\
           ScriptProcessElements := [etFile];\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Files.pas", json!({}));
    assert_eq!(result["processed"], 2, "one element per loaded file");
    let lines = lines(&result);
    assert_eq!(lines[1], "[00] Master.esm");
    assert_eq!(lines[2], "[01] Plugin.esp");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_script_process_signatures_hook_filters_the_records() {
    let _guard = test_lock();
    // The list is upper-cased when it is stored, so the lower case of the
    // script matches the signature of the record (`ContainsStr`).
    let folder = script_folder(
        "signatures",
        "Signatures.pas",
        "unit Signatures;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           AddMessage(Name(e));\n\
         end;\n\
         function Initialize: Integer;\n\
         begin\n\
           ScriptProcessSignatures := 'gmst';\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Signatures.pas", json!({}));
    assert_eq!(result["processed"], 4, "every record is a GMST");
    let _ = std::fs::remove_dir_all(&folder);

    let folder = script_folder(
        "signatures-none",
        "Signatures.pas",
        "unit Signatures;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           AddMessage('processed');\n\
         end;\n\
         function Initialize: Integer;\n\
         begin\n\
           ScriptProcessSignatures := 'XXXX';\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Signatures.pas", json!({}));
    assert_eq!(result["processed"], 0, "no record has that signature");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_signature_that_is_not_four_characters_is_an_interpreter_error() {
    let _guard = test_lock();
    let folder = script_folder(
        "signatures-bad",
        "Bad.pas",
        "unit Bad;\n\
         function Initialize: Integer;\n\
         begin\n\
           ScriptProcessSignatures := 'TOOLONG';\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Bad.pas", json!({}));
    assert_eq!(result["aborted"], true);
    let error = result["error"].as_str().unwrap();
    assert!(
        error.contains("ScriptProcessSignatures expects a comma separated list of 4 character signatures"),
        "{error}"
    );
    assert!(
        error.contains("Invalid type of argument in call to function"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_filter_variables_are_recorded_and_apply_filter_runs() {
    let _guard = test_lock();
    let folder = script_folder(
        "filter",
        "Filter.pas",
        "unit Filter;\n\
         function Initialize: Integer;\n\
         begin\n\
           FilterByName := True;\n\
           FilterName := 'fMaster';\n\
           InheritConflictByParent := True;\n\
           ApplyFilter;\n\
           if FilterApplied then AddMessage('applied') else AddMessage('not applied');\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Filter.pas", json!({}));
    let lines = lines(&result);
    assert!(lines.contains(&"applied".to_owned()), "{lines:?}");
    assert_eq!(result["aborted"], false);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_tool_mode_and_the_legacy_switch_run_the_same_script() {
    let _guard = test_lock();
    let folder = script_folder(
        "tool-mode",
        "Mode.pas",
        "unit Mode;\n\
         function Initialize: Integer;\n\
         begin\n\
           AddMessage('the mode ran the script');\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = Registry::standard()
        .call(
            &mut session,
            "tool.run",
            json!({
                "mode": "script",
                "script": folder.join("Mode.pas").display().to_string(),
                "scripts": folder.display().to_string()
            }),
        )
        .unwrap();
    let messages: Vec<&str> = result["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap())
        .collect();
    assert!(
        messages.iter().any(|line| line.ends_with("the mode ran the script")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|line| line.contains("Start: Applying script \"Mode\"")),
        "{messages:?}"
    );
    assert_eq!(result["mode"], "script");

    // The mode needs a script.
    let error = Registry::standard()
        .call(&mut session, "tool.run", json!({ "mode": "script" }))
        .unwrap_err();
    assert_eq!(error.code, "invalid_params");

    // `-script:` names it on xEdit's own command line, and `-S:` the folder.
    let dir = script_folder("legacy", "MyScript.pas", "unit MyScript; end.\n");
    std::fs::write(dir.join("MyMod.esp"), b"").unwrap();
    let params: Vec<String> = [
        "-SSE".to_owned(),
        format!("-D:{}", dir.display()),
        format!("-S:{}", dir.display()),
        format!("-script:{}", dir.join("MyScript.pas").display()),
        "MyMod.esp".to_owned(),
    ]
    .to_vec();
    let legacy = xedit_session::tool_modes::parse_legacy(&params, "xedit.exe")
        .unwrap()
        .expect("a legacy command line");
    assert_eq!(legacy.tool_mode, xedit_core::interface::globals::ToolMode::tmScript);
    assert_eq!(legacy.mode, "script");
    assert_eq!(
        legacy.script.as_deref(),
        Some(dir.join("MyScript.pas").display().to_string().as_str())
    );
    assert_eq!(
        legacy.scripts_folder.as_deref(),
        Some(dir.display().to_string().as_str())
    );
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_runs_the_script_and_writes_nothing() {
    let _guard = test_lock();
    let folder = script_folder(
        "dry",
        "Dry.pas",
        "unit Dry;\n\
         function Initialize: Integer;\n\
         begin\n\
           AddMessage('dry run');\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Dry.pas", json!({ "dry_run": true }));
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["aborted"], false);
    assert!(result["files"].as_array().unwrap().is_empty());
    let lines = lines(&result);
    assert!(lines.contains(&"dry run".to_owned()), "{lines:?}");
    assert!(session.edit_allowed(), "the edit flag is restored");

    // The registry gate: without `--edit` the run needs `dry_run`.
    let error = Registry::standard()
        .call(&mut Session::default(), "script.run", json!({ "file": "Dry.pas" }))
        .unwrap_err();
    assert_eq!(error.code, "edit_required");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_script_that_does_not_compile_reports_the_interpreter_error() {
    let _guard = test_lock();
    let folder = script_folder(
        "compile",
        "Broken.pas",
        "unit Broken;\nfunction x: integer;\nbegin\n  if then\nend;\nend.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Broken.pas", json!({}));
    assert_eq!(result["aborted"], true);
    assert!(
        result["error"].as_str().unwrap().contains("expected"),
        "{:?}",
        result["error"]
    );
    assert!(
        result["messages"].as_array().unwrap().is_empty(),
        "the script never started"
    );

    // A script that is not there: `DoRunScript`'s message.
    let error = Registry::standard()
        .call(
            &mut session,
            "script.run",
            json!({ "file": "NoSuchScript.pas", "scripts": folder.display().to_string() }),
        )
        .unwrap_err();
    assert_eq!(error.code, "io");
    assert!(error.message.starts_with("Could not open script:"), "{}", error.message);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_script_that_uses_another_script_runs_it() {
    let _guard = test_lock();
    let folder = script_folder(
        "uses",
        "UsesScript.pas",
        "unit UsesScript;\n\
         uses Helper;\n\
         function Initialize: Integer;\n\
         begin\n\
           AddMessage(Greeting());\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    // The unit beside the script is read by the host (`unit_source`), and its
    // routine is called by the script: a bare constant of another unit is not
    // a bare identifier of the script, as upstream.
    std::fs::write(
        folder.join("Helper.pas"),
        "unit Helper;\n\
         function Greeting: string;\n\
         begin\n\
           Result := 'hello from the unit';\n\
         end;\n\
         end.\n",
    )
    .unwrap();
    let mut session = open_session();
    let result = run(&mut session, &folder, "UsesScript.pas", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", result["error"]);
    let lines = lines(&result);
    assert_eq!(lines[1], "hello from the unit");
    let _ = std::fs::remove_dir_all(&folder);
}

/// The corpus scripts that need no element API, run end to end: the oracle's
/// `Edit Scripts` is needed, so the test does nothing without
/// `XEDIT_ORACLE_DIR`.
#[test]
fn the_corpus_scripts_run_end_to_end() {
    let _guard = test_lock();
    let Some(oracle) = std::env::var_os("XEDIT_ORACLE_DIR") else {
        return;
    };
    let folder = PathBuf::from(oracle).join("Edit Scripts");

    // `List loaded plugins and their masters.pas`: the host functions
    // `FileCount`, `FileByIndex`, `Name`, `MasterCount`, `MasterByIndex` and
    // `AddMessage` over the loaded files.
    let mut session = open_session();
    let result = Registry::standard()
        .call(
            &mut session,
            "script.run",
            json!({
                "file": folder.join("List loaded plugins and their masters.pas").display().to_string(),
                "save": false
            }),
        )
        .unwrap();
    let run_lines = lines(&result);
    assert_eq!(
        run_lines[0],
        "Start: Applying script \"List loaded plugins and their masters\""
    );
    assert_eq!(run_lines[1], "[00] Master.esm");
    assert_eq!(run_lines[2], "[01] Plugin.esp");
    assert_eq!(run_lines[3], "      [00] Master.esm");
    assert_eq!(run_lines[4], "Script terminated itself, Result=1");
    assert_eq!(run_lines.last().unwrap(), "You can close this application now.");

    // `Apply filter for cleaning.pas`: every filter variable through
    // `OnSetValue`, then `ApplyFilter`.
    let mut session = open_session();
    let result = Registry::standard()
        .call(
            &mut session,
            "script.run",
            json!({
                "file": folder.join("Apply filter for cleaning.pas").display().to_string(),
                "save": false
            }),
        )
        .unwrap();
    assert_eq!(result["aborted"], false);
    assert_eq!(result["self_terminated"], 1);
    let run_lines = lines(&result);
    assert!(
        run_lines
            .iter()
            .any(|line| line == "Script terminated itself, Result=1"),
        "{run_lines:?}"
    );
    assert_eq!(run_lines.last().unwrap(), "You can close this application now.");
}
