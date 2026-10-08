// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of `wbImplementation.pas`: the element tree of a loaded plugin.
//!
//! State: the binary scan of a file into its header record, group records
//! and main records, with the record data kept as ranges of the mapped
//! file, and the subrecords of a record grouped by its definition on first
//! use. The write path (`write`) saves a file whose records are unmodified
//! or rebuilt from their elements.

pub mod add;
pub mod assign;
pub mod copy;
pub mod edit;
pub mod file_flags;
pub mod flag;
pub mod form_ids;
mod info_sort;
pub mod masters;
pub mod new_form_id;
pub mod refcache;
pub mod refs;
mod scan;
pub mod sortable;
pub mod structs;
pub mod write;

/// The value accessors of an element without a value.
macro_rules! element_no_values {
    (with_values) => {
        fn get_value_def(&self) -> Option<Arc<dyn ValueDef>> {
            None
        }

        fn get_native_value(&self) -> Variant {
            Variant::Empty
        }

        fn get_edit_value(&self) -> String {
            String::new()
        }

        fn get_links_to(&self) -> Option<ElementRef> {
            None
        }
    };
    (own_values) => {};
}

/// The display name of an element: the name with the suffix.
macro_rules! element_display_name {
    ($base:ident) => {
        fn get_display_name(&self, use_suffix: bool) -> String {
            self.$base().display_name(self.get_name(), use_suffix)
        }
    };
}

macro_rules! element_common {
    ($base:ident) => {
        element_common!($base, with_values);
    };
    ($base:ident, $values:ident) => {
        element_no_values!($values);
        fn get_element_id(&self) -> usize {
            std::ptr::from_ref(self) as *const () as usize
        }

        fn get_masters_updated(&self) -> bool {
            false
        }

        fn add_referenced_from_id(&self, form_id: FormID) {
            $crate::implementation::refs::add_referenced_from_id(form_id)
        }

        fn get_container(&self) -> Option<ElementRef> {
            self.$base().container()
        }

        fn get_memory_order(&self) -> i32 {
            self.$base()
                .e_memory_order
                .load(std::sync::atomic::Ordering::Relaxed)
        }

        fn get_full_path(&self) -> String {
            match self.$base().container() {
                Some(container) => format!("{} \\ {}", container.get_full_path(), self.get_name()),
                None => self.get_name(),
            }
        }

        fn get_path(&self) -> String {
            match self.$base().container() {
                Some(container) => format!("{} \\ {}", container.get_path(), self.get_name()),
                None => self.get_name(),
            }
        }

        fn get_localized(&self) -> TriBool {
            TriBool::tbUnknown
        }

        fn get_conflict_priority(&self) -> ConflictPriority {
            $crate::implementation::element_conflict_priority(self)
        }

        fn get_dont_show(&self) -> bool {
            $crate::implementation::element_dont_show(self)
        }

        fn as_element_impl(&self) -> Option<&dyn ElementImpl> {
            Some(self)
        }

        fn get_sort_order(&self) -> i32 {
            self.$base()
                .e_sort_order
                .load(std::sync::atomic::Ordering::Relaxed)
        }

        fn set_edit_value(&self, value: &str) -> Result<(), EditError> {
            $crate::implementation::ElementImpl::set_edit_value_impl(self, value)
        }

        fn set_native_value(&self, value: Variant) -> Result<(), EditError> {
            $crate::implementation::ElementImpl::set_native_value_impl(self, value)
        }

        fn set_to_default(&self) -> Result<(), EditError> {
            $crate::implementation::edit::set_to_default(self)
        }

        fn get_is_editable(&self) -> bool {
            $crate::implementation::ElementImpl::get_is_editable_impl(self)
        }

        fn remove(&self) {
            $crate::implementation::edit::remove(self)
        }

        fn request_storage_change(&self, new_size: usize) -> Option<Vec<u8>> {
            $crate::implementation::ElementImpl::request_storage_change_impl(self, new_size)
        }

        fn commit_storage(&self, bytes: Vec<u8>) {
            $crate::implementation::ElementImpl::commit_storage_impl(self, bytes)
        }

        fn set_sort_order(&self, order: i32) {
            self.$base()
                .e_sort_order
                .store(order, std::sync::atomic::Ordering::Relaxed);
        }

        fn begin_update(&self) {
            $crate::implementation::edit::begin_update(self)
        }

        fn end_update(&self) {
            $crate::implementation::edit::end_update(self)
        }

        fn set_data_size(&self, size: i32) -> Result<(), EditError> {
            $crate::implementation::ElementImpl::set_data_size_impl(self, size)
        }

        fn get_sort_key(&self, extended: bool) -> String {
            $crate::implementation::ElementImpl::get_sort_key_impl(self, extended)
        }

        fn mark_modified_recursive(&self) {
            $crate::implementation::write::mark_modified_recursive(
                self,
                <$crate::interface::types::ElementType as $crate::interface::types::PascalEnum>::ALL,
            )
        }
    };
}

pub mod sub_record;
pub mod value;

use crate::interface::ModuleType;
pub use edit::Storage;
pub use write::{ElementState, ResetModified, SaveError};

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, OnceLock, RwLock, Weak};

use xedit_io::{Encoding, MappedFile};

use crate::delphi::{int_power, path_file_name, round};
use crate::interface::builders::{wb_array_count, wb_len_string};
use crate::interface::constructors::find_record_def;
use crate::interface::def::{Def, NamedDef, ValueDef};
use crate::interface::element::{
    Container, CopyArgs, DataContainer, DataPtr, Element, ElementRef, File, FileRef, MainRecord, MainRecordRef,
};
use crate::interface::form_id::{FileID, FormID};
use crate::interface::globals::{
    GameMode, ToolSource, create_contained_in, data_path, display_load_order_form_id, edit_allowed, extract_info,
    file_chapters, file_header, file_magic, file_plugins, game_exe_name, game_master_esm, game_mode,
    has_added_light_support, header_signature, is_fallout3, is_fallout4, is_fallout76, is_internal_edit,
    is_light_supported, is_medium_supported, is_skyrim, is_starfield, is_update_supported, pseudo_light, pseudo_medium,
    pseudo_update, remove_offset_data, size_of_main_record_struct, tool_source, track_all_editor_id,
    vwd_as_quest_children, wb_get_group_order,
};
use crate::interface::integer::IntegerDefFormater;
use crate::interface::main_record::{
    IndexKeys, MainRecordDef, idx_editor_id, main_record_header, named_index_key, named_index_name,
};
use crate::interface::misc::{EditError, Variant, progress};
use crate::interface::sub_record_group::RecordDef;
use crate::interface::types::{
    ConflictPriority, DefFlag, ElementType, FileState, FileStates, KnownSubRecord, Signature, TriBool,
};
use crate::interface::types::{DefType, PascalEnum};

use self::structs::{GroupRecordStruct, MainRecordStruct, MainRecordStructFlags};

/// The bytes of a plugin: the mapped file, or a buffer given to `wb_file`.
pub enum FileBytes {
    Mapped(MappedFile),
    Owned(Vec<u8>),
}

impl FileBytes {
    pub fn as_slice(&self) -> &[u8] {
        match self {
            FileBytes::Mapped(map) => map,
            FileBytes::Owned(bytes) => bytes,
        }
    }
}

pub use crate::threads::InitOnce;

/// The bytes an element reads its data from: the file, or the decompressed
/// data of a record.
#[derive(Clone)]
pub enum DataBlock {
    File(Arc<FileBytes>),
    Buffer(Arc<Vec<u8>>),
}

impl DataBlock {
    pub fn as_slice(&self) -> &[u8] {
        match self {
            DataBlock::File(bytes) => bytes.as_slice(),
            DataBlock::Buffer(bytes) => bytes,
        }
    }
}

/// Port of `TwbContainer.GetElementByName`: by name, then by display name,
/// ignoring case.
pub(crate) fn element_by_name(container: &dyn Container, name: &str) -> Option<ElementRef> {
    let elements: Vec<ElementRef> = (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .collect();
    elements
        .iter()
        .find(|element| element.get_name().eq_ignore_ascii_case(name))
        .or_else(|| {
            elements
                .iter()
                .find(|element| element.get_display_name(true).eq_ignore_ascii_case(name))
        })
        .cloned()
}

/// Port of `ElementByPath` with `ResolveElementName`: the names separated by
/// `\\`, with `.`, `..`, `...` (this container or any parent), `[n]` for the
/// element at a position, and a name of four characters that is also tried
/// as a signature.
pub(crate) fn element_by_path(container: &dyn Container, path: &str) -> Option<ElementRef> {
    let (first, rest) = match path.split_once('\\') {
        Some((first, rest)) => (first, Some(rest)),
        None => (path, None),
    };
    if first == "." {
        return match rest {
            Some(rest) => container.get_element_by_path(rest),
            None => None,
        };
    }
    if first == "..." {
        return element_by_path_from_any_parent(container, rest.unwrap_or(""));
    }
    let element = if first == ".." {
        container.get_container()
    } else if let Some(index) = first.strip_prefix('[').and_then(|index| index.strip_suffix(']')) {
        container.get_element(index.parse().unwrap_or(0))
    } else {
        container.get_element_by_name(first).or_else(|| {
            let bytes: [u8; 4] = first.as_bytes().try_into().ok()?;
            container.get_record_by_signature(Signature::new(&bytes))
        })
    };
    match rest {
        Some(rest) => element?.as_container()?.get_element_by_path(rest),
        None => element,
    }
}

/// Port of the `...` case of `ResolveElementName`: the rest of the path is
/// looked up from this container and from each of its parents in turn; a
/// parent whose name is the next name of the path resolves the rest itself.
fn element_by_path_from_any_parent(container: &dyn Container, rest: &str) -> Option<ElementRef> {
    let (next_name, next_rest) = match rest.split_once('\\') {
        Some((next_name, next_rest)) => (next_name.trim(), Some(next_rest)),
        None => (rest.trim(), None),
    };
    if next_name.is_empty() {
        return None;
    }
    let signature = Signature::from_str(next_name).ok();
    // This container first, then each parent.
    if let Some(found) = container.get_element_by_path(rest) {
        return Some(found);
    }
    if container.get_name().eq_ignore_ascii_case(next_name)
        || container.get_display_name(true).eq_ignore_ascii_case(next_name)
        || (signature.is_some() && container.get_record_signature() == signature)
    {
        return match next_rest {
            Some(next_rest) => container.get_element_by_path(next_rest),
            None => None,
        };
    }
    let mut current = container.get_container();
    while let Some(element) = current {
        let parent = element.as_container()?;
        if let Some(found) = parent.get_element_by_path(rest) {
            return Some(found);
        }
        if element.get_name().eq_ignore_ascii_case(next_name)
            || element.get_display_name(true).eq_ignore_ascii_case(next_name)
            || (signature.is_some() && element.get_record_signature() == signature)
        {
            return match next_rest {
                Some(next_rest) => parent.get_element_by_path(next_rest),
                None => Some(element),
            };
        }
        current = element.get_container();
    }
    None
}

/// Port of `TwbElement.GetConflictPriority` and the override of
/// `TwbDataContainer`: from the value definition, resolved against the data
/// of a data container, else the definition; in the translate mode an
/// element whose definition is not translatable is ignored, and `cpFormID`
/// is resolved for the record.
pub(crate) fn element_conflict_priority(element: &dyn ElementImpl) -> ConflictPriority {
    let self_ref = element.self_element_ref();
    let def: Option<Arc<dyn Def>> = match element.get_value_def() {
        Some(value_def) => match element.as_data_container() {
            Some(data_container) => {
                Some(value::resolve(value_def, data_container.get_data(), self_ref.as_ref()) as Arc<dyn Def>)
            }
            None => Some(value_def as Arc<dyn Def>),
        },
        None => element.get_def().map(|def| def as Arc<dyn Def>),
    };
    let mut result = ConflictPriority::cpNormal;
    if let Some(def) = def {
        result = def.get_conflict_priority(self_ref.as_ref());
        if crate::interface::globals::translation_mode() && !def.def_base().def_flags.contains(DefFlag::dfTranslatable)
        {
            result = ConflictPriority::cpIgnore;
        }
    }
    if result == ConflictPriority::cpFormID {
        result = ConflictPriority::cpCritical;
        if let Some(main_record) = element.get_containing_main_record()
            && matches!(main_record.get_signature().0.as_slice(), b"GMST" | b"DFOB")
        {
            result = ConflictPriority::cpBenign;
        }
    }
    result
}

/// Port of `TwbElement.GetDontShow`: from the value definition, else the
/// definition.
pub(crate) fn element_dont_show(element: &dyn ElementImpl) -> bool {
    let self_ref = element.self_element_ref();
    if let Some(value_def) = element.get_value_def()
        && value_def.get_dont_show(self_ref.as_ref())
    {
        return true;
    }
    element
        .get_def()
        .is_some_and(|def| def.get_dont_show(self_ref.as_ref()))
}

/// The fields of `TwbElement`.
pub struct ElementBase {
    e_container: RwLock<Option<Weak<dyn Element>>>,
    e_sort_order: AtomicI32,
    e_memory_order: AtomicI32,
    /// Port of `eNameSuffix`: `#3` for the elements of an array.
    e_name_suffix: RwLock<String>,
    /// Port of the save states of `eStates`: `esModified`,
    /// `esInternalModified` and `esUnsaved`, as bits of [`ElementState`].
    pub(crate) e_states: std::sync::atomic::AtomicU32,
    /// Port of `eUpdateCount`: while above zero, change notifications and
    /// the modified state of the parent are deferred to `EndUpdate`.
    pub(crate) e_update_count: AtomicI32,
    /// Port of `eReportMastersGen`: the generation of the last
    /// `ReportRequiredMasters`, with the top bit for a recursive one.
    pub(crate) e_report_masters_gen: std::sync::atomic::AtomicU32,
}

impl ElementBase {
    fn new(container: Option<&ElementRef>) -> Self {
        ElementBase {
            e_container: RwLock::new(container.map(Arc::downgrade)),
            // `TwbElement.Create`: unset until the container assigns them.
            e_sort_order: AtomicI32::new(i32::MAX),
            e_memory_order: AtomicI32::new(i32::MIN),
            e_name_suffix: RwLock::new(String::new()),
            e_states: std::sync::atomic::AtomicU32::new(0),
            e_update_count: AtomicI32::new(0),
            e_report_masters_gen: std::sync::atomic::AtomicU32::new(0),
        }
    }

    pub(crate) fn name_suffix(&self) -> String {
        self.e_name_suffix.read().unwrap().clone()
    }

    pub(crate) fn set_name_suffix(&self, suffix: &str) {
        *self.e_name_suffix.write().unwrap() = suffix.to_owned();
    }

    /// Port of `TwbElement.GetDisplayName`: the name with the suffix.
    pub(crate) fn display_name(&self, name: String, use_suffix: bool) -> String {
        let suffix = self.name_suffix();
        if use_suffix && !suffix.is_empty() {
            if name.is_empty() {
                suffix
            } else {
                format!("{name} {suffix}")
            }
        } else {
            name
        }
    }

    fn container(&self) -> Option<ElementRef> {
        self.e_container.read().unwrap().as_ref().and_then(Weak::upgrade)
    }

    /// Port of `SetContainer`.
    pub(crate) fn set_container(&self, container: &ElementRef) {
        *self.e_container.write().unwrap() = Some(Arc::downgrade(container));
    }
}

/// The fields of `TwbContainer`.
#[derive(Default)]
pub struct ContainerBase {
    cnt_elements: RwLock<Vec<ElementRef>>,
    /// Port of `csAsCreatedEmpty`: the one element an array of variable
    /// size was created with, which the first `Assign` takes over.
    pub(crate) cnt_as_created_empty: AtomicBool,
}

impl ContainerBase {
    /// Port of `AddElement`: the element takes its position as its memory
    /// order, which the callbacks that size it may read
    /// (`wbLGDIRankSlotArrayShouldInclude`).
    pub(crate) fn add_element(&self, element: ElementRef) {
        let mut elements = self.cnt_elements.write().unwrap();
        if let Some(element) = element.as_element_impl() {
            element
                .element_base()
                .e_memory_order
                .store(elements.len() as i32, Ordering::Relaxed);
        }
        elements.push(element);
    }

    /// Port of `InsertElement`.
    pub(crate) fn insert_element(&self, index: usize, element: ElementRef) {
        self.cnt_elements.write().unwrap().insert(index, element);
    }

    /// Port of `RemoveElement` by position.
    pub(crate) fn remove_element(&self, index: usize) -> ElementRef {
        self.cnt_elements.write().unwrap().remove(index)
    }

    pub(crate) fn elements(&self) -> Vec<ElementRef> {
        self.cnt_elements.read().unwrap().clone()
    }

    /// The number of elements, without copying the list.
    pub(crate) fn element_count(&self) -> usize {
        self.cnt_elements.read().unwrap().len()
    }

    /// The element at `index`, without copying the list.
    pub(crate) fn element_at(&self, index: usize) -> Option<ElementRef> {
        self.cnt_elements.read().unwrap().get(index).cloned()
    }

    /// Port of `ReleaseElements`: the container gives up its elements, and
    /// with them the element it was created with (`csAsCreatedEmpty`).
    pub(crate) fn release_elements(&self) -> Vec<ElementRef> {
        self.cnt_as_created_empty.store(false, Ordering::Relaxed);
        std::mem::take(&mut *self.cnt_elements.write().unwrap())
    }

    /// Port of `RemoveElement` by element: removes the element itself,
    /// not one that merely compares equal.
    pub(crate) fn remove_element_by_identity(&self, element: &ElementRef) -> Option<ElementRef> {
        let mut elements = self.cnt_elements.write().unwrap();
        let index = elements.iter().position(|candidate| Arc::ptr_eq(candidate, element))?;
        Some(elements.remove(index))
    }

    /// Port of `wbMergeSortPtr` on the elements: a stable sort.
    pub(crate) fn sort_by(&self, compare: impl FnMut(&ElementRef, &ElementRef) -> std::cmp::Ordering) {
        self.cnt_elements.write().unwrap().sort_by(compare);
    }

    /// Port of `ReverseElements`.
    pub(crate) fn reverse(&self) {
        self.cnt_elements.write().unwrap().reverse();
    }

    /// Port of `TwbContainer.GetElementBySortOrder` after the init: the
    /// element whose sort order is `sort_order`, which the caller has
    /// already reduced by the additional element count.
    pub(crate) fn element_by_sort_order(&self, sort_order: i32) -> Option<ElementRef> {
        self.cnt_elements
            .read()
            .unwrap()
            .iter()
            .find(|element| {
                element
                    .as_element_impl()
                    .is_some_and(|element| element.element_base().e_sort_order.load(Ordering::Relaxed) == sort_order)
            })
            .cloned()
    }
}

/// A failure to load a plugin. The message is the upstream exception message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct LoadError(pub String);

/// Port of `TwbFile`.
pub struct FileImpl {
    self_ref: Weak<FileImpl>,
    base: ElementBase,
    container: ContainerBase,
    fl_file_name: String,
    fl_load_order: AtomicI32,
    fl_load_order_file_id: RwLock<FileID>,
    fl_states: RwLock<FileStates>,
    /// The bytes of the file, shared with its records so that they do not
    /// need a strong reference to the file.
    fl_bytes: Arc<FileBytes>,
    /// Port of `flRecords`: the main records, in file order while the
    /// file is scanned and by FormID from `SortRecords` on.
    fl_records: RwLock<Vec<Arc<MainRecordImpl>>>,
    /// Port of `flFormIDsSorted`: `fl_records` is in FormID order
    /// (`SortRecords`), so the lookups may search it and a record added by
    /// an edit goes to its place.
    fl_form_ids_sorted: AtomicBool,

    fl_masters: RwLock<Vec<Arc<FileImpl>>>,
    fl_load_finished: OnceLock<()>,
    /// The header version, read once the header exists: a loaded file does
    /// not change it.
    fl_version: OnceLock<f64>,
    /// Port of `flCompareTo`: the file a compare load takes the load order
    /// of, such as the game master for the hardcoded records.
    fl_compare_to: Option<String>,
    /// Port of `flInjectedRecords`: records of other files with FormIDs of
    /// this file, sorted by FormID.
    fl_injected_records: RwLock<Vec<Arc<MainRecordImpl>>>,
    /// Port of `flRecordsIndices`: the records by key, per named index,
    /// built once the file is scanned (`flIndicesActive`).
    fl_records_indices: RwLock<Vec<HashMap<String, Arc<MainRecordImpl>>>>,
    fl_indices_active: std::sync::atomic::AtomicBool,
    /// Port of `flSetContainsFixedFormID`: the fixed FormIDs of the records
    /// added while the file is scanned, which finds a duplicate anywhere in
    /// the file.
    fl_scanned_form_ids: std::sync::Mutex<std::collections::HashSet<u32>>,
    /// Port of `flCRC32`: the CRC32 of the file as loaded, computed on first
    /// use, replaced by the CRC32 of the bytes of the last save.
    pub(crate) fl_crc32: std::sync::atomic::AtomicU32,
}

static NEXT_LOAD_ORDER: AtomicI32 = AtomicI32::new(0);

/// Port of `FilesMap`: the loaded files by their full path, so that a
/// master loads once.
static FILES_MAP: RwLock<Vec<Arc<FileImpl>>> = RwLock::new(Vec::new());

/// Forgets the loaded files, for the tests and a new session.
pub fn clear_files_map() {
    FILES_MAP.write().unwrap().clear();
    NEXT_LOAD_ORDER.store(0, Ordering::Relaxed);
    reset_load_order_slots();
}

fn find_in_files_map(file_name: &str) -> Option<Arc<FileImpl>> {
    FILES_MAP
        .read()
        .unwrap()
        .iter()
        .find(|file| file.fl_file_name.eq_ignore_ascii_case(file_name))
        .cloned()
}

impl FileImpl {
    fn self_arc(&self) -> Arc<Self> {
        self.self_ref.upgrade().expect("a file is alive while it is used")
    }

    pub fn file_name(&self) -> &str {
        &self.fl_file_name
    }

    pub fn load_order(&self) -> i32 {
        self.fl_load_order.load(Ordering::Relaxed)
    }

    /// The header record (`TES4`).
    pub fn header(&self) -> Option<Arc<MainRecordImpl>> {
        self.container
            .element_at(0)
            .and_then(|element| element.into_main_record_impl())
    }

    /// Port of `GetVersion`: `HEDR\Version` of the header, rounded to two
    /// decimals as `RoundTo` does. Zero without a header.
    pub fn get_version(&self) -> f64 {
        if let Some(version) = self.fl_version.get() {
            return *version;
        }
        let Some(header) = self.header() else { return 0.0 };
        let version = match header.get_element_native_value(r"HEDR\Version") {
            Variant::Float(version) => version,
            Variant::Int(version) => version as f64,
            Variant::UInt(version) => version as f64,
            _ => 0.0,
        };
        let factor = int_power(10.0, -2);
        *self.fl_version.get_or_init(|| round(version / factor) as f64 * factor)
    }

    /// Port of `flRecords`: the main records of the file, in FormID order once
    /// the scan is done (`SortRecords`).
    pub fn records(&self) -> Vec<Arc<MainRecordImpl>> {
        self.fl_records.read().unwrap().clone()
    }

    /// Port of `AddMainRecord`: keeps the record and registers it as the
    /// override of the record of a master with the same FormID. Once the
    /// records are sorted (after the scan) the record goes to its place in
    /// the FormID index, a FormID the file has already is an error, and
    /// the keys of the record go into the named indices.
    pub(crate) fn add_main_record(self: &Arc<Self>, record: Arc<MainRecordImpl>) -> Result<(), String> {
        let form_id = record.get_fixed_form_id();
        // The file header, with the null FormID, is not one of the records.
        if form_id.is_null() {
            return Ok(());
        }
        match self.find_form_id_index(form_id) {
            None => {
                if game_mode() > GameMode::gmTES3
                    && !self.fl_scanned_form_ids.lock().unwrap().insert(form_id.to_cardinal())
                {
                    return Err(format!(
                        "Duplicate FormID [{}] in file {}",
                        form_id.to_string(true),
                        self.get_name()
                    ));
                }
                self.fl_records.write().unwrap().push(record.clone())
            }
            Some(Ok(_)) => {
                return Err(format!(
                    "Duplicate FormID [{}] in file {}",
                    form_id.to_string(true),
                    self.get_name()
                ));
            }
            Some(Err(index)) => self.fl_records.write().unwrap().insert(index, record.clone()),
        }
        self.register_main_record(&record, form_id);
        if self.fl_indices_active.load(Ordering::Acquire) {
            let keys = record.activate_index_keys();
            self.add_keys_to_indices(&record, &keys);
        }
        Ok(())
    }

    /// `AddMainRecord` for a record the scan read: a FormID the file has
    /// already marks the record as a duplicate, which upstream skips.
    fn register_scanned(self: &Arc<Self>, record: &Arc<MainRecordImpl>) {
        if self.add_main_record(record.clone()).is_err() {
            record.mr_duplicate.store(true, Ordering::Relaxed);
            progress(&format!(
                "Skipped Load: Duplicate FormID [{}] in file {}",
                record.get_fixed_form_id().to_string(true),
                self.fl_file_name
            ));
        }
    }

    /// The masters of each module type (`GetFullMasterCount`,
    /// `GetMediumMasterCount`, `GetLightMasterCount`).
    fn master_type_counts(&self) -> (i16, i16, i16) {
        let (mut full, mut medium, mut light) = (0, 0, 0);
        for master in self.fl_masters.read().unwrap().iter() {
            match master.module_type() {
                ModuleType::mtFull => full += 1,
                ModuleType::mtMedium => medium += 1,
                ModuleType::mtLight => light += 1,
            }
        }
        (full, medium, light)
    }

    /// Port of `IsNewRecord`: whether the FileID is the file's own. Under
    /// `wbComplexFileFileID` the slot counts within the masters of its
    /// module type.
    pub(crate) fn is_new_record(&self, file_id: FileID) -> bool {
        if crate::interface::globals::complex_file_file_id() {
            let (full, medium, light) = self.master_type_counts();
            return match file_id.module_type() {
                ModuleType::mtLight => file_id.light_slot() >= light,
                ModuleType::mtMedium => file_id.medium_slot() >= medium,
                ModuleType::mtFull => file_id.full_slot() >= full,
            };
        }
        i32::from(file_id.full_slot()) >= self.master_count()
    }

    /// Port of `GetMasterForFileID`: the master at the slot (of its module
    /// type under `wbComplexFileFileID`), or the last master for a compare
    /// load.
    fn get_master_for_file_id(&self, file_id: FileID) -> Option<Arc<FileImpl>> {
        if crate::interface::globals::complex_file_file_id() {
            let index = self.get_master_index_for_file_id(file_id);
            return usize::try_from(index)
                .ok()
                .and_then(|index| self.fl_masters.read().unwrap().get(index).cloned());
        }
        let masters = self.fl_masters.read().unwrap();
        let slot = file_id.full_slot();
        if slot >= 0 && (slot as usize) < masters.len() {
            return Some(masters[slot as usize].clone());
        }
        if self.get_file_states().contains(FileState::fsIsCompareLoad) {
            return masters.last().cloned();
        }
        None
    }

    /// Port of `InjectMainRecord`: a record of another file with a FormID of
    /// this file, or an override of one that is injected already.
    fn inject_main_record(&self, record: Arc<MainRecordImpl>) {
        let form_id = record.get_fixed_form_id().to_cardinal();
        let mut injected = self.fl_injected_records.write().unwrap();
        match injected.binary_search_by_key(&form_id, |injected| injected.get_fixed_form_id().to_cardinal()) {
            Ok(index) => injected[index].clone().add_override(&record),
            Err(index) => injected.insert(index, record),
        }
    }

    /// Port of `FindInjectedID`.
    fn find_injected_id(&self, form_id: FormID) -> Option<Arc<MainRecordImpl>> {
        let injected = self.fl_injected_records.read().unwrap();
        let index = injected
            .binary_search_by_key(&form_id.to_cardinal(), |record| {
                record.get_fixed_form_id().to_cardinal()
            })
            .ok()?;
        Some(injected[index].clone())
    }

    fn master_count(&self) -> i32 {
        self.fl_masters.read().unwrap().len() as i32
    }

    /// Port of `GetFileFileID`: the FileID of the file as its own records
    /// use it, which is the number of its masters.
    pub fn get_file_file_id(&self) -> FileID {
        if crate::interface::globals::complex_file_file_id() {
            let (full, medium, light) = self.master_type_counts();
            return match self.module_type() {
                ModuleType::mtLight => FileID::create_light(light),
                ModuleType::mtMedium => FileID::create_medium(medium),
                ModuleType::mtFull => FileID::create_full(full),
            };
        }
        FileID::create_full(self.master_count() as i16)
    }

    /// Port of `FindFormID` on the sorted records: a FormID past the
    /// masters belongs to the file itself.
    fn find_form_id(&self, form_id: FormID) -> Option<Arc<MainRecordImpl>> {
        let index = self.find_form_id_index(form_id)?.ok()?;
        self.fl_records.read().unwrap().get(index).cloned()
    }

    /// Port of `FindFormID` with the index: `Ok` with the position of the
    /// first record with the FormID, `Err` with the position it would take.
    /// `None` while the records are not sorted.
    pub(crate) fn find_form_id_index(&self, form_id: FormID) -> Option<Result<usize, usize>> {
        if !self.fl_form_ids_sorted.load(Ordering::Acquire) {
            return None;
        }
        let sorted = self.fl_records.read().unwrap();
        let form_id = if self.is_new_record(form_id.file_id()) {
            form_id.change_file_id(self.get_file_file_id())
        } else {
            form_id
        };
        // Port of the binary search of `FindFormID`: the first record with the
        // FormID. Of records with the same FormID the stable sort keeps the
        // file order, so the first one in the file wins.
        let key = form_id.to_cardinal();
        let index = sorted.partition_point(|record| record.get_fixed_form_id().to_cardinal() < key);
        let found = sorted
            .get(index)
            .is_some_and(|record| record.get_fixed_form_id().to_cardinal() == key);
        Some(if found { Ok(index) } else { Err(index) })
    }

    /// Port of `GetMasterRecordByFormID`: the record of the master the
    /// FormID points to.
    fn get_master_record_by_form_id(
        &self,
        mut form_id: FormID,
        allow_injected: bool,
        new_masters: bool,
    ) -> Option<Arc<MainRecordImpl>> {
        let mut master: Option<Arc<FileImpl>> = None;
        if form_id.object_id() < 0x800 && !self.get_allow_hardcoded_range_use() {
            master = game_master_file();
            if let Some(master) = &master {
                form_id = form_id.change_file_id(master.get_file_file_id());
            }
        }
        if master.is_none() && crate::interface::globals::complex_file_file_id() {
            // The slot among the masters of the module type of the FileID.
            let index = self.get_master_index_for_file_id(form_id.file_id());
            master = usize::try_from(index)
                .ok()
                .and_then(|index| self.fl_masters.read().unwrap().get(index).cloned());
        } else if master.is_none() {
            let slot = form_id.file_id().full_slot();
            let masters = self.fl_masters.read().unwrap();
            if slot >= 0 && (slot as usize) < masters.len() {
                master = Some(masters[slot as usize].clone());
            }
        }
        let master = master?;
        if std::ptr::eq(&*master, self) {
            return None;
        }
        let target = form_id.change_file_id(master.get_file_file_id());
        master.record_by_form_id(target, allow_injected, new_masters)
    }

    /// Port of `GetRecordByFormID` with the concrete record type.
    pub fn record_by_form_id(
        self: &Arc<Self>,
        form_id: FormID,
        allow_injected: bool,
        new_masters: bool,
    ) -> Option<Arc<MainRecordImpl>> {
        if let Some(record) = self.find_form_id(form_id) {
            return Some(record);
        }
        if allow_injected
            && self.is_new_record(form_id.file_id())
            && let Some(record) = self.find_injected_id(form_id)
        {
            return Some(record);
        }
        if self.get_file_states().contains(FileState::fsIsGameMaster) {
            return None;
        }
        let master = self.get_master_record_by_form_id(form_id, allow_injected, new_masters)?;
        Some(master.highest_override_visible_for_file(self))
    }

    /// Port of `FileFileIDtoLoadOrderFileID`.
    fn file_file_id_to_load_order_file_id(&self, file_id: FileID) -> Option<FileID> {
        if crate::interface::globals::complex_file_file_id() {
            if let Ok(index) = usize::try_from(self.get_master_index_for_file_id(file_id))
                && let Some(master) = self.fl_masters.read().unwrap().get(index)
            {
                return Some(master.get_load_order_file_id());
            }
            let own = self.get_load_order_file_id();
            return own.is_valid().then_some(own);
        }
        let slot = file_id.full_slot();
        let masters = self.fl_masters.read().unwrap();
        if slot >= 0 && (slot as usize) < masters.len() {
            return Some(masters[slot as usize].get_load_order_file_id());
        }
        let own = self.get_load_order_file_id();
        own.is_valid().then_some(own)
    }

    /// Port of `LoadOrderFormIDtoFileFormID`: `None` when the file does
    /// not see the file of the FormID.
    pub fn load_order_form_id_to_file_form_id(&self, form_id: FormID) -> Option<FormID> {
        if form_id.object_id() < 0x800 && !self.get_allow_hardcoded_range_use() {
            return Some(form_id);
        }
        let file_id = form_id.file_id();
        if file_id == self.get_load_order_file_id() {
            return Some(form_id.change_file_id(self.get_file_file_id()));
        }
        let masters = self.fl_masters.read().unwrap();
        if crate::interface::globals::complex_file_file_id() {
            // Port of the `wbComplexFileFileID` branch of
            // `LoadOrderFileIDtoFileFileID`: the slot among the masters of
            // the module type.
            let (mut full, mut medium, mut light) = (0, 0, 0);
            for master in masters.iter() {
                let module_type = master.module_type();
                if master.get_load_order_file_id() == file_id {
                    let file_file_id = match module_type {
                        ModuleType::mtLight => FileID::create_light(light),
                        ModuleType::mtMedium => FileID::create_medium(medium),
                        ModuleType::mtFull => FileID::create_full(full),
                    };
                    return Some(form_id.change_file_id(file_file_id));
                }
                match module_type {
                    ModuleType::mtLight => light += 1,
                    ModuleType::mtMedium => medium += 1,
                    ModuleType::mtFull => full += 1,
                }
            }
            return None;
        }
        let index = masters
            .iter()
            .position(|master| master.get_load_order_file_id() == file_id)?;
        Some(form_id.change_file_id(FileID::create_full(index as i16)))
    }

    /// Port of `ContainedRecordByLoadOrderFormID`.
    pub fn contained_record_by_load_order_form_id(&self, form_id: FormID) -> Option<Arc<MainRecordImpl>> {
        let file_form_id = self.load_order_form_id_to_file_form_id(form_id)?;
        self.find_form_id(file_form_id)
    }

    pub fn masters(&self) -> Vec<Arc<FileImpl>> {
        self.fl_masters.read().unwrap().clone()
    }

    /// Port of `SortRecords`: `flRecords` in FormID order (a stable sort by
    /// the fixed FormID), which the lookups by FormID search.
    pub fn sort_records(&self) {
        let mut sorted = self.fl_records.read().unwrap().clone();
        // The keys once per record: a comparison would read two records.
        sorted.sort_by_cached_key(|record| record.get_fixed_form_id().to_cardinal());
        *self.fl_records.write().unwrap() = sorted;
        self.fl_form_ids_sorted.store(true, Ordering::Release);
    }

    /// Port of `AddMaster` by file name: loads the master from the directory
    /// of the file, once.
    fn add_master(self: &Arc<Self>, file_name: &str) -> Result<(), LoadError> {
        let name = path_file_name(file_name);
        let dir = match self.fl_file_name.rfind(['\\', '/']) {
            Some(index) => &self.fl_file_name[..=index],
            None => "",
        };
        let path = format!("{dir}{name}");
        progress(&format!("[{}] Adding master \"{name}\"", self.get_name()));
        let master = match find_in_files_map(&path) {
            Some(master) => master,
            None => wb_file(&path, i32::MAX, FileStates::empty())?,
        };
        self.fl_masters.write().unwrap().push(master);
        Ok(())
    }

    fn element_ref(&self) -> ElementRef {
        self.self_ref.upgrade().expect("a file is alive while it is used")
    }

    /// Port of `TwbFile.Scan` for the structure of the file.
    fn scan(self: &Arc<Self>) -> Result<(), LoadError> {
        self.fl_states.write().unwrap().include(FileState::fsScanning);
        let result = self.scan_records();
        self.fl_states.write().unwrap().exclude(FileState::fsScanning);
        result?;
        self.fl_load_finished.set(()).ok();
        Ok(())
    }

    /// Port of `TwbFileSource.Scan`: a save or co-save is its header and the
    /// chapters of the game. The plugins the header lists load as masters
    /// from the data folder when they exist there.
    fn scan_save(self: &Arc<Self>) -> Result<(), LoadError> {
        let container = self.element_ref();
        let file = Arc::downgrade(self);
        *self.fl_load_order_file_id.write().unwrap() = FileID::create_full(0xFF);
        let unexpected = || LoadError(format!("Unexpected error reading file \"{}\"", self.fl_file_name));
        let header_def = file_header().ok_or_else(unexpected)?;
        let mut cursor = value::Cursor {
            block: DataBlock::File(self.fl_bytes.clone()),
            pos: 0,
            end: self.fl_bytes.as_slice().len(),
        };
        let header = value::create_value_element(&container, &file, &mut cursor, header_def, "");
        let header: ElementRef = header;
        // Port of `TwbFileHeader.GetFileMagic`.
        let magic = match header.as_container().and_then(|header| header.get_element(0)) {
            Some(magic) => match magic.get_native_value() {
                Variant::Str(text) => text,
                _ => magic.get_value(),
            },
            None => String::new(),
        };
        if magic != file_magic() {
            return Err(LoadError(format!(
                "Expected header Magic {}, found {magic} in file \"{}\"",
                file_magic(),
                self.fl_file_name
            )));
        }
        if self.fl_states.read().unwrap().contains(FileState::fsOnlyHeader) {
            return Ok(());
        }
        let plugins = file_plugins();
        let master_files: Option<ElementRef> = match plugins.strip_prefix("Absolute:") {
            Some(offset) => {
                // A co-save lists its plugins at a fixed offset, outside of
                // its chapters: the array is not an element of the file.
                let offset = offset.trim().parse::<usize>().map_err(|_| unexpected())?;
                let modules = wb_array_count(
                    "Modules",
                    wb_len_string("PluginName", 2, ConflictPriority::cpNormal, false, None, None)
                        .map(|def| def as Arc<dyn ValueDef>),
                    -4,
                    ConflictPriority::cpNormal,
                    false,
                    None,
                    None,
                )
                .ok_or_else(unexpected)?;
                let mut cursor = value::Cursor {
                    block: DataBlock::File(self.fl_bytes.clone()),
                    pos: offset.min(self.fl_bytes.as_slice().len()),
                    end: self.fl_bytes.as_slice().len(),
                };
                let modules: ElementRef =
                    value::create_value_element(&container, &file, &mut cursor, modules as Arc<dyn ValueDef>, "");
                self.container.remove_element_by_identity(&modules);
                Some(modules)
            }
            None => header
                .as_container()
                .and_then(|header| header.get_element_by_name(&plugins)),
        };
        if let Some(master_files) = master_files
            && let Some(master_files) = master_files.as_container()
        {
            for index in 0..master_files.get_element_count() {
                let Some(master) = master_files.get_element(index) else {
                    continue;
                };
                let path = format!("{}{}", data_path(), master.get_edit_value());
                if Path::new(&path).is_file() {
                    self.add_master_path(&path)?;
                }
            }
        }
        let chapters = file_chapters().ok_or_else(unexpected)?;
        let extract_info = extract_info();
        for index in 0..usize::try_from(chapters.get_member_count()).unwrap_or(0) {
            let mut member = chapters.get_member(index).clone();
            if member.get_def_type() == DefType::dtResolvable {
                member = value::resolve(member, cursor.data(), Some(&container));
            }
            if member.def_base().def_flags.contains(DefFlag::dfUnionStaticResolve) {
                member = value::resolve(member, cursor.data(), Some(&container));
            }
            let element = value::create_value_element(&container, &file, &mut cursor, member, "");
            if extract_info.contains(&(index as u8)) {
                element.do_init();
            }
        }
        for (index, element) in self.container.elements().iter().enumerate() {
            if let Some(element) = element.as_element_impl() {
                element
                    .element_base()
                    .e_sort_order
                    .store(index as i32, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    /// Port of `AddMaster` with the full path of a plugin that a save lists.
    fn add_master_path(self: &Arc<Self>, path: &str) -> Result<(), LoadError> {
        progress(&format!(
            "[{}] Adding master \"{}\"",
            self.get_name(),
            path_file_name(path)
        ));
        let master = match find_in_files_map(path) {
            Some(master) => master,
            None => wb_file(path, i32::MAX, FileStates::empty())?,
        };
        self.fl_masters.write().unwrap().push(master);
        Ok(())
    }

    fn scan_records(self: &Arc<Self>) -> Result<(), LoadError> {
        // Port of the choice of `wbFile`: a file that is not a module is a
        // `TwbFileSource`, a save or co-save.
        if tool_source() == ToolSource::tsSaves && !is_module(&self.fl_file_name) {
            return self.scan_save();
        }
        let bytes = self.fl_bytes.as_slice();
        let container = self.element_ref();
        let mut offset = 0;
        // The header record.
        let header = create_record(self, &container, bytes, &mut offset, None)?;
        let Some(header) = header.and_then(|record| record.into_main_record_impl()) else {
            return Err(LoadError(format!(
                "Unexpected error reading file \"{}\"",
                self.fl_file_name
            )));
        };
        if header.get_signature() != header_signature() {
            return Err(LoadError(format!(
                "Expected header signature TES4, found {} in file \"{}\"",
                header.get_signature(),
                self.fl_file_name
            )));
        }
        if self.fl_states.read().unwrap().contains(FileState::fsOnlyHeader) {
            return Ok(());
        }
        // A compare load takes the load order of the file it compares to.
        if self.get_file_states().contains(FileState::fsIsCompareLoad) {
            let compare_to = self.fl_compare_to.as_deref().and_then(|name| {
                let name = path_file_name(name);
                FILES_MAP
                    .read()
                    .unwrap()
                    .iter()
                    .find(|file| file.get_name().eq_ignore_ascii_case(name))
                    .cloned()
            });
            *self.fl_load_order_file_id.write().unwrap() = match compare_to {
                Some(compare_to) => compare_to.get_load_order_file_id(),
                None => FileID::create_full(0xFF),
            };
        }
        // The masters load before the slot is decided.
        if let Some(master_files) = header.get_element_by_name("Master Files") {
            let master_files = master_files.as_container().expect("the master files are a container");
            for index in 0..master_files.get_element_count() {
                let master = master_files
                    .get_element(index)
                    .and_then(|master| master.as_container()?.get_record_by_signature(Signature::new(b"MAST")))
                    .ok_or_else(|| {
                        LoadError(format!(
                            "Unexpected error reading master list for file \"{}\"",
                            self.fl_file_name
                        ))
                    })?;
                self.add_master(&master.get_edit_value())?;
            }
        }
        self.assign_slot(&header)?;
        // The top level groups, on the worker threads when there are several.
        scan::scan_top_level(self, &container, bytes, &mut offset)?;
        // `SortRecords` and `flActivateIndices` come before the group check:
        // the sort of a merged group finds the record of a child group in
        // this file.
        // `SortRecords` sorts `flRecords` itself, so the indices take the
        // records in FormID order: of records with the same key, the one with
        // the lowest FormID is kept.
        self.sort_records();

        self.activate_indices();
        // Port of the top level group check of `TwbFile.Scan` for the games
        // from Skyrim on: an empty top level group is removed, and a group
        // whose label appears again later in the file is merged into that
        // later group, which is then sorted.
        if is_skyrim() || is_fallout3() || is_fallout4() || is_fallout76() || is_starfield() {
            let elements = self.container.elements();
            let mut groups: HashMap<i32, Arc<GroupRecordImpl>> = HashMap::new();
            for index in (1..elements.len()).rev() {
                let Some(group) = elements[index]
                    .as_element_impl()
                    .and_then(|element| element.group_record_impl())
                else {
                    progress(&format!(
                        "[{}] Error: File contains invalid top level record: {}",
                        self.get_name(),
                        elements[index].get_name()
                    ));
                    continue;
                };
                let name = group.get_name();
                if group.get_element_count() == 0 {
                    progress(&format!(
                        "[{}] Warning: File contains empty top level group: {name}",
                        self.get_name()
                    ));
                    self.container.remove_element_by_identity(&elements[index]);
                    // `GroupRecord.Remove` under `wbBeginInternalEdit(True)`.
                    let internal = crate::interface::globals::begin_internal_edit(true);
                    self.set_modified(true);
                    if internal {
                        crate::interface::globals::end_internal_edit();
                    }
                    progress(&format!("[{}] Removed empty group: {name}", self.get_name()));
                    continue;
                }
                if group.group_type() != 0 {
                    progress(&format!(
                        "[{}] Error: File contains invalid top level group type {} for group: {name}",
                        self.get_name(),
                        group.group_type()
                    ));
                    continue;
                }
                let sort_order = group.base.e_sort_order.load(Ordering::Relaxed);
                if sort_order < 0 {
                    progress(&format!(
                        "[{}] Error: File contains top level group without known sort order: {name}",
                        self.get_name()
                    ));
                    continue;
                }
                if let Some(later) = groups.get(&sort_order).cloned() {
                    progress(&format!(
                        "[{}] Warning: File contains duplicated top level group: {name}",
                        self.get_name()
                    ));
                    let later_ref: ElementRef = later.clone();
                    // Upstream merges under `wbBeginInternalEdit(True)`: the
                    // group that takes the records and the file are modified
                    // internally, so the save writes the merged group with
                    // its new size.
                    let internal = crate::interface::globals::begin_internal_edit(true);
                    if later.get_element_count() == 0 {
                        self.container.remove_element_by_identity(&later_ref);
                        self.set_modified(true);
                        groups.insert(sort_order, group);
                    } else {
                        let moved = group.container.release_elements();
                        let count = moved.len();
                        for element in moved {
                            if let Some(element_impl) = element.as_element_impl() {
                                element_impl.element_base().set_container(&later_ref);
                            }
                            later.container.add_element(element);
                        }
                        later.sort();
                        later.set_modified(true);
                        progress(&format!(
                            "[{}] Merged {count} record from duplicated group: {name}",
                            self.get_name()
                        ));
                        self.container.remove_element_by_identity(&elements[index]);
                        self.set_modified(true);
                    }
                    if internal {
                        crate::interface::globals::end_internal_edit();
                    }
                    continue;
                }
                groups.insert(sort_order, group);
            }
        }
        Ok(())
    }

    /// Port of `flActivateIndices`: the keys of every record go into the
    /// named indices.
    fn activate_indices(self: &Arc<Self>) {
        if self.fl_indices_active.swap(true, Ordering::AcqRel) {
            return;
        }
        progress(&format!("[{}] Building string indices", self.get_name()));
        let records = self.fl_records.read().unwrap().clone();
        for record in &records {
            let keys = record.activate_index_keys();
            self.add_keys_to_indices(record, &keys);
        }
        progress(&format!("[{}] String indices built", self.get_name()));
    }

    /// Port of `flAddKeysToIndices`.
    fn add_keys_to_indices(&self, record: &Arc<MainRecordImpl>, keys: &[(i32, String)]) {
        if !self.fl_indices_active.load(Ordering::Acquire) {
            return;
        }
        let mut indices = self.fl_records_indices.write().unwrap();
        for (index, key) in keys {
            let Ok(position) = usize::try_from(*index) else {
                continue;
            };
            if position >= indices.len() {
                indices.resize_with(position + 1, HashMap::new);
            }
            let key = named_index_key(*index, key);
            if let Some(existing) = indices[position].get(&key) {
                progress(&format!(
                    "[{}] Duplicate Key in Index \"{}\": \"{}\" Existing: {} New: {}",
                    self.get_name(),
                    named_index_name(*index),
                    key,
                    existing.get_short_name(),
                    record.get_short_name()
                ));
                continue;
            }
            indices[position].insert(key, record.clone());
        }
    }

    /// Port of `flFindKeyInIndex`.
    fn find_key_in_index(&self, index: i32, key: &str) -> Option<Arc<MainRecordImpl>> {
        if !self.fl_indices_active.load(Ordering::Acquire) {
            return None;
        }
        let indices = self.fl_records_indices.read().unwrap();
        let records = indices.get(usize::try_from(index).ok()?)?;
        records.get(&named_index_key(index, key)).cloned()
    }

    /// Port of `AddAllMastersToSet` with `SortByReverseLoadOrder`: every
    /// master, direct or not, from the highest load order down.
    fn all_masters(&self) -> Vec<Arc<FileImpl>> {
        let mut result: Vec<Arc<FileImpl>> = Vec::new();
        let mut pending = self.masters();
        while let Some(master) = pending.pop() {
            if result.iter().any(|known| Arc::ptr_eq(known, &master)) {
                continue;
            }
            pending.extend(master.masters());
            result.push(master);
        }
        result.sort_by_key(|master| std::cmp::Reverse(master.load_order()));
        result
    }

    /// Port of `GetRecordFromIndexByKey`.
    pub fn record_from_index_by_key(&self, index: i32, key: &str) -> Option<Arc<MainRecordImpl>> {
        if let Some(record) = self.find_key_in_index(index, key) {
            return Some(record);
        }
        self.all_masters()
            .iter()
            .find_map(|master| master.find_key_in_index(index, key))
    }

    /// Port of `AssignSlot` in `TwbFile.Scan`, for a file without masters
    /// that are loaded: the load order slot of the file.
    fn assign_slot(&self, header: &MainRecordImpl) -> Result<(), LoadError> {
        if self.fl_load_order_file_id.read().unwrap().full_slot() >= 0 {
            return Ok(());
        }
        if self.load_order() == i32::MAX {
            self.fl_load_order
                .store(NEXT_LOAD_ORDER.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        let load_order = self.load_order();
        if load_order < 0 {
            return Ok(());
        }
        NEXT_LOAD_ORDER.fetch_max(load_order + 1, Ordering::Relaxed);
        let flags = header.mr_struct().flags;
        let file_id = if is_light_supported()
            || pseudo_light()
            || is_medium_supported()
            || pseudo_medium()
            || is_update_supported()
            || pseudo_update()
        {
            // UPSTREAM-QUIRK: the light, medium and update slots count up
            // across the files of a session; the first file gets slot 0.
            if (is_update_supported() || pseudo_update()) && flags.is_update() {
                FileID::invalid()
            } else if flags.is_light() || self.fl_file_name.to_ascii_lowercase().ends_with(".esl") {
                FileID::create_light(next_light_slot())
            } else if flags.is_medium() && is_medium_supported() {
                FileID::create_medium(next_medium_slot())
            } else {
                FileID::create_full(next_full_slot())
            }
        } else {
            if load_order > i32::from(FileID::max_full_slot()) {
                return Err(LoadError("Too many modules".to_owned()));
            }
            FileID::create_full(load_order as i16)
        };
        *self.fl_load_order_file_id.write().unwrap() = file_id;
        Ok(())
    }
}

static NEXT_FULL_SLOT: AtomicI32 = AtomicI32::new(0);
static NEXT_LIGHT_SLOT: AtomicI32 = AtomicI32::new(0);
static NEXT_MEDIUM_SLOT: AtomicI32 = AtomicI32::new(0);

/// Port of `wbContainedInDef`: the definitions of the contained-in elements
/// by group type, created once.
fn contained_in_def(group_type: i32, name: &str, signature: Signature) -> Arc<dyn ValueDef> {
    static DEFS: RwLock<Vec<(i32, Arc<dyn ValueDef>)>> = RwLock::new(Vec::new());
    if let Some((_, def)) = DEFS.read().unwrap().iter().find(|(kind, _)| *kind == group_type) {
        return def.clone();
    }
    let def = crate::interface::builders::wb_form_id_ck(
        name,
        &[signature],
        false,
        ConflictPriority::cpNormal,
        true,
        None,
        None,
    )
    .expect("the contained-in definition") as Arc<dyn ValueDef>;
    DEFS.write().unwrap().push((group_type, def.clone()));
    def
}

/// The bytes of a `PLYR` group with the player reference `[00000014]
/// <PlayerRef>` that `TwbFile.Scan` adds to the hardcoded file: a new record
/// with the form version of the game and only the editor ID. The record and
/// group headers have the size of the game: Oblivion's have no form version.
fn player_reference_group() -> Vec<u8> {
    let header_size = size_of_main_record_struct() as usize;
    let version: u16 = match game_mode() {
        GameMode::gmSF1 => 582,
        GameMode::gmFO76 => 208,
        GameMode::gmFO4 | GameMode::gmFO4VR => 131,
        GameMode::gmSSE | GameMode::gmTES5VR | GameMode::gmEnderalSE => 44,
        GameMode::gmTES5 | GameMode::gmEnderal => 43,
        _ => 15,
    };
    let editor_id = b"PlayerRef\0";
    let mut record = Vec::new();
    record.extend_from_slice(b"EDID");
    record.extend_from_slice(&(editor_id.len() as u16).to_le_bytes());
    record.extend_from_slice(editor_id);
    let mut header = Vec::new();
    header.extend_from_slice(b"PLYR");
    header.extend_from_slice(&(record.len() as u32).to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&0x14u32.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&version.to_le_bytes());
    header.extend_from_slice(&0u16.to_le_bytes());
    header.truncate(header_size);
    let group_size = header_size + header.len() + record.len();
    let mut group = Vec::with_capacity(group_size);
    group.extend_from_slice(b"GRUP");
    group.extend_from_slice(&(group_size as u32).to_le_bytes());
    group.extend_from_slice(b"PLYR");
    group.extend_from_slice(&0i32.to_le_bytes());
    group.extend_from_slice(&0u32.to_le_bytes());
    group.extend_from_slice(&0u32.to_le_bytes());
    group.truncate(header_size);
    group.extend_from_slice(&header);
    group.extend_from_slice(&record);
    group
}

/// Port of `PrecombinedCache`: the precombined references of the cell that
/// was looked at last, by cell FormID and file name. One per thread: the
/// cache only saves reading the cell again, and a thread does not wait for
/// another one's cell while it holds the cache.
type PrecombinedCache = Option<(FormID, String, Arc<Vec<(u32, u32)>>)>;
thread_local! {
    static PRECOMBINED_CACHE: std::cell::RefCell<PrecombinedCache> = const { std::cell::RefCell::new(None) };
}

/// The main records in the order their subrecords were built, with the
/// count of the build. Delphi resets a record when its last
/// `IwbContainerElementRef` goes away, so a record that a callback reads
/// (a navmesh an edge of another navmesh links to) does not keep its
/// subrecords. The port has no such count and resets the records built
/// longest ago instead, at the points where nothing holds their elements
/// (`trim_initialized_records`).
static INITIALIZED_RECORDS: std::sync::Mutex<std::collections::VecDeque<(Weak<MainRecordImpl>, u32)>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

/// About the length of `INITIALIZED_RECORDS`, read without its lock: the
/// workers of the dump check it after every line.
static INITIALIZED_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The main records that a dump writes on a worker thread, by address:
/// [`trim_initialized_records`] keeps them built, as it keeps the record of
/// the serial dump, so that a record is not built again for every line.
static PINNED_RECORDS: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

/// Keeps a record built while the guard lives (see [`PINNED_RECORDS`]).
pub struct RecordPin(usize);

impl Drop for RecordPin {
    fn drop(&mut self) {
        let mut pinned = PINNED_RECORDS.lock().unwrap();
        if let Some(index) = pinned.iter().position(|address| *address == self.0) {
            pinned.swap_remove(index);
        }
    }
}

/// Pins `record` (see [`PINNED_RECORDS`]).
pub fn pin_record(record: &Arc<MainRecordImpl>) -> RecordPin {
    let address = Arc::as_ptr(record) as usize;
    PINNED_RECORDS.lock().unwrap().push(address);
    RecordPin(address)
}

fn is_pinned(record: &Arc<MainRecordImpl>) -> bool {
    let address = Arc::as_ptr(record) as usize;
    PINNED_RECORDS.lock().unwrap().contains(&address)
}

/// Resets the main records whose subrecords were built longest ago until at
/// most `keep` builds remain, except `except` and the pinned records, which
/// stay built. A reset record gives up its elements and builds them again on
/// the next use; an element of it that is held keeps working, detached.
pub fn trim_initialized_records(keep: usize, except: Option<&Arc<MainRecordImpl>>) {
    if INITIALIZED_COUNT.load(Ordering::Relaxed) <= keep {
        return;
    }
    let expired: Vec<(Weak<MainRecordImpl>, u32)> = {
        // While another thread trims, this one does not wait for it: the
        // records are trimmed all the same, and when does not change what
        // is written.
        let mut records = match INITIALIZED_RECORDS.try_lock() {
            Ok(records) => records,
            Err(std::sync::TryLockError::WouldBlock) => return,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        };
        if records.len() <= keep {
            return;
        }
        let excess = records.len() - keep;
        let expired = records.drain(..excess).collect();
        INITIALIZED_COUNT.store(records.len(), Ordering::Relaxed);
        expired
    };
    for (record, build) in expired {
        let Some(record) = record.upgrade() else { continue };
        if except.is_some_and(|except| Arc::ptr_eq(except, &record)) || is_pinned(&record) {
            INITIALIZED_RECORDS
                .lock()
                .unwrap()
                .push_back((Arc::downgrade(&record), build));
            INITIALIZED_COUNT.fetch_add(1, Ordering::Relaxed);
        } else if record.mr_builds.load(Ordering::Relaxed) == build {
            // A record built again since this entry has a newer entry.
            record.reset();
        }
    }
}

/// Port of `StrRight`: the text padded on the left with spaces to `len`.
fn str_right(text: &str, len: usize) -> String {
    format!("{text:>len$}")
}

/// Port of `wbGetGameMasterFile`.
pub fn game_master_file() -> Option<Arc<FileImpl>> {
    FILES_MAP
        .read()
        .unwrap()
        .iter()
        .find(|file| file.get_file_states().contains(FileState::fsIsGameMaster))
        .cloned()
}

fn next_full_slot() -> i16 {
    NEXT_FULL_SLOT.fetch_add(1, Ordering::Relaxed) as i16
}

fn next_light_slot() -> i16 {
    NEXT_LIGHT_SLOT.fetch_add(1, Ordering::Relaxed) as i16
}

fn next_medium_slot() -> i16 {
    NEXT_MEDIUM_SLOT.fetch_add(1, Ordering::Relaxed) as i16
}

/// Resets the load order slots, for the tests.
pub fn reset_load_order_slots() {
    NEXT_FULL_SLOT.store(0, Ordering::Relaxed);
    NEXT_LIGHT_SLOT.store(0, Ordering::Relaxed);
    NEXT_MEDIUM_SLOT.store(0, Ordering::Relaxed);
}

/// Port of `wbFile`: loads a plugin.
///
/// `states` keeps `fsIsTemporary`, `fsIsHardcoded`, `fsOnlyHeader` and
/// `fsIsDeltaPatch` as upstream.
pub fn wb_file(file_name: &str, load_order: i32, states: FileStates) -> Result<Arc<FileImpl>, LoadError> {
    if let Some(file) = find_in_files_map(file_name) {
        return Ok(file);
    }
    let path = Path::new(file_name);
    let mapped = MappedFile::open(path).map_err(|error| LoadError(format!("Cannot open {file_name}: {error}")))?;
    wb_file_from_bytes(file_name, load_order, states, FileBytes::Mapped(mapped))
}

/// Port of `wbFile` with the data of the file given.
pub fn wb_file_from_bytes(
    file_name: &str,
    load_order: i32,
    states: FileStates,
    bytes: FileBytes,
) -> Result<Arc<FileImpl>, LoadError> {
    wb_file_compare(file_name, load_order, None, states, bytes)
}

/// Port of `wbFile` with `aCompareTo`: a file loaded in the place of
/// `compare_to`, which it takes the load order of. The hardcoded records
/// load this way, with the name of the game executable and the game
/// master as the file to compare to.
pub fn wb_file_compare(
    file_name: &str,
    load_order: i32,
    compare_to: Option<&str>,
    states: FileStates,
    bytes: FileBytes,
) -> Result<Arc<FileImpl>, LoadError> {
    let mut fl_states = FileStates::empty();
    for state in [
        FileState::fsIsTemporary,
        FileState::fsIsHardcoded,
        FileState::fsOnlyHeader,
        FileState::fsIsDeltaPatch,
    ] {
        if states.contains(state) {
            fl_states.include(state);
        }
    }
    let base_name = path_file_name(file_name);
    if compare_to.is_some() {
        fl_states.include(FileState::fsIsCompareLoad);
        if base_name.eq_ignore_ascii_case(&game_exe_name()) {
            fl_states.include(FileState::fsIsHardcoded);
        }
    } else if base_name.eq_ignore_ascii_case(&game_master_esm()) {
        fl_states.include(FileState::fsIsGameMaster);
        fl_states.include(FileState::fsIsOfficial);
    } else if crate::interface::globals::official_dlc()
        .iter()
        .any(|name| name.eq_ignore_ascii_case(base_name))
        || (crate::interface::globals::is_skyrim() && base_name.eq_ignore_ascii_case("Update.esm"))
    {
        // The module of an official DLC (`miOfficialIndex`, which Skyrim's
        // Update.esm has too) is official; Starfield does not let it be
        // edited (`GetIsEditable`).
        fl_states.include(FileState::fsIsOfficial);
    }
    fl_states.include(FileState::fsMemoryMapped);
    // UPSTREAM-QUIRK: upstream adds the player reference to the hardcoded
    // file through the editing API after the scan; here its group is
    // appended to the bytes before the scan, which reads the same.
    let bytes = if fl_states.contains(FileState::fsIsHardcoded) && game_mode() > GameMode::gmTES3 {
        let mut owned = bytes.as_slice().to_vec();
        owned.extend_from_slice(&player_reference_group());
        FileBytes::Owned(owned)
    } else {
        bytes
    };
    let file = Arc::new_cyclic(|self_ref: &Weak<FileImpl>| FileImpl {
        self_ref: self_ref.clone(),
        base: ElementBase::new(None),
        container: ContainerBase::default(),
        fl_file_name: file_name.to_owned(),
        fl_load_order: AtomicI32::new(load_order),
        fl_load_order_file_id: RwLock::new(FileID::invalid()),
        fl_states: RwLock::new(fl_states),
        fl_bytes: Arc::new(bytes),
        fl_records: RwLock::new(Vec::new()),
        fl_form_ids_sorted: AtomicBool::new(false),
        fl_masters: RwLock::new(Vec::new()),
        fl_load_finished: OnceLock::new(),
        fl_version: OnceLock::new(),
        fl_compare_to: compare_to.map(str::to_owned),
        fl_injected_records: RwLock::new(Vec::new()),
        fl_records_indices: RwLock::new(Vec::new()),
        fl_indices_active: std::sync::atomic::AtomicBool::new(false),
        fl_scanned_form_ids: std::sync::Mutex::new(std::collections::HashSet::new()),
        fl_crc32: std::sync::atomic::AtomicU32::new(0),
    });
    progress(&format!("[{}] Loading file", file.get_name()));
    FILES_MAP.write().unwrap().push(file.clone());
    file.scan()?;
    crate::interface::element::add_file(file.clone());
    Ok(file)
}

/// Port of `TwbRecord.CreateForPtr` for the records of a file or group:
/// a group record for `GRUP`, a main record otherwise.
fn create_record(
    file: &Arc<FileImpl>,
    container: &ElementRef,
    bytes: &[u8],
    offset: &mut usize,
    prev_main_record: Option<&Arc<MainRecordImpl>>,
) -> Result<Option<ElementRef>, LoadError> {
    let Some(signature) = bytes.get(*offset..*offset + 4) else {
        return Err(LoadError(format!(
            "Unexpected end of file in \"{}\"",
            file.fl_file_name
        )));
    };
    if signature == b"GRUP" {
        let group = GroupRecordImpl::create(file, container, bytes, offset)?;
        Ok(Some(group as ElementRef))
    } else {
        let record = MainRecordImpl::create(file, container, bytes, offset, prev_main_record)?;
        Ok(Some(record as ElementRef))
    }
}

/// Port of `TwbGroupRecord`.
pub struct GroupRecordImpl {
    self_ref: Weak<GroupRecordImpl>,
    base: ElementBase,
    container: ContainerBase,
    file: Weak<FileImpl>,
    gr_struct: RwLock<GroupRecordStruct>,
    /// The range of the group in the file, header included.
    dc_base: usize,
    dc_end: usize,
    /// Port of `gsSorted`: the group was sorted, and `sort` does nothing
    /// until a change of its members clears it.
    gr_sorted: AtomicBool,
    /// Port of `gsSorting`: the group sorts now.
    gr_sorting: AtomicBool,
}

impl GroupRecordImpl {
    fn create(
        file: &Arc<FileImpl>,
        container: &ElementRef,
        bytes: &[u8],
        offset: &mut usize,
    ) -> Result<Arc<Self>, LoadError> {
        let header_size = size_of_main_record_struct() as usize;
        let gr_struct = GroupRecordStruct::parse(bytes, *offset)
            .ok_or_else(|| LoadError(format!("Unexpected end of file in \"{}\"", file.fl_file_name)))?;
        if (gr_struct.group_size as usize) < header_size {
            return Err(LoadError(format!(
                "[{}] {} size is invalid.",
                file.fl_file_name,
                gr_struct.name()
            )));
        }
        let dc_base = *offset;
        let dc_end = (dc_base + gr_struct.group_size as usize).min(bytes.len());
        let group = Arc::new_cyclic(|self_ref: &Weak<GroupRecordImpl>| GroupRecordImpl {
            self_ref: self_ref.clone(),
            base: ElementBase::new(Some(container)),
            container: ContainerBase::default(),
            file: Arc::downgrade(file),
            gr_struct: RwLock::new(gr_struct),
            dc_base,
            dc_end,
            gr_sorted: AtomicBool::new(false),
            gr_sorting: AtomicBool::new(false),
        });
        if gr_struct.group_type == 0 {
            let order = wb_get_group_order(gr_struct.label_signature());
            group.base.e_sort_order.store(order, Ordering::Relaxed);
            group.base.e_memory_order.store(order, Ordering::Relaxed);
        }
        scan::attach(container, group.clone());
        // Port of `TwbGroupRecord.ScanData`, the large child groups on the
        // worker threads.
        let self_element: ElementRef = group.clone();
        scan::scan_records(file, &self_element, bytes, dc_base + header_size, dc_end)?;
        *offset = dc_end;
        Ok(group)
    }

    /// Port of `grStruct`: the group header as it is now.
    pub fn gr_struct(&self) -> GroupRecordStruct {
        *self.gr_struct.read().unwrap()
    }

    pub fn group_type(&self) -> i32 {
        self.gr_struct().group_type
    }

    pub fn group_label(&self) -> u32 {
        self.gr_struct().label
    }

    /// Port of `ChildrenOf`: the record the group holds the children of,
    /// for the group types whose label is a FormID and the exterior blocks.
    pub fn children_of(&self) -> Option<Arc<MainRecordImpl>> {
        match self.gr_struct().group_type {
            1 | 6..=10 => {
                let file = self.file.upgrade()?;
                file.record_by_form_id(FormID::from_cardinal(self.get_group_label()), true, true)
            }
            // An exterior cell block or sub-block: the worldspace of the
            // world children group above it.
            4 | 5 => self.parent_group()?.children_of(),
            _ => None,
        }
    }

    fn parent_group(&self) -> Option<Arc<GroupRecordImpl>> {
        self.base.container()?.as_element_impl()?.group_record_impl()
    }

    /// Port of `TwbGroupRecord.Sort` without `aForce`, for the groups that
    /// sort by `CompareGroupContents`. A topic group (type 7) sorts its INFOs
    /// by their links instead, which xDump does not do without the load
    /// order FormIDs, so it is left alone.
    /// UPSTREAM-QUIRK: the merge of a duplicated top level group adds the
    /// records without clearing `gsSorted`, so a group that two duplicates
    /// merge into is sorted only after the first of them.
    pub(crate) fn sort(&self) {
        // The responses of a topic, in a game that sorts them by their
        // `PNAM` (`wbCanSortINFO`).
        if self.sort_topic(false) {
            return;
        }
        if self.gr_struct().group_type == 7 || self.gr_sorted.load(Ordering::Relaxed) {
            return;
        }
        self.container.sort_by(compare_group_contents);
        self.gr_sorted.store(true, Ordering::Relaxed);
    }
}

/// Port of `FindSortElement`: a group of the children of a record sorts
/// like that record.
fn find_sort_element(element: &ElementRef) -> ElementRef {
    element
        .as_element_impl()
        .and_then(|element| element.group_record_impl())
        .and_then(|group| group.children_of())
        .map_or_else(|| element.clone(), |record| record as ElementRef)
}

/// Port of `TwbMainRecord.GetSortPriority`.
fn sort_priority(record: &MainRecordImpl) -> i32 {
    match &record.mr_struct().signature.0 {
        b"ROAD" | b"LAND" => -2,
        b"CELL" | b"PGRD" | b"NAVM" => -1,
        _ => 0,
    }
}

/// Port of `CompareGroupContents`: the order of the elements of a group.
/// Main records sort by priority, then FormID; the groups of the children
/// of a record follow the record.
fn compare_group_contents(a: &ElementRef, b: &ElementRef) -> std::cmp::Ordering {
    use std::cmp::Ordering::{Equal, Greater, Less};
    if Arc::ptr_eq(a, b) {
        return Equal;
    }
    let sort_a = find_sort_element(a);
    let sort_b = find_sort_element(b);
    let mut result = sort_a.get_element_type().cmp(&sort_b.get_element_type());
    if result == Equal {
        let group_a = sort_a.as_element_impl().and_then(|element| element.group_record_impl());
        let group_b = sort_b.as_element_impl().and_then(|element| element.group_record_impl());
        let record_a = sort_a.as_element_impl().and_then(|element| element.main_record_impl());
        let record_b = sort_b.as_element_impl().and_then(|element| element.main_record_impl());
        if let (Some(group_a), Some(group_b)) = (&group_a, &group_b) {
            result = match group_a.group_type() {
                // `CompareText` on the signatures: case insensitive.
                0 => group_a
                    .gr_struct()
                    .label_signature()
                    .to_string()
                    .to_uppercase()
                    .cmp(&group_b.gr_struct().label_signature().to_string().to_uppercase()),
                2 | 3 => (group_a.group_label() as i32).cmp(&(group_b.group_label() as i32)),
                4 | 5 => {
                    // `LongRecSmall`: the high word, then the low word, as signed.
                    let words = |label: u32| ((label >> 16) as u16 as i16, label as u16 as i16);
                    words(group_a.group_label()).cmp(&words(group_b.group_label()))
                }
                _ => Equal,
            };
        } else if let (Some(record_a), Some(record_b)) = (&record_a, &record_b) {
            result = sort_priority(record_a).cmp(&sort_priority(record_b));
            if result == Equal {
                result = if display_load_order_form_id() {
                    record_a
                        .get_load_order_form_id()
                        .cmp(&record_b.get_load_order_form_id())
                } else {
                    record_a.get_fixed_form_id().cmp(&record_b.get_fixed_form_id())
                };
            }
            if result == Equal {
                result = record_a.get_element_id().cmp(&record_b.get_element_id());
            }
        }
    }
    if result == Equal {
        let a_is_group = !Arc::ptr_eq(a, &sort_a);
        let b_is_group = !Arc::ptr_eq(b, &sort_b);
        result = match (a_is_group, b_is_group) {
            (true, true) => {
                // Both are groups of the same record.
                let group_a = a.as_element_impl().and_then(|element| element.group_record_impl());
                let group_b = b.as_element_impl().and_then(|element| element.group_record_impl());
                match (group_a, group_b) {
                    (Some(group_a), Some(group_b)) => group_a
                        .group_type()
                        .cmp(&group_b.group_type())
                        .then(group_a.group_label().cmp(&group_b.group_label())),
                    _ => Equal,
                }
            }
            (true, false) => Greater,
            (false, true) => Less,
            (false, false) => Equal,
        };
    }
    result
}

/// The state of the decompressed data of a compressed record.
enum DataStorage {
    Unloaded,
    Loaded(Arc<Vec<u8>>),
    Failed,
}

/// The value of `mr_fixed_form_id` before the fixed FormID is computed.
const UNSET_FIXED_FORM_ID: u64 = u64::MAX;

/// Port of `TwbMainRecord`, as far as the structure of the record goes.
pub struct MainRecordImpl {
    self_ref: Weak<MainRecordImpl>,
    base: ElementBase,
    container: ContainerBase,
    file: Weak<FileImpl>,
    bytes: Arc<FileBytes>,
    /// Port of `mrStruct`: the header, which the edits change in place
    /// (`MakeHeaderWriteable`).
    mr_struct: RwLock<MainRecordStruct>,
    mr_def: Option<Arc<MainRecordDef>>,
    /// The range of the record data in the file, after the header.
    dc_data_base: usize,
    dc_data_end: usize,
    /// Port of `mrDataStorage`: the decompressed data of a compressed
    /// record, decompressed on first use and released by `reset`.
    mr_data_storage: std::sync::Mutex<DataStorage>,
    /// The decompressed data lent out by `data`, which `reset` cannot take
    /// back while the borrow lives; kept for the life of the record.
    mr_pinned_data: OnceLock<Option<Arc<Vec<u8>>>>,
    /// Port of `DoInit`: the subrecords are built on first use.
    mr_init: InitOnce,
    mr_editor_id: RwLock<String>,
    mr_full_name: RwLock<String>,
    /// Port of `mrsQuickInitDone` and `csInitOnce`: the subrecords were
    /// built once, so the names are known after a reset.
    mr_names_known: AtomicBool,
    /// The number of times the subrecords were built.
    mr_builds: std::sync::atomic::AtomicU32,
    /// Port of `mrMaster` and `mrOverrides`.
    mr_master: RwLock<Option<Weak<MainRecordImpl>>>,
    mr_overrides: RwLock<Vec<Weak<MainRecordImpl>>>,
    /// Port of `mrFixedFormID`: the fixed FormID once computed, or
    /// `UNSET_FIXED_FORM_ID`; cleared when the FormID changes.
    mr_fixed_form_id: std::sync::atomic::AtomicU64,
    /// Port of `mrDisplayName`: cached for the records of official files,
    /// dropped when a named subrecord changes.
    mr_display_name: RwLock<Option<String>>,
    /// Port of `mrPrecombinedCellID` and `mrPrecombinedID` with the
    /// `mrsHasPrecombinedMesh` state: the cell and mesh of a precombined
    /// reference, checked once.
    mr_precombined: OnceLock<Option<(u32, u32)>>,
    /// Port of `mrsOFSTRemoved` in `mrStates`: the init dropped the offsets
    /// of a worldspace, and `PrepareSave` marks its children modified.
    mr_ofst_removed: AtomicBool,
    /// A record that repeats the FormID of the record before it, which
    /// upstream skips on load.
    mr_duplicate: AtomicBool,
    /// Port of `mrReferences` with `csRefsBuild`, `cntRefsBuildAt` and
    /// `mrsBuildingRef`: the FormIDs the record refers to.
    pub(crate) mr_refs: std::sync::Mutex<refs::RecordRefs>,
    /// Port of `mrReferencedBy` with `mrsReferencedByUnsorted`: the records
    /// that refer to this one (kept by the master only).
    pub(crate) mr_referenced_by: std::sync::Mutex<refs::ReferencedBy>,
    /// Port of `mrsGridCellChecked`, `mrsHasGridCell` and `mrGridCell` as
    /// the init leaves them, which the reference cache keeps.
    pub(crate) mr_grid_cell: std::sync::Mutex<refcache::GridCellState>,
}

impl MainRecordImpl {
    fn create(
        file: &Arc<FileImpl>,
        container: &ElementRef,
        bytes: &[u8],
        offset: &mut usize,
        prev_main_record: Option<&Arc<MainRecordImpl>>,
    ) -> Result<Arc<Self>, LoadError> {
        let header_size = size_of_main_record_struct() as usize;
        let mr_struct = MainRecordStruct::parse(bytes, *offset)
            .ok_or_else(|| LoadError(format!("Unexpected end of file in \"{}\"", file.fl_file_name)))?;
        let dc_data_base = *offset + header_size;
        let dc_data_end = (dc_data_base + mr_struct.data_size as usize).min(bytes.len());
        let mr_def = find_record_def(mr_struct.signature);
        if mr_def.is_none() {
            scan::progress(&format!("Error: unknown record type {}", mr_struct.signature));
        }
        let duplicate = prev_main_record.is_some_and(|prev| prev.mr_struct().form_id == mr_struct.form_id);
        let skipped = |form_id: FormID| {
            // Port of `EwbSkipLoad` for a duplicate FormID: the record is skipped.
            scan::progress(&format!(
                "Skipped Load: Duplicate FormID [{}] in file {}",
                form_id.to_string(true),
                file.fl_file_name
            ));
        };
        if duplicate {
            skipped(mr_struct.form_id);
        }
        let record = Arc::new_cyclic(|self_ref: &Weak<MainRecordImpl>| MainRecordImpl {
            self_ref: self_ref.clone(),
            base: ElementBase::new(Some(container)),
            container: ContainerBase::default(),
            file: Arc::downgrade(file),
            bytes: file.fl_bytes.clone(),
            mr_struct: RwLock::new(mr_struct),
            mr_def,
            dc_data_base,
            dc_data_end,
            mr_data_storage: std::sync::Mutex::new(DataStorage::Unloaded),
            mr_pinned_data: OnceLock::new(),
            mr_init: InitOnce::new(),
            mr_editor_id: RwLock::new(String::new()),
            mr_full_name: RwLock::new(String::new()),
            mr_names_known: AtomicBool::new(false),
            mr_builds: std::sync::atomic::AtomicU32::new(0),
            mr_master: RwLock::new(None),
            mr_overrides: RwLock::new(Vec::new()),
            mr_fixed_form_id: std::sync::atomic::AtomicU64::new(UNSET_FIXED_FORM_ID),
            mr_display_name: RwLock::new(None),
            mr_precombined: OnceLock::new(),
            mr_ofst_removed: AtomicBool::new(false),
            mr_duplicate: AtomicBool::new(duplicate),
            mr_refs: Default::default(),
            mr_referenced_by: Default::default(),
            mr_grid_cell: Default::default(),
        });
        scan::attach(container, record.clone());
        // A record whose FormID the scan saw before (`AddMainRecord`'s
        // `flSetContainsFixedFormID`) is skipped as well.
        if !duplicate && !scan::defer_registration(&record) {
            file.register_scanned(&record);
        }
        *offset = dc_data_end;
        Ok(record)
    }

    pub fn get_signature(&self) -> Signature {
        self.mr_struct().signature
    }

    pub fn form_id(&self) -> FormID {
        self.mr_struct().form_id
    }

    pub fn header_struct(&self) -> MainRecordStruct {
        self.mr_struct()
    }

    /// Port of `mrStruct`: the header as it is now.
    pub fn mr_struct(&self) -> MainRecordStruct {
        *self.mr_struct.read().unwrap()
    }

    pub fn def(&self) -> Option<&Arc<MainRecordDef>> {
        self.mr_def.as_ref()
    }

    pub(crate) fn cache_editor_id(&self, editor_id: String) {
        *self.mr_editor_id.write().unwrap() = editor_id;
    }

    pub(crate) fn set_full_name(&self, full_name: String) {
        *self.mr_full_name.write().unwrap() = full_name;
    }

    /// Port of `GetFullName`, read while the subrecords are built.
    pub fn get_full_name(&self) -> String {
        if self.can_have(KnownSubRecord::ksrFullName) {
            self.self_arc().quick_init();
        }
        self.mr_full_name.read().unwrap().clone()
    }

    /// Port of `GetCanHaveEditorID` and `GetCanHaveFullName`.
    fn can_have(&self, known: KnownSubRecord) -> bool {
        self.mr_def
            .as_ref()
            .is_some_and(|def| def.get_contains_known_sub_record(known))
    }

    /// Port of the quick init of `GetEditorID` and `GetFullName`: a record
    /// whose subrecords were never built builds them to read its names and
    /// resets at once (`DoReset(True)`), so that a record read only for its
    /// name does not keep its subrecords. The names stay cached
    /// (`mrsQuickInitDone`, `csInitOnce`).
    /// UPSTREAM-QUIRK: upstream builds only the subrecords up to
    /// `QuickInitLimit`; the port builds all of them, which reads the same
    /// names. While the record's own init runs, upstream returns `<EditorID
    /// not yet available: init still running>`; the port returns the names
    /// read so far, as it did before the quick init was ported.
    ///
    /// The subrecords are released before the build counts as done
    /// (`InitOnce::run_then`), so no other thread sees them: a thread that
    /// needs the record meanwhile waits and builds it itself. A record built
    /// already, by any thread, stays built.
    fn quick_init(self: &Arc<Self>) {
        if self.mr_names_known.load(Ordering::Acquire) || self.mr_init.is_running_here() {
            return;
        }
        self.mr_init.run_then(
            || !self.mr_names_known.load(Ordering::Acquire),
            || self.build(),
            || self.release_elements_and_data(),
        );
    }

    /// Port of `FixedFormID`. The hardcoded range of the game master is
    /// not adjusted yet.
    /// Port of `GetFixedFormID` and `DoGetFixedFormID` without the complex
    /// FileIDs: the FormID with the FileID of the game master for the
    /// hardcoded range, and the file's own FileID for a slot beyond the
    /// masters.
    pub fn get_fixed_form_id(&self) -> FormID {
        let cached = self.mr_fixed_form_id.load(Ordering::Acquire);
        if cached != UNSET_FIXED_FORM_ID {
            return FormID::from_cardinal(cached as u32);
        }
        let result = (|| {
            let mut result = self.mr_struct().form_id;
            let Some(file) = self.file.upgrade() else {
                return result;
            };
            if result.object_id() < 0x800 {
                if file.get_allow_hardcoded_range_use() {
                    if result.is_hardcoded() {
                        return result;
                    }
                } else {
                    return result.change_file_id(FileID::null());
                }
            }
            // `IsNewRecord`, by the slot of the module type under
            // `wbComplexFileFileID`.
            if file.is_new_record(result.file_id()) {
                result = result.change_file_id(file.get_file_file_id());
            }
            result
        })();
        self.mr_fixed_form_id
            .store(u64::from(result.to_cardinal()), Ordering::Release);
        result
    }

    /// Port of `mrFixedFormID := TwbFormID.Null`: the fixed FormID is
    /// computed again on the next use, after the FormID or the masters
    /// changed.
    pub(crate) fn clear_fixed_form_id(&self) {
        self.mr_fixed_form_id.store(UNSET_FIXED_FORM_ID, Ordering::Release);
    }

    /// Port of `ActivateIndexKeys` and `BuildIndexKeys`: the keys of the
    /// record in the named indices, from the editor ID when the definition
    /// indexes it and from the callback of the definition.
    fn activate_index_keys(self: &Arc<Self>) -> Vec<(i32, String)> {
        let Some(def) = &self.mr_def else {
            return Vec::new();
        };
        let mut keys = IndexKeys::default();
        let mut result = false;
        if (def.get_contains_known_sub_record(KnownSubRecord::ksrEditorID) && track_all_editor_id())
            || def.get_def_flags().contains(DefFlag::dfIndexEditorID)
        {
            result = true;
            keys.set_key(idx_editor_id(), &self.get_editor_id());
        }
        let main_record: MainRecordRef = self.clone();
        if def.build_index_keys(&main_record, &mut keys) {
            result = true;
        }
        if result { keys.defined_keys() } else { Vec::new() }
    }

    /// Port of `GetPrecombinedMesh` up to the cache check: the cell FormID
    /// object ID and the mesh ID of a precombined placed record.
    fn precombined(self: &Arc<Self>) -> Option<(u32, u32)> {
        *self.mr_precombined.get_or_init(|| {
            if !matches!(game_mode(), GameMode::gmFO4 | GameMode::gmFO4VR | GameMode::gmFO76) {
                return None;
            }
            self.file.upgrade()?;
            if !matches!(
                self.mr_struct().signature.0.as_slice(),
                b"REFR" | b"PGRE" | b"PMIS" | b"PARW" | b"PBEA" | b"PFLA" | b"PCON" | b"PBAR" | b"PHZD"
            ) {
                return None;
            }
            // Markers can't be precombined.
            let def = self.mr_def.as_ref()?;
            let base = def.known_sub_record_signatures()[KnownSubRecord::ksrBaseRecord.ord()];
            let base_form_id = self
                .get_element_native_value(&base.to_string())
                .as_ordinal()
                .unwrap_or(0) as u32;
            if base_form_id < 0x800 {
                return None;
            }
            let cell = self
                .base
                .container()
                .and_then(|container| container.as_element_impl()?.group_record_impl())
                .and_then(|group| group.children_of())?;
            let cell_form_id = cell.mr_struct().form_id;
            // Port of `PrecombinedCache`: the references of the cell, kept
            // between the records of the same cell and file.
            let file_name = self.file.upgrade().map(|file| file.get_name()).unwrap_or_default();
            let own = self.mr_struct().form_id.to_cardinal();
            let cached = PRECOMBINED_CACHE.with_borrow(|cache| {
                cache
                    .as_ref()
                    .filter(|(form_id, name, _)| *form_id == cell_form_id && *name == file_name)
                    .map(|(_, _, entries)| entries.clone())
            });
            let entries = if let Some(entries) = cached {
                entries
            } else {
                let mut entries = Vec::new();
                let ordinal = |element: Option<ElementRef>| {
                    element.map_or(0, |element| element.get_native_value().as_ordinal().unwrap_or(0)) as u32
                };
                if game_mode() == GameMode::gmFO76 {
                    if let Some(refs) = cell.get_element_by_path("XCRP\\References")
                        && let Some(refs) = refs.as_container()
                    {
                        for index in 0..refs.get_element_count() {
                            entries.push((ordinal(refs.get_element(index)), 0));
                        }
                    }
                } else if let Some(refs) = cell.get_element_by_path("XCRI\\References")
                    && let Some(refs) = refs.as_container()
                {
                    for index in 0..refs.get_element_count() {
                        let Some(pair) = refs.get_element(index) else { continue };
                        let Some(pair) = pair.as_container() else { continue };
                        if pair.get_element_count() != 2 {
                            continue;
                        }
                        entries.push((ordinal(pair.get_element(0)), ordinal(pair.get_element(1))));
                    }
                }
                let entries = Arc::new(entries);
                PRECOMBINED_CACHE.set(Some((cell_form_id, file_name, entries.clone())));
                entries
            };
            entries
                .iter()
                .find(|(reference, _)| *reference == own)
                .map(|(_, id)| (cell_form_id.object_id(), *id))
        })
    }

    /// Port of `Master` (`mrMaster`): the record this one overrides.
    pub fn master(&self) -> Option<Arc<MainRecordImpl>> {
        self.mr_master.read().unwrap().as_ref().and_then(Weak::upgrade)
    }

    /// Port of `Overrides` (`mrOverrides`), in load order.
    pub fn overrides(&self) -> Vec<Arc<MainRecordImpl>> {
        self.mr_overrides
            .read()
            .unwrap()
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    }

    /// Port of `IsWinningOverride`: no override in a later file.
    pub(crate) fn is_winning_override(self: &Arc<Self>) -> bool {
        let winning = self.get_winning_override();
        winning.get_element_id() == self.get_element_id()
    }

    /// Port of `AddOverride`.
    /// The overrides stay in load order (`mrsOverridesSorted`), so that an
    /// override an edit adds to a file before the last one takes its place.
    fn add_override(self: &Arc<Self>, record: &Arc<MainRecordImpl>) {
        *record.mr_master.write().unwrap() = Some(Arc::downgrade(self));
        let load_order = record.file_impl().map_or(i32::MAX, |file| file.load_order());
        let mut overrides = self.mr_overrides.write().unwrap();
        let position = overrides
            .iter()
            .rposition(|other| {
                other
                    .upgrade()
                    .and_then(|other| other.file_impl())
                    .is_none_or(|file| file.load_order() <= load_order)
            })
            .map_or(0, |position| position + 1);
        overrides.insert(position, Arc::downgrade(record));
    }

    fn file_impl(&self) -> Option<Arc<FileImpl>> {
        self.file.upgrade()
    }

    /// Port of `GetHighestOverrideVisibleForFile`: the override of the
    /// record in `file` or its masters, or the record itself.
    pub fn highest_override_visible_for_file(self: &Arc<Self>, file: &Arc<FileImpl>) -> Arc<MainRecordImpl> {
        let form_id = self.get_load_order_form_id();
        let mut result = self.clone();
        let own_load_order = self.file_impl().map_or(-1, |own| own.load_order());
        if file.load_order() > own_load_order || self.get_is_partial_form() {
            if let Some(record) = file.contained_record_by_load_order_form_id(form_id)
                && !record.get_is_partial_form()
            {
                result = record;
            } else {
                for master in file.masters().iter().rev() {
                    let result_load_order = result.file_impl().map_or(-1, |own| own.load_order());
                    if (master.load_order() > result_load_order || result.get_is_partial_form())
                        && let Some(record) = master.contained_record_by_load_order_form_id(form_id)
                        && !record.get_is_partial_form()
                    {
                        result = record;
                    }
                }
            }
        }
        result
    }

    /// Port of `GetShortNameInternal`; the GUI's `wbDisplayShorterNames`
    /// gives `EditorID "Full Name" [SIG:FormID]`.
    fn short_name_internal(&self) -> String {
        if crate::interface::globals::display_shorter_names() {
            let mut result = self.get_editor_id();
            let full_name = self.get_full_name();
            if !full_name.is_empty() {
                if !result.is_empty() {
                    result.push(' ');
                }
                result.push_str(&format!("\"{}\"", full_name.replace('"', "\"\"")));
            }
            if !result.is_empty() {
                result.push(' ');
            }
            let form_id = if display_load_order_form_id() {
                self.get_load_order_form_id()
            } else {
                self.mr_struct().form_id
            };
            result.push_str(&format!("[{}:{}]", self.mr_struct().signature, form_id.to_string(true)));
            return result;
        }
        let mut result = self.mr_struct().signature.to_string();
        if let Some(def) = &self.mr_def {
            result = format!("{result} - {}", def.get_name());
        }
        if display_load_order_form_id() {
            result = format!("{result} [{}]", self.get_load_order_form_id().to_string(true));
        } else {
            result = format!("{result} [{}]", self.mr_struct().form_id.to_string(true));
        }
        let editor_id = self.get_editor_id();
        if !editor_id.is_empty() {
            result = format!("{result} <{editor_id}>");
        }
        let full_name = self.get_full_name();
        if !full_name.is_empty() {
            result = format!("{result} \"{}\"", full_name.replace('"', "\"\""));
        }
        result
    }

    /// `read` on the elements once they are built. It runs under the lock
    /// that `reset` takes to release them, so it never sees the elements of
    /// a record that another thread reset in the meantime: the record is
    /// built again instead (`crate::threads`). Inside its own build, or when
    /// two builds needed each other (`init_cycles`), it sees the elements
    /// built so far, as upstream does.
    fn read_built<T>(&self, read: impl Fn(&[ElementRef]) -> T) -> T {
        let this = self.self_arc();
        loop {
            let cycles = crate::threads::init_cycles();
            this.do_init();
            let elements = self.container.cnt_elements.read().unwrap();
            if self.mr_init.is_done()
                || self.mr_init.is_running_here()
                || (self.mr_init.is_running() && crate::threads::init_cycles() != cycles)
            {
                return read(&elements);
            }
        }
    }

    /// Port of `DoInit`: builds the subrecords once. Returns whether this
    /// call built them.
    pub fn do_init(self: &Arc<Self>) -> bool {
        self.mr_init.run(|| self.build())
    }

    /// The init of `do_init`.
    fn build(self: &Arc<Self>) {
        {
            let build = self.mr_builds.fetch_add(1, Ordering::Relaxed) + 1;
            INITIALIZED_RECORDS
                .lock()
                .unwrap()
                .push_back((Arc::downgrade(self), build));
            INITIALIZED_COUNT.fetch_add(1, Ordering::Relaxed);
            self.create_contained_in();
            self.create_record_header();
            sub_record::init_main_record(self);
            self.mr_names_known.store(true, Ordering::Release);
            // Port of the `wbRemoveOffsetData` step of `TwbMainRecord.Init`:
            // the offsets of a worldspace are dropped.
            if remove_offset_data() && self.mr_struct().signature == Signature::new(b"WRLD") {
                let position = self
                    .container
                    .elements()
                    .iter()
                    .position(|element| element.get_record_signature() == Some(Signature::new(b"OFST")));
                if let Some(position) = position {
                    // Upstream removes the subrecord inside
                    // `wbBeginInternalEdit(True)`, so the record is modified
                    // internally (`esInternalModified`, not `esUnsaved`) and is
                    // written from its elements on save.
                    let internal = crate::interface::globals::begin_internal_edit(true);
                    self.container.remove_element(position);
                    self.set_modified(true);
                    if internal {
                        crate::interface::globals::end_internal_edit();
                    }
                    self.mr_ofst_removed.store(true, Ordering::Relaxed);
                }
            }
            // `mrDef.AfterLoad(Self)`, before the required members are added.
            if let Some(def) = &self.mr_def {
                let self_ref: ElementRef = self.clone();
                def.after_load(&self_ref);
            }
            sub_record::add_required_members(self);
            self.sort_info_after_init();
        }
    }

    /// Port of `TwbMainRecord.Reset` through `DoReset(False)`: the
    /// subrecords built by `do_init` are released and the next use builds
    /// them again. In Delphi this runs when the last `IwbContainerElementRef`
    /// of the record goes away, which keeps the memory of a dump bounded to
    /// one record at a time. The decompressed data is released as upstream
    /// `mrDataStorage`; elements that outlive the reset keep their own
    /// reference to it.
    ///
    /// The elements are released and the init undone under the lock that
    /// `read_built` reads under, so another thread that reads the record
    /// meanwhile builds it again (`crate::threads`).
    pub fn reset(&self) {
        let released = {
            let mut elements = self.container.cnt_elements.write().unwrap();
            // A record whose init runs keeps its elements and data.
            if self.mr_init.is_running() {
                return;
            }
            // Port of the `esModified` check of `TwbContainer.DoReset`: a
            // modified record keeps its elements, which hold the change.
            if self.base.has_state(ElementState::esModified) {
                return;
            }
            self.mr_init.reset();
            std::mem::take(&mut *elements)
        };
        crate::threads::retire(released);
        self.release_data();
    }

    /// The release of the quick init: the elements are given up before the
    /// build counts as done. False for a modified record, which keeps them.
    fn release_elements_and_data(&self) -> bool {
        if self.base.has_state(ElementState::esModified) {
            return false;
        }
        self.container.release_elements();
        self.release_data();
        true
    }

    /// Port of the release of `mrDataStorage`.
    fn release_data(&self) {
        let mut storage = self.mr_data_storage.lock().unwrap();
        if matches!(*storage, DataStorage::Loaded(_)) {
            *storage = DataStorage::Unloaded;
        }
    }

    /// Port of the `TwbContainedInElement` creation in `TwbMainRecord.Init`:
    /// a record in a worldspace, cell or topic group gets an element that
    /// holds the FormID of the parent record.
    fn create_contained_in(self: &Arc<Self>) {
        if !create_contained_in() {
            return;
        }
        let Some(mut group) = self
            .base
            .container()
            .and_then(|c| c.as_element_impl()?.group_record_impl())
        else {
            return;
        };
        if !matches!(group.group_type(), 1 | 4..=10) {
            return;
        }
        // Port of the walk up in `TwbContainedInElement.Create`.
        if group.group_type() == 5
            && let Some(parent) = group.parent_group()
        {
            group = parent;
        }
        if group.group_type() == 4
            && let Some(parent) = group.parent_group()
        {
            group = parent;
        }
        let children = if vwd_as_quest_children() { 8..=9 } else { 8..=10 };
        if children.contains(&group.group_type())
            && let Some(parent) = group.parent_group()
        {
            group = parent;
        }
        let (name, signature) = match group.group_type() {
            1 => ("Worldspace", b"WRLD"),
            6 => ("Cell", b"CELL"),
            7 => ("Topic", b"DIAL"),
            10 if vwd_as_quest_children() => ("Quest", b"QUST"),
            _ => return,
        };
        let def = contained_in_def(group.group_type(), name, Signature::new(signature));
        let self_ref: ElementRef = self.clone();
        let element = value::create_contained_in_element(&self_ref, &self.file, def, group.group_label());
        self.container.add_element(element);
    }

    /// Port of the `TwbRecordHeaderStruct` creation in `TwbMainRecord.Init`:
    /// a structure over the header bytes of the record, with the header
    /// definition of the record definition or the shared one.
    fn create_record_header(self: &Arc<Self>) {
        let header_def = match &self.mr_def {
            Some(def) => def.get_record_header_struct(),
            None => main_record_header().and_then(|header| header.into_struct_def()),
        };
        let Some(header_def) = header_def else { return };
        let header_size = size_of_main_record_struct() as usize;
        // The element reads the header from `mrStruct`, so that the edits of
        // the header (`MakeHeaderWriteable`) and of the element agree.
        let bytes = self.mr_struct().to_bytes();
        let self_ref: ElementRef = self.clone();
        let mut cursor = value::Cursor {
            block: DataBlock::Buffer(Arc::new(bytes)),
            pos: 0,
            end: header_size,
        };
        let element =
            value::create_value_element(&self_ref, &self.file, &mut cursor, header_def as Arc<dyn ValueDef>, "");
        element.set_sort_and_memory_order(-1);
        // `Include(dcFlags, dcfDontSave)`: the header is written from `mrStruct`.
        element.vb.dont_save.store(true, Ordering::Relaxed);
        element.vb.record_header.store(true, Ordering::Relaxed);
    }

    /// The record header element (`GetElementBySortOrder(-1 + additional)`).
    fn record_header_element(&self) -> Option<Arc<value::ValueImpl>> {
        self.container
            .elements()
            .into_iter()
            .find(|element| element.get_sort_order() == -1)
            .and_then(|element| element.as_element_impl()?.value_impl())
    }

    /// The common part of `TwbMainRecord.SetEditValue` and `SetNativeValue`.
    fn set_form_id_value(&self, form_id: FormID) -> Result<(), EditError> {
        if !is_internal_edit() {
            if !edit_allowed() {
                return Err(format!("{} can not be edited.", self.get_name()));
            }
            if self
                .mr_def
                .as_ref()
                .is_some_and(|def| def.def_base().def_internal_edit_only())
            {
                return Ok(());
            }
        }
        if !display_load_order_form_id() {
            return Err("FormID can only be edited if wbDisplayLoadOrderFormID is active".to_owned());
        }
        self.self_arc().set_load_order_form_id(form_id)?;
        if let Some(container) = self.base.container()
            && let Some(container) = container.as_element_impl()
        {
            container.notify_changed();
        }
        Ok(())
    }

    /// Port of `MakeHeaderWriteable`: the record is modified, and the
    /// header element reads the header again after `change` edits it.
    pub(crate) fn make_header_writeable(self: &Arc<Self>, change: impl FnOnce(&mut MainRecordStruct)) {
        self.do_init();
        self.set_modified(true);
        edit::invalidate_parent_storage(&**self);
        change(&mut self.mr_struct.write().unwrap());
        self.refresh_record_header();
    }

    /// Port of `InformStorage` on the record header element: its data is
    /// `mrStruct` again, and its elements are built over it.
    fn refresh_record_header(&self) {
        if let Some(header) = self.record_header_element() {
            header.replace_data(self.mr_struct().to_bytes());
        }
    }

    /// Port of `SetIsCompressed`.
    pub fn set_is_compressed(self: &Arc<Self>, value: bool) {
        if value != self.mr_struct().flags.is_compressed() {
            self.make_header_writeable(|header| header.flags.set_compressed(value));
        }
    }

    /// Port of `SetIsESM`.
    pub fn set_is_esm(self: &Arc<Self>, value: bool) {
        if value != self.mr_struct().flags.is_esm() {
            self.make_header_writeable(|header| header.flags.set_esm(value));
        }
    }

    /// Port of `SetIsLight`.
    pub fn set_is_light(self: &Arc<Self>, value: bool) {
        if value != self.mr_struct().flags.is_light() {
            self.make_header_writeable(|header| header.flags.set_light(value));
        }
    }

    /// Port of `SetIsUpdate`.
    pub fn set_is_update(self: &Arc<Self>, value: bool) {
        if value != self.mr_struct().flags.is_update() {
            self.make_header_writeable(|header| header.flags.set_update(value));
        }
    }

    /// Port of `SetIsMedium`.
    pub fn set_is_medium(self: &Arc<Self>, value: bool) {
        if value != self.mr_struct().flags.is_medium() {
            self.make_header_writeable(|header| header.flags.set_medium(value));
        }
    }

    /// Port of `SetIsBlueprint`.
    pub fn set_is_blueprint(self: &Arc<Self>, value: bool) {
        if value != self.mr_struct().flags.is_blueprint() {
            self.make_header_writeable(|header| header.flags.set_blueprint(value));
        }
    }

    /// Port of `SetIsLocalized`.
    pub fn set_is_localized(self: &Arc<Self>, value: bool) {
        if value != self.mr_struct().flags.is_localized() {
            self.make_header_writeable(|header| header.flags.set_localized(value));
        }
    }

    /// Port of `SetIsPersistent`: a placed record moves to the children
    /// group of its cell the flag calls for (`UpdateCellChildGroup`).
    pub fn set_is_persistent(self: &Arc<Self>, value: bool) {
        if value != self.mr_struct().flags.is_persistent() {
            let need_update = self.check_child_of_cell();
            self.make_header_writeable(|header| header.flags.set_persistent(value));
            if need_update {
                self.update_cell_child_group();
            }
        }
    }

    /// Port of `SetIsVisibleWhenDistant`, with `UpdateCellChildGroup`.
    pub fn set_is_visible_when_distant(self: &Arc<Self>, value: bool) {
        if value != self.mr_struct().flags.is_visible_when_distant() {
            let need_update = self.check_child_of_cell();
            self.make_header_writeable(|header| header.flags.set_visible_when_distant(value));
            if need_update {
                self.update_cell_child_group();
            }
        }
    }

    /// Port of `ClampFormID`: a FormID beyond the masters of the file is
    /// moved to the file itself, and the child group follows.
    pub(crate) fn clamp_form_id(self: &Arc<Self>, index: i32) {
        if game_mode() == GameMode::gmTES3 || crate::interface::globals::complex_file_file_id() {
            return;
        }
        let form_id = self.mr_struct().form_id;
        let slot = i32::from(form_id.file_id().full_slot());
        if slot > index {
            let clamped = form_id.change_file_id(FileID::create_full(index as i16));
            // `mrGroup` is the group found by the old FormID.
            let group = self.child_group();
            self.make_header_writeable(|header| header.form_id = clamped);
            if let Some(group) = group {
                // The child groups of a record can take a label.
                let _ = group.set_group_label(clamped.to_cardinal());
            }
        } else if slot == index
            && let Some(group) = self.child_group()
        {
            let _ = group.set_group_label(form_id.to_cardinal());
        }
    }

    /// Port of `TwbRecordHeaderStruct.ElementChanged`: an edit of the
    /// `Record Flags` of the header element goes into `mrStruct`, with the
    /// ESM flag kept off outside the file header.
    pub(crate) fn record_header_changed(self: &Arc<Self>, child: &ElementRef) {
        let Some(header) = self.record_header_element() else {
            return;
        };
        let is_flags = child
            .get_def()
            .is_some_and(|def| def.get_name().eq_ignore_ascii_case("Record Flags"));
        if is_flags
            && let Some(data) = child.as_data_container().and_then(|data| data.get_data())
            && data.len() >= 4
        {
            let mut flags = MainRecordStructFlags(u32::from_le_bytes([data[0], data[1], data[2], data[3]]));
            if flags.is_esm() && self.mr_struct().signature != header_signature() {
                flags.set_esm(false);
            }
            let old = self.mr_struct().flags;
            // The deleted, partial form, persistent and visible when distant
            // flags change through their setters, which keep the record's
            // state in step; the first two are not ported yet and keep
            // their value.
            flags.set_deleted(old.is_deleted());
            flags.set_partial_form(old.is_partial_form());
            let persistent = flags.is_persistent();
            let visible_when_distant = flags.is_visible_when_distant();
            flags.set_persistent(old.is_persistent());
            flags.set_visible_when_distant(old.is_visible_when_distant());
            self.make_header_writeable(|header| header.flags = flags);
            if !old.is_deleted() {
                self.set_is_persistent(persistent);
                self.set_is_visible_when_distant(visible_when_distant);
            }
        }
        header.replace_data(self.mr_struct().to_bytes());
    }

    /// Port of `TwbMainRecord.Add`: a member of the record, or a child of a
    /// cell, a topic, a worldspace or a quest (placed records, responses,
    /// cells, scenes) in the child group.
    pub(crate) fn add(self: &Arc<Self>, name: &str, silent: bool) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && (!edit_allowed() || !self.get_is_editable_impl()) {
            return Err(format!("{} can not be edited", self.get_name()));
        }
        if self.get_is_deleted() {
            return Ok(None);
        }
        if let Some(added) = self.add_child(name, silent)? {
            return Ok(added);
        }
        let Some(mr_def) = &self.mr_def else { return Ok(None) };
        self.do_init();
        let count = usize::try_from(mr_def.get_member_count()).unwrap_or(0);
        for index in 0..count {
            let member = mr_def.get_member(index);
            if member.get_name().eq_ignore_ascii_case(name)
                || member.get_default_signature().to_string().eq_ignore_ascii_case(name)
            {
                let sort_order = index as i32;
                if let Some(existing) = self.container.element_by_sort_order(sort_order) {
                    return Ok(Some(existing));
                }
                self.assign_member(index)?;
                return Ok(self.container.element_by_sort_order(sort_order));
            }
        }
        Ok(None)
    }

    /// Port of `TwbMainRecord.AssignInternal(aIndex, nil, False)` for a
    /// member index: the member is created from its definition when the
    /// record does not have it.
    pub(crate) fn assign_member(self: &Arc<Self>, index: usize) -> Result<Option<ElementRef>, EditError> {
        self.assign_internal_impl(index as i32, None, false)
    }

    fn self_arc(&self) -> Arc<Self> {
        self.self_ref
            .upgrade()
            .expect("a main record is alive while it is used")
    }

    /// Port of `GetChildGroup` (`mrGroup`): the group of the children of
    /// this record. The group that follows the record with its FormID as
    /// label (`InformPrevMainRecord`, types 1, 6 and 7), else the group of
    /// the record's child type with that label in the containing group
    /// (`FindChildGroup`; a quest's group of type 10 under
    /// `wbVWDAsQuestChildren`). Labels compare as `GetGroupLabel` gives
    /// them, with a FileID beyond the masters read as the file's own.
    pub fn child_group(&self) -> Option<Arc<GroupRecordImpl>> {
        let container = self.base.container()?;
        let base = container.as_element_impl()?.container_base()?;
        let form_id = self.mr_struct().form_id.to_cardinal();
        let elements = base.elements();
        let index = elements
            .iter()
            .position(|element| std::ptr::addr_eq(Arc::as_ptr(element), self as *const Self))?;
        if let Some(group) = elements
            .get(index + 1)
            .and_then(|e| e.as_element_impl()?.group_record_impl())
            && matches!(group.group_type(), 1 | 6 | 7)
            && group.get_group_label() == form_id
        {
            return Some(group);
        }
        let wanted = match &self.mr_struct().signature.0 {
            b"WRLD" => 1,
            b"CELL" => 6,
            b"DIAL" => 7,
            b"QUST" if vwd_as_quest_children() => 10,
            _ => return None,
        };
        let containing = container.as_element_impl()?.group_record_impl()?;
        containing.find_child_group(wanted, form_id)
    }

    /// The record data as stored in the file, after the header.
    fn raw_data(&self) -> DataPtr<'_> {
        self.bytes.as_slice().get(self.dc_data_base..self.dc_data_end)
    }

    /// Port of `DecompressIfNeeded` for a compressed record: the
    /// decompressed data, `None` when the decompression fails. A failure
    /// is reported once and stays.
    fn decompressed(&self) -> Option<Arc<Vec<u8>>> {
        let mut storage = self.mr_data_storage.lock().unwrap();
        match &*storage {
            DataStorage::Loaded(data) => return Some(data.clone()),
            DataStorage::Failed => return None,
            DataStorage::Unloaded => {}
        }
        let decompressed = (|| {
            let raw = self.raw_data()?;
            let uncompressed_length = u32::from_le_bytes(raw.get(..4)?.try_into().ok()?) as usize;
            if uncompressed_length == 0 {
                return Some(Arc::new(Vec::new()));
            }
            let mut data = vec![0u8; uncompressed_length];
            match xedit_io::CompressionType::ZLib.decompress(raw.get(4..)?, &mut data) {
                Ok(()) => Some(Arc::new(data)),
                Err(error) => {
                    progress(&format!(
                        "<Error decompressing [{}:{}]: {error}>",
                        self.mr_struct().signature,
                        self.mr_struct().form_id.to_string(false)
                    ));
                    None
                }
            }
        })();
        *storage = match &decompressed {
            Some(data) => DataStorage::Loaded(data.clone()),
            None => DataStorage::Failed,
        };
        decompressed
    }

    /// Port of `DecompressIfNeeded`: the record data, decompressed when the
    /// record is compressed. `None` when the decompression fails. The
    /// decompressed data of a borrow stays for the life of the record; the
    /// element tree reads it through `data_block`, which `reset` releases.
    pub fn data(&self) -> DataPtr<'_> {
        let raw = self.raw_data()?;
        if !self.mr_struct().flags.is_compressed() {
            return Some(raw);
        }
        self.mr_pinned_data
            .get_or_init(|| self.decompressed())
            .as_deref()
            .map(Vec::as_slice)
    }

    /// The size of the record data, decompressed when the record is
    /// compressed.
    fn data_len(&self) -> Option<usize> {
        if self.mr_struct().flags.is_compressed() {
            self.decompressed().map(|data| data.len())
        } else {
            self.raw_data().map(<[u8]>::len)
        }
    }

    /// The block the data of the record lives in, with the data range.
    pub(crate) fn data_block(&self) -> Option<(DataBlock, usize, usize)> {
        if self.mr_struct().flags.is_compressed() {
            let storage = self.decompressed()?;
            let len = storage.len();
            Some((DataBlock::Buffer(storage), 0, len))
        } else {
            Some((DataBlock::File(self.bytes.clone()), self.dc_data_base, self.dc_data_end))
        }
    }
}

// ----- the element casts -----

/// The casts between the element objects of this module.
pub trait ElementImpl: Element {
    fn element_base(&self) -> &ElementBase;

    /// The element as a reference, for the callbacks that take one. `None`
    /// while the element is being created.
    fn self_element_ref(&self) -> Option<ElementRef>;

    /// Port of `BeginResolve`: whether this element may resolve a definition
    /// now; false while this thread is resolving one through it already
    /// (`esResolving`, per thread as in `crate::threads`).
    fn begin_resolve(&self) -> bool {
        crate::threads::begin_resolve(std::ptr::from_ref(self.element_base()) as usize)
    }

    /// Port of `EndResolve`.
    fn end_resolve(&self) {
        crate::threads::end_resolve(std::ptr::from_ref(self.element_base()) as usize);
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        None
    }

    fn main_record_impl(&self) -> Option<Arc<MainRecordImpl>> {
        None
    }

    fn file_impl(&self) -> Option<Arc<FileImpl>> {
        None
    }

    fn group_record_impl(&self) -> Option<Arc<GroupRecordImpl>> {
        None
    }

    fn sub_record_impl(&self) -> Option<&sub_record::SubRecordImpl> {
        None
    }

    fn sub_record_array_impl(&self) -> Option<&sub_record::SubRecordArrayImpl> {
        None
    }

    fn value_impl(&self) -> Option<Arc<value::ValueImpl>> {
        None
    }

    /// Port of `SetSortOrder` and `SetMemoryOrder` with the same value.
    fn set_sort_and_memory_order(&self, order: i32) {
        self.element_base().e_sort_order.store(order, Ordering::Relaxed);
        self.element_base().e_memory_order.store(order, Ordering::Relaxed);
    }

    // ----- the write path (`write`) -----

    /// This element as a trait object, for the shared defaults below.
    fn as_dyn_element_impl(&self) -> Option<ElementRef> {
        self.self_element_ref()
    }

    /// Port of `TwbElement.SetModified`: marks the element and its
    /// containers modified. `false` does nothing, as upstream.
    fn set_modified(&self, value: bool) {
        if let Some(this) = self.as_dyn_element_impl()
            && let Some(this) = this.as_element_impl()
        {
            write::element_set_modified(this, value);
        }
    }

    /// Port of `TwbElement.SetParentModified`.
    fn set_parent_modified(&self) {
        if let Some(container) = self.element_base().container()
            && let Some(container) = container.as_element_impl()
        {
            container.set_modified(true);
        }
    }

    /// Port of `WriteToStream` with `WriteToStreamInternal`: appends the
    /// bytes of the element to `out`. The default is `TwbContainer`'s:
    /// the elements in order, then the states reset.
    fn write_to_stream(&self, out: &mut Vec<u8>, reset: ResetModified) -> Result<(), SaveError> {
        let this = self
            .as_dyn_element_impl()
            .ok_or_else(|| SaveError::Internal("an element was written while being created".to_owned()))?;
        let this = this
            .as_element_impl()
            .ok_or_else(|| SaveError::Internal("an element without a write path".to_owned()))?;
        write::container_write_to_stream(this, out, reset)
    }

    /// Port of `PrepareSave`. The default is `TwbContainer`'s: the elements
    /// in reverse order, unless the records load delayed and this element
    /// is unmodified.
    fn prepare_save(&self) -> Result<(), SaveError> {
        match self
            .as_dyn_element_impl()
            .as_ref()
            .and_then(|this| this.as_element_impl())
        {
            Some(this) => write::container_prepare_save(this),
            None => Ok(()),
        }
    }

    /// Port of `GetCountedRecordCount`: the records and groups below this
    /// element, as the file header counts them.
    fn get_counted_record_count(&self) -> u32 {
        self.as_dyn_element_impl()
            .as_ref()
            .and_then(|this| this.as_element_impl())
            .map_or(0, write::container_counted_record_count)
    }

    // ----- the editing path (`edit`) -----

    /// The storage of a data container (`dcDataStorage`); `None` for an
    /// element without data of its own.
    fn storage(&self) -> Option<&edit::Storage> {
        None
    }

    /// Port of `GetDataPrefixSize`: the bytes before the elements that the
    /// element keeps itself, such as the count of an array.
    fn get_data_prefix_size(&self) -> usize {
        0
    }

    /// The data without rebuilding stale storage (`dcDataBasePtr` read
    /// directly): the storage, else the bytes as loaded.
    fn raw_data(&self) -> DataPtr<'_> {
        None
    }

    /// The data with stale storage rebuilt first (`GetDataBasePtr`).
    fn current_data(&self) -> DataPtr<'_> {
        self.update_storage_from_elements();
        self.raw_data()
    }

    /// Port of `dcfDontSave` with `dfDontSave`: the element is not written
    /// and not merged into its container's data.
    fn dont_save(&self) -> bool {
        false
    }

    /// Port of `csInitializing`: the elements are being built.
    fn init_running(&self) -> bool {
        false
    }

    /// Port of `IsFlags`: a value whose elements are its flags.
    fn is_flags(&self) -> bool {
        false
    }

    /// Port of `UpdateStorageFromElements`.
    fn update_storage_from_elements(&self) {
        if let Some(this) = self.as_dyn_element_impl()
            && let Some(this) = this.as_element_impl()
        {
            edit::update_storage_from_elements(this);
        }
    }

    /// Port of `InvalidateStorage`.
    fn invalidate_storage(&self) {
        if let Some(this) = self.as_dyn_element_impl()
            && let Some(this) = this.as_element_impl()
        {
            edit::invalidate_storage(this);
        }
    }

    fn request_storage_change_impl(&self, new_size: usize) -> Option<Vec<u8>> {
        let this = self.as_dyn_element_impl()?;
        edit::request_storage_change(this.as_element_impl()?, new_size)
    }

    fn commit_storage_impl(&self, bytes: Vec<u8>) {
        if let Some(storage) = self.storage() {
            storage.set(bytes);
        }
    }

    fn update_ended(&self) {
        edit::update_ended(self.as_this())
    }

    fn notify_changed(&self) {
        edit::notify_changed(self.as_this())
    }

    fn notify_changed_internal(&self) {
        edit::notify_changed_internal(self.as_this())
    }

    /// Port of `ElementChanged`: a child changed.
    fn element_changed(&self, _child: &ElementRef) {
        self.notify_changed();
    }

    fn do_after_set(&self, old: &Variant, new: &Variant) {
        edit::do_after_set(self.as_this(), old, new)
    }

    /// `Reset; Init` after a change of the data: the elements are built
    /// again over the current data.
    fn reset_and_init(&self) {}

    /// Port of `DoReset(True)` before `SetToDefault`: the elements are
    /// released.
    fn release_and_detach(&self) {}

    /// Port of `TwbValue.SetEditValue`'s `RecreateFlags` for a flags value.
    fn after_value_changed(&self) {
        if self.is_flags() {
            self.reset_and_init();
        }
    }

    /// The tail of `TwbSubRecord.SetToDefaultInternal` for an array.
    fn after_set_to_default(&self) -> Result<(), EditError> {
        Ok(())
    }

    fn set_edit_value_impl(&self, _value: &str) -> Result<(), EditError> {
        Err(format!("{} can not be edited.", self.get_name()))
    }

    fn set_native_value_impl(&self, _value: Variant) -> Result<(), EditError> {
        Err(format!("{} can not be edited.", self.get_name()))
    }

    /// Port of `SetToDefaultInternal`. The default is `TwbContainer`'s.
    fn set_to_default_internal(&self) -> Result<(), EditError> {
        edit::container_set_to_default_internal(self.as_this())
    }

    /// Port of `IsEditable`. The default is `TwbElement`'s.
    fn get_is_editable_impl(&self) -> bool {
        crate::interface::globals::is_internal_edit()
    }

    /// Port of `TwbContainer.RemoveElement(aElement, aMarkModified)`.
    fn remove_child(&self, child: &ElementRef, mark_modified: bool) -> Option<ElementRef> {
        edit::container_remove_child(self.as_this(), child, mark_modified)
    }

    /// Port of `Add(aName, aSilent)`.
    fn add_impl(&self, _name: &str, _silent: bool) -> Result<Option<ElementRef>, EditError> {
        Ok(None)
    }

    /// Port of `Assign(wbAssignAdd, nil, False)`: one element added to an
    /// array from its definition alone.
    fn assign_add(&self) -> Result<Option<ElementRef>, EditError> {
        Ok(None)
    }

    /// Port of `TwbStringListTerminator.Create(Self)`.
    fn add_string_list_terminator(&self) {}

    /// Port of `TwbDataContainer.SetDataSize`: the data is resized and the
    /// elements are built again over it.
    fn set_data_size_impl(&self, size: i32) -> Result<(), EditError> {
        if self.storage().is_none() {
            return Err(format!("{} can not be resized.", self.get_name()));
        }
        if size == self.get_data_size() {
            return Ok(());
        }
        let size = usize::try_from(size).map_err(|_| format!("{} can not take a negative size", self.get_name()))?;
        let Some(bytes) = self.request_storage_change_impl(size) else {
            return Err(format!("{} can not be resized.", self.get_name()));
        };
        self.commit_storage_impl(bytes);
        self.reset_and_init();
        Ok(())
    }

    /// Port of `GetSortKeyInternal`. The default is empty.
    fn get_sort_key_impl(&self, _extended: bool) -> String {
        String::new()
    }

    // ----- the assign and copy path (`assign`, `copy`) -----

    /// Port of `CanAssignInternal`. The default is `TwbContainer`'s.
    fn can_assign_internal(&self, index: i32, source: Option<&ElementRef>, check_dont_show: bool) -> bool {
        assign::container_can_assign_internal(self.as_this(), index, source, check_dont_show)
    }

    /// Port of `AssignInternal`. The default is `TwbContainer`'s.
    fn assign_internal(
        &self,
        index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        assign::container_assign_internal(self.as_this(), index, source, only_sk)
    }

    /// Port of `IsElementEditable`: whether this container lets `element`
    /// be edited. The default is `TwbContainer`'s.
    fn is_element_editable(&self, element: Option<&ElementRef>) -> bool {
        assign::container_is_element_editable(self.as_this(), element)
    }

    /// Port of `IsElementRemovable`. The default is `TwbContainer`'s.
    fn is_element_removable(&self, _element: &ElementRef) -> bool {
        false
    }

    /// Port of `GetIsInSK`: whether the member at the sort order is part of
    /// the sort key. The default is `TwbContainer`'s.
    fn get_is_in_sk(&self, _sort_order: i32) -> bool {
        false
    }

    /// Port of `CanContainFormIDs`. The default is `TwbElement`'s.
    fn can_contain_form_ids(&self) -> bool {
        true
    }

    /// Port of `AddIfMissingInternal`. The default is `TwbElement`'s, which
    /// raises.
    fn add_if_missing_internal(&self, _source: &ElementRef, _args: &CopyArgs) -> Result<Option<ElementRef>, EditError> {
        Err(format!("{}.AddIfMissingInternal is not implemented", self.get_name()))
    }

    /// Port of `ReportRequiredMasters`. The default is `TwbContainer`'s.
    fn report_required_masters(&self, masters: &mut copy::FilesSet, as_new: bool, recursive: bool, initial: bool) {
        copy::container_report_required_masters(self.as_this(), masters, as_new, recursive, initial)
    }

    /// Port of `BeforeActualRemove`: the element is about to leave its
    /// container.
    fn before_actual_remove(&self) {}

    /// Port of `TwbContainer.RemoveElement(aPos, aMarkModified)`.
    fn remove_child_at(&self, index: i32, mark_modified: bool) -> Option<ElementRef> {
        let base = self.container_base()?;
        let child = base.element_at(usize::try_from(index).ok()?)?;
        self.remove_child(&child, mark_modified)
    }

    /// Port of `TwbContainer.ReverseElements`.
    fn reverse_elements_impl(&self) {
        if let Some(base) = self.container_base() {
            base.reverse();
            self.set_modified(true);
            self.invalidate_storage();
        }
    }

    /// Port of `TwbContainer.SortBySortOrder`.
    fn sort_by_sort_order_impl(&self) {
        if let Some(base) = self.container_base() {
            self.set_modified(true);
            base.sort_by(|a, b| a.get_sort_order().cmp(&b.get_sort_order()));
            self.invalidate_storage();
        }
    }

    /// This element as the trait object the `edit` functions take.
    fn as_this(&self) -> &dyn ElementImpl;
}

trait ElementImplCasts {
    fn as_container_base(&self) -> Option<&ContainerBase>;
    fn into_main_record_impl(self) -> Option<Arc<MainRecordImpl>>;
}

impl ElementImplCasts for ElementRef {
    fn as_container_base(&self) -> Option<&ContainerBase> {
        self.as_element_impl()?.container_base()
    }

    fn into_main_record_impl(self) -> Option<Arc<MainRecordImpl>> {
        self.as_element_impl()?.main_record_impl()
    }
}

impl Element for FileImpl {
    element_common!(element_base);
    element_display_name!(element_base);

    fn get_name(&self) -> String {
        path_file_name(&self.fl_file_name).to_owned()
    }

    fn get_data_size(&self) -> i32 {
        self.fl_bytes.as_slice().len() as i32
    }

    fn get_element_type(&self) -> ElementType {
        ElementType::etFile
    }

    fn get_def(&self) -> Option<Arc<dyn NamedDef>> {
        None
    }

    fn get_file(&self) -> Option<FileRef> {
        Some(self.self_ref.upgrade()? as FileRef)
    }

    fn get_containing_main_record(&self) -> Option<MainRecordRef> {
        None
    }

    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }
}

impl ElementImpl for FileImpl {
    fn as_this(&self) -> &dyn ElementImpl {
        self
    }

    fn self_element_ref(&self) -> Option<ElementRef> {
        self.self_ref.upgrade().map(|element| element as ElementRef)
    }

    fn element_base(&self) -> &ElementBase {
        &self.base
    }

    fn file_impl(&self) -> Option<Arc<FileImpl>> {
        self.self_ref.upgrade()
    }

    fn write_to_stream(&self, _out: &mut Vec<u8>, _reset: ResetModified) -> Result<(), SaveError> {
        Err(SaveError::Internal(
            "a file is written with FileImpl::write_to_bytes, not as an element".to_owned(),
        ))
    }

    fn prepare_save(&self) -> Result<(), SaveError> {
        self.self_arc().prepare_save_impl()
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.container)
    }

    /// Port of `TwbFile.IsElementEditable`.
    fn is_element_editable(&self, _element: Option<&ElementRef>) -> bool {
        FileImpl::is_element_editable(self)
    }

    fn get_is_editable_impl(&self) -> bool {
        FileImpl::get_is_editable(self)
    }

    /// Port of `TwbFile.IsElementRemovable`: a group, or a record other than
    /// the file header.
    fn is_element_removable(&self, element: &ElementRef) -> bool {
        if !FileImpl::is_element_editable(self) {
            return false;
        }
        match element.get_element_type() {
            ElementType::etMainRecord => element.get_record_signature() != Some(header_signature()),
            ElementType::etGroupRecord => true,
            _ => false,
        }
    }

    /// Port of `TwbFile.GetIsRemovable`.
    fn add_impl(&self, name: &str, _silent: bool) -> Result<Option<ElementRef>, EditError> {
        self.self_arc().add_impl_file(name)
    }

    fn add_if_missing_internal(&self, source: &ElementRef, args: &CopyArgs) -> Result<Option<ElementRef>, EditError> {
        self.self_arc().add_if_missing_internal_impl(source, args)
    }
}

impl Container for FileImpl {
    fn as_container_ref(&self) -> Option<ElementRef> {
        self.self_element_ref()
    }

    fn add(&self, name: &str, silent: bool) -> Result<Option<ElementRef>, EditError> {
        self.add_impl(name, silent)
    }

    fn get_element_by_name(&self, name: &str) -> Option<ElementRef> {
        element_by_name(self, name)
    }

    fn get_element_by_path(&self, path: &str) -> Option<ElementRef> {
        element_by_path(self, path)
    }

    fn get_element_count(&self) -> i32 {
        self.container.element_count() as i32
    }

    fn get_element(&self, index: i32) -> Option<ElementRef> {
        self.container.element_at(usize::try_from(index).ok()?)
    }

    fn get_element_by_sort_order(&self, sort_order: i32) -> Option<ElementRef> {
        self.container.element_by_sort_order(sort_order)
    }

    fn get_any_element(&self) -> Option<ElementRef> {
        self.get_element(0)
    }

    fn get_additional_element_count(&self) -> i32 {
        0
    }
}

impl File for FileImpl {
    fn get_load_order(&self) -> i32 {
        self.load_order()
    }

    fn get_record_from_index_by_key(&self, index: i32, key: &str) -> Option<MainRecordRef> {
        self.record_from_index_by_key(index, key)
            .map(|record| record as MainRecordRef)
    }

    /// Port of `TwbFile.GetRecordByEditorID`.
    fn get_record_by_editor_id(&self, editor_id: &str) -> Option<MainRecordRef> {
        if let Some(record) = self.find_key_in_index(idx_editor_id(), editor_id) {
            return Some(record as MainRecordRef);
        }
        self.masters()
            .iter()
            .rev()
            .find_map(|master| master.get_record_by_editor_id(editor_id))
    }

    /// Port of `TwbFile.GetEncoding` without the overrides of the file.
    fn get_encoding(&self, translatable: bool) -> Encoding {
        if translatable {
            crate::interface::globals::encoding_trans()
        } else {
            crate::interface::globals::encoding()
        }
    }

    fn get_is_localized(&self) -> bool {
        self.header()
            .is_some_and(|header| header.mr_struct().flags.is_localized())
    }

    fn get_is_esm(&self) -> bool {
        self.header().is_some_and(|header| header.mr_struct().flags.is_esm())
    }

    fn get_record_count(&self) -> i32 {
        self.fl_records.read().unwrap().len() as i32
    }

    fn get_file_states(&self) -> FileStates {
        *self.fl_states.read().unwrap()
    }

    fn get_load_order_file_id(&self) -> FileID {
        *self.fl_load_order_file_id.read().unwrap()
    }

    fn get_master_count(&self, _new: bool) -> i32 {
        self.master_count()
    }

    fn get_master(&self, index: i32, _new: bool) -> Option<FileRef> {
        let master = self
            .fl_masters
            .read()
            .unwrap()
            .get(usize::try_from(index).ok()?)
            .cloned()?;
        Some(master as FileRef)
    }

    /// Port of `GetAllowHardcodedRangeUse` without the generation cache: a
    /// plugin of a game and header version that lets its own records use
    /// object IDs below $800, once it has masters. Such a FormID then stays
    /// in the plugin instead of pointing at the game master.
    fn get_allow_hardcoded_range_use(&self) -> bool {
        let mode = game_mode();
        let game_allows = mode == GameMode::gmTES3
            || ((matches!(mode, GameMode::gmSSE | GameMode::gmEnderalSE)
                || (mode == GameMode::gmTES5VR && has_added_light_support()))
                && self.get_version() >= 1.709)
            || (mode == GameMode::gmFO4 && self.get_version() >= 1.0)
            || mode == GameMode::gmSF1;
        game_allows && self.master_count() > 0
    }

    fn get_record_by_form_id(
        &self,
        form_id: FormID,
        allow_injected: bool,
        new_masters: bool,
    ) -> Result<Option<MainRecordRef>, String> {
        let this = self.self_ref.upgrade().expect("a file is alive while it is used");
        Ok(this
            .record_by_form_id(form_id, allow_injected, new_masters)
            .map(|record| record as MainRecordRef))
    }

    /// Port of `FileFormIDtoLoadOrderFormID`.
    fn load_order_form_id_to_file_form_id(&self, form_id: FormID, _new: bool) -> Result<FormID, String> {
        FileImpl::load_order_form_id_to_file_form_id(self, form_id).ok_or_else(|| {
            format!(
                "FormID [{}] can not be mapped to file FormID for file \"{}\"",
                form_id.to_string(true),
                self.get_name()
            )
        })
    }

    fn file_form_id_to_load_order_form_id(&self, form_id: FormID, _new: bool) -> Result<FormID, String> {
        let mut result = form_id;
        if result.object_id() < 0x800 {
            if self.get_allow_hardcoded_range_use() {
                if result.is_hardcoded() {
                    return Ok(result);
                }
            } else {
                return Ok(result.change_file_id(FileID::null()));
            }
        }
        match self.file_file_id_to_load_order_file_id(result.file_id()) {
            Some(file_id) => {
                result = result.change_file_id(file_id);
                Ok(result)
            }
            None => Err("File has no slot assigned".to_owned()),
        }
    }
}

impl Element for GroupRecordImpl {
    element_common!(element_base);
    element_display_name!(element_base);

    /// Port of `TwbGroupRecord.GetName`: the label of a group of children
    /// is the name of the record it belongs to.
    fn get_name(&self) -> String {
        let prefix = match self.gr_struct().group_type {
            1 => "GRUP World Children of ",
            6 => "GRUP Cell Children of ",
            7 => "GRUP Topic Children of ",
            8 => "GRUP Cell Persistent Children of ",
            9 => "GRUP Cell Temporary Children of ",
            10 if vwd_as_quest_children() => "GRUP Quest Children of ",
            10 => "GRUP Cell Visible Distant Children of ",
            _ => return self.gr_struct().name(),
        };
        let label = match crate::interface::constructors::wb_form_id() {
            Some(formater) => {
                let self_ref = self.self_ref.upgrade().map(|group| group as ElementRef);
                IntegerDefFormater::to_string(&*formater, i64::from(self.gr_struct().label), self_ref.as_ref(), false)
            }
            None => format!("[{:08X}]", self.gr_struct().label),
        };
        format!("{prefix}{label}")
    }

    fn get_data_size(&self) -> i32 {
        (self.dc_end - self.dc_base) as i32
    }

    fn get_element_type(&self) -> ElementType {
        ElementType::etGroupRecord
    }

    fn get_def(&self) -> Option<Arc<dyn NamedDef>> {
        None
    }

    fn get_file(&self) -> Option<FileRef> {
        Some(self.file.upgrade()? as FileRef)
    }

    fn get_containing_main_record(&self) -> Option<MainRecordRef> {
        None
    }

    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }
}

impl ElementImpl for GroupRecordImpl {
    fn as_this(&self) -> &dyn ElementImpl {
        self
    }

    fn self_element_ref(&self) -> Option<ElementRef> {
        self.self_ref.upgrade().map(|element| element as ElementRef)
    }

    fn element_base(&self) -> &ElementBase {
        &self.base
    }

    fn group_record_impl(&self) -> Option<Arc<GroupRecordImpl>> {
        self.self_ref.upgrade()
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.container)
    }

    fn write_to_stream(&self, out: &mut Vec<u8>, reset: ResetModified) -> Result<(), SaveError> {
        self.write_to_stream_impl(out, reset)
    }

    fn prepare_save(&self) -> Result<(), SaveError> {
        self.prepare_save_impl()
    }

    fn get_counted_record_count(&self) -> u32 {
        self.counted_record_count_impl()
    }

    fn add_impl(&self, name: &str, silent: bool) -> Result<Option<ElementRef>, EditError> {
        match self.self_ref.upgrade() {
            Some(this) => this.add_impl_group(name, silent),
            None => Ok(None),
        }
    }

    fn add_if_missing_internal(&self, source: &ElementRef, args: &CopyArgs) -> Result<Option<ElementRef>, EditError> {
        match self.self_ref.upgrade() {
            Some(this) => this.add_if_missing_internal_impl(source, args),
            None => Ok(None),
        }
    }

    /// Port of `TwbGroupRecord.IsElementRemovable`.
    fn is_element_removable(&self, element: &ElementRef) -> bool {
        self.is_element_editable(Some(element))
    }
}

impl Container for GroupRecordImpl {
    fn as_container_ref(&self) -> Option<ElementRef> {
        self.self_element_ref()
    }

    fn add(&self, name: &str, silent: bool) -> Result<Option<ElementRef>, EditError> {
        self.add_impl(name, silent)
    }

    fn get_element_by_name(&self, name: &str) -> Option<ElementRef> {
        element_by_name(self, name)
    }

    fn get_element_by_path(&self, path: &str) -> Option<ElementRef> {
        self.get_element_by_name(path)
    }

    fn get_element_count(&self) -> i32 {
        self.container.element_count() as i32
    }

    fn get_element(&self, index: i32) -> Option<ElementRef> {
        self.container.element_at(usize::try_from(index).ok()?)
    }

    fn get_element_by_sort_order(&self, sort_order: i32) -> Option<ElementRef> {
        self.container.element_by_sort_order(sort_order)
    }

    fn get_any_element(&self) -> Option<ElementRef> {
        self.get_element(0)
    }

    fn get_additional_element_count(&self) -> i32 {
        0
    }
}

impl Element for MainRecordImpl {
    element_common!(element_base);

    /// Port of `TwbMainRecord.GetName`.
    fn get_name(&self) -> String {
        let mut result = self.short_name_internal();
        if let Some(def) = &self.mr_def
            && let Some(main_record) = self.self_ref.upgrade().map(|record| record as MainRecordRef)
        {
            let info = RecordDef::additional_info_for(&**def, &main_record);
            let info = info.trim();
            if !info.is_empty() {
                result = format!("{result} ({info})");
            }
        }
        result
    }

    /// Port of `TwbMainRecord.GetDisplayName`: the full name, the names
    /// of references, cells and responses, or the summary. The special names
    /// of placed records and cells are not ported yet.
    fn get_display_name(&self, _use_suffix: bool) -> String {
        if let Some(cached) = self.mr_display_name.read().unwrap().as_ref() {
            return cached.clone();
        }
        let mut result = self.get_full_name();
        let signature = self.mr_struct().signature;
        if result.is_empty() {
            if matches!(
                signature.0.as_slice(),
                b"REFR"
                    | b"PGRE"
                    | b"PMIS"
                    | b"ACHR"
                    | b"ACRE"
                    | b"PARW"
                    | b"PBEA"
                    | b"PFLA"
                    | b"PCON"
                    | b"PBAR"
                    | b"PHZD"
            ) {
                if let Some(def) = &self.mr_def {
                    let known = def.known_sub_record_signatures();
                    let record = match self
                        .get_element_by_name("Map Marker")
                        .filter(|marker| marker.as_container().is_some())
                    {
                        Some(marker) => marker.as_container().and_then(|marker| {
                            marker.get_record_by_signature(known[KnownSubRecord::ksrFullName.ord()])
                        }),
                        None => self.get_record_by_signature(known[KnownSubRecord::ksrBaseRecord.ord()]),
                    };
                    if let Some(record) = record {
                        result = record.get_value().trim().to_owned();
                    }
                }
            } else if signature == Signature::new(b"CELL") {
                let in_world = self
                    .base
                    .container()
                    .and_then(|container| container.as_element_impl()?.group_record_impl())
                    .is_some_and(|group| group.group_type() == 1);
                if in_world {
                    result = "<Persistent Worldspace Cell>".to_owned();
                } else if let Some((x, y)) = self.get_grid_cell() {
                    result = format!("<{}, {}>", str_right(&x.to_string(), 3), str_right(&y.to_string(), 3));
                }
            } else if signature == Signature::new(b"INFO") {
                result = self
                    .get_element_by_path("Responses\\Response\\NAM1")
                    .map(|element| element.get_value())
                    .unwrap_or_default();
            }
        }
        if result.is_empty() {
            result = self.get_summary();
        }
        if self
            .file_impl()
            .is_some_and(|file| file.get_file_states().contains(FileState::fsIsOfficial))
        {
            *self.mr_display_name.write().unwrap() = Some(result.clone());
        }
        result
    }

    /// Port of `TwbMainRecord.GetSummary`.
    fn get_summary(&self) -> String {
        let Some(def) = &self.mr_def else {
            return String::new();
        };
        let self_ref: ElementRef = self.self_arc();
        let mut links_to = None;
        def.to_summary(0, Some(&self_ref), &mut links_to)
    }

    fn get_data_size(&self) -> i32 {
        self.data_len().map_or(0, |len| len as i32)
    }

    fn get_element_type(&self) -> ElementType {
        ElementType::etMainRecord
    }

    fn get_def(&self) -> Option<Arc<dyn NamedDef>> {
        Some(self.mr_def.clone()? as Arc<dyn NamedDef>)
    }

    fn get_record_signature(&self) -> Option<Signature> {
        Some(self.mr_struct().signature)
    }

    fn get_file(&self) -> Option<FileRef> {
        Some(self.file.upgrade()? as FileRef)
    }

    fn get_containing_main_record(&self) -> Option<MainRecordRef> {
        Some(self.self_ref.upgrade()? as MainRecordRef)
    }

    fn into_main_record(self: Arc<Self>) -> Option<MainRecordRef> {
        Some(self as MainRecordRef)
    }

    fn as_main_record(&self) -> Option<&dyn MainRecord> {
        Some(self)
    }

    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }

    fn as_data_container(&self) -> Option<&dyn DataContainer> {
        Some(self)
    }
}

impl ElementImpl for MainRecordImpl {
    fn as_this(&self) -> &dyn ElementImpl {
        self
    }

    fn self_element_ref(&self) -> Option<ElementRef> {
        self.self_ref.upgrade().map(|element| element as ElementRef)
    }

    fn element_base(&self) -> &ElementBase {
        &self.base
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.container)
    }

    fn main_record_impl(&self) -> Option<Arc<MainRecordImpl>> {
        self.self_ref.upgrade()
    }

    fn write_to_stream(&self, out: &mut Vec<u8>, reset: ResetModified) -> Result<(), SaveError> {
        self.self_arc().write_to_stream_impl(out, reset)
    }

    fn prepare_save(&self) -> Result<(), SaveError> {
        self.self_arc().prepare_save_impl()
    }

    /// Port of `TwbMainRecord.GetCountedRecordCount`: the record itself.
    /// UPSTREAM-QUIRK: upstream skips a record that repeats the FormID of
    /// the record before it (`EwbSkipLoad`), so it is not counted, while its
    /// bytes stay in the data of its unmodified group and are written; the
    /// port keeps the record and leaves it out of the count.
    fn get_counted_record_count(&self) -> u32 {
        u32::from(!self.mr_duplicate.load(Ordering::Relaxed))
    }

    fn init_running(&self) -> bool {
        self.mr_init.is_running()
    }

    /// Port of `TwbMainRecord.ElementChanged`: the names cached from the
    /// editor ID and full name subrecords follow a change.
    fn element_changed(&self, child: &ElementRef) {
        if let (Some(sub_record), Some(def)) = (
            child.as_element_impl().and_then(ElementImpl::sub_record_impl),
            &self.mr_def,
        ) {
            let known = def.known_sub_record_signatures();
            let signature = sub_record.get_signature();
            let mut relevant = true;
            if signature == known[KnownSubRecord::ksrEditorID.ord()] {
                self.cache_editor_id(def.get_editor_id(child));
            } else if signature == known[KnownSubRecord::ksrFullName.ord()] {
                self.set_full_name(child.get_edit_value());
            } else if signature != known[KnownSubRecord::ksrBaseRecord.ord()]
                && signature != known[KnownSubRecord::ksrGridCell.ord()]
            {
                relevant = false;
            }
            if relevant {
                *self.mr_display_name.write().unwrap() = None;
            }
        }
        self.notify_changed();
        // `if not (mrsNoUpdateRefs in mrStates) then UpdateRefs`.
        self.self_arc().update_refs();
    }

    /// Port of `TwbMainRecord.SetParentModified`: the group is marked
    /// modified, and the references are built again.
    fn set_parent_modified(&self) {
        if let Some(container) = self.base.container()
            && let Some(container) = container.as_element_impl()
        {
            container.set_modified(true);
        }
        self.self_arc().update_refs();
    }

    /// Port of `TwbMainRecord.DoAfterSet` without the cell child group
    /// update of a moved reference.
    fn do_after_set(&self, old: &Variant, new: &Variant) {
        self.self_arc().do_init();
        edit::do_after_set(self, old, new);
    }

    /// Port of `TwbMainRecord.RemoveElement(aPos, aMarkModified)`: the
    /// cached names of a removed named subrecord are dropped.
    fn remove_child(&self, child: &ElementRef, mark_modified: bool) -> Option<ElementRef> {
        let removed = edit::container_remove_child(self, child, mark_modified)?;
        if mark_modified
            && let (Some(sub_record), Some(def)) = (
                removed.as_element_impl().and_then(ElementImpl::sub_record_impl),
                &self.mr_def,
            )
        {
            let known = def.known_sub_record_signatures();
            let signature = sub_record.get_signature();
            if signature == known[KnownSubRecord::ksrEditorID.ord()] {
                self.cache_editor_id(String::new());
            } else if signature == known[KnownSubRecord::ksrFullName.ord()] {
                self.set_full_name(String::new());
            }
            *self.mr_display_name.write().unwrap() = None;
        }
        Some(removed)
    }

    /// Port of `TwbMainRecord.GetIsEditable`.
    fn get_is_editable_impl(&self) -> bool {
        if is_internal_edit() {
            return true;
        }
        if self
            .mr_def
            .as_ref()
            .is_some_and(|def| def.def_base().def_internal_edit_only())
        {
            return false;
        }
        assign::parent_allows_edit(self)
    }

    fn add_impl(&self, name: &str, silent: bool) -> Result<Option<ElementRef>, EditError> {
        self.self_arc().add(name, silent)
    }

    fn assign_internal(
        &self,
        index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        self.self_arc().assign_internal_impl(index, source, only_sk)
    }

    fn can_assign_internal(&self, index: i32, source: Option<&ElementRef>, check_dont_show: bool) -> bool {
        self.self_arc().can_assign_internal_impl(index, source, check_dont_show)
    }

    fn add_if_missing_internal(&self, source: &ElementRef, args: &CopyArgs) -> Result<Option<ElementRef>, EditError> {
        self.self_arc().add_if_missing_internal_impl(source, args)
    }

    fn report_required_masters(&self, masters: &mut copy::FilesSet, as_new: bool, recursive: bool, initial: bool) {
        copy::main_record_report_required_masters(self, masters, as_new, recursive, initial)
    }

    /// Port of `TwbMainRecord.IsElementRemovable`: a member that is not
    /// required.
    fn is_element_removable(&self, element: &ElementRef) -> bool {
        self.is_element_editable(Some(element)) && !element.get_def().is_some_and(|def| def.def_base().def_required())
    }

    /// Port of `TwbMainRecord.SetEditValue`: the load order FormID of the
    /// record, in hexadecimal (`SetLoadOrderFormID`).
    fn set_edit_value_impl(&self, value: &str) -> Result<(), EditError> {
        let form_id = crate::interface::form_id::FormID::from_str(value)
            .ok_or_else(|| format!("\"{value}\" is not a valid integer value"))?;
        self.set_form_id_value(form_id)
    }

    /// Port of `TwbMainRecord.SetNativeValue`.
    fn set_native_value_impl(&self, value: Variant) -> Result<(), EditError> {
        let form_id = value
            .as_ordinal()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| format!("{} can not be edited.", self.get_name()))?;
        self.set_form_id_value(crate::interface::form_id::FormID::from_cardinal(form_id))
    }
}

impl Container for MainRecordImpl {
    fn as_container_ref(&self) -> Option<ElementRef> {
        self.self_element_ref()
    }

    fn add(&self, name: &str, silent: bool) -> Result<Option<ElementRef>, EditError> {
        self.add_impl(name, silent)
    }

    fn remove_element_at(&self, index: i32, mark_modified: bool) -> Option<ElementRef> {
        self.remove_child_at(index, mark_modified)
    }

    fn get_element_by_name(&self, name: &str) -> Option<ElementRef> {
        element_by_name(self, name)
    }

    fn get_element_by_path(&self, path: &str) -> Option<ElementRef> {
        element_by_path(self, path)
    }

    fn get_element_count(&self) -> i32 {
        self.read_built(|elements| elements.len() as i32)
    }

    fn get_element(&self, index: i32) -> Option<ElementRef> {
        let index = usize::try_from(index).ok()?;
        self.read_built(|elements| elements.get(index).cloned())
    }

    fn get_element_by_sort_order(&self, sort_order: i32) -> Option<ElementRef> {
        let sort_order = sort_order - self.get_additional_element_count();
        self.read_built(|elements| {
            elements
                .iter()
                .find(|element| {
                    element.as_element_impl().is_some_and(|element| {
                        element.element_base().e_sort_order.load(Ordering::Relaxed) == sort_order
                    })
                })
                .cloned()
        })
    }

    fn get_any_element(&self) -> Option<ElementRef> {
        self.get_element(0)
    }

    /// Port of `TwbMainRecord.GetAdditionalElementCount`: the record header
    /// and the contained-in element.
    fn get_additional_element_count(&self) -> i32 {
        let mut result = 1;
        if create_contained_in()
            && let Some(group) = self
                .base
                .container()
                .and_then(|container| container.as_element_impl()?.group_record_impl())
            && matches!(group.group_type(), 1 | 4..=10)
        {
            result += 1;
        }
        result
    }
}

impl DataContainer for MainRecordImpl {
    fn get_data(&self) -> DataPtr<'_> {
        self.data()
    }
}

impl MainRecord for MainRecordImpl {
    /// Port of `GetLoadOrderFormID`.
    fn get_form_id(&self) -> FormID {
        self.mr_struct().form_id
    }

    fn get_has_precombined_mesh(&self) -> bool {
        self.self_arc().precombined().is_some()
    }

    /// Port of `GetPrecombinedMesh` for a record that has one.
    fn get_precombined_mesh(&self) -> String {
        let Some((cell_id, mesh_id)) = self.self_arc().precombined() else {
            return String::new();
        };
        if game_mode() == GameMode::gmFO76 {
            return format!("Precombined\\{cell_id:08X}\\{cell_id:08X}nif");
        }
        let mut master_folder = String::new();
        if let Some(cell) = self
            .base
            .container()
            .and_then(|container| container.as_element_impl()?.group_record_impl())
            .and_then(|group| group.children_of())
        {
            let cell = cell.get_master_or_self();
            if let Some(file) = cell.get_file()
                && file.get_load_order() > 0
            {
                master_folder = format!("{}\\", file.get_name());
            }
        }
        format!("Precombined\\{master_folder}{cell_id:08X}_{mesh_id:08X}_OC.nif")
    }

    fn get_fixed_form_id(&self) -> FormID {
        MainRecordImpl::get_fixed_form_id(self)
    }

    fn get_load_order_form_id(&self) -> FormID {
        let form_id = self.get_fixed_form_id();
        match self.file_impl() {
            Some(file) => file
                .file_form_id_to_load_order_form_id(form_id, false)
                .unwrap_or(form_id),
            None => form_id,
        }
    }

    fn get_signature(&self) -> Signature {
        self.mr_struct().signature
    }

    fn get_editor_id(&self) -> String {
        if self.can_have(KnownSubRecord::ksrEditorID) {
            self.self_arc().quick_init();
        }
        self.mr_editor_id.read().unwrap().clone()
    }

    fn get_short_name(&self) -> String {
        self.short_name_internal()
    }

    /// Port of `TwbMainRecord.GetIsPartialForm`: only a record whose
    /// definition allows partial forms is one.
    fn get_is_partial_form(&self) -> bool {
        self.mr_def.as_ref().is_some_and(|def| def.get_can_be_partial()) && self.mr_struct().flags.is_partial_form()
    }

    fn get_flags(&self) -> MainRecordStructFlags {
        self.mr_struct().flags
    }

    /// Port of `TwbMainRecord.GetCanBePartial`.
    fn get_can_be_partial(&self) -> bool {
        let Some(def) = &self.mr_def else { return false };
        if !def.get_can_be_partial() {
            return false;
        }
        if self.mr_struct().signature != Signature::new(b"CELL") {
            return true;
        }
        let master_or_self = self.get_master_or_self();
        // No partial for temporary exterior cells.
        if !self.get_is_persistent() && master_or_self.get_grid_cell().is_some() {
            return false;
        }
        // Only interior cells get here. No partial for interior cells in
        // FO4 if they are not defined in Fallout4.esm.
        if game_mode() == GameMode::gmFO4
            && let Some(file) = master_or_self.get_file()
            && !file.get_file_states().contains(FileState::fsIsGameMaster)
        {
            return false;
        }
        true
    }

    /// Port of `TwbMainRecord.GetGridCell`: the `XCLC` position of a cell.
    fn get_grid_cell(&self) -> Option<(i32, i32)> {
        if self.mr_struct().signature != Signature::new(b"CELL") {
            return None;
        }
        let xclc = self.get_record_by_signature(Signature::new(b"XCLC"))?;
        let xclc = xclc.as_container()?;
        let x = xclc.get_element_native_value("X").as_ordinal()?;
        let y = xclc.get_element_native_value("Y").as_ordinal()?;
        Some((x as i32, y as i32))
    }

    fn get_is_persistent(&self) -> bool {
        self.mr_struct().flags.is_persistent()
    }

    fn get_version(&self) -> u32 {
        u32::from(self.mr_struct().version)
    }

    fn get_is_deleted(&self) -> bool {
        self.mr_struct().flags.is_deleted()
    }

    /// Port of `GetMasterOrSelf`.
    fn get_is_master(&self) -> bool {
        self.master().is_none()
    }

    fn set_is_compressed(&self, value: bool) {
        self.self_arc().set_is_compressed(value);
    }

    fn get_master_or_self(&self) -> MainRecordRef {
        let master = self.mr_master.read().unwrap().as_ref().and_then(Weak::upgrade);
        master.unwrap_or_else(|| self.self_arc()) as MainRecordRef
    }

    /// Port of `GetWinningOverride`: the last override.
    fn get_winning_override(&self) -> MainRecordRef {
        let master = self.mr_master.read().unwrap().as_ref().and_then(Weak::upgrade);
        let base = master.unwrap_or_else(|| self.self_arc());
        let overrides = base.mr_overrides.read().unwrap();
        overrides.iter().rev().find_map(Weak::upgrade).unwrap_or(base.clone()) as MainRecordRef
    }

    fn get_highest_override_visible_for_file(&self, file: &FileRef) -> Option<MainRecordRef> {
        let file = file.as_element_impl()?.file_impl()?;
        Some(self.self_arc().highest_override_visible_for_file(&file) as MainRecordRef)
    }

    /// Port of `GetHighestOverrideOrSelf`.
    fn get_highest_override_or_self(&self, max_load_order: i32) -> MainRecordRef {
        let master = self.mr_master.read().unwrap().as_ref().and_then(Weak::upgrade);
        let base = master.unwrap_or_else(|| self.self_arc());
        let overrides = base.mr_overrides.read().unwrap();
        overrides
            .iter()
            .rev()
            .filter_map(Weak::upgrade)
            .find(|record| {
                !record.get_is_partial_form()
                    && record
                        .file_impl()
                        .is_some_and(|file| file.load_order() <= max_load_order)
            })
            .map_or_else(|| self.self_arc() as MainRecordRef, |record| record as MainRecordRef)
    }

    /// Port of `GetBaseRecord` without the cache of the base record FormID.
    fn get_base_record(&self) -> Option<MainRecordRef> {
        let def = self.mr_def.as_ref()?;
        if !def.get_contains_known_sub_record(KnownSubRecord::ksrBaseRecord) {
            return None;
        }
        let signature = def.known_sub_record_signatures()[KnownSubRecord::ksrBaseRecord.ord()];
        self.get_record_by_signature(signature)?
            .get_links_to()?
            .into_main_record()
    }
}

/// Port of `wbIsModule`: the game executable, or a plugin by its extension,
/// also when it is ghosted.
fn is_module(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let base = lower.strip_suffix(".ghost").unwrap_or(&lower);
    path_file_name(file_name).eq_ignore_ascii_case(&game_exe_name())
        || [".esp", ".esm", ".esl", ".esu"]
            .iter()
            .any(|extension| base.ends_with(extension))
}
