// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! `TwbValueBase`, `TwbValue`, `TwbStruct`, `TwbArray` and `TwbUnion`: the
//! elements of the values inside a subrecord, built from the value
//! definitions over the data of the subrecord.
//!
//! Not ported yet: flags as array elements (`wbFlagsAsArray`) and the
//! sorting of sorted arrays.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock, Weak};

use xedit_io::compression::CompressionType;

use crate::interface::def::{EmptyDef, NamedDef, NamedDefArgs, ValueDef};
use crate::interface::element::{
    Container, CopyArgs, DataContainer, DataPtr, Element, ElementRef, FileRef, MainRecordRef,
};
use crate::interface::form_id::FormID;
use crate::interface::globals::{compare_raw_data, edit_allowed, hide_never_show, is_internal_edit, sort_sub_records};
use crate::interface::misc::{EditError, Variant};
use crate::interface::struct_def::ChapterKind;
use crate::interface::types::{
    ASSIGN_ADD, ASSIGN_THIS, ConflictPriority, DefFlag, DefType, ElementType, TriBool, dt_non_values,
};

use super::edit::{self, Storage};
use super::{ContainerBase, DataBlock, ElementBase, ElementImpl};

/// The fields of `TwbValueBase`.
pub struct ValueBase {
    pub(super) base: ElementBase,
    pub(super) container: ContainerBase,
    file: Weak<super::FileImpl>,
    block: DataBlock,
    /// The range of the data of the element in `block`. `None` for an
    /// element without data, such as an optional member that is missing.
    /// The end moves once, when `InitDataPtr` sizes the element.
    range: RwLock<Option<(usize, usize)>>,
    vb_value_def: Arc<dyn ValueDef>,
    e_name_suffix: String,
    init: super::InitOnce,
    optional_and_missing: AtomicBool,
    /// The decompressed data of a compressed structure (`dcDataStorage`),
    /// which replaces the data of the element once it is initialized.
    decompressed: OnceLock<DataBlock>,
    /// Port of `dcfDontSave` and `dcfDontCompare`: the element is not
    /// written. Set for the record header and the contained-in element.
    pub(super) dont_save: AtomicBool,
    /// Port of `dcDataStorage`: the data once the element was changed.
    pub(super) storage: Storage,
    /// Port of `arrSizePrefix`: the size of the count before the elements
    /// of an array.
    arr_size_prefix: AtomicUsize,
    /// Whether this is the `TwbRecordHeaderStruct` of a main record, whose
    /// flag edits go into the record header.
    pub(super) record_header: AtomicBool,
    /// Whether this is the `TwbContainedInElement` of a main record.
    pub(super) contained_in: AtomicBool,
}

impl ValueBase {
    fn new(
        container: &ElementRef,
        file: &Weak<super::FileImpl>,
        block: &DataBlock,
        range: Option<(usize, usize)>,
        value_def: Arc<dyn ValueDef>,
        name_suffix: &str,
    ) -> Self {
        ValueBase {
            base: ElementBase::new(Some(container)),
            container: ContainerBase::default(),
            file: file.clone(),
            block: block.clone(),
            range: RwLock::new(range),
            vb_value_def: value_def,
            e_name_suffix: name_suffix.to_owned(),
            init: super::InitOnce::new(),
            optional_and_missing: AtomicBool::new(false),
            decompressed: OnceLock::new(),
            dont_save: AtomicBool::new(false),
            storage: Storage::default(),
            arr_size_prefix: AtomicUsize::new(0),
            record_header: AtomicBool::new(false),
            contained_in: AtomicBool::new(false),
        }
    }

    pub fn range(&self) -> Option<(usize, usize)> {
        *self.range.read().unwrap()
    }

    /// The data as it is: the storage of a changed element, the
    /// decompressed data of a compressed structure, or the bytes as loaded.
    /// `None` for an element without data.
    pub fn data_raw(&self) -> DataPtr<'_> {
        if let Some(bytes) = self.storage.current() {
            return Some(bytes);
        }
        if self.storage.is_detached() {
            return None;
        }
        if let Some(block) = self.decompressed.get() {
            return Some(block.as_slice());
        }
        let (start, end) = self.range()?;
        self.block.as_slice().get(start..end)
    }

    /// The block and range the elements are built over.
    fn data_source(&self) -> Option<(DataBlock, usize, usize)> {
        if let Some(block) = self.storage.current_block() {
            let len = block.as_slice().len();
            return Some((block, 0, len));
        }
        if self.storage.is_detached() {
            return None;
        }
        if let Some(block) = self.decompressed.get() {
            let len = block.as_slice().len();
            return Some((block.clone(), 0, len));
        }
        let (start, end) = self.range()?;
        Some((self.block.clone(), start, end))
    }

    /// Port of `GetName`: the name of the definition with the suffix.
    fn name(&self) -> String {
        let name = self.vb_value_def.get_name();
        if self.e_name_suffix.is_empty() {
            name.to_owned()
        } else {
            format!("{name} {}", self.e_name_suffix)
        }
    }
}

/// The position of the next element while the children of a container are
/// built: `None` once the data is used up.
pub(crate) struct Cursor {
    pub(crate) block: DataBlock,
    pub pos: usize,
    pub end: usize,
}

impl Cursor {
    /// The data from the cursor on. An empty block is no data at all
    /// (upstream `aBasePtr = nil`), as the elements made from their
    /// definitions alone are built over.
    pub(super) fn data(&self) -> DataPtr<'_> {
        if self.block.as_slice().is_empty() {
            return None;
        }
        self.block.as_slice().get(self.pos..self.end)
    }

    fn has_data(&self) -> bool {
        self.pos < self.end
    }
}

/// Port of `Resolve`: follows the resolvable definitions to the definition
/// of the data.
pub(super) fn resolve(value_def: Arc<dyn ValueDef>, data: DataPtr, element: Option<&ElementRef>) -> Arc<dyn ValueDef> {
    let mut result = value_def;
    // Port of `BeginResolve`: an element that is resolving a definition
    // already does not resolve one that needs it, which stops the recursion
    // of a decider that looks at the element.
    let internal = element.and_then(|element| element.as_element_impl());
    let mut can_resolve = false;
    while let Some(resolvable) = result.as_resolvable_def() {
        can_resolve = can_resolve || internal.is_some_and(|internal| internal.begin_resolve());
        if resolvable.needs_element_to_resolve() && !can_resolve {
            break;
        }
        match resolvable.resolve_def(data, element) {
            Some(resolved) => result = resolved.clone(),
            None => break,
        }
    }
    if can_resolve && let Some(internal) = internal {
        internal.end_resolve();
    }
    result
}

/// Port of `TwbValue`, `TwbStruct`, `TwbArray` and `TwbUnion` in one type.
pub struct ValueImpl {
    self_ref: Weak<ValueImpl>,
    pub(super) vb: ValueBase,
    kind: ValueKind,
}

impl std::ops::Deref for ValueImpl {
    type Target = ValueBase;

    fn deref(&self) -> &ValueBase {
        &self.vb
    }
}

impl ValueImpl {
    /// The element base, for the shared element methods.
    fn vb_base(&self) -> &ElementBase {
        &self.vb.base
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Value,
    Struct,
    Array,
    Union,
    /// Port of `TwbStringListTerminator`: the zero byte that ends an array of
    /// zero-terminated strings.
    Terminator,
}

/// Port of `TwbValueBase.Create` from a pointer with `InitDataPtr`: the
/// element takes the size its definition gives, and `cursor` moves past it.
pub(super) fn create_value_element(
    container: &ElementRef,
    file: &Weak<super::FileImpl>,
    cursor: &mut Cursor,
    value_def: Arc<dyn ValueDef>,
    name_suffix: &str,
) -> Arc<ValueImpl> {
    let kind = match value_def.get_def_type() {
        DefType::dtArray => ValueKind::Array,
        DefType::dtStruct | DefType::dtStructChapter => ValueKind::Struct,
        DefType::dtUnion => {
            // Port of `wbUnionCreate`: a union of values is a value.
            let complex = value_def.as_union_def().is_some_and(|union| {
                let types = union.get_member_types();
                types.contains(DefType::dtArray)
                    || types.contains(DefType::dtStruct)
                    || types.contains(DefType::dtStructChapter)
                    || types.contains(DefType::dtUnion)
                    || value_def.def_base().def_flags.contains(DefFlag::dfMustBeUnion)
            });
            if complex { ValueKind::Union } else { ValueKind::Value }
        }
        _ => ValueKind::Value,
    };
    let start = cursor.pos;
    let mut end = cursor.end;
    // An element built over an empty buffer at its start has no data at
    // all (`aBasePtr = nil`), as a new element has.
    let no_data = cursor.block.as_slice().is_empty() && start == 0 && end == 0;
    let range = (!no_data && (cursor.has_data() || cursor.pos == cursor.end)).then_some((start, end));
    let element = Arc::new_cyclic(|self_ref: &Weak<ValueImpl>| ValueImpl {
        self_ref: self_ref.clone(),
        vb: ValueBase::new(container, file, &cursor.block, range, value_def, name_suffix),
        kind,
    });
    if let Some(parent) = container.as_element_impl().and_then(ElementImpl::container_base) {
        parent.add_element(element.clone());
    }
    // Port of `TwbValueBase.InitDataPtr`: the size is computed through the
    // element itself, which may initialize its children on the way when a
    // decider looks at them, as upstream does from the constructor.
    if range.is_some() {
        let self_ref = element.element_ref();
        let size = element.vb.vb_value_def.get_size(element.data(), Some(&self_ref));
        if (0..i32::MAX).contains(&size) {
            end = (start + size as usize).min(cursor.end);
            *element.vb.range.write().unwrap() = Some((start, end));
        }
        cursor.pos = end;
    }
    element
}

impl ValueImpl {
    fn element_ref(&self) -> ElementRef {
        self.self_ref.upgrade().expect("a value is alive while it is used")
    }

    /// Port of `GetDataBasePtr` up to `GetDataEndPtr`: the data, rebuilt
    /// from the elements first when a child changed.
    pub fn data(&self) -> DataPtr<'_> {
        if self.vb.storage.is_invalid() {
            edit::update_storage_from_elements(self);
        }
        self.vb.data_raw()
    }

    /// Port of `TwbValueBase.Create(aContainer, aValueDef, nil, aOnlySK,
    /// aNameSuffix)`: an element without data, which takes storage of its
    /// default size and the default value.
    pub(super) fn create_new(
        container: &ElementRef,
        file: &Weak<super::FileImpl>,
        value_def: Arc<dyn ValueDef>,
        name_suffix: &str,
    ) -> Result<Arc<ValueImpl>, EditError> {
        let block = DataBlock::Buffer(Arc::new(Vec::new()));
        let mut cursor = Cursor { block, pos: 0, end: 0 };
        let element = create_value_element(container, file, &mut cursor, value_def, name_suffix);
        element.vb.storage.detach();
        element.set_modified(true);
        let size = usize::try_from(element.get_data_size()).unwrap_or(0);
        if let Some(bytes) = edit::request_storage_change(&*element, size) {
            element.vb.storage.set(bytes);
        }
        edit::set_to_default(&*element)?;
        Ok(element)
    }

    /// Port of `TwbValueBase.Create(aContainer, aValueDef, aSource, aOnlySK,
    /// aNameSuffix)` with a source: an element without data takes storage
    /// of its default size, its default value and then the value of the
    /// source. Without a source it is `create_new`.
    pub(super) fn create_from(
        container: &ElementRef,
        file: &Weak<super::FileImpl>,
        value_def: Arc<dyn ValueDef>,
        source: Option<&ElementRef>,
        only_sk: bool,
        name_suffix: &str,
    ) -> Result<Arc<ValueImpl>, EditError> {
        let Some(source) = source else {
            return ValueImpl::create_new(container, file, value_def, name_suffix);
        };
        let block = DataBlock::Buffer(Arc::new(Vec::new()));
        let mut cursor = Cursor { block, pos: 0, end: 0 };
        let element = create_value_element(container, file, &mut cursor, value_def, name_suffix);
        element.vb.storage.detach();
        let result = (|| {
            let size = usize::try_from(element.get_data_size()).unwrap_or(0);
            if let Some(bytes) = edit::request_storage_change(&*element, size) {
                element.vb.storage.set(bytes);
            }
            edit::set_to_default(&*element)?;
            element.assign(ASSIGN_THIS, Some(source), only_sk);
            element.set_modified(true);
            Ok(())
        })();
        if let Err(error) = result {
            let element_ref: ElementRef = element.clone();
            if let Some(container) = container.as_element_impl() {
                container.remove_child(&element_ref, false);
            }
            return Err(error);
        }
        Ok(element)
    }

    /// The element definition of an array as `AssignInternal` and
    /// `AddIfMissingInternal` create an entry with, resolved without data.
    pub(super) fn array_entry_def(
        array_def: &Arc<dyn ValueDef>,
        source: Option<&ElementRef>,
    ) -> Option<Arc<dyn ValueDef>> {
        let array = array_def.as_array_def()?;
        let mut element_def = array.get_element().clone();
        if element_def.get_def_type() == DefType::dtResolvable {
            element_def = resolve(element_def, None, source);
        }
        if element_def.def_base().def_flags.contains(DefFlag::dfUnionStaticResolve) {
            element_def = resolve(element_def, None, source);
        }
        Some(element_def)
    }

    /// Port of `TwbArray.AssignInternal`.
    fn array_assign_internal(
        &self,
        index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && !edit_allowed() {
            return Err(format!("{} can not be assigned.", self.get_name()));
        }
        // An entry added without a source is `Add`, which `assign_add` ports.
        if index == ASSIGN_ADD && source.is_none() {
            return self.assign_add();
        }
        self.do_init();
        let value_def = self.vb.vb_value_def.clone();
        let Some(array_def) = value_def.as_array_def() else {
            return Ok(None);
        };
        let self_ref = self.element_ref();
        let source_def = source.and_then(|source| source.get_value_def());
        let source_def_ref = source_def.as_deref().map(|def| def.as_dyn_def());
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
            if value_def.get_is_variable_size() {
                self.set_modified(true);
                self.invalidate_storage();
                self.vb.container.release_elements();
                self.vb.storage.set(Vec::new());
                self.vb.storage.set_invalid(false);
                if array_def.get_count() < 0 {
                    let size = usize::try_from(source.get_data_size()).unwrap_or(0);
                    let size = if size > 0 {
                        size
                    } else {
                        usize::try_from(array_def.get_prefix_size(None)).unwrap_or(0)
                    };
                    if let Some(mut bytes) = self.request_storage_change_impl(size) {
                        if source.get_data_size() > 0
                            && let Some(data) = source.as_data_container().and_then(|data| data.get_data())
                        {
                            let len = size.min(data.len());
                            bytes[..len].copy_from_slice(&data[..len]);
                        }
                        self.commit_storage_impl(bytes);
                    }
                }
                self.notify_changed();
                if let Some(source_container) = source_container {
                    for i in 0..source_count {
                        if let Some(source_child) = source_container.get_element(i) {
                            self.assign(i, Some(&source_child), only_sk);
                        }
                    }
                }
            } else if let Some(source_container) = source_container {
                for i in 0..source_count {
                    let (Some(source_child), Some(target)) =
                        (source_container.get_element(i), self.get_element_by_memory_order(i))
                    else {
                        continue;
                    };
                    target.assign(ASSIGN_THIS, Some(&source_child), only_sk);
                }
            }
        } else if index >= 0
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
                format!("#{}", self.vb.container.element_count())
            };
            let is_terminator =
                source.is_some_and(|source| source.get_element_type() == ElementType::etStringListTerminator);
            if !is_terminator && let Some(element_def) = ValueImpl::array_entry_def(&value_def, source) {
                let element = ValueImpl::create_from(&self_ref, &self.vb.file, element_def, source, only_sk, &suffix)?;
                result = Some(element as ElementRef);
            }
        }
        edit::check_count(self, Some(&value_def));
        edit::check_terminator(self, Some(&value_def));
        Ok(result)
    }

    /// Port of `TwbArray.CanAssignInternal`.
    fn array_can_assign_internal(&self, index: i32, source: Option<&ElementRef>, check_dont_show: bool) -> bool {
        if !is_internal_edit()
            && (!edit_allowed()
                || self
                    .vb
                    .vb_value_def
                    .def_base()
                    .def_flags
                    .contains(DefFlag::dfInternalEditOnly))
        {
            return false;
        }
        if !super::assign::parent_allows_edit(self) {
            return false;
        }
        if check_dont_show && self.get_dont_show() {
            return false;
        }
        let Some(array_def) = self.vb.vb_value_def.as_array_def() else {
            return false;
        };
        let Some(source) = source else {
            return index == ASSIGN_ADD && array_def.get_count() <= 0;
        };
        let self_ref = self.element_ref();
        let source_def = source.get_value_def();
        let source_def = source_def.as_deref().map(|def| def.as_dyn_def());
        self.vb.vb_value_def.can_assign(Some(&self_ref), index, source_def)
            || (array_def.get_count() <= 0
                && array_def
                    .get_element()
                    .can_assign(Some(&self_ref), ASSIGN_THIS, source_def))
    }

    /// Port of `TwbArray.AddIfMissingInternal`: a copy of the source as a
    /// new entry, or the entry with the same sort key of a sorted array.
    fn array_add_if_missing_internal(
        &self,
        source: &ElementRef,
        args: &CopyArgs,
    ) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && !edit_allowed() {
            return Err(format!("{} can not be modified.", self.get_name()));
        }
        self.do_init();
        let value_def = self.vb.vb_value_def.clone();
        let sorted = sort_sub_records() && value_def.as_array_def().is_some_and(|array| array.get_sorted());
        if sorted && let Some(found) = find_by_sort_key(&self.vb.container, source) {
            if args.deep_copy {
                found.assign(ASSIGN_THIS, Some(source), false);
            }
            return Ok(Some(found));
        }
        let suffix = if sorted {
            String::new()
        } else {
            format!("#{}", self.vb.container.element_count())
        };
        let self_ref = self.element_ref();
        let mut result = None;
        if source.get_element_type() != ElementType::etStringListTerminator
            && let Some(element_def) = ValueImpl::array_entry_def(&value_def, Some(source))
        {
            let element = ValueImpl::create_from(
                &self_ref,
                &self.vb.file,
                element_def,
                Some(source),
                !args.deep_copy,
                &suffix,
            )?;
            result = Some(element as ElementRef);
        }
        edit::check_count(self, Some(&value_def));
        edit::check_terminator(self, Some(&value_def));
        Ok(result)
    }

    /// Port of `TwbRecordHeaderStruct.AddIfMissingInternal`: the member of
    /// the record header takes the value of the source.
    fn record_header_add_if_missing_internal(
        &self,
        source: &ElementRef,
        args: &CopyArgs,
    ) -> Result<Option<ElementRef>, EditError> {
        if !is_internal_edit() && !edit_allowed() {
            return Err(format!("{} can not be assigned.", self.get_name()));
        }
        self.do_init();
        if self.vb.vb_value_def.as_struct_def().is_none() {
            return Ok(None);
        }
        let result = self.vb.container.element_by_sort_order(source.get_sort_order());
        if let Some(result) = &result {
            result.assign(ASSIGN_THIS, Some(source), !args.deep_copy);
        }
        Ok(result)
    }

    /// Makes `bytes` the data of the element and builds the elements again
    /// over it (`InformStorage` on the record header, a `Reset; Init`).
    pub(super) fn replace_data(&self, bytes: Vec<u8>) {
        self.vb.storage.set(bytes);
        self.vb.storage.set_invalid(false);
        self.reset_and_init();
    }

    /// Port of `TwbArray.AssignInternal(wbAssignAdd, nil)` and of the same
    /// branch of `TwbSubRecord.AssignInternal`: one element added to an
    /// array of variable size from its definition.
    pub(super) fn array_assign_add(
        container: &ElementRef,
        container_base: &ContainerBase,
        file: &Weak<super::FileImpl>,
        array_def: &Arc<dyn ValueDef>,
        sorted: bool,
    ) -> Result<Option<Arc<ValueImpl>>, EditError> {
        let Some(array) = array_def.as_array_def() else {
            return Ok(None);
        };
        if array.get_count() > 0 {
            return Ok(None);
        }
        let suffix = if sorted {
            String::new()
        } else {
            format!("#{}", container_base.element_count())
        };
        let mut element_def = array.get_element().clone();
        if element_def.get_def_type() == DefType::dtResolvable
            || element_def.def_base().def_flags.contains(DefFlag::dfUnionStaticResolve)
        {
            element_def = resolve(element_def, None, Some(container));
        }
        Ok(Some(ValueImpl::create_new(container, file, element_def, &suffix)?))
    }

    /// Port of `DoInit`: builds the children once.
    pub fn do_init(&self) {
        self.vb.init.run(|| {
            let Some((block, start, end)) = self.vb.data_source() else {
                return;
            };
            let self_ref = self.element_ref();
            let mut cursor = Cursor { block, pos: start, end };
            if self.kind == ValueKind::Struct
                && let Some(block) = self.decompress_if_needed(&self_ref)
            {
                cursor = Cursor {
                    pos: 0,
                    end: block.as_slice().len(),
                    block,
                };
            }
            match self.kind {
                ValueKind::Value => {}
                ValueKind::Struct => struct_do_init(&self.vb.vb_value_def, &self_ref, &self.vb.file, &mut cursor),
                ValueKind::Array => {
                    let (_, prefix) = array_do_init(&self.vb.vb_value_def, &self_ref, &self.vb.file, &mut cursor);
                    self.vb.arr_size_prefix.store(prefix, Ordering::Relaxed);
                }
                ValueKind::Union => {
                    union_do_init(&self.vb.vb_value_def, &self_ref, &self.vb.file, &mut cursor);
                }
                ValueKind::Terminator => {}
            }
            // `StructDoInit`, `ArrayDoInit` and `UnionDoInit` end with the
            // `AfterLoad` of the definition.
            if matches!(self.kind, ValueKind::Struct | ValueKind::Array | ValueKind::Union) {
                self.vb.vb_value_def.after_load(&self_ref);
            }
        });
    }

    pub fn value_def(&self) -> &Arc<dyn ValueDef> {
        &self.vb.vb_value_def
    }

    /// Port of `TwbArray.GetSorted` and `TwbValue.GetSorted`
    /// (`IwbSortableContainer`): `arrSorted` of an array (the definition
    /// sorts it, under `wbSortSubRecords`); a value is sorted when it holds
    /// flags (`vIsFlags`) or its definition is a sorted `wbEmpty`. `None`
    /// for the structures and unions, which are not sortable containers.
    pub fn get_sorted(&self) -> Option<bool> {
        match self.kind {
            ValueKind::Array => Some(
                !compare_raw_data()
                    && sort_sub_records()
                    && self
                        .vb
                        .vb_value_def
                        .as_array_def()
                        .is_some_and(|array| array.get_sorted()),
            ),
            ValueKind::Value => {
                if compare_raw_data() {
                    return Some(false);
                }
                let self_ref = self.element_ref();
                let is_flags = super::flag::is_flags(&self_ref);
                Some(
                    is_flags
                        || resolve(self.vb.vb_value_def.clone(), self.data(), Some(&self_ref))
                            .as_empty_def()
                            .is_some_and(|empty| empty.get_sorted()),
                )
            }
            ValueKind::Struct | ValueKind::Union | ValueKind::Terminator => None,
        }
    }

    /// Port of `TwbArray.GetAlignable` and `TwbValue.GetAlignable`: an
    /// unsorted array of a variable count whose definition does not forbid
    /// it (`dfNotAlignable`).
    pub fn get_alignable(&self) -> bool {
        match self.kind {
            ValueKind::Array => {
                if compare_raw_data() || self.get_sorted() == Some(true) {
                    return false;
                }
                let def = &self.vb.vb_value_def;
                if def.def_base().def_flags.contains(DefFlag::dfNotAlignable) {
                    return false;
                }
                def.as_array_def().is_some_and(|array| array.get_count() <= 0)
            }
            _ => false,
        }
    }

    /// Port of `esOptionalAndMissing`: an optional member of a structure
    /// that the data ended before.
    pub fn is_optional_and_missing(&self) -> bool {
        self.vb.optional_and_missing.load(Ordering::Relaxed)
    }

    /// Port of `TwbStruct.DecompressIfNeeded` and `GetIsCompressed`: the
    /// decompressed data of a `TwbStructZDef` or `TwbStructLZDef` whose size
    /// callback gives an uncompressed size. Upstream passes the first four
    /// bytes of the compressed data as the size of the output, and the
    /// decompression fails unless the output has exactly that size; the
    /// structure then has no data.
    fn decompress_if_needed(&self, self_ref: &ElementRef) -> Option<DataBlock> {
        let struct_def = self.vb.vb_value_def.as_struct_def()?;
        let compression = match struct_def.chapter()?.kind {
            ChapterKind::Chapter => return None,
            ChapterKind::ZLib => CompressionType::ZLib,
            ChapterKind::Lz4 => CompressionType::LZ4,
        };
        let mut compressed_size = 0;
        let uncompressed_size = struct_def.get_sizing(self.data(), Some(self_ref), &mut compressed_size);
        if uncompressed_size == 0 {
            return None;
        }
        let data = self.data().unwrap_or_default();
        let output_size = data
            .get(..4)
            .map_or(0, |bytes| u32::from_le_bytes(bytes.try_into().unwrap()));
        let mut output = vec![0u8; uncompressed_size as usize];
        let decompressed = output_size == uncompressed_size && compression.decompress(data, &mut output).is_ok();
        if !decompressed {
            output.clear();
        }
        let block = DataBlock::Buffer(Arc::new(output));
        Some(self.vb.decompressed.get_or_init(|| block).clone())
    }

    /// The union step of `TwbContainer.AssignInternal`: the element the
    /// union was decided with goes, and the union is decided again without
    /// data (`UnionDoInit` with `nil`), from the elements assigned before
    /// it.
    /// UPSTREAM-QUIRK: upstream builds the new element over no data, and its
    /// members take storage as they are assigned; here the element takes
    /// storage of its default size with its default value at once
    /// (`create_new`), so that its members exist to take the values.
    pub(super) fn union_reinit_without_data(&self) {
        if self.kind != ValueKind::Union {
            return;
        }
        self.do_init();
        if self.vb.container.element_count() == 1 {
            self.vb.container.remove_element(0);
        }
        if self.vb.container.element_count() != 0 {
            return;
        }
        let self_ref = self.element_ref();
        let Some(resolvable) = self.vb.vb_value_def.as_resolvable_def() else {
            return;
        };
        let Some(mut resolved) = resolvable.resolve_def(None, Some(&self_ref)).cloned() else {
            return;
        };
        if resolved.get_def_type() == DefType::dtResolvable
            || resolved.def_base().def_flags.contains(DefFlag::dfUnionStaticResolve)
        {
            resolved = resolve(resolved, None, Some(&self_ref));
        }
        if matches!(
            resolved.get_def_type(),
            DefType::dtArray | DefType::dtStruct | DefType::dtStructChapter | DefType::dtUnion
        ) {
            match ValueImpl::create_new(&self_ref, &self.vb.file, resolved, "") {
                Ok(element) => element.set_sort_and_memory_order(0),
                Err(error) => crate::interface::misc::progress(&error),
            }
        }
    }

    /// Port of `TwbValueBase.GetValue`.
    pub fn get_value(&self) -> String {
        self.do_init();
        let self_ref = self.element_ref();
        self.vb.vb_value_def.to_string(self.data(), Some(&self_ref))
    }
}

/// Port of `FindBySortKey` on the elements of a sorted container: the
/// element with the sort key of `source`.
/// UPSTREAM-QUIRK: upstream searches the sorted elements by binary search;
/// a linear search finds the same element while the keys are unique.
pub(super) fn find_by_sort_key(container: &ContainerBase, source: &ElementRef) -> Option<ElementRef> {
    let key = source.get_sort_key(false);
    container
        .elements()
        .into_iter()
        .find(|element| element.get_sort_key(false) == key)
}

/// Port of `TwbContainedInElement.Create`: a value over the label of the
/// group that holds the record, with the definition of the group type.
pub(super) fn create_contained_in_element(
    container: &ElementRef,
    file: &Weak<super::FileImpl>,
    value_def: Arc<dyn ValueDef>,
    label: u32,
) -> Arc<ValueImpl> {
    let block = DataBlock::Buffer(Arc::new(label.to_le_bytes().to_vec()));
    let element = Arc::new_cyclic(|self_ref: &Weak<ValueImpl>| ValueImpl {
        self_ref: self_ref.clone(),
        vb: ValueBase::new(container, file, &block, Some((0, 4)), value_def, ""),
        kind: ValueKind::Value,
    });
    element.set_sort_and_memory_order(-2);
    // `TwbContainedInElement.Create` passes `aDontCompare`.
    element.vb.dont_save.store(true, Ordering::Relaxed);
    element.vb.contained_in.store(true, Ordering::Relaxed);
    element
}

/// Port of `StructDoInit`: one child per member of the structure.
pub(super) fn struct_do_init(
    value_def: &Arc<dyn ValueDef>,
    container: &ElementRef,
    file: &Weak<super::FileImpl>,
    cursor: &mut Cursor,
) {
    let Some(struct_def) = value_def.as_struct_def() else {
        return;
    };
    let mut optional_from = struct_def.get_optional_from_element();
    if optional_from < 0 {
        optional_from = i32::MAX;
    }
    let member_count = usize::try_from(struct_def.get_member_count()).unwrap_or(0);
    for i in 0..member_count {
        let mut member = struct_def.get_member(i).clone();
        if member.get_def_type() == DefType::dtResolvable
            || member.def_base().def_flags.contains(DefFlag::dfUnionStaticResolve)
        {
            member = resolve(member, None, Some(container));
        }
        let mut over = false;
        if i as i32 >= optional_from {
            over = cursor.pos >= cursor.end;
            if !over {
                let size = member.get_size(cursor.data(), Some(container));
                over = size < i32::MAX && cursor.pos + size as usize > cursor.end;
            }
            if over {
                cursor.end = cursor.pos;
                member = resolve(member, cursor.data(), Some(container));
                let is_flags = member
                    .as_integer_def()
                    .and_then(|def| def.get_formater(Some(container)))
                    .is_some_and(|formater| formater.as_flags_def().is_some());
                // Port of `wbEmpty` for the missing optional member.
                member = EmptyDef::create(
                    NamedDefArgs {
                        priority: member.get_conflict_priority(None),
                        required: false,
                        name: member.get_name().to_owned(),
                        after_load: None,
                        after_set: None,
                        dont_show: None,
                        get_cp: None,
                        terminator: false,
                    },
                    is_flags,
                );
            }
        }
        let element = create_value_element(container, file, cursor, member, "");
        if over {
            element.vb.optional_and_missing.store(true, Ordering::Relaxed);
        }
        element.set_sort_and_memory_order(i as i32);
    }
}

/// Port of `ArrayDoInit`: the elements of an array, as many as the count of
/// the definition, the prefix, the callback or the data allow.
pub(super) fn array_do_init(
    value_def: &Arc<dyn ValueDef>,
    container: &ElementRef,
    file: &Weak<super::FileImpl>,
    cursor: &mut Cursor,
) -> (bool, usize) {
    let Some(array_def) = value_def.as_array_def() else {
        return (false, 0);
    };
    let sorted = sort_sub_records() && array_def.get_sorted();
    let size_prefix = array_def.get_prefix_size(cursor.data()).max(0) as usize;
    let mut element_def = array_def.get_element().clone();
    if element_def.get_def_type() == DefType::dtResolvable
        || element_def.def_base().def_flags.contains(DefFlag::dfUnionStaticResolve)
    {
        element_def = resolve(element_def, None, Some(container));
    }
    let mut var_size = array_def.get_is_variable_size();
    let mut arr_size = i64::from(array_def.get_count());
    // Port of the `not Assigned(aBasePtr)` cases: an array built without
    // data (a new element) gets the elements of its default edit values, or
    // one element for a variable size array without a count prefix.
    let no_data = cursor.data().is_none();
    let default_edit_values = if no_data {
        array_def.get_default_edit_values()
    } else {
        Vec::new()
    };
    if no_data && !default_edit_values.is_empty() {
        arr_size = arr_size.max(default_edit_values.len() as i64);
    }
    if arr_size < 0 {
        arr_size = i64::from(array_def.get_prefix_count(cursor.data()));
    } else if arr_size < 1
        && let Some(callback) = array_def.get_count_callback()
    {
        arr_size = i64::from(callback(cursor.data(), Some(container)));
    } else if var_size {
        if arr_size > 0 && no_data {
            var_size = false;
        } else if !no_data {
            if arr_size < 1 {
                arr_size = i64::from(i32::MAX);
            }
        } else if size_prefix == 0 {
            arr_size = 1;
        }
    }
    cursor.pos = (cursor.pos + size_prefix).min(cursor.end);
    let wrongly_assumed = array_def.get_wrongly_assumed_fixed_size_per_element();
    let mut final_pos = None;
    if wrongly_assumed > 0 {
        arr_size = ((cursor.end - cursor.pos) / wrongly_assumed as usize) as i64;
        final_pos = Some(cursor.pos + wrongly_assumed as usize * arr_size as usize);
    }
    let element_can_be_zero_size = || element_def.get_default_size(None, None) == 0;
    let mut i = 0;
    if arr_size > 0 {
        while !var_size
            || no_data
            || cursor.has_data()
            || (arr_size < i64::from(i32::MAX) && cursor.pos == cursor.end && element_can_be_zero_size())
        {
            let suffix = if sorted {
                String::new()
            } else {
                array_def.get_element_name_suffix(i)
            };
            if !array_def.should_include(cursor.data(), Some(container)) {
                break;
            }
            if element_def.get_def_type() == DefType::dtString
                && element_def.get_is_variable_size()
                && cursor.data().is_some_and(|data| data.first() == Some(&0))
            {
                cursor.pos += 1;
                break;
            }
            let element = create_value_element(container, file, cursor, element_def.clone(), &suffix);
            element.vb.base.e_memory_order.store(i, Ordering::Relaxed);
            if let Some(default) = default_edit_values.get(i as usize) {
                let _ = element.set_edit_value(default);
            }
            i += 1;
            if var_size && no_data {
                // Port of `CreatedEmpty`: the one element of a new array.
                if let Some(base) = container.as_element_impl().and_then(ElementImpl::container_base) {
                    base.cnt_as_created_empty.store(true, Ordering::Relaxed);
                }
                break;
            }
            if arr_size < i64::from(i32::MAX) {
                arr_size -= 1;
            }
            if arr_size == 0 {
                break;
            }
        }
    }
    if let Some(final_pos) = final_pos
        && cursor.pos < final_pos
    {
        cursor.pos = final_pos;
    }
    if element_def.get_def_type() == DefType::dtString && element_def.get_is_variable_size() {
        let element = create_string_list_terminator(container, file);
        element.vb.base.e_memory_order.store(i, Ordering::Relaxed);
    }
    (sorted, size_prefix)
}

/// Port of `TwbStringListTerminator.Create`: the `Terminator` element after
/// the strings of an array of zero-terminated strings. It has no data of its
/// own; its size is the one byte of the terminator.
pub(super) fn create_string_list_terminator(container: &ElementRef, file: &Weak<super::FileImpl>) -> Arc<ValueImpl> {
    let def = EmptyDef::create(
        NamedDefArgs {
            priority: ConflictPriority::cpIgnore,
            required: false,
            name: "Terminator".to_owned(),
            after_load: None,
            after_set: None,
            // Port of `TwbStringListTerminator.GetDontShow`.
            dont_show: Some(Arc::new(|_| hide_never_show())),
            get_cp: None,
            terminator: false,
        },
        false,
    );
    let block = DataBlock::Buffer(Arc::new(Vec::new()));
    let element = Arc::new_cyclic(|self_ref: &Weak<ValueImpl>| ValueImpl {
        self_ref: self_ref.clone(),
        vb: ValueBase::new(container, file, &block, None, def, ""),
        kind: ValueKind::Terminator,
    });
    if let Some(parent) = container.as_element_impl().and_then(ElementImpl::container_base) {
        parent.add_element(element.clone());
    }
    element
}

/// Port of `UnionDoInit`: the one child the decider selects, or none for a
/// union of values.
pub(super) fn union_do_init(
    value_def: &Arc<dyn ValueDef>,
    container: &ElementRef,
    file: &Weak<super::FileImpl>,
    cursor: &mut Cursor,
) -> Option<Arc<dyn ValueDef>> {
    let resolvable = value_def.as_resolvable_def()?;
    let mut resolved = resolvable.resolve_def(cursor.data(), Some(container))?.clone();
    if resolved.get_def_type() == DefType::dtResolvable
        || resolved.def_base().def_flags.contains(DefFlag::dfUnionStaticResolve)
    {
        resolved = resolve(resolved, cursor.data(), Some(container));
    }
    match resolved.get_def_type() {
        DefType::dtArray | DefType::dtStruct | DefType::dtStructChapter | DefType::dtUnion => {
            let element = create_value_element(container, file, cursor, resolved.clone(), "");
            element.set_sort_and_memory_order(0);
        }
        _ => {}
    }
    Some(resolved)
}

// ----- the element traits -----

impl Element for ValueImpl {
    element_common!(vb_base, own_values);

    fn get_name(&self) -> String {
        self.vb.name()
    }

    /// Port of `TwbValueBase.GetBaseName`: the name of the definition.
    fn get_base_name(&self) -> String {
        self.vb.vb_value_def.get_name().to_owned()
    }

    /// Port of `TwbValueBase.GetDisplayName` without the dump offsets: the
    /// name of the resolved definition, unless that is a container type
    /// different from the definition of the element.
    fn get_display_name(&self, use_suffix: bool) -> String {
        let self_ref = self.element_ref();
        let resolved = resolve(self.vb.vb_value_def.clone(), self.data(), Some(&self_ref));
        let same = std::ptr::addr_eq(Arc::as_ptr(&resolved), Arc::as_ptr(&self.vb.vb_value_def));
        let mut result = if !same && dt_non_values().contains(resolved.get_def_type()) {
            self.vb.vb_value_def.get_name().to_owned()
        } else {
            resolved.get_name().to_owned()
        };
        if use_suffix && !self.vb.e_name_suffix.is_empty() {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(&self.vb.e_name_suffix);
        }
        result
    }

    /// Port of `TwbValueBase.GetDataSize` over `TwbDataContainer.GetDataSize`:
    /// the size of the elements and the prefix while the storage is stale,
    /// the default size of the definition for an element without data,
    /// else the size of the data.
    fn get_data_size(&self) -> i32 {
        if self.kind == ValueKind::Terminator {
            return 1;
        }
        if self.vb.storage.is_invalid() {
            return edit::data_size_from_elements(self) + self.vb.arr_size_prefix.load(Ordering::Relaxed) as i32;
        }
        match self.vb.data_raw() {
            Some(data) if self.vb.decompressed.get().is_none() || self.vb.storage.has_storage() => data.len() as i32,
            Some(_) => self.vb.range().map_or(0, |(start, end)| (end - start) as i32),
            None => {
                let self_ref = self.element_ref();
                self.vb.vb_value_def.get_default_size(None, Some(&self_ref))
            }
        }
    }

    fn get_element_type(&self) -> ElementType {
        match self.kind {
            ValueKind::Value => ElementType::etValue,
            ValueKind::Struct if self.vb.vb_value_def.get_def_type() == DefType::dtStructChapter => {
                ElementType::etStructChapter
            }
            ValueKind::Struct => ElementType::etStruct,
            ValueKind::Array => ElementType::etArray,
            ValueKind::Union => ElementType::etUnion,
            ValueKind::Terminator => ElementType::etStringListTerminator,
        }
    }

    fn get_def(&self) -> Option<Arc<dyn NamedDef>> {
        Some(self.vb.vb_value_def.clone() as Arc<dyn NamedDef>)
    }

    fn get_value_def(&self) -> Option<Arc<dyn ValueDef>> {
        Some(self.vb.vb_value_def.clone())
    }

    fn get_value(&self) -> String {
        ValueImpl::get_value(self)
    }

    fn get_edit_value(&self) -> String {
        self.do_init();
        let self_ref = self.element_ref();
        self.vb.vb_value_def.to_edit_value(self.data(), Some(&self_ref))
    }

    fn get_native_value(&self) -> Variant {
        self.do_init();
        let self_ref = self.element_ref();
        self.vb.vb_value_def.to_native_value(self.data(), Some(&self_ref))
    }

    /// Port of `TwbValueBase.GetSummary`.
    fn get_summary(&self) -> String {
        self.do_init();
        let self_ref = self.element_ref();
        let mut links_to = None;
        self.vb
            .vb_value_def
            .to_summary(0, self.data(), Some(&self_ref), &mut links_to)
    }

    /// Port of `TwbValueBase.InternalGetLinksTo`.
    fn get_links_to(&self) -> Option<ElementRef> {
        self.do_init();
        let self_ref = self.element_ref();
        self.vb.vb_value_def.get_links_to(self.data(), Some(&self_ref))
    }

    fn get_file(&self) -> Option<FileRef> {
        Some(self.vb.file.upgrade()? as FileRef)
    }

    fn get_containing_main_record(&self) -> Option<MainRecordRef> {
        self.vb.base.container()?.get_containing_main_record()
    }

    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }

    fn as_data_container(&self) -> Option<&dyn DataContainer> {
        Some(self)
    }
}

impl ElementImpl for ValueImpl {
    fn as_this(&self) -> &dyn ElementImpl {
        self
    }

    fn self_element_ref(&self) -> Option<ElementRef> {
        self.self_ref.upgrade().map(|element| element as ElementRef)
    }

    fn element_base(&self) -> &ElementBase {
        &self.vb.base
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.vb.container)
    }

    fn value_impl(&self) -> Option<Arc<ValueImpl>> {
        self.self_ref.upgrade()
    }

    fn storage(&self) -> Option<&Storage> {
        Some(&self.vb.storage)
    }

    fn get_data_prefix_size(&self) -> usize {
        self.vb.arr_size_prefix.load(Ordering::Relaxed)
    }

    fn raw_data(&self) -> DataPtr<'_> {
        self.vb.data_raw()
    }

    fn current_data(&self) -> DataPtr<'_> {
        self.data()
    }

    fn dont_save(&self) -> bool {
        self.vb.dont_save.load(Ordering::Relaxed) || edit::def_dont_save(self)
    }

    fn init_running(&self) -> bool {
        self.vb.init.is_running()
    }

    fn reset_and_init(&self) {
        self.vb.container.release_elements();
        self.vb.init.reset();
        self.do_init();
    }

    fn release_and_detach(&self) {
        self.vb.container.release_elements();
        self.vb.init.reset();
    }

    fn set_edit_value_impl(&self, value: &str) -> Result<(), EditError> {
        if self.kind == ValueKind::Terminator {
            return Err(format!("{} can not be edited.", self.get_name()));
        }
        edit::set_edit_value(self, value, self.kind == ValueKind::Value)
    }

    fn set_native_value_impl(&self, value: Variant) -> Result<(), EditError> {
        if self.kind == ValueKind::Terminator {
            return Err(format!("{} can not be edited.", self.get_name()));
        }
        edit::set_native_value(self, value)
    }

    fn set_to_default_internal(&self) -> Result<(), EditError> {
        if self.kind == ValueKind::Terminator {
            return Ok(());
        }
        edit::value_set_to_default_internal(self)
    }

    /// Port of `TwbArray.DoAfterSet`: the counters along the count paths
    /// follow the element count.
    fn do_after_set(&self, old: &Variant, new: &Variant) {
        edit::do_after_set(self, old, new);
        if self.kind == ValueKind::Array {
            edit::update_count_via_path(self, Some(&self.vb.vb_value_def));
        }
    }

    /// Port of `TwbArray.NotifyChangedInternal` and of
    /// `TwbRecordHeaderStruct.ElementChanged` through the main record.
    fn notify_changed_internal(&self) {
        if self.kind == ValueKind::Array && self.vb.base.has_state(super::ElementState::esModified) {
            edit::check_count(self, Some(&self.vb.vb_value_def));
            edit::check_terminator(self, Some(&self.vb.vb_value_def));
        }
        edit::notify_changed_internal(self);
    }

    fn element_changed(&self, child: &ElementRef) {
        if self.vb.record_header.load(Ordering::Relaxed)
            && let Some(record) = self
                .vb
                .base
                .container()
                .and_then(|c| c.as_element_impl()?.main_record_impl())
        {
            record.record_header_changed(child);
        }
        self.notify_changed();
    }

    fn after_set_to_default(&self) -> Result<(), EditError> {
        if self.kind == ValueKind::Array {
            let defaults = self
                .vb
                .vb_value_def
                .as_array_def()
                .map(|array| array.get_default_edit_values())
                .unwrap_or_default();
            let elements = self.vb.container.elements();
            for (element, default) in elements.iter().zip(&defaults) {
                element.set_edit_value(default)?;
            }
            edit::update_count_via_path(self, Some(&self.vb.vb_value_def));
        }
        Ok(())
    }

    fn assign_add(&self) -> Result<Option<ElementRef>, EditError> {
        if self.kind != ValueKind::Array {
            return Ok(None);
        }
        edit::check_edit_allowed(self)?;
        self.do_init();
        let self_ref = self.element_ref();
        let sorted = sort_sub_records() && self.vb.vb_value_def.as_array_def().is_some_and(|a| a.get_sorted());
        let added = ValueImpl::array_assign_add(
            &self_ref,
            &self.vb.container,
            &self.vb.file,
            &self.vb.vb_value_def,
            sorted,
        )?;
        edit::check_count(self, Some(&self.vb.vb_value_def));
        edit::check_terminator(self, Some(&self.vb.vb_value_def));
        Ok(added.map(|element| element as ElementRef))
    }

    fn add_impl(&self, _name: &str, _silent: bool) -> Result<Option<ElementRef>, EditError> {
        self.assign_add()
    }

    /// Port of `TwbArray.AssignInternal` and
    /// `TwbStringListTerminator.AssignInternal`; the other values are
    /// `TwbContainer`'s.
    fn assign_internal(
        &self,
        index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        match self.kind {
            ValueKind::Array => self.array_assign_internal(index, source, only_sk),
            ValueKind::Terminator => Ok(None),
            _ => super::assign::container_assign_internal(self, index, source, only_sk),
        }
    }

    /// Port of `TwbArray.CanAssignInternal` and
    /// `TwbStringListTerminator.CanAssignInternal`.
    fn can_assign_internal(&self, index: i32, source: Option<&ElementRef>, check_dont_show: bool) -> bool {
        match self.kind {
            ValueKind::Array => self.array_can_assign_internal(index, source, check_dont_show),
            ValueKind::Terminator => false,
            _ => super::assign::container_can_assign_internal(self, index, source, check_dont_show),
        }
    }

    /// Port of `TwbRecordHeaderStruct.IsElementEditable` (only the record
    /// flags) and `TwbContainedInElement.IsElementEditable` (nothing).
    fn is_element_editable(&self, element: Option<&ElementRef>) -> bool {
        if self.vb.contained_in.load(Ordering::Relaxed) {
            return false;
        }
        if self.vb.record_header.load(Ordering::Relaxed) {
            let flags = element
                .and_then(|element| element.get_value_def())
                .is_some_and(|def| def.get_name().eq_ignore_ascii_case("Record Flags"));
            return flags && super::assign::parent_allows_edit(self);
        }
        super::assign::container_is_element_editable(self, element)
    }

    /// Port of `TwbArray.IsElementRemovable`.
    fn is_element_removable(&self, element: &ElementRef) -> bool {
        if self.kind != ValueKind::Array {
            return false;
        }
        let value_def = &self.vb.vb_value_def;
        let mut result = self.is_element_editable(Some(element))
            && !value_def.def_base().def_flags.contains(DefFlag::dfArrayStaticSize)
            && value_def.as_array_def().is_some_and(|array| array.get_count() <= 0);
        if result && value_def.def_base().def_flags.contains(DefFlag::dfRemoveLastOnly) {
            result = self
                .vb
                .container
                .elements()
                .last()
                .is_some_and(|last| Arc::ptr_eq(last, element));
        }
        result
    }

    /// Port of `TwbValueBase.CanContainFormIDs`, and of the record header
    /// and the contained-in element, which hold none.
    fn can_contain_form_ids(&self) -> bool {
        if self.vb.record_header.load(Ordering::Relaxed) || self.vb.contained_in.load(Ordering::Relaxed) {
            return false;
        }
        self.vb
            .vb_value_def
            .def_base()
            .def_flags
            .contains(DefFlag::dfCanContainFormID)
    }

    /// Port of `AddIfMissingInternal` of `TwbArray` and
    /// `TwbRecordHeaderStruct`; the other values raise (the flags of
    /// `TwbValue` are not child elements in the port).
    fn add_if_missing_internal(&self, source: &ElementRef, args: &CopyArgs) -> Result<Option<ElementRef>, EditError> {
        if self.vb.record_header.load(Ordering::Relaxed) {
            return self.record_header_add_if_missing_internal(source, args);
        }
        match self.kind {
            ValueKind::Array => self.array_add_if_missing_internal(source, args),
            _ => Err(format!("{}.AddIfMissingInternal is not implemented", self.get_name())),
        }
    }

    /// Port of `TwbArray.BeforeActualRemove`: the counters along the count
    /// paths go to zero.
    fn before_actual_remove(&self) {
        if self.kind == ValueKind::Array
            && let Some(array_def) = self.vb.vb_value_def.as_array_def()
        {
            edit::zero_count_paths(self, array_def.get_count_paths());
        }
    }

    fn add_string_list_terminator(&self) {
        let self_ref = self.element_ref();
        create_string_list_terminator(&self_ref, &self.vb.file);
    }

    fn get_is_editable_impl(&self) -> bool {
        if crate::interface::globals::is_internal_edit() {
            return true;
        }
        self.do_init();
        let self_ref = self.element_ref();
        self.vb.vb_value_def.get_is_editable(self.data(), Some(&self_ref))
    }

    /// Port of `TwbValueBase.GetSortKeyInternal`.
    fn get_sort_key_impl(&self, extended: bool) -> String {
        self.do_init();
        let self_ref = self.element_ref();
        self.vb.vb_value_def.to_sort_key(self.data(), Some(&self_ref), extended)
    }

    /// Port of `TwbDataContainer.WriteToStreamInternal` for a value, and of
    /// `TwbStringListTerminator.WriteToStreamInternal` for the terminator.
    fn write_to_stream(&self, out: &mut Vec<u8>, reset: super::ResetModified) -> Result<(), super::SaveError> {
        if self.kind == ValueKind::Terminator {
            out.push(0);
            self.vb.base.reset_modified(reset);
            return Ok(());
        }
        let dont_save = self.vb.dont_save.load(Ordering::Relaxed)
            || self.vb.vb_value_def.def_base().def_flags.contains(DefFlag::dfDontSave);
        super::write::data_container_write_to_stream(self, self.data(), dont_save, out, reset)
    }
}

impl DataContainer for ValueImpl {
    fn get_data(&self) -> DataPtr<'_> {
        self.data()
    }

    fn get_block(&self) -> Option<&[u8]> {
        if let Some(bytes) = self.vb.storage.current() {
            return Some(bytes);
        }
        Some(match self.vb.decompressed.get() {
            Some(block) => block.as_slice(),
            None => self.vb.block.as_slice(),
        })
    }
}

impl Container for ValueImpl {
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
        self.do_init();
        self.vb.container.element_count() as i32
    }

    /// Port of `TwbContainer.GetElement` with `cntElementsMap`: the element
    /// map of the definition reorders the elements for the callers.
    fn get_element(&self, index: i32) -> Option<ElementRef> {
        self.do_init();
        let count = self.vb.container.element_count();
        let mut index = usize::try_from(index).ok()?;
        if index >= count {
            return None;
        }
        let map = self.vb.vb_value_def.get_element_map();
        if map.len() == count {
            index = map[index] as usize;
        }
        self.vb.container.element_at(index)
    }

    fn get_element_by_sort_order(&self, sort_order: i32) -> Option<ElementRef> {
        self.do_init();
        self.vb.container.element_by_sort_order(sort_order)
    }

    fn get_any_element(&self) -> Option<ElementRef> {
        self.get_element(0)
    }

    fn get_additional_element_count(&self) -> i32 {
        0
    }
}
