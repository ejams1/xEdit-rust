// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (AddFileInternal, vstNavInitChildren,
// vstNavInitNode, vstNavGetText, vstNavPaintText, vstNavBeforeItemErase,
// vstNavChange's path and header titles, ReInitTree, NavUpdate, NavUpdate's
// collapse cleanup, edFileNameFilterChange, edFormIDSearchKeyDown,
// edEditorIDSearchKeyDown, mniNavAdd's GetAddList, GetSortedSelection,
// EditableSelection, RemovableSelection) and xEdit/xeMainForm.dfm (vstNav,
// pnlSearch, pnlNavBottom, lblFilterHint, pmuNav, pmuNavHeaderPopup,
// pmuNavAdd)

//! The navigation tree of the main form (`vstNav`) and everything it drives:
//! the lazily built node tree over the session's elements, the cell texts of
//! `vstNavGetText`, the conflict colours of `vstNavBeforeItemErase` and
//! `vstNavPaintText`, the file-name filter, the two search boxes, the filter
//! view of `filter.apply`/`filter.remove`, and the action of every item of
//! `pmuNav`, `pmuNavHeaderPopup` and `pmuNavAdd`.
//!
//! The tree is the port of upstream's lazy `TVirtualEditTree`: a node's
//! children are built the first time it is expanded, from
//! `Container.Elements[i]` in container order, with the same grouping rules
//! as `vstNavInitNode` (a parented group record of types 1, 6 and 7 and, for
//! VWD quest children, 10, shows below the main record it holds the children
//! of). Only expanded nodes contribute rows, so a tree over a million
//! records costs what the screen shows.
//!
//! Every action is a session command (design rule 1). The only exceptions
//! are the items whose upstream handler opens a dialog of a later step; those
//! call a named placeholder of [`crate::dialogs`]. [`nav_actions`] is a pure
//! function over the selection, so the dispatch of every menu item is tested
//! headlessly.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{Value, json};

use xedit_core::implementation::{ElementImpl, FileImpl, GroupRecordImpl, MainRecordImpl};
use xedit_core::interface::constructors::find_record_def;
use xedit_core::interface::globals::{
    game_mode, group_to_skip, header_signature, record_to_skip, vwd_as_quest_children, wb_group_order,
};
use xedit_core::interface::types::{ConflictAll, ConflictThis, ElementType, Signature};
use xedit_core::interface::{Element, ElementRef, File, MainRecord, NamedDef};

use crate::dialogs;
use crate::menu_model::MenuItem;
use crate::selection::{NodeKind, SelectedNode, Selection};
use crate::widgets::conflict::wb_lighter;
use crate::widgets::virtual_tree::{TreeCell, TreeColumn, TreeRow, TreeSource, TreeState};

/// The background of the search boxes of `vstNav`: `clWindowText` over
/// `clWindow`, then the three states of `edFormIDSearchKeyDown` and
/// `edEditorIDSearchKeyDown` (`wbLighter(clLime|clYellow|clRed, 0.85)`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchState {
    /// Nothing typed or the box was changed again (`clWindow`).
    #[default]
    Normal,
    /// The record was found and jumped to (`clLime`).
    Found,
    /// The record exists but is not in the tree (`clYellow`).
    NotInTree,
    /// No record (`clRed`), or a bad file-name regular expression.
    NotFound,
}

impl SearchState {
    pub fn color(self, dark_mode: bool) -> Option<eframe::egui::Color32> {
        let base = match self {
            SearchState::Normal => return None,
            SearchState::Found => eframe::egui::Color32::from_rgb(0, 255, 0),
            SearchState::NotInTree => eframe::egui::Color32::from_rgb(255, 255, 0),
            SearchState::NotFound => eframe::egui::Color32::from_rgb(255, 0, 0),
        };
        Some(wb_lighter(base, 0.85, dark_mode))
    }
}

/// The two search boxes and the file-name filter of `pnlNav`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavSearch {
    /// `edFormIDSearch.Text`.
    pub form_id: String,
    /// `edFormIDSearch.Color`.
    pub form_id_state: SearchState,
    /// `edEditorIDSearch.Text`.
    pub editor_id: String,
    /// `edEditorIDSearch.Color`.
    pub editor_id_state: SearchState,
    /// `edFileNameFilter.Text`.
    pub file_name: String,
    /// `cbRegExFilter.Checked`.
    pub file_regex: bool,
    /// `edFileNameFilter.Color`.
    pub file_state: SearchState,
}

/// The state of the tree after `filter.apply` (`FilterApplied` and the
/// filtered `vstNav`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilterView {
    /// `FilterApplied`: the tree is fossilised (its structure no longer
    /// follows edits) and `lblFilterHint` is visible.
    pub applied: bool,
    /// The elements of the surviving nodes, by `Element::get_element_id`.
    /// A node whose element is not in the set was deleted by the filter.
    pub survivors: Option<HashSet<usize>>,
    /// The conflict status the filter left on the nodes (`NodeData.ConflictAll`
    /// and `ConflictThis` after `InheritStateFromChildren`), by element id.
    pub statuses: HashMap<usize, (ConflictAll, ConflictThis)>,
}

/// One node of the navigation tree.
pub struct NavNode {
    /// `TNavNodeData.Element`; `None` for the node of a parented group that
    /// its main record shows (`InitialStates` contains `ivsHidden`).
    pub element: Option<ElementRef>,
    /// `TNavNodeData.Container`: the element's own container, or the child
    /// group of a main record.
    pub container: Option<ElementRef>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Whether the children were built (`OnInitChildren` ran).
    pub loaded: bool,
    pub expanded: bool,
    pub depth: usize,
    /// Whether `vstNavInitNode` set `ivsHasChildren`.
    pub has_children: bool,
}

/// The navigation tree (`vstNav`).
pub struct NavSource {
    pub nodes: Vec<NavNode>,
    pub roots: Vec<usize>,
    /// The display order of the expanded nodes; rebuilt by [`NavSource::prepare`].
    visible: Vec<usize>,
    dirty: bool,
    pub filter: FilterView,
    /// The conflict status of the records, by `(file, load order FormID)`.
    conflicts: HashMap<(String, String), (ConflictAll, ConflictThis)>,
    /// The file-name filter: the roots the current text keeps visible.
    pub file_visibility: Option<HashSet<String>>,
    pub search: NavSearch,
    /// `wbShowFileFlags` and `wbShowGroupRecordCount` of the settings file.
    pub show_file_flags: bool,
    pub show_group_record_count: bool,
    columns: Vec<TreeColumn>,
    /// The header titles of the last focused node (`vstNavChange`).
    header_kind: Option<NodeKind>,
}

/// The conflict statuses asked for at most in one frame; the rest follow on
/// later frames, as upstream compares the rows it paints.
pub const PENDING_CONFLICT_BATCH: usize = 200;

fn element_type(element: &ElementRef) -> ElementType {
    element.get_element_type()
}

fn main_record_of(element: &ElementRef) -> Option<Arc<MainRecordImpl>> {
    element.as_element_impl().and_then(ElementImpl::main_record_impl)
}

fn group_of(element: &ElementRef) -> Option<Arc<GroupRecordImpl>> {
    element.as_element_impl().and_then(ElementImpl::group_record_impl)
}

fn file_of(element: &ElementRef) -> Option<Arc<FileImpl>> {
    element.as_element_impl().and_then(ElementImpl::file_impl)
}

/// `ParentedGroupRecordType` of `FormShow`: the groups that show below the
/// record they hold the children of.
fn is_parented_group_type(group_type: i32) -> bool {
    matches!(group_type, 1 | 6 | 7) || (group_type == 10 && vwd_as_quest_children())
}

/// `(Element as IwbMainRecord).FormID.ToCardinal = GroupRecord.GroupLabel`:
/// the group holds the children of the record before it. The label is the
/// fixed-up `GetGroupLabel` the analysis filter uses.
fn holds_children_of(record: &MainRecordImpl, group: &GroupRecordImpl) -> bool {
    is_parented_group_type(group.group_type()) && record.mr_struct().form_id.to_cardinal() == group.get_group_label()
}

/// The elements of a container in the order of `Container.Elements[i]`.
fn elements_of(container: &ElementRef) -> Vec<ElementRef> {
    let Some(container) = container.as_container() else {
        return Vec::new();
    };
    (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .collect()
}

impl NavSource {
    /// `ReInitTree(False, nil)`: one root per loaded file in load order.
    pub fn from_loaded() -> Self {
        let mut files: Vec<Arc<FileImpl>> = xedit_core::interface::files()
            .into_iter()
            .filter_map(|file| file.as_element_impl().and_then(ElementImpl::file_impl))
            .collect();
        // `Files` is in load order; the hardcoded file sorts after its slot.
        files.sort_by_key(|file| {
            (
                file.load_order(),
                file.get_file_states()
                    .contains(xedit_core::interface::types::FileState::fsIsHardcoded),
            )
        });
        Self::new(files)
    }

    /// A tree over the given files, in the order given.
    pub fn new(files: Vec<Arc<FileImpl>>) -> Self {
        let mut source = Self {
            nodes: Vec::new(),
            roots: Vec::new(),
            visible: Vec::new(),
            dirty: true,
            filter: FilterView::default(),
            conflicts: HashMap::new(),
            file_visibility: None,
            search: NavSearch::default(),
            show_file_flags: false,
            show_group_record_count: false,
            columns: vec![
                TreeColumn::new("FormID", crate::settings::NAV_COLUMN_WIDTHS[0]),
                TreeColumn::new("EditorID", crate::settings::NAV_COLUMN_WIDTHS[1]),
                TreeColumn::new("Name", crate::settings::NAV_COLUMN_WIDTHS[2]),
            ],
            header_kind: None,
        };
        source.set_files(files);
        source
    }

    /// `ReInitTree`: the tree drops every node and starts again from the
    /// files. `filter.remove` and a load call it.
    pub fn set_files(&mut self, files: Vec<Arc<FileImpl>>) {
        self.nodes.clear();
        self.roots.clear();
        self.visible.clear();
        self.dirty = true;
        self.conflicts.clear();
        for file in files {
            let element: ElementRef = file.clone();
            let container = (!elements_of(&element).is_empty()).then(|| element.clone());
            let id = self.nodes.len();
            self.nodes.push(NavNode {
                element: Some(element),
                container,
                parent: None,
                children: Vec::new(),
                loaded: false,
                expanded: false,
                depth: 0,
                has_children: false,
            });
            self.nodes[id].has_children = self.nodes[id].container.is_some();
            self.roots.push(id);
        }
    }

    /// `ReInitTree(False, nil)` without a reload of `Files`.
    pub fn reset(&mut self) {
        self.filter = FilterView::default();
        self.file_visibility = None;
        let files: Vec<Arc<FileImpl>> = self
            .roots
            .iter()
            .filter_map(|&root| self.nodes[root].element.as_ref().and_then(file_of))
            .collect();
        self.set_files(files);
    }

    /// `mniNavHeaderFilesClick`: `vstNav.Header.SortTree` over the roots.
    pub fn sort_roots(&mut self, sort: crate::settings::FilesSort) {
        use crate::settings::FilesSort;
        match sort {
            FilesSort::AsSelected | FilesSort::LoadOrder => {
                self.roots.sort_by_key(|&id| {
                    self.nodes[id]
                        .element
                        .as_ref()
                        .and_then(file_of)
                        .map_or(0, |file| file.load_order())
                });
            }
            FilesSort::FileName => {
                self.roots.sort_by_key(|&id| {
                    self.nodes[id]
                        .element
                        .as_ref()
                        .map(|element| element.get_name().to_lowercase())
                        .unwrap_or_default()
                });
            }
        }
        self.dirty = true;
    }

    /// `vstNav.FullExpand`: expands every node, building the children as
    /// it walks, so the whole tree is materialised (`Expand All` and the
    /// tests that check the whole tree).
    pub fn expand_all(&mut self) {
        let mut index = 0;
        while index < self.nodes.len() {
            if self.nodes[index].has_children {
                if !self.nodes[index].loaded {
                    self.load_children(index);
                }
                self.nodes[index].expanded = true;
            }
            index += 1;
        }
        self.dirty = true;
    }

    /// `NavUpdate(True)` after an edit: the children of every node are
    /// rebuilt when it is next shown, and the conflict colours are asked for
    /// again. The expansion and the selection stay where they are.
    pub fn refresh(&mut self) {
        for node in &mut self.nodes {
            node.children.clear();
            node.loaded = false;
        }
        self.conflicts.clear();
        self.dirty = true;
    }

    pub fn node(&self, id: usize) -> Option<&NavNode> {
        self.nodes.get(id)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The node of a row of the last [`NavSource::prepare`].
    pub fn row_node(&self, row: usize) -> Option<usize> {
        self.visible.get(row).copied()
    }

    /// `OnInitChildren` and `OnInitNode`: builds the children of a node.
    fn load_children(&mut self, id: usize) {
        if self.nodes.get(id).is_none_or(|node| node.loaded) {
            return;
        }
        self.nodes[id].loaded = true;
        let Some(container) = self.nodes[id].container.clone() else {
            return;
        };
        let elements = elements_of(&container);
        let depth = self.nodes[id].depth + 1;
        let mut children = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            // The filter deleted the nodes it did not keep from the tree
            // (`vstNav.DeleteNode`); a filtered tree builds the survivors
            // only, so expanding a surviving cell does not build the placed
            // references the filter took out.
            if self.filter.applied
                && !self
                    .filter
                    .survivors
                    .as_ref()
                    .is_some_and(|survivors| survivors.contains(&element.get_element_id()))
            {
                continue;
            }
            // `vstNavInitNode`: a main record whose next sibling is a
            // parented group that holds its children shows the group's
            // elements below the record; the group's own node is hidden.
            let record = main_record_of(element);
            let mut child_container = None;
            if record.is_some()
                && let Some(next) = elements.get(index + 1)
                && let Some(group) = group_of(next)
                && holds_children_of(record.as_ref().expect("checked above"), &group)
            {
                child_container = Some(next.clone());
            }
            let child_has_children = match (&record, &child_container) {
                (Some(_), Some(group)) => !elements_of(group).is_empty(),
                _ => {
                    child_container = element
                        .as_container()
                        .is_some_and(|container| container.get_element_count() > 0)
                        .then(|| element.clone());
                    child_container.is_some()
                }
            };
            let child = self.nodes.len();
            self.nodes.push(NavNode {
                element: Some(element.clone()),
                container: child_container,
                parent: Some(id),
                children: Vec::new(),
                loaded: false,
                expanded: false,
                depth,
                has_children: child_has_children,
            });
            children.push(child);
        }
        // The group nodes the records show: `ivsHidden` with no element.
        // A filtered tree has none: the group node is not a survivor.
        if self.filter.applied {
            self.nodes[id].children = children;
            return;
        }
        for (index, element) in elements.iter().enumerate() {
            if index == 0 {
                continue;
            }
            if let (Some(group), Some(previous)) = (group_of(element), main_record_of(&elements[index - 1]))
                && holds_children_of(&previous, &group)
            {
                let child = self.nodes.len();
                self.nodes.push(NavNode {
                    element: None,
                    container: Some(element.clone()),
                    parent: Some(id),
                    children: Vec::new(),
                    loaded: true,
                    expanded: false,
                    depth,
                    has_children: false,
                });
                children.push(child);
            }
        }
        // The hidden group nodes were added after the record nodes; restore
        // the container order (`vstNavInitChildren` numbers the elements).
        self.nodes[id].children = children;
    }

    /// Rebuilds the display order, loading the children of expanded nodes.
    pub fn prepare(&mut self, focused: Option<NodeKind>) {
        if self.header_kind != focused {
            self.header_kind = focused;
            self.columns = header_columns(focused);
        }
        if !self.dirty {
            return;
        }
        self.visible.clear();
        let roots = self.roots.clone();
        for root in roots {
            self.push_visible(root);
        }
        self.dirty = false;
    }

    fn push_visible(&mut self, id: usize) {
        if !self.node_visible(id) {
            return;
        }
        self.visible.push(id);
        if !self.nodes[id].expanded {
            return;
        }
        if !self.nodes[id].loaded {
            self.load_children(id);
        }
        let children = self.nodes[id].children.clone();
        for child in children {
            self.push_visible(child);
        }
    }

    /// Whether a node survived the filter and the file-name filter.
    fn node_visible(&self, id: usize) -> bool {
        let Some(node) = self.nodes.get(id) else {
            return false;
        };
        if node.parent.is_none()
            && let Some(visible) = &self.file_visibility
            && let Some(name) = node.element.as_ref().and_then(file_of).map(|file| file.get_name())
        {
            // `edFileNameFilterChange` sets `IsVisible` of the file nodes.
            if !visible.contains(&name) {
                return false;
            }
        }
        if !self.filter.applied {
            return true;
        }
        // A filtered tree keeps the nodes the filter left; a node without an
        // element (the hidden group) never has one.
        match (&node.element, &self.filter.survivors) {
            (Some(element), Some(survivors)) => survivors.contains(&element.get_element_id()),
            _ => false,
        }
    }

    /// The conflict status of a node, as `vstNavBeforeItemErase` computes it.
    fn node_conflict(&self, node: &NavNode) -> (ConflictAll, ConflictThis) {
        let Some(element) = &node.element else {
            return (ConflictAll::caUnknown, ConflictThis::ctUnknown);
        };
        if let Some(status) = self.filter.statuses.get(&element.get_element_id()) {
            return *status;
        }
        if element_type(element) != ElementType::etMainRecord {
            return (ConflictAll::caUnknown, ConflictThis::ctUnknown);
        }
        record_key(element)
            .and_then(|key| self.conflicts.get(&key).copied())
            .unwrap_or((ConflictAll::caUnknown, ConflictThis::ctUnknown))
    }

    /// The records among the visible rows whose conflict was not asked for
    /// yet, at most [`PENDING_CONFLICT_BATCH`] of them.
    pub fn pending_conflicts(&self) -> Vec<(String, Option<String>)> {
        let mut pending = Vec::new();
        for &id in &self.visible {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            let Some(element) = &node.element else {
                continue;
            };
            if element_type(element) != ElementType::etMainRecord {
                continue;
            }
            let Some(key) = record_key(element) else {
                continue;
            };
            if self.conflicts.contains_key(&key) || self.filter.statuses.contains_key(&element.get_element_id()) {
                continue;
            }
            pending.push((key.1, Some(key.0)));
            if pending.len() >= PENDING_CONFLICT_BATCH {
                break;
            }
        }
        pending
    }

    /// `ConflictLevelForMainRecord`'s result for the rows `conflicts.list`
    /// classified.
    pub fn apply_conflicts(&mut self, records: &[(String, String, ConflictAll, ConflictThis)]) {
        for (file, form_id, all, this) in records {
            self.conflicts.insert((file.clone(), form_id.clone()), (*all, *this));
        }
    }

    /// Installs the tree `filter.apply` left: the surviving nodes and the
    /// statuses of `InheritStateFromChildren`.
    pub fn install_filter(&mut self, survivors: Vec<usize>, statuses: Vec<(usize, ConflictAll, ConflictThis)>) {
        self.filter = FilterView {
            applied: true,
            survivors: Some(survivors.into_iter().collect()),
            statuses: statuses
                .into_iter()
                .map(|(element, all, this)| (element, (all, this)))
                .collect(),
        };
        self.dirty = true;
    }

    /// `mniNavFilterRemoveClick`: `ReInitTree(False, nil)`.
    pub fn remove_filter(&mut self) {
        self.reset();
    }

    /// `edFileNameFilterChange`: the file roots whose name the text keeps.
    /// `Err` is a bad regular expression (the box turns red, the previous
    /// visibility stays, as upstream ignores the error while typing).
    pub fn set_file_visibility(&mut self, names: Vec<String>) {
        self.file_visibility = if self.search.file_name.is_empty() {
            None
        } else {
            Some(names.into_iter().collect())
        };
        self.dirty = true;
    }

    /// `vstNavChange`'s path: the focused row and its ancestors, each with
    /// column 0 and, for a file, its flags in angle brackets.
    pub fn path_text(&self, state: &TreeState) -> String {
        let Some(row) = state.focused else {
            return String::new();
        };
        let Some(&id) = self.visible.get(row) else {
            return String::new();
        };
        let mut parts = Vec::new();
        let mut current = Some(id);
        while let Some(node_id) = current {
            let Some(node) = self.nodes.get(node_id) else {
                break;
            };
            let Some(element) = &node.element else {
                current = node.parent;
                continue;
            };
            let text = if element_type(element) == ElementType::etFile {
                let mut text = element.get_name();
                // `vstNav.Text[Node, 1]`: the flags of the `Flags` column,
                // empty while `wbShowFileFlags` is off.
                let flags = if self.show_file_flags {
                    file_flags(element)
                } else {
                    String::new()
                };
                if !flags.is_empty() {
                    text.push_str(&format!(" <{flags}>"));
                }
                text
            } else {
                cell_text(element, 0, self.show_group_record_count, self.show_file_flags)
            };
            parts.push(text);
            current = node.parent;
        }
        parts.reverse();
        format!(" {}", parts.join(" \\ "))
    }

    /// The focused node's kind, for the header titles and the menu gating.
    pub fn focused_kind(&self, state: &TreeState) -> Option<NodeKind> {
        let row = state.focused?;
        let id = self.visible.get(row)?;
        let element = self.nodes.get(*id)?.element.as_ref()?;
        Some(kind_of(element))
    }

    /// The focused node's element, as the menus read `vstNav.FocusedNode`.
    pub fn focused_element(&self, state: &TreeState) -> Option<ElementRef> {
        let row = state.focused?;
        let id = self.visible.get(row)?;
        self.nodes.get(*id)?.element.clone()
    }

    /// `vstNav.GetSortedSelection(True)` and `FocusedNode` as a [`Selection`].
    pub fn selection(&self, state: &TreeState) -> Selection {
        let mut rows = state.selected.clone();
        rows.sort_unstable();
        rows.dedup();
        let nodes: Vec<SelectedNode> = rows
            .iter()
            .filter_map(|&row| self.visible.get(row))
            .filter_map(|&id| self.selected_node(id))
            .collect();
        let focused = state
            .focused
            .and_then(|row| self.visible.get(row))
            .and_then(|&id| self.selected_node(id));
        Selection { nodes, focused }
    }

    /// One node as the menu handlers read it.
    fn selected_node(&self, id: usize) -> Option<SelectedNode> {
        let node = self.nodes.get(id)?;
        let element = node.element.as_ref()?;
        let record = main_record_of(element);
        let file = element
            .get_file()
            .map(|file| file.get_name())
            .or_else(|| (element_type(element) == ElementType::etFile).then(|| element.get_name()));
        Some(SelectedNode {
            kind: kind_of(element),
            element_name: element.get_name(),
            file,
            form_id: record
                .as_ref()
                .map(|record| record.get_load_order_form_id().to_string(false)),
            signature: record.as_ref().map(|record| record.get_signature().to_string()),
            editor_id: record.as_ref().map(|record| record.get_editor_id()),
            record_name: record.as_ref().map(|record| record.get_display_name(true)),
            has_children: node.has_children,
            editable: element.get_is_editable(),
            hidden: xedit_core::implementation::sortable::is_hidden(element),
        })
    }

    /// `FindNodeForElement`: the node of a record version, or of its file's
    /// copy, among the nodes built so far.
    pub fn find_record(&self, form_id: &str, file: Option<&str>) -> Option<usize> {
        self.nodes.iter().position(|node| {
            let Some(element) = &node.element else {
                return false;
            };
            let Some(record) = main_record_of(element) else {
                return false;
            };
            if record.get_load_order_form_id().to_string(false) != form_id {
                return false;
            }
            match file {
                Some(file) => record
                    .get_file()
                    .is_some_and(|owner| owner.get_name().eq_ignore_ascii_case(file)),
                None => true,
            }
        })
    }

    /// Expands the ancestors of a node so its row is visible
    /// (`FullyVisible[Node] := True`), and returns its row.
    pub fn reveal(&mut self, id: usize) -> Option<usize> {
        let mut current = self.nodes.get(id)?.parent;
        while let Some(parent) = current {
            self.nodes[parent].expanded = true;
            if !self.nodes[parent].loaded {
                self.load_children(parent);
            }
            current = self.nodes[parent].parent;
        }
        self.dirty = true;
        self.prepare(self.header_kind);
        self.visible.iter().position(|&visible| visible == id)
    }

    /// `mniNavAddClick`'s menu: `Container.GetAddList` of the focused node.
    ///
    /// A read-only enumeration, like the module list of the module select
    /// dialog; the action itself is the `elements.add` command.
    pub fn add_list(&self, state: &TreeState) -> Vec<String> {
        let Some(row) = state.focused else {
            return Vec::new();
        };
        let Some(&id) = self.visible.get(row) else {
            return Vec::new();
        };
        self.nodes
            .get(id)
            .and_then(|node| node.element.as_ref())
            .map(add_list_of)
            .unwrap_or_default()
    }

    /// The record key of a main record node (`(file, form_id)`), for the
    /// `conflicts.list` requests.
    pub fn record_key(&self, id: usize) -> Option<(String, String)> {
        self.nodes.get(id)?.element.as_ref().and_then(record_key)
    }

    /// The elements a handler walks below a node, records only
    /// (`mniNavSetVWDAutoClick`'s `GetLast`/`GetPrevious`).
    pub fn records_below(&mut self, id: usize) -> Vec<(String, String)> {
        let mut records = Vec::new();
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            if !self.nodes[id].loaded {
                self.load_children(id);
            }
            let Some(element) = self.nodes[id].element.clone() else {
                // A hidden group node: the record above it shows the same
                // children (`ivsHidden`).
                continue;
            };
            if let Some(key) = record_key(&element) {
                records.push(key);
            }
            stack.extend(self.nodes[id].children.iter().rev().copied());
        }
        records
    }
}

fn kind_of(element: &ElementRef) -> NodeKind {
    match element_type(element) {
        ElementType::etFile => NodeKind::File,
        ElementType::etGroupRecord => NodeKind::Group,
        ElementType::etMainRecord => NodeKind::MainRecord,
        _ => NodeKind::Element,
    }
}

/// `(file name, load order FormID)` of a main record.
fn record_key(element: &ElementRef) -> Option<(String, String)> {
    let record = main_record_of(element)?;
    let file = record.get_file()?.get_name();
    Some((file, record.get_load_order_form_id().to_string(false)))
}

/// The `Flags` text of a file (`vstNavGetText`, `wbShowFileFlags`).
fn file_flags(element: &ElementRef) -> String {
    let Some(file) = file_of(element) else {
        return String::new();
    };
    let Some(header) = file.header() else {
        return String::new();
    };
    let flags = header.mr_struct().flags;
    let mut text = String::new();
    if flags.is_esm() {
        text.push_str("<ESM>");
    }
    if flags.is_light() {
        // `wbLightName` of 4.1.5q.
        text.push_str("<Light>");
    }
    if flags.is_medium() {
        text.push_str("<Medium>");
    }
    if flags.is_update() {
        text.push_str("<Update>");
    }
    if flags.is_localized() {
        text.push_str("<Localized>");
    }
    text
}

/// `vstNavGetText` for one column.
///
/// UPSTREAM-QUIRK: the port has no CRC32 accessor for a file, so the third
/// column of a file is empty where upstream prints `_File.CRC32` when it is
/// valid; the file's flags and every other column are the upstream texts.
fn cell_text(element: &ElementRef, column: i32, show_group_record_count: bool, show_file_flags: bool) -> String {
    match element_type(element) {
        ElementType::etGroupRecord => {
            if column < 1 {
                return group_of(element)
                    .map(|group| group.get_short_name())
                    .unwrap_or_default();
            }
            if show_group_record_count
                && column == 2
                && let Some(container) = element.as_container()
                && container.get_element_count() != 0
            {
                return container.get_element_count().to_string();
            }
            String::new()
        }
        ElementType::etMainRecord => {
            let Some(record) = main_record_of(element) else {
                return String::new();
            };
            match column {
                -1 | 0 => {
                    if record.get_signature() == header_signature() {
                        "File Header".to_owned()
                    } else {
                        record.get_load_order_form_id().to_string(true)
                    }
                }
                1 => record.get_editor_id(),
                2 => record.get_display_name(true),
                _ => String::new(),
            }
        }
        ElementType::etFile => match column {
            -1 | 0 => element.get_name(),
            1 => {
                if show_file_flags {
                    file_flags(element)
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        },
        _ => {
            if column < 1 {
                element.get_name()
            } else {
                element.get_value()
            }
        }
    }
}

/// `vstNavChange`'s header titles for the focused node type.
fn header_columns(kind: Option<NodeKind>) -> Vec<TreeColumn> {
    let widths = crate::settings::NAV_COLUMN_WIDTHS;
    let (one, two) = match kind {
        Some(NodeKind::File) => ("Flags", "CRC32"),
        Some(NodeKind::Group) => ("", "Child Count"),
        _ => ("EditorID", "Name"),
    };
    vec![
        TreeColumn::new("FormID", widths[0]),
        TreeColumn::new(one, widths[1]),
        TreeColumn::new(two, widths[2]),
    ]
}

/// `TwbFile.GetAddList`, `TwbGroupRecord.GetAddList` and
/// `TwbMainRecord.GetAddList`: the captions of the `mniNavAdd` submenu.
pub fn add_list_of(element: &ElementRef) -> Vec<String> {
    let mut signatures: Vec<Signature> = Vec::new();
    match element_type(element) {
        ElementType::etFile => {
            if !element.get_is_editable() {
                return Vec::new();
            }
            let Some(file) = file_of(element) else {
                return Vec::new();
            };
            let mut list: Vec<Signature> = wb_group_order();
            // `TStringList.Sorted` with `CaseSensitive = True`: the text
            // order of the signatures.
            list.sort_by_key(|signature| signature.to_string());
            list.dedup();
            list.retain(|signature| {
                !group_to_skip().contains(signature)
                    && !record_to_skip().contains(signature)
                    && file.group_by_signature(*signature).is_none()
            });
            // `Sorted := False` then a descending delete keeps the order.
            list.reverse();
            signatures = list;
        }
        ElementType::etGroupRecord => {
            let Some(group) = group_of(element) else {
                return Vec::new();
            };
            let add = |signatures: &mut Vec<Signature>, signature: &[u8; 4]| {
                signatures.push(Signature::new(signature));
            };
            match group.group_type() {
                0 => add(&mut signatures, &group.gr_struct().label_signature().0),
                1 => {
                    add(&mut signatures, b"CELL");
                    if game_mode() == xedit_core::interface::globals::GameMode::gmTES4 {
                        add(&mut signatures, b"ROAD");
                    }
                }
                7 => add(&mut signatures, b"INFO"),
                6 | 8 | 9 => {
                    for signature in [
                        b"ACHR", b"ACRE", b"REFR", b"PGRE", b"PMIS", b"PARW", b"PBEA", b"PFLA", b"PCON", b"PBAR",
                        b"PHZD",
                    ] {
                        add(&mut signatures, signature);
                    }
                }
                _ => {}
            }
        }
        ElementType::etMainRecord => {
            let Some(record) = main_record_of(element) else {
                return Vec::new();
            };
            if record.get_is_deleted() {
                return Vec::new();
            }
            let mut add = |signature: &[u8; 4]| signatures.push(Signature::new(signature));
            match &record.get_signature().0 {
                b"DIAL" => add(b"INFO"),
                b"CELL" => {
                    for signature in [
                        b"ACHR", b"ACRE", b"REFR", b"PGRE", b"PMIS", b"PARW", b"PBEA", b"PFLA", b"PCON", b"PBAR",
                        b"PHZD",
                    ] {
                        add(signature);
                    }
                    if !record.get_is_persistent() {
                        if let Some(group) = element
                            .get_container()
                            .and_then(|container| container.as_element_impl().and_then(ElementImpl::group_record_impl))
                            && group.group_type() == 5
                        {
                            add(b"LAND");
                        }
                        add(b"PGRD");
                        add(b"NAVM");
                    }
                }
                b"WRLD" => {
                    add(b"CELL");
                    add(b"ROAD");
                }
                b"QUST" if vwd_as_quest_children() => {
                    add(b"DIAL");
                    add(b"DLBR");
                    add(b"SCEN");
                }
                _ => {}
            }
        }
        _ => {
            // `TwbContainer.GetAddList` is empty; the view's Add lists the
            // assign templates instead.
            return Vec::new();
        }
    }
    signatures
        .into_iter()
        .filter_map(|signature| {
            let def = find_record_def(signature)?;
            Some(format!("{} - {}", signature, def.get_name()))
        })
        .collect()
}

impl TreeSource for NavSource {
    fn row_count(&self) -> usize {
        self.visible.len()
    }

    fn columns(&self) -> &[TreeColumn] {
        &self.columns
    }

    fn row(&self, index: usize, out: &mut TreeRow) {
        let Some(&id) = self.visible.get(index) else {
            return;
        };
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        out.id = id as u64;
        out.depth = node.depth;
        out.has_children = node.has_children;
        out.expanded = node.expanded;
        let (conflict_all, conflict_this) = self.node_conflict(node);
        out.conflict_all = conflict_all;
        out.conflict_this = conflict_this;
        let Some(element) = &node.element else {
            return;
        };
        if element_type(element) == ElementType::etFile {
            out.icon = Some('\u{25A4}');
        }
        out.cells = vec![
            TreeCell::new(cell_text(
                element,
                0,
                self.show_group_record_count,
                self.show_file_flags,
            )),
            TreeCell::new(cell_text(
                element,
                1,
                self.show_group_record_count,
                self.show_file_flags,
            )),
            TreeCell::new(cell_text(
                element,
                2,
                self.show_group_record_count,
                self.show_file_flags,
            )),
        ];
    }

    fn toggle(&mut self, index: usize) -> bool {
        let Some(&id) = self.visible.get(index) else {
            return false;
        };
        if !self.nodes[id].has_children {
            return false;
        }
        if !self.nodes[id].loaded {
            self.load_children(id);
        }
        self.nodes[id].expanded = !self.nodes[id].expanded;
        self.dirty = true;
        true
    }

    fn row_tooltip(&self, index: usize) -> Option<&str> {
        let _ = index;
        None
    }
}

// Every row of the tree is painted with the conflict colours of
// `vstNavBeforeItemErase` and `vstNavPaintText` through
// `crate::widgets::conflict`; the navigation pane passes the palette to the
// widget, so no palette lives here.

// ---------------------------------------------------------------------------
// The actions of the navigation menus.
// ---------------------------------------------------------------------------

/// The text `mniNavCleaningObsoleteClick` shows.
pub const CLEANING_OBSOLETE: &str = "This function has been made obsolete by the introduction of Quick Auto Clean mode.\r\n\r\nIf you have used this function because you were following a guide, please be aware that the guide is outdated and no longer applies.\r\n\r\nFor more information about Quick Auto Clean mode, please check the What's New document or the online help (press the help button in the top right corner of the main form).\r\n\r\nYou can hide this function by checking the \"Hide Manual Cleaning functions\" Option.";

/// What a menu item asks the form to do.
#[derive(Clone, Debug, PartialEq)]
pub enum NavAction {
    /// One session command, with the selection of the item resolved into the
    /// request.
    Command {
        command: &'static str,
        params: Value,
        /// The `AddMessage` line of the action, when upstream writes one.
        message: Option<String>,
    },
    /// A `MessageDlg` in front of the actions (`mniNavRemove`,
    /// `mniNavCompactFormIDs`, `mniNavDeleteModGroups`).
    Confirm { message: String, then: Vec<NavAction> },
    /// A dialog of a later phase-7 step ([`dialogs::Placeholder`]).
    Placeholder(dialogs::Placeholder),
    /// A presentation-only item the form handles itself.
    Presentation(&'static str),
    /// A `ShowMessage` or a log line with no command.
    Message(String),
    /// The item applies to nothing in the current selection.
    None,
}

impl NavAction {
    fn command(command: &'static str, params: Value) -> Self {
        NavAction::Command {
            command,
            params,
            message: None,
        }
    }

    fn command_with_message(command: &'static str, params: Value, message: String) -> Self {
        NavAction::Command {
            command,
            params,
            message: Some(message),
        }
    }
}

/// The status a record node's `mniNavHidden` check shows
/// (`mniNavHidden.Checked := esHidden in Element.ElementStates`).
pub fn hidden_checked(nav: &NavSource, state: &TreeState) -> bool {
    nav.focused_element(state)
        .is_some_and(|element| xedit_core::implementation::sortable::is_hidden(&element))
}

/// Resolves a `pmuNav` or `pmuNavHeaderPopup` item into the actions it runs.
///
/// `edit_allowed` is `wbEditAllowed`; without it the mutating items behave as
/// upstream (they do nothing), while the session refuses the command itself
/// too.
pub fn nav_actions(item: &MenuItem, selection: &Selection, edit_allowed: bool) -> Vec<NavAction> {
    let id = item.id;
    let files = selection.files();
    let records = selection.records();
    let mut actions = Vec::new();
    match id {
        // The header sorts are presentation (`mniNavHeaderFilesClick` and
        // `mniNavHeaderINFOClick` write the setting and sort the roots).
        "mniNavHeaderFilesDefault" | "mniNavHeaderFilesLoadOrder" | "mniNavHeaderFilesFileName" => {
            actions.push(NavAction::Presentation(id))
        }
        "mniNavHeaderINFObyPreviousINFO" | "mniNavHeaderINFObyFormID" => actions.push(NavAction::Presentation(id)),
        // pmuNav.
        "mniNavCompareTo" => actions.push(NavAction::Placeholder(dialogs::show_file_select("Compare to..."))),
        "mniNavCreateDeltaPatch" => actions.push(NavAction::Placeholder(dialogs::show_file_select(
            "Please select a newer version to create a delta patch...",
        ))),
        "mniNavCompareSelected" => {
            let active = selection.active_records();
            if active.len() >= 2 {
                let records: Vec<Value> = active
                    .iter()
                    .map(|record| json!({ "form_id": record.form_id, "file": record.file }))
                    .collect();
                actions.push(NavAction::command(
                    "records.compare_selected",
                    json!({ "records": records }),
                ));
            }
        }
        "mniNavFilterRemove" => actions.push(NavAction::command("filter.remove", json!({}))),
        "mniNavFilterApply" | "mniNavFilterApplySelected" => {
            actions.push(NavAction::Placeholder(dialogs::show_filter_options()))
        }
        "mniNavFilterForCleaning" | "mniNavFilterForCleaningSelected" => {
            // The cleaning walk runs over the selected nodes; the command
            // cleans the plugins those nodes belong to.
            let plugins = selection.touched_files();
            if edit_allowed && !plugins.is_empty() {
                actions.push(NavAction::command(
                    "files.clean",
                    json!({ "files": plugins, "itm": true, "udr": true }),
                ));
            }
        }
        "mniNavFilterForCleaningObsolete"
        | "mniNavFilterForCleaningSelectedObsolete"
        | "mniNavUndeleteAndDisableReferencesObsolete"
        | "mniNavRemoveIdenticalToMasterObsolete" => actions.push(NavAction::Message(CLEANING_OBSOLETE.to_owned())),
        "mniNavFilterConflicts" => {
            // `mniNavFilterConflictsClick`: the conflict statuses
            // `[ctIdenticalToMasterWinsConflict, ctConflictWins,
            // ctConflictLoses]`, the flattening and inheritance on, no mod
            // groups, `FilterConflictOnly` from `xeVeryQuickShowConflicts`.
            actions.push(NavAction::command(
                "filter.apply",
                json!({
                    "options": {
                        "conflict_this": ["ctIdenticalToMasterWinsConflict", "ctConflictWins", "ctConflictLoses"],
                        "flatten_blocks": true,
                        "flatten_cell_childs": true,
                        "assign_pers_wrld_child": true,
                        "inherit_conflict_by_parent": true
                    },
                    "preset": null,
                    // `FilterConflictOnly` and `FilterNoGameMaster` follow
                    // `xeVeryQuickShowConflicts`, off outside that mode.
                    "conflict_only": false,
                    "no_game_master": false,
                    "list_records": false
                }),
            ));
        }
        "mniNavFilterConflictsSelected" => actions.push(NavAction::Placeholder(dialogs::show_file_select(
            "What files should be added to the navigation treeview before applying the filter?",
        ))),
        "mniNavCheckForErrors" => {
            if edit_allowed {
                if !files.is_empty() {
                    actions.push(NavAction::command("files.check", json!({ "files": files })));
                }
                // `CheckForErrorsLinear` over each node: the records of a
                // plugin are checked as that plugin's version.
                let mut record_files: Vec<String> = Vec::new();
                for (_, file) in &records {
                    if let Some(file) = file
                        && !record_files.contains(file)
                    {
                        record_files.push(file.clone());
                    }
                }
                for file in record_files {
                    let record_ids: Vec<String> = records
                        .iter()
                        .filter(|(_, record_file)| record_file.as_deref() == Some(file.as_str()))
                        .map(|(form_id, _)| form_id.clone())
                        .collect();
                    actions.push(NavAction::command(
                        "files.check",
                        json!({ "records": record_ids, "record_file": file }),
                    ));
                }
            }
        }
        "mniNavCheckForCircularLeveledLists" => {
            if edit_allowed {
                actions.push(NavAction::command("leveled_lists.check_circular", json!({})));
            }
        }
        "mniNavChangeFormID" => actions.push(NavAction::Placeholder(dialogs::prompt_new_form_id())),
        "mniNavChangeReferencingRecords" => actions.push(NavAction::Placeholder(dialogs::prompt_new_form_id())),
        "mniNavRenumberFormIDsFrom" | "mniNavRenumberFormIDsInject" => {
            actions.push(NavAction::Placeholder(dialogs::show_module_select(id)))
        }
        "mniNavCompactFormIDs" => {
            if edit_allowed && let Some(file) = selection.focused_file().or_else(|| files.first().cloned()) {
                actions.push(NavAction::Confirm {
                    message: format!("This operation will modify the FormID of every record of {file} that has no master yet. Continue?"),
                    then: vec![NavAction::command(
                        "formids.renumber",
                        json!({ "file": file, "compact": true }),
                    )],
                });
            }
        }
        "mniNavApplyScript" => actions.push(NavAction::Placeholder(dialogs::show_script_select())),
        "mniNavUndeleteAndDisableReferences" => {
            if edit_allowed && !files.is_empty() {
                actions.push(NavAction::command(
                    "files.clean",
                    json!({ "files": files, "udr": true }),
                ));
            }
        }
        "mniNavRemoveIdenticalToMaster" => {
            if edit_allowed && !files.is_empty() {
                actions.push(NavAction::command(
                    "files.clean",
                    json!({ "files": files, "itm": true }),
                ));
            }
        }
        "mniNavLOManagersDirtyInfo" => actions.push(NavAction::command("files.dirty_info", json!({}))),
        "mniNavSetVWDAuto" => {
            if edit_allowed {
                let record_ids: Vec<String> = records.iter().map(|(form_id, _)| form_id.clone()).collect();
                actions.push(NavAction::command(
                    "references.set_vwd_auto",
                    json!({ "files": files, "records": record_ids }),
                ));
            }
        }
        "mniNavSetVWDAutoInto" => actions.push(NavAction::Placeholder(dialogs::show_module_select(
            "Set VWD for all REFR with VWD Mesh as Override into....",
        ))),
        "mniNavCellChildTemp" | "mniNavCellChildPers" | "mniNavCellChildNotVWD" | "mniNavCellChildVWD" => {
            if edit_allowed {
                let persistent = id == "mniNavCellChildPers";
                let visible_when_distant = id == "mniNavCellChildVWD";
                let command = if id == "mniNavCellChildTemp" || id == "mniNavCellChildPers" {
                    "records.set_persistent"
                } else {
                    "records.set_visible_when_distant"
                };
                for (form_id, file) in records {
                    let params = if command == "records.set_persistent" {
                        json!({ "form_id": form_id, "file": file, "persistent": persistent })
                    } else {
                        json!({
                            "form_id": form_id,
                            "file": file,
                            "visible_when_distant": visible_when_distant
                        })
                    };
                    actions.push(NavAction::command(command, params));
                }
            }
        }
        "mniCreateNewFile" => actions.push(NavAction::Placeholder(dialogs::show_new_file_dialog())),
        // `mniNavAdd` is the list `Container.GetAddList` gives, drawn by the
        // form (`add_item_action` per entry); there is no item of its own.
        "mniNavAdd" => actions.push(NavAction::None),
        "mniNavRemove" => {
            if edit_allowed {
                let removable: Vec<(String, Option<String>)> = selection
                    .nodes
                    .iter()
                    .filter(|node| node.editable && node.is_record())
                    .filter_map(|node| node.record())
                    .collect();
                if !removable.is_empty() {
                    let message = if removable.len() == 1 {
                        format!(
                            "Are you sure you want to permanently remove {}?",
                            selection.nodes[0].element_name
                        )
                    } else {
                        format!(
                            "Are you sure you want to permanently remove the {} removable selected records?",
                            removable.len()
                        )
                    };
                    let then = removable
                        .into_iter()
                        .map(|(form_id, file)| {
                            NavAction::command("records.delete", json!({ "form_id": form_id, "file": file }))
                        })
                        .collect();
                    actions.push(NavAction::Confirm { message, then });
                }
            }
        }
        "mniNavMarkModified" => {
            if edit_allowed {
                for node in selection.editable() {
                    if let (Some(form_id), Some(file)) = (node.form_id.clone(), node.file.clone()) {
                        actions.push(NavAction::command(
                            "elements.mark_modified",
                            json!({ "form_id": form_id, "file": file }),
                        ));
                    }
                }
            }
        }
        "mniNavAddMasters" => actions.push(NavAction::Placeholder(dialogs::show_module_select(
            "Which masters do you want to add?",
        ))),
        "mniNavSortMasters" => {
            if edit_allowed {
                for file in files {
                    actions.push(NavAction::command("masters.sort", json!({ "file": file })));
                }
            }
        }
        "mniNavCleanMasters" => {
            if edit_allowed {
                for file in files {
                    actions.push(NavAction::command("masters.clean", json!({ "file": file })));
                }
            }
        }
        "mniNavCreateModGroup" | "mniNavEditModGroup" => {
            actions.push(NavAction::Placeholder(dialogs::show_modgroup_editor(id)))
        }
        "mniNavDeleteModGroups" => actions.push(NavAction::Confirm {
            message: "Are you sure you want to delete the following ModGroups?\r\n\r\nThis can not be undone!"
                .to_owned(),
            then: vec![NavAction::Placeholder(dialogs::show_modgroup_select(
                "Which ModGroups do you want to delete?",
            ))],
        }),
        "mniNavUpdateCRCModGroups" => actions.push(NavAction::Placeholder(dialogs::show_modgroup_select(
            "Please select the modules for which you want to add or update the current CRC to/in ModGroup Items...",
        ))),
        "mniNavCopyAsOverride"
        | "mniNavCopyAsOverrideWithOverwrite"
        | "mniNavDeepCopyAsOverride"
        | "mniNavDeepCopyAsOverrideWithOverwriting"
        | "mniNavCopyAsNewRecord"
        | "mniNavCopyAsSpawnRateOverride"
        | "mniNavCopyAsWrapper" => {
            if edit_allowed && !records.is_empty() {
                actions.push(NavAction::Placeholder(dialogs::select_copy_target_file(id)));
            }
        }
        "mniNavCleanupInjected" => {
            if edit_allowed {
                let file = selection
                    .focused
                    .as_ref()
                    .and_then(|node| node.file.clone())
                    .or_else(|| files.first().cloned());
                let form_ids: Vec<String> = records
                    .iter()
                    .filter(|(_, record_file)| record_file.as_deref() == file.as_deref())
                    .map(|(form_id, _)| form_id.clone())
                    .collect();
                if let Some(file) = file {
                    actions.push(NavAction::command(
                        "records.cleanup_injected",
                        json!({ "file": file, "form_ids": form_ids }),
                    ));
                }
            }
        }
        "mniNavCopyIdle" => actions.push(NavAction::Placeholder(dialogs::show_file_select(
            "Which Idles do you want to copy?",
        ))),
        "mniNavHidden" => {
            // `mniNavHidden.Checked` is the focused element's state; the
            // handler sets every selected element to it.
            if let Some(focused) = selection.focused.as_ref().filter(|node| node.form_id.is_some()) {
                let hidden = !focused.hidden;
                for node in selection.nodes.iter().filter(|node| node.form_id.is_some()) {
                    actions.push(NavAction::command(
                        "records.set_hidden",
                        json!({ "form_id": node.form_id, "file": node.file, "hidden": hidden }),
                    ));
                }
            }
        }
        "mniNavTest" => actions.push(NavAction::Message(
            "The Test menu item has no function: upstream's handler is commented out.".to_owned(),
        )),
        "mniNavBanditFix" => actions.push(NavAction::command("records.bandit_fix", json!({}))),
        // pmuNav's Other submenu.
        "mniNavCreateMergedPatch" => actions.push(NavAction::Placeholder(dialogs::select_patch_target_file())),
        "mniNavCreateSEQFile" => {
            if !files.is_empty() {
                actions.push(NavAction::command_with_message(
                    "tool.run",
                    json!({ "mode": "generateseq", "files": files }),
                    "[Create SEQ file done]".to_owned(),
                ));
            }
        }
        "mniNavGenerateLOD" => actions.push(NavAction::Placeholder(dialogs::show_lodgen())),
        "mniNavBuildRef" => actions.push(NavAction::Placeholder(dialogs::show_module_select(
            "Build reference information for:",
        ))),
        "mniNavBuildReachable" => actions.push(NavAction::command("refs.build_reachable", json!({}))),
        "mniNavBatchChangeReferencingRecords" => actions.push(NavAction::Placeholder(dialogs::show_file_select(
            "The CSV file of the batch reference change...",
        ))),
        "mniNavRaceLVLIs" => {
            if edit_allowed {
                actions.push(NavAction::command("records.race_leveled_lists", json!({})));
            }
        }
        "mniNavLocalizationSwitch" => actions.push(NavAction::Placeholder(dialogs::show_localizer())),
        "mniNavLogAnalyzer" => actions.push(NavAction::Placeholder(dialogs::show_log_analyzer())),
        "mniNavOptions" => actions.push(NavAction::Placeholder(dialogs::show_options())),
        "mniNavOtherCodeSiteLogging" => actions.push(NavAction::Presentation("codesite")),
        "mniMarkallfileswithoutONAMasmodified" => {
            if edit_allowed {
                actions.push(NavAction::command("files.mark_modified_headers", json!({})));
            }
        }
        // pmuMain's covered bindings that belong to this step.
        "mniMainSave" => actions.push(NavAction::command("files.save", json!({}))),
        "mniMainOptions" => actions.push(NavAction::Placeholder(dialogs::show_options())),
        _ => actions.push(NavAction::None),
    }
    actions
}

/// `mniNavAdd`'s generated child: `elements.add` with the caption of the
/// item (`mniNavAddClick` uses the focused node's element as the container).
pub fn add_item_action(caption: &str, selection: &Selection) -> NavAction {
    let Some(node) = selection
        .focused
        .as_ref()
        .filter(|node| node.form_id.is_some())
        .or_else(|| selection.nodes.iter().find(|node| node.is_record()))
    else {
        return NavAction::None;
    };
    NavAction::command(
        "elements.add",
        json!({ "form_id": node.form_id, "file": node.file, "name": caption }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_model::{self, MenuKind};

    fn selection_of(nodes: Vec<SelectedNode>) -> Selection {
        Selection { nodes, focused: None }
    }

    fn file_node(name: &str) -> SelectedNode {
        SelectedNode {
            kind: NodeKind::File,
            element_name: name.to_owned(),
            file: Some(name.to_owned()),
            editable: true,
            ..SelectedNode::default()
        }
    }

    fn record_node(file: &str, form_id: &str) -> SelectedNode {
        SelectedNode {
            kind: NodeKind::MainRecord,
            element_name: "Record".to_owned(),
            file: Some(file.to_owned()),
            form_id: Some(form_id.to_owned()),
            signature: Some("REFR".to_owned()),
            editable: true,
            ..SelectedNode::default()
        }
    }

    #[test]
    fn an_empty_tree_has_no_rows() {
        let mut nav = NavSource::new(Vec::new());
        nav.prepare(None);
        assert_eq!(TreeSource::row_count(&nav), 0);
        assert!(!TreeSource::toggle(&mut nav, 0));
        assert_eq!(nav.path_text(&TreeState::default()), "");
        assert!(nav.pending_conflicts().is_empty());
    }

    #[test]
    fn the_header_titles_follow_the_focused_node() {
        assert_eq!(header_columns(Some(NodeKind::File))[1].title, "Flags");
        assert_eq!(header_columns(Some(NodeKind::File))[2].title, "CRC32");
        assert_eq!(header_columns(Some(NodeKind::Group))[1].title, "");
        assert_eq!(header_columns(Some(NodeKind::Group))[2].title, "Child Count");
        assert_eq!(header_columns(Some(NodeKind::MainRecord))[1].title, "EditorID");
        assert_eq!(header_columns(None)[2].title, "Name");
    }

    #[test]
    fn the_search_states_have_the_upstream_colours() {
        assert_eq!(SearchState::Normal.color(false), None);
        assert_eq!(
            SearchState::Found.color(false),
            Some(wb_lighter(eframe::egui::Color32::from_rgb(0, 255, 0), 0.85, false))
        );
        assert_eq!(
            SearchState::NotFound.color(false),
            Some(wb_lighter(eframe::egui::Color32::from_rgb(255, 0, 0), 0.85, false))
        );
    }

    fn a_full_selection() -> Selection {
        Selection {
            nodes: vec![
                file_node("Update.esm"),
                record_node("Update.esm", "000001"),
                record_node("Update.esm", "000002"),
            ],
            focused: Some(record_node("Update.esm", "000001")),
        }
    }

    #[test]
    fn every_nav_item_with_a_command_dispatches_it_or_names_a_placeholder() {
        for tree in [&menu_model::PMU_NAV, &menu_model::PMU_NAV_HEADER_POPUP] {
            menu_model::walk(tree.items, &mut |item| {
                if item.kind != MenuKind::Action || item.command.is_none() {
                    return;
                }
                // `mniNavAdd` opens the list `Container.GetAddList` gives;
                // its items call `elements.add` (see `add_item_action`).
                if item.id == "mniNavAdd" {
                    match add_item_action("REFR - Placed Object", &a_full_selection()) {
                        NavAction::Command { command, .. } => assert_eq!(command, "elements.add"),
                        other => panic!("mniNavAdd: unexpected {other:?}"),
                    }
                    return;
                }
                let actions = nav_actions(item, &a_full_selection(), true);
                let mut found = false;
                for action in &actions {
                    match action {
                        NavAction::Command { command, .. } => found |= *command == item.command.unwrap(),
                        NavAction::Placeholder(_) => found = true,
                        NavAction::Confirm { then, .. } => {
                            found |= then.iter().any(|action| {
                                matches!(action, NavAction::Command { command, .. } if *command == item.command.unwrap())
                            }) || then.iter().any(|action| matches!(action, NavAction::Placeholder(_)));
                        }
                        _ => {}
                    }
                }
                assert!(
                    found,
                    "{} ({:?}) dispatches neither {} nor a placeholder",
                    item.id,
                    item.caption,
                    item.command.unwrap()
                );
            });
        }
    }

    #[test]
    fn every_pmu_nav_item_has_an_action() {
        menu_model::walk(menu_model::PMU_NAV.items, &mut |item| {
            if item.kind != MenuKind::Action || item.id == "mniNavAdd" {
                return;
            }
            let actions = nav_actions(item, &a_full_selection(), true);
            assert!(
                !matches!(actions.first(), None | Some(NavAction::None)),
                "{} has no action",
                item.id
            );
        });
    }

    fn assert_params<T: serde::de::DeserializeOwned>(command: &str, params: &Value) {
        if let Err(error) = serde_json::from_value::<T>(params.clone()) {
            panic!("{command}: {error}");
        }
    }

    #[test]
    fn every_nav_command_params_match_its_request_schema() {
        for tree in [&menu_model::PMU_NAV, &menu_model::PMU_NAV_HEADER_POPUP] {
            menu_model::walk(tree.items, &mut |item| {
                if item.kind != MenuKind::Action {
                    return;
                }
                let mut actions = nav_actions(item, &a_full_selection(), true);
                if item.id == "mniNavAdd" {
                    actions = vec![add_item_action("REFR - Placed Object", &a_full_selection())];
                }
                for action in &actions {
                    let (command, params) = match action {
                        NavAction::Command { command, params, .. } => (*command, params),
                        NavAction::Confirm { then, .. } => {
                            for action in then {
                                if let NavAction::Command { command, params, .. } = action {
                                    assert_params_for(command, params);
                                }
                            }
                            continue;
                        }
                        _ => continue,
                    };
                    assert_params_for(command, params);
                }
            });
        }
    }

    fn assert_params_for(command: &str, params: &Value) {
        use xedit_session as session;
        match command {
            "records.compare_selected" => assert_params::<session::conflicts::CompareSelectedRequest>(command, params),
            "filter.apply" => assert_params::<session::filter::FilterApplyRequest>(command, params),
            "filter.remove" => assert_params::<session::filter::FilterRemoveRequest>(command, params),
            "files.clean" => assert_params::<session::clean::CleanRequest>(command, params),
            "files.check" => assert_params::<session::check::CheckRequest>(command, params),
            "files.dirty_info" => assert_params::<session::clean::DirtyInfoRequest>(command, params),
            "leveled_lists.check_circular" => {
                assert_params::<session::leveled_lists::CheckCircularRequest>(command, params)
            }
            "references.set_vwd_auto" => assert_params::<session::refs::SetVwdAutoRequest>(command, params),
            "records.set_persistent" => assert_params::<session::edit::RecordsSetPersistentRequest>(command, params),
            "records.set_visible_when_distant" => {
                assert_params::<session::edit::RecordsSetVisibleWhenDistantRequest>(command, params)
            }
            "records.delete" => assert_params::<session::edit::RecordsDeleteRequest>(command, params),
            "elements.mark_modified" => assert_params::<session::edit::ElementsMarkModifiedRequest>(command, params),
            "masters.sort" | "masters.clean" => assert_params::<session::masters::MastersRequest>(command, params),
            "records.cleanup_injected" => assert_params::<session::clean::CleanupInjectedRequest>(command, params),
            "records.set_hidden" => assert_params::<session::commands::RecordsSetHiddenRequest>(command, params),
            "tool.run" => assert_params::<session::tool_modes::ToolRunRequest>(command, params),
            "refs.build_reachable" => assert_params::<session::filter::BuildReachableRequest>(command, params),
            "records.bandit_fix" => assert_params::<session::oblivion::BanditFixRequest>(command, params),
            "records.race_leveled_lists" => {
                assert_params::<session::oblivion::RaceLeveledListsRequest>(command, params)
            }
            "files.mark_modified_headers" => {
                assert_params::<session::edit::FilesMarkModifiedHeadersRequest>(command, params)
            }
            "formids.renumber" => assert_params::<session::formids::FormIdsRenumberRequest>(command, params),
            "elements.add" => assert_params::<session::edit::ElementsAddRequest>(command, params),
            other => panic!("{other} has no schema check in this test"),
        }
    }

    #[test]
    fn the_cell_child_items_change_each_selected_record() {
        let item = menu_model::item(&menu_model::PMU_NAV, "mniNavCellChildPers").unwrap();
        let selection = selection_of(vec![record_node("A.esm", "000001"), record_node("B.esp", "000002")]);
        let actions = nav_actions(item, &selection, true);
        assert_eq!(actions.len(), 2);
        match &actions[0] {
            NavAction::Command { command, params, .. } => {
                assert_eq!(*command, "records.set_persistent");
                assert_eq!(params["persistent"], json!(true));
                assert_eq!(params["form_id"], json!("000001"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn the_cleaning_items_pass_the_selected_files() {
        let item = menu_model::item(&menu_model::PMU_NAV, "mniNavUndeleteAndDisableReferences").unwrap();
        let actions = nav_actions(item, &selection_of(vec![file_node("A.esm"), file_node("B.esp")]), true);
        match &actions[0] {
            NavAction::Command { command, params, .. } => {
                assert_eq!(*command, "files.clean");
                assert_eq!(params["files"], json!(["A.esm", "B.esp"]));
                assert_eq!(params["udr"], json!(true));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn without_edit_the_mutating_items_do_nothing() {
        for id in ["mniNavRemove", "mniNavCellChildPers", "mniNavCompactFormIDs"] {
            let item = menu_model::item(&menu_model::PMU_NAV, id).unwrap();
            let selection = selection_of(vec![file_node("A.esm"), record_node("A.esm", "000001")]);
            let actions = nav_actions(item, &selection, false);
            assert!(
                actions.iter().all(|action| matches!(
                    action,
                    NavAction::None | NavAction::Placeholder(_) | NavAction::Message(_)
                )),
                "{id}: {actions:?}"
            );
        }
    }
}
