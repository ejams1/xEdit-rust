---
name: building-patches
description: Use when a conflict found in a load order has to be resolved by building a patch plugin with the native xedit CLI of this repository - xEdit's "Create Merged Patch" for the leveled lists, containers, factions, form lists and keywords several plugins change, or a purpose-built override patch made with files new, records copy and elements set. The native replacement for the xEdit-llm daemon's patch routes (patches.merge, patches.delta).
---

# Building patches

Two routes, both writing one new plugin that loads last:

| Route | For |
| --- | --- |
| `xedit patch merged <name>` | xEdit's "Create Merged Patch": the lists (leveled lists, container items, factions, form lists, keywords, ...) that several plugins change, merged so no plugin's additions or removals are lost to the plugin that loads last. |
| `xedit files new <name>` + `xedit records copy` | A chosen set of records (a conflict audit's losers, a mod's wanted values) overridden or copied into a patch the agent designs. |

The general mutation rules (edit flag, dry run, explicit save, readback) are in `using-xedit-cli`; what to patch comes from `auditing-conflicts`.

## The merged patch

```
xedit --json --edit --game fnv --load "<Data>\FalloutNV.esm" --load "<Data>\DeadMoney.esm" ... patch merged "Merged Patch"
xedit --json --game fnv --load ... patch merged "Merged Patch" --dry-run
```

- The name gets `.esp` (a `.esp`, `.esm` or `.esl` extension is dropped first) and the plugin is made in the data folder like xEdit's `AddNewFile`; a file of that name there or in the session is `file_exists`. Every loaded file becomes a master and the unused ones are cleaned at the end. UPSTREAM-QUIRK: the game master is listed twice (the handler adds it, then the list of every loaded file adds it again) and the patch's FormIDs of the game master point to the second entry, as in xEdit's patches.
- What is merged, as in xEdit 4.1.5q: the records with **two overrides or more** whose lists the overrides changed in different ways — leveled list entries (`LVLI`, `LVLC`, `LVLN`, `LVSP`, with the counter `LLCT`), container items (`COCT`), faction relations, the hairs, eyes and actor effects of a race, form lists (merged as a set; only those whose array is sorted, or whose editor ID ends in `OrderedList`), creature items and factions, the added quests of a Fallout New Vegas `DIAL`, the items, factions, actor effects, perks and keywords of an actor, and from Skyrim on the keywords of the item and effect records.
- An **ordered form list** (editor ID ends in `OrderedList`) takes appends only, because its order matters (`UpdateOrderedTargetList`); a faulty one (an override that is not a superset in order) is reported (`Error: Can't merge faulty ordered list ...`) and left out.
- Only sorted arrays are merged; entries are compared by their extended sort keys. The keywords of the Skyrim and later records sit in a `Keywords` structure, where xEdit's lookup of `KWDA - Keywords` on the record finds nothing, so they are not merged — in xEdit as here.
- **Mod groups play no part**: the handler reads the overrides directly, so a patch made with mod groups active is the patch made without (`cargo xtask parity merged` checks this with a mod group active: the same patch). Do not activate mod groups for the merge; do activate them in the audit that decided what to patch.
- For Skyrim, Fallout 4, Fallout 76 and Starfield xEdit asks first whether to go on with a merge it calls unsupported; the command goes on and returns the question as `warning`.
- The response lists `records` (FormID, signature, name, the plugin of the winning override, and per merged list the entries of the merged and of the winning list), `checked` (records with two overrides or more), `masters` after the clean, `messages` and `saved`. The patch is a loaded plugin of the session afterwards.
- `--dry-run` reports what would be merged and makes no plugin. `--no-save` keeps the patch in memory for `batch` and `serve`, where `save --file "<name>.esp"` follows; without either, the command saves the patch itself (to `--output`, else into the data folder, with a backup unless `--no-backup`).
- The merged patch merges **lists**, not scalar fields: a conflict on a single value (a damage, a flag) is not resolved by it and needs the hand-built route.

## A purpose-built patch

```
cat > patch.json <<'EOF'
[
  {"command": "files.new", "params": {"file": "My Conflict Patch"}},
  {"command": "records.copy", "params": {"form_id": "0001A332", "to": "My Conflict Patch.esp"}},
  {"command": "records.copy", "params": {"form_id": "0001A332", "to": "My Conflict Patch.esp", "as_new": true, "prefix": "Patch "}},
  {"command": "elements.set", "params": {"form_id": "0001A332", "file": "My Conflict Patch.esp", "path": "DATA\\Damage", "value": "42"}},
  {"command": "files.save", "params": {"file": "My Conflict Patch.esp", "output": "<scratch>\\My Conflict Patch.esp", "backup": false}}
]
EOF
xedit --json --edit --game sse --load "<Data>\Skyrim.esm" --load "<Data>\Update.esm" batch patch.json
```

- **`files new <name> [--light] [--medium]`** is xEdit's `AddNewFile`: the extension of the name is replaced (`.esl` with `--light`, `.esp` otherwise; `--medium` is Starfield), the plugin is made in the data folder with the slot after the last loaded file and the game master as its master, and it exists in memory only until `files.save` writes it — use it in a `batch` or `serve` session. `--dry-run` checks the name and reports path, load order and masters. Not ported: the module templates of xEdit's "Create New File" dialog.
- **`records copy <FormID> --to <patch>`** copies the version of the record that `--from` sees (the last loaded plugin by default). Without `--as-new` it is an override with the same FormID; when the target has the record already, the existing override is returned unchanged (`existed: true`) and nothing is copied over (the "...with overwriting" mode is not ported). With `--as-new` the record takes the next free FormID of the target and `--prefix`, `--suffix`, `--prefix-remove`, `--suffix-remove` change its editor ID. `--deep` copies the child group too: the references of a cell, the responses of a topic, the cells of a worldspace. The parents of the record come along as overrides without their contents (the worldspace and cell of a reference, the topic of a response).
- The copy reports `required_masters` and the `missing_masters` it adds to the target (sorted by load order). A required master that loads after the target fails with `The required master "X" can not be added to "Y" as it has a higher load order`; make the patch load after the plugins it copies from. Copying into a game master or the hardcoded file fails with `not_editable`.
- After the copies, `elements set` fixes the values (the edit runs the definition's `AfterSet`, so a GMST whose editor ID changes its first letter gets its `DATA` rebuilt), `masters clean` drops the masters the patch does not use, and `save` writes it.
- A patch that overrides records must load **after** every plugin it overrides, or its records lose. Verify the order the same way the audit sees it: load the whole order including the patch and run `conflicts --file "<patch>"`.

## Verifying a patch

1. `records get <FormID> --file "<patch>"` for every record it should hold, and `compare <FormID>` with the patch loaded last: the patch's column must be the winning one — `ctConflictWins`, or `ctIdenticalToMasterWinsConflict` where the patch forwards the master's values over other overrides.
2. `xedit check --file "<patch>"` for new errors (an unresolved FormID into a master the patch does not have shows here).
3. Load the saved patch in a fresh process and read a couple of records back: `save` then `records get`; `files list` shows its masters.
4. For a merged patch, `--dry-run` first and compare its `records` list with what the audit says is lost; the merge only covers its list of list-typed fields.

## The oracle

`cargo xtask parity merged` makes the scenario's plugins with the port, has the GUI create its merged patch from the same files (clicking the menu items that activate mod groups and answering its dialogs headlessly) and compares the patches byte for byte and the handler's log lines. Results (2026-10-09, in `docs/PLAN.md` under phase 4): 12 of 12 scenarios on Fallout New Vegas, Skyrim SE (with a mod group active too), Fallout 4 and Oblivion equal, the faulty ordered list's message included. The findings the check produced: `LoadOrderFileIDtoFileFileID` takes the last of two masters with one slot (the game master twice), the handler's messages carry the GUI's short record names, and the name suffixes of array entries are numbered lazily (the `sse-flst-orderedlist` edit sequence shows the FLST/editor-ID behaviour below).

## Gotchas

- **Form lists and the `OrderedList` suffix.** `wbFLSTEDIDAfterSet` drops the `FormIDs` of an `FLST` whose editor ID starts or stops ending in `OrderedList` — the editor ID change on a record resets its entries, in the oracle as here. A copy with `--prefix`/`--suffix` that moves the suffix on or off an ordered list therefore copies an empty list; copy first, rename after, or rename with the suffix in place.
- `records copy` reports the masters of the record itself, not of the records a `--deep` copy takes along (an upstream quirk): after a deep copy, read `files list` (and `save --dry-run`) before saving, and add a missing master with `masters add` if the deep records need one.
- The merged patch needs the plugins loaded in load order (the first loaded file is skipped, as in the handler); a patch made from a partial load order merges only what was loaded.
- Localized strings: a copy into a localized plugin assigns the strings as text, which become new strings of the target's tables (see `using-xedit-cli`); a merged patch is `.esp` and not localized.
- Conflict status and mod groups are not inputs to the merge; if the audit was run with mod groups active, the patch is still built from the raw overrides.
- Not ported: xEdit's "Create delta patch" (`mniNavCreateDeltaPatchClick`, `pending` in `coverage/ledger.toml`, the gap the xEdit-llm daemon covered as `patches.delta`) has no command here — build a delta as a `records copy` over the records that differ from the compared file.
