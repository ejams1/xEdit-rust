// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (vstNav.GetSortedSelection,
// TNavNodeData, EditableSelection, RemovableSelection, GetFocusedNavElement)

//! The selection of the navigation tree, shared by everything it drives.
//!
//! Upstream every action reads `vstNav.GetSortedSelection(True)` (the
//! selected nodes in tree order) or `vstNav.FocusedNode`, and takes the
//! element and container out of each node's `TNavNodeData`. The port keeps
//! the same information in a small plain model so the view grid, the record
//! panes and the menu dispatch read the selection without knowing the tree's
//! node storage.
//!
//! [`crate::nav_tree::NavSource::selection`] builds it from the widget's
//! [`crate::widgets::virtual_tree::TreeState`]; [`Selection`] itself is pure
//! data and has no egui dependency, so the dispatch of the context menus is
//! testable headlessly.

/// What a selected navigation node holds, in the order of the tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NodeKind {
    /// A loaded file (a root node, `etFile`).
    File,
    /// A group record (`etGroupRecord`).
    Group,
    /// A main record (`etMainRecord`).
    MainRecord,
    /// Any other element a container's children show (a cell child, a
    /// record header, a sub-record).
    #[default]
    Element,
}

/// One selected node, as the handlers of `pmuNav` read a `TNavNodeData`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectedNode {
    pub kind: NodeKind,
    /// `Element.Name`.
    pub element_name: String,
    /// The file the element belongs to (`Element._File`).
    pub file: Option<String>,
    /// `MainRecord.LoadOrderFormID` for a main record, as hexadecimal.
    pub form_id: Option<String>,
    /// `MainRecord.Signature`.
    pub signature: Option<String>,
    /// `MainRecord.EditorID`.
    pub editor_id: Option<String>,
    /// `MainRecord.DisplayName[True]`.
    pub record_name: Option<String>,
    /// Whether `NodeData.Container` has elements (`ContainsChilds` of
    /// `RemovableSelection` and the remove confirmation).
    pub has_children: bool,
    /// Whether the element may be edited (`Element.IsEditable`).
    pub editable: bool,
    /// `esHidden in Element.ElementStates`.
    pub hidden: bool,
}

impl SelectedNode {
    pub fn is_file(&self) -> bool {
        self.kind == NodeKind::File
    }

    pub fn is_record(&self) -> bool {
        self.kind == NodeKind::MainRecord
    }

    /// The record a command names: its load order FormID and the plugin
    /// whose version it is.
    pub fn record(&self) -> Option<(String, Option<String>)> {
        Some((self.form_id.clone()?, self.file.clone()))
    }
}

/// `vstNav.GetSortedSelection(True)` plus the focused node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// The selected nodes, in the display order of the tree.
    pub nodes: Vec<SelectedNode>,
    /// `vstNav.FocusedNode` (`GetFocusedNavElementSafely`).
    pub focused: Option<SelectedNode>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.focused.is_none()
    }

    /// `GetSortedSelection` with the file nodes only, as
    /// `PerformActionOnSelectedFiles` keeps them.
    pub fn files(&self) -> Vec<String> {
        let mut files = Vec::new();
        for node in &self.nodes {
            if node.is_file()
                && let Some(name) = &node.file
                && !files.contains(name)
            {
                files.push(name.clone());
            }
        }
        files
    }

    /// The file nodes of the focused node too, when nothing is selected
    /// (upstream's `PerformActionOnSelectedFiles` reads the selection, but
    /// the focused node is what the `ReInitTree`/renumber handlers read).
    pub fn focused_file(&self) -> Option<String> {
        self.focused
            .as_ref()
            .filter(|node| node.is_file())
            .and_then(|node| node.file.clone())
    }

    /// The selected main records, in tree order, as `(form_id, file)`.
    pub fn records(&self) -> Vec<(String, Option<String>)> {
        self.nodes
            .iter()
            .filter(|node| node.is_record())
            .filter_map(SelectedNode::record)
            .collect()
    }

    /// The records of the focused node and its selection, without
    /// duplicates (the view tab shows these).
    pub fn active_records(&self) -> Vec<SelectedNode> {
        let mut records: Vec<SelectedNode> = Vec::new();
        for node in &self.nodes {
            if node.is_record()
                && !records
                    .iter()
                    .any(|seen| seen.form_id == node.form_id && seen.file == node.file)
            {
                records.push(node.clone());
            }
        }
        if records.is_empty()
            && let Some(node) = &self.focused
            && node.is_record()
        {
            records.push(node.clone());
        }
        records
    }

    /// `ContainsChilds` of `RemovableSelection`/`EditableSelection`.
    pub fn contains_childs(&self) -> bool {
        self.nodes.iter().any(|node| node.has_children)
    }

    /// `EditableSelection`: the nodes whose element may be edited.
    pub fn editable(&self) -> Vec<&SelectedNode> {
        self.nodes.iter().filter(|node| node.editable).collect()
    }

    /// Every file a selected node belongs to, in tree order.
    pub fn touched_files(&self) -> Vec<String> {
        let mut files = Vec::new();
        for node in &self.nodes {
            if let Some(name) = &node.file
                && !files.contains(name)
            {
                files.push(name.clone());
            }
        }
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> SelectedNode {
        SelectedNode {
            kind: NodeKind::File,
            element_name: name.to_owned(),
            file: Some(name.to_owned()),
            editable: true,
            ..SelectedNode::default()
        }
    }

    fn record(file: &str, form_id: &str) -> SelectedNode {
        SelectedNode {
            kind: NodeKind::MainRecord,
            element_name: "Record".to_owned(),
            file: Some(file.to_owned()),
            form_id: Some(form_id.to_owned()),
            signature: Some("NPC_".to_owned()),
            editable: true,
            ..SelectedNode::default()
        }
    }

    #[test]
    fn files_and_records_come_out_in_tree_order() {
        let selection = Selection {
            nodes: vec![record("A.esm", "000001"), file("A.esm"), record("B.esp", "000002")],
            focused: None,
        };
        assert_eq!(selection.files(), vec!["A.esm".to_owned()]);
        assert_eq!(
            selection.records(),
            vec![
                ("000001".to_owned(), Some("A.esm".to_owned())),
                ("000002".to_owned(), Some("B.esp".to_owned()))
            ]
        );
        assert_eq!(selection.touched_files(), vec!["A.esm".to_owned(), "B.esp".to_owned()]);
    }

    #[test]
    fn the_focused_record_stands_in_for_an_empty_selection() {
        let selection = Selection {
            nodes: Vec::new(),
            focused: Some(record("A.esm", "000001")),
        };
        assert_eq!(selection.active_records().len(), 1);
        assert_eq!(selection.focused_file(), None);
    }

    #[test]
    fn duplicates_are_dropped() {
        let selection = Selection {
            nodes: vec![record("A.esm", "000001"), record("A.esm", "000001")],
            focused: None,
        };
        assert_eq!(selection.active_records().len(), 1);
    }
}
