// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit.dpr (Application.Initialize, the title and icon,
// Application.CreateForm(TfrmMain) and Application.Run), xEdit/xeInit.pas
// (CheckForcedMode and DetectAppMode for the game the command line names)

//! The `xedit-gui` binary: the window, and the two switches the startup
//! flow takes from the command line instead of the game selection dialog
//! (`--game <tag>` and `--load <plugin>`).

use eframe::egui;
use xedit_gui::TfrmMain;
use xedit_gui::startup::GAME_MODES;

fn main() -> eframe::Result {
    let mut game = None;
    let mut plugins: Vec<String> = Vec::new();
    let mut edit = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--game" | "-game" => game = args.next(),
            "--load" | "-load" => {
                if let Some(plugin) = args.next() {
                    plugins.push(plugin);
                }
            }
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
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        "xEdit",
        options,
        Box::new(move |_cc| Ok(Box::new(TfrmMain::new(game, plugins, edit)))),
    )
}
