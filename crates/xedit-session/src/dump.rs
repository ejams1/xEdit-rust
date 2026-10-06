// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xDump.dpr

//! The `dump` command: the element tree of a plugin as `xDump.exe` prints it
//! in its plain mode (no report, no sizes, no hidden elements), with the
//! summaries of the elements without a value. The lines end with CRLF and
//! the text is in the Windows-1252 code page, as the oracle writes them.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, LazyLock};

use xedit_core::container_handler::{add_archive, add_folder, clear_containers};
use xedit_core::delphi::{change_file_ext, extract_file_path};
use xedit_core::implementation::{ElementImpl, FileBytes, FileImpl, game_master_file, wb_file, wb_file_compare};
use xedit_core::interface::globals::{
    GameMode, game_exe_name, game_master_esm, language, set_create_contained_in, set_data_path, set_game_exe_name,
    set_game_master_esm, set_game_mode, set_game_name, set_hide_unused, set_language, set_simple_records,
};
use xedit_core::interface::misc::{progress, set_progress_callback};
use xedit_core::interface::{
    Container, Element, ElementRef, File, FileState, FileStates, clear_record_defs, init_records,
};
use xedit_core::localization::{
    add_default_l_encodings_if_missing, add_l_encoding_if_missing, install_localization_handler, set_l_encoding_default,
};
use xedit_io::Encoding;

/// Sends the progress messages to stderr, like the log of xDump.
pub fn log_progress_to_stderr() {
    set_progress_callback(Some(Arc::new(|status: &str| eprintln!("{status}"))));
}

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
    // xDump turns the contained-in elements off for Fallout 4 and later.
    set_create_contained_in(!matches!(
        mode,
        GameMode::gmFO4 | GameMode::gmFO4VR | GameMode::gmFO76 | GameMode::gmSF1
    ));
    clear_record_defs();
    match mode {
        GameMode::gmFO4 => {
            set_game_name("Fallout4");
            set_game_exe_name("Fallout4.exe");
            set_game_master_esm("Fallout4.esm");
            xedit_defs::fo4::define_fo4();
        }
        GameMode::gmSSE => {
            set_game_name("Skyrim");
            set_game_exe_name("SkyrimSE.exe");
            set_game_master_esm("Skyrim.esm");
            xedit_defs::tes5::define_tes5();
        }
        GameMode::gmTES5 => {
            set_game_name("Skyrim");
            set_game_exe_name("TESV.exe");
            set_game_master_esm("Skyrim.esm");
            xedit_defs::tes5::define_tes5();
        }
        _ => unreachable!(),
    }
    init_records();
    setup_language(mode);
    Ok(mode)
}

/// Port of the language setup of `xDump.dpr`: the default language of the
/// game and the encodings of its string tables.
fn setup_language(mode: GameMode) {
    set_language(if mode == GameMode::gmFO4 { "En" } else { "English" });
    set_l_encoding_default(Encoding::Utf8, false);
    match mode {
        GameMode::gmSSE => add_l_encoding_if_missing("english", Encoding::Mbcs(1252), false),
        GameMode::gmFO4 => add_l_encoding_if_missing("en", Encoding::Mbcs(1252), false),
        _ => add_default_l_encodings_if_missing(false),
    }
    add_default_l_encodings_if_missing(true);
}

/// Port of the resource loading of `xDump.dpr`: when the plugin or one of
/// its masters is localized and a loose `.STRINGS` file is missing, the
/// archives of each master and of the plugin (`<name>`, `<name> - Interface`,
/// `<name> - Localization`, `<name> - Wwise*`) are added, then the data folder.
pub(crate) fn load_resources(file: &FileImpl, path: &str, mode: GameMode) {
    let data_path = extract_file_path(path);
    set_data_path(&data_path);
    clear_containers();
    let mut names: Vec<String> = file.masters().iter().map(|master| master.get_name()).collect();
    names.push(file.get_name());
    let is_localized = file.get_is_localized() || file.masters().iter().any(|master| master.get_is_localized());
    let load_archives = is_localized
        && names.iter().any(|name| {
            let strings = format!(
                "{data_path}Strings\\{}_{}.STRINGS",
                change_file_ext(name, ""),
                language()
            );
            !Path::new(&strings).is_file()
        });
    if load_archives {
        let extension = if mode == GameMode::gmFO4 { ".ba2" } else { ".bsa" };
        for name in &names {
            let stem = change_file_ext(name, "");
            for suffix in ["", " - Interface", " - Localization"] {
                let archive = format!("{data_path}{stem}{suffix}{extension}");
                if Path::new(&archive).is_file() {
                    add_resource_archive(&archive);
                }
            }
            // The Wwise archives match by prefix.
            let prefix = format!("{stem} - Wwise").to_ascii_lowercase();
            let mut wwise: Vec<String> = std::fs::read_dir(&data_path)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|file_name| {
                    let lower = file_name.to_ascii_lowercase();
                    lower.starts_with(&prefix) && lower.ends_with(extension)
                })
                .collect();
            wwise.sort();
            for file_name in wwise {
                add_resource_archive(&format!("{data_path}{file_name}"));
            }
        }
    }
    add_folder(Path::new(&data_path));
    install_localization_handler();
}

fn add_resource_archive(archive: &str) {
    progress(&format!(
        "[{}] Loading Resources.",
        xedit_core::delphi::path_file_name(archive)
    ));
    if let Err(error) = add_archive(Path::new(archive)) {
        progress(&error.to_string());
    }
}

/// Loads the plugin with its masters, its resources and the hardcoded
/// records, as `xDump.dpr` does before it starts the dump.
pub fn load_file(path: &str, mode: GameMode) -> Result<Arc<FileImpl>, String> {
    let file = wb_file(path, i32::MAX, FileStates::empty()).map_err(|error| error.to_string())?;
    load_resources(&file, path, mode);
    load_hardcoded(mode)?;
    Ok(file)
}

/// Loads the plugin and writes its dump.
pub fn dump_file(path: &str, mode: GameMode, out: &mut dyn Write) -> Result<(), String> {
    let file = load_file(path, mode)?;
    write_container(&file, out).map_err(|error| error.to_string())
}

/// Port of the hardcoded load of `xDump.dpr`: when the game master is
/// loaded, the embedded plugin of the hardcoded records loads in its place
/// under the name of the game executable.
pub(crate) fn load_hardcoded(mode: GameMode) -> Result<(), String> {
    if game_master_file().is_none() {
        return Ok(());
    }
    let Some(bytes) = xedit_defs::hardcoded::hardcoded_dat(mode) else {
        return Ok(());
    };
    let mut states = FileStates::empty();
    states.include(FileState::fsIsHardcoded);
    wb_file_compare(
        &game_exe_name(),
        0,
        Some(&game_master_esm()),
        states,
        FileBytes::Owned(bytes.to_vec()),
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Port of `WriteContainer`.
fn write_container(container: &FileImpl, out: &mut dyn Write) -> std::io::Result<()> {
    write_elements(container, 0, out)
}

/// Writes the elements of `container` at nesting `depth`.
fn write_elements(container: &dyn Container, depth: usize, out: &mut dyn Write) -> std::io::Result<()> {
    for index in 0..container.get_element_count() {
        if let Some(element) = container.get_element(index) {
            write_element(&element, depth, out)?;
        }
    }
    Ok(())
}

/// Writes the text as xDump does: through the ANSI code page of the
/// console output (Windows-1252), where a character without a mapping
/// becomes `?`.
fn write_text(out: &mut dyn Write, text: &str) -> std::io::Result<()> {
    if text.is_ascii() {
        out.write_all(text.as_bytes())
    } else {
        out.write_all(&Encoding::Mbcs(1252).get_bytes(text))
    }
}

/// Writes the two spaces of indentation per nesting level.
fn write_indent(out: &mut dyn Write, depth: usize) -> std::io::Result<()> {
    const SPACES: &[u8; 64] = &[b' '; 64];
    let mut remaining = depth * 2;
    while remaining > 0 {
        let count = remaining.min(SPACES.len());
        out.write_all(&SPACES[..count])?;
        remaining -= count;
    }
    Ok(())
}

/// Whether `XEDIT_TRACE` is set: every element name goes to stderr.
fn trace_enabled() -> bool {
    static TRACE: LazyLock<bool> = LazyLock::new(|| std::env::var_os("XEDIT_TRACE").is_some());
    *TRACE
}

/// Port of `WriteElement` in the plain dump mode. The name, the value and
/// the summary are written straight into `out`; the summary is only
/// computed when the line needs it.
fn write_element(element: &ElementRef, depth: usize, out: &mut dyn Write) -> std::io::Result<()> {
    if trace_enabled() {
        eprintln!("{:width$}{}", "", element.get_name(), width = depth * 2);
    }
    let name = element.get_display_name(true);
    let value = element.get_value();
    let mut child_depth = depth;
    if element.get_name() != "Unused" && name != "Unused" {
        if !name.is_empty() {
            write_indent(out, depth)?;
            write_text(out, &name)?;
        }
        if !name.is_empty() || !value.is_empty() {
            child_depth += 1;
        }
        if !name.starts_with("Hidden: ") {
            if !value.is_empty() {
                out.write_all(b": ")?;
                write_text(out, &value)?;
                out.write_all(b"\r\n")?;
            } else if !name.is_empty() {
                let summary = element.get_summary();
                if summary.is_empty() {
                    out.write_all(b"\r\n")?;
                } else {
                    out.write_all(b" [S]: ")?;
                    write_text(out, &summary)?;
                    out.write_all(b"\r\n")?;
                }
            }
        }
    }
    if let Some(container) = element.as_container()
        && !name.starts_with("Hidden: ")
    {
        write_elements(container, child_depth, out)?;
    }
    // `WriteContainer` holds an `IwbContainerElementRef` on the record while
    // it writes the elements; releasing it resets the record and frees the
    // subrecords, so the dump never holds more than one record tree.
    if let Some(record) = element.as_element_impl().and_then(ElementImpl::main_record_impl) {
        record.reset();
    }
    Ok(())
}
