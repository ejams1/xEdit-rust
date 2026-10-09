// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbLoadOrder.pas (TwbModuleInfo, wbLoadModules,
// wbModuleByName, wbModulesByLoadOrder, GetCRC32, HasCRC32)

//! The module information of `wbLoadOrder.pas` as far as the mod groups
//! read it: a module for every plugin of the data folder (and the game's
//! executable, the hardcoded file), the loaded file of a module with its
//! load order, and the CRC32 of a module's file.
//!
//! Differences to upstream: the port does not build its load order from
//! the modules (a session loads the plugins it is given), so the modules of
//! the loaded files come first in their load order and the other modules of
//! the data folder follow by name, where upstream sorts all modules by the
//! official order, `Plugins.txt` and the file times. The order of the
//! modules only decides the order of the mod group files of modules that
//! are not loaded. A plugin of the data folder counts as a module when its
//! first record is a file header (upstream reads its masters and flags,
//! `wbMastersForFile`); the flags are not read.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use xedit_core::implementation::FileImpl;
use xedit_core::interface::Element;
use xedit_core::interface::globals::{GameMode, game_exe_name, game_mode, is_light_supported};

use crate::ini_files::same_text;

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
        || (lower.ends_with(".esl") && is_light_supported())
}

/// Whether the file starts with a file header record, as `wbMastersForFile`
/// needs it to.
fn has_file_header(path: &Path) -> bool {
    use std::io::Read;
    let mut signature = [0u8; 4];
    let read = std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut signature));
    let expected: &[u8; 4] = if game_mode() == GameMode::gmTES3 {
        b"TES3"
    } else {
        b"TES4"
    };
    read.is_ok() && &signature == expected
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
        // loads as.
        modules.push(ModuleInfo {
            name: game_exe_name(),
            original_name: game_exe_name(),
            file: None,
            load_order: i32::MAX,
            valid: false,
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
            if !is_module_name(&name) || !has_file_header(&Path::new(data_path).join(&original_name)) {
                continue;
            }
            modules.push(ModuleInfo {
                name,
                original_name,
                file: None,
                load_order: i32::MAX,
                valid: true,
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
        // Loaded modules in load order, the others by name.
        modules.sort_by(|a, b| {
            (a.load_order, a.name.to_ascii_uppercase()).cmp(&(b.load_order, b.name.to_ascii_uppercase()))
        });
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
