## Cleaning

`clean` is xEdit's plugin cleaning on one loaded plugin (`--file`, or the only plugin in `--load`) or several (`--file A --file B`: one click of each step over the selected file nodes, in load order); load the plugins with `--load` and their masters load with them. `cleaning-plugins` has the options, partial forms and the dirty information.

- `--itm` removes the records whose conflict status is "identical to master" (`ctIdenticalToMaster`, a navigation mesh that is only a benign conflict too), walking the plugin from its last record as the GUI does, so a cell or worldspace goes when all its children went, and every group that ends up empty goes with them. The `itm` count is xEdit's: it counts the removed groups too. A record injected into a master's FormID space is never removed.
- `--udr` undeletes the deleted placed references (`REFR`, `ACHR`, `ACRE`, `PGRE`, `PMIS` and the Skyrim projectiles) and disables them: the reference gets the data of its master again (moving to the master's cell when it was moved), is set initially disabled, gets the player as an opposite enable parent (`XESP`), loses its enable parent and teleport, and a reference that is not persistent moves to z -30000 (Fallout 3 and New Vegas keep the position, as xEdit's defaults).

Deleted navigation meshes can not be undeleted (`deleted_navmeshes`, xEdit's "nav" count), nor can injected references, references without a base record or New Vegas trees with LOD (`not_undeleted`).
- `--quick` is xEdit's `-quickautoclean`: UDR then ITM, the plugin saved (to `--output`, else over the loaded file with a backup in `<AppName>Edit Backups` unless `--no-backup`), and again while a pass changed the plugin, at most three passes (the third is not saved, as upstream).

It loads the plugins as that mode does: the full record definitions (not the simple ones of the other commands), the PNAM of the topic responses filled in where the game sorts them (`-FillPNAM`), no `INOM`/`INOA` lists on the topics.

Those settings change the comparison, so only `xedit clean --quick` (or the legacy `-quickclean`/`-quickautoclean` command line, which loads the same way, or a `serve` session started for it) gives xEdit's quick clean result; a session loaded otherwise adds a warning to `messages`.

The global `--fill-pnam` turns on the PNAM fill for any command.
- `--itm` and `--udr` without `--quick` change memory only; save with `save` in the same `batch` or `serve` session. With `--dry-run`, every mode counts what it would clean in one pass and changes nothing.
- The response has one entry per pass in `passes` (the filter's node counts, `udr` and `itm` with `processed`, `count`, the `records` cleaned and the `skipped` ones, and `saved` for a quick save), the totals `itm`, `udr` and `deleted_navmeshes` (what xEdit reports to LOOT), `unsaved`, and `messages`, the lines xEdit writes to its message log (`Removing: ...`, `Undeleting: ...`, `Skipping: ...` with the GUI's record names).
- `records cleanup-injected` takes the records to clean up (all records of the plugin that refer to injected records of a plugin that is not one of its masters when none is named).

The records that refer to the injected records of exactly one plugin, the plugin of the first such record, are copied into that plugin as overrides (its missing masters are added, without xEdit's question) and lose those references in their own plugin; `changed_files` names the plugins to save.

Records of other plugins are `skipped`.

```
xedit --json --edit --game sse --load "<Data>\Dawnguard.esm" clean --quick --output "<somewhere>\Dawnguard.esm"
xedit --json --game fo4 --load "<Data>\DLCRobot.esm" clean --itm --udr --dry-run
```
