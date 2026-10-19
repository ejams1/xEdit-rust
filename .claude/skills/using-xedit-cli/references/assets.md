## Assets

`assets dump|blocks|types|scan|save|set|optimize|from-json` read and write the files of the data format units of xEdit: NIF meshes and KF animations of every game from Morrowind to Fallout 4, Fallout 4 BGSM and BGEM materials, the LOD settings (`*.lod` of Skyrim and Fallout 4, `*.dlodsettings` of Fallout 3 and New Vegas), the tree LOD files (`*.lst`, `*.btt`, `*.dtl`), FUZ voice files and DDS headers.

They need no `--game` or `--load`.

Each takes `<file>` on disk, or the archive that holds it with `--archive-path <path in the archive>`; the format comes from the extension, or `--kind nif|bgsm|bgem|lod|dlodsettings|lst|btt|fuz|dds`.

| CLI | Registry name | What it does |
|---|---|---|
| `assets dump <file> [--archive-path P] [--kind K] [--format text\|json] [--decimals N] [--euler]` | `assets.dump` | The file as xEdit's `ToText` (one element per line, tab indented, CRLF) or `ToJSON` (what Sniff's "Convert to and from JSON" writes). Floats have 6 decimals unless `--decimals` (6 to 16); rotations print as an angle in degrees and an axis, or as Euler angles with `--euler`. Materials have no JSON form (`load_failed: Not implemented`, as upstream). |
| `assets blocks <file> [--archive-path P]` | `assets.blocks` | The NIF version and every block: index, type and name. |
| `assets types` | `assets.types` | Every NIF block type the definitions know. |
| `call assets.scan --params '{"file":..., "archive_path":..., "what":"blocks"\|"textures", "uv_range":N}'` | `assets.scan` | The light scanner of the script functions `NifBlockList` (`Name=Type` per block) and `NifTextureList` / `NifTextureListUVRange` (texture names with their slot). Reads NIF 20.0.0.0 and later only. |
| `assets save <file> --output PATH [--archive-path P] [--dry-run]` | `assets.save` | Loads and writes the file as xEdit saves it (`SaveToData`; for a NIF the header's block types, sizes and string table are rebuilt, unused strings removed). Reports `equal_to_input`. Needs `--edit` unless `--dry-run`. |
| `assets set <file> <path> <value> --output PATH [--block B] [--archive-path P] [--dry-run]` | `assets.set` | Sets one value and writes the file; the request takes a list of `edits` (`block`, `path`, `value`) through `call`/`batch`. Needs `--edit` unless `--dry-run`. |
| `assets optimize <file> --output PATH [--option NAME]... [--archive-path P] [--kind K] [--dry-run]` | `assets.optimize` | Runs the mesh spells of `TwbNifFile` and writes the file: `SpellOptimize` with `--option vertex-cache`, `overdraw`, `vertex-fetch`, `stripify` or `triangulate` (repeatable; omitted: vertex-cache and overdraw, the default of `SpellOptimize`). `stripify+vertex-cache` is `SpellStripify`, `triangulate` is `SpellTriangulate`. Reports `changed` (`SpellOptimize`'s result) and `equal_to_input`. Needs `--edit` unless `--dry-run`. |
| `assets from-json <file.json> --output PATH [--kind K] [--dry-run]` | `assets.from-json` | Builds a file from a `assets dump --format json` dump (or a material from the material editor's JSON) and writes it. Needs `--edit` unless `--dry-run`. The file is read as xEdit reads it: by its byte order mark, else in the ANSI code page, so save a dump with non-ASCII text with a UTF-8 BOM. Values of the wrong JSON type fail as in xEdit (`Cannot cast Array into String`). |

- Paths below a NIF block use the dump's names with `\` between them: `Transform\Scale`, `Vertex Data\[3]\Normal`, `Shader Flags 1\Model_Space_Normals` (a flag by name reads and sets `1`/`0`). `--block` picks the block: `header`, `footer`, a block index, or a path of block types and names from a root block (`BSFadeNode\Body\BSLightingShaderProperty`, as Sniff's universal tweaker). Without `--block` the path starts at the file: `[1]\Name` is the name of block 0 (`[0]` is the header).
- The value is the text the dump prints: `Havok | Dynamic` for flags, an enumeration name or its number, `#RRGGBB` for colours, `None` or an index for a block reference. A reference shows as `<index> <type> "<name>"`.
- Saving a file the port loaded unchanged gives xEdit's bytes; `cargo xtask parity nif` checks that on every NIF and material of the corpus archives (see the `checking-parity` skill).
- Error code `load_failed` carries upstream's message (`Error reading NIF block 3 NiTriShapeData: Unexpected end of stream...`, `Unknown NIF version "20.2.0.7" ("User Version"=12, "User Version 2"=155)`: Fallout 76 and Starfield meshes are not supported by xEdit 4.1.5q either), `edit_failed` the message of a value that does not take (`'x' is not a valid integer value`).
