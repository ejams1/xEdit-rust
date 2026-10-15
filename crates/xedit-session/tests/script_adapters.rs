// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Phase 6 step 5: the registrations of `xejviScriptAdapterElement.pas`,
//! `xejviScriptAdapterRecord.pas`, `xejviScriptAdapterFile.pas`,
//! `xejviScriptAdapterContainer.pas` and `xejviScriptAdapterResource.pas`,
//! exercised by scripts over a synthetic load order.
//!
//! The first half runs small scripts of its own that call the functions of
//! the five units and asserts their values and their effect on the saved
//! plugin bytes; the second half runs scripts of the 4.1.5q corpus that need
//! these units (`XEDIT_ORACLE_DIR`, skipped without it).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use xedit_core::implementation::{FileBytes, FileImpl, wb_file_from_bytes};
use xedit_core::interface::globals::GameMode;
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
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

fn group_label(name: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*name)
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

/// A game setting with an editor ID and a value.
fn gmst(form_id: u32, editor_id: &[u8], value: f32) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"DATA", &value.to_le_bytes()));
    main_record(b"GMST", 0, form_id, &data)
}

fn open(name: &str, bytes: Vec<u8>) -> Arc<FileImpl> {
    wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap()
}

/// A master with three game settings and a plugin with one override, one new
/// record and one record with an editor ID of its own.
fn open_session() -> Session {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let mut master = header(1, 3, 0x802, &[]);
    master.extend(group(
        group_label(b"GMST"),
        0,
        &[
            gmst(0x800, b"fMaster\0", 1.5),
            gmst(0x801, b"fOther\0", 2.5),
            gmst(0x802, b"fThird\0", 3.5),
        ]
        .concat(),
    ));
    let mut plugin = header(0, 3, 0x802, &["Master.esm"]);
    plugin.extend(group(
        group_label(b"GMST"),
        0,
        // An override names the master's FormID (`00000800`); a record of the
        // plugin's own carries its slot (`01000802`).
        &[
            gmst(0x0000_0800, b"fOverride\0", 4.5),
            gmst(0x0100_0802, b"fPlugin\0", 5.5),
        ]
        .concat(),
    ));
    let master = open("Master.esm", master);
    let plugin = open("Plugin.esp", plugin);
    let mut session = Session::with_files(GameMode::gmSSE, vec![master, plugin]);
    session.allow_edit(true);
    session
}

/// A scripts folder of its own for a test, holding the script.
fn script_folder(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xedit-adapter-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.pas")), text).unwrap();
    dir
}

fn run(session: &mut Session, folder: &Path, name: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut params = json!({
        "file": format!("{name}.pas"),
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

/// The index of a subrecord in a plugin's bytes, or `None`.
fn sub_record_at(bytes: &[u8], signature: &[u8]) -> Option<usize> {
    bytes.windows(signature.len()).position(|window| window == signature)
}

/// The loaded plugin, for the assertions on the saved bytes.
fn plugin_file(_session: &Session) -> Arc<FileImpl> {
    use xedit_core::interface::Element;
    xedit_core::interface::files()
        .into_iter()
        .filter_map(|file| file.as_element_impl().and_then(|element| element.file_impl()))
        .find(|file| file.get_name() == "Plugin.esp")
        .expect("the plugin is loaded")
}

/// Runs a script of the corpus of the oracle's `Edit Scripts`; `None`
/// without `XEDIT_ORACLE_DIR`.
fn corpus_folder() -> Option<PathBuf> {
    let oracle = std::env::var_os("XEDIT_ORACLE_DIR")?;
    Some(PathBuf::from(oracle).join("Edit Scripts"))
}

//=== IwbElement =============================================================

const ELEMENT_SCRIPT: &str = "\
unit Elements;
function Process(e: IInterface): Integer;
var
  f: IInterface;
begin
  Result := 0;
  if Signature(e) <> 'GMST' then Exit;
  AddMessage('name=' + Name(e));
  AddMessage('path=' + Path(e));
  AddMessage('full=' + FullPath(e));
  AddMessage('short=' + ShortName(e));
  AddMessage('display=' + DisplayName(e));
  AddMessage('base=' + BaseName(e));
  AddMessage('value=' + GetValue(e));
  AddMessage('edit=' + GetEditValue(e));
  AddMessage('native=' + VarToStr(GetNativeValue(ElementByPath(e, 'DATA\\Float'))));
  AddMessage('datanative=' + VarToStr(GetElementNativeValues(e, 'DATA\\Float')));
  AddMessage('type=' + ElementTypeAsText(e));
  AddMessage('deftype=' + DefTypeAsText(e));
  AddMessage('sig=' + Signature(e));
  AddMessage('edid=' + EditorID(e));
  AddMessage('formid=' + IntToStr(GetLoadOrderFormID(e)));
  AddMessage('count=' + IntToStr(ElementCount(e)));
  AddMessage('byindex=' + Name(ElementByIndex(e, 0)));
  AddMessage('byname=' + Name(ElementByName(e, 'DATA')));
  AddMessage('bysig=' + Name(ElementBySignature(e, 'EDID')));
  AddMessage('bypath=' + Name(ElementByPath(e, 'DATA')));
  AddMessage('exists=' + VarToStr(ElementExists(e, 'EDID')));
  AddMessage('editvals=' + GetElementEditValues(e, 'EDID'));
  AddMessage('count2=' + IntToStr(ElementCount(e)));
  f := GetFile(e);
  AddMessage('file=' + GetFileName(f));
end;
end.
";

#[test]
fn the_element_functions_read_the_tree() {
    let _guard = test_lock();
    let folder = script_folder("Elements", ELEMENT_SCRIPT);
    let mut session = open_session();
    let result = run(&mut session, &folder, "Elements", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert_eq!(
        lines(&result),
        vec![
            "Start: Applying script \"Elements\"",
            "name=fThird [GMST:00000802]",
            "path=GMST",
            // `TwbElement.GetFullPath` (`wbImplementation.pas:19641-19652`):
            // the container chain with the index of every level and the
            // prefixed file name; verified against the oracle.
            "full= \\ [00] Master.esm \\ [1] GRUP Top \"GMST\" \\ [2] fThird [GMST:00000802]",
            // `GetShortNameInternal` with `wbDisplayShorterNames` off, and
            // `GetDisplayName`'s fallback to the summary for a\ GMST without a
            // `FULL` subrecord.
            "short=fThird [GMST:00000802]",
            "display=3.5",
            "base=fThird [GMST:00000802]",
            // `TwbMainRecord.GetValue` is empty; `GetEditValue` is the FormID
            // and `DefType` of a record is its record definition.
            "value=",
            "edit=00000802",
            // The native value of the `Float` member of the `DATA` union:
            // `TwbFloatDef.ToValue` with six `RoundToEx` digits through the
            // single-precision `10^digit` factor, the ported upstream quirk;
            // the oracle prints the same shape (`399.999999990097` for a
            // `400.0` game setting, probe of 4.1.5q, 2026-10-10), so no
            // single rounding is applied at the native value.
            "native=3.49999999116335",
            "datanative=3.49999999116335",
            "type=etMainRecord",
            "deftype=dtRecord",
            "sig=GMST",
            "edid=fThird",
            // A Cardinal through `IntToStr` prints its decimal text, as in Delphi.
            "formid=2050",
            // The element count and the first child: upstream's main record
            // keeps the `Record Header` among its elements (`TwbElement.
            // Create` adds every child to its container,
            // `wbImplementation.pas:19316-19322`; `TwbMainRecord.Init`
            // creates the header with the record as its container,
            // `:10453-10465`; `TwbContainer.GetElementCount` is the child
            // array, `:7664-7669`), so both match the oracle's script output.
            "count=3",
            "byindex=Record Header",
            // `ElementByName` is a name or display name lookup upstream too:
            // the subrecord's name is `DATA - Value`, so `DATA` finds none.
            "byname=",
            "bysig=EDID - Editor ID",
            "bypath=DATA - Value",
            "exists=True",
            "editvals=fThird",
            "count2=3",
            "file=Master.esm",
            "name=fOther [GMST:00000801]",
            "path=GMST",
            "full= \\ [00] Master.esm \\ [1] GRUP Top \"GMST\" \\ [1] fOther [GMST:00000801]",
            "short=fOther [GMST:00000801]",
            "display=2.5",
            "base=fOther [GMST:00000801]",
            "value=",
            "edit=00000801",
            "native=2.49999999368811",
            "datanative=2.49999999368811",
            "type=etMainRecord",
            "deftype=dtRecord",
            "sig=GMST",
            "edid=fOther",
            "formid=2049",
            "count=3",
            "byindex=Record Header",
            "byname=",
            "bysig=EDID - Editor ID",
            "bypath=DATA - Value",
            "exists=True",
            "editvals=fOther",
            "count2=3",
            "file=Master.esm",
            "name=fMaster [GMST:00000800]",
            "path=GMST",
            "full= \\ [00] Master.esm \\ [1] GRUP Top \"GMST\" \\ [0] fMaster [GMST:00000800]",
            "short=fMaster [GMST:00000800]",
            "display=1.5",
            "base=fMaster [GMST:00000800]",
            "value=",
            "edit=00000800",
            "native=1.49999999621286",
            "datanative=1.49999999621286",
            "type=etMainRecord",
            "deftype=dtRecord",
            "sig=GMST",
            "edid=fMaster",
            "formid=2048",
            "count=3",
            "byindex=Record Header",
            "byname=",
            "bysig=EDID - Editor ID",
            "bypath=DATA - Value",
            "exists=True",
            "editvals=fMaster",
            "count2=3",
            "file=Master.esm",
            "name=fPlugin [GMST:01000802]",
            "path=GMST",
            "full= \\ [01] Plugin.esp \\ [1] GRUP Top \"GMST\" \\ [1] fPlugin [GMST:01000802]",
            "short=fPlugin [GMST:01000802]",
            "display=5.5",
            "base=fPlugin [GMST:01000802]",
            "value=",
            "edit=01000802",
            "native=5.49999998611383",
            "datanative=5.49999998611383",
            "type=etMainRecord",
            "deftype=dtRecord",
            "sig=GMST",
            "edid=fPlugin",
            "formid=16779266",
            "count=3",
            "byindex=Record Header",
            "byname=",
            "bysig=EDID - Editor ID",
            "bypath=DATA - Value",
            "exists=True",
            "editvals=fPlugin",
            "count2=3",
            "file=Plugin.esp",
            "name=fOverride [GMST:00000800]",
            "path=GMST",
            "full= \\ [01] Plugin.esp \\ [1] GRUP Top \"GMST\" \\ [0] fOverride [GMST:00000800]",
            "short=fOverride [GMST:00000800]",
            "display=4.5",
            "base=fOverride [GMST:00000800]",
            "value=",
            "edit=00000800",
            "native=4.49999998863859",
            "datanative=4.49999998863859",
            "type=etMainRecord",
            "deftype=dtRecord",
            "sig=GMST",
            "edid=fOverride",
            "formid=2048",
            "count=3",
            "byindex=Record Header",
            "byname=",
            "bysig=EDID - Editor ID",
            "bypath=DATA - Value",
            "exists=True",
            "editvals=fOverride",
            "count2=3",
            "file=Plugin.esp",
            "Done: Applying script \"Elements\", Processed Records: 7, Elapsed Time: 00:00",
            "You can close this application now.",
        ]
    );
    let _ = std::fs::remove_dir_all(&folder);
}

const CONTAINER_SCRIPT: &str = "\
unit Containers;
function Process(e: IInterface): Integer;
var
  c: IInterface;
begin
  Result := 0;
  if Signature(e) <> 'GMST' then Exit;
  c := GetContainer(ElementBySignature(e, 'EDID'));
  AddMessage('container=' + Name(c));
  AddMessage('last=' + Name(LastElement(e)));
  AddMessage('indexof=' + IntToStr(IndexOf(e, ElementBySignature(e, 'DATA'))));
  AddMessage('equals=' + VarToStr(Equals(e, e)));
  AddMessage('containing=' + Name(ContainingMainRecord(ElementBySignature(e, 'EDID'))));
end;
end.
";

#[test]
fn the_container_functions_walk_an_element() {
    let _guard = test_lock();
    let folder = script_folder("Containers", CONTAINER_SCRIPT);
    let mut session = open_session();
    let result = run(&mut session, &folder, "Containers", json!({}));
    // The script runs to the end: integers and booleans go through
    // `IntToStr`/`VarToStr`, because a string beside a non-string raises
    // `Type mismatch` (parity #28), so a bare concatenation would abort.
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    let lines = lines(&result);
    assert_eq!(lines[1], "container=fThird [GMST:00000802]");
    assert_eq!(lines[2], "last=DATA - Value");
    assert_eq!(lines[3], "indexof=2");
    assert_eq!(lines[4], "equals=True");
    assert_eq!(lines[5], "containing=fThird [GMST:00000802]");
    let _ = std::fs::remove_dir_all(&folder);
}

//=== IwbElement: the edits, and what they save ==============================

const EDIT_SCRIPT: &str = "\
unit Edits;
function Process(e: IInterface): Integer;
begin
  Result := 0;
  if Signature(e) <> 'GMST' then Exit;
  if EditorID(e) <> 'fPlugin' then Exit;
  SetEditValue(ElementBySignature(e, 'EDID'), 'fRenamed');
  SetElementEditValues(e, 'EDID', 'fRenamed');
  SetElementNativeValues(e, 'DATA\\Float', 9.5);
  AddMessage('renamed=' + EditorID(e) + ' value=' + GetElementEditValues(e, 'DATA\\Float'));
end;
end.
";

#[test]
fn the_element_setters_change_the_record() {
    let _guard = test_lock();
    let folder = script_folder("Edits", EDIT_SCRIPT);
    let mut session = open_session();
    let plugin = plugin_file(&session);
    let result = run(&mut session, &folder, "Edits", json!({}));
    let lines = lines(&result);
    assert!(
        lines.iter().any(|line| line == "renamed=fRenamed value=9.500000"),
        "{lines:?}"
    );
    // The change is on the file, which the next save writes.
    let saved = plugin
        .write_to_bytes(xedit_core::implementation::ResetModified::rmSetInternal)
        .unwrap();
    let edid = sub_record_at(&saved, b"fRenamed").unwrap();
    assert_eq!(&saved[edid..edid + 8], b"fRenamed");
    assert_eq!(sub_record_at(&saved, b"fPlugin"), None, "the old editor ID is gone");
    let _ = std::fs::remove_dir_all(&folder);
}

//=== IwbMainRecord ==========================================================

const RECORD_SCRIPT: &str = "\
unit Records;
function Process(e: IInterface): Integer;
var
  m: IInterface;
begin
  Result := 0;
  if Signature(e) <> 'GMST' then Exit;
  if EditorID(e) <> 'fMaster' then Exit;
  m := MasterOrSelf(e);
  AddMessage('self=' + Name(m));
  AddMessage('overrides=' + IntToStr(OverrideCount(m)));
  AddMessage('override0=' + Name(OverrideByIndex(m, 0)));
  AddMessage('winning=' + Name(WinningOverride(m)));
  AddMessage('highest=' + Name(HighestOverrideOrSelf(m, 1)));
  AddMessage('ismaster=' + VarToStr(IsMaster(m)));
  AddMessage('iswinning=' + VarToStr(IsWinningOverride(m)));
  AddMessage('version=' + IntToStr(GetFormVersion(m)));
  AddMessage('childgroup=' + Name(ChildGroup(m)));
  AddMessage('deleted=' + VarToStr(GetIsDeleted(m)));
  AddMessage('persistent=' + VarToStr(GetIsPersistent(m)));
  AddMessage('refcount=' + IntToStr(ReferencedByCount(m)));
end;
end.
";

#[test]
fn the_record_functions_walk_the_overrides() {
    let _guard = test_lock();
    let folder = script_folder("Records", RECORD_SCRIPT);
    let mut session = open_session();
    let result = run(&mut session, &folder, "Records", json!({}));
    // The script runs to the end: integers and booleans go through
    // `IntToStr`/`VarToStr`, because a string beside a non-string raises
    // `Type mismatch` (parity #28), so a bare concatenation would abort.
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    let lines = lines(&result);
    assert_eq!(lines[1], "self=fMaster [GMST:00000800]");
    assert_eq!(lines[2], "overrides=1");
    assert_eq!(lines[6], "ismaster=True");
    assert_eq!(lines[7], "iswinning=False");
    let _ = std::fs::remove_dir_all(&folder);
}

const RECORD_VALUES_SCRIPT: &str = "\
unit RecordValues;
function Process(e: IInterface): Integer;
var
  m: IInterface;
begin
  Result := 0;
  if Signature(e) <> 'GMST' then Exit;
  if EditorID(e) <> 'fMaster' then Exit;
  m := MasterOrSelf(e);
  AddMessage('override0=' + Name(OverrideByIndex(m, 0)));
  AddMessage('winning=' + Name(WinningOverride(m)));
  AddMessage('highest=' + Name(HighestOverrideOrSelf(m, 1)));
  AddMessage('iswinning=' + VarToStr(IsWinningOverride(m)));
  AddMessage('childgroup=' + Name(ChildGroup(m)));
  AddMessage('deleted=' + VarToStr(GetIsDeleted(m)));
  AddMessage('persistent=' + VarToStr(GetIsPersistent(m)));
  m := OverrideByIndex(m, 0);
  AddMessage('override master=[' + Name(Master(m)) + ']');
  AddMessage('override masterorself=[' + Name(MasterOrSelf(m)) + ']');
end;
end.
";

#[test]
fn the_record_functions_follow_master_and_override() {
    let _guard = test_lock();
    let folder = script_folder("RecordValues", RECORD_VALUES_SCRIPT);
    let mut session = open_session();
    let folder_b = script_folder("RecordValuesB", RECORD_VALUES_SCRIPT);
    let result = run(&mut session, &folder_b, "RecordValuesB", json!({}));
    let lines = lines(&result);
    assert_eq!(lines[1], "override0=fOverride [GMST:00000800]");
    assert_eq!(lines[2], "winning=fOverride [GMST:00000800]");
    assert_eq!(lines[3], "highest=fOverride [GMST:00000800]");
    assert_eq!(lines[4], "iswinning=False");
    assert!(lines[5].starts_with("childgroup="), "{lines:?}");
    assert_eq!(lines[6], "deleted=False");
    assert_eq!(lines[7], "persistent=False");
    assert_eq!(lines[8], "override master=[fMaster [GMST:00000800]]");
    assert_eq!(lines[9], "override masterorself=[fMaster [GMST:00000800]]");
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_dir_all(&folder_b);
}

//=== IwbFile ================================================================

const FILE_SCRIPT: &str = "\
unit Files;
function Process(e: IInterface): Integer;
var
  f, r: IInterface;
  sl: TStringList;
begin
  Result := 0;
  if Signature(e) <> 'GMST' then Exit;
  if EditorID(e) <> 'fPlugin' then Exit;
  AddMessage('files=' + IntToStr(FileCount));
  f := FileByIndex(1);
  AddMessage('byname=' + Name(FileByName('[01] Plugin.esp')));
  AddMessage('name=' + GetFileName(f));
  AddMessage('masters=' + IntToStr(MasterCount(f)));
  AddMessage('master0=' + Name(MasterByIndex(f, 0)));
  AddMessage('records=' + IntToStr(RecordCount(f)));
  AddMessage('byindex=' + Name(RecordByIndex(f, 0)));
  r := RecordByFormID(f, $01000802, True);
  AddMessage('byformid=' + Name(r));
  r := RecordByEditorID(f, 'fPlugin');
  AddMessage('byeditorid=' + Name(r));
  AddMessage('group=' + DisplayName(GroupBySignature(f, 'GMST')));
  AddMessage('hasgroup=' + VarToStr(HasGroup(f, 'GMST')));
  AddMessage('hasmaster=' + VarToStr(HasMaster(f, 'Master.esm')));
  AddMessage('loadorder=' + IntToStr(GetLoadOrder(f)));
  AddMessage('esm=' + VarToStr(GetIsESM(f)));
  AddMessage('canbelight=' + VarToStr(CanBeLight(f)));
  sl := TStringList.Create;
  GetMasters(f, sl);
  AddMessage('slcount=' + IntToStr(sl.Count) + ' sl0=' + sl.Strings[0]);
end;
end.
";

const FILE_SCRIPT_NO_SYSUTILS: &str = "\
unit FilesB;
function Process(e: IInterface): Integer;
var
  f, r: IInterface;
  sl: TStringList;
begin
  Result := 0;
  if Signature(e) <> 'GMST' then Exit;
  if EditorID(e) <> 'fPlugin' then Exit;
  f := FileByIndex(1);
  AddMessage('byname=' + Name(FileByName('[01] Plugin.esp')));
  AddMessage('name=' + GetFileName(f));
  AddMessage('master0=' + Name(MasterByIndex(f, 0)));
  AddMessage('byindex=' + Name(RecordByIndex(f, 0)));
  r := RecordByFormID(f, $01000802, True);
  AddMessage('byformid=' + Name(r));
  r := RecordByEditorID(f, 'fPlugin');
  AddMessage('byeditorid=' + Name(r));
  AddMessage('group=' + DisplayName(GroupBySignature(f, 'GMST')));
  AddMessage('hasgroup=' + VarToStr(HasGroup(f, 'GMST')));
  AddMessage('hasmaster=' + VarToStr(HasMaster(f, 'Master.esm')));
  AddMessage('loadorder=' + IntToStr(GetLoadOrder(f)));
  AddMessage('esm=' + VarToStr(GetIsESM(f)));
  AddMessage('canbelight=' + VarToStr(CanBeLight(f)));
  AddMessage('loadorderid=' + GetLoadOrderFileID(f));
  sl := TStringList.Create;
  GetMasters(f, sl);
  AddMessage('slcount=' + IntToStr(sl.Count));
  // The indexed property (`Strings`, `JvInterpreter_Classes.pas:1268-1271`):
  // both the named and the default form, read and written.
  AddMessage('sl0=' + sl.Strings[0]);
  AddMessage('slid0=' + sl[0]);
  sl[0] := 'changed';
  AddMessage('sl0b=' + sl[0]);
  sl.Free;
end;
end.
";

#[test]
fn the_file_functions_read_the_load_order() {
    let _guard = test_lock();
    let folder = script_folder("FilesB", FILE_SCRIPT_NO_SYSUTILS);
    let mut session = open_session();
    let result = run(&mut session, &folder, "FilesB", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert_eq!(
        lines(&result),
        vec![
            "Start: Applying script \"FilesB\"",
            "byname=[01] Plugin.esp",
            "name=Plugin.esp",
            "master0=[00] Master.esm",
            "byindex=fOverride [GMST:00000800]",
            "byformid=fPlugin [GMST:01000802]",
            "byeditorid=fPlugin [GMST:01000802]",
            "group=GRUP Top \"GMST\"",
            "hasgroup=True",
            "hasmaster=True",
            "loadorder=1",
            "esm=False",
            "canbelight=False",
            "loadorderid=01",
            "slcount=1",
            "sl0=Master.esm",
            "slid0=Master.esm",
            "sl0b=changed",
            "Done: Applying script \"FilesB\", Processed Records: 7, Elapsed Time: 00:00",
            "You can close this application now.",
        ]
    );
    let _ = std::fs::remove_dir_all(&folder);
    let _ = &FILE_SCRIPT;
}

//=== xejviScriptAdapterResource =============================================

const RESOURCE_SCRIPT: &str = "\
unit Resources;
function Process(e: IInterface): Integer;
var
  cell: TwbGridCell;
  v: TwbVector;
  sl: TStringList;
begin
  Result := 0;
  if Signature(e) <> 'GMST' then Exit;
  if EditorID(e) <> 'fPlugin' then Exit;
  AddMessage('crc=' + IntToHex(bscrc32('meshes\\test.nif'), 8));
  cell.x := 5;
  cell.y := -3;
  AddMessage('label=' + IntToHex(wbGridCellToGroupLabel(cell), 8));
  AddMessage('assettype=' + IntToStr(AssetTypeByExtension('meshes\\test.nif')));
  AddMessage('assetfolder=' + IntToStr(AssetTypeByFolder('textures\\a.dds')));
  sl := TStringList.Create;
  sl.Add('meshes');
  sl.Add('textures');
  AddMessage('instring=' + IntToStr(wbStringListInString(sl, 'x\\meshes\\y.dds')));
end;
end.
";

const RESOURCE_SCRIPT_NO_SYSUTILS: &str = "\
unit ResourcesB;
function Process(e: IInterface): Integer;
var
  cell: TwbGridCell;
  v: TwbVector;
  sl: TStringList;
begin
  Result := 0;
  if Signature(e) <> 'GMST' then Exit;
  if EditorID(e) <> 'fPlugin' then Exit;
  cell.x := 5;
  cell.y := -3;
  AddMessage('label=' + IntToStr(wbGridCellToGroupLabel(cell)));
  AddMessage('assetext=' + IntToStr(AssetTypeByExtension('meshes\\test.nif')));
  AddMessage('assetfolder=' + IntToStr(AssetTypeByFolder('textures\\a.dds')));
  v.x := 100.0;
  v.y := -100.0;
  v.z := 0.0;
  cell.x := wbPositionToGridCell(v).x;
  cell.y := wbPositionToGridCell(v).y;
  AddMessage('cell=' + IntToStr(cell.x) + ',' + IntToStr(cell.y));
  sl := TStringList.Create;
  sl.Add('meshes');
  sl.Add('textures');
  AddMessage('instring=' + IntToStr(wbStringListInString(sl, 'x\\meshes\\y.dds')));
  AddMessage('notfound=' + IntToStr(wbStringListInString(sl, 'x\\nope\\y.dds')));
  AddMessage('exists=' + VarToStr(ResourceExists('meshes\\nope.nif')));
  sl.Free;
end;
end.
";

#[test]
fn the_resource_functions_hash_and_map() {
    let _guard = test_lock();
    let folder = script_folder("ResourcesB", RESOURCE_SCRIPT_NO_SYSUTILS);
    let mut session = open_session();
    let result = run(&mut session, &folder, "ResourcesB", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    let lines = lines(&result);
    // `wbGridCellToGroupLabel(5,-3)`: the label the port's `grid_cell_to_group_label`
    // gives (the oracle comparison is phase 6 step 10's).
    assert_eq!(lines[1], "label=393213");
    assert_eq!(lines[2], "assetext=1"); // atMesh
    assert_eq!(lines[3], "assetfolder=2");
    assert_eq!(lines[4], "cell=0,-1");
    assert_eq!(lines[5], "instring=0");
    assert_eq!(lines[6], "notfound=-1");
    assert_eq!(lines[7], "exists=False");
    let _ = std::fs::remove_dir_all(&folder);
    let _ = RESOURCE_SCRIPT;
}

//=== the corpus scripts these units need ====================================

#[test]
fn the_corpus_scripts_that_need_these_units_run() {
    let _guard = test_lock();
    let Some(folder) = corpus_folder() else {
        return;
    };
    // A plugin with a record whose FormID index is zero (`List null
    // FormID.pas`) and a game setting of its own.
    let mut session = {
        xedit_core::implementation::clear_files_map();
        xedit_core::implementation::reset_load_order_slots();
        xedit_core::interface::clear_files();
        xedit_session::dump::setup_game("sse").unwrap();
        xedit_session::save::apply_edit_settings(GameMode::gmSSE);
        let mut master = header(1, 2, 0x802, &[]);
        master.extend(group(
            group_label(b"GMST"),
            0,
            &[gmst(0x800, b"fMaster\0", 1.5), gmst(0x801, b"fOther\0", 2.5)].concat(),
        ));
        let mut plugin = header(0, 1, 0x802, &["Master.esm"]);
        plugin.extend(group(group_label(b"GMST"), 0, &gmst(0x0100_0000, b"fNullId\0", 3.5)));
        let master = open("Master.esm", master);
        let plugin = open("Plugin.esp", plugin);
        let mut session = Session::with_files(GameMode::gmSSE, vec![master, plugin]);
        session.allow_edit(true);
        session
    };

    // `List null FormID.pas`: the walk, `Signature`, `FormID` and `FullPath`.
    let result = run(&mut session, &folder, "List null FormID", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert_eq!(
        lines(&result),
        vec![
            "Start: Applying script \"List null FormID\"",
            // The step 5b `FullPath` fixup added the upstream ` \ [id] `
            // prefixes (oracle-verified shape; see the `full=` expectations of
            // the synthetic tests above).
            " \\ [01] Plugin.esp \\ [1] GRUP Top \"GMST\" \\ [0] fNullId [GMST:00000000]",
            "Done: Applying script \"List null FormID\", Processed Records: 5, Elapsed Time: 00:00",
            "You can close this application now.",
        ]
    );

    // `Check for invalid alternate textures.pas`: the element walk over every
    // record with the resource functions of the corpus script it reaches.
    let result = run(&mut session, &folder, "Check for invalid alternate textures", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert_eq!(
        lines(&result),
        vec![
            "Start: Applying script \"Check for invalid alternate textures\"",
            "Done: Applying script \"Check for invalid alternate textures\", Processed Records: 5, Elapsed Time: 00:00",
            "You can close this application now.",
        ]
    );
}

#[test]
fn the_corpus_script_that_edits_a_record() {
    let _guard = test_lock();
    let Some(folder) = corpus_folder() else {
        return;
    };
    // A master light, a plugin cell and a been placed reference of the light:
    // `Skyrim - Add shadow bias to lights.pas` adds the `XLIG` subrecord to
    // the reference and writes 23.76 into its shadow depth bias.
    let dir = std::env::temp_dir().join(format!("xedit-adapter-light-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut session = {
        xedit_core::implementation::clear_files_map();
        xedit_core::implementation::reset_load_order_slots();
        xedit_core::interface::clear_files();
        xedit_session::dump::setup_game("sse").unwrap();
        xedit_session::save::apply_edit_settings(GameMode::gmSSE);
        let light = {
            let mut data = sub_record(b"EDID", b"TestLight\0");
            data.extend(sub_record(b"DATA", &[0u8; 16]));
            main_record(b"LIGH", 0, 0x800, &data)
        };
        let mut master = header(1, 2, 0x802, &[]);
        master.extend(group(group_label(b"LIGH"), 0, &light));
        let cell = {
            let data = sub_record(b"EDID", b"PluginCell\0");
            main_record(b"CELL", 0, 0x0100_0800, &data)
        };
        let reference = {
            let mut data = sub_record(b"NAME", &0x0000_0800u32.to_le_bytes());
            let mut position = Vec::new();
            for value in [0.0f32, 0.0, 0.0, 0.0, 0.0, 0.0] {
                position.extend_from_slice(&value.to_le_bytes());
            }
            data.extend(sub_record(b"DATA", &position));
            main_record(b"REFR", 0, 0x0100_0801, &data)
        };
        // The `REFR` is a temporary child of the cell.
        let children = group(0x0100_0800, 9, &reference);
        let mut plugin = header(0, 2, 0x802, &["Master.esm"]);
        plugin.extend(group(group_label(b"CELL"), 0, &[cell, children].concat()));
        let master = open("Master.esm", master);
        let plugin = open("Plugin.esp", plugin);
        let mut session = Session::with_files(GameMode::gmSSE, vec![master, plugin]);
        session.allow_edit(true);
        session
    };
    let result = run(&mut session, &folder, "Skyrim - Add shadow bias to lights", json!({}));
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    // The script prints nothing; the change is the `XLIG` subrecord of the
    // saved plugin, its 20 bytes with 23.76 as the fourth float (the shadow
    // depth bias of the structure).
    let plugin = plugin_file(&session);
    let saved = plugin
        .write_to_bytes(xedit_core::implementation::ResetModified::rmSetInternal)
        .unwrap();
    let xlig = sub_record_at(&saved, b"XLIG").expect("the script added XLIG");
    assert_eq!(&saved[xlig..xlig + 4], b"XLIG");
    assert_eq!(u16::from_le_bytes([saved[xlig + 4], saved[xlig + 5]]), 20);
    let bias = 23.76f32.to_le_bytes();
    assert!(
        saved[xlig..xlig + 28].windows(4).any(|window| window == bias),
        "the shadow depth bias 23.76 is in the record: {:02X?}",
        &saved[xlig..xlig + 28]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The native value of a single-precision game setting (the defect-4
/// finding): the port reports the double of `TwbFloatDef.ToValue` with its
/// six `RoundToEx` digits through the single-precision `10^digit` factor
/// (the ported upstream quirk, `wbInterface.pas:16789-16796` for the digit
/// substitution and `:5499-5505` for `RoundToEx`). The oracle's script
/// output matches: a `400.0` game setting prints `399.999999990097`
/// (probe of 4.1.5q on `Skyrim.esm`'s `fSunXExtreme`, 2026-10-10), so no
/// single-rounding is applied at the native value.
#[test]
fn the_native_value_of_a_single_float_matches_the_oracle() {
    let _guard = test_lock();
    let mut session = open_session();
    let registry = Registry::standard();
    let element = registry
        .call(
            &mut session,
            "elements.get",
            json!({ "form_id": "00000802", "path": "DATA\\Float" }),
        )
        .unwrap();
    assert_eq!(
        element["native"].as_f64().unwrap(),
        3.4999999911633495,
        "native={:?}",
        element["native"]
    );
    assert_eq!(element["value"], "3.500000");
}
