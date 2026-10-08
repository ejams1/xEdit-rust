// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of the FormID change of a main record: `TwbMainRecord.SetLoadOrderFormID`
//! with `UpdateInteriorCellGroup`, `GetIsInjected`, `ContainerChanged`, the
//! registration part of `TwbFile.AddMainRecord` (overrides, injected records,
//! the light, medium and update checks), `flRemoveKeysFromIndices`, and
//! `TwbGroupRecord.SetGroupLabel`. `SortRecords` is in `mod.rs`;
//! `RemoveMainRecord`, `RemoveOverride`, `YouAreTheMaster`, `YouGotAMaster`
//! and the groups `TwbGroupRecord.Create` makes are in `add.rs`, `NewFormID`
//! in `new_form_id.rs`.
//!
//! State: the referenced-by lists are not kept (the reference index is phase
//! 4), so `YouAreTheMaster` hands over only the overrides; the callers that
//! update the references find the referencing records with `refs`.

use std::sync::Arc;

use crate::interface::element::{Container, Element, ElementRef, File, MainRecord};
use crate::interface::form_id::{FileID, FormID};
use crate::interface::globals::{GameMode, game_mode};
use crate::interface::misc::progress;
use crate::interface::types::{FileState, Signature};

use super::{ElementImpl, FileImpl, GroupRecordImpl, MainRecordImpl, edit};

impl FileImpl {
    /// Port of `ContainedRecordByLoadOrderFormID` with `aAllowInjected`: the
    /// record of this file with the load order FormID, or the record
    /// injected into it.
    pub fn contained_record_by_load_order_form_id_injected(&self, form_id: FormID) -> Option<Arc<MainRecordImpl>> {
        let file_form_id = self.load_order_form_id_to_file_form_id(form_id)?;
        self.find_form_id(file_form_id).or_else(|| {
            if self.is_new_record(file_form_id.file_id()) {
                self.find_injected_id(file_form_id)
            } else {
                None
            }
        })
    }

    /// Port of `flRemoveKeysFromIndices`: a key leaves an index when the
    /// index holds this record for it.
    pub(crate) fn remove_keys_from_indices(&self, record: &Arc<MainRecordImpl>, keys: &[(i32, String)]) {
        let mut indices = self.fl_records_indices.write().unwrap();
        for (index, key) in keys {
            let Some(records) = usize::try_from(*index).ok().and_then(|index| indices.get_mut(index)) else {
                return;
            };
            let key = crate::interface::main_record::named_index_key(*index, key);
            if records.get(&key).is_some_and(|existing| Arc::ptr_eq(existing, record)) {
                records.remove(&key);
            }
        }
    }

    /// The registration of a record in `AddMainRecord` after the FormID
    /// index: the record becomes the override of the record of a master
    /// with its FormID or is injected into the master its FormID points to.
    /// A new record of the file is checked against the light, medium and
    /// update flags.
    pub(crate) fn register_main_record(self: &Arc<Self>, record: &Arc<MainRecordImpl>, form_id: FormID) {
        let file_id = form_id.file_id();
        let states = self.get_file_states();
        let hardcoded_elsewhere = form_id.is_hardcoded() && !states.contains(FileState::fsIsGameMaster);
        if self.is_new_record(file_id) && !states.contains(FileState::fsIsCompareLoad) && !hardcoded_elsewhere {
            self.check_new_record_compatibility(record, form_id);
            return;
        }
        if let Some(master) = self.get_master_record_by_form_id(form_id, true, true) {
            master.add_override(record);
        } else if hardcoded_elsewhere {
            if let Some(game_master) = super::game_master_file() {
                game_master.inject_main_record(record.clone());
            }
        } else if let Some(master) = self.get_master_for_file_id(file_id) {
            master.inject_main_record(record.clone());
        } else {
            progress(&format!(
                "Error: <master file not found> while trying to determine master record for {}",
                record.get_name()
            ));
        }
    }
}

impl MainRecordImpl {
    /// Port of `GetIsInjected`: a record without a master whose FormID
    /// points to a master of its file.
    pub fn is_injected(&self) -> bool {
        let Some(file) = self.file_impl() else { return false };
        let form_id = self.get_fixed_form_id();
        self.master().is_none()
            && !form_id.is_null()
            && !file.is_new_record(form_id.file_id())
            && !file.get_file_states().contains(FileState::fsIsHardcoded)
            && (game_mode() > GameMode::gmTES3 || form_id.file_id().full_slot() > 0)
    }

    /// Port of `TwbMainRecord.SetLoadOrderFormID`: the record takes the load
    /// order FormID `form_id`. A record that stays a new record of its file
    /// with the same object ID only changes its header; otherwise it leaves
    /// the FormID index, its master and its overrides (the first override
    /// becomes the master of the others), takes the FormID and is added
    /// again, as an override where a master has the FormID. The child group
    /// follows, and an interior cell moves to the block of its new FormID.
    /// The references to the record are not changed: the callers do that
    /// with `CompareExchangeFormID` on the referencing records.
    pub fn set_load_order_form_id(self: &Arc<Self>, form_id: FormID) -> Result<(), String> {
        let form_id = if self.mr_struct().signature == Signature::new(b"TES4") {
            FormID::null()
        } else {
            form_id
        };
        if self.get_load_order_form_id() == form_id {
            return Ok(());
        }
        if game_mode() == GameMode::gmTES3 {
            return Ok(());
        }
        let Some(file) = self.file_impl() else { return Ok(()) };
        if form_id.object_id() < 0x800 && !form_id.is_hardcoded() && file.master_count() < 1 {
            return Err(format!(
                "Using FormID [{}] requires \"{}\" to have at least 1 master",
                form_id.to_string(true),
                file.get_name()
            ));
        }
        let file_form_id = File::load_order_form_id_to_file_form_id(&*file, form_id, true)?;
        let own = self.mr_struct().form_id;
        if own.object_id() == file_form_id.object_id()
            && file.is_new_record(own.file_id())
            && file.is_new_record(file_form_id.file_id())
        {
            // The quiet change: the record stays a new record of the file.
            let group = self.child_group();
            self.make_header_writeable(|header| header.form_id = file_form_id);
            self.clear_fixed_form_id();
            if let Some(group) = group {
                group.set_group_label(file_form_id.to_cardinal())?;
            }
            return self.update_interior_cell_group();
        }
        if let Some(existing) = file.record_by_form_id(file_form_id, false, true)
            && existing.file_impl().is_some_and(|other| Arc::ptr_eq(&other, &file))
        {
            return Err(format!(
                "FormID [{}] is already present in file {}",
                form_id.to_string(true),
                file.get_name()
            ));
        }
        let master = file
            .record_by_form_id(file_form_id, true, true)
            .map(|record| record.master_or_self_impl());

        file.remove_main_record(self)?;
        // The referenced-by list goes to the override that becomes the
        // master, or is dropped (`mrReferencedBy := nil`).
        let referenced_by = self.take_referenced_by();
        if let Some(old_master) = self.master() {
            old_master.remove_override(self);
        } else {
            let overrides = self.overrides();
            if let Some(first) = overrides.first() {
                first.you_are_the_master(&overrides, referenced_by);
            }
        }
        *self.mr_master.write().unwrap() = None;
        self.mr_overrides.write().unwrap().clear();

        let group = self.child_group();
        self.make_header_writeable(|header| header.form_id = file_form_id);
        self.clear_fixed_form_id();
        if let Some(group) = group {
            group.set_group_label(file_form_id.to_cardinal())?;
        }
        self.update_interior_cell_group()?;

        file.add_main_record(self.clone())?;

        if let Some(master) = master
            && master.is_injected()
            && self.master().is_none()
        {
            master.you_got_a_master(self);
        }
        Ok(())
    }

    /// `GetMasterOrSelf` with the concrete record type.
    pub fn master_or_self_impl(self: &Arc<Self>) -> Arc<MainRecordImpl> {
        self.master().unwrap_or_else(|| self.clone())
    }

    /// Port of `TwbMainRecord.ContainerChanged` for the contained-in
    /// element: it takes the label of the group again.
    pub(crate) fn container_changed(&self) {
        let contained_in = self
            .container
            .elements()
            .into_iter()
            .find(|element| element.get_sort_order() == -2)
            .and_then(|element| element.as_element_impl()?.value_impl());
        let Some(contained_in) = contained_in else { return };
        let Some(mut group) = self
            .base
            .container()
            .and_then(|container| container.as_element_impl()?.group_record_impl())
        else {
            return;
        };
        for group_type in [5, 4] {
            if group.group_type() == group_type
                && let Some(parent) = group.parent_group()
            {
                group = parent;
            }
        }
        let vwd_as_quest = crate::interface::globals::vwd_as_quest_children();
        let children = if vwd_as_quest { 8..=9 } else { 8..=10 };
        if children.contains(&group.group_type()) {
            let Some(parent) = group.parent_group() else { return };
            group = parent;
        }
        let parents: &[i32] = if vwd_as_quest { &[1, 6, 7, 10] } else { &[1, 6, 7] };
        if !parents.contains(&group.group_type()) {
            return;
        }
        contained_in.replace_data(group.group_label().to_le_bytes().to_vec());
    }

    /// Port of `TwbMainRecord.UpdateInteriorCellGroup`: an interior cell
    /// sits in the block and sub-block its object ID gives (the last and
    /// the second to last decimal digit), which are made when missing; its
    /// child group moves with it, and an emptied sub-block or block goes.
    pub(crate) fn update_interior_cell_group(self: &Arc<Self>) -> Result<(), String> {
        if game_mode() == GameMode::gmTES3 || self.mr_struct().signature != Signature::new(b"CELL") {
            return Ok(());
        }
        if !self.get_element_exists("DATA") {
            return Ok(());
        }
        let data = self.get_element_native_value("DATA").as_ordinal().unwrap_or(0);
        if data & 1 != 1 {
            return Ok(());
        }
        let not_in_group = || format!("{} is not contained in a group.", self.get_name());
        let container = self
            .base
            .container()
            .and_then(|container| container.as_element_impl()?.group_record_impl())
            .ok_or_else(not_in_group)?;
        let is_top_cell = |group: &GroupRecordImpl| {
            group.group_type() == 0 && group.gr_struct().label_signature() == Signature::new(b"CELL")
        };
        let (sub_block_group, block_group, top_group) = if container.group_type() == 3 {
            let block = container
                .parent_group()
                .ok_or_else(|| format!("{} is not contained in a group.", container.get_name()))?;
            if block.group_type() != 2 {
                return Err(format!(
                    "{} is not contained in a group of type \"Interior Cell Block\"",
                    self.get_name()
                ));
            }
            let top = block.parent_group().ok_or_else(not_in_group)?;
            if !is_top_cell(&top) {
                return Err(format!(
                    "{} is not contained in a group of type \"Top CELL\"",
                    self.get_name()
                ));
            }
            (Some(container.clone()), Some(block), top)
        } else if is_top_cell(&container) {
            (None, None, container.clone())
        } else {
            return Err(format!(
                "{} is not contained in a group of type \"Interior Cell Sub-Block\"",
                self.get_name()
            ));
        };
        let digits = format!("00{}", self.mr_struct().form_id.object_id());
        let digits = digits.as_bytes();
        let block = u32::from(digits[digits.len() - 1] - b'0');
        let sub_block = u32::from(digits[digits.len() - 2] - b'0');

        let mut new_block_group = block_group.clone();
        let mut new_sub_block_group = sub_block_group.clone();
        if new_block_group
            .as_ref()
            .is_none_or(|group| group.group_label() != block)
        {
            new_sub_block_group = None;
            new_block_group = child_group_with_label(&top_group, block);
            if new_block_group.is_none() {
                let group = GroupRecordImpl::create_labelled(&top_group, 2, block);
                top_group.set_modified(true);
                top_group.resort();
                new_block_group = Some(group);
            }
        }
        let new_block_group = new_block_group.expect("the block group exists");
        if new_sub_block_group
            .as_ref()
            .is_none_or(|group| group.group_label() != sub_block)
        {
            new_sub_block_group = child_group_with_label(&new_block_group, sub_block);
            if new_sub_block_group.is_none() {
                let group = GroupRecordImpl::create_labelled(&new_block_group, 3, sub_block);
                new_block_group.set_modified(true);
                new_block_group.resort();
                new_sub_block_group = Some(group);
            }
        }
        let new_sub_block_group = new_sub_block_group.expect("the sub-block group exists");
        if Arc::ptr_eq(&container, &new_sub_block_group) {
            return Ok(());
        }
        let child_group = self.child_group();
        let self_ref: ElementRef = self.clone();
        container.container.remove_element_by_identity(&self_ref);
        let child_ref = child_group.map(|group| group as ElementRef);
        if let Some(child) = &child_ref {
            container.container.remove_element_by_identity(child);
        }
        let new_sub_block_ref: ElementRef = new_sub_block_group.clone();
        self.base.set_container(&new_sub_block_ref);
        new_sub_block_group.container.add_element(self_ref);
        if let Some(child) = child_ref {
            if let Some(child_impl) = child.as_element_impl() {
                child_impl.element_base().set_container(&new_sub_block_ref);
            }
            new_sub_block_group.container.add_element(child);
        }
        new_sub_block_group.set_modified(true);
        new_sub_block_group.resort();
        if let Some(sub_block_group) = sub_block_group {
            if sub_block_group.container.element_count() == 0 {
                sub_block_group.remove();
                if let Some(block_group) = block_group {
                    if block_group.container.element_count() == 0 {
                        block_group.remove();
                        top_group.set_modified(true);
                    } else {
                        block_group.set_modified(true);
                    }
                }
            } else {
                sub_block_group.set_modified(true);
            }
        }
        Ok(())
    }
}

/// The group in `parent` with the label, as the loops of
/// `UpdateInteriorCellGroup` find it.
fn child_group_with_label(parent: &GroupRecordImpl, label: u32) -> Option<Arc<GroupRecordImpl>> {
    parent
        .container
        .elements()
        .iter()
        .filter_map(|element| element.as_element_impl()?.group_record_impl())
        .find(|group| group.group_label() == label)
}

impl GroupRecordImpl {
    /// The label written into the group header, without the rest of
    /// `SetGroupLabel` (the `grsLabel` field itself).
    pub(crate) fn set_raw_group_label(&self, label: u32) {
        self.gr_struct.write().unwrap().label = label;
    }

    /// Port of `TwbGroupRecord.GetGroupLabel`: the label, with the FileID
    /// of a FormID beyond the masters replaced by the file's own for the
    /// groups whose label is a FormID.
    pub(crate) fn get_group_label(&self) -> u32 {
        let label = self.group_label();
        if matches!(self.group_type(), 1 | 6..=10)
            && let Some(file) = self.file.upgrade()
        {
            let form_id = FormID::from_cardinal(label);
            if file.is_new_record(form_id.file_id()) {
                let file_file_id: FileID = file.get_file_file_id();
                if form_id.file_id() != file_file_id {
                    return form_id.change_file_id(file_file_id).to_cardinal();
                }
            }
        }
        label
    }

    /// Port of `TwbGroupRecord.SetGroupLabel`: the label of a group of the
    /// children of a record; a FormID of a new record of the file takes the
    /// file's own FileID. The children groups of a cell take the label too,
    /// and the records in the group update their contained-in element.
    pub(crate) fn set_group_label(&self, label: u32) -> Result<(), String> {
        let group_type = self.group_type();
        if !matches!(group_type, 1 | 6..=10) {
            return Err(format!("Can not set Label of {}", self.get_name()));
        }
        let mut label = label;
        if let Some(file) = self.file.upgrade() {
            let form_id = FormID::from_cardinal(label);
            if file.is_new_record(form_id.file_id()) {
                let file_file_id: FileID = file.get_file_file_id();
                if form_id.file_id() != file_file_id {
                    label = form_id.change_file_id(file_file_id).to_cardinal();
                }
            }
        }
        let changed = label != self.group_label();
        if changed {
            // `MakeHeaderWriteable`.
            self.set_modified(true);
            edit::invalidate_parent_storage(self);
            self.set_raw_group_label(label);
        }
        for element in self.container.elements() {
            if let Some(group) = element.as_element_impl().and_then(ElementImpl::group_record_impl) {
                if matches!(group.group_type(), 8..=10) {
                    group.set_group_label(label)?;
                }
                continue;
            }
            if changed && let Some(record) = element.as_element_impl().and_then(ElementImpl::main_record_impl) {
                record.container_changed();
            }
        }
        Ok(())
    }
}

/// The signatures of the placed records `CheckChildOfCell` and
/// `GetPosition` accept.
const CELL_CHILDREN: [&[u8; 4]; 11] = [
    b"REFR", b"PMIS", b"PGRE", b"ACRE", b"ACHR", b"PARW", b"PBEA", b"PFLA", b"PCON", b"PBAR", b"PHZD",
];

impl MainRecordImpl {
    /// Port of `CheckChildOfCell`: whether the record is a placed record in
    /// a persistent, temporary or visible distant children group of a cell.
    /// UPSTREAM-QUIRK: upstream raises for a placed record outside such a
    /// group; the flag setters here leave it where it is instead.
    pub(crate) fn check_child_of_cell(&self) -> bool {
        if !CELL_CHILDREN.contains(&&self.mr_struct().signature.0) {
            return false;
        }
        let Some(type_group) = self
            .base
            .container()
            .and_then(|container| container.as_element_impl()?.group_record_impl())
        else {
            return false;
        };
        matches!(type_group.group_type(), 8..=10)
            && type_group.parent_group().is_some_and(|group| group.group_type() == 6)
    }

    /// Port of `GetPosition`: `DATA\Position` of a placed record.
    fn position(&self) -> Option<(f64, f64, f64)> {
        if !CELL_CHILDREN.contains(&&self.mr_struct().signature.0) {
            return None;
        }
        let axis = |name: &str| match self.get_element_native_value(&format!("DATA\\Position\\{name}")) {
            crate::interface::misc::Variant::Float(value) => Some(value),
            other => other.as_ordinal().map(|value| value as f64),
        };
        Some((axis("X")?, axis("Y")?, axis("Z")?))
    }

    /// Port of `EnsureChildGroup` for a cell: its children group, made in
    /// the group that holds the cell when it has none.
    fn ensure_cell_child_group(self: &Arc<Self>) -> Option<Arc<GroupRecordImpl>> {
        if let Some(group) = self.child_group() {
            return Some(group);
        }
        let containing = self.base.container()?.as_element_impl()?.group_record_impl()?;
        if !containing.is_element_editable(None) {
            return None;
        }
        Some(GroupRecordImpl::create_child(&containing, 6, self))
    }

    /// Port of `UpdateCellChildGroup`: a placed record whose persistent or
    /// visible when distant flag changed moves to the children group of its
    /// cell that the flags call for (persistent 8, visible distant 10 unless
    /// `wbVWDInTemporary`, else temporary 9); an exterior persistent record
    /// moves to the persistent cell of its worldspace and a temporary one to
    /// the cell of its grid position. Emptied groups go.
    pub(crate) fn update_cell_child_group(self: &Arc<Self>) {
        let Some(old_type_group) = self
            .base
            .container()
            .and_then(|container| container.as_element_impl()?.group_record_impl())
        else {
            return;
        };
        if !matches!(old_type_group.group_type(), 8..=10) {
            return;
        }
        let Some(old_child_group) = old_type_group.parent_group() else {
            return;
        };
        if old_child_group.group_type() != 6 {
            return;
        }
        let flags = self.mr_struct().flags;
        let correct = if flags.is_persistent() {
            8
        } else if flags.is_visible_when_distant() && !crate::interface::globals::vwd_in_temporary() {
            10
        } else {
            9
        };
        let old_type = old_type_group.group_type();
        if old_type == correct && correct != 9 {
            return;
        }
        let Some(old_cell) = old_child_group.children_of() else {
            return;
        };
        let not_partial = if old_cell.get_element_exists("DATA") {
            old_cell.clone()
        } else {
            let Some(file) = self.file_impl() else { return };
            let visible = old_cell.highest_override_visible_for_file(&file);
            if !visible.get_element_exists("DATA") {
                return;
            }
            visible
        };
        let data = not_partial.get_element_native_value("DATA").as_ordinal().unwrap_or(0);
        let is_exterior = data & 1 == 0;
        if old_type == correct && !is_exterior {
            return;
        }
        let mut new_cell: Option<Arc<MainRecordImpl>> = None;
        let new_child_group = if is_exterior {
            let Some(owner) = old_cell
                .base
                .container()
                .and_then(|container| container.as_element_impl()?.group_record_impl())
            else {
                return;
            };
            if !matches!(owner.group_type(), 1 | 5) {
                return;
            }
            let add_cell = |name: &str| -> Option<Arc<MainRecordImpl>> {
                let world = owner
                    .children_of()
                    .filter(|record| record.mr_struct().signature == Signature::new(b"WRLD"))?;
                world
                    .add(name, true)
                    .ok()
                    .flatten()?
                    .as_element_impl()?
                    .main_record_impl()
            };
            if correct == 8 {
                if owner.group_type() != 1 {
                    let Some(cell) = add_cell("CELL[P]") else { return };
                    let group = cell.ensure_cell_child_group();
                    new_cell = Some(cell);
                    group
                } else {
                    Some(old_child_group.clone())
                }
            } else {
                let Some((x, y, _)) = self.position() else { return };
                let grid = (position_to_grid_cell(x), position_to_grid_cell(y));
                if !not_partial.mr_struct().flags.is_persistent() {
                    // Upstream raises "Could not determine grid cell of ..."
                    // here; the record stays where it is.
                    let Some(cell_grid) = not_partial.get_grid_cell() else {
                        return;
                    };
                    if cell_grid == grid {
                        if old_type == correct {
                            return;
                        }
                        new_cell = Some(old_cell.clone());
                    }
                }
                if new_cell.is_none() {
                    new_cell = add_cell(&format!("CELL[{},{}]", grid.0, grid.1));
                }
                let Some(cell) = &new_cell else { return };
                cell.ensure_cell_child_group()
            }
        } else {
            Some(old_child_group.clone())
        };
        let Some(new_child_group) = new_child_group else {
            return;
        };
        let Some(new_cell) = new_cell.or_else(|| new_child_group.children_of()) else {
            return;
        };
        let new_type_group = new_child_group
            .find_child_group(correct, new_cell.mr_struct().form_id.to_cardinal())
            .unwrap_or_else(|| GroupRecordImpl::create_child(&new_child_group, correct, &new_cell));
        if Arc::ptr_eq(&old_type_group, &new_type_group) {
            return;
        }
        let self_ref: ElementRef = self.clone();
        old_type_group.container.remove_element_by_identity(&self_ref);
        if old_type_group.container.element_count() == 0 {
            old_type_group.remove();
        } else {
            old_type_group.set_modified(true);
        }
        let new_type_ref: ElementRef = new_type_group.clone();
        self.base.set_container(&new_type_ref);
        new_type_group.container.add_element(self_ref);
        new_type_group.set_modified(true);
        // `Sort` without `aForce`: a group sorts once, later records are
        // appended.
        new_type_group.sort();
        if old_child_group.container.element_count() == 0 {
            old_child_group.remove();
        } else {
            old_child_group.set_modified(true);
        }
    }
}

/// Port of one axis of `wbPositionToGridCell`: the cell of a coordinate,
/// rounded down.
fn position_to_grid_cell(value: f64) -> i32 {
    let factor = crate::interface::globals::cell_size_factor();
    let mut result = (value / factor).trunc() as i32;
    if value < 0.0 && (value / factor).fract() != 0.0 {
        result -= 1;
    }
    result
}
