## Commands

FormIDs are load order FormIDs in hexadecimal, as xEdit shows them: `01003274` is object `003274` of the file in slot 1. `--file` names a loaded plugin; without it, `records list`, `records find` and the plugin commands need exactly one plugin in `--load` (else `ambiguous_file`), and `records get`, `elements get` and the element commands see the record from the last loaded plugin (the winning override for that plugin).

Every command below that mutates takes `--dry-run`, and without `--edit` only a dry run is accepted.

Inspection (never mutates):

| CLI | Registry name | What it does |
|---|---|---|
| `session info` | `session.info` | Game tag, game name, data folder and the loaded files in load order. |
| `files list [--name-filter TEXT] [--regex]` | `files.list` | Every loaded file with load order (`load_order`, the position counting every file) and the slot xEdit shows before its name (`file_id`: `05`, `FE 001` for a light plugin), masters, record count, ESM and localized flags; `--name-filter` is the navigation tree's "Filter by filename" (a case-insensitive substring, or a `preCaseLess` PCRE with `--regex`). |
| `session load [--plugin P]... [--mode M] [--compare-to F] [--force] [--dry-run]` | `session.load` | Loads another plugin list into the running session, or the same one with a tool mode's load settings (`--mode quickclean`, `quickautoclean`, `checkforerrors`, `translate`), as `xeInit.pas` applies them before anything loads. The plugins of a `serve` or MCP session are otherwise fixed at startup. A load discards the unsaved changes of the loaded files unless `--force`; a dry run reports the plugins, the mode's settings and what a reload would discard. `--compare-to F` is the tree's "Compare to...": the file loads as the last plugin's compare load (it has to sit in the same folder), and this request's file wins over the global `--compare-to`. Needs `--edit` unless `--dry-run`. See "Daemon and MCP server". |
| `records list [--file F] [--signature SIG] [--offset N] [--limit N]` | `records.list` | Records of one plugin in file order; `total` counts the matches before paging. |
| `records find [--file F] [--signature SIG] [--editor-id TEXT] [--name TEXT] [--limit N]` | `records.find` | Records whose editor ID or display name contains the text, compared without case. |
| `records get <FormID> [--file F] [--depth N]` | `records.get` | One record with its elements as a tree. |
| `elements get <FormID> <path> [--file F] [--depth N]` | `elements.get` | One element of a record by path. |
| `conflicts [--file F]... [--signature SIG]... [--min-conflict-all CA] [--conflict-this CT]... [--include-single] [--master-and-leafs] [--quick-show-conflicts] [--modgroups NAME]... [--all-modgroups] [--saved-modgroups] [--offset N] [--limit N]` | `conflicts.list` | The conflict status of the records of the loaded plugins, and per file the counts and the highest status. See "Conflicts" and "Mod groups". |
| `compare-selected <FormID>... [--file F] [--master-and-leafs] [--hide-no-conflict] [--include-hidden] [--raw-data] [--modgroups NAME]... [--all-modgroups] [--saved-modgroups] [--view-filter-name TEXT] [--view-filter-value TEXT] [--view-filter-or] [--keep-children] [--keep-siblings] [--keep-parents-siblings]` | `records.compare_selected` | Records of different FormIDs side by side, as the GUI compares the selected navigation tree nodes ("Compare Selected"): the records are the columns and their members the rows (`ComparingSiblings`, `DoSetActiveRecord` with several records). The GUI's own limit (five records) is not applied. |
| `conflict-for-elements --form-id ID --path P [--file F] --other-form-id ID --other-path P [--other-file F] [--sibling-compare] [--injected]` | `conflicts.for_elements` | The conflict of two elements, as the script function `ConflictAllForElements(a, b, aSiblingCompare, aInjected)` reports it (`ConflictLevelForNodeDatas`, `ConflictLevelForChildNodeDatas` for a container). |
| `compare <FormID> [--file F] [--master-and-leafs] [--hide-no-conflict] [--include-hidden] [--raw-data] [--modgroups NAME]... [--all-modgroups] [--saved-modgroups] [--limit N] [--depth N] [--compact]` | `records.compare` | The records of a FormID side by side, row by row, with the conflict status of each row and cell, as the view tab shows them. A record with many overrides and large arrays (a race) makes tens of megabytes: `--limit` caps the rows (the whole tree, a row before its children), `--depth 1` lists the members without their children, and `--compact` keeps only each cell's value and conflict statuses; with `--limit` or `--depth` the response's `truncated` gives `total_rows` and `listed_rows`. Showing the record aligns its sorted and aligned arrays: the view's sort orders are written to the entries, so a later `elements assign` at a position creates the entry there (see "Adding, removing and copying"). |
| `modgroups list [--all]` | `modgroups.list` | The valid mod groups by name (`--all`: every group of every file) with their items, validity, messages and whether the saved selection has them. |
| `modgroups show <name> [--file F]` | `modgroups.show` | One mod group with the state of each item: loaded, load order, CRC32s, current CRC32, valid. |
| `elements check <FormID> <path> [--file F]` | `elements.check` | The `Check` of one element and the errors below it, as "Check for Errors" on a selected node reports them (the script function `Check`). |
| `check [--file F]... [--record FormID]... [--record-file F] [--last]` | `files.check` | xEdit's "Check for Errors": every element of the plugins (or files, or records) asked for its errors, printed as xEdit's message log shows them (with `--json` the records and errors as data). See "Checking for errors". |
| `dump --check --game G <plugin>` | (none) | `xDump.exe -check`: the errors of every element of the plugin in xDump's form (children from the last, `Above errors were found in: ...` below each container), to stdout. |
| `refs get <FormID> [--file F] [--offset N] [--limit N]` | `refs.get` | The records that refer to a record (xEdit's "Referenced By" tab) and the FormIDs it refers to. Builds the reference index first. |
| `refs build [--file F] [--only-load]` | `refs.build` | Builds the reference index of the loaded files, or loads it from the reference cache, and saves the cache (`BuildOrLoadRef`). |
| `refs dump` | (none) | The referenced-by lists of every record as text, for the parity check. |
| `modules list [--all] [--active] [--masters-present]` | `modules.list` | The modules of the data folder in `wbModulesByLoadOrder` order, as the module selection dialog and the mod group editor's Insert list them: name, the ESM/light/medium/update/localized flags, active and valid, the masters, the load order and FileID, the path, the size, the write time and the CRC32. `--all` includes the game's executable and a plugin whose header did not read (the dialogs list only the valid ones); `--active` and `--masters-present` keep the active modules and the ones whose masters are there. |
| `signatures list` | `signatures.list` | The record and base-record signatures of the game's definitions (`wbRecordDefs`), the filter options dialog's `clbRecordSignatures` and `clbBaseRecordSignatures`: `SIGNATURE - Name` per record definition, the base list only for definitions with `ReferenceSignatureCount > 0`, both sorted by that text. |
| `settings get [--section S] [--key K] [--type T] [--default V]` | `settings.get` | xEdit's settings file (`xEdit.ini`): the section names, a section's keys or one key. `--type string\|integer\|bool\|float\|double` reads it as the matching `TMemIniFile.Read*` (the raw text is reported either way) and `--default` is that read's default. The GUI's keys live here: `[frmMain]` placement and `pnlNavWidth`/`vstNavColumnWidth<i>`, `[ColorConflictThis]`/`[ColorConflictAll]` (a `TColor` per enum name), `[Nav] FilesSort`/`INFOSort`, `[View]`, and the `[Options]` switches `xeInit.pas` reads. |
| `call system.version` | `system.version` | The version of the build. |
| `dump --game G <plugin>` | (none) | The whole plugin as `xDump.exe` prints it, to stdout; progress goes to stderr. |
| `tool modes` | `tool.modes` | The seventeen tool modes of xEdit with the switch of each, whether the game of the session has it, what it does and what of it is missing here. |
| `saves dump --game G --data <Data> <save>` | (none) | A save or co-save as `xDump.exe -saves` prints it. The plugins the save lists load from `<Data>`. |
| `call conflicts.probe --params '{"records": [...]}'` | `conflicts.probe` | The comparison of records as the oracle probe of `cargo xtask parity conflicts --record` reads it: the master's and first override's statuses, their pair, and the two walked side by side by index with the display names, names, extended sort keys and element checks. The harness's side of the probe; `hidden` sets `esHidden` first. |
| `schema` | (none) | The registry: every command with its request and response JSON Schema. |

Changing data (`mutates`, needs `--edit` or `--dry-run`):

| CLI | Registry name | What it does |
|---|---|---|
| `elements set <FormID> <path> [<value>] [--file F] [--native] [--default]` | `elements.set` | Sets the value of one element. See "Editing an element". |
| `elements add <FormID> <name> [--path P] [--file F]` | `elements.add` | Adds a member, an array entry or a child record, as xEdit's "Add". |
| `elements remove <FormID> <path> [--file F]` | `elements.remove` | Removes an element that xEdit lets you remove. |
| `records copy <FormID> --to F [--from F] [--as-new] [--deep] [--overwrite] [--remove-deleted] [--prefix T] [--suffix T] [--prefix-remove T] [--suffix-remove T]` | `records.copy` | Copies a record into a plugin as an override or a new record, adding the masters it needs; `--overwrite` copies over the target's record. |
| `elements assign <FormID> [--path P] [--file F] [--index I] [--source-form-id FID] [--source-file F] [--source-path P] [--template T] [--only-sk]` | `elements.assign` | Assigns an element of another record, or a template of the definitions, to an element at an index (the view's drag and drop, `ElementAssign`, `TemplateAssign`). |
| `elements templates <FormID> [--path P] [--file F] [--index I]` | `elements.templates` | Lists the templates an element offers for an assign at an index (`GetAssignTemplates`). |
| `elements move <FormID> <path> [--direction up\|down] [--position N] [--file F]` | `elements.move` | Moves an entry of an unsorted array up or down (the view's Move Up and Move Down) or to a position (`InsertElement`). |
| `records set-hidden <FormID> [--file F] [--overrides] [--show]` | `records.set_hidden` | Hides a record or shows it again (the tree's Hide; `--overrides` the record and every override, the view's "Unhide all"). The comparison, the file lists and the filter's only-one mode leave hidden records out. Session state, like the compare alignment: no `--edit` needed. |
| `records delete <FormID> [--file F]` | `records.delete` | Removes the plugin's version of a record with its child group. |
| `records set-deleted <FormID> [--file F] [--undelete]` | `records.set_deleted` | Marks the plugin's version of a record deleted (`SetIsDeleted`, `TwbMainRecord.Delete`: it stays in the plugin with its child group, loses its contents; from Fallout 4 on a placed record keeps its base record) or undeletes it. |
| `formids change <FormID> [<new FormID>] [--file F] [--target-file T] [--overrides]` | `formids.change` | Gives a record a new load order FormID and updates the records that refer to it. |
| `formids renumber [--file F] [--start HEX] [--compact] [--inject-into M] [--preserve-object-ids] [--all-or-nothing]` | `formids.renumber` | Renumbers the new records of a plugin, updating the referencing records. |
| `files flags [--file F] [--esm B] [--light B] [--medium B] [--update B] [--blueprint B] [--localized B]` | `files.flags` | Sets the module flags of a plugin header; `--dry-run` alone reads them with the compatibility of the new records. |
| `masters add <name>... [--file F] [--no-sort]` | `masters.add` | Adds loaded plugins as masters and sorts the masters by load order unless `--no-sort`. |
| `masters sort [--file F]` | `masters.sort` | Sorts the masters by load order. |
| `masters clean [--file F]` | `masters.clean` | Removes the masters no FormID of the plugin points to. |
| `save [--file F] [--output PATH] [--no-backup]` | `files.save` | Writes a loaded plugin as xEdit saves it. Changes files, not data. |
| `clean [--file F]... [--itm] [--udr] [--quick] [--output PATH] [--no-backup] [--udr-xesp B] [--udr-scale B] [--udr-scale-value N] [--udr-z B] [--udr-z-value N] [--udr-mstt B] [--udr-mstt-value HEX]` | `files.clean` | Cleans one plugin or a selection of several: removes the records identical to their master (ITM), undeletes and disables the deleted references (UDR) with the options of xEdit's Cleaning tab, or runs xEdit's quick auto clean mode, which saves. See "Cleaning". |
| `call files.dirty_info` | `files.dirty_info` | The dirty information the session's cleaning steps stored, per plugin and CRC32, and its LOOT (and Oblivion's BOSS) masterlist entries. |
| `files compare-to <file> --path PATH [--dry-run]` | `files.compare_to` | Copies the file at `--path` into the folder of the loaded plugin `<file>` and loads the copy in the place of that plugin, at the plugin's load order, as a compare load (xEdit's "Compare to..."). The copy takes the plugin's load order and the plugin becomes its last master. The copy has to sit in the same folder as the plugin. Needs `--edit` unless `--dry-run`. |
| `files new <name> [--light] [--medium] [--template T]` | `files.new` | A new empty plugin in the data folder with the game master as its master (xEdit's `AddNewFile`); `--template` takes one of the module templates of xEdit's "Create New File" dialog (`esp`, `esp-esm`, `esp-light`, `esp-esm-light`, `esp-medium`, `esp-esm-medium`, `esp-update`, `esp-esm-update`, `esm`, `esm-light`, `esm-medium`, `esm-update`), which the game's modules offer. Save it with `save --file <name>` in the same `batch` or `serve` session. `file_exists` when the data folder or the session has it. |
| `patch merged <name> [--no-save] [--output PATH] [--no-backup]` | `patch.merged` | xEdit's "Create Merged Patch": a new `.esp` that merges the lists several plugins change, saved into the data folder (or `--output`). See "Merged patch". |
| `records cleanup-injected [<FormID>...] [--file F]` | `records.cleanup_injected` | Copies the records that refer to injected records of a plugin that is not their master into that plugin and removes those references from the originals (xEdit's "Cleanup injected records"). |
| `settings set --section S --key K --value V [--type T] [--dry-run]` | `settings.set` | Writes one value of xEdit's settings file as the GUI writes it (`--type` string, integer, bool (`1`/`0`), float or double (`FloatToStr`)); a missing section is added at the end. Reports the old and new text and whether it changed. Needs `--edit` unless `--dry-run`. |
| `modgroups select [<name>...] [--saved] [--all]` | `modgroups.select` | Chooses the active mod groups and saves the selection in xEdit's settings file. Changes a file. |
| `modgroups create <name> --module M... [--item LINE]... [--include-crcs] [--add-current-crcs] [--file M]` | `modgroups.create` | Adds a mod group to the `.modgroups` file of one of its modules. Changes a file. |
| `modgroups edit <name> [--file F] [--new-name N] [--item LINE]... [--add-current-crcs]` | `modgroups.edit` | Renames a mod group or replaces its items. Changes a file. |
| `modgroups delete <name>... [--file F]` | `modgroups.delete` | Deletes mod groups. Changes files. |
| `modgroups update-crcs [--no-add] [--no-update] [--module M]... [--modgroup NAME]...` | `modgroups.update_crcs` | Adds the current CRC32s of the modules to the items of mod groups. Changes files. |
| `modgroups presets` | `modgroups.presets` | The selection presets of the mod group dialog with the saved selection and the settings section they live in. |
| `modgroups preset-load <preset>` | `modgroups.preset_load` | Loads a selection preset (or `<Last>`), saves the selection and activates it, as the dialog's Load then OK does. Changes the settings file. |
| `modgroups preset-save <preset> [--after-modal] [--confirm \| --no-confirm]` | `modgroups.preset_save` | Writes the current selection as a preset. `--after-modal` is the question the dialog asks on close after a preset was loaded (default No), `--confirm`/`--no-confirm` answer the overwrite question (default Yes, asked only when the preset exists). Changes the settings file. |
| `modgroups preset-delete <preset> [--confirm \| --no-confirm]` | `modgroups.preset_delete` | Deletes a selection preset (the question's default is Yes). Changes the settings file. |
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
| `batch <file.json or -> [--keep-going]` | Runs a JSON array of `{"command": name, "params": {...}}` in one session and prints one envelope per command; stops at the first failure unless `--keep-going`, reporting each command after it as `{"ok": false, "skipped": true}`. An unknown command name fails alone (`unknown_command`) and the batch goes on. |
| `serve [--pipe NAME]` | Keeps the session loaded and answers JSON-RPC requests, one per line, on stdio or a named pipe. See "Daemon and MCP server". |
| `mcp` | Serves the commands as MCP tools on stdio. |

Element paths use `\` between names, as in xEdit scripts: `DATA\Health`, `ACBS\Flags\Female`, `Conditions\Condition #0\CTDA\Function`. Signatures (`DNAM`) and names (`DNAM - Flags`) both work for a step, and `[n]` picks an entry by position.

A record or element node has `name`, `display_name` (only when it differs, for example a placed object with its base record), `value` (the text xEdit shows), `summary` (the `[S]:` text of containers without a value), `native` (number, boolean, string or bytes as hexadecimal) and `children`. `--depth 1` keeps the first level of children, which is enough to list the subrecords of a record.
