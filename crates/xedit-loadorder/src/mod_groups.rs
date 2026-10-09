// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbModGroups.pas

//! Mod groups: named lists of modules, kept in `.modgroups` files, that
//! tell xEdit which records of a load order a user has checked against each
//! other. While mod groups are active, the comparison of the records of a
//! FormID leaves out the records of the modules an active mod group says
//! are hidden by a module that loads later (`ctHiddenByModGroup`).
//!
//! A mod group file is an ini file: each section is a mod group, each line
//! of it an item, `[flags]filename[:crc32,crc32,...]`. The flags are `+`
//! (optional), `-` (neither a source nor a target), `!` (forbidden), `@`
//! (a target only: hidden, hides nothing), `#` (a source only: hides,
//! is not hidden), `}` (the load order is ignored) and `{` (the load order
//! is ignored within a block of such items). The files are
//! `<module>.modgroups` next to every module of the data folder and
//! `<AppName><ToolName>.modgroups` next to the program
//! (`wbModGroupFileName`).
//!
//! Upstream keeps the mod group files in a unit-level array that is loaded
//! once and reloaded on request, and the state of the activation on the
//! module records (`miModGroupTargets`, `mfIsModGroupTarget`); here
//! [`ModGroups`] holds the files of one load and [`Activation`] the result
//! of `Activate`.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::ini_files::{MemIniFile, same_text, save_strings, trim};
use crate::load_order::Modules;

static MOD_GROUP_FILE_NAME: RwLock<String> = RwLock::new(String::new());

/// `wbModGroupFileName`: the program's own mod group file.
pub fn mod_group_file_name() -> String {
    MOD_GROUP_FILE_NAME.read().unwrap().clone()
}

/// Sets `wbModGroupFileName`.
pub fn set_mod_group_file_name(name: &str) {
    *MOD_GROUP_FILE_NAME.write().unwrap() = name.to_owned();
}

/// Port of `TwbMessageType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Debug,
    Info,
    Hint,
    Warning,
    Error,
}

impl MessageType {
    /// `wbMessageTypeString`.
    pub fn as_str(self) -> &'static str {
        match self {
            MessageType::Debug => "Debug",
            MessageType::Info => "Info",
            MessageType::Hint => "Hint",
            MessageType::Warning => "Warning",
            MessageType::Error => "Error",
        }
    }
}

/// Port of `TwbMessage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub kind: MessageType,
    pub text: String,
}

impl std::fmt::Display for Message {
    /// `TwbMessage.ToString`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.text)
    }
}

/// Port of `TwbModGroupItemFlags`, the flags an item is written with and
/// the state `mgiCheckValid` leaves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemFlags {
    /// `mgifOptional` (`+`).
    pub optional: bool,
    /// `mgifIsTarget`: its records can be hidden.
    pub is_target: bool,
    /// `mgifIsSource`: its records hide the targets above it.
    pub is_source: bool,
    /// `mgifForbidden` (`!`).
    pub forbidden: bool,
    /// `mgifIgnoreLoadOrderAlways` (`}`).
    pub ignore_load_order_always: bool,
    /// `mgifIgnoreLoadOrderInBlock` (`{`).
    pub ignore_load_order_in_block: bool,
    /// `mgifValid`.
    pub valid: bool,
    /// `mgifHasFile`: the module is loaded and matches the CRC32s.
    pub has_file: bool,
}

/// Port of `TwbModGroupItem`.
#[derive(Debug, Clone)]
pub struct ModGroupItem {
    pub flags: ItemFlags,
    /// `mgiFileName`.
    pub file_name: String,
    /// `mgiModule`: the index of the module in [`Modules`], `None` for
    /// upstream's `_InvalidModule`.
    pub module: Option<usize>,
    /// `mgiCRC32s`.
    pub crc32s: Vec<u32>,
    /// `mgiValidMsgs`.
    pub valid_msgs: Vec<Message>,
}

/// `TwbCRC32.AssignFromString`: exactly eight hexadecimal digits.
pub fn crc32_from_string(text: &str) -> Option<u32> {
    if text.len() != 8 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(text, 16).ok()
}

impl ModGroupItem {
    /// Port of `mgiLoad`: an item from a line of its section; `None` for a
    /// line that is not one (empty, a comment, no file name, more than one
    /// `:`).
    pub fn load(line: &str, modules: &Modules) -> Option<ModGroupItem> {
        let line = trim(line);
        if line.is_empty() || line.starts_with(';') {
            return None;
        }
        let mut flags = ItemFlags {
            is_target: true,
            is_source: true,
            ..ItemFlags::default()
        };
        let mut rest = line;
        for (at, c) in line.char_indices() {
            match c {
                ' ' => {}
                '+' => flags.optional = true,
                // Neither hides nor is being hidden.
                '-' => {
                    flags.is_target = false;
                    flags.is_source = false;
                }
                '!' => {
                    flags.forbidden = true;
                    flags.is_target = false;
                    flags.is_source = false;
                }
                // Being hidden, does not hide others.
                '@' => flags.is_source = false,
                // Hides others, is not being hidden.
                '#' => flags.is_target = false,
                '}' => flags.ignore_load_order_always = true,
                '{' => flags.ignore_load_order_in_block = true,
                _ => {
                    rest = &line[at..];
                    break;
                }
            }
            rest = &line[at + c.len_utf8()..];
        }
        let rest = trim(rest);
        if rest.is_empty() {
            return None;
        }
        // `Split` keeps an empty part after the last separator.
        let fragments: Vec<&str> = rest.split(':').map(trim).collect();
        if fragments.len() > 2 {
            return None;
        }
        let file_name = fragments[0];
        if file_name.is_empty() {
            return None;
        }
        let mut crc32s = Vec::new();
        if let Some(list) = fragments.get(1) {
            crc32s = list
                .split(',')
                .map(trim)
                .filter(|part| !part.is_empty())
                .filter_map(crc32_from_string)
                .collect();
            // A list without one valid CRC32 matches no file.
            if crc32s.is_empty() {
                crc32s.push(0xFFFF_FFFF);
            }
        }
        Some(ModGroupItem {
            flags,
            file_name: file_name.to_owned(),
            module: modules.by_name(file_name),
            crc32s,
            valid_msgs: Vec::new(),
        })
    }

    /// Port of `mgiCheckValid`.
    fn check_valid(&mut self, modules: &Modules) {
        const NO_CRC_MATCH: &str =
            "module \"{}\" is present, but will be ignored as it doesn't match any of the specified CRC32s";
        self.valid_msgs.clear();
        self.flags.valid = false;
        self.flags.has_file = false;
        if let Some(module) = self.module.map(|index| &modules.modules[index])
            && let Some(file) = &module.file
        {
            if self.crc32s.is_empty() || self.crc32s.contains(&file.crc32()) {
                self.flags.has_file = true;
            } else {
                let (kind, which) = if self.flags.forbidden {
                    (MessageType::Hint, "Forbidden")
                } else if self.flags.optional {
                    (MessageType::Warning, "Optional")
                } else {
                    (MessageType::Error, "Required")
                };
                self.valid_msgs.push(Message {
                    kind,
                    text: format!("{which} {}", NO_CRC_MATCH.replace("{}", &self.file_name)),
                });
            }
        }
        if self.flags.has_file && self.flags.forbidden {
            return;
        }
        if self.flags.has_file || self.flags.optional {
            self.flags.valid = true;
        }
    }

    /// Port of `mgiFlagFilesMissingCRC`: whether the module of the item
    /// lacks the current CRC32 in a list (`mfModGroupMissingCurrentCRC`) or
    /// has no list (`mfModGroupMissingAnyCRC`).
    fn files_missing_crc(&self, modules: &Modules) -> Option<(usize, CrcMissing)> {
        if self.flags.forbidden {
            return None;
        }
        let index = self.module?;
        let crc = modules.modules[index].get_crc32()?;
        if self.crc32s.is_empty() {
            Some((index, CrcMissing::Any))
        } else if !self.crc32s.contains(&crc) {
            Some((index, CrcMissing::Current))
        } else {
            None
        }
    }

    /// Port of `mgiNeedsCRCUpdateForTaggedFiles`.
    fn needs_crc_update_for_tagged_files(&self, modules: &Modules, tagged: &[usize], add: bool, update: bool) -> bool {
        if self.flags.forbidden {
            return false;
        }
        let Some(index) = self.module else {
            return false;
        };
        if !tagged.contains(&index) {
            return false;
        }
        match modules.modules[index].get_crc32() {
            Some(crc) if !self.crc32s.is_empty() => !self.crc32s.contains(&crc) && update,
            Some(_) => add,
            None => false,
        }
    }

    /// Port of `mgiUpdateCRC`: adds the module's current CRC32 to an item
    /// without a list (`add`) or whose list lacks it (`update`).
    fn update_crc(&mut self, modules: &Modules, add: bool, update: bool) -> bool {
        if self.flags.forbidden {
            return false;
        }
        let Some(index) = self.module else {
            return false;
        };
        let Some(crc) = modules.modules[index].get_crc32() else {
            return false;
        };
        let result = if self.crc32s.is_empty() {
            add
        } else {
            !self.crc32s.contains(&crc) && update
        };
        if result {
            self.crc32s.push(crc);
        }
        result
    }
}

impl std::fmt::Display for ModGroupItem {
    /// Port of `TwbModGroupItem.ToString`: the line the item is written
    /// as. UPSTREAM-QUIRK: a forbidden item is written `!-name`, as it is
    /// neither a source nor a target.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let flags = &self.flags;
        if flags.ignore_load_order_always {
            f.write_str("}")?;
        } else if flags.ignore_load_order_in_block {
            f.write_str("{")?;
        }
        if flags.forbidden {
            f.write_str("!")?;
        } else if flags.optional {
            f.write_str("+")?;
        }
        match (flags.is_target, flags.is_source) {
            (true, true) => {}
            (true, false) => f.write_str("@")?,
            (false, true) => f.write_str("#")?,
            (false, false) => f.write_str("-")?,
        }
        f.write_str(&self.file_name)?;
        if !self.crc32s.is_empty() {
            f.write_str(":")?;
            for (i, crc) in self.crc32s.iter().enumerate() {
                if i > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{crc:08X}")?;
            }
        }
        Ok(())
    }
}

/// Which CRC32 a module lacks in an item (`mfModGroupMissingAnyCRC`,
/// `mfModGroupMissingCurrentCRC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrcMissing {
    Any,
    Current,
}

/// Port of `TwbModGroup`.
#[derive(Debug, Clone)]
pub struct ModGroup {
    /// `mgModGroupsFile`: the index of the file in [`ModGroups::files`].
    pub file: usize,
    /// `mgfValid`.
    pub valid: bool,
    /// `mgName`.
    pub name: String,
    /// `mgItems`.
    pub items: Vec<ModGroupItem>,
    /// `mgValidMsgs`.
    pub valid_msgs: Vec<Message>,
}

impl ModGroup {
    /// A mod group of the lines of a section (`mgLoad`).
    pub fn load(file: usize, name: &str, lines: &[String], modules: &Modules) -> ModGroup {
        ModGroup {
            file,
            valid: false,
            name: name.to_owned(),
            items: lines
                .iter()
                .filter_map(|line| ModGroupItem::load(line, modules))
                .collect(),
            valid_msgs: Vec::new(),
        }
    }

    /// Port of `mgCheckValid`: a mod group is valid when each item is
    /// (a required one is loaded with a listed CRC32, a forbidden one is
    /// not loaded), the loaded items load in the order of the group (but
    /// for the `}` items, and the `{` items among each other), and at least
    /// one source has a target above it.
    pub fn check_valid(&mut self, modules: &Modules) {
        self.valid_msgs.clear();
        self.valid = false;
        let mut any_invalid = false;
        let mut source_count = 0;
        let mut target_count = 0;
        let mut last_load_order = -1;
        let mut last_load_order_index = 0;
        let mut high_load_order_in_block = -1;
        let mut high_load_order_in_block_index = 0;
        for i in 0..self.items.len() {
            self.items[i].check_valid(modules);
            let item = &self.items[i];
            if !item.flags.valid {
                any_invalid = true;
                continue;
            }
            if !item.flags.has_file {
                continue;
            }
            if item.flags.is_source {
                if target_count > 0 {
                    source_count += 1;
                } else if i > 0 {
                    self.valid_msgs.push(Message {
                        kind: MessageType::Hint,
                        text: format!(
                            "\"{}\" is ignored as a source as it has no valid targets above it",
                            item.file_name
                        ),
                    });
                }
            }
            if item.flags.is_target {
                target_count += 1;
            }
            if item.flags.ignore_load_order_always {
                continue;
            }
            // A loaded item has a module.
            let load_order = item.module.map_or(i32::MAX, |index| modules.modules[index].load_order);
            if item.flags.ignore_load_order_in_block {
                if load_order > high_load_order_in_block {
                    high_load_order_in_block = load_order;
                    high_load_order_in_block_index = i;
                }
            } else if high_load_order_in_block >= 0 {
                if high_load_order_in_block > last_load_order {
                    last_load_order = high_load_order_in_block;
                    last_load_order_index = high_load_order_in_block_index;
                }
                high_load_order_in_block = -1;
            }
            if last_load_order >= 0 && load_order < last_load_order {
                any_invalid = true;
                self.valid_msgs.push(Message {
                    kind: MessageType::Error,
                    text: format!(
                        "\"{}\" has a lower load order than \"{}\"",
                        item.file_name, self.items[last_load_order_index].file_name
                    ),
                });
            }
            if !item.flags.ignore_load_order_in_block {
                last_load_order = load_order;
                last_load_order_index = i;
            }
        }
        if source_count < 1 {
            self.valid_msgs.push(Message {
                kind: MessageType::Error,
                text: "No active sources".to_owned(),
            });
        }
        if !any_invalid && source_count > 0 {
            self.valid = true;
        }
    }

    /// Port of `GetValidationMessages`: the messages of the items, then the
    /// group's.
    pub fn validation_messages(&self) -> Vec<&Message> {
        self.items
            .iter()
            .flat_map(|item| item.valid_msgs.iter())
            .chain(self.valid_msgs.iter())
            .collect()
    }

    /// Port of `ToStrings`: the section of the mod group as written.
    pub fn to_strings(&self) -> Vec<String> {
        let mut lines = vec![format!("[{}]", self.name)];
        lines.extend(self.items.iter().map(ModGroupItem::to_string));
        lines
    }

    /// Port of `mgTagTargetFiles`: the modules this valid group makes
    /// targets of `source`: the loaded targets above each loaded source item
    /// of the module.
    fn tag_target_files(&self, source: usize, tagged: &mut [bool]) {
        if !self.valid {
            return;
        }
        for (i, item) in self.items.iter().enumerate() {
            if item.module != Some(source) || !(item.flags.is_source && item.flags.has_file) {
                continue;
            }
            for target in self.items[..i].iter().rev() {
                if let Some(module) = target.module
                    && target.flags.is_target
                    && target.flags.has_file
                {
                    tagged[module] = true;
                }
            }
        }
    }
}

/// Port of `TwbModGroupsFile`.
#[derive(Debug, Clone)]
pub struct ModGroupsFile {
    /// `mgfFileName`.
    pub file_name: PathBuf,
    /// `mgfModules`: the modules whose `.modgroups` file this is; none for
    /// the program's own file.
    pub modules: Vec<usize>,
    /// `mgfModGroups`.
    pub mod_groups: Vec<ModGroup>,
    /// `mgffValid`.
    pub valid: bool,
}

impl ModGroupsFile {
    /// Port of `mgfAnyModuleHasFile`.
    pub fn any_module_has_file(&self, modules: &Modules) -> bool {
        self.modules.iter().any(|&index| modules.modules[index].has_file())
    }
}

/// A mod group: the index of its file and its index in the file.
pub type ModGroupRef = (usize, usize);

/// The mod group files of a load (`_ModGroupFiles`).
pub struct ModGroups {
    pub modules: Modules,
    pub files: Vec<ModGroupsFile>,
}

/// Port of `wbExpandFileName`: a name without a folder is in the data
/// folder, but for the game's executable.
fn expand_file_name(data_path: &str, name: &str) -> PathBuf {
    if !name.contains(['\\', '/', ':']) && !same_text(name, &xedit_core::interface::globals::game_exe_name()) {
        PathBuf::from(format!("{data_path}{name}"))
    } else {
        PathBuf::from(name)
    }
}

/// `ChangeFileExt`.
fn change_file_ext(name: &str, extension: &str) -> String {
    match name.rfind(['.', '\\', '/', ':']) {
        Some(at) if name[at..].starts_with('.') => format!("{}{extension}", &name[..at]),
        _ => format!("{name}{extension}"),
    }
}

impl ModGroups {
    /// Port of `wbLoadModGroups`: the `.modgroups` file of every module, in
    /// the order of the modules (a file that several modules share once,
    /// with each of them), then the program's own file (`global`), with
    /// the validity of every group checked.
    pub fn load(modules: Modules, data_path: &str, global: &Path) -> std::io::Result<ModGroups> {
        let mut files: Vec<ModGroupsFile> = Vec::new();
        for i in 0..=modules.modules.len() {
            let name = match modules.modules.get(i) {
                Some(module) => expand_file_name(data_path, &change_file_ext(&module.name, ".modgroups")),
                None => global.to_owned(),
            };
            let known = files
                .iter()
                .position(|file| same_text(&file.file_name.to_string_lossy(), &name.to_string_lossy()));
            let index = match known {
                Some(index) => Some(index),
                None if !name.as_os_str().is_empty() && name.is_file() => {
                    let index = files.len();
                    let ini = MemIniFile::open(&name)?;
                    let mod_groups = ini
                        .read_sections()
                        .iter()
                        .map(|section| ModGroup::load(index, section, &ini.read_section_values(section), &modules))
                        .collect();
                    files.push(ModGroupsFile {
                        file_name: name,
                        modules: Vec::new(),
                        mod_groups,
                        valid: false,
                    });
                    Some(index)
                }
                None => None,
            };
            if let Some(index) = index
                && i < modules.modules.len()
            {
                files[index].modules.push(i);
            }
        }
        let mut mod_groups = ModGroups { modules, files };
        for i in 0..mod_groups.files.len() {
            mod_groups.check_file_valid(i);
        }
        Ok(mod_groups)
    }

    /// Port of `mgfCheckValid`: a file is valid when one of its groups is
    /// and it belongs to no module or to a loaded one.
    fn check_file_valid(&mut self, index: usize) {
        let modules = &self.modules;
        let file = &mut self.files[index];
        let mut any_valid = false;
        for group in &mut file.mod_groups {
            group.check_valid(modules);
            any_valid |= group.valid;
        }
        file.valid = any_valid && (file.modules.is_empty() || file.any_module_has_file(modules));
    }

    pub fn group(&self, group: ModGroupRef) -> &ModGroup {
        &self.files[group.0].mod_groups[group.1]
    }

    /// Port of `wbModGroupsByName`: the groups of the files in file order
    /// (only the valid groups of valid files with `valid_only`), sorted by
    /// name without regard to case, keeping the file order of equal names.
    pub fn by_name(&self, valid_only: bool) -> Vec<ModGroupRef> {
        let mut list = Vec::new();
        for (i, file) in self.files.iter().enumerate() {
            if valid_only && !file.valid {
                continue;
            }
            for (j, group) in file.mod_groups.iter().enumerate() {
                if !valid_only || group.valid {
                    list.push((i, j));
                }
            }
        }
        list.sort_by(|&a, &b| compare_text(&self.group(a).name, &self.group(b).name));
        list
    }

    /// Port of `ShowValidationMessages`: for each group with messages, a
    /// line that says whether it is valid and its messages, each as
    /// ` - <type>: <text>`. An invalid group of a file whose modules are
    /// not loaded says nothing. UPSTREAM-QUIRK: neither does an invalid
    /// group of the program's own file, which belongs to no module.
    pub fn validation_messages(&self, groups: &[ModGroupRef]) -> Vec<String> {
        let mut lines = Vec::new();
        for &reference in groups {
            let group = self.group(reference);
            let header = if group.valid {
                "is valid, but has messages:"
            } else {
                if !self.files[group.file].any_module_has_file(&self.modules) {
                    continue;
                }
                "is invalid:"
            };
            let messages = group.validation_messages();
            if !messages.is_empty() {
                lines.push(format!("ModGroup \"{}\" {header}", group.name));
                lines.extend(messages.iter().map(|message| format!(" - {message}")));
            }
        }
        lines
    }

    /// Port of `TwbModGroupPtrsHelper.Activate`: for each module in load
    /// order the earlier modules that one of the groups makes its targets.
    pub fn activate(&self, groups: &[ModGroupRef]) -> Activation {
        let count = self.modules.modules.len();
        let mut targets: Vec<Vec<usize>> = vec![Vec::new(); count];
        for (i, module_targets) in targets.iter_mut().enumerate() {
            let mut tagged = vec![false; count];
            for &group in groups {
                self.group(group).tag_target_files(i, &mut tagged);
            }
            *module_targets = (0..i).rev().filter(|&j| tagged[j]).collect();
        }
        let mut sources: Vec<Vec<usize>> = vec![Vec::new(); count];
        let mut exist = false;
        for (i, module_targets) in targets.iter().enumerate() {
            for &target in module_targets {
                exist = true;
                sources[target].push(i);
            }
        }
        Activation {
            targets,
            sources,
            exist,
        }
    }

    /// Port of `FlagFilesMissingCRC` over `groups`: the modules that lack
    /// a CRC32 in an item, each once, in module order, with what they lack
    /// (a module can lack both).
    pub fn files_missing_crc(&self, groups: &[ModGroupRef]) -> (Vec<usize>, Vec<usize>) {
        let mut any = vec![false; self.modules.modules.len()];
        let mut current = vec![false; self.modules.modules.len()];
        for &group in groups {
            for item in &self.group(group).items {
                match item.files_missing_crc(&self.modules) {
                    Some((index, CrcMissing::Any)) => any[index] = true,
                    Some((index, CrcMissing::Current)) => current[index] = true,
                    None => {}
                }
            }
        }
        let collect = |flags: Vec<bool>| {
            flags
                .into_iter()
                .enumerate()
                .filter(|(_, set)| *set)
                .map(|(i, _)| i)
                .collect()
        };
        (collect(any), collect(current))
    }

    /// Port of `FlagModGroupsNeedingCRCUpdateForTaggedFiles` and
    /// `FilteredByFlag(mgfNeedCRCUpdate)`: the groups with an item of one of
    /// the `tagged` modules that would get its CRC32.
    pub fn needing_crc_update(
        &self,
        groups: &[ModGroupRef],
        tagged: &[usize],
        add: bool,
        update: bool,
    ) -> Vec<ModGroupRef> {
        groups
            .iter()
            .copied()
            .filter(|&group| {
                self.group(group)
                    .items
                    .iter()
                    .any(|item| item.needs_crc_update_for_tagged_files(&self.modules, tagged, add, update))
            })
            .collect()
    }

    /// Port of `mgUpdateCRC` without the save: adds the current CRC32s to
    /// the items of a group; whether one changed. UPSTREAM-QUIRK: every
    /// item of the group gets its module's CRC32, not only the items of the
    /// modules the user chose.
    pub fn update_crc(&mut self, group: ModGroupRef, add: bool, update: bool) -> bool {
        let modules = &self.modules;
        let mut changed = false;
        for item in &mut self.files[group.0].mod_groups[group.1].items {
            changed = item.update_crc(modules, add, update) || changed;
        }
        changed
    }

    /// Port of `mgSaveToFile`: the file of the group read again as an ini
    /// file, without the group's section, followed by the group as
    /// [`ModGroup::to_strings`] writes it; written in `TEncoding.Default`.
    pub fn save_to_file(&self, group: ModGroupRef) -> std::io::Result<()> {
        let group = self.group(group);
        write_group_section(&self.files[group.file].file_name, &group.name, &group.to_strings())
    }
}

/// The file of a mod group with its section `name` replaced by `lines` at
/// the end, as `mgSaveToFile` and the GUI's "Edit ModGroup" write it: the
/// other sections as `TMemIniFile.GetStrings` gives them (comments and
/// blank lines gone), and the whole list saved by a new `TStringList`, so
/// in `TEncoding.Default` whatever encoding the file had.
pub fn write_group_section(file_name: &Path, name: &str, lines: &[String]) -> std::io::Result<()> {
    let mut ini = MemIniFile::open(file_name)?;
    ini.erase_section(name);
    let mut all = ini.get_strings();
    all.extend(lines.iter().cloned());
    save_strings(file_name, &all, None)
}

/// Delphi `CompareText`: the comparison of two strings with the ASCII
/// letters upper case.
pub fn compare_text(a: &str, b: &str) -> std::cmp::Ordering {
    let upper = |text: &str| -> Vec<u16> {
        text.encode_utf16()
            .map(|unit| {
                if (u16::from(b'a')..=u16::from(b'z')).contains(&unit) {
                    unit - 32
                } else {
                    unit
                }
            })
            .collect()
    };
    upper(a).cmp(&upper(b))
}

/// The result of `Activate` (`miModGroupTargets`, `miModGroupSources`).
#[derive(Debug, Clone, Default)]
pub struct Activation {
    /// For each module, the earlier modules whose records its records hide,
    /// the nearest first.
    pub targets: Vec<Vec<usize>>,
    /// For each module, the later modules that hide its records.
    pub sources: Vec<Vec<usize>>,
    /// Whether a module hides another (`ModGroupsExist`).
    pub exist: bool,
}

impl Activation {
    /// The mod groups' part of `NodeDatasForMainRecord`: of the records of
    /// a FormID (more than two, in load order; `modules` holds the module of
    /// each record's file), the ones the comparison keeps. A record of a
    /// module that is a target of another record's module is left out,
    /// unless it is the first or the last record with a module.
    pub fn keep(&self, modules: &[Option<usize>]) -> Vec<bool> {
        let mut tagged: Vec<usize> = Vec::new();
        let first = modules.iter().flatten().next().copied();
        let last = modules.iter().flatten().last().copied();
        for module in modules.iter().flatten() {
            tagged.extend(self.targets.get(*module).into_iter().flatten().copied());
        }
        tagged.retain(|module| Some(*module) != first && Some(*module) != last);
        modules
            .iter()
            .map(|module| module.is_none_or(|module| !tagged.contains(&module)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_modules() -> Modules {
        Modules::default()
    }

    #[test]
    fn items_parse_and_write_as_upstream() {
        let modules = no_modules();
        let cases = [
            ("Plain.esp", "Plain.esp", 0),
            (
                "  +Optional.esp:1234abcd , 5678EF01 ",
                "+Optional.esp:1234ABCD,5678EF01",
                2,
            ),
            ("!Old.esp:DEADBEEF", "!-Old.esp:DEADBEEF", 1),
            ("{@Block.esp", "{@Block.esp", 0),
            ("}#Free.esp", "}#Free.esp", 0),
            ("- Neither.esp", "-Neither.esp", 0),
            ("Mod.esp:", "Mod.esp:FFFFFFFF", 1),
            ("Mod.esp:,,", "Mod.esp:FFFFFFFF", 1),
            ("Mod.esp:XYZ", "Mod.esp:FFFFFFFF", 1),
            ("}{Both.esp", "}Both.esp", 0),
        ];
        for (line, written, crcs) in cases {
            let item = ModGroupItem::load(line, &modules).unwrap_or_else(|| panic!("{line}"));
            assert_eq!(item.to_string(), written, "{line}");
            assert_eq!(item.crc32s.len(), crcs, "{line}");
        }
        for line in ["", "   ", "; comment", "+", "a:b:c", ":12345678"] {
            assert!(ModGroupItem::load(line, &modules).is_none(), "{line}");
        }
    }

    #[test]
    fn the_first_and_last_record_are_kept() {
        // Modules 0..4; module 3 hides 1 and 2, module 2 hides 0.
        let activation = Activation {
            targets: vec![vec![], vec![], vec![0], vec![2, 1]],
            sources: vec![],
            exist: true,
        };
        assert_eq!(
            activation.keep(&[Some(0), Some(1), Some(2), Some(3)]),
            [true, false, false, true]
        );
        // The last record's module is never hidden.
        assert_eq!(activation.keep(&[Some(0), Some(1), Some(2)]), [true, true, true]);
        assert_eq!(activation.keep(&[None, Some(1), Some(3)]), [true, true, true]);
    }

    #[test]
    fn compare_text_folds_ascii_letters_only() {
        assert_eq!(compare_text("abc", "ABC"), std::cmp::Ordering::Equal);
        assert_eq!(compare_text("a_", "AB"), std::cmp::Ordering::Greater);
        assert_eq!(compare_text("A", "a1"), std::cmp::Ordering::Less);
    }
}
