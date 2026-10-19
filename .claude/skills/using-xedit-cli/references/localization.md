## Localization

A localized plugin (the localized flag of its header; Skyrim, Fallout 4, Fallout 76 and Starfield) holds string IDs where other plugins hold text, and the texts live in its three string tables, `Strings\<plugin>_<language>.STRINGS`, `.DLSTRINGS` (descriptions, book texts, quest log entries) and `.ILSTRINGS` (dialogue), loose or in the game's archives. The commands work as xEdit does:

- **Editing a string.** `elements set` on a localized string of a localized plugin changes the text in the table (the ID stays); a string without an ID (ID 0, an empty text) or with an ID the table lacks gets a new string with the next free ID of the plugin's three tables (`AddValue`). `STRINGID:<hex>` as the value sets the ID itself. `records copy` into a localized plugin assigns the copied strings as text, so they become new strings of the target plugin's tables. `localization set` changes a string of a table directly, by table and ID or through an element; `--editor-text` stores the text as xEdit's editor memo gives it (CR LF line breaks and one at the end).
- **Tables load lazily, as in xEdit.** A plugin's tables load on the first lookup of a string with an ID (any read of a name or a localized element; changing an existing string reads it first).

A new string for a localized plugin whose tables are not loaded yet loads them first, so it takes the next ID after the plugin's strings and only its own table is saved.

This is a deliberate divergence (UPSTREAM-BUG, issue #75): xEdit makes three new, empty tables then, gives the string ID 1, and its save replaces the plugin's real tables (Skyrim SE's `Update.esm` keeps 1 of its 1,593 strings).

The difference is checked by `parity oracle-edit` (`fo4-strings-archive-unloaded` and `sse-copy-npc-vmad-shown` are `known-difference`, and their saves equal the release's saves of `fo4-strings-archive-loaded` and `sse-copy-npc-vmad-shown-loaded`, where a name is read first).

A plugin that is not localized still gets empty new tables (localizing one).
- **Saving.** `save` writes the plugin and every modified table of it to the `Strings` folder next to the output (`strings` in the response, with a backup of an existing table in `<AppName>Edit Backups`). A table is written in xEdit's layout, every string once and in table order, so a game's own table, which shares the text of equal strings, grows when it is saved even unchanged; xEdit writes the same bytes.
- **Localize and delocalize.** `localization localize` gives every localized string of a plugin that is not localized a new ID in new tables (in xEdit's order, the last record first) and sets the flag; with `--translate-from`/`--translate-to` (tables of the data folder's `Strings` folder, in pairs) a text found in the "from" tables takes the string at the same position of the "to" tables. `localization delocalize` puts the texts into the plugin and clears the flag. xEdit closes itself after either; save the plugin and load it again before editing further.

The game master can not be (de)localized.

A dry run counts `localizable` (and `translated`) strings.
- **Language.** `--language` (xEdit's `-l:`) picks the tables' language at load (`English`, `En`, `French`, ...; the game's default otherwise). `localization language` switches in a session and is refused with `unsaved_strings` while a table has unsaved changes, unless `--discard`.
- **Translate mode.** `--translate` loads in xEdit's translate mode: only translatable elements take part in the comparison (`conflicts`, `compare`), `elements set` edits only them, and the commands that change the structure of a plugin (`records copy|delete`, `elements add|remove`, `formids change|renumber`, `masters add|sort|clean`, `clean`, `records cleanup-injected`) fail with `translate_mode`.
- Error codes: `unknown_table` (not a loaded table: see `localization files`), `unknown_string` (no such ID in the table), `not_localized` (the element is no localized string of a localized plugin), `invalid_state` (localizing a localized plugin, or the reverse), `unsaved_strings`, `translate_mode`.

```
xedit --json --game sse --load "<Data>\Dawnguard.esm" localization get --form-id 02000800 --path FULL
xedit --json --edit --game sse --load "<Data>\ccBGSSSE025-AdvDSGS.esm" batch delocalize.json
```
