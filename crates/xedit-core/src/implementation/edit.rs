// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of the editing path of `wbImplementation.pas`: the storage a changed
//! element owns (`dcDataStorage`, `RequestStorageChange`,
//! `UpdateStorageFromElements`), the update counter with its deferred
//! notifications (`BeginUpdate`, `EndUpdate`, `NotifyChanged`, `DoAfterSet`),
//! `SetEditValue`, `SetNativeValue` and `SetToDefault`, the elements made
//! from their definitions alone (`TwbSubRecord.Create` with a definition,
//! `TwbValueBase.Create` without a source, `AddRequiredElements`), and
//! `Add`, `Assign` of a missing member and `Remove`.
//!
//! State: every value definition type writes its edit and native values;
//! `Assign` only creates missing members and adds array entries from
//! nothing (the copy from another element is the next step); sorted arrays
//! are not sorted after a change (sort keys are not ported).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::interface::def::ValueDef;
use crate::interface::element::ElementRef;
use crate::interface::globals::{edit_allowed, is_internal_edit, sort_sub_records};
use crate::interface::misc::{EditError, Variant};
use crate::interface::types::{DefFlag, DefType, ElementType, PascalEnum};

use super::write::ElementState;
use super::{DataBlock, ElementImpl};

/// Port of `dcDataStorage` with `dcfStorageInvalid`: the bytes an element
/// owns once it was changed, and whether they are stale because a child
/// changed.
///
/// Every change makes a new buffer. The old buffers stay until the element
/// is dropped, because `data` hands out slices of the current buffer for
/// the life of the element and nothing tracks those borrows.
#[derive(Default)]
pub struct Storage {
    generations: Mutex<Vec<Arc<Vec<u8>>>>,
    /// Whether the last generation is the data of the element. Cleared by
    /// `detach`, which is upstream `dcDataStorage := nil` with the data
    /// pointers set to `nil`: the element has no data until the next
    /// `RequestStorageChange`.
    present: AtomicBool,
    /// Upstream `dcDataBasePtr := nil`: the element has no data at all, not
    /// even the bytes it was loaded from.
    detached: AtomicBool,
    /// Upstream `dcfStorageInvalid`: a child changed, so the bytes are
    /// rebuilt from the elements on the next read.
    invalid: AtomicBool,
}

impl Storage {
    /// The current bytes, when the element owns its data.
    pub fn current(&self) -> Option<&[u8]> {
        if !self.present.load(Ordering::Acquire) {
            return None;
        }
        let generations = self.generations.lock().unwrap();
        let last = generations.last()?;
        let (ptr, len) = (last.as_ptr(), last.len());
        // SAFETY: a buffer pushed into `generations` is never removed,
        // replaced or mutated for the life of this `Storage`, and the heap
        // allocation of a `Vec<u8>` does not move when the outer vector
        // grows, so the slice stays valid for as long as `self` does.
        Some(unsafe { std::slice::from_raw_parts(ptr, len) })
    }

    /// The current bytes as a block the children can read from.
    pub fn current_block(&self) -> Option<DataBlock> {
        if !self.present.load(Ordering::Acquire) {
            return None;
        }
        let generations = self.generations.lock().unwrap();
        generations.last().map(|last| DataBlock::Buffer(last.clone()))
    }

    pub fn has_storage(&self) -> bool {
        self.present.load(Ordering::Acquire)
    }

    /// Makes `bytes` the data of the element.
    pub fn set(&self, bytes: Vec<u8>) {
        self.generations.lock().unwrap().push(Arc::new(bytes));
        self.present.store(true, Ordering::Release);
        self.detached.store(false, Ordering::Release);
    }

    /// Upstream `dcDataStorage := nil; dcDataBasePtr := nil`: the element
    /// has no data until it requests storage again.
    pub fn detach(&self) {
        self.present.store(false, Ordering::Release);
        self.detached.store(true, Ordering::Release);
    }

    pub fn is_detached(&self) -> bool {
        self.detached.load(Ordering::Acquire)
    }

    pub fn is_invalid(&self) -> bool {
        self.invalid.load(Ordering::Acquire)
    }

    pub fn set_invalid(&self, invalid: bool) {
        self.invalid.store(invalid, Ordering::Release);
    }
}

/// The check at the top of every setter: `if not wbIsInternalEdit then if
/// not wbEditAllowed then raise`.
pub(crate) fn check_edit_allowed(element: &dyn ElementImpl) -> Result<(), EditError> {
    if !is_internal_edit() && !edit_allowed() {
        return Err(format!("{} can not be edited.", element.get_name()));
    }
    Ok(())
}

// ----- the update counter -----

/// Port of `TwbElement.BeginUpdate`.
pub(crate) fn begin_update(element: &dyn ElementImpl) {
    element.element_base().e_update_count.fetch_add(1, Ordering::AcqRel);
}

/// Port of `TwbElement.EndUpdate`: the last `EndUpdate` runs the deferred
/// notifications.
pub(crate) fn end_update(element: &dyn ElementImpl) {
    let count = element.element_base().e_update_count.fetch_sub(1, Ordering::AcqRel) - 1;
    if count == 0 {
        element.update_ended();
    }
}

pub(crate) fn update_count(element: &dyn ElementImpl) -> i32 {
    element.element_base().e_update_count.load(Ordering::Acquire)
}

/// Port of `TwbElement.UpdateEnded`.
pub(crate) fn update_ended(element: &dyn ElementImpl) {
    let base = element.element_base();
    while base.has_state(ElementState::esChangeNotified) {
        base.exclude_state(ElementState::esChangeNotified);
        element.notify_changed_internal();
    }
    if base.has_state(ElementState::esModifiedUpdated) {
        base.exclude_state(ElementState::esModifiedUpdated);
        if base.has_state(ElementState::esModified) {
            if base.has_state(ElementState::esInternalModified) {
                let internal = crate::interface::globals::begin_internal_edit(true);
                element.set_parent_modified();
                if internal {
                    crate::interface::globals::end_internal_edit();
                }
            } else {
                element.set_parent_modified();
            }
        }
    }
}

/// Port of `TwbElement.NotifyChanged` and the `csInitializing` check of
/// `TwbContainer.NotifyChanged`.
pub(crate) fn notify_changed(element: &dyn ElementImpl) {
    if element.init_running() {
        return;
    }
    if update_count(element) > 0 {
        element.element_base().include_state(ElementState::esChangeNotified);
    } else {
        element.notify_changed_internal();
    }
}

/// Port of `TwbContainer.NotifyChangedInternal` over
/// `TwbElement.NotifyChangedInternal`: the container is told, and a
/// modified element runs its `AfterSet` callbacks.
pub(crate) fn notify_changed_internal(element: &dyn ElementImpl) {
    if element.init_running() {
        return;
    }
    if let Some(base) = element.container_base() {
        base.cnt_as_created_empty.store(false, Ordering::Relaxed);
    }
    if let (Some(this), Some(container)) = (element.as_dyn_element_impl(), element.element_base().container())
        && let Some(container) = container.as_element_impl()
    {
        container.element_changed(&this);
    }
    if element.element_base().has_state(ElementState::esModified) {
        element.do_after_set(&Variant::Empty, &Variant::Empty);
    }
}

/// Port of `TwbElement.DoAfterSet`: the `AfterSet` callbacks of the value
/// definition and of the definition when it differs.
pub(crate) fn do_after_set(element: &dyn ElementImpl, old: &Variant, new: &Variant) {
    let Some(this) = element.as_dyn_element_impl() else {
        return;
    };
    let value_def = element.get_value_def();
    if let Some(value_def) = &value_def {
        value_def.after_set(&this, old, new);
    }
    if let Some(def) = element.get_def() {
        let same = value_def
            .as_ref()
            .is_some_and(|value_def| std::ptr::addr_eq(Arc::as_ptr(value_def), Arc::as_ptr(&def)));
        if !same {
            def.after_set(&this, old, new);
        }
    }
}

// ----- the storage -----

/// Port of `TwbElement.InvalidateParentStorage`.
pub(crate) fn invalidate_parent_storage(element: &dyn ElementImpl) {
    if let Some(container) = element.element_base().container()
        && let Some(container) = container.as_element_impl()
    {
        container.invalidate_storage();
    }
}

/// Port of `TwbDataContainer.InvalidateStorage` over
/// `TwbElement.InvalidateStorage`: the storage is stale, and so is the
/// parent's.
pub(crate) fn invalidate_storage(element: &dyn ElementImpl) {
    if let Some(storage) = element.storage() {
        storage.set_invalid(true);
    }
    invalidate_parent_storage(element);
}

/// Port of `TwbDataContainer.UpdateStorageFromElements`: the data of an
/// element whose storage is stale is built again from the data of its
/// elements, after the prefix it keeps.
pub(crate) fn update_storage_from_elements(element: &dyn ElementImpl) {
    let Some(storage) = element.storage() else { return };
    if !storage.is_invalid() {
        return;
    }
    let children = element.container_base().map(|base| base.elements()).unwrap_or_default();
    for child in &children {
        if let Some(child) = child.as_element_impl() {
            child.update_storage_from_elements();
        }
    }
    let prefix = element.get_data_prefix_size();
    let mut new_storage = Vec::new();
    if prefix > 0
        && let Some(data) = element.raw_data()
    {
        new_storage.extend_from_slice(&data[..prefix.min(data.len())]);
    }
    new_storage.resize(prefix, 0);
    for child in &children {
        let Some(child_impl) = child.as_element_impl() else {
            continue;
        };
        if child_impl.dont_save() {
            continue;
        }
        if child.get_element_type() == ElementType::etStringListTerminator {
            new_storage.push(0);
            continue;
        }
        if let Some(data) = child.as_data_container().and_then(|data| data.get_data()) {
            new_storage.extend_from_slice(data);
        }
    }
    storage.set(new_storage);
    storage.set_invalid(false);
}

/// Port of `TwbContainer.GetDataSizeFromElements`.
pub(crate) fn data_size_from_elements(element: &dyn ElementImpl) -> i32 {
    if let Some(container) = element.as_container() {
        container.get_element_count();
    }
    if let Some(array_def) = element
        .get_value_def()
        .and_then(|def| def.as_array_def().map(|_| def.clone()))
        && let Some(array_def) = array_def.as_array_def()
    {
        let per_element = array_def.get_wrongly_assumed_fixed_size_per_element();
        if per_element > 0 {
            return element.as_container().map_or(0, |c| c.get_element_count()) * per_element;
        }
    }
    element.container_base().map_or(0, |base| {
        base.elements()
            .iter()
            .filter(|child| !child.as_element_impl().is_some_and(ElementImpl::dont_save))
            .map(|child| child.get_data_size())
            .sum()
    })
}

/// Port of `TwbDataContainer.RequestStorageChange`: the element takes
/// ownership of its data, is marked modified, and the parent's storage is
/// stale. Returns the data as `new_size` bytes for the caller to fill and
/// commit.
pub(crate) fn request_storage_change(element: &dyn ElementImpl, new_size: usize) -> Option<Vec<u8>> {
    let storage = element.storage()?;
    if storage.is_invalid() {
        element.update_storage_from_elements();
    }
    element.set_modified(true);
    invalidate_parent_storage(element);
    let mut bytes = element.raw_data().map(<[u8]>::to_vec).unwrap_or_default();
    bytes.resize(new_size, 0);
    storage.set_invalid(false);
    Some(bytes)
}

// ----- the setters -----

/// Port of `TwbValueBase.SetEditValue` (and of `TwbValue.SetEditValue`,
/// which also edits an element without data).
pub(crate) fn set_edit_value(element: &dyn ElementImpl, value: &str, even_without_data: bool) -> Result<(), EditError> {
    check_edit_allowed(element)?;
    let Some(this) = element.as_dyn_element_impl() else {
        return Err(format!("{} can not be edited.", element.get_name()));
    };
    let Some(value_def) = element.get_value_def() else {
        return Err(format!("{} can not be edited", element.get_name()));
    };
    begin_update(element);
    let result = (|| {
        let no_data = even_without_data && element.raw_data().is_none();
        if no_data || value != element.get_edit_value() {
            let old = element.get_native_value();
            value_def.from_edit_value(element.current_data(), Some(&this), value)?;
            element.set_modified(true);
            element.after_value_changed();
            let new = element.get_native_value();
            element.do_after_set(&old, &new);
            element.notify_changed();
        }
        Ok(())
    })();
    end_update(element);
    result
}

/// Port of `TwbValueBase.SetNativeValue`.
pub(crate) fn set_native_value(element: &dyn ElementImpl, value: Variant) -> Result<(), EditError> {
    check_edit_allowed(element)?;
    let Some(this) = element.as_dyn_element_impl() else {
        return Err(format!("{} can not be edited.", element.get_name()));
    };
    let Some(value_def) = element.get_value_def() else {
        return Err(format!("{} can not be edited", element.get_name()));
    };
    begin_update(element);
    let result = (|| {
        let old = element.get_native_value();
        value_def.from_native_value(element.current_data(), Some(&this), value)?;
        element.set_modified(true);
        element.after_value_changed();
        let new = element.get_native_value();
        element.do_after_set(&old, &new);
        element.notify_changed();
        Ok(())
    })();
    end_update(element);
    result
}

/// Port of `TwbDataContainer.CopyFrom`: the data of the element becomes a
/// copy of `bytes`.
pub(crate) fn copy_from(element: &dyn ElementImpl, bytes: &[u8]) -> Result<(), EditError> {
    if !is_internal_edit() && !edit_allowed() {
        return Err(format!("{} can not be edited.", element.get_name()));
    }
    if let Some(container) = element.as_container() {
        container.get_element_count();
    }
    begin_update(element);
    let result = (|| {
        if element.get_value_def().is_none() {
            return Err(format!("{} can not be edited", element.get_name()));
        }
        let old = element.get_native_value();
        if let Some(mut storage) = element.request_storage_change_impl(bytes.len()) {
            storage.copy_from_slice(bytes);
            element.commit_storage_impl(storage);
        }
        element.set_modified(true);
        let new = element.get_native_value();
        element.do_after_set(&old, &new);
        element.notify_changed();
        Ok(())
    })();
    end_update(element);
    result
}

/// Port of `TwbElement.SetToDefault`.
pub(crate) fn set_to_default(element: &dyn ElementImpl) -> Result<(), EditError> {
    begin_update(element);
    let result = element.set_to_default_internal();
    end_update(element);
    result
}

/// Port of `TwbDataContainer.SetToDefaultInternal` over
/// `TwbContainer.SetToDefaultInternal`: the value definition writes its
/// default, then every element takes its default.
pub(crate) fn data_container_set_to_default_internal(element: &dyn ElementImpl) -> Result<(), EditError> {
    if let Some(container) = element.as_container() {
        container.get_element_count();
    }
    if let Some(value_def) = element.get_value_def()
        && let Some(this) = element.as_dyn_element_impl()
    {
        let old = element.get_native_value();
        if value_def.set_to_default(element.current_data(), Some(&this))? {
            element.set_modified(true);
            let new = element.get_native_value();
            element.do_after_set(&old, &new);
            element.notify_changed();
            if element.is_flags() {
                element.reset_and_init();
            }
        }
    }
    container_set_to_default_internal(element)
}

/// Port of `TwbContainer.SetToDefaultInternal`.
pub(crate) fn container_set_to_default_internal(element: &dyn ElementImpl) -> Result<(), EditError> {
    if let Some(container) = element.as_container() {
        container.get_element_count();
    }
    if let Some(base) = element.container_base() {
        for child in base.elements() {
            child.set_to_default()?;
        }
        base.cnt_as_created_empty.store(false, Ordering::Relaxed);
    }
    Ok(())
}

/// Port of `TwbValueBase.SetToDefaultInternal` and
/// `TwbSubRecord.SetToDefaultInternal`: the elements are dropped, the data
/// is replaced by zeros of the default size, the elements are built again
/// over it and take their defaults.
pub(crate) fn value_set_to_default_internal(element: &dyn ElementImpl) -> Result<(), EditError> {
    let Some(storage) = element.storage() else {
        return Ok(());
    };
    let Some(this) = element.as_dyn_element_impl() else {
        return Ok(());
    };
    element.release_and_detach();
    storage.detach();
    if let Some(container) = element.as_container() {
        container.get_element_count();
    }
    let default_size = element
        .get_value_def()
        .map_or(0, |value_def| value_def.get_default_size(None, Some(&this)));
    let default_size = usize::try_from(default_size).unwrap_or(0);
    if let Some(bytes) = element.request_storage_change_impl(default_size) {
        element.commit_storage_impl(bytes);
        // `inherited InformStorage`: the elements read from the new data.
        element.reset_and_init();
    }
    data_container_set_to_default_internal(element)?;
    element.after_set_to_default()
}

/// Port of `TwbElement.Remove`: the element leaves its container, which is
/// modified by that.
pub(crate) fn remove(element: &dyn ElementImpl) {
    let Some(this) = element.as_dyn_element_impl() else {
        return;
    };
    let Some(container) = element.element_base().container() else {
        return;
    };
    let Some(container_impl) = container.as_element_impl() else {
        return;
    };
    // `TwbMainRecord.Remove` first takes the record out of its file.
    if let Some(record) = element.main_record_impl()
        && let Err(error) = record.remove_from_file()
    {
        crate::interface::misc::progress(&error);
    }
    begin_update(container_impl);
    element.set_modified(true);
    invalidate_parent_storage(element);
    element.before_actual_remove();
    container_impl.remove_child(&this, true);
    end_update(container_impl);
}

/// Port of `TwbContainer.RemoveElement(aElement, aMarkModified)`.
pub(crate) fn container_remove_child(
    container: &dyn ElementImpl,
    child: &ElementRef,
    mark_modified: bool,
) -> Option<ElementRef> {
    let base = container.container_base()?;
    if mark_modified {
        container.set_modified(true);
        container.invalidate_storage();
    }
    let removed = base.remove_element_by_identity(child)?;
    container.notify_changed();
    Some(removed)
}

/// Port of `CompareSubRecords` as a sort of a main record's elements: by
/// sort order, then element type; the file order is kept beyond that.
pub(crate) fn sort_sub_records_of(container: &dyn ElementImpl) {
    if !sort_sub_records() {
        return;
    }
    let Some(base) = container.container_base() else { return };
    if base.element_count() < 2 {
        return;
    }
    base.sort_by(|a, b| {
        let key = |element: &ElementRef| (element.get_sort_order(), element.get_element_type().ord());
        key(a).cmp(&key(b))
    });
}

/// Port of `TwbArrayDef`'s `UpdateCountViaPath` as `TwbSubRecord` and
/// `TwbArray` run it: the counters named by the array definition take the
/// element count, created along the path when the count is not zero.
pub(crate) fn update_count_via_path(element: &dyn ElementImpl, value_def: Option<&Arc<dyn ValueDef>>) {
    let Some(array_def) = value_def.and_then(|def| def.as_array_def()) else {
        return;
    };
    update_count_via_paths(element, array_def.get_count_paths());
}

/// The body of `UpdateCountViaPath` for the count paths of any array
/// definition.
pub(crate) fn update_count_via_paths(element: &dyn ElementImpl, paths: Vec<String>) {
    let Some(container) = element.element_base().container() else {
        return;
    };
    let Some(container) = container.as_container() else {
        return;
    };
    let count = element.as_container().map_or(0, |c| c.get_element_count());
    for path in paths {
        if path.is_empty() {
            continue;
        }
        if count > 0 {
            let _ = container.set_element_native_value(&path, Variant::Int(i64::from(count)));
            continue;
        }
        if let Some(counter) = container.get_element_by_path(&path) {
            let _ = counter.set_native_value(Variant::Int(i64::from(count)));
        }
    }
}

/// Port of the `BeforeActualRemove` of the arrays: the counters along the
/// count paths go to zero, when they exist.
pub(crate) fn zero_count_paths(element: &dyn ElementImpl, paths: Vec<String>) {
    let Some(container) = element.element_base().container() else {
        return;
    };
    let Some(container) = container.as_container() else {
        return;
    };
    for path in paths {
        if path.is_empty() {
            continue;
        }
        if let Some(counter) = container.get_element_by_path(&path) {
            let _ = counter.set_native_value(Variant::Int(0));
        }
    }
}

/// Port of `CheckCount` of `TwbSubRecord` and `TwbArray`: the count prefix
/// of an array follows the number of elements.
pub(crate) fn check_count(element: &dyn ElementImpl, value_def: Option<&Arc<dyn ValueDef>>) {
    if update_count(element) > 0 {
        return;
    }
    let prefix = element.get_data_prefix_size();
    if prefix == 0 {
        return;
    }
    let Some(array_def) = value_def.and_then(|def| def.as_array_def()) else {
        return;
    };
    let count = array_def.get_prefix_count(element.current_data());
    let elements = element.as_container().map_or(0, |c| c.get_element_count());
    if count != elements as u32 {
        element.set_modified(true);
        element.invalidate_storage();
        element.update_storage_from_elements();
        let size = element.current_data().map_or(0, <[u8]>::len);
        if let Some(mut bytes) = element.request_storage_change_impl(size) {
            array_def.set_prefix_count(&mut bytes, elements as u32);
            element.commit_storage_impl(bytes);
        }
    }
}

/// Port of `CheckTerminator`: an array of variable size strings ends with
/// a terminator element.
pub(crate) fn check_terminator(element: &dyn ElementImpl, value_def: Option<&Arc<dyn ValueDef>>) {
    if update_count(element) > 0 {
        return;
    }
    let Some(value_def) = value_def else { return };
    let Some(array_def) = value_def.as_array_def() else {
        return;
    };
    if !value_def.get_is_variable_size() {
        return;
    }
    let element_def = array_def.get_element();
    if element_def.get_def_type() != DefType::dtString {
        return;
    }
    if element_def
        .as_string_def()
        .is_none_or(|string_def| string_def.get_string_size() > 0)
    {
        return;
    }
    let has_terminator = element.container_base().is_some_and(|base| {
        base.elements()
            .iter()
            .any(|child| child.get_element_type() == ElementType::etStringListTerminator)
    });
    if has_terminator {
        return;
    }
    element.set_modified(true);
    element.invalidate_storage();
    element.add_string_list_terminator();
}

/// Whether the definition carries `dfDontSave`.
pub(crate) fn def_dont_save(element: &dyn ElementImpl) -> bool {
    element
        .get_def()
        .is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfDontSave))
}
