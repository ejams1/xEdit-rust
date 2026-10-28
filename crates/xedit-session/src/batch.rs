// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Several commands run against one loaded session. The `xedit batch`
//! subcommand, the `rpc.batch` method of `xedit serve` and any later front
//! end all go through `Registry::run_batch`.

use serde_json::{Value, json};

use crate::{CommandError, Registry, Session};

/// One command of a batch: `{"command": name, "params": {...}}`.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchCommand {
    pub command: String,
    pub params: Value,
}

/// The commands of a batch, from its JSON text.
pub fn parse_batch(text: &str) -> Result<Vec<BatchCommand>, CommandError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| CommandError::new("invalid_params", format!("the batch is not valid JSON: {e}")))?;
    batch_from_value(value)
}

/// The commands of a batch, from its JSON array.
pub fn batch_from_value(value: Value) -> Result<Vec<BatchCommand>, CommandError> {
    let invalid = |message: String| CommandError::new("invalid_params", message);
    let Value::Array(items) = value else {
        return Err(invalid("the batch must be a JSON array of commands".to_owned()));
    };
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let command = item["command"]
                .as_str()
                .ok_or_else(|| invalid(format!("command {index} has no \"command\" name")))?
                .to_owned();
            let params = match &item["params"] {
                Value::Null => json!({}),
                params @ Value::Object(_) => params.clone(),
                _ => return Err(invalid(format!("command {index}: \"params\" must be an object"))),
            };
            Ok(BatchCommand { command, params })
        })
        .collect()
}

impl Registry {
    /// Runs the commands in order against the one session, so an edit is
    /// visible to the commands after it and a save at the end writes it.
    /// Returns one envelope per command: `{"ok": true, "command",
    /// "result"}` or `{"ok": false, "command", "error"}`. A command that
    /// fails stops the batch unless `keep_going`, and the commands after it
    /// are reported as not run (`"skipped": true`), so the results always
    /// line up with the commands. A command name the registry does not know
    /// changed nothing, so it fails alone and the batch goes on.
    pub fn run_batch(&self, session: &mut Session, commands: Vec<BatchCommand>, keep_going: bool) -> Value {
        let mut results = Vec::with_capacity(commands.len());
        let mut stopped_by: Option<String> = None;
        for command in commands {
            if let Some(failed) = &stopped_by {
                let error = CommandError::new(
                    "skipped",
                    format!("not run: the batch stopped at the failure of {failed}"),
                );
                results.push(json!({ "ok": false, "command": command.command, "skipped": true, "error": error }));
                continue;
            }
            match self.call(session, &command.command, command.params) {
                Ok(result) => results.push(json!({ "ok": true, "command": command.command, "result": result })),
                Err(error) => {
                    if !keep_going && error.code != "unknown_command" {
                        stopped_by = Some(command.command.clone());
                    }
                    results.push(json!({ "ok": false, "command": command.command, "error": error }));
                }
            }
        }
        Value::Array(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_stops_at_the_first_failure_unless_told_to_go_on() {
        let registry = Registry::standard();
        // A command that fails (an unloaded session) stops the batch; the
        // commands after it are reported as skipped.
        let commands = parse_batch(
            r#"[{"command":"system.version"},{"command":"files.list"},{"command":"system.version","params":{}}]"#,
        )
        .unwrap();
        let stopped = registry.run_batch(&mut Session::default(), commands.clone(), false);
        assert_eq!(stopped.as_array().unwrap().len(), 3);
        assert_eq!(stopped[1]["ok"], false);
        assert_eq!(stopped[2]["ok"], false);
        assert_eq!(stopped[2]["skipped"], true);
        assert_eq!(stopped[2]["error"]["code"], "skipped");
        let all = registry.run_batch(&mut Session::default(), commands, true);
        assert_eq!(all.as_array().unwrap().len(), 3);
        assert_eq!(all[2]["ok"], true);
    }

    #[test]
    fn an_unknown_command_fails_alone() {
        let registry = Registry::standard();
        let commands =
            parse_batch(r#"[{"command":"compare"},{"command":"system.version"},{"command":"system.version"}]"#)
                .unwrap();
        let results = registry.run_batch(&mut Session::default(), commands, false);
        assert_eq!(results.as_array().unwrap().len(), 3);
        assert_eq!(results[0]["ok"], false);
        assert_eq!(results[0]["error"]["code"], "unknown_command");
        assert_eq!(results[1]["ok"], true);
        assert_eq!(results[2]["ok"], true);
    }

    #[test]
    fn malformed_batches_are_invalid_params() {
        for text in ["{}", "[{}]", r#"[{"command":"a","params":3}]"#, "nope"] {
            assert_eq!(parse_batch(text).unwrap_err().code, "invalid_params", "{text}");
        }
    }
}
