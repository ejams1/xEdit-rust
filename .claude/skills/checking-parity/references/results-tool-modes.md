# Tool modes



`parity tool-modes` runs each mode of `xeInit.pas` the release has for a game (`-setesm`, `-clearesm`, `-masterupdate`, `-masterrestore`, `-onamupdate`, `-sortandclean`, `-generateseq:<plugin>`, `-checkforitm`, `-checkfordr`; `-CheckForErrors` is `parity check`, `-dump` is `parity dump` and the quick clean modes are `parity clean`) on one plugin per game: the largest vanilla plugin that has masters and is at most 16 MiB, the game master excluded (`--file` names the plugins and `--mode` the modes; `--file` skips the size cap).

The GUI run gets `-autoload` (its module selection answers itself; without it a run waits there on the hidden desktop), the executable of Fallout 3 and New Vegas in its private data folder (the release's plugin list loader opens `<Data>\<game>.exe`; without it the load ends with `Fatal: Error loading plugin list`), and for `-clearesm` an ESM-flagged `.esp` of the plugin, which the port builds first (`files.flags --esm true` and `files.save` in one batch).

A run ends at the exit of the GUI or at `--= All Done =--` in its message log, whichever comes first, because the release can wait after `SaveChanged` for something a hidden desktop never delivers; the saves it queued for renaming then sit next to the plugins as `<name>.save.<timestamp>`, and that is where the check reads them.

Every plugin the mode saved is compared byte for byte, with the lines of the message log the mode writes (`Setting ESM Flag:`, `Removing ESM Flag:`, `Updating ONAM in:`, `Skipping:`, `Created:` and `Skipped: ... doesn't need sequence file` for `-generateseq`) and the exit code of the check modes (the count, at most 127); every plugin the mode did *not* save must be untouched on both sides.

The modes the release does not offer for a game are skipped and named (Morrowind is view only; `-masterupdate` and `-masterrestore` are Fallout 3 and New Vegas; `-onamupdate` is the Skyrim family; Starfield has no `-setesm` or `-clearesm`).

The export ('`export`') runs `xDump.exe -<GAME> -export RAW` in a folder of its own and compares both of its outputs with `xedit tool run export --format RAW`: the profile file next to the program and the structure text on stdout (the port's definitions are built as xDump builds them, `wbSimpleRecords` off, which `Session::load` reads through `tool_modes::loads_like_xdump`).

A profile that differs in one definition's type name is reported as `known-difference`, which does not fail the check (see `docs/PLAN.md`, owed from step 9).

The oracle's runs are cached as `<cache>/<tag>/<MODE>-oracle-tool-modes/<plugin>.<mode>.<key>.oracle.{json,log.zst,exit}` — the manifest names the plugins the mode saved and keeps their bytes (zstd) — and a run of every game takes about an hour on the corpus; `--jobs` and the memory budget apply as everywhere else.

The report is `target/parity/tool-modes.json`.
