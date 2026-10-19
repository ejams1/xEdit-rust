# Project sessions

## Launch from the assigned worktree

Open this repository as its own project in the primary agent's desktop or editor interface. Select the assigned worktree as the session's working directory. In a terminal:

```sh
cd <assigned-worktree>
claude
```

Use the parent projects directory only for cross-repository coordination. Keep the repositories in place. Confirm the working directory with `git rev-parse --show-toplevel` and `git status --short`; confirm loaded instructions and memory with `/memory` and `/context`. Read this repository's `CLAUDE.md`, the assigned plan section, and the relevant skill references. Do not import this workflow or its templates with `@` into the startup file.

Implementation sessions own one issue or phase step in the assigned step worktree. The phase coordinator keeps its own phase worktree. Name the owning crate, upstream unit, commands, and paths in the brief; coordinate changes to shared registries, the coverage ledger, and the upstream map.

Use the phase's assigned model and fresh independent review. A narrow reproduction diagnoses a difference; it does not replace the complete required corpus when closing a gate. Inspect ownership before reusing an existing phase or step worktree.

## Brief and ownership

Copy [the task brief](agent/task-brief.md) into an ignored or external task directory, then fill in one issue or batch, its branch/baseline, owned files, required references, checks, restrictions, and stopping condition. Keep machine paths and current task inventories local. Assign shared files before editing them; other sessions may be working nearby.

## Find filenames before reading contents

```sh
rg --files crates/xedit-session/src -g '*command*'
rg --files crates/xedit-core/src/implementation -g '*.rs'
rg -l -F 'elements.set' crates/xedit-session/src
```

Use the returned filename for a content search or a small Read window around the selected symbol and its callers. Query only the named entry in `coverage/ledger.toml` or `upstream-map.toml`. Generated definitions remain available through explicit paths; handwritten callbacks stay in ordinary searches.

For native tools, set Read's offset and limit and scope Grep/Glob by path or filename. Use filenames-only search before content. `rg --max-count` limits matches per file, not the whole search. `.rgignore` controls exploratory Bash `rg`, not native Read/Grep/Glob permissions or explicitly named files.

## Complete logs, bounded excerpts

Choose one absolute task directory on the configured project storage drive. Use distinct log names for each attempt so later commands do not overwrite evidence. Ignored worktree logs are acceptable only if the handoff preserves them before the worktree is removed.

Example in PowerShell, after the command has been authorized for this task:

```powershell
$sessionTaskDir = '<absolute-task-directory>'
New-Item -ItemType Directory -Path $sessionTaskDir -Force | Out-Null
cargo check -p xedit-session *> (Join-Path $sessionTaskDir 'build-01.log')
$sessionCommandExit = $LASTEXITCODE
python tools/log_excerpt.py (Join-Path $sessionTaskDir 'build-01.log') --max-lines 80 --max-chars 8000
Write-Output "Command exit status: $sessionCommandExit"
```

For a Bash shell, redirect both streams with `> "$task_dir/build-01.log" 2>&1` and save `$?` immediately. The excerpt reader's status does not replace the original command's status. Record the command and its status in the handoff.

The reader emits valid JSON, caps the whole response including escaping and its newline, and reads only a bounded byte window even for a huge single line. Its `offset` and `next_offset` are byte positions. Read another page only if it is relevant:

```sh
python tools/log_excerpt.py <log> --offset <next_offset> --max-lines 80 --max-chars 8000
```

If a search is verbose, write it to a task log and read it through the same helper. Return a short result, the relevant failure, and the evidence path. Do not dump full schemas, tables, build logs, or oracle outputs into the conversation.

Select an authorized parity mode and case through its skill reference before running it. Store complete oracle output in the task directory and report its verdict and first relevant difference. A failed, skipped, or interactive oracle case is not an equal result. Keep the official release baseline, expected outputs, and phase review requirements intact.

## Local plugins and worktrees

Personal plugin choices belong in ignored `.claude/settings.local.json`, not committed project settings or copied personal skill directories. Use an explicit false entry in `enabledPlugins` for an unrelated globally enabled plugin; preserve other settings and leave relevant project skills discoverable. Keep plugin identifiers and machine-specific paths local.

A local settings file exists only in its checkout. When launching a new worktree, either review and copy the relevant settings into its own local file, or pass the primary checkout's reviewed file explicitly:

```sh
claude --settings "<primary-checkout>/.claude/settings.local.json"
```

Do not overwrite an existing local settings file or assume a warm worktree inherited it. Inspect effective context after starting a fresh session. Global user hooks are separate from plugin enablement; a plugin override does not disable them. No synchronization service is required.

## Handoff and completion

Fill [the short handoff](agent/handoff.md) with the commit and pushed state, completed work, changed paths, validation and evidence, blockers, restrictions, and next action. Archive superseded checkpoints by link. Use `/compact` for the same task and `/clear` for unrelated work only after the handoff is saved. Resume from the recorded branch and current restrictions rather than old conversation text.
