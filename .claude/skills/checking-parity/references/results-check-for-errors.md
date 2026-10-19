# Check for errors



`parity check` runs the GUI's own check mode (`-CheckForErrors <plugin>` through `GuiRun::run_check_for_errors`, hidden) on every vanilla plugin of every game (`crates/xtask/src/parity/check.rs`) and compares its message log with the port's `xedit check`.

The log holds `Start:` and `Done:` around the error lines: the record's line (`Checking for Errors in [<load order>] <name>`), the `    <path> -> <error>` of every error under it and `Above errors were found in: <element>` after the elements of an element with errors; the "Processed Records" and "Errors found" counts and the exit code are compared too.

The oracle is cached as `<cache>/<tag>/<MODE>-oracle-check/<file>.<key>.log.zst` with its exit code and counts.

The port loads with the settings of that mode (`commands::set_check_on_load`: the checks run, `wbBuildRefs` off, `wbAllowInternalEdit` off with `wbEditAllowed` on during the load and the check, as `xeInit.pas` does for `tmCheckForErrors`) and checks the records on worker threads in batches of 512. `equal-log-loss` means the oracle's log lost a run of lines (the 4.1.5q memo drops lines under load) but every line it kept matches the port's, in order, and the counts and exit code are equal. `oracle-unsupported`: Morrowind, which the GUI runs in view mode only.

A game master as the target is special: `-CheckForErrors <game master>` has the GUI check the **hardcoded** file (`[00] <game>.exe`), so its log holds the hardcoded file's records and not the master's; the harness runs the port with `check --last` for it (the last loaded file, which is the target) and the master's own content is covered by `parity check-dump` instead.

`parity check-dump` runs `xDump.exe -check` against `xedit dump --check` over the same corpus and compares the output as the dump is compared (an oracle run that ended in an error compares as a prefix: the port matches it up to the crash and then has to finish without an error, `equal-prefix`). The oracle's output is cached in `<cache>/<tag>/<MODE>-check`.

Results (2026-10-09, with the final code): `parity check` over the 5,852,286 records of the 249 vanilla plugins of the 11 games: 245 `equal`, 1 `equal-log-loss` (`fo76/NW.esm`: the oracle's log lost 2,365 of 87,414 lines in one run; its 171,206 records, 12,395 with errors and every line it kept equal), 3 `oracle-unsupported` (the Morrowind masters), 0 `different`, 0 failed (143,951 records with errors, 260,691 error lines). `parity check-dump` over the corpus: 246 of the 246 files the oracle dumps equal (229 `equal` and 17 as prefixes where xDump crashes on the file: `Fallout4.esm` of Fallout 4 and of Fallout 4 VR, `DLCCoast.esm`, `DLCNukaWorld.esm`, `DLCRobot.esm`, `DLCworkshop03.esm` and 5 CC ESLs, `fo76/SeventySix.esm`, and `Starfield.esm`, `ShatteredSpace.esm`, `SFBGS003.esm`, `SFBGS050.esm` and `SFBGS00D.esm`), 0 `different`; the 3 Morrowind masters have no oracle dump at all (xDump fails on them, issue #9).

The harness caps each process's committed memory at the budget (23.7 GiB of this machine), so on a loaded machine a big file can end as `port-memory-limit`: `sf1/Starfield.esm` did when other sessions ran their own parity jobs, and it is `equal-prefix` when run alone.

The check found, all fixed in the port: the path of a main record is its signature; `TwbValue.Init` runs the `AfterLoad` of the resolved definition; the sorted array flags are cleared only after the sort, so a reader on another thread saw unsorted entries and parallel output depended on timing; `wbQUSTEventToStr` reads `ReferencedBy` under `wbBuildRefs`; a flag of `wbFlagsAsArray` found by name in a path; `TwbValueBase.GetName` without the space before the suffix of an unnamed value; `Contained subrecords:` after `Errors were found in:` (the release is built with `DBGSUBREC`); `SetReferencesInjected` and the cached `GetReferencesInjected`.

The Fallout 76 check-dump found that `wbDefinitionsFO76` has its own, older copy of `wbLGDIFiltersToStr` (the same name and signature as the one in `wbDefinitionsCommon`); the port had answered Fallout 76's `LGDI` filters with the common copy.

Issue #40 settled the class: the transpiler keeps routines of one name and signature in different units apart and resolves a name by the unit it is written in, so the Fallout 76 definitions name `wb_lgdi_filters_to_str_fo76` (callbacks/fo76.rs), Fallout 3's and New Vegas's `wbSOUNAfterLoad` are their own ports, and the `probes/soun-after-load.pas` fixtures of both games (`cargo xtask parity script --script soun-after-load`) check the differing copy the vanilla corpus does not show.
