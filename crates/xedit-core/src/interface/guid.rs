// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbGuidDef`.

use std::sync::{Arc, Weak};

use super::def::{
    Def, DefBase, DefKind, DefRef, NamedDef, NamedDefArgs, NamedDefBase, ValueDef, ValueDefBase, value_def_plumbing,
};
use super::element::{DataPtr, ElementArg, ElementRef};
use super::formaters::comma_text;
use super::globals::is_internal_edit;
use super::misc::Variant;
use super::types::{CallbackType, DefType, EditType};

/// Upstream `TwbGuidDef`: 16 bytes shown as `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`.
pub struct GuidDef {
    self_ref: Weak<GuidDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
}

impl GuidDef {
    /// Port of the constructor that `TwbGuidDef` inherits from `TwbNamedDef`.
    pub fn create(args: NamedDefArgs) -> Arc<Self> {
        let (def, nd) = NamedDefBase::create(args);
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            vd: ValueDefBase::default(),
        });
        DefBase::after_construction(&*this);
        this
    }

    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(NamedDefBase::clone_args(source));
        ValueDefBase::after_clone(&*this, source);
        this
    }

    /// Port of `ToStringInternal`.
    // UPSTREAM-QUIRK: the text is two 64-bit integers in hexadecimal, so the
    // bytes of each half show in reverse order. That is not the usual text form
    // of a GUID.
    fn to_string_internal(&self, data: DataPtr) -> String {
        let bytes = data.unwrap_or_default();
        if bytes.len() < 16 {
            return String::new();
        }
        let first = u64::from_le_bytes(bytes[..8].try_into().expect("eight bytes"));
        let second = u64::from_le_bytes(bytes[8..16].try_into().expect("eight bytes"));
        let hex = format!("{first:016X}{second:016X}");
        format!(
            "{{{}-{}-{}-{}-{}}}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        )
    }

    fn with_callback(&self, mut result: String, data: DataPtr, element: ElementArg, callback: CallbackType) -> String {
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, callback);
        }
        result
    }
}

impl Def for GuidDef {
    value_def_plumbing!(Def);

    fn get_def_type(&self) -> DefType {
        DefType::dtGuid
    }

    fn get_def_type_name(&self) -> String {
        "GUID".to_owned()
    }

    fn as_guid_def(&self) -> Option<&GuidDef> {
        Some(self)
    }
}

impl NamedDef for GuidDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for GuidDef {
    value_def_plumbing!(ValueDef);

    fn to_string(&self, data: DataPtr, element: ElementArg) -> String {
        let result = self.with_callback(self.to_string_internal(data), data, element, CallbackType::ctToStr);
        self.used(element, &result);
        result
    }

    fn to_summary(&self, _depth: i32, data: DataPtr, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        let result = self.with_callback(self.to_string_internal(data), data, element, CallbackType::ctToSummary);
        if links_to.is_none()
            && !result.is_empty()
            && let Some(element) = element
        {
            *links_to = element.get_links_to();
        }
        result
    }

    fn get_size(&self, _data: DataPtr, _element: ElementArg) -> i32 {
        16
    }

    fn get_default_size(&self, _data: DataPtr, _element: ElementArg) -> i32 {
        16
    }

    fn to_edit_value(&self, data: DataPtr, element: ElementArg) -> String {
        self.with_callback(
            self.to_string_internal(data),
            data,
            element,
            CallbackType::ctToEditValue,
        )
    }

    fn to_native_value(&self, data: DataPtr, element: ElementArg) -> Variant {
        Variant::Str(self.with_callback(
            self.to_string_internal(data),
            data,
            element,
            CallbackType::ctToNativeValue,
        ))
    }

    fn get_is_editable(&self, _data: DataPtr, _element: ElementArg) -> bool {
        !(self.def.def_internal_edit_only() && !is_internal_edit())
    }

    fn get_edit_type(&self, data: DataPtr, element: ElementArg) -> EditType {
        let text = self.with_callback(String::new(), data, element, CallbackType::ctEditType);
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
        if self.nd.nd_to_str.is_assigned() {
            comma_text(&self.with_callback(String::new(), data, element, CallbackType::ctEditInfo))
        } else {
            Vec::new()
        }
    }
}

impl DefKind for GuidDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::globals::test_lock;
    use super::super::types::ConflictPriority;
    use super::*;

    #[test]
    fn text_form() {
        let _guard = test_lock();
        let def = GuidDef::create(NamedDefArgs {
            priority: ConflictPriority::cpNormal,
            required: false,
            name: "ID".to_owned(),
            after_load: None,
            after_set: None,
            dont_show: None,
            get_cp: None,
            terminator: false,
        });
        let data: Vec<u8> = (1..=16).collect();
        assert_eq!(
            def.to_string(Some(&data), None),
            "{08070605-0403-0201-100F-0E0D0C0B0A09}"
        );
        assert_eq!(def.to_string(Some(&data[..15]), None), "");
        assert_eq!(def.to_string(None, None), "");
        assert_eq!(
            def.to_sort_key(Some(&data), None, false),
            "{08070605-0403-0201-100F-0E0D0C0B0A09}"
        );
        assert_eq!(def.get_size(None, None), 16);
        assert_eq!(def.get_def_type_name(), "GUID");
        assert_eq!(GuidDef::clone_from(&def).get_name(), "ID");
    }
}
