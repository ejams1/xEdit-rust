## Output conventions

- **Messages.** `AddMessage(text)` appends to xEdit's message log, which is the run's observable output: without `--json` the log lines go to stdout and an abort also writes `error: <location>: <message>` to stderr; with `--json` they are `messages` in the response. Use it for progress and results; a report file goes to `wbOutputPath`.
- **A `#0` in a message.** The port logs the whole line. The GUI's message log is a SynEdit memo, which shows a line only up to a NUL in it, and lines that reach it in the same flush as a line with a NUL before them are lost with every line after them (an upstream bug: the log stops for the rest of the run). Log control characters as `#<code>` text when the oracle's log has to be read.
- **NaN.** The floating point exceptions are masked: `0.0/0.0`, `Inf - Inf`, `Inf * 0`, `Sqrt(-1)` and `Ln(-1)` are NaN and `1/0` is `INF`, nothing raises `EInvalidOp` or `EZeroDivide`. A NaN compares equal to every number, as the RTL's variant comparison answers equal when neither `<` nor `>` holds (`NaN = 1` is `True`, `NaN < 1` and `NaN > 1` are `False`; `float-invalid.pas`), so test it with `IsNan`.
- **The run log line shapes**, as the oracle's log has them (the `[mm:ss]` prefix is the progress callback's; the text after it is what the harness compares):
  - `[mm:ss] Start: Applying script "<name>"` — dropped for `Mode: Silent`.
  - the script's `AddMessage` lines, each `[mm:ss]`-prefixed.
  - `[mm:ss] Done: Applying script "<name>", Processed Records: <n>, Elapsed Time: <mm:ss>`.
  - `You can close this application now.` — no prefix (it is a plain message log line, `DoRunScript`'s last).
  - per changed plugin `[mm:ss] Saving: <name>.save.<yyyy_mm_dd_hh_nn_ss>`, then `[mm:ss] Queued renaming of save "<data path><name>.save.<time>" to "<data path><name>" on shutdown.`, and a closing `[mm:ss] Done saving.` — after the closing line of a completed run, the last lines of an aborted one.
  - a failure ends the log with `[mm:ss] Exception in unit <u> line <n>: [<class>] <message>` and `[mm:ss] Aborted: Applying script "<name>"`, with no `Done:` and no closing line; the run exits non-zero, and plugins the script changed before the failure are still saved at the end unless `--dry-run`/`--no-save` (a one-shot run stands in for the GUI's close, which saves them too — the harness's aborted save case records the same on the oracle).
- **Files.** A script writes its own files; xEdit's convention is `wbOutputPath` (a trailing separator included), which the port starts as the data folder when nothing set it (`-O:` on the legacy command line sets it) and a script may set itself (`wbOutputPath := 'C:\somewhere\'`).

During a run `wbScriptsPath` is the script's own folder and `wbTempPath` the temp folder. xEdit's `cargo xtask parity script` harness captures a run's files by where they were written (`out/` for `wbOutputPath`, `script/` for the run folder, `program/` beside the executable, `data/` for the private data folder), which is a good model for where a script should put things.
- **The filter.** `ApplyFilter` runs when the call that asked for it returns (the run state applies the recorded variables then), so the filter's own log lines appear after the messages of the call that triggered it; `FilterApplied` answers true from the moment it is called, as upstream.

With `FilterScripted` on the filter calls the script's `Filter(e: IInterface): Boolean` back through the interpreter, once per main record node its walk reaches (unassigned and 0 are False; a variant that does not convert raises `EVariantTypeCastError`, and a `Filter` that raises aborts the run with `Error during Applying Filter: <message>` then the `Exception in unit ...` and `Aborted:` lines).

The flag is read before every record, so a `Filter` that clears it stops the scripted checks there, and the filter clears it when it ends.

The `Process` walk after a filter sees the nodes the filter kept alone.
