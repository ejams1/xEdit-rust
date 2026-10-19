# Scripts



```
cargo xtask parity script [--script <name part>]... [--skip <name part>]... [--sample <n>] [--game <game>]... [--phase parse|run|probe]... [--oracle-only]
                          [--refresh-oracle] [--keep] [--list] [--jobs <n>] [--memory-budget <GiB>] [--max-memory <GiB>] [--oracle-timeout <minutes>]
```

Two phases over the 150 `.pas` of the oracle's `Edit Scripts` folder (`XEDIT_SCRIPTS`, default `<XEDIT_ORACLE_DIR>/Edit Scripts`), each cached under `<cache>/<tag>/<MODE>-oracle-script/<stem>.<key>/`: `result.json` (the class, the dialogs, the manifest of the files the run left), `log.txt` and the captured files zstd-compressed below `files/` (the saved plugins below `files/saved/`).

The key hashes the recipe, the script text, the units it `uses`, the plugins with their masters and the executable's name (`--refresh-oracle` runs the oracle again regardless).

The report is `target/parity/script.json`; `--sample <n>` limits each phase to its first `n` cases, `--script <name part>` selects by name (also how a deferred interactive case is run), `--skip <name part>` leaves the named scripts out (the slow ones), `--phase` picks one phase and `--list` prints the corpus with its classes.

A case whose plugins are not in its game's data folder (a machine with the variable set to an emptied install) is `skipped` and named, as a mode skips a game whose variable is unset.

The check exits with an error on `different`, `oracle-failed` and `port-failed`; the deferrals (`oracle-interactive`, `form`, `port-pending`) are printed and counted.

**parse**: every `.pas`.

The oracle compiles each script in the GUI's script mode (`-script:`, hidden, one script per run) and the run is classed by what its log shows: `done` (the mode posted `You can close this application now.`), `aborted` (the log holds `Aborted: Applying script`: `ApplyScript` caught a runtime raise and aborted, which skips that line), `interactive` (a dialog ended the run, but the `Applying script` line proves the compile: `CreateScript` compiles before `PerformLongAction` posts the line), `compile-error` (the application's own exception dialog, which is what a JvInterpreter compile error shows: a task dialog whose caption carries the GUI's version and whose message is `Error in unit 'X' on line N : ...`, or an access violation when the compiler itself faults) or `failed`.

A dialog of the script's own — a window class of its own (the `TForm` it built, the `TInputQueryForm` of an `InputQuery`, the common `Open`/`Save As` dialogs) or a default caption (`Information`, `Confirm`) — proves that the compile finished: `Compile` runs a script's main block and `ApplyScript` posts `Applying script` only after it, so `Bookmark.pas` shows its bookmark list before that line exists.

A dialog the run does not see leaves the GUI idle instead: the 120 s idle watch ends the run and the error names a `Mode:` keyword as the likely reason (a `Mode: Silent` script's progress lines, the `Aborted:` one among them, are suppressed by `wbProgressLock`).

The runs load an empty game master the harness writes (a valid `TES4` record with no records, the ESM flag in its record flags): the module selection forces the game master of the mode into every load, and compiling a script reads no record, so the parse phase needs no game data (the scripts still execute, over nothing).

The port answers `xedit script check <file>` (exit 0 = parsed, else the `file:line: message` lines), with `XEDIT_SCRIPTS` set to the scripts folder so the units resolve there. `equal` when the oracle compiled the script (`done`, `aborted` or `interactive`) and the port parsed it, `equal-error` when both refuse it, `port-pending` until the port CLI has `script check`.

**run**: the form-free corpus of `crates/xtask/oracle/scripts/runs.json` (the 109 headless scripts and the 8 that ask for input; the 32 that build a form have no case, the port's form shim covering them instead).

A case names the game and the plugins to load (defaults in the file), whether the oracle builds the reference information, and with `close_after` the plugins whose saved bytes are compared (`SaveChanged` on close writes them into the private data folder). `answers` (issue #26) lists the dialogs the script shows, in order: the oracle GUI is answered with them (class, title substring, keys, button, text, `control` and `repeat` -- the `gui::Answer` matching) and the port's headless VCL model consumes the same JSON (`script.run`'s `answers`), so one case file drives both sides; a dialog the answers do not cover still ends the oracle run as `oracle-interactive` and stops the port with `pending_interaction`.

A `variant` names a second case of the same script (the `norefs` case of `Fallout4 - Filter for precombined statics.pas`, whose `MessageDlg` shows only without the reference build), appended to its stem.

A probe case names a script of the repository (`probes/<name>.pas`, read from `crates/xtask/oracle/scripts/probes`) and with `master` a synthetic plugin of the repository (`tests/fixtures/parity/*.esm`) that both sides load under the game master's name, so the case needs no game data and runs in seconds; `data_files` (parity #38) copies extra files into the private data folder of both sides (`<repository path>:<path under Data>`, for a probe that reads resources through the container handler, such as the `atlas` probe's fixture textures), with `refcache` (which needs `build_refs`) each side first loads the plugins with a no-op script and the reference build, which writes the reference cache (the oracle's `-C:` folder, the port's `--cache-path`; a file above `wbCacheRecordsThreshold`, 500 records), and then runs the case with those files in place, so the records' references come from the cache (the case's stem gains `-refcache`).

The parity #23 probes: `edid-quick-init` (the quick-init collapse of a record read by its name first, the AVIF `CNAM` AfterLoad at the save, `Copy`, `StrictDelimiter`) and `refs-edit` with and without the cache (the referenced-by lists and the EditorID index around edits), all `equal` on 2026-10-15; the unit tests `the_edid_quick_init_probe_gives_the_oracle_lines` and `the_refs_edit_probe_gives_the_oracle_lines` of `crates/xedit-session/tests/script.rs` pin the oracle's lines.

A script the corpus classes interactive (step 1's `corpus.json` when it is there, else the `TForm|ShowModal|InputQuery|InputBox` greps) is deferred as `oracle-interactive` unless named with `--script`, and so is a headless-classed script that turns out to show a dialog at runtime: a dialog is a class, not a difference.

The oracle's run: the hidden GUI with a private data folder, the script and the units it `uses` copied into the run folder (upstream sets `wbScriptsPath` to the folder of the script before running it, and `GetUnitSource` reads `<wbScriptsPath>\<Unit>.pas` for every unit not compiled into the host), `-O:` to the run folder's `out`, `-IKnowIllBreakMyGameWithThis` (with the `-IKnowWhatImDoing` every run gets: a script may edit the game master of the private copy, without which `TwbFile.GetIsEditable` refuses every record of it and each mutating script aborts on its first edit) and the case's plugins.

What is compared with the port's `xedit script run`: the port runs from its own run folder, which holds the script and its units (so `ScriptsPath()` is that folder, and the working directory with it), the plugins as private copies below its `Data` (the folder the port's load takes its data path from, so string tables, `DataPath` messages and the DDS scan see a private folder exactly as the oracle's do) and an empty `bin` that stands in for the oracle's program folder; its invocation is `xedit --json --edit --game <MODE> --allow-edit-game-master --program-path <run>\bin\ --load <run>\Data\\<plugin>... script run <run>\<name> --scripts <run> [--build-refs]`, where `--allow-edit-game-master` is the oracle's `-IKnowIllBreakMyGameWithThis` and `--build-refs` stands in for the oracle's load-time `wbBuildRefs` on the cases whose scripts read `ReferencedBy`.

The port saves the plugins it changed at its end, as the close does.

Compared: every file the script wrote, mapped by where it was written — `out/` (`wbOutputPath`), `script/` (the run folder), `program/` (the `bin` folder) and `data/` (the private data folder less the plugins that went in) — byte for byte; the message log lines of the script section (`Start: Applying script` .. the closing line, the `[mm:ss] ` prefixes and the `, Elapsed Time:` of the `Done:` line removed; for a `Mode:` script, whose whole run is suppressed, the closing line alone), with each side's run folder mapped to `<root>` and the digits of a `.save.<timestamp>` suffix to `<time>` (the two runs' absolute paths and clocks differ by construction); and the bytes of the case's `save` plugins.

The GUI's own files (its message log, the exception log every caught exception appends to, the executable, the reference cache) are not a script's output and are not captured.

Selection semantics, recorded because nothing is explicitly selected headlessly: `DoRunScript` runs `SelectRootNodes(vstNav)`, which selects every root node of the navigation tree (every loaded file), and `ApplyScript` walks each selected node's subtree from its last descendant (`vstNav.GetLast`, then `GetPrevious`, the selected node itself last), so `Process` runs for every element whose `ElementType` is in `ScriptProcessElements` (default `[etMainRecord]`) and, for a main record, whose signature is in `ScriptProcessSignatures` when the script sets it: a case that loads plugins runs a `Process`-driven script over every main record of every loaded file, the hardcoded file included.

**probe**: the language probes of `crates/xtask/oracle/scripts/probes/` (every `.pas` there whose name does not start with `Unit`: 880 small scripts, one per interpreter behaviour -- a variant conversion, an array bound, a `Format` argument, the expression stack, a routine terminator, a unit's encoding, an exception's class and line).

Both sides run each probe over the probe master the harness writes (the parse phase's empty game master plus two `GMST` records, so a `Process` runs), and the comparison is the run phase's log comparison; nothing is written.

A probe the oracle refuses to compile compares on its `line : message` against `script check`; a probe the oracle ends on a dialog of its own (`oracle-dialog`), or never finishes (cached as `failed` with the reason, so the next run does not wait for it again), is a class and not a difference; `probes.json` accepts an outcome with a reason as `known-difference` -- a native fault the port cannot reproduce (`SetLength` of an `array of string`, `High(5)`, `'abc'[4]`), a hang, a clock value, or a divergence `docs/PLAN.md` documents (the UTF-8 string model, the slot kinds).

Results (2026-10-16, `fix-sse-filters` on `main` 65366d0, 892 probes): **891 of 892** -- 852 `equal`, 32 `known-difference`, 7 `equal-error`, 1 `oracle-dialog` (`numerics-parse.pas`: the oracle never finished, cached as failed, a class), exit 0; at `main` itself `numerics.pas` (parity #39) was `different` on `FormatFloat('0.#####', 1E22)` (`1E22` against every digit), fixed in that branch.

Earlier (2026-10-15, parity #28, on the tree rebased onto #22/#23/#30/#32/#40): 880 of 880 equal -- 841 `equal`, 32 `known-difference`, 7 `equal-error`, exit 0; `t-30.pas` (the hardcoded file's walk order) flipped from accepted to equal with #23's walk work, and a probe is compared on its log lines alone (parity #30's file-writing `streams.pas` added the case where the oracle captures no files in this phase).

At fix-numerics (2026-10-16, issue #67): **907 of 908** -- 864 `equal`, 36 `known-difference`, 7 `equal-error`, 1 `oracle-dialog` (`sys-showexception.pas`), 0 `different`.

At fix-numerics-2 (2026-10-16, issue #69, on `main` `3961851`): **911 of 912** -- 868 `equal`, 36 `known-difference`, 7 `equal-error`, 1 `oracle-dialog` (`sys-showexception.pas`), 0 `different`.

A `#0` in the message log (fix-numerics-2): the memo is SynEdit's, whose `WM_GETTEXT` stops at the first NUL, so the harness reads the lines with `EM_GETLINE` when the copy is short (SynEdit gives each line up to a NUL of its own) and compares every line up to its first NUL on both sides.

The oracle loses the lines that reach the memo in the same `tmrMessagesTimer` flush as a NUL line before them, and every line after them (an upstream bug the port does not reproduce): a probe that logs a NUL pauses after that line so that the GUI flushes it alone (`log-nul.pas`), or spells control characters as `#<code>` (`numerics-parse.pas`).

Results (2026-10-10). **Parse**, all 150 `.pas` in 4m50s at `--jobs 4`, with step 2's `xedit script check`: **150 of 150 equal** -- 149 `equal` + 1 `known-difference`, exit 0.

The oracle compiled 148 (87 `done`, 51 `interactive`, 10 `aborted`) and refused 2; the port parsed all 150, so the one `known-difference` is `xEditAPI.pas`, which `parse.json` accepts with its reason (JvInterpreter 4.1.5q faults compiling the declaration-only API file: an access violation in `InterpretConst`/`Expression`, `JvInterpreter.pas:7426`).

What the classification cost, all found by running it: `Bookmark.pas` is not a compile error -- it compiled and showed its bookmark list from its main block, which `Compile` runs *before* `ApplyScript` posts `Applying script`, and its `MessageDlg` (`Information`) is not the application's exception dialog (whose caption carries the GUI's `4.1.5q`); three scripts (`LOD Statistics`, `Oblivion - Export all scripts in xml format`, `NIF - List blocks references`) open a Windows `Open`/`Save as` dialog, also class `#32770`, whose *paths* once carried `4.1.5q` inside the dialog text until the title was read on its own; a syntax error inside a routine body does not fail the compile at all (probe: `Result := ;` in `Initialize` raised at *run* time; `Compile` skips bodies), only one outside one does (`Error in unit 'BrokenChar' on line 6 : ';' expected but 'end' found.`), and an undeclared *type* in a `var` section is accepted.

**Run**, 107 of the 117 cases (12m03s at `--jobs 3`; the seven slowest scripts skipped with `--skip`, and 2 Fallout New Vegas cases skipped for an emptied data folder): **0 `equal`, 78 `different`, 29 `oracle-interactive`** -- the port's runtime is steps 5 to 8, so almost every script that runs on the oracle stops in the port on a name the host has not registered yet.

The causes, by count: `Undeclared Identifier 'Signature'` (19 cases), `'TStringList'` (18), `'IsMaster'` (4), `'TList'` (3), `'TwbFastStringList'`, `'RecordByFormID'`, `'GetIsEditable'`, `'SetElementEditValues'`, `'Pred'`, `'SetJDOLineBreak'`; `SetESL.pas` fails the port's *body parse* at the same `except on ... do` handler step 5 found (`'end' expected but 'Result' found`, line 35) where the oracle compiles and runs it (its first line: `xEdit must be started with the -PseudoESL argument in order to use this script.`); `Apply filter for cleaning`, `Apply custom scripted filter` and `Apply filter for deleted navmeshes` log `Start: Applying Filter` on the oracle and `Script terminated itself, Result=1` on the port; a `Mode:` script's lines are suppressed by the oracle (`wbProgressLock`) and printed by the port (`Bookmark1`, `Bookmark1Go`); and the abort text differs in shape (`Exception in unit X line N: [Exception] <message>` vs `[EJvInterpreterError] Error in unit 'X' on line N : <message>`). `_newscript_.pas` is also the live evidence of step 5's `FullPath` finding: the oracle logs 540,969 lines like `Processing:  \ [00] Skyrim.esm \ [111] GRUP Top "REVB" \ [9] IntRoomWoodMediumReverb [REVB:000C5B6A]` -- the position segments are upstream's -- while the port stops at `Undeclared Identifier 'FullPath'`.

No case reaches step 5's single-precision print yet: the scripts that print such a value stop earlier on an unregistered name.

Of the 29 `oracle-interactive`, the runtime found 21 the greps did not (`Bookmark.pas`, `Conflict Status`, `Export dialogues`, `LOD Statistics`, `NIF - List blocks references`, the Worldspace confirmations, ...); each is deferred, with the dialog named.

Oracle walk counts for the port's host to match: 869,712 `Processed Records` for Skyrim LE's `Skyrim.esm` and `TESV.exe`, 869,717 for the same with `Skyrim - Find uncompressed records` (its `Process` fires for every record, so its case cannot settle what the walk hands an *overridden* record: that needs a two-plugin load), 962,397 for Skyrim SE's `Skyrim.esm` with its hardcoded file, 1,549,451 for `Fallout4.esm` with its own.

Parity batch #27 (2026-10-15, PR #54) ported the scripted filter: `Apply custom scripted filter.pas` is **equal** at its tip (the filter calls the script's `Filter` function through the run's interpreter; `[Skyrim.esm] Filtered 868,496 of 869,687 records` and `[Pass 1] 920209 / [Pass 2] 1312 / 1193` are the oracle's), and its two fixtures (`probes/filter-scripted.pas`, `probes/filter-scripted-error.pas`, `equal`) pin the acceptance set, the visitation order, an edit during the callback, a mid-walk flag clear and the abort shape.

Parity batch #36 (2026-10-16, PR #58) closed the step-6 owed items of this mode: `cargo xtask parity modgroups` is **15 of 15 scenarios equal** — the 9 above and `sse-create` (the view's Create ModGroup takes the loaded override chain of a record the script shows with `JumpTo`, without its defining master; the CRC32 question, the typed name, the store-file module selection by Enter and the reload are answered), `sse-edit-duplicate` (a name two files hold, one of them the file of an *unloaded* official module: the ordering picks it, and the rename writes that file), `sse-edit-items` (the Delete key on the focused node and its question), `sse-presets`, `sse-preset-save` and `sse-preset-delete` (the preset name typed into the combo box of the selection dialog, Load/Save/Delete and the questions; the settings file bytes are compared).

The harness learned for them: `unloaded` plugins linked into the run folder without being loaded, `jump_to`, `text` in the control of a class and `repeat` for a window answered step by step; and a VCL button whose handler opens a dialog must be clicked with `PostMessage` (a waiting `BM_CLICK` runs the handler's modal question and times out, which reads as a missing button).

Parity batch #25 (2026-10-17, PR #72) bound the rest of the script host and changed the port's side of the harness twice: the port gets `--settings <run>\plugins.<app>viewsettings`, the oracle's `xeSettingsFileName` (`-P:<work>\plugins.txt` with the extension changed), so `wbSettingsFileName` and `wbSettings` compare; and a case without `close_after` runs the port with `--no-save`, because the oracle's GUI is never closed there and its `SaveChanged` never runs (an aborted run's save lines had been compared against nothing).

Its probes are `host-*.pas` (the run cases `host-paint`, `host-dialogs`, `host-uvrange` and `host-lod` over `tests/fixtures/parity/host-25.esm`, made by a generator kept out of the tree: 40 `GMST`s, statics with distant and tree LOD flags, an interior cell and a worldspace with references; the `lodsettings` file of its worldspace is the fixture's own 16 bytes).

A case that copies or names a record whose subrecords all lie before `QuickInitLimit` must read it through `ElementByPath` first (#64: the release loses the subrecords of a record named first).

A recorded answer for the `TfrmFileSelect` form names `control: TCheckListBox`; its list is multi-select, so the first Down of the keys lands on the second entry.

Full probe phase at the batch's tip: 927 cases, 881 `equal`, 38 `known-difference`, 7 `equal-error`, 1 `oracle-dialog`, 0 `different`.

Later runs (2026-10-10/11).

The last complete full comparison is the re-run at `a1c2f22` with the canonical cache (`M:\projects\xedit-parity-cache`; do not unset `XEDIT_PARITY_CACHE` — the xtask default cache holds different oracle instances of the same keys): parse 149 `equal` + 1 `known-difference` = 150/150; run **64 `equal`, 20 `different`, 31 `oracle-interactive`, 3 `oracle-failed`** of 117 (report `target/parity/script.json`, log `M:\projects\xedit-parity-scratch-p6-step10\rerun-a1c2f22.log`).

The intervening rounds: `p6-rp-fix` (walk over-delivery, short names, abort-line shape, `RemoveNode`), `p6-rp2` (filter `Start:`/`Done:` lines, exe `[00]` name, Delphi OS-error texts, the private run-folder recipe with `--program-path`/`--allow-edit-game-master`/`--build-refs`, the GUI save shapes, `--program-path` mapping of run folders to `<root>` and save clocks to `<time>`, the `equal-log-loss` class), `p6-rev-fix` (recursion depth guard, the `etSubRecord` walk, refused-edit errors, `Mode: Silent`, `Processed Records` only when the walk fired) and `p6-rp3` (the missing-folder message's collapsed path, raw union-member element indices, the `TStrings.Text :=` setter through `Add`, the unused-data load warnings plus the per-run `build_refs`).

The re-classification at the merged tip was stopped by the system (low memory) and is owed; the `equal-log-loss` rule (first/last/Done lines equal, port lines a superset) covers the oracle memo's lost lines where its guards hold.
