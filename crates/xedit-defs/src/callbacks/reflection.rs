// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsReflection.pas

//! The callbacks of `wbDefinitionsReflection.pas` that are ported by hand.
//! The ones that are not ported yet are stubs in `reflection_stubs.rs`.

#[allow(unused_imports)]
pub use super::reflection_stubs::*;

use xedit_core::interface::*;

/// Upstream names of the negative type indices of `wbREFLStringToStr`.
const REFL_TYPES: [(i64, &str); 17] = [
    (0xFFFF_FF01, "Null"),
    (0xFFFF_FF02, "String"),
    (0xFFFF_FF03, "List"),
    (0xFFFF_FF04, "Map"),
    (0xFFFF_FF05, "Ref"),
    (0xFFFF_FF08, "Int8"),
    (0xFFFF_FF09, "UInt8"),
    (0xFFFF_FF0A, "Int16"),
    (0xFFFF_FF0B, "UInt16"),
    (0xFFFF_FF0C, "Int32"),
    (0xFFFF_FF0D, "UInt32"),
    (0xFFFF_FF0E, "Int64"),
    (0xFFFF_FF0F, "UInt64"),
    (0xFFFF_FF10, "Bool"),
    (0xFFFF_FF11, "Float"),
    (0xFFFF_FF12, "Double"),
    (0xFFFF_FF13, "Diff"),
];

/// Upstream `wbREFLStringToStr`: a type name for a negative index, else
/// the zero-terminated string at the offset in the string table of the
/// subrecord.
pub fn wb_refl_string_to_str(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    let Some(element) = a_element else {
        return String::new();
    };
    if !matches!(
        a_type,
        CallbackType::ctToEditValue | CallbackType::ctToSortKey | CallbackType::ctToStr | CallbackType::ctToSummary
    ) {
        return String::new();
    }
    if a_int < 0 {
        // The index is a signed 32-bit value, the names are its bits.
        let bits = a_int & 0xFFFF_FFFF;
        return REFL_TYPES
            .iter()
            .find(|(value, _)| *value == bits)
            .map_or("<Warning: Unknown Type>", |(_, name)| name)
            .to_owned();
    }
    let Some(sub_record) = element.get_containing_sub_record() else {
        return String::new();
    };
    let Some(table) = sub_record
        .as_container()
        .and_then(|sub_record| sub_record.get_element_by_path("String Table\\Strings"))
    else {
        return String::new();
    };
    let Some(data) = table.as_data_container().and_then(|table| table.get_data()) else {
        return String::new();
    };
    let Some(text) = usize::try_from(a_int).ok().and_then(|offset| data.get(offset..)) else {
        return String::new();
    };
    let length = text.iter().position(|&byte| byte == 0).unwrap_or(text.len());
    xedit_io::Encoding::Mbcs(1252)
        .get_string(&text[..length])
        .unwrap_or_default()
}

/// Upstream `wbREFLStringToInt`: the index of a type name. Editing a
/// string is not ported.
pub fn wb_refl_string_to_int(a_string: &str, a_element: ElementArg) -> i64 {
    if a_string.is_empty() || a_element.is_none() {
        return 0;
    }
    REFL_TYPES
        .iter()
        .find(|(_, name)| *name == a_string)
        .map_or(0, |(value, _)| *value)
}

/// Upstream anonymous routine at line 258 of `wbDefinitionsReflection.pas`:
/// the strings continue up to the `TYPE` chunk.
pub fn wb_reflection_anonymous_258(a_base_ptr: DataPtr, _a_array: ElementArg) -> bool {
    let Some(bytes) = a_base_ptr.and_then(|data| data.get(..4)) else {
        return true;
    };
    u32::from_le_bytes(bytes.try_into().unwrap()) != 0x4550_5954
}
