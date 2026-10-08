// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcShaderFlagsUpdate.pas

//! `Update shader flags`: adds, sets or removes flags of the two flag
//! fields of the `BSShaderProperty` blocks.

use crate::data_format::{DfError, El, R, Tree};
use crate::data_format_nif::{NifFile, block, block_is_ni_object, blocks_count};
use crate::data_format_nif_types::{
    wb_bs_shader_flags, wb_bs_shader_flags2, wb_fallout4_shader_property_flags1, wb_fallout4_shader_property_flags2,
    wb_skyrim_shader_property_flags1, wb_skyrim_shader_property_flags2,
};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage};
use crate::variant::Variant;

pub struct ProcShaderFlagsUpdate {
    base: ProcBase,
    /// `chkReport`.
    report_checked: bool,
    /// The checked items of `clbFlags1` and `clbFlags2`.
    flags_checked: u32,
    flags2_checked: u32,
    report_only: bool,
    mode: i32,
    flags: u32,
    flags2: u32,
}

impl ProcShaderFlagsUpdate {
    pub fn new() -> ProcShaderFlagsUpdate {
        ProcShaderFlagsUpdate {
            base: ProcBase::new(
                "Update shader flags",
                &[
                    GameType::Fo3,
                    GameType::Fnv,
                    GameType::Tes5,
                    GameType::Sse,
                    GameType::Fo4,
                ],
                &["nif"],
            ),
            report_checked: false,
            flags_checked: 0,
            flags2_checked: 0,
            report_only: false,
            mode: 0,
            flags: 0,
            flags2: 0,
        }
    }
}

/// The number of items of the check list boxes for the game of `cmbGame`
/// (`cmbGameSelect`): the flags of the definitions; none for an index out
/// of the list, which leaves the boxes empty.
fn flag_counts(game: i32) -> (usize, usize) {
    let count = |def: crate::data_format::Def| def.values_map().len();
    match game {
        0 => (
            count(wb_bs_shader_flags("", "", &[])),
            count(wb_bs_shader_flags2("", "", &[])),
        ),
        1 => (
            count(wb_skyrim_shader_property_flags1("", "", &[])),
            count(wb_skyrim_shader_property_flags2("", "", &[])),
        ),
        2 => (
            count(wb_fallout4_shader_property_flags1("", "", &[])),
            count(wb_fallout4_shader_property_flags2("", "", &[])),
        ),
        _ => (0, 0),
    }
}

/// `Int32ToCheckListBox` then `CheckListBoxToInt32`: the bits of the items
/// the box has.
fn mask(flags: u32, count: usize) -> u32 {
    if count >= 32 {
        flags
    } else {
        flags & ((1u32 << count) - 1)
    }
}

impl Proc for ProcShaderFlagsUpdate {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        let game = storage.get_integer("iGame", 0);
        let (count1, count2) = flag_counts(game);
        self.report_checked = storage.get_bool("bReportOnly", false);
        let mode = storage.get_integer("iMode", 0);
        // `rbAdd` is checked for 0 and for a value out of range.
        self.mode = if (0..=2).contains(&mode) { mode } else { 0 };
        self.flags_checked = mask(storage.get_integer("iFlags", 0) as u32, count1);
        self.flags2_checked = mask(storage.get_integer("iFlags2", 0) as u32, count2);
    }

    fn on_start(&mut self) -> R<()> {
        self.report_only = self.report_checked;
        self.base.no_output = self.report_only;
        self.flags = self.flags_checked;
        self.flags2 = self.flags2_checked;
        if self.flags == 0 && self.flags2 == 0 {
            return Err(DfError::new("No flags selected"));
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut log: Vec<String> = Vec::new();
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;

        // `UpdateFlags`.
        let mut update_flags = |tree: &mut Tree, el: Option<El>, flags: u32| -> R<()> {
            let Some(el) = el else { return Ok(()) };
            let current = tree.native_value(el)?.to_i64()? as u32;
            let new = match self.mode {
                0 => current | flags,
                1 => flags,
                _ => current & !flags,
            };
            if current != new {
                let old = tree.edit_value(el)?;
                tree.set_native_value(el, Variant::Int(i64::from(new)))?;
                if self.report_only {
                    if log.is_empty() {
                        log.push(format!("\r\n{}", file.file_name));
                    }
                    let path = tree.path(el)?;
                    let value = tree.edit_value(el)?;
                    log.push(format!("\t{path}\r\n\t\t\"{old}\"\r\n\t\t\"{value}\""));
                }
                changed = true;
            }
            Ok(())
        };

        for i in 0..blocks_count(tree)? {
            let shader = block(tree, i)?;
            if !block_is_ni_object(tree, shader, "BSShaderProperty", true) {
                continue;
            }
            let flags1 = tree.elements(shader, "Shader Flags 1")?;
            update_flags(tree, flags1, self.flags)?;
            let flags2 = tree.elements(shader, "Shader Flags 2")?;
            update_flags(tree, flags2, self.flags2)?;
        }

        if changed {
            if !self.report_only {
                return nif.save_to_data();
            }
            ctx.add_messages(log);
        }
        Ok(Vec::new())
    }
}
