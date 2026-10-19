## Conflicts

`conflicts` and `compare` classify the records of a FormID as the GUI colours them (`ConflictLevelForMainRecord`; the view tab's `InitConflictStatus`). Load every plugin of the load order that matters: a record is compared with the records of its FormID in all loaded files, and the masters of the loaded plugins load with them.

- `conflict_all` is the conflict of all the records of the FormID, from low to high: `caOnlyOne` (a single record), `caNoConflict` (several, all equal), `caConflictBenign`, `caOverride` (overridden without a conflict: one override, or every override agrees with the last), `caConflict`, `caConflictCritical` (a FormID-type value or an injected record). `conflict_this` is the part one record plays: `ctMaster`, `ctIdenticalToMaster` (an identical override: ITM), `ctOverride`, `ctIdenticalToMasterWinsConflict`, `ctConflictWins`, `ctConflictLoses`, `ctConflictBenign`, `ctOnlyOne`, and `ctHiddenByModGroup`, `ctIgnored`, `ctNotDefined`.
- `conflicts` lists every record that is not the only record of its FormID; `--include-single` lists those too. `--file` limits the list, not the comparison. `--min-conflict-all caConflict --conflict-this ctConflictLoses` finds the records of a plugin that lose a conflict; `--conflict-this ctIdenticalToMaster` finds identical overrides.

The `files` part of the result has, for every loaded file, `records`, `single`, the counts by status and the highest status of its records, which is what the navigation tree shows for a file. `messages` holds the warnings xEdit writes to its message log while it compares (`Comparing a mix of sorted, unsorted, and/or alignable entries ...`).
- Game settings (`GMST`) and default objects (`DFOB`) are compared by editor ID, the first of each file, not by FormID, as xEdit does; a `NAVI` record is compared only with the records of its own file, so its overrides show `caOnlyOne`/`ctHiddenByModGroup`.
- `compare` gives one column per record (`columns`, with the file and the record's `conflict_this`) and the rows of the view as a tree (`rows`, each with `name`, `conflict_all`, one cell per column with `value` and `conflict_this`, and `children`).

A cell without an element is `null`.

Rows the view hides (ignored members such as the record header's data size, members no record has) are left out unless `--include-hidden`; `--hide-no-conflict` leaves out the rows without a conflict, as the view's "Hide no conflict and empty rows".

Sorted arrays are matched by their sort keys (" (sorted)" in the row name) and unsorted arrays aligned entry by entry (" (aligned)").
- The comparison follows the GUI's settings: the session builds the definitions with simple records (`wbSimpleRecords`, so the landscape data and similar subrecords are byte arrays), the unused fields hidden, the contained-in element of a placed record (its cell or worldspace) and the flags of a flags value as elements of their own.

Localized strings compare as they resolve: a plugin whose strings files are not found shows `<Error: No strings file for lstring ID ...>`, which differs from a resolved text.
- `--master-and-leafs` compares only the master and the overrides no other override has as a master ("Only show Master and Leafs"); `--quick-show-conflicts` classifies a FormID with one override as an override without comparing them, as `-quickshowconflicts` does.
- The records of different FormIDs are compared on all CPUs (`--threads`); the result is the same for every thread count. A full Fallout 4 load order of vanilla files takes seconds.
- **The view filter** (the filter panel above the view tab) is `--view-filter-name` (the row name contains it, without case) and `--view-filter-value` (one of the row's cells contains it, the first column excluded as in xEdit), combined with AND by default or with `--view-filter-or` as xEdit's And/Or switch.

A matching row keeps its parents always, and with `--keep-children`, `--keep-siblings` or `--keep-parents-siblings` also the rows below it, beside it, or beside its parent.

The result lists `view_filter` with `rows`, `filtered` and `shown`; the hidden rows are left out (with `--include-hidden` they are listed with `filtered: true`).

```
xedit --game fo4 --load "<Data>\Fallout4.esm" --load "<Data>\DLCRobot.esm" conflicts --file DLCRobot.esm --min-conflict-all caConflict
xedit --game fo4 --load "<Data>\DLCRobot.esm" compare 000BB1F9 --hide-no-conflict
```
