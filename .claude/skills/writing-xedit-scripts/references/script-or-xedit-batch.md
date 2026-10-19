## Script or `xedit batch`?

Use `xedit batch` (see the `using-xedit-cli` skill) when the work is a list of commands you can name up front: a few `elements set`, `records copy`, `masters add`, then `save`. Use a script when the logic depends on the data:

- Walk every record of a load order and decide per record (filtering, checking, renaming with a condition, collecting totals).
- Keep state across records or recurse (`Initialize` state, `Finalize` writing a report; `Check for errors.pas` recurses the element tree).
- Write report or data files in a format you control (`wbOutputPath`).
- Run an existing script unchanged. The corpus scripts (and mod authors' scripts) are the compatibility surface; prefer running one over rewriting it as commands. `xedit script list` names the folder's scripts, and `XEDIT_SCRIPTS` points it at another folder.

A script is also the only way to exercise the script API itself, but it is not always the safer tool: a script that builds or shows a form cannot run headless (see "Headless and the Vcl seam"), and a script's failure saves nothing only when it aborts before its `SaveChanged` at the end. For a quick, reviewable change, commands are easier to dry-run and read back.
