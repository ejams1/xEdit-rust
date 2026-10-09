// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcCopyControlledBlocks.pas

//! `Copy anim controlled blocks`: copies the controlled blocks of the
//! animation of the same path in a source folder that the destination does
//! not have, with their interpolators and interpolator data.

use std::collections::HashMap;
use std::path::Path;

use xedit_io::encoding::ansi_compare_text;

use crate::data_format::{El, R, Tree};
use crate::data_format_nif::{NifFile, add_block, block_type, blocks_count, root_node};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage};
use crate::variant::Variant;

pub struct ProcCopyControlledBlocks {
    base: ProcBase,
    /// `edSourceDirectory`.
    source_text: String,
    /// `fSourceDirectory`.
    source_directory: String,
}

impl ProcCopyControlledBlocks {
    pub fn new() -> ProcCopyControlledBlocks {
        ProcCopyControlledBlocks {
            base: ProcBase::new(
                "Copy anim controlled blocks",
                &[GameType::Tes4, GameType::Fo3, GameType::Fnv],
                &["kf"],
            ),
            source_text: String::new(),
            source_directory: String::new(),
        }
    }
}

/// `ControlledBlockToken`: a unique token string for a controlled block.
fn controlled_block_token(tree: &mut Tree, entry: El) -> R<String> {
    Ok(format!(
        "{} {}",
        tree.edit_values(entry, "Node Name")?,
        tree.edit_values(entry, "Controller Type")?
    ))
}

/// `Elements[aPath].LinksTo` of a missing element reads through nil
/// upstream.
fn link(tree: &mut Tree, element: El, path: &str) -> R<Option<El>> {
    let element = tree
        .elements(element, path)?
        .ok_or_else(crate::sniff::processor::access_violation)?;
    tree.links_to(element)
}

/// `CopyInterpolator`: copies the interpolator and the blocks it refers to,
/// which the later interpolators reuse (`CopiedBlocks` by their source
/// index).
fn copy_interpolator(tree: &mut Tree, src_tree: &mut Tree, block: El, copied: &mut HashMap<i32, El>) -> R<El> {
    // copy Interpolator itself first
    // don't need to check for already copied ones because controlled blocks can't use the same interpolators
    // they are always new
    let block_type_name = block_type(src_tree, block);
    let result = add_block(tree, block_type_name)?;
    tree.assign_from(result, src_tree, Some(block))?;

    // copying referenced blocks (interpolator data) if any
    for i in 0..src_tree.count(block) {
        let reference = src_tree.item(block, i)?;
        let Some(ref_block) = src_tree.links_to(reference)? else {
            continue;
        };
        let index = src_tree.index(ref_block)?;

        // check if we already copied that block because interpolator data can be reused by interpolators
        let new_ref_block = match copied.get(&index) {
            Some(existing) => *existing,
            None => {
                let created = add_block(tree, block_type(src_tree, ref_block))?;
                tree.assign_from(created, src_tree, Some(ref_block))?;
                copied.insert(index, created);
                created
            }
        };

        // linking to the copied block
        let new_index = tree.index(new_ref_block)?;
        let target = tree.item(result, i)?;
        tree.set_native_value(target, Variant::Int(i64::from(new_index)))?;
    }
    Ok(result)
}

impl Proc for ProcCopyControlledBlocks {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.source_text = storage.get_string("sSourceDirectory", "");
    }

    fn on_start(&mut self) -> R<()> {
        self.source_directory = self.source_text.clone();
        if self.source_directory.is_empty() || !Path::new(&self.source_directory).is_dir() {
            return Err(crate::data_format::DfError::new("Source directory not found"));
        }
        if !self.source_directory.ends_with('\\') {
            self.source_directory.push('\\');
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let source = format!("{}{}", self.source_directory, file.file_name);
        if !Path::new(&source).is_file() {
            return Ok(Vec::new());
        }

        let mut changed = false;
        let mut nif = NifFile::new()?;
        let mut src_nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        src_nif.load_from_file(Path::new(&source))?;

        if blocks_count(&mut src_nif.tree)? == 0 || blocks_count(&mut nif.tree)? == 0 {
            return Ok(Vec::new());
        }

        let src_root = root_node(&mut src_nif.tree)?;
        let dst_root = root_node(&mut nif.tree)?;
        let (Some(src_blocks), Some(dst_blocks)) = (
            src_nif.tree.elements(src_root, "Controlled Blocks")?,
            nif.tree.elements(dst_root, "Controlled Blocks")?,
        ) else {
            return Ok(Vec::new());
        };

        // building a list of controlled blocks in destination nif
        let mut dst_list: Vec<String> = Vec::new();
        for i in 0..nif.tree.count(dst_blocks) {
            let entry = nif.tree.item(dst_blocks, i)?;
            dst_list.push(controlled_block_token(&mut nif.tree, entry)?);
        }
        // `DstList.Sorted := True` sorts with `AnsiCompareText` (a quick sort).
        dst_list.sort_by(|a, b| ansi_compare_text(a, b));

        // copying controlled blocks from the source nif
        let mut copied: HashMap<i32, El> = HashMap::new();
        for i in 0..src_nif.tree.count(src_blocks) {
            let entry = src_nif.tree.item(src_blocks, i)?;
            let token = controlled_block_token(&mut src_nif.tree, entry)?;

            // skip if block with the same token exists in the destination nif
            // (`IndexOf` of a sorted list: a binary search for the first of
            // the equal names)
            let index = dst_list.partition_point(|known| ansi_compare_text(known, &token).is_lt());
            if index < dst_list.len() && ansi_compare_text(&dst_list[index], &token).is_eq() {
                continue;
            }

            // copy controlled block
            let new_entry = nif.tree.add(dst_blocks)?;
            nif.tree.assign_from(new_entry, &mut src_nif.tree, Some(entry))?;
            let count = nif
                .tree
                .native_values(dst_blocks, "..\\Num Controlled Blocks")?
                .to_i64()?;
            nif.tree
                .set_native_values(dst_blocks, "..\\Num Controlled Blocks", Variant::Int(count + 1))?;

            // copy interpolator if any
            let interpolator = link(&mut src_nif.tree, entry, "Interpolator")?;
            if let Some(interpolator) = interpolator {
                let new_interpolator = copy_interpolator(&mut nif.tree, &mut src_nif.tree, interpolator, &mut copied)?;
                let index = nif.tree.index(new_interpolator)?;
                nif.tree
                    .set_native_values(new_entry, "Interpolator", Variant::Int(i64::from(index)))?;
            }

            changed = true;
        }

        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
