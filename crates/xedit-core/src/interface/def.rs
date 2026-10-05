// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! The base of the definition model: `TwbDef`, `TwbNamedDef` and `TwbValueDef`.
//!
//! # How the Pascal class hierarchy maps to Rust
//!
//! - Each Pascal class is a struct that embeds the field blocks of its
//!   ancestors (`DefBase`, `NamedDefBase`, `ValueDefBase`), with the upstream
//!   field names.
//! - Each Pascal interface is a trait (`IwbDef` is [`Def`], `IwbNamedDef` is
//!   [`NamedDef`]). A virtual method is a trait method whose default body is
//!   the implementation of the base class. An override that calls `inherited`
//!   calls the free function that holds the base implementation.
//! - `Supports(aDef, IwbXxx, x)` is `def.as_xxx()`.
//! - Definitions are shared as `Arc<dyn Trait>` and are `Send + Sync`.
//!
//! # Changes after construction
//!
//! Upstream changes a definition after its constructor ran: setters such as
//! `SetToStr`, and `InitFromParent`. A setter on a locked definition (one that
//! has a parent or is a template) works on a duplicate. The port keeps this.
//! Fields that change after construction are atomics or [`DefCell`]s, which
//! read without taking a lock.

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use arc_swap::{ArcSwapOption, Guard};

use super::array::ArrayDef;
use super::byte_array::ByteArrayDef;
use super::element::{DataPtr, ElementArg, ElementRef};
use super::enum_def::EnumDef;
use super::flags::{FlagDef, FlagsDef};
use super::float::FloatDef;
use super::globals::{
    collapse_benign_array, hide_unused, is_internal_edit, make_unknown_elements_unique, report_mode, report_unknown,
};
use super::guid::GuidDef;
use super::integer::{IntegerDefFormater, IntegerDefInterface};
use super::len_string::LenStringDef;
use super::misc::{Variant, shorten_text};
use super::resolvable::{ResolvableDef, UnionDef};
use super::string::{StringDef, StringDefFormater};
use super::struct_def::StructDef;
use super::types::{
    CallbackType, ConflictPriority, DefFlag, DefFlags, DefType, EditType, EnumSet, PascalEnum, def_flags_dont_clone,
    def_flags_inherit_down, def_flags_inherit_up,
};

/// A reference to a definition, upstream `IwbDef`.
pub type DefRef = Arc<dyn Def>;

pub type AfterLoadCallback = Arc<dyn Fn(&ElementRef) + Send + Sync>;
pub type AfterSetCallback = Arc<dyn Fn(&ElementRef, &Variant, &Variant) + Send + Sync>;
pub type DontShowCallback = Arc<dyn Fn(ElementArg) -> bool + Send + Sync>;
pub type GetConflictPriority = Arc<dyn Fn(ElementArg, &mut ConflictPriority) + Send + Sync>;
pub type IsRemovableCallback = Arc<dyn Fn(ElementArg) -> bool + Send + Sync>;
pub type LinksToCallback = Arc<dyn Fn(ElementArg) -> Option<ElementRef> + Send + Sync>;
pub type SetToDefaultCallback = Arc<dyn Fn(DataPtr, ElementArg) -> bool + Send + Sync>;
pub type ToStrCallback = Arc<dyn Fn(&mut String, DataPtr, ElementArg, CallbackType) + Send + Sync>;

/// A field of a definition that a setter can change after construction.
/// Reading does not take a lock.
pub struct DefCell<T>(ArcSwapOption<T>);

impl<T> DefCell<T> {
    pub fn new(value: Option<T>) -> Self {
        Self(ArcSwapOption::new(value.map(Arc::new)))
    }

    pub fn set(&self, value: Option<T>) {
        self.0.store(value.map(Arc::new));
    }

    /// Copies the content of `source`, as an assignment between two Pascal fields.
    pub fn assign(&self, source: &DefCell<T>) {
        self.0.store(source.0.load_full());
    }

    /// The current value. Use `.as_deref()` on the result to get an `Option<&T>`.
    pub fn load(&self) -> Guard<Option<Arc<T>>> {
        self.0.load()
    }

    /// Pascal `Assigned(field)`.
    pub fn is_assigned(&self) -> bool {
        self.0.load().is_some()
    }
}

impl<T> Default for DefCell<T> {
    fn default() -> Self {
        Self::new(None)
    }
}

/// A Pascal set field that changes after construction.
pub struct AtomicEnumSet<T: PascalEnum> {
    bits: AtomicU64,
    marker: PhantomData<T>,
}

impl<T: PascalEnum> AtomicEnumSet<T> {
    pub fn new(value: EnumSet<T>) -> Self {
        Self {
            bits: AtomicU64::new(value.bits()),
            marker: PhantomData,
        }
    }

    pub fn get(&self) -> EnumSet<T> {
        EnumSet::from_bits(self.bits.load(Ordering::Relaxed))
    }

    pub fn set(&self, value: EnumSet<T>) {
        self.bits.store(value.bits(), Ordering::Relaxed);
    }

    pub fn contains(&self, value: T) -> bool {
        self.get().contains(value)
    }

    pub fn include(&self, value: T) {
        self.bits.fetch_or(EnumSet::of(&[value]).bits(), Ordering::Relaxed);
    }

    pub fn exclude(&self, value: T) {
        self.bits.fetch_and(!EnumSet::of(&[value]).bits(), Ordering::Relaxed);
    }
}

impl<T: PascalEnum> Default for AtomicEnumSet<T> {
    fn default() -> Self {
        Self::new(EnumSet::empty())
    }
}

/// The fields of `TwbDef`.
pub struct DefBase {
    def_source: DefCell<DefRef>,
    def_parent: OnceLock<Weak<dyn Def>>,
    pub def_priority: ConflictPriority,
    pub def_get_cp: Option<GetConflictPriority>,
    pub def_flags: AtomicEnumSet<DefFlag>,
    def_collapsed_gen: AtomicI32,
    pub def_required: AtomicBool,
    def_used: AtomicBool,
    /// Read by `Report`, which is ported with the signature definitions.
    #[allow(dead_code)]
    def_reported: AtomicBool,
    def_possibly_required: AtomicBool,
    def_not_required: AtomicBool,
    pub is_unknown: AtomicBool,
    is_unknown_checked: AtomicBool,
    /// Value to the paths of up to 20 elements that have it. Filled in report mode.
    unknown_values: Mutex<BTreeMap<String, BTreeSet<String>>>,
}

impl DefBase {
    /// Port of `TwbDef.Create`.
    pub fn create(mut priority: ConflictPriority, required: bool, get_cp: Option<GetConflictPriority>) -> Self {
        let def_flags = AtomicEnumSet::default();
        if priority == ConflictPriority::cpTranslate {
            def_flags.include(DefFlag::dfTranslatable);
            priority = ConflictPriority::cpNormal;
        }
        Self {
            def_source: DefCell::default(),
            def_parent: OnceLock::new(),
            def_priority: priority,
            def_get_cp: get_cp,
            def_flags,
            def_collapsed_gen: AtomicI32::new(0),
            def_required: AtomicBool::new(required),
            def_used: AtomicBool::new(false),
            def_reported: AtomicBool::new(false),
            def_possibly_required: AtomicBool::new(false),
            def_not_required: AtomicBool::new(false),
            is_unknown: AtomicBool::new(false),
            is_unknown_checked: AtomicBool::new(false),
            unknown_values: Mutex::new(BTreeMap::new()),
        }
    }

    /// Port of `TwbDef.AfterClone`.
    pub fn after_clone(&self, source: &dyn Def) {
        self.def_source.set(Some(source.def_ref()));
        self.def_flags
            .set(source.def_base().def_flags.get() - def_flags_dont_clone());
    }

    /// Port of `TwbDef.AfterConstruction`. Every constructor calls it last.
    pub fn after_construction(def: &dyn Def) {
        let base = def.def_base();
        if collapse_benign_array()
            && matches!(
                def.get_def_type(),
                DefType::dtSubRecord | DefType::dtSubRecordArray | DefType::dtArray
            )
            && base.def_priority == ConflictPriority::cpBenign
            && base.def_get_cp.is_none()
        {
            base.def_flags.include(DefFlag::dfCollapsed);
        }
    }

    /// Forgets the definition this one was cloned from.
    pub fn clear_def_source(&self) {
        self.def_source.set(None);
    }

    pub fn def_parent(&self) -> Option<DefRef> {
        self.def_parent.get().and_then(Weak::upgrade)
    }

    fn parent_is(&self, parent: &Weak<dyn Def>) -> bool {
        self.def_parent
            .get()
            .is_some_and(|current| Weak::ptr_eq(current, parent))
    }

    pub fn def_required(&self) -> bool {
        self.def_required.load(Ordering::Relaxed)
    }

    pub fn def_internal_edit_only(&self) -> bool {
        self.def_flags.contains(DefFlag::dfInternalEditOnly)
    }

    /// Port of `TwbDef.defIsLocked`.
    pub fn def_is_locked(&self) -> bool {
        self.def_parent.get().is_some() || self.def_flags.contains(DefFlag::dfTemplate)
    }
}

/// Upstream `IwbDef`, implemented by `TwbDef`.
pub trait Def: Send + Sync + 'static {
    fn def_base(&self) -> &DefBase;

    /// `self` as a trait object. Default methods cannot make this conversion.
    fn as_dyn_def(&self) -> &dyn Def;

    /// A new reference to `self`.
    fn def_ref(&self) -> DefRef;

    fn get_def_type(&self) -> DefType;

    fn get_def_type_name(&self) -> String;

    /// Port of `TwbDef.Duplicate`: calls the `Clone` constructor of the class.
    fn duplicate(&self) -> DefRef;

    fn as_named_def(&self) -> Option<&dyn NamedDef> {
        None
    }

    fn as_value_def(&self) -> Option<&dyn ValueDef> {
        None
    }

    fn into_value_def(self: Arc<Self>) -> Option<Arc<dyn ValueDef>> {
        None
    }

    fn as_empty_def(&self) -> Option<&dyn EmptyDefInterface> {
        None
    }

    fn as_integer_def(&self) -> Option<&dyn IntegerDefInterface> {
        None
    }

    fn as_enum_def(&self) -> Option<&EnumDef> {
        None
    }

    fn as_flags_def(&self) -> Option<&FlagsDef> {
        None
    }

    fn as_float_def(&self) -> Option<&FloatDef> {
        None
    }

    fn as_byte_array_def(&self) -> Option<&ByteArrayDef> {
        None
    }

    fn as_string_def(&self) -> Option<&StringDef> {
        None
    }

    fn as_len_string_def(&self) -> Option<&LenStringDef> {
        None
    }

    fn as_guid_def(&self) -> Option<&GuidDef> {
        None
    }

    fn as_struct_def(&self) -> Option<&StructDef> {
        None
    }

    fn as_array_def(&self) -> Option<&ArrayDef> {
        None
    }

    fn as_resolvable_def(&self) -> Option<&dyn ResolvableDef> {
        None
    }

    fn as_union_def(&self) -> Option<&UnionDef> {
        None
    }

    fn into_string_def_formater(self: Arc<Self>) -> Option<Arc<dyn StringDefFormater>> {
        None
    }

    fn into_flags_def(self: Arc<Self>) -> Option<Arc<FlagsDef>> {
        None
    }

    fn as_flag_def(&self) -> Option<&FlagDef> {
        None
    }

    fn into_integer_def_formater(self: Arc<Self>) -> Option<Arc<dyn IntegerDefFormater>> {
        None
    }

    /// Port of `GetDefID`: the identity of the definition object.
    fn get_def_id(&self) -> usize {
        std::ptr::from_ref(self.def_base()) as usize
    }

    /// Port of `TwbDef.Equals`.
    fn equals(&self, def: Option<&dyn Def>) -> bool {
        def.is_some_and(|def| def.get_def_id() == self.get_def_id())
    }

    fn get_conflict_priority(&self, element: ElementArg) -> ConflictPriority {
        let base = self.def_base();
        let mut result = base.def_priority;
        if let Some(get_cp) = &base.def_get_cp {
            get_cp(element, &mut result);
        }
        result
    }

    fn get_conflict_priority_can_change(&self) -> bool {
        self.def_base().def_get_cp.is_some()
    }

    fn get_required(&self) -> bool {
        self.def_base().def_required()
    }

    /// The default is `TwbNamedDef.GetDontShow`, because every class is a named definition.
    fn get_dont_show(&self, element: ElementArg) -> bool {
        match self.as_named_def() {
            Some(named) => named_def_get_dont_show(named, element),
            None => false,
        }
    }

    /// The default is `TwbNamedDef.GetHasDontShow`.
    fn get_has_dont_show(&self) -> bool {
        match self.as_named_def() {
            Some(named) => {
                let base = named.named_def_base();
                base.nd_dont_show.is_assigned() || (hide_unused() && base.nd_unused())
            }
            None => false,
        }
    }

    fn get_no_reach(&self) -> bool {
        false
    }

    fn get_parent(&self) -> Option<DefRef> {
        self.def_base().def_parent()
    }

    fn get_def_flags(&self) -> DefFlags {
        self.def_base().def_flags.get()
    }

    fn get_collapsed(&self) -> bool {
        self.def_base().def_flags.contains(DefFlag::dfCollapsed)
    }

    fn set_collapsed(&self, value: bool) {
        let base = self.def_base();
        if value != self.get_collapsed() {
            base.def_collapsed_gen.fetch_add(1, Ordering::Relaxed);
        }
        if value {
            base.def_flags.include(DefFlag::dfCollapsed);
        } else {
            base.def_flags.exclude(DefFlag::dfCollapsed);
        }
    }

    fn get_collapsed_gen(&self) -> i32 {
        self.def_base().def_collapsed_gen.load(Ordering::Relaxed)
    }

    /// Position of `child` among the children, or -1.
    fn get_child_pos(&self, _child: &dyn Def) -> i32 {
        -1
    }

    /// Port of `TwbDef.Used`. Records usage for the report mode of xDump.
    fn used(&self, element: ElementArg, s: &str) {
        if !report_mode() {
            return;
        }
        let base = self.def_base();
        base.def_used.store(true, Ordering::Relaxed);
        if !base.is_unknown.load(Ordering::Relaxed) && !base.is_unknown_checked.swap(true, Ordering::Relaxed) {
            let parent = base.def_parent();
            if let Some(named) = parent.as_deref().and_then(Def::as_named_def)
                && named.get_name().to_lowercase().contains("unknown")
            {
                base.is_unknown.store(true, Ordering::Relaxed);
            }
        }
        if report_unknown()
            && base.is_unknown.load(Ordering::Relaxed)
            && !s.is_empty()
            && let Some(element) = element
        {
            let mut unknown_values = base.unknown_values.lock().unwrap();
            if unknown_values.len() < 20 || unknown_values.contains_key(s) {
                let paths = unknown_values.entry(s.to_owned()).or_default();
                if paths.len() < 20 {
                    paths.insert(element.get_full_path());
                }
            }
        }
    }

    fn possibly_required(&self) {
        self.def_base().def_possibly_required.store(true, Ordering::Relaxed);
    }

    fn not_required(&self) {
        self.def_base().def_not_required.store(true, Ordering::Relaxed);
    }

    fn is_not_required(&self) -> bool {
        self.def_base().def_not_required.load(Ordering::Relaxed)
    }

    /// Port of `IncludeFlagNoClone`.
    fn include_flag_no_clone(&self, flag: DefFlag, only_when_true: bool) {
        if only_when_true {
            self.def_base().def_flags.include(flag);
        }
    }

    /// Called when the parent was set. Can be overridden.
    fn parent_set(&self) {}

    /// Port of `TwbDef.InitFromParent`.
    fn init_from_parent(&self) {
        self.init_from_parent_before_children();
        self.init_from_parent_do_children();
        self.init_from_parent_after_children();
    }

    /// The default is `TwbNamedDef.InitFromParentBeforeChildren`.
    fn init_from_parent_before_children(&self) {
        match self.as_named_def() {
            Some(named) => named_def_init_from_parent_before_children(named),
            None => def_init_from_parent_before_children(self.as_dyn_def()),
        }
    }

    /// Can be overridden.
    fn init_from_parent_do_children(&self) {}

    fn init_from_parent_after_children(&self) {
        def_init_from_parent_after_children(self.as_dyn_def());
    }
}

/// Port of `TwbDef.InitFromParentBeforeChildren`.
pub fn def_init_from_parent_before_children(def: &dyn Def) {
    let base = def.def_base();
    let parent = base.def_parent();
    if let Some(parent) = &parent {
        base.def_flags
            .set(base.def_flags.get() | (parent.def_base().def_flags.get() & def_flags_inherit_up()));
    }
    if base.def_flags.contains(DefFlag::dfUnmappedFormID) {
        base.def_flags.include(DefFlag::dfCanContainUnmappedFormID);
    }
    // UPSTREAM-QUIRK: the second test upstream asks for IwbUnionDef but stores the
    // result in an IwbSubRecordUnionDef variable. Both read the Required property,
    // so the effect is the intended one. The test for IwbSubRecordUnionDef
    // arrives with that class.
    if !base.def_required()
        && let Some(parent) = &parent
        && parent.as_union_def().is_some()
        && parent.get_required()
    {
        base.def_required.store(true, Ordering::Relaxed);
    }
}

/// Port of `TwbDef.InitFromParentAfterChildren`.
pub fn def_init_from_parent_after_children(def: &dyn Def) {
    let base = def.def_base();
    if let Some(parent) = base.def_parent() {
        let parent_flags = &parent.def_base().def_flags;
        parent_flags.set(parent_flags.get() | (base.def_flags.get() & def_flags_inherit_down()));
    }
}

/// Port of `TwbDef.GetRoot`: the definition this one was cloned from, transitively.
pub fn get_root(def: &DefRef) -> DefRef {
    match def.def_base().def_source.load().as_deref() {
        Some(source) => get_root(source),
        None => def.clone(),
    }
}

/// A definition type that can duplicate itself without losing its static type.
/// Implemented by the classes and by the trait objects of the interfaces.
pub trait DefKind: Def {
    fn duplicate_same(&self) -> Arc<Self>;
}

/// Port of `TwbDef.SetParent`. Returns the definition that now has `parent`,
/// which is a duplicate when `child` was locked.
pub fn set_parent<T: DefKind + ?Sized>(child: Arc<T>, parent: &Weak<dyn Def>, force_duplicate: bool) -> Arc<T> {
    let base = child.def_base();
    if base.parent_is(parent) {
        return child;
    }
    if base.def_is_locked() || force_duplicate {
        set_parent(child.duplicate_same(), parent, false)
    } else {
        let _ = base.def_parent.set(parent.clone());
        child.parent_set();
        child
    }
}

/// Returns `def`, or a duplicate when `def` is locked. The start of every
/// upstream setter: `if defIsLocked then Exit(Duplicate.SetXxx(...))`.
fn unlocked<T: DefKind + ?Sized>(def: Arc<T>) -> Arc<T> {
    if def.def_base().def_is_locked() {
        def.duplicate_same()
    } else {
        def
    }
}

/// The setters of `TwbDef` that return `Self` upstream.
pub trait DefSetters: Sized {
    /// Port of `IncludeFlag`.
    fn include_flag(self, flag: DefFlag) -> Self {
        self.include_flag_when(flag, true)
    }

    /// Port of `IncludeFlag` with `aOnlyWhenTrue`.
    fn include_flag_when(self, flag: DefFlag, only_when_true: bool) -> Self;
}

impl<T: DefKind + ?Sized> DefSetters for Arc<T> {
    fn include_flag_when(self, flag: DefFlag, only_when_true: bool) -> Self {
        let base = self.def_base();
        let this = if base.def_is_locked() && only_when_true && !base.def_flags.contains(flag) {
            self.duplicate_same()
        } else {
            self
        };
        if only_when_true {
            this.def_base().def_flags.include(flag);
        }
        this
    }
}

/// The fields of `TwbNamedDef`.
pub struct NamedDefBase {
    nd_name: String,
    /// The name with its position, set under `wdMakeUnknownElementsUnique`.
    nd_unique_name: OnceLock<String>,
    nd_summary_name: DefCell<String>,
    nd_singular_name: OnceLock<String>,
    pub nd_after_load: DefCell<AfterLoadCallback>,
    pub nd_after_set: DefCell<AfterSetCallback>,
    pub nd_to_str: DefCell<ToStrCallback>,
    pub nd_dont_show: DefCell<DontShowCallback>,
    pub nd_is_removable: DefCell<IsRemovableCallback>,
    pub nd_terminator: bool,
    nd_unused: AtomicBool,
    nd_tree_head: AtomicBool,
    nd_tree_branch: AtomicBool,
    pub nd_summary_links_to_callback: DefCell<LinksToCallback>,
}

/// The constructor arguments shared by `TwbNamedDef` and its descendants.
pub struct NamedDefArgs {
    pub priority: ConflictPriority,
    pub required: bool,
    pub name: String,
    pub after_load: Option<AfterLoadCallback>,
    pub after_set: Option<AfterSetCallback>,
    pub dont_show: Option<DontShowCallback>,
    pub get_cp: Option<GetConflictPriority>,
    pub terminator: bool,
}

impl NamedDefBase {
    /// Port of `TwbNamedDef.Create`, which continues into `TwbDef.Create`.
    pub fn create(args: NamedDefArgs) -> (DefBase, NamedDefBase) {
        let mut priority = args.priority;
        let unused = args.name == "Unused";
        if unused && priority == ConflictPriority::cpNormal {
            priority = ConflictPriority::cpIgnore;
        }
        let def = DefBase::create(priority, args.required, args.get_cp);
        if args.name.to_lowercase().contains("unknown") {
            def.is_unknown.store(true, Ordering::Relaxed);
        }
        let named = NamedDefBase {
            nd_name: args.name,
            nd_unique_name: OnceLock::new(),
            nd_summary_name: DefCell::default(),
            nd_singular_name: OnceLock::new(),
            nd_after_load: DefCell::new(args.after_load),
            nd_after_set: DefCell::new(args.after_set),
            nd_to_str: DefCell::default(),
            nd_dont_show: DefCell::new(args.dont_show),
            nd_is_removable: DefCell::default(),
            nd_terminator: args.terminator,
            nd_unused: AtomicBool::new(unused),
            nd_tree_head: AtomicBool::new(false),
            nd_tree_branch: AtomicBool::new(false),
            nd_summary_links_to_callback: DefCell::default(),
        };
        (def, named)
    }

    /// The constructor arguments that recreate `source`, for the `Clone` constructors.
    pub fn clone_args(source: &dyn NamedDef) -> NamedDefArgs {
        let def = source.def_base();
        let named = source.named_def_base();
        NamedDefArgs {
            priority: def.def_priority,
            required: def.def_required(),
            name: named.nd_name.clone(),
            after_load: named.nd_after_load.load().as_deref().cloned(),
            after_set: named.nd_after_set.load().as_deref().cloned(),
            dont_show: named.nd_dont_show.load().as_deref().cloned(),
            get_cp: def.def_get_cp.clone(),
            terminator: named.nd_terminator,
        }
    }

    /// Port of `TwbNamedDef.AfterClone`.
    pub fn after_clone(this: &dyn NamedDef, source: &dyn NamedDef) {
        this.def_base().after_clone(source.as_dyn_def());
        let (target, source) = (this.named_def_base(), source.named_def_base());
        target.nd_tree_head.store(source.nd_tree_head(), Ordering::Relaxed);
        target.nd_tree_branch.store(source.nd_tree_branch(), Ordering::Relaxed);
        target.nd_to_str.assign(&source.nd_to_str);
        target.nd_is_removable.assign(&source.nd_is_removable);
        target
            .nd_summary_links_to_callback
            .assign(&source.nd_summary_links_to_callback);
        target.nd_summary_name.assign(&source.nd_summary_name);
    }

    pub fn nd_name(&self) -> &str {
        self.nd_unique_name.get().unwrap_or(&self.nd_name)
    }

    pub fn nd_unused(&self) -> bool {
        self.nd_unused.load(Ordering::Relaxed)
    }

    pub fn nd_tree_head(&self) -> bool {
        self.nd_tree_head.load(Ordering::Relaxed)
    }

    pub fn nd_tree_branch(&self) -> bool {
        self.nd_tree_branch.load(Ordering::Relaxed)
    }
}

/// Port of `TwbNamedDef.MakeSingularName`.
pub fn make_singular_name(name: &str) -> String {
    if let Some(stem) = name.strip_suffix("ies") {
        format!("{stem}y")
    } else if let Some(stem) = name.strip_suffix('s') {
        stem.to_owned()
    } else {
        name.to_owned()
    }
}

/// Upstream `IwbNamedDef`, implemented by `TwbNamedDef`.
pub trait NamedDef: Def {
    fn named_def_base(&self) -> &NamedDefBase;

    fn get_name(&self) -> &str {
        self.named_def_base().nd_name()
    }

    fn get_summary_name(&self) -> String {
        match self.named_def_base().nd_summary_name.load().as_deref() {
            Some(name) if !name.is_empty() => name.clone(),
            _ => self.get_name().to_owned(),
        }
    }

    fn get_full_name(&self) -> String {
        self.get_name().to_owned()
    }

    fn get_singular_name(&self) -> &str {
        self.named_def_base()
            .nd_singular_name
            .get_or_init(|| make_singular_name(self.get_name()))
    }

    fn get_summary_singular_name(&self) -> String {
        make_singular_name(&self.get_summary_name())
    }

    fn get_path(&self) -> String {
        let mut result = self.get_name().to_owned();
        let mut parent = self.def_base().def_parent();
        while let Some(current) = parent {
            result = match current.as_named_def() {
                Some(named) => format!("{} \\ {result}", named.get_name()),
                None => format!("{} \\ {result}", current.get_def_type_name()),
            };
            parent = current.get_parent();
        }
        result
    }

    fn get_full_path(&self) -> String {
        let mut result = self.get_full_name();
        let mut child = self.def_ref();
        let mut parent = self.def_base().def_parent();
        while let Some(current) = parent {
            let pos = current.get_child_pos(&*child);
            let pos_str = if pos >= 0 { format!("[{pos}] ") } else { String::new() };
            result = match current.as_named_def() {
                Some(named) => format!("{} \\ {pos_str}{result}", named.get_full_name()),
                None => format!("{} \\ {pos_str}{result}", current.get_def_type_name()),
            };
            parent = current.get_parent();
            child = current;
        }
        result
    }

    fn after_load(&self, element: &ElementRef) {
        self.used(None, "");
        if let Some(after_load) = self.named_def_base().nd_after_load.load().as_deref() {
            after_load(element);
        }
    }

    fn after_set(&self, element: &ElementRef, old_value: &Variant, new_value: &Variant) {
        if let Some(after_set) = self.named_def_base().nd_after_set.load().as_deref() {
            after_set(element, old_value, new_value);
        }
    }

    /// Is the element expected to be a "header record" in the tree navigator.
    fn get_tree_head(&self) -> bool {
        self.named_def_base().nd_tree_head()
    }

    fn set_tree_head(&self, value: bool) {
        self.named_def_base().nd_tree_head.store(value, Ordering::Relaxed);
    }

    /// Is the element included in a "leaf" expected to be displayed in the view pane.
    fn get_tree_branch(&self) -> bool {
        self.named_def_base().nd_tree_branch()
    }

    fn set_tree_branch(&self, value: bool) {
        self.named_def_base().nd_tree_branch.store(value, Ordering::Relaxed);
    }

    /// Port of `TwbNamedDef.ToString`: lets the `ToStr` callback change `result`.
    fn call_to_str(&self, result: &mut String, element: ElementArg, callback_type: CallbackType) {
        if let Some(to_str) = self.named_def_base().nd_to_str.load().as_deref() {
            to_str(result, None, element, callback_type);
        }
    }

    fn get_summary_links_to(&self, element: ElementArg) -> Option<ElementRef> {
        match self.named_def_base().nd_summary_links_to_callback.load().as_deref() {
            Some(callback) => callback(element),
            None => None,
        }
    }

    fn is_removable(&self, element: ElementArg) -> bool {
        match self.named_def_base().nd_is_removable.load().as_deref() {
            Some(callback) => callback(element),
            None => true,
        }
    }
}

/// Port of `TwbNamedDef.GetDontShow`.
pub fn named_def_get_dont_show(def: &dyn NamedDef, element: ElementArg) -> bool {
    let base = def.named_def_base();
    match base.nd_dont_show.load().as_deref() {
        Some(dont_show) => dont_show(element),
        None => hide_unused() && base.nd_unused(),
    }
}

/// Port of `TwbNamedDef.InitFromParentBeforeChildren`.
pub fn named_def_init_from_parent_before_children(def: &dyn NamedDef) {
    let base = def.def_base();
    let named = def.named_def_base();
    let parent = base.def_parent();
    if !(base.is_unknown.load(Ordering::Relaxed) || named.nd_unused())
        && named.nd_name().is_empty()
        && let Some(parent_named) = parent.as_deref().and_then(Def::as_named_def)
    {
        if parent_named.get_name().to_lowercase().contains("unknown") {
            base.is_unknown.store(true, Ordering::Relaxed);
        }
        if parent_named.get_name() == "Unused" {
            named.nd_unused.store(true, Ordering::Relaxed);
        }
    }
    // Signature definitions keep their name. They arrive with their classes and
    // then override this method.
    if make_unknown_elements_unique()
        && base.is_unknown.load(Ordering::Relaxed)
        && !named.nd_name().contains('@')
        && let Some(parent) = &parent
    {
        let pos = parent.get_child_pos(def.as_dyn_def());
        if pos >= 0 {
            let _ = named.nd_unique_name.set(format!("{}@{pos}", named.nd_name()));
        }
    }
    def_init_from_parent_before_children(def.as_dyn_def());
}

/// The setters of `TwbNamedDef` that return `Self` upstream.
pub trait NamedDefSetters: Sized {
    fn set_after_load(self, after_load: Option<AfterLoadCallback>) -> Self;
    fn set_after_set(self, after_set: Option<AfterSetCallback>) -> Self;
    fn set_dont_show(self, dont_show: Option<DontShowCallback>) -> Self;
    fn set_is_removable(self, callback: Option<IsRemovableCallback>) -> Self;
    fn set_summary_name(self, name: &str) -> Self;
    fn set_summary_links_to_callback(self, callback: Option<LinksToCallback>) -> Self;
    fn set_to_str(self, to_str: Option<ToStrCallback>) -> Self;
}

impl<T: NamedDef + DefKind + ?Sized> NamedDefSetters for Arc<T> {
    fn set_after_load(self, after_load: Option<AfterLoadCallback>) -> Self {
        let this = unlocked(self);
        this.named_def_base().nd_after_load.set(after_load);
        this
    }

    fn set_after_set(self, after_set: Option<AfterSetCallback>) -> Self {
        let this = unlocked(self);
        this.named_def_base().nd_after_set.set(after_set);
        this
    }

    fn set_dont_show(self, dont_show: Option<DontShowCallback>) -> Self {
        let this = unlocked(self);
        this.named_def_base().nd_dont_show.set(dont_show);
        this
    }

    fn set_is_removable(self, callback: Option<IsRemovableCallback>) -> Self {
        let this = unlocked(self);
        this.named_def_base().nd_is_removable.set(callback);
        this
    }

    fn set_summary_name(self, name: &str) -> Self {
        let this = unlocked(self);
        this.named_def_base().nd_summary_name.set(Some(name.to_owned()));
        this
    }

    fn set_summary_links_to_callback(self, callback: Option<LinksToCallback>) -> Self {
        let this = unlocked(self);
        this.named_def_base().nd_summary_links_to_callback.set(callback);
        this
    }

    fn set_to_str(self, to_str: Option<ToStrCallback>) -> Self {
        let this = unlocked(self);
        this.named_def_base().nd_to_str.set(to_str);
        this
    }
}

super::types::pascal_enum! {
    ValueDefState {
        vdsIsVariableSize,
        vdsIsVariableSizeChecked,
        vdsHasDefaultEditValue,
        vdsHasDefaultNativeValue,
    }
}

/// The fields of `TwbValueDef`.
#[derive(Default)]
pub struct ValueDefBase {
    pub vd_states: AtomicEnumSet<ValueDefState>,
    pub vd_default_edit_value: DefCell<String>,
    pub vd_default_native_value: DefCell<Variant>,
    pub vd_links_to_callback: DefCell<LinksToCallback>,
    pub vd_edit_info: DefCell<Vec<String>>,
    pub vd_set_to_default: DefCell<SetToDefaultCallback>,
}

impl ValueDefBase {
    /// Port of `TwbValueDef.AfterClone`.
    pub fn after_clone(this: &dyn ValueDef, source: &dyn ValueDef) {
        NamedDefBase::after_clone(this.as_dyn_named_def(), source.as_dyn_named_def());
        let (target, from) = (this.value_def_base(), source.value_def_base());
        let kept = EnumSet::of(&[
            ValueDefState::vdsHasDefaultEditValue,
            ValueDefState::vdsHasDefaultNativeValue,
        ]);
        target
            .vd_states
            .set(target.vd_states.get() | (from.vd_states.get() & kept));
        target.vd_default_edit_value.assign(&from.vd_default_edit_value);
        target.vd_default_native_value.assign(&from.vd_default_native_value);
        target.vd_links_to_callback.assign(&from.vd_links_to_callback);
        this.named_def_base()
            .nd_to_str
            .assign(&source.named_def_base().nd_to_str);
        target.vd_edit_info.assign(&from.vd_edit_info);
        target.vd_set_to_default.assign(&from.vd_set_to_default);
    }
}

/// Upstream `IwbValueDef`, implemented by `TwbValueDef`.
///
/// The methods that change data (`FromEditValue`, `FromNativeValue`,
/// `SetLinksTo`, `MastersUpdated`, `FindUsedMasters`, `CompareExchangeFormID`,
/// `PrepareSave`) come with the write path.
pub trait ValueDef: NamedDef {
    fn value_def_base(&self) -> &ValueDefBase;

    /// `self` as a trait object of the parent interface.
    fn as_dyn_named_def(&self) -> &dyn NamedDef;

    /// Port of `TwbValueDef.ToString`: the display value of the data.
    fn to_string(&self, data: DataPtr, element: ElementArg) -> String;

    fn to_summary(&self, _depth: i32, data: DataPtr, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        value_def_to_summary(self, data, element, links_to)
    }

    fn to_sort_key(&self, data: DataPtr, element: ElementArg, _extended: bool) -> String {
        // Delphi UpperCase changes only the letters a to z.
        let mut result = self.to_string(data, element).to_ascii_uppercase();
        if self.def_base().def_flags.contains(DefFlag::dfZeroSortKey) && !result.is_empty() {
            result = "0".repeat(super::misc::length(&result));
        }
        result
    }

    fn check(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = String::new();
        if let Some(to_str) = self.named_def_base().nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctCheck);
        }
        result
    }

    fn get_size(&self, data: DataPtr, element: ElementArg) -> i32;

    fn get_default_size(&self, data: DataPtr, element: ElementArg) -> i32;

    fn get_links_to(&self, _data: DataPtr, element: ElementArg) -> Option<ElementRef> {
        match self.value_def_base().vd_links_to_callback.load().as_deref() {
            Some(callback) => callback(element),
            None => None,
        }
    }

    fn build_ref(&self, _data: DataPtr, _element: ElementArg) {}

    /// Port of `GetIsVariableSize`, which caches `GetIsVariableSizeInternal`.
    fn get_is_variable_size(&self) -> bool {
        let states = &self.value_def_base().vd_states;
        if !states.contains(ValueDefState::vdsIsVariableSizeChecked) {
            if self.get_is_variable_size_internal() {
                states.include(ValueDefState::vdsIsVariableSize);
            } else {
                states.exclude(ValueDefState::vdsIsVariableSize);
            }
            states.include(ValueDefState::vdsIsVariableSizeChecked);
        }
        states.contains(ValueDefState::vdsIsVariableSize)
    }

    fn get_is_variable_size_internal(&self) -> bool {
        false
    }

    fn get_can_be_zero_size(&self) -> bool {
        false
    }

    fn to_edit_value(&self, _data: DataPtr, _element: ElementArg) -> String {
        String::new()
    }

    fn to_native_value(&self, _data: DataPtr, _element: ElementArg) -> Variant {
        Variant::Empty
    }

    fn get_is_editable(&self, _data: DataPtr, _element: ElementArg) -> bool {
        // The test of defInternalEditOnly upstream cannot change this result.
        is_internal_edit()
    }

    fn get_edit_type(&self, _data: DataPtr, _element: ElementArg) -> EditType {
        EditType::etDefault
    }

    fn get_edit_info(&self, _data: DataPtr, _element: ElementArg) -> Vec<String> {
        self.value_def_base()
            .vd_edit_info
            .load()
            .as_deref()
            .cloned()
            .unwrap_or_default()
    }

    fn set_to_default(&self, data: DataPtr, element: ElementArg) -> bool {
        match self.value_def_base().vd_set_to_default.load().as_deref() {
            Some(callback) => callback(data, element),
            None => false,
        }
    }

    fn get_element_map(&self) -> Vec<u32> {
        Vec::new()
    }
}

/// Port of `TwbValueDef.ToSummary`.
pub fn value_def_to_summary<T: ValueDef + ?Sized>(
    def: &T,
    data: DataPtr,
    element: ElementArg,
    links_to: &mut Option<ElementRef>,
) -> String {
    let mut result = String::new();
    if let Some(to_str) = def.named_def_base().nd_to_str.load().as_deref() {
        to_str(&mut result, data, element, CallbackType::ctToSummary);
    }
    if result.is_empty() {
        result = shorten_text(&def.to_string(data, element));
    }
    if links_to.is_none()
        && !result.is_empty()
        && let Some(element) = element
    {
        *links_to = element.get_links_to();
    }
    result
}

impl DefKind for dyn ValueDef {
    fn duplicate_same(&self) -> Arc<Self> {
        self.duplicate()
            .into_value_def()
            .expect("the duplicate of a value definition is a value definition")
    }
}

/// The setters of `TwbValueDef` that return `Self` upstream.
pub trait ValueDefSetters: Sized {
    fn set_default_edit_value(self, value: &str) -> Self;
    fn set_default_native_value(self, value: Variant) -> Self;
    fn set_links_to_callback(self, callback: Option<LinksToCallback>) -> Self;
    fn set_set_to_default(self, callback: Option<SetToDefaultCallback>) -> Self;
}

impl<T: ValueDef + DefKind + ?Sized> ValueDefSetters for Arc<T> {
    fn set_default_edit_value(self, value: &str) -> Self {
        let this = unlocked(self);
        let base = this.value_def_base();
        base.vd_default_edit_value.set(Some(value.to_owned()));
        base.vd_states.include(ValueDefState::vdsHasDefaultEditValue);
        this
    }

    fn set_default_native_value(self, value: Variant) -> Self {
        let this = unlocked(self);
        let base = this.value_def_base();
        base.vd_default_native_value.set(Some(value));
        base.vd_states.include(ValueDefState::vdsHasDefaultNativeValue);
        this
    }

    fn set_links_to_callback(self, callback: Option<LinksToCallback>) -> Self {
        let this = unlocked(self);
        this.value_def_base().vd_links_to_callback.set(callback);
        this
    }

    fn set_set_to_default(self, callback: Option<SetToDefaultCallback>) -> Self {
        let this = unlocked(self);
        this.value_def_base().vd_set_to_default.set(callback);
        this
    }
}

/// Implements the methods of [`Def`], [`NamedDef`] and [`ValueDef`] that every
/// value definition class implements in the same way. The class has the
/// fields `def`, `nd` and `vd` and a `clone_from` constructor.
macro_rules! value_def_plumbing {
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

        fn as_value_def(&self) -> Option<&dyn ValueDef> {
            Some(self)
        }

        fn into_value_def(self: Arc<Self>) -> Option<Arc<dyn ValueDef>> {
            Some(self)
        }
    };
    (NamedDef) => {
        fn named_def_base(&self) -> &NamedDefBase {
            &self.nd
        }
    };
    (ValueDef) => {
        fn value_def_base(&self) -> &ValueDefBase {
            &self.vd
        }

        fn as_dyn_named_def(&self) -> &dyn NamedDef {
            self
        }
    };
}
pub(crate) use value_def_plumbing;

/// Upstream `IwbEmptyDef`.
pub trait EmptyDefInterface: ValueDef {
    fn get_sorted(&self) -> bool;
}

/// Upstream `TwbEmptyDef`: a place holder for optional elements.
pub struct EmptyDef {
    self_ref: Weak<EmptyDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
    ed_sorted: bool,
}

impl EmptyDef {
    /// Port of `TwbEmptyDef.Create`.
    pub fn create(args: NamedDefArgs, sorted: bool) -> Arc<Self> {
        let (def, nd) = NamedDefBase::create(NamedDefArgs {
            terminator: false,
            ..args
        });
        def.def_flags.include(DefFlag::dfSkipImplicitEdit);
        let sorted = sorted && !super::globals::never_sorted();
        let this = Arc::new_cyclic(|self_ref| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            vd: ValueDefBase::default(),
            ed_sorted: sorted,
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbEmptyDef.Clone`.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(NamedDefBase::clone_args(source), source.ed_sorted);
        ValueDefBase::after_clone(&*this, source);
        this
    }
}

impl Def for EmptyDef {
    value_def_plumbing!(Def);

    fn get_def_type(&self) -> DefType {
        DefType::dtEmpty
    }

    fn get_def_type_name(&self) -> String {
        "Place holder for optional elements".to_owned()
    }

    fn as_empty_def(&self) -> Option<&dyn EmptyDefInterface> {
        Some(self)
    }
}

impl NamedDef for EmptyDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for EmptyDef {
    value_def_plumbing!(ValueDef);

    fn to_string(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = String::new();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToStr);
        }
        self.used(element, &result);
        result
    }

    fn to_sort_key(&self, data: DataPtr, element: ElementArg, _extended: bool) -> String {
        let mut result = "<Empty>".to_owned();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToSortKey);
        }
        result
    }

    fn get_size(&self, _data: DataPtr, _element: ElementArg) -> i32 {
        0
    }

    fn get_default_size(&self, _data: DataPtr, _element: ElementArg) -> i32 {
        0
    }

    fn get_can_be_zero_size(&self) -> bool {
        true
    }

    fn get_is_editable(&self, _data: DataPtr, _element: ElementArg) -> bool {
        !(self.def.def_internal_edit_only() && !is_internal_edit())
    }
}

impl EmptyDefInterface for EmptyDef {
    fn get_sorted(&self) -> bool {
        self.ed_sorted
    }
}

impl DefKind for EmptyDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::globals::{set_hide_unused, test_lock};
    use super::*;

    fn args(name: &str, priority: ConflictPriority) -> NamedDefArgs {
        NamedDefArgs {
            priority,
            required: false,
            name: name.to_owned(),
            after_load: None,
            after_set: None,
            dont_show: None,
            get_cp: None,
            terminator: false,
        }
    }

    #[test]
    fn create_applies_name_and_priority_rules() {
        let _guard = test_lock();
        let unused = EmptyDef::create(args("Unused", ConflictPriority::cpNormal), false);
        assert_eq!(unused.def_base().def_priority, ConflictPriority::cpIgnore);
        assert!(unused.get_dont_show(None) && unused.get_has_dont_show());
        set_hide_unused(false);
        assert!(!unused.get_dont_show(None));

        let unknown = EmptyDef::create(args("Unknown 3", ConflictPriority::cpTranslate), false);
        assert!(unknown.def_base().is_unknown.load(Ordering::Relaxed));
        assert_eq!(unknown.def_base().def_priority, ConflictPriority::cpNormal);
        assert!(unknown.get_def_flags().contains(DefFlag::dfTranslatable));
        assert!(unknown.get_def_flags().contains(DefFlag::dfSkipImplicitEdit));
        assert_eq!(unknown.get_def_type(), DefType::dtEmpty);
    }

    #[test]
    fn names() {
        let _guard = test_lock();
        let def = EmptyDef::create(args("Properties", ConflictPriority::cpNormal), false);
        assert_eq!(def.get_name(), "Properties");
        assert_eq!(def.get_singular_name(), "Property");
        assert_eq!(make_singular_name("Entries"), "Entry");
        assert_eq!(def.get_summary_name(), "Properties");
        let def = def.set_summary_name("Props");
        assert_eq!(def.get_summary_name(), "Props");
        assert_eq!(def.get_summary_singular_name(), "Prop");
        assert_eq!(def.get_path(), "Properties");
    }

    #[test]
    fn to_str_callback_drives_values() {
        let _guard = test_lock();
        let callback: ToStrCallback = Arc::new(|value, _, _, callback_type| match callback_type {
            CallbackType::ctToStr => value.push_str("shown"),
            CallbackType::ctToSortKey => value.push('!'),
            CallbackType::ctCheck => value.push_str("bad"),
            _ => {}
        });
        let def = EmptyDef::create(args("Marker", ConflictPriority::cpNormal), true).set_to_str(Some(callback));
        assert_eq!(def.to_string(None, None), "shown");
        assert_eq!(def.to_sort_key(None, None, false), "<Empty>!");
        assert_eq!(def.check(None, None), "bad");
        assert_eq!(def.to_summary(0, None, None, &mut None), "shown");
        assert!(def.get_sorted() && def.get_can_be_zero_size() && !def.get_is_variable_size());
    }

    #[test]
    fn locked_definition_duplicates_on_change() {
        let _guard = test_lock();
        let parent = EmptyDef::create(args("Parent", ConflictPriority::cpNormal), false);
        let parent_ref: DefRef = parent.clone();
        let weak = Arc::downgrade(&parent_ref);
        let child = EmptyDef::create(args("Child", ConflictPriority::cpNormal), false);

        let placed = set_parent(child.clone(), &weak, false);
        assert!(Arc::ptr_eq(&placed, &child));
        assert!(child.def_base().def_is_locked());
        assert_eq!(child.get_path(), "Parent \\ Child");
        assert!(Arc::ptr_eq(&set_parent(child.clone(), &weak, false), &child));

        // A second parent gets a duplicate, and so does a setter.
        let other = EmptyDef::create(args("Other", ConflictPriority::cpNormal), false);
        let other_ref: DefRef = other.clone();
        let placed = set_parent(child.clone(), &Arc::downgrade(&other_ref), false);
        assert!(!Arc::ptr_eq(&placed, &child));
        assert_eq!(placed.get_path(), "Other \\ Child");
        let child_ref: DefRef = child.clone();
        let placed_ref: DefRef = placed.clone();
        assert!(Arc::ptr_eq(&get_root(&placed_ref), &child_ref));

        let flagged = child.clone().include_flag(DefFlag::dfNoReport);
        assert!(!Arc::ptr_eq(&flagged, &child));
        assert!(flagged.get_def_flags().contains(DefFlag::dfNoReport));
        assert!(!child.get_def_flags().contains(DefFlag::dfNoReport));
        assert!(flagged.get_parent().is_none());

        // The flag is already set, so no duplicate.
        let again = flagged.clone().include_flag(DefFlag::dfNoReport);
        assert!(Arc::ptr_eq(&again, &flagged));

        let dynamic: Arc<dyn ValueDef> = child.clone();
        let renamed = dynamic.clone().set_summary_name("Kid");
        assert!(!Arc::ptr_eq(&renamed, &dynamic));
        assert_eq!(renamed.get_summary_name(), "Kid");
        assert_eq!(child.get_summary_name(), "Child");
    }

    #[test]
    fn flags_inherit_between_parent_and_child() {
        let _guard = test_lock();
        let parent =
            EmptyDef::create(args("Parent", ConflictPriority::cpNormal), false).include_flag(DefFlag::dfNoReport);
        let parent_ref: DefRef = parent.clone();
        let child =
            EmptyDef::create(args("", ConflictPriority::cpNormal), false).include_flag(DefFlag::dfUnmappedFormID);
        let child = set_parent(child, &Arc::downgrade(&parent_ref), false);
        child.init_from_parent();
        assert!(child.get_def_flags().contains(DefFlag::dfNoReport));
        assert!(parent.get_def_flags().contains(DefFlag::dfCanContainUnmappedFormID));
        assert!(!parent.get_def_flags().contains(DefFlag::dfUnmappedFormID));
    }

    #[test]
    fn collapsed_generation_counts_changes() {
        let _guard = test_lock();
        let def = EmptyDef::create(args("Marker", ConflictPriority::cpNormal), false);
        def.set_collapsed(true);
        def.set_collapsed(true);
        def.set_collapsed(false);
        assert_eq!(def.get_collapsed_gen(), 2);
        assert!(!def.get_collapsed());
    }
}
