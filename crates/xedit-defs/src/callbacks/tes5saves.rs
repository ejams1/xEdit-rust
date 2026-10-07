// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsTES5Saves.pas

//! The callbacks of `wbDefinitionsTES5Saves.pas` that are ported by hand.
//! The ones that are not ported yet are stubs in `tes5saves_stubs.rs`.

#[allow(unused_imports)]
pub use super::tes5saves_stubs::*;

use std::sync::atomic::Ordering;

use xedit_core::interface::globals::{
    set_extract_info, set_file_chapters, set_file_header, set_file_magic, set_file_plugins,
};
use xedit_core::interface::*;

use super::fo4saves::{
    self, LAST_REGISTRATION_START, VM_ARRAY_TABLE_COUNT, VM_OBJECT_ARRAY_COUNT, VM_OBJECT_DETACHED_ARRAY_COUNT,
    data_element_count, native, root_value, save_child, save_value, save_value_decider, save_version_decider,
};
use super::save_interface::{query_count_for_vm_array_handle, wb_find_save_element};
use crate::tes5::define_tes5;
use crate::tes5saves::{
    WB_CHANGE_TYPES, WB_CO_SAVE_CHAPTERS, WB_CO_SAVE_HEADER, define_tes5_saves_a, define_tes5_saves_s,
};

pub use super::fo4saves::ARRAY_CONTENT_ENTRY_DATA;

/// Upstream `ExtractInfoSave`: the chapters of a save that are initialized
/// while it loads.
const EXTRACT_INFO_SAVE: [u8; 2] = [4, 5];

/// Port of `DefineTES5Saves`.
pub fn define_tes5_saves() {
    set_file_magic("TESV_SAVEGAME");
    set_extract_info(&EXTRACT_INFO_SAVE);
    set_file_plugins("Plugins");
    define_tes5();
    define_tes5_saves_a();
    define_tes5_saves_s();
}

/// Port of `SwitchToTES5CoSave`.
pub fn switch_to_tes5_co_save() {
    set_file_magic("SKSE");
    set_extract_info(&[]);
    set_file_plugins("Absolute:44");
    set_file_chapters(WB_CO_SAVE_CHAPTERS.get());
    set_file_header(WB_CO_SAVE_HEADER.get());
}

/// Upstream `ArrayContentTableCounter`, the same as for Fallout 4.
pub fn array_content_table_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::array_content_table_counter(a_base_ptr, a_element)
}

/// Upstream `ArrayTableAfterLoad`, the same as for Fallout 4.
pub fn array_table_after_load(a_element: &ElementRef) {
    fo4saves::array_table_after_load(a_element)
}

/// Upstream `ChangedExtraUnionDecider`, the same as for Fallout 4.
pub fn changed_extra_union_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_extra_union_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag00Decider`, the same as for Fallout 4.
pub fn changed_flag00_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag00_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag01Decider`, the same as for Fallout 4.
pub fn changed_flag01_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag01_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag02Decider`, the same as for Fallout 4.
pub fn changed_flag02_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag02_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag03Decider`, the same as for Fallout 4.
pub fn changed_flag03_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag03_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag04Decider`, the same as for Fallout 4.
pub fn changed_flag04_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag04_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag05Decider`, the same as for Fallout 4.
pub fn changed_flag05_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag05_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag05or27Decider`, the same as for Fallout 4.
pub fn changed_flag05or27_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag05or27_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag06Decider`, the same as for Fallout 4.
pub fn changed_flag06_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag06_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag07Decider`, the same as for Fallout 4.
pub fn changed_flag07_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag07_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag09Decider`, the same as for Fallout 4.
pub fn changed_flag09_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag09_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag10Decider`, the same as for Fallout 4.
pub fn changed_flag10_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag10_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag11Decider`, the same as for Fallout 4.
pub fn changed_flag11_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag11_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag12Decider`, the same as for Fallout 4.
pub fn changed_flag12_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag12_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag13Decider`, the same as for Fallout 4.
pub fn changed_flag13_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag13_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag19Decider`, the same as for Fallout 4.
pub fn changed_flag19_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag19_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag20Decider`, the same as for Fallout 4.
pub fn changed_flag20_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag20_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag21Decider`, the same as for Fallout 4.
pub fn changed_flag21_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag21_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag22Decider`, the same as for Fallout 4.
pub fn changed_flag22_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag22_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag23Decider`, the same as for Fallout 4.
pub fn changed_flag23_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag23_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag24Decider`, the same as for Fallout 4.
pub fn changed_flag24_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag24_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag25Decider`, the same as for Fallout 4.
pub fn changed_flag25_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag25_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag26Decider`, the same as for Fallout 4.
pub fn changed_flag26_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag26_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag27Decider`, the same as for Fallout 4.
pub fn changed_flag27_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag27_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag28Decider`, the same as for Fallout 4.
pub fn changed_flag28_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag28_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag29Decider`, the same as for Fallout 4.
pub fn changed_flag29_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag29_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag30Decider`, the same as for Fallout 4.
pub fn changed_flag30_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag30_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFlag31Decider`, the same as for Fallout 4.
pub fn changed_flag31_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_flag31_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormActorHasEquipDataDecider`, the same as for Fallout 4.
pub fn changed_form_actor_has_equip_data_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_form_actor_has_equip_data_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormAnimationSubBufferCounter`, the same as for Fallout 4.
pub fn changed_form_animation_sub_buffer_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::changed_form_animation_sub_buffer_counter(a_base_ptr, a_element)
}

/// Upstream `ChangedFormCellIsInteriorDecider`, the same as for Fallout 4.
pub fn changed_form_cell_is_interior_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_form_cell_is_interior_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormDataDecider`, the same as for Fallout 4.
pub fn changed_form_data_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_form_data_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormDataLengthDecider`, the same as for Fallout 4.
pub fn changed_form_data_length_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_form_data_length_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormDataSizer`, the same as for Fallout 4.
pub fn changed_form_data_sizer(a_base_ptr: DataPtr, a_element: ElementArg, compressed_size: &mut i32) -> u32 {
    fo4saves::changed_form_data_sizer(a_base_ptr, a_element, compressed_size)
}

/// Upstream `ChangedFormExtraDecider`, the same as for Fallout 4.
pub fn changed_form_extra_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_form_extra_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormExtraUnknown12HasUnk010Decider`, the same as for Fallout 4.
pub fn changed_form_extra_unknown12_has_unk010_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_form_extra_unknown12_has_unk010_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormFlagsDecider`, the same as for Fallout 4.
pub fn changed_form_flags_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_form_flags_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormGetChapterName`, the same as for Fallout 4.
pub fn changed_form_get_chapter_name(a_base_ptr: DataPtr, a_element: ElementArg) -> String {
    fo4saves::changed_form_get_chapter_name(a_base_ptr, a_element)
}

/// Upstream `ChangedFormGetChapterType`, the same as for Fallout 4.
pub fn changed_form_get_chapter_type(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_form_get_chapter_type(a_base_ptr, a_element)
}

/// Upstream `ChangedFormHavokMovedSubBufferCounter`, the same as for Fallout 4.
pub fn changed_form_havok_moved_sub_buffer_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::changed_form_havok_moved_sub_buffer_counter(a_base_ptr, a_element)
}

/// Upstream `ChangedFormNPCFaceHasFaceMorphDecider`, the same as for Fallout 4.
pub fn changed_form_npc_face_has_face_morph_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_form_npc_face_has_face_morph_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormNPCFaceHasHeadDataDecider`, the same as for Fallout 4.
pub fn changed_form_npc_face_has_head_data_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_form_npc_face_has_head_data_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormProjectileHasInventoryDecider`, the same as for Fallout 4.
pub fn changed_form_projectile_has_inventory_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::changed_form_projectile_has_inventory_decider(a_base_ptr, a_element)
}

/// Upstream `ChangedFormRemainingDataCounter`, the same as for Fallout 4.
pub fn changed_form_remaining_data_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::changed_form_remaining_data_counter(a_base_ptr, a_element)
}

/// Upstream `ChangedFormSizer`, the same as for Fallout 4.
pub fn changed_form_sizer(a_base_ptr: DataPtr, a_element: ElementArg, compressed_size: &mut i32) -> u32 {
    fo4saves::changed_form_sizer(a_base_ptr, a_element, compressed_size)
}

/// Upstream `ChangedFormsCounter`, the same as for Fallout 4.
pub fn changed_forms_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::changed_forms_counter(a_base_ptr, a_element)
}

/// Upstream `CodeParameterTypeValueDecider`, the same as for Fallout 4.
pub fn code_parameter_type_value_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::code_parameter_type_value_decider(a_base_ptr, a_element)
}

/// Upstream `DataCounter`, the same as for Fallout 4.
pub fn data_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::data_counter(a_base_ptr, a_element)
}

/// Upstream `DataQuartetCounter`, the same as for Fallout 4.
pub fn data_quartet_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::data_quartet_counter(a_base_ptr, a_element)
}

/// Upstream `DataQuartetRemainderCounter`, the same as for Fallout 4.
pub fn data_quartet_remainder_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::data_quartet_remainder_counter(a_base_ptr, a_element)
}

/// Upstream `DumpCounter`, the same as for Fallout 4.
pub fn dump_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::dump_counter(a_base_ptr, a_element)
}

/// Upstream `FirstCountCounter`, the same as for Fallout 4.
pub fn first_count_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::first_count_counter(a_base_ptr, a_element)
}

/// Upstream `FrameExtraVariablesCounter`, the same as for Fallout 4.
pub fn frame_extra_variables_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::frame_extra_variables_counter(a_base_ptr, a_element)
}

/// Upstream `FunctorDecider`, the same as for Fallout 4.
pub fn functor_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::functor_decider(a_base_ptr, a_element)
}

/// Upstream `GlobalData1Counter`, the same as for Fallout 4.
pub fn global_data1_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::global_data1_counter(a_base_ptr, a_element)
}

/// Upstream `GlobalData2Counter`, the same as for Fallout 4.
pub fn global_data2_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::global_data2_counter(a_base_ptr, a_element)
}

/// Upstream `GlobalDataGetChapterType`, the same as for Fallout 4.
pub fn global_data_get_chapter_type(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::global_data_get_chapter_type(a_base_ptr, a_element)
}

/// Upstream `GlobalDataGetChapterTypeName`, the same as for Fallout 4.
pub fn global_data_get_chapter_type_name(a_base_ptr: DataPtr, a_element: ElementArg) -> String {
    fo4saves::global_data_get_chapter_type_name(a_base_ptr, a_element)
}

/// Upstream `GlobalDataSizer`, the same as for Fallout 4.
pub fn global_data_sizer(a_base_ptr: DataPtr, a_element: ElementArg, compressed_size: &mut i32) -> u32 {
    fo4saves::global_data_sizer(a_base_ptr, a_element, compressed_size)
}

/// Upstream `InitialDataTypeDecider`, the same as for Fallout 4.
pub fn initial_data_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::initial_data_type_decider(a_base_ptr, a_element)
}

/// Upstream `IsActorPlayerDecider`, the same as for Fallout 4.
pub fn is_actor_player_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::is_actor_player_decider(a_base_ptr, a_element)
}

/// Upstream `ObjectDataTableEntryExtraDecider`, the same as for Fallout 4.
pub fn object_data_table_entry_extra_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::object_data_table_entry_extra_decider(a_base_ptr, a_element)
}

/// Upstream `ObjectDetachedTableAfterLoad`, the same as for Fallout 4.
pub fn object_detached_table_after_load(a_element: &ElementRef) {
    fo4saves::object_detached_table_after_load(a_element)
}

/// Upstream `ObjectTableAfterLoad`, the same as for Fallout 4.
pub fn object_table_after_load(a_element: &ElementRef) {
    fo4saves::object_table_after_load(a_element)
}

/// Upstream `OpcodeVariableParameterDecider`, the same as for Fallout 4.
pub fn opcode_variable_parameter_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::opcode_variable_parameter_decider(a_base_ptr, a_element)
}

/// Upstream `QuestRuntimeAliasTypeDecider`, the same as for Fallout 4.
pub fn quest_runtime_alias_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::quest_runtime_alias_type_decider(a_base_ptr, a_element)
}

/// Upstream `QuestRuntimeHasEventDecider`, the same as for Fallout 4.
pub fn quest_runtime_has_event_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::quest_runtime_has_event_decider(a_base_ptr, a_element)
}

/// Upstream `QuestRuntimeParamTypeDecider`, the same as for Fallout 4.
pub fn quest_runtime_param_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::quest_runtime_param_type_decider(a_base_ptr, a_element)
}

/// Upstream `RefIDTableAfterLoad`, the same as for Fallout 4.
pub fn ref_id_table_after_load(a_element: &ElementRef) {
    fo4saves::ref_id_table_after_load(a_element)
}

/// Upstream `SecondCountCounter`, the same as for Fallout 4.
pub fn second_count_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::second_count_counter(a_base_ptr, a_element)
}

/// Upstream `SkipCounter`, the same as for Fallout 4.
pub fn skip_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::skip_counter(a_base_ptr, a_element)
}

/// Upstream `StackCallBackTypeDecider`, the same as for Fallout 4.
pub fn stack_call_back_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::stack_call_back_type_decider(a_base_ptr, a_element)
}

/// Upstream `StackContentTableCounter`, the same as for Fallout 4.
pub fn stack_content_table_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::stack_content_table_counter(a_base_ptr, a_element)
}

/// Upstream `StackTableAfterLoad`, the same as for Fallout 4.
pub fn stack_table_after_load(a_element: &ElementRef) {
    fo4saves::stack_table_after_load(a_element)
}

/// Upstream `StackTableDataEntryStackExtraDecider`, the same as for Fallout 4.
pub fn stack_table_data_entry_stack_extra_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::stack_table_data_entry_stack_extra_decider(a_base_ptr, a_element)
}

/// Upstream `Unknown1000_00001Decider`, the same as for Fallout 4.
pub fn unknown1000_00001_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::unknown1000_00001_decider(a_base_ptr, a_element)
}

/// Upstream `VersionGreaterThan1Decider`, the same as for Fallout 4.
pub fn version_greater_than1_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::version_greater_than1_decider(a_base_ptr, a_element)
}

/// Upstream `VersionGreaterThan3Decider`, the same as for Fallout 4.
pub fn version_greater_than3_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::version_greater_than3_decider(a_base_ptr, a_element)
}

/// Upstream `VersionGreaterThan4Decider`, the same as for Fallout 4.
pub fn version_greater_than4_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::version_greater_than4_decider(a_base_ptr, a_element)
}

/// Upstream `VMTypeAfterLoad`, the same as for Fallout 4.
pub fn vm_type_after_load(a_element: &ElementRef) {
    fo4saves::vm_type_after_load(a_element)
}

/// Upstream `WorldspaceTableAfterLoad`, the same as for Fallout 4.
pub fn worldspace_table_after_load(a_element: &ElementRef) {
    fo4saves::worldspace_table_after_load(a_element)
}

/// Upstream `ChangedFormGetChapterTypeName`, with the change types of
/// Skyrim.
pub fn changed_form_get_chapter_type_name(a_base_ptr: DataPtr, a_element: ElementArg) -> String {
    fo4saves::changed_form_chapter_type_name(WB_CHANGE_TYPES.get(), a_base_ptr, a_element)
}

/// Upstream `ArrayElementsTableElementCounter`: the count of the array the
/// entry has the handle of, with the handle in 32 bits.
pub fn array_elements_table_element_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    let mut handle: u32 = 0;
    let entry = wb_find_save_element(fo4saves::ARRAY_CONTENT_ENTRY_DATA, element);
    if let Some(container) = entry.as_data_container() {
        match container.get_element_by_name("Array Handle") {
            Some(found) => handle = native(Some(found)) as u32,
            None => return 0,
        }
    }
    if VM_ARRAY_TABLE_COUNT.load(Ordering::Relaxed) >= 0 {
        return query_count_for_vm_array_handle(i64::from(handle)) as u32;
    }
    let papyrus = wb_find_save_element("Papyrus Struct", element);
    let Some(table) = papyrus
        .as_data_container()
        .and_then(|papyrus| papyrus.get_element_by_name("Array Table"))
    else {
        return 0;
    };
    let Some(table) = table.as_data_container() else {
        return 0;
    };
    for index in 0..table.get_element_count() {
        let Some(entry) = table.get_element(index) else {
            continue;
        };
        let Some(entry) = entry.as_data_container() else {
            continue;
        };
        if native(entry.get_element_by_name("Array Handle")) == i64::from(handle) {
            return native(entry.get_element_by_name("Count")) as u32;
        }
    }
    0
}

/// Upstream `ArrayTableEntryOptionalStringDecider`.
pub fn array_table_entry_optional_string_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_child("Array Entry Data", element, "Array Type") {
        Some(kind) if native(Some(kind.clone())) == 1 => 2,
        Some(_) => 1,
        None => 0,
    }
}

/// Upstream `VMVersionDecider`.
pub fn vm_version_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match save_value_decider("Papyrus Struct", "SkyrimVM_version", a_element) {
        version @ 1..=4 => version,
        _ => 0,
    }
}

/// Upstream `FunctionTypeAndFlagsDecider`.
pub fn function_type_and_flags_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    if vm_version_decider(None, a_element) <= 1 {
        return 1;
    }
    let frame = wb_find_save_element("Frame Data", element);
    let Some(container) = frame.as_data_container() else {
        return 0;
    };
    let Some(flags) = container.get_element_by_name("Flags") else {
        return 0;
    };
    if native(Some(flags)) & 1 != 0 {
        return 0;
    }
    match container.get_element_by_path("Function Type") {
        Some(kind) if native(Some(kind.clone())) == 0 => 1,
        _ => 0,
    }
}

/// Upstream `GlobalData3Counter`: one more than the table says, as UESP
/// documents.
pub fn global_data3_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 1 };
    (root_value(
        element,
        "Save File Header\\File Location Table\\Global Data Table 3 Count",
    )
    .unwrap_or(0) as u32)
        .wrapping_add(1)
}

/// Upstream `GlobalDataDecider`: the member for the global data type; the
/// types 3 to 1000 are not decoded.
pub fn global_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    let mut element = element.clone();
    while !element.get_name().contains("Global Data ") {
        match element.get_container() {
            Some(container) => element = container,
            None => break,
        }
    }
    let Some(container) = element.as_data_container() else {
        return 0;
    };
    let mut result = 0;
    if let Some(kind) = container.get_element_by_name("Type") {
        let kind = native(Some(kind));
        result = match kind {
            0..=8 => kind as i32 + 1,
            100..=114 => (kind - 100 + 9 + 1) as i32,
            1000..=1005 => (kind - 1000 + 9 + 15 + 1) as i32,
            _ => 0,
        };
    }
    if result < 1001 - 1000 + 9 + 15 + 1 && result > 2 {
        result = 0;
    }
    result
}

/// Upstream `LZSaveSizer`: the size of the members, as the compressed size.
pub fn lz_save_sizer(a_base_ptr: DataPtr, a_element: ElementArg, compressed_size: &mut i32) -> u32 {
    fo4saves::changed_form_sizer(a_base_ptr, a_element, compressed_size)
}

/// Upstream `ObjectDataTableCounter`: the objects and the detached objects.
pub fn object_data_table_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let objects = VM_OBJECT_ARRAY_COUNT.load(Ordering::Relaxed);
    if objects >= 0 {
        return (objects + VM_OBJECT_DETACHED_ARRAY_COUNT.load(Ordering::Relaxed)) as u32;
    }
    let Some(element) = a_element else { return 0 };
    let papyrus = wb_find_save_element("Papyrus Struct", element);
    let Some(container) = papyrus.as_data_container() else {
        return 0;
    };
    (data_element_count(container.get_element_by_name("Object Table"))
        + data_element_count(container.get_element_by_name("Detached Object Table"))) as u32
}

/// Upstream `OpcodeParameterCounter`: the parameters of the opcode.
pub fn opcode_parameter_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    let Some(opcode) = save_value("Code", element, "Opcode") else {
        return 0;
    };
    match opcode {
        0 => 0,
        10..=14 => 2,
        20 => 1,
        21 | 22 | 24 => 2,
        26 => 1,
        30 | 31 => 2,
        34 | 35 => 4,
        _ => 3,
    }
}

/// Upstream `PreviousUnknownDecider`.
pub fn previous_unknown_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    if vm_version_decider(None, a_element) <= 1 {
        return 0;
    }
    match save_value("Papyrus Struct", element, "Previous Unknown") {
        Some(value) if value > 0 => 1,
        _ => 0,
    }
}

/// Upstream `SaveDataDecider`: the compression of the save data.
pub fn save_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match root_value(element, "Save File Header\\Header\\Compression Type") {
        Some(kind @ (1 | 2)) => kind as i32,
        _ => 0,
    }
}

/// Upstream `SaveDataSizer`: the sizes are in the elements before the data,
/// which the data cannot reach (`IsValidOffset` refuses a negative offset).
pub fn save_data_sizer(_a_base_ptr: DataPtr, a_element: ElementArg, compressed_size: &mut i32) -> u32 {
    let Some(container) = a_element.and_then(|element| element.as_data_container()) else {
        *compressed_size = 0;
        return 0;
    };
    let mut result = 0;
    if let Some(size) = container.get_element_by_path("Uncompressed Size") {
        result = native(Some(size)) as u32;
    }
    if let Some(size) = container.get_element_by_path("Compressed Size") {
        *compressed_size = native(Some(size)) as i32;
    }
    result
}

/// Port of `SaveFormVersionDecider`: the form version is in the content.
fn save_form_version_decider(minimum: i64, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match root_value(element, "Save File Header\\Content\\Form Version") {
        Some(version) if version > minimum => 1,
        _ => 0,
    }
}

/// Upstream `SaveFormVersion55Decider`, with the form version of the content.
pub fn save_form_version55_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    i32::from(root_value(element, "Save File Header\\Form Version") == Some(55))
}

/// Upstream `SaveFormVersionGreaterThan10Decider`.
pub fn save_form_version_greater_than10_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_form_version_decider(10, a_element)
}

/// Upstream `SaveFormVersionGreaterThan35Decider`: above 36, as upstream.
pub fn save_form_version_greater_than35_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_form_version_decider(36, a_element)
}

/// Upstream `SaveFormVersionGreaterThan72Decider`: above 73, as upstream.
pub fn save_form_version_greater_than72_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_form_version_decider(73, a_element)
}

/// Upstream `SaveFormVersionGreaterThan77Decider`: above 78, as upstream.
pub fn save_form_version_greater_than77_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_form_version_decider(78, a_element)
}

/// Port of `HasUnknownS1Decider`: only from version 2 of the Papyrus data.
fn has_unknown_s1_decider(name: &str, a_element: ElementArg) -> i32 {
    if vm_version_decider(None, a_element) <= 1 {
        return 0;
    }
    save_value_decider(name, "Has UnknownS1", a_element)
}

/// Upstream `HasFunctionUnknownS1Decider`.
pub fn has_function_unknown_s1_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    has_unknown_s1_decider("Function Call/Return Message", a_element)
}

/// Upstream `HasStackUnknownS1Decider`.
pub fn has_stack_unknown_s1_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    has_unknown_s1_decider("Suspended Stack", a_element)
}

/// Upstream `SaveIsValidDecider`.
pub fn save_is_valid_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Papyrus Struct", element, "Save File Version") {
        Some(version) if (0..=5).contains(&version) => 1,
        _ => 0,
    }
}

/// Upstream `SaveVersionGreaterThan11Decider`.
pub fn save_version_greater_than11_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_version_decider(11, a_element)
}

/// Upstream `ScreenShotDataCounter`: four bytes per pixel from save version
/// 12, three before.
pub fn screen_shot_data_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let bit_size = if save_version_decider(11, a_element) == 1 { 4 } else { 3 };
    let Some(element) = a_element else { return 0 };
    let Some(width) = root_value(element, "Save File Header\\Header\\Screenshot Width") else {
        return 0;
    };
    match root_value(element, "Save File Header\\Header\\Screenshot Height") {
        Some(height) => (bit_size * width * height) as u32,
        None => width as u32,
    }
}

/// Upstream `SKSEChaptersDecider`: the member for the chunk type, which
/// also starts the registrations.
pub fn skse_chapters_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    fo4saves::f4_se_chapters_decider(a_base_ptr, a_element)
}

/// Upstream `SKSERegKeyDecider`.
pub fn skse_reg_key_decider(_a_base_ptr: DataPtr, _a_element: ElementArg) -> i32 {
    match LAST_REGISTRATION_START.load(Ordering::Relaxed) {
        5 | 7 | 8 => 1,
        6 | 11 => 2,
        9 | 10 => 3,
        _ => 0,
    }
}

/// Upstream `SKSERegDataDecider`.
pub fn skse_reg_data_decider(_a_base_ptr: DataPtr, _a_element: ElementArg) -> i32 {
    i32::from(LAST_REGISTRATION_START.load(Ordering::Relaxed) == 8)
}

/// Upstream `StackTableDataEntryStackTypeDecider`.
pub fn stack_table_data_entry_stack_type_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match save_value_decider("Stack", "Type", a_element) {
        kind @ 1..=3 => kind,
        _ => 0,
    }
}

/// Upstream `VariableDecider`.
pub fn variable_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match save_value_decider("Variable", "Type", a_element) {
        kind @ 1..=5 => kind,
        kind @ 11..=15 => kind - 5,
        _ => 0,
    }
}
