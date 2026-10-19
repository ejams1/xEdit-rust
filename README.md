# xEdit-rust
Rebuild of the xEdit project in Rust. Additionally features CLI control of all xEdit features and native support for AI agents to control it.

The build plan is in [docs/PLAN.md](docs/PLAN.md). Agents start with [CLAUDE.md](CLAUDE.md); completed phase evidence is linked from the plan.

## Progress

Overall: 7 of 10 phases merged.

```text
[##############......] 70%   phases 0-6 of 0-9 merged into main
```

| Phase | Scope | Progress | State |
| --- | --- | --- | --- |
| 0 | Foundations | `[##########]` | Merged |
| 1 | Read path for the first games | `[##########]` | Merged |
| 2 | All games and saves | `[##########]` | Merged |
| 3 | Write path and daemon | `[##########]` | Merged (PR #12) |
| 4 | Analysis and tool modes | `[##########]` 10 of 10 steps | Merged (PR #13) |
| 5 | Archives, assets and LOD | `[##########]` | Merged (PR #14) |
| 6 | Scripting | `[##########]` 11 of 11 steps | Merged (PR #20) |
| 7 | GUI | `[..........]` | Not started |
| 8 | Performance | `[..........]` | Not started |
| 9 | Release and cutover | `[..........]` | Not started |

The overall bar counts merged phases only: 2 cells per phase out of 20. A phase on a draft pull request counts once it merges. Per-phase bars have 10 cells and show completed steps as a share of the phase's steps, as listed in [docs/PLAN.md](docs/PLAN.md).

**Keep this section current.** Update it in the same pull request whenever a step lands, a phase starts or a phase merges: redraw the bars, change the step counts and the state column, and update the overall percentage.

## Goals

- Port xEdit to Rust with 1:1 functionality*
  - Morrowind, Enderal and Oblivion Remastered load but are not verified against the oracle yet: the 4.1.5q release cannot dump or save Morrowind plugins (issue #9), and Enderal (issue #7) and Oblivion Remastered (issue #80) are not installed on the development machine. The rest of what is verified, deferred and out of the oracle's reach is under Status.
- Match the upstream release build exactly
  - The official binaries of the baseline tag (`xedit-4.1.5q`) are the oracle; a differential harness compares the port's output with theirs on the real game files, and each phase closes only when its parity gate holds. Upstream quirks are reproduced, not corrected; an upstream bug that loses or corrupts data is the one exception: the port keeps the data and records the divergence (see Status).
- Keep existing scripts, plugins and workflows working. Compatibility with xEdit behaviour, its Pascal scripts and the files it writes wins over every other concern.
- Be easier to contribute to than the Delphi original: a modern toolchain (`cargo build`, `cargo test`, clippy, one command for the parity check), no proprietary compiler, typed interfaces instead of 103 COM-style interfaces in one 50,000-line unit, and a module per upstream unit so the Pascal source stays a readable map of the Rust.
- Expose every operation through the `xedit` CLI as a typed command with `--json` output, stable error codes and a printable schema (`xedit schema`), so that an AI agent can discover and drive the whole tool without the GUI.
  - Make mutation safe for agents: an explicit edit flag, `--dry-run`, structured outcomes, atomic saves with a backup, and a long-lived session (`xedit batch`, `xedit serve`, `xedit mcp`) that runs the same commands.
- Stay mergeable with upstream. One Rust module per Pascal unit with matching names, and `upstream-map.toml` records the last upstream commit merged into each, so upstream changes map onto the port line by line.
- Get faster than upstream once parity holds

## Non-goals

- No new file formats, record definitions or game support beyond what the baseline tag ships. New upstream releases are taken in through the sync procedure, not ahead of it.
- No redesign of the data model, the scripting language or the GUI. JvInterpreter Pascal stays the script language; the GUI reproduces the upstream forms on top of the same commands.
- No separate agent API. Agents use the same command layer as the CLI and the GUI; there is no second surface to keep in step.
- No dependence on a Delphi compiler. The oracle is the released binary, never a rebuild.
- No non-Windows platform before the Windows port is complete. Linux and macOS follow, behind thin platform modules.
- No game files in the repository. The parity corpus comes from local installs through environment variables.

## Status

The progress bar above counts merged phases of [docs/PLAN.md](docs/PLAN.md); it says nothing about how much of xEdit is verified. That is below, from the full-corpus reruns of every parity mode that closed the parity backlog (issue #41, pull request #77, port `610b70f`, oracle xEdit 4.1.5q, the vanilla plugins of the local installs of 10 games). Every number names the `cargo xtask parity` mode that produced it; [docs/PLAN.md](docs/PLAN.md) has the history of each mode and the `checking-parity` skill how to run and read it.

### What the port does

- **Read and dump** every game mode of the baseline from Morrowind to Starfield and every save format: `xedit dump` (xDump's output), `xedit saves dump`, `xedit check` and `xedit dump --check` (Check for Errors), with the GUI's archive discovery, resource lookup and string tables.
- **Edit and save**: elements by path (`xedit elements get|set|add|remove|assign|templates|move|check`), records (`xedit records copy|delete|set-deleted|set-hidden|cleanup-injected|compare|compare-selected`), masters (`xedit masters add|sort|clean`), FormIDs (`xedit formids change|renumber`), module flags and new modules (`xedit files flags|new`), the string tables of localized plugins (`xedit localization ...`) and `xedit save`, with the AfterLoad and AfterSet callbacks of the definitions. A mutating command needs `--edit` or runs as `--dry-run`.
- **Analysis**: conflict status and the view (`xedit conflicts`, `xedit compare`, `--compare-to`), the reference index and its cache files (`xedit refs`), the filter of the navigation tree with its presets and the scripted filter (`xedit filter`), the quick auto clean and its steps with the Cleaning options (`xedit clean`), mod groups and their presets (`xedit modgroups`), the merged patch (`xedit patch merged`), and the tool modes and legacy switches of `xeInit.pas` (`xedit tool modes|run`, and the command line a mod manager passes to xEdit).
- **Assets**: BSA and BA2 archives (`xedit archive`, the `bsarch` binary, BSArchPro's XBox target), NIF, KF, materials and the other asset formats (`xedit assets`), Sniff's 50 batch operations (`xedit sniff`, the `sniff` binary) and LODGen (`xedit lodgen`).
- **Scripting**: the JvInterpreter Pascal of the `Edit Scripts` folder (`xedit script list|check|run` and `script.resume`), with the script API of `xEditAPI.pas` and the JVCL units bound, and a headless VCL object model that runs forms and dialogs from recorded answers.
- **One command layer**: the same typed commands drive the CLI, `xedit batch`, `xedit serve` (JSON-RPC over stdio or a named pipe) and `xedit mcp` (MCP tools), with `--json` output and `xedit schema`; a test checks that the three list the same commands. The skills under `.claude/skills` describe how to use them.

### Verified against the release

| `cargo xtask parity` mode | Corpus | Result |
| --- | --- | --- |
| `script --phase parse` | 150 corpus `.pas` | 150 of 150 equal (149 `equal`, 1 `known-difference`) |
| `script --phase probe` | 935 probes | 934 of 935 equal (890 `equal`, 37 `known-difference`, 7 `equal-error`; 1 oracle dialog) |
| `script --phase run` | 156 run cases | 146 of 156 at `b157821` (138 `equal`, 5 `equal-log-loss`, 3 `known-difference`); 5 `different` accepted with their issues (#64, #84); 4 oracle timeouts under load, rerun pending |
| `dump` | 249 plugins of 11 games | 246 of 249 equal; the 3 Morrowind masters are `oracle-unsupported` |
| `saves` | 49 save files | 49 of 49 equal |
| `check` | 249 plugins, 5,852,286 records, 260,691 error lines | 246 of 249 equal; 3 `oracle-unsupported` |
| `check-dump` | 249 plugins | 245 of 249 equal, 3 `oracle-unsupported`; `Starfield.esm` hit the 23.7 GiB port cap in the batch (rerun alone pending) |
| `clean` | 238 plugins | 235 `equal`, 2 `oracle-unsupported`; `NW.esm` is `oracle-log-lost` (the release's log drops the counts; saved bytes equal) |
| `clean-scenarios` | 8 scenarios | 8 of 8 equal |
| `tool-modes` | 74 mode runs | 66 `equal`, 7 `oracle-unsupported`; `setesm` on `BlueprintShips-SFBGS050.esm` hit the 20-minute oracle limit (rerun alone pending) |
| `modgroups` | 16 scenarios | 16 of 16 equal |
| `conflicts` | 11 games, 23.4M records | 10 of 11 equal (Morrowind `oracle-unsupported`); the record probe 2,203 lines, 0 different |
| `refs` | 10 games | 10 of 10 equal: 7,013,961 records, 61,219,249 entries, 79 of 79 cache files |
| `roundtrip` | 249 plugins | 239 `equal` + 10 `refused` (the release refuses them too) |
| `oracle-save` | 249 plugins | 239 `equal` + 7 `equal-error`; 3 `oracle-unsupported` |
| `oracle-edit` | 76 edit sequences | 68 equal + 8 `known-difference` (upstream bugs not reproduced) |
| `filter` | 4 scenarios, 51 configurations | all equal (`sse-filters` 37 of 37) |
| `merged` | 12 scenarios | 12 of 12 equal |
| `resources` | 8 scenarios | 8 of 8 equal |
| `discovery` | 6 runs (2 games' own plugin lists, 4 module-list cases) | 6 of 6 equal |
| `lodgen` | 20 scenarios | 20 of 20 equal |
| `bsarch` | 1,472 pack/unpack/list checks | 1,472 equal |
| `bsarchpro` | 6 GUI packing cases | 6 of 6 equal: five archives byte for byte with BSArchPro's own autopacking, and the XBox refusal `DDS is not in XBox format` |
| `nif` | the NIF and material corpus | 542,505 equal + 250,543 `equal-error` (the release's own crashes) |
| `sniff` | every archive of 10 games, 69 operations | 2,028,002 `equal`, 3,360 `equal-error`, 10,665,231 `unchanged`, 6,157 `oracle-failed` (the release's Sniff crashes or hangs), 0 output-different; 578 log differences, 537 of them the release's uninitialised `XBOX` flag |

### Deferred, each with an owner

`coverage/ledger.toml` lists every upstream GUI binding, switch, game mode, tool mode and script registration with its command; `cargo xtask check` refuses an open entry that names no owner. The open work:

- **2 xDump switch entries** (issue #79): `-bts`/`-btd` change only Skyrim LE saves (the `Unused`/`Remaining` arrays are commented out in the FO4/FO3/FNV definitions), and a full TES5 save dump exceeds the harness timeouts, so no oracle case can witness them; the port takes both from the saves dump CLI and the parity saves mode. Every other switch is covered (`-do` is excluded: it prints raw pointers against `wbBaseOffset`, which only a scanned plugin has).
- **Game modes not verified** (issues #7, #9, #80): Enderal and Oblivion Remastered are not installed on the development machine, and the 4.1.5q oracle cannot dump a Morrowind plugin (below).

The four `JvInterpreterFm` form runners (issue #81), the last open script registrations, landed 2026-10-18: the `jvfm` run case compares `JvInterpreterMakeForm`/`RunForm`/`RunFormModal`/`RunUnit` over a fixture unit and its DFM with the oracle (`cargo xtask parity script --phase run --script jvfm` equal).

### Out of the oracle's reach

- **Morrowind** (issue #9): the 4.1.5q `xDump.exe` dumps no TES3 plugin and its GUI cannot save one, so `dump`, `check-dump`, `roundtrip` and `oracle-save` report the TES3 masters as `oracle-unsupported`; the port still loads them, and `refs`, `conflicts-record` and the other modes that do not need xDump run on them.
- **`Update MOPP code`** (Sniff): the released 64-bit `Sniff.exe` refuses before it looks for `NifMopp.dll`, and the port reproduces the refusal (`sniff --case mopp-update` equal); no released binary can run the operation, so its file body has no oracle.
- **BSArchPro's GUI packing route**: the XBox 360 target and the `xtexconv.exe` conversion only exist in the GUI, which packs from its own autopacking mode; `cargo xtask parity bsarchpro` drives that route.
- **Enderal and Oblivion Remastered**: not installed (issues #7 and #80), so their game modes are ledger entries only.

### Deliberate divergences

The port reproduces upstream's quirks (`UPSTREAM-QUIRK` in the code). An upstream bug that loses or corrupts data is not reproduced (the user's decision of 2026-10-16): the port keeps the data, the code says `UPSTREAM-BUG`, and the parity case is shaped so both sides end in the same state, or the case is counted `known-difference` with its reason, never as equal.

- **A record named first loses its subrecords upstream** (issue #64): the release drops them when a script's first access is by name. `Add Prefix Plus Name To EDID.pas` (850 records), `Oblivion - Export Dialogues.pas` (898 lines), `probes/tes4-topic-full.pas`, `Put worldspace references in the right cells.pas` (the release's save empties 20 `ACTI` records the script names first; the other 869,668 records and 50,483 groups are byte-identical) and `walk-move.pas` are accepted `known-difference`, and the port's behaviour is the one that keeps the data.
- **String tables of an unloaded localized plugin** (issue #75): `TwbLocalizationHandler.AddValue` writes empty tables over a plugin whose tables were not loaded, which loses every existing string. The port loads the tables and adds the value (`UPSTREAM-BUG`; the oracle-edit case is `known-difference`).
- **Starfield perk JSON after a record copy** (issue #73): a set through the virtual perk tree after a copy loses the first edit upstream. The port keeps it (`UPSTREAM-BUG`, `known-difference`).
- **`Worldspace displacement.pas` visits some references twice** (residue of issue #84): the walk now builds the navigation tree's nodes lazily and frozen as the release does (the issue's two cases and the `walk-*` fixtures are equal), but the release's `RemoveNode` searches the tree for the element's ancestors during the move, which initialises nodes earlier than the walk would, and the GUI pump re-enters it; the port does not emulate those, so the case's saved file still differs (accepted `known-difference` on the run case).
- **The scale the cleaning's scale step leaves behind** (parity #35): the release keeps a scale where the port removes it, data the release loses; the clean case is `known-difference` for that file.
- **`Find textures` prints an uninitialised `XBOX` flag on BA2 DDS archives**: the release's local record in `ProcessFile` is never assigned in the BA2 DDS branch, and its report prints `XBOX` from whatever the stack holds (every line of a DDS-type BA2 archive on the corpus). The port prints nothing. No saved bytes differ; the log of `sniff --case find-textures` and `-filter` counts as a log difference on those archives, never a failing output.

Later phases (GUI, performance, release) have not started.

## License

xEdit-rust is a port of [xEdit](https://github.com/TES5Edit/TES5Edit) by ElminsterAU and the xEdit contributors. xEdit is licensed under the Mozilla Public License 2.0, and ported files are Modifications that must stay under it, so this whole repository is licensed under [MPL-2.0](LICENSE). See [NOTICE](NOTICE) for attribution and third-party terms.

Rules for contributions:

- Every source file starts with the MPL-2.0 notice shown in [NOTICE](NOTICE).
- A file ported from upstream names its upstream origin below that notice.
- Every crate sets `license.workspace = true`.
- Dependencies must pass `cargo deny check licenses` against [deny.toml](deny.toml).
