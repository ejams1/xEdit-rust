## Output

Without `--json` a command prints its result as pretty JSON and an error as `error: <code>: <message>` on stderr with exit code 1.

With `--json` it prints one envelope, `{"ok":true,"result":...}` or `{"ok":false,"error":{"code":"...","message":"..."}}`, always on stdout; `batch` puts one envelope per command in the `result` array (`{"ok", "command", "result" or "error"}`).

Error codes are stable: `no_session` (no `--game`), `unknown_file`, `ambiguous_file`, `unknown_mod_group`, `unknown_preset`, `unknown_record`, `unknown_element`, `invalid_params`, `load_failed`, `unknown_command`, `edit_required`, `edit_failed`, `not_editable`, `not_removable`, `save_refused`, `archive_failed`, `unsupported`, `io`, `internal`, and the localization codes `unknown_table`, `unknown_string`, `not_localized`, `invalid_state`, `unsaved_strings`, `translate_mode`, and `file_exists` (a new plugin whose name the data folder or the session has).

The progress log of a load and a save (`[Skyrim.esm] Loading file`, the string table encodings) goes to stderr, as xEdit's messages do. With several threads the messages of the workers arrive in no fixed order.
