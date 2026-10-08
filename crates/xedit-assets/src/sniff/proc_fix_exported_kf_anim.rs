// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcFixExportedKFAnim.pas

//! `Fix 3DS exported KF`: sets the missing controller type of the
//! controlled blocks and the `start` text key of animations exported from
//! 3ds Max.

use crate::data_format::R;
use crate::data_format_nif::{NifFile, block_by_type, blocks_count, root_node};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject};

pub struct ProcFixExportedKFAnim {
    base: ProcBase,
}

impl ProcFixExportedKFAnim {
    pub fn new() -> ProcFixExportedKFAnim {
        ProcFixExportedKFAnim {
            base: ProcBase::new(
                "Fix 3DS exported KF",
                &[GameType::Tes4, GameType::Fo3, GameType::Fnv],
                &["kf"],
            ),
        }
    }
}

impl Proc for ProcFixExportedKFAnim {
    proc_base!();

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        if blocks_count(tree)? == 0 {
            return Ok(Vec::new());
        }
        let root = root_node(tree)?;
        let Some(entries) = tree.elements(root, "Controlled Blocks")? else {
            return Ok(Vec::new());
        };
        for i in 0..tree.count(entries) {
            let entry = tree.item(entries, i)?;
            if tree.edit_values(entry, "Controller Type")?.is_empty() {
                tree.set_edit_values(entry, "Controller Type", "NiTransformController")?;
                changed = true;
            }
        }
        if let Some(extra_data) = block_by_type(tree, "NiTextKeyExtraData", false)?
            && let Some(entries) = tree.elements(extra_data, "Text Keys")?
        {
            for i in 0..tree.count(entries) {
                let entry = tree.item(entries, i)?;
                let key_value = tree.edit_values(entry, "Value")?;
                if key_value.encode_utf16().count() > 5 && key_value.starts_with("start") {
                    tree.set_edit_values(entry, "Value", "start")?;
                    changed = true;
                }
            }
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
