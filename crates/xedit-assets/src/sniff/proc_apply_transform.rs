// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcApplyTransform.pas

//! `Apply transformation`: applies the transform of every node to its
//! children and geometry, optionally leaving the ones that can not be
//! transformed without issues.

use crate::data_format::R;
use crate::data_format_nif::{ApplyTransformOptions, NifFile, block_apply_transform, blocks_count, root_node};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage};

pub struct ProcApplyTransform {
    base: ProcBase,
    skip_skinned: bool,
    skip_animated: bool,
    skip_collision: bool,
    skip_root: bool,
    skip_controller_manager: bool,
    options: ApplyTransformOptions,
}

impl ProcApplyTransform {
    pub fn new() -> ProcApplyTransform {
        ProcApplyTransform {
            base: ProcBase::new("Apply transformation", GameType::ALL, &["nif"]),
            skip_skinned: true,
            skip_animated: true,
            skip_collision: true,
            skip_root: true,
            skip_controller_manager: true,
            options: ApplyTransformOptions::default(),
        }
    }
}

impl Proc for ProcApplyTransform {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.skip_skinned = storage.get_bool("bSkipSkinned", true);
        self.skip_animated = storage.get_bool("bSkipAnimated", true);
        self.skip_collision = storage.get_bool("bSkipCollision", true);
        self.skip_root = storage.get_bool("bSkipRoot", true);
        self.skip_controller_manager = storage.get_bool("bSkipControllerManager", true);
    }

    fn on_start(&mut self) -> R<()> {
        self.options = ApplyTransformOptions {
            skinned: !self.skip_skinned,
            animated: !self.skip_animated,
            collision: !self.skip_collision,
            root: !self.skip_root,
            ctrl_manager: !self.skip_controller_manager,
        };
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        if blocks_count(tree)? == 0 {
            return Ok(Vec::new());
        }
        let root = root_node(tree)?;
        if block_apply_transform(tree, root, true, self.options)? {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
