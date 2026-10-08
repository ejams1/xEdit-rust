// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! `TwbFlag`: one set flag of a flags value as an element of its own, and
//! the `wbFlagsAsArray` part of `ValueDoInit` that makes them.
//!
//! The GUI sets `wbFlagsAsArray`, so a flags value (a `TwbValue`, a
//! `TwbUnion` of a value, or a subrecord whose value is the flags integer
//! itself) holds a `TwbFlag` for each flag that is set, and the view and
//! the conflict detection compare the flags one by one. The port does not
//! put the flags into the element list of their container yet (owed from
//! phase 3: flags as child elements); [`flags_as_array`] builds them on
//! demand for the readers that need them, with the container as their
//! parent, so that the callbacks of the definitions see the tree upstream
//! has.

use std::sync::{Arc, Weak};

use crate::interface::def::{Def, NamedDef, ValueDef};
use crate::interface::element::{Element, ElementRef, FileRef, MainRecordRef};
use crate::interface::flags::FlagsDef;
use crate::interface::form_id::FormID;
use crate::interface::globals::{flags_as_array, hide_unused, translation_mode};
use crate::interface::integer::IntegerDefInterface;
use crate::interface::misc::Variant;
use crate::interface::types::{ConflictPriority, DefType, ElementType, TriBool};

/// Port of `TwbFlag`.
pub struct FlagImpl {
    self_ref: Weak<FlagImpl>,
    container: Weak<dyn Element>,
    integer_def: Arc<dyn IntegerDefInterface>,
    /// Port of `GetFlagsDef`: upstream keeps `fFlagsDef` only when the
    /// formater of the integer can not change and asks the integer for it
    /// otherwise; the flags are built on demand here, so the formater of
    /// the time is the one upstream would find.
    flags_def: Arc<FlagsDef>,
    index: i32,
    sort_order: std::sync::atomic::AtomicI32,
}

impl FlagImpl {
    /// Port of `TwbFlag.Create`: the sort order and the memory order are
    /// the index of the flag.
    fn create(
        container: &ElementRef,
        integer_def: Arc<dyn IntegerDefInterface>,
        flags_def: Arc<FlagsDef>,
        index: i32,
    ) -> Arc<Self> {
        Arc::new_cyclic(|self_ref| FlagImpl {
            self_ref: self_ref.clone(),
            container: Arc::downgrade(container),
            integer_def,
            flags_def,
            index,
            sort_order: std::sync::atomic::AtomicI32::new(index),
        })
    }

    /// Port of `GetFlagIndex`.
    pub fn flag_index(&self) -> i32 {
        self.index
    }

    /// Port of `GetFlagsDef`.
    pub fn flags_def(&self) -> &Arc<FlagsDef> {
        &self.flags_def
    }

    fn container_ref(&self) -> Option<ElementRef> {
        self.container.upgrade()
    }

    /// The flag as the element the callbacks of the definitions get.
    fn self_ref(&self) -> Option<ElementRef> {
        Some(self.self_ref.upgrade()? as ElementRef)
    }

    /// The character of the flag in the edit value of the container.
    fn edit_char(&self) -> Option<char> {
        let value = self.container_ref()?.get_edit_value();
        value.chars().nth(usize::try_from(self.index).ok()?)
    }
}

impl Element for FlagImpl {
    fn get_element_id(&self) -> usize {
        std::ptr::from_ref(self) as *const () as usize
    }

    /// Port of `TwbFlag.GetName`.
    fn get_name(&self) -> String {
        self.flags_def.get_flag(self.index as usize, false)
    }

    fn get_full_path(&self) -> String {
        match self.container_ref() {
            Some(container) => format!("{} \\ {}", container.get_full_path(), self.get_name()),
            None => self.get_name(),
        }
    }

    /// Port of `TwbFlag.GetDataSize`.
    fn get_data_size(&self) -> i32 {
        0
    }

    fn get_masters_updated(&self) -> bool {
        false
    }

    fn add_referenced_from_id(&self, form_id: FormID) {
        super::refs::add_referenced_from_id(form_id)
    }

    /// Port of `TwbFlag.GetEditValue`.
    fn get_edit_value(&self) -> String {
        self.edit_char().map_or_else(|| "0".to_owned(), String::from)
    }

    /// Port of `TwbFlag.GetValue`.
    fn get_value(&self) -> String {
        self.flags_def.get_flag(self.index as usize, false)
    }

    /// Port of `TwbFlag.GetSummary`.
    fn get_summary(&self) -> String {
        self.flags_def.get_flag(self.index as usize, true)
    }

    fn get_links_to(&self) -> Option<ElementRef> {
        None
    }

    /// Port of `TwbFlag.GetElementType`.
    fn get_element_type(&self) -> ElementType {
        ElementType::etFlag
    }

    /// Port of `TwbFlag.GetDef`.
    fn get_def(&self) -> Option<Arc<dyn NamedDef>> {
        Some(self.flags_def.get_flag_def(self.index)? as Arc<dyn NamedDef>)
    }

    /// Port of `TwbFlag.GetValueDef`.
    fn get_value_def(&self) -> Option<Arc<dyn ValueDef>> {
        Some(self.flags_def.get_flag_def(self.index)? as Arc<dyn ValueDef>)
    }

    /// Port of `TwbFlag.GetNativeValue`.
    fn get_native_value(&self) -> Variant {
        Variant::Bool(self.edit_char() == Some('1'))
    }

    /// Port of `TwbElement.GetSortOrder`.
    fn get_sort_order(&self) -> i32 {
        self.sort_order.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_sort_order(&self, order: i32) {
        self.sort_order.store(order, std::sync::atomic::Ordering::Relaxed);
    }

    /// Port of `TwbFlag.GetSortKeyInternal` (the key is not cached here, so
    /// the check of `GetSortKey` for a changed definition is not needed).
    fn get_sort_key(&self, _extended: bool) -> String {
        let root = crate::interface::def::get_root(&(self.flags_def.clone() as crate::interface::def::DefRef));
        let flags_def = root.into_flags_def().unwrap_or_else(|| self.flags_def.clone());
        let base = flags_def.get_base_flags_def();
        let mut result = format!("{:08X}{:02X}", base.get_def_id(), self.index);
        if !flags_def.equals(Some(&*base)) {
            let index = self.index as usize;
            let name = flags_def.get_flag(index, false);
            let base_name = if index < base.get_flag_count() as usize {
                base.get_flag(index, false)
            } else {
                String::new()
            };
            if !name.eq_ignore_ascii_case(&base_name) {
                result.push_str(&name);
            }
        }
        result
    }

    fn get_container(&self) -> Option<ElementRef> {
        self.container_ref()
    }

    /// The flags of a value keep the order of their index.
    fn get_memory_order(&self) -> i32 {
        self.index
    }

    fn get_file(&self) -> Option<FileRef> {
        self.container_ref()?.get_file()
    }

    fn get_containing_main_record(&self) -> Option<MainRecordRef> {
        self.container_ref()?.get_containing_main_record()
    }

    fn get_path(&self) -> String {
        match self.container_ref() {
            Some(container) => format!("{} \\ {}", container.get_path(), self.get_name()),
            None => self.get_name(),
        }
    }

    fn get_localized(&self) -> TriBool {
        TriBool::tbUnknown
    }

    /// Port of `TwbFlag.GetConflictPriority`.
    fn get_conflict_priority(&self) -> ConflictPriority {
        let mut result = if translation_mode() || self.flags_def.get_flag_ignore_conflict(self.index) {
            ConflictPriority::cpIgnore
        } else {
            let self_ref = self.self_ref();
            self.integer_def.get_conflict_priority(self_ref.as_ref())
        };
        if result == ConflictPriority::cpFormID {
            result = ConflictPriority::cpCritical;
            if let Some(main_record) = self.get_containing_main_record()
                && matches!(main_record.get_signature().0.as_slice(), b"GMST" | b"DFOB")
            {
                result = ConflictPriority::cpBenign;
            }
        }
        result
    }

    /// Port of `TwbFlag.GetDontShow`.
    fn get_dont_show(&self) -> bool {
        let self_ref = self.self_ref();
        self.flags_def.get_flag_dont_show(self_ref.as_ref(), self.index)
    }
}

/// The integer definition `ValueDoInit` reads a container's value with,
/// when the container holds a value directly: a `TwbValue`, the simple case
/// of a `TwbUnion`, or a subrecord whose value is not an array, a structure
/// or a union.
fn flags_source(container: &ElementRef) -> Option<Arc<dyn IntegerDefInterface>> {
    let element = container.as_element_impl()?;
    let value_def: Arc<dyn ValueDef> = if let Some(value) = element.value_impl() {
        match value.get_element_type() {
            // `TwbValue.Init` and the simple case of `UnionDoInit`: the
            // definition of the element, resolved over its data.
            ElementType::etValue | ElementType::etUnion => {
                super::value::resolve(value.value_def().clone(), value.data(), Some(container))
            }
            _ => return None,
        }
    } else {
        // `TwbSubRecord.Init` with a value of its own that is not an array,
        // a structure or a union.
        let sub_record = element.sub_record_impl()?;
        let value_def = sub_record.resolved_value_def()?;
        super::value::resolve(value_def, sub_record.data(), Some(container))
    };
    if matches!(
        value_def.get_def_type(),
        DefType::dtArray | DefType::dtStruct | DefType::dtStructChapter | DefType::dtUnion
    ) {
        return None;
    }
    value_def.into_integer_def()
}

/// Whether `ValueDoInit` makes `container` a flags value: its resolved value
/// definition is an integer with a flags formater, under `wbFlagsAsArray`.
pub fn is_flags(container: &ElementRef) -> bool {
    flags_as_array() && flags_def_of(container).is_some()
}

fn flags_def_of(container: &ElementRef) -> Option<(Arc<dyn IntegerDefInterface>, Arc<FlagsDef>)> {
    let integer_def = flags_source(container)?;
    let flags_def = integer_def.get_formater(Some(container))?.into_flags_def()?;
    Some((integer_def, flags_def))
}

/// Port of the `wbFlagsAsArray` part of `ValueDoInit`: the `TwbFlag` of
/// every flag that is set, in the order of their index, for a container
/// that is a flags value (`None` for one that is not, or when
/// `wbFlagsAsArray` is off). A flag without a name, and one named `Unused`
/// under `wbHideUnused`, has no element.
pub fn flags_as_array_of(container: &ElementRef) -> Option<Vec<ElementRef>> {
    if !flags_as_array() {
        return None;
    }
    let (integer_def, flags_def) = flags_def_of(container)?;
    let data = match container.as_element_impl() {
        Some(element) => match (element.value_impl(), element.sub_record_impl()) {
            (Some(value), _) => value.data().map(<[u8]>::to_vec),
            (None, Some(sub_record)) => sub_record.data().map(<[u8]>::to_vec),
            (None, None) => None,
        },
        None => None,
    };
    let mut result = Vec::new();
    let flag_count = flags_def.get_flag_count();
    if let Some(data) = data
        && flag_count > 0
    {
        let mut value = integer_def.to_int(Some(&data), Some(container));
        if value != 0 {
            for index in 0..flag_count.min(64) {
                if value & (1i64 << index) == 0 {
                    continue;
                }
                let name = flags_def.get_flag(index as usize, false);
                if !name.is_empty() && !(hide_unused() && name.eq_ignore_ascii_case("Unused")) {
                    result
                        .push(FlagImpl::create(container, integer_def.clone(), flags_def.clone(), index) as ElementRef);
                }
                value &= !(1i64 << index);
                if value == 0 {
                    break;
                }
            }
        }
    }
    Some(result)
}
