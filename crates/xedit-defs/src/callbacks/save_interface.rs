// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbSaveInterface.pas

//! The save interface: the formaters, counters and deciders that the save
//! game definitions share, with the lookup tables that the chapters of a
//! save fill while it loads.

use std::sync::{Arc, RwLock};

use xedit_core::interface::formaters::str4_to_string;
use xedit_core::interface::globals::hide_never_show;
use xedit_core::interface::misc::int_to_hex64;
use xedit_core::interface::*;

/// Upstream `sifVMTypeArray`: the names of the script types.
static VM_TYPE_ARRAY: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// A script object handle and the index of its type.
#[derive(Clone, Copy)]
struct ObjectHandle {
    handle: i64,
    vm_type: i64,
}

/// Upstream `ohfVMObjectHandleTable` and `ohfVMObjectDetachedHandleTable`.
static OBJECT_HANDLE_TABLE: RwLock<Vec<ObjectHandle>> = RwLock::new(Vec::new());
static OBJECT_DETACHED_HANDLE_TABLE: RwLock<Vec<ObjectHandle>> = RwLock::new(Vec::new());

/// Upstream `ahfVMArrayHandleTable`: the script arrays and their counts.
static ARRAY_HANDLE_TABLE: RwLock<Vec<(i64, i64)>> = RwLock::new(Vec::new());

/// Upstream `sifSaveWorldspaceArray`: the elements of the visited
/// worldspaces. Their values are read when a worldspace index is shown, as
/// upstream does: reading them here, while the save loads, would resolve the
/// names of the worldspace records before their strings are loaded.
static SAVE_WORLDSPACE_ARRAY: RwLock<Vec<ElementRef>> = RwLock::new(Vec::new());

/// Upstream `SaveRefIDArray`.
static SAVE_REF_ID_ARRAY: RwLock<Vec<u32>> = RwLock::new(Vec::new());

/// The ordinal native value of an element, 0 without one.
fn ordinal(element: Option<ElementRef>) -> i64 {
    match element.map(|element| element.get_native_value()) {
        Some(Variant::Float(value)) => value as i64,
        Some(value) => value.as_ordinal().unwrap_or(0),
        None => 0,
    }
}

/// The elements of a container.
fn elements(container: &ElementRef) -> Vec<ElementRef> {
    let Some(container) = container.as_container() else {
        return Vec::new();
    };
    (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .collect()
}

/// The handle and the type of each element of an object table.
fn object_handles(container: &ElementRef) -> Vec<ObjectHandle> {
    elements(container)
        .iter()
        .map(|element| {
            let element = element.as_container();
            ObjectHandle {
                handle: ordinal(element.and_then(|element| element.get_element_by_name("Object Handle"))),
                vm_type: ordinal(element.and_then(|element| element.get_element_by_name("Name"))),
            }
        })
        .collect()
}

/// Port of `InitializeVMTypeArray`: once.
pub fn initialize_vm_type_array(a_container: &ElementRef) {
    let mut table = VM_TYPE_ARRAY.write().unwrap();
    if table.is_empty() {
        *table = elements(a_container)
            .iter()
            .map(|element| match element.get_native_value() {
                Variant::Str(text) => text,
                _ => element.get_value(),
            })
            .collect();
    }
}

/// Port of `InitializeVMObjectArray`: once.
pub fn initialize_vm_object_array(a_container: &ElementRef) {
    let mut table = OBJECT_HANDLE_TABLE.write().unwrap();
    if table.is_empty() {
        *table = object_handles(a_container);
    }
}

/// Port of `InitializeVMObjectDetachedArray`: once, from a table that is
/// not empty.
pub fn initialize_vm_object_detached_array(a_container: &ElementRef) {
    let mut table = OBJECT_DETACHED_HANDLE_TABLE.write().unwrap();
    if table.is_empty() {
        *table = object_handles(a_container);
    }
}

/// Port of `InitializeVMArrayTable`: once.
pub fn initialize_vm_array_table(a_container: &ElementRef) {
    let mut table = ARRAY_HANDLE_TABLE.write().unwrap();
    if table.is_empty() {
        *table = elements(a_container)
            .iter()
            .map(|element| {
                let element = element.as_container();
                (
                    ordinal(element.and_then(|element| element.get_element_by_name("Array Handle"))),
                    ordinal(element.and_then(|element| element.get_element_by_name("Count"))),
                )
            })
            .collect();
    }
}

/// Port of `InitializeSaveWorldspaceArray`: once.
pub fn initialize_save_worldspace_array(a_container: &ElementRef) {
    let mut table = SAVE_WORLDSPACE_ARRAY.write().unwrap();
    if table.is_empty() {
        *table = elements(a_container);
    }
}

/// Port of `InitializeSaveRefIDArray`: once, and the array of the reference
/// IDs.
pub fn initialize_save_ref_id_array(a_container: &ElementRef) {
    let mut table = SAVE_REF_ID_ARRAY.write().unwrap();
    if table.is_empty() {
        *table = elements(a_container)
            .iter()
            .map(|element| ordinal(Some(element.clone())) as u32)
            .collect();
        initialize_ref_id_array(table.clone());
    }
}

/// Port of `GetSaveRefID`: the FormID at `a_index - 1`, 0 when out of range.
pub fn get_save_ref_id(a_index: u32) -> u32 {
    let table = SAVE_REF_ID_ARRAY.read().unwrap();
    if a_index > 0 && (a_index as usize) < table.len() {
        table[a_index as usize - 1]
    } else {
        0
    }
}

/// Port of `QueryCountForVMArrayHandle`: the count of the array, 0 when the
/// handle is not known.
pub fn query_count_for_vm_array_handle(an_array_handle: i64) -> i64 {
    ARRAY_HANDLE_TABLE
        .read()
        .unwrap()
        .iter()
        .find(|(handle, _)| *handle == an_array_handle)
        .map_or(0, |(_, count)| (*count).max(0))
}

/// Port of `TwbVMTypeFormaterToString`.
fn vm_type_to_string(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    if a_type == CallbackType::ctToSortKey {
        return int_to_hex64(a_int, 8);
    }
    let table = VM_TYPE_ARRAY.read().unwrap();
    match usize::try_from(a_int).ok().and_then(|index| table.get(index)) {
        Some(name) => format!("[{}] {name}", int_to_hex64(a_int, 8)),
        None => format!("[{}] <no such string>", int_to_hex64(a_int, 8)),
    }
}

/// Port of `ReadObjectName`: the type of the object, from the object table
/// or else the detached object table; the last entry with the handle wins.
fn read_object_name(a_int: i64) -> String {
    let find = |table: &RwLock<Vec<ObjectHandle>>| {
        table
            .read()
            .unwrap()
            .iter()
            .rev()
            .find(|entry| entry.handle == a_int)
            .map(|entry| entry.vm_type)
    };
    let vm_type = find(&OBJECT_HANDLE_TABLE)
        .filter(|vm_type| *vm_type >= 0)
        .or_else(|| find(&OBJECT_DETACHED_HANDLE_TABLE))
        .unwrap_or(-1);
    if vm_type < 0 {
        return String::new();
    }
    let types = VM_TYPE_ARRAY.read().unwrap();
    let name = usize::try_from(vm_type)
        .ok()
        .and_then(|index| types.get(index))
        .cloned()
        .unwrap_or_default();
    format!("[{}] {name}", int_to_hex64(a_int, 8))
}

/// Port of `TwbHandleFormaterToString`.
fn handle_to_string(a_int: i64, _a_element: ElementArg, _a_type: CallbackType) -> String {
    int_to_hex64(a_int, 16)
}

/// Port of `TwbObjectHandleFormaterToString`.
fn object_handle_to_string(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    if a_type == CallbackType::ctToSortKey {
        return int_to_hex64(a_int, 8);
    }
    let result = read_object_name(a_int);
    if !result.is_empty() {
        result
    } else if a_int == 0 {
        format!("[{}] [empty]", int_to_hex64(a_int, 8))
    } else {
        format!("[{}] <no such object>", int_to_hex64(a_int, 8))
    }
}

/// Port of `TwbVMArrayHandleFormaterToString`.
fn array_handle_to_string(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    if a_type == CallbackType::ctToSortKey {
        return int_to_hex64(a_int, 8);
    }
    format!(
        "[{}] Count = {}",
        int_to_hex64(a_int, 8),
        query_count_for_vm_array_handle(a_int)
    )
}

/// Port of `TwbSaveWorldspaceIndexFormaterToString`.
fn worldspace_index_to_string(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    if a_type == CallbackType::ctToSortKey {
        return int_to_hex64(a_int, 8);
    }
    let element = SAVE_WORLDSPACE_ARRAY
        .read()
        .unwrap()
        .get((a_int - 1).max(0) as usize)
        .cloned();
    if let (true, Some(element)) = (a_int > 0, element) {
        format!("[{}] {}", int_to_hex64(a_int, 8), element.get_value())
    } else {
        format!("[{}] <no such worldspace>", int_to_hex64(a_int, 8))
    }
}

/// Upstream `wbVMType`.
pub fn wb_vm_type() -> Option<Arc<dyn IntegerDefFormater>> {
    wb_callback(Some(Arc::new(vm_type_to_string)), None)
}

/// Upstream `wbVMHandle`.
pub fn wb_vm_handle() -> Option<Arc<dyn IntegerDefFormater>> {
    wb_callback(Some(Arc::new(handle_to_string)), None)
}

/// Upstream `wbVMObjectHandle`.
pub fn wb_vm_object_handle() -> Option<Arc<dyn IntegerDefFormater>> {
    wb_callback(Some(Arc::new(object_handle_to_string)), None)
}

/// Upstream `wbVMArrayHandle`.
pub fn wb_vm_array_handle() -> Option<Arc<dyn IntegerDefFormater>> {
    wb_callback(Some(Arc::new(array_handle_to_string)), None)
}

/// Upstream `wbSaveWorldspaceIndex`.
pub fn wb_save_worldspace_index() -> Option<Arc<dyn IntegerDefFormater>> {
    wb_callback(Some(Arc::new(worldspace_index_to_string)), None)
}

/// Port of `wbFindSaveElement`: the first element up the tree whose base
/// name is `a_name`, else the first element with that base name below the
/// element, else the element itself.
pub fn wb_find_save_element(a_name: &str, a_element: &ElementRef) -> ElementRef {
    fn find_below(name: &str, container: &ElementRef) -> Option<ElementRef> {
        for element in elements(container) {
            if element.get_base_name().eq_ignore_ascii_case(name) {
                return Some(element);
            }
            if element.as_container().is_some()
                && let Some(found) = find_below(name, &element)
            {
                return Some(found);
            }
        }
        None
    }
    let mut result = a_element.clone();
    while !result.get_base_name().eq_ignore_ascii_case(a_name) {
        match result.get_container() {
            Some(container) => result = container,
            None => break,
        }
    }
    if result.get_base_name().eq_ignore_ascii_case(a_name) {
        return result;
    }
    find_below(a_name, a_element).unwrap_or_else(|| a_element.clone())
}

/// Port of `wbDontShowBranch`.
pub fn wb_dont_show_branch(_a_element: ElementArg) -> bool {
    hide_never_show()
}

/// The ordinal of the element called `name` in the save element `found`.
fn named_ordinal(found: &ElementRef, name: &str) -> Option<i64> {
    let container = found.as_data_container()?;
    container
        .get_element_by_name(name)
        .map(|element| ordinal(Some(element)))
}

/// Port of `wbCoSaveChapterOtherCounter`: the length of the chunk.
pub fn wb_co_save_chapter_other_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    named_ordinal(&wb_find_save_element("Chunk", element), "Length").unwrap_or(0) as u32
}

/// Port of `wbCoSavePluginCounter`: the plugin count of the co-save header.
pub fn wb_co_save_plugin_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(file) = a_element.and_then(|element| element.get_file()) else {
        return 0;
    };
    let Some(header) = file
        .as_container()
        .and_then(|file| file.get_element_by_name("CoSave File Header"))
    else {
        return 0;
    };
    named_ordinal(&header, "Plugins count").unwrap_or(0) as u32
}

/// Port of `wbCoSaveChunkCounter`: the chunk count of the plugin.
pub fn wb_co_save_chunk_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    named_ordinal(&wb_find_save_element("Plugin", element), "Chunks count").unwrap_or(0) as u32
}

/// The first four bytes of the data.
fn cardinal(a_base_ptr: DataPtr) -> u32 {
    a_base_ptr
        .and_then(|data| data.get(..4))
        .map_or(0, |bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
}

/// Port of `wbCoSaveChunkType`.
pub fn wb_co_save_chunk_type(a_base_ptr: DataPtr, _a_element: ElementArg) -> i32 {
    match a_base_ptr {
        Some(_) => cardinal(a_base_ptr) as i32,
        None => -1,
    }
}

/// Port of `wbCoSaveChunkTypeName`.
pub fn wb_co_save_chunk_type_name(a_base_ptr: DataPtr, _a_element: ElementArg) -> String {
    match a_base_ptr {
        Some(_) => str4_to_string(i64::from(cardinal(a_base_ptr))),
        None => String::new(),
    }
}

/// Port of `wbCoSaveArrayKeyElementDecider`: 1 for a numeric key, else 2.
pub fn wb_co_save_array_key_element_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match named_ordinal(&wb_find_save_element("Array_var", element), "Key Type") {
        Some(1) => 1,
        Some(_) => 2,
        None => 0,
    }
}

/// Port of `wbCoSaveArrayDataElementDecider`: the data type of the element.
pub fn wb_co_save_array_data_element_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    match named_ordinal(&wb_find_save_element("Element", element), "Data Type") {
        Some(kind @ 1..=4) => kind as i32,
        _ => 0,
    }
}

/// Port of `wbCoSaveArrayType`.
pub fn wb_co_save_array_type(a_base_ptr: DataPtr, _a_element: ElementArg) -> i32 {
    match a_base_ptr {
        Some(_) => cardinal(a_base_ptr) as i32,
        None => -1,
    }
}

/// Port of `wbCoSaveArrayTypeName`.
pub fn wb_co_save_array_type_name(a_base_ptr: DataPtr, _a_element: ElementArg) -> String {
    match a_base_ptr {
        Some(_) => int_to_hex64(i64::from(cardinal(a_base_ptr)), 8),
        None => String::new(),
    }
}

/// Upstream `wbCoSaveArrayTypeEnum`, which the initialization of the unit
/// creates once.
pub struct CoSaveArrayTypeEnum(std::sync::OnceLock<Option<Arc<EnumDef>>>);

impl CoSaveArrayTypeEnum {
    pub fn get(&self) -> Option<Arc<EnumDef>> {
        self.0
            .get_or_init(|| wb_enum(&["Invalid", "Numeric", "Form", "String", "Array"]))
            .clone()
    }
}

/// Upstream `wbCoSaveArrayTypeEnum`.
pub static WB_CO_SAVE_ARRAY_TYPE_ENUM: CoSaveArrayTypeEnum = CoSaveArrayTypeEnum(std::sync::OnceLock::new());

/// Port of `ToBeDeterminedDecider`.
pub fn to_be_determined_decider(_a_base_ptr: DataPtr, _a_element: ElementArg) -> i32 {
    0
}

/// Port of `ToBeDeterminedCounter`.
pub fn to_be_determined_counter(_a_base_ptr: DataPtr, _a_element: ElementArg) -> u32 {
    0
}

/// Port of `ToBeDeterminedCountCallback`.
pub fn to_be_determined_count_callback(_a_base_ptr: DataPtr, _a_element: ElementArg) -> u32 {
    0
}
