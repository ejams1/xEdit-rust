// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbArrayDef`.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Weak};

use super::byte_array::CountCallback;
use super::def::{
    Def, DefBase, DefCell, DefKind, DefRef, NamedDef, NamedDefArgs, NamedDefBase, ValueDef, ValueDefBase, set_parent,
    value_def_plumbing,
};
use super::element::{Container, DataPtr, ElementArg, ElementRef};
use super::enum_def::EnumDef;
use super::globals::{copy_is_running, never_sorted};
use super::misc::{length, read_integer_counter, read_integer_counter_size};
use super::struct_def::{data_from, trim};
use super::types::{CallbackType, ConflictPriority, DefFlag, DefType, ElementType};

/// Decides whether an array without a count has another element. The data
/// starts at the next element. The element is the array.
pub type ShouldIncludeCallback = Arc<dyn Fn(DataPtr, ElementArg) -> bool + Send + Sync>;

/// `arCount`: the count is in four bytes before the elements.
pub const ARRAY_COUNT_U32: i32 = -1;
/// `arCount`: the count is in two bytes before the elements.
pub const ARRAY_COUNT_U16: i32 = -2;
/// `arCount`: the count is in one byte before the elements.
pub const ARRAY_COUNT_U8: i32 = -4;
/// `arCount`: a square matrix whose side is in four bytes before the elements.
pub const ARRAY_COUNT_MATRIX_U32: i32 = -241;
/// `arCount`: a square matrix whose side is a counter of one to four bytes.
pub const ARRAY_COUNT_MATRIX_COUNTER: i32 = -253;
/// `arCount`: the count is a counter of one to four bytes.
pub const ARRAY_COUNT_COUNTER: i32 = -254;
/// `arCount`: no elements.
pub const ARRAY_COUNT_NONE: i32 = -255;

/// The upstream names of the array counts, for the generated definitions.
pub const ARC_U32: i32 = ARRAY_COUNT_U32;
pub const ARC_U16: i32 = ARRAY_COUNT_U16;
pub const ARC_U8: i32 = ARRAY_COUNT_U8;

/// The constructor arguments of `TwbArrayDef` after those of `TwbNamedDef`.
pub struct ArrayDefArgs {
    pub element: Arc<dyn ValueDef>,
    /// Number of elements when positive, 0 for as many as the data holds, or
    /// one of the negative `ARRAY_COUNT_` values.
    pub count: i32,
    /// The second constructor upstream: a callback gives the count.
    pub count_callback: Option<CountCallback>,
    pub labels: Vec<String>,
    pub sorted: bool,
    pub can_add_to: bool,
    pub terminated: bool,
}

/// Upstream `TwbArrayDef`: elements of one definition that follow each other.
pub struct ArrayDef {
    self_ref: Weak<ArrayDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
    ar_count: AtomicI32,
    ar_count_callback: DefCell<CountCallback>,
    ar_element: Arc<dyn ValueDef>,
    ar_labels: DefCell<Vec<String>>,
    ar_sorted: bool,
    ar_can_add_to: bool,
    ar_terminated: bool,
    ar_default_edit_values: DefCell<Vec<String>>,
    ar_count_paths: DefCell<Vec<String>>,
    ar_should_include: DefCell<ShouldIncludeCallback>,
    ar_wrongly_assumed_fixed_size_per_element: AtomicI32,
    ar_summary_delimiter: DefCell<String>,
    ar_summary_passthrough_max_count: AtomicI32,
    ar_summary_passthrough_max_length: AtomicI32,
    ar_summary_passthrough_max_depth: AtomicI32,
}

/// Port of `_GetCountCallbackForPath`: the count is the value of another
/// element of the container of the array.
fn get_count_callback_for_path(path: String) -> CountCallback {
    Arc::new(move |data, element| {
        if data.is_none() {
            return 0;
        }
        let Some(parent) = element.and_then(|element| element.get_container()) else {
            return 0;
        };
        let Some(container) = parent.as_container() else {
            return 0;
        };
        container
            .get_element_native_value(&path)
            .as_ordinal()
            .map_or(0, |count| count as u32)
    })
}

impl ArrayDef {
    /// Port of both `TwbArrayDef.Create` constructors.
    pub fn create(args: NamedDefArgs, array: ArrayDefArgs) -> Arc<Self> {
        let sorted = array.sorted && !never_sorted();
        assert!(!sorted || array.labels.is_empty());
        let count = if array.count_callback.is_some() { 0 } else { array.count };
        let (def, nd) = NamedDefBase::create(args);
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| {
            let parent: Weak<dyn Def> = self_ref.clone();
            Self {
                self_ref: self_ref.clone(),
                def,
                nd,
                vd: ValueDefBase::default(),
                ar_count: AtomicI32::new(count),
                ar_count_callback: DefCell::new(array.count_callback),
                ar_element: set_parent(array.element, &parent, false),
                ar_labels: DefCell::new(Some(array.labels)),
                ar_sorted: sorted,
                ar_can_add_to: array.can_add_to,
                ar_terminated: array.terminated,
                ar_default_edit_values: DefCell::default(),
                ar_count_paths: DefCell::default(),
                ar_should_include: DefCell::default(),
                ar_wrongly_assumed_fixed_size_per_element: AtomicI32::new(0),
                ar_summary_delimiter: DefCell::new(Some(", ".to_owned())),
                ar_summary_passthrough_max_count: AtomicI32::new(-1),
                ar_summary_passthrough_max_length: AtomicI32::new(-1),
                ar_summary_passthrough_max_depth: AtomicI32::new(-1),
            }
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbArrayDef.Clone` and `AfterClone`.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(
            NamedDefBase::clone_args(source),
            ArrayDefArgs {
                element: source.ar_element.clone(),
                count: source.get_count(),
                count_callback: source.get_count_callback(),
                labels: source.labels(),
                sorted: source.ar_sorted,
                can_add_to: source.ar_can_add_to,
                terminated: source.ar_terminated,
            },
        );
        ValueDefBase::after_clone(&*this, source);
        this.ar_summary_delimiter.assign(&source.ar_summary_delimiter);
        for (target, from) in [
            (
                &this.ar_summary_passthrough_max_count,
                &source.ar_summary_passthrough_max_count,
            ),
            (
                &this.ar_summary_passthrough_max_length,
                &source.ar_summary_passthrough_max_length,
            ),
            (
                &this.ar_summary_passthrough_max_depth,
                &source.ar_summary_passthrough_max_depth,
            ),
            (
                &this.ar_wrongly_assumed_fixed_size_per_element,
                &source.ar_wrongly_assumed_fixed_size_per_element,
            ),
        ] {
            target.store(from.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        this.ar_default_edit_values.assign(&source.ar_default_edit_values);
        this.ar_count_paths.assign(&source.ar_count_paths);
        this.ar_should_include.assign(&source.ar_should_include);
        this
    }

    fn labels(&self) -> Vec<String> {
        self.ar_labels.load().as_deref().cloned().unwrap_or_default()
    }

    pub fn get_element(&self) -> &Arc<dyn ValueDef> {
        &self.ar_element
    }

    pub fn get_count(&self) -> i32 {
        self.ar_count.load(Ordering::Relaxed)
    }

    pub fn get_count_callback(&self) -> Option<CountCallback> {
        self.ar_count_callback.load().as_deref().cloned()
    }

    pub fn get_count_paths(&self) -> Vec<String> {
        self.ar_count_paths.load().as_deref().cloned().unwrap_or_default()
    }

    pub fn get_element_label(&self, index: i32) -> String {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.ar_labels.load().as_deref()?.get(index).cloned())
            .unwrap_or_default()
    }

    /// Upstream `GetElementNameSuffix`: `#3`, or `#3 (Label)`. An element
    /// definition without a name shows the label alone.
    pub fn get_element_name_suffix(&self, index: i32) -> String {
        let mut result = self.get_element_label(index);
        if !result.is_empty() {
            if self.ar_element.get_name().is_empty() {
                return result;
            }
            result = format!(" ({result})");
        }
        format!("#{index}{result}")
    }

    pub fn get_sorted(&self) -> bool {
        copy_is_running() == 0 && self.ar_sorted
    }

    pub fn get_can_add_to(&self) -> bool {
        self.ar_can_add_to && !self.def.def_flags.contains(DefFlag::dfArrayStaticSize)
    }

    pub fn get_default_edit_values(&self) -> Vec<String> {
        self.ar_default_edit_values
            .load()
            .as_deref()
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_wrongly_assumed_fixed_size_per_element(&self) -> i32 {
        self.ar_wrongly_assumed_fixed_size_per_element.load(Ordering::Relaxed)
    }

    /// Size of the count before the elements.
    pub fn get_prefix_length(&self, data: DataPtr) -> i32 {
        match self.get_count() {
            ARRAY_COUNT_U32 | ARRAY_COUNT_MATRIX_U32 => 4,
            ARRAY_COUNT_U16 => 2,
            ARRAY_COUNT_U8 => 1,
            ARRAY_COUNT_MATRIX_COUNTER | ARRAY_COUNT_COUNTER => read_integer_counter_size(data) as i32,
            _ => 0,
        }
    }

    /// Size of the count with its separator.
    pub fn get_prefix_size(&self, data: DataPtr) -> i32 {
        let result = self.get_prefix_length(data);
        if result > 0 && self.nd.nd_terminator {
            result + 1
        } else {
            result
        }
    }

    /// The count that is stored before the elements. Bytes that are missing
    /// read as zero, where upstream reads past the end of the data.
    pub fn get_prefix_count(&self, data: DataPtr) -> u32 {
        let read = |size: usize| -> u32 {
            let bytes = data.unwrap_or_default();
            let mut buffer = [0u8; 4];
            let available = bytes.len().min(size);
            buffer[..available].copy_from_slice(&bytes[..available]);
            u32::from_le_bytes(buffer)
        };
        match self.get_count() {
            ARRAY_COUNT_NONE => 0,
            ARRAY_COUNT_COUNTER => read_integer_counter(data) as u32,
            ARRAY_COUNT_MATRIX_COUNTER => {
                let count = read_integer_counter(data);
                count.wrapping_mul(count) as u32
            }
            ARRAY_COUNT_MATRIX_U32 => {
                let count = i64::from(read(4));
                count.wrapping_mul(count) as u32
            }
            _ if data.is_some() => match self.get_prefix_length(data) {
                1 => read(1),
                2 => read(2),
                4 => read(4),
                _ => 0,
            },
            _ => 0,
        }
    }

    /// Upstream `ShouldInclude`.
    pub fn should_include(&self, data: DataPtr, array: ElementArg) -> bool {
        match self.ar_should_include.load().as_deref() {
            None => true,
            Some(callback) => data.is_some() && callback(data, array),
        }
    }

    fn unlocked(self: Arc<Self>) -> Arc<Self> {
        if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        }
    }

    pub fn set_summary_passthrough_max_count(self: Arc<Self>, count: i32) -> Arc<Self> {
        let this = self.unlocked();
        this.ar_summary_passthrough_max_count.store(count, Ordering::Relaxed);
        this
    }

    pub fn set_summary_passthrough_max_length(self: Arc<Self>, length: i32) -> Arc<Self> {
        let this = self.unlocked();
        this.ar_summary_passthrough_max_length.store(length, Ordering::Relaxed);
        this
    }

    pub fn set_summary_passthrough_max_depth(self: Arc<Self>, depth: i32) -> Arc<Self> {
        let this = self.unlocked();
        this.ar_summary_passthrough_max_depth.store(depth, Ordering::Relaxed);
        this
    }

    pub fn set_summary_delimiter(self: Arc<Self>, delimiter: &str) -> Arc<Self> {
        let this = self.unlocked();
        this.ar_summary_delimiter.set(Some(delimiter.to_owned()));
        this
    }

    pub fn set_default_edit_values(self: Arc<Self>, values: &[&str]) -> Arc<Self> {
        let this = self.unlocked();
        this.ar_default_edit_values
            .set(Some(values.iter().map(|&value| value.to_owned()).collect()));
        this
    }

    /// Port of `SetCountPath`: the paths of the elements that hold the count.
    /// With `use_for_count_callback` the first path also gives the count.
    /// Port of `SetCountPath` with one path.
    pub fn set_count_path(self: Arc<Self>, value: &str, use_for_count_callback: bool) -> Arc<Self> {
        self.set_count_paths(&[value], use_for_count_callback)
    }

    /// Port of `SetCountPath` with several paths.
    pub fn set_count_paths(self: Arc<Self>, values: &[&str], use_for_count_callback: bool) -> Arc<Self> {
        let new_count_paths: Vec<String> = values
            .iter()
            .filter(|value| !value.is_empty())
            .map(|&value| value.to_owned())
            .collect();
        let set_callback = use_for_count_callback && !new_count_paths.is_empty();
        if new_count_paths == self.get_count_paths() && !set_callback {
            return self;
        }
        let this = self.unlocked();
        if set_callback {
            this.ar_count_callback
                .set(Some(get_count_callback_for_path(new_count_paths[0].clone())));
        }
        this.ar_count_paths.set(Some(new_count_paths));
        this
    }

    pub fn set_should_include(self: Arc<Self>, callback: Option<ShouldIncludeCallback>) -> Arc<Self> {
        let this = self.unlocked();
        this.ar_should_include.set(callback);
        this
    }

    /// Port of `SetCountFromEnum`: one element per name of the enumeration.
    pub fn set_count_from_enum(self: Arc<Self>, enum_def: Option<Arc<EnumDef>>) -> Arc<Self> {
        let enum_def = enum_def.expect("the enumeration that gives the count");
        let this = self.unlocked();
        let count = enum_def.get_name_count();
        this.ar_count.store(count, Ordering::Relaxed);
        this.ar_labels.set(Some(
            (0..count).map(|index| enum_def.get_name_of(i64::from(index))).collect(),
        ));
        this
    }

    pub fn set_wrongly_assumed_fixed_size_per_element(self: Arc<Self>, size: i32) -> Arc<Self> {
        let this = self.unlocked();
        this.ar_wrongly_assumed_fixed_size_per_element
            .store(size, Ordering::Relaxed);
        this
    }

    /// The container of the array element, when `element` is the array that
    /// this definition describes. Port of the nested `CheckContainer`.
    fn array_container<'a>(&self, element: ElementArg<'a>) -> Option<&'a dyn Container> {
        let element = element?;
        let value_def = element.get_value_def()?;
        if value_def.get_def_id() != self.get_def_id() {
            return None;
        }
        element.as_container()
    }
}

impl Def for ArrayDef {
    value_def_plumbing!(Def);

    fn get_def_type(&self) -> DefType {
        DefType::dtArray
    }

    fn get_def_type_name(&self) -> String {
        let count = self.get_count();
        let separated = if self.nd.nd_terminator { "Separated " } else { "" };
        let prefix = if count < 0 {
            let size = match count {
                ARRAY_COUNT_U32 => 4,
                ARRAY_COUNT_U16 => 2,
                _ => 1,
            };
            format!("{separated}Array with {size} Bytes Counter of ")
        } else if count < 1 && self.ar_count_callback.is_assigned() {
            // Upstream has no blank between this text and the name of the element type.
            format!("{separated}Variable Count Array")
        } else if count > 0 {
            format!("{separated}Array of {count} ")
        } else {
            format!("{separated}Array of ")
        };
        format!("{prefix}{}", self.ar_element.get_def_type_name())
    }

    fn as_array_def(&self) -> Option<&ArrayDef> {
        Some(self)
    }

    fn into_array_def(self: Arc<Self>) -> Option<Arc<ArrayDef>> {
        Some(self)
    }

    fn init_from_parent_do_children(&self) {
        self.ar_element.init_from_parent();
    }
}

impl NamedDef for ArrayDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for ArrayDef {
    value_def_plumbing!(ValueDef);

    fn to_string(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = String::new();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToStr);
        }
        self.used(element, &result);
        result
    }

    fn to_summary(&self, depth: i32, data: DataPtr, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        let mut result = String::new();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToSummary);
        }
        if result.is_empty()
            && let Some(array) = element
            && let Some(cer) = array.as_container()
        {
            let mut element_count = cer.get_element_count();
            if element_count > 0 {
                if cer
                    .get_element(element_count - 1)
                    .is_some_and(|last| last.get_element_type() == ElementType::etStringListTerminator)
                {
                    element_count -= 1;
                }
                let delimiter = self.ar_summary_delimiter.load().as_deref().cloned().unwrap_or_default();
                let mut max_count = self.ar_summary_passthrough_max_count.load(Ordering::Relaxed);
                let mut max_length = self.ar_summary_passthrough_max_length.load(Ordering::Relaxed);
                let max_depth = self.ar_summary_passthrough_max_depth.load(Ordering::Relaxed);
                if element_count == 1 && max_count < 0 {
                    max_count = 1;
                }
                if max_count < 0 && max_length > 0 {
                    max_count = element_count;
                }
                if max_length == 0 {
                    max_count = 0;
                }
                if self.def.def_flags.contains(DefFlag::dfSummaryNoPassthrough)
                    || (max_depth >= 0 && depth >= max_depth)
                {
                    max_count = 0;
                    max_length = 0;
                }
                let mut current_count = 0;
                while element_count > 0 && current_count < max_count {
                    if let Some(child) = cer.get_element(current_count)
                        && child.as_container().is_some()
                        && let Some(dc) = child.as_data_container()
                        && child.get_conflict_priority() > ConflictPriority::cpIgnore
                        && let Some(value_def) = child.get_value_def()
                    {
                        let summary = value_def.to_summary(depth + 1, dc.get_data(), Some(&child), links_to);
                        let s = trim(&summary);
                        if !s.is_empty() {
                            let need_delimiter = !result.is_empty();
                            let mut len = length(&result) + length(s);
                            if need_delimiter {
                                len += length(&delimiter);
                            }
                            if max_length > 0 && len > max_length as usize {
                                break;
                            }
                            if need_delimiter {
                                result.push_str(&delimiter);
                            }
                            result.push_str(s);
                        }
                    }
                    current_count += 1;
                    element_count -= 1;
                }
                if element_count > 0 {
                    let glue = if result.is_empty() {
                        " "
                    } else {
                        result.push_str(&delimiter);
                        " more "
                    };
                    let mut name = if element_count == 1 {
                        self.get_summary_singular_name()
                    } else {
                        self.get_summary_name()
                    };
                    if name.is_empty()
                        && let Some(def) = array.get_def()
                    {
                        name = if element_count == 1 {
                            def.get_summary_singular_name()
                        } else {
                            def.get_summary_name()
                        };
                    }
                    result.push_str(&format!("<{element_count}{glue}{}>", name.to_lowercase()));
                }
            }
        }
        if links_to.is_none()
            && !result.is_empty()
            && let Some(element) = element
        {
            *links_to = element.get_links_to();
        }
        result
    }

    fn get_size(&self, data: DataPtr, element: ElementArg) -> i32 {
        let len = data.map_or(0, <[u8]>::len) as i64;
        let prefix = self.get_prefix_size(data);
        let mut result: i32 = 0;
        let mut count = i64::from(self.get_count());
        let count_callback = self.get_count_callback();
        let should_include = self.ar_should_include.load().as_deref().cloned();
        let wrongly_assumed = self.get_wrongly_assumed_fixed_size_per_element();
        let container = self.array_container(element);
        if count < 0 {
            count = i64::from(self.get_prefix_count(data));
            result = prefix;
        } else {
            if wrongly_assumed > 0 {
                if count == 0 {
                    // UPSTREAM-QUIRK: upstream reads the element count of a container
                    // that it has not looked up yet. The port looks it up.
                    let array_element_count = container.map_or(0, |container| container.get_element_count());
                    count = if array_element_count > 0 {
                        i64::from(array_element_count)
                    } else {
                        len / i64::from(wrongly_assumed)
                    };
                }
                return count.wrapping_mul(i64::from(wrongly_assumed)) as i32;
            }
            if count < 1
                && let Some(callback) = &count_callback
                && container.is_some()
            {
                count = i64::from(callback(data, element));
            }
            if data.is_none() && count < 1 && count_callback.is_none() {
                // EXPERIMENT: Probably should not be done
                count = 1;
            }
            if count < 1 && count_callback.is_none() && should_include.is_none() {
                return i32::MAX;
            }
        }
        debug_assert!(should_include.is_none() || count == 0);
        let mut offset = prefix.max(0) as usize;
        if count > 0 || should_include.is_some() {
            if self.ar_element.get_is_variable_size() || should_include.is_some() {
                let Some(container) = container else {
                    return if data.is_none() {
                        self.ar_element.get_default_size(None, element)
                    } else {
                        i32::MAX
                    };
                };
                let element_count = i64::from(container.get_element_count());
                let mut known_size = count > 0 && element_count == count;
                if known_size {
                    for index in 0..count {
                        let child = container.get_element(index as i32);
                        match child.as_ref().and_then(|child| child.as_data_container()) {
                            Some(dc) => {
                                result = result.wrapping_add(dc.get_data().map_or(0, <[u8]>::len) as i32);
                            }
                            None => {
                                known_size = false;
                                break;
                            }
                        }
                    }
                }
                if !known_size {
                    if data.is_none() || (offset as i64 == len && element_count < 1 && count > 0) {
                        // A variable sized array with a static count of elements and no
                        // existing data to read: all elements have their default size.
                        result =
                            (i64::from(self.ar_element.get_default_size(None, element)).wrapping_mul(count)) as i32;
                    } else {
                        if count == 0 && should_include.is_some() {
                            count = i64::from(i32::MAX);
                        }
                        let mut index: i64 = 0;
                        while count > index && (offset as i64) < len {
                            // An element that does not exist yet falls back to the array.
                            let child = container.get_element(index as i32);
                            let child = child.as_ref().or(element);
                            let rest = data_from(data, offset);
                            if let Some(should_include) = &should_include
                                && !should_include(rest, element)
                            {
                                break;
                            }
                            let size = self.ar_element.get_size(rest, child);
                            if size == i32::MAX {
                                return i32::MAX;
                            }
                            result = result.wrapping_add(size);
                            if data.is_some() && len < i64::from(result) {
                                // UPSTREAM-QUIRK: upstream adds the size to the length
                                // of the data here, where the other branches return
                                // the length.
                                return (len + i64::from(result)) as i32;
                            }
                            offset = offset.saturating_add(size.max(0) as usize);
                            index += 1;
                        }
                    }
                }
            } else {
                let any = container.and_then(|container| container.get_any_element());
                let child = any.as_ref().or(element);
                let size = i64::from(self.ar_element.get_size(data_from(data, offset), child));
                if size >= i64::from(i32::MAX) {
                    return i32::MAX;
                }
                // Sizes above High(Integer) are assumed to come from decoding errors.
                let size = count.wrapping_mul(size).wrapping_add(i64::from(prefix));
                if size >= i64::from(i32::MAX) {
                    return i32::MAX;
                }
                result = size as i32;
                if data.is_some() && len < size {
                    return len as i32;
                }
            }
        }
        result.wrapping_add(i32::from(self.ar_terminated))
    }

    fn get_default_size(&self, data: DataPtr, element: ElementArg) -> i32 {
        let can_be_empty = self.def.def_flags.contains(DefFlag::dfArrayCanBeEmpty);
        if self.get_count() == 0 && !self.ar_count_callback.is_assigned() {
            if can_be_empty {
                0
            } else {
                self.get_wrongly_assumed_fixed_size_per_element()
            }
        } else if can_be_empty {
            self.get_prefix_size(data)
        } else {
            self.get_size(data, element)
        }
    }

    fn get_is_variable_size_internal(&self) -> bool {
        !self.def.def_flags.contains(DefFlag::dfArrayStaticSize)
            && (self.get_count() <= 0 || self.ar_element.get_is_variable_size())
    }

    fn get_can_be_zero_size(&self) -> bool {
        true
    }
}

impl DefKind for ArrayDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::enum_def::EnumClass;
    use super::super::globals::test_lock;
    use super::super::integer::IntegerDef;
    use super::super::string::{StringClass, StringDef};
    use super::super::types::IntType;
    use super::*;

    fn args(name: &str, terminator: bool) -> NamedDefArgs {
        NamedDefArgs {
            priority: ConflictPriority::cpNormal,
            required: false,
            name: name.to_owned(),
            after_load: None,
            after_set: None,
            dont_show: None,
            get_cp: None,
            terminator,
        }
    }

    fn array_of(element: Arc<dyn ValueDef>, count: i32) -> Arc<ArrayDef> {
        ArrayDef::create(
            args("Items", false),
            ArrayDefArgs {
                element,
                count,
                count_callback: None,
                labels: Vec::new(),
                sorted: false,
                can_add_to: true,
                terminated: false,
            },
        )
    }

    fn u16_def() -> Arc<dyn ValueDef> {
        IntegerDef::create(args("Item", false), IntType::itU16, None, 0)
    }

    #[test]
    fn fixed_size_elements() {
        let _guard = test_lock();
        let fixed = array_of(u16_def(), 3);
        assert_eq!(fixed.get_size(Some(&[0; 8]), None), 6);
        assert_eq!(fixed.get_size(None, None), 6);
        // Data that ends early limits the size.
        assert_eq!(fixed.get_size(Some(&[0; 5]), None), 5);
        assert!(!fixed.get_is_variable_size());
        assert_eq!(fixed.get_def_type_name(), "Array of 3 Unsigned Word");
        assert_eq!(fixed.get_element().get_path(), "Items \\ Item");

        let open = array_of(u16_def(), 0);
        assert_eq!(open.get_size(Some(&[0; 8]), None), i32::MAX);
        // Without data upstream assumes one element.
        assert_eq!(open.get_size(None, None), 2);
        assert_eq!(open.get_default_size(None, None), 0);
        assert!(open.get_is_variable_size() && open.get_can_be_zero_size());
        assert_eq!(open.get_def_type_name(), "Array of Unsigned Word");
    }

    #[test]
    fn counted_arrays() {
        let _guard = test_lock();
        let counted = array_of(u16_def(), ARRAY_COUNT_U32);
        assert_eq!(counted.get_prefix_length(None), 4);
        assert_eq!(counted.get_prefix_count(Some(&[2, 0, 0, 0, 9, 9, 9, 9])), 2);
        assert_eq!(counted.get_size(Some(&[2, 0, 0, 0, 9, 9, 9, 9, 7]), None), 8);
        assert_eq!(counted.get_size(Some(&[0, 0, 0, 0, 9]), None), 4);
        assert_eq!(
            counted.get_def_type_name(),
            "Array with 4 Bytes Counter of Unsigned Word"
        );
        let byte_counted = ArrayDef::create(
            args("Items", true),
            ArrayDefArgs {
                element: u16_def(),
                count: ARRAY_COUNT_U8,
                count_callback: None,
                labels: Vec::new(),
                sorted: false,
                can_add_to: true,
                terminated: true,
            },
        );
        // One byte count, its separator, two elements and the terminator.
        assert_eq!(byte_counted.get_prefix_size(None), 2);
        assert_eq!(byte_counted.get_size(Some(&[2, b'|', 1, 0, 2, 0, b'|', 9]), None), 7);
        let matrix = array_of(u16_def(), ARRAY_COUNT_MATRIX_U32);
        assert_eq!(matrix.get_prefix_count(Some(&[3, 0, 0, 0])), 9);
        assert_eq!(array_of(u16_def(), ARRAY_COUNT_NONE).get_prefix_count(Some(&[3])), 0);
        assert_eq!(
            array_of(u16_def(), ARRAY_COUNT_COUNTER).get_prefix_count(Some(&[0b1100])),
            3
        );
    }

    #[test]
    fn variable_size_elements_need_the_array_element() {
        let _guard = test_lock();
        let strings = array_of(StringDef::create(StringClass::String, args("Line", false), 0, false), 2);
        assert!(strings.get_is_variable_size());
        assert_eq!(strings.get_size(Some(b"a\0b\0"), None), i32::MAX);
        assert_eq!(strings.get_size(None, None), 1);
    }

    #[test]
    fn labels_and_setters() {
        let _guard = test_lock();
        let labelled = ArrayDef::create(
            args("Colors", false),
            ArrayDefArgs {
                element: u16_def(),
                count: 2,
                count_callback: None,
                labels: vec!["Red".to_owned(), "Green".to_owned()],
                sorted: false,
                can_add_to: false,
                terminated: false,
            },
        );
        assert_eq!(labelled.get_element_name_suffix(1), "#1 (Green)");
        assert_eq!(labelled.get_element_name_suffix(2), "#2");
        let unnamed = ArrayDef::create(
            args("Colors", false),
            ArrayDefArgs {
                element: IntegerDef::create(args("", false), IntType::itU8, None, 0),
                count: 1,
                count_callback: None,
                labels: vec!["Red".to_owned()],
                sorted: false,
                can_add_to: false,
                terminated: false,
            },
        );
        assert_eq!(unnamed.get_element_name_suffix(0), "Red");

        let enum_def = EnumDef::create(EnumClass::Enum, false, &["A", "B", "C"], &[]);
        let from_enum = array_of(u16_def(), 0).set_count_from_enum(Some(enum_def.clone()));
        assert_eq!(from_enum.get_count(), 3);
        assert_eq!(from_enum.get_element_label(2), "C");

        let with_path = array_of(u16_def(), 0).set_count_paths(&["", "Count"], true);
        assert_eq!(with_path.get_count_paths(), ["Count"]);
        assert!(with_path.get_count_callback().is_some());
        assert_eq!(with_path.get_def_type_name(), "Variable Count ArrayUnsigned Word");
        let same = with_path.clone().set_count_paths(&["Count"], false);
        assert!(Arc::ptr_eq(&same, &with_path));

        let copy = ArrayDef::clone_from(
            &array_of(u16_def(), 2)
                .set_summary_delimiter("; ")
                .set_wrongly_assumed_fixed_size_per_element(4),
        );
        assert_eq!(copy.get_wrongly_assumed_fixed_size_per_element(), 4);
        assert_eq!(copy.get_size(Some(&[0; 3]), None), 8);
        assert!(!Arc::ptr_eq(copy.get_element(), &u16_def()));
        assert!(array_of(u16_def(), 2).should_include(None, None));
    }
}
