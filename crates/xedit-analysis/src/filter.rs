// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (ReInitTree, vstNavInitNode,
// vstNavInitChildren, mniNavFilterApplyClick, mniNavFilterForCleaningClick,
// InheritStateFromChildren)

//! The navigation tree of the main form (`vstNav`) and its filter.
//!
//! The cleaning functions of the GUI work on the navigation tree after a
//! filter was applied: the filter computes the conflict status of every
//! record of the tree, gives each node with children the highest status of
//! its children (`InheritStateFromChildren`) and removes the nodes it
//! filters out, and the cleaning walks what is left from the last node.
//! [`NavTree`] is that tree, built at once (the GUI builds it as it is
//! walked, which the filter does completely), and [`apply_filter`] is
//! `mniNavFilterApplyClick` with the options of [`FilterOptions`].
//!
//! Ported so far: the tree, the preset of `mniNavFilterForCleaningClick`
//! ([`FilterOptions::for_cleaning`]) and the conflict status filters
//! (`FilterConflictAll`, `FilterConflictThis` with their sets, the
//! "conflict status inherited by parent" option). Phase 4 step 4 adds the
//! other options of `TfrmFilterOptions` (status, names, signatures,
//! persistence, flattening and the rest) to [`FilterOptions`] and to
//! `check_filter_node`; [`apply_filter`] refuses a filter it can not apply.
//! The node flags of the references (`nnfNotReachable`,
//! `nnfReferencesInjected`) need the reference index and are not set yet.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;

use xedit_core::implementation::{ElementImpl, FileImpl, MainRecordImpl};
use xedit_core::interface::constructors::find_record_def;
use xedit_core::interface::globals::{sort_info, vwd_as_quest_children};
use xedit_core::interface::types::{ConflictAll, ConflictThis, ElementType, PascalEnum, Signature};
use xedit_core::interface::{Element, ElementRef, MainRecord, NamedDef};

use crate::conflict::{ConflictOptions, ConflictResults, conflict_statuses_of};

/// A node of a [`NavTree`].
pub type NodeId = usize;

/// Port of `TNavNodeFlag` as bits of [`NavNodeData::flags`].
pub mod nav_node_flag {
    /// `nnfInjected`.
    pub const INJECTED: u8 = 1;
    /// `nnfNotReachable`.
    pub const NOT_REACHABLE: u8 = 1 << 1;
    /// `nnfReferencesInjected`.
    pub const REFERENCES_INJECTED: u8 = 1 << 2;
    /// `nnfFilterChecked`.
    pub const FILTER_CHECKED: u8 = 1 << 3;
}

/// Port of `TNavNodeData`.
#[derive(Clone)]
pub struct NavNodeData {
    /// The element of the node: a file, a group or a main record. `None`
    /// for the node of a child group that its record shows (the GUI hides
    /// it, the filter deletes it).
    pub element: Option<ElementRef>,
    /// The container whose elements are the children of the node: the
    /// element itself, or the child group of a main record.
    pub container: Option<ElementRef>,
    pub conflict_all: ConflictAll,
    pub conflict_this: ConflictThis,
    pub org_conflict_all: ConflictAll,
    pub org_conflict_this: ConflictThis,
    pub flags: u8,
}

struct Node {
    data: NavNodeData,
    parent: Option<NodeId>,
    first_child: Option<NodeId>,
    last_child: Option<NodeId>,
    prev_sibling: Option<NodeId>,
    next_sibling: Option<NodeId>,
    child_count: usize,
    deleted: bool,
}

/// Port of the navigation tree (`vstNav`): one root node per file, the
/// elements of a container as the children of its node, a child group
/// shown below the record it belongs to.
pub struct NavTree {
    nodes: Vec<Node>,
    first_root: Option<NodeId>,
    last_root: Option<NodeId>,
    /// The filter last applied (`FilterApplied` with the filter variables).
    applied: Option<FilterOptions>,
}

/// Upstream `ParentedGroupRecordType`: the groups that show below their
/// record.
fn is_parented_group_type(group_type: i32) -> bool {
    matches!(group_type, 1 | 6 | 7) || (group_type == 10 && vwd_as_quest_children())
}

fn group_of(element: &ElementRef) -> Option<Arc<xedit_core::implementation::GroupRecordImpl>> {
    element.as_element_impl().and_then(ElementImpl::group_record_impl)
}

fn record_of(element: &ElementRef) -> Option<Arc<MainRecordImpl>> {
    element.as_element_impl().and_then(ElementImpl::main_record_impl)
}

/// `(Element as IwbMainRecord).FormID.ToCardinal = GroupRecord.GroupLabel`:
/// the group holds the children of the record before it.
fn holds_children_of(record: &MainRecordImpl, group: &xedit_core::implementation::GroupRecordImpl) -> bool {
    is_parented_group_type(group.group_type()) && record.mr_struct().form_id.to_cardinal() == group.group_label()
}

fn elements_of(container: &ElementRef) -> Vec<ElementRef> {
    let Some(container) = container.as_container() else {
        return Vec::new();
    };
    (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .collect()
}

impl NavTree {
    /// Port of `ReInitTree` with the nodes initialised as the filter walks
    /// them (`vstNavInitNode`, `vstNavInitChildren`): the files in the order
    /// given.
    pub fn new(files: &[Arc<FileImpl>]) -> Self {
        let mut tree = NavTree {
            nodes: Vec::new(),
            first_root: None,
            last_root: None,
            applied: None,
        };
        for file in files {
            let element: ElementRef = file.clone();
            let container = (!elements_of(&element).is_empty()).then(|| element.clone());
            let id = tree.add_node(None, Some(element), container);
            tree.init_children(id);
        }
        tree
    }

    fn add_node(
        &mut self,
        parent: Option<NodeId>,
        element: Option<ElementRef>,
        container: Option<ElementRef>,
    ) -> NodeId {
        let id = self.nodes.len();
        let prev = match parent {
            Some(parent) => self.nodes[parent].last_child,
            None => self.last_root,
        };
        self.nodes.push(Node {
            data: NavNodeData {
                element,
                container,
                conflict_all: ConflictAll::caUnknown,
                conflict_this: ConflictThis::ctUnknown,
                org_conflict_all: ConflictAll::caUnknown,
                org_conflict_this: ConflictThis::ctUnknown,
                flags: 0,
            },
            parent,
            first_child: None,
            last_child: None,
            prev_sibling: prev,
            next_sibling: None,
            child_count: 0,
            deleted: false,
        });
        if let Some(prev) = prev {
            self.nodes[prev].next_sibling = Some(id);
        }
        match parent {
            Some(parent) => {
                let node = &mut self.nodes[parent];
                if node.first_child.is_none() {
                    node.first_child = Some(id);
                }
                node.last_child = Some(id);
                node.child_count += 1;
            }
            None => {
                if self.first_root.is_none() {
                    self.first_root = Some(id);
                }
                self.last_root = Some(id);
            }
        }
        id
    }

    /// Port of `vstNavInitChildren` and `vstNavInitNode` for the children of
    /// a node, recursively.
    fn init_children(&mut self, id: NodeId) {
        let Some(container) = self.nodes[id].data.container.clone() else {
            return;
        };
        let elements = elements_of(&container);
        let mut added = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            // A child group after its record shows below the record: its
            // own node is hidden.
            if index > 0
                && let Some(group) = group_of(element)
                && let Some(previous) = record_of(&elements[index - 1])
                && holds_children_of(&previous, &group)
            {
                added.push(self.add_node(Some(id), None, None));
                continue;
            }
            let container = if element.get_element_type() == ElementType::etMainRecord {
                let record = record_of(element);
                elements
                    .get(index + 1)
                    .and_then(|next| {
                        let group = group_of(next)?;
                        let record = record.as_ref()?;
                        holds_children_of(record, &group).then(|| next.clone())
                    })
                    .filter(|group| !elements_of(group).is_empty())
            } else {
                (!elements_of(element).is_empty()).then(|| element.clone())
            };
            added.push(self.add_node(Some(id), Some(element.clone()), container));
        }
        for child in added {
            self.init_children(child);
        }
    }

    pub fn data(&self, id: NodeId) -> &NavNodeData {
        &self.nodes[id].data
    }

    pub fn data_mut(&mut self, id: NodeId) -> &mut NavNodeData {
        &mut self.nodes[id].data
    }

    /// `Node.ChildCount`.
    pub fn child_count(&self, id: NodeId) -> usize {
        self.nodes[id].child_count
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].parent
    }

    /// The root nodes (the files) in order.
    pub fn roots(&self) -> Vec<NodeId> {
        self.siblings_from(self.first_root)
    }

    /// The children of a node in order.
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        self.siblings_from(self.nodes[id].first_child)
    }

    fn siblings_from(&self, first: Option<NodeId>) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut current = first;
        while let Some(id) = current {
            result.push(id);
            current = self.nodes[id].next_sibling;
        }
        result
    }

    /// The root node of a file.
    pub fn file_node(&self, file: &FileImpl) -> Option<NodeId> {
        self.roots().into_iter().find(|&id| {
            self.nodes[id]
                .data
                .element
                .as_ref()
                .is_some_and(|element| element.get_element_id() == file.get_element_id())
        })
    }

    /// `GetLast(Node)`: the last node of the subtree of `start`, or of the
    /// tree for `None`.
    pub fn get_last(&self, start: Option<NodeId>) -> Option<NodeId> {
        let mut current = match start {
            Some(start) => start,
            None => self.last_root?,
        };
        while let Some(last) = self.nodes[current].last_child {
            current = last;
        }
        Some(current)
    }

    /// `GetPrevious(Node)`: the node before `id` in the order of the tree
    /// (a node before its children).
    pub fn get_previous(&self, id: NodeId) -> Option<NodeId> {
        match self.nodes[id].prev_sibling {
            Some(prev) => self.get_last(Some(prev)),
            None => self.nodes[id].parent,
        }
    }

    /// `GetNext(Node)`: the node after `id` in the order of the tree.
    pub fn get_next(&self, id: NodeId) -> Option<NodeId> {
        if let Some(first) = self.nodes[id].first_child {
            return Some(first);
        }
        let mut current = Some(id);
        while let Some(node) = current {
            if let Some(next) = self.nodes[node].next_sibling {
                return Some(next);
            }
            current = self.nodes[node].parent;
        }
        None
    }

    /// `DeleteNode`: the node and its subtree leave the tree.
    pub fn delete_node(&mut self, id: NodeId) {
        let (parent, prev, next) = {
            let node = &self.nodes[id];
            (node.parent, node.prev_sibling, node.next_sibling)
        };
        match prev {
            Some(prev) => self.nodes[prev].next_sibling = next,
            None => match parent {
                Some(parent) => self.nodes[parent].first_child = next,
                None => self.first_root = next,
            },
        }
        match next {
            Some(next) => self.nodes[next].prev_sibling = prev,
            None => match parent {
                Some(parent) => self.nodes[parent].last_child = prev,
                None => self.last_root = prev,
            },
        }
        if let Some(parent) = parent {
            self.nodes[parent].child_count -= 1;
        }
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            stack.extend(self.siblings_from(self.nodes[current].first_child));
            let node = &mut self.nodes[current];
            node.deleted = true;
            node.data.element = None;
            node.data.container = None;
        }
        let node = &mut self.nodes[id];
        node.prev_sibling = None;
        node.next_sibling = None;
    }

    /// Whether the node was deleted.
    pub fn is_deleted(&self, id: NodeId) -> bool {
        self.nodes[id].deleted
    }

    /// Port of `vstNav.Clear`.
    pub fn clear(&mut self) {
        for id in self.roots() {
            self.delete_node(id);
        }
    }

    /// `Sort(Node)` of the tree with `toAutoSort`, which runs when a node
    /// is expanded: the children of `id` in the order of
    /// [`compare_nodes`].
    pub fn sort_children(&mut self, id: NodeId) {
        let mut children = self.children(id);
        if children.len() < 2 {
            return;
        }
        children.sort_by(|&a, &b| compare_nodes(&self.nodes[a].data, &self.nodes[b].data));
        let mut prev: Option<NodeId> = None;
        for &child in &children {
            self.nodes[child].prev_sibling = prev;
            self.nodes[child].next_sibling = None;
            if let Some(prev) = prev {
                self.nodes[prev].next_sibling = Some(child);
            }
            prev = Some(child);
        }
        self.nodes[id].first_child = children.first().copied();
        self.nodes[id].last_child = children.last().copied();
    }

    /// The filter that was applied last (`FilterApplied`).
    pub fn applied_filter(&self) -> Option<&FilterOptions> {
        self.applied.as_ref()
    }

    /// Port of `InheritStateFromChildren`: the node takes the highest
    /// status and the flags of its children.
    pub fn inherit_state_from_children(&mut self, id: NodeId) {
        for child in self.children(id) {
            let child = self.nodes[child].data.clone();
            let data = &mut self.nodes[id].data;
            if child.conflict_all > data.conflict_all {
                data.conflict_all = child.conflict_all;
            }
            if child.conflict_this > data.conflict_this {
                data.conflict_this = child.conflict_this;
            }
            data.flags |= child.flags
                & (nav_node_flag::INJECTED | nav_node_flag::NOT_REACHABLE | nav_node_flag::REFERENCES_INJECTED);
        }
    }
}

/// The filter variables of the main form that [`apply_filter`] reads
/// (`FilterConflictAll`, `FilterConflictAllSet` and the rest). Phase 4
/// step 4 adds the other options of `TfrmFilterOptions`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FilterOptions {
    /// `FilterConflictAll` with `FilterConflictAllSet`: keep the records
    /// whose `ConflictAll` is in the set.
    pub conflict_all: Option<Vec<ConflictAll>>,
    /// `FilterConflictThis` with `FilterConflictThisSet`.
    pub conflict_this: Option<Vec<ConflictThis>>,
    /// `InheritConflictByParent`: a node with children takes the highest
    /// status of its children.
    pub inherit_conflict_by_parent: bool,
    /// `FlattenBlocks`, `FlattenCellChilds`, `AssignPersWrldChild`: not
    /// ported yet (phase 4 step 4).
    pub flatten_blocks: bool,
    pub flatten_cell_childs: bool,
    pub assign_pers_wrld_child: bool,
    /// `ModGroupsEnabled`, `OnlyShowMasterAndLeafs` and
    /// `xeQuickShowConflicts`: the conflict options the cleaning functions
    /// refuse.
    pub mod_groups_enabled: bool,
    pub only_master_and_leafs: bool,
    pub quick_show_conflicts: bool,
}

impl FilterOptions {
    /// Port of `mniNavFilterForCleaningClick`: no filter but the conflict
    /// status inherited by parent, with mod groups, "Only Show Master and
    /// Leafs" and the quick show conflicts mode off.
    pub fn for_cleaning() -> Self {
        FilterOptions {
            inherit_conflict_by_parent: true,
            ..Default::default()
        }
    }

    /// The check of `mniNavRemoveIdenticalToMasterClick` and
    /// `mniNavUndeleteAndDisableReferencesClick`: only "conflict status
    /// inherited by parent" may be active.
    pub fn is_cleaning_filter(&self) -> bool {
        self.conflict_all.is_none()
            && self.conflict_this.is_none()
            && !self.flatten_blocks
            && !self.flatten_cell_childs
            && !self.assign_pers_wrld_child
            && !self.mod_groups_enabled
            && !self.only_master_and_leafs
            && !self.quick_show_conflicts
            && self.inherit_conflict_by_parent
    }

    fn conflict_options(&self) -> ConflictOptions {
        ConflictOptions {
            only_master_and_leafs: self.only_master_and_leafs,
            quick_show_conflicts: self.quick_show_conflicts,
            ..Default::default()
        }
    }
}

/// What [`apply_filter`] did, as the GUI reports it ("[Pass 1] Processed
/// Records: ..., [Pass 2] Processed Records: ..., Remaining unfiltered
/// nodes: ...").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterReport {
    /// Nodes the first pass walked.
    pub pass1: u64,
    /// Nodes the second pass walked.
    pub pass2: u64,
    /// Nodes left.
    pub unfiltered: u64,
    /// The warnings of the conflict detection.
    pub messages: Vec<String>,
}

/// Port of `mniNavFilterApplyClick` with `FilterPreset` (the options are
/// given, no dialog): the conflict status of every record in the tree, the
/// nodes filtered out removed from the tree, and every node with children
/// given the highest status of its children.
pub fn apply_filter(
    tree: &mut NavTree,
    options: &FilterOptions,
    files: &[Arc<FileImpl>],
) -> Result<FilterReport, String> {
    if options.flatten_blocks || options.flatten_cell_childs || options.assign_pers_wrld_child {
        return Err("the flattening options of the filter are not ported yet".to_owned());
    }
    if options.mod_groups_enabled {
        return Err("mod groups are not ported yet".to_owned());
    }
    let mut options = options.clone();
    if options
        .conflict_all
        .as_ref()
        .is_some_and(|set| ConflictAll::ALL.iter().all(|value| set.contains(value)))
    {
        options.conflict_all = None;
    }
    if options
        .conflict_this
        .as_ref()
        .is_some_and(|set| ConflictThis::ALL.iter().all(|value| set.contains(value)))
    {
        options.conflict_this = None;
    }
    let mut report = FilterReport::default();
    tree.applied = None;
    if options.conflict_all.as_ref().is_some_and(Vec::is_empty)
        || options.conflict_this.as_ref().is_some_and(Vec::is_empty)
    {
        tree.clear();
        tree.applied = Some(options);
        return Ok(report);
    }
    let check_conflict = options.conflict_all.is_some() || options.conflict_this.is_some();

    // The statuses the first pass reads, asked in the order it walks.
    let results = if check_conflict || options.inherit_conflict_by_parent {
        let mut records = Vec::new();
        let mut current = tree.get_last(None);
        while let Some(id) = current {
            if let Some(record) = tree.data(id).element.as_ref().and_then(record_of) {
                records.push(record);
            }
            current = tree.get_previous(id);
        }
        conflict_statuses_of(&records, files, &options.conflict_options())
    } else {
        ConflictResults::default()
    };
    report.messages = results.messages.clone();

    let filtered_out = |data: &NavNodeData| {
        options
            .conflict_all
            .as_ref()
            .is_some_and(|set| !set.contains(&data.conflict_all))
            || options
                .conflict_this
                .as_ref()
                .is_some_and(|set| !set.contains(&data.conflict_this))
    };

    // Pass 1.
    let mut current = tree.get_last(None);
    while let Some(id) = current {
        report.pass1 += 1;
        let next = tree.get_previous(id);
        let Some(element) = tree.data(id).element.clone() else {
            tree.delete_node(id);
            current = next;
            continue;
        };
        if let Some(record) = record_of(&element) {
            let data = tree.data_mut(id);
            if record.is_injected() {
                data.flags |= nav_node_flag::INJECTED;
            }
            data.flags &= !nav_node_flag::FILTER_CHECKED;
            // `CheckFilterNode(False)`: a node without children is checked
            // once; the options ported so far filter by nothing else than
            // the conflict, which is checked below.
            if tree.child_count(id) == 0 {
                tree.data_mut(id).flags |= nav_node_flag::FILTER_CHECKED;
            }
            if check_conflict || options.inherit_conflict_by_parent {
                let status = results.status(&record);
                let data = tree.data_mut(id);
                data.conflict_all = status.all;
                data.conflict_this = status.this;
                data.org_conflict_all = status.all;
                data.org_conflict_this = status.this;
                if tree.child_count(id) == 0 && filtered_out(tree.data(id)) {
                    tree.delete_node(id);
                    current = next;
                    continue;
                }
            }
        }
        if tree.child_count(id) > 0 {
            if options.inherit_conflict_by_parent {
                tree.inherit_state_from_children(id);
            }
        } else if element.get_skipped() {
            tree.delete_node(id);
        }
        current = next;
    }

    // Pass 2: `CheckFilterNode(True)` on the nodes without children.
    let mut current = tree.get_last(None);
    while let Some(id) = current {
        let next = tree.get_previous(id);
        report.pass2 += 1;
        let mut deleted = false;
        if tree.child_count(id) == 0 && tree.data(id).flags & nav_node_flag::FILTER_CHECKED == 0 {
            tree.data_mut(id).flags |= nav_node_flag::FILTER_CHECKED;
            if check_conflict && filtered_out(tree.data(id)) {
                tree.delete_node(id);
                deleted = true;
            }
        }
        if !deleted {
            report.unfiltered += 1;
        }
        current = next;
    }
    tree.applied = Some(options);
    Ok(report)
}

/// The main records of the subtree of `start` that the tree still shows.
pub fn records_below(tree: &NavTree, start: NodeId) -> Vec<Arc<MainRecordImpl>> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![start];
    while let Some(id) = stack.pop() {
        if let Some(record) = tree.data(id).element.as_ref().and_then(record_of)
            && seen.insert(record.get_element_id())
        {
            result.push(record);
        }
        stack.extend(tree.children(id).into_iter().rev());
    }
    result
}

/// Delphi's `CompareText`: the texts compared with their ASCII letters in
/// upper case.
fn compare_text(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|byte| byte.to_ascii_uppercase())
        .cmp(b.bytes().map(|byte| byte.to_ascii_uppercase()))
}

/// Port of `FindSortElement`: a child group sorts like the record it holds
/// the children of.
fn sort_element(element: &ElementRef) -> ElementRef {
    if let Some(group) = group_of(element)
        && let Some(record) = group.children_of()
    {
        return record;
    }
    element.clone()
}

/// Port of `TfrmMain.vstNavCompareNodes` for the FormID column (the default
/// sort column of the navigation tree), with the files by load order, the
/// top level groups by the names of their records (`xeSortGroupsByFullName`)
/// and the responses of a topic by their previous response (the default
/// "INFO by Previous INFO"). Upstream breaks the last ties by the addresses
/// of the elements; the port by their identities.
pub fn compare_nodes(data1: &NavNodeData, data2: &NavNodeData) -> Ordering {
    let (element1, element2) = match (&data1.element, &data2.element) {
        (None, None) => return Ordering::Equal,
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        (Some(a), Some(b)) => (a, b),
    };
    if element1.get_element_id() == element2.get_element_id() {
        return Ordering::Equal;
    }
    let sort1 = sort_element(element1);
    let sort2 = sort_element(element2);
    let mut result = sort1.get_element_type().ord().cmp(&sort2.get_element_type().ord());
    if result == Ordering::Equal {
        match sort1.get_element_type() {
            ElementType::etFile => {
                let order = |element: &ElementRef| {
                    element
                        .as_element_impl()
                        .and_then(ElementImpl::file_impl)
                        .map_or(0, |file| file.load_order())
                };
                return order(&sort1).cmp(&order(&sort2));
            }
            ElementType::etGroupRecord => {
                if let (Some(group1), Some(group2)) = (group_of(&sort1), group_of(&sort2)) {
                    result = (group1.group_type() as u32).cmp(&(group2.group_type() as u32));
                    if result == Ordering::Equal {
                        let (label1, label2) = (group1.group_label(), group2.group_label());
                        result = match group1.group_type() {
                            0 => {
                                let name = |label: u32| {
                                    let signature = Signature::new(&label.to_le_bytes());
                                    find_record_def(signature)
                                        .map_or_else(|| signature.to_string(), |def| def.get_name().to_owned())
                                };
                                compare_text(&name(label1), &name(label2))
                            }
                            2 | 3 => (label1 as i32).cmp(&(label2 as i32)),
                            4 | 5 => {
                                let hi = |label: u32| (label >> 16) as i16;
                                let lo = |label: u32| label as i16;
                                hi(label1).cmp(&hi(label2)).then(lo(label1).cmp(&lo(label2)))
                            }
                            _ => label1.cmp(&label2),
                        };
                    }
                }
            }
            ElementType::etMainRecord => {
                if let (Some(record1), Some(record2)) = (record_of(&sort1), record_of(&sort2)) {
                    result = sort_priority(&record1).cmp(&sort_priority(&record2));
                    let info = Signature::new(b"INFO");
                    if sort_info() && record1.get_signature() == info && record2.get_signature() == info {
                        if let Some(group) = record1
                            .get_container()
                            .and_then(|container| container.as_element_impl()?.group_record_impl())
                        {
                            group.sort_responses();
                        }
                        result = (record1.get_sort_order() as u32).cmp(&(record2.get_sort_order() as u32));
                    }
                    if result == Ordering::Equal {
                        result = record1
                            .get_load_order_form_id()
                            .to_cardinal()
                            .cmp(&record2.get_load_order_form_id().to_cardinal())
                            .then(record1.get_element_id().cmp(&record2.get_element_id()));
                    }
                }
            }
            _ => {
                return sort1
                    .get_sort_order()
                    .cmp(&sort2.get_sort_order())
                    .then(element1.get_element_id().cmp(&element2.get_element_id()));
            }
        }
    }
    if result != Ordering::Equal {
        return result;
    }
    let grouped1 = element1.get_element_id() != sort1.get_element_id();
    let grouped2 = element2.get_element_id() != sort2.get_element_id();
    match (grouped1, grouped2) {
        // Both are groups of the same record.
        (true, true) => match (group_of(element1), group_of(element2)) {
            (Some(group1), Some(group2)) => group1
                .group_type()
                .cmp(&group2.group_type())
                .then(group1.group_label().cmp(&group2.group_label())),
            _ => Ordering::Equal,
        },
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => Ordering::Equal,
    }
}

/// Port of `TwbMainRecord.GetSortPriority`.
fn sort_priority(record: &MainRecordImpl) -> i32 {
    match &record.get_signature().0 {
        b"ROAD" | b"LAND" => -2,
        b"CELL" | b"PGRD" | b"NAVM" => -1,
        _ => 0,
    }
}
