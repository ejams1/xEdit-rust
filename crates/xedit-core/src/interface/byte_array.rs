// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbByteArrayDef`.
//!
//! Upstream also uses this class in report mode to guess what unknown bytes
//! are (FormIDs, floats, strings). That analysis and its `Report` are not
//! ported yet.

use std::sync::{Arc, Weak};

use super::def::{
    Def, DefBase, DefKind, DefRef, NamedDef, NamedDefArgs, NamedDefBase, ValueDef, ValueDefBase, value_def_plumbing,
};
use super::def::{ValueDefState, request_storage};
use super::element::{DataPtr, ElementArg};
use super::formaters::comma_text;
use super::globals::is_internal_edit;
use super::misc::{EditError, Variant, length};
use super::types::{CallbackType, DefFlag, DefType, EditType};

pub type CountCallback = Arc<dyn Fn(DataPtr, ElementArg) -> u32 + Send + Sync>;

/// `badSize`: the array is prefixed by its length in four bytes.
pub const BYTE_ARRAY_LENGTH_U32: i64 = -1;
/// `badSize`: the array is prefixed by its length in two bytes.
pub const BYTE_ARRAY_LENGTH_U16: i64 = -2;
/// `badSize`: the array is prefixed by its length in one byte.
pub const BYTE_ARRAY_LENGTH_U8: i64 = -4;
/// `badSize`: explicitly null, for `wbNull`. Displays better in unions.
pub const BYTE_ARRAY_NULL: i64 = -255;

/// Upstream `TwbByteArrayDef`: bytes shown in hexadecimal.
pub struct ByteArrayDef {
    self_ref: Weak<ByteArrayDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
    /// Number of bytes when positive, 0 for all remaining data, or one of the
    /// negative `BYTE_ARRAY_` values.
    bad_size: i64,
    bad_count_callback: Option<CountCallback>,
}

impl ByteArrayDef {
    /// Port of `TwbByteArrayDef.Create`. The `after_load` and `after_set`
    /// arguments are not used, as upstream.
    pub fn create(args: NamedDefArgs, size: i64, count_callback: Option<CountCallback>) -> Arc<Self> {
        let (def, nd) = NamedDefBase::create(NamedDefArgs {
            after_load: None,
            after_set: None,
            ..args
        });
        def.def_flags.include(DefFlag::dfSkipImplicitEdit);
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            vd: ValueDefBase::default(),
            bad_size: size,
            bad_count_callback: count_callback,
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbByteArrayDef.Clone`.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(
            NamedDefBase::clone_args(source),
            source.bad_size,
            source.bad_count_callback.clone(),
        );
        ValueDefBase::after_clone(&*this, source);
        this
    }

    pub fn bad_size(&self) -> i64 {
        self.bad_size
    }

    /// The bytes after the length prefix, if the size has one.
    fn content<'a>(&self, data: DataPtr<'a>) -> &'a [u8] {
        let data = data.unwrap_or_default();
        let prefix = match self.bad_size {
            BYTE_ARRAY_LENGTH_U32 => 4,
            BYTE_ARRAY_LENGTH_U16 => 2,
            BYTE_ARRAY_LENGTH_U8 => 1,
            _ => 0,
        };
        data.get(prefix..).unwrap_or_default()
    }

    /// Port of `ToStringInternal` without the report mode analysis.
    fn to_string_internal(&self, data: DataPtr, element: ElementArg) -> String {
        let content = self.content(data);
        let mut result = String::with_capacity(content.len() * 3);
        for (index, byte) in content.iter().enumerate() {
            if index > 0 {
                result.push(' ');
            }
            result.push_str(&format!("{byte:02X}"));
        }
        self.used(element, &result);
        result
    }
}

impl ByteArrayDef {
    /// The tail of `FromEditValue` and `FromNativeValue`: the bytes after the
    /// length prefix, cut or extended to a fixed size.
    // UPSTREAM-QUIRK: the length prefix itself is not written; it keeps the
    // bytes it had, or zeros for new storage.
    fn store_bytes(&self, element: ElementArg, mut bytes: Vec<u8>) -> Result<(), EditError> {
        let prefix = match self.bad_size {
            BYTE_ARRAY_LENGTH_U32 => 4,
            BYTE_ARRAY_LENGTH_U16 => 2,
            BYTE_ARRAY_LENGTH_U8 => 1,
            _ => 0,
        };
        if self.bad_size > 0 {
            bytes.resize(self.bad_size as usize, 0);
        }
        let (element, mut storage) = request_storage(element, bytes.len() + prefix)?;
        storage[prefix..].copy_from_slice(&bytes);
        element.commit_storage(storage);
        Ok(())
    }
}

/// Reads a little-endian length prefix. Bytes that are missing read as zero,
/// where upstream reads past the end of the data.
fn read_prefix(data: &[u8], size: usize) -> i64 {
    let mut bytes = [0u8; 8];
    let available = data.len().min(size);
    bytes[..available].copy_from_slice(&data[..available]);
    i64::from_le_bytes(bytes)
}

impl Def for ByteArrayDef {
    value_def_plumbing!(Def);

    /// Port of `TwbByteArrayDef.CanAssign`: a byte array that fits.
    fn can_assign(&self, _element: ElementArg, _index: i32, def: Option<&dyn Def>) -> bool {
        if super::def::def_dont_assign(self) {
            return false;
        }
        let Some(other) = def.and_then(|def| def.as_byte_array_def()) else {
            return false;
        };
        self.bad_size <= 0
            || other.get_is_variable_size()
            || i64::from(other.get_default_size(None, None)) <= self.bad_size
    }

    fn get_def_type(&self) -> DefType {
        DefType::dtByteArray
    }

    fn get_def_type_name(&self) -> String {
        if self.bad_size > 0 {
            format!("{} Bytes Array", self.bad_size)
        } else if self.bad_count_callback.is_some() {
            "Variable Size Byte Array".to_owned()
        } else {
            match self.bad_size {
                BYTE_ARRAY_LENGTH_U32 => "Variable Size Byte Array with four bytes length",
                BYTE_ARRAY_LENGTH_U16 => "Variable Size Byte Array with two bytes length",
                BYTE_ARRAY_LENGTH_U8 => "Variable Size Byte Array with one byte length",
                BYTE_ARRAY_NULL => "Null",
                0 => "Filler for remaining data",
                _ => "",
            }
            .to_owned()
        }
    }

    fn as_byte_array_def(&self) -> Option<&ByteArrayDef> {
        Some(self)
    }
}

impl NamedDef for ByteArrayDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for ByteArrayDef {
    value_def_plumbing!(ValueDef);

    fn to_string(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = self.to_string_internal(data, element);
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToStr);
        }
        result
    }

    fn to_sort_key(&self, data: DataPtr, element: ElementArg, _extended: bool) -> String {
        let mut result = self.to_string_internal(data, element);
        if self.def.def_flags.contains(DefFlag::dfZeroSortKey) {
            if !result.is_empty() {
                result = "0".repeat(length(&result));
            }
        } else if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToSortKey);
        }
        result
    }

    fn get_size(&self, data: DataPtr, element: ElementArg) -> i32 {
        let mut result: i32 = match data {
            Some([]) => 0,
            _ if self.bad_count_callback.is_some() => {
                let callback = self.bad_count_callback.as_ref().expect("tested above");
                callback(data, element) as i32
            }
            Some(bytes) => match self.bad_size {
                BYTE_ARRAY_LENGTH_U32 => (read_prefix(bytes, 4) + 4) as i32,
                BYTE_ARRAY_LENGTH_U16 => (read_prefix(bytes, 2) + 2) as i32,
                BYTE_ARRAY_LENGTH_U8 => (read_prefix(bytes, 1) + 1) as i32,
                BYTE_ARRAY_NULL => 0,
                0 => i32::MAX,
                size => size as i32,
            },
            None => self.bad_size.max(0) as i32,
        };
        if result > 0 {
            // Overflows for the filler with a terminator, as upstream.
            result = result.wrapping_add(i32::from(self.nd.nd_terminator));
        }
        result
    }

    fn get_default_size(&self, _data: DataPtr, _element: ElementArg) -> i32 {
        let result = match self.bad_size {
            size if size >= 0 => size as i32,
            BYTE_ARRAY_LENGTH_U32 => 4,
            BYTE_ARRAY_LENGTH_U16 => 2,
            BYTE_ARRAY_LENGTH_U8 => 1,
            _ => 0,
        };
        if result > 0 {
            result + i32::from(self.nd.nd_terminator)
        } else {
            result
        }
    }

    fn get_is_variable_size_internal(&self) -> bool {
        self.bad_size <= 0
    }

    fn to_edit_value(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = self.to_string_internal(data, element);
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToEditValue);
        }
        result
    }

    fn to_native_value(&self, data: DataPtr, _element: ElementArg) -> Variant {
        Variant::Bytes(self.content(data).to_vec())
    }

    /// Port of `TwbByteArrayDef.FromEditValue`: pairs of hexadecimal digits,
    /// separated by blanks, commas or semicolons.
    fn from_edit_value(&self, _data: DataPtr, element: ElementArg, value: &str) -> Result<(), EditError> {
        let chars: Vec<char> = value.chars().collect();
        let mut bytes = Vec::with_capacity(chars.len() / 2);
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                ' ' | ',' | ';' => i += 1,
                c if c.is_ascii_hexdigit() => {
                    if i + 1 == chars.len() {
                        return Err("Unexpected end of value. Single digit in hexadecimal pair".to_owned());
                    }
                    let next = chars[i + 1];
                    if !next.is_ascii_hexdigit() {
                        return Err(format!(
                            "\"{next}\" at position {} is not a valid character for {}",
                            i + 2,
                            self.get_name()
                        ));
                    }
                    bytes.push((c.to_digit(16).unwrap_or(0) * 16 + next.to_digit(16).unwrap_or(0)) as u8);
                    i += 2;
                }
                other => {
                    return Err(format!(
                        "\"{other}\" at position {} is not a valid character for {}",
                        i + 1,
                        self.get_name()
                    ));
                }
            }
        }
        self.store_bytes(element, bytes)
    }

    fn from_native_value(&self, _data: DataPtr, element: ElementArg, value: Variant) -> Result<(), EditError> {
        match value {
            Variant::Bytes(bytes) => self.store_bytes(element, bytes),
            Variant::Empty => self.store_bytes(element, Vec::new()),
            _ => Err("Could not convert variant into type (Array Byte)".to_owned()),
        }
    }

    fn set_to_default(&self, data: DataPtr, element: ElementArg) -> Result<bool, EditError> {
        if self.set_to_default_callback(data, element) {
            return Ok(true);
        }
        if let Some(result) = self.set_to_default_native_value(data, element) {
            return result;
        }
        let default = match self.vd.vd_default_edit_value.load().as_deref() {
            Some(default) if self.vd.vd_states.contains(ValueDefState::vdsHasDefaultEditValue) => default.clone(),
            _ => {
                let size = self.get_size(data, element);
                if size > 0 && size < i32::MAX {
                    vec!["00"; size as usize].join(" ")
                } else {
                    String::new()
                }
            }
        };
        let changed = data.is_none() || self.to_string(data, element) != default;
        if changed {
            self.from_edit_value(data, element, &default)?;
        }
        Ok(changed)
    }

    fn get_is_editable(&self, _data: DataPtr, _element: ElementArg) -> bool {
        !(self.def.def_internal_edit_only() && !is_internal_edit())
    }

    fn get_edit_type(&self, data: DataPtr, element: ElementArg) -> EditType {
        let mut text = String::new();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut text, data, element, CallbackType::ctEditType);
        }
        if text.eq_ignore_ascii_case("ComboBox") {
            EditType::etComboBox
        } else if text.eq_ignore_ascii_case("CheckComboBox") {
            EditType::etCheckComboBox
        } else {
            EditType::etDefault
        }
    }

    fn get_edit_info(&self, data: DataPtr, element: ElementArg) -> Vec<String> {
        if let Some(edit_info) = self.vd.vd_edit_info.load().as_deref() {
            return edit_info.clone();
        }
        match self.nd.nd_to_str.load().as_deref() {
            Some(to_str) => {
                let mut text = String::new();
                to_str(&mut text, data, element, CallbackType::ctEditInfo);
                comma_text(&text)
            }
            None => Vec::new(),
        }
    }
}

impl DefKind for ByteArrayDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::def::DefSetters;
    use super::super::globals::test_lock;
    use super::super::types::ConflictPriority;
    use super::*;

    fn def_with(size: i64, terminator: bool, count: Option<CountCallback>) -> Arc<ByteArrayDef> {
        ByteArrayDef::create(
            NamedDefArgs {
                priority: ConflictPriority::cpNormal,
                required: false,
                name: "Unknown".to_owned(),
                after_load: None,
                after_set: None,
                dont_show: None,
                get_cp: None,
                terminator,
            },
            size,
            count,
        )
    }

    #[test]
    fn hexadecimal_text() {
        let _guard = test_lock();
        let def = def_with(0, false, None);
        assert_eq!(def.to_string(Some(&[0x00, 0x0A, 0xFF]), None), "00 0A FF");
        assert_eq!(def.to_string(Some(&[]), None), "");
        assert_eq!(def.to_string(None, None), "");
        assert_eq!(def.to_sort_key(Some(&[1, 2]), None, false), "01 02");
        assert_eq!(def.to_native_value(Some(&[1, 2]), None), Variant::Bytes(vec![1, 2]));
        let zero_key = def_with(0, false, None).include_flag(DefFlag::dfZeroSortKey);
        assert_eq!(zero_key.to_sort_key(Some(&[1, 2]), None, false), "00000");
        assert!(def.get_def_flags().contains(DefFlag::dfSkipImplicitEdit));
    }

    #[test]
    fn length_prefixes_are_not_shown() {
        let _guard = test_lock();
        assert_eq!(
            def_with(BYTE_ARRAY_LENGTH_U32, false, None).to_string(Some(&[2, 0, 0, 0, 0xAA, 0xBB]), None),
            "AA BB"
        );
        assert_eq!(
            def_with(BYTE_ARRAY_LENGTH_U16, false, None).to_string(Some(&[2, 0, 0xAA, 0xBB]), None),
            "AA BB"
        );
        assert_eq!(
            def_with(BYTE_ARRAY_LENGTH_U8, false, None).to_string(Some(&[2]), None),
            ""
        );
    }

    #[test]
    fn sizes() {
        let _guard = test_lock();
        let fixed = def_with(8, false, None);
        assert_eq!(fixed.get_size(Some(&[1, 2]), None), 8);
        assert_eq!(fixed.get_size(Some(&[]), None), 0);
        assert_eq!(fixed.get_size(None, None), 8);
        assert_eq!(fixed.get_default_size(None, None), 8);
        assert!(!fixed.get_is_variable_size());
        assert_eq!(fixed.get_def_type_name(), "8 Bytes Array");

        let filler = def_with(0, false, None);
        assert_eq!(filler.get_size(Some(&[1]), None), i32::MAX);
        assert_eq!(filler.get_size(None, None), 0);
        assert!(filler.get_is_variable_size());
        assert_eq!(filler.get_def_type_name(), "Filler for remaining data");

        let prefixed = def_with(BYTE_ARRAY_LENGTH_U16, true, None);
        assert_eq!(prefixed.get_size(Some(&[3, 0, 9, 9, 9, 0]), None), 6);
        assert_eq!(prefixed.get_size(None, None), 0);
        assert_eq!(prefixed.get_default_size(None, None), 3);

        let null = def_with(BYTE_ARRAY_NULL, false, None);
        assert_eq!(null.get_size(Some(&[1]), None), 0);
        assert_eq!(null.get_def_type_name(), "Null");

        let counted: CountCallback = Arc::new(|data, _| data.map_or(5, |data| u32::from(data[0])));
        let counted = def_with(0, false, Some(counted));
        assert_eq!(counted.get_size(Some(&[3, 0]), None), 3);
        assert_eq!(counted.get_size(None, None), 5);
        assert_eq!(counted.get_def_type_name(), "Variable Size Byte Array");
        assert_eq!(ByteArrayDef::clone_from(&counted).get_size(None, None), 5);
    }
}
