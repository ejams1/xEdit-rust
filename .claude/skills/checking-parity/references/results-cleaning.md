# Cleaning



`parity clean` runs the GUI's own quick auto clean mode (`-quickautoclean -autoexit -autoload <plugin>`) on every corpus plugin that has masters and compares the saved bytes, per pass the filter's counts, the UDR and ITM counts with the record lines (`Undeleting:`, `Skipping:`, `Removing:`, `Can't remove:`), and the dirty report the mode ends its log with (`LOOT Masterlist Entries` with the CRC32 of each pass). Report `target/parity/clean.json`.

`parity clean-scenarios` (parity #35) runs the scenarios of `crates/xtask/oracle/cleaning/*.json` (`--file <stem>`, `--game`): the menu items of the main form over a selection of plugins, which the quick clean mode cannot reach.

The GUI runs `crates/xtask/oracle/cleaning.pas` in its script mode: the filter for cleaning (`ApplyFilter` with the variables of `Apply filter for cleaning.pas`), the file nodes collapsed and selected with the mouse messages of a click and Ctrl+clicks (`frmMain.FindComponent('vstNav').Perform(WM_LBUTTONDOWN, MK_LBUTTON or MK_CONTROL, ...)` at the row of each file, `DefaultNodeHeight` and `Indent` read through RTTI), then `frmMain.FindComponent('mniNavUndeleteAndDisableReferences').Click`, `mniNavRemoveIdenticalToMaster` and `mniNavLOManagersDirtyInfo`, and the plugins written with `FileWriteToStream`.

A scenario names `load`, `select`, `udr`/`itm`, `options` (the `files.clean` request fields: the UDR options, which the oracle reads from the `[Options]` section of the settings file the harness writes next to the plugin list, and `allow_make_partial`, which adds `-AllowMakePartial`; `-IKnowWhatImDoing` is always passed so `EditWarn` asks nothing), `ini` (further `[Options]` lines, `FillPNAM` mapped to the port's `--fill-pnam`), `save`, and `prepare` (a plugin the port builds first over the masters with a batch of commands, which both sides then load: a case the vanilla plugins lack, such as a deleted reference to a moveable static).

The port runs `files.clean` with the same selection and options, `files.dirty_info` and `files.save`.

Compared: the closing lines of the steps without the elapsed time and the warnings, the record lines, the dirty report and the saved bytes.

Cache `<cache>/<tag>/<MODE>-oracle-clean-scenarios/`, report `target/parity/clean-scenarios.json`.
