---
name: using-xedit-cli
description: Inspect, edit, and save Bethesda plugins or saves with this repository's native xedit CLI, batch, serve, or MCP commands.
---

# Using the xedit CLI

Build with `cargo build --release -p xedit-cli`; the binary is `target/release/xedit.exe`. Use the CLI's subcommand help for the operation at hand. `xedit schema` emits the complete registry and can be large: capture it to a scratch file and extract the required command rather than returning the whole catalogue.

## Editing safeguards

- Read [mutation rules](references/mutation-rules.md) before a mutation, including its dry run and save example.
- Mutations require `--edit` or `dry_run: true`. Run the dry run and inspect its report first; it does not prove that a subsequent write will succeed.
- Ordinary one-shot edits disappear when the process exits. Edit and explicitly save in one `batch`, `serve`, or `mcp` session. The edit gate and loaded plugins are fixed at startup.
- Scripts are an exception: `script run` saves changed plugins by default. Use `--no-save` or `save: false` when a later explicit save should control the destination.
- Test against copies outside a game's `Data` directory. Save to an explicit output, reload it in a fresh process, and check the changed values, masters, and save result.
- Match errors by stable `code`, not message text. Keep stderr load logs out of the JSON response.

## Read only what the operation needs

Read [loading plugins](references/loading-plugins.md) for game, load-order, archive, or string-table options. Start with a named record or signature and a small `--limit`; use `--offset` where the command supports it. The defaults of every command are not bounded. Scope the native Grep/Glob tools to the relevant crate or use the repository's `.rgignore` with Bash `rg` for exploratory searches.

Read the reference for the selected operation below. References are independent; do not load the whole directory.

- [Mutation rules](references/mutation-rules.md)
- [Loading plugins](references/loading-plugins.md)
- [Commands](references/commands.md)
- [Archives](references/archives.md)
- [Assets](references/assets.md)
- [Sniff](references/sniff.md)
- [Scripts](references/scripts.md)
- [LOD generation](references/lod-generation.md)
- [Conflicts](references/conflicts.md)
- [Filtering the navigation tree](references/filtering-the-navigation-tree.md)
- [Mod groups](references/mod-groups.md)
- [Editing an element](references/editing-an-element.md)
- [Adding, removing and copying](references/adding-removing-and-copying.md)
- [FormIDs and module flags](references/formids-and-module-flags.md)
- [References](references/references.md)
- [Masters](references/masters.md)
- [Cleaning](references/cleaning.md)
- [Merged patch](references/merged-patch.md)
- [Localization](references/localization.md)
- [Checking for errors](references/checking-for-errors.md)
- [Saving a plugin](references/saving-a-plugin.md)
- [Tool modes](references/tool-modes.md)
- [Daemon and MCP server](references/daemon-and-mcp-server.md)
- [Saves](references/saves.md)
- [Output](references/output.md)
- [Examples](references/examples.md)
- [Known gaps](references/known-gaps.md)

- [Background and compatibility scope](references/background.md)
