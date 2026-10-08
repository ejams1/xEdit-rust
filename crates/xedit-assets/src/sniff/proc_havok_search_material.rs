// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcHavokSearchMaterial.pas

//! `Search for Havok material`: reports the Havok shapes of a material, or
//! replaces it with another, optionally leaving the collision of the root
//! node.

use crate::data_format::{DfError, El, R, Tree};
use crate::data_format_nif::{
    NifFile, block, block_is_ni_object, block_referenced_by, block_type, blocks_count, nifblk, root_nodes,
};
use crate::data_format_nif_types::{wb_fallout3_havok_material, wb_oblivion_havok_material, wb_skyrim_havok_material};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, ansi_same_text,
};

pub struct ProcHavokSearchMaterial {
    base: ProcBase,
    /// The game of the radio buttons: 0 Oblivion, 1 Fallout 3, 2 Skyrim.
    game: i32,
    /// `chkSkipRoot`.
    skip_root_checked: bool,
    /// The texts of `cmbSearch` and `cmbReplace`.
    search_text: String,
    replace_text: String,
    material_search: String,
    material_replace: String,
    skip_root: bool,
}

impl ProcHavokSearchMaterial {
    pub fn new() -> ProcHavokSearchMaterial {
        ProcHavokSearchMaterial {
            base: ProcBase::new(
                "Search for Havok material",
                &[
                    GameType::Tes4,
                    GameType::Fo3,
                    GameType::Fnv,
                    GameType::Tes5,
                    GameType::Sse,
                ],
                &["nif"],
            ),
            game: 2,
            skip_root_checked: false,
            search_text: String::new(),
            replace_text: String::new(),
            material_search: String::new(),
            material_replace: String::new(),
            skip_root: false,
        }
    }
}

/// `slMaterial`: the materials of the three games, sorted, after an empty
/// one.
fn materials() -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    for def in [
        wb_oblivion_havok_material("", "", &[]),
        wb_fallout3_havok_material("", "", &[]),
        wb_skyrim_havok_material("", "", &[]),
    ] {
        list.extend(def.values_map().iter().map(|(_, name)| name.clone()));
    }
    // `TStringList.Sort` compares with `AnsiCompareText`.
    list.sort_by_key(|name| name.to_uppercase());
    list.insert(0, String::new());
    list
}

/// `rbTES5Click(nil)`: the items of a combo box for the game and the
/// filter text.
fn filtered(materials: &[String], prefix: &str, filter: &str) -> Vec<String> {
    let filter = filter.trim_matches(|c: char| c <= ' ').to_uppercase();
    materials
        .iter()
        .filter(|name| name.is_empty() || (name.starts_with(prefix) && (filter.is_empty() || name.contains(&filter))))
        .cloned()
        .collect()
}

/// `GetTarget`: the node a collision belongs to, through the first block
/// that refers to the block.
fn get_target(tree: &mut Tree, b: El) -> R<Option<El>> {
    if block_is_ni_object(tree, b, "bhkCollisionObject", true) {
        let link = tree.elements(b, "Target")?.ok_or_else(access_violation)?;
        return tree.links_to(link);
    }
    if let Some(&reference) = block_referenced_by(tree, b)?.first() {
        let referrer = nifblk(tree, reference).ok_or_else(access_violation)?;
        return get_target(tree, referrer);
    }
    Ok(None)
}

/// `UpdateField`.
fn update_field(tree: &mut Tree, el: Option<El>, value: &str, changed: &mut bool) -> R<()> {
    let Some(el) = el else { return Ok(()) };
    if value.is_empty() {
        return Ok(());
    }
    if tree.edit_value(el)? != value {
        tree.set_edit_value(el, value)?;
        *changed = true;
    }
    Ok(())
}

impl Proc for ProcHavokSearchMaterial {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.game = match storage.get_integer("iGame", 2) {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        self.skip_root_checked = storage.get_bool("bSkipRoot", false);
        let filter_search = storage.get_string("sFilterSearch", "");
        let filter_replace = storage.get_string("sFilterReplace", "");
        let prefix = ["OB_", "FO_", "SKY_"][self.game as usize];
        let materials = materials();
        let search_items = filtered(&materials, prefix, &filter_search);
        let replace_items = filtered(&materials, prefix, &filter_replace);
        // The first item (empty) unless the setting is an item.
        let pick = |items: &[String], wanted: &str| {
            items
                .iter()
                .find(|item| ansi_same_text(item, wanted))
                .cloned()
                .unwrap_or_default()
        };
        self.search_text = pick(&search_items, &storage.get_string("sMaterialSearch", ""));
        self.replace_text = pick(&replace_items, &storage.get_string("sMaterialReplace", ""));
    }

    fn on_start(&mut self) -> R<()> {
        self.material_search = self.search_text.clone();
        self.material_replace = self.replace_text.clone();
        self.skip_root = self.skip_root_checked;
        self.base.no_output = self.material_replace.is_empty();
        if self.material_search == self.material_replace && !self.material_search.is_empty() {
            return Err(DfError::new("Searched and replacing materials must be different"));
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut log: Vec<String> = Vec::new();
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        // `RootNode` reads past an empty array for a file without blocks.
        let root = *root_nodes(tree)?.first().ok_or_else(access_violation)?;
        for i in 0..blocks_count(tree)? {
            let b = block(tree, i)?;
            if matches!(
                block_type(tree, b),
                "hkPackedNiTriStripsData" | "bhkCompressedMeshShapeData"
            ) {
                if self.skip_root && get_target(tree, b)? == Some(root) {
                    continue;
                }
                let sub_shapes = match tree.elements(b, "Sub Shapes")? {
                    Some(sub_shapes) => Some(sub_shapes),
                    None => tree.elements(b, "Chunk Materials")?,
                };
                let Some(sub_shapes) = sub_shapes else { continue };
                for j in 0..tree.count(sub_shapes) {
                    let sub_shape = tree.item(sub_shapes, j)?;
                    let material = tree.edit_values(sub_shape, "Material")?;
                    if !self.material_search.is_empty() && material != self.material_search {
                        continue;
                    }
                    if self.material_replace.is_empty() {
                        log.push(format!("\t{}: {material}", tree.path(sub_shape)?));
                    } else {
                        let el = tree.elements(sub_shape, "Material")?;
                        update_field(tree, el, &self.material_replace, &mut changed)?;
                    }
                }
            } else if block_is_ni_object(tree, b, "bhkShape", true) {
                if self.skip_root && get_target(tree, b)? == Some(root) {
                    continue;
                }
                let material = tree.edit_values(b, "Material")?;
                if material.is_empty() {
                    continue;
                }
                if !self.material_search.is_empty() && material != self.material_search {
                    continue;
                }
                if self.material_replace.is_empty() {
                    log.push(format!("\t{}: {material}", tree.name(b)?));
                } else {
                    let el = tree.elements(b, "Material")?;
                    update_field(tree, el, &self.material_replace, &mut changed)?;
                }
            }
        }
        if !log.is_empty() {
            log.insert(0, file.file_name.clone());
            log.push(String::new());
            ctx.add_messages(log);
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
