// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcRemoveControlledBlocks.pas

//! `Remove controlled blocks`: removes the controlled blocks of the names
//! (or of the other names) from the sequences, and in a KF their
//! interpolators and controllers.

use crate::data_format::R;
use crate::data_format_nif::{NifFile, block_remove_branch, block_type, blocks_by_type, root_node};
use crate::proc_base;
use crate::sniff::proc_anim_quadratic_to_linear::{names_match, split_names};
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation};
use crate::variant::Variant;

pub struct ProcRemoveControlledBlocks {
    base: ProcBase,
    names_text: String,
    exact_match_checked: bool,
    not_matching_checked: bool,
    names: Vec<String>,
    exact_match: bool,
    not_matching: bool,
}

impl ProcRemoveControlledBlocks {
    pub fn new() -> ProcRemoveControlledBlocks {
        ProcRemoveControlledBlocks {
            base: ProcBase::new(
                "Remove controlled blocks",
                &[GameType::Tes4, GameType::Fo3, GameType::Fnv],
                &["nif", "kf"],
            ),
            names_text: "Head,Neck".to_owned(),
            exact_match_checked: true,
            not_matching_checked: false,
            names: Vec::new(),
            exact_match: true,
            not_matching: false,
        }
    }
}

impl Proc for ProcRemoveControlledBlocks {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.names_text = storage.get_string("sNames", "Head,Neck");
        self.exact_match_checked = storage.get_bool("bExactMatch", true);
        self.not_matching_checked = storage.get_bool("bNotMatching", false);
    }

    fn on_start(&mut self) -> R<()> {
        self.names = split_names(&self.names_text)?;
        self.exact_match = self.exact_match_checked;
        self.not_matching = self.not_matching_checked;
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        for b in blocks_by_type(tree, "NiSequence", true)? {
            let Some(entries) = tree.elements(b, "Controlled Blocks")? else {
                continue;
            };
            for i in (0..tree.count(entries)).rev() {
                let entry = tree.item(entries, i)?;
                let name = tree.edit_values(entry, "Node Name")?;
                if names_match(&name, &self.names, self.exact_match, self.not_matching) {
                    let link = tree.elements(entry, "Interpolator")?.ok_or_else(access_violation)?;
                    let interpolator = tree.links_to(link)?;
                    let link = tree.elements(entry, "Controller")?.ok_or_else(access_violation)?;
                    let controller = tree.links_to(link)?;
                    // The controlled block.
                    tree.remove(entry)?;
                    // The branches of its blocks, in a KF.
                    let root = root_node(tree)?;
                    if block_type(tree, root) == "NiControllerSequence" {
                        if let Some(interpolator) = interpolator {
                            block_remove_branch(tree, interpolator, false)?;
                        }
                        if let Some(controller) = controller {
                            block_remove_branch(tree, controller, false)?;
                        }
                    }
                    changed = true;
                }
                let count = tree.count(entries);
                tree.set_native_values(b, "Num Controlled Blocks", Variant::Int(i64::from(count)))?;
            }
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
