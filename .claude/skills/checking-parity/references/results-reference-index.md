# Reference index



`parity refs` checks the reference index (phase 4 step 1) game by game: the GUI oracle loads all the vanilla plugins of the game (`-script:` with `crates/xtask/oracle/refs.pas`; the script mode builds the reference information on load) and writes `ReferencedByCount` and `ReferencedByIndex` of every master record, and keeps the reference cache files it writes (`-C:`).

The port loads the same plugins in the oracle's load order, from links to them in `<scratch>/<tag>/<MODE>-refs/data` (no archives, no strings, as the oracle's private folder), with `--edit` (the GUI edits, so the load fix-ups run where `wbAllowInternalEdit` is off) and writes the lists with `xedit refs dump`, twice: once building the index and saving its cache files into `<MODE>-refs/cache`, once loading them.

The lists are compared record by record and entry by entry (files by name, not load order), and each oracle cache file with the port's of the same plugin after decompression (the names differ in the program's CRC32; the compressed bytes differ because upstream uses LZ4 HC).

The oracle's output is cached in `<cache>/<tag>/<MODE>-oracle-refs` (`refs.<key>.txt.zst`, `.status`, the folder `.refcache`).

The report is `target/parity/refs.json`; a difference names the first records with the first differing entry, and for a cache file the first record and field.

Morrowind is skipped (upstream builds no references for it).

The oracle needs about two minutes for Fallout 3 and over an hour for Starfield; `--oracle-timeout` raises the default of two hours.

Results 2026-10-08: equal for Oblivion, Fallout 3, New Vegas, Skyrim, Skyrim SE, Skyrim VR, Fallout 4, Fallout 4 VR and Fallout 76 (33.2 million entries of 5.8 million records, 70 of 70 cache files); Starfield lacks 18 entries in 17 records, which the perk activity JSON gives in upstream (`wbPerkActivityAfterLoad`, not ported).

The other checks in [the checks reference](checks.md) are added to `crates/xtask/src/parity.rs` in the phase that ports the feature.
