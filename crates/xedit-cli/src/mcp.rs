// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `xedit mcp`: the commands of the registry as Model Context Protocol
//! tools over stdio.
//!
//! The protocol is the small part of MCP a tool server needs (`initialize`,
//! `ping`, `tools/list`, `tools/call`), written on the line-framed JSON-RPC
//! loop of `rpc`. The maintained Rust SDK (`rmcp`) brings tokio and a macro
//! layer for the same four methods, and its tool schemas come from its own
//! derive macros, not from the schemas of the registry.
//!
//! A tool is a command: the name is the command name with `.` replaced by
//! `_` (tool names allow letters, digits, `_` and `-` only), the
//! `inputSchema` is the request schema of the command and the
//! `outputSchema` its response schema. The list is read from the registry
//! at each `tools/list`, so a command added to the registry is a tool with
//! no change here.

use serde_json::{Value, json};
use xedit_session::{CommandInfo, Registry};

use crate::engine::Engine;
use crate::rpc::{Handler, INVALID_PARAMS, METHOD_NOT_FOUND, RpcError};

/// The protocol versions this server speaks, newest last.
const PROTOCOL_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

/// The tool name of a command.
pub fn tool_name(command: &str) -> String {
    command.replace('.', "_")
}

pub struct McpHandler {
    engine: Engine,
}

impl McpHandler {
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }

    fn instructions(&self) -> String {
        let edit = if self.engine.edit_allowed() {
            "This server was started with --edit: tools that change plugin data run for real."
        } else {
            "This server was started without --edit: tools that change plugin data only run with dry_run set to true."
        };
        format!(
            "xEdit session over the plugins given at startup. {edit} Changes stay in memory until files_save writes \
             the plugin. Record and element tools take load order FormIDs in hexadecimal; list with records_find or \
             records_list, then read with records_get."
        )
    }
}

fn tool_of(command: &CommandInfo) -> Value {
    let mut description = command.summary.to_owned();
    if command.mutates {
        description.push_str(
            " Changes plugin data or files: needs the server started with --edit, or dry_run set to true for a report \
             only.",
        );
    }
    let mut tool = json!({
        "name": tool_name(command.name),
        "title": command.name,
        "description": description,
        "inputSchema": command.request,
        "annotations": {
            "readOnlyHint": !command.mutates,
            "destructiveHint": command.mutates,
            "idempotentHint": !command.mutates,
            "openWorldHint": false,
        },
    });
    // structuredContent has to be an object, and so must its schema be.
    if command.response.get("type") == Some(&json!("object")) {
        tool["outputSchema"] = json!(command.response);
    }
    tool
}

/// The command a tool name stands for.
fn command_of(registry: &Registry, tool: &str) -> Option<&'static str> {
    registry
        .commands()
        .find(|command| tool_name(command.name) == tool)
        .map(|command| command.name)
}

impl Handler for McpHandler {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or_default();
                let version = PROTOCOL_VERSIONS
                    .iter()
                    .find(|version| **version == asked)
                    .unwrap_or(&PROTOCOL_VERSIONS[PROTOCOL_VERSIONS.len() - 1]);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "xedit", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": self.instructions(),
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => {
                let tools: Vec<Value> = self
                    .engine
                    .registry()
                    .commands()
                    .map(|command| tool_of(&command))
                    .collect();
                Ok(json!({ "tools": tools }))
            }
            "tools/call" => {
                let tool = params["name"]
                    .as_str()
                    .ok_or_else(|| RpcError::new(INVALID_PARAMS, "tools/call needs a tool \"name\""))?;
                let arguments = match &params["arguments"] {
                    Value::Null => json!({}),
                    arguments @ Value::Object(_) => arguments.clone(),
                    _ => return Err(RpcError::new(INVALID_PARAMS, "\"arguments\" must be an object")),
                };
                let command = command_of(self.engine.registry(), tool)
                    .ok_or_else(|| RpcError::new(INVALID_PARAMS, format!("unknown tool {tool}")))?;
                // A failed command is a tool result with isError, so the
                // model sees the message; only protocol faults are errors.
                Ok(match self.engine.call(command, arguments) {
                    Ok(result) => {
                        let mut reply = json!({
                            "content": [{ "type": "text", "text": result.to_string() }],
                            "isError": false,
                        });
                        if result.is_object() {
                            reply["structuredContent"] = result;
                        }
                        reply
                    }
                    Err(error) => json!({
                        "content": [{ "type": "text", "text": format!("{}: {}", error.code, error.message) }],
                        "isError": true,
                    }),
                })
            }
            method => Err(RpcError::new(METHOD_NOT_FOUND, format!("no method named {method}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use xedit_session::Session;

    use super::*;

    fn handler() -> McpHandler {
        McpHandler::new(Engine::with_session(Session::default()))
    }

    fn list(handler: &mut McpHandler) -> Vec<Value> {
        handler.call("tools/list", json!({})).unwrap()["tools"]
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn initialize_negotiates_the_protocol_version() {
        let mut handler = handler();
        let reply = handler
            .call("initialize", json!({ "protocolVersion": "2025-03-26" }))
            .unwrap();
        assert_eq!(reply["protocolVersion"], "2025-03-26");
        assert!(reply["capabilities"]["tools"].is_object());
        assert!(reply["instructions"].as_str().unwrap().contains("without --edit"));
        let reply = handler
            .call("initialize", json!({ "protocolVersion": "2099-01-01" }))
            .unwrap();
        assert_eq!(reply["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn every_registered_command_is_a_tool() {
        let mut handler = handler();
        let tools = list(&mut handler);
        let listed: BTreeSet<&str> = tools.iter().map(|tool| tool["name"].as_str().unwrap()).collect();
        let registered: BTreeSet<String> = handler
            .engine
            .registry()
            .commands()
            .map(|command| tool_name(command.name))
            .collect();
        assert_eq!(listed.len(), tools.len(), "two commands map to one tool name");
        assert_eq!(listed, registered.iter().map(String::as_str).collect::<BTreeSet<_>>());
        for tool in &tools {
            let name = tool["name"].as_str().unwrap();
            assert!(
                name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && name.len() <= 64,
                "{name} is not a valid tool name"
            );
            assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
            assert!(
                tool["description"].as_str().is_some_and(|text| !text.is_empty()),
                "{name}"
            );
        }
    }

    #[test]
    fn every_tool_is_callable() {
        let mut handler = handler();
        for tool in list(&mut handler) {
            let name = tool["name"].as_str().unwrap();
            // A failing command is a result with isError, never a protocol error.
            let reply = handler
                .call("tools/call", json!({ "name": name, "arguments": {} }))
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert!(reply["content"][0]["text"].is_string(), "{name}");
        }
    }

    #[test]
    fn mutating_tools_keep_the_edit_gate() {
        let mut handler = handler();
        let tools = list(&mut handler);
        let save = tools.iter().find(|tool| tool["name"] == "files_save").unwrap();
        assert_eq!(save["annotations"]["readOnlyHint"], false);
        let reply = handler
            .call("tools/call", json!({ "name": "files_save", "arguments": {} }))
            .unwrap();
        assert_eq!(reply["isError"], true);
        assert!(
            reply["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("edit_required: ")
        );
        let reply = handler
            .call(
                "tools/call",
                json!({ "name": "files_save", "arguments": { "dry_run": true } }),
            )
            .unwrap();
        assert!(
            !reply["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("edit_required")
        );
    }

    #[test]
    fn a_result_has_text_and_structured_content() {
        let mut handler = handler();
        let reply = handler
            .call("tools/call", json!({ "name": "system_version", "arguments": {} }))
            .unwrap();
        assert_eq!(reply["isError"], false);
        assert_eq!(reply["structuredContent"]["version"], env!("CARGO_PKG_VERSION"));
        let text: Value = serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, reply["structuredContent"]);
    }

    #[test]
    fn protocol_faults_are_errors() {
        let mut handler = handler();
        assert_eq!(
            handler
                .call("tools/call", json!({ "name": "no_such" }))
                .unwrap_err()
                .code,
            INVALID_PARAMS
        );
        assert_eq!(handler.call("tools/call", json!({})).unwrap_err().code, INVALID_PARAMS);
        assert_eq!(
            handler.call("resources/list", json!({})).unwrap_err().code,
            METHOD_NOT_FOUND
        );
    }
}
