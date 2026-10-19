## Headless and the Vcl seam

Since issue #26 the `Vcl` seam's headless implementation is the VCL object model (`crates/xedit-session/src/script_vcl/interactive.rs`): a script creates real form and control objects (`TForm.Create(nil)`, `TButton.Create(frm)`, `Button.Parent := frm`), reads and writes their properties (`Caption`, `Width`, `Visible`, `ModalResult`, `ItemIndex`, `Checked`, `Items`/`Lines` as the one `TStrings` value; a published property is answered whichever class the JVCL tables registered it on, as upstream's `GetValueRTTI`/`SetValueRTTI` read a control's RTTI before the tables -- `btn.ModalResult` works although the tables carry `ModalResult` for `TCustomForm` alone, and a name that is no published property stays the undeclared identifier), stores event assignments (`Button.OnClick := Handler` -- *stored*, not fired: a real message loop fires them in the GUI phase), and shows a form modal (`frm.ShowModal`) whose answer closes it through the clicked button's `ModalResult`, the VCL's own click rule.

A dialog the script shows is answered from **recorded answers** instead of a window: pass them to `script run` with `--answer '<JSON>'` (repeat, in dialog order) or `--answers <file>` (a JSON array).

An answer is `{"class", "title", "keys": [...], "button", "text", "control", "repeat"}` and matches a dialog by its window class and a caption substring -- the same rule the parity harness uses to answer the oracle GUI: `text` goes into the first edit control, `keys` post to the focused control (the arrow keys move a list cursor -- a fresh list selects nothing, so the first Down lands on its first entry -- space toggles a check, return/escape click the default/cancel button) and `button` is clicked by its caption. `control` names the class of the child the text and the keys go to instead (`TCheckListBox`, `TCheckBox`, `TLabeledEdit`), and `repeat` marks an answer that goes to the same form as the one before it: a form with several controls is driven control by control, one answer per control, the last one clicking the button (`Find records.pas` in `runs.json` is the worked example).

A file dialog's `text` is the file name; without a drive or a root it is taken under the dialog's `InitialDir`, as the common dialog resolves it. `MessageDlg` returns the `mr*` of the answered button, `InputQuery`/`InputBox` the answered text, `TOpenDialog`/`TSaveDialog.Execute` the file.

A dialog with no remaining answer stops the run; the response's `pending_interaction` names the question (`kind`, `class`, `title`, `text`, `buttons`), and `xedit script resume` with the answer appended continues it (the interpreter has no continuation, so the script runs from its start; a deterministic script reaches the same state and proceeds past the interaction). `cargo xtask parity script` drives the same cases on the oracle GUI and on the port (`crates/xtask/oracle/scripts/runs.json`, the `answers` field).

What still refuses: the four `JvInterpreterFm` form-runner functions (issue #81 owns them; no script of the corpus calls them) (`JvInterpreterMakeForm`, `JvInterpreterRunForm`, `JvInterpreterRunFormModal`, `JvInterpreterRunUnit` -- they compile a second unit through a fresh interpreter) and `SelectDirectory` (its own refusal from the misc adapter), with the `[EVclUnavailable]` text below for the former; a VCL unit function the model does not know refuses the same way.

Presentational members keep their state and draw nothing headless (the ledger marks the `TCanvas` and paint members `presentation`).

The corpus classes 32 of the 150 scripts as `form` and 11 as `interactive` (110 headless), per `crates/xtask/oracle/scripts/corpus.json`; the scripts backed by recorded answers run on both sides (the runs are recorded with pull request #26).

What answers headless, beside everything without a window: the `Windows` value types (`TPoint`/`TRect`, `Point`/`Rect`/`Bounds`, the `TopLeft`/`BottomRight` accessors, the `MB_*`/`VK_*` constants), the `Controls` hint and cursor helpers (`GetShortHint`, `GetLongHint`, `CursorToString`/`StringToCursor`/`CursorToIdent`/`IdentToCursor`), `IsAccel`, `KeysToShiftState`/`KeyDataToShiftState`, the `Menus` shortcut codec (`ShortCut`, `ShortCutToKey`, `ShortCutToText`, `TextToShortCut`, with the key-name table captured from the oracle), the `IniFiles` classes over an in-memory model (`TIniFile` writes through, `TMemIniFile.UpdateFile` writes xEdit's section-per-block text), and `IOUtils`' `TDirectory`/`TFile` (`GetDirectories`/`GetFiles`, `ReadAllText`/`WriteAllText`), and the `Classes` unit's binary streams (`TBinaryReader`, `TBinaryWriter`, `TBytesStream` over the same `TStream` objects as `TMemoryStream`/`TFileStream`, parity #30: `xejviScriptAdapterMiscVcl.pas` registrations bound with the `Classes` unit's stream model, constructors included, and witnessed by the `streams.pas` probe; no corpus script uses them).

### The walk and the filter (parity #41)

- The `Process` walk steps a lazily built copy of the navigation tree, as the release does (issues #82 and #84): `vstNavInitChildren` freezes a node's child count the first time the walk needs it, `vstNavInitNode` binds a child's element when the walk reaches it, and a `RemoveNode` takes its node with it.

A children group a `Process` creates under a node the walk already enumerated is therefore never visited (`Change persistent flag.pas` over `walk-churn`, `probes/persistent-walk.pas`), and the walk keeps its constant cost per step through a per-container position index.

The residue the release's `RemoveNode` search leaves (`Worldspace displacement.pas`) is accepted as a `known-difference`.
- After `ApplyFilter` the walk follows the filtered tree's nodes with the elements they were bound to: a topic's responses keep the order the filter's walk bound them in, also when a check of that walk sorted the topic's group (the `INOM`/`INOA` fill of an editing session; `probes/filter-topic-bind.pas`).
- A second `Free` of an object raises `EInvalidPointer` (`Invalid pointer operation`), as the release's memory manager does for a block already free (`Report masters.pas` frees its lists in `Finalize`, which its `Process` calls before the run does).
- The release's quick init of a record named first (its `Name`, `EditorID` or a name that includes it) empties the record at its next init (#64); the port keeps the record. A case that names a record before reading its elements compares only when it reads the record through an element first (`walk-move.pas`, `tes4-topic-full.pas`), or is accepted as `known-difference` (`Oblivion - Export Dialogues.pas`).

### The main form's state (parity #25)

The host functions that read the GUI's main form get a model of it (`HostState` in `script_host.rs`), each part checked against the oracle:

- **The selection.** Headless `DoRunScript` selects every root node, so `wbSelectedFiles(list)` and `wbSelectedFilesToFileNames(sl)` give the loaded files (the hardcoded file as the executable's name, `SkyrimSE.exe`); `JumpTo(record, backward)` selects the record's node alone, which they read afterwards. The `Process` walk took its selection before `Initialize`, so a jump does not change what `Process` sees.
- **The nodes' conflict status.** `ConflictAllForNode`/`ConflictThisForNode` read what the tree's node holds: 0 (`caUnknown`) until something classifies it.

The filter classifies the nodes it keeps (with `InheritConflictByParent` the files and groups take their children's highest), `RemoveFilter` drops it, and `JumpTo` classifies the row it focuses and the records the expansion opens above it (the `CELL` of a reference) -- the GUI paints those; whether its window shows and paints further rows depends on its size, which the port does not model, so read the jumped record, not its neighbours.

An element without a node (a subrecord) leaves the result `Unassigned`.

The filter runs when the call that asked for it returns, so read the nodes in the next call into the script (`Process` after an `ApplyFilter` in `Initialize`).
- **The record conflict cache.** `ConflictAllForMainRecord`/`ConflictThisForMainRecord` classify the record's override chain and cache it, as `TwbMainRecord.ConflictAll` does; an edit anywhere in the chain resets it. `IsPositionChanged` reads that cache (a record cached as the master or identical to it is never "changed").
- **New files.** `AddNewFileName(name[, light[, medium]])` makes the file after the last of `Files` (the hardcoded file included) without masters and refuses a name the data folder holds with the "A file of that name exists already." message (answer it, class `#32770`); `AddNewFile([light[, medium]])` asks the name in an `InputQuery` (`TInputQueryForm`, "New Module File"), drops a plugin extension, adds `.esp`/`.esl` and the game master. `AddRequiredElementMasters(element, file, asNew[, silent])` asks the `Confirm` `MessageDlg` unless silent.

A file a script made is saved under its own name when the run saves (no `.save.<time>` and no queued rename: it is not on disk yet).
- **The forms.** `frmMain` is one `TfrmMain` object of the VCL model (shows nothing headless; a script owns its forms by it); `frmFileSelect` is a new `TfrmFileSelect` each call with `CheckListBox1`, `edSearch`, `cbBackup`, `btnOK` (`mrOk`), `Label1` and `PopupMenu1`, found with `FindComponent`.

Its check list is multi-select, so the caret starts on the first entry: an answer's first Down lands on the second one.

Answer it with `{"class": "TfrmFileSelect", "title": <the caption>, "control": "TCheckListBox", "keys": [...], "button": "OK"}`.

A Space on that list before any Down checks the first entry (parity #41: `Copy as override.pas` checks `<new file>` so in the release).
- `ExecuteCaptureConsoleOutput(cmd)` runs the line hidden, logs each read of its output (trimmed) and returns the exit code; a program that does not start raises `EOSError`.
- **`external` functions.** `function F(a: Integer): Integer; external 'kernel32.dll' name 'F';` loads the library and calls the export, with `Integer`/`Boolean` (as `Integer`, `True` is -1), `SmallInt`/`Word`, `string` and `var` `Integer`/`SmallInt` arguments and `Integer`, `Boolean`, `SmallInt`/`Word` or no result. A declaration by `index` never finds its function (upstream looks up the empty name). The functions the `Windows` unit registers (`MessageBox`) stay the VCL model's dialogs.
