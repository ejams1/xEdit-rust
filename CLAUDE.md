# xEdit Rust agent instructions

## Start with the assigned work

- Start the session in this repository or its task worktree. Read the design rules and the assigned phase in `docs/PLAN.md`, then the relevant skill under `.claude/skills/`. Use `README.md` for merged progress; in-progress branches can be ahead of it.
- Follow the phase's existing model assignment and fresh review requirement. A task brief should name the branch, crate or files, gate, and stopping point. Do not read every completed phase or parity result before starting.
- Keep the source shaped like upstream. Record the origin and last merged upstream commit in `upstream-map.toml`; keep license headers, quirks, and `SAFETY` comments. Do not hide an output difference by changing the oracle fixtures.
- Every operation belongs in the typed `xedit-session` command layer with its request, response, schema, CLI, serve, and MCP coverage. Use the `adding-a-command` skill when adding one.
- The released binaries of the baseline tag are the parity oracle. A focused reproduction helps diagnosis; full required corpus evidence and the plan's independent review close a phase gate.

## Keep context bounded

- Use Read with an offset and limit. Scope Grep/Glob to the relevant crate, filename, signature, command, or upstream unit. Use filenames first, then a small code window. Bash `rg` uses `.rgignore`; native tools still need explicit paths or globs.
- Generated game definitions, VCL tables, the coverage ledger, and the upstream map are query inputs. Retrieve the selected definition or entry explicitly. Do not dump these tables or `xedit schema` into the conversation.
- `crates/xedit-defs/src/<game>.rs` and `callbacks/*_stubs.rs` are generated. Change the transpiler or handwritten callbacks and regenerate; do not exclude handwritten callbacks from an investigation.
- Skill references load only for the selected operation. Use ordinary Markdown links; importing references with `@` would load them at startup.
- Capture full build and oracle logs in a task scratch directory. Return the command, outcome, first relevant failure, and evidence path. A line limit alone is insufficient for long JSON or historical paragraphs.
- At a batch boundary, leave a short handoff with the branch and commit, changed files, validation, blockers, and next action. Preserve dated evidence in a linked topic or history file.

## Validation and docs

Run checks appropriate to the changed crate and the phase gate; preserve required upstream and parity checks. Update the affected skill and `README.md` progress when functionality or phase status changes. Keep `docs/PLAN.md` an overview: detailed run evidence belongs in `docs/history/` or the issue that owns it.


## Session entry and ownership

Use [the session workflow](docs/AGENT-SESSIONS.md) when starting or handing off a task. Copy the [brief](docs/agent/task-brief.md) and [handoff](docs/agent/handoff.md) templates into the task's ignored or external scratch directory; fill only the fields needed for the assignment. One session owns one issue or batch and its named paths. Expand shared-file ownership with the coordinator before editing; do not revert another session's work.

Keep complete command logs and their exit statuses in the task directory. Read them with `python tools/log_excerpt.py LOG --max-lines 80 --max-chars 8000`; continue from the returned byte `next_offset` only when needed. The helper bounds its entire JSON response and does not execute commands. Keep detailed histories and machine-specific launch settings out of the startup instructions.
