## Archives

`archive list|extract|pack` read, unpack and write BSA (Morrowind through Skyrim Special Edition) and BA2 (Fallout 4, Fallout 76, Starfield) archives.

They are the three modes of `BSArch.exe`, need no `--game` or `--load`, and the archives `pack` writes are byte for byte the ones `BSArch.exe -mt:no` writes for the same sources and options, whatever the thread count.

The `bsarch` binary (`cargo build --release -p bsarch`) takes the upstream arguments and prints the upstream text; use it where a script already calls `BSArch.exe`.

| CLI | Registry name | What it does |
|---|---|---|
| `archive list <archive> [--files] [--folder F] [--offset N] [--limit N]` | `archive.list` | Format, version, flags, compression, warnings and with `--files` a page of the file table. Reads only. |
| `archive extract <archive> [<folder>] [--threads N]` | `archive.extract` | Unpacks every file below the folder (which must exist; the archive's folder when omitted). Needs `--edit` unless `--dry-run`. |
| `archive pack <archive> <source>... --format F [-z[=zlib\|lz4\|lz4f]] [--split GB] [--filter MASK]... [--no-share] [--threads N] [--archive-flags HEX] [--file-flags HEX] [--target pc|xbox]` | `archive.pack` | Packs folders, files and archives (later sources win on equal names). Needs `--edit` unless `--dry-run`. `--target` is the platform of a DDS archive (`pc` default, `xbox` the GUI's "XBox 360 Archive"): see "Texture archives" under Known gaps. |

- `--format` is `tes3`, `tes4`, `fo3`, `fnv`, `tes5` (the last three are one format), `sse`, `fo4`, `sf1`; `fo4dds` and `sf1dds` are the texture archives (Fallout 4 and Starfield DX10 BA2).

Every file packed into one must be a DDS file: it is stored as chunks of mipmaps (a 1024 pixel texture in three, a cube map in one) and `extract` writes the DDS file again with the header xEdit makes (`Not a valid DDS file`, `Unsupported DDS format` and `DDS is in XBox format` are the refusals; a 24 bit RGB file is stored as 32 bit).

Texture archives should be compressed (`-z`); BSArch warns that uncompressed ones crash the game.
- `-z` compresses with the default of the format (zlib for Oblivion, Fallout 3, New Vegas, Skyrim LE and Fallout 4, LZ4F for Skyrim SE, zlib or LZ4 for Starfield); sounds, music and strings stay stored, as BSArch leaves them. A BSA over 2 GB is split by default (`--split 0` keeps one archive, a number is GB, at most 8) and a split archive numbers its files from the second one: `mod.bsa`, `mod2.bsa`.
- A dry run of `archive pack` adds the sources and reports `source_files` and `source_counts` without creating anything; run it first to see which files a folder yields (files under `data\` or a known asset folder keep the path from there; the extensions in `cSkippedExtensions` such as `.esp`, `.bsa`, `.dll` are never packed).
- Error code `archive_failed` carries the message BSArch prints (`Error processing "meshes\a.nif": ...`, `Cannot open file "...". The system cannot find the file specified`). A pack that fails leaves no archive behind. A name that would leave the destination folder (`..`, a drive) is refused by `extract`, where BSArch would write it.
- The same bytes come out for every `--threads`; upstream's multithreaded packing writes the data in the order the threads finish, the port writes it in the order of a single thread.
