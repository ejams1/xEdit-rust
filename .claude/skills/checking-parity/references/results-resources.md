# Resources



`cargo xtask parity resources [--game G]... [--file <stem>]...` runs the scenarios of `crates/xtask/oracle/resources/*.json` (issue #32): a game, the plugins to load, the archives of the game's data folder to link into the private data folder (`*` patterns), loose files, the game ini and custom ini, switches (`-skipbsa`, `-forcebsa`), a generated `hash_plugin` (Fallout 3 and New Vegas: one `STAT` whose `MODT` holds the TES4 hashes of given texture paths) and probes.

The GUI runs `crates/xtask/oracle/resources.pas` in its script mode with the probes filled in; the port runs the same script through the same legacy command line (`xedit -FO4 -D: -P: -M: -I: -S: -script:`), so both load in the script tool mode.

A scenario with `"mode": "check-for-errors"` or `"quick-auto-clean"` runs that tool mode instead (no probes) — the modes where `wbLoadBSAs` is off or `-skipbsa` acts, since the script mode always loads the archives.

Compared: the probe's output (`containers`; `resource` with the containers that hold it and the CRC32 of the winning data; `listing` of a container below a folder, count and the first and last names; `value` of a localized element; `key`, the sort key of a file or folder hash) and the loader's resource lines (`Loading Resources.`, `Version:`, `Skipped.`, `Can't find`, `Setting Resource Path.`, the cache lines), each side's data folder written `<data>\`.

Report `target/parity/resources.json`; oracle results cached in `<cache>/<tag>/<MODE>-oracle-resources/<scenario>.<key>/`.

Results 2026-10-15 (issue #32): 8 of 8 scenarios equal: `fo4-dlc-main` (the DLC tables from `DLCRobot - Main.ba2`, no ini), `fo4-game-ini` (the archives of the game ini and the custom ini first, a missing one warned of, a loose table winning over its archive), `fo4-skipbsa` (`-skipbsa` does nothing in the script mode), `fo4-check-skip` and `fo4-clean-skipbsa` (every archive `Skipped.`, the Misc archive twice since a skipped one is not a container), `fnv-clean-default` and `fnv-clean-forcebsa` (New Vegas skips by default, `-forcebsa` loads), and `fnv-hashes` (the names of ten pairs of colliding TES4 hashes, which fixed the bucket order of `BuildCache`'s `TDictionary`: 2^21 buckets and FNV-1a of the UTF-16 code units).
