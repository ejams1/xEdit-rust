// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! The element interfaces (`IwbElement` and its descendants).
//!
//! The traits grow with the port: a method is added when the first ported
//! caller needs it. The implementations are in the port of
//! `wbImplementation.pas`.

use std::sync::{Arc, RwLock};

use xedit_io::Encoding;

use super::def::{NamedDef, ValueDef};
use super::form_id::{FileID, FormID};
use super::misc::{EditError, Variant};
use super::types::{ConflictPriority, ElementType, FileState, FileStates, Signature, TriBool};

/// A reference to an element, upstream `IwbElement`.
pub type ElementRef = Arc<dyn Element>;

/// An element argument that upstream allows to be `nil`.
pub type ElementArg<'a> = Option<&'a ElementRef>;

/// A reference to a file, upstream `IwbFile`.
pub type FileRef = Arc<dyn File>;

/// A reference to a main record, upstream `IwbMainRecord`.
pub type MainRecordRef = Arc<dyn MainRecord>;

/// The data of an element: the bytes from upstream `aBasePtr` up to `aEndPtr`.
/// `None` is a `nil` base pointer.
pub type DataPtr<'a> = Option<&'a [u8]>;

/// Upstream `IwbElement`.
pub trait Element: Send + Sync {
    /// The identity of the element object, upstream `ElementID`.
    fn get_element_id(&self) -> usize;

    fn get_name(&self) -> String;

    fn get_full_path(&self) -> String;

    /// Number of bytes of data that the element owns.
    fn get_data_size(&self) -> i32;

    /// Whether the masters of the file changed and the element still stores
    /// FormIDs for the old list of masters.
    fn get_masters_updated(&self) -> bool;

    /// Records that this element refers to the record `form_id`.
    fn add_referenced_from_id(&self, form_id: FormID);

    /// `Supports(element, IwbMainRecord)`.
    fn into_main_record(self: Arc<Self>) -> Option<MainRecordRef> {
        None
    }

    fn as_main_record(&self) -> Option<&dyn MainRecord> {
        None
    }

    fn get_edit_value(&self) -> String;

    /// Upstream `IwbRecord.Signature`: the signature of a record or
    /// subrecord, `None` for the other elements.
    fn get_record_signature(&self) -> Option<Signature> {
        None
    }

    /// Upstream `BaseName`: the name without the suffix of an array element.
    fn get_base_name(&self) -> String {
        self.get_name()
    }

    /// Upstream `Skipped`: the contents of the element are not read, such
    /// as a subrecord on the ignore list of its record.
    fn get_skipped(&self) -> bool {
        false
    }

    /// Upstream `IwbHasSignature.Signature`: the signature of a record or
    /// subrecord, or of the first record of a subrecord structure or array
    /// (`NONE` without one). `None` for the elements without a signature.
    fn get_has_signature(&self) -> Option<Signature> {
        self.get_record_signature()
    }

    /// Upstream `Value`: the value as the dump shows it. Empty for an
    /// element without a value.
    fn get_value(&self) -> String {
        String::new()
    }

    /// Upstream `DisplayName[aUseSuffix]`.
    fn get_display_name(&self, _use_suffix: bool) -> String {
        self.get_name()
    }

    /// Upstream `Summary`: the short form of the value that the definition
    /// builds. Empty for an element without one.
    fn get_summary(&self) -> String {
        String::new()
    }

    fn get_links_to(&self) -> Option<ElementRef>;

    fn get_element_type(&self) -> ElementType;

    fn get_def(&self) -> Option<Arc<dyn NamedDef>>;

    fn get_value_def(&self) -> Option<Arc<dyn ValueDef>>;

    fn get_native_value(&self) -> Variant;

    /// Upstream `SetEditValue`. The error is the message of the exception
    /// upstream raises.
    fn set_edit_value(&self, _value: &str) -> Result<(), EditError> {
        Err(format!("{} can not be edited.", self.get_name()))
    }

    /// Upstream `SetNativeValue`.
    fn set_native_value(&self, _value: Variant) -> Result<(), EditError> {
        Err(format!("{} can not be edited.", self.get_name()))
    }

    /// Upstream `SetToDefault`: the element takes the default value of its
    /// definition.
    fn set_to_default(&self) -> Result<(), EditError> {
        Ok(())
    }

    /// Upstream `IsEditable`.
    fn get_is_editable(&self) -> bool {
        super::globals::is_internal_edit()
    }

    /// Upstream `Remove`: the element leaves its container.
    fn remove(&self) {}

    /// Upstream `SortOrder`.
    fn get_sort_order(&self) -> i32 {
        i32::MAX
    }

    /// Upstream `SortOrder := aValue`.
    fn set_sort_order(&self, _order: i32) {}

    /// Upstream `BeginUpdate`: change notifications wait for `end_update`.
    fn begin_update(&self) {}

    /// Upstream `EndUpdate`.
    fn end_update(&self) {}

    /// Upstream `DataSize := aValue` (`SetDataSize`): the data of the
    /// element is resized, and its elements are built again over it.
    fn set_data_size(&self, _size: i32) -> Result<(), EditError> {
        Err(format!("{} can not be resized.", self.get_name()))
    }

    /// Upstream `SortKey[aExtended]`: the key the sorted containers order
    /// their elements by.
    fn get_sort_key(&self, _extended: bool) -> String {
        String::new()
    }

    /// Upstream `MarkModifiedRecursive(AllElementTypes)`.
    fn mark_modified_recursive(&self) {}

    /// Upstream `Assign(Low(Integer), aSource, False)` (`wbAssignThis`) with
    /// a source element: the element takes the value of the source, member
    /// by member for a container. The default of `TwbDef.Assign` is the
    /// edit value of the source.
    fn assign_from(&self, source: &ElementRef) -> Result<(), EditError> {
        self.set_edit_value(&source.get_edit_value())
    }

    /// Upstream `RequestStorageChange(aBasePtr, aEndPtr, aNewSize)`: makes
    /// the element the owner of its data and marks it modified. Returns the
    /// data as a buffer of `new_size` bytes (the old bytes, cut or extended
    /// with zeros) for the caller to fill and hand back to `commit_storage`.
    /// `None` for an element without data of its own.
    fn request_storage_change(&self, _new_size: usize) -> Option<Vec<u8>> {
        None
    }

    /// Stores the buffer of `request_storage_change` as the data of the element.
    fn commit_storage(&self, _bytes: Vec<u8>) {}

    /// The element that contains this one.
    fn get_container(&self) -> Option<ElementRef>;

    /// Upstream `MemoryOrder`: the position of the element in the data of
    /// its container.
    fn get_memory_order(&self) -> i32;

    /// Upstream `_File`: the file that contains this element.
    fn get_file(&self) -> Option<FileRef>;

    fn get_containing_main_record(&self) -> Option<MainRecordRef>;

    /// Upstream `ContainingSubRecord`: the subrecord that contains this
    /// element, or the element itself when it is one.
    fn get_containing_sub_record(&self) -> Option<ElementRef> {
        let mut current = self.get_container();
        while let Some(element) = current {
            if element.get_element_type() == ElementType::etSubRecord {
                return Some(element);
            }
            current = element.get_container();
        }
        None
    }

    /// Upstream `IwbSubRecord.SubRecordHeaderSize`: the data size from the
    /// header of a subrecord, `None` for the other elements.
    fn get_sub_record_header_size(&self) -> Option<i32> {
        None
    }

    fn get_path(&self) -> String;

    /// Whether the element stores an ID into the string tables.
    fn get_localized(&self) -> TriBool;

    fn get_conflict_priority(&self) -> ConflictPriority;

    fn get_dont_show(&self) -> bool;

    /// `Supports(element, IwbDataContainer)`.
    fn as_data_container(&self) -> Option<&dyn DataContainer> {
        None
    }

    /// `Supports(element, IwbContainer)`.
    fn as_container(&self) -> Option<&dyn Container> {
        None
    }

    /// The object of `wbImplementation`, for the casts between its types.
    fn as_element_impl(&self) -> Option<&dyn crate::implementation::ElementImpl> {
        None
    }
}

/// Upstream `IwbFile`.
pub trait File: Container {
    /// Upstream `Encoding[aTranslatable]`: the encoding of the strings of the file.
    fn get_encoding(&self, translatable: bool) -> Encoding;

    /// Upstream `RecordFromIndexByKey[aIndex, aKey]`: the record with the
    /// key in the named index, in this file or one of its masters.
    fn get_record_from_index_by_key(&self, index: i32, key: &str) -> Option<MainRecordRef>;

    /// Upstream `RecordByEditorID[aEditorID]`: the record with the editor ID
    /// in this file, else in its masters from the last one down.
    fn get_record_by_editor_id(&self, editor_id: &str) -> Option<MainRecordRef>;

    /// Upstream `LoadOrder`.
    fn get_load_order(&self) -> i32;

    fn get_is_localized(&self) -> bool;

    /// Upstream `IsESM`: the ESM flag of the file header.
    fn get_is_esm(&self) -> bool;

    /// Upstream `RecordCount`: the number of main records in the file.
    fn get_record_count(&self) -> i32;

    fn get_file_states(&self) -> FileStates;

    /// Upstream `LoadOrderFileID`: the slot of the file in the load order.
    fn get_load_order_file_id(&self) -> FileID;

    /// Upstream `MasterCount[aNew]`.
    fn get_master_count(&self, new: bool) -> i32;

    /// Upstream `Masters[aIndex, aNew]`.
    fn get_master(&self, index: i32, new: bool) -> Option<FileRef>;

    /// Whether the file may define records with object IDs below $800.
    fn get_allow_hardcoded_range_use(&self) -> bool;

    /// Upstream `RecordByFormID[aFormID, aAllowInjected, aNewMasters]`: the
    /// record that a FormID of this file refers to. The error is the message
    /// of the exception that upstream raises for a FormID that the file cannot
    /// hold.
    fn get_record_by_form_id(
        &self,
        form_id: FormID,
        allow_injected: bool,
        new_masters: bool,
    ) -> Result<Option<MainRecordRef>, String>;

    /// Converts a FormID of this file to the FormID in the load order.
    fn file_form_id_to_load_order_form_id(&self, form_id: FormID, new: bool) -> Result<FormID, String>;

    /// Upstream `LoadOrderFormIDtoFileFormID`: the FormID as this file
    /// stores it. The error is the message of the upstream exception for a
    /// FormID of a file that is not a master of this one.
    fn load_order_form_id_to_file_form_id(&self, form_id: FormID, new: bool) -> Result<FormID, String>;
}

static FILES: RwLock<Vec<FileRef>> = RwLock::new(Vec::new());

/// The loaded files, upstream `Files`.
pub fn files() -> Vec<FileRef> {
    FILES.read().unwrap().clone()
}

/// Adds a file to upstream `Files`.
pub fn add_file(file: FileRef) {
    FILES.write().unwrap().push(file);
}

/// Empties upstream `Files`.
pub fn clear_files() {
    FILES.write().unwrap().clear();
}

/// Port of `wbGetGameMasterFile`.
pub fn get_game_master_file() -> Option<FileRef> {
    let files = FILES.read().unwrap();
    files
        .iter()
        .find(|file| file.get_file_states().contains(FileState::fsIsGameMaster))
        .or_else(|| {
            files.iter().find(|file| {
                let file_id = file.get_load_order_file_id();
                file_id.is_full_slot() && file_id.full_slot() == 0
            })
        })
        .cloned()
}

/// Port of `wbRecordByLoadOrderFormID`: the record with this load order
/// FormID, as the file `seen_from_file` sees it.
pub fn record_by_load_order_form_id(form_id: FormID, seen_from_file: Option<&FileRef>) -> Option<MainRecordRef> {
    let file_id = form_id.file_id();
    let file = FILES
        .read()
        .unwrap()
        .iter()
        .find(|file| file.get_load_order_file_id() == file_id)
        .cloned()?;
    // Upstream lets an exception of the lookup pass to the caller.
    let result = file.get_record_by_form_id(form_id, true, false).ok().flatten()?;
    match seen_from_file.and_then(|seen_from| result.get_highest_override_visible_for_file(seen_from)) {
        Some(visible) => Some(visible),
        None => Some(result),
    }
}

/// Upstream `IwbMainRecord`.
pub trait MainRecord: Container {
    fn get_load_order_form_id(&self) -> FormID;

    fn get_signature(&self) -> Signature;

    fn get_editor_id(&self) -> String;

    /// Upstream `ShortName`: the editor ID or the FormID of the record.
    fn get_short_name(&self) -> String;

    fn get_is_partial_form(&self) -> bool;

    fn get_is_persistent(&self) -> bool;

    /// Upstream `Version`: the form version in the record header.
    fn get_version(&self) -> u32;

    fn get_is_deleted(&self) -> bool;

    /// Upstream `MasterOrSelf`: the record this one overrides, or itself.
    fn get_master_or_self(&self) -> MainRecordRef;

    /// Upstream `Flags`: the flags of the record header.
    fn get_flags(&self) -> crate::implementation::structs::MainRecordStructFlags;

    /// Upstream `FormID`: the FormID as the file stores it.
    fn get_form_id(&self) -> FormID;

    /// Upstream `FixedFormID`.
    fn get_fixed_form_id(&self) -> FormID;

    /// Upstream `HasPrecombinedMesh`: whether a placed record of Fallout 4
    /// is part of a precombined mesh of its cell.
    fn get_has_precombined_mesh(&self) -> bool;

    /// Upstream `PrecombinedMesh`: the file of that mesh.
    fn get_precombined_mesh(&self) -> String;

    /// Upstream `CanBePartial`: whether the record may be a partial form.
    fn get_can_be_partial(&self) -> bool;

    /// Upstream `GetGridCell`: the grid position of an exterior cell.
    fn get_grid_cell(&self) -> Option<(i32, i32)>;

    /// The override of this record in the last file that has one.
    fn get_winning_override(&self) -> MainRecordRef;

    /// The override of this record that the file `file` sees: the one in the
    /// file itself or in the last of its masters that has one.
    fn get_highest_override_visible_for_file(&self, file: &FileRef) -> Option<MainRecordRef>;

    /// Upstream `HighestOverrideOrSelf[aMaxLoadOrder]`: the last override
    /// that is not a partial form in a file at or before the load order, or
    /// the record itself.
    fn get_highest_override_or_self(&self, max_load_order: i32) -> MainRecordRef;

    /// Upstream `BaseRecord`: the record the base record subrecord of a
    /// placed record links to.
    fn get_base_record(&self) -> Option<MainRecordRef>;

    /// Upstream `IsMaster`: the record overrides no record of a master.
    fn get_is_master(&self) -> bool {
        true
    }

    /// Upstream `IsCompressed := aValue`.
    fn set_is_compressed(&self, _value: bool) {}
}

/// Upstream `IwbContainer`, with the methods of `IwbContainerBase`.
pub trait Container: Element {
    /// Upstream `ElementNativeValues[aPath]`: the native value of the element
    /// at `path`, or an empty variant when there is none. The last name of
    /// the path may be a flag of a flags value, as `GetMemberNativeValue`
    /// reads it.
    fn get_element_native_value(&self, path: &str) -> Variant {
        match self.get_element_by_path(path) {
            Some(element) => element.get_native_value(),
            None => match self.member_flag_value(path) {
                Some(set) => Variant::Bool(set),
                None => Variant::Empty,
            },
        }
    }

    /// Upstream `ElementEditValues[aPath]`, with a flag of a flags value as
    /// the last name like `GetMemberEditValue`.
    fn get_element_edit_value(&self, path: &str) -> String {
        match self.get_element_by_path(path) {
            Some(element) => element.get_edit_value(),
            None => match self.member_flag_value(path) {
                Some(set) => if set { "1" } else { "0" }.to_owned(),
                None => String::new(),
            },
        }
    }

    /// Port of `GetMemberEditValue` and `GetMemberNativeValue` for a path:
    /// when the container at the path without its last name is a flags
    /// value, whether the flag with that name is set.
    fn member_flag_value(&self, path: &str) -> Option<bool> {
        let (container, name): (Option<ElementRef>, &str) = match path.rsplit_once('\\') {
            Some((container_path, name)) => (self.get_element_by_path(container_path), name),
            None => (None, path),
        };
        let (value_def, edit_value) = match &container {
            Some(container) => (container.get_value_def()?, container.get_edit_value()),
            None => (self.get_value_def()?, self.get_edit_value()),
        };
        let integer_def = value_def.as_integer_def()?;
        let formater = integer_def.get_formater(container.as_ref())?;
        let flag_def = formater.as_flags_def()?.find_flag(name)?;
        let index = flag_def.get_flag_index() as usize;
        Some(edit_value.as_bytes().get(index) == Some(&b'1'))
    }

    /// Upstream `ElementEditValues[aPath] := aValue` (`SetElementEditValue`):
    /// the element at the path takes the value; a missing last element is
    /// added (`SetMemberEditValue`), and a flag of a flags value is set by
    /// its name.
    fn set_element_edit_value(&self, path: &str, value: &str) -> Result<(), EditError> {
        let (name, rest) = match path.split_once('\\') {
            Some((name, rest)) => (name, Some(rest)),
            None => (path, None),
        };
        let element = match name {
            "." => self.as_container_ref(),
            ".." => self.get_container(),
            _ => self.get_element_by_name(name),
        };
        match (element, rest) {
            (None, None) => self.set_member_edit_value(name, value),
            (None, Some(_)) => Ok(()),
            (Some(element), None) => element.set_edit_value(value),
            (Some(element), Some(rest)) => match element.as_container() {
                Some(container) => container.set_element_edit_value(rest, value),
                None => Ok(()),
            },
        }
    }

    /// Upstream `ElementNativeValues[aPath] := aValue` (`SetElementNativeValue`).
    fn set_element_native_value(&self, path: &str, value: Variant) -> Result<(), EditError> {
        let (name, rest) = match path.split_once('\\') {
            Some((name, rest)) => (name, Some(rest)),
            None => (path, None),
        };
        let element = match name {
            "." => self.as_container_ref(),
            ".." => self.get_container(),
            _ => self.get_element_by_name(name),
        };
        match (element, rest) {
            (None, None) => self.set_member_native_value(name, value),
            (None, Some(_)) => Ok(()),
            (Some(element), None) => element.set_native_value(value),
            (Some(element), Some(rest)) => match element.as_container() {
                Some(container) => container.set_element_native_value(rest, value),
                None => Ok(()),
            },
        }
    }

    /// This container as an element reference, for the `.` path.
    fn as_container_ref(&self) -> Option<ElementRef> {
        None
    }

    /// Port of `SetMemberEditValue`: a flag of a flags value by its name,
    /// else the member added by its name.
    fn set_member_edit_value(&self, name: &str, value: &str) -> Result<(), EditError> {
        if let Some(flags) = self.flag_edit_value(name, value == "1") {
            return self.set_edit_value(&flags);
        }
        if let Some(element) = self.add(name, true)? {
            element.set_edit_value(value)?;
        }
        Ok(())
    }

    /// Port of `SetMemberNativeValue`.
    fn set_member_native_value(&self, name: &str, value: Variant) -> Result<(), EditError> {
        if let Some(flags) = self.flag_edit_value(name, matches!(value, Variant::Bool(true))) {
            return self.set_edit_value(&flags);
        }
        if let Some(element) = self.add(name, true)? {
            // UPSTREAM-QUIRK: the native value is assigned as an edit value.
            element.set_edit_value(&super::misc::variant_to_string(&value))?;
        }
        Ok(())
    }

    /// The edit value of this flags value with the flag `name` set or
    /// cleared, when the value is a flags value with that flag.
    fn flag_edit_value(&self, name: &str, set: bool) -> Option<String> {
        let value_def = self.get_value_def()?;
        let integer_def = value_def.as_integer_def()?;
        let formater = integer_def.get_formater(self.as_container_ref().as_ref())?;
        let flag_def = formater.as_flags_def()?.find_flag(name)?;
        let index = usize::try_from(flag_def.get_flag_index()).ok()?;
        let mut flags: Vec<u8> = self.get_edit_value().into_bytes();
        flags.resize(64.max(flags.len()), b'0');
        if index >= flags.len() {
            return None;
        }
        flags[index] = if set { b'1' } else { b'0' };
        Some(String::from_utf8(flags).unwrap_or_default())
    }

    /// Upstream `Add(aName, aSilent)`: adds the member element with the name
    /// or signature to the container, or returns the one that exists.
    fn add(&self, _name: &str, _silent: bool) -> Result<Option<ElementRef>, EditError> {
        Ok(None)
    }

    /// Upstream `RemoveElement(aName)`: removes the element the name resolves
    /// to (`ResolveElementName`: a name, a path or a signature) and returns it.
    fn remove_element_by_name(&self, name: &str) -> Option<ElementRef> {
        let element = self.get_element_by_path(name)?;
        element.remove();
        Some(element)
    }

    /// Upstream `RemoveElement(aPos, aMarkModified)`: removes the element at
    /// the position and returns it.
    fn remove_element_at(&self, _index: i32, _mark_modified: bool) -> Option<ElementRef> {
        None
    }

    /// Upstream `ReverseElements`.
    fn reverse_elements(&self) {}

    /// Upstream `SortBySortOrder`.
    fn sort_by_sort_order(&self) {}

    /// Upstream `ElementByMemoryOrder[aOrder]`.
    fn get_element_by_memory_order(&self, order: i32) -> Option<ElementRef> {
        (0..self.get_element_count())
            .filter_map(|index| self.get_element(index))
            .find(|element| element.get_memory_order() == order)
    }

    /// Upstream `ElementLinksTo[aPath]`.
    fn get_element_links_to(&self, path: &str) -> Option<ElementRef> {
        self.get_element_by_path(path)?.get_links_to()
    }

    /// Upstream `ElementExists[aPath]`.
    fn get_element_exists(&self, path: &str) -> bool {
        self.get_element_by_path(path).is_some()
    }

    /// Upstream `ElementBySignature[aSignature]`: the first element with the
    /// signature, a subrecord structure or array by its first record.
    fn get_element_by_signature(&self, signature: Signature) -> Option<ElementRef> {
        (0..self.get_element_count())
            .filter_map(|index| self.get_element(index))
            .find(|element| element.get_has_signature() == Some(signature))
    }

    /// Upstream `ElementByName[aName]`.
    fn get_element_by_name(&self, name: &str) -> Option<ElementRef>;

    /// Upstream `ElementByPath[aPath]`: names separated by `\`.
    fn get_element_by_path(&self, path: &str) -> Option<ElementRef>;

    fn get_element_count(&self) -> i32;

    /// Upstream `Elements[aIndex]`.
    fn get_element(&self, index: i32) -> Option<ElementRef>;

    /// Upstream `ElementBySortOrder[aSortOrder]`.
    fn get_element_by_sort_order(&self, sort_order: i32) -> Option<ElementRef>;

    /// Upstream `AnyElement`: one of the elements, or `None` for an empty container.
    fn get_any_element(&self) -> Option<ElementRef>;

    /// Number of elements before the ones of the definition, such as a record header.
    fn get_additional_element_count(&self) -> i32;

    /// Upstream `RecordBySignature[aSignature]`: the subrecord with the
    /// signature among the elements.
    fn get_record_by_signature(&self, signature: Signature) -> Option<ElementRef> {
        (0..self.get_element_count())
            .filter_map(|index| self.get_element(index))
            .find(|element| element.get_record_signature() == Some(signature))
    }
}

/// Upstream `IwbDataContainer`: a container that owns a range of data.
pub trait DataContainer: Container {
    /// The bytes from upstream `DataBasePtr` up to `DataEndPtr`.
    fn get_data(&self) -> DataPtr<'_>;

    /// All the bytes the data of the element is part of, for the callbacks
    /// that read before their data pointer as upstream does with pointer
    /// arithmetic. `None` when the element does not keep them.
    fn get_block(&self) -> Option<&[u8]> {
        None
    }
}

/// The bytes at `offset` before the start of `data`, read from the block of
/// `element` that `data` is part of: upstream `PByte(aBasePtr) - offset`.
pub fn bytes_before<'a>(data: DataPtr<'a>, element: &'a ElementRef, offset: usize, len: usize) -> Option<&'a [u8]> {
    let data = data?;
    let block = element.as_data_container()?.get_block()?;
    let start = (data.as_ptr() as usize).checked_sub(block.as_ptr() as usize)?;
    let begin = start.checked_sub(offset)?;
    block.get(begin..begin + len)
}

/// The `len` bytes after the end of `data`, read from the block of `element`
/// that `data` is part of: upstream reads them when a stored length exceeds
/// the data. `None` at the end of the block.
pub fn bytes_after<'a>(data: DataPtr<'a>, element: &'a ElementRef, len: usize) -> Option<&'a [u8]> {
    let data = data?;
    let block = element.as_data_container()?.get_block()?;
    let start = (data.as_ptr() as usize).checked_sub(block.as_ptr() as usize)?;
    let end = start.checked_add(data.len())?;
    block.get(end..end + len)
}
