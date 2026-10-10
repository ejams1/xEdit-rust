// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Phase 6 step 9: the VCL form shim and the ten VCL-backed JVCL units plus
//! `xejviScriptAdapterMiscVcl.pas`.
//!
//! The two check scripts below are the ones run side by side on the 4.1.5q
//! oracle (through the hidden GUI, `cargo xtask parity script --phase parse`)
//! and in the port on 2026-10-10: each check prints `OK <name>` or
//! `FAIL <name>`, and the port's log and the oracle's `SSEScript_log` hold
//! the same lines in the same order (50 `OK` lines for the helpers, 24 for
//! the files) -- the expected lists below are the oracle's. A check failing
//! on either side prints `FAIL`, and this test fails with the whole log.
//!
//! The corpus tests at the bottom run two scripts of the oracle's
//! `Edit Scripts` folder -- one step 10's corpus classes `form`, one it
//! classes `interactive` -- and assert that the shim's refusal names the
//! surface and the script, the abort step 10's parity run can grade.

use std::path::{Path, PathBuf};

use serde_json::json;
use xedit_core::interface::globals::GameMode;
use xedit_core::interface::globals::test_lock;
use xedit_session::{Registry, Session};

/// A session with the SSE mode and no files: the helpers under test do not
/// read the tree.
fn open_session() -> Session {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    Session::with_files(GameMode::gmSSE, Vec::new())
}

/// A scripts folder of its own for a test, holding the script.
fn script_folder(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xedit-vcl-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.pas")), text).unwrap();
    dir
}

fn run(session: &mut Session, folder: &Path, name: &str) -> serde_json::Value {
    session.allow_edit(true);
    Registry::standard()
        .call(
            session,
            "script.run",
            json!({
                "file": format!("{name}.pas"),
                "scripts": folder.display().to_string(),
                "save": false,
            }),
        )
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

/// The `OK <name>` lines of a check script's log, in order; a `FAIL` panics
/// with the whole log.
fn ok_lines(result: &serde_json::Value) -> Vec<String> {
    let lines = lines(result);
    let mut ok = Vec::new();
    for line in &lines {
        if let Some(name) = line.strip_prefix("OK ") {
            ok.push(name.to_owned());
        } else if let Some(name) = line.strip_prefix("FAIL ") {
            panic!("check {name} failed: {lines:?}");
        }
    }
    ok
}

//=== The headless helpers ===================================================

/// 50 checks of the pure helpers: the `Menus` shortcut codec, the `Controls`
/// hint and cursor helpers, the `Forms` shift-state helpers and the `Windows`
/// records -- the same script the oracle ran, with the same `OK` lines.
const HELPERS_SCRIPT: &str = r#"{
  Headless VCL helper checks for the xEdit-rust port (phase 6 step 9).
  Every check prints one OK or FAIL line, so the whole log can be compared
  between the 4.1.5q oracle and the port.
}
unit VclHelpers;
function Initialize: integer;
var
  r: TRect;
  p: TPoint;
  key: Word;
  shift: integer;
begin
  if ShortCut(65, 4) = 16449 then AddMessage('OK ShortCut.Ctrl') else AddMessage('FAIL ShortCut.Ctrl');
  if ShortCut(116, 1) = 8308 then AddMessage('OK ShortCut.ShiftF5') else AddMessage('FAIL ShortCut.ShiftF5');
  if ShortCut(65, 7) = 57409 then AddMessage('OK ShortCut.All') else AddMessage('FAIL ShortCut.All');
  if ShortCut(300, 0) = 0 then AddMessage('OK ShortCut.Word') else AddMessage('FAIL ShortCut.Word');
  if ShortCutToText(24641) = 'Ctrl+Shift+A' then AddMessage('OK ShortCutToText.CtrlShiftA') else AddMessage('FAIL ShortCutToText.CtrlShiftA');
  if ShortCutToText(8308) = 'Shift+F5' then AddMessage('OK ShortCutToText.ShiftF5') else AddMessage('FAIL ShortCutToText.ShiftF5');
  if ShortCutToText(0) = '' then AddMessage('OK ShortCutToText.Zero') else AddMessage('FAIL ShortCutToText.Zero');
  if ShortCutToText(49152) = '' then AddMessage('OK ShortCutToText.Modifiers') else AddMessage('FAIL ShortCutToText.Modifiers');
  if ShortCutToText(57409) = 'Ctrl+Alt+Shift+A' then AddMessage('OK ShortCutToText.All') else AddMessage('FAIL ShortCutToText.All');
  if ShortCutToText(5) = '' then AddMessage('OK ShortCutToText.Bare') else AddMessage('FAIL ShortCutToText.Bare');
  if ShortCutToText(TextToShortCut('Ctrl+Shift+A')) = 'Ctrl+Shift+A' then AddMessage('OK TextToShortCut.RoundTrip') else AddMessage('FAIL TextToShortCut.RoundTrip');
  if TextToShortCut('F5') = 116 then AddMessage('OK TextToShortCut.F5') else AddMessage('FAIL TextToShortCut.F5');
  if TextToShortCut('Alt+R') = 32850 then AddMessage('OK TextToShortCut.AltR') else AddMessage('FAIL TextToShortCut.AltR');
  if TextToShortCut('Alt+Del') = 32814 then AddMessage('OK TextToShortCut.AltDel') else AddMessage('FAIL TextToShortCut.AltDel');
  if TextToShortCut('') = 0 then AddMessage('OK TextToShortCut.Empty') else AddMessage('FAIL TextToShortCut.Empty');
  if TextToShortCut('NotAShortcut') = 0 then AddMessage('OK TextToShortCut.Junk') else AddMessage('FAIL TextToShortCut.Junk');
  if TextToShortCut('f5') = 116 then AddMessage('OK TextToShortCut.Lower') else AddMessage('FAIL TextToShortCut.Lower');
  if TextToShortCut('CTRL+F5') = 16500 then AddMessage('OK TextToShortCut.Upper') else AddMessage('FAIL TextToShortCut.Upper');
  if TextToShortCut('Num 5') = 12 then AddMessage('OK TextToShortCut.Num5') else AddMessage('FAIL TextToShortCut.Num5');
  if GetShortHint('Long hint|Short hint') = 'Long hint' then AddMessage('OK GetShortHint') else AddMessage('FAIL GetShortHint');
  if GetShortHint('Just a hint') = 'Just a hint' then AddMessage('OK GetShortHint.None') else AddMessage('FAIL GetShortHint.None');
  if GetShortHint('') = '' then AddMessage('OK GetShortHint.Empty') else AddMessage('FAIL GetShortHint.Empty');
  if GetLongHint('Long hint|Short hint') = 'Short hint' then AddMessage('OK GetLongHint') else AddMessage('FAIL GetLongHint');
  if GetLongHint('Just a hint') = 'Just a hint' then AddMessage('OK GetLongHint.None') else AddMessage('FAIL GetLongHint.None');
  if IsAccel(70, '&File') then AddMessage('OK IsAccel.Exact') else AddMessage('FAIL IsAccel.Exact');
  if IsAccel(102, '&File') then AddMessage('OK IsAccel.Lower') else AddMessage('FAIL IsAccel.Lower');
  if IsAccel(65, 'Save &as') then AddMessage('OK IsAccel.Other') else AddMessage('FAIL IsAccel.Other');
  if not IsAccel(65, '&File') then AddMessage('OK IsAccel.Wrong') else AddMessage('FAIL IsAccel.Wrong');
  if not IsAccel(65, '&&A') then AddMessage('OK IsAccel.Escaped') else AddMessage('FAIL IsAccel.Escaped');
  if not IsAccel(65, '') then AddMessage('OK IsAccel.Empty') else AddMessage('FAIL IsAccel.Empty');
  { The set comparisons go through `ShortCut`: a variant set cannot be
    compared with `=`, and `[0, 3]` is a set whose elements are the values 0
    and 3 (`SetExpression1`). }
  if ShortCut(0, KeysToShiftState(5)) = 8192 then AddMessage('OK KeysToShiftState.ShiftLeft') else AddMessage('FAIL KeysToShiftState.ShiftLeft');
  if ShortCut(0, KeysToShiftState(31)) = 24576 then AddMessage('OK KeysToShiftState.All') else AddMessage('FAIL KeysToShiftState.All');
  if ShortCut(0, KeysToShiftState(65535)) = 24576 then AddMessage('OK KeysToShiftState.Mask') else AddMessage('FAIL KeysToShiftState.Mask');
  if ShortCut(0, KeysToShiftState(0)) = 0 then AddMessage('OK KeysToShiftState.None') else AddMessage('FAIL KeysToShiftState.None');
  if ShortCut(0, KeyDataToShiftState(0)) = 0 then AddMessage('OK KeyDataToShiftState') else AddMessage('FAIL KeyDataToShiftState');
  if CursorToString(0) = 'crDefault' then AddMessage('OK CursorToString.Zero') else AddMessage('FAIL CursorToString.Zero');
  if CursorToString(-22) = 'crSizeAll' then AddMessage('OK CursorToString.SizeAll') else AddMessage('FAIL CursorToString.SizeAll');
  if CursorToString(-12) = 'crDrag' then AddMessage('OK CursorToString.Drag') else AddMessage('FAIL CursorToString.Drag');
  if CursorToString(-23) = '-23' then AddMessage('OK CursorToString.Unknown') else AddMessage('FAIL CursorToString.Unknown');
  if CursorToString(5) = '5' then AddMessage('OK CursorToString.Five') else AddMessage('FAIL CursorToString.Five');
  key := 0;
  if (IdentToCursor('crHandPoint', key)) and (key = -21) then AddMessage('OK IdentToCursor') else AddMessage('FAIL IdentToCursor');
  key := 0;
  if not IdentToCursor('crNone', key) then AddMessage('OK IdentToCursor.None') else AddMessage('FAIL IdentToCursor.None');
  if StringToCursor('crSizeAll') = -22 then AddMessage('OK StringToCursor') else AddMessage('FAIL StringToCursor');
  if StringToCursor('-7') = -7 then AddMessage('OK StringToCursor.Number') else AddMessage('FAIL StringToCursor.Number');
  r := Rect(1, 2, 3, 4);
  if (r.Left = 1) and (r.Top = 2) and (r.Right = 3) and (r.Bottom = 4) then AddMessage('OK Rect') else AddMessage('FAIL Rect');
  r.TopLeft := Point(10, 20);
  if (r.Left = 10) and (r.Top = 20) and (r.Right = 3) and (r.Bottom = 4) then AddMessage('OK Rect.TopLeft') else AddMessage('FAIL Rect.TopLeft');
  p := r.BottomRight;
  if (p.X = 3) and (p.Y = 4) then AddMessage('OK Rect.BottomRight') else AddMessage('FAIL Rect.BottomRight');
  r.BottomRight := Point(30, 40);
  if (r.Right = 30) and (r.Bottom = 40) then AddMessage('OK Rect.BottomRightWrite') else AddMessage('FAIL Rect.BottomRightWrite');
  r := Bounds(5, 6, 7, 8);
  if (r.Left = 5) and (r.Top = 6) and (r.Right = 12) and (r.Bottom = 14) then AddMessage('OK Bounds') else AddMessage('FAIL Bounds');
  shift := 0;
  key := 0;
  ShortCutToKey(TextToShortCut('Ctrl+Shift+A'), key, shift);
  if (key = 65) and (ShortCut(0, shift) = 24576) then AddMessage('OK ShortCutToKey') else AddMessage('FAIL ShortCutToKey');
  AddMessage('VclHelpers done');
  Result := 0;
end;
end.
"#;

#[test]
fn the_headless_vcl_helpers_answer_as_the_oracle() {
    let _guard = test_lock();
    let folder = script_folder("VclHelpers", HELPERS_SCRIPT);
    let mut session = open_session();
    let result = run(&mut session, &folder, "VclHelpers");
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert_eq!(
        ok_lines(&result),
        vec![
            "ShortCut.Ctrl",
            "ShortCut.ShiftF5",
            "ShortCut.All",
            "ShortCut.Word",
            "ShortCutToText.CtrlShiftA",
            "ShortCutToText.ShiftF5",
            "ShortCutToText.Zero",
            "ShortCutToText.Modifiers",
            "ShortCutToText.All",
            "ShortCutToText.Bare",
            "TextToShortCut.RoundTrip",
            "TextToShortCut.F5",
            "TextToShortCut.AltR",
            "TextToShortCut.AltDel",
            "TextToShortCut.Empty",
            "TextToShortCut.Junk",
            "TextToShortCut.Lower",
            "TextToShortCut.Upper",
            "TextToShortCut.Num5",
            "GetShortHint",
            "GetShortHint.None",
            "GetShortHint.Empty",
            "GetLongHint",
            "GetLongHint.None",
            "IsAccel.Exact",
            "IsAccel.Lower",
            "IsAccel.Other",
            "IsAccel.Wrong",
            "IsAccel.Escaped",
            "IsAccel.Empty",
            "KeysToShiftState.ShiftLeft",
            "KeysToShiftState.All",
            "KeysToShiftState.Mask",
            "KeysToShiftState.None",
            "KeyDataToShiftState",
            "CursorToString.Zero",
            "CursorToString.SizeAll",
            "CursorToString.Drag",
            "CursorToString.Unknown",
            "CursorToString.Five",
            "IdentToCursor",
            "IdentToCursor.None",
            "StringToCursor",
            "StringToCursor.Number",
            "Rect",
            "Rect.TopLeft",
            "Rect.BottomRight",
            "Rect.BottomRightWrite",
            "Bounds",
            "ShortCutToKey",
        ]
    );
    assert_eq!(
        lines(&result).last().map(String::as_str),
        Some("You can close this application now.")
    );
}

//=== The IniFiles and IOUtils members =======================================

/// 24 checks of `TCustomIniFile`/`TMemIniFile`/`TIniFile` and the `IOUtils`
/// helpers over `ScriptsPath`, including the bytes `UpdateFile` writes; the
/// same script the oracle ran, with the same `OK` lines.
const INI_SCRIPT: &str = r#"{
  Headless `IniFiles`/`IOUtils` checks for the xEdit-rust port (phase 6 step 9).
  Every check prints one OK or FAIL line, so the whole log can be compared
  between the 4.1.5q oracle and the port.
}
unit VclIni;
function Initialize: integer;
var
  ini: TMemIniFile;
  ini2: TIniFile;
  sl: TStringList;
  t: TDirectory;
  tf: TFile;
  files: TStringDynArray;
  h: string;
begin
  ini := TMemIniFile.Create(ScriptsPath + 'probe.ini');
  ini.WriteString('S1', 'A', 'one');
  ini.WriteInteger('S1', 'B', 42);
  ini.WriteBool('S1', 'C', True);
  ini.WriteBool('S1', 'D', False);
  ini.WriteFloat('S1', 'E', 1.5);
  ini.WriteString('S2', 'X', 'two');
  ini.DeleteKey('S1', 'B');
  ini.UpdateFile;
  if ini.ReadString('S1', 'A', 'def') = 'one' then AddMessage('OK Ini.ReadString') else AddMessage('FAIL Ini.ReadString');
  if ini.ReadString('S1', 'Missing', 'def') = 'def' then AddMessage('OK Ini.ReadString.Default') else AddMessage('FAIL Ini.ReadString.Default');
  if ini.ReadInteger('S1', 'B', 7) = 7 then AddMessage('OK Ini.ReadInteger.Deleted') else AddMessage('FAIL Ini.ReadInteger.Deleted');
  if ini.ReadInteger('S1', 'C', 0) = 1 then AddMessage('OK Ini.ReadInteger.Bool') else AddMessage('FAIL Ini.ReadInteger.Bool');
  if ini.ReadFloat('S1', 'E', 0) = 1.5 then AddMessage('OK Ini.ReadFloat') else AddMessage('FAIL Ini.ReadFloat');
  if ini.ReadBool('S1', 'D', True) = False then AddMessage('OK Ini.ReadBool') else AddMessage('FAIL Ini.ReadBool');
  if ini.SectionExists('S1') and ini.SectionExists('S2') then AddMessage('OK Ini.SectionExists') else AddMessage('FAIL Ini.SectionExists');
  if not ini.SectionExists('S3') then AddMessage('OK Ini.SectionExists.No') else AddMessage('FAIL Ini.SectionExists.No');
  if ini.ValueExists('S1', 'A') then AddMessage('OK Ini.ValueExists') else AddMessage('FAIL Ini.ValueExists');
  if not ini.ValueExists('S1', 'B') then AddMessage('OK Ini.ValueExists.Deleted') else AddMessage('FAIL Ini.ValueExists.Deleted');
  sl := TStringList.Create;
  ini.GetStrings(sl);
  if sl.Count = 9 then AddMessage('OK Ini.GetStrings.Count') else AddMessage('FAIL Ini.GetStrings.Count');
  if sl[0] = '[S1]' then AddMessage('OK Ini.GetStrings.0') else AddMessage('FAIL Ini.GetStrings.0');
  if sl[1] = 'A=one' then AddMessage('OK Ini.GetStrings.1') else AddMessage('FAIL Ini.GetStrings.1');
  if sl[2] = 'C=1' then AddMessage('OK Ini.GetStrings.2') else AddMessage('FAIL Ini.GetStrings.2');
  if sl[3] = 'D=0' then AddMessage('OK Ini.GetStrings.3') else AddMessage('FAIL Ini.GetStrings.3');
  if sl[4] = 'E=1.5' then AddMessage('OK Ini.GetStrings.4') else AddMessage('FAIL Ini.GetStrings.4');
  if sl[6] = '[S2]' then AddMessage('OK Ini.GetStrings.6') else AddMessage('FAIL Ini.GetStrings.6');
  ini.ReadSections(sl);
  if (sl.Count = 2) and (sl[0] = 'S1') and (sl[1] = 'S2') then AddMessage('OK Ini.ReadSections') else AddMessage('FAIL Ini.ReadSections');
  ini.ReadSection('S1', sl);
  if (sl.Count = 4) and (sl[0] = 'A') and (sl[3] = 'E') then AddMessage('OK Ini.ReadSection') else AddMessage('FAIL Ini.ReadSection');
  ini.ReadSectionValues('S1', sl);
  if (sl.Count = 4) and (sl[0] = 'A=one') and (sl[3] = 'E=1.5') then AddMessage('OK Ini.ReadSectionValues') else AddMessage('FAIL Ini.ReadSectionValues');
  ini.Free;
  ini2 := TIniFile.Create(ScriptsPath + 'probe-t.ini');
  ini2.WriteString('T', 'K', 'v');
  if ini2.ReadString('T', 'K', '') = 'v' then AddMessage('OK IniFile.WriteThrough') else AddMessage('FAIL IniFile.WriteThrough');
  ini2.Free;
  h := tf.ReadAllText(ScriptsPath + 'probe.ini');
  if h = '[S1]' + #13#10 + 'A=one' + #13#10 + 'C=1' + #13#10 + 'D=0' + #13#10 + 'E=1.5' + #13#10 + #13#10 + '[S2]' + #13#10 + 'X=two' + #13#10 + #13#10 then AddMessage('OK Ini.UpdateFile.Text') else AddMessage('FAIL Ini.UpdateFile.Text');
  files := t.GetFiles(ScriptsPath, 'probe-t.*', soTopDirectoryOnly);
  if files[0] = ScriptsPath + 'probe-t.ini' then AddMessage('OK Directory.GetFiles') else AddMessage('FAIL Directory.GetFiles');
  tf.WriteAllText(ScriptsPath + 'probe-out.txt', 'written');
  if tf.ReadAllText(ScriptsPath + 'probe-out.txt') = 'written' then AddMessage('OK File.WriteAllText') else AddMessage('FAIL File.WriteAllText');
  AddMessage('VclIni done');
  Result := 0;
end;
end.
"#;

#[test]
fn the_ini_and_io_helpers_answer_as_the_oracle() {
    let _guard = test_lock();
    let folder = script_folder("VclIni", INI_SCRIPT);
    let mut session = open_session();
    let result = run(&mut session, &folder, "VclIni");
    assert_eq!(result["aborted"], false, "{:?}", lines(&result));
    assert_eq!(
        ok_lines(&result),
        vec![
            "Ini.ReadString",
            "Ini.ReadString.Default",
            "Ini.ReadInteger.Deleted",
            "Ini.ReadInteger.Bool",
            "Ini.ReadFloat",
            "Ini.ReadBool",
            "Ini.SectionExists",
            "Ini.SectionExists.No",
            "Ini.ValueExists",
            "Ini.ValueExists.Deleted",
            "Ini.GetStrings.Count",
            "Ini.GetStrings.0",
            "Ini.GetStrings.1",
            "Ini.GetStrings.2",
            "Ini.GetStrings.3",
            "Ini.GetStrings.4",
            "Ini.GetStrings.6",
            "Ini.ReadSections",
            "Ini.ReadSection",
            "Ini.ReadSectionValues",
            "IniFile.WriteThrough",
            "Ini.UpdateFile.Text",
            "Directory.GetFiles",
            "File.WriteAllText",
        ]
    );
    // `TMemIniFile.UpdateFile` left the file the script checked.
    let written = std::fs::read_to_string(folder.join("probe.ini")).unwrap();
    assert_eq!(
        written,
        "[S1]\r\nA=one\r\nC=1\r\nD=0\r\nE=1.5\r\n\r\n[S2]\r\nX=two\r\n\r\n"
    );
}

//=== The shim's refusals ====================================================

/// `StringToCursor` of a text that is neither an ident nor a number raises
/// the `EConvertError` the oracle raises (`'crNone' is not a valid integer
/// value`).
#[test]
fn string_to_cursor_of_an_unknown_ident_raises_as_the_oracle() {
    let _guard = test_lock();
    let folder = script_folder(
        "BadCursor",
        "unit BadCursor;\n\
         function Initialize: integer;\n\
         begin\n\
           AddMessage('value=' + StringToCursor('crNone'));\n\
           Result := 0;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "BadCursor");
    assert_eq!(result["aborted"], true);
    let lines = lines(&result);
    let exception = lines
        .iter()
        .find(|line| line.starts_with("Exception in unit BadCursor line "))
        .unwrap_or_else(|| panic!("no exception line: {lines:?}"));
    assert!(
        exception.contains("[EConvertError]") && exception.contains("'crNone' is not a valid integer value"),
        "{exception}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "Aborted: Applying script \"BadCursor\"")
    );
}

/// A script that constructs a registered VCL class whose object the port
/// cannot build refuses through the shim (not as an undeclared identifier):
/// the class and its members are registered, so a script compiles and runs to
/// the point of the call.
#[test]
fn a_control_member_aborts_with_the_shim_error() {
    let _guard = test_lock();
    let folder = script_folder(
        "Control",
        "unit Control;\n\
         function Initialize: integer;\n\
         var\n\
           cb: TComboBox;\n\
         begin\n\
           cb := TComboBox.Create(nil);\n\
           Result := 0;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Control");
    assert_eq!(result["aborted"], true);
    let lines = lines(&result);
    let exception = lines
        .iter()
        .find(|line| line.starts_with("Exception in unit Control line "))
        .unwrap_or_else(|| panic!("no exception line: {lines:?}"));
    assert!(
        exception.contains("[EVclUnavailable]") && exception.contains("TComboBox.Create is not available"),
        "{exception}"
    );
}

//=== The corpus scripts of the two classes ==================================

/// The folder of the oracle's `Edit Scripts`; `None` without
/// `XEDIT_ORACLE_DIR`.
fn corpus_folder() -> Option<PathBuf> {
    let oracle = std::env::var_os("XEDIT_ORACLE_DIR")?;
    Some(PathBuf::from(oracle).join("Edit Scripts"))
}

/// The class step 10's corpus gives a script (`corpus.json`).
fn corpus_class(name: &str) -> Option<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join("crates/xtask/oracle/scripts/corpus.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let scripts = value["scripts"].as_array()?;
    for script in scripts {
        if script["name"].as_str() == Some(name) {
            if script["api"].as_bool().unwrap_or(false) {
                return Some("api".to_owned());
            }
            if script["form"].as_bool().unwrap_or(false) {
                return Some("form".to_owned());
            }
            if script["dialogs"].as_bool().unwrap_or(false) {
                return Some("interactive".to_owned());
            }
            return Some("headless".to_owned());
        }
    }
    None
}

/// A form script of the corpus (`Find records.pas` builds a `TForm` in its
/// `Initialize`) aborts with the shim's refusal, which names the class and
/// the script -- and the class is the one step 10's parity gives the case
/// (`form`: the corpus greps `TForm`/`ShowModal`), so the parity run can
/// grade the abort against the oracle's form classification.
#[test]
fn a_form_script_aborts_with_the_shim_error() {
    let Some(folder) = corpus_folder() else {
        eprintln!("skipped: XEDIT_ORACLE_DIR is not set");
        return;
    };
    let _guard = test_lock();
    let mut session = open_session();
    let result = run(&mut session, &folder, "Find records");
    assert_eq!(result["aborted"], true);
    let lines = lines(&result);
    let exception = lines
        .iter()
        .find(|line| line.starts_with("Exception in unit userscript line "))
        .unwrap_or_else(|| panic!("no exception line: {lines:?}"));
    assert!(exception.contains("[EVclUnavailable]"), "{exception}");
    assert!(
        exception.contains("TForm.Create is not available without a GUI (script \"Find records\")"),
        "{exception}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "Aborted: Applying script \"Find records\"")
    );
    assert_eq!(corpus_class("Find records.pas").as_deref(), Some("form"));
}

/// A dialog-only script of the corpus (`Add prefix or suffix to Editor ID.pas`
/// asks with `MessageDlg`/`InputQuery`) aborts with the shim's refusal naming
/// the dialog -- step 10's `interactive` classification of the case, which
/// the parity run defers to a dialog of the oracle's own.
#[test]
fn a_dialog_script_aborts_with_the_shim_error() {
    let Some(folder) = corpus_folder() else {
        eprintln!("skipped: XEDIT_ORACLE_DIR is not set");
        return;
    };
    let _guard = test_lock();
    let mut session = open_session();
    let result = run(&mut session, &folder, "Add prefix or suffix to Editor ID");
    assert_eq!(result["aborted"], true);
    let lines = lines(&result);
    let exception = lines
        .iter()
        .find(|line| line.starts_with("Exception in unit UserScript line "))
        .unwrap_or_else(|| panic!("no exception line: {lines:?}"));
    assert!(exception.contains("[EVclUnavailable]"), "{exception}");
    assert!(
        exception.contains("MessageDlg is not available without a GUI (script \"Add prefix or suffix to Editor ID\")"),
        "{exception}"
    );
    assert_eq!(
        corpus_class("Add prefix or suffix to Editor ID.pas").as_deref(),
        Some("interactive")
    );
}
