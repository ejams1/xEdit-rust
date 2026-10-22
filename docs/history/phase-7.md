# Phase 7: GUI

- **Model:** Opus 5.5 for the main window and the virtualised tree and grid, Sonnet 5.5 for the dialogs.
- **Port:** Main window (navigation tree, view page with conflict colours, referenced-by, messages, information), all dialogs, themes, bookmarks, settings files. Every action calls a session command.
- **CLI:** No new commands of the phase's own beyond the six the command layer needed (`modules.list`, `signatures.list`, `settings.get`/`settings.set`, `script.read`/`script.save`).
- **Skills:** None new.
- **Gate:** Ledger complete. Manual walkthrough of the upstream menus against the Delphi build.

The phase ran on the branch `phase-7-gui` from `main` `6d9646d`, in nine steps: 1 (crate and main window), 1b (command-layer gaps), 2 (view and navigation), 3 (the flow shell), 4 (the tree and grid bindings), 5 (selection dialogs), 6 (viewers and editors), 7 (settings and modal tools), 8 (menu and binding audit) and 9 (the gate). The step notes live in `docs/PLAN.md`; this file records the gate itself.

## The ledger is closed

`cargo xtask check` at the phase tip:

```
gui: 663 entries, 0 pending
switch: 156 entries, 2 pending
game_mode: 14 entries, 4 pending
tool_mode: 17 entries, 0 pending
script: 3663 entries, 0 pending
units: 172 tracked, 27 pending
audit: 240 rows, 185 reachable, 7 reimplemented, 48 unreachable
```

Every one of the 663 `gui.*` bindings is `covered` (240, each naming the command it calls) or `presentation` (423). The eight bindings that were `excluded` in the step-8 audit became `presentation` in `601cc5b`: the six `mniMainPluggyLink*` items and `mniNavOtherCodeSiteLogging` and `mniNavTest` are drawn by the port, and the upstream action behind them is not a file operation (the Pluggy Link items and the code-site logging open other applications; the "Test" item prints a message). A presentation binding is a claim that the port draws the same control and does not pretend to do the upstream's side effect, not a claim of parity for it.

The other ledgers are the phase's own scope: the two pending `switch` and four pending `game_mode` entries are the game's own switches, and the 27 pending `units` are the upstream units the phase does not port.

## The menu walkthrough against the Delphi build

Upstream 4.1.5q has no `TMainMenu`: the File menu is the popup `pmuMain` on the `bnMainMenu` speed button, and the rest of the command surface is the tree and view context menus. The released 4.1.5q source is the upstream clone at its own 4.1.5q commit (`fd1e360`), so `xEdit/xeMainForm.dfm` is the Delphi build's own menu definition, item by item, with the caption, the nesting, `Enabled`, `Checked` and `RadioItem` of every entry.

The walkthrough is a tree diff of that definition against the port's `crates/xedit-gui/src/menu_model.rs`: `tools/menu_diff.py` parses the ten popup menus out of the `.dfm` (the depth of an item is its indent, the radio property is `RadioItem = True`) and out of the Rust model (the caption's `&` is the accelerator and is ignored on both sides), then compares the two caption trees and the state flags of every node. The run at the phase tip:

```
python tools/menu_diff.py --dfm <upstream>/xEdit/xeMainForm.dfm

== pmuMain: 6 top-level, 14 in all (dfm == port)
== pmuNav: 70 top-level, 84 in all (dfm == port)
== pmuView: 28 top-level, 49 in all (dfm == port)
== pmuSpreadsheet: 3 top-level, 3 in all (dfm == port)
== pmuViewHeader: 13 top-level, 13 in all (dfm == port)
== pmuNavHeaderPopup: 2 top-level, 7 in all (dfm == port)
== pmuRefBy: 15 top-level, 15 in all (dfm == port)
== pmuNavAdd: 0 top-level, 0 in all (dfm == port)
== pmuMessages: 4 top-level, 4 in all (dfm == port)
== pmuBtnMenu: 1 top-level, 1 in all (dfm == port)

problems: 0
```

Zero differences: no caption differs, no item is missing or extra, no nesting differs, and no `Enabled`, `Checked`, `RadioItem` or `Visible` flag differs. `pmuNavAdd` is empty in the Delphi build, and it is empty in the port. The differ is in the repository, so the walkthrough is reproducible from the branch and from the upstream clone.

## The runtime screen-checks against the 4.1.5q binary

The oracle is the released `xTESEdit.exe` (4.1.5q), run on the hidden private desktop of the session (`M:\projects\scratch\p7-shot-evidence\gate-session.json`), never on the user's screen. The popups are the `#32768` windows the Delphi build opens, read with `cargo xtask gui menupopup` and captured with `gui shot --handle` of the popup window itself.

Captured and compared with the port's model:

| Menu | Runtime capture | What the 4.1.5q build shows |
| --- | --- | --- |
| `pmuMain` | `oracle-main-menu.png` | the hamburger popup of the loaded form |
| `pmuNav` | `oracle-nav.png`, `oracle-navmenu.png` | the navigation tree's file menu (`Compare to...`, `Create delta patch using...`, ...) |
| `pmuNavHeaderPopup` | `oracle-pmuNavHeaderPopup.png`, `.txt` | the tree's column header menu (21 items with the ids, the separators included) |
| `pmuView` | `oracle-pmuView.png`, `.txt` | `&Clipboard` (id 107), a separator (117), `&Hide no conflict and empty rows` (118), `&Stick to` (119), `C&olumn Widths` (122), `&ModGroups` (127), `O&nly Master and Leafs` (132) |
| `pmuSpreadsheet` | `oracle-pmuSpreadsheet.png` | only `&Rebuild` (138) with one record selected; `Compare Selected` is hidden below two selected records |
| `pmuMessages` | `oracle-pmuMessages.png` | `Clear`, `Save selected text`, a separator, `Autoscroll to the last message` |
| `pmuBtnMenu` | `oracle-btnmenu2.png` | `Shrink Buttons` |

Three menus are not openable in this build's default state, and the record says so rather than claiming a capture that does not exist:

- `pmuViewHeader` is the header of the view tree (`Header.PopupMenu = pmuViewHeader` in the `.dfm`). The 4.1.5q runtime opens no header control for the view tree: right-clicking every row of its top edge (client y 104..126) opens `pmuView` instead, and the `THeaderControl` of the view tree is not among the window's children. The `Ctrl+M` route of `xeMainForm.pas:19286` is a follow-up on a header right-click, so it needs the same missing control.
- `pmuRefBy` is the popup of `lvReferencedBy`, the list of the `Referenced By` page. The page is `TabVisible = False` in the `.dfm` and the runtime keeps it hidden until the loader has reference data (`xeMainForm.pas:16800`: `tbsReferencedBy.TabVisible := wbLoaderDone and (lvReferencedBy.Tag > 0)`); a load of `Skyrim.esm` and `Update.esm` leaves the tag at zero, so the page and its popup cannot be reached.
- `pmuNavAdd` holds no item in the Delphi build, so there is nothing to open at runtime; the port's menu is empty for the same reason.

For those three the `.dfm` diff above is the evidence, and it is the same definition the runtime is built from.

## The dialogs

Nineteen PNGs of the port's own dialogs, each written by the app itself on the private desktop (`port-dialog-*.png` in `M:\projects\scratch\p7-shot-evidence`), covering eighteen dialogs: the filter options, the mod group select, the mod group edit, the file select, the log analyzer, the script editor, the view elements, the rich edit, the localization, the localize plugin, the developer message, the tip, the legend, the options, the LOD generator, the worldspace cell details, the edit warning and the FormID prompt. The mod group edit has two captures: the dialog with the group list and the dialog with the empty case (`port-dialog-modgroup-edit-nogrp.png`), because this machine has no mod groups to edit. The module select and the game select are captured under their own names (`sse-module-select.png`, `game-select.png`).

The screen-check findings that the gate turned into tooling are in `.claude/skills/checking-parity/references/screen-checks.md`.

## The parity rerun at the tip

The full harness was started at the phase tip on 2026-10-21 at 20:14 (`tip 601cc5b`; the branch tip at the end of the day is `e572f6a`, which adds only `docs/phase-8-performance-review.md` and the merge of `main` — the crates the harness exercises are byte-identical between the two, so the run is the run for the tip). The chain is `M:\projects\scratch\p7-shot-evidence\run-parity-gate.sh`, the logs are in `M:\projects\scratch\p7-shot-evidence\parity-logs`, the reports in `target/parity`.

Three steps finished:

| Step | Exit | Result | Time |
| --- | --- | --- | --- |
| `dump` | 0 | 246 of 249 equal: 229 `equal`, 17 `equal-prefix`, 3 `oracle-unsupported` (tes3 Morrowind, Bloodmoon, Tribunal) | 54m |
| `check` | 0 | 246 of 249 plugins equal: 232 `equal`, 14 `equal-log-loss`, 3 `oracle-unsupported` | 35m |
| `clean` | 1 | 235 of 238 plugins equal: 235 `equal`, 2 `oracle-unsupported` (tes3), 1 `oracle-log-lost` (fo76 `NW.esm`) | 28m |

The `check` step checked 5,852,286 records, 143,951 of them with errors, 261,102 error lines. The 14 `equal-log-loss` cases are plugins whose oracle log lost lines while keeping every line the port wrote, in order; the `clean` step's one `oracle-log-lost` is the oracle's own log loss on fo76 `NW.esm`. The three `oracle-unsupported` cases are the same in every step: the released 4.1.5q build does not support tes3. The `clean` step exits 1 on those three cases, which is the same exit it gives on the earlier tip.

The `saves` step did not finish. The corpus is this machine's live Fallout 4 save folder, and it has grown since the morning run: the oracle crashes on three of the new autosaves with an `EAccessViolation` and the `xDump` process then does not exit, so the step waits on a process that will never finish. The harness has a remedy for exactly this (`--oracle-timeout`, which keeps the oracle's partial output and compares it as a prefix), and the step was restarted with `--oracle-timeout 5`; the run was then stopped on instruction rather than left running.

The rest of the chain (`roundtrip`, `oracle-save`, `oracle-edit`, `conflicts`, `refs`, `modgroups`, `merged`, `filter`, `tool-modes`, `clean-scenarios`, `resources`, `discovery`, `lodgen`, `bsarch`) was therefore not rerun at this tip. The morning run of the same chain at `36fbb3d` has their results (`M:\projects\scratch\p7-shot-evidence\parity-logs\chain-prev-1021.log`): `roundtrip` 0, `oracle-save` 246 of 249 (239 `equal`, 7 `equal-error` on sf1, 3 `oracle-unsupported`), `oracle-edit` 68 of 76 (8 `known-difference`, an upstream bug the port does not reproduce), `conflicts` 10 of 11, `refs` 8 of 10, `modgroups` 16 of 16, `merged` 12 of 12, `filter` 4 of 4, `tool-modes` 66 of 74, `clean-scenarios` 8 of 8, `resources` 8 of 8, `discovery` 7 of 7, `lodgen` 0, `bsarch` 1472 of 1472.

**The rerun continues separately.** The steps above are the gate's own evidence for the work of this phase, which is GUI work: the harness's three finished steps cover the dump, check and clean routes, and the port's change to the tree is confined to `crates/xedit-gui` and `crates/xtask/src/gui`, which the harness does not exercise. The `saves` step and the rest of the chain are continued on their own, on the branch they are run on, with the `saves` step carrying `--oracle-timeout` so the upstream hang cannot stall the chain again.

## What this gate does not claim

- The runtime captures are of the menus the 4.1.5q build opens in the states this machine reaches. The state-dependent visibility of an item (upstream sets `Visible` in the popup handlers, `pmuViewHeaderPopup` for instance) is not reproduced here for every state; the `.dfm` diff compares the definitions, not the runtime visibility of every item in every state.
- The port's own popups are egui surfaces inside one frame, not Win32 menus, and egui reads raw window events: the helper's posted mouse messages cannot open them, so the port's menus are not captured as pictures. What is checked on the port's side is the model that draws them (the `.dfm` diff above) and the audit of step 8, which names the control of every covered binding and is checked by `cargo xtask check`.
- The dialog captures are the port's own pixels. They show that the dialog is drawn and holds the expected content, not that every dialog's every control matches the Delphi build pixel for pixel. The oracle's dialogs were not captured one by one: the port's dialogs are ported from the 4.1.5q forms' own DFM, and the module selection is captured on both sides (`oracle-moduleselect.png` and `sse-module-select.png`).
- The parity numbers of the phase are the harness rerun above, not the menu work. That rerun covers the `dump`, `check` and `clean` steps at this tip; the `saves` step and the rest of the chain are continued separately.
