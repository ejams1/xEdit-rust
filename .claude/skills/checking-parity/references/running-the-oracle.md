## Running the oracle

`XEDIT_ORACLE_DIR` points at the unpacked release archive of the baseline tag.

The dump checks run `xDump.exe` as below; the save checks run the GUI builds as described under "Oracle save".

The game is selected with a switch such as `-FO4` or `-SSE`.

Masters are read from the directory of the input file, and `-D:<Data path>` is needed for every plugin but the game master: without it the oracle loads the hardcoded records, cannot find the game master again and stops with `EOSError: System Error. Code: 2`.

```
"$XEDIT_ORACLE_DIR/xDump.exe" -FO4 -q "-D:$XEDIT_FO4_DATA" "$XEDIT_FO4_DATA\<plugin>" > dump.txt 2> dump.log
"$XEDIT_ORACLE_DIR/xDump.exe" -SSE -q "-D:$XEDIT_SSE_DATA" "$XEDIT_SSE_DATA\<plugin>" > dump.txt 2> dump.log
```

Pass the plugin path with backslashes and a drive letter. The oracle does not find masters when the path uses forward slashes. The oracle exits with 0 after an exception; a complete run ends `dump.log` with `All Done.`.

The dump goes to stdout and progress goes to stderr. `xDump.exe -FO4 -?` lists the options. The plain dump prints `[S]: <summary>` after an element without value whose summary is not empty (`DumpSummary` is on by default), and the file header record itself has no name line: its elements start at column 0.

A whole-file dump of `Fallout4.esm` (`-FO4 -q`) wrote 12.4 GB in 1 h 53 min and then stopped with `Unexpected Error: <EAccessViolation ...>` while dumping INFO `[000673E3]`, without `All Done.`.

The crash is deterministic: `wbConditionAliasToStr` resolves the alias of an INFO condition through `INFO\Topic`, which Fallout 4 INFO records do not have, so the first FO4 INFO with an alias condition kills the dump (`DLCRobot.esm` at `[010102A3]`, `DLCworkshop03.esm` at `[010024C4]`, `DLCCoast.esm` at `[0013C634]`, `DLCNukaWorld.esm` likewise).

The port prints an empty alias there.

Treat the oracle output of a crashed run as valid up to the last complete record and compare only that prefix (`head -c $(stat -c %s oracle.txt) port.txt | cmp - oracle.txt`); the records after the crash need a plugin without such conditions.

Skyrim SE files dump completely.

**A crashed oracle can hang instead of exit.** On a Fallout 4 save the oracle can raise `EAccessViolation` and then stay alive without ever exiting, spinning on nothing: the harness waits on its output forever and the step never ends. The `xDump.exe` process is then the thing to stop, and `--oracle-timeout <minutes>` is the harness's own remedy: it stops the oracle after that long, keeps its partial output as `<file>.<hash>.oracle.timeout.txt.zst` and compares it as a prefix (`equal-prefix`), which is the same evidence a crashed run gives. Give the saves mode a timeout whenever the corpus is a live save folder, because the folder grows and a new save is a new hang.

The `xEdit-llm` automation build is a secondary oracle for conflict, reference and cleaning results as JSON. It is a fork at a different upstream commit: use it to cross-check, never to close a gate.
