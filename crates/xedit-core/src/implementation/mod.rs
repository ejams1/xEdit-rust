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

fn not_ported(what: &str) -> ! {
    unimplemented!("{what}: the values of the element tree are not ported yet")
}

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
    game_master_esm, header_signature, is_light_supported, is_medium_supported, is_update_supported, pseudo_light,
    pseudo_medium, pseudo_update, size_of_main_record_struct, wb_get_group_order,
};
use crate::interface::main_record::MainRecordDef;
use crate::interface::misc::{Variant, progress};
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
}

impl ElementBase {
    fn new(container: Option<&ElementRef>) -> Self {
        ElementBase {
            e_container: RwLock::new(container.map(Arc::downgrade)),
            e_sort_order: AtomicI32::new(0),
            e_memory_order: AtomicI32::new(0),
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
    fl_masters: RwLock<Vec<FileRef>>,
    fl_load_finished: OnceLock<()>,
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

    fn add_main_record(&self, record: Arc<MainRecordImpl>) {
        self.fl_records.write().unwrap().push(record);
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
        self.assign_slot(&header)?;
        while offset < bytes.len() {
            create_record(self, &container, bytes, &mut offset, None)?;
        }
        Ok(())
    }

    /// Port of `AssignSlot` in `TwbFile.Scan`, for a file without masters
    /// that are loaded: the load order slot of the file.
    fn assign_slot(&self, header: &MainRecordImpl) -> Result<(), LoadError> {
        if self.fl_load_order_file_id.read().unwrap().full_slot() >= 0 {
            return Ok(());
        }
        let load_order = self.load_order();
        if load_order < 0 {
            return Ok(());
        }
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
        fl_masters: RwLock::new(Vec::new()),
        fl_load_finished: OnceLock::new(),
    });
    file.scan()?;
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
        let group = Arc::new(GroupRecordImpl {
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

    /// Port of `DoInit`: builds the subrecords once.
    pub fn do_init(self: &Arc<Self>) {
        self.mr_init.get_or_init(|| sub_record::init_main_record(self));
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
        self.fl_masters.read().unwrap().len() as i32
    }

    fn get_master(&self, index: i32, _new: bool) -> Option<FileRef> {
        self.fl_masters
            .read()
            .unwrap()
            .get(usize::try_from(index).ok()?)
            .cloned()
    }

    fn get_allow_hardcoded_range_use(&self) -> bool {
        false
    }

    fn get_record_by_form_id(
        &self,
        _form_id: FormID,
        _allow_injected: bool,
        _new_masters: bool,
    ) -> Result<Option<MainRecordRef>, String> {
        not_ported("GetRecordByFormID")
    }

    fn file_form_id_to_load_order_form_id(&self, _form_id: FormID, _new: bool) -> Result<FormID, String> {
        not_ported("FileFormIDtoLoadOrderFormID")
    }
}

impl Element for GroupRecordImpl {
    element_common!(element_base);

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

    /// Port of `TwbMainRecord.GetName` without the editor ID and the name
    /// of the record, which need the subrecords.
    fn get_name(&self) -> String {
        format!(
            "{} [{}]",
            self.mr_struct.signature,
            self.mr_struct.form_id.to_string(false)
        )
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
    fn get_load_order_form_id(&self) -> FormID {
        not_ported("LoadOrderFormID")
    }

    fn get_signature(&self) -> Signature {
        self.mr_struct.signature
    }

    fn get_editor_id(&self) -> String {
        self.self_arc().do_init();
        self.mr_editor_id.read().unwrap().clone()
    }

    fn get_short_name(&self) -> String {
        not_ported("ShortName")
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

    fn get_winning_override(&self) -> MainRecordRef {
        not_ported("WinningOverride")
    }

    fn get_highest_override_visible_for_file(&self, _file: &FileRef) -> Option<MainRecordRef> {
        not_ported("HighestOverrideVisibleForFile")
    }
}
