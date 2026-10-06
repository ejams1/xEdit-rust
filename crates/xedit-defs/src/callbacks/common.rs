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

use xedit_core::delphi::{float_to_str_f_fixed, format_general, round, str_to_float};
use xedit_core::interface::builders::wb_flags_unknown_is_unused;
use xedit_core::interface::constructors::get_container_from_union;
use xedit_core::interface::globals::{
    GameMode, cs, game_mode, is_fallout_nv, is_fallout3, is_fallout76, is_morrowind, is_oblivion, is_skyrim,
    is_starfield, resolve_alias,
};
use xedit_core::interface::misc::{int_to_hex64, str_to_int_def};
use xedit_core::interface::string::to_comma_text;
use xedit_core::interface::*;

use crate::common::{wb_idx_addon_node, wb_idx_collision_layer, wb_package_schedule_month_enum};

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

/// Upstream `wbPositionToGridCell` with `wbCellSizeFactor` of 4096.
fn position_to_grid_cell(x: f64, y: f64) -> (i32, i32) {
    let cell = |value: f64| {
        let scaled = value / 4096.0;
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
    let coordinate = |index| match position.get_element(index)?.get_native_value() {
        Variant::Float(value) => Some(value),
        Variant::Int(value) => Some(value as f64),
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

/// The text of a file or folder hash that the containers do not resolve.
fn hash_callback(a_int: i64, a_type: CallbackType) -> String {
    // UPSTREAM-QUIRK: upstream resolves the hash through the container
    // handler once the loader is done; the hash tables are not ported.
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
    hash_callback(a_int, a_type)
}

/// Upstream `wbFolderHashCallback`.
pub fn wb_folder_hash_callback(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    hash_callback(a_int, a_type)
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
        if alias.get_record_signature() == Some(Signature::new(b"ALCS"))
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
    let flags_value = variant_int(&flags.get_native_value());
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
