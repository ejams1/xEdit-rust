---
name: adding-a-command
description: Use when adding a session command to xedit-session so the CLI, the serve daemon and the MCP server all offer it. Covers the request and response structs with their JSON Schema, registering the command (name rules, mutates and dry_run, the edit gate), the CLI subcommand, keeping stdout clean, the tests that pick the command up automatically, the coverage ledger entry, the upstream map and the docs to update.
---

# Adding a command

Every operation of the port is a command of the registry in `crates/xedit-session`. The CLI, `xedit serve`, `xedit mcp` and (later) the GUI call the registry and nothing else, so a command is added once and every front end has it. A feature is not done until its command exists (design rule 1 in `docs/PLAN.md`). Use this skill together with `porting-pascal-unit` (which ports the upstream routine the command calls) and read a finished command next to it: `crates/xedit-session/src/masters.rs` is the smallest complete example, `formids.rs` and `edit.rs` show the heavier cases.

## 1. Pick the name and the module

- A command name is `<group>.<verb>`: `masters.add`, `records.copy`, `files.save`. Reuse a group when the object is the same (`files.*` for a plugin as a file, `records.*` for main records, `elements.*` for anything inside a record).
- Allowed characters are `[A-Za-z0-9._-]`. The MCP tool name is the command name with `.` replaced by `_` and must be `[A-Za-z0-9_-]` and at most 64 characters, so two commands may not differ only by `.` against `_` (`files.save` and `files_save`). The test `every_registered_command_is_a_tool` in `crates/xedit-cli/src/mcp.rs` fails if they collide or the name is invalid.
- Put the command in the module of its group, or in a new module `crates/xedit-session/src/<group>.rs` declared with `pub mod <group>;` in `lib.rs`. Start the file with the MPL notice and, for a command that runs an upstream routine, `// Ported from xEdit: <unit> (<routines>)`. `cargo xtask check` rejects a file without the notice.

## 2. Write the request and response

Both are plain serde structs that derive `JsonSchema`; the registry turns them into the object schemas that `xedit schema`, `rpc.discover` and MCP `tools/list` publish, so the doc comments on the fields are the documentation an agent reads.

```rust
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MastersRequest {
    /// Name of the loaded plugin to change; the only loaded plugin when omitted.
    pub file: Option<String>,
    /// Report the master list the command would leave, but change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct MastersResponse { /* the facts a caller needs, with doc comments */ }
```

- The request is an object: derive `Deserialize` with `#[serde(deny_unknown_fields)]` so a misspelt field is `invalid_params`, not ignored. A command without parameters takes `NoParams`. Optional parameters are `Option<T>` or `#[serde(default)]`; a default other than the type's own is a `#[serde(default = "fn")]` function (see `default_true` in `masters.rs`).
- Name fields as the other commands do: `form_id` (load order FormID as hexadecimal text, parsed with `commands::parse_form_id`), `file` (a loaded plugin, resolved with `Session::file`), `path` (an element path with `\`), `dry_run`. Use JSON types a client can build: strings, numbers, booleans, arrays and objects; no tuples.
- The response is serialized with `Serialize` and derives `JsonSchema`. Report what changed (`old` and `new` values, the lists a caller needs next such as `masters`), `changed`, and for a mutating command the plugin that now holds a change to save. A dry run fills the same fields with what the command would give, and says so (`dry_run: true`, or `changed: false`).
- Errors are `CommandError::new(code, message)`. Use the existing codes where they fit (`invalid_params`, `unknown_file`, `unknown_record`, `unknown_element`, `not_editable`, `not_removable`, `edit_failed`, `save_refused`, `io`, `internal`); an upstream exception becomes `edit_failed` with the upstream message unchanged, because agents match on it and parity checks compare it. A new code is a public interface: add it to the list in `using-xedit-cli`.

## 3. Write the handler and register it

```rust
fn masters_sort(session: &mut Session, request: MastersRequest) -> Result<MastersResponse, CommandError> {
    let file = session.file(request.file.as_deref())?;
    ...
}

pub fn register(registry: &mut Registry) {
    registry.register("masters.sort", "Sort the masters of a plugin by load order (SortMasters).", true, masters_sort);
}
```

- A handler is `fn(&mut Session, Request) -> Result<Response, CommandError>`. The summary is one sentence naming the upstream routine; it is the MCP tool description.
- A new module's `register` is called from `Registry::standard()` in `lib.rs`; a command added to an existing module is registered in that module's `register`. Registering a name twice panics at startup, which every test sees.
- **Reads** pass `mutates = false`. **Anything that changes plugin data or writes a file** passes `mutates = true` and has a `dry_run: bool` field in its request. `Registry::call` then enforces the edit gate before the parameters are parsed: without `--edit` the command fails with `edit_required` unless the raw params hold `"dry_run": true`. The handler never checks the flag itself; it must make a dry run change nothing. The same gate holds in `batch`, `serve` and `mcp`, and the daemon tests assert it for every mutating command.
- A dry run reports what the real call would do, so run the same checks. Upstream's editability checks read `wbEditAllowed`, which is off without `--edit`; run them as with the flag on and restore it: `with_edit_allowed` in `edit.rs` for one check, the `DryRunEdit` guard in `formids.rs` for a whole handler. Compute counts and lists without calling the mutating routine (see the dry runs of `masters_add` and `masters_clean`), or use the routine's own dry mode; never mutate and revert.
- Handlers run on one thread against one `Session`. The data lives in the process-wide file list, so a handler must not keep references across calls; take what it needs from `session.file` and `session.record`.
- Changes stay in memory until `files.save`. A command that edits data therefore does not write files; say in its summary or response that the plugin needs saving.

## 4. Add the CLI subcommand

In `crates/xedit-cli/src/main.rs`:

1. Add a variant with the same fields as the request to the clap enum of its group (`MastersAction`, `RecordsAction`, ...), or a new group enum and an `Action` variant. Doc comments become the `--help` text. A `dry_run` field is `#[arg(long)] dry_run: bool`.
2. Map it in `command_of` to `(name, json!({...}))` with the request's field names. The CLI does no work of its own: it only builds the request, so the CLI, `batch` and `call` can not drift apart. Defaults live in the request struct; map a negative flag such as `--no-sort` to `"sort": !no_sort`.
3. Nothing else is needed for `serve` and `mcp`: they build their method and tool lists from the registry when they start.

A command with no CLI subcommand is still reachable with `xedit call <name> --params '<json>'`; add the subcommand when a person would type it.

**Write nothing to stdout.** In `serve` and `mcp` stdout is the protocol, and with `--json` it is the envelope. Command code must not call `println!` or `print!`; progress and logs go to stderr (`eprintln!`, or the progress callback that `log_progress_to_stderr` installs). A command returns its result as the response struct and the front end prints it.

## 5. Test it

- **Integration test** in `crates/xedit-session/tests/<group>.rs`: build small synthetic plugins as bytes (copy the `sub_record`, `main_record`, `group` and `header` helpers of `tests/structure.rs` or `tests/masters.rs`), load them with `wb_file_from_bytes` into `Session::with_files`, call `Registry::standard().call(&mut session, "<name>", json!({...}))`, and assert on the response and on the bytes of `write_to_bytes`. Take `test_lock()` first: the globals are process-wide, so tests that load files serialize on it. Cover the dry run (nothing changed), the real call, an error code, and the saved bytes. When a corpus game is installed, an extra test may read `XEDIT_SSE_DATA` and skip itself when it is not set (see `copies_a_skyrim_npc_as_an_override`).
- **Registry-wide tests pick the command up with no change**: `every_registered_command_is_a_method` (`serve.rs`), `every_registered_command_is_a_tool` and `every_tool_is_callable` (`mcp.rs`) call each command with empty params on an empty session, and `mutating_commands_keep_the_edit_gate_and_dry_run` calls each mutating command with `{}` and with `{"dry_run": true}`. So a command must answer empty params with an error, never a panic or a hang, a mutating one must return `edit_required` for `{}`, and with `dry_run` it must fail with something else (usually `no_session`). A required field gives `invalid_params`, which is fine.
- **Parity** for anything that writes a file: the plan's gate compares saved bytes with the oracle's. Add an edit sequence under `crates/xtask/oracle/edits/` (see `checking-parity`, "Oracle edit sequences") when the oracle's script API can do the same edit, and translate the command in `edit_step` of `crates/xtask/src/parity/oracle_save.rs`.
- Run `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `cargo xtask check`; CI runs the same four.

## 6. Ledger, map and docs

- **`coverage/ledger.toml`**: set every upstream entry the command covers (a GUI event, a script function, a switch) to `status = "covered"` with `command = "<name>"`, and keep `source`. Use `note` for what is presentation (a dialog turned into parameters) or partial (one file per call). `cargo xtask check` requires a `command` on every `covered` entry but does not look the name up in the registry, so type it exactly. Entries the command does not cover stay `pending`; `presentation` and `excluded` (which needs a note) are for what no command should do. Never add or remove entries by hand: `cargo xtask sync <upstream checkout>` maintains the list.
- **`upstream-map.toml`**: the unit that holds the ported routine keeps `status = "pending"` until the whole unit is ported. Extend its `note` with the routines the command made reachable and with what is still missing, and make `rust` point at the file or directory that holds the port (`check` fails when the path does not exist). For the `xeMainForm.pas` menu handlers the note names each handler and the command that runs it.
- **`docs/PLAN.md`**: the step list and Status paragraph of the phase, with the parity result and anything owed.
- **`README.md`**: the Status paragraph, and the Goals and Non-goals when the command changes them (for example a command for something the list calls a non-goal).
- **`.claude/skills/using-xedit-cli/SKILL.md`**: a row in the command tables, the rules the command adds (its dry run, its error codes), and a line under "Known gaps" for anything not ported.

## Checklist

1. Request and response structs derive `JsonSchema`; doc comments on every field; `deny_unknown_fields`.
2. Registered with the right `mutates`; mutating requests have `dry_run`; the dry run changes nothing.
3. No stdout output anywhere in the command path.
4. CLI variant and `command_of` mapping, or a deliberate `call`-only command.
5. Integration test with the bytes of a save, under `test_lock()`; registry-wide tests still pass.
6. Ledger entries `covered`, map note, PLAN, README and `using-xedit-cli` updated.
