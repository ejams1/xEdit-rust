// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbResolvableDef` with its descendants `TwbUnionDef` and `TwbRecursiveDef`.

use std::sync::{Arc, OnceLock, Weak};

use super::def::{
    Def, DefBase, DefKind, DefRef, NamedDef, NamedDefArgs, NamedDefBase, ValueDef, ValueDefBase,
    def_init_from_parent_after_children, named_def_init_from_parent_before_children, set_parent, value_def_plumbing,
};
use super::element::{DataPtr, ElementArg, ElementRef};
use super::globals::is_internal_edit;
use super::misc::{EditError, Variant};
use super::types::{
    CallbackType, DefFlag, DefType, DefTypes, EditType, ElementType, EnumSet, def_flags_inherit_down,
    def_flags_inherit_up,
};

/// Decides which member of a union the data holds: the index of the member.
pub type UnionDecider = Arc<dyn Fn(DataPtr, ElementArg) -> i32 + Send + Sync>;

/// Upstream `IwbResolvableDef`: a definition that stands for another one,
/// which it finds from the data or from its position.
pub trait ResolvableDef: ValueDef {
    /// The definition that this one stands for, if it can be found.
    fn resolve_def(&self, data: DataPtr, element: ElementArg) -> Option<&Arc<dyn ValueDef>>;

    fn needs_element_to_resolve(&self) -> bool {
        false
    }

    /// Like `resolve_def`. Also replaces `element` by the element that the
    /// resolved definition describes, when that is a child of `element`.
    fn resolve_def_and_element(&self, data: DataPtr, element: &mut Option<ElementRef>) -> Option<&Arc<dyn ValueDef>> {
        self.resolve_def(data, element.as_ref())
    }
}

/// Port of `TwbResolvableDef.InitFromResolvedDef`.
fn init_from_resolved_def<T: ResolvableDef + ?Sized>(def: &T) {
    if let Some(value_def) = def.resolve_def(None, None) {
        let own = &def.def_base().def_flags;
        let resolved = &value_def.def_base().def_flags;
        own.set(own.get() | (resolved.get() & def_flags_inherit_down()));
        resolved.set(resolved.get() | (own.get() & def_flags_inherit_up()));
    }
}

/// The methods of [`Def`] that `TwbResolvableDef` overrides.
macro_rules! resolvable_def_methods {
    () => {
        fn as_resolvable_def(&self) -> Option<&dyn ResolvableDef> {
            Some(self)
        }

        fn init_from_parent_before_children(&self) {
            init_from_resolved_def(self);
            named_def_init_from_parent_before_children(self);
            init_from_resolved_def(self);
        }

        fn init_from_parent_after_children(&self) {
            init_from_resolved_def(self);
            def_init_from_parent_after_children(self);
            init_from_resolved_def(self);
        }
    };
}

/// The methods of [`ValueDef`] that `TwbResolvableDef` overrides and that its
/// descendants keep.
macro_rules! resolvable_value_def_methods {
    () => {
        fn to_string(&self, data: DataPtr, element: ElementArg) -> String {
            let mut result = match self.resolve_def(data, element) {
                Some(value_def) => value_def.to_string(data, element),
                None => String::new(),
            };
            if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
                to_str(&mut result, data, element, CallbackType::ctToStr);
            }
            self.used(element, &result);
            result
        }

        fn to_summary(
            &self,
            depth: i32,
            data: DataPtr,
            element: ElementArg,
            links_to: &mut Option<ElementRef>,
        ) -> String {
            let mut result = String::new();
            if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
                to_str(&mut result, data, element, CallbackType::ctToSummary);
            }
            if result.is_empty() {
                let mut resolved_element = element.cloned();
                if let Some(value_def) = self.resolve_def_and_element(data, &mut resolved_element) {
                    result = value_def.to_summary(depth, data, resolved_element.as_ref(), links_to);
                }
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

        fn to_sort_key(&self, data: DataPtr, element: ElementArg, extended: bool) -> String {
            let mut result = match self.resolve_def(data, element) {
                Some(value_def) => value_def.to_sort_key(data, element, extended),
                None => String::new(),
            };
            if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
                to_str(&mut result, data, element, CallbackType::ctToSortKey);
            }
            result
        }

        fn check(&self, data: DataPtr, element: ElementArg) -> String {
            let mut result = match self.resolve_def(data, element) {
                Some(value_def) => value_def.check(data, element),
                None => "Union could not be resolved".to_owned(),
            };
            if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
                to_str(&mut result, data, element, CallbackType::ctCheck);
            }
            result
        }

        fn get_default_size(&self, data: DataPtr, element: ElementArg) -> i32 {
            match self.resolve_def(data, element) {
                Some(value_def) => value_def.get_default_size(data, element),
                None => 0,
            }
        }

        fn get_links_to(&self, data: DataPtr, element: ElementArg) -> Option<ElementRef> {
            if let Some(callback) = self.vd.vd_links_to_callback.load().as_deref() {
                return callback(element);
            }
            self.resolve_def(data, element)?.get_links_to(data, element)
        }

        fn build_ref(&self, data: DataPtr, element: ElementArg) {
            if self.def.def_flags.contains(DefFlag::dfExcludeFromBuildRef) {
                return;
            }
            if let Some(value_def) = self.resolve_def(data, element) {
                value_def.build_ref(data, element);
            }
        }

        fn to_edit_value(&self, data: DataPtr, element: ElementArg) -> String {
            let mut result = match self.resolve_def(data, element) {
                Some(value_def) => value_def.to_edit_value(data, element),
                None => String::new(),
            };
            if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
                to_str(&mut result, data, element, CallbackType::ctToEditValue);
            }
            result
        }

        fn to_native_value(&self, data: DataPtr, element: ElementArg) -> Variant {
            match self.resolve_def(data, element) {
                Some(value_def) => value_def.to_native_value(data, element),
                None => Variant::Str(String::new()),
            }
        }

        fn from_edit_value(&self, data: DataPtr, element: ElementArg, value: &str) -> Result<(), EditError> {
            match self.resolve_def(data, element) {
                Some(value_def) => value_def.from_edit_value(data, element, value),
                None => Err("Union could not be resolved".to_owned()),
            }
        }

        fn from_native_value(&self, data: DataPtr, element: ElementArg, value: Variant) -> Result<(), EditError> {
            match self.resolve_def(data, element) {
                Some(value_def) => value_def.from_native_value(data, element, value),
                None => Err("Union could not be resolved".to_owned()),
            }
        }

        fn set_to_default(&self, data: DataPtr, element: ElementArg) -> Result<bool, EditError> {
            if self.set_to_default_callback(data, element) {
                return Ok(true);
            }
            match self.resolve_def(data, element) {
                Some(value_def) => value_def.set_to_default(data, element),
                None => Ok(false),
            }
        }

        fn get_is_editable(&self, data: DataPtr, element: ElementArg) -> bool {
            let result = is_internal_edit()
                || match self.resolve_def(data, element) {
                    Some(value_def) => value_def.get_is_editable(data, element),
                    None => false,
                };
            result && !(self.def.def_internal_edit_only() && !is_internal_edit())
        }

        fn get_edit_type(&self, data: DataPtr, element: ElementArg) -> EditType {
            match self.resolve_def(data, element) {
                Some(value_def) => value_def.get_edit_type(data, element),
                None => EditType::etDefault,
            }
        }

        fn get_edit_info(&self, data: DataPtr, element: ElementArg) -> Vec<String> {
            if let Some(edit_info) = self.vd.vd_edit_info.load().as_deref() {
                return edit_info.clone();
            }
            match self.resolve_def(data, element) {
                Some(value_def) => value_def.get_edit_info(data, element),
                None => Vec::new(),
            }
        }
    };
}

/// Upstream `TwbUnionDef`: one of several definitions, chosen by a decider.
pub struct UnionDef {
    self_ref: Weak<UnionDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
    ud_decider: UnionDecider,
    ud_members: Vec<Arc<dyn ValueDef>>,
    ud_member_types: OnceLock<DefTypes>,
}

impl UnionDef {
    /// Port of `TwbUnionDef.Create`. The `after_load` and `terminator`
    /// arguments are not used, as upstream.
    pub fn create(args: NamedDefArgs, decider: UnionDecider, members: Vec<Arc<dyn ValueDef>>) -> Arc<Self> {
        let (def, nd) = NamedDefBase::create(NamedDefArgs {
            after_load: None,
            terminator: false,
            ..args
        });
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| {
            let parent: Weak<dyn Def> = self_ref.clone();
            Self {
                self_ref: self_ref.clone(),
                def,
                nd,
                vd: ValueDefBase::default(),
                ud_decider: decider,
                ud_members: members
                    .into_iter()
                    .map(|member| set_parent(member, &parent, false))
                    .collect(),
                ud_member_types: OnceLock::new(),
            }
        });
        DefBase::after_construction(&*this);
        this
    }

    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(
            NamedDefBase::clone_args(source),
            source.ud_decider.clone(),
            source.ud_members.clone(),
        );
        ValueDefBase::after_clone(&*this, source);
        this
    }

    /// Upstream `GetMember`. Fails when `index` is out of range.
    pub fn get_member(&self, index: usize) -> &Arc<dyn ValueDef> {
        &self.ud_members[index]
    }

    pub fn get_member_count(&self) -> i32 {
        self.ud_members.len() as i32
    }

    /// The definition types of the members.
    pub fn get_member_types(&self) -> DefTypes {
        *self.ud_member_types.get_or_init(|| {
            let mut types = EnumSet::empty();
            for member in &self.ud_members {
                types.include(member.get_def_type());
            }
            types
        })
    }
}

impl Def for UnionDef {
    value_def_plumbing!(Def);
    resolvable_def_methods!();

    /// Port of `TwbUnionDef.CanAssign`: a member of either union that the
    /// other can take.
    fn can_assign(&self, element: ElementArg, index: i32, def: Option<&dyn Def>) -> bool {
        if super::def::def_dont_assign(self) {
            return false;
        }
        match def.and_then(|def| def.as_union_def()) {
            Some(other) => {
                self.equals(def)
                    || (0..usize::try_from(other.get_member_count()).unwrap_or(0))
                        .any(|i| self.can_assign(element, index, Some(other.get_member(i).as_dyn_def())))
            }
            None => (0..usize::try_from(self.get_member_count()).unwrap_or(0))
                .any(|i| self.get_member(i).can_assign(element, index, def)),
        }
    }

    fn get_def_type(&self) -> DefType {
        DefType::dtUnion
    }

    fn get_def_type_name(&self) -> String {
        "Union".to_owned()
    }

    fn as_union_def(&self) -> Option<&UnionDef> {
        Some(self)
    }

    fn get_child_pos(&self, child: &dyn Def) -> i32 {
        self.ud_members
            .iter()
            .position(|member| child.equals(Some(member.as_dyn_def())))
            .map_or(-1, |index| index as i32)
    }

    fn init_from_parent_do_children(&self) {
        for member in &self.ud_members {
            member.init_from_parent();
        }
    }
}

impl NamedDef for UnionDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for UnionDef {
    value_def_plumbing!(ValueDef);
    resolvable_value_def_methods!();

    fn get_is_variable_size_internal(&self) -> bool {
        if self.ud_members.iter().any(|member| member.get_is_variable_size()) {
            return true;
        }
        // Members of different sizes make the union variable. Upstream fails
        // for a union without members.
        let mut sizes = self.ud_members.iter().map(|member| member.get_default_size(None, None));
        match sizes.next() {
            Some(first) => sizes.any(|size| size != first),
            None => false,
        }
    }

    fn get_size(&self, data: DataPtr, element: ElementArg) -> i32 {
        let variable = self.get_is_variable_size();
        let member = if variable {
            self.resolve_def(data, element)
        } else {
            None
        };
        let Some(member) = member else {
            let Some(first) = self.ud_members.first() else {
                return i32::MIN;
            };
            let mut result = first.get_size(data, element);
            if result > 0 && variable {
                for other in &self.ud_members[1..] {
                    if result == i32::MAX {
                        break;
                    }
                    let size = other.get_size(data, element);
                    if size == 0 {
                        // No valid value can be found.
                        return 0;
                    }
                    result = result.max(size);
                }
            }
            return result;
        };
        // The element of the union knows the definition that nested unions resolved to.
        let mut member: Option<Arc<dyn ValueDef>> = Some(member.clone());
        let mut child: Option<ElementRef> = None;
        if variable
            && let Some(union_element) = element
            && let Some(container) = union_element.as_container()
            && union_element
                .get_value_def()
                .is_some_and(|value_def| self.equals(Some(value_def.as_dyn_def())))
            && container.get_element_count() == 1
        {
            child = container.get_element(0);
            let matches = |member: &Option<Arc<dyn ValueDef>>, child: &Option<ElementRef>| {
                let child_def = child.as_ref().and_then(|child| child.get_value_def());
                match (member, &child_def) {
                    (Some(member), Some(child_def)) => member.equals(Some(child_def.as_dyn_def())),
                    _ => false,
                }
            };
            while !matches(&member, &child) {
                let Some(resolvable) = member.as_deref().and_then(|member| member.as_resolvable_def()) else {
                    break;
                };
                let next = resolvable.resolve_def_and_element(data, &mut child).cloned();
                member = next;
            }
            if !matches(&member, &child) {
                child = None;
            }
        }
        // Upstream fails when a nested union does not resolve.
        let Some(member) = member else {
            return 0;
        };
        let child = child.as_ref().or(element);
        let result = member.get_size(data, child);
        if result == i32::MAX {
            return result;
        }
        match data {
            Some(bytes) if (bytes.len() as i64) < i64::from(result) => bytes.len() as i32,
            _ => result,
        }
    }
}

impl ResolvableDef for UnionDef {
    fn resolve_def(&self, data: DataPtr, element: ElementArg) -> Option<&Arc<dyn ValueDef>> {
        let member_index = (self.ud_decider)(data, element);
        let result = usize::try_from(member_index)
            .ok()
            .and_then(|index| self.ud_members.get(index));
        self.used(None, "");
        result
    }

    fn needs_element_to_resolve(&self) -> bool {
        true
    }

    fn resolve_def_and_element(&self, data: DataPtr, element: &mut Option<ElementRef>) -> Option<&Arc<dyn ValueDef>> {
        let result = self.resolve_def(data, element.as_ref())?;
        // Upstream fails without an element.
        if let Some(current) = element.clone()
            && current.get_element_type() == ElementType::etUnion
            && let Some(container) = current.as_container()
            && container.get_element_count() == 1
            && let Some(child) = container.get_element(0)
            && child
                .get_value_def()
                .is_some_and(|value_def| result.equals(Some(value_def.as_dyn_def())))
        {
            *element = Some(child);
        }
        Some(result)
    }
}

impl DefKind for UnionDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

/// Upstream `TwbRecursiveDef`: stands for one of its ancestors, which makes a
/// definition that contains itself.
pub struct RecursiveDef {
    self_ref: Weak<RecursiveDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
    rd_levels_up: i32,
    rd_cached: OnceLock<Arc<dyn ValueDef>>,
}

impl RecursiveDef {
    /// Port of `TwbRecursiveDef.Create`. The `after_load` and `terminator`
    /// arguments are not used, as upstream.
    pub fn create(args: NamedDefArgs, levels_up: i32) -> Arc<Self> {
        let (def, nd) = NamedDefBase::create(NamedDefArgs {
            after_load: None,
            terminator: false,
            ..args
        });
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            vd: ValueDefBase::default(),
            rd_levels_up: levels_up,
            rd_cached: OnceLock::new(),
        });
        DefBase::after_construction(&*this);
        this
    }

    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(NamedDefBase::clone_args(source), source.rd_levels_up);
        ValueDefBase::after_clone(&*this, source);
        this
    }
}

impl Def for RecursiveDef {
    value_def_plumbing!(Def);
    resolvable_def_methods!();

    /// Port of `TwbResolvableDef.CanAssign`: the definitions both resolve
    /// to decide.
    fn can_assign(&self, element: ElementArg, index: i32, def: Option<&dyn Def>) -> bool {
        if super::def::def_dont_assign(self) {
            return false;
        }
        let Some(own) = self.resolve_def(None, element) else {
            return false;
        };
        let recursive = def
            .filter(|def| def.as_resolvable_def().is_some() && def.get_def_type() == DefType::dtResolvable)
            .and_then(|def| def.as_resolvable_def())
            .and_then(|resolvable| resolvable.resolve_def(None, element).cloned());
        match recursive {
            Some(source) => own.can_assign(element, index, Some(source.as_dyn_def())),
            None => own.can_assign(element, index, def),
        }
    }

    fn get_def_type(&self) -> DefType {
        DefType::dtResolvable
    }

    fn get_def_type_name(&self) -> String {
        "Resolvable".to_owned()
    }
}

impl NamedDef for RecursiveDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for RecursiveDef {
    value_def_plumbing!(ValueDef);
    resolvable_value_def_methods!();

    fn get_is_variable_size_internal(&self) -> bool {
        match self.resolve_def(None, None) {
            Some(value_def) => value_def.get_is_variable_size(),
            // We don't know, better assume yes.
            None => true,
        }
    }

    fn get_size(&self, data: DataPtr, element: ElementArg) -> i32 {
        match self.resolve_def(data, element) {
            Some(value_def) => value_def.get_size(data, element),
            None => 0,
        }
    }
}

impl ResolvableDef for RecursiveDef {
    fn resolve_def(&self, _data: DataPtr, _element: ElementArg) -> Option<&Arc<dyn ValueDef>> {
        if let Some(cached) = self.rd_cached.get() {
            return Some(cached);
        }
        // UPSTREAM-QUIRK: with no levels to go up the result is nil and is not cached.
        let mut result: Option<Arc<dyn ValueDef>> = None;
        for level in 1..=self.rd_levels_up {
            let parent = if level == 1 {
                self.def.def_parent()
            } else {
                result.as_ref().and_then(|current| current.get_parent())
            };
            result = Some(parent?.into_value_def()?);
        }
        let result = result?;
        Some(self.rd_cached.get_or_init(|| result))
    }
}

impl DefKind for RecursiveDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::array::{ArrayDef, ArrayDefArgs};
    use super::super::globals::test_lock;
    use super::super::integer::IntegerDef;
    use super::super::string::{StringClass, StringDef};
    use super::super::struct_def::{StructDef, StructDefArgs};
    use super::super::types::{ConflictPriority, IntType};
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

    fn int(name: &str, int_type: IntType) -> Arc<dyn ValueDef> {
        IntegerDef::create(args(name), int_type, None, 0)
    }

    /// Decides by the first byte of the data. Without data nothing resolves.
    fn by_first_byte() -> UnionDecider {
        Arc::new(|data, _| data.and_then(|data| data.first()).map_or(-1, |&byte| i32::from(byte)))
    }

    #[test]
    fn union_delegates_to_the_decided_member() {
        let _guard = test_lock();
        let union = UnionDef::create(
            args("Value"),
            by_first_byte(),
            vec![
                int("Byte", IntType::itU8),
                int("Word", IntType::itU16),
                StringDef::create(StringClass::String, args("Text"), 0, false),
            ],
        );
        assert_eq!(union.to_string(Some(&[0]), None), "0");
        assert_eq!(union.to_string(Some(&[1, 2]), None), "513");
        assert_eq!(union.to_string(Some(&[2, b'a', 0]), None), "\u{2}a");
        assert_eq!(union.to_string(Some(&[9]), None), "");
        assert_eq!(union.check(Some(&[9]), None), "Union could not be resolved");
        assert_eq!(union.to_sort_key(Some(&[1, 2]), None, false), "00201");
        assert_eq!(union.to_native_value(Some(&[1, 2]), None), Variant::UInt(513));
        assert_eq!(union.to_native_value(None, None), Variant::Str(String::new()));
        assert_eq!(union.to_summary(0, Some(&[1, 2]), None, &mut None), "513");
        assert!(union.get_is_variable_size());
        assert_eq!(union.get_size(Some(&[1, 2, 3]), None), 2);
        assert_eq!(union.get_size(Some(&[1]), None), 1);
        // Nothing resolves: the largest member, or 0 when a member has no size.
        assert_eq!(union.get_size(Some(&[9, 0]), None), 2);
        assert_eq!(union.get_default_size(Some(&[1, 2]), None), 2);
        assert_eq!(union.get_default_size(None, None), 0);
        assert_eq!(union.get_member_count(), 3);
        assert_eq!(union.get_child_pos(union.get_member(1).as_dyn_def()), 1);
        assert!(union.get_member_types().contains(DefType::dtString));
        assert!(!union.get_member_types().contains(DefType::dtFloat));
        assert_eq!(union.get_member(2).get_path(), "Value \\ Text");
        assert!(union.needs_element_to_resolve());
        assert_eq!(UnionDef::clone_from(&union).to_string(Some(&[0]), None), "0");
    }

    #[test]
    fn union_of_equal_sizes_is_not_variable() {
        let _guard = test_lock();
        let union = UnionDef::create(
            args("Value"),
            by_first_byte(),
            vec![int("Unsigned", IntType::itU16), int("Signed", IntType::itS16)],
        );
        assert!(!union.get_is_variable_size());
        // A union of fixed size does not ask the decider for its size.
        assert_eq!(union.get_size(Some(&[9, 9, 9]), None), 2);
        assert_eq!(union.to_string(Some(&[1, 0xFF]), None), "-255");
    }

    #[test]
    fn recursive_resolves_to_its_ancestor() {
        let _guard = test_lock();
        let recursive = RecursiveDef::create(args("Child"), 2);
        assert!(recursive.resolve_def(None, None).is_none());
        assert!(recursive.get_is_variable_size_internal());
        assert_eq!(recursive.to_string(Some(&[1]), None), "");

        let recursive_value: Arc<dyn ValueDef> = recursive.clone();
        let children = ArrayDef::create(
            args("Children"),
            ArrayDefArgs {
                element: Some(recursive_value),
                count: 0,
                count_callback: None,
                labels: Vec::new(),
                sorted: false,
                can_add_to: true,
                terminated: false,
            },
        );
        let node = StructDef::create(
            args("Node"),
            StructDefArgs {
                members: vec![int("ID", IntType::itU8), children],
                ..StructDefArgs::default()
            },
        );
        let resolved = recursive.resolve_def(None, None).unwrap();
        assert_eq!(resolved.get_name(), "Node");
        assert_eq!(resolved.get_def_id(), node.get_def_id());
        assert_eq!(recursive.get_def_type_name(), "Resolvable");
        assert_eq!(recursive.get_path(), "Node \\ Children \\ Child");
    }
}
