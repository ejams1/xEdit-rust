## Editing an element

`elements set` runs `SetEditValue`, `SetNativeValue` or `SetToDefault` on the element at the path, as a script's `SetEditValue` does, so the `AfterSet` callbacks of the definition run (a GMST's `DATA` is rebuilt when the first letter of its editor ID changes, a MGEF's actor values follow its archetype, and so on). The response holds the element before (`old`) and after (`new`), `changed`, and `added` when the element was created.

- Edit values are the texts xEdit shows in its editor: `1.5` for a float, `Dawnguard "Dawnguard" [QUST:0200C97A]` or `0200C97A` for a FormID (load order FormIDs, as `--game` loads the plugins), a name or a number for an enum, `0000000000000001` for flags. A flag can also be set by its name as the last element of the path: `ACBS\Flags\Female` with `1` or `0`.
- `--native` takes a JSON number or boolean and sets it as the native value; `--default` sets the element to the default of its definition.
- A path whose last element is missing adds that member when the record's definition has it (`ElementEditValues`): `elements set 01003274 SNAM "text"` on a record without `SNAM`.
- Loading runs the fix-ups xEdit runs on load (`wbAllowInternalEdit`): a record that lacks a required subrecord gets it, a worldspace loses its offset data, and the `AfterLoad` callbacks of the definitions apply their corrections. Those records are modified internally and are written from their elements on save.
- Error codes: `edit_failed` carries the message of the upstream exception (`"x" is not a valid character for a flag`, `Not a valid GUID: ...`, `FormID [...] can not be mapped to file FormID for file "..."`), `not_editable` a dry run on an element the editor would not let you change.
- A localized string of a localized plugin takes only a `STRINGID:<hex>` text (an existing string ID); a new text fails with `Can not assign to a localized string` because the string tables are not written yet.
