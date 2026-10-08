// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcRemoveUnusedNodes.pas

//! `Remove unused nodes`: removes the blocks no root node reaches,
//! optionally keeping only the first root.

use crate::data_format::{El, R, Tree};
use crate::data_format_nif::{NifFile, block_refs, blocks_count, footer};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage};

pub struct ProcRemoveUnusedNodes {
    base: ProcBase,
    /// `chkSingleRoot`.
    single_root: bool,
}

impl ProcRemoveUnusedNodes {
    pub fn new() -> ProcRemoveUnusedNodes {
        ProcRemoveUnusedNodes {
            base: ProcBase::new("Remove unused nodes", GameType::ALL, &["nif", "kf", "kfm"]),
            single_root: true,
        }
    }
}

/// `CountBlocksUsage`.
fn count_blocks_usage(tree: &mut Tree, block: Option<El>, usage: &mut [i32]) -> R<()> {
    let Some(block) = block else { return Ok(()) };
    let index = tree.index(block)? as usize;
    usage[index] += 1;
    // Scan the references only once.
    if usage[index] == 1 {
        for reference in block_refs(tree, block) {
            let target = tree.links_to(reference)?;
            count_blocks_usage(tree, target, usage)?;
        }
    }
    Ok(())
}

impl Proc for ProcRemoveUnusedNodes {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.single_root = storage.get_bool("bSingleRoot", true);
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let root = nif.root;
        let tree = &mut nif.tree;
        if blocks_count(tree)? == 0 {
            return Ok(Vec::new());
        }
        let footer_el = footer(tree)?;
        if self.single_root
            && let Some(roots) = tree.elements(footer_el, "Roots")?
            && tree.count(roots) > 1
        {
            tree.set_count(roots, 1)?;
        }
        let mut usage = vec![0; blocks_count(tree)? as usize];
        // The root nodes are linked from the footer.
        for reference in block_refs(tree, footer_el) {
            let target = tree.links_to(reference)?;
            count_blocks_usage(tree, target, &mut usage)?;
        }
        let mut modified = false;
        for index in (0..usage.len()).rev() {
            if usage[index] == 0 {
                tree.delete(root, index as i32)?;
                modified = true;
            }
        }
        if modified {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
