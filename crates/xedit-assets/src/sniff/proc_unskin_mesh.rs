// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcUnskinMesh.pas

//! `Unskin mesh`: removes the bones and the skin instances of the
//! `NiTriBasedGeom` shapes.

use crate::data_format::{El, R, Tree};
use crate::data_format_nif::{
    NifFile, NifOptions, block_get_skin, block_is_bone, block_property_by_type, block_remove_branch,
    block_update_bounds, blocks_by_type,
};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject};
use crate::variant::Variant;

pub struct ProcUnskinMesh {
    base: ProcBase,
}

impl ProcUnskinMesh {
    pub fn new() -> ProcUnskinMesh {
        ProcUnskinMesh {
            base: ProcBase::new(
                "Unskin mesh",
                &[GameType::Tes4, GameType::Fo3, GameType::Fnv, GameType::Tes5],
                &["nif"],
            ),
        }
    }
}

/// `UnSkin`.
fn unskin(tree: &mut Tree, shape: El, changed: &mut bool) -> R<()> {
    let Some(skin) = block_get_skin(tree, shape)? else {
        return Ok(());
    };
    block_remove_branch(tree, skin, true)?;
    *changed = true;
    if let Some(shader) = block_property_by_type(tree, shape, "BSShaderProperty", true)? {
        tree.set_native_values(shader, "Shader Flags 1\\Skinned", Variant::Bool(false))?;
        *changed = true;
    }
    block_update_bounds(tree, shape)?;
    Ok(())
}

impl Proc for ProcUnskinMesh {
    proc_base!();

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        nif.tree.nif.options = NifOptions {
            collapse_link_arrays: true,
            remove_unused_strings: true,
        };
        let mut changed = false;
        nif.load_from_data(&file.get_data()?)?;
        let root = nif.root;
        let tree = &mut nif.tree;
        for b in blocks_by_type(tree, "NiNode", false)? {
            if block_is_bone(tree, b)? {
                let index = tree.index(b)?;
                tree.delete(root, index)?;
                changed = true;
            }
        }
        for b in blocks_by_type(tree, "NiTriBasedGeom", true)? {
            unskin(tree, b, &mut changed)?;
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
