// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDataFormatNif.pas

//! The callbacks of `wbDataFormatNif` that the transpiler does not
//! translate.

use super::*;
use crate::data_format::split_string;
use crate::variant::str_to_int;

/// Upstream `RemoveNoneLinks`: before saving, drops the `None` entries of
/// a link array when the file collapses link arrays.
pub fn remove_none_links(t: &mut Tree, a_element: El) -> R<()> {
    if t.nif.internal_updates && t.nif.options.collapse_link_arrays && t.def(a_element)?.size <= 0 {
        for index in (0..t.count(a_element)).rev() {
            let item = t.item(a_element, index)?;
            if t.native_value(item)? < 0 {
                t.delete(a_element, index)?;
            }
        }
    }
    Ok(())
}

/// Upstream `wbString_OnCreate`.
pub fn wb_string_on_create(t: &mut Tree, a_element: El) -> R<()> {
    let block = nifblk_r(t, a_element)?;
    block_add_string(t, block, a_element);
    Ok(())
}

/// Upstream `wbString_OnDestroy`.
pub fn wb_string_on_destroy(t: &mut Tree, a_element: El) -> R<()> {
    let block = nifblk_r(t, a_element)?;
    block_remove_string(t, block, a_element);
    Ok(())
}

/// Upstream `wbString_AfterLoad`: a new string is `None`.
pub fn wb_string_after_load(t: &mut Tree, a_element: El, a_data_start: bool, _a_data_size: i32) -> R<()> {
    if !a_data_start {
        t.set_native_value(a_element, Variant::Int(-1))?;
    }
    Ok(())
}

/// Upstream `wbString_Get`: the string of the header's table.
pub fn wb_string_get(t: &mut Tree, a_element: El, a_text: &mut String) -> R<()> {
    let index = t.native_value(a_element)?.to_i32()?;
    if index == -1 {
        a_text.clear();
        return Ok(());
    }
    let header = header(t)?;
    let strings = t.elements(header, "Strings")?;
    match strings {
        Some(strings) if index < t.count(strings) => {
            let item = t.item(strings, index)?;
            *a_text = t.edit_value(item)?;
        }
        _ => *a_text = format!("<Error: Invalid string index {a_text}>"),
    }
    Ok(())
}

/// Upstream `wbString_Set`: the index of the string in the header's table,
/// added when missing.
pub fn wb_string_set(t: &mut Tree, a_element: El, a_text: &mut String) -> R<()> {
    if a_text.is_empty() {
        *a_text = "-1".to_owned();
        return Ok(());
    }
    let header = header(t)?;
    let Some(strings) = t.elements(header, "Strings")? else {
        return Err(t.exception(a_element, "Strings table not found in NiHeader"));
    };
    // Reuse an existing string.
    for index in 0..t.count(strings) {
        let item = t.item(strings, index)?;
        if t.edit_value(item)? == *a_text {
            *a_text = index.to_string();
            return Ok(());
        }
    }
    let item = t.add(strings)?;
    t.set_edit_value(item, a_text)?;
    *a_text = (t.count(strings) - 1).to_string();
    Ok(())
}

/// Upstream `wbNiRef_OnCreate`.
pub fn wb_ni_ref_on_create(t: &mut Tree, a_element: El) -> R<()> {
    let block = nifblk_r(t, a_element)?;
    block_add_ref(t, block, a_element);
    Ok(())
}

/// Upstream `wbNiRef_OnDestroy`.
pub fn wb_ni_ref_on_destroy(t: &mut Tree, a_element: El) -> R<()> {
    let block = nifblk_r(t, a_element)?;
    block_remove_ref(t, block, a_element);
    Ok(())
}

/// Upstream `wbNiRef_GetLinksTo`: the block the reference points to.
pub fn wb_ni_ref_get_links_to(t: &mut Tree, a_element: El) -> R<Option<El>> {
    let index = t.native_value(a_element)?.to_i32()?;
    if index >= 0 && index < blocks_count(t)? {
        Ok(Some(block(t, index)?))
    } else {
        Ok(None)
    }
}

/// Upstream `wbNiRef_GetText`: `None`, or the index with the block type and
/// name.
pub fn wb_ni_ref_get_text(t: &mut Tree, a_element: El, a_text: &mut String) -> R<()> {
    let index = t.native_value(a_element)?.to_i32()?;
    if index == -1 {
        *a_text = "None".to_owned();
    } else if let Some(block) = t.links_to(a_element)? {
        let name = t.edit_values(block, "Name")?;
        a_text.push(' ');
        a_text.push_str(block_type(t, block));
        if !name.is_empty() {
            a_text.push_str(&format!(" \"{name}\""));
        }
    }
    Ok(())
}

/// Upstream `wbNiRef_SetText`: the index from the text.
pub fn wb_ni_ref_set_text(_t: &mut Tree, _a_element: El, a_text: &mut String) -> R<()> {
    if !a_text.is_empty() {
        *a_text = split_string(a_text, " ").into_iter().next().unwrap_or_default();
    }
    if a_text.is_empty() || a_text == "None" {
        *a_text = "-1".to_owned();
    }
    Ok(())
}

// The vertex format flags (`VF_*`).
const VF_VERTEX: u16 = 1 << 4;
const VF_UV: u16 = 1 << 5;
const VF_NORMAL: u16 = 1 << 7;
const VF_TANGENT: u16 = 1 << 8;
const VF_COLORS: u16 = 1 << 9;
const VF_SKINNED: u16 = 1 << 10;
const VF_EYEDATA: u16 = 1 << 12;
const VF_FULLPREC: u16 = 1 << 14;

/// Upstream `wbVertexDesc_SetValue`: the vertex size and the offsets of
/// the vertex description from its flags.
pub fn wb_vertex_desc_set_value(t: &mut Tree, e: El, a_value: &mut Variant) -> R<()> {
    const SZ_BYTE: u8 = 1;
    const SZ_FLOAT: u8 = 4;
    const SZ_HALF_FLOAT: u8 = 2;
    let vf = a_value.to_i64()? as u16;
    let full_prec = vf & VF_FULLPREC > 0 || t.nif.nif_version == NifVersion::Sse;

    let mut ofst: u8 = 0;
    if vf & VF_VERTEX > 0 {
        // Either Bitangent X or the unknown int or short is with the vertex.
        ofst = ofst.wrapping_add(if full_prec {
            SZ_FLOAT * 3 + SZ_FLOAT
        } else {
            SZ_HALF_FLOAT * 3 + SZ_HALF_FLOAT
        });
    }
    let mut o_tex_coord0: u8 = 0;
    if vf & VF_UV > 0 {
        o_tex_coord0 = ofst / 4;
        ofst = ofst.wrapping_add(SZ_HALF_FLOAT * 2);
    }
    let o_tex_coord1: u8 = 0;
    let mut o_normal: u8 = 0;
    if vf & VF_NORMAL > 0 {
        o_normal = ofst / 4;
        // Bitangent Y is with the normal.
        ofst = ofst.wrapping_add(SZ_BYTE * 3 + SZ_BYTE);
    }
    let mut o_tangent: u8 = 0;
    if vf & VF_NORMAL > 0 && vf & VF_TANGENT > 0 {
        o_tangent = ofst / 4;
        // Bitangent Z is with the tangent.
        ofst = ofst.wrapping_add(SZ_BYTE * 3 + SZ_BYTE);
    }
    let mut o_color: u8 = 0;
    if vf & VF_COLORS > 0 {
        o_color = ofst / 4;
        ofst = ofst.wrapping_add(SZ_BYTE * 4);
    }
    let mut o_skinning_data: u8 = 0;
    if vf & VF_SKINNED > 0 {
        o_skinning_data = ofst / 4;
        // Bone weights and indices.
        ofst = ofst.wrapping_add(SZ_HALF_FLOAT * 4 + SZ_BYTE * 4);
    }
    let o_landscape_data: u8 = 0;
    let mut o_eye_data: u8 = 0;
    if vf & VF_EYEDATA > 0 {
        o_eye_data = ofst / 4;
        ofst = ofst.wrapping_add(SZ_FLOAT);
    }

    t.set_native_values(e, "..\\VF1", Variant::Int(i64::from(ofst / 4)))?;
    // SSE has a separate Vertex Size field with the size in bytes.
    t.set_native_values(e, "..\\..\\Vertex Size", Variant::Int(i64::from(ofst)))?;
    // The cached flags of the Vertex Data array.
    if let Some(el) = t.elements(e, "..\\..\\Vertex Data")? {
        t.set_user_data(el, i32::from(vf));
    }
    t.set_native_values(
        e,
        "..\\VF2",
        Variant::Int(i64::from(o_tex_coord0 | (o_tex_coord1 << 4))),
    )?;
    t.set_native_values(e, "..\\VF3", Variant::Int(i64::from(o_normal | (o_tangent << 4))))?;
    t.set_native_values(e, "..\\VF4", Variant::Int(i64::from(o_color | (o_skinning_data << 4))))?;
    t.set_native_values(
        e,
        "..\\VF5",
        Variant::Int(i64::from(o_landscape_data | (o_eye_data << 4))),
    )?;
    t.set_native_values(e, "..\\VF8", Variant::Int(0))?;
    Ok(())
}

/// Upstream `NiHeader_GetTextBlockType`: the name of the block type index.
pub fn ni_header_get_text_block_type(t: &mut Tree, e: El, a_text: &mut String) -> R<()> {
    let block_types = t.elements(e, "..\\..\\Block Types")?;
    let index = str_to_int(a_text).ok_or_else(|| DfError::new(format!("'{a_text}' is not a valid integer value")))?;
    match block_types {
        Some(block_types) if index >= 0 && index < t.count(block_types) => {
            let item = t.item(block_types, index)?;
            *a_text = t.edit_value(item)?;
            Ok(())
        }
        _ => Err(t.exception(e, &format!("Invalid block type index {a_text}"))),
    }
}

/// Upstream `NiHeader_SetTextBlockType`: the index of the block type name.
pub fn ni_header_set_text_block_type(t: &mut Tree, e: El, a_text: &mut String) -> R<()> {
    if let Some(block_types) = t.elements(e, "..\\..\\Block Types")? {
        for index in 0..t.count(block_types) {
            let item = t.item(block_types, index)?;
            if t.edit_value(item)? == *a_text {
                *a_text = index.to_string();
                return Ok(());
            }
        }
    }
    Err(t.exception(e, &format!("Block type not found in NiHeader: {a_text}")))
}

/// Upstream `wbDefineBSPackedCombinedGeomDataExtra_GetCountTriangles`: the
/// sum of the LOD triangle counts.
pub fn wb_define_bs_packed_combined_geom_data_extra_get_count_triangles(
    t: &mut Tree,
    e: El,
    a_count: &mut i32,
) -> R<()> {
    if let Some(el) = t.elements(e, "..\\LOD")? {
        for index in 0..t.count(el) {
            let lod = t.item(el, index)?;
            let first = t.item(lod, 0)?;
            *a_count = a_count.wrapping_add(t.native_value(first)?.to_i32()?);
        }
    }
    Ok(())
}

/// Upstream `NiSkinPartition_GetCountVertexData`: the data size divided by
/// the vertex size. The variant division is a float division, rounded to
/// the integer count.
pub fn ni_skin_partition_get_count_vertex_data(t: &mut Tree, e: El, a_count: &mut i32) -> R<()> {
    *a_count = t.native_values(e, "..\\Vertex Size")?.to_i32()?;
    if *a_count > 0 {
        let size = t.native_values(e, "..\\Data Size")?.to_f64();
        *a_count = match size {
            Ok(size) => (size / f64::from(*a_count)).round_ties_even() as i32,
            Err(_) => 0,
        };
    }
    Ok(())
}
