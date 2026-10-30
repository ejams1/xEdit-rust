## Saving a plugin

`save` runs `PrepareSave` and `WriteToStream` as upstream does: every unmodified record is copied as loaded, a modified record is rebuilt from its elements (and compressed again when its flag says so), and the file's CRC32 is computed on the result.

The response reports `bytes`, `crc32`, `loaded_crc32`, `changed`, `written` and `backup`.

A save of an untouched plugin equals the bytes xEdit's own GUI saves for every plugin of the corpus that the oracle saves (239 of 249 plugins; the rest are refusals the oracle gives too, and the Morrowind masters, which it can not save).

That is not always the input file: xEdit drops some data on load (the `OFST` of a worldspace) and rewrites the header (the `HEDR` record count, `INCC`, the `ONAM` list of a master, the ESM flag of an `.esm`).

- `--dry-run` builds the file and reports, writing nothing; use it to see `changed` and the refusal a save would give.
- `--output` writes somewhere else than the loaded path. Never point it into a game's `Data` folder while testing; the harness writes into the parity cache.
- Error codes: `save_refused` carries an upstream `PrepareSave` message, which the oracle gives for the same file (a Starfield blueprint module, a record in the wrong group, an `.esp` master where the game forbids it, an official Starfield module whose header the save would have to edit: `[TES4:00000000] can not be edited`). The messages of a save name records as the xEdit GUI does (`EditorID "Name" [SIG:FormID]`), so they read like the GUI's; the other commands keep xDump's names.
- Only the worldspace records are initialized by a save of an unmodified file (upstream drops their `OFST` subrecord and marks their children modified), so a big master saves in seconds; the children of its worldspaces are rebuilt record by record.
- That initialization runs upstream's `wbWorldAfterLoad`, so a worldspace record of an unedited plugin can gain or lose members on save: a member that the parent flags (`Parent Worldspace\PNAM`) say is inherited is removed (`CNAM` climate with "Use Climate Data", `NAM2` with "Use Water Data", `DNAM` with "Use Land Data", `LOD Data` with "Use LOD Data", `ICON`/`MNAM` with "Use Map Data"), and one it says is not inherited is added with its default (a worldspace without a parent gets `CNAM` `0000015F` if it had none). Together with the header's `INCC`, this is why `changed` is true for a WRLD/CELL override plugin that nothing edited. The oracle's save is byte-identical (checked 2026-10-29 on Diamond City and Goodneighbor overrides carrying a `CNAM` and on Northern Springs worldspace overrides), so the port keeps it; deleting other records does not cause it.
