// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of the master management of `TwbFile` (`AddMasters`,
//! `AddMastersIfMissing`, `AddMasterIfMissing`, `CleanMasters`,
//! `SortMasters`, `HasMaster`, `GetIsEditable`), of the `MastersUpdated`
//! chain that rewrites every FormID of a file after its masters changed
//! (`TwbFile`, `TwbContainer`, `TwbMainRecord`, `TwbGroupRecord`,
//! `TwbSubRecord`, `TwbUnion`, `TwbValue`), and of the `FindUsedMasters`
//! chain that `CleanMasters` reads.
//!
//! State: the FormIDs are rewritten without the two views of the masters
//! upstream keeps while it updates them (`fsMastersUpdating` with
//! `flOldMasters`, `eMastersGeneration`): the definitions that run code on
//! an update (`dfAfterSetOnIDUpdate`, Starfield only) see the new masters
//! already. The references of a record (`mrReferences`) are not built by
//! the port, so a record is always walked element by element, which upstream
//! does whenever the references are out of date. The `INFO` records of a
//! topic group are not sorted again after the update (`TwbGroupRecord.Sort`
//! of a type 7 group is not ported).

use std::sync::Arc;

use crate::delphi::path_file_name;
use crate::interface::element::{Container, Element, ElementRef, File};
use crate::interface::form_id::{
    FileID, MastersUpdate, ModuleType, SlotCounts, UsedMasters, mark_used_master, new_used_masters,
};
use crate::interface::globals::{
    GameMode, ToolSource, allow_edit_game_master, allow_esp_masters, begin_internal_edit, edit_allowed,
    end_internal_edit, enforce_all_masters, game_master_esm, game_mode, game_name, is_internal_edit,
    is_light_supported, is_starfield, red_pill, tool_source,
};
use crate::interface::misc::{EditError, progress};
use crate::interface::types::{DefFlag, ElementType, FileState, Signature};

use super::write::ElementState;
use super::{ElementImpl, FILES_MAP, FileImpl, GroupRecordImpl, MainRecordImpl, edit, find_in_files_map, value};

/// Port of `GetLoadedFileByName`: the loaded file with the name, compared
/// without case.
pub fn get_loaded_file_by_name(name: &str) -> Option<Arc<FileImpl>> {
    FILES_MAP
        .read()
        .unwrap()
        .iter()
        .find(|file| file.get_name().eq_ignore_ascii_case(name))
        .cloned()
}

/// Port of `ExtractFileExt`: the extension with its dot, or nothing.
fn extract_file_ext(name: &str) -> &str {
    match name.rfind(['.', '\\', '/', ':']) {
        Some(index) if name[index..].starts_with('.') => &name[index..],
        _ => "",
    }
}

/// Port of `CompareLoadOrder` on files: by load order, then by identity.
fn compare_load_order(a: &Arc<FileImpl>, b: &Arc<FileImpl>) -> std::cmp::Ordering {
    a.load_order()
        .cmp(&b.load_order())
        .then((Arc::as_ptr(a) as usize).cmp(&(Arc::as_ptr(b) as usize)))
}

/// A `TStringList` with `Sorted` and `dupIgnore`: names kept in the order
/// of `CompareText`, each once.
#[derive(Default)]
struct SortedFiles(Vec<(String, Arc<FileImpl>)>);

impl SortedFiles {
    fn position(&self, name: &str) -> Result<usize, usize> {
        let key = name.to_uppercase();
        self.0.binary_search_by(|(known, _)| known.to_uppercase().cmp(&key))
    }

    fn add(&mut self, file: &Arc<FileImpl>) {
        let name = file.get_name();
        if let Err(index) = self.position(&name) {
            self.0.insert(index, (name, file.clone()));
        }
    }

    fn delete(&mut self, name: &str) {
        if let Ok(index) = self.position(name) {
            self.0.remove(index);
        }
    }
}

/// The FileID a master takes in a list of masters, counted by module type
/// under `wbComplexFileFileID`.
fn create_by_type(module_type: ModuleType, slot: i16) -> FileID {
    match module_type {
        ModuleType::mtFull => FileID::create_full(slot),
        ModuleType::mtMedium => FileID::create_medium(slot),
        ModuleType::mtLight => FileID::create_light(slot),
    }
}

/// The counters of the masters of each module type, as the master loops
/// keep them (`nextFullID`, `nextSmallID`, `nextMediumID`).
#[derive(Default, Clone, Copy)]
struct TypeCounters {
    full: i16,
    light: i16,
    medium: i16,
}

impl TypeCounters {
    fn get(&self, module_type: ModuleType) -> i16 {
        match module_type {
            ModuleType::mtFull => self.full,
            ModuleType::mtMedium => self.medium,
            ModuleType::mtLight => self.light,
        }
    }

    fn inc(&mut self, module_type: ModuleType) {
        match module_type {
            ModuleType::mtFull => self.full += 1,
            ModuleType::mtMedium => self.medium += 1,
            ModuleType::mtLight => self.light += 1,
        }
    }
}

/// `wbBeginInternalEdit(True)` around `body`, ended on every path.
fn with_internal_edit<T>(body: impl FnOnce() -> T) -> T {
    let internal = begin_internal_edit(true);
    let result = body();
    if internal {
        end_internal_edit();
    }
    result
}

/// `BeginUpdate` around `body`, ended on every path.
fn with_update<T>(element: &dyn ElementImpl, body: impl FnOnce() -> T) -> T {
    element.begin_update();
    let result = body();
    element.end_update();
    result
}

impl FileImpl {
    /// Port of `GetIsNotPlugin`: a save or co-save.
    fn is_not_plugin(&self) -> bool {
        tool_source() == ToolSource::tsSaves && !super::is_module(&self.fl_file_name)
    }

    /// Port of `TwbFile.GetIsEditable`.
    pub fn get_is_editable(&self) -> bool {
        let states = self.get_file_states();
        let result = is_internal_edit()
            || (edit_allowed()
                && (!states.contains(FileState::fsIsGameMaster) || allow_edit_game_master())
                && !states.contains(FileState::fsIsHardcoded)
                && (!states.contains(FileState::fsIsCompareLoad) || states.contains(FileState::fsIsDeltaPatch)));
        if is_starfield()
            && !red_pill()
            && (states.contains(FileState::fsIsGameMaster)
                || states.contains(FileState::fsIsHardcoded)
                || states.contains(FileState::fsIsOfficial))
        {
            return false;
        }
        result
    }

    /// Port of `TwbFile.IsElementEditable`.
    pub fn is_element_editable(&self) -> bool {
        is_internal_edit() || self.get_is_editable()
    }

    /// The error of a change to a file that is not editable.
    fn not_editable(&self) -> EditError {
        format!("File \"{}\" is not editable", self.get_name())
    }

    /// Port of `GetModuleType`: the module type of the slot of the file.
    pub fn module_type(&self) -> ModuleType {
        self.get_load_order_file_id().module_type()
    }

    /// Port of `HasMaster`: whether a master has the name, compared
    /// without case.
    pub fn has_master(&self, file_name: &str) -> bool {
        self.fl_masters
            .read()
            .unwrap()
            .iter()
            .any(|master| master.get_name().eq_ignore_ascii_case(file_name))
    }

    /// Port of `GetMasterIndexForFileID`: the index of the master a file
    /// FileID points to, or -1.
    pub fn get_master_index_for_file_id(&self, file_id: FileID) -> i32 {
        let masters = self.fl_masters.read().unwrap();
        if crate::interface::globals::complex_file_file_id() {
            let mut counters = TypeCounters::default();
            for (index, master) in masters.iter().enumerate() {
                let module_type = master.module_type();
                let slot = counters.get(module_type);
                counters.inc(module_type);
                let matches = match module_type {
                    ModuleType::mtLight => file_id.is_light_slot() && file_id.light_slot() == slot,
                    ModuleType::mtMedium => file_id.is_medium_slot() && file_id.medium_slot() == slot,
                    ModuleType::mtFull => file_id.is_full_slot() && file_id.full_slot() == slot,
                };
                if matches {
                    return index as i32;
                }
            }
            return -1;
        }
        let slot = i32::from(file_id.full_slot());
        if slot < masters.len() as i32 { slot } else { -1 }
    }

    /// The masters as `TwbSlotCounts.Create` counts them.
    fn slot_counts(masters: &[Arc<FileImpl>]) -> SlotCounts {
        SlotCounts::create(masters.iter().map(|master| master.module_type()))
    }

    /// The `Master Files` element of the header and its entries.
    fn master_files(&self) -> Result<(Arc<MainRecordImpl>, Option<ElementRef>), EditError> {
        let header = self
            .header()
            .ok_or_else(|| format!("Unexpected error reading file \"{}\"", self.fl_file_name))?;
        let master_files = header.get_element_by_name("Master Files");
        Ok((header, master_files))
    }

    /// Port of `AddMasterIfMissing`.
    pub fn add_master_if_missing(
        self: &Arc<Self>,
        master: &str,
        sort_masters: bool,
        silent: bool,
    ) -> Result<(), EditError> {
        if self.has_master(master) {
            return Ok(());
        }
        self.add_masters_if_missing(&[master], sort_masters, silent)
    }

    /// Port of `AddMastersIfMissing`: the loaded files named that are not
    /// masters yet (with their own masters under `wbEnforceAllMasters`)
    /// become masters in load order, and the masters are sorted when
    /// `sort_masters` is set.
    pub fn add_masters_if_missing<S: AsRef<str>>(
        self: &Arc<Self>,
        masters: &[S],
        sort_masters: bool,
        silent: bool,
    ) -> Result<(), EditError> {
        let mut list = SortedFiles::default();
        for name in masters {
            let name = name.as_ref();
            if self.has_master(name) {
                continue;
            }
            let file = get_loaded_file_by_name(name).ok_or_else(|| {
                format!("[AddMAddMastersIfMissingasters] Requested file to add is not loaded: \"{name}\"")
            })?;
            // The masters of the masters, for the games that need them.
            if enforce_all_masters() {
                for master in file.all_masters() {
                    list.add(&master);
                }
            }
            list.add(&file);
        }
        for master in self.masters() {
            list.delete(&master.get_name());
        }
        list.delete(&self.get_name());
        if list.0.is_empty() {
            return Ok(());
        }
        // `CustomSort(CompareLoadOrder)` on the string list: by load order.
        list.0.sort_by_key(|(_, file)| file.load_order());
        let names: Vec<String> = list.0.into_iter().map(|(name, _)| name).collect();
        self.add_masters(&names, silent)?;
        if sort_masters {
            self.sort_masters()?;
        }
        Ok(())
    }

    /// Port of `AddMasters`: the files named become masters in the order
    /// given, each with a new entry in `Master Files`, and every FormID of
    /// the file that points to the file itself moves past the new masters.
    /// Only `.esm`, `.esp` and (where the game has light modules) `.esl`
    /// names are taken. Fails when a file is not loaded, and after the
    /// masters that fit when the list is full.
    pub fn add_masters<S: AsRef<str>>(self: &Arc<Self>, masters: &[S], silent: bool) -> Result<(), EditError> {
        let old_masters = self.masters();
        let mut list = Vec::new();
        for name in masters {
            let name = name.as_ref().trim();
            let extension = extract_file_ext(name);
            if extension.eq_ignore_ascii_case(".esp") && !allow_esp_masters() {
                return Err(format!(
                    "[AddMasters] You cannot add a .esp as a master in {}.",
                    game_name()
                ));
            }
            if extension.eq_ignore_ascii_case(".esm")
                || extension.eq_ignore_ascii_case(".esp")
                || (is_light_supported() && extension.eq_ignore_ascii_case(".esl"))
            {
                list.push(name.to_owned());
            }
        }
        if list.is_empty() {
            return Ok(());
        }
        // UPSTREAM-QUIRK: a failure inside the loop leaves the masters added
        // before it without the FormID update below.
        let not_all_added = self.add_masters_inner(&list, silent)?;
        let new_masters = self.masters();
        if game_mode() >= GameMode::gmTES4 && old_masters.len() != new_masters.len() {
            let update = MastersUpdate {
                old: Vec::new(),
                new: Vec::new(),
                old_count: Self::slot_counts(&old_masters),
                new_count: Self::slot_counts(&new_masters),
            };
            self.masters_updated(&update)?;
            self.sort_records();
        }
        self.set_modified(true);
        if not_all_added {
            return Err(format!(
                "Only {} of {} masters could be added. Master list now contains {} entries and is full.",
                new_masters.len() - old_masters.len(),
                list.len(),
                new_masters.len()
            ));
        }
        Ok(())
    }

    /// The `Inner` procedure of `AddMasters`: the entries of `Master Files`
    /// and the masters. Returns `NotAllAdded`.
    fn add_masters_inner(self: &Arc<Self>, list: &[String], silent: bool) -> Result<bool, EditError> {
        if !self.is_element_editable() {
            return Err(self.not_editable());
        }
        if self.is_not_plugin() {
            return Ok(false);
        }
        let (header, master_files) = self.master_files()?;
        let mut is_new = false;
        let master_files = match master_files {
            Some(master_files) => master_files,
            None => {
                is_new = true;
                header
                    .add("Master Files", false)?
                    .ok_or_else(|| "[AddMasters] not Assigned(MasterFiles)".to_owned())?
            }
        };
        let max_master_count = i32::from(FileID::max_full_slot()) + 1;
        let max_light_master_count = i32::from(FileID::max_light_slot()) + 1;
        let max_medium_master_count = i32::from(FileID::max_medium_slot()) + 1;
        with_internal_edit(|| {
            let mut not_all_added = false;
            for name in list {
                let file = get_loaded_file_by_name(name)
                    .ok_or_else(|| format!("[AddMasters] Requested file to add is not loaded: \"{name}\""))?;
                let is_blueprint = file
                    .header()
                    .is_some_and(|header| header.mr_struct().flags.is_blueprint());
                if is_blueprint && is_starfield() {
                    return Err(format!(
                        "[AddMasters] File [{name}] not added. {} does not support blueprint files as masters to other modules.",
                        game_name()
                    ));
                }
                let masters = self.masters();
                let count = |module_type: ModuleType| {
                    masters
                        .iter()
                        .filter(|master| master.module_type() == module_type)
                        .count() as i32
                };
                let full = if crate::interface::globals::complex_file_file_id() {
                    // `-1` for safety in case the module is converted later.
                    match file.module_type() {
                        ModuleType::mtLight => count(ModuleType::mtLight) >= max_light_master_count - 1,
                        ModuleType::mtMedium => count(ModuleType::mtMedium) >= max_medium_master_count - 1,
                        ModuleType::mtFull => count(ModuleType::mtFull) >= max_master_count,
                    }
                } else {
                    masters.len() as i32 >= max_master_count
                };
                if full {
                    not_all_added = true;
                    break;
                }
                let container = master_files
                    .as_element_impl()
                    .ok_or_else(|| "[AddMasters] Master Files is not editable".to_owned())?;
                let entry = if is_new {
                    is_new = false;
                    master_files.as_container().and_then(|c| c.get_element(0))
                } else {
                    container.assign_add()?
                };
                let rec = entry
                    .as_ref()
                    .and_then(|entry| entry.as_container()?.get_record_by_signature(Signature::new(b"MAST")))
                    .ok_or_else(|| "[AddMasters] not Assigned(Rec)".to_owned())?;
                self.add_master_for_edit(name, &file, silent);
                rec.set_edit_value(name)?;
            }
            Ok(not_all_added)
        })
    }

    /// Port of `AddMaster(aFileName, False, False, aSilent)` for a loaded
    /// file: the file from the directory of this file, which is the loaded
    /// one unless the two folders differ.
    fn add_master_for_edit(&self, file_name: &str, loaded: &Arc<FileImpl>, silent: bool) {
        let name = path_file_name(file_name);
        let dir = match self.fl_file_name.rfind(['\\', '/']) {
            Some(index) => &self.fl_file_name[..=index],
            None => "",
        };
        if !silent {
            progress(&format!("[{}] Adding master \"{name}\"", self.get_name()));
        }
        let master = find_in_files_map(&format!("{dir}{name}")).unwrap_or_else(|| loaded.clone());
        self.fl_masters.write().unwrap().push(master);
    }

    /// Port of `CleanMasters`: the masters no FormID of the file points to
    /// leave the file, except the game master (and, under
    /// `wbEnforceAllMasters`, the masters of a used master), and the FormIDs
    /// follow the masters that move down.
    pub fn clean_masters(self: &Arc<Self>) -> Result<(), EditError> {
        if !self.is_element_editable() {
            return Err(self.not_editable());
        }
        self.master_files()?;
        let old_masters = self.masters();
        if old_masters.is_empty() {
            progress(&format!("[{}] Has no masters.", self.get_name()));
            return Ok(());
        }
        // The walk runs before the entries of `Master Files` are taken:
        // it releases the elements of the records it does not change, the
        // file header among them.
        let mut used = new_used_masters();
        self.find_used_masters(&mut used);
        let (header, master_files) = self.master_files()?;
        let master_files = master_files.ok_or_else(|| "[TwbFile.CleanMasters] not Assigned(MasterFiles)".to_owned())?;
        let entries = |master_files: &ElementRef| -> Vec<ElementRef> {
            master_files
                .as_container()
                .map(|c| (0..c.get_element_count()).filter_map(|i| c.get_element(i)).collect())
                .unwrap_or_default()
        };
        let old_entries = entries(&master_files);
        if old_entries.len() != old_masters.len() {
            return Err("[TwbFile.CleanMasters] MasterFiles.ElementCount <> Length(flMasters)".to_owned());
        }
        for (i, entry) in old_entries.iter().enumerate() {
            entry.set_sort_order(i as i32);
        }
        let mut keep: Vec<String> = Vec::new();
        if enforce_all_masters() {
            for (i, master) in old_masters.iter().enumerate() {
                if used[i] {
                    keep.extend(master.all_masters().iter().map(|m| m.get_name().to_uppercase()));
                }
            }
        }
        let mut masters = old_masters.clone();
        let mut old = Vec::new();
        let mut new = Vec::new();
        let mut next = TypeCounters::default();
        let mut last = TypeCounters::default();
        let mut j = 0usize;
        for i in 0..old_masters.len() {
            let master = &old_masters[i];
            let module_type = master.module_type();
            let name = master.get_name();
            if used[i] || name.eq_ignore_ascii_case(&game_master_esm()) || keep.contains(&name.to_uppercase()) {
                if i != j {
                    masters[j] = master.clone();
                    old_entries[i].set_sort_order(j as i32);
                    if crate::interface::globals::complex_file_file_id() {
                        old.push(create_by_type(module_type, last.get(module_type)));
                        new.push(create_by_type(module_type, next.get(module_type)));
                        next.inc(module_type);
                    } else {
                        old.push(FileID::create_full(i as i16));
                        new.push(FileID::create_full(j as i16));
                    }
                } else {
                    next.inc(module_type);
                }
                j += 1;
            } else {
                progress(&format!("[{}] Removing unused master: {name}", self.get_name()));
                old_entries[i].set_sort_order(0x1200);
            }
            last.inc(module_type);
        }
        let mut removed_count = 0;
        if j != old_masters.len() {
            masters.truncate(j);
            *self.fl_masters.write().unwrap() = masters.clone();
            let container = master_files
                .as_element_impl()
                .ok_or_else(|| "[TwbFile.CleanMasters] Master Files is not editable".to_owned())?;
            container.sort_by_sort_order_impl();
            with_internal_edit(|| {
                let current = entries(&master_files);
                for i in (0..current.len()).rev() {
                    if current[i].get_sort_order() == 0x1200 {
                        container.remove_child_at(i as i32, false);
                    }
                }
                container.invalidate_storage();
                if container.container_base().is_some_and(|base| base.element_count() == 0) {
                    master_files.remove();
                }
                header.invalidate_storage();
            });
            self.set_modified(true);
            let remaining = entries(&master_files);
            removed_count = old_masters.len() - remaining.len();
            if remaining.len() != masters.len() {
                return Err("[TwbFile.CleanMasters] Length(flMasters) <> MasterFiles.ElementCount".to_owned());
            }
            if game_mode() >= GameMode::gmTES4 {
                let update = MastersUpdate {
                    old,
                    new,
                    old_count: Self::slot_counts(&old_masters),
                    new_count: Self::slot_counts(&masters),
                };
                self.masters_updated(&update)?;
            }
            self.sort_records();
        }
        progress(&format!(
            "[{}] Removed {removed_count} unused masters.",
            self.get_name()
        ));
        Ok(())
    }

    /// Port of `SortMasters`: the masters in load order, with the entries of
    /// `Master Files` and the FormIDs following them.
    pub fn sort_masters(self: &Arc<Self>) -> Result<(), EditError> {
        if !self.is_element_editable() {
            return Err(self.not_editable());
        }
        if self.is_not_plugin() {
            return Ok(());
        }
        let (_header, master_files) = self.master_files()?;
        let old_masters = self.masters();
        if old_masters.len() <= 1 {
            return Ok(());
        }
        let master_files = master_files.ok_or_else(|| "[TwbFile.SortMasters] not Assigned(MasterFiles)".to_owned())?;
        let entries: Vec<ElementRef> = master_files
            .as_container()
            .map(|c| (0..c.get_element_count()).filter_map(|i| c.get_element(i)).collect())
            .unwrap_or_default();
        if entries.len() != old_masters.len() {
            return Err("[TwbFile.SortMasters] MasterFiles.ElementCount <> Length(flMasters)".to_owned());
        }
        for (i, entry) in entries.iter().enumerate() {
            entry.set_sort_order(i as i32);
        }
        let old_index = |name: &str| {
            old_masters
                .iter()
                .position(|master| master.get_name().eq_ignore_ascii_case(name))
                .unwrap_or(0)
        };
        // Port of `GetOldFileID`: the FileID of a master in the old list.
        let old_file_id = |name: &str| {
            let mut counters = TypeCounters::default();
            for master in &old_masters {
                let module_type = master.module_type();
                if master.get_name().eq_ignore_ascii_case(name) {
                    return create_by_type(module_type, counters.get(module_type));
                }
                counters.inc(module_type);
            }
            FileID::invalid()
        };
        let mut masters = old_masters.clone();
        masters.sort_by(compare_load_order);
        let mut old = Vec::new();
        let mut new = Vec::new();
        let mut next = TypeCounters::default();
        for (i, master) in masters.iter().enumerate() {
            let name = master.get_name();
            let j = old_index(&name);
            let module_type = master.module_type();
            if i != j {
                entries[j].set_sort_order(i as i32);
                if crate::interface::globals::complex_file_file_id() {
                    old.push(old_file_id(&name));
                    new.push(create_by_type(module_type, next.get(module_type)));
                    next.inc(module_type);
                } else {
                    old.push(FileID::create_full(j as i16));
                    new.push(FileID::create_full(i as i16));
                }
            } else {
                next.inc(module_type);
            }
        }
        *self.fl_masters.write().unwrap() = masters.clone();
        if !old.is_empty() {
            if let Some(container) = master_files.as_element_impl() {
                with_internal_edit(|| container.sort_by_sort_order_impl());
            }
            if game_mode() >= GameMode::gmTES4 {
                let update = MastersUpdate {
                    old,
                    new,
                    old_count: Self::slot_counts(&old_masters),
                    new_count: Self::slot_counts(&masters),
                };
                self.masters_updated(&update)?;
            }
            self.set_modified(true);
        }
        self.sort_records();
        Ok(())
    }

    /// Port of `TwbFile.MastersUpdated` over `TwbContainer.MastersUpdated`:
    /// every element of the file rewrites its FormIDs. Returns whether
    /// anything changed.
    pub fn masters_updated(self: &Arc<Self>, update: &MastersUpdate) -> Result<bool, EditError> {
        let result = with_update(&**self, || container_masters_updated(&**self, update));
        // The fixed FormIDs of the records depend on the master count.
        for record in self.records() {
            record.clear_fixed_form_id();
        }
        result
    }

    /// Port of `TwbFile.FindUsedMasters` (`TwbContainer.FindUsedMasters`):
    /// the masters the FormIDs of the file point to.
    pub fn find_used_masters(self: &Arc<Self>, masters: &mut UsedMasters) {
        container_find_used_masters(&**self, masters);
    }

    /// Whether master 0 is the game master or the hardcoded file, for the
    /// FormIDs of the hardcoded range.
    fn master_zero_is_game_master(&self) -> bool {
        self.fl_masters.read().unwrap().first().is_some_and(|master| {
            let states = master.get_file_states();
            states.contains(FileState::fsIsGameMaster) || states.contains(FileState::fsIsHardcoded)
        })
    }
}

/// Port of `CanContainFormIDs`: a main record, a group and the file always
/// can; the record header and the contained-in element never; any other
/// element when its definition has `dfCanContainFormID`.
fn can_contain_form_ids(element: &ElementRef) -> bool {
    let Some(element_impl) = element.as_element_impl() else {
        return false;
    };
    if element_impl.main_record_impl().is_some()
        || element_impl.group_record_impl().is_some()
        || element_impl.file_impl().is_some()
    {
        return true;
    }
    if let Some(value) = element_impl.value_impl()
        && value.vb.dont_save.load(std::sync::atomic::Ordering::Relaxed)
    {
        // `TwbRecordHeaderStruct` and `TwbContainedInElement`.
        return false;
    }
    element
        .get_def()
        .is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfCanContainFormID))
}

/// The children of a container that can hold FormIDs.
fn form_id_children(container: &dyn ElementImpl) -> Vec<ElementRef> {
    if let Some(container) = container.as_container() {
        container.get_element_count();
    }
    container
        .container_base()
        .map(|base| base.elements())
        .unwrap_or_default()
        .into_iter()
        .filter(can_contain_form_ids)
        .collect()
}

/// Port of the loop of `TwbContainer.MastersUpdated`.
fn container_masters_updated(container: &dyn ElementImpl, update: &MastersUpdate) -> Result<bool, EditError> {
    let mut result = false;
    for child in form_id_children(container) {
        result = element_masters_updated(&child, update)? || result;
    }
    Ok(result)
}

/// Port of `MastersUpdated` for any element of a file: the FormIDs in the
/// element and below it after the masters of the file changed. Returns
/// whether anything changed.
pub fn element_masters_updated(element: &ElementRef, update: &MastersUpdate) -> Result<bool, EditError> {
    let Some(element_impl) = element.as_element_impl() else {
        return Ok(false);
    };
    if let Some(record) = element_impl.main_record_impl() {
        return main_record_masters_updated(&record, update);
    }
    if let Some(group) = element_impl.group_record_impl() {
        return group_masters_updated(&group, update);
    }
    if let Some(file) = element_impl.file_impl() {
        return file.masters_updated(update);
    }
    // `TwbSubRecord` without a definition changes nothing.
    if let Some(sub_record) = element_impl.sub_record_impl()
        && sub_record.def().is_none()
    {
        return Ok(false);
    }
    with_update(element_impl, || {
        let mut result = container_masters_updated(element_impl, update)?;
        // `TwbSubRecord`, `TwbUnion` and `TwbValue` rewrite their own data
        // through the resolved definition; `TwbStruct`, `TwbArray` and the
        // subrecord containers only through their elements.
        let element_type = element.get_element_type();
        if matches!(
            element_type,
            ElementType::etSubRecord | ElementType::etUnion | ElementType::etValue
        ) && let Some(value_def) = element.get_value_def()
        {
            let old_value = (element_type == ElementType::etValue).then(|| element.get_native_value());
            let data = element_impl.current_data();
            let resolved = value::resolve(value_def, data, Some(element));
            if resolved.masters_updated(data, Some(element), update)? {
                result = true;
                element_impl.set_modified(true);
                if let Some(old_value) = old_value
                    && element
                        .get_def()
                        .is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfAfterSetOnIDUpdate))
                {
                    let new_value = element.get_native_value();
                    element_impl.do_after_set(&old_value, &new_value);
                }
            }
        }
        Ok(result)
    })
}

/// Port of `TwbMainRecord.MastersUpdated`: the FormID of the record and
/// every FormID in its elements, inside an internal edit. A record that
/// was not modified before and is now is modified internally. A record
/// that did not change releases its elements again.
fn main_record_masters_updated(record: &Arc<MainRecordImpl>, update: &MastersUpdate) -> Result<bool, EditError> {
    let base = &record.base;
    let was_modified = base.has_state(ElementState::esModified) && !base.has_state(ElementState::esInternalModified);
    let result = with_internal_edit(|| {
        with_update(&**record, || {
            record.do_init();
            let allow_hardcoded_range_use = record
                .file
                .upgrade()
                .is_some_and(|file| file.get_allow_hardcoded_range_use());
            let mut header_updated = false;
            let old = record.mr_struct().form_id;
            if !old.is_null() {
                let new = update.fixup(old, allow_hardcoded_range_use);
                if old != new {
                    record.make_header_writeable(|header| header.form_id = new);
                    record.clear_fixed_form_id();
                    header_updated = true;
                }
            }
            let result = container_masters_updated(&**record, update)?;
            Ok(result || header_updated)
        })
    });
    if !was_modified && base.has_state(ElementState::esModified) {
        // `CollapseStorage` is not ported.
        base.include_state(ElementState::esInternalModified);
    }
    // A record that did not change releases its elements, as upstream drops
    // its references; a modified record keeps them.
    record.reset();
    result
}

/// Port of `TwbGroupRecord.MastersUpdated`: the records of the group, then
/// the label of a group whose label is a FormID.
fn group_masters_updated(group: &Arc<GroupRecordImpl>, update: &MastersUpdate) -> Result<bool, EditError> {
    with_update(&**group, || {
        let mut result = container_masters_updated(&**group, update)?;
        // Upstream sorts the INFOs of a topic group (type 7) again here;
        // `TwbGroupRecord.Sort` of a topic group is not ported.
        let changed = with_internal_edit(|| {
            if !matches!(group.group_type(), 1 | 6..=10) {
                return false;
            }
            let old = crate::interface::form_id::FormID::from_cardinal(group.group_label());
            if old.is_null() {
                return false;
            }
            let allow_hardcoded_range_use = group
                .file
                .upgrade()
                .is_some_and(|file| file.get_allow_hardcoded_range_use());
            let new = update.fixup(old, allow_hardcoded_range_use);
            if group.group_label() == new.to_cardinal() {
                return false;
            }
            // `MakeHeaderWriteable`.
            group.set_modified(true);
            edit::invalidate_parent_storage(&**group);
            group.set_group_label(new.to_cardinal());
            true
        });
        if changed {
            result = true;
            group_container_changed(group);
        }
        Ok(result)
    })
}

/// Port of `TwbGroupRecord.ContainerChanged`: the contained-in elements of
/// the records of the group read the new label.
fn group_container_changed(group: &GroupRecordImpl) {
    let label = group.group_label().to_le_bytes().to_vec();
    for child in group.container.elements() {
        let Some(record) = child.as_element_impl().and_then(|child| child.main_record_impl()) else {
            continue;
        };
        for element in record.container.elements() {
            if element.get_sort_order() != -2 {
                continue;
            }
            if let Some(value) = element.as_element_impl().and_then(|element| element.value_impl()) {
                value.replace_data(label.clone());
            }
        }
    }
}

/// Port of the loop of `TwbContainer.FindUsedMasters`.
fn container_find_used_masters(container: &dyn ElementImpl, masters: &mut UsedMasters) {
    for child in form_id_children(container) {
        element_find_used_masters(&child, masters);
    }
}

/// Port of `FindUsedMasters` for any element of a file: flags the masters
/// the FormIDs in the element and below it point to.
pub fn element_find_used_masters(element: &ElementRef, masters: &mut UsedMasters) {
    let Some(element_impl) = element.as_element_impl() else {
        return;
    };
    if let Some(record) = element_impl.main_record_impl() {
        main_record_find_used_masters(&record, masters);
        return;
    }
    if let Some(group) = element_impl.group_record_impl() {
        group_find_used_masters(&group, masters);
        return;
    }
    if let Some(file) = element_impl.file_impl() {
        file.find_used_masters(masters);
        return;
    }
    if let Some(sub_record) = element_impl.sub_record_impl()
        && sub_record.def().is_none()
    {
        return;
    }
    container_find_used_masters(element_impl, masters);
    if matches!(
        element.get_element_type(),
        ElementType::etSubRecord | ElementType::etUnion | ElementType::etValue
    ) && let Some(value_def) = element.get_value_def()
    {
        let data = element_impl.current_data();
        let resolved = value::resolve(value_def, data, Some(element));
        resolved.find_used_masters(data, Some(element), masters);
    }
}

/// Port of `TwbMainRecord.FindUsedMasters` without the references: the
/// master of the FormID of the record, then the elements.
fn main_record_find_used_masters(record: &Arc<MainRecordImpl>, masters: &mut UsedMasters) {
    record.do_init();
    if let Some(file) = record.file.upgrade() {
        // `MarkMaster` of the fixed FormID.
        let mut form_id = record.get_fixed_form_id();
        if !form_id.is_null() {
            let allow_hardcoded_range_use = file.get_allow_hardcoded_range_use();
            if form_id.object_id() < 0x800 && !allow_hardcoded_range_use {
                form_id.set_file_id(FileID::null());
            }
            if form_id.is_hardcoded() {
                if file.master_zero_is_game_master() {
                    mark_used_master(masters, 0);
                }
            } else {
                let index = file.get_master_index_for_file_id(form_id.file_id());
                if index >= 0 {
                    mark_used_master(masters, index as usize);
                }
            }
        }
    }
    container_find_used_masters(&**record, masters);
    // The elements are released again, as upstream drops its references;
    // a modified record keeps them.
    record.reset();
}

/// Port of `TwbGroupRecord.FindUsedMasters`: the records, then the master
/// of the label of a group whose label is a FormID.
fn group_find_used_masters(group: &Arc<GroupRecordImpl>, masters: &mut UsedMasters) {
    container_find_used_masters(&**group, masters);
    if !matches!(group.group_type(), 1 | 6..=10) || group.group_label() == 0 {
        return;
    }
    let form_id = crate::interface::form_id::FormID::from_cardinal(group.group_label());
    let Some(file) = group.file.upgrade() else {
        return;
    };
    if form_id.object_id() < 0x800 {
        let master_zero_is_game_master = file.master_zero_is_game_master();
        if !file.get_allow_hardcoded_range_use() || form_id.is_hardcoded() {
            if master_zero_is_game_master {
                mark_used_master(masters, 0);
            }
            return;
        }
    }
    let index = file.get_master_index_for_file_id(form_id.file_id());
    if index >= 0 {
        mark_used_master(masters, index as usize);
    }
}
