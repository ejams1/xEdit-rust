## Loading plugins

The global options select the game and the plugins:

```
xedit --game sse --load "<Data>\Skyrim.esm" --load "<Data>\Update.esm" <command>
xedit --game fo4 --load "<Data>\DLCRobot.esm" <command>
```

- `--compare-to <path>` is the navigation tree's "Compare to...": the file at `<path>` loads in the place of the last `--load` plugin, takes its load order slot and is compared with it record by record (`wbFile` with `aCompareTo`, `AddMaster(flCompareTo)`, `fsCompareToHasSameMasters`, `IsCompareToSame`). It has to sit in the folder of that plugin, as the GUI copies the chosen file into the data folder. The flag is not global: it belongs to the startup load, before the command (`xedit --game G --compare-to F --load P ...`); `session load --compare-to` names the compare file of the load that command makes instead. It is not editable (a save refuses it as `[TES4:00000000] can not be edited`, as the release does). A record both files hold shows 2 columns where one is the compare load, identical records use the fast `ContentEquals` path (`caNoConflict`, the original `ctMaster`, the compare load `ctIdenticalToMaster`).

It has to sit in the folder of that plugin, as the GUI copies the chosen file into the data folder; it is not editable (a save refuses it as `[TES4:00000000] can not be edited`, as the release does).

A record both files hold shows 2 columns where one is the compare load, identical records use the fast `ContentEquals` path (`caNoConflict`, the original `ctMaster`, the compare load `ctIdenticalToMaster`).
- `--source` is xEdit's `-plugins`/`-saves` (the tool source of `DetectAppMode`): a `saves` session reads no plugin and builds the *save* definitions of the game instead of the plugin ones, which `tool run export --source saves` writes.

The legacy command line takes `-saves` the same way.- `--game` takes the switch of `xDump.exe` without the dash: `tes3` (Morrowind), `tes4` (Oblivion), `fo3`, `fnv`, `tes5` (Skyrim LE), `enderal`, `sse`, `tes5vr`, `enderalse`, `fo4`, `fo4vr`, `fo76`, `sf1` (Starfield).
- `--plugins-list <path>` is xEdit's `-P:`: the plugin list the module order reads (`wbPluginsFileName`).

A module's line in it is its `Plugins.txt` index, and the modules it marks active (a `*` in the later games; every line of the older games' lists is active) are the active ones of `wbLoadModules`, together with the game master, `Update.esm` of the Skyrim games, the official DLC and the Creation Club modules.

The default is no list, so the module order falls back to the write time and the name for modules the official and Creation Club orders do not place.
- `--load` takes the full path of a plugin and may repeat; the plugins load in the order given and each takes the next load order slot after its masters. Masters load from the same folder automatically, so loading `Update.esm` also loads `Skyrim.esm`.
- The hardcoded records of the game load as a file named after the game executable (`SkyrimSE.exe`, `Fallout4.exe`), like xEdit does. It is not a plugin: it can not be saved, and it is not editable.
- Archives load as the GUI's loader loads them (`TLoaderThread`), whether or not a plugin is localized: first the archives the game inis list (`sResourceArchiveList` and the other lists of `[Archive]`, the custom ini's list replacing the game ini's of the same key), then for every module of the load order `<plugin>*.bsa`/`.ba2` in the order the folder lists them (exactly `<plugin>.bsa` for Skyrim LE and Enderal, after the archives of `<plugin>.ini` in the Skyrim games), then the data folder, which wins over every archive for a file both hold.

So the Fallout 4 DLC find their string tables in `<plugin> - Main.ba2`.

The progress log names each archive (`[DLCRobot - Main.ba2] Loading Resources.`, `Version: 8`, `Warning: <Can't find ...>`).
- `--skip-bsa` and `--force-bsa` are xEdit's `-skipbsa` and `-forcebsa` (`wbLoadBSAs`): skipped, the archives are only named (`[...] Skipped.`) and localized strings come from the loose `Strings` folder alone. Fallout 3, New Vegas and Morrowind skip by default, the others load; `LoadBSAs` of the settings file (`--settings`) applies before the switches, and the script, LOD and translate modes load whatever the switches say, the check modes (`check`, `-CheckForErrors`, `-checkforitm`, ...) skip.
- `--my-games`, `--game-ini` and `--custom-ini` are xEdit's `-M:`, `-I:` and `-CustomIni:`. By default the inis are xEdit's: `<Documents>\My Games\<game>\<game>.ini` and `<game>Custom.ini` (`Fallout.ini` and `FalloutCustom.ini` for Fallout 3 and New Vegas). The inis also give the language when `--language` is not passed (`sLanguage` of `[General]`, the custom ini's winning). Pass `--my-games` to a folder without inis to keep the user's inis out of a run.
- `xedit dump` is xDump's loader instead: archives only when a plugin of the load is localized and a loose table is missing, and then only `<plugin>.ba2`, `- Interface`, `- Localization` and `- Wwise*`; `--bsa` always loads those, `--all-bsa` every `<plugin>*` archive (the Fallout 4 DLC strings), `--no-bsa` none.
- `--threads N` sets the threads that read the groups of a plugin while it loads and that build and write the records of `dump`; the default is `RAYON_NUM_THREADS`, else one per CPU, and `--threads 1` runs everything on one thread. The output (dump, saved bytes, every command result) is the same for every count. `saves dump` writes on one thread, and the session commands run on one thread.
- `--fill-pnam` gives a topic response without `PNAM` the `PNAM` of the response it follows when it is built (xEdit's `-FillPNAM`, in the games that sort responses: Oblivion to Skyrim). `--allow-make-partial` lets a copy make a partial form of a cell or worldspace (xEdit's `-AllowMakePartial`). `clean --quick` loads with the settings of the quick clean mode, see "Cleaning".
- `--json` prints one envelope per invocation (see "Output").

Loading is per process: every invocation loads the plugins again. `Skyrim.esm` takes a few seconds and `Starfield.esm` or `SeventySix.esm` much longer. For more than one or two commands, use `batch` (one load, a fixed list of commands) or `serve`/`mcp` (one load, a conversation), and prefer `records list --signature` and `records find` over reading records one by one.
