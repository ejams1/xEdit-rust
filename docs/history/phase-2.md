# Phase 2: All games and saves


- **Model:** Sonnet 5.5. High-volume, mechanical work driven by the transpiler and checked by dump parity. Hand a definition callback or a parity difference that resists two attempts to Opus 5.5.
- **Port:** Remaining definitions (TES3, TES4, FO3, FNV, TES5, Enderal, VR variants, FO76, SF1 including reflection data) and the save game definitions.
- **CLI:** `--game` for all 14 game modes, `xedit saves dump`.
- **Skills:** Extend `using-xedit-cli` with per-game notes. First draft of `syncing-upstream` (see below), exercised on definition-only upstream commits, which are the most common kind.
- **Gate:** Dump parity for every game mode and every save format.
- **Status:** Gate met 2026-10-07 on branch `phase-2-all-games` (pull request #8). `cargo xtask parity dump` over all 11 games: 246 of 249 files equal or equal-prefix; the 3 others are the Morrowind masters, which the oracle cannot dump (#9). `cargo xtask parity saves` over 58 saves of 6 games: all equal, equal-prefix or equal-error.

The Skyrim LE saves are prefix checks against a 15 minute oracle run (`--oracle-timeout`), because the oracle raises an exception per unresolved FormID and writes 1 KB per second on them; the Oblivion saves end with the same header magic error in the oracle and the port, because upstream's Oblivion save definitions expect `FO3SAVEGAME`.

Enderal is not installed (#7).
