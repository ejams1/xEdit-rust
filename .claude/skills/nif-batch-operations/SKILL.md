---
name: nif-batch-operations
description: Use when many NIF meshes, KF animations, BGSM or BGEM materials, or DDS textures have to be processed in one go with this repository - updating tangents or bounds, applying and adjusting transforms, the universal tweaker and fixer, converting to and from JSON, replacing asset paths, copying, merging, grouping and painting geometry, the animation and collision operations, the reports (check for errors, find textures, draw calls, UVs), or generating LOD from the meshes of a load order (`xedit lodgen`).
---

# NIF batch operations

These are the batch operations of Sniff (S'Lanter's NIF Helper) over NIF, KF and material files, plus the LODGen mode, which builds the LOD meshes and atlases of a load order. They read a folder or an archive; nothing here needs a loaded plugin except `xedit lodgen`, which needs the whole load order.

## Which route

| Route | Use it for |
| --- | --- |
| `xedit sniff list\|run` | The agent's route: an operation by title, settings as `--set NAME=VALUE`, the file statuses and Sniff's messages as JSON, `--dry-run`, the `--edit` gate. Also MCP tools through `xedit mcp`. |
| `sniff` (`cargo build --release -p sniff`) | Replacing `Sniff.exe` in a script or a batch file: the automation-mode arguments (`-OP: -I: -O: -S: -LOG: -P: -subdir: -skip: -all: -threads:`) and the output text. `sniff -list` prints the operations. |
| `xedit lodgen` | Generating distant and tree LOD (Oblivion `.lod`/`.cmp`, the atlases and blocks of the later games) from the loaded plugins. |
| `xedit assets ...` | One file: dump, edit or save a single mesh or material (see `using-xedit-cli`). |

## Mutation rules

`sniff.run` and `lodgen.generate` are `mutates` commands. Without the global `--edit` they fail with `edit_required` unless `--dry-run` is given; `sniff.list` reads only.

- `xedit sniff run "<operation>" <input> --dry-run` processes every file and reports what would change, writing nothing: no output file and no log file.
- `xedit lodgen --dry-run` lists the worldspaces and the options and writes nothing; without it the run writes the LOD files, the export files of `LODGenx64.exe` and, when `--settings` names a file, the settings file back, as the form does.
- Neither command needs a `files.save`: they write their own files.

## Running a Sniff operation

```
xedit sniff list
xedit sniff run "Update tangents and binormals" "<folder or archive>" --output "<folder>" --dry-run
xedit --edit sniff run "Universal tweaker" "<archive>" --output "<folder>" --set sBlocks=BSLightingShaderProperty --set sPath=Alpha --set sValue=0.5
xedit --edit sniff run "Check for errors" "<archive>" --skip-on-errors
```

1. `xedit sniff list` first. It names every operation with its group, the games it supports, the file extensions it takes, whether it only reports, its settings section and each setting with its default. `sniff list` gives the same from the binary.
2. An operation is picked by its title, any case. `--set` sets a setting of its section (`sniff list` names them); `--settings` reads an ini in Sniff's form, where the section is the title without spaces (`[Universaltweaker]`) and `[Main]` holds the parameters. The settings ini of the `sniff` binary is resolved beside the program, as upstream's; `sniff.ini` is read when `-S:` is not given.
3. Changed files go to `--output` under their path in the input (the folder must exist); an operation that only reports needs none. `--copy-all` writes the unchanged files too, `--path-contains` keeps a subset, `--no-subdir` leaves the subfolders.
4. Read the response: the messages (`Updated: <file>`, `Skipped: <file>: <error>`, the summary line), the counts and each file's status. Without `--skip-on-errors` the first failing file stops the run and is reported as `aborted`, with the files before it done, as Sniff does.
5. Compare against the oracle when it matters: `cargo xtask parity sniff` (below).

The operations, by group, so you know what is there: **NIF** - tangents and binormals, bounds, optimize mesh, search and replace assets, JSON converter, universal tweaker and fixer, apply and adjust transformation, attach parent, copy geometry blocks, vertex color painting, group and merge shapes, merge properties, remove nodes, remove unused nodes, convert block type, unskin mesh, add NiLODNode, RootCollisionNode and bounding box, set missing names. **Report** - check for errors, analyze mesh, transform and Havok information, unwelded vertices, excessive draw calls, UVs, find textures. **Animation** - copy anim controlled blocks and priorities, remove controlled blocks, quadratic to linear, fix 3DS exported KF, optimize Animations, add headtracking, facial, NiTransformData and skeleton blocks, and Weijiesen's blow up thing. **Shader** - update shader flags, real time reflections (NVSE), vanilla plus particles (NVSE). **Collision** - update Havok settings, inertia and ragdoll constraint, search for Havok material. 49 of the 50 operations of the 4.1.5q form are ported; `Update MOPP code` (Collision) fails with `unsupported` (it calls `NifMopp.dll`, which the port does not have) and `ProcCollapseLinksArrays` has no operation in that form.

### Textures

Two operations work on DDS files: `Find textures` reports (or copies, without `bReportOnly`) the textures that match its filters, and the texture checks of `Check for errors` (`Invalid texture size or format`, `Unsupported texture formats`) run on `.dds` files while `ProcessedFiles` of the check list is set to them. Both read the DDS record of an entry of a texture archive (width, height, mipmaps, DXGI format, chunks) or the header of a loose or archived file with `wbDDS`. A file that is not a DDS is reported `Not a valid DDS file`. `Find textures` on a texture archive prints an `XBOX` marker on every line of the oracle's report that the port does not (upstream never assigns the flag; `UPSTREAM-QUIRK` in `proc_find_textures.rs`) - strip the marker when comparing by hand.

## Generating LOD

```
xedit --edit --game sse --load "<Data>\Skyrim.esm" --load "<Data>\Update.esm" ... lodgen \
  --worldspace Tamriel --set AtlasWidth=4096 --set AtlasDiffuseFormat=DXT5 --output "<scratch>\LOD" --seed 12345
```

- Load every plugin of the load order, in order, with `--load`; the LOD is built from the records and textures that win. `--worldspace` selects by editor ID (repeatable; without it the worldspaces the form checks itself, and Oblivion generates every worldspace).
- `--set NAME=VALUE` sets an option of `[<APP> LOD Options]` (`ObjectsLOD`, `TreesLOD`, `Trees3D`, `BuildAtlas`, `AtlasWidth`, `AtlasDiffuseFormat`, `Chunk`, `LODLevel`, `TreesBrightness`, ...; the schema lists them all, `xedit lodgen --dry-run` prints them). `--settings` is the `<APP>LODGen.ini` the form reads, and the command writes it back as the form does.
- Give `--output`: the default is the data folder. `--scripts` is the folder with `LODGenx64.exe`, `Texconvx64.exe`, `LODGen_flat_lod.nif` and the atlas maps: the objects LOD meshes are built by `LODGenx64.exe` from the `LODGen.txt` the command writes there, so a missing binary or atlas map is a visible failure. `--data` and `--game-ini` control where the resources (archives) load from.
- Tree rotations come from `RandSeed`, which upstream seeds from the clock; pass `--seed` to make a run reproducible (the response reports the seed used). `--split-trees` runs the form's hidden `Split LOD Atlas` instead: one billboard `.dds` and `.txt` per tree below `<output>Textures\Terrain\LODGen\AtlasSplit_<atlas>\` (Skyrim and the Fallouts before Fallout 4).
- Fallout 4 and Skyrim objects LOD find no LOD models in the 4.1.5q definitions (the LOD model path the code reads does not exist there), so they end with `no valid references found`, as the oracle does; that is not a failure of the run. Fallout 76 and Starfield are refused (`unsupported`).
- The response lists the worldspaces with their checks, every option as read, the archives and folders the resources came from, the messages of the generator and the seed. Read the messages before trusting the output.

## Determinism and the oracle's own failures

- Every count of `--threads` gives the same files and the same bytes: Sniff's processing runs on a pool, but the results are committed in file order (its `-threads:1` order), and LODGen's output does not depend on the count.
- Some files make `Sniff.exe` itself fail or differ among its threads (its threads share state): `Merge shapes` crashes with an access violation on the files it merges, `Remove nodes` reads freed memory on three Fallout 4 Creation Club meshes (`TdfElement.Root`, after `RemoveBranch`), and `Add blocks from skeleton` hangs when the harness drives it on its own desktop. The port writes those files; the parity harness reports them as `oracle-failed` (or `equal-error` where both refuse the same file, such as the Fallout 76 and Starfield meshes with `Unknown NIF version`). Do not "fix" a difference like this without reading the harness report first.
- The single-file mode of `Copy geometry blocks` (`bMatchingFiles=0`) hangs `Sniff.exe` as soon as a block matches: it has no parity case and a unit test stands in for it.

## Verifying

- `cargo xtask parity sniff [--case NAME]... [--game G]... [--archive NAME]... [--sample N] [--threads N] [--list] [--keep] [--refresh-oracle]` runs each case (an operation with a settings set) against `Sniff.exe` on the archives of its games and compares each file's output, the errors, the log lines, the summary counts and the processor's own log file. `--sample N` unpacks only the first N files per archive for a quick run; the gate runs without it. `--list` prints the cases.
- `cargo xtask parity lodgen [--case NAME]... [--game G]... [--oracle-only] [--refresh-oracle] [--keep] [--timeout M] [--list]` compares every output file, the export files and the log with the GUI's LODGen mode; the harness recovers the oracle's tree-rotation seed and passes it to the port with `--seed`.
- The recorded results, the cases still owed and the known differences are in `docs/PLAN.md` under phase 5, and the harness details are in `checking-parity`.

## Gotchas

- Unknown operation or a bad setting is `invalid_params` with Sniff's own message; a missing input or output folder is `invalid_params` too. The operation's settings section name is its title without spaces (`Copy anim controlled blocks` -> `[Copyanimcontrolledblocks]`); `sniff list` prints it.
- An ini written by the GUI's Sniff holds the settings of every operation; the command reads only the section of the operation it runs, and `--set` overrides it.
- The `sniff` binary writes the messages to stdout and, with `-LOG:`, the same lines to the log file as Sniff writes it. Do not parse the JSON envelopes of `xedit sniff run` and the binary's text as the same format.
