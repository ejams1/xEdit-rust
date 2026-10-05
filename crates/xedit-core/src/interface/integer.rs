// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbIntegerDef` and the base of its formaters, `TwbIntegerDefFormater`.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Weak};

use super::def::{
    Def, DefBase, DefCell, DefKind, DefRef, NamedDef, NamedDefArgs, NamedDefBase, ValueDef, ValueDefBase, set_parent,
    value_def_plumbing,
};
use super::element::{DataPtr, ElementArg, ElementRef};
use super::globals::{check_expected_bytes, is_internal_edit};
use super::misc::{Variant, int_to_hex64, read_integer_counter, read_integer_counter_size, read_integer24};
use super::types::{CallbackType, ConflictPriority, DefType, EditType, IntType};

pub type IntOverlayCallback = Arc<dyn Fn(i64, ElementArg, CallbackType) -> i64 + Send + Sync>;

/// Upstream `IwbIntegerDefFormater`, implemented by `TwbIntegerDefFormater`.
///
/// The methods that change data (`FromEditValue`, `FromLinksTo`,
/// `MastersUpdated`, `FindUsedMasters`, `CompareExchangeFormID`) come with the
/// write path.
pub trait IntegerDefFormater: NamedDef {
    /// Port of `ToString`: the display value of `int`.
    fn to_string(&self, int: i64, element: ElementArg, for_summary: bool) -> String;

    fn to_sort_key(&self, int: i64, element: ElementArg) -> String;

    fn check(&self, _int: i64, _element: ElementArg) -> String {
        String::new()
    }

    fn build_ref(&self, _int: i64, _element: ElementArg) {}

    fn get_edit_type(&self, _element: ElementArg) -> EditType {
        EditType::etDefault
    }

    fn get_edit_info(&self, _element: ElementArg) -> Vec<String> {
        Vec::new()
    }

    fn to_edit_value(&self, _int: i64, _element: ElementArg) -> String {
        String::new()
    }

    fn get_is_editable(&self, _int: i64, _element: ElementArg) -> bool {
        // The test of defInternalEditOnly upstream cannot change this result.
        is_internal_edit()
    }

    fn get_links_to(&self, _int: i64, _element: ElementArg) -> Option<ElementRef> {
        None
    }

    fn get_requires_key(&self) -> bool {
        false
    }

    /// `Supports(formater, IwbIntegerDefFormaterUnion)`.
    fn is_union(&self) -> bool {
        false
    }

    /// Port of `IwbIntegerDefFormaterUnion.Decide` for a formater that is a union.
    /// `None` when the formater is not a union.
    fn decide(&self, _element: ElementArg) -> Option<Option<Arc<dyn IntegerDefFormater>>> {
        None
    }
}

impl DefKind for dyn IntegerDefInterface {
    fn duplicate_same(&self) -> Arc<Self> {
        self.duplicate()
            .into_integer_def()
            .expect("the duplicate of an integer definition is an integer definition")
    }
}

impl DefKind for dyn IntegerDefFormater {
    fn duplicate_same(&self) -> Arc<Self> {
        self.duplicate()
            .into_integer_def_formater()
            .expect("the duplicate of a formater is a formater")
    }
}

/// Port of `TwbIntegerDefFormater.Create`: a formater is named after its class.
pub fn integer_def_formater_create(class_name: &str) -> (DefBase, NamedDefBase) {
    NamedDefBase::create(NamedDefArgs {
        priority: ConflictPriority::cpNormal,
        required: false,
        name: class_name.to_owned(),
        after_load: None,
        after_set: None,
        dont_show: None,
        get_cp: None,
        terminator: false,
    })
}

/// Upstream `IwbIntegerDef`.
pub trait IntegerDefInterface: ValueDef {
    fn to_int(&self, data: DataPtr, element: ElementArg) -> i64;

    /// The formater for `element`, with formater unions resolved.
    fn get_formater(&self, element: ElementArg) -> Option<Arc<dyn IntegerDefFormater>>;

    fn get_formater_can_change(&self) -> bool;

    fn get_int_type(&self) -> IntType;

    fn get_expected_length(&self, value: i64) -> i32;

    /// Upstream `IwbIntegerDefInternal.ReplaceFormater`.
    fn replace_formater(&self, formater: Option<Arc<dyn IntegerDefFormater>>);
}

/// Upstream `TwbIntegerDef`.
pub struct IntegerDef {
    self_ref: Weak<IntegerDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
    in_type: IntType,
    in_formater: DefCell<Arc<dyn IntegerDefFormater>>,
    in_default: AtomicI64,
    in_overlay_callback: DefCell<IntOverlayCallback>,
}

impl IntegerDef {
    /// Port of `TwbIntegerDef.Create`. `args.after_load` is not used, as upstream.
    pub fn create(
        args: NamedDefArgs,
        int_type: IntType,
        formater: Option<Arc<dyn IntegerDefFormater>>,
        default: i64,
    ) -> Arc<Self> {
        let this = Arc::new_cyclic(|self_ref: &Weak<IntegerDef>| {
            let parent: Weak<dyn Def> = self_ref.clone();
            let formater = formater.map(|formater| set_parent(formater, &parent, false));
            let (def, nd) = NamedDefBase::create(NamedDefArgs {
                after_load: None,
                ..args
            });
            Self {
                self_ref: self_ref.clone(),
                def,
                nd,
                vd: ValueDefBase::default(),
                in_type: int_type,
                in_formater: DefCell::new(formater),
                in_default: AtomicI64::new(default),
                in_overlay_callback: DefCell::default(),
            }
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbIntegerDef.Clone`.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(
            NamedDefBase::clone_args(source),
            source.in_type,
            source.in_formater(),
            source.in_default.load(Ordering::Relaxed),
        );
        ValueDefBase::after_clone(&*this, source);
        this.in_overlay_callback.assign(&source.in_overlay_callback);
        this
    }

    /// The formater as given to the constructor, unions not resolved.
    pub fn in_formater(&self) -> Option<Arc<dyn IntegerDefFormater>> {
        self.in_formater.load().as_deref().cloned()
    }

    pub fn in_default(&self) -> i64 {
        self.in_default.load(Ordering::Relaxed)
    }

    /// Port of `AddOverlay`.
    pub fn add_overlay(self: Arc<Self>, callback: Option<IntOverlayCallback>) -> Arc<Self> {
        let this = if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        };
        this.in_overlay_callback.set(callback);
        this
    }

    /// Port of the override of `SetDefaultNativeValue`, which sets `inDefault`.
    pub fn set_default_int(self: Arc<Self>, value: i64) -> Arc<Self> {
        let this = if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        };
        this.in_default.store(value, Ordering::Relaxed);
        this
    }

    /// Port of `ReplaceFormater`.
    pub fn replace_formater(&self, formater: Option<Arc<dyn IntegerDefFormater>>) {
        self.def.clear_def_source();
        let parent: Weak<dyn Def> = self.self_ref.clone();
        self.in_formater
            .set(formater.map(|formater| set_parent(formater, &parent, true)));
    }

    /// Number of bytes between the base and the end pointer.
    fn len(data: DataPtr) -> i64 {
        data.map_or(0, |data| data.len() as i64)
    }

    /// Reads the value. `data` holds at least `get_expected_length(0)` bytes.
    fn read(&self, data: &[u8]) -> i64 {
        match self.in_type {
            IntType::itU8 => i64::from(data[0]),
            IntType::itS8 => i64::from(data[0] as i8),
            IntType::itU16 => i64::from(u16::from_le_bytes([data[0], data[1]])),
            IntType::itS16 => i64::from(i16::from_le_bytes([data[0], data[1]])),
            IntType::itU24 => read_integer24(data),
            IntType::itU32 => i64::from(u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
            IntType::itS32 => i64::from(i32::from_le_bytes([data[0], data[1], data[2], data[3]])),
            // Upstream reads an unsigned value into an Int64, which reinterprets the bits.
            IntType::itU64 | IntType::itS64 => {
                i64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]])
            }
            IntType::itU6to30 => read_integer_counter(Some(data)),
            IntType::it0 => 0,
        }
    }

    /// The value with the overlay applied, or `None` when the data is too short.
    fn value(&self, data: DataPtr, element: ElementArg, callback_type: CallbackType) -> Option<i64> {
        if Self::len(data) < i64::from(self.get_expected_length(0)) {
            return None;
        }
        let mut value = self.read(data.unwrap_or_default());
        if let Some(overlay) = self.in_overlay_callback.load().as_deref() {
            value = overlay(value, element, callback_type);
        }
        Some(value)
    }

    /// Shared body of `ToString` and `ToSummary`.
    fn display(&self, data: DataPtr, element: ElementArg, callback_type: CallbackType, for_summary: bool) -> String {
        let expected = self.get_expected_length(0);
        let len = Self::len(data);
        match self.value(data, element, callback_type) {
            None => {
                if check_expected_bytes() {
                    format!("<Error: Expected {expected} bytes of data, found {len}>")
                } else {
                    String::new()
                }
            }
            Some(value) => {
                let mut result = match self.in_formater.load().as_deref() {
                    Some(formater) => formater.to_string(value, element, for_summary),
                    None => value.to_string(),
                };
                if len > i64::from(expected) && self.in_type != IntType::itU6to30 && check_expected_bytes() {
                    result.push_str(&format!(" <Warning: Expected {expected} bytes of data, found {len}>"));
                }
                result
            }
        }
    }
}

impl Def for IntegerDef {
    value_def_plumbing!(Def);

    fn get_def_type(&self) -> DefType {
        DefType::dtInteger
    }

    fn get_def_type_name(&self) -> String {
        if let Some(formater) = self.in_formater.load().as_deref() {
            return formater.get_def_type_name();
        }
        let name = match self.in_type {
            IntType::itS8 => "Signed Byte",
            IntType::itU16 => "Unsigned Word",
            IntType::itS16 => "Signed Word",
            IntType::itU24 => "RefID",
            IntType::itU32 => "Unsigned DWord",
            IntType::itS32 => "Signed DWord",
            IntType::itU64 => "Unsigned QWord",
            IntType::itS64 => "Signed QWord",
            IntType::itU6to30 => "Counter",
            IntType::it0 | IntType::itU8 => "Unsigned Byte",
        };
        name.to_owned()
    }

    fn as_integer_def(&self) -> Option<&dyn IntegerDefInterface> {
        Some(self)
    }

    fn into_integer_def(self: Arc<Self>) -> Option<Arc<dyn IntegerDefInterface>> {
        Some(self)
    }

    fn get_no_reach(&self) -> bool {
        self.in_formater
            .load()
            .as_deref()
            .is_some_and(|formater| formater.get_no_reach())
    }

    fn get_conflict_priority(&self, element: ElementArg) -> ConflictPriority {
        if let Some(formater) = self.in_formater.load().as_deref()
            && formater.get_conflict_priority_can_change()
        {
            return formater.get_conflict_priority(element);
        }
        let mut result = self.def.def_priority;
        if let Some(get_cp) = &self.def.def_get_cp {
            get_cp(element, &mut result);
        }
        result
    }

    fn get_conflict_priority_can_change(&self) -> bool {
        self.def.def_get_cp.is_some()
            || self
                .in_formater
                .load()
                .as_deref()
                .is_some_and(|formater| formater.get_conflict_priority_can_change())
    }

    fn init_from_parent_do_children(&self) {
        if let Some(formater) = self.in_formater.load().as_deref() {
            formater.init_from_parent();
        }
    }
}

impl NamedDef for IntegerDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for IntegerDef {
    value_def_plumbing!(ValueDef);

    /// Port of the override of `SetDefaultNativeValue`, which sets `inDefault`.
    fn apply_default_native_value(&self, value: Variant) {
        // Upstream fails for a value that is not a number.
        let default = match &value {
            Variant::Float(float) => crate::delphi::round(*float),
            other => other.as_ordinal().unwrap_or(0),
        };
        self.in_default.store(default, Ordering::Relaxed);
    }

    fn to_string(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = String::new();
        if self.in_type == IntType::it0 {
            return result;
        }
        result = self.display(data, element, CallbackType::ctToStr, false);
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToStr);
        }
        self.used(element, &result);
        result
    }

    fn to_summary(&self, _depth: i32, data: DataPtr, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        let mut result = String::new();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToSummary);
        }
        if result.is_empty() {
            if self.in_type == IntType::it0 {
                return result;
            }
            result = self.display(data, element, CallbackType::ctToSummary, true);
        }
        if links_to.is_none()
            && !result.is_empty()
            && let Some(element) = element
        {
            *links_to = element.get_links_to();
        }
        self.used(element, &result);
        result
    }

    fn to_sort_key(&self, data: DataPtr, element: ElementArg, _extended: bool) -> String {
        let formater = self.in_formater.load();
        let formater = formater.as_deref();
        let mut result = match self.value(data, element, CallbackType::ctToSortKey) {
            None => match formater {
                Some(formater) if formater.get_requires_key() => formater.to_sort_key(0, element),
                _ => String::new(),
            },
            Some(mut value) => {
                let mut result = match formater {
                    Some(formater) => formater.to_sort_key(value, element),
                    None => String::new(),
                };
                if result.is_empty() {
                    // Shifts signed values so that the keys sort like the values.
                    value = match self.in_type {
                        IntType::itS8 => value + 0x80,
                        IntType::itS16 => value + 0x8000,
                        IntType::itS32 => value + 0x8000_0000,
                        // Abs(Low(Int64)) overflows to Low(Int64) upstream.
                        IntType::itS64 => value.wrapping_add(i64::MIN),
                        _ => value,
                    };
                    result = int_to_hex64(value, (self.get_expected_length(value) * 2 + 1) as usize);
                }
                result
            }
        };
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToSortKey);
        }
        result
    }

    fn check(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = match self.value(data, element, CallbackType::ctCheck) {
            None => {
                if check_expected_bytes() {
                    format!(
                        "Expected {} bytes of data, found {}",
                        self.get_expected_length(0),
                        Self::len(data)
                    )
                } else {
                    String::new()
                }
            }
            Some(value) => match self.in_formater.load().as_deref() {
                Some(formater) => formater.check(value, element),
                None => String::new(),
            },
        };
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctCheck);
        }
        result
    }

    fn get_size(&self, data: DataPtr, element: ElementArg) -> i32 {
        if self.in_type == IntType::it0 {
            return 0;
        }
        match data {
            Some(bytes) if !bytes.is_empty() => {
                if self.in_type == IntType::itU6to30 {
                    read_integer_counter_size(data) as i32 + i32::from(self.nd.nd_terminator)
                } else {
                    self.get_default_size(data, element)
                }
            }
            _ => self.get_default_size(data, element),
        }
    }

    fn get_default_size(&self, _data: DataPtr, _element: ElementArg) -> i32 {
        let size = match self.in_type {
            IntType::it0 => return 0,
            IntType::itU8 | IntType::itS8 | IntType::itU6to30 => 1,
            IntType::itU16 | IntType::itS16 => 2,
            IntType::itU24 => 3,
            IntType::itU32 | IntType::itS32 => 4,
            IntType::itU64 | IntType::itS64 => 8,
        };
        size + i32::from(self.nd.nd_terminator)
    }

    fn get_links_to(&self, data: DataPtr, element: ElementArg) -> Option<ElementRef> {
        if let Some(callback) = self.vd.vd_links_to_callback.load().as_deref() {
            return callback(element);
        }
        let formater = self.in_formater.load();
        let formater = formater.as_deref()?;
        let value = self.value(data, element, CallbackType::ctLinksTo)?;
        formater.get_links_to(value, element)
    }

    fn build_ref(&self, data: DataPtr, element: ElementArg) {
        if self
            .def
            .def_flags
            .contains(super::types::DefFlag::dfExcludeFromBuildRef)
        {
            return;
        }
        if let Some(formater) = self.in_formater.load().as_deref()
            && let Some(value) = self.value(data, element, CallbackType::ctBuildRef)
        {
            formater.build_ref(value, element);
        }
    }

    fn to_edit_value(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = String::new();
        if self.in_type != IntType::it0
            && let Some(value) = self.value(data, element, CallbackType::ctToEditValue)
        {
            if let Some(formater) = self.in_formater.load().as_deref() {
                result = formater.to_edit_value(value, element);
            }
            if result.is_empty() {
                result = value.to_string();
            }
        }
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToEditValue);
        }
        result
    }

    fn to_native_value(&self, data: DataPtr, element: ElementArg) -> Variant {
        let Some(value) = self.value(data, element, CallbackType::ctToNativeValue) else {
            return Variant::Empty;
        };
        match self.in_type {
            IntType::itU8 => Variant::UInt(u64::from(value as u8)),
            IntType::itS8 => Variant::Int(i64::from(value as i8)),
            IntType::itU16 => Variant::UInt(u64::from(value as u16)),
            IntType::itS16 => Variant::Int(i64::from(value as i16)),
            IntType::itU32 => Variant::UInt(u64::from(value as u32)),
            IntType::itS32 => Variant::Int(i64::from(value as i32)),
            IntType::itU64 => Variant::UInt(value as u64),
            IntType::itS64 | IntType::itU24 | IntType::itU6to30 => Variant::Int(value),
            IntType::it0 => Variant::Int(0),
        }
    }

    fn get_is_editable(&self, data: DataPtr, element: ElementArg) -> bool {
        let result = is_internal_edit()
            || match self.in_formater.load().as_deref() {
                Some(formater) => formater.get_is_editable(self.to_int(data, element), element),
                None => true,
            };
        if self.def.def_internal_edit_only() && !is_internal_edit() {
            return false;
        }
        result
    }

    fn get_edit_type(&self, _data: DataPtr, element: ElementArg) -> EditType {
        match self.in_formater.load().as_deref() {
            Some(formater) => formater.get_edit_type(element),
            None => EditType::etDefault,
        }
    }

    fn get_edit_info(&self, _data: DataPtr, element: ElementArg) -> Vec<String> {
        match self.in_formater.load().as_deref() {
            Some(formater) => formater.get_edit_info(element),
            None => self.vd.vd_edit_info.load().as_deref().cloned().unwrap_or_default(),
        }
    }
}

impl IntegerDefInterface for IntegerDef {
    fn replace_formater(&self, formater: Option<Arc<dyn IntegerDefFormater>>) {
        IntegerDef::replace_formater(self, formater);
    }

    fn to_int(&self, data: DataPtr, element: ElementArg) -> i64 {
        let mut result = if Self::len(data) < i64::from(self.get_expected_length(0)) {
            0
        } else {
            self.read(data.unwrap_or_default())
        };
        // The overlay also runs when the data is too short.
        if let Some(overlay) = self.in_overlay_callback.load().as_deref() {
            result = overlay(result, element, CallbackType::ctToInt);
        }
        result
    }

    fn get_formater(&self, element: ElementArg) -> Option<Arc<dyn IntegerDefFormater>> {
        let mut result = self.in_formater();
        while let Some(decided) = result.as_ref().and_then(|formater| formater.decide(element)) {
            result = decided;
        }
        result
    }

    fn get_formater_can_change(&self) -> bool {
        self.in_formater
            .load()
            .as_deref()
            .is_some_and(|formater| formater.is_union())
    }

    fn get_int_type(&self) -> IntType {
        self.in_type
    }

    fn get_expected_length(&self, value: i64) -> i32 {
        let terminator = i32::from(self.nd.nd_terminator);
        match self.in_type {
            IntType::it0 => 0,
            IntType::itU8 | IntType::itS8 => 1 + terminator,
            IntType::itU16 | IntType::itS16 => 2 + terminator,
            IntType::itU32 | IntType::itS32 => 4 + terminator,
            IntType::itU64 | IntType::itS64 => 8 + terminator,
            IntType::itU24 => 3 + terminator,
            IntType::itU6to30 => {
                if value == 0 {
                    1 + terminator
                } else {
                    match value & 3 {
                        0 => 1 + terminator,
                        1 => 2 + terminator,
                        2 => 4 + terminator,
                        _ => 1 + terminator,
                    }
                }
            }
        }
    }
}

impl DefKind for IntegerDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::def::{DefSetters, NamedDefSetters, ToStrCallback};
    use super::super::globals::{set_check_expected_bytes, test_lock};
    use super::super::types::DefFlag;
    use super::*;

    fn args(name: &str) -> NamedDefArgs {
        NamedDefArgs {
            priority: ConflictPriority::cpNormal,
            required: false,
            name: name.to_owned(),
            after_load: None,
            after_set: None,
            dont_show: None,
            get_cp: None,
            terminator: false,
        }
    }

    /// A formater that shows the value in brackets.
    struct Brackets {
        self_ref: Weak<Brackets>,
        def: DefBase,
        nd: NamedDefBase,
    }

    impl Brackets {
        fn create() -> Arc<Self> {
            let (def, nd) = integer_def_formater_create("TwbBrackets");
            Arc::new_cyclic(|self_ref| Self {
                self_ref: self_ref.clone(),
                def,
                nd,
            })
        }
    }

    impl Def for Brackets {
        fn def_base(&self) -> &DefBase {
            &self.def
        }

        fn as_dyn_def(&self) -> &dyn Def {
            self
        }

        fn def_ref(&self) -> DefRef {
            self.self_ref.upgrade().unwrap()
        }

        fn get_def_type(&self) -> DefType {
            DefType::dtIntegerFormater
        }

        fn get_def_type_name(&self) -> String {
            "TwbBrackets".to_owned()
        }

        fn duplicate(&self) -> DefRef {
            let this = Self::create();
            NamedDefBase::after_clone(&*this, self);
            this
        }

        fn as_named_def(&self) -> Option<&dyn NamedDef> {
            Some(self)
        }

        fn into_integer_def_formater(self: Arc<Self>) -> Option<Arc<dyn IntegerDefFormater>> {
            Some(self)
        }
    }

    impl NamedDef for Brackets {
        fn named_def_base(&self) -> &NamedDefBase {
            &self.nd
        }
    }

    impl IntegerDefFormater for Brackets {
        fn to_string(&self, int: i64, _element: ElementArg, for_summary: bool) -> String {
            if for_summary {
                format!("({int})")
            } else {
                format!("[{int}]")
            }
        }

        fn to_sort_key(&self, _int: i64, _element: ElementArg) -> String {
            String::new()
        }
    }

    fn plain(int_type: IntType) -> Arc<IntegerDef> {
        IntegerDef::create(args("Value"), int_type, None, 0)
    }

    #[test]
    fn reads_every_integer_type() {
        let _guard = test_lock();
        let data = [0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let expect = |int_type, text: &str| {
            assert_eq!(plain(int_type).to_string(Some(&data), None), text, "{int_type:?}");
        };
        expect(IntType::itU8, "254 <Warning: Expected 1 bytes of data, found 8>");
        set_check_expected_bytes(false);
        expect(IntType::itU8, "254");
        expect(IntType::itS8, "-2");
        expect(IntType::itU16, "65534");
        expect(IntType::itS16, "-2");
        expect(IntType::itU24, "16711679");
        expect(IntType::itU32, "4294967294");
        expect(IntType::itS32, "-2");
        expect(IntType::itU64, "-2");
        expect(IntType::itS64, "-2");
        expect(IntType::it0, "");
        // Counter: the two low bits select four bytes, the rest is the count.
        expect(IntType::itU6to30, "1073741823");
        assert_eq!(plain(IntType::itU6to30).to_string(Some(&[0b0001_0100]), None), "5");
    }

    #[test]
    fn short_data_is_an_error() {
        let _guard = test_lock();
        let def = plain(IntType::itU32);
        assert_eq!(
            def.to_string(Some(&[1, 2]), None),
            "<Error: Expected 4 bytes of data, found 2>"
        );
        assert_eq!(def.to_string(None, None), "<Error: Expected 4 bytes of data, found 0>");
        assert_eq!(def.check(Some(&[1, 2]), None), "Expected 4 bytes of data, found 2");
        assert_eq!(def.to_int(Some(&[1, 2]), None), 0);
        assert_eq!(def.to_edit_value(Some(&[1, 2]), None), "");
        assert_eq!(def.to_sort_key(Some(&[1, 2]), None, false), "");
        assert_eq!(def.to_native_value(None, None), Variant::Empty);
        set_check_expected_bytes(false);
        assert_eq!(def.to_string(Some(&[1, 2]), None), "");
    }

    #[test]
    fn sizes() {
        let _guard = test_lock();
        assert_eq!(plain(IntType::itU24).get_size(None, None), 3);
        assert_eq!(plain(IntType::itS64).get_default_size(None, None), 8);
        assert_eq!(plain(IntType::it0).get_size(Some(&[1]), None), 0);
        let counter = plain(IntType::itU6to30);
        assert_eq!(counter.get_size(None, None), 1);
        assert_eq!(counter.get_size(Some(&[0b01, 0]), None), 2);
        assert_eq!(counter.get_size(Some(&[0b10, 0, 0, 0]), None), 4);
        assert_eq!(counter.get_expected_length(0b10), 4);
        let terminated = IntegerDef::create(
            NamedDefArgs {
                terminator: true,
                ..args("Value")
            },
            IntType::itU16,
            None,
            0,
        );
        assert_eq!(terminated.get_size(Some(&[1, 2, 0]), None), 3);
        assert_eq!(terminated.get_expected_length(0), 3);
    }

    #[test]
    fn sort_keys_order_like_values() {
        let _guard = test_lock();
        let key = |int_type, data: &[u8]| plain(int_type).to_sort_key(Some(data), None, false);
        assert_eq!(key(IntType::itU8, &[0x0A]), "00A");
        assert_eq!(key(IntType::itS8, &[0xFF]), "07F");
        assert_eq!(key(IntType::itS8, &[0x01]), "081");
        assert_eq!(key(IntType::itU32, &[1, 0, 0, 0]), "000000001");
        assert_eq!(key(IntType::itS32, &[0xFF; 4]), "07FFFFFFF");
        assert_eq!(key(IntType::itS64, &[0xFF; 8]), "07FFFFFFFFFFFFFFF");
        assert_eq!(key(IntType::itS64, &[0; 8]), "08000000000000000");
    }

    #[test]
    fn formater_and_overlay() {
        let _guard = test_lock();
        let formater: Arc<dyn IntegerDefFormater> = Brackets::create();
        let def = IntegerDef::create(args("Value"), IntType::itU8, Some(formater.clone()), 7);
        // The first use of a formater does not duplicate it.
        assert!(Arc::ptr_eq(&def.in_formater().unwrap(), &formater));
        assert_eq!(formater.get_path(), "Value \\ TwbBrackets");
        assert_eq!(def.get_def_type_name(), "TwbBrackets");
        assert_eq!(def.to_string(Some(&[5]), None), "[5]");
        assert_eq!(def.to_summary(0, Some(&[5]), None, &mut None), "(5)");
        assert_eq!(def.to_edit_value(Some(&[5]), None), "5");
        assert_eq!(def.to_native_value(Some(&[5]), None), Variant::UInt(5));
        assert_eq!(def.in_default(), 7);
        assert!(!def.get_formater_can_change());

        // A second integer gets its own copy of the formater.
        let second = IntegerDef::create(args("Other"), IntType::itU8, Some(formater.clone()), 0);
        assert!(!Arc::ptr_eq(&second.in_formater().unwrap(), &formater));

        let overlay: IntOverlayCallback = Arc::new(|value, _, _| value * 2);
        let doubled = def.clone().add_overlay(Some(overlay));
        assert!(Arc::ptr_eq(&doubled, &def), "an integer without parent is not locked");
        assert_eq!(def.to_string(Some(&[5]), None), "[10]");
        assert_eq!(def.to_int(Some(&[5]), None), 10);

        let copy = IntegerDef::clone_from(&def);
        assert_eq!(copy.to_string(Some(&[5]), None), "[10]");
        assert!(!Arc::ptr_eq(&copy.in_formater().unwrap(), &formater));
    }

    #[test]
    fn to_str_callback_runs_last() {
        let _guard = test_lock();
        let callback: ToStrCallback = Arc::new(|value, _, _, callback_type| {
            if callback_type == CallbackType::ctToStr {
                value.push('!');
            }
        });
        let def = plain(IntType::itU8).set_to_str(Some(callback));
        assert_eq!(def.to_string(Some(&[3]), None), "3!");
        assert_eq!(def.to_summary(0, Some(&[3]), None, &mut None), "3");
        let flagged = def.include_flag(DefFlag::dfZeroSortKey);
        assert_eq!(flagged.to_sort_key(Some(&[3]), None, false), "003");
    }
}
