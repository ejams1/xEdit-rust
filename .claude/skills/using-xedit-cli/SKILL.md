---
name: using-xedit-cli
description: Use when inspecting or saving Bethesda plugins and save games with the native xedit CLI of this repository: loading plugins of any game from Oblivion to Starfield, listing files and records, reading a record or an element, dumping a plugin or a save like xDump, saving a loaded plugin back to disk. No element editing yet.
---

# Using the xedit CLI

`xedit` is the command-line interface of the Rust port. Build it with `cargo build --release -p xedit-cli`; the binary is `target/release/xedit.exe`. Every subcommand is a command of the session registry, so `xedit schema` prints the full list with the JSON Schema of each request and response, and `xedit call <name> --params '<json>'` runs any of them by name.

## Loading plugins

The global options select the game and the plugins:

```
xedit --game sse --load "<Data>\Skyrim.esm" --load "<Data>\Update.esm" <command>
xedit --game fo4 --load "<Data>\DLCRobot.esm" <command>
```

- `--game` takes the switch of `xDump.exe` without the dash: `tes4` (Oblivion), `fo3`, `fnv`, `tes5` (Skyrim LE), `enderal`, `sse`, `tes5vr`, `enderalse`, `fo4`, `fo4vr`, `fo76`, `sf1` (Starfield). `tes3` (Morrowind) loads but is not verified, because the oracle cannot dump Morrowind plugins.
- `--load` takes the full path of a plugin and may repeat; the plugins load in the order given and each takes the next load order slot after its masters. Masters load from the same folder automatically, so loading `Update.esm` also loads `Skyrim.esm`.
- The hardcoded records of the game load as a file named after the game executable (`SkyrimSE.exe`, `Fallout4.exe`), like xEdit does.
- Strings of localized plugins come from the loose `Strings` folder or from the game archives next to the plugin (BSA, and BA2 including the Starfield versions).

Loading is per process: every invocation loads the plugins again. `Skyrim.esm` takes a few seconds and `Starfield.esm` or `SeventySix.esm` much longer; keep a shell loop or a script around one call rather than calling once per record when many records are needed, and prefer `records list --signature` and `records find` over reading records one by one.

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
| `saves dump --game G --data <Data> <save>` | A save or co-save as `xDump.exe -saves` prints it. The plugins the save lists load from `<Data>`. |
| `save [--file F] [--output PATH] [--dry-run] [--no-backup]` | Writes a loaded plugin as xEdit saves it (`files.save`). Needs the global `--edit` flag unless `--dry-run`. |

FormIDs are load order FormIDs in hexadecimal, as xEdit shows them: `01003274` is object `003274` of the file in slot 1. `--file` names a loaded plugin; without it, `records list` and `records find` need exactly one plugin in `--load`, and `records get` and `elements get` see the record from the last loaded plugin (the winning override for that plugin).

Element paths use `\` between names, as in xEdit scripts: `DATA\Health`, `ACBS\Flags\Female`, `Conditions\Condition #0\CTDA\Function`. Signatures (`DNAM`) and names (`DNAM - Flags`) both work for a step.

## Saving a plugin

`save` is the first mutating command. It runs `PrepareSave` and `WriteToStream` as upstream does: every unmodified record is copied as loaded, a modified record is rebuilt from its elements (and compressed again when its flag says so), and the file's CRC32 is computed on the result. The response reports `bytes`, `crc32`, `loaded_crc32`, `changed` and `written`.

- Mutating commands refuse to run without the global `--edit` flag (`edit_required`); with `--dry-run` they run without it and write nothing. In JSON, the same is `dry_run: true` in the request.
- `--output` writes somewhere else than the loaded path. Never point it into a game's `Data` folder while testing; the harness writes into the parity cache.
- Without `--output` the file is written over the loaded one, through a temporary file and a rename. An existing file first moves to `<AppName>Edit Backups\<name>.backup.<timestamp>` next to it (`--no-backup` skips that), and a save whose bytes did not change is dropped, as upstream removes it.
- Error codes: `save_refused` carries an upstream `PrepareSave` message, which the oracle gives for the same file (a Starfield blueprint module, a record in the wrong group, an `.esp` master where the game forbids it); `unsupported` names an edit upstream makes on save that this version cannot (the `HEDR` record count or `INCC` cell count off, `ONAM` entries in the header or needed for overridden placed records, a flag that does not follow the extension, a FormID beyond the masters). Those come with the element editing step of phase 3.
- Only the worldspace records are initialized by a save of an unmodified file (upstream drops their `OFST` subrecord and marks their children modified), so a big master saves in seconds; the children of its worldspaces are rebuilt record by record.

## Saves

`saves dump` reads the saves of `tes4` (`.ess`, OBSE `.obse`), `fo3` (`.fos`, FOSE `.fose`), `fnv` (`.fos`, NVSE `.nvse`), `fo4` (`.fos`, F4SE `.f4se`) and `tes5`, `sse`, `enderal`, `enderalse` (`.ess`, SKSE `.skse`); the co-save definitions are chosen by the extension. For Fallout 3 and Oblivion saves the oracle warns that they are not supported yet; it reads Fallout 3 saves all the same, and the port does both. Upstream's Oblivion save definitions expect the Fallout 3 header magic, so every real Oblivion save stops with `Expected header Magic FO3SAVEGAME, found TES4SAVEGAM`, in the oracle and in the port; only an OBSE co-save (`.obse`) would read. The dump follows the oracle exactly, including its quirks: the LZ4-compressed body of a Skyrim SE save is shown as raw bytes because the oracle's decompression fails, and a Skyrim LE save whose plugin list sits in the save content loads no plugins, so its hardcoded FormIDs show the oracle's access violation text.

## Output

Without `--json` a command prints its result as pretty JSON and an error as `error: <code>: <message>` on stderr with exit code 1. With `--json` it prints one envelope, `{"ok":true,"result":...}` or `{"ok":false,"error":{"code":"...","message":"..."}}`, always on stdout. Error codes are stable: `no_session` (no `--game`), `unknown_file`, `ambiguous_file`, `unknown_record`, `unknown_element`, `invalid_params`, `load_failed`, `unknown_command`, `edit_required`, `save_refused`, `unsupported`, `io`.

The progress log of a load and a save (`[Skyrim.esm] Loading file`, the string table encodings) goes to stderr, as xEdit's messages do.

A record or element node has `name`, `display_name` (only when it differs, for example a placed object with its base record), `value` (the text xEdit shows), `summary` (the `[S]:` text of containers without a value), `native` (number, boolean, string or bytes as hexadecimal) and `children`. `--depth 1` keeps the first level of children, which is enough to list the subrecords of a record.

## Examples

```
xedit --game sse --load "<Data>\Update.esm" records find --editor-id Dawnguard --signature MESG
xedit --game sse --load "<Data>\Update.esm" elements get 01003274 DNAM
xedit --game fo4 --load "<Data>\DLCRobot.esm" records list --signature NPC_ --limit 20
xedit dump --game fo4 "<Data>\DLCworkshop01.esm" > DLCworkshop01.txt
xedit saves dump --game fo4 --data "<Data>" "<My Games>\Fallout4\Saves\Autosave1.fos" > save.txt
xedit --json --game sse --load "<Data>\Skyrim.esm" save --dry-run
xedit --json --edit --game sse --load "<Data>\Skyrim.esm" save --no-backup --output "<somewhere else>\Skyrim.esm"
```

## Limits of this version

- No element editing, no masters or FormID changes; `save` writes a loaded file back and reports `unsupported` where upstream would edit its header on save.
- Morrowind plugins are not verified. Oblivion saves do not read (an upstream limit, see above).
- One session per process; `xedit serve` comes with the write path.
