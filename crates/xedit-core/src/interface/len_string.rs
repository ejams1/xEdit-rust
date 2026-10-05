// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbLenStringDef`: a string that is prefixed by its length.

use std::sync::{Arc, Weak};

use xedit_io::Encoding;

use super::def::{
    Def, DefBase, DefCell, DefKind, DefRef, NamedDef, NamedDefArgs, NamedDefBase, ValueDef, ValueDefBase, set_parent,
    value_def_plumbing,
};
use super::element::{DataPtr, ElementArg};
use super::globals::{check_expected_bytes, is_internal_edit};
use super::misc::{Variant, progress};
use super::string::{StringDefFormater, base_string_get_edit_info, base_string_get_edit_type, bsd_get_encoding};
use super::types::{CallbackType, DefFlag, DefType, EditType};

/// Upstream `TwbLenStringDef`.
pub struct LenStringDef {
    self_ref: Weak<LenStringDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
    /// Number of bytes before the text. A positive value is the size of the
    /// length. A negative value is the size of the length plus one separator
    /// byte: -2, -3 and -5 for a length of 1, 2 and 4 bytes.
    prefix: i32,
    bsd_encoding_override: DefCell<Encoding>,
    bsd_formater: DefCell<Arc<dyn StringDefFormater>>,
}

impl LenStringDef {
    /// Port of `TwbLenStringDef.Create`.
    pub fn create(args: NamedDefArgs, prefix: i32) -> Arc<Self> {
        let prefix = if (1..=5).contains(&prefix.abs()) { prefix } else { 4 };
        let (def, nd) = NamedDefBase::create(args);
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            vd: ValueDefBase::default(),
            prefix,
            bsd_encoding_override: DefCell::default(),
            bsd_formater: DefCell::default(),
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbLenStringDef.Clone` and `TwbBaseStringDef.AfterClone`.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(NamedDefBase::clone_args(source), source.prefix);
        ValueDefBase::after_clone(&*this, source);
        this.bsd_encoding_override.assign(&source.bsd_encoding_override);
        if let Some(formater) = source.bsd_formater.load().as_deref() {
            let parent: Weak<dyn Def> = this.self_ref.clone();
            this.bsd_formater
                .set(Some(set_parent(formater.clone(), &parent, false)));
        }
        this
    }

    fn unlocked(self: Arc<Self>) -> Arc<Self> {
        if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        }
    }

    /// Port of `OverrideEncoding`.
    pub fn override_encoding(self: Arc<Self>, encoding: Option<Encoding>) -> Arc<Self> {
        let this = self.unlocked();
        this.bsd_encoding_override.set(encoding);
        this
    }

    /// Port of `SetFormater`.
    pub fn set_formater(self: Arc<Self>, formater: Option<Arc<dyn StringDefFormater>>) -> Arc<Self> {
        let this = self.unlocked();
        if let Some(formater) = formater {
            let parent: Weak<dyn Def> = this.self_ref.clone();
            this.bsd_formater.set(Some(set_parent(formater, &parent, false)));
        }
        this
    }

    /// Size of the length field.
    pub fn get_prefix_len(&self) -> i32 {
        match self.prefix {
            1 | -2 => 1,
            2 | -3 => 2,
            4 | -5 => 4,
            _ => 0,
        }
    }

    /// Number of bytes before the text.
    pub fn get_prefix_offset(&self) -> i32 {
        self.prefix.abs()
    }

    /// The stored length. Bytes that are missing read as zero, where upstream
    /// reads past the end of the data.
    fn get_prefix_value(&self, data: &[u8]) -> i32 {
        let size = self.get_prefix_len() as usize;
        let mut bytes = [0u8; 4];
        let available = data.len().min(size);
        bytes[..available].copy_from_slice(&data[..available]);
        i32::from_le_bytes(bytes)
    }

    /// Port of `ToStringInternal`.
    fn to_string_internal(&self, data: DataPtr, element: ElementArg) -> String {
        let bytes = data.unwrap_or_default();
        let offset = self.get_prefix_offset() as usize;
        if bytes.len() < offset + usize::from(self.nd.nd_terminator) {
            return String::new();
        }
        let size = self.get_prefix_value(bytes) as u32 as usize;
        // UPSTREAM-QUIRK: upstream limits the text to the size of the whole data,
        // prefix included, and so can read past the end. The port stops there.
        let len = bytes.len().min(size);
        let text = &bytes[offset..(offset + len).min(bytes.len())];
        let mut result = String::new();
        if len > 0 {
            let mut b = text;
            if self.def.def_flags.contains(DefFlag::dfHasZeroTerminator) && b.last() == Some(&0) {
                b = &b[..b.len() - 1];
            }
            match bsd_get_encoding(&self.def, &self.bsd_encoding_override, element).get_string(b) {
                Ok(text) => result = text,
                Err(error) => {
                    let hex: Vec<String> = b.iter().map(|byte| format!("{byte:02X}")).collect();
                    result = format!("{} <Error: Can't read string: [EEncodingError] {error}>", hex.join(" "));
                    // Upstream fails without an element.
                    let path = element.map(|element| element.get_full_path()).unwrap_or_default();
                    progress(&format!("[{path}] <Error reading string: [EEncodingError] {error}>"));
                }
            }
        }
        self.used(element, &result);
        result
    }
}

impl Def for LenStringDef {
    value_def_plumbing!(Def);

    fn get_def_type(&self) -> DefType {
        DefType::dtLenString
    }

    fn get_def_type_name(&self) -> String {
        if self.prefix > 0 {
            format!("string with length of {} bytes", self.prefix)
        } else {
            format!("Separated string with length of {} bytes", self.get_prefix_len())
        }
    }

    fn as_len_string_def(&self) -> Option<&LenStringDef> {
        Some(self)
    }

    fn init_from_parent_do_children(&self) {
        if let Some(formater) = self.bsd_formater.load().as_deref() {
            formater.init_from_parent();
        }
    }
}

impl NamedDef for LenStringDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for LenStringDef {
    value_def_plumbing!(ValueDef);

    fn to_string(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = self.to_string_internal(data, element);
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToStr);
        }
        result
    }

    fn check(&self, data: DataPtr, element: ElementArg) -> String {
        let bytes = data.unwrap_or_default();
        let len = bytes.len() as u32;
        let offset = self.get_prefix_offset() as u32;
        let mut result = String::new();
        if len < offset {
            if check_expected_bytes() {
                result = format!("Expected at least {offset} bytes of data, found {len}");
            }
        } else {
            let size = (self.get_prefix_value(bytes) as u32).wrapping_add(offset);
            if len < size && check_expected_bytes() {
                result = format!("Expected {size} bytes of data, found {len}");
            }
        }
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctCheck);
        }
        result
    }

    fn get_size(&self, data: DataPtr, _element: ElementArg) -> i32 {
        let offset = self.get_prefix_offset();
        match data {
            None => offset,
            Some([]) => 0,
            Some(bytes) => {
                let available = bytes.len() as i32;
                let mut len = self.get_prefix_value(bytes);
                if len > 0 {
                    len = len.wrapping_add(offset).wrapping_add(i32::from(self.nd.nd_terminator));
                } else {
                    len = offset;
                }
                available.min(len)
            }
        }
    }

    fn get_default_size(&self, _data: DataPtr, _element: ElementArg) -> i32 {
        self.get_prefix_offset() + i32::from(self.nd.nd_terminator)
    }

    fn get_is_variable_size_internal(&self) -> bool {
        true
    }

    fn to_edit_value(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = self.to_string_internal(data, element);
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToEditValue);
        }
        result
    }

    fn to_native_value(&self, data: DataPtr, element: ElementArg) -> Variant {
        let mut result = self.to_string_internal(data, element);
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToNativeValue);
        }
        Variant::Str(result)
    }

    fn get_is_editable(&self, _data: DataPtr, _element: ElementArg) -> bool {
        !(self.def.def_internal_edit_only() && !is_internal_edit())
    }

    fn get_edit_type(&self, data: DataPtr, element: ElementArg) -> EditType {
        base_string_get_edit_type(&self.nd, &self.bsd_formater, data, element)
    }

    fn get_edit_info(&self, data: DataPtr, element: ElementArg) -> Vec<String> {
        base_string_get_edit_info(&self.nd, &self.vd, &self.bsd_formater, data, element)
    }
}

impl DefKind for LenStringDef {
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

    fn def_with(prefix: i32, terminator: bool) -> Arc<LenStringDef> {
        LenStringDef::create(
            NamedDefArgs {
                priority: ConflictPriority::cpNormal,
                required: false,
                name: "Name".to_owned(),
                after_load: None,
                after_set: None,
                dont_show: None,
                get_cp: None,
                terminator,
            },
            prefix,
        )
    }

    #[test]
    fn prefixes() {
        let _guard = test_lock();
        assert_eq!(def_with(1, false).to_string(Some(b"\x03abcdef"), None), "abc");
        assert_eq!(def_with(2, false).to_string(Some(b"\x02\0abcdef"), None), "ab");
        assert_eq!(def_with(4, false).to_string(Some(b"\x05\0\0\0hello"), None), "hello");
        // An invalid prefix size becomes four bytes.
        assert_eq!(def_with(9, false).to_string(Some(b"\x02\0\0\0hello"), None), "he");
        // A negative prefix has one separator byte after the length.
        let separated = def_with(-3, false);
        assert_eq!(separated.to_string(Some(b"\x02\0|abc"), None), "ab");
        assert_eq!(separated.get_prefix_len(), 2);
        assert_eq!(separated.get_def_type_name(), "Separated string with length of 2 bytes");
        assert_eq!(def_with(2, false).get_def_type_name(), "string with length of 2 bytes");
    }

    #[test]
    fn short_data() {
        let _guard = test_lock();
        let def = def_with(2, false);
        assert_eq!(def.to_string(Some(b"\x05"), None), "");
        assert_eq!(def.to_string(None, None), "");
        assert_eq!(def.to_string(Some(b"\x05\0ab"), None), "ab");
        assert_eq!(
            def.check(Some(b"\x05"), None),
            "Expected at least 2 bytes of data, found 1"
        );
        assert_eq!(def.check(Some(b"\x05\0ab"), None), "Expected 7 bytes of data, found 4");
        assert_eq!(def.check(Some(b"\x02\0ab"), None), "");
    }

    #[test]
    fn sizes() {
        let _guard = test_lock();
        let def = def_with(2, false);
        assert_eq!(def.get_size(None, None), 2);
        assert_eq!(def.get_size(Some(&[]), None), 0);
        assert_eq!(def.get_size(Some(b"\x03\0abcdef"), None), 5);
        assert_eq!(def.get_size(Some(b"\0\0abcdef"), None), 2);
        assert_eq!(def.get_size(Some(b"\x09\0ab"), None), 4);
        assert_eq!(def.get_default_size(None, None), 2);
        let terminated = def_with(1, true);
        assert_eq!(terminated.get_size(Some(b"\x02ab|rest"), None), 4);
        assert_eq!(terminated.get_default_size(None, None), 2);
        assert!(def.get_is_variable_size());
    }

    #[test]
    fn zero_terminator_flag_and_sort_key() {
        let _guard = test_lock();
        let def = def_with(2, false).include_flag(DefFlag::dfHasZeroTerminator);
        assert_eq!(def.to_string(Some(b"\x03\0ab\0"), None), "ab");
        assert_eq!(def.to_sort_key(Some(b"\x03\0ab\0"), None, false), "AB");
        assert_eq!(
            def.to_native_value(Some(b"\x03\0ab\0"), None),
            Variant::Str("ab".to_owned())
        );
        let copy = LenStringDef::clone_from(&def);
        assert_eq!(copy.to_string(Some(b"\x03\0ab\0"), None), "ab");
    }
}
