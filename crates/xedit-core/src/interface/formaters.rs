// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! The small integer formaters: `TwbIntegerDefFormaterUnion`,
//! `TwbDumpIntegerDefFormater`, `TwbStr4`, `TwbDivDef`, `TwbDivFDef`,
//! `TwbMulDef` and `TwbCallbackDef`.

use std::sync::{Arc, Weak};

use super::def::{Def, DefBase, DefKind, DefRef, NamedDef, NamedDefBase, set_parent};
use super::element::{ElementArg, ElementRef};
use super::globals::is_internal_edit;
use super::integer::{IntegerDefFormater, integer_def_formater_create};
use super::misc::int_to_hex64;
use super::types::{CallbackType, DefFlag, DefType, EditType, Signature};
use crate::delphi::{float_to_str_f_fixed, round, str_to_float};

pub type IntegerDefFormaterUnionDecider = Arc<dyn Fn(ElementArg) -> i32 + Send + Sync>;
pub type IntToStrCallback = Arc<dyn Fn(i64, ElementArg, CallbackType) -> String + Send + Sync>;
pub type StrToIntCallback = Arc<dyn Fn(&str, ElementArg) -> i64 + Send + Sync>;

/// Implements the methods of [`Def`] and [`NamedDef`] that every formater
/// class implements in the same way. The class has the fields `self_ref`,
/// `def` and `nd` and a `clone_from` constructor.
macro_rules! formater_plumbing {
    ($class_name:literal) => {
        fn def_base(&self) -> &DefBase {
            &self.def
        }

        fn as_dyn_def(&self) -> &dyn Def {
            self
        }

        fn def_ref(&self) -> DefRef {
            self.self_ref
                .upgrade()
                .expect("a definition is alive while it is used")
        }

        fn duplicate(&self) -> DefRef {
            Self::clone_from(self)
        }

        fn as_named_def(&self) -> Option<&dyn NamedDef> {
            Some(self)
        }

        fn into_integer_def_formater(self: Arc<Self>) -> Option<Arc<dyn IntegerDefFormater>> {
            Some(self)
        }

        /// Upstream `ClassName`.
        fn get_def_type_name(&self) -> &'static str {
            $class_name
        }
    };
}

/// Implements `NamedDef` and `DefKind` for a formater class.
macro_rules! formater_impls {
    ($class:ty) => {
        impl NamedDef for $class {
            fn named_def_base(&self) -> &NamedDefBase {
                &self.nd
            }
        }

        impl DefKind for $class {
            fn duplicate_same(&self) -> Arc<Self> {
                Self::clone_from(self)
            }
        }
    };
}

/// `GetIsEditable` of the formaters that are always editable.
fn editable_unless_internal_only(def: &DefBase) -> bool {
    !(def.def_internal_edit_only() && !is_internal_edit())
}

/// Upstream `TwbIntegerDefFormaterUnion`: picks one of its members per element.
pub struct IntegerDefFormaterUnion {
    self_ref: Weak<IntegerDefFormaterUnion>,
    def: DefBase,
    nd: NamedDefBase,
    idfu_decider: IntegerDefFormaterUnionDecider,
    idfu_members: Vec<Arc<dyn IntegerDefFormater>>,
}

impl IntegerDefFormaterUnion {
    pub fn create(decider: IntegerDefFormaterUnionDecider, members: &[Arc<dyn IntegerDefFormater>]) -> Arc<Self> {
        let (def, nd) = integer_def_formater_create("TwbIntegerDefFormaterUnion");
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| {
            let parent: Weak<dyn Def> = self_ref.clone();
            Self {
                self_ref: self_ref.clone(),
                def,
                nd,
                idfu_decider: decider,
                idfu_members: members
                    .iter()
                    .map(|member| set_parent(member.clone(), &parent, false))
                    .collect(),
            }
        });
        DefBase::after_construction(&*this);
        this
    }

    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(source.idfu_decider.clone(), &source.idfu_members);
        NamedDefBase::after_clone(&*this, source);
        this
    }

    pub fn get_member(&self, index: i32) -> Option<Arc<dyn IntegerDefFormater>> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.idfu_members.get(index))
            .cloned()
    }

    pub fn get_member_count(&self) -> i32 {
        self.idfu_members.len() as i32
    }

    fn decide_member(&self, element: ElementArg) -> Option<&Arc<dyn IntegerDefFormater>> {
        usize::try_from((self.idfu_decider)(element))
            .ok()
            .and_then(|index| self.idfu_members.get(index))
    }
}

impl Def for IntegerDefFormaterUnion {
    formater_plumbing!("TwbIntegerDefFormaterUnion");

    fn get_def_type(&self) -> DefType {
        DefType::dtIntegerFormaterUnion
    }

    fn get_no_reach(&self) -> bool {
        self.idfu_members.iter().any(|member| member.get_no_reach())
    }

    fn init_from_parent_do_children(&self) {
        for member in &self.idfu_members {
            member.init_from_parent();
        }
    }
}

formater_impls!(IntegerDefFormaterUnion);

impl IntegerDefFormater for IntegerDefFormaterUnion {
    fn to_string(&self, int: i64, element: ElementArg, for_summary: bool) -> String {
        match self.decide_member(element) {
            Some(member) => member.to_string(int, element, for_summary),
            None => String::new(),
        }
    }

    fn to_sort_key(&self, int: i64, element: ElementArg) -> String {
        match self.decide_member(element) {
            Some(member) => member.to_sort_key(int, element),
            None => String::new(),
        }
    }

    fn check(&self, int: i64, element: ElementArg) -> String {
        // Upstream does not test the decision here and fails on an invalid one.
        match self.decide_member(element) {
            Some(member) => member.check(int, element),
            None => String::new(),
        }
    }

    fn build_ref(&self, int: i64, element: ElementArg) {
        if self.def.def_flags.contains(DefFlag::dfExcludeFromBuildRef) {
            return;
        }
        if let Some(member) = self.decide_member(element) {
            member.build_ref(int, element);
        }
    }

    fn get_edit_type(&self, element: ElementArg) -> EditType {
        match self.decide_member(element) {
            Some(member) => member.get_edit_type(element),
            None => EditType::etDefault,
        }
    }

    fn get_edit_info(&self, element: ElementArg) -> Vec<String> {
        match self.decide_member(element) {
            Some(member) => member.get_edit_info(element),
            None => Vec::new(),
        }
    }

    fn to_edit_value(&self, int: i64, element: ElementArg) -> String {
        match self.decide_member(element) {
            Some(member) => member.to_edit_value(int, element),
            None => String::new(),
        }
    }

    fn get_is_editable(&self, int: i64, element: ElementArg) -> bool {
        let result = match self.decide_member(element) {
            Some(member) => member.get_is_editable(int, element),
            None => false,
        };
        result && editable_unless_internal_only(&self.def)
    }

    fn get_links_to(&self, int: i64, element: ElementArg) -> Option<ElementRef> {
        self.decide_member(element)?.get_links_to(int, element)
    }

    fn is_union(&self) -> bool {
        true
    }

    fn decide(&self, element: ElementArg) -> Option<Option<Arc<dyn IntegerDefFormater>>> {
        Some(self.decide_member(element).cloned())
    }
}

/// Declares a formater class without fields of its own.
macro_rules! plain_formater {
    ($(#[$meta:meta])* $class:ident, $class_name:literal) => {
        $(#[$meta])*
        pub struct $class {
            self_ref: Weak<$class>,
            def: DefBase,
            nd: NamedDefBase,
        }

        impl $class {
            pub fn create() -> Arc<Self> {
                let (def, nd) = integer_def_formater_create($class_name);
                let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
                    self_ref: self_ref.clone(),
                    def,
                    nd,
                });
                DefBase::after_construction(&*this);
                this
            }

            pub fn clone_from(source: &Self) -> Arc<Self> {
                let this = Self::create();
                NamedDefBase::after_clone(&*this, source);
                this
            }
        }

        impl Def for $class {
            formater_plumbing!($class_name);

            fn get_def_type(&self) -> DefType {
                DefType::dtIntegerFormater
            }
        }

        formater_impls!($class);
    };
}

plain_formater!(
    /// Upstream `TwbDumpIntegerDefFormater`: shows a counter with its raw parts.
    DumpIntegerDefFormater,
    "TwbDumpIntegerDefFormater"
);

impl IntegerDefFormater for DumpIntegerDefFormater {
    fn to_string(&self, int: i64, _element: ElementArg, _for_summary: bool) -> String {
        // Pascal `shr` on an Int64 is a logical shift.
        format!(
            "{int} [{}] [{}:{}]",
            int_to_hex64(int, 8),
            int & 0x03,
            ((int as u64) >> 2) as i64
        )
    }

    fn to_sort_key(&self, int: i64, _element: ElementArg) -> String {
        int_to_hex64(int, 8)
    }
}

/// Port of `wbStr4ToString`: the four characters of the integer, last byte first.
pub fn str4_to_string(int: i64) -> String {
    if int == 0 {
        return "    ".to_owned();
    }
    let text = Signature::from_int(int as u32).to_string();
    if text.chars().count() == 4 {
        text.chars().rev().collect()
    } else {
        "    ".to_owned()
    }
}

plain_formater!(
    /// Upstream `TwbStr4`: a four-character string stored as an integer.
    Str4,
    "TwbStr4"
);

impl IntegerDefFormater for Str4 {
    fn to_string(&self, int: i64, element: ElementArg, _for_summary: bool) -> String {
        let result = str4_to_string(int);
        self.used(element, &result);
        result
    }

    fn to_sort_key(&self, int: i64, _element: ElementArg) -> String {
        str4_to_string(int)
    }

    fn to_edit_value(&self, int: i64, _element: ElementArg) -> String {
        str4_to_string(int)
    }

    fn get_is_editable(&self, _int: i64, _element: ElementArg) -> bool {
        editable_unless_internal_only(&self.def)
    }
}

/// The sort key of `TwbDivDef` and `TwbDivFDef`: the sign of the stored value
/// and the absolute value that the edit value converts back to.
fn div_sort_key(int: i64, element: ElementArg, divisor: f64) -> String {
    // Upstream fails without an element and on an edit value that is not a number.
    let edit_value = element.map(|element| element.get_edit_value()).unwrap_or_default();
    let from_edit_value = str_to_float(&edit_value).map_or(0, |value| round(value * divisor));
    format!(
        "{}{}",
        if int < 0 { '-' } else { '+' },
        int_to_hex64(from_edit_value.wrapping_abs(), 16)
    )
}

/// Declares `TwbDivDef` or `TwbDivFDef`: shows the integer divided by a constant.
macro_rules! div_formater {
    ($(#[$meta:meta])* $class:ident, $class_name:literal, $value:ty) => {
        $(#[$meta])*
        pub struct $class {
            self_ref: Weak<$class>,
            def: DefBase,
            nd: NamedDefBase,
            dd_value: $value,
            dd_precision: i32,
        }

        impl $class {
            pub fn create(value: $value, precision: i32) -> Arc<Self> {
                let (def, nd) = integer_def_formater_create($class_name);
                let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
                    self_ref: self_ref.clone(),
                    def,
                    nd,
                    dd_value: value,
                    dd_precision: precision,
                });
                DefBase::after_construction(&*this);
                this
            }

            pub fn clone_from(source: &Self) -> Arc<Self> {
                let this = Self::create(source.dd_value, source.dd_precision);
                NamedDefBase::after_clone(&*this, source);
                this
            }

            fn divisor(&self) -> f64 {
                #[allow(clippy::unnecessary_cast)]
                {
                    self.dd_value as f64
                }
            }

            fn display(&self, int: i64) -> String {
                float_to_str_f_fixed(int as f64 / self.divisor(), self.dd_precision.max(0) as usize)
            }
        }

        impl Def for $class {
            formater_plumbing!($class_name);

            fn get_def_type(&self) -> DefType {
                DefType::dtIntegerFormater
            }
        }

        formater_impls!($class);

        impl IntegerDefFormater for $class {
            fn to_string(&self, int: i64, element: ElementArg, _for_summary: bool) -> String {
                let result = self.display(int);
                self.used(element, &result);
                result
            }

            fn to_sort_key(&self, int: i64, element: ElementArg) -> String {
                div_sort_key(int, element, self.divisor())
            }

            fn to_edit_value(&self, int: i64, _element: ElementArg) -> String {
                self.display(int)
            }

            fn get_is_editable(&self, _int: i64, _element: ElementArg) -> bool {
                editable_unless_internal_only(&self.def)
            }
        }
    };
}

div_formater!(
    /// Upstream `TwbDivDef`.
    DivDef,
    "TwbDivDef",
    i32
);
div_formater!(
    /// Upstream `TwbDivFDef`.
    DivFDef,
    "TwbDivFDef",
    f64
);

/// Upstream `TwbMulDef`: shows the integer multiplied by a constant.
pub struct MulDef {
    self_ref: Weak<MulDef>,
    def: DefBase,
    nd: NamedDefBase,
    md_value: i32,
}

impl MulDef {
    pub fn create(value: i32) -> Arc<Self> {
        let (def, nd) = integer_def_formater_create("TwbMulDef");
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            md_value: value,
        });
        DefBase::after_construction(&*this);
        this
    }

    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(source.md_value);
        NamedDefBase::after_clone(&*this, source);
        this
    }

    fn display(&self, int: i64) -> String {
        int.wrapping_mul(i64::from(self.md_value)).to_string()
    }
}

impl Def for MulDef {
    formater_plumbing!("TwbMulDef");

    fn get_def_type(&self) -> DefType {
        DefType::dtIntegerFormater
    }
}

formater_impls!(MulDef);

impl IntegerDefFormater for MulDef {
    fn to_string(&self, int: i64, element: ElementArg, _for_summary: bool) -> String {
        let result = self.display(int);
        self.used(element, &result);
        result
    }

    /// Empty: the integer definition makes the key.
    fn to_sort_key(&self, _int: i64, _element: ElementArg) -> String {
        String::new()
    }

    fn to_edit_value(&self, int: i64, _element: ElementArg) -> String {
        self.display(int)
    }

    fn get_is_editable(&self, _int: i64, _element: ElementArg) -> bool {
        editable_unless_internal_only(&self.def)
    }
}

/// Upstream `TwbCallbackDef`: a callback makes every text form of the integer.
pub struct CallbackDef {
    self_ref: Weak<CallbackDef>,
    def: DefBase,
    nd: NamedDefBase,
    cd_to_str: IntToStrCallback,
    cd_to_int: Option<StrToIntCallback>,
}

impl CallbackDef {
    pub fn create(to_str: IntToStrCallback, to_int: Option<StrToIntCallback>) -> Arc<Self> {
        let (def, nd) = integer_def_formater_create("TwbCallbackDef");
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            cd_to_str: to_str,
            cd_to_int: to_int,
        });
        DefBase::after_construction(&*this);
        this
    }

    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(source.cd_to_str.clone(), source.cd_to_int.clone());
        NamedDefBase::after_clone(&*this, source);
        this
    }

    pub fn get_callback(&self) -> &IntToStrCallback {
        &self.cd_to_str
    }

    pub fn get_to_int_callback(&self) -> Option<&StrToIntCallback> {
        self.cd_to_int.as_ref()
    }
}

impl Def for CallbackDef {
    formater_plumbing!("TwbCallbackDef");

    fn get_def_type(&self) -> DefType {
        DefType::dtIntegerFormater
    }
}

formater_impls!(CallbackDef);

impl IntegerDefFormater for CallbackDef {
    fn to_string(&self, int: i64, element: ElementArg, for_summary: bool) -> String {
        let callback_type = if for_summary {
            CallbackType::ctToSummary
        } else {
            CallbackType::ctToStr
        };
        let result = (self.cd_to_str)(int, element, callback_type);
        self.used(element, &result);
        result
    }

    fn to_sort_key(&self, int: i64, element: ElementArg) -> String {
        (self.cd_to_str)(int, element, CallbackType::ctToSortKey)
    }

    fn check(&self, int: i64, element: ElementArg) -> String {
        (self.cd_to_str)(int, element, CallbackType::ctCheck)
    }

    fn get_edit_type(&self, element: ElementArg) -> EditType {
        let text = (self.cd_to_str)(0, element, CallbackType::ctEditType);
        if text.eq_ignore_ascii_case("ComboBox") {
            EditType::etComboBox
        } else if text.eq_ignore_ascii_case("CheckComboBox") {
            EditType::etCheckComboBox
        } else {
            debug_assert!(
                text.is_empty(),
                "Invalid result from ToStr ctEditType callback for {}",
                self.get_full_path()
            );
            EditType::etDefault
        }
    }

    fn get_edit_info(&self, element: ElementArg) -> Vec<String> {
        comma_text((self.cd_to_str)(0, element, CallbackType::ctEditInfo).as_str())
    }

    fn to_edit_value(&self, int: i64, element: ElementArg) -> String {
        let result = (self.cd_to_str)(int, element, CallbackType::ctToEditValue);
        if result.is_empty() { int.to_string() } else { result }
    }

    fn get_is_editable(&self, _int: i64, _element: ElementArg) -> bool {
        editable_unless_internal_only(&self.def)
    }
}

/// Port of assigning to `TStrings.CommaText` and reading the items back.
///
/// Items are separated by commas or blanks. An item in double quotes keeps
/// its separators, and a doubled quote inside it is one quote.
pub fn comma_text(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut items = Vec::new();
    let mut position = 0;
    let is_blank = |c: char| c <= ' ';
    while position < chars.len() && is_blank(chars[position]) {
        position += 1;
    }
    while position < chars.len() {
        let mut item = String::new();
        if chars[position] == '"' {
            position += 1;
            while position < chars.len() {
                if chars[position] == '"' {
                    position += 1;
                    if position < chars.len() && chars[position] == '"' {
                        item.push('"');
                        position += 1;
                    } else {
                        break;
                    }
                } else {
                    item.push(chars[position]);
                    position += 1;
                }
            }
        } else {
            while position < chars.len() && !is_blank(chars[position]) && chars[position] != ',' {
                item.push(chars[position]);
                position += 1;
            }
        }
        items.push(item);
        while position < chars.len() && is_blank(chars[position]) {
            position += 1;
        }
        if position < chars.len() && chars[position] == ',' {
            position += 1;
            while position < chars.len() && is_blank(chars[position]) {
                position += 1;
            }
            // A trailing comma adds an empty item.
            if position >= chars.len() {
                items.push(String::new());
            }
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::super::globals::test_lock;
    use super::*;

    #[test]
    fn union_delegates_to_the_decided_member() {
        let _guard = test_lock();
        let decider: IntegerDefFormaterUnionDecider = Arc::new(|element| if element.is_some() { 5 } else { 1 });
        let dump: Arc<dyn IntegerDefFormater> = DumpIntegerDefFormater::create();
        let mul: Arc<dyn IntegerDefFormater> = MulDef::create(3);
        let union = IntegerDefFormaterUnion::create(decider, &[dump, mul.clone()]);
        assert_eq!(union.to_string(7, None, false), "21");
        assert_eq!(union.to_edit_value(7, None), "21");
        assert_eq!(union.to_sort_key(7, None), "");
        assert!(union.is_union());
        assert!(Arc::ptr_eq(&union.decide(None).unwrap().unwrap(), &mul));
        assert_eq!(union.get_member_count(), 2);
        assert!(union.get_member(2).is_none());
        assert_eq!(mul.get_path(), "TwbIntegerDefFormaterUnion \\ TwbMulDef");
        assert_eq!(union.get_def_type(), DefType::dtIntegerFormaterUnion);

        let copy = IntegerDefFormaterUnion::clone_from(&union);
        assert_eq!(copy.to_string(7, None, false), "21");
        assert!(!Arc::ptr_eq(&copy.get_member(1).unwrap(), &mul));
    }

    #[test]
    fn dump_formater() {
        let formater = DumpIntegerDefFormater::create();
        assert_eq!(formater.to_string(0x16, None, false), "22 [00000016] [2:5]");
        assert_eq!(formater.to_sort_key(0x16, None), "00000016");
        assert_eq!(formater.get_name(), "TwbDumpIntegerDefFormater");
    }

    #[test]
    fn str4() {
        assert_eq!(str4_to_string(0), "    ");
        assert_eq!(str4_to_string(i64::from(u32::from_le_bytes(*b"DCBA"))), "ABCD");
        // A zero byte ends the string, which then does not have four characters.
        assert_eq!(str4_to_string(i64::from(u32::from_le_bytes(*b"AB\0D"))), "    ");
        assert_eq!(
            Str4::create().to_string(i64::from(u32::from_le_bytes(*b"TXET")), None, false),
            "TEXT"
        );
    }

    #[test]
    fn div_and_mul() {
        let _guard = test_lock();
        let div = DivDef::create(100, 2);
        assert_eq!(div.to_string(250, None, false), "2.50");
        assert_eq!(div.to_string(-1, None, false), "-0.01");
        assert_eq!(div.to_edit_value(1, None), "0.01");
        assert_eq!(div.to_sort_key(-5, None), "-0000000000000000");
        assert!(div.get_is_editable(0, None));
        let div_f = DivFDef::create(2.5, 1);
        assert_eq!(div_f.to_string(5, None, false), "2.0");
        assert_eq!(DivFDef::clone_from(&div_f).to_string(10, None, false), "4.0");
        let mul = MulDef::create(4);
        assert_eq!(mul.to_string(-3, None, false), "-12");
    }

    #[test]
    fn callback() {
        let to_str: IntToStrCallback = Arc::new(|int, _, callback_type| match callback_type {
            CallbackType::ctToStr => format!("value {int}"),
            CallbackType::ctToSummary => format!("v{int}"),
            CallbackType::ctToSortKey => format!("{int:04}"),
            CallbackType::ctCheck => "wrong".to_owned(),
            CallbackType::ctEditType => "ComboBox".to_owned(),
            CallbackType::ctEditInfo => "One,\"Two, three\",Four".to_owned(),
            _ => String::new(),
        });
        let def = CallbackDef::create(to_str, None);
        assert_eq!(def.to_string(3, None, false), "value 3");
        assert_eq!(def.to_string(3, None, true), "v3");
        assert_eq!(def.to_sort_key(3, None), "0003");
        assert_eq!(def.check(3, None), "wrong");
        assert_eq!(def.to_edit_value(3, None), "3");
        assert_eq!(def.get_edit_type(None), EditType::etComboBox);
        assert_eq!(def.get_edit_info(None), ["One", "Two, three", "Four"]);
    }

    #[test]
    fn comma_text_rules() {
        assert_eq!(comma_text(""), Vec::<String>::new());
        assert_eq!(comma_text("a,b c"), ["a", "b", "c"]);
        assert_eq!(comma_text("\"a \"\"b\"\"\",c"), ["a \"b\"", "c"]);
        assert_eq!(comma_text("a,"), ["a", ""]);
        assert_eq!(comma_text("a,,b"), ["a", "", "b"]);
    }
}
