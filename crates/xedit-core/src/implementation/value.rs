// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! `TwbValueBase`, `TwbValue`, `TwbStruct`, `TwbArray` and `TwbUnion`: the
//! elements of the values inside a subrecord, built from the value
//! definitions over the data of the subrecord.
//!
//! Not ported yet: flags as array elements (`wbFlagsAsArray`), the sorting
//! of sorted arrays, chapters and the compressed structures of save files.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use crate::interface::def::{EmptyDef, NamedDef, NamedDefArgs, ValueDef};
use crate::interface::element::{Container, DataContainer, DataPtr, Element, ElementRef, FileRef, MainRecordRef};
use crate::interface::form_id::FormID;
use crate::interface::globals::sort_sub_records;
use crate::interface::misc::Variant;
use crate::interface::types::{ConflictPriority, DefFlag, DefType, ElementType, TriBool};

use super::{ContainerBase, DataBlock, ElementBase, ElementImpl};

/// The fields of `TwbValueBase`.
pub struct ValueBase {
    pub(super) base: ElementBase,
    pub(super) container: ContainerBase,
    file: Weak<super::FileImpl>,
    block: DataBlock,
    /// The range of the data of the element in `block`. `None` for an
    /// element without data, such as an optional member that is missing.
    range: Option<(usize, usize)>,
    vb_value_def: Arc<dyn ValueDef>,
    e_name_suffix: String,
    init: super::InitOnce,
    optional_and_missing: AtomicBool,
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
            range,
            vb_value_def: value_def,
            e_name_suffix: name_suffix.to_owned(),
            init: super::InitOnce::new(),
            optional_and_missing: AtomicBool::new(false),
        }
    }

    pub fn data(&self) -> DataPtr<'_> {
        let (start, end) = self.range?;
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
    loop {
        let Some(resolvable) = result.as_resolvable_def() else {
            return result;
        };
        // UPSTREAM-QUIRK: `BeginResolve` guards against recursion upstream;
        // the definitions resolve from the data and the element directly.
        match resolvable.resolve_def(data, element) {
            Some(resolved) => result = resolved.clone(),
            None => return result,
        }
    }
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
    let range = if cursor.has_data() || cursor.pos == cursor.end {
        let size = value_def.get_size(cursor.data(), Some(container));
        if (0..i32::MAX).contains(&size) {
            end = (start + size as usize).min(cursor.end);
        }
        Some((start, end))
    } else {
        None
    };
    let element = Arc::new_cyclic(|self_ref: &Weak<ValueImpl>| ValueImpl {
        self_ref: self_ref.clone(),
        vb: ValueBase::new(container, file, &cursor.block, range, value_def, name_suffix),
        kind,
    });
    if range.is_some() {
        cursor.pos = end;
    }
    if let Some(parent) = container.as_element_impl().and_then(ElementImpl::container_base) {
        parent.add_element(element.clone());
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
            let Some((start, end)) = self.vb.range else { return };
            let self_ref = self.element_ref();
            let mut cursor = Cursor {
                block: self.vb.block.clone(),
                pos: start,
                end,
            };
            match self.kind {
                ValueKind::Value => {}
                ValueKind::Struct => struct_do_init(&self.vb.vb_value_def, &self_ref, &self.vb.file, &mut cursor),
                ValueKind::Array => {
                    array_do_init(&self.vb.vb_value_def, &self_ref, &self.vb.file, &mut cursor);
                }
                ValueKind::Union => {
                    union_do_init(&self.vb.vb_value_def, &self_ref, &self.vb.file, &mut cursor);
                }
            }
        });
    }

    pub fn value_def(&self) -> &Arc<dyn ValueDef> {
        &self.vb.vb_value_def
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
    sorted
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
    element_display_name!(vb_base);

    fn get_name(&self) -> String {
        self.vb.name()
    }

    fn get_data_size(&self) -> i32 {
        match self.vb.range {
            Some((start, end)) => (end - start) as i32,
            None => self.vb.vb_value_def.get_default_size(None, None),
        }
    }

    fn get_element_type(&self) -> ElementType {
        match self.kind {
            ValueKind::Value => ElementType::etValue,
            ValueKind::Struct => ElementType::etStruct,
            ValueKind::Array => ElementType::etArray,
            ValueKind::Union => ElementType::etUnion,
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
    fn get_element_native_value(&self, path: &str) -> Variant {
        self.get_element_by_path(path)
            .map_or(Variant::Empty, |element| element.get_native_value())
    }

    fn get_element_by_name(&self, name: &str) -> Option<ElementRef> {
        super::element_by_name(self, name)
    }

    fn get_element_by_path(&self, path: &str) -> Option<ElementRef> {
        super::element_by_path(self, path)
    }

    fn get_element_count(&self) -> i32 {
        self.do_init();
        self.vb.container.elements().len() as i32
    }

    fn get_element(&self, index: i32) -> Option<ElementRef> {
        self.do_init();
        self.vb.container.elements().get(usize::try_from(index).ok()?).cloned()
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
