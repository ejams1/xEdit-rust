## FormIDs and module flags

`formids change` runs upstream's "Change FormID": `SetLoadOrderFormID` on the record (the record leaves its master and overrides, takes the FormID, and is registered again; its child group follows, and an interior cell moves to the block and sub-block of its new object ID), then `CompareExchangeFormID` on every record that refers to it.

- The new FormID is a load order FormID.

Omitted, it is the next free FormID of the record's file (`NewFormID`, which moves `HEDR\Next Object ID`), or of `--target-file` (the record's file or one of its masters, upstream's "renumber to destination file"). `00000000` and `00000014` are refused, a FormID the file has already fails with `FormID [...] is already present in file ...`, and a FormID of a file that loads before the record's file but is not its master makes that file a master (upstream's `AddRequiredMaster`, without its question; the response lists it in `masters_added`).
- `--overrides` changes the later overrides too (upstream asks "has later overrides, update them too?"): all of them for a master record, the following ones for an override.
- The response lists the referencing records (`referenced_by`) and how many were updated. Without `--target-file` only the records in editable files are updated (the records the dialog lets you pick); with it, and for `formids renumber`, every referencing record is updated when one of them is editable, as upstream's silent update does. The game master is not editable (upstream needs `-IKnowWhatImDoing -IKnowIllBreakMyGameWithThis`), so its records can not be changed.
- The referencing records come from the reference index (see "References"), which the first command that needs it builds for every loaded file; on a big load order without a reference cache that first build takes a while.

`formids renumber` is "Renumber FormIDs from...": the new records of the plugin take the FormIDs from `--start` (six hex digits, three for a light plugin; the next object ID when omitted), keeping the ones already in the new range; `--compact` packs them into `000800`..`000FFF` for an ESL; `--inject-into <master>` gives them free FormIDs of that master (`--preserve-object-ids` keeps the object IDs where the master has them free, `--all-or-nothing` stops when one can not be kept).

Their overrides in later plugins follow, the referencing records are updated, and `HEDR\Next Object ID` of the target moves past the highest FormID used.

`files flags` sets the ESM, light (ESL), medium, update (`--overlay`), blueprint and localized flags of the header as `TwbFile.SetIs...` does: a flag the game does not have is ignored (`supported` lists the game's flags), a medium or update flag clears the other two in Starfield, and a file that is not editable fails (`File "..." is not editable`).

The flags apply in the order esm, localized, blueprint, update, medium, light. `light_compatible` tells whether every new record has an object ID up to `000FFF` (the save refuses a Light plugin otherwise), `medium_compatible` the same for `00FFFF`, `update_compatible` whether the plugin has no new records.

Changing a flag does not move the plugin to another load order slot in the running session.

Run `files flags --dry-run` first to read the compatibility.

```
xedit --json --edit --game sse --load "<Data>\MyMod.esp" batch - <<'JSON'
[
  {"command": "formids.renumber", "params": {"compact": true}},
  {"command": "files.flags", "params": {"light": true}},
  {"command": "files.save", "params": {"output": "<somewhere else>\MyMod.esp"}}
]
JSON
```
