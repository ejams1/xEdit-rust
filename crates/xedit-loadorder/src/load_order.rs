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
//! Differences to upstream: the port does not build a load order from the
//! modules (a session loads the plugins it is given, and the CLI reads no
//! plugin list), so a loaded module takes its session load order as
//! `miPluginsTxtIndex` (upstream reads it from `Plugins.txt`, which #37
//! owns) and every other module keeps the highest index; the Creation Club
//! list (the game's `.ccc` file) is not read either, so `miCCIndex` is the
//! highest for every module. A plugin of the data folder counts as a
//! module when its first record is a file header (upstream reads its
//! masters and flags, `wbMastersForFile`); the header flags that the order
//! reads (the ESM and blueprint bits) are read here.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use xedit_core::implementation::FileImpl;
use xedit_core::interface::Element;
use xedit_core::interface::globals::{
    GameMode, game_exe_name, game_master_esm, game_mode, official_dlc,
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
    /// (`is_esm_extension`).
    pub is_esm: bool,
    /// `mfHasBlueprintFlag`: the header flag 11, a Starfield blueprint.
    pub has_blueprint: bool,
    /// `miOfficialIndex`: the place of the game master, the game's
    /// executable and its official DLC (and `Update.esm` of the Skyrim
    /// family), `High(Integer)` for every other module.
    pub official_index: i32,
    /// `miCCIndex`: the place of the Creation Club content (not read, so
    /// `High(Integer)` for every module).
    pub cc_index: i32,
    /// `miPluginsTxtIndex`: for a loaded module its load order, else
    /// `High(Integer)`.
    pub plugins_txt_index: i32,
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
    matches!(
        game_mode(),
        gmTES5 | gmEnderal | gmTES5VR | gmSSE | gmEnderalSE
    )
}

/// What the load order reads of a file header: the ESM flag of the games
/// that take it from the header, and the Starfield blueprint flag.
struct ModuleHeader {
    is_esm: bool,
    has_blueprint: bool,
}

/// The header of a file whose flags are read, as `wbMastersForFile` reads
/// them (the port reads no masters); `None` for a file that does not start
/// with a file header record.
fn read_module_header(path: &Path) -> Option<ModuleHeader> {
    use std::io::Read;
    let mut bytes = [0u8; 24];
    let mut file = std::fs::File::open(path).ok()?;
    file.read_exact(&mut bytes).ok()?;
    let expected: &[u8; 4] = if game_mode() == GameMode::gmTES3 {
        b"TES3"
    } else {
        b"TES4"
    };
    if &bytes[..4] != expected {
        return None;
    }
    // The flags of the header record (`TwbFile.GetIsESM`, `IsBlueprint`,
    // the bit 11 of the Fallout 4 header flags upstream reads for
    // Starfield).
    let flags = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    Some(ModuleHeader {
        is_esm: flags & 0x1 != 0,
        has_blueprint: game_mode() == GameMode::gmSF1 && flags & 0x800 != 0,
    })
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
        return if a.has_blueprint { Ordering::Greater } else { Ordering::Less };
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

/// The official indices of `wbLoadModules`: the game master, the game's
/// executable, `Update.esm` of the Skyrim family and the game's official
/// DLC (`wbOfficialDLC`); every other module keeps `High(Integer)`.
fn set_official_indices(modules: &mut [ModuleInfo]) {
    let set = |modules: &mut [ModuleInfo], name: &str, index: i32, valid_only: bool| {
        if let Some(module) = modules
            .iter_mut()
            .find(|module| same_text(&module.name, name) && (!valid_only || module.valid))
        {
            module.official_index = index;
        }
    };
    set(modules, &game_master_esm(), i32::MIN, true);
    // The executable takes its index whether its header read or not.
    set(modules, &game_exe_name(), i32::MIN + 1, false);
    if is_skyrim() {
        set(modules, "Update.esm", update_index(), true);
    }
    for (i, name) in official_dlc().iter().enumerate() {
        set(modules, name, i as i32, true);
    }
}

impl Modules {
    /// The modules of the data folder `data_path` (with a trailing
    /// separator) and of the loaded `files` (`wbLoadModules`, with the
    /// loaded files attached as `TwbFile.Create` does).
    pub fn load(data_path: &str, files: &[Arc<FileImpl>]) -> Modules {
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
        // `_Modules[0]`: the game's executable, which the hardcoded file
        // loads as. It carries the ESM flag of the hardcoded file
        // (`mfHasESMFlag`, `mfIsESM`).
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
            let extension = name.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase());
            modules.push(ModuleInfo {
                name,
                original_name,
                file: None,
                load_order: i32::MAX,
                valid: true,
                // The extension of the later games, the header flag of the
                // others (`mfIsESM`).
                is_esm: (esm_by_extension()
                    && matches!(extension.as_deref(), Some("esm") | Some("esl")))
                    || header.is_esm,
                has_blueprint: header.has_blueprint,
                official_index: i32::MAX,
                cc_index: i32::MAX,
                plugins_txt_index: i32::MAX,
                date_time: date_time(&path),
                data_path: data_path.to_owned(),
                crc32: OnceLock::new(),
            });
        }
        let mut loaded: Vec<&Arc<FileImpl>> = files.iter().collect();
        loaded.sort_by_key(|file| file.load_order());
        for file in loaded {
            let name = file.get_name();
            let index = match modules.iter().position(|module| same_text(&module.name, &name)) {
                Some(index) => index,
                None => {
                    // `TwbModuleInfo.AddNewModule`.
                    modules.push(ModuleInfo {
                        name: name.clone(),
                        original_name: name.clone(),
                        file: None,
                        load_order: i32::MAX,
                        valid: true,
                        is_esm: false,
                        has_blueprint: false,
                        official_index: i32::MAX,
                        cc_index: i32::MAX,
                        plugins_txt_index: i32::MAX,
                        date_time: 0.0,
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
        set_official_indices(&mut modules);
        // The plugin list index of a loaded module is its load order: the
        // caller loads the plugins in the order the plugin list has them.
        for module in &mut modules {
            if module.file.is_some() {
                module.plugins_txt_index = module.load_order;
            }
        }
        // `wbMergeSortPtr(_ModulesLoadOrder, _ModulesLoadOrderCompare)`, a
        // stable sort that keeps the file order of two modules that compare
        // equal (and so the order of the mod group files, `wbLoadModGroups`).
        modules.sort_by(load_order_compare);
        Modules { modules }
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
