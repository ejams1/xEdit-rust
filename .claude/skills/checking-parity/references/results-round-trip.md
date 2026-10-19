# Round trip



`parity roundtrip` runs `xedit --edit --game <game> --load <plugin> save --no-backup --output <scratch>/<tag>/<game>-roundtrip/<file>.<hash>.saved` on every corpus plugin and compares the saved bytes with the input, byte for byte.

No oracle binary runs: the input file is the oracle, which holds exactly where xEdit writes a loaded file back unchanged, and the GUI oracle's own save decides wherever `parity oracle-save` has cached it (below): a save that equals the oracle's save is `equal` whatever it changed in the input (the detail says how the oracle's save differs from the input), one that does not is `different`, and a refusal with the oracle's message stays `refused` with the oracle's confirmation in the detail.

Only a file without a cached oracle save falls back to the classes below that judge the input alone.

The report is `target/parity/roundtrip.json` and the statuses are:

- `equal`: identical bytes; the saved file is deleted.
- `ofst-dropped`: the saved file is the input without the `OFST` subrecord (and its `XXXX` size prefix) of every worldspace (and `CLSZ` for Fallout 4, Fallout 76 and Starfield, `VISI` for Fallout 76, the `RNAM` of the game master), with the record and group sizes adjusted. xEdit drops those offsets when it loads a worldspace and writes the record without them, so this is the expected result for a file that overrides a worldspace; the harness rebuilds the expected bytes from the input and compares.

Not counted as passing until the oracle's own save confirms it (step 6 of phase 3).
- `header-edited`: the bytes after the file header record are the input's (or the input's without the `OFST` data), and only the header record differs; the detail names what changed (`HEDR 1234 -> 1233 records`, `INCC added (12 interior cells)`, `ONAM added (857 entries)`, `flags 00000000 -> 00000001`).

Those are the edits upstream makes to the header on every save, so this is the expected result for a master or a file whose header is off; step 6 confirms the exact bytes against the oracle's own save.
- `structure-edited`: every main record of the saved file is the input's (or the input's without the `OFST` data), but the header and the groups changed: the load drops an empty top level group and merges a duplicated one, as upstream does, and the detail gives the group counts (`groups 50494 (1013 empty) -> 50487 (1006 empty)`). Expected for the same reason as `header-edited`.
- `different`: the first differing offset is printed, the saved file kept, and the detail lists the records that changed, were dropped or were added by signature and FormID, with the first few sizes.

Investigate as any difference, but check first whether upstream itself changes the records on save in a way the input cannot show (the subrecord diff of the step 2 results below tells the three known cases apart): it initialises every worldspace record on load, so `wbWorldAfterLoad` adds the members the parent flags call for (`CNAM`, `NAM2`, `DNAM`, `MNAM`, `INAM`; a worldspace grows by 10 or 20 bytes) and the required members are added; `PrepareSave` clamps a FormID whose file index is beyond the masters (the record is reported dropped and added under its new FormID) and sets the persistent flag of an `ONAM` override whose earlier override has it; and `PrepareSave` marks the child group of a worldspace whose offsets were dropped modified, so every record under it is rebuilt from its elements (its `AfterLoad` fixes apply, its subrecords are sorted, a short `DATA` is written at its definition's size, compressed records are compressed again with libdeflate level 12).

Those files need the oracle's own saved bytes, which step 6 adds (the GUI build driven headlessly on a copy of the corpus).
- `refused`: the port refused the save with an upstream `PrepareSave` message, which the oracle gives for the same file (a Starfield blueprint module, for example). Counted as passing.
- `unsupported`: a save upstream edits in a way this version cannot yet. None since the element editing step.
- `port-failed`, `port-memory-limit`: as for the dump.

The saves run under the same memory cap and budget as the dumps and reuse the `.port.peak` files. The port's log of each save is `<file>.<hash>.port.log` next to the saved file.

#### Step 1 results (2026-10-07)

249 plugins of 11 games in 1.5 minutes: 132 `equal`, 38 `ofst-dropped`, 2 `refused` (the Starfield blueprint masters), 77 `unsupported`, 0 `different`.

The `unsupported` saves are the header edits upstream makes on save: 39 `ONAM` rebuilds, 34 `HEDR` record counts that differ from the loaded tree because the load drops empty top level groups and merges duplicated ones (verified on `Dragonborn.esm`: the raw file holds an empty `AVIF` group the oracle and the port both drop, so the count is one less), the ESM flag of the 3 Morrowind masters, one clamped FormID (`FalloutNV`).

The numbers are also in the phase 3 status of `docs/PLAN.md`.

#### Step 2 results (2026-10-07)

249 plugins in 8 minutes: 125 `equal`, 46 `ofst-dropped`, 24 `header-edited`, 15 `structure-edited`, 5 `refused`, 0 `unsupported`, 34 `different`, 0 failed.

The 3 Morrowind `refused` are a port gap (`must have a FormID`: the identity FormID of a Morrowind record, owed).

Every `different` file was diffed at subrecord level with `cmp.py` (a scratch script: walks the groups of both files, pairs the records by FormID, decompresses, and prints the subrecords that differ) and is one of the three upstream changes listed under `different` above: 25 files whose worldspaces gained the members `wbWorldAfterLoad` adds (`CNAM` with its default `351` in a Skyrim worldspace, `INAM` too in Fallout 3), 9 files with a clamped FormID (`Oblivion.esm` 10 `REFR`, `Skyrim.esm` the `GMST` `0123C00E`, the `INFO`/`QUST` of the Fallout 3 DLC, `GunRunnersArsenal.esm` 1 `REFR`), and the Skyrim masters (359 `CELL` records of `Tamriel` rebuilt with their `XCLW` fix, sorted subrecords and a two byte `DATA`; 4 `REFR` overrides of `Dawnguard.esm` set persistent by the `ONAM` rebuild).

None is a port bug by that analysis; the oracle's own save (step 6) decides.

#### Step 6 results (2026-10-07)

With the oracle's saves cached: 239 `equal`, 10 `refused` (7 confirmed by the oracle's refusal: the two blueprint masters and the official Starfield modules whose header `PrepareSave` would have to edit; the 3 Morrowind masters unconfirmed, as the oracle saves no Morrowind plugin), 0 `ofst-dropped`, 0 `header-edited`, 0 `structure-edited`, 0 `different`. Every file of the classes above equals the oracle's save.
