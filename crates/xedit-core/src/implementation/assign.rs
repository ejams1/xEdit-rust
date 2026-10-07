// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of the assign path of `wbImplementation.pas` that the element
//! classes share: `TwbElement.Assign` and `CanAssign` with their
//! `AssignInternal` and `CanAssignInternal`, the overrides of
//! `TwbContainer`, `IsElementEditable`, `IsElementRemovable` and
//! `GetIsRemovable`, and `ContainsReflection` and `ContainsUnmappedFormID`.
//! The overrides of the other classes are next to them (`sub_record`,
//! `value`, the main record in `mod.rs`).
//!
//! State: the source of an assign is one element; `IwbMultipleElements`,
//! the template elements and the aligned arrays (`GetAlignable`) are not
//! ported, nor the sort keys of a value structure (`GetIsInSK` of a
//! subrecord).

use std::sync::Arc;

use crate::interface::def::Def;
use crate::interface::element::{Element, ElementRef};
use crate::interface::globals::{edit_allowed, is_internal_edit};
use crate::interface::misc::{EditError, progress};
use crate::interface::types::{ASSIGN_THIS, ConflictPriority, DefFlag, ElementType};

use super::ElementImpl;

/// The definition an assign compares: the value definition, else the
/// definition (`GetValueDef`, else `GetDef`).
pub(crate) fn assign_def(element: &dyn Element) -> Option<Arc<dyn Def>> {
    match element.get_value_def() {
        Some(value_def) => Some(value_def.def_ref()),
        None => element.get_def().map(|def| def.def_ref()),
    }
}

/// `Supports(aDef, ...)` on an optional definition.
pub(crate) fn as_def(def: &Option<Arc<dyn Def>>) -> Option<&dyn Def> {
    def.as_deref()
}

/// Port of `TwbElement.Assign`: the update counter around `AssignInternal`.
/// An error is reported (`Error assigning to ...`) and gives `None`, as
/// upstream catches every exception here.
pub(crate) fn assign(
    element: &dyn ElementImpl,
    index: i32,
    source: Option<&ElementRef>,
    only_sk: bool,
) -> Option<ElementRef> {
    edit_begin_update(element);
    let result = (|| {
        if let Some(source) = source
            && contains_reflection(&**source)
            && (source.as_main_record().is_none() || element.as_main_record().is_some())
        {
            return Err(format!(
                "{} contains Reflection and can not be assigned",
                source.get_name()
            ));
        }
        element.assign_internal(index, source, only_sk)
    })();
    edit_end_update(element);
    match result {
        Ok(result) => result,
        Err(error) => {
            let source_name = source.map_or_else(|| "nil".to_owned(), |source| source.get_full_path());
            progress(&format!(
                "Error assigning to [{}] from [{source_name}]: [Exception] {error}",
                element.get_full_path()
            ));
            None
        }
    }
}

fn edit_begin_update(element: &dyn ElementImpl) {
    super::edit::begin_update(element);
}

fn edit_end_update(element: &dyn ElementImpl) {
    super::edit::end_update(element);
}

/// Port of `TwbElement.CanAssign`.
pub(crate) fn can_assign(
    element: &dyn ElementImpl,
    index: i32,
    source: Option<&ElementRef>,
    check_dont_show: bool,
) -> bool {
    if let Some(source) = source
        && contains_reflection(&**source)
        && (source.as_main_record().is_none() || element.as_main_record().is_some())
    {
        return false;
    }
    element.can_assign_internal(index, source, check_dont_show)
}

/// Port of `TwbElement.CanAssignInternal`: the definitions decide.
pub(crate) fn element_can_assign_internal(
    element: &dyn ElementImpl,
    index: i32,
    source: Option<&ElementRef>,
    check_dont_show: bool,
) -> bool {
    if is_internal_edit() {
        return true;
    }
    if !edit_allowed() || !element.get_is_editable() {
        return false;
    }
    let Some(source) = source else { return false };
    let Some(target_def) = assign_def(element) else {
        return false;
    };
    if target_def.def_base().def_flags.contains(DefFlag::dfInternalEditOnly) {
        return false;
    }
    let source_def = assign_def(&**source);
    if source_def.is_none() {
        return false;
    }
    let this = element.self_element_ref();
    let mut result = target_def.can_assign(this.as_ref(), index, as_def(&source_def));
    if result && check_dont_show && element.get_dont_show() {
        result = false;
    }
    result
}

/// Port of `TwbElement.AssignInternal`: the value definition assigns.
pub(crate) fn element_assign_internal(
    element: &dyn ElementImpl,
    index: i32,
    source: Option<&ElementRef>,
    only_sk: bool,
) -> Result<Option<ElementRef>, EditError> {
    if !is_internal_edit() && !edit_allowed() {
        return Err(format!("{} can not be assigned", element.get_name()));
    }
    let Some(value_def) = element.get_value_def() else {
        return Err(format!("{} can not be assigned", element.get_name()));
    };
    if !is_internal_edit() && value_def.def_base().def_flags.contains(DefFlag::dfInternalEditOnly) {
        return Err(format!("{} can not be assigned", element.get_name()));
    }
    let Some(this) = element.self_element_ref() else {
        return Ok(None);
    };
    value_def.assign(&this, index, source, only_sk)
}

/// The test of `TwbContainer.AssignInternal` and `CanAssignInternal` that
/// the source is the same kind of container: the same number of elements,
/// and value definitions that are the same or can take each other.
fn same_container(element: &dyn ElementImpl, index: i32, source: &ElementRef) -> bool {
    let Some(source_container) = source.as_container() else {
        return false;
    };
    let count = element
        .as_container()
        .map_or(0, |container| container.get_element_count());
    if source_container.get_element_count() != count {
        return false;
    }
    let value_def = element.get_value_def();
    let source_value_def = source.get_value_def();
    match (&value_def, &source_value_def) {
        (None, None) => true,
        (Some(value_def), _) => {
            let source_def = source_value_def.as_deref().map(|def| def.as_dyn_def());
            let this = element.self_element_ref();
            value_def.equals(source_def) || value_def.can_assign(this.as_ref(), index, source_def)
        }
        (None, Some(_)) => false,
    }
}

/// Port of `TwbContainer.CanAssignInternal`: the definitions, or a
/// container of the same kind whose elements can take each other.
pub(crate) fn container_can_assign_internal(
    element: &dyn ElementImpl,
    index: i32,
    source: Option<&ElementRef>,
    check_dont_show: bool,
) -> bool {
    if !is_internal_edit() {
        if !edit_allowed() {
            return false;
        }
        if element
            .get_def()
            .is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfInternalEditOnly))
        {
            return false;
        }
    }
    let Some(source) = source else { return false };
    if !parent_allows_edit(element) {
        return false;
    }
    let mut result = element_can_assign_internal(element, index, Some(source), check_dont_show);
    let count = element
        .as_container()
        .map_or(0, |container| container.get_element_count());
    if !result && index == ASSIGN_THIS && count > 0 {
        result = same_container(element, index, source);
        if result {
            let children = element.container_base().map(|base| base.elements()).unwrap_or_default();
            let Some(source_container) = source.as_container() else {
                return false;
            };
            for (i, child) in children.iter().enumerate() {
                let Some(source_child) = source_container.get_element(i as i32) else {
                    return false;
                };
                let ignored = child.get_conflict_priority() == ConflictPriority::cpIgnore
                    || source_child.get_conflict_priority() == ConflictPriority::cpIgnore;
                if !ignored && !child.can_assign(ASSIGN_THIS, Some(&source_child), check_dont_show) {
                    return false;
                }
            }
        }
    }
    result
}

/// Port of `TwbContainer.AssignInternal`: the definitions assign, and a
/// container of the same kind assigns element by element.
pub(crate) fn container_assign_internal(
    element: &dyn ElementImpl,
    index: i32,
    source: Option<&ElementRef>,
    only_sk: bool,
) -> Result<Option<ElementRef>, EditError> {
    if !is_internal_edit() {
        if !edit_allowed() {
            return Err(format!("{} can not be assigned.", element.get_name()));
        }
        if element
            .get_def()
            .is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfInternalEditOnly))
        {
            return Ok(None);
        }
    }
    if !parent_allows_edit(element) {
        return Ok(None);
    }
    let count = element
        .as_container()
        .map_or(0, |container| container.get_element_count());
    let mut result = None;
    if element_can_assign_internal(element, index, source, false) {
        result = element_assign_internal(element, index, source, only_sk)?;
    }
    if index == ASSIGN_THIS
        && count > 0
        && let Some(source) = source
        && same_container(element, index, source)
        && let Some(source_container) = source.as_container()
    {
        // With an element map the elements first take their space on
        // disk in the order of the data, and then the values in the order
        // shown.
        let map = element
            .get_value_def()
            .map(|value_def| value_def.get_element_map().to_vec())
            .unwrap_or_default();
        let has_map = !map.is_empty() && map.len() == count as usize;
        if has_map {
            element.set_to_default()?;
        }
        for i in 0..count as usize {
            let j = if has_map { map[i] as usize } else { i };
            let Some(source_child) = source_container.get_element(i as i32) else {
                continue;
            };
            let Some(child) = element.container_base().and_then(|base| base.element_at(j)) else {
                continue;
            };
            // A union is decided again, from the elements assigned before it.
            if source_child.get_element_type() == ElementType::etUnion
                && let Some(value) = child.as_element_impl().and_then(ElementImpl::value_impl)
                && value.value_def().as_resolvable_def().is_some()
            {
                value.union_reinit_without_data();
            }
            if only_sk && !element.get_is_in_sk(child.get_sort_order()) {
                continue;
            }
            if child.can_assign(ASSIGN_THIS, Some(&source_child), false) {
                child.assign(ASSIGN_THIS, Some(&source_child), only_sk);
            } else if source_child
                .get_value_def()
                .is_some_and(|def| def.as_empty_def().is_some())
            {
                // UPSTREAM-QUIRK: a structure whose source ends with the
                // missing optional members is cut after the last member
                // the source has; upstream does that only for a data
                // container whose source is such a structure.
                let optional_from = source
                    .get_value_def()
                    .and_then(|def| def.as_struct_def().map(|def| def.get_optional_from_element()))
                    .unwrap_or(-1);
                if optional_from >= 0 && optional_from as usize <= j && !has_map && element.storage().is_some() {
                    let children = element.container_base().map(|base| base.elements()).unwrap_or_default();
                    let our_size: i32 = children[..i].iter().map(|child| child.get_data_size()).sum();
                    if element.get_data_size() > our_size {
                        element.update_storage_from_elements();
                        let data = element.raw_data().map(<[u8]>::to_vec).unwrap_or_default();
                        let our_size = (our_size as usize).min(data.len());
                        element.release_and_detach();
                        if let Some(storage) = element.storage() {
                            storage.set(data[..our_size].to_vec());
                            storage.set_invalid(false);
                        }
                        element.reset_and_init();
                    }
                    return Ok(result);
                }
                // The source is empty and the target is something else:
                // the target takes its default.
                child.set_to_default()?;
            }
        }
    }
    Ok(result)
}

/// The `eContainer.IsElementEditable(Self)` test of the assign methods: an
/// element without a container may be edited.
pub(crate) fn parent_allows_edit(element: &dyn ElementImpl) -> bool {
    match element.element_base().container() {
        Some(container) => {
            let this = element.self_element_ref();
            container
                .as_element_impl()
                .is_none_or(|container| container.is_element_editable(this.as_ref()))
        }
        None => true,
    }
}

/// Port of `TwbContainer.IsElementEditable`: no internal-only definition
/// on the way, and the container agrees.
pub(crate) fn container_is_element_editable(container: &dyn ElementImpl, element: Option<&ElementRef>) -> bool {
    if !is_internal_edit() {
        let internal_only = |def: Option<Arc<dyn Def>>| {
            def.is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfInternalEditOnly))
        };
        if internal_only(container.get_def().map(|def| def.def_ref()))
            || internal_only(container.get_value_def().map(|def| def.def_ref()))
        {
            return false;
        }
        if let Some(element) = element
            && (internal_only(element.get_def().map(|def| def.def_ref()))
                || internal_only(element.get_value_def().map(|def| def.def_ref())))
        {
            return false;
        }
    }
    parent_allows_edit(container)
}

/// Port of `TwbElement.GetIsRemovable`: the definitions allow it and the
/// container agrees.
pub(crate) fn element_is_removable(element: &dyn ElementImpl) -> bool {
    let this = element.self_element_ref();
    let def = element.get_def();
    if let Some(def) = &def
        && !def.is_removable(this.as_ref())
    {
        return false;
    }
    if let Some(value_def) = element.get_value_def() {
        let same = def.as_ref().is_some_and(|def| def.equals(Some(value_def.as_dyn_def())));
        if !same && !value_def.is_removable(this.as_ref()) {
            return false;
        }
    }
    match (element.element_base().container(), &this) {
        (Some(container), Some(this)) => container
            .as_element_impl()
            .is_some_and(|container| container.is_element_removable(this)),
        _ => true,
    }
}

/// Port of `TwbElement.ContainsReflection` and the override of
/// `TwbContainer`: a Starfield reflection value in the element.
pub(crate) fn contains_reflection(element: &dyn Element) -> bool {
    let Some(def) = element.get_def() else { return false };
    let flags = def.def_base().def_flags.get();
    if !flags.contains(DefFlag::dfCanContainReflection) {
        return false;
    }
    if element
        .get_value_def()
        .is_some_and(|value_def| value_def.def_base().def_flags.contains(DefFlag::dfCanContainReflection))
    {
        return true;
    }
    let Some(container) = element.as_container() else {
        return false;
    };
    if flags.contains(DefFlag::dfIsReflection) {
        return true;
    }
    (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .any(|child| contains_reflection(&*child))
}

/// Port of `TwbElement.ContainsUnmappedFormID` and the override of
/// `TwbContainer`: an unmapped FormID that is not zero in the element.
pub(crate) fn contains_unmapped_form_id(element: &dyn Element) -> bool {
    let Some(def) = element.get_def() else { return false };
    if !def.def_base().def_flags.contains(DefFlag::dfCanContainUnmappedFormID) {
        return false;
    }
    if element
        .get_value_def()
        .is_some_and(|value_def| value_def.def_base().def_flags.contains(DefFlag::dfUnmappedFormID))
        && element.get_native_value().as_ordinal().is_some_and(|value| value != 0)
    {
        return true;
    }
    let Some(container) = element.as_container() else {
        return false;
    };
    (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .any(|child| contains_unmapped_form_id(&*child))
}
