// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `xedit serve` and `xedit mcp` as processes: the transports, the framing
//! and the shutdown. The commands themselves are covered by the unit tests
//! of the handlers.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Client {
    fn start(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_xedit"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn xedit");
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self { child, stdin, stdout }
    }

    fn send(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn receive(&mut self) -> Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line:?}"))
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let reply = self.receive();
        assert_eq!(reply["id"], id);
        reply
    }

    /// Closes the input and waits for the process to end.
    fn finish(mut self) {
        drop(self.stdin.take());
        assert!(self.child.wait().unwrap().success());
    }
}

#[test]
fn serve_answers_commands_over_stdio_until_the_input_ends() {
    let mut client = Client::start(&["serve"]);
    let reply = client.request(1, "system.version", json!({}));
    assert_eq!(reply["result"]["version"], env!("CARGO_PKG_VERSION"));
    let reply = client.request(2, "no.such", json!({}));
    assert_eq!(reply["error"]["code"], -32601);
    let reply = client.request(3, "files.save", json!({}));
    assert_eq!(reply["error"]["data"]["code"], "edit_required");
    let reply = client.request(4, "rpc.discover", json!({}));
    assert!(reply["result"]["commands"].as_array().unwrap().len() > 5);
    client.finish();
}

#[test]
fn serve_ends_on_shutdown() {
    let mut client = Client::start(&["serve"]);
    let reply = client.request(1, "rpc.shutdown", json!({}));
    assert!(reply["result"].is_object());
    assert!(client.child.wait().unwrap().success());
}

#[test]
fn serve_fails_at_startup_when_the_plugins_do_not_load() {
    let output = Command::new(env!("CARGO_BIN_EXE_xedit"))
        .args(["serve", "--game", "sse", "--load", "no-such-plugin.esp"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn mcp_speaks_the_tool_protocol_over_stdio() {
    let mut client = Client::start(&["mcp"]);
    let reply = client.request(
        1,
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }),
    );
    assert_eq!(reply["result"]["serverInfo"]["name"], "xedit");
    client.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let reply = client.request(2, "tools/list", json!({}));
    let tools = reply["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|tool| tool["name"] == "records_get"));
    let reply = client.request(3, "tools/call", json!({ "name": "system_version", "arguments": {} }));
    assert_eq!(reply["result"]["isError"], false);
    client.finish();
}

#[cfg(windows)]
#[test]
fn serve_answers_on_a_named_pipe_and_accepts_the_next_client() {
    use std::fs::OpenOptions;
    use std::time::{Duration, Instant};

    let name = format!("xedit-test-{}", std::process::id());
    let mut child = Command::new(env!("CARGO_BIN_EXE_xedit"))
        .args(["serve", "--pipe", &name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let path = format!(r"\\.\pipe\{name}");
    let connect = || {
        let started = Instant::now();
        loop {
            match OpenOptions::new().read(true).write(true).open(&path) {
                Ok(pipe) => return pipe,
                Err(error) if started.elapsed() < Duration::from_secs(60) => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => panic!("the pipe never opened: {error}"),
            }
        }
    };
    let ask = |pipe: &mut std::fs::File, id: u64, method: &str| -> Value {
        writeln!(pipe, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": method })).unwrap();
        let mut line = String::new();
        BufReader::new(&*pipe).read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let mut first = connect();
    assert_eq!(
        ask(&mut first, 1, "system.version")["result"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    drop(first);
    // The daemon outlives its first client.
    let mut second = connect();
    assert_eq!(
        ask(&mut second, 2, "files.save")["error"]["data"]["code"],
        "edit_required"
    );
    assert!(ask(&mut second, 3, "rpc.shutdown")["result"].is_object());
    assert!(child.wait().unwrap().success());
}
