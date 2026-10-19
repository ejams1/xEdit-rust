// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit.dpr (Application.Initialize, the title and icon,
// Application.CreateForm(TfrmMain) and Application.Run), xEdit/xeInit.pas
// (CheckForcedMode and DetectAppMode for the game the command line names)

//! The `xedit-gui` binary: the window, and the two switches the startup
//! flow takes from the command line instead of the game selection dialog
//! (`--game <tag>` and `--load <plugin>`).

use std::io::Write;

use eframe::egui;
use xedit_gui::TfrmMain;
use xedit_gui::startup::GAME_MODES;

/// `XEDIT_GUI_LOG=<path>`: the shutdown, panic and startup lines of a
/// headless screen-check run, which the private desktop otherwise hides.
pub fn log_line(message: &str) {
    let Ok(path) = std::env::var("XEDIT_GUI_LOG") else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
    }
}

fn main() -> eframe::Result {
    log_line("xedit-gui starting");
    // The screen-check harness's private desktop reports a system scale
    // whose window sizing and surface disagree; DPI unaware makes it a
    // 96 DPI program, as the desktop's own surfaces are.
    #[cfg(windows)]
    if std::env::var_os("XEDIT_GUI_DPI_UNAWARE").is_some() {
        // SAFETY: a process-wide DPI setting, called before any window.
        unsafe {
            windows_sys::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
                windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_UNAWARE,
            );
        }
    }
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_line(&format!("panic: {info}"));
        previous_hook(info);
    }));
    let mut game = None;
    let mut plugins: Vec<String> = Vec::new();
    let mut edit = true;
    let mut dialog: Option<String> = None;
    let mut dialog_arg: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--game" | "-game" => game = args.next(),
            "--load" | "-load" => {
                if let Some(plugin) = args.next() {
                    plugins.push(plugin);
                }
            }
            // The screen-check harness opens one dialog of step 5 after the
            // load instead of walking the menus (upstream has no Win32
            // menu to drive).
            "--dialog" => dialog = args.next(),
            "--dialog-arg" => dialog_arg = args.next(),
            "--view" => edit = false,
            "--edit" => edit = true,
            "--version" => {
                println!("xedit-gui {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("xedit-gui: the xEdit graphical editor");
                println!("  --game <tag>   the game mode (fo4, sse, tes5, ...); the selection dialog without it");
                println!("  --load <file>  a plugin to load (repeatable); the module selection dialog without any");
                println!("  --dialog <name>  open a dialog after the load: filter, modgroups, modgroup-edit, files");
                println!("  --dialog-arg <name>  the mod group the edit dialog opens for");
                println!("  --view         open without the edit gate");
                println!("  --version      print the version and exit");
                return Ok(());
            }
            other => {
                eprintln!("xedit-gui: unknown argument {other} (see --help)");
                std::process::exit(2);
            }
        }
    }
    xedit_core::interface::globals::set_program_path(
        &std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|folder| format!("{}\\", folder.display())))
            .unwrap_or_default(),
    );
    let game = match game.as_deref() {
        None => None,
        Some(tag) => match GAME_MODES
            .iter()
            .find(|(name, _, supported)| *name == tag && *supported)
        {
            Some((_, mode, _)) => Some(*mode),
            None => {
                let tags: Vec<&str> = GAME_MODES.iter().map(|(name, _, _)| *name).collect();
                eprintln!(
                    "xedit-gui: {tag} is not a game mode this build loads: use one of {}",
                    tags.join(", ")
                );
                std::process::exit(2);
            }
        },
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("xEdit")
            .with_inner_size([1370.0, 663.0])
            .with_min_inner_size([640.0, 400.0])
            .with_clamp_size_to_monitor_size(true),
        ..Default::default()
    };
    let result = eframe::run_native(
        "xEdit",
        options,
        Box::new(move |_cc| {
            let mut form = TfrmMain::new(game, plugins, edit);
            form.pending_dialog = dialog;
            form.dialog_arg = dialog_arg;
            Ok(Box::new(form))
        }),
    );
    if let Err(error) = &result {
        log_line(&format!("eframe::run_native failed: {error}"));
    }
    log_line("xedit-gui stopped");
    result
}
