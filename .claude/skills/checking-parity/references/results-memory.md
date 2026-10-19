# Memory



Every oracle and port process runs in a Windows job object that caps its committed memory at `--max-memory` (default: the budget), so a runaway dump fails alone instead of pushing the machine out of memory.

A port that reaches the cap is reported as `port-memory-limit`; an oracle that reaches it is reported as `oracle-failed` and its output is not cached.

The peak of each complete run is stored next to the cache as `.oracle.peak` and `.port.peak` and shown in `dump.json` as `oracle_peak` and `port_peak`.

The processes that run at once stay within `--memory-budget` (default: three quarters of the installed memory).

Each run reserves the peak its file had in the last run, or, before the first run, 6 times the size of the plugin and its game master; a file whose reservation exceeds the budget runs alone. `--jobs` (default 3) is only an upper bound on the parallel runs.

The port dumps on all CPUs (phase 3 step 8): the largest port peak of the Fallout 4 and Skyrim SE corpus is 2.5 GiB (`DLCCoast.esm`; 1.6 GiB on one thread), `Skyrim.esm` peaks at 1.8 GiB.

The wall clock is bounded by the largest file: the port dumps `Skyrim.esm` (8.3 GB of text) in about 30 seconds and `Update.esm` in about 4 seconds on the reference machine (16 threads; 3 minutes and 28 seconds on one).

Each port process uses every CPU, so `--jobs` above 1 mostly overlaps the oracle runs with the port's.

`parity saves` runs `xDump.exe -saves` and `xedit saves dump` on every save and co-save in the save folder of each game (`.fos`, `.ess`, `.f4se`, `.skse`, `.nvse`, `.fose`, `.obse`), with the game's data folder as `-D:` and `--data`, and caches the oracle output under `<cache>/<tag>/<game>-saves`.

The report is `target/parity/saves.json`.

The oracle is far too slow on Skyrim LE saves to finish: it raises an exception for every hardcoded FormID it cannot resolve (see `using-xedit-cli`) and writes about 1 KB per second, where the port writes 140 MB for `quicksave.ess`. `--oracle-timeout <minutes>` stops the oracle after that long and keeps its output as `<file>.<hash>.oracle.timeout.txt.zst`, compared as a prefix like a crashed run (`equal-prefix`).

The Skyrim LE saves were checked with `--oracle-timeout 15`; delete the `.oracle.timeout.txt.zst` of a save to extend its prefix.

Oblivion saves need `XEDIT_TES4_SAVES` (`.ess`, `.obse`) and Fallout 3 saves `XEDIT_FO3_SAVES` (`.fos`, `.fose`); on this machine both folders are under `M:\projects\saves`.
