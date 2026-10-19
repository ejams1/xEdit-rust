## Corpus scripts to copy from

All in the 4.1.5q `Edit Scripts` folder (`XEDIT_SCRIPTS`; `xedit script list` shows the folder this build uses).

| Script | Shows |
|---|---|
| `_newscript_.pas` | the unit template: `Initialize`/`Process`/`Finalize`, `AddMessage('Processing: ' + FullPath(e))`, the non-zero `Result` comment. |
| `List loaded plugins and their masters.pas` | `Initialize`-only work over `FileCount`/`FileByIndex`/`MasterCount`/`MasterByIndex`/`Name`, self-terminating with `Result := 1`. Runs headless and is asserted by the port's tests. |
| `Apply filter for cleaning.pas` | setting the filter fields and calling `ApplyFilter` from `Initialize`; the port asserts its run. |
| `Check for errors.pas` | recursion over the element tree (`Check`, `ElementCount`, `ElementByIndex`), `StringOfChar` for indenting. |
| `Bookmark1.pas` | `Mode: Silent`, a `uses Bookmark` unit beside the script, `Finalize` doing the work; one of the two `equal` run cases of the phase-6 harness (it writes `Bookmark.ini` byte-identically to the oracle). |
| `Skyrim - Add shadow bias to lights.pas` | walking records, `SetElementEditValues`, saving changed plugins; the port asserts the saved `XLIG` bytes. |
| `JSON - Demo.pas` | the `JsonDataObjects` surface (parse, `S`/`I`/`F`/`B`/`A`/`O`/`V`, `ToJSON`). |
| `Find records.pas` | a form script: it compiles and runs headless up to `TForm.Create`, then refuses through the Vcl seam. |
| `Skyrim - Convert BOOK to SCRL.pas` | a conversion of records of one signature into another: `wbCopyElementToFile` of a base record as new, `SetElementEditValues` from the source, `RemoveNode` and `SetLoadOrderFormID`; a run case of the parity harness, equal to the oracle's saved `Skyrim.esm`. |
| `Fallout4 - Convert ALCH to SPEL.pas` | `Add` and `ElementAssign` over templates (`LowInteger`/`True`), the same case class; equal to the oracle's saved `Fallout4.esm`. |
| `Make new records from overrides.pas` | `LinksTo`, `CanContainFormIDs`, `ElementCount`/`ElementByIndex` recursion and `SetLoadOrderFormID`; equal to the oracle's saved `Update.esm`. |
