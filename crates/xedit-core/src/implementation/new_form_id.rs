// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of `TwbFile.NewFormID` with `GetNextObjectID`, `SetNextObjectID`
//! and `GetHighObjectID`: the FormID a new record of a file takes, from the
//! `Next Object ID` of the file header.
//!
//! State: without the complex FileIDs of Starfield (`wbComplexFileFileID`).

use std::sync::Arc;

use crate::interface::element::{Container, Element, File};
use crate::interface::form_id::FormID;
use crate::interface::globals::{GameMode, game_mode, header_signature, hedr_next_object_id};
use crate::interface::misc::{EditError, Variant};
use crate::interface::types::ElementType;

use super::FileImpl;

impl FileImpl {
    /// Port of `GetNextObjectID`: `HEDR\Next Object ID` of the file header.
    pub fn get_next_object_id(&self) -> u32 {
        if game_mode() >= GameMode::gmTES4
            && let Some(header) = self.header()
        {
            return header
                .get_element_native_value(r"HEDR\Next Object ID")
                .as_ordinal()
                .unwrap_or(0) as u32;
        }
        hedr_next_object_id() as u32
    }

    /// Port of `SetNextObjectID`.
    pub fn set_next_object_id(&self, object_id: u32) -> Result<(), EditError> {
        if game_mode() >= GameMode::gmTES4
            && let Some(header) = self.header()
        {
            header.set_element_native_value(r"HEDR\Next Object ID", Variant::UInt(u64::from(object_id)))?;
        }
        Ok(())
    }

    /// Port of `GetHighObjectID`: the object ID of the record of the file
    /// with the highest FormID, when it is a new record of the file.
    pub fn get_high_object_id(&self) -> u32 {
        let mut result = if self.get_allow_hardcoded_range_use() { 1 } else { 0x800 };
        if let Some(last) = self.fl_records.read().unwrap().last() {
            let form_id = last.get_fixed_form_id();
            if self.is_new_record(form_id.file_id()) {
                result = form_id.object_id();
            }
        }
        result
    }

    /// Port of `TwbFile.NewFormID`: the next free FormID of the file from
    /// its `Next Object ID`, which moves past it.
    /// UPSTREAM-QUIRK: the next object ID moves one past the FormID only
    /// when the file has records already.
    pub fn new_form_id(self: &Arc<Self>) -> Result<FormID, EditError> {
        let name = self.get_name();
        let header = self.header().ok_or_else(|| format!("File {name} has no file header"))?;
        if header.get_element_type() != ElementType::etMainRecord {
            return Err(format!(
                "File {name} has invalid record {} as file header.",
                header.get_name()
            ));
        }
        let flags = header.mr_struct().flags;
        if flags.is_update() {
            return Err(format!("File {name} is an update and can not contain new records."));
        }
        if header.get_signature() != header_signature() {
            return Err(format!(
                "File {name} has invalid record {} with invalid signature as file header.",
                header.get_name()
            ));
        }
        if header
            .get_record_by_signature(crate::interface::types::Signature::new(b"HEDR"))
            .is_none()
        {
            return Err(format!("File {name} has a file header with missing HEDR subrecord"));
        }
        let load_order_file_id = self.get_load_order_file_id();
        let mask: u32 = if flags.is_light() || load_order_file_id.is_light_slot() {
            0xFFF
        } else if flags.is_medium() || load_order_file_id.is_medium_slot() {
            0xFFFF
        } else {
            0xFF_FFFF
        };
        let hardcoded_range = self.get_allow_hardcoded_range_use();
        let lowest = if hardcoded_range { 1 } else { 0x800 };
        let mut next = self.get_next_object_id() & mask;
        if next < lowest || next == mask {
            next = self.get_high_object_id();
            if next > mask {
                next = lowest;
            }
        }
        let file_id = self.get_file_file_id();
        let mut result = FormID::from_cardinal(next).change_file_id(file_id);
        let first = result;
        while self.record_by_form_id(result, true, true).is_some() {
            next += 1;
            if next > mask {
                next = lowest;
            }
            result = FormID::from_cardinal(next).change_file_id(file_id);
            if result == first {
                return Err(format!("File {name} has no more space for a new FormID"));
            }
        }
        if self.get_record_count() > 0 {
            next += 1;
        }
        if next > mask {
            next = lowest;
        }
        self.set_next_object_id(next)?;
        Ok(result)
    }
}
