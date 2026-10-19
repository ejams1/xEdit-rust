# Merged patch



`parity merged` runs the scenarios of `crates/xtask/oracle/merged/*.json` (`--file <stem>` selects some, `--game` the games).

A scenario names a game, the plugins to load and the name of the patch, and optionally mod group files with a settings file and the menu items that activate them, as the scenarios of `parity modgroups` do.

The GUI build of xEdit runs `crates/xtask/oracle/merged.pas` in its script mode: the script clicks the menu items and then "Create Merged Patch" (`mniNavCreateMergedPatch`), the harness answers the handler's dialogs (the warning of the games from Skyrim on, the file name `InputQuery` asks for), and the script writes the patch as the GUI saves it.

The port loads the same plugins in the order the GUI loaded them, runs the scenario's commands and `xedit patch merged <name>` with `save`, and the two patches are compared byte for byte, with the lines the handler logs for faulty ordered lists too.

The oracle's outputs are cached in `<cache>/<tag>/<MODE>-oracle-merged/<scenario>.<key>/`; the key hashes the plugins, the script and the scenario.

The report is `target/parity/merged.json`.

Results 2026-10-09: 12 of 12 scenarios equal: Fallout New Vegas vanilla (71 records merged) and with two plugins the port makes (74), Skyrim SE with those plugins (3) and with a mod group active (the same patch, 933 bytes), the ordered form lists (`sse-orderedlist`: the list merged with 4 entries where the winning override has 3, and the faulty one reported and left out), Fallout 4 (3), Oblivion (2), and the vanilla loads of the games with a header-only patch.

The gate found and the port follows: `LoadOrderFileIDtoFileFileID` takes the last of two masters with one slot (the merged patch lists the game master twice, an `UPSTREAM-QUIRK`), and the handler's messages carry the GUI's short record names (`wbDisplayShorterNames`, which the main form sets at its creation, is set around the merge).
