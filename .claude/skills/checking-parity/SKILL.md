---
name: checking-parity
description: Use when verifying that the Rust port behaves the same as Delphi xEdit, when a phase gate in docs/PLAN.md has to be closed, or when a parity difference has to be investigated.
---

# Checking parity

The official xEdit release build of the tag in `upstream-map.toml` is the oracle. The project has no Delphi compiler, so only released binaries can be the oracle. The port is correct when it produces the same output as the oracle on the same input.

## Running the harness

```
cargo xtask parity dump|saves|roundtrip [--game <game>]... [--file <name>]... [--oracle-only] [--jobs <n>]
                                        [--memory-budget <GiB>] [--max-memory <GiB>] [--oracle-timeout <minutes>]
```

The harness runs the oracle `xDump.exe` and the port's `xedit dump` on every vanilla plugin of the selected games and compares the outputs byte for byte. The games are `fo4`, `sse`, `tes3`, `tes4`, `fo3`, `fnv`, `tes5`, `tes5vr`, `fo4vr`, `fo76` and `sf1`. Without `--game` it checks every game whose data directory is set and prints a `skipped` line for the others. `--file` restricts the run to named plugins, vanilla or not. It prints the first differing line of each file, writes `target/parity/dump.json` and exits with an error when any file differs.

Environment variables:

| Variable | Content |
| --- | --- |
| `XEDIT_ORACLE_DIR` | Unpacked release archive of the baseline tag. |
| `XEDIT_<GAME>_DATA` | `Data` directory of each game: `XEDIT_FO4_DATA`, `XEDIT_SSE_DATA`, `XEDIT_TES3_DATA` (Morrowind's `Data Files`), `XEDIT_TES4_DATA`, `XEDIT_FO3_DATA`, `XEDIT_FNV_DATA`, `XEDIT_TES5_DATA`, `XEDIT_TES5VR_DATA`, `XEDIT_FO4VR_DATA`, `XEDIT_FO76_DATA`, `XEDIT_SF1_DATA`. |
| `XEDIT_<GAME>_SAVES` | Save folder of each game for `parity saves`: `XEDIT_FO4_SAVES`, `XEDIT_SSE_SAVES`, `XEDIT_TES5_SAVES`, `XEDIT_FNV_SAVES`, `XEDIT_FO3_SAVES`, `XEDIT_TES4_SAVES`. |
| `XEDIT_PARITY_CACHE` | Cache directory. Optional; defaults to the user cache directory. |

Oracle output is cached zstd-compressed as `<cache>/<tag>/<game>/<file>.<hash>.oracle.txt.zst`, so the oracle runs once per input file; plain `.oracle.txt` files from older runs are still read. A run that the oracle ended with `Unexpected Error` is kept as `<file>.<hash>.oracle.crashed.txt.zst`, with the exception's log line in `<file>.<hash>.oracle.error`, and compared as a prefix: the port has to match it up to the crash, and the rest of its output must still finish without an error, which the report shows as `equal-prefix`. A port that stops with the same error message after the same output (`error: <message>` as its last log line) is reported as `equal-error`; the Oblivion saves end that way, because upstream's Oblivion save definitions expect the Fallout 3 header magic. An oracle run that ended with `EInOutError` (I/O error 1450 under memory pressure) is not kept and is reported as `oracle-failed`; run the file again. An oracle assertion failure while loading (`EAssertionFailed` in `wbImplementation.pas`) was seen once on a Fallout New Vegas save and did not repeat: delete the `.oracle.crashed.txt.zst` and run the file again before reading anything into it. The oracle is slow on large masters: it writes about 80 MB of dump per minute and needs more than ten minutes for `Fallout4.esm`. `--oracle-only` fills the cache without running the port. The port output is compared while the port writes it; it is kept only for a file that differs, as `.port.txt.zst` up to the first difference (`zstd -dc` reads it), next to the port's log `.port.log`.

### Memory

Every oracle and port process runs in a Windows job object that caps its committed memory at `--max-memory` (default: the budget), so a runaway dump fails alone instead of pushing the machine out of memory. A port that reaches the cap is reported as `port-memory-limit`; an oracle that reaches it is reported as `oracle-failed` and its output is not cached. The peak of each complete run is stored next to the cache as `.oracle.peak` and `.port.peak` and shown in `dump.json` as `oracle_peak` and `port_peak`.

The processes that run at once stay within `--memory-budget` (default: three quarters of the installed memory). Each run reserves the peak its file had in the last run, or, before the first run, 6 times the size of the plugin and its game master; a file whose reservation exceeds the budget runs alone. `--jobs` (default 3) is only an upper bound on the parallel runs. The largest port peak of the Fallout 4 and Skyrim SE corpus is 1.6 GiB (`DLCCoast.esm`, `Fallout4.esm`). The wall clock is bounded by the largest file: the port dumps `Skyrim.esm` (8.3 GB of text) in about 3 minutes and `Update.esm` in about 20 seconds on the reference machine.

`parity saves` runs `xDump.exe -saves` and `xedit saves dump` on every save and co-save in the save folder of each game (`.fos`, `.ess`, `.f4se`, `.skse`, `.nvse`, `.fose`, `.obse`), with the game's data folder as `-D:` and `--data`, and caches the oracle output under `<cache>/<tag>/<game>-saves`. The report is `target/parity/saves.json`. The oracle is far too slow on Skyrim LE saves to finish: it raises an exception for every hardcoded FormID it cannot resolve (see `using-xedit-cli`) and writes about 1 KB per second, where the port writes 140 MB for `quicksave.ess`. `--oracle-timeout <minutes>` stops the oracle after that long and keeps its output as `<file>.<hash>.oracle.timeout.txt.zst`, compared as a prefix like a crashed run (`equal-prefix`). The Skyrim LE saves were checked with `--oracle-timeout 15`; delete the `.oracle.timeout.txt.zst` of a save to extend its prefix. Oblivion saves need `XEDIT_TES4_SAVES` (`.ess`, `.obse`) and Fallout 3 saves `XEDIT_FO3_SAVES` (`.fos`, `.fose`); on this machine both folders are under `M:\projects\saves`.

### Round trip

`parity roundtrip` runs `xedit --edit --game <game> --load <plugin> save --no-backup --output <cache>/<tag>/<game>-roundtrip/<file>.<hash>.saved` on every corpus plugin and compares the saved bytes with the input, byte for byte. No oracle binary runs: the input file is the oracle, which holds exactly where xEdit writes a loaded file back unchanged. The report is `target/parity/roundtrip.json` and the statuses are:

- `equal`: identical bytes; the saved file is deleted.
- `ofst-dropped`: the saved file is the input without the `OFST` subrecord (and its `XXXX` size prefix) of every worldspace, with the record and group sizes adjusted. xEdit drops those offsets when it loads a worldspace and writes the record without them, so this is the expected result for a file that overrides a worldspace; the harness rebuilds the expected bytes from the input and compares. Not counted as passing until the oracle's own save confirms it (step 6 of phase 3).
- `different`: the first differing offset is printed and the saved file kept. Investigate as any difference, but check first whether upstream itself changes the file on save in a way the input cannot show: after a `PrepareSave` on a modified file it marks the children of a worldspace whose offsets were dropped modified (every record under it is rebuilt, compressed records are compressed again with libdeflate level 12), it rebuilds the `ONAM` list of a master, and it writes a group in its sorted order. Those files need the oracle's own saved bytes, which step 6 adds (the GUI build driven headlessly on a copy of the corpus).
- `refused`: the port refused the save with an upstream `PrepareSave` message, which the oracle gives for the same file (a Starfield blueprint module, for example). Counted as passing.
- `unsupported`: upstream would edit the file header on save (the `HEDR` record count, `INCC`, `ONAM`, a flag, a clamped FormID) and this version cannot yet. Not passing; the element editing step of phase 3 removes these.
- `port-failed`, `port-memory-limit`: as for the dump.

The saves run under the same memory cap and budget as the dumps and reuse the `.port.peak` files. The port's log of each save is `<file>.<hash>.port.log` next to the saved file.

#### Step 1 results (2026-10-07)

249 plugins of 11 games in 1.5 minutes: 132 `equal`, 38 `ofst-dropped`, 2 `refused` (the Starfield blueprint masters), 77 `unsupported`, 0 `different`. The `unsupported` saves are the header edits upstream makes on save: 39 `ONAM` rebuilds, 34 `HEDR` record counts that differ from the loaded tree because the load drops empty top level groups and merges duplicated ones (verified on `Dragonborn.esm`: the raw file holds an empty `AVIF` group the oracle and the port both drop, so the count is one less), the ESM flag of the 3 Morrowind masters, one clamped FormID (`FalloutNV`). The numbers are also in the phase 3 status of `docs/PLAN.md`.

The other checks of the table below are added to `crates/xtask/src/parity.rs` in the phase that ports the feature.

## Running the oracle

`XEDIT_ORACLE_DIR` points at the unpacked release archive of the baseline tag. The game is selected with a switch such as `-FO4` or `-SSE`. Masters are read from the directory of the input file, and `-D:<Data path>` is needed for every plugin but the game master: without it the oracle loads the hardcoded records, cannot find the game master again and stops with `EOSError: System Error. Code: 2`.

```
"$XEDIT_ORACLE_DIR/xDump.exe" -FO4 -q "-D:$XEDIT_FO4_DATA" "$XEDIT_FO4_DATA\<plugin>" > dump.txt 2> dump.log
"$XEDIT_ORACLE_DIR/xDump.exe" -SSE -q "-D:$XEDIT_SSE_DATA" "$XEDIT_SSE_DATA\<plugin>" > dump.txt 2> dump.log
```

Pass the plugin path with backslashes and a drive letter. The oracle does not find masters when the path uses forward slashes. The oracle exits with 0 after an exception; a complete run ends `dump.log` with `All Done.`.

The dump goes to stdout and progress goes to stderr. `xDump.exe -FO4 -?` lists the options. The plain dump prints `[S]: <summary>` after an element without value whose summary is not empty (`DumpSummary` is on by default), and the file header record itself has no name line: its elements start at column 0.

A whole-file dump of `Fallout4.esm` (`-FO4 -q`) wrote 12.4 GB in 1 h 53 min and then stopped with `Unexpected Error: <EAccessViolation ...>` while dumping INFO `[000673E3]`, without `All Done.`. The crash is deterministic: `wbConditionAliasToStr` resolves the alias of an INFO condition through `INFO\Topic`, which Fallout 4 INFO records do not have, so the first FO4 INFO with an alias condition kills the dump (`DLCRobot.esm` at `[010102A3]`, `DLCworkshop03.esm` at `[010024C4]`, `DLCCoast.esm` at `[0013C634]`, `DLCNukaWorld.esm` likewise). The port prints an empty alias there. Treat the oracle output of a crashed run as valid up to the last complete record and compare only that prefix (`head -c $(stat -c %s oracle.txt) port.txt | cmp - oracle.txt`); the records after the crash need a plugin without such conditions. Skyrim SE files dump completely.

The `xEdit-llm` automation build is a secondary oracle for conflict, reference and cleaning results as JSON. It is a fork at a different upstream commit: use it to cross-check, never to close a gate.

## Inputs

- **Corpus:** vanilla master files from the local game installs, found through `XEDIT_FO4_DATA` and `XEDIT_SSE_DATA` (each is the game's `Data` directory). Never commit game files. Small synthetic plugins live in `tests/fixtures` and are committed.
- **Oracle output:** produced by the release binaries (xDump, xEdit tool modes, BSArch, Sniff) and cached outside the repository, keyed by release tag, game and input file hash.

## Checks

| Check | Oracle | Port |
| --- | --- | --- |
| Element dump | `xDump` text output | `xedit dump` |
| Round-trip save | input file bytes | load then save, compare bytes |
| Conflict status | xEdit conflict export | `xedit conflicts` |
| Cleaning | plugin saved by `-quickautoclean` | `xedit clean` |
| Scripts | plugin and log after `-script:` | `xedit script run` |
| Archives | BSArch pack output | `bsarch` pack output |

## Investigating a difference

1. Reduce to the smallest input that differs: one file, then one record, then one element.
2. Read the Pascal code that produces the oracle value. The oracle is right by definition, including its quirks. When the Pascal code does not explain the output, disassemble the oracle binary (`capstone` and `pefile` are installed for Python): the compiled overload resolution and RTL helpers are what actually ran.
3. Fix the port. Add the reduced case as a fixture test.
4. Never change expected output to match the port.

## Quirks found and reproduced

- **Float rounding.** `xDump.exe` 4.1.5q binds `IntPower(10, ADigit)` in `RoundToEx` to the `Single` overload, so a value is divided by and multiplied with the single-precision `10^-digits` in double precision, and 64-bit `FloatToDecimal` scales the result by a power of ten in double precision into an 18 digit mantissa before rounding to the decimals. `delphi.rs` reproduces both (`int_power_single`, `round_to_ex`, `float_to_decimal`). `make_float_probe.py <out.esp>` writes a plugin of 1800 GMST floats to probe the oracle (`xDump.exe -SSE -q -D:<dir> <out.esp>`), and `angle_probe.py <plugin> <oracle dump>` pairs the oracle's REFR rotation lines with the raw radians; both agree with the port. Upstream's `TwoPi` is `2 * Single(3.1415927)` (`TWO_PI` in `constructors.rs`). The rounding helpers were found by disassembling `xDump.exe` with `capstone`: `RoundToEx` loads `10.0` as a single before calling `IntPower`, and `Round` is a plain `cvtsd2si`.
- **Length-prefixed strings read past their data.** `TwbLenStringDef.ToStringInternal` limits the text to the length of the whole data, prefix included, so a stored length larger than the data reads up to the prefix's bytes past the end of the element, out of the bytes that follow it in the file. A Fallout New Vegas save's dialogue `Response Text` showed it. `len_string.rs` reads those bytes from the block of the data container (`bytes_after` in `element.rs`).
- **Save tables keep elements, not values.** `InitializeSaveWorldspaceArray` stores the elements of the `Visited Worldspace` chapter and reads their values when a worldspace index is shown. Reading the values while the save loads resolved the names of the worldspace records before their strings were loaded, and the empty names stayed cached: a Fallout 4 save showed `<SanctuaryHillsWorld>` without its name.
