// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbEnumDef` with `TwbKey2Data6EnumDef` and `TwbData6Key2EnumDef`.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, Weak};

use super::def::{Def, DefBase, DefKind, DefRef, NamedDef, NamedDefBase};
use super::element::ElementArg;
use super::formaters::{editable_unless_internal_only, formater_impls, formater_plumbing};
use super::globals::{report_mode, report_unknown_enums, show_flag_enum_value};
use super::integer::{IntegerDefFormater, integer_def_formater_create};
use super::misc::{get_unknown_int_string, int_to_hex64};
use super::types::{DefType, EditType};

/// A name of an enumeration that is not in the dense list. Upstream `TwbSparseName`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SparseName {
    pub sn_index: i64,
    pub sn_name: String,
    pub sn_summary: String,
}

impl SparseName {
    pub fn new(index: i64, name: &str) -> Self {
        Self::with_summary(index, name, "")
    }

    pub fn with_summary(index: i64, name: &str, summary: &str) -> Self {
        Self {
            sn_index: index,
            sn_name: name.to_owned(),
            sn_summary: summary.to_owned(),
        }
    }
}

/// The three upstream enumeration classes. They differ in `ToString`,
/// `ToSortKey` and the class name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnumClass {
    /// `TwbEnumDef`
    Enum,
    /// `TwbKey2Data6EnumDef`: the two high bits are a size, the six low bits the value.
    Key2Data6,
    /// `TwbData6Key2EnumDef`
    Data6Key2,
}

impl EnumClass {
    fn class_name(self) -> &'static str {
        match self {
            EnumClass::Enum => "TwbEnumDef",
            EnumClass::Key2Data6 => "TwbKey2Data6EnumDef",
            EnumClass::Data6Key2 => "TwbData6Key2EnumDef",
        }
    }
}

/// Key of the name dictionary, which upstream compares with `TIStringComparer.Ordinal`.
fn dictionary_key(name: &str) -> String {
    name.to_uppercase()
}

/// Upstream `TwbEnumDef`: names for the values of an integer.
pub struct EnumDef {
    self_ref: Weak<EnumDef>,
    def: DefBase,
    nd: NamedDefBase,
    class: EnumClass,
    en_names: Vec<String>,
    en_summaries: Vec<String>,
    en_sparse_names: Vec<SparseName>,
    /// Indices into `en_sparse_names`, ordered by `sn_index`.
    en_sparse_names_map: Vec<usize>,
    en_edit_info: Vec<String>,
    en_dictionary: HashMap<String, i64>,
    /// Unknown value to the paths of up to 10 elements that have it, with a
    /// count per path. Filled in report mode.
    unknown_enums: Mutex<BTreeMap<String, BTreeMap<String, i32>>>,
}

impl EnumDef {
    /// Port of `TwbEnumDef.Create`.
    ///
    /// With `has_summary`, `names` holds pairs of name and summary. The summary
    /// of a sparse name is used only with `has_summary`.
    pub fn create(class: EnumClass, has_summary: bool, names: &[&str], sparse_names: &[SparseName]) -> Arc<Self> {
        let step_size = if has_summary { 2 } else { 1 };
        assert!(names.len().is_multiple_of(step_size));
        let count = names.len() / step_size;
        let mut en_dictionary = HashMap::with_capacity(count + sparse_names.len());
        let mut edit_info = Vec::new();
        let mut en_names = Vec::with_capacity(count);
        let mut en_summaries = Vec::with_capacity(count);
        let mut add_edit_info = |name: &str, index: i64| {
            if show_flag_enum_value() {
                edit_info.push(format!("{name} ({index})"));
            } else {
                edit_info.push(name.to_owned());
            }
        };
        for i in 0..count {
            let mut name = names[i * step_size].to_owned();
            if !name.is_empty() {
                if en_dictionary.contains_key(&dictionary_key(&name)) {
                    name = format!("{name}@{i}");
                }
                en_dictionary.insert(dictionary_key(&name), i as i64);
                add_edit_info(&name, i as i64);
            }
            let mut summary = if has_summary {
                names[i * step_size + 1].to_owned()
            } else {
                String::new()
            };
            if summary.is_empty() {
                summary = name.clone();
            }
            en_names.push(name);
            en_summaries.push(summary);
        }
        let mut en_sparse_names = Vec::with_capacity(sparse_names.len());
        for sparse in sparse_names {
            let mut sparse = sparse.clone();
            if !has_summary || sparse.sn_summary.is_empty() {
                sparse.sn_summary = sparse.sn_name.clone();
            }
            if !sparse.sn_name.is_empty() {
                if en_dictionary.contains_key(&dictionary_key(&sparse.sn_name)) {
                    sparse.sn_name = format!("{}@{}", sparse.sn_name, sparse.sn_index);
                }
                en_dictionary.insert(dictionary_key(&sparse.sn_name), sparse.sn_index);
                add_edit_info(&sparse.sn_name, sparse.sn_index);
            }
            en_sparse_names.push(sparse);
        }
        // Upstream sorts with a case-insensitive string list.
        edit_info.sort_by_key(|item| item.to_ascii_uppercase());
        Self::new(class, en_names, en_summaries, en_sparse_names, edit_info, en_dictionary)
    }

    fn new(
        class: EnumClass,
        en_names: Vec<String>,
        en_summaries: Vec<String>,
        en_sparse_names: Vec<SparseName>,
        en_edit_info: Vec<String>,
        en_dictionary: HashMap<String, i64>,
    ) -> Arc<Self> {
        let mut en_sparse_names_map: Vec<usize> = (0..en_sparse_names.len()).collect();
        // A stable sort, as the merge sort upstream.
        en_sparse_names_map.sort_by_key(|&index| en_sparse_names[index].sn_index);
        let (def, nd) = integer_def_formater_create(class.class_name());
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            class,
            en_names,
            en_summaries,
            en_sparse_names,
            en_sparse_names_map,
            en_edit_info,
            en_dictionary,
            unknown_enums: Mutex::new(BTreeMap::new()),
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbEnumDef.Clone`, which copies the fields of the source.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::new(
            source.class,
            source.en_names.clone(),
            source.en_summaries.clone(),
            source.en_sparse_names.clone(),
            source.en_edit_info.clone(),
            source.en_dictionary.clone(),
        );
        NamedDefBase::after_clone(&*this, source);
        this
    }

    /// Port of `FindSparseName`: the position in the sorted map of the first
    /// sparse name with `search_index`, if there is one.
    fn find_sparse_name(&self, search_index: i64) -> Option<&SparseName> {
        let position = self
            .en_sparse_names_map
            .partition_point(|&index| self.en_sparse_names[index].sn_index < search_index);
        self.en_sparse_names_map
            .get(position)
            .map(|&index| &self.en_sparse_names[index])
            .filter(|sparse| sparse.sn_index == search_index)
    }

    /// The dense name of `int`, or `""`.
    fn dense_name(&self, int: i64) -> &str {
        usize::try_from(int)
            .ok()
            .and_then(|index| self.en_names.get(index))
            .map_or("", String::as_str)
    }

    /// Upstream `GetName`.
    pub fn get_name_of(&self, index: i64) -> String {
        let name = self.dense_name(index);
        if !name.is_empty() {
            return name.to_owned();
        }
        self.find_sparse_name(index)
            .map_or_else(String::new, |sparse| sparse.sn_name.clone())
    }

    pub fn get_name_count(&self) -> i32 {
        self.en_names.len() as i32
    }

    /// Upstream `FindName`: the value with this name, ignoring case.
    pub fn find_name(&self, name: &str) -> Option<i64> {
        self.en_dictionary.get(&dictionary_key(name)).copied()
    }

    fn report_unknown(&self, value: &str, element: ElementArg) {
        if !(report_mode() && report_unknown_enums()) {
            return;
        }
        let mut unknown_enums = self.unknown_enums.lock().unwrap();
        let paths = unknown_enums.entry(value.to_owned()).or_default();
        if paths.len() < 10 {
            // Upstream fails without an element.
            let path = element.map(|element| element.get_full_path()).unwrap_or_default();
            *paths.entry(path).or_default() += 1;
        }
    }

    /// Port of the `ToString` overload of `IwbStringDefFormater`.
    pub fn string_to_string(&self, string: &str, element: ElementArg, _for_summary: bool) -> String {
        if string.is_empty() {
            return String::new();
        }
        if let Some(index) = self.find_name(string) {
            return self.get_name_of(index);
        }
        let result = format!("<Unknown: {string}>");
        self.report_unknown(&result, element);
        result
    }

    /// Port of the `ToSortKey` overload of `IwbStringDefFormater`.
    pub fn string_to_sort_key(&self, string: &str, _element: ElementArg) -> String {
        self.string_to_edit_value(string, _element)
    }

    /// Port of the `Check` overload of `IwbStringDefFormater`.
    pub fn string_check(&self, string: &str, _element: ElementArg) -> String {
        if string.is_empty() || self.find_name(string).is_some() {
            String::new()
        } else {
            format!("<Unknown: {string}>")
        }
    }

    /// Port of the `ToEditValue` overload of `IwbStringDefFormater`.
    pub fn string_to_edit_value(&self, string: &str, _element: ElementArg) -> String {
        if string.is_empty() {
            return String::new();
        }
        match self.find_name(string) {
            Some(index) => self.get_name_of(index),
            None => string.to_owned(),
        }
    }
}

impl Def for EnumDef {
    formater_plumbing!();

    fn get_def_type(&self) -> DefType {
        DefType::dtIntegerFormater
    }

    fn get_def_type_name(&self) -> String {
        if self.en_names.is_empty() {
            self.class.class_name().to_owned()
        } else {
            format!("({})", self.en_names.join(","))
        }
    }

    fn as_enum_def(&self) -> Option<&EnumDef> {
        Some(self)
    }
}

formater_impls!(EnumDef);

impl IntegerDefFormater for EnumDef {
    fn to_string(&self, int: i64, element: ElementArg, for_summary: bool) -> String {
        match self.class {
            EnumClass::Enum => {}
            EnumClass::Key2Data6 => {
                // Pascal `shr` is a logical shift.
                let key = ((int as u64) >> 6) as i32;
                let value = int & 0x3f;
                let mut result = match self.en_names.get(value as usize) {
                    Some(name) => name.clone(),
                    None => format!("Bad enum index: {value} [{}]", int_to_hex64(value, 2)),
                };
                result.push_str(match key {
                    0 => " Small size",
                    1 => " Medium size",
                    2 => " Large size",
                    _ => "",
                });
                return result;
            }
            EnumClass::Data6Key2 => {
                return if int < 1 << 6 {
                    format!("{int} Small size")
                } else if int < 1 << 14 {
                    format!("{int} Medium size")
                } else if int < 1 << 22 {
                    format!("{int} Large size")
                } else {
                    "0 Null size".to_owned()
                };
            }
        }
        let mut result = String::new();
        if usize::try_from(int).is_ok_and(|index| index < self.en_names.len()) {
            let index = int as usize;
            result = if for_summary {
                self.en_summaries[index].clone()
            } else {
                self.en_names[index].clone()
            };
            if show_flag_enum_value() && !result.is_empty() {
                result.push_str(&format!(" ({int})"));
            }
        }
        if result.is_empty() {
            match self.find_sparse_name(int) {
                Some(sparse) => {
                    result = if for_summary {
                        sparse.sn_summary.clone()
                    } else {
                        sparse.sn_name.clone()
                    };
                    if show_flag_enum_value() {
                        result.push_str(&format!(" ({})", sparse.sn_index));
                    }
                }
                None => {
                    result = get_unknown_int_string(int);
                    self.report_unknown(&result, element);
                }
            }
        }
        self.used(element, &result);
        result
    }

    fn to_sort_key(&self, int: i64, _element: ElementArg) -> String {
        match self.class {
            // Empty: the integer definition makes the key.
            EnumClass::Enum => String::new(),
            EnumClass::Key2Data6 | EnumClass::Data6Key2 => int_to_hex64(int, 2),
        }
    }

    fn check(&self, int: i64, _element: ElementArg) -> String {
        if self.get_name_of(int).is_empty() {
            get_unknown_int_string(int)
        } else {
            String::new()
        }
    }

    fn get_edit_type(&self, _element: ElementArg) -> EditType {
        EditType::etComboBox
    }

    fn get_edit_info(&self, _element: ElementArg) -> Vec<String> {
        self.en_edit_info.clone()
    }

    fn to_edit_value(&self, int: i64, _element: ElementArg) -> String {
        let suffix = |index: i64| {
            if show_flag_enum_value() {
                format!(" ({index})")
            } else {
                String::new()
            }
        };
        let name = self.dense_name(int);
        // UPSTREAM-QUIRK: with wbShowFlagEnumValue an empty dense name still gets
        // its suffix, so the sparse names are not searched for that value.
        if usize::try_from(int).is_ok_and(|index| index < self.en_names.len()) {
            let result = format!("{name}{}", suffix(int));
            if !result.is_empty() {
                return result;
            }
        }
        match self.find_sparse_name(int) {
            Some(sparse) if !sparse.sn_name.is_empty() || show_flag_enum_value() => {
                format!("{}{}", sparse.sn_name, suffix(sparse.sn_index))
            }
            _ => format!("{int}{}", suffix(int)),
        }
    }

    fn get_is_editable(&self, _int: i64, _element: ElementArg) -> bool {
        editable_unless_internal_only(&self.def)
    }
}

#[cfg(test)]
mod tests {
    use super::super::globals::{set_show_flag_enum_value, test_lock};
    use super::*;

    fn sample() -> Arc<EnumDef> {
        EnumDef::create(
            EnumClass::Enum,
            false,
            &["None", "Armor", "", "Weapon"],
            &[
                SparseName::new(100, "Special"),
                SparseName::new(-1, "Any"),
                SparseName::new(2, "Two"),
            ],
        )
    }

    #[test]
    fn names_and_sparse_names() {
        let _guard = test_lock();
        let def = sample();
        assert_eq!(def.to_string(1, None, false), "Armor");
        assert_eq!(def.to_string(2, None, false), "Two");
        assert_eq!(def.to_string(100, None, false), "Special");
        assert_eq!(def.to_string(-1, None, false), "Any");
        assert_eq!(def.to_string(5, None, false), "<Unknown: 5 $5>");
        assert_eq!(def.check(5, None), "<Unknown: 5 $5>");
        assert_eq!(def.check(100, None), "");
        assert_eq!(def.to_sort_key(1, None), "");
        assert_eq!(def.to_edit_value(3, None), "Weapon");
        assert_eq!(def.to_edit_value(2, None), "Two");
        assert_eq!(def.to_edit_value(7, None), "7");
        assert_eq!(def.get_def_type_name(), "(None,Armor,,Weapon)");
        assert_eq!(def.get_name(), "TwbEnumDef");
        assert_eq!(def.get_name_count(), 4);
        assert_eq!(def.find_name("weapon"), Some(3));
        assert_eq!(def.find_name("special"), Some(100));
        assert_eq!(def.find_name("Missing"), None);
        assert_eq!(
            def.get_edit_info(None),
            ["Any", "Armor", "None", "Special", "Two", "Weapon"]
        );
        assert_eq!(def.get_edit_type(None), EditType::etComboBox);
    }

    #[test]
    fn show_values() {
        let _guard = test_lock();
        set_show_flag_enum_value(true);
        let def = sample();
        assert_eq!(def.to_string(1, None, false), "Armor (1)");
        assert_eq!(def.to_string(2, None, false), "Two (2)");
        assert_eq!(def.to_edit_value(100, None), "Special (100)");
        assert_eq!(def.to_edit_value(2, None), " (2)");
        assert_eq!(def.get_edit_info(None)[0], "Any (-1)");
    }

    #[test]
    fn summaries_and_duplicates() {
        let _guard = test_lock();
        let def = EnumDef::create(
            EnumClass::Enum,
            true,
            &["Equal to", "==", "Same", "", "same", "~"],
            &[SparseName::with_summary(9, "Same", "S9")],
        );
        assert_eq!(def.to_string(0, None, true), "==");
        assert_eq!(def.to_string(0, None, false), "Equal to");
        assert_eq!(def.to_string(1, None, true), "Same");
        // The second and third use of a name get the value appended.
        assert_eq!(def.to_string(2, None, false), "same@2");
        assert_eq!(def.to_string(9, None, false), "Same@9");
        assert_eq!(def.to_string(9, None, true), "S9");
        assert_eq!(def.find_name("SAME"), Some(1));
        assert_eq!(def.find_name("same@9"), Some(9));

        let copy = EnumDef::clone_from(&def);
        assert_eq!(copy.to_string(9, None, true), "S9");
        assert_eq!(copy.find_name("same@2"), Some(2));
    }

    #[test]
    fn string_formater() {
        let _guard = test_lock();
        let def = sample();
        assert_eq!(def.string_to_string("armor", None, false), "Armor");
        assert_eq!(def.string_to_string("shield", None, false), "<Unknown: shield>");
        assert_eq!(def.string_to_string("", None, false), "");
        assert_eq!(def.string_check("shield", None), "<Unknown: shield>");
        assert_eq!(def.string_check("ARMOR", None), "");
        assert_eq!(def.string_to_sort_key("ARMOR", None), "Armor");
        assert_eq!(def.string_to_edit_value("shield", None), "shield");
    }

    #[test]
    fn key_and_data_classes() {
        let _guard = test_lock();
        let def = EnumDef::create(EnumClass::Key2Data6, false, &["A", "B"], &[]);
        assert_eq!(def.to_string(0x41, None, false), "B Medium size");
        assert_eq!(def.to_string(0x05, None, false), "Bad enum index: 5 [05] Small size");
        assert_eq!(def.to_string(0xC0, None, false), "A");
        assert_eq!(def.to_sort_key(0x41, None), "41");
        assert_eq!(def.get_name(), "TwbKey2Data6EnumDef");

        let def = EnumDef::create(EnumClass::Data6Key2, false, &[], &[]);
        assert_eq!(def.to_string(63, None, false), "63 Small size");
        assert_eq!(def.to_string(64, None, false), "64 Medium size");
        assert_eq!(def.to_string(1 << 14, None, false), "16384 Large size");
        assert_eq!(def.to_string(1 << 22, None, false), "0 Null size");
        assert_eq!(def.get_def_type_name(), "TwbData6Key2EnumDef");
    }
}
