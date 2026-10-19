## References

The reference index is xEdit's "Referenced By" information: for every record the FormIDs its elements refer to (`References`), and for every master record the records that refer to it or to one of its overrides (`ReferencedBy`).

The GUI builds it for every loaded file once they are loaded; the CLI builds it when a command first needs it (`refs get`, `formids change`, `formids renumber`) or when `refs build` runs, once per session, on all CPUs (`--threads`; the lists are the same for every count).

- `refs get <FormID>` reports `referenced_by` (the list of the master of the record, sorted by load order FormID and then by the load order of the referencing file, as xEdit's tab shows it; a record that refers through two FormIDs that resolve to the same record is listed twice, as in xEdit), `referenced_by_count`, `master`, and `references`: the FormIDs the record's own elements hold, as its file stores them, each with the record it resolves to. `--offset` and `--limit` page the list (a keyword can have tens of thousands of entries).
- Edits keep the index right as xEdit does: a changed record collects its references again, a new or copied record joins the lists of the records it refers to, a deleted record leaves them, and a record whose FormID changes hands its list to the override that becomes the master.
- **The reference cache.** Like xEdit, the build saves the references of every file with more than 500 records (or that took more than 2 seconds) to a cache file, and the next process loads them instead of building (`refs build` reports `loaded`, `built` or `built_and_saved` per file).

The cache folder is xEdit's: `<AppName>Edit Cache` in the data folder of the plugins (`SSEEdit Cache`, `FO4Edit Cache`), so by default the CLI writes into the game's `Data` folder.

Pass the global `--cache-path <folder>` to put it elsewhere, `--dont-cache-save` to write none, `--dont-cache-load` to read none and `--dont-cache` for neither.

A cache file is named after the CRC32 of the program, the plugin and its CRC32, the code pages and the language, so a changed plugin or another build of `xedit` never reads a stale file; xEdit's own files carry xEdit's CRC32 and are not read by the CLI, but the format is the same (renamed, each reads the other's).

Delete the folder to start over.
- **The referenced-by filter** (the filter panel above the tab) is `--filter-name` (the entry's name contains it, without case), `--filter-signature` and `--filter-file`, joined as xEdit does: the name and the signature with AND, or with OR when `--filter-or` is given (xEdit's And/Or switch), the file always AND. `referenced_by` then holds the matching entries (paged), `referenced_by_count` stays the count before the filter (xEdit's tab caption `Referenced by (N / F: M)`) and `filtered_count` is the filtered count.
- Morrowind has no reference information (xEdit builds none): `referenced_by` is always empty there. `refs build-reachable` is xEdit's "Build Reachable Info": it builds the references of the files that have none, then the reachable information of every file (the records a game reaches while it runs), which `filter apply --by-not-reachable-status` reads.

```
xedit --game sse --load "<Data>\Update.esm" --cache-path "<scratch>\cache" refs get 0001A332 --limit 20
xedit --json --game fo4 --load "<Data>\DLCRobot.esm" --dont-cache refs build
```
