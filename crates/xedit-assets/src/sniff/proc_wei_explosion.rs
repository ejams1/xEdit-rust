// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcWeiExplosion.pas

//! `Weijiesen's blow up thing`: moves the root `NiNode`s under the
//! `NonAccum` node and registers them in the extra targets of the
//! multi-target transform controller, the object palette and the sequence.

use crate::data_format::{El, R, Tree};
use crate::data_format_nif::{NifFile, block, block_by_type, block_type, blocks_count, nif_delete, root_nodes};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, access_violation};
use crate::variant::Variant;

const NON_ACCUM: &str = "NonAccum";

pub struct ProcWeiExplosion {
    base: ProcBase,
}

impl ProcWeiExplosion {
    pub fn new() -> ProcWeiExplosion {
        ProcWeiExplosion {
            base: ProcBase::new("Weijiesen's blow up thing", &[GameType::Fo3, GameType::Fnv], &["nif"]),
        }
    }
}

/// `Elements[aPath]` of a missing element reads through nil upstream.
fn element(tree: &mut Tree, el: El, path: &str) -> R<El> {
    tree.elements(el, path)?.ok_or_else(access_violation)
}

/// `Elements[aPath].LinksTo` of a missing element reads through nil
/// upstream.
fn link(tree: &mut Tree, el: El, path: &str) -> R<Option<El>> {
    let element = element(tree, el, path)?;
    tree.links_to(element)
}

impl Proc for ProcWeiExplosion {
    proc_base!();

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;

        // empty nif
        if blocks_count(tree)? == 0 {
            return Ok(Vec::new());
        }

        // search for key nodes
        let mut non_accum = None;
        for i in 0..blocks_count(tree)? {
            let b = block(tree, i)?;
            if block_type(tree, b) == "NiNode" && tree.edit_values(b, "Name")?.ends_with(NON_ACCUM) {
                non_accum = Some(b);
                break;
            }
        }

        let Some(non_accum) = non_accum else {
            ctx.add_message(format!("\t{}: NonAccum node is missng", file.file_name));
            return Ok(Vec::new());
        };

        let controller = block_by_type(tree, "NiMultiTargetTransformController", false)?;
        let Some(controller) = controller else {
            ctx.add_message(format!(
                "\t{}: NiMultiTargetTransformController is missng",
                file.file_name
            ));
            return Ok(Vec::new());
        };

        let palette = block_by_type(tree, "NiDefaultAVObjectPalette", false)?;
        let Some(palette) = palette else {
            ctx.add_message(format!("\t{}: NiDefaultAVObjectPalette is missing", file.file_name));
            return Ok(Vec::new());
        };

        let sequence = block_by_type(tree, "NiControllerSequence", false)?;
        let Some(sequence) = sequence else {
            ctx.add_message(format!("\t{}: NiControllerSequence is missing", file.file_name));
            return Ok(Vec::new());
        };

        let root = root_nodes(tree)?.first().copied().ok_or_else(access_violation)?;
        let root_children = element(tree, root, "Children")?;

        // root children have acceptable NiNodes
        let mut found = false;
        for i in 0..tree.count(root_children) {
            let link = tree.item(root_children, i)?;
            if let Some(b) = tree.links_to(link)?
                && b != non_accum
                && block_type(tree, b) == "NiNode"
            {
                found = true;
                break;
            }
        }

        if !found {
            ctx.add_message(format!("\t{}: No acceptable NiNodes to process", file.file_name));
            return Ok(Vec::new());
        }

        let non_accum_children = element(tree, non_accum, "Children")?;
        tree.set_count(non_accum_children, 0)?;

        // add root NiNodes to children of NonAccum
        for i in 0..tree.count(root_children) {
            let link = tree.item(root_children, i)?;
            let Some(b) = tree.links_to(link)? else {
                continue;
            };

            if b == non_accum || block_type(tree, b) != "NiNode" {
                continue;
            }

            let index = tree.index(b)?;
            let added = tree.add(non_accum_children)?;
            tree.set_native_value(added, Variant::Int(i64::from(index)))?;
        }

        // remove NonAccum children from root children
        for i in 0..tree.count(non_accum_children) {
            let child = tree.item(non_accum_children, i)?;
            let value = tree.native_value(child)?;
            for j in (0..tree.count(root_children)).rev() {
                let link = tree.item(root_children, j)?;
                if tree.native_value(link)? == value {
                    tree.remove(link)?;
                }
            }
        }

        // add NonAccum children to Extra Targets of controller
        let entries = element(tree, controller, "Extra Targets")?;
        tree.set_count(entries, 0)?;
        for i in 0..tree.count(non_accum_children) {
            let child = tree.item(non_accum_children, i)?;
            let value = tree.native_value(child)?;
            let added = tree.add(entries)?;
            tree.set_native_value(added, value)?;
        }

        // add NonAccum children to object palette
        let entries = element(tree, palette, "Objects")?;
        tree.set_count(entries, 0)?;
        for i in 0..tree.count(non_accum_children) {
            let child = tree.item(non_accum_children, i)?;
            let child_block = tree.links_to(child)?.ok_or_else(access_violation)?;
            let node_name = tree.edit_values(child_block, "Name")?;
            let value = tree.native_value(child)?;
            let entry = tree.add(entries)?;
            tree.set_edit_values(entry, "Name", &node_name)?;
            tree.set_native_values(entry, "AV Object", value)?;
        }

        // add NonAccum children to controlled blocks in sequence
        let entries = element(tree, sequence, "Controlled Blocks")?;
        tree.set_count(entries, 0)?;
        for i in 0..tree.count(non_accum_children) {
            let child = tree.item(non_accum_children, i)?;
            let b = tree.links_to(child)?.ok_or_else(access_violation)?;
            let node_name = tree.edit_values(b, "Name")?;
            let controller_index = tree.index(controller)?;
            let interpolator = link(tree, b, "Controller")?.ok_or_else(access_violation)?;
            let interpolator = tree.native_values(interpolator, "Interpolator")?;
            let entry = tree.add(entries)?;
            tree.set_edit_values(entry, "Node Name", &node_name)?;
            tree.set_native_values(entry, "Controller", Variant::Int(i64::from(controller_index)))?;
            tree.set_native_values(entry, "Interpolator", interpolator)?;
        }

        // remove NiTransformController from NonAccum children
        for i in 0..tree.count(non_accum_children) {
            let child = tree.item(non_accum_children, i)?;
            let b = tree.links_to(child)?.ok_or_else(access_violation)?;
            let controller = tree.native_values(b, "Controller")?.to_i64()? as i32;
            nif_delete(tree, tree.root_el(), controller)?;
        }

        nif.save_to_data()
    }
}
