---
name: checking-parity
description: Use when verifying that the Rust port behaves the same as Delphi xEdit, when a phase gate in docs/PLAN.md has to be closed, or when a parity difference has to be investigated.
---

# Checking parity

The official xEdit release build of the tag in `upstream-map.toml` is the oracle. The project has no Delphi compiler, so only released binaries can be the oracle. The port is correct when it produces the same output as the oracle on the same input.

## State of the harness

The harness is not built yet. Until it exists, run the oracle by hand and compare with `diff`. Update this section when that changes.

## Running the oracle

`XEDIT_ORACLE_DIR` points at the unpacked release (on the development machine: `M:\projectsíit-upstream-srcíit 4.1.5q`). The game is selected with a switch such as `-FO4` or `-SSE`. Masters are read from the directory of the input file.

```
"$XEDIT_ORACLE_DIR/xDump.exe" -FO4 -q "<game>/Data/<plugin>" > dump.txt 2> dump.log
```

The dump goes to stdout and progress goes to stderr. `xDump.exe -FO4 -?` lists the options.

The `xEdit-llm` automation build is a secondary oracle for conflict, reference and cleaning results as JSON. It is a fork at a different upstream commit: use it to cross-check, never to close a gate.

## Inputs

- **Corpus:** vanilla master files from the local game installs. Never commit game files. Small synthetic plugins live in `tests/fixtures` and are committed.
- **Oracle output:** produced by the release binaries (xDump, xEdit tool modes, BSArch, Sniff) and cached outside the repository, keyed by release tag and input file hash.

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
