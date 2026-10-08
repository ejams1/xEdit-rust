// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcAddRootCollisionNode.pas

//! `Add RootCollisionNode`: adds a `RootCollisionNode` to Morrowind meshes
//! with copies of their visible triangle shapes, transformed, as the
//! collision.

use crate::data_format::{El, R, Tree};
use crate::data_format_nif::{
    NifFile, add_block, block_add_child, block_apply_transform, block_get_transform, block_is_hidden,
    block_set_transform, block_type, blocks_by_type, blocks_count, root_node,
};
use crate::nif_math::Transform;
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, access_violation};
use crate::variant::Variant;

pub struct ProcAddRootCollisionNode {
    base: ProcBase,
}

impl ProcAddRootCollisionNode {
    pub fn new() -> ProcAddRootCollisionNode {
        ProcAddRootCollisionNode {
            base: ProcBase::new("Add RootCollisionNode", &[GameType::Tes3], &["nif"]),
        }
    }
}

/// `TShapeData`: a shape's data and the transform down to it.
struct ShapeData {
    data: El,
    transform: Transform,
}

/// `CollectShapeDatas`.
fn collect_shape_datas(tree: &mut Tree, b: Option<El>, mut transform: Transform, datas: &mut Vec<ShapeData>) -> R<()> {
    let Some(b) = b else { return Ok(()) };
    // Not the transform of the root node.
    let mut t = Transform::default();
    if tree.index(b)? != 0 && block_get_transform(tree, b, &mut t)? {
        transform = transform * t;
    }
    if block_type(tree, b) == "NiTriShape" {
        if block_is_hidden(tree, b)? {
            return Ok(());
        }
        let link = tree.elements(b, "Data")?.ok_or_else(access_violation)?;
        let Some(shape_data) = tree.links_to(link)? else {
            return Ok(());
        };
        if tree.native_values(shape_data, "Num Vertices")?.to_i64()? == 0
            || tree.native_values(shape_data, "Num Triangles")?.to_i64()? == 0
        {
            return Ok(());
        }
        // A data that several nodes link to is taken once.
        if datas.iter().any(|known| known.data == shape_data) {
            return Ok(());
        }
        datas.push(ShapeData {
            data: shape_data,
            transform,
        });
    } else {
        // The children, recursively.
        let Some(children) = tree.elements(b, "Children")? else {
            return Ok(());
        };
        for i in 0..tree.count(children) {
            let item = tree.item(children, i)?;
            let child = tree.links_to(item)?;
            collect_shape_datas(tree, child, transform, datas)?;
        }
    }
    Ok(())
}

impl Proc for ProcAddRootCollisionNode {
    proc_base!();

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        if blocks_count(tree)? == 0 {
            return Ok(Vec::new());
        }
        if !blocks_by_type(tree, "RootCollisionNode", false)?.is_empty() {
            return Ok(Vec::new());
        }
        let root = root_node(tree)?;
        if block_type(tree, root) != "NiNode" {
            return Ok(Vec::new());
        }
        let mut datas = Vec::new();
        collect_shape_datas(tree, Some(root), Transform::none(), &mut datas)?;
        if datas.is_empty() {
            return Ok(Vec::new());
        }
        let rc = block_add_child(tree, root, "RootCollisionNode")?;
        tree.set_edit_values(rc, "Name", "RCN")?;
        tree.set_native_values(rc, "Flags", Variant::Int(3))?;
        for d in datas {
            let tri_shape = block_add_child(tree, rc, "NiTriShape")?;
            tree.set_native_values(tri_shape, "Flags", Variant::Int(2))?;
            block_set_transform(tree, tri_shape, &d.transform)?;
            let tri_shape_data = add_block(tree, "NiTriShapeData")?;
            let index = tree.index(tri_shape_data)?;
            tree.set_native_values(tri_shape, "Data", Variant::Int(i64::from(index)))?;
            tree.assign(tri_shape_data, Some(d.data))?;
            tree.set_native_values(tri_shape_data, "Num UV Sets", Variant::Int(0))?;
            let uv_sets = tree.elements(tri_shape_data, "UV Sets")?.ok_or_else(access_violation)?;
            tree.set_count(uv_sets, 0)?;
            tree.set_native_values(tri_shape_data, "Num Match Groups", Variant::Int(0))?;
            let match_groups = tree
                .elements(tri_shape_data, "Match Groups")?
                .ok_or_else(access_violation)?;
            tree.set_count(match_groups, 0)?;
            tree.set_native_values(tri_shape_data, "Has UV", Variant::Int(0))?;
            tree.set_native_values(tri_shape_data, "Has Vertex Colors", Variant::Int(0))?;
            block_apply_transform(tree, tri_shape, false, Default::default())?;
        }
        nif.save_to_data()
    }
}
