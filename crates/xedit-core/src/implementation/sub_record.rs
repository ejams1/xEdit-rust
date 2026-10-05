// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! `TwbSubRecord`, `TwbSubRecordArray` and `TwbSubRecordStruct`: the
//! subrecords of a main record and the groups the definition makes of them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, Weak};

use crate::interface::def::{NamedDef, ValueDef};
use crate::interface::element::{Container, DataContainer, DataPtr, Element, ElementRef, FileRef, MainRecordRef};
use crate::interface::form_id::FormID;
use crate::interface::globals::{ignore_records, sort_sub_records};
use crate::interface::misc::{Variant, progress};
use crate::interface::sub_record::RecordMemberDef;
use crate::interface::sub_record_group::RecordDef;
use crate::interface::types::{ConflictPriority, DefFlag, DefType, ElementType, Signature, TriBool};

use super::structs::SubRecordHeaderStruct;
use super::value::{Cursor, array_do_init, create_value_element, resolve, struct_do_init, union_do_init};
use super::{ContainerBase, DataBlock, ElementBase, ElementImpl, MainRecordImpl};
use std::sync::OnceLock;

/// Port of `TwbSubRecord`.
pub struct SubRecordImpl {
    self_ref: Weak<SubRecordImpl>,
    pub(super) base: ElementBase,
    pub(super) container: ContainerBase,
    file: Weak<super::FileImpl>,
    block: DataBlock,
    sr_struct: SubRecordHeaderStruct,
    /// Port of `DoInit`: the value elements are built once.
    sr_init: OnceLock<()>,
    /// Port of `srValueDef`: the resolved value definition, when the value
    /// has no name and its elements live in the subrecord itself.
    sr_value_def: RwLock<Option<Arc<dyn ValueDef>>>,
    /// The range of the subrecord data in `bytes`, after the header.
    dc_data_base: usize,
    dc_data_end: usize,
    sr_def: RwLock<Option<Arc<dyn RecordMemberDef>>>,
    sr_skipped: AtomicBool,
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
        let mut data_size = usize::from(header.data_size);
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
        let dc_data_base = *offset + SubRecordHeaderStruct::SIZE;
        let dc_data_end = (dc_data_base + data_size).min(data_start + data.len());
        let sub_record = Arc::new_cyclic(|self_ref: &Weak<SubRecordImpl>| SubRecordImpl {
            self_ref: self_ref.clone(),
            base: ElementBase::new(Some(container)),
            container: ContainerBase::default(),
            file: Arc::downgrade(file),
            block: block.clone(),
            sr_struct: header,
            sr_init: OnceLock::new(),
            sr_value_def: RwLock::new(None),
            dc_data_base,
            dc_data_end,
            sr_def: RwLock::new(None),
            sr_skipped: AtomicBool::new(false),
        });
        container_base.add_element(sub_record.clone());
        *offset = dc_data_end;
        Some(sub_record)
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

    pub fn data(&self) -> DataPtr<'_> {
        self.block.as_slice().get(self.dc_data_base..self.dc_data_end)
    }

    fn element_ref(&self) -> ElementRef {
        self.self_ref.upgrade().expect("a subrecord is alive while it is used")
    }

    /// Port of `TwbSubRecord.Init`: the value elements of the subrecord.
    pub fn do_init(&self) {
        self.sr_init.get_or_init(|| {
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
            let mut cursor = Cursor {
                block: self.block.clone(),
                pos: self.dc_data_base,
                end: self.dc_data_end,
            };
            let value_def = resolve(value.clone(), cursor.data(), Some(&self_ref));
            if value_def.get_name().is_empty() || value.def_base().def_flags.contains(DefFlag::dfUnionStaticResolve) {
                *self.sr_value_def.write().unwrap() = Some(value_def.clone());
                match value_def.get_def_type() {
                    DefType::dtArray => {
                        array_do_init(&value_def, &self_ref, &self.file, &mut cursor);
                    }
                    DefType::dtStruct | DefType::dtStructChapter => {
                        struct_do_init(&value_def, &self_ref, &self.file, &mut cursor)
                    }
                    DefType::dtUnion => {
                        if let Some(resolved) = union_do_init(&value_def, &self_ref, &self.file, &mut cursor) {
                            *self.sr_value_def.write().unwrap() = Some(resolved);
                        }
                    }
                    _ => {}
                }
            } else {
                create_value_element(&self_ref, &self.file, &mut cursor, value_def, "");
            }
        });
    }

    /// The value definition the subrecord data is read with.
    fn value_def(&self) -> Option<Arc<dyn ValueDef>> {
        self.do_init();
        if let Some(resolved) = self.sr_value_def.read().unwrap().clone() {
            return Some(resolved);
        }
        self.def()?.as_sub_record_def()?.get_value()
    }

    pub fn def(&self) -> Option<Arc<dyn RecordMemberDef>> {
        self.sr_def.read().unwrap().clone()
    }

    /// Port of `SetDef`.
    pub fn set_def(&self, def: Arc<dyn RecordMemberDef>) {
        *self.sr_def.write().unwrap() = Some(def);
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
    pub(super) base: ElementBase,
    pub(super) container: ContainerBase,
    file: Weak<super::FileImpl>,
    arc_def: Arc<dyn RecordMemberDef>,
}

/// Port of `TwbSubRecordStruct`: the subrecords of one structure member.
pub struct SubRecordStructImpl {
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
    let array = Arc::new(SubRecordArrayImpl {
        base: ElementBase::new(Some(owner)),
        container: ContainerBase::default(),
        file: file.clone(),
        arc_def: def,
    });
    let array_ref: ElementRef = array.clone();
    array.do_process(&array_ref, owner_base, pos);
    array
}

impl SubRecordArrayImpl {
    /// Port of `DoProcess`.
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
        // UPSTREAM-QUIRK: the sorted flag is only set under wbSortSubRecords.
        let _ = sort_sub_records();
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
    let structure = Arc::new(SubRecordStructImpl {
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
            // UPSTREAM-QUIRK: `dfMergeIfMultiple` merges into the last
            // element upstream; merging belongs to the write path.
            if let Some(last) = &last_def
                && last.def_base().def_flags.contains(DefFlag::dfMergeIfMultiple)
                && last.can_handle(Some(&self_ref), signature, Some(element))
            {
                progress(&format!("<Warning: merging of multiple {signature} is not ported>"));
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
        found_members[current_def_pos] = Some(new_element);
        let terminator = current_def.def_base().def_flags.contains(DefFlag::dfTerminator);
        last_def = Some(current_def);
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
    }
    let Some(mr_def) = &record.mr_def else { return };

    let member_count = usize::try_from(mr_def.get_member_count()).unwrap_or(0);
    let mut last_element_for_member: Vec<Option<ElementRef>> = vec![None; member_count];
    let mut current_def_pos = 0usize;
    let mut current_rec_pos = 0usize;
    let mut found_error = false;
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
                    record.set_editor_id(mr_def.get_editor_id(element));
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
    }
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
    fn get_name(&self) -> String {
        match self.def() {
            Some(def) => format!("{} - {}", self.get_display_signature(), def.get_name()),
            None => self.get_display_signature(),
        }
    }

    fn get_data_size(&self) -> i32 {
        (self.dc_data_end - self.dc_data_base) as i32
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

    fn get_record_signature(&self) -> Option<Signature> {
        Some(self.sr_struct.signature)
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
    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.container)
    }

    fn sub_record_impl(&self) -> Option<&SubRecordImpl> {
        Some(self)
    }

    fn element_base(&self) -> &ElementBase {
        &self.base
    }
}

impl DataContainer for SubRecordImpl {
    fn get_data(&self) -> DataPtr<'_> {
        self.data()
    }
}

macro_rules! container_by_elements {
    ($init:ident) => {
        fn get_element_native_value(&self, path: &str) -> Variant {
            self.get_element_by_path(path)
                .map_or(Variant::Empty, |element| element.get_native_value())
        }

        fn get_element_by_name(&self, name: &str) -> Option<ElementRef> {
            self.$init();
            self.container
                .elements()
                .into_iter()
                .find(|element| element.get_name() == name)
        }

        fn get_element_by_path(&self, path: &str) -> Option<ElementRef> {
            super::element_by_path(self, path)
        }

        fn get_element_count(&self) -> i32 {
            self.$init();
            self.container.elements().len() as i32
        }

        fn get_element(&self, index: i32) -> Option<ElementRef> {
            self.$init();
            self.container
                .elements()
                .get(usize::try_from(index).ok()?)
                .cloned()
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
    };
}

impl Container for SubRecordImpl {
    container_by_elements!(do_init);
}

impl Element for SubRecordArrayImpl {
    element_common!(element_base);

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
    fn container_base(&self) -> Option<&ContainerBase> {
        Some(&self.container)
    }

    fn sub_record_array_impl(&self) -> Option<&SubRecordArrayImpl> {
        Some(self)
    }

    fn element_base(&self) -> &ElementBase {
        &self.base
    }
}

impl SubRecordArrayImpl {
    fn no_init(&self) {}
}

impl Container for SubRecordArrayImpl {
    container_by_elements!(no_init);
}

impl Element for SubRecordStructImpl {
    element_common!(element_base);

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
    container_by_elements!(no_init);
}
