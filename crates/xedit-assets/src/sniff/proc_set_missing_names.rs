// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcSetMissingNames.pas

//! `Set missing names`: names the unnamed `NiAVObject` blocks after the
//! file, and optionally the root node.

use crate::data_format::R;
use crate::data_format_nif::{
    NifFile, block_by_name, block_is_ni_object, blocks_by_type, blocks_count, get_unique_name, root_node,
};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, change_file_ext, extract_file_name, same_text,
};

pub struct ProcSetMissingNames {
    base: ProcBase,
    /// `chkRenameRoot`.
    rename_root: bool,
}

impl ProcSetMissingNames {
    pub fn new() -> ProcSetMissingNames {
        ProcSetMissingNames {
            base: ProcBase::new("Set missing names", GameType::ALL, &["nif"]),
            rename_root: true,
        }
    }
}

impl Proc for ProcSetMissingNames {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.rename_root = storage.get_bool("bRenameRoot", true);
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut i = 0;
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        if blocks_count(tree)? == 0 {
            return Ok(Vec::new());
        }
        let fname = change_file_ext(extract_file_name(&file.file_name), "");
        let root = root_node(tree)?;
        if self.rename_root
            && block_is_ni_object(tree, root, "NiAVObject", true)
            && !same_text(&tree.edit_values(root, "Name")?, &fname)
        {
            let name = get_unique_name(tree, &fname)?;
            tree.set_edit_values(root, "Name", &name)?;
            changed = true;
        }
        for b in blocks_by_type(tree, "NiAVObject", true)? {
            if !tree.edit_values(b, "Name")?.is_empty() {
                continue;
            }
            let new_name = loop {
                let name = format!("{fname}:{i}");
                i += 1;
                if block_by_name(tree, &name, "")?.is_none() {
                    break name;
                }
            };
            tree.set_edit_values(b, "Name", &new_name)?;
            changed = true;
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
