// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeScriptHost.pas (IxeScript, TxeScriptHost,
// CreateForName/CreateScript/Init/Register) and xEdit/JvI/xejviScriptHost.pas
// (TxejviScript: Create with the uses namespace stripping, FunctionExists,
// FunctionIsEmpty, CallFunction with VResult, GetLastErrorLocation, and
// JvInterpreterProgramGetUnitSource with the built-in unit stubs)

//! The script host: one compiled script (`IxeScript`) and the registry of
//! the hosts that compile them (`TxeScriptHost`).
//!
//! A [`Script`] is the port of `TxejviScript`: it strips the Delphi
//! namespace prefixes from the `uses` clauses ([`crate::check::strip_namespaces`],
//! as `TxejviScript.Create` does), resolves the units a script uses through
//! [`unit_source`] (a file beside the script, or the empty stub of a unit
//! compiled into the host -- upstream `IsUnitCompiledIn`), compiles with
//! [`Interpreter::compile`], and runs the script's routines with
//! [`Script::call_function`].
//!
//! The host does not run the script: `xeMainForm.pas`'s `ApplyScript` does,
//! and its port is `xedit_session::script` (`script.run`). The registrations
//! the script sees (the adapter and the `OnSetValue`/`OnStatement` hooks)
//! belong to that caller, which hands the host a configured
//! [`Interpreter`].
//!
//! `TxeScriptHost`'s name registry is the [`ScriptHost`] section below: the
//! names of the hosts the build carries (`JvI`; upstream selects one with
//! `-scripthost:` and `xeDefaultScriptHost`), the "has not been initialized"
//! and "is not supported" errors of the two entry points, and `Init`'s
//! "has already been initialized".

// The host's errors are the interpreter's (`ExecError`, the port of
// `EJvInterpreterError`), a value the caller moves around; boxing it would
// hide the structure the port mirrors.
#![allow(clippy::result_large_err)]

use std::path::Path;

use crate::check;
use crate::eval::{ExecError, Interpreter};
use crate::values::Value;

/// The units the interpreter's host provides without a file: `xEditAPI`,
/// `UITypes` and the units compiled into the executable, which
/// `JvInterpreterProgramGetUnitSource` answers with `unit <name>; end.`
/// (`xejviScriptHost.pas:444`).
///
/// Upstream asks the executable's own package list (`IsUnitCompiledIn`,
/// `GetPackageInfo`) and strips the five namespace prefixes from every
/// enumerated name before the comparison. The port cannot enumerate the
/// linked units of the Delphi build, so it lists the units the script API
/// registers under: the xEdit adapter units (`wbInterface`, `wbDataFormat`,
/// `TwbVector`, `TwbGridCell`), the Delphi units the 14 JVCL adapter units
/// register (phase 6 step 8), and the RTL units the xEdit adapters register
/// (`StrUtils`, `IOUtils`, ...), whose names are compared the same way (with
/// the prefixes stripped), so `uses Vcl.Graphics` matches `Graphics`. A unit
/// outside the list resolves from the scripts folder, and one in neither is
/// `ieUnitNotFound`, as upstream.
pub const HOST_UNITS: &[&str] = &[
    // The two units the host answers explicitly, whatever the folder holds.
    "xEditAPI",
    "UITypes",
    // The xEdit units of the script API.
    "wbInterface",
    "wbDataFormat",
    "TwbVector",
    "TwbGridCell",
    // The Delphi units of the step-8 JVCL adapters.
    "System",
    "SysUtils",
    "Classes",
    "Math",
    "Dialogs",
    "Windows",
    "Graphics",
    "Controls",
    "Buttons",
    "StdCtrls",
    "ComCtrls",
    "ExtCtrls",
    "Forms",
    "Menus",
    // The RTL units the xEdit adapters register under.
    "StrUtils",
    "IOUtils",
    "IniFiles",
    "RegularExpressionsCore",
    "Variants",
    "Types",
    "ShellApi",
    "Registry",
    "FileCtrl",
    "CheckLst",
    "JsonDataObjects",
    "Clipbrd",
];

/// Whether `unit_name` is one of the units the interpreter's host provides
/// (`IsUnitCompiledIn`): the comparison strips the same namespace prefixes
/// from the host's name as `HasUnitProc` does, and is without case.
fn is_host_unit(unit_name: &str) -> bool {
    HOST_UNITS.iter().any(|host| {
        let stripped = check::strip_unit_prefixes(host.as_bytes());
        stripped.eq_ignore_ascii_case(unit_name.as_bytes())
    })
}

/// `JvInterpreterProgramGetUnitSource` (`xejviScriptHost.pas:439`): the
/// source of the unit `unit_name`, or `None` for the `ieUnitNotFound` the
/// interpreter raises (`JvInterpreter.pas:8042`).
///
/// A host unit answers `unit <name>; end.`; another name is read from
/// `<scripts_folder>\<Unit>.pas` (the folder is `wbScriptsPath`, which the
/// run sets to the script's own folder, `xeMainForm.pas:5075`). The file's
/// bytes are handed to the interpreter as they are, as `crate::check` reads
/// them.
pub fn unit_source(scripts_folder: &Path, unit_name: &str) -> Option<Vec<u8>> {
    if is_host_unit(unit_name) {
        return Some(host_unit_stub(unit_name));
    }
    let path = check::find_unit_file(scripts_folder, unit_name)?;
    std::fs::read(path).ok()
}

/// The `unit <name>; end.` source of a unit compiled into the host, which
/// `JvInterpreterProgramGetUnitSource` answers with (`xejviScriptHost.pas:445`).
pub fn host_unit_stub(unit_name: &str) -> Vec<u8> {
    format!("unit {unit_name}; end.").into_bytes()
}

/// One compiled script: `IxeScript` (`xeScriptHost.pas:19`), implemented by
/// `TxejviScript` (`xejviScriptHost.pas:31`).
pub struct Script {
    /// `FScriptName`: the unit name the run reports, the file name without
    /// its extension (`ChangeFileExt(ExtractFileName(xeScriptToRun),'')`,
    /// `xeMainForm.pas:5079`).
    name: String,
    /// `FScript`: the source after the namespace stripping, which
    /// [`Script::function_is_empty`] scans.
    source: Vec<u8>,
    /// `FProgram`: the interpreter (`TJvInterpreterProgram`).
    program: Interpreter,
}

impl Script {
    /// `TxejviScript.Create` (`xejviScriptHost.pas:337`): strip the `uses`
    /// namespace prefixes, wire `OnGetUnitSource` to [`unit_source`] and
    /// `Compile`. `program` is the interpreter the caller configured (its
    /// adapter carries the registrations, its hooks the `OnSetValue` and
    /// `OnStatement` of the host); the unit source hook belongs to the script
    /// host and is installed here.
    ///
    /// A compile failure is the [`ExecError`] `Compile` raises.
    pub fn create(
        name: &str,
        source: &[u8],
        scripts_folder: &Path,
        mut program: Interpreter,
    ) -> Result<Script, ExecError> {
        let source = check::strip_namespaces(source);
        let folder = scripts_folder.to_path_buf();
        program.set_unit_source(move |unit_name| unit_source(&folder, unit_name));
        program.compile(&source)?;
        Ok(Script {
            name: name.to_owned(),
            source,
            program,
        })
    }

    /// `FScriptName`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `FScript`: the namespace-stripped source the script was compiled from.
    pub fn source(&self) -> &[u8] {
        &self.source
    }

    /// The interpreter, for the caller that needs its error state or its
    /// adapter.
    pub fn program(&self) -> &Interpreter {
        &self.program
    }

    /// `TxejviScript.FunctionExists` (`xejviScriptHost.pas:381`):
    /// `FProgram.FunctionExists('', aName)` -- a routine of the script's own
    /// unit (or of a unit it uses, as the empty unit name matches any).
    pub fn function_exists(&self, name: &str) -> bool {
        self.program.function_exists("", name)
    }

    /// `TxejviScript.FunctionIsEmpty` (`xejviScriptHost.pas:386`), the
    /// source-text scan `ApplyScript` skips `Initialize`/`Process`/`Finalize`
    /// with: the *last* registered routine of that name in the script text,
    /// lowercased, cut at its first `begin`, with every space, tab, CR and LF
    /// removed, is empty when it reads `begin end` or `begin Result := 0;
    /// end` (each with or without the `;`/`.` that follows). A routine with
    /// no `begin` in its text (a declaration, an `external`) is not empty, a
    /// missing routine is not empty either.
    pub fn function_is_empty(&self, name: &str) -> bool {
        let Some(desc) = self.source_function(name) else {
            return false;
        };
        let span = &desc.decl.span;
        if span.end <= span.start || span.end > self.source.len() {
            return false;
        }
        let body = &self.source[span.start..span.end];
        let lower: Vec<u8> = body.iter().map(|byte| byte.to_ascii_lowercase()).collect();
        // `P := Pos('begin', Body)`: the first occurrence.
        let Some(at) = find(&lower, b"begin") else {
            return false;
        };
        // `Delete(Body, I, 1)` for every ' ', #9, #10, #13.
        let stripped: Vec<u8> = lower[at..]
            .iter()
            .copied()
            .filter(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
            .collect();
        /// The `Result` of `TxejviScript.FunctionIsEmpty`: a body that only
        /// opens and closes, or sets `Result := 0`, is empty.
        const EMPTY_BODIES: [&[u8]; 6] = [
            b"beginend",
            b"beginend;",
            b"beginend.",
            b"beginresult:=0;end",
            b"beginresult:=0;end;",
            b"beginresult:=0;end.",
        ];
        EMPTY_BODIES.contains(&stripped.as_slice())
    }

    /// The `FunctionDesc` `FunctionIsEmpty` reads, as the Pascal loops the
    /// `SrcFunctionList` from its last entry down and takes the first
    /// identifier match (case-insensitively).
    fn source_function(&self, name: &str) -> Option<std::rc::Rc<crate::adapter::FunctionDesc>> {
        self.program.adapter.find_fun_desc("", name.as_bytes(), b"")
    }

    /// `TxejviScript.CallFunction` (`xejviScriptHost.pas:331`):
    /// `FProgram.CallFunction(aName, nil, aParams)` and the program's
    /// `VResult`. The parameters are values the caller built (an element is
    /// the object value of the run's element wrapper).
    pub fn call_function(&mut self, name: &str, params: &[Value]) -> Result<Value, ExecError> {
        self.program.call_function(name, params)
    }

    /// `TxejviScript.GetLastErrorLocation` (`xejviScriptHost.pas:430`):
    /// `unit <ErrUnitName> line <ErrLine>`. Upstream's `LastError` object is
    /// always assigned, so a host that never failed reports `unit  line -1`
    /// rather than the empty text its `else` branch suggests;
    /// `Interpreter::last_error_location` carries the same note.
    pub fn last_error_location(&self) -> String {
        self.program.last_error_location()
    }

    /// `FProgram.LastError`.
    pub fn last_error(&self) -> &ExecError {
        self.program.last_error()
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

//=== TxeScriptHost: the name registry ========================================

use std::sync::RwLock;

/// `xeDefaultScriptHost` (`xeInit.pas:47`): the host `xeInit.pas` selects
/// when the command line names none.
pub const DEFAULT_SCRIPT_HOST: &str = "JvI";

/// The hosts this build carries (`TxeScriptHost.Register`): the port of the
/// JvInterpreter host is the only one.
pub const SCRIPT_HOSTS: &[&str] = &["JvI"];

/// `_ScriptHost` (`xeScriptHost.pas:76`): the one host `Init` selected.
static SCRIPT_HOST: RwLock<Option<&'static str>> = RwLock::new(None);

/// `TxeScriptHost.CreateForName`'s name test: whether a host of that name is
/// supported (upstream `SameText`).
pub fn host_supported(name: &str) -> bool {
    SCRIPT_HOSTS.iter().any(|host| host.eq_ignore_ascii_case(name))
}

/// `TxeScriptHost.Init` (`xeScriptHost.pas:85`): select the host by name.
/// A second `Init` is upstream's `TxeScriptHost has already been
/// initialized`; an unknown name is `CreateForName`'s `ScriptHost "<name>"
/// is not supported`.
pub fn init(name: &str) -> Result<(), String> {
    let mut host = SCRIPT_HOST.write().unwrap();
    if host.is_some() {
        return Err("TxeScriptHost has already been initialized".to_owned());
    }
    if !host_supported(name) {
        return Err(format!("ScriptHost \"{name}\" is not supported"));
    }
    let found = SCRIPT_HOSTS
        .iter()
        .find(|host| host.eq_ignore_ascii_case(name))
        .expect("checked by host_supported");
    *host = Some(found);
    Ok(())
}

/// The selected host, or `None` before [`init`].
pub fn initialized_host() -> Option<&'static str> {
    *SCRIPT_HOST.read().unwrap()
}

/// Clears the selection, as the `finalization` of `xeScriptHost.pas` frees
/// `_ScriptHost`. For tests that initialize more than once.
pub fn reset_host() {
    *SCRIPT_HOST.write().unwrap() = None;
}

/// `TxeScriptHost.CreateScript` (`xeScriptHost.pas:78`): compile a script
/// with the selected host. Without [`init`] it is upstream's
/// `TxeScriptHost has not been initialized`.
pub fn create_script(name: &str, source: &[u8], scripts_folder: &Path, program: Interpreter) -> Result<Script, String> {
    if initialized_host().is_none() {
        return Err("TxeScriptHost has not been initialized".to_owned());
    }
    Script::create(name, source, scripts_folder, program).map_err(|error| error.message().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_host_registry_follows_upstream() {
        reset_host();
        assert!(!host_supported("JvI2"));
        assert!(host_supported("jvi"));
        let error = match create_script("S", b"unit S; end.", Path::new("."), Interpreter::new()) {
            Ok(_) => panic!("no host was initialized"),
            Err(error) => error,
        };
        assert_eq!(error, "TxeScriptHost has not been initialized");
        assert_eq!(init("Nope").unwrap_err(), "ScriptHost \"Nope\" is not supported");
        init("JvI").unwrap();
        assert_eq!(initialized_host(), Some("JvI"));
        assert_eq!(init("JvI").unwrap_err(), "TxeScriptHost has already been initialized");
        reset_host();
        assert_eq!(initialized_host(), None);
    }

    #[test]
    fn host_units_compare_with_the_prefixes_stripped() {
        assert!(is_host_unit("xEditAPI"));
        assert!(is_host_unit("System"));
        assert!(is_host_unit("Graphics"));
        assert!(!is_host_unit("wbImplementation"));
        assert!(!is_host_unit("Nope"));
        assert_eq!(host_unit_stub("xEditAPI"), b"unit xEditAPI; end.");
    }
}
