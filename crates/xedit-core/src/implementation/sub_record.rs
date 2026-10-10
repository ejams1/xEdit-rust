// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! `TwbSubRecord`, `TwbSubRecordArray` and `TwbSubRecordStruct`: the
//! subrecords of a main record and the groups the definition makes of them.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, Weak};

use crate::interface::def::{NamedDef, ValueDef};
use crate::interface::element::{
    Container, CopyArgs, DataContainer, DataPtr, Element, ElementRef, FileRef, MainRecord, MainRecordRef,
};
use crate::interface::form_id::FormID;
use crate::interface::globals::{
    always_fast_assign, compare_raw_data, edit_allowed, ignore_records, is_internal_edit, sort_sub_records,
};
use crate::interface::misc::{EditError, Variant, progress};
use crate::interface::sub_record::RecordMemberDef;
use crate::interface::sub_record_group::RecordDef;
use crate::interface::types::{
    ASSIGN_ADD, ASSIGN_THIS, CallbackType, ConflictPriority, DefFlag, DefType, ElementType, KnownSubRecord, PascalEnum,
    Signature, TriBool,
};

use super::edit::{self, Storage};
use super::structs::SubRecordHeaderStruct;
use super::value::{Cursor, array_do_init, create_value_element, resolve, struct_do_init, union_do_init};
use super::{ContainerBase, DataBlock, ElementBase, ElementImpl, MainRecordImpl};

/// Port of `TwbSubRecord`.
pub struct SubRecordImpl {
    self_ref: Weak<SubRecordImpl>,
    pub(super) base: ElementBase,
    pub(super) container: ContainerBase,
    file: Weak<super::FileImpl>,
    block: DataBlock,
    sr_struct: SubRecordHeaderStruct,
    /// Port of `DoInit`: the value elements are built once.
    sr_init: super::InitOnce,
    /// Port of `srValueDef`: the resolved value definition, when the value
    /// has no name and its elements live in the subrecord itself.
    sr_value_def: RwLock<Option<Arc<dyn ValueDef>>>,
    /// The range of the subrecord data in `bytes`, after the header.
    dc_data_base: usize,
    dc_data_end: usize,
    sr_def: RwLock<Option<Arc<dyn RecordMemberDef>>>,
    sr_skipped: AtomicBool,
    /// Port of `dcDataStorage`: the data once the subrecord was changed.
    storage: Storage,
    /// Port of `srArraySizePrefix`.
    sr_array_size_prefix: AtomicUsize,
    /// Port of `srsIsArray` in `srStates`.
    sr_is_array: AtomicBool,
    /// Whether the subrecord was loaded with data (`dcDataBasePtr` set); a
    /// subrecord made from its definition has none until it takes storage.
    sr_has_data: AtomicBool,
    /// Port of `srsSorted` for an array value: the definition sorts the
    /// entries (under `wbSortSubRecords`).
    sr_sorted: AtomicBool,
    /// Port of `srsSortInvalid`: set by the init and by every change, the
    /// entries are sorted at the next `DoInit(True)` (an element read by
    /// index, the save).
    sr_sort_invalid: AtomicBool,
}

impl SubRecordImpl {
    /// Port of `TwbSubRecord.Create` from a pointer, with `InitDataPtr`:
    /// reads the header at `offset` and moves `offset` past the data. A
    /// subrecord with size 0 after an `XXXX` subrecord takes the size from it
    /// and the `XXXX` leaves the container.
    pub(super) fn create(
        file: &Arc<super::FileImpl>,
        block: &DataBlock,
        container: &ElementRef,
        container_base: &ContainerBase,
        data: &[u8],
        data_start: usize,
        offset: &mut usize,
    ) -> Option<Arc<Self>> {
        let header = SubRecordHeaderStruct::parse(data, *offset - data_start)?;
        let mut data_size = header.data_size as usize;
        if data_size == 0 {
            // The size of a subrecord that is too big for 16 bits comes before it.
            let elements = container_base.elements();
            if let Some(last) = elements.last()
                && let Some(xxxx) = last.as_element_impl().and_then(ElementImpl::sub_record_impl)
                && xxxx.get_signature() == Signature::new(b"XXXX")
                && let Some(size) = xxxx.data().and_then(|data| data.get(..4))
            {
                data_size = u32::from_le_bytes([size[0], size[1], size[2], size[3]]) as usize;
                container_base.remove_element(elements.len() - 1);
            }
        }
        let dc_data_base = *offset + SubRecordHeaderStruct::size();
        let dc_data_end = (dc_data_base + data_size).min(data_start + data.len());
        let sub_record = Arc::new_cyclic(|self_ref: &Weak<SubRecordImpl>| SubRecordImpl {
            self_ref: self_ref.clone(),
            base: ElementBase::new(Some(container)),
            container: ContainerBase::default(),
            file: Arc::downgrade(file),
            block: block.clone(),
            sr_struct: header,
            sr_init: super::InitOnce::new(),
            sr_value_def: RwLock::new(None),
            dc_data_base,
            dc_data_end,
            sr_def: RwLock::new(None),
            sr_skipped: AtomicBool::new(false),
            storage: Storage::default(),
            sr_array_size_prefix: AtomicUsize::new(0),
            sr_is_array: AtomicBool::new(false),
            sr_has_data: AtomicBool::new(true),
            sr_sorted: AtomicBool::new(false),
            sr_sort_invalid: AtomicBool::new(false),
        });
        container_base.add_element(sub_record.clone());
        *offset = dc_data_end;
        Some(sub_record)
    }

    /// Port of `TwbSubRecord.Create(aContainer, aSubRecordDef)`: a subrecord
    /// made from its definition, with storage of its data size and the
    /// default value.
    pub(super) fn create_new(
        container: &ElementRef,
        container_base: &ContainerBase,
        file: &Weak<super::FileImpl>,
        def: Arc<dyn RecordMemberDef>,
    ) -> Result<ElementRef, EditError> {
        let Some(file_arc) = file.upgrade() else {
            return Err("the file of the record is gone".to_owned());
        };
        let header = SubRecordHeaderStruct {
            signature: def.get_default_signature(),
            data_size: 0,
        };
        let sub_record = Arc::new_cyclic(|self_ref: &Weak<SubRecordImpl>| SubRecordImpl {
            self_ref: self_ref.clone(),
            base: ElementBase::new(Some(container)),
            container: ContainerBase::default(),
            file: Arc::downgrade(&file_arc),
            block: DataBlock::Buffer(Arc::new(Vec::new())),
            sr_struct: header,
            sr_init: super::InitOnce::new(),
            sr_value_def: RwLock::new(None),
            dc_data_base: 0,
            dc_data_end: 0,
            sr_def: RwLock::new(Some(def)),
            sr_skipped: AtomicBool::new(false),
            storage: Storage::default(),
            sr_array_size_prefix: AtomicUsize::new(0),
            sr_is_array: AtomicBool::new(false),
            sr_has_data: AtomicBool::new(false),
            sr_sorted: AtomicBool::new(false),
            sr_sort_invalid: AtomicBool::new(false),
        });
        container_base.add_element(sub_record.clone());
        sub_record.do_init();
        let created_empty = sub_record.container.cnt_as_created_empty.load(Ordering::Relaxed);
        let size = usize::try_from(sub_record.get_data_size()).unwrap_or(0);
        if let Some(bytes) = edit::request_storage_change(&*sub_record, size) {
            sub_record.storage.set(bytes);
            sub_record.sr_has_data.store(true, Ordering::Relaxed);
        }
        edit::set_to_default(&*sub_record)?;
        if created_empty {
            sub_record.container.cnt_as_created_empty.store(true, Ordering::Relaxed);
        }
        Ok(sub_record)
    }

    pub fn get_signature(&self) -> Signature {
        self.sr_struct.signature
    }

    /// Port of `GetDisplaySignature`: the `IAD` subrecords show their first
    /// byte as a number.
    pub fn get_display_signature(&self) -> String {
        let bytes = self.sr_struct.signature.0;
        if &bytes[1..] == b"IAD" {
            format!("#${:02X}IAD", bytes[0])
        } else {
            self.sr_struct.signature.to_string()
        }
    }

    /// Port of `TwbRecord.GetName`: the signature with its control
    /// characters shown as the letters from `a`.
    pub fn get_name_signature(&self) -> String {
        self.sr_struct
            .signature
            .0
            .iter()
            .map(|&byte| {
                if byte < 32 {
                    char::from(b'a' + byte)
                } else {
                    char::from(byte)
                }
            })
            .collect()
    }

    /// Port of `GetDataBasePtr` up to `GetDataEndPtr`: the data, rebuilt
    /// from the elements first when a child changed.
    pub fn data(&self) -> DataPtr<'_> {
        if self.storage.is_invalid() {
            edit::update_storage_from_elements(self);
        }
        self.data_raw()
    }

    /// The data as it is: the storage of a changed subrecord, else the bytes
    /// as loaded; `None` for a subrecord without data.
    fn data_raw(&self) -> DataPtr<'_> {
        if let Some(bytes) = self.storage.current() {
            return Some(bytes);
        }
        if !self.sr_has_data.load(Ordering::Relaxed) || self.storage.is_detached() {
            return None;
        }
        self.block.as_slice().get(self.dc_data_base..self.dc_data_end)
    }

    /// The block and range the elements are built over.
    fn data_source(&self) -> Option<(DataBlock, usize, usize)> {
        if let Some(block) = self.storage.current_block() {
            let len = block.as_slice().len();
            return Some((block, 0, len));
        }
        if !self.sr_has_data.load(Ordering::Relaxed) || self.storage.is_detached() {
            return None;
        }
        Some((self.block.clone(), self.dc_data_base, self.dc_data_end))
    }

    /// The header as loaded (`srStruct`).
    pub(super) fn header_struct(&self) -> SubRecordHeaderStruct {
        self.sr_struct
    }

    /// The bytes of the header as loaded (`dcBasePtr^`), before the data.
    pub(super) fn header_bytes(&self) -> &[u8] {
        let size = SubRecordHeaderStruct::size();
        self.block
            .as_slice()
            .get(self.dc_data_base - size..self.dc_data_base)
            .unwrap_or(&[])
    }

    fn element_ref(&self) -> ElementRef {
        self.self_ref.upgrade().expect("a subrecord is alive while it is used")
    }

    /// Port of `TwbSubRecord.DoInit(True)`: the init, then the entries of a
    /// sorted array value sorted by their sort keys when a change (or the
    /// init) made the order invalid.
    pub fn sorted_init(&self) {
        self.do_init();
        if super::sorting_allowed() && self.sr_sorted.load(Ordering::Relaxed) {
            edit::sort_if_invalid(&self.sr_sort_invalid, &self.container);
        }
    }

    /// Port of `TwbSubRecord.Init`: the value elements of the subrecord.
    /// The entries of a sorted array value stay in the loaded order until a
    /// `DoInit(True)` ([`Self::sorted_init`]) sorts them.
    pub fn do_init(&self) {
        self.sr_init.run(|| self.init());
    }

    /// The body of `TwbSubRecord.Init`, which `SetDataSize` runs again
    /// while the init runs (from an `AfterLoad` callback).
    fn init(&self) {
        {
            if self.skipped() {
                return;
            }
            let Some(def) = self.def() else { return };
            let Some(sub_record_def) = def.as_sub_record_def() else {
                return;
            };
            let Some(value) = sub_record_def.get_value() else {
                return;
            };
            let self_ref = self.element_ref();
            // A subrecord without data (made from its definition) builds its
            // elements without data too, as upstream does with `nil`.
            let (block, pos, end) = self
                .data_source()
                .unwrap_or_else(|| (DataBlock::Buffer(Arc::new(Vec::new())), 0, 0));
            let mut cursor = Cursor { block, pos, end };
            self.sr_array_size_prefix.store(0, Ordering::Relaxed);
            self.sr_is_array.store(false, Ordering::Relaxed);
            let value_def = resolve(value.clone(), cursor.data(), Some(&self_ref));
            if value_def.get_name().is_empty() || value.def_base().def_flags.contains(DefFlag::dfUnionStaticResolve) {
                *self.sr_value_def.write().unwrap() = Some(value_def.clone());
                match value_def.get_def_type() {
                    DefType::dtArray => {
                        self.sr_is_array.store(true, Ordering::Relaxed);
                        let (_, prefix) = array_do_init(&value_def, &self_ref, &self.file, &mut cursor);
                        self.sr_array_size_prefix.store(prefix, Ordering::Relaxed);
                    }
                    DefType::dtStruct | DefType::dtStructChapter => {
                        struct_do_init(&value_def, &self_ref, &self.file, &mut cursor)
                    }
                    DefType::dtUnion => {
                        if let Some(resolved) = union_do_init(&value_def, &self_ref, &self.file, &mut cursor) {
                            if resolved.get_def_type() == DefType::dtArray {
                                self.sr_is_array.store(true, Ordering::Relaxed);
                            }
                            *self.sr_value_def.write().unwrap() = Some(resolved);
                        }
                    }
                    _ => {}
                }
            } else {
                create_value_element(&self_ref, &self.file, &mut cursor, value_def, "");
            }
            // `srsSorted` and `srsSortInvalid` of a sorted array value (the
            // flags are built in their order already).
            let sorted = self.sr_is_array.load(Ordering::Relaxed)
                && sort_sub_records()
                && self
                    .value_def()
                    .is_some_and(|def| def.as_array_def().is_some_and(|array| array.get_sorted()));
            self.sr_sorted.store(sorted, Ordering::Relaxed);
            self.sr_sort_invalid.store(sorted, Ordering::Relaxed);
            // `srDef.AfterLoad(Self)`.
            def.after_load(&self_ref);
        }
    }

    /// Port of the `wbAssignAdd` branch of `TwbSubRecord.AssignInternal`
    /// without a source: one element added to the array of the subrecord,
    /// the element it was created with first.
    fn array_assign_add(&self) -> Result<Option<ElementRef>, EditError> {
        edit::check_edit_allowed(self)?;
        self.do_init();
        let Some(value_def) = self.value_def() else {
            return Err(format!("{} can not be assigned.", self.get_name()));
        };
        if value_def.get_def_type() != DefType::dtArray {
            return Ok(None);
        }
        let self_ref = self.element_ref();
        let result = if self.container.cnt_as_created_empty.load(Ordering::Relaxed) {
            self.set_modified(true);
            self.container.cnt_as_created_empty.store(false, Ordering::Relaxed);
            let first = self.container.element_at(0);
            // `Result.Assign(wbAssignThis, nil, aOnlySK)`: `CanAssignInternal`
            // refuses a nil source unless the edit is internal, where the
            // FormID formater's `Assign` sets the element to zero.
            if let Some(first) = &first
                && crate::interface::globals::is_internal_edit()
            {
                first.set_native_value(Variant::UInt(0))?;
            }
            first
        } else {
            let sorted = sort_sub_records() && value_def.as_array_def().is_some_and(|a| a.get_sorted());
            super::value::ValueImpl::array_assign_add(&self_ref, &self.container, &self.file, &value_def, sorted)?
                .map(|element| element as ElementRef)
        };
        edit::check_count(self, Some(&value_def));
        edit::check_terminator(self, Some(&value_def));
        Ok(result)
    }

    /// Port of `TwbSubRecord.AssignInternal`: an array takes the entries
    /// of the source or adds one; any other value assigns as a container.
    fn assign_internal_impl(
        &self,
        mut index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && !edit_allowed() {
            return Err(format!("{} can not be assigned.", self.get_name()));
        }
        self.do_init();
        let Some(value_def) = self.value_def() else {
            if super::assign::container_can_assign_internal(self, index, source, false) {
                return super::assign::container_assign_internal(self, index, source, only_sk);
            }
            return Ok(None);
        };
        if value_def.get_def_type() != DefType::dtArray {
            return super::assign::container_assign_internal(self, index, source, only_sk);
        }
        // An entry added without a source is `Add`, which `array_assign_add`
        // ports.
        if index == ASSIGN_ADD && source.is_none() {
            return self.array_assign_add();
        }
        let Some(array_def) = value_def.as_array_def() else {
            return Ok(None);
        };
        let self_ref = self.element_ref();
        let source_def = source.and_then(|source| super::assign::assign_def(&**source));
        let source_def_ref = super::assign::as_def(&source_def);
        let mut result = None;
        if index == ASSIGN_THIS
            && let Some(source) = source
            && value_def.can_assign(Some(&self_ref), index, source_def_ref)
        {
            if only_sk {
                return Ok(None);
            }
            let source_container = source.as_container();
            let source_count = source_container.map_or(0, |container| container.get_element_count());
            let fast = self
                .def()
                .is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfFastAssign))
                || value_def.def_base().def_flags.contains(DefFlag::dfFastAssign)
                || always_fast_assign();
            let source_data = source.as_data_container().and_then(|data| data.get_data());
            if fast && source.as_data_container().is_some() {
                self.set_modified(true);
                self.invalidate_storage();
                self.release_and_detach();
                self.storage.set(Vec::new());
                self.storage.set_invalid(false);
                let size = usize::try_from(source.get_data_size()).unwrap_or(0);
                if let Some(mut bytes) = self.request_storage_change_impl(size) {
                    if let Some(data) = source_data {
                        let len = size.min(data.len());
                        bytes[..len].copy_from_slice(&data[..len]);
                    }
                    self.commit_storage_impl(bytes);
                }
                self.reset_and_init();
                let children = self.container.elements();
                if let Some(source_container) = source_container {
                    for (i, child) in children.iter().enumerate() {
                        if let Some(source_child) = source_container.get_element(i as i32) {
                            child.assign(ASSIGN_THIS, Some(&source_child), only_sk);
                        }
                    }
                }
            } else if value_def.get_is_variable_size() {
                self.set_modified(true);
                self.invalidate_storage();
                self.container.release_elements();
                self.storage.set(Vec::new());
                self.storage.set_invalid(false);
                if array_def.get_count() < 0 {
                    // UPSTREAM-QUIRK: four bytes, whatever the size of the
                    // count prefix.
                    if let Some(bytes) = self.request_storage_change_impl(4) {
                        self.commit_storage_impl(bytes);
                    }
                } else if array_def.get_count() > 0 {
                    edit::value_set_to_default_internal(self)?;
                }
                let mut copy_count = source_count;
                if array_def.get_count() > 0 && copy_count > array_def.get_count() {
                    copy_count = array_def.get_count();
                }
                if let Some(source_container) = source_container {
                    for i in 0..copy_count {
                        let Some(source_child) = source_container.get_element(i) else {
                            continue;
                        };
                        match self.container.element_at(i as usize) {
                            Some(child) if child.get_element_type() != ElementType::etStringListTerminator => {
                                child.assign(ASSIGN_THIS, Some(&source_child), only_sk);
                            }
                            _ => {
                                self.assign(i, Some(&source_child), only_sk);
                            }
                        }
                    }
                }
            } else if let Some(source_container) = source_container {
                for i in 0..source_count {
                    if let (Some(child), Some(source_child)) =
                        (self.container.element_at(i as usize), source_container.get_element(i))
                    {
                        child.assign(ASSIGN_THIS, Some(&source_child), only_sk);
                    }
                }
            }
        } else {
            if index == ASSIGN_THIS && source.is_some_and(|source| source.as_main_record().is_some()) {
                index = ASSIGN_ADD;
            }
            if index >= 0
                && array_def.get_count() <= 0
                && (index == ASSIGN_ADD
                    || array_def
                        .get_element()
                        .can_assign(Some(&self_ref), ASSIGN_THIS, source_def_ref))
            {
                let sorted = sort_sub_records() && array_def.get_sorted();
                let suffix = if sorted {
                    String::new()
                } else {
                    format!("#{}", self.container.element_count())
                };
                if self.container.cnt_as_created_empty.load(Ordering::Relaxed) {
                    self.set_modified(true);
                    self.container.cnt_as_created_empty.store(false, Ordering::Relaxed);
                    result = self.container.element_at(0);
                    if let Some(result) = &result {
                        result.assign(ASSIGN_THIS, source, only_sk);
                    }
                } else if source.is_none_or(|source| source.get_element_type() != ElementType::etStringListTerminator)
                    && let Some(element_def) = super::value::ValueImpl::array_entry_def(&value_def, source)
                {
                    let element = super::value::ValueImpl::create_from(
                        &self_ref,
                        &self.file,
                        element_def,
                        source,
                        only_sk,
                        &suffix,
                    )?;
                    result = Some(element as ElementRef);
                }
            }
        }
        edit::check_count(self, Some(&value_def));
        edit::check_terminator(self, Some(&value_def));
        Ok(result)
    }

    /// Port of `TwbSubRecord.CanAssignInternal`.
    fn can_assign_internal_impl(&self, index: i32, source: Option<&ElementRef>, check_dont_show: bool) -> bool {
        if !is_internal_edit()
            && (!edit_allowed()
                || self
                    .def()
                    .is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfInternalEditOnly)))
        {
            return false;
        }
        if !super::assign::parent_allows_edit(self) {
            return false;
        }
        if check_dont_show && self.get_dont_show() {
            return false;
        }
        self.do_init();
        let is_array = self.sr_is_array.load(Ordering::Relaxed);
        let value_def = self.value_def();
        let array_def = value_def
            .as_ref()
            .filter(|_| is_array)
            .and_then(|def| def.as_array_def());
        let Some(source) = source else {
            return is_array && index == ASSIGN_ADD && array_def.is_some_and(|array| array.get_count() <= 0);
        };
        let source_def = super::assign::assign_def(&**source);
        let source_def = super::assign::as_def(&source_def);
        let self_ref = self.element_ref();
        if let (Some(value_def), Some(array_def)) = (&value_def, array_def) {
            value_def.can_assign(Some(&self_ref), index, source_def)
                || (array_def.get_count() <= 0
                    && array_def
                        .get_element()
                        .can_assign(Some(&self_ref), ASSIGN_THIS, source_def))
        } else {
            super::assign::container_can_assign_internal(self, index, Some(source), check_dont_show)
                || self
                    .def()
                    .is_some_and(|def| def.can_assign(Some(&self_ref), index, source_def))
        }
    }

    /// Port of `TwbSubRecord.AddIfMissingInternal`: an entry of an array, or
    /// a member of a structure, takes the value of the source.
    fn add_if_missing_internal_impl(
        &self,
        source: &ElementRef,
        args: &CopyArgs,
    ) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && !edit_allowed() {
            return Err(format!("{} can not be modified.", self.get_name()));
        }
        self.do_init();
        let not_implemented = || Err(format!("{}.AddIfMissingInternal is not implemented", self.get_name()));
        let Some(value_def) = self.value_def() else {
            return not_implemented();
        };
        let self_ref = self.element_ref();
        match value_def.get_def_type() {
            DefType::dtArray => {
                let sorted = sort_sub_records() && value_def.as_array_def().is_some_and(|array| array.get_sorted());
                if sorted && let Some(found) = super::value::find_by_sort_key(&self.container, source) {
                    if args.deep_copy {
                        found.assign(ASSIGN_THIS, Some(source), false);
                    }
                    return Ok(Some(found));
                }
                let suffix = if sorted {
                    String::new()
                } else {
                    format!("#{}", self.container.element_count())
                };
                let mut result = None;
                if source.get_element_type() != ElementType::etStringListTerminator {
                    if self.container.cnt_as_created_empty.load(Ordering::Relaxed) {
                        self.set_modified(true);
                        result = self.container.element_at(0);
                        self.container.cnt_as_created_empty.store(false, Ordering::Relaxed);
                        if let Some(result) = &result {
                            result.assign(ASSIGN_THIS, Some(source), !args.deep_copy);
                        }
                    } else if let Some(element_def) = super::value::ValueImpl::array_entry_def(&value_def, Some(source))
                    {
                        let element = super::value::ValueImpl::create_from(
                            &self_ref,
                            &self.file,
                            element_def,
                            Some(source),
                            !args.deep_copy,
                            &suffix,
                        )?;
                        result = Some(element as ElementRef);
                    }
                }
                edit::check_count(self, Some(&value_def));
                edit::check_terminator(self, Some(&value_def));
                Ok(result)
            }
            DefType::dtStruct | DefType::dtStructChapter => {
                let result = self.container.element_by_sort_order(source.get_sort_order());
                let Some(result) = result else {
                    return Err(format!("{} has no member for {}", self.get_name(), source.get_name()));
                };
                // A flags value is not copied into when only the sort key is.
                if !args.deep_copy
                    && result
                        .get_value_def()
                        .and_then(|def| {
                            let integer = def.as_integer_def()?;
                            integer.get_formater(Some(&result))
                        })
                        .is_some_and(|formater| formater.as_flags_def().is_some())
                {
                    return Ok(Some(result));
                }
                result.assign(ASSIGN_THIS, Some(source), !args.deep_copy);
                Ok(Some(result))
            }
            _ => not_implemented(),
        }
    }

    /// Port of `TwbSubRecord.MergeMultiple`: the elements of another
    /// subrecord of an array that may repeat (`dfMergeIfMultiple`) join the
    /// elements of this one, numbered from 0 again. An empty subrecord merges
    /// without elements.
    pub fn merge_multiple(&self, other: &SubRecordImpl) -> bool {
        if other.dc_data_end <= other.dc_data_base {
            return true;
        }
        let Some(def) = self.def() else { return false };
        if !def.def_base().def_flags.contains(DefFlag::dfMergeIfMultiple) {
            return false;
        }
        let Some(value_def) = self.value_def() else {
            return false;
        };
        if value_def.get_def_type() != DefType::dtArray {
            return false;
        }
        let self_ref = self.element_ref();
        let mut cursor = Cursor {
            block: other.block.clone(),
            pos: other.dc_data_base,
            end: other.dc_data_end,
        };
        array_do_init(&value_def, &self_ref, &self.file, &mut cursor);
        true
    }

    /// Port of `GetValueDef`: the value definition the subrecord data is
    /// read with. `None` when the value is a child element instead (a named
    /// value definition), like upstream `srValueDef`.
    fn value_def(&self) -> Option<Arc<dyn ValueDef>> {
        self.do_init();
        self.sr_value_def.read().unwrap().clone()
    }

    pub fn def(&self) -> Option<Arc<dyn RecordMemberDef>> {
        self.sr_def.read().unwrap().clone()
    }

    /// Port of `srValueDef` for the readers outside the subrecord: the
    /// resolved value definition when the value has no name and its
    /// elements live in the subrecord itself.
    pub fn resolved_value_def(&self) -> Option<Arc<dyn ValueDef>> {
        self.value_def()
    }

    /// Port of `TwbSubRecord.GetSorted` (`IwbSortableContainer`):
    /// `srsSorted` (the value is an array the definition sorts, under
    /// `wbSortSubRecords`, or flags under `wbFlagsAsArray`), or a value
    /// definition that is a sorted `wbEmpty`.
    pub fn get_sorted(&self) -> bool {
        if compare_raw_data() {
            return false;
        }
        let Some(value_def) = self.value_def() else {
            return false;
        };
        let self_ref = self.element_ref();
        let srs_sorted = match value_def.get_def_type() {
            DefType::dtArray => sort_sub_records() && value_def.as_array_def().is_some_and(|array| array.get_sorted()),
            DefType::dtStruct | DefType::dtStructChapter => false,
            _ => super::flag::is_flags(&self_ref),
        };
        srs_sorted
            || resolve(value_def, self.data(), Some(&self_ref))
                .as_empty_def()
                .is_some_and(|empty| empty.get_sorted())
    }

    /// Whether `DoInit(True)` sorts the entries of the subrecord: a sorted
    /// array value (`srsSortInvalid` is set for `srsSorted` without
    /// `srsIsFlags`).
    pub fn sorts_its_entries(&self) -> bool {
        self.sr_is_array.load(Ordering::Relaxed) && self.get_sorted()
    }

    /// Port of `TwbSubRecord.GetAlignable`: an unsorted array value of a
    /// variable count whose definitions do not forbid it (`dfNotAlignable`).
    /// UPSTREAM-QUIRK: upstream casts `srValueDef` to an array definition
    /// whenever `srsIsArray` is set, which raises for an array reached
    /// through a union the definitions could not resolve on load; the port
    /// keeps the resolved definition there and answers.
    pub fn get_alignable(&self) -> bool {
        if compare_raw_data() || self.get_sorted() {
            return false;
        }
        if self
            .def()
            .is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfNotAlignable))
        {
            return false;
        }
        let value_def = self.value_def();
        if value_def
            .as_ref()
            .is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfNotAlignable))
        {
            return false;
        }
        self.sr_is_array.load(Ordering::Relaxed)
            && value_def
                .and_then(|def| def.as_array_def().map(|array| array.get_count() <= 0))
                .unwrap_or(false)
    }

    /// Port of `SetDef` with its `DoReset(True)`: the elements built before
    /// the definition was known are dropped and built again on the next use.
    pub fn set_def(&self, def: Arc<dyn RecordMemberDef>) {
        *self.sr_def.write().unwrap() = Some(def);
        self.container.release_elements();
        *self.sr_value_def.write().unwrap() = None;
        self.sr_is_array.store(false, Ordering::Relaxed);
        self.sr_array_size_prefix.store(0, Ordering::Relaxed);
        self.sr_init.reset();
    }

    pub fn skipped(&self) -> bool {
        self.sr_skipped.load(Ordering::Relaxed)
    }

    pub fn set_skipped(&self, skipped: bool) {
        self.sr_skipped.store(skipped, Ordering::Relaxed);
    }
}

/// Port of `TwbSubRecordArray`: the subrecords of one array member.
pub struct SubRecordArrayImpl {
    self_ref: Weak<SubRecordArrayImpl>,
    pub(super) base: ElementBase,
    pub(super) container: ContainerBase,
    file: Weak<super::FileImpl>,
    arc_def: Arc<dyn RecordMemberDef>,
    /// Port of `arcSorted`: the definition sorts the members (under
    /// `wbSortSubRecords`).
    arc_sorted: AtomicBool,
    /// Port of `arcSortInvalid`: the members are to be sorted on the next
    /// `DoInit(True)`.
    arc_sort_invalid: AtomicBool,
    /// Port of `eGeneration`, for the name suffixes alone: the counter
    /// `arcNameGen` is compared with.
    e_generation: AtomicI32,
    /// Port of `arcNameGen`: the generation the members were numbered at.
    arc_name_gen: AtomicI32,
}

/// Port of `TwbSubRecordStruct`: the subrecords of one structure member.
pub struct SubRecordStructImpl {
    self_ref: Weak<SubRecordStructImpl>,
    pub(super) base: ElementBase,
    pub(super) container: ContainerBase,
    file: Weak<super::FileImpl>,
    src_def: Arc<dyn RecordMemberDef>,
}

/// Port of `TwbSubRecordArray.Create` with `DoProcess`: takes the subrecords
/// at `pos` of `owner` that the array element definition handles.
pub(super) fn create_sub_record_array(
    owner: &ElementRef,
    owner_base: &ContainerBase,
    pos: usize,
    def: Arc<dyn RecordMemberDef>,
    file: &Weak<super::FileImpl>,
) -> Arc<SubRecordArrayImpl> {
    let array = Arc::new_cyclic(|self_ref: &Weak<SubRecordArrayImpl>| SubRecordArrayImpl {
        self_ref: self_ref.clone(),
        base: ElementBase::new(Some(owner)),
        container: ContainerBase::default(),
        file: file.clone(),
        arc_def: def,
        arc_sorted: AtomicBool::new(false),
        arc_sort_invalid: AtomicBool::new(false),
        // `TwbElement.Create`: the generation starts at one, the numbering
        // at none, so the first name numbers the members.
        e_generation: AtomicI32::new(1),
        arc_name_gen: AtomicI32::new(0),
    });
    let array_ref: ElementRef = array.clone();
    array.do_process(&array_ref, owner_base, pos);
    array
}

impl SubRecordArrayImpl {
    /// Port of the `wbAssignAdd` branch of `TwbSubRecordArray.AssignInternal`
    /// without a source: one member added from the element definition.
    fn member_assign_add(&self) -> Result<Option<ElementRef>, EditError> {
        edit::check_edit_allowed(self)?;
        let Some(array_def) = self.arc_def.as_sub_record_array_def() else {
            return Ok(None);
        };
        let self_ref: ElementRef = self.self_ref.upgrade().expect("the array is alive");
        let mut element_def = array_def.get_element().clone();
        while element_def.get_def_type() == DefType::dtSubRecordUnion {
            element_def = element_def
                .as_record_def()
                .map(|union| union.get_member(0))
                .ok_or_else(|| format!("{} has an empty union member", self.get_name()))?;
        }
        let element: ElementRef = match element_def.get_def_type() {
            DefType::dtSubRecord => SubRecordImpl::create_new(&self_ref, &self.container, &self.file, element_def)?,
            DefType::dtSubRecordArray => {
                create_sub_record_array_new(&self_ref, &self.container, element_def, &self.file)?
            }
            DefType::dtSubRecordStruct => {
                create_sub_record_struct_new(&self_ref, &self.container, element_def, &self.file)?
            }
            other => return Err(format!("unexpected member type {other:?} in {}", self.get_name())),
        };
        self.sort_members();
        Ok(Some(element))
    }

    /// The tail of `TwbSubRecordArray.AssignInternal`: the definition of the
    /// members is sorted after every assignment, under `wbSortSubRecords`
    /// only.
    fn sort_members(&self) {
        let sorted = sort_sub_records()
            && self
                .arc_def
                .as_sub_record_array_def()
                .is_some_and(|array_def| array_def.get_sorted(self.base.container().as_ref()));
        if sorted {
            edit::sort_by_sort_keys(&self.container);
        }
        self.arc_sorted.store(sorted, Ordering::Relaxed);
    }

    /// Port of `TwbSubRecordArray.UpdateNameSuffixes`: the members are
    /// numbered in the order of the container, once per generation. A
    /// sorted array is left alone: upstream sets no suffix for it, so its
    /// members keep the names they have (none, for a loaded or copied
    /// array).
    fn update_name_suffixes_impl(&self) {
        if self.arc_sorted.load(Ordering::Relaxed) {
            return;
        }
        let generation = self.e_generation.load(Ordering::Relaxed);
        if self.arc_name_gen.load(Ordering::Relaxed) >= generation {
            return;
        }
        self.arc_name_gen.store(generation, Ordering::Relaxed);
        for (index, element) in self.container.elements().iter().enumerate() {
            if let Some(element) = element.as_element_impl() {
                element.element_base().set_name_suffix(&format!("#{index}"));
            }
        }
    }

    /// Port of `TwbSubRecordArray.AssignInternal`.
    fn assign_internal_impl(
        &self,
        index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && !edit_allowed() {
            return Err(format!("{} can not be assigned.", self.get_name()));
        }
        let Some(array_def) = self.arc_def.as_sub_record_array_def() else {
            return Ok(None);
        };
        // An entry added without a source is `Add`.
        if index >= 0 && source.is_none() {
            return self.member_assign_add();
        }
        let self_ref: ElementRef = self.self_ref.upgrade().expect("the array is alive");
        let source_def = source.and_then(|source| source.get_def());
        let source_def = source_def.as_deref().map(|def| def.as_dyn_def());
        let element_def = array_def.get_element().clone();
        let mut result = None;
        if index == ASSIGN_THIS
            && let Some(source) = source
            && self.arc_def.can_assign(Some(&self_ref), index, source_def)
        {
            if only_sk {
                return Ok(None);
            }
            self.set_modified(true);
            self.invalidate_storage();
            self.container.release_elements();
            if let Some(source_container) = source.as_container() {
                for i in 0..source_container.get_element_count() {
                    if let Some(source_child) = source_container.get_element(i) {
                        self.assign(i, Some(&source_child), only_sk);
                    }
                }
            }
        } else if let Some(source) = source
            && ((index >= 0 && element_def.can_assign(Some(&self_ref), ASSIGN_THIS, source_def))
                || (index == ASSIGN_THIS && element_def.can_assign(Some(&self_ref), index, source_def)))
        {
            let element = if self.container.cnt_as_created_empty.load(Ordering::Relaxed) {
                self.set_modified(true);
                self.container.cnt_as_created_empty.store(false, Ordering::Relaxed);
                self.container.element_at(0)
            } else {
                let mut member = element_def;
                while member.get_def_type() == DefType::dtSubRecordUnion {
                    let signature = source.get_has_signature().unwrap_or(Signature::new(b"NONE"));
                    member = member
                        .as_record_def()
                        .and_then(|union| union.get_member_for(Some(&self_ref), signature, Some(source)))
                        .ok_or_else(|| format!("{} has no member for {}", self.get_name(), source.get_name()))?;
                }
                Some(create_member_new(&self_ref, &self.container, &self.file, member)?)
            };
            if let Some(element) = &element {
                element.assign(ASSIGN_THIS, Some(source), only_sk);
                self.container.cnt_as_created_empty.store(false, Ordering::Relaxed);
            }
            result = element;
        }
        self.sort_members();
        Ok(result)
    }

    /// Port of `TwbSubRecordArray.CanAssignInternal`.
    fn can_assign_internal_impl(&self, index: i32, source: Option<&ElementRef>, check_dont_show: bool) -> bool {
        if !is_internal_edit()
            && (!edit_allowed() || self.arc_def.def_base().def_flags.contains(DefFlag::dfInternalEditOnly))
        {
            return false;
        }
        if !super::assign::parent_allows_edit(self) {
            return false;
        }
        if check_dont_show && self.get_dont_show() {
            return false;
        }
        let Some(source) = source else {
            return index >= 0;
        };
        let Some(array_def) = self.arc_def.as_sub_record_array_def() else {
            return false;
        };
        let self_ref: Option<ElementRef> = self.self_ref.upgrade().map(|array| array as ElementRef);
        let source_def = source.get_def();
        let source_def = source_def.as_deref().map(|def| def.as_dyn_def());
        if self.arc_def.can_assign(self_ref.as_ref(), index, source_def) {
            return true;
        }
        let element_def = array_def.get_element();
        element_def.can_assign(self_ref.as_ref(), ASSIGN_THIS, source_def)
            && !(check_dont_show && element_def.get_dont_show(Some(source)))
    }

    /// Port of `TwbSubRecordArray.AddIfMissingInternal`.
    fn add_if_missing_internal_impl(
        &self,
        source: &ElementRef,
        args: &CopyArgs,
    ) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && !edit_allowed() {
            return Err(format!("{} can not be assigned.", self.get_name()));
        }
        let Some(array_def) = self.arc_def.as_sub_record_array_def() else {
            return Ok(None);
        };
        let self_ref: ElementRef = self.self_ref.upgrade().expect("the array is alive");
        let sorted = sort_sub_records() && array_def.get_sorted(Some(&self_ref));
        if sorted
            && !args.as_new
            && let Some(found) = super::value::find_by_sort_key(&self.container, source)
        {
            if args.deep_copy {
                found.assign(ASSIGN_THIS, Some(source), false);
            }
            return Ok(Some(found));
        }
        let result = if self.container.cnt_as_created_empty.load(Ordering::Relaxed) {
            self.set_modified(true);
            self.container.cnt_as_created_empty.store(false, Ordering::Relaxed);
            self.container.element_at(0)
        } else {
            Some(create_member_new(
                &self_ref,
                &self.container,
                &self.file,
                array_def.get_element().clone(),
            )?)
        };
        if let Some(result) = &result {
            result.assign(ASSIGN_THIS, Some(source), !args.deep_copy);
        }
        Ok(result)
    }

    pub(super) fn do_process(&self, self_ref: &ElementRef, container: &ContainerBase, mut pos: usize) {
        let Some(array_def) = self.arc_def.as_sub_record_array_def() else {
            return;
        };
        loop {
            let elements = container.elements();
            let Some(element) = elements.get(pos) else { break };
            let Some(sub_record) = element.as_element_impl().and_then(ElementImpl::sub_record_impl) else {
                break;
            };
            if sub_record.skipped() {
                pos += 1;
                continue;
            }
            let mut element_def = array_def.get_element().clone();
            if element_def.get_def_type() == DefType::dtSubRecordUnion {
                let Some(member) = element_def
                    .as_record_def()
                    .and_then(|union| union.get_member_for(Some(self_ref), sub_record.get_signature(), Some(element)))
                else {
                    break;
                };
                element_def = member;
            }
            if !element_def.can_handle(Some(self_ref), sub_record.get_signature(), Some(element)) {
                break;
            }
            match element_def.get_def_type() {
                DefType::dtSubRecord => {
                    container.remove_element(pos);
                    sub_record.set_def(element_def);
                    sub_record.base.set_container(self_ref);
                    self.container.add_element(element.clone());
                }
                DefType::dtSubRecordArray => {
                    let nested = create_sub_record_array(self_ref, container, pos, element_def, &self.file);
                    self.container.add_element(nested);
                }
                DefType::dtSubRecordStruct => {
                    let nested = create_sub_record_struct(self_ref, container, pos, element_def, &self.file);
                    self.container.add_element(nested);
                }
                _ => panic!(
                    "Unexpected def type for SubRecord {} in array",
                    sub_record.get_signature()
                ),
            }
        }
        // The members are sorted on the first `DoInit(True)`, under
        // `wbSortSubRecords` only.
        let sorted = sort_sub_records() && array_def.get_sorted(self.base.container().as_ref());
        self.arc_sorted.store(sorted, Ordering::Relaxed);
        self.arc_sort_invalid.store(sorted, Ordering::Relaxed);
        // The names are numbered lazily (`UpdateNameSuffixes`), not here.
    }
}

/// The `case Member.DefType of` of the assign methods: a subrecord, a
/// subrecord array or a subrecord structure made from its definition.
pub(super) fn create_member_new(
    owner: &ElementRef,
    owner_base: &ContainerBase,
    file: &Weak<super::FileImpl>,
    def: Arc<dyn RecordMemberDef>,
) -> Result<ElementRef, EditError> {
    match def.get_def_type() {
        DefType::dtSubRecord => SubRecordImpl::create_new(owner, owner_base, file, def),
        DefType::dtSubRecordArray => create_sub_record_array_new(owner, owner_base, def, file),
        DefType::dtSubRecordStruct => create_sub_record_struct_new(owner, owner_base, def, file),
        other => Err(format!("unexpected member type {other:?} in {}", owner.get_name())),
    }
}

/// Port of `TwbSubRecordArray.Create` with `aPos = Low(Integer)`: an array
/// made from its definition, with as many members as its definition needs
/// and their default edit values.
pub(super) fn create_sub_record_array_new(
    owner: &ElementRef,
    owner_base: &ContainerBase,
    def: Arc<dyn RecordMemberDef>,
    file: &Weak<super::FileImpl>,
) -> Result<ElementRef, EditError> {
    let array = Arc::new_cyclic(|self_ref: &Weak<SubRecordArrayImpl>| SubRecordArrayImpl {
        self_ref: self_ref.clone(),
        base: ElementBase::new(Some(owner)),
        container: ContainerBase::default(),
        file: file.clone(),
        arc_def: def,
        arc_sorted: AtomicBool::new(false),
        arc_sort_invalid: AtomicBool::new(false),
        // `TwbElement.Create`: the generation starts at one, the numbering
        // at none, so the first name numbers the members.
        e_generation: AtomicI32::new(1),
        arc_name_gen: AtomicI32::new(0),
    });
    let array_ref: ElementRef = array.clone();
    if let Some(array_def) = array.arc_def.as_sub_record_array_def() {
        let defaults = array_def.get_default_edit_values();
        let mut min_count = usize::from(!array.arc_def.def_base().def_flags.contains(DefFlag::dfArrayCanBeEmpty));
        min_count = min_count
            .max(usize::try_from(array_def.get_count()).unwrap_or(0))
            .max(defaults.len());
        while array.container.element_count() < min_count {
            if array.member_assign_add()?.is_none() {
                break;
            }
        }
        for (element, default) in array.container.elements().iter().zip(&defaults) {
            element.set_edit_value(default)?;
        }
        array.container.cnt_as_created_empty.store(true, Ordering::Relaxed);
    }
    owner_base.add_element(array_ref.clone());
    array.set_modified(true);
    array.invalidate_storage();
    Ok(array_ref)
}

/// Port of `TwbSubRecordStruct.Create` with `aPos = Low(Integer)`: a
/// structure made from its definition, with its required members.
pub(super) fn create_sub_record_struct_new(
    owner: &ElementRef,
    owner_base: &ContainerBase,
    def: Arc<dyn RecordMemberDef>,
    file: &Weak<super::FileImpl>,
) -> Result<ElementRef, EditError> {
    let structure = Arc::new_cyclic(|self_ref: &Weak<SubRecordStructImpl>| SubRecordStructImpl {
        self_ref: self_ref.clone(),
        base: ElementBase::new(Some(owner)),
        container: ContainerBase::default(),
        file: file.clone(),
        src_def: def,
    });
    let self_ref: ElementRef = structure.clone();
    owner_base.add_element(self_ref.clone());
    structure.add_required_elements()?;
    Ok(self_ref)
}

impl SubRecordStructImpl {
    /// Port of `AddRequiredElements`: the first member unless the structure
    /// allows any order or says the first is not required, and every
    /// required member.
    fn add_required_elements(&self) -> Result<(), EditError> {
        let Some(src_def) = self.src_def.as_record_def() else {
            return Ok(());
        };
        let self_ref: ElementRef = self.self_ref.upgrade().expect("the structure is alive");
        let first_required = !(src_def.allow_unordered()
            || self
                .src_def
                .def_base()
                .def_flags
                .contains(DefFlag::dfStructFirstNotRequired));
        let count = usize::try_from(src_def.get_member_count()).unwrap_or(0);
        for index in 0..count {
            let mut member = src_def.get_member(index);
            if !((index == 0 && first_required) || member.def_base().def_required()) {
                continue;
            }
            if member.get_def_type() == DefType::dtSubRecordUnion {
                member = member
                    .as_record_def()
                    .map(|union| union.get_member(0))
                    .ok_or_else(|| format!("{} has an empty union member", self.get_name()))?;
            }
            let element: ElementRef = match member.get_def_type() {
                DefType::dtSubRecord => SubRecordImpl::create_new(&self_ref, &self.container, &self.file, member)?,
                DefType::dtSubRecordArray => {
                    create_sub_record_array_new(&self_ref, &self.container, member, &self.file)?
                }
                DefType::dtSubRecordStruct => {
                    create_sub_record_struct_new(&self_ref, &self.container, member, &self.file)?
                }
                other => return Err(format!("unexpected member type {other:?} in {}", self.get_name())),
            };
            if let Some(element) = element.as_element_impl() {
                element.set_sort_and_memory_order(index as i32);
            }
        }
        Ok(())
    }
}

impl SubRecordStructImpl {
    /// Port of `TwbSubRecordStruct.Add`: the member with the signature,
    /// added when it is missing.
    fn add_by_name(&self, name: &str) -> Result<Option<ElementRef>, EditError> {
        if !self.parent_editable() {
            return Err(format!("\"{}\" is not editable", self.get_name()));
        }
        let Ok(signature) = Signature::from_str(&name.chars().take(4).collect::<String>()) else {
            return Ok(None);
        };
        if name.chars().count() < 4 {
            return Ok(None);
        }
        if let Some(found) = self.get_element_by_signature(signature) {
            return Ok(Some(found));
        }
        let self_ref: ElementRef = self.self_ref.upgrade().expect("the structure is alive");
        let Some(src_def) = self.src_def.as_record_def() else {
            return Ok(None);
        };
        let index = src_def.get_member_index_for(Some(&self_ref), signature, None);
        if index < 0 {
            return Ok(None);
        }
        self.assign(index, None, false);
        Ok(self.get_element_by_signature(signature))
    }

    /// `IsElementEditable(nil)` of the structure itself.
    fn parent_editable(&self) -> bool {
        super::assign::container_is_element_editable(self, None)
    }

    /// Port of `TwbSubRecordStruct.AssignInternal`.
    fn assign_internal_impl(
        &self,
        index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && !edit_allowed() {
            return Err(format!("{} can not be assigned.", self.get_name()));
        }
        let Some(src_def) = self.src_def.as_record_def() else {
            return Ok(None);
        };
        let self_ref: ElementRef = self.self_ref.upgrade().expect("the structure is alive");
        let mut result = None;
        if index == ASSIGN_THIS {
            if let Some(source) = source
                && !self
                    .src_def
                    .equals(source.get_def().as_deref().map(|def| def.as_dyn_def()))
            {
                // A member of a union of structures switches to the member
                // the source is.
                let source_def = source.get_def().and_then(|def| def.def_ref().into_record_member_def());
                let union_of = |def: &dyn crate::interface::def::Def| {
                    def.get_parent()
                        .filter(|parent| parent.as_sub_record_union_def().is_some())
                };
                if let Some(source_def) = source_def
                    && let (Some(own_union), Some(source_union)) =
                        (union_of(self.src_def.as_dyn_def()), union_of(source_def.as_dyn_def()))
                    && own_union.equals(Some(&*source_union))
                    && let Some(container) = self.base.container()
                    && let Some(container_impl) = container.as_element_impl()
                    && let Some(container_base) = container_impl.container_base()
                {
                    let replacement = create_member_new(&container, container_base, &self.file, source_def)?;
                    if let Some(replacement_impl) = replacement.as_element_impl() {
                        replacement_impl.set_sort_and_memory_order(self.get_sort_order());
                    }
                    replacement.assign(ASSIGN_THIS, Some(source), only_sk);
                    container_impl.remove_child(&self_ref, true);
                    if let Some(container) = container.as_container() {
                        container.sort_by_sort_order();
                    }
                }
                return Ok(None);
            }
            self.set_modified(true);
            self.invalidate_storage();
            self.container.release_elements();
            self.add_required_elements()?;
            if let Some(source_container) = source.and_then(|source| source.as_container()) {
                for i in 0..source_container.get_element_count() {
                    let Some(child) = source_container.get_element(i) else {
                        continue;
                    };
                    if !only_sk || self.get_is_in_sk(child.get_sort_order()) {
                        self.assign(child.get_sort_order(), Some(&child), only_sk);
                    }
                }
            }
        } else if index >= 0 && index < src_def.get_member_count() {
            let mut member = src_def.get_member(index as usize);
            let source_def = source.and_then(|source| source.get_def());
            let source_def_ref = source_def.as_deref().map(|def| def.as_dyn_def());
            if source.is_none() || member.can_assign(Some(&self_ref), ASSIGN_THIS, source_def_ref) {
                let existing = self.container.element_by_sort_order(index);
                if let Some(existing) = existing {
                    if let Some(source) = source {
                        existing.assign(ASSIGN_THIS, Some(source), only_sk);
                    }
                    result = Some(existing);
                } else {
                    if member.get_def_type() == DefType::dtSubRecordUnion {
                        match source_def.and_then(|def| def.def_ref().into_record_member_def()) {
                            Some(source_member) => member = source_member,
                            None => {
                                while member.get_def_type() == DefType::dtSubRecordUnion {
                                    member = member
                                        .as_record_def()
                                        .map(|union| union.get_member(0))
                                        .ok_or_else(|| format!("{} has an empty union member", self.get_name()))?;
                                }
                            }
                        }
                    }
                    let element = create_member_new(&self_ref, &self.container, &self.file, member)?;
                    if let Some(element_impl) = element.as_element_impl() {
                        element_impl.element_base().e_sort_order.store(index, Ordering::Relaxed);
                    }
                    if let Some(source) = source {
                        element.assign(ASSIGN_THIS, Some(source), only_sk);
                    }
                    result = Some(element);
                }
            }
        }
        edit::sort_sub_records_of(self);
        Ok(result)
    }

    /// Port of `TwbSubRecordStruct.CanAssignInternal`.
    fn can_assign_internal_impl(&self, index: i32, source: Option<&ElementRef>, check_dont_show: bool) -> bool {
        if !is_internal_edit()
            && (!edit_allowed() || self.src_def.def_base().def_flags.contains(DefFlag::dfInternalEditOnly))
        {
            return false;
        }
        if !super::assign::parent_allows_edit(self) {
            return false;
        }
        if check_dont_show && self.get_dont_show() {
            return false;
        }
        let Some(src_def) = self.src_def.as_record_def() else {
            return false;
        };
        let self_ref: Option<ElementRef> = self.self_ref.upgrade().map(|structure| structure as ElementRef);
        let member_ok = |index: i32| {
            let member = src_def.get_member(index as usize);
            !(check_dont_show && member.get_dont_show(self_ref.as_ref()))
                && (is_internal_edit() || !member.def_base().def_flags.contains(DefFlag::dfInternalEditOnly))
        };
        let Some(source) = source else {
            return index >= 0
                && index < src_def.get_member_count()
                && self.container.element_by_sort_order(index).is_none()
                && member_ok(index);
        };
        let source_def = source.get_def();
        let source_def_ref = source_def.as_deref().map(|def| def.as_dyn_def());
        if index == ASSIGN_THIS {
            if self.src_def.equals(source_def_ref) {
                return true;
            }
            let union_of = |def: &dyn crate::interface::def::Def| {
                def.get_parent()
                    .filter(|parent| parent.as_sub_record_union_def().is_some())
            };
            return match (union_of(self.src_def.as_dyn_def()), source_def_ref.and_then(union_of)) {
                (Some(own), Some(other)) => own.equals(Some(&*other)),
                _ => false,
            };
        }
        index >= 0
            && index < src_def.get_member_count()
            && src_def
                .get_member(index as usize)
                .can_assign(self_ref.as_ref(), ASSIGN_THIS, source_def_ref)
            && member_ok(index)
    }

    /// Port of `TwbSubRecordStruct.AddIfMissingInternal`.
    fn add_if_missing_internal_impl(
        &self,
        source: &ElementRef,
        args: &CopyArgs,
    ) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && !edit_allowed() {
            return Err(format!("{} can not be assigned.", self.get_name()));
        }
        let index = source.get_sort_order();
        if let Some(result) = self.container.element_by_sort_order(index) {
            result.assign(ASSIGN_THIS, Some(source), !args.deep_copy);
            return Ok(Some(result));
        }
        self.assign(index, Some(source), !args.deep_copy);
        let result = self.container.element_by_sort_order(index);
        edit::sort_sub_records_of(self);
        Ok(result)
    }

    /// Port of `TwbSubRecordStruct.TryAssignMembers`: the members of the
    /// source go into the members with the same name, and the first
    /// member of the sort key into the first member of this sort key.
    pub fn try_assign_members(&self, source: &ElementRef) {
        let Some(source_container) = source.as_container() else {
            return;
        };
        let Some(source_def) = source.get_def() else { return };
        let Some(source_record_def) = source_def.as_record_def() else {
            return;
        };
        let Some(own_def) = self.src_def.as_record_def() else {
            return;
        };
        for i in 0..source_container.get_element_count() {
            let Some(source_element) = source_container.get_element(i) else {
                continue;
            };
            let target = self.get_element_by_name(&source_element.get_name());
            if let Some(target) = target
                && target.can_assign(ASSIGN_THIS, Some(&source_element), false)
            {
                target.assign(ASSIGN_THIS, Some(&source_element), false);
                continue;
            }
            let Some(source_element_def) = source_element.get_def() else {
                continue;
            };
            let target_index = (0..own_def.get_member_count()).find(|&j| {
                let member = own_def.get_member(j as usize);
                member.get_name() == source_element_def.get_name()
                    && member.can_assign(None, ASSIGN_THIS, Some(source_element_def.as_dyn_def()))
            });
            if let Some(target_index) = target_index {
                self.assign(target_index, Some(&source_element), false);
            }
        }
        let (Some(own_struct), Some(source_struct)) = (
            self.src_def.as_sub_record_struct_def(),
            source_def.as_sub_record_struct_def(),
        ) else {
            return;
        };
        if !own_struct.has_sort_key()
            || !source_struct.has_sort_key()
            || own_struct.get_sort_key_count(false) < 1
            || source_struct.get_sort_key_count(false) < 1
        {
            return;
        }
        let target_index = own_struct.get_sort_key(0, false);
        let source_index = source_struct.get_sort_key(0, false);
        let target_member = own_def.get_member(target_index as usize);
        let source_member = source_record_def.get_member(source_index as usize);
        if target_member.can_assign(None, ASSIGN_THIS, Some(source_member.as_dyn_def()))
            && let Some(source_element) = source_container.get_element_by_sort_order(source_index)
        {
            self.assign(target_index, Some(&source_element), false);
        }
    }
}

/// Port of `TwbSubRecordStruct.Create` from the subrecords at `pos` of `owner`.
pub(super) fn create_sub_record_struct(
    owner: &ElementRef,
    owner_base: &ContainerBase,
    mut pos: usize,
    def: Arc<dyn RecordMemberDef>,
    file: &Weak<super::FileImpl>,
) -> Arc<SubRecordStructImpl> {
    let structure = Arc::new_cyclic(|self_ref: &Weak<SubRecordStructImpl>| SubRecordStructImpl {
        self_ref: self_ref.clone(),
        base: ElementBase::new(Some(owner)),
        container: ContainerBase::default(),
        file: file.clone(),
        src_def: def,
    });
    let self_ref: ElementRef = structure.clone();
    let Some(src_def) = structure.src_def.as_record_def() else {
        return structure;
    };
    let member_count = usize::try_from(src_def.get_member_count()).unwrap_or(0);
    let mut found_members: Vec<Option<ElementRef>> = vec![None; member_count];
    let mut current_def_pos = 0;
    let mut last_def: Option<Arc<dyn RecordMemberDef>> = None;
    let mut last_element: Option<ElementRef> = None;
    while current_def_pos < member_count {
        let elements = owner_base.elements();
        let Some(element) = elements.get(pos) else { break };
        let Some(current_rec) = element.as_element_impl().and_then(ElementImpl::sub_record_impl) else {
            break;
        };
        if current_rec.skipped() {
            pos += 1;
            continue;
        }
        let signature = current_rec.get_signature();
        if !src_def.contains_member_for(Some(&self_ref), signature, Some(element)) {
            if src_def.get_skip_signature(signature) {
                pos += 1;
                continue;
            }
            break;
        }
        if src_def.allow_unordered() {
            let index = src_def.get_member_index_for(Some(&self_ref), signature, Some(element));
            if index < 0 {
                progress(&format!(
                    "Error: record {} contains unexpected (or out of order) subrecord {} {:08X}",
                    structure.get_signature(),
                    signature,
                    signature.to_int()
                ));
                pos += 1;
                continue;
            }
            current_def_pos = index as usize;
        }
        let mut current_def = src_def.get_member(current_def_pos);
        if !current_def.can_handle(Some(&self_ref), signature, Some(element)) {
            // A repeated subrecord of an array that may repeat joins the
            // last element.
            if let Some(last) = &last_def
                && last.def_base().def_flags.contains(DefFlag::dfMergeIfMultiple)
                && last.can_handle(Some(&self_ref), signature, Some(element))
                && let Some(last_record) = last_element
                    .as_ref()
                    .and_then(|last| last.as_element_impl())
                    .and_then(ElementImpl::sub_record_impl)
                && last_record.merge_multiple(current_rec)
            {
                owner_base.remove_element(pos);
                continue;
            }
            current_def_pos += 1;
            continue;
        }
        if current_def.get_def_type() == DefType::dtSubRecordUnion {
            current_def = current_def
                .as_record_def()
                .and_then(|union| union.get_member_for(Some(&self_ref), signature, Some(element)))
                .expect("the union has a member for the subrecord");
        }
        if found_members[current_def_pos].is_some() {
            // Duplicate members are not allowed.
            break;
        }
        let new_element: ElementRef = match current_def.get_def_type() {
            DefType::dtSubRecord => {
                owner_base.remove_element(pos);
                current_rec.set_def(current_def.clone());
                current_rec.base.set_container(&self_ref);
                structure.container.add_element(element.clone());
                element.clone()
            }
            DefType::dtSubRecordArray => {
                let nested = create_sub_record_array(&self_ref, owner_base, pos, current_def.clone(), file);
                structure.container.add_element(nested.clone());
                nested
            }
            DefType::dtSubRecordStruct => {
                let nested = create_sub_record_struct(&self_ref, owner_base, pos, current_def.clone(), file);
                structure.container.add_element(nested.clone());
                nested
            }
            _ => panic!("Unexpected def type for SubRecord {signature}"),
        };
        new_element.set_orders(current_def_pos as i32);
        found_members[current_def_pos] = Some(new_element.clone());
        let terminator = current_def.def_base().def_flags.contains(DefFlag::dfTerminator);
        last_def = Some(current_def);
        last_element = Some(new_element);
        if terminator {
            break;
        }
        if src_def.allow_unordered() {
            current_def_pos = 0;
        } else {
            current_def_pos += 1;
        }
    }
    structure
}

impl SubRecordStructImpl {
    pub fn get_signature(&self) -> Signature {
        self.src_def.get_default_signature()
    }
}

/// Port of `TwbMainRecord.Init` for the subrecords: scans them from the
/// record data and groups them by the record definition.
pub(super) fn init_main_record(record: &Arc<MainRecordImpl>) {
    let Some(file) = record.file.upgrade() else { return };
    let self_ref: ElementRef = record.clone();
    let Some((block, data_start, end)) = record.data_block() else {
        return;
    };
    let data = &block.as_slice()[data_start..end];
    let mut offset = data_start;
    let ignored = ignore_records();
    // `lSubRecords` of `DBGSUBREC`, which the 4.1.5q release is built with:
    // the subrecords as read, for the message of a record with errors.
    let mut sub_records: Vec<String> = Vec::new();
    while offset < end {
        let Some(sub_record) = SubRecordImpl::create(
            &file,
            &block,
            &self_ref,
            &record.container,
            data,
            data_start,
            &mut offset,
        ) else {
            break;
        };
        let signature = sub_record.get_signature();
        if ignored.contains(&signature) || record.mr_def.as_ref().is_some_and(|def| def.should_ignore(signature)) {
            sub_record.set_skipped(true);
        }
        sub_records.push(sub_record.get_display_signature());
    }
    let Some(mr_def) = &record.mr_def else { return };

    let member_count = usize::try_from(mr_def.get_member_count()).unwrap_or(0);
    let mut last_element_for_member: Vec<Option<ElementRef>> = vec![None; member_count];
    let mut current_def_pos = 0usize;
    let mut current_rec_pos = 0usize;
    let mut found_error = false;
    // `Exclude(mrStates, mrsGridCellChecked)`.
    let quick_init_limit = mr_def.get_quick_init_limit();
    record.mr_grid_cell.lock().unwrap().checked = false;
    loop {
        let elements = record.container.elements();
        let Some(element) = elements.get(current_rec_pos) else {
            break;
        };
        let Some(current_rec) = element.as_element_impl().and_then(ElementImpl::sub_record_impl) else {
            current_rec_pos += 1;
            continue;
        };
        if current_rec.skipped() {
            current_rec_pos += 1;
            continue;
        }
        let signature = current_rec.get_signature();
        let unexpected = |found_error: &mut bool| {
            progress(&format!(
                "Error: record {} contains unexpected (or out of order) subrecord {} {:08X}",
                record.get_signature(),
                current_rec.get_display_signature(),
                signature.to_int()
            ));
            *found_error = true;
        };
        let mut current_def: Arc<dyn RecordMemberDef>;
        if mr_def.allow_unordered() {
            let index = mr_def.get_member_index_for(Some(&self_ref), signature, Some(element));
            if index < 0 {
                unexpected(&mut found_error);
                current_rec_pos += 1;
                continue;
            }
            current_def_pos = index as usize;
            current_def = mr_def.get_member(current_def_pos);
        } else {
            if !mr_def.contains_member_for(Some(&self_ref), signature, Some(element)) {
                unexpected(&mut found_error);
                current_rec_pos += 1;
                continue;
            }
            if current_def_pos < member_count {
                current_def = mr_def.get_member(current_def_pos);
                if !current_def.can_handle(Some(&self_ref), signature, Some(element)) {
                    current_def_pos += 1;
                    continue;
                }
            } else {
                progress(&format!(
                    "Error: record {} contains unexpected (or out of order) subrecord {}",
                    record.get_signature(),
                    current_rec.get_display_signature()
                ));
                found_error = true;
                current_rec_pos += 1;
                continue;
            }
        }
        // Past `QuickInitLimit` the grid cell counts as checked (where the
        // quick init of upstream stops).
        if current_def_pos as i32 > quick_init_limit {
            record.mr_grid_cell.lock().unwrap().checked = true;
        }
        if current_def.get_def_type() == DefType::dtSubRecordUnion {
            current_def = current_def
                .as_record_def()
                .and_then(|union| union.get_member_for(Some(&self_ref), signature, Some(element)))
                .expect("the union has a member for the subrecord");
        }
        match current_def.get_def_type() {
            DefType::dtSubRecord => {
                current_rec.set_def(current_def.clone());
                let known = mr_def.known_sub_record_signatures();
                if signature == known[0] {
                    record.cache_editor_id(mr_def.get_editor_id(element));
                } else if signature == known[1] {
                    record.set_full_name(element.get_edit_value());
                } else if signature == known[KnownSubRecord::ksrGridCell.ord()]
                    && !record.mr_grid_cell.lock().unwrap().checked
                {
                    // `mrsHasGridCell` and `mrGridCell` from the subrecord.
                    let cell = mr_def.get_grid_cell(element);
                    let mut grid_cell = record.mr_grid_cell.lock().unwrap();
                    grid_cell.cell = cell.map(|cell| (cell.x, cell.y));
                    grid_cell.checked = true;
                }
            }
            DefType::dtSubRecordArray => {
                if let Some(last) = &last_element_for_member[current_def_pos]
                    && let Some(array) = last.as_element_impl().and_then(ElementImpl::sub_record_array_impl)
                {
                    array.do_process(last, &record.container, current_rec_pos);
                    continue;
                }
                let array = create_sub_record_array(
                    &self_ref,
                    &record.container,
                    current_rec_pos,
                    current_def.clone(),
                    &record.file,
                );
                record.container.insert_element(current_rec_pos, array);
            }
            DefType::dtSubRecordStruct => {
                let structure = create_sub_record_struct(
                    &self_ref,
                    &record.container,
                    current_rec_pos,
                    current_def.clone(),
                    &record.file,
                );
                record.container.insert_element(current_rec_pos, structure);
            }
            _ => panic!(
                "Unexpected def type for SubRecord {signature} in {}",
                record.get_signature()
            ),
        }
        let placed = record.container.elements()[current_rec_pos].clone();
        placed.set_orders(current_def_pos as i32);
        last_element_for_member[current_def_pos] = Some(placed);
        current_rec_pos += 1;
        current_def_pos += 1;
    }
    // The subrecords after the last member are unexpected.
    for element in record.container.elements().iter().skip(current_rec_pos) {
        if let Some(sub_record) = element.as_element_impl().and_then(ElementImpl::sub_record_impl)
            && !sub_record.skipped()
        {
            progress(&format!(
                "Error: record {} contains unexpected (or out of order) subrecord {}",
                record.get_signature(),
                sub_record.get_signature()
            ));
            found_error = true;
        }
    }
    if found_error {
        progress(&format!("Errors were found in: {}", record.get_name()));
        progress(&format!("Contained subrecords: {}", sub_records.join(" ")));
    }
    if sort_sub_records() && (mr_def.allow_unordered() || record.base.has_state(super::ElementState::esModified)) {
        edit::sort_sub_records_of(&**record);
    }
}

/// Port of the tail of `TwbMainRecord.Init` under `wbAllowInternalEdit`:
/// the required members the record lacks are added (`Adding missing
/// record`), as an internal edit. Runs after the offsets of a worldspace
/// are dropped, as upstream.
pub(super) fn add_required_members(record: &Arc<MainRecordImpl>) {
    let Some(mr_def) = &record.mr_def else { return };
    let flags = record.mr_struct().flags;
    if flags.is_deleted() || record.get_is_partial_form() {
        return;
    }
    let count = usize::try_from(mr_def.get_member_count()).unwrap_or(0);
    let present: Vec<bool> = {
        let elements = record.container.elements();
        (0..count)
            .map(|index| elements.iter().any(|element| element.get_sort_order() == index as i32))
            .collect()
    };
    let missing: Vec<usize> = (0..count)
        .filter(|&index| mr_def.get_member(index).def_base().def_required() && !present[index])
        .collect();
    if missing.is_empty() {
        return;
    }
    if !crate::interface::globals::begin_internal_edit(false) {
        return;
    }
    for index in missing {
        if crate::interface::globals::more_info_for_required() {
            progress(&format!(
                " [{}] Adding missing record: {}",
                record.get_fixed_form_id().to_string(true),
                mr_def.get_member(index).get_name()
            ));
        }
        if let Err(error) = record.assign_member(index) {
            progress(&format!(
                "Error assigning to [{}] from [nil]: [Exception] {error}",
                record.get_full_path()
            ));
        }
    }
    crate::interface::globals::end_internal_edit();
}

// ----- the element traits -----

trait SetOrders {
    fn set_orders(&self, order: i32);
}

impl SetOrders for ElementRef {
    fn set_orders(&self, order: i32) {
        if let Some(element) = self.as_element_impl() {
            element.set_sort_and_memory_order(order);
        }
    }
}

impl Element for SubRecordImpl {
    element_common!(element_base, own_values);

    /// Port of `TwbSubRecord.GetName`: the signature and the name of the
    /// definition.
    /// Port of `TwbSubRecord.GetName`.
    fn get_name(&self) -> String {
        match self.def() {
            Some(def) => format!("{} - {}", self.get_name_signature(), def.get_name()),
            None => self.get_name_signature(),
        }
    }

    /// Port of `TwbSubRecord.GetDataSize` over `TwbDataContainer.GetDataSize`.
    fn get_data_size(&self) -> i32 {
        self.do_init();
        if self.storage.is_invalid() {
            return edit::data_size_from_elements(self) + self.sr_array_size_prefix.load(Ordering::Relaxed) as i32;
        }
        match self.data_raw() {
            Some(data) => data.len() as i32,
            None => match self.value_def() {
                Some(value_def) => {
                    let self_ref = self.element_ref();
                    value_def.get_default_size(None, Some(&self_ref))
                }
                None => edit::data_size_from_elements(self),
            },
        }
    }

    fn get_element_type(&self) -> ElementType {
        ElementType::etSubRecord
    }

    fn get_def(&self) -> Option<Arc<dyn NamedDef>> {
        Some(self.def()? as Arc<dyn NamedDef>)
    }

    fn get_value_def(&self) -> Option<Arc<dyn ValueDef>> {
        self.value_def()
    }

    /// Port of `TwbSubRecord.GetDisplayName`: the signature with the name of
    /// the resolved value, or of the definition.
    fn get_display_name(&self, use_suffix: bool) -> String {
        let mut result = self.get_name_signature();
        // Upstream reads `srValueDef` as it is: a subrecord that was not
        // initialised yet has none, and asking for its display name (as
        // `ElementByName` does for every member it compares) does not
        // build its elements, which would sort a sorted array of them.
        let value_def = self.sr_value_def.read().unwrap().clone();
        if let Some(value_def) = value_def
            && !value_def.get_name().is_empty()
        {
            return format!("{result} - {}", value_def.get_name());
        }
        if let Some(def) = self.def() {
            result = format!("{result} - {}", def.get_name());
        }
        self.base.display_name(result, use_suffix)
    }

    fn get_record_signature(&self) -> Option<Signature> {
        Some(self.sr_struct.signature)
    }

    fn get_skipped(&self) -> bool {
        self.skipped()
    }

    fn get_containing_sub_record(&self) -> Option<ElementRef> {
        Some(self.element_ref())
    }

    fn get_sub_record_header_size(&self) -> Option<i32> {
        Some(self.sr_struct.data_size as i32)
    }

    /// Port of `TwbSubRecord.GetValue`: the value of the subrecord data.
    fn get_value(&self) -> String {
        let Some(value_def) = self.value_def() else {
            return String::new();
        };
        let self_ref = self.element_ref();
        value_def.to_string(self.data(), Some(&self_ref))
    }

    fn get_edit_value(&self) -> String {
        let Some(value_def) = self.value_def() else {
            return String::new();
        };
        let self_ref = self.element_ref();
        value_def.to_edit_value(self.data(), Some(&self_ref))
    }

    fn get_native_value(&self) -> Variant {
        let Some(value_def) = self.value_def() else {
            return Variant::Empty;
        };
        let self_ref = self.element_ref();
        value_def.to_native_value(self.data(), Some(&self_ref))
    }

    /// Port of `TwbSubRecord.GetCheck`: the `Check` of the value definition
    /// over the data, then the `ToStr` callback of the definition with
    /// `ctCheck`. A subrecord that is not shown has none.
    fn get_check(&self) -> String {
        let Some(def) = self.def() else {
            return String::new();
        };
        if self.get_dont_show() {
            return String::new();
        }
        self.do_init();
        let self_ref = self.element_ref();
        let mut result = match self.value_def() {
            Some(value_def) => value_def.check(self.data(), Some(&self_ref)),
            None => String::new(),
        };
        def.call_to_str(&mut result, Some(&self_ref), CallbackType::ctCheck);
        result
    }

    /// Port of `TwbSubRecord.GetSummary`.
    fn get_summary(&self) -> String {
        let Some(def) = self.def() else {
            return String::new();
        };
        self.do_init();
        let self_ref = self.element_ref();
        let mut links_to = None;
        def.to_summary(0, Some(&self_ref), &mut links_to)
    }

    /// Port of `TwbSubRecord.InternalGetLinksTo`: through the resolved value
    /// definition of a value without a name.
    fn get_links_to(&self) -> Option<ElementRef> {
        self.do_init();
        let value_def = self.sr_value_def.read().unwrap().clone()?;
        let self_ref = self.element_ref();
        value_def.get_links_to(self.data(), Some(&self_ref))
    }

    fn get_file(&self) -> Option<FileRef> {
        Some(self.file.upgrade()? as FileRef)
    }

    fn get_containing_main_record(&self) -> Option<MainRecordRef> {
        self.base.container()?.get_containing_main_record()
    }

    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }

    fn as_data_container(&self) -> Option<&dyn DataContainer> {
        Some(self)
    }
}

impl ElementImpl for SubRecordImpl {
    fn as_this(&self) -> &dyn ElementImpl {
        self
    }

    fn storage(&self) -> Option<&Storage> {
        Some(&self.storage)
    }

    fn get_data_prefix_size(&self) -> usize {
        self.sr_array_size_prefix.load(Ordering::Relaxed)
    }

    fn raw_data(&self) -> DataPtr<'_> {
        self.data_raw()
    }

    fn current_data(&self) -> DataPtr<'_> {
        self.data()
    }

    fn dont_save(&self) -> bool {
        edit::def_dont_save(self)
    }

    fn init_running(&self) -> bool {
        self.sr_init.is_running()
    }

    /// Port of `if csInit in cntStates then begin Reset; Init; end` of
    /// `SetDataSize`: inside the init of the subrecord itself (an
    /// `AfterLoad` that resizes it), the init runs again at once.
    fn reset_and_init(&self) {
        self.container.release_elements();
        if self.sr_init.is_running_here() {
            self.init();
            return;
        }
        self.sr_init.reset();
        self.do_init();
    }

    fn release_and_detach(&self) {
        self.container.release_elements();
        self.sr_init.reset();
    }

    fn commit_storage_impl(&self, bytes: Vec<u8>) {
        self.storage.set(bytes);
        self.sr_has_data.store(true, Ordering::Relaxed);
    }

    /// Port of `TwbSubRecord.SetEditValue`.
    fn set_edit_value_impl(&self, value: &str) -> Result<(), EditError> {
        edit::check_edit_allowed(self)?;
        if self.def().is_none() {
            return if value.is_empty() {
                Ok(())
            } else {
                Err(format!("{} can not be edited", self.get_name()))
            };
        }
        self.do_init();
        edit::set_edit_value(self, value, false)
    }

    /// Port of `TwbSubRecord.SetNativeValue`.
    fn set_native_value_impl(&self, value: Variant) -> Result<(), EditError> {
        edit::check_edit_allowed(self)?;
        if self.def().is_none() {
            return Err(format!("{} can not be edited", self.get_name()));
        }
        self.do_init();
        edit::set_native_value(self, value)
    }

    fn set_to_default_internal(&self) -> Result<(), EditError> {
        edit::value_set_to_default_internal(self)
    }

    /// The tail of `TwbSubRecord.SetToDefaultInternal` for an array.
    fn after_set_to_default(&self) -> Result<(), EditError> {
        if self.sr_is_array.load(Ordering::Relaxed)
            && let Some(value_def) = self.value_def()
        {
            let defaults = value_def
                .as_array_def()
                .map(|array| array.get_default_edit_values())
                .unwrap_or_default();
            for (element, default) in self.container.elements().iter().zip(&defaults) {
                element.set_edit_value(default)?;
            }
            edit::update_count_via_path(self, Some(&value_def));
        }
        Ok(())
    }

    /// Port of `TwbSubRecord.DoAfterSet`.
    fn do_after_set(&self, old: &Variant, new: &Variant) {
        edit::do_after_set(self, old, new);
        edit::update_count_via_path(self, self.value_def().as_ref());
    }

    fn do_init_sorted(&self) {
        self.sorted_init();
    }

    fn clear_sort_invalid(&self) {
        self.sr_sort_invalid.store(false, Ordering::Relaxed);
    }

    /// Port of `TwbSubRecord.ElementChanged`.
    fn element_changed(&self, _child: &ElementRef) {
        if self.sr_sorted.load(Ordering::Relaxed) {
            self.sr_sort_invalid.store(true, Ordering::Relaxed);
        }
        self.notify_changed();
    }

    /// Port of `TwbSubRecord.SetModified`.
    fn set_modified(&self, value: bool) {
        super::write::element_set_modified(self, value);
        if value && self.sr_sorted.load(Ordering::Relaxed) {
            self.sr_sort_invalid.store(true, Ordering::Relaxed);
        }
    }

    /// Port of `TwbSubRecord.NotifyChangedInternal`.
    fn notify_changed_internal(&self) {
        if self.sr_is_array.load(Ordering::Relaxed) && self.base.has_state(super::ElementState::esModified) {
            let value_def = self.value_def();
            edit::check_count(self, value_def.as_ref());
            edit::check_terminator(self, value_def.as_ref());
        }
        edit::notify_changed_internal(self);
    }

    fn assign_add(&self) -> Result<Option<ElementRef>, EditError> {
        self.array_assign_add()
    }

    fn add_impl(&self, _name: &str, _silent: bool) -> Result<Option<ElementRef>, EditError> {
        self.array_assign_add()
    }

    fn assign_internal(
        &self,
        index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        self.assign_internal_impl(index, source, only_sk)
    }

    fn can_assign_internal(&self, index: i32, source: Option<&ElementRef>, check_dont_show: bool) -> bool {
        self.can_assign_internal_impl(index, source, check_dont_show)
    }

    fn add_if_missing_internal(&self, source: &ElementRef, args: &CopyArgs) -> Result<Option<ElementRef>, EditError> {
        self.add_if_missing_internal_impl(source, args)
    }

    /// Port of `TwbSubRecord.CanContainFormIDs`.
    fn can_contain_form_ids(&self) -> bool {
        self.def()
            .is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfCanContainFormID))
    }

    /// Port of `TwbSubRecord.IsElementRemovable`: an entry of an array that
    /// may shrink.
    fn is_element_removable(&self, element: &ElementRef) -> bool {
        let Some(value_def) = self.value_def() else {
            return false;
        };
        let flags = value_def.def_base().def_flags.get();
        let mut result = self.is_element_editable(Some(element))
            && self.sr_is_array.load(Ordering::Relaxed)
            && !flags.contains(DefFlag::dfArrayStaticSize)
            && value_def.as_array_def().is_some_and(|array| array.get_count() <= 0)
            && (flags.contains(DefFlag::dfArrayCanBeEmpty) || self.container.element_count() > 1);
        if result && flags.contains(DefFlag::dfRemoveLastOnly) {
            result = self
                .container
                .elements()
                .last()
                .is_some_and(|last| Arc::ptr_eq(last, element));
        }
        result
    }

    /// Port of `TwbSubRecord.BeforeActualRemove`: the counters along the
    /// count paths of an array go to zero.
    fn before_actual_remove(&self) {
        if let Some(array_def) = self.value_def().as_ref().and_then(|def| def.as_array_def()) {
            edit::zero_count_paths(self, array_def.get_count_paths());
        }
    }

    fn add_string_list_terminator(&self) {
        let self_ref = self.element_ref();
        super::value::create_string_list_terminator(&self_ref, &self.file);
    }

    /// Port of `TwbSubRecord.GetSortKeyInternal`: the signature without a
    /// definition, else the key of the value, or the keys of the elements
    /// (`TwbContainer`'s).
    fn get_sort_key_impl(&self, extended: bool) -> String {
        if self.def().is_none() {
            return self.get_signature().to_string();
        }
        self.do_init();
        if let Some(value_def) = self.value_def() {
            let self_ref = self.element_ref();
            return value_def.to_sort_key(self.data(), Some(&self_ref), extended);
        }
        edit::container_sort_key(&self.container, extended)
    }

    /// Port of `TwbSubRecord.GetIsEditable`.
    fn get_is_editable_impl(&self) -> bool {
        if crate::interface::globals::is_internal_edit() {
            return true;
        }
        let Some(def) = self.def() else { return false };
        if def.def_base().def_internal_edit_only() {
            return false;
        }
        self.do_init();
        let self_ref = self.element_ref();
        self.value_def()
            .is_some_and(|value_def| value_def.get_is_editable(self.data(), Some(&self_ref)))
    }

    fn self_element_ref(&self) -> Option<ElementRef> {
        self.self_ref.upgrade().map(|element| element as ElementRef)
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.container)
    }

    fn sub_record_impl(&self) -> Option<&SubRecordImpl> {
        Some(self)
    }

    fn element_base(&self) -> &ElementBase {
        &self.base
    }

    fn write_to_stream(&self, out: &mut Vec<u8>, reset: super::ResetModified) -> Result<(), super::SaveError> {
        self.write_to_stream_impl(out, reset)
    }
}

impl DataContainer for SubRecordImpl {
    fn get_data(&self) -> DataPtr<'_> {
        self.data()
    }
}

macro_rules! container_by_elements {
    ($init:ident, $sorted:ident) => {
        fn as_container_ref(&self) -> Option<ElementRef> {
            self.self_element_ref()
        }

        fn add(&self, name: &str, silent: bool) -> Result<Option<ElementRef>, EditError> {
            self.add_impl(name, silent)
        }

        fn remove_element_at(&self, index: i32, mark_modified: bool) -> Option<ElementRef> {
            self.remove_child_at(index, mark_modified)
        }

        fn reverse_elements(&self) {
            self.reverse_elements_impl()
        }

        fn sort_by_sort_order(&self) {
            self.sort_by_sort_order_impl()
        }

        fn get_element_by_name(&self, name: &str) -> Option<ElementRef> {
            super::element_by_name(self, name)
        }

        fn get_element_by_path(&self, path: &str) -> Option<ElementRef> {
            super::element_by_path(self, path)
        }

        fn get_element_count(&self) -> i32 {
            self.$init();
            self.container.element_count() as i32
        }

        fn get_element(&self, index: i32) -> Option<ElementRef> {
            self.$sorted();
            self.container.element_at(usize::try_from(index).ok()?)
        }

        fn get_element_by_sort_order(&self, sort_order: i32) -> Option<ElementRef> {
            self.$sorted();
            self.container.element_by_sort_order(sort_order)
        }

        fn get_any_element(&self) -> Option<ElementRef> {
            self.get_element(0)
        }

        fn get_additional_element_count(&self) -> i32 {
            0
        }
    };
}

impl Container for SubRecordImpl {
    container_by_elements!(do_init, sorted_init);
}

impl Element for SubRecordArrayImpl {
    element_common!(element_base);
    element_display_name!(element_base);

    /// Port of `GetSignature`: the signature of the first record.
    fn get_has_signature(&self) -> Option<Signature> {
        Some(
            self.container
                .elements()
                .iter()
                .find_map(|element| element.get_record_signature())
                .unwrap_or(Signature::new(b"NONE")),
        )
    }

    /// Port of `TwbSubRecordArray.GetValue`: the `ToStr` callback of the
    /// definition.
    fn get_value(&self) -> String {
        let self_ref = self.self_ref.upgrade().map(|array| array as ElementRef);
        let mut result = String::new();
        self.arc_def
            .call_to_str(&mut result, self_ref.as_ref(), CallbackType::ctToStr);
        result
    }

    /// Port of `TwbSubRecordArray.GetCheck` (the entries are built with the
    /// array, which is upstream's `DoInit(False)`).
    fn get_check(&self) -> String {
        let self_ref = self.self_ref.upgrade().map(|array| array as ElementRef);
        super::check::record_member_check(&*self.arc_def, self_ref.as_ref())
    }

    /// Port of `TwbSubRecordArray.GetSummary`.
    fn get_summary(&self) -> String {
        let self_ref = self.self_ref.upgrade().map(|array| array as ElementRef);
        let mut links_to = None;
        self.arc_def.to_summary(0, self_ref.as_ref(), &mut links_to)
    }

    fn get_name(&self) -> String {
        self.arc_def.get_name().to_owned()
    }

    fn get_data_size(&self) -> i32 {
        self.container
            .elements()
            .iter()
            .map(|element| element.get_data_size())
            .sum()
    }

    fn get_element_type(&self) -> ElementType {
        ElementType::etSubRecordArray
    }

    fn get_def(&self) -> Option<Arc<dyn NamedDef>> {
        Some(self.arc_def.clone() as Arc<dyn NamedDef>)
    }

    fn get_file(&self) -> Option<FileRef> {
        Some(self.file.upgrade()? as FileRef)
    }

    fn get_containing_main_record(&self) -> Option<MainRecordRef> {
        self.base.container()?.get_containing_main_record()
    }

    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }
}

impl ElementImpl for SubRecordArrayImpl {
    fn as_this(&self) -> &dyn ElementImpl {
        self
    }

    fn assign_add(&self) -> Result<Option<ElementRef>, EditError> {
        self.member_assign_add()
    }

    /// Port of `TwbSubRecordArray.Add`: `Assign(StrToIntDef(aName,
    /// wbAssignAdd), nil, False)`.
    fn add_impl(&self, name: &str, _silent: bool) -> Result<Option<ElementRef>, EditError> {
        Ok(self.assign(name.trim().parse().unwrap_or(ASSIGN_ADD), None, false))
    }

    fn assign_internal(
        &self,
        index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        self.assign_internal_impl(index, source, only_sk)
    }

    fn can_assign_internal(&self, index: i32, source: Option<&ElementRef>, check_dont_show: bool) -> bool {
        self.can_assign_internal_impl(index, source, check_dont_show)
    }

    fn add_if_missing_internal(&self, source: &ElementRef, args: &CopyArgs) -> Result<Option<ElementRef>, EditError> {
        self.add_if_missing_internal_impl(source, args)
    }

    /// Port of `TwbSubRecordArray.CanContainFormIDs`.
    fn can_contain_form_ids(&self) -> bool {
        self.arc_def.def_base().def_flags.contains(DefFlag::dfCanContainFormID)
    }

    /// Port of `TwbSubRecordArray.IsElementRemovable`: an entry beyond the
    /// count the array must have.
    fn is_element_removable(&self, element: &ElementRef) -> bool {
        let count = self
            .arc_def
            .as_sub_record_array_def()
            .map_or(0, |array| array.get_count());
        let flags = self.arc_def.def_base().def_flags.get();
        let mut result = self.is_element_editable(Some(element))
            && !flags.contains(DefFlag::dfArrayStaticSize)
            && self.container.element_count() as i32 > count.max(1);
        if result && flags.contains(DefFlag::dfRemoveLastOnly) {
            result = self
                .container
                .elements()
                .last()
                .is_some_and(|last| Arc::ptr_eq(last, element));
        }
        result
    }

    /// Port of `TwbSubRecordArray.BeforeActualRemove`.
    fn before_actual_remove(&self) {
        if let Some(array_def) = self.arc_def.as_sub_record_array_def() {
            edit::zero_count_paths(self, array_def.get_count_paths());
        }
    }

    /// Port of `TwbSubRecordArray.DoAfterSet`.
    fn do_after_set(&self, old: &Variant, new: &Variant) {
        edit::do_after_set(self, old, new);
        if let Some(array_def) = self.arc_def.as_sub_record_array_def() {
            edit::update_count_via_paths(self, array_def.get_count_paths());
        }
    }

    fn self_element_ref(&self) -> Option<ElementRef> {
        self.self_ref.upgrade().map(|element| element as ElementRef)
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.container)
    }

    fn sub_record_array_impl(&self) -> Option<&SubRecordArrayImpl> {
        Some(self)
    }

    /// `TwbContainer.GetSortKeyInternal`.
    fn get_sort_key_impl(&self, extended: bool) -> String {
        self.sorted_init();
        edit::container_sort_key(&self.container, extended)
    }

    /// Port of `TwbSubRecordArray.ElementChanged`: a change of a member
    /// makes the order invalid.
    fn element_changed(&self, _child: &ElementRef) {
        self.notify_changed();
        if self.arc_sorted.load(Ordering::Relaxed) {
            self.arc_sort_invalid.store(true, Ordering::Relaxed);
        }
    }

    /// Port of `TwbSubRecordArray.SetModified`.
    fn set_modified(&self, value: bool) {
        super::write::element_set_modified(self, value);
        if value {
            // `Inc(eGeneration)`: the numbering of the names is to be made
            // again on the next request.
            self.e_generation.fetch_add(1, Ordering::Relaxed);
            if self.arc_sorted.load(Ordering::Relaxed) {
                self.arc_sort_invalid.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Port of `TwbSubRecordArray.UpdateNameSuffixes`.
    fn update_name_suffixes(&self) {
        self.update_name_suffixes_impl();
    }

    fn element_base(&self) -> &ElementBase {
        &self.base
    }
}

impl SubRecordArrayImpl {
    /// Port of `TwbSubRecordArray.GetSorted` (`IwbSortableContainer`):
    /// `arcSorted`.
    pub fn get_sorted(&self) -> bool {
        !compare_raw_data() && self.arc_sorted.load(Ordering::Relaxed)
    }

    /// Port of `TwbSubRecordArray.GetAlignable`.
    pub fn get_alignable(&self) -> bool {
        !compare_raw_data()
            && !self.get_sorted()
            && !self.arc_def.def_base().def_flags.contains(DefFlag::dfNotAlignable)
    }

    /// Port of `TwbSubRecordArray.DoInit(True)`: the members are sorted by
    /// their sort keys when a change made the order invalid.
    fn sorted_init(&self) {
        if self.arc_sorted.load(Ordering::Relaxed) {
            edit::sort_if_invalid(&self.arc_sort_invalid, &self.container);
        }
    }
}

impl Container for SubRecordArrayImpl {
    container_by_elements!(sorted_init, sorted_init);
}

impl Element for SubRecordStructImpl {
    element_common!(element_base);
    element_display_name!(element_base);

    /// Port of `GetSignature`: the signature of the first record.
    fn get_has_signature(&self) -> Option<Signature> {
        Some(
            self.container
                .elements()
                .iter()
                .find_map(|element| element.get_record_signature())
                .unwrap_or(Signature::new(b"NONE")),
        )
    }

    /// Port of `TwbSubRecordStruct.GetValue`: the `ToStr` callback of the
    /// definition.
    fn get_value(&self) -> String {
        let self_ref = self.self_ref.upgrade().map(|structure| structure as ElementRef);
        let mut result = String::new();
        self.src_def
            .call_to_str(&mut result, self_ref.as_ref(), CallbackType::ctToStr);
        result
    }

    /// Port of `TwbSubRecordStruct.GetCheck` (the members are built with the
    /// structure, which is upstream's `DoInit(False)`).
    fn get_check(&self) -> String {
        let self_ref = self.self_ref.upgrade().map(|structure| structure as ElementRef);
        super::check::record_member_check(&*self.src_def, self_ref.as_ref())
    }

    /// Port of `TwbSubRecordStruct.GetSummary`.
    fn get_summary(&self) -> String {
        let self_ref = self.self_ref.upgrade().map(|structure| structure as ElementRef);
        let mut links_to = None;
        self.src_def.to_summary(0, self_ref.as_ref(), &mut links_to)
    }

    fn get_name(&self) -> String {
        self.src_def.get_name().to_owned()
    }

    fn get_data_size(&self) -> i32 {
        self.container
            .elements()
            .iter()
            .map(|element| element.get_data_size())
            .sum()
    }

    fn get_element_type(&self) -> ElementType {
        ElementType::etSubRecordStruct
    }

    fn get_def(&self) -> Option<Arc<dyn NamedDef>> {
        Some(self.src_def.clone() as Arc<dyn NamedDef>)
    }

    fn get_file(&self) -> Option<FileRef> {
        Some(self.file.upgrade()? as FileRef)
    }

    fn get_containing_main_record(&self) -> Option<MainRecordRef> {
        self.base.container()?.get_containing_main_record()
    }

    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }
}

impl ElementImpl for SubRecordStructImpl {
    fn as_this(&self) -> &dyn ElementImpl {
        self
    }

    /// Port of `TwbSubRecordStruct.GetSortKeyInternal`: the keys of the
    /// members of the sort key of the definition, separated by `|`.
    fn get_sort_key_impl(&self, extended: bool) -> String {
        let Some(def) = self.src_def.as_sub_record_struct_def().filter(|def| def.has_sort_key()) else {
            return String::new();
        };
        let mut result = String::new();
        if def.get_sort_key_count(false) > 0 {
            let count = def.get_sort_key_count(extended).max(0) as usize;
            let additional = self.get_additional_element_count();
            for index in 0..count {
                let member = def.get_sort_key(index, extended);
                if let Some(element) = self.get_element_by_sort_order(member + additional) {
                    result.push_str(&element.get_sort_key(extended));
                }
                if index + 1 < count {
                    result.push('|');
                }
            }
        }
        result
    }

    fn add_impl(&self, name: &str, _silent: bool) -> Result<Option<ElementRef>, EditError> {
        self.add_by_name(name)
    }

    fn assign_internal(
        &self,
        index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        self.assign_internal_impl(index, source, only_sk)
    }

    fn can_assign_internal(&self, index: i32, source: Option<&ElementRef>, check_dont_show: bool) -> bool {
        self.can_assign_internal_impl(index, source, check_dont_show)
    }

    fn add_if_missing_internal(&self, source: &ElementRef, args: &CopyArgs) -> Result<Option<ElementRef>, EditError> {
        self.add_if_missing_internal_impl(source, args)
    }

    /// Port of `TwbSubRecordStruct.CanContainFormIDs`.
    fn can_contain_form_ids(&self) -> bool {
        self.src_def.def_base().def_flags.contains(DefFlag::dfCanContainFormID)
    }

    /// Port of `TwbSubRecordStruct.GetIsInSK`.
    fn get_is_in_sk(&self, sort_order: i32) -> bool {
        self.src_def
            .as_sub_record_struct_def()
            .is_some_and(|def| def.has_sort_key() && def.is_in_sk(sort_order))
    }

    /// Port of `TwbSubRecordStruct.IsElementRemovable`: a member that is not
    /// required and not the first one of an ordered structure.
    fn is_element_removable(&self, element: &ElementRef) -> bool {
        let first_fixed = !(self.src_def.as_record_def().is_some_and(|def| def.allow_unordered())
            || self
                .src_def
                .def_base()
                .def_flags
                .contains(DefFlag::dfStructFirstNotRequired));
        let is_first = self
            .container
            .elements()
            .first()
            .is_some_and(|first| Arc::ptr_eq(first, element));
        self.is_element_editable(Some(element))
            && self.container.element_count() > 1
            && !(first_fixed && is_first)
            && !element.get_def().is_some_and(|def| def.def_base().def_required())
    }

    fn self_element_ref(&self) -> Option<ElementRef> {
        self.self_ref.upgrade().map(|element| element as ElementRef)
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.container)
    }

    fn element_base(&self) -> &ElementBase {
        &self.base
    }
}

impl SubRecordStructImpl {
    fn no_init(&self) {}
}

impl Container for SubRecordStructImpl {
    container_by_elements!(no_init, no_init);
}
