// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Session state and the command registry.
//!
//! Every xEdit operation is a command registered here. The CLI, the daemon,
//! the MCP server and the GUI call commands and nothing else, so each of them
//! covers the same set of operations.

use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, schema_for};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// State shared by all commands of one run.
#[derive(Default)]
pub struct Session {}

/// A command failure. `code` is stable and safe to match on.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CommandError {}

type Handler = Box<dyn Fn(&mut Session, Value) -> Result<Value, CommandError> + Send + Sync>;

struct Command {
    summary: &'static str,
    mutates: bool,
    request: Schema,
    response: Schema,
    handler: Handler,
}

/// The set of commands a session accepts.
#[derive(Default)]
pub struct Registry {
    commands: BTreeMap<&'static str, Command>,
}

impl Registry {
    /// The registry with every built-in command.
    pub fn standard() -> Self {
        let mut registry = Self::default();
        registry.register("system.version", "Report the xEdit version.", false, version);
        registry
    }

    /// Adds a command. `mutates` marks commands that change plugin data or files.
    pub fn register<Req, Res>(
        &mut self,
        name: &'static str,
        summary: &'static str,
        mutates: bool,
        run: fn(&mut Session, Req) -> Result<Res, CommandError>,
    ) where
        Req: DeserializeOwned + JsonSchema + 'static,
        Res: Serialize + JsonSchema + 'static,
    {
        let handler: Handler = Box::new(move |session, params| {
            let request =
                serde_json::from_value(params).map_err(|e| CommandError::new("invalid_params", e.to_string()))?;
            let response = run(session, request)?;
            serde_json::to_value(response).map_err(|e| CommandError::new("internal", e.to_string()))
        });
        let command = Command {
            summary,
            mutates,
            request: schema_for!(Req),
            response: schema_for!(Res),
            handler,
        };
        let previous = self.commands.insert(name, command);
        assert!(previous.is_none(), "command {name} registered twice");
    }

    /// Runs the command `name` with JSON `params`.
    pub fn call(&self, session: &mut Session, name: &str, params: Value) -> Result<Value, CommandError> {
        let command = self
            .commands
            .get(name)
            .ok_or_else(|| CommandError::new("unknown_command", format!("no command named {name}")))?;
        (command.handler)(session, params)
    }

    /// Describes every command with its request and response JSON Schema.
    pub fn catalogue(&self) -> Value {
        let commands: Vec<Value> = self
            .commands
            .iter()
            .map(|(name, command)| {
                json!({
                    "name": name,
                    "summary": command.summary,
                    "mutates": command.mutates,
                    "request": command.request,
                    "response": command.response,
                })
            })
            .collect();
        json!({ "version": env!("CARGO_PKG_VERSION"), "commands": commands })
    }
}

/// A request without parameters.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

#[derive(Serialize, JsonSchema)]
pub struct Version {
    /// Version of xEdit-rust.
    pub version: String,
}

fn version(_: &mut Session, _: NoParams) -> Result<Version, CommandError> {
    Ok(Version {
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_command_reports_package_version() {
        let result = Registry::standard().call(&mut Session::default(), "system.version", json!({}));
        assert_eq!(result, Ok(json!({ "version": env!("CARGO_PKG_VERSION") })));
    }

    #[test]
    fn unknown_command_has_stable_code() {
        let error = Registry::standard()
            .call(&mut Session::default(), "no.such", json!({}))
            .unwrap_err();
        assert_eq!(error.code, "unknown_command");
    }

    #[test]
    fn unexpected_params_are_rejected() {
        let error = Registry::standard()
            .call(&mut Session::default(), "system.version", json!({ "extra": 1 }))
            .unwrap_err();
        assert_eq!(error.code, "invalid_params");
    }

    #[test]
    fn catalogue_lists_schemas() {
        let catalogue = Registry::standard().catalogue();
        let command = &catalogue["commands"][0];
        assert_eq!(command["name"], "system.version");
        assert_eq!(command["mutates"], false);
        assert!(command["response"]["properties"]["version"].is_object());
    }
}
