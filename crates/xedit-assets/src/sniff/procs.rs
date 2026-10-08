// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/frmMain.pas (FormCreate: the AddProc calls)

//! The operations of Sniff in the order of the main form, each with its
//! group. An operation whose unit is not ported yet is listed with why.

use crate::sniff::processor::Proc;
use crate::sniff::{
    proc_adjust_transform, proc_apply_transform, proc_attach_parent, proc_convert_root_node, proc_fix_exported_kf_anim,
    proc_json_converter, proc_remove_nodes, proc_remove_unused_nodes, proc_replace_assets, proc_set_missing_names,
    proc_tangents, proc_universal_fixer, proc_universal_tweaker, proc_unskin_mesh, proc_update_bounds,
};

/// An operation of the main form.
pub struct ProcEntry {
    pub group: &'static str,
    pub title: &'static str,
    /// The constructor, or `None` when the unit is not ported yet.
    pub create: Option<fn() -> Box<dyn Proc>>,
    /// Why the unit is not ported, for one that is not.
    pub pending: &'static str,
}

macro_rules! ported {
    ($group:expr, $title:expr, $ty:path) => {
        ProcEntry {
            group: $group,
            title: $title,
            create: Some(|| Box::new(<$ty>::new())),
            pending: "",
        }
    };
}

macro_rules! pending {
    ($group:expr, $title:expr, $why:expr) => {
        ProcEntry {
            group: $group,
            title: $title,
            create: None,
            pending: $why,
        }
    };
}

/// Not ported yet, for step 5 of phase 5.
const STEP_5: &str = "not ported yet (phase 5 step 5)";
/// Needs `wbMeshOptimize`, which is phase 5 step 6.
const MESH_OPTIMIZE: &str = "needs wbMeshOptimize (phase 5 step 6)";

/// `FormCreate`: the operations in the order of `AddProc`.
pub const PROCS: &[ProcEntry] = &[
    ported!("NIF", "Update tangents and binormals", proc_tangents::ProcTangents),
    ported!("NIF", "Update bounds", proc_update_bounds::ProcUpdateBounds),
    pending!("NIF", "Optimize mesh", MESH_OPTIMIZE),
    ported!("NIF", "Search and replace assets", proc_replace_assets::ProcReplaceAssets),
    ported!("NIF", "Convert to and from JSON", proc_json_converter::ProcJsonConverter),
    ported!("NIF", "Universal tweaker", proc_universal_tweaker::ProcUniversalTweaker),
    ported!("NIF", "Universal fixer", proc_universal_fixer::ProcUniversalFixer),
    ported!("NIF", "Apply transformation", proc_apply_transform::ProcApplyTransform),
    ported!("NIF", "Adjust transformation", proc_adjust_transform::ProcAdjustTransform),
    ported!("NIF", "Attach parent NiNode", proc_attach_parent::ProcAttachParent),
    pending!("NIF", "Copy geometry blocks", STEP_5),
    pending!("NIF", "Vertex color painting", STEP_5),
    pending!("NIF", "Group shapes", STEP_5),
    pending!("NIF", "Merge shapes", STEP_5),
    pending!("NIF", "Merge properties", STEP_5),
    ported!("NIF", "Remove nodes", proc_remove_nodes::ProcRemoveNodes),
    ported!("NIF", "Remove unused nodes", proc_remove_unused_nodes::ProcRemoveUnusedNodes),
    ported!("NIF", "Convert block type", proc_convert_root_node::ProcConvertRootNode),
    ported!("NIF", "Unskin mesh", proc_unskin_mesh::ProcUnskinMesh),
    pending!("NIF", "Add NiLODNode", STEP_5),
    pending!("NIF", "Add RootCollisionNode", STEP_5),
    pending!("NIF", "Add bounding box", STEP_5),
    ported!("NIF", "Set missing names", proc_set_missing_names::ProcSetMissingNames),
    pending!("Report", "Check for errors", STEP_5),
    pending!("Report", "Analyze mesh", MESH_OPTIMIZE),
    pending!("Report", "Transform information", STEP_5),
    pending!("Report", "Havok information", STEP_5),
    pending!("Report", "Find unwelded vertices", STEP_5),
    pending!("Report", "Find excessive draw calls", STEP_5),
    pending!("Report", "Find UVs", STEP_5),
    pending!("Report", "Find textures", "needs wbDDS (phase 5 step 2)"),
    pending!("Animation", "Copy anim controlled blocks", STEP_5),
    pending!("Animation", "Copy anim priorities", STEP_5),
    pending!("Animation", "Remove controlled blocks", STEP_5),
    pending!("Animation", "Quadratic to linear anim", STEP_5),
    ported!("Animation", "Fix 3DS exported KF", proc_fix_exported_kf_anim::ProcFixExportedKFAnim),
    pending!("Animation", "Optimize Animations", STEP_5),
    pending!("Animation", "Add headtracking anim", STEP_5),
    pending!("Animation", "Add facial anim", STEP_5),
    pending!("Animation", "Add NiTransformData", STEP_5),
    pending!("Animation", "Weijiesen's blow up thing", STEP_5),
    pending!("Animation", "Add blocks from skeleton", STEP_5),
    pending!(
        "Collision",
        "Update MOPP code",
        "calls NifMopp.dll (Havok's MOPP builder), which the port does not have"
    ),
    pending!("Collision", "Update Havok settings", STEP_5),
    pending!("Collision", "Update Havok inertia", STEP_5),
    pending!("Collision", "Update ragdoll constraint", STEP_5),
    pending!("Collision", "Search for Havok material", STEP_5),
    pending!("Shader", "Update shader flags", STEP_5),
    pending!("Shader", "Real Time Reflections - NVSE", STEP_5),
    pending!("Shader", "Vanilla Plus Particles - NVSE", STEP_5),
];

/// The ported operations, in order, with their groups.
pub fn all() -> Vec<Box<dyn Proc>> {
    PROCS
        .iter()
        .filter_map(|entry| {
            let mut proc = entry.create?();
            debug_assert_eq!(proc.base().title, entry.title);
            proc.base_mut().group = entry.group;
            Some(proc)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_match_the_units() {
        for entry in PROCS {
            if let Some(create) = entry.create {
                assert_eq!(create().base().title, entry.title);
            } else {
                assert!(!entry.pending.is_empty());
            }
        }
        assert_eq!(PROCS.len(), 50);
    }
}
