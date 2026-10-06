---
name: checking-parity
description: Use when verifying that the Rust port behaves the same as Delphi xEdit, when a phase gate in docs/PLAN.md has to be closed, or when a parity difference has to be investigated.
---

# Checking parity

The official xEdit release build of the tag in `upstream-map.toml` is the oracle. The project has no Delphi compiler, so only released binaries can be the oracle. The port is correct when it produces the same output as the oracle on the same input.

## Running the harness

```
cargo xtask parity dump [--game fo4|sse]... [--file <name>]... [--oracle-only] [--jobs <n>]
```

The harness runs the oracle `xDump.exe` and the port's `xedit dump` on every vanilla plugin of the selected games and compares the outputs byte for byte. Without `--game` it checks all games. `--file` restricts the run to named plugins, vanilla or not. It prints the first differing line of each file, writes `target/parity/dump.json` and exits with an error when any file differs.

Environment variables:

| Variable | Content |
| --- | --- |
| `XEDIT_ORACLE_DIR` | Unpacked release archive of the baseline tag. |
| `XEDIT_FO4_DATA`, `XEDIT_SSE_DATA` | `Data` directory of each game. |
| `XEDIT_PARITY_CACHE` | Cache directory. Optional; defaults to the user cache directory. |

Oracle output is cached as `<cache>/<tag>/<game>/<file>.<hash>.oracle.txt`, so the oracle runs once per input file. The oracle is slow on large masters: it writes about 80 MB of dump per minute and needs more than ten minutes for `Fallout4.esm`. `--oracle-only` fills the cache without running the port. The port output of the last run is next to the oracle output as `.port.txt`.

Only the dump check exists so far. Add the other checks of the table below to `crates/xtask/src/parity.rs` in the phase that ports the feature.

## Running the oracle

`XEDIT_ORACLE_DIR` points at the unpacked release archive of the baseline tag. The game is selected with a switch such as `-FO4` or `-SSE`. Masters are read from the directory of the input file, and `-D:<Data path>` is needed for every plugin but the game master: without it the oracle loads the hardcoded records, cannot find the game master again and stops with `EOSError: System Error. Code: 2`.

```
"$XEDIT_ORACLE_DIR/xDump.exe" -FO4 -q "-D:$XEDIT_FO4_DATA" "$XEDIT_FO4_DATA\<plugin>" > dump.txt 2> dump.log
"$XEDIT_ORACLE_DIR/xDump.exe" -SSE -q "-D:$XEDIT_SSE_DATA" "$XEDIT_SSE_DATA\<plugin>" > dump.txt 2> dump.log
```

Pass the plugin path with backslashes and a drive letter. The oracle does not find masters when the path uses forward slashes. The oracle exits with 0 after an exception; a complete run ends `dump.log` with `All Done.`.

The dump goes to stdout and progress goes to stderr. `xDump.exe -FO4 -?` lists the options. The plain dump prints `[S]: <summary>` after an element without value whose summary is not empty (`DumpSummary` is on by default), and the file header record itself has no name line: its elements start at column 0.

A whole-file dump of `Fallout4.esm` (`-FO4 -q`) wrote 12.4 GB in 1 h 53 min and then stopped with `Unexpected Error: <EAccessViolation ...>` while dumping INFO `[000673E3]`, without `All Done.`. Treat the oracle output of a crashed run as valid up to the last complete record and compare only that prefix; the records after the crash need a dump that starts past them, or a smaller plugin.

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
2. Read the Pascal code that produces the oracle value. The oracle is right by definition, including its quirks.
3. Fix the port. Add the reduced case as a fixture test.
4. Never change expected output to match the port.

## Known open differences

- **Single floats at six decimals (`xDump.exe` 4.1.5q, 64-bit).** The oracle prints `RoundToEx(Value, -6)` formatted with `FloatToStrF(ffFixed, 99, 6)`. For a few percent of values whose seventh decimal is near 5 (navmesh vertices, `Approx Location`, bounds) the oracle's last digit is one off from correct rounding of the single value, in both directions, deterministic per value (for example `14043.0400390625` prints `14043.040040`, `9823.9248046875` prints `9823.924804`, exact ties come out odd or even by value). No decimal or double-arithmetic model tried so far reproduces it (see the scratch scripts in the session that found it: constant-factor division, 15 to 20 significant digits with every rounding mode, digit-by-digit extraction). The port prints the correctly rounded value. `make_float_probe.py <out.esp>` in this directory writes a plugin of GMST floats (300 values with few mantissa bits near ties, 1500 random) to probe the oracle: `xDump.exe -SSE -q -D:<dir> <out.esp>`. On that probe 291 of 1800 values differ, all among values whose seventh decimal is near 5; the oracle's result is the neighbour on the far side of the correct rounding (`n+1` when the fraction is below a half, `n` when above, either on a tie), in a way that depends on the whole value and not on the fraction alone (`451.0703125` prints `.070313`, `3515.0703125` prints `.070312`). Double and 80-bit division by `IntPower(10, -6)`, every neighbour of `1e-6`, multiplication by `1e6`, and digit extraction with 15 to 18 digits all score the same as correct rounding. Revisit with the Delphi 64-bit RTL sources for `Round`, `IntPower` and `FloatToDecimal`.
