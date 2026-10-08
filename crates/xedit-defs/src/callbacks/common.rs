// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsCommon.pas

//! The callbacks and helpers of `wbDefinitionsCommon.pas` that the
//! transpiler does not write. The functions that are not ported yet are
//! stubs in `common_stubs.rs`.
//!
//! The `wbTryGet...` helpers have `out` parameters upstream. Here they
//! return the value as an `Option`.

use std::sync::Arc;

use xedit_core::container_handler;
use xedit_core::delphi::{float_to_str_f_fixed, format_general, round, str_to_float};
use xedit_core::interface::builders::wb_flags_unknown_is_unused;
use xedit_core::interface::constructors::get_container_from_union;
use xedit_core::interface::globals::{
    GameMode, begin_internal_edit, cell_size_factor, cs, end_internal_edit, game_mode, is_fallout_nv, is_fallout3,
    is_fallout4, is_fallout76, is_morrowind, is_oblivion, is_skyrim, is_starfield, remove_offset_data, resolve_alias,
};
use xedit_core::interface::misc::{EditError, int_to_hex64, progress, str_to_int_def};
use xedit_core::interface::string::to_comma_text;
use xedit_core::interface::*;

use crate::common::{wb_idx_addon_node, wb_idx_collision_layer, wb_package_schedule_month_enum};

// The stubs of the callbacks not ported yet; empty once every callback is ported.
#[allow(unused_imports)]
pub use super::common_stubs::*;

/// Upstream `Sig2Int`.
pub fn sig2_int(a_signature: Signature) -> u32 {
    a_signature.to_int()
}

/// Upstream `wbNormalizeToRange`: a normalizer that clamps to `aMin..aMax`.
pub fn wb_normalize_to_range(a_min: f64, a_max: f64) -> Option<FloatNormalizer> {
    Some(Arc::new(move |_element: ElementArg, a_float: f64| {
        if a_float < a_min {
            a_min
        } else if a_float > a_max {
            a_max
        } else {
            a_float
        }
    }))
}

/// Upstream `wbTryGetContainerFromUnion`.
pub fn wb_try_get_container_from_union(a_element: ElementArg) -> Option<ElementRef> {
    get_container_from_union(a_element?)
}

/// Upstream `wbTryGetContainerRefFromUnionOrValue`.
pub fn wb_try_get_container_ref_from_union_or_value(a_element: ElementArg) -> Option<ElementRef> {
    get_container_ref_from_union_or_value(a_element?)
}

/// Upstream `wbTryGetContainerWithValidMainRecord`: the element when it is
/// a main record with elements that is not deleted.
pub fn wb_try_get_container_with_valid_main_record(a_element: ElementArg) -> Option<MainRecordRef> {
    let element = a_element?;
    if element.as_container()?.get_element_count() < 1 {
        return None;
    }
    let main_record = element.as_main_record()?;
    if main_record.get_is_deleted() {
        return None;
    }
    element.clone().into_main_record()
}

/// Upstream `wbTryGetContainingMainRecord`.
pub fn wb_try_get_containing_main_record(a_element: ElementArg) -> Option<MainRecordRef> {
    a_element?.get_containing_main_record()
}

/// Upstream `wbTryGetMainRecord`: the main record the element links to,
/// with the signature `aSignature` unless it is empty.
pub fn wb_try_get_main_record(a_element: ElementArg, a_signature: &str) -> Option<MainRecordRef> {
    let main_record = a_element?.get_links_to()?.into_main_record()?;
    if !a_signature.is_empty() && main_record.get_signature().to_string() != a_signature {
        return None;
    }
    Some(main_record)
}

/// Upstream `wbTrySetContainer`: the element when it is a container and the
/// callback builds a summary.
pub fn wb_try_set_container(a_element: ElementArg, a_type: CallbackType) -> Option<ElementRef> {
    if a_type != CallbackType::ctToSummary {
        return None;
    }
    a_element.filter(|element| element.as_container().is_some()).cloned()
}

/// Upstream `wbFlagDecider`: 1 when the flag `aFlag` of the `Flags` element
/// of the container is set.
pub fn wb_flag_decider(a_flag: u8) -> Option<UnionDecider> {
    Some(Arc::new(move |_base_ptr: DataPtr, a_element: ElementArg| {
        let Some(container) = wb_try_get_container_ref_from_union_or_value(a_element) else {
            return 0;
        };
        let Some(flags) = container
            .as_container()
            .and_then(|container| container.get_element_by_path("Flags"))
        else {
            return 0;
        };
        let flag_bits = match flags.get_native_value() {
            Variant::Int(value) => value,
            Variant::UInt(value) => value as i64,
            _ => 0,
        };
        if flag_bits & (1i64 << a_flag) != 0 { 1 } else { 0 }
    }))
}

fn containing_version(a_element: ElementArg) -> Option<i64> {
    let main_record = a_element?.get_containing_main_record()?;
    Some(i64::from(main_record.get_version()))
}

/// Upstream `wbFormVersionDecider` with one version: 1 when the form
/// version of the containing record is at least `aVersion`.
pub fn wb_form_version_decider_version(a_version: i32) -> Option<UnionDecider> {
    Some(Arc::new(
        move |_base_ptr: DataPtr, a_element: ElementArg| match containing_version(a_element) {
            Some(version) if version >= i64::from(a_version) => 1,
            _ => 0,
        },
    ))
}

/// Upstream `wbFormVersionDecider` with a range of versions.
pub fn wb_form_version_decider_min_version(a_min_version: i32, a_max_version: i32) -> Option<UnionDecider> {
    Some(Arc::new(
        move |_base_ptr: DataPtr, a_element: ElementArg| match containing_version(a_element) {
            Some(version) if version >= i64::from(a_min_version) && version <= i64::from(a_max_version) => 1,
            _ => 0,
        },
    ))
}

/// Upstream `wbFormVersionDecider` with a list of versions: the position
/// of the first version the form version is below, or the count.
pub fn wb_form_version_decider_versions(a_versions: &[i32]) -> Option<UnionDecider> {
    let versions = a_versions.to_vec();
    Some(Arc::new(move |_base_ptr: DataPtr, a_element: ElementArg| {
        let Some(version) = containing_version(a_element) else {
            return 0;
        };
        for (index, &candidate) in versions.iter().enumerate() {
            if version < i64::from(candidate) {
                return index as i32;
            }
        }
        versions.len() as i32
    }))
}

/// The size of the data of a subrecord element: `None` when the element is
/// not a subrecord, `Some(None)` when it has no data.
fn sub_record_data_size(a_element: ElementArg) -> Option<Option<i32>> {
    let element = a_element?;
    if element.get_element_type() != ElementType::etSubRecord {
        return None;
    }
    let has_data = element.as_data_container()?.get_data().is_some();
    Some(has_data.then(|| element.get_data_size()))
}

/// Upstream `wbRecordSizeDecider` with one size: 1 when the subrecord has no
/// data or at least `aSize` bytes.
pub fn wb_record_size_decider_size(a_size: i32) -> Option<UnionDecider> {
    Some(Arc::new(
        move |_base_ptr: DataPtr, a_element: ElementArg| match sub_record_data_size(a_element) {
            None => 0,
            Some(None) => 1,
            Some(Some(size)) if size >= a_size => 1,
            Some(Some(_)) => 0,
        },
    ))
}

/// Upstream `wbRecordSizeDecider` with a range: 1 when the size is outside it.
pub fn wb_record_size_decider_min_size(a_min_size: i32, a_max_size: i32) -> Option<UnionDecider> {
    Some(Arc::new(
        move |_base_ptr: DataPtr, a_element: ElementArg| match sub_record_data_size(a_element) {
            None => 0,
            Some(size) => {
                let size = size.unwrap_or(0);
                if size > a_max_size || size < a_min_size { 1 } else { 0 }
            }
        },
    ))
}

/// Upstream `wbRecordSizeDecider` with a list of sizes: the position of the
/// first size the data is below, or the count.
pub fn wb_record_size_decider_sizes(a_sizes: &[i32]) -> Option<UnionDecider> {
    let sizes = a_sizes.to_vec();
    Some(Arc::new(move |_base_ptr: DataPtr, a_element: ElementArg| {
        let Some(size) = sub_record_data_size(a_element) else {
            return 0;
        };
        let size = size.unwrap_or(0);
        for (index, &candidate) in sizes.iter().enumerate() {
            if size < candidate {
                return index as i32;
            }
        }
        sizes.len() as i32
    }))
}

/// Upstream `wbCombineVarRecs`.
pub fn wb_combine_var_recs(a: &[VarRec], b: &[VarRec]) -> Vec<VarRec> {
    a.iter().chain(b).cloned().collect()
}

/// Upstream `wbMakeVarRecs`.
pub fn wb_make_var_recs(a: &[VarRec]) -> Vec<VarRec> {
    a.to_vec()
}

/// Upstream `wbGMSTUnionDecider`: the type of a game setting from the first
/// letter of its editor ID.
pub fn wb_gmst_union_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(editor_id) = a_element
        .and_then(|element| element.get_container())
        .and_then(|container| {
            container
                .as_container()?
                .get_record_by_signature(Signature::new(b"EDID"))
        })
        .map(|edid| edid.get_value())
    else {
        return 1;
    };
    match editor_id.chars().next() {
        Some('s') => 0,
        Some('i') => 1,
        Some('f') => 2,
        Some('b') if game_mode() >= GameMode::gmTES5 => 3,
        Some('u') if matches!(game_mode(), GameMode::gmFO76 | GameMode::gmSF1) => 4,
        _ => 1,
    }
}

/// Upstream `ElementEditValues[aPath]` on a main record.
fn element_edit_value(main_record: &MainRecordRef, path: &str) -> String {
    main_record
        .get_element_by_path(path)
        .map(|element| element.get_edit_value())
        .unwrap_or_default()
}

/// Upstream `ElementValues[aPath]` on a main record.
fn element_value(main_record: &MainRecordRef, path: &str) -> String {
    main_record
        .get_element_by_path(path)
        .map(|element| element.get_value())
        .unwrap_or_default()
}

fn in_container(value: String) -> String {
    if value.is_empty() {
        value
    } else {
        format!(" in {value}")
    }
}

/// Upstream `wbCellAddInfo`: the worldspace and the grid of a cell.
pub fn wb_cell_add_info(a_main_record: &MainRecordRef) -> String {
    let mut result = in_container(element_edit_value(a_main_record, "Worldspace"));
    if !a_main_record.get_is_persistent()
        && let Some(xclc) = a_main_record.get_record_by_signature(Signature::new(b"XCLC"))
        && let Some(container) = xclc.as_container()
        && let (Some(x), Some(y)) = (container.get_element(0), container.get_element(1))
    {
        result = format!("{result} at {},{}", x.get_value(), y.get_value());
    }
    result
}

/// Upstream `wbDIALAddInfo`: the quest of a topic.
pub fn wb_dial_add_info(a_main_record: &MainRecordRef) -> String {
    let path = if is_skyrim() { "QNAM" } else { "Quest" };
    in_container(element_edit_value(a_main_record, path))
}

/// Upstream `wbDLBRAddInfo`: the quest of a dialog branch.
pub fn wb_dlbr_add_info(a_main_record: &MainRecordRef) -> String {
    let path = if is_skyrim() { "QNAM" } else { "Quest" };
    in_container(element_edit_value(a_main_record, path))
}

/// Upstream `wbINFOAddInfo`: the response text and the topic of a response.
pub fn wb_info_add_info(a_main_record: &MainRecordRef) -> String {
    let mut result = in_container(element_edit_value(a_main_record, "Topic"));
    if is_oblivion() || is_fallout3() {
        result = format!("{result} in {}", element_edit_value(a_main_record, "QSTI"));
    }
    if !result.is_empty() {
        let response = element_value(a_main_record, r"Responses\Response\NAM1");
        let response = response.trim();
        if !response.is_empty() {
            result = format!("''{response}''{result}");
        }
    }
    result
}

/// Upstream `wbLANDAddInfo`: the cell of a landscape.
pub fn wb_land_add_info(a_main_record: &MainRecordRef) -> String {
    in_container(element_edit_value(a_main_record, "Cell"))
}

/// Upstream `wbNAVMAddInfo`: the cell of a navmesh.
pub fn wb_navm_add_info(a_main_record: &MainRecordRef) -> String {
    in_container(element_edit_value(a_main_record, "Cell"))
}

/// Upstream `wbPGRDAddInfo`: the cell of a path grid.
pub fn wb_pgrd_add_info(a_main_record: &MainRecordRef) -> String {
    in_container(element_edit_value(a_main_record, "Cell"))
}

/// Upstream `wbPositionToGridCell`.
fn position_to_grid_cell(x: f64, y: f64) -> (i32, i32) {
    let cell = |value: f64| {
        let scaled = value / cell_size_factor();
        let mut result = scaled.trunc() as i32;
        if value < 0.0 && scaled.fract() != 0.0 {
            result -= 1;
        }
        result
    };
    (cell(x), cell(y))
}

/// Upstream `TwbMainRecord.GetPosition`: the position of a placed record.
fn record_position(main_record: &MainRecordRef) -> Option<(f64, f64, f64)> {
    let data = main_record.get_record_by_signature(Signature::new(b"DATA"))?;
    let data = data.as_container()?;
    if data.get_element_count() != 2 {
        return None;
    }
    let position = data.get_element(0)?;
    let position = position.as_container()?;
    if position.get_element_count() != 3 {
        return None;
    }
    // `TwbVector` holds singles, so the rounded native values round again
    // to single precision: 4095.9999997 is 4096 and lies in the next cell.
    let coordinate = |index| match position.get_element(index)?.get_native_value() {
        Variant::Float(value) => Some(f64::from(value as f32)),
        Variant::Int(value) => Some(f64::from(value as f32)),
        _ => None,
    };
    Some((coordinate(0)?, coordinate(1)?, coordinate(2)?))
}

/// Upstream `wbPlacedAddInfo`: what a placed record places and where. The
/// precombined mesh of Fallout 4 is not ported yet.
pub fn wb_placed_add_info(a_main_record: &MainRecordRef) -> String {
    let mut result = in_container(element_edit_value(a_main_record, "Cell"));
    if !a_main_record.get_is_deleted() {
        let name = a_main_record
            .get_record_by_signature(Signature::new(b"NAME"))
            .map(|name| name.get_value())
            .unwrap_or_default();
        result = format!("Places {}{result}", name.trim());
        let cell = a_main_record
            .get_container()
            .and_then(|group| group.as_element_impl()?.group_record_impl())
            .and_then(|group| group.children_of());
        if let Some(cell) = cell
            && cell.get_is_persistent()
            && let Some((x, y, _)) = record_position(a_main_record)
        {
            let (grid_x, grid_y) = position_to_grid_cell(x, y);
            result = format!("{result} at {grid_x},{grid_y}");
        }
        if a_main_record.get_has_precombined_mesh() {
            result = format!("{result} in {}", a_main_record.get_precombined_mesh());
        }
    }
    result
}

/// Upstream `wbROADAddInfo`: the worldspace of a road.
pub fn wb_road_add_info(a_main_record: &MainRecordRef) -> String {
    in_container(element_edit_value(a_main_record, "Worldspace"))
}

/// Upstream `wbSCENAddInfo`: the quest of a scene.
pub fn wb_scen_add_info(a_main_record: &MainRecordRef) -> String {
    let path = if is_skyrim() { "PNAM" } else { "Quest" };
    in_container(element_edit_value(a_main_record, path))
}

/// Upstream `VarIsOrdinal` for the value of an element: the integer, or `None`.
fn ordinal(value: Variant) -> Option<i64> {
    match value {
        Variant::Int(value) => Some(value),
        Variant::UInt(value) => Some(value as i64),
        Variant::Bool(value) => Some(i64::from(value)),
        _ => None,
    }
}

/// Upstream `wbSceneActionTypeDecider`: the `ANAM` type of a scene action.
pub fn wb_scene_action_type_decider(a_container: ElementArg) -> i32 {
    let Some(container) = a_container.and_then(|container| container.as_container()) else {
        return -1;
    };
    ordinal(container.get_element_native_value("ANAM")).map_or(-1, |value| value as i32)
}

/// Upstream `wbSceneTimelineTypeDecider`: the kind of a scene timeline entry.
pub fn wb_scene_timeline_type_decider(a_container: ElementArg) -> i32 {
    let Some(container) = a_container.and_then(|container| container.as_container()) else {
        return -1;
    };
    match ordinal(container.get_element_native_value("TNAM")) {
        None => -1,
        Some(2) => 1,
        Some(4 | 5) => 2,
        Some(0 | 7) => 3,
        Some(_) => 0,
    }
}

/// The main record an element links to.
fn linked_record(element: &ElementRef) -> Option<MainRecordRef> {
    element.get_links_to()?.into_main_record()
}

/// Upstream `wbDIALQuestToStr`: warns when the quest of a topic differs from
/// the quest of its branch.
pub fn wb_dial_quest_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    if a_type != CallbackType::ctCheck && a_type != CallbackType::ctToStr {
        return;
    }
    let Some(element) = a_element else { return };
    let Some(branch_quest) = element
        .get_containing_main_record()
        .and_then(|record| record.get_record_by_signature(Signature::new(b"BNAM")))
        .and_then(|bnam| linked_record(&bnam))
        .and_then(|branch| branch.get_record_by_signature(Signature::new(b"QNAM")))
        .and_then(|qnam| linked_record(&qnam))
        .map(|quest| quest.get_master_or_self())
    else {
        return;
    };
    let Some(element_quest) = linked_record(element).map(|quest| quest.get_master_or_self()) else {
        return;
    };
    if element_quest.get_element_id() == branch_quest.get_element_id() {
        return;
    }
    match a_type {
        CallbackType::ctCheck => *a_value = "<Warning: does not match Quest assigned on Branch>".to_owned(),
        _ => a_value.push_str(" <Warning: does not match Quest assigned on Branch>"),
    }
}

/// Upstream `wbRGBAToStr`: the summary of a color.
pub fn wb_rgba_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    let Some(container) = wb_try_set_container(a_element, a_type) else {
        return;
    };
    let Some(container) = container.as_container() else {
        return;
    };
    if container.get_element_count() < 3 {
        return;
    }
    let summary = |index| {
        container
            .get_element(index)
            .map(|element| element.get_summary())
            .unwrap_or_default()
    };
    let (r, g, b) = (summary(0), summary(1), summary(2));
    let alpha = container.get_element(3).filter(|alpha| {
        alpha.get_conflict_priority() > ConflictPriority::cpIgnore
            && alpha
                .get_def()
                .is_none_or(|def| def.get_def_type() != DefType::dtByteArray)
    });
    *a_value = match alpha {
        Some(alpha) => format!("RGBA({r}, {g}, {b}, {})", alpha.get_summary()),
        None => format!("RGB({r}, {g}, {b})"),
    };
}

/// Upstream `wbVec3ToStr`: the summary of a vector.
pub fn wb_vec3_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    let Some(container) = wb_try_set_container(a_element, a_type) else {
        return;
    };
    let Some(container) = container.as_container() else {
        return;
    };
    let summary = |index| {
        container
            .get_element(index)
            .map(|element| element.get_summary())
            .unwrap_or_default()
    };
    *a_value = format!("({}, {}, {})", summary(0), summary(1), summary(2));
}

/// Upstream `wbToStringFromLinksToSummary`: the summary of the element the
/// value links to, with the record it is on.
pub fn wb_to_string_from_links_to_summary(
    a_value: &mut String,
    _a_base_ptr: DataPtr,
    a_element: ElementArg,
    a_type: CallbackType,
) {
    if a_type != CallbackType::ctToStr {
        return;
    }
    let Some(links_to) = a_element.and_then(|element| element.get_links_to()) else {
        return;
    };
    let summary = links_to.get_summary();
    if summary.is_empty() {
        return;
    }
    *a_value = summary;
    if links_to.as_main_record().is_none()
        && let Some(main_record) = links_to.get_containing_main_record()
    {
        let record_name = main_record.get_name();
        if !record_name.is_empty() {
            a_value.push_str(" on ");
            a_value.push_str(&record_name);
        }
    }
}

/// Upstream `wbToStringFromLinksToMainRecordName`: the value in brackets
/// with the name of the record it links to.
pub fn wb_to_string_from_links_to_main_record_name(
    a_value: &mut String,
    _a_base_ptr: DataPtr,
    a_element: ElementArg,
    a_type: CallbackType,
) {
    if a_type != CallbackType::ctToStr || a_value.is_empty() {
        return;
    }
    *a_value = format!("[{a_value}]");
    let Some(main_record) = a_element
        .and_then(|element| element.get_links_to())
        .and_then(|e| e.into_main_record())
    else {
        return;
    };
    let record_name = main_record.get_name();
    if !record_name.is_empty() {
        a_value.push(' ');
        a_value.push_str(&record_name);
    }
}

/// Delphi `Integer(aVariant)` for the deciders: the integer of a number
/// or flag value, 0 for anything else.
pub fn variant_int(value: &Variant) -> i64 {
    match value {
        Variant::Float(float) => float.round() as i64,
        Variant::Str(text) => text.trim().parse().unwrap_or(0),
        other => other.as_ordinal().unwrap_or(0),
    }
}

/// `Container.ElementNativeValues[aPath]` as an integer, 0 when missing.
fn container_int(container: &ElementRef, path: &str) -> i64 {
    container
        .as_container()
        .map_or(0, |container| variant_int(&container.get_element_native_value(path)))
}

/// Upstream `wbGetScriptObjFormat`: 1 when the `Object Format` of the
/// enclosing subrecord is 1.
pub fn wb_get_script_obj_format(a_element: ElementArg) -> i32 {
    let mut container = a_element.and_then(|element| element.get_container());
    while let Some(current) = &container
        && current.get_element_type() != ElementType::etSubRecord
    {
        container = current.get_container();
    }
    let Some(container) = container else { return 0 };
    if container_int(&container, "Object Format") == 1 {
        1
    } else {
        0
    }
}

/// Upstream `wbScriptObjFormatDecider`.
pub fn wb_script_obj_format_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    wb_get_script_obj_format(a_element)
}

/// The number of the low `count` bits set in the `Flags` of the container,
/// shared by the script fragment counters.
fn script_fragments_counter(a_element: ElementArg, count: u32) -> u32 {
    let Some(container) = a_element.and_then(|element| element.get_container()) else {
        return 0;
    };
    let flags = container_int(&container, "Flags");
    (0..count).filter(|bit| flags >> bit & 1 == 1).count() as u32
}

/// Upstream `wbScriptFragmentsInfoCounter`.
pub fn wb_script_fragments_info_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    script_fragments_counter(a_element, 2)
}

/// Upstream `wbScriptFragmentsPackCounter`.
pub fn wb_script_fragments_pack_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    script_fragments_counter(a_element, 3)
}

/// Upstream `wbScriptFragmentsSceneCounter`.
pub fn wb_script_fragments_scene_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    script_fragments_counter(a_element, 2)
}

/// Upstream `wbNavmeshGridCounter`: the square of the `Divisor`.
pub fn wb_navmesh_grid_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(container) = a_element.and_then(|element| element.get_container()) else {
        return 0;
    };
    let Some(grid_size) = container
        .as_container()
        .and_then(|container| container.get_element_by_name("Divisor"))
    else {
        return 0;
    };
    let grid_size = variant_int(&grid_size.get_native_value());
    if !(0..=12).contains(&grid_size) {
        return 0;
    }
    (grid_size * grid_size) as u32
}

/// Upstream `wbFlagREFRInteriorDontShow`: hidden unless the cell of the
/// reference is an interior.
pub fn wb_flag_refr_interior_dont_show(a_element: ElementArg) -> bool {
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return false;
    };
    let Some(cell) = main_record
        .get_element_links_to("Cell")
        .and_then(|cell| cell.into_main_record())
    else {
        return false;
    };
    variant_int(&cell.get_element_native_value("DATA")) & 0x1 != 0
}

/// Upstream `wbFlagPartialFormDontShow`.
pub fn wb_flag_partial_form_dont_show(a_element: ElementArg) -> bool {
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return false;
    };
    if main_record.get_is_partial_form() {
        return false;
    }
    !main_record.get_can_be_partial()
}

/// Upstream `wbFlagREFRSkyMarkerDontShow`: hidden unless the base record
/// has the flag `$1000000`.
pub fn wb_flag_refr_sky_marker_dont_show(a_element: ElementArg) -> bool {
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return false;
    };
    let Some(name) = main_record
        .get_element_links_to("NAME")
        .and_then(|name| name.into_main_record())
    else {
        return false;
    };
    name.get_flags().0 & 0x100_0000 == 0
}

/// Upstream `wbCellExteriorDontShow`: hidden for an exterior cell.
pub fn wb_cell_exterior_dont_show(a_element: ElementArg) -> bool {
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return false;
    };
    let path = if is_morrowind() { "DATA\\Flags" } else { "DATA" };
    variant_int(&main_record.get_element_native_value(path)) & 1 == 0
}

/// Upstream `wbHideFFFF`: `None` for `$FFFF`.
pub fn wb_hide_ffff(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    match a_type {
        CallbackType::ctToSortKey => int_to_hex64(a_int, 4),
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            if a_int == 0xFFFF {
                "None".to_owned()
            } else {
                a_int.to_string()
            }
        }
        _ => String::new(),
    }
}

/// Upstream `wbACBSLevelDecider`: 1 for the PC level mult flag.
pub fn wb_acbs_level_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    if container_int(&container, "Flags") & 0x0000_0080 != 0 {
        1
    } else {
        0
    }
}

/// Upstream `wbCOEDOwnerDecider`: 1 for an NPC owner, 2 for a faction.
pub fn wb_coed_owner_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let Some(main_record) = container
        .as_container()
        .and_then(|container| container.get_element_links_to("Owner"))
        .and_then(|links_to| links_to.into_main_record())
    else {
        return 0;
    };
    match main_record.get_signature().0.as_slice() {
        b"NPC_" => 1,
        b"FACT" => 2,
        _ => 0,
    }
}

/// Upstream `wbConditionCompValueDecider`: 1 for the "use global" flag.
pub fn wb_condition_comp_value_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    if container_int(&container, "Type") & 4 != 0 {
        1
    } else {
        0
    }
}

/// Upstream `wbConditionParam3Decider`: the `Run On` value.
pub fn wb_condition_param3_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    container_int(&container, "Run On") as i32
}

/// Upstream `wbConditionReferenceDecider`: 1 when the condition runs on a reference.
pub fn wb_condition_reference_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    if is_fallout_nv() {
        // IsFacingUp, IsLeftUp
        let function = container_int(&container, "Function");
        if function == 106 || function == 285 {
            return 0;
        }
    }
    if container_int(&container, "Run On") == 2 { 1 } else { 0 }
}

/// The container of the element, or the element itself when it is one.
fn self_or_container(a_element: ElementArg) -> Option<ElementRef> {
    let element = a_element?;
    if element.as_container().is_some() {
        Some(element.clone())
    } else {
        element.get_container()
    }
}

/// Upstream `wbNAVIIslandDataDecider`: the `Has Island Data` value.
pub fn wb_navi_island_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = self_or_container(a_element) else {
        return 0;
    };
    let Some(element) = container
        .as_container()
        .and_then(|container| container.get_element_by_path("...\\Has Island Data"))
    else {
        return 0;
    };
    variant_int(&element.get_native_value()) as i32
}

/// Upstream `wbNAVIParentDecider`: 1 for an interior (no parent world).
pub fn wb_navi_parent_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = self_or_container(a_element) else {
        return 0;
    };
    let Some(element) = container
        .as_container()
        .and_then(|container| container.get_element_by_path("...\\Parent World"))
    else {
        return 0;
    };
    if variant_int(&element.get_native_value()) == 0 {
        1
    } else {
        0
    }
}

/// Upstream `wbNVNMParentDecider`: 1 for an interior cell.
pub fn wb_nvnm_parent_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = a_element.and_then(|element| element.get_container()) else {
        return 0;
    };
    let Some(parent) = container
        .as_container()
        .and_then(|container| container.get_element_by_name("Parent World"))
    else {
        return 0;
    };
    if variant_int(&parent.get_native_value()) == 0 {
        1
    } else {
        0
    }
}

/// Upstream `wbModelInfoDecider`: the model header format by the form
/// version, checked against the data where a record of the other version
/// range looks like the newer or the older format.
pub fn wb_model_info_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return 0;
    };
    let version = main_record.get_version();
    // Arbitary limit of 8 supported headers for now.
    let first = a_base_ptr
        .filter(|data| data.len() >= 4)
        .map(|data| u32::from_le_bytes(data[..4].try_into().unwrap()));
    if version >= 40 {
        // Most likely older version format in FormVersion 40+ record.
        if first.is_some_and(|first| first > 8) {
            return 1;
        }
        3
    } else if version >= 38 {
        // Most likely newer version format in FormVersion 38-39 record.
        if first.is_some_and(|first| first <= 8) {
            return 1;
        }
        2
    } else {
        0
    }
}

/// Upstream `wbModelInfoDontShow`: hidden before form version 38.
pub fn wb_model_info_dont_show(a_element: ElementArg) -> bool {
    if game_mode() < GameMode::gmTES5 {
        return false;
    }
    match a_element.and_then(|element| element.get_containing_main_record()) {
        Some(main_record) => main_record.get_version() < 38,
        None => true,
    }
}

/// Upstream `wbModelInfoGetCP`: ignored before form version 38.
pub fn wb_model_info_get_cp(a_element: ElementArg, a_conflict_priority: &mut ConflictPriority) {
    *a_conflict_priority = ConflictPriority::cpNormal;
    if game_mode() < GameMode::gmTES5 {
        return;
    }
    if let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record())
        && main_record.get_version() < 38
    {
        *a_conflict_priority = ConflictPriority::cpIgnore;
    }
}

/// Upstream `wbLandNormalsGetCP`.
///
/// UPSTREAM-QUIRK: upstream raises the priority to normal when the record
/// has conflicts; the conflict state is not ported, so it stays benign.
pub fn wb_land_normals_get_cp(_a_element: ElementArg, a_conflict_priority: &mut ConflictPriority) {
    *a_conflict_priority = ConflictPriority::cpBenign;
}

/// The data container two levels up (or three when the path is not found
/// two levels up), as the row and column counters look for their bounds.
fn counter_container(a_element: ElementArg, path: &str) -> Option<ElementRef> {
    let container = a_element?.get_container()?;
    container.as_data_container()?;
    let mut container = container.get_container()?;
    container.as_data_container()?;
    if container.as_container()?.get_element_by_path(path).is_none() {
        container = container.get_container()?;
        container.as_data_container()?;
    }
    Some(container)
}

/// Upstream `wbMHDTColumnsCounter`: the columns of the max height data.
pub fn wb_mhdt_columns_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(container) = counter_container(a_element, "Dimensions\\Min\\X") else {
        return 0;
    };
    let Some(container) = container.as_container() else {
        return 0;
    };
    let Some(min_x) = container.get_element_by_path("Dimensions\\Min\\X") else {
        return 0;
    };
    let min_x = min_x.get_native_value().as_ordinal().unwrap_or(0);
    let Some(max_x) = container.get_element_by_path("Dimensions\\Max\\X") else {
        return 0;
    };
    let max_x = max_x.get_native_value().as_ordinal().unwrap_or(0);
    (max_x - min_x + 1) as u32
}

/// A worldspace bound as the counters read it: 0 for the extreme values.
fn world_bound(container: &dyn Container, path: &str) -> Option<f64> {
    let value = match container.get_element_by_path(path)?.get_native_value() {
        Variant::Float(value) => value,
        other => other.as_ordinal().unwrap_or(0) as f64,
    };
    if value == f64::from(f32::MAX) || value == f64::from(f32::MIN) {
        Some(0.0)
    } else {
        Some(value)
    }
}

/// Upstream `wbWorldColumnsCounter`.
pub fn wb_world_columns_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(container) = counter_container(a_element, "Worldspace Bounds\\NAM0\\X") else {
        return 0;
    };
    let Some(container) = container.as_container() else {
        return 0;
    };
    let Some(min_x) = world_bound(container, "Worldspace Bounds\\NAM0\\X") else {
        return 0;
    };
    let Some(max_x) = world_bound(container, "Worldspace Bounds\\NAM9\\X") else {
        return 0;
    };
    (round(max_x) - round(min_x) + 1) as u32
}

/// Upstream `wbWorldRowsCounter`.
pub fn wb_world_rows_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(container) = a_element
        .and_then(|element| element.get_container())
        .filter(|container| container.as_data_container().is_some())
        .and_then(|container| container.get_container())
        .filter(|container| container.as_data_container().is_some())
    else {
        return 0;
    };
    let Some(container) = container.as_container() else {
        return 0;
    };
    let Some(min_y) = world_bound(container, "Worldspace Bounds\\NAM0\\Y") else {
        return 0;
    };
    let Some(max_y) = world_bound(container, "Worldspace Bounds\\NAM9\\Y") else {
        return 0;
    };
    (round(max_y) - round(min_y) + 1) as u32
}

/// Upstream `wbWeatherCloudColorsCounter`: 32 layers from form version 35.
pub fn wb_weather_cloud_colors_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    match a_element.and_then(|element| element.get_containing_main_record()) {
        Some(main_record) if main_record.get_version() >= 35 => 32,
        Some(_) => 4,
        None => 0,
    }
}

/// Upstream `wbNoFlagsDecider`: 1 when the `Flags` of the container are 0.
pub fn wb_no_flags_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_ref_from_union_or_value(a_element) else {
        return 0;
    };
    let Some(flags) = container
        .as_container()
        .and_then(|container| container.get_element_by_path("Flags"))
    else {
        return 0;
    };
    if variant_int(&flags.get_native_value()) == 0 {
        1
    } else {
        0
    }
}

/// Upstream `wbWeatherTimeOfDayDecider`: 1 for the subrecord sizes with
/// four values.
pub fn wb_weather_time_of_day_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match a_element.and_then(|element| element.get_sub_record_header_size()) {
        Some(64 | 160) => 1,
        _ => 0,
    }
}

/// Upstream `wbCellInteriorDontShow`: hidden for an interior cell.
pub fn wb_cell_interior_dont_show(a_element: ElementArg) -> bool {
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return false;
    };
    let path = if is_morrowind() { "DATA\\Flags" } else { "DATA" };
    variant_int(&main_record.get_element_native_value(path)) & 1 == 1
}

/// Upstream `wbGetREGNType`: the `RDAT\Type` of the region data entry
/// that contains the element, -1 when there is none.
pub fn wb_get_regn_type(a_element: ElementArg) -> i64 {
    let mut container = a_element.cloned();
    while let Some(current) = &container
        && current.get_name() != "Region Data Entry"
    {
        container = current.get_container();
    }
    match container {
        Some(container) => container_int(&container, "RDAT\\Type"),
        None => -1,
    }
}

/// Upstream `wbREGNSoundDontShow`.
pub fn wb_regn_sound_dont_show(a_element: ElementArg) -> bool {
    wb_get_regn_type(a_element) != 7
}

/// Upstream `wbTemplateActorDontShow`: hidden unless the template flag of
/// the position of the element in its subrecord is set.
pub fn wb_template_actor_dont_show(a_element: ElementArg) -> bool {
    let Some(element) = a_element else { return false };
    let Some(sub_record) = element.get_containing_sub_record() else {
        return false;
    };
    let Some(sub_record) = sub_record.as_container() else {
        return false;
    };
    let Some(main_record) = element.get_containing_main_record() else {
        return false;
    };
    let template_flags = variant_int(&main_record.get_element_native_value("ACBS\\Template Flags")) as u32;
    if template_flags == 0 {
        return true;
    }
    let index = (0..sub_record.get_element_count()).find(|&index| {
        sub_record
            .get_element(index)
            .is_some_and(|other| other.get_element_id() == element.get_element_id())
    });
    match index {
        Some(index) => (template_flags >> index) & 1 == 0,
        None => false,
    }
}

/// Upstream `wbScaledInt4ToInt`.
pub fn wb_scaled_int4_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    round(str_to_float(a_string).unwrap_or(0.0) * 10000.0)
}

/// Upstream `wbScaledInt4ToStr`: the value divided by 10000.
pub fn wb_scaled_int4_to_str(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => {
            float_to_str_f_fixed(a_int as f64 / 10000.0, 4)
        }
        CallbackType::ctToSortKey => {
            let mut result = float_to_str_f_fixed(a_int as f64 / 10000.0, 4);
            if result.len() < 22 {
                result = format!("{}{result}", "0".repeat(22 - result.len()));
            }
            format!("{}{result}", if a_int < 0 { '-' } else { '+' })
        }
        _ => String::new(),
    }
}

/// Upstream `wbWeatherCloudSpeedToInt`.
pub fn wb_weather_cloud_speed_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    let value = str_to_float(a_string).unwrap_or(0.0) * 10.0 * 127.0 + 127.0;
    round(value).min(254)
}

/// Upstream `wbWeatherCloudSpeedToStr`.
pub fn wb_weather_cloud_speed_to_str(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => {
            float_to_str_f_fixed((a_int - 127) as f64 / 127.0 / 10.0, 4)
        }
        _ => String::new(),
    }
}

/// Upstream `wbVTXTPosition`: the row and column of a vertex texture position.
pub fn wb_vtxt_position(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    match a_type {
        CallbackType::ctToSortKey => format!("{}{}", int_to_hex64(a_int / 17, 2), int_to_hex64(a_int % 17, 2)),
        CallbackType::ctCheck => {
            if !(0..=288).contains(&a_int) {
                format!("<Out of range: {a_int}>")
            } else {
                String::new()
            }
        }
        CallbackType::ctToStr | CallbackType::ctToSummary => format!("{a_int} -> {}:{}", a_int / 17, a_int % 17),
        _ => String::new(),
    }
}

/// The text of a file or folder hash: the name the containers give it once
/// the load order is loaded (`wbLoaderDone`), else the hash.
fn hash_callback(a_int: i64, a_type: CallbackType, resolve: fn(i64) -> String) -> String {
    if container_handler::loader_done()
        && matches!(
            a_type,
            CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToSortKey
        )
    {
        let name = resolve(a_int);
        if !name.is_empty() {
            return name;
        }
    }
    match a_type {
        CallbackType::ctToSortKey => int_to_hex64(a_int, 16),
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            if a_int > i64::from(u32::MAX) || a_type == CallbackType::ctToStr {
                format!("{{{}}}", int_to_hex64(a_int, 16))
            } else {
                format!("{{{}}}", int_to_hex64(a_int, 8))
            }
        }
        CallbackType::ctToEditValue => a_int.to_string(),
        _ => String::new(),
    }
}

/// Upstream `wbFileHashCallback`.
pub fn wb_file_hash_callback(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    hash_callback(a_int, a_type, container_handler::resolve_file_hash)
}

/// Upstream `wbFolderHashCallback`.
pub fn wb_folder_hash_callback(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    hash_callback(a_int, a_type, container_handler::resolve_folder_hash)
}

/// The alpha element of a color, or `None` when it is ignored or unused.
fn color_alpha(container: &dyn Container, index: i32) -> Option<ElementRef> {
    let alpha = container.get_element(index)?;
    if alpha.get_conflict_priority() <= ConflictPriority::cpIgnore
        || alpha
            .get_def()
            .is_some_and(|def| def.get_def_type() == DefType::dtByteArray)
    {
        return None;
    }
    Some(alpha)
}

fn color_text(r: &str, g: &str, b: &str, alpha: Option<ElementRef>) -> String {
    match alpha {
        Some(alpha) => format!("RGBA({r}, {g}, {b}, {})", alpha.get_summary()),
        None => format!("RGB({r}, {g}, {b})"),
    }
}

/// Upstream `wbABGRToStr`.
pub fn wb_abgr_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    let Some(container) = wb_try_set_container(a_element, a_type) else {
        return;
    };
    let Some(container) = container.as_container() else {
        return;
    };
    let summary = |index: i32| {
        container
            .get_element(index)
            .map(|element| element.get_summary())
            .unwrap_or_default()
    };
    let (b, g, r) = (summary(1), summary(2), summary(3));
    *a_value = color_text(&r, &g, &b, color_alpha(container, 0));
}

/// Upstream `wbBGRAToStr`.
pub fn wb_bgra_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    let Some(container) = wb_try_set_container(a_element, a_type) else {
        return;
    };
    let Some(container) = container.as_container() else {
        return;
    };
    if container.get_element_count() < 3 {
        return;
    }
    let summary = |index: i32| {
        container
            .get_element(index)
            .map(|element| element.get_summary())
            .unwrap_or_default()
    };
    let (b, g, r) = (summary(0), summary(1), summary(2));
    let alpha = if container.get_element_count() >= 4 {
        color_alpha(container, 3)
    } else {
        None
    };
    *a_value = color_text(&r, &g, &b, alpha);
}

/// Upstream `wbFactionRelationToStr`: the reaction and the faction.
pub fn wb_faction_relation_to_str(
    a_value: &mut String,
    _a_base_ptr: DataPtr,
    a_element: ElementArg,
    a_type: CallbackType,
) {
    let Some(container) = wb_try_set_container(a_element, a_type) else {
        return;
    };
    let Some(container) = container.as_container() else {
        return;
    };
    let Some(faction) = container.get_element(0) else {
        return;
    };
    if faction.get_links_to().is_none() {
        return;
    }
    let Some(reaction) = container.get_element(1) else {
        return;
    };
    *a_value = faction.get_value();
    if is_oblivion() {
        let native_reaction = reaction.get_native_value().as_ordinal().unwrap_or(0);
        *a_value = format!("{native_reaction} {a_value}");
        if native_reaction >= 0 {
            *a_value = format!("+{a_value}");
        }
        return;
    }
    let modifier = container
        .get_element(2)
        .map(|element| element.get_value())
        .unwrap_or_default();
    *a_value = format!("{modifier} {a_value}");
}

/// Upstream `wbLANDTextureToStr`: the default texture for a null texture.
pub fn wb_land_texture_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    let Some(element) = a_element else { return };
    if element.get_native_value().as_ordinal().unwrap_or(0) != 0 {
        return;
    }
    let default_texture = match game_mode() {
        GameMode::gmTES4 | GameMode::gmTES4R => "TerrainHDDirt01dds",
        GameMode::gmFO3 | GameMode::gmFNV => "LDirtWasteland01",
        GameMode::gmTES5 | GameMode::gmTES5VR | GameMode::gmSSE => "LDirt02",
        GameMode::gmFO4 | GameMode::gmFO4VR => "LCWDefault01Grass01",
        _ => "",
    };
    if matches!(a_type, CallbackType::ctToStr | CallbackType::ctToSummary) {
        *a_value = format!("{default_texture} [LTEX:00000000]");
    }
}

/// Upstream `wbCellGridIsRemovable`.
pub fn wb_cell_grid_is_removable(a_element: ElementArg) -> bool {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .is_some_and(|main_record| variant_int(&main_record.get_element_native_value("DATA")) & 1 == 1)
}

/// The `Parent Worldspace\PNAM` flags of the containing worldspace.
fn parent_worldspace_flags(a_element: ElementArg) -> i64 {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .map_or(0, |main_record| {
            variant_int(&main_record.get_element_native_value("Parent Worldspace\\PNAM"))
        })
}

/// Upstream `wbWorldLandDataIsRemovable`.
pub fn wb_world_land_data_is_removable(a_element: ElementArg) -> bool {
    parent_worldspace_flags(a_element) & 0x01 == 1
}

/// Upstream `wbWorldLODDataIsRemovable`.
pub fn wb_world_lod_data_is_removable(a_element: ElementArg) -> bool {
    parent_worldspace_flags(a_element) & 0x02 == 2
}

/// Upstream `wbWorldMapDataIsRemovable`.
pub fn wb_world_map_data_is_removable(a_element: ElementArg) -> bool {
    if is_oblivion() {
        a_element
            .and_then(|element| element.get_containing_main_record())
            .is_some_and(|main_record| main_record.get_record_by_signature(Signature::new(b"WNAM")).is_some())
    } else {
        parent_worldspace_flags(a_element) & 0x04 == 4
    }
}

/// Delphi `StrToInt64`: decimal, or hexadecimal after `$`.
fn str_to_int64(text: &str) -> Option<i64> {
    let text = text.trim();
    match text.strip_prefix('$') {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// The quest record that `aQuestRef` is or links to, with the override the
/// alias lookups use.
fn alias_quest(a_quest_ref: &ElementRef) -> Option<MainRecordRef> {
    let main_record = match a_quest_ref.clone().into_main_record() {
        Some(main_record) => main_record,
        None => a_quest_ref.get_links_to()?.into_main_record()?,
    };
    let main_record = if is_skyrim() {
        main_record.get_winning_override()
    } else {
        // The winning quest override except for partial forms.
        let winning = main_record.get_winning_override();
        if winning.get_flags().0 & 0x0000_4000 == 0 {
            winning
        } else if main_record.get_flags().0 & 0x0000_4000 != 0 {
            main_record.get_master_or_self()
        } else {
            main_record
        }
    };
    Some(main_record)
}

/// The alias containers of a quest: the `ALST` struct of a collection
/// alias, the alias itself otherwise.
fn quest_aliases(main_record: &MainRecordRef) -> Vec<ElementRef> {
    let Some(aliases) = main_record.get_element_by_name("Aliases") else {
        return Vec::new();
    };
    let Some(aliases) = aliases.as_container() else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for index in 0..aliases.get_element_count() {
        let Some(mut alias) = aliases.get_element(index) else {
            continue;
        };
        if alias.as_container().is_none() {
            continue;
        }
        if alias.get_has_signature() == Some(Signature::new(b"ALCS"))
            && let Some(alst) = alias
                .as_container()
                .and_then(|alias| alias.get_element_by_signature(Signature::new(b"ALST")))
        {
            if alst.as_container().is_none() {
                continue;
            }
            alias = alst;
        }
        result.push(alias);
    }
    result
}

/// The alias ID: the first element of the alias.
fn alias_id(alias: &ElementRef) -> i64 {
    alias
        .as_container()
        .and_then(|alias| alias.get_element(0))
        .map_or(0, |id| variant_int(&id.get_native_value()))
}

/// Upstream `wbAliasLinksTo`: the alias `aInt` of the quest.
pub fn wb_alias_links_to(a_int: i64, a_quest_ref: ElementArg) -> Option<ElementRef> {
    if a_int < 0 {
        return None;
    }
    let main_record = alias_quest(a_quest_ref?)?;
    if main_record.get_signature() != Signature::new(b"QUST") {
        return None;
    }
    quest_aliases(&main_record)
        .into_iter()
        .find(|alias| alias_id(alias) == a_int)
}

/// Whether `aInt` is one of the fixed alias values of the game.
fn is_fixed_alias(a_int: i64) -> bool {
    a_int == -1 || (a_int == -2 && !is_skyrim()) || ((-5..=-3).contains(&a_int) && is_starfield())
}

/// Upstream `wbAliasToStr`: the text of the alias `aInt` of the quest
/// `aQuestRef` (a quest record or a reference to one).
pub fn wb_alias_to_str(a_int: i64, a_quest_ref: ElementArg, a_type: CallbackType) -> String {
    let mut result = String::new();
    match a_type {
        CallbackType::ctToEditValue | CallbackType::ctToStr | CallbackType::ctToSummary => {
            result = if a_int == -1 {
                "None".to_owned()
            } else if a_int == -2 && !is_skyrim() {
                "Player".to_owned()
            } else if a_int == -3 && is_starfield() {
                "Non-Actor Track".to_owned()
            } else if a_int == -4 && is_starfield() {
                "Play Audio At Player(Voice Note)".to_owned()
            } else if a_int == -5 && is_starfield() {
                "Dialogue For Scene".to_owned()
            } else {
                let mut text = a_int.to_string();
                if a_type == CallbackType::ctToStr {
                    text.push_str(" <Warning: Could not resolve alias>");
                }
                text
            };
        }
        CallbackType::ctToSortKey => return int_to_hex64(a_int, 8),
        CallbackType::ctCheck if !is_fixed_alias(a_int) => {
            result = format!("<Warning: Could not resolve alias [{a_int}]>");
        }
        _ => {}
    }
    // UPSTREAM-QUIRK: the operator precedence makes the edit type and edit
    // info exits apply to the last comparison only.
    if a_int == -1
        || (a_int == -2 && !is_skyrim())
        || (a_int == -3 && is_starfield())
        || (a_int == -4 && is_starfield())
        || (a_int == -5 && is_starfield() && a_type != CallbackType::ctEditType && a_type != CallbackType::ctEditInfo)
    {
        return result;
    }
    let Some(a_quest_ref) = a_quest_ref else { return result };
    let Some(main_record) = alias_quest(a_quest_ref) else {
        return result;
    };
    if main_record.get_signature() != Signature::new(b"QUST") {
        match a_type {
            CallbackType::ctToStr | CallbackType::ctToSummary => {
                result = a_int.to_string();
                if a_type == CallbackType::ctToStr {
                    result.push_str(&format!(
                        " <Warning: \"{}\" is not a Quest record>",
                        main_record.get_short_name()
                    ));
                }
            }
            CallbackType::ctCheck => {
                result = format!("<Warning: \"{}\" is not a Quest record>", main_record.get_short_name());
            }
            _ => {}
        }
        return result;
    }
    let mut edit_infos: Option<Vec<String>> = match a_type {
        CallbackType::ctEditType => return "ComboBox".to_owned(),
        CallbackType::ctEditInfo => Some(Vec::new()),
        _ => None,
    };
    for alias in quest_aliases(&main_record) {
        let id = alias_id(&alias);
        if edit_infos.is_some() || id == a_int {
            let name = alias
                .as_container()
                .map(|alias| alias.get_element_edit_value("ALID"))
                .unwrap_or_default();
            let mut text = format!("{id:0>3}");
            if !name.is_empty() {
                text = format!("{text} {name}");
            }
            if let Some(edit_infos) = &mut edit_infos {
                edit_infos.push(text);
            } else if id == a_int {
                match a_type {
                    CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => result = text,
                    CallbackType::ctCheck => result = String::new(),
                    _ => {}
                }
                return result;
            }
        }
    }
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            result = a_int.to_string();
            if a_type == CallbackType::ctToStr {
                result.push_str(&format!(
                    " <Warning: Quest Alias [{a_int}] not found in \"{}\">",
                    main_record.get_name()
                ));
            }
        }
        CallbackType::ctCheck => {
            result = format!(
                "<Warning: Quest Alias [{a_int}] not found in \"{}\">",
                main_record.get_name()
            );
        }
        CallbackType::ctEditInfo => {
            let mut edit_infos = edit_infos.unwrap_or_default();
            edit_infos.push("None".to_owned());
            edit_infos.sort_by_key(|text| text.to_lowercase());
            result = to_comma_text(&edit_infos);
        }
        _ => {}
    }
    result
}

/// Upstream `wbAliasToInt`: the ID from the text of an alias.
pub fn wb_alias_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    if a_string == "None" {
        return -1;
    }
    if a_string == "Player" && !is_skyrim() {
        return -2;
    }
    if is_starfield() {
        match a_string {
            "Non-Actor Track" => return -3,
            "Play Audio At Player(Voice Note)" => return -4,
            "Dialogue For Scene" => return -5,
            _ => {}
        }
    }
    let text = a_string.trim();
    let digits: String = text.chars().take_while(|c| *c == '-' || c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(-1)
}

/// The text of an alias when they are not resolved: the ID.
fn unresolved_alias_text(a_int: i64, a_type: CallbackType) -> String {
    match a_type {
        CallbackType::ctToSortKey => int_to_hex64(a_int, 8),
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => a_int.to_string(),
        _ => String::new(),
    }
}

/// Upstream `wbConditionAliasToStr`: the alias of the quest the condition
/// belongs to, found through the record that holds the condition.
pub fn wb_condition_alias_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let Some(element) = a_element else {
        return String::new();
    };
    if !resolve_alias() {
        return unresolved_alias_text(a_int, a_type);
    }
    let Some(main_record) = element.get_containing_main_record() else {
        return String::new();
    };
    match main_record.get_signature().0.as_slice() {
        b"QUST" => {
            let quest: ElementRef = main_record;
            wb_alias_to_str(a_int, Some(&quest), a_type)
        }
        b"SCEN" => wb_alias_to_str(
            a_int,
            main_record.get_element_by_signature(Signature::new(b"PNAM")).as_ref(),
            a_type,
        ),
        b"PACK" => wb_alias_to_str(
            a_int,
            main_record.get_element_by_signature(Signature::new(b"QNAM")).as_ref(),
            a_type,
        ),
        b"TERM" if is_fallout76() => wb_alias_to_str(
            a_int,
            main_record.get_element_by_signature(Signature::new(b"QNAM")).as_ref(),
            a_type,
        ),
        b"INFO" => {
            // The DIAL of the INFO.
            let Some(topic) = main_record
                .get_element_by_name("Topic")
                .and_then(|topic| topic.get_links_to())
                .and_then(|topic| topic.into_main_record())
            else {
                return String::new();
            };
            let Some(file) = element.get_file() else {
                return String::new();
            };
            let Some(topic) = topic.get_highest_override_visible_for_file(&file) else {
                return String::new();
            };
            wb_alias_to_str(
                a_int,
                topic.get_element_by_signature(Signature::new(b"QNAM")).as_ref(),
                a_type,
            )
        }
        _ => String::new(),
    }
}

/// Upstream `wbPackageLocationAliasToStr`.
pub fn wb_package_location_alias_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let Some(element) = a_element else {
        return String::new();
    };
    if !resolve_alias() {
        return unresolved_alias_text(a_int, a_type);
    }
    let Some(main_record) = element.get_containing_main_record() else {
        return String::new();
    };
    wb_alias_to_str(
        a_int,
        main_record.get_element_by_signature(Signature::new(b"QNAM")).as_ref(),
        a_type,
    )
}

/// Upstream `wbQuestAliasToStr`.
pub fn wb_quest_alias_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let Some(element) = a_element else {
        return String::new();
    };
    if !resolve_alias() {
        return unresolved_alias_text(a_int, a_type);
    }
    let Some(main_record) = element.get_containing_main_record() else {
        return String::new();
    };
    let quest: ElementRef = main_record;
    wb_alias_to_str(a_int, Some(&quest), a_type)
}

/// Upstream `wbQuestExternalAliasToStr`.
///
/// UPSTREAM-QUIRK: upstream never assigns the container it looks the
/// `ALEQ` up in, so the resolved form is always empty.
pub fn wb_quest_external_alias_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    if a_element.is_none() {
        return String::new();
    }
    if resolve_alias() {
        return String::new();
    }
    unresolved_alias_text(a_int, a_type)
}

/// Upstream `wbSceneAliasToStr`.
pub fn wb_scene_alias_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let Some(element) = a_element else {
        return String::new();
    };
    if !resolve_alias() {
        return unresolved_alias_text(a_int, a_type);
    }
    let Some(main_record) = element.get_containing_main_record() else {
        return String::new();
    };
    wb_alias_to_str(
        a_int,
        main_record.get_element_by_signature(Signature::new(b"PNAM")).as_ref(),
        a_type,
    )
}

/// Upstream `wbScriptObjectAliasToStr`.
pub fn wb_script_object_alias_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let Some(element) = a_element else {
        return String::new();
    };
    if !resolve_alias() {
        return unresolved_alias_text(a_int, a_type);
    }
    let Some(container) = element.get_container() else {
        return String::new();
    };
    wb_alias_to_str(
        a_int,
        container
            .as_container()
            .and_then(|container| container.get_element_by_name("FormID"))
            .as_ref(),
        a_type,
    )
}

/// Upstream `wbSCENAliasLinksTo`.
pub fn wb_scen_alias_links_to(a_element: ElementArg) -> Option<ElementRef> {
    if !resolve_alias() {
        return None;
    }
    let element = a_element?;
    let main_record = element.get_containing_main_record()?;
    let alias = element.get_native_value().as_ordinal()?;
    wb_alias_links_to(
        alias,
        main_record.get_element_by_signature(Signature::new(b"PNAM")).as_ref(),
    )
}

/// Upstream `wbScriptObjectAliasLinksTo`.
pub fn wb_script_object_alias_links_to(a_element: ElementArg) -> Option<ElementRef> {
    if !resolve_alias() {
        return None;
    }
    let element = a_element?;
    let container = wb_try_get_container_ref_from_union_or_value(Some(element))?;
    let alias = element.get_native_value().as_ordinal()?;
    wb_alias_links_to(
        alias,
        container
            .as_container()
            .and_then(|container| container.get_element_by_name("FormID"))
            .as_ref(),
    )
}

/// Upstream `wbConditionToStr`: the condition as an expression.
pub fn wb_condition_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    let Some(container) = wb_try_set_container(a_element, a_type) else {
        return;
    };
    let Some(container_ref) = container.as_container() else {
        return;
    };
    let ctda: ElementRef = if game_mode() > GameMode::gmFNV {
        let Some(ctda) = container_ref
            .get_record_by_signature(Signature::new(b"CTDA"))
            .filter(|ctda| ctda.as_container().is_some())
        else {
            return;
        };
        ctda
    } else {
        container.clone()
    };
    let Some(cer) = ctda.as_container() else { return };
    let element = |index: i32| cer.get_element(index);
    let typ = element(0).map_or(0, |e| variant_int(&e.get_native_value())) as u8;
    let Some(func) = element(3) else { return };
    let def_type = |e: &ElementRef| e.get_def().map(|def| def.get_def_type());
    if cer.get_element_count() >= 9
        && element(7).is_some_and(|e| def_type(&e) != Some(DefType::dtEmpty))
        && element(8).is_some_and(|e| def_type(&e) != Some(DefType::dtEmpty))
    {
        let run_on = element(7).unwrap();
        let mut run_on_int = variant_int(&run_on.get_native_value());
        if is_fallout_nv() {
            let func_int = variant_int(&func.get_native_value());
            if func_int == 106 || func_int == 285 {
                run_on_int = 0;
            }
        }
        if run_on_int == 2 {
            *a_value = format!("({})", element(8).map(|e| e.get_summary()).unwrap_or_default());
        } else {
            *a_value = run_on.get_summary().replace(' ', "");
        }
    } else if typ & 0x02 == 0 {
        *a_value = "Subject".to_owned();
    } else {
        *a_value = "Target".to_owned();
    }
    a_value.push('.');
    a_value.push_str(&func.get_summary());
    if let Some(param1) = element(5)
        && param1.get_conflict_priority() != ConflictPriority::cpIgnore
    {
        a_value.push('(');
        a_value.push_str(&param1.get_summary());
        if let Some(param2) = element(6)
            && param2.get_conflict_priority() != ConflictPriority::cpIgnore
        {
            a_value.push_str(", ");
            a_value.push_str(&param2.get_summary());
        }
        a_value.push(')');
    }
    match typ & 0xE0 {
        0x00 => a_value.push_str(" = "),
        0x20 => a_value.push_str(" <> "),
        0x40 => a_value.push_str(" > "),
        0x60 => a_value.push_str(" >= "),
        0x80 => a_value.push_str(" < "),
        0xA0 => a_value.push_str(" <= "),
        _ => {}
    }
    a_value.push_str(&element(2).map(|e| e.get_summary()).unwrap_or_default());
    if let Some(conditions) = container.get_container()
        && let Some(conditions_ref) = conditions.as_container()
    {
        let count = conditions_ref.get_element_count();
        if count < 2
            || conditions_ref
                .get_element(count - 1)
                .is_some_and(|last| last.get_element_id() == container.get_element_id())
        {
            return;
        }
    }
    if typ & 0x01 == 0 {
        a_value.push_str(" AND");
    } else {
        a_value.push_str(" OR");
    }
}

/// Upstream `wbConditionOwnerToStr`: the player for a null owner.
pub fn wb_condition_owner_to_str(
    a_value: &mut String,
    _a_base_ptr: DataPtr,
    a_element: ElementArg,
    a_type: CallbackType,
) {
    let Some(element) = a_element else { return };
    if variant_int(&element.get_native_value()) != 0 {
        return;
    }
    if matches!(a_type, CallbackType::ctToStr | CallbackType::ctToSummary) {
        *a_value = "Player [NPC_:00000000]".to_owned();
    }
}

/// Upstream `wbConditionStringToStr`: the `CIS1` or `CIS2` text of the
/// condition for the parameter.
pub fn wb_condition_string_to_str(_a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let Some(element) = a_element else {
        return String::new();
    };
    let Some(container) = get_container_from_union(element) else {
        return String::new();
    };
    let Some(cer) = container.as_container() else {
        return String::new();
    };
    match a_type {
        CallbackType::ctToEditValue | CallbackType::ctToNativeValue | CallbackType::ctToSummary => {
            let is_element = |index: i32| {
                cer.get_element(index)
                    .is_some_and(|other| other.get_element_id() == element.get_element_id())
            };
            if is_element(5) {
                cer.get_element_edit_value("..\\CIS1")
            } else if is_element(6) {
                cer.get_element_edit_value("..\\CIS2")
            } else {
                String::new()
            }
        }
        CallbackType::ctToSortKey => "0".to_owned(),
        _ => String::new(),
    }
}

/// Upstream `wbConditionTypeToStr`: the compare operator and the flags.
pub fn wb_condition_type_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let is_tes4_fo3 = |a: &'static str, b: &'static str| if game_mode() <= GameMode::gmFNV { a } else { b };
    let flags = wb_flags_unknown_is_unused(
        &[
            "Or",
            is_tes4_fo3("Run On Target", "Use Aliases"),
            "Use Global",
            is_tes4_fo3("", "Use Packdata"),
            is_tes4_fo3("", "Swap Subject and Target"),
        ],
        false,
    )
    .expect("a flags definition");
    match a_type {
        CallbackType::ctEditType => "CheckComboBox".to_owned(),
        CallbackType::ctEditInfo => is_tes4_fo3(
            "\"Equal To\", \"Greater Than\", \"Less Than\", \"Or\", \"Run On Target\", \"Use Global\"",
            "\"Equal To\", \"Greater Than\", \"Less Than\", \"Or\", \"Use Aliases\", \"Use Global\", \"Use Packdata\", \"Swap Subject and Target\"",
        )
        .to_owned(),
        CallbackType::ctToEditValue => {
            let mut result = *b"00000000";
            match a_int & 224 {
                0 => result[0] = b'1',
                64 => result[1] = b'1',
                96 => {
                    result[0] = b'1';
                    result[1] = b'1';
                }
                128 => result[2] = b'1',
                160 => {
                    result[0] = b'1';
                    result[2] = b'1';
                }
                _ => {}
            }
            for (bit, index) in [(1, 3), (2, 4), (4, 5), (8, 6), (16, 7)] {
                if a_int & bit != 0 {
                    result[index] = b'1';
                }
            }
            String::from_utf8_lossy(&result).into_owned()
        }
        CallbackType::ctToSortKey => int_to_hex64(a_int, 2),
        CallbackType::ctCheck => {
            let mut result = match a_int & 224 {
                0 | 32 | 64 | 96 | 128 | 160 => String::new(),
                _ => "<Unknown Compare Operator>".to_owned(),
            };
            let s = flags.check(a_int & 31, a_element);
            if !s.is_empty() {
                result = format!("{result} / {s}");
            }
            result
        }
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            let mut result = match a_int & 224 {
                0 => "Equal To",
                32 => "Not Equal To",
                64 => "Greater Than",
                96 => "Greater Than Or Equal To",
                128 => "Less Than",
                160 => "Less Than Or Equal To",
                _ => "<Unknown Compare Operator>",
            }
            .to_owned();
            let s = flags.to_string(a_int & 31, a_element, a_type == CallbackType::ctToSummary);
            if !s.is_empty() {
                result = format!("{result} / {s}");
            }
            result
        }
        _ => String::new(),
    }
}

/// Upstream `wbConditionTypeToInt`: the value from the edit text of flags.
pub fn wb_condition_type_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    let s: Vec<u8> = format!("{a_string}00000000").into_bytes();
    let bit = |index: usize| s[index] == b'1';
    let mut result = if bit(0) {
        if bit(1) {
            if bit(2) { 0 } else { 96 }
        } else if bit(2) {
            160
        } else {
            0
        }
    } else if bit(1) {
        if bit(2) { 32 } else { 64 }
    } else if bit(2) {
        128
    } else {
        32
    };
    // Or; Run On Target or Use Aliases; Use global; Use Packdata; Swap
    // Subject and Target.
    for (index, value) in [(3, 1), (4, 2), (5, 4), (6, 8), (7, 16)] {
        if bit(index) {
            result |= value;
        }
    }
    result
}

/// Upstream `wbStrToInt`: the number before a space or colon, 0 otherwise.
pub fn wb_str_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    let end = a_string.find(' ').or_else(|| a_string.find(':'));
    let text = match end {
        Some(end) => &a_string[..end],
        None => a_string,
    };
    str_to_int64(text).unwrap_or(0)
}

/// Upstream `wbNPCPackageToStr`: warns about a package owned by a quest.
pub fn wb_npc_package_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    let Some(element) = a_element else { return };
    let Some(pack_record) = element.get_links_to().and_then(|links_to| links_to.into_main_record()) else {
        return;
    };
    let Some(qnam) = pack_record.get_element_by_signature(Signature::new(b"QNAM")) else {
        return;
    };
    let Some(qust_record) = qnam.get_links_to().and_then(|links_to| links_to.into_main_record()) else {
        return;
    };
    match a_type {
        CallbackType::ctCheck => {
            *a_value = format!(
                "<Error: Package [{}] is owned by Quest [{}] and cannot be assigned to an NPC record>",
                pack_record.get_editor_id(),
                qust_record.get_editor_id()
            );
        }
        CallbackType::ctToStr => {
            *a_value = format!(
                "{} <Error: Package is owned by Quest [{}] and cannot be assigned to an NPC record>",
                element.get_edit_value(),
                qust_record.get_editor_id()
            );
        }
        _ => {}
    }
}

/// Upstream `wbQUSTAliasToStr`: warns about an alias forced to none.
pub fn wb_qust_alias_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    let Some(alias) = a_element else { return };
    let Some(cer) = alias.as_container() else { return };
    let Some(flags) = cer.get_element_by_signature(Signature::new(b"FNAM")) else {
        return;
    };
    // UPSTREAM-QUIRK: a subrecord whose value is an element of its own, such
    // as the flags union of Fallout 76, has the native value Null, and
    // `Null and $2 <> 0` is true under the loose null rules of Delphi: the
    // alias counts as optional.
    let native = flags.get_native_value();
    if native == Variant::Empty && flags.get_element_type() == ElementType::etSubRecord {
        return;
    }
    let flags_value = variant_int(&native);
    // Optional, or Forced By Aliases.
    if flags_value & 0x2 != 0 || flags_value & 0x800 != 0 {
        return;
    }
    let name = alias.get_name();
    if name == "Reference Alias"
        && [
            "Specific Reference",
            "Unique Actor",
            "Unique Reference",
            "Location Alias Reference",
            "External Alias Reference",
            "Create Reference To Object",
            "Create Matching Ref",
            "Find Matching Reference",
            "Match Conditions",
        ]
        .iter()
        .any(|member| cer.get_element_by_name(member).is_some())
    {
        return;
    }
    if name == "Location Alias"
        && (cer.get_element_by_signature(Signature::new(b"ALFL")).is_some()
            || [
                "Reference Alias Location",
                "External Alias Location",
                "Find Matching Location",
                "Match Conditions",
            ]
            .iter()
            .any(|member| cer.get_element_by_name(member).is_some()))
    {
        return;
    }
    match a_type {
        CallbackType::ctCheck => {
            *a_value = format!("<Warning: {} is non-optional and Forced to NONE>", alias.get_summary());
        }
        CallbackType::ctToStr => {
            *a_value = format!(
                "{} <Warning: Alias is non-optional and Forced to NONE>",
                alias.get_summary()
            );
        }
        _ => {}
    }
}

/// The record flags of the containing main record, 0 without one.
fn containing_record_flags(a_element: ElementArg) -> u32 {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .map_or(0, |main_record| main_record.get_flags().0)
}

/// Upstream `wbFlagNavmeshFilterDontSHow`.
pub fn wb_flag_navmesh_filter_dont_s_how(a_element: ElementArg) -> bool {
    let flags = containing_record_flags(a_element);
    flags & 0x800_0000 != 0
        || (flags & 0x1000_0000 != 0 && is_starfield())
        || (flags & 0x2000_0000 != 0 && is_starfield())
        || flags & 0x4000_0000 != 0
}

/// Upstream `wbFlagNavmeshBoundingBoxDontSHow`.
pub fn wb_flag_navmesh_bounding_box_dont_s_how(a_element: ElementArg) -> bool {
    let flags = containing_record_flags(a_element);
    flags & 0x400_0000 != 0
        || (flags & 0x1000_0000 != 0 && is_starfield())
        || (flags & 0x2000_0000 != 0 && is_starfield())
        || flags & 0x4000_0000 != 0
}

/// Upstream `wbFlagNavmeshGroundDontSHow`.
pub fn wb_flag_navmesh_ground_dont_s_how(a_element: ElementArg) -> bool {
    let flags = containing_record_flags(a_element);
    flags & 0x400_0000 != 0
        || flags & 0x800_0000 != 0
        || (flags & 0x1000_0000 != 0 && is_starfield())
        || (flags & 0x2000_0000 != 0 && is_starfield())
}

/// Upstream `wbBookTeachesDontSHow`: hidden for a book that teaches nothing.
pub fn wb_book_teaches_dont_s_how(a_element: ElementArg) -> bool {
    let flags = a_element
        .and_then(|element| element.get_container())
        .map_or(0, |container| container_int(&container, "Flags"));
    flags & 0x1 == 0 && flags & 0x4 == 0
}

/// The `DATA\Flags` of the containing light record.
fn light_flags(a_element: ElementArg) -> Option<i64> {
    let main_record = a_element?.get_containing_main_record()?;
    let data = main_record.get_element_by_signature(Signature::new(b"DATA"))?;
    let flags = data.as_container()?.get_element_by_name("Flags")?;
    Some(variant_int(&flags.get_native_value()))
}

/// Upstream `wbLIGHCarryDontShow`: hidden unless the light can be carried.
pub fn wb_ligh_carry_dont_show(a_element: ElementArg) -> bool {
    light_flags(a_element).is_some_and(|flags| flags & 0x2 == 0)
}

/// Upstream `wbLIGHFalloffDontShow`: hidden unless the light is a shadow
/// spotlight or hemisphere.
pub fn wb_ligh_falloff_dont_show(a_element: ElementArg) -> bool {
    light_flags(a_element)
        .is_some_and(|flags| flags & 0x400 == 0 && flags & 0x800 == 0 && (!cs() || flags & 0x4000 == 0))
}

/// Upstream `wbLIGHFlickerDontShow`: hidden unless the light flickers or pulses.
pub fn wb_ligh_flicker_dont_show(a_element: ElementArg) -> bool {
    light_flags(a_element)
        .is_some_and(|flags| flags & 0x8 == 0 && flags & 0x40 == 0 && flags & 0x80 == 0 && flags & 0x100 == 0)
}

/// Upstream `wbLIGHShadowSpotDontShow`: hidden unless the light is a shadow spotlight.
pub fn wb_ligh_shadow_spot_dont_show(a_element: ElementArg) -> bool {
    light_flags(a_element).is_some_and(|flags| flags & 0x400 == 0 && (!cs() || flags & 0x4000 == 0))
}

/// Upstream `wbMESGTNAMDontShow`: hidden for a message box.
pub fn wb_mesgtnam_dont_show(a_element: ElementArg) -> bool {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .is_some_and(|main_record| variant_int(&main_record.get_element_native_value("DNAM")) & 1 != 0)
}

/// Upstream `wbLCTNCellDontShow`: hidden when the location is a cell.
pub fn wb_lctn_cell_dont_show(a_element: ElementArg) -> bool {
    let Some(container) = a_element.and_then(|element| element.get_container()) else {
        return false;
    };
    container
        .as_container()
        .and_then(|container| container.get_element_by_name("World/Cell"))
        .and_then(|location| location.get_links_to())
        .and_then(|links_to| links_to.into_main_record())
        .is_some_and(|main_record| main_record.get_signature() == Signature::new(b"CELL"))
}

/// Upstream `wbPACKTemplateDontShow`: hidden for a package with a template.
pub fn wb_pack_template_dont_show(a_element: ElementArg) -> bool {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .is_some_and(|main_record| variant_int(&main_record.get_element_native_value("PKCU\\Package Template")) != 0)
}

/// Upstream `wbREGNGrassDontShow`.
pub fn wb_regn_grass_dont_show(a_element: ElementArg) -> bool {
    wb_get_regn_type(a_element) != 6
}

/// Upstream `wbREGNLandDontShow`.
pub fn wb_regn_land_dont_show(a_element: ElementArg) -> bool {
    wb_get_regn_type(a_element) != 5
}

/// Upstream `wbREGNMapDontShow`.
pub fn wb_regn_map_dont_show(a_element: ElementArg) -> bool {
    wb_get_regn_type(a_element) != 4
}

/// Upstream `wbREGNObjectsDontShow`.
pub fn wb_regn_objects_dont_show(a_element: ElementArg) -> bool {
    wb_get_regn_type(a_element) != 2
}

/// Upstream `wbREGNWeatherDontShow`.
pub fn wb_regn_weather_dont_show(a_element: ElementArg) -> bool {
    wb_get_regn_type(a_element) != 3
}

/// Upstream `wbWorldXWEMDontShow`: hidden for an exterior.
pub fn wb_world_xwem_dont_show(a_element: ElementArg) -> bool {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .is_some_and(|main_record| variant_int(&main_record.get_element_native_value("DATA")) & 1 == 0)
}

/// Upstream `wbCellLightingIsRemovable`.
pub fn wb_cell_lighting_is_removable(a_element: ElementArg) -> bool {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .is_some_and(|main_record| variant_int(&main_record.get_element_native_value("DATA")) & 1 == 0)
}

/// Upstream `wbMessageTNAMIsRemovable`.
pub fn wb_message_tnam_is_removable(a_element: ElementArg) -> bool {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .is_some_and(|main_record| variant_int(&main_record.get_element_native_value("DNAM")) & 1 == 1)
}

/// Upstream `wbWorldWaterIsRemovable`.
pub fn wb_world_water_is_removable(a_element: ElementArg) -> bool {
    if is_oblivion() {
        a_element
            .and_then(|element| element.get_containing_main_record())
            .is_some_and(|main_record| main_record.get_record_by_signature(Signature::new(b"WNAM")).is_some())
    } else {
        parent_worldspace_flags(a_element) & 0x08 == 8
    }
}

/// Upstream `wbWorldClimateIsRemovable`.
pub fn wb_world_climate_is_removable(a_element: ElementArg) -> bool {
    if is_oblivion() {
        a_element
            .and_then(|element| element.get_containing_main_record())
            .is_some_and(|main_record| main_record.get_record_by_signature(Signature::new(b"WNAM")).is_some())
    } else {
        parent_worldspace_flags(a_element) & 0x10 == 16
    }
}

/// Upstream `wbClmtTime`: the time of day in ten minute steps, as
/// `TimeToStr` prints it with the English long time format.
pub fn wb_clmt_time(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    let mut a_int = a_int;
    while a_int > 143 {
        a_int -= 143;
    }
    match a_type {
        CallbackType::ctToSortKey => int_to_hex64(a_int, 4),
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            let hour = a_int / 6;
            let minute = (a_int % 6) * 10;
            let (hour12, suffix) = match hour {
                0 => (12, "AM"),
                1..=11 => (hour, "AM"),
                12 => (12, "PM"),
                _ => (hour - 12, "PM"),
            };
            format!("{hour12}:{minute:02}:00 {suffix}")
        }
        _ => String::new(),
    }
}

/// Upstream `wbClmtMoonsPhaseLength`: the moons and the phase length.
pub fn wb_clmt_moons_phase_length(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    match a_type {
        CallbackType::ctToSortKey => int_to_hex64(a_int, 2),
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            let phase_length = a_int % 64;
            let secunda = a_int & 64 != 0;
            let masser = a_int & 128 != 0;
            let moons = match (masser, secunda) {
                (true, true) => "Masser, Secunda / ",
                (true, false) => "Masser / ",
                (false, true) => "Secunda / ",
                (false, false) => "No Moon / ",
            };
            format!("{moons}{phase_length}")
        }
        _ => String::new(),
    }
}

/// Upstream `wbREFRNavmeshTriangleToStr`: the triangle of the linked navmesh.
pub fn wb_refr_navmesh_triangle_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let mut result = match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => a_int.to_string(),
        CallbackType::ctToSortKey => return int_to_hex64(a_int, 8),
        _ => String::new(),
    };
    let Some(container) = wb_try_get_container_ref_from_union_or_value(a_element) else {
        return result;
    };
    let navmesh = container.as_container().and_then(|container| container.get_element(0));
    let Some(main_record) = wb_try_get_main_record(navmesh.as_ref(), "") else {
        return result;
    };
    let main_record = main_record.get_winning_override();
    if main_record.get_signature() != Signature::new(b"NAVM") {
        match a_type {
            CallbackType::ctToStr | CallbackType::ctToSummary => {
                result = a_int.to_string();
                if a_type == CallbackType::ctToStr {
                    result.push_str(&format!(
                        " <Warning: \"{}\" is not a Navmesh record>",
                        main_record.get_short_name()
                    ));
                }
            }
            CallbackType::ctCheck => {
                result = format!(
                    "<Warning: \"{}\" is not a Navmesh record>",
                    main_record.get_short_name()
                );
            }
            _ => {}
        }
        return result;
    }
    if a_type == CallbackType::ctCheck {
        let triangles = main_record
            .get_element_by_path("NVTR")
            .or_else(|| main_record.get_element_by_path("NVNM\\Triangles"))
            .filter(|triangles| triangles.as_container().is_some());
        if let Some(triangles) = triangles
            && let Some(triangles) = triangles.as_container()
            && a_int >= i64::from(triangles.get_element_count())
        {
            result = format!(
                "<Warning: Navmesh triangle [{a_int}] not found in \"{}\">",
                main_record.get_name()
            );
        }
    }
    result
}

/// Upstream `wbPackagePSDTMonthValueToStr`: the month, offset by one from
/// form version 122.
pub fn wb_package_psdt_month_value_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return format!("Unknown: {a_int}");
    };
    let a_int = if main_record.get_version() >= 122 {
        a_int - 1
    } else {
        a_int
    };
    let Some(months) = wb_package_schedule_month_enum() else {
        return String::new();
    };
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            months.to_string(a_int, a_element, a_type == CallbackType::ctToSummary)
        }
        CallbackType::ctToSortKey => int_to_hex64(a_int, 16),
        CallbackType::ctCheck => months.check(a_int, a_element),
        CallbackType::ctToEditValue => months.to_edit_value(a_int, a_element),
        CallbackType::ctEditType => "ComboBox".to_owned(),
        CallbackType::ctEditInfo => to_comma_text(&months.get_edit_info(a_element)),
        _ => String::new(),
    }
}

/// Upstream `wbPackagePSDTMonthValueToInt`.
pub fn wb_package_psdt_month_value_to_int(a_string: &str, a_element: ElementArg) -> i64 {
    let mut result = wb_package_schedule_month_enum()
        .and_then(|months| months.find_name(a_string))
        .unwrap_or_else(|| a_string.trim().parse().unwrap_or(0));
    let Some(main_record) = a_element.and_then(|element| element.get_containing_main_record()) else {
        return 0;
    };
    if main_record.get_version() >= 122 {
        result += 1;
    }
    result
}

/// Upstream `wbQuestStageToInt`: the leading digits of the text.
pub fn wb_quest_stage_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    let digits: String = a_string.trim().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

/// Upstream `wbQUSTEventToStr`: warns about a quest the story manager
/// does not know.
///
/// UPSTREAM-QUIRK: upstream looks for an `SMQN` among the references to the
/// quest. The references are only built on request, which the dump never
/// does, so the quest is never found and the warning always applies, as
/// the oracle prints it.
pub fn wb_qust_event_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    let Some(element) = a_element else { return };
    let Some(main_record) = element.get_containing_main_record() else {
        return;
    };
    match a_type {
        CallbackType::ctCheck => {
            *a_value = format!(
                "<Warning: {} has not been added to the story manager>",
                main_record.get_short_name()
            );
        }
        CallbackType::ctToStr => {
            *a_value = format!(
                "{}<Warning: {} has not been added to the story manager>",
                element.get_edit_value(),
                main_record.get_short_name()
            );
        }
        _ => {}
    }
}

/// Upstream `wbTriangleLinksTo`: the triangle of the navmesh at the index.
pub fn wb_triangle_links_to(a_element: ElementArg) -> Option<ElementRef> {
    let element = a_element?;
    element.get_container()?.as_container()?;
    let main_record = element.get_containing_main_record()?;
    let index = variant_int(&element.get_native_value());
    let triangles = main_record.get_element_by_path("NVNM\\Triangles")?;
    let triangles = triangles.as_container()?;
    if index < 0 || index >= i64::from(triangles.get_element_count()) {
        return None;
    }
    triangles.get_element(index as i32)
}

/// Upstream `wbVertexLinksTo`: the vertex of the navmesh at the index.
pub fn wb_vertex_links_to(a_element: ElementArg) -> Option<ElementRef> {
    let element = a_element?;
    element.get_container()?.as_container()?;
    let main_record = element.get_containing_main_record()?;
    let vertices = main_record.get_element_by_path("NVNM\\Vertices")?;
    let vertices = vertices.as_container()?;
    let index = variant_int(&element.get_native_value());
    if index < 0 || index >= i64::from(vertices.get_element_count()) {
        return None;
    }
    vertices.get_element(index as i32)
}

/// The triangle container, the containing navmesh record and the value of
/// a vertex or edge element.
fn navmesh_context(a_element: ElementArg) -> Option<(ElementRef, MainRecordRef, i64)> {
    let element = a_element?;
    let triangle = element.get_container()?;
    triangle.as_container()?;
    let main_record = element.get_containing_main_record()?;
    Some((triangle, main_record, variant_int(&element.get_native_value())))
}

/// The element at `index` of the container at `path` of the record.
fn navmesh_item(main_record: &MainRecordRef, path: &str, index: i64) -> Option<ElementRef> {
    let items = main_record.get_element_by_path(path)?;
    let items = items.as_container()?;
    if index < 0 || index >= i64::from(items.get_element_count()) {
        return None;
    }
    items.get_element(index as i32)
}

/// Upstream `wbEdgeLinksTo`: the triangle across the edge, through the
/// edge links of another navmesh when the edge flag says so.
pub fn wb_edge_links_to(a_edge: i32, a_element: ElementArg) -> Option<ElementRef> {
    let (triangle, mut main_record, mut index) = navmesh_context(a_element)?;
    let flags = container_int(&triangle, "Flags");
    if flags & (1 << a_edge) != 0 {
        let edge_link = navmesh_item(&main_record, "NVNM\\Edge Links", index)?;
        let edge_link = edge_link.as_container()?;
        main_record = edge_link.get_element_links_to("Navmesh")?.into_main_record()?;
        index = variant_int(&edge_link.get_element_native_value("Triangle"));
    }
    navmesh_item(&main_record, "NVNM\\Triangles", index)
}

/// Upstream `wbEdgeToStr`: the triangle index, with the navmesh of an
/// edge link.
pub fn wb_edge_to_str(a_edge: i32, a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            if a_int < 0 {
                return "None".to_owned();
            }
            let mut result = a_int.to_string();
            let Some((triangle, main_record, _)) = navmesh_context(a_element) else {
                return result;
            };
            let flags = container_int(&triangle, "Flags");
            if flags & (1 << a_edge) != 0 {
                let Some(edge_link) = navmesh_item(&main_record, "NVNM\\Edge Links", a_int) else {
                    return result;
                };
                let Some(edge_link) = edge_link.as_container() else {
                    return result;
                };
                let Some(main_record) = edge_link
                    .get_element_links_to("Navmesh")
                    .and_then(|navmesh| navmesh.into_main_record())
                else {
                    return result;
                };
                let index = variant_int(&edge_link.get_element_native_value("Triangle"));
                result.push_str(&format!(" (#{index} in {})", main_record.get_name()));
            }
            result
        }
        CallbackType::ctToSortKey => {
            let mut result = format!("00000000{}", int_to_hex64(a_int, 4));
            let Some((triangle, main_record, _)) = navmesh_context(a_element) else {
                return result;
            };
            let mut form_id = main_record.get_load_order_form_id();
            let mut index = a_int;
            let flags = container_int(&triangle, "Flags");
            if flags & (1 << a_edge) != 0 {
                let Some(edge_link) = navmesh_item(&main_record, "NVNM\\Edge Links", a_int) else {
                    return result;
                };
                let Some(edge_link) = edge_link.as_container() else {
                    return result;
                };
                let Some(main_record) = edge_link
                    .get_element_links_to("Navmesh")
                    .and_then(|navmesh| navmesh.into_main_record())
                else {
                    return result;
                };
                form_id = main_record.get_load_order_form_id();
                index = variant_int(&edge_link.get_element_native_value("Triangle"));
            }
            result = format!("{}{}", form_id.to_string(false), int_to_hex64(index, 4));
            result
        }
        CallbackType::ctToEditValue => {
            if a_int < 0 {
                String::new()
            } else {
                a_int.to_string()
            }
        }
        _ => String::new(),
    }
}

/// Upstream `wbEdgeToInt`.
pub fn wb_edge_to_int(_a_edge: i32, a_string: &str, _a_element: ElementArg) -> i64 {
    let text = a_string.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("None") {
        -1
    } else {
        i64::from(str_to_int_def(a_string, 0))
    }
}

/// Upstream `wbVertexToStr`: the vertex index with its position.
///
/// UPSTREAM-QUIRK: the sort key of the vertex is not ported; the sort key
/// form gives the index only.
pub fn wb_vertex_to_str(_a_vertex: i32, a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            let mut result = a_int.to_string();
            let Some((_, main_record, _)) = navmesh_context(a_element) else {
                return result;
            };
            let Some(vertex) = navmesh_item(&main_record, "NVNM\\Vertices", a_int) else {
                return result;
            };
            if let Some(vertex) = vertex.as_container() {
                result.push_str(&format!(
                    " ({}, {}, {})",
                    vertex.get_element_edit_value("X"),
                    vertex.get_element_edit_value("Y"),
                    vertex.get_element_edit_value("Z")
                ));
            }
            result
        }
        CallbackType::ctToSortKey => int_to_hex64(a_int, 4),
        CallbackType::ctToEditValue => a_int.to_string(),
        _ => String::new(),
    }
}

/// Upstream `wbVertexToInt`.
pub fn wb_vertex_to_int(_a_vertex: i32, a_string: &str, _a_element: ElementArg) -> i64 {
    i64::from(str_to_int_def(a_string, 0))
}

/// The index keys callback of a record whose key is an ordinal subrecord
/// value: `ADDN` by `DATA`, `COLL` by `BNAM`.
pub fn index_key_from_ordinal(
    a_main_record: &MainRecordRef,
    a_index_keys: &mut IndexKeys,
    signature: &str,
    index: i32,
) {
    let value = a_main_record.get_element_native_value(signature);
    let Some(ordinal) = value.as_ordinal() else { return };
    a_index_keys.set_key(index, &ordinal.to_string());
}

/// The links-to callback of a collision layer index (`XTRI`): the `COLL`
/// record with that index in the file or its masters.
pub fn collision_layer_links_to(a_element: ElementArg) -> Option<ElementRef> {
    let element = a_element?;
    let index = element.get_native_value().as_ordinal()?;
    let file = element.get_file()?;
    file.get_record_from_index_by_key(wb_idx_collision_layer(), &index.to_string())
        .map(|record| record as ElementRef)
}

/// Upstream `wbIdleMarkerPNAMDontShow`: hidden when the record has `QNAM`.
pub fn wb_idle_marker_pnam_dont_show(a_element: ElementArg) -> bool {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .is_some_and(|main_record| main_record.get_element_by_signature(Signature::new(b"QNAM")).is_some())
}

/// Upstream `wbIdleMarkerQNAMDontShow`: hidden when the record has `PNAM`.
pub fn wb_idle_marker_qnam_dont_show(a_element: ElementArg) -> bool {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .is_some_and(|main_record| main_record.get_element_by_signature(Signature::new(b"PNAM")).is_some())
}

/// Upstream `wbTemplateActorsDontShow`: hidden without template flags.
pub fn wb_template_actors_dont_show(a_element: ElementArg) -> bool {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .is_some_and(|main_record| {
            variant_int(&main_record.get_element_native_value("ACBS\\Template Flags")) as u32 == 0
        })
}

/// Upstream `wbCoverLinksTo`: the cover of the navmesh at the index.
pub fn wb_cover_links_to(a_element: ElementArg) -> Option<ElementRef> {
    let element = a_element?;
    element.get_container()?.as_container()?;
    let main_record = element.get_containing_main_record()?;
    let index = variant_int(&element.get_native_value());
    navmesh_item(&main_record, "NVNM\\Cover Array", index)
}

/// Upstream `wbNoteTypeDecider`: the member for the `DNAM` type of the note.
pub fn wb_note_type_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let Some(container) = container.get_container() else {
        return 0;
    };
    let Some(dnam) = container
        .as_container()
        .and_then(|container| container.get_element_by_signature(Signature::new(b"DNAM")))
    else {
        return 0;
    };
    match variant_int(&dnam.get_native_value()) {
        0 => 1,
        1 => 2,
        3 => 3,
        _ => 0,
    }
}

/// Upstream `wbScriptFragmentsEmptyScriptDecider`: 1 without a script name.
pub fn wb_script_fragments_empty_script_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let empty = container
        .as_container()
        .is_some_and(|container| container.get_element_edit_value("ScriptName").is_empty());
    if empty { 1 } else { 0 }
}

/// Upstream `wbObjectPropertyToStr`: the actor value and its value.
pub fn wb_object_property_to_str(
    a_value: &mut String,
    _a_base_ptr: DataPtr,
    a_element: ElementArg,
    a_type: CallbackType,
) {
    let Some(container) = wb_try_set_container(a_element, a_type) else {
        return;
    };
    let Some(cer) = container.as_container() else { return };
    let actor_value_form = cer.get_element_by_name("Actor Value");
    let Some(main_record) = wb_try_get_main_record(actor_value_form.as_ref(), "") else {
        return;
    };
    let value = cer
        .get_element_by_name("Value")
        .map(|value| value.get_value())
        .unwrap_or_default();
    let number = str_to_float(&value).unwrap_or(0.0);
    *a_value = format!("{} = {}", main_record.get_editor_id(), format_general(number, 5));
    if !matches!(game_mode(), GameMode::gmFO76 | GameMode::gmSF1) {
        return;
    }
    let Some(curve_table) = cer.get_element_by_name("Curve Table") else {
        return;
    };
    let Some(curve_table) = curve_table.as_container() else {
        return;
    };
    let form = curve_table.get_element_by_name("Curve Table");
    let Some(main_record) = wb_try_get_main_record(form.as_ref(), "") else {
        return;
    };
    a_value.push_str(&format!(" {{Curve Table: {}}}", main_record.get_short_name()));
}

/// Upstream anonymous routine at line 8545 of `wbDefinitionsCommon.pas`: the
/// `ADDN` record with the addon node index.
pub fn wb_model_info_anonymous_8545(a_element: ElementArg) -> Option<ElementRef> {
    let element = a_element?;
    let index = element.get_native_value().as_ordinal()?;
    let file = element.get_file()?;
    file.get_record_from_index_by_key(wb_idx_addon_node(), &index.to_string())
        .map(|record| record as ElementRef)
}

/// Upstream anonymous routine at line 8585 of `wbDefinitionsCommon.pas`: the
/// check text of model information in the wrong format.
pub fn wb_model_info_anonymous_8585(
    a_value: &mut String,
    _a_base_ptr: DataPtr,
    _a_element: ElementArg,
    a_type: CallbackType,
) {
    if a_type == CallbackType::ctCheck {
        *a_value = "SubRecord has invalid format for the Form Version of this record".to_owned();
    }
}

/// Upstream `wbEdgeLinksTo0`.
pub fn wb_edge_links_to0(a_element: ElementArg) -> Option<ElementRef> {
    wb_edge_links_to(0, a_element)
}

/// Upstream `wbEdgeToStr0`.
pub fn wb_edge_to_str0(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    wb_edge_to_str(0, a_int, a_element, a_type)
}

/// Upstream `wbEdgeToInt0`.
pub fn wb_edge_to_int0(a_string: &str, a_element: ElementArg) -> i64 {
    wb_edge_to_int(0, a_string, a_element)
}

/// Upstream `wbVertexToStr0`.
pub fn wb_vertex_to_str0(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    wb_vertex_to_str(0, a_int, a_element, a_type)
}

/// Upstream `wbVertexToInt0`.
pub fn wb_vertex_to_int0(a_string: &str, a_element: ElementArg) -> i64 {
    wb_vertex_to_int(0, a_string, a_element)
}

/// Upstream `wbEdgeLinksTo1`.
pub fn wb_edge_links_to1(a_element: ElementArg) -> Option<ElementRef> {
    wb_edge_links_to(1, a_element)
}

/// Upstream `wbEdgeToStr1`.
pub fn wb_edge_to_str1(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    wb_edge_to_str(1, a_int, a_element, a_type)
}

/// Upstream `wbEdgeToInt1`.
pub fn wb_edge_to_int1(a_string: &str, a_element: ElementArg) -> i64 {
    wb_edge_to_int(1, a_string, a_element)
}

/// Upstream `wbVertexToStr1`.
pub fn wb_vertex_to_str1(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    wb_vertex_to_str(1, a_int, a_element, a_type)
}

/// Upstream `wbVertexToInt1`.
pub fn wb_vertex_to_int1(a_string: &str, a_element: ElementArg) -> i64 {
    wb_vertex_to_int(1, a_string, a_element)
}

/// Upstream `wbEdgeLinksTo2`.
pub fn wb_edge_links_to2(a_element: ElementArg) -> Option<ElementRef> {
    wb_edge_links_to(2, a_element)
}

/// Upstream `wbEdgeToStr2`.
pub fn wb_edge_to_str2(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    wb_edge_to_str(2, a_int, a_element, a_type)
}

/// Upstream `wbEdgeToInt2`.
pub fn wb_edge_to_int2(a_string: &str, a_element: ElementArg) -> i64 {
    wb_edge_to_int(2, a_string, a_element)
}

/// Upstream `wbVertexToStr2`.
pub fn wb_vertex_to_str2(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    wb_vertex_to_str(2, a_int, a_element, a_type)
}

/// Upstream `wbVertexToInt2`.
pub fn wb_vertex_to_int2(a_string: &str, a_element: ElementArg) -> i64 {
    wb_vertex_to_int(2, a_string, a_element)
}

/// Upstream `wbAlwaysDontShow`.
pub fn wb_always_dont_show(_a_element: ElementArg) -> bool {
    true
}

/// Upstream `wbREGNImposterDontShow`: the imposters only show for region
/// data of type 8.
pub fn wb_regn_imposter_dont_show(a_element: ElementArg) -> bool {
    wb_get_regn_type(a_element) != 8
}

/// Upstream `wbPxDTLocationDecider`: the location `Type` of the package
/// location.
pub fn wb_px_dt_location_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    container
        .as_container()
        .and_then(|container| container.get_element_by_name("Type"))
        .map_or(0, |kind| variant_int(&kind.get_native_value()) as i32)
}

/// Upstream `wbConditionStringToInt`. Upstream also writes the string into
/// `CIS1` or `CIS2` for parameters 5 and 6; that edit comes with the write
/// path.
pub fn wb_condition_string_to_int(_a_string: &str, _a_element: ElementArg) -> i64 {
    0
}

/// Upstream `wbNVTREdgeToInt`.
pub fn wb_nvtr_edge_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    a_string.trim().parse().unwrap_or(0)
}

/// Upstream `wbNVTREdgeToStr`: an external edge names the triangle and
/// navmesh of its edge link.
pub fn wb_nvtr_edge_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let mut is_external = false;
    let container = a_element.filter(|element| element.as_container().is_some());
    if let Some(container) = container {
        let name = container.get_name();
        // `Copy(Name, 11, 1)`: the digit of `Edge 0-1` and the like.
        let index = name.chars().nth(10).and_then(|c| c.to_digit(10));
        if let Some(index @ 0..=2) = index {
            let flags = variant_int(
                &container
                    .as_container()
                    .unwrap()
                    .get_element_native_value("..\\..\\Flags"),
            ) as u32;
            is_external = flags & (1 << index) != 0;
        }
    }
    let Some(container) = container.filter(|_| is_external) else {
        return match a_type {
            CallbackType::ctToStr | CallbackType::ctToSummary => a_int.to_string(),
            _ => String::new(),
        };
    };
    let container = container.as_container().unwrap();
    let link = format!("..\\..\\..\\..\\NVEX\\Edge Link #{a_int}");
    let exists = container.get_element_exists(&link);
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary => {
            if exists {
                format!(
                    "{a_int} (Triangle #{} in {})",
                    container
                        .get_element_by_path(&format!("{link}\\Triangle"))
                        .map(|element| element.get_value())
                        .unwrap_or_default(),
                    container
                        .get_element_by_path(&format!("{link}\\Navmesh"))
                        .map(|element| element.get_value())
                        .unwrap_or_default()
                )
            } else if a_type == CallbackType::ctToStr {
                format!("{a_int} <Error: NVEX\\Edge Link #{a_int} is missing>")
            } else {
                a_int.to_string()
            }
        }
        // The element sort keys are not ported yet.
        CallbackType::ctToSortKey => String::new(),
        CallbackType::ctCheck if exists => String::new(),
        CallbackType::ctCheck => format!("NVEX\\Edge Link #{a_int} is missing"),
        _ => String::new(),
    }
}

/// Upstream `wbScriptToStr`: the one line of a script, or a summary of the
/// script source.
pub fn wb_script_to_str(a_value: &mut String, _a_base_ptr: DataPtr, a_element: ElementArg, a_type: CallbackType) {
    let Some(container) = wb_try_set_container(a_element, a_type) else {
        return;
    };
    let Some(container) = container.as_container() else {
        return;
    };
    let compiled = if is_morrowind() {
        container.get_element_by_signature(Signature::new(b"SCDT"))
    } else {
        container.get_element_by_signature(Signature::new(b"SCDA"))
    };
    let source = container.get_element_by_signature(Signature::new(b"SCTX"));
    let Some(_) = compiled else {
        *a_value = if source.is_some() {
            "<Source not compiled>"
        } else {
            "<Empty>"
        }
        .to_owned();
        return;
    };
    let Some(source) = source else {
        *a_value = "<Source missing>".to_owned();
        return;
    };
    // `TStringList.Text` splits at CR, LF and CRLF; `Trim` removes the
    // control characters and spaces.
    let delphi_trim = |line: &str| line.trim_matches(|c: char| c <= ' ').to_owned();
    let text = source.get_value();
    let lines: Vec<String> = text
        .split("\r\n")
        .flat_map(|line| line.split(['\r', '\n']))
        .map(delphi_trim)
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .collect();
    // A trailing line break does not start a line.
    *a_value = match lines.len() {
        0 => "<Source missing>".to_owned(),
        1 => lines[0].clone(),
        count => format!("<{count} lines>"),
    };
}

/// Upstream `wbWwiseKeywordMappingSoundDecider`: the `WMTI` of the record.
pub fn wb_wwise_keyword_mapping_sound_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    element
        .get_containing_main_record()
        .map_or(0, |record| variant_int(&record.get_element_native_value("WMTI")) as i32)
}

/// Upstream `wbFlagNavmeshOnlyCutDontSHow`.
pub fn wb_flag_navmesh_only_cut_dont_s_how(a_element: ElementArg) -> bool {
    let flags = containing_record_flags(a_element);
    flags & (0x400_0000 | 0x800_0000 | 0x2000_0000 | 0x4000_0000) != 0
}

/// Upstream `wbFlagNavmeshIgnoreErosionDontSHow`.
pub fn wb_flag_navmesh_ignore_erosion_dont_s_how(a_element: ElementArg) -> bool {
    let flags = containing_record_flags(a_element);
    flags & (0x400_0000 | 0x800_0000 | 0x1000_0000 | 0x4000_0000) != 0
}

/// Upstream `wbConditionSummaryLinksTo`: the first parameter, the second
/// parameter or the comparison value of the condition that links to a
/// record.
pub fn wb_condition_summary_links_to(a_element: ElementArg) -> Option<ElementRef> {
    let container = wb_try_set_container(a_element, CallbackType::ctToSummary)?;
    let ctda = if game_mode() > GameMode::gmFNV {
        let ctda = container
            .as_container()?
            .get_record_by_signature(Signature::new(b"CTDA"))?;
        ctda.as_container()?;
        ctda
    } else {
        container
    };
    let ctda = ctda.as_container()?;
    [5, 6, 2]
        .into_iter()
        .find_map(|index| ctda.get_element(index)?.get_links_to())
}

/// Upstream `wbCrowdPropertyToStr`: the actor value with its value, and the
/// curve table in Fallout 76 and Starfield.
pub fn wb_crowd_property_to_str(
    a_value: &mut String,
    _a_base_ptr: DataPtr,
    a_element: ElementArg,
    a_type: CallbackType,
) {
    let Some(container) = wb_try_set_container(a_element, a_type) else {
        return;
    };
    let Some(container) = container.as_container() else {
        return;
    };
    let actor = container.get_element_by_name("Actor");
    let Some(main_record) = wb_try_get_main_record(actor.as_ref(), "") else {
        return;
    };
    let value = container
        .get_element_by_name("Value")
        .map(|element| element.get_value())
        .unwrap_or_default();
    let Some(value) = str_to_float(&value) else { return };
    *a_value = format!("{} = {}", main_record.get_editor_id(), format_general(value, 5));
    if !matches!(game_mode(), GameMode::gmFO76 | GameMode::gmSF1) {
        return;
    }
    let Some(curve_table) = container.get_element_by_name("Curve Table") else {
        return;
    };
    let curve_table = curve_table
        .as_container()
        .and_then(|curve_table| curve_table.get_element_by_name("Curve Table"));
    let Some(main_record) = wb_try_get_main_record(curve_table.as_ref(), "") else {
        return;
    };
    a_value.push_str(&format!(" {{Curve Table: {}}}", main_record.get_short_name()));
}

/// Upstream `wbLGDIFiltersToStr`: the legendary mod of the star slot the
/// filter selects.
pub fn wb_lgdi_filters_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    const WARNING: &str = "<Warning: Could not resolve mod index>";
    let mut result = match a_type {
        CallbackType::ctToStr => format!("{a_int} {WARNING}"),
        CallbackType::ctToSummary | CallbackType::ctToEditValue => a_int.to_string(),
        CallbackType::ctToSortKey => return int_to_hex64(a_int, 8),
        CallbackType::ctCheck => WARNING.to_owned(),
        CallbackType::ctEditType => return "ComboBox".to_owned(),
        CallbackType::ctEditInfo => String::new(),
        _ => return String::new(),
    };
    let Some(element) = a_element else { return result };
    let Some(filter) = element.get_container().filter(|filter| filter.as_container().is_some()) else {
        return result;
    };
    let Some(main_record) = element.get_containing_main_record() else {
        return result;
    };
    let has_signature = |element: Option<ElementRef>| element.and_then(|element| element.get_has_signature());
    let Some(base) = has_signature(filter.get_container())
        .or_else(|| has_signature(filter.get_container().and_then(|container| container.get_container())))
    else {
        return result;
    };
    let rank_slots_signature = match &base.to_string()[..] {
        "CNAM" | "DNAM" | "LNAM" => Signature::new(b"BNAM"),
        "INAM" | "HNAM" => Signature::new(b"GNAM"),
        _ => return result,
    };
    let Some(rank_slots) = main_record.get_element_by_signature(rank_slots_signature) else {
        return result;
    };
    let Some(rank_slots) = rank_slots.as_container() else {
        return result;
    };
    let Some(star_slot_index) = filter
        .as_container()
        .and_then(|filter| filter.get_element(0))
        .and_then(|element| element.get_native_value().as_ordinal())
    else {
        return result;
    };
    if star_slot_index < 0 || star_slot_index >= i64::from(rank_slots.get_element_count()) {
        return result;
    }
    let Some(star_slot) = rank_slots.get_element(star_slot_index as i32) else {
        return result;
    };
    let Some(star_slot) = star_slot.as_container() else {
        return result;
    };
    let mod_name = |legendary_mod: &ElementRef| {
        legendary_mod
            .as_container()
            .and_then(|legendary_mod| legendary_mod.get_element(1))
            .map(|element| element.get_edit_value())
            .unwrap_or_default()
    };
    if a_type == CallbackType::ctEditInfo {
        let mut infos: Vec<String> = (0..star_slot.get_element_count())
            .filter_map(|index| {
                let legendary_mod = star_slot.get_element(index)?;
                Some(format!("{index:02} {}", mod_name(&legendary_mod)))
            })
            .collect();
        infos.sort_by_key(|info| info.to_lowercase());
        return to_comma_text(&infos);
    }
    let Some(mod_index) = element.get_native_value().as_ordinal() else {
        return result;
    };
    if mod_index < 0 || mod_index >= i64::from(star_slot.get_element_count()) {
        return result;
    }
    let Some(legendary_mod) = star_slot.get_element(mod_index as i32) else {
        return result;
    };
    let name = mod_name(&legendary_mod);
    if name.is_empty() {
        return result;
    }
    match a_type {
        CallbackType::ctCheck => return String::new(),
        CallbackType::ctToSummary => {
            let omod = legendary_mod
                .as_container()
                .and_then(|legendary_mod| legendary_mod.get_element(1))
                .and_then(|element| element.get_links_to())
                .and_then(|record| record.into_main_record());
            return match omod {
                Some(omod) => {
                    let editor_id = omod.get_editor_id();
                    if editor_id.is_empty() {
                        omod.get_short_name()
                    } else {
                        editor_id
                    }
                }
                None => String::new(),
            };
        }
        _ => result = format!("{mod_index:02}"),
    }
    format!("{result} {name}")
}

/// Upstream `wbLGDIRankSlotArrayShouldInclude`: the array of a rank slot
/// continues while the data starts with its position.
pub fn wb_lgdi_rank_slot_array_should_include(a_base_ptr: DataPtr, a_array: ElementArg) -> bool {
    let (Some(array), Some(data)) = (a_array, a_base_ptr) else {
        return false;
    };
    let Some(bytes) = data.get(..4) else { return false };
    i32::from_le_bytes(bytes.try_into().unwrap()) == array.get_memory_order()
}

/// Upstream anonymous routine at line 7802 of `wbDefinitionsCommon.pas`:
/// the set-to-default callback of a legendary slot. Editing is not ported.
pub fn wb_lgdi_slot_def_anonymous_7802(_a_base_ptr: DataPtr, _a_element: ElementArg) -> bool {
    false
}

/// Upstream `wbINFOAliasToStr`: the alias of the quest of the topic of the
/// response.
pub fn wb_info_alias_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let Some(element) = a_element else {
        return String::new();
    };
    if resolve_alias() {
        let Some(main_record) = element.get_containing_main_record() else {
            return String::new();
        };
        let Some(topic) = main_record
            .get_element_by_name("Topic")
            .and_then(|topic| topic.get_links_to())
            .and_then(|topic| topic.into_main_record())
        else {
            return String::new();
        };
        let quest = topic.get_element_by_signature(Signature::new(b"QNAM"));
        wb_alias_to_str(a_int, quest.as_ref(), a_type)
    } else {
        match a_type {
            CallbackType::ctToSortKey => int_to_hex64(a_int, 8),
            CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => a_int.to_string(),
            _ => String::new(),
        }
    }
}

/// The chargen entries of the race of the `NPC_` for its gender, by the
/// path below `Chargen`, with the race when it is one.
fn npc_race_chargen(container: &dyn Container, path: &str) -> (Option<MainRecordRef>, String, Option<ElementRef>) {
    let race = container
        .get_element_links_to("...\\RNAM")
        .and_then(|race| race.into_main_record());
    let gender = if container.get_element_exists("...\\ACBS\\Flags\\Female") {
        "Female"
    } else {
        "Male"
    };
    let entries = race.as_ref().and_then(|race| {
        race.get_element_by_path(&format!("Chargen and Skintones\\{gender}\\Chargen\\{path}"))
            .filter(|entries| entries.as_container().is_some())
    });
    (race, gender.to_owned(), entries)
}

/// The entry of the race chargen list whose `index_signature` is `index`.
fn npc_chargen_entry(entries: &ElementRef, index_signature: &str, index: i64) -> Option<ElementRef> {
    let entries = entries.as_container()?;
    (0..entries.get_element_count()).find_map(|i| {
        let entry = entries.get_element(i)?;
        let value = entry
            .as_container()?
            .get_element_native_value(index_signature)
            .as_ordinal()?;
        (value == index).then_some(entry)
    })
}

/// Upstream `wbNPCFaceDialToStr` and `wbNPCFaceMorphToStr`: the chargen
/// entry of the race with the index, by its label.
fn npc_chargen_to_str(
    a_int: i64,
    a_element: ElementArg,
    a_type: CallbackType,
    path: &str,
    what: &str,
    index_signature: &str,
    label_signature: &str,
) -> String {
    let Some(container) = a_element.and_then(|element| element.as_container()) else {
        return String::new();
    };
    let int = a_int.to_string();
    let could_not = format!("<Warning: Could not resolve {}>", what.to_lowercase());
    let mut result = match a_type {
        CallbackType::ctToStr => format!("{int} {could_not}"),
        CallbackType::ctToSummary | CallbackType::ctToEditValue => int.clone(),
        CallbackType::ctToSortKey => return int_to_hex64(a_int, 8),
        CallbackType::ctCheck => could_not,
        CallbackType::ctEditType => return "ComboBox".to_owned(),
        CallbackType::ctEditInfo => String::new(),
        _ => String::new(),
    };
    let (race, gender, entries) = npc_race_chargen(container, path);
    let Some(race) = race else { return result };
    if race.get_signature().to_string() != "RACE" {
        let warning = format!("<Warning: \"{}\" is not a Race record>", race.get_short_name());
        match a_type {
            CallbackType::ctToStr => result = format!("{int} {warning}"),
            CallbackType::ctCheck => result = warning,
            _ => {}
        }
        return result;
    }
    let Some(entries) = entries else {
        let warning = format!(
            "<Warning: \"{}\" does not contain {gender} Chargen {}>",
            race.get_short_name(),
            if what == "Face Dial" {
                "Face Dials"
            } else {
                "Face Morph Phenotype"
            }
        );
        match a_type {
            CallbackType::ctToStr => result = format!("{int} {warning}"),
            CallbackType::ctCheck => result = warning,
            _ => {}
        }
        return result;
    };
    let entries = entries.as_container().unwrap();
    let mut edit_infos = (a_type == CallbackType::ctEditInfo).then(Vec::new);
    for i in 0..entries.get_element_count() {
        let Some(entry) = entries.get_element(i) else { continue };
        let Some(entry) = entry.as_container() else { continue };
        let Some(index) = entry.get_element_native_value(index_signature).as_ordinal() else {
            continue;
        };
        let index = index as i32;
        if i64::from(index) != a_int && edit_infos.is_none() {
            continue;
        }
        let mut text = format!("{index:03}");
        let label = match a_type {
            CallbackType::ctToSummary => entry
                .get_element_by_path(label_signature)
                .map(|label| label.get_summary())
                .unwrap_or_default(),
            _ => entry
                .get_element_by_path(label_signature)
                .map(|label| label.get_value())
                .unwrap_or_default(),
        };
        if !label.is_empty() {
            text = format!("{text} {label}");
        }
        if let Some(edit_infos) = edit_infos.as_mut() {
            edit_infos.push(text);
        } else {
            match a_type {
                CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToEditValue => result = text,
                CallbackType::ctCheck => result = String::new(),
                _ => {}
            }
            return result;
        }
    }
    let not_found = format!("<Warning: {what} [{int}] not found in \"{}\">", race.get_name());
    match a_type {
        CallbackType::ctToStr => result = format!("{int} {not_found}"),
        CallbackType::ctToSummary => result = int,
        CallbackType::ctCheck => result = not_found,
        CallbackType::ctEditInfo => {
            let mut edit_infos = edit_infos.unwrap_or_default();
            edit_infos.sort_by_key(|info| info.to_lowercase());
            result = to_comma_text(&edit_infos);
        }
        _ => {}
    }
    result
}

/// Upstream `wbNPCFaceDialToStr`.
pub fn wb_npc_face_dial_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    npc_chargen_to_str(a_int, a_element, a_type, "Face Dials", "Face Dial", "FDSI", "FDSL")
}

/// Upstream `wbNPCFaceMorphToStr`.
pub fn wb_npc_face_morph_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    npc_chargen_to_str(
        a_int,
        a_element,
        a_type,
        "Face Morph Phenotypes",
        "Face Morph Phenotype",
        "FMRI",
        "FMRN",
    )
}

/// Upstream `wbNPCFaceDialLinksTo` and `wbNPCFaceMorphLinksTo`.
fn npc_chargen_links_to(a_element: ElementArg, path: &str, index_signature: &str) -> Option<ElementRef> {
    let element = a_element?;
    let container = element.as_container()?;
    let index = element.get_native_value().as_ordinal()?;
    let (_, _, entries) = npc_race_chargen(container, path);
    npc_chargen_entry(&entries?, index_signature, index)
}

/// Upstream `wbNPCFaceDialLinksTo`.
pub fn wb_npc_face_dial_links_to(a_element: ElementArg) -> Option<ElementRef> {
    npc_chargen_links_to(a_element, "Face Dials", "FDSI")
}

/// Upstream `wbNPCFaceMorphLinksTo`.
pub fn wb_npc_face_morph_links_to(a_element: ElementArg) -> Option<ElementRef> {
    npc_chargen_links_to(a_element, "Face Morph Phenotypes", "FMRI")
}

/// Upstream `wbIntPrefixedStrToInt`: the leading integer of the text.
pub fn wb_int_prefixed_str_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    let text = a_string.trim();
    let end = text
        .find(|c: char| c != '-' && !c.is_ascii_digit())
        .unwrap_or(text.len());
    i64::from(str_to_int_def(&text[..end], 0))
}

// ----- the editing callbacks: helpers -----

/// `if wbBeginInternalEdit then try ... finally wbEndInternalEdit end`.
pub fn with_internal_edit(body: impl FnOnce()) {
    if begin_internal_edit(false) {
        body();
        end_internal_edit();
    }
}

/// `wbBeginInternalEdit(True)`.
pub fn with_forced_internal_edit(body: impl FnOnce()) {
    if begin_internal_edit(true) {
        body();
        end_internal_edit();
    }
}

/// An edit a callback made that failed: upstream lets the exception out of
/// the callback; the port reports it on the progress output and goes on.
fn report_edit(result: Result<(), EditError>) {
    if let Err(error) = result {
        progress(&format!("<Error in a definition callback: {error}>"));
    }
}

/// `aElement.NativeValue := aValue` in a callback.
pub fn set_native(element: &ElementRef, value: impl Into<Variant>) {
    report_edit(element.set_native_value(value.into()));
}

/// `aElement.EditValue := aValue` in a callback.
pub fn set_edit(element: &ElementRef, value: &str) {
    report_edit(element.set_edit_value(value));
}

/// `aContainer.ElementNativeValues[aPath] := aValue`.
pub fn set_path_native(container: &ElementRef, path: &str, value: impl Into<Variant>) {
    if let Some(container) = container.as_container() {
        report_edit(container.set_element_native_value(path, value.into()));
    }
}

/// `aContainer.ElementEditValues[aPath] := aValue`.
pub fn set_path_edit(container: &ElementRef, path: &str, value: &str) {
    if let Some(container) = container.as_container() {
        report_edit(container.set_element_edit_value(path, value));
    }
}

/// `aContainer.ElementNativeValues[aPath]`.
pub fn path_native(container: &ElementRef, path: &str) -> Variant {
    container
        .as_container()
        .map_or(Variant::Empty, |container| container.get_element_native_value(path))
}

/// `aContainer.ElementNativeValues[aPath]` as an integer, 0 when missing.
pub fn path_int(container: &ElementRef, path: &str) -> i64 {
    variant_int(&path_native(container, path))
}

/// `aContainer.ElementEditValues[aPath]`.
pub fn path_edit(container: &ElementRef, path: &str) -> String {
    container
        .as_container()
        .map_or_else(String::new, |container| container.get_element_edit_value(path))
}

/// `aContainer.ElementExists[aPath]`.
pub fn path_exists(container: &ElementRef, path: &str) -> bool {
    container
        .as_container()
        .is_some_and(|container| container.get_element_exists(path))
}

/// `aContainer.Add(aName, True)`; a failure is reported.
pub fn add_member(container: &ElementRef, name: &str) -> Option<ElementRef> {
    match container.as_container()?.add(name, true) {
        Ok(element) => element,
        Err(error) => {
            report_edit(Err(error));
            None
        }
    }
}

/// `aContainer.RemoveElement(aName)`.
pub fn remove_member(container: &ElementRef, name: &str) -> Option<ElementRef> {
    container.as_container()?.remove_element_by_name(name)
}

/// `aElement.Container` as a container element.
pub fn container_of(element: &ElementRef) -> Option<ElementRef> {
    element
        .get_container()
        .filter(|container| container.as_container().is_some())
}

/// `Supports(aElement, IwbContainerElementRef, Container)`.
pub fn as_container_ref(element: &ElementRef) -> Option<ElementRef> {
    element.as_container().map(|_| element.clone())
}

/// `aContainer.Elements[aIndex]`.
pub fn element_at(container: &ElementRef, index: i32) -> Option<ElementRef> {
    container.as_container()?.get_element(index)
}

/// `aContainer.ElementCount`.
pub fn element_count(container: &ElementRef) -> i32 {
    container
        .as_container()
        .map_or(0, |container| container.get_element_count())
}

/// A variant as the text a Delphi `string` assignment gives it.
pub fn variant_text(value: &Variant) -> String {
    xedit_core::interface::misc::variant_to_string(value)
}

/// A variant as a floating point number, 0 when it is not one.
pub fn variant_float(value: &Variant) -> f64 {
    value.as_number().unwrap_or(0.0)
}

// ----- the editing callbacks of wbDefinitionsCommon -----

/// Upstream `wbACBSLevelMultAfterLoad`: the level multiplier is kept between
/// 100 and 10000.
pub fn wb_acbs_level_mult_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        if variant_int(&a_element.get_native_value()) > 10000 {
            set_native(a_element, 10000i64);
        }
        if variant_int(&a_element.get_native_value()) < 100 {
            set_native(a_element, 100i64);
        }
    });
}

/// Upstream `wbACBSLevelMultAfterSet`.
pub fn wb_acbs_level_mult_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    with_internal_edit(|| {
        if a_element.get_name() == "Level Mult" {
            let new_value = variant_int(a_new_value);
            if new_value > 10000 {
                set_native(a_element, 10000i64);
            }
            if new_value < 100 {
                set_native(a_element, 100i64);
            }
        }
    });
}

/// Upstream `wbAVIFSkillAfterLoad`: a skill beyond 3 becomes 0.
pub fn wb_avif_skill_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        if variant_int(&a_element.get_native_value()) > 3 {
            set_native(a_element, 0i64);
        }
    });
}

/// Upstream `wbDialogueTextAfterLoad`: the text is trimmed in a file that
/// is not localized, unless it is a single blank.
pub fn wb_dialogue_text_after_load(a_element: &ElementRef) {
    if a_element.get_edit_value() == " " {
        return;
    }
    with_internal_edit(|| {
        let Some(file) = a_element.get_file() else { return };
        if !file.get_is_localized() {
            let trimmed = a_element.get_edit_value().trim().to_owned();
            set_edit(a_element, &trimmed);
        }
    });
}

/// Upstream `wbDialogueTextAfterSet`.
pub fn wb_dialogue_text_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    if a_element.get_edit_value() == " " {
        return;
    }
    with_internal_edit(|| {
        let Some(file) = a_element.get_file() else { return };
        if !file.get_is_localized() {
            set_edit(a_element, variant_text(a_new_value).trim());
        }
    });
}

/// Upstream `wbDOBJObjectsAfterLoad`: the default object entries with an
/// unused slot are dropped.
pub fn wb_dobj_objects_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(array) = as_container_ref(a_element) else {
            return;
        };
        array.begin_update();
        for index in (0..element_count(&array)).rev() {
            if let Some(entry) = element_at(&array, index).and_then(|entry| as_container_ref(&entry))
                && path_int(&entry, "Use") == 0
            {
                array.as_container().and_then(|c| c.remove_element_at(index, true));
            }
        }
        array.end_update();
    });
}

/// Upstream `wbMESGAfterLoad`: a message box has no display time.
pub fn wb_mesg_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(main_record) = a_element.get_containing_main_record() else {
            return;
        };
        let main_record: ElementRef = main_record;
        if path_int(&main_record, "DNAM") & 1 != 0 && path_exists(&main_record, "TNAM") {
            remove_member(&main_record, "TNAM");
        }
    });
}

/// The body of `wbPACKDateAfterLoad` and `wbPACKDateAfterSet`: the day is
/// kept within the month.
fn pack_date_clamp(a_element: &ElementRef) {
    let Some(main_record) = a_element.get_containing_main_record() else {
        return;
    };
    with_internal_edit(|| {
        let month = container_of(a_element)
            .and_then(|container| container.as_container()?.get_element_by_name("Month"))
            .map_or(0, |month| variant_int(&month.get_native_value()));
        let month = if main_record.get_version() < 122 {
            month + 1
        } else {
            month
        };
        let max_date: i64 = match month {
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        if variant_int(&a_element.get_native_value()) > max_date {
            set_native(a_element, max_date);
        }
        if variant_int(&a_element.get_native_value()) < 0 {
            set_native(a_element, 0i64);
        }
    });
}

/// Upstream `wbPACKDateAfterLoad`.
pub fn wb_pack_date_after_load(a_element: &ElementRef) {
    pack_date_clamp(a_element);
}

/// Upstream `wbPACKDateAfterSet`.
pub fn wb_pack_date_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    pack_date_clamp(a_element);
}

/// Upstream `wbPNDTAfterLoad`: a master record keeps `CNAM` and drops
/// `EOVR`, an override the other way round; entries without a worldspace
/// are dropped.
pub fn wb_pndt_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(main_record) = a_element.as_main_record() else {
            return;
        };
        let record: ElementRef = a_element.clone();
        let record_container = record.as_container().expect("a main record is a container");
        let mut cnam = record_container.get_element_by_signature(Signature::new(b"CNAM"));
        let mut eovr = record_container.get_element_by_signature(Signature::new(b"EOVR"));
        let prune = |list: &ElementRef| {
            for index in (0..element_count(list)).rev() {
                if let Some(worldspace) = element_at(list, index)
                    && element_at(&worldspace, 1).is_some_and(|slot| variant_int(&slot.get_native_value()) == 0)
                {
                    worldspace.remove();
                }
            }
        };
        if main_record.get_is_master() {
            if cnam.is_none() {
                cnam = add_member(&record, "CNAM");
            }
            if let Some(cnam) = &cnam {
                prune(cnam);
            }
            if let Some(eovr) = eovr {
                eovr.remove();
            }
        } else {
            if eovr.is_none() {
                eovr = add_member(&record, "EOVR");
            }
            if let Some(eovr) = &eovr {
                prune(eovr);
            }
            if let Some(cnam) = cnam {
                cnam.remove();
            }
        }
    });
}

/// Upstream `wbRecipeCategoryDataAfterLoad`: only the lowest bit stays.
pub fn wb_recipe_category_data_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        if variant_int(&a_element.get_native_value()) & 1 == 0 {
            set_native(a_element, 0i64);
        }
        if variant_int(&a_element.get_native_value()) & 1 == 1 {
            set_native(a_element, 1i64);
        }
    });
}

/// Upstream `wbRPLDAfterLoad`: the points of a region are reversed when
/// the first is beyond the last.
pub fn wb_rpld_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(list) = as_container_ref(a_element) else {
            return;
        };
        let count = element_count(&list);
        let mut needs_flip = false;
        if count > 1 {
            let coordinate = |index: i32, member: i32| -> f64 {
                element_at(&list, index)
                    .and_then(|point| element_at(&point, member))
                    .and_then(|value| str_to_float(&value.get_value()))
                    .unwrap_or(0.0)
            };
            let (a, b) = (coordinate(0, 0), coordinate(count - 1, 0));
            if a == b {
                needs_flip = coordinate(0, 1) > coordinate(count - 1, 1);
            } else if a > b {
                needs_flip = true;
            }
        }
        if needs_flip && let Some(container) = list.as_container() {
            container.reverse_elements();
        }
    });
}

/// Upstream `wbScrollCastAfterLoad`: the cast type of a scroll is always 3.
pub fn wb_scroll_cast_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        if variant_int(&a_element.get_native_value()) != 3 {
            set_native(a_element, 3i64);
        }
    });
}

/// Upstream `wbScrollTypeAfterLoad`: the type of a scroll is always 0.
pub fn wb_scroll_type_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        if variant_int(&a_element.get_native_value()) != 0 {
            set_native(a_element, 0i64);
        }
    });
}

/// Upstream `wbSOUNAfterLoad`: a legacy `SNDD` becomes `SNDX`.
pub fn wb_soun_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        if a_element.as_main_record().is_none() {
            return;
        }
        let record = a_element.clone();
        if !path_exists(&record, "SNDD") {
            return;
        }
        let container = record.as_container().expect("a main record is a container");
        if container.get_element_by_signature(Signature::new(b"SNDX")).is_none() {
            add_member(&record, "SNDX");
        }
        let (Some(sndx), Some(sndd)) = (
            container.get_element_by_signature(Signature::new(b"SNDX")),
            container.get_element_by_signature(Signature::new(b"SNDD")),
        ) else {
            return;
        };
        for index in 0..element_count(&sndd) {
            if let (Some(target), Some(source)) = (element_at(&sndx, index), element_at(&sndd, index)) {
                report_edit(target.assign_from(&source));
            }
        }
        remove_member(&record, "SNDD");
    });
}

/// Upstream `wbWorldAfterLoad`: the members that follow the parent flags,
/// the offset data dropped from a worldspace of the game master, and a
/// warning for abnormally large worldspace bounds.
pub fn wb_world_after_load(a_element: &ElementRef) {
    wb_world_after_set(a_element, &Variant::Int(0), &Variant::Int(1));
    with_internal_edit(|| {
        let Some(main_record) = a_element.as_main_record() else {
            return;
        };
        let record = a_element.clone();
        if remove_offset_data() {
            if (is_skyrim() || is_fallout4() || is_fallout76())
                && main_record.get_file().is_some_and(|file| file.get_load_order() == 0)
            {
                remove_member(&record, "Large References");
            }
            if is_fallout4() || is_fallout76() || is_starfield() {
                remove_member(&record, "CLSZ");
            }
            if is_fallout76() {
                remove_member(&record, "VISI");
            }
        }
        let out_of_range = |value: i64| !(-256..=256).contains(&value);
        if let Some(bounds) = record
            .as_container()
            .and_then(|c| c.get_element_by_name("Worldspace Bounds"))
            .and_then(|bounds| as_container_ref(&bounds))
        {
            let bound = |path: &str| i64::from(str_to_int_def(&path_edit(&bounds, path), 0));
            if out_of_range(bound("NAM0\\X"))
                || out_of_range(bound("NAM0\\Y"))
                || out_of_range(bound("NAM9\\X"))
                || out_of_range(bound("NAM9\\Y"))
            {
                progress(&format!(
                    "<Warning: Worldspace Bounds in {} are abnormally large and can cause performance issues in game>",
                    a_element.get_name()
                ));
            }
        }
    });
}

/// Upstream `wbWorldAfterSet`: the members a worldspace has follow the
/// flags of its parent worldspace.
pub fn wb_world_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    with_internal_edit(|| {
        let Some(record) = as_container_ref(a_element) else {
            return;
        };
        let container = record.as_container().expect("checked above");
        if is_oblivion() {
            if container.get_record_by_signature(Signature::new(b"WNAM")).is_some() {
                remove_member(&record, "CNAM");
                remove_member(&record, "NAM2");
                remove_member(&record, "ICON");
                remove_member(&record, "MNAM");
            } else {
                add_member(&record, "CNAM");
                add_member(&record, "NAM2");
                add_member(&record, "MNAM");
            }
        } else if container.get_element_by_name("Parent Worldspace").is_some() {
            let flags = path_int(&record, "Parent Worldspace\\PNAM");
            if flags & 0x01 == 1 {
                remove_member(&record, "DNAM");
            } else {
                add_member(&record, "DNAM");
            }
            if flags & 0x02 == 2 {
                remove_member(&record, "LOD Data");
            } else {
                add_member(&record, "LOD Data");
            }
            if flags & 0x04 == 4 {
                if is_fallout3() {
                    remove_member(&record, "Icon");
                } else {
                    remove_member(&record, "ICON");
                }
                remove_member(&record, "MNAM");
            } else {
                add_member(&record, "MNAM");
            }
            if flags & 0x08 == 8 {
                remove_member(&record, "NAM2");
            } else {
                add_member(&record, "NAM2");
            }
            if flags & 0x10 == 16 {
                remove_member(&record, "CNAM");
            } else if !is_starfield() {
                add_member(&record, "CNAM");
            }
            if is_fallout3() && flags & 0x20 == 32 {
                remove_member(&record, "INAM");
            } else {
                add_member(&record, "INAM");
            }
        } else {
            add_member(&record, "DNAM");
            add_member(&record, "LOD Data");
            add_member(&record, "MNAM");
            add_member(&record, "NAM2");
            if !is_starfield() {
                add_member(&record, "CNAM");
            }
            if is_fallout3() {
                add_member(&record, "INAM");
            }
        }
    });
}

/// Upstream `wbBOOKDataFlagsAfterSet`: the "teaches skill" and "teaches
/// spell" flags exclude each other.
pub fn wb_book_data_flags_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    with_internal_edit(|| {
        wb_update_same_parent_unions(a_element, a_old_value, a_new_value);
        let native = variant_int(&a_element.get_native_value());
        let (old, new) = (variant_int(a_old_value), variant_int(a_new_value));
        if old & 0x1 == 0 && new & 0x1 != 0 && new & 0x4 != 0 {
            set_native(a_element, native ^ 0x4);
        }
        if old & 0x4 == 0 && new & 0x4 != 0 && new & 0x1 != 0 {
            set_native(a_element, native ^ 0x1);
        }
    });
}

/// Upstream `wbConditionTypeAfterSet`: the comparison value is reset when
/// "use global" changes, and the "run on target" flag of Fallout 3 moves
/// into `Run On`.
pub fn wb_condition_type_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    with_internal_edit(|| {
        let Some(container) = as_container_ref(a_element) else {
            return;
        };
        let (old, new) = (variant_int(a_old_value), variant_int(a_new_value));
        if old & 4 != new & 4 {
            set_path_native(&container, "..\\Comparison Value", 0i64);
        }
        if new & 2 != 0 && is_fallout3() {
            set_path_native(&container, "..\\Run On", 1i64);
            if path_int(&container, "..\\Run On") == 1 {
                set_native(a_element, (new as u8 & !2u8) as i64);
            }
        }
    });
}

/// Upstream `wbConditionRunOnAfterSet`: the reference is cleared unless
/// the condition runs on a reference.
pub fn wb_condition_run_on_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    with_internal_edit(|| {
        if variant_int(a_new_value) != 2
            && let Some(container) = container_of(a_element)
        {
            set_path_native(&container, "Reference", 0i64);
        }
    });
}

/// Upstream `wbIdleMarkerPNAMAfterSet`: `PNAM` and `QNAM` exclude each other.
pub fn wb_idle_marker_pnam_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    with_internal_edit(|| {
        if let Some(record) = a_element.get_containing_main_record()
            && let Some(qnam) = record.get_element_by_signature(Signature::new(b"QNAM"))
        {
            qnam.remove();
        }
    });
}

/// Upstream `wbIdleMarkerQNAMAfterSet`.
pub fn wb_idle_marker_qnam_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    with_internal_edit(|| {
        if let Some(record) = a_element.get_containing_main_record()
            && let Some(pnam) = record.get_element_by_signature(Signature::new(b"PNAM"))
        {
            pnam.remove();
        }
    });
}

/// Upstream `wbMESGDNAMAfterSet`: a message box loses its display time, a
/// notification gets one.
pub fn wb_mesgdnam_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    with_internal_edit(|| {
        let Some(record) = a_element.get_containing_main_record() else {
            return;
        };
        let record: ElementRef = record;
        if variant_int(&a_element.get_native_value()) & 1 != 0 {
            remove_member(&record, "TNAM");
        } else {
            add_member(&record, "TNAM");
        }
    });
}

/// Upstream `wbPERKPRKETypeAfterSet`: the effect data follows the type of
/// the perk effect.
pub fn wb_perkprke_type_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    with_internal_edit(|| {
        let Some(effect) = container_of(a_element).and_then(|prke| container_of(&prke)) else {
            return;
        };
        remove_member(&effect, "DATA");
        add_member(&effect, "DATA");
        remove_member(&effect, "Perk Conditions");
        remove_member(&effect, "Entry Point Function Parameters");
        if variant_int(a_new_value) != 2 {
            return;
        }
        add_member(&effect, "EPFT");
        set_path_native(&effect, "DATA\\Entry Point\\Function", 2i64);
    });
}

/// Upstream `wbPERKPRUCAfterSet`: at most 255.
pub fn wb_perkpruc_after_set(a_element: &ElementRef, _a_old_value: &Variant, _a_new_value: &Variant) {
    with_internal_edit(|| {
        if variant_int(&a_element.get_native_value()) > 255 {
            set_native(a_element, 255i64);
        }
    });
}

/// Upstream `wbSceneActionTypeAfterSet`: the type specific data of a scene
/// action is dropped when it no longer matches the type.
pub fn wb_scene_action_type_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if !(a_old_value.is_ordinal() && a_new_value.is_ordinal()) {
        return;
    }
    if a_old_value.same_value(a_new_value) {
        return;
    }
    let Some(container) = container_of(a_element) else {
        return;
    };
    // Sort order 8: 'Type Specific Action'.
    if let Some(data) = container.as_container().and_then(|c| c.get_element_by_sort_order(8))
        && data.get_name() != a_element.get_value()
    {
        data.remove();
    }
}

/// Upstream `wbScriptFragmentsQuestScriptNameAfterSet`: the script resets
/// when the name appears or disappears.
pub fn wb_script_fragments_quest_script_name_after_set(
    a_element: &ElementRef,
    a_old_value: &Variant,
    a_new_value: &Variant,
) {
    if a_old_value == a_new_value {
        return;
    }
    let (old, new) = (variant_text(a_old_value), variant_text(a_new_value));
    if old.is_empty() != new.is_empty()
        && let Some(script) = container_of(a_element).and_then(|c| c.as_container()?.get_element_by_name("Script"))
    {
        report_edit(script.set_to_default());
    }
}

/// Upstream `wbScriptPropertyTypeAfterSet`: the value resets with the type.
pub fn wb_script_property_type_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value == a_new_value {
        return;
    }
    if let Some(value) = container_of(a_element).and_then(|c| c.as_container()?.get_element_by_name("Value")) {
        report_edit(value.set_to_default());
    }
}

/// Upstream `wbUpdateSameParentUnions`: the unions of the container decide
/// again. The port resolves a union on every read, so there is nothing to
/// refresh beyond touching the elements.
pub fn wb_update_same_parent_unions(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    with_internal_edit(|| {
        if let Some(container) = container_of(a_element) {
            for index in 0..element_count(&container) {
                if let Some(element) = element_at(&container, index) {
                    let _ = element.get_value_def();
                }
            }
        }
    });
}

/// Upstream `wbWwiseKeywordMappingTemplateAfterSet`: the sound mappings
/// are dropped when the template changes.
pub fn wb_wwise_keyword_mapping_template_after_set(
    a_element: &ElementRef,
    a_old_value: &Variant,
    a_new_value: &Variant,
) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    with_internal_edit(|| {
        if let Some(sounds) =
            container_of(a_element).and_then(|c| c.as_container()?.get_element_by_path("Sound Mappings"))
        {
            sounds.remove();
        }
    });
}

/// Upstream `wbStrToLGDIFilter`: the leading digits of the text.
pub fn wb_str_to_lgdi_filter(a_string: &str, _a_element: ElementArg) -> i64 {
    let digits: String = a_string.trim().chars().take_while(char::is_ascii_digit).collect();
    i64::from(str_to_int_def(&digits, 0))
}

// ----- the bodies the game units share -----

/// `wbGMSTEDIDAfterSet` of every game: the value subrecord is rebuilt when
/// the first character of the editor ID, which gives the type, changes.
pub fn gmst_edid_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    let Some(container) = container_of(a_element) else {
        return;
    };
    let (old, new) = (variant_text(a_old_value), variant_text(a_new_value));
    // UPSTREAM-QUIRK: the old value is tested for emptiness twice and the
    // new one not at all; an empty new value with an old one fails there.
    if old.is_empty() || new.is_empty() || old.chars().next() != new.chars().next() {
        remove_member(&container, "DATA");
        add_member(&container, "DATA");
    }
}

/// `wbFLSTEDIDAfterSet` of every game: the FormIDs are dropped when the
/// list changes between ordered and unordered.
pub fn flst_edid_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    let Some(container) = container_of(a_element) else {
        return;
    };
    const ORDERED_LIST: &str = "OrderedList";
    let ends_ordered = |value: String| {
        let tail: String = if value.len() > ORDERED_LIST.len() {
            value.chars().skip(value.chars().count() - ORDERED_LIST.len()).collect()
        } else {
            value
        };
        tail.eq_ignore_ascii_case(ORDERED_LIST)
    };
    if ends_ordered(variant_text(a_old_value)) != ends_ordered(variant_text(a_new_value)) {
        remove_member(&container, "FormIDs");
    }
}

/// `wbFLSTLNAMIsSorted` of every game: never sorted.
// UPSTREAM-QUIRK: the editor ID is tested and the result stays false.
pub fn flst_lnam_is_sorted(a_container: ElementArg) -> bool {
    let _ = a_container
        .and_then(|container| {
            container
                .as_container()?
                .get_record_by_signature(Signature::new(b"EDID"))
        })
        .map(|edid| edid.get_value());
    false
}

/// `wbConditionEventToInt` of every game: `Function:Member` through the
/// event enums of the game.
pub fn condition_event_to_int(
    a_string: &str,
    function_enum: Option<Arc<EnumDef>>,
    member_enum: Option<Arc<EnumDef>>,
) -> i64 {
    let (function, member) = match a_string.split_once(':') {
        Some((function, member)) => {
            let function = function_enum
                .and_then(|def| def.from_edit_value(function, None).ok())
                .unwrap_or(0);
            let member = member_enum
                .and_then(|def| def.from_edit_value(member, None).ok())
                .unwrap_or(0);
            (function, member)
        }
        None => (0, 0),
    };
    (member << 16) + function
}

/// `wbMGEFAssocItemAfterSet` and `wbMGEFAV2WeightAfterSet`: the archetype
/// is marked so that its next change leaves the item alone.
pub fn mgef_assoc_item_after_set(a_element: &ElementRef, a_new_value: &Variant, archetype_name: &str) {
    let Some(container) = wb_try_get_container_from_union(Some(a_element)) else {
        return;
    };
    if variant_float(a_new_value) == 0.0 {
        return;
    }
    if let Some(archetype) = container
        .as_container()
        .and_then(|c| c.get_element_by_name(archetype_name))
        && variant_int(&archetype.get_native_value()) == 0
    {
        set_native(&archetype, 0xFFi64);
    }
}

/// `wbMGEFArchtypeAfterSet` of Skyrim, Fallout 4 and Fallout 76: the actor
/// values follow the archetype.
pub fn mgef_archtype_after_set(
    a_element: &ElementRef,
    a_old_value: &Variant,
    a_new_value: &Variant,
    actor_values: &[(i64, i64)],
) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    let Some(container) = as_container_ref(a_element) else {
        return;
    };
    let (old, new) = (variant_int(a_old_value), variant_int(a_new_value));
    if new < 0xFF && old < 0xFF {
        set_path_native(&container, "..\\Assoc. Item", 0i64);
        let actor_value = actor_values
            .iter()
            .find(|(archetype, _)| *archetype == new)
            .map_or(-1, |(_, value)| *value);
        set_path_native(&container, "..\\Actor Value", actor_value);
        if is_skyrim() || is_fallout4() || is_fallout76() {
            set_path_native(&container, "..\\Second Actor Value", -1i64);
            set_path_native(&container, "..\\Second AV Weight", 0.0f64);
        }
    }
}

/// `wbCELLXCLWGetConflictPriority`: the water height of an interior cell
/// is ignored.
pub fn cell_xclw_get_conflict_priority(a_element: ElementArg, a_cp: &mut ConflictPriority) {
    let Some(element) = a_element else { return };
    let Some(container) = container_of(element) else { return };
    if element_count(&container) < 1 {
        return;
    }
    let Some(main_record) = container.as_main_record() else {
        return;
    };
    if main_record.get_is_deleted() {
        return;
    }
    let Some(data) = container
        .as_container()
        .and_then(|c| c.get_element_by_signature(Signature::new(b"DATA")))
    else {
        return;
    };
    if variant_int(&data.get_native_value()) & 1 == 1 {
        *a_cp = ConflictPriority::cpIgnore;
    }
}

/// `wbCELLDATAAfterSet`: the conflict state of the record resets, which the
/// port does not keep.
pub fn cell_data_after_set(_a_element: &ElementRef) {}

/// `wbEFITAfterLoad` of Skyrim and the Fallout games: the actor value of
/// an effect item follows its magic effect.
pub fn efit_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(container) = as_container_ref(a_element) else {
            return;
        };
        if element_count(&container) < 1 {
            return;
        }
        let Some(main_record) = container.get_containing_main_record() else {
            return;
        };
        if main_record.get_is_deleted() {
            return;
        }
        let efid = container.as_container().and_then(|c| c.get_element_by_path("..\\EFID"));
        let Some(mgef) = wb_try_get_main_record(efid.as_ref(), "MGEF") else {
            return;
        };
        let mgef: ElementRef = mgef;
        let actor_value = path_native(&mgef, "DATA - Data\\Actor Value");
        if matches!(actor_value, Variant::Empty) {
            return;
        }
        if !actor_value.same_value(&path_native(&container, "Actor Value")) {
            set_path_native(&container, "Actor Value", actor_value);
        }
    });
}

/// `wbREFRAfterLoad` of Skyrim, Fallout 4 and Fallout 76: a lock has at
/// least level 1; Skyrim drops `XPTL`.
pub fn refr_after_load_lock(a_element: &ElementRef, remove_xptl: bool) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        if !path_exists(&record, "XLOC") {
            return;
        }
        if path_int(&record, "XLOC - Lock Data\\Level") == 0 {
            set_path_native(&record, "XLOC - Lock Data\\Level", 1i64);
        }
        if remove_xptl {
            remove_member(&record, "XPTL");
        }
    });
}

/// `wbLLEAfterLoad`: the chance none of every entry is zero before form
/// version 69.
pub fn lle_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        if let Some(decider) = wb_form_version_decider_version(69)
            && decider(None, Some(a_element)) == 1
        {
            return;
        }
        let Some(record) = as_container_ref(a_element) else {
            return;
        };
        if element_count(&record) < 1 {
            return;
        }
        let Some(main_record) = a_element.as_main_record() else {
            return;
        };
        if main_record.get_is_deleted() {
            return;
        }
        let Some(entries) = record
            .as_container()
            .and_then(|c| c.get_element_by_name("Leveled List Entries"))
            .and_then(|entries| as_container_ref(&entries))
        else {
            return;
        };
        for index in 0..element_count(&entries) {
            let Some(entry) = element_at(&entries, index).and_then(|entry| as_container_ref(&entry)) else {
                return;
            };
            set_path_native(&entry, "LVLO\\Chance None", 0i64);
        }
    });
}

/// `wbPackageDataInputValueTypeAfterSet`: the value member follows the type.
pub fn package_data_input_value_type_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value == a_new_value {
        return;
    }
    let Some(container) = container_of(a_element) else {
        return;
    };
    let new = variant_text(a_new_value);
    let has_value = matches!(new.as_str(), "Bool" | "Int" | "Float" | "ObjectList");
    match container.as_container().and_then(|c| c.get_element_by_path("CNAM")) {
        Some(value) => {
            if has_value {
                report_edit(value.set_to_default());
            } else {
                value.remove();
            }
        }
        None => {
            if has_value {
                add_member(&container, "CNAM");
            }
        }
    }
}

/// `wbCELLCombinedRefsAfterSet`: the counter is twice the number of
/// entries.
pub fn cell_combined_refs_after_set(a_element: &ElementRef) {
    let Some(container) = container_of(a_element) else {
        return;
    };
    let Some(this) = as_container_ref(a_element) else {
        return;
    };
    if let Some(counter) = container
        .as_container()
        .and_then(|c| c.get_element_by_name("References Count"))
    {
        let expected = i64::from(element_count(&this)) * 2;
        if variant_int(&counter.get_native_value()) != expected {
            // Upstream swallows a failure here.
            let _ = counter.set_native_value(Variant::Int(expected));
        }
    }
}

/// `wbAECHTypeAfterSet`: the value of the audio effect chain resets.
pub fn aech_type_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value == a_new_value {
        return;
    }
    if let Some(value) = container_of(a_element)
        .and_then(|c| c.as_container()?.get_element_by_path("DNAM\\Value"))
        .and_then(|value| as_container_ref(&value))
    {
        report_edit(value.set_to_default());
    }
}

/// `wbReplaceBODTwithBOD2` of Skyrim and the Fallout games.
// UPSTREAM-QUIRK: the routine exits at once ("causes problems with
// Dawnguard.esm"), so nothing is replaced.
pub fn replace_bodt_with_bod2(_a_element: &ElementRef) {}

/// `wbCheckMorphKeyOrder` of Fallout 4 and Fallout 76: the morph keys and
/// values of an override follow the order of its master.
pub fn check_morph_key_order(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(main_record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = main_record.clone();
        let Some(master) = main_record.get_master_or_self().into() else {
            return;
        };
        let master: MainRecordRef = master;
        if master.get_element_id() == main_record.get_element_id() {
            return;
        }
        let master: ElementRef = master;
        let by_signature = |container: &ElementRef, signature: &[u8; 4]| {
            container
                .as_container()?
                .get_element_by_signature(Signature::new(signature))
                .and_then(|element| as_container_ref(&element))
        };
        let (Some(keys), Some(master_keys), Some(values)) = (
            by_signature(&record, b"MSDK"),
            by_signature(&master, b"MSDK"),
            by_signature(&record, b"MSDV"),
        ) else {
            return;
        };
        if element_count(&keys) < element_count(&master_keys) || element_count(&keys) != element_count(&values) {
            return;
        }
        let mut master_order: Vec<(String, i32)> = (0..element_count(&master_keys))
            .filter_map(|index| element_at(&master_keys, index).map(|key| (key.get_sort_key(true), index)))
            .collect();
        master_order.sort();
        let mut needs_sort = false;
        let mut next = element_count(&master_keys);
        for index in 0..element_count(&keys) {
            let (Some(key), Some(value)) = (element_at(&keys, index), element_at(&values, index)) else {
                continue;
            };
            let sort_key = key.get_sort_key(true);
            let order = match master_order.binary_search_by(|(candidate, _)| candidate.cmp(&sort_key)) {
                Ok(found) => {
                    let order = master_order[found].1;
                    if order != index {
                        needs_sort = true;
                    }
                    order
                }
                Err(_) => {
                    next += 1;
                    next - 1
                }
            };
            key.set_sort_order(order);
            value.set_sort_order(order);
        }
        if !needs_sort || next != element_count(&keys) {
            return;
        }
        if let (Some(keys), Some(values)) = (keys.as_container(), values.as_container()) {
            keys.sort_by_sort_order();
            values.sort_by_sort_order();
        }
    });
}

/// `wbCELLAfterLoad` of Fallout 3 and New Vegas: an exterior cell gets a
/// default water height and a water noise texture.
pub fn cell_after_load_fallout3(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        let exterior = path_int(&record, "DATA") & 0x02 != 0;
        if !path_exists(&record, "XCLW") && exterior {
            add_member(&record, "XCLW");
            set_path_edit(&record, "XCLW", "Default");
        }
        if !path_exists(&record, "XNAM") && exterior {
            add_member(&record, "XNAM");
        }
    });
}

/// `wbConditionAfterLoad` of Fallout 3 and New Vegas: the "run on target"
/// flag of a 20 byte condition moves into the `Run On` field.
pub fn condition_after_load_fallout3(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(container) = as_container_ref(a_element) else {
            return;
        };
        if element_count(&container) < 1 {
            return;
        }
        let type_flags = path_int(&container, "Type");
        if type_flags & 2 != 0 {
            if container.get_data_size() == 20 {
                report_edit(container.set_data_size(28));
            }
            set_path_native(&container, "Type", type_flags & !2);
            set_path_edit(&container, "Run On", "Target");
        }
    });
}

/// `wbHeadPartsAfterSet` of Fallout 3 and New Vegas.
pub fn head_parts_after_set(a_element: &ElementRef) {
    with_forced_internal_edit(|| {
        if let Some(container) = as_container_ref(a_element)
            && element_at(&container, 0).is_some_and(|first| variant_int(&first.get_native_value()) == 1)
            && element_count(&container) > 2
        {
            container.as_container().and_then(|c| c.remove_element_at(1, false));
        }
    });
}

/// `wbMGEFAfterLoad` of Fallout 3 and New Vegas: the actor value follows
/// the archetype.
pub fn mgef_after_load_fallout3(a_element: &ElementRef, actor_values: &[(i64, i64)]) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        let old = path_int(&record, "DATA - Data\\Actor Value");
        let archetype = path_int(&record, "DATA - Data\\Archtype");
        let new = actor_values
            .iter()
            .find(|(candidate, _)| *candidate == archetype)
            .map_or(old, |(_, value)| *value);
        if old != new {
            set_path_native(&record, "DATA - Data\\Actor Value", new);
        }
    });
}

/// `wbPACKAfterLoad` of Fallout 3 and New Vegas: the members a package type
/// needs are added.
pub fn pack_after_load_fallout3(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        match path_int(&record, "PKDT - General\\Type") {
            0 => {
                add_member(&record, "PTDT");
            }
            1 => {
                add_member(&record, "PKFD");
            }
            3 => {
                add_member(&record, "PTDT");
                add_member(&record, "PKED");
            }
            4 => {
                if !path_exists(&record, "Locations")
                    && let Some(locations) = add_member(&record, "Locations").and_then(|l| as_container_ref(&l))
                {
                    set_path_edit(&locations, "PLDT - Location 1\\Type", "Near editor location");
                }
            }
            13 => {
                if !path_exists(&record, "Locations")
                    && let Some(locations) = add_member(&record, "Locations").and_then(|l| as_container_ref(&l))
                {
                    set_path_edit(&locations, "PLDT - Location 1\\Type", "Near linked reference");
                }
                add_member(&record, "PKPT");
            }
            _ => {}
        }
    });
}

/// `wbNPCAfterLoad` of Fallout 3 and New Vegas: `NAM5` is at most 255.
pub fn npc_after_load_fallout3(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        if path_int(&record, "NAM5") > 255 {
            set_path_native(&record, "NAM5", 255i64);
        }
    });
}

/// `wbREFRAfterLoad` of Fallout 3 and New Vegas: `RCLR` is dropped, and the
/// ammo of a reference whose base is not a weapon.
pub fn refr_after_load_fallout3(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(main_record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = main_record.clone();
        remove_member(&record, "RCLR");
        if path_exists(&record, "Ammo")
            && let Some(base) = main_record.get_base_record()
            && base.get_signature() != Signature::new(b"WEAP")
        {
            remove_member(&record, "Ammo");
        }
    });
}

/// `wbINFOAfterLoad` of Fallout 3 and New Vegas.
pub fn info_after_load_fallout3(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        if path_int(&record, "DATA\\Flags 1") & 0x80 == 0 {
            remove_member(&record, "DNAM");
        }
        remove_member(&record, "SNDD");
        if path_int(&record, "DATA\\Type") == 3 {
            set_path_native(&record, "DATA\\Type", 0i64);
        }
    });
}

/// `wbEmbeddedScriptAfterLoad` of Fallout 3 and New Vegas: a quest script
/// type becomes an object script.
pub fn embedded_script_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(container) = as_container_ref(a_element) else {
            return;
        };
        if element_count(&container) < 1 {
            return;
        }
        if path_edit(&container, "SCHR\\Type") == "Quest" {
            set_path_edit(&container, "SCHR\\Type", "Object");
        }
    });
}

/// `wbEFSHAfterLoad` of Fallout 3 and New Vegas: particle birth ratios at
/// most 1 are scaled by 78.
pub fn efsh_after_load_fallout3(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        if !path_exists(&record, "DATA") {
            return;
        }
        for path in [
            "DATA\\Particle Shader - Full Particle Birth Ratio",
            "DATA\\Particle Shader - Persistant Particle Birth Ratio",
        ] {
            let ratio = variant_float(&path_native(&record, path));
            if ratio != 0.0 && ratio <= 1.0 {
                set_path_native(&record, path, ratio * 78.0);
            }
        }
    });
}

/// `wbFACTAfterLoad` of Fallout 3 and New Vegas: `CNAM` is dropped.
pub fn fact_after_load_fallout3(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = as_container_ref(a_element) else {
            return;
        };
        if element_count(&record) < 1 || !path_exists(&record, "CNAM") {
            return;
        }
        let Some(main_record) = a_element.as_main_record() else {
            return;
        };
        if main_record.get_is_deleted() {
            return;
        }
        remove_member(&record, "CNAM");
    });
}

/// `wbWEAPAfterLoad` of Fallout 3 and New Vegas: zero animation
/// multipliers become 1; New Vegas resets a reload animation of 255.
pub fn weap_after_load_fallout3(a_element: &ElementRef, reset_reload_animation: bool) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        if !path_exists(&record, "DNAM") {
            return;
        }
        if reset_reload_animation && path_int(&record, "DNAM\\Reload Animation") == 255 {
            set_path_native(&record, "DNAM\\Reload Animation", 0i64);
        }
        for path in ["DNAM\\Animation Multiplier", "DNAM\\Animation Attack Multiplier"] {
            if variant_float(&path_native(&record, path)) == 0.0 {
                set_path_native(&record, path, 1.0f64);
            }
        }
    });
}

/// `wbPerkDATAFunctionAfterSet` of Fallout 3 and New Vegas, with the
/// parameter type of each function.
pub fn perk_data_function_after_set(
    a_element: &ElementRef,
    a_old_value: &Variant,
    a_new_value: &Variant,
    param_types: &[i64],
) {
    let new_function = variant_int(a_new_value);
    let Ok(index) = usize::try_from(new_function) else {
        return;
    };
    let Some(&new_param_type) = param_types.get(index) else {
        return;
    };
    let Some(container) = as_container_ref(a_element) else {
        return;
    };
    const PATH: &str = "..\\..\\..\\Entry Point Function Parameters\\EPFT";
    let old_param_type = path_int(&container, PATH);
    if old_param_type == new_param_type && !a_old_value.same_value(a_new_value) && matches!(new_function, 4 | 5) {
        set_path_native(&container, PATH, 0i64);
    }
    set_path_native(&container, PATH, new_param_type);
}

/// `wbPerkEPFTAfterSet` of Fallout 3 and New Vegas: the parameter members
/// follow the parameter type (0 none, 1 float, 2 two floats, 3 leveled
/// item, 4 script).
pub fn perk_epft_after_set(a_element: &ElementRef, a_old_value: &Variant, a_new_value: &Variant) {
    if a_old_value.same_value(a_new_value) {
        return;
    }
    let param_type = variant_int(a_new_value);
    if !(0..=4).contains(&param_type) {
        return;
    }
    let Some(container) = container_of(a_element) else {
        return;
    };
    remove_member(&container, "EPFD");
    remove_member(&container, "EPF2");
    remove_member(&container, "EPF3");
    remove_member(&container, "Embedded Script");
    match param_type {
        1..=3 => {
            add_member(&container, "EPFD");
        }
        4 => {
            add_member(&container, "EPF2");
            add_member(&container, "EPF3");
            add_member(&container, "SCHR");
        }
        _ => {}
    }
}

/// `wbPERKEntryPointAfterSet` of Fallout 3 and New Vegas: the function,
/// the condition tab count and the perk conditions follow the entry point.
/// `entry_points` gives the condition index and function type of each entry
/// point, `conditions` the count and captions of each condition type, and
/// `functions` the function type of each function.
pub fn perk_entry_point_after_set(
    a_element: &ElementRef,
    a_old_value: &Variant,
    a_new_value: &Variant,
    entry_points: &[(usize, i64)],
    conditions: &[(i64, &str, &str)],
    functions: &[i64],
) {
    if a_old_value == a_new_value {
        return;
    }
    let (Ok(old_index), Ok(new_index)) = (
        usize::try_from(variant_int(a_old_value)),
        usize::try_from(variant_int(a_new_value)),
    ) else {
        return;
    };
    let (Some(old_entry), Some(new_entry)) = (entry_points.get(old_index), entry_points.get(new_index)) else {
        return;
    };
    let (Some(old_condition), Some(new_condition)) = (conditions.get(old_entry.0), conditions.get(new_entry.0)) else {
        return;
    };
    let Some(entry_point) = container_of(a_element) else {
        return;
    };
    let function = usize::try_from(path_int(&entry_point, "Function")).ok();
    let old_function_type = function.and_then(|index| functions.get(index)).copied();
    if old_function_type != Some(new_entry.1)
        && let Some(index) = functions.iter().position(|&function_type| function_type == new_entry.1)
    {
        set_path_native(&entry_point, "Function", index as i64);
    }
    set_path_native(&entry_point, "Perk Condition Tab Count", new_condition.0);
    let Some(effect) = container_of(&entry_point).and_then(|data| container_of(&data)) else {
        return;
    };
    let Some(perk_conditions) = effect
        .as_container()
        .and_then(|c| c.get_element_by_name("Perk Conditions"))
        .and_then(|conditions| as_container_ref(&conditions))
    else {
        return;
    };
    for index in (0..element_count(&perk_conditions)).rev() {
        let Some(condition) = element_at(&perk_conditions, index).and_then(|c| as_container_ref(&c)) else {
            continue;
        };
        let tab = path_int(&condition, "PRKC");
        if tab >= new_condition.0 {
            condition.remove();
        } else {
            match tab {
                2 if old_condition.1 != new_condition.1 => condition.remove(),
                3 if old_condition.2 != new_condition.2 => condition.remove(),
                _ => {}
            }
        }
    }
}
