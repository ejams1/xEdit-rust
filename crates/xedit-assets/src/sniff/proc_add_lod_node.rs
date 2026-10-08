// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcAddLODNode.pas

//! `Add NiLODNode`: puts the shapes under the root `BSFadeNode` into a
//! `NiLODNode` with range or screen LOD data.

use crate::data_format::{DfError, El, R, df_str_to_float};
use crate::data_format_nif::{
    NifFile, NifOptions, add_block, block, block_is_ni_object, block_type, blocks_count, convert_block, insert_block,
    root_node,
};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, string_list_lines,
    string_list_text_of, string_to_text, text_to_string, trim,
};
use crate::variant::Variant;

pub struct ProcAddLODNode {
    base: ProcBase,
    /// `chkRange` (`NiRangeLODData`) or `chkScreen` (`NiScreenLODData`).
    lod_data_checked: String,
    extents_lines: Vec<String>,
    proportions_lines: Vec<String>,
    lod_data: String,
    extents: Vec<f64>,
    proportions: Vec<f64>,
}

impl ProcAddLODNode {
    pub fn new() -> ProcAddLODNode {
        ProcAddLODNode {
            base: ProcBase::new(
                "Add NiLODNode",
                &[GameType::Tes4, GameType::Fo3, GameType::Fnv],
                &["nif"],
            ),
            lod_data_checked: "NiRangeLODData".to_owned(),
            extents_lines: vec!["2000".to_owned(), "50000".to_owned()],
            proportions_lines: vec!["0.48".to_owned()],
            lod_data: String::new(),
            extents: Vec::new(),
            proportions: Vec::new(),
        }
    }
}

/// The floats of the memo lines, the empty ones skipped.
fn parse_lines(lines: &[String], what: &str) -> R<Vec<f64>> {
    let mut result = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let s = trim(line);
        if s.is_empty() {
            continue;
        }
        match df_str_to_float(s) {
            Ok(value) => result.push(value),
            Err(_) => {
                return Err(DfError::new(format!("Line {} has invalid {what} value {s}", index + 1)));
            }
        }
    }
    Ok(result)
}

impl Proc for ProcAddLODNode {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.lod_data_checked = storage.get_string("sLODData", "NiRangeLODData");
        // The memo's text has no break after its last line.
        let memo_text = |lines: &[String]| {
            let text = string_list_text_of(lines);
            text.strip_suffix("\r\n").unwrap_or(&text).to_owned()
        };
        let extents = text_to_string(&memo_text(&self.extents_lines));
        self.extents_lines = string_list_lines(&string_to_text(&storage.get_string("sExtents", &extents)));
        let proportions = text_to_string(&memo_text(&self.proportions_lines));
        self.proportions_lines = string_list_lines(&string_to_text(&storage.get_string("sProportions", &proportions)));
    }

    fn on_start(&mut self) -> R<()> {
        // Neither radio button is checked for another value: the data type
        // stays empty.
        self.lod_data = match self.lod_data_checked.as_str() {
            "NiRangeLODData" | "NiScreenLODData" => self.lod_data_checked.clone(),
            _ => String::new(),
        };
        self.extents = vec![0.0];
        self.extents.extend(parse_lines(&self.extents_lines, "extent")?);
        self.proportions = parse_lines(&self.proportions_lines, "proportion")?;
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        nif.tree.nif.options = NifOptions {
            collapse_link_arrays: true,
            remove_unused_strings: false,
        };
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        if blocks_count(tree)? == 0 {
            return Ok(Vec::new());
        }
        let root = root_node(tree)?;
        if block_type(tree, root) != "BSFadeNode" {
            return Ok(Vec::new());
        }
        let Some(entries) = tree.elements(root, "Children")? else {
            return Ok(Vec::new());
        };
        let mut lod_node: Option<El> = None;
        let mut shapes: Vec<El> = Vec::new();
        for i in 0..tree.count(entries) {
            let entry = tree.item(entries, i)?;
            let Some(child) = tree.links_to(entry)? else { continue };
            // An existing NiLODNode.
            if block_type(tree, child) == "NiLODNode" {
                lod_node = Some(child);
            } else if block_is_ni_object(tree, child, "NiTriBasedGeom", true) {
                // The shapes.
                shapes.push(entry);
            }
        }
        if shapes.len() < 2 && lod_node.is_none() {
            return Ok(Vec::new());
        }
        let (lod_node, lod_data_node) = match lod_node {
            None => {
                // The NiLODNode at the place of the first shape, a child of
                // the root.
                let first = tree.native_value(shapes[0])?.to_i32()?;
                let lod_node = insert_block(tree, first, "NiLODNode")?;
                let entry = tree.add(entries)?;
                let index = tree.index(lod_node)?;
                tree.set_native_value(entry, Variant::Int(i64::from(index)))?;
                (lod_node, None)
            }
            Some(lod_node) => {
                let link = tree
                    .elements(lod_node, "LOD Level Data")?
                    .ok_or_else(access_violation)?;
                (lod_node, tree.links_to(link)?)
            }
        };
        // The LOD data.
        let lod_data_node = match lod_data_node {
            None => {
                let data = add_block(tree, &self.lod_data)?;
                let index = tree.index(data)?;
                tree.set_native_values(lod_node, "LOD Level Data", Variant::Int(i64::from(index)))?;
                data
            }
            Some(data) if block_type(tree, data) != self.lod_data => {
                let i = tree.index(data)?;
                convert_block(tree, i, &self.lod_data)?;
                block(tree, i)?
            }
            Some(data) => data,
        };
        // The shapes go under the NiLODNode.
        let lod_children = tree.elements(lod_node, "Children")?.ok_or_else(access_violation)?;
        for &shape in &shapes {
            let entry = tree.add(lod_children)?;
            let value = tree.native_value(shape)?;
            tree.set_native_value(entry, value)?;
            tree.set_native_value(shape, Variant::Int(-1))?;
        }
        // The LOD data of the children.
        for i in 0..tree.count(lod_children) {
            let data_type = block_type(tree, lod_data_node);
            if data_type == "NiRangeLODData" {
                let levels = tree
                    .elements(lod_data_node, "LOD Levels")?
                    .ok_or_else(access_violation)?;
                if i == 0 {
                    tree.set_count(levels, 0)?;
                }
                let entry = tree.add(levels)?;
                let i = i as usize;
                if i + 1 < self.extents.len() {
                    tree.set_native_values(entry, "Near Extent", Variant::Float(self.extents[i]))?;
                    tree.set_native_values(entry, "Far Extent", Variant::Float(self.extents[i + 1]))?;
                }
            } else if data_type == "NiScreenLODData" {
                let levels = tree
                    .elements(lod_data_node, "Proportion Levels")?
                    .ok_or_else(access_violation)?;
                if i == 0 {
                    tree.set_count(levels, 0)?;
                }
                if (i as usize) < self.proportions.len() {
                    let entry = tree.add(levels)?;
                    tree.set_native_value(entry, Variant::Float(self.proportions[i as usize]))?;
                }
            }
        }
        nif.save_to_data()
    }
}
