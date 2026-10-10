---
name: cleaning-plugins
description: Use when a Bethesda plugin has to be cleaned with the native xedit CLI of this repository - removing the records identical to their master (ITM), undeleting and disabling the deleted references (UDR), cleaning up records that refer to injected records, or running xEdit's quick auto clean mode (-quickautoclean) as a mod manager's "QuickAutoClean" does. The native replacement for the xEdit-llm daemon's cleaning routes (cleaning.quick_auto_clean, cleaning.remove_itm, cleaning.undelete_and_disable_refs, cleaning.cleanup_injected_references).
---

# Cleaning plugins

`xedit clean` is xEdit's plugin cleaning over **one** loaded plugin per call: "Remove Identical to Master records" (ITM), "Undelete and Disable References" (UDR), and the quick auto clean mode (`-quickautoclean`) that runs both and saves. The steps are the GUI's own handlers (`mniNavRemoveIdenticalToMasterClick`, `mniNavUndeleteAndDisableReferencesClick`, the passes of `tmrGeneratorTimer`), oracle-checked against the release.

## Safety

Cleaning mutates: every mode needs the global `--edit`, or `--dry-run`, which counts and changes nothing. Read the mutation rules in `using-xedit-cli` first. Never clean a plugin in a game's `Data` folder while testing; write beside it with `--output` and load the result in a fresh process to verify. `--quick` writes the file itself, over the loaded path unless `--output` says otherwise, moving an existing file to `<AppName>Edit Backups` first unless `--no-backup`.

## The steps alone (in memory)

```
xedit --json --game fo4 --load "<Data>\DLCRobot.esm" clean --itm --udr --dry-run
xedit --json --edit --game fo4 --load "<Data>\DLCRobot.esm" batch - <<'JSON'
[
  {"command": "files.clean", "params": {"itm": true, "udr": true}},
  {"command": "files.save", "params": {"output": "<scratch>\\DLCRobot.esm", "backup": false}}
]
JSON
```

- `--itm` removes the records whose conflict status is `ctIdenticalToMaster` (a navigation mesh that is only a benign conflict too), walking the plugin from its last record, so a cell or worldspace goes when all its children went and every group that ends up empty goes with them. The `itm` count is xEdit's: it counts the removed groups too. A record injected into a master's FormID space is never removed.
- `--udr` undeletes the deleted placed references (`REFR`, `ACHR`, `ACRE`, `PGRE`, `PMIS` and the Skyrim projectiles) and disables them: the reference gets the data of its master again (moving to the master's cell when it was moved), is set initially disabled, gets the player as an opposite enable parent (`XESP`), loses its enable parent and teleport; a reference that is not persistent moves to z -30000 (Fallout 3 and New Vegas keep the position, as xEdit's defaults). Deleted navigation meshes can not be undeleted (`deleted_navmeshes`, xEdit's "nav" count), nor can injected references, references without a base record or New Vegas trees with LOD (`not_undeleted`).
- Without `--quick`, the steps change memory only: save in the same `batch` or `serve` session. With `--dry-run`, every mode counts what one pass would clean.

## The quick auto clean mode

```
xedit --json --edit --game sse --load "<Data>\Dawnguard.esm" clean --quick --output "<scratch>\Dawnguard.esm"
xedit --json --edit --game sse --load "<Data>\Dawnguard.esm" clean --quick --dry-run
```

- `--quick` is a pass of UDR then ITM, the plugin saved, and again while a pass changed it, at most three passes (the third is not saved, as upstream). Mod managers run the same mode as `SSEEditQuickAutoClean.exe` and `xedit -SSE -D:"<Data>" -quickautoclean -autoexit` reads that command line.
- The mode loads the plugins with its own settings (`xeInit.pas`): the full record definitions (not the simple ones), the PNAM of the topic responses filled where the game sorts them (`-FillPNAM`), no `INOM`/`INOA` lists on the topics. `xedit clean --quick` and the legacy `-quickclean`/`-quickautoclean` command line apply them; a session started any other way (a plain `serve`) adds a `<Warning: the plugins were not loaded as the quick clean mode loads them ...>` to `messages` and its comparisons can differ. The global `--fill-pnam` turns the PNAM fill on for any command.
- Use `--quick` for one answer, not for inspection: the per-pass counts and the cleaned records are in the response, but a `--dry-run` first (or `--itm --udr --dry-run`) tells you what the clean will find before any file changes.

## Records that refer to injected records

`xedit records cleanup-injected [<FormID>...] --file <plugin> [--dry-run]` is "Cleanup injected records": the records of the plugin that refer to injected records of a plugin that is not their master are copied into that plugin as overrides (its missing masters are added, without xEdit's question) and lose those references in their own plugin. With no FormID it takes every record of the plugin that refers to such injected records (the "References injected" filter). It builds the reference index first (see `auditing-conflicts`), so on a big load order the first run takes a while. `changed_files` names the plugins to save.

## Reading the result

The response has one entry per pass in `passes` (the filter's node counts `pass1`, `pass2`, `unfiltered`, `udr` and `itm` with `processed`/`count`/`records`/`skipped`, and `saved` for a quick save), the totals `itm`, `udr` and `deleted_navmeshes` (what xEdit reports as the plugin's dirty information to LOOT), `unsaved`, and `messages`, the lines xEdit writes to its log (`Removing: ...`, `Undeleting: ...`, `Skipping: ...`, with the GUI's record names). A dry run fills the same counts with `dry_run: true`.

## The oracle and its known differences

`cargo xtask parity clean` runs the GUI's own quick auto clean mode headlessly on every corpus plugin that has masters and compares the saved bytes, per pass the filter's counts, the UDR and ITM counts and the cleaned record lines of the log. Results (2026-10-08, in `docs/PLAN.md` under phase 4): 231 of 238 equal; 4 `different`, 1 `oracle-failed`, 2 `oracle-unsupported`. In all 235 compared plugins the counts of every pass and the records the log names are the oracle's; the 4 `different` differ only in the bytes of records the clean did not touch:

- `Anchorage.esm`, `BrokenSteel.esm`, `ThePitt.esm` (Fallout 3): 2 to 3 responses of the `GREETING` topic whose response text the port trims (`wbDialogueTextAfterLoad`), where the oracle writes them as loaded.
- `Fallout4_VR.esm`: the oracle rebuilds its `DOBJ`, dropping the unused entries; what builds it in the oracle is not found.
- `NW.esm` (Fallout 76) is the `oracle-failed` one: the GUI cleans and saves it, but its log file holds only the closing line, so the counts can not be read. The Morrowind masters are `oracle-unsupported` (the 4.1.5q GUI does not edit Morrowind).

## After cleaning

1. Read the response: the counts and the named records are the evidence. Zero ITM and zero UDR means nothing was cleaned; check the plugin really was the one loaded.
2. Verify on the saved file: `xedit check` (no new errors), `xedit conflicts --include-single` (the ITM records are gone), and `records get` on a record the log named.
3. Never clean a game master or another mod's original: clean a copy, or the plugin you are shipping.

## Gotchas

- One plugin per call: `files.clean` cleans the plugin of `--file` (or the only one loaded), with its masters loaded beside it.
- The quick mode needs its own load settings to give xEdit's bytes; a session loaded otherwise gets the warning and can differ (the comparison of ITM records depends on the PNAM fill and the full definitions).
- The translate mode refuses `files.clean` and `records.cleanup_injected` (`translate_mode`), as the GUI hides those menu items then.
- Not ported: xEdit's `-AllowMakePartial` (partial forms of cells and worldspaces with children), the UDR options of xEdit's Options dialog (the per-game defaults are used), and the LOOT dirty-information report (the response's counts are the data).
- `deleted_navmeshes` are counted and left deleted: they have to be regenerated in the Creation Kit, not undeleted here.
