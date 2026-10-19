## Examples

```
xedit --game sse --load "<Data>\Update.esm" records find --editor-id Dawnguard --signature MESG
xedit --game sse --load "<Data>\Update.esm" elements get 01003274 DNAM
xedit --game fo4 --load "<Data>\DLCRobot.esm" records list --signature NPC_ --limit 20
xedit --game sse --load "<Data>\Update.esm" elements set 01003274 DNAM\Flags 0000000000000001 --dry-run
xedit dump --game fo4 "<Data>\DLCworkshop01.esm" > DLCworkshop01.txt
xedit saves dump --game fo4 --data "<Data>" "<My Games>\Fallout4\Saves\Autosave1.fos" > save.txt
xedit --json --game sse --load "<Data>\Skyrim.esm" save --dry-run
xedit --json --edit --game sse --load "<Data>\Skyrim.esm" save --no-backup --output "<somewhere else>\Skyrim.esm"
```
