// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! The signature definitions: `TwbBaseSignatureDef`, `TwbSignatureDef`,
//! `TwbRecordMemberDef` and `TwbSubRecordDef`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use super::array::ArrayDef;
use super::def::{
    Def, DefBase, DefCell, DefKind, DefRef, DefSetters, LinksToCallback, NamedDef, NamedDefArgs, NamedDefBase,
    NamedDefSetters, ToStrCallback, ValueDef, ValueDefSetters, set_parent,
};
use super::element::{ElementArg, ElementRef};
use super::enum_def::EnumDef;
use super::misc::Variant;
use super::string::StringDefFormater;
use super::struct_def::StructDef;
use super::types::{CallbackType, DefFlag, DefType, Signature};

/// The signature of a definition that has none.
pub const NO_SIGNATURE: Signature = Signature([0; 4]);

/// Upstream `IwbSignatureDef`, implemented by `TwbBaseSignatureDef`.
pub trait SignatureDef: NamedDef {
    fn get_default_signature(&self) -> Signature {
        NO_SIGNATURE
    }

    fn get_signature(&self, _index: i32) -> Signature {
        self.get_default_signature()
    }

    fn get_signature_count(&self) -> i32 {
        i32::from(self.get_default_signature() != NO_SIGNATURE)
    }

    /// Whether this definition describes the subrecord `signature` of
    /// `container`. `data_container` is the subrecord, if it exists already.
    fn can_handle(&self, _container: ElementArg, signature: Signature, _data_container: ElementArg) -> bool {
        signature == self.get_default_signature()
    }
}

/// Port of `TwbBaseSignatureDef.GetFullName`: `EDID - Editor ID`.
pub fn signature_def_get_full_name<T: SignatureDef + ?Sized>(def: &T) -> String {
    if def.get_signature_count() > 0 {
        format!("{} - {}", def.get_signature(0), def.get_name())
    } else {
        def.get_name().to_owned()
    }
}

/// Upstream `IwbRecordMemberDef`, implemented by `TwbRecordMemberDef` and
/// `TwbSubRecordDef`: a subrecord or a group of subrecords of a record.
pub trait RecordMemberDef: SignatureDef {
    /// Port of `ToSummary`: a short text for the element `element`.
    fn to_summary(&self, depth: i32, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        record_member_def_to_summary(self, depth, element, links_to)
    }

    /// Port of `ToSummaryInternal`. Can be overridden.
    fn to_summary_internal(&self, _depth: i32, _element: ElementArg, _links_to: &mut Option<ElementRef>) -> String {
        String::new()
    }
}

/// Port of `TwbRecordMemberDef.ToSummary`.
pub fn record_member_def_to_summary<T: RecordMemberDef + ?Sized>(
    def: &T,
    depth: i32,
    element: ElementArg,
    links_to: &mut Option<ElementRef>,
) -> String {
    let mut result = String::new();
    if let Some(to_str) = def.named_def_base().nd_to_str.load().as_deref() {
        to_str(&mut result, None, element, CallbackType::ctToSummary);
    }
    if result.is_empty() {
        result = def.to_summary_internal(depth, element, links_to);
    }
    if links_to.is_none()
        && !result.is_empty()
        && let Some(element) = element
    {
        *links_to = element.get_links_to();
    }
    result
}

impl DefKind for dyn RecordMemberDef {
    fn duplicate_same(&self) -> Arc<Self> {
        self.duplicate()
            .into_record_member_def()
            .expect("the duplicate of a record member is a record member")
    }
}

/// The setter of `TwbRecordMemberDef` that the other setter traits do not have.
pub trait RecordMemberDefSetters: Sized {
    /// Port of `SetRequired`.
    fn set_required(self, required: bool) -> Self;
}

impl<T: RecordMemberDef + DefKind + ?Sized> RecordMemberDefSetters for Arc<T> {
    fn set_required(self, required: bool) -> Self {
        let this = if self.def_base().def_is_locked() {
            self.duplicate_same()
        } else {
            self
        };
        this.def_base().def_required.store(required, Ordering::Relaxed);
        this
    }
}

/// Implements the methods of [`Def`] and [`NamedDef`] that every record
/// member class implements in the same way. The class has the fields
/// `self_ref`, `def` and `nd` and a `clone_from` constructor.
macro_rules! record_member_plumbing {
    (Def) => {
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

        fn as_signature_def(&self) -> Option<&dyn SignatureDef> {
            Some(self)
        }

        fn as_record_member_def(&self) -> Option<&dyn RecordMemberDef> {
            Some(self)
        }

        fn into_record_member_def(self: Arc<Self>) -> Option<Arc<dyn RecordMemberDef>> {
            Some(self)
        }
    };
    (NamedDef) => {
        fn named_def_base(&self) -> &NamedDefBase {
            &self.nd
        }

        fn get_full_name(&self) -> String {
            signature_def_get_full_name(self)
        }
    };
}
pub(crate) use record_member_plumbing;

/// Upstream `TwbSubRecordDef`: a subrecord with one value.
pub struct SubRecordDef {
    self_ref: Weak<SubRecordDef>,
    def: DefBase,
    nd: NamedDefBase,
    so_signatures: Vec<Signature>,
    sr_value: DefCell<Arc<dyn ValueDef>>,
    sr_size_match: bool,
    sr_has_unused_data: AtomicBool,
}

impl SubRecordDef {
    /// Port of both `TwbSubRecordDef.Create` constructors. An empty name
    /// becomes the first signature. `args.terminator` is not used, as upstream.
    pub fn create(
        args: NamedDefArgs,
        signatures: &[Signature],
        value: Option<Arc<dyn ValueDef>>,
        size_match: bool,
    ) -> Arc<Self> {
        assert!(!signatures.is_empty());
        let name = if args.name.is_empty() {
            signatures[0].to_string()
        } else {
            args.name.clone()
        };
        let (def, nd) = NamedDefBase::create(NamedDefArgs {
            name,
            terminator: false,
            ..args
        });
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| {
            let parent: Weak<dyn Def> = self_ref.clone();
            Self {
                self_ref: self_ref.clone(),
                def,
                nd,
                so_signatures: signatures.to_vec(),
                sr_value: DefCell::new(value.map(|value| set_parent(value, &parent, false))),
                sr_size_match: size_match,
                sr_has_unused_data: AtomicBool::new(false),
            }
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbSubRecordDef.Clone`.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(
            NamedDefBase::clone_args(source),
            &source.so_signatures,
            source.get_value(),
            source.sr_size_match,
        );
        NamedDefBase::after_clone(&*this, source);
        this
    }

    /// The definition of the data of the subrecord.
    pub fn get_value(&self) -> Option<Arc<dyn ValueDef>> {
        self.sr_value.load().as_deref().cloned()
    }

    /// Port of `HasUnusedData`: remembers that a subrecord had more data than
    /// its definition uses.
    pub fn has_unused_data(&self) {
        self.sr_has_unused_data.store(true, Ordering::Relaxed);
    }

    pub fn sr_has_unused_data(&self) -> bool {
        self.sr_has_unused_data.load(Ordering::Relaxed)
    }

    fn unlocked(self: Arc<Self>) -> Arc<Self> {
        if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        }
    }

    /// Replaces the value by the result of a setter on it. The value is
    /// locked, so the setter returns a duplicate, which gets this subrecord as
    /// its parent.
    fn replace_value(&self, change: impl FnOnce(Arc<dyn ValueDef>) -> Arc<dyn ValueDef>) {
        if let Some(value) = self.get_value() {
            let parent: Weak<dyn Def> = self.self_ref.clone();
            self.sr_value.set(Some(set_parent(change(value), &parent, false)));
        }
    }

    fn on_value(self: Arc<Self>, change: impl FnOnce(Arc<dyn ValueDef>) -> Arc<dyn ValueDef>) -> Arc<Self> {
        let this = self.unlocked();
        this.replace_value(change);
        this
    }

    /// Upstream fails when the value is not a structure.
    fn on_struct(self: Arc<Self>, change: impl FnOnce(Arc<StructDef>) -> Arc<StructDef>) -> Arc<Self> {
        self.on_value(|value| {
            change(
                value
                    .into_struct_def()
                    .expect("the value of the subrecord is a structure"),
            )
        })
    }

    /// Upstream fails when the value is not an array.
    fn on_array(self: Arc<Self>, change: impl FnOnce(Arc<ArrayDef>) -> Arc<ArrayDef>) -> Arc<Self> {
        self.on_value(|value| change(value.into_array_def().expect("the value of the subrecord is an array")))
    }

    pub fn include_flag_on_value(self: Arc<Self>, flag: DefFlag, only_when_true: bool) -> Arc<Self> {
        self.on_value(|value| value.include_flag_when(flag, only_when_true))
    }

    pub fn set_default_edit_value(self: Arc<Self>, value: &str) -> Arc<Self> {
        self.on_value(|def| def.set_default_edit_value(value))
    }

    pub fn set_default_native_value(self: Arc<Self>, value: Variant) -> Arc<Self> {
        self.on_value(|def| def.set_default_native_value(value))
    }

    pub fn set_links_to_callback_on_value(self: Arc<Self>, callback: Option<LinksToCallback>) -> Arc<Self> {
        self.on_value(|def| def.set_links_to_callback(callback))
    }

    pub fn set_summary_links_to_callback_on_value(self: Arc<Self>, callback: Option<LinksToCallback>) -> Arc<Self> {
        self.on_value(|def| def.set_summary_links_to_callback(callback))
    }

    pub fn set_summary_key_on_value(self: Arc<Self>, summary_key: &[i32]) -> Arc<Self> {
        self.on_struct(|def| def.set_summary_key(summary_key))
    }

    pub fn set_summary_prefix_suffix_on_value(self: Arc<Self>, index: i32, prefix: &str, suffix: &str) -> Arc<Self> {
        self.on_struct(|def| def.set_summary_member_prefix_suffix(index, prefix, suffix))
    }

    pub fn set_summary_member_max_depth_on_value(self: Arc<Self>, index: i32, max_depth: i32) -> Arc<Self> {
        self.on_struct(|def| def.set_summary_member_max_depth(index, max_depth))
    }

    /// Upstream `SetSummaryDelimiterOnValue` of a subrecord with a structure.
    pub fn set_summary_delimiter_on_struct(self: Arc<Self>, delimiter: &str) -> Arc<Self> {
        self.on_struct(|def| def.set_summary_delimiter(delimiter))
    }

    /// Upstream `SetSummaryDelimiterOnValue` of a subrecord with an array.
    pub fn set_summary_delimiter_on_array(self: Arc<Self>, delimiter: &str) -> Arc<Self> {
        self.on_array(|def| def.set_summary_delimiter(delimiter))
    }

    pub fn set_summary_passthrough_max_count_on_value(self: Arc<Self>, count: i32) -> Arc<Self> {
        self.on_array(|def| def.set_summary_passthrough_max_count(count))
    }

    pub fn set_summary_passthrough_max_length_on_value(self: Arc<Self>, length: i32) -> Arc<Self> {
        self.on_array(|def| def.set_summary_passthrough_max_length(length))
    }

    pub fn set_summary_passthrough_max_depth_on_value(self: Arc<Self>, depth: i32) -> Arc<Self> {
        self.on_array(|def| def.set_summary_passthrough_max_depth(depth))
    }

    pub fn set_default_edit_values_on_value(self: Arc<Self>, values: &[&str]) -> Arc<Self> {
        self.on_array(|def| def.set_default_edit_values(values))
    }

    /// Port of `SetDefaultEditValues`: does nothing when the value is not an array.
    pub fn set_default_edit_values(self: Arc<Self>, values: &[&str]) -> Arc<Self> {
        self.on_value(|value| match value.clone().into_array_def() {
            Some(array) => array.set_default_edit_values(values),
            None => value,
        })
    }

    /// Port of `SetCountPathOnValue` with one path.
    pub fn set_count_path_on_value(self: Arc<Self>, value: &str, use_for_count_callback: bool) -> Arc<Self> {
        self.set_count_paths_on_value(&[value], use_for_count_callback)
    }

    /// Port of `SetCountPathOnValue` with several paths.
    pub fn set_count_paths_on_value(self: Arc<Self>, values: &[&str], use_for_count_callback: bool) -> Arc<Self> {
        if self.def.def_is_locked() {
            // A locked subrecord stays as it is when its array has the paths already.
            let array = self
                .get_value()
                .and_then(|value| value.into_array_def())
                .expect("the value of the subrecord is an array");
            if array.get_count_callback().is_none() {
                let new_paths: Vec<&str> = values.iter().copied().filter(|value| !value.is_empty()).collect();
                let old_paths = array.get_count_paths();
                let same =
                    new_paths.len() == old_paths.len() && new_paths.iter().zip(&old_paths).all(|(new, old)| new == old);
                if same && (!use_for_count_callback || new_paths.is_empty()) {
                    return self;
                }
            }
        }
        self.on_array(|def| def.set_count_paths(values, use_for_count_callback))
    }

    pub fn set_count_from_enum_on_value(self: Arc<Self>, enum_def: Option<Arc<EnumDef>>) -> Arc<Self> {
        self.on_array(|def| def.set_count_from_enum(enum_def))
    }

    pub fn set_wrongly_assumed_fixed_size_per_element_on_value(self: Arc<Self>, size: i32) -> Arc<Self> {
        self.on_array(|def| def.set_wrongly_assumed_fixed_size_per_element(size))
    }

    /// Port of `SetFormaterOnValue`. Upstream fails when the value is not a string.
    pub fn set_formater_on_value(self: Arc<Self>, formater: Option<Arc<dyn StringDefFormater>>) -> Arc<Self> {
        self.on_value(|value| {
            if let Some(string) = value.clone().into_string_def() {
                string.set_formater(formater)
            } else {
                value
                    .into_len_string_def()
                    .expect("the value of the subrecord is a string")
                    .set_formater(formater)
            }
        })
    }

    /// Port of `ForValue`: runs `callback` with the value.
    pub fn for_value(self: Arc<Self>, callback: impl FnOnce(Option<Arc<dyn ValueDef>>)) -> Arc<Self> {
        let this = self.unlocked();
        callback(this.get_value());
        this
    }
}

impl Def for SubRecordDef {
    record_member_plumbing!(Def);

    fn get_def_type(&self) -> DefType {
        DefType::dtSubRecord
    }

    fn get_def_type_name(&self) -> String {
        // Upstream fails for a subrecord without value.
        let value = self
            .get_value()
            .map(|value| value.get_def_type_name())
            .unwrap_or_default();
        format!("SubRecord of {value}")
    }

    fn as_sub_record_def(&self) -> Option<&SubRecordDef> {
        Some(self)
    }

    fn init_from_parent_do_children(&self) {
        if let Some(value) = self.sr_value.load().as_deref() {
            value.init_from_parent();
        }
    }
}

impl NamedDef for SubRecordDef {
    record_member_plumbing!(NamedDef);

    /// Port of the override of `SetToStr`: the callback goes to the value.
    fn apply_to_str(&self, to_str: Option<ToStrCallback>) {
        if self.sr_value.is_assigned() {
            self.replace_value(|value| value.set_to_str(to_str));
        } else {
            self.nd.nd_to_str.set(to_str);
        }
    }
}

impl SignatureDef for SubRecordDef {
    fn get_default_signature(&self) -> Signature {
        self.so_signatures[0]
    }

    fn get_signature(&self, index: i32) -> Signature {
        self.so_signatures[index as usize]
    }

    fn get_signature_count(&self) -> i32 {
        self.so_signatures.len() as i32
    }

    fn can_handle(&self, _container: ElementArg, signature: Signature, data_container: ElementArg) -> bool {
        let mut result = signature == self.get_default_signature();
        if result
            && self.sr_size_match
            && let Some(data_container) = data_container
            && let Some(value) = self.sr_value.load().as_deref()
        {
            result = data_container.get_data_size() == value.get_default_size(None, None);
        }
        result
    }
}

impl RecordMemberDef for SubRecordDef {
    fn to_summary(&self, depth: i32, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        let mut result = String::new();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, None, element, CallbackType::ctToSummary);
        }
        if result.is_empty()
            && let Some(element) = element
            && let Some(value) = self.sr_value.load().as_deref()
            && let Some(data_container) = element.as_data_container()
        {
            result = value.to_summary(depth, data_container.get_data(), Some(element), links_to);
        }
        if links_to.is_none()
            && !result.is_empty()
            && let Some(element) = element
        {
            *links_to = element.get_links_to();
        }
        result
    }
}

impl DefKind for SubRecordDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::array::ArrayDefArgs;
    use super::super::globals::test_lock;
    use super::super::integer::IntegerDef;
    use super::super::struct_def::StructDefArgs;
    use super::super::types::{ConflictPriority, IntType};
    use super::*;

    const EDID: Signature = Signature::new(b"EDID");
    const DATA: Signature = Signature::new(b"DATA");
    const DNAM: Signature = Signature::new(b"DNAM");

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

    fn int(name: &str) -> Arc<dyn ValueDef> {
        IntegerDef::create(args(name), IntType::itU32, None, 0)
    }

    #[test]
    fn names_and_signatures() {
        let _guard = test_lock();
        let def = SubRecordDef::create(args("Value"), &[DATA, DNAM], Some(int("")), false);
        assert_eq!(def.get_name(), "Value");
        assert_eq!(def.get_full_name(), "DATA - Value");
        assert_eq!(def.get_default_signature(), DATA);
        assert_eq!(def.get_signature(1), DNAM);
        assert_eq!(def.get_signature_count(), 2);
        assert!(def.can_handle(None, DATA, None));
        // Only the first signature is the default signature.
        assert!(!def.can_handle(None, DNAM, None));
        assert_eq!(def.get_def_type_name(), "SubRecord of Unsigned DWord");
        assert_eq!(def.get_value().unwrap().get_full_path(), "DATA - Value \\ ");
        assert_eq!(def.to_summary(0, None, &mut None), "");

        let unnamed = SubRecordDef::create(args(""), &[EDID], None, false);
        assert_eq!(unnamed.get_name(), "EDID");
        assert_eq!(unnamed.get_full_name(), "EDID - EDID");
        assert!(unnamed.get_value().is_none());
    }

    #[test]
    fn to_str_goes_to_the_value() {
        let _guard = test_lock();
        let def = SubRecordDef::create(args("Value"), &[DATA], Some(int("")), false);
        let value = def.get_value().unwrap();
        let callback: ToStrCallback = Arc::new(|text, _, _, _| text.push('!'));
        let same = def.clone().set_to_str(Some(callback));
        assert!(Arc::ptr_eq(&same, &def));
        // The value was locked, so the subrecord has a changed duplicate of it.
        let new_value = def.get_value().unwrap();
        assert!(!Arc::ptr_eq(&new_value, &value));
        assert_eq!(new_value.to_string(Some(&[1, 0, 0, 0]), None), "1!");
        assert_eq!(value.to_string(Some(&[1, 0, 0, 0]), None), "1");
        assert!(!def.nd.nd_to_str.is_assigned());
        assert_eq!(new_value.get_path(), "Value \\ ");
    }

    #[test]
    fn setters_on_the_value() {
        let _guard = test_lock();
        let structure: Arc<dyn ValueDef> = StructDef::create(
            args("Bounds"),
            StructDefArgs {
                members: vec![int("Min"), int("Max")],
                ..StructDefArgs::default()
            },
        );
        let def = SubRecordDef::create(args("Bounds"), &[DATA], Some(structure), false)
            .set_summary_key_on_value(&[0, 1])
            .set_summary_delimiter_on_struct(" to ")
            .include_flag_on_value(DefFlag::dfCollapsed, true)
            .set_required(true);
        assert!(def.get_required());
        let value = def.get_value().unwrap();
        assert!(value.get_def_flags().contains(DefFlag::dfCollapsed));
        assert!(value.as_struct_def().is_some());
        assert_eq!(value.get_parent().unwrap().get_def_id(), def.get_def_id());

        let array: Arc<dyn ValueDef> = ArrayDef::create(
            args("Items"),
            ArrayDefArgs {
                element: Some(int("Item")),
                count: 0,
                count_callback: None,
                labels: Vec::new(),
                sorted: false,
                can_add_to: true,
                terminated: false,
            },
        );
        let def = SubRecordDef::create(args("Items"), &[DNAM], Some(array), false)
            .set_count_paths_on_value(&["Count"], false)
            .set_summary_passthrough_max_count_on_value(3);
        let array = def.get_value().unwrap().into_array_def().unwrap();
        assert_eq!(array.get_count_paths(), ["Count"]);

        // A locked subrecord with the same paths stays as it is.
        let _record = SubRecordDef::create(args("Outer"), &[DATA], None, false);
        let parent: DefRef = _record.clone();
        let locked = set_parent(def.clone(), &Arc::downgrade(&parent), false);
        let same = locked.clone().set_count_paths_on_value(&["Count"], false);
        assert!(Arc::ptr_eq(&same, &locked));
        let changed = locked.clone().set_count_paths_on_value(&["Other"], false);
        assert!(!Arc::ptr_eq(&changed, &locked));
    }

    #[test]
    fn clone_and_dynamic_setters() {
        let _guard = test_lock();
        let def = SubRecordDef::create(args("Value"), &[DATA], Some(int("")), true);
        let copy = SubRecordDef::clone_from(&def);
        assert_eq!(copy.get_full_name(), "DATA - Value");
        assert!(!Arc::ptr_eq(&copy.get_value().unwrap(), &def.get_value().unwrap()));
        let member: Arc<dyn RecordMemberDef> = def.clone();
        let named = member.clone().set_summary_name("V");
        assert!(Arc::ptr_eq(&named, &member));
        assert_eq!(member.get_summary_name(), "V");
        def.has_unused_data();
        assert!(def.sr_has_unused_data());
    }
}
