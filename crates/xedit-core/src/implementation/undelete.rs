// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas (TwbMainRecord.SetIsDeleted,
// SetIsPartialForm, MakePartialForm, SetIsInitiallyDisabled,
// GetIsInitiallyDisabled, GetPosition, SetPosition)

//! The record edits of the cleaning functions of the main form and of the
//! copy: a deleted record undeleted (`SetIsDeleted(False)`, which rebuilds
//! it from the version of a master), a partial form made
//! (`MakePartialForm`) or made whole again (`SetIsPartialForm(False)`, the
//! same rebuild), the initially disabled flag, and the position of a placed
//! record.
//!
//! Not ported: `SetIsDeleted(True)` (`TwbMainRecord.Delete`), which no
//! command needs yet.

use std::sync::Arc;

use crate::interface::element::{Container, Element, ElementRef, File, MainRecord};
use crate::interface::misc::{EditError, Variant};
use crate::interface::types::{ASSIGN_THIS, Signature};

use crate::interface::sub_record_group::RecordDef;

use super::{ElementImpl, MainRecordImpl};

/// The placed records that have a position (`GetPosition`, `SetPosition`).
const POSITIONED: [&[u8; 4]; 11] = [
    b"REFR", b"ACRE", b"ACHR", b"PGRE", b"PMIS", b"PARW", b"PBEA", b"PFLA", b"PCON", b"PBAR", b"PHZD",
];

impl MainRecordImpl {
    /// Port of `GetIsInitiallyDisabled`.
    pub fn get_is_initially_disabled(&self) -> bool {
        self.mr_struct().flags.0 & 0x0000_0800 != 0
    }

    /// Port of `SetIsInitiallyDisabled`.
    pub fn set_is_initially_disabled(self: &Arc<Self>, value: bool) {
        if value != self.get_is_initially_disabled() {
            self.make_header_writeable(|header| {
                if value {
                    header.flags.0 |= 0x0000_0800;
                } else {
                    header.flags.0 &= !0x0000_0800;
                }
            });
        }
    }

    /// Port of `TwbMainRecord.SetIsDeleted`: `Delete` for `true`; for
    /// `false` the record gets its required members and then the contents
    /// of the last version before it that is in one of its file's masters
    /// and is neither deleted nor a partial form, else of its master when
    /// that is neither.
    pub fn set_is_deleted(self: &Arc<Self>, value: bool) -> Result<(), EditError> {
        if value == self.get_is_deleted() {
            return Ok(());
        }
        if value {
            self.delete();
            return Ok(());
        }
        self.rebuild_from_masters(|header| header.flags.set_deleted(false));
        Ok(())
    }

    /// Port of `TwbMainRecord.SetIsPartialForm`: a record that can not be
    /// partial is made whole; a partial form is made with `MakePartialForm`,
    /// and a whole record is rebuilt from its masters as the undelete does.
    pub fn set_is_partial_form(self: &Arc<Self>, value: bool) -> Result<(), EditError> {
        let value = value && self.get_can_be_partial();
        if value == self.get_is_partial_form() {
            return Ok(());
        }
        self.set_is_deleted(false)?;
        if value {
            self.make_partial_form();
        } else {
            self.rebuild_from_masters(|header| header.flags.set_partial_form(false));
        }
        Ok(())
    }

    /// Port of `TwbMainRecord.MakePartialForm`: the record keeps its header
    /// (not deleted, partial, not compressed) and its editor ID, nothing
    /// else.
    pub fn make_partial_form(self: &Arc<Self>) {
        if !self.get_can_be_partial() {
            return;
        }
        self.do_init();
        self.begin_update();
        let editor_id = self.get_editor_id();
        self.set_modified(true);
        self.invalidate_storage();
        self.container.release_elements();
        self.make_header_writeable(|header| {
            header.flags.set_deleted(false);
            header.flags.set_partial_form(true);
            header.flags.set_compressed(false);
        });
        self.create_contained_in();
        self.create_record_header();
        if !editor_id.is_empty()
            && let Some(def) = &self.mr_def
        {
            let index = def.get_known_sub_record_member_index(crate::interface::types::KnownSubRecord::ksrEditorID);
            if index >= 0
                && let Some(element) = self.assign(index, None, false)
            {
                // `lBaseRecordElement.EditValue := lEditorID`: upstream
                // raises out of `MakePartialForm`; the editor ID of a record
                // that had one is a valid value.
                let _ = element.set_edit_value(&editor_id);
            }
        }
        self.end_update();
    }

    /// The common part of the undelete and of `SetIsPartialForm(False)`:
    /// the elements released, the header flag changed by `change`, the
    /// required members added, and the contents of the last version before
    /// the record in a master of its file that is neither deleted nor a
    /// partial form, else of its master when that is neither.
    fn rebuild_from_masters(self: &Arc<Self>, change: impl FnOnce(&mut super::structs::MainRecordStruct)) {
        self.do_init();
        self.set_modified(true);
        self.invalidate_storage();
        self.container.release_elements();
        self.make_header_writeable(change);
        self.create_contained_in();
        self.create_record_header();

        self.begin_update();
        if let Some(def) = &self.mr_def {
            for index in 0..usize::try_from(def.get_member_count()).unwrap_or(0) {
                if def.get_member(index).def_base().def_required() {
                    self.assign(index as i32, None, false);
                }
            }
        }
        if let Some(master) = self.master() {
            let overrides = master.overrides();
            let own = overrides.iter().position(|record| Arc::ptr_eq(record, self));
            let file = self.file_impl();
            let masters: Vec<String> = file
                .as_ref()
                .map(|file| {
                    (0..file.get_master_count(true))
                        .filter_map(|index| file.get_master(index, true))
                        .map(|master| master.get_name())
                        .collect()
                })
                .unwrap_or_default();
            let in_masters = |record: &Arc<MainRecordImpl>| {
                let name = record.get_file().map(|file| file.get_name()).unwrap_or_default();
                masters.iter().any(|master| master.eq_ignore_ascii_case(&name))
            };
            let earlier = own.map_or(&overrides[..0], |own| &overrides[..own]);
            let source = earlier
                .iter()
                .rev()
                .find(|record| !(record.get_is_deleted() || record.get_is_partial_form()) && in_masters(record))
                .cloned()
                .or_else(|| (!(master.get_is_deleted() || master.get_is_partial_form())).then_some(master));
            if let Some(source) = source {
                let source: ElementRef = source;
                self.assign(ASSIGN_THIS, Some(&source), false);
            }
        }
        self.end_update();
    }

    /// Port of `TwbMainRecord.GetPosition`: `DATA\Position` of a placed
    /// record, when its `DATA` has the position and the rotation.
    pub fn get_position(self: &Arc<Self>) -> Option<(f64, f64, f64)> {
        let signature = self.get_signature();
        if !POSITIONED.iter().any(|expected| signature == Signature::new(expected)) {
            return None;
        }
        self.do_init();
        let data = self.get_record_by_signature(Signature::new(b"DATA"))?;
        let data = data.as_container().filter(|data| data.get_element_count() == 2)?;
        let position = data.get_element(0)?;
        let values = position
            .as_container()
            .filter(|values| values.get_element_count() == 3)?;
        let axis = |index: i32| match values.get_element(index)?.get_native_value() {
            Variant::Float(value) => Some(value),
            other => other.as_ordinal().map(|value| value as f64),
        };
        Some((axis(0)?, axis(1)?, axis(2)?))
    }

    /// Port of `TwbMainRecord.GetRotation`: `DATA`'s second element, read
    /// as `StrToFloatDef(Value, 0)` like upstream.
    pub fn get_rotation(self: &Arc<Self>) -> Option<(f64, f64, f64)> {
        let signature = self.get_signature();
        if !POSITIONED.iter().any(|expected| signature == Signature::new(expected)) {
            return None;
        }
        self.do_init();
        let data = self.get_record_by_signature(Signature::new(b"DATA"))?;
        let data = data.as_container().filter(|data| data.get_element_count() == 2)?;
        let rotation = data.get_element(1)?;
        let values = rotation
            .as_container()
            .filter(|values| values.get_element_count() == 3)?;
        let axis = |index: i32| {
            let text = values.get_element(index)?.get_value();
            Some(crate::delphi::str_to_float(&text).unwrap_or(0.0))
        };
        Some((axis(0)?, axis(1)?, axis(2)?))
    }

    /// Port of `TwbMainRecord.SetPosition`.
    pub fn set_record_position(self: &Arc<Self>, position: (f64, f64, f64)) -> bool {
        self.do_init();
        self.set_position(position)
    }
}

std::thread_local! {
    /// Port of `cieLockCount`: while a relocation runs, the
    /// `ContainerChanged` of the contained-in elements leaves the group
    /// label alone.
    static CONTAINED_IN_LOCK: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// The guard of `cieLockCount` for one relocation.
struct ContainedInGuard;

impl ContainedInGuard {
    fn enter() -> Self {
        CONTAINED_IN_LOCK.with(|count| count.set(count.get() + 1));
        ContainedInGuard
    }
}

impl Drop for ContainedInGuard {
    fn drop(&mut self) {
        CONTAINED_IN_LOCK.with(|count| count.set(count.get() - 1));
    }
}

impl MainRecordImpl {
    /// Port of `TwbMainRecord.EnsureChildGroup`: the group of the children
    /// of the record, made in the group that holds the record when it is
    /// missing (worldspace 1, cell 6, topic 7, quest children 10).
    pub(crate) fn ensure_child_group(self: &Arc<Self>) -> Option<Arc<super::GroupRecordImpl>> {
        if let Some(group) = self.child_group() {
            return Some(group);
        }
        let wanted = match &self.mr_struct().signature.0 {
            b"WRLD" => 1,
            b"CELL" => 6,
            b"DIAL" => 7,
            b"QUST" if crate::interface::globals::vwd_as_quest_children() => 10,
            _ => 0,
        };
        if wanted == 0 {
            return None;
        }
        let containing = self.base.container()?.as_element_impl()?.group_record_impl()?;
        if let Some(group) = containing.find_child_group(wanted, self.mr_struct().form_id.to_cardinal()) {
            return Some(group);
        }
        if !containing.is_element_editable(None) {
            return None;
        }
        Some(super::GroupRecordImpl::create_child(&containing, wanted, self))
    }

    /// The contained-in element of the record (its `Worldspace`, `Cell`,
    /// `Topic` or `Quest` element).
    fn contained_in_element(self: &Arc<Self>) -> Option<ElementRef> {
        self.container.element_by_sort_order(-2)
    }

    /// The `Assign(wbAssignThis, ...)` of the contained-in element of a
    /// record (`AssignInternal` with index -2): when the source sits in
    /// another cell (or topic), the record moves to the children group of
    /// that owner, which is copied into the file when it is not there yet.
    /// Returns whether the record moved.
    pub(crate) fn assign_contained_in(self: &Arc<Self>, source: &ElementRef) -> bool {
        let Some(owner) = source
            .get_containing_main_record()
            .and_then(|record| record.as_element_impl()?.main_record_impl())
            .and_then(|record| {
                record
                    .base
                    .container()
                    .and_then(|container| container.as_element_impl()?.group_record_impl())
            })
            .and_then(|group| owner_of_group(&group))
        else {
            return false;
        };
        let element = self.contained_in_element();
        let old = element
            .as_ref()
            .and_then(|element| element.get_native_value().as_ordinal())
            .map(|value| value as u32)
            .unwrap_or(0);
        let new = owner.get_load_order_form_id().to_cardinal();
        let moved = relocate_contained_in(self, element.as_ref(), old, new);
        if moved && let Some(element) = &element {
            refresh_contained_in_label(self, element);
        }
        moved
    }
}

/// Port of `TwbContainedInElement.DoAfterSet`: the value of the contained-in
/// element changed from `old` to `new` (load order FormIDs); the record
/// moves to the children group of the record `new` names in its own file,
/// which is copied into the file when it is not there yet, and the element
/// gets the label of the group the record sits in.
pub(crate) fn contained_in_do_after_set(
    record: &Arc<MainRecordImpl>,
    element: &ElementRef,
    old: &crate::interface::misc::Variant,
    new: &crate::interface::misc::Variant,
) {
    let old = old.as_ordinal().map(|value| value as u32).unwrap_or(0);
    let new = new.as_ordinal().map(|value| value as u32).unwrap_or(0);
    if old != new {
        relocate_contained_in(record, Some(element), old, new);
    }
    refresh_contained_in_label(record, element);
}

/// The relocation of `TwbContainedInElement.DoAfterSet`. Returns whether
/// the record moved.
fn relocate_contained_in(record: &Arc<MainRecordImpl>, element: Option<&ElementRef>, old: u32, new: u32) -> bool {
    if old == new {
        return false;
    }
    let lock = ContainedInGuard::enter();
    let Some(file) = record.file_impl() else {
        return false;
    };
    let Some(old_group) = record
        .base
        .container()
        .and_then(|container| container.as_element_impl()?.group_record_impl())
    else {
        return false;
    };
    // `_File.RecordByFormID[NewFormID, False, True]`.
    let Some(mut new_owner) = file
        .load_order_form_id_to_file_form_id(crate::interface::form_id::FormID::from_cardinal(new))
        .and_then(|form_id| file.record_by_form_id(form_id, false, true))
    else {
        // `PCardinal(dcDataBasePtr)^ := OldFormID.ToCardinal`: the edit is
        // taken back.
        if let Some(element) = element {
            set_contained_in_label(element, old);
        }
        return false;
    };
    if !new_owner
        .file_impl()
        .is_some_and(|owner_file| Arc::ptr_eq(&owner_file, &file))
    {
        let args = crate::interface::element::CopyArgs {
            as_new: false,
            deep_copy: true,
            prefix_remove: String::new(),
            suffix_remove: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            allow_overwrite: false,
        };
        let owner_ref: ElementRef = new_owner.clone();
        let Ok(Some(copy)) = super::copy::copy_element_to_file(&owner_ref, &file, &args) else {
            return false;
        };
        let Some(copy) = copy.as_element_impl().and_then(super::ElementImpl::main_record_impl) else {
            return false;
        };
        new_owner = copy;
    }
    let Some(new_group) = new_owner.ensure_child_group() else {
        return false;
    };
    let self_ref: ElementRef = record.clone();
    let mut group_record = new_group.clone();
    match new_group.group_type() {
        1 => {
            // `Assert(OldGroup.GroupType in [1, 5])`.
            if !matches!(old_group.group_type(), 1 | 5) {
                return false;
            }
            let group1 = old_group.clone();
            let mut old_block: Option<Arc<super::GroupRecordImpl>> = None;
            let mut old_world: Option<Arc<super::GroupRecordImpl>> = None;
            if group1.group_type() == 5 {
                let Some(parent) = group1.parent_group() else {
                    return false;
                };
                old_block = Some(parent.clone());
                // The block group of the new worldspace with the label of
                // the old one, made when it is missing.
                let new_block = group_record
                    .container
                    .elements()
                    .iter()
                    .find_map(|element| {
                        let group = element.as_element_impl()?.group_record_impl()?;
                        (group.group_type() == 4 && group.get_group_label() == parent.get_group_label())
                            .then_some(group)
                    })
                    .unwrap_or_else(|| {
                        super::GroupRecordImpl::create_labelled(&group_record, 4, parent.get_group_label())
                    });
                group_record = new_block;
                // The sub-block group with the label of the record's group.
                let new_sub_block = group_record
                    .container
                    .elements()
                    .iter()
                    .find_map(|element| {
                        let group = element.as_element_impl()?.group_record_impl()?;
                        (group.group_type() == 5 && group.get_group_label() == group1.get_group_label())
                            .then_some(group)
                    })
                    .unwrap_or_else(|| {
                        super::GroupRecordImpl::create_labelled(&group_record, 5, group1.get_group_label())
                    });
                group_record = new_sub_block;
                // `Group3` of the removal chain: the world children group
                // the old block hangs under.
                old_world = parent.parent_group();
                if !old_world.as_ref().is_some_and(|group| group.group_type() == 1) {
                    return false;
                }
            }
            group1.container.remove_element_by_identity(&self_ref);
            if group1.container.element_count() == 0 {
                group1.set_modified(true);
                group1.remove();
                if let Some(block) = &old_block {
                    if block.container.element_count() == 0 {
                        block.set_modified(true);
                        block.remove();
                        if let Some(world) = &old_world {
                            if world.container.element_count() == 0 {
                                world.set_modified(true);
                                world.remove();
                            } else {
                                world.set_modified(true);
                            }
                        }
                    } else {
                        block.set_modified(true);
                    }
                }
            } else {
                group1.set_modified(true);
            }
            let group_ref: ElementRef = group_record.clone();
            record.base.set_container(&group_ref);
            group_record.container.add_element(self_ref);
            group_record.set_modified(true);
            group_record.sort();
        }
        6 => {
            let flags = record.mr_struct().flags;
            let correct = if flags.is_persistent() {
                8
            } else if flags.is_visible_when_distant() && !crate::interface::globals::vwd_in_temporary() {
                10
            } else {
                9
            };
            let group = new_group
                .find_child_group(correct, new_owner.mr_struct().form_id.to_cardinal())
                .unwrap_or_else(|| super::GroupRecordImpl::create_child(&new_group, correct, &new_owner));
            old_group.container.remove_element_by_identity(&self_ref);
            if old_group.container.element_count() == 0 {
                old_group.remove();
            } else {
                old_group.set_modified(true);
            }
            let group_ref: ElementRef = group.clone();
            record.base.set_container(&group_ref);
            group.container.add_element(self_ref);
            group.set_modified(true);
            group.sort();
        }
        7 => {
            old_group.container.remove_element_by_identity(&self_ref);
            if old_group.container.element_count() == 0 {
                old_group.remove();
            } else {
                old_group.set_modified(true);
            }
            let group_ref: ElementRef = new_group.clone();
            record.base.set_container(&group_ref);
            new_group.container.add_element(self_ref);
            new_group.set_modified(true);
            new_group.sort();
        }
        10 if crate::interface::globals::game_mode() >= crate::interface::globals::GameMode::gmFO4 => {
            old_group.container.remove_element_by_identity(&self_ref);
            if old_group.container.element_count() == 0 {
                old_group.remove();
            } else {
                old_group.set_modified(true);
            }
            let group_ref: ElementRef = new_group.clone();
            record.base.set_container(&group_ref);
            new_group.container.add_element(self_ref);
            new_group.set_modified(true);
            new_group.sort();
        }
        // Upstream asserts here; a record with no such owner group stays
        // where it is.
        _ => return false,
    }
    drop(lock);
    record.container_changed();
    true
}

/// Writes a label into the data of a contained-in element.
fn set_contained_in_label(element: &ElementRef, label: u32) {
    if let Some(value) = element.as_element_impl().and_then(|element| element.value_impl()) {
        value.replace_data(label.to_le_bytes().to_vec());
    }
}

/// Port of `TwbContainedInElement.ContainerChanged`: the data of the
/// element becomes the label of the group the record sits in (walking up
/// the sub-block, block and children groups).
fn refresh_contained_in_label(record: &Arc<MainRecordImpl>, element: &ElementRef) {
    if CONTAINED_IN_LOCK.with(|count| count.get()) > 0 {
        return;
    }
    let Some(mut group) = record
        .base
        .container()
        .and_then(|container| container.as_element_impl()?.group_record_impl())
    else {
        return;
    };
    for expected in [5, 4] {
        if group.group_type() == expected {
            match group.parent_group() {
                Some(parent) => group = parent,
                None => return,
            }
        }
    }
    let children = if crate::interface::globals::vwd_as_quest_children() {
        8..=9
    } else {
        8..=10
    };
    if children.contains(&group.group_type()) {
        match group.parent_group() {
            Some(parent) => group = parent,
            None => return,
        }
    }
    let parented: &[i32] = if crate::interface::globals::vwd_as_quest_children() {
        &[1, 6, 7, 10]
    } else {
        &[1, 6, 7]
    };
    if !parented.contains(&group.group_type()) {
        return;
    }
    set_contained_in_label(element, group.group_label());
}

/// The record a group holds the children of, walking up from the groups
/// below a cell's children group, as `TwbContainedInElement.Create` does.
fn owner_of_group(group: &Arc<super::GroupRecordImpl>) -> Option<Arc<MainRecordImpl>> {
    let mut group = group.clone();
    for group_type in [5, 4] {
        if group.group_type() == group_type {
            group = group.parent_group()?;
        }
    }
    let children = if crate::interface::globals::vwd_as_quest_children() {
        8..=9
    } else {
        8..=10
    };
    if children.contains(&group.group_type()) {
        group = group.parent_group()?;
    }
    group.children_of()
}
