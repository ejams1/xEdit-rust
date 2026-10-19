## Checks

| Check | Oracle | Port |
| --- | --- | --- |
| Element dump | `xDump` text output | `xedit dump` |
| Round-trip save | input file bytes | load then save, compare bytes |
| Conflict status | `ConflictAllForMainRecord`/`ConflictThisForMainRecord` of every record by a `-script:` of the GUI (`oracle/conflicts.pas`) | `xedit conflicts` (`conflicts.list`) |
| Cleaning | plugin saved by `-quickautoclean` | `xedit clean` |
| Filter | `ApplyFilter` from the GUI's script mode (`oracle/filter.pas`), its message-log counts | `xedit filter apply` (`parity filter`) |
| Merged patch | the patch "Create Merged Patch" writes in the GUI's script mode (`oracle/merged.pas`) | `xedit patch merged` (`parity merged`) |
| Error checks | the message log of `-CheckForErrors` (the GUI) and of `xDump -check` | `xedit check` (`files.check`) and `xedit dump --check` |
| Saved bytes | plugin written by a `-script:` of the GUI (`oracle/save.pas`, `oracle/edit.pas`) | `xedit save`, `xedit batch` |
| Scripts | compile outcome of every script, and the files, log lines and saved plugins of the form-free corpus and the answered interactive cases (a case's `answers`), by a `-script:` run of the GUI (`parity script`) | `xedit script check`, `xedit script run`, `xedit script resume` |
| Archives | `BSArch.exe pack -mt:no`, `unpack` and `-dump` output | `bsarch`, `xedit archive pack` (`parity bsarch`) |
| Resources | the container list, resource lookups and loader lines of a `-script:` run of the GUI (`oracle/resources.pas`) or of a tool mode | the same command line of `xedit` (`parity resources`) |
| NIF and material dumps and saves | `Sniff.exe` JSON converter and universal tweaker; the GUI's script adapter for text dumps | `cargo xtask parity nif` (in process) |
| Sniff operations | `Sniff.exe` automation mode, outputs and log | `cargo xtask parity sniff` (in process) |
| LOD generation | the GUI's `-lodgen` mode: output files, export files and log | `xedit lodgen` (`parity lodgen`) |
