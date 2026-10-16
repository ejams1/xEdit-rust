// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/frmMain.pas (FormCreate: the AddProc calls)

//! The operations of Sniff in the order of the main form, each with its
//! group. `ProcCollapseLinksArrays.pas` has no entry: `frmMain.pas` does
//! not use the unit, so the released Sniff has no such operation.

use crate::sniff::processor::Proc;
use crate::sniff::*;

/// An operation of the main form.
pub struct ProcEntry {
    pub group: &'static str,
    pub title: &'static str,
    /// The constructor. `None` is reserved for a unit that is not ported;
    /// every operation of the 4.1.5q main form is ported now.
    pub create: Option<fn() -> Box<dyn Proc>>,
    /// Why the unit is not ported; empty for every entry now, and the field
    /// stays for the lists that report it.
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

/// `FormCreate`: the operations in the order of `AddProc`.
pub const PROCS: &[ProcEntry] = &[
    ported!("NIF", "Update tangents and binormals", proc_tangents::ProcTangents),
    ported!("NIF", "Update bounds", proc_update_bounds::ProcUpdateBounds),
    ported!("NIF", "Optimize mesh", proc_optimize::ProcOptimize),
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
    ported!(
        "NIF",
        "Copy geometry blocks",
        proc_copy_geometry_blocks::ProcCopyGeometryBlocks
    ),
    ported!("NIF", "Vertex color painting", proc_vertex_paint::ProcVertexPaint),
    ported!("NIF", "Group shapes", proc_group_shapes::ProcGroupShapes),
    ported!("NIF", "Merge shapes", proc_merge_shapes::ProcMergeShapes),
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
    ported!("Report", "Analyze mesh", proc_analyze_mesh::ProcAnalyzeMesh),
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
    ported!("Report", "Find textures", proc_find_textures::ProcFindTextures),
    ported!(
        "Animation",
        "Copy anim controlled blocks",
        proc_copy_controlled_blocks::ProcCopyControlledBlocks
    ),
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
    ported!(
        "Animation",
        "Add headtracking anim",
        proc_add_headtracking_anim::ProcAddHeadtrackingAnim
    ),
    ported!("Animation", "Add facial anim", proc_add_facial_anim::ProcAddFacialAnim),
    ported!("Animation", "Add NiTransformData", proc_jam_anim::ProcJamAnim),
    ported!(
        "Animation",
        "Weijiesen's blow up thing",
        proc_wei_explosion::ProcWeiExplosion
    ),
    ported!(
        "Animation",
        "Add blocks from skeleton",
        proc_anim_skeleton_death::ProcAnimSkeletonDeath
    ),
    ported!("Collision", "Update MOPP code", proc_mopp_update::ProcMoppUpdate),
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
