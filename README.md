# xEdit-rust
Rebuild of the xEdit project in Rust. Additionally features CLI control of all xEdit features and native support for AI agents to control it.

The build plan is in [docs/PLAN.md](docs/PLAN.md).

## Goals

- Port xEdit to Rust with 1:1 functionality*
  - Morrowind and Enderal are loaded but not verified against the oracle yet (it can not dump or save Morrowind plugins, and Enderal is not installed); verified support comes later
- Match the upstream release build exactly
  - The official binaries of the baseline tag (`xedit-4.1.5q`) are the oracle; a differential harness compares the port's output with theirs on the real game files, and each phase closes only when its parity gate holds. Upstream quirks are reproduced, not corrected.
- Keep existing scripts, plugins and workflows working. Compatibility with xEdit behaviour, its Pascal scripts and the files it writes wins over every other concern.
- Be easier to contribute to than the Delphi original: a modern toolchain (`cargo build`, `cargo test`, clippy, one command for the parity check), no proprietary compiler, typed interfaces instead of 103 COM-style interfaces in one 50,000-line unit, and a module per upstream unit so the Pascal source stays a readable map of the Rust.
- Expose every operation through the `xedit` CLI as a typed command with `--json` output, stable error codes and a printable schema (`xedit schema`), so that an AI agent can discover and drive the whole tool without the GUI.
  - Make mutation safe for agents: an explicit edit flag, `--dry-run`, structured outcomes, atomic saves with a backup, and a long-lived session (`xedit batch`, `xedit serve`, `xedit mcp`) that runs the same commands.
- Stay mergeable with upstream. One Rust module per Pascal unit with matching names, and `upstream-map.toml` records the last upstream commit merged into each, so upstream changes map onto the port line by line.
- Get faster than upstream once parity holds

## Status

- Phases 0 to 2 of the plan are done: every game mode from Morrowind to Starfield and every save format load and dump like the release build, verified file by file against it on the local game installs (Morrowind's masters and Enderal are not verified: the oracle cannot dump Morrowind plugins and Enderal is not installed on the development machine).
- Phase 3 (write path and daemon) is done on draft pull request #12. A loaded plugin can be edited, saved and served to a long-running session, and the saved bytes are xEdit's. `cargo xtask parity oracle-save` drives the xEdit GUI release build headlessly to save every plugin of the corpus and compares the port's save with it: 239 of 249 plugins are byte-identical, 7 are refused with the same message by both, and the 3 Morrowind masters have no oracle save because the 4.1.5q GUI cannot save Morrowind plugins; `cargo xtask parity roundtrip` loads and saves the corpus and agrees. `cargo xtask parity oracle-edit` replays scripted edit sequences on both: 3 of 4 give the oracle's bytes, and the master update of a plugin with unchanged sorted arrays is the remaining difference.
- The write commands are `xedit elements set|add|remove` (any element by path, a member, an array entry or a child record such as a reference of a cell), `xedit records copy|delete` (as an override or a new record, deep, with the masters it needs added), `xedit masters add|sort|clean` (with every FormID of the plugin rewritten to follow them), `xedit formids change|renumber` (updating the records that refer to the changed FormIDs), `xedit files flags` (ESM, ESL, medium, update, blueprint, localized) and `xedit save`. Every mutating command takes `--dry-run`, refuses to run without the global `--edit` flag otherwise, and changes memory only until `xedit save`. The AfterSet and AfterLoad callbacks of the definitions run, as in xEdit.
- `xedit batch` runs several commands in one session, so an edit and its save go together. `xedit serve` keeps a session loaded and answers every registry command as a JSON-RPC method over stdio or a Windows named pipe, and `xedit mcp` serves the same commands as MCP tools over stdio; both are built from the registry at run time, keep the `--edit` gate and `dry_run`, and leave saving to an explicit `files.save`. The skills `using-xedit-cli` and `adding-a-command` under `.claude/skills` describe how to drive the tool safely and how to add a command.
- Loading reads the groups of a plugin on all CPUs and `xedit dump` builds and writes the records on all CPUs (`--threads N`; about five times faster on `Skyrim.esm` than on one thread), with output that does not depend on the thread count.
- Known gaps of the write path, all listed in [docs/PLAN.md](docs/PLAN.md) under "Owed from phase 3": writing the string tables of a localized plugin (a localized string can not be set or copied), copying over an existing override and partial forms, sorted arrays inside a subrecord, the reference index (records that refer to a FormID are found by a scan until phase 4), the identity FormID of a Morrowind record, and one session per process.
- Archives (phase 5 step 1): `xedit archive list|extract|pack` and the `bsarch` binary read, unpack and write BSA (Morrowind, Oblivion, Fallout 3, New Vegas, Skyrim LE and SE) and general BA2 (Fallout 4, Fallout 76, Starfield) archives. `bsarch` takes the arguments of `BSArch.exe` and prints its text. Packing runs on all CPUs, and the archives are byte for byte the ones `BSArch.exe -mt:no` writes, for every thread count: `cargo xtask parity bsarch` compares the listings, the unpacked files and the packed archives of the game archives with the oracle. The texture archives (`DX10`, Fallout 4 and Starfield) pack and unpack too (phase 5 step 2): a DDS file is stored as chunks of mipmaps and written again with the header xEdit makes, byte for byte as `BSArch.exe` does for the 125 texture archives of the corpus and for generated DDS folders, with the pixel conversion and the half-float array conversions on vector instructions where the machine has them (a scalar version gives the same bytes; `XEDIT_SIMD=off` forces it).
- NIF meshes and materials (phase 5 step 3): `xedit assets dump|blocks|save|set|from-json` read, dump, edit and write the NIF and KF files of every game from Morrowind to Fallout 4, Fallout 4 BGSM and BGEM materials, the LOD settings and tree LOD files, FUZ voice files and DDS headers, loose or inside an archive. A file loaded and saved gives xEdit's bytes, and the dump is the JSON of Sniff's converter or xEdit's text dump: `cargo xtask parity nif` compares the port with `Sniff.exe` and with the xEdit GUI's script adapter on every mesh and material of the game archives. Fallout 76 and Starfield meshes are refused with xEdit's own message, as the 4.1.5q release does; optimizing, stripifying and triangulating meshes came with LOD generation (step 6).
- Sniff (phase 5 step 4): `xedit sniff list|run` and the `sniff` binary (which takes the arguments of `Sniff.exe`'s automation mode) run 38 of Sniff's 50 batch operations on the meshes of a folder or an archive, on all CPUs: updating tangents and bounds, optimizing meshes for the vertex cache, overdraw and vertex fetch (the meshoptimizer port of `wbMeshOptimize`) and analyzing them, applying and adjusting transforms, the universal tweaker and fixer, the JSON converter, replacing asset paths, the Havok, shader flag and animation operations, and the reports (check for errors, draw calls, UVs, transforms, Havok info). `cargo xtask parity sniff` compares each operation's output files and log with `Sniff.exe` on the game archives. The other 12 operations come with step 5 (Find textures and the texture checks of Check for errors need the DDS code, which is ported).
- LOD generation (phase 5 step 6): `xedit lodgen` runs xEdit's LODGen mode on the loaded plugins with the options of its form: Oblivion's distant LOD, the trees LOD of Skyrim and the Fallouts (billboard atlas, list and blocks, or trees as 3D objects LOD), the objects LOD with its texture atlas (through `LODGenx64.exe`, as upstream), and the split of a trees LOD atlas into billboards. The texture work is a port of the part of the Vampyre Imaging Library xEdit uses (DXT and ATI block codecs, format conversion, Lanczos resampling, mipmaps, DDS). `cargo xtask parity lodgen` compares the output files and log with the GUI's LODGen mode on worldspaces of Fallout 4, Skyrim SE, New Vegas, Fallout 3 and Oblivion.
- Later phases (scripting, GUI, performance, release) have not started; phases 4 and 5 are in progress (phase 5 still to do: the other Sniff operations, the skills and docs step).

## Non-goals

- No new file formats, record definitions or game support beyond what the baseline tag ships. New upstream releases are taken in through the sync procedure, not ahead of it.
- No redesign of the data model, the scripting language or the GUI. JvInterpreter Pascal stays the script language; the GUI reproduces the upstream forms on top of the same commands.
- No separate agent API. Agents use the same command layer as the CLI and the GUI; there is no second surface to keep in step.
- No dependence on a Delphi compiler. The oracle is the released binary, never a rebuild.
- No non-Windows platform before the Windows port is complete. Linux and macOS follow, behind thin platform modules.
- No game files in the repository. The parity corpus comes from local installs through environment variables.

## License

xEdit-rust is a port of [xEdit](https://github.com/TES5Edit/TES5Edit) by ElminsterAU and the xEdit contributors. xEdit is licensed under the Mozilla Public License 2.0, and ported files are Modifications that must stay under it, so this whole repository is licensed under [MPL-2.0](LICENSE). See [NOTICE](NOTICE) for attribution and third-party terms.

Rules for contributions:

- Every source file starts with the MPL-2.0 notice shown in [NOTICE](NOTICE).
- A file ported from upstream names its upstream origin below that notice.
- Every crate sets `license.workspace = true`.
- Dependencies must pass `cargo deny check licenses` against [deny.toml](deny.toml).
