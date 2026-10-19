# Oracle edit sequences



`parity oracle-edit` runs the scripted edit sequences of `crates/xtask/oracle/edits/*.json` (`--file <stem>` selects some, `--game` the games) on both sides and compares every saved plugin, reported as `<sequence>/<plugin>` in `target/parity/oracle-edit.json`.

A sequence names the game, the plugins to load (masters first, in the order the GUI loads them: the official order, or the file dates for Oblivion to New Vegas), the commands as `xedit batch` takes them, the plugins to save, and `build_refs` (whether the oracle builds the reference information, which a FormID change needs; see above for what that costs).

The port runs the batch with a `files.save` per plugin; the oracle runs a Pascal script that `oracle_save.rs` generates from the same commands around `crates/xtask/oracle/edit.pas`: a record is the one the named file sees (`RecordByFormID` with the file's FormID), `elements.set` is `SetElementEditValues` (or the native and default setters) with the element required afterwards, `records.copy` adds the required masters and copies as the GUI's copy commands do (`wbCopyElementToFile` with the record's contents, or the child group with `deep`), `formids.change` collects `ReferencedBy` before `SetLoadOrderFormID` and updates the editable referencing records, and `formids.renumber` (with `start`, in the file itself) follows `mniNavRenumberFormIDsFromClick`.

A command or parameter the translation does not know fails the sequence instead of running on one side only.

The oracle's saves are cached in `<cache>/<tag>/<MODE>-oracle-edit`, keyed by the sequence, the generated script and the plugins.

Since the copy batch: a sequence may give `answers`, the dialogs of the GUI's handlers in the order they come (a copy with overwriting shows the module selection, whose keys go to the module list with `control`, and the overwrite and deleted-record questions; "Create New File" the template selection and its `InputQuery`), and `allow_make_partial` runs the GUI with `-AllowMakePartial` and the port with `--allow-make-partial`. `records.copy` with `overwrite` takes the GUI's own path (the record shown with `JumpTo`, then the "with overwriting" menu item), so the sequence must show the record with a `records.compare` step of the same record and `from` file first; `elements.assign` and `elements.move` are `ElementAssign`/`AssignTemplateByName`/`TemplateAssign` and `MoveUp`/`MoveDown`/`InsertElement`/`RemoveElement`; `records.compare` itself is `JumpTo` (the view's alignment of sorted and aligned arrays, which an assign at a position needs); `files.new` with a template clicks `mniCreateNewFile`, and without one runs `AddNewFileName`.

Since parity #33: a sequence with `build_refs` may name `refs`, records (`form_id` as the `file` sees it) whose referenced-by lists are compared after the steps, reported as `<sequence>/refs`: the oracle writes `ReferencedBy` of each record's master from the reference information it built at load and kept through the edits, the port builds its index before the steps (`refs.build`, for every sequence with `build_refs`, as the GUI builds it at load: the reference build of a copied record initialises its elements, the Starfield perk activity's virtual tree among them, so a step can find a path only then), keeps it through them and reads the lists with `refs.get`; the lines are `R <FormID>@<file> <count>` and `|<FormID>@<file>` per entry.

A staged sequence without `strings` gives neither side the tables of a renamed plugin (the GUI gets no tables then, so the port's staging folder leaves them out too).

Deliberate divergence (UPSTREAM-BUG, user decision 2026-10-16): after a record copy upstream loses the first edit of a perk activity's virtual tree (the tree is rebuilt from the old `ATAV` text when Delphi releases the copied record's elements); the port keeps the edit, and the sequences set every tree value twice so both sides end in the same state (`sf1-perk-activity-edit`, `sf1-perk-activity-masters`, the probe `probes/perk-activity.pas`).

Issue #73: `sse-copy-npc-vmad` copies Nazeem [NPC_:00013BBF], whose VMAD properties are stored out of name order, into Update.esm without reading him first; `TwbArrayDef.GetSorted` answers False while `wbCopyIsRunning`, so the arrays the copy builds keep the stored order and their `#n` names, and the port resets the records a copy built when the outermost copy ends (as Delphi releases them), so a later read sees them sorted as the release does (the run case `probes/copy-vmad-order.pas`).

A perk activity tree that a masters update builds again (a copy as new collapses the record) does not write its `ATAV` text while it is being built (`perk-activity-masters.pas`); the release does (UPSTREAM-BUG, not reproduced: its save of `sf1-light-masters` had every rank's `Data` at the defaults), so that sequence shows the copied PERK before the masters update and both sides keep the text.

A cached oracle run whose script stopped is run again rather than replayed: a stop can come from a harness recipe the key does not hash.

#### Step 6 results (2026-10-07)

`oracle-save` over the 249 plugins: 239 `equal`, 7 `equal-error`, 3 `oracle-unsupported`, 0 `different` (the first run, before the port fixes listed in `docs/PLAN.md`, had 12 `different`). `oracle-edit`: `fnv-caravanpack`, `fnv-gmst` and `fo4-dlcworkshop01` equal; `fo4-dlcworkshop01-masters` different (31 records whose unchanged sorted arrays the oracle leaves unsorted after a master update and the port sorts; equal since phase 4 steps 5 and 7).

#### String tables in edit sequences (phase 4 step 7)

A sequence with `"strings": true` runs on a staging folder of its own (`<scratch>/<tag>/<MODE>-oracle-edit/<sequence>/data`): the plugins it loads with their masters (hard links, or copies across volumes) and, in `Strings`, the tables of every localized one, taken from the game's loose `Data\Strings` or the first archive of the data folder that has them (the interface and localization archives first).

The GUI gets the tables in its private data folder, and the harness closes its main form after the script (`GuiExtras::close_after`): `FormClose` runs `SaveChanged` without its dialog in the script mode and writes the modified tables, which its message log names (`Saving: Strings\<name>.save.<time>`, renamed over the old file).

The 4.1.5q GUI then does not exit on the harness's desktop (its message loop raises exceptions), so the run ends once the log file `bin\<name>_log.txt` is there.

The port loads from the staging folder and `files.save` writes the tables next to the saved plugin.

Each table either side saved is an outcome `<sequence>/Strings/<table>`; a table only one side saved is `different`. `"rename": {"New.esm": "Game.esm"}` loads a copy of a game plugin (and its tables) under another name, so an official Starfield module can be edited; `"inputs_from": "<sequence>"` takes the plugins that sequence saves from its cached oracle saves (run it first: the sequences run in name order).

The steps `localization.localize` and `localization.delocalize` run the GUI's own handler, `mniNavLocalizationSwitchClick` ("Localize plugin" / "Delocalize plugin"), through `SwitchLocalization` of `strings.rs` (since parity #33): the script `JumpTo`s the file header (which focuses its node and ends the `BeginUpdate` the script mode holds the navigation tree in), sends the tree a Left key (`Perform(WM_KEYDOWN, VK_LEFT)`, which focuses the parent, the file node) and clicks `mniNavLocalizationSwitch`; the sequence answers the "Localize plugin" dialog (`{"class": "TfrmLocalizePlugin", "title": "Localize plugin", "button": "Localize"}`), and `-IKnowWhatImDoing` skips `EditWarn`.

The handler closes the main form at its end, so `FormClose` saves the plugin and the tables and releases the file list: a localization step must be the only step of a sequence with `strings` and no `refs`, the script writes no plugin of its own, and the harness takes the plugin the GUI saved (`Saving: <plugin>.save.<time>` in the message log, `strings::saved_plugins`).

The helper checks the localized flag afterwards, so a handler that returned early (the focused node not the file) fails the step.

The earlier emulation through `SetEditValue` could not do what the handler does (it sets each text with `wbLocalizationHandler.NoTranslate` while the file is still localized, so `wbDialogueTextAfterSet` does not trim): it trimmed one `sf1-strings-delocalize` response text the handler keeps.

Results 2026-10-08: 30 of 31 saved files over the 14 sequences; the 10 string sequences 26 of 27 (17 of 17 tables), the one difference the trim above. Parity #33 (2026-10-16, the localization steps through the GUI's handler): all 34 sequences 63 of 63, the 20 sequences with string tables 47 of 47 (27 tables).

Since fix-strings (issue #75, 2026-10-17): a sequence may name an `upstream_bug` (`reason`, `expect`), for a sequence in which the release loses data the port does not lose (rule 14 of the parity brief).

The port's saves are then compared with the oracle's saves of the sequence `expect` names (the same edits shaped so that the release keeps its data; it must load the same game, save the same plugins and have no `upstream_bug` itself, and it runs with the sequence, before it, also under `--file`).

A file equal there and different from the release's own save is `known-difference`, and so is a file only the release saved (the empty tables it writes over a plugin's); if a file differs from the expected save, or no file differs from the release's (the port reproduces the loss again), every file of the sequence is `different`. `known-difference` counts as passing in the exit status.

The step `elements.get` (`GetEditValue` of the element on the oracle side) reads a value; a read of a localized string loads the plugin's tables on both sides, which is how the `expect` sequences of the string-table bug load them.

The cases: `fo4-strings-archive-unloaded` (expect `fo4-strings-archive-loaded`, a name of DLCworkshop01 read first) and `sse-copy-npc-vmad-shown` (Nazeem shown, then copied into Update.esm; expect `sse-copy-npc-vmad-shown-loaded`, a name of Update.esm read before the copy). `sse-copy-npc-vmad` reads a name of Update.esm before its copy too: before that it counted the release's loss of Update.esm's tables as equal, because the port reproduced it.

The release's `JumpTo` never loads Update.esm's tables in `sse-copy-npc-vmad-shown` because the view paints only the rows on screen; the port's `records.compare` gives the text of every row, and the Tint Layers row (`wbTintLayerToStr` reads the race's winning override, RedguardRace of Update.esm) loads them.

With the port's `AddValue` loading the tables in any case, which tables a comparison loaded no longer changes a save.

Results at the fix-strings tip: `cargo xtask parity oracle-edit`, all 38 sequences, 65 `equal` and 8 `known-difference` of 73 saved files, exit 0; the 24 sequences with string tables 49 `equal` and 8 `known-difference` of 57 files (33 tables).

`"archives": ["Fallout4 - Interface.ba2", "DLCworkshop01*"]` (names, or the start of names with `*`) links those archives of the game's data folder next to the staged plugins on both sides (`GuiExtras::links` for the GUI) and puts no table loose: the tables are then read from the archives, as the loader finds them (issue #32). `"switches": ["-skipbsa"]` adds switches to the oracle's command line and their flags to the port's (`--skip-bsa`, `--force-bsa`); `inputs_from` also carries the tables the other sequence's oracle saved, loose in `Strings` (a reload).

The port gets a My Games folder of its own with an empty game ini (`--my-games`, `--game-ini`), as the GUI gets `-M:` and `-I:`.

Results 2026-10-15 (issue #32): `fo4-strings-archive-edit`, `-reload`, `-unloaded` and `-dlcrobot` 13 of 13 saved plugins and tables equal; `-unloaded` is the oracle's own replacement of unloaded tables (a new description first: a `.DLSTRINGS` of 1 string and empty `.STRINGS`/`.ILSTRINGS`).
