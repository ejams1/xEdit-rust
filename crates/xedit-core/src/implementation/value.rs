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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock, Weak};

use xedit_io::compression::CompressionType;

use crate::interface::def::{EmptyDef, NamedDef, NamedDefArgs, ValueDef};
use crate::interface::element::{Container, DataContainer, DataPtr, Element, ElementRef, FileRef, MainRecordRef};
use crate::interface::form_id::FormID;
use crate::interface::globals::{ToolSource, hide_never_show, sort_sub_records, tool_source};
use crate::interface::misc::Variant;
use crate::interface::struct_def::ChapterKind;
use crate::interface::types::{ConflictPriority, DefFlag, DefType, ElementType, TriBool, dt_non_values};

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
        }
    }

    pub fn range(&self) -> Option<(usize, usize)> {
        *self.range.read().unwrap()
    }

    pub fn data(&self) -> DataPtr<'_> {
        if let Some(block) = self.decompressed.get() {
            return Some(block.as_slice());
        }
        let (start, end) = self.range()?;
        self.block.as_slice().get(start..end)
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
    pub(super) fn data(&self) -> DataPtr<'_> {
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
    let range = (cursor.has_data() || cursor.pos == cursor.end).then_some((start, end));
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
        let size = element.vb.vb_value_def.get_size(element.vb.data(), Some(&self_ref));
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

    /// Port of `DoInit`: builds the children once.
    pub fn do_init(&self) {
        self.vb.init.run(|| {
            let Some((start, end)) = self.vb.range() else { return };
            let self_ref = self.element_ref();
            let mut cursor = Cursor {
                block: self.vb.block.clone(),
                pos: start,
                end,
            };
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
                    array_do_init(&self.vb.vb_value_def, &self_ref, &self.vb.file, &mut cursor);
                }
                ValueKind::Union => {
                    union_do_init(&self.vb.vb_value_def, &self_ref, &self.vb.file, &mut cursor);
                }
                ValueKind::Terminator => {}
            }
            // `StructDoInit`, `ArrayDoInit` and `UnionDoInit` end with the
            // `AfterLoad` of the definition. The callbacks of the plugin
            // definitions only act while editing, so only the save
            // definitions, which fill their lookup tables there, get the call.
            if tool_source() == ToolSource::tsSaves
                && matches!(self.kind, ValueKind::Struct | ValueKind::Array | ValueKind::Union)
            {
                self.vb.vb_value_def.after_load(&self_ref);
            }
        });
    }

    pub fn value_def(&self) -> &Arc<dyn ValueDef> {
        &self.vb.vb_value_def
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
        let uncompressed_size = struct_def.get_sizing(self.vb.data(), Some(self_ref), &mut compressed_size);
        if uncompressed_size == 0 {
            return None;
        }
        let data = self.vb.data().unwrap_or_default();
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

    /// Port of `TwbValueBase.GetValue`.
    pub fn get_value(&self) -> String {
        self.do_init();
        let self_ref = self.element_ref();
        self.vb.vb_value_def.to_string(self.vb.data(), Some(&self_ref))
    }
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
) -> bool {
    let Some(array_def) = value_def.as_array_def() else {
        return false;
    };
    let sorted = sort_sub_records() && array_def.get_sorted();
    let size_prefix = array_def.get_prefix_size(cursor.data()).max(0) as usize;
    let mut element_def = array_def.get_element().clone();
    if element_def.get_def_type() == DefType::dtResolvable
        || element_def.def_base().def_flags.contains(DefFlag::dfUnionStaticResolve)
    {
        element_def = resolve(element_def, None, Some(container));
    }
    let var_size = array_def.get_is_variable_size();
    let mut arr_size = i64::from(array_def.get_count());
    if arr_size < 0 {
        arr_size = i64::from(array_def.get_prefix_count(cursor.data()));
    } else if arr_size < 1
        && let Some(callback) = array_def.get_count_callback()
    {
        arr_size = i64::from(callback(cursor.data(), Some(container)));
    } else if var_size && arr_size < 1 {
        arr_size = i64::from(i32::MAX);
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
            i += 1;
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
    sorted
}

/// Port of `TwbStringListTerminator.Create`: the `Terminator` element after
/// the strings of an array of zero-terminated strings. It has no data of its
/// own; its size is the one byte of the terminator.
fn create_string_list_terminator(container: &ElementRef, file: &Weak<super::FileImpl>) -> Arc<ValueImpl> {
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
        let resolved = resolve(self.vb.vb_value_def.clone(), self.vb.data(), Some(&self_ref));
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

    fn get_data_size(&self) -> i32 {
        if self.kind == ValueKind::Terminator {
            return 1;
        }
        match self.vb.range() {
            Some((start, end)) => (end - start) as i32,
            None => self.vb.vb_value_def.get_default_size(None, None),
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
        self.vb.vb_value_def.to_edit_value(self.vb.data(), Some(&self_ref))
    }

    fn get_native_value(&self) -> Variant {
        self.do_init();
        let self_ref = self.element_ref();
        self.vb.vb_value_def.to_native_value(self.vb.data(), Some(&self_ref))
    }

    /// Port of `TwbValueBase.GetSummary`.
    fn get_summary(&self) -> String {
        self.do_init();
        let self_ref = self.element_ref();
        let mut links_to = None;
        self.vb
            .vb_value_def
            .to_summary(0, self.vb.data(), Some(&self_ref), &mut links_to)
    }

    /// Port of `TwbValueBase.InternalGetLinksTo`.
    fn get_links_to(&self) -> Option<ElementRef> {
        self.do_init();
        let self_ref = self.element_ref();
        self.vb.vb_value_def.get_links_to(self.vb.data(), Some(&self_ref))
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
    fn self_element_ref(&self) -> Option<ElementRef> {
        self.self_ref.upgrade().map(|element| element as ElementRef)
    }

    fn element_base(&self) -> &ElementBase {
        &self.vb.base
    }

    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.vb.container)
    }
}

impl DataContainer for ValueImpl {
    fn get_data(&self) -> DataPtr<'_> {
        self.vb.data()
    }
}

impl Container for ValueImpl {
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
