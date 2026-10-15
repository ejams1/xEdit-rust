// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of `wbNewFile` and both `TwbFile.CreateNew`: an empty plugin that
//! exists only in memory until it is saved, with a file header made from
//! the definitions, flagged light or medium (`aIsLight`, `aIsMedium`) or as
//! a module template of "Create New File" says (`aTemplate`, the template
//! modules of `wbLoadModules`, see [`ModuleTemplate`]).
//!
//! State: the module information of upstream (`flModule`, `wbModuleByName`,
//! `TwbModuleInfo.AddNewModule`) does not exist in the port; the slot of
//! the file is decided as `CreateNew` decides it. `BuildOrLoadRef` of the
//! new file builds nothing, as it holds no record yet.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

use crate::interface::element::Container;
use crate::interface::form_id::FileID;
use crate::interface::globals::{
    GameMode, game_mode, hedr_next_object_id, hedr_version, ignore_light, ignore_medium, ignore_update,
    is_light_supported, is_medium_supported, is_starfield, is_update_supported, pseudo_light, pseudo_medium,
    pseudo_update,
};
use crate::interface::misc::{EditError, Variant};
use crate::interface::types::{FileState, FileStates, Signature};

use super::{
    ContainerBase, ElementBase, FILES_MAP, FileBytes, FileImpl, MainRecordImpl, NEXT_LOAD_ORDER, find_in_files_map,
    next_full_slot, next_light_slot, next_medium_slot,
};

/// Port of `wbNewFile(aFileName, aLoadOrder, aIsLight, aIsMedium)`: a new
/// empty file under the name, which must not be loaded yet.
pub fn wb_new_file(file_name: &str, load_order: i32, light: bool, medium: bool) -> Result<Arc<FileImpl>, EditError> {
    if light && medium {
        return Err("Assertion failed: not (aIsLight and aIsMedium)".to_owned());
    }
    if find_in_files_map(file_name).is_some() {
        return Err(format!("{file_name} exists already"));
    }
    let file = FileImpl::create_new(file_name, load_order, light, medium)?;
    crate::interface::element::add_file(file.clone());
    Ok(file)
}

/// A module template of "Create New File" (`TwbModuleInfo.AddNewModule`
/// with `aTemplate`, `wbLoadOrder.pas:561-619`): the extension of the file
/// and the flags of its header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModuleTemplate {
    /// `meESM` (`<new file>.esm`), else `.esp`.
    pub esm_extension: bool,
    /// `mfHasESMFlag`.
    pub esm: bool,
    /// `mfHasLightFlag`.
    pub light: bool,
    /// `mfHasMediumFlag`.
    pub medium: bool,
    /// `mfHasUpdateFlag`.
    pub update: bool,
}

impl ModuleTemplate {
    /// The extension of a file made from the template
    /// (`aTemplate.miExtension.ToString`).
    pub fn extension(&self) -> &'static str {
        if self.esm_extension { ".esm" } else { ".esp" }
    }
}

/// The template modules of the game, in the order `wbLoadModules` adds
/// them (the order "Create New File" lists them).
pub fn module_templates() -> Vec<ModuleTemplate> {
    let esp = |esm: bool, light: bool, medium: bool, update: bool| ModuleTemplate {
        esm_extension: false,
        esm,
        light,
        medium,
        update,
    };
    let esm = |light: bool, medium: bool, update: bool| ModuleTemplate {
        esm_extension: true,
        esm: true,
        light,
        medium,
        update,
    };
    let starfield = is_starfield();
    let mut templates = vec![esp(false, false, false, false)];
    if !starfield {
        templates.push(esp(true, false, false, false));
    }
    if is_light_supported() && !starfield {
        templates.push(esp(false, true, false, false));
        templates.push(esp(true, true, false, false));
    }
    if is_medium_supported() && !starfield {
        templates.push(esp(false, false, true, false));
        templates.push(esp(true, false, true, false));
    }
    if is_update_supported() && !starfield {
        templates.push(esp(false, false, false, true));
        templates.push(esp(true, false, false, true));
    }
    templates.push(esm(false, false, false));
    if is_light_supported() {
        templates.push(esm(true, false, false));
    }
    if is_medium_supported() {
        templates.push(esm(false, true, false));
    }
    if is_update_supported() && !starfield {
        templates.push(esm(false, false, true));
    }
    templates
}

/// Port of `wbNewFile(aFileName, aLoadOrder, aTemplate)`.
pub fn wb_new_file_from_template(
    file_name: &str,
    load_order: i32,
    template: &ModuleTemplate,
) -> Result<Arc<FileImpl>, EditError> {
    if find_in_files_map(file_name).is_some() {
        return Err(format!("{file_name} exists already"));
    }
    let file = FileImpl::create_new_with(file_name, load_order, |flags| {
        // `TwbFile.CreateNew(..., aTemplate)`: the update, light and medium
        // flags where the game has them, then the ESM flag. The template
        // modules carry no blueprint flag and no masters.
        if template.update && is_update_supported() {
            flags.set_update(true);
        }
        if template.light && is_light_supported() {
            flags.set_light(true);
        }
        if template.medium && is_medium_supported() {
            flags.set_medium(true);
        }
        if template.esm {
            flags.set_esm(true);
        }
    })?;
    crate::interface::element::add_file(file.clone());
    Ok(file)
}

impl FileImpl {
    /// Port of `TwbFile.CreateNew(aFileName, aLoadOrder, aIsLight,
    /// aIsMedium)`.
    fn create_new(file_name: &str, load_order: i32, light: bool, medium: bool) -> Result<Arc<FileImpl>, EditError> {
        Self::create_new_with(file_name, load_order, |flags| {
            if light {
                flags.set_light(true);
            }
            if medium {
                flags.set_medium(true);
            }
        })
    }

    /// The body both `CreateNew` share; `set_flags` sets the flags of the
    /// file header.
    fn create_new_with(
        file_name: &str,
        load_order: i32,
        set_flags: impl FnOnce(&mut super::structs::MainRecordStructFlags),
    ) -> Result<Arc<FileImpl>, EditError> {
        let mut states = FileStates::empty();
        for state in [
            FileState::fsIsNew,
            FileState::fsLightCompatible,
            FileState::fsMediumCompatible,
            FileState::fsUpdateCompatible,
        ] {
            states.include(state);
        }
        let file = Arc::new_cyclic(|self_ref: &Weak<FileImpl>| FileImpl {
            self_ref: self_ref.clone(),
            base: ElementBase::new(None),
            container: ContainerBase::default(),
            fl_file_name: file_name.to_owned(),
            fl_load_order: AtomicI32::new(load_order),
            fl_load_order_file_id: RwLock::new(FileID::invalid()),
            fl_states: RwLock::new(states),
            fl_bytes: Arc::new(FileBytes::Owned(Vec::new())),
            fl_records: RwLock::new(Vec::new()),
            fl_form_ids_sorted: AtomicBool::new(false),
            fl_masters: RwLock::new(Vec::new()),
            fl_load_finished: OnceLock::new(),
            fl_version: OnceLock::new(),
            fl_compare_to: None,
            fl_injected_records: RwLock::new(Vec::new()),
            fl_records_indices: RwLock::new(Vec::new()),
            fl_indices_active: AtomicBool::new(false),
            fl_scanned_form_ids: Mutex::new(std::collections::HashSet::new()),
            fl_crc32: AtomicU32::new(0),
        });
        // `wbNewFile` adds the file to `FilesMap` after the constructor; the
        // header's edits below look the file up there.
        FILES_MAP.write().unwrap().push(file.clone());

        let header = MainRecordImpl::create_header(&file)?;
        let hedr = header
            .get_record_by_signature(Signature::new(b"HEDR"))
            .ok_or_else(|| "the file header has no HEDR".to_owned())?;
        let hedr = hedr
            .as_container()
            .map(|hedr| (hedr.get_element(0), hedr.get_element(2)))
            .ok_or_else(|| "the HEDR of the file header has no elements".to_owned())?;
        if let Some(version) = hedr.0 {
            version.set_native_value(Variant::Float(hedr_version()))?;
        }
        if game_mode() >= GameMode::gmTES4
            && let Some(next_object_id) = hedr.1
        {
            next_object_id.set_native_value(Variant::Int(i64::from(hedr_next_object_id())))?;
        }
        set_flags(&mut header.mr_struct.write().unwrap().flags);

        file.fl_load_finished.set(()).ok();
        file.fl_form_ids_sorted.store(true, Ordering::Relaxed);
        file.fl_indices_active.store(true, Ordering::Release);

        if load_order >= 0 {
            NEXT_LOAD_ORDER.fetch_max(load_order + 1, Ordering::Relaxed);
            let flags = header.mr_struct().flags;
            let file_id = if is_light_supported()
                || pseudo_light()
                || is_medium_supported()
                || pseudo_medium()
                || pseudo_update()
            {
                if flags.is_light() && !ignore_light() {
                    FileID::create_light(next_light_slot())
                } else if flags.is_medium() && !ignore_medium() {
                    FileID::create_medium(next_medium_slot())
                } else if (is_update_supported() || pseudo_update()) && flags.is_update() && !ignore_update() {
                    FileID::invalid()
                } else {
                    FileID::create_full(next_full_slot())
                }
            } else {
                FileID::create_full(load_order as i16)
            };
            *file.fl_load_order_file_id.write().unwrap() = file_id;
        }

        if is_starfield() {
            file.add_masters(&["Starfield.esm"], false)?;
        }
        Ok(file)
    }
}
