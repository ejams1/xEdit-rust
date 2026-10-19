## Running the harness

```
cargo xtask parity dump|saves|roundtrip|oracle-save|oracle-edit|conflicts|refs|clean|clean-scenarios|check|check-dump|modgroups|merged|filter|tool-modes|script [--game <game>]... [--file <name>]... [--record <FormID>]... [--mode <tool mode>]... [--oracle-only]
                    [--jobs <n>] [--memory-budget <GiB>] [--max-memory <GiB>] [--oracle-timeout <minutes>]
```

The harness runs the oracle `xDump.exe` and the port's `xedit dump` on every vanilla plugin of the selected games and compares the outputs byte for byte.

The games are `fo4`, `sse`, `tes3`, `tes4`, `fo3`, `fnv`, `tes5`, `tes5vr`, `fo4vr`, `fo76` and `sf1`.

Without `--game` it checks every game whose data directory is set and prints a `skipped` line for the others. `--file` restricts the run to named plugins, vanilla or not.

It prints the first differing line of each file, writes `target/parity/dump.json` and exits with an error when any file differs.

Environment variables:

| Variable | Content |
| --- | --- |
| `XEDIT_ORACLE_DIR` | Unpacked release archive of the baseline tag. |
| `XEDIT_<GAME>_DATA` | `Data` directory of each game: `XEDIT_FO4_DATA`, `XEDIT_SSE_DATA`, `XEDIT_TES3_DATA` (Morrowind's `Data Files`), `XEDIT_TES4_DATA`, `XEDIT_FO3_DATA`, `XEDIT_FNV_DATA`, `XEDIT_TES5_DATA`, `XEDIT_TES5VR_DATA`, `XEDIT_FO4VR_DATA`, `XEDIT_FO76_DATA`, `XEDIT_SF1_DATA`. |
| `XEDIT_<GAME>_SAVES` | Save folder of each game for `parity saves`: `XEDIT_FO4_SAVES`, `XEDIT_SSE_SAVES`, `XEDIT_TES5_SAVES`, `XEDIT_FNV_SAVES`, `XEDIT_FO3_SAVES`, `XEDIT_TES4_SAVES`. |
| `XEDIT_PARITY_CACHE` | Cache directory. Optional; defaults to the user cache directory. Shared: it holds the expensive oracle outputs. |
| `XEDIT_PARITY_SCRATCH` | Where the port's saves (`roundtrip`, `oracle-save`, `oracle-edit`), the files kept for a difference and the oracle's run folders go. Optional; defaults to the cache. Set it per branch when several branches run the round trip at once. |
| `XEDIT_ORACLE_KEEP_WORK` | Set to keep the run folder of a GUI oracle run (the generated script, the private data folder) to read it or start the GUI on it by hand. |

Oracle output is cached zstd-compressed as `<cache>/<tag>/<game>/<file>.<hash>.oracle.txt.zst`, so the oracle runs once per input file; plain `.oracle.txt` files from older runs are still read.

A run that the oracle ended with `Unexpected Error` is kept as `<file>.<hash>.oracle.crashed.txt.zst`, with the exception's log line in `<file>.<hash>.oracle.error`, and compared as a prefix: the port has to match it up to the crash, and the rest of its output must still finish without an error, which the report shows as `equal-prefix`.

A port that stops with the same error message after the same output (`error: <message>` as its last log line) is reported as `equal-error`; the Oblivion saves end that way, because upstream's Oblivion save definitions expect the Fallout 3 header magic.

An oracle run that ended with `EInOutError` (I/O error 1450 under memory pressure) is not kept and is reported as `oracle-failed`; run the file again.

An oracle assertion failure while loading (`EAssertionFailed` in `wbImplementation.pas`) was seen once on a Fallout New Vegas save and did not repeat: delete the `.oracle.crashed.txt.zst` and run the file again before reading anything into it.

The oracle is slow on large masters: it writes about 80 MB of dump per minute and needs more than ten minutes for `Fallout4.esm`. `--oracle-only` fills the cache without running the port.

The port output is compared while the port writes it; it is kept only for a file that differs, as `.port.txt.zst` up to the first difference (`zstd -dc` reads it), next to the port's log `.port.log`.
