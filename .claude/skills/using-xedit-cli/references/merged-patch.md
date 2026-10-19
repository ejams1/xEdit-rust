## Merged patch

`patch merged <name>` is xEdit's "Create Merged Patch" over every loaded plugin. Load the whole load order with `--load` (in load order), then:

```
xedit --json --edit --game fnv --load "<Data>\FalloutNV.esm" --load "<Data>\DeadMoney.esm" ... patch merged "Merged Patch"
xedit --json --game sse --load ... patch merged "Merged Patch" --dry-run
```

- The name gets `.esp` (a `.esp`, `.esm` or `.esl` extension is dropped first), and the plugin is made in the data folder like xEdit's `AddNewFile`; a file of that name there or in the session is `file_exists`.

Every loaded file becomes a master and the unused ones are cleaned at the end.

UPSTREAM-QUIRK: the game master is listed twice (`AddNewFile` adds it, then the list of every loaded file adds it again), and the patch's FormIDs of the game master point to the second entry, as in xEdit's patches.
- What is merged, as in xEdit 4.1.5q: the records with at least two overrides whose lists the overrides changed in different ways: leveled list entries (`LVLI`, `LVLC`, `LVLN`, `LVSP`, with the counter `LLCT`), container items (`COCT`), faction relations, the hairs, eyes and spells of a race, form lists (only those whose array is sorted, or whose editor ID ends in `OrderedList`, where overrides may only append), creature items and factions, the topics' added quests (New Vegas only), the items, factions, spells, perks and keywords of NPCs (head parts up to Fallout New Vegas), and from Skyrim on the keywords of the item and effect records.

Only sorted arrays are merged; entries are compared by their extended sort keys.

The keywords of the Skyrim and later records sit in a `Keywords` structure, where xEdit's lookup of `KWDA - Keywords` on the record finds nothing, so they are not merged, in xEdit as here.
- The patch copies the winning override of each merged record and replaces its lists with the merged ones. Conflict status and mod groups play no part (xEdit 4.1.5q reads the overrides directly); a patch made with mod groups active is the same.
- For Skyrim, Fallout 4, Fallout 76 and Starfield xEdit asks first whether to go on with a merge it calls unsupported; the command goes on and returns the question as `warning`.
- The response lists `records` (FormID, signature, name, the plugin of the winning override, and per merged list the entries of the merged and of the winning list), `checked` (records with two overrides or more), `masters` after the clean, `messages` (xEdit's log lines: `Error: Can't merge faulty ordered list ...` and the errors of records that could not be merged) and `saved`. `--dry-run` reports the records and lists and makes no plugin; `--no-save` keeps the patch in memory (for `batch` and `serve`, where `save --file "<name>.esp"` follows).
