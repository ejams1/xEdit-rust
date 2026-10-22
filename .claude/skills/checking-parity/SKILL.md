---
name: checking-parity
description: Verify xEdit Rust against the official Delphi release, investigate an output difference, or close a phase's parity gate.
---

# Checking parity

The oracle is the official release of the tag in `upstream-map.toml`; the project has no Delphi compiler. A fork or a newer source checkout cannot close the gate for that release.

1. Read [running the harness](references/running-the-harness.md) and the result reference for the affected subsystem. Set the oracle and game paths from the local environment; put caches and scratch output outside tracked source.
2. For a diagnosis, select a game, file, record, or case using the mode's supported flags. Start with `cargo xtask parity <mode> --help`; do not run every mode to answer one local question.
3. Preserve inputs and expected outputs. Capture full output to a scratch file and report the verdict, first relevant difference, command, and evidence path.
4. Follow [investigating a difference](references/investigating-a-difference.md). A skipped, interactive, or failed oracle case is not an equal result. Distinguish known differences from newly observed differences.
5. At a phase gate, run the full required corpus. A focused diagnosis does not replace the gate or its independent review in `docs/PLAN.md`.

Read [running the oracle](references/running-the-oracle.md) before invoking a GUI oracle or handling its process. Read [checks](references/checks.md) for comparison rules and [inputs](references/inputs.md) for corpus constraints. Historical results explain behavior and limits; rerun the relevant check before claiming that a current commit passes.

## References by subsystem

- [Running the harness](references/running-the-harness.md)
- [Close-out results (2026-10-18)](references/close-out-summary.md)
- [Results: Memory](references/results-memory.md)
- [Results: Round trip](references/results-round-trip.md)
- [Results: Oracle save](references/results-oracle-save.md)
- [Results: Oracle edit sequences](references/results-oracle-edit-sequences.md)
- [Results: Resources](references/results-resources.md)
- [Results: Scripts](references/results-scripts.md)
- [Results: Cleaning](references/results-cleaning.md)
- [Results: Tool modes](references/results-tool-modes.md)
- [Results: Reference index](references/results-reference-index.md)
- [Results: NIF and materials](references/results-nif-and-materials.md)
- [Results: Sniff operations](references/results-sniff-operations.md)
- [Results: LOD generation](references/results-lod-generation.md)
- [Results: Conflict status](references/results-conflict-status.md)
- [Results: Mod groups](references/results-mod-groups.md)
- [Results: Merged patch](references/results-merged-patch.md)
- [Results: Filter](references/results-filter.md)
- [Results: Check for errors](references/results-check-for-errors.md)
- [Running the oracle](references/running-the-oracle.md)
- [Inputs](references/inputs.md)
- [Checks](references/checks.md)
- [Investigating a difference](references/investigating-a-difference.md)
- [Quirks found and reproduced](references/quirks-found-and-reproduced.md)
- [Screen checks](references/screen-checks.md)

- [Background and compatibility scope](references/background.md)
