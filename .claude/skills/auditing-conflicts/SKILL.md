---
name: auditing-conflicts
description: Use when a load order's record conflicts have to be audited with the native xedit CLI of this repository - which override of a FormID wins and what xEdit's conflict statuses mean, narrowing the list to the records a plugin wins or loses, comparing a record's overrides side by side, the mod groups that hide records from the comparison, and the reference index that shows what a conflicting record is used by. The native replacement for the xEdit-llm daemon's comparison routes (comparisons.records, records.conflict_status).
---

# Auditing conflicts

`xedit conflicts` and `xedit compare` are the navigation tree and the view tab of xEdit: for every FormID they classify the records of the loaded files with xEdit's `ConflictAll` and `ConflictThis` (`ConflictLevelForMainRecord`), on all CPUs, with the same result for every `--threads`. Nothing here mutates: no `--edit`, no dry run, no save. Use `using-xedit-cli` for the loading and mutation rules, `checking-parity` for the harness.

## Load the whole load order

The classification compares a record with the records of its FormID in **every loaded file**, so an audit loads every plugin that participates, in load order:

```
xedit --game sse --load "<Data>\Skyrim.esm" --load "<Data>\Update.esm" --load "<Data>\Dragonborn.esm" conflicts
```

Masters load automatically with each `--load`, and `--load` may repeat. `--file` and `--signature` only limit the listing, never the comparison, so a record's status is the same however the list is filtered. Load order is the audit: the last override is the winner the game uses, and the masters/overrides before it are what it overrides.

## The statuses

`conflict_all` is the conflict of all the records of the FormID, from low to high: `caOnlyOne` (a single record), `caNoConflict` (several, all equal), `caConflictBenign`, `caOverride` (overridden without a conflict), `caConflict`, `caConflictCritical` (a FormID-type value or an injected record). `conflict_this` is the part one record plays: `ctMaster`, `ctOverride`, `ctIdenticalToMaster` (ITM), `ctIdenticalToMasterWinsConflict`, `ctConflictWins`, `ctConflictLoses`, `ctConflictBenign`, `ctOnlyOne`, and `ctHiddenByModGroup`, `ctIgnored`, `ctNotDefined`.

The statuses to triage first:

| Status | What it means for the audit |
| --- | --- |
| `ctConflictLoses` | A plugin's change is dead: a later record overwrites what it set. The usual reason to build a patch. |
| `ctIdenticalToMasterWinsConflict` | The record is an ITM but by winning it still hides somebody's change. |
| `ctConflictWins` | A plugin wins against an earlier change; fine, but the change it hides may be wanted. |
| `ctIdenticalToMaster` | A change with no effect (cleaning's food, see `cleaning-plugins`). |
| `caConflictCritical` | A critical conflict (an injected record, or a FormID-type value changed). Triage these before the rest. |

## Listing and narrowing

```
xedit --game fo4 --load "<Data>\Fallout4.esm" --load "<Data>\DLCRobot.esm" conflicts --file DLCRobot.esm --min-conflict-all caConflict
xedit --game sse --load ... conflicts --conflict-this ctConflictLoses --conflict-this ctConflictWins --signature WEAP --signature NPC_ --limit 50
xedit --game sse --load ... conflicts --conflict-this ctConflictLoses --json > losing.json
```

- `--file` (repeat), `--signature` (repeat), `--min-conflict-all caConflict`, `--conflict-this` (repeat) and `--offset`/`--limit` page the list; `total` is the count before paging.
- By default the list is the records that are not the only record of their FormID. `--include-single` adds those too (on a big load order it spends most of its time reading editor IDs; a filter in the request is cheaper).
- The response's `files` has, per loaded file, `records`, `single`, the counts by `conflict_all` and `conflict_this` and the highest status of its records (`InheritStateFromChildren`), which is what the navigation tree shows for a file. Triage a file by its highest status and its `ctConflictLoses` count.
- `--master-and-leafs` compares only the master and the overrides no other override has as a master ("Only show Master and Leafs"). `--quick-show-conflicts` classifies a FormID with exactly one override as an override without comparing the two records (`-quickshowconflicts`), faster but it never sees an identical override.
- `messages` holds the warnings xEdit logs while it compares (`Comparing a mix of sorted, unsorted, and/or alignable entries ...`), which explain a surprising status.

## Reading one record's overrides

`compare` shows the records of a FormID side by side as the view tab does: one column per record (file and `conflict_this`), then the rows of the element tree, each with its `conflict_all` and one cell per column:

```
xedit --game fo4 --load "<Data>\Fallout4.esm" --load "<Data>\DLCRobot.esm" compare 000BB1F9 --hide-no-conflict
xedit --game sse --load ... compare 0001A332 --view-filter-name Damage --keep-children
```

- `--hide-no-conflict` leaves out the rows without a conflict ("Hide no conflict and empty rows"), which turns a big record into just the disputed rows. `--include-hidden` adds the rows the view hides (ignored members, members no record has).
- Sorted arrays are matched by their sort keys (" (sorted)" in the row name) and unsorted arrays aligned entry by entry (" (aligned)"), so the rows show which entry differs. A cell without an element is `null`.
- The view filter is `--view-filter-name`, `--view-filter-value` and `--view-filter-or`, with `--keep-children`, `--keep-siblings` and `--keep-parents-siblings`, as the filter panel above the view tab.
- A game setting (`GMST`) and a default object (`DFOB`) are compared by editor ID, the first of each file, not by FormID; a `NAVI` record is compared only with the records of its own file. Localized strings compare as they resolve: a plugin whose tables are not found shows `<Error: No strings file for lstring ID ...>`, which differs from a resolved text — give the audit loose string tables (`Data\Strings`) if a text matters.
- The row codes are the same that decide the record status the oracle checks; the element-level view has no oracle of its own (see `checking-parity`).

## Mod groups

A mod group says which records of a module another module's records hide: while one is active, the hidden records are left out of the comparison and get `ctHiddenByModGroup` (the first and the last record of a FormID are never hidden). The groups come from the `<plugin>.modgroups` files next to the plugins and the program's own `<AppName>Edit.modgroups` (global `--modgroups-file`; the saved selection in xEdit's settings file, global `--settings`).

```
xedit --game sse --load ... modgroups list
xedit --game sse --load ... conflicts --saved-modgroups --conflict-this ctHiddenByModGroup
xedit --game sse --load ... conflicts --modgroups "STEP" --min-conflict-all caConflict
```

`--modgroups NAME` (repeat), `--all-modgroups` (as `-autoload`) and `--saved-modgroups` activate valid groups; `conflicts` and `compare` compare without any otherwise. The response lists `mod_groups` (activated) and `mod_group_hides` (each module with the modules whose records it hides). Write groups with `modgroups create|edit|delete|update-crcs` (see `using-xedit-cli`).

## What refers to the record: the reference index

An audit of what a conflict affects needs the "Referenced By" information, which the CLI builds on demand, once per session, on all CPUs, and can keep in xEdit's cache files:

```
xedit --game sse --load ... --cache-path "<scratch>\cache" refs get 0001A332 --limit 20
xedit --game sse --load ... --dont-cache refs build
```

`refs get` reports the referencing records (`referenced_by`, of the record's master, paged), `referenced_by_count`, the `references` of the record itself, and the referenced-by filter (`--filter-name`, `--filter-signature`, `--filter-file`, `--filter-or`). `refs build` builds or loads the index explicitly (`--threads`; the cache lands in `<AppName>Edit Cache` in the data folder unless `--cache-path` moves it, `--dont-cache*` turns it off). `conflicts` and `compare` themselves do not need the index; the commands that do are `refs get`, `formids change|renumber`, the filter's `--by-references-injected-status` and `--unnecessary-persistent` options, `check` through a session, and `records cleanup-injected`.

## The audit flow

1. Load the whole order and get the per-file triage: `conflicts --json`, read `files`.
2. Narrow to the losers and criticals: `conflicts --conflict-this ctConflictLoses --min-conflict-all caConflict`, per file.
3. For each: `compare <FormID> --hide-no-conflict` to see the disputed rows, `records get`/`elements get` for the winning override, and `refs get <FormID>` to see what the record is used by (a losing `REFR` placement matters more than a losing unused `STAT`).
4. Check the records for errors before trusting them: `xedit check --record <FormID> --record-file <file>` (see `using-xedit-cli`).
5. Fix: ITM records go to `cleaning-plugins`, lost changes to a patch (`building-patches`); where the last-wins rule itself is the problem, change the load order or activate a mod group instead.

## Gotchas

- A record's status changes with the load order, not with the listing: rerun the whole audit after moving a plugin, and keep the same `--load` list for every step.
- `ctHiddenByModGroup` records are not compared at all: without their mod group they may be `ctConflictLoses`. Say which groups the audit had active when reporting.
- The records the GUI user hid and the compare-to load (`Compare to...`) do not exist in the port; the raw data compare and `Compare Selected` across FormIDs have no command yet.
- The names of placed records and cells are not the special ones xEdit builds (`wbRefrDisplayName` and its neighbours), so a `--view-filter-name` on such a name can miss.
- `--include-single` on a big load order is the slow path; prefer filters.
