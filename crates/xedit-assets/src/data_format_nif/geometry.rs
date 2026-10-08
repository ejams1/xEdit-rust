// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDataFormatNif.pas (TwbNifBlock: GetVertices to
// RemoveBranch; TwbNifFile: ConvertBlock, GetAssets, GetAssetsList,
// GetLinkArrays, DetectBSXFlags, SpellFaceNormals, SpellUpdateTangents,
// SpellAddUpdateTangents)

//! The geometry of NIF blocks: vertices, normals, texture coordinates,
//! triangles and strips, transforms and bounds, the transform applied to
//! the geometry, normals and tangents computed again, branches removed,
//! and the asset and link lists of a file.
//!
//! `SpellOptimize`, `SpellStripify` and `SpellTriangulate` use
//! `wbMeshOptimize` and are ported with LOD generation (phase 5 step 6).

use super::*;
use crate::data_format::{df_float_to_str, df_str_to_float};
use crate::nif_math::{
    BoundSphere, Matrix33, Strip, Transform, Triangle, Vector2, Vector3, calculate_center_radius,
    calculate_face_normals, calculate_tangents_bitangents2, m33_to_quaternion, quaternion_to_m33, triangulate_strips,
};

/// `wbGetVector2`: from the native values, or from the text (`asText`).
pub fn get_vector2(tree: &mut Tree, el: El, as_text: bool) -> R<Vector2> {
    let mut vector = Vector2::default();
    for index in 0..2 {
        let path = format!("[{index}]");
        vector.v[index] = if as_text {
            df_str_to_float(&tree.edit_values(el, &path)?)?
        } else {
            tree.native_values(el, &path)?.to_f64()?
        };
    }
    Ok(vector)
}

/// `wbGetVector3`.
pub fn get_vector3(tree: &mut Tree, el: El, as_text: bool) -> R<Vector3> {
    let mut vector = Vector3::default();
    for index in 0..3 {
        let path = format!("[{index}]");
        vector.v[index] = if as_text {
            df_str_to_float(&tree.edit_values(el, &path)?)?
        } else {
            tree.native_values(el, &path)?.to_f64()?
        };
    }
    Ok(vector)
}

/// `wbSetVector3`.
pub fn set_vector3(tree: &mut Tree, vector: Vector3, el: El, as_text: bool) -> R<()> {
    for index in 0..3 {
        let path = format!("[{index}]");
        if as_text {
            tree.set_edit_values(el, &path, &df_float_to_str(vector.v[index]))?;
        } else {
            tree.set_native_values(el, &path, Variant::Float(vector.v[index]))?;
        }
    }
    Ok(())
}

/// `wbGetTriangle`.
pub fn get_triangle(tree: &mut Tree, el: El) -> R<Triangle> {
    let mut triangle = [0u32; 3];
    for (index, corner) in triangle.iter_mut().enumerate() {
        *corner = tree.native_values(el, &format!("[{index}]"))?.to_i64()? as u32;
    }
    Ok(triangle)
}

/// `wbSetTriangle`.
pub fn set_triangle(tree: &mut Tree, triangle: Triangle, el: El) -> R<()> {
    for (index, corner) in triangle.iter().enumerate() {
        tree.set_native_values(el, &format!("[{index}]"), Variant::Int(i64::from(*corner)))?;
    }
    Ok(())
}

fn flag(tree: &mut Tree, el: El, path: &str) -> R<bool> {
    tree.native_values(el, path)?.to_bool()
}

fn children(tree: &mut Tree, el: El) -> R<Vec<El>> {
    let mut items = Vec::new();
    for index in 0..tree.count(el) {
        items.push(tree.item(el, index)?);
    }
    Ok(items)
}

/// `GetVertices`: of the block, or of `element` (a skin partition).
pub fn block_get_vertices(tree: &mut Tree, block: El, element: Option<El>) -> R<Vec<Vector3>> {
    let el = element.unwrap_or(block);
    if block_type(tree, block) == "BSDynamicTriShape" {
        let Some(entries) = tree.elements(el, "Dynamic Vertices")? else {
            return Ok(Vec::new());
        };
        let mut result = Vec::new();
        for entry in children(tree, entries)? {
            result.push(get_vector3(tree, entry, false)?);
        }
        Ok(result)
    } else if block_is_ni_object(tree, block, "BSTriShape", true)
        || block_is_ni_object(tree, block, "NiSkinPartition", true)
    {
        if !flag(tree, el, "VertexDesc\\VF\\VF_VERTEX")? {
            return Ok(Vec::new());
        }
        let Some(entries) = tree.elements(el, "Vertex Data")? else {
            return Ok(Vec::new());
        };
        let mut result = Vec::new();
        for entry in children(tree, entries)? {
            // The vertex is the first member; upstream reads it as text.
            let vertex = tree.item(entry, 0)?;
            result.push(get_vector3(tree, vertex, true)?);
        }
        Ok(result)
    } else if block_is_ni_object(tree, block, "NiTriBasedGeomData", true)
        || block_type(tree, block) == "hkPackedNiTriStripsData"
        || block_type(tree, block) == "bhkConvexVerticesShape"
    {
        let Some(entries) = tree.elements(el, "Vertices")? else {
            return Ok(Vec::new());
        };
        let mut result = Vec::new();
        for entry in children(tree, entries)? {
            result.push(get_vector3(tree, entry, false)?);
        }
        Ok(result)
    } else {
        Ok(Vec::new())
    }
}

/// `GetNormals`.
pub fn block_get_normals(tree: &mut Tree, block: El, element: Option<El>) -> R<Vec<Vector3>> {
    let el = element.unwrap_or(block);
    if block_is_ni_object(tree, block, "BSTriShape", true) || block_type(tree, block) == "NiSkinPartition" {
        if !flag(tree, el, "VertexDesc\\VF\\VF_NORMAL")? {
            return Ok(Vec::new());
        }
        let Some(entries) = tree.elements(el, "Vertex Data")? else {
            return Ok(Vec::new());
        };
        let mut result = Vec::new();
        for entry in children(tree, entries)? {
            let normal = req(tree.elements(entry, "Normal")?)?;
            result.push(get_vector3(tree, normal, true)?);
        }
        Ok(result)
    } else if block_is_ni_object(tree, block, "NiTriBasedGeomData", true) {
        let Some(entries) = tree.elements(el, "Normals")? else {
            return Ok(Vec::new());
        };
        let mut result = Vec::new();
        for entry in children(tree, entries)? {
            result.push(get_vector3(tree, entry, false)?);
        }
        Ok(result)
    } else {
        Ok(Vec::new())
    }
}

/// `GetTexCoord`: the first UV set.
pub fn block_get_tex_coord(tree: &mut Tree, block: El, element: Option<El>) -> R<Vec<Vector2>> {
    let el = element.unwrap_or(block);
    if block_is_ni_object(tree, block, "BSTriShape", true) || block_type(tree, block) == "NiSkinPartition" {
        if !flag(tree, el, "VertexDesc\\VF\\VF_UV")? {
            return Ok(Vec::new());
        }
        let Some(entries) = tree.elements(el, "Vertex Data")? else {
            return Ok(Vec::new());
        };
        let mut result = Vec::new();
        for entry in children(tree, entries)? {
            let uv = req(tree.elements(entry, "UV")?)?;
            result.push(get_vector2(tree, uv, true)?);
        }
        Ok(result)
    } else if block_is_ni_object(tree, block, "NiTriBasedGeomData", true) {
        let mut result = Vec::new();
        if let Some(sets) = tree.elements(el, "UV Sets")?
            && tree.count(sets) != 0
        {
            let first = tree.item(sets, 0)?;
            for entry in children(tree, first)? {
                result.push(get_vector2(tree, entry, false)?);
            }
        }
        Ok(result)
    } else {
        Ok(Vec::new())
    }
}

/// `GetTriangles`: the triangles, or the strips triangulated. A skin
/// partition block gives the triangles of all its partitions.
pub fn block_get_triangles(tree: &mut Tree, block: El, element: Option<El>) -> R<Vec<Triangle>> {
    let el = match element {
        Some(el) => el,
        None => {
            // From the partitions, only without an element to avoid recursion.
            if block_type(tree, block) == "NiSkinPartition" {
                let mut result = Vec::new();
                if let Some(partitions) = tree.elements(block, "Partitions")? {
                    for partition in children(tree, partitions)? {
                        result.extend(block_get_triangles(tree, block, Some(partition))?);
                    }
                }
                return Ok(result);
            }
            block
        }
    };
    match tree.elements(el, "Triangles")? {
        Some(entries) => {
            let mut result = Vec::new();
            for entry in children(tree, entries)? {
                result.push(get_triangle(tree, entry)?);
            }
            Ok(result)
        }
        None => Ok(triangulate_strips(&block_get_strips(tree, block, Some(el))?)),
    }
}

/// `SetTriangles`.
pub fn block_set_triangles(tree: &mut Tree, block: El, tris: &[Triangle], element: Option<El>) -> R<bool> {
    let el = element.unwrap_or(block);
    let Some(num_triangles) = tree.elements(el, "Num Triangles")? else {
        return Ok(false);
    };
    let max_tris: u64 = if tree.data_size(num_triangles)? == 4 {
        u64::from(u32::MAX)
    } else {
        u64::from(u16::MAX)
    };
    if tris.len() as u64 > max_tris {
        let name = tree.name(block)?;
        return Err(DfError::new(format!(
            "{name}: Num Triangles {} > {max_tris}",
            tris.len()
        )));
    }
    let Some(entries) = tree.elements(el, "Triangles")? else {
        return Ok(false);
    };
    tree.set_count(entries, tris.len() as i32)?;
    let count = tree.count(entries);
    tree.set_native_value(num_triangles, Variant::Int(i64::from(count)))?;
    for (index, entry) in children(tree, entries)?.into_iter().enumerate() {
        set_triangle(tree, tris[index], entry)?;
    }
    let Some(entries) = tree.elements(el, "Triangles Copy")? else {
        return Ok(false);
    };
    tree.set_count(entries, tris.len() as i32)?;
    for (index, entry) in children(tree, entries)?.into_iter().enumerate() {
        set_triangle(tree, tris[index], entry)?;
    }
    Ok(true)
}

/// `GetStrips`.
pub fn block_get_strips(tree: &mut Tree, block: El, element: Option<El>) -> R<Vec<Strip>> {
    let el = element.unwrap_or(block);
    let mut result = Vec::new();
    if let Some(entries) = tree.elements(el, "Strips")? {
        for strip in children(tree, entries)? {
            let mut points = Vec::new();
            for point in children(tree, strip)? {
                points.push(tree.native_value(point)?.to_i64()? as u32);
            }
            result.push(points);
        }
    }
    Ok(result)
}

/// `SetStrips`.
pub fn block_set_strips(tree: &mut Tree, block: El, strips: &[Strip], element: Option<El>) -> R<bool> {
    let el = element.unwrap_or(block);
    tree.set_native_values(el, "Num Strips", Variant::Int(strips.len() as i64))?;
    if strips.is_empty() {
        return Ok(false);
    }
    tree.set_native_values(el, "Has Points", Variant::Int(1))?;
    let tris: i64 = strips.iter().map(|strip| strip.len() as i64 - 2).sum();
    if tris > i64::from(u16::MAX) {
        let name = tree.name(block)?;
        return Err(DfError::new(format!("{name}: Num Triangles {tris} > 65535")));
    }
    tree.set_native_values(el, "Num Triangles", Variant::Int(tris))?;
    let Some(entries) = tree.elements(el, "Strips")? else {
        return Ok(false);
    };
    tree.set_count(entries, strips.len() as i32)?;
    for (index, strip) in children(tree, entries)?.into_iter().enumerate() {
        tree.set_count(strip, strips[index].len() as i32)?;
        for (point_index, point) in children(tree, strip)?.into_iter().enumerate() {
            tree.set_native_value(point, Variant::Int(i64::from(strips[index][point_index])))?;
        }
    }
    let Some(entries) = tree.elements(el, "Strip Lengths")? else {
        return Ok(false);
    };
    tree.set_count(entries, strips.len() as i32)?;
    for (index, entry) in children(tree, entries)?.into_iter().enumerate() {
        tree.set_native_value(entry, Variant::Int(strips[index].len() as i64))?;
    }
    Ok(true)
}

fn matrix_member(i: usize, j: usize) -> String {
    format!("m{}{}", i + 1, j + 1)
}

/// `GetTransform`. UPSTREAM-QUIRK: a block that is neither an NiAVObject
/// nor a `bhkRigidBodyT` reports a transform it did not fill.
#[allow(clippy::needless_range_loop)]
pub fn block_get_transform(tree: &mut Tree, block: El, transform: &mut Transform) -> R<bool> {
    if block_is_ni_object(tree, block, "NiAVObject", true) {
        let Some(t) = tree.elements(block, "Transform")? else {
            return Ok(false);
        };
        transform.scale = tree.native_values(t, "Scale")?.to_f64()? as f32;
        transform.translation = Vector3::new(
            tree.native_values(t, "Translation\\X")?.to_f64()?,
            tree.native_values(t, "Translation\\Y")?.to_f64()?,
            tree.native_values(t, "Translation\\Z")?.to_f64()?,
        );
        let r = req(tree.elements(t, "Rotation")?)?;
        let mut m: Matrix33 = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m[j][i] = tree.native_values(r, &matrix_member(i, j))?.to_f64()?;
            }
        }
        transform.rotation = m33_to_quaternion(&m);
    } else if block_type(tree, block) == "bhkRigidBodyT" {
        transform.translation = Vector3::new(
            tree.native_values(block, "Translation\\X")?.to_f64()?,
            tree.native_values(block, "Translation\\Y")?.to_f64()?,
            tree.native_values(block, "Translation\\Z")?.to_f64()?,
        );
        transform.rotation.q[1] = tree.native_values(block, "Rotation\\X")?.to_f64()?;
        transform.rotation.q[2] = tree.native_values(block, "Rotation\\Y")?.to_f64()?;
        transform.rotation.q[3] = tree.native_values(block, "Rotation\\Z")?.to_f64()?;
        transform.rotation.q[0] = tree.native_values(block, "Rotation\\W")?.to_f64()?;
        transform.scale = 1.0;
    }
    Ok(true)
}

/// `SetTransform`.
#[allow(clippy::needless_range_loop)]
pub fn block_set_transform(tree: &mut Tree, block: El, transform: &Transform) -> R<bool> {
    if !block_is_ni_object(tree, block, "NiAVObject", true) {
        return Ok(false);
    }
    let Some(t) = tree.elements(block, "Transform")? else {
        return Ok(false);
    };
    tree.set_native_values(t, "Scale", Variant::Float(f64::from(transform.scale)))?;
    tree.set_native_values(t, "Translation\\X", Variant::Float(transform.translation.x()))?;
    tree.set_native_values(t, "Translation\\Y", Variant::Float(transform.translation.y()))?;
    tree.set_native_values(t, "Translation\\Z", Variant::Float(transform.translation.z()))?;
    let m = quaternion_to_m33(&transform.rotation);
    let r = req(tree.elements(t, "Rotation")?)?;
    for i in 0..3 {
        for j in 0..3 {
            tree.set_native_values(r, &matrix_member(i, j), Variant::Float(m[j][i]))?;
        }
    }
    Ok(true)
}

/// `GetBoundSphere`.
pub fn block_get_bound_sphere(tree: &mut Tree, block: El, sphere: &mut BoundSphere) -> R<bool> {
    if !block_is_ni_object(tree, block, "NiGeometryData", true) && !block_is_ni_object(tree, block, "BSTriShape", true)
    {
        return Ok(false);
    }
    let Some(e) = tree.elements(block, "Bounding Sphere")? else {
        return Ok(false);
    };
    let center = req(tree.elements(e, "Center")?)?;
    sphere.center = get_vector3(tree, center, false)?;
    sphere.radius = tree.native_values(e, "Radius")?.to_f64()? as f32;
    Ok(true)
}

/// `SetBoundSphere`.
pub fn block_set_bound_sphere(tree: &mut Tree, block: El, sphere: &BoundSphere) -> R<bool> {
    if !block_is_ni_object(tree, block, "NiGeometryData", true) && !block_is_ni_object(tree, block, "BSTriShape", true)
    {
        return Ok(false);
    }
    let Some(e) = tree.elements(block, "Bounding Sphere")? else {
        return Ok(false);
    };
    let center = req(tree.elements(e, "Center")?)?;
    set_vector3(tree, sphere.center, center, false)?;
    tree.set_native_values(e, "Radius", Variant::Float(f64::from(sphere.radius)))?;
    Ok(true)
}

fn linked(tree: &mut Tree, reference: Option<El>) -> R<Option<El>> {
    match reference {
        Some(reference) => tree.links_to(reference),
        None => Ok(None),
    }
}

/// `GetSkin`.
pub fn block_get_skin(tree: &mut Tree, block: El) -> R<Option<El>> {
    let e = match tree.elements(block, "Skin")? {
        Some(e) => Some(e),
        None => tree.elements(block, "Skin Instance")?,
    };
    linked(tree, e)
}

/// `GetCollision`.
pub fn block_get_collision(tree: &mut Tree, block: El) -> R<Option<El>> {
    if !block_is_ni_object(tree, block, "NiAVObject", true) {
        return Ok(None);
    }
    let e = tree.elements(block, "Collision Object")?;
    linked(tree, e)
}

/// `GetController`: the first controller, or the first of `block_type`
/// along the chain when `chained`.
pub fn block_get_controller(tree: &mut Tree, block: El, controller_type: &str, chained: bool) -> R<Option<El>> {
    if !block_is_ni_object(tree, block, "NiObjectNET", true) {
        return Ok(None);
    }
    let first = req(tree.elements(block, "Controller")?)?;
    let mut result = tree.links_to(first)?;
    while let Some(current) = result {
        if controller_type.is_empty() || block_type(tree, current) == controller_type {
            return Ok(Some(current));
        }
        if !chained || !block_is_ni_object(tree, current, "NiTimeController", true) {
            return Ok(Some(current));
        }
        let next = req(tree.elements(current, "Next Controller")?)?;
        result = tree.links_to(next)?;
    }
    Ok(None)
}

/// `CanTransform` of `ApplyTransform`.
fn can_transform(tree: &mut Tree, block: El, options: ApplyTransformOptions) -> R<bool> {
    if Some(block) == root_nodes(tree)?.first().copied() {
        return Ok(false);
    }
    // Skip skinned, animated and nodes with collision.
    let mut result = (options.skinned || (block_get_skin(tree, block)?.is_none() && !block_is_bone(tree, block)?))
        && (options.animated || block_get_controller(tree, block, "", false)?.is_none())
        && (options.collision || block_get_collision(tree, block)?.is_none())
        && (options.ctrl_manager || block_by_type(tree, "NiControllerManager", false)?.is_none());
    // A root node with bone nodes as children: checked by the presence of
    // skin instances.
    if result && !options.root && tree.index(block)? == 0 {
        result = block_by_type(tree, "NiSkinInstance", true)?.is_none()
            && block_by_type(tree, "BSSkin::Instance", true)?.is_none();
    }
    Ok(result)
}

/// `TwbApplyTransformOptions`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApplyTransformOptions {
    pub skinned: bool,
    pub animated: bool,
    pub collision: bool,
    pub root: bool,
    pub ctrl_manager: bool,
}

/// `UpdateRotation` of `ApplyTransform`.
fn update_rotation(tree: &mut Tree, entries: Option<El>, transform: &Transform) -> R<()> {
    let Some(entries) = entries else { return Ok(()) };
    for entry in children(tree, entries)? {
        let v = get_vector3(tree, entry, false)? * transform.rotation;
        set_vector3(tree, v, entry, false)?;
    }
    Ok(())
}

/// `ApplyTransform`: moves the transform of a shape into its geometry, or
/// of a node into its children.
pub fn block_apply_transform(tree: &mut Tree, block: El, recursive: bool, options: ApplyTransformOptions) -> R<bool> {
    let mut t = Transform::default();
    let mut s = BoundSphere::default();
    if block_is_ni_object(tree, block, "NiTriBasedGeom", true) {
        // NiTriShape, NiTriStrips
        if !can_transform(tree, block, options)? {
            return Ok(false);
        }
        let data_ref = req(tree.elements(block, "Data")?)?;
        let Some(data) = tree.links_to(data_ref)? else {
            return Ok(false);
        };
        if !block_get_transform(tree, block, &mut t)? || t.is_none() {
            return Ok(false);
        }
        if let Some(verts) = tree.elements(data, "Vertices")? {
            for entry in children(tree, verts)? {
                let v = get_vector3(tree, entry, false)? * t;
                set_vector3(tree, v, entry, false)?;
            }
        }
        if !t.rotation.is_identity() {
            for name in ["Normals", "Tangents", "Bitangents"] {
                let entries = tree.elements(data, name)?;
                update_rotation(tree, entries, &t)?;
            }
            // Oblivion tangents and binormals in extra data.
            if let Some(exdata) = block_extra_data_by_name(tree, block, TES4_TANGENTS_EXTRA_DATA_NAME)? {
                let mut bytes = tree.native_values(exdata, "Data")?.to_bytes()?;
                for chunk in bytes.as_chunks_mut::<12>().0 {
                    let read =
                        |offset: usize| f64::from(f32::from_le_bytes(chunk[offset..offset + 4].try_into().unwrap()));
                    let v = Vector3::new(read(0), read(4), read(8)) * t.rotation;
                    for (axis, value) in v.v.iter().enumerate() {
                        chunk[axis * 4..axis * 4 + 4].copy_from_slice(&(*value as f32).to_le_bytes());
                    }
                }
                tree.set_native_values(exdata, "Data", Variant::Bytes(bytes))?;
            }
        }
        if block_get_bound_sphere(tree, data, &mut s)? {
            let s = s * t;
            block_set_bound_sphere(tree, data, &s)?;
        }
        t.set_none();
        block_set_transform(tree, block, &t)?;
        Ok(true)
    } else if block_is_ni_object(tree, block, "BSTriShape", true) {
        if tree.native_values(block, "Num Vertices")? == 0 && block_get_skin(tree, block)?.is_some() {
            return Ok(false);
        }
        if !can_transform(tree, block, options)? {
            return Ok(false);
        }
        if !block_get_transform(tree, block, &mut t)? || t.is_none() {
            return Ok(false);
        }
        let Some(verts) = tree.elements(block, "Vertex Data")? else {
            return Ok(false);
        };
        // UPSTREAM-QUIRK: whether the vertices move is read from the normal
        // flag.
        let has_vertex = flag(tree, block, "VertexDesc\\VF\\VF_NORMAL")?;
        let has_normal = flag(tree, block, "VertexDesc\\VF\\VF_NORMAL")?;
        let has_tangent = flag(tree, block, "VertexDesc\\VF\\VF_TANGENT")?;
        for e in children(tree, verts)? {
            if has_vertex {
                let vertex = req(tree.elements(e, "Vertex")?)?;
                let v = get_vector3(tree, vertex, true)? * t;
                set_vector3(tree, v, vertex, true)?;
            }
            if has_normal {
                let normal = req(tree.elements(e, "Normal")?)?;
                let v = get_vector3(tree, normal, true)? * t.rotation;
                set_vector3(tree, v, normal, true)?;
            }
            if has_tangent {
                let tangent = req(tree.elements(e, "Tangent")?)?;
                let v = get_vector3(tree, tangent, true)? * t.rotation;
                set_vector3(tree, v, tangent, true)?;
                let bitangent: Vec<El> = ["Bitangent X", "Bitangent Y", "Bitangent Z"]
                    .iter()
                    .map(|name| tree.elements(e, name).and_then(req))
                    .collect::<R<_>>()?;
                let mut v = Vector3::default();
                for (axis, &el) in bitangent.iter().enumerate() {
                    v.v[axis] = df_str_to_float(&tree.edit_value(el)?)?;
                }
                let v = v * t.rotation;
                for (axis, &el) in bitangent.iter().enumerate() {
                    tree.set_edit_value(el, &df_float_to_str(v.v[axis]))?;
                }
            }
        }
        if block_get_bound_sphere(tree, block, &mut s)? {
            let s = s * t;
            block_set_bound_sphere(tree, block, &s)?;
        }
        t.set_none();
        block_set_transform(tree, block, &t)?;
        Ok(true)
    } else if block_is_ni_object(tree, block, "NiNode", true) {
        // Billboards are skipped.
        if block_type(tree, block) == "NiBillboardNode" {
            return Ok(false);
        }
        if !block_get_transform(tree, block, &mut t)? {
            return Ok(false);
        }
        let mut can_apply = can_transform(tree, block, options)?;
        let mut child_transformed = false;
        let mut result = false;
        let children_list = req(tree.elements(block, "Children")?)?;
        let entries = children(tree, children_list)?;
        // A parent of an animated node keeps its transform.
        if can_apply && !options.animated {
            for &entry in &entries {
                let Some(child) = tree.links_to(entry)? else { continue };
                can_apply = block_get_controller(tree, child, "", false)?.is_none();
                if !can_apply {
                    break;
                }
            }
        }
        for &entry in &entries {
            let Some(child) = tree.links_to(entry)? else { continue };
            // Allowed and there is a transform to apply.
            if can_apply && !t.is_none() {
                let mut child_t = Transform::default();
                if block_get_transform(tree, child, &mut child_t)? {
                    let child_t = t * child_t;
                    block_set_transform(tree, child, &child_t)?;
                    child_transformed = true;
                    result = true;
                }
            }
            if recursive {
                result = block_apply_transform(tree, child, recursive, options)? || result;
            }
        }
        // Our transform is reset only when a child took it.
        if child_transformed && !t.is_none() {
            t.set_none();
            block_set_transform(tree, block, &t)?;
            result = true;
        }
        Ok(result)
    } else {
        Ok(false)
    }
}

/// `UpdateBounds`: the bounding sphere from the vertices.
pub fn block_update_bounds(tree: &mut Tree, block: El) -> R<bool> {
    let verts = block_get_vertices(tree, block, None)?;
    // Oblivion and volatile meshes would need another center algorithm;
    // upstream always takes the center of the bounding box.
    let (center, r) = calculate_center_radius(&verts, true, true);
    let sphere = BoundSphere {
        center,
        radius: r as f32,
    };
    block_set_bound_sphere(tree, block, &sphere)
}

/// The geometry block of a shape for `UpdateNormals` and `UpdateTangents`.
fn geometry_block(tree: &mut Tree, block: El, is_bs_tri_shape: bool, is_tri_geom: bool) -> R<Option<El>> {
    if is_bs_tri_shape {
        if let Some(shader) = block_property_by_type(tree, block, "BSShaderProperty", true)?
            && flag(tree, shader, "Shader Flags 1\\Model_Space_Normals")?
        {
            return Ok(None);
        }
        let mut result = Some(block);
        // A skinned SSE shape has its geometry in the skin partition; a
        // partition reference to None leaves no block.
        if tree.nif.nif_version == NifVersion::Sse
            && let Some(skin) = block_get_skin(tree, block)?
            && let Some(partition) = tree.elements(skin, "Skin Partition")?
        {
            result = tree.links_to(partition)?;
        }
        Ok(result)
    } else if is_tri_geom {
        let data = tree.elements(block, "Data")?;
        linked(tree, data)
    } else {
        Ok(None)
    }
}

/// `UpdateNormals`: the normals as the normalized sums of the face normals.
pub fn block_update_normals(tree: &mut Tree, block: El, add_if_missing: bool) -> R<bool> {
    let is_bs_tri_shape = block_is_ni_object(tree, block, "BSTriShape", true);
    let is_tri_geom = block_is_ni_object(tree, block, "NiTriBasedGeom", true);
    let Some(geometry) = geometry_block(tree, block, is_bs_tri_shape, is_tri_geom)? else {
        return Ok(false);
    };
    if !add_if_missing
        && ((is_bs_tri_shape && !flag(tree, geometry, "VertexDesc\\VF\\VF_NORMAL")?)
            || (is_tri_geom && tree.native_values(geometry, "Has Normals")? == 0))
    {
        return Ok(false);
    }
    let verts = block_get_vertices(tree, geometry, None)?;
    if verts.is_empty() {
        return Ok(false);
    }
    let triangles = block_get_triangles(tree, geometry, None)?;
    if triangles.is_empty() {
        return Ok(false);
    }
    let norms = calculate_face_normals(&verts, &triangles)?;
    if norms.is_empty() {
        return Ok(false);
    }
    if is_bs_tri_shape {
        tree.set_native_values(geometry, "VertexDesc\\VF\\VF_NORMAL", Variant::Int(1))?;
        let entries = req(tree.elements(geometry, "Vertex Data")?)?;
        for (index, entry) in children(tree, entries)?.into_iter().enumerate() {
            let normal = req(tree.elements(entry, "Normal")?)?;
            set_vector3(tree, norms[index], normal, true)?;
        }
    } else if is_tri_geom {
        tree.set_native_values(geometry, "Has Normals", Variant::Int(1))?;
        let entries = req(tree.elements(geometry, "Normals")?)?;
        tree.set_count(entries, norms.len() as i32)?;
        for (index, entry) in children(tree, entries)?.into_iter().enumerate() {
            set_vector3(tree, norms[index], entry, false)?;
        }
    }
    Ok(true)
}

/// `UpdateTangents`: tangents and bitangents from the normals and the
/// texture coordinates.
pub fn block_update_tangents(tree: &mut Tree, block: El, add_if_missing: bool) -> R<bool> {
    if tree.nif.nif_version <= NifVersion::Tes3 {
        return Ok(false);
    }
    let is_bs_tri_shape = block_is_ni_object(tree, block, "BSTriShape", true);
    let is_tri_geom = block_is_ni_object(tree, block, "NiTriBasedGeom", true);
    let Some(geometry) = geometry_block(tree, block, is_bs_tri_shape, is_tri_geom)? else {
        return Ok(false);
    };
    // Oblivion meshes do not use the tangents flag.
    if !add_if_missing
        && ((is_bs_tri_shape && !flag(tree, geometry, "VertexDesc\\VF\\VF_TANGENT")?)
            || (is_tri_geom
                && !(flag(tree, geometry, "Vector Flags\\Has_Tangents")? || tree.nif.nif_version == NifVersion::Tes4)))
    {
        return Ok(false);
    }
    let verts = block_get_vertices(tree, geometry, None)?;
    if verts.is_empty() {
        return Ok(false);
    }
    let mut norms = block_get_normals(tree, geometry, None)?;
    if norms.is_empty() {
        return Ok(false);
    }
    let mut texco = block_get_tex_coord(tree, geometry, None)?;
    if texco.is_empty() {
        return Ok(false);
    }
    let triangles = block_get_triangles(tree, geometry, None)?;
    if triangles.is_empty() {
        return Ok(false);
    }
    // The NifSkope equation with Unity's weighting.
    let (tan, bin) = calculate_tangents_bitangents2(&verts, &mut norms, &mut texco, &triangles)?;
    if tan.is_empty() {
        return Ok(false);
    }
    if is_bs_tri_shape {
        tree.set_native_values(geometry, "VertexDesc\\VF\\VF_TANGENT", Variant::Int(1))?;
        let entries = req(tree.elements(geometry, "Vertex Data")?)?;
        for (index, (tangent, bitangent)) in tan.iter().zip(&bin).enumerate() {
            let e = tree.item(entries, index as i32)?;
            // The members by index: Bitangent X, Y and Z and Tangent.
            let bx = tree.item(e, 1)?;
            tree.set_edit_value(bx, &df_float_to_str(bitangent.x()))?;
            let by = tree.item(e, 5)?;
            tree.set_edit_value(by, &df_float_to_str(bitangent.y()))?;
            let bz = tree.item(e, 7)?;
            tree.set_edit_value(bz, &df_float_to_str(bitangent.z()))?;
            let t = tree.item(e, 6)?;
            set_vector3(tree, *tangent, t, true)?;
        }
    } else if is_tri_geom {
        if tree.nif.nif_version != NifVersion::Tes4 {
            tree.set_native_values(geometry, "Vector Flags\\Has_Tangents", Variant::Int(1))?;
            for (name, values) in [("Tangents", &tan), ("Bitangents", &bin)] {
                let entries = req(tree.elements(geometry, name)?)?;
                tree.set_count(entries, values.len() as i32)?;
                for (index, entry) in children(tree, entries)?.into_iter().enumerate() {
                    set_vector3(tree, values[index], entry, false)?;
                }
            }
        } else {
            // Oblivion keeps tangents in an NiBinaryExtraData.
            let mut exdata = block_extra_data_by_name(tree, block, TES4_TANGENTS_EXTRA_DATA_NAME)?;
            if exdata.is_none() && add_if_missing {
                let added = block_add_extra_data(tree, block, "NiBinaryExtraData")?;
                tree.set_edit_values(added, "Name", TES4_TANGENTS_EXTRA_DATA_NAME)?;
                exdata = Some(added);
            }
            if let Some(exdata) = exdata {
                let mut bytes = Vec::with_capacity(12 * (tan.len() + bin.len()));
                for v in tan.iter().chain(&bin) {
                    for value in v.v {
                        bytes.extend_from_slice(&(value as f32).to_le_bytes());
                    }
                }
                tree.set_native_values(exdata, "Data", Variant::Bytes(bytes))?;
            }
        }
    }
    Ok(true)
}

/// `RemoveBranch`: removes the block and the blocks only it refers to.
pub fn block_remove_branch(tree: &mut Tree, block: El, even_if_used: bool) -> R<()> {
    fn gather(tree: &mut Tree, blocks: &mut Vec<El>, block: El) -> R<()> {
        // A block that other blocks use is kept.
        if block_is_referenced(tree, block)? {
            return Ok(());
        }
        if blocks.contains(&block) {
            return Ok(());
        }
        blocks.push(block);
        let mut refs = Vec::new();
        for reference in block_refs(tree, block) {
            // The root node is never removed.
            if let Some(target) = tree.links_to(reference)?
                && tree.index(target)? != 0
            {
                refs.push(target);
            }
            // Unlinked for the next `IsReferenced`.
            tree.set_native_value(reference, Variant::Int(-1))?;
        }
        for target in refs {
            gather(tree, blocks, target)?;
        }
        Ok(())
    }
    if even_if_used {
        for reference in block_referenced_by(tree, block)? {
            tree.set_native_value(reference, Variant::Int(-1))?;
        }
    }
    let mut branch = Vec::new();
    gather(tree, &mut branch, block)?;
    let root = tree.root_el();
    for block in branch {
        let index = tree.index(block)?;
        tree.delete(root, index)?;
    }
    Ok(())
}

/// `ConvertBlock`: replaces the block with a new block of another type that
/// takes over the values of the same names. Strips become triangles when
/// an `NiTriStripsData` becomes an `NiTriShapeData`.
pub fn convert_block(tree: &mut Tree, index: i32, new_type: &str) -> R<()> {
    let old = block(tree, index)?;
    if block_type(tree, old) == new_type {
        return Ok(());
    }
    let root = tree.root_el();
    let def = wb_ni_object_def(new_type)?;
    let new = tree.create_element(def, Some(root))?;
    tree.set_to_default(new)?;
    let triangulate = block_type(tree, old) == "NiTriStripsData" && block_type(tree, new) == "NiTriShapeData";
    let tris = if triangulate {
        triangulate_strips(&block_get_strips(tree, old, None)?)
    } else {
        Vec::new()
    };
    let assigned: R<()> = (|| {
        tree.assign(new, Some(old))?;
        if block_type(tree, new) == "bhkMalleableConstraint" {
            let name = match block_type(tree, old) {
                "bhkBallAndSocketConstraint" => "Ball and Socket",
                "bhkHingeConstraint" => "Hinge",
                "bhkLimitedHingeConstraint" => "Limited Hinge",
                "bhkPrismaticConstraint" => "Prismatic",
                "bhkRagdollConstraint" => "Ragdoll",
                "bhkStiffSpringConstraint" => "Stiff Spring",
                _ => "",
            };
            if !name.is_empty() {
                tree.set_edit_values(new, "Hinge\\Type", name)?;
                let hinge = tree.elements(new, "Hinge")?;
                // A copy of Entities and Priority.
                if let Some(hinge) = hinge {
                    tree.assign(hinge, Some(old))?;
                }
                let source = tree.elements(old, name)?;
                if let Some(target) = tree.elements(new, &format!("Hinge\\{name}"))? {
                    tree.assign(target, source)?;
                }
            }
        }
        Ok(())
    })();
    if assigned.is_err() {
        tree.free_element(new)?;
        return Err(tree.exception(root, &format!("Incompatible block type: {new_type}")));
    }
    // +1 for the header.
    tree.put(root, (index + 1) as usize, new);
    tree.free_element(old)?;
    if triangulate {
        tree.set_native_values(new, "Has Triangles", Variant::Int(1))?;
        tree.set_native_values(new, "Num Triangles", Variant::Int(tris.len() as i64))?;
        let entries = req(tree.elements(new, "Triangles")?)?;
        tree.set_count(entries, tris.len() as i32)?;
        for (i, entry) in children(tree, entries)?.into_iter().enumerate() {
            set_triangle(tree, tris[i], entry)?;
        }
    }
    Ok(())
}

/// `GetAssets`: the elements that name other files: textures, materials,
/// behaviour graphs and segment files.
pub fn get_assets(tree: &mut Tree) -> R<Vec<El>> {
    fn is_material(tree: &mut Tree, el: Option<El>) -> R<bool> {
        let Some(el) = el else { return Ok(false) };
        let value = tree.edit_value(el)?.to_ascii_lowercase();
        Ok(value.ends_with(".bgsm") || value.ends_with(".bgem"))
    }
    let mut result = Vec::new();
    let mut add = |el: Option<El>| result.extend(el);
    let fo4 = tree.nif.nif_version == NifVersion::Fo4;
    for index in 0..blocks_count(tree)? {
        let block = block(tree, index)?;
        let block_type = block_type(tree, block);
        if block_type == "BSShaderTextureSet" {
            let textures = req(tree.elements(block, "Textures")?)?;
            for texture in children(tree, textures)? {
                add(Some(texture));
            }
        } else if fo4 && block_type == "BSLightingShaderProperty" && {
            let name = tree.elements(block, "Name")?;
            is_material(tree, name)?
        } {
            add(tree.elements(block, "Name")?);
        } else if block_type == "BSEffectShaderProperty" {
            for name in [
                "Source Texture",
                "Grayscale Texture",
                "Env Map Texture",
                "Normal Texture",
                "Env Mask Texture",
            ] {
                add(tree.elements(block, name)?);
            }
            let name = tree.elements(block, "Name")?;
            if fo4 && is_material(tree, name)? {
                add(name);
            }
        } else if block_type == "BSShaderNoLightingProperty"
            || block_type == "TallGrassShaderProperty"
            || block_type == "TileShaderProperty"
            || block_is_ni_object(tree, block, "NiTexture", true)
        {
            add(tree.elements(block, "File Name")?);
        } else if block_type == "BSSkyShaderProperty" {
            add(tree.elements(block, "Source Texture")?);
        } else if block_type == "BSBehaviorGraphExtraData" {
            add(tree.elements(block, "Behavior Graph File")?);
        } else if block_type == "BSSubIndexTriShape" {
            add(tree.elements(block, "Segment Data\\SSF File")?);
        }
    }
    Ok(result)
}

/// `GetAssetsList`: the file names of `GetAssets`, without repeats.
pub fn get_assets_list(tree: &mut Tree) -> R<Vec<String>> {
    let mut result: Vec<String> = Vec::new();
    for asset in get_assets(tree)? {
        let name = tree.edit_value(asset)?;
        if name.is_empty() || result.iter().any(|known| known.eq_ignore_ascii_case(&name)) {
            continue;
        }
        result.push(name);
    }
    Ok(result)
}

/// `GetLinkArrays`: the arrays of block references.
pub fn get_link_arrays(tree: &mut Tree) -> R<Vec<El>> {
    let mut result = Vec::new();
    let footer = footer(tree)?;
    result.extend(tree.elements(footer, "Roots")?);
    const ARRAYS: &[(&str, &[&str])] = &[
        ("NiObjectNET", &["Extra Data List"]),
        ("NiAVObject", &["Properties"]),
        ("NiNode", &["Children", "Effects"]),
        ("BSTreeNode", &["Bones 1", "Bones"]),
        ("BSAnimNotes", &["Anim Notes"]),
        ("NiSkinInstance", &["Bones"]),
        ("bhkRigidBody", &["Constraints"]),
        ("bhkConvexListShape", &["Sub Shapes"]),
        ("bhkListShape", &["Sub Shapes"]),
        ("bhkMeshShape", &["Strips Data"]),
        ("bhkNiTriStripsShape", &["Strips Data"]),
        ("bhkRagdollTemplate", &["Bones"]),
        ("NiDynamicEffect", &["Affected Nodes"]),
    ];
    const LATER: &[(&str, &[&str])] = &[
        ("NiFlipController", &["Sources"]),
        ("NiControllerManager", &["Controller Sequences"]),
        ("NiControllerSequence", &["Anim Notes Array"]),
        ("NiParticleSystem", &["Modifiers"]),
        ("NiParticleMeshModifier", &["Particle Meshes"]),
        ("NiPSysMeshUpdateModifier", &["Meshes"]),
        ("BSPSysHavokUpdateModifier", &["Nodes"]),
        ("NiPSysMeshEmitter", &["Emitter Meshes"]),
        ("BSMasterParticleSystem", &["Particle Systems"]),
        ("BSSkin::Instance", &["Bones"]),
    ];
    for index in 0..blocks_count(tree)? {
        let block = block(tree, index)?;
        for (template, names) in ARRAYS {
            if block_is_ni_object(tree, block, template, true) {
                for name in *names {
                    result.extend(tree.elements(block, name)?);
                }
            }
        }
        if block_is_ni_object(tree, block, "NiBoneLODController", true) {
            for name in ["Node Groups", "Shade Groups 2"] {
                if let Some(el) = tree.elements(block, name)? {
                    result.extend(children(tree, el)?);
                }
            }
        }
        for (template, names) in LATER {
            if block_is_ni_object(tree, block, template, true) {
                for name in *names {
                    result.extend(tree.elements(block, name)?);
                }
            }
        }
    }
    Ok(result)
}

/// `DetectBSXFlags`: the BSX flags the blocks of the file call for.
pub fn detect_bsx_flags(tree: &mut Tree) -> R<u32> {
    let mut result: u32 = 0;
    let version = tree.nif.nif_version;
    if version < NifVersion::Fo3 {
        return Ok(result);
    }
    let (mut cols, mut constraints, mut controllers, mut bounds) = (0, 0, 0, 0);
    let (mut addons, mut dynbodies, mut markers, mut emitters) = (0, 0, 0, 0);
    for index in 0..blocks_count(tree)? {
        let b = block(tree, index)?;
        let is = |tree: &Tree, template: &str| block_is_ni_object(tree, b, template, true);
        if is(tree, "NiCollisionObject") {
            cols += 1;
        } else if is(tree, "bhkConstraint") || is(tree, "bhkBallSocketConstraintChain") {
            constraints += 1;
        } else if is(tree, "NiTimeController") {
            controllers += 1;
        } else if is(tree, "BSBound") {
            bounds += 1;
        } else if is(tree, "BSValueNode") {
            addons += 1;
        } else if is(tree, "BSShaderProperty") && flag(tree, b, "Shader Flags 1\\External_Emittance")? {
            emitters += 1;
        } else if version < NifVersion::Tes5 && block_is_ni_object(tree, b, "NiNode", false) {
            // Older games use NiNodes with special names.
            let name = tree.edit_values(b, "Name")?;
            if name.starts_with("FlameNode") || name.starts_with("AttachLight") {
                addons += 1;
            }
        }
        if block_is_dynamic_rigid_body(tree, b)? {
            dynbodies += 1;
        }
        if block_is_editor_marker(tree, b)? {
            markers += 1;
        }
    }
    // Animated, except skeletons, which have bounds.
    if (controllers > 0 || addons > 0) && bounds == 0 {
        result |= 1 << 0;
    }
    // Havok.
    if cols > 0 {
        result |= 1 << 1;
    }
    // Ragdoll.
    if constraints > 0 {
        result |= 1 << 2;
    }
    // Complex, used by grabbing, except skeletons.
    if dynbodies > 1 && bounds == 0 {
        result |= 1 << 3;
    }
    // Addon.
    if addons > 0 {
        result |= 1 << 4;
    }
    // Editor marker.
    if markers > 0 {
        result |= 1 << 5;
    }
    // Dynamic.
    if dynbodies > 0 {
        result |= 1 << 6;
    }
    // Articulated.
    if dynbodies > if version == NifVersion::Fo3 { 1 } else { 0 } {
        result |= 1 << 7;
    }
    // Emitters.
    if emitters > 0 {
        result |= 1 << 9;
    }
    Ok(result)
}

/// `SpellFaceNormals`.
pub fn spell_face_normals(tree: &mut Tree) -> R<bool> {
    let mut result = false;
    for index in 0..blocks_count(tree)? {
        let block = block(tree, index)?;
        result = block_update_normals(tree, block, true)? || result;
    }
    Ok(result)
}

/// `SpellUpdateTangents`.
pub fn spell_update_tangents(tree: &mut Tree) -> R<bool> {
    let mut result = false;
    for index in 0..blocks_count(tree)? {
        let block = block(tree, index)?;
        result = block_update_tangents(tree, block, false)? || result;
    }
    Ok(result)
}

/// `SpellAddUpdateTangents`.
pub fn spell_add_update_tangents(tree: &mut Tree) -> R<bool> {
    let mut result = false;
    for index in 0..blocks_count(tree)? {
        let block = block(tree, index)?;
        result = block_update_tangents(tree, block, true)? || result;
    }
    Ok(result)
}
