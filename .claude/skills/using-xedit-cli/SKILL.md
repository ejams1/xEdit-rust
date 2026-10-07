---
name: using-xedit-cli
description: Use when inspecting, editing or saving Bethesda plugins and save games with the native xedit CLI of this repository: loading plugins of any game from Oblivion to Starfield, listing files and records, reading a record or an element, setting the value of an element, adding, sorting and cleaning the masters of a plugin, dumping a plugin or a save like xDump, saving a loaded plugin back to disk, running several commands in one session, keeping a session loaded behind a JSON-RPC daemon (`xedit serve`) or an MCP server (`xedit mcp`).
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
| `elements set <FormID> <path> [<value>] [--file F] [--native] [--default] [--dry-run]` | Sets the value of one element (`elements.set`): the edit value as xEdit shows it in its editor, a native number or boolean with `--native`, or the default of the definition with `--default`. A missing last element of the path is added when the record's definition has it. Needs `--edit` unless `--dry-run`. |
| `save [--file F] [--output PATH] [--dry-run] [--no-backup]` | Writes a loaded plugin as xEdit saves it (`files.save`). Needs the global `--edit` flag unless `--dry-run`. |
| `masters add <name>... [--file F] [--no-sort] [--dry-run]` | Adds loaded plugins as masters (`masters.add`, `AddMastersIfMissing`) and sorts the masters by load order unless `--no-sort`. Needs `--edit` unless `--dry-run`. |
| `masters sort [--file F] [--dry-run]` | Sorts the masters by load order (`masters.sort`, `SortMasters`). Needs `--edit` unless `--dry-run`. |
| `masters clean [--file F] [--dry-run]` | Removes the masters no FormID of the plugin points to (`masters.clean`, `CleanMasters`). Needs `--edit` unless `--dry-run`. |
| `batch <file.json or ->` [--keep-going] | Runs a JSON array of `{"command": name, "params": {...}}` in one session and prints one envelope per command; stops at the first failure unless `--keep-going`. |
| `serve [--pipe NAME]` | Keeps the session loaded and answers JSON-RPC requests, one per line, on stdio or a named pipe. See "Daemon and MCP server". |
| `mcp` | Serves the commands as MCP tools on stdio. |

FormIDs are load order FormIDs in hexadecimal, as xEdit shows them: `01003274` is object `003274` of the file in slot 1. `--file` names a loaded plugin; without it, `records list` and `records find` need exactly one plugin in `--load`, and `records get` and `elements get` see the record from the last loaded plugin (the winning override for that plugin).

Element paths use `\` between names, as in xEdit scripts: `DATA\Health`, `ACBS\Flags\Female`, `Conditions\Condition #0\CTDA\Function`. Signatures (`DNAM`) and names (`DNAM - Flags`) both work for a step.

## Editing an element

`elements set` runs `SetEditValue`, `SetNativeValue` or `SetToDefault` on the element at the path, as a script's `SetEditValue` does, so the `AfterSet` callbacks of the definition run (a GMST's `DATA` is rebuilt when the first letter of its editor ID changes, a MGEF's actor values follow its archetype, and so on). The response holds the element before (`old`) and after (`new`), `changed`, and `added` when the element was created.

- Edit values are the texts xEdit shows in its editor: `1.5` for a float, `Dawnguard "Dawnguard" [QUST:0200C97A]` or `0200C97A` for a FormID (load order FormIDs, as `--game` loads the plugins), a name or a number for an enum, `0000000000000001` for flags. A flag can also be set by its name as the last element of the path: `ACBS\Flags\Female` with `1` or `0`.
- `--native` takes a JSON number or boolean and sets it as the native value; `--default` sets the element to the default of its definition.
- A path whose last element is missing adds that member when the record's definition has it (`ElementEditValues`): `elements set 01003274 SNAM "text"` on a record without `SNAM`.
- Loading runs the fix-ups xEdit runs on load (`wbAllowInternalEdit`): a record that lacks a required subrecord gets it, a worldspace loses its offset data, and the `AfterLoad` callbacks of the definitions apply their corrections. Those records are modified internally and are written from their elements on save.
- The edit is in memory only. To keep it, save in the same process: use `batch` with an `elements.set` and a `files.save`, or send the commands to `xedit serve` or `xedit mcp`, which keep the session between calls.
- Error codes: `edit_failed` carries the message of the upstream exception (`"x" is not a valid character for a flag`, `Not a valid GUID: ...`, `FormID [...] can not be mapped to file FormID for file "..."`), `not_editable` a dry run on an element the editor would not let you change.

```
cat > edit.json <<'EOF'
[
  {"command": "elements.set", "params": {"form_id": "01003274", "path": "DNAM\\Flags", "value": "0000000000000001"}},
  {"command": "files.save", "params": {"backup": true}}
]
EOF
xedit --json --edit --game sse --load "<Data>\Update.esm" batch edit.json
```

## Saving a plugin

`save` runs `PrepareSave` and `WriteToStream` as upstream does: every unmodified record is copied as loaded, a modified record is rebuilt from its elements (and compressed again when its flag says so), and the file's CRC32 is computed on the result. The response reports `bytes`, `crc32`, `loaded_crc32`, `changed` and `written`.

- Mutating commands refuse to run without the global `--edit` flag (`edit_required`); with `--dry-run` they run without it and write nothing. In JSON, the same is `dry_run: true` in the request.
- `--output` writes somewhere else than the loaded path. Never point it into a game's `Data` folder while testing; the harness writes into the parity cache.
- Without `--output` the file is written over the loaded one, through a temporary file and a rename. An existing file first moves to `<AppName>Edit Backups\<name>.backup.<timestamp>` next to it (`--no-backup` skips that), and a save whose bytes did not change is dropped, as upstream removes it.
- The save edits the file header as upstream does: the ESM flag follows an `.esm` extension (ESM and Light an `.esl`), `HEDR` gets the record count, `INCC` the interior cell count, the `ONAM` list of a master is rebuilt from its overridden temporary placed records, and a FormID beyond the masters is clamped. The `unsupported` code is gone.
- Error codes: `save_refused` carries an upstream `PrepareSave` message, which the oracle gives for the same file (a Starfield blueprint module, a record in the wrong group, an `.esp` master where the game forbids it).
- Only the worldspace records are initialized by a save of an unmodified file (upstream drops their `OFST` subrecord and marks their children modified), so a big master saves in seconds; the children of its worldspaces are rebuilt record by record.

## Masters

The master commands change the master list of one plugin as the navigation menu of xEdit does ("Add Masters...", "Sort Masters", "Clean Masters"), and rewrite every FormID of the plugin to follow it (`MastersUpdated`): the FormIDs of the records, the FormIDs in their elements and the labels of the child groups. Load order FormIDs do not change, so `records get` finds the same records before and after. Like `elements set`, the change is in memory: save in the same session (`batch`, or `xedit serve`).

- `masters add` takes file names of plugins that are loaded (`--load` them, before the plugin to change), skips the masters the plugin has already and the plugin itself, adds the rest in load order and then sorts all masters by load order (`--no-sort` keeps them at the end). Only `.esm`, `.esp` and, where the game has light modules, `.esl` names are added; an `.esp` fails where the game forbids `.esp` masters. Under Starfield the masters of every added master are added too (`wbEnforceAllMasters`).
- `masters clean` keeps a master when a FormID of the plugin points to it, when it is the game master, and under Starfield when it is a master of a used master. It logs `Removing unused master: <name>` per master.
- The response holds `old_masters`, `masters` (the list after the command, or the list it would leave for a dry run) and `changed`.
- Error code `edit_failed` carries the upstream message: `[AddMAddMastersIfMissingasters] Requested file to add is not loaded: "<name>"` (sic), `File "<name>" is not editable` (the game master, the hardcoded file), `Only N of M masters could be added. Master list now contains K entries and is full.`

```
cat > masters.json <<'EOF'
[
  {"command": "masters.add", "params": {"file": "MyPatch.esp", "masters": ["Dawnguard.esm"]}},
  {"command": "files.save", "params": {"file": "MyPatch.esp"}}
]
EOF
xedit --json --edit --game sse --load "<Data>\Dawnguard.esm" --load "<Data>\MyPatch.esp" batch masters.json
xedit --game sse --load "<Data>\MyPatch.esp" masters clean --dry-run
```

## Daemon and MCP server

`serve` and `mcp` load the session once and run the same registry commands against it, so an edit stays in memory between calls and loading is paid once. Both take the global `--game`, `--load` and `--edit` options at startup; the edit gate is fixed then and no request can lift it. Both build their method or tool list from the registry when they run, so every command, including ones added later, is there without any change; `rpc.discover` (serve) and `tools/list` (mcp) show what this build has.

```
xedit --game fo4 --load "<Data>\DLCRobot.esm" --edit serve
xedit --game fo4 --load "<Data>\DLCRobot.esm" --edit serve --pipe NAME
xedit --game fo4 --load "<Data>\DLCRobot.esm" --edit mcp
```

- `serve` reads one JSON-RPC 2.0 request per line from stdin and writes one response line to stdout (`--pipe NAME` listens on the Windows named pipe `\\.\pipe\NAME` instead, one client at a time, and takes the next client when one disconnects; only the user that started it can open it). Progress messages go to stderr. The input ending closes a stdio daemon.
- Each registry command is a method of its own name, with the request fields of `xedit schema` as `params` (an object): `{"jsonrpc":"2.0","id":1,"method":"records.find","params":{"editor_id":"Dawnguard"}}`. A JSON array of requests is a JSON-RPC batch.
- The daemon's own methods: `rpc.discover` (the `xedit schema` catalogue plus `edit_allowed`), `rpc.batch` (`{"commands":[{"command","params"}],"keep_going":false}`, the result of `xedit batch`) and `rpc.shutdown` (ends the daemon).
- Mutating commands behave as on the CLI: without `--edit` at startup they fail with `edit_required` unless `dry_run` is true, and nothing reaches disk until a `files.save` request (use `output` to write somewhere safe while testing).
- A failed command is an error object: `message` is the message of the command, `data.code` its stable code (`edit_required`, `unknown_record`, ...), and `code` is `-32601` for `unknown_command`, `-32602` for `invalid_params`, `-32603` for `internal` and `-32000` for every other failure.
- `mcp` speaks the Model Context Protocol on stdio. Register it with an MCP client as the command above. Each command is a tool named like the command with `.` replaced by `_` (`records_get`, `elements_set`, `files_save`), its `inputSchema` and `outputSchema` are the request and response schemas, and a result carries the JSON as text and as `structuredContent`. A failed command is a result with `isError: true` and the text `<code>: <message>`. Mutating tools say so in their description and are not read-only in their annotations. The plugins load at the first tool call, which can take a while on a big load order; `initialize` and `tools/list` answer at once.

## Saves

`saves dump` reads the saves of `tes4` (`.ess`, OBSE `.obse`), `fo3` (`.fos`, FOSE `.fose`), `fnv` (`.fos`, NVSE `.nvse`), `fo4` (`.fos`, F4SE `.f4se`) and `tes5`, `sse`, `enderal`, `enderalse` (`.ess`, SKSE `.skse`); the co-save definitions are chosen by the extension. For Fallout 3 and Oblivion saves the oracle warns that they are not supported yet; it reads Fallout 3 saves all the same, and the port does both. Upstream's Oblivion save definitions expect the Fallout 3 header magic, so every real Oblivion save stops with `Expected header Magic FO3SAVEGAME, found TES4SAVEGAM`, in the oracle and in the port; only an OBSE co-save (`.obse`) would read. The dump follows the oracle exactly, including its quirks: the LZ4-compressed body of a Skyrim SE save is shown as raw bytes because the oracle's decompression fails, and a Skyrim LE save whose plugin list sits in the save content loads no plugins, so its hardcoded FormIDs show the oracle's access violation text.

## Output

Without `--json` a command prints its result as pretty JSON and an error as `error: <code>: <message>` on stderr with exit code 1. With `--json` it prints one envelope, `{"ok":true,"result":...}` or `{"ok":false,"error":{"code":"...","message":"..."}}`, always on stdout. Error codes are stable: `no_session` (no `--game`), `unknown_file`, `ambiguous_file`, `unknown_record`, `unknown_element`, `invalid_params`, `load_failed`, `unknown_command`, `edit_required`, `edit_failed`, `not_editable`, `save_refused`, `unsupported`, `io`.

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

- No adding or removing of elements from the command line beyond the member `elements set` adds, no copying of records, no FormID changes. Sorted arrays are not sorted again after a change, and flags are not shown as child elements.
- Morrowind plugins are not verified. Oblivion saves do not read (an upstream limit, see above).
- One session per process, fixed at startup: `serve` and `mcp` cannot load other plugins later, so restart them to change the load order.
