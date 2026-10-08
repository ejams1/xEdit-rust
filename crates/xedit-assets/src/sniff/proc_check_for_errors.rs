// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcCheckForErrors.pas

//! `Check for errors`: a list of checks of meshes and textures that report
//! what would break or slow the game; each check is a setting of its own,
//! named after the check. The texture checks (`CheckDDS`,
//! `CheckSSEDdsFormat`) read the DDS header with `wbDDS`, which is phase 5
//! step 2: a DDS file fails with a message saying so while they are on.

use crate::data_format::{DfError, El, R, Tree, df_float_to_str};
use crate::data_format_nif::{
    NifFile, NifVersion, TES4_TANGENTS_EXTRA_DATA_NAME, block, block_by_name, block_by_type, block_children_by_type,
    block_extra_data_by_name, block_extra_data_by_type, block_get_collision, block_get_controller, block_get_skin,
    block_get_tex_coord, block_is_dynamic_rigid_body, block_is_editor_marker, block_is_hidden, block_is_ni_object,
    block_property_by_type, block_referenced_by, block_refs, block_strings, block_type, blocks_by_type, blocks_count,
    detect_bsx_flags, footer, get_controlled_block_name, get_link_arrays, header, nifblk, root_nodes,
};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, ansi_same_text, extract_file_ext,
    same_text, same_value, same_value_single,
};

/// A check (`TCheckProcedure`): the file, the loaded mesh and the log.
type CheckProc = fn(&ProcFileObject, &mut NifFile, &mut Vec<String>) -> R<()>;

/// `TCheck`.
struct Check {
    name: &'static str,
    #[allow(dead_code)]
    group: &'static str,
    extensions: &'static [&'static str],
    #[allow(dead_code)]
    comment: &'static str,
    /// The check, or `None` for a texture check, which needs `wbDDS`.
    proc: Option<CheckProc>,
    active: bool,
}

impl Check {
    /// `DoesExtension`.
    fn does_extension(&self, extension: &str) -> bool {
        self.extensions.iter().any(|known| same_text(known, extension))
    }
}

const NIF: &[&str] = &[".nif"];
const NIF_KF: &[&str] = &[".nif", ".kf"];
const DDS: &[&str] = &[".dds"];

/// The checks in the order of the constructor, with their default state.
const CHECKS: &[Check] = &[
    Check {
        name: "Invalid string index",
        group: "Meshes",
        extensions: NIF_KF,
        comment: "String index used in blocks for meshes with strings table (Fallout 3 and later games) is out of range, always crashes the game",
        proc: Some(check_string_index),
        active: true,
    },
    Check {
        name: "Invalid blocks order",
        group: "Meshes",
        extensions: NIF_KF,
        comment: "Wrong order of bhkCollisionObject children when child has larger index than its parent, always crashes the game",
        proc: Some(check_blocks_order),
        active: true,
    },
    Check {
        name: "Unused blocks",
        group: "Meshes",
        extensions: NIF,
        comment: "Multiple root nodes or blocks not referenced from the root scenegraph, could crash the game",
        proc: Some(check_unused_blocks),
        active: true,
    },
    Check {
        name: "Repeated NiNode childen names",
        group: "Meshes",
        extensions: NIF,
        comment: "Invalid names or the same named blocks (or the same block) is used several times in NiNode children, might cause issues or even crash the game depending on usage context",
        proc: Some(check_invalid_repeated_children_names),
        active: true,
    },
    Check {
        name: "Wrong link types",
        group: "Meshes",
        extensions: NIF,
        comment: "References to blocks of wrong type. Always crashes the game",
        proc: Some(check_wrong_link_types),
        active: true,
    },
    Check {
        name: "Invalid array links",
        group: "Meshes",
        extensions: NIF,
        comment: "Links in arrays (children, extradatas, properties, etc.) are either empty, point to nonexisting blocks or repeated. Could crash the game",
        proc: Some(check_invalid_array_links),
        active: true,
    },
    Check {
        name: "Invalid geometry",
        group: "Meshes",
        extensions: NIF,
        comment: "Triangles or strips reference invalid vertices. Unused vertices in geometry. Duplicate vertices in BSTriShape. Multiple strips in NiTriStripsData.",
        proc: Some(check_geometry),
        active: true,
    },
    Check {
        name: "Hardcoded block names",
        group: "Meshes",
        extensions: NIF,
        comment: "Some blocks must have specific name to work properly (BSX for BSXFlags, INV for BsInvMarker, etc.), \"Weapon\" nodes in non-skeletons, [TES4] unnamed NiMaterialProperty",
        proc: Some(check_hardcoded_block_names),
        active: true,
    },
    Check {
        name: "Collision Havok issues",
        group: "Meshes",
        extensions: NIF,
        comment: "Moveable collision has zero mass or uses inertia system without inertia tensor matrix set (will break the physics not only for that object, but other objects using totally different meshes as well), Havok layer and motion settings",
        proc: Some(check_collision),
        active: true,
    },
    Check {
        name: "Collision MOPP issues",
        group: "Meshes",
        extensions: NIF,
        comment: "Badly optimized MOPP collision using high poly shapes",
        proc: Some(check_collision_mopp),
        active: true,
    },
    Check {
        name: "Check BSXFlags",
        group: "Meshes",
        extensions: NIF,
        comment: "Check for invalid BSXFlags: Animated, Havok, Ragdoll, Complex, Addon, Editor Marker and Dynamic. Emittance flag is checked by \"Invalid shader types and flags\". Complex and Articulated affect grabbing behaviour only",
        proc: Some(check_bsx_flags),
        active: true,
    },
    Check {
        name: "Check consistency flags",
        group: "Meshes",
        extensions: NIF,
        comment: "Check for invalid consistency flags value. CT_MUTABLE when shape is controlled by NiGeomMorpherController or NiUVController, CT_STATIC for the rest. CT_VOLATILE isn't used. Affects performance",
        proc: Some(check_consistency_flags),
        active: true,
    },
    Check {
        name: "Check texture set slots",
        group: "Meshes",
        extensions: NIF,
        comment: "Check for absolute paths and invalid combinations of textures in BSShaderTextureSet",
        proc: Some(check_texture_set_slots),
        active: true,
    },
    Check {
        name: "Check NiAlphaProperty",
        group: "Meshes",
        extensions: NIF,
        comment: "Check for enabled Blending in NiAlphaProperty except NoLighting shader",
        proc: Some(check_ni_alpha_property),
        active: false,
    },
    Check {
        name: "Invalid shader types and flags",
        group: "Meshes",
        extensions: NIF,
        comment: "Check for invalid combinations of shader type and shader flags: environment mapping, eye envmapping, glow, external emittance, glow + treeanim. etc. Could crash the game",
        proc: Some(check_shader_type_flags),
        active: true,
    },
    Check {
        name: "Particle system checks",
        group: "Meshes",
        extensions: NIF,
        comment: "Check for the invalid Modifier Name in descendants of NiPSysModifierCtlr, too long Life Span in NiPSysEmitter, Emitter nodes without NiParticleSystem, mesh emitters without Particle Data. Could crash the game",
        proc: Some(check_particle_system),
        active: true,
    },
    Check {
        name: "Invalid Target field",
        group: "Meshes",
        extensions: NIF,
        comment: "Check for the invalid Target field in NiCollisionObject, bhkCompressedMeshShape, NiTimeController. Check for invalid node names in NiControllerSequence",
        proc: Some(check_target_field),
        active: true,
    },
    Check {
        name: "Animation stop time",
        group: "Meshes",
        extensions: NIF_KF,
        comment: "Check for the incorrect End key not matching the animation Stop time. Could cause animation issues",
        proc: Some(check_anim_stop_time),
        active: true,
    },
    Check {
        name: "Skinning issues",
        group: "Meshes",
        extensions: NIF,
        comment: "BSDismemberSkinInstance and Body Parts checks, missing Skin in BSDynamicTriShape, [TES5/SSE] disrepancies between _0 and _1 morph models",
        proc: Some(check_skinning_issues),
        active: true,
    },
    Check {
        name: "Miscellaneous checks",
        group: "Meshes",
        extensions: NIF_KF,
        comment: "Root node is a NiNode/NiSequence descendant and the first block, Invalid subshapes in bhkListShape, [TES4] Tangents size not matching the vertices count, Unsupported NiSpecularPropertry in post Oblivion meshes",
        proc: Some(check_miscellaneous),
        active: true,
    },
    Check {
        name: "Check vertex colors",
        group: "Meshes",
        extensions: NIF,
        comment: "Check for alpha < 1.0 but missing Vertex_Alpha shader flag, possibly redundant all white vertex colors except for leaf animations and parallax, HDR vertex colors (outside of 0..1 range) which sometimes are not intended and lead to rendering issues",
        proc: Some(check_vertex_colors),
        active: true,
    },
    Check {
        name: "Clamped tiling UVs",
        group: "Meshes",
        extensions: NIF,
        comment: "Check for tiling UVs outside of 0..1 range in CLAMP mode. Causes texture stretching",
        proc: Some(check_uvs),
        active: false,
    },
    Check {
        name: "Optional checks",
        group: "Meshes",
        extensions: NIF,
        comment: "Potential false positives, could be done on purpose: Empty shader flags, Envmap + Light_fade flags and 5th + 6th slots in textureset for BSShaderPPLightingProperty",
        proc: Some(check_optional),
        active: false,
    },
    Check {
        name: "Repeated denegerate tris in strips",
        group: "Meshes",
        extensions: NIF,
        comment: "Check for strips with repeated degenerate triangles",
        proc: Some(check_strips_degenerate),
        active: false,
    },
    Check {
        name: "Invalid texture size or format",
        group: "Textures",
        extensions: DDS,
        comment: "Texture size is not power of 2 or unsupported DXGI format, likely to crash the game",
        proc: None,
        active: true,
    },
    Check {
        name: "Unsupported mesh formats",
        group: "Skyrim SE",
        extensions: NIF,
        comment: "Unsupported nif blocks which crash Skyrim SE: NiTriStrips, stripified NiSkipPartition and bhkMultiSphereShape",
        proc: Some(check_sse_nif_format),
        active: false,
    },
    Check {
        name: "Unsupported texture formats",
        group: "Skyrim SE",
        extensions: DDS,
        comment: "Uncompressed formats which crash Skyrim SE in Windows 7: R5G6B5, A1R5G5B5, A4R4G4B4 and other reduced bits formats",
        proc: None,
        active: false,
    },
];

pub struct ProcCheckForErrors {
    base: ProcBase,
    /// Whether each check of `CHECKS` is on.
    active: Vec<bool>,
    load_nif: bool,
    load_dds: bool,
}

impl ProcCheckForErrors {
    pub fn new() -> ProcCheckForErrors {
        let mut base = ProcBase::new(
            "Check for errors",
            &[
                GameType::Tes4,
                GameType::Fo3,
                GameType::Fnv,
                GameType::Tes5,
                GameType::Sse,
                GameType::Fo4,
            ],
            &["nif", "kf", "dds"],
        );
        base.no_output = true;
        ProcCheckForErrors {
            base,
            active: CHECKS.iter().map(|check| check.active).collect(),
            load_nif: false,
            load_dds: false,
        }
    }
}

// ---- helpers ----

fn el(tree: &mut Tree, element: El, path: &str) -> R<El> {
    tree.elements(element, path)?.ok_or_else(access_violation)
}

/// `Elements[aPath].LinksTo`, through nil when the element is missing.
fn link(tree: &mut Tree, element: El, path: &str) -> R<Option<El>> {
    let element = el(tree, element, path)?;
    tree.links_to(element)
}

fn flag(tree: &mut Tree, element: El, path: &str) -> R<bool> {
    tree.native_values(element, path)?.to_bool()
}

fn name(tree: &mut Tree, element: El) -> R<String> {
    tree.name(element)
}

fn items(tree: &mut Tree, element: El) -> R<Vec<El>> {
    (0..tree.count(element)).map(|i| tree.item(element, i)).collect()
}

fn version(tree: &Tree) -> NifVersion {
    tree.nif.nif_version
}

/// `RootNode`: reads past an empty array when there are no blocks.
fn root_node(tree: &mut Tree) -> R<El> {
    root_nodes(tree)?.first().copied().ok_or_else(access_violation)
}

/// The block that holds a reference (`while not (refblock is
/// TwbNifBlock) do refblock := refblock.Parent`).
fn referrer(tree: &Tree, reference: El) -> R<El> {
    nifblk(tree, reference).ok_or_else(access_violation)
}

// ===========================================================================
/// `CheckStringIndex`.
fn check_string_index(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    if !matches!(
        version(tree),
        NifVersion::Fo3 | NifVersion::Tes5 | NifVersion::Sse | NifVersion::Fo4
    ) {
        return Ok(());
    }
    let header = header(tree)?;
    let num_strings = el(tree, header, "Num Strings")?;
    let n = tree.native_value(num_strings)?.to_i64()?;
    for i in 0..blocks_count(tree)? {
        let b = block(tree, i)?;
        for string in block_strings(tree, b) {
            if tree.native_value(string)?.to_i64()? >= n {
                log.push(format!("\t{}: Invalid string index", tree.path(string)?));
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `RecursiveIndexCheck` of `CheckBlocksOrder`.
fn recursive_index_check(tree: &mut Tree, parent: El, log: &mut Vec<String>) -> R<()> {
    for reference in block_refs(tree, parent) {
        // Ref only.
        if tree.def(reference)?.ptr() {
            continue;
        }
        let Some(child) = tree.links_to(reference)? else {
            continue;
        };
        if tree.user_data(child) == 1 {
            continue;
        }
        // Skip the constraints.
        if block_is_ni_object(tree, child, "bhkConstraint", true)
            || block_is_ni_object(tree, child, "bhkBallSocketConstraintChain", true)
        {
            continue;
        }
        if block_is_ni_object(tree, child, "bhkAction", true) {
            // An action refers to a rigid body and loads after it.
            if tree.index(child)? < tree.index(parent)? {
                log.push(format!(
                    "\t{}: Must have greater index than its parent {}",
                    name(tree, child)?,
                    name(tree, parent)?
                ));
            }
        } else if block_is_ni_object(tree, child, "bhkRefObject", true) && tree.index(child)? > tree.index(parent)? {
            // Ref objects are the opposite: they load before.
            log.push(format!(
                "\t{}: Must have lesser index than its parent {}",
                name(tree, child)?,
                name(tree, parent)?
            ));
        }
        // Marked as touched against circular links.
        tree.set_user_data(child, 1);
        recursive_index_check(tree, child, log)?;
        tree.set_user_data(child, 0);
    }
    Ok(())
}

/// `CheckBlocksOrder`.
fn check_blocks_order(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    if !matches!(
        version(tree),
        NifVersion::Tes4 | NifVersion::Fo3 | NifVersion::Tes5 | NifVersion::Sse
    ) {
        return Ok(());
    }
    for b in blocks_by_type(tree, "bhkCollisionObject", false)? {
        recursive_index_check(tree, b, log)?;
    }
    Ok(())
}

// ===========================================================================
/// `CountBlocksUsage` of `CheckUnusedBlocks`.
fn count_blocks_usage(tree: &mut Tree, b: Option<El>, usage: &mut [i32]) -> R<()> {
    let Some(b) = b else { return Ok(()) };
    let index = tree.index(b)? as usize;
    usage[index] += 1;
    // Scan the references only once.
    if usage[index] == 1 {
        for reference in block_refs(tree, b) {
            let target = tree.links_to(reference)?;
            count_blocks_usage(tree, target, usage)?;
        }
    }
    Ok(())
}

/// `CheckUnusedBlocks`.
fn check_unused_blocks(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    let roots = root_nodes(tree)?;
    let footer = footer(tree)?;
    if roots.is_empty() {
        log.push(format!("\t{}: No root node", name(tree, footer)?));
    }
    if roots.len() > 1 {
        log.push(format!("\t{}: Multiple root nodes", name(tree, footer)?));
    }
    let mut usage = vec![0; blocks_count(tree)? as usize];
    for root in roots {
        count_blocks_usage(tree, Some(root), &mut usage)?;
    }
    for (i, count) in usage.iter().enumerate() {
        if *count == 0 {
            let b = block(tree, i as i32)?;
            log.push(format!(
                "\t{}: Unused block not referenced from the root scenegparh",
                name(tree, b)?
            ));
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckInvalidRepeatedChildrenNames`.
fn check_invalid_repeated_children_names(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    if !matches!(
        version(tree),
        NifVersion::Fo3 | NifVersion::Tes5 | NifVersion::Sse | NifVersion::Fo4
    ) {
        return Ok(());
    }
    // A case-sensitive list of the names.
    let mut names: Vec<(String, El)> = Vec::new();
    for b in blocks_by_type(tree, "NiObjectNET", true)? {
        if block_type(tree, b) == "BSValueNode" || block_is_editor_marker(tree, b)? {
            continue;
        }
        let n = tree.edit_values(b, "Name")?;
        if n.is_empty() || n == "InvMarker" || n == "FurnitureMarker" {
            continue;
        }
        // The material file in the shader's name of Fallout 4 meshes.
        if version(tree) >= NifVersion::Fo4 && block_is_ni_object(tree, b, "BSShaderProperty", true) {
            continue;
        }
        match names.iter().find(|(known, _)| *known == n) {
            Some(&(_, other)) => {
                let other = name(tree, other)?;
                log.push(format!(
                    "\t{}: The same name \"{n}\" is also used by {other}",
                    name(tree, b)?
                ));
            }
            None => names.push((n, b)),
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckInvalidArrayLinks`.
fn check_invalid_array_links(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    let count = i64::from(blocks_count(tree)?);
    for links in get_link_arrays(tree)? {
        let mut idx: Vec<i64> = Vec::new();
        for item in items(tree, links)? {
            let n = tree.native_value(item)?.to_i64()?;
            let i = idx.len();
            idx.push(n);
            if n < 0 {
                log.push(format!("\t{} is a null link", tree.path(item)?));
            } else if n >= count {
                log.push(format!("\t{} is a broken link", tree.path(item)?));
            } else if idx[..i].contains(&n) {
                log.push(format!("\t{} is a repeated link", tree.path(item)?));
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckWrongLinkTypes`.
fn check_wrong_link_types(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    for i in 0..blocks_count(tree)? {
        let b = block(tree, i)?;
        for reference in block_refs(tree, b) {
            let Some(target) = tree.links_to(reference)? else {
                continue;
            };
            let template = tree.def(reference)?.template().to_owned();
            if !block_is_ni_object(tree, target, &template, true) {
                log.push(format!(
                    "\t{} links to {}, expected {template}",
                    tree.path(reference)?,
                    name(tree, target)?
                ));
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckHardcodedBlockNames`.
fn check_hardcoded_block_names(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    const NAMES: [(&str, &str); 15] = [
        ("BSBehaviorGraphExtraData", "BGED"),
        ("BSBoneLODExtraData", "BSBoneLOD"),
        ("BSBound", "BBX"),
        ("BSClothExtraData", "CED"),
        ("BSConnectPoint::Children", "CPT"),
        ("BSConnectPoint::Parents", "CPA"),
        ("BSDecalPlacementVectorExtraData", "DVPG"),
        ("BSDistantObjectLargeRefExtraData", "DOLRED"),
        ("BSEyeCenterExtraData", "ECED"),
        ("BSFurnitureMarker", "FRN"),
        ("BSFurnitureMarkerNode", "FRN"),
        ("BSInvMarker", "INV"),
        ("BSPositionData", "BSPosData"),
        ("BSWArray", "BSW"),
        ("BSXFlags", "BSX"),
    ];
    let tree = &mut nif.tree;
    if version(tree) == NifVersion::Unknown {
        return Ok(());
    }
    for i in 0..blocks_count(tree)? {
        let b = block(tree, i)?;
        if tree.elements(b, "Name")?.is_none() {
            continue;
        }
        let current = tree.edit_values(b, "Name")?;
        if let Some((_, hard_name)) = NAMES.iter().find(|(kind, _)| block_type(tree, b) == *kind)
            && current != *hard_name
        {
            log.push(format!("\t{}: Must be named {hard_name}", name(tree, b)?));
        }
        if block_type(tree, b) == "BSValueNode" {
            let proper_name = format!("AddOnNode{}", tree.edit_values(b, "Value")?);
            if !current.starts_with(&proper_name) {
                log.push(format!("\t{}: Name must start with \"{proper_name}\"", name(tree, b)?));
            }
        }
        if version(tree) > NifVersion::Tes3
            && current == "Weapon"
            && block_is_ni_object(tree, b, "NiAVObject", true)
            && block_by_type(tree, "BSBound", false)?.is_none()
        {
            log.push(format!(
                "\t{}: \"Weapon\" name is hardcoded and can be used in skeleton nifs only",
                name(tree, b)?
            ));
        }
        if version(tree) == NifVersion::Tes4 && block_is_ni_object(tree, b, "NiTriBasedGeom", true) {
            let material = block_property_by_type(tree, b, "NiMaterialProperty", false)?;
            // Only rendered shapes (with NiTexturingProperty) need a named
            // material.
            if let Some(material) = material
                && tree.edit_values(material, "Name")?.is_empty()
                && block_property_by_type(tree, b, "NiTexturingProperty", false)?.is_some()
            {
                log.push(format!(
                    "\t{}: Has no name which is required for NiMaterialProperty",
                    name(tree, material)?
                ));
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `BadTensor`.
fn bad_tensor(t: f32) -> bool {
    t.is_nan() || same_value_single(t, 0.0) || t < 0.0
}

/// `GetColObjects` of `CheckCollision`: the collision objects of a node and
/// its child nodes, sorted by name without repeats.
fn get_col_objects(tree: &mut Tree, node: Option<El>, cols: &mut Vec<(String, El)>) -> R<()> {
    let Some(node) = node else { return Ok(()) };
    if let Some(col) = block_get_collision(tree, node)? {
        let col_name = name(tree, col)?;
        // A sorted `TStringList` with `dupIgnore`.
        let key = col_name.to_uppercase();
        match cols.binary_search_by(|(known, _)| known.to_uppercase().cmp(&key)) {
            Ok(_) => {}
            Err(position) => cols.insert(position, (col_name, col)),
        }
    }
    for child in block_children_by_type(tree, node, "NiNode", true)? {
        get_col_objects(tree, Some(child), cols)?;
    }
    Ok(())
}

/// `CheckCollision`.
fn check_collision(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    let ver = version(tree);
    if !matches!(
        ver,
        NifVersion::Tes4 | NifVersion::Fo3 | NifVersion::Tes5 | NifVersion::Sse
    ) {
        return Ok(());
    }
    let animated = block_by_type(tree, "NiControllerManager", false)?.is_some();

    for b in blocks_by_type(tree, "bhkRigidBody", true)? {
        let Some(shape) = link(tree, b, "Shape")? else { continue };
        let ms = tree.edit_values(b, "Motion System")?;
        let mq = tree.edit_values(b, "Motion Quality")?;
        let mass = tree.native_values(b, "Mass")?.to_f64()? as f32;
        let dynamic = block_is_dynamic_rigid_body(tree, b)?;
        let b_name = name(tree, b)?;

        if ms == "MO_SYS_FIXED" && mq != "MO_QUAL_FIXED" {
            log.push(format!(
                "\t{b_name}: Motion System is MO_SYS_FIXED but Motion Quality is not MO_QUAL_FIXED"
            ));
        }
        if ver < NifVersion::Tes5 {
            if ms != "MO_SYS_FIXED" && mq == "MO_QUAL_FIXED" {
                log.push(format!(
                    "\t{b_name}: Motion System is not MO_SYS_FIXED but Motion Quality is MO_QUAL_FIXED"
                ));
            }
            if ms.ends_with("STABILIZED") {
                log.push(format!("\t{b_name}: {ms} Motion System is not supported pre Skyrim"));
            }
        }
        // No MOPP in statics and animations.
        if (dynamic || ms == "MO_SYS_KEYFRAMED") && block_type(tree, shape) == "bhkMoppBvTreeShape" {
            log.push(format!(
                "\t{b_name}: MOPP shape is used with dynamic or keyframed motion system instead of simple shape(s)"
            ));
        }
        if !dynamic && tree.native_values(b, "Body Flags")?.to_f64()? > 0.0 {
            log.push(format!(
                "\t{b_name}: Body Flags (used for wind simulation) set on static collision, causes performance issues"
            ));
        }

        let min_pen: f32 = if ver < NifVersion::Tes5 { 0.01 } else { 0.002 };
        let pen = tree.native_values(b, "Penetration Depth")?.to_f64()? as f32;
        if pen > 0.0 && pen < min_pen {
            log.push(format!(
                "\t{b_name}: Penetration Depth < {} causes Havok issues due to precision loss",
                df_float_to_str(f64::from(min_pen))
            ));
        }

        // Animated meshes have an "infinite" mass in the engine.
        if dynamic && !animated {
            if same_value_single(mass, 0.0) {
                log.push(format!("\t{b_name}: Zero moveable collision mass"));
            }
            if mass > 0.0 && mass < 0.95 {
                log.push(format!(
                    "\t{b_name}: Moveable mass < 0.1 causes physics issues due to precision loss"
                ));
            }
            if mass > 0.0 {
                let mut bad = false;
                for m in ["Inertia Tensor\\m11", "Inertia Tensor\\m22", "Inertia Tensor\\m33"] {
                    if bad_tensor(tree.native_values(b, m)?.to_f64()? as f32) {
                        bad = true;
                        break;
                    }
                }
                if bad {
                    log.push(format!(
                        "\t{b_name}: Moveable mass is not zero but Inertia Tensor matrix is zero or invalid"
                    ));
                }
            }
            if tree.native_values(b, "Max Angular Velocity")?.to_f64()? < 1.0 {
                log.push(format!(
                    "\t{b_name}: Max Angular Velocity < 1.0 causes sinking through the terrain"
                ));
            }
            if ver >= NifVersion::Tes5 && tree.edit_values(b, "Enable Deactivation")? == "no" {
                log.push(format!(
                    "\t{b_name}: Enable Deactivation=no causes performance issues on dynamic bodies"
                ));
            }
            if ver < NifVersion::Tes5 && tree.edit_values(b, "Deactivator Type")? == "DEACTIVATOR_NEVER" {
                log.push(format!(
                    "\t{b_name}: DEACTIVATOR_NEVER causes performance issues on dynamic bodies"
                ));
            }
            if tree.edit_values(b, "Solver Deactivation")? == "SOLVER_DEACTIVATION_OFF" {
                log.push(format!(
                    "\t{b_name}: SOLVER_DEACTIVATION_OFF causes performance issues on dynamic bodies"
                ));
            }
        }
    }

    for b in blocks_by_type(tree, "bhkConstraint", true)? {
        let prefix = match block_type(tree, b) {
            "bhkMalleableConstraint" => "Hinge\\Ragdoll",
            "bhkBreakableConstraint" => "Constraint Data\\Ragdoll",
            _ => "Ragdoll",
        };
        if let Some(angle) = tree.elements(b, &format!("{prefix}\\Cone Max Angle"))?
            && same_value(tree.native_value(angle)?.to_f64()?, 0.0)
        {
            log.push(format!("\t{}: 0.0 value causes jittering", tree.path(angle)?));
        }
    }

    // The animated collision objects: of the nodes (and their children)
    // that controlled blocks target.
    let mut cols: Vec<(String, El)> = Vec::new();
    for b in blocks_by_type(tree, "NiControllerSequence", true)? {
        let blocks = el(tree, b, "Controlled Blocks")?;
        for entry in items(tree, blocks)? {
            if get_controlled_block_name(tree, entry, "Controller Type")? != "NiTransformController" {
                continue;
            }
            let node_name = get_controlled_block_name(tree, entry, "Node Name")?;
            if node_name.is_empty() {
                continue;
            }
            let interp = link(tree, entry, "Interpolator")?;
            let Some(interp) = interp.filter(|&interp| block_type(tree, interp) == "NiTransformInterpolator") else {
                continue;
            };
            if link(tree, interp, "Data")?.is_none() {
                continue;
            }
            let node = block_by_name(tree, &node_name, "")?;
            get_col_objects(tree, node, &mut cols)?;
        }
    }
    // And the nodes with a NiTransformController.
    for b in blocks_by_type(tree, "NiNode", true)? {
        if block_get_collision(tree, b)?.is_none() {
            continue;
        }
        let Some(controller) = block_get_controller(tree, b, "NiTransformController", true)? else {
            continue;
        };
        let interp = link(tree, controller, "Interpolator")?;
        let Some(interp) = interp.filter(|&interp| block_type(tree, interp) == "NiTransformInterpolator") else {
            continue;
        };
        if link(tree, interp, "Data")?.is_none() {
            continue;
        }
        get_col_objects(tree, Some(b), &mut cols)?;
    }

    for col in blocks_by_type(tree, "bhkCollisionObject", true)? {
        let rigid = link(tree, col, "Body")?;
        if let Some(rigid) = rigid
            && matches!(tree.native_values(rigid, "Havok Filter\\Layer")?.to_i64()?, 2 | 28)
            && !cols.iter().any(|(_, known)| *known == col)
        {
            log.push(format!(
                "\t{}: Animated layer is used on non animated collision",
                name(tree, col)?
            ));
        }
    }

    for (_, col) in cols {
        if block_type(tree, col) != "bhkCollisionObject" {
            continue;
        }
        let Some(rigid) = link(tree, col, "Body")? else {
            continue;
        };
        let layer = tree.native_values(rigid, "Havok Filter\\Layer")?.to_i64()?;
        if !matches!(layer, 2 | 4 | 5 | 6 | 14 | 15 | 16 | 28) {
            log.push(format!(
                "\t{}: Animated collision must use Animated layer",
                name(tree, rigid)?
            ));
        }
        // SET_LOCAL is set at run time in the earlier games.
        if matches!(ver, NifVersion::Tes5 | NifVersion::Sse)
            && matches!(layer, 2 | 28)
            && !flag(tree, col, "Flags\\SET_LOCAL")?
        {
            log.push(format!(
                "\t{}: Animated layer collision is missing SET_LOCAL flag",
                name(tree, col)?
            ));
        }
        if matches!(ver, NifVersion::Tes4 | NifVersion::Fo3)
            && !flag(tree, col, "Flags\\USE_VEL")?
            && tree.edit_values(rigid, "Motion System")? == "MO_SYS_KEYFRAMED"
        {
            log.push(format!(
                "\t{}: Transformed keyframed animated collision is missing USE_VEL flag",
                name(tree, col)?
            ));
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckCollisionMOPP`.
fn check_collision_mopp(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    if !matches!(
        version(tree),
        NifVersion::Tes4 | NifVersion::Fo3 | NifVersion::Tes5 | NifVersion::Sse
    ) {
        return Ok(());
    }
    if block_by_type(tree, "bhkMoppBvTreeShape", false)?.is_none() {
        return Ok(());
    }
    // The complexity of the MOPP collision.
    let mut tris: i64 = 0;
    let mut col_tris: i64 = 0;
    for i in 0..blocks_count(tree)? {
        let b = block(tree, i)?;
        let kind = block_type(tree, b);
        if kind == "bhkNiTriStripsShape" {
            let strips = el(tree, b, "Strips Data")?;
            for data in items(tree, strips)? {
                let Some(shape) = tree.links_to(data)? else { continue };
                if block_type(tree, shape) == "NiTriStripsData" {
                    col_tris += tree.native_values(shape, "Num Triangles")?.to_i64()?;
                }
            }
        } else if kind == "hkPackedNiTriStripsData" {
            let triangles = el(tree, b, "Triangles")?;
            col_tris += i64::from(tree.count(triangles));
        } else if kind == "bhkCompressedMeshShapeData" {
            let big_tris = el(tree, b, "Big Tris")?;
            col_tris += i64::from(tree.count(big_tris));
            let chunks = el(tree, b, "Chunks")?;
            for chunk in items(tree, chunks)? {
                let mut strips_len = 0;
                let lengths = el(tree, chunk, "Strip Lengths")?;
                for strip in items(tree, lengths)? {
                    let s = tree.native_value(strip)?.to_i64()?;
                    col_tris += s - 2;
                    strips_len += s;
                }
                let indices = el(tree, chunk, "Indices")?;
                col_tris += (i64::from(tree.count(indices)) - strips_len) / 3;
            }
        } else if block_is_ni_object(tree, b, "NiTriBasedGeom", true) {
            let Some(data) = link(tree, b, "Data")? else { continue };
            if block_is_ni_object(tree, data, "NiTriBasedGeomData", true) {
                tris += tree.native_values(data, "Num Triangles")?.to_i64()?;
            }
        } else if block_is_ni_object(tree, b, "BSTriShape", true) {
            tris += tree.native_values(b, "Num Triangles")?.to_i64()?;
        }
    }
    if tris > 10 && col_tris > 10 {
        let ratio = (col_tris as f64 / tris as f64 * 100.0).round_ties_even() as i64;
        if ratio > 50 {
            log.push(format!(
                "\tMOPP collision tris to geometry tris ratio is {ratio}% ({col_tris}/{tris}), poorly optimized collision"
            ));
        }
    }
    Ok(())
}

// ===========================================================================
/// `GetSkinPartition` of `CheckSkinningIssues`.
fn get_skin_partition(tree: &mut Tree, shape: El) -> R<Option<El>> {
    let Some(skin) = block_get_skin(tree, shape)? else {
        return Ok(None);
    };
    link(tree, skin, "Skin Partition")
}

/// `CheckSkinningIssues`.
fn check_skinning_issues(file: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    let ver = version(tree);
    if matches!(ver, NifVersion::Fo3 | NifVersion::Tes5 | NifVersion::Sse) {
        let root = root_node(tree)?;
        if block_type(tree, root) == "NiNode" {
            for skin in blocks_by_type(tree, "NiSkinInstance", true)? {
                if block_type(tree, skin) != "BSDismemberSkinInstance" {
                    log.push(format!("\t{}: Must be BSDismemberSkinInstance", name(tree, skin)?));
                    continue;
                }
                let skeleton = link(tree, skin, "Skeleton root")?;
                let valid = match skeleton {
                    Some(skeleton) => tree.index(skeleton)? == 0,
                    None => false,
                };
                if !valid {
                    log.push(format!("\t{}: Invalid Skeleton Root", name(tree, skin)?));
                }
                let parts = el(tree, skin, "Partitions")?;
                // A case-insensitive list of the body parts.
                let mut seen: Vec<String> = Vec::new();
                for part in items(tree, parts)? {
                    let num = tree.native_values(part, "Body Part")?.to_i64()?;
                    let p = tree.edit_values(part, "Body Part")?;
                    if matches!(ver, NifVersion::Tes5 | NifVersion::Sse) && !(30..=62).contains(&num) {
                        log.push(format!("\t{}: Invalid body part {p}", tree.path(part)?));
                    }
                    if seen.iter().any(|known| ansi_same_text(known, &p)) {
                        log.push(format!("\t{}: Repeated body part {p}", tree.path(part)?));
                    } else {
                        seen.push(p);
                    }
                }
                if let Some(partition) = link(tree, skin, "Skin Partition")?
                    && i64::from(tree.count(parts)) < tree.native_values(partition, "Num Partitions")?.to_i64()?
                {
                    log.push(format!(
                        "\t{}: Has lower Num Partitions than in {}",
                        name(tree, skin)?,
                        name(tree, partition)?
                    ));
                }
            }
        }
    }

    if matches!(ver, NifVersion::Tes5 | NifVersion::Sse | NifVersion::Fo4) {
        for shape in blocks_by_type(tree, "BSDynamicTriShape", true)? {
            if block_get_skin(tree, shape)?.is_none() {
                log.push(format!(
                    "\t{}: Missing skin instance (acceptable only in headparts and facegen)",
                    name(tree, shape)?
                ));
            }
        }
    }

    // UPSTREAM-QUIRK: `FileName` of a mesh loaded from data is empty, so the
    // comparison of the _0 and _1 morph models never runs in Sniff.
    let file_name = tree.nif.file_name.clone();
    if matches!(ver, NifVersion::Tes5 | NifVersion::Sse)
        && file_name.to_lowercase().ends_with("_0.nif")
        && block_by_type(tree, "NiSkinInstance", true)?.is_some()
    {
        compare_morph_models(file, nif, log)?;
    }
    Ok(())
}

/// The `_0.nif` against `_1.nif` part of `CheckSkinningIssues`.
fn compare_morph_models(file: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let lower = file.file_name.to_lowercase();
    let f = match lower.rfind("_0.nif") {
        Some(index) => format!("{}_1.nif{}", &file.file_name[..index], &file.file_name[index + 6..]),
        None => file.file_name.clone(),
    };
    let mut nif1 = NifFile::new()?;
    // Loading issues are ignored.
    let loaded = match (file.file_entry, &file.input.archive) {
        (Some(_), Some(archive)) => match archive.read(&f) {
            Ok(Some(data)) => nif1.load_from_data(&data).is_ok(),
            _ => false,
        },
        _ => std::path::Path::new(&f).is_file() && nif1.load_from_file(std::path::Path::new(&f)).is_ok(),
    };
    if !loaded {
        return Ok(());
    }
    let tree = &mut nif.tree;
    let tree1 = &mut nif1.tree;
    let (Some(&r0), Some(&r1)) = (root_nodes(tree)?.first(), root_nodes(tree1)?.first()) else {
        return Ok(());
    };
    let root_name = name(tree, r0)?;
    let m0 = block_children_by_type(tree, r0, "NiNode", false)?;
    let m1 = block_children_by_type(tree1, r1, "NiNode", false)?;
    if m0.len() != m1.len() {
        log.push(format!(
            "\t{root_name}: Bone counts between morph models don't match in _0.nif and _1.nif"
        ));
        return Ok(());
    }
    let mut names0: Vec<String> = m0.iter().map(|&n| tree.edit_values(n, "Name")).collect::<R<_>>()?;
    let mut names1: Vec<String> = m1.iter().map(|&n| tree1.edit_values(n, "Name")).collect::<R<_>>()?;
    names0.sort_by_key(|n| n.to_uppercase());
    names1.sort_by_key(|n| n.to_uppercase());
    if names0 != names1 {
        log.push(format!(
            "\t{root_name}: Bones between morph models don't match in _0.nif and _1.nif"
        ));
    }
    let mut m0 = block_children_by_type(tree, r0, "BSTriShape", false)?;
    let mut m1 = block_children_by_type(tree1, r1, "BSTriShape", false)?;
    if m0.is_empty() {
        m0 = block_children_by_type(tree, r0, "NiTriBasedGeom", false)?;
    }
    if m1.is_empty() {
        m1 = block_children_by_type(tree1, r1, "NiTriBasedGeom", false)?;
    }
    if m0.len() != m1.len() {
        log.push(format!(
            "\t{root_name}: Shape counts between morph models don't match in _0.nif and _1.nif"
        ));
        return Ok(());
    }
    for (&b0, &b1) in m0.iter().zip(&m1) {
        if tree.edit_values(b0, "Name")? != tree1.edit_values(b1, "Name")? {
            log.push(format!(
                "\t{}: Shapes names between morph models don't match in _0.nif and _1.nif",
                name(tree, b0)?
            ));
            continue;
        }
        let (Some(p0), Some(p1)) = (get_skin_partition(tree, b0)?, get_skin_partition(tree1, b1)?) else {
            continue;
        };
        if tree.native_values(p0, "Num Partitions")? != tree1.native_values(p1, "Num Partitions")? {
            log.push(format!(
                "\t{}: Skin partition counts between morph models don't match in _0.nif and _1.nif",
                name(tree, p0)?
            ));
            continue;
        }
        let parts0 = el(tree, p0, "Partitions")?;
        let parts1 = el(tree1, p1, "Partitions")?;
        for j in 0..tree.count(parts0) {
            let part0 = tree.item(parts0, j)?;
            let part1 = tree1.item(parts1, j)?;
            if tree.native_values(part0, "Num Vertices")? != tree1.native_values(part1, "Num Vertices")? {
                log.push(format!(
                    "\t{}: Model vertex counts between morph models don't match in _0.nif and _1.nif",
                    tree.path(part0)?
                ));
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckParticleSystem`.
fn check_particle_system(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    const EMITTER_LIFE_SPAN_MARGIN: f64 = 12.0;
    let tree = &mut nif.tree;

    // Unnamed modifiers.
    for b in blocks_by_type(tree, "NiPSysModifier", true)? {
        if tree.edit_values(b, "Name")?.is_empty() {
            log.push(format!("\t{}: Name is not set", name(tree, b)?));
        }
    }

    // Invalid modifiers in controllers.
    for b in blocks_by_type(tree, "NiPSysModifierCtlr", true)? {
        let mod_name = tree.edit_values(b, "Modifier Name")?;
        let modifier = if mod_name.is_empty() {
            None
        } else {
            block_by_name(tree, &mod_name, "")?
        };
        if !modifier.is_some_and(|modifier| block_is_ni_object(tree, modifier, "NiPSysModifier", true)) {
            log.push(format!(
                "\t{}: Modifier Name \"{mod_name}\" points to invalid block",
                name(tree, b)?
            ));
        }
    }

    // Too long particle life spans.
    for b in blocks_by_type(tree, "NiPSysEmitter", true)? {
        let life_span = tree.native_values(b, "Life Span")?;
        if life_span.to_f64()? > EMITTER_LIFE_SPAN_MARGIN {
            log.push(format!(
                "\t{}: Life Span of {} might negatively affect performance",
                name(tree, b)?,
                life_span.to_i64()?
            ));
        }
    }

    // Orphan "-Emitter" nodes.
    for b in blocks_by_type(tree, "NiAVObject", true)? {
        let node_name = tree.edit_values(b, "Name")?;
        if !node_name.to_lowercase().ends_with("-emitter") {
            continue;
        }
        // A target of a NiPSysEmitter.
        let mut found = false;
        for reference in block_referenced_by(tree, b)? {
            let referring = referrer(tree, reference)?;
            if block_is_ni_object(tree, referring, "NiPSysEmitter", true) {
                found = true;
                break;
            }
        }
        // Or a NiParticleSystem of the name.
        if !found {
            let chars: Vec<char> = node_name.chars().collect();
            let base: String = chars[..chars.len().saturating_sub(8)].iter().collect();
            found = !base.is_empty() && block_by_name(tree, &base, "NiParticleSystem")?.is_some();
            if base.is_empty() {
                // `BlockByName('')` raises.
                return Err(DfError::new("Can not find block by an empty name"));
            }
        }
        if !found {
            log.push(format!(
                "\t{}: \"{node_name}\" is an emitter but missing the same named NiParticleSystem and not a target of any NiPSysEmitter",
                name(tree, b)?
            ));
        }
    }

    // Num Subtexture Offsets.
    let num = if version(tree) >= NifVersion::Tes5 { 256 } else { 16 };
    for b in blocks_by_type(tree, "NiParticlesData", true)? {
        if tree.native_values(b, "Num Subtexture Offsets")?.to_f64()? > f64::from(num) {
            log.push(format!(
                "\t{}: Num Subtexture Offsets cannot be higher than {num}",
                name(tree, b)?
            ));
        }
    }

    // Mesh emitters.
    if version(tree) >= NifVersion::Sse {
        for b in blocks_by_type(tree, "NiPSysMeshEmitter", true)? {
            let meshes = el(tree, b, "Emitter Meshes")?;
            for item in items(tree, meshes)? {
                let Some(mesh) = tree.links_to(item)? else { continue };
                if block_type(tree, mesh) != "BSTriShape" {
                    continue;
                }
                if tree.native_values(mesh, "Particle Data Size")?.to_i64()? == 0
                    && block_extra_data_by_type(tree, mesh, "BSPositionData", false)?.is_none()
                {
                    log.push(format!(
                        "\t{}: Has no Particle Data or BSPositionData but used as mesh emitter in {}",
                        name(tree, mesh)?,
                        name(tree, b)?
                    ));
                }
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckTargetField`.
fn check_target_field(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;

    // `GetCollisionParentNode`.
    let collision_parent = |tree: &mut Tree, col: El| -> R<Option<El>> {
        for node in blocks_by_type(tree, "NiAVObject", true)? {
            if link(tree, node, "Collision Object")? == Some(col) {
                return Ok(Some(node));
            }
        }
        Ok(None)
    };

    // The collision targets.
    for col in blocks_by_type(tree, "bhkNiCollisionObject", true)? {
        let body = link(tree, col, "Body")?;
        if body.is_none() {
            log.push(format!("\t{}: Missing collision Body", name(tree, col)?));
        }
        let target = link(tree, col, "Target")?;
        if version(tree) >= NifVersion::Tes5
            && let Some(target) = target
            && tree.edit_values(target, "Name")?.is_empty()
        {
            log.push(format!("\t{}: The used Target node must have a name", name(tree, col)?));
        }
        let Some(parent) = collision_parent(tree, col)? else {
            continue;
        };
        if target != Some(parent) {
            log.push(format!("\t{}: Invalid Target field", name(tree, col)?));
        }

        // The other Target of bhkCompressedMeshShape.
        let Some(body) = body.filter(|&body| block_is_ni_object(tree, body, "bhkWorldObject", true)) else {
            continue;
        };
        let Some(shape) = link(tree, body, "Shape")? else {
            if block_type(tree, body) != "bhkAabbPhantom" {
                log.push(format!("\t{}: Missing rigid body shape", name(tree, body)?));
            }
            continue;
        };
        if !block_is_ni_object(tree, shape, "bhkMoppBvTreeShape", true) {
            continue;
        }
        let mesh_shape = link(tree, shape, "Shape")?;
        let Some(mesh_shape) =
            mesh_shape.filter(|&mesh| block_is_ni_object(tree, mesh, "bhkCompressedMeshShape", true))
        else {
            continue;
        };
        let target = link(tree, mesh_shape, "Target")?;
        if let Some(target) = target
            && tree.edit_values(target, "Name")?.is_empty()
        {
            log.push(format!(
                "\t{}: The used Target node must have a name",
                name(tree, mesh_shape)?
            ));
        }
        // The root node is a target too.
        let root = root_node(tree)?;
        if target != Some(parent) && target != Some(root) {
            log.push(format!("\t{}: Invalid Target field", name(tree, mesh_shape)?));
        }
    }

    // The controller targets.
    for b in blocks_by_type(tree, "NiObjectNET", true)? {
        let mut controller = link(tree, b, "Controller")?;
        while let Some(current) = controller {
            if !block_is_ni_object(tree, current, "NiTimeController", true) {
                log.push(format!(
                    "\t{}: uses controller {} which is not a descendant of NiTimeController",
                    name(tree, b)?,
                    name(tree, current)?
                ));
                break;
            }
            match link(tree, current, "Target")? {
                None => log.push(format!("\t{}: Invalid Target field", name(tree, current)?)),
                Some(target) if target != b => log.push(format!(
                    "\t{}: Used by {} but targets {}",
                    name(tree, current)?,
                    name(tree, b)?,
                    name(tree, target)?
                )),
                Some(_) => {}
            }
            // Down the chain of Next Controller.
            controller = link(tree, current, "Next Controller")?;
        }
    }

    // The node names of the animations.
    for manager in blocks_by_type(tree, "NiControllerManager", false)? {
        let seqs = el(tree, manager, "Controller Sequences")?;
        for seq in items(tree, seqs)? {
            let Some(sequence) = tree.links_to(seq)? else { continue };
            if let Some(seq_manager) = tree.elements(sequence, "Manager")?
                && tree.links_to(seq_manager)? != Some(manager)
            {
                log.push(format!(
                    "\t{}: Invalid Manager field, must be {}",
                    name(tree, sequence)?,
                    name(tree, manager)?
                ));
            }
            if tree.elements(sequence, "Accum Root Name")?.is_some() {
                let root_name = tree.edit_values(sequence, "Accum Root Name")?;
                if root_name.is_empty() || block_by_name(tree, &root_name, "NiAVObject")?.is_none() {
                    log.push(format!(
                        "\t{}: Invalid Accum Root Name \"{root_name}\", must be existing NiAVObject",
                        name(tree, sequence)?
                    ));
                }
            }
            // The targets of the controlled blocks.
            let blocks = el(tree, sequence, "Controlled Blocks")?;
            for entry in items(tree, blocks)? {
                let t_name = get_controlled_block_name(tree, entry, "Node Name")?;
                let t = if t_name.is_empty() {
                    None
                } else {
                    block_by_name(tree, &t_name, "NiAVObject")?
                };
                let Some(t) = t else {
                    log.push(format!(
                        "\t{}: Invalid Node Name \"{t_name}\", must be existing NiAVObject",
                        tree.path(entry)?
                    ));
                    continue;
                };
                if block_is_hidden(tree, t)? {
                    log.push(format!("\t{}: Target node \"{t_name}\" is hidden", tree.path(entry)?));
                }
                let prop_type = get_controlled_block_name(tree, entry, "Property Type")?;
                if !prop_type.is_empty() && block_property_by_type(tree, t, &prop_type, false)?.is_none() {
                    log.push(format!(
                        "\t{}: Property {prop_type} not found for Target \"{t_name}\"",
                        tree.path(entry)?
                    ));
                }
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckKeys` of `CheckAnimStopTime`.
fn check_keys(tree: &mut Tree, data: El, keys: Option<El>, stop_time: &str, log: &mut Vec<String>) -> R<()> {
    let Some(keys) = keys else { return Ok(()) };
    let count = tree.count(keys);
    if count == 0 {
        return Ok(());
    }
    let last = tree.item(keys, count - 1)?;
    let t = tree.edit_values(last, "Time")?;
    if t != stop_time {
        log.push(format!(
            "\t{}: Time {t} of the last key in {} doesn't match Stop Time {stop_time}",
            name(tree, data)?,
            tree.path(keys)?
        ));
    }
    Ok(())
}

/// `CheckAnimStopTime`.
fn check_anim_stop_time(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    for b in blocks_by_type(tree, "NiControllerSequence", false)? {
        let stop_time = tree.edit_values(b, "Stop Time")?;

        // The interpolators.
        let entries = el(tree, b, "Controlled Blocks")?;
        for entry in items(tree, entries)? {
            let Some(interpolator) = link(tree, entry, "Interpolator")? else {
                continue;
            };
            if tree.elements(interpolator, "Data")?.is_none() {
                continue;
            }
            let Some(data) = link(tree, interpolator, "Data")? else {
                continue;
            };
            for path in [
                "Data\\Keys",
                "Translations\\Keys",
                "Scales\\Keys",
                "Quaternion Keys",
                "XYZ Rotations\\[0]\\Keys",
                "XYZ Rotations\\[1]\\Keys",
                "XYZ Rotations\\[2]\\Keys",
            ] {
                let keys = tree.elements(data, path)?;
                check_keys(tree, data, keys, &stop_time, log)?;
            }
        }

        // NiTextKeyExtraData.
        let Some(x_data) = link(tree, b, "Text Keys")? else {
            continue;
        };
        let Some(keys) = tree.elements(x_data, "Text Keys")? else {
            continue;
        };
        for j in (0..tree.count(keys)).rev() {
            let key = tree.item(keys, j)?;
            if tree.edit_values(key, "Value")? != "end" {
                continue;
            }
            let value = tree.edit_values(key, "Float")?;
            if value != stop_time {
                log.push(format!(
                    "\t{}: The value of \"end\" key {value} doesn't match Stop Time {stop_time}",
                    name(tree, x_data)?
                ));
            }
            break;
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckBSXFlags`.
fn check_bsx_flags(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    const FLAGS: [&str; 10] = [
        "Animated",
        "Havok",
        "Ragdoll",
        "Complex",
        "Addon",
        "Editor Marker",
        "Dynamic",
        "",
        "",
        "External Emit",
    ];
    const WHY: [&str; 10] = [
        "Controller or AddOn",
        "Collision",
        "Constraint",
        "multiple Dynamic rigid bodies",
        "BSValueNode/AttachLight/FlameNode",
        "EditorMarker",
        "Dynamic rigid body",
        "",
        "",
        "Emitting shader",
    ];
    let tree = &mut nif.tree;
    if version(tree) < NifVersion::Fo3 {
        return Ok(());
    }
    let bsx = block_by_type(tree, "BSXFlags", false)?;
    let (flags, bsx_name) = match bsx {
        Some(bsx) => (tree.native_values(bsx, "Flags")?.to_i64()? as u32, name(tree, bsx)?),
        None => (0, "Missing BSXFlags".to_owned()),
    };
    let new_flags = detect_bsx_flags(tree)?;
    for (i, flag_name) in FLAGS.iter().enumerate() {
        if flag_name.is_empty() {
            continue;
        }
        // Complex and Dynamic are not reported in Fallout 4 meshes, where
        // dynamic bodies are not detected.
        if version(tree) >= NifVersion::Fo4 && (i == 3 || i == 6) {
            continue;
        }
        let mask = 1u32 << i;
        if new_flags & mask != 0 && flags & mask == 0 {
            log.push(format!(
                "\t{bsx_name}: Has {} but bit {i} ({flag_name}) is not set",
                WHY[i]
            ));
        }
        if new_flags & mask == 0 && flags & mask != 0 {
            log.push(format!("\t{bsx_name}: No {} but bit {i} ({flag_name}) is set", WHY[i]));
        }
    }
    if let Some(bsx) = bsx
        && new_flags == 0
    {
        log.push(format!("\t{}: BSXFlags is present but not needed", name(tree, bsx)?));
    }
    Ok(())
}

// ===========================================================================
/// `CheckConsistencyFlags`.
fn check_consistency_flags(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    if !matches!(version(tree), NifVersion::Tes4 | NifVersion::Fo3 | NifVersion::Tes5) {
        return Ok(());
    }
    for shape in blocks_by_type(tree, "NiGeometry", true)? {
        let Some(data) = link(tree, shape, "Data")? else {
            continue;
        };
        if tree.elements(data, "Consistency Flags")?.is_none() {
            continue;
        }
        let controller = block_get_controller(tree, shape, "", false)?;
        let mutable = block_is_ni_object(tree, shape, "NiParticles", true)
            || controller.is_some_and(|controller| {
                block_is_ni_object(tree, controller, "NiGeomMorpherController", true)
                    || block_is_ni_object(tree, controller, "NiUVController", true)
            });
        let f = if mutable { "CT_MUTABLE" } else { "CT_STATIC" };
        let f2 = tree.edit_values(data, "Consistency Flags")?;
        if f2 != f {
            log.push(format!(
                "\t{}: Invalid Consistency Flags value {f2}, should be {f} (doesn't matter for LODs)",
                name(tree, data)?
            ));
        }
    }
    Ok(())
}

// ===========================================================================
/// `TPath.HasValidPathChars(Path, False)`: no control character and none
/// of `"<>|`.
fn has_valid_path_chars(path: &str) -> bool {
    !path.chars().any(|c| c < ' ' || matches!(c, '"' | '<' | '>' | '|'))
}

/// `TPath.IsPathRooted`.
fn is_path_rooted(path: &str) -> bool {
    let mut chars = path.chars();
    match (chars.next(), chars.next()) {
        (Some('\\' | '/'), _) => true,
        (Some(letter), Some(':')) => letter.is_ascii_alphabetic(),
        _ => false,
    }
}

/// `CheckTextureSetSlots`.
fn check_texture_set_slots(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    for shader in blocks_by_type(tree, "BSShaderProperty", true)? {
        let Some(texset_link) = tree.elements(shader, "Texture Set")? else {
            continue;
        };
        let Some(texset) = tree.links_to(texset_link)? else {
            continue;
        };
        let Some(textures) = tree.elements(texset, "Textures")? else {
            continue;
        };
        let textures = items(tree, textures)?;
        let mut values = Vec::with_capacity(textures.len());
        for &texture in &textures {
            let value = tree.edit_value(texture)?;
            if !has_valid_path_chars(&value) {
                log.push(format!("\t{}: Invalid characters in {value}", tree.path(texture)?));
            } else if is_path_rooted(&value) {
                log.push(format!("\t{}: Absolute path {value}", tree.path(texture)?));
            }
            values.push(value);
        }
        let slot = |index: usize| -> Option<&str> { values.get(index).map(String::as_str) };

        if version(tree) == NifVersion::Fo3 {
            let slot4 = slot(4).ok_or_else(access_violation)?;
            if !slot4.is_empty()
                && !(flag(tree, shader, "Shader Flags 1\\Environment_Mapping")?
                    || flag(tree, shader, "Shader Flags 1\\Eye_Environment_Mapping")?
                    || flag(tree, shader, "Shader Flags 1\\Window_Environment_Mapping")?)
            {
                log.push(format!(
                    "\t{}: Has assigned envmap texture in {} but missing envmap flag",
                    name(tree, shader)?,
                    name(tree, texset)?
                ));
            }
        }

        if matches!(version(tree), NifVersion::Tes5 | NifVersion::Sse) {
            let shader_type = tree.edit_values(shader, "Shader Type")?;
            let texset_name = name(tree, texset)?;
            let used = |index: usize| slot(index).is_some_and(|value| !value.is_empty());
            if used(2)
                && shader_type != "Glow Shader"
                && shader_type != "Facegen"
                && shader_type != "Skin Tint"
                && !flag(tree, shader, "Shader Flags 2\\Soft_Lighting")?
                && !flag(tree, shader, "Shader Flags 2\\Rim_Lighting")?
            {
                log.push(format!(
                    "\t{texset_name}: Has texture assigned in slot 2, but is not used"
                ));
            }
            if used(3) && shader_type != "Parallax" && shader_type != "Facegen" {
                log.push(format!(
                    "\t{texset_name}: Has texture assigned in slot 3, but is not used"
                ));
            }
            for index in [4, 5] {
                if used(index)
                    && shader_type != "Environment Map"
                    && shader_type != "MultiLayer Parallax"
                    && shader_type != "Eye Envmap"
                {
                    log.push(format!(
                        "\t{texset_name}: Has texture assigned in slot {index}, but is not used"
                    ));
                }
            }
            if used(6) && shader_type != "Facegen" && shader_type != "MultiLayer Parallax" {
                log.push(format!(
                    "\t{texset_name}: Has texture assigned in slot 6, but is not used"
                ));
            }
            if used(7)
                && !flag(tree, shader, "Shader Flags 2\\Back_Lighting")?
                && !flag(tree, shader, "Shader Flags 1\\Model_Space_Normals")?
            {
                log.push(format!(
                    "\t{texset_name}: Has texture assigned in slot 7, but is not used"
                ));
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckNiAlphaProperty`.
fn check_ni_alpha_property(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    for i in 0..blocks_count(tree)? {
        let shape = block(tree, i)?;
        if !(block_is_ni_object(tree, shape, "BSTriShape", true) || block_is_ni_object(tree, shape, "NiGeometry", true))
        {
            continue;
        }
        let Some(prop) = block_property_by_type(tree, shape, "NiAlphaProperty", false)? else {
            continue;
        };
        if block_property_by_type(tree, shape, "BSShaderNoLightingProperty", false)?.is_some() {
            continue;
        }
        let flags = tree.native_values(prop, "Flags")?.to_i64()? as u32;
        let alpha_blend = flags & 1 == 1;
        let shader = block_property_by_type(tree, shape, "BSShaderProperty", true)?;
        if let Some(shader) = shader
            && version(tree) < NifVersion::Fo4
            && alpha_blend
            && !flag(tree, shader, "Shader Flags 2\\Assume_Shadowmask")?
        {
            log.push(format!(
                "\t{}: Blend alpha forces the object to be in single-pass mode, and can cause lighting issues if multiple lights are illuminating the object",
                name(tree, prop)?
            ));
        }
    }
    Ok(())
}

// ===========================================================================
/// The checks of `CheckShaderTypeFlags` for a `BSLightingShaderProperty`
/// of Skyrim. Returns `false` for the `Continue` of a missing texture set.
fn check_lighting_shader(
    tree: &mut Tree,
    shape: El,
    shader: El,
    texset: Option<El>,
    has_vertex_colors: bool,
    facegen: bool,
    log: &mut Vec<String>,
) -> R<bool> {
    let shader_name = name(tree, shader)?;
    let Some(texset) = texset else {
        log.push(format!("\t{shader_name}: Missing BSShaderTextureSet"));
        return Ok(false);
    };
    let texset_name = name(tree, texset)?;
    let shape_name = name(tree, shape)?;
    let shader_type = tree.edit_values(shader, "Shader Type")?;
    let mut tex =
        |index: usize| -> R<bool> { Ok(tree.edit_values(texset, &format!("Textures\\[{index}]"))?.is_empty()) };
    let empty: [bool; 8] = [tex(0)?, tex(1)?, tex(2)?, tex(3)?, tex(4)?, tex(5)?, tex(6)?, tex(7)?];
    let f = |tree: &mut Tree, path: &str| flag(tree, shader, path);
    let mut add = |line: String| log.push(line);

    // Diffuse and normal textures.
    if empty[0] {
        add(format!(
            "\t{texset_name}: Diffuse Texture [Slot 0] must be set for all Shaders"
        ));
    }
    if empty[1] {
        add(format!(
            "\t{texset_name}: Normal Texture [Slot 1] must be set for all Shaders"
        ));
    }

    // Vertex colors and alpha.
    let vertex_colors = f(tree, "Shader Flags 2\\Vertex_Colors")?;
    if has_vertex_colors && !vertex_colors {
        add(format!(
            "\t{shape_name}: Has vertex colors but missing Vertex_Colors shader flag in {shader_name}"
        ));
    }
    if !has_vertex_colors && vertex_colors {
        add(format!(
            "\t{shape_name}: Has no vertex colors but Vertex_Colors shader flag is set in {shader_name}"
        ));
    }
    if !has_vertex_colors && f(tree, "Shader Flags 1\\Vertex_Alpha")? {
        add(format!(
            "\t{shape_name}: Has no vertex colors but Vertex_Alpha shader flag is set in {shader_name}"
        ));
    }

    // Environment map: type, flag and textures.
    if shader_type == "Environment Map" {
        if !f(tree, "Shader Flags 1\\Environment_Mapping")? {
            add(format!(
                "\t{shader_name}: Environment Map shader type is used but missing Environment_Mapping shader flag"
            ));
        }
        if empty[4] {
            add(format!(
                "\t{texset_name}: Environment Texture [Slot 4] must be set for Environment shader"
            ));
        }
        if empty[5] {
            add(format!(
                "\t{texset_name}: Environment Mask Texture [Slot 5] must be set for Environment shader"
            ));
        }
    } else if f(tree, "Shader Flags 1\\Environment_Mapping")? {
        add(format!(
            "\t{shader_name}: Environment_Mapping shader flag is set but shader type is not Environment Map"
        ));
    }

    // Glow shader: flags and texture.
    if shader_type == "Glow Shader" {
        if !f(tree, "Shader Flags 2\\Glow_Map")? {
            add(format!(
                "\t{shader_name}: Glow Shader type is used but missing Glow_Map shader flag"
            ));
        }
        if !f(tree, "Shader Flags 1\\Own_Emit")? {
            add(format!(
                "\t{shader_name}: Glow Shader type is used but missing Own_Emit shader flag"
            ));
        }
        if tree.edit_values(shader, "Emissive Color")? == "#000000" {
            add(format!(
                "\t{shader_name}: Glow Shader type is used but Emissive Color is blank"
            ));
        }
        if empty[2] {
            add(format!(
                "\t{texset_name}: Glow Texture [Slot 2] must be set for Glow shader"
            ));
        }
    } else if f(tree, "Shader Flags 2\\Glow_Map")? {
        add(format!(
            "\t{shader_name}: Glow_Map shader flag is set but shader type is not Glow Shader"
        ));
    }

    // Parallax shader: flag, colors and texture.
    if shader_type == "Parallax" {
        if !f(tree, "Shader Flags 1\\Parallax")? {
            add(format!(
                "\t{shader_name}: Parallax shader type is used but missing Parallax shader flag"
            ));
        }
        if !has_vertex_colors {
            add(format!(
                "\t{shader_name}: Parallax shader type is used but missing Vertex Colors on shape"
            ));
        }
        if f(tree, "Shader Flags 2\\Multi_Layer_Parallax")? {
            add(format!(
                "\t{shader_name}: Multi_Layer_Parallax shader flag can't be used with Parallax shader type"
            ));
        }
        if empty[3] {
            add(format!(
                "\t{texset_name}: Parallax Texture [Slot 3] must be set for Parallax shader"
            ));
        }
    } else if f(tree, "Shader Flags 1\\Parallax")? {
        add(format!(
            "\t{shader_name}: Parallax shader flag is set but shader type is not Parallax"
        ));
    }

    // Facegen shader: flags and textures.
    if shader_type == "Facegen" {
        if !f(tree, "Shader Flags 1\\Facegen")? {
            add(format!(
                "\t{shader_name}: Facegen shader type is used but missing Facegen shader flag"
            ));
        }
        if !f(tree, "Shader Flags 2\\Soft_Lighting")? {
            add(format!(
                "\t{shader_name}: Facegen shader type is used but missing Soft_Lighting shader flag"
            ));
        }
        if f(tree, "Shader Flags 2\\Anisotropic_Lighting")? {
            add(format!(
                "\t{shader_name}: Anisotropic_Lighting shader flag cannot be used with Facegen Shader Type"
            ));
        }
        if empty[2] {
            add(format!(
                "\t{texset_name}: Skin Tint Texture [Slot 2] must be set for Facegen shader"
            ));
        }
        if empty[3] {
            add(format!(
                "\t{texset_name}: Facegen Detail Texture [Slot 3] must be set for Facegen shader"
            ));
        }
        if empty[6] {
            add(format!(
                "\t{texset_name}: Facegen Tint Texture [Slot 6] must be set for Facegen shader"
            ));
        }
    } else if f(tree, "Shader Flags 1\\Facegen")? {
        add(format!(
            "\t{shader_name}: Facegen shader flag is set but shader type is not Facegen"
        ));
    }

    // Skin tint shader: flags and textures.
    if shader_type == "Skin Tint" {
        if !f(tree, "Shader Flags 1\\Skin_Tint")? {
            add(format!(
                "\t{shader_name}: Skin Tint shader type is used but missing Skin_Tint shader flag"
            ));
        }
        if !f(tree, "Shader Flags 2\\Soft_Lighting")? {
            add(format!(
                "\t{shader_name}: Skin Tint shader type is used but missing Soft_Lighting shader flag"
            ));
        }
        if empty[2] {
            add(format!(
                "\t{texset_name}: Skin Tint Texture [Slot 2] must be set for Skin Tint shader"
            ));
        }
    } else if f(tree, "Shader Flags 1\\Skin_Tint")? {
        add(format!(
            "\t{shader_name}: Skin_Tint shader flag is set but shader type is not Skin Tint"
        ));
    }

    // Hair tint shader.
    if shader_type == "Hair Tint" {
        if !f(tree, "Shader Flags 1\\Hair_Tint")? {
            add(format!(
                "\t{shader_name}: Hair Tint shader type is used but missing Hair_Tint shader flag"
            ));
        }
    } else if f(tree, "Shader Flags 1\\Hair_Tint")? {
        add(format!(
            "\t{shader_name}: Hair_Tint shader flag is set but shader type is not Hair Tint"
        ));
    }

    // Multi layer parallax shader.
    if shader_type == "MultiLayer Parallax" {
        if !f(tree, "Shader Flags 2\\Multi_Layer_Parallax")? {
            add(format!(
                "\t{shader_name}: MultiLayer Parallax shader type is used but missing Multi Layer Parallax shader flag"
            ));
        }
        if f(tree, "Shader Flags 1\\Parallax")? {
            add(format!(
                "\t{shader_name}: Parallax shader flag can't be used with MultiLayer Parallax shader type"
            ));
        }
        if empty[4] {
            add(format!(
                "\t{texset_name}: Environment Texture [Slot 4] must be set for MultiLayer Parallax shader"
            ));
        }
        if empty[5] {
            add(format!(
                "\t{texset_name}: Environment Mask Texture [Slot 5] must be set for MultiLayer Parallax shader"
            ));
        }
        if empty[6] {
            add(format!(
                "\t{texset_name}: Inner Layer Texture [Slot 6] must be set for MultiLayer Parallax shader"
            ));
        }
    } else if f(tree, "Shader Flags 2\\Multi_Layer_Parallax")? {
        add(format!(
            "\t{shader_name}: Multi_Layer_Parallax shader flag is set but shader type is not MultiLayer Parallax"
        ));
    }

    // Eye envmap shader and flag.
    if shader_type == "Eye Envmap" {
        if !f(tree, "Shader Flags 1\\Eye_Environment_Mapping")? {
            add(format!(
                "\t{shader_name}: Eye Envmap shader type is used but missing Eye_Environment_Mapping shader flag"
            ));
        }
        if empty[4] {
            add(format!(
                "\t{texset_name}: Environment Texture [Slot 4] must be set for Eye Envmap shader"
            ));
        }
        if empty[5] {
            add(format!(
                "\t{texset_name}: Environment Mask Texture [Slot 5] must be set for Eye Envmap shader"
            ));
        }
    } else if f(tree, "Shader Flags 1\\Eye_Environment_Mapping")? {
        add(format!(
            "\t{shader_name}: Eye_Environment_Mapping shader flag is set but shader type is not Eye Envmap"
        ));
    }

    // The Back_Lighting flag.
    if f(tree, "Shader Flags 2\\Back_Lighting")? && empty[7] {
        add(format!(
            "\t{texset_name}: Back Lighting Texture [Slot 7] must be set with Back lighting flag"
        ));
    }

    // The Character_Lighting flag.
    let character_lighting = f(tree, "Shader Flags 2\\Character_Lighting")?;
    if facegen {
        if !character_lighting {
            add(format!(
                "\t{shader_name}: file is a Facegen nif but Character_Lighting flag is not set"
            ));
        }
    } else if character_lighting {
        add(format!(
            "\t{shader_name}: file is not a Facegen nif but Character_Lighting flag is set"
        ));
    }

    // The EnvMap_Light_Fade flag.
    if (shader_type == "Environment Map" || shader_type == "MultiLayer Parallax" || shader_type == "Eye Envmap")
        && !f(tree, "Shader Flags 2\\EnvMap_Light_Fade")?
    {
        add(format!(
            "\t{shader_name}: Shader Type is Environment/MultiLayer Parallax, but missing EnvMap_Light_Fade flag"
        ));
    }

    // The Rim_Lighting flag.
    let rim = f(tree, "Shader Flags 2\\Rim_Lighting")?;
    if rim && empty[2] {
        add(format!(
            "\t{texset_name}: Rim Lighting Texture [Slot 2] must be set with Rim_Lighting flag"
        ));
    }

    // The Soft_Lighting flag.
    let soft = f(tree, "Shader Flags 2\\Soft_Lighting")?;
    if soft && shader_type != "Skin Tint" && shader_type != "Facegen" && empty[2] {
        add(format!(
            "\t{texset_name}: Soft Lighting Texture [Slot 2] must be set with Soft_lighting flag"
        ));
    }

    // Rim and soft lighting.
    if rim && soft {
        add(format!(
            "\t{shader_name}: Rim and Soft lighting can not be used together"
        ));
    }

    // The Specular flag.
    if f(tree, "Shader Flags 1\\Specular")? {
        if tree.edit_values(shader, "Specular Color")? == "#000000" {
            add(format!(
                "\t{shader_name}: Specular flag is set, but Specular Color is blank"
            ));
        }
        // The Model_Space_Normals flag.
        if f(tree, "Shader Flags 1\\Model_Space_Normals")? {
            if empty[7] {
                add(format!(
                    "\t{texset_name}: Specular Texture [Slot 7] must be set for Model_Space_Normals + Specular flags"
                ));
            }
            if f(tree, "Shader Flags 2\\Back_Lighting")? {
                add(format!(
                    "\t{shader_name}: Back_Lighting flag can't be used with Model_Space_Normals and Specular flags"
                ));
            }
        }
    }

    // The Tree_Anim flag.
    if f(tree, "Shader Flags 2\\Tree_Anim")? {
        let root = root_node(tree)?;
        let root_block = block_type(tree, root);
        if root_block != "BSLeafAnimNode" && root_block != "BSTreeNode" {
            add(format!(
                "\t{shader_name}: Tree_Anim flag is set but the root node is not BSLeafAnimNode or BSTreeNode"
            ));
        }
        if f(tree, "Shader Flags 2\\Glow_Map")? {
            add(format!(
                "\t{shader_name}: Tree_Anim and Glow_Map flags don't work together in game and crash CK"
            ));
        }
        if !f(tree, "Shader Flags 2\\Vertex_Colors")? {
            add(format!(
                "\t{shader_name}: Tree_Anim shader flag requires Vertex_Colors shader flag"
            ));
        }
        if !f(tree, "Shader Flags 1\\Vertex_Alpha")? {
            add(format!(
                "\t{shader_name}: Tree_Anim shader flag requires Vertex_Alpha shader flag"
            ));
        }
    }

    // Glossiness.
    if let Some(glossiness) = tree.elements(shader, "Glossiness")?
        && same_value(tree.native_value(glossiness)?.to_f64()?, 0.0)
    {
        add(format!("\t{shader_name}: Zero Glossiness causes lighting issues"));
    }
    Ok(true)
}

/// `CheckShaderTypeFlags`.
fn check_shader_type_flags(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    let ver = version(tree);
    if !matches!(
        ver,
        NifVersion::Fo3 | NifVersion::Tes5 | NifVersion::Sse | NifVersion::Fo4
    ) {
        return Ok(());
    }
    let mut external_emit_shader = false;
    let mut emit_shader = String::new();
    let facegen = block_by_name(tree, "BSFaceGenNiNodeSkinned", "NiNode")?.is_some();

    for i in 0..blocks_count(tree)? {
        let shape = block(tree, i)?;
        if !(block_is_ni_object(tree, shape, "BSTriShape", true) || block_is_ni_object(tree, shape, "NiGeometry", true))
        {
            continue;
        }
        let mut has_vertex_colors = false;
        if block_is_ni_object(tree, shape, "NiGeometry", true) {
            if let Some(shape_data) = link(tree, shape, "Data")? {
                has_vertex_colors = flag(tree, shape_data, "Has Vertex Colors")?;
            }
        } else {
            has_vertex_colors = flag(tree, shape, "VertexDesc\\VF\\VF_COLORS")?;
        }

        let Some(shader) = block_property_by_type(tree, shape, "BSShaderProperty", true)? else {
            // Editor markers need no shader.
            if block_is_editor_marker(tree, shape)? {
                continue;
            }
            // Shapes without tangents are not rendered.
            if block_is_ni_object(tree, shape, "BSTriShape", true) && !flag(tree, shape, "VertexDesc\\VF\\VF_TANGENT")?
            {
                continue;
            }
            // Mesh emitters need no shader.
            let mut emitter = false;
            for reference in block_referenced_by(tree, shape)? {
                let referring = referrer(tree, reference)?;
                if block_is_ni_object(tree, referring, "NiPSysEmitter", true) {
                    emitter = true;
                    break;
                }
            }
            if !emitter {
                log.push(format!("\t{}: Missing shader property", name(tree, shape)?));
            }
            continue;
        };

        let texset = match tree.elements(shader, "Texture Set")? {
            Some(texture_set) => tree.links_to(texture_set)?,
            None => None,
        };

        if !external_emit_shader && flag(tree, shader, "Shader Flags 1\\External_Emittance")? {
            external_emit_shader = true;
            emit_shader = name(tree, shader)?;
        }

        // Fallout 3 and New Vegas.
        if ver == NifVersion::Fo3 {
            if (tree.edit_values(shader, "Shader Type")? == "SHADER_SKIN")
                ^ flag(tree, shader, "Shader Flags 1\\FaceGen")?
            {
                log.push(format!(
                    "\t{}: SHADER_SKIN shader type and FaceGen shader flag must be set together",
                    name(tree, shader)?
                ));
            }
            if tree.edit_values(shader, "Shader Type")? == "SHADER_NOLIGHTING"
                && block_type(tree, shader) == "BSShaderPPLightingProperty"
            {
                log.push(format!(
                    "\t{}: Invalid shader type SHADER_NOLIGHTING for BSShaderPPLightingProperty",
                    name(tree, shader)?
                ));
            }
        }

        // Skyrim.
        if matches!(ver, NifVersion::Tes5 | NifVersion::Sse) {
            // BSTriShapes need tangents.
            if block_is_ni_object(tree, shape, "BSTriShape", true)
                && !(flag(tree, shader, "Shader Flags 1\\Model_Space_Normals")?
                    || flag(tree, shape, "VertexDesc\\VF\\VF_TANGENT")?)
            {
                log.push(format!(
                    "\t{}: Has no tangentspace and is not using modelspace normals and will not render",
                    name(tree, shape)?
                ));
            }

            // The Dynamic_Decal flag.
            if flag(tree, shader, "Shader Flags 1\\Dynamic_Decal")? {
                if !flag(tree, shader, "Shader Flags 1\\Decal")? {
                    log.push(format!(
                        "\t{}: Dynamic_Decal flag is used, but Decal flag is not set",
                        name(tree, shader)?
                    ));
                }
                if !flag(tree, shader, "Shader Flags 2\\Assume_Shadowmask")? {
                    log.push(format!(
                        "\t{}: Dynamic_Decal flag is used, but Assume_Shadowmask flag is not set",
                        name(tree, shader)?
                    ));
                }
            }

            if block_type(tree, shader) == "BSEffectShaderProperty"
                && (flag(tree, shader, "Shader Flags 1\\Grayscale_To_PaletteColor")?
                    || flag(tree, shader, "Shader Flags 1\\Grayscale_To_PaletteAlpha")?)
                && tree.edit_values(shader, "Grayscale Texture")?.is_empty()
            {
                log.push(format!(
                    "\t{}: Grayscale Texture must be set for Grayscale_To_PaletteColor or Grayscale_To_PaletteAlpha flags",
                    name(tree, shader)?
                ));
            }

            if block_type(tree, shader) == "BSLightingShaderProperty"
                && !check_lighting_shader(tree, shape, shader, texset, has_vertex_colors, facegen, log)?
            {
                continue;
            }
        }
        // Fallout 4: an external material file makes the shader settings
        // unused; nothing follows.
    }

    // The BSXFlags check reports the rest.
    if let Some(bsx) = block_by_name(tree, "BSX", "")?
        && tree.native_values(bsx, "Flags")?.to_i64()? & (1 << 9) == 0
        && external_emit_shader
    {
        // Points at the emitting shader.
        log.push(format!(
            "\t{emit_shader}: External_Emit shader flag is set but the same flag is not set in {}",
            name(tree, bsx)?
        ));
    }
    Ok(())
}

// ===========================================================================
/// `CheckTris` of `CheckGeometry`.
fn check_tris(tree: &mut Tree, tris: Option<El>, num_verts: i64, tri_element: &str, log: &mut Vec<String>) -> R<()> {
    let Some(tris) = tris else { return Ok(()) };
    let num_verts_usize = num_verts.max(0) as usize;
    let mut used = vec![0u8; num_verts_usize];
    let mut reported = false;
    for i in 0..tree.count(tris) {
        let item = tree.item(tris, i)?;
        let tri_el = if tri_element.is_empty() {
            item
        } else {
            el(tree, item, tri_element)?
        };
        // The three words of the triangle.
        let bytes = tree.value_bytes(tri_el);
        let word = |k: usize| -> i64 {
            i64::from(u16::from_le_bytes([
                bytes.get(2 * k).copied().unwrap_or(0),
                bytes.get(2 * k + 1).copied().unwrap_or(0),
            ]))
        };
        let tri = [word(0), word(1), word(2)];
        if !reported && tri.iter().any(|&v| v >= num_verts) {
            let path = tree.path(item)?;
            log.push(format!(
                "\t{path}: Triangle ({}, {}, {}) exceeds the number of vertices {num_verts}",
                tri[0], tri[1], tri[2]
            ));
            // One is enough; broken meshes can have many.
            reported = true;
        }
        for v in tri {
            if v < num_verts {
                used[v as usize] = 1;
            }
        }
    }
    let num_used: i64 = used.iter().map(|&v| i64::from(v)).sum();
    if num_verts != num_used {
        let parent = tree.parent(tris).ok_or_else(access_violation)?;
        log.push(format!(
            "\t{}: Unused vertices (Num Vertices: {num_verts}, Used vertices: {num_used})",
            name(tree, parent)?
        ));
    }
    Ok(())
}

/// `CheckStrips` of `CheckGeometry`.
fn check_strips(tree: &mut Tree, strips: Option<El>, num_verts: i64, log: &mut Vec<String>) -> R<()> {
    let Some(strips) = strips else { return Ok(()) };
    let mut used = vec![0u8; num_verts.max(0) as usize];
    let mut reported = false;
    for strip in items(tree, strips)? {
        for point in items(tree, strip)? {
            let p = tree.native_value(point)?.to_i64()?;
            if !reported && p >= num_verts {
                log.push(format!(
                    "\t{}: Strip point {p} exceeds the number of vertices {num_verts}",
                    tree.path(strip)?
                ));
                reported = true;
            }
            if p < num_verts && p >= 0 {
                used[p as usize] = 1;
            }
        }
    }
    let num_used: i64 = used.iter().map(|&v| i64::from(v)).sum();
    if num_verts != num_used {
        let parent = tree.parent(strips).ok_or_else(access_violation)?;
        log.push(format!(
            "\t{}: Unused vertices (Num Vertices: {num_verts}, Used Vertices: {num_used})",
            name(tree, parent)?
        ));
    }
    Ok(())
}

/// `CheckGeometry`.
fn check_geometry(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    for i in 0..blocks_count(tree)? {
        let b = block(tree, i)?;
        match block_type(tree, b) {
            "BSTriShape" => {
                let tris = tree.elements(b, "Triangles")?;
                let num_verts = tree.native_values(b, "Num Vertices")?.to_i64()?;
                check_tris(tree, tris, num_verts, "", log)?;
                // Duplicate vertices.
                let Some(vertex_data) = tree.elements(b, "Vertex Data")? else {
                    continue;
                };
                if num_verts > 0 {
                    let first = tree.item(vertex_data, 0).map_err(|_| access_violation())?;
                    let size = tree.data_size(first)?.max(0) as usize;
                    let mut verts = Vec::new();
                    tree.serialize(vertex_data, &mut verts)?;
                    verts.resize(size * num_verts as usize, 0);
                    let n = num_verts as usize;
                    let v_size = verts.len() / n;
                    let mut dups = vec![false; n];
                    let mut num_dups = 0;
                    if v_size > 0 {
                        for v1 in 0..n.saturating_sub(1) {
                            if dups[v1] {
                                continue;
                            }
                            let a = &verts[v1 * v_size..(v1 + 1) * v_size];
                            for v2 in v1 + 1..n {
                                if !dups[v2] && verts[v2 * v_size..(v2 + 1) * v_size] == *a {
                                    num_dups += 1;
                                    dups[v2] = true;
                                }
                            }
                        }
                    }
                    if num_dups > 0 {
                        log.push(format!(
                            "\t{}: Duplicate vertices (Num Vertices: {num_verts}, Dup Vertices: {num_dups})",
                            name(tree, b)?
                        ));
                    }
                }
            }
            "NiTriShapeData" => {
                let tris = tree.elements(b, "Triangles")?;
                let num_verts = tree.native_values(b, "Num Vertices")?.to_i64()?;
                check_tris(tree, tris, num_verts, "", log)?;
            }
            "NiTriStripsData" => {
                let strips = tree.elements(b, "Strips")?;
                let num_verts = tree.native_values(b, "Num Vertices")?.to_i64()?;
                check_strips(tree, strips, num_verts, log)?;
                let s_num = tree.native_values(b, "Num Strips")?.to_i64()?;
                if s_num > 1 {
                    log.push(format!(
                        "\t{}: Num Strips = {s_num}, should always be 1 to reduce the number of draw calls",
                        name(tree, b)?
                    ));
                }
            }
            "hkPackedNiTriStripsData" => {
                let tris = tree.elements(b, "Triangles")?;
                let num_verts = tree.native_values(b, "Num Vertices")?.to_i64()?;
                check_tris(tree, tris, num_verts, "Triangle", log)?;
            }
            "NiSkinPartition" => {
                let parts = el(tree, b, "Partitions")?;
                for part in items(tree, parts)? {
                    let s_num = tree.native_values(part, "Num Strips")?.to_i64()?;
                    if s_num > 1 {
                        log.push(format!(
                            "\t{}: Num Strips = {s_num}, should always be 1 to reduce the number of draw calls",
                            tree.path(part)?
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckVertexColors`.
fn check_vertex_colors(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    const WHITE: [u8; 16] = [0, 0, 0x80, 0x3F, 0, 0, 0x80, 0x3F, 0, 0, 0x80, 0x3F, 0, 0, 0x80, 0x3F];
    let tree = &mut nif.tree;
    for i in 0..blocks_count(tree)? {
        let shape = block(tree, i)?;
        if !(block_is_ni_object(tree, shape, "BSTriShape", true)
            || block_is_ni_object(tree, shape, "NiTriBasedGeom", true))
        {
            continue;
        }
        let mut all_white = true;
        let mut hdr = false;
        let mut alpha = false;
        let shape_name;

        if block_is_ni_object(tree, shape, "NiTriBasedGeom", true) {
            let Some(shape_data) = link(tree, shape, "Data")? else {
                continue;
            };
            if tree.native_values(shape_data, "Has Vertex Colors")?.to_i64()? == 0 {
                continue;
            }
            let Some(colors) = tree.elements(shape_data, "Vertex Colors")? else {
                continue;
            };
            if tree.count(colors) == 0 {
                continue;
            }
            for (j, color) in items(tree, colors)?.into_iter().enumerate() {
                let bytes: Vec<u8> = tree.value_bytes(color).to_vec();
                if bytes.len() >= 16 && bytes[..16] == WHITE {
                    continue;
                }
                let c: Vec<f32> = (0..4)
                    .map(|k| {
                        bytes
                            .get(4 * k..4 * k + 4)
                            .map_or(0.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    })
                    .collect();
                all_white = false;
                if !alpha && c[3] < 1.0 {
                    alpha = true;
                }
                if !hdr && c.iter().any(|&v| v < 0.0 || v > 1.0) {
                    log.push(format!("\t{}: HDR vertex color at index {j}", name(tree, shape_data)?));
                    hdr = true;
                }
                // Everything found.
                if !all_white && alpha && hdr {
                    break;
                }
            }
            shape_name = name(tree, shape_data)?;
        } else {
            if !flag(tree, shape, "VertexDesc\\VF\\VF_COLORS")? {
                continue;
            }
            let Some(vertices) = tree.elements(shape, "Vertex Data")? else {
                continue;
            };
            if tree.count(vertices) == 0 {
                continue;
            }
            for vertex in items(tree, vertices)? {
                let colors = el(tree, vertex, "Vertex Colors")?;
                let c: Vec<u8> = tree.value_bytes(colors).to_vec();
                if c.len() >= 4 && c[..4] == [0xFF; 4] {
                    continue;
                }
                all_white = false;
                if !alpha && c.get(3).copied().unwrap_or(0) < 255 {
                    alpha = true;
                }
                if !all_white && alpha {
                    break;
                }
            }
            shape_name = name(tree, shape)?;
        }

        let shader = block_property_by_type(tree, shape, "BSShaderProperty", true)?;
        if all_white {
            // Vertex colors are needed for leaf animations and parallax.
            let needed = match shader {
                Some(shader) => {
                    flag(tree, shader, "Shader Flags 2\\Tree_Anim")?
                        || tree.edit_values(shader, "Shader Type")? == "Parallax"
                }
                None => false,
            };
            if !needed {
                log.push(format!("\t{shape_name}: All white #FFFFFFFF vertex colors"));
            }
        }
        if alpha
            && let Some(shader) = shader
            && matches!(version(tree), NifVersion::Fo3 | NifVersion::Tes5 | NifVersion::Sse)
            && block_property_by_type(tree, shape, "NiAlphaProperty", false)?.is_some()
            && !flag(tree, shader, "Shader Flags 1\\Vertex_Alpha")?
        {
            log.push(format!(
                "\t{shape_name}: Has alpha < 1.0 in vertex colors and attached NiAlphaProperty but missing Vertex_Alpha flag in {}",
                name(tree, shader)?
            ));
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckMiscellaneous`.
fn check_miscellaneous(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    // The root node.
    let r = root_node(tree)?;
    if tree.index(r)? != 0 {
        log.push(format!("\t{}: Root node must be at index 0", name(tree, r)?));
    }
    if !(block_is_ni_object(tree, r, "NiAVObject", true)
        || block_is_ni_object(tree, r, "NiSequence", true)
        || block_type(tree, r) == "NiSequenceStreamHelper")
    {
        log.push(format!(
            "\t{}: Root node is not a NiAVObject/NiSequence or their descendant",
            name(tree, r)?
        ));
    }

    // The vertex count of the tangents and binormals of Oblivion.
    if version(tree) == NifVersion::Tes4 {
        for shape in blocks_by_type(tree, "NiTriBasedGeom", true)? {
            let Some(tan) = block_extra_data_by_name(tree, shape, TES4_TANGENTS_EXTRA_DATA_NAME)? else {
                continue;
            };
            let Some(shape_data) = link(tree, shape, "Data")? else {
                continue;
            };
            // A tangent and a binormal of three floats per vertex.
            let data = el(tree, tan, "Data")?;
            let v = i64::from(tree.data_size(data)? / 24);
            if v != 0 && v != tree.native_values(shape_data, "Num Vertices")?.to_i64()? {
                log.push(format!(
                    "\t{}: Tangents and binormals size doesn't match vertices count in {}",
                    name(tree, tan)?,
                    name(tree, shape_data)?
                ));
            }
        }
    }

    // Subshapes of bhkListShape that are not primitives.
    if version(tree) >= NifVersion::Tes5 {
        for list in blocks_by_type(tree, "bhkListShape", false)? {
            let shapes = el(tree, list, "Sub Shapes")?;
            for item in items(tree, shapes)? {
                if let Some(shape) = tree.links_to(item)?
                    && !block_is_ni_object(tree, shape, "bhkConvexShape", true)
                {
                    // The message of the CK log.
                    log.push(format!(
                        "\t{}: Uses invalid child container subshape {}",
                        name(tree, list)?,
                        name(tree, shape)?
                    ));
                }
            }
        }
    }

    // Shapes of FaceGen meshes are BSDynamicTriShape.
    let facegen = block_by_name(tree, "BSFaceGenNiNodeSkinned", "NiNode")?.is_some();
    if version(tree) == NifVersion::Sse && facegen {
        for shape in blocks_by_type(tree, "BSTriShape", false)? {
            log.push(format!(
                "\t{}: Shapes must be of type BSDynamicTriShape in Facegen nifs",
                name(tree, shape)?
            ));
        }
    }

    // NiSpecularProperty in Oblivion and later.
    if version(tree) >= NifVersion::Tes4 {
        for spec in blocks_by_type(tree, "NiSpecularProperty", false)? {
            log.push(format!("\t{}: Not supported, does nothing", name(tree, spec)?));
        }
    }

    // A shader property with a controller of the other kind crashes.
    for (shader_type, wrong) in [
        ("BSEffectShaderProperty", "BSLighting"),
        ("BSLightingShaderProperty", "BSEffect"),
    ] {
        for shader in blocks_by_type(tree, shader_type, false)? {
            if let Some(controller) = block_get_controller(tree, shader, "", false)?
                && block_type(tree, controller).starts_with(wrong)
            {
                log.push(format!(
                    "\t{}: Attached controller {} is invalid for this shader property",
                    name(tree, shader)?,
                    block_type(tree, controller)
                ));
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckOptional`.
fn check_optional(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    for shader in blocks_by_type(tree, "BSShaderProperty", true)? {
        if tree.native_values(shader, "Shader Flags 1")?.to_i64()? == 0 {
            log.push(format!("\t{}: Empty shader flags", name(tree, shader)?));
        }
        if block_type(tree, shader) == "BSShaderPPLightingProperty" {
            if flag(tree, shader, "Shader Flags 1\\Environment_Mapping")?
                && !flag(tree, shader, "Shader Flags 2\\Envmap_Light_Fade")?
            {
                log.push(format!(
                    "\t{}: Environment_Mapping flag is set but no Envmap_Light_Fade flag",
                    name(tree, shader)?
                ));
            }
            let Some(texset) = link(tree, shader, "Texture Set")? else {
                continue;
            };
            let Some(textures) = tree.elements(texset, "Textures")? else {
                continue;
            };
            if tree.count(textures) > 5 {
                let t4 = tree.item(textures, 4)?;
                let t5 = tree.item(textures, 5)?;
                let (e4, e5) = (tree.edit_value(t4)?.is_empty(), tree.edit_value(t5)?.is_empty());
                if e4 != e5 {
                    log.push(format!(
                        "\t{}: 6th texture (counting from 1) should be used if 5th is set in FO3/FNV meshes otherwise the game uses the gloss map from the normal map",
                        name(tree, texset)?
                    ));
                }
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckUVs`.
fn check_uvs(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    for i in 0..blocks_count(tree)? {
        let mut b = block(tree, i)?;
        if !(block_is_ni_object(tree, b, "NiTriBasedGeom", true) || block_is_ni_object(tree, b, "BSTriShape", true)) {
            continue;
        }
        let shader;
        let mode;
        if version(tree) > NifVersion::Tes4 {
            let Some(found) = block_property_by_type(tree, b, "BSShaderProperty", true)? else {
                continue;
            };
            if block_type(tree, found) == "BSEffectShaderProperty" {
                continue;
            }
            if version(tree) >= NifVersion::Fo4 && !tree.edit_values(found, "Name")?.is_empty() {
                continue;
            }
            shader = found;
            mode = tree.elements(found, "Texture Clamp Mode")?;
        } else {
            let Some(found) = block_property_by_type(tree, b, "NiTexturingProperty", false)? else {
                continue;
            };
            shader = found;
            mode = tree.elements(found, "Base Texture\\Clamp Mode")?;
        }
        let Some(mode) = mode else { continue };
        let m = tree.edit_value(mode)?;
        if m == "WRAP_S_WRAP_T" {
            continue;
        }
        if block_is_ni_object(tree, b, "NiTriBasedGeom", true) {
            let Some(data) = link(tree, b, "Data")? else { continue };
            b = data;
        }
        let uvs = block_get_tex_coord(tree, b, None)?;
        let clamp_s = m == "CLAMP_S_CLAMP_T" || m == "CLAMP_S_WRAP_T";
        let clamp_t = m == "CLAMP_S_CLAMP_T" || m == "WRAP_S_CLAMP_T";
        for uv in uvs {
            if ((uv.v[0] < -0.001 || uv.v[0] > 1.001) && clamp_s) || ((uv.v[1] < -0.001 || uv.v[1] > 1.001) && clamp_t)
            {
                log.push(format!(
                    "\t{}: Has UVs outside of 0..1 range but uses CLAMP mode in {}",
                    name(tree, b)?,
                    name(tree, shader)?
                ));
                break;
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckStrips` of `CheckStripsDegenerate`: three repeats in a row.
fn degenerate_strips(tree: &mut Tree, strips: Option<El>) -> R<bool> {
    let Some(strips) = strips else { return Ok(false) };
    for strip in items(tree, strips)? {
        let mut v = 0;
        let points = items(tree, strip)?;
        for k in 1..points.len() {
            if tree.native_value(points[k - 1])? == tree.native_value(points[k])? {
                v += 1;
                if v == 3 {
                    return Ok(true);
                }
            } else {
                v = 0;
            }
        }
    }
    Ok(false)
}

/// `CheckStripsDegenerate`.
fn check_strips_degenerate(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    for i in 0..blocks_count(tree)? {
        let b = block(tree, i)?;
        if block_type(tree, b) == "NiTriStripsData" {
            let strips = tree.elements(b, "Strips")?;
            if degenerate_strips(tree, strips)? {
                log.push(format!("\t{}: Repeated degenerate tris in strip", name(tree, b)?));
            }
        } else if block_type(tree, b) == "NiSkinPartition" {
            let Some(parts) = tree.elements(b, "Partitions")? else {
                continue;
            };
            for part in items(tree, parts)? {
                let strips = tree.elements(part, "Strips")?;
                if degenerate_strips(tree, strips)? {
                    log.push(format!("\t{}: Repeated degenerate tris in strip", name(tree, b)?));
                    break;
                }
            }
        }
    }
    Ok(())
}

// ===========================================================================
/// `CheckSSENifFormat`.
fn check_sse_nif_format(_: &ProcFileObject, nif: &mut NifFile, log: &mut Vec<String>) -> R<()> {
    let tree = &mut nif.tree;
    for i in 0..blocks_count(tree)? {
        let b = block(tree, i)?;
        match block_type(tree, b) {
            "NiTriStrips" => log.push(format!("\t{}: NiTriStrips block crashes Skyrim SE", name(tree, b)?)),
            "bhkMultiSphereShape" => {
                log.push(format!(
                    "\t{}: bhkMultiSphereShape block crashes Skyrim SE",
                    name(tree, b)?
                ));
            }
            "NiSkinPartition" => {
                if let Some(parts) = tree.elements(b, "Partitions")? {
                    for part in items(tree, parts)? {
                        if tree.native_values(part, "Num Strips")?.to_i64()? != 0 {
                            log.push(format!(
                                "\t{}: NiSkinPartition block with triangle strips crashes Skyrim SE",
                                name(tree, b)?
                            ));
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

impl Proc for ProcCheckForErrors {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        // Each check is a setting named after it.
        for (index, check) in CHECKS.iter().enumerate() {
            self.active[index] = storage.get_bool(check.name, check.active);
        }
    }

    fn on_start(&mut self) -> R<()> {
        self.load_nif = false;
        self.load_dds = false;
        for (index, check) in CHECKS.iter().enumerate() {
            let on = self.active[index];
            self.load_nif = self.load_nif || (on && (check.does_extension(".nif") || check.does_extension(".kf")));
            self.load_dds = self.load_dds || (on && check.does_extension(".dds"));
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut log: Vec<String> = Vec::new();
        let ext = extract_file_ext(&file.file_name).to_owned();
        if self.load_dds && same_text(&ext, ".dds") {
            return Err(DfError::new(
                "The texture checks need wbDDS, which is not ported yet (phase 5 step 2)",
            ));
        }
        if self.load_nif && !same_text(&ext, ".dds") {
            let mut nif = NifFile::new()?;
            nif.load_from_data(&file.get_data()?)?;
            for (index, check) in CHECKS.iter().enumerate() {
                if self.active[index]
                    && check.does_extension(&ext)
                    && let Some(proc) = check.proc
                {
                    proc(file, &mut nif, &mut log)?;
                }
            }
        }
        if !log.is_empty() {
            log.insert(0, file.file_name.clone());
            log.push(String::new());
            ctx.add_messages(log);
        }
        Ok(Vec::new())
    }
}
