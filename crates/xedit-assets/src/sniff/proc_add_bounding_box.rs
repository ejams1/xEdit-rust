// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcAddBoundingBox.pas

//! `Add bounding box`: adds a `Bounding Box` node with a box bounding
//! volume to the root of Morrowind meshes.

use crate::data_format::{R, df_str_to_float};
use crate::data_format_nif::{NifFile, block_add_child, blocks_count, root_node};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, trim};
use crate::variant::{Variant, str_to_int};

/// `sBoundingBox`.
const BOUNDING_BOX: &str = "Bounding Box";

pub struct ProcAddBoundingBox {
    base: ProcBase,
    flags_text: String,
    /// The center and extent edits: X, Y, Z of each.
    texts: [String; 6],
    flags: i32,
    /// The center and extent of the run.
    values: [String; 6],
}

impl ProcAddBoundingBox {
    pub fn new() -> ProcAddBoundingBox {
        ProcAddBoundingBox {
            base: ProcBase::new("Add bounding box", &[GameType::Tes3], &["nif"]),
            flags_text: "12".to_owned(),
            texts: Default::default(),
            flags: 12,
            values: Default::default(),
        }
    }
}

const KEYS: [&str; 6] = ["sCenterX", "sCenterY", "sCenterZ", "sExtentX", "sExtentY", "sExtentZ"];
const PATHS: [&str; 6] = [
    "Center\\X",
    "Center\\Y",
    "Center\\Z",
    "Extent\\X",
    "Extent\\Y",
    "Extent\\Z",
];

impl Proc for ProcAddBoundingBox {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.flags_text = storage.get_string("sFlags", "12");
        for (index, key) in KEYS.iter().enumerate() {
            self.texts[index] = storage.get_string(key, "");
        }
    }

    fn on_start(&mut self) -> R<()> {
        self.flags = str_to_int(&self.flags_text).unwrap_or(0);
        for index in 0..6 {
            // `GetVerifyFloat`: empty is 0.
            let text = trim(&self.texts[index]).to_owned();
            self.values[index] = if text.is_empty() {
                "0".to_owned()
            } else {
                df_str_to_float(&text)?;
                text
            };
        }
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
        let Some(children) = tree.elements(root, "Children")? else {
            return Ok(Vec::new());
        };
        for i in 0..tree.count(children) {
            let item = tree.item(children, i)?;
            if let Some(child) = tree.links_to(item)?
                && tree.edit_values(child, "Name")? == BOUNDING_BOX
            {
                return Ok(Vec::new());
            }
        }
        let bv = block_add_child(tree, root, "NiNode")?;
        tree.set_edit_values(bv, "Name", BOUNDING_BOX)?;
        tree.set_native_values(bv, "Flags", Variant::Int(i64::from(self.flags)))?;
        tree.set_edit_values(bv, "Has Bounding Volume", "yes")?;
        tree.set_edit_values(bv, "Bounding Volume\\Collision Type", "BOX_BV")?;
        let v = tree
            .elements(bv, "Bounding Volume\\Box")?
            .ok_or_else(access_violation)?;
        for (index, path) in PATHS.iter().enumerate() {
            tree.set_edit_values(v, path, &self.values[index])?;
        }
        tree.set_edit_values(v, "Axis\\[0]\\X", "1.0")?;
        tree.set_edit_values(v, "Axis\\[1]\\Y", "1.0")?;
        tree.set_edit_values(v, "Axis\\[2]\\Z", "1.0")?;
        nif.save_to_data()
    }
}
