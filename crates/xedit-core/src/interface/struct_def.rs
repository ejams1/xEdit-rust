// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbStructDef`.
//!
//! `TwbStructCDef` and its descendants are the chapters of save files and come
//! with the save game definitions.

use std::sync::{Arc, Weak};

use super::def::{
    Def, DefBase, DefCell, DefKind, DefRef, NamedDef, NamedDefArgs, NamedDefBase, ValueDef, ValueDefBase, set_parent,
    value_def_plumbing,
};
use super::element::{DataPtr, ElementArg, ElementRef};
use super::globals::hide_ignored;
use super::types::{CallbackType, ConflictPriority, DefFlag, DefType, ElementType};

pub type StructSizeCallback = Arc<dyn Fn(DataPtr, ElementArg) -> u32 + Send + Sync>;

/// The data from `offset` on, as upstream `PByte(aBasePtr) + offset` with the
/// same end pointer. A base pointer past the end gives empty data.
pub(crate) fn data_from(data: DataPtr, offset: usize) -> DataPtr {
    data.map(|data| &data[offset.min(data.len())..])
}

/// The data between two offsets, both limited to the end of the data.
pub(crate) fn data_between(data: DataPtr, start: usize, end: usize) -> DataPtr {
    data.map(|data| {
        let start = start.min(data.len());
        let end = end.clamp(start, data.len());
        &data[start..end]
    })
}

/// Port of `TFromArray<T>.Get`: the entry at `index`, or the default value.
pub(crate) fn from_array<T: Clone + Default>(array: &[T], index: usize) -> T {
    array.get(index).cloned().unwrap_or_default()
}

/// Port of `SetArrayEntry` in `wbSetPrefixSuffix` and `wbSetMaxDepth`: grows
/// the array for a value that is not the default, then stores the value when
/// the array has the entry.
pub(crate) fn set_array_entry<T: Clone + Default + PartialEq>(array: &mut Vec<T>, index: usize, value: T) {
    if value != T::default() && array.len() < index + 1 {
        array.resize(index + 1, T::default());
    }
    if let Some(entry) = array.get_mut(index) {
        *entry = value;
    }
}

/// Delphi `string.Trim`: removes leading and trailing characters up to the blank.
pub(crate) fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| c <= ' ')
}

/// The constructor arguments of `TwbStructDef` after those of `TwbNamedDef`.
#[derive(Default)]
pub struct StructDefArgs {
    pub members: Vec<Arc<dyn ValueDef>>,
    pub sort_key: Vec<i32>,
    pub ex_sort_key: Vec<i32>,
    pub element_map: Vec<u32>,
    pub optional_from_element: i32,
}

/// Upstream `TwbStructDef`: members that follow each other in the data.
pub struct StructDef {
    self_ref: Weak<StructDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
    st_members: Vec<Arc<dyn ValueDef>>,
    st_sort_key: Vec<i32>,
    st_ex_sort_key: Vec<i32>,
    st_summary_key: DefCell<Vec<i32>>,
    st_element_map: Vec<u32>,
    st_optional_from_element: i32,
    st_summary_prefix: DefCell<Vec<String>>,
    st_summary_suffix: DefCell<Vec<String>>,
    st_summary_max_depth: DefCell<Vec<i32>>,
    st_summary_delimiter: DefCell<String>,
    st_size_callback: DefCell<StructSizeCallback>,
}

impl StructDef {
    /// Port of `TwbStructDef.Create`. The `terminator` argument is not used, as upstream.
    pub fn create(args: NamedDefArgs, structure: StructDefArgs) -> Arc<Self> {
        let (def, nd) = NamedDefBase::create(NamedDefArgs {
            terminator: false,
            ..args
        });
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| {
            let parent: Weak<dyn Def> = self_ref.clone();
            let st_members: Vec<Arc<dyn ValueDef>> = structure
                .members
                .into_iter()
                .map(|member| set_parent(member, &parent, false))
                .collect();
            assert!(structure.element_map.is_empty() || structure.element_map.len() == st_members.len());
            Self {
                self_ref: self_ref.clone(),
                def,
                nd,
                vd: ValueDefBase::default(),
                st_members,
                st_sort_key: structure.sort_key,
                st_ex_sort_key: structure.ex_sort_key,
                st_summary_key: DefCell::default(),
                st_element_map: structure.element_map,
                st_optional_from_element: structure.optional_from_element,
                st_summary_prefix: DefCell::default(),
                st_summary_suffix: DefCell::default(),
                st_summary_max_depth: DefCell::default(),
                st_summary_delimiter: DefCell::new(Some(" ".to_owned())),
                st_size_callback: DefCell::default(),
            }
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbStructDef.Clone` and `AfterClone`.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(
            NamedDefBase::clone_args(source),
            StructDefArgs {
                members: source.st_members.clone(),
                sort_key: source.st_sort_key.clone(),
                ex_sort_key: source.st_ex_sort_key.clone(),
                element_map: source.st_element_map.clone(),
                optional_from_element: source.st_optional_from_element,
            },
        );
        ValueDefBase::after_clone(&*this, source);
        this.st_summary_key.assign(&source.st_summary_key);
        this.st_summary_prefix.assign(&source.st_summary_prefix);
        this.st_summary_suffix.assign(&source.st_summary_suffix);
        this.st_summary_max_depth.assign(&source.st_summary_max_depth);
        this.st_summary_delimiter.assign(&source.st_summary_delimiter);
        this.st_size_callback.assign(&source.st_size_callback);
        this
    }

    pub fn get_member_count(&self) -> i32 {
        self.st_members.len() as i32
    }

    /// Upstream `GetMember`. Fails when `index` is out of range.
    pub fn get_member(&self, index: usize) -> &Arc<dyn ValueDef> {
        &self.st_members[index]
    }

    /// Upstream `GetMemberByName`: the first member with this name, ignoring case.
    pub fn get_member_by_name(&self, name: &str) -> Option<&Arc<dyn ValueDef>> {
        self.st_members
            .iter()
            .find(|member| member.get_name().eq_ignore_ascii_case(name))
    }

    pub fn get_optional_from_element(&self) -> i32 {
        self.st_optional_from_element
    }

    fn unlocked(self: Arc<Self>) -> Arc<Self> {
        if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        }
    }

    /// Port of `SetSummaryKey`.
    pub fn set_summary_key(self: Arc<Self>, summary_key: &[i32]) -> Arc<Self> {
        let this = self.unlocked();
        this.st_summary_key.set(Some(summary_key.to_vec()));
        this
    }

    /// Port of `SetSummaryMemberPrefixSuffix`.
    pub fn set_summary_member_prefix_suffix(self: Arc<Self>, index: usize, prefix: &str, suffix: &str) -> Arc<Self> {
        let this = self.unlocked();
        assert!(
            index < this.st_members.len(),
            "[TwbStructDef.SetSummaryMemberPrefixSuffix] not InRange(aIndex, Low(stMembers), High(stMembers))"
        );
        let mut prefixes = this.loaded(&this.st_summary_prefix);
        set_array_entry(&mut prefixes, index, prefix.to_owned());
        this.st_summary_prefix.set(Some(prefixes));
        let mut suffixes = this.loaded(&this.st_summary_suffix);
        set_array_entry(&mut suffixes, index, suffix.to_owned());
        this.st_summary_suffix.set(Some(suffixes));
        this
    }

    /// Port of `SetSummaryMemberMaxDepth`.
    pub fn set_summary_member_max_depth(self: Arc<Self>, index: usize, max_depth: i32) -> Arc<Self> {
        let this = self.unlocked();
        assert!(
            index < this.st_members.len(),
            "[TwbStructDef.SetSummaryMemberMaxDepth] not InRange(aIndex, Low(stMembers), High(stMembers))"
        );
        let mut max_depths = this.loaded(&this.st_summary_max_depth);
        set_array_entry(&mut max_depths, index, max_depth);
        this.st_summary_max_depth.set(Some(max_depths));
        this
    }

    /// Port of `SetSummaryDelimiter`.
    pub fn set_summary_delimiter(self: Arc<Self>, delimiter: &str) -> Arc<Self> {
        let this = self.unlocked();
        this.st_summary_delimiter.set(Some(delimiter.to_owned()));
        this
    }

    /// Port of `SetSizeCallback`.
    pub fn set_size_callback(self: Arc<Self>, callback: Option<StructSizeCallback>) -> Arc<Self> {
        let this = self.unlocked();
        this.st_size_callback.set(callback);
        this
    }

    fn loaded<T: Clone + Default>(&self, cell: &DefCell<T>) -> T {
        cell.load().as_deref().cloned().unwrap_or_default()
    }

    /// The sort key of the member `sort_member`, which starts after the
    /// members before it.
    fn member_sort_key(&self, sort_member: usize, data: DataPtr, element: ElementArg, extended: bool) -> String {
        let mut offset = 0usize;
        for member in &self.st_members[..sort_member] {
            offset = offset.saturating_add(member.get_size(data_from(data, offset), element).max(0) as usize);
        }
        let member = &self.st_members[sort_member];
        let size = member.get_size(data_from(data, offset), element).max(0) as usize;
        member.to_sort_key(
            data_between(data, offset, offset.saturating_add(size)),
            element,
            extended,
        )
    }

    fn keys_sort_key(&self, keys: &[i32], data: DataPtr, element: ElementArg, extended: bool) -> String {
        let mut result = String::new();
        for (i, &sort_member) in keys.iter().enumerate() {
            if let Ok(sort_member) = usize::try_from(sort_member)
                && sort_member < self.st_members.len()
            {
                result.push_str(&self.member_sort_key(sort_member, data, element, extended));
            }
            if i + 1 < keys.len() {
                result.push('|');
            }
        }
        result
    }

    /// Port of the nested `Process` of `ToSummary`: adds the summaries of the
    /// members in `keys` to `state`.
    fn summary_process(
        &self,
        keys: &[i32],
        depth: i32,
        container: &ElementRef,
        links_to: &mut Option<ElementRef>,
        state: &mut SummaryState,
    ) {
        let Some(cer) = container.as_container() else {
            return;
        };
        let max_depths = self.loaded(&self.st_summary_max_depth);
        let prefixes = self.loaded(&self.st_summary_prefix);
        let suffixes = self.loaded(&self.st_summary_suffix);
        let delimiter = self.loaded(&self.st_summary_delimiter);
        for &sort_member in keys {
            let Ok(sort_member) = usize::try_from(sort_member) else {
                continue;
            };
            if sort_member >= self.st_members.len() {
                continue;
            }
            if state.member_used.is_empty() {
                state.member_used = vec![false; self.st_members.len()];
            }
            if state.member_used[sort_member] {
                continue;
            }
            state.member_used[sort_member] = true;
            let max_depth = from_array(&max_depths, sort_member);
            if !(max_depth == 0 || depth < max_depth) {
                continue;
            }
            let Some(element) = cer.get_element_by_sort_order(sort_member as i32 + cer.get_additional_element_count())
            else {
                continue;
            };
            let (Some(member_cer), Some(dc)) = (element.as_container(), element.as_data_container()) else {
                continue;
            };
            let element_def = element.get_def();
            let shows = state.members_show_ignore
                || element_def
                    .as_deref()
                    .is_some_and(|def| def.get_def_flags().contains(DefFlag::dfSummaryShowIgnore))
                || !hide_ignored()
                || element.get_conflict_priority() > ConflictPriority::cpIgnore;
            if !shows || element.get_dont_show() {
                continue;
            }
            let mut member_def = self.st_members[sort_member].clone();
            if let Some(dc_def) = element_def
                && !member_def.equals(Some(dc_def.as_dyn_def()))
            {
                // A union member shows through the definition it resolved to, and
                // an empty place holder through its own.
                let replaces =
                    member_def.get_def_type() == DefType::dtUnion || dc_def.get_def_type() == DefType::dtEmpty;
                debug_assert!(
                    replaces,
                    "TwbStructDef.ToSummary for [{}]: [{}] is not equal to [{}]",
                    element.get_full_path(),
                    member_def.get_path(),
                    dc_def.get_path()
                );
                if replaces && let Some(value_def) = dc_def.into_value_def() {
                    member_def = value_def;
                }
            }
            let summary = member_def.to_summary(depth + 1, dc.get_data(), Some(&element), links_to);
            let member_summary = trim(&summary);
            if member_summary.is_empty() {
                continue;
            }
            let prefix = from_array(&prefixes, sort_member);
            let suffix = from_array(&suffixes, sort_member);
            let has_fix = !prefix.is_empty() || !suffix.is_empty();
            let no_name = state.members_no_name || member_def.get_def_flags().contains(DefFlag::dfSummaryNoName);
            if !state.result.is_empty() {
                if !state.delayed_name.is_empty() {
                    state.result = format!("{}:({})", state.delayed_name, state.result);
                    state.delayed_name.clear();
                }
                state.result.push_str(&delimiter);
            }
            let mut member_summary_name = member_def.get_summary_name();
            if member_cer.get_element_type() == ElementType::etArray && member_cer.get_element_count() == 1 {
                member_summary_name = member_def.get_summary_singular_name();
            }
            let named_start = format!("{member_summary_name}:(").to_lowercase();
            if no_name || has_fix || member_summary.to_lowercase().starts_with(&named_start) {
                state.result.push_str(&prefix);
                state.result.push_str(member_summary);
                state.result.push_str(&suffix);
            } else if state.result.is_empty() {
                state.delayed_name = member_summary_name;
                state.result = member_summary.to_owned();
            } else {
                state
                    .result
                    .push_str(&format!("{member_summary_name}:({member_summary})"));
            }
        }
    }
}

/// The variables that the nested `Process` of `ToSummary` shares upstream.
struct SummaryState {
    result: String,
    member_used: Vec<bool>,
    delayed_name: String,
    members_no_name: bool,
    members_show_ignore: bool,
}

impl Def for StructDef {
    value_def_plumbing!(Def);

    fn get_def_type(&self) -> DefType {
        DefType::dtStruct
    }

    fn get_def_type_name(&self) -> String {
        "Structure".to_owned()
    }

    fn as_struct_def(&self) -> Option<&StructDef> {
        Some(self)
    }

    fn get_child_pos(&self, child: &dyn Def) -> i32 {
        self.st_members
            .iter()
            .position(|member| child.equals(Some(member.as_dyn_def())))
            .map_or(-1, |index| index as i32)
    }

    fn init_from_parent_do_children(&self) {
        for member in &self.st_members {
            member.init_from_parent();
        }
    }
}

impl NamedDef for StructDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for StructDef {
    value_def_plumbing!(ValueDef);

    fn to_string(&self, data: DataPtr, element: ElementArg) -> String {
        let mut result = String::new();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToStr);
        }
        self.used(element, &result);
        result
    }

    fn to_summary(&self, depth: i32, data: DataPtr, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        let mut result = String::new();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToSummary);
        }
        if result.is_empty()
            && let Some(container) = element
            && container.as_container().is_some()
        {
            let flags = self.def.def_flags.get();
            let mut state = SummaryState {
                result: String::new(),
                member_used: Vec::new(),
                delayed_name: String::new(),
                members_no_name: flags.contains(DefFlag::dfSummaryMembersNoName),
                members_show_ignore: flags.contains(DefFlag::dfSummaryMembersShowIgnore),
            };
            if !flags.contains(DefFlag::dfSummaryNoSortKey) {
                self.summary_process(&self.st_sort_key, depth, container, links_to, &mut state);
                self.summary_process(&self.st_ex_sort_key, depth, container, links_to, &mut state);
            }
            let summary_key = self.loaded(&self.st_summary_key);
            self.summary_process(&summary_key, depth, container, links_to, &mut state);
            result = state.result;
        }
        if links_to.is_none()
            && !result.is_empty()
            && let Some(element) = element
        {
            *links_to = element.get_links_to();
        }
        result
    }

    fn to_sort_key(&self, data: DataPtr, element: ElementArg, extended: bool) -> String {
        let mut result = String::new();
        let has_ex_key = extended && !self.st_ex_sort_key.is_empty();
        if !self.st_sort_key.is_empty() || has_ex_key {
            result.push_str(&self.keys_sort_key(&self.st_sort_key, data, element, extended));
            if extended {
                if !self.st_sort_key.is_empty() && !self.st_ex_sort_key.is_empty() {
                    result.push('|');
                }
                result.push_str(&self.keys_sort_key(&self.st_ex_sort_key, data, element, extended));
            }
        } else {
            let mut offset = 0usize;
            for (j, member) in self.st_members.iter().enumerate() {
                let size = member.get_size(data_from(data, offset), element).max(0) as usize;
                let end = offset.saturating_add(size);
                result.push_str(&member.to_sort_key(data_between(data, offset, end), element, extended));
                offset = end.min(data.map_or(0, <[u8]>::len));
                if j + 1 < self.st_members.len() {
                    result.push('|');
                }
            }
        }
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToSortKey);
        }
        result
    }

    fn get_size(&self, data: DataPtr, element: ElementArg) -> i32 {
        if let Some(callback) = self.st_size_callback.load().as_deref() {
            return callback(data, element) as i32;
        }
        let len = data.map_or(0, <[u8]>::len);
        if (data.is_none() || len == 0) && self.get_is_variable_size_internal() {
            return 0;
        }
        let mut result: i32 = 0;
        let mut offset = 0usize;
        // Adds one size. Returns `false` when the loop upstream breaks.
        let add = |size: i32, result: &mut i32, offset: &mut usize| -> bool {
            if size == i32::MAX {
                *result = i32::MAX;
                return false;
            }
            *result = result.wrapping_add(size);
            if data.is_some() && (len as i64) < i64::from(*result) {
                *result = len as i32;
                return false;
            }
            *offset = offset.saturating_add(size.max(0) as usize);
            true
        };
        // The elements of the container know the definitions that unions resolved to.
        let container = element.filter(|_| self.get_is_variable_size()).and_then(|element| {
            let container = element.as_container()?;
            let value_def = element.get_value_def()?;
            (self.equals(Some(value_def.as_dyn_def())) && container.get_element_count() > 0).then_some(container)
        });
        match container {
            Some(container) => {
                for i in 0..container.get_element_count() {
                    let Some(child) = container.get_element(i) else {
                        break;
                    };
                    let Some(value_def) = child.get_value_def() else {
                        break;
                    };
                    let size = value_def.get_size(data_from(data, offset), Some(&child));
                    if !add(size, &mut result, &mut offset) {
                        break;
                    }
                }
            }
            None => {
                for member in &self.st_members {
                    let size = member.get_size(data_from(data, offset), element);
                    if !add(size, &mut result, &mut offset) {
                        break;
                    }
                }
            }
        }
        result
    }

    fn get_default_size(&self, data: DataPtr, element: ElementArg) -> i32 {
        if let Some(callback) = self.st_size_callback.load().as_deref() {
            return callback(data, element) as i32;
        }
        let mut result: i32 = 0;
        let mut offset = 0usize;
        for member in &self.st_members {
            let size = member.get_default_size(data_from(data, offset), element);
            if size == i32::MAX {
                return i32::MAX;
            }
            offset = offset.saturating_add(size.max(0) as usize);
            result = result.wrapping_add(size);
        }
        result
    }

    fn get_is_variable_size_internal(&self) -> bool {
        self.st_members.iter().any(|member| member.get_is_variable_size())
    }

    fn get_element_map(&self) -> Vec<u32> {
        self.st_element_map.clone()
    }
}

impl DefKind for StructDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::globals::test_lock;
    use super::super::integer::IntegerDef;
    use super::super::string::{StringClass, StringDef};
    use super::super::types::IntType;
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

    fn sample(sort_key: Vec<i32>, ex_sort_key: Vec<i32>) -> Arc<StructDef> {
        StructDef::create(
            args("Data"),
            StructDefArgs {
                members: vec![
                    int("Value", IntType::itU16),
                    StringDef::create(StringClass::String, args("Name"), 0, false),
                    int("Count", IntType::itU8),
                ],
                sort_key,
                ex_sort_key,
                ..StructDefArgs::default()
            },
        )
    }

    const DATA: &[u8] = b"\x05\x00ab\0\x07";

    #[test]
    fn members_and_sizes() {
        let _guard = test_lock();
        let def = sample(vec![], vec![]);
        assert_eq!(def.get_member_count(), 3);
        assert_eq!(def.get_member(1).get_path(), "Data \\ Name");
        assert_eq!(def.get_member_by_name("count").unwrap().get_name(), "Count");
        assert!(def.get_member_by_name("other").is_none());
        assert_eq!(def.get_child_pos(def.get_member(2).as_dyn_def()), 2);
        assert_eq!(def.get_child_pos(def.as_dyn_def()), -1);
        assert!(def.get_is_variable_size());
        assert_eq!(def.get_size(Some(DATA), None), 6);
        assert_eq!(def.get_size(Some(&[]), None), 0);
        assert_eq!(def.get_size(None, None), 0);
        // Data that ends early limits the size.
        assert_eq!(def.get_size(Some(&DATA[..3]), None), 3);
        assert_eq!(def.get_default_size(None, None), 4);
        assert_eq!(def.to_string(Some(DATA), None), "");

        let fixed = StructDef::create(
            args("Pair"),
            StructDefArgs {
                members: vec![int("A", IntType::itU32), int("B", IntType::itU16)],
                ..StructDefArgs::default()
            },
        );
        assert!(!fixed.get_is_variable_size());
        assert_eq!(fixed.get_size(None, None), 6);
        assert_eq!(fixed.get_size(Some(&[0; 4]), None), 4);
        let sized = fixed.set_size_callback(Some(Arc::new(|_, _| 9)));
        assert_eq!(sized.get_size(None, None), 9);
        assert_eq!(sized.get_default_size(None, None), 9);
    }

    #[test]
    fn sort_keys() {
        let _guard = test_lock();
        assert_eq!(
            sample(vec![], vec![]).to_sort_key(Some(DATA), None, false),
            "00005|AB|007"
        );
        let keyed = sample(vec![2, 0], vec![1]);
        assert_eq!(keyed.to_sort_key(Some(DATA), None, false), "007|00005");
        assert_eq!(keyed.to_sort_key(Some(DATA), None, true), "007|00005|AB");
        // A key that is not a member adds nothing but keeps its separator.
        assert_eq!(
            sample(vec![7, 0], vec![]).to_sort_key(Some(DATA), None, false),
            "|00005"
        );
        // Without a sort key the extended key alone is used.
        assert_eq!(sample(vec![], vec![1]).to_sort_key(Some(DATA), None, true), "AB");
    }

    #[test]
    fn setters_duplicate_locked_definitions() {
        let _guard = test_lock();
        let def = sample(vec![0], vec![]);
        let same = def.clone().set_summary_key(&[1, 2]);
        assert!(Arc::ptr_eq(&same, &def));
        let member = def.get_member(0).clone();
        assert!(member.def_base().def_is_locked());

        let outer = StructDef::create(
            args("Outer"),
            StructDefArgs {
                members: vec![def.clone()],
                ..StructDefArgs::default()
            },
        );
        assert_eq!(def.get_path(), "Outer \\ Data");
        let changed = def
            .clone()
            .set_summary_delimiter(", ")
            .set_summary_member_prefix_suffix(1, "[", "]")
            .set_summary_member_max_depth(2, 3);
        assert!(!Arc::ptr_eq(&changed, &def));
        assert!(changed.get_parent().is_none());
        assert_eq!(changed.loaded(&changed.st_summary_key), [1, 2]);
        assert_eq!(changed.loaded(&changed.st_summary_prefix), ["", "["]);
        assert_eq!(changed.loaded(&changed.st_summary_max_depth), [0, 0, 3]);
        assert_eq!(def.loaded(&def.st_summary_delimiter), " ");
        // The duplicate has its own copies of the members.
        assert!(!Arc::ptr_eq(changed.get_member(0), &member));
        assert_eq!(outer.get_member_count(), 1);
    }

    #[test]
    fn helpers() {
        assert_eq!(data_from(Some(b"abc"), 1), Some(&b"bc"[..]));
        assert_eq!(data_from(Some(b"abc"), 9), Some(&b""[..]));
        assert_eq!(data_from(None, 1), None);
        assert_eq!(data_between(Some(b"abc"), 1, 9), Some(&b"bc"[..]));
        let mut prefixes: Vec<String> = Vec::new();
        set_array_entry(&mut prefixes, 2, String::new());
        assert!(prefixes.is_empty());
        set_array_entry(&mut prefixes, 2, "x".to_owned());
        assert_eq!(prefixes, ["", "", "x"]);
        set_array_entry(&mut prefixes, 2, String::new());
        assert_eq!(prefixes, ["", "", ""]);
        assert_eq!(from_array(&prefixes, 5), "");
        assert_eq!(trim(" \t a b \r\n"), "a b");
    }
}
