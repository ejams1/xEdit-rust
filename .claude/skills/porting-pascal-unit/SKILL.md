---
name: porting-pascal-unit
description: Use when porting an upstream xEdit Pascal unit (or part of one) to Rust in this repository, or when merging an upstream change into an already ported unit. Covers file placement, naming, the license header, the upstream map entry, the session command and the parity test.
---

# Porting a Pascal unit

The upstream source is a checkout of https://github.com/TES5Edit/TES5Edit at the commit in `upstream-map.toml`. Read `docs/PLAN.md` for the crate layout and the design rules.

## Procedure

1. Find the unit in `upstream-map.toml`. If its status is `replaced` or `not-ported`, stop and read the note.
2. Read the whole Pascal unit, and the units it uses for anything whose behaviour you depend on. Do not port from memory of how xEdit behaves.
3. Choose the crate from the table in `docs/PLAN.md`. Create the crate when it does not exist yet, with `edition.workspace = true` and `license.workspace = true`.
4. Create one Rust module per unit. Drop the `wb`/`xe` prefix and use snake case: `Core/wbLoadOrder.pas` becomes `crates/xedit-loadorder/src/load_order.rs`.
5. Start the file with the MPL-2.0 notice from `NOTICE`, then `// Ported from xEdit: <unit path>`.
6. Keep upstream names, converted to Rust case and without the `wb`/`xe` prefix: `wbIsPlugin` becomes `is_plugin`. The definition builder API is the exception and keeps the prefix (`wbStruct` becomes `wb_struct`), so that definition files stay line-for-line comparable. Keep the order of declarations the same as upstream so that an upstream diff maps onto the Rust file.
7. Port behaviour exactly, including quirks. When upstream behaviour looks wrong, port it as it is and add a comment that starts with `UPSTREAM-QUIRK:`. Parity comes before correctness.
8. Do not port Delphi plumbing: reference counting, interface GUIDs, memory manager calls, VCL message pumping.
9. When the unit adds an operation a user can run, register a command in `xedit-session` and follow the `adding-a-command` skill. Set the matching entries in `coverage/ledger.toml` to `covered` with the command name.
10. Add tests. Use unit tests for pure logic and a parity test (see the `checking-parity` skill) for anything that reads or writes game files.
11. Set the unit in `upstream-map.toml` to `ported` with its `rust` path. For a partial port keep `pending` and say in `note` what is missing.
12. Run `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `cargo xtask check`.

## Rules

- Never copy code from a GPL project. Dependencies must pass `cargo deny check licenses`.
- No `unsafe` without a `// SAFETY:` comment. SIMD code needs a scalar fallback with identical output.
- Output must not depend on thread count or CPU features.
