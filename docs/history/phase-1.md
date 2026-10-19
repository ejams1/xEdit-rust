# Phase 1: Read path for the first games


- **Model:** Fable 5.1. The element and definition model and the transpiler set the patterns every later phase copies, and mistakes here are the most expensive to undo.
- **Port:** `xedit-io`, the element and definition model, record and group parsing, compressed records, localized strings, game detection, load order. Write `xtask port-defs`, a transpiler from the Pascal definition calls to the Rust builder API; callbacks are ported by hand. Port the Fallout 4 and Skyrim SE definitions first.
- **CLI:** `xedit session info`, `xedit files list`, `xedit records get|find|list`, `xedit elements get`, `xedit dump` (xDump equivalent).
- **Skills:** `using-xedit-cli` version 1, covering read-only inspection. `porting-definitions` (run the transpiler, port callbacks, verify).
- **Performance:** Memory-map files. Add criterion benchmarks for load time and dump time against the Delphi numbers. Parallel parsing with rayon moves to phase 3: the game globals are process-wide statics and the element model was not written for concurrent initialisation (issue #4), which the write path has to settle first.
- **Gate:** Dump of every vanilla Fallout 4 master and every vanilla Skyrim Special Edition master equals the oracle dump. Both games close the gate together; neither is a follow-up.
- **Status:** Done (pull request #3, merged 2026-10-06). `cargo xtask parity dump` reports 190 of 190 files equal: 170 Fallout 4 and 10 Skyrim SE files byte for byte, and 10 Fallout 4 files equal up to the point where the oracle itself crashes (`wbConditionAliasToStr` on INFO alias conditions, see the `checking-parity` skill).

Dump performance was taken on in issue #4 (pull request #5): `Update.esm` loads in 0.6 s and dumps in 19 s against the oracle's 2.4 s and 98 s; `Skyrim.esm` dumps in 182 s.

The Fable review of the phase was skipped at merge time and is owed.
