## Tool modes

xEdit is started in a tool mode: the executable name (`SSEEditQuickAutoClean.exe`, `SSEEditLODGen.exe`) or a switch selects it, and the mode decides how the plugins load and what runs over them once they are loaded. `xedit tool modes` lists all seventeen with the switch of each; `xedit tool run <mode>` runs one.

A mode that changes things loads with its own settings, acts over the loaded files and saves everything it changed, each plugin to its own path (with a backup, unless `--no-backup`), as the GUI's `SaveChanged` at the end of an auto mode does.

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
| `export` | xDump's `-export RAW`: the profile of the game's record definitions. `--format UESPWIKI` writes the wiki tables instead; the profile file goes to `--output` (default `<AppName>ExportPlugins.txt` next to the program, as xDump writes it) and the structure of the definitions comes back as the response's `text`. `--source saves` exports the *save* definitions instead (`xDump.exe -saves -export RAW`): `wbFileHeader` and the chapters `wbFileChapters` (`ProfileChapters`), the profile named `<AppName>ExportSaves.txt`. The save definitions are built at load, so a session exports them only when it was loaded with `--source saves` (the legacy `-saves`). |
| `edit`, `view`, `translate` | Loading modes: `edit` is the default, `view` loads read-only and `translate` is `--translate`. They report what they are. |

- The modes reachable as their own commands are not run twice: `dump` points at `xedit dump` and `lodgen` at `xedit lodgen` (`lodgen.generate`; the mode itself generates nothing), while the `script` mode runs the `-script:` script over the loaded plugins as `script run` does, saving what it changed. `quickclean`/`quickautoclean` are the edit mode's quick clean switches: `-quickclean` cleans in memory only and `-quickautoclean` saves, as `xedit clean --quick` (the quick auto clean mode) does.
- `--dry-run` reports what the mode would change without changing anything; for `export` that means it writes nothing and still returns the text.
- The modes take (and the modes of the GUI take) the switches of `xeInit.pas` too: `-filteronam`, `-FixPersistence`, `-alwayssaveonam`, `-IKnowWhatImDoing` and the switches it unlocks (`-AllowMakePartial`, `-AllowMasterFilesEdit`, `-AllowEditHEDRVersion`, `-StripEmptyMasters`, `-StripMasters:<name,...>`, `-AllowESPMaster`, `-IKnowIllBreakMyGameWithThis`), `-FillPNAM`/`-NoFillPNAM`, `-sortinfo`/`-nosortinfo`, `-nobuildrefs`, `-fixup`/`-nofixup`, `-showfixup`/`-hidefixup`, `-SimpleFormIDs`, the light, medium and update flags of the load (`-PseudoESL`/`-PseudoLight`/`-PseudoSmall`, `-IgnoreESL`/`-IgnoreLight`/`-IgnoreSmall`, `-PseudoMedium`, `-IgnoreMedium`, `-PseudoUpdate`, `-IgnoreUpdate`), `-dontremoveoffsetdata`, `-reportinjected`/`-noreportinjected`, `-TrackAllEditorID`, `-EnforceAllMasters`, `-moreunknown`, `-speed`/`-memory`, `-cp-general:`/`-cp:`/`-cp-trans:`, `-l:`, `-DontCache`/`-DontCacheLoad`/`-DontCacheSave`, `-T:`, `-G:`, `-B:`, `-C:` and, for Starfield, `-ItJustWorksTM -ThisIsFine -GiveMeTheRedPill`.

Each has a flag of its own on the CLI (`--pseudo-light`, `--ignore-light`, `--pseudo-medium`, `--ignore-medium`, `--pseudo-update`, `--ignore-update`, `--dont-remove-offset-data`, `--report-injected`/`--no-report-injected`, `--track-all-editor-id`, `--enforce-all-masters`, `--more-unknown`, `--speed-over-memory`/`--memory-over-speed`, `--code-page`, `--code-page-trans`, `--strip-masters`, `--allow-direct-saves`, `--temp-path`, `--save-path`, `--backup-path`, `--mo-profile`, `--starfield-red-pill`), so a `serve` or MCP session loads as the same command line would.

The switches the port reads and does not act on are named on stderr with the reason (a no-op of the release, a switch that only changes the GUI, or an unported feature: `-report`, `-MoreInfoForIndex`, `-fixuppgrd`, `-exceptiontest`, `-devmode`, `-moreunknown`, `-speed`/`-memory`, `-moprofile`, `-resetsettings`, `-AllowDirectSaves`); `-moprofile` leaves the Mod Organizer hook (modorganizer\hook.dll) unloaded.

**The command line of xEdit itself.** Mod managers start xEdit with its own switches (`SSEEdit.exe -quickautoclean -autoexit -autoload "<plugin>"`, `-IKnowWhatImDoing`, `-D:<data>`, `-P:<plugins.txt>`), and those are not the CLI's syntax (`--game`, `--load`).

The port reads them as xEdit does: a command line that holds one of them is taken as a tool mode invocation — the tool mode, the game (`-SSE`, `-FO4`, ... or the executable name), the plugin list (`-P:` or the plugin named as a parameter), the data folder (`-D:`), the settings above (`-quickclean` and `-quickautoclean` load with the quick clean settings of `xeInit.pas`, see "Cleaning"), and the switches the port does not act on, which it names on stderr.

The mode runs, its message log goes to stdout and the exit code is what xEdit exits with (the count of the check modes, at most 127). `-R:<file>` writes the log to a file.

```
xedit -SSE -D:"<Data>" -P:"<plugins.txt>" -quickautoclean -autoexit
xedit -FO4 -D:"<Data>" -checkforitm "<plugin>.esp"; echo $?   # the ITM count
xedit -TES4 -D:"<Data>" -P:"<plugins.txt>" -quickedit:"<plugin>.esp" -autoexit
```
