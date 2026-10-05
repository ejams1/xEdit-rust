// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbFileID`, `TwbFormID`, `TwbSlotCounts` and the `TwbCRC32` helper.

use std::cmp::Ordering;
use std::fmt;

use super::globals::{is_light_supported, is_medium_supported, pretty_form_id, pseudo_light, pseudo_medium};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum ModuleType {
    mtFull,
    mtMedium,
    mtLight,
}

/// The load order slot of a file. A slot that is not used is -1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileID {
    light_slot: i16,
    medium_slot: i16,
    full_slot: i16,
}

impl FileID {
    pub fn create_full(full_slot: i16) -> Self {
        Self {
            full_slot,
            medium_slot: -1,
            light_slot: -1,
        }
    }

    pub fn create_medium(medium_slot: i16) -> Self {
        assert!(is_medium_supported() || pseudo_medium());
        Self {
            full_slot: Self::medium_full_slot(),
            medium_slot,
            light_slot: -1,
        }
    }

    pub fn create_light(light_slot: i16) -> Self {
        assert!(is_light_supported() || pseudo_light());
        Self {
            full_slot: Self::light_full_slot(),
            medium_slot: -1,
            light_slot,
        }
    }

    pub fn create_from_form_id(form_id: u32) -> Self {
        let full_slot = (form_id >> 24) as i16;
        let light_slot = if full_slot == Self::light_full_slot() && (pseudo_light() || is_light_supported()) {
            ((form_id >> 12) & 0xFFF) as i16
        } else {
            -1
        };
        let medium_slot = if full_slot == Self::medium_full_slot() && (pseudo_medium() || is_medium_supported()) {
            ((form_id >> 16) & 0xFF) as i16
        } else {
            -1
        };
        Self {
            light_slot,
            medium_slot,
            full_slot,
        }
    }

    pub fn null() -> Self {
        Self {
            light_slot: -1,
            medium_slot: -1,
            full_slot: 0,
        }
    }

    pub fn invalid() -> Self {
        Self {
            light_slot: -1,
            medium_slot: -1,
            full_slot: -1,
        }
    }

    pub fn max_full_slot() -> i16 {
        let mut result = 0xFE;
        if pseudo_light() || is_light_supported() {
            result -= 1; // $FD
            if pseudo_medium() || is_medium_supported() {
                result -= 1; // $FC
            }
        }
        result
    }

    pub fn max_medium_slot() -> i16 {
        if pseudo_medium() || is_medium_supported() {
            0xFF
        } else {
            -1
        }
    }

    pub fn max_light_slot() -> i16 {
        if pseudo_light() || is_light_supported() {
            0xFFF
        } else {
            -1
        }
    }

    pub fn light_full_slot() -> i16 {
        if pseudo_light() || is_light_supported() {
            0xFE
        } else {
            -1
        }
    }

    pub fn medium_full_slot() -> i16 {
        if pseudo_medium() || is_medium_supported() {
            0xFD
        } else {
            -1
        }
    }

    pub fn is_light_slot(self) -> bool {
        self.light_slot >= 0
    }

    pub fn is_medium_slot(self) -> bool {
        self.medium_slot >= 0
    }

    pub fn is_full_slot(self) -> bool {
        !self.is_light_slot() && !self.is_medium_slot() && self.full_slot >= 0
    }

    pub fn is_valid(self) -> bool {
        self.light_slot >= 0 || self.medium_slot >= 0 || self.full_slot >= 0
    }

    pub fn module_type(self) -> ModuleType {
        if self.light_slot >= 0 {
            ModuleType::mtLight
        } else if self.medium_slot >= 0 {
            ModuleType::mtMedium
        } else {
            ModuleType::mtFull
        }
    }

    pub fn base_form_id(self) -> u32 {
        if self.is_light_slot() {
            ((Self::light_full_slot() as u32) << 24) | ((self.light_slot as u32) << 12)
        } else if self.is_medium_slot() {
            ((Self::medium_full_slot() as u32) << 24) | ((self.medium_slot as u32) << 16)
        } else if self.is_full_slot() {
            (self.full_slot as u32) << 24
        } else {
            0xFFFF_FFFF
        }
    }

    pub fn full_slot(self) -> i16 {
        self.full_slot
    }

    pub fn medium_slot(self) -> i16 {
        self.medium_slot
    }

    pub fn light_slot(self) -> i16 {
        self.light_slot
    }
}

impl fmt::Display for FileID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.light_slot >= 0 {
            write!(f, "FE {:03X}", self.light_slot)
        } else if self.medium_slot >= 0 {
            write!(f, "FD {:02X}", self.medium_slot)
        } else if self.full_slot >= 0 {
            write!(f, "{:02X}", self.full_slot)
        } else {
            f.write_str("XX")
        }
    }
}

/// `ERangeError` of `TwbFormID.SetObjectID`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("ObjectID out of bounds")]
pub struct ObjectIDOutOfBounds;

/// A FormID. The file part is a full, medium or light slot, see [`FileID`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct FormID(u32);

impl FormID {
    pub const fn from_cardinal(value: u32) -> Self {
        Self(value)
    }

    /// Parses hexadecimal digits. Port of `TwbFormID.FromStr`, which converts
    /// with `StrToInt64('$' + aValue)` and keeps the low 32 bits.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        let value = if pretty_form_id() {
            value.replace(' ', "")
        } else {
            value.to_owned()
        };
        str_to_int64_hex(&value).map(|parsed| Self(parsed as u32))
    }

    pub fn from_str_def(value: &str, default: u32) -> Self {
        Self::from_str(value).unwrap_or(Self(default))
    }

    pub const fn null() -> Self {
        Self(0)
    }

    pub const fn none() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub fn compare(a: Self, b: Self) -> Ordering {
        a.0.cmp(&b.0)
    }

    pub const fn to_cardinal(self) -> u32 {
        self.0
    }

    pub fn file_id(self) -> FileID {
        FileID::create_from_form_id(self.0)
    }

    pub fn set_file_id(&mut self, value: FileID) {
        let object_id = self.object_id();
        self.0 = value.base_form_id();
        self.set_object_id_silent(object_id);
    }

    pub fn change_file_id(self, file_id: FileID) -> Self {
        let mut result = self;
        result.set_file_id(file_id);
        result
    }

    fn object_id_mask(self) -> u32 {
        let file_id = self.file_id();
        if file_id.is_light_slot() {
            0xFFF
        } else if file_id.is_medium_slot() {
            0xFFFF
        } else {
            0xFF_FFFF
        }
    }

    pub fn object_id(self) -> u32 {
        self.0 & self.object_id_mask()
    }

    /// Sets the object part. Fails when `value` does not fit the slot type.
    pub fn set_object_id(&mut self, value: u32) -> Result<(), ObjectIDOutOfBounds> {
        if value != (value & self.object_id_mask()) {
            return Err(ObjectIDOutOfBounds);
        }
        self.set_object_id_silent(value);
        Ok(())
    }

    /// Port of `SetObjectID(Value, True)`.
    // UPSTREAM-QUIRK: the silent form does not mask the value, so bits above the
    // object part change the file part.
    pub fn set_object_id_silent(&mut self, value: u32) {
        self.0 = (self.0 & !self.object_id_mask()) | value;
    }

    /// Port of the `Inc` operator: the next object ID, which is never below $800.
    pub fn inc(self) -> Self {
        let mask = self.object_id_mask();
        Self((self.0 & !mask) | (((self.0 & mask).wrapping_add(1)) & mask).max(2048))
    }

    /// Port of `A + B`. Fails when the object ID leaves its range.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, b: i64) -> Result<Self, ObjectIDOutOfBounds> {
        let mut result = self;
        result.set_object_id((i64::from(result.object_id()) + b) as u32)?;
        Ok(result)
    }

    /// Port of `A - B` with an integer.
    pub fn subtract(self, b: i64) -> Result<Self, ObjectIDOutOfBounds> {
        let mut result = self;
        result.set_object_id((i64::from(result.object_id()) - b) as u32)?;
        Ok(result)
    }

    /// Port of `A - B` with a FormID.
    pub fn distance(self, b: Self) -> i64 {
        i64::from(self.0) - i64::from(b.0)
    }

    pub fn is_null(self) -> bool {
        self.0 == 0
    }

    pub fn is_player(self) -> bool {
        self.0 == 0x0000_0014
    }

    pub fn is_none(self) -> bool {
        self.0 == 0xFFFF_FFFF
    }

    pub fn is_hardcoded(self) -> bool {
        self.0 < 0x800
    }

    /// Port of `TwbFormID.ToString`. `for_display` applies `wbPrettyFormID`.
    pub fn to_string(self, for_display: bool) -> String {
        let mut result = format!("{:08X}", self.0);
        if pretty_form_id() && for_display {
            result.insert(2, ' ');
            let file_id = self.file_id();
            if file_id.is_light_slot() {
                result.insert(6, ' ');
            } else if file_id.is_medium_slot() {
                result.insert(5, ' ');
            }
        }
        result
    }
}

/// Port of Delphi `StrToInt64('$' + value)` for the cases xEdit depends on:
/// one to 16 hexadecimal digits.
pub(crate) fn str_to_int64_hex(value: &str) -> Option<i64> {
    let digits = value;
    if digits.is_empty() || digits.len() > 16 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(digits, 16).ok().map(|parsed| parsed as i64)
}

/// Number of masters of each module type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotCounts {
    pub full: u8,
    pub light: i16,
    pub medium: u8,
}

impl SlotCounts {
    pub fn total(self) -> i16 {
        i16::from(self.full) + i16::from(self.medium) + self.light
    }
}

/// A CRC32 of a file. Port of `TwbCRC32` with `TwbCRC32Helper`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CRC32(pub u32);

impl CRC32 {
    /// Parses exactly eight hexadecimal digits.
    pub fn assign_from_string(s: &str) -> Option<Self> {
        if s.chars().count() != 8 {
            return None;
        }
        str_to_int64_hex(s).map(|parsed| Self(parsed as u32))
    }

    pub fn is_null(self) -> bool {
        self.0 == 0
    }

    pub fn is_none(self) -> bool {
        self.0 == 0xFFFF_FFFF
    }

    pub fn is_valid(self) -> bool {
        !(self.is_null() || self.is_none())
    }
}

impl fmt::Display for CRC32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08X}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::super::globals::{GameMode, set_game_mode, set_pretty_form_id, test_lock};
    use super::*;

    #[test]
    fn full_slots_without_light_support() {
        let _guard = test_lock();
        set_game_mode(GameMode::gmTES5);
        let form_id = FormID::from_cardinal(0xFE00_1ABC);
        assert!(form_id.file_id().is_full_slot());
        assert_eq!(form_id.file_id().full_slot(), 0xFE);
        assert_eq!(form_id.object_id(), 0x00_1ABC);
        assert_eq!(FileID::max_full_slot(), 0xFE);
        assert_eq!(FileID::light_full_slot(), -1);
        assert_eq!(form_id.file_id().to_string(), "FE");
    }

    #[test]
    fn light_slots() {
        let _guard = test_lock();
        set_game_mode(GameMode::gmFO4);
        let form_id = FormID::from_cardinal(0xFE00_1ABC);
        let file_id = form_id.file_id();
        assert!(file_id.is_light_slot() && !file_id.is_full_slot());
        assert_eq!(file_id.light_slot(), 1);
        assert_eq!(file_id.module_type(), ModuleType::mtLight);
        assert_eq!(form_id.object_id(), 0xABC);
        assert_eq!(file_id.base_form_id(), 0xFE00_1000);
        assert_eq!(file_id.to_string(), "FE 001");
        assert_eq!(FileID::max_full_slot(), 0xFD);
        assert_eq!(FileID::create_light(1), file_id);
    }

    #[test]
    fn medium_slots() {
        let _guard = test_lock();
        set_game_mode(GameMode::gmSF1);
        let form_id = FormID::from_cardinal(0xFD02_1234);
        let file_id = form_id.file_id();
        assert!(file_id.is_medium_slot());
        assert_eq!(file_id.medium_slot(), 2);
        assert_eq!(form_id.object_id(), 0x1234);
        assert_eq!(file_id.to_string(), "FD 02");
        assert_eq!(FileID::max_full_slot(), 0xFC);
    }

    #[test]
    fn file_id_change_keeps_object_id() {
        let _guard = test_lock();
        set_game_mode(GameMode::gmFO4);
        let form_id = FormID::from_cardinal(0x0100_0801);
        assert_eq!(
            form_id.change_file_id(FileID::create_full(5)).to_cardinal(),
            0x0500_0801
        );
        assert_eq!(
            form_id.change_file_id(FileID::create_light(3)).to_cardinal(),
            0xFE00_3801
        );
        assert_eq!(FormID::none().file_id().base_form_id(), 0xFF00_0000);
        assert_eq!(FileID::invalid().base_form_id(), 0xFFFF_FFFF);
    }

    #[test]
    fn object_id_arithmetic() {
        let _guard = test_lock();
        set_game_mode(GameMode::gmFO4);
        let mut form_id = FormID::from_cardinal(0xFE00_1FFF);
        assert_eq!(form_id.inc().to_cardinal(), 0xFE00_1800);
        assert_eq!(form_id.set_object_id(0x1000), Err(ObjectIDOutOfBounds));
        assert_eq!(form_id.subtract(0xFFF).unwrap().to_cardinal(), 0xFE00_1000);
        assert_eq!(form_id.add(1), Err(ObjectIDOutOfBounds));
        form_id.set_object_id(1).unwrap();
        assert_eq!(form_id.inc().to_cardinal(), 0xFE00_1800);
        assert_eq!(FormID::from_cardinal(0x0100_0801).inc().to_cardinal(), 0x0100_0802);
        assert_eq!(FormID::from_cardinal(5).distance(FormID::from_cardinal(7)), -2);
    }

    #[test]
    fn text_forms() {
        let _guard = test_lock();
        set_game_mode(GameMode::gmFO4);
        assert_eq!(FormID::from_str("0001FAB4"), Some(FormID::from_cardinal(0x1FAB4)));
        assert_eq!(FormID::from_str("xyz"), None);
        assert_eq!(FormID::from_str_def("", 7).to_cardinal(), 7);
        assert_eq!(FormID::from_cardinal(0xFE00_1ABC).to_string(true), "FE001ABC");
        set_pretty_form_id(true);
        assert_eq!(FormID::from_cardinal(0xFE00_1ABC).to_string(true), "FE 001 ABC");
        assert_eq!(FormID::from_cardinal(0x0100_0801).to_string(true), "01 000801");
        assert_eq!(FormID::from_cardinal(0x0100_0801).to_string(false), "01000801");
        assert_eq!(FormID::from_str("01 000801"), Some(FormID::from_cardinal(0x0100_0801)));
    }

    #[test]
    fn predicates() {
        assert!(FormID::null().is_null() && FormID::none().is_none());
        assert!(FormID::from_cardinal(0x14).is_player() && FormID::from_cardinal(0x7FF).is_hardcoded());
        assert!(!FormID::from_cardinal(0x800).is_hardcoded());
    }

    #[test]
    fn crc32_helper() {
        assert_eq!(CRC32::assign_from_string("F88D2046"), Some(CRC32(0xF88D_2046)));
        assert_eq!(CRC32::assign_from_string("F88D204"), None);
        assert_eq!(CRC32(0xF88D_2046).to_string(), "F88D2046");
        assert!(CRC32(1).is_valid() && !CRC32(0).is_valid() && !CRC32(0xFFFF_FFFF).is_valid());
    }
}
