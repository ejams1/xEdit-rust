# xEdit-rust build plan

## Goal

Port xEdit to Rust with 1:1 functionality, expose every operation through a CLI, and make that CLI the native control surface for AI agents. Keep the port mergeable with upstream Delphi changes. Improve performance through concurrency and SIMD once parity holds.

## What is being ported

Baseline: upstream [TES5Edit/TES5Edit](https://github.com/TES5Edit/TES5Edit), release tag `xedit-4.1.5q` (commit `fd1e360`, recorded in `upstream-map.toml`). The baseline is always an official release, because the release binaries are the parity oracle. About 260,000 lines of Pascal in `Core` and `xEdit`, plus BSArch and Sniff.

| Area | Upstream units | Size (lines) | Notes |
| --- | --- | --- | --- |
| Element and definition model | `wbInterface`, `wbImplementation` | 50,000 | 103 interfaces. The heart of the port. |
| Game record definitions | `wbDefinitions{TES3,TES4,FO3,FNV,TES5,FO4,FO76,SF1,Common,Signatures,Reflection}` | 97,000 | Declarative `wbRecord`/`wbStruct` calls plus callbacks. 14 game modes. |
| Save game definitions | `wbDefinitions*Saves`, `wbSaveInterface` | 34,000 | Tool source `tsSaves`. |
| Load order and session data | `wbLoadOrder`, `wbModGroups`, `wbLocalization`, `wbSteamVDFParser`, `wbHelpers` | 5,000 | |
| Conflict detection | `wbConflict` | 1,000 | |
| Archives and textures | `wbBSA`, `wbBSArchive`, `wbDDS`, `wbCompression`, `wbHash`, BSArch | 8,000 | BSA, BA2, libdeflate, lz4, xxHash. |
| Asset formats | `wbDataFormat*`, `wbNif*`, `wbMeshOptimize`, Sniff | 38,000 | NIF, materials, Wwise. |
| LOD generation | `wbLOD` | 3,700 | |
| Scripting | `xEdit/JvI/*`, `xeScriptHost` | 6,000 | JvInterpreter Pascal. 604 host registrations. 171 shipped scripts in `Build/Edit Scripts`. |
| GUI | `xeMainForm` and 21 other forms | 31,000 | 214 menu handlers on the main form. |
| Startup and tool modes | `xeInit`, `wbCommandLine` | 1,700 | 17 tool modes, more than 60 command-line switches. |

Upstream uses three thread classes in total. Loading, reference building and conflict detection are mostly single-threaded, which is where the concurrency gains are.

## Design rules

1. **One operation layer.** Every operation is a typed command in `xedit-session` with a serde request, a serde response and a JSON Schema. The CLI, the daemon, the MCP server and the GUI all call these commands and nothing else. A feature is not done until its command exists. This is what makes CLI coverage and agent control complete by construction and not a follow-up.
2. **Upstream-shaped source.** Each Pascal unit maps to one Rust module with matching names (`wbLoadOrder.pas` to `load_order.rs`, `wbRecord(` to `wb_record(`). `upstream-map.toml` records the unit, the Rust module and the upstream commit last merged. An upstream diff then maps onto Rust lines directly.
3. **Parity is measured, not asserted.** The official Delphi release build of the baseline is the oracle. A differential harness compares Rust output with oracle output on the same inputs. Each phase has a parity gate.
4. **Safe by default for agents.** Mutating commands support `--dry-run`, return structured outcomes, and require an explicit edit flag. Save is atomic (write temp file, rename), which removes the deferred-save problem of the Delphi daemon.
5. **Machine-first output.** Every CLI command has `--json`. Errors have stable codes. `xedit schema` prints the full command catalogue so an agent can discover the surface without documentation.

## Workspace layout

| Crate | Content |
| --- | --- |
| `xedit-io` | Memory-mapped files, streams, compression, hashes, string encodings. |
| `xedit-core` | Element tree, definition model, records, groups, files, FormIDs. |
| `xedit-defs` | Definition builder API plus one module per game and per save format. |
| `xedit-loadorder` | Game detection, load order, mod groups, localization. |
| `xedit-analysis` | Conflicts, reference index, filters, error checks, cleaning. |
| `xedit-archive` | BSA, BA2, DDS. |
| `xedit-assets` | NIF, materials, Wwise, mesh optimization, LOD generation. |
| `xedit-script` | JvInterpreter-compatible Pascal interpreter and host API. |
| `xedit-session` | Session state, command registry, jobs, mutation policy, schemas. |
| `xedit-cli` | `xedit` binary: one-shot subcommands, `serve` (JSON-RPC over stdio and named pipe), `mcp`. |
| `xedit-gui` | GUI on top of `xedit-session`. |
| `bsarch`, `sniff`, `xdump` | Binaries matching the upstream tools. |
| `xtask` | Definition transpiler, parity harness, upstream-diff tooling, coverage ledger. |

## Parity harness

- **Oracle:** the official xEdit release binaries of the baseline tag (`xDump.exe`, `xFOEdit.exe`, `xTESEdit.exe`, `xSFEdit.exe`, `BSArch.exe`, `BSArchPro.exe`, `Sniff.exe`). The project has no Delphi compiler, so the oracle cannot be rebuilt and the port source must match the release tag exactly. The harness finds the binaries through the `XEDIT_ORACLE_DIR` environment variable.
- **Secondary oracle:** the `xEdit-llm` automation build returns conflict, reference and cleaning results as JSON, which is easier to compare than GUI state. It is a fork at a different upstream commit, so it is used only to cross-check and never to close a gate.
- **Corpus:** vanilla masters for each game from the local game installs (never committed), plus small synthetic plugins committed under `tests/fixtures`. The harness finds each game's `Data` directory through an environment variable (`XEDIT_FO4_DATA`, `XEDIT_SSE_DATA`, and one per later game). Fallout 4 and Skyrim Special Edition are installed on the development machine; every other game needs its masters before its gate can close.
- **Checks:** full element dump equality, byte-identical round-trip save, conflict status equality, reference index equality, script output equality, archive listing and extraction equality.
- **Coverage ledger:** `coverage/ledger.toml` lists every GUI event binding, game mode, tool mode, command-line switch and script host function with its status and the command that covers it. `cargo xtask sync <upstream checkout>` generates it from the upstream source and adds new upstream entries as `pending`. `cargo xtask check` validates it in CI. The `xEdit-llm` inventory (`Tools/AgentCoverage`) is a reference for which actions are presentation-only.

## Phases

Each phase lists the port work, the CLI surface it adds, the agent skill work, and the gate that closes it.

### How a phase is run

1. **One pull request per phase.** Each phase is developed on its own branch (`phase-1-read-path` and so on) and opened as one pull request. Open it as a draft when the phase starts, so that CI runs on every push. Upstream sync work is not part of a phase and gets its own pull requests.
2. **Suggested model.** Each phase names the Claude model suggested for the implementation work. The suggestion weighs how much of the phase is new design against how much is volume work that the parity harness checks mechanically. Escalate to the next model up when a problem resists two attempts.
3. **Review by Fable.** When the phase gate passes, a fresh Fable 5.1 session reviews the whole pull request before it is merged. The session must be fresh, also for phases that Fable implemented, so that the review does not inherit the implementer's assumptions. Findings are fixed in the same pull request, and the review is repeated on the fixes.

The review checks:

- The gate evidence is real: the harness ran on the full corpus, and no expected output was edited to match the port.
- Ported code follows the upstream unit and keeps its quirks. Deviations are marked `UPSTREAM-QUIRK:` or justified.
- Every operation added in the phase is a session command with a schema, and `coverage/ledger.toml` and `upstream-map.toml` match what was ported.
- Correctness and safety: error handling, `unsafe` blocks with `// SAFETY:` comments, no output that depends on thread count or CPU features.
- License: file headers, upstream origin lines, dependency licenses.
- The agent skills changed in the phase describe what the code now does.

### Phase 0: Foundations

- **Model:** Fable 5.1. Done.
- **Port:** Workspace, command registry and CI (format, clippy, test, `cargo deny check licenses`, `cargo xtask check`). Pin the upstream release in `upstream-map.toml`. Generate the unit map and the coverage ledger. Build the parity harness skeleton.
- **CLI:** `xedit --version`, `xedit schema`, `xedit call <command>`, global `--json` and error code conventions.
- **Skills:** `porting-pascal-unit` (the procedure for porting one unit: headers, naming, map entry, parity test). `checking-parity` (run the harness and read its report).
- **Gate:** CI green. Oracle produces a dump of one vanilla master.
- **Status:** Done. The dump check of the harness (`cargo xtask parity dump`) was added at the start of phase 1. The 4.1.5q `xDump.exe` dumps a vanilla Fallout 4 master (`-FO4`) and a vanilla Skyrim Special Edition master (`-SSE`) on the development machine.

### Phase 1: Read path for the first games

- **Model:** Fable 5.1. The element and definition model and the transpiler set the patterns every later phase copies, and mistakes here are the most expensive to undo.
- **Port:** `xedit-io`, the element and definition model, record and group parsing, compressed records, localized strings, game detection, load order. Write `xtask port-defs`, a transpiler from the Pascal definition calls to the Rust builder API; callbacks are ported by hand. Port the Fallout 4 and Skyrim SE definitions first.
- **CLI:** `xedit session info`, `xedit files list`, `xedit records get|find|list`, `xedit elements get`, `xedit dump` (xDump equivalent).
- **Skills:** `using-xedit-cli` version 1, covering read-only inspection. `porting-definitions` (run the transpiler, port callbacks, verify).
- **Performance:** Memory-map files. Add criterion benchmarks for load time and dump time against the Delphi numbers. Parallel parsing with rayon moves to phase 3: the game globals are process-wide statics and the element model was not written for concurrent initialisation (issue #4), which the write path has to settle first.
- **Gate:** Dump of every vanilla Fallout 4 master and every vanilla Skyrim Special Edition master equals the oracle dump. Both games close the gate together; neither is a follow-up.
- **Status:** Done (pull request #3, merged 2026-10-06). `cargo xtask parity dump` reports 190 of 190 files equal: 170 Fallout 4 and 10 Skyrim SE files byte for byte, and 10 Fallout 4 files equal up to the point where the oracle itself crashes (`wbConditionAliasToStr` on INFO alias conditions, see the `checking-parity` skill). Dump performance was taken on in issue #4 (pull request #5): `Update.esm` loads in 0.6 s and dumps in 19 s against the oracle's 2.4 s and 98 s; `Skyrim.esm` dumps in 182 s. The Fable review of the phase was skipped at merge time and is owed.

### Phase 2: All games and saves

- **Model:** Sonnet 5.5. High-volume, mechanical work driven by the transpiler and checked by dump parity. Hand a definition callback or a parity difference that resists two attempts to Opus 5.5.
- **Port:** Remaining definitions (TES3, TES4, FO3, FNV, TES5, Enderal, VR variants, FO76, SF1 including reflection data) and the save game definitions.
- **CLI:** `--game` for all 14 game modes, `xedit saves dump`.
- **Skills:** Extend `using-xedit-cli` with per-game notes. First draft of `syncing-upstream` (see below), exercised on definition-only upstream commits, which are the most common kind.
- **Gate:** Dump parity for every game mode and every save format.
- **Status:** Gate met 2026-10-07 on branch `phase-2-all-games` (pull request #8). `cargo xtask parity dump` over all 11 games: 246 of 249 files equal or equal-prefix; the 3 others are the Morrowind masters, which the oracle cannot dump (#9). `cargo xtask parity saves` over 58 saves of 6 games: all equal, equal-prefix or equal-error. The Skyrim LE saves are prefix checks against a 15 minute oracle run (`--oracle-timeout`), because the oracle raises an exception per unresolved FormID and writes 1 KB per second on them; the Oblivion saves end with the same header magic error in the oracle and the port, because upstream's Oblivion save definitions expect `FO3SAVEGAME`. Enderal is not installed (#7).

### Phase 3: Write path and daemon

- **Model:** Opus 5.5. Byte-identical saving and FormID and master rewriting are subtle but follow the upstream code closely.
- **Port:** Element editing, add and remove, copy as override and as new record, master management, FormID change and renumber, ESL/ESM/ESP flag handling, sort, save.
- **Performance:** Parse files in parallel with rayon, once the globals and the element initialisation are safe to share between threads (moved here from phase 1). Build and format records on worker threads for the dump (issue #4, item 3).
- **CLI:** `xedit elements set|add|remove`, `xedit records copy|delete`, `xedit masters add|clean|sort`, `xedit formids change|renumber`, `xedit save`, all with `--dry-run`. `xedit serve` keeps a session loaded and accepts the same commands as JSON-RPC. `xedit mcp` exposes the command registry as MCP tools generated from the schemas.
- **Skills:** `using-xedit-cli` version 2 with the mutation rules (edit flag, dry run, save, readback). `adding-a-command` (command, schema, CLI subcommand, test, ledger entry).
- **Gate:** Load then save is byte-identical for the corpus. A scripted edit sequence gives the same saved bytes as the oracle.

### Phase 4: Analysis and tool modes

- **Model:** Opus 5.5. Many separate features, each with its own oracle check.
- **Port:** Conflict detection, reference index, filters, comparisons, error checks, ITM and UDR cleaning, quick auto clean, mod groups, merged patch, localization tools, and all 17 tool modes with their command-line switches.
- **CLI:** `xedit conflicts`, `xedit refs`, `xedit filter`, `xedit compare`, `xedit check`, `xedit clean`, `xedit modgroups`, `xedit patch merged`, `xedit localization`. The legacy switches (`-quickautoclean`, `-IKnowWhatImDoing` and the rest) stay accepted for compatibility with existing mod manager setups.
- **Skills:** Agent workflow skills on the native CLI: conflict audit, plugin cleaning, patch building. These replace the `xEdit-llm` daemon route.
- **Performance:** Build the reference index and run conflict detection across files in parallel.
- **Gate:** Conflict status, reference index, cleaning result and error check output equal the oracle on the corpus.

### Phase 5: Archives, assets and LOD

- **Model:** Opus 5.5 for NIF, Sniff and LODGen. Sonnet 5.5 for BSA, BA2 and DDS, which are well-specified container formats.
- **Port:** BSA and BA2 read and write, DDS, BSArch, NIF and material formats, Wwise, Sniff operations, LODGen.
- **CLI:** `bsarch` with the upstream arguments, `xedit archive list|extract|pack`, `sniff <operation>`, `xedit lodgen`.
- **Skills:** Asset skills: archive handling, NIF batch operations.
- **Performance:** Parallel pack and unpack. SIMD for half-float conversion, hashing and texture block processing.
- **Gate:** Packed archives are byte-identical to BSArch output. Sniff and LODGen outputs equal the oracle.

### Phase 6: Scripting

- **Model:** Fable 5.1 for the interpreter, because script compatibility is the most important goal and JvInterpreter semantics are undocumented. Sonnet 5.5 for the 598 host bindings once the binding pattern exists.
- **Port:** A Pascal interpreter compatible with the JvInterpreter dialect, and all 604 host registrations bound to `xedit-session` commands. Scripts that build VCL forms run against a form shim in the GUI and fail with a clear error when headless.
- **CLI:** `xedit script run|list|check`.
- **Skills:** `writing-xedit-scripts` for agents that need custom batch logic.
- **Gate:** All 171 shipped scripts parse. Every script without a form produces the same output and the same plugin bytes as the oracle.

### Phase 7: GUI

- **Model:** Opus 5.5 for the main window and the virtualised tree and grid. Sonnet 5.5 for the dialogs.
- **Port:** Main window (navigation tree, view tab with conflict colours, referenced-by, messages, information), all dialogs, themes, bookmarks, settings files. Every action calls a session command.
- **CLI:** No new commands. Any GUI action without a command is a bug against design rule 1.
- **Skills:** None new. The coverage ledger must show every GUI binding as covered or presentation-only.
- **Gate:** Ledger complete. Manual walkthrough of the upstream menus against the Delphi build.

### Phase 8: Performance

- **Model:** Fable 5.1. SIMD, `unsafe` and concurrency changes must keep output identical, and errors here are silent.
- **Work:** Profile with the benchmarks collected since phase 1. Add SIMD with runtime dispatch (SSE2 baseline, AVX2, AVX-512 where the CPU supports it) to hot loops: byte comparison in conflict detection, string and signature scanning, checksums, decompression, numeric conversion. Tune allocation with arenas and string interning. Parallelise remaining serial stages.
- **Rule:** Every optimisation keeps a scalar fallback and must pass the full parity harness. Output never depends on thread count or CPU features.
- **Gate:** Load, reference build, conflict scan, clean and archive pack are each faster than the Delphi build on the corpus, with numbers published in the repository.

### Phase 9: Release and cutover

- **Model:** Sonnet 5.5.
- **Work:** Release packaging with the per-game executable names (`FO4Edit.exe`, `SSEEdit.exe` and the rest). Migration notes. Point the agent plugin skills at `xedit mcp` and retire the `xEdit-llm` daemon route.

## Upstream sync skill

`syncing-upstream` is drafted in phase 2 and extended in every later phase. Procedure:

1. Fetch upstream and find the newest release tag that has published binaries. List the commits between the tag recorded in `upstream-map.toml` and that tag. Commits after the newest release are not merged, because nothing can verify them.
2. Classify each changed unit through the map: definitions, core, GUI, scripting, tools.
3. Port the change. Definition changes go through `xtask port-defs`; other changes are ported by hand with `porting-pascal-unit`.
4. Add or update the session command and CLI subcommand when the change adds an operation, and update the coverage ledger.
5. Point `XEDIT_ORACLE_DIR` at the binaries of the new release and run the parity harness.
6. Advance the recorded tag and commit with `cargo xtask sync <upstream checkout> <commit>` and open one pull request per upstream release.

A unit that is not yet ported gets its upstream change recorded as pending in the map, so nothing is lost while the port is in progress.

## Decisions

1. **GUI toolkit:** egui with a custom virtualised tree and grid.
2. **Game order:** Fallout 4 and Skyrim SE first. Every game mode upstream supports is in scope.
3. **Script compatibility:** full JvInterpreter Pascal compatibility. Compatibility with existing xEdit behaviour, scripts and files is the most important goal of the project and wins over every other concern.
4. **Platforms:** Windows first, then Linux and macOS. Windows-only code stays in thin platform modules.
5. **Side tools:** BSArch, BSArchPro, Sniff, xDump and LODGen are all in scope.

## Risks

- **Undocumented behaviour.** Much of xEdit's behaviour lives in definition callbacks and special cases. The parity harness on real masters is the main defence, so it is built in phase 0, before the port.
- **Definition volume.** 131,000 lines of definitions are not practical to port by hand. The transpiler is on the critical path of phase 1.
- **Scripts that use VCL forms.** 69 host registrations expose VCL classes. The form shim in phase 6 must cover what shipped and popular community scripts use.
- **Upstream drift during the port.** Handled by the pending entries in `upstream-map.toml` and by running `syncing-upstream` from phase 2 on.
