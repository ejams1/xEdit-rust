---
name: writing-xedit-scripts
description: Write or run Pascal scripts when Bethesda plugin work needs data-dependent iteration beyond the native xedit batch commands.
---

# Writing xEdit scripts

The interpreter is `crates/xedit-script`; the host is `crates/xedit-session/src/script.rs`. Use `xedit batch` for a known sequence of typed commands and a script for conditional walks, recursion, or state across records. Read [script or batch](references/script-or-xedit-batch.md) when choosing.

1. Read [running one](references/running-one.md) before running a script, and [the dialect](references/the-dialect.md) before writing one. `script check` scans some routine bodies only for balanced `end`; a successful check does not establish runtime correctness.
2. Read the declarations needed from `xEditAPI.pas` and the corresponding entry in `coverage/ledger.toml`; see [where the API lives](references/where-the-api-lives.md). Retrieve only the named members.
3. Start with `--dry-run` on private copies. A script mutation needs `--edit`; `script run` saves changed plugins in place by default, without the normal GUI backup folder.
4. To control saving, run `script.run` with `save: false` in a batch or persistent session, then explicitly save to a scratch output. A one-shot `--no-save` run loses its edits. Reload saved output and inspect it.
5. Read [the headless VCL seam](references/headless-and-the-vcl-seam.md) before using forms or dialogs. Headless execution does not imply that every interactive script can finish unattended.

Select the additional reference that matches the script. Do not load all the parity history or corpus examples for an ordinary edit.

- [Script or `xedit batch`?](references/script-or-xedit-batch.md)
- [Running one](references/running-one.md)
- [Where the API lives](references/where-the-api-lives.md)
- [The dialect](references/the-dialect.md)
- [Headless and the Vcl seam](references/headless-and-the-vcl-seam.md)
- [Output conventions](references/output-conventions.md)
- [Upstream quirks a script meets](references/upstream-quirks-a-script-meets.md)
- [Known divergences](references/known-divergences.md)
- [Corpus scripts to copy from](references/corpus-scripts-to-copy-from.md)

- [Background and compatibility scope](references/background.md)
