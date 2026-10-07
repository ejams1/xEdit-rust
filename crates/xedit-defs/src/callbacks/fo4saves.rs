// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsFO4Saves.pas

//! The callbacks of `wbDefinitionsFO4Saves.pas` that are ported by hand.
//! The ones that are not ported yet are stubs in `fo4saves_stubs.rs`.

#[allow(unused_imports)]
pub use super::fo4saves_stubs::*;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use xedit_core::interface::globals::{
    bytes_to_dump, bytes_to_group, bytes_to_skip, set_extract_info, set_file_chapters, set_file_header, set_file_magic,
    set_file_plugins,
};
use xedit_core::interface::*;

use super::save_interface::{
    get_save_ref_id, initialize_save_ref_id_array, initialize_save_worldspace_array, initialize_vm_array_table,
    initialize_vm_object_array, initialize_vm_object_detached_array, initialize_vm_type_array,
    query_count_for_vm_array_handle, wb_find_save_element,
};
use crate::fo4::define_fo4;
use crate::fo4saves::{
    WB_CHANGE_TYPES, WB_CO_SAVE_CHAPTERS, WB_CO_SAVE_HEADER, define_fo4_saves_a, define_fo4_saves_s,
};

/// Upstream `ArrayContentEntryData`.
pub const ARRAY_CONTENT_ENTRY_DATA: &str = "Array Content Entry Data";

/// Upstream `ExtractInfoSave`: the chapters of a save that are initialized
/// while it loads.
const EXTRACT_INFO_SAVE: [u8; 2] = [4, 5];

/// Port of `DefineFO4Saves`.
pub fn define_fo4_saves() {
    set_file_magic("FO4_SAVEGAME");
    set_extract_info(&EXTRACT_INFO_SAVE);
    set_file_plugins("Plugins");
    define_fo4();
    define_fo4_saves_a();
    define_fo4_saves_s();
}

/// Port of `SwitchToFO4CoSave`.
pub fn switch_to_fo4_co_save() {
    set_file_magic("F4SE");
    set_extract_info(&[]);
    set_file_plugins("Absolute:44");
    set_file_chapters(WB_CO_SAVE_CHAPTERS.get());
    set_file_header(WB_CO_SAVE_HEADER.get());
}

/// Upstream `wbChangedFormOffset`: the chapter types of the changed forms
/// start here.
pub(crate) const CHANGED_FORM_OFFSET: i32 = 10000;

/// The lengths of the tables that the `AfterLoad` callbacks set once,
/// upstream `VMTypeCount` and the others; -1 until then.
pub(crate) static VM_TYPE_COUNT: AtomicI64 = AtomicI64::new(-1);
pub(crate) static WORLDSPACE_TABLE_COUNT: AtomicI64 = AtomicI64::new(-1);
pub(crate) static REF_ID_TABLE_COUNT: AtomicI64 = AtomicI64::new(-1);
pub(crate) static VM_OBJECT_ARRAY_COUNT: AtomicI64 = AtomicI64::new(-1);
pub(crate) static VM_SUPPLEMENT_OBJECT_ARRAY_COUNT: AtomicI64 = AtomicI64::new(-1);
pub(crate) static VM_OBJECT_DETACHED_ARRAY_COUNT: AtomicI64 = AtomicI64::new(-1);
pub(crate) static VM_ARRAY_TABLE_COUNT: AtomicI64 = AtomicI64::new(-1);
pub(crate) static STACK_TABLE_COUNT: AtomicI64 = AtomicI64::new(-1);
/// Upstream `LastRegistrationStart`.
pub(crate) static LAST_REGISTRATION_START: AtomicI64 = AtomicI64::new(0);

/// `Element.NativeValue` as an integer, 0 without the element.
pub(crate) fn native(element: Option<ElementRef>) -> i64 {
    match element.map(|element| element.get_native_value()) {
        Some(Variant::Float(value)) => value as i64,
        Some(value) => value.as_ordinal().unwrap_or(0),
        None => 0,
    }
}

/// The element count of a data container, 0 for anything else.
pub(crate) fn data_element_count(element: Option<ElementRef>) -> i64 {
    element
        .as_ref()
        .and_then(|element| element.as_data_container())
        .map_or(0, |container| i64::from(container.get_element_count()))
}

/// The file the element is in: the last container up the tree.
pub(crate) fn root(element: &ElementRef) -> ElementRef {
    let mut result = element.clone();
    while let Some(container) = result.get_container() {
        result = container;
    }
    result
}

/// `Container.ElementByPath[path].NativeValue` from the file of the element.
pub(crate) fn root_value(element: &ElementRef, path: &str) -> Option<i64> {
    let root = root(element);
    let found = root.as_container()?.get_element_by_path(path)?;
    Some(native(Some(found)))
}

/// The child `child` of the save element `name` (`wbFindSaveElement`), when
/// that element is a data container.
pub(crate) fn save_child(name: &str, element: &ElementRef, child: &str) -> Option<ElementRef> {
    let found = wb_find_save_element(name, element);
    found.as_data_container()?.get_element_by_name(child)
}

/// The native value of `save_child`.
pub(crate) fn save_value(name: &str, element: &ElementRef, child: &str) -> Option<i64> {
    save_child(name, element, child).map(|child| native(Some(child)))
}

/// Port of `SaveVersionDecider`: 1 when the save version is above the
/// minimum.
pub(crate) fn save_version_decider(minimum: i64, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match root_value(element, "Save File Header\\Header\\Version") {
        Some(version) if version > minimum => 1,
        _ => 0,
    }
}

/// Port of `SaveFormVersionDecider`: 1 when the form version is above the
/// minimum.
pub(crate) fn save_form_version_decider(minimum: i64, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match root_value(element, "Save File Header\\Form Version") {
        Some(version) if version > minimum => 1,
        _ => 0,
    }
}

/// Upstream `SaveVersionGreaterThan14Decider`.
pub fn save_version_greater_than14_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_version_decider(14, a_element)
}

/// Upstream `SaveFormVersion55Decider`.
pub fn save_form_version55_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    i32::from(root_value(element, "Save File Header\\Form Version") == Some(55))
}

/// Upstream `SaveFormVersionGreaterThan10Decider`.
pub fn save_form_version_greater_than10_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_form_version_decider(10, a_element)
}

/// A decider that is 1 when the bits of the mask are all set in the child
/// of the save element.
pub(crate) fn save_bits_decider(name: &str, child: &str, mask: i64, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value(name, element, child) {
        Some(value) if value & mask == mask => 1,
        _ => 0,
    }
}

/// A decider that is 1 when the child of the save element is not 0.
pub(crate) fn save_nonzero_decider(name: &str, child: &str, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value(name, element, child) {
        Some(value) if value != 0 => 1,
        _ => 0,
    }
}

/// A decider that is the value of the child of the save element.
pub(crate) fn save_value_decider(name: &str, child: &str, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    save_value(name, element, child).unwrap_or(0) as i32
}

/// A counter that is the value of the child of the save element.
pub(crate) fn save_value_counter(name: &str, child: &str, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    save_value(name, element, child).unwrap_or(0) as u32
}

/// Upstream `GlobalData6FlagsBit0Decider`.
pub fn global_data6_flags_bit0_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_bits_decider("Weather", "Flags", 0x1, a_element)
}

/// Upstream `GlobalData6FlagsBit1Decider`.
pub fn global_data6_flags_bit1_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_bits_decider("Weather", "Flags", 0x2, a_element)
}

/// Upstream `wbGlobalData6OwnedControllerDecider`.
pub fn wb_global_data6_owned_controller_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match save_value_decider("Reference Effect", "Owned Controller Type", a_element) {
        kind @ (1 | 2) => kind,
        _ => 0,
    }
}

/// Upstream `wbBSTempEffectScreenSpaceDecalSixDecider`.
pub fn wb_bs_temp_effect_screen_space_decal_six_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_nonzero_decider("Temp Effect Screen Space Decal", "Flag", a_element)
}

/// Upstream `wbBSTempEffectScreenSpaceDecalRemainDecider`.
pub fn wb_bs_temp_effect_screen_space_decal_remain_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_nonzero_decider("Reference Effect", "Flag2", a_element)
}

/// Upstream `wbBSTempEffectScreenSpaceDecalThirdDecider`.
pub fn wb_bs_temp_effect_screen_space_decal_third_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_nonzero_decider("Remain", "Flag3", a_element)
}

/// Upstream `wbGlobalData1000FlagDecider`.
pub fn wb_global_data1000_flag_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_nonzero_decider("Unknown 02 struct", "Flag", a_element)
}

/// Upstream `wbSubBufferCounter`.
pub fn wb_sub_buffer_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    save_value_counter("SubBuffer", "Length", a_element)
}

/// Upstream `wbGlobalData10TypeDecider`.
pub fn wb_global_data10_type_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Unknown struct", element, "Type") {
        Some(kind) if kind < 5 => 1 + kind as i32,
        _ => 0,
    }
}

/// Upstream `ScreenShotDataCounter`: four bytes per pixel.
pub fn screen_shot_data_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    let Some(width) = root_value(element, "Save File Header\\Header\\Screenshot Width") else {
        return 0;
    };
    match root_value(element, "Save File Header\\Header\\Screenshot Height") {
        Some(height) => (4 * width * height) as u32,
        None => width as u32,
    }
}

/// Port of `FileLocationTableCountCounter`.
fn file_location_table_count_counter(name: &str, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    root_value(element, &format!("Save File Header\\File Location Table\\{name} Count")).unwrap_or(0) as u32
}

/// Upstream `GlobalData1Counter`.
pub fn global_data1_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    file_location_table_count_counter("Global Data Table 1", a_element)
}

/// Upstream `GlobalData2Counter`.
pub fn global_data2_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    file_location_table_count_counter("Global Data Table 2", a_element)
}

/// Upstream `GlobalData3Counter`.
pub fn global_data3_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    file_location_table_count_counter("Global Data Table 3", a_element)
}

/// Upstream `ChangedFormsCounter`.
pub fn changed_forms_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    file_location_table_count_counter("Changed Forms", a_element)
}

/// Upstream `GlobalDataDecider`: the member for the global data type.
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
    let Some(kind) = container.get_element_by_name("Type") else {
        return 0;
    };
    let kind = native(Some(kind));
    match kind {
        0..=11 => kind as i32 + 1,
        100..=117 => (kind - 100 + 12 + 1) as i32,
        1000..=1008 => (kind - 1000 + 12 + 18 + 1) as i32,
        _ => 0,
    }
}

/// Upstream `ArrayTableEntryOptionalStringDecider`.
pub fn array_table_entry_optional_string_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_child("Array Entry Data", element, "Array Type") {
        Some(kind) => match native(Some(kind)) {
            1 | 7 => 2,
            _ => 1,
        },
        None => 0,
    }
}

/// An `AfterLoad` that keeps the length of the table once.
pub(crate) fn table_after_load(count: &AtomicI64, a_element: &ElementRef) -> bool {
    if count.load(Ordering::Relaxed) >= 0 {
        return false;
    }
    count.store(data_element_count(Some(a_element.clone())), Ordering::Relaxed);
    true
}

/// Upstream `VMTypeAfterLoad`.
pub fn vm_type_after_load(a_element: &ElementRef) {
    if table_after_load(&VM_TYPE_COUNT, a_element) {
        initialize_vm_type_array(a_element);
    }
}

/// Upstream `WorldspaceTableAfterLoad`.
pub fn worldspace_table_after_load(a_element: &ElementRef) {
    if table_after_load(&WORLDSPACE_TABLE_COUNT, a_element) {
        initialize_save_worldspace_array(a_element);
    }
}

/// Upstream `RefIDTableAfterLoad`.
pub fn ref_id_table_after_load(a_element: &ElementRef) {
    if table_after_load(&REF_ID_TABLE_COUNT, a_element) {
        initialize_save_ref_id_array(a_element);
    }
}

/// Upstream `ObjectTableAfterLoad`.
pub fn object_table_after_load(a_element: &ElementRef) {
    if table_after_load(&VM_OBJECT_ARRAY_COUNT, a_element) {
        initialize_vm_object_array(a_element);
    }
}

/// Upstream `SupplementObjectTableAfterLoad`: the object table already
/// holds its entries, as upstream initializes it once.
pub fn supplement_object_table_after_load(a_element: &ElementRef) {
    if table_after_load(&VM_SUPPLEMENT_OBJECT_ARRAY_COUNT, a_element) {
        initialize_vm_object_array(a_element);
        let supplement = VM_SUPPLEMENT_OBJECT_ARRAY_COUNT.load(Ordering::Relaxed);
        let count = VM_OBJECT_ARRAY_COUNT.load(Ordering::Relaxed);
        if count >= 0 {
            VM_OBJECT_ARRAY_COUNT.store(count + supplement, Ordering::Relaxed);
        }
    }
}

/// Upstream `ObjectDetachedTableAfterLoad`.
pub fn object_detached_table_after_load(a_element: &ElementRef) {
    if table_after_load(&VM_OBJECT_DETACHED_ARRAY_COUNT, a_element) {
        initialize_vm_object_detached_array(a_element);
    }
}

/// Upstream `ArrayTableAfterLoad`.
pub fn array_table_after_load(a_element: &ElementRef) {
    if table_after_load(&VM_ARRAY_TABLE_COUNT, a_element) {
        initialize_vm_array_table(a_element);
    }
}

/// Upstream `StackTableAfterLoad`.
pub fn stack_table_after_load(a_element: &ElementRef) {
    table_after_load(&STACK_TABLE_COUNT, a_element);
}

/// Upstream `VariableDecider`.
pub fn variable_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match save_value_decider("Variable", "Type", a_element) {
        kind @ 1..=7 => kind,
        kind @ 11..=17 => kind - 3,
        _ => 0,
    }
}

/// Upstream `TypeTable2Counter`.
pub fn type_table2_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    save_value_counter("Type tables for internal VM save data", "Type table 2 Count", a_element)
}

/// Upstream `TypeTable1Counter`.
pub fn type_table1_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    save_value_counter("Type tables for internal VM save data", "Type table 1 Count", a_element)
}

/// A counter that is the length of the table once its `AfterLoad` ran, else
/// the element count of the table in the Papyrus structure.
pub(crate) fn table_counter(count: &AtomicI64, path: &str, a_element: ElementArg) -> u32 {
    let count = count.load(Ordering::Relaxed);
    if count >= 0 {
        return count as u32;
    }
    let Some(element) = a_element else { return 0 };
    let papyrus = wb_find_save_element("Papyrus Struct", element);
    let Some(container) = papyrus.as_data_container() else {
        return 0;
    };
    data_element_count(container.get_element_by_path(path)) as u32
}

/// Upstream `ObjectTableDataCounter`.
pub fn object_table_data_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    table_counter(&VM_OBJECT_ARRAY_COUNT, "Object Table", a_element)
}

/// Upstream `DetachedObjectTableDataCounter`.
pub fn detached_object_table_data_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    table_counter(&VM_OBJECT_DETACHED_ARRAY_COUNT, "Detached Object Table", a_element)
}

/// Upstream `ArrayContentTableCounter`.
pub fn array_content_table_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    table_counter(&VM_ARRAY_TABLE_COUNT, "Array Table", a_element)
}

/// Upstream `StackContentTableCounter`.
pub fn stack_content_table_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    table_counter(&STACK_TABLE_COUNT, "Stacks\\Stack Table", a_element)
}

/// Upstream `ArrayElementsTableElementCounter`: the count of the array the
/// entry has the handle of.
pub fn array_elements_table_element_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    let mut handle = 0;
    let entry = wb_find_save_element(ARRAY_CONTENT_ENTRY_DATA, element);
    if let Some(container) = entry.as_data_container() {
        match container.get_element_by_name("Array Handle") {
            Some(found) => handle = native(Some(found)),
            None => return 0,
        }
    }
    if VM_ARRAY_TABLE_COUNT.load(Ordering::Relaxed) >= 0 {
        return query_count_for_vm_array_handle(handle) as u32;
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
        if native(entry.get_element_by_name("Array Handle")) == handle {
            return native(entry.get_element_by_name("Count")) as u32;
        }
    }
    0
}

/// Upstream `ObjectDataTableEntryExtraDecider`.
pub fn object_data_table_entry_extra_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_bits_decider("Script", "Unknown Flags", 0x4, a_element)
}

/// Upstream `GroupedDecider`.
pub fn grouped_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    i32::from(save_value_decider("Grouped", "Flag2bits", a_element) == 3)
}

/// Upstream `CodeParameterTypeValueDecider`.
pub fn code_parameter_type_value_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_value_decider("Parameter", "Type", a_element)
}

/// Upstream `OpcodeVariableParameterDecider`.
pub fn opcode_variable_parameter_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    i32::from(matches!(save_value_decider("Code", "Opcode", a_element), 23..=25))
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
        37 => 1,
        40 | 41 => 5,
        44 | 46 => 1,
        _ => 3,
    }
}

/// Upstream `FrameExtraVariablesCounter`.
pub fn frame_extra_variables_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    save_value_counter("Frame", "Extra Variables", a_element)
}

/// Upstream `StackTableDataEntryStackExtraDecider`.
pub fn stack_table_data_entry_stack_extra_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Stack", element, "Extra Flag") {
        Some(flags) if flags & 1 == 1 => 1,
        Some(flags) if flags & 2 == 2 => 2,
        _ => 0,
    }
}

/// The string native value of the child of the save element.
pub(crate) fn save_string(name: &str, element: &ElementRef, child: &str) -> Option<String> {
    save_child(name, element, child).map(|child| match child.get_native_value() {
        Variant::Str(text) => text,
        _ => child.get_value(),
    })
}

/// Upstream `StackCallBackTypeDecider`.
pub fn stack_call_back_type_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    let Some(kind) = save_string("Callback", element, "Type") else {
        return 0;
    };
    if kind.eq_ignore_ascii_case("QuestStage") {
        1
    } else if kind.eq_ignore_ascii_case("ScenePhaseResults") || kind.eq_ignore_ascii_case("SceneActionResults") {
        2
    } else if kind.eq_ignore_ascii_case("SceneResults") {
        3
    } else {
        0
    }
}

/// Upstream `StackTableDataEntryStackTypeDecider`.
pub fn stack_table_data_entry_stack_type_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    match save_value_decider("CallBacks", "Type", a_element) {
        kind @ (1 | 2) => kind,
        _ => 0,
    }
}

/// Upstream `wbCallbackTypeDataDecider`.
pub fn wb_callback_type_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_string("CallBack", element, "Type").as_deref() {
        Some("QuestStage") => 1,
        Some("SceneResults") => 2,
        Some("SceneActionResults") => 3,
        Some("ScenePhaseResults") => 4,
        _ => 0,
    }
}

/// Port of `VMVersionDecider`.
fn vm_version_decider(a_element: ElementArg) -> i32 {
    match save_value_decider("Papyrus Struct", "VM_version", a_element) {
        version @ (1 | 2) => version,
        _ => 0,
    }
}

/// Upstream `VMVersionGreaterThan1Decider`.
pub fn vm_version_greater_than1_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    i32::from(vm_version_decider(a_element) > 1)
}

/// Port of `SaveFileVersionDecider`.
fn save_file_version(a_element: ElementArg) -> i32 {
    save_value_decider("Papyrus Struct", "Save File Version", a_element)
}

/// Upstream `SaveFileVersionGreaterThanDDecider`.
pub fn save_file_version_greater_than_d_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    i32::from(save_file_version(a_element) > 0xD)
}

/// Upstream `SaveFileVersionGreaterThanBDecider`.
pub fn save_file_version_greater_than_b_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    i32::from(save_file_version(a_element) > 0xB)
}

/// Upstream `SaveIsValidDecider`.
pub fn save_is_valid_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Papyrus Struct", element, "Save File Version") {
        Some(version) if (0..=0xE).contains(&version) => 1,
        _ => 0,
    }
}

/// Upstream `VersionGreaterThan1Decider`.
pub fn version_greater_than1_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Papyrus Struct", element, "Save File Version") {
        Some(version) if version > 1 => 1,
        _ => 0,
    }
}

/// Upstream `FunctionTypeAndFlagsDecider`.
pub fn function_type_and_flags_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    if vm_version_decider(a_element) <= 1 {
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
    match container.get_element_by_name("Function Type") {
        Some(kind) if native(Some(kind.clone())) == 0 => 1,
        _ => 0,
    }
}

/// Upstream `HasFunctionUnknownS1Decider`.
pub fn has_function_unknown_s1_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_value_decider("Function Call/Return Message", "Has UnknownS1", a_element)
}

/// Upstream `HasStackUnknownS1Decider`.
pub fn has_stack_unknown_s1_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_value_decider("Suspended Stack", "Has UnknownS1", a_element)
}

/// Upstream `PreviousUnknownDecider`.
pub fn previous_unknown_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Tables", element, "Previous Unknown") {
        Some(value) if value > 0 => 1,
        _ => 0,
    }
}

/// Upstream `wbLOSEventDataDecider`.
pub fn wb_los_event_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("LOS event", element, "Type") {
        Some(0) => 1,
        Some(1) => 2,
        _ => 0,
    }
}

/// Upstream `FunctorDecider`. The test is always true upstream.
pub fn functor_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Functor", element, "Type") {
        Some(kind) => kind as i32 + 1,
        None => 0,
    }
}

/// Upstream `GlobalDataGetChapterType`.
pub fn global_data_get_chapter_type(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    a_element
        .and_then(|element| element.as_container())
        .and_then(|container| container.get_element_by_name("Type"))
        .map_or(-1, |kind| native(Some(kind)) as i32)
}

/// The middle word of a value such as `1001 (0x3E9) Name`: the text after
/// the first blank, up to the next blank.
pub(crate) fn chapter_type_word(mut text: String) -> String {
    if text.len() > 1
        && let Some(blank) = text.find(' ')
    {
        text = text[blank + 1..].to_owned();
    }
    if text.len() > 1
        && let Some(blank) = text.find(' ')
    {
        text.truncate(blank);
    }
    text
}

/// Upstream `GlobalDataGetChapterTypeName`.
pub fn global_data_get_chapter_type_name(a_base_ptr: DataPtr, a_element: ElementArg) -> String {
    match a_element {
        Some(element) => {
            let value = element
                .as_container()
                .and_then(|container| container.get_element_by_path("Type"))
                .map(|kind| kind.get_value())
                .unwrap_or_default();
            chapter_type_word(value)
        }
        None => global_data_get_chapter_type(a_base_ptr, a_element).to_string(),
    }
}

/// Port of `ChangedFormGetRawType`: the type byte of the changed form, with
/// the changed form itself.
pub(crate) fn changed_form_get_raw_type(element: &ElementRef) -> (i32, ElementRef) {
    const OFFSET_TYPE: usize = 7;
    let changed_form = wb_find_save_element("Changed Form", element);
    let kind = changed_form
        .as_data_container()
        .and_then(|container| container.get_data())
        .and_then(|data| data.get(OFFSET_TYPE).copied())
        .map_or(-1, i32::from);
    (kind, changed_form)
}

/// Upstream `GlobalDataSizer`.
pub fn global_data_sizer(a_base_ptr: DataPtr, _a_element: ElementArg, compressed_size: &mut i32) -> u32 {
    let length = a_base_ptr
        .and_then(|data| data.get(4..8))
        .map_or(0, |bytes| u32::from_le_bytes(bytes.try_into().unwrap()));
    *compressed_size = length.wrapping_add(8) as i32;
    *compressed_size as u32
}

/// The unsigned integer of `size` bytes (1, 2 or 4) at `offset`.
pub(crate) fn sized_value(data: &[u8], offset: usize, size_length: i32) -> Option<i64> {
    Some(match size_length {
        0 => i64::from(*data.get(offset)?),
        1 => i64::from(u16::from_le_bytes(data.get(offset..offset + 2)?.try_into().ok()?)),
        2 => i64::from(u32::from_le_bytes(data.get(offset..offset + 4)?.try_into().ok()?)),
        _ => return None,
    })
}

/// Upstream `ChangedFormDataSizer`: the uncompressed and compressed lengths
/// of the data of the changed form.
pub fn changed_form_data_sizer(_a_base_ptr: DataPtr, a_element: ElementArg, compressed_size: &mut i32) -> u32 {
    const OFFSET_LENGTH: usize = 9;
    let Some(element) = a_element else {
        *compressed_size = 0;
        return 0;
    };
    let (kind, changed_form) = changed_form_get_raw_type(element);
    let Some(container) = changed_form.as_data_container().filter(|_| kind >= 0) else {
        *compressed_size = 0;
        return 0;
    };
    let data = container.get_data().unwrap_or_default();
    let size_length = kind >> 6;
    let mut result = 0;
    match container.get_element_by_path("Datas\\CForm Data\\Uncompressed Length") {
        Some(length) => result = native(Some(length)) as u32,
        None => {
            if let Some(value) = sized_value(data, OFFSET_LENGTH + size_length as usize + 1, size_length) {
                result = value as u32;
            }
        }
    }
    match container.get_element_by_path("Datas\\CForm Data\\Length") {
        Some(length) => *compressed_size = native(Some(length)) as i32,
        None => {
            if let Some(value) = sized_value(data, OFFSET_LENGTH, size_length) {
                *compressed_size = value as i32;
            }
        }
    }
    result
}

/// Upstream `ChangedFormSizer`: the size of the members, as the compressed
/// size.
pub fn changed_form_sizer(a_base_ptr: DataPtr, a_element: ElementArg, compressed_size: &mut i32) -> u32 {
    *compressed_size = 0;
    let Some(element) = a_element else { return 0 };
    let Some(value_def) = element.get_value_def() else {
        return 0;
    };
    let Some(struct_def) = value_def.as_struct_def().filter(|def| def.chapter().is_some()) else {
        return 0;
    };
    let mut data = a_base_ptr;
    for index in 0..usize::try_from(struct_def.get_member_count()).unwrap_or(0) {
        let size = struct_def.get_member(index).get_size(data, Some(element));
        if size != i32::MAX {
            data = data.map(|data| &data[(size.max(0) as usize).min(data.len())..]);
            *compressed_size = compressed_size.wrapping_add(size);
        }
    }
    0
}

/// Upstream `ChangedFormGetChapterType`.
pub fn changed_form_get_chapter_type(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return -1 };
    let (kind, _) = changed_form_get_raw_type(element);
    if kind >= 0 {
        CHANGED_FORM_OFFSET + (kind & 0x3F)
    } else {
        kind
    }
}

/// Upstream `ChangedFormGetChapterTypeName`.
pub fn changed_form_get_chapter_type_name(a_base_ptr: DataPtr, a_element: ElementArg) -> String {
    changed_form_chapter_type_name(WB_CHANGE_TYPES.get(), a_base_ptr, a_element)
}

/// Port of `ChangedFormGetChapterTypeName` with the change types of the game.
pub(crate) fn changed_form_chapter_type_name(
    change_types: Option<Arc<EnumDef>>,
    a_base_ptr: DataPtr,
    a_element: ElementArg,
) -> String {
    let kind = changed_form_get_chapter_type(a_base_ptr, a_element);
    let mut result = String::new();
    if let Some(change_types) = change_types
        && kind >= CHANGED_FORM_OFFSET
        && kind < CHANGED_FORM_OFFSET + change_types.get_name_count()
    {
        result = change_types.get_name_of(i64::from(kind - CHANGED_FORM_OFFSET));
    }
    let result = chapter_type_word(result);
    if result.is_empty() { kind.to_string() } else { result }
}

/// Upstream `ChangedFormGetChapterName`: the reference ID of the form.
pub fn changed_form_get_chapter_name(_a_base_ptr: DataPtr, a_element: ElementArg) -> String {
    a_element
        .and_then(|element| element.as_container())
        .and_then(|container| container.get_element_by_name("RefID"))
        .map(|ref_id| ref_id.get_value())
        .unwrap_or_default()
}

/// Upstream `ChangedFormDataLengthDecider`: the size of the lengths.
pub fn changed_form_data_length_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 3 };
    match changed_form_get_raw_type(element).0 >> 6 {
        size @ 0..=2 => size,
        _ => 3,
    }
}

/// Upstream `ChangedFormDataDecider`: the member for the form type.
pub fn changed_form_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    let (kind, changed_form) = changed_form_get_raw_type(element);
    if kind >= 0 && changed_form.as_data_container().is_some() {
        1 + (kind & 0x3F)
    } else {
        0
    }
}

/// Upstream `ChangedFormNPCFaceHasHeadDataDecider`.
pub fn changed_form_npc_face_has_head_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_value_decider("Change Actor Face", "Has Head Data", a_element)
}

/// Upstream `ChangedFormNPCFaceHasFaceMorphDecider`.
pub fn changed_form_npc_face_has_face_morph_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_value_decider("Change Actor Face", "Has Face Morph", a_element)
}

/// Upstream `ChangedFormProjectileHasInventoryDecider`.
pub fn changed_form_projectile_has_inventory_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_value_decider("Change Projectile", "Has Inventory", a_element)
}

/// Upstream `ChangedFormExtraUnknown12HasUnk010Decider`.
pub fn changed_form_extra_unknown12_has_unk010_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_value_decider("Extra Unknown 12", "Has Unk010", a_element)
}

/// Upstream `ChangedFormActorHasEquipDataDecider`.
pub fn changed_form_actor_has_equip_data_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_value_decider("Change Actor", "Has Equip Data", a_element)
}

/// Upstream `ChangedFormHavokMovedSubBufferCounter`.
pub fn changed_form_havok_moved_sub_buffer_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    save_value_counter("Havok Moved SubBuffer", "Length", a_element)
}

/// Upstream `ChangedFormAnimationSubBufferCounter`.
pub fn changed_form_animation_sub_buffer_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    save_value_counter("Animation SubBuffer", "Length", a_element)
}

/// Upstream `ChangedFormFlagsDecider`.
pub fn changed_form_flags_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    let (kind, changed_form) = changed_form_get_raw_type(element);
    // `and $3F` of -1 is 63, as upstream.
    let kind = kind & 0x3F;
    if changed_form.as_data_container().is_some() {
        1 + kind
    } else {
        0
    }
}

/// Port of `ChangedFlagXXDecider`: 1 when one of the bits of the mask is set
/// in the change flags of the changed form.
pub(crate) fn changed_flag_xx_decider(mask: i64, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Changed Form", element, "Change Flags") {
        Some(flags) if flags & mask != 0 => 1,
        _ => 0,
    }
}

/// Upstream `ChangedFlag00Decider`.
pub fn changed_flag00_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0001, a_element)
}

/// Upstream `ChangedFlag01Decider`.
pub fn changed_flag01_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0002, a_element)
}

/// Upstream `ChangedFlag02Decider`.
pub fn changed_flag02_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0004, a_element)
}

/// Upstream `ChangedFlag03Decider`.
pub fn changed_flag03_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0008, a_element)
}

/// Upstream `ChangedFlag04Decider`.
pub fn changed_flag04_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0010, a_element)
}

/// Upstream `ChangedFlag05Decider`.
pub fn changed_flag05_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0020, a_element)
}

/// Upstream `ChangedFlag05or27Decider`.
pub fn changed_flag05or27_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0800_0020, a_element)
}

/// Upstream `ChangedFlag06Decider`.
pub fn changed_flag06_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0040, a_element)
}

/// Upstream `ChangedFlag07Decider`.
pub fn changed_flag07_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0080, a_element)
}

/// Upstream `ChangedFlag08Decider`.
pub fn changed_flag08_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0100, a_element)
}

/// Upstream `ChangedFlag09Decider`.
pub fn changed_flag09_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0200, a_element)
}

/// Upstream `ChangedFlag10Decider`.
pub fn changed_flag10_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0400, a_element)
}

/// Upstream `ChangedFlag11Decider`.
pub fn changed_flag11_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_0800, a_element)
}

/// Upstream `ChangedFlag12Decider`.
pub fn changed_flag12_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_1000, a_element)
}

/// Upstream `ChangedFlag13Decider`.
pub fn changed_flag13_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_2000, a_element)
}

/// Upstream `ChangedFlag14Decider`.
pub fn changed_flag14_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0000_4000, a_element)
}

/// Upstream `ChangedFlag17Decider`.
pub fn changed_flag17_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0002_0000, a_element)
}

/// Upstream `ChangedFlag18Decider`.
pub fn changed_flag18_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0004_0000, a_element)
}

/// Upstream `ChangedFlag19Decider`.
pub fn changed_flag19_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0008_0000, a_element)
}

/// Upstream `ChangedFlag20Decider`.
pub fn changed_flag20_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0010_0000, a_element)
}

/// Upstream `ChangedFlag21Decider`.
pub fn changed_flag21_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0020_0000, a_element)
}

/// Upstream `ChangedFlag22Decider`.
pub fn changed_flag22_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0040_0000, a_element)
}

/// Upstream `ChangedFlag23Decider`.
pub fn changed_flag23_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0080_0000, a_element)
}

/// Upstream `ChangedFlag24Decider`.
pub fn changed_flag24_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0100_0000, a_element)
}

/// Upstream `ChangedFlag25Decider`.
pub fn changed_flag25_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0200_0000, a_element)
}

/// Upstream `ChangedFlag26Decider`.
pub fn changed_flag26_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0400_0000, a_element)
}

/// Upstream `ChangedFlag27Decider`.
pub fn changed_flag27_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x0800_0000, a_element)
}

/// Upstream `ChangedFlag28Decider`.
pub fn changed_flag28_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x1000_0000, a_element)
}

/// Upstream `ChangedFlag29Decider`.
pub fn changed_flag29_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x2000_0000, a_element)
}

/// Upstream `ChangedFlag30Decider`.
pub fn changed_flag30_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x4000_0000, a_element)
}

/// Upstream `ChangedFlag31Decider`.
pub fn changed_flag31_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    changed_flag_xx_decider(0x8000_0000, a_element)
}

/// Upstream `QuestRuntimeAliasTypeDecider`.
pub fn quest_runtime_alias_type_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_value_decider("Alias", "Type", a_element)
}

/// Upstream `QuestRuntimeHasEventDecider`.
pub fn quest_runtime_has_event_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_value_decider("Runtime Data", "Has Event", a_element)
}

/// Upstream `QuestRuntimeParamTypeDecider`.
pub fn quest_runtime_param_type_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Param", element, "Type") {
        Some(kind) if kind > 4 => 0,
        Some(kind) => kind as i32 + 1,
        None => 0,
    }
}

/// Upstream `InitialDataTypeDecider`.
pub fn initial_data_type_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    let (kind, changed_form) = changed_form_get_raw_type(element);
    let kind = kind & 0x3F;
    let Some(container) = changed_form.as_data_container() else {
        return 0;
    };
    let flag = |mask: i64| changed_flag_xx_decider(mask, a_element) != 0;
    let mut result = 0;
    if kind == 6 {
        // CELL
        if flag(0x4000_0000) && flag(0x2000_0000) {
            result = 1;
        } else if flag(0x4000_0000) && flag(0x1000_0000) {
            result = 2;
        } else if flag(0x4000_0000) {
            result = 3;
        }
    }
    if matches!(kind, 0..=5 | 40..=42) {
        // REFR or a descendant.
        let constructed = container
            .get_element_by_name("RefID")
            .is_some_and(|ref_id| native(Some(ref_id)) >> 22 == 2);
        if constructed {
            result = 5;
        } else if flag(0x0200_0008) {
            result = 6;
        } else if flag(0x0000_0006) {
            result = 4;
        }
    }
    let _ = a_base_ptr;
    result
}

/// Upstream `ExtraTypeToDecider` of `ChangedExtraUnionDecider`.
const EXTRA_TYPE_TO_DECIDER: [u8; 165] = [
    0, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0x0A, 0x4A, 0x0B, 0x0C, 0x0D,
    0x0E, 0x0F, 0x4A, 0x10, 0x11, 0x4A, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x4A, 0x18, 0x19, 0x4A, 0x1A, 0x4A, 0x4A,
    0x4A, 0x1B, 0x4A, 0x4A, 0x4A, 0x4A, 0x1C, 0x1D, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x1E, 0x1F, 0x4A, 0x4A, 0x20, 0x21,
    0x4A, 0x4A, 0x22, 0x23, 0x4A, 0x24, 0x4A, 0x4A, 0x4A, 0x25, 0x26, 0x27, 0x4A, 0x4A, 0x28, 0x29, 0x4A, 0x2A, 0x2B,
    0x2C, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x2D, 0x4A, 0x4A, 0x2E, 0x4A, 0x2F, 0x4A, 0x30, 0x4A, 0x4A, 0x31,
    0x32, 0x33, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x34, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A, 0x4A,
    0x4A, 0x4A, 0x35, 0x4A, 0x36, 0x37, 0x4A, 0x4A, 0x4A, 0x38, 0x4A, 0x39, 0x4A, 0x4A, 0x4A, 0x3A, 0x4A, 0x4A, 0x3B,
    0x3C, 0x4A, 0x3D, 0x3E, 0x4A, 0x3F, 0x40, 0x41, 0x4A, 0x42, 0x43, 0x44, 0x4A, 0x4A, 0x45, 0x4A, 0x4A, 0x4A, 0x4A,
    0x46, 0x4A, 0x4A, 0x4A, 0x4A, 0x47, 0x48, 0x49,
];

/// Upstream `ChangedExtraUnionDecider`: the member for the extra type, only
/// the first 16 are decoded.
pub fn changed_extra_union_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    let mut result = 0;
    if let Some(kind) = save_value("Extra", element, "Extra Type")
        && (0x0C..=0x0C + 164).contains(&kind)
    {
        result = 1 + i32::from(EXTRA_TYPE_TO_DECIDER[(kind - 0x0C) as usize]);
        if result == 0x4A + 1 {
            result = 0;
        }
    }
    if result > 1 + 0x10 { 0 } else { result }
}

/// Upstream `ChangedFormExtraDecider`.
pub fn changed_form_extra_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    let kind = changed_form_get_raw_type(element).0 & 0x3F;
    let mut result = 0;
    if matches!(kind, 0 | 2..=5 | 40..=42 | 47) && changed_flag_xx_decider(0xA602_1C40, a_element) != 0 {
        result = 1;
    }
    if kind == 1 && changed_flag_xx_decider(0xA606_1840, a_element) != 0 {
        result = 1;
    }
    result
}

/// The distance from `origin` to `pointer` in the same data, as upstream
/// subtracts pointers in 32 bits.
pub(crate) fn consumed(pointer: DataPtr, origin: DataPtr) -> u32 {
    let address = |data: DataPtr| data.map_or(0, |data| data.as_ptr() as usize);
    address(pointer).wrapping_sub(address(origin)) as u32
}

/// The data length of the `CForm Data`: the uncompressed length, or the
/// length when that is 0.
pub(crate) fn cform_data_length(element: &ElementRef) -> u32 {
    let cform = wb_find_save_element("CForm Data", element);
    let Some(container) = cform.as_data_container() else {
        return 0;
    };
    let Some(uncompressed) = container
        .get_element_by_name("Uncompressed Length")
        .filter(|length| length.as_data_container().is_some())
    else {
        return 0;
    };
    let mut result = native(Some(uncompressed)) as u32;
    if result == 0
        && let Some(length) = container
            .get_element_by_name("Length")
            .filter(|length| length.as_data_container().is_some())
    {
        result = native(Some(length)) as u32;
    }
    result
}

/// Port of `ChangedFormRemainingDataFromHereCounter`.
pub(crate) fn changed_form_remaining_data_from_here_counter(a_base_ptr: DataPtr, element: &ElementRef) -> u32 {
    let mut result = cform_data_length(element);
    if result > 0 {
        let data = wb_find_save_element("Changed Form Data", element);
        if let Some(container) = data.as_data_container() {
            result = result.wrapping_sub(consumed(a_base_ptr, container.get_data()));
        }
    }
    result
}

/// Upstream `ChangedFormRemainingDataCounter`.
pub fn changed_form_remaining_data_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    let mut result = cform_data_length(element);
    if result > 0 {
        let data = wb_find_save_element("Changed Form Data", element);
        if let Some(container) = data.as_data_container()
            && container.get_element_count() == 3
            && let Some(third) = container.get_element(2)
            && let Some(third) = third.as_data_container()
        {
            result = result.wrapping_sub(consumed(third.get_data(), container.get_data()));
        }
    }
    result
}

/// Upstream `ChangedFormCellIsInteriorDecider`.
pub fn changed_form_cell_is_interior_decider(a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    const EXTERIOR_SIZE: u32 = 0x20;
    let Some(element) = a_element else { return 0 };
    let mut remaining = changed_form_remaining_data_from_here_counter(a_base_ptr, element);
    let cell = wb_find_save_element("Change CELL Data", element);
    if cell.as_container().is_none() {
        return 0;
    }
    if changed_flag_xx_decider(0x4, a_element) == 1 {
        let string_size = 2 + a_base_ptr
            .and_then(|data| data.get(EXTERIOR_SIZE as usize..EXTERIOR_SIZE as usize + 2))
            .map_or(0, |bytes| u32::from(u16::from_le_bytes(bytes.try_into().unwrap())));
        remaining = remaining.wrapping_sub(string_size);
    }
    if changed_flag_xx_decider(0x8, a_element) == 1 {
        remaining = remaining.wrapping_sub(3);
    }
    i32::from(remaining != EXTERIOR_SIZE)
}

/// Port of `DataLengthCounter`.
pub(crate) fn data_length_counter(name: &str, a_element: ElementArg, modifier: i32) -> u32 {
    let Some(element) = a_element else { return 0 };
    let Some(length) = save_value(name, element, "DataLength") else {
        return 0;
    };
    let group = bytes_to_group();
    match modifier {
        1 => (length / group) as u32,
        2 => (length % group) as u32,
        _ => length as u32,
    }
}

/// Port of `DataLengthRemainderCounter`.
pub(crate) fn data_length_remainder_counter(name: &str, a_element: ElementArg, modifier: i32) -> u32 {
    let Some(element) = a_element else { return 0 };
    let found = wb_find_save_element(name, element);
    let Some(container) = found.as_data_container() else {
        return 0;
    };
    let Some(length) = container.get_element_by_name("DataLength") else {
        return 0;
    };
    let Some(length_data) = length.as_data_container().map(|length| length.get_data()) else {
        return 0;
    };
    let mut result = (native(Some(length.clone())) as u32).wrapping_add(4);
    if let Some(remainder) = container.get_element_by_name("Remainder")
        && let Some(remainder) = remainder.as_data_container()
    {
        result = result.wrapping_sub(consumed(remainder.get_data(), length_data));
        let group = bytes_to_group() as u32;
        match modifier {
            1 => result /= group,
            2 => result %= group,
            _ => {}
        }
    }
    result
}

/// Upstream `DataCounter`.
pub fn data_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    data_length_counter("Global Data", a_element, 0)
}

/// Upstream `DataQuartetCounter`.
pub fn data_quartet_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    data_length_remainder_counter("Global Data", a_element, 1)
}

/// Upstream `DataQuartetRemainderCounter`.
pub fn data_quartet_remainder_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    data_length_remainder_counter("Global Data", a_element, 2)
}

/// Upstream `F4SEChaptersDecider`: the member for the chunk type, which
/// also starts the registrations.
pub fn f4_se_chapters_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    let chunk = wb_find_save_element("Chunk", element);
    let Some(kind) = chunk
        .as_data_container()
        .and_then(|chunk| chunk.get_element_by_name("Type"))
    else {
        return 0;
    };
    let registration = |result: i64| {
        LAST_REGISTRATION_START.store(result, Ordering::Relaxed);
        result as i32
    };
    match kind.get_value().as_str() {
        "MODS" => 1,
        "LMOD" => 2,
        // Everything from here is copied from SKSE and not verified upstream.
        "REGS" => 3,
        "REGE" => 4,
        "MENR" => registration(5),
        "KEYR" => registration(6),
        "CTLR" => registration(7),
        "MCBR" => registration(8),
        "CHRR" => registration(9),
        "CAMR" => registration(10),
        "AACT" => registration(11),
        _ => 12,
    }
}

/// Upstream `F4SERegKeyDecider`.
pub fn f4_se_reg_key_decider(_a_base_ptr: DataPtr, _a_element: ElementArg) -> i32 {
    match LAST_REGISTRATION_START.load(Ordering::Relaxed) {
        5 | 7 | 8 => 1,
        6 | 11 => 2,
        9 | 10 => 3,
        _ => 0,
    }
}

/// Upstream `F4SERegDataDecider`.
pub fn f4_se_reg_data_decider(_a_base_ptr: DataPtr, _a_element: ElementArg) -> i32 {
    i32::from(LAST_REGISTRATION_START.load(Ordering::Relaxed) == 8)
}

/// Upstream `SaveFormVersionGreaterThan35Decider`: above 36, as upstream.
pub fn save_form_version_greater_than35_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_form_version_decider(36, a_element)
}

/// Upstream `SaveFormVersionGreaterThan72Decider`: above 73, as upstream.
pub fn save_form_version_greater_than72_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    save_form_version_decider(73, a_element)
}

/// Upstream `VersionGreaterThan3Decider`.
pub fn version_greater_than3_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Papyrus Struct", element, "Save File Version") {
        Some(version) if version > 3 => 1,
        _ => 0,
    }
}

/// Upstream `VersionGreaterThan4Decider`.
pub fn version_greater_than4_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Papyrus Struct", element, "Save File Version") {
        Some(version) if version > 4 => 1,
        _ => 0,
    }
}

/// Upstream `FirstCountCounter`.
pub fn first_count_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    save_value_counter("Dual RefID Table", "First Count", a_element)
}

/// Upstream `SecondCountCounter`.
pub fn second_count_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    save_value_counter("Dual RefID Table", "Second Count", a_element)
}

/// Upstream `Unknown1000_00001Decider`.
pub fn unknown1000_00001_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match save_value("Unknown1000_0000", element, "Unknown1000_00000") {
        Some(0) | None => 0,
        Some(_) => 1,
    }
}

/// Upstream `SkipCounter`: the bytes to skip (`-bts`).
pub fn skip_counter(_a_base_ptr: DataPtr, _a_element: ElementArg) -> u32 {
    bytes_to_skip() as u32
}

/// Upstream `DumpCounter`: the groups of bytes to dump (`-btd`), else all
/// the remaining data.
pub fn dump_counter(a_base_ptr: DataPtr, _a_element: ElementArg) -> u32 {
    let group = bytes_to_group() as u32;
    if bytes_to_dump() == 0xFFFF_FFFF {
        a_base_ptr.map_or(0, <[u8]>::len) as u32 / group + 1
    } else {
        bytes_to_dump() as u32 / group + 1
    }
}

/// Upstream `PlayerRefIndex`: the reference ID of the player once found.
static PLAYER_REF_INDEX: AtomicI64 = AtomicI64::new(0);

/// Upstream `IsActorPlayerDecider`: 1 for the changed form of the player.
pub fn is_actor_player_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    /// Upstream `wbPlayerRefID`.
    const PLAYER_REF_ID: u32 = 0x14;
    let Some(element) = a_element else { return 0 };
    let Some(id) = save_value("Changed Form", element, "RefID") else {
        return 0;
    };
    let id = id as i32 as i64;
    if id <= 0 {
        return 0;
    }
    if PLAYER_REF_INDEX.load(Ordering::Relaxed) == 0 && id >> 22 == 0 && get_save_ref_id(id as u32) == PLAYER_REF_ID {
        PLAYER_REF_INDEX.store(id, Ordering::Relaxed);
    }
    i32::from(id == PLAYER_REF_INDEX.load(Ordering::Relaxed))
}
