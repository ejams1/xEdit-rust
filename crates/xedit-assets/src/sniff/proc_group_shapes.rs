// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcGroupShapes.pas

//! `Group shapes`: groups the shapes that use the same textures under new
//! `NiNode`s, optionally splitting them where the vertices or triangles of
//! the group would overflow a word.

use xedit_io::encoding::lower_case;

use crate::data_format::{El, R, Tree};
use crate::data_format_nif::{
    NifFile, NifVersion, block_is_ni_object, block_type, blocks_count, get_unique_name, insert_block, root_nodes,
};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, extract_file_name,
};
use crate::variant::Variant;

/// `sHigh(Word)`: the limit the split mode checks.
const HIGH_WORD: i64 = 65535;

pub struct ProcGroupShapes {
    base: ProcBase,
    split_checked: bool,
    all_features_checked: bool,
    split: bool,
    all_features: bool,
}

impl ProcGroupShapes {
    pub fn new() -> ProcGroupShapes {
        ProcGroupShapes {
            base: ProcBase::new("Group shapes", GameType::ALL, &["nif"]),
            split_checked: false,
            all_features_checked: false,
            split: false,
            all_features: false,
        }
    }
}

/// `GetUsedTexture`: the textures the shape uses, lower cased; an empty
/// string when there is none.
fn get_used_texture(nif_version: NifVersion, all_features: bool, tree: &mut Tree, shape: El) -> R<String> {
    let mut result = String::new();
    if let Some(shader_property) = tree.elements(shape, "Shader Property")? {
        let Some(shader) = tree.links_to(shader_property)? else {
            return Ok(result);
        };

        // group by material file in the Name field for FO4 meshes
        if nif_version == NifVersion::Fo4
            && block_type(tree, shader) == "BSLightingShaderProperty"
            && !tree.edit_values(shader, "Name")?.is_empty()
        {
            return Ok(extract_file_name(&tree.edit_values(shader, "Name")?).to_owned());
        }

        let Some(texture_set) = tree.elements(shader, "Texture Set")? else {
            return Ok(result);
        };
        let Some(texset) = tree.links_to(texture_set)? else {
            return Ok(result);
        };
        let Some(textures) = tree.elements(texset, "Textures")? else {
            return Ok(result);
        };

        for index in 0..tree.count(textures) {
            if !result.is_empty() {
                result.push(',');
            }
            let item = tree.item(textures, index)?;
            result.push_str(extract_file_name(&tree.edit_value(item)?));
            if !all_features {
                break;
            }
        }
    } else {
        let Some(props) = tree.elements(shape, "Properties")? else {
            return Ok(result);
        };

        for index in 0..tree.count(props) {
            let prop = tree.item(props, index)?;
            let Some(prop) = tree.links_to(prop)? else {
                return Err(access_violation());
            };

            if block_type(tree, prop) == "NiTexturingProperty" {
                let Some(source) = tree.elements(prop, "Base Texture\\Source")? else {
                    return Ok(result);
                };
                let Some(texture) = tree.links_to(source)? else {
                    continue;
                };
                result = extract_file_name(&tree.edit_values(texture, "File Name")?).to_owned();
                return Ok(result);
            } else if block_type(tree, prop) == "BSShaderPPLightingProperty" {
                let Some(texture_set) = tree.elements(prop, "Texture Set")? else {
                    continue;
                };
                let Some(texset) = tree.links_to(texture_set)? else {
                    continue;
                };
                let Some(textures) = tree.elements(texset, "Textures")? else {
                    return Ok(result);
                };
                for index in 0..tree.count(textures) {
                    if !result.is_empty() {
                        result.push(',');
                    }
                    let item = tree.item(textures, index)?;
                    result.push_str(extract_file_name(&tree.edit_value(item)?));
                    if !all_features {
                        break;
                    }
                }
                return Ok(result);
            }
        }
    }
    Ok(result)
}

/// `GetVerts`.
fn get_verts(tree: &mut Tree, shape: El) -> R<i64> {
    if block_is_ni_object(tree, shape, "BSTriShape", true) {
        return tree.native_values(shape, "Num Vertices")?.to_i64();
    }
    let data = tree.elements(shape, "Data")?.ok_or_else(access_violation)?;
    match tree.links_to(data)? {
        Some(data) => tree.native_values(data, "Num Vertices")?.to_i64(),
        None => Ok(0),
    }
}

/// `GetTris`.
fn get_tris(tree: &mut Tree, shape: El) -> R<i64> {
    if block_is_ni_object(tree, shape, "BSTriShape", true) {
        return tree.native_values(shape, "Num Triangles")?.to_i64();
    }
    let data = tree.elements(shape, "Data")?.ok_or_else(access_violation)?;
    match tree.links_to(data)? {
        Some(data) => tree.native_values(data, "Num Triangles")?.to_i64(),
        None => Ok(0),
    }
}

impl Proc for ProcGroupShapes {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.split_checked = storage.get_bool("bSplit", self.split_checked);
        self.all_features_checked = storage.get_bool("bAllFeatures", self.all_features_checked);
    }

    fn on_start(&mut self) -> R<()> {
        self.split = self.split_checked;
        self.all_features = self.all_features_checked;
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;

        let changed = {
            let version = nif.tree.nif.nif_version;
            let tree = &mut nif.tree;
            if blocks_count(tree)? == 0 {
                return Ok(Vec::new());
            }

            let root = root_nodes(tree)?.first().copied().ok_or_else(access_violation)?;
            let Some(children) = tree.elements(root, "Children")? else {
                return Ok(Vec::new());
            };

            // iterate over children of root node
            let mut tokens: Vec<(String, Vec<El>)> = Vec::new();
            for index in 0..tree.count(children) {
                let child = tree.item(children, index)?;
                let Some(child) = tree.links_to(child)? else {
                    continue;
                };

                if !(block_is_ni_object(tree, child, "NiTriBasedGeom", true)
                    || block_is_ni_object(tree, child, "BSTriShape", true))
                {
                    continue;
                }

                let token = lower_case(&get_used_texture(version, self.all_features, tree, child)?);
                if token.is_empty() {
                    continue;
                }

                // check if we already have such token
                match tokens.iter_mut().find(|(known, _)| *known == token) {
                    Some((_, shapes)) => shapes.push(child),
                    None => tokens.push((token, vec![child])),
                }
            }

            // iterate over collected tokens
            let mut changed = false;
            for (token, shapes) in &tokens {
                if shapes.len() < 2 {
                    continue;
                }

                let mut node: Option<El> = None;
                let mut verts: i64 = 0;
                let mut tris: i64 = 0;

                for shape in shapes {
                    let shape = *shape;
                    if self.split {
                        let v = get_verts(tree, shape)?;
                        let t = get_tris(tree, shape)?;
                        // if NiNode exists already, check that the current shape can fit there
                        // otherwise force create new NiNode
                        if verts + v > HIGH_WORD || tris + t > HIGH_WORD {
                            node = None;
                            verts = v;
                            tris = t;
                        } else {
                            verts += v;
                            tris += t;
                        }
                    }

                    // create NiNode at the index of the current shape if doesn't exist yet
                    if node.is_none() {
                        let shape_index = tree.index(shape)?;
                        let created = insert_block(tree, shape_index, "NiNode")?;
                        let mut diffuse = extract_file_name(token.split(',').next().unwrap_or("")).to_owned();
                        if diffuse.is_empty() {
                            diffuse = "nodiffuse.dds".to_owned();
                        }
                        let unique = get_unique_name(tree, &diffuse)?;
                        tree.set_edit_values(created, "Name", &unique)?;
                        node = Some(created);
                    }
                    let node = node.unwrap();

                    let node_children = tree.elements(node, "Children")?.ok_or_else(access_violation)?;
                    let added = tree.add(node_children)?;
                    let shape_index = tree.index(shape)?;
                    tree.set_native_value(added, Variant::Int(i64::from(shape_index)))?;

                    // find the link to the current shape in root's children
                    for idx in 0..tree.count(children) {
                        let link = tree.item(children, idx)?;
                        if tree.links_to(link)? != Some(shape) {
                            continue;
                        }
                        // if it is the first shape of created NiNode then relink to it
                        if tree.count(node_children) == 1 {
                            let node_index = tree.index(node)?;
                            tree.set_native_value(link, Variant::Int(i64::from(node_index)))?;
                        } else {
                            tree.delete(children, idx)?;
                        }
                        break;
                    }
                }

                changed = true;
            }

            changed
        };

        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
