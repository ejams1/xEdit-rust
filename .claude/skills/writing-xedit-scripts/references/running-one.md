## Running one

```
xedit script list [--scripts DIR]                       # script.list, read-only
xedit script check MyScript.pas [--json]                # script.check, read-only
xedit --edit --game sse --load "<Data>\Update.esm" \
      script run MyScript.pas [--scripts DIR] [--dry-run] [--no-save] [--build-refs]
```

- `script list` lists the `*.pas` of the scripts folder (the top folder only) with line counts, as the form's list shows them. The folder is `--scripts`, else `XEDIT_SCRIPTS`, else the oracle's `Edit Scripts` (`XEDIT_ORACLE_DIR`), else `Edit Scripts` beside `xedit.exe` — upstream's `wbScriptsPath`, which `-S:` sets.
- `script check <file>...` compiles each script and the units it `uses`, exactly as `TJvInterpreterUnit.Compile` accepts them: a routine body is only scanned for its balanced `end`, so a statement-level error inside a body surfaces when the script runs, not here. It exits 0 when all compile, else non-zero listing each failure as `file:line: message` (with `--json`, the structured result). A unit with no file beside the script is a unit compiled into the host and parses to nothing.
- `script run <file>` is xEdit's `-script:` mode: the script compiles, `Initialize` runs, `Process` runs per selected element, `Finalize` runs, then every plugin the run changed is saved (the GUI's `SaveChanged` on close): the port writes the file in place at once and logs the GUI's lines (`Saving: <name>.save.<timestamp>`, `Queued renaming of save ... on shutdown.`, `Done saving.`), without the GUI's backup folder.

A relative file name resolves against the scripts folder, but the units a script `uses` always resolve from the script's own folder, as `DoRunScript` sets `wbScriptsPath`.
- `--dry-run` runs the script with editing disabled and saves nothing; `--no-save` leaves the changed plugins in memory, for `xedit save` in the same `batch` or `serve` session.

A one-shot run with `--no-save` and no save afterwards loses the changes. `--build-refs` builds the reference information before the script runs, as the GUI's script mode does at its load (`-nobuildrefs` turns it off there): the scripts that read `ReferencedBy` need it; off by default.

Needs `--edit` unless `--dry-run` (registry gate, as every mutating command); `script list` and `script check` need no `--game` or `--load`.
- Exit status: `script check` exits non-zero when a script does not compile; `script run` exits non-zero when the script aborted.

With `--json` the run response has `file`, `script`, `dry_run`, `processed` (the `Process` calls that returned), `aborted`, `self_terminated`, `error`, `error_location`, `files` (changed/saved) and `messages` (the run log). `xedit schema` (or `call script.run --params '<json>'`) is the authority on fields; `save` is true by default on the registry request, so `--no-save` is `"save": false` through `call`/`batch`.
- The `-script:<file>` legacy switch and `xedit tool run script --script <file> [--scripts DIR]` run the same path and save as the auto modes do.
- A run needs no selection option: headless, `DoRunScript` selects every root node of the navigation tree, so `Process` sees every loaded file's elements from the last back to the file node (a file's records in the reverse of their order, the file node last).

The script narrows this in `Initialize`: `ScriptProcessElements` (default `[etMainRecord]`; `[etFile]` selects the file nodes) and `ScriptProcessSignatures` (a comma-separated list of 4-character signatures, upper-cased and matched as a substring).

A safe sequence, dry run first, then save somewhere else through `batch`:

```
xedit --game sse --load "<Data>\Update.esm" script run MyScript.pas --scripts <scripts> --dry-run
xedit --json --edit --game sse --load "<Data>\Update.esm" batch - <<'JSON'
[
  {"command": "script.run", "params": {"file": "MyScript.pas", "scripts": "<scripts>", "save": false}},
  {"command": "files.save", "params": {"file": "Update.esm", "output": "<scratch>\\Update.esm"}}
]
JSON
```
