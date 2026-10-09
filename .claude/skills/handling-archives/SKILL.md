---
name: handling-archives
description: Use when a Bethesda archive (BSA or BA2) has to be read, unpacked, built or checked with this repository - listing what an archive holds and its format, extracting it, packing folders, files or other archives into one, the texture (DX10) archives, or running BSArch's own console tool as the `bsarch` binary.
---

# Handling archives

The port reads, unpacks and writes every archive format the baseline ships: BSA of Morrowind, Oblivion, Fallout 3, New Vegas and Skyrim (LE and SE) and general BA2 of Fallout 4, Fallout 76 and Starfield, plus the texture (DX10) archives of Fallout 4 and Starfield. The archives the port writes are byte for byte the ones `BSArch.exe -mt:no` writes for the same sources and options, whatever the thread count; that is the phase gate, checked by `cargo xtask parity bsarch` (see `checking-parity`).

## Which route

| Route | Use it for |
| --- | --- |
| `xedit archive list\|extract\|pack` | Everything an agent does: typed parameters with a schema (`xedit schema`), `--json` results, `--dry-run`, the `--edit` gate. Also MCP tools through `xedit mcp`. |
| `bsarch` (`cargo build --release -p bsarch`) | Replacing `BSArch.exe` in a script or build step: it takes the upstream arguments and prints the upstream text. One extension: `-threads:N`. |
| `xedit assets ... --archive-path P` | Reading or editing one NIF, material or DDS header inside an archive without extracting it (see `using-xedit-cli`). |

Extract only when the files are needed on disk (other tools, a diff, a pack from a folder); the `assets` and `sniff` commands read archives directly.

## Mutation rules

`archive.extract` and `archive.pack` are `mutates` commands: without the global `--edit` they fail with `edit_required` unless `--dry-run` is given, and a dry run writes nothing. `archive list` reads only. Nothing else needs saving; these commands write files, not plugin data.

- `xedit archive pack --archive out.ba2 --format fo4 <source...> --dry-run` merges the sources, applies the filters and reports `source_files` and `source_counts` without writing. Run this first: it shows exactly which files a folder yields, which is where the surprises are (below).
- `xedit archive extract <archive> <folder> --dry-run` reports the number of files that would be written.

## Listing

```
xedit archive list "<Data>\Fallout4 - Meshes.ba2" [--files] [--folder meshes\armor] [--offset N] [--limit N]
```

The response has the format name, the header version, the file count, how many files are compressed, the compression type, the BSA archive and file flags (with the names of the set bits, ready to pass back as `--archive-flags`/`--file-flags` of a pack), the warnings the archive raises for its game and, with `--files`, a page of the file table: path, size, hash, compression and, in a texture archive, width, height and DXGI format (`total` counts the matches before paging). This is BSArch's `-list` and `-dump` information.

Check the warnings before shipping an archive: they name what makes the game fail, as `TwbBSArchive.Warnings` does (`DDS archive contains uncompressed textures which crash the game`, a file that starts above the 2 GB limit of the BSA formats, a cubemap without the embedded names flag).

## Extracting

```
xedit --edit archive extract "<archive>" "<existing folder>" [--threads N]
```

- With no folder the files go below the folder of the archive. The folder must exist; every file is written under its path in the archive.
- A name that would leave the destination folder (`..`, a drive) is refused with `archive_failed` (`Error processing "<name>": The name leaves the destination folder`), where `BSArch.exe` would write it outside the folder.
- The files are the same for every `--threads`; the work is decompressing and writing. A texture archive writes the DDS files again with the header xEdit makes (`Not a valid DDS file`, `Unsupported DDS format` and `DDS is in XBox format` are the refusals).
- An archive `BSArch.exe` itself cannot unpack (a file flagged compressed that is not, `LibDeflate error: Bad data`) stops the port with the same message, as `archive_failed`.

## Packing

```
xedit --edit archive pack "<out.bsa>" "<folder>" ["<folder2>" "<archive.bsa>"] --format sse -z --dry-run
xedit --edit archive pack "<out.ba2>" "<folder>" --format fo4 -z --split 0 --threads 8
```

- `--format` is `tes3`, `tes4`, `fo3`, `fnv`, `tes5` (the last three are one format), `sse`, `fo4`, `sf1`, or the texture archives `fo4dds` and `sf1dds`. Pack with the format the game loads: an SSE game wants `sse` (LZ4F), Fallout 4 general assets `fo4` (zlib), Fallout 4 textures `fo4dds`.
- Sources are folders, single files or other archives, mixed freely; a later source wins where two hold the same path in the archive. That is how a mod's files are merged over the game's, or a patch over its base.
- `-z` compresses (the default of the format, or `-z:zlib`, `-z:lz4`, `-z:lz4f`); sounds, music and strings stay stored as BSArch leaves them. Morrowind archives have no compression. Texture archives should always be compressed; BSArch warns that uncompressed ones crash the game.
- `--split N` is GB per archive (a BSA splits at 2 GB by default, BA2 does not split, `--split 0` disables it); a split archive numbers from the second file (`mod.bsa`, `mod2.bsa`) and every part is reported. `--filter MASK` (`*`, `?`) keeps only matching names, `--no-share` stops identical files sharing one block of data.
- What a folder yields is not everything under it: files under `data\` or a known asset folder keep the path from there, and the extensions of `cSkippedExtensions` (`.esp`, `.esm`, `.ba2`, `.bsa`, `.dll`, and the rest) are never packed. A dry run shows the count before anything is written.
- A pack that fails leaves no archive behind; `archive_failed` carries the BSArch message (`Error processing "meshes\a.nif": ...`).

## The `bsarch` binary

```
bsarch pack "<folder>+<folder2>" "<out.bsa>" -sse -z -mt:no
bsarch unpack "<archive>" "<folder>" -mt:yes
bsarch "<archive>" -dump
bsarch
```

It is a drop-in for `BSArch.exe`: same argument grammar, same output text (progress lines included), same exit behaviour, so a script written for BSArch needs only the path changed. `-mt:no` and `-mt:yes` write the same bytes in the port; upstream's multithreaded run orders the shared data by thread finish time, and the port reproduces the single-threaded order at any thread count. Use `-threads:N` to bound the pool without changing the bytes.

## Verifying

`cargo xtask parity bsarch [--game G]... [--archive NAME] [--max-size MB] [--textures] [--synthetic] [--cross] [--only TEXT] [--keep]` runs the oracle and the port over the game archives and the generated synthetic folders and compares the `-dump` text, the files `unpack` writes and the archives `pack` writes (every applicable compression, the run text too). Use it after a change to the archive code, and `xedit archive list` on both sides for a one-off check. The gate result and the counts are in `docs/PLAN.md` under phase 5.

## Gotchas

- The oracle (`BSArch.exe`) fails on paths longer than 260 characters and can hang at its start; the harness stops a run without CPU time and repeats it. The port writes those paths; keep folder names short when comparing by hand.
- `BSArch.exe` packs at `-mt:no` for the parity runs, because its multithreaded data order is a race. Never compare the port's archive with an oracle run at `-mt:yes`.
- Names are hashed with the system ANSI code page (1252 here); a name outside it hashes as `AnsiString` hashes it in the port too, but the oracle's bytes were not checked for other code pages.
- Morrowind archives: `Tribunal.bsa` is the smallest in the corpus and was checked by hand; the synthetic folders cover the format in the harness.
