// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbBaseStringDef`, `TwbStringDef` and its descendants `TwbStringLCDef`,
//! `TwbStringKCDef`, `TwbStringScriptDef`, `TwbLStringDef` and
//! `TwbLStringKCDef`.
//!
//! `TwbStringMgefCodeDef` is used by Oblivion only and comes with that game.

use std::sync::{Arc, Weak};

use xedit_io::Encoding;

use super::def::{
    Def, DefBase, DefCell, DefKind, DefRef, NamedDef, NamedDefArgs, NamedDefBase, ValueDef, ValueDefBase, set_parent,
    value_def_plumbing,
};
use super::element::{DataPtr, ElementArg, ElementRef};
use super::formaters::comma_text;
use super::globals::{check_non_cpn_chars, encoding, encoding_trans, is_internal_edit, show_string_bytes};
use super::misc::{Variant, localization_get_value, progress};
use super::types::{CallbackType, DefFlag, DefType, EditType, TriBool};

/// Upstream `wbTerminator`.
pub const TERMINATOR: u8 = b'|';

/// Upstream `IwbStringDefFormater`: gives the text forms of a string value.
/// `TwbEnumDef` implements it. The methods have the prefix `str_`, because
/// that class also implements the integer formater methods of the same names.
pub trait StringDefFormater: Def {
    fn str_to_string(&self, string: &str, element: ElementArg, for_summary: bool) -> String;
    fn str_to_sort_key(&self, string: &str, element: ElementArg) -> String;
    fn str_check(&self, string: &str, element: ElementArg) -> String;
    fn str_get_edit_type(&self, element: ElementArg) -> EditType;
    fn str_get_edit_info(&self, element: ElementArg) -> Vec<String>;
    fn str_to_edit_value(&self, string: &str, element: ElementArg) -> String;
}

impl DefKind for dyn StringDefFormater {
    fn duplicate_same(&self) -> Arc<Self> {
        self.duplicate()
            .into_string_def_formater()
            .expect("the duplicate of a string formater is a string formater")
    }
}

/// Upstream `TwbStringTransformType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum StringTransformType {
    ttToString,
    ttToSortKey,
    ttCheck,
    ttToEditValue,
    ttFromEditValue,
    ttToNativeValue,
    ttFromNativeValue,
}

/// The upstream string classes. They differ in `TransformString`, `ToSortKey`
/// and, for the localized ones, in where the text comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringClass {
    /// `TwbStringDef`
    String,
    /// `TwbStringLCDef`: shown in lower case.
    LC,
    /// `TwbStringKCDef`: the sort key keeps its case.
    KC,
    /// `TwbStringScriptDef`: the sort key ignores blank lines and indentation.
    Script,
    /// `TwbLStringDef`: an ID into the string tables when the file is localized.
    LString,
    /// `TwbLStringKCDef`
    LStringKC,
    /// `TwbStringMgefCodeDef`: the code of an Oblivion magic effect, four
    /// characters or a dynamic code with the file it comes from.
    MgefCode,
}

impl StringClass {
    fn is_localized(self) -> bool {
        matches!(self, StringClass::LString | StringClass::LStringKC)
    }
}

/// Upstream `TwbStringDef`: a string that ends at a zero byte, at a fixed size
/// or at the end of the data.
pub struct StringDef {
    self_ref: Weak<StringDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
    class: StringClass,
    sd_size: i32,
    sd_forward: bool,
    bsd_encoding_override: DefCell<Encoding>,
    bsd_formater: DefCell<Arc<dyn StringDefFormater>>,
}

impl StringDef {
    /// Port of `TwbStringDef.Create` and `TwbLStringDef.Create`.
    ///
    /// `forward` ends the string at the first zero byte, for strings where the
    /// editor can leave characters after it.
    pub fn create(class: StringClass, args: NamedDefArgs, size: i32, forward: bool) -> Arc<Self> {
        let (def, nd) = NamedDefBase::create(args);
        if class.is_localized() {
            def.def_flags.include(DefFlag::dfTranslatable);
        }
        // Port of `TwbStringMgefCodeDef.AfterConstruction`.
        if class == StringClass::MgefCode {
            def.def_flags.include(DefFlag::dfCanContainFormID);
        }
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            vd: ValueDefBase::default(),
            class,
            sd_size: size,
            sd_forward: forward,
            bsd_encoding_override: DefCell::default(),
            bsd_formater: DefCell::default(),
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbStringDef.Clone` and `TwbBaseStringDef.AfterClone`.
    // UPSTREAM-QUIRK: Clone does not pass aForward, so a clone is never forward.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(source.class, NamedDefBase::clone_args(source), source.sd_size, false);
        ValueDefBase::after_clone(&*this, source);
        this.bsd_encoding_override.assign(&source.bsd_encoding_override);
        if let Some(formater) = source.bsd_formater.load().as_deref() {
            let parent: Weak<dyn Def> = this.self_ref.clone();
            this.bsd_formater
                .set(Some(set_parent(formater.clone(), &parent, false)));
        }
        this
    }

    pub fn get_string_size(&self) -> i32 {
        self.sd_size
    }

    pub fn class(&self) -> StringClass {
        self.class
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

    fn bsd_get_encoding(&self, element: ElementArg) -> Encoding {
        bsd_get_encoding(&self.def, &self.bsd_encoding_override, element)
    }

    /// Whether `element` stores an ID into the string tables.
    fn element_is_localized(element: ElementArg) -> bool {
        // Upstream fails without an element.
        let Some(element) = element else {
            return false;
        };
        match element.get_localized() {
            TriBool::tbFalse => false,
            TriBool::tbTrue => true,
            TriBool::tbUnknown => element.get_file().is_some_and(|file| file.get_is_localized()),
        }
    }

    /// Port of `TwbStringDef.ToStringNative`.
    fn string_to_string_native(&self, data: DataPtr, element: ElementArg, transform: StringTransformType) -> String {
        use StringTransformType::*;
        let mut result = String::new();
        let bytes = data.unwrap_or_default();
        let mut len = bytes.len();
        if len > 0 && self.nd.nd_terminator && bytes[len - 1] == TERMINATOR {
            len -= 1;
        }
        if self.sd_size > 0 && len > self.sd_size as usize {
            len = self.sd_size as usize;
        }
        if self.sd_forward {
            len = bytes[..len].iter().position(|&byte| byte == 0).unwrap_or(len);
        } else {
            while len > 0 && bytes[len - 1] == 0 {
                len -= 1;
            }
        }
        if len > 0 {
            let b = &bytes[..len];
            match self.bsd_get_encoding(element).get_string(b) {
                Ok(text) => {
                    if transform != ttCheck {
                        result = text;
                    }
                }
                Err(error) => {
                    if transform != ttCheck {
                        result = b.iter().map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(" ");
                    }
                    if transform == ttToString {
                        result.push_str(" <Error: ");
                    }
                    if matches!(transform, ttToString | ttCheck) {
                        result.push_str(&format!("Can't read string: [EEncodingError] {error}"));
                    }
                    if transform == ttToString {
                        result.push('>');
                    }
                    if transform != ttCheck {
                        // Upstream fails without an element.
                        let (form_id, path) = match element {
                            Some(element) => (
                                element
                                    .get_containing_main_record()
                                    .map(|record| record.get_load_order_form_id().to_string(false))
                                    .unwrap_or_default(),
                                element.get_path(),
                            ),
                            None => (String::new(), String::new()),
                        };
                        progress(&format!(
                            "[{form_id}] [{path}] <Error reading string: [EEncodingError] {error}>"
                        ));
                    }
                }
            }
            if check_non_cpn_chars()
                && matches!(transform, ttToString | ttCheck)
                && !self.def.def_flags.contains(DefFlag::dfTranslatable)
                && b.iter()
                    .any(|&byte| !(byte == 10 || byte == 13 || (32..=126).contains(&byte)))
            {
                if transform == ttToString {
                    result.push_str(" <Warning: ");
                }
                result.push_str("non-code page neutral character in non-translatable string");
                if transform == ttToString {
                    result.push('>');
                }
            }
        }
        self.used(element, &result);
        result
    }

    /// Port of `ToStringNative`, with the overrides of `TwbLStringDef` and
    /// `TwbStringMgefCodeDef`.
    fn to_string_native(&self, data: DataPtr, element: ElementArg, transform: StringTransformType) -> String {
        if self.class == StringClass::MgefCode {
            return self.mgef_code_to_string_native(data, element, transform);
        }
        if !(self.class.is_localized() && Self::element_is_localized(element)) {
            return self.string_to_string_native(data, element, transform);
        }
        let check = transform == StringTransformType::ttCheck;
        let bytes = data.unwrap_or_default();
        let Ok(id) = <[u8; 4]>::try_from(bytes) else {
            return if check {
                "lstring ID is not Int32".to_owned()
            } else {
                "<Error: lstring ID is not Int32>".to_owned()
            };
        };
        let id = u32::from_le_bytes(id);
        let (found, value) = localization_get_value(id, element);
        if !check {
            value
        } else if found {
            String::new()
        } else {
            format!("lstring ID [{id:08X}] could not be resolved")
        }
    }

    /// Port of `TwbStringMgefCodeDef.ToStringNative`.
    fn mgef_code_to_string_native(&self, data: DataPtr, element: ElementArg, transform: StringTransformType) -> String {
        use StringTransformType::*;
        let bytes = data.unwrap_or_default();
        let mut len = bytes.len();
        if self.sd_size > 0 && len > self.sd_size as usize {
            len = self.sd_size as usize;
        }
        if self.sd_forward {
            len = bytes[..len].iter().position(|&byte| byte == 0).unwrap_or(len);
        } else {
            while len > 0 && bytes[len - 1] == 0 {
                len -= 1;
            }
        }
        if len == 4 {
            // UPSTREAM-QUIRK: the loop tests the first character four times.
            let is_alpha = bytes[0].is_ascii_alphanumeric() || bytes[0] == b'_';
            if !is_alpha {
                let code = u32::from_le_bytes(bytes[..4].try_into().unwrap());
                if code & 0x8000_0000 != 0
                    && let Some(element) = element
                    && let Some(file) = element.get_file()
                {
                    if transform == ttCheck {
                        return String::new();
                    }
                    let file_id = (code & 0xFF) as i32;
                    let file_name = if file_id >= file.get_master_count(false) {
                        file.get_name()
                    } else {
                        file.get_master(file_id, false)
                            .map(|master| master.get_name())
                            .unwrap_or_default()
                    };
                    return format!("{file_name}:{}", (code & !0x8000_00FF) >> 8);
                }
                if transform == ttCheck {
                    return "Effect Code is neither alphanumeric nor dynamic".to_owned();
                }
                let mut result = format!("{code:08X}");
                if transform == ttToString {
                    result.push_str(" <Warning: Effect Code is neither alphanumeric nor dynamic>");
                }
                return result;
            }
        }
        let mut result = self.string_to_string_native(data, element, transform);
        let mut length = result.chars().count();
        if transform == ttCheck {
            if length == 0 {
                result = self.string_to_string_native(data, element, ttToString);
                length = result.chars().count();
            } else {
                return result;
            }
        }
        if length != 4 {
            match transform {
                ttToString => result.push_str(&format!(" <Warning: Expected 4 bytes but found {length}>")),
                ttCheck => return format!("Expected 4 bytes but found {length}: {result}"),
                _ => {}
            }
        }
        if transform == ttCheck {
            return String::new();
        }
        result
    }

    /// Port of `TransformString` with the overrides of the descendants.
    fn transform_string(&self, s: String, transform: StringTransformType) -> String {
        match self.class {
            StringClass::LC => {
                if transform == StringTransformType::ttCheck {
                    s
                } else {
                    // Delphi LowerCase changes only the letters A to Z.
                    s.to_ascii_lowercase()
                }
            }
            StringClass::Script => {
                if transform == StringTransformType::ttToSortKey {
                    script_sort_key(&s)
                } else {
                    s
                }
            }
            _ => {
                let mut result = s;
                if show_string_bytes() && transform == StringTransformType::ttToString {
                    let units: Vec<String> = result.encode_utf16().map(|unit| format!("{unit:04X}")).collect();
                    result = format!("{result} [{}]", units.join(" "));
                }
                result
            }
        }
    }

    fn to_string_transform(&self, data: DataPtr, element: ElementArg, transform: StringTransformType) -> String {
        self.transform_string(self.to_string_native(data, element, transform), transform)
    }

    fn string_get_size(&self, data: DataPtr) -> i32 {
        let terminator = i32::from(self.nd.nd_terminator);
        if self.sd_size > 0 {
            return self.sd_size + i32::from(self.def.def_flags.contains(DefFlag::dfHasZeroTerminator)) + terminator;
        }
        match data {
            None => 1 + terminator,
            Some(bytes) => match bytes.iter().position(|&byte| byte == 0) {
                Some(position) => terminator + position as i32 + 1,
                None => terminator + bytes.len() as i32,
            },
        }
    }

    fn string_get_default_size(&self) -> i32 {
        let terminator = i32::from(self.nd.nd_terminator);
        if self.sd_size > 0 {
            self.sd_size + i32::from(self.def.def_flags.contains(DefFlag::dfHasZeroTerminator)) + terminator
        } else {
            1 + terminator
        }
    }
}

/// The sort key of a script: its lines trimmed, without the empty ones. Port
/// of the `TStringList` round trip in `TwbStringScriptDef.TransformString`.
fn script_sort_key(s: &str) -> String {
    let mut result = String::new();
    let mut rest = s;
    while !rest.is_empty() {
        let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
        let line = rest[..end].trim_matches(|c: char| c <= ' ');
        if !line.is_empty() {
            result.push_str(line);
            result.push_str("\r\n");
        }
        rest = &rest[end..];
        if let Some(stripped) = rest.strip_prefix("\r\n") {
            rest = stripped;
        } else if !rest.is_empty() {
            rest = &rest[1..];
        }
    }
    result
}

impl Def for StringDef {
    value_def_plumbing!(Def);

    fn get_def_type(&self) -> DefType {
        if self.class.is_localized() {
            DefType::dtLString
        } else {
            DefType::dtString
        }
    }

    fn get_def_type_name(&self) -> String {
        if self.class.is_localized() {
            "Localized string".to_owned()
        } else {
            "Terminated string".to_owned()
        }
    }

    fn as_string_def(&self) -> Option<&StringDef> {
        Some(self)
    }

    fn into_string_def(self: Arc<Self>) -> Option<Arc<StringDef>> {
        Some(self)
    }

    fn init_from_parent_do_children(&self) {
        if let Some(formater) = self.bsd_formater.load().as_deref() {
            formater.init_from_parent();
        }
    }
}

impl NamedDef for StringDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for StringDef {
    value_def_plumbing!(ValueDef);

    fn to_string(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = self.to_string_transform(data, element, StringTransformType::ttToString);
        if let Some(formater) = self.bsd_formater.load().as_deref() {
            result = formater.str_to_string(&result, element, false);
        }
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToStr);
        }
        result
    }

    fn to_summary(&self, _depth: i32, data: DataPtr, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        let mut result = self.to_string_transform(data, element, StringTransformType::ttToString);
        if let Some(formater) = self.bsd_formater.load().as_deref() {
            result = formater.str_to_string(&result, element, true);
        }
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToSummary);
        }
        if links_to.is_none()
            && !result.is_empty()
            && let Some(element) = element
        {
            *links_to = element.get_links_to();
        }
        result
    }

    fn to_sort_key(&self, data: DataPtr, element: ElementArg, _extended: bool) -> String {
        let mut result = self.to_string_transform(data, element, StringTransformType::ttToSortKey);
        if !matches!(self.class, StringClass::KC | StringClass::LStringKC) {
            // Delphi UpperCase changes only the letters a to z.
            result = result.to_ascii_uppercase();
        }
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToSortKey);
        }
        result
    }

    fn check(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = self.to_string_transform(data, element, StringTransformType::ttCheck);
        if let Some(formater) = self.bsd_formater.load().as_deref() {
            result = formater.str_check(&result, element);
        }
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctCheck);
        }
        result
    }

    /// Port of `TwbStringMgefCodeDef.GetLinksTo`: the magic effect with the
    /// code as its editor ID.
    fn get_links_to(&self, data: DataPtr, element: ElementArg) -> Option<ElementRef> {
        if let Some(callback) = self.vd.vd_links_to_callback.load().as_deref() {
            return callback(element);
        }
        if self.class != StringClass::MgefCode {
            return None;
        }
        let file = element?.get_file()?;
        let key = self.to_string_transform(data, element, StringTransformType::ttToSortKey);
        file.get_record_by_editor_id(&key).map(|record| record as ElementRef)
    }

    fn get_size(&self, data: DataPtr, element: ElementArg) -> i32 {
        if self.class.is_localized() {
            match data {
                Some([]) => return 0,
                Some(bytes) if Self::element_is_localized(element) => return bytes.len().min(4) as i32,
                _ => {}
            }
        }
        self.string_get_size(data)
    }

    fn get_default_size(&self, _data: DataPtr, element: ElementArg) -> i32 {
        if self.class.is_localized() && Self::element_is_localized(element) {
            4
        } else {
            self.string_get_default_size()
        }
    }

    fn get_is_variable_size_internal(&self) -> bool {
        self.sd_size == 0
    }

    fn to_edit_value(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = self.to_string_transform(data, element, StringTransformType::ttToEditValue);
        if let Some(formater) = self.bsd_formater.load().as_deref() {
            result = formater.str_to_edit_value(&result, element);
        }
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToEditValue);
        }
        result
    }

    fn to_native_value(&self, data: DataPtr, element: ElementArg) -> Variant {
        let mut result = self.to_string_transform(data, element, StringTransformType::ttToNativeValue);
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

/// Port of `TwbBaseStringDef.bsdGetEncoding`.
pub(crate) fn bsd_get_encoding(def: &DefBase, encoding_override: &DefCell<Encoding>, element: ElementArg) -> Encoding {
    if let Some(encoding) = encoding_override.load().as_deref() {
        return *encoding;
    }
    let translatable = def.def_flags.contains(DefFlag::dfTranslatable);
    if let Some(file) = element.and_then(|element| element.get_file()) {
        return file.get_encoding(translatable);
    }
    if translatable { encoding_trans() } else { encoding() }
}

/// Port of `TwbBaseStringDef.GetEditType`.
pub(crate) fn base_string_get_edit_type(
    nd: &NamedDefBase,
    formater: &DefCell<Arc<dyn StringDefFormater>>,
    data: DataPtr,
    element: ElementArg,
) -> EditType {
    let mut result = match formater.load().as_deref() {
        Some(formater) => formater.str_get_edit_type(element),
        None => EditType::etDefault,
    };
    if let Some(to_str) = nd.nd_to_str.load().as_deref() {
        let mut text = match result {
            EditType::etComboBox => "ComboBox",
            EditType::etCheckComboBox => "CheckComboBox",
            EditType::etDefault => "",
        }
        .to_owned();
        to_str(&mut text, data, element, CallbackType::ctEditType);
        result = if text.eq_ignore_ascii_case("ComboBox") {
            EditType::etComboBox
        } else if text.eq_ignore_ascii_case("CheckComboBox") {
            EditType::etCheckComboBox
        } else {
            EditType::etDefault
        };
    }
    result
}

/// Port of `TwbBaseStringDef.GetEditInfo`.
pub(crate) fn base_string_get_edit_info(
    nd: &NamedDefBase,
    vd: &ValueDefBase,
    formater: &DefCell<Arc<dyn StringDefFormater>>,
    data: DataPtr,
    element: ElementArg,
) -> Vec<String> {
    if let Some(edit_info) = vd.vd_edit_info.load().as_deref() {
        return edit_info.clone();
    }
    let mut result = match formater.load().as_deref() {
        Some(formater) => formater.str_get_edit_info(element),
        None => Vec::new(),
    };
    if let Some(to_str) = nd.nd_to_str.load().as_deref() {
        let mut text = to_comma_text(&result);
        to_str(&mut text, data, element, CallbackType::ctEditInfo);
        result = comma_text(&text);
    }
    result
}

/// Port of reading `TStrings.CommaText`: items separated by commas, an item
/// with a blank, a comma or a quote is in double quotes.
pub fn to_comma_text(items: &[String]) -> String {
    if items.len() == 1 && items[0].is_empty() {
        return "\"\"".to_owned();
    }
    items
        .iter()
        .map(|item| {
            if item.chars().any(|c| c <= ' ' || c == ',' || c == '"') {
                format!("\"{}\"", item.replace('"', "\"\""))
            } else {
                item.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

impl DefKind for StringDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::enum_def::{EnumClass, EnumDef};
    use super::super::globals::{set_check_non_cpn_chars, set_show_string_bytes, test_lock};
    use super::super::types::ConflictPriority;
    use super::*;

    fn args(terminator: bool) -> NamedDefArgs {
        NamedDefArgs {
            priority: ConflictPriority::cpNormal,
            required: false,
            name: "Text".to_owned(),
            after_load: None,
            after_set: None,
            dont_show: None,
            get_cp: None,
            terminator,
        }
    }

    fn plain() -> Arc<StringDef> {
        StringDef::create(StringClass::String, args(false), 0, false)
    }

    #[test]
    fn zero_terminated() {
        let _guard = test_lock();
        let def = plain();
        assert_eq!(def.to_string(Some(b"Hello\0"), None), "Hello");
        assert_eq!(def.to_string(Some(b"Hello\0\0\0"), None), "Hello");
        assert_eq!(def.to_string(Some(b"He\0lo\0"), None), "He\0lo");
        assert_eq!(def.to_string(Some(b"Caf\xE9\0"), None), "Café");
        assert_eq!(def.to_string(None, None), "");
        assert_eq!(def.to_sort_key(Some(b"Hello\0"), None, false), "HELLO");
        assert_eq!(def.check(Some(b"Hello\0"), None), "");
        assert_eq!(def.to_native_value(Some(b"Hi\0"), None), Variant::Str("Hi".to_owned()));
        assert_eq!(def.get_size(Some(b"Hi\0rest"), None), 3);
        assert_eq!(def.get_size(Some(b"Hi"), None), 2);
        assert_eq!(def.get_size(None, None), 1);
        assert!(def.get_is_variable_size());
        assert_eq!(def.get_def_type_name(), "Terminated string");
    }

    #[test]
    fn forward_fixed_and_terminated() {
        let _guard = test_lock();
        let forward = StringDef::create(StringClass::String, args(false), 0, true);
        assert_eq!(forward.to_string(Some(b"He\0lo\0"), None), "He");
        // A clone is never forward.
        assert_eq!(
            StringDef::clone_from(&forward).to_string(Some(b"He\0lo\0"), None),
            "He\0lo"
        );
        let fixed = StringDef::create(StringClass::String, args(false), 4, false);
        assert_eq!(fixed.to_string(Some(b"ABCDEF"), None), "ABCD");
        assert_eq!(fixed.get_size(Some(b"ABCDEF"), None), 4);
        assert!(!fixed.get_is_variable_size());
        let terminated = StringDef::create(StringClass::String, args(true), 0, false);
        assert_eq!(terminated.to_string(Some(b"AB|"), None), "AB");
        assert_eq!(terminated.get_size(None, None), 2);
    }

    #[test]
    fn classes() {
        let _guard = test_lock();
        let lower = StringDef::create(StringClass::LC, args(false), 0, false);
        assert_eq!(lower.to_string(Some(b"MiXed\0"), None), "mixed");
        let keep = StringDef::create(StringClass::KC, args(false), 0, false);
        assert_eq!(keep.to_sort_key(Some(b"MiXed\0"), None, false), "MiXed");
        let script = StringDef::create(StringClass::Script, args(false), 0, false);
        assert_eq!(
            script.to_sort_key(Some(b"  begin\r\n\r\n\tx\nend\0"), None, false),
            "BEGIN\r\nX\r\nEND\r\n"
        );
        assert_eq!(script.to_string(Some(b" a\0"), None), " a");
        let localized = StringDef::create(StringClass::LString, args(false), 0, false);
        assert!(localized.get_def_flags().contains(DefFlag::dfTranslatable));
        assert_eq!(localized.get_def_type(), DefType::dtLString);
        // Without an element the data is a plain string.
        assert_eq!(localized.to_string(Some(b"Name\0"), None), "Name");
        assert_eq!(localized.get_size(Some(&[]), None), 0);
    }

    #[test]
    fn encoding_override_and_errors() {
        let _guard = test_lock();
        let utf8 = plain().override_encoding(Some(Encoding::Utf8));
        assert_eq!(utf8.to_string(Some("Café\0".as_bytes()), None), "Café");
        assert_eq!(
            utf8.to_string(Some(b"Caf\xE9\0"), None),
            "43 61 66 E9 <Error: Can't read string: [EEncodingError] No mapping for the Unicode character exists in \
             the target multi-byte code page>"
        );
        assert_eq!(
            utf8.check(Some(b"Caf\xE9\0"), None),
            "Can't read string: [EEncodingError] No mapping for the Unicode character exists in the target \
             multi-byte code page"
        );
        assert_eq!(utf8.to_edit_value(Some(b"Caf\xE9\0"), None), "43 61 66 E9");
    }

    #[test]
    fn options() {
        let _guard = test_lock();
        let def = plain();
        set_show_string_bytes(true);
        assert_eq!(def.to_string(Some(b"Hi\0"), None), "Hi [0048 0069]");
        assert_eq!(def.to_edit_value(Some(b"Hi\0"), None), "Hi");
        set_show_string_bytes(false);
        set_check_non_cpn_chars(true);
        assert_eq!(
            def.to_string(Some(b"Caf\xE9\0"), None),
            "Café <Warning: non-code page neutral character in non-translatable string>"
        );
        assert_eq!(
            def.check(Some(b"Caf\xE9\0"), None),
            "non-code page neutral character in non-translatable string"
        );
    }

    #[test]
    fn enum_formater() {
        let _guard = test_lock();
        let formater: Arc<dyn StringDefFormater> = EnumDef::create(EnumClass::Enum, false, &["Fire", "Frost"], &[]);
        let def = plain().set_formater(Some(formater));
        assert_eq!(def.to_string(Some(b"fire\0"), None), "Fire");
        assert_eq!(def.to_string(Some(b"shock\0"), None), "<Unknown: shock>");
        assert_eq!(def.check(Some(b"shock\0"), None), "");
        assert_eq!(def.get_edit_type(None, None), EditType::etComboBox);
        assert_eq!(def.get_edit_info(None, None), ["Fire", "Frost"]);
        let copy = StringDef::clone_from(&def);
        assert_eq!(copy.to_string(Some(b"frost\0"), None), "Frost");
    }

    #[test]
    fn comma_text_writing() {
        assert_eq!(to_comma_text(&[]), "");
        assert_eq!(to_comma_text(&[String::new()]), "\"\"");
        assert_eq!(
            to_comma_text(&["a".to_owned(), "b c".to_owned(), "d\"e".to_owned()]),
            "a,\"b c\",\"d\"\"e\""
        );
    }
}
