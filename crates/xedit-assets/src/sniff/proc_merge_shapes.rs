// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcMergeShapes.pas

//! `Merge shapes`: merges the shapes of the `NiNode`s whose name matches
//! into one, triangulating strips and applying the transforms first.

use crate::data_format::{DfError, El, R, Tree};
use crate::data_format_nif::{
    ApplyTransformOptions, NifFile, NifOptions, NifVersion, block, block_apply_transform, block_is_ni_object,
    block_remove_branch, block_type, block_update_bounds, block_update_tangents, blocks_by_type, convert_block,
};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, contains_text, delimited_text, same_text, trim,
};
use crate::variant::Variant;

/// `sHigh(Word)`: the vertex and triangle count a node may hold.
const HIGH_WORD: i64 = 65535;

pub struct ProcMergeShapes {
    base: ProcBase,
    /// `edNames`.
    names_text: String,
    exact_match_checked: bool,
    names: Vec<String>,
    exact_match: bool,
}

impl ProcMergeShapes {
    pub fn new() -> ProcMergeShapes {
        ProcMergeShapes {
            base: ProcBase::new("Merge shapes", GameType::ALL, &["nif"]),
            names_text: ".dds".to_owned(),
            exact_match_checked: false,
            names: Vec::new(),
            exact_match: false,
        }
    }
}

/// `merge_arrays`: appends the items of `src` to `dst`. `true` when
/// nothing is left to do or when both exist.
fn merge_arrays(tree: &mut Tree, dst: Option<El>, src: Option<El>) -> R<bool> {
    // both arrays are missing, nothing to do
    if src.is_none() && dst.is_none() {
        return Ok(true);
    }
    // if either is missing, error
    let (Some(dst), Some(src)) = (dst, src) else {
        return Ok(false);
    };
    let v = tree.count(dst);
    let count = tree.count(src);
    tree.set_count(dst, v + count)?;
    for i in 0..count {
        let source = tree.item(src, i)?;
        let destination = tree.item(dst, v + i)?;
        tree.assign(destination, Some(source))?;
    }
    Ok(true)
}

/// `merge_tris`: appends the triangles of `src` to `dst`, moving their
/// indices by the vertices `dst` held.
fn merge_tris(tree: &mut Tree, dst: Option<El>, src: Option<El>, v: i64) -> R<bool> {
    let (Some(dst), Some(src)) = (dst, src) else {
        return Ok(false);
    };
    let t = tree.count(dst);
    let count = tree.count(src);
    tree.set_count(dst, t + count)?;
    for i in 0..count {
        let source = tree.item(src, i)?;
        let destination = tree.item(dst, t + i)?;
        for index in ["[0]", "[1]", "[2]"] {
            let value = tree.native_values(source, index)?.to_i64()? + v;
            tree.set_native_values(destination, index, Variant::Int(value))?;
        }
    }
    Ok(true)
}

/// `GetVerts`.
fn get_verts(tree: &mut Tree, shape: El) -> R<i64> {
    if block_is_ni_object(tree, shape, "BSTriShape", true) {
        return tree.native_values(shape, "Num Vertices")?.to_i64();
    }
    match tree.elements(shape, "Data")? {
        Some(data) => match tree.links_to(data)? {
            Some(data) => tree.native_values(data, "Num Vertices")?.to_i64(),
            None => Ok(0),
        },
        None => Ok(0),
    }
}

/// `GetTris`.
fn get_tris(tree: &mut Tree, shape: El) -> R<i64> {
    if block_is_ni_object(tree, shape, "BSTriShape", true) {
        return tree.native_values(shape, "Num Triangles")?.to_i64();
    }
    match tree.elements(shape, "Data")? {
        Some(data) => match tree.links_to(data)? {
            Some(data) => tree.native_values(data, "Num Triangles")?.to_i64(),
            None => Ok(0),
        },
        None => Ok(0),
    }
}

impl Proc for ProcMergeShapes {
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
        if self.names.is_empty() {
            return Err(DfError::new("Names field can not be empty"));
        }
        self.exact_match = self.exact_match_checked;
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
        let version = nif.tree.nif.nif_version;
        let tree = &mut nif.tree;

        for node in blocks_by_type(tree, "NiNode", true)? {
            let name = tree.edit_values(node, "Name")?;
            let matched = self
                .names
                .iter()
                .any(|s| (self.exact_match && same_text(&name, s)) || (!self.exact_match && contains_text(&name, s)));
            if !matched {
                continue;
            }

            let Some(children) = tree.elements(node, "Children")? else {
                return Ok(Vec::new());
            };

            // check for vertices and tris overflow before merging
            let mut verts = 0;
            let mut tris = 0;
            for i in 0..tree.count(children) {
                let link = tree.item(children, i)?;
                if let Some(shape) = tree.links_to(link)? {
                    verts += get_verts(tree, shape)?;
                    tris += get_tris(tree, shape)?;
                }
            }

            // just skip such nodes
            if verts > HIGH_WORD || tris > HIGH_WORD {
                continue;
            }

            let mut merged: Option<El> = None;

            // merging BSTriShape
            if matches!(version, NifVersion::Sse | NifVersion::Fo4) {
                for i in 0..tree.count(children) {
                    let link = tree.item(children, i)?;
                    let Some(shape) = tree.links_to(link)? else {
                        continue;
                    };
                    if block_type(tree, shape) != "BSTriShape" {
                        continue;
                    }

                    block_apply_transform(tree, shape, false, ApplyTransformOptions::default())?;

                    if merged.is_none() {
                        merged = Some(shape);
                        continue;
                    }
                    let merged_block = merged.unwrap();

                    let v = match tree.elements(merged_block, "Vertex Data")? {
                        Some(data) => tree.count(data),
                        None => 0,
                    };
                    let merged_data = tree.elements(merged_block, "Vertex Data")?;
                    let shape_data = tree.elements(shape, "Vertex Data")?;
                    merge_arrays(tree, merged_data, shape_data)?;

                    let merged_tris = tree.elements(merged_block, "Triangles")?;
                    let shape_tris = tree.elements(shape, "Triangles")?;
                    merge_tris(tree, merged_tris, shape_tris, i64::from(v))?;

                    let vertices = match tree.elements(merged_block, "Vertex Data")? {
                        Some(data) => tree.count(data),
                        None => 0,
                    };
                    let triangles = match tree.elements(merged_block, "Triangles")? {
                        Some(data) => tree.count(data),
                        None => 0,
                    };
                    tree.set_native_values(merged_block, "Num Vertices", Variant::Int(i64::from(vertices)))?;
                    tree.set_native_values(merged_block, "Num Triangles", Variant::Int(i64::from(triangles)))?;

                    tree.set_native_value(link, Variant::Int(-1))?;
                    block_remove_branch(tree, shape, false)?;
                    changed = true;
                }
            }

            // merging NiTriShape and NiTriStrips
            if matches!(
                version,
                NifVersion::Tes3 | NifVersion::Tes4 | NifVersion::Fo3 | NifVersion::Tes5
            ) {
                for i in 0..tree.count(children) {
                    let link = tree.item(children, i)?;
                    let Some(mut shape) = tree.links_to(link)? else {
                        continue;
                    };
                    if !block_is_ni_object(tree, shape, "NiTriBasedGeom", true) {
                        continue;
                    }

                    // triangulate if strips
                    if block_type(tree, shape) == "NiTriStrips" {
                        let j = tree.index(shape)?;
                        convert_block(tree, j, "NiTriShape")?;
                        shape = block(tree, j)?;
                    }

                    block_apply_transform(tree, shape, false, ApplyTransformOptions::default())?;

                    let toremove = shape; // store shape block to remove later
                    let Some(data) = tree
                        .elements(shape, "Data")?
                        .map(|data| tree.links_to(data))
                        .transpose()?
                        .flatten()
                    else {
                        continue;
                    };
                    let mut shape = data;

                    // triangulate if strips data
                    if block_type(tree, shape) == "NiTriStripsData" {
                        let j = tree.index(shape)?;
                        convert_block(tree, j, "NiTriShapeData")?;
                        shape = block(tree, j)?;
                    }

                    if merged.is_none() {
                        merged = Some(shape);
                        continue;
                    }
                    let merged_block = merged.unwrap();

                    let v = match tree.elements(merged_block, "Vertices")? {
                        Some(data) => tree.count(data),
                        None => 0,
                    };

                    for field in ["Vertices", "Normals", "Tangents", "Bitangents", "Vertex Colors"] {
                        let merged_data = tree.elements(merged_block, field)?;
                        let shape_data = tree.elements(shape, field)?;
                        merge_arrays(tree, merged_data, shape_data)?;
                    }

                    let merged_uv = tree.elements(merged_block, "UV Sets")?;
                    let shape_uv = tree.elements(shape, "UV Sets")?;
                    if let (Some(merged_uv), Some(shape_uv)) = (merged_uv, shape_uv)
                        && tree.count(merged_uv) > 0
                        && tree.count(shape_uv) > 0
                    {
                        let destination = tree.item(merged_uv, 0)?;
                        let source = tree.item(shape_uv, 0)?;
                        merge_arrays(tree, Some(destination), Some(source))?;
                    }

                    let merged_tris = tree.elements(merged_block, "Triangles")?;
                    let shape_tris = tree.elements(shape, "Triangles")?;
                    merge_tris(tree, merged_tris, shape_tris, i64::from(v))?;

                    let vertices = match tree.elements(merged_block, "Vertices")? {
                        Some(data) => tree.count(data),
                        None => 0,
                    };
                    let triangles = match tree.elements(merged_block, "Triangles")? {
                        Some(data) => tree.count(data),
                        None => 0,
                    };
                    tree.set_native_values(merged_block, "Num Vertices", Variant::Int(i64::from(vertices)))?;
                    tree.set_native_values(merged_block, "Num Triangles", Variant::Int(i64::from(triangles)))?;

                    tree.set_native_value(link, Variant::Int(-1))?;
                    block_remove_branch(tree, toremove, false)?;
                    changed = true;
                }
            }

            if let Some(merged) = merged {
                block_update_bounds(tree, merged)?;
                if version == NifVersion::Tes4 {
                    block_update_tangents(tree, merged, false)?;
                }
            }
        }

        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_split_on_commas_and_trim() {
        let mut proc = ProcMergeShapes::new();
        assert_eq!(proc.names_text, ".dds");
        proc.names_text = " Scene Root , Bip01 ".to_owned();
        proc.on_start().unwrap();
        assert_eq!(proc.names, vec!["Scene Root", "Bip01"]);
        assert!(!proc.exact_match);

        proc.names_text = String::new();
        assert_eq!(proc.on_start().unwrap_err().0, "Names field can not be empty");
    }
}
