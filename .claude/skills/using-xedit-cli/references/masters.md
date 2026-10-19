## Masters

The master commands change the master list of one plugin as the navigation menu of xEdit does ("Add Masters...", "Sort Masters", "Clean Masters"), and rewrite every FormID of the plugin to follow it (`MastersUpdated`): the FormIDs of the records, the FormIDs in their elements and the labels of the child groups. Load order FormIDs do not change, so `records get` finds the same records before and after.

- `masters add` takes file names of plugins that are loaded (`--load` them, before the plugin to change), skips the masters the plugin has already and the plugin itself, adds the rest in load order and then sorts all masters by load order (`--no-sort` keeps them at the end).

Only `.esm`, `.esp` and, where the game has light modules, `.esl` names are added; an `.esp` fails where the game forbids `.esp` masters.

Under Starfield the masters of every added master are added too (`wbEnforceAllMasters`).
- `masters clean` keeps a master when a FormID of the plugin points to it, when it is the game master, and under Starfield when it is a master of a used master. It logs `Removing unused master: <name>` per master.
- The response holds `old_masters`, `masters` (the list after the command, or the list it would leave for a dry run) and `changed`.
- Error code `edit_failed` carries the upstream message: `[AddMAddMastersIfMissingasters] Requested file to add is not loaded: "<name>"` (sic), `File "<name>" is not editable` (the game master, the hardcoded file), `Only N of M masters could be added. Master list now contains K entries and is full.`
- The update initialises every record of the plugin, as upstream does, so a saved plugin shows the changes an initialised record shows (sorted subrecords, required members, `AfterLoad` fixes) besides the master change.

```
cat > masters.json <<'EOF'
[
  {"command": "masters.add", "params": {"file": "MyPatch.esp", "masters": ["Dawnguard.esm"]}},
  {"command": "files.save", "params": {"file": "MyPatch.esp"}}
]
EOF
xedit --json --edit --game sse --load "<Data>\Dawnguard.esm" --load "<Data>\MyPatch.esp" batch masters.json
xedit --game sse --load "<Data>\MyPatch.esp" masters clean --dry-run
```
