## Checking for errors

`check` is xEdit's "Check for Errors" (`files.check`).

It walks every element below each checked node in order and asks it for `Check`: an unresolved FormID (`[01000ABC] <Error: Could not be resolved>`), a FormID of a record type the field does not take (`Found a GMST reference, expected: ARMO,LVLI`), a `NULL` where none is allowed, an enum or flag value without a name (`<Unknown: 2 $2>`), missing required members, a deleted record or partial form that still has data, a new record in an update module, an object ID beyond a light (`FFF`) or medium module's range, a FormID that differs from its fixed FormID (`HITME`), data shorter than its definition, and the checks of the game's definitions (the story manager of a quest, the variables of a script, the face dials of a Starfield NPC and so on).

- Without `--file` and `--record` it checks the plugins given with `--load`; `--last` checks the last file of the load order as the `-CheckForErrors` mode does (with only a game master loaded that is the file of the hardcoded records, `[00] <game>.exe`).
- `xedit check` loads the plugins as xEdit's `-CheckForErrors` mode does: no internal edits of the load (`wbAllowInternalEdit` off; the records still get the fixes of their init, because the GUI checks only where editing is allowed) and no reference information (`wbBuildRefs` off, so the story manager check of a quest is skipped). Through `call files.check`, `batch`, `serve` or `mcp` a session checks as the edit mode's menu item does: the reference index is built first.
- Text output: `Start: Checking for Errors`, `Checking for Errors in <node>`, then per record with errors its name and a line `    <path> -> <error>` per error (the path starts at the record's signature: `OTFT \ INAM - Items \ Item`), and `Done: Checking for Errors, Processed Records: N, Errors found: M` (records with errors).

The messages a record logs while it is built (`Error: record STAT contains unexpected (or out of order) subrecord ...`, `Errors were found in: ...`, `Contained subrecords: ...`) come where xEdit logs them, twice, as xEdit builds a record twice during the check. `--json` gives `checked`, `errors_found`, `exit_code` (the errors found, at most 127, the exit code of `-CheckForErrors`), `records` (name, FormID, signature, file and the errors of each) and `messages` (the lines above).

The command reads; it changes nothing and needs no `--edit`.
- The records are checked on all CPUs with the same output as on one thread.

```
xedit --game sse --load "<Data>\Dawnguard.esm" check
xedit --json --game fo4 --load "<Data>\DLCRobot.esm" check --record 01000F99
```
