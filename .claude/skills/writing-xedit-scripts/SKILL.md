---
name: writing-xedit-scripts
description: Use when an agent needs custom batch logic over Bethesda plugin data in this repository and the typed commands of xedit are not enough, and a Pascal script of the xEdit JvInterpreter dialect is the right tool. Covers choosing a script over xedit batch, running one with xedit script list, check and run, the --edit gate and --dry-run and --no-save, where the script API lives (xEditAPI.pas and the coverage ledger), the dialect rules the phase established, what answers headless and what the Vcl seam refuses, the output and message-log conventions, and the known divergences.
---

# Writing xEdit scripts

The port runs xEdit's Pascal scripts with a JvInterpreter-compatible interpreter (`crates/xedit-script`), driven by the port of `xeMainForm.pas`'s `DoRunScript`/`ApplyScript` (`crates/xedit-session/src/script.rs`). The compatibility goal is the phase-6 gate: the 150 shipped `.pas` scripts of the 4.1.5q `Edit Scripts` folder parse (150 of 150), and every form-free script that runs produces the oracle's output. This skill says when to reach for a script, how to run one, and what the port does and does not answer.

## Script or `xedit batch`?

Use `xedit batch` (see the `using-xedit-cli` skill) when the work is a list of commands you can name up front: a few `elements set`, `records copy`, `masters add`, then `save`. Use a script when the logic depends on the data:

- Walk every record of a load order and decide per record (filtering, checking, renaming with a condition, collecting totals).
- Keep state across records or recurse (`Initialize` state, `Finalize` writing a report; `Check for errors.pas` recurses the element tree).
- Write report or data files in a format you control (`wbOutputPath`).
- Run an existing script unchanged. The corpus scripts (and mod authors' scripts) are the compatibility surface; prefer running one over rewriting it as commands. `xedit script list` names the folder's scripts, and `XEDIT_SCRIPTS` points it at another folder.

A script is also the only way to exercise the script API itself, but it is not always the safer tool: a script that builds or shows a form cannot run headless (see "Headless and the Vcl seam"), and a script's failure saves nothing only when it aborts before its `SaveChanged` at the end. For a quick, reviewable change, commands are easier to dry-run and read back.

## Running one

```
xedit script list [--scripts DIR]                       # script.list, read-only
xedit script check MyScript.pas [--json]                # script.check, read-only
xedit --edit --game sse --load "<Data>\Update.esm" \
      script run MyScript.pas [--scripts DIR] [--dry-run] [--no-save]
```

- `script list` lists the `*.pas` of the scripts folder (the top folder only) with line counts, as the form's list shows them. The folder is `--scripts`, else `XEDIT_SCRIPTS`, else the oracle's `Edit Scripts` (`XEDIT_ORACLE_DIR`), else `Edit Scripts` beside `xedit.exe` — upstream's `wbScriptsPath`, which `-S:` sets.
- `script check <file>...` compiles each script and the units it `uses`, exactly as `TJvInterpreterUnit.Compile` accepts them: a routine body is only scanned for its balanced `end`, so a statement-level error inside a body surfaces when the script runs, not here. It exits 0 when all compile, else non-zero listing each failure as `file:line: message` (with `--json`, the structured result). A unit with no file beside the script is a unit compiled into the host and parses to nothing.
- `script run <file>` is xEdit's `-script:` mode: the script compiles, `Initialize` runs, `Process` runs per selected element, `Finalize` runs, then every plugin the run changed is saved (the GUI's `SaveChanged` on close; an existing file is moved to the backup folder). A relative file name resolves against the scripts folder, but the units a script `uses` always resolve from the script's own folder, as `DoRunScript` sets `wbScriptsPath`.
- `--dry-run` runs the script with editing disabled and saves nothing; `--no-save` leaves the changed plugins in memory, for `xedit save` in the same `batch` or `serve` session. A one-shot run with `--no-save` and no save afterwards loses the changes. Needs `--edit` unless `--dry-run` (registry gate, as every mutating command); `script list` and `script check` need no `--game` or `--load`.
- Exit status: `script check` exits non-zero when a script does not compile; `script run` exits non-zero when the script aborted. With `--json` the run response has `file`, `script`, `dry_run`, `processed` (the `Process` calls that returned), `aborted`, `self_terminated`, `error`, `error_location`, `files` (changed/saved) and `messages` (the run log). `xedit schema` (or `call script.run --params '<json>'`) is the authority on fields; `save` is true by default on the registry request, so `--no-save` is `"save": false` through `call`/`batch`.
- The `-script:<file>` legacy switch and `xedit tool run script --script <file> [--scripts DIR]` run the same path and save as the auto modes do.
- A run needs no selection option: headless, `DoRunScript` selects every root node of the navigation tree, so `Process` sees every loaded file's elements from the last back to the file node (a file's records in the reverse of their order, the file node last). The script narrows this in `Initialize`: `ScriptProcessElements` (default `[etMainRecord]`; `[etFile]` selects the file nodes) and `ScriptProcessSignatures` (a comma-separated list of 4-character signatures, upper-cased and matched as a substring).

A safe sequence, dry run first, then save somewhere else through `batch`:

```
xedit --game sse --load "<Data>\Update.esm" script run MyScript.pas --scripts <scripts> --dry-run
xedit --json --edit --game sse --load "<Data>\Update.esm" batch - <<'JSON'
[
  {"command": "script.run", "params": {"file": "MyScript.pas", "scripts": "<scripts>", "save": false}},
  {"command": "files.save", "params": {"file": "Update.esm", "output": "<scratch>\\Update.esm"}}
]
JSON
```

## Where the API lives

- **`xEditAPI.pas`** in the oracle's `Edit Scripts` is the declaration reference: 1,633 lines declaring 202 routines, the `IInterface` element type and the constants a script compiles against. An agent writing a script should read the declarations there for signatures. At run time the host stubs `xEditAPI` (as upstream does), so `uses xEditAPI` resolves to the host's registrations; the port's `script check` parses the real file when the folder holds one, because that file is part of the corpus gate.
- **`coverage/ledger.toml` is the source of truth for the ported surface.** Every registration is a `[script."<Unit>:<Class>.<Member>:<kind>"]` entry (`kind` is `function`, `const`, `get`, `set`, `iget`, `iset`, `idget`, `idset`, `class`, `rec`, `recget`, `recset`, `handler`, `extfun` or `extunit`) with `status = "covered"` (and `command = "script.run"`), `"excluded"`, or `"pending"`. At the phase-6 tip: 3,663 entries — 2,392 covered, 1,240 excluded, 31 pending. The 31 pending are 30 functions of `xejviScriptAdapterHost.pas` (`AddNewFile`/`AddNewFileName`, `JumpTo`, `RemoveNode`, `RecordByFormID`/`RecordByHexFormID`/`RecordFromFileByFormID`, `FileByLoadOrder`/`FileByLoadOrderFileID`, `wbSelectedFiles`/`wbSelectedFilesToFileNames`, `wbSettings`/`wbSettingsFileName`, `wbSimpleRecords`, `wbRecordDefMap`, `wbLoadBSAs`, `wbTrackAllEditorID`, `IsPositionChanged`, `ExecuteCaptureConsoleOutput`, the conflict helpers `ConflictAllForElements`/`ConflictAllForNode`/`ConflictThisForNode`, the LOD and atlas builders `GenerateLODTES4`/`GenerateLODTES5Objects`/`GenerateLODTES5Trees`/`wbBuildAtlasFromAtlasMap`/`wbBuildAtlasFromTexturesList`/`wbDecodeTextureHashes`/`wbGetUVRangeTexturesList`, and the form handles `frmMain`/`frmFileSelect`) and one `JvInterpreter` entry. A script that calls a pending name aborts with `Undeclared Identifier '<name>'`. `excluded` is used for registrations commented out upstream and for GUI-only surface (the VCL members the form shim holds for the GUI phase).
- Check the ledger before promising an agent-written script will run: grep the member name in `coverage/ledger.toml` and read its `status`.

The surface, by where it lives in the port:

| Unit(s) | Surface | Port |
|---|---|---|
| `xejviScriptAdapterHost.pas` | the file list (`FileCount`/`FileByIndex`/`FileByName`), the paths and game names, the message log (`AddMessage`, `ClearMessages`), `ApplyFilter`/`FilterApplied`/`RemoveFilter` | `crates/xedit-session/src/script.rs` |
| `Element`, `Record`, `File`, `Container`, `Resource` | elements and their values (the `wbInterface` API), records and overrides, groups and signatures, FormID conversion, containers, `TwbAsset`, hashes, `TStrings`/`TList` fills | `crates/xedit-session/src/script_units/` |
| `xejviScriptAdapterDF.pas` | `TdfDef`/`TdfElement` over the NIF, material, LOD, FUZ and DDS code (`TwbNifFile`, `TwbBGSMFile`, ...): `NativeValues`/`EditValues`, `ToText`/`ToJSON`/`FromJSON`, `SaveToFile`, `LoadFromResource` | `crates/xedit-session/src/script_df.rs` |
| `xejviScriptAdapter.pas`, `Misc`, `MiscJson`, `MiscRegEx` | the `gm*`/`es*`/`dt*`/`ct*`/`res*`/`at*` constants, `Assigned`, `FullPathToFilename`, `GetRecordDefNames`, `wbFilterStrings`, `wbRemoveDuplicateStrings`, `wbVersionNumber`; `TStringList`/`TEncoding`; StrUtils/SysUtils/Math/Variants/WinAPI helpers; `JsonDataObjects` (`TJsonObject`/`TJsonArray`); `TPerlRegEx` | `script_adapter.rs`, `script_strings.rs`, `script_classes.rs`, `script_misc*.rs` |
| JVCL `JvInterpreter_System`, `_SysUtils`, `_Math`, `_Classes`, `JvInterpreterFm` | the RTL a script calls without `uses`: `IntToStr`/`Format`/`Pos`/`Pred`, `TStringList`/`TList`/`TStream`/`TFileStream`, `FormatDateTime`, `Math` | `crates/xedit-session/src/script_rtl/` |
| the ten VCL units and `xejviScriptAdapterMiscVcl.pas` | every form, dialog, control, graphics and ini class; the headless parts are the `Windows` records, the hint/cursor/accel helpers, the shortcut codec, `TIniFile`/`TMemIniFile`, `TDirectory`/`TFile` | `crates/xedit-session/src/script_vcl/` |

The tests are executable examples: `crates/xedit-session/tests/script.rs` (the run loop, `the_jvcl_rtl_units_run_a_script`, corpus runs), `script_adapters.rs`, `script_misc.rs`, `script_df.rs` (unit tests) and `script_vcl.rs`.

## The dialect

The language is JvInterpreter Pascal — the subset the JvInterpreter interpreter accepts, not full Delphi — and compatibility with the 4.1.5q oracle is the goal; upstream quirks are reproduced, not corrected. What matters when writing a script:

- **The unit shape.** A script is `unit <Name>;` (the unit name matches the file name), with optional `Initialize`, `Process(e: IInterface): Integer` and `Finalize`. A missing routine, or one whose body reads as `begin end` / `begin Result := 0; end` (upstream's `FunctionIsEmpty` source-text scan, six accepted spellings), is skipped. A non-zero `Result` from any of the three stops the run (`Script terminated itself, Result=<n>`) and skips the rest; it is not an abort.
- **Compile is shallow.** `script check` mirrors `Compile`: routine bodies skip to their `end`, so most statement mistakes appear at run time as `Exception in unit <u> line <n>: [<class>] <message>` followed by `Aborted: ...`. `uses` a unit beside the script (`uses Bookmark`, as `Bookmark1.pas` does) and everything in `<scripts folder>\<Unit>.pas` is compiled into the run.
- **The script variables.** `Initialize` may write the filter fields (`FilterByName`, `FilterName`, `FilterConflictAllSet`, `FilterConflictThisSet`, `FilterBySignature`, `FilterSignatures`, ... — the 36 fields of the GUI's filter dialog), then call `ApplyFilter`; `ScriptProcessElements`, `ScriptProcessSignatures`, `wbOutputPath` and `dfFloatDecimalDigits` are the other settable globals. An identifier the host does not know is the interpreter's undeclared identifier, at run time.
- **Registered RTL, not the full language.** `IntToStr`, `Format`, `Pos`, `TStringList`, `TFileStream` and the rest resolve because the JVCL units are registered; a Delphi unit outside the registered set has no members. Indexed properties dispatch through the registered `AddIGet`/`AddISet` pair (`sl[i]`, `sl.Strings[i]`, `el[i]`, `Json.S['x']`), and the default property forms work where the surface registers one.
- **Integer and variant semantics follow the interpreter**: 64-bit `for`/`case` bounds behave as `whatsnew.md` records, `VarType('x')` answers `$102` (`varUString`, the oracle's value), an integer element of a set literal contributes its value (upstream's `SetExpression1` quirk, so build shift-state checks through `ShortCut`), and variant conversions are the interpreter's, with the exceptions in "Known divergences".
- **`Mode:` in the header comment.** `Mode: Silent` is the one keyword upstream reads: it suppresses the run's own log lines, so the closing line alone remains. The CLI `script run` saves changed plugins at the end unless `--no-save`/`--dry-run`, as the GUI's close does.

## Headless and the Vcl seam

Every VCL registration is in (a form script compiles and runs to its first VCL call), and the call then refuses: headless the `Vcl` seam is `script_rtl::NoVcl`, which raises

```
[EVclUnavailable] <class>.<member> is not available without a GUI (script "<name>")
```

(unit functions refuse with the function name in place of `<class>.<member>`). That is the policy: the shim names exactly what a script asked for, and the GUI phase implements the trait without touching the registrations. A form script aborts at `TForm.Create` (or `ShowModal`), a dialog script at `MessageDlg`/`InputQuery`/`InputBox`/`SelectDirectory`, control scripts at the first member (`TComboBox.Create`), and `MessageBox` routes through the same seam. `cargo xtask parity script` classes those corpus scripts `form`/`interactive`; 32 of the 150 build a form and 11 ask for input (110 are headless), per `crates/xtask/oracle/scripts/corpus.json`.

What answers headless, beside everything without a window: the `Windows` value types (`TPoint`/`TRect`, `Point`/`Rect`/`Bounds`, the `TopLeft`/`BottomRight` accessors, the `MB_*`/`VK_*` constants), the `Controls` hint and cursor helpers (`GetShortHint`, `GetLongHint`, `CursorToString`/`StringToCursor`/`CursorToIdent`/`IdentToCursor`), `IsAccel`, `KeysToShiftState`/`KeyDataToShiftState`, the `Menus` shortcut codec (`ShortCut`, `ShortCutToKey`, `ShortCutToText`, `TextToShortCut`, with the key-name table captured from the oracle), the `IniFiles` classes over an in-memory model (`TIniFile` writes through, `TMemIniFile.UpdateFile` writes xEdit's section-per-block text), and `IOUtils`' `TDirectory`/`TFile` (`GetDirectories`/`GetFiles`, `ReadAllText`/`WriteAllText`).

## Output conventions

- **Messages.** `AddMessage(text)` appends to xEdit's message log, which is the run's observable output: without `--json` the log lines go to stdout and an abort also writes `error: <location>: <message>` to stderr; with `--json` they are `messages` in the response. Use it for progress and results; a report file goes to `wbOutputPath`.
- **The run log line shapes**, as the oracle's log has them (the `[mm:ss]` prefix is the progress callback's; the text after it is what the harness compares):
  - `[mm:ss] Start: Applying script "<name>"` — dropped for `Mode: Silent`.
  - the script's `AddMessage` lines, each `[mm:ss]`-prefixed.
  - `[mm:ss] Done: Applying script "<name>", Processed Records: <n>, Elapsed Time: <mm:ss>`.
  - `You can close this application now.` — no prefix (it is a plain message log line, `DoRunScript`'s last).
  - `[mm:ss] Saving: <name>` per changed plugin, after the closing line.
  - a failure ends the log with `[mm:ss] Exception in unit <u> line <n>: [<class>] <message>` and `[mm:ss] Aborted: Applying script "<name>"`, with no `Done:` and no closing line; the run exits non-zero, and plugins the script changed before the failure are still saved at the end unless `--dry-run`/`--no-save` (a one-shot run stands in for the GUI's close, which saves them too — the harness's aborted save case records the same on the oracle).
- **Files.** A script writes its own files; xEdit's convention is `wbOutputPath` (a trailing separator included), which the port starts as the data folder when nothing set it (`-O:` on the legacy command line sets it) and a script may set itself (`wbOutputPath := 'C:\somewhere\'`). During a run `wbScriptsPath` is the script's own folder and `wbTempPath` the temp folder. xEdit's `cargo xtask parity script` harness captures a run's files by where they were written (`out/` for `wbOutputPath`, `script/` for the run folder, `program/` beside the executable, `data/` for the private data folder), which is a good model for where a script should put things.
- **The filter.** `ApplyFilter` runs when the call that asked for it returns (the run state applies the recorded variables then), so the filter's own log lines appear after the messages of the call that triggered it; `FilterApplied` answers true from the moment it is called, as upstream.

## Known divergences

Behaviour a script can meet where the port does not reproduce the oracle exactly. Each is recorded in `docs/PLAN.md` under the phase-6 steps; say so rather than guessing when a script trips one.

- **`'x' + 5` and `'x' + True` convert instead of failing.** The oracle raises `Type mismatch`; the port's `+` produces `x5`/`xTrue` (`crates/xedit-script/src/values.rs`). No corpus script does it; do not rely on either.
- **Regex subset.** `TPerlRegEx` runs the Rust `regex` crate: no lookaround, no in-pattern backreferences, no possessive quantifiers; `\d`/`\w`/`\s` are Unicode where PCRE was ASCII; `preNotBOL`/`preNotEOL`/`preNoAutoCapture` have no equivalent; offsets count characters where upstream counts UTF-8 bytes.
- **JSON dates and streams.** The date-time `UseUtcTime` conversion does not apply the system's time-zone bias, `D[]`'s string parsing is an ISO 8601 subset, and `jdtUtcDateTime` folds into `jdtDateTime`. `TJsonBaseObject.LoadFromStream`/`SaveToStream` raise (the JVCL stream classes exist; the JSON wiring is owed) — use `LoadFromFile`/`SaveToFile`.
- **The abort line's message.** An exception raised by a host function still carries the port's `External error in unit '<unit>' on line <n> : <message>` wrapper where the oracle prints the bare message (recorded and owed to the abort-path comparison); a raised data-format error prints `[EJvInterpreterError]` as its class where the GUI prints `[Exception]`.
- **Elapsed time and `Saving:` names.** The elapsed origin differs (the oracle counts from the load's first long action, the port from the run's start; the harness strips times before comparing), and the `Saving:` line names the port's loaded path where the oracle logs the queued `<name>.save.<timestamp>` name. A run whose `Process` is missing still prints the `Processed Records` count.
- **Record methods are over-accepted through the type name.** `TFile.ReadAllText(...)` resolves in the port; the oracle needs a variable of the record type.
- **Resources.** `LoadFromResource` reads through the container handler, which a load fills only where a plugin's string tables need the archives, so a resource inside the game archives is not always found (load a localized plugin such as `Skyrim.esm` to have them listed).
- **No-ops and refusals where the GUI exists.** `Free` of a data-format root releases the document (a later call fails with `The file has been freed`; upstream is a use-after-free), `Free` of a non-root does nothing; `O2V(nil)` is `Unassigned`; the view machinery is inert (`MoveUp`/`MoveDown`, `AssignTemplate*`/`TemplateAssign`, the view element states `esHidden`/`esSortKeyValid`/..., `ContainerStates`, `InsertElement`, `SetEditValue` of a main record writes nothing, `FileWriteToStream`); a `LoadFromFile`/`SaveToFile` OS error carries `Cannot open file "<path>". <os error>` where the GUI raises `RaiseLastOSError`'s text; `SetLength` of an interpreter array raises `Array to array assignment` (declare a fixed-size array and assign it instead — no corpus script calls `SetLength`).
- **Minor host behaviours.** `Beep` is silent, `ShowException` writes nothing, `DiskFree`/`DiskSize` answer -1, `GetLocaleStr` answers the en-US values, `CreateProcessWait` does not set the window's show state, `SetPrecisionMode`/`SetRoundMode` record without touching the FPU.
- **Front-end limits.** A routine body terminated by the unit's `.` (`function P: integer; begin end.`) is refused (`';' expected but '.' found`), so two of `FunctionIsEmpty`'s six spellings are unreachable in practice; unit files are read as bytes (no encoding detection); `xEditAPI.pas` is stubbed at run time even when the folder holds it, as upstream; `-scripthost:` is not read (only `JvI` exists).

## Corpus scripts to copy from

All in the 4.1.5q `Edit Scripts` folder (`XEDIT_SCRIPTS`; `xedit script list` shows the folder this build uses).

| Script | Shows |
|---|---|
| `_newscript_.pas` | the unit template: `Initialize`/`Process`/`Finalize`, `AddMessage('Processing: ' + FullPath(e))`, the non-zero `Result` comment. |
| `List loaded plugins and their masters.pas` | `Initialize`-only work over `FileCount`/`FileByIndex`/`MasterCount`/`MasterByIndex`/`Name`, self-terminating with `Result := 1`. Runs headless and is asserted by the port's tests. |
| `Apply filter for cleaning.pas` | setting the filter fields and calling `ApplyFilter` from `Initialize`; the port asserts its run. |
| `Check for errors.pas` | recursion over the element tree (`Check`, `ElementCount`, `ElementByIndex`), `StringOfChar` for indenting. |
| `Bookmark1.pas` | `Mode: Silent`, a `uses Bookmark` unit beside the script, `Finalize` doing the work; one of the two `equal` run cases of the phase-6 harness (it writes `Bookmark.ini` byte-identically to the oracle). |
| `Skyrim - Add shadow bias to lights.pas` | walking records, `SetElementEditValues`, saving changed plugins; the port asserts the saved `XLIG` bytes. |
| `JSON - Demo.pas` | the `JsonDataObjects` surface (parse, `S`/`I`/`F`/`B`/`A`/`O`/`V`, `ToJSON`). |
| `Find records.pas` | a form script: it compiles and runs headless up to `TForm.Create`, then refuses through the Vcl seam. |
