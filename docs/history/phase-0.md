# Phase 0: Foundations


- **Model:** Fable 5.1. Done.
- **Port:** Workspace, command registry and CI (format, clippy, test, `cargo deny check licenses`, `cargo xtask check`). Pin the upstream release in `upstream-map.toml`. Generate the unit map and the coverage ledger. Build the parity harness skeleton.
- **CLI:** `xedit --version`, `xedit schema`, `xedit call <command>`, global `--json` and error code conventions.
- **Skills:** `porting-pascal-unit` (the procedure for porting one unit: headers, naming, map entry, parity test). `checking-parity` (run the harness and read its report).
- **Gate:** CI green. Oracle produces a dump of one vanilla master.
- **Status:** Done. The dump check of the harness (`cargo xtask parity dump`) was added at the start of phase 1. The 4.1.5q `xDump.exe` dumps a vanilla Fallout 4 master (`-FO4`) and a vanilla Skyrim Special Edition master (`-SSE`) on the development machine.
