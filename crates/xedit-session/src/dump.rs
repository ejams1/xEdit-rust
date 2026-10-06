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
use xedit_core::implementation::{
    ElementImpl, FileBytes, FileImpl, MainRecordImpl, game_master_file, trim_initialized_records, wb_file,
    wb_file_compare,
};
use xedit_core::interface::globals::{
    GameMode, game_exe_name, game_master_esm, game_name, language, set_create_contained_in, set_data_path,
    set_game_exe_name, set_game_master_esm, set_game_mode, set_game_name, set_hide_unused, set_language,
    set_simple_records,
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

/// The game modes `xDump.exe` can dump plugins of, with their switch: the
/// name of the game mode without `gm`. `gmTES4R` has no case in xDump.
const GAMES: [(&str, GameMode); 13] = [
    ("tes3", GameMode::gmTES3),
    ("tes4", GameMode::gmTES4),
    ("fo3", GameMode::gmFO3),
    ("fnv", GameMode::gmFNV),
    ("tes5", GameMode::gmTES5),
    ("enderal", GameMode::gmEnderal),
    ("fo4", GameMode::gmFO4),
    ("sse", GameMode::gmSSE),
    ("tes5vr", GameMode::gmTES5VR),
    ("enderalse", GameMode::gmEnderalSE),
    ("fo4vr", GameMode::gmFO4VR),
    ("fo76", GameMode::gmFO76),
    ("sf1", GameMode::gmSF1),
];

/// The tag of a game mode as `--game` takes it.
pub fn game_tag(mode: GameMode) -> &'static str {
    GAMES
        .iter()
        .find(|(_, game)| *game == mode)
        .map_or("unknown", |(tag, _)| tag)
}

/// Port of the game setup of `xDump.dpr` for the plugins of a game: the
/// names of the game, its executable and its master, and the definitions.
pub fn setup_game(game: &str) -> Result<GameMode, String> {
    let tag = game.to_ascii_lowercase();
    let Some(&(_, mode)) = GAMES.iter().find(|(name, _)| *name == tag) else {
        let tags: Vec<&str> = GAMES.iter().map(|(tag, _)| *tag).collect();
        return Err(format!("unknown game {game}: use one of {}", tags.join(", ")));
    };
    // `wbGameName`, `wbGameExeName` without `.exe` when it is not the game
    // name, and `wbGameMasterEsm` when it is not the game name plus `.esm`.
    let (game_name, exe_name, master_esm): (&str, Option<&str>, Option<&str>) = match mode {
        GameMode::gmTES3 => ("Morrowind", None, None),
        GameMode::gmTES4 => ("Oblivion", None, None),
        GameMode::gmFO3 => ("Fallout3", None, None),
        GameMode::gmFNV => ("FalloutNV", None, None),
        GameMode::gmTES5 => ("Skyrim", Some("TESV"), None),
        GameMode::gmEnderal => ("Enderal", Some("TESV"), Some("Skyrim.esm")),
        GameMode::gmTES5VR => ("Skyrim", Some("SkyrimVR"), None),
        GameMode::gmFO4 => ("Fallout4", None, None),
        GameMode::gmFO4VR => ("Fallout4", Some("Fallout4VR"), None),
        GameMode::gmSSE => ("Skyrim", Some("SkyrimSE"), None),
        GameMode::gmEnderalSE => ("Enderal", Some("SkyrimSE"), Some("Skyrim.esm")),
        GameMode::gmFO76 => ("Fallout76", None, Some("SeventySix.esm")),
        GameMode::gmSF1 => ("Starfield", None, None),
        GameMode::gmTES4R => unreachable!("not in GAMES"),
    };
    let define: fn() = match mode {
        GameMode::gmTES5 | GameMode::gmEnderal | GameMode::gmTES5VR | GameMode::gmSSE | GameMode::gmEnderalSE => {
            xedit_defs::tes5::define_tes5
        }
        GameMode::gmFO4 | GameMode::gmFO4VR => xedit_defs::fo4::define_fo4,
        GameMode::gmTES3 => xedit_defs::tes3::define_tes3,
        GameMode::gmTES4 => xedit_defs::tes4::define_tes4,
        GameMode::gmFO3 => xedit_defs::fo3::define_fo3,
        GameMode::gmFNV => xedit_defs::fnv::define_fnv,
        GameMode::gmFO76 => xedit_defs::fo76::define_fo76,
        _ => return Err(format!("the definitions of {tag} are not ported yet")),
    };
    set_simple_records(false);
    set_hide_unused(false);
    set_game_mode(mode);
    // xDump turns the contained-in elements off for Fallout 4 and later.
    set_create_contained_in(!matches!(
        mode,
        GameMode::gmFO4 | GameMode::gmFO4VR | GameMode::gmFO76 | GameMode::gmSF1
    ));
    set_game_name(game_name);
    set_game_exe_name(&format!("{}.exe", exe_name.unwrap_or(game_name)));
    set_game_master_esm(&master_esm.map_or_else(|| format!("{game_name}.esm"), str::to_owned));
    clear_record_defs();
    define();
    init_records();
    setup_language(mode);
    Ok(mode)
}

/// Whether the archives of the game are BA2 files (`wbArchiveExtension`).
fn uses_ba2(mode: GameMode) -> bool {
    matches!(
        mode,
        GameMode::gmFO4 | GameMode::gmFO4VR | GameMode::gmFO76 | GameMode::gmSF1
    )
}

/// Port of the language setup of `xDump.dpr`: the default language of the
/// game and the encodings of its string tables. The language in the game's
/// INI files is not read.
fn setup_language(mode: GameMode) {
    set_language(if uses_ba2(mode) { "En" } else { "English" });
    if mode <= GameMode::gmEnderal {
        add_default_l_encodings_if_missing(false);
    } else {
        set_l_encoding_default(Encoding::Utf8, false);
        match mode {
            GameMode::gmSSE | GameMode::gmTES5VR | GameMode::gmEnderalSE => {
                add_l_encoding_if_missing("english", Encoding::Mbcs(1252), false)
            }
            _ => add_l_encoding_if_missing("en", Encoding::Mbcs(1252), false),
        }
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
        let extension = if uses_ba2(mode) { ".ba2" } else { ".bsa" };
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
    load_hardcoded()?;
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
pub(crate) fn load_hardcoded() -> Result<(), String> {
    if game_master_file().is_none() {
        return Ok(());
    }
    let Some(bytes) = xedit_defs::hardcoded::hardcoded_dat(&game_name()) else {
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
    write_elements(container, 0, None, out)
}

/// Writes the elements of `container` at nesting `depth`. `record` is the
/// main record the elements belong to.
fn write_elements(
    container: &dyn Container,
    depth: usize,
    record: Option<&Arc<MainRecordImpl>>,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    for index in 0..container.get_element_count() {
        if let Some(element) = container.get_element(index) {
            write_element(&element, depth, record, out)?;
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
fn write_element(
    element: &ElementRef,
    depth: usize,
    record: Option<&Arc<MainRecordImpl>>,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let own_record = element.as_element_impl().and_then(ElementImpl::main_record_impl);
    let record = own_record.as_ref().or(record);
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
    // The other records the callbacks of this line built, such as the
    // navmeshes the edges of a navmesh link to, are released by Delphi when
    // the callback lets go of them. The port resets them once more than
    // `KEPT_RECORDS` are built; the record being written stays.
    trim_initialized_records(KEPT_RECORDS, record);
    if let Some(container) = element.as_container()
        && !name.starts_with("Hidden: ")
    {
        write_elements(container, child_depth, record, out)?;
    }
    // `WriteContainer` holds an `IwbContainerElementRef` on the record while
    // it writes the elements; releasing it resets the record and frees the
    // subrecords, so the dump never holds more than one record tree.
    if let Some(own_record) = &own_record {
        own_record.reset();
    }
    Ok(())
}

/// The number of other records whose subrecords stay built while the dump
/// goes on, so that the records the callbacks read often are not built
/// again for every line.
///
/// Measured on `DLCCoast.esm`: 64 records peak at 1.3 GB in 78 s, 256 at
/// 1.6 GB in 69 s, 1024 at 2.1 GB in 67 s.
const KEPT_RECORDS: usize = 256;
