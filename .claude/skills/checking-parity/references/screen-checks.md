## Screen checks (`cargo xtask gui`)

The phase-7 gate witnesses a GUI claim with a screenshot or a scripted screen interaction against the 4.1.5q binary. `cargo xtask gui` starts a program on a private desktop of the current window station, which is never shown: nothing it draws reaches the user's desktop. A **session** is a JSON file; a helper process (`gui session`, started by `launch` and run from a copy of `xtask`) creates the desktop, starts the program on it, holds the desktop open and serves the other commands through request files beside the session. Windows belong to their desktop, so every list, capture, key, click, menu and close request is executed by that helper, not by the command's own process.

| Command | What it does |
| --- | --- |
| `gui launch <exe> [--args <arg>]... [--desktop <name>] [--session <file>] [--wait <seconds>] [--json]` | Starts the program, waits for its first visible window, writes the session (default `$XEDIT_GUI_SESSIONS`, else `$XEDIT_PARITY_SCRATCH/gui`, else `<cwd>/.xtask-gui/session.json`). A program that exits during startup fails the launch with its exit code. |
| `gui windows <session> [--json]` | Every top-level window of the desktop: handle, pid, class, title, rect, visible, and whether it belongs to the session's program. |
| `gui shot <session> (--title <substr>\|--handle <hwnd>)... --out <png> [--json]` | Captures every matching window with `PrintWindow` (BitBlt fallback), writes PNGs. Several matches write `<stem>-<handle>.png`. |
| `gui keys <session> (--title\|--handle) --keys "ctrl+s,enter,text:FO4" [--control <class>] [--delay <ms>]` | Posts keys with `WM_KEYDOWN`/`WM_CHAR`/`WM_KEYUP`, modifiers in the target thread's keyboard state. `alt+f` sends `SC_KEYMENU` (a posted Alt+letter never opens a menu). `--control` names the control class that receives the keys (`TLabeledEdit`, `TVirtualStringTree`): **a dialog on a private desktop has no focused control**, so without it the keys go to the form and text is lost. |
| `gui click <session> (--title\|--handle) --at <x>,<y> [--button left\|right\|middle]` | Posts a mouse click at a client point, passed down to the deepest child under it. |
| `gui click <session> (--title\|--handle) --click-button <caption>` | Finds a `TButton`/`Button`/`TSpeedButton` child by caption (an `&` is ignored) and sends `BM_CLICK`. Use it where the dialog is DPI-clipped and the button is outside the visible client area. |
| `gui menus <session> (--title\|--handle) [--json]` | The window's Win32 menu tree (`GetMenu`/`GetMenuItemInfoW`): text, id, enabled, checked, nested submenus. |
| `gui menupopup <session> --handle <hwnd> [--json]` | An open popup menu (a window of the class `#32768`): text, id, enabled, checked, in order. A right-click that opens a context menu creates one; `gui windows` finds it. |
| `gui children <session> (--title <substr>\|--handle <hwnd>) [--json]` | The child windows of one window (`EnumChildWindows`): class, text, rect, visible. The layout of a program that draws its own controls (the 4.1.5q form) is only visible in its children. |
| `gui resize <session> (--title\|--handle) (--size <w>x<h>\|--at <x>,<y>) [--json]` | Moves and resizes a window (`SetWindowPos`). A window that is too small for its own layout (the 4.1.5q form is cut off at the bottom of the screen, where its tab strip is) has to grow before a screen check can reach the controls at its edge. |
| `gui close <session> [--timeout <seconds>]` | `WM_CLOSE` to the program's windows, then `TerminateProcess` on exactly that pid (checked by its creation time), then ends the helper and removes the session. |

Notes paid for on the first run (2026-10-19):

- The helper runs from a **copy** of `xtask` beside the session, so a rebuild while a session runs is safe, but a command whose protocol changed needs a **new session** (close and launch again).
- `xEdit 4.1.5q` has **no Win32 menu**: its File/View/... menu is `pmuMain`, a popup menu on the `bnMainMenu` hamburger speed button (`Caption = #926`), a `TGraphicControl` with no window handle. `gui menus` therefore answers `has no menu` for it, and `GetMenu` cannot be used for the walkthrough. The button is disabled until a file is loaded; the gate has to open the popup from the main form after loading and capture the `#32768` menu window.
- At 150 % display scaling the 4.1.5q dialogs are clipped: their OK/Cancel buttons lie outside the visible client area, so click them with `--click-button`, not `--at`.
- `C:\Windows\System32\notepad.exe` on Windows 11 is the packaged Notepad (a hardlink into `WindowsApps`): launched directly it exits with code 0 without a window (`launch` reports `the program exited during startup`). Use a classic Win32 target (`dxdiag.exe`, `charmap.exe`) for a screen-check smoke test on this machine.

Notes paid for at the phase-7 gate (2026-10-21), on the 4.1.5q binary at 200 % display scaling:

- **The capture is a quarter of the form.** The Delphi build is DPI aware, so its window is twice the size in pixels that the helper (DPI unaware) sees, and `gui shot` sizes its bitmap from the helper's rectangle: the PNG is the top left quarter of the form at one pixel per physical pixel. A PNG pixel is therefore **two** client units (the helper's coordinates are the form's own units), and the part of the form past the PNG's edge is not in the capture at all. `gui resize` grows the window, which moves what the quarter shows but never widens it.
- **The layout is in the children, not in the picture.** `gui children` lists every child window of the form with its class, rect and visibility, which is where a control's position is found when it is outside the capture. A right-click at a child rect's top edge is how the column-header popups (`pmuNavHeaderPopup`, the nav tree's header) are reached.
- **A popup menu is a window of its own.** A right-click that opens a context menu creates a `#32768` window: `gui windows` finds its handle, `gui menupopup` reads its items with their ids, and `gui shot --handle` captures the menu itself, which is the evidence the walkthrough record keeps. `gui keys --keys escape` closes it.
- **A stale popup reads as a menu.** `gui menupopup` answers for whatever `#32768` window is open, so a popup that was not dismissed is read again on the next probe. Send `escape` before every probe and check the handle.
- **Not every popup of the release can be opened.** The view tree of 4.1.5q has no header control at runtime (a right-click on its top edge opens the view's own popup), the `Referenced By` page stays hidden until the loader has reference data, and one popup holds no item at all. The walkthrough then rests on the `.dfm` of the release, which is the build's own menu definition: `tools/menu_diff.py` diffs its ten popup menus against `crates/xedit-gui/src/menu_model.rs` (caption tree, nesting, `Enabled`, `Checked`, `RadioItem`, `Visible`) and exits non-zero on a difference.

Worked example (the oracle's module selection, evidence in `M:\projects\scratch\p7-screen-evidence`):

```
W='M:\projects\scratch\p7-screen-evidence\oracle-private'   # Data\, plugins.txt, temp\, cache\, backup\, mygames\
cargo xtask gui launch "$XEDIT_ORACLE_DIR/xFOEdit.exe" \
    --args -FO4 --args "-D:$W\Data\\" --args "-P:$W\plugins.txt" --args "-T:$W\temp\\" \
    --args "-C:$W\cache\\" --args "-B:$W\backup\\" --args "-S:$W\\" \
    --args "-M:$W\mygames\\" --args "-I:$W\mygames\game.ini" --args "-G:$W\mygames\Saves\\" \
    --session M:/projects/scratch/p7-screen-evidence/fo4edit-session.json --desktop xedit-gui-fo4 --wait 40
cargo xtask gui windows M:/projects/scratch/p7-screen-evidence/fo4edit-session.json   # TfrmMain + TfrmModuleSelect
cargo xtask gui click  M:/projects/scratch/p7-screen-evidence/fo4edit-session.json --title "What's New" --at 820,600
cargo xtask gui shot   M:/projects/scratch/p7-screen-evidence/fo4edit-session.json --title "Module Selection" --out module-selection.png
cargo xtask gui keys   M:/projects/scratch/p7-screen-evidence/fo4edit-session.json --title "Module Selection" \
                       --control TLabeledEdit --keys "text:FO4"           # the filter box shows FO4
cargo xtask gui close  M:/projects/scratch/p7-screen-evidence/fo4edit-session.json
```

`--json` prints the same result as the text output as JSON on every command.

### Capturing the Rust GUI (`xedit-gui --screenshot`)

Our GUI paints through eframe/wgpu, and on the private desktop `gui shot` cannot witness it: the window is hidden and has no composited surface, so `PrintWindow` and the `BitBlt` fallback both return a black client area. The app captures itself instead, through egui's own screenshot path (`ViewportCommand::Screenshot`, which renders the frame to a capture texture and reads it back to the CPU even when the window is hidden):

```
cargo xtask gui launch M:\projects\scratch\xedit-p7-target\debug\xedit-gui.exe \
    --args --game --args sse --args --load --args Update.esm \
    --args --screenshot --args 'M:\projects\scratch\p7-shot-evidence\sse-main.png' --args --exit \
    --session M:/projects/scratch/p7-shot-evidence/sse-main-session.json --desktop xedit-shot-sse --wait 90
cargo xtask gui close M:/projects/scratch/p7-shot-evidence/sse-main-session.json
```

- `--screenshot <path>` writes one PNG of the window. It waits for the screen the other switches name: with `--load`, until the load finishes; with `--dialog <name>`, the dialog is opened as part of that same finish, so it is in the capture; with neither, the game or module selection dialog. It also captures the `Fatal:` screen of a failed load.
- `--screenshot-frames <n>` (default 2) is the number of painted frames to wait after the screen is ready, and `--screenshot-delay <ms>` a further wall-clock wait, for content that arrives on a worker thread.
- `--exit` closes the window once the PNG is written, so a test cannot hang; a screen that is not ready after 180 s closes without a PNG (the `XEDIT_GUI_LOG` line says why). The exit costs one `ViewportCommand::Close`; settings are saved on the close request as usual.
- The capture is the app's own pixels, so it proves what egui laid out, not what a Win32 compositor would show. Use `gui windows`/`gui keys`/`gui close` against the process as normal; only `gui shot` is replaced. `xtask gui shot` stays the right tool for the 4.1.5q Delphi binary and its GDI dialogs.
- A blank or black PNG means the app's frame was not painted before the read-back (or the GPU adapter refused the capture): check `XEDIT_GUI_LOG` for `screenshot: wrote <path>` and `screenshot: could not write ...`. The file only appears once the callback has run.
- On this machine the loaded screen opens the upstream developer message (step 6's `startup_dialogs`) on the first ready frame: the SSE settings file does not exist and FO4's has `[DeveloperMessage] Version=0`. The `--dialog <step-5 name>` runs suppress it (their dialog opens inside `finish_load`), and an unknown `--dialog main` opens nothing and leaves the bare shell; capture both states when the gate needs the shell without a modal. `sse-main-developer-message.png` and `sse-main.png` in the phase-7 evidence are the two. A step-6 `--dialog` name (`tip`, `script`, `rich-edit`, `view-elements`, `log-analyzer`, `localization`, `localize-plugin`) is capturable alone since step 6's ordering was fixed: the dialog-only path opens its own dialog and leaves the developer message behind it. The phase-7 evidence holds the nineteen dialog views (`port-dialog-*.png`).

Idle rule that the capture work exposed: eframe 0.37 calls `App::ui` only when a repaint is requested, so an idle window costs no CPU — but every `send_viewport_cmd` requests a repaint of its viewport. Sending the window title/icon per frame repainted the loaded window continuously at a full core on the private desktop; they are now sent once. When adding a viewport command, send it on change, not per pass, and schedule `request_repaint_after` only while an animation or a job is in flight. A hidden window runs `App::logic` only; `--screenshot` polls the load and its own state there, and asks for painted frames with `ViewportCommand::RequestPaintWhileHidden`.
