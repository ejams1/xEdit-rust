---
name: using-xedit-cli
description: Use when inspecting, editing or saving Bethesda plugins and save games with the native xedit CLI of this repository (version 2, with the write path). Covers the mutation rules (edit flag, dry run, explicit save, readback), loading plugins of any game from Morrowind to Starfield, listing and reading records and elements, setting, adding, removing and copying them, deleting records, changing and renumbering FormIDs, module flags, the masters of a plugin, dumping plugins and saves like xDump, listing, unpacking and packing BSA and BA2 archives (`archive list|extract|pack` and the `bsarch` binary), reading and writing NIF meshes and materials (`assets`), running the batch operations of Sniff on them (`sniff list|run` and the `sniff` binary), running several commands in one session with batch, and keeping a session loaded behind a JSON-RPC daemon (serve) or an MCP server (mcp).
---

# Using the xedit CLI

`xedit` is the command-line interface of the Rust port. Build it with `cargo build --release -p xedit-cli`; the binary is `target/release/xedit.exe`. Every subcommand except `dump`, `saves dump`, `batch`, `serve` and `mcp` is a command of the session registry, so `xedit schema` prints the full list with the JSON Schema of each request and response, and `xedit call <name> --params '<json>'` runs any of them by name. The schema is the authority on parameters; this skill says how to use them safely.

## Mutation rules

Read these before changing anything. They hold for the CLI, `batch`, `serve` and `mcp` alike.

1. **Nothing is written without `--edit`.** Commands that change plugin data or files (`mutates` in `xedit schema`) fail with `edit_required` unless the global `--edit` flag is given or the request has `dry_run: true` (CLI: `--dry-run`). The gate is checked before the parameters are parsed, and a daemon fixes it at startup: no request can lift it.
2. **Run `--dry-run` first.** A dry run does the lookups and the checks the real call does (editability, removability, masters, FormID collisions) and reports what the command would do; it changes nothing, even with `--edit`, and needs no `--edit`. Read its report, then repeat the command without `--dry-run`. A dry run is a report, not a rehearsal: an error the real call raises while changing the data can still appear only then.
3. **Edits live in memory.** An invocation of a one-shot subcommand loads the plugins, runs one command and exits, so the edit is gone. To keep it, save in the same session: `batch` with the edits and a `files.save`, or `serve`/`mcp` where the session stays between calls. Nothing reaches disk until `files.save` (CLI: `save`) runs.
4. **Save explicitly, and never into a game's `Data` folder while testing.** `save --output <path>` writes somewhere else than the loaded path. Without `--output` the plugin is written over the loaded file through a temporary file and a rename, the old file first moves to `<AppName>Edit Backups\<name>.backup.<timestamp>` next to it (`--no-backup` skips that), and a save whose bytes equal the loaded file is not written.
5. **Read the result back after the save.** The response of the edit shows the element before and after; the response of the save shows `crc32`, `loaded_crc32`, `changed` and `written`. Then load the saved file in a fresh process and check it: `records get`/`elements get` for the changed values, `files list` for the masters, and `save --dry-run` on it, which normally reports `changed: false` (a saved file saves to itself). Masters load from the folder of the plugin, so a file saved with `--output` somewhere else needs its masters beside it to load again.
6. **One edit session per process.** The plugins and the edit gate are fixed at startup. To load other plugins, start another process.
7. **Know the stable error codes** (below) and match on `code`, not on the message.

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

## Loading plugins

The global options select the game and the plugins:

```
xedit --game sse --load "<Data>\Skyrim.esm" --load "<Data>\Update.esm" <command>
xedit --game fo4 --load "<Data>\DLCRobot.esm" <command>
```

- `--game` takes the switch of `xDump.exe` without the dash: `tes3` (Morrowind), `tes4` (Oblivion), `fo3`, `fnv`, `tes5` (Skyrim LE), `enderal`, `sse`, `tes5vr`, `enderalse`, `fo4`, `fo4vr`, `fo76`, `sf1` (Starfield).
- `--load` takes the full path of a plugin and may repeat; the plugins load in the order given and each takes the next load order slot after its masters. Masters load from the same folder automatically, so loading `Update.esm` also loads `Skyrim.esm`.
- The hardcoded records of the game load as a file named after the game executable (`SkyrimSE.exe`, `Fallout4.exe`), like xEdit does. It is not a plugin: it can not be saved, and it is not editable.
- Strings of localized plugins come from the loose `Strings` folder or from the game archives next to the plugin (BSA, and BA2 including the Starfield versions).
- `--threads N` sets the threads that read the groups of a plugin while it loads and that build and write the records of `dump`; the default is `RAYON_NUM_THREADS`, else one per CPU, and `--threads 1` runs everything on one thread. The output (dump, saved bytes, every command result) is the same for every count. `saves dump` writes on one thread, and the session commands run on one thread.
- `--json` prints one envelope per invocation (see "Output").

Loading is per process: every invocation loads the plugins again. `Skyrim.esm` takes a few seconds and `Starfield.esm` or `SeventySix.esm` much longer. For more than one or two commands, use `batch` (one load, a fixed list of commands) or `serve`/`mcp` (one load, a conversation), and prefer `records list --signature` and `records find` over reading records one by one.

## Commands

FormIDs are load order FormIDs in hexadecimal, as xEdit shows them: `01003274` is object `003274` of the file in slot 1. `--file` names a loaded plugin; without it, `records list`, `records find` and the plugin commands need exactly one plugin in `--load` (else `ambiguous_file`), and `records get`, `elements get` and the element commands see the record from the last loaded plugin (the winning override for that plugin). Every command below that mutates takes `--dry-run`, and without `--edit` only a dry run is accepted.

Inspection (never mutates):

| CLI | Registry name | What it does |
|---|---|---|
| `session info` | `session.info` | Game tag, game name, data folder and the loaded files in load order. |
| `files list` | `files.list` | Every loaded file with load order, masters, record count, ESM and localized flags. |
| `records list [--file F] [--signature SIG] [--offset N] [--limit N]` | `records.list` | Records of one plugin in file order; `total` counts the matches before paging. |
| `records find [--file F] [--signature SIG] [--editor-id TEXT] [--name TEXT] [--limit N]` | `records.find` | Records whose editor ID or display name contains the text, compared without case. |
| `records get <FormID> [--file F] [--depth N]` | `records.get` | One record with its elements as a tree. |
| `elements get <FormID> <path> [--file F] [--depth N]` | `elements.get` | One element of a record by path. |
| `call system.version` | `system.version` | The version of the build. |
| `dump --game G <plugin>` | (none) | The whole plugin as `xDump.exe` prints it, to stdout; progress goes to stderr. |
| `saves dump --game G --data <Data> <save>` | (none) | A save or co-save as `xDump.exe -saves` prints it. The plugins the save lists load from `<Data>`. |
| `schema` | (none) | The registry: every command with its request and response JSON Schema. |

Changing data (`mutates`, needs `--edit` or `--dry-run`):

| CLI | Registry name | What it does |
|---|---|---|
| `elements set <FormID> <path> [<value>] [--file F] [--native] [--default]` | `elements.set` | Sets the value of one element. See "Editing an element". |
| `elements add <FormID> <name> [--path P] [--file F]` | `elements.add` | Adds a member, an array entry or a child record, as xEdit's "Add". |
| `elements remove <FormID> <path> [--file F]` | `elements.remove` | Removes an element that xEdit lets you remove. |
| `records copy <FormID> --to F [--from F] [--as-new] [--deep] [--prefix T] [--suffix T] [--prefix-remove T] [--suffix-remove T]` | `records.copy` | Copies a record into a plugin as an override or a new record, adding the masters it needs. |
| `records delete <FormID> [--file F]` | `records.delete` | Removes the plugin's version of a record with its child group. |
| `formids change <FormID> [<new FormID>] [--file F] [--target-file T] [--overrides]` | `formids.change` | Gives a record a new load order FormID and updates the records that refer to it. |
| `formids renumber [--file F] [--start HEX] [--compact] [--inject-into M] [--preserve-object-ids] [--all-or-nothing]` | `formids.renumber` | Renumbers the new records of a plugin, updating the referencing records. |
| `files flags [--file F] [--esm B] [--light B] [--medium B] [--update B] [--blueprint B] [--localized B]` | `files.flags` | Sets the module flags of a plugin header; `--dry-run` alone reads them with the compatibility of the new records. |
| `masters add <name>... [--file F] [--no-sort]` | `masters.add` | Adds loaded plugins as masters and sorts the masters by load order unless `--no-sort`. |
| `masters sort [--file F]` | `masters.sort` | Sorts the masters by load order. |
| `masters clean [--file F]` | `masters.clean` | Removes the masters no FormID of the plugin points to. |
| `save [--file F] [--output PATH] [--no-backup]` | `files.save` | Writes a loaded plugin as xEdit saves it. Changes files, not data. |

Sessions:

| CLI | What it does |
|---|---|
| `call <name> --params '<json>'` | Runs any registry command by name; the way to reach `system.version` and any command without its own subcommand. |
| `batch <file.json or -> [--keep-going]` | Runs a JSON array of `{"command": name, "params": {...}}` in one session and prints one envelope per command; stops at the first failure unless `--keep-going`. |
| `serve [--pipe NAME]` | Keeps the session loaded and answers JSON-RPC requests, one per line, on stdio or a named pipe. See "Daemon and MCP server". |
| `mcp` | Serves the commands as MCP tools on stdio. |

Element paths use `\` between names, as in xEdit scripts: `DATA\Health`, `ACBS\Flags\Female`, `Conditions\Condition #0\CTDA\Function`. Signatures (`DNAM`) and names (`DNAM - Flags`) both work for a step, and `[n]` picks an entry by position.

A record or element node has `name`, `display_name` (only when it differs, for example a placed object with its base record), `value` (the text xEdit shows), `summary` (the `[S]:` text of containers without a value), `native` (number, boolean, string or bytes as hexadecimal) and `children`. `--depth 1` keeps the first level of children, which is enough to list the subrecords of a record.

## Archives

`archive list|extract|pack` read, unpack and write BSA (Morrowind through Skyrim Special Edition) and BA2 (Fallout 4, Fallout 76, Starfield) archives. They are the three modes of `BSArch.exe`, need no `--game` or `--load`, and the archives `pack` writes are byte for byte the ones `BSArch.exe -mt:no` writes for the same sources and options, whatever the thread count. The `bsarch` binary (`cargo build --release -p bsarch`) takes the upstream arguments and prints the upstream text; use it where a script already calls `BSArch.exe`.

| CLI | Registry name | What it does |
|---|---|---|
| `archive list <archive> [--files] [--folder F] [--offset N] [--limit N]` | `archive.list` | Format, version, flags, compression, warnings and with `--files` a page of the file table. Reads only. |
| `archive extract <archive> [<folder>] [--threads N]` | `archive.extract` | Unpacks every file below the folder (which must exist; the archive's folder when omitted). Needs `--edit` unless `--dry-run`. |
| `archive pack <archive> <source>... --format F [-z[=zlib\|lz4\|lz4f]] [--split GB] [--filter MASK]... [--no-share] [--threads N] [--archive-flags HEX] [--file-flags HEX]` | `archive.pack` | Packs folders, files and archives (later sources win on equal names). Needs `--edit` unless `--dry-run`. |

- `--format` is `tes3`, `tes4`, `fo3`, `fnv`, `tes5` (the last three are one format), `sse`, `fo4`, `sf1`; `fo4dds` and `sf1dds` (the texture archives) are accepted and stop with `Texture (DX10) archives are not supported yet` until phase 5 step 2, as does extracting a texture from a texture archive. Listing a texture archive works.
- `-z` compresses with the default of the format (zlib for Oblivion, Fallout 3, New Vegas, Skyrim LE and Fallout 4, LZ4F for Skyrim SE, zlib or LZ4 for Starfield); sounds, music and strings stay stored, as BSArch leaves them. A BSA over 2 GB is split by default (`--split 0` keeps one archive, a number is GB, at most 8) and a split archive numbers its files from the second one: `mod.bsa`, `mod2.bsa`.
- A dry run of `archive pack` adds the sources and reports `source_files` and `source_counts` without creating anything; run it first to see which files a folder yields (files under `data\` or a known asset folder keep the path from there; the extensions in `cSkippedExtensions` such as `.esp`, `.bsa`, `.dll` are never packed).
- Error code `archive_failed` carries the message BSArch prints (`Error processing "meshes\a.nif": ...`, `Cannot open file "...". The system cannot find the file specified`). A pack that fails leaves no archive behind. A name that would leave the destination folder (`..`, a drive) is refused by `extract`, where BSArch would write it.
- The same bytes come out for every `--threads`; upstream's multithreaded packing writes the data in the order the threads finish, the port writes it in the order of a single thread.

## Assets

`assets dump|blocks|types|scan|save|set|from-json` read and write the files of the data format units of xEdit: NIF meshes and KF animations of every game from Morrowind to Fallout 4, Fallout 4 BGSM and BGEM materials, the LOD settings (`*.lod` of Skyrim and Fallout 4, `*.dlodsettings` of Fallout 3 and New Vegas), the tree LOD files (`*.lst`, `*.btt`, `*.dtl`), FUZ voice files and DDS headers. They need no `--game` or `--load`. Each takes `<file>` on disk, or the archive that holds it with `--archive-path <path in the archive>`; the format comes from the extension, or `--kind nif|bgsm|bgem|lod|dlodsettings|lst|btt|fuz|dds`.

| CLI | Registry name | What it does |
|---|---|---|
| `assets dump <file> [--archive-path P] [--kind K] [--format text\|json] [--decimals N] [--euler]` | `assets.dump` | The file as xEdit's `ToText` (one element per line, tab indented, CRLF) or `ToJSON` (what Sniff's "Convert to and from JSON" writes). Floats have 6 decimals unless `--decimals` (6 to 16); rotations print as an angle in degrees and an axis, or as Euler angles with `--euler`. Materials have no JSON form (`load_failed: Not implemented`, as upstream). |
| `assets blocks <file> [--archive-path P]` | `assets.blocks` | The NIF version and every block: index, type and name. |
| `assets types` | `assets.types` | Every NIF block type the definitions know. |
| `call assets.scan --params '{"file":..., "archive_path":..., "what":"blocks"\|"textures", "uv_range":N}'` | `assets.scan` | The light scanner of the script functions `NifBlockList` (`Name=Type` per block) and `NifTextureList` / `NifTextureListUVRange` (texture names with their slot). Reads NIF 20.0.0.0 and later only. |
| `assets save <file> --output PATH [--archive-path P] [--dry-run]` | `assets.save` | Loads and writes the file as xEdit saves it (`SaveToData`; for a NIF the header's block types, sizes and string table are rebuilt, unused strings removed). Reports `equal_to_input`. Needs `--edit` unless `--dry-run`. |
| `assets set <file> <path> <value> --output PATH [--block B] [--archive-path P] [--dry-run]` | `assets.set` | Sets one value and writes the file; the request takes a list of `edits` (`block`, `path`, `value`) through `call`/`batch`. Needs `--edit` unless `--dry-run`. |
| `assets from-json <file.json> --output PATH [--kind K] [--dry-run]` | `assets.from-json` | Builds a file from a `assets dump --format json` dump (or a material from the material editor's JSON) and writes it. Needs `--edit` unless `--dry-run`. The file is read as xEdit reads it: by its byte order mark, else in the ANSI code page, so save a dump with non-ASCII text with a UTF-8 BOM. Values of the wrong JSON type fail as in xEdit (`Cannot cast Array into String`). |

- Paths below a NIF block use the dump's names with `\` between them: `Transform\Scale`, `Vertex Data\[3]\Normal`, `Shader Flags 1\Model_Space_Normals` (a flag by name reads and sets `1`/`0`). `--block` picks the block: `header`, `footer`, a block index, or a path of block types and names from a root block (`BSFadeNode\Body\BSLightingShaderProperty`, as Sniff's universal tweaker). Without `--block` the path starts at the file: `[1]\Name` is the name of block 0 (`[0]` is the header).
- The value is the text the dump prints: `Havok | Dynamic` for flags, an enumeration name or its number, `#RRGGBB` for colours, `None` or an index for a block reference. A reference shows as `<index> <type> "<name>"`.
- Saving a file the port loaded unchanged gives xEdit's bytes; `cargo xtask parity nif` checks that on every NIF and material of the corpus archives (see the `checking-parity` skill).
- Error code `load_failed` carries upstream's message (`Error reading NIF block 3 NiTriShapeData: Unexpected end of stream...`, `Unknown NIF version "20.2.0.7" ("User Version"=12, "User Version 2"=155)`: Fallout 76 and Starfield meshes are not supported by xEdit 4.1.5q either), `edit_failed` the message of a value that does not take (`'x' is not a valid integer value`).

## Sniff

The batch operations of Sniff on the NIF, KF and material files of a folder or an archive. They need no `--game` or `--load`.

| CLI | Registry name | What it does |
|---|---|---|
| `sniff list` | `sniff.list` | Every operation: title, group, games, file extensions, whether it only reports, its settings section and each setting with its default, and `not_ported` with the reason for those that are not ported yet. |
| `sniff run <operation> <input> [--output DIR] [--settings INI] [--set NAME=VALUE]... [--path-contains TEXT] [--no-subdir] [--skip-on-errors] [--copy-all] [--threads N] [--log FILE] [--dry-run]` | `sniff.run` | Runs the operation (its title in any case, such as `"Update tangents and binormals"`) on the files below `<input>` (a folder, or a BSA or BA2), as Sniff's `-OP:` automation mode. Changed files go to `--output` (a folder that exists) under their path in the input; an operation that only reports needs none. `--set` sets one setting of the operation's section (`sniff list` names them: `--set bAddIfMissing=1`); `--settings` reads an ini in Sniff's form (`[Main]` and the section named after the title without spaces). The response has Sniff's messages (`Updated: <file>`, `Skipped: <file>: <error>`, the summary), counts and each file's status. Without `--skip-on-errors` the first error stops the run (`aborted`), as Sniff does. Needs `--edit` unless `--dry-run`, which writes nothing (no output files, no log). |

- The `sniff` binary takes Sniff's own arguments: `sniff -S:<settings.ini> -OP:<title> -I:<archive or folder> -O:<folder> -LOG:<file> -skip:yes -subdir:yes|no -threads:N [-P:<path part>] [-all:yes]`, prints the messages and writes the same files and log as `Sniff.exe`. `sniff -list` prints the operations. Without `-S:` it reads `sniff.ini` beside the binary.
- 36 of the 50 operations are ported; `cargo xtask parity sniff` checks them against `Sniff.exe`. The others fail with error code `unsupported` and the reason: the shape merging, geometry copying and animation adding operations come with phase 5 step 5, `Optimize mesh` and `Analyze mesh` need the mesh optimizer (step 6), `Find textures` needs DDS support (step 2), and `Update MOPP code` calls a Havok DLL that is not ported. `Check for errors` with a texture check on fails each `.dds` file with `The texture checks need wbDDS, which is not ported yet (phase 5 step 2)`; its mesh checks work.
- Error codes: `invalid_params` (unknown operation, a missing input or output folder, a setting that does not parse: the message is Sniff's), `unsupported`, `io`.

## Editing an element

`elements set` runs `SetEditValue`, `SetNativeValue` or `SetToDefault` on the element at the path, as a script's `SetEditValue` does, so the `AfterSet` callbacks of the definition run (a GMST's `DATA` is rebuilt when the first letter of its editor ID changes, a MGEF's actor values follow its archetype, and so on). The response holds the element before (`old`) and after (`new`), `changed`, and `added` when the element was created.

- Edit values are the texts xEdit shows in its editor: `1.5` for a float, `Dawnguard "Dawnguard" [QUST:0200C97A]` or `0200C97A` for a FormID (load order FormIDs, as `--game` loads the plugins), a name or a number for an enum, `0000000000000001` for flags. A flag can also be set by its name as the last element of the path: `ACBS\Flags\Female` with `1` or `0`.
- `--native` takes a JSON number or boolean and sets it as the native value; `--default` sets the element to the default of its definition.
- A path whose last element is missing adds that member when the record's definition has it (`ElementEditValues`): `elements set 01003274 SNAM "text"` on a record without `SNAM`.
- Loading runs the fix-ups xEdit runs on load (`wbAllowInternalEdit`): a record that lacks a required subrecord gets it, a worldspace loses its offset data, and the `AfterLoad` callbacks of the definitions apply their corrections. Those records are modified internally and are written from their elements on save.
- Error codes: `edit_failed` carries the message of the upstream exception (`"x" is not a valid character for a flag`, `Not a valid GUID: ...`, `FormID [...] can not be mapped to file FormID for file "..."`), `not_editable` a dry run on an element the editor would not let you change.
- A localized string of a localized plugin takes only a `STRINGID:<hex>` text (an existing string ID); a new text fails with `Can not assign to a localized string` because the string tables are not written yet.

## Adding, removing and copying

These commands change the structure of a plugin as the navigation menu of xEdit does ("Add", "Remove", "Copy as override into...", "Copy as new record into...", "Deep copy as override into..."). Read the result back with `records get` before saving.

- `elements add` runs xEdit's `Add(name, silent)` on the record (or on the container at `--path`). On a record, `name` is a member name or signature (`FULL`, `Model`); an existing member is returned, not doubled. On a `CELL` the signature of a placed record (`REFR`, `ACHR`, `NAVM`, `LAND`, `PGRD`) adds a new record with a new FormID of the cell's plugin to the right child group, made when missing; `INFO` does the same for a `DIAL`, `ROAD` and `CELL[x,y]` or `CELL[P]` (persistent) for a `WRLD`, and `DLBR`, `DIAL` or `SCEN` for a Fallout 4 or later `QUST`. A `LAND`, `PGRD`, `ROAD` or worldspace cell that a master has already is copied as an override instead, as upstream does. On an array, any `name` adds an entry (a number adds at that position of a subrecord array). The response has `element`, and `record` when a record was added.
- `elements remove` removes the element only when xEdit offers "Remove" for it (`IsRemovable`): not a required member, not the last entry of an array that must keep one, not the record header; else `not_removable`. Counters along the count paths of a removed array go to zero.
- `records copy` copies the version of the record that `--from` sees (the last loaded plugin by default) into `--to`. Without `--as-new` the copy is an override with the same load order FormID; when `--to` has the record already, the existing override is returned unchanged (`existed: true`) and nothing is copied over. With `--as-new` the record takes the next free FormID of the target (`NewFormID`, from `HEDR\Next Object ID`, which moves on) and `--prefix`, `--suffix`, `--prefix-remove`, `--suffix-remove` change its editor ID; a cell or the road of a worldspace can not be copied as new (`Can't copy record ... as new record.`). `--deep` copies the child group too (the references of a cell, the responses of a topic, the cells of a worldspace). The parents of the record come along as overrides without their contents (the worldspace and cell of a reference, the topic of a response). The response lists `required_masters` and the `missing_masters` the copy adds to the target (sorted by load order); a required master that loads after the target fails with `The required master "X" can not be added to "Y" as it has a higher load order`. The masters are reported for the record itself, not for the records of a deep copy, as upstream does.
- A record copied into a game master or the hardcoded file fails with `not_editable` (xEdit does not edit the game master). A GMST whose editor ID changes its first letter gets its `DATA` reset by the definition's `AfterSet`, as in xEdit.
- `records delete` removes the record of `--file` (not the version another plugin sees) with its child group. The response has `child_records`. The file header can not be removed.

```
cat > copy.json <<'EOF'
[
  {"command": "records.copy", "params": {"form_id": "0001A332", "to": "MyPatch.esp"}},
  {"command": "elements.set", "params": {"form_id": "0001A332", "file": "MyPatch.esp", "path": "FULL", "value": "New name"}},
  {"command": "files.save", "params": {"file": "MyPatch.esp"}}
]
EOF
xedit --edit --game sse --load "<Data>\Skyrim.esm" --load "<Data>\MyPatch.esp" batch copy.json
```

## FormIDs and module flags

`formids change` runs upstream's "Change FormID": `SetLoadOrderFormID` on the record (the record leaves its master and overrides, takes the FormID, and is registered again; its child group follows, and an interior cell moves to the block and sub-block of its new object ID), then `CompareExchangeFormID` on every record that refers to it.

- The new FormID is a load order FormID. Omitted, it is the next free FormID of the record's file (`NewFormID`, which moves `HEDR\Next Object ID`), or of `--target-file` (the record's file or one of its masters, upstream's "renumber to destination file"). `00000000` and `00000014` are refused, a FormID the file has already fails with `FormID [...] is already present in file ...`, and a FormID of a file that loads before the record's file but is not its master makes that file a master (upstream's `AddRequiredMaster`, without its question; the response lists it in `masters_added`).
- `--overrides` changes the later overrides too (upstream asks "has later overrides, update them too?"): all of them for a master record, the following ones for an override.
- The response lists the referencing records (`referenced_by`) and how many were updated. Without `--target-file` only the records in editable files are updated (the records the dialog lets you pick); with it, and for `formids renumber`, every referencing record is updated when one of them is editable, as upstream's silent update does. The game master is not editable (upstream needs `-IKnowWhatImDoing -IKnowIllBreakMyGameWithThis`), so its records can not be changed.
- The referencing records are found by scanning the files that can see the FormID (its file and the files that have it as a master) until the reference index of phase 4 exists; on a big load order a change or a renumbering can take a while.

`formids renumber` is "Renumber FormIDs from...": the new records of the plugin take the FormIDs from `--start` (six hex digits, three for a light plugin; the next object ID when omitted), keeping the ones already in the new range; `--compact` packs them into `000800`..`000FFF` for an ESL; `--inject-into <master>` gives them free FormIDs of that master (`--preserve-object-ids` keeps the object IDs where the master has them free, `--all-or-nothing` stops when one can not be kept). Their overrides in later plugins follow, the referencing records are updated, and `HEDR\Next Object ID` of the target moves past the highest FormID used.

`files flags` sets the ESM, light (ESL), medium, update (`--overlay`), blueprint and localized flags of the header as `TwbFile.SetIs...` does: a flag the game does not have is ignored (`supported` lists the game's flags), a medium or update flag clears the other two in Starfield, and a file that is not editable fails (`File "..." is not editable`). The flags apply in the order esm, localized, blueprint, update, medium, light. `light_compatible` tells whether every new record has an object ID up to `000FFF` (the save refuses a Light plugin otherwise), `medium_compatible` the same for `00FFFF`, `update_compatible` whether the plugin has no new records. Changing a flag does not move the plugin to another load order slot in the running session. Run `files flags --dry-run` first to read the compatibility.

```
xedit --json --edit --game sse --load "<Data>\MyMod.esp" batch - <<'JSON'
[
  {"command": "formids.renumber", "params": {"compact": true}},
  {"command": "files.flags", "params": {"light": true}},
  {"command": "files.save", "params": {"output": "<somewhere else>\MyMod.esp"}}
]
JSON
```

## Masters

The master commands change the master list of one plugin as the navigation menu of xEdit does ("Add Masters...", "Sort Masters", "Clean Masters"), and rewrite every FormID of the plugin to follow it (`MastersUpdated`): the FormIDs of the records, the FormIDs in their elements and the labels of the child groups. Load order FormIDs do not change, so `records get` finds the same records before and after.

- `masters add` takes file names of plugins that are loaded (`--load` them, before the plugin to change), skips the masters the plugin has already and the plugin itself, adds the rest in load order and then sorts all masters by load order (`--no-sort` keeps them at the end). Only `.esm`, `.esp` and, where the game has light modules, `.esl` names are added; an `.esp` fails where the game forbids `.esp` masters. Under Starfield the masters of every added master are added too (`wbEnforceAllMasters`).
- `masters clean` keeps a master when a FormID of the plugin points to it, when it is the game master, and under Starfield when it is a master of a used master. It logs `Removing unused master: <name>` per master.
- The response holds `old_masters`, `masters` (the list after the command, or the list it would leave for a dry run) and `changed`.
- Error code `edit_failed` carries the upstream message: `[AddMAddMastersIfMissingasters] Requested file to add is not loaded: "<name>"` (sic), `File "<name>" is not editable` (the game master, the hardcoded file), `Only N of M masters could be added. Master list now contains K entries and is full.`
- The update initialises every record of the plugin, as upstream does, so a saved plugin shows the changes an initialised record shows (sorted subrecords, required members, `AfterLoad` fixes) besides the master change.

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

## Saving a plugin

`save` runs `PrepareSave` and `WriteToStream` as upstream does: every unmodified record is copied as loaded, a modified record is rebuilt from its elements (and compressed again when its flag says so), and the file's CRC32 is computed on the result. The response reports `bytes`, `crc32`, `loaded_crc32`, `changed`, `written` and `backup`. A save of an untouched plugin equals the bytes xEdit's own GUI saves for every plugin of the corpus that the oracle saves (239 of 249 plugins; the rest are refusals the oracle gives too, and the Morrowind masters, which it can not save). That is not always the input file: xEdit drops some data on load (the `OFST` of a worldspace) and rewrites the header (the `HEDR` record count, `INCC`, the `ONAM` list of a master, the ESM flag of an `.esm`).

- `--dry-run` builds the file and reports, writing nothing; use it to see `changed` and the refusal a save would give.
- `--output` writes somewhere else than the loaded path. Never point it into a game's `Data` folder while testing; the harness writes into the parity cache.
- Error codes: `save_refused` carries an upstream `PrepareSave` message, which the oracle gives for the same file (a Starfield blueprint module, a record in the wrong group, an `.esp` master where the game forbids it, an official Starfield module whose header the save would have to edit: `[TES4:00000000] can not be edited`). The messages of a save name records as the xEdit GUI does (`EditorID "Name" [SIG:FormID]`), so they read like the GUI's; the other commands keep xDump's names.
- Only the worldspace records are initialized by a save of an unmodified file (upstream drops their `OFST` subrecord and marks their children modified), so a big master saves in seconds; the children of its worldspaces are rebuilt record by record.

## Daemon and MCP server

`serve` and `mcp` load the session once and run the same registry commands against it, so an edit stays in memory between calls and loading is paid once. Use them for a long session: several edits with reads in between, an agent exploring a load order, a save after a check. Both take the global `--game`, `--load`, `--edit` and `--threads` options at startup; the edit gate is fixed then and no request can lift it. Both build their method or tool list from the registry when they run, so every command, including ones added later, is there without any change; `rpc.discover` (serve) and `tools/list` (mcp) show what this build has.

```
xedit --game fo4 --load "<Data>\DLCRobot.esm" --edit serve
xedit --game fo4 --load "<Data>\DLCRobot.esm" --edit serve --pipe NAME
xedit --game fo4 --load "<Data>\DLCRobot.esm" --edit mcp
```

- `serve` reads one JSON-RPC 2.0 request per line from stdin and writes one response line to stdout (`--pipe NAME` listens on the Windows named pipe `\\.\pipe\NAME` instead, one client at a time, and takes the next client when one disconnects; only the user that started it can open it). Progress messages go to stderr, never to stdout. The input ending closes a stdio daemon.
- Each registry command is a method of its own name, with the request fields of `xedit schema` as `params` (an object): `{"jsonrpc":"2.0","id":1,"method":"records.find","params":{"editor_id":"Dawnguard"}}`. A JSON array of requests is a JSON-RPC batch.
- The daemon's own methods: `rpc.discover` (the `xedit schema` catalogue plus `edit_allowed`), `rpc.batch` (`{"commands":[{"command","params"}],"keep_going":false}`, the result of `xedit batch`) and `rpc.shutdown` (ends the daemon).
- Mutating commands behave as on the CLI: without `--edit` at startup they fail with `edit_required` unless `dry_run` is true, and nothing reaches disk until a `files.save` request (use `output` to write somewhere safe while testing).
- A failed command is an error object: `message` is the message of the command, `data.code` its stable code (`edit_required`, `unknown_record`, ...), and `code` is `-32601` for `unknown_command`, `-32602` for `invalid_params`, `-32603` for `internal` and `-32000` for every other failure.
- `mcp` speaks the Model Context Protocol on stdio. Register it with an MCP client as the command above. Each command is a tool named like the command with `.` replaced by `_` (`records_get`, `elements_set`, `files_save`), its `inputSchema` and `outputSchema` are the request and response schemas, and a result carries the JSON as text and as `structuredContent`. A failed command is a result with `isError: true` and the text `<code>: <message>`. Mutating tools say so in their description and are not read-only in their annotations. The plugins load at the first tool call, which can take a while on a big load order; `initialize` and `tools/list` answer at once. The server has no resources, prompts or progress notifications.
- A session is fixed at startup: neither daemon can load other plugins later, so restart it to change the load order.

## Saves

`saves dump` reads the saves of `tes4` (`.ess`, OBSE `.obse`), `fo3` (`.fos`, FOSE `.fose`), `fnv` (`.fos`, NVSE `.nvse`), `fo4` (`.fos`, F4SE `.f4se`) and `tes5`, `sse`, `enderal`, `enderalse` (`.ess`, SKSE `.skse`); the co-save definitions are chosen by the extension. For Fallout 3 and Oblivion saves the oracle warns that they are not supported yet; it reads Fallout 3 saves all the same, and the port does both. Upstream's Oblivion save definitions expect the Fallout 3 header magic, so every real Oblivion save stops with `Expected header Magic FO3SAVEGAME, found TES4SAVEGAM`, in the oracle and in the port; only an OBSE co-save (`.obse`) would read. The dump follows the oracle exactly, including its quirks: the LZ4-compressed body of a Skyrim SE save is shown as raw bytes because the oracle's decompression fails, and a Skyrim LE save whose plugin list sits in the save content loads no plugins, so its hardcoded FormIDs show the oracle's access violation text. Saves are read only; there is no save-editing command.

## Output

Without `--json` a command prints its result as pretty JSON and an error as `error: <code>: <message>` on stderr with exit code 1. With `--json` it prints one envelope, `{"ok":true,"result":...}` or `{"ok":false,"error":{"code":"...","message":"..."}}`, always on stdout; `batch` puts one envelope per command in the `result` array (`{"ok", "command", "result" or "error"}`). Error codes are stable: `no_session` (no `--game`), `unknown_file`, `ambiguous_file`, `unknown_record`, `unknown_element`, `invalid_params`, `load_failed`, `unknown_command`, `edit_required`, `edit_failed`, `not_editable`, `not_removable`, `save_refused`, `archive_failed`, `unsupported`, `io`, `internal`.

The progress log of a load and a save (`[Skyrim.esm] Loading file`, the string table encodings) goes to stderr, as xEdit's messages do. With several threads the messages of the workers arrive in no fixed order.

## Examples

```
xedit --game sse --load "<Data>\Update.esm" records find --editor-id Dawnguard --signature MESG
xedit --game sse --load "<Data>\Update.esm" elements get 01003274 DNAM
xedit --game fo4 --load "<Data>\DLCRobot.esm" records list --signature NPC_ --limit 20
xedit --game sse --load "<Data>\Update.esm" elements set 01003274 DNAM\Flags 0000000000000001 --dry-run
xedit dump --game fo4 "<Data>\DLCworkshop01.esm" > DLCworkshop01.txt
xedit saves dump --game fo4 --data "<Data>" "<My Games>\Fallout4\Saves\Autosave1.fos" > save.txt
xedit --json --game sse --load "<Data>\Skyrim.esm" save --dry-run
xedit --json --edit --game sse --load "<Data>\Skyrim.esm" save --no-backup --output "<somewhere else>\Skyrim.esm"
```

## Known gaps

Behaviour a user can meet, as of the end of phase 3. Each is an upstream behaviour not ported yet; say so rather than work around it silently.

- **LString.** The string tables of a localized plugin are not written. Setting a localized string from text fails (`Can not assign to a localized string: writing the string tables is not ported yet`; only a `STRINGID:` text works), and `records copy` into a localized plugin copies the record with its localized strings empty (the `FULL` of a copied `NPC_` reads `""`). Check the strings of a copy before saving it.
- **Copy over an existing override.** xEdit's "...with overwriting" (`aAllowOverwrite`) is not ported: `records copy` returns the existing override unchanged. A partial form (`MakePartialForm`), template elements and aligned arrays can not be copied either.
- **Sorted arrays.** A record that is rebuilt on save sorts its sorted subrecord arrays as upstream does, but an array that is sorted by the value of a subrecord (the `KWDA` keyword arrays, `srsSorted` and `arrSorted`) is not, an array is not sorted again after a FormID update of its entries, and after a master update (`masters add|sort|clean`) the port sorts unchanged sorted arrays that the oracle leaves in file order (30 `MSWP` and 1 `RACE` of `DLCworkshop01.esm`); a saved plugin can therefore differ from xEdit's in the order of those entries.
- **References.** The records that refer to a FormID are found by scanning the loaded files on demand (`formids.change`, `formids.renumber`) until the reference index of phase 4; the editor ID index of a file does not learn the records an edit adds.
- **Flags** are not shown as child elements.
- **Morrowind.** Plugins load and dump, but the save stops at `must have a FormID`: the identity FormID of a TES3 record is not ported. The 4.1.5q oracle saves no Morrowind plugin either (its GUI runs Morrowind in view mode), so there is nothing to compare with.
- **Sessions.** One session per process; `serve` and `mcp` can not load other plugins later, and a named pipe serves one client at a time.
- **Starfield.** The complex FileIDs are ported in the master functions but the FormID lookups ignore them, so FormID changes and renumbering across masters are unchecked there. The oracle refuses to save the official Starfield modules whose header the save would edit, and so does the port.
- **Oblivion saves** do not read (an upstream limit, see "Saves").
- **Mesh optimizing.** `SpellOptimize`, `SpellStripify` and `SpellTriangulate` of a NIF (and stripifying triangles) need `wbMeshOptimize`, which comes with LOD generation (phase 5 step 6).
- **Sniff.** 14 of Sniff's 50 operations are not ported yet (`sniff list` gives each one's `not_ported` reason); see "Sniff".
- **Texture archives.** `archive pack` and `archive extract` stop on `Fallout 4 DDS` and `Starfield DDS` archives (`wbDDS` is phase 5 step 2); listing works. The loose texture and `BSArch.exe -fo4dds` paths are the same gap.
