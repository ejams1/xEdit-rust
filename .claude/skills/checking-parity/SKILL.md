---
name: checking-parity
description: Use when verifying that the Rust port behaves the same as Delphi xEdit, when a phase gate in docs/PLAN.md has to be closed, or when a parity difference has to be investigated.
---

# Checking parity

The Delphi build of the upstream commit in `upstream-map.toml` is the oracle. The port is correct when it produces the same output as the oracle on the same input.

## State of the harness

The harness is not built yet. Building the oracle needs Delphi 12, which is not installed on the development machine. Until both exist, this skill describes the contract the harness must meet. Update this section when that changes.

## Inputs

- **Corpus:** vanilla master files from the local game installs. Never commit game files. Small synthetic plugins live in `tests/fixtures` and are committed.
- **Oracle output:** produced by the Delphi tools (xDump, xEdit tool modes, BSArch, Sniff) and cached outside the repository, keyed by upstream commit and input file hash.

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
