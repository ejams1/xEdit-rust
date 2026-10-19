## Mod groups

A mod group is a named list of modules in a `.modgroups` file that says which records a user has checked against each other: while it is active, the comparison of a FormID's records leaves out the records of the modules the group says a later module hides, and they get `ctHiddenByModGroup`.

The files are xEdit's: `<plugin>.modgroups` next to each plugin of the data folder (also of plugins that are not loaded, whose groups then count for nothing) and the program's own `<AppName>Edit.modgroups` (`--modgroups-file` names another).

Each section is a group, each line an item `[flags]file[:crc32,crc32,...]`: no flag is a module that is both a target (can be hidden) and a source (hides the targets above it); `@` target only, `#` source only, `-` neither, `+` optional, `!` forbidden (the group is invalid while it is loaded), `}` load order not checked, `{` order not checked within a block of such items.

A CRC32 list makes the item count only for a file with one of those CRC32s.

- A group is valid when every required item is loaded with a listed CRC32, no forbidden one is, the loaded items load in the group's order and at least one source has a target above it. `modgroups list` shows the valid groups of loaded modules, `--all` every group; each has `messages` (the reasons, as xEdit's log says them) and `selected`.
- **Activating.** `conflicts` and `compare` compare without mod groups unless told: `--modgroups NAME` (repeat) activates valid groups by name, `--all-modgroups` every valid group (as xEdit's `-autoload`), `--saved-modgroups` the saved selection. The result lists `mod_groups` (activated) and `mod_group_hides` (each module with the modules whose records it hides). The first and the last record of a FormID are never hidden.
- **The saved selection** is the `[ModGroups] Selection` of xEdit's settings file (`--settings` names another; the default is xEdit's own, `<AppName>Edit.ini` next to the program, else `Plugins.<app>viewsettings` next to the game's `Plugins.txt` in `%LOCALAPPDATA%`), so the GUI and the CLI share it. `modgroups select` writes it; the other mutating `modgroups` commands end, as the GUI does, with a reload: the files read again, `validation_messages`, and the saved selection written again with only its valid groups (a group just created is added).
- **The selection presets** of the dialog live in the same file, in the section `frmModGroupSelect_ActiveModGroups_Presets` (`GetPresetsSection`: the form's name and the preset category), one `name=comma text` line per preset: `modgroups presets` lists them, `preset-save <name>` writes the current selection as one (the overwrite question only when the name exists, `--confirm`/`--no-confirm`, default Yes), `preset-load <name>` tags the preset's valid groups and then saves and activates the selection as the dialog's OK does (`<Last>` loads the saved selection instead; `preset_differs` says whether the selection now differs from the preset, which the dialog asks to update on close: `preset-save --after-modal`, whose default answer is No), and `preset-delete <name>` removes one (the question's default Yes).

The name a scenario types is what the GUI's combo box does; a name that is not a preset is `unknown_preset`.
- **Orders.** A name several groups have resolves to the first one in the dialog's list order (the list is by name, ties in file order); the mod group files of loaded and unloaded modules alike come in xEdit's load order (the game master, `Update.esm` of the Skyrim family, the official DLC, then the plugin list, the file times and the name), so the file of a duplicate name is the one the dialog would pick.

The modules of the game's Creation Club list (the `.ccc` file next to the data folder) come after the official ones in the list's order, and a loaded module's load order stands in for its `Plugins.txt` index (issue #83).
- **Writing.** The files are written as xEdit writes them: `create` appends the group after an empty line to the file of the module given with `--file` (default: the group's first loaded module), keeping its text and encoding; `edit` and `update-crcs` write the file back as an ini file (comments and blank lines gone, spaces around `=` removed, each section followed by an empty line) with the group at the end, in the ANSI code page; `delete` writes it back as an ini file in its encoding. `update-crcs` gives every item of a chosen group its module's current CRC32 (also the items of modules not chosen, as xEdit), `--no-add` and `--no-update` are xEdit's two questions (No to the first skips the second).

```
xedit --game sse --load "<Data>\Dragonborn.esm" modgroups list --all
xedit --edit --game sse --load "<Data>\Dragonborn.esm" modgroups create "DB over DG" --module Dawnguard.esm --module Dragonborn.esm --include-crcs --dry-run
xedit --game sse --load "<Data>\Dragonborn.esm" conflicts --saved-modgroups --conflict-this ctHiddenByModGroup
```
