# LOD generation



```
cargo xtask parity lodgen [--case <name or name part>]... [--game <game>]... [--oracle-only] [--refresh-oracle] [--keep] [--timeout <minutes>] [--list]
```

Each case (`CASES` in `crates/xtask/src/parity/lodgen.rs`, `--list` prints them) is a game, its plugins, the worldspaces to check in the LODGen form, the options of `[<APP> LOD Options]`, loose files added to the data folder (synthetic billboards and LOD models the vanilla games lack) and whether it splits the trees atlases instead of generating.

The oracle is the GUI build in `-<MODE> -lodgen` mode, started hidden on a private data folder (hard links of the plugins and archives, on the drive of the game's data) with its own `bin` (the settings ini), `Edit Scripts` copy (`LODGenx64.exe`, `Texconvx64.exe`, the atlas maps), output, temp and My Games folders; the harness answers the module selection, checks the case's worldspaces in the form (toggling the `TCheckListBox` items) and presses `Generate`, or holds Shift for the GUI thread (`AttachThreadInput` and `SetKeyboardState`, no desktop input) so that the hidden `Split LOD Atlas` button shows and presses it.

It reads the message log from the main form and stops at `LOD Generator: finished` (or the last split message of each worldspace, counted one per worldspace; the text of a memo can end in a run of NUL characters right after its last line, which the count trims like `log_lines` does).

The port is `xedit lodgen` with the same folders and options.

Compared: every file below the output folder byte for byte, the export files of the scripts folder (`LODGen.txt`, `LODGenAtlasMap.txt`, with the run folder replaced) and the log (loader lines and the output of `LODGenx64.exe` skipped, times masked).

The tree rotations are `2 * Pi * Random` from a clock seed: the harness recovers the oracle's seed from its block files and runs the port with `--seed`; the GUI sometimes draws a number elsewhere in the middle of a run, so a rotation that comes later in the same `Random` sequence is accepted (with a note) when the order of the draws is the port's.

Oracle outputs are cached per case and input key under the cache; `--refresh-oracle` runs it again.

Work goes to `XEDIT_PARITY_SCRATCH/<tag>/lodgen/<game>` (or the same path on the game's drive); an equal case's work is removed unless `--keep`.
