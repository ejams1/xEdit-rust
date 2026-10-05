// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of `wbImplementation.pas`: the element tree of a loaded plugin.
//!
//! State: the binary scan of a file into its header record, group records
//! and main records, with the record data kept as ranges of the mapped
//! file, and the subrecords of a record grouped by its definition on first
//! use. The values come next.

pub mod structs;

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

        fn add_referenced_from_id(&self, _form_id: FormID) {}

        fn get_links_to(&self) -> Option<ElementRef> {
            None
        }

        fn get_container(&self) -> Option<ElementRef> {
            self.$base().container()
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
            ConflictPriority::cpNormal
        }

        fn get_dont_show(&self) -> bool {
            false
        }

        fn as_element_impl(&self) -> Option<&dyn ElementImpl> {
            Some(self)
        }
    };
}

pub mod sub_record;
pub mod value;

use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, OnceLock, RwLock, Weak};

use xedit_io::{Encoding, MappedFile};

use crate::interface::constructors::find_record_def;
use crate::interface::def::{NamedDef, ValueDef};
use crate::interface::element::{
    Container, DataContainer, DataPtr, Element, ElementRef, File, FileRef, MainRecord, MainRecordRef,
};
use crate::interface::form_id::{FileID, FormID};
use crate::interface::globals::{
    create_contained_in, display_load_order_form_id, game_master_esm, header_signature, is_light_supported,
    is_medium_supported, is_update_supported, pseudo_light, pseudo_medium, pseudo_update, size_of_main_record_struct,
    vwd_as_quest_children, wb_get_group_order,
};
use crate::interface::main_record::MainRecordDef;
use crate::interface::misc::{Variant, progress};
use crate::interface::sub_record_group::RecordDef;
use crate::interface::types::{ConflictPriority, ElementType, FileState, FileStates, Signature, TriBool};

use self::structs::{GroupRecordStruct, MainRecordStruct};

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

/// Port of `ElementByPath`: the names separated by `\`, with `[n]` for the
/// element at a position.
pub(crate) fn element_by_path(container: &dyn Container, path: &str) -> Option<ElementRef> {
    let (first, rest) = match path.split_once('\\') {
        Some((first, rest)) => (first, Some(rest)),
        None => (path, None),
    };
    let element = if let Some(index) = first.strip_prefix('[').and_then(|index| index.strip_suffix(']')) {
        container.get_element(index.parse().ok()?)
    } else {
        container.get_element_by_name(first)
    }?;
    match rest {
        Some(rest) => element.as_container()?.get_element_by_path(rest),
        None => Some(element),
    }
}

/// The fields of `TwbElement`.
pub struct ElementBase {
    e_container: RwLock<Option<Weak<dyn Element>>>,
    e_sort_order: AtomicI32,
    e_memory_order: AtomicI32,
    /// Port of `eNameSuffix`: `#3` for the elements of an array.
    e_name_suffix: RwLock<String>,
}

impl ElementBase {
    fn new(container: Option<&ElementRef>) -> Self {
        ElementBase {
            e_container: RwLock::new(container.map(Arc::downgrade)),
            e_sort_order: AtomicI32::new(0),
            e_memory_order: AtomicI32::new(0),
            e_name_suffix: RwLock::new(String::new()),
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
}

impl ContainerBase {
    pub(crate) fn add_element(&self, element: ElementRef) {
        self.cnt_elements.write().unwrap().push(element);
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
    fl_records: RwLock<Vec<Arc<MainRecordImpl>>>,
    /// Port of `SortRecords`: the records by FormID, for the lookups.
    fl_sorted_records: OnceLock<Vec<Arc<MainRecordImpl>>>,
    fl_masters: RwLock<Vec<Arc<FileImpl>>>,
    fl_load_finished: OnceLock<()>,
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
    pub fn file_name(&self) -> &str {
        &self.fl_file_name
    }

    pub fn load_order(&self) -> i32 {
        self.fl_load_order.load(Ordering::Relaxed)
    }

    /// The header record (`TES4`).
    pub fn header(&self) -> Option<Arc<MainRecordImpl>> {
        self.container
            .elements()
            .first()
            .and_then(|element| element.clone().into_main_record_impl())
    }

    /// Port of `flRecords`: the main records of the file in file order.
    pub fn records(&self) -> Vec<Arc<MainRecordImpl>> {
        self.fl_records.read().unwrap().clone()
    }

    /// Port of `AddMainRecord`: keeps the record and registers it as the
    /// override of the record of a master with the same FormID.
    fn add_main_record(self: &Arc<Self>, record: Arc<MainRecordImpl>) {
        self.fl_records.write().unwrap().push(record.clone());
        let form_id = record.get_fixed_form_id();
        if form_id.is_null() {
            return;
        }
        let file_id = form_id.file_id();
        let is_new = i32::from(file_id.full_slot()) >= self.master_count();
        if is_new || (form_id.is_hardcoded() && !self.get_file_states().contains(FileState::fsIsGameMaster)) {
            return;
        }
        if let Some(master) = self.get_master_record_by_form_id(form_id, true, true) {
            master.add_override(&record);
        }
    }

    fn master_count(&self) -> i32 {
        self.fl_masters.read().unwrap().len() as i32
    }

    /// Port of `GetFileFileID`: the FileID of the file as its own records
    /// use it, which is the number of its masters.
    pub fn get_file_file_id(&self) -> FileID {
        FileID::create_full(self.master_count() as i16)
    }

    /// Port of `FindFormID` on the sorted records.
    fn find_form_id(&self, form_id: FormID) -> Option<Arc<MainRecordImpl>> {
        let sorted = self.fl_sorted_records.get()?;
        let index = sorted
            .binary_search_by_key(&form_id.to_cardinal(), |record| {
                record.get_fixed_form_id().to_cardinal()
            })
            .ok()?;
        Some(sorted[index].clone())
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
        if master.is_none() {
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
        if self.get_file_states().contains(FileState::fsIsGameMaster) {
            return None;
        }
        let master = self.get_master_record_by_form_id(form_id, allow_injected, new_masters)?;
        Some(master.highest_override_visible_for_file(self))
    }

    /// Port of `FileFileIDtoLoadOrderFileID`.
    fn file_file_id_to_load_order_file_id(&self, file_id: FileID) -> Option<FileID> {
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

    fn scan_records(self: &Arc<Self>) -> Result<(), LoadError> {
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
        while offset < bytes.len() {
            create_record(self, &container, bytes, &mut offset, None)?;
        }
        let mut sorted = self.fl_records.read().unwrap().clone();
        sorted.sort_by_key(|record| record.get_fixed_form_id().to_cardinal());
        self.fl_sorted_records.set(sorted).ok();
        Ok(())
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
        let flags = header.mr_struct.flags;
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

/// Port of `wbGetGameMasterFile`.
fn game_master_file() -> Option<Arc<FileImpl>> {
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
    if base_name.eq_ignore_ascii_case(&game_master_esm()) {
        fl_states.include(FileState::fsIsGameMaster);
        fl_states.include(FileState::fsIsOfficial);
    }
    fl_states.include(FileState::fsMemoryMapped);
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
        fl_sorted_records: OnceLock::new(),
        fl_masters: RwLock::new(Vec::new()),
        fl_load_finished: OnceLock::new(),
    });
    progress(&format!("[{}] Loading file", file.get_name()));
    FILES_MAP.write().unwrap().push(file.clone());
    file.scan()?;
    crate::interface::element::add_file(file.clone());
    Ok(file)
}

/// Port of `ExtractFileName`.
fn path_file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
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
    gr_struct: GroupRecordStruct,
    /// The range of the group in the file, header included.
    dc_base: usize,
    dc_end: usize,
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
            gr_struct,
            dc_base,
            dc_end,
        });
        if gr_struct.group_type == 0 {
            let order = wb_get_group_order(gr_struct.label_signature());
            group.base.e_sort_order.store(order, Ordering::Relaxed);
            group.base.e_memory_order.store(order, Ordering::Relaxed);
        }
        if let Some(parent) = container.as_container_base() {
            parent.add_element(group.clone());
        }
        // Port of `TwbGroupRecord.ScanData`.
        let self_element: ElementRef = group.clone();
        let mut current = dc_base + header_size;
        let mut prev_main_record: Option<Arc<MainRecordImpl>> = None;
        while current < dc_end {
            let record = create_record(file, &self_element, bytes, &mut current, prev_main_record.as_ref())?;
            prev_main_record = record.and_then(|record| record.into_main_record_impl());
        }
        *offset = dc_end;
        Ok(group)
    }

    pub fn group_type(&self) -> i32 {
        self.gr_struct.group_type
    }

    pub fn group_label(&self) -> u32 {
        self.gr_struct.label
    }

    /// Port of `ChildrenOf`: the record the group holds the children of,
    /// for the group types whose label is a FormID.
    pub fn children_of(&self) -> Option<Arc<MainRecordImpl>> {
        if !matches!(self.gr_struct.group_type, 1 | 6..=10) {
            return None;
        }
        let file = self.file.upgrade()?;
        file.record_by_form_id(FormID::from_cardinal(self.gr_struct.label), true, true)
    }

    fn parent_group(&self) -> Option<Arc<GroupRecordImpl>> {
        self.base.container()?.as_element_impl()?.group_record_impl()
    }
}

/// Port of `TwbMainRecord`, as far as the structure of the record goes.
pub struct MainRecordImpl {
    self_ref: Weak<MainRecordImpl>,
    base: ElementBase,
    container: ContainerBase,
    file: Weak<FileImpl>,
    bytes: Arc<FileBytes>,
    mr_struct: MainRecordStruct,
    mr_def: Option<Arc<MainRecordDef>>,
    /// The range of the record data in the file, after the header.
    dc_data_base: usize,
    dc_data_end: usize,
    /// The decompressed data of a compressed record, decompressed on first use.
    mr_data_storage: OnceLock<Option<Arc<Vec<u8>>>>,
    /// Port of `DoInit`: the subrecords are built on first use.
    mr_init: OnceLock<()>,
    mr_editor_id: RwLock<String>,
    mr_full_name: RwLock<String>,
    /// Port of `mrMaster` and `mrOverrides`.
    mr_master: RwLock<Option<Weak<MainRecordImpl>>>,
    mr_overrides: RwLock<Vec<Weak<MainRecordImpl>>>,
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
            progress(&format!("Error: unknown record type {}", mr_struct.signature));
        }
        if let Some(prev) = prev_main_record
            && prev.mr_struct.form_id == mr_struct.form_id
        {
            // Port of `EwbSkipLoad` for a duplicate FormID: the record is skipped.
            progress(&format!(
                "Skipped Load: Duplicate FormID [{}] in file {}",
                mr_struct.form_id.to_string(true),
                file.fl_file_name
            ));
        }
        let record = Arc::new_cyclic(|self_ref: &Weak<MainRecordImpl>| MainRecordImpl {
            self_ref: self_ref.clone(),
            base: ElementBase::new(Some(container)),
            container: ContainerBase::default(),
            file: Arc::downgrade(file),
            bytes: file.fl_bytes.clone(),
            mr_struct,
            mr_def,
            dc_data_base,
            dc_data_end,
            mr_data_storage: OnceLock::new(),
            mr_init: OnceLock::new(),
            mr_editor_id: RwLock::new(String::new()),
            mr_full_name: RwLock::new(String::new()),
            mr_master: RwLock::new(None),
            mr_overrides: RwLock::new(Vec::new()),
        });
        if let Some(parent) = container.as_container_base() {
            parent.add_element(record.clone());
        }
        file.add_main_record(record.clone());
        *offset = dc_data_end;
        Ok(record)
    }

    pub fn get_signature(&self) -> Signature {
        self.mr_struct.signature
    }

    pub fn form_id(&self) -> FormID {
        self.mr_struct.form_id
    }

    pub fn header_struct(&self) -> &MainRecordStruct {
        &self.mr_struct
    }

    pub fn def(&self) -> Option<&Arc<MainRecordDef>> {
        self.mr_def.as_ref()
    }

    pub(crate) fn set_editor_id(&self, editor_id: String) {
        *self.mr_editor_id.write().unwrap() = editor_id;
    }

    pub(crate) fn set_full_name(&self, full_name: String) {
        *self.mr_full_name.write().unwrap() = full_name;
    }

    /// Port of `GetFullName`, read while the subrecords are built.
    pub fn get_full_name(&self) -> String {
        self.self_arc().do_init();
        self.mr_full_name.read().unwrap().clone()
    }

    /// Port of `FixedFormID`. The hardcoded range of the game master is
    /// not adjusted yet.
    pub fn get_fixed_form_id(&self) -> FormID {
        self.mr_struct.form_id
    }

    /// Port of `AddOverride`.
    fn add_override(self: &Arc<Self>, record: &Arc<MainRecordImpl>) {
        *record.mr_master.write().unwrap() = Some(Arc::downgrade(self));
        self.mr_overrides.write().unwrap().push(Arc::downgrade(record));
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

    /// Port of `GetShortNameInternal` without `wbDisplayShorterNames`.
    fn short_name_internal(&self) -> String {
        let mut result = self.mr_struct.signature.to_string();
        if let Some(def) = &self.mr_def {
            result = format!("{result} - {}", def.get_name());
        }
        if display_load_order_form_id() {
            result = format!("{result} [{}]", self.get_load_order_form_id().to_string(true));
        } else {
            result = format!("{result} [{}]", self.mr_struct.form_id.to_string(true));
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

    /// Port of `DoInit`: builds the subrecords once.
    pub fn do_init(self: &Arc<Self>) {
        self.mr_init.get_or_init(|| {
            self.create_contained_in();
            sub_record::init_main_record(self);
        });
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
        self.container.insert_element(0, element);
    }

    fn self_arc(&self) -> Arc<Self> {
        self.self_ref
            .upgrade()
            .expect("a main record is alive while it is used")
    }

    /// Port of `DecompressIfNeeded`: the record data, decompressed when the
    /// record is compressed. `None` when the decompression fails.
    pub fn data(&self) -> DataPtr<'_> {
        let raw = self.bytes.as_slice().get(self.dc_data_base..self.dc_data_end)?;
        if !self.mr_struct.flags.is_compressed() {
            return Some(raw);
        }
        self.mr_data_storage
            .get_or_init(|| {
                let uncompressed_length = u32::from_le_bytes(raw.get(..4)?.try_into().ok()?) as usize;
                if uncompressed_length == 0 {
                    return Some(Arc::new(Vec::new()));
                }
                let mut storage = vec![0u8; uncompressed_length];
                match xedit_io::CompressionType::ZLib.decompress(raw.get(4..)?, &mut storage) {
                    Ok(()) => Some(Arc::new(storage)),
                    Err(error) => {
                        progress(&format!(
                            "<Error decompressing [{}:{}]: {error}>",
                            self.mr_struct.signature,
                            self.mr_struct.form_id.to_string(false)
                        ));
                        None
                    }
                }
            })
            .as_deref()
            .map(Vec::as_slice)
    }

    /// The block the data of the record lives in, with the data range.
    pub(crate) fn data_block(&self) -> Option<(DataBlock, usize, usize)> {
        if self.mr_struct.flags.is_compressed() {
            self.data()?;
            let storage = self.mr_data_storage.get()?.clone()?;
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

    /// Port of `SetSortOrder` and `SetMemoryOrder` with the same value.
    fn set_sort_and_memory_order(&self, order: i32) {
        self.element_base().e_sort_order.store(order, Ordering::Relaxed);
        self.element_base().e_memory_order.store(order, Ordering::Relaxed);
    }
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
    fn element_base(&self) -> &ElementBase {
        &self.base
    }

    fn file_impl(&self) -> Option<Arc<FileImpl>> {
        self.self_ref.upgrade()
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.container)
    }
}

impl Container for FileImpl {
    fn get_element_native_value(&self, _path: &str) -> Variant {
        Variant::Empty
    }

    fn get_element_by_name(&self, name: &str) -> Option<ElementRef> {
        self.container
            .elements()
            .into_iter()
            .find(|element| element.get_name() == name)
    }

    fn get_element_by_path(&self, path: &str) -> Option<ElementRef> {
        self.get_element_by_name(path)
    }

    fn get_element_count(&self) -> i32 {
        self.container.elements().len() as i32
    }

    fn get_element(&self, index: i32) -> Option<ElementRef> {
        self.container.elements().get(usize::try_from(index).ok()?).cloned()
    }

    fn get_element_by_sort_order(&self, sort_order: i32) -> Option<ElementRef> {
        self.get_element(sort_order)
    }

    fn get_any_element(&self) -> Option<ElementRef> {
        self.get_element(0)
    }

    fn get_additional_element_count(&self) -> i32 {
        0
    }
}

impl File for FileImpl {
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
            .is_some_and(|header| header.mr_struct.flags.is_localized())
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

    fn get_allow_hardcoded_range_use(&self) -> bool {
        false
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

    fn get_name(&self) -> String {
        self.gr_struct.name()
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
    fn element_base(&self) -> &ElementBase {
        &self.base
    }

    fn group_record_impl(&self) -> Option<Arc<GroupRecordImpl>> {
        self.self_ref.upgrade()
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.container)
    }
}

impl Container for GroupRecordImpl {
    fn get_element_native_value(&self, _path: &str) -> Variant {
        Variant::Empty
    }

    fn get_element_by_name(&self, name: &str) -> Option<ElementRef> {
        self.container
            .elements()
            .into_iter()
            .find(|element| element.get_name() == name)
    }

    fn get_element_by_path(&self, path: &str) -> Option<ElementRef> {
        self.get_element_by_name(path)
    }

    fn get_element_count(&self) -> i32 {
        self.container.elements().len() as i32
    }

    fn get_element(&self, index: i32) -> Option<ElementRef> {
        self.container.elements().get(usize::try_from(index).ok()?).cloned()
    }

    fn get_element_by_sort_order(&self, sort_order: i32) -> Option<ElementRef> {
        self.get_element(sort_order)
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

    /// Port of `TwbMainRecord.GetDisplayName` for the full name; the names
    /// of references, cells and responses and the summary are not ported yet.
    fn get_display_name(&self, _use_suffix: bool) -> String {
        self.get_full_name()
    }

    fn get_data_size(&self) -> i32 {
        self.data().map_or(0, |data| data.len() as i32)
    }

    fn get_element_type(&self) -> ElementType {
        ElementType::etMainRecord
    }

    fn get_def(&self) -> Option<Arc<dyn NamedDef>> {
        Some(self.mr_def.clone()? as Arc<dyn NamedDef>)
    }

    fn get_record_signature(&self) -> Option<Signature> {
        Some(self.mr_struct.signature)
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
    fn element_base(&self) -> &ElementBase {
        &self.base
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.container)
    }

    fn main_record_impl(&self) -> Option<Arc<MainRecordImpl>> {
        self.self_ref.upgrade()
    }
}

impl Container for MainRecordImpl {
    fn get_element_native_value(&self, path: &str) -> Variant {
        self.get_element_by_path(path)
            .map_or(Variant::Empty, |element| element.get_native_value())
    }

    fn get_element_by_name(&self, name: &str) -> Option<ElementRef> {
        self.self_arc().do_init();
        self.container
            .elements()
            .into_iter()
            .find(|element| element.get_name() == name)
    }

    fn get_element_by_path(&self, path: &str) -> Option<ElementRef> {
        element_by_path(self, path)
    }

    fn get_element_count(&self) -> i32 {
        self.self_arc().do_init();
        self.container.elements().len() as i32
    }

    fn get_element(&self, index: i32) -> Option<ElementRef> {
        self.self_arc().do_init();
        self.container.elements().get(usize::try_from(index).ok()?).cloned()
    }

    fn get_element_by_sort_order(&self, sort_order: i32) -> Option<ElementRef> {
        self.get_element(sort_order)
    }

    fn get_any_element(&self) -> Option<ElementRef> {
        self.get_element(0)
    }

    fn get_additional_element_count(&self) -> i32 {
        0
    }
}

impl DataContainer for MainRecordImpl {
    fn get_data(&self) -> DataPtr<'_> {
        self.data()
    }
}

impl MainRecord for MainRecordImpl {
    /// Port of `GetLoadOrderFormID`.
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
        self.mr_struct.signature
    }

    fn get_editor_id(&self) -> String {
        self.self_arc().do_init();
        self.mr_editor_id.read().unwrap().clone()
    }

    fn get_short_name(&self) -> String {
        self.short_name_internal()
    }

    fn get_is_partial_form(&self) -> bool {
        self.mr_struct.flags.is_partial_form()
    }

    fn get_is_persistent(&self) -> bool {
        self.mr_struct.flags.is_persistent()
    }

    fn get_version(&self) -> u32 {
        u32::from(self.mr_struct.version)
    }

    fn get_is_deleted(&self) -> bool {
        self.mr_struct.flags.is_deleted()
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
}
