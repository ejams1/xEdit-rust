// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! The element queries of the view and the conflict detection:
//! `IwbSortableContainer` (`Sorted`, `Alignable`), `DisplaySortKey`,
//! `ConflictPriorityCanChange`, `ContentIsAllZero`, the
//! `esOptionalAndMissing` state, and the elements of a container in the
//! order the GUI holds them (sorted arrays sorted by `DoInit(True)`, the
//! flags of a flags value as elements under `wbFlagsAsArray`).
//!
//! Upstream's view sets the `SortOrder` of the elements it aligns; the
//! callers here keep their own sort orders instead (see the conflict code
//! of `xedit-analysis`), so reading a view changes nothing in the tree.

use crate::interface::element::{Element, ElementRef};
use crate::interface::types::ElementType;

use super::ElementImpl;

/// `Supports(aElement, IwbSortableContainer)` with its two properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sortable {
    /// Upstream `Sorted`: the elements are in the order of their sort keys.
    pub sorted: bool,
    /// Upstream `Alignable`: the entries of an unsorted array of a variable
    /// count, which the view aligns across records.
    pub alignable: bool,
}

/// Port of `Supports(aElement, IwbSortableContainer)`: `TwbSubRecord`,
/// `TwbSubRecordArray`, `TwbArray` and `TwbValue` are sortable containers;
/// every other element gives `None`.
pub fn sortable(element: &ElementRef) -> Option<Sortable> {
    let element = element.as_element_impl()?;
    if let Some(sub_record) = element.sub_record_impl() {
        return Some(Sortable {
            sorted: sub_record.get_sorted(),
            alignable: sub_record.get_alignable(),
        });
    }
    if let Some(array) = element.sub_record_array_impl() {
        return Some(Sortable {
            sorted: array.get_sorted(),
            alignable: array.get_alignable(),
        });
    }
    if let Some(value) = element.value_impl() {
        let sorted = value.get_sorted()?;
        return Some(Sortable {
            sorted,
            alignable: value.get_alignable(),
        });
    }
    None
}

/// Port of `TwbElement.GetDisplaySortKey`: the sort key the view compares.
/// Not ported: the raw data of `wbCompareRawData` (`GetRawDataAsString`),
/// an option of the GUI that is off by default; the sort key is used.
pub fn display_sort_key(element: &ElementRef, extended: bool) -> String {
    element.get_sort_key(extended)
}

/// Port of `TwbElement.GetConflictPriorityCanChange`: from the value
/// definition, else the definition.
pub fn conflict_priority_can_change(element: &ElementRef) -> bool {
    if let Some(value_def) = element.get_value_def() {
        return value_def.get_conflict_priority_can_change();
    }
    element
        .get_def()
        .is_some_and(|def| def.get_conflict_priority_can_change())
}

/// Port of `ContentIsAllZero`: `TwbDataContainer` checks its data,
/// `TwbContainer` its elements, any other element is not all zero.
pub fn content_is_all_zero(element: &ElementRef) -> bool {
    if element.get_element_type() == ElementType::etFlag {
        return false;
    }
    if let Some(data_container) = element.as_data_container() {
        return data_container
            .get_data()
            .is_none_or(|data| data.iter().all(|&byte| byte == 0));
    }
    if let Some(container) = element.as_container() {
        return (0..container.get_element_count())
            .filter_map(|index| container.get_element(index))
            .all(|child| content_is_all_zero(&child));
    }
    false
}

/// Whether `esOptionalAndMissing` is in the states of the element: an
/// optional member of a structure that its data ended before.
pub fn is_optional_and_missing(element: &ElementRef) -> bool {
    element
        .as_element_impl()
        .and_then(ElementImpl::value_impl)
        .is_some_and(|value| value.is_optional_and_missing())
}

/// The elements of a container in the order upstream's GUI holds them, as
/// `ElementCount` and `Elements[i]` give them there: a flags value holds
/// its flags (`wbFlagsAsArray`); a sorted array is sorted by the extended
/// sort keys (`DoInit(True)` with `CompareSortKeys`, which keeps the order
/// of equal keys), which the port does not do on load (owed from phase 3).
pub fn view_elements(element: &ElementRef) -> Vec<ElementRef> {
    if let Some(flags) = super::flag::flags_as_array_of(element) {
        return flags;
    }
    let Some(container) = element.as_container() else {
        return Vec::new();
    };
    let mut elements: Vec<ElementRef> = (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .collect();
    let sorts_entries = element.as_element_impl().is_some_and(|element| {
        element
            .sub_record_impl()
            .is_some_and(|sub_record| sub_record.sorts_its_entries())
            || element.value_impl().is_some_and(|value| {
                value.get_element_type() == ElementType::etArray && value.get_sorted() == Some(true)
            })
    });
    if sorts_entries && elements.len() > 1 {
        let keys: Vec<String> = elements.iter().map(|element| element.get_sort_key(true)).collect();
        let mut order: Vec<usize> = (0..elements.len()).collect();
        order.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
        elements = order.into_iter().map(|index| elements[index].clone()).collect();
    }
    elements
}
