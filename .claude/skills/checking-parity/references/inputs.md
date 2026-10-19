## Inputs

- **Corpus:** vanilla master files from the local game installs, found through `XEDIT_FO4_DATA` and `XEDIT_SSE_DATA` (each is the game's `Data` directory). Never commit game files. Small synthetic plugins live in `tests/fixtures` and are committed.
- **Oracle output:** produced by the release binaries (xDump, xEdit tool modes, BSArch, Sniff) and cached outside the repository, keyed by release tag, game and input file hash.

### Archives

`cargo xtask parity bsarch [--game <game>]... [--archive <name>]... [--max-size <MB>] [--jobs <n>] [--cross] [--synthetic] [--only <text>] [--textures] [--keep]` runs `BSArch.exe` (in `XEDIT_ORACLE_DIR`) and the port's `bsarch` (built in release) on the archives of the game folders and compares: `list` (the `-dump` text, byte for byte), `unpack` (the files written; an archive the oracle cannot unpack must stop the port with the same message), and `pack` (the folder the oracle unpacked, packed by both into the format of the source with each compression it has, or into every format with `--cross`; every archive written is compared byte for byte, with the oracle at `-mt:no`, the port at all threads and at `-mt:no`, and the text of the run too).

Archives up to `--max-size` MB (40) are taken unless some are named with `--archive`. `--synthetic` adds generated folders for what the game archives lack (two sources merged, filters, flags, `-split`, empty files, names that sort several ways, shared data), and the `edge` and `edge-wide` folders of issue #32 (TES4 hash collisions in one folder, BA2 names with one CRC-32, names that sort differently by ordinal and by the locale, names outside ASCII in and out of the ANSI code page), each packed in the six formats and the oracle's archive then listed and unpacked by both (with `-mt:no`, so an unpack that fails stops on the same file).

The texture archives (`DX10`) go through the same checks (`unpack` writes the DDS files; `pack` packs them with `-fo4dds` or `-sf1dds`); `--textures` takes only those, `--synthetic` adds generated DDS folders (formats, mipmap counts, a cube map, a 24 bit file, identical textures, data that ends early, and files both sides must refuse), and `--only <text>` keeps the synthetic packs whose label contains the text.

The work goes to `XEDIT_PARITY_SCRATCH/<tag>/bsarch` and is removed for an equal case (`--keep` keeps it); a different case keeps its folders and prints the first difference.

The report is `target/parity/bsarch.json`. `BSArch.exe` was seen to hang at its start (no CPU time for a quarter of an hour); a run over 30 minutes is stopped and run once more.

The oracle packs on one thread and compresses at level 12, so a few hundred MB take minutes: the default selection of the 11 games takes about half an hour with `--jobs 4`.

### BSArchPro packing (XBox target)

`cargo xtask parity bsarchpro [--case <name or name part>]... [--keep] [--refresh-oracle] [--oracle-only] [--list]` runs the packing workflow of the GUI `BSArchPro.exe` (parity #38), the 4.1.5q route to the XBox 360 archive target and to the `xtexconv.exe` bitmap conversion: a `.bsarch` list (plain JSON; `LoadList` reads it as well as the zip `SaveList` writes) with the archive type, the target, the archive name and the assets, the fixtures below `src` (generated DDS files, a PC and an XBox one, with the port's own `dds::set_up_header`), the GUI started in its autopacking mode (`BSArchPro.exe <list>`, hidden, its settings folder kept in the work folder through the child's `HOME`/`USERPROFILE`/`HOMEDRIVE`/`HOMEPATH`) and the port's `xedit --edit archive pack --target` over the same sources.

A case with `xtexconv` also puts the `xtexconv-stub` binary (this package's second binary: it rewrites the DDS header as an XBox one, the form the real tool produces) next to the program as `xtexconv.exe`, and the port gets `--program-path` pointed at that folder, so the spawn, the arguments, the temporary file and the read-back are exercised on both sides.

Compared: the archive bytes, or for a case that refuses (the XBox target over a PC texture with no tool) the dialog with no archive written and the port's message against the oracle executable's own UTF-16 strings (`oracle_utf16_strings`; a task dialog's body is drawn by DirectUI and holds no readable text).

Finding (parity #38): the 4.1.5q `BSArchPro.exe` autopacking mode shows its progress form and starts no work when it is driven without a user (no CPU time, empty memos, no file written; with and without `SW_HIDE`, with the threading on and off), so every case is reported `oracle-failed` after the 120 s watch and the run does not fail on it; the mode is kept for a run at a screen.

The port's route is checked meanwhile by the tests of `crates/xedit-session/tests/archive.rs` (`the_xbox_target_refuses_the_other_platforms_textures`, `dds2xbox_gives_upstream_messages`).
