## Saves

`saves dump` reads the saves of `tes4` (`.ess`, OBSE `.obse`), `fo3` (`.fos`, FOSE `.fose`), `fnv` (`.fos`, NVSE `.nvse`), `fo4` (`.fos`, F4SE `.f4se`) and `tes5`, `sse`, `enderal`, `enderalse` (`.ess`, SKSE `.skse`); the co-save definitions are chosen by the extension.

For Fallout 3 and Oblivion saves the oracle warns that they are not supported yet; it reads Fallout 3 saves all the same, and the port does both.

Upstream's Oblivion save definitions expect the Fallout 3 header magic, so every real Oblivion save stops with `Expected header Magic FO3SAVEGAME, found TES4SAVEGAM`, in the oracle and in the port; only an OBSE co-save (`.obse`) would read.

The dump follows the oracle exactly, including its quirks: the LZ4-compressed body of a Skyrim SE save is shown as raw bytes because the oracle's decompression fails, and a Skyrim LE save whose plugin list sits in the save content loads no plugins, so its hardcoded FormIDs show the oracle's access violation text.

Saves are read only; there is no save-editing command.
