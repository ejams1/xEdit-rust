// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsCommon.pas

//! The callbacks and helpers of `wbDefinitionsCommon.pas` that the
//! transpiler does not write. The functions that are not ported yet are
//! stubs in `common_stubs.rs`.
//!
//! The `wbTryGet...` helpers have `out` parameters upstream. Here they
//! return the value as an `Option`.

use std::sync::Arc;

use xedit_core::interface::*;

pub use super::common_stubs::*;

/// Upstream `Sig2Int`.
pub fn sig2_int(a_signature: Signature) -> u32 {
    a_signature.to_int()
}

/// Upstream `wbNormalizeToRange`: a normalizer that clamps to `aMin..aMax`.
pub fn wb_normalize_to_range(a_min: f64, a_max: f64) -> Option<FloatNormalizer> {
    Some(Arc::new(move |_element: ElementArg, a_float: f64| {
        if a_float < a_min {
            a_min
        } else if a_float > a_max {
            a_max
        } else {
            a_float
        }
    }))
}

/// Upstream `wbTryGetContainerFromUnion`.
pub fn wb_try_get_container_from_union(a_element: ElementArg) -> Option<ElementRef> {
    get_container_from_union(a_element?)
}

/// Upstream `wbTryGetContainerRefFromUnionOrValue`.
pub fn wb_try_get_container_ref_from_union_or_value(a_element: ElementArg) -> Option<ElementRef> {
    get_container_ref_from_union_or_value(a_element?)
}

/// Upstream `wbTryGetContainerWithValidMainRecord`: the element when it is
/// a main record with elements that is not deleted.
pub fn wb_try_get_container_with_valid_main_record(a_element: ElementArg) -> Option<MainRecordRef> {
    let element = a_element?;
    if element.as_container()?.get_element_count() < 1 {
        return None;
    }
    let main_record = element.as_main_record()?;
    if main_record.get_is_deleted() {
        return None;
    }
    element.clone().into_main_record()
}

/// Upstream `wbTryGetContainingMainRecord`.
pub fn wb_try_get_containing_main_record(a_element: ElementArg) -> Option<MainRecordRef> {
    a_element?.get_containing_main_record()
}

/// Upstream `wbTryGetMainRecord`: the main record the element links to,
/// with the signature `aSignature` unless it is empty.
pub fn wb_try_get_main_record(a_element: ElementArg, a_signature: &str) -> Option<MainRecordRef> {
    let main_record = a_element?.get_links_to()?.into_main_record()?;
    if !a_signature.is_empty() && main_record.get_signature().to_string() != a_signature {
        return None;
    }
    Some(main_record)
}

/// Upstream `wbTrySetContainer`: the element when it is a container and the
/// callback builds a summary.
pub fn wb_try_set_container(a_element: ElementArg, a_type: CallbackType) -> Option<ElementRef> {
    if a_type != CallbackType::ctToSummary {
        return None;
    }
    a_element.filter(|element| element.as_container().is_some()).cloned()
}

/// Upstream `wbFlagDecider`: 1 when the flag `aFlag` of the `Flags` element
/// of the container is set.
pub fn wb_flag_decider(a_flag: u8) -> Option<UnionDecider> {
    Some(Arc::new(move |_base_ptr: DataPtr, a_element: ElementArg| {
        let Some(container) = wb_try_get_container_ref_from_union_or_value(a_element) else {
            return 0;
        };
        let Some(flags) = container
            .as_container()
            .and_then(|container| container.get_element_by_path("Flags"))
        else {
            return 0;
        };
        let flag_bits = match flags.get_native_value() {
            Variant::Int(value) => value,
            Variant::UInt(value) => value as i64,
            _ => 0,
        };
        if flag_bits & (1i64 << a_flag) != 0 { 1 } else { 0 }
    }))
}

fn containing_version(a_element: ElementArg) -> Option<i64> {
    let main_record = a_element?.get_containing_main_record()?;
    Some(i64::from(main_record.get_version()))
}

/// Upstream `wbFormVersionDecider` with one version: 1 when the form
/// version of the containing record is at least `aVersion`.
pub fn wb_form_version_decider_version(a_version: i32) -> Option<UnionDecider> {
    Some(Arc::new(
        move |_base_ptr: DataPtr, a_element: ElementArg| match containing_version(a_element) {
            Some(version) if version >= i64::from(a_version) => 1,
            _ => 0,
        },
    ))
}

/// Upstream `wbFormVersionDecider` with a range of versions.
pub fn wb_form_version_decider_min_version(a_min_version: i32, a_max_version: i32) -> Option<UnionDecider> {
    Some(Arc::new(
        move |_base_ptr: DataPtr, a_element: ElementArg| match containing_version(a_element) {
            Some(version) if version >= i64::from(a_min_version) && version <= i64::from(a_max_version) => 1,
            _ => 0,
        },
    ))
}

/// Upstream `wbFormVersionDecider` with a list of versions: the position
/// of the first version the form version is below, or the count.
pub fn wb_form_version_decider_versions(a_versions: &[i32]) -> Option<UnionDecider> {
    let versions = a_versions.to_vec();
    Some(Arc::new(move |_base_ptr: DataPtr, a_element: ElementArg| {
        let Some(version) = containing_version(a_element) else {
            return 0;
        };
        for (index, &candidate) in versions.iter().enumerate() {
            if version < i64::from(candidate) {
                return index as i32;
            }
        }
        versions.len() as i32
    }))
}

/// The size of the data of a subrecord element: `None` when the element is
/// not a subrecord, `Some(None)` when it has no data.
fn sub_record_data_size(a_element: ElementArg) -> Option<Option<i32>> {
    let element = a_element?;
    if element.get_element_type() != ElementType::etSubRecord {
        return None;
    }
    let has_data = element.as_data_container()?.get_data().is_some();
    Some(has_data.then(|| element.get_data_size()))
}

/// Upstream `wbRecordSizeDecider` with one size: 1 when the subrecord has no
/// data or at least `aSize` bytes.
pub fn wb_record_size_decider_size(a_size: i32) -> Option<UnionDecider> {
    Some(Arc::new(
        move |_base_ptr: DataPtr, a_element: ElementArg| match sub_record_data_size(a_element) {
            None => 0,
            Some(None) => 1,
            Some(Some(size)) if size >= a_size => 1,
            Some(Some(_)) => 0,
        },
    ))
}

/// Upstream `wbRecordSizeDecider` with a range: 1 when the size is outside it.
pub fn wb_record_size_decider_min_size(a_min_size: i32, a_max_size: i32) -> Option<UnionDecider> {
    Some(Arc::new(
        move |_base_ptr: DataPtr, a_element: ElementArg| match sub_record_data_size(a_element) {
            None => 0,
            Some(size) => {
                let size = size.unwrap_or(0);
                if size > a_max_size || size < a_min_size { 1 } else { 0 }
            }
        },
    ))
}

/// Upstream `wbRecordSizeDecider` with a list of sizes: the position of the
/// first size the data is below, or the count.
pub fn wb_record_size_decider_sizes(a_sizes: &[i32]) -> Option<UnionDecider> {
    let sizes = a_sizes.to_vec();
    Some(Arc::new(move |_base_ptr: DataPtr, a_element: ElementArg| {
        let Some(size) = sub_record_data_size(a_element) else {
            return 0;
        };
        let size = size.unwrap_or(0);
        for (index, &candidate) in sizes.iter().enumerate() {
            if size < candidate {
                return index as i32;
            }
        }
        sizes.len() as i32
    }))
}
