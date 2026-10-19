## Known gaps

Behaviour a user can meet, as of the parity close-out (issue #41). Each is an upstream behaviour not ported yet, with the issue that owns it, or a limit of the oracle; say so rather than work around it silently.

- **Strings and archives.** Every session loads the archives as the GUI does, so the Fallout 4 DLC strings resolve from `<plugin> - Main.ba2`, and the oracle agrees on the archives found, their order, what each resource resolves to and the saved tables (`parity resources`, `parity oracle-edit` with `archives`).

The settings file is read for `LoadBSAs` only (not `DecodeTextureHashes2`, which the definitions read before it); the inis are read as `GetPrivateProfileString` reads them for the archive lists (2047 characters at most) and as `TMemIniFile` for the language.

Only the ANSI code page 1252 was checked against the oracle for archive names (see `handling-archives`).
- **Startup discovery.** A `serve`/`mcp`/`xedit` session still loads the plugins `--load` names; the *legacy* command line (`-SSE -quickautoclean`) finds what `DoInitPath` finds when `-D:` and `-P:` are missing (the folder of the program, the Steam library, then the registry key for the data folder; the game's own `Plugins.txt` in the local application data folder; the save folder from `SLocalSavePath` of the ini) and prints the `Using ...` lines of the GUI's startup log, which `cargo xtask parity discovery` compares with the release.

A game folder the registry has no key for fails with upstream's `Fatal: Could not determine <game> installation path, no "<value>" registry key`.

The module list of a legacy load is `wbLoadModules`': a module's masters are read from its file header and a module with a missing master is not active; the plugin list gives each module it names its `Plugins.txt` index (the order the game reads; the older games' lists name the active plugins, the later ones mark them with `*`) and its active state; the game master, `Update.esm` of the Skyrim games, the official DLC and the Creation Club modules of the game's `.ccc` list are active even when the list does not name them; the order is `_ModulesLoadOrderCompare` (blueprint, official, Creation Club, ESM, `Plugins.txt` index, write time, name), with `loadorder.txt` next to the plugin list reordering modules it does not index (`_ModulesLoadOrderCompareCombined`) for Oblivion Remastered, Skyrim and Enderal; and the run loads the active modules with the masters they need, each after its masters (`SimulateLoad`). `cargo xtask parity discovery` compares that end to end: the original case plus synthetic data folders for the implicit actives (SSE and FO4), missing masters (FO4) and a `loadorder.txt` (Skyrim LE), comparing the `Using ...` lines and the `[<file>] Loading file` order of both runs.

The CLI's own sessions read no plugin list unless `--plugins-list` names one.
- **Aligned arrays.** An assign at a position of an array that records compare aligned (the view's drag and drop, `AlignedCreate`) needs that alignment: run `records compare` on the record first, which gives the entries of both versions their sort orders, as showing the record in the GUI's view does. Without it the entries carry no sort orders to align at.
- **Sorted arrays.** A record that is rebuilt on save sorts its sorted arrays as upstream does (the subrecord arrays, and the sorted array values, `srsSorted` and `arrSorted`, at their first read by index or write after a change), and a master update (`masters add|sort|clean`) keeps them in their order as xEdit does; a modified topic group of Fallout 4 and later sorts its responses by FormID as xEdit does; in the games that sort them by their `PNAM` (Oblivion to Skyrim) they are not sorted again after their FormIDs change, so such a save can differ from xEdit's in their order.
- **References.** `refs build-reachable` is xEdit's "Build Reachable Info"; what it reaches is right for the groups a game always loads and for the references, scripting, quest-objective, furniture and NPC cases of `TwbMainRecord.Reached`, but a record whose references come from the reference cache takes only its editor ID and full name from it (its base record, grid cell and GUI names are read from the record when needed), and a record's own elements are not walked while its references are built by an edit (`UpdateRefs`).
- **Mod groups.** The modules of the game's Creation Club list (`Skyrim.ccc`, `Fallout4.ccc`, `Starfield.ccc` next to the data folder) come after the official ones in the list's order (parity #41, `parity modgroups --file sse-ccc-order`); a loaded module's `Plugins.txt` index comes from the plugin list `--plugins-list` names (the CLI reads none otherwise, so the index stays unset as upstream's when the file is not there).

The presets of the *module* selection dialog (`frmModuleSelect_ActiveModules_Presets`) are not ported (issue #78); the mod group dialog's are.
- **Conflicts.** The hidden-record state, the compare-to load (`--compare-to`), the raw data compare (`records.compare --raw-data`) and compare of selected records of different FormIDs (`records.compare_selected`) are in; `conflicts.for_elements` is the script function `ConflictAllForElements`.

The release never sets `wbCompareRawData` (no switch assigns it), so `--raw-data` is an option over the dormant upstream code paths with no oracle run.

The compare-to load cannot be created by a script either (no host function loads a plugin), so it is pinned to the release's code (`wb_file_compare`, `IsCompareToSame`) and the port's own runs.
- **Cleaning.** In the oracle check (235 of 238 plugins equal) `NW.esm`'s saved bytes are equal while the release's log loses its counts (see `docs/PLAN.md`, parity #35).

Under the PNAM fill of the quick clean modes a filter can leave responses of a topic uncompared, as the release does (the sort a response without `PNAM` forces reorders the nodes the walk has not reached).

The UDR scale step deliberately differs from the release, which writes the scale into the reference before when a reference has one (`UPSTREAM-BUG`).
- **Check for errors.** The script function `Check` of one element and the errors below it are `elements.check`.
- **Filters.** `filter apply` runs every option of the dialog, the scripted filter (`--scripted`, the script's `Filter` function, parity #27) included; records the user hid leave the comparison and the only-one mode (`records.set_hidden`), and the file name filter of the navigation tree is `files.list --name-filter/--regex`.

The first pass asks for the conflict statuses where the GUI does, node by node, in the games that sort a topic's responses, so a topic its walk sorts binds its responses as the release's tree does, and a script's `Process` walk after `ApplyFilter` follows the filtered tree's nodes (parity #41).

The `FilterOnlyOne`, `FilterConflictOnly` and `FilterNoGameMaster` flags that `filter apply`'s `only_one`, `conflict_only` and `no_game_master` mirror have no oracle route: the switches that set the latter two run in the edit mode only (`xeInit.pas:1216`) and no script host variable exposes them, so they are pinned to the release's code (`mniNavFilterForOnlyOneClick`, `mniNavFilterConflictsClick`, `ReInitTree`).

The selected-file dialogs of the "* Selected" menu items are the `files` parameter of `filter apply`, and the dialog's own buttons are presentation: the request carries the sets and the names.
- **Tool modes.** `-setesm` changes nothing: xEdit 4.1.5q's auto mode dispatch has no branch for it, and the port follows the release (`files flags --esm true` sets the flag).

The `-lodgen` mode is accepted and points at `xedit lodgen` (`lodgen.generate`).

Of the legacy switches, the ones the port reads and does not act on are named on stderr with their reason (see [tool modes](tool-modes.md) and `coverage/ledger.toml`): `-moreunknown` (`wbMoreInfoForUnknown`, which the release uses to name an element with unknown data; issue #79 with xDump's `-more`), `-devmode` (the release's fixed development CRC32), `-speed`/`-memory` (`wbSpeedOverMemory`, which trades memory for speed in the release's caches), `-moprofile` (the Mod Organizer hook), `-resetsettings` and `-AllowDirectSaves` (GUI only) and the four switches the release itself does nothing with (`-report`, `-MoreInfoForIndex` under `DebugHook`, `-fixuppgrd`, `-exceptiontest`).
- **xDump's switches.** `xedit dump` is xDump's plain dump with `-check`, `-bsa`, `-allbsa`, `-nobsa` and `-l:`, and the dump-side filters, element-output and report switches are in: `-dg`, `-dr`, `-xr`, `-xsr`, `-xg`, `-xcg`, `-xbloat`, `-top`, `-es`, `-dh`, `-more` (and `-moreunknown`), `-sr`, `-sro`, `-sroe`, `-summary`.

Upstream's gating is kept (`-xcg` alone keeps the group line and drops its contents; `-xbloat` is the else of `-xg`; `-es` sizes a group past its 24-byte header). `cargo xtask parity dump --switch <name[:value]>` runs both sides with one switch. `-dcr`/`-report` walk `ReportDefs` and are equal, and the save-dump switches `-df`/`-xf`/`-dc`/`-xc`/`-xcbloat` are options of `xedit saves dump` with equal oracle cases.

Only `-bts`/`-btd` stay unwitnessed: they change Skyrim LE saves alone (the `Unused`/`Remaining` arrays are commented out in the other games' definitions) and a full TES5 save dump exceeds the harness timeouts. `-do` is excluded: it prints raw pointers against `wbBaseOffset`, which only a scanned plugin has.
- **GUI operations without a command.** Some menu items of the release have no typed command yet: Mark Modified, Element Clear, Next/Previous Member of a union, copying a value to the selected records, the cell child persistence and VWD items, Change Referencing Records, Check for Circular Leveled Lists, the wrapper and spawn rate copies, Create Delta Patch, the Oblivion helpers, the Log Analyzer, the module selection presets and BSArchPro's asset list editor (issue #78; `coverage/ledger.toml` lists them).
- **Morrowind.** Plugins load and dump, but the save stops at `must have a FormID`: the identity FormID of a TES3 record (`wbFormIDFromIdentity`) is not ported (issue #9). The 4.1.5q oracle saves no Morrowind plugin either (its GUI runs Morrowind in view mode), so there is nothing to compare with.
- **New files.** `files new` makes `.esp` and `.esl` plugins from the light and medium flags, or from a module template of xEdit's "Create New File" dialog; the dialog's own list is presentation, the templates it offers are the game's modules (the request takes their names).
- **Sessions.** One session per command line, and one session per daemon: `serve` and `mcp` take the game, the plugins and the edit gate at startup, and `session.load` loads another plugin list (or the same one with a tool mode's load settings) into the running session. A named pipe serves one client at a time and one daemon owns a pipe name (see "Daemon and MCP server").
- **Starfield.** The complex FileIDs (light and medium masters with slots of their own) are followed by the master functions and the FormID lookups, but FormID changes and renumbering across masters are unchecked there. The oracle refuses to save the official Starfield modules whose header the save would edit, and so does the port.
- **Oblivion saves** do not read (an upstream limit, see "Saves").
- **Mesh optimizing.** `assets optimize` runs `SpellOptimize` on one file (`stripify` is `SpellStripify`'s pass, `triangulate` `SpellTriangulate`'s) and Sniff's `Optimize mesh` runs the same spells on a folder; `SpellFaceNormals`, `SpellUpdateTangents` and `SpellAddUpdateTangents` have no command of their own (Sniff's "Update tangents and binormals" runs them).
- **Sniff.** `Update MOPP code` is the released 64-bit build's refusal (its message is `sniff.run`'s error), and `ProcCollapseLinksArrays` has no operation in the 4.1.5q form; see "Sniff".
- **Scripts.** `script list`, `script check`, `script run` and `script resume` are in, and the script API is bound (the four form runners of `JvInterpreterFm.pas`, which build forms from DFM files, refuse headless with `[EVclUnavailable]` and are issue #81; nothing in the corpus calls them).

A form, dialog or control call runs against the headless VCL object model (`crates/xedit-session/src/script_vcl/interactive.rs`, issue #26): a dialog, `ShowException`'s box included, is answered from the recorded `answers` of `script run` (`pending_interaction` names the question when none remains; `script resume` continues), the `TCanvas` and paint members draw nothing, and event handlers a script assigns are stored but not fired.

The corpus's form scripts that have no run case yet are issue #81.

The walk of `Process` steps over the live tree at a constant cost per step (issue #82), and after `ApplyFilter` over the filtered tree's nodes.

The `LoadFromResource` of the data format methods and the `Resource*` functions read through the container handler, which every load fills as the GUI's loader does (`parity resources` compares what a script finds with the oracle's script mode); `xedit script run` loads in the edit mode, where `--skip-bsa` skips the archives, while the legacy `-script:` is the script mode, which loads them whatever `-skipbsa` says, as upstream.

The `writing-xedit-scripts` skill lists the divergences (the UTF-8 string model where the oracle counts UTF-16 units, the slot kinds -- a `Cardinal` as an `Int64`, a `Double` against upstream's `Currency` 0, the per-kind custom variant codes -- the native faults and hangs upstream dies on, the runaway-recursion guard, the elapsed origin and the save suffix's clock), with `docs/PLAN.md` as the record.
- **Texture archives.** Packing and extracting work for `Fallout 4 DDS` and `Starfield DDS` archives. `archive pack --target xbox` is the GUI's "XBox 360 Archive" target (`BSArchPro`): an `xbox` archive refuses a texture that is not in the XBox format and a `pc` one refuses an XBox texture, and with `xtexconv.exe` next to the program (`.local`/the program folder; the release does not ship it, and neither does the port) a PC texture is converted through it first (`DDS2XBOX`, `xtexconv.exe -xbox <temp file>`, read back).

The conversion itself has no oracle: the release ships no `xtexconv.exe`, and the GUI's autopacking mode does not start its work when it is driven without a user (`cargo xtask parity bsarchpro` reports it and says so), so the GUI route is owed a run at a screen.

A texture whose data ends before its last mipmap is cut where the data ends, where `BSArch.exe` reads past its buffer in the route without a split.
- **Vector instructions.** The pixel conversion of the texture code (24 to 32 bit) runs on SSSE3 or AVX2 and the half-float array conversions (`xedit_core::half_float`) on F16C where the machine has them, each with a scalar version that gives the same bytes; `XEDIT_SIMD=off` takes the scalar route everywhere.
