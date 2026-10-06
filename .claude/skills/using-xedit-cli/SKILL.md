---
name: using-xedit-cli
description: Use when inspecting Bethesda plugins (Skyrim SE, Fallout 4) with the native xedit CLI of this repository: loading plugins, listing files and records, reading a record or an element, dumping a plugin like xDump. Read-only in this version; there are no editing commands yet.
---

# Using the xedit CLI

`xedit` is the command-line interface of the Rust port. Build it with `cargo build --release -p xedit-cli`; the binary is `target/release/xedit.exe`. Every subcommand is a command of the session registry, so `xedit schema` prints the full list with the JSON Schema of each request and response, and `xedit call <name> --params '<json>'` runs any of them by name.

## Loading plugins

The global options select the game and the plugins:

```
xedit --game sse --load "<Data>\Skyrim.esm" --load "<Data>\Update.esm" <command>
xedit --game fo4 --load "<Data>\DLCRobot.esm" <command>
```

- `--game` is `sse` (Skyrim Special Edition), `fo4` (Fallout 4) or `tes5` (Skyrim LE).
- `--load` takes the full path of a plugin and may repeat; the plugins load in the order given and each takes the next load order slot after its masters. Masters load from the same folder automatically, so loading `Update.esm` also loads `Skyrim.esm`.
- The hardcoded records of the game load as a file named after the game executable (`SkyrimSE.exe`, `Fallout4.exe`), like xEdit does.
- Strings of localized plugins come from the loose `Strings` folder or from the game archives next to the plugin.

Loading is per process: every invocation loads the plugins again. `Skyrim.esm` takes a few seconds; keep a shell loop or a script around one call rather than calling once per record when many records are needed, and prefer `records list --signature` and `records find` over reading records one by one.

## Commands

| Command | What it prints |
|---|---|
| `session info` | Game tag, game name, data folder and the loaded files in load order. |
| `files list` | Every loaded file with load order, masters, record count, ESM and localized flags. |
| `records list [--file F] [--signature SIG] [--offset N] [--limit N]` | Records of one plugin in file order; `total` counts the matches before paging. |
| `records find [--file F] [--signature SIG] [--editor-id TEXT] [--name TEXT] [--limit N]` | Records whose editor ID or display name contains the text, compared without case. |
| `records get <FormID> [--file F] [--depth N]` | One record with its elements as a tree. |
| `elements get <FormID> <path> [--file F] [--depth N]` | One element of a record by path. |
| `dump --game G <plugin>` | The whole plugin as `xDump.exe` prints it, to stdout; progress goes to stderr. |

FormIDs are load order FormIDs in hexadecimal, as xEdit shows them: `01003274` is object `003274` of the file in slot 1. `--file` names a loaded plugin; without it, `records list` and `records find` need exactly one plugin in `--load`, and `records get` and `elements get` see the record from the last loaded plugin (the winning override for that plugin).

Element paths use `\` between names, as in xEdit scripts: `DATA\Health`, `ACBS\Flags\Female`, `Conditions\Condition #0\CTDA\Function`. Signatures (`DNAM`) and names (`DNAM - Flags`) both work for a step.

## Output

Without `--json` a command prints its result as pretty JSON and an error as `error: <code>: <message>` on stderr with exit code 1. With `--json` it prints one envelope, `{"ok":true,"result":...}` or `{"ok":false,"error":{"code":"...","message":"..."}}`, always on stdout. Error codes are stable: `no_session` (no `--game`), `unknown_file`, `ambiguous_file`, `unknown_record`, `unknown_element`, `invalid_params`, `load_failed`, `unknown_command`.

A record or element node has `name`, `display_name` (only when it differs, for example a placed object with its base record), `value` (the text xEdit shows), `summary` (the `[S]:` text of containers without a value), `native` (number, boolean, string or bytes as hexadecimal) and `children`. `--depth 1` keeps the first level of children, which is enough to list the subrecords of a record.

## Examples

```
xedit --game sse --load "<Data>\Update.esm" records find --editor-id Dawnguard --signature MESG
xedit --game sse --load "<Data>\Update.esm" elements get 01003274 DNAM
xedit --game fo4 --load "<Data>\DLCRobot.esm" records list --signature NPC_ --limit 20
xedit dump --game fo4 "<Data>\DLCworkshop01.esm" > DLCworkshop01.txt
```

## Limits of this version

- Read-only: no element editing, no saving, no masters or FormID changes.
- Games: Skyrim SE, Skyrim LE and Fallout 4.
- One session per process; `xedit serve` comes with the write path.
