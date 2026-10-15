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
    // file), and the files in load order. A run's names are the short form
    // (`wbDisplayShorterNames`, `xeMainForm.pas:5152`): `EditorID
    // "Full Name" [SIG:FormID]`.
    assert_eq!(lines[2], "process fOther [GMST:00000801]");
    assert_eq!(lines[3], "process fMaster [GMST:00000800]");
    assert_eq!(lines[4], "process [TES4:00000000]");
    assert_eq!(lines[5], "process fPlugin [GMST:01000801]");
    assert_eq!(lines[6], "process fOverride [GMST:01000800]");
    assert_eq!(lines[7], "process [TES4:00000000]");
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
    // No `Process` fired (`FunctionIsEmpty` skips the walk), so
    // `wbCurrentProgress` was never set and the closing line carries no
    // `Processed Records` segment (`xeMainForm.pas:15298-15300`).
    assert_eq!(lines[1], "Done: Applying script \"Empty\", Elapsed Time: 00:00");
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
    // `FLastError.Assign` never copies the unit or the line of an interpreter
    // error (`JvInterpreter.pas:1491-1503`), so `GetLastErrorLocation` keeps
    // the cleared `unit  line -1` -- the oracle's shape.
    assert_eq!(result["error_location"].as_str().unwrap(), "unit  line -1");
    let lines = lines(&result);
    assert_eq!(lines[1], "before");
    assert!(
        lines[2].starts_with(
            "Exception in unit  line -1: [EJvInterpreterError] Error in unit 'Planted' on line 5 : Undeclared Identifier"
        ),
        "{:?}",
        lines[2]
    );
    assert_eq!(lines[3], "Aborted: Applying script \"Planted\"");
    assert_eq!(lines.len(), 4, "an aborted run never reaches the closing line");
    let _ = std::fs::remove_dir_all(&folder);
}

/// The review's runaway-recursion probe: the interpreter's call-depth guard
/// reports the overflow the way the oracle's `ApplyScript` does when the
/// native stack runs out (`Exception in unit ...: [EStackOverflow] Stack
/// overflow`, `xeMainForm.pas:8847-8855`), and the session survives it.
#[test]
fn a_runaway_recursion_aborts_with_the_oracle_stack_overflow() {
    let _guard = test_lock();
    // 128 interpreted calls of a debug build need more than the test
    // thread's 2 MiB (the CLI gives its session a large stack too).
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(runaway_recursion_body)
        .unwrap()
        .join()
        .unwrap();
}

fn runaway_recursion_body() {
    let folder = script_folder(
        "recurse",
        "Rec.pas",
        "unit Rec;\r\n\
         function R: Integer;\r\n\
         begin\r\n\
           Result := R;\r\n\
         end;\r\n\
         function Initialize: Integer;\r\n\
         begin\r\n\
           Result := R;\r\n\
         end;\r\n\
         end.\r\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Rec.pas", json!({}));
    assert_eq!(result["aborted"], true);
    assert_eq!(result["error"].as_str().unwrap(), "Stack overflow");
    let location = result["error_location"].as_str().unwrap();
    assert_eq!(location, "unit Rec line 4");
    let lines = lines(&result);
    assert_eq!(lines[0], "Start: Applying script \"Rec\"");
    assert_eq!(
        lines[1],
        "Exception in unit Rec line 4: [EStackOverflow] Stack overflow"
    );
    assert_eq!(lines[2], "Aborted: Applying script \"Rec\"");
    assert_eq!(lines.len(), 3, "an aborted run never reaches the closing line");
    // The session survives the abort, as the oracle's GUI does.
    let result = run(&mut session, &folder, "Rec.pas", json!({}));
    assert_eq!(result["aborted"], true);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A refused edit aborts, as upstream's `ElementEditValues[...] := value`
/// (`TwbSubRecord.SetEditValue`, `wbImplementation.pas:16444-16468`) does;
/// the discarded setter error made it a silent no-op.
#[test]
fn a_refused_edit_aborts_with_the_upstream_message() {
    let _guard = test_lock();
    let folder = script_folder(
        "refused",
        "Refuse.pas",
        "unit Refuse;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           if EditorID(e) = 'fMaster' then\n\
             SetElementEditValues(e, 'DATA', '2.5');\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Refuse.pas", json!({}));
    assert_eq!(result["aborted"], true, "{:?}", lines(&result));
    assert_eq!(result["error"].as_str().unwrap(), "DATA - Value can not be edited");
    let lines = lines(&result);
    // The refusal is upstream's `Exception.Create(GetName + ' can not be
    // edited')` (`wbImplementation.pas:16444-16468`); the script is read as
    // `TStringList.LoadFromFile` reads it, so its LF line breaks become the
    // CRLF ones `GetLineByPos` counts (`JvJCLUtils.pas:1358`): line 5.
    assert_eq!(
        lines[1],
        "Exception in unit Refuse line 5: [Exception] DATA - Value can not be edited"
    );
    assert_eq!(lines[2], "Aborted: Applying script \"Refuse\"");
    let _ = std::fs::remove_dir_all(&folder);
}

/// The legitimate set still writes: the same surface over the plugin's own
/// record reads its value back.
#[test]
fn a_legitimate_set_writes() {
    let _guard = test_lock();
    let folder = script_folder(
        "legit-set",
        "Set.pas",
        "unit Set;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           if EditorID(e) = 'fPlugin' then begin\n\
             SetElementEditValues(e, 'EDID', 'fRenamed');\n\
             AddMessage('read ' + GetElementEditValues(e, 'EDID'));\n\
           end;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Set.pas", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert!(
        lines(&result).iter().any(|line| line == "read fRenamed"),
        "{:?}",
        lines(&result)
    );
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

/// A main record's node children are its child group alone, never its own
/// element container (`NodeData.Container := GroupRecord`,
/// `xeMainForm.pas:20285-20288`): a selection of `etSubRecord` has no node
/// to fire on -- the GUI tree holds no subrecords -- where the pre-fix walk
/// descended each record's elements and delivered them (the review probe
/// over SSE `Update.esm`: 4,373,854 processed against the oracle's 0).
#[test]
fn a_subrecord_selection_processes_nothing() {
    let _guard = test_lock();
    let folder = script_folder(
        "subrecord",
        "Subrecords.pas",
        "unit Subrecords;\n\
         var\n\
           count: Integer;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           count := count + 1;\n\
         end;\n\
         function Initialize: Integer;\n\
         begin\n\
           ScriptProcessElements := [etSubRecord];\n\
           count := 0;\n\
         end;\n\
         function Finalize: Integer;\n\
         begin\n\
           AddMessage('subrecords=' + IntToStr(count));\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Subrecords.pas", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert_eq!(result["processed"], 0);
    let lines = lines(&result);
    assert_eq!(lines[1], "subrecords=0");
    // `Process` never fired: the closing line has no `Processed Records`.
    assert_eq!(lines[2], "Done: Applying script \"Subrecords\", Elapsed Time: 00:00");
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

//=== The data format adapter (phase 6 step 6) ================================

/// A script over the `wbDataFormat` surface: it builds a NIF, writes it, its
/// JSON and a DDS header, clears and rebuilds the file from the JSON, and
/// reports every step through `AddMessage`.
#[test]
fn a_script_builds_a_nif_through_the_data_format_adapter() {
    let _guard = test_lock();
    let dir = temp_dir("df-nif");
    let nif_path = dir.join("built.nif");
    let json_path = dir.join("built.json");
    let dds_path = dir.join("built.dds");
    let text = format!(
        "unit DfBuild;\n\
         function Initialize: Integer;\n\
         var\n\
           nif: TwbNifFile;\n\
           node, child: TwbNifBlock;\n\
           dds: TwbDDSFile;\n\
         begin\n\
           nif := TwbNifFile.Create;\n\
           nif.SetToDefault;\n\
           nif.NifVersion := nfSSE;\n\
           AddMessage('version: ' + IntToStr(nif.NifVersion));\n\
           node := nif.AddBlock('NiNode');\n\
           node.EditValues['Name'] := 'Root';\n\
           child := node.AddChild('NiNode');\n\
           child.EditValues['Name'] := 'Child';\n\
           AddMessage('blocks: ' + IntToStr(nif.BlocksCount));\n\
           child := nif.Blocks[1];\n\
           AddMessage('child: ' + child.EditValues['Name']);\n\
           AddMessage('path: ' + child.Path);\n\
           AddMessage('refs: ' + IntToStr(node.RefsCount));\n\
           if node.IsNiObject('NiNode', True) then AddMessage('object: True') else AddMessage('object: False');\n\
           nif.SaveToFile('{nif}');\n\
           nif.SaveToJSONFile('{json}', True);\n\
           nif.SetToDefault;\n\
           AddMessage('cleared: ' + IntToStr(nif.BlocksCount));\n\
           nif.LoadFromJSONFile('{json}');\n\
           AddMessage('rebuilt: ' + IntToStr(nif.BlocksCount));\n\
           child := nif.Blocks[1];\n\
           AddMessage('rebuilt child: ' + child.EditValues['Name']);\n\
           nif.Free;\n\
           dds := TwbDDSFile.Create;\n\
           dds.SetToDefault;\n\
           AddMessage('dds mips: ' + IntToStr(dds.NativeValues['HEADER\\dwMipMapCount']));\n\
           dds.SaveToFile('{dds}');\n\
           dds.Free;\n\
           Result := 1;\n\
         end;\n\
         end.\n",
        nif = nif_path.display(),
        json = json_path.display(),
        dds = dds_path.display(),
    );
    let folder = script_folder("df-build", "DfBuild.pas", &text);
    let mut session = open_session();
    let result = run(&mut session, &folder, "DfBuild.pas", json!({}));
    assert_eq!(result["aborted"], false, "{:?} {:?}", result["error"], lines(&result));
    assert_eq!(result["self_terminated"], 1);
    let build_lines = lines(&result);
    let expected = [
        "version: 5",
        "blocks: 2",
        "child: Child",
        "path: 1 NiNode",
        "object: True",
        "cleared: 0",
        "rebuilt: 2",
        "rebuilt child: Child",
        "dds mips: 0",
    ];
    for line in expected {
        assert!(
            build_lines.contains(&line.to_owned()),
            "{line} missing of {build_lines:?}"
        );
    }
    let refs = build_lines
        .iter()
        .find(|line| line.starts_with("refs: "))
        .expect("a refs line");
    let refs: i32 = refs.trim_start_matches("refs: ").parse().expect("a number");
    assert!(refs > 0, "{build_lines:?}");

    // The files the script wrote: the NIF carries the SSE magic, the JSON
    // the rebuilt block, the DDS the header of the empty file.
    let nif = std::fs::read(&nif_path).expect("the NIF");
    assert!(
        nif.starts_with(b"Gamebryo File Format, Version 20.2.0.7"),
        "{:?}",
        String::from_utf8_lossy(&nif[..40.min(nif.len())])
    );
    let json = std::fs::read_to_string(&json_path).expect("the JSON");
    assert!(json.contains("Child"), "{json}");
    assert!(dds_path.is_file(), "the DDS header was written");

    // `LoadFromResource`: the `dfResourceGetDataCallback` of the GUI is the
    // container handler, so the folder the file was written to is added as a
    // container and the resource name finds it. A name no container has
    // raises the oracle's message.
    xedit_core::container_handler::add_folder(&dir);
    let resource = script_folder(
        "df-resource",
        "DfResource.pas",
        "unit DfResource;\n\
         function Initialize: Integer;\n\
         var\n\
           nif: TwbNifFile;\n\
         begin\n\
           nif := TwbNifFile.Create;\n\
           nif.LoadFromResource('built.nif');\n\
           AddMessage('resource blocks: ' + IntToStr(nif.BlocksCount));\n\
           nif.Free;\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &resource, "DfResource.pas", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", result["error"]);
    let resource_lines = lines(&result);
    assert!(
        resource_lines.contains(&"resource blocks: 2".to_owned()),
        "{resource_lines:?}"
    );

    let missing = script_folder(
        "df-missing",
        "DfMissing.pas",
        "unit DfMissing;\n\
         function Initialize: Integer;\n\
         var\n\
           nif: TwbNifFile;\n\
         begin\n\
           nif := TwbNifFile.Create;\n\
           nif.LoadFromResource('no\\such\\mesh.nif');\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &missing, "DfMissing.pas", json!({}));
    assert_eq!(result["aborted"], true);
    let error = result["error"].as_str().unwrap();
    assert!(
        error.contains("Resource not found (File \"no\\such\\mesh.nif\")"),
        "{error}"
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_dir_all(&resource);
    let _ = std::fs::remove_dir_all(&missing);
}

/// The corpus NIF of the oracle's `Edit Scripts` (`LODGen_flat_lod.nif`, a
/// Skyrim mesh of 6 blocks), walked through the adapter: needs
/// `XEDIT_ORACLE_DIR`, so the test does nothing without it.
#[test]
fn a_corpus_nif_walks_through_the_data_format_adapter() {
    let _guard = test_lock();
    let Some(oracle) = std::env::var_os("XEDIT_ORACLE_DIR") else {
        return;
    };
    let corpus = PathBuf::from(&oracle).join("Edit Scripts").join("LODGen_flat_lod.nif");
    let text = format!(
        "unit DfCorpus;\n\
         function Initialize: Integer;\n\
         var\n\
           nif: TwbNifFile;\n\
           block: TwbNifBlock;\n\
           el: TdfElement;\n\
           i: Integer;\n\
         begin\n\
           nif := TwbNifFile.Create;\n\
           nif.LoadFromFile('{file}');\n\
           AddMessage('version: ' + IntToStr(nif.NifVersion));\n\
           AddMessage('blocks: ' + IntToStr(nif.BlocksCount));\n\
           for i := 0 to nif.BlocksCount - 1 do begin\n\
             block := nif.Blocks[i];\n\
             AddMessage('block: ' + block.BlockType);\n\
           end;\n\
           block := nif.Blocks[0];\n\
           AddMessage('name: ' + block.EditValues['Name']);\n\
           AddMessage('node: ' + block.IsNiObject('NiNode', True));\n\
           AddMessage('shape: ' + nif.Blocks[1].IsNiObject('NiTriBasedGeom', True));\n\
           AddMessage('header magic: ' + nif.Header.EditValues['Magic']);\n\
           block := nif.Blocks[4];\n\
           el := block.Elements['Textures'];\n\
           AddMessage('texture 0: ' + el[0].EditValue);\n\
           AddMessage('texture 0 name: ' + el[0].Name);\n\
           nif.Free;\n\
           Result := 1;\n\
         end;\n\
         end.\n",
        file = corpus.display(),
    );
    let folder = script_folder("df-corpus", "DfCorpus.pas", &text);
    let mut session = open_session();
    let result = run(&mut session, &folder, "DfCorpus.pas", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", result["error"]);
    let lines = lines(&result);
    for line in [
        "version: 4",
        "blocks: 6",
        "block: NiNode",
        "block: NiTriShape",
        "block: NiTriShapeData",
        "block: BSLightingShaderProperty",
        "block: BSShaderTextureSet",
        "block: NiAlphaProperty",
        "name: Scene Root",
        "node: True",
        "shape: True",
        "header magic: Gamebryo File Format, Version 20.2.0.7",
        "texture 0 name: Textures #0",
    ] {
        assert!(lines.contains(&line.to_owned()), "{line} missing of {lines:?}");
    }
    let _ = std::fs::remove_dir_all(&folder);
}

//=== The JVCL RTL units (phase 6 step 8) ====================================

/// `BoolToStr` is not a script function of the oracle (none of the units it
/// builds registers one), so the script spells it out.
const RTL_BOOL_HELPER: &str = "\
function BoolToStr2(b: Boolean): string;
begin
  if b then Result := 'True' else Result := 'False';
end;
";

/// The `System`, `SysUtils`, `Math` and `Classes` units in one script: every
/// `Show` line is a value the RTL computes, and the test reads the exact
/// lines back, so a regression in one function shows as the line that
/// changed.
const RTL_SCRIPT: &str = "\
procedure Show(const s: string);
begin
  AddMessage(s);
end;
function Initialize: Integer;
var
  i, j, k: Integer;
  f: Double;
  s: string;
  arr: array[0..2] of integer;
  sl, sl2: TStringList;
  lst: TList;
  ms: TMemoryStream;
  fs: TFileStream;
  fh, fsize: Integer;
begin
  Result := 0;
  // System
  Show('upcase=' + UpCase('x'));
  Show('chr=' + Chr(65));
  Show('ord=' + IntToStr(Ord(65)));
  Show('abs=' + IntToStr(Abs(-7)));
  Show('round=' + IntToStr(Round(2.5)) + ',' + IntToStr(Round(3.5)));
  Show('trunc=' + IntToStr(Trunc(-2.7)));
  Show('sqrt=' + FloatToStr(Sqrt(16)));
  Show('sqr=' + FloatToStr(Sqr(1.5)));
  Show('pos=' + IntToStr(Pos('c', 'abcd')));
  Show('copy=' + Copy('abcdef', 2, 3) + ',' + Copy('abcdef', 0, 2));
  s := 'abcdef';
  Delete(s, 2, 2);
  Show('delete=' + s);
  Insert('XY', s, 1);
  Show('insert=' + s);
  Show('varto=' + VarToStr(12) + BoolToStr2(VarIsEmpty(1)) + BoolToStr2(VarIsNull(Null)));
  SetLength(s, 3);
  Show('setlength=' + IntToStr(Length(s)));
  arr[0] := 5; arr[1] := 6; arr[2] := 7;
  Show('array=' + IntToStr(Length(arr)) + ',' + IntToStr(arr[0]) + ',' + IntToStr(arr[2]));
  Show('bounds=' + IntToStr(Low(arr)) + ',' + IntToStr(High(arr)));
  DeleteFromArray(arr, 0);
  InsertIntoArray(arr, 0, 9);
  Show('array2=' + IntToStr(Length(arr)) + ',' + IntToStr(arr[0]));
  // SysUtils
  Show('case=' + UpperCase('aBc') + ',' + LowerCase('aBc'));
  Show('trim=' + Trim('  x  ') + ',');
  Show('strtoint=' + IntToStr(StrToInt('42')) + ',' + IntToStr(StrToIntDef('x', 7)));
  Show('hex=' + IntToHex(255, 4));
  Show('format=' + Format('%s: %d %.4d', ['n', 12, 7]));
  Show('float=' + FloatToStr(0.5) + ',' + FloatToStr(StrToFloat('2.25')));
  Show('quoted=' + QuotedStr('a''b'));
  Show('ident=' + BoolToStr2(IsValidIdent('a1b')) + BoolToStr2(IsValidIdent('1a')));
  Show('path=' + ExtractFileName('a\\b\\c.d') + ',' + ExtractFileExt('a\\b\\c.d') + ',' + ChangeFileExt('c.d', '.e'));
  Show('date=' + FormatDateTime('yyyy-mm-dd', EncodeDate(2024, 2, 29)));
  DecodeDate(EncodeDate(2024, 2, 29), i, j, k);
  Show('dateday=' + IntToStr(DayOfWeek(EncodeDate(2024, 2, 29))) + ',' + BoolToStr2(IsLeapYear(2024)));
  // Math
  Show('math=' + IntToStr(Floor(2.7)) + ',' + IntToStr(Ceil(2.1)) + ',' + IntToStr(Max(3, 5)) + ',' + IntToStr(Min(3, 5)));
  Show('power=' + FloatToStr(Power(2, 10)) + ',' + FloatToStr(IntPower(3, 3)));
  // Classes: TStringList
  sl := TStringList.Create;
  sl.Add('beta');
  sl.Add('alpha');
  sl.Add('gamma');
  Show('sl=' + IntToStr(sl.Count) + ',' + sl[0] + ',' + IntToStr(sl.IndexOf('alpha')));
  sl.Sort;
  Show('sorted=' + sl[0] + ',' + sl[2]);
  sl.Values['k'] := 'v';
  Show('values=' + sl.Values['k'] + ',' + sl.Names[3]);
  sl.CommaText := 'a,b,c';
  Show('comma=' + sl.CommaText + ',' + IntToStr(sl.Count));
  sl.Text := 'one' + #13#10 + 'two';
  Show('text=' + IntToStr(sl.Count) + ',' + sl[1]);
  sl2 := TStringList.Create;
  sl2.Sorted := True;
  sl2.Duplicates := dupIgnore;
  sl2.Add('b');
  sl2.Add('a');
  sl2.Add('a');
  Show('sortedlist=' + IntToStr(sl2.Count) + ',' + sl2[0] + ',' + sl2[1]);
  sl2.SaveToFile(ScriptsPath + 'rtl-test.txt');
  sl2.LoadFromFile(ScriptsPath + 'rtl-test.txt');
  Show('roundtrip=' + IntToStr(sl2.Count) + ',' + sl2[1]);
  ms := TMemoryStream.Create;
  sl2.SaveToStream(ms);
  Show('stream=' + IntToStr(ms.Size));
  ms.Free;
  sl.Free;
  sl2.Free;
  // Classes: TList
  lst := TList.Create;
  lst.Add('x');
  lst.Add('y');
  Show('list=' + IntToStr(lst.Count) + ',' + lst[0] + ',' + IntToStr(lst.IndexOf('y')) + ',' + lst.Last);
  lst.Delete(0);
  Show('list2=' + IntToStr(lst.Count) + ',' + lst[0]);
  lst.Free;
  // SysUtils + Classes: a file behind a TFileStream
  fh := FileCreate(ScriptsPath + 'rtl-test.bin');
  FileClose(fh);
  fs := TFileStream.Create(ScriptsPath + 'rtl-test.bin', fmOpenReadWrite);
  fs.Write('abc', 3);
  Show('file=' + IntToStr(fs.Size) + ',' + IntToStr(fs.Position));
  fs.Free;
  Show('filesize=' + BoolToStr2(FileAge(ScriptsPath + 'rtl-test.bin') <> -1));
  DeleteFile(ScriptsPath + 'rtl-test.bin');
  // SysUtils: exceptions
  try
    raise Exception.Create('boom');
  except
    on E: Exception do
      Show('except=' + E.Message);
  end;
  // SysUtils: the picture formatter
  Show('formatfloat=' + FormatFloat('0.00', 1.005));
  f := 1.0;
  fsize := 0;
  Show('done=' + FloatToStr(f) + IntToStr(fsize));
end;
end.
";

#[test]
fn the_jvcl_rtl_units_run_a_script() {
    let _guard = test_lock();
    let source = format!("unit Rtl;\n{RTL_BOOL_HELPER}\n{RTL_SCRIPT}");
    let folder = script_folder("Rtl", "Rtl.pas", &source);
    let mut session = open_session();
    let result = run(&mut session, &folder, "Rtl.pas", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    let lines = lines(&result);
    let expected = [
        "upcase=X",
        "chr=A",
        "ord=65",
        "abs=7",
        // `Round` is half to even, as the RTL's.
        "round=2,4",
        "trunc=-2",
        "sqrt=4",
        "sqr=2.25",
        "pos=3",
        // `Copy` clamps a zero index to 1 and keeps the count (the oracle's
        // `Copy('abcdef', 0, 3)` is `abc`; probes `x-010`,
        // `edid-quick-init.pas`).
        "copy=bcd,ab",
        "delete=adef",
        "insert=XYadef",
        // `VarToStr(12)` is `12`, `VarIsEmpty(1)` False, `VarIsNull(Null)` True.
        "varto=12FalseTrue",
        "setlength=3",
        "array=3,5,7",
        "bounds=0,2",
        "array2=3,9",
        "case=ABC,abc",
        "trim=x,",
        "strtoint=42,7",
        "hex=00FF",
        "format=n: 12 0007",
        "float=0.5,2.25",
        "quoted='a''b'",
        "ident=TrueFalse",
        "path=c.d,.d,c.e",
        "date=2024-02-29",
        // 2024-02-29 was a Thursday (5); 2024 is a leap year.
        "dateday=5,True",
        "math=2,3,5,3",
        "power=1024,27",
        "sl=3,beta,1",
        "sorted=alpha,gamma",
        "values=v,k",
        "comma=a,b,c,3",
        "text=2,two",
        "sortedlist=2,a,b",
        "roundtrip=2,b",
        // `sl2` holds "a\r\n" and "b\r\n".
        "stream=6",
        "list=2,x,1,y",
        "list2=1,y",
        "file=3,3",
        "filesize=True",
        "except=boom",
        "formatfloat=1.00",
        "done=10",
    ];
    let section: Vec<String> = lines.iter().skip(1).take(expected.len()).cloned().collect();
    let expected: Vec<String> = expected.iter().map(|line| (*line).to_owned()).collect();
    assert_eq!(section, expected, "{lines:?}");
}

/// The corpus scripts the JVCL units of this step unblock: each one needs
/// `System`/`SysUtils`/`Math`/`Classes` calls and runs headless over the
/// two-plugin session to its `Done:` line. Needs the oracle's `Edit
/// Scripts`, so the test does nothing without `XEDIT_ORACLE_DIR`.
#[test]
fn the_corpus_scripts_that_need_the_jvcl_units_run() {
    let _guard = test_lock();
    let Some(oracle) = std::env::var_os("XEDIT_ORACLE_DIR") else {
        return;
    };
    let folder = PathBuf::from(oracle).join("Edit Scripts");
    // Each entry: the script, and a line its run must produce.
    let scripts = [
        ("List perk entry points usage.pas", None),
        ("List LOD objects and meshes.pas", None),
        ("Check for invalid alternate textures.pas", None),
        ("Worldspace scale refs position.pas", None),
        ("Skyrim - List used scripts.pas", Some("No scripts found.")),
    ];
    for (name, line) in scripts {
        if !folder.join(name).is_file() {
            continue;
        }
        let mut session = open_session();
        let result = run(&mut session, &folder, name, json!({}));
        let lines = lines(&result);
        assert_eq!(result["aborted"], false, "{name}: {lines:?}");
        assert!(
            lines.iter().any(|entry| entry.starts_with("Done: Applying script")),
            "{name}: {lines:?}"
        );
        if let Some(line) = line {
            assert!(lines.iter().any(|entry| entry == line), "{name}: {lines:?}");
        }
    }
}

//=== The run-parity fixups ==================================================

/// A main record whose child group follows it in the same container: the
/// GUI's tree hides the group's sibling node and hangs its elements on the
/// record (`vstNavInitNode`, `xeMainForm.pas:20228-20347`), so every record
/// of a child group is reached exactly once. The pre-fixup walk descended
/// the group through both paths and double-counted them (the parity re-run
/// of `_newscript_.pas` over `Skyrim.esm`: 2,312,470 against the oracle's
/// 869,712).
#[test]
fn a_child_group_is_walked_through_its_record_once() {
    let _guard = test_lock();
    let folder = script_folder(
        "child-group",
        "Walk.pas",
        "unit Walk;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           AddMessage('process ' + Name(e));\n\
         end;\n\
         end.\n",
    );
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let mut master = header(1, 3, 0x902, &[]);
    // A CELL whose child group (type 6, labeled with the cell's FormID)
    // follows the record in the same top group, as the on-disk structure
    // nests it.
    let mut cell = main_record(b"CELL", 0, 0x900, &sub_record(b"EDID", b"CellA\0"));
    // The reference carries its required `NAME` and `DATA`: a record whose
    // subrecords all precede `QuickInitLimit` loses them when its name is
    // read first (the quick init collapse, parity #23), which is not what
    // this test is about.
    let mut refr = sub_record(b"EDID", b"RefA\0");
    refr.extend(sub_record(b"NAME", &0u32.to_le_bytes()));
    refr.extend(sub_record(b"DATA", &[0u8; 24]));
    cell.extend(group(0x900, 6, &main_record(b"REFR", 0, 0x901, &refr)));
    master.extend(group(u32::from_le_bytes(*b"CELL"), 0, &cell));
    master.extend(group(u32::from_le_bytes(*b"GMST"), 0, &gmst(0x800, b"fMaster\0")));
    let master = open("Master.esm", master);
    let mut session = Session::with_files(GameMode::gmSSE, vec![master]);
    session.allow_edit(true);
    let result = run(&mut session, &folder, "Walk.pas", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert_eq!(
        lines(&result),
        vec![
            "Start: Applying script \"Walk\"",
            // The reverse flat tree: the GMST group, then the cell, its
            // child record (once), and the file header. The REFR carries
            // its `AdditionalInfoFor` suffix (`(Places ...)`), as the
            // oracle's names do.
            "process fMaster [GMST:00000800]",
            "process RefA [REFR:00000901] (Places NULL - Null Reference [00000000] in CellA [CELL:00000900])",
            "process CellA [CELL:00000900]",
            "process [TES4:00000000]",
            "Done: Applying script \"Walk\", Processed Records: 4, Elapsed Time: 00:00",
            "You can close this application now."
        ]
    );
    let _ = std::fs::remove_dir_all(&folder);
}

/// `RemoveNode` (`xejviScriptAdapterHost.pas:330`): the element -- a main
/// record's child group ahead of it -- leaves its container, and the records
/// the walk has not reached yet are unaffected.
#[test]
fn remove_node_takes_the_element_out_of_its_container() {
    let _guard = test_lock();
    let remove_folder = script_folder(
        "remove-node",
        "Remove.pas",
        "unit Remove;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           if EditorID(e) = 'fOther' then\n\
             if RemoveNode(e) then\n\
               AddMessage('removed')\n\
             else\n\
               AddMessage('not removed');\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &remove_folder, "Remove.pas", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert!(
        lines(&result).iter().any(|line| line == "removed"),
        "{:?}",
        lines(&result)
    );
    // The record is gone for the next run over the same session.
    let remaining_folder = script_folder(
        "remove-node-remaining",
        "Remaining.pas",
        "unit Remaining;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           AddMessage('still ' + Name(e));\n\
         end;\n\
         end.\n",
    );
    let result = run(&mut session, &remaining_folder, "Remaining.pas", json!({}));
    assert_eq!(
        lines(&result),
        vec![
            "Start: Applying script \"Remaining\"",
            "still fMaster [GMST:00000800]",
            "still [TES4:00000000]",
            "still fPlugin [GMST:01000801]",
            "still fOverride [GMST:01000800]",
            "still [TES4:00000000]",
            "Done: Applying script \"Remaining\", Processed Records: 5, Elapsed Time: 00:00",
            "You can close this application now."
        ]
    );
    let _ = std::fs::remove_dir_all(&remove_folder);
    let _ = std::fs::remove_dir_all(&remaining_folder);
}

/// `Randomize` and `Beep` take no parameters (upstream registers them
/// `0, []`); calling them with none must not raise `Not enough parameters`
/// (what the parity re-run of `Remove excess references.pas` and
/// `Remove percentage of references of specific object.pas` hit).
#[test]
fn the_zero_argument_rtl_functions_run() {
    let _guard = test_lock();
    let folder = script_folder(
        "zero-args",
        "ZeroArgs.pas",
        "unit ZeroArgs;\n\
         function Initialize: Integer;\n\
         begin\n\
           Randomize;\n\
           Beep;\n\
           AddMessage('ok');\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "ZeroArgs.pas", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert!(lines(&result).iter().any(|line| line == "ok"), "{:?}", lines(&result));
    let _ = std::fs::remove_dir_all(&folder);
}

/// `Skyrim - Convert BOOK to SCRL.pas` line 16: `if not
/// Assigned(baserecord) then` with `baserecord` a unit-level `IInterface`
/// variable. Upstream layers two adapters (`TJvInterpreterExpression.GetValue`:
/// the program's own adapter holds the script's variables and units, the
/// shared registrations answer after it), so the variable wins over the
/// `BaseRecord` registration, whose name matches only case-insensitively.
/// (The corpus case aborted with `Not enough parameters` while the oracle
/// resolved the variable and ran; the fixed lookup reaches the script's own
/// handling of a missing record.)
#[test]
fn a_script_variable_wins_over_a_case_matching_function() {
    let _guard = test_lock();
    let folder = script_folder(
        "not-assigned",
        "NotAssigned.pas",
        "unit NotAssigned;\n\
         var\n\
           baserecord: IInterface;\n\
         function Initialize: Integer;\n\
         begin\n\
           baserecord := RecordByFormID(FileByIndex(0), $000A44B5, True);\n\
           if not Assigned(baserecord) then begin\n\
             AddMessage('can not find base record');\n\
             Result := 1;\n\
             Exit;\n\
           end;\n\
           AddMessage('found');\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "NotAssigned.pas", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert_eq!(result["self_terminated"], 1, "{:?}", lines(&result));
    let lines = lines(&result);
    assert!(lines.iter().any(|line| line == "can not find base record"), "{lines:?}");
    assert!(!lines.iter().any(|line| line == "found"), "{lines:?}");
    let _ = std::fs::remove_dir_all(&folder);
}

/// A probe script of the parity-23 fixtures
/// (`crates/xtask/oracle/scripts/probes`) run over its synthetic master
/// (`tests/fixtures/parity`): the lines of the run between the `Start:` and
/// the `Done:` line.
fn fixture_lines(probe: &str, fixture: &str, build_refs: bool) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let text = std::fs::read_to_string(root.join("crates/xtask/oracle/scripts/probes").join(probe)).unwrap();
    let bytes = std::fs::read(root.join("tests/fixtures/parity").join(fixture)).unwrap();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    // The fixture stands in for the game master, which the parity run edits
    // with `--allow-edit-game-master`.
    xedit_core::interface::globals::set_allow_edit_game_master(true);
    let master = open("Skyrim.esm", bytes);
    let mut session = Session::with_files(GameMode::gmSSE, vec![master]);
    session.allow_edit(true);
    let folder = script_folder(probe.trim_end_matches(".pas"), probe, &text);
    let result = run(&mut session, &folder, probe, json!({"build_refs": build_refs}));
    xedit_core::interface::globals::set_allow_edit_game_master(false);
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    let all = lines(&result);
    let start = all
        .iter()
        .position(|line| line.starts_with("Start: Applying script"))
        .unwrap();
    let end = all
        .iter()
        .position(|line| line.starts_with("Done: Applying script"))
        .unwrap();
    all[start + 1..end].to_vec()
}

/// The lines the oracle wrote for the probe (`cargo xtask parity script
/// --script edid-quick-init`, 2026-10-15, equal): the record read by its
/// name first loses its subrecords (0 elements, the added EDID alone), the
/// one read through `ElementByName` first keeps them and gains its required
/// colour, the AVIF category above 3 becomes 0, `Copy` clamps the index to
/// 1 and keeps the count, and `StrictDelimiter` splits on the delimiter
/// alone.
#[test]
fn the_edid_quick_init_probe_gives_the_oracle_lines() {
    let _guard = test_lock();
    let expected: Vec<&str> = vec![
        "Processing: KwdNoColorNamed [KYWD:00000800]",
        "EDID missing",
        "before: 0 []",
        "after: 1 [EDID - Editor ID]",
        "EDID=KwdNoColorNamed2 CNAM=",
        "Processing: 00000801",
        "EDID present",
        "before: 3 [Record Header, EDID - Editor ID, CNAM - Color]",
        "after: 3 [Record Header, EDID - Editor ID, CNAM - Color]",
        "EDID=KwdNoColorPath2 CNAM=",
        "Processing: KwdColor [KYWD:00000802]",
        "EDID present",
        "before: 3 [Record Header, EDID - Editor ID, CNAM - Color]",
        "after: 3 [Record Header, EDID - Editor ID, CNAM - Color]",
        "EDID=KwdColor2 CNAM=",
        "Processing: ActiBounds [ACTI:00000803]",
        "EDID missing",
        "before: 0 []",
        "after: 1 [EDID - Editor ID]",
        "EDID=ActiBounds2 CNAM=",
        "Processing: ActiFull \"Full Activator\" [ACTI:00000804]",
        "EDID present",
        "before: 6 [Record Header, EDID - Editor ID, OBND - Object Bounds, FULL - Name, PNAM - Marker Color, FNAM - Flags]",
        "after: 6 [Record Header, EDID - Editor ID, OBND - Object Bounds, FULL - Name, PNAM - Marker Color, FNAM - Flags]",
        "EDID=ActiFull2 CNAM=",
        "Processing: AvifBig [AVIF:00000805]",
        "EDID present",
        "before: 4 [Record Header, EDID - Editor ID, DESC - Description, CNAM - Skill Category]",
        "after: 4 [Record Header, EDID - Editor ID, DESC - Description, CNAM - Skill Category]",
        "EDID=AvifBig2 CNAM=None",
        "Processing: AvifSmall [AVIF:00000806]",
        "EDID present",
        "before: 4 [Record Header, EDID - Editor ID, DESC - Description, CNAM - Skill Category]",
        "after: 4 [Record Header, EDID - Editor ID, DESC - Description, CNAM - Skill Category]",
        "EDID=AvifSmall2 CNAM=Magic",
        "Copy0=abc",
        "Copy-1=abc",
        "Copy1=abc",
        "Copy0big=abcdef",
        "Copy7=",
        "Copy6=f",
        "strict=6 [Model\\MODL",
        "Male World Model",
        " \"a",
        "b\" ",
        "",
        "last",
        "]",
        "loose=7 [Model\\MODL",
        "Male",
        "World",
        "Model",
        "a,b",
        "",
        "last",
        "]",
    ];
    assert_eq!(
        fixture_lines("edid-quick-init.pas", "edid-quick-init.esm", false),
        expected
    );
}

/// The lines the oracle wrote for the probe (`cargo xtask parity script
/// --script refs-edit`, 2026-10-15, equal with and without the reference
/// cache): the referenced-by lists follow a replaced, an added and a removed
/// keyword, and the EditorID index follows the renames (a record renamed
/// onto a taken name is reported and not indexed; the name it leaves is
/// not re-taken by the other record), a removed EDID and a record added and
/// removed. The group lookups search the current names.
#[test]
fn the_refs_edit_probe_gives_the_oracle_lines() {
    let _guard = test_lock();
    let expected: Vec<&str> = vec![
        "initial: Kwd000 [KYWD:00000800] referenced by 1 [ActiOne \"One\" [ACTI:00001000]]",
        "initial: Kwd001 [KYWD:00000801] referenced by 2 [ActiOne \"One\" [ACTI:00001000], ActiTwo \"Two\" [ACTI:00001001]]",
        "initial: Kwd002 [KYWD:00000802] referenced by 0 []",
        "initial: Kwd003 [KYWD:00000803] referenced by 0 []",
        "KWDA of ActiOne \"One\" [ACTI:00001000]: ",
        "KWDA after set: ",
        "after set: Kwd001 [KYWD:00000801] referenced by 1 [ActiTwo \"Two\" [ACTI:00001001]]",
        "after set: Kwd002 [KYWD:00000802] referenced by 1 [ActiOne \"One\" [ACTI:00001000]]",
        "KWDA after add: ",
        "after add: Kwd003 [KYWD:00000803] referenced by 1 [ActiOne \"One\" [ACTI:00001000]]",
        "KWDA after remove: ",
        "after remove: Kwd000 [KYWD:00000800] referenced by 0 []",
        "after remove: Kwd002 [KYWD:00000802] referenced by 1 [ActiOne \"One\" [ACTI:00001000]]",
        "group lookup iParityOne -> iParityOne [GMST:00001800]",
        "index lookup iParityOne -> iParityOne [GMST:00001800]",
        "group lookup iparityone -> iParityOne [GMST:00001800]",
        "index lookup iparityone -> iParityOne [GMST:00001800]",
        "group lookup iParityTwo -> iParityTwo [GMST:00001801]",
        "index lookup iParityTwo -> iParityTwo [GMST:00001801]",
        "group lookup iParityTwo -> nil",
        "index lookup iParityTwo -> nil",
        "group lookup iParityRenamed -> iParityRenamed [GMST:00001801]",
        "index lookup iParityRenamed -> iParityRenamed [GMST:00001801]",
        "[Skyrim.esm] Duplicate Key in Index \"EditorID\": \"iParityRenamed\" Existing Record: iParityRenamed [GMST:00001801] New Record: iParityRenamed [GMST:00001802]",
        "group lookup iParityRenamed -> iParityRenamed [GMST:00001801]",
        "index lookup iParityRenamed -> iParityRenamed [GMST:00001801]",
        "group lookup iParityOne -> iParityOne [GMST:00001800]",
        "index lookup iParityOne -> iParityOne [GMST:00001800]",
        "group lookup iParityRenamed -> iParityRenamed [GMST:00001802]",
        "index lookup iParityRenamed -> nil",
        "group lookup iParityTwo -> iParityTwo [GMST:00001801]",
        "index lookup iParityTwo -> iParityTwo [GMST:00001801]",
        "[Skyrim.esm] Duplicate Key in Index \"EditorID\": \"iParityOne\" Existing Record: iParityOne [GMST:00001800] New Record: iParityOne [GMST:00001802]",
        "group lookup iParityOne -> iParityOne [GMST:00001800]",
        "index lookup iParityOne -> iParityOne [GMST:00001800]",
        "group lookup iParityFour -> nil",
        "index lookup iParityFour -> nil",
        "added [GMST:00002000]",
        "group lookup iParityNew -> iParityNew [GMST:00002000]",
        "index lookup iParityNew -> iParityNew [GMST:00002000]",
        "[Skyrim.esm] Duplicate Key in Index \"EditorID\": \"iParityTwo\" Existing Record: iParityTwo [GMST:00001801] New Record: iParityTwo [GMST:00002000]",
        "group lookup iParityTwo -> iParityTwo [GMST:00001801]",
        "index lookup iParityTwo -> iParityTwo [GMST:00001801]",
        "group lookup iParityNew -> nil",
        "index lookup iParityNew -> nil",
        "group lookup iParityTwo -> iParityTwo [GMST:00001801]",
        "index lookup iParityTwo -> iParityTwo [GMST:00001801]",
        "group lookup KwdOne -> KwdOne [KYWD:00000805]",
        "index lookup KwdOne -> nil",
        "group lookup kwdone -> KwdOne [KYWD:00000805]",
        "index lookup kwdone -> nil",
        "group lookup KwdTwo -> KwdTwo [KYWD:00000806]",
        "index lookup KwdTwo -> nil",
        "group lookup KwdTwo -> nil",
        "index lookup KwdTwo -> nil",
        "group lookup KwdRenamed -> KwdRenamed [KYWD:00000806]",
        "index lookup KwdRenamed -> nil",
        "group lookup KwdRenamed -> KwdRenamed [KYWD:00000806]",
        "index lookup KwdRenamed -> nil",
        "group lookup KwdOne -> KwdOne [KYWD:00000805]",
        "index lookup KwdOne -> nil",
        "group lookup KwdRenamed -> KwdRenamed [KYWD:00000807]",
        "index lookup KwdRenamed -> nil",
        "group lookup KwdTwo -> KwdTwo [KYWD:00000806]",
        "index lookup KwdTwo -> nil",
        "group lookup KwdOne -> KwdOne [KYWD:00000805]",
        "index lookup KwdOne -> nil",
        "group lookup KwdFour -> nil",
        "index lookup KwdFour -> nil",
        "added [KYWD:00002001]",
        "group lookup KwdNew -> KwdNew [KYWD:00002001]",
        "index lookup KwdNew -> nil",
        "group lookup KwdTwo -> KwdTwo [KYWD:00000806]",
        "index lookup KwdTwo -> nil",
        "group lookup KwdNew -> nil",
        "index lookup KwdNew -> nil",
        "group lookup KwdTwo -> KwdTwo [KYWD:00000806]",
        "index lookup KwdTwo -> nil",
        "done",
    ];
    assert_eq!(fixture_lines("refs-edit.pas", "refs-edit.esm", true), expected);
}

/// A probe script of the parity fixtures
/// (`crates/xtask/oracle/scripts/probes`) run over the parity fixture
/// master: the lines of the run between the `Start:` and the `Done:` line.
/// The probes write their files into the run folder (`ScriptsPath`), the
/// temporary scripts folder this helper makes.
fn parity_probe_lines(probe: &str) -> Vec<String> {
    let _guard = test_lock();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let text = std::fs::read_to_string(root.join("crates/xtask/oracle/scripts/probes").join(probe)).unwrap();
    let bytes = std::fs::read(root.join("tests/fixtures/parity/edid-quick-init.esm")).unwrap();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let master = open("Skyrim.esm", bytes);
    let mut session = Session::with_files(GameMode::gmSSE, vec![master]);
    session.allow_edit(true);
    let folder = script_folder(probe.trim_end_matches(".pas"), probe, &text);
    let result = run(&mut session, &folder, probe, json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    let all = lines(&result);
    let start = all
        .iter()
        .position(|line| line.starts_with("Start: Applying script"))
        .unwrap();
    let end = all
        .iter()
        .position(|line| line.starts_with("Done: Applying script"))
        .unwrap();
    all[start + 1..end].to_vec()
}

/// The lines the oracle wrote for the streams probe (`cargo xtask parity
/// script --phase run --script probes/streams`, 2026-10-15, equal): the JSON
/// round trips over the memory, bytes, string and file streams with the
/// sizes and positions they leave (81 bytes from position 0, 162 appended,
/// 84 after a save at position 3 that truncates a `TMemoryStream`), the
/// UTF-8 default, the UTF-8 mark and the half-written UTF-16 form
/// `TEncoding.Unicode` gives, the `TBinaryReader`/`TBinaryWriter` round trip
/// of every typed read (the writer has no buffer: the file grows by 4 bytes
/// on the first `Write(42)`), `Read`'s one byte through the encoding and the
/// nil and freed stream exceptions (a save into nil writes nothing, a nil
/// constructor argument is `EArgumentNilException`).
#[test]
fn the_stream_probe_gives_the_oracle_lines() {
    let expected = vec![
        "memory | fresh size=0 pos=0",
        "memory | saved size=81 pos=81",
        "memory | loaded pos=81 name=xEdit count=42",
        "memory | json={\"name\":\"xEdit\",\"count\":42,\"flag\":true,\"pi\":3.5,\"list\":[1,\"two\"],\"sub\":{\"k\":\"v\"}}",
        "memory | appended size=162 pos=162",
        "memory | overwrote size=84 pos=84",
        "memory | mid-load exc=EJsonParserException",
        "empty | exc=EJsonParserException",
        "encoding | utf8 size=16 pos=16",
        "encoding | utf8-bom size=19 pos=19",
        "encoding | utf16 size=18 pos=18",
        "encoding | utf16-load exc=EJsonParserException Unexpected end of file where value was expected (1, 9)",
        "encoding | utf16-enc-load exc=EJsonParserException",
        "encoding | utf16-as-utf8 exc=EJsonParserException",
        "bytes | file size=27 pos=27",
        "bytes | tbytes size=27 pos=0",
        "bytes | load pos=27 name=xEdit count=42",
        "bytes | json={\"name\":\"xEdit\",\"count\":42}",
        "bytes | memory size=27",
        "string | size=16 pos=16 data={\"name\":\"xEdit\"}",
        "string | load pos=16 json={\"name\":\"xEdit\"}",
        "binary | empty size=0 pos=0",
        "binary | int size=4",
        "binary | string ok size=20",
        "binary | buffered size=24",
        "binary | long size=5026",
        "binary | freed size=5026 pos=5026",
        "binary | read-plain=42 type=3 pos=2",
        "binary | read-plain2=0 type=3 pos=4",
        "binary | read-int=42 pos=4",
        "binary | read-int2 exc=EEncodingError No mapping for the Unicode character exists in the target multi-byte code page pos=4",
        "binary | read-double=2.12199578750681E-314 pos=12",
        "binary | read-bool=0",
        "binary | read-string= pos=14",
        "binary | read-single=1.5132080674216E20 pos=18",
        "binary | read-byte=98 pos=19",
        "binary | read-shortint=99",
        "binary | read-smallint=0",
        "binary | read-uint16=16416",
        "binary | read-uint32=825239432",
        "binary | read-char=2",
        "binary | end pos=29 size=5026",
        "binary | past-end=909456435",
        "binary | size=5026",
        "flush | after4 size=5 pos=5",
        "flush | after4096 size=4099 pos=4099",
        "flush | after4097 size=4101 pos=4101",
        "flush | after8192 size=12295 pos=12295",
        "flush | freed size=12295",
        "nil | load exc=EAccessViolation",
        "nil | save ok",
        "nil | reader-create exc=EArgumentNilException",
        "nil | writer-create exc=EArgumentNilException",
        "freed | load exc=EAccessViolation",
    ];
    assert_eq!(parity_probe_lines("streams.pas"), expected);
}

/// The time zone independent lines of the datetime probe (`cargo xtask
/// parity script --phase run --script probes/datetime`, 2026-10-15, equal):
/// the `EncodeDate` day numbers, `DecodeDate`'s truncation and
/// `DecodeTime`'s positive reading of a negative `TDateTime`'s fraction, the
/// offset arithmetic `DateTimeToISO8601` does, the value kinds
/// `DateTimeValue` reads, the invalid date strings (`0`, and
/// `EConvertError` for a date `EncodeDate` refuses). The lines whose text
/// depends on the machine's time zone are asserted as relationships after
/// them, because CI runs in UTC.
#[test]
fn the_datetime_probe_gives_the_oracle_lines() {
    let expected = vec![
        "enc | 1899-12-29=-1 1899-12-30=0 1899-12-31=1",
        "dec | dd(-1.25)=1899-12-29",
        "dec | dd(-0.25)=1899-12-30",
        "dec | dd(-0.75)=1899-12-30",
        "dec | dt(-1.25)=6:0:0.0",
        "dec | dt(-0.79166)=19:0:0.0",
        "dec | dt(-1.79166)=19:0:0.0",
        "off | -1.25-u=-0.791666666666667 -0.25-u=-0.791666666666667 -0.75-u=-1.79166666666667",
        "none | 0",
        "kinds | i=3 l=4 u=5 f=1.5 b=1",
        "types | i=2 s=1 b=8",
        "null | exc=EJsonCastException Cannot cast Object into DateTime",
        "null | types=10",
        "parse-dateonly | value=42036 types=1",
        "parse-empty | value=0 types=1",
        "parse-garbage | value=0 types=1",
        "parse-short | value=0 types=1",
        "parse-zero | exc=EConvertError Invalid argument to date encode",
        "parse-badmonth | exc=EConvertError Invalid argument to date encode",
    ];
    let all = parity_probe_lines("datetime.pas");
    let kept: Vec<String> = all
        .iter()
        .filter(|line| {
            [
                "enc |",
                "dec |",
                "off |",
                "none |",
                "kinds |",
                "types |",
                "null |",
                "parse-empty",
                "parse-garbage",
                "parse-short",
                "parse-zero",
                "parse-badmonth",
                "parse-dateonly",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        })
        .cloned()
        .collect();
    assert_eq!(kept, expected);

    // The conversions do not depend on the zone: `parse-z` and `parse-plain`
    // are the same (a text without a zone is UTC), an offset moves the value
    // by its own size and a date alone carries no time.
    let value = |tag: &str| -> f64 {
        let line = all
            .iter()
            .find(|line| line.starts_with(tag))
            .unwrap_or_else(|| panic!("{tag} is in the probe"));
        line.split("value=")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    let z = value("parse-z");
    assert_eq!(value("parse-plain"), z);
    assert!((value("parse-offset") - (z - 2.0 / 24.0)).abs() < 1e-9);
    assert!((value("parse-neg") - (z + 5.0 / 24.0)).abs() < 1e-9);
    assert_eq!(value("parse-dateonly"), 42036.0);
    // The utc form of a stored value carries the `Z`, its local form the
    // machine's offset; the utc text is the local one moved by that offset.
    for line in &all {
        if let Some(rest) = line.strip_prefix("2015-02-01 | utc=") {
            assert!(rest.ends_with("Z types=6 raw=42036.6724444676"), "{line}");
        }
        if let Some(rest) = line.strip_prefix("2015-02-01 | local=") {
            assert!(rest.starts_with("2015-02-01T16:08:19.202"), "{line}");
            let offset = &rest["2015-02-01T16:08:19.202".len()..];
            assert!(offset.starts_with('+') || offset.starts_with('-'), "{line}");
        }
    }
}
