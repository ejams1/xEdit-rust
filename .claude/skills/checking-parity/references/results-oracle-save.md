# Oracle save



`parity oracle-save` has the GUI build of the oracle (`xTESEdit.exe`, `xFOEdit.exe`, `xSFEdit.exe` in `XEDIT_ORACLE_DIR`) load every corpus plugin with its masters and write it with `FileWriteToStream` (`crates/xtask/oracle/save.pas`), which runs the `PrepareSave` and `WriteToStream` of the GUI's save, and compares the port's save (as the round trip makes it) with it byte for byte. Statuses:

- `equal`: identical bytes; the detail says whether the oracle's save equals the input.
- `equal-error`: both refused the save with the same message (the oracle's exception, the port's `save_refused`). Counted as passing.
- `different`: the first differing offset, the record report (dropped: only in the oracle's save, added: only in the port's) and the two files, kept in the scratch folder as `<file>.<hash>.oracle.saved` and `.port.saved`. Also a refusal on one side only, or with another message.
- `oracle-unsupported`: Morrowind. The 4.1.5q GUI runs Morrowind in its view mode only (`ToolModes := [tmView]` in `xeInit.pas`) and saves nothing, so there is no oracle save.
- `oracle-failed`: the GUI run failed (a dialog, a timeout, a hang); the detail has the reason. Nothing is cached; run the file again.
- `port-failed`, `port-memory-limit`, `oracle-only`: as for the round trip.

The oracle's result is cached as `<cache>/<tag>/<MODE>-oracle-save/<file>.<key>.saved.zst` (zstd) or `<file>.<key>.error` (the exception's message), with the script's log as `.oracle.log`; the key hashes the plugin, its masters (found through the `MAST` subrecords, recursively) and the script, so a change to `save.pas` runs the oracle again on everything.

The script text is hashed with its line endings normalised to LF (and `.gitattributes` keeps `crates/xtask/oracle/*.pas` LF), so a CRLF checkout under `core.autocrlf` finds the same cached saves.

The GUI is fast: `Starfield.esm` loads and saves in about 2 minutes, the whole corpus in about an hour with `--jobs 3`, most of it the port's saves of the big masters.

How a GUI run works (`crates/xtask/src/parity/gui.rs`), and what to check when one fails:

- The GUI's script mode (`-script:<file>`) loads the plugins, runs the script and stays open; `-autoload` and `-autoexit` work in the edit mode only. The harness presses OK on the module selection (`TfrmModuleSelect`, with the plugins of the private plugin list checked), waits for `done.txt` (the script writes `status.txt` and then the marker), and ends the process through its job object, which also kills it when the harness dies.
- Every run gets a private folder under `<scratch>/<tag>/oracle-work`: a copy of the executable (the GUI writes its log, ini and exception log next to it), copies of the plugins as the data folder (`-D:`), a plugin list (`-P:`, with `*` where the game uses it), an empty game ini (`-I:`, with `-M:` and `-G:`, so no archive and no setting of the user's `My Games` folder applies, and a game that was never started does not stop the GUI with `Fatal: Could not find ini`), and `-T:`, `-C:`, `-B:`, `-S:`.

Nothing is written to a game install, the user's `Plugins.txt` or the settings next to the oracle binaries.

The folder is removed after the run.
- Any other dialog that stays up for 10 seconds ends the run with its texts (a Windows task dialog shows only its buttons), as does a `Fatal:` line or `Aborted: Applying script` (a script that did not compile) in the message log of the main form, the timeout (`--oracle-timeout`, default 120 minutes) or 10 minutes without CPU time. The windows of the GUI appear on the desktop while it runs.
- JvInterpreter resolves names without case and before locals: a parameter called `path` or `name`, or a variable `e` next to `on E: Exception`, breaks a script (`Undeclared Identifier 'Message'`), so the scripts use `aPath`, `aName`, `el`.
- The script mode builds the reference information unless `-nobuildrefs` is passed; the plain save passes it. Building the references initialises every record of the loaded files, which applies their load fix-ups (required members, `AfterLoad`), so a save then differs from a plain one wherever a record has a fix-up.
