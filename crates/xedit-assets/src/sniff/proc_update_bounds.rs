// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcUpdateBounds.pas

//! `Update bounds`: recalculates the bounds (center and radius) of
//! `BSTriShape`, `NiTriShapeData` and `NiTriStripsData`.

use crate::data_format::R;
use crate::data_format_nif::{NifFile, block, blocks_count, block_update_bounds};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject};

pub struct ProcUpdateBounds {
    base: ProcBase,
}

impl ProcUpdateBounds {
    pub fn new() -> ProcUpdateBounds {
        ProcUpdateBounds {
            base: ProcBase::new("Update bounds", GameType::ALL, &["nif"]),
        }
    }
}

impl Proc for ProcUpdateBounds {
    proc_base!();

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        let mut changed = false;
        for index in 0..blocks_count(tree)? {
            let block = block(tree, index)?;
            changed = block_update_bounds(tree, block)? || changed;
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
