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

use rayon::prelude::*;

use xedit_core::delphi::extract_file_path;
use xedit_core::implementation::{
    ElementImpl, FileBytes, FileImpl, MainRecordImpl, game_master_file, pin_record, trim_initialized_records, wb_file,
    wb_file_compare,
};
use xedit_core::interface::globals::{
    GameMode, ToolSource, chapters_to_skip_contains, clear_resources_loaded_handlers, dump_chapters, game_exe_name,
    game_master_esm, game_name, set_create_contained_in, set_data_path, set_game_exe_name, set_game_master_esm,
    set_game_mode, set_game_name, set_hide_unused, set_language, set_simple_records, set_tool_source,
    signature_list_contains,
};
use xedit_core::interface::misc::set_progress_callback;
use xedit_core::interface::types::{DefFlag, Signature};
use xedit_core::interface::{Container, Def, ElementRef, FileState, FileStates, clear_record_defs, init_records};
use xedit_core::localization::{add_default_l_encodings_if_missing, add_l_encoding_if_missing, set_l_encoding_default};
use xedit_core::threads::{self, init_cycles};
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
    setup(game, DefinitionSource::Plugins, false, None)
}

/// The game setup of a dump run with `-sr`, which `xDump.dpr` applies to
/// `wbSimpleRecords` before it builds the definitions.
pub fn setup_game_with_simple_records(game: &str, simple_records: bool) -> Result<GameMode, String> {
    setup(game, DefinitionSource::Plugins, false, Some(simple_records))
}

/// The source of the definitions a game is set up with (`xDump.dpr` chooses
/// it from `-saves` and `-plugins`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DefinitionSource<'a> {
    /// The plugins of the game (`DefineFO4` and the like).
    Plugins,
    /// The save definitions (`DefineFO4Saves` and the like), with the path
    /// of the save when there is one: its extension picks the co-save
    /// definitions. `None` is the export of the save definitions, which
    /// reads no file.
    Saves(Option<&'a str>),
}

/// Port of the game setup of `xDump.dpr -saves` for the *definitions* of
/// the saves of a game without a save file (`xDump.exe -<GAME> -saves
/// -export`).
pub fn setup_game_for_save_definitions(game: &str) -> Result<GameMode, String> {
    setup(game, DefinitionSource::Saves(None), false, None)
}

/// The game setup of the editor (`xeInit.pas`) for a session: as
/// [`setup_game`], with the settings the GUI reads before it builds the
/// definitions and that xDump changes: simple records (`wbSimpleRecords`,
/// the GUI's default), the unused fields hidden (`wbHideUnused`) and the
/// contained-in elements of the records in a worldspace, cell or topic
/// group (`wbCreateContainedIn`, off for Morrowind only).
pub fn setup_game_for_edit(game: &str) -> Result<GameMode, String> {
    setup(game, DefinitionSource::Plugins, true, None)
}

/// Port of the game setup of `xDump.dpr -saves` for a save or co-save of a
/// game: the save definitions, switched to the co-save ones for the
/// extension of the script extender.
pub fn setup_saves(game: &str, path: &str) -> Result<GameMode, String> {
    setup(game, DefinitionSource::Saves(Some(path)), false, None)
}

/// The save definitions of a game.
struct SaveDefinitions {
    /// The definitions of the saves.
    define: fn(),
    /// The switch to the definitions of the co-saves.
    switch_to_co_save: fn(),
    /// The extension of the co-saves of the script extender.
    co_save_extension: &'static str,
}

/// The save definitions of a game, `None` for a game without saves.
fn save_definitions(mode: GameMode) -> Option<SaveDefinitions> {
    use xedit_defs::callbacks::fnvsaves::{define_fnv_saves, switch_to_fnv_co_save};
    use xedit_defs::callbacks::fo3saves::{define_fo3_saves, switch_to_fo3_co_save};
    use xedit_defs::callbacks::fo4saves::{define_fo4_saves, switch_to_fo4_co_save};
    use xedit_defs::callbacks::tes4saves::{define_tes4_saves, switch_to_tes4_co_save};
    use xedit_defs::callbacks::tes5saves::{define_tes5_saves, switch_to_tes5_co_save};
    match mode {
        GameMode::gmTES4 => Some(SaveDefinitions {
            define: define_tes4_saves,
            switch_to_co_save: switch_to_tes4_co_save,
            co_save_extension: "obse",
        }),
        GameMode::gmFO3 => Some(SaveDefinitions {
            define: define_fo3_saves,
            switch_to_co_save: switch_to_fo3_co_save,
            co_save_extension: "fose",
        }),
        GameMode::gmFNV => Some(SaveDefinitions {
            define: define_fnv_saves,
            switch_to_co_save: switch_to_fnv_co_save,
            co_save_extension: "nvse",
        }),
        GameMode::gmFO4 => Some(SaveDefinitions {
            define: define_fo4_saves,
            switch_to_co_save: switch_to_fo4_co_save,
            co_save_extension: "f4se",
        }),
        GameMode::gmTES5 | GameMode::gmEnderal | GameMode::gmSSE | GameMode::gmEnderalSE => Some(SaveDefinitions {
            define: define_tes5_saves,
            switch_to_co_save: switch_to_tes5_co_save,
            co_save_extension: "skse",
        }),
        _ => None,
    }
}

fn setup(game: &str, source: DefinitionSource, edit: bool, simple_records: Option<bool>) -> Result<GameMode, String> {
    // `xDump.dpr`: `wbAllowInternalEdit := False`, so a dump shows a record
    // as loaded, without the required subrecords the editor would add.
    xedit_core::interface::globals::set_allow_internal_edit(false);
    xedit_core::interface::globals::set_sort_sub_records(false);
    xedit_core::interface::globals::set_display_load_order_form_id(false);
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
        GameMode::gmSF1 => xedit_defs::sf1::define_sf1,
        _ => return Err(format!("the definitions of {tag} are not ported yet")),
    };
    // The quick clean modes load with the full definitions.
    set_simple_records(simple_records.unwrap_or(edit && !crate::commands::quick_clean_on_load()));
    set_hide_unused(edit);
    set_game_mode(mode);
    // xDump turns the contained-in elements off for Fallout 4 and later, the
    // editor for Morrowind.
    set_create_contained_in(if edit {
        mode != GameMode::gmTES3
    } else {
        !matches!(
            mode,
            GameMode::gmFO4 | GameMode::gmFO4VR | GameMode::gmFO76 | GameMode::gmSF1
        )
    });
    set_game_name(game_name);
    set_game_exe_name(&format!("{}.exe", exe_name.unwrap_or(game_name)));
    set_game_master_esm(&master_esm.map_or_else(|| format!("{game_name}.esm"), str::to_owned));
    clear_record_defs();
    clear_resources_loaded_handlers();
    match source {
        DefinitionSource::Plugins => {
            set_tool_source(ToolSource::tsPlugins);
            define();
        }
        DefinitionSource::Saves(path) => {
            set_tool_source(ToolSource::tsSaves);
            let saves =
                save_definitions(mode).ok_or_else(|| format!("the save definitions of {tag} are not ported yet"))?;
            let is_co_save = path.is_some_and(|path| {
                Path::new(path)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case(saves.co_save_extension))
            });
            // The oracle warns that the saves of Fallout 3 and Oblivion are
            // not supported yet, and reads them all the same; the export of
            // the definitions names the format it was given there too
            // (`xDump.dpr:1475`).
            if let Some(path) = path
                && !is_co_save
                && matches!(mode, GameMode::gmFO3 | GameMode::gmTES4)
            {
                eprintln!("Save are not supported yet \"{path}\". Please check the command line parameters.");
            }
            (saves.define)();
            if is_co_save {
                (saves.switch_to_co_save)();
            }
        }
    }
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

/// Loads the plugin with its masters, its resources and the hardcoded
/// records, as `xDump.dpr` does before it starts the dump.
pub fn load_file(path: &str, mode: GameMode) -> Result<Arc<FileImpl>, String> {
    let file = wb_file(path, i32::MAX, FileStates::empty()).map_err(|error| error.to_string())?;
    crate::resources::load_dump_resources(&file, path, &extract_file_path(path), mode);
    load_hardcoded()?;
    Ok(file)
}

/// Loads a save or co-save with the plugins it lists from `data_path`, the
/// resources and the hardcoded records, and writes its dump.
pub fn dump_save(path: &str, data_path: &str, mode: GameMode, out: &mut dyn Write) -> Result<(), String> {
    let mut data_path = data_path.to_owned();
    if !data_path.ends_with(['\\', '/']) {
        data_path.push('\\');
    }
    set_data_path(&data_path);
    let file = wb_file(path, i32::MAX, FileStates::empty()).map_err(|error| error.to_string())?;
    crate::resources::load_dump_resources(&file, path, &data_path, mode);
    load_hardcoded()?;
    // A save has no main records to spread over the workers, and the save
    // definitions keep the tables of the save being read in globals.
    write_elements(&*file, 0, None, &DumpOptions::default(), out).map_err(|error| error.to_string())
}

/// The saves-side switches of `xDump.dpr` (`-dc`, `-df`, `-xc`, `-xcbloat`,
/// `-bts` and `-btd`).
#[derive(Clone, Debug, Default)]
pub struct SaveDumpOptions {
    /// `-dc`: the global data IDs to keep.
    pub dc: Option<String>,
    /// `-df`: the ChangedForm types to keep.
    pub df: Option<String>,
    /// `-xc`: the global data IDs to skip.
    pub xc: Option<String>,
    /// `-xf`: the ChangedForm types to skip.
    pub xf: Option<String>,
    /// `-xcbloat`: skip the global data type 1001.
    pub xcbloat: bool,
    /// `-bts`: the bytes the `Unused` array skips.
    pub bts: Option<i64>,
    /// `-btd`: the bytes the `Remaining` array dumps.
    pub btd: Option<i64>,
}

/// Port of the `-dc`/`-df`/`-xc`/`-xcbloat`/`-bts`/`-btd` handling of
/// `xDump.dpr`: sets `DumpChapters`, `ChaptersToSkip` and the byte limits,
/// which the save definitions and the dump read.
pub fn configure_save_dump(options: &SaveDumpOptions) -> Result<(), String> {
    use xedit_core::interface::formaters::comma_text;
    use xedit_core::interface::globals::{
        chapters_to_skip_add, set_bytes_to_dump, set_bytes_to_skip, set_chapters_to_skip, set_dump_chapters,
    };
    use xedit_defs::callbacks::fo4saves::CHANGED_FORM_OFFSET;

    // `-dc` fills the list, then `-df` adds the changed form chapter types.
    if options.dc.is_some() || options.df.is_some() {
        let mut chapters = Vec::new();
        if let Some(dc) = &options.dc {
            chapters.extend(comma_text(dc));
        }
        if let Some(df) = &options.df {
            for entry in comma_text(df) {
                let value: i32 = entry.parse().map_err(|_| format!("-df: {entry} is not an integer"))?;
                chapters.push((CHANGED_FORM_OFFSET + value).to_string());
            }
        }
        set_dump_chapters(Some(chapters));
    }
    // `-xc` replaces the list; `-xcbloat` only applies without it. `-xf`
    // then adds the changed form chapter types to whatever the list holds.
    if let Some(xc) = &options.xc {
        set_chapters_to_skip(comma_text(xc));
    } else if options.xcbloat {
        chapters_to_skip_add("1001".to_owned());
    }
    if let Some(xf) = &options.xf {
        for entry in comma_text(xf) {
            let value: i32 = entry.parse().map_err(|_| format!("-xf: {entry} is not an integer"))?;
            chapters_to_skip_add((CHANGED_FORM_OFFSET + value).to_string());
        }
    }
    if let Some(bts) = options.bts {
        set_bytes_to_skip(bts);
    }
    if let Some(btd) = options.btd {
        set_bytes_to_dump(btd);
    }
    Ok(())
}

/// The switches of `xDump.dpr` that change what the plain dump writes.
/// The defaults are the upstream ones: every group and record, no sizes, no
/// hidden elements and no report mode.
#[derive(Clone, Debug, Default)]
pub struct DumpOptions {
    /// `-dg`: the top level groups the dump keeps, by label.
    pub dump_groups: Option<Vec<Signature>>,
    /// `-dr`: the records whose children the dump keeps, by signature.
    pub dump_records: Option<Vec<Signature>>,
    /// `-xcg`: the non-top-level groups whose parent record the dump skips.
    pub skip_child_groups: Option<Vec<Signature>>,
    /// `-top`: the number of main records the dump writes, the file header
    /// included (`DumpMax`).
    pub dump_max: i32,
    /// `-es`: write the data size of every element (`DumpSize`).
    pub dump_size: bool,
    /// `-dh`: keep the elements the editor hides (`DumpHidden`).
    pub dump_hidden: bool,
    /// `-report`, `-sro`, `-sroe` and `-dcr`: `wbReportMode`.
    pub report_mode: bool,
    /// `-sro` and `-sroe`: `DontWriteReport`, which silences the dump.
    pub dont_write_report: bool,
    /// `-dcr`: `DumpCheckReport`, the `Check` of every element and the
    /// REPORT header.
    pub dump_check_report: bool,
}

/// One component of a definition path (`TwbDefPath`): the definition and the
/// index of the member of its parent that leads to it, or -1.
struct DefPathPart {
    def: xedit_core::interface::DefRef,
    index: i32,
}

/// Port of `wbDefToName`: the name of a definition in a report line: the
/// signature and the name of a record member, the name of a named definition,
/// else the name of its definition type.
fn def_to_name(def: &dyn xedit_core::interface::Def) -> String {
    use xedit_core::interface::types::PascalEnum;

    let result = if let Some(signature_def) = def.as_signature_def() {
        let signature = signature_def.get_default_signature();
        let mut result = String::new();
        let chars = |result: &mut String, bytes: &[u8]| {
            for &byte in bytes {
                match byte {
                    0..=0x7F => result.push(char::from(byte)),
                    _ => result.push_str(&Encoding::Mbcs(1252).get_string(&[byte]).unwrap_or_default()),
                }
            }
        };
        if signature.0[0] == 0 {
            result.push_str("$(00)");
            chars(&mut result, &signature.0[1..]);
        } else {
            chars(&mut result, &signature.0);
        }
        result.push_str(" - ");
        result.push_str(signature_def.get_name());
        result
    } else if let Some(named_def) = def.as_named_def() {
        named_def.get_name().to_owned()
    } else {
        format!("<{}>", def.get_def_type().enum_name())
    };
    result
        .chars()
        .map(|c| {
            if (c as u32) < 0x20 {
                format!("$({:02X})", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// Port of `wbDefsToPath`: every component as `name \ [index] `, with the
/// index left out for a negative one.
fn defs_to_path(parents: &[DefPathPart]) -> String {
    let mut result = String::new();
    for part in parents {
        result.push_str(&def_to_name(&*part.def));
        result.push_str(" \\ ");
        if part.index >= 0 {
            result.push_str(&format!("[{}] ", part.index));
        }
    }
    result
}

/// Port of the `TStringList.CommaText` of a sorted list of names. Names with
/// whitespace, commas or quotes would be quoted; the report only stores
/// signatures, which are four visible characters.
fn comma_text(entries: &[(String, i32)]) -> String {
    entries
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// Port of the `Unknown Flags in` part of `TwbFlagsDef.Report`.
fn report_flags(
    def: &dyn xedit_core::interface::Def,
    parents: &[DefPathPart],
    out: &mut dyn Write,
) -> std::io::Result<()> {
    use xedit_core::interface::globals::{report_mode, report_unknown_flags};

    let Some(flags_def) = def.as_flags_def() else {
        return Ok(());
    };
    let counts: Vec<(usize, i32)> = (0..64)
        .map(|index| (index, flags_def.unknown_flag_count(index)))
        .filter(|(_, count)| *count > 0)
        .collect();
    if report_mode() && report_unknown_flags() && !counts.is_empty() {
        write_text(
            out,
            &format!("Unknown Flags in: {}{}\r\n", defs_to_path(parents), def_to_name(def)),
        )?;
        for (index, count) in counts {
            write_text(out, &format!("  {index} ({count})\r\n"))?;
        }
    }
    Ok(())
}

/// Port of the `Unknown Enums in` part of `TwbEnumDef.Report`.
fn report_enums(
    def: &dyn xedit_core::interface::Def,
    parents: &[DefPathPart],
    out: &mut dyn Write,
) -> std::io::Result<()> {
    use xedit_core::interface::globals::{report_mode, report_unknown_enums};

    let Some(enum_def) = def.as_enum_def() else {
        return Ok(());
    };
    let unknown = enum_def.unknown_enums();
    if report_mode() && report_unknown_enums() && !unknown.is_empty() {
        write_text(
            out,
            &format!("Unknown Enums in: {}{}\r\n", defs_to_path(parents), def_to_name(def)),
        )?;
        for (value, paths) in unknown {
            write_text(out, &format!("  {value} ({})\r\n", paths.len()))?;
            for (path, count) in paths {
                write_text(out, &format!("    {path} ({count})\r\n"))?;
            }
        }
    }
    Ok(())
}

/// Port of `TwbFormIDDefFormater.Report`.
fn report_form_id_formater(
    def: &dyn xedit_core::interface::Def,
    parents: &[DefPathPart],
    out: &mut dyn Write,
) -> std::io::Result<()> {
    use xedit_core::interface::form_id_formater::FormIDClass;
    use xedit_core::interface::globals::{report_form_ids, report_mode};

    let Some(formater) = def.as_form_id_def_formater() else {
        return Ok(());
    };
    if !(report_mode() && !def.get_def_flags().contains(DefFlag::dfNoReport) && report_form_ids()) {
        return Ok(());
    }
    let found = formater.found_signatures();
    if !found.is_empty() && formater.class() == FormIDClass::FormID {
        write_text(
            out,
            &format!(
                "Unchecked FormID Formatter: {}{}\r\n",
                defs_to_path(parents),
                def_to_name(def)
            ),
        )?;
        write_text(out, &format!("  {}\r\n", comma_text(&found)))?;
        for (signature, count) in &found {
            write_text(out, &format!("  {signature} ({count})\r\n"))?;
        }
    }
    let not_resolved = formater.not_resolved();
    if !not_resolved.is_empty() {
        write_text(
            out,
            &format!(
                "FormID contains unresolvable values: {}{}\r\n",
                defs_to_path(parents),
                def_to_name(def)
            ),
        )?;
        for (form_id, count) in not_resolved {
            write_text(out, &format!("  {form_id} ({count})\r\n"))?;
        }
    }
    let not_allowed = formater.found_not_allowed_references();
    if !not_allowed.is_empty() {
        write_text(
            out,
            &format!(
                "FormID contains not allowed references: {}{}\r\n",
                defs_to_path(parents),
                def_to_name(def)
            ),
        )?;
        for (name, count) in not_allowed {
            write_text(out, &format!("  {name} ({count})\r\n"))?;
        }
    }
    Ok(())
}

/// Port of `TwbFormIDChecked.Report` after its `inherited` call.
fn report_form_id_checked(
    def: &dyn xedit_core::interface::Def,
    parents: &[DefPathPart],
    out: &mut dyn Write,
) -> std::io::Result<()> {
    use xedit_core::interface::form_id_formater::FormIDClass;
    use xedit_core::interface::globals::{report_form_ids, report_mode, report_not_found_but_allowed_form_ids};

    let Some(formater) = def.as_form_id_def_formater() else {
        return Ok(());
    };
    if !matches!(formater.class(), FormIDClass::Checked | FormIDClass::CheckedST) {
        return Ok(());
    }
    if !(report_mode() && !def.get_def_flags().contains(DefFlag::dfNoReport) && report_form_ids()) {
        return Ok(());
    }
    let found = formater.found_signatures();
    if found.is_empty() {
        return Ok(());
    }
    let mut sigs = found;
    let mut not_found = String::new();
    for index in 0..formater.get_signature_count() {
        let valid = formater.get_signature(index as usize).to_string();
        if let Some(position) = sigs.iter().position(|(signature, _)| *signature == valid) {
            sigs.remove(position);
        } else {
            not_found.push_str(&valid);
            not_found.push(',');
        }
    }
    if !not_found.is_empty() {
        not_found.pop();
    }
    if !report_not_found_but_allowed_form_ids() {
        not_found.clear();
    }
    if not_found.is_empty() && sigs.is_empty() {
        return Ok(());
    }
    write_text(
        out,
        &format!(
            "Differences in Checked FormID Formater: {}{}\r\n",
            defs_to_path(parents),
            def_to_name(def)
        ),
    )?;
    if !not_found.is_empty() {
        write_text(out, &format!("  Not found but allowed: {not_found}\r\n"))?;
    }
    if !sigs.is_empty() {
        write_text(out, &format!("  Found but not allowed: {}\r\n", comma_text(&sigs)))?;
        for (signature, count) in &sigs {
            write_text(out, &format!("    {signature} ({count})\r\n"))?;
        }
    }
    Ok(())
}

/// Port of `wbDef.Report`: the shared part of every definition report, then
/// the members of the definition type.
fn report_def(
    def: &dyn xedit_core::interface::Def,
    parents: &mut Vec<DefPathPart>,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    use xedit_core::interface::globals::{report_required, report_unknown, report_unused};

    if def.def_base().def_reported() || def.get_def_flags().contains(DefFlag::dfNoReport) {
        return Ok(());
    }

    // Port of `TwbDef.Report`.
    if report_unused() && !def.def_base().def_used() {
        write_text(
            out,
            &format!("Unused: {}{}\r\n", defs_to_path(parents), def_to_name(def)),
        )?;
    }
    if report_required()
        && def.def_base().def_possibly_required()
        && def.def_base().def_not_required() == def.def_base().def_required()
    {
        let label = if def.def_base().def_not_required() {
            "Not Required: "
        } else {
            "Required: "
        };
        write_text(
            out,
            &format!("{label}{}{}\r\n", defs_to_path(parents), def_to_name(def)),
        )?;
    }
    if report_unknown() {
        let unknown = def.def_base().unknown_values();
        if !unknown.is_empty() {
            write_text(
                out,
                &format!(
                    "Unknown Field: {}{} ({})\r\n",
                    defs_to_path(parents),
                    def_to_name(def),
                    unknown.len()
                ),
            )?;
            for (value, paths) in unknown {
                write_text(out, &format!("  {value} ({})\r\n", paths.len()))?;
                for path in paths {
                    write_text(out, &format!("    {path}\r\n"))?;
                }
            }
        } else if def.def_base().is_unknown.load(std::sync::atomic::Ordering::Relaxed) {
            write_text(
                out,
                &format!("Unknown Field: {}{}\r\n", defs_to_path(parents), def_to_name(def)),
            )?;
        }
    }
    def.def_base().mark_reported();

    // `TwbMainRecordDef.Report` and the `TwbSubRecordStructDef` and
    // `TwbSubRecordUnionDef` reports: the members, each with its index.
    if let Some(record_def) = def.as_record_def() {
        parents.push(DefPathPart {
            def: def.def_ref(),
            index: -1,
        });
        for index in 0..record_def.get_member_count() {
            parents.last_mut().unwrap().index = index;
            let member = record_def.get_member(index as usize);
            report_def(member.as_dyn_def(), parents, out)?;
        }
        parents.pop();
        return Ok(());
    }

    // `TwbSubRecordDef.Report`: the value, then the unused data marker of the
    // subrecord itself.
    if let Some(sub_record) = def.as_sub_record_def() {
        if let Some(value) = sub_record.get_value() {
            parents.push(DefPathPart {
                def: def.def_ref(),
                index: -1,
            });
            report_def(value.as_dyn_def(), parents, out)?;
            parents.pop();
        }
        if xedit_core::interface::globals::report_unused_data() && sub_record.sr_has_unused_data() {
            write_text(
                out,
                &format!("Has Unused Data: {}{}\r\n", defs_to_path(parents), def_to_name(def)),
            )?;
        }
        return Ok(());
    }

    // `TwbSubRecordArrayDef.Report`: the element with the array in the path.
    if let Some(sub_array) = def.as_sub_record_array_def() {
        parents.push(DefPathPart {
            def: def.def_ref(),
            index: -1,
        });
        report_def(sub_array.get_element().as_dyn_def(), parents, out)?;
        parents.pop();
        return Ok(());
    }

    // `TwbStructDef.Report`: the members, each with its index.
    if let Some(struct_def) = def.as_struct_def() {
        parents.push(DefPathPart {
            def: def.def_ref(),
            index: -1,
        });
        for index in 0..struct_def.get_member_count() {
            parents.last_mut().unwrap().index = index;
            let member = struct_def.get_member(index as usize);
            report_def(member.as_dyn_def(), parents, out)?;
        }
        parents.pop();
        return Ok(());
    }

    // `TwbArrayDef.Report` reports the element with the path of the array
    // itself (`arElement.Report(aParents)` upstream).
    if let Some(array_def) = def.as_array_def() {
        if let Some(element) = array_def.try_get_element() {
            report_def(element.as_dyn_def(), parents, out)?;
        }
        return Ok(());
    }

    // `TwbUnionDef.Report`: the members, each with its index.
    if let Some(union_def) = def.as_union_def() {
        parents.push(DefPathPart {
            def: def.def_ref(),
            index: -1,
        });
        for index in 0..union_def.get_member_count() {
            parents.last_mut().unwrap().index = index;
            let member = union_def.get_member(index as usize);
            report_def(member.as_dyn_def(), parents, out)?;
        }
        parents.pop();
        return Ok(());
    }

    // `TwbIntegerDef.Report` reports its formater with the path of the
    // integer itself (`inFormater.Report(aParents)` upstream).
    if let Some(integer_def) = def.as_integer_def() {
        if let Some(formater) = integer_def.get_raw_formater() {
            report_def(formater.as_dyn_def(), parents, out)?;
        }
        return Ok(());
    }

    // `TwbBaseStringDef.Report` reports its formater with the path of the
    // string itself (`bsdFormater.Report(aParents)` upstream).
    if let Some(string_def) = def.as_string_def() {
        if let Some(formater) = string_def.formater() {
            report_def(formater.as_dyn_def(), parents, out)?;
        }
        return Ok(());
    }
    if let Some(len_string_def) = def.as_len_string_def() {
        if let Some(formater) = len_string_def.formater() {
            report_def(formater.as_dyn_def(), parents, out)?;
        }
        return Ok(());
    }

    if def.as_flags_def().is_some() {
        report_flags(def, parents, out)?;
        return Ok(());
    }
    if def.as_enum_def().is_some() {
        report_enums(def, parents, out)?;
        return Ok(());
    }
    if def.as_form_id_def_formater().is_some() {
        report_form_id_formater(def, parents, out)?;
        report_form_id_checked(def, parents, out)?;
    }
    Ok(())
}

/// Port of the `ReportDefs` tail of `xDump.dpr`: every record definition, in
/// definition order, reports what the load and the dump recorded in report
/// mode.
fn report_defs(out: &mut dyn Write) -> std::io::Result<()> {
    let mut parents = Vec::new();
    for (_, record_def) in xedit_core::interface::record_defs() {
        report_def(record_def.as_dyn_def(), &mut parents, out)?;
    }
    Ok(())
}

/// Loads the plugin and writes its dump with `options`.
pub fn dump_file_with_options(
    path: &str,
    mode: GameMode,
    options: &DumpOptions,
    out: &mut dyn Write,
) -> Result<(), String> {
    let file = load_file(path, mode)?;
    write_container_with_options(&file, options, out).map_err(|error| error.to_string())?;
    if options.report_mode {
        if options.dump_check_report {
            out.write_all(
                b"\r\n==================================== REPORT ====================================\r\n\r\n",
            )
            .map_err(|error| error.to_string())?;
        }
        if !options.dont_write_report {
            report_defs(out).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Loads the save and writes its dump with the default options.
pub fn dump_file(path: &str, mode: GameMode, out: &mut dyn Write) -> Result<(), String> {
    dump_file_with_options(path, mode, &DumpOptions::default(), out)
}

/// Port of `xDump -check`: loads the plugin and writes the errors of its
/// elements (`CheckForErrors` of `xDump.dpr`, `xedit_analysis::check::dump_check`)
/// instead of its dump. The `Checking:` lines of the top groups go to the
/// progress log.
pub fn check_file(path: &str, mode: GameMode, out: &mut dyn Write) -> Result<(), String> {
    let file = load_file(path, mode)?;
    let element: xedit_core::interface::ElementRef = file;
    let lines = xedit_analysis::check::dump_check(&element, &mut |line| {
        xedit_core::interface::misc::progress(line);
    });
    for line in lines {
        write_text(out, &line.text()).map_err(|error| error.to_string())?;
        out.write_all(b"\r\n").map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Writes the dump of a loaded file, on the threads of `threads::threads`.
pub fn write_dump(file: &FileImpl, out: &mut dyn Write) -> std::io::Result<()> {
    write_container(file, out)
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

/// Port of `WriteContainer`. With more than one thread the main records
/// are built and written on worker threads (`ParallelDump`); the output is
/// the same. `-top` counts the main records in file order, so it forces the
/// serial dump.
fn write_container(container: &FileImpl, out: &mut dyn Write) -> std::io::Result<()> {
    write_container_with_options(container, &DumpOptions::default(), out)
}

fn write_container_with_options(
    container: &FileImpl,
    options: &DumpOptions,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    match threads::pool() {
        None => {
            let mut count = 0;
            write_elements_with_count(container, 0, None, options, &mut count, out)
        }
        Some(pool) if options.dump_max <= 0 => ParallelDump::new(pool, options, out).run(container),
        Some(_) => {
            let mut count = 0;
            write_elements_with_count(container, 0, None, options, &mut count, out)
        }
    }
}

/// The number of main records `WriteElement` wrote so far (`DumpCount`):
/// the file header counts as one, as it does upstream.
type DumpCount = i32;

/// Writes the elements of `container` at nesting `depth` with the default
/// options. `record` is the main record the elements belong to.
fn write_elements(
    container: &dyn Container,
    depth: usize,
    record: Option<&Arc<MainRecordImpl>>,
    options: &DumpOptions,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let mut count = 0;
    write_elements_with_count(container, depth, record, options, &mut count, out)
}

/// Writes the elements of `container` at nesting `depth`. `record` is the
/// main record the elements belong to.
fn write_elements_with_count(
    container: &dyn Container,
    depth: usize,
    record: Option<&Arc<MainRecordImpl>>,
    options: &DumpOptions,
    count: &mut DumpCount,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    for index in 0..container.get_element_count() {
        if let Some(element) = container.get_element(index) {
            write_element_with_count(&element, depth, record, options, count, out)?;
        }
    }
    Ok(())
}

/// A main record of a batch, written by a worker at its place in the
/// output.
struct RecordJob {
    record: Arc<MainRecordImpl>,
    element: ElementRef,
    depth: usize,
}

/// The output of a batch in file order.
enum Segment {
    /// The line of an element outside of the main records (the file, a
    /// group), with the element and its depth to write it again.
    Line(ElementRef, usize, Vec<u8>),
    /// `<contents skipped>` at a depth.
    Skipped(usize),
    /// A main record, by its index in the batch.
    Record(usize),
}

/// A batch ends after this many main records, or after records with this
/// much data, whichever comes first.
const BATCH_RECORDS: usize = 512;
const BATCH_BYTES: usize = 4 << 20;

/// The batches on the workers at most. A record that takes long holds the
/// output of the batches after it, which bounds the memory.
const BATCHES_IN_FLIGHT: usize = 4;

/// A batch whose records are written on the workers.
struct Batch {
    segments: Vec<Segment>,
    jobs: Arc<Vec<RecordJob>>,
    /// `init_cycles` when the walk of the batch began.
    cycles: u64,
    written: std::sync::mpsc::Receiver<std::io::Result<Vec<Vec<u8>>>>,
}

/// The dump with the main records built and written on worker threads.
///
/// The calling thread walks the tree and writes the lines of the file and
/// its groups; the main records are collected in batches, and the workers
/// build and write each record of a batch into a buffer of its own while the
/// walk goes on. The batches are written to the output in file order, so
/// the output is the serial output. A worker writes a record as the serial
/// dump does, trimming the other records after each line and resetting the
/// record at the end; a record that another thread reads at that moment is
/// built again for it (see `xedit_core::threads`), and the records the
/// workers write are pinned, so that a trim does not reset them.
///
/// When two builds needed each other (`init_cycles`), what the threads built
/// meanwhile could depend on timing: the dump waits for the workers, resets
/// the records, and writes every batch not yet written again on the calling
/// thread alone.
struct ParallelDump<'a> {
    pool: &'static rayon::ThreadPool,
    options: &'a DumpOptions,
    out: &'a mut dyn Write,
    /// The batch being collected.
    segments: Vec<Segment>,
    jobs: Vec<RecordJob>,
    batch_bytes: usize,
    batch_cycles: u64,
    /// The batches on the workers, in file order.
    in_flight: std::collections::VecDeque<Batch>,
}

impl<'a> ParallelDump<'a> {
    fn new(pool: &'static rayon::ThreadPool, options: &'a DumpOptions, out: &'a mut dyn Write) -> Self {
        ParallelDump {
            pool,
            options,
            out,
            segments: Vec::new(),
            jobs: Vec::new(),
            batch_bytes: 0,
            batch_cycles: init_cycles(),
            in_flight: std::collections::VecDeque::new(),
        }
    }

    fn run(mut self, container: &FileImpl) -> std::io::Result<()> {
        self.walk_elements(container, 0)?;
        self.flush()?;
        while !self.in_flight.is_empty() {
            self.write_first()?;
        }
        Ok(())
    }

    fn walk_elements(&mut self, container: &dyn Container, depth: usize) -> std::io::Result<()> {
        for index in 0..container.get_element_count() {
            if let Some(element) = container.get_element(index) {
                self.walk_element(&element, depth)?;
            }
        }
        Ok(())
    }

    /// `write_element` for the elements outside of the main records; a main
    /// record goes into the batch.
    fn walk_element(&mut self, element: &ElementRef, depth: usize) -> std::io::Result<()> {
        if group_filtered(element, self.options, true) {
            return Ok(());
        }
        if let Some(record) = element.as_element_impl().and_then(ElementImpl::main_record_impl) {
            self.batch_bytes += record.header_struct().data_size as usize;
            self.segments.push(Segment::Record(self.jobs.len()));
            self.jobs.push(RecordJob {
                record,
                element: element.clone(),
                depth,
            });
            if self.jobs.len() >= BATCH_RECORDS || self.batch_bytes >= BATCH_BYTES {
                self.flush()?;
            }
            return Ok(());
        }
        if trace_enabled() {
            eprintln!("{:width$}{}", "", element.get_name(), width = depth * 2);
        }
        let mut line = Vec::new();
        let (name, child_depth) = {
            let _read = threads::read_guard();
            let name = element.get_display_name(true);
            let child_depth = write_line(element, &name, depth, self.options, &mut line)?;
            (name, child_depth)
        };
        self.segments.push(Segment::Line(element.clone(), depth, line));
        if let Some(container) = element.as_container()
            && !group_filtered(element, self.options, false)
            && (self.options.dump_hidden || !name.starts_with("Hidden: "))
        {
            if element.get_skipped() {
                if !self.options.report_mode || self.options.dump_check_report {
                    self.segments.push(Segment::Skipped(child_depth));
                }
            } else {
                self.walk_elements(container, child_depth)?;
            }
        }
        Ok(())
    }

    /// Hands the collected batch to the workers, and writes the batches
    /// before it that are done while more than `BATCHES_IN_FLIGHT` run.
    fn flush(&mut self) -> std::io::Result<()> {
        let jobs = Arc::new(std::mem::take(&mut self.jobs));
        let (sender, written) = std::sync::mpsc::channel();
        let batch_jobs = jobs.clone();
        let options = self.options.clone();
        self.pool.spawn(move || {
            let result = batch_jobs
                .par_iter()
                .with_max_len(1)
                .map(|job| {
                    // The record stays built while the worker writes it, and
                    // is reset at the end as on one thread.
                    let _pin = pin_record(&job.record);
                    let mut buffer = Vec::new();
                    write_element(&job.element, job.depth, None, &options, &mut buffer)?;
                    Ok(buffer)
                })
                .collect();
            // The dump may have stopped at an error and dropped the receiver.
            let _ = sender.send(result);
        });
        self.in_flight.push_back(Batch {
            segments: std::mem::take(&mut self.segments),
            jobs,
            cycles: self.batch_cycles,
            written,
        });
        self.batch_bytes = 0;
        self.batch_cycles = init_cycles();
        while self.in_flight.len() > BATCHES_IN_FLIGHT {
            self.write_first()?;
        }
        Ok(())
    }

    /// Waits for the first batch on the workers and writes it.
    fn write_first(&mut self) -> std::io::Result<()> {
        let Some(batch) = self.in_flight.pop_front() else {
            return Ok(());
        };
        let written = batch
            .written
            .recv()
            .unwrap_or_else(|_| Err(std::io::Error::other("a dump worker stopped")))?;
        if init_cycles() == batch.cycles {
            return write_batch(self.out, &batch.segments, &written);
        }
        // Two builds needed each other since this batch began: no thread
        // may build anything while the batches not yet written are written
        // again, from records that are not built.
        let mut batches = vec![batch];
        while let Some(batch) = self.in_flight.pop_front() {
            let _ = batch.written.recv();
            batches.push(batch);
        }
        eprintln!(
            "Warning: two records needed each other while they were built; writing the records again on one thread"
        );
        for batch in &batches {
            for job in batch.jobs.iter() {
                job.record.reset();
            }
        }
        trim_initialized_records(0, None);
        for batch in &batches {
            let written = batch
                .jobs
                .iter()
                .map(|job| {
                    let mut buffer = Vec::new();
                    write_element(&job.element, job.depth, None, self.options, &mut buffer)?;
                    Ok(buffer)
                })
                .collect::<std::io::Result<Vec<_>>>()?;
            let mut segments = Vec::with_capacity(batch.segments.len());
            for segment in &batch.segments {
                segments.push(match segment {
                    Segment::Line(element, depth, _) => {
                        let mut line = Vec::new();
                        write_line(
                            element,
                            &element.get_display_name(true),
                            *depth,
                            self.options,
                            &mut line,
                        )?;
                        Segment::Line(element.clone(), *depth, line)
                    }
                    Segment::Skipped(depth) => Segment::Skipped(*depth),
                    Segment::Record(index) => Segment::Record(*index),
                });
            }
            write_batch(self.out, &segments, &written)?;
        }
        Ok(())
    }
}

/// Writes the output of a batch in file order.
fn write_batch(out: &mut dyn Write, segments: &[Segment], written: &[Vec<u8>]) -> std::io::Result<()> {
    for segment in segments {
        match segment {
            Segment::Line(_, _, line) => out.write_all(line)?,
            Segment::Skipped(depth) => {
                write_indent(out, *depth)?;
                out.write_all(b"<contents skipped>\r\n")?;
            }
            Segment::Record(index) => out.write_all(&written[*index])?,
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

/// The group filters of `WriteElement` and `WriteContainer` (`-dg` and
/// `-xcg`): whether the element is left out entirely.
///
/// `WriteElement` runs its filters only when `DumpGroups` is assigned
/// (`xDump.dpr:671`), so a child group excluded by `-xcg` alone still writes
/// its line; `WriteContainer` then skips the children (`xDump.dpr:609`).
/// `write_element` is false for that second check.
fn group_filtered(element: &ElementRef, options: &DumpOptions, write_element: bool) -> bool {
    let Some(group) = element.as_element_impl().and_then(ElementImpl::group_record_impl) else {
        return false;
    };
    if write_element && options.dump_groups.is_none() {
        return false;
    }
    if group.group_type() == 0 {
        if let Some(dump_groups) = &options.dump_groups
            && !signature_list_contains(dump_groups, group.gr_struct().label_signature())
        {
            return true;
        }
    } else if let Some(skip_child_groups) = &options.skip_child_groups
        && let Some(children_of) = group.children_of()
        && signature_list_contains(skip_child_groups, children_of.get_signature())
    {
        return true;
    }
    false
}

/// The chapter type of a save element (`IwbChapter.ChapterType`), or `None`
/// when its definition has no chapter.
fn chapter_type(element: &ElementRef) -> Option<i32> {
    let value_def = element.get_value_def()?;
    let struct_def = value_def.as_struct_def()?;
    struct_def.chapter()?;
    let data = element.as_data_container()?.get_data();
    Some(struct_def.get_chapter_type(data, Some(element)))
}

/// The chapter filters of `WriteContainer` (`xDump.dpr:627`, saves only):
/// `-dc`/`-df` keep only the listed chapter types and `-xc`/`-xf`/
/// `-xcbloat` drop the listed ones. The checks run before the `Skipped`
/// handling, so a filtered chapter writes no line at all.
fn chapter_filtered(element: &ElementRef) -> bool {
    let Some(chapter_type) = chapter_type(element) else {
        return false;
    };
    let chapter_type = chapter_type.to_string();
    if let Some(dump_chapters) = dump_chapters()
        && !dump_chapters.iter().any(|entry| entry == &chapter_type)
    {
        return true;
    }
    chapters_to_skip_contains(&chapter_type)
}

/// Port of `WriteElement` in the plain dump mode. The name, the value and
/// the summary are written straight into `out`; the summary is only
/// computed when the line needs it.
fn write_element(
    element: &ElementRef,
    depth: usize,
    record: Option<&Arc<MainRecordImpl>>,
    options: &DumpOptions,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let mut count = 0;
    write_element_with_count(element, depth, record, options, &mut count, out)
}

fn write_element_with_count(
    element: &ElementRef,
    depth: usize,
    record: Option<&Arc<MainRecordImpl>>,
    options: &DumpOptions,
    count: &mut DumpCount,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    if group_filtered(element, options, true) {
        return Ok(());
    }
    let own_record = element.as_element_impl().and_then(ElementImpl::main_record_impl);
    let record = own_record.as_ref().or(record);
    if own_record.is_some() {
        *count += 1;
        // `if wbReportMode and DontWriteReport`: the record is built and the
        // sampling count goes on, and nothing is written.
        if options.report_mode && options.dont_write_report {
            return Ok(());
        }
    }
    if options.dump_max > 0 && *count > options.dump_max {
        return Ok(());
    }
    let error = if options.dump_check_report {
        element.get_check()
    } else {
        String::new()
    };
    if trace_enabled() {
        eprintln!("{:width$}{}", "", element.get_name(), width = depth * 2);
    }
    // The line reads other records, which another thread may reset.
    let (name, child_depth) = {
        let _read = threads::read_guard();
        let name = element.get_display_name(true);
        let child_depth = write_line(element, &name, depth, options, out)?;
        (name, child_depth)
    };
    // The other records the callbacks of this line built, such as the
    // navmeshes the edges of a navmesh link to, are released by Delphi when
    // the callback lets go of them. The port resets them once more than
    // `KEPT_RECORDS` are built; the record being written stays.
    trim_initialized_records(kept_records(), record);
    if options.dump_check_report && !error.is_empty() {
        write_indent(out, child_depth)?;
        out.write_all(b"[ERROR: ")?;
        write_text(out, &error)?;
        out.write_all(b"]\r\n")?;
    }
    if let Some(container) = element.as_container()
        && !group_filtered(element, options, false)
        && !chapter_filtered(element)
        && (options.dump_hidden || !name.starts_with("Hidden: "))
    {
        if element.get_skipped() {
            if !options.report_mode || options.dump_check_report {
                write_indent(out, child_depth)?;
                out.write_all(b"<contents skipped>\r\n")?;
            }
        } else if let Some(own_record) = &own_record {
            // Port of `WriteContainer`'s `DumpRecords` filter: a record not
            // listed keeps its line but not its children.
            let kept = options
                .dump_records
                .as_ref()
                .is_none_or(|dump_records| signature_list_contains(dump_records, own_record.get_signature()));
            if kept {
                write_elements_with_count(container, child_depth, record, options, count, out)?;
            }
        } else {
            write_elements_with_count(container, child_depth, record, options, count, out)?;
        }
    }
    // `WriteContainer` holds an `IwbContainerElementRef` on the record while
    // it writes the elements; releasing it resets the record and frees the
    // subrecords, so the dump never holds more than one record tree.
    if let Some(own_record) = &own_record {
        own_record.reset();
    }
    Ok(())
}

/// The line `WriteElement` writes for the element with the display name
/// `name`: the name with the value or the summary. Returns the depth of the
/// children.
fn write_line(
    element: &ElementRef,
    name: &str,
    depth: usize,
    options: &DumpOptions,
    out: &mut dyn Write,
) -> std::io::Result<usize> {
    let value = element.get_value();
    // `(not wbReportMode) or DumpCheckReport`: the report modes without a
    // check report write no line at all.
    let writes = !options.report_mode || options.dump_check_report;
    let mut child_depth = depth;
    if options.dump_hidden || (element.get_name() != "Unused" && name != "Unused") {
        if writes && !name.is_empty() {
            write_indent(out, depth)?;
            write_text(out, name)?;
        }
        if !name.is_empty() || !value.is_empty() {
            child_depth += 1;
            if options.dump_size && writes {
                if !name.is_empty() {
                    out.write_all(b" ")?;
                }
                out.write_all(b"[")?;
                out.write_all(element.get_data_size().to_string().as_bytes())?;
                out.write_all(b"]")?;
            }
        }
        if (options.dump_hidden || !name.starts_with("Hidden: ")) && writes {
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
    Ok(child_depth)
}

/// The number of other records whose subrecords stay built while the dump
/// goes on, so that the records the callbacks read often are not built
/// again for every line.
///
/// Measured on `DLCCoast.esm`: 64 records peak at 1.3 GB in 78 s, 256 at
/// 1.6 GB in 69 s, 1024 at 2.1 GB in 67 s.
const KEPT_RECORDS: usize = 256;

/// The kept records of `KEPT_RECORDS`, times this for the workers of the
/// parallel dump, which share them.
const KEPT_RECORDS_PER_WORKER: usize = 4;

fn kept_records() -> usize {
    match rayon::current_thread_index() {
        Some(_) => KEPT_RECORDS * KEPT_RECORDS_PER_WORKER,
        None => KEPT_RECORDS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xedit_core::interface::def::NamedDefArgs;
    use xedit_core::interface::form_id_formater::FormIDDefFormater;
    use xedit_core::interface::globals::test_lock;
    use xedit_core::interface::integer::IntegerDef;
    use xedit_core::interface::struct_def::{StructDef, StructDefArgs};
    use xedit_core::interface::sub_record::SubRecordDef;
    use xedit_core::interface::types::{ConflictPriority, IntType};

    fn named(name: &str) -> NamedDefArgs {
        NamedDefArgs {
            priority: ConflictPriority::cpNormal,
            required: false,
            name: name.to_owned(),
            after_load: None,
            after_set: None,
            dont_show: None,
            get_cp: None,
            terminator: false,
        }
    }

    fn integer(name: &str) -> DefPathPart {
        DefPathPart {
            def: IntegerDef::create(named(name), IntType::itU32, None, 0),
            index: -1,
        }
    }

    /// `wbDefToName` and `wbDefsToPath` on the witness path of the
    /// `ccswkfo4002-pipnuka.esl` `-dcr` case.
    #[test]
    fn report_names() {
        let _guard = test_lock();
        let sub_record = DefPathPart {
            def: SubRecordDef::create(named("Data"), &[Signature::new(b"DATA")], None, false),
            index: -1,
        };
        let structure = DefPathPart {
            def: StructDef::create(named(""), StructDefArgs::default()),
            index: 11,
        };
        assert_eq!(def_to_name(&*sub_record.def), "DATA - Data");
        // The unnamed structure of `wbStruct` is a named definition with an
        // empty name, which renders as nothing, as the witness path shows.
        assert_eq!(def_to_name(&*structure.def), "");
        assert_eq!(def_to_name(&*integer("Value 1").def), "Value 1");

        let formater = FormIDDefFormater::create();
        assert_eq!(def_to_name(&*formater), "TwbFormIDDefFormater");
        let mut parents = vec![sub_record, structure, integer("Property"), integer("Value 1")];
        parents[2].index = 6;
        parents[3].index = 4;
        assert_eq!(
            format!("{}{}", defs_to_path(&parents), def_to_name(&*formater)),
            r"DATA - Data \  \ [11] Property \ [6] Value 1 \ [4] TwbFormIDDefFormater"
        );

        // Control characters are escaped, and a zero first signature byte
        // renders as $(00).
        let control = integer("A\u{1}B");
        assert_eq!(def_to_name(&*control.def), "A$(01)B");
        let zeroed = SubRecordDef::create(named("Seek"), &[Signature::new(&[0, b'S', b'K', b'R'])], None, false);
        assert_eq!(def_to_name(&*zeroed), "$(00)SKR - Seek");
    }

    /// The `-dc`/`-df`/`-xc`/`-xf`/`-xcbloat` wiring of `xDump.dpr`: the
    /// chapter lists the dump and the save definitions read.
    #[test]
    fn configure_save_dump_chapters() {
        let _guard = test_lock();
        use xedit_core::interface::globals::{
            bytes_to_dump, bytes_to_skip, chapters_to_skip, dump_chapters, set_bytes_to_dump, set_bytes_to_skip,
            set_chapters_to_skip, set_dump_chapters,
        };

        let options = SaveDumpOptions {
            dc: Some("1, 2".to_owned()),
            df: Some("5".to_owned()),
            ..SaveDumpOptions::default()
        };
        configure_save_dump(&options).unwrap();
        assert_eq!(
            dump_chapters(),
            Some(vec!["1".to_owned(), "2".to_owned(), "10005".to_owned()])
        );
        assert!(chapters_to_skip().is_empty());

        set_dump_chapters(None);
        set_chapters_to_skip(Vec::new());
        let options = SaveDumpOptions {
            xc: Some("7,8".to_owned()),
            xcbloat: true,
            xf: Some("3".to_owned()),
            ..SaveDumpOptions::default()
        };
        configure_save_dump(&options).unwrap();
        assert_eq!(chapters_to_skip(), ["7", "8", "10003"]);

        // `-xcbloat` only applies without `-xc`.
        set_chapters_to_skip(Vec::new());
        configure_save_dump(&SaveDumpOptions {
            xcbloat: true,
            ..SaveDumpOptions::default()
        })
        .unwrap();
        assert_eq!(chapters_to_skip(), ["1001"]);

        let options = SaveDumpOptions {
            bts: Some(-1),
            btd: Some(16),
            ..SaveDumpOptions::default()
        };
        configure_save_dump(&options).unwrap();
        assert_eq!(bytes_to_skip(), -1);
        assert_eq!(bytes_to_dump(), 16);
        set_bytes_to_skip(0);
        set_bytes_to_dump(0xFFFF_FFFF);

        let error = configure_save_dump(&SaveDumpOptions {
            df: Some("x".to_owned()),
            ..SaveDumpOptions::default()
        })
        .unwrap_err();
        assert!(error.contains("-df: x is not an integer"), "{error}");
    }
}
