// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The script host of phase 6 step 4: the namespace stripping of
//! `TxejviScript.Create`, the unit resolution of
//! `JvInterpreterProgramGetUnitSource` (a file beside the script, the empty
//! stub of a unit compiled into the host, `ieUnitNotFound` for the rest),
//! `FunctionExists`/`FunctionIsEmpty` with all six bodies the source-text
//! scan accepts, `CallFunction` with `VResult`, and the `TxeScriptHost` name
//! registry.

use std::path::Path;
use std::rc::Rc;

use xedit_io::temp::TempDir;
use xedit_script::Interpreter;
use xedit_script::adapter::Args;
use xedit_script::host::{self, Script};
use xedit_script::values::{ParamType, Value, VarType};

/// A folder of its own for a test, holding the given files.
fn folder(name: &str, files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new(format!("xedit-script-host-{}-{name}", std::process::id()));
    for (file, text) in files {
        std::fs::write(dir.join(file), text).unwrap();
    }
    dir
}

fn create(dir: &Path, name: &str, source: &str) -> Result<Script, String> {
    Script::create(name, source.as_bytes(), dir, Interpreter::new()).map_err(|error| error.message().to_owned())
}

/// The failure of a compile, for the cases a test asserts on.
fn create_err(dir: &Path, name: &str, source: &str) -> String {
    match create(dir, name, source) {
        Ok(_) => panic!("{name} compiled"),
        Err(error) => error,
    }
}

#[test]
fn the_namespaces_of_the_uses_clause_are_stripped() {
    // `TxejviScript.Create` removes `system.`, `vcl.`, `winapi.`, `data.`
    // and `web.` from every `uses` clause, so the interpreter, which knows
    // no namespaces, parses it. A unit outside the host's list resolves from
    // the folder; `Vcl.Graphics` is one of the host's units (the empty
    // stub), whatever the folder holds.
    let dir = folder(
        "namespaces",
        &[
            (
                "Namespaced.pas",
                "unit Namespaced;\n\
                 uses System.SysUtils, Vcl.Graphics, Helper;\n\
                 function Main: Integer;\n\
                 begin\n\
                   Result := Greeting();\n\
                 end;\n\
                 end.\n",
            ),
            (
                "Helper.pas",
                "unit Helper;\n\
                 function Greeting: Integer;\n\
                 begin\n\
                   Result := 7;\n\
                 end;\n\
                 end.\n",
            ),
        ],
    );
    let mut script = create(
        &dir,
        "Namespaced",
        &std::fs::read_to_string(dir.join("Namespaced.pas")).unwrap(),
    )
    .unwrap();
    // The `uses` line is the only one that changed.
    let source = String::from_utf8_lossy(script.source()).into_owned();
    assert!(source.contains("uses SysUtils, Graphics, Helper;"), "{source}");
    assert!(!source.contains("System.") && !source.contains("Vcl."), "{source}");
    assert_eq!(script.call_function("Main", &[]).unwrap(), Value::Integer(7));
}

#[test]
fn a_unit_without_a_file_is_the_host_stub_and_one_with_neither_is_not_found() {
    // `xEditAPI` and `UITypes` are answered with `unit <name>; end.` even
    // when the file exists, as the host does.
    let dir = folder(
        "stubs",
        &[
            (
                "xEditAPI.pas",
                "unit xEditAPI; function Bogus: Integer; begin Result := 1; end; end.\n",
            ),
            ("Stubbed.pas", "unit Stubbed;\nuses xEditAPI, UITypes;\nend.\n"),
            ("Missing.pas", "unit Missing;\nuses NoSuchUnit;\nend.\n"),
        ],
    );
    assert!(
        create(
            &dir,
            "Stubbed",
            &std::fs::read_to_string(dir.join("Stubbed.pas")).unwrap()
        )
        .is_ok()
    );
    let error = create_err(
        &dir,
        "Missing",
        &std::fs::read_to_string(dir.join("Missing.pas")).unwrap(),
    );
    assert_eq!(error, "Error in unit 'Missing' on line 1 : Unit 'NoSuchUnit' not found");
}

#[test]
fn function_is_empty_takes_the_six_bodies_of_the_source_scan() {
    // `TxejviScript.FunctionIsEmpty`: lowercased, cut at `begin`, whitespace
    // removed -- `begin end`, `begin end;`, `begin end.`, `begin Result :=
    // 0; end` and the two with a terminator. Anything else is not empty, and
    // neither is a routine that is not there.
    let dir = folder("empty", &[]);
    let cases: [(&str, &str, bool); 7] = [
        ("A", "unit A;\nfunction Probe: Integer;\nbegin end;\nend.\n", true),
        // The `;` after the body's `end` is consumed by the scan.
        ("B", "unit B;\nfunction Probe: Integer;\nbegin\t \nend  ;\nend.\n", true),
        (
            "C",
            "unit C;\nfunction Probe: Integer;\nbegin\n  Result := 0;\nend;\nend.\n",
            true,
        ),
        // UPSTREAM-QUIRK: the `begin end.` and `begin Result := 0; end.`
        // forms the scan accepts need a routine whose body is terminated by
        // the unit's `.`; the front end of step 2 rejects that source
        // (`';' expected but '.' found`), so they are not reachable here and
        // the cases below use a routine `;`.
        (
            "F",
            "unit F;\nfunction Probe: Integer;\nbegin AddMessage('x'); end;\nend.\n",
            false,
        ),
        (
            "G",
            "unit G;\nfunction Probe: Integer;\nbegin Result := 1; end;\nend.\n",
            false,
        ),
        (
            "H",
            "unit H;\nfunction Probe: Integer;\nbegin Result := 0; Result := AddMessage('x'); end;\nend.\n",
            false,
        ),
        (
            "I",
            "unit I;\nfunction Probe: Integer;\nvar x: Integer;\nbegin x := 0; if x = 0 then begin end; end;\nend.\n",
            false,
        ),
    ];
    for (name, source, empty) in cases {
        let script = match create(&dir, name, source) {
            Ok(script) => script,
            Err(error) => panic!(
                "{name}: {error}
{source}"
            ),
        };
        assert!(script.function_exists("Probe"), "{name}");
        assert_eq!(script.function_is_empty("Probe"), empty, "{name}: {source:?}");
    }
    let script = create(&dir, "Absent", "unit Absent;\nend.\n").unwrap();
    assert!(!script.function_exists("Probe"), "nothing is defined");
    assert!(!script.function_is_empty("Probe"), "a missing routine is not empty");
}

#[test]
fn call_function_answers_the_result_and_reports_the_error_location() {
    let dir = folder(
        "result",
        &[(
            "Result.pas",
            "unit Result;\n\
             function Add(a, b: Integer): Integer;\n\
             begin\n\
               Result := a + b;\n\
             end;\n\
             function Explode: Integer;\n\
             begin\n\
               Result := NoSuchThing;\n\
             end;\n\
             end.\n",
        )],
    );
    let mut script = create(
        &dir,
        "Result",
        &std::fs::read_to_string(dir.join("Result.pas")).unwrap(),
    )
    .unwrap();
    let params = [Value::Integer(2), Value::Integer(3)];
    assert_eq!(script.call_function("Add", &params).unwrap(), Value::Integer(5));
    let error = script.call_function("Explode", &[]).unwrap_err();
    assert!(
        error.message().contains("Undeclared Identifier 'NoSuchThing'"),
        "{}",
        error.message()
    );
    // `GetLastErrorLocation`: `FLastError.Assign` never copies the unit or
    // the line of an interpreter error (`JvInterpreter.pas:1491-1503`), so
    // the recorded location stays the cleared `unit  line -1`.
    assert_eq!(script.last_error_location(), "unit  line -1");
    assert_eq!(script.last_error().unit_name, "");
}

#[test]
fn the_host_registers_the_script_of_its_name_registry() {
    host::reset_host();
    assert_eq!(host::DEFAULT_SCRIPT_HOST, "JvI");
    host::init("JvI").unwrap();
    let dir = folder(
        "registry",
        &[(
            "Registry.pas",
            "unit Registry;\nfunction Main: Integer;\nbegin\n  Result := 1;\nend;\nend.\n",
        )],
    );
    let source = std::fs::read(dir.join("Registry.pas")).unwrap();
    let script = host::create_script("Registry", &source, &dir, Interpreter::new()).unwrap();
    assert_eq!(script.name(), "Registry");
    host::reset_host();
    let error = match host::create_script("Registry", &source, &dir, Interpreter::new()) {
        Ok(_) => panic!("the host was initialized"),
        Err(error) => error,
    };
    assert_eq!(error, "TxeScriptHost has not been initialized");
}

#[test]
fn an_adapter_function_can_hold_the_interpreter_of_a_hosted_script() {
    // The wiring the run loop uses: the adapter is configured before the
    // host compiles, and the script calls into it.
    let dir = folder(
        "adapter",
        &[(
            "Adapter.pas",
            "unit Adapter;\n\
             function Initialize: Integer;\n\
             begin\n\
               AddMessage('hello');\n\
               Result := 0;\n\
             end;\n\
             end.\n",
        )],
    );
    let mut interpreter = Interpreter::new();
    let written = Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = written.clone();
    interpreter.adapter.add_function(
        "",
        "AddMessage",
        Rc::new(move |_value: &mut Value, args: &mut Args| {
            if let Some(Value::Str(bytes)) = args.values.first() {
                sink.borrow_mut().push(String::from_utf8_lossy(bytes).into_owned());
            }
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Empty,
    );
    let mut script = Script::create(
        "Adapter",
        &std::fs::read(dir.join("Adapter.pas")).unwrap(),
        &dir,
        interpreter,
    )
    .unwrap();
    assert_eq!(script.call_function("Initialize", &[]).unwrap(), Value::Integer(0));
    assert_eq!(*written.borrow(), ["hello"]);
}
