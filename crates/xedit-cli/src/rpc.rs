// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON-RPC 2.0 over lines of text: one message per line, in both
//! directions, as the MCP stdio transport frames it. `xedit serve` and
//! `xedit mcp` share this loop and differ in their `Handler` only.

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};
use xedit_session::CommandError;

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
/// The code of every command failure that has no standard code.
pub const COMMAND_FAILED: i64 = -32000;

/// A JSON-RPC error object.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    fn to_json(&self) -> Value {
        match &self.data {
            Some(data) => json!({ "code": self.code, "message": self.message, "data": data }),
            None => json!({ "code": self.code, "message": self.message }),
        }
    }
}

/// A command failure as an error object: the message of the command, and
/// its stable code (`edit_required`, `unknown_record`, ...) in `data.code`.
impl From<&CommandError> for RpcError {
    fn from(error: &CommandError) -> Self {
        let code = match error.code.as_str() {
            "unknown_command" => METHOD_NOT_FOUND,
            "invalid_params" => INVALID_PARAMS,
            "internal" => INTERNAL_ERROR,
            _ => COMMAND_FAILED,
        };
        Self {
            code,
            message: error.message.clone(),
            data: Some(json!({ "code": error.code })),
        }
    }
}

/// What answers the methods of a connection.
pub trait Handler {
    /// Runs a request. `params` is always an object.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, RpcError>;

    /// A notification, which has no response.
    fn notify(&mut self, _method: &str, _params: Value) {}

    /// Whether the handler wants the connection closed after the current
    /// message.
    fn stopped(&self) -> bool {
        false
    }
}

fn response(id: Value, outcome: Result<Value, RpcError>) -> Value {
    match outcome {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error.to_json() }),
    }
}

/// Handles one message object. A notification (no `id`) gives no response.
fn handle_message(message: Value, handler: &mut impl Handler) -> Option<Value> {
    let Value::Object(mut object) = message else {
        return Some(response(
            Value::Null,
            Err(RpcError::new(INVALID_REQUEST, "a request must be a JSON object")),
        ));
    };
    let id = object.remove("id");
    let has_id = id.is_some();
    let id = id.unwrap_or(Value::Null);
    let invalid = |message: &str| Some(response(id.clone(), Err(RpcError::new(INVALID_REQUEST, message))));
    if object.get("jsonrpc") != Some(&json!("2.0")) {
        return invalid("\"jsonrpc\" must be \"2.0\"");
    }
    let Some(Value::String(method)) = object.remove("method") else {
        // A response from the client (no method) needs no answer.
        return if object.contains_key("result") || object.contains_key("error") {
            None
        } else {
            invalid("a request needs a \"method\" string")
        };
    };
    if !matches!(id, Value::Null | Value::String(_) | Value::Number(_)) {
        return invalid("\"id\" must be a string, a number or null");
    }
    let params = match object.remove("params") {
        None | Some(Value::Null) => json!({}),
        Some(params @ Value::Object(_)) => params,
        Some(_) => {
            return has_id.then(|| response(id, Err(RpcError::new(INVALID_PARAMS, "\"params\" must be an object"))));
        }
    };
    if has_id {
        let outcome = handler.call(&method, params);
        Some(response(id, outcome))
    } else {
        handler.notify(&method, params);
        None
    }
}

/// Handles one line: a request, or a batch (array) of requests.
fn handle_line(line: &[u8], handler: &mut impl Handler) -> Option<Value> {
    let message: Value = match serde_json::from_slice(line) {
        Ok(message) => message,
        Err(error) => {
            return Some(response(
                Value::Null,
                Err(RpcError::new(PARSE_ERROR, format!("invalid JSON: {error}"))),
            ));
        }
    };
    match message {
        Value::Array(items) if items.is_empty() => Some(response(
            Value::Null,
            Err(RpcError::new(INVALID_REQUEST, "an empty batch")),
        )),
        Value::Array(items) => {
            let mut responses = Vec::new();
            for item in items {
                responses.extend(handle_message(item, handler));
                if handler.stopped() {
                    break;
                }
            }
            (!responses.is_empty()).then_some(Value::Array(responses))
        }
        message => handle_message(message, handler),
    }
}

/// Reads requests line by line from `input` and writes one response line
/// for each to `output`, until the input ends or the handler stops.
pub fn serve_lines(mut input: impl BufRead, mut output: impl Write, handler: &mut impl Handler) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if let Some(reply) = handle_line(&line, handler) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
        if handler.stopped() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo {
        stop: bool,
        notified: Vec<String>,
    }

    impl Handler for Echo {
        fn call(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
            match method {
                "fail" => Err(RpcError::new(COMMAND_FAILED, "failed")),
                "stop" => {
                    self.stop = true;
                    Ok(Value::Null)
                }
                _ => Ok(json!({ "method": method, "params": params })),
            }
        }
        fn notify(&mut self, method: &str, _: Value) {
            self.notified.push(method.to_owned());
        }
        fn stopped(&self) -> bool {
            self.stop
        }
    }

    fn run(input: &str) -> (Vec<Value>, Echo) {
        let mut echo = Echo {
            stop: false,
            notified: Vec::new(),
        };
        let mut output = Vec::new();
        serve_lines(input.as_bytes(), &mut output, &mut echo).unwrap();
        let replies = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        (replies, echo)
    }

    #[test]
    fn requests_get_one_response_each() {
        let (replies, _) = run(concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"a","params":{"x":1}}"#,
            "\n\n",
            r#"{"jsonrpc":"2.0","id":"two","method":"fail"}"#,
            "\n",
        ));
        assert_eq!(replies[0]["id"], 1);
        assert_eq!(replies[0]["result"]["params"]["x"], 1);
        assert_eq!(replies[1]["id"], "two");
        assert_eq!(replies[1]["error"]["message"], "failed");
    }

    #[test]
    fn notifications_get_no_response() {
        let (replies, echo) = run("{\"jsonrpc\":\"2.0\",\"method\":\"note\"}\n");
        assert!(replies.is_empty());
        assert_eq!(echo.notified, ["note"]);
    }

    #[test]
    fn malformed_input_gets_standard_errors() {
        let (replies, _) = run(concat!(
            "not json\n",
            "[]\n",
            "3\n",
            r#"{"id":4,"method":"a"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":5,"method":"a","params":[1]}"#,
            "\n",
        ));
        let codes: Vec<i64> = replies
            .iter()
            .map(|reply| reply["error"]["code"].as_i64().unwrap())
            .collect();
        assert_eq!(
            codes,
            [
                PARSE_ERROR,
                INVALID_REQUEST,
                INVALID_REQUEST,
                INVALID_REQUEST,
                INVALID_PARAMS
            ]
        );
        assert_eq!(replies[3]["id"], 4);
    }

    #[test]
    fn batches_answer_as_an_array_and_skip_notifications() {
        let (replies, _) = run(concat!(
            r#"[{"jsonrpc":"2.0","id":1,"method":"a"},{"jsonrpc":"2.0","method":"n"},{"jsonrpc":"2.0","id":2,"method":"b"}]"#,
            "\n",
        ));
        let batch = replies[0].as_array().unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[1]["result"]["method"], "b");
    }

    #[test]
    fn a_stopping_handler_ends_the_connection() {
        let (replies, _) = run(concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"stop"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"a"}"#,
            "\n",
        ));
        assert_eq!(replies.len(), 1);
    }

    #[test]
    fn command_errors_keep_their_code_and_message() {
        let error = RpcError::from(&CommandError::new("edit_required", "pass --edit"));
        assert_eq!(error.code, COMMAND_FAILED);
        assert_eq!(error.message, "pass --edit");
        assert_eq!(error.data, Some(json!({ "code": "edit_required" })));
        assert_eq!(
            RpcError::from(&CommandError::new("unknown_command", "x")).code,
            METHOD_NOT_FOUND
        );
    }
}
