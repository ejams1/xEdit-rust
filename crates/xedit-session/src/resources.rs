// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (TLoaderThread.Execute, the
// resources), xDump.dpr (the resources of the dump), xEdit/xeInit.pas
// (DoInitPath: the My Games folder and the game inis; wbLoadBSAs; the
// language of the game inis)

//! Which archives a load adds to the container handler
//! (`xedit_core::container_handler`), in which order, and whether.
//!
//! Two loaders exist upstream and both are here:
//!
//! - **The GUI's** (`TLoaderThread.Execute`), which every session of the
//!   port runs ([`load_gui_resources`]): the archives the game inis list
//!   (`FindBSAs` of `wbTheGameIniFileName`, with `wbCustomIniFileName` where
//!   it has an `[Archive]` section), then for each module of the load order
//!   `HasBSAs`: `<plugin>*.<ext>` in the order the folder lists them
//!   (exactly `<plugin>.<ext>` for Skyrim LE and Enderal, after the archives
//!   of `<plugin>.ini` in the Skyrim games), then the data folder. Every
//!   archive found is added only while `wbLoadBSAs` is on, else named as
//!   `Skipped.`; it does not depend on any plugin being localized. So the
//!   Fallout 4 DLC find their string tables in `<plugin> - Main.ba2`.
//! - **xDump's** ([`load_dump_resources`]), which `xedit dump` and `xedit
//!   saves dump` run: archives only when a plugin of the load is localized and
//!   a loose `.STRINGS` table is missing (or with `-bsa`), and then only the
//!   plugin's own, interface, localization and Wwise archives (every
//!   `<plugin>*` one with `-allbsa`); `-nobsa` turns the automatic case off.
//!
//! `wbLoadBSAs` follows `xeInit.pas`: off for Fallout 3, New Vegas,
//! Morrowind and Oblivion Remastered, on for the others ([`crate::save::apply_edit_settings`]),
//! then `LoadBSAs` of the settings file and `-skipbsa`/`-forcebsa`
//! ([`apply_load_settings`]), then the tool mode (`tool_modes::apply_on_load`:
//! on for the script, LOD and translate modes, off for the check, clean
//! report, master update, ESMify/ESPify and ONAM modes). The game inis are
//! upstream's: `-I:` and `-CustomIni:` (`--game-ini`, `--custom-ini`), else
//! `<My Games>\<game>.ini` and `<game>Custom.ini` (`Fallout.ini` and
//! `FalloutCustom.ini` for Fallout 3 and New Vegas), where `<My Games>` is
//! `-M:` (`--my-games`) or `Documents\My Games\<wbGameName2>\` ([`init_game_paths`]).

use std::path::Path;
use std::sync::Mutex;

use xedit_core::container_handler::{
    add_archive, add_folder, clear_containers, container_list, ensure_cache, set_loader_done,
};
use xedit_core::delphi::{change_file_ext, extract_file_path};
use xedit_core::helpers::{archive_extension, find_bsas, find_bsas_custom, has_bsas, make_data_file_name};
use xedit_core::implementation::FileImpl;
use xedit_core::interface::globals::{
    self, GameMode, custom_ini_file_name, decode_texture_hashes, game_mode, game_name, game_name2, language,
    load_all_bsas, load_bsas, set_custom_ini_file_name, set_data_path, set_load_all_bsas, set_load_bsas,
    set_my_games_the_game_path, set_the_game_ini_file_name, the_game_ini_file_name, wb_resources_loaded,
};
use xedit_core::interface::misc::progress;
use xedit_core::interface::{Element, File};
use xedit_core::localization::install_localization_handler;

/// The options of a load that decide its archives, as the command line of
/// xEdit gives them. The CLI sets them before a session loads
/// ([`set_options`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceOptions {
    /// `-forcebsa` (`Some(true)`) or `-skipbsa` (`Some(false)`), read after
    /// the settings file and before the tool mode.
    pub load_bsas: Option<bool>,
    /// `-M:`: the folder of the game's inis below `My Games`.
    pub my_games: Option<String>,
    /// `-I:`: the game ini.
    pub game_ini: Option<String>,
    /// `-CustomIni:`: the custom ini.
    pub custom_ini: Option<String>,
}

static OPTIONS: Mutex<ResourceOptions> = Mutex::new(ResourceOptions {
    load_bsas: None,
    my_games: None,
    game_ini: None,
    custom_ini: None,
});

/// Sets the options of the loads that follow.
pub fn set_options(options: ResourceOptions) {
    *OPTIONS.lock().unwrap() = options;
}

/// The options of the loads.
pub fn options() -> ResourceOptions {
    OPTIONS.lock().unwrap().clone()
}

/// xDump's archive switches: `-bsa`, `-allbsa` and `-nobsa`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DumpArchiveSwitches {
    /// `-bsa`: the plugin's own, interface, localization and Wwise archives
    /// of every module, also when no string table is missing.
    pub bsa: bool,
    /// `-allbsa`: every `<plugin>*` archive of every module.
    pub all_bsa: bool,
    /// `-nobsa`: no archives for a localized plugin whose loose tables are
    /// missing.
    pub no_bsa: bool,
}

static DUMP_SWITCHES: Mutex<DumpArchiveSwitches> = Mutex::new(DumpArchiveSwitches {
    bsa: false,
    all_bsa: false,
    no_bsa: false,
});

/// Sets xDump's archive switches for the dumps that follow.
pub fn set_dump_switches(switches: DumpArchiveSwitches) {
    *DUMP_SWITCHES.lock().unwrap() = switches;
}

/// `GetCSIDLShellFolder(CSIDL_PERSONAL)`: the user's documents folder with
/// a trailing separator, or an empty string.
fn documents_folder() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::{CSIDL_PERSONAL, SHGetFolderPathW};
        let mut buffer = [0u16; 260];
        // SAFETY: the buffer holds MAX_PATH characters, as the function needs.
        let result = unsafe {
            SHGetFolderPathW(
                std::ptr::null_mut(),
                CSIDL_PERSONAL as i32,
                std::ptr::null_mut(),
                0,
                buffer.as_mut_ptr(),
            )
        };
        if result != 0 {
            return String::new();
        }
        let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
        let path = String::from_utf16_lossy(&buffer[..length]);
        if path.is_empty() {
            path
        } else {
            xedit_io::archive::include_trailing_path_delimiter(&path)
        }
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME")
            .map(|home| format!("{home}/Documents/"))
            .unwrap_or_default()
    }
}

/// The folder above the data folder, with a trailing separator
/// (`ExtractFilePath(ExcludeTrailingPathDelimiter(wbDataPath))`).
pub(crate) fn game_folder(data_path: &str) -> String {
    let trimmed = data_path.strip_suffix(['\\', '/']).unwrap_or(data_path);
    extract_file_path(trimmed)
}

/// Port of the part of `DoInitPath` that finds the game's inis:
/// `wbMyGamesTheGamePath` (`-M:`, else `<Documents>\My Games\<wbGameName2>\`,
/// the game folder for Morrowind, `FalloutNV_Epic` for the Epic release of
/// New Vegas), `wbTheGameIniFileName` (`-I:`, else the game's ini there,
/// the one of the game folder for the VR games and Starfield when it is
/// missing) and `wbCustomIniFileName` (`-CustomIni:`, else the custom ini
/// there).
pub fn init_game_paths(mode: GameMode, data_path: &str) {
    let options = options();
    let mut my_games = match &options.my_games {
        Some(path) => path.clone(),
        None => {
            let documents = documents_folder();
            if documents.is_empty() {
                String::new()
            } else if mode == GameMode::gmTES3 {
                xedit_io::archive::include_trailing_path_delimiter(&game_folder(data_path))
            } else if mode == GameMode::gmFNV
                && Path::new(&format!(
                    "{}EOSSDK-Win32-Shipping.dll",
                    xedit_io::archive::include_trailing_path_delimiter(&game_folder(data_path))
                ))
                .is_file()
            {
                format!("{documents}My Games\\FalloutNV_Epic\\")
            } else {
                format!("{documents}My Games\\{}\\", game_name2())
            }
        }
    };
    let fallout = matches!(mode, GameMode::gmFO3 | GameMode::gmFNV);
    let game_ini = match &options.game_ini {
        Some(path) => path.clone(),
        None => {
            let ini = if fallout {
                format!("{my_games}Fallout.ini")
            } else {
                format!("{my_games}{}.ini", game_name())
            };
            // VR games don't create ini file in My Games by default, use the
            // one in the game folder
            if matches!(mode, GameMode::gmTES5VR | GameMode::gmFO4VR | GameMode::gmSF1) && !Path::new(&ini).is_file() {
                format!(
                    "{}\\{}",
                    game_folder(data_path),
                    xedit_core::delphi::path_file_name(&ini)
                )
            } else {
                ini
            }
        }
    };
    let custom_ini = match &options.custom_ini {
        Some(path) => path.clone(),
        None if fallout => format!("{my_games}FalloutCustom.ini"),
        None => format!("{my_games}{}Custom.ini", game_name()),
    };
    if my_games.is_empty() {
        my_games = extract_file_path(&game_ini);
    }
    set_my_games_the_game_path(&my_games);
    set_the_game_ini_file_name(&game_ini);
    set_custom_ini_file_name(&custom_ini);
}

/// Port of the language part of `xeInit.pas` without `-l:`: `sLanguage` of
/// `[General]` (trimmed, in lower case; `iLanguage` of `[Controls]` in
/// Oblivion) of the game ini, then of the custom ini where it has the key;
/// the language when one was found that differs from the current one, else
/// `None`.
pub fn game_ini_language(mode: GameMode) -> Option<String> {
    use xedit_loadorder::ini_files::MemIniFile;
    const MISSING: &str = "\u{1}";
    let oblivion_language = |value: &str| -> String {
        match value.trim().parse::<i64>().unwrap_or(0) {
            1 => "German",
            2 => "French",
            3 => "Spanish",
            4 => "Italian",
            _ => "English",
        }
        .to_owned()
    };
    let mut found = String::new();
    let game_ini = the_game_ini_file_name();
    if Path::new(&game_ini).is_file()
        && let Ok(ini) = MemIniFile::open(Path::new(&game_ini))
    {
        found = if mode == GameMode::gmTES4 {
            oblivion_language(&ini.read_string("Controls", "iLanguage", "0"))
        } else {
            ini.read_string("General", "sLanguage", "").trim().to_lowercase()
        };
    }
    let custom_ini = custom_ini_file_name();
    if Path::new(&custom_ini).is_file()
        && let Ok(ini) = MemIniFile::open(Path::new(&custom_ini))
    {
        if mode == GameMode::gmTES4 {
            let value = ini.read_string("Controls", "iLanguage", MISSING);
            if value != MISSING {
                found = oblivion_language(&value);
            }
        } else {
            let value = ini.read_string("General", "sLanguage", MISSING);
            if value != MISSING {
                found = value.trim().to_lowercase();
            }
        }
    }
    (!found.is_empty() && !found.eq_ignore_ascii_case(&language())).then_some(found)
}

/// Port of what `xeInit.pas` gives `wbLoadBSAs` between the game's own
/// setting and the tool mode's: `LoadBSAs` of `[Options]` in the settings
/// file (`ReadSettings`), then `-skipbsa` or `-forcebsa`.
pub fn apply_load_settings() {
    let settings = crate::modgroups::settings_file();
    if settings.is_file()
        && let Ok(ini) = xedit_loadorder::ini_files::MemIniFile::open(&settings)
    {
        set_load_bsas(ini.read_bool("Options", "LoadBSAs", load_bsas()));
    }
    if let Some(value) = options().load_bsas {
        set_load_bsas(value);
    }
}

/// The exception text of an archive that could not be added, as
/// `Format('[%s] %s', [E.ClassName, E.Message])` shows it. The archive
/// reader's errors are `Exception`s upstream.
fn exception_text(error: &xedit_io::ArchiveError) -> String {
    format!("[Exception] {error}")
}

/// Port of the resource part of `TLoaderThread.Execute`: the archives the
/// game inis name, then those of each module of `modules` (file names, in
/// load order), then the data folder; the cache of the hash names when
/// `wbDecodeTextureHashes` is on; `wbResourcesLoaded`. The containers that
/// were there are dropped first (the GUI creates the handler for its first
/// load). `game_ini` and `custom_ini` are the inis to read
/// (`wbTheGameIniFileName`, `wbCustomIniFileName`), `load` whether the
/// archives found are added (`wbLoadBSAs`). The progress lines are the
/// GUI's. The result is the container list; an archive of the game inis that
/// can not be read ends the load as upstream's does (`Fatal:`).
pub fn load_gui_resources(
    data_path: &str,
    modules: &[String],
    game_ini: &str,
    custom_ini: &str,
    load: bool,
) -> Result<Vec<String>, String> {
    clear_containers();
    let ba2 = archive_extension() == ".ba2";
    let added = |name: &str, version: u32| {
        if ba2 {
            progress(&format!("[{name}] Version: {version}"));
        }
    };

    // Load archives defined in the game ini
    let (mut found, mut missing) = (Vec::new(), Vec::new());
    let mut count = 0;
    if Path::new(game_ini).is_file() {
        count = if Path::new(custom_ini).is_file() {
            find_bsas_custom(
                Path::new(game_ini),
                Path::new(custom_ini),
                data_path,
                &mut found,
                &mut missing,
            )
        } else {
            find_bsas(Path::new(game_ini), data_path, &mut found, &mut missing)
        };
    }
    if count > 0 {
        for name in &found {
            if load {
                progress(&format!("[{name}] Loading Resources."));
                // UPSTREAM-QUIRK: no `try` around these, unlike the
                // archives of the plugins: an archive of the game ini that
                // can not be read ends the whole load.
                let version = add_archive(Path::new(&make_data_file_name(name, data_path))).map_err(|error| {
                    let message = format!("Fatal: <Exception: {error}>");
                    progress(&message);
                    message
                })?;
                added(name, version);
            } else {
                progress(&format!("[{name}] Skipped."));
            }
        }
        for name in &missing {
            progress(&format!("Warning: <Can't find {name}>"));
        }
    }

    // Load archives associated with plugins
    let exact = matches!(game_mode(), GameMode::gmTES5 | GameMode::gmEnderal);
    for module in modules {
        let (mut found, mut missing) = (Vec::new(), Vec::new());
        // all games except old Skyrim load BSA files with partial matching,
        // Skyrim requires exact names match and can use a private ini to
        // specify the bsa to use.
        if has_bsas(
            &change_file_ext(module, ""),
            data_path,
            exact,
            globals::is_skyrim(),
            &mut found,
            &mut missing,
        ) > 0
        {
            for name in &found {
                if load {
                    progress(&format!("[{name}] Loading Resources."));
                    match add_archive(Path::new(&make_data_file_name(name, data_path))) {
                        Ok(version) => added(name, version),
                        Err(error) => progress(&format!(
                            "[{name}] Could not be loaded. <Error: {}>",
                            exception_text(&error)
                        )),
                    }
                } else {
                    progress(&format!("[{name}] Skipped."));
                }
            }
            for name in &missing {
                progress(&format!("Warning: <Can't find {name}>"));
            }
        }
    }
    progress(&format!("[{data_path}] Setting Resource Path."));
    add_folder(Path::new(data_path));

    if decode_texture_hashes() {
        progress("Start building resources cache...");
        ensure_cache();
        progress("...resources cache finished building");
    }
    wb_resources_loaded();
    install_localization_handler();
    Ok(container_list())
}

/// The modules of the load order, as the GUI's load list names them: every
/// loaded file but the hardcoded one, in load order.
pub fn load_order_modules() -> Vec<String> {
    let mut files: Vec<(i32, String)> = xedit_core::interface::element::files()
        .iter()
        .filter(|file| {
            !file
                .get_file_states()
                .contains(xedit_core::interface::FileState::fsIsHardcoded)
        })
        .map(|file| (file.get_load_order(), file.get_name()))
        .collect();
    files.sort_by_key(|(load_order, _)| *load_order);
    files.into_iter().map(|(_, name)| name).collect()
}

/// The GUI's resources of a session that was just loaded from `data_path`:
/// [`load_gui_resources`] with the game inis and `wbLoadBSAs`, then
/// `wbLoaderDone` (the GUI sets it when the loader thread reports back,
/// `WMUserLoaderDone`), from which file and folder hashes display their
/// names.
pub fn load_session_resources(data_path: &str) -> Result<Vec<String>, String> {
    set_data_path(data_path);
    let containers = load_gui_resources(
        data_path,
        &load_order_modules(),
        &the_game_ini_file_name(),
        &custom_ini_file_name(),
        load_bsas(),
    )?;
    set_loader_done(true);
    Ok(containers)
}

/// Port of the resource loading of `xDump.dpr`: when the plugin or one of
/// its masters is localized and a loose `.STRINGS` table of one of them is
/// missing (unless `-nobsa`), or with `-bsa`/`-allbsa`, the archives of each
/// master and of the plugin are added (`<name>`, `<name> - Interface`,
/// `<name> - Localization`, `<name> - Wwise*`; with `-allbsa` every
/// `<name>*` archive, exactly and with the plugin's ini in the Skyrim
/// games), then the data folder. xDump has no game ini (it never sets
/// `wbTheGameIniFileName`), so `-allbsa` adds none of its archives, and
/// `wbLoaderDone` stays off.
pub(crate) fn load_dump_resources(file: &FileImpl, path: &str, data_path: &str, mode: GameMode) {
    let switches = *DUMP_SWITCHES.lock().unwrap();
    set_load_bsas(mode != GameMode::gmTES3 && (switches.bsa || switches.all_bsa));
    set_load_all_bsas(switches.all_bsa);
    let data_path = data_path.to_owned();
    set_data_path(&data_path);
    // The loose strings files are looked for next to the file, which for a
    // save is not the data folder.
    let file_path = extract_file_path(path);
    clear_containers();
    let mut names: Vec<String> = file.masters().iter().map(|master| master.get_name()).collect();
    names.push(file.get_name());
    let is_localized = file.get_is_localized() || file.masters().iter().any(|master| master.get_is_localized());
    if is_localized && !load_bsas() && !switches.no_bsa {
        let missing = names.iter().any(|name| {
            let strings = format!(
                "{file_path}Strings\\{}_{}.STRINGS",
                change_file_ext(name, ""),
                language()
            );
            !Path::new(&strings).is_file()
        });
        if missing {
            set_load_bsas(true);
        }
    }
    if load_bsas() {
        let add_found = |stem: &str, exact: bool, mod_ini: bool| {
            let (mut found, mut missing) = (Vec::new(), Vec::new());
            if has_bsas(stem, &data_path, exact, mod_ini, &mut found, &mut missing) > 0 {
                for name in &found {
                    add_resource_archive(name, &make_data_file_name(name, &data_path));
                }
            }
        };
        let skyrim = matches!(
            mode,
            GameMode::gmTES5 | GameMode::gmEnderal | GameMode::gmTES5VR | GameMode::gmSSE
        );
        for name in &names {
            let stem = change_file_ext(name, "");
            if load_all_bsas() {
                add_found(&stem, skyrim, skyrim);
            } else {
                add_found(&stem, true, false);
                add_found(&format!("{stem} - Interface"), true, false);
                add_found(&format!("{stem} - Localization"), true, false);
                add_found(&format!("{stem} - Wwise"), false, false);
            }
        }
    }
    progress(&format!("[{data_path}] Setting Resource Path."));
    add_folder(Path::new(&data_path));
    wb_resources_loaded();
    install_localization_handler();
}

fn add_resource_archive(name: &str, path: &str) {
    progress(&format!("[{name}] Loading Resources."));
    if let Err(error) = add_archive(Path::new(path)) {
        progress(&error.to_string());
    }
}

/// The game's own `wbLoadBSAs` (`xeInit.pas`, `case wbGameMode of`): off
/// for Fallout 3, New Vegas, Morrowind and Oblivion Remastered.
pub(crate) fn game_loads_bsas(mode: GameMode) -> bool {
    !matches!(
        mode,
        GameMode::gmFNV | GameMode::gmFO3 | GameMode::gmTES3 | GameMode::gmTES4R
    )
}
