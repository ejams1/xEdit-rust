// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcAnimQuadraticToLinear.pas

//! `Quadratic to linear anim`: changes the quadratic translation and
//! rotation keys of the controlled blocks of the names (or of the other
//! names) to linear ones.

use crate::data_format::{DfError, R};
use crate::data_format_nif::{NifFile, blocks_count, root_node};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, contains_text, delimited_text,
    same_text, trim,
};

/// The names of a controlled block match the list of names
/// (`bMatched xor fNotMatching`).
pub(crate) fn names_match(name: &str, names: &[String], exact_match: bool, not_matching: bool) -> bool {
    let matched = names
        .iter()
        .any(|s| (exact_match && same_text(name, s)) || (!exact_match && contains_text(name, s)));
    matched ^ not_matching
}

/// The names of `edNames`, trimmed; none is an error.
pub(crate) fn split_names(text: &str) -> R<Vec<String>> {
    let names: Vec<String> = delimited_text(text, ',')
        .iter()
        .map(|name| trim(name).to_owned())
        .collect();
    if names.is_empty() {
        return Err(DfError::new("Names field can not be empty"));
    }
    Ok(names)
}

pub struct ProcAnimQuadraticToLinear {
    base: ProcBase,
    names_text: String,
    exact_match_checked: bool,
    not_matching_checked: bool,
    names: Vec<String>,
    exact_match: bool,
    not_matching: bool,
}

impl ProcAnimQuadraticToLinear {
    pub fn new() -> ProcAnimQuadraticToLinear {
        ProcAnimQuadraticToLinear {
            base: ProcBase::new(
                "Quadratic to linear anim",
                &[GameType::Tes4, GameType::Fo3, GameType::Fnv],
                &["kf"],
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

impl Proc for ProcAnimQuadraticToLinear {
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
        if blocks_count(tree)? == 0 {
            return Ok(Vec::new());
        }
        let root = root_node(tree)?;
        let Some(entries) = tree.elements(root, "Controlled Blocks")? else {
            return Ok(Vec::new());
        };
        for i in (0..tree.count(entries)).rev() {
            let entry = tree.item(entries, i)?;
            let name = tree.edit_values(entry, "Node Name")?;
            if !names_match(&name, &self.names, self.exact_match, self.not_matching) {
                continue;
            }
            let link = tree.elements(entry, "Interpolator")?.ok_or_else(access_violation)?;
            let Some(interpolator) = tree.links_to(link)? else {
                continue;
            };
            let Some(data_link) = tree.elements(interpolator, "Data")? else {
                continue;
            };
            let Some(transform_data) = tree.links_to(data_link)? else {
                continue;
            };
            if let Some(interpolation) = tree.elements(transform_data, "Translations\\Interpolation")?
                && tree.edit_value(interpolation)? == "QUADRATIC_KEY"
            {
                tree.set_edit_value(interpolation, "LINEAR_KEY")?;
                changed = true;
            }
            if let Some(rotations) = tree.elements(transform_data, "XYZ Rotations")? {
                for j in 0..tree.count(rotations) {
                    let rotation = tree.item(rotations, j)?;
                    if let Some(interpolation) = tree.elements(rotation, "Interpolation")?
                        && tree.edit_value(interpolation)? == "QUADRATIC_KEY"
                    {
                        tree.set_edit_value(interpolation, "LINEAR_KEY")?;
                        changed = true;
                    }
                }
            }
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
