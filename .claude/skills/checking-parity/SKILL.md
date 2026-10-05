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

Oracle output is cached as `<cache>/<tag>/<game>/<file>.<hash>.oracle.txt`, so the oracle runs once per input file. The oracle needs more than an hour for `Fallout4.esm`. `--oracle-only` fills the cache without running the port. The port output of the last run is next to the oracle output as `.port.txt`.

Only the dump check exists so far. Add the other checks of the table below to `crates/xtask/src/parity.rs` in the phase that ports the feature.

## Running the oracle

`XEDIT_ORACLE_DIR` points at the unpacked release archive of the baseline tag. The game is selected with a switch such as `-FO4` or `-SSE`. Masters are read from the directory of the input file.

```
"$XEDIT_ORACLE_DIR/xDump.exe" -FO4 -q "$XEDIT_FO4_DATA\<plugin>" > dump.txt 2> dump.log
"$XEDIT_ORACLE_DIR/xDump.exe" -SSE -q "$XEDIT_SSE_DATA\<plugin>" > dump.txt 2> dump.log
```

Pass the plugin path with backslashes and a drive letter. The oracle does not find masters when the path uses forward slashes. The oracle exits with 0 after an exception; a complete run ends `dump.log` with `All Done.`.

The dump goes to stdout and progress goes to stderr. `xDump.exe -FO4 -?` lists the options.

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
