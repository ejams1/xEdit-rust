// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcRemoveNodes.pas

//! `Remove nodes`: removes the blocks of the names or of a type with their
//! branches.

use crate::data_format::{DfError, El, R, Tree};
use crate::data_format_nif::{
    NifFile, NifOptions, block, block_is_ni_object, block_refs, block_remove_branch, blocks_count, wb_ni_object_list,
};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, ansi_same_text, contains_text, delimited_text,
    same_text, trim,
};
use crate::variant::Variant;

pub struct ProcRemoveNodes {
    base: ProcBase,
    /// `edNames`.
    names_text: String,
    /// `chkExactMatch`.
    exact_match_checked: bool,
    /// `rbName` (1) or `rbType` (2).
    mode_checked: i32,
    /// `cmbType`.
    type_text: String,
    mode: i32,
    names: Vec<String>,
    exact_match: bool,
    block_type: String,
}

impl ProcRemoveNodes {
    pub fn new() -> ProcRemoveNodes {
        ProcRemoveNodes {
            base: ProcBase::new("Remove nodes", GameType::ALL, &["nif", "kf"]),
            names_text: String::new(),
            exact_match_checked: true,
            mode_checked: 1,
            type_text: String::new(),
            mode: 0,
            names: Vec::new(),
            exact_match: true,
            block_type: String::new(),
        }
    }
}

/// `UnlinkBlock`.
fn unlink_block(tree: &mut Tree, target: El) -> R<()> {
    for i in 0..blocks_count(tree)? {
        let b = block(tree, i)?;
        for reference in block_refs(tree, b) {
            if tree.links_to(reference)? == Some(target) {
                tree.set_native_value(reference, Variant::Int(-1))?;
            }
        }
    }
    Ok(())
}

impl Proc for ProcRemoveNodes {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.names_text = storage.get_string("sNames", "");
        self.exact_match_checked = storage.get_bool("bExactMatch", true);
        self.mode_checked = if storage.get_integer("iMode", 1) == 2 { 2 } else { 1 };
        // The sorted list of block types; the text of the combo box is ''
        // until an item is chosen.
        let mut types = wb_ni_object_list();
        types.sort_by_key(|name| name.to_lowercase());
        let wanted = storage.get_string("sType", "");
        let index = types.iter().position(|name| ansi_same_text(name, &wanted)).unwrap_or(0);
        self.type_text = types.get(index).cloned().unwrap_or_default();
    }

    fn on_start(&mut self) -> R<()> {
        // `fMode` keeps its value when neither radio button is checked,
        // which can not happen.
        self.mode = self.mode_checked;
        if self.mode == 1 {
            let names = delimited_text(&self.names_text, ',');
            if names.is_empty() {
                return Err(DfError::new("Set the node name(s) to remove"));
            }
            self.names = names.iter().map(|name| trim(name).to_owned()).collect();
        }
        self.exact_match = self.exact_match_checked;
        self.block_type = self.type_text.clone();
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.tree.nif.options = NifOptions {
            collapse_link_arrays: true,
            remove_unused_strings: true,
        };
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        // Removing blocks shifts the indices: search again until nothing
        // is left to remove.
        loop {
            let mut found = false;
            for i in 0..blocks_count(tree)? {
                let b = block(tree, i)?;
                let mut remove = None;
                if self.mode == 1 {
                    let name = tree.edit_values(b, "Name")?;
                    if self.names.iter().any(|s| {
                        (self.exact_match && same_text(&name, s)) || (!self.exact_match && contains_text(&name, s))
                    }) {
                        remove = Some(b);
                    }
                } else if self.mode == 2 && block_is_ni_object(tree, b, &self.block_type, true) {
                    remove = Some(b);
                }
                let Some(remove) = remove else { continue };
                unlink_block(tree, remove)?;
                block_remove_branch(tree, remove, false)?;
                changed = true;
                found = true;
                break;
            }
            if !found {
                break;
            }
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
