// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! The binary headers of records: `TwbMainRecordStruct`,
//! `TwbMainRecordStructFlags`, `TwbGroupRecordStruct` and
//! `TwbSubRecordHeaderStruct`.

use crate::interface::form_id::FormID;
use crate::interface::globals::{
    GameMode, game_mode, is_blueprint_supported, is_light_supported, is_medium_supported, is_starfield,
    is_update_supported, size_of_main_record_struct, vresl,
};
use crate::interface::types::Signature;

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

fn signature_at(bytes: &[u8], offset: usize) -> Option<Signature> {
    let bytes: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(Signature::new(&bytes))
}

/// Upstream `TwbMainRecordStructFlags`: the record flags of the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MainRecordStructFlags(pub u32);

impl MainRecordStructFlags {
    pub fn is_esm(self) -> bool {
        self.0 & 0x0000_0001 != 0
    }

    pub fn is_deleted(self) -> bool {
        self.0 & 0x0000_0020 != 0
    }

    pub fn is_localized(self) -> bool {
        self.0 & 0x0000_0080 != 0
    }

    pub fn is_persistent(self) -> bool {
        self.0 & 0x0000_0400 != 0
    }

    pub fn is_compressed(self) -> bool {
        self.0 & 0x0004_0000 != 0
    }

    /// The light flag of the file header, `ESL`.
    pub fn is_light(self) -> bool {
        let bit = if is_starfield() { 0x0000_0100 } else { 0x0000_0200 };
        is_light_supported() && self.0 & bit != 0
    }

    /// The medium flag of the file header, Starfield.
    pub fn is_medium(self) -> bool {
        is_medium_supported() && self.0 & 0x0000_0400 != 0
    }

    /// The update flag of the file header, Starfield.
    pub fn is_update(self) -> bool {
        is_update_supported()
            && ((is_starfield() && self.0 & 0x0000_0200 != 0) || (vresl() && self.0 & 0x0010_0000 != 0))
    }

    pub fn is_visible_when_distant(self) -> bool {
        self.0 & 0x0000_8000 != 0
    }

    /// The blueprint flag of the file header, Starfield.
    pub fn is_blueprint(self) -> bool {
        is_blueprint_supported() && self.0 & 0x0000_0800 != 0
    }

    pub fn is_partial_form(self) -> bool {
        self.0 & 0x0000_4000 != 0
    }

    pub fn is_ignored(self) -> bool {
        self.0 & 0x0000_1000 != 0
    }

    fn set_bit(&mut self, bit: u32, value: bool) {
        if value {
            self.0 |= bit;
        } else {
            self.0 &= !bit;
        }
    }

    pub fn set_esm(&mut self, value: bool) {
        self.set_bit(0x0000_0001, value);
    }

    pub fn set_deleted(&mut self, value: bool) {
        self.set_bit(0x0000_0020, value);
    }

    pub fn set_persistent(&mut self, value: bool) {
        self.set_bit(0x0000_0400, value);
    }

    pub fn set_compressed(&mut self, value: bool) {
        self.set_bit(0x0004_0000, value);
    }

    pub fn set_visible_when_distant(&mut self, value: bool) {
        self.set_bit(0x0000_8000, value);
    }

    pub fn set_partial_form(&mut self, value: bool) {
        self.set_bit(0x0000_4000, value);
    }

    /// Port of `SetLight`: nothing happens where the game has no light flag.
    pub fn set_light(&mut self, value: bool) {
        if is_light_supported() {
            let bit = if is_starfield() { 0x0000_0100 } else { 0x0000_0200 };
            self.set_bit(bit, value);
        }
    }

    pub fn set_medium(&mut self, value: bool) {
        if is_medium_supported() {
            self.set_bit(0x0000_0400, value);
        }
    }

    pub fn set_update(&mut self, value: bool) {
        if is_update_supported() {
            if is_starfield() {
                self.set_bit(0x0000_0200, value);
            } else if vresl() {
                self.set_bit(0x0010_0000, value);
            }
        }
    }
}

/// Upstream `TwbMainRecordStruct`: the header of a main record. The TES4
/// layout has no version and second version control field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainRecordStruct {
    pub signature: Signature,
    pub data_size: u32,
    pub flags: MainRecordStructFlags,
    pub form_id: FormID,
    pub vcs1: u32,
    pub version: u16,
    pub vcs2: u16,
}

impl MainRecordStruct {
    /// The header at `offset`, or `None` when the bytes end before it.
    pub fn parse(bytes: &[u8], offset: usize) -> Option<Self> {
        let size = size_of_main_record_struct() as usize;
        bytes.get(offset..offset + size)?;
        let (version, vcs2) = if size >= 24 {
            (u16_at(bytes, offset + 20)?, u16_at(bytes, offset + 22)?)
        } else {
            (0, 0)
        };
        let (flags, form_id, vcs1) = if game_mode() == GameMode::gmTES3 {
            (
                MainRecordStructFlags(u32_at(bytes, offset + 12)?),
                FormID::null(),
                u32_at(bytes, offset + 8)?,
            )
        } else {
            (
                MainRecordStructFlags(u32_at(bytes, offset + 8)?),
                FormID::from_cardinal(u32_at(bytes, offset + 12)?),
                u32_at(bytes, offset + 16)?,
            )
        };
        Some(MainRecordStruct {
            signature: signature_at(bytes, offset)?,
            data_size: u32_at(bytes, offset + 4)?,
            flags,
            form_id,
            vcs1,
            version,
            vcs2,
        })
    }

    /// The header as the bytes `WriteToStream` writes: `wbSizeOfMainRecordStruct`
    /// bytes in the layout `parse` reads.
    pub fn to_bytes(&self) -> Vec<u8> {
        let size = size_of_main_record_struct() as usize;
        let mut bytes = Vec::with_capacity(size);
        bytes.extend_from_slice(&self.signature.0);
        bytes.extend_from_slice(&self.data_size.to_le_bytes());
        if game_mode() == GameMode::gmTES3 {
            bytes.extend_from_slice(&self.vcs1.to_le_bytes());
            bytes.extend_from_slice(&self.flags.0.to_le_bytes());
        } else {
            bytes.extend_from_slice(&self.flags.0.to_le_bytes());
            bytes.extend_from_slice(&self.form_id.to_cardinal().to_le_bytes());
            bytes.extend_from_slice(&self.vcs1.to_le_bytes());
        }
        if size >= 24 {
            bytes.extend_from_slice(&self.version.to_le_bytes());
            bytes.extend_from_slice(&self.vcs2.to_le_bytes());
        }
        bytes.resize(size, 0);
        bytes
    }
}

/// Upstream `TwbGroupRecordStruct`: the header of a group record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupRecordStruct {
    pub group_size: u32,
    pub label: u32,
    pub group_type: i32,
    pub stamp: u32,
    pub unknown: u32,
}

impl GroupRecordStruct {
    /// The header at `offset`, which starts with `GRUP`.
    pub fn parse(bytes: &[u8], offset: usize) -> Option<Self> {
        bytes.get(offset..offset + 24)?;
        Some(GroupRecordStruct {
            group_size: u32_at(bytes, offset + 4)?,
            label: u32_at(bytes, offset + 8)?,
            group_type: u32_at(bytes, offset + 12)? as i32,
            stamp: u32_at(bytes, offset + 16)?,
            unknown: u32_at(bytes, offset + 20)?,
        })
    }

    /// The header as the bytes `WriteToStream` writes: `GRUP` and the
    /// fields that fit `wbSizeOfMainRecordStruct` (20 bytes for Oblivion,
    /// without the last field).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(24);
        bytes.extend_from_slice(b"GRUP");
        bytes.extend_from_slice(&self.group_size.to_le_bytes());
        bytes.extend_from_slice(&self.label.to_le_bytes());
        bytes.extend_from_slice(&(self.group_type as u32).to_le_bytes());
        bytes.extend_from_slice(&self.stamp.to_le_bytes());
        bytes.extend_from_slice(&self.unknown.to_le_bytes());
        bytes.truncate(size_of_main_record_struct() as usize);
        bytes
    }

    /// The label as a signature, which it is for a top level group.
    pub fn label_signature(self) -> Signature {
        Signature::new(&self.label.to_le_bytes())
    }

    /// Port of `TwbGroupRecord.GetName` for the error messages of the scan.
    pub fn name(self) -> String {
        match self.group_type {
            0 => format!("GRUP Top \"{}\"", self.label_signature()),
            1 => format!("GRUP World Children of [{:08X}]", self.label),
            2 => format!("GRUP Interior Cell Block {}", self.label),
            3 => format!("GRUP Interior Cell Sub-Block {}", self.label),
            // `LongRecSmall(grsLabel).Hi` first, then `.Lo`.
            4 => format!(
                "GRUP Exterior Cell Block {}, {}",
                (self.label >> 16) as i16,
                self.label as i16
            ),
            5 => format!(
                "GRUP Exterior Cell Sub-Block {}, {}",
                (self.label >> 16) as i16,
                self.label as i16
            ),
            6 => format!("GRUP Cell Children of [{:08X}]", self.label),
            7 => format!("GRUP Topic Children of [{:08X}]", self.label),
            8 => format!("GRUP Cell Persistent Children of [{:08X}]", self.label),
            9 => format!("GRUP Cell Temporary Children of [{:08X}]", self.label),
            10 => format!("GRUP Cell Visible Distant Children of [{:08X}]", self.label),
            other => format!("GRUP Unknown type {other}"),
        }
    }
}

/// Upstream `TwbSubRecordHeaderStruct`: a signature and a 16 bit size, a
/// 32 bit size for Morrowind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubRecordHeaderStruct {
    pub signature: Signature,
    pub data_size: u32,
}

impl SubRecordHeaderStruct {
    /// Port of `TwbSubRecordHeaderStruct.SizeOf`.
    pub fn size() -> usize {
        if game_mode() == GameMode::gmTES3 { 8 } else { 6 }
    }

    pub fn parse(bytes: &[u8], offset: usize) -> Option<Self> {
        let data_size = if game_mode() == GameMode::gmTES3 {
            u32::from_le_bytes(bytes.get(offset + 4..offset + 8)?.try_into().ok()?)
        } else {
            u32::from(u16_at(bytes, offset + 4)?)
        };
        Some(SubRecordHeaderStruct {
            signature: signature_at(bytes, offset)?,
            data_size,
        })
    }

    /// The header as the bytes `WriteToStream` writes. A size over 16 bits
    /// is the caller's problem (`XXXX`); it is truncated here.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8);
        bytes.extend_from_slice(&self.signature.0);
        if game_mode() == GameMode::gmTES3 {
            bytes.extend_from_slice(&self.data_size.to_le_bytes());
        } else {
            bytes.extend_from_slice(&(self.data_size as u16).to_le_bytes());
        }
        bytes
    }
}
