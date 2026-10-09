// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Session state and the command registry.
//!
//! Every xEdit operation is a command registered here. The CLI, the daemon,
//! the MCP server and the GUI call commands and nothing else, so each of them
//! covers the same set of operations.

pub mod archive;
pub mod assets;
pub mod batch;
pub mod clean;
pub mod commands;
pub mod conflicts;
pub mod dump;
pub mod edit;
pub mod formids;
pub mod lodgen;
pub mod masters;
pub mod refs;
pub mod save;
pub mod sniff;

use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, schema_for};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use batch::{BatchCommand, batch_from_value, parse_batch};

/// State shared by all commands of one run: the game and the plugins
/// loaded with `Session::load`.
#[derive(Default)]
pub struct Session {
    game: Option<xedit_core::interface::globals::GameMode>,
    files: Vec<std::sync::Arc<xedit_core::implementation::FileImpl>>,
    /// Whether commands that change plugin data or files may run for real
    /// (the `--edit` flag of the CLI). Without it a mutating command runs
    /// only as a dry run.
    edit_allowed: bool,
}

impl Session {
    /// A session over files that are loaded already, in load order.
    pub fn with_files(
        game: xedit_core::interface::globals::GameMode,
        files: Vec<std::sync::Arc<xedit_core::implementation::FileImpl>>,
    ) -> Self {
        Self {
            game: Some(game),
            files,
            edit_allowed: false,
        }
    }

    /// Allows the mutating commands to change files and plugin data.
    pub fn allow_edit(&mut self, allowed: bool) {
        self.edit_allowed = allowed;
        xedit_core::interface::globals::set_edit_allowed(allowed);
    }

    pub fn edit_allowed(&self) -> bool {
        self.edit_allowed
    }
}

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

/// A command as listed by `Registry::commands`.
pub struct CommandInfo<'a> {
    pub name: &'static str,
    pub summary: &'static str,
    /// Whether the command changes plugin data or files.
    pub mutates: bool,
    /// JSON Schema of the parameters.
    pub request: &'a Schema,
    /// JSON Schema of the result.
    pub response: &'a Schema,
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
        commands::register(&mut registry);
        conflicts::register(&mut registry);
        edit::register(&mut registry);
        save::register(&mut registry);
        masters::register(&mut registry);
        formids::register(&mut registry);
        refs::register(&mut registry);
        clean::register(&mut registry);
        archive::register(&mut registry);
        assets::register(&mut registry);
        sniff::register(&mut registry);
        lodgen::register(&mut registry);
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

    /// Runs the command `name` with JSON `params`. A mutating command needs
    /// the edit flag of the session unless its `dry_run` parameter is set.
    pub fn call(&self, session: &mut Session, name: &str, params: Value) -> Result<Value, CommandError> {
        let command = self
            .commands
            .get(name)
            .ok_or_else(|| CommandError::new("unknown_command", format!("no command named {name}")))?;
        if command.mutates && !session.edit_allowed {
            let dry_run = params.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
            if !dry_run {
                return Err(CommandError::new(
                    "edit_required",
                    format!("{name} changes plugin data or files: pass --edit, or dry_run for a report only"),
                ));
            }
        }
        (command.handler)(session, params)
    }

    /// Whether the command `name` changes plugin data or files.
    pub fn mutates(&self, name: &str) -> Option<bool> {
        self.commands.get(name).map(|command| command.mutates)
    }

    /// Every command in name order, for front ends that generate their own
    /// surface from the registry (the daemon, the MCP server).
    pub fn commands(&self) -> impl Iterator<Item = CommandInfo<'_>> {
        self.commands.iter().map(|(name, command)| CommandInfo {
            name,
            summary: command.summary,
            mutates: command.mutates,
            request: &command.request,
            response: &command.response,
        })
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
        let commands = catalogue["commands"].as_array().unwrap();
        let command = commands
            .iter()
            .find(|command| command["name"] == "system.version")
            .unwrap();
        assert_eq!(command["mutates"], false);
        assert!(command["response"]["properties"]["version"].is_object());
    }
}
