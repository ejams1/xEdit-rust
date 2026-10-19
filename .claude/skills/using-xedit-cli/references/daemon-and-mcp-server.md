## Daemon and MCP server

`serve` and `mcp` load the session once and run the same registry commands against it, so an edit stays in memory between calls and loading is paid once.

Use them for a long session: several edits with reads in between, an agent exploring a load order, a save after a check.

Both take the global `--game`, `--load`, `--edit` and `--threads` options at startup; the edit gate is fixed then and no request can lift it.

Both build their method or tool list from the registry when they run, so every command, including ones added later, is there without any change; `rpc.discover` (serve) and `tools/list` (mcp) show what this build has.

```
xedit --game fo4 --load "<Data>\DLCRobot.esm" --edit serve
xedit --game fo4 --load "<Data>\DLCRobot.esm" --edit serve --pipe NAME
xedit --game fo4 --load "<Data>\DLCRobot.esm" --edit mcp
```

- `serve` reads one JSON-RPC 2.0 request per line from stdin and writes one response line to stdout (`--pipe NAME` listens on the Windows named pipe `\\.\pipe\NAME` instead, one client at a time, and takes the next client when one disconnects; only the user that started it can open it). Progress messages go to stderr, never to stdout. The input ending closes a stdio daemon.
- **Concurrent clients and daemons.** One client at a time: while a client is connected the daemon holds the one instance of the pipe, so a second client's connect fails with `ERROR_PIPE_BUSY` (231) until the first disconnects (retry it, or wait with `WaitNamedPipe`); nothing of a client that never got the pipe reaches the session.

One daemon per name: the instance is created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a second `xedit serve --pipe NAME` fails at startup (Windows reports `ERROR_PIPE_BUSY` while the instance waits for a client and `ERROR_ACCESS_DENIED` while one is connected; both are reported as `NAME is already served by another xedit serve`) instead of running a second session over the same plugins.

Two daemons on two names are two processes whose edits reach disk only at their own `files.save`.
- Each registry command is a method of its own name, with the request fields of `xedit schema` as `params` (an object): `{"jsonrpc":"2.0","id":1,"method":"records.find","params":{"editor_id":"Dawnguard"}}`. A JSON array of requests is a JSON-RPC batch.
- The daemon's own methods: `rpc.discover` (the `xedit schema` catalogue plus `edit_allowed`), `rpc.batch` (`{"commands":[{"command","params"}],"keep_going":false}`, the result of `xedit batch`) and `rpc.shutdown` (ends the daemon).
- Mutating commands behave as on the CLI: without `--edit` at startup they fail with `edit_required` unless `dry_run` is true, and nothing reaches disk until a `files.save` request (use `output` to write somewhere safe while testing).
- A failed command is an error object: `message` is the message of the command, `data.code` its stable code (`edit_required`, `unknown_record`, ...), and `code` is `-32601` for `unknown_command`, `-32602` for `invalid_params`, `-32603` for `internal` and `-32000` for every other failure.
- `mcp` speaks the Model Context Protocol on stdio.

Register it with an MCP client as the command above.

Each command is a tool named like the command with `.` replaced by `_` (`records_get`, `elements_set`, `files_save`), its `inputSchema` and `outputSchema` are the request and response schemas, and a result carries the JSON as text and as `structuredContent`.

A failed command is a result with `isError: true` and the text `<code>: <message>`.

Mutating tools say so in their description and are not read-only in their annotations.

The plugins load at the first tool call, which can take a while on a big load order; `initialize` and `tools/list` answer at once.

The server has no resources, prompts or progress notifications.
- A session starts with the plugins of the command line; `session.load` (needs `--edit` unless `--dry-run`) loads another list into the running one, or the same list with a tool mode's load settings, which is how a daemon runs the quick clean modes with xEdit's own load settings: `session.load --plugin <path> --mode quickautoclean`, then `clean --quick` (its warning about a generic load is gone then) or `tool run quickautoclean`.
