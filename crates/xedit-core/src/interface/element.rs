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
use super::misc::Variant;
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
    fn as_main_record(&self) -> Option<&dyn MainRecord> {
        None
    }

    fn get_edit_value(&self) -> String;

    fn get_links_to(&self) -> Option<ElementRef>;

    fn get_element_type(&self) -> ElementType;

    fn get_def(&self) -> Option<Arc<dyn NamedDef>>;

    fn get_value_def(&self) -> Option<Arc<dyn ValueDef>>;

    fn get_native_value(&self) -> Variant;

    /// The element that contains this one.
    fn get_container(&self) -> Option<ElementRef>;

    /// Upstream `_File`: the file that contains this element.
    fn get_file(&self) -> Option<FileRef>;

    fn get_containing_main_record(&self) -> Option<MainRecordRef>;

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
}

/// Upstream `IwbFile`.
pub trait File: Container {
    /// Upstream `Encoding[aTranslatable]`: the encoding of the strings of the file.
    fn get_encoding(&self, translatable: bool) -> Encoding;

    fn get_is_localized(&self) -> bool;

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

    /// The override of this record in the last file that has one.
    fn get_winning_override(&self) -> MainRecordRef;

    /// The override of this record that the file `file` sees: the one in the
    /// file itself or in the last of its masters that has one.
    fn get_highest_override_visible_for_file(&self, file: &FileRef) -> Option<MainRecordRef>;
}

/// Upstream `IwbContainer`, with the methods of `IwbContainerBase`.
pub trait Container: Element {
    /// Upstream `ElementNativeValues[aPath]`: the native value of the element
    /// at `path`, or an empty variant when there is none.
    fn get_element_native_value(&self, path: &str) -> Variant;

    /// Upstream `ElementByName[aName]`.
    fn get_element_by_name(&self, name: &str) -> Option<ElementRef>;

    fn get_element_count(&self) -> i32;

    /// Upstream `Elements[aIndex]`.
    fn get_element(&self, index: i32) -> Option<ElementRef>;

    /// Upstream `ElementBySortOrder[aSortOrder]`.
    fn get_element_by_sort_order(&self, sort_order: i32) -> Option<ElementRef>;

    /// Upstream `AnyElement`: one of the elements, or `None` for an empty container.
    fn get_any_element(&self) -> Option<ElementRef>;

    /// Number of elements before the ones of the definition, such as a record header.
    fn get_additional_element_count(&self) -> i32;
}

/// Upstream `IwbDataContainer`: a container that owns a range of data.
pub trait DataContainer: Container {
    /// The bytes from upstream `DataBasePtr` up to `DataEndPtr`.
    fn get_data(&self) -> DataPtr<'_>;
}
