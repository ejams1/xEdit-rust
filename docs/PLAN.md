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
- **Corpus:** vanilla masters for each game from the local game installs (never committed), plus small synthetic plugins committed under `tests/fixtures`.
- **Checks:** full element dump equality, byte-identical round-trip save, conflict status equality, reference index equality, script output equality, archive listing and extraction equality.
- **Coverage ledger:** `coverage/ledger.toml` lists every GUI event binding, game mode, tool mode, command-line switch and script host function with its status and the command that covers it. `cargo xtask sync <upstream checkout>` generates it from the upstream source and adds new upstream entries as `pending`. `cargo xtask check` validates it in CI. The `xEdit-llm` inventory (`Tools/AgentCoverage`) is a reference for which actions are presentation-only.

## Phases

Each phase lists the port work, the CLI surface it adds, the agent skill work, and the gate that closes it.

### Phase 0: Foundations

- **Port:** Workspace, command registry and CI (format, clippy, test, `cargo deny check licenses`, `cargo xtask check`). Pin the upstream release in `upstream-map.toml`. Generate the unit map and the coverage ledger. Build the parity harness skeleton.
- **CLI:** `xedit --version`, `xedit schema`, `xedit call <command>`, global `--json` and error code conventions.
- **Skills:** `porting-pascal-unit` (the procedure for porting one unit: headers, naming, map entry, parity test). `checking-parity` (run the harness and read its report).
- **Gate:** CI green. Oracle produces a dump of one vanilla master.
- **Status:** Done except the harness. The 4.1.5q `xDump.exe` dumps a vanilla Fallout 4 master on the development machine.

### Phase 1: Read path for the first games

- **Port:** `xedit-io`, the element and definition model, record and group parsing, compressed records, localized strings, game detection, load order. Write `xtask port-defs`, a transpiler from the Pascal definition calls to the Rust builder API; callbacks are ported by hand. Port the Fallout 4 and Skyrim SE definitions first.
- **CLI:** `xedit session info`, `xedit files list`, `xedit records get|find|list`, `xedit elements get`, `xedit dump` (xDump equivalent).
- **Skills:** `using-xedit-cli` version 1, covering read-only inspection. `porting-definitions` (run the transpiler, port callbacks, verify).
- **Performance:** Memory-map files. Parse files in parallel with rayon. Add criterion benchmarks for load time and dump time against the Delphi numbers.
- **Gate:** Dump of every vanilla FO4 and SSE master equals the oracle dump.

### Phase 2: All games and saves

- **Port:** Remaining definitions (TES3, TES4, FO3, FNV, TES5, Enderal, VR variants, FO76, SF1 including reflection data) and the save game definitions.
- **CLI:** `--game` for all 14 game modes, `xedit saves dump`.
- **Skills:** Extend `using-xedit-cli` with per-game notes. First draft of `syncing-upstream` (see below), exercised on definition-only upstream commits, which are the most common kind.
- **Gate:** Dump parity for every game mode and every save format.

### Phase 3: Write path and daemon

- **Port:** Element editing, add and remove, copy as override and as new record, master management, FormID change and renumber, ESL/ESM/ESP flag handling, sort, save.
- **CLI:** `xedit elements set|add|remove`, `xedit records copy|delete`, `xedit masters add|clean|sort`, `xedit formids change|renumber`, `xedit save`, all with `--dry-run`. `xedit serve` keeps a session loaded and accepts the same commands as JSON-RPC. `xedit mcp` exposes the command registry as MCP tools generated from the schemas.
- **Skills:** `using-xedit-cli` version 2 with the mutation rules (edit flag, dry run, save, readback). `adding-a-command` (command, schema, CLI subcommand, test, ledger entry).
- **Gate:** Load then save is byte-identical for the corpus. A scripted edit sequence gives the same saved bytes as the oracle.

### Phase 4: Analysis and tool modes

- **Port:** Conflict detection, reference index, filters, comparisons, error checks, ITM and UDR cleaning, quick auto clean, mod groups, merged patch, localization tools, and all 17 tool modes with their command-line switches.
- **CLI:** `xedit conflicts`, `xedit refs`, `xedit filter`, `xedit compare`, `xedit check`, `xedit clean`, `xedit modgroups`, `xedit patch merged`, `xedit localization`. The legacy switches (`-quickautoclean`, `-IKnowWhatImDoing` and the rest) stay accepted for compatibility with existing mod manager setups.
- **Skills:** Agent workflow skills on the native CLI: conflict audit, plugin cleaning, patch building. These replace the `xEdit-llm` daemon route.
- **Performance:** Build the reference index and run conflict detection across files in parallel.
- **Gate:** Conflict status, reference index, cleaning result and error check output equal the oracle on the corpus.

### Phase 5: Archives, assets and LOD

- **Port:** BSA and BA2 read and write, DDS, BSArch, NIF and material formats, Wwise, Sniff operations, LODGen.
- **CLI:** `bsarch` with the upstream arguments, `xedit archive list|extract|pack`, `sniff <operation>`, `xedit lodgen`.
- **Skills:** Asset skills: archive handling, NIF batch operations.
- **Performance:** Parallel pack and unpack. SIMD for half-float conversion, hashing and texture block processing.
- **Gate:** Packed archives are byte-identical to BSArch output. Sniff and LODGen outputs equal the oracle.

### Phase 6: Scripting

- **Port:** A Pascal interpreter compatible with the JvInterpreter dialect, and all 604 host registrations bound to `xedit-session` commands. Scripts that build VCL forms run against a form shim in the GUI and fail with a clear error when headless.
- **CLI:** `xedit script run|list|check`.
- **Skills:** `writing-xedit-scripts` for agents that need custom batch logic.
- **Gate:** All 171 shipped scripts parse. Every script without a form produces the same output and the same plugin bytes as the oracle.

### Phase 7: GUI

- **Port:** Main window (navigation tree, view tab with conflict colours, referenced-by, messages, information), all dialogs, themes, bookmarks, settings files. Every action calls a session command.
- **CLI:** No new commands. Any GUI action without a command is a bug against design rule 1.
- **Skills:** None new. The coverage ledger must show every GUI binding as covered or presentation-only.
- **Gate:** Ledger complete. Manual walkthrough of the upstream menus against the Delphi build.

### Phase 8: Performance

- **Work:** Profile with the benchmarks collected since phase 1. Add SIMD with runtime dispatch (SSE2 baseline, AVX2, AVX-512 where the CPU supports it) to hot loops: byte comparison in conflict detection, string and signature scanning, checksums, decompression, numeric conversion. Tune allocation with arenas and string interning. Parallelise remaining serial stages.
- **Rule:** Every optimisation keeps a scalar fallback and must pass the full parity harness. Output never depends on thread count or CPU features.
- **Gate:** Load, reference build, conflict scan, clean and archive pack are each faster than the Delphi build on the corpus, with numbers published in the repository.

### Phase 9: Release and cutover

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
