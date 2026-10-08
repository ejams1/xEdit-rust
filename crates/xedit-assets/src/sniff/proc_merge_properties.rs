// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcMergeProperties.pas

//! `Merge properties`: links the shapes to one of the property blocks of
//! the types chosen that have the same values, and removes the others.

use xedit_io::encoding::ansi_compare_text;

use crate::data_format::{DfError, R};
use crate::data_format_nif::{
    NifFile, NifOptions, NifVersion, block, block_referenced_by, block_remove_branch, block_type, blocks_count,
    wb_is_ni_object, wb_ni_object_list,
};
use crate::json::Json;
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, comma_text};
use crate::variant::Variant;

pub struct ProcMergeProperties {
    base: ProcBase,
    ignore_name_checked: bool,
    /// The checked items of `lvProps`.
    blocks_checked: Vec<String>,
    blocks: Vec<String>,
    ignore_name: bool,
}

impl ProcMergeProperties {
    pub fn new() -> ProcMergeProperties {
        ProcMergeProperties {
            base: ProcBase::new("Merge properties", GameType::ALL, &["nif"]),
            ignore_name_checked: true,
            blocks_checked: Vec::new(),
            blocks: Vec::new(),
            ignore_name: true,
        }
    }
}

impl Proc for ProcMergeProperties {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.ignore_name_checked = storage.get_bool("bIgnoreName", true);
        let checked = comma_text(&storage.get_string("sBlocks", "BSShaderTextureSet,NiMaterialProperty"));
        // The property types, sorted.
        let mut types: Vec<String> = wb_ni_object_list()
            .into_iter()
            .filter(|name| {
                name == "BSShaderTextureSet"
                    || (name != "NiProperty"
                        && wb_is_ni_object(name, "NiProperty")
                        && !wb_is_ni_object(name, "NiShadeProperty"))
            })
            .collect();
        types.sort_by(|a, b| ansi_compare_text(a, b));
        self.blocks_checked = types
            .into_iter()
            .filter(|name| checked.iter().any(|known| ansi_compare_text(known, name).is_eq()))
            .collect();
    }

    fn on_start(&mut self) -> R<()> {
        self.ignore_name = self.ignore_name_checked;
        self.blocks = self.blocks_checked.clone();
        if self.blocks.is_empty() {
            return Err(DfError::new("Select properties to merge"));
        }
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
        // The properties seen: the JSON of their values and the block.
        let mut props: Vec<(String, crate::data_format::El)> = Vec::new();
        // In reverse: some go.
        for i in (0..blocks_count(tree)?).rev() {
            let b = block(tree, i)?;
            if !self.blocks.iter().any(|name| name == block_type(tree, b)) {
                continue;
            }
            let mut json = Json::object();
            for index in 0..tree.count(b) {
                let el = tree.item(b, index)?;
                let name = tree.def(el)?.name.clone();
                if self.ignore_name && name == "Name" {
                    continue;
                }
                // Unused fields.
                if tree.nif.nif_version >= NifVersion::Fo3
                    && block_type(tree, b) == "NiMaterialProperty"
                    && name == "Specular Color"
                {
                    continue;
                }
                tree.serialize_to_json(el, &mut json)?;
            }
            let token = format!("{} {}", block_type(tree, b), json.to_text(true));
            // `TStringList.IndexOf` ignores case.
            match props.iter().find(|(known, _)| ansi_compare_text(known, &token).is_eq()) {
                Some(&(_, kept)) => {
                    let index = tree.index(kept)?;
                    // Link to the property kept and remove this one.
                    for reference in block_referenced_by(tree, b)? {
                        tree.set_native_value(reference, Variant::Int(i64::from(index)))?;
                    }
                    block_remove_branch(tree, b, false)?;
                    changed = true;
                }
                None => props.push((token, b)),
            }
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
