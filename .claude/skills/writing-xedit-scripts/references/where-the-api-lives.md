## Where the API lives

- **`xEditAPI.pas`** in the oracle's `Edit Scripts` is the declaration reference: 1,633 lines declaring 202 routines, the `IInterface` element type and the constants a script compiles against.

An agent writing a script should read the declarations there for signatures.

At run time the host stubs `xEditAPI` (as upstream does), so `uses xEditAPI` resolves to the host's registrations; the port's `script check` parses the real file when the folder holds one, because that file is part of the corpus gate.
- **`coverage/ledger.toml` is the source of truth for the ported surface.** Every registration is a `[script."<Unit>:<Class>.<Member>:<kind>"]` entry (`kind` is `function`, `const`, `get`, `set`, `iget`, `iset`, `idget`, `idset`, `class`, `rec`, `recget`, `recset`, `handler`, `extfun` or `extunit`) with `status = "covered"` (and `command = "script.run"`), `"presentation"`, `"excluded"`, or `"pending"`.

After parity #25: 3,663 entries — 3,579 covered, 54 presentation (the `TCanvas` members and the paint methods a renderer draws), 30 excluded (the registrations commented out upstream), none pending.

The last 27 (and four the ledger had wrongly covered by CLI commands: `AddRequiredElementMasters`, `ConflictAllForMainRecord`, `ConflictThisForMainRecord`, `ConflictAllForElements`) are `crates/xedit-session/src/script_host.rs` and `script_extfun.rs`; the test `every_covered_host_function_of_the_ledger_is_registered` holds the ledger and the run's adapter together.

A name no unit registers is the interpreter's `Undeclared Identifier '<name>'`. `excluded` is used for registrations commented out upstream; the VCL members are the headless object model's since parity #26, and only the `presentation` members wait for a renderer.
- Check the ledger before promising an agent-written script will run: grep the member name in `coverage/ledger.toml` and read its `status`.

The surface, by where it lives in the port:

| Unit(s) | Surface | Port |
|---|---|---|
| `xejviScriptAdapterHost.pas` | the file list (`FileCount`/`FileByIndex`/`FileByName`), the paths and game names, the message log (`AddMessage`, `ClearMessages`), `ApplyFilter`/`FilterApplied`/`RemoveFilter`, the atlas builders | `crates/xedit-session/src/script.rs` |
| `xejviScriptAdapterHost.pas` (parity #25) | the load options, `wbSettings`/`wbSettingsFileName`, `wbRecordDefMap`, the file and record lookups, the new files and their masters, `IsPositionChanged`, the conflict helpers, `JumpTo` and the selection, `ExecuteCaptureConsoleOutput`, the LOD generators and `wbGetUVRangeTexturesList`, `frmMain`/`frmFileSelect` | `crates/xedit-session/src/script_host.rs` (the forms in `script_vcl/interactive.rs`) |
| `JvInterpreter.pas` `external` routines | a function a script declares `external 'lib.dll' name 'F'` | `crates/xedit-session/src/script_extfun.rs` |
| `Element`, `Record`, `File`, `Container`, `Resource` | elements and their values (the `wbInterface` API), records and overrides, groups and signatures, FormID conversion, containers, `TwbAsset`, hashes, `TStrings`/`TList` fills | `crates/xedit-session/src/script_units/` |
| `xejviScriptAdapterDF.pas` | `TdfDef`/`TdfElement` over the NIF, material, LOD, FUZ and DDS code (`TwbNifFile`, `TwbBGSMFile`, ...): `NativeValues`/`EditValues`, `ToText`/`ToJSON`/`FromJSON`, `SaveToFile`, `LoadFromResource` | `crates/xedit-session/src/script_df.rs` |
| `xejviScriptAdapter.pas`, `Misc`, `MiscJson`, `MiscRegEx` | the `gm*`/`es*`/`dt*`/`ct*`/`res*`/`at*` constants, `Assigned`, `FullPathToFilename`, `GetRecordDefNames`, `wbFilterStrings`, `wbRemoveDuplicateStrings`, `wbVersionNumber`; `TStringList`/`TEncoding`; StrUtils/SysUtils/Math/Variants/WinAPI helpers; `JsonDataObjects` (`TJsonObject`/`TJsonArray`); `TPerlRegEx` | `script_adapter.rs`, `script_strings.rs`, `script_classes.rs`, `script_misc*.rs` |
| JVCL `JvInterpreter_System`, `_SysUtils`, `_Math`, `_Classes`, `JvInterpreterFm` | the RTL a script calls without `uses`: `IntToStr`/`Format`/`Pos`/`Pred`, `TStringList`/`TList`/`TStream`/`TFileStream`, `FormatDateTime`, `Math` | `crates/xedit-session/src/script_rtl/` |
| the ten VCL units and `xejviScriptAdapterMiscVcl.pas` | every form, dialog, control, graphics and ini class; the headless parts are the `Windows` records, the hint/cursor/accel helpers, the shortcut codec, `TIniFile`/`TMemIniFile`, `TDirectory`/`TFile` | `crates/xedit-session/src/script_vcl/` |

The tests are executable examples: `crates/xedit-session/tests/script.rs` (the run loop, `the_jvcl_rtl_units_run_a_script`, corpus runs), `script_adapters.rs`, `script_misc.rs`, `script_df.rs` (unit tests) and `script_vcl.rs`.
