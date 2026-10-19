## Mutation rules

Read these before changing anything. They hold for the CLI, `batch`, `serve` and `mcp` alike.

1. **Nothing is written without `--edit`.** Commands that change plugin data or files (`mutates` in `xedit schema`) fail with `edit_required` unless the global `--edit` flag is given or the request has `dry_run: true` (CLI: `--dry-run`). The gate is checked before the parameters are parsed, and a daemon fixes it at startup: no request can lift it.
2. **Run `--dry-run` first.** A dry run does the lookups and the checks the real call does (editability, removability, masters, FormID collisions) and reports what the command would do; it changes nothing, even with `--edit`, and needs no `--edit`. Read its report, then repeat the command without `--dry-run`. A dry run is a report, not a rehearsal: an error the real call raises while changing the data can still appear only then.
3. **Edits live in memory.** An invocation of a one-shot subcommand loads the plugins, runs one command and exits, so the edit is gone. To keep it, save in the same session: `batch` with the edits and a `files.save`, or `serve`/`mcp` where the session stays between calls. Nothing reaches disk until `files.save` (CLI: `save`) runs.
4. **Save explicitly, and never into a game's `Data` folder while testing.** `save --output <path>` writes somewhere else than the loaded path. Without `--output` the plugin is written over the loaded file through a temporary file and a rename, the old file first moves to `<AppName>Edit Backups\<name>.backup.<timestamp>` next to it (`--no-backup` skips that), and a save whose bytes equal the loaded file is not written.
5. **Read the result back after the save.** The response of the edit shows the element before and after; the response of the save shows `crc32`, `loaded_crc32`, `changed` and `written`.

Then load the saved file in a fresh process and check it: `records get`/`elements get` for the changed values, `files list` for the masters, and `save --dry-run` on it, which normally reports `changed: false` (a saved file saves to itself).

Masters load from the folder of the plugin, so a file saved with `--output` somewhere else needs its masters beside it to load again.
6. **One edit session per process.** The plugins and the edit gate are fixed at startup. To load other plugins, start another process.
7. **Know the stable error codes** ([output](output.md)) and match on `code`, not on the message.

A safe sequence for a scripted edit:

```
cat > edit.json <<'EOF'
[
  {"command": "elements.set", "params": {"form_id": "01003274", "path": "DNAM\\Flags", "value": "0000000000000001", "dry_run": true}},
  {"command": "elements.set", "params": {"form_id": "01003274", "path": "DNAM\\Flags", "value": "0000000000000001"}},
  {"command": "files.save", "params": {"output": "<scratch>\\Update.esm"}}
]
EOF
xedit --json --edit --game sse --load "<Data>\Update.esm" batch edit.json
xedit --json --game sse --load "<scratch>\Update.esm" elements get 01003274 "DNAM\Flags"
```

(`<scratch>` needs the masters of `Update.esm` beside it for the second command.)
