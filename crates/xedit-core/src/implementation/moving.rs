// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas (TwbElement.CanMoveUp,
// CanMoveDown, MoveUp, MoveDown; TwbContainer.CanMoveElement,
// CanMoveElementUp, CanMoveElementDown, MoveElementUp, MoveElementDown,
// InsertElement; the CanMoveElement of TwbSubRecord, TwbSubRecordArray and
// TwbArray)

//! The order of the entries of an unsorted array: an entry moved up or down
//! by one ("Move Up" and "Move Down" of the view, the script functions of
//! the same names) and an element put into a container at a position
//! (`InsertElement` of the script API).

use crate::interface::def::Def;
use crate::interface::element::{Element, ElementRef};
use crate::interface::globals::is_internal_edit;
use crate::interface::misc::EditError;
use crate::interface::types::{DefFlag, ElementType};

use super::ElementImpl;

/// Port of the `CanMoveElement` overrides: an unsorted array whose
/// definitions allow it (`dfNoMove`); `TwbContainer`'s is False.
fn can_move_element(container: &dyn ElementImpl) -> bool {
    let no_move = |def: Option<std::sync::Arc<dyn Def>>| {
        def.is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfNoMove))
    };
    if let Some(sub_record) = container.sub_record_impl() {
        // `srStates * [srsIsArray, srsSorted] = [srsIsArray]`.
        return sub_record.is_array()
            && !sub_record.get_sorted()
            && !no_move(container.get_def().map(|def| def.def_ref()))
            && !no_move(container.get_value_def().map(|def| def.def_ref()));
    }
    if let Some(array) = container.sub_record_array_impl() {
        return !array.is_sorted() && !no_move(container.get_def().map(|def| def.def_ref()));
    }
    if let Some(value) = container.value_impl()
        && value.get_element_type() == ElementType::etArray
    {
        return value.get_sorted() != Some(true) && !no_move(container.get_value_def().map(|def| def.def_ref()));
    }
    false
}

/// The checks `CanMoveElementUp` and `CanMoveElementDown` share: no
/// internal-only definition, an editable container, a container whose
/// entries can move, and two entries at least.
fn container_allows_move(container: &dyn ElementImpl) -> bool {
    if !is_internal_edit() {
        let internal_only = |def: Option<std::sync::Arc<dyn Def>>| {
            def.is_some_and(|def| def.def_base().def_flags.contains(DefFlag::dfInternalEditOnly))
        };
        if internal_only(container.get_def().map(|def| def.def_ref()))
            || internal_only(container.get_value_def().map(|def| def.def_ref()))
        {
            return false;
        }
    }
    super::assign::parent_allows_edit(container) && can_move_element(container)
}

/// The position of `element` among the elements of `container`.
fn position_of(container: &dyn ElementImpl, element: &dyn Element) -> Option<(usize, usize)> {
    let base = container.container_base()?;
    let id = element.get_element_id();
    let elements = base.elements();
    let position = elements.iter().position(|candidate| candidate.get_element_id() == id)?;
    Some((position, elements.len()))
}

/// Port of `TwbContainer.CanMoveElementUp`.
pub fn can_move_element_up(container: &dyn ElementImpl, element: &dyn Element) -> bool {
    if !container_allows_move(container) {
        return false;
    }
    matches!(position_of(container, element), Some((position, count)) if count >= 2 && position > 0)
}

/// Port of `TwbContainer.CanMoveElementDown`.
pub fn can_move_element_down(container: &dyn ElementImpl, element: &dyn Element) -> bool {
    if !container_allows_move(container) {
        return false;
    }
    matches!(position_of(container, element), Some((position, count)) if count >= 2 && position + 1 < count)
}

/// Port of `MoveElementUp` and `MoveElementDown`: the element swaps places
/// with its neighbour, the container is modified and its storage stale.
fn move_element(container: &dyn ElementImpl, element: &dyn Element, up: bool) {
    if !super::assign::parent_allows_edit(container) || !can_move_element(container) {
        return;
    }
    let Some((position, count)) = position_of(container, element) else {
        return;
    };
    if count < 2 {
        return;
    }
    let other = if up {
        if position == 0 {
            return;
        }
        position - 1
    } else {
        if position + 1 >= count {
            return;
        }
        position + 1
    };
    container.set_modified(true);
    container.invalidate_storage();
    if let Some(base) = container.container_base() {
        base.swap(position, other);
    }
    container.reset_memory_order(position.min(other), position.max(other));
}

/// The container of an element, as the element implementation.
fn container_of(element: &ElementRef) -> Option<ElementRef> {
    element.as_element_impl()?.element_base().container()
}

/// Port of `TwbElement.CanMoveUp`.
pub fn can_move_up(element: &ElementRef) -> bool {
    container_of(element)
        .as_ref()
        .and_then(|container| container.as_element_impl())
        .is_some_and(|container| can_move_element_up(container, &**element))
}

/// Port of `TwbElement.CanMoveDown`.
pub fn can_move_down(element: &ElementRef) -> bool {
    container_of(element)
        .as_ref()
        .and_then(|container| container.as_element_impl())
        .is_some_and(|container| can_move_element_down(container, &**element))
}

/// Port of `TwbElement.MoveUp`.
pub fn move_up(element: &ElementRef) {
    if !can_move_up(element) {
        return;
    }
    if let Some(container) = container_of(element)
        && let Some(container) = container.as_element_impl()
    {
        move_element(container, &**element, true);
    }
}

/// Port of `TwbElement.MoveDown`.
pub fn move_down(element: &ElementRef) {
    if !can_move_down(element) {
        return;
    }
    if let Some(container) = container_of(element)
        && let Some(container) = container.as_element_impl()
    {
        move_element(container, &**element, false);
    }
}

/// Port of `TwbContainer.InsertElement`: the element, which must not be in
/// a container (`TwbElement.SetContainer` asserts it), goes into the
/// container at the position (kept within the elements), the memory orders
/// from there on follow, and the container is told of the change.
pub fn insert_element(container: &ElementRef, position: i32, element: &ElementRef) -> Result<(), EditError> {
    let Some(container_impl) = container.as_element_impl() else {
        return Err(format!("{} is not a container", container.get_name()));
    };
    let Some(base) = container_impl.container_base() else {
        return Err(format!("{} is not a container", container.get_name()));
    };
    let Some(element_impl) = element.as_element_impl() else {
        return Err(format!("{} can not be inserted", element.get_name()));
    };
    // `TwbContainer.RemoveElement` clears the container of the element it
    // removes (`SetContainer(nil)`, `edit::container_remove_child`); the
    // check also holds for a container that still names the element.
    let id = element.get_element_id();
    let held = element_impl.element_base().container().is_some_and(|owner| {
        owner
            .as_element_impl()
            .and_then(|owner| owner.container_base())
            .is_some_and(|owner| {
                owner
                    .elements()
                    .iter()
                    .any(|candidate| candidate.get_element_id() == id)
            })
    });
    if held {
        // UPSTREAM-QUIRK: `SetContainer` asserts instead of moving an
        // element that has a container.
        return Err("Assertion failure (TwbElement.SetContainer: the element has a container)".to_owned());
    }
    // UPSTREAM-QUIRK: an entry of an array value that goes back into an
    // array writes no data when the array is saved (the oracle's save of
    // `InsertElement(c, i, RemoveElement(c, e))` on the IDLA array of an
    // idle marker lacks the entry, at another position and at its own; a
    // member of a structure and a subrecord of a subrecord array keep
    // theirs), so the port gives it empty data.
    let into_array = container_impl
        .sub_record_impl()
        .is_some_and(|sub_record| sub_record.is_array())
        || container_impl
            .value_impl()
            .is_some_and(|value| value.get_element_type() == ElementType::etArray);
    if into_array
        && element_impl.value_impl().is_some()
        && let Some(storage) = element_impl.storage()
    {
        storage.set(Vec::new());
        storage.set_invalid(false);
    }
    let count = base.element_count();
    let position = usize::try_from(position.max(0)).unwrap_or(0).min(count);
    base.insert_element(position, element.clone());
    container_impl.reset_memory_order(position, usize::MAX);
    element_impl.element_base().set_container(container);
    container_impl.notify_changed();
    Ok(())
}
