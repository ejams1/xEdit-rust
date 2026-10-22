// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeScriptHost.pas (IxeScript, TxeScriptHost:
// Create, CreateForName, CreateScript, Init, Register, GetName,
// CreateScriptInternal)

//! `xeScriptHost`: the seam between the GUI and the script interpreter, as
//! the dialogs see it. A host is registered under a name, one host is
//! initialized for the process, and `CreateScript(name, source)` compiles a
//! script into an `IxeScript` the caller drives through
//! `FunctionExists`/`CallFunction`.
//!
//! The port registers the Pascal host instead of upstream's JvInterpreter
//! host (`xejviScriptHost.pas`): `xedit_script::Interpreter` compiles the
//! script and resolves its `uses` from the scripts folder. Running a script
//! over the loaded plugins still goes through `script.run` (the GUI's Apply
//! Script); this seam is what the form checks before it offers the text,
//! and what a dialog's own compiled unit would be made through.
//!
//! No command is added: the seam is process state, not an operation.

use std::cell::RefCell;
use std::collections::HashMap;

use xedit_script::Interpreter;
use xedit_script::values::Value;

/// The failure of a host lookup or a script compile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptHostError(pub String);

impl std::fmt::Display for ScriptHostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScriptHostError {}

/// `IxeScript` (`:19-26`).
pub trait IxeScript {
    /// `FunctionExists`.
    fn function_exists(&self, name: &str) -> bool;
    /// `FunctionIsEmpty`.
    fn function_is_empty(&self, name: &str) -> bool;
    /// `CallFunction`.
    fn call_function(&mut self, name: &str, params: &[Value]) -> Result<Value, ScriptHostError>;
    /// `GetLastErrorLocation`: `unit <name> line <n>`.
    fn get_last_error_location(&self) -> String;
}

/// `TxeScriptHost`: one registered script host.
pub trait ScriptHost {
    /// `GetName`.
    fn get_name(&self) -> String;
    /// The `class of TxeScriptHost` row of the registry: a new host with the
    /// registered one's configuration.
    fn create(&self) -> Box<dyn ScriptHost>;
    /// `CreateScriptInternal`.
    fn create_script_internal(&self, script_name: &str, script: &str) -> Result<Box<dyn IxeScript>, ScriptHostError>;
}

/// The compiled Pascal script over `xedit_script::Interpreter`.
pub struct PascalScript {
    interpreter: Interpreter,
}

impl IxeScript for PascalScript {
    fn function_exists(&self, name: &str) -> bool {
        // An empty unit name matches any unit (`find_fun_desc`).
        self.interpreter.function_exists("", name)
    }

    fn function_is_empty(&self, _name: &str) -> bool {
        // UPSTREAM-QUIRK: the interpreter keeps no source of a compiled
        // routine, so the port cannot tell an empty body; the GUI never
        // asks (`xeMainForm.pas` checks only `FunctionExists` before
        // calling `Process`).
        false
    }

    fn call_function(&mut self, name: &str, params: &[Value]) -> Result<Value, ScriptHostError> {
        self.interpreter
            .call_function(name, params)
            .map_err(|error| ScriptHostError(format!("{error:?}")))
    }

    fn get_last_error_location(&self) -> String {
        self.interpreter.last_error_location()
    }
}

/// `TxeScriptHost.CreateForName`'s Pascal host: the interpreter with the
/// scripts folder as its `OnGetUnitSource`.
pub struct PascalScriptHost {
    /// `wbScriptsPath`: where a `uses` unit is read from.
    pub scripts_folder: String,
}

impl ScriptHost for PascalScriptHost {
    fn get_name(&self) -> String {
        "Pascal".to_owned()
    }

    fn create(&self) -> Box<dyn ScriptHost> {
        Box::new(PascalScriptHost {
            scripts_folder: self.scripts_folder.clone(),
        })
    }

    fn create_script_internal(&self, script_name: &str, script: &str) -> Result<Box<dyn IxeScript>, ScriptHostError> {
        let folder = self.scripts_folder.clone();
        let mut interpreter = Interpreter::new();
        interpreter.set_unit_source(move |unit| {
            let path = std::path::Path::new(&folder).join(format!("{unit}.pas"));
            std::fs::read(&path).ok()
        });
        // `CreateScript(aScriptName, aScript)`: the unit name is the script
        // name, so a `uses` of the script itself resolves.
        let source = if script.is_empty() {
            format!("unit {script_name};\nend.")
        } else {
            script.to_owned()
        };
        interpreter
            .compile(source.as_bytes())
            .map_err(|error| ScriptHostError(format!("{script_name}: {error:?}")))?;
        Ok(Box::new(PascalScript { interpreter }))
    }
}

/// The process's hosts and the initialized one (`_ScriptHosts` and
/// `_ScriptHost` of the unit).
#[derive(Default)]
pub struct ScriptHosts {
    hosts: HashMap<String, Box<dyn ScriptHost>>,
    current: Option<Box<dyn ScriptHost>>,
}

impl ScriptHosts {
    /// `Register`: a host name registers once.
    pub fn register(&mut self, host: Box<dyn ScriptHost>) {
        self.hosts.entry(host.get_name()).or_insert(host);
    }

    /// `CreateForName`: the registered host, else
    /// `ScriptHost "<name>" is not supported`.
    pub fn create_for_name(&self, name: &str) -> Result<Box<dyn ScriptHost>, ScriptHostError> {
        match self.hosts.get(name) {
            Some(host) => Ok(host.create()),
            None => Err(ScriptHostError(format!("ScriptHost \"{name}\" is not supported"))),
        }
    }

    /// `Init`: one host for the process; a second init is
    /// `TxeScriptHost has already been initialized`.
    pub fn init(&mut self, name: &str) -> Result<(), ScriptHostError> {
        if self.current.is_some() {
            return Err(ScriptHostError("TxeScriptHost has already been initialized".to_owned()));
        }
        self.current = Some(self.create_for_name(name)?);
        Ok(())
    }

    /// `CreateScript`: without `Init` it is
    /// `TxeScriptHost has not been initialized`.
    pub fn create_script(&self, script_name: &str, script: &str) -> Result<Box<dyn IxeScript>, ScriptHostError> {
        let Some(host) = &self.current else {
            return Err(ScriptHostError("TxeScriptHost has not been initialized".to_owned()));
        };
        host.create_script_internal(script_name, script)
    }

    pub fn is_initialized(&self) -> bool {
        self.current.is_some()
    }

    /// The unit's `finalization`: drop the initialized host.
    pub fn clear(&mut self) {
        self.current = None;
    }
}

thread_local! {
    /// The unit's class variables, per thread: an `Interpreter` holds `Rc`s
    /// and cannot cross threads.
    static HOSTS: RefCell<ScriptHosts> = RefCell::new(ScriptHosts::default());
}

/// Runs `f` over the thread's host registry (`_ScriptHosts`).
pub fn with_hosts<R>(f: impl FnOnce(&mut ScriptHosts) -> R) -> R {
    HOSTS.with(|hosts| f(&mut hosts.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xedit_script::values::Value;

    struct NoHost;

    impl ScriptHost for NoHost {
        fn get_name(&self) -> String {
            "NoScript".to_owned()
        }

        fn create(&self) -> Box<dyn ScriptHost> {
            Box::new(NoHost)
        }

        fn create_script_internal(&self, _name: &str, _script: &str) -> Result<Box<dyn IxeScript>, ScriptHostError> {
            Err(ScriptHostError("not a script host".to_owned()))
        }
    }

    #[test]
    fn a_name_registers_once_and_an_unknown_name_is_refused() {
        let mut hosts = ScriptHosts::default();
        hosts.register(Box::new(NoHost));
        hosts.register(Box::new(NoHost));
        assert_eq!(hosts.hosts.len(), 1);
        let error = match hosts.create_for_name("Other") {
            Ok(_) => panic!("an unknown host was created"),
            Err(error) => error,
        };
        assert_eq!(error.0, "ScriptHost \"Other\" is not supported");
    }

    #[test]
    fn init_is_once_per_process() {
        let mut hosts = ScriptHosts::default();
        hosts.register(Box::new(PascalScriptHost {
            scripts_folder: ".".to_owned(),
        }));
        assert!(!hosts.is_initialized());
        assert_eq!(
            match hosts.create_script("x", "") {
                Ok(_) => panic!("a script was created without a host"),
                Err(error) => error.0,
            },
            "TxeScriptHost has not been initialized"
        );
        hosts.init("Pascal").unwrap();
        assert!(hosts.is_initialized());
        assert_eq!(
            hosts.init("Pascal").unwrap_err().0,
            "TxeScriptHost has already been initialized"
        );
        hosts.clear();
        assert!(!hosts.is_initialized());
    }

    #[test]
    fn the_pascal_host_compiles_a_script_and_reports_its_functions() {
        let host = PascalScriptHost {
            scripts_folder: ".".to_owned(),
        };
        let mut script = host
            .create_script_internal(
                "Test",
                "unit Test;\nfunction Initialize: integer;\nbegin\n  Result := 1;\nend;\nend.",
            )
            .unwrap();
        assert!(script.function_exists("Initialize"));
        assert!(!script.function_exists("Finalize"));
        assert!(!script.function_is_empty("Initialize"));
        let value = script.call_function("Initialize", &[]).unwrap();
        assert!(matches!(value, Value::Integer(1) | Value::Int64(1)), "{value:?}");
        assert!(script.get_last_error_location().starts_with("unit "));
    }

    #[test]
    fn a_script_that_does_not_compile_reports_the_failure() {
        let host = PascalScriptHost {
            scripts_folder: ".".to_owned(),
        };
        let error = match host.create_script_internal("Broken", "unit Broken; begin") {
            Ok(_) => panic!("a broken script compiled"),
            Err(error) => error,
        };
        assert!(error.0.contains("Broken"));
    }
}
