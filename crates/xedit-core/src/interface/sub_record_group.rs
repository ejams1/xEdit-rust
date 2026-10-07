// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! The record members that group subrecords: `TwbSubRecordArrayDef`,
//! `TwbSubRecordStructDef` with `TwbSubRecordStructSKDef`, and
//! `TwbSubRecordUnionDef`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Weak};

use super::def::{
    Def, DefBase, DefCell, DefKind, DefRef, NamedDef, NamedDefArgs, NamedDefBase, def_dont_assign, set_parent,
};
use super::element::{ElementArg, ElementRef, MainRecordRef};
use super::globals::{hide_ignored, never_sorted, report_mode, report_required};
use super::struct_def::{from_array, set_array_entry, trim};
use super::sub_record::{
    RecordMemberDef, SignatureDef, record_member_def_to_summary, record_member_plumbing, signature_def_get_full_name,
};
use super::types::{ASSIGN_ADD, ASSIGN_THIS, ConflictPriority, DefFlag, DefType, ElementType, Signature};

/// Decides whether a subrecord array is sorted, from its container.
pub type IsSortedCallback = Arc<dyn Fn(ElementArg) -> bool + Send + Sync>;

/// Decides which member of a subrecord union a container holds.
pub type RUnionDecider = Arc<dyn Fn(ElementArg) -> i32 + Send + Sync>;

/// Upstream `IwbRecordDef`: a definition with record members. Implemented by
/// the main record, the subrecord struct and the subrecord union.
pub trait RecordDef: SignatureDef {
    fn contains_member_for(&self, container: ElementArg, signature: Signature, data_container: ElementArg) -> bool;

    fn get_member_for(
        &self,
        container: ElementArg,
        signature: Signature,
        data_container: ElementArg,
    ) -> Option<Arc<dyn RecordMemberDef>>;

    /// The index of the member for the subrecord `signature`, or -1.
    fn get_member_index_for(&self, container: ElementArg, signature: Signature, data_container: ElementArg) -> i32;

    fn allow_unordered(&self) -> bool;

    fn additional_info_for(&self, _main_record: &MainRecordRef) -> String {
        String::new()
    }

    /// Upstream `GetMember`. Fails when `index` is out of range.
    fn get_member(&self, index: usize) -> Arc<dyn RecordMemberDef>;

    fn get_member_count(&self) -> i32;

    /// Whether subrecords with this signature are skipped when the record loads.
    fn get_skip_signature(&self, signature: Signature) -> bool;
}

/// Port of `StructKeysToSummary`: adds the summaries of the members in `keys`
/// to `result`.
#[allow(clippy::too_many_arguments)]
pub fn struct_keys_to_summary(
    depth: i32,
    result: &mut String,
    element: ElementArg,
    members: &[Arc<dyn RecordMemberDef>],
    keys: &[i32],
    prefixes: &[String],
    suffixes: &[String],
    max_depths: &[i32],
    delimiter: &str,
    links_to: &mut Option<ElementRef>,
) {
    if keys.is_empty() {
        return;
    }
    let Some(container) = element else { return };
    let Some(cer) = container.as_container() else {
        return;
    };
    let mut delayed_name = String::new();
    let container_flags = container.get_def().map(|def| def.get_def_flags()).unwrap_or_default();
    let members_no_name = container_flags.contains(DefFlag::dfSummaryMembersNoName);
    let members_show_ignore = container_flags.contains(DefFlag::dfSummaryMembersShowIgnore);
    for &sort_order in keys {
        let Ok(index) = usize::try_from(sort_order) else {
            continue;
        };
        if index >= members.len() {
            continue;
        }
        let max_depth = from_array(max_depths, index);
        if !(max_depth == 0 || depth < max_depth) {
            continue;
        }
        let Some(member) = cer.get_element_by_sort_order(sort_order + cer.get_additional_element_count()) else {
            continue;
        };
        if member.get_dont_show() {
            continue;
        }
        let Some(member_def) = member.get_def() else {
            continue;
        };
        let Some(rmd) = member_def.as_record_member_def() else {
            continue;
        };
        if !(members_show_ignore
            || rmd.get_def_flags().contains(DefFlag::dfSummaryShowIgnore)
            || !hide_ignored()
            || member.get_conflict_priority() > ConflictPriority::cpIgnore)
        {
            continue;
        }
        let summary = rmd.to_summary(depth + 1, Some(&member), links_to);
        let member_summary = trim(&summary);
        if member_summary.is_empty() {
            continue;
        }
        let prefix = from_array(prefixes, index);
        let suffix = from_array(suffixes, index);
        let has_fix = !prefix.is_empty() || !suffix.is_empty();
        let no_name = members_no_name || rmd.get_def_flags().contains(DefFlag::dfSummaryNoName);
        if !result.is_empty() {
            if !delayed_name.is_empty() {
                *result = format!("{delayed_name}:({result})");
                delayed_name.clear();
            }
            result.push_str(delimiter);
        }
        let mut member_summary_name = rmd.get_summary_name();
        // Upstream fails here for a member that is not a container.
        if let Some(member_cer) = member.as_container()
            && member.get_element_type() == ElementType::etSubRecordArray
            && member_cer.get_element_count() == 1
        {
            member_summary_name = rmd.get_summary_singular_name();
        }
        let named_start = format!("{member_summary_name}:(").to_lowercase();
        if no_name || has_fix || member_summary.to_lowercase().starts_with(&named_start) {
            result.push_str(&prefix);
            result.push_str(member_summary);
            result.push_str(&suffix);
        } else if result.is_empty() {
            delayed_name = member_summary_name;
            *result = member_summary.to_owned();
        } else {
            result.push_str(&format!("{member_summary_name}:({member_summary})"));
        }
    }
}

/// The paths without the empty ones, as `SetCountPath` stores them.
fn count_paths(values: &[&str]) -> Vec<String> {
    values
        .iter()
        .filter(|value| !value.is_empty())
        .map(|&value| value.to_owned())
        .collect()
}

/// Upstream `TwbSubRecordArrayDef`: a record member that repeats.
pub struct SubRecordArrayDef {
    self_ref: Weak<SubRecordArrayDef>,
    def: DefBase,
    nd: NamedDefBase,
    sra_element: Arc<dyn RecordMemberDef>,
    sra_count: i32,
    sra_sorted: bool,
    sra_is_sorted: Option<IsSortedCallback>,
    sra_default_edit_values: DefCell<Vec<String>>,
    sra_count_paths: DefCell<Vec<String>>,
}

impl SubRecordArrayDef {
    /// Port of `TwbSubRecordArrayDef.Create`. `args.terminator` is not used, as upstream.
    pub fn create(
        args: NamedDefArgs,
        element: Arc<dyn RecordMemberDef>,
        count: i32,
        sorted: bool,
        is_sorted: Option<IsSortedCallback>,
    ) -> Arc<Self> {
        let (def, nd) = NamedDefBase::create(NamedDefArgs {
            terminator: false,
            ..args
        });
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| {
            let parent: Weak<dyn Def> = self_ref.clone();
            Self {
                self_ref: self_ref.clone(),
                def,
                nd,
                sra_element: set_parent(element, &parent, false),
                sra_count: count,
                sra_sorted: sorted && !never_sorted(),
                sra_is_sorted: is_sorted,
                sra_default_edit_values: DefCell::default(),
                sra_count_paths: DefCell::default(),
            }
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbSubRecordArrayDef.Clone` and `AfterClone`.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(
            NamedDefBase::clone_args(source),
            source.sra_element.clone(),
            source.sra_count,
            source.sra_sorted,
            source.sra_is_sorted.clone(),
        );
        NamedDefBase::after_clone(&*this, source);
        this.sra_default_edit_values.assign(&source.sra_default_edit_values);
        this.sra_count_paths.assign(&source.sra_count_paths);
        this
    }

    pub fn get_element(&self) -> &Arc<dyn RecordMemberDef> {
        &self.sra_element
    }

    pub fn get_count(&self) -> i32 {
        self.sra_count
    }

    /// Upstream `GetSorted`: whether the elements of `container` show in sorted order.
    pub fn get_sorted(&self, container: ElementArg) -> bool {
        match &self.sra_is_sorted {
            Some(is_sorted) => is_sorted(container),
            None => self.sra_sorted,
        }
    }

    pub fn get_count_paths(&self) -> Vec<String> {
        self.sra_count_paths.load().as_deref().cloned().unwrap_or_default()
    }

    pub fn get_default_edit_values(&self) -> Vec<String> {
        self.sra_default_edit_values
            .load()
            .as_deref()
            .cloned()
            .unwrap_or_default()
    }

    fn unlocked(self: Arc<Self>) -> Arc<Self> {
        if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        }
    }

    /// Port of `SetCountPath`: the paths of the elements that hold the count.
    /// Port of `SetCountPath` with one path.
    pub fn set_count_path(self: Arc<Self>, value: &str) -> Arc<Self> {
        self.set_count_paths(&[value])
    }

    /// Port of `SetCountPath` with several paths.
    pub fn set_count_paths(self: Arc<Self>, values: &[&str]) -> Arc<Self> {
        let new_count_paths = count_paths(values);
        if new_count_paths == self.get_count_paths() {
            return self;
        }
        let this = self.unlocked();
        this.sra_count_paths.set(Some(new_count_paths));
        this
    }

    pub fn set_default_edit_values(self: Arc<Self>, values: &[&str]) -> Arc<Self> {
        let this = self.unlocked();
        this.sra_default_edit_values
            .set(Some(values.iter().map(|&value| value.to_owned()).collect()));
        this
    }
}

/// The member by member test of `CanAssign` for two record definitions with
/// the same member count.
fn record_members_can_assign(own: &dyn RecordDef, other: &dyn Def, element: ElementArg, index: i32) -> bool {
    let Some(other) = other.as_record_def() else {
        return false;
    };
    if own.get_member_count() != other.get_member_count() {
        return false;
    }
    let count = usize::try_from(own.get_member_count()).unwrap_or(0);
    (0..count).all(|i| {
        own.get_member(i)
            .can_assign(element, index, Some(other.get_member(i).as_dyn_def()))
    })
}

impl Def for SubRecordArrayDef {
    record_member_plumbing!(Def);

    /// Port of `TwbSubRecordArrayDef.CanAssign`.
    fn can_assign(&self, element: ElementArg, index: i32, def: Option<&dyn Def>) -> bool {
        if def_dont_assign(self) {
            return false;
        }
        if index == ASSIGN_THIS {
            def.and_then(|def| def.as_sub_record_array_def()).is_some_and(|other| {
                self.get_element()
                    .can_assign(element, index, Some(other.get_element().as_dyn_def()))
            })
        } else if index == ASSIGN_ADD {
            self.get_element().can_assign(element, ASSIGN_THIS, def)
        } else {
            false
        }
    }

    fn get_def_type(&self) -> DefType {
        DefType::dtSubRecordArray
    }

    fn get_def_type_name(&self) -> String {
        format!("SubRecordArray of {}", self.sra_element.get_def_type_name())
    }

    fn as_sub_record_array_def(&self) -> Option<&SubRecordArrayDef> {
        Some(self)
    }

    fn init_from_parent_do_children(&self) {
        self.sra_element.init_from_parent();
    }
}

impl NamedDef for SubRecordArrayDef {
    record_member_plumbing!(NamedDef);

    fn after_load(&self, element: &ElementRef) {
        self.used(None, "");
        if let Some(after_load) = self.nd.nd_after_load.load().as_deref() {
            after_load(element);
        }
        if report_mode()
            && report_required()
            && let Some(container) = element.as_container()
        {
            self.sra_element.possibly_required();
            if container.get_element_count() < 1 {
                self.sra_element.not_required();
            }
        }
    }
}

impl SignatureDef for SubRecordArrayDef {
    fn get_default_signature(&self) -> Signature {
        self.sra_element.get_default_signature()
    }

    fn get_signature(&self, index: i32) -> Signature {
        self.sra_element.get_signature(index)
    }

    fn get_signature_count(&self) -> i32 {
        self.sra_element.get_signature_count()
    }

    fn can_handle(&self, container: ElementArg, signature: Signature, data_container: ElementArg) -> bool {
        self.sra_element.can_handle(container, signature, data_container)
    }
}

impl RecordMemberDef for SubRecordArrayDef {
    fn to_summary_internal(&self, depth: i32, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        let mut result = String::new();
        if let Some(array) = element
            && let Some(cer) = array.as_container()
        {
            let count = cer.get_element_count();
            if count > 0 {
                if count == 1
                    && let Some(child) = cer.get_element(0)
                    && let Some(child_def) = child.get_def()
                    && let Some(rmd) = child_def.as_record_member_def()
                {
                    let summary = rmd.to_summary(depth + 1, Some(&child), links_to);
                    let summary = trim(&summary);
                    if !summary.is_empty() {
                        return summary.to_owned();
                    }
                }
                let name = if count == 1 {
                    self.get_summary_singular_name()
                } else {
                    self.get_summary_name()
                };
                result = format!("<{count} {}>", name.to_lowercase());
            }
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

impl DefKind for SubRecordArrayDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

/// Signature to the index of the member that handles it. Upstream keeps a
/// sorted string list that ignores duplicates, so the first member wins.
fn signature_map(members: &[Arc<dyn RecordMemberDef>]) -> BTreeMap<Signature, usize> {
    let mut map = BTreeMap::new();
    for (index, member) in members.iter().enumerate() {
        for j in 0..member.get_signature_count() {
            map.entry(member.get_signature(j)).or_insert(index);
        }
    }
    map
}

/// The sort key of a `TwbSubRecordStructSKDef`.
struct StructSortKey {
    srs_sort_key: Vec<i32>,
    srs_ex_sort_key: Vec<i32>,
    srs_member_in_sk: Vec<bool>,
}

/// Upstream `TwbSubRecordStructDef`: record members that belong together.
/// With a sort key it is upstream `TwbSubRecordStructSKDef`.
pub struct SubRecordStructDef {
    self_ref: Weak<SubRecordStructDef>,
    def: DefBase,
    nd: NamedDefBase,
    srs_members: Vec<Arc<dyn RecordMemberDef>>,
    srs_signatures: BTreeMap<Signature, usize>,
    srs_skip_signatures: BTreeSet<Signature>,
    srs_allow_unordered: bool,
    srs_summary_key: DefCell<Vec<i32>>,
    srs_summary_prefix: DefCell<Vec<String>>,
    srs_summary_suffix: DefCell<Vec<String>>,
    srs_summary_max_depth: DefCell<Vec<i32>>,
    srs_summary_delimiter: DefCell<String>,
    sort_key: Option<StructSortKey>,
}

impl SubRecordStructDef {
    /// Port of `TwbSubRecordStructDef.Create`. `args.terminator` is not used, as upstream.
    pub fn create(
        args: NamedDefArgs,
        members: Vec<Arc<dyn RecordMemberDef>>,
        skip_sigs: &[Signature],
        allow_unordered: bool,
    ) -> Arc<Self> {
        Self::new(args, members, skip_sigs, allow_unordered, None)
    }

    /// Port of `TwbSubRecordStructSKDef.Create`: the members in `sort_key`
    /// make the sort key, those in `ex_sort_key` the extended sort key.
    pub fn create_sk(
        args: NamedDefArgs,
        members: Vec<Arc<dyn RecordMemberDef>>,
        skip_sigs: &[Signature],
        sort_key: &[i32],
        ex_sort_key: &[i32],
        allow_unordered: bool,
    ) -> Arc<Self> {
        let mut srs_member_in_sk = vec![false; members.len()];
        for &key in sort_key {
            assert!(!srs_member_in_sk[key as usize]);
            srs_member_in_sk[key as usize] = true;
        }
        Self::new(
            args,
            members,
            skip_sigs,
            allow_unordered,
            Some(StructSortKey {
                srs_sort_key: sort_key.to_vec(),
                srs_ex_sort_key: ex_sort_key.to_vec(),
                srs_member_in_sk,
            }),
        )
    }

    fn new(
        args: NamedDefArgs,
        members: Vec<Arc<dyn RecordMemberDef>>,
        skip_sigs: &[Signature],
        allow_unordered: bool,
        sort_key: Option<StructSortKey>,
    ) -> Arc<Self> {
        let (def, nd) = NamedDefBase::create(NamedDefArgs {
            terminator: false,
            ..args
        });
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| {
            let parent: Weak<dyn Def> = self_ref.clone();
            let srs_members: Vec<Arc<dyn RecordMemberDef>> = members
                .into_iter()
                .map(|member| set_parent(member, &parent, false))
                .collect();
            Self {
                self_ref: self_ref.clone(),
                def,
                nd,
                srs_signatures: signature_map(&srs_members),
                srs_members,
                srs_skip_signatures: skip_sigs.iter().copied().collect(),
                srs_allow_unordered: allow_unordered,
                srs_summary_key: DefCell::default(),
                srs_summary_prefix: DefCell::default(),
                srs_summary_suffix: DefCell::default(),
                srs_summary_max_depth: DefCell::default(),
                srs_summary_delimiter: DefCell::new(Some(" ".to_owned())),
                sort_key,
            }
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of the `Clone` constructors and `AfterClone`.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let skip_sigs: Vec<Signature> = source.srs_skip_signatures.iter().copied().collect();
        let this = Self::new(
            NamedDefBase::clone_args(source),
            source.srs_members.clone(),
            &skip_sigs,
            source.srs_allow_unordered,
            source.sort_key.as_ref().map(|key| StructSortKey {
                srs_sort_key: key.srs_sort_key.clone(),
                srs_ex_sort_key: key.srs_ex_sort_key.clone(),
                srs_member_in_sk: key.srs_member_in_sk.clone(),
            }),
        );
        NamedDefBase::after_clone(&*this, source);
        this.srs_summary_key.assign(&source.srs_summary_key);
        this.srs_summary_prefix.assign(&source.srs_summary_prefix);
        this.srs_summary_suffix.assign(&source.srs_summary_suffix);
        this.srs_summary_max_depth.assign(&source.srs_summary_max_depth);
        this.srs_summary_delimiter.assign(&source.srs_summary_delimiter);
        this
    }

    /// `Supports(def, IwbHasSortKeyDef)`.
    pub fn has_sort_key(&self) -> bool {
        self.sort_key.is_some()
    }

    /// Upstream `GetSortKey`: the member at position `index` of the sort key
    /// followed by the extended sort key. Fails when `index` is out of range.
    pub fn get_sort_key(&self, index: usize, _extended: bool) -> i32 {
        let key = self.sort_key.as_ref().expect("the structure has a sort key");
        match key.srs_sort_key.get(index) {
            Some(&member) => member,
            None => key.srs_ex_sort_key[index - key.srs_sort_key.len()],
        }
    }

    pub fn get_sort_key_count(&self, extended: bool) -> i32 {
        match &self.sort_key {
            Some(key) => (key.srs_sort_key.len() + if extended { key.srs_ex_sort_key.len() } else { 0 }) as i32,
            None => 0,
        }
    }

    /// Upstream `IsInSK`: whether the member `index` is part of the sort key.
    pub fn is_in_sk(&self, index: i32) -> bool {
        self.sort_key.as_ref().is_some_and(|key| {
            usize::try_from(index)
                .ok()
                .and_then(|index| key.srs_member_in_sk.get(index))
                .copied()
                .unwrap_or(false)
        })
    }

    fn any_member(&self) -> bool {
        self.srs_allow_unordered || self.def.def_flags.contains(DefFlag::dfAllowAnyMember)
    }

    fn unlocked(self: Arc<Self>) -> Arc<Self> {
        if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        }
    }

    fn loaded<T: Clone + Default>(cell: &DefCell<T>) -> T {
        cell.load().as_deref().cloned().unwrap_or_default()
    }

    pub fn set_summary_key(self: Arc<Self>, summary_key: &[i32]) -> Arc<Self> {
        let this = self.unlocked();
        this.srs_summary_key.set(Some(summary_key.to_vec()));
        this
    }

    pub fn set_summary_member_prefix_suffix(self: Arc<Self>, index: i32, prefix: &str, suffix: &str) -> Arc<Self> {
        let index = usize::try_from(index).expect("a non-negative index");
        let this = self.unlocked();
        assert!(
            index < this.srs_members.len(),
            "[TwbSubRecordStructDef.SetSummaryMemberPrefixSuffix] not InRange(aIndex, Low(srsMembers), \
             High(srsMembers))"
        );
        let mut prefixes = Self::loaded(&this.srs_summary_prefix);
        set_array_entry(&mut prefixes, index, prefix.to_owned());
        this.srs_summary_prefix.set(Some(prefixes));
        let mut suffixes = Self::loaded(&this.srs_summary_suffix);
        set_array_entry(&mut suffixes, index, suffix.to_owned());
        this.srs_summary_suffix.set(Some(suffixes));
        this
    }

    pub fn set_summary_member_max_depth(self: Arc<Self>, index: i32, max_depth: i32) -> Arc<Self> {
        let index = usize::try_from(index).expect("a non-negative index");
        let this = self.unlocked();
        assert!(
            index < this.srs_members.len(),
            "[TwbSubRecordStructDef.SetSummaryMemberMaxDepth] not InRange(aIndex, Low(srsMembers), High(srsMembers))"
        );
        let mut max_depths = Self::loaded(&this.srs_summary_max_depth);
        set_array_entry(&mut max_depths, index, max_depth);
        this.srs_summary_max_depth.set(Some(max_depths));
        this
    }

    pub fn set_summary_delimiter(self: Arc<Self>, delimiter: &str) -> Arc<Self> {
        let this = self.unlocked();
        this.srs_summary_delimiter.set(Some(delimiter.to_owned()));
        this
    }

    fn keys_to_summary(
        &self,
        keys: &[i32],
        depth: i32,
        result: &mut String,
        element: ElementArg,
        links_to: &mut Option<ElementRef>,
    ) {
        struct_keys_to_summary(
            depth,
            result,
            element,
            &self.srs_members,
            keys,
            &Self::loaded(&self.srs_summary_prefix),
            &Self::loaded(&self.srs_summary_suffix),
            &Self::loaded(&self.srs_summary_max_depth),
            &Self::loaded(&self.srs_summary_delimiter),
            links_to,
        );
    }
}

impl Def for SubRecordStructDef {
    record_member_plumbing!(Def);

    /// Port of `TwbSubRecordStructDef.CanAssign`.
    fn can_assign(&self, element: ElementArg, index: i32, def: Option<&dyn Def>) -> bool {
        if def_dont_assign(self) {
            return false;
        }
        let Some(other) = def.filter(|def| def.as_sub_record_struct_def().is_some()) else {
            return false;
        };
        if self.equals(def) {
            return true;
        }
        record_members_can_assign(self, other, element, index)
    }

    fn get_def_type(&self) -> DefType {
        DefType::dtSubRecordStruct
    }

    fn get_def_type_name(&self) -> String {
        "SubRecordStruct".to_owned()
    }

    fn as_sub_record_struct_def(&self) -> Option<&SubRecordStructDef> {
        Some(self)
    }

    fn as_record_def(&self) -> Option<&dyn RecordDef> {
        Some(self)
    }

    fn get_child_pos(&self, child: &dyn Def) -> i32 {
        self.srs_members
            .iter()
            .position(|member| child.equals(Some(member.as_dyn_def())))
            .map_or(-1, |index| index as i32)
    }

    fn init_from_parent_do_children(&self) {
        for member in &self.srs_members {
            member.init_from_parent();
        }
    }
}

impl NamedDef for SubRecordStructDef {
    record_member_plumbing!(NamedDef);

    fn after_load(&self, element: &ElementRef) {
        self.used(None, "");
        if let Some(after_load) = self.nd.nd_after_load.load().as_deref() {
            after_load(element);
        }
        if report_mode()
            && report_required()
            && let Some(container) = element.as_container()
        {
            for member in self.srs_members.iter().skip(1) {
                if member.is_not_required() {
                    continue;
                }
                let found = (0..container.get_element_count()).any(|j| {
                    container.get_element(j).is_some_and(|child| {
                        let def = child.get_def();
                        let value_def = child.get_value_def();
                        member.equals(def.as_deref().map(|def| def.as_dyn_def()))
                            || member.equals(value_def.as_deref().map(|def| def.as_dyn_def()))
                    })
                });
                member.possibly_required();
                if !found {
                    member.not_required();
                }
            }
        }
    }
}

impl SignatureDef for SubRecordStructDef {
    fn get_default_signature(&self) -> Signature {
        self.srs_members[0].get_default_signature()
    }

    fn get_signature(&self, index: i32) -> Signature {
        if self.any_member() {
            *self
                .srs_signatures
                .keys()
                .nth(index as usize)
                .expect("the index of a signature")
        } else {
            self.srs_members[0].get_signature(index)
        }
    }

    fn get_signature_count(&self) -> i32 {
        if self.any_member() {
            self.srs_signatures.len() as i32
        } else {
            self.srs_members[0].get_signature_count()
        }
    }

    fn can_handle(&self, container: ElementArg, signature: Signature, data_container: ElementArg) -> bool {
        if self.any_member() {
            self.contains_member_for(container, signature, data_container)
        } else {
            self.srs_members[0].can_handle(container, signature, data_container)
        }
    }
}

impl RecordMemberDef for SubRecordStructDef {
    fn to_summary(&self, depth: i32, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        record_member_def_to_summary(self, depth, element, links_to)
    }

    fn to_summary_internal(&self, depth: i32, element: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        let mut result = String::new();
        if let Some(key) = &self.sort_key
            && !self.def.def_flags.contains(DefFlag::dfSummaryNoSortKey)
        {
            self.keys_to_summary(&key.srs_sort_key, depth, &mut result, element, links_to);
            self.keys_to_summary(&key.srs_ex_sort_key, depth, &mut result, element, links_to);
        }
        let mut own = String::new();
        self.keys_to_summary(&Self::loaded(&self.srs_summary_key), depth, &mut own, element, links_to);
        if self.sort_key.is_none() {
            return own;
        }
        if !own.is_empty() {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(&own);
        }
        result
    }
}

impl RecordDef for SubRecordStructDef {
    fn contains_member_for(&self, _container: ElementArg, signature: Signature, _data_container: ElementArg) -> bool {
        self.srs_signatures.contains_key(&signature)
    }

    fn get_member_for(
        &self,
        _container: ElementArg,
        signature: Signature,
        _data_container: ElementArg,
    ) -> Option<Arc<dyn RecordMemberDef>> {
        self.srs_signatures
            .get(&signature)
            .map(|&index| self.srs_members[index].clone())
    }

    fn get_member_index_for(&self, _container: ElementArg, signature: Signature, _data_container: ElementArg) -> i32 {
        self.srs_signatures.get(&signature).map_or(-1, |&index| index as i32)
    }

    fn allow_unordered(&self) -> bool {
        self.srs_allow_unordered
    }

    fn get_member(&self, index: usize) -> Arc<dyn RecordMemberDef> {
        self.srs_members[index].clone()
    }

    fn get_member_count(&self) -> i32 {
        self.srs_members.len() as i32
    }

    fn get_skip_signature(&self, signature: Signature) -> bool {
        self.srs_skip_signatures.contains(&signature)
    }
}

impl DefKind for SubRecordStructDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

/// Upstream `TwbSubRecordUnionDef`: one of several record members.
pub struct SubRecordUnionDef {
    self_ref: Weak<SubRecordUnionDef>,
    def: DefBase,
    nd: NamedDefBase,
    sru_members: Vec<Arc<dyn RecordMemberDef>>,
    sru_skip_signatures: BTreeSet<Signature>,
    sru_decider: Option<RUnionDecider>,
}

impl SubRecordUnionDef {
    /// Port of `TwbSubRecordUnionDef.Create`. The `after_load`, `after_set`
    /// and `terminator` arguments are not used, as upstream.
    pub fn create(
        args: NamedDefArgs,
        members: Vec<Arc<dyn RecordMemberDef>>,
        skip_sigs: &[Signature],
        decider: Option<RUnionDecider>,
    ) -> Arc<Self> {
        let (def, nd) = NamedDefBase::create(NamedDefArgs {
            after_load: None,
            after_set: None,
            terminator: false,
            ..args
        });
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| {
            let parent: Weak<dyn Def> = self_ref.clone();
            Self {
                self_ref: self_ref.clone(),
                def,
                nd,
                sru_members: members
                    .into_iter()
                    .map(|member| set_parent(member, &parent, false))
                    .collect(),
                sru_skip_signatures: skip_sigs.iter().copied().collect(),
                sru_decider: decider,
            }
        });
        DefBase::after_construction(&*this);
        this
    }

    pub fn clone_from(source: &Self) -> Arc<Self> {
        let skip_sigs: Vec<Signature> = source.sru_skip_signatures.iter().copied().collect();
        let this = Self::create(
            NamedDefBase::clone_args(source),
            source.sru_members.clone(),
            &skip_sigs,
            source.sru_decider.clone(),
        );
        NamedDefBase::after_clone(&*this, source);
        this
    }

    /// The index of the member that handles the subrecord: the decided member
    /// when there is a decider, else the first member that can handle it.
    fn handling_member(
        &self,
        container: ElementArg,
        signature: Signature,
        data_container: ElementArg,
    ) -> Option<usize> {
        match &self.sru_decider {
            Some(decider) => usize::try_from(decider(container)).ok().filter(|&index| {
                self.sru_members
                    .get(index)
                    .is_some_and(|member| member.can_handle(container, signature, data_container))
            }),
            None => self
                .sru_members
                .iter()
                .position(|member| member.can_handle(container, signature, data_container)),
        }
    }
}

impl Def for SubRecordUnionDef {
    record_member_plumbing!(Def);

    /// Port of `TwbSubRecordUnionDef.CanAssign`: a member that can take the
    /// definition, or a union of members that can take each other.
    fn can_assign(&self, element: ElementArg, index: i32, def: Option<&dyn Def>) -> bool {
        if def_dont_assign(self) {
            return false;
        }
        let count = usize::try_from(self.get_member_count()).unwrap_or(0);
        if (0..count).any(|i| self.get_member(i).can_assign(element, index, def)) {
            return true;
        }
        let Some(other) = def.filter(|def| def.as_sub_record_union_def().is_some()) else {
            return false;
        };
        if self.equals(def) {
            return true;
        }
        record_members_can_assign(self, other, element, index)
    }

    fn get_def_type(&self) -> DefType {
        DefType::dtSubRecordUnion
    }

    fn get_def_type_name(&self) -> String {
        "SubRecordUnion".to_owned()
    }

    fn as_sub_record_union_def(&self) -> Option<&SubRecordUnionDef> {
        Some(self)
    }

    fn as_record_def(&self) -> Option<&dyn RecordDef> {
        Some(self)
    }

    fn get_child_pos(&self, child: &dyn Def) -> i32 {
        self.sru_members
            .iter()
            .position(|member| child.equals(Some(member.as_dyn_def())))
            .map_or(-1, |index| index as i32)
    }

    fn init_from_parent_do_children(&self) {
        for member in &self.sru_members {
            member.init_from_parent();
        }
    }
}

impl NamedDef for SubRecordUnionDef {
    record_member_plumbing!(NamedDef);
}

impl SignatureDef for SubRecordUnionDef {
    fn get_default_signature(&self) -> Signature {
        self.sru_members[0].get_default_signature()
    }

    /// The signatures of all members, one member after the other.
    fn get_signature(&self, index: i32) -> Signature {
        let mut remaining = index;
        for member in &self.sru_members {
            let count = member.get_signature_count();
            if remaining >= count {
                remaining -= count;
            } else {
                return member.get_signature(remaining);
            }
        }
        panic!("Invalid index");
    }

    fn get_signature_count(&self) -> i32 {
        self.sru_members.iter().map(|member| member.get_signature_count()).sum()
    }

    fn can_handle(&self, container: ElementArg, signature: Signature, data_container: ElementArg) -> bool {
        self.handling_member(container, signature, data_container).is_some()
    }
}

impl RecordMemberDef for SubRecordUnionDef {}

impl RecordDef for SubRecordUnionDef {
    fn contains_member_for(&self, container: ElementArg, signature: Signature, data_container: ElementArg) -> bool {
        self.can_handle(container, signature, data_container)
    }

    fn get_member_for(
        &self,
        container: ElementArg,
        signature: Signature,
        data_container: ElementArg,
    ) -> Option<Arc<dyn RecordMemberDef>> {
        self.handling_member(container, signature, data_container)
            .map(|index| self.sru_members[index].clone())
    }

    fn get_member_index_for(&self, container: ElementArg, signature: Signature, data_container: ElementArg) -> i32 {
        self.handling_member(container, signature, data_container)
            .map_or(-1, |index| index as i32)
    }

    fn allow_unordered(&self) -> bool {
        true
    }

    fn get_member(&self, index: usize) -> Arc<dyn RecordMemberDef> {
        self.sru_members[index].clone()
    }

    fn get_member_count(&self) -> i32 {
        self.sru_members.len() as i32
    }

    fn get_skip_signature(&self, signature: Signature) -> bool {
        self.sru_skip_signatures.contains(&signature)
    }
}

impl DefKind for SubRecordUnionDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::def::DefSetters;
    use super::super::globals::test_lock;
    use super::super::sub_record::SubRecordDef;
    use super::*;

    const MODL: Signature = Signature::new(b"MODL");
    const MODT: Signature = Signature::new(b"MODT");
    const MODS: Signature = Signature::new(b"MODS");
    const XXXX: Signature = Signature::new(b"XXXX");

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

    fn sub(signature: Signature, name: &str) -> Arc<dyn RecordMemberDef> {
        SubRecordDef::create(args(name), &[signature], None, false)
    }

    fn model() -> Arc<SubRecordStructDef> {
        SubRecordStructDef::create(
            args("Model"),
            vec![
                sub(MODL, "Model FileName"),
                sub(MODT, "Model Information"),
                sub(MODS, "Material Swap"),
            ],
            &[XXXX],
            false,
        )
    }

    #[test]
    fn subrecord_struct() {
        let _guard = test_lock();
        let def = model();
        assert_eq!(def.get_default_signature(), MODL);
        assert_eq!(def.get_signature_count(), 1);
        assert_eq!(def.get_full_name(), "MODL - Model");
        assert!(def.can_handle(None, MODL, None));
        // A struct starts with its first member.
        assert!(!def.can_handle(None, MODT, None));
        assert!(def.contains_member_for(None, MODT, None));
        assert_eq!(def.get_member_index_for(None, MODS, None), 2);
        assert_eq!(def.get_member_index_for(None, XXXX, None), -1);
        assert_eq!(
            def.get_member_for(None, MODT, None).unwrap().get_name(),
            "Model Information"
        );
        assert!(def.get_skip_signature(XXXX) && !def.get_skip_signature(MODL));
        assert_eq!(def.get_member_count(), 3);
        assert_eq!(def.get_child_pos(def.get_member(1).as_dyn_def()), 1);
        assert_eq!(
            def.get_member(1).get_full_path(),
            "MODL - Model \\ [1] MODT - Model Information"
        );
        assert!(!def.has_sort_key() && !def.is_in_sk(0));
        assert_eq!(def.to_summary(0, None, &mut None), "");

        let unordered = model().include_flag(DefFlag::dfAllowAnyMember);
        assert!(unordered.can_handle(None, MODT, None));
        assert_eq!(unordered.get_signature_count(), 3);
        // The signatures of an unordered struct are in sorted order.
        assert_eq!(unordered.get_signature(0), MODL);
        assert_eq!(unordered.get_signature(1), MODS);
    }

    #[test]
    fn subrecord_struct_sort_key_and_clone() {
        let _guard = test_lock();
        let def = SubRecordStructDef::create_sk(
            args("Item"),
            vec![sub(MODL, "A"), sub(MODT, "B"), sub(MODS, "C")],
            &[],
            &[1],
            &[2],
            true,
        )
        .set_summary_key(&[0])
        .set_summary_member_prefix_suffix(1, "<", ">")
        .set_summary_member_max_depth(0, 2)
        .set_summary_delimiter(", ");
        assert!(def.has_sort_key() && def.allow_unordered());
        assert_eq!(def.get_sort_key_count(false), 1);
        assert_eq!(def.get_sort_key_count(true), 2);
        assert_eq!(def.get_sort_key(0, true), 1);
        assert_eq!(def.get_sort_key(1, true), 2);
        assert!(def.is_in_sk(1) && !def.is_in_sk(2) && !def.is_in_sk(9));
        let copy = SubRecordStructDef::clone_from(&def);
        assert!(copy.has_sort_key());
        assert_eq!(SubRecordStructDef::loaded(&copy.srs_summary_prefix), ["", "<"]);
        assert_eq!(SubRecordStructDef::loaded(&copy.srs_summary_delimiter), ", ");
        assert!(!Arc::ptr_eq(&copy.get_member(0), &def.get_member(0)));
    }

    #[test]
    fn subrecord_array() {
        let _guard = test_lock();
        let models: Arc<dyn RecordMemberDef> = model();
        let def = SubRecordArrayDef::create(args("Models"), models, 0, true, None);
        assert_eq!(def.get_default_signature(), MODL);
        assert!(def.can_handle(None, MODL, None) && !def.can_handle(None, MODT, None));
        assert_eq!(def.get_def_type_name(), "SubRecordArray of SubRecordStruct");
        assert_eq!(def.get_full_name(), "MODL - Models");
        assert!(def.get_sorted(None));
        assert_eq!(def.get_element().get_path(), "Models \\ Model");
        let with_paths = def.clone().set_count_paths(&["", "Count"]);
        assert!(Arc::ptr_eq(&with_paths, &def));
        assert_eq!(def.get_count_paths(), ["Count"]);
        let decided: IsSortedCallback = Arc::new(|container| container.is_some());
        let unsorted = SubRecordArrayDef::create(args("Models"), sub(MODL, "Model"), 0, true, Some(decided));
        assert!(!unsorted.get_sorted(None));
        assert_eq!(SubRecordArrayDef::clone_from(&def).get_count_paths(), ["Count"]);
    }

    #[test]
    fn subrecord_union() {
        let _guard = test_lock();
        let def = SubRecordUnionDef::create(
            args("Variant"),
            vec![sub(MODL, "File"), sub(MODT, "Info")],
            &[XXXX],
            None,
        );
        assert_eq!(def.get_default_signature(), MODL);
        assert_eq!(def.get_signature_count(), 2);
        assert_eq!(def.get_signature(1), MODT);
        assert!(def.can_handle(None, MODT, None) && !def.can_handle(None, MODS, None));
        assert_eq!(def.get_member_index_for(None, MODT, None), 1);
        assert_eq!(def.get_member_for(None, MODL, None).unwrap().get_name(), "File");
        assert!(def.allow_unordered() && def.get_skip_signature(XXXX));
        assert_eq!(def.get_full_name(), "MODL - Variant");

        // With a decider only the decided member counts.
        let decider: RUnionDecider = Arc::new(|_| 0);
        let decided = SubRecordUnionDef::create(
            args("Variant"),
            vec![sub(MODL, "File"), sub(MODT, "Info")],
            &[],
            Some(decider),
        );
        assert!(decided.can_handle(None, MODL, None) && !decided.can_handle(None, MODT, None));
        assert_eq!(SubRecordUnionDef::clone_from(&decided).get_member_count(), 2);
    }
}
