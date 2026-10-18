// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbLoadOrder.pas (TwbModuleInfo, wbLoadModules,
// wbModuleByName, wbModulesByLoadOrder, _ModulesLoadOrderCompare,
// GetCRC32, HasCRC32)

//! The module information of `wbLoadOrder.pas` as far as the mod groups
//! read it: a module for every plugin of the data folder (and the game's
//! executable, the hardcoded file), the loaded file of a module with its
//! load order, the CRC32 of a module's file, and the keys of upstream's
//! load order (`miOfficialIndex`, `miCCIndex`, `miPluginsTxtIndex`,
//! `miDateTime`, the ESM flag), so that the modules are ordered as
//! `wbModulesByLoadOrder` orders them.
//!
//! [`Modules::load_data_folder`] is `wbLoadModules` without loaded files:
//! the modules of the data folder, the masters of each (a module whose
//! masters are missing is not active, `mfMastersMissing`), `Plugins.txt`
//! (`miPluginsTxtIndex`, the active modules), the modules the release makes
//! active without the plugin list naming them (the game master,
//! `Update.esm` of the Skyrim games, `wbOfficialDLC` and the game's
//! Creation Club list), `loadorder.txt` for Oblivion Remastered, Skyrim and
//! Enderal (`miCombinedIndex`, `_ModulesLoadOrderCompareCombined`), and the
//! load order. [`Modules::active_load_order`] is `SimulateLoad`: the active
//! modules with the masters they need, in the order the session loads them.
//! A plugin of the data folder counts as a module when its first record is
//! a file header (upstream reads its masters and flags,
//! `wbMastersForFile`); the header flags that the order reads (the ESM and
//! blueprint bits) and its `MAST` names are read here.
//!
//! A session of the CLI loads the plugins it is given, and the CLI reads no
//! plugin list unless `--plugins-list` names one (`wbPluginsFileName`), so
//! [`Modules::load`] leaves `miPluginsTxtIndex` at the highest integer
//! without one, as `wbLoadModules` does when the file is not there.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use xedit_core::implementation::FileImpl;
use xedit_core::interface::Element;
use xedit_core::interface::globals::{
    GameMode, always_load_game_master, creation_club_content_file_name, game_exe_name, game_master_esm, game_mode,
    official_dlc, plugins_file_name,
};

use crate::ini_files::same_text;
use crate::mod_groups::compare_text;

/// Port of `TwbModuleInfo`, the fields the mod groups read.
pub struct ModuleInfo {
    /// `miName`: the file name, without `.ghost`.
    pub name: String,
    /// `miOriginalName`: the name of the file in the data folder.
    pub original_name: String,
    /// `miFile`: the loaded file of the module.
    pub file: Option<Arc<FileImpl>>,
    /// `miLoadOrder`: the load order of the loaded file, `i32::MAX` for a
    /// module that is not loaded.
    pub load_order: i32,
    /// `mfValid`: a plugin whose header could be read (not the hardcoded
    /// executable).
    pub valid: bool,
    /// `mfIsESM`: the ESM extension of the later games or the header flag
    /// (`esm_by_extension`).
    pub is_esm: bool,
    /// `mfHasBlueprintFlag`: the header flag 11, a Starfield blueprint.
    pub has_blueprint: bool,
    /// `miOfficialIndex`: the place of the game master, the game's
    /// executable and its official DLC (and `Update.esm` of the Skyrim
    /// family), `High(Integer)` for every other module.
    pub official_index: i32,
    /// `miCCIndex`: the place of the module in the game's Creation Club
    /// list, from 1, `High(Integer)` for a module the list does not name.
    pub cc_index: i32,
    /// `miPluginsTxtIndex`: the line of the module in the plugin list,
    /// `High(Integer)` for a module the list does not name.
    pub plugins_txt_index: i32,
    /// `miLoadOrderTxtIndex`: the line of the module in `loadorder.txt`,
    /// `High(Integer)` for a module the file does not name.
    pub load_order_txt_index: i32,
    /// `miCombinedIndex`: the position of the module the combined sort of
    /// `loadorder.txt` gives it, `High(Integer)` before that sort.
    pub combined_index: i32,
    /// `mfHasIndex`: the module has one of the indices the sort reads (the
    /// plugin list, the official order, the Creation Club order, or an
    /// index `loadorder.txt` inherited). `TwbModuleInfo.HasIndex` also
    /// requires `mfValid`.
    pub has_index: bool,
    /// `mfMastersMissing`: one of the module's masters is not a module of
    /// the data folder, or a master's own masters are missing.
    pub masters_missing: bool,
    /// `mfActive`: the module is active (`mfActiveInPluginsTxt` or one of
    /// `wbLoadModules`' implicit actives); `Modules::active_load_order`
    /// loads it.
    pub active: bool,
    /// `mfActiveInPluginsTxt`: the plugin list names the module with no
    /// `#` before its active mark. The module select dialog's `<plugins.txt>`
    /// preset checks these (and the official modules) rather than every
    /// implicitly active one.
    pub active_in_plugins_txt: bool,
    /// `miMasterNames`: the file names the module's file header lists.
    pub master_names: Vec<String>,
    /// `miDateTime`: the last write time of the module's file in the data
    /// folder, as a Delphi `TDateTime`.
    pub date_time: f64,
    /// The folder the module's file is in, with a trailing separator.
    data_path: String,
    /// `miCRC32` of a module that is not loaded, read once.
    crc32: OnceLock<u32>,
}

impl ModuleInfo {
    /// `mfHasFile`: the module's file is loaded.
    pub fn has_file(&self) -> bool {
        self.file.is_some()
    }

    /// The CRC32 of the module's file: the loaded file's (`TwbFile.CRC32`,
    /// of the bytes as loaded or last saved), else the file in the data
    /// folder's (`TwbHash.CRC32(wbDataPath + miOriginalName)`, 0 when it
    /// can not be read).
    fn crc32_value(&self) -> u32 {
        match &self.file {
            Some(file) => file.crc32(),
            None => *self.crc32.get_or_init(|| {
                std::fs::read(format!("{}{}", self.data_path, self.original_name))
                    .map(|bytes| xedit_io::crc32(&bytes))
                    .unwrap_or(0)
            }),
        }
    }

    /// Port of `GetCRC32`: the CRC32, when it is valid (`TwbCRC32.IsValid`:
    /// neither 0 nor `FFFFFFFF`).
    pub fn get_crc32(&self) -> Option<u32> {
        let crc = self.crc32_value();
        is_valid_crc32(crc).then_some(crc)
    }

    /// Port of `HasCRC32`.
    pub fn has_crc32(&self, crc: u32) -> bool {
        self.crc32_value() == crc
    }
}

/// `TwbCRC32.IsValid`: neither `IsNull` nor `IsNone`.
pub fn is_valid_crc32(crc: u32) -> bool {
    crc != 0 && crc != 0xFFFF_FFFF
}

/// The modules in load order (`wbModulesByLoadOrder`).
#[derive(Default)]
pub struct Modules {
    pub modules: Vec<ModuleInfo>,
}

/// Whether a file name has the extension of a module (`miExtension` other
/// than `meUnknown`).
fn is_module_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".esm")
        || lower.ends_with(".esp")
        || lower.ends_with(".esu")
        || (lower.ends_with(".esl") && xedit_core::interface::globals::is_light_supported())
}

/// `wbIsSkyrim`.
fn is_skyrim() -> bool {
    use GameMode::*;
    matches!(game_mode(), gmTES5 | gmEnderal | gmTES5VR | gmSSE | gmEnderalSE)
}

/// What the load order reads of a file header: the ESM flag of the games
/// that take it from the header, the Starfield blueprint flag, and the
/// masters (`TwbFile.GetMasters`).
struct ModuleHeader {
    is_esm: bool,
    has_blueprint: bool,
    masters: Vec<String>,
}

/// The size of a main record's header, `wbSizeOfMainRecordStruct`: 16 for
/// Morrowind, 20 for Oblivion and Oblivion Remastered, 24 for the later
/// games.
fn main_record_header_size() -> usize {
    match game_mode() {
        GameMode::gmTES3 => 16,
        GameMode::gmTES4 | GameMode::gmTES4R => 20,
        _ => 24,
    }
}

/// The header of a file whose flags and masters are read, as
/// `wbMastersForFile` reads them; `None` for a file that does not start
/// with a file header record.
fn read_module_header(path: &Path) -> Option<ModuleHeader> {
    use std::io::Read;
    let header_size = main_record_header_size();
    let mut file = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; header_size];
    file.read_exact(&mut head).ok()?;
    let expected: &[u8; 4] = if game_mode() == GameMode::gmTES3 {
        b"TES3"
    } else {
        b"TES4"
    };
    if &head[..4] != expected {
        return None;
    }
    // A file header is small (the masters and the screenshot); a file that
    // claims more is not one.
    let data_size = u32::from_le_bytes(head[4..8].try_into().ok()?) as usize;
    if data_size > 4 * 1024 * 1024 {
        return None;
    }
    let mut data = vec![0u8; data_size];
    file.read_exact(&mut data).ok()?;
    // The record flags are at offset 8 in a TES4 header and at 12 in a
    // Morrowind one (`TwbMainRecordStruct`). The ESM bit is 0x1 in both;
    // the blueprint bit of Starfield is 0x800.
    let flags_at = if game_mode() == GameMode::gmTES3 { 12 } else { 8 };
    let flags = u32::from_le_bytes(head[flags_at..flags_at + 4].try_into().ok()?);
    Some(ModuleHeader {
        is_esm: flags & 0x1 != 0,
        has_blueprint: game_mode() == GameMode::gmSF1 && flags & 0x800 != 0,
        masters: header_masters(&data, game_mode() == GameMode::gmTES3),
    })
}

/// The `MAST` names of a file header record's data. Morrowind's subrecords
/// carry a four byte size; the later games two, with `XXXX` giving the size
/// of the subrecord that follows it.
fn header_masters(data: &[u8], wide: bool) -> Vec<String> {
    let mut masters = Vec::new();
    let mut pos = 0usize;
    let mut next_size: Option<usize> = None;
    while pos + if wide { 8 } else { 6 } <= data.len() {
        let signature = &data[pos..pos + 4];
        let (size, start) = if wide {
            let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap_or([0; 4])) as usize;
            (size, pos + 8)
        } else {
            let size = u16::from_le_bytes(data[pos + 4..pos + 6].try_into().unwrap_or([0; 2])) as usize;
            (next_size.take().unwrap_or(size), pos + 6)
        };
        let Some(body) = data.get(start..start.saturating_add(size)) else {
            break;
        };
        if signature == b"XXXX" && size == 4 {
            next_size = Some(u32::from_le_bytes(body.try_into().unwrap_or([0; 4])) as usize);
        } else if signature == b"MAST" {
            let end = body.iter().position(|&byte| byte == 0).unwrap_or(body.len());
            let name = String::from_utf8_lossy(&body[..end]).into_owned();
            if !name.is_empty() {
                masters.push(name);
            }
        }
        pos = start.saturating_add(size);
    }
    masters
}

/// The extension decides the ESM-ness of the Fallout 4 family and later
/// (`wbLoadModules`: `if wbGameMode >= gmFO4`).
fn esm_by_extension() -> bool {
    game_mode() >= GameMode::gmFO4
}

/// The write time of a file as a Delphi `TDateTime` (`wbGetLastWriteTime`).
fn date_time(path: &Path) -> f64 {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |since| since.as_secs() as f64 / 86_400.0 + 25_569.0)
}

/// `CmpDouble`.
fn cmp_double(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

/// `_UpdateIndex`: `-1`, the index `Update.esm` of the Skyrim family takes
/// (the highest but one for Enderal Special Edition, where it loads last).
fn update_index() -> i32 {
    if game_mode() == GameMode::gmEnderalSE {
        i32::MAX - 1
    } else {
        -1
    }
}

/// `wbSimplePluginsTxt`: the games whose plugin list names only the active
/// plugins; the later ones mark an active plugin with `*`.
fn simple_plugins_txt() -> bool {
    use GameMode::*;
    matches!(
        game_mode(),
        gmTES3 | gmTES4 | gmTES4R | gmFO3 | gmFNV | gmTES5 | gmEnderal
    )
}

/// `wbOrderFromPluginsTxt`: the games that take the load order from the
/// order of the plugin list (`miPluginsTxtIndex`).
fn order_from_plugins_txt() -> bool {
    use GameMode::*;
    matches!(
        game_mode(),
        gmTES4R | gmTES5 | gmEnderal | gmSSE | gmEnderalSE | gmTES5VR | gmFO4 | gmFO4VR | gmFO76 | gmSF1
    )
}

/// The games that read a `loadorder.txt` next to the plugin list
/// (`wbLoadModules`: `wbGameMode in [gmTES4R, gmTES5, gmEnderal]`).
fn reads_load_order_txt() -> bool {
    use GameMode::*;
    matches!(game_mode(), gmTES4R | gmTES5 | gmEnderal)
}

/// Port of `_ModulesLoadOrderCompare`: the blueprint flag first, then the
/// official order, the Creation Club order, the ESM flag (the games before
/// Fallout 4 keep the plugin list order among ESM and ESP alike),
/// `Plugins.txt`, the write time and the name. UPSTREAM-QUIRK: the
/// comparison ends on the addresses of the two modules (upstream's
/// `CmpPtr`), which the port leaves out: two modules that differ in nothing
/// else have the same name and cannot both exist.
fn load_order_compare(a: &ModuleInfo, b: &ModuleInfo) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a.has_blueprint != b.has_blueprint {
        return if a.has_blueprint {
            Ordering::Greater
        } else {
            Ordering::Less
        };
    }
    a.official_index
        .cmp(&b.official_index)
        .then_with(|| a.cc_index.cmp(&b.cc_index))
        .then_with(|| {
            if a.is_esm == b.is_esm || game_mode() == GameMode::gmTES4R {
                a.plugins_txt_index
                    .cmp(&b.plugins_txt_index)
                    .then_with(|| cmp_double(a.date_time, b.date_time))
                    .then_with(|| compare_text(&a.name, &b.name))
            } else if a.is_esm {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        })
}

/// Port of `_ModulesLoadOrderCompareCombined`: [`load_order_compare`] with
/// the `loadorder.txt` index (`miCombinedIndex`) before the plugin list
/// index.
fn load_order_compare_combined(a: &ModuleInfo, b: &ModuleInfo) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a.has_blueprint != b.has_blueprint {
        return if a.has_blueprint {
            Ordering::Greater
        } else {
            Ordering::Less
        };
    }
    a.official_index
        .cmp(&b.official_index)
        .then_with(|| a.cc_index.cmp(&b.cc_index))
        .then_with(|| {
            if a.is_esm == b.is_esm || game_mode() == GameMode::gmTES4R {
                a.combined_index
                    .cmp(&b.combined_index)
                    .then_with(|| a.plugins_txt_index.cmp(&b.plugins_txt_index))
                    .then_with(|| cmp_double(a.date_time, b.date_time))
                    .then_with(|| compare_text(&a.name, &b.name))
            } else if a.is_esm {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        })
}

/// `wbModuleByName`: the module of the name, without regard to case. The
/// module must be valid (a header that read), and a name that ends with
/// `.ghost` is never found (upstream's `TwbModuleInfo` quirk).
fn find_module(modules: &[ModuleInfo], name: &str) -> Option<usize> {
    if name.is_empty() || name.to_ascii_lowercase().ends_with(".ghost") {
        return None;
    }
    modules
        .iter()
        .position(|module| module.valid && same_text(&module.name, name))
}

/// The text of a `TStringList.LoadFromFile`: a UTF-8 BOM is skipped, the
/// other bytes are the ANSI text, and CR, LF or CRLF end a line (`TStrings.
/// SetTextStr`); a last line without one counts.
fn read_text_lines(path: &str) -> Option<Vec<String>> {
    let bytes = std::fs::read(path).ok()?;
    let text = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(rest) => String::from_utf8_lossy(rest).into_owned(),
        None => bytes.iter().map(|&byte| char::from(byte)).collect(),
    };
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                lines.push(std::mem::take(&mut line));
            }
            _ => line.push(c),
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    Some(lines)
}

/// A line of the plugin list or `loadorder.txt`: the text before the first
/// `#`, trimmed (`wbLoadModules`' `Pos('#', s)` and `Trim`).
fn list_name(line: &str) -> &str {
    let line = match line.find('#') {
        Some(at) => &line[..at],
        None => line,
    };
    line.trim()
}

/// `wbLoadModules`' reading of the plugin list: the `miPluginsTxtIndex` of
/// every valid module the list names, and `mfActive` for the active ones.
fn apply_plugins_txt(modules: &mut [ModuleInfo]) {
    let path = plugins_file_name();
    if path.is_empty() {
        return;
    }
    let Some(lines) = read_text_lines(&path) else {
        return;
    };
    let simple = simple_plugins_txt();
    let ordered = order_from_plugins_txt();
    for (index, line) in lines.iter().enumerate() {
        let name = list_name(line);
        let (active, name) = if simple {
            (true, name)
        } else if let Some(name) = name.strip_prefix('*') {
            (true, name.trim())
        } else {
            (false, name)
        };
        let Some(module) = find_module(modules, name) else {
            continue;
        };
        if ordered {
            modules[module].plugins_txt_index = i32::try_from(index).unwrap_or(i32::MAX);
            modules[module].has_index = true;
        }
        if active {
            modules[module].active = true;
            modules[module].active_in_plugins_txt = true;
        }
    }
}

/// `wbLoadModules`' resolution of the masters: a module whose `MAST` names
/// a file that is not a module of the data folder, or that is itself
/// missing masters, gets `mfMastersMissing`. The check repeats until no
/// module changes, so the state is transitive.
fn resolve_masters(modules: &mut [ModuleInfo]) {
    loop {
        let mut changed = false;
        for index in 0..modules.len() {
            if modules[index].masters_missing || modules[index].master_names.is_empty() {
                continue;
            }
            let missing = modules[index]
                .master_names
                .iter()
                .any(|master| match find_module(modules, master) {
                    Some(master) => modules[master].masters_missing,
                    None => true,
                });
            if missing {
                modules[index].masters_missing = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

/// `if mfMastersMissing in miFlags then Exclude(miFlags, mfActive)`.
fn deactivate_missing_masters(modules: &mut [ModuleInfo]) {
    for module in modules {
        if module.masters_missing {
            module.active = false;
        }
    }
}

/// The official indices of `wbLoadModules`: the game master, the game's
/// executable, `Update.esm` of the Skyrim family and the game's official
/// DLC (`wbOfficialDLC`); every other module keeps `High(Integer)`. The
/// valid ones among them are active (`mfActive`), whether the plugin list
/// names them or not.
fn set_official_indices(modules: &mut [ModuleInfo]) {
    let set = |modules: &mut [ModuleInfo], name: &str, index: i32, valid_only: bool, active: bool| {
        if let Some(module) = modules
            .iter_mut()
            .find(|module| same_text(&module.name, name) && (!valid_only || module.valid))
        {
            module.official_index = index;
            if active && module.valid {
                module.active = true;
                module.has_index = true;
            }
        }
    };
    set(modules, &game_master_esm(), i32::MIN, true, true);
    // The executable takes its index whether its header read or not.
    set(modules, &game_exe_name(), i32::MIN + 1, false, false);
    if is_skyrim() {
        set(modules, "Update.esm", update_index(), true, true);
    }
    for (i, name) in official_dlc().iter().enumerate() {
        set(modules, name, i as i32, true, true);
    }
}

/// `wbCreationClubContent`: the lines of the game's Creation Club list,
/// `ExtractFilePath(ExcludeTrailingPathDelimiter(wbDataPath)) +
/// wbCreationClubContentFileName` (`xeInit.pas`, `TStringList.LoadFromFile`),
/// or none when the game has no list or the file is not there.
pub fn creation_club_content(data_path: &str) -> Vec<String> {
    let name = creation_club_content_file_name();
    if name.is_empty() {
        return Vec::new();
    }
    let trimmed = data_path.trim_end_matches(['\\', '/']);
    let folder = match trimmed.rfind(['\\', '/']) {
        Some(at) => &trimmed[..=at],
        None => "",
    };
    read_text_lines(&format!("{folder}{name}")).unwrap_or_default()
}

/// `for i := Low(wbCreationClubContent) to High(wbCreationClubContent) do
/// with wbModuleByName(...) do if IsValid then miCCIndex := Succ(i)`: the
/// Creation Club modules are active and indexed too.
fn set_cc_indices(modules: &mut [ModuleInfo], content: &[String]) {
    for (i, name) in content.iter().enumerate() {
        if let Some(module) = modules
            .iter_mut()
            .find(|module| module.valid && same_text(&module.name, name))
        {
            module.cc_index = i32::try_from(i + 1).unwrap_or(i32::MAX);
            module.active = true;
            module.has_index = true;
        }
    }
}

/// The time now as a Delphi `TDateTime`, which `AddNewModule` gives a
/// loaded file that is not in the data folder.
fn now_date_time() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs() as f64 / 86_400.0 + 25_569.0)
}

/// The modules of the data folder `data_path` (with a trailing separator)
/// as `wbLoadModules` scans it: the game's executable and every plugin file
/// whose header reads.
fn scan_modules(data_path: &str) -> Vec<ModuleInfo> {
    let mut names: Vec<String> = std::fs::read_dir(data_path)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    // `CompareText` order, which keeps a ghost after its original.
    names.sort_by_key(|name| name.to_ascii_uppercase());
    let mut modules: Vec<ModuleInfo> = Vec::new();
    // `_Modules[0]`: the game's executable, which the hardcoded file loads
    // as. It carries the ESM flag of the hardcoded file (`mfHasESMFlag`,
    // `mfIsESM`).
    modules.push(ModuleInfo {
        name: game_exe_name(),
        original_name: game_exe_name(),
        file: None,
        load_order: i32::MAX,
        valid: false,
        is_esm: true,
        has_blueprint: false,
        official_index: i32::MAX,
        cc_index: i32::MAX,
        plugins_txt_index: i32::MAX,
        load_order_txt_index: i32::MAX,
        combined_index: i32::MAX,
        has_index: false,
        masters_missing: false,
        active: false,
        active_in_plugins_txt: false,
        master_names: Vec::new(),
        date_time: 0.0,
        data_path: data_path.to_owned(),
        crc32: OnceLock::new(),
    });
    for original_name in names {
        let lower = original_name.to_ascii_lowercase();
        let name = if lower.ends_with(".ghost") {
            let name = original_name[..original_name.len() - ".ghost".len()].to_owned();
            // A ghost is ignored when the original exists.
            if modules.last().is_some_and(|last| same_text(&last.name, &name)) {
                continue;
            }
            name
        } else {
            original_name.clone()
        };
        if !is_module_name(&name) {
            continue;
        }
        let path = Path::new(data_path).join(&original_name);
        let Some(header) = read_module_header(&path) else {
            continue;
        };
        let extension = name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase());
        modules.push(ModuleInfo {
            name,
            original_name,
            file: None,
            load_order: i32::MAX,
            valid: true,
            // The extension of the later games, the header flag of the
            // others (`mfIsESM`).
            is_esm: (esm_by_extension() && matches!(extension.as_deref(), Some("esm") | Some("esl"))) || header.is_esm,
            has_blueprint: header.has_blueprint,
            official_index: i32::MAX,
            cc_index: i32::MAX,
            plugins_txt_index: i32::MAX,
            load_order_txt_index: i32::MAX,
            combined_index: i32::MAX,
            has_index: false,
            masters_missing: false,
            active: false,
            active_in_plugins_txt: false,
            master_names: header.masters,
            date_time: date_time(&path),
            data_path: data_path.to_owned(),
            crc32: OnceLock::new(),
        });
    }
    modules
}

/// `TwbFile.Create`'s attachment of the loaded `files` to their modules
/// (`miFile`, `miLoadOrder`), with `AddNewModule` for a file that is not in
/// the data folder.
fn attach_files(modules: &mut Vec<ModuleInfo>, data_path: &str, files: &[Arc<FileImpl>]) {
    let mut loaded: Vec<&Arc<FileImpl>> = files.iter().collect();
    loaded.sort_by_key(|file| file.load_order());
    for file in loaded {
        let name = file.get_name();
        let index = match modules.iter().position(|module| same_text(&module.name, &name)) {
            Some(index) => index,
            None => {
                // `TwbModuleInfo.AddNewModule`: the extension decides its
                // ESM-ness, the time is now.
                let lower = name.to_ascii_lowercase();
                modules.push(ModuleInfo {
                    name: name.clone(),
                    original_name: name.clone(),
                    file: None,
                    load_order: i32::MAX,
                    valid: true,
                    is_esm: matches!(lower.rsplit_once('.').map(|(_, ext)| ext), Some("esm") | Some("esl")),
                    has_blueprint: false,
                    official_index: i32::MAX,
                    cc_index: i32::MAX,
                    plugins_txt_index: i32::MAX,
                    load_order_txt_index: i32::MAX,
                    combined_index: i32::MAX,
                    has_index: false,
                    masters_missing: false,
                    active: false,
                    active_in_plugins_txt: false,
                    master_names: Vec::new(),
                    date_time: now_date_time(),
                    data_path: data_path.to_owned(),
                    crc32: OnceLock::new(),
                });
                modules.len() - 1
            }
        };
        let module = &mut modules[index];
        if module.file.is_none() {
            module.file = Some(file.clone());
            module.load_order = file.load_order();
        }
    }
}

/// The `loadorder.txt` entries `wbLoadModules` keeps: the line index in
/// the file and the module of each valid name, in file order. `None` for a
/// game that does not read the file (or a file with fewer than two valid
/// names).
fn load_order_txt_lines(modules: &[ModuleInfo]) -> Option<Vec<(usize, usize)>> {
    if !reads_load_order_txt() {
        return None;
    }
    let plugins = plugins_file_name();
    let folder = match plugins.rfind(['\\', '/']) {
        Some(at) => &plugins[..=at],
        None => "",
    };
    let lines = read_text_lines(&format!("{folder}loadorder.txt"))?;
    let mut resolved = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let name = list_name(line);
        if let Some(module) = find_module(modules, name) {
            resolved.push((index, module));
        }
    }
    (resolved.len() > 1).then_some(resolved)
}

/// `wbLoadModules`' `miCombinedIndex` pass: every module starts at
/// `Succ(position) * 1000` of the plugin-list order, and a module of the
/// `loadorder.txt` without an index of its own inherits the index of the
/// previous entry that has one, plus one (`mfHasIndex`).
fn apply_combined_indices(modules: &mut [ModuleInfo], lines: &[(usize, usize)]) {
    for (position, module) in modules.iter_mut().enumerate() {
        module.combined_index = i32::try_from(position + 1).unwrap_or(i32::MAX / 1000) * 1000;
    }
    // Upstream starts at 1: the first entry of the file gives no index.
    for i in 1..lines.len() {
        let (original, index) = lines[i];
        modules[index].load_order_txt_index = i32::try_from(original).unwrap_or(i32::MAX);
        if modules[index].has_index {
            continue;
        }
        let mut previous = None;
        for &(_, earlier) in lines[..i].iter().rev() {
            if modules[earlier].has_index {
                previous = Some(modules[earlier].combined_index);
                break;
            }
        }
        if let Some(previous) = previous {
            modules[index].combined_index = previous.saturating_add(1);
            modules[index].has_index = true;
        }
    }
}

/// The last part of `wbLoadModules`: the masters, the plugin list, the
/// implicit actives, the official and Creation Club indices, and the two
/// sorts.
fn finish(modules: &mut [ModuleInfo], data_path: &str) {
    resolve_masters(modules);
    apply_plugins_txt(modules);
    deactivate_missing_masters(modules);
    set_official_indices(modules);
    set_cc_indices(modules, &creation_club_content(data_path));
    // `wbMergeSortPtr(_ModulesLoadOrder, _ModulesLoadOrderCompare)`, a
    // stable sort that keeps the file order of two modules that compare
    // equal (and so the order of the mod group files, `wbLoadModGroups`).
    modules.sort_by(load_order_compare);
    if let Some(lines) = load_order_txt_lines(modules) {
        apply_combined_indices(modules, &lines);
        modules.sort_by(load_order_compare_combined);
    }
    for (position, module) in modules.iter_mut().enumerate() {
        module.combined_index = i32::try_from(position).unwrap_or(i32::MAX);
    }
}

/// Port of `SimulateLoad`'s `Load`: the module after its masters, which
/// load first when they are not loaded yet.
fn load_module(
    modules: &[ModuleInfo],
    index: usize,
    loaded: &mut [bool],
    loading: &mut [bool],
    order: &mut Vec<usize>,
) -> Result<(), String> {
    if loaded[index] {
        return Ok(());
    }
    if loading[index] {
        return Err(format!(
            "Modules contain circular references. Can't load \"{}\"",
            modules[index].name
        ));
    }
    loading[index] = true;
    for master in &modules[index].master_names {
        match find_module(modules, master) {
            Some(master) => load_module(modules, master, loaded, loading, order)?,
            None => {
                return Err(format!(
                    "Module \"{}\" requires master \"{master}\" which can not be found",
                    modules[index].name
                ));
            }
        }
    }
    loading[index] = false;
    loaded[index] = true;
    order.push(index);
    Ok(())
}

impl ModuleInfo {
    /// The module's file in the data folder, with the folder's trailing
    /// separator (`wbDataPath + miOriginalName`).
    pub fn file_path(&self) -> String {
        format!("{}{}", self.data_path, self.original_name)
    }

    /// `TwbModuleInfo.HasIndex`: a valid module with one of the indices the
    /// sort reads.
    pub fn has_index(&self) -> bool {
        self.valid && self.has_index
    }
}

impl Modules {
    /// `wbLoadModules` without loaded files: the modules of the data
    /// folder `data_path` (with a trailing separator), their masters, the
    /// plugin list and the load order. A legacy command line loads
    /// [`Modules::active_load_order`] of it.
    pub fn load_data_folder(data_path: &str) -> Modules {
        let mut modules = scan_modules(data_path);
        finish(&mut modules, data_path);
        Modules { modules }
    }

    /// The modules of the data folder `data_path` (with a trailing
    /// separator) and of the loaded `files` (`wbLoadModules`, with the
    /// loaded files attached as `TwbFile.Create` does).
    pub fn load(data_path: &str, files: &[Arc<FileImpl>]) -> Modules {
        let mut modules = scan_modules(data_path);
        attach_files(&mut modules, data_path, files);
        finish(&mut modules, data_path);
        Modules { modules }
    }

    /// Port of `SimulateLoad`: the active modules and the masters they
    /// need, each after its masters, in load order. The indices point into
    /// [`Modules::modules`].
    pub fn active_load_order(&self) -> Result<Vec<usize>, String> {
        let active: Vec<bool> = self.modules.iter().map(|module| module.active).collect();
        self.load_checked(&active, false)
    }

    /// Port of the module selection's `SimulateLoad` over the modules its
    /// tree has checked (`checked`, by index into [`Modules::modules`]):
    /// the checked modules and the masters they need, each after its
    /// masters, in load order. The game master also loads when
    /// `wbAlwaysLoadGameMaster` is set, as `TfrmModuleSelect.SimulateLoad`
    /// gives it `mfForceLoad`.
    pub fn simulate_load(&self, checked: &[bool]) -> Result<Vec<usize>, String> {
        self.load_checked(checked, always_load_game_master())
    }

    fn load_checked(&self, checked: &[bool], force_game_master: bool) -> Result<Vec<usize>, String> {
        let mut loaded = vec![false; self.modules.len()];
        let mut loading = vec![false; self.modules.len()];
        let mut order = Vec::new();
        for index in 0..self.modules.len() {
            let load = checked.get(index).copied().unwrap_or(false)
                || (force_game_master && self.modules[index].official_index == i32::MIN);
            if load {
                load_module(&self.modules, index, &mut loaded, &mut loading, &mut order)?;
            }
        }
        Ok(order)
    }

    /// Port of `wbModuleByName`: the module of the name, without regard to
    /// case; `None` is upstream's `_InvalidModule` (no file, never loaded).
    /// UPSTREAM-QUIRK: a name that ends with `.ghost` is never found
    /// (upstream lengthens the name where it means to cut the extension).
    pub fn by_name(&self, name: &str) -> Option<usize> {
        if name.is_empty() || name.to_ascii_lowercase().ends_with(".ghost") {
            return None;
        }
        self.modules.iter().position(|module| same_text(&module.name, name))
    }

    /// The module of a loaded file (`TwbFile.ModuleInfo`).
    pub fn of_file(&self, file: &FileImpl) -> Option<usize> {
        self.modules
            .iter()
            .position(|module| module.file.as_ref().is_some_and(|loaded| std::ptr::eq(&**loaded, file)))
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use xedit_core::interface::globals::{
        set_creation_club_content_file_name, set_game_exe_name, set_game_master_esm, set_game_mode,
        set_official_dlc_at, set_official_dlc_length, set_plugins_file_name, test_lock,
    };

    use super::*;

    fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut bytes = signature.to_vec();
        bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    /// A plugin whose file header lists `masters`. The header record flags
    /// carry the ESM bit (0x1) and the light bit of the later games (0x200).
    fn plugin(flags: u32, masters: &[&str]) -> Vec<u8> {
        let mut hedr = Vec::new();
        hedr.extend_from_slice(&1.7f32.to_le_bytes());
        hedr.extend_from_slice(&0u32.to_le_bytes());
        hedr.extend_from_slice(&0x0000_0800u32.to_le_bytes());
        let mut data = sub_record(b"HEDR", &hedr);
        for master in masters {
            let mut name = master.as_bytes().to_vec();
            name.push(0);
            data.extend(sub_record(b"MAST", &name));
            data.extend(sub_record(b"DATA", &[0u8; 8]));
        }
        let mut bytes = b"TES4".to_vec();
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&44u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend(data);
        bytes
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("xedit-loadorder-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write(data: &Path, name: &str, bytes: &[u8]) {
        std::fs::write(data.join(name), bytes).unwrap();
    }

    fn data_path(dir: &Path) -> String {
        format!("{}\\", dir.display())
    }

    fn names<'a>(modules: &'a Modules, order: &[usize]) -> Vec<&'a str> {
        order
            .iter()
            .map(|&index| modules.modules[index].name.as_str())
            .collect()
    }

    #[test]
    fn implicit_actives_load_without_the_plugin_list_naming_them() {
        let _lock = test_lock();
        let dir = temp_dir("implicit-actives");
        let data = dir.join("Data");
        std::fs::create_dir_all(&data).unwrap();
        set_game_mode(GameMode::gmSSE);
        set_game_exe_name("SkyrimSE.exe");
        set_game_master_esm("Skyrim.esm");
        set_official_dlc_length(3);
        set_official_dlc_at(0, "Dawnguard.esm");
        set_official_dlc_at(1, "HearthFires.esm");
        set_official_dlc_at(2, "Dragonborn.esm");
        set_creation_club_content_file_name("Skyrim.ccc");
        write(&data, "Skyrim.esm", &plugin(0x1, &[]));
        for name in [
            "Update.esm",
            "Dawnguard.esm",
            "HearthFires.esm",
            "Dragonborn.esm",
            "Mod.esp",
        ] {
            let flags = if name.ends_with(".esm") { 0x1 } else { 0 };
            write(&data, name, &plugin(flags, &["Skyrim.esm"]));
        }
        for name in ["ccA.esl", "ccB.esl"] {
            write(&data, name, &plugin(0x201, &["Skyrim.esm"]));
        }
        std::fs::write(dir.join("Skyrim.ccc"), "ccA.esl\r\nccB.esl\r\n").unwrap();
        let list = dir.join("plugins.txt");
        std::fs::write(&list, "# the list names no official module\r\n*Mod.esp\r\n").unwrap();
        set_plugins_file_name(list.to_str().unwrap());

        let modules = Modules::load_data_folder(&data_path(&data));
        let order = modules.active_load_order().unwrap();
        assert_eq!(
            names(&modules, &order),
            [
                "Skyrim.esm",
                "Update.esm",
                "Dawnguard.esm",
                "HearthFires.esm",
                "Dragonborn.esm",
                "ccA.esl",
                "ccB.esl",
                "Mod.esp"
            ]
        );
    }

    #[test]
    fn a_module_with_missing_masters_is_not_active() {
        let _lock = test_lock();
        let dir = temp_dir("missing-masters");
        let data = dir.join("Data");
        std::fs::create_dir_all(&data).unwrap();
        set_game_mode(GameMode::gmFO4);
        set_game_exe_name("Fallout4.exe");
        set_game_master_esm("Fallout4.esm");
        set_official_dlc_length(0);
        set_creation_club_content_file_name("Fallout4.ccc");
        write(&data, "Fallout4.esm", &plugin(0x1, &[]));
        write(&data, "A.esp", &plugin(0, &["Fallout4.esm"]));
        write(&data, "B.esp", &plugin(0, &["A.esp"]));
        write(&data, "C.esp", &plugin(0, &["Missing.esm"]));
        let list = dir.join("plugins.txt");
        std::fs::write(&list, "*A.esp\r\n*B.esp\r\n*C.esp\r\n").unwrap();
        set_plugins_file_name(list.to_str().unwrap());

        let modules = Modules::load_data_folder(&data_path(&data));
        let c = modules.by_name("C.esp").unwrap();
        assert!(modules.modules[c].masters_missing);
        assert!(!modules.modules[c].active);
        let order = modules.active_load_order().unwrap();
        assert_eq!(names(&modules, &order), ["Fallout4.esm", "A.esp", "B.esp"]);
    }

    #[test]
    fn masters_are_read_and_missing_ones_propagate() {
        let _lock = test_lock();
        let dir = temp_dir("masters-transitive");
        let data = dir.join("Data");
        std::fs::create_dir_all(&data).unwrap();
        set_game_mode(GameMode::gmFO4);
        set_game_exe_name("Fallout4.exe");
        set_game_master_esm("Fallout4.esm");
        set_official_dlc_length(0);
        set_creation_club_content_file_name("");
        write(&data, "Fallout4.esm", &plugin(0x1, &[]));
        write(&data, "A.esp", &plugin(0, &["B.esm"]));
        write(&data, "B.esm", &plugin(0x1, &["Missing.esm"]));
        write(&data, "C.esp", &plugin(0, &["A.esp"]));
        let list = dir.join("plugins.txt");
        std::fs::write(&list, "*A.esp\r\n*B.esm\r\n*C.esp\r\n").unwrap();
        set_plugins_file_name(list.to_str().unwrap());

        let modules = Modules::load_data_folder(&data_path(&data));
        let b = modules.by_name("B.esm").unwrap();
        assert_eq!(modules.modules[b].master_names, ["Missing.esm"]);
        for name in ["A.esp", "B.esm", "C.esp"] {
            assert!(
                modules.modules[modules.by_name(name).unwrap()].masters_missing,
                "{name}"
            );
        }
        // The game master is an implicit active and ignores the missing
        // masters (upstream activates it after the exclusion).
        assert_eq!(
            modules
                .active_load_order()
                .unwrap()
                .iter()
                .map(|&index| modules.modules[index].name.as_str())
                .collect::<Vec<_>>(),
            ["Fallout4.esm"]
        );
    }

    #[test]
    fn loadorder_txt_reorders_a_module_the_plugin_list_does_not_name() {
        let _lock = test_lock();
        let dir = temp_dir("loadorder");
        let data = dir.join("Data");
        std::fs::create_dir_all(&data).unwrap();
        set_game_mode(GameMode::gmTES5);
        set_game_exe_name("TESV.exe");
        set_game_master_esm("Skyrim.esm");
        set_official_dlc_length(0);
        set_creation_club_content_file_name("");
        write(&data, "Skyrim.esm", &plugin(0x1, &[]));
        for name in ["ModA.esp", "ModB.esp", "Extra.esp"] {
            write(&data, name, &plugin(0, &["Skyrim.esm"]));
        }
        let list = dir.join("plugins.txt");
        std::fs::write(&list, "Skyrim.esm\r\nModA.esp\r\nModB.esp\r\n").unwrap();
        std::fs::write(
            dir.join("loadorder.txt"),
            "Skyrim.esm\r\nModA.esp\r\nExtra.esp\r\nModB.esp\r\n",
        )
        .unwrap();
        set_plugins_file_name(list.to_str().unwrap());

        let modules = Modules::load_data_folder(&data_path(&data));
        // The loaded modules are the plugin list's; the combined order
        // moves the module the list does not name between the two it does.
        let order = modules.active_load_order().unwrap();
        assert_eq!(names(&modules, &order), ["Skyrim.esm", "ModA.esp", "ModB.esp"]);
        let position = |name: &str| modules.by_name(name).unwrap();
        let (a, extra, b) = (position("ModA.esp"), position("Extra.esp"), position("ModB.esp"));
        assert!(a < extra && extra < b, "Extra.esp is not between the listed ones");
        assert_eq!(modules.modules[extra].load_order_txt_index, 2);

        // A game that does not read the file keeps the plugin-list order.
        set_game_mode(GameMode::gmSSE);
        let modules = Modules::load_data_folder(&data_path(&data));
        let position = |name: &str| modules.by_name(name).unwrap();
        let (a, extra, b) = (position("ModA.esp"), position("Extra.esp"), position("ModB.esp"));
        assert!(extra < a || extra > b, "Extra.esp was moved without a loadorder.txt");
        assert_eq!(modules.modules[extra].load_order_txt_index, i32::MAX);
    }
}
