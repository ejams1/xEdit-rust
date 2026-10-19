# Conflict status



`parity conflicts` has the GUI build load the vanilla plugins of each game together (all of them, `--file` names some instead, with their masters) and run `crates/xtask/oracle/conflicts.pas`, which writes `ConflictAllForMainRecord` and `ConflictThisForMainRecord` of every main record of every loaded file, files in load order and records in `RecordByIndex` order (the order matters for the GMST and DFOB records, which upstream groups by editor ID and caches on the first record asked).

A record that is the only one of its FormID (`caOnlyOne` with `ctOnlyOne`) is counted per file, the others are listed.

The output is cached as `<cache>/<tag>/<MODE>-oracle-conflicts/<set>.<key>.txt.zst` (the key hashes the plugins and the script; `vanilla` is the whole corpus of the game, `selected` a `--file` set).

The port loads the same plugins in the load order the GUI reports (the hardcoded executable file loads with them), from `<scratch>/<tag>/<MODE>-conflicts/Data`, a folder of hard links to the plugins only, as the GUI's private data folder has no archives and no strings files: a localized string resolves to `<Error: No strings file for lstring ID ...>` on both sides.

It runs `conflicts.list` and compares every listed record and the counts of each file.

Statuses: `equal`, `different` (the first 20 records and the file counts that differ), `oracle-failed`, `oracle-unsupported` (Morrowind: the GUI runs it in view mode only, without scripts), `port-failed`, `oracle-only`.

The report is `target/parity/conflicts.json`.

The GUI runs take minutes (all of Fallout 4 with its 181 files about 6 minutes, Starfield and Fallout 76 longer).

`--record <FormID>` (with `--game` and usually `--file` to keep the load small) runs `crates/xtask/oracle/conflicts-probe.pas` on both sides instead and diffs them: for each record the status of the master and of its first override, the pair (`ConflictAllForElements`), and the elements of the two walked side by side by index with the path, the `DisplayName`, the `Name`, the `ConflictAllForElements` of the pair (`-1` when one side has no element), the extended sort keys of both (`<none>` for a missing side; a flag's own key is `<flag>`, because it embeds the runtime definition id) and `Check(el)` when the element has an error.

The port's side is the `conflicts.probe` command, which the harness runs as one `xedit call`; both sides load the plugins from a folder that has only the plugins (no archives, no strings files), so the localized strings resolve the same. `--hidden <FormID>` (repeatable) sets `esHidden` on those records first on both sides (`records.set_hidden` and `SetElementState(el, 5)`), and the run exits non-zero on a difference; `crates/xtask/src/parity/conflicts.rs`'s `KNOWN_DIFFERENCES` accepts the recorded ones with their reason and counts them as `known-difference`.

Nothing is cached; the oracle's `probe.txt` is left in `<scratch>/<tag>/<MODE>-conflicts`.

Results 2026-10-16 (parity #34, branch `parity-34`): over `DOBJ:00000031`, `CELL:00000D74`, `FLST:00000163`, `AVIF:000005E9`, `QUST:00013EB3`, `QUST:01000803` and `REFR:01000801` in `Skyrim.esm`, `Update.esm` and `Dawnguard.esm`, 2203 oracle lines and 2203 port lines with **0 different and 1 known-difference** (the `DOBJ` name: the release reports no editor ID for a record whose EDID only the internal edit adds, the port caches it when the added subrecord's element is built); the same set with `--hidden 00013EB3 --hidden 01000803` compares 2203 lines with the same counts. `cargo xtask parity conflicts --game sse` (11 files, 1,188,933 records, 20,782 listed) stays equal.

What the gate taught (all fixed): the session builds its definitions with the GUI's settings (`wbSimpleRecords`, `wbHideUnused`, `wbCreateContainedIn`), not xDump's; the view compares the flags of a flags value one by one (`wbFlagsAsArray`); a topic of the games that sort their responses (`wbSortINFO`) has the `INOM` and `INOA` lists once it is built; Starfield's light and medium masters count their own slots (`wbComplexFileFileID`); and an element knows its memory order from the moment its container adds it (`AddElement`), which the rank slot arrays of a Starfield `LGDI` read while they are sized.

#### Phase 4 step 2 results (2026-10-08)

`cargo xtask parity conflicts`, 2026-10-08, the vanilla plugins of each game loaded together: 10 of 10 games equal, 23,383,514 records compared (422,552 that are not the only record of their FormID listed record by record, the rest counted per file): Fallout 4 181 files 2,248,005 records, Skyrim SE 11 files 1,188,933, Oblivion 12 files 1,190,878, Fallout 3 7 files 932,219, New Vegas 11 files 629,839, Skyrim LE 6 files 1,161,503, Skyrim VR 7 files 1,177,152, Fallout 4 VR 3 files 1,548,595, Fallout 76 3 files 5,816,095, Starfield 15 files 7,490,295; Morrowind `oracle-unsupported` (the GUI runs it in view mode, without scripts).

The oracle takes about 6 minutes for Fallout 4, the port's `conflicts.list` seconds (Skyrim SE 3.8 s on 16 threads, 12.6 s on one; Fallout 76 23 s and 48 s; the output is byte for byte the same).
