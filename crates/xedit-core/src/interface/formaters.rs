// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! The small integer formaters: `TwbIntegerDefFormaterUnion`,
//! `TwbDumpIntegerDefFormater`, `TwbStr4`, `TwbChar4`, `TwbDivDef`,
//! `TwbDivFDef`, `TwbMulDef` and `TwbCallbackDef`.

use std::sync::{Arc, Weak};

use super::def::{Def, DefBase, DefKind, DefRef, NamedDef, NamedDefBase, set_parent};
use super::element::{ElementArg, ElementRef};
use super::form_id::{MastersUpdate, UsedMasters};
use super::globals::is_internal_edit;
use super::integer::{IntegerDefFormater, integer_def_formater_create};
use super::misc::{EditError, int_to_hex64, str_to_int64};
use super::types::{CallbackType, DefFlag, DefType, EditType, Signature};
use crate::delphi::{float_to_str_f_fixed, round, str_to_float};

pub type IntegerDefFormaterUnionDecider = Arc<dyn Fn(ElementArg) -> i32 + Send + Sync>;
pub type IntToStrCallback = Arc<dyn Fn(i64, ElementArg, CallbackType) -> String + Send + Sync>;
pub type StrToIntCallback = Arc<dyn Fn(&str, ElementArg) -> i64 + Send + Sync>;

/// Implements the methods of [`Def`] and [`NamedDef`] that every formater
/// class implements in the same way. The class has the fields `self_ref`,
/// `def` and `nd` and a `clone_from` constructor.
macro_rules! formater_plumbing {
    () => {
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
    };
}
pub(crate) use formater_plumbing;

/// `GetDefTypeName` of a formater: upstream `ClassName`.
macro_rules! formater_type_name {
    ($class_name:literal) => {
        fn get_def_type_name(&self) -> String {
            $class_name.to_owned()
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
pub(crate) use formater_impls;

/// `GetIsEditable` of the formaters that are always editable.
pub(crate) fn editable_unless_internal_only(def: &DefBase) -> bool {
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
    formater_plumbing!();
    formater_type_name!("TwbIntegerDefFormaterUnion");

    /// Port of `TwbIntegerDefFormaterUnion.CanAssign`: the member the
    /// decider picks decides.
    fn can_assign(&self, element: ElementArg, index: i32, def: Option<&dyn Def>) -> bool {
        if super::def::def_dont_assign(self) {
            return false;
        }
        self.decide_member(element)
            .is_some_and(|member| member.can_assign(element, index, def))
    }

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
    /// Port of `TwbIntegerDefFormaterUnion.Assign`.
    fn assign(
        &self,
        target: &ElementRef,
        index: i32,
        source: Option<&ElementRef>,
        only_sk: bool,
    ) -> Result<Option<ElementRef>, EditError> {
        match self.decide_member(Some(target)) {
            Some(member) => member.assign(target, index, source, only_sk),
            None => Ok(None),
        }
    }

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

    /// Port of `TwbIntegerDefFormaterUnion.CompareExchangeFormID`.
    fn compare_exchange_form_id(
        &self,
        int: &mut i64,
        old: crate::interface::form_id::FormID,
        new: crate::interface::form_id::FormID,
        element: ElementArg,
    ) -> Result<bool, EditError> {
        match self.decide_member(element) {
            Some(member) => member.compare_exchange_form_id(int, old, new, element),
            None => Ok(false),
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

    fn from_edit_value(&self, value: &str, element: ElementArg) -> Result<i64, EditError> {
        match self.decide_member(element) {
            Some(member) => member.from_edit_value(value, element),
            None => Ok(0),
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

    /// Port of `TwbIntegerDefFormaterUnion.MastersUpdated`.
    // UPSTREAM-QUIRK: without a member for the element the result is 0, so
    // the integer is set to 0.
    fn masters_updated(&self, int: i64, element: ElementArg, update: &MastersUpdate) -> i64 {
        match self.decide_member(element) {
            Some(member) => member.masters_updated(int, element, update),
            None => 0,
        }
    }

    /// Port of `TwbIntegerDefFormaterUnion.FindUsedMasters`.
    fn find_used_masters(&self, int: i64, element: ElementArg, masters: &mut UsedMasters) {
        if let Some(member) = self.decide_member(element) {
            member.find_used_masters(int, element, masters);
        }
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
            formater_plumbing!();
            formater_type_name!($class_name);

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

    /// Port of `TwbStr4.FromEditValue`: the four characters reversed.
    fn from_edit_value(&self, value: &str, _element: ElementArg) -> Result<i64, EditError> {
        let mut bytes = four_characters(value)?;
        bytes.reverse();
        Ok(i64::from(u32::from_le_bytes(bytes)))
    }

    fn get_is_editable(&self, _int: i64, _element: ElementArg) -> bool {
        editable_unless_internal_only(&self.def)
    }
}

plain_formater!(
    /// Upstream `TwbChar4`: the four characters of an Oblivion editor ID
    /// stored as an integer, such as the code of a magic effect, which name
    /// the record with that editor ID.
    Char4,
    "TwbChar4"
);

impl Char4 {
    /// Port of `TwbChar4.Create` with `AfterConstruction`.
    pub fn create_char4() -> Arc<Self> {
        let this = Self::create();
        this.def.def_flags.include(DefFlag::dfCanContainFormID);
        this
    }

    /// The characters in the order of the bytes (`PwbSignature(@U32)^`).
    fn characters(int: i64) -> String {
        Signature::from_int(int as u32).to_string()
    }
}

impl IntegerDefFormater for Char4 {
    /// Port of `TwbChar4.ToString`: the name of the record with the editor
    /// ID, or the characters with a warning.
    fn to_string(&self, int: i64, element: ElementArg, _for_summary: bool) -> String {
        let mut result = Self::characters(int);
        if let Some(file) = element.and_then(|element| element.get_file())
            && let Some(record) = file.get_record_by_editor_id(&result)
        {
            let result = record.get_name();
            self.used(element, &result);
            return result;
        }
        if int as u32 != 0 {
            result.push_str(" <Warning: could not be resolved>");
        }
        self.used(element, &result);
        result
    }

    fn to_sort_key(&self, int: i64, _element: ElementArg) -> String {
        Self::characters(int)
    }

    fn to_edit_value(&self, int: i64, _element: ElementArg) -> String {
        Self::characters(int)
    }

    /// Port of `TwbChar4.FromEditValue`.
    fn from_edit_value(&self, value: &str, _element: ElementArg) -> Result<i64, EditError> {
        Ok(i64::from(u32::from_le_bytes(four_characters(value)?)))
    }

    fn get_is_editable(&self, _int: i64, _element: ElementArg) -> bool {
        editable_unless_internal_only(&self.def)
    }

    fn get_links_to(&self, int: i64, element: ElementArg) -> Option<ElementRef> {
        let file = element?.get_file()?;
        file.get_record_by_editor_id(&Self::characters(int))
            .map(|record| record as ElementRef)
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
            formater_plumbing!();
            formater_type_name!($class_name);

            /// Port of `TwbDivDef.CanAssign` and `TwbDivFDef.CanAssign`.
            fn can_assign(&self, _element: ElementArg, _index: i32, _def: Option<&dyn Def>) -> bool {
                !super::def::def_dont_assign(self)
            }

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

            /// Port of `FromEditValue`: the number multiplied by the divisor,
            /// rounded.
            fn from_edit_value(&self, value: &str, _element: ElementArg) -> Result<i64, EditError> {
                let number =
                    str_to_float(value).ok_or_else(|| format!("'{value}' is not a valid floating point value"))?;
                Ok(round(number * self.divisor()))
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
    formater_plumbing!();
    formater_type_name!("TwbMulDef");

    /// Port of `TwbMulDef.CanAssign`.
    fn can_assign(&self, _element: ElementArg, _index: i32, _def: Option<&dyn Def>) -> bool {
        !super::def::def_dont_assign(self)
    }

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

    /// Port of `TwbMulDef.FromEditValue`: the integer divided by the factor.
    fn from_edit_value(&self, value: &str, _element: ElementArg) -> Result<i64, EditError> {
        let factor = i64::from(self.md_value);
        if factor == 0 {
            return Err("Division by zero".to_owned());
        }
        Ok(str_to_int64(value)? / factor)
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
    formater_plumbing!();
    formater_type_name!("TwbCallbackDef");

    fn as_callback_def(&self) -> Option<&CallbackDef> {
        Some(self)
    }

    /// Port of `TwbCallbackDef.CanAssign`: a callback definition with the
    /// same callback.
    fn can_assign(&self, _element: ElementArg, _index: i32, def: Option<&dyn Def>) -> bool {
        if super::def::def_dont_assign(self) {
            return false;
        }
        def.and_then(|def| def.as_callback_def())
            .is_some_and(|other| Arc::ptr_eq(other.get_callback(), &self.cd_to_str))
    }

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

    fn from_edit_value(&self, value: &str, element: ElementArg) -> Result<i64, EditError> {
        match &self.cd_to_int {
            Some(to_int) => Ok(to_int(value, element)),
            None => str_to_int64(value),
        }
    }

    fn get_is_editable(&self, _int: i64, _element: ElementArg) -> bool {
        editable_unless_internal_only(&self.def)
    }
}

/// The bytes of a four character edit value, or the blank signature for an
/// empty one.
fn four_characters(value: &str) -> Result<[u8; 4], EditError> {
    if value.is_empty() {
        return Ok(*b"    ");
    }
    let bytes = value.as_bytes();
    if bytes.len() != 4 {
        return Err("The value must be exactly 4 characters".to_owned());
    }
    Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
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
