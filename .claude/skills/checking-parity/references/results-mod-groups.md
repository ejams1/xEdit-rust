# Mod groups



`parity modgroups` runs the scenarios of `crates/xtask/oracle/modgroups/*.json` (`--file <stem>` selects some, `--game` the games).

A scenario names the game, the plugins to load, the mod group files to put down (`<plugin>.modgroups` in the data folder, `<global>` for the program's own file; a list of lines is written with CRLF after each, a string as it is, and `{{CRC:<plugin>}}` becomes the plugin's CRC32), the settings file of the GUI (`Plugins.<app>viewsettings` next to the plugin list, which holds `[ModGroups] Selection` and the `frmModGroupSelect_ActiveModGroups_Presets` section of the selection dialog's presets; give it `[Init] FirstStart=1`, or the GUI's first start writes it), `unloaded` plugins of the game's data folder that are put next to the loaded ones without being loaded (the modules of a `.modgroups` file whose plugin is not in the plugin list), `jump_to`, a load order FormID the script shows with `JumpTo` before its actions (the view's "Create ModGroup" makes a group of the shown record's loaded override chain, without its defining master), the menu items of the main form to click, the dialogs that follow with their answers, and the port's commands for the same action.

An answer may put `text` into the control of the class `control` (`TComboBox` of the preset name of a selection dialog, `TVirtualStringTree` for the module selections' lists, the first edit control of the dialog when no class is given), post `keys` to it and click `button`; `repeat` answers the same window again, which drives a dialog control by control (type a preset name, press Load, then press OK) with one answer per step.

Answers are matched before the harness's own handling of a `TfrmModuleSelect` (its OK button), so a scenario answers the module selections of the create flow (Enter, 13, with nothing checked picks the first visible module and confirms) and its own dialogs.

The GUI's script mode never activates mod groups (`tmScript` is one of `wbAutoModes`, so the startup activation is skipped), so `oracle/modgroups.pas` clicks the menu items (`frmMain.FindComponent('mniViewModGroupsReload').Click`; `mniNavEditModGroup`, `mniNavDeleteModGroups` and `mniNavUpdateCRCModGroups` end with the same reload) and then writes the conflict status of every record as `conflicts.pas` does.

The harness (`gui.rs`, `GuiExtras`) writes the files into the run folder and answers each dialog in order: the next visible window of the answer's class whose title contains the answer's title gets the key presses (posted to its focused control, so the forms' `KeyPreview` handlers see them: 13 is Enter, which in a selection with nothing checked picks the first entry and confirms; 106 is the numpad `*`, which inverts the selection, so with none checked selects every entry) and then the click on the button. `MessageDlg` and `ShowMessage` are system dialogs, class `#32770` (title `Confirm` for a question), with buttons `&Yes`, `&No`, `OK`.

A dialog without an answer fails the run after 10 seconds, and an answer whose dialog never came fails it at the end.

The port loads the same plugins from a folder of links with the same files (`--modgroups-file`, `--settings` pointing into it), runs the commands as a batch and `conflicts.list` with `saved_mod_groups`, and the check compares the statuses, every mod group file and the settings file byte for byte, and the lines the reload writes to the main form's log (`ShowValidationMessages`) with the `validation_messages` of the port's responses.

The oracle's outputs are cached in `<cache>/<tag>/<MODE>-oracle-modgroups/<scenario>.<key>/`, keyed by the plugins, the script and the scenario.

A run takes about a minute per Skyrim SE scenario.

The navigation tree itself cannot be selected from a script, so the create scenario shows a record whose override chain the view's "Create ModGroup" takes (`jump_to`).

#### Phase 4 step 6 results (2026-10-08)

9 of 9 scenarios equal (8 on the Skyrim SE masters, 1 on Fallout 4 with four DLCs): every record's status, 29 files and 34 validation lines. What the scenarios taught: Delphi's `Split` keeps an empty part after the last separator, `TMemIniFile` drops a section whose name it had before with its lines, and the 4.1.5q main form's memo returns its text in the ANSI code page to `WM_GETTEXT`.
