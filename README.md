# xEdit-rust
Rebuild of the xEdit project in Rust. Additionally features CLI control of all xEdit features and native support for AI agents to control it.

The build plan is in [docs/PLAN.md](docs/PLAN.md).

## Goals

- Port xEdit to Rust with 1:1 functionality*
  - Morrowind and Enderal support coming later
- Match the upstream release build exactly
  - The official binaries of the baseline tag (`xedit-4.1.5q`) are the oracle; a differential harness compares the port's output with theirs on the real game files, and each phase closes only when its parity gate holds. Upstream quirks are reproduced, not corrected.
- Keep existing scripts, plugins and workflows working. Compatibility with xEdit behaviour, its Pascal scripts and the files it writes wins over every other concern.
- Be easier to contribute to than the Delphi original: a modern toolchain (`cargo build`, `cargo test`, clippy, one command for the parity check), no proprietary compiler, typed interfaces instead of 103 COM-style interfaces in one 50,000-line unit, and a module per upstream unit so the Pascal source stays a readable map of the Rust.
- Expose every operation through the `xedit` CLI as a typed command with `--json` output, stable error codes and a printable schema (`xedit schema`), so that an AI agent can discover and drive the whole tool without the GUI.
  - Make mutation safe for agents: an explicit edit flag, `--dry-run`, structured outcomes, atomic saves, and a long-lived session (`xedit serve`, `xedit mcp`) that runs the same commands.
- Stay mergeable with upstream. One Rust module per Pascal unit with matching names, and `upstream-map.toml` records the last upstream commit merged into each, so upstream changes map onto the port line by line.
- Get faster than upstream once parity holds

## Status

- Phases 0 to 2 of the plan are done: every game mode from Morrowind to Starfield and every save format load and dump like the release build, verified file by file against it on the local game installs.
- Phase 3 (write path and daemon) is in progress. `xedit save` writes a loaded plugin back as xEdit saves it, with the edits xEdit makes to the file header on save, `--dry-run` and an explicit `--edit` flag; the round-trip check (`cargo xtask parity roundtrip`) compares the result with the input, and `cargo xtask parity oracle-save` drives the xEdit GUI release build headlessly to compare it with the bytes xEdit itself saves: the port's save equals xEdit's for every plugin of the corpus that xEdit saves (the 4.1.5q GUI saves no Morrowind plugin), and `cargo xtask parity oracle-edit` replays scripted edit sequences on both. `xedit elements set` changes the value of any element (and adds a missing member of a record), with every `AfterSet` and `AfterLoad` callback of the definitions ported, and `xedit batch` runs several commands in one session, so an edit and a save go together. `xedit masters add|sort|clean` adds, sorts and cleans the masters of a plugin as the xEdit menu does, with every FormID of the plugin rewritten to follow them. `xedit formids change` and `xedit formids renumber` give records new FormIDs (renumbering, compacting for ESL, injecting into a master) and update every record that refers to them, and `xedit files flags` sets the ESM, ESL, medium, update and localized flags of a plugin header. `xedit elements add|remove` adds a member, an array entry or a child record (a reference of a cell, a response of a topic, a cell of a worldspace) and removes what xEdit lets you remove, `xedit records copy` copies a record into a plugin as an override or as a new record (with its child group when deep, and with the masters it needs added), and `xedit records delete` removes a record with its child group. `xedit serve` keeps a session loaded and answers every registry command as a JSON-RPC method over stdio or a Windows named pipe, and `xedit mcp` serves the same commands as MCP tools over stdio; both are built from the registry at run time, keep the `--edit` gate and `dry_run`, and leave saving to an explicit `files.save`.
- Later phases (analysis and tool modes, archives and assets, scripting, GUI, performance, release) have not started.

## Non-goals

- No new file formats, record definitions or game support beyond what the baseline tag ships. New upstream releases are taken in through the sync procedure, not ahead of it.
- No redesign of the data model, the scripting language or the GUI. JvInterpreter Pascal stays the script language; the GUI reproduces the upstream forms on top of the same commands.
- No separate agent API. Agents use the same command layer as the CLI and the GUI; there is no second surface to keep in step.
- No dependence on a Delphi compiler. The oracle is the released binary, never a rebuild.
- No non-Windows platform before the Windows port is complete. Linux and macOS follow, behind thin platform modules.
- No game files in the repository. The parity corpus comes from local installs through environment variables.

## License

xEdit-rust is a port of [xEdit](https://github.com/TES5Edit/TES5Edit) by ElminsterAU and the xEdit contributors. xEdit is licensed under the Mozilla Public License 2.0, and ported files are Modifications that must stay under it, so this whole repository is licensed under [MPL-2.0](LICENSE). See [NOTICE](NOTICE) for attribution and third-party terms.

Rules for contributions:

- Every source file starts with the MPL-2.0 notice shown in [NOTICE](NOTICE).
- A file ported from upstream names its upstream origin below that notice.
- Every crate sets `license.workspace = true`.
- Dependencies must pass `cargo deny check licenses` against [deny.toml](deny.toml).
