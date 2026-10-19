# Filter



`parity filter` runs the scenarios of `crates/xtask/oracle/filter/*.json` (`--file <stem>` selects some, `--game` the games).

A scenario names a game, the plugins to load and a list of filter configurations of `TfrmFilterOptions`.

The GUI build of xEdit runs `crates/xtask/oracle/filter.pas` in its script mode: the harness generates one block per configuration that sets every filter variable (as `Apply filter for cleaning.pas` does) and calls `ApplyFilter`, which skips the options dialog (`FilterPreset`).

The filter writes its counts to the message log of the main form (`[file] Filtered n of m records` per partly filtered file and the closing `Done: Applying Filter, [Pass 1] Processed Records: ..., [Pass 2] Processed Records: ..., Remaining unfiltered nodes: ...`), which the harness reads per configuration; a scenario that filters by the reachable status clicks `mniNavBuildReachable` first, which the port's `refs.build-reachable` mirrors.

The port runs the same configurations as one `xedit batch` from a folder of hard links to the plugins, as the GUI's private data folder (no archives and no strings files on either side), and compares the two passes, the nodes left and the records each file lost.

The oracle's log is cached in `<cache>/<tag>/<MODE>-oracle-filter/<scenario>.<key>/`; the key hashes the plugins, the generated script and the scenario.

The report is `target/parity/filter.json`.

Results 2026-10-09 (the reachable scenario re-run 2026-10-10): `--file sse-filters` (Skyrim.esm and Update.esm, 37 configurations, 26,849,591 nodes walked in the first passes) 37 of 37 equal; `--file fo4-filters` (Fallout4.esm and DLCRobot.esm, 8 configurations) 8 of 8 equal; `--file sse-reachable` (the five Skyrim SE masters, "Build Reachable Info" and 4 configurations) 4 of 4 equal.

Re-run port-side on 2026-10-16 (`origin/main` `7d792b9` and parity batch #27's tip, the oracle logs from the cache): `fo4-filters` 8 of 8 and `sse-reachable` 4 of 4 equal, `sse-filters` was **36 of 37** -- the `unnecessary-persistent` configuration filtered 3 fewer records of Skyrim.esm than the oracle (`869186/869687` against `869189/869687`; `[Pass 2] Processed Records` 40679 against 40677, `Remaining unfiltered nodes` 697 against 692).

Fixed 2026-10-16 (`fix-sse-filters`, issue #64): **37 of 37** again.

The cause was the reference index, not `IsUnnecessaryPersistent`: the earlier configurations that read a base record's name quick-initialized `ACTI` 0005307A, whose `VMAD` holds the two kept references, and parity #23's reproduction of upstream's quick init emptied the record at its next init (the collapse of an invalidated storage from no elements), so the references were rebuilt from nothing; the GUI masks it by building the references at load.

The port no longer reproduces that data loss (an `UPSTREAM-BUG`, see `docs/PLAN.md`).

A standalone `filter apply` of one configuration cannot show this class of difference -- the configurations of a scenario run in one session, as the GUI's do, so a filter can leave state the next one reads; bisect by running prefixes of the scenario's batch file (`<scratch>/<GAME>-filter/<scenario>.<key>.batch.json`) through `xedit batch` with `list_records` on the last command.

The scripted flag's scenario `sse-scripted` (Skyrim.esm and Update.esm, 2 configurations, 2026-10-15, parity batch #27) is 2 of 2 equal: `FilterScripted` with no `Filter` function in the generated script rejects every record -- only the file and group nodes stay -- and the filter clears the flag when it ends, so the cleaning configuration after it runs unscripted.

The check found and the port follows: the walk of `AssignPersWrldChild` continues at the first moved node; `PotentiallyUnfilteredRefs` (not the top level groups) decides whether a base record filter clears the tree; `IsUnnecessaryPersistent` reads the reference information, which the filter builds first (`BuildAllRef`), and its `LCTN` loop never takes an entry off the count (a main record's `LinksTo` is nil).
