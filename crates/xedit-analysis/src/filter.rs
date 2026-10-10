// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (ReInitTree, vstNavInitNode,
// vstNavInitChildren, mniNavFilterApplyClick, mniNavFilterConflictsClick,
// mniNavFilterForCleaningClick, mniNavFilterForOnlyOneClick,
// mniNavFilterRemoveClick, InheritStateFromChildren, CheckFilterNode,
// CheckContainerForElementValue, CheckValueRegex, IsUnnecessaryPersistent,
// IsMasterTemporary, IsPositionChanged)

//! The navigation tree of the main form (`vstNav`) and its filter.
//!
//! The cleaning functions of the GUI work on the navigation tree after a
//! filter was applied: the filter computes the conflict status of every
//! record of the tree, gives each node with children the highest status of
//! its children (`InheritStateFromChildren`) and removes the nodes it
//! filters out, and the cleaning walks what is left from the last node.
//! [`NavTree`] is that tree, built at once (the GUI builds it as it is
//! walked, which the filter does completely), and [`apply_filter`] is
//! `mniNavFilterApplyClick` with the options of [`FilterOptions`], every
//! option of the filter dialog (`TfrmFilterOptions`) among them: the
//! conflict status, the inject, not-reachable and references-injected
//! states, the editor ID, name, element value (regular expressions among
//! them), base editor ID, base name, scaled actors, signatures and base
//! signatures, the persistence options, the deleted and visible-when-
//! distant flags, the precombined mesh, the flattening of blocks and cell
//! children, the assignment of the persistent worldspace children, and the
//! conflict status inherited by parent.
//!
//! What the port reads from the session rather than from the dialog:
//! `ReachableBuild` ([`FilterOptions::reachable_build`], which
//! `refs.build_reachable` sets) and the mod groups
//! ([`FilterOptions::mod_groups_enabled`] with the filter the request
//! activates). `FilterScripted` (the `Filter` function of a script), the
//! records the user hid (`IsHidden`) and the compare-to load have no state
//! in the port.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use xedit_core::implementation::form_ids::position_to_grid_cell;
use xedit_core::implementation::{ElementImpl, FileImpl, MainRecordImpl};
use xedit_core::interface::constructors::find_record_def;
use xedit_core::interface::form_id::FormID;
use xedit_core::interface::globals::{
    is_fallout4, is_fallout76, is_morrowind, is_oblivion, is_skyrim, is_starfield, sort_info, vwd_as_quest_children,
};
use xedit_core::interface::main_record::MainRecordDef;
use xedit_core::interface::misc::Variant;
use xedit_core::interface::types::{ConflictAll, ConflictThis, ElementType, PascalEnum, Signature};
use xedit_core::interface::{Container, Element, ElementRef, File, MainRecord, NamedDef};
use xedit_loadorder::ini_files::parse_comma_text;

use crate::conflict::{ConflictOptions, ConflictResults, ModGroupFilter, conflict_statuses_of};

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
/// the group holds the children of the record before it. The label is the
/// fixed-up one ([`xedit_core::implementation::GroupRecordImpl::get_group_label`]):
/// in Fallout 4 the label of the child groups of a record carries the
/// file's own FileID while the record holds the FormID of the masters-updated
/// space, so the raw label of those groups never matches the record.
fn holds_children_of(record: &MainRecordImpl, group: &xedit_core::implementation::GroupRecordImpl) -> bool {
    is_parented_group_type(group.group_type()) && record.mr_struct().form_id.to_cardinal() == group.get_group_label()
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

    /// `Node.LastChild`.
    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].last_child
    }

    /// `GetPreviousSibling`.
    pub fn previous_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].prev_sibling
    }

    /// `vstNav.MoveTo(aNode, aParent, amAddChildFirst, False)`: the node and
    /// its subtree become the first children of `parent`.
    pub fn move_to_child_first(&mut self, id: NodeId, parent: NodeId) {
        let (old_parent, prev, next) = {
            let node = &self.nodes[id];
            (node.parent, node.prev_sibling, node.next_sibling)
        };
        // Detach.
        match prev {
            Some(prev) => self.nodes[prev].next_sibling = next,
            None => match old_parent {
                Some(old_parent) => self.nodes[old_parent].first_child = next,
                None => self.first_root = next,
            },
        }
        match next {
            Some(next) => self.nodes[next].prev_sibling = prev,
            None => match old_parent {
                Some(old_parent) => self.nodes[old_parent].last_child = prev,
                None => self.last_root = prev,
            },
        }
        if let Some(old_parent) = old_parent {
            self.nodes[old_parent].child_count -= 1;
        }
        // Attach before the first child of the new parent.
        let first = self.nodes[parent].first_child;
        self.nodes[id].parent = Some(parent);
        self.nodes[id].prev_sibling = None;
        self.nodes[id].next_sibling = first;
        match first {
            Some(first) => self.nodes[first].prev_sibling = Some(id),
            None => self.nodes[parent].last_child = Some(id),
        }
        self.nodes[parent].first_child = Some(id);
        self.nodes[parent].child_count += 1;
    }

    /// `vstNav.MoveTo(aNode, aNode, amInsertBefore, True)`: the children of
    /// the node are inserted before it, in their order, and the node keeps
    /// none.
    pub fn move_children_before(&mut self, id: NodeId) {
        let Some(parent) = self.nodes[id].parent else {
            return;
        };
        let children = self.children(id);
        // Detach every child from the node first.
        for &child in &children {
            let prev = self.nodes[child].prev_sibling;
            let next = self.nodes[child].next_sibling;
            match prev {
                Some(prev) => self.nodes[prev].next_sibling = next,
                None => self.nodes[id].first_child = next,
            }
            match next {
                Some(next) => self.nodes[next].prev_sibling = prev,
                None => self.nodes[id].last_child = prev,
            }
            self.nodes[child].prev_sibling = None;
            self.nodes[child].next_sibling = None;
            self.nodes[id].child_count -= 1;
        }
        // Insert them before the node, in order.
        let mut previous = self.nodes[id].prev_sibling;
        for &child in &children {
            self.nodes[child].parent = Some(parent);
            self.nodes[child].prev_sibling = previous;
            match previous {
                Some(previous) => self.nodes[previous].next_sibling = Some(child),
                None => self.nodes[parent].first_child = Some(child),
            }
            previous = Some(child);
            self.nodes[parent].child_count += 1;
        }
        if let Some(previous) = previous {
            self.nodes[previous].next_sibling = Some(id);
            self.nodes[id].prev_sibling = Some(previous);
        }
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

/// Port of the filter variables of the main form (`FilterConflictAll`,
/// `FilterConflictAllSet` and the rest, which `TfrmFilterOptions` fills from
/// its dialog and a script fills directly; `xedit filter` takes them from a
/// preset of the settings file or from the request).
///
/// A `by_...` field is `None` while its checkbox is off and `Some(value)`
/// while it is on with that value, as the dialog's checkbox pairs (`cbByX`
/// and `cbX`) are; `by_persistent` keeps its two values in plain fields, as
/// `FilterByPersistent` carries the first checkbox alone.
///
/// Not ported: `FilterScripted` (the `Filter` function of a script, phase
/// 6), `IsHidden` of a record (the user hid it in the GUI, which the port
/// keeps no state for) and `fsCompareToHasSameMasters` (the compare-to
/// load).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FilterOptions {
    /// `FilterConflictAll` with `FilterConflictAllSet`: keep the records
    /// whose `ConflictAll` is in the set.
    pub conflict_all: Option<Vec<ConflictAll>>,
    /// `FilterConflictThis` with `FilterConflictThisSet`.
    pub conflict_this: Option<Vec<ConflictThis>>,
    /// `FilterByInjectStatus` with `FilterInjectStatus`: keep the records
    /// that are (or are not) injected.
    pub by_inject_status: Option<bool>,
    /// `FilterByNotReachableStatus` with `FilterNotReachableStatus`, which
    /// reads the reachable state of "Build Reachable Info".
    pub by_not_reachable_status: Option<bool>,
    /// `FilterByReferencesInjectedStatus` with
    /// `FilterReferencesInjectedStatus`.
    pub by_references_injected_status: Option<bool>,
    /// `FilterByEditorID` with `FilterEditorID` (a substring, or a regular
    /// expression with `regex_comparison`; an empty text keeps everything).
    pub by_editor_id: Option<String>,
    /// `FilterByElementValue` with `FilterElementValue`: the value is
    /// searched in the values of the record and of every element below it.
    pub by_element_value: Option<String>,
    /// `FilterByName` with `FilterName` (`DisplayName[True]`).
    pub by_name: Option<String>,
    /// `FilterByBaseEditorID` with `FilterBaseEditorID`; a text of 8 or 9
    /// characters is read as the FormID of the base record instead
    /// (`FilterByBaseFormID`).
    pub by_base_editor_id: Option<String>,
    /// `FilterByBaseName` with `FilterBaseName`.
    pub by_base_name: Option<String>,
    /// `FilterScaledActors`: the references (`ACHR`, `ACRE`) whose `XSCL`
    /// is missing or 1.
    pub scaled_actors: bool,
    /// `FilterBySignature` with `FilterSignatures` (a comma separated list
    /// of four character signatures; an empty list filters everything out).
    pub by_signature: Option<String>,
    /// `FilterByBaseSignature` with `FilterBaseSignatures`.
    pub by_base_signature: Option<String>,
    /// `FilterByPersistent` with `FilterPersistent`.
    pub by_persistent: bool,
    pub persistent: bool,
    /// `FilterUnnecessaryPersistent` with `IsUnnecessaryPersistent`.
    pub unnecessary_persistent: bool,
    /// `FilterMasterIsTemporary` with `FilterIsMaster`.
    pub master_is_temporary: bool,
    pub is_master: bool,
    /// `FilterPersistentPosChanged` with `IsPositionChanged`.
    pub persistent_pos_changed: bool,
    /// `FilterDeleted`.
    pub deleted: bool,
    /// `FilterByVWD` with `FilterVWD`.
    pub by_vwd: Option<bool>,
    /// `FilterByHasVWDMesh` with `FilterHasVWDMesh`.
    pub by_has_vwd_mesh: Option<bool>,
    /// `FilterByHasPrecombinedMesh` with `FilterHasPrecombinedMesh`.
    pub by_has_precombined_mesh: Option<bool>,
    /// `FilterByRegexComparison`: the texts are Perl regular expressions,
    /// case-insensitive and multi-line.
    pub regex_comparison: bool,
    /// `FlattenBlocks`: the block and sub-block groups (type 2 to 5) leave
    /// the tree, their records take their place.
    pub flatten_blocks: bool,
    /// `FlattenCellChilds`: the cell children groups (type 8 to 10).
    pub flatten_cell_childs: bool,
    /// `AssignPersWrldChild`: the persistent references of a worldspace
    /// move into the cell they stand in.
    pub assign_pers_wrld_child: bool,
    /// `InheritConflictByParent`: a node with children takes the highest
    /// status of its children.
    pub inherit_conflict_by_parent: bool,
    /// `ModGroupsEnabled`: the mod groups of the session hide records from
    /// the comparison, which the cleaning functions refuse.
    pub mod_groups_enabled: bool,
    /// `OnlyShowMasterAndLeafs` of the conflict code, which the cleaning
    /// functions refuse.
    pub only_master_and_leafs: bool,
    /// `xeQuickShowConflicts` of the conflict code, which the cleaning
    /// functions refuse.
    pub quick_show_conflicts: bool,
    /// `FilterConflictOnly` (the `xeVeryQuickShowConflicts` mode of
    /// `mniNavFilterConflictsClick`): a record without overrides leaves the
    /// tree without a comparison.
    pub conflict_only: bool,
    /// `FilterOnlyOne` (`mniNavFilterForOnlyOneClick`): a record that is
    /// the only version of its FormID leaves the tree.
    pub only_one: bool,
    /// `FilterNoGameMaster` (`ReInitTree`): the game master leaves the tree.
    pub no_game_master: bool,
    /// The global `ReachableBuild`: "Build Reachable Info" ran
    /// (`refs.build_reachable`). Without it `by_not_reachable_status` has
    /// no effect, as in the GUI, which disables its checkbox.
    pub reachable_build: bool,
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

    /// Port of `mniNavFilterConflictsClick` (`mniNavFilterConflicts`, and
    /// `mniNavFilterConflictsSelected` with the file selection): the records
    /// that win or lose a conflict, or that are identical to their master
    /// but win one, with the blocks and cell children flattened and the
    /// persistent worldspace children moved into their cells. The
    /// `xeVeryQuickShowConflicts` mode (which also sets `conflict_only` and
    /// `no_game_master`) is off unless its switch asked for it.
    pub fn for_conflicts() -> Self {
        FilterOptions {
            conflict_this: Some(vec![
                ConflictThis::ctIdenticalToMasterWinsConflict,
                ConflictThis::ctConflictWins,
                ConflictThis::ctConflictLoses,
            ]),
            flatten_blocks: true,
            flatten_cell_childs: true,
            assign_pers_wrld_child: true,
            inherit_conflict_by_parent: true,
            ..Default::default()
        }
    }

    /// Port of `mniNavFilterForOnlyOneClick`: everything that has more than
    /// one version of its FormID.
    pub fn for_only_one() -> Self {
        FilterOptions {
            inherit_conflict_by_parent: true,
            only_one: true,
            ..Default::default()
        }
    }

    /// The check of `mniNavRemoveIdenticalToMasterClick`,
    /// `mniNavUndeleteAndDisableReferencesClick` and the other cleaning
    /// functions: only "conflict status inherited by parent" may be active.
    pub fn is_cleaning_filter(&self) -> bool {
        !(self.conflict_all.is_some()
            || self.conflict_this.is_some()
            || self.by_inject_status.is_some()
            || self.by_persistent
            || self.by_vwd.is_some()
            || (self.by_not_reachable_status.is_some() && self.reachable_build)
            || self.by_references_injected_status.is_some()
            || self.by_editor_id.is_some()
            || self.by_name.is_some()
            || self.by_element_value.is_some()
            || self.by_signature.is_some()
            || self.by_base_editor_id.is_some()
            || self.by_base_name.is_some()
            || self.scaled_actors
            || self.by_base_signature.is_some()
            || self.flatten_blocks
            || self.flatten_cell_childs
            || self.assign_pers_wrld_child
            || self.mod_groups_enabled
            || self.only_master_and_leafs
            || self.quick_show_conflicts
            || !self.inherit_conflict_by_parent)
    }

    /// The conflict options of the filter (`ModGroupsEnabled`,
    /// `OnlyShowMasterAndLeafs` and `xeQuickShowConflicts`).
    fn conflict_options(&self, mod_groups: Option<Arc<dyn ModGroupFilter>>) -> ConflictOptions {
        ConflictOptions {
            only_master_and_leafs: self.only_master_and_leafs,
            quick_show_conflicts: self.quick_show_conflicts,
            mod_groups,
            ..Default::default()
        }
    }
}

/// One file of a filter ([`FilterReport::files`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredFile {
    /// `Files[i].Name`: the name of the plugin without its folder.
    pub name: String,
    /// `Files[i].FileName`: the path it was loaded from, which the log line
    /// shows.
    pub file_name: String,
    pub load_order: i32,
    /// `RecordCount` of the file.
    pub records: i64,
    /// `FileFiltered[i]`: the records the filter took out of the file. 0
    /// when the file has no record left (upstream leaves the count at 0),
    /// which is also why the log line is printed only for a partial count.
    pub filtered: i64,
}

impl FilteredFile {
    /// The line `wbProgress` writes for a partly filtered file
    /// (`[%s] Filtered %.0n of %.0n records`).
    pub fn log_line(&self) -> String {
        format!(
            "[{}] Filtered {} of {} records",
            self.file_name,
            delphi_grouped(self.filtered),
            delphi_grouped(self.records)
        )
    }

    /// Whether the GUI prints the line for this file (`FileFiltered[i] > 0`
    /// and `< RecordCount`).
    pub fn is_logged(&self) -> bool {
        self.filtered > 0 && self.filtered < self.records
    }
}

/// Delphi's `%.0n`: decimal digits with a thousands separator every three.
/// The GUI formats with the format settings of the machine (a comma on the
/// development machine).
fn delphi_grouped(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut result = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    if value < 0 {
        result.insert(0, '-');
    }
    result
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
    /// The records the filter took out of each loaded file, in load order.
    pub files: Vec<FilteredFile>,
}

impl FilterReport {
    /// The lines `wbProgress` writes for the files the filter partly
    /// filtered, in load order.
    pub fn file_lines(&self) -> Vec<String> {
        self.files
            .iter()
            .filter(|file| file.is_logged())
            .map(FilteredFile::log_line)
            .collect()
    }
}

/// The signatures of the records that put their top level group into the
/// tree (`TopLevelGroups` of `mniNavFilterApplyClick`).
const REFERENCE_TOP_LEVEL_GROUPS: [&[u8; 4]; 14] = [
    b"LAND", b"PGRD", b"NAVM", b"REFR", b"PGRE", b"PMIS", b"ACRE", b"ACHR", b"PHZD", b"PARW", b"PBAR", b"PBEA",
    b"PCON", b"PFLA",
];

/// The prelude of `mniNavFilterApplyClick`: the option fields resolved into
/// what `CheckFilterNode` reads.
struct FilterChecks {
    /// The options with the sets and the flags the prelude changes.
    options: FilterOptions,
    /// `Assigned(Signatures)` with the list.
    signatures: Option<BTreeSet<String>>,
    /// `Assigned(BaseSignatures)` with the list.
    base_signatures: Option<BTreeSet<String>>,
    /// `FilterByBaseFormID` with `FilterBaseFormID`.
    base_form_id: Option<FormID>,
    /// `FilterByBaseEditorID` after the FormID conversion may turn it off.
    by_base_editor_id: bool,
    /// `FilterByStatus`.
    by_status: bool,
    /// `FilterRequiresReference`.
    requires_reference: bool,
    /// `FilterRequiresBaseRecord`.
    requires_base_record: bool,
    /// `FilterRequiresMainRecord`.
    requires_main_record: bool,
    /// `FilterByAnythingNotConflict`.
    anything_not_conflict: bool,
    /// `TopLevelGroups`: the top level groups that stay in the tree.
    top_level_groups: Option<BTreeSet<String>>,
    /// `PotentiallyUnfilteredRefs`: whether records the top level groups do
    /// not cover may stay (which a base record filter needs).
    potentially_unfiltered_refs: bool,
}

/// Port of `wbFindRecordDef` by a four character name.
fn find_record_def_by_name(name: &str) -> Option<Arc<MainRecordDef>> {
    let bytes: [u8; 4] = name.as_bytes().try_into().ok()?;
    find_record_def(Signature::new(&bytes))
}

impl FilterChecks {
    /// The prelude of `mniNavFilterApplyClick` before the two passes.
    fn new(mut options: FilterOptions) -> FilterChecks {
        // A set that holds every value filters nothing (`FilterConflictAll
        // := False`).
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

        // `if FilterBySignature then Signatures.CommaText := FilterSignatures`
        // (a sorted list without duplicates).
        let mut signatures: Option<BTreeSet<String>> = options
            .by_signature
            .as_ref()
            .map(|text| parse_comma_text(text).into_iter().collect());
        let mut base_signatures: Option<BTreeSet<String>> = options
            .by_base_signature
            .as_ref()
            .map(|text| parse_comma_text(text).into_iter().collect());

        // A base editor ID of 8 or 9 characters is the FormID of a base
        // record.
        let mut by_base_editor_id = options.by_base_editor_id.is_some();
        let mut base_form_id = None;
        if let Some(text) = &options.by_base_editor_id
            && matches!(text.len(), 8 | 9)
            && let Some(form_id) = FormID::from_str(text)
        {
            base_form_id = Some(form_id);
            by_base_editor_id = false;
        }

        // `FilterScaledActors` and `FilterByHasVWDMesh` narrow the base
        // signatures to the references they read.
        if options.scaled_actors {
            base_signatures = Some(match base_signatures {
                Some(set) => set
                    .into_iter()
                    .filter(|name| name == "ACHR" || name == "ACRE")
                    .collect(),
                None => ["ACHR", "ACRE"].into_iter().map(str::to_owned).collect(),
            });
        }
        if options.by_has_vwd_mesh.is_some() {
            base_signatures = Some(match base_signatures {
                Some(set) => set.into_iter().filter(|name| name == "REFR").collect(),
                None => ["REFR"].into_iter().map(str::to_owned).collect(),
            });
        }

        let mut potentially_unfiltered_refs = true;
        let mut top_level_groups: Option<BTreeSet<String>> = None;
        if signatures.is_some() || base_signatures.is_some() {
            potentially_unfiltered_refs = false;
            let mut groups: BTreeSet<String> = BTreeSet::new();
            match &base_signatures {
                Some(base) => {
                    match &mut signatures {
                        None => {
                            // The signatures of the records that can have
                            // one of the base signatures as their base.
                            let mut derived: BTreeSet<String> = BTreeSet::new();
                            for name in base {
                                if let Some(def) = find_record_def_by_name(name) {
                                    for index in 0..def.get_reference_signature_count() {
                                        derived.insert(def.get_reference_signature(index as usize).to_string());
                                    }
                                }
                            }
                            signatures = Some(derived);
                        }
                        Some(signatures) => {
                            // Keep the signatures that can have one of the
                            // base signatures as their base.
                            signatures.retain(|name| {
                                find_record_def_by_name(name).is_some_and(|def| {
                                    (0..def.get_base_signature_count())
                                        .any(|index| base.contains(&def.get_base_signature(index as usize).to_string()))
                                })
                            });
                        }
                    }
                    if signatures.as_ref().is_some_and(|signatures| !signatures.is_empty()) {
                        potentially_unfiltered_refs = true;
                        groups.insert("CELL".to_owned());
                        groups.insert("WRLD".to_owned());
                    }
                }
                None => {
                    groups = signatures.clone().expect("one of the two is set");
                    if !groups.contains("CELL")
                        && REFERENCE_TOP_LEVEL_GROUPS
                            .iter()
                            .any(|signature| groups.contains(&Signature::new(signature).to_string()))
                    {
                        groups.insert("CELL".to_owned());
                    }
                    if !groups.contains("WRLD") && (groups.contains("ROAD") || groups.contains("CELL")) {
                        groups.insert("WRLD".to_owned());
                    }
                    if !groups.contains("DIAL") && groups.contains("INFO") {
                        groups.insert("DIAL".to_owned());
                    }
                    if vwd_as_quest_children()
                        && !groups.contains("QUST")
                        && (groups.contains("DIAL") || groups.contains("DLBR") || groups.contains("SCEN"))
                    {
                        groups.insert("QUST".to_owned());
                    }
                }
            }
            for name in &groups {
                if let Some(def) = find_record_def_by_name(name)
                    && def.get_is_reference()
                {
                    potentially_unfiltered_refs = true;
                }
            }
            top_level_groups = Some(groups);
        }
        if !potentially_unfiltered_refs {
            options.assign_pers_wrld_child = false;
        }

        let requires_base_record =
            by_base_editor_id || options.by_base_name.is_some() || options.by_has_vwd_mesh.is_some();
        let requires_reference = base_form_id.is_some()
            || requires_base_record
            || options.scaled_actors
            || options.by_persistent
            || options.by_vwd.is_some()
            || options.by_has_precombined_mesh.is_some()
            || base_signatures.is_some();
        let requires_main_record = requires_reference
            || options.by_editor_id.is_some()
            || options.by_name.is_some()
            || options.by_element_value.is_some()
            || signatures.is_some()
            || options.deleted;
        let by_status = options.by_inject_status.is_some()
            || options.by_references_injected_status.is_some()
            || options.by_not_reachable_status.is_some();
        FilterChecks {
            by_status,
            anything_not_conflict: by_status || requires_main_record,
            requires_reference,
            requires_base_record,
            requires_main_record,
            potentially_unfiltered_refs,
            by_base_editor_id,
            base_form_id,
            signatures,
            base_signatures,
            top_level_groups,
            options,
        }
    }

    /// Port of the `CheckFilterNode` closure: only a node without children
    /// is checked, once; `true` means the node was filtered out and deleted.
    fn check_filter_node(
        &self,
        tree: &mut NavTree,
        id: NodeId,
        check_conflict: bool,
        results: &ConflictResults,
    ) -> bool {
        if tree.child_count(id) != 0 {
            return false;
        }
        if tree.data(id).flags & nav_node_flag::FILTER_CHECKED != 0 {
            return false;
        }
        tree.data_mut(id).flags |= nav_node_flag::FILTER_CHECKED;
        let data = tree.data(id);
        let filtered = (check_conflict
            && (self
                .options
                .conflict_all
                .as_ref()
                .is_some_and(|set| !set.contains(&data.conflict_all))
                || self
                    .options
                    .conflict_this
                    .as_ref()
                    .is_some_and(|set| !set.contains(&data.conflict_this))))
            || (self.anything_not_conflict && self.node_is_filtered(data, results));
        if filtered {
            tree.delete_node(id);
        }
        filtered
    }

    /// The `FilterByAnythingNotConflict` half of `CheckFilterNode`.
    fn node_is_filtered(&self, data: &NavNodeData, results: &ConflictResults) -> bool {
        let options = &self.options;
        if self.by_status {
            if let Some(wanted) = options.by_inject_status
                && (data.flags & nav_node_flag::INJECTED != 0) != wanted
            {
                return true;
            }
            if let Some(wanted) = options.by_references_injected_status
                && (data.flags & nav_node_flag::REFERENCES_INJECTED != 0) != wanted
            {
                return true;
            }
            if let Some(wanted) = options.by_not_reachable_status
                && options.reachable_build
                && (data.flags & nav_node_flag::NOT_REACHABLE != 0) != wanted
            {
                return true;
            }
        }
        if !self.requires_main_record {
            return false;
        }
        let Some(record) = data.element.as_ref().and_then(record_of) else {
            return true;
        };
        if self.requires_reference && !record.def().is_some_and(|def| def.get_is_reference()) {
            return true;
        }
        let base_record = self.requires_base_record.then(|| record.get_base_record());
        if self.requires_base_record && base_record.clone().flatten().is_none() {
            return true;
        }
        if options.deleted && !record.get_is_deleted() {
            return true;
        }
        if let Some(signatures) = &self.signatures
            && !signatures.contains(&record.get_signature().to_string())
        {
            return true;
        }
        if let Some(text) = &options.by_editor_id
            && !self.text_matches(text, &record.get_editor_id())
        {
            return true;
        }
        if let Some(text) = &options.by_name
            && !self.text_matches(text, &record.get_display_name(true))
        {
            return true;
        }
        if let Some(text) = &options.by_element_value {
            let element: ElementRef = record.clone();
            if !self.container_holds_value(text, &element) {
                return true;
            }
        }
        if !self.requires_reference {
            return false;
        }
        if let Some(wanted) = options.by_vwd
            && record.get_flags().is_visible_when_distant() != wanted
        {
            return true;
        }
        if let Some(wanted) = self.base_form_id
            && record.get_base_record().map(|base| base.get_load_order_form_id()) != Some(wanted)
        {
            return true;
        }
        if let Some(base_signatures) = &self.base_signatures {
            let signature = record
                .get_base_record()
                .map_or_else(|| "NULL".to_owned(), |base| base.get_signature().to_string());
            if !base_signatures.contains(&signature) {
                return true;
            }
        }
        if options.scaled_actors {
            // `FilterScaledActors`: only the actors whose `XSCL` is missing
            // or 1 get this far (`SameValue(Rec.NativeValue, 1)`).
            let scale = record
                .get_record_by_signature(Signature::new(b"XSCL"))
                .map(|element| element.get_native_value());
            match scale {
                Some(value) if !value.same_value(&Variant::Int(1)) => {}
                _ => return true,
            }
        }
        if self.requires_base_record {
            let base = base_record.flatten();
            let base = base.as_deref();
            if self.by_base_editor_id
                && let Some(text) = &options.by_base_editor_id
                && !self.text_matches(text, &base.map(|base| base.get_editor_id()).unwrap_or_default())
            {
                return true;
            }
            if let Some(text) = &options.by_base_name
                && !self.text_matches(text, &base.map(|base| base.get_display_name(true)).unwrap_or_default())
            {
                return true;
            }
            if let Some(wanted) = options.by_has_vwd_mesh
                && base.is_some_and(|base| base.get_has_visible_when_distant_mesh() != wanted)
            {
                return true;
            }
        }
        if options.by_persistent {
            if record.get_is_persistent() != options.persistent {
                return true;
            }
            if options.unnecessary_persistent
                && (!is_unnecessary_persistent(&record)
                    || (options.master_is_temporary
                        && !is_master_temporary(&record)
                        && !(options.is_master && record.get_is_master())))
            {
                return true;
            }
            if options.persistent_pos_changed && !is_position_changed(&record, results) {
                return true;
            }
        }
        if let Some(wanted) = options.by_has_precombined_mesh
            && record.get_has_precombined_mesh() != wanted
        {
            return true;
        }
        false
    }

    /// `Pos(AnsiUpperCase(aText), AnsiUpperCase(aComparison)) < 1` as the
    /// dialog reads it, or `CheckValueRegex` with the case-insensitive and
    /// multi-line `TPerlRegEx` options.
    fn text_matches(&self, text: &str, comparison: &str) -> bool {
        if self.options.regex_comparison {
            return check_value_regex(text, comparison);
        }
        comparison.to_ascii_uppercase().contains(&text.to_ascii_uppercase())
    }

    /// `CheckContainerForElementValue`: the contents of the element are
    /// searched depth first, an element without contents by its value.
    fn container_holds_value(&self, value: &str, element: &ElementRef) -> bool {
        let Some(container) = element.as_container() else {
            return false;
        };
        if container.get_element_count() == 0 {
            let text = element.get_value();
            if self.options.regex_comparison {
                return check_value_regex(value, &text);
            }
            return text.to_ascii_uppercase().contains(&value.to_ascii_uppercase());
        }
        for index in 0..container.get_element_count() {
            let Some(child) = container.get_element(index) else {
                continue;
            };
            if self.container_holds_value(value, &child) {
                return true;
            }
        }
        false
    }
}

/// Port of `CheckValueRegex`: `TPerlRegEx` with `preCaseLess` and
/// `preMultiLine`, and `MatchAgain` (whether the expression matches anywhere
/// in the text).
fn check_value_regex(expression: &str, text: &str) -> bool {
    match regex::RegexBuilder::new(expression)
        .case_insensitive(true)
        .multi_line(true)
        .build()
    {
        Ok(regex) => regex.is_match(text),
        // An expression `TPerlRegEx` cannot compile matches nothing.
        Err(_) => false,
    }
}

/// Port of the `IsUnnecessaryPersistent` function of the main form.
fn is_unnecessary_persistent(record: &Arc<MainRecordImpl>) -> bool {
    if record.get_is_deleted() {
        return is_master_temporary(record);
    }
    if !matches!(&record.get_signature().0, b"ACHR" | b"REFR") {
        return false;
    }
    let modern = is_skyrim() || is_fallout4() || is_fallout76() || is_starfield();
    if modern && record.get_flags().0 & 0x0001_0000 != 0 {
        return false;
    }
    let mut count = record.referenced_by_count();
    if count > 0 && modern {
        for reference in record.referenced_by() {
            // `Supports(ReferencedBy[i].LinksTo, IwbMainRecord, lRefRecord)`
            // with the signature `LCTN`: `TwbElement.InternalGetLinksTo` is
            // nil and no main record overrides it, so no entry is ever taken
            // off the count (UPSTREAM-QUIRK: the upstream loop is dead code).
            if reference
                .get_links_to()
                .and_then(Element::into_main_record)
                .is_some_and(|record| record.get_signature().0 == *b"LCTN")
            {
                count -= 1;
            }
        }
    }
    if count > 0 {
        return false;
    }
    if record.get_record_by_signature(Signature::new(b"XTEL")).is_some() {
        return false;
    }
    let Some(name) = record.get_record_by_signature(Signature::new(b"NAME")) else {
        return false;
    };
    match name.get_native_value().as_ordinal() {
        Some(0x4 | 0x5 | 0x6 | 0x10 | 0x12 | 0x15 | 0x1F | 0x34 | 0x3B | 0x138C0 | 0x3DF55) => return false,
        Some(_) => {}
        None => return false,
    }
    let Some(base) = name.get_links_to().and_then(Element::into_main_record) else {
        return false;
    };
    if base.get_record_by_signature(Signature::new(b"SCRI")).is_some() {
        return false;
    }
    if !(is_morrowind() || is_oblivion())
        && base.get_signature().0 == *b"ACTI"
        && base.get_record_by_signature(Signature::new(b"WNAM")).is_some()
    {
        return false;
    }
    if matches!(&base.get_signature().0, b"SBSP" | b"TXST") {
        return false;
    }
    true
}

/// Port of the `IsMasterTemporary` function of the main form.
fn is_master_temporary(record: &Arc<MainRecordImpl>) -> bool {
    if record.get_is_master() {
        return false;
    }
    record.master().is_some_and(|master| !master.get_is_persistent())
}

/// Port of the `IsPositionChanged` function of the main form: the `DATA` of
/// the record against the one of its master. Upstream reads the cached
/// `ConflictThis` of the record, which the port keeps in the results of the
/// comparison.
fn is_position_changed(record: &Arc<MainRecordImpl>, results: &ConflictResults) -> bool {
    if matches!(
        results.status(record).this,
        ConflictThis::ctMaster | ConflictThis::ctIdenticalToMaster
    ) {
        return false;
    }
    if record.get_is_master() || record.get_is_deleted() {
        return false;
    }
    let Some(master) = record.master() else {
        return false;
    };
    let master_pos = master.get_record_by_signature(Signature::new(b"DATA"));
    let this_pos = record.get_record_by_signature(Signature::new(b"DATA"));
    match (master_pos, this_pos) {
        (None, None) => false,
        (Some(_), None) | (None, Some(_)) => true,
        (Some(master_pos), Some(this_pos)) => master_pos.get_sort_key(true) != this_pos.get_sort_key(true),
    }
}

/// Port of `TfrmMain.mniNavFilterApplyClick` with `FilterPreset` (the
/// options are given, no dialog and no file selection): the conflict status
/// of every record in the tree, the nodes filtered out removed from the
/// tree, and every node with children given the highest status of its
/// children.
///
/// The tree is the caller's (`ReInitTree` builds the nodes, which the GUI
/// does inside the same handler); the port's [`NavTree`] holds every node
/// at once, so the walk covers what the GUI's lazy initialisation builds as
/// it walks. The port's tree has no expand state either: upstream drops the
/// nodes a collapsed parent does not show (`vsVisible`), while the filter
/// runs with the tree hidden, and the parity check of the cleaning filter
/// (which runs this very code path) matches the oracle node for node.
pub fn apply_filter(
    tree: &mut NavTree,
    options: &FilterOptions,
    files: &[Arc<FileImpl>],
) -> Result<FilterReport, String> {
    apply_filter_with(tree, options, files, None)
}

/// [`apply_filter`] with the mod groups of the session, which the conflict
/// status reads while `ModGroupsEnabled` is on.
pub fn apply_filter_with(
    tree: &mut NavTree,
    options: &FilterOptions,
    files: &[Arc<FileImpl>],
    mod_groups: Option<Arc<dyn ModGroupFilter>>,
) -> Result<FilterReport, String> {
    let checks = FilterChecks::new(options.clone());
    let options = &checks.options;
    let mut report = FilterReport::default();
    tree.applied = None;

    // `if (FilterConflictAll and (FilterConflictAllSet = [])) or ... then
    // vstNav.Clear`.
    if options.conflict_all.as_ref().is_some_and(Vec::is_empty)
        || options.conflict_this.as_ref().is_some_and(Vec::is_empty)
        || (checks.requires_base_record && !checks.potentially_unfiltered_refs)
        || checks.signatures.as_ref().is_some_and(BTreeSet::is_empty)
        || checks.base_signatures.as_ref().is_some_and(BTreeSet::is_empty)
        || checks.top_level_groups.as_ref().is_some_and(BTreeSet::is_empty)
    {
        tree.clear();
        tree.applied = Some(options.clone());
        return Ok(report);
    }

    let check_conflict = options.conflict_all.is_some() || options.conflict_this.is_some();
    let conflict_options = options.conflict_options(mod_groups);

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
        conflict_statuses_of(&records, files, &conflict_options)
    } else {
        ConflictResults::default()
    };
    report.messages = results.messages.clone();

    // `TopLevelGroups`: the top level groups of the signatures asked for
    // stay, the others go.
    if let Some(groups) = &checks.top_level_groups {
        for file in tree.roots() {
            for group in tree.children(file) {
                let unwanted = tree
                    .data(group)
                    .element
                    .as_ref()
                    .and_then(group_of)
                    .is_some_and(|group| {
                        group.group_type() == 0 && !groups.contains(&group.gr_struct().label_signature().to_string())
                    });
                if unwanted {
                    tree.delete_node(group);
                }
            }
        }
    }

    // The files of the tree, which the report names even when every record of
    // one was filtered out (upstream walks `Files`, not the tree).
    let tree_files = tree_file_nodes(tree);

    // ----- Pass 1 -----
    let flatten_cell = options.flatten_cell_childs && options.assign_pers_wrld_child;
    let mut cells: HashMap<(i32, i32), Vec<NodeId>> = HashMap::new();
    let mut pers_cell_checked = false;
    let mut pers_cell: Option<NodeId> = None;
    let mut current = tree.get_last(None);
    while let Some(id) = current {
        report.pass1 += 1;
        let mut next = tree.get_previous(id);
        let mut found_any = false;
        let Some(element) = tree.data(id).element.clone() else {
            // A node without an element (the child group of a record) goes.
            tree.delete_node(id);
            current = next;
            continue;
        };
        if let Some(record) = record_of(&element) {
            if options.conflict_only && record.master_or_self_impl().overrides().len() < 2 && tree.child_count(id) == 0
            {
                // `MasterOrSelf.OverrideCount < 2`: no conflict is possible.
                tree.delete_node(id);
                current = next;
                continue;
            }
            if options.only_one && tree.child_count(id) == 0 {
                let master = record.master_or_self_impl();
                if !master.overrides().is_empty() && 1 + master.overrides().len() > 1 {
                    // `IsHidden` (a record the user hid in the GUI) keeps no
                    // state in the port: every version counts.
                    tree.delete_node(id);
                    current = next;
                    continue;
                }
            }
            {
                let data = tree.data_mut(id);
                if record.is_injected() {
                    data.flags |= nav_node_flag::INJECTED;
                }
                if record.get_is_not_reachable() {
                    data.flags |= nav_node_flag::NOT_REACHABLE;
                }
                if record.references_injected() {
                    data.flags |= nav_node_flag::REFERENCES_INJECTED;
                }
                data.flags &= !nav_node_flag::FILTER_CHECKED;
            }
            if !(flatten_cell && record.get_signature().0 == *b"CELL")
                && checks.check_filter_node(tree, id, false, &results)
            {
                current = next;
                continue;
            }
            if check_conflict || options.inherit_conflict_by_parent {
                let status = results.status(&record);
                let data = tree.data_mut(id);
                data.conflict_all = status.all;
                data.conflict_this = status.this;
                data.org_conflict_all = status.all;
                data.org_conflict_this = status.this;
            }
            if !(flatten_cell && record.get_signature().0 == *b"CELL") && tree.child_count(id) == 0 {
                let data = tree.data(id);
                let filtered = options
                    .conflict_all
                    .as_ref()
                    .is_some_and(|set| !set.contains(&data.conflict_all))
                    || options
                        .conflict_this
                        .as_ref()
                        .is_some_and(|set| !set.contains(&data.conflict_this));
                if filtered {
                    tree.delete_node(id);
                    current = next;
                    continue;
                }
            }
            // `AssignPersWrldChild`: the persistent references of the
            // worldspace move into the cell they stand in.
            if flatten_cell {
                match &record.get_signature().0 {
                    b"WRLD" => {
                        cells.clear();
                        pers_cell_checked = false;
                        pers_cell = None;
                    }
                    b"CELL" => {
                        let group_type = element
                            .get_container()
                            .and_then(|container| container.as_element_impl()?.group_record_impl())
                            .map(|group| group.group_type());
                        if group_type == Some(5) {
                            if !pers_cell_checked {
                                pers_cell_checked = true;
                                pers_cell = persistent_cell_node(tree, id);
                                cells.clear();
                                if let Some(cell) = pers_cell {
                                    let mut node = tree.last_child(cell);
                                    while let Some(current) = node {
                                        let previous = tree.previous_sibling(current);
                                        if let Some(record) = tree.data(current).element.as_ref().and_then(record_of)
                                            && let Some(position) = record.get_position()
                                        {
                                            let cell_of =
                                                (position_to_grid_cell(position.0), position_to_grid_cell(position.1));
                                            cells.entry(cell_of).or_default().push(current);
                                        }
                                        node = previous;
                                    }
                                }
                            }
                            if pers_cell.is_some()
                                && let Some(grid_cell) = record.get_grid_cell()
                                && let Some(nodes) = cells.remove(&grid_cell)
                            {
                                for node in nodes {
                                    tree.move_to_child_first(node, id);
                                    if !found_any {
                                        // `if not FoundAny then begin
                                        // FoundAny := True; NextNode := Node2;
                                        // end`: the walk goes on at the first
                                        // moved node, in its new place.
                                        found_any = true;
                                        next = Some(node);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        } else if let Some(group) = group_of(&element)
            && group.group_type() == 1
            && pers_cell.is_some()
        {
            pers_cell_checked = false;
            pers_cell = None;
            cells.clear();
        }

        if found_any {
            current = next;
            continue;
        }
        let flatten_group = tree.data(id).element.as_ref().and_then(group_of).is_some_and(|group| {
            (options.flatten_blocks && (2..=5).contains(&group.group_type()))
                || (options.flatten_cell_childs && (8..=10).contains(&group.group_type()))
        });
        if tree.child_count(id) > 0 {
            if flatten_group {
                // `MoveTo(Node, Node, amInsertBefore, True)` then
                // `DeleteNode`: the children of the group take its place.
                tree.move_children_before(id);
                tree.delete_node(id);
            } else if options.inherit_conflict_by_parent && pers_cell != Some(id) {
                tree.inherit_state_from_children(id);
            }
        } else if element.get_skipped() || flatten_group {
            // A node whose element is skipped goes, and so does a group the
            // flattening takes out even when it has no children left.
            tree.delete_node(id);
        }
        current = next;
    }

    // ----- Pass 2: `CheckFilterNode(True)` on the nodes without children
    // and the records left per file. -----
    let mut remaining: HashMap<NodeId, i64> = HashMap::new();
    let mut records_of_file: i64 = 0;
    let mut current = tree.get_last(None);
    while let Some(id) = current {
        let next = tree.get_previous(id);
        report.pass2 += 1;
        let is_file = tree
            .data(id)
            .element
            .as_ref()
            .is_some_and(|element| element.get_element_type() == ElementType::etFile);
        if is_file {
            // `FileFiltered[i] := RecordCount - MainRecordCount`: the
            // records walked since the file node below are the ones this
            // file keeps.
            remaining.insert(id, records_of_file);
            records_of_file = 0;
        }
        let deleted = checks.check_filter_node(tree, id, true, &results);
        if !deleted {
            report.unfiltered += 1;
            if !is_file
                && tree
                    .data(id)
                    .element
                    .as_ref()
                    .is_some_and(|element| element.get_element_type() == ElementType::etMainRecord)
            {
                records_of_file += 1;
            }
        }
        current = next;
    }
    for (node, file) in &tree_files {
        let records = i64::from(file.get_record_count());
        // `FileFiltered[i] := RecordCount - MainRecordCount` only when the
        // count is above zero (UPSTREAM-QUIRK: a file whose every record was
        // filtered out reports nothing filtered).
        let filtered = match remaining.get(node).copied() {
            Some(kept) if kept > 0 => records - kept,
            _ => 0,
        };
        report.files.push(FilteredFile {
            name: file.get_name(),
            file_name: file.file_name().to_owned(),
            load_order: file.load_order(),
            records,
            filtered,
        });
    }
    tree.applied = Some(options.clone());
    Ok(report)
}

/// The file nodes of the tree with their files, in order.
fn tree_file_nodes(tree: &NavTree) -> Vec<(NodeId, Arc<FileImpl>)> {
    tree.roots()
        .into_iter()
        .filter_map(|node| {
            let file = tree
                .data(node)
                .element
                .as_ref()
                .and_then(|element| element.as_element_impl()?.file_impl())?;
            Some((node, file))
        })
        .collect()
}

/// The node of the persistent cell of the worldspace of an exterior cell
/// record: the first `CELL` child of the `WRLD` record above the cell, then
/// the group of type 8 below that cell (`AssignPersWrldChild` of
/// `mniNavFilterApplyClick`).
fn persistent_cell_node(tree: &NavTree, cell: NodeId) -> Option<NodeId> {
    let block = tree.parent(cell)?;
    let sub_block = tree.parent(block)?;
    let world = tree.parent(sub_block)?;
    let is_world = tree
        .data(world)
        .element
        .as_ref()
        .and_then(record_of)
        .is_some_and(|record| record.get_signature().0 == *b"WRLD");
    if !is_world {
        return None;
    }
    let first_cell = tree.children(world).into_iter().find(|child| {
        tree.data(*child)
            .element
            .as_ref()
            .and_then(record_of)
            .is_some_and(|record| record.get_signature().0 == *b"CELL")
    })?;
    tree.children(first_cell).into_iter().find(|child| {
        tree.data(*child)
            .element
            .as_ref()
            .and_then(group_of)
            .is_some_and(|group| group.group_type() == 8)
    })
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
