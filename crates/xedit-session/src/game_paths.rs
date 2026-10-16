// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeInit.pas (CheckAppPath, GetCSIDLShellFolder,
// DoInitPath), Core/wbSteamVDFParser.pas (TSteamVDFParser, TSteamVDFNode,
// GetSteamInstallFolder, GetSteamLibraryListFile, GetInstallPathBySteamID)

//! The paths `xeInit.pas` finds for a game when the command line does not
//! name them: the data folder (`CheckAppPath`, then the Steam library, then
//! the registry), the game's own `Plugins.txt`, the settings file, the save
//! folder and the backup and cache folders.
//!
//! Only what `DoInitPath` needs is here; the inis below `My Games` are in
//! [`crate::resources`].

use std::path::{Path, PathBuf};

use xedit_core::interface::globals::GameMode;
use xedit_io::archive::include_trailing_path_delimiter;

/// The Steam app ids and the registry name of a game, as `_DoInit` sets them
/// (`wbGameSteamID` and `wbGameNameReg`).
struct GameInfo {
    /// `wbGameSteamID`: the ids `GetInstallPathBySteamID` is asked for, in
    /// the order upstream tries them.
    steam_ids: &'static [&'static str],
    /// `wbGameNameReg`, when it differs from `wbGameName2`.
    name_reg: Option<&'static str>,
}

/// `wbGameName`, `wbGameExeName` (without `.exe`) and `wbAppName` of a game,
/// the names `_DoInit` sets before `DoInitPath` reads them. They are the
/// values the definition setup uses (`dump::setup`).
pub fn game_name(mode: GameMode) -> &'static str {
    match mode {
        GameMode::gmTES3 => "Morrowind",
        GameMode::gmTES4 | GameMode::gmTES4R => "Oblivion",
        GameMode::gmFO3 => "Fallout3",
        GameMode::gmFNV => "FalloutNV",
        GameMode::gmTES5 | GameMode::gmTES5VR | GameMode::gmSSE => "Skyrim",
        GameMode::gmEnderal | GameMode::gmEnderalSE => "Enderal",
        GameMode::gmFO4 | GameMode::gmFO4VR => "Fallout4",
        GameMode::gmFO76 => "Fallout76",
        GameMode::gmSF1 => "Starfield",
    }
}

/// `wbGameExeName`, with its extension: the game's own executable, which
/// `CheckAppPath` looks for and the hardcoded records are named after.
pub fn game_exe_name(mode: GameMode) -> String {
    let name = match mode {
        GameMode::gmTES5 | GameMode::gmEnderal => "TESV".to_owned(),
        GameMode::gmTES5VR => "SkyrimVR".to_owned(),
        GameMode::gmSSE | GameMode::gmEnderalSE => "SkyrimSE".to_owned(),
        GameMode::gmFO4VR => "Fallout4VR".to_owned(),
        GameMode::gmTES4R => "Oblivion Remastered".to_owned(),
        _ => game_name(mode).to_owned(),
    };
    format!("{name}.exe")
}

/// `wbGameName2`, the title of the game: the folder it has below `My Games`
/// and in the local application data folder.
pub fn game_name2(mode: GameMode) -> &'static str {
    match mode {
        GameMode::gmTES4R => "Oblivion Remastered",
        GameMode::gmEnderalSE => "Enderal Special Edition",
        GameMode::gmTES5VR => "Skyrim VR",
        GameMode::gmSSE => "Skyrim Special Edition",
        GameMode::gmFO4VR => "Fallout4VR",
        GameMode::gmFO76 => "Fallout 76",
        _ => game_name(mode),
    }
}

/// `wbAppName`: the name of the program and of its files (`TES4Edit.ini`,
/// `TES4ExportPlugins.txt`).
pub fn app_name(mode: GameMode) -> &'static str {
    match mode {
        GameMode::gmTES3 => "TES3",
        GameMode::gmTES4 | GameMode::gmTES4R => "TES4",
        GameMode::gmFO3 => "FO3",
        GameMode::gmFNV => "FNV",
        GameMode::gmTES5 => "TES5",
        GameMode::gmEnderal => "Enderal",
        GameMode::gmTES5VR => "TES5VR",
        GameMode::gmSSE => "SSE",
        GameMode::gmEnderalSE => "EnderalSE",
        GameMode::gmFO4 => "FO4",
        GameMode::gmFO4VR => "FO4VR",
        GameMode::gmFO76 => "FO76",
        GameMode::gmSF1 => "SF1",
    }
}

/// The values of `_DoInit` the path discovery reads, per game.
fn game_info(mode: GameMode) -> GameInfo {
    match mode {
        GameMode::gmTES3 => GameInfo {
            steam_ids: &["22320"],
            name_reg: None,
        },
        GameMode::gmTES4 => GameInfo {
            steam_ids: &["22330", "900883"],
            name_reg: None,
        },
        GameMode::gmTES4R => GameInfo {
            steam_ids: &["2623190"],
            name_reg: Some("Steam App 2623190"),
        },
        GameMode::gmFO3 => GameInfo {
            steam_ids: &["22370", "22300"],
            name_reg: None,
        },
        GameMode::gmFNV => GameInfo {
            steam_ids: &["22380", "22490"],
            name_reg: None,
        },
        GameMode::gmTES5 => GameInfo {
            steam_ids: &["72850"],
            name_reg: None,
        },
        GameMode::gmEnderal => GameInfo {
            steam_ids: &["933480"],
            name_reg: None,
        },
        GameMode::gmTES5VR => GameInfo {
            steam_ids: &["611670"],
            name_reg: None,
        },
        GameMode::gmSSE => GameInfo {
            steam_ids: &["489830"],
            name_reg: None,
        },
        GameMode::gmEnderalSE => GameInfo {
            steam_ids: &["976620"],
            name_reg: Some("EnderalSE"),
        },
        GameMode::gmFO4 => GameInfo {
            steam_ids: &["377160"],
            name_reg: None,
        },
        GameMode::gmFO4VR => GameInfo {
            steam_ids: &["611660"],
            name_reg: Some("Fallout 4 VR"),
        },
        GameMode::gmFO76 => GameInfo {
            steam_ids: &["1151340"],
            name_reg: Some("Steam App 1151340"),
        },
        GameMode::gmSF1 => GameInfo {
            steam_ids: &["1716740"],
            name_reg: Some("Steam App 1716740"),
        },
    }
}

/// The registry name of the game (`wbGameNameReg`, else `wbGameName2`).
fn registry_name(mode: GameMode) -> String {
    game_info(mode)
        .name_reg
        .map_or_else(|| game_name2(mode).to_owned(), str::to_owned)
}

/// `DataName[wbGameMode = gmTES3]`: the data folder is `Data Files` for
/// Morrowind and `Data` for every other game.
pub fn data_folder_name(mode: GameMode) -> &'static str {
    if mode == GameMode::gmTES3 { "Data Files" } else { "Data" }
}

/// `GetCSIDLShellFolder(CSIDL_LOCAL_APPDATA)`: the local application data
/// folder with a trailing separator, or an empty string.
pub fn local_app_data_folder() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::{CSIDL_LOCAL_APPDATA, SHGetFolderPathW};
        let mut buffer = [0u16; 260];
        // SAFETY: the buffer holds MAX_PATH characters, as the function needs.
        let result = unsafe {
            SHGetFolderPathW(
                std::ptr::null_mut(),
                CSIDL_LOCAL_APPDATA as i32,
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
            include_trailing_path_delimiter(&path)
        }
    }
    #[cfg(not(windows))]
    {
        std::env::var("XDG_DATA_HOME")
            .map(|home| include_trailing_path_delimiter(&home))
            .unwrap_or_default()
    }
}

/// Upstream `CheckAppPath`: the folder that holds the game's executable and
/// its data folder, found from the current folder upwards and then from the
/// folder of the program, or an empty string.
pub fn check_app_path(mode: GameMode) -> String {
    let data = data_folder_name(mode);
    let exe = game_exe_name(mode);
    let check = |start: &Path| -> String {
        let mut folder = Some(start.to_path_buf());
        while let Some(s) = folder {
            // `while Length(s) > 3 do`: a drive root or a UNC share is the
            // end of the walk.
            if s.as_os_str().len() <= 3 {
                break;
            }
            if s.join(&exe).is_file() && s.join(data).is_dir() {
                return include_trailing_path_delimiter(&s.to_string_lossy());
            }
            folder = s.parent().map(Path::to_path_buf);
        }
        String::new()
    };
    let current = std::env::current_dir().unwrap_or_default();
    let found = check(&current);
    if !found.is_empty() {
        return found;
    }
    let Some(exe_path) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    else {
        return String::new();
    };
    if exe_path == current {
        return String::new();
    }
    check(&exe_path)
}

/// The value of a registry string under `HKLM` or `HKCU`, read like Delphi's
/// `TRegistry` reads it: the 32-bit view first, then the 64-bit one.
#[cfg(windows)]
fn registry_string(root: RegistryRoot, key: &str, name: &str) -> String {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_SZ, RegCloseKey,
        RegOpenKeyExW, RegQueryValueExW,
    };
    let root = match root {
        RegistryRoot::LocalMachine => HKEY_LOCAL_MACHINE,
        RegistryRoot::CurrentUser => HKEY_CURRENT_USER,
    };
    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    // `TRegistry.OpenKey` takes a leading backslash, `RegOpenKeyExW` refuses
    // it (ERROR_BAD_PATHNAME), as the constants of `DoInitPath` carry one
    // (`\SOFTWARE\Bethesda Softworks\...`).
    let key = key.trim_start_matches('\\');
    let key_wide = wide(key);
    let name_wide = wide(name);
    for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
        let mut handle = std::ptr::null_mut();
        // SAFETY: an open key handle is written to `handle`; the key and the
        // value names are NUL terminated and outlive the calls.
        let opened = unsafe { RegOpenKeyExW(root, key_wide.as_ptr(), 0, KEY_READ | view, &mut handle) };
        if opened != 0 {
            continue;
        }
        let mut kind = 0u32;
        let mut size = 0u32;
        // The size of the value first, as `ReadString` does.
        let queried = unsafe {
            RegQueryValueExW(
                handle,
                name_wide.as_ptr(),
                std::ptr::null_mut(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        let value = if queried == 0 && kind == REG_SZ && size > 0 {
            let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
            let mut size = size;
            let read = unsafe {
                RegQueryValueExW(
                    handle,
                    name_wide.as_ptr(),
                    std::ptr::null_mut(),
                    &mut kind,
                    buffer.as_mut_ptr().cast::<u8>(),
                    &mut size,
                )
            };
            if read == 0 {
                let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
                String::from_utf16_lossy(&buffer[..length])
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        // SAFETY: the handle came from `RegOpenKeyExW` and is closed once.
        unsafe { RegCloseKey(handle) };
        if !value.is_empty() || queried == 0 {
            return value;
        }
    }
    String::new()
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum RegistryRoot {
    LocalMachine,
    CurrentUser,
}

/// `GetSteamInstallFolder`: the Steam folder from the registry, when it
/// exists there.
fn steam_install_folder() -> String {
    #[cfg(windows)]
    {
        let path = registry_string(RegistryRoot::LocalMachine, r"\SOFTWARE\Valve\Steam\", "InstallPath");
        let path = path.replace('"', "");
        if Path::new(&path).is_dir() {
            return include_trailing_path_delimiter(&path);
        }
    }
    String::new()
}

/// One node of a Steam VDF or ACF file (`TSteamVDFNode`).
#[derive(Debug, Default)]
struct VdfNode {
    name: String,
    value: String,
    children: Vec<VdfNode>,
}

impl VdfNode {
    /// `TSteamVDFNode.FindChildByName`: `SameText`, the first match.
    fn find_child(&self, name: &str) -> Option<&VdfNode> {
        self.children.iter().find(|node| node.name.eq_ignore_ascii_case(name))
    }

    /// `TSteamVDFNode.FindNodeByPath`.
    fn find_by_path(&self, path: &str) -> Option<&VdfNode> {
        let mut node = Some(self);
        for key in path.split('\\') {
            node = Some(node.unwrap_or(self).find_child(key)?);
        }
        node
    }

    /// `TSteamVDFNode.GetValueByPath`.
    fn value_by_path(&self, path: &str) -> String {
        self.find_by_path(path)
            .map(|node| node.value.clone())
            .unwrap_or_default()
    }
}

/// `TSteamVDFParser.ParseNode`: the lines of a VDF file read into a tree.
/// The parsing assumes the Steam format: a `}` closes a node, a `{` on the
/// line after a name makes it a parent, and a key/value line is separated by
/// a tab.
fn parse_vdf_node(lines: &[String], index: &mut usize, parent: &mut VdfNode) {
    while *index < lines.len() {
        let line = lines[*index].trim().to_owned();
        let next = lines
            .get(*index + 1)
            .map(|line| line.trim().to_owned())
            .unwrap_or_default();
        *index += 1;
        if line == "}" {
            break;
        }
        let mut node = VdfNode::default();
        if next.contains('{') {
            node.name = line.replace('"', "");
            *index += 1;
            parse_vdf_node(lines, index, &mut node);
        } else {
            // `Pos(#9, Line)`: a line without a tab gives an empty name and
            // the whole line as the value, as upstream reads it.
            let (name, value) = match line.find('\t') {
                Some(tab) => (line[..tab].trim(), line[tab + 1..].trim()),
                None => ("", line.as_str()),
            };
            node.name = name.replace('"', "");
            node.value = value.replace('"', "").replace("\\\\", "\\");
        }
        parent.children.push(node);
    }
}

/// `TSteamVDFParser.LoadFromFile`: the file as a tree, or `None` when it
/// cannot be read. `TStringList.LoadFromFile` reads it in the system's code
/// page.
fn load_vdf(path: &str) -> Option<VdfNode> {
    let bytes = std::fs::read(path).ok()?;
    let text = xedit_io::Encoding::Mbcs(1252)
        .get_string(&bytes)
        .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut root = VdfNode::default();
    let mut index = 0usize;
    parse_vdf_node(&lines, &mut index, &mut root);
    Some(root)
}

/// `GetInstallPathBySteamID`: the folder of the Steam app, or an empty
/// string.
fn install_path_by_steam_id(steam_id: &str) -> String {
    if steam_id.is_empty() {
        return String::new();
    }
    let steam = steam_install_folder();
    if steam.is_empty() {
        return String::new();
    }
    let library_file = format!("{steam}SteamApps\\libraryfolders.vdf");
    if !Path::new(&library_file).is_file() {
        return String::new();
    }
    let Some(parser) = load_vdf(&library_file) else {
        return String::new();
    };
    let Some(root) = parser.find_by_path("libraryfolders") else {
        return String::new();
    };
    let mut library_folder = String::new();
    for node in &root.children {
        if node.find_by_path(&format!("apps\\{steam_id}")).is_some() {
            library_folder = node.value_by_path("path");
            break;
        }
    }
    if library_folder.is_empty() {
        return String::new();
    }
    let apps = format!("{library_folder}\\SteamApps\\");
    let manifest = format!("{apps}appmanifest_{steam_id}.acf");
    if !Path::new(&manifest).is_file() {
        return String::new();
    }
    let Some(parser) = load_vdf(&manifest) else {
        return String::new();
    };
    let folder = parser.value_by_path("AppState\\installdir");
    if folder.is_empty() {
        return String::new();
    }
    let full = format!("{apps}common\\{folder}");
    if Path::new(&full).is_dir() { full } else { String::new() }
}

/// The registry key and value `DoInitPath` reads for a game: the Bethesda
/// key with `Installed Path` for Morrowind to Fallout 4 VR, SureAI with
/// `Install_Path` for Enderal and the uninstall key with `InstallLocation`
/// for Fallout 76, Starfield and Oblivion Remastered.
pub fn registry_lookup(mode: GameMode) -> (&'static str, String, &'static str) {
    let name = registry_name(mode);
    let (root, key, value) = match mode {
        GameMode::gmEnderal | GameMode::gmEnderalSE => (
            "HKEY_CURRENT_USER",
            format!(r"\Software\SureAI\{name}\"),
            "Install_Path",
        ),
        GameMode::gmFO76 | GameMode::gmSF1 | GameMode::gmTES4R => (
            "HKEY_LOCAL_MACHINE",
            format!(r"\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{name}\"),
            "InstallLocation",
        ),
        _ => (
            "HKEY_LOCAL_MACHINE",
            format!(r"\SOFTWARE\Bethesda Softworks\{name}\"),
            "Installed Path",
        ),
    };
    (root, key, value)
}

/// Upstream's `Fatal: Could not determine <game> installation path, no
/// "<value>" registry key` with the hint it shows beside it.
pub fn missing_install_message(mode: GameMode) -> String {
    let (_, _, value) = registry_lookup(mode);
    format!(
        "Fatal: Could not determine {} installation path, no \"{value}\" registry key\r\nThis can happen after Steam updates, run the game's launcher to restore registry settings",
        game_name2(mode)
    )
}

/// The game folder from the registry, or an empty string (`DoInitPath`).
fn registry_install_path(mode: GameMode) -> String {
    #[cfg(windows)]
    {
        let (_, key, value) = registry_lookup(mode);
        let root = if mode == GameMode::gmEnderal || mode == GameMode::gmEnderalSE {
            RegistryRoot::CurrentUser
        } else {
            RegistryRoot::LocalMachine
        };
        registry_string(root, &key, value).replace('"', "")
    }
    #[cfg(not(windows))]
    {
        let _ = mode;
        String::new()
    }
}

/// The game folder as `CheckAppPath`, the Steam library and the registry
/// find it, or an empty string. The caller prints the `Fatal: ...` message
/// when it is empty.
pub fn discover_game_folder(mode: GameMode) -> String {
    let folder = check_app_path(mode);
    if !folder.is_empty() {
        return folder;
    }
    for steam_id in game_info(mode).steam_ids {
        let folder = install_path_by_steam_id(steam_id);
        if !folder.is_empty() {
            return include_trailing_path_delimiter(&folder);
        }
    }
    let folder = registry_install_path(mode);
    if folder.is_empty() {
        return String::new();
    }
    include_trailing_path_delimiter(&folder)
}

/// The data folder of a game folder: `<game>\Data\` (Morrowind's `Data
/// Files`), which `DoInitPath` appends to what it found.
pub fn data_path_of(game_folder: &str, mode: GameMode) -> String {
    if game_folder.is_empty() {
        return String::new();
    }
    format!(
        "{}{}\\",
        include_trailing_path_delimiter(game_folder),
        data_folder_name(mode)
    )
}

/// The game's own `Plugins.txt` (`DoInitPath`): the local application data
/// folder, the game's folder name in it and `Plugins.txt`. Fallout 76 uses
/// `wbGameName`, every other game `wbGameName2`.
pub fn default_plugins_file(mode: GameMode, data_path: &str) -> String {
    let local = local_app_data_folder();
    if local.is_empty() {
        return String::new();
    }
    let folder = match mode {
        GameMode::gmFO76 => game_name(mode),
        _ => game_name2(mode),
    };
    // The Epic release of New Vegas keeps its files below `My Games` and the
    // local application data folder as `FalloutNV_Epic` (`xeInit.pas:506`).
    if mode == GameMode::gmFNV
        && Path::new(&format!(
            "{}EOSSDK-Win32-Shipping.dll",
            crate::resources::game_folder(data_path)
        ))
        .is_file()
    {
        return format!("{local}FalloutNV_Epic\\Plugins.txt");
    }
    format!("{local}{folder}\\Plugins.txt")
}

/// `xeSettingsFileName` of `DoInitPath`: `<program path><AppName><ToolName>.ini`
/// when that file exists, else `Plugins.<app>viewsettings` next to the
/// `Plugins.txt` found above.
pub fn settings_file_name(app_name: &str, tool_name: &str, program_path: &str, plugins_file: &str) -> String {
    let name = format!(
        "{}{}{}.ini",
        include_trailing_path_delimiter(program_path),
        app_name,
        tool_name
    );
    if Path::new(&name).is_file() {
        return name;
    }
    let plugin_path = PathBuf::from(plugins_file);
    let file = plugin_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = plugin_path
        .parent()
        .map(|folder| folder.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!(
        "{}{}.{}viewsettings",
        if folder.is_empty() {
            String::new()
        } else {
            include_trailing_path_delimiter(&folder)
        },
        file,
        app_name.to_lowercase()
    )
}

/// `wbBackupPath` of `DoInitPath`: `-B:` else `<data>\<AppName>Edit
/// Backups\`, and an empty string when the run does not save
/// (`wbDontSave`).
pub fn default_backup_path(data_path: &str, app_name: &str) -> String {
    if data_path.is_empty() {
        return String::new();
    }
    format!(
        "{}{}Edit Backups\\",
        include_trailing_path_delimiter(data_path),
        app_name
    )
}

/// `wbCachePath` of `DoInitPath`: `-C:` else `<data>\<AppName>Edit Cache\`,
/// and an empty string when there is no data folder (the caller then turns
/// the cache off, as upstream sets `wbDontCache`).
pub fn default_cache_path(data_path: &str, app_name: &str) -> String {
    if data_path.is_empty() {
        return String::new();
    }
    format!("{}{}Edit Cache\\", include_trailing_path_delimiter(data_path), app_name)
}

/// The save folder of `DoInitPath`: `-G:`, else the `SLocalSavePath` of the
/// game's ini (`Saves\` when it has none), the custom ini's value winning,
/// resolved against the folder below `My Games`.
pub fn save_path(my_games: &str, game_ini: &str, custom_ini: &str) -> String {
    let mut relative = "Saves\\".to_owned();
    if Path::new(game_ini).is_file()
        && let Some(value) = ini_value(game_ini, "General", "SLocalSavePath")
    {
        relative = value;
    }
    if Path::new(custom_ini).is_file()
        && let Some(value) = ini_value(custom_ini, "General", "SLocalSavePath")
    {
        relative = value;
    }
    // `PathRelativeToFull`: the ini's value is relative to the My Games
    // folder of the game.
    if my_games.is_empty() {
        return include_trailing_path_delimiter(&relative);
    }
    let base = include_trailing_path_delimiter(my_games);
    // `ExpandFileName`: a relative path is joined, an absolute one kept.
    let joined = if Path::new(&relative).is_absolute() {
        relative
    } else {
        format!("{base}{relative}")
    };
    include_trailing_path_delimiter(&joined)
}

/// `TMemIniFile`/`GetPrivateProfileString` of one value of a game ini: the
/// section and the key without case. The file is read in the system's code
/// page, as Delphi reads it.
pub fn ini_value(path: &str, section: &str, key: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let text = xedit_io::Encoding::Mbcs(1252)
        .get_string(&bytes)
        .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
            in_section = name.trim().eq_ignore_ascii_case(section);
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((name, value)) = line.split_once('=')
            && name.trim().eq_ignore_ascii_case(key)
        {
            return Some(value.trim().to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vdf_parser_reads_the_shape_of_a_library_file() {
        let lines: Vec<String> = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps"
		{
			"22330"		"1234"
		}
	}
	"1"
	{
		"path"		"M:\\SteamLibrary"
		"apps"
		{
			"377160"		"5678"
		}
	}
}
"#
        .lines()
        .map(str::to_owned)
        .collect();
        let mut root = VdfNode::default();
        let mut index = 0;
        parse_vdf_node(&lines, &mut index, &mut root);
        let library = root.find_by_path("libraryfolders").expect("the root node");
        assert_eq!(library.children.len(), 2);
        let one = library.find_child("1").expect("the second library");
        assert_eq!(one.value_by_path("path"), r"M:\SteamLibrary");
        assert!(one.find_by_path("apps\\377160").is_some());
        assert!(one.find_by_path("apps\\22330").is_none());
    }

    #[test]
    fn a_save_path_is_read_from_the_ini() {
        let dir = xedit_io::temp::TempDir::new(format!("xedit-game-paths-{}", std::process::id()));
        let ini = dir.join("game.ini");
        std::fs::write(&ini, "[General]\r\nSLocalSavePath=MySaves\\\r\n").unwrap();
        assert_eq!(
            ini_value(&ini.to_string_lossy(), "General", "slocalsavepath").as_deref(),
            Some("MySaves\\")
        );
        let my_games = format!("{}\\", dir.display());
        assert_eq!(
            save_path(&my_games, &ini.to_string_lossy(), ""),
            format!("{my_games}MySaves\\")
        );
        // Without the key the default of `DoInitPath` stands.
        std::fs::write(&ini, "[General]\r\n").unwrap();
        assert_eq!(
            save_path(&my_games, &ini.to_string_lossy(), ""),
            format!("{my_games}Saves\\")
        );
    }
}
