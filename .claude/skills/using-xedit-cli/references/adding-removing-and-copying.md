## Adding, removing and copying

These commands change the structure of a plugin as the navigation menu of xEdit does ("Add", "Remove", "Copy as override into...", "Copy as new record into...", "Deep copy as override into..."). Read the result back with `records get` before saving.

- `elements add` runs xEdit's `Add(name, silent)` on the record (or on the container at `--path`).

On a record, `name` is a member name or signature (`FULL`, `Model`); an existing member is returned, not doubled.

On a `CELL` the signature of a placed record (`REFR`, `ACHR`, `NAVM`, `LAND`, `PGRD`) adds a new record with a new FormID of the cell's plugin to the right child group, made when missing; `INFO` does the same for a `DIAL`, `ROAD` and `CELL[x,y]` or `CELL[P]` (persistent) for a `WRLD`, and `DLBR`, `DIAL` or `SCEN` for a Fallout 4 or later `QUST`.

A `LAND`, `PGRD`, `ROAD` or worldspace cell that a master has already is copied as an override instead, as upstream does.

On an array, any `name` adds an entry (a number adds at that position of a subrecord array).

The response has `element`, and `record` when a record was added.
- `elements remove` removes the element only when xEdit offers "Remove" for it (`IsRemovable`): not a required member, not the last entry of an array that must keep one, not the record header; else `not_removable`. Counters along the count paths of a removed array go to zero.
- `records copy` copies the version of the record that `--from` sees (the last loaded plugin by default) into `--to`.

Without `--as-new` the copy is an override with the same load order FormID; when `--to` has the record already, the existing override is returned unchanged (`existed: true`) and nothing is copied over unless `--overwrite` is given (xEdit's "... (with overwriting) into...", `aAllowOverwrite`), which copies the source over every record the target has, the child group's records too with `--deep`, and logs each as `Overwriting "..." with "..."`.

A source flagged as deleted is copied over with its deleted flag (`remove_deleted: false`, the default); `--remove-deleted` removes the target's record instead (`Removing "..."`, xEdit's "Yes to All" on that question). `overwrites` lists what the copy did to each record the target had.

With `--as-new` the record takes the next free FormID of the target (`NewFormID`, from `HEDR\Next Object ID`, which moves on) and `--prefix`, `--suffix`, `--prefix-remove`, `--suffix-remove` change its editor ID; a cell or the road of a worldspace can not be copied as new (`Can't copy record ... as new record.`). `--deep` copies the child group too (the references of a cell, the responses of a topic, the cells of a worldspace).

The parents of the record come along as overrides without their contents (the worldspace and cell of a reference, the topic of a response).

The response lists `required_masters` and the `missing_masters` the copy adds to the target (sorted by load order); a required master that loads after the target fails with `The required master "X" can not be added to "Y" as it has a higher load order`.

The masters are reported for the record itself, not for the records of a deep copy, as upstream does.
- A record copied into a game master or the hardcoded file fails with `not_editable` (xEdit does not edit the game master). A GMST whose editor ID changes its first letter gets its `DATA` reset by the definition's `AfterSet`, as in xEdit.
- `records delete` removes the record of `--file` (not the version another plugin sees) with its child group. The response has `child_records`. The file header can not be removed.
- `elements assign` is the view's drag and drop, the script functions `ElementAssign`, `AssignTemplateByName`/`AssignTemplateByIndex` and `TemplateAssign`: the element at `--path` (`--index this`, the default) takes the value of the source element, or a new entry is added (`--index add`), or the element sits at a position (`--index <n>`), which for a member of a union is the member's position and for an array entry the position `records compare` aligned.

The source is an element of another record (`--source-form-id` with `--source-file` and `--source-path`) or a template (`--template <name>`, which `elements templates` lists for the element and index), and `--only-sk` assigns the members of the sort key only.

An aligned position needs the view's alignment: run `records compare` on the record first (the oracle's `JumpTo` shows the record in the view, which aligns the arrays), which gives the entries their sort orders; without it a position appends.

A template makes a member of a union entry without data and sets an element made empty to its default, as the view does.

The response reports `can_assign`; an assign the definitions refuse changes nothing and gives `element: null` (upstream's `Assign` returns nil), and a template name the element does not offer fails with `unknown_template` (the message lists the names it has).
- `elements move` moves one entry of an unsorted array up or down (the view's Move Up and Move Down, the script's `MoveUp`/`MoveDown`), or to a position (`--position`, the script's `InsertElement` after `RemoveElement`). A sorted array does not move (`can_move_up`/`can_move_down` are false and nothing moves); the response always reports both. Moving a member of a structure to a position keeps its data only where the target position takes it, as upstream.
- A cell or worldspace a copy makes new in a file that lacks it becomes a partial form (header and editor ID only, not the contents) when the session runs with the global `--allow-make-partial` (xEdit's `-AllowMakePartial`) and the record can be partial (`GetCanBePartial`: a cell with a grid position can not). A copy over a record the target has already is never made partial (upstream's `lIsNew` stays false); a source that is itself a partial form is copied as one.

```
cat > copy.json <<'EOF'
[
  {"command": "records.copy", "params": {"form_id": "0001A332", "to": "MyPatch.esp"}},
  {"command": "elements.set", "params": {"form_id": "0001A332", "file": "MyPatch.esp", "path": "FULL", "value": "New name"}},
  {"command": "files.save", "params": {"file": "MyPatch.esp"}}
]
EOF
xedit --edit --game sse --load "<Data>\Skyrim.esm" --load "<Data>\MyPatch.esp" batch copy.json
```
