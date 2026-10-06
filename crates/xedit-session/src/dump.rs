// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xDump.dpr

//! The `dump` command: the element tree of a plugin as `xDump.exe` prints it
//! in its plain mode (no report, no sizes, no hidden elements), with the
//! summaries of the elements without a value. The lines end with CRLF as
//! the oracle writes them.

use std::io::Write;

use xedit_core::implementation::{FileImpl, wb_file};
use xedit_core::interface::globals::{
    GameMode, set_game_exe_name, set_game_master_esm, set_game_mode, set_game_name, set_hide_unused, set_simple_records,
};
use xedit_core::interface::{Container, ElementRef, FileStates, clear_record_defs};

/// Port of the game setup of `xDump.dpr` for the plugins of a game.
pub fn setup_game(game: &str) -> Result<GameMode, String> {
    let mode = match game.to_ascii_lowercase().as_str() {
        "fo4" => GameMode::gmFO4,
        "sse" => GameMode::gmSSE,
        "tes5" => GameMode::gmTES5,
        other => return Err(format!("unknown game {other}: use fo4, sse or tes5")),
    };
    set_simple_records(false);
    set_hide_unused(false);
    set_game_mode(mode);
    clear_record_defs();
    match mode {
        GameMode::gmFO4 => {
            set_game_name("Fallout4");
            set_game_exe_name("Fallout4");
            set_game_master_esm("Fallout4.esm");
            xedit_defs::fo4::define_fo4();
        }
        GameMode::gmSSE => {
            set_game_name("Skyrim");
            set_game_exe_name("SkyrimSE");
            set_game_master_esm("Skyrim.esm");
            xedit_defs::tes5::define_tes5();
        }
        GameMode::gmTES5 => {
            set_game_name("Skyrim");
            set_game_exe_name("TESV");
            set_game_master_esm("Skyrim.esm");
            xedit_defs::tes5::define_tes5();
        }
        _ => unreachable!(),
    }
    Ok(mode)
}

/// Loads the plugin and writes its dump.
pub fn dump_file(path: &str, out: &mut dyn Write) -> Result<(), String> {
    let file = wb_file(path, i32::MAX, FileStates::empty()).map_err(|error| error.to_string())?;
    write_container(&file, "", out).map_err(|error| error.to_string())
}

/// Port of `WriteContainer`.
fn write_container(container: &FileImpl, indent: &str, out: &mut dyn Write) -> std::io::Result<()> {
    write_elements(container, indent, out)
}

fn write_elements(container: &dyn Container, indent: &str, out: &mut dyn Write) -> std::io::Result<()> {
    for index in 0..container.get_element_count() {
        if let Some(element) = container.get_element(index) {
            write_element(&element, indent, out)?;
        }
    }
    Ok(())
}

/// Port of `WriteElement` in the plain dump mode.
fn write_element(element: &ElementRef, indent: &str, out: &mut dyn Write) -> std::io::Result<()> {
    if std::env::var_os("XEDIT_TRACE").is_some() {
        eprintln!("{indent}{}", element.get_name());
    }
    let name = element.get_display_name(true);
    let value = element.get_value();
    let summary = if value.is_empty() {
        element.get_summary()
    } else {
        String::new()
    };
    let mut indent = indent.to_owned();
    if element.get_name() != "Unused" && name != "Unused" {
        if !name.is_empty() {
            write!(out, "{indent}{name}")?;
        }
        if !name.is_empty() || !value.is_empty() {
            indent.push_str("  ");
        }
        if !name.starts_with("Hidden: ") {
            if !value.is_empty() {
                write!(out, ": {value}\r\n")?;
            } else if !name.is_empty() {
                if summary.is_empty() {
                    out.write_all(b"\r\n")?;
                } else {
                    write!(out, " [S]: {summary}\r\n")?;
                }
            }
        }
    }
    if let Some(container) = element.as_container()
        && !name.starts_with("Hidden: ")
    {
        write_elements(container, &indent, out)?;
    }
    Ok(())
}
