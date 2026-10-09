// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas (TwbMainRecord.SetIsDeleted,
// SetIsInitiallyDisabled, GetIsInitiallyDisabled, GetPosition,
// SetPosition)

//! The record edits of the cleaning functions of the main form: a deleted
//! record undeleted (`SetIsDeleted(False)`, which rebuilds it from the
//! version of a master), the initially disabled flag, and the position of a
//! placed record.
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

    /// Port of `TwbMainRecord.SetIsDeleted`. Only the undelete is ported: the
    /// record gets its required members and then the contents of the last
    /// version before it that is in one of its file's masters and is neither
    /// deleted nor a partial form, else of its master when that is neither.
    pub fn set_is_deleted(self: &Arc<Self>, value: bool) -> Result<(), EditError> {
        if value == self.get_is_deleted() {
            return Ok(());
        }
        if value {
            return Err("deleting a record (TwbMainRecord.Delete) is not ported".to_owned());
        }
        self.do_init();
        self.set_modified(true);
        self.invalidate_storage();
        self.container.release_elements();
        self.make_header_writeable(|header| header.flags.set_deleted(false));
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
        Ok(())
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

    /// Port of `TwbMainRecord.SetPosition`.
    pub fn set_record_position(self: &Arc<Self>, position: (f64, f64, f64)) -> bool {
        self.do_init();
        self.set_position(position)
    }
}

impl MainRecordImpl {
    /// The `Assign(wbAssignThis, ...)` of the contained-in element of a
    /// record (`AssignInternal` with index -2) with
    /// `TwbContainedInElement.DoAfterSet`: when the source sits in another
    /// cell (or topic), the record moves to the children group of that
    /// owner in its own file, which is copied into the file when it is not
    /// there yet. Returns whether the record moved.
    ///
    /// Not ported: the move of a cell between worldspaces (an owner group of
    /// type 1), which a record copy does not meet.
    pub(crate) fn assign_contained_in(self: &Arc<Self>, source: &ElementRef) -> bool {
        let Some(file) = self.file_impl() else {
            return false;
        };
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
        let own_owner = self
            .base
            .container()
            .and_then(|container| container.as_element_impl()?.group_record_impl())
            .and_then(|group| owner_of_group(&group));
        let new_form_id = owner.get_load_order_form_id();
        if own_owner.is_some_and(|own| own.get_load_order_form_id() == new_form_id) {
            return false;
        }
        let Some(file_form_id) = file.load_order_form_id_to_file_form_id(new_form_id) else {
            return false;
        };
        let Some(mut new_owner) = file.record_by_form_id(file_form_id, false, true) else {
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
        let Some(old_group) = self
            .base
            .container()
            .and_then(|container| container.as_element_impl()?.group_record_impl())
        else {
            return false;
        };
        let new_group = match new_owner.get_signature().0.as_slice() {
            b"CELL" => {
                let Some(child_group) = new_owner.ensure_cell_child_group() else {
                    return false;
                };
                let flags = self.mr_struct().flags;
                let correct = if flags.is_persistent() {
                    8
                } else if flags.is_visible_when_distant() && !crate::interface::globals::vwd_in_temporary() {
                    10
                } else {
                    9
                };
                child_group
                    .find_child_group(correct, new_owner.mr_struct().form_id.to_cardinal())
                    .unwrap_or_else(|| super::GroupRecordImpl::create_child(&child_group, correct, &new_owner))
            }
            _ => return false,
        };
        let self_ref: ElementRef = self.clone();
        old_group.container.remove_element_by_identity(&self_ref);
        if old_group.container.element_count() == 0 {
            old_group.remove();
        } else {
            old_group.set_modified(true);
        }
        let new_ref: ElementRef = new_group.clone();
        self.base.set_container(&new_ref);
        new_group.container.add_element(self_ref);
        new_group.set_modified(true);
        new_group.sort();
        self.container_changed();
        true
    }
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
