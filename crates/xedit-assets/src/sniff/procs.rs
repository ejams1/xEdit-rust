// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/frmMain.pas (FormCreate: the AddProc calls)

//! The operations of Sniff in the order of the main form, each with its
//! group. An operation whose unit is not ported yet is listed with why.

use crate::sniff::processor::Proc;
use crate::sniff::*;

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
    ported!(
        "NIF",
        "Search and replace assets",
        proc_replace_assets::ProcReplaceAssets
    ),
    ported!(
        "NIF",
        "Convert to and from JSON",
        proc_json_converter::ProcJsonConverter
    ),
    ported!("NIF", "Universal tweaker", proc_universal_tweaker::ProcUniversalTweaker),
    ported!("NIF", "Universal fixer", proc_universal_fixer::ProcUniversalFixer),
    ported!("NIF", "Apply transformation", proc_apply_transform::ProcApplyTransform),
    ported!(
        "NIF",
        "Adjust transformation",
        proc_adjust_transform::ProcAdjustTransform
    ),
    ported!("NIF", "Attach parent NiNode", proc_attach_parent::ProcAttachParent),
    pending!("NIF", "Copy geometry blocks", STEP_5),
    pending!("NIF", "Vertex color painting", STEP_5),
    pending!("NIF", "Group shapes", STEP_5),
    pending!("NIF", "Merge shapes", STEP_5),
    ported!("NIF", "Merge properties", proc_merge_properties::ProcMergeProperties),
    ported!("NIF", "Remove nodes", proc_remove_nodes::ProcRemoveNodes),
    ported!(
        "NIF",
        "Remove unused nodes",
        proc_remove_unused_nodes::ProcRemoveUnusedNodes
    ),
    ported!("NIF", "Convert block type", proc_convert_root_node::ProcConvertRootNode),
    ported!("NIF", "Unskin mesh", proc_unskin_mesh::ProcUnskinMesh),
    ported!("NIF", "Add NiLODNode", proc_add_lod_node::ProcAddLODNode),
    ported!(
        "NIF",
        "Add RootCollisionNode",
        proc_add_root_collision_node::ProcAddRootCollisionNode
    ),
    ported!("NIF", "Add bounding box", proc_add_bounding_box::ProcAddBoundingBox),
    ported!("NIF", "Set missing names", proc_set_missing_names::ProcSetMissingNames),
    ported!("Report", "Check for errors", proc_check_for_errors::ProcCheckForErrors),
    pending!("Report", "Analyze mesh", MESH_OPTIMIZE),
    ported!(
        "Report",
        "Transform information",
        proc_transform_info::ProcTransformInfo
    ),
    ported!("Report", "Havok information", proc_havok_info::ProcHavokInfo),
    ported!(
        "Report",
        "Find unwelded vertices",
        proc_unwelded_vertices::ProcUnweldedVertices
    ),
    ported!(
        "Report",
        "Find excessive draw calls",
        proc_find_draw_calls::ProcFindDrawCalls
    ),
    ported!("Report", "Find UVs", proc_find_uvs::ProcFindUVs),
    pending!(
        "Report",
        "Find textures",
        "wbDDS is ported (xedit_io::dds); the processor is phase 5 step 5"
    ),
    pending!("Animation", "Copy anim controlled blocks", STEP_5),
    ported!(
        "Animation",
        "Copy anim priorities",
        proc_copy_priorities::ProcCopyPriorities
    ),
    ported!(
        "Animation",
        "Remove controlled blocks",
        proc_remove_controlled_blocks::ProcRemoveControlledBlocks
    ),
    ported!(
        "Animation",
        "Quadratic to linear anim",
        proc_anim_quadratic_to_linear::ProcAnimQuadraticToLinear
    ),
    ported!(
        "Animation",
        "Fix 3DS exported KF",
        proc_fix_exported_kf_anim::ProcFixExportedKFAnim
    ),
    ported!("Animation", "Optimize Animations", proc_optimize_kf::ProcOptimizeKF),
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
    ported!(
        "Collision",
        "Update Havok settings",
        proc_havok_settings_update::ProcHavokSettingsUpdate
    ),
    ported!(
        "Collision",
        "Update Havok inertia",
        proc_inertia_update::ProcInertiaUpdate
    ),
    ported!(
        "Collision",
        "Update ragdoll constraint",
        proc_ragdoll_constraint_update::ProcRagdollConstraintUpdate
    ),
    ported!(
        "Collision",
        "Search for Havok material",
        proc_havok_search_material::ProcHavokSearchMaterial
    ),
    ported!(
        "Shader",
        "Update shader flags",
        proc_shader_flags_update::ProcShaderFlagsUpdate
    ),
    ported!(
        "Shader",
        "Real Time Reflections - NVSE",
        proc_walls_reflection_flag::ProcWallsReflectionFlag
    ),
    ported!(
        "Shader",
        "Vanilla Plus Particles - NVSE",
        proc_soft_particles::ProcSoftParticles
    ),
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
