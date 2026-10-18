// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of `TwbElement.CanChangeMember`, `TwbContainer.CanChangeElementMember`,
//! `TwbContainer.ChangeElementMember`, `TwbElement.NextMember` and
//! `TwbElement.PreviousMember`: the member of a union an element is switched
//! to (the view's "Next Member" and "Previous Member"). A union is either
//! the element of a subrecord array (`SubRecordArrayDef.Element`) or the
//! member at the element's sort order of a subrecord structure
//! (`SubRecordStructDef.Members`). The element of the old member takes the
//! default of the new member's definition, and the new element keeps what
//! its members share with the old one through `TryAssignMembers`.

use std::sync::Arc;

use crate::interface::element::{Element, ElementRef};
use crate::interface::globals::is_internal_edit;
use crate::interface::sub_record_group::RecordDef;
use crate::interface::types::{DefFlag, DefType};

use super::{ElementImpl, assign, sub_record};

/// Upstream `IsElementEditable(Self)` of the container, with its own
/// definition's flags.
fn container_editable(container: &dyn ElementImpl) -> bool {
    let this = container.self_element_ref();
    assign::container_is_element_editable(container, this.as_ref())
}

/// Port of `TwbContainer.CanChangeElementMember`: the definition of the
/// container makes the element a member of a union, and the container is
/// editable.
pub(crate) fn can_change_element_member(container: &dyn ElementImpl, element: &ElementRef) -> bool {
    if !is_internal_edit() {
        let internal_only = |def: Option<crate::interface::def::DefRef>| {
            def.is_some_and(|def| def.def_base().def_flags.get().contains(DefFlag::dfInternalEditOnly))
        };
        if internal_only(container.get_def().map(|def| def.def_ref()))
            || internal_only(container.get_value_def().map(|def| def.def_ref()))
        {
            return false;
        }
    }
    if !container_editable(container) {
        return false;
    }
    let Some(def) = container.get_def() else {
        return false;
    };
    let def = def.def_ref();
    if let Some(array_def) = def.as_sub_record_array_def() {
        array_def.get_element().get_def_type() == DefType::dtSubRecordUnion
    } else if let Some(struct_def) = def.as_sub_record_struct_def() {
        let sort_order = element.get_sort_order();
        sort_order >= 0
            && sort_order < struct_def.get_member_count()
            && struct_def.get_member(sort_order as usize).get_def_type() == DefType::dtSubRecordUnion
    } else {
        false
    }
}

/// The index of the element's definition in the members of the union.
fn member_index(record_def: &dyn RecordDef, element: &ElementRef) -> Option<usize> {
    let def = element.get_def()?;
    let def = def.def_ref();
    (0..usize::try_from(record_def.get_member_count()).ok()?)
        .find(|&index| record_def.get_member(index).equals(Some(&*def)))
}

/// The member the element switches to: the next one, or the previous one,
/// wrapping around in both directions. `None` when the element is not a
/// member of the union.
fn next_member_index(record_def: &dyn RecordDef, element: &ElementRef, previous: bool) -> Option<usize> {
    let old = member_index(record_def, element)?;
    let count = usize::try_from(record_def.get_member_count()).ok()?;
    if count == 0 {
        return None;
    }
    let new = if previous {
        if old == 0 { count - 1 } else { old - 1 }
    } else if old + 1 >= count {
        0
    } else {
        old + 1
    };
    (new != old).then_some(new)
}

/// Port of `TwbElement.CanChangeMember`: the element has a container that
/// lets it change its union member.
pub(crate) fn can_change_member(element: &dyn ElementImpl) -> bool {
    let Some(this) = element.self_element_ref() else {
        return false;
    };
    let Some(container) = element.element_base().container() else {
        return false;
    };
    container
        .as_element_impl()
        .is_some_and(|container| can_change_element_member(container, &this))
}

/// Port of `TwbElement.NextMember` (`aPrevious` false) and
/// `TwbElement.PreviousMember`: the element that took the member's place,
/// or the element itself when it can not change its member.
pub(crate) fn change_element_member(element: &dyn ElementImpl, previous: bool) -> Option<ElementRef> {
    let this = element.self_element_ref()?;
    let container = element.element_base().container()?;
    let container_impl = container.as_element_impl()?;
    // `if Assigned(eContainer) and not IwbContainer(eContainer).IsElementEditable(Self)`:
    // the container itself has to be editable in its own container.
    if !assign::parent_allows_edit(container_impl) {
        return Some(this);
    }
    if !can_change_element_member(container_impl, &this) {
        return Some(this);
    }
    let Some(def) = container_impl.get_def() else {
        return Some(this);
    };
    let def = def.def_ref();
    if def.as_sub_record_array_def().is_some() {
        let array = container_impl.sub_record_array_impl()?;
        array.change_member(&this, previous).or(Some(this))
    } else if def.as_sub_record_struct_def().is_some() {
        let structure = container_impl.sub_record_struct_impl()?;
        structure.change_member(&this, previous).or(Some(this))
    } else {
        Some(this)
    }
}

impl sub_record::SubRecordArrayImpl {
    /// The `SubRecordArrayDef` branch of `TwbContainer.ChangeElementMember`:
    /// a new member of the union is made and put where the old one was,
    /// takes what its members share with the old one, and the old one goes.
    fn change_member(&self, element: &ElementRef, previous: bool) -> Option<ElementRef> {
        let array_def = self.arc_def.as_sub_record_array_def()?;
        let union_def = array_def.get_element();
        if union_def.get_def_type() != DefType::dtSubRecordUnion {
            return None;
        }
        let record_def = union_def.as_record_def()?;
        let new_index = next_member_index(record_def, element, previous)?;
        let new_def = record_def.get_member(new_index);
        let self_ref = self.self_element_ref()?;
        super::edit::begin_update(self);
        let result = (|| {
            let new_element = sub_record::create_member_new(&self_ref, &self.container, &self.file, new_def).ok()?;
            new_element.set_to_default().ok()?;
            let old_pos = self
                .container
                .elements()
                .iter()
                .position(|candidate| Arc::ptr_eq(candidate, element))?;
            let new_pos = self
                .container
                .elements()
                .iter()
                .position(|candidate| Arc::ptr_eq(candidate, &new_element))?;
            if old_pos != new_pos {
                self.container.swap(old_pos, new_pos);
            }
            new_element.set_memory_order(old_pos as i32);
            element.set_memory_order(new_pos as i32);
            if let Some(structure) = new_element
                .as_element_impl()
                .and_then(ElementImpl::sub_record_struct_impl)
            {
                structure.try_assign_members(element);
            }
            element.remove();
            Some(new_element)
        })();
        super::edit::end_update(self);
        result
    }
}

impl sub_record::SubRecordStructImpl {
    /// The `SubRecordStructDef` branch of `TwbContainer.ChangeElementMember`:
    /// the old member goes, the member of the new definition is made at its
    /// sort order, and takes what its members share with the old one.
    fn change_member(&self, element: &ElementRef, previous: bool) -> Option<ElementRef> {
        let struct_def = self.src_def.as_sub_record_struct_def()?;
        let sort_order = element.get_sort_order();
        if sort_order < 0 || sort_order >= struct_def.get_member_count() {
            return None;
        }
        let union_def = struct_def.get_member(sort_order as usize);
        if union_def.get_def_type() != DefType::dtSubRecordUnion {
            return None;
        }
        let record_def = union_def.as_record_def()?;
        let new_index = next_member_index(record_def, element, previous)?;
        let new_def = record_def.get_member(new_index);
        super::edit::begin_update(self);
        let result = (|| {
            element.remove();
            let template: ElementRef =
                super::template::TemplateElement::new(new_def as Arc<dyn crate::interface::def::NamedDef>);
            let result = self.assign(sort_order, Some(&template), false)?;
            result.set_to_default().ok()?;
            if let Some(structure) = result.as_element_impl().and_then(ElementImpl::sub_record_struct_impl) {
                structure.try_assign_members(element);
            }
            Some(result)
        })();
        super::edit::end_update(self);
        result
    }
}
