// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcAttachParent.pas

//! `Attach parent NiNode`: puts a new `NiNode` between the first node of a
//! name and its parent.

use crate::data_format::{DfError, R};
use crate::data_format_nif::{NifFile, block_by_name, blocks_by_type, blocks_count, insert_block};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation};
use crate::variant::Variant;

pub struct ProcAttachParent {
    base: ProcBase,
    /// `edFindNodeName`.
    find_node_name: String,
    /// `edAttachNodeName`.
    attach_node_name: String,
}

impl ProcAttachParent {
    pub fn new() -> ProcAttachParent {
        ProcAttachParent {
            base: ProcBase::new("Attach parent NiNode", GameType::ALL, &["nif"]),
            find_node_name: "##SightingNode".to_owned(),
            attach_node_name: "##ISControl".to_owned(),
        }
    }
}

impl Proc for ProcAttachParent {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.find_node_name = storage.get_string("sFindNodeName", "##SightingNode");
        self.attach_node_name = storage.get_string("sAttachNodeName", "##ISControl");
    }

    fn on_start(&mut self) -> R<()> {
        if self.find_node_name.is_empty() {
            return Err(DfError::new("Name to find can not be empty"));
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        if blocks_count(tree)? == 0 {
            return Ok(Vec::new());
        }
        // The parent node exists already.
        if block_by_name(tree, &self.attach_node_name, "")?.is_some() {
            return Ok(Vec::new());
        }
        // The node to find does not exist.
        if block_by_name(tree, &self.find_node_name, "")?.is_none() {
            return Ok(Vec::new());
        }
        // Search the children of all NiNodes.
        for node in blocks_by_type(tree, "NiNode", true)? {
            let Some(children) = tree.elements(node, "Children")? else {
                continue;
            };
            for i in 0..tree.count(children) {
                let item = tree.item(children, i)?;
                let Some(child) = tree.links_to(item)? else {
                    continue;
                };
                if tree.edit_values(child, "Name")? != self.find_node_name {
                    continue;
                }
                let child_index = tree.index(child)?;
                let parent = insert_block(tree, child_index, "NiNode")?;
                tree.set_edit_values(parent, "Name", &self.attach_node_name)?;
                let parent_children = tree.elements(parent, "Children")?.ok_or_else(access_violation)?;
                let entry = tree.add(parent_children)?;
                // The child moved up by one with the insertion.
                let child_index = tree.index(child)?;
                tree.set_native_value(entry, Variant::Int(i64::from(child_index)))?;
                let item = tree.item(children, i)?;
                let parent_index = tree.index(parent)?;
                tree.set_native_value(item, Variant::Int(i64::from(parent_index)))?;
                changed = true;
                break;
            }
            if changed {
                break;
            }
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
