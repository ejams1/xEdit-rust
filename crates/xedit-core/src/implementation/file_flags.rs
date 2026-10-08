// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of the module flags of `TwbFile`: `GetIsNotPlugin`, the `GetIs...`
//! and `SetIs...` pairs of the ESM, light (ESL), medium, update (overlay), blueprint and
//! localized flags of the file header, and the light, medium and update
//! compatibility states that `AddMainRecord` keeps for the new records of
//! the file.

use crate::interface::element::{Element, File};
use crate::interface::form_id::FormID;
use crate::interface::globals::{
    is_blueprint_supported, is_light_supported, is_medium_supported, is_update_supported, pseudo_light, pseudo_medium,
    pseudo_update,
};
use crate::interface::misc::progress;
use crate::interface::types::FileState;

use super::{FileImpl, MainRecordImpl, is_module};

/// The flags of a module file header that `SetIs...` changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleFlag {
    Esm,
    Light,
    Medium,
    Update,
    Blueprint,
    Localized,
}

impl FileImpl {
    /// Port of `GetIsNotPlugin`: the file is not a module by its extension.
    pub fn get_is_not_plugin(&self) -> bool {
        !is_module(self.file_name())
    }

    /// The file header, or upstream's `Unexpected error reading file`.
    fn module_header(&self) -> Result<std::sync::Arc<MainRecordImpl>, String> {
        self.header()
            .ok_or_else(|| format!("Unexpected error reading file \"{}\"", self.file_name()))
    }

    /// Port of `TwbFile.GetIsLight`.
    pub fn get_is_light(&self) -> bool {
        if pseudo_light() {
            return self.get_file_states().contains(FileState::fsPseudoLight);
        }
        if !is_light_supported() || self.get_is_not_plugin() {
            return false;
        }
        self.header().is_some_and(|header| header.mr_struct().flags.is_light())
    }

    /// Port of `TwbFile.GetIsMedium`.
    pub fn get_is_medium(&self) -> bool {
        if pseudo_medium() {
            return self.get_file_states().contains(FileState::fsPseudoMedium);
        }
        if !is_medium_supported() || self.get_is_not_plugin() {
            return false;
        }
        self.header().is_some_and(|header| header.mr_struct().flags.is_medium())
    }

    /// Port of `TwbFile.GetIsUpdate`.
    pub fn get_is_update(&self) -> bool {
        if pseudo_update() {
            return self.get_file_states().contains(FileState::fsPseudoUpdate);
        }
        if !is_update_supported() || self.get_is_not_plugin() {
            return false;
        }
        self.header().is_some_and(|header| header.mr_struct().flags.is_update())
    }

    /// Port of `TwbFile.GetIsBlueprint`.
    pub fn get_is_blueprint(&self) -> bool {
        if !is_blueprint_supported() || self.get_is_not_plugin() {
            return false;
        }
        self.header()
            .is_some_and(|header| header.mr_struct().flags.is_blueprint())
    }

    /// Whether the game has the flag at all (`wbIsLightSupported` and so on).
    pub fn module_flag_supported(flag: ModuleFlag) -> bool {
        match flag {
            ModuleFlag::Esm | ModuleFlag::Localized => true,
            ModuleFlag::Light => is_light_supported(),
            ModuleFlag::Medium => is_medium_supported(),
            ModuleFlag::Update => is_update_supported(),
            ModuleFlag::Blueprint => is_blueprint_supported(),
        }
    }

    /// The flag as the file header has it (`Header.IsESM` and so on).
    pub fn module_flag(&self, flag: ModuleFlag) -> bool {
        let Some(header) = self.header() else { return false };
        let flags = header.mr_struct().flags;
        match flag {
            ModuleFlag::Esm => flags.is_esm(),
            ModuleFlag::Light => flags.is_light(),
            ModuleFlag::Medium => flags.is_medium(),
            ModuleFlag::Update => flags.is_update(),
            ModuleFlag::Blueprint => flags.is_blueprint(),
            ModuleFlag::Localized => flags.is_localized(),
        }
    }

    /// Port of `TwbFile.SetIsESM`, `SetIsLight`, `SetIsMedium`,
    /// `SetIsUpdate`, `SetIsBlueprint` and `SetIsLocalized`: nothing happens
    /// for a file that is not a module or a flag the game does not have; a
    /// change of a file that is not editable fails.
    pub fn set_module_flag(&self, flag: ModuleFlag, value: bool) -> Result<(), String> {
        if !Self::module_flag_supported(flag) || self.get_is_not_plugin() {
            return Ok(());
        }
        let header = self.module_header()?;
        if value == self.module_flag(flag) {
            return Ok(());
        }
        if !self.is_element_editable() {
            return Err(format!("File \"{}\" is not editable", self.file_name()));
        }
        match flag {
            ModuleFlag::Esm => header.set_is_esm(value),
            ModuleFlag::Light => header.set_is_light(value),
            ModuleFlag::Medium => header.set_is_medium(value),
            ModuleFlag::Update => header.set_is_update(value),
            ModuleFlag::Blueprint => header.set_is_blueprint(value),
            ModuleFlag::Localized => header.set_is_localized(value),
        }
        Ok(())
    }

    /// Port of the compatibility checks of `TwbFile.AddMainRecord` for a
    /// new record of the file: an object ID above `$FFF` (above `$FFFF`)
    /// makes the file unfit for the light (medium) flag, and any new record
    /// makes it unfit for the update flag. The error is reported when the
    /// file has the flag or its slot.
    pub(crate) fn check_new_record_compatibility(&self, record: &MainRecordImpl, form_id: FormID) {
        let cardinal = form_id.to_cardinal();
        let object_id = cardinal & 0x00FF_FFFF;
        if !crate::interface::globals::complex_file_file_id() {
            if cardinal & 0x00FF_F000 != 0 {
                self.fl_states.write().unwrap().exclude(FileState::fsLightCompatible);
                if self.get_is_light() || self.get_load_order_file_id().is_light_slot() {
                    progress(&format!(
                        "<Error: {} has invalid ObjectID {object_id:06X} for a light module. You will not be able to save this file with the Light flag active.>",
                        record.get_name()
                    ));
                }
            }
            if cardinal & 0x00FF_0000 != 0 {
                self.fl_states.write().unwrap().exclude(FileState::fsMediumCompatible);
                if self.get_is_medium() || self.get_load_order_file_id().is_medium_slot() {
                    progress(&format!(
                        "<Error: {} has invalid ObjectID {object_id:06X} for a medium module. You will not be able to save this file with the Medium flag active.>",
                        record.get_name()
                    ));
                }
            }
        }
        self.fl_states.write().unwrap().exclude(FileState::fsUpdateCompatible);
        if self.get_is_update() {
            progress(&format!(
                "<Error: {} has invalid ObjectID {object_id:06X} for an update module. You will not be able to save this file with the Update flag active.>",
                record.get_name()
            ));
        }
    }
}
