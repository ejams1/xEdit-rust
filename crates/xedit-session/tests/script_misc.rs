// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The phase-6 step-7 units through `script.run`: `xejviScriptAdapterMisc`
//! (the string list surface, the string/file/math helpers), the
//! `xejviScriptAdapter.pas` unit's own functions and records,
//! `TPerlRegEx` and the `JsonDataObjects` surface, with the corpus scripts
//! that need nothing but them. The corpus tests skip without
//! `XEDIT_SCRIPTS`/`XEDIT_ORACLE_DIR`.

mod script_fixture;

use serde_json::json;

use std::path::PathBuf;

use script_fixture::{lines, open_session, run, script_folder, session_lock};

/// The `TStringList`/`TStrings` surface of `xejviScriptAdapterMisc.pas`.
///
/// `ValueFromIndex[2] := '3'` goes through the `AddISet` writer with the
/// index arguments: the indexed assignment's probe of the target uses a
/// fresh args object with no index (`JvInterpreter.pas:6985`), so the
/// `AddIGet`-only reader never answers it (`:7005`).
#[test]
fn the_string_list_surface_works() {
    let _guard = session_lock();
    let folder = script_folder(
        "strings",
        "Strings.pas",
        "unit Strings;\n\
         function Initialize: Integer;\n\
         var\n\
           sl, other: TStringList;\n\
         begin\n\
           sl := TStringList.Create;\n\
           sl.Add('alpha');\n\
           sl.Add('beta=2');\n\
           sl.Add('gamma');\n\
           AddMessage('count ' + IntToHex64(sl.Count, 0));\n\
           AddMessage('value ' + sl.ValueFromIndex[1]);\n\
           sl.ValueFromIndex[2] := '3';\n\
           AddMessage('gamma ' + sl[2]);\n\
           AddMessage('text'#13#10 + sl.Text);\n\
           AddMessage('contains ' + IfThen(sl.Contains('alpha'), 'yes', 'no'));\n\
           AddMessage('containsname ' + IfThen(sl.ContainsName('beta'), 'yes', 'no'));\n\
           AddMessage('containsname2 ' + IfThen(sl.ContainsName('nope'), 'yes', 'no'));\n\
           sl.Delimiter := ';';\n\
           AddMessage('delimited ' + sl.DelimitedText);\n\
           other := TStringList.Create;\n\
           other.Delimiter := ';';\n\
           other.DelimitedText := 'alpha;delta';\n\
           sl.Difference(other);\n\
           AddMessage('difference'#13#10 + sl.Text);\n\
           sl.Add('alpha');\n\
           sl.Add('delta');\n\
           sl.Intersection(other);\n\
           AddMessage('intersection'#13#10 + sl.Text);\n\
           sl.Union(other);\n\
           AddMessage('union'#13#10 + sl.Text);\n\
           sl.Free;\n\
           other.Free;\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Strings.pas", json!({}));
    assert_eq!(result["aborted"], false, "{}", script_fixture::abort_text(&result));
    assert_eq!(
        lines(&result),
        [
            "Start: Applying script \"Strings\"",
            "count 3",
            "value 2",
            "gamma gamma=3",
            "text",
            "alpha",
            "beta=2",
            "gamma=3",
            "",
            "contains yes",
            "containsname yes",
            "containsname2 no",
            "delimited alpha;beta=2;gamma=3",
            "difference",
            "beta=2",
            "gamma=3",
            "",
            "intersection",
            "alpha",
            "delta",
            "",
            "union",
            "alpha",
            "delta",
            "",
            "Script terminated itself, Result=1",
            "Done: Applying script \"Strings\", Processed Records: 0, Elapsed Time: 00:00",
            "You can close this application now.",
        ],
        "{}",
        script_fixture::abort_text(&result)
    );
    let _ = std::fs::remove_dir_all(&folder);
}

/// `xejviScriptAdapter.pas`'s own functions and records.
#[test]
fn the_adapter_helpers_and_records_work() {
    let _guard = session_lock();
    let folder = script_folder(
        "adapter",
        "Adapter.pas",
        "unit Adapter;\n\
         function Initialize: Integer;\n\
         var\n\
           sl, other: TStringList;\n\
           v: TwbVector;\n\
           c: TwbGridCell;\n\
           x: IInterface;\n\
           filter: string;\n\
         begin\n\
           AddMessage('version ' + IntToHex64(wbVersionNumber, 0));\n\
           AddMessage('path ' + FullPathToFilename('C:\\Games\\Foo Bar\\x.nif'));\n\
           sl := TStringList.Create;\n\
           AddMessage('assigned-list ' + IfThen(Assigned(sl), 'yes', 'no'));\n\
           AddMessage('assigned-int ' + IfThen(Assigned(1), 'yes', 'no'));\n\
           sl.Add('alpha.dds');\n\
           sl.Add('beta.nif');\n\
           sl.Add('gamma_dds');\n\
           sl.Add('alpha.dds');\n\
           wbRemoveDuplicateStrings(sl);\n\
           AddMessage('dedupe ' + IntToHex64(sl.Count, 0));\n\
           filter := 'DDS';\n\
           sl.Sorted := True;\n\
           other := TStringList.Create;\n\
           wbFilterStrings(sl, other, filter);\n\
           AddMessage('filtered'#13#10 + other.Text);\n\
           v := wbVector(1, 2.5, -3);\n\
           AddMessage('vector ' + v.x + ' ' + v.y + ' ' + v.z);\n\
           c := wbGridCell(5, -7);\n\
           AddMessage('cell ' + c.x + ' ' + c.y);\n\
           sl.Clear;\n\
           GetRecordDefNames(sl);\n\
           AddMessage('defs ' + IfThen(sl.Contains('GMST - Game Setting'), 'yes', 'no'));\n\
           EnableSkyrimSaveFormat;\n\
           x := ObjectToElement(sl);\n\
           AddMessage('element ' + IfThen(Assigned(x), 'yes', 'no'));\n\
           sl.Free;\n\
           other.Free;\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Adapter.pas", json!({}));
    assert_eq!(result["aborted"], false, "{}", script_fixture::abort_text(&result));
    let lines = lines(&result);
    assert_eq!(lines[1], "version 4010510", "{lines:?}");
    assert_eq!(lines[2], r"path C-\Games\Foo Bar\x.nif", "{lines:?}");
    assert_eq!(lines[3], "assigned-list yes", "{lines:?}");
    assert_eq!(lines[4], "assigned-int no", "{lines:?}");
    // `wbRemoveDuplicateStrings` walks the list from the end, `Find` runs
    // its binary search whatever `Sorted` says (an unsorted list lands on
    // whatever the search sees), and the walk deletes from the end.
    assert_eq!(lines[5], "dedupe 3", "{lines:?}");
    assert!(lines.iter().any(|line| line == "filtered"), "{lines:?}");
    assert!(lines.iter().any(|line| line == "alpha.dds"), "{lines:?}");
    assert!(lines.iter().any(|line| line == "gamma_dds"), "{lines:?}");
    assert_eq!(
        lines.iter().find(|line| line.starts_with("vector ")).unwrap(),
        "vector 1 2.5 -3"
    );
    assert_eq!(
        lines.iter().find(|line| line.starts_with("cell ")).unwrap(),
        "cell 5 -7"
    );
    assert_eq!(
        lines.iter().find(|line| line.starts_with("defs ")).unwrap(),
        "defs yes",
        "the record definitions of the loaded game"
    );
    assert_eq!(
        lines.iter().find(|line| line.starts_with("element ")).unwrap(),
        "element no"
    );
    // `EnableSkyrimSaveFormat` set the save settings of the session.
    assert!(xedit_core::interface::globals::test_write());
    assert!(xedit_core::interface::globals::force_new_header());
    xedit_core::interface::globals::set_test_write(false);
    xedit_core::interface::globals::set_force_new_header(false);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The `StrUtils`, `SysUtils`, `Math` and `Variants` registrations.
#[test]
fn the_string_and_math_helpers_work() {
    let _guard = session_lock();
    let folder = script_folder(
        "helpers",
        "Helpers.pas",
        "unit Helpers;\n\
         function Initialize: Integer;\n\
         var\n\
           values: TStringList;\n\
           parts: array of string;\n\
         begin\n\
           AddMessage('contains ' + IfThen(ContainsStr('Hello', 'ell'), 'yes', 'no'));\n\
           AddMessage('contains-case ' + IfThen(ContainsStr('Hello', 'ELL'), 'yes', 'no'));\n\
           AddMessage('containstext ' + IfThen(ContainsText('Hello', 'ELL'), 'yes', 'no'));\n\
           AddMessage('dupe ' + DupeString('ab', 3));\n\
           AddMessage('starts ' + IfThen(StartsStr('ab', 'abc'), 'yes', 'no'));\n\
           AddMessage('ends ' + IfThen(EndsText('TXT', 'abc.txt'), 'yes', 'no'));\n\
           AddMessage('left ' + LeftStr('abcdef', 3));\n\
           AddMessage('right ' + RightStr('abcdef', 2));\n\
           AddMessage('mid ' + MidStr('abcdef', 2, 3));\n\
           AddMessage('reverse ' + ReverseString('abc'));\n\
           AddMessage('stuff ' + StuffString('abcdef', 3, 2, 'XY'));\n\
           values := TStringList.Create;\n\
           values.Add('one');\n\
           values.Add('two');\n\
           AddMessage('indexstr ' + IndexStr('two', values));\n\
           AddMessage('indextext ' + IndexText('TWO', values));\n\
           AddMessage('matchstr ' + IfThen(MatchStr('two', values), 'yes', 'no'));\n\
           AddMessage('ifthen ' + IfThen(1 > 2, 'a', 'b'));\n\
           parts := SplitString('a;b;c', ';');\n\
           AddMessage('split ' + parts[0] + parts[1] + parts[2]);\n\
           values.Free;\n\
           AddMessage('same ' + IfThen(SameText('ABC', 'abc'), 'yes', 'no'));\n\
           AddMessage('replaceall ' + StringReplace('a-b-c', '-', '+', 1));\n\
           AddMessage('replace ' + StringReplace('a-b-c', '-', '+', 2));\n\
           AddMessage('hex ' + IntToHex64(-1, 2));\n\
           AddMessage('hex2 ' + IntToHex64(255, 4));\n\
           AddMessage('int64 ' + IntToHex64(StrToInt64('$10'), 0));\n\
           AddMessage('int64def ' + IntToHex64(StrToInt64Def('zz', 7), 0));\n\
           AddMessage('vd ' + IfThen(DirectoryExists('Z:\\nope'), 'yes', 'no'));\n\
           AddMessage('backslash ' + IncludeTrailingBackslash('C:\\x'));\n\
           AddMessage('nobackslash ' + ExcludeTrailingBackslash('C:\\x\\'));\n\
           AddMessage('stringofchar ' + StringOfChar('-', 3));\n\
           AddMessage('vartype ' + IntToHex64(VarType('x'), 0));\n\
           AddMessage('vartype2 ' + IntToHex64(VarType(12), 0));\n\
           AddMessage('lerp ' + Lerp(0, 10, 0.5));\n\
           AddMessage('inverse ' + InverseLerp(0, 10, 2.5));\n\
           AddMessage('clamp ' + EnsureRange(12, 0, 10));\n\
           AddMessage('inrange ' + IfThen(InRange(5, 0, 10), 'yes', 'no'));\n\
           AddMessage('fmod ' + FMod(7, 3));\n\
           AddMessage('roundto ' + RoundTo(1.2345, -2));\n\
           AddMessage('sign ' + Sign(-4));\n\
           AddMessage('compare ' + CompareValue(1, 2, 0));\n\
           AddMessage('iszero ' + IfThen(IsZero(0.0000000001, 0), 'yes', 'no'));\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Helpers.pas", json!({}));
    assert_eq!(result["aborted"], false, "{}", script_fixture::abort_text(&result));
    let lines = lines(&result);
    for expected in [
        "contains yes",
        "contains-case no",
        "containstext yes",
        "dupe ababab",
        // `StartsStr(AStr, ASubStr)`: the *substring* is the second
        // argument -- `StartsStr('ab', 'abc')` asks whether 'abc' starts
        // with 'ab'.
        "starts yes",
        "ends yes",
        "left abc",
        "right ef",
        "mid bcd",
        "reverse cba",
        "stuff abXYef",
        "indexstr 1",
        "indextext 1",
        "matchstr yes",
        "ifthen b",
        "split abc",
        "same yes",
        "replaceall a+b+c",
        "replace a+b-c",
        // The two's complement of a negative value: 16 hex digits.
        "hex FFFFFFFFFFFFFFFF",
        "hex2 00FF",
        "int64 10",
        "int64def 7",
        "vd no",
        "backslash C:\\x\\",
        "nobackslash C:\\x",
        "stringofchar ---",
        "vartype 102",
        "vartype2 3",
        "lerp 5",
        "inverse 0.25",
        "clamp 10",
        "inrange yes",
        "fmod 1",
        "roundto 1.23",
        "sign -1",
        "compare -1",
        "iszero no",
    ] {
        assert!(
            lines.iter().any(|line| line == expected),
            "missing {expected:?} in {lines:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&folder);
}

/// The `TPerlRegEx` surface of `xejviScriptAdapterMiscRegEx.pas`.
#[test]
fn the_perl_regex_surface_works() {
    let _guard = session_lock();
    let folder = script_folder(
        "regex",
        "Regex.pas",
        "unit Regex;\n\
         function Initialize: Integer;\n\
         var\n\
           re: TPerlRegEx;\n\
           other: TStringList;\n\
           i: Integer;\n\
         begin\n\
           re := TPerlRegEx.Create;\n\
           re.Subject := 'foo=1 bar=22';\n\
           re.RegEx := '(\\w+)=(\\d+)';\n\
           while re.MatchAgain do begin\n\
             AddMessage(re.MatchedText + ' at ' + IntToHex64(re.MatchedOffset, 0));\n\
             for i := 0 to re.GroupCount do\n\
               AddMessage('group' + IntToHex64(i, 0) + '=' + re.Groups[i]);\n\
           end;\n\
           AddMessage('found ' + IfThen(re.FoundMatch, 'yes', 'no'));\n\
           re.Replacement := '$2:$1';\n\
           AddMessage('replaceall ' + IfThen(re.ReplaceAll, 'yes', 'no'));\n\
           AddMessage('subject ' + re.Subject);\n\
           AddMessage('escape ' + re.EscapeRegExChars('a.b(c)'));\n\
           AddMessage('compiled ' + IfThen(re.Compiled, 'yes', 'no'));\n\
           re.Study;\n\
           AddMessage('studied ' + IfThen(re.Studied, 'yes', 'no'));\n\
           re.RegEx := '(?P<first>\\w+)=(?P<second>\\d+)';\n\
           re.Subject := 'key=9';\n\
           re.Match;\n\
           AddMessage('named ' + IntToHex64(re.NamedGroup('second'), 0));\n\
           AddMessage('left ' + re.SubjectLeft + ' right ' + re.SubjectRight);\n\
           AddMessage('length ' + IntToHex64(re.MatchedLength, 0));\n\
           re.Replacement := '$1';\n\
           AddMessage('computed ' + re.ComputeReplacement);\n\
           other := TStringList.Create;\n\
           re.RegEx := ';';\n\
           re.Subject := 'a;b;c';\n\
           re.Split(other, 0);\n\
           AddMessage('split'#13#10 + other.Text);\n\
           other.Free;\n\
           re.Free;\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Regex.pas", json!({}));
    assert_eq!(result["aborted"], false, "{}", script_fixture::abort_text(&result));
    assert_eq!(
        lines(&result),
        [
            "Start: Applying script \"Regex\"",
            "foo=1 at 1",
            "group0=foo=1",
            "group1=foo",
            "group2=1",
            "bar=22 at 7",
            "group0=bar=22",
            "group1=bar",
            "group2=22",
            "found no",
            "replaceall yes",
            "subject 1:foo 22:bar",
            "escape a\\.b\\(c\\)",
            "compiled yes",
            "studied yes",
            "named 2",
            "left  right ",
            "length 5",
            "computed key",
            "split",
            "a",
            "b",
            "c",
            "",
            "Script terminated itself, Result=1",
            "Done: Applying script \"Regex\", Processed Records: 0, Elapsed Time: 00:00",
            "You can close this application now.",
        ],
        "{}",
        script_fixture::abort_text(&result)
    );
    let _ = std::fs::remove_dir_all(&folder);
}

/// The `JsonDataObjects` surface of `xejviScriptAdapterMiscJson.pas`.
#[test]
fn the_json_surface_works() {
    let _guard = session_lock();
    let folder = script_folder(
        "json",
        "Json.pas",
        "unit Json;\n\
         function Initialize: Integer;\n\
         var\n\
           obj, child, copy: TJsonObject;\n\
           arr: TJsonArray;\n\
         begin\n\
           // The serialization configuration is process-wide (upstream's\n\
           // unit variable); set it so the test does not depend on the\n\
           // demo's own `SetJDOLineBreak(#13#10)`.\n\
           SetJDOLineBreak(#10);\n\
           obj := TJsonObject.Create;\n\
           obj.S['name'] := 'xedit';\n\
           obj.I['count'] := 3;\n\
           obj.B['flag'] := True;\n\
           obj.F['ratio'] := 0.5;\n\
           obj['easy'] := 'yes';\n\
           arr := obj.A['list'];\n\
           arr.Add(1);\n\
           arr.Add('two');\n\
           child := arr.AddObject;\n\
           child.S['kind'] := 'child';\n\
           AddMessage('compact ' + obj.ToJSON(True));\n\
           AddMessage('pretty'#13#10 + obj.ToJSON(False));\n\
           AddMessage('count ' + IntToHex64(arr.Count, 0));\n\
           AddMessage('types ' + IntToHex64(obj.Types['count'], 0));\n\
           AddMessage('easy ' + obj['easy']);\n\
           AddMessage('get ' + obj.S['name']);\n\
           AddMessage('indexof ' + IntToHex64(obj.IndexOf('ratio'), 0));\n\
           AddMessage('contains ' + IfThen(obj.Contains('nope'), 'yes', 'no'));\n\
           AddMessage('isnull ' + IfThen(obj.IsNull('nope'), 'yes', 'no'));\n\
           AddMessage('child ' + arr.O[2].S['kind']);\n\
           copy := TJsonObject.Parse(obj.ToJSON(True));\n\
           AddMessage('parsed ' + copy.S['name']);\n\
           copy.Assign(obj);\n\
           AddMessage('assigned ' + copy.A['list'].S[1]);\n\
           copy.SaveToFile(TempPath + 'xedit-script-misc-json.json');\n\
           copy.Clear;\n\
           copy.LoadFromFile(TempPath + 'xedit-script-misc-json.json');\n\
           AddMessage('loaded ' + copy.S['easy']);\n\
           obj.Free;\n\
           copy.Free;\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    // The run's `-O:` default is the data folder; the `TempPath` of the
    // probe's write is the application's temp folder, which the GUI would
    // have created.
    let temp = std::env::temp_dir().join("SSEEdit");
    std::fs::create_dir_all(&temp).unwrap();
    let mut session = open_session();
    let result = run(&mut session, &folder, "Json.pas", json!({}));
    assert_eq!(result["aborted"], false, "{}", script_fixture::abort_text(&result));
    let lines = lines(&result);
    assert_eq!(
        lines.iter().find(|line| line.starts_with("compact ")).unwrap(),
        "compact {\"name\":\"xedit\",\"count\":3,\"flag\":true,\"ratio\":0.5,\"easy\":\"yes\",\"list\":[1,\"two\",{\"kind\":\"child\"}]}",
        "{lines:?}"
    );
    assert_eq!(lines.iter().find(|line| line.starts_with("count ")).unwrap(), "count 3");
    assert_eq!(lines.iter().find(|line| line.starts_with("types ")).unwrap(), "types 2");
    assert_eq!(lines.iter().find(|line| line.starts_with("easy ")).unwrap(), "easy yes");
    assert_eq!(lines.iter().find(|line| line.starts_with("get ")).unwrap(), "get xedit");
    assert_eq!(
        lines.iter().find(|line| line.starts_with("indexof ")).unwrap(),
        "indexof 3"
    );
    assert_eq!(
        lines.iter().find(|line| line.starts_with("contains ")).unwrap(),
        "contains no"
    );
    assert_eq!(
        lines.iter().find(|line| line.starts_with("isnull ")).unwrap(),
        "isnull yes"
    );
    assert_eq!(
        lines.iter().find(|line| line.starts_with("child ")).unwrap(),
        "child child"
    );
    assert_eq!(
        lines.iter().find(|line| line.starts_with("parsed ")).unwrap(),
        "parsed xedit"
    );
    assert_eq!(
        lines.iter().find(|line| line.starts_with("assigned ")).unwrap(),
        "assigned two"
    );
    assert_eq!(
        lines.iter().find(|line| line.starts_with("loaded ")).unwrap(),
        "loaded yes"
    );
    // The readable form: one entry per line at the tab indents, closed by
    // the line break of the configuration (the default `#10`, which
    // `AddMessage` does not split -- only CRLF separates log lines; the
    // probe's own `#13#10` before the text does split it).
    let pretty = lines.iter().position(|line| line == "pretty").unwrap();
    assert_eq!(
        lines[pretty + 1],
        "{\n\t\"name\": \"xedit\",\n\t\"count\": 3,\n\t\"flag\": true,\n\t\"ratio\": 0.5,\n\t\"easy\": \"yes\",\n\t\"list\": [\n\t\t1,\n\t\t\"two\",\n\t\t{\n\t\t\t\"kind\": \"child\"\n\t\t}\n\t]\n}\n",
        "{lines:?}"
    );
    let _ = std::fs::remove_file(temp.join("xedit-script-misc-json.json"));
    let _ = std::fs::remove_dir_all(&folder);
}

/// The corpus's `JSON - Demo.pas`: the part that needs only `AddMessage`,
/// `wbGameMode` and the `JsonDataObjects` surface runs, and the script stops
/// at its first `IntToStr` -- a JVCL `SysUtils` function, phase 6 step 8.
/// Once step 8 lands, the whole demo runs and the full log is the one
/// recorded by `the_json_surface_works` plus the demo's own text.
#[test]
fn the_json_demo_corpus_script_runs_to_the_misc_surface() {
    let _guard = session_lock();
    let scripts = std::env::var_os("XEDIT_SCRIPTS").map(PathBuf::from);
    let scripts = match scripts {
        Some(scripts) => Some(scripts),
        None => std::env::var_os("XEDIT_ORACLE_DIR").map(|dir| PathBuf::from(dir).join("Edit Scripts")),
    };
    let Some(folder) = scripts else {
        return;
    };
    if !folder.join("JSON - Demo.pas").is_file() {
        return;
    }
    let mut session = open_session();
    let result = run(&mut session, &folder, "JSON - Demo.pas", json!({}));
    let lines = lines(&result);
    // `Demo1`: `TJsonBaseObject.Parse`, `S[]`, `A[]`, `Count`, `I[]`.
    assert_eq!(lines[0], "Start: Applying script \"JSON - Demo\"", "{lines:?}");
    assert_eq!(lines[1], "---------- Demo1 ----------", "{lines:?}");
    assert_eq!(lines[2], "bar", "{lines:?}");
    if result["aborted"] == false {
        // Step 8's `IntToStr` is in: the whole demo runs.
        assert_eq!(
            lines[3], "2",
            "the demo's Demo1 continues with IntToStr, so the whole log should \
             be the full demo run"
        );
        return;
    }
    let text = script_fixture::abort_text(&result);
    assert!(
        text.contains("Undeclared Identifier 'IntToStr'"),
        "the demo should stop at its first JVCL SysUtils call, not elsewhere: {text}"
    );
    assert!(text.contains("Aborted: Applying script \"JSON - Demo\""), "{text}");
}

/// An adapter error carries its Delphi class name into the run's
/// `Exception in unit ...: [<class>] <message>` line: the RTL's
/// `ERegularExpressionError` texts and the JSON library's cast exception
/// (`ValueError::ExternalClass`).
#[test]
fn the_misc_errors_name_their_delphi_class() {
    let _guard = session_lock();
    let folder = script_folder(
        "errors",
        "Errors.pas",
        "unit Errors;\n\
         function Initialize: Integer;\n\
         var\n\
           re: TPerlRegEx;\n\
         begin\n\
           re := TPerlRegEx.Create;\n\
           re.Subject := 'abc';\n\
           re.RegEx := 'z';\n\
           re.Match;\n\
           AddMessage(re.Groups[0]);\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let mut session = open_session();
    let result = run(&mut session, &folder, "Errors.pas", json!({}));
    assert_eq!(result["aborted"], true);
    let text = script_fixture::abort_text(&result);
    assert!(
        // The propagating exception keeps its raw message; the unit and the
        // line live on the recorded `FLastError` and come out as the run
        // loop's outer location -- the oracle's line shape.
        text.contains("Exception in unit Errors line 1: [ERegularExpressionError] Successful match required"),
        "{text}"
    );
    assert!(text.contains("Aborted: Applying script \"Errors\""), "{text}");

    let folder = script_folder(
        "errors-json",
        "ErrorsJson.pas",
        "unit ErrorsJson;\n\
         function Initialize: Integer;\n\
         var\n\
           obj: TJsonObject;\n\
           o: TJsonObject;\n\
         begin\n\
           obj := TJsonObject.Create;\n\
           obj.S['name'] := 'x';\n\
           o := obj.O['name'];\n\
           AddMessage('never');\n\
           Result := 1;\n\
         end;\n\
         end.\n",
    );
    let result = run(&mut session, &folder, "ErrorsJson.pas", json!({}));
    assert_eq!(result["aborted"], true);
    let text = script_fixture::abort_text(&result);
    assert!(
        text.contains("Exception in unit ErrorsJson line 1: [EJsonCastException] Cannot cast String into Object"),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&folder);
}
