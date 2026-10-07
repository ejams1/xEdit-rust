// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsTES3.pas

//! The callbacks of `wbDefinitionsTES3.pas` that are ported by hand. The
//! ones that are not ported yet are stubs in `tes3_stubs.rs`.

// The stubs of the callbacks not ported yet; empty once every callback is ported.
#[allow(unused_imports)]
pub use super::tes3_stubs::*;

use xedit_core::interface::main_record::GridCell;
use xedit_core::interface::*;

use super::common::{
    variant_int, wb_try_get_container_from_union, wb_try_get_container_with_valid_main_record, wb_try_set_container,
};

/// The native value at `path` of the container of the element, as an
/// integer (`Integer(aElement.Container.ElementNativeValues[aPath])`).
fn container_value(a_element: ElementArg, path: &str) -> i64 {
    a_element
        .and_then(|element| element.get_container())
        .and_then(|container| {
            container
                .as_container()
                .map(|container| variant_int(&container.get_element_native_value(path)))
        })
        .unwrap_or(0)
}

fn is_alchemy(a_element: ElementArg) -> bool {
    a_element
        .and_then(|element| element.get_containing_main_record())
        .is_some_and(|record| record.get_signature() == Signature::new(b"ALCH"))
}

/// Upstream `wbEffectAreaDontShow`.
pub fn wb_effect_area_dont_show(a_element: ElementArg) -> bool {
    matches!(container_value(a_element, "Range"), 1 | 2) || is_alchemy(a_element)
}

/// Upstream `wbEffectAttributeDontShow`.
pub fn wb_effect_attribute_dont_show(a_element: ElementArg) -> bool {
    !matches!(container_value(a_element, "Magic Effect"), 17 | 22 | 74 | 79 | 85)
}

/// Upstream `wbEffectDurationDontShow`.
pub fn wb_effect_duration_dont_show(a_element: ElementArg) -> bool {
    matches!(
        container_value(a_element, "Magic Effect"),
        12 | 13 | 57 | 60 | 61 | 62 | 63 | 69 | 70 | 71 | 72 | 73 | 133
    )
}

/// Upstream `wbEffectSkillDontShow`.
pub fn wb_effect_skill_dont_show(a_element: ElementArg) -> bool {
    !matches!(container_value(a_element, "Magic Effect"), 21 | 26 | 78 | 83 | 89)
}

/// Upstream `wbEffectRangeDontShow`.
pub fn wb_effect_range_dont_show(a_element: ElementArg) -> bool {
    matches!(
        container_value(a_element, "Magic Effect"),
        59..=66 | 102..=116 | 120..=125 | 127..=135 | 137..=142
    ) || is_alchemy(a_element)
}

/// Upstream `wbEffectMagnitudeDontShow`.
pub fn wb_effect_magnitude_dont_show(a_element: ElementArg) -> bool {
    matches!(
        container_value(a_element, "Magic Effect"),
        0 | 2 | 39 | 45 | 46 | 58 | 60..=63 | 69..=73 | 102..=116 | 120..=134 | 136..=142
    )
}

/// The native value at `path` of the container of the union.
fn union_value(a_element: ElementArg, path: &str) -> Option<i64> {
    let container = wb_try_get_container_from_union(a_element)?;
    Some(variant_int(&container.as_container()?.get_element_native_value(path)))
}

/// Upstream `wbConditionFunctionDecider`.
pub fn wb_condition_function_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match union_value(a_element, "Type") {
        Some(50 | 51 | 67) => 1,
        Some(52) => 2,
        Some(53) => 3,
        Some(54) => 4,
        Some(55) => 5,
        Some(56) => 6,
        Some(57) => 7,
        Some(65) => 8,
        Some(66) => 9,
        _ => 0,
    }
}

/// Upstream `wbEffectRangeDecider`.
pub fn wb_effect_range_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match union_value(a_element, "Magic Effect") {
        Some(12 | 13 | 44 | 49..=56 | 58 | 85..=89 | 101 | 118 | 119 | 126) => 1,
        _ => 0,
    }
}

/// Upstream `wbNPCDataDecider`: 1 for the short form of 12 bytes.
pub fn wb_npc_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    i32::from(a_element.is_some_and(|element| element.get_data_size() == 12))
}

/// Upstream `wbSkillDecider`: the member for the skill of the `INDX` of the
/// record.
pub fn wb_skill_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(container) = wb_try_get_container_from_union(a_element) else {
        return 0;
    };
    let Some(container) = container.get_container() else {
        return 0;
    };
    let Some(indx) = container
        .as_container()
        .and_then(|container| container.get_element_by_signature(Signature::new(b"INDX")))
    else {
        return 0;
    };
    match variant_int(&indx.get_native_value()) {
        1 => 1,
        2 | 3 | 17 | 21 => 2,
        4 | 5 | 6 | 7 | 22 | 23 | 26 => 3,
        8 => 4,
        9 => 5,
        10..=15 => 6,
        16 => 7,
        18 => 8,
        19 => 9,
        20 => 10,
        24 => 11,
        25 => 12,
        _ => 0,
    }
}

/// Upstream `wbCalcPGRCSize`: the connection count of the point of the path
/// grid that the element belongs to.
pub fn wb_calc_pgrc_size(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    let Some(container) = element.get_container() else {
        return 0;
    };
    let count = container
        .as_container()
        .map_or(0, |container| container.get_element_count());
    // `ExtractCountFromLabel`: one past the number after `#` in the name.
    let name = element.get_name();
    let index = match name.find('#') {
        None => count,
        Some(at) => name[at + 1..].trim().parse::<i32>().map_or(count, |number| number + 1),
    };
    let Some(main_record) = container.get_container().and_then(|record| record.into_main_record()) else {
        return 0;
    };
    main_record
        .get_record_by_signature(Signature::new(b"PGRP"))
        .and_then(|pgrp| pgrp.as_container()?.get_element(index - 1))
        .and_then(|point| point.as_container()?.get_element(2))
        .map_or(0, |connections| variant_int(&connections.get_native_value()) as u32)
}

/// Upstream `wbFactionReactionToStr`: the signed reaction and the faction.
pub fn wb_faction_reaction_to_str(
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
    let faction = container
        .get_element(0)
        .map(|faction| faction.get_value())
        .unwrap_or_default();
    let reaction = container
        .get_element(1)
        .map_or(0, |reaction| variant_int(&reaction.get_native_value()));
    *a_value = format!("{reaction} {faction}");
    if reaction >= 0 {
        a_value.insert(0, '+');
    }
}

/// Upstream `wbFRMRToString`: the reference number in hexadecimal.
pub fn wb_frmr_to_string(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSummary | CallbackType::ctToSortKey => {
            format!("{:08X}", a_int as u32)
        }
        CallbackType::ctToEditValue => format!("${:08X}", a_int as u32),
        _ => String::new(),
    }
}

/// Upstream `wbGridCellToFormID`: the FormID of an exterior cell from its
/// grid position, in the range of `a_form_id_base`.
fn wb_grid_cell_to_form_id(a_form_id_base: u8, grid_cell: GridCell) -> Option<FormID> {
    let GridCell { x, y } = grid_cell;
    if !(-512..=511).contains(&x) || !(-512..=511).contains(&y) {
        return None;
    }
    Some(FormID::from_cardinal(
        (((x + 512) as u32) << 10) + (y + 512) as u32 + (u32::from(a_form_id_base) << 16),
    ))
}

/// Upstream `TwbGridCell.SortKey`.
fn grid_cell_sort_key(grid_cell: GridCell) -> String {
    format!(
        "{:08X}|{:08X}",
        i64::from(grid_cell.x) + 0x8000_0000,
        i64::from(grid_cell.y) + 0x8000_0000
    )
}

/// The grid cell of a main record as `GetGridCell` gives it.
fn grid_cell(a_main_record: &MainRecordRef) -> Option<GridCell> {
    a_main_record.get_grid_cell().map(|(x, y)| GridCell { x, y })
}

/// Upstream anonymous routine at line 916 of `wbDefinitionsTES3.pas`: the
/// file header has the null FormID.
pub fn define_tes3_anonymous_916(_a_main_record: &MainRecordRef) -> Option<FormID> {
    Some(FormID::from_cardinal(0))
}

/// Upstream anonymous routine at line 1126 of `wbDefinitionsTES3.pas`: the
/// grid cell of an exterior `CELL`.
pub fn define_tes3_anonymous_1126(a_sub_record: &ElementRef) -> Option<GridCell> {
    let container = a_sub_record.as_container()?;
    if variant_int(&container.get_element_native_value("Flags\\Is Interior Cell")) != 0 {
        return None;
    }
    Some(GridCell {
        x: variant_int(&container.get_element_native_value("Grid\\X")) as i32,
        y: variant_int(&container.get_element_native_value("Grid\\Y")) as i32,
    })
}

/// Upstream anonymous routine at line 1135 of `wbDefinitionsTES3.pas`.
pub fn define_tes3_anonymous_1135(a_main_record: &MainRecordRef) -> Option<FormID> {
    wb_grid_cell_to_form_id(0xA0, grid_cell(a_main_record)?)
}

/// Upstream anonymous routine at line 1139 of `wbDefinitionsTES3.pas`.
pub fn define_tes3_anonymous_1139(a_main_record: &MainRecordRef) -> String {
    match grid_cell(a_main_record) {
        Some(cell) => format!("<Exterior>{}", grid_cell_sort_key(cell)),
        None => a_main_record.get_editor_id(),
    }
}

/// Upstream anonymous routine at line 1702 of `wbDefinitionsTES3.pas`.
pub fn define_tes3_anonymous_1702(a_main_record: &MainRecordRef) -> Option<FormID> {
    wb_grid_cell_to_form_id(0xC0, grid_cell(a_main_record)?)
}

/// Upstream anonymous routine at line 1706 of `wbDefinitionsTES3.pas`.
pub fn define_tes3_anonymous_1706(a_main_record: &MainRecordRef) -> String {
    grid_cell(a_main_record).map(grid_cell_sort_key).unwrap_or_default()
}

/// Upstream anonymous routine at line 2029 of `wbDefinitionsTES3.pas`: the
/// grid cell of a path grid, none at (0, 0).
pub fn define_tes3_anonymous_2029(a_sub_record: &ElementRef) -> Option<GridCell> {
    let container = a_sub_record.as_container()?;
    let x = variant_int(&container.get_element_native_value("Grid\\X")) as i32;
    let y = variant_int(&container.get_element_native_value("Grid\\Y")) as i32;
    (x != 0 || y != 0).then_some(GridCell { x, y })
}

/// Upstream anonymous routine at line 2036 of `wbDefinitionsTES3.pas`.
pub fn define_tes3_anonymous_2036(a_main_record: &MainRecordRef) -> Option<FormID> {
    wb_grid_cell_to_form_id(0xE0, grid_cell(a_main_record)?)
}

/// Upstream anonymous routine at line 2040 of `wbDefinitionsTES3.pas`.
pub fn define_tes3_anonymous_2040(a_main_record: &MainRecordRef) -> String {
    define_tes3_anonymous_1139(a_main_record)
}

/// Upstream anonymous routine at line 2170 of `wbDefinitionsTES3.pas`: the
/// FormID of a reference is its `FRMR`, in the file's own slot when the
/// reference number has none.
pub fn define_tes3_anonymous_2170(a_main_record: &MainRecordRef) -> Option<FormID> {
    let frmr = a_main_record.get_record_by_signature(Signature::new(b"FRMR"))?;
    let form_id = FormID::from_cardinal(variant_int(&frmr.get_native_value()) as u32);
    Some(if form_id.file_id().full_slot() == 0 {
        form_id.change_file_id(FileID::create_full(0xFF))
    } else {
        form_id
    })
}

/// Upstream anonymous routine at line 2251 of `wbDefinitionsTES3.pas`.
pub fn define_tes3_anonymous_2251(a_sub_record: &ElementRef) -> String {
    a_sub_record
        .as_container()
        .map(|container| container.get_element_edit_value("Name"))
        .unwrap_or_default()
}

// ----- the editing callbacks -----

use super::common::{
    as_container_ref, path_edit, path_int, path_native, remove_member, set_native, set_path_edit, set_path_native,
    with_internal_edit,
};

/// Upstream anonymous `SetEditorIDCallback` of a script: the editor ID is
/// the `Name` of the `SCHD` subrecord.
pub fn define_tes3_anonymous_2254(a_sub_record: &ElementRef, a_editor_id: &str) {
    set_path_edit(a_sub_record, "Name", a_editor_id);
}

/// Upstream `wbCELLAfterLoad`: an interior cell's `INTV` becomes `WHGT`.
pub fn wb_cell_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        if path_int(&record, "DATA\\Flags") & 1 != 0
            && record
                .as_container()
                .and_then(|c| c.get_element_by_signature(Signature::new(b"WHGT")))
                .is_none()
        {
            set_path_native(&record, "WHGT", path_native(&record, "INTV"));
            remove_member(&record, "INTV");
        }
    });
}

/// Upstream `wbDeletedAfterLoad`: `DELE` is zero.
pub fn wb_deleted_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        if record
            .as_container()
            .and_then(|c| c.get_element_by_signature(Signature::new(b"DELE")))
            .is_some()
        {
            set_path_native(&record, "DELE", 0i64);
        }
    });
}

/// Upstream `wbEffectRangeAfterLoad`: a range of 0 becomes 1.
pub fn wb_effect_range_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        if a_element.as_container().is_none() {
            return;
        }
        if path_edit(a_element, "Range") == "0" {
            set_path_native(a_element, "Range", 1i64);
        }
    });
}

/// Upstream `wbEffectRangeAfterSet`: the range resets.
pub fn wb_effect_range_after_set(a_element: &ElementRef, _a_old_value: &Variant, _a_new_value: &Variant) {
    if let Some(range) = a_element
        .get_container()
        .and_then(|container| container.as_container()?.get_element_by_name("Range"))
    {
        let _ = range.set_to_default();
    }
}

/// Upstream `wbForwardForReal`: the text up to the first zero byte goes
/// into the `Target` (or `Sound`) member.
pub fn wb_forward_for_real(a_element: &ElementRef) {
    with_internal_edit(|| {
        let value = a_element.get_value();
        if value.is_empty() {
            return;
        }
        let Some(container) = a_element.get_container() else {
            return;
        };
        let Some(container) = container.as_container() else {
            return;
        };
        let Some(target) = container
            .get_element_by_name("Target")
            .or_else(|| container.get_element_by_name("Sound"))
        else {
            return;
        };
        // UPSTREAM-QUIRK: `Copy(s, 0, i)` keeps the zero byte that ends the text.
        let end = value.find('\0').map_or(value.len(), |position| position + 1);
        set_native(&target, value[..end].to_owned());
    });
}

/// Upstream `wbGlobalAfterLoad`: the broken float of a short global is zero.
pub fn wb_global_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        let by_signature = |signature: &[u8; 4]| {
            record
                .as_container()
                .and_then(|c| c.get_element_by_signature(Signature::new(signature)))
        };
        let (Some(fltv), Some(fnam)) = (by_signature(b"FLTV"), by_signature(b"FNAM")) else {
            return;
        };
        if fnam.get_value() != "Short" {
            return;
        }
        let native = fltv.get_native_value();
        let number = native.as_number();
        if number == Some((-92233720368547758.1f32) as f64) || number == Some(0.04) || fltv.get_value() == "NaN" {
            set_path_native(&record, "FLTV", 0i64);
        }
    });
}

/// Upstream `wbIngredientAfterLoad`: the skill and attribute of each effect
/// follow the magic effect.
pub fn wb_ingredient_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = record;
        for index in 0..4 {
            let effect = path_int(&record, &format!("IRDT\\Effects\\Magic Effects\\Magic Effect #{index}"));
            let skill = format!("IRDT\\Effects\\Skills\\Skill #{index}");
            let attribute = format!("IRDT\\Effects\\Attributes\\Attribute #{index}");
            match effect {
                17 | 22 | 74 | 79 => set_path_native(&record, &skill, -1i64),
                21 | 26 | 78 | 83 => set_path_native(&record, &attribute, -1i64),
                _ => {
                    set_path_native(&record, &skill, -1i64);
                    set_path_native(&record, &attribute, -1i64);
                }
            }
        }
    });
}

/// Upstream `wbTES3AfterLoad`: the ESM flag of the header record follows
/// the flags in `HEDR`.
pub fn wb_tes3_after_load(a_element: &ElementRef) {
    with_internal_edit(|| {
        let Some(main_record) = wb_try_get_container_with_valid_main_record(Some(a_element)) else {
            return;
        };
        let record: ElementRef = main_record.clone();
        if record
            .as_container()
            .and_then(|c| c.get_element_by_signature(Signature::new(b"HEDR")))
            .is_some()
            && path_int(&record, "HEDR\\Record Flags") & 1 == 1
            && let Some(record_impl) = record.as_element_impl().and_then(|e| e.main_record_impl())
        {
            record_impl.set_is_esm(true);
        }
        let _ = as_container_ref(&record);
    });
}
