// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcAnimSkeletonDeath.pas

//! `Add blocks from skeleton`: adds the controlled blocks the `death.kf`
//! of a folder lacks for the bones of the `skeleton.nif` beside it.

use std::path::Path;

use crate::data_format::{El, R, Tree};
use crate::data_format_nif::{NifFile, add_block, blocks_by_type, root_nodes};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, contains_text, delimited_text,
    extract_file_name, extract_file_path, same_text, trim,
};

pub struct ProcAnimSkeletonDeath {
    base: ProcBase,
    /// `edNames`.
    names_text: String,
    exact_match_checked: bool,
    names: Vec<String>,
    exact_match: bool,
}

impl ProcAnimSkeletonDeath {
    pub fn new() -> ProcAnimSkeletonDeath {
        ProcAnimSkeletonDeath {
            base: ProcBase::new(
                "Add blocks from skeleton",
                &[GameType::Tes4, GameType::Fo3, GameType::Fnv],
                &["kf"],
            ),
            names_text: "Weapon,HeadAnims".to_owned(),
            exact_match_checked: true,
            names: Vec::new(),
            exact_match: true,
        }
    }
}

/// `Elements[aPath].LinksTo` of a missing element reads through nil
/// upstream.
fn link(tree: &mut Tree, el: El, path: &str) -> R<Option<El>> {
    let element = tree.elements(el, path)?.ok_or_else(access_violation)?;
    tree.links_to(element)
}

impl Proc for ProcAnimSkeletonDeath {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.names_text = storage.get_string("sNames", &self.names_text);
        self.exact_match_checked = storage.get_bool("bExactMatch", self.exact_match_checked);
    }

    fn on_start(&mut self) -> R<()> {
        self.names = delimited_text(&self.names_text, ',')
            .iter()
            .map(|name| trim(name).to_owned())
            .collect();
        self.exact_match = self.exact_match_checked;
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        if !same_text(extract_file_name(&file.file_name), "death.kf") {
            return Ok(Vec::new());
        }

        let skeleton_file = format!("{}skeleton.nif", extract_file_path(&file.file_name));
        let skeleton_path = format!("{}{}", file.input.input_directory, skeleton_file);
        if !Path::new(&skeleton_path).is_file() {
            return Ok(Vec::new());
        }

        let mut changed = false;
        let mut nif = NifFile::new()?;
        let mut skeleton = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;

        if crate::data_format_nif::blocks_count(&mut nif.tree)? == 0 {
            return Ok(Vec::new());
        }

        let root = root_nodes(&mut nif.tree)?
            .first()
            .copied()
            .ok_or_else(access_violation)?;
        let Some(entries) = nif.tree.elements(root, "Controlled Blocks")? else {
            return Ok(Vec::new());
        };

        skeleton.load_from_file(Path::new(&skeleton_path))?;

        // going over NiNode bones in skeleton
        for bone in blocks_by_type(&mut skeleton.tree, "NiNode", false)? {
            // skip unnamed nodes
            let name = skeleton.tree.edit_values(bone, "Name")?;
            if name.is_empty() {
                continue;
            }

            // skip bones with collision
            if link(&mut skeleton.tree, bone, "Collision Object")?.is_some() {
                continue;
            }

            // skip bones with given names
            let matched = self
                .names
                .iter()
                .any(|s| (self.exact_match && same_text(&name, s)) || (!self.exact_match && contains_text(&name, s)));
            if matched {
                continue;
            }

            // check if controlled block already exists for this bone
            let mut found = false;
            for i in 0..nif.tree.count(entries) {
                let entry = nif.tree.item(entries, i)?;
                if nif.tree.edit_values(entry, "Node Name")? == name {
                    found = true;
                    break;
                }
            }
            if found {
                continue;
            }

            // adding a new controlled block
            let entry = nif.tree.add(entries)?;
            nif.tree.set_edit_values(entry, "Node Name", &name)?;
            nif.tree
                .set_native_values(entry, "Priority", crate::variant::Variant::Int(99))?;
            nif.tree
                .set_edit_values(entry, "Controller Type", "NiTransformController")?;

            // interpolator with the same transform as on a bone
            let interpolator = add_block(&mut nif.tree, "NiTransformInterpolator")?;
            let destination = nif
                .tree
                .elements(interpolator, "Transform")?
                .ok_or_else(access_violation)?;
            let source = skeleton
                .tree
                .elements(bone, "Transform")?
                .ok_or_else(access_violation)?;
            nif.tree.assign_from(destination, &mut skeleton.tree, Some(source))?;
            let index = nif.tree.index(interpolator)?;
            nif.tree
                .set_native_values(entry, "Interpolator", crate::variant::Variant::Int(i64::from(index)))?;

            changed = true;
        }

        if changed {
            let first = crate::data_format_nif::block(&mut nif.tree, 0)?;
            let count = nif.tree.count(entries);
            nif.tree.set_native_values(
                first,
                "Num Controlled Blocks",
                crate::variant::Variant::Int(i64::from(count)),
            )?;
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_exact_match() {
        let mut proc = ProcAnimSkeletonDeath::new();
        proc.on_show(&Storage::new("Addblocksfromskeleton".to_owned(), None));
        proc.on_start().unwrap();
        assert_eq!(proc.names, vec!["Weapon", "HeadAnims"]);
        assert!(proc.exact_match);
    }
}
