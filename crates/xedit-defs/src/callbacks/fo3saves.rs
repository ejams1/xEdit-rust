// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsFO3Saves.pas

//! The callbacks of `wbDefinitionsFO3Saves.pas` that are ported by hand.
//! The ones that are not ported yet are stubs in `fo3saves_stubs.rs`.

#[allow(unused_imports)]
pub use super::fo3saves_stubs::*;

use xedit_core::interface::globals::{
    set_extract_info, set_file_chapters, set_file_header, set_file_magic, set_file_plugins,
};
use xedit_core::interface::*;

use super::fnvsaves;
use super::save_interface::wb_find_save_element;
use crate::fo3::define_fo3;
use crate::fo3saves::{
    WB_ACTOR_VALUE_LABELS, WB_CHANGE_TYPES, WB_CO_SAVE_CHAPTERS, WB_CO_SAVE_HEADER, define_fo3_saves_s,
};

/// Upstream `ExtractInfoSave`: the chapters of a save that are initialized
/// while it loads.
const EXTRACT_INFO_SAVE: [u8; 2] = [3, 4];

/// Port of `DefineFO3SavesA`: the labels of the actor value arrays.
pub fn define_fo3_saves_a() {
    let labels = actor_value_enum().map_or_else(Vec::new, |enum_def| {
        (0..enum_def.get_name_count())
            .map(|index| enum_def.get_name_of(i64::from(index)))
            .collect()
    });
    WB_ACTOR_VALUE_LABELS.set(labels);
}

/// Port of `DefineFO3Saves`. The oracle reads Fallout 3 saves after it
/// warns that they are not supported yet.
pub fn define_fo3_saves() {
    set_file_magic("FO3SAVEGAME");
    set_extract_info(&EXTRACT_INFO_SAVE);
    set_file_plugins("Plugins");
    define_fo3();
    define_fo3_saves_a();
    define_fo3_saves_s();
}

/// Port of `SwitchToFO3CoSave`.
pub fn switch_to_fo3_co_save() {
    set_file_magic("FOSE");
    set_extract_info(&[]);
    set_file_plugins("Absolute:44");
    set_file_chapters(WB_CO_SAVE_CHAPTERS.get());
    set_file_header(WB_CO_SAVE_HEADER.get());
}

/// Upstream `ChangedFormDataDecider`: the 42 form types of Fallout 3.
pub fn changed_form_data_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match fnvsaves::changed_form_data_decider(a_base_ptr, a_element) {
        result if result > 42 => 0,
        result => result,
    }
}

/// Upstream `ChangedFormFlagsDecider`: the 42 form types of Fallout 3.
pub fn changed_form_flags_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_form_data_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormGetChapterTypeName`, with the change types of
/// Fallout 3.
pub fn changed_form_get_chapter_type_name(a_base_ptr: DataPtr, a_element: ElementArg) -> String {
    const CHANGED_FORM_OFFSET: i32 = 10000;
    let kind = fnvsaves::changed_form_get_chapter_type(a_base_ptr, a_element);
    let mut result = String::new();
    if let Some(change_types) = WB_CHANGE_TYPES.get()
        && kind >= CHANGED_FORM_OFFSET
        && kind < CHANGED_FORM_OFFSET + change_types.get_name_count()
    {
        result = change_types.get_name_of(i64::from(kind - CHANGED_FORM_OFFSET));
    }
    if result.is_empty() { kind.to_string() } else { result }
}

/// Upstream `FOSEChaptersDecider`: the plugin list chunk, or another one.
pub fn fose_chapters_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    let chunk = wb_find_save_element("Chunk", element);
    let Some(kind) = chunk
        .as_data_container()
        .and_then(|chunk| chunk.get_element_by_name("Type"))
    else {
        return 0;
    };
    if kind.get_value() == "MODS" { 1 } else { 2 }
}

// ----- generated -----
/// Upstream `ChangeFormBaseProcessCreatedPackageDecider`, the same as for Fallout New Vegas.
pub fn change_form_base_process_created_package_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_base_process_created_package_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormBaseProcessPackageDecider`, the same as for Fallout New Vegas.
pub fn change_form_base_process_package_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_base_process_package_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCombatControllerHasUnk0A0Decider`, the same as for Fallout New Vegas.
pub fn change_form_combat_controller_has_unk0_a0_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_combat_controller_has_unk0_a0_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCombatProcedureTypeDecider`, the same as for Fallout New Vegas.
pub fn change_form_combat_procedure_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_combat_procedure_type_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageDecider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageHasContentFlagBit0Decider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_has_content_flag_bit0_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_has_content_flag_bit0_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageHasContentFlagBit1Decider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_has_content_flag_bit1_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_has_content_flag_bit1_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageHasContentFlagBit2Decider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_has_content_flag_bit2_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_has_content_flag_bit2_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageHasContentFlagBit3Decider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_has_content_flag_bit3_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_has_content_flag_bit3_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageHasContentFlagBit4Decider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_has_content_flag_bit4_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_has_content_flag_bit4_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageHasContentFlagBit5Decider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_has_content_flag_bit5_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_has_content_flag_bit5_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageHasPresentFlagBit0Decider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_has_present_flag_bit0_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_has_present_flag_bit0_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageHasPresentFlagBit1Decider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_has_present_flag_bit1_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_has_present_flag_bit1_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageHasPresentFlagBit2Decider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_has_present_flag_bit2_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_has_present_flag_bit2_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageHasPresentFlagBit3Decider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_has_present_flag_bit3_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_has_present_flag_bit3_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormCreatedPackageHasPresentFlagBit4Decider`, the same as for Fallout New Vegas.
pub fn change_form_created_package_has_present_flag_bit4_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_created_package_has_present_flag_bit4_decider(a_base_ptr, a_element)
}

/// Upstream `ChangeFormExtraPackageTypeDecider`, the same as for Fallout New Vegas.
pub fn change_form_extra_package_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::change_form_extra_package_type_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedExtraUnionDecider`, the same as for Fallout New Vegas.
pub fn changed_extra_union_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_extra_union_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag00Decider`, the same as for Fallout New Vegas.
pub fn changed_flag00_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag00_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag01Decider`, the same as for Fallout New Vegas.
pub fn changed_flag01_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag01_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag02Decider`, the same as for Fallout New Vegas.
pub fn changed_flag02_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag02_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag03Decider`, the same as for Fallout New Vegas.
pub fn changed_flag03_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag03_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag04Decider`, the same as for Fallout New Vegas.
pub fn changed_flag04_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag04_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag05Decider`, the same as for Fallout New Vegas.
pub fn changed_flag05_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag05_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag05or27Decider`, the same as for Fallout New Vegas.
pub fn changed_flag05or27_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag05or27_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag09Decider`, the same as for Fallout New Vegas.
pub fn changed_flag09_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag09_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag10Decider`, the same as for Fallout New Vegas.
pub fn changed_flag10_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag10_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag11Decider`, the same as for Fallout New Vegas.
pub fn changed_flag11_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag11_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag11OrCreatedDecider`, the same as for Fallout New Vegas.
pub fn changed_flag11_or_created_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag11_or_created_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag19Decider`, the same as for Fallout New Vegas.
pub fn changed_flag19_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag19_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag20Decider`, the same as for Fallout New Vegas.
pub fn changed_flag20_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag20_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag21Decider`, the same as for Fallout New Vegas.
pub fn changed_flag21_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag21_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag22Decider`, the same as for Fallout New Vegas.
pub fn changed_flag22_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag22_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag23Decider`, the same as for Fallout New Vegas.
pub fn changed_flag23_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag23_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag24Decider`, the same as for Fallout New Vegas.
pub fn changed_flag24_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag24_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag25Decider`, the same as for Fallout New Vegas.
pub fn changed_flag25_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag25_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag28Decider`, the same as for Fallout New Vegas.
pub fn changed_flag28_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag28_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag28NotActorDecider`, the same as for Fallout New Vegas.
pub fn changed_flag28_not_actor_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag28_not_actor_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag29Decider`, the same as for Fallout New Vegas.
pub fn changed_flag29_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag29_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag30Decider`, the same as for Fallout New Vegas.
pub fn changed_flag30_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag30_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag31Decider`, the same as for Fallout New Vegas.
pub fn changed_flag31_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_flag31_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormActorBaseDecider`, the same as for Fallout New Vegas.
pub fn changed_form_actor_base_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_actor_base_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormAnimationSubBufferCounter`, the same as for Fallout New Vegas.
pub fn changed_form_animation_sub_buffer_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fnvsaves::changed_form_animation_sub_buffer_counter(a_base_ptr, a_element)
}

/// Upstream `ChangedFormCellIsInteriorDecider`, the same as for Fallout New Vegas.
pub fn changed_form_cell_is_interior_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_cell_is_interior_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormDataLengthDecider`, the same as for Fallout New Vegas.
pub fn changed_form_data_length_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_data_length_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormDataSizer`, the same as for Fallout New Vegas.
pub fn changed_form_data_sizer(a_base_ptr: DataPtr, a_element: ElementArg, compressed_size: &mut i32) -> u32 {
    fnvsaves::changed_form_data_sizer(a_base_ptr, a_element, compressed_size)
}

/// Upstream `ChangedFormEventListHasStruct010Decider`, the same as for Fallout New Vegas.
pub fn changed_form_event_list_has_struct010_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_event_list_has_struct010_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormExtraDecider`, the same as for Fallout New Vegas.
pub fn changed_form_extra_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_extra_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormExtraScriptEventDecider`, the same as for Fallout New Vegas.
pub fn changed_form_extra_script_event_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_extra_script_event_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormGetChapterName`, the same as for Fallout New Vegas.
pub fn changed_form_get_chapter_name(a_base_ptr: DataPtr, a_element: ElementArg) -> String {
    fnvsaves::changed_form_get_chapter_name(a_base_ptr, a_element)
}

/// Upstream `ChangedFormGetChapterType`, the same as for Fallout New Vegas.
pub fn changed_form_get_chapter_type(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_get_chapter_type(a_base_ptr, a_element)
}

/// Upstream `ChangedFormHasUnk3DCDecider`, the same as for Fallout New Vegas.
pub fn changed_form_has_unk3_dc_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_has_unk3_dc_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormHavokMovedSubBufferCounter`, the same as for Fallout New Vegas.
pub fn changed_form_havok_moved_sub_buffer_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fnvsaves::changed_form_havok_moved_sub_buffer_counter(a_base_ptr, a_element)
}

/// Upstream `ChangedFormHighProcessSubBufferCounter`, the same as for Fallout New Vegas.
pub fn changed_form_high_process_sub_buffer_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fnvsaves::changed_form_high_process_sub_buffer_counter(a_base_ptr, a_element)
}

/// Upstream `ChangedFormList44CData00CDecider`, the same as for Fallout New Vegas.
pub fn changed_form_list44_c_data00_c_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_list44_c_data00_c_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormMobileObjectBaseProcessDecider`, the same as for Fallout New Vegas.
pub fn changed_form_mobile_object_base_process_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_mobile_object_base_process_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormMobileObjectHighProcessDecider`, the same as for Fallout New Vegas.
pub fn changed_form_mobile_object_high_process_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_mobile_object_high_process_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormMobileObjectLowProcessDecider`, the same as for Fallout New Vegas.
pub fn changed_form_mobile_object_low_process_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_mobile_object_low_process_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormMobileObjectMiddleHighProcessDecider`, the same as for Fallout New Vegas.
pub fn changed_form_mobile_object_middle_high_process_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_mobile_object_middle_high_process_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormMobileObjectMiddleLowProcessDecider`, the same as for Fallout New Vegas.
pub fn changed_form_mobile_object_middle_low_process_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_mobile_object_middle_low_process_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageActorMoverContentFlagsBit0Decider`, the same as for Fallout New Vegas.
pub fn changed_form_package_actor_mover_content_flags_bit0_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_actor_mover_content_flags_bit0_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageActorMoverContentFlagsBit1Decider`, the same as for Fallout New Vegas.
pub fn changed_form_package_actor_mover_content_flags_bit1_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_actor_mover_content_flags_bit1_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageActorMoverContentFlagsBit2Decider`, the same as for Fallout New Vegas.
pub fn changed_form_package_actor_mover_content_flags_bit2_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_actor_mover_content_flags_bit2_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageActorMoverContentFlagsBit3Decider`, the same as for Fallout New Vegas.
pub fn changed_form_package_actor_mover_content_flags_bit3_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_actor_mover_content_flags_bit3_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageCreatedAUEHasLocationDecider`, the same as for Fallout New Vegas.
pub fn changed_form_package_created_aue_has_location_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_created_aue_has_location_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageCreatedContentFlagsBit0Decider`, the same as for Fallout New Vegas.
pub fn changed_form_package_created_content_flags_bit0_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_created_content_flags_bit0_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageCreatedContentFlagsBit1Decider`, the same as for Fallout New Vegas.
pub fn changed_form_package_created_content_flags_bit1_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_created_content_flags_bit1_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageCreatedContentFlagsBit2Decider`, the same as for Fallout New Vegas.
pub fn changed_form_package_created_content_flags_bit2_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_created_content_flags_bit2_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageCreatedDHasLocationDecider`, the same as for Fallout New Vegas.
pub fn changed_form_package_created_d_has_location_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_created_d_has_location_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageCreatedFEHasLocationDecider`, the same as for Fallout New Vegas.
pub fn changed_form_package_created_fe_has_location_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_created_fe_has_location_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageCreatedPackageDataTypeDecider`, the same as for Fallout New Vegas.
pub fn changed_form_package_created_package_data_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_created_package_data_type_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageCreatedUWHasLocationDecider`, the same as for Fallout New Vegas.
pub fn changed_form_package_created_uw_has_location_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_created_uw_has_location_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageCreatedUWHasTargetDecider`, the same as for Fallout New Vegas.
pub fn changed_form_package_created_uw_has_target_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_created_uw_has_target_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageLocationTypeDecider`, the same as for Fallout New Vegas.
pub fn changed_form_package_location_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_location_type_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPackageTargetTypeDecider`, the same as for Fallout New Vegas.
pub fn changed_form_package_target_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_package_target_type_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPathingRequestSubStructHasUnk00CDecider`, the same as for Fallout New Vegas.
pub fn changed_form_pathing_request_sub_struct_has_unk00_c_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_pathing_request_sub_struct_has_unk00_c_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormPathingRequestTypeDecider`, the same as for Fallout New Vegas.
pub fn changed_form_pathing_request_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_pathing_request_type_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormProjectileHasEntryDataDecider`, the same as for Fallout New Vegas.
pub fn changed_form_projectile_has_entry_data_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_projectile_has_entry_data_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormQuestStageHasLogDataDecider`, the same as for Fallout New Vegas.
pub fn changed_form_quest_stage_has_log_data_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::changed_form_quest_stage_has_log_data_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormRemainingDataCounter`, the same as for Fallout New Vegas.
pub fn changed_form_remaining_data_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fnvsaves::changed_form_remaining_data_counter(a_base_ptr, a_element)
}

/// Upstream `ChangedFormSizer`, the same as for Fallout New Vegas.
pub fn changed_form_sizer(a_base_ptr: DataPtr, a_element: ElementArg, compressed_size: &mut i32) -> u32 {
    fnvsaves::changed_form_sizer(a_base_ptr, a_element, compressed_size)
}

/// Upstream `ChangedFormsCounter`, the same as for Fallout New Vegas.
pub fn changed_forms_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fnvsaves::changed_forms_counter(a_base_ptr, a_element)
}

/// Upstream `DataCounter`, the same as for Fallout New Vegas.
pub fn data_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fnvsaves::data_counter(a_base_ptr, a_element)
}

/// Upstream `DataQuartetCounter`, the same as for Fallout 4.
pub fn data_quartet_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    super::fo4saves::data_quartet_counter(a_base_ptr, a_element)
}

/// Upstream `DataQuartetRemainderCounter`, the same as for Fallout 4.
pub fn data_quartet_remainder_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    super::fo4saves::data_quartet_remainder_counter(a_base_ptr, a_element)
}

/// Upstream `GlobalData1Counter`, the same as for Fallout New Vegas.
pub fn global_data1_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fnvsaves::global_data1_counter(a_base_ptr, a_element)
}

/// Upstream `GlobalData2Counter`, the same as for Fallout New Vegas.
pub fn global_data2_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fnvsaves::global_data2_counter(a_base_ptr, a_element)
}

/// Upstream `GlobalData2TableSizeCounter`, the same as for Fallout New Vegas.
pub fn global_data2_table_size_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fnvsaves::global_data2_table_size_counter(a_base_ptr, a_element)
}

/// Upstream `GlobalDataDecider`, the same as for Fallout New Vegas.
pub fn global_data_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::global_data_decider(a_base_ptr, a_element)
}

/// Upstream `GlobalDataGetChapterType`, the same as for Fallout New Vegas.
pub fn global_data_get_chapter_type(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::global_data_get_chapter_type(a_base_ptr, a_element)
}

/// Upstream `GlobalDataGetChapterTypeName`, the same as for Fallout New Vegas.
pub fn global_data_get_chapter_type_name(a_base_ptr: DataPtr, a_element: ElementArg) -> String {
    fnvsaves::global_data_get_chapter_type_name(a_base_ptr, a_element)
}

/// Upstream `GlobalDataSizer`, the same as for Fallout New Vegas.
pub fn global_data_sizer(a_base_ptr: DataPtr, a_element: ElementArg, compressed_size: &mut i32) -> u32 {
    fnvsaves::global_data_sizer(a_base_ptr, a_element, compressed_size)
}

/// Upstream `HasDialogueItemsDecider`, the same as for Fallout New Vegas.
pub fn has_dialogue_items_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::has_dialogue_items_decider(a_base_ptr, a_element)
}

/// Upstream `InitialDataTypeDecider`, the same as for Fallout New Vegas.
pub fn initial_data_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::initial_data_type_decider(a_base_ptr, a_element)
}

/// Upstream `IsActorPlayerDecider`, the same as for Fallout New Vegas.
pub fn is_actor_player_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::is_actor_player_decider(a_base_ptr, a_element)
}

/// Upstream `Lst0044Has00D4DataDecider`, the same as for Fallout New Vegas.
pub fn lst0044_has00_d4_data_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::lst0044_has00_d4_data_decider(a_base_ptr, a_element)
}

/// Upstream `Lst0044HasArr0018DataDecider`, the same as for Fallout New Vegas.
pub fn lst0044_has_arr0018_data_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fnvsaves::lst0044_has_arr0018_data_decider(a_base_ptr, a_element)
}

/// Upstream `RefIDTableAfterLoad`, the same as for Fallout New Vegas.
pub fn ref_id_table_after_load(a_element: &ElementRef) {
    fnvsaves::ref_id_table_after_load(a_element)
}

/// Upstream `ScreenShotDataCounter`, the same as for Fallout New Vegas.
pub fn screen_shot_data_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fnvsaves::screen_shot_data_counter(a_base_ptr, a_element)
}

/// Upstream `WorldspaceTableAfterLoad`, the same as for Fallout New Vegas.
pub fn worldspace_table_after_load(a_element: &ElementRef) {
    fnvsaves::worldspace_table_after_load(a_element)
}
