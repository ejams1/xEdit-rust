// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `xedit serve`: the commands of the registry as JSON-RPC methods, over
//! stdio or a Windows named pipe, against one loaded session.
//!
//! Every registered command is a method of its own name (`records.get`),
//! with the command's parameters as `params`. The `rpc.` methods are the
//! daemon's own:
//!
//! - `rpc.discover`: the command catalogue (the output of `xedit schema`)
//!   and whether the session may edit.
//! - `rpc.batch`: `{"commands": [{"command", "params"}], "keep_going"}`,
//!   the path of `xedit batch`.
//! - `rpc.shutdown`: ends the daemon.

use serde_json::{Value, json};
use xedit_session::batch_from_value;

use crate::engine::Engine;
use crate::rpc::{Handler, METHOD_NOT_FOUND, RpcError};

pub struct ServeHandler {
    engine: Engine,
    shutdown: bool,
}

impl ServeHandler {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            shutdown: false,
        }
    }

    /// Whether `rpc.shutdown` was called, which ends the daemon and not
    /// just the connection.
    pub fn shutdown_requested(&self) -> bool {
        self.shutdown
    }
}

impl Handler for ServeHandler {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        match method {
            "rpc.discover" => {
                let mut catalogue = self.engine.registry().catalogue();
                catalogue["edit_allowed"] = json!(self.engine.edit_allowed());
                Ok(catalogue)
            }
            "rpc.batch" => {
                let commands = params.get("commands").cloned().unwrap_or(Value::Null);
                let commands = batch_from_value(commands).map_err(|e| RpcError::from(&e))?;
                let keep_going = params.get("keep_going").and_then(Value::as_bool).unwrap_or(false);
                self.engine.batch(commands, keep_going).map_err(|e| RpcError::from(&e))
            }
            "rpc.shutdown" => {
                self.shutdown = true;
                Ok(json!({}))
            }
            method if method.starts_with("rpc.") => {
                Err(RpcError::new(METHOD_NOT_FOUND, format!("no method named {method}")))
            }
            command => self.engine.call(command, params).map_err(|e| RpcError::from(&e)),
        }
    }

    fn stopped(&self) -> bool {
        self.shutdown
    }
}

#[cfg(test)]
mod tests {
    use xedit_session::Session;

    use super::*;

    fn handler() -> ServeHandler {
        ServeHandler::new(Engine::with_session(Session::default()))
    }

    /// The `data.code` of a command failure.
    fn failure_code(error: &RpcError) -> Option<&str> {
        error.data.as_ref().and_then(|data| data["code"].as_str())
    }

    #[test]
    fn every_registered_command_is_a_method() {
        let mut handler = handler();
        let names: Vec<&str> = handler
            .engine
            .registry()
            .commands()
            .map(|command| command.name)
            .collect();
        assert!(names.len() > 5, "the registry looks empty: {names:?}");
        for name in names {
            // The session has no game, so most commands fail, but none of
            // them may be unknown.
            if let Err(error) = handler.call(name, json!({})) {
                assert_ne!(failure_code(&error), Some("unknown_command"), "{name}");
                assert_ne!(error.code, METHOD_NOT_FOUND, "{name}");
            }
        }
    }

    #[test]
    fn discover_lists_the_registry() {
        let mut handler = handler();
        let catalogue = handler.call("rpc.discover", json!({})).unwrap();
        let listed: Vec<&str> = catalogue["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|command| command["name"].as_str().unwrap())
            .collect();
        let registered: Vec<&str> = handler
            .engine
            .registry()
            .commands()
            .map(|command| command.name)
            .collect();
        assert_eq!(listed, registered);
        assert_eq!(catalogue["edit_allowed"], false);
    }

    #[test]
    fn unknown_methods_are_method_not_found() {
        let mut handler = handler();
        for method in ["no.such", "rpc.nothing"] {
            assert_eq!(handler.call(method, json!({})).unwrap_err().code, METHOD_NOT_FOUND);
        }
    }

    #[test]
    fn mutating_commands_keep_the_edit_gate_and_dry_run() {
        let mut handler = handler();
        let mutating: Vec<&str> = handler
            .engine
            .registry()
            .commands()
            .filter(|command| command.mutates)
            .map(|command| command.name)
            .collect();
        assert!(mutating.contains(&"files.save"));
        for name in mutating {
            let error = handler.call(name, json!({})).unwrap_err();
            assert_eq!(failure_code(&error), Some("edit_required"), "{name}");
            assert!(error.message.contains(name), "{name}: {}", error.message);
            // With dry_run the gate opens, and the command runs as far as
            // the empty session allows.
            let error = handler.call(name, json!({ "dry_run": true })).unwrap_err();
            assert_ne!(failure_code(&error), Some("edit_required"), "{name}");
        }
    }

    #[test]
    fn batch_runs_in_the_one_session() {
        let mut handler = handler();
        let reply = handler
            .call(
                "rpc.batch",
                json!({ "commands": [
                    { "command": "system.version" },
                    { "command": "no.such" },
                    { "command": "system.version" },
                ], "keep_going": true }),
            )
            .unwrap();
        assert_eq!(reply.as_array().unwrap().len(), 3);
        assert_eq!(reply[1]["error"]["code"], "unknown_command");
        let error = handler.call("rpc.batch", json!({})).unwrap_err();
        assert_eq!(error.code, crate::rpc::INVALID_PARAMS);
    }

    #[test]
    fn shutdown_stops_the_daemon() {
        let mut handler = handler();
        assert!(!handler.stopped());
        handler.call("rpc.shutdown", json!({})).unwrap();
        assert!(handler.stopped() && handler.shutdown_requested());
    }
}
