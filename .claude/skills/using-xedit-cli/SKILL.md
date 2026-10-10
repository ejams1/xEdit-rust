---
name: using-xedit-cli
description: Use when inspecting, editing or saving Bethesda plugins and save games with the native xedit CLI of this repository (version 2, with the write path). Covers the mutation rules (edit flag, dry run, explicit save, readback), loading plugins of any game from Morrowind to Starfield, listing and reading records and elements, setting, adding, removing and copying them, deleting records, changing and renumbering FormIDs, module flags, the masters of a plugin, dumping plugins and saves like xDump, listing, unpacking and packing BSA and BA2 archives (`archive list|extract|pack` and the `bsarch` binary), reading and writing NIF meshes and materials (`assets`), running the batch operations of Sniff on them (`sniff list|run` and the `sniff` binary), generating LOD like the LODGen mode (`lodgen`), running several commands in one session with batch, and keeping a session loaded behind a JSON-RPC daemon (serve) or an MCP server (mcp).
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
- `--fill-pnam` gives a topic response without `PNAM` the `PNAM` of the response it follows when it is built (xEdit's `-FillPNAM`, in the games that sort responses: Oblivion to Skyrim). `clean --quick` loads with the settings of the quick clean mode, see "Cleaning".
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
| `conflicts [--file F]... [--signature SIG]... [--min-conflict-all CA] [--conflict-this CT]... [--include-single] [--master-and-leafs] [--quick-show-conflicts] [--modgroups NAME]... [--all-modgroups] [--saved-modgroups] [--offset N] [--limit N]` | `conflicts.list` | The conflict status of the records of the loaded plugins, and per file the counts and the highest status. See "Conflicts" and "Mod groups". |
| `compare <FormID> [--file F] [--master-and-leafs] [--hide-no-conflict] [--include-hidden] [--modgroups NAME]... [--all-modgroups] [--saved-modgroups]` | `records.compare` | The records of a FormID side by side, row by row, with the conflict status of each row and cell, as the view tab shows them. |
| `modgroups list [--all]` | `modgroups.list` | The valid mod groups by name (`--all`: every group of every file) with their items, validity, messages and whether the saved selection has them. |
| `modgroups show <name> [--file F]` | `modgroups.show` | One mod group with the state of each item: loaded, load order, CRC32s, current CRC32, valid. |
| `check [--file F]... [--record FormID]... [--record-file F] [--last]` | `files.check` | xEdit's "Check for Errors": every element of the plugins (or files, or records) asked for its errors, printed as xEdit's message log shows them (with `--json` the records and errors as data). See "Checking for errors". |
| `dump --check --game G <plugin>` | (none) | `xDump.exe -check`: the errors of every element of the plugin in xDump's form (children from the last, `Above errors were found in: ...` below each container), to stdout. |
| `refs get <FormID> [--file F] [--offset N] [--limit N]` | `refs.get` | The records that refer to a record (xEdit's "Referenced By" tab) and the FormIDs it refers to. Builds the reference index first. |
| `refs build [--file F] [--only-load]` | `refs.build` | Builds the reference index of the loaded files, or loads it from the reference cache, and saves the cache (`BuildOrLoadRef`). |
| `refs dump` | (none) | The referenced-by lists of every record as text, for the parity check. |
| `call system.version` | `system.version` | The version of the build. |
| `dump --game G <plugin>` | (none) | The whole plugin as `xDump.exe` prints it, to stdout; progress goes to stderr. |
| `tool modes` | `tool.modes` | The seventeen tool modes of xEdit with the switch of each, whether the game of the session has it, what it does and what of it is missing here. |
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
| `clean [--file F] [--itm] [--udr] [--quick] [--output PATH] [--no-backup]` | `files.clean` | Cleans a plugin: removes the records identical to their master (ITM), undeletes and disables the deleted references (UDR), or runs xEdit's quick auto clean mode, which saves. See "Cleaning". |
| `files new <name> [--light] [--medium]` | `files.new` | A new empty plugin in the data folder with the game master as its master (xEdit's `AddNewFile`); save it with `save --file <name>` in the same `batch` or `serve` session. `file_exists` when the data folder or the session has it. |
| `patch merged <name> [--no-save] [--output PATH] [--no-backup]` | `patch.merged` | xEdit's "Create Merged Patch": a new `.esp` that merges the lists several plugins change, saved into the data folder (or `--output`). See "Merged patch". |
| `records cleanup-injected [<FormID>...] [--file F]` | `records.cleanup_injected` | Copies the records that refer to injected records of a plugin that is not their master into that plugin and removes those references from the originals (xEdit's "Cleanup injected records"). |
| `modgroups select [<name>...] [--saved] [--all]` | `modgroups.select` | Chooses the active mod groups and saves the selection in xEdit's settings file. Changes a file. |
| `modgroups create <name> --module M... [--item LINE]... [--include-crcs] [--add-current-crcs] [--file M]` | `modgroups.create` | Adds a mod group to the `.modgroups` file of one of its modules. Changes a file. |
| `modgroups edit <name> [--file F] [--new-name N] [--item LINE]... [--add-current-crcs]` | `modgroups.edit` | Renames a mod group or replaces its items. Changes a file. |
| `modgroups delete <name>... [--file F]` | `modgroups.delete` | Deletes mod groups. Changes files. |
| `modgroups update-crcs [--no-add] [--no-update] [--module M]... [--modgroup NAME]...` | `modgroups.update_crcs` | Adds the current CRC32s of the modules to the items of mod groups. Changes files. |
| `localization set <text> (--table T --id HEX \| --form-id ID --path P [--file F]) [--editor-text]` | `localization.set` | Changes the text of a string of a string table (the localization editor's Save). See "Localization". |
| `localization export <table> [--output PATH]` | `localization.export` | Writes a table as text, an `[ID]` line and the text per string. |
| `localization localize [--file F] [--translate-from T]... [--translate-to T]...` | `localization.localize` | "Localize plugin": the strings move into new tables, the plugin holds their IDs. |
| `localization delocalize [--file F]` | `localization.delocalize` | "Delocalize plugin": the texts go into the plugin, the localized flag is cleared. |
| `tool run <mode> [<plugin>...] [--output PATH] [--format RAW\|UESPWIKI] [--seq-path DIR]` | `tool.run` | Runs a tool mode of xEdit over the loaded plugins (`setesm`, `clearesm`, `masterupdate`, `masterrestore`, `onamupdate`, `sortandcleanmasters`, `generateseq`, `checkforerrors`, `checkforitm`, `checkfordr`, `export`, `edit`, `view`, `translate`). See "Tool modes". |

String tables (no `--edit` needed):

| CLI | Registry name | What it does |
|---|---|---|
| `localization files [--file F] [--no-load]` | `localization.files` | The loaded string tables, as the localization editor lists them: name, path, count, `modified`, next ID. Loads the tables of the localized plugins first. |
| `localization strings <table> [--offset N] [--limit N]` | `localization.strings` | The strings of a table with their IDs, in table order. |
| `localization get (--table T --id HEX \| --form-id ID --path P [--file F])` | `localization.get` | One string, by table and ID or through the localized element that holds it (its table and ID are in the response). |
| `localization languages` | `localization.languages` | The session's language and the languages and tables the data folder and its archives hold. |
| `localization language <language> [--discard]` | `localization.language` | Switches the language of the tables (xEdit's Language menu); the records read their names again. |

Sessions:

| CLI | What it does |
|---|---|
| `call <name> --params '<json>'` | Runs any registry command by name; the way to reach `system.version` and any command without its own subcommand. |
| `batch <file.json or -> [--keep-going]` | Runs a JSON array of `{"command": name, "params": {...}}` in one session and prints one envelope per command; stops at the first failure unless `--keep-going`. |
| `serve [--pipe NAME]` | Keeps the session loaded and answers JSON-RPC requests, one per line, on stdio or a named pipe. See "Daemon and MCP server". |
| `mcp` | Serves the commands as MCP tools on stdio. |

Element paths use `\` between names, as in xEdit scripts: `DATA\Health`, `ACBS\Flags\Female`, `Conditions\Condition #0\CTDA\Function`. Signatures (`DNAM`) and names (`DNAM - Flags`) both work for a step, and `[n]` picks an entry by position.

A record or element node has `name`, `display_name` (only when it differs, for example a placed object with its base record), `value` (the text xEdit shows), `summary` (the `[S]:` text of containers without a value), `native` (number, boolean, string or bytes as hexadecimal) and `children`. `--depth 1` keeps the first level of children, which is enough to list the subrecords of a record.

## Conflicts

`conflicts` and `compare` classify the records of a FormID as the GUI colours them (`ConflictLevelForMainRecord`; the view tab's `InitConflictStatus`). Load every plugin of the load order that matters: a record is compared with the records of its FormID in all loaded files, and the masters of the loaded plugins load with them.

- `conflict_all` is the conflict of all the records of the FormID, from low to high: `caOnlyOne` (a single record), `caNoConflict` (several, all equal), `caConflictBenign`, `caOverride` (overridden without a conflict: one override, or every override agrees with the last), `caConflict`, `caConflictCritical` (a FormID-type value or an injected record). `conflict_this` is the part one record plays: `ctMaster`, `ctIdenticalToMaster` (an identical override: ITM), `ctOverride`, `ctIdenticalToMasterWinsConflict`, `ctConflictWins`, `ctConflictLoses`, `ctConflictBenign`, `ctOnlyOne`, and `ctHiddenByModGroup`, `ctIgnored`, `ctNotDefined`.
- `conflicts` lists every record that is not the only record of its FormID; `--include-single` lists those too. `--file` limits the list, not the comparison. `--min-conflict-all caConflict --conflict-this ctConflictLoses` finds the records of a plugin that lose a conflict; `--conflict-this ctIdenticalToMaster` finds identical overrides. The `files` part of the result has, for every loaded file, `records`, `single`, the counts by status and the highest status of its records, which is what the navigation tree shows for a file. `messages` holds the warnings xEdit writes to its message log while it compares (`Comparing a mix of sorted, unsorted, and/or alignable entries ...`).
- Game settings (`GMST`) and default objects (`DFOB`) are compared by editor ID, the first of each file, not by FormID, as xEdit does; a `NAVI` record is compared only with the records of its own file, so its overrides show `caOnlyOne`/`ctHiddenByModGroup`.
- `compare` gives one column per record (`columns`, with the file and the record's `conflict_this`) and the rows of the view as a tree (`rows`, each with `name`, `conflict_all`, one cell per column with `value` and `conflict_this`, and `children`). A cell without an element is `null`. Rows the view hides (ignored members such as the record header's data size, members no record has) are left out unless `--include-hidden`; `--hide-no-conflict` leaves out the rows without a conflict, as the view's "Hide no conflict and empty rows". Sorted arrays are matched by their sort keys (" (sorted)" in the row name) and unsorted arrays aligned entry by entry (" (aligned)").
- The comparison follows the GUI's settings: the session builds the definitions with simple records (`wbSimpleRecords`, so the landscape data and similar subrecords are byte arrays), the unused fields hidden, the contained-in element of a placed record (its cell or worldspace) and the flags of a flags value as elements of their own. Localized strings compare as they resolve: a plugin whose strings files are not found shows `<Error: No strings file for lstring ID ...>`, which differs from a resolved text.
- `--master-and-leafs` compares only the master and the overrides no other override has as a master ("Only show Master and Leafs"); `--quick-show-conflicts` classifies a FormID with one override as an override without comparing them, as `-quickshowconflicts` does.
- The records of different FormIDs are compared on all CPUs (`--threads`); the result is the same for every thread count. A full Fallout 4 load order of vanilla files takes seconds.

```
xedit --game fo4 --load "<Data>\Fallout4.esm" --load "<Data>\DLCRobot.esm" conflicts --file DLCRobot.esm --min-conflict-all caConflict
xedit --game fo4 --load "<Data>\DLCRobot.esm" compare 000BB1F9 --hide-no-conflict
```
## Archives

`archive list|extract|pack` read, unpack and write BSA (Morrowind through Skyrim Special Edition) and BA2 (Fallout 4, Fallout 76, Starfield) archives. They are the three modes of `BSArch.exe`, need no `--game` or `--load`, and the archives `pack` writes are byte for byte the ones `BSArch.exe -mt:no` writes for the same sources and options, whatever the thread count. The `bsarch` binary (`cargo build --release -p bsarch`) takes the upstream arguments and prints the upstream text; use it where a script already calls `BSArch.exe`.

| CLI | Registry name | What it does |
|---|---|---|
| `archive list <archive> [--files] [--folder F] [--offset N] [--limit N]` | `archive.list` | Format, version, flags, compression, warnings and with `--files` a page of the file table. Reads only. |
| `archive extract <archive> [<folder>] [--threads N]` | `archive.extract` | Unpacks every file below the folder (which must exist; the archive's folder when omitted). Needs `--edit` unless `--dry-run`. |
| `archive pack <archive> <source>... --format F [-z[=zlib\|lz4\|lz4f]] [--split GB] [--filter MASK]... [--no-share] [--threads N] [--archive-flags HEX] [--file-flags HEX]` | `archive.pack` | Packs folders, files and archives (later sources win on equal names). Needs `--edit` unless `--dry-run`. |

- `--format` is `tes3`, `tes4`, `fo3`, `fnv`, `tes5` (the last three are one format), `sse`, `fo4`, `sf1`; `fo4dds` and `sf1dds` are the texture archives (Fallout 4 and Starfield DX10 BA2). Every file packed into one must be a DDS file: it is stored as chunks of mipmaps (a 1024 pixel texture in three, a cube map in one) and `extract` writes the DDS file again with the header xEdit makes (`Not a valid DDS file`, `Unsupported DDS format` and `DDS is in XBox format` are the refusals; a 24 bit RGB file is stored as 32 bit). Texture archives should be compressed (`-z`); BSArch warns that uncompressed ones crash the game.
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

The batch operations of Sniff on the NIF, KF and material files of a folder or an archive, and on the DDS files of a texture folder or archive (`Find textures`, `Check for errors`). They need no `--game` or `--load`.

| CLI | Registry name | What it does |
|---|---|---|
| `sniff list` | `sniff.list` | Every operation: title, group, games, file extensions, whether it only reports, its settings section and each setting with its default, and `not_ported` with the reason for those that are not ported yet. |
| `sniff run <operation> <input> [--output DIR] [--settings INI] [--set NAME=VALUE]... [--path-contains TEXT] [--no-subdir] [--skip-on-errors] [--copy-all] [--threads N] [--log FILE] [--dry-run]` | `sniff.run` | Runs the operation (its title in any case, such as `"Update tangents and binormals"`) on the files below `<input>` (a folder, or a BSA or BA2), as Sniff's `-OP:` automation mode. Changed files go to `--output` (a folder that exists) under their path in the input; an operation that only reports needs none. `--set` sets one setting of the operation's section (`sniff list` names them: `--set bAddIfMissing=1`); `--settings` reads an ini in Sniff's form (`[Main]` and the section named after the title without spaces). The response has Sniff's messages (`Updated: <file>`, `Skipped: <file>: <error>`, the summary), counts and each file's status. Without `--skip-on-errors` the first error stops the run (`aborted`), as Sniff does. Needs `--edit` unless `--dry-run`, which writes nothing (no output files, no log). |

- The `sniff` binary takes Sniff's own arguments: `sniff -S:<settings.ini> -OP:<title> -I:<archive or folder> -O:<folder> -LOG:<file> -skip:yes -subdir:yes|no -threads:N [-P:<path part>] [-all:yes]`, prints the messages and writes the same files and log as `Sniff.exe`. `sniff -list` prints the operations. Without `-S:` it reads `sniff.ini` beside the binary.
- 49 of the 50 operations are ported; `cargo xtask parity sniff` checks them against `Sniff.exe`. `Update MOPP code` fails with error code `unsupported`: it calls `NifMopp.dll`, the Havok MOPP builder, which the port does not have. `ProcCollapseLinksArrays` is not registered in the 4.1.5q form and has no operation. `Find textures` reports the DDS files that match its filters, or copies them without `bReportOnly`, over the DDS record of a texture archive entry and the header `wbDDS` reads otherwise; the texture checks of `Check for errors` (`Invalid texture size or format`, `Unsupported texture formats`) run on DDS files while they are on (`ProcessedFiles=*.dds`, the setting of the check list), and a file that is not a DDS is reported as `Not a valid DDS file`.
- Error codes: `invalid_params` (unknown operation, a missing input or output folder, a setting that does not parse: the message is Sniff's), `unsupported`, `io`.

## LOD generation

`xedit lodgen` runs xEdit's LODGen mode (`-lodgen`, the LODGen form and `wbLOD`) on the loaded plugins: Oblivion's distant LOD (`.lod` and `.cmp` files of every worldspace), the trees LOD (billboard atlas, `.lst` list, `.btt` or `.dtl` blocks) and objects LOD of Skyrim and the Fallouts, and the objects LOD of Fallout 4. Load every plugin of the load order with `--load` (in load order) and give `--game`.

| CLI | Command | What it does |
| --- | --- | --- |
| `lodgen [--worldspace EDITORID]... [--set NAME=VALUE]... [--settings INI] [--output DIR] [--scripts DIR] [--temp DIR] [--data DIR] [--game-ini INI] [--seed N] [--split-trees] [--dry-run]` | `lodgen.generate` | Generates the LOD of the worldspaces (without `--worldspace`, the default one the form checks; Oblivion does all). `--set` sets an option of the form by its name in `[<APP> LOD Options]` of `<APP>LODGen.ini` (`ObjectsLOD`, `TreesLOD`, `Trees3D`, `BuildAtlas`, `AtlasWidth`, `AtlasDiffuseFormat=DXT5`, `Chunk`, `LODLevel`, `TreesBrightness`, ...; the schema lists them); `--settings` is that ini, read first and written back as the form does. The output goes below `--output` (the data folder by default, so give it). `--scripts` is the `Edit Scripts` folder with `LODGenx64.exe`, `Texconvx64.exe`, `LODGen_flat_lod.nif` and the atlas maps: the objects LOD meshes are built by `LODGenx64.exe` from the `LODGen.txt` the command writes there. `--game-ini` is the game's ini whose archive lists load before the plugins' archives. The response lists the worldspaces with their checks, every option, the archives loaded and the messages of the log. Needs `--edit` unless `--dry-run` (which lists the worldspaces and options and writes nothing). |

- `--seed` sets the `RandSeed` of the tree rotations; upstream seeds from the clock, so two runs differ there unless the seed is given (the response has the seed used).
- `--split-trees` splits the trees LOD atlas of each worldspace into one billboard `.dds` and `.txt` per tree (the form's hidden `Split LOD Atlas` button), below `<output>Textures/Terrain/LODGen/AtlasSplit_<atlas>/`; Skyrim, Fallout 3 and New Vegas.
- Fallout 4 and Skyrim objects LOD find no LOD models in the 4.1.5q definitions (an upstream quirk: the LOD model path it reads does not exist), so they end with `no valid references found`, as the oracle does. Fallout 76 and Starfield are refused (`unsupported`).
- `cargo xtask parity lodgen` compares the output files and the log with the xEdit GUI's LODGen mode.

## Mod groups

A mod group is a named list of modules in a `.modgroups` file that says which records a user has checked against each other: while it is active, the comparison of a FormID's records leaves out the records of the modules the group says a later module hides, and they get `ctHiddenByModGroup`. The files are xEdit's: `<plugin>.modgroups` next to each plugin of the data folder (also of plugins that are not loaded, whose groups then count for nothing) and the program's own `<AppName>Edit.modgroups` (`--modgroups-file` names another). Each section is a group, each line an item `[flags]file[:crc32,crc32,...]`: no flag is a module that is both a target (can be hidden) and a source (hides the targets above it); `@` target only, `#` source only, `-` neither, `+` optional, `!` forbidden (the group is invalid while it is loaded), `}` load order not checked, `{` order not checked within a block of such items. A CRC32 list makes the item count only for a file with one of those CRC32s.

- A group is valid when every required item is loaded with a listed CRC32, no forbidden one is, the loaded items load in the group's order and at least one source has a target above it. `modgroups list` shows the valid groups of loaded modules, `--all` every group; each has `messages` (the reasons, as xEdit's log says them) and `selected`.
- **Activating.** `conflicts` and `compare` compare without mod groups unless told: `--modgroups NAME` (repeat) activates valid groups by name, `--all-modgroups` every valid group (as xEdit's `-autoload`), `--saved-modgroups` the saved selection. The result lists `mod_groups` (activated) and `mod_group_hides` (each module with the modules whose records it hides). The first and the last record of a FormID are never hidden.
- **The saved selection** is the `[ModGroups] Selection` of xEdit's settings file (`--settings` names another; the default is xEdit's own, `<AppName>Edit.ini` next to the program, else `Plugins.<app>viewsettings` next to the game's `Plugins.txt` in `%LOCALAPPDATA%`), so the GUI and the CLI share it. `modgroups select` writes it; the other mutating `modgroups` commands end, as the GUI does, with a reload: the files read again, `validation_messages`, and the saved selection written again with only its valid groups (a group just created is added).
- **Writing.** The files are written as xEdit writes them: `create` appends the group after an empty line to the file of the module given with `--file` (default: the group's first loaded module), keeping its text and encoding; `edit` and `update-crcs` write the file back as an ini file (comments and blank lines gone, spaces around `=` removed, each section followed by an empty line) with the group at the end, in the ANSI code page; `delete` writes it back as an ini file in its encoding. `update-crcs` gives every item of a chosen group its module's current CRC32 (also the items of modules not chosen, as xEdit), `--no-add` and `--no-update` are xEdit's two questions (No to the first skips the second).

```
xedit --game sse --load "<Data>\Dragonborn.esm" modgroups list --all
xedit --edit --game sse --load "<Data>\Dragonborn.esm" modgroups create "DB over DG" --module Dawnguard.esm --module Dragonborn.esm --include-crcs --dry-run
xedit --game sse --load "<Data>\Dragonborn.esm" conflicts --saved-modgroups --conflict-this ctHiddenByModGroup
```

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
- The referencing records come from the reference index (see "References"), which the first command that needs it builds for every loaded file; on a big load order without a reference cache that first build takes a while.

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

## References

The reference index is xEdit's "Referenced By" information: for every record the FormIDs its elements refer to (`References`), and for every master record the records that refer to it or to one of its overrides (`ReferencedBy`). The GUI builds it for every loaded file once they are loaded; the CLI builds it when a command first needs it (`refs get`, `formids change`, `formids renumber`) or when `refs build` runs, once per session, on all CPUs (`--threads`; the lists are the same for every count).

- `refs get <FormID>` reports `referenced_by` (the list of the master of the record, sorted by load order FormID and then by the load order of the referencing file, as xEdit's tab shows it; a record that refers through two FormIDs that resolve to the same record is listed twice, as in xEdit), `referenced_by_count`, `master`, and `references`: the FormIDs the record's own elements hold, as its file stores them, each with the record it resolves to. `--offset` and `--limit` page the list (a keyword can have tens of thousands of entries).
- Edits keep the index right as xEdit does: a changed record collects its references again, a new or copied record joins the lists of the records it refers to, a deleted record leaves them, and a record whose FormID changes hands its list to the override that becomes the master.
- **The reference cache.** Like xEdit, the build saves the references of every file with more than 500 records (or that took more than 2 seconds) to a cache file, and the next process loads them instead of building (`refs build` reports `loaded`, `built` or `built_and_saved` per file). The cache folder is xEdit's: `<AppName>Edit Cache` in the data folder of the plugins (`SSEEdit Cache`, `FO4Edit Cache`), so by default the CLI writes into the game's `Data` folder. Pass the global `--cache-path <folder>` to put it elsewhere, `--dont-cache-save` to write none, `--dont-cache-load` to read none and `--dont-cache` for neither. A cache file is named after the CRC32 of the program, the plugin and its CRC32, the code pages and the language, so a changed plugin or another build of `xedit` never reads a stale file; xEdit's own files carry xEdit's CRC32 and are not read by the CLI, but the format is the same (renamed, each reads the other's). Delete the folder to start over.
- Morrowind has no reference information (xEdit builds none): `referenced_by` is always empty there.

```
xedit --game sse --load "<Data>\Update.esm" --cache-path "<scratch>\cache" refs get 0001A332 --limit 20
xedit --json --game fo4 --load "<Data>\DLCRobot.esm" --dont-cache refs build
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

## Cleaning

`clean` is xEdit's plugin cleaning on one loaded plugin (`--file`, or the only plugin in `--load`); load the plugin with `--load` and its masters load with it.

- `--itm` removes the records whose conflict status is "identical to master" (`ctIdenticalToMaster`, a navigation mesh that is only a benign conflict too), walking the plugin from its last record as the GUI does, so a cell or worldspace goes when all its children went, and every group that ends up empty goes with them. The `itm` count is xEdit's: it counts the removed groups too. A record injected into a master's FormID space is never removed.
- `--udr` undeletes the deleted placed references (`REFR`, `ACHR`, `ACRE`, `PGRE`, `PMIS` and the Skyrim projectiles) and disables them: the reference gets the data of its master again (moving to the master's cell when it was moved), is set initially disabled, gets the player as an opposite enable parent (`XESP`), loses its enable parent and teleport, and a reference that is not persistent moves to z -30000 (Fallout 3 and New Vegas keep the position, as xEdit's defaults). Deleted navigation meshes can not be undeleted (`deleted_navmeshes`, xEdit's "nav" count), nor can injected references, references without a base record or New Vegas trees with LOD (`not_undeleted`).
- `--quick` is xEdit's `-quickautoclean`: UDR then ITM, the plugin saved (to `--output`, else over the loaded file with a backup in `<AppName>Edit Backups` unless `--no-backup`), and again while a pass changed the plugin, at most three passes (the third is not saved, as upstream). It loads the plugins as that mode does: the full record definitions (not the simple ones of the other commands), the PNAM of the topic responses filled in where the game sorts them (`-FillPNAM`), no `INOM`/`INOA` lists on the topics. Those settings change the comparison, so only `xedit clean --quick` (or a `serve` session started for it) gives xEdit's quick clean result; a session loaded otherwise adds a warning to `messages`. The global `--fill-pnam` turns on the PNAM fill for any command.
- `--itm` and `--udr` without `--quick` change memory only; save with `save` in the same `batch` or `serve` session. With `--dry-run`, every mode counts what it would clean in one pass and changes nothing.
- The response has one entry per pass in `passes` (the filter's node counts, `udr` and `itm` with `processed`, `count`, the `records` cleaned and the `skipped` ones, and `saved` for a quick save), the totals `itm`, `udr` and `deleted_navmeshes` (what xEdit reports to LOOT), `unsaved`, and `messages`, the lines xEdit writes to its message log (`Removing: ...`, `Undeleting: ...`, `Skipping: ...` with the GUI's record names).
- `records cleanup-injected` takes the records to clean up (all records of the plugin that refer to injected records of a plugin that is not one of its masters when none is named). The records that refer to the injected records of exactly one plugin, the plugin of the first such record, are copied into that plugin as overrides (its missing masters are added, without xEdit's question) and lose those references in their own plugin; `changed_files` names the plugins to save. Records of other plugins are `skipped`.

```
xedit --json --edit --game sse --load "<Data>\Dawnguard.esm" clean --quick --output "<somewhere>\Dawnguard.esm"
xedit --json --game fo4 --load "<Data>\DLCRobot.esm" clean --itm --udr --dry-run
```

## Merged patch

`patch merged <name>` is xEdit's "Create Merged Patch" over every loaded plugin. Load the whole load order with `--load` (in load order), then:

```
xedit --json --edit --game fnv --load "<Data>\FalloutNV.esm" --load "<Data>\DeadMoney.esm" ... patch merged "Merged Patch"
xedit --json --game sse --load ... patch merged "Merged Patch" --dry-run
```

- The name gets `.esp` (a `.esp`, `.esm` or `.esl` extension is dropped first), and the plugin is made in the data folder like xEdit's `AddNewFile`; a file of that name there or in the session is `file_exists`. Every loaded file becomes a master and the unused ones are cleaned at the end. UPSTREAM-QUIRK: the game master is listed twice (`AddNewFile` adds it, then the list of every loaded file adds it again), and the patch's FormIDs of the game master point to the second entry, as in xEdit's patches.
- What is merged, as in xEdit 4.1.5q: the records with at least two overrides whose lists the overrides changed in different ways: leveled list entries (`LVLI`, `LVLC`, `LVLN`, `LVSP`, with the counter `LLCT`), container items (`COCT`), faction relations, the hairs, eyes and spells of a race, form lists (only those whose array is sorted, or whose editor ID ends in `OrderedList`, where overrides may only append), creature items and factions, the topics' added quests (New Vegas only), the items, factions, spells, perks and keywords of NPCs (head parts up to Fallout New Vegas), and from Skyrim on the keywords of the item and effect records. Only sorted arrays are merged; entries are compared by their extended sort keys. The keywords of the Skyrim and later records sit in a `Keywords` structure, where xEdit's lookup of `KWDA - Keywords` on the record finds nothing, so they are not merged, in xEdit as here.
- The patch copies the winning override of each merged record and replaces its lists with the merged ones. Conflict status and mod groups play no part (xEdit 4.1.5q reads the overrides directly); a patch made with mod groups active is the same.
- For Skyrim, Fallout 4, Fallout 76 and Starfield xEdit asks first whether to go on with a merge it calls unsupported; the command goes on and returns the question as `warning`.
- The response lists `records` (FormID, signature, name, the plugin of the winning override, and per merged list the entries of the merged and of the winning list), `checked` (records with two overrides or more), `masters` after the clean, `messages` (xEdit's log lines: `Error: Can't merge faulty ordered list ...` and the errors of records that could not be merged) and `saved`. `--dry-run` reports the records and lists and makes no plugin; `--no-save` keeps the patch in memory (for `batch` and `serve`, where `save --file "<name>.esp"` follows).

## Localization

A localized plugin (the localized flag of its header; Skyrim, Fallout 4, Fallout 76 and Starfield) holds string IDs where other plugins hold text, and the texts live in its three string tables, `Strings\<plugin>_<language>.STRINGS`, `.DLSTRINGS` (descriptions, book texts, quest log entries) and `.ILSTRINGS` (dialogue), loose or in the game's archives. The commands work as xEdit does:

- **Editing a string.** `elements set` on a localized string of a localized plugin changes the text in the table (the ID stays); a string without an ID (ID 0, an empty text) or with an ID the table lacks gets a new string with the next free ID of the plugin's three tables (`AddValue`). `STRINGID:<hex>` as the value sets the ID itself. `records copy` into a localized plugin assigns the copied strings as text, so they become new strings of the target plugin's tables. `localization set` changes a string of a table directly, by table and ID or through an element; `--editor-text` stores the text as xEdit's editor memo gives it (CR LF line breaks and one at the end).
- **Tables load lazily, as in xEdit.** A plugin's tables load on the first lookup of a string with an ID (any read of a name or a localized element). A new string for a plugin whose tables are not loaded makes three new, empty tables (an xEdit quirk): saving them would replace the plugin's real tables. Read a localized value of the plugin (or run `localization files`) before adding strings to it.
- **Saving.** `save` writes the plugin and every modified table of it to the `Strings` folder next to the output (`strings` in the response, with a backup of an existing table in `<AppName>Edit Backups`). A table is written in xEdit's layout, every string once and in table order, so a game's own table, which shares the text of equal strings, grows when it is saved even unchanged; xEdit writes the same bytes.
- **Localize and delocalize.** `localization localize` gives every localized string of a plugin that is not localized a new ID in new tables (in xEdit's order, the last record first) and sets the flag; with `--translate-from`/`--translate-to` (tables of the data folder's `Strings` folder, in pairs) a text found in the "from" tables takes the string at the same position of the "to" tables. `localization delocalize` puts the texts into the plugin and clears the flag. xEdit closes itself after either; save the plugin and load it again before editing further. The game master can not be (de)localized. A dry run counts `localizable` (and `translated`) strings.
- **Language.** `--language` (xEdit's `-l:`) picks the tables' language at load (`English`, `En`, `French`, ...; the game's default otherwise). `localization language` switches in a session and is refused with `unsaved_strings` while a table has unsaved changes, unless `--discard`.
- **Translate mode.** `--translate` loads in xEdit's translate mode: only translatable elements take part in the comparison (`conflicts`, `compare`), `elements set` edits only them, and the commands that change the structure of a plugin (`records copy|delete`, `elements add|remove`, `formids change|renumber`, `masters add|sort|clean`, `clean`, `records cleanup-injected`) fail with `translate_mode`.
- Error codes: `unknown_table` (not a loaded table: see `localization files`), `unknown_string` (no such ID in the table), `not_localized` (the element is no localized string of a localized plugin), `invalid_state` (localizing a localized plugin, or the reverse), `unsaved_strings`, `translate_mode`.

```
xedit --json --game sse --load "<Data>\Dawnguard.esm" localization get --form-id 02000800 --path FULL
xedit --json --edit --game sse --load "<Data>\ccBGSSSE025-AdvDSGS.esm" batch delocalize.json
```

## Checking for errors

`check` is xEdit's "Check for Errors" (`files.check`). It walks every element below each checked node in order and asks it for `Check`: an unresolved FormID (`[01000ABC] <Error: Could not be resolved>`), a FormID of a record type the field does not take (`Found a GMST reference, expected: ARMO,LVLI`), a `NULL` where none is allowed, an enum or flag value without a name (`<Unknown: 2 $2>`), missing required members, a deleted record or partial form that still has data, a new record in an update module, an object ID beyond a light (`FFF`) or medium module's range, a FormID that differs from its fixed FormID (`HITME`), data shorter than its definition, and the checks of the game's definitions (the story manager of a quest, the variables of a script, the face dials of a Starfield NPC and so on).

- Without `--file` and `--record` it checks the plugins given with `--load`; `--last` checks the last file of the load order as the `-CheckForErrors` mode does (with only a game master loaded that is the file of the hardcoded records, `[00] <game>.exe`).
- `xedit check` loads the plugins as xEdit's `-CheckForErrors` mode does: no internal edits of the load (`wbAllowInternalEdit` off; the records still get the fixes of their init, because the GUI checks only where editing is allowed) and no reference information (`wbBuildRefs` off, so the story manager check of a quest is skipped). Through `call files.check`, `batch`, `serve` or `mcp` a session checks as the edit mode's menu item does: the reference index is built first.
- Text output: `Start: Checking for Errors`, `Checking for Errors in <node>`, then per record with errors its name and a line `    <path> -> <error>` per error (the path starts at the record's signature: `OTFT \ INAM - Items \ Item`), and `Done: Checking for Errors, Processed Records: N, Errors found: M` (records with errors). The messages a record logs while it is built (`Error: record STAT contains unexpected (or out of order) subrecord ...`, `Errors were found in: ...`, `Contained subrecords: ...`) come where xEdit logs them, twice, as xEdit builds a record twice during the check. `--json` gives `checked`, `errors_found`, `exit_code` (the errors found, at most 127, the exit code of `-CheckForErrors`), `records` (name, FormID, signature, file and the errors of each) and `messages` (the lines above). The command reads; it changes nothing and needs no `--edit`.
- The records are checked on all CPUs with the same output as on one thread.

```
xedit --game sse --load "<Data>\Dawnguard.esm" check
xedit --json --game fo4 --load "<Data>\DLCRobot.esm" check --record 01000F99
```

## Saving a plugin

`save` runs `PrepareSave` and `WriteToStream` as upstream does: every unmodified record is copied as loaded, a modified record is rebuilt from its elements (and compressed again when its flag says so), and the file's CRC32 is computed on the result. The response reports `bytes`, `crc32`, `loaded_crc32`, `changed`, `written` and `backup`. A save of an untouched plugin equals the bytes xEdit's own GUI saves for every plugin of the corpus that the oracle saves (239 of 249 plugins; the rest are refusals the oracle gives too, and the Morrowind masters, which it can not save). That is not always the input file: xEdit drops some data on load (the `OFST` of a worldspace) and rewrites the header (the `HEDR` record count, `INCC`, the `ONAM` list of a master, the ESM flag of an `.esm`).

- `--dry-run` builds the file and reports, writing nothing; use it to see `changed` and the refusal a save would give.
- `--output` writes somewhere else than the loaded path. Never point it into a game's `Data` folder while testing; the harness writes into the parity cache.
- Error codes: `save_refused` carries an upstream `PrepareSave` message, which the oracle gives for the same file (a Starfield blueprint module, a record in the wrong group, an `.esp` master where the game forbids it, an official Starfield module whose header the save would have to edit: `[TES4:00000000] can not be edited`). The messages of a save name records as the xEdit GUI does (`EditorID "Name" [SIG:FormID]`), so they read like the GUI's; the other commands keep xDump's names.
- Only the worldspace records are initialized by a save of an unmodified file (upstream drops their `OFST` subrecord and marks their children modified), so a big master saves in seconds; the children of its worldspaces are rebuilt record by record.

## Tool modes

xEdit is started in a tool mode: the executable name (`SSEEditQuickAutoClean.exe`, `SSEEditLODGen.exe`) or a switch selects it, and the mode decides how the plugins load and what runs over them once they are loaded. `xedit tool modes` lists all seventeen with the switch of each; `xedit tool run <mode>` runs one. A mode that changes things loads with its own settings, acts over the loaded files and saves everything it changed, each plugin to its own path (with a backup, unless `--no-backup`), as the GUI's `SaveChanged` at the end of an auto mode does.

| Mode | What it does over the loaded plugins |
|---|---|
| `masterupdate` | Sets the ESM flag of every loaded plugin that is not one, and marks the header of the files that have masters modified so their `ONAM` list is rebuilt (`-filteronam` follows the switch). Fallout 3 and New Vegas only, as in xEdit. Saves every plugin it changed. |
| `masterrestore` | Clears the ESM flag of the `.esp` files that have it (Fallout 3 and New Vegas only, as in xEdit). |
| `clearesm` | The same clearing, in every game (the `-clearESM` mode). |
| `setesm` | **Changes nothing**: the release 4.1.5q has no branch for the mode in its auto mode dispatch (an upstream bug this port reproduces; the result says so). The load just sets the ONAM filter. Use `files flags --esm true` to set the flag. |
| `onamupdate` | Marks the header of every editable plugin with masters modified, so its ONAM list is rebuilt (`-onamupdate`, the Skyrim games). |
| `sortandcleanmasters` | Sorts the masters of the named plugin by load order and removes the unused ones. |
| `generateseq` | Writes `<data>\Seq\<plugin>.seq` with the fixed FormIDs of the start-game-enabled quests the plugin adds (or sets the flag on), as the edit mode's `-generateseq:<plugin>` does; `--seq-path` puts them elsewhere. |
| `checkforerrors` | xEdit's "Check for Errors" on the last file of the load order; the response's `exit_code` is the count, at most 127 (`check --last` does the same). |
| `checkforitm`, `checkfordr` | Count the records identical to their master, or the deleted references, of the last file of the load order and change nothing (the `Counting`/`Counted` lines of xEdit's log); `exit_code` is the count. |
| `export` | xDump's `-export RAW`: the profile of the game's record definitions. `--format UESPWIKI` writes the wiki tables instead; the profile file goes to `--output` (default `<AppName>ExportPlugins.txt` next to the program, as xDump writes it) and the structure of the definitions comes back as the response's `text`. |
| `edit`, `view`, `translate` | Loading modes: `edit` is the default, `view` loads read-only and `translate` is `--translate`. They report what they are. |

- The modes reachable as their own commands are not run twice: `dump` points at `xedit dump`, `lodgen` (phase 5) and `script` (phase 6) are accepted and leave the work to those phases, and `quickclean`/`quickautoclean` are `xedit clean --quick`.
- `--dry-run` reports what the mode would change without changing anything; for `export` that means it writes nothing and still returns the text.
- The modes take (and the modes of the GUI take) the switches of `xeInit.pas` too: `-filteronam`, `-FixPersistence`, `-alwayssaveonam`, `-IKnowWhatImDoing` and the switches it unlocks, `-FillPNAM`, `-sortinfo`, `-nobuildrefs`, `-fixup`/`-nofixup`. The ones the port reads and does not act on are named in the result.

**The command line of xEdit itself.** Mod managers start xEdit with its own switches (`SSEEdit.exe -quickautoclean -autoexit -autoload "<plugin>"`, `-IKnowWhatImDoing`, `-D:<data>`, `-P:<plugins.txt>`), and those are not the CLI's syntax (`--game`, `--load`). The port reads them as xEdit does: a command line that holds one of them is taken as a tool mode invocation — the tool mode, the game (`-SSE`, `-FO4`, ... or the executable name), the plugin list (`-P:` or the plugin named as a parameter), the data folder (`-D:`), the settings above, and the switches the port does not act on, which it names on stderr. The mode runs, its message log goes to stdout and the exit code is what xEdit exits with (the count of the check modes, at most 127). `-R:<file>` writes the log to a file.

```
xedit -SSE -D:"<Data>" -P:"<plugins.txt>" -quickautoclean -autoexit
xedit -FO4 -D:"<Data>" -checkforitm "<plugin>.esp"; echo $?   # the ITM count
xedit -TES4 -D:"<Data>" -P:"<plugins.txt>" -quickedit:"<plugin>.esp" -autoexit
```

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

Without `--json` a command prints its result as pretty JSON and an error as `error: <code>: <message>` on stderr with exit code 1. With `--json` it prints one envelope, `{"ok":true,"result":...}` or `{"ok":false,"error":{"code":"...","message":"..."}}`, always on stdout; `batch` puts one envelope per command in the `result` array (`{"ok", "command", "result" or "error"}`). Error codes are stable: `no_session` (no `--game`), `unknown_file`, `ambiguous_file`, `unknown_mod_group`, `ambiguous_mod_group`, `unknown_record`, `unknown_element`, `invalid_params`, `load_failed`, `unknown_command`, `edit_required`, `edit_failed`, `not_editable`, `not_removable`, `save_refused`, `unsupported`, `io`, `internal`, and the localization codes `unknown_table`, `unknown_string`, `not_localized`, `invalid_state`, `unsaved_strings`, `translate_mode`, and `file_exists` (a new plugin whose name the data folder or the session has).
Without `--json` a command prints its result as pretty JSON and an error as `error: <code>: <message>` on stderr with exit code 1. With `--json` it prints one envelope, `{"ok":true,"result":...}` or `{"ok":false,"error":{"code":"...","message":"..."}}`, always on stdout; `batch` puts one envelope per command in the `result` array (`{"ok", "command", "result" or "error"}`). Error codes are stable: `no_session` (no `--game`), `unknown_file`, `ambiguous_file`, `unknown_mod_group`, `ambiguous_mod_group`, `unknown_record`, `unknown_element`, `invalid_params`, `load_failed`, `unknown_command`, `edit_required`, `edit_failed`, `not_editable`, `not_removable`, `save_refused`, `unsupported`, `io`, `internal`, and the localization codes `unknown_table`, `unknown_string`, `not_localized`, `invalid_state`, `unsaved_strings`, `translate_mode`.
Without `--json` a command prints its result as pretty JSON and an error as `error: <code>: <message>` on stderr with exit code 1. With `--json` it prints one envelope, `{"ok":true,"result":...}` or `{"ok":false,"error":{"code":"...","message":"..."}}`, always on stdout; `batch` puts one envelope per command in the `result` array (`{"ok", "command", "result" or "error"}`). Error codes are stable: `no_session` (no `--game`), `unknown_file`, `ambiguous_file`, `unknown_mod_group`, `ambiguous_mod_group`, `unknown_record`, `unknown_element`, `invalid_params`, `load_failed`, `unknown_command`, `edit_required`, `edit_failed`, `not_editable`, `not_removable`, `save_refused`, `unsupported`, `io`, `internal`.
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

Behaviour a user can meet, as of phase 4 step 9. Each is an upstream behaviour not ported yet; say so rather than work around it silently.

- **Strings.** The Fallout 4 DLC keep their string tables in `<plugin> - Main.ba2`, which the load does not read (it reads loose tables and `<plugin>.ba2`, `<plugin> - Interface.ba2`, `<plugin> - Localization.ba2`), so their strings show `<Error: No strings file ...>` unless the tables are loose in `Data\Strings`; editing such a string then makes new, empty tables. The `.cpoverride` code page of a table is read; the archive's files are listed sorted, not in archive order.
- **Copy over an existing override.** xEdit's "...with overwriting" (`aAllowOverwrite`) is not ported: `records copy` returns the existing override unchanged. A partial form (`MakePartialForm`), template elements and aligned arrays can not be copied either.
- **Sorted arrays.** A record that is rebuilt on save sorts its sorted arrays as upstream does (the subrecord arrays, and the sorted array values, `srsSorted` and `arrSorted`, at their first read by index or write after a change), and a master update (`masters add|sort|clean`) keeps them in their order as xEdit does; a modified topic group of Fallout 4 and later sorts its responses by FormID as xEdit does; in the games that sort them by their `PNAM` (Oblivion to Skyrim) they are not sorted again after their FormIDs change, so such a save can differ from xEdit's in their order.
- **References.** The index has no "reachable" information (xEdit's "Build Reachable Info"), and a record whose references come from the cache takes only its editor ID and full name from it (its base record, grid cell and GUI names are read from the record when needed). The editor ID index of a file does not learn the records an edit adds.
- **Flags** are not child elements of their value in `records get`; `compare` shows them as rows, as the view does.
- **Mod groups.** The named selection presets of xEdit's selection dialog are not ported. The modules that are not loaded are ordered by name, where xEdit uses its load order, which decides only the order of their `.modgroups` files (and of groups of the same name in them).
- **Conflicts.** Records the GUI user hid and the compare-to load (`Compare to...`) do not exist; the raw data compare (`wbCompareRawData`) is not ported; compare of selected records of different FormIDs (`Compare Selected`) and the script function `ConflictAllForElements` have no command yet.
- **Cleaning.** `clean` cleans one plugin per call. xEdit's `-AllowMakePartial` (partial forms of cells and worldspaces with children) is not ported, the UDR options of xEdit's Options dialog are fixed at their defaults, and the LOOT dirty-information report is the counts of the response. In the oracle check a few saved plugins still differ from xEdit's in the bytes of records the clean did not touch (see `docs/PLAN.md`, owed from step 5).
- **Check for errors.** The script function `Check` of one element has no command.
- **Tool modes.** `-setesm` changes nothing: xEdit 4.1.5q's auto mode dispatch has no branch for it, and the port follows the release (`files flags --esm true` sets the flag). The `-lodgen` and `-script` modes are accepted and run nothing (phases 5 and 6). Of the legacy switches, the ones the port reads and does not act on (the temporary folder, the game ini, the code pages, the archive loading, the pseudo light/medium/update flags and the rest, see `coverage/ledger.toml`) are named on stderr. The plugin list of `-P:` is read for the modules it marks active; the game's own `Plugins.txt` is not found by itself, so pass `-P:` (or a plugin name) and `-D:`. The save chapters are not exported (`-export` writes the plugin definitions only).
- **Morrowind.** Plugins load and dump, but the save stops at `must have a FormID`: the identity FormID of a TES3 record is not ported. The 4.1.5q oracle saves no Morrowind plugin either (its GUI runs Morrowind in view mode), so there is nothing to compare with.
- **New files.** `files new` makes `.esp` and `.esl` plugins from the light and medium flags; the module templates of xEdit's "Create New File" dialog are not ported.
- **Sessions.** One session per process; `serve` and `mcp` can not load other plugins later, and a named pipe serves one client at a time.
- **Starfield.** The complex FileIDs (light and medium masters with slots of their own) are followed by the master functions and the FormID lookups, but FormID changes and renumbering across masters are unchecked there. The oracle refuses to save the official Starfield modules whose header the save would edit, and so does the port.
- **Oblivion saves** do not read (an upstream limit, see "Saves").
- **Mesh optimizing.** `SpellOptimize`, `SpellStripify` and `SpellTriangulate` of a NIF are ported (`wbMeshOptimize`) but have no command of their own; `sniff run "Optimize mesh"` runs them on a folder.
- **Sniff.** `Update MOPP code` is not ported (`sniff list` gives its `not_ported` reason; it needs `NifMopp.dll`), and `ProcCollapseLinksArrays` has no operation in the 4.1.5q form; see "Sniff".
- **Texture archives.** Packing and extracting work for `Fallout 4 DDS` and `Starfield DDS` archives. The Xbox 360 conversion of `BSArchPro` (`xtexconv.exe`) is part of the GUI tool and not ported, so the archive target is always the PC. A texture whose data ends before its last mipmap is cut where the data ends, where `BSArch.exe` reads past its buffer in the route without a split.
- **Vector instructions.** The pixel conversion of the texture code (24 to 32 bit) runs on SSSE3 or AVX2 and the half-float array conversions (`xedit_core::half_float`) on F16C where the machine has them, each with a scalar version that gives the same bytes; `XEDIT_SIMD=off` takes the scalar route everywhere.
