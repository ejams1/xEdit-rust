// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (DoSetActiveRecord,
// DoSetActiveRecord(aMainRecords), DoSetActiveContainer, InitNodes,
// InitChildren, ExpandView, InitConflictStatus, vstViewGetText,
// vstViewGetEditText, vstViewNewText, vstViewEditing, vstViewBeforeCellPaint,
// vstViewPaintText, mniViewSortClick, UpdateColumnWidths) and
// xEdit/xeMainForm.dfm (vstView with the Labels and Values columns,
// toEditable, toEditOnClick and the filler column, fpnlViewFilter)

//! The view tab: `vstView` over one record or a set of records.
//!
//! `DoSetActiveRecord` (`xeMainForm.pas:17089-17233`) shows a record with one
//! column per version of it (the master and its overrides) headed by the
//! file name; `DoSetActiveRecord(aMainRecords)` (`:16807-16910`) shows the
//! records the user selected, one column each, headed
//! `EditorID [FileName]`. Both end in a filler column when more than one
//! record is shown. The rows are the members of the definition
//! (`MemberCount + AdditionalElementCount`) and, below them, the children the
//! definitions give each member (`InitNodes`/`InitChildren`).
//!
//! The rows and cells come from the `records.compare` and
//! `records.compare_selected` commands (design rule 1: the GUI calls the
//! session commands and nothing else), which return what `vstViewGetText`
//! computes: the display name of every column's element, its value or
//! summary, its conflict status, the path `elements.set` takes to edit it and
//! the edit text. This module turns that response into the grid model, keeps
//! the expansion state and routes an in-place edit back to the command layer.
//!
//! The model and the edit routing are independent of the egui context and
//! are tested headlessly; the drawing goes through
//! [`crate::widgets::grid`].

use std::collections::HashSet;

use xedit_core::interface::types::{ConflictAll, ConflictThis};
use xedit_session::conflicts::{CompareResponse, CompareRow, CompareSelectedResponse};

use crate::settings::ColumnWidthMode;
use crate::widgets::grid::{GridCell, GridColumn, GridSource, GridState};

/// `Options.ColumnWidth` of xEdit's settings file: the width of a view
/// column in the standard mode.
pub const COLUMN_WIDTH: f32 = 200.0;
/// The width of the fixed label column before the record columns are built
/// (`vstView`'s static `Labels` column).
pub const LABEL_COLUMN_WIDTH: f32 = 250.0;
/// The width of `vstView`'s static `Values` column.
pub const VALUES_COLUMN_WIDTH: f32 = 233.0;

/// `fpnlViewFilter` of the DFM: `edViewFilterName`, `cobViewFilter` (and/or),
/// `edViewFilterValue` and the three "Keep" boxes. The filter runs in the
/// command layer (`ApplyViewFilter`), so a change builds the view again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewFilterState {
    /// `edViewFilterName`: the row name must contain this text.
    pub name: String,
    /// `edViewFilterValue`: a cell value must contain this text.
    pub value: String,
    /// `cobViewFilter` at 1: either text matches (`or`), not both (`and`).
    pub or: bool,
    /// `cbViewFilterKeepChildren`.
    pub keep_children: bool,
    /// `cbViewFilterKeepSiblings`.
    pub keep_siblings: bool,
    /// `cbViewFilterKeepParentsSiblings`.
    pub keep_parents_siblings: bool,
}

impl ViewFilterState {
    /// Whether the filter hides anything: `edViewFilterChange` is a no-op
    /// while both texts are empty.
    pub fn active(&self) -> bool {
        !self.name.is_empty() || !self.value.is_empty()
    }
}

/// One record the view shows, as the navigation tree names it:
/// a load order FormID and the plugin it is seen from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewRecord {
    /// Load order FormID as eight hexadecimal digits.
    pub form_id: String,
    /// Plugin that holds this version of the record.
    pub file: String,
}

/// What the view is showing (`DoSetActiveRecord`, `DoSetActiveRecord`
/// with a list of records, or nothing).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewKind {
    /// `DoSetActiveRecord(nil)`: one filler column and no rows.
    #[default]
    Empty,
    /// `DoSetActiveRecord(aMainRecord)`: the master and its overrides.
    Single,
    /// `DoSetActiveRecord(aMainRecords)`: the selected records side by side
    /// (`ComparingSiblings`).
    Multiple,
}

/// A column of the view (`vstView.Header.Columns[i + 1]`).
#[derive(Clone, Debug, PartialEq)]
pub struct ViewColumn {
    /// The header text: the file name of a single record's version, or
    /// `EditorID [FileName]` for a selected record.
    pub title: String,
    /// File that holds the column's record.
    pub file: String,
    /// Load order FormID of the column's record.
    pub form_id: String,
}

/// One cell of a row: an element of one record.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewCell {
    /// `vstViewGetText`: the value, or the summary of a collapsed row.
    pub value: Option<String>,
    /// `Element.Summary`, shown when the value is empty and the row is
    /// collapsed.
    pub summary: Option<String>,
    /// The path `elements.get`/`elements.set` take to the element.
    pub path: Option<String>,
    /// `Element.IsEditable`.
    pub editable: bool,
    /// `Element.EditValue`, the text the in-place editor starts with.
    pub edit_value: Option<String>,
    /// `Element.DisplayName[True]`, which the label column shows while this
    /// column has the focus.
    pub display_name: Option<String>,
    /// `Element.Name`, for the clipboard items.
    pub name: Option<String>,
    /// `DisplaySortKey[True]` of a sibling comparison.
    pub sort_key: Option<String>,
    /// `NodeDatas[Column].ConflictThis`.
    pub conflict_this: ConflictThis,
    /// `NodeDatas[Column].ConflictAll`.
    pub conflict_all: ConflictAll,
}

impl Default for ViewCell {
    fn default() -> Self {
        Self {
            value: None,
            summary: None,
            path: None,
            editable: false,
            edit_value: None,
            display_name: None,
            name: None,
            sort_key: None,
            conflict_this: ConflictThis::ctUnknown,
            conflict_all: ConflictAll::caUnknown,
        }
    }
}

/// One node of the view's tree (`InitNodes`).
#[derive(Clone, Debug, PartialEq)]
pub struct ViewNode {
    /// `vstViewGetText` of the label column (the name the command gave the
    /// row, with its sorted/aligned suffix).
    pub name: String,
    /// `Node.Index`: the position below the parent, as the definitions
    /// index it.
    pub index: usize,
    /// The definition member of a row without an element (`InitNodes`),
    /// which `elements.add` names.
    pub member_name: Option<String>,
    /// The parent node, `None` for a top-level row.
    pub parent: Option<usize>,
    /// `NodeDatas[0].ConflictAll`: the row background.
    pub conflict_all: ConflictAll,
    /// One cell per record column.
    pub cells: Vec<ViewCell>,
    /// Children in definition order.
    pub children: Vec<usize>,
}

/// One visible row of the grid, pointing at a node of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewRow {
    pub node: usize,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
}

/// The model of the view tab: the columns, the tree and the visible rows.
#[derive(Clone, Debug)]
pub struct ViewModel {
    pub kind: ViewKind,
    pub columns: Vec<ViewColumn>,
    /// The tree, in the order the command returned the rows (pre-order).
    pub nodes: Vec<ViewNode>,
    /// The top-level rows.
    pub roots: Vec<usize>,
    /// The rows the grid draws: the tree without the collapsed subtrees
    /// (`ExpandView` keeps the containers that are not collapsed expanded).
    pub rows: Vec<ViewRow>,
    /// The nodes whose children are hidden (`Container.Collapsed`).
    pub collapsed: HashSet<usize>,
    /// `ActiveIndex`: the column of the record the user selected; the view
    /// paints it slightly darker (`vstViewBeforeCellPaint`).
    pub active_index: Option<usize>,
    /// The records of the columns, in the order the view shows them.
    pub records: Vec<ViewRecord>,
    /// The FormID all the columns share.
    pub form_id: String,
    /// `NodeDatas[0].ConflictAll` of the whole view.
    pub conflict_all: ConflictAll,
}

impl Default for ViewModel {
    fn default() -> Self {
        Self {
            kind: ViewKind::Empty,
            columns: Vec::new(),
            nodes: Vec::new(),
            roots: Vec::new(),
            rows: Vec::new(),
            collapsed: HashSet::new(),
            active_index: None,
            records: Vec::new(),
            form_id: String::new(),
            conflict_all: ConflictAll::caUnknown,
        }
    }
}

impl ViewModel {
    /// `DoSetActiveRecord(nil)`: one filler column, no rows.
    pub fn empty() -> Self {
        Self {
            kind: ViewKind::Empty,
            ..Self::default()
        }
    }

    /// The view of one record (`DoSetActiveRecord(aMainRecord)`): one column
    /// per version of it, headed by the file name (`:17141-17184`).
    pub fn from_compare(response: &CompareResponse, selected: &ViewRecord) -> Self {
        let columns: Vec<ViewColumn> = response
            .columns
            .iter()
            .map(|column| ViewColumn {
                title: column.file.clone(),
                file: column.file.clone(),
                form_id: response.form_id.clone(),
            })
            .collect();
        let records = columns
            .iter()
            .map(|column| ViewRecord {
                form_id: response.form_id.clone(),
                file: column.file.clone(),
            })
            .collect();
        let mut model = Self {
            kind: ViewKind::Single,
            columns,
            records,
            form_id: response.form_id.clone(),
            conflict_all: response.conflict_all.into(),
            ..Self::default()
        };
        model.active_index = model
            .records
            .iter()
            .position(|record| record.file.eq_ignore_ascii_case(&selected.file));
        model.build(&response.rows);
        model
    }

    /// The view of the selected records (`DoSetActiveRecord(aMainRecords)`):
    /// one column each, headed `EditorID [FileName]` (`:16865-16890`).
    pub fn from_compare_selected(response: &CompareSelectedResponse) -> Self {
        let columns: Vec<ViewColumn> = response
            .records
            .iter()
            .map(|record| ViewColumn {
                title: if record.editor_id.is_empty() {
                    record.file.clone()
                } else {
                    format!("{} [{}]", record.editor_id, record.file)
                },
                file: record.file.clone(),
                form_id: record.form_id.clone(),
            })
            .collect();
        let records = response
            .records
            .iter()
            .map(|record| ViewRecord {
                form_id: record.form_id.clone(),
                file: record.file.clone(),
            })
            .collect();
        let mut model = Self {
            kind: ViewKind::Multiple,
            columns,
            records,
            ..Self::default()
        };
        model.build(&response.rows);
        model
    }

    /// Builds the tree and the visible rows from the command's rows.
    fn build(&mut self, rows: &[CompareRow]) {
        self.nodes.clear();
        self.roots = self.build_level(rows);
        self.prune_collapsed();
        self.flatten();
    }

    fn build_level(&mut self, rows: &[CompareRow]) -> Vec<usize> {
        self.build_level_of(rows, None)
    }

    fn build_level_of(&mut self, rows: &[CompareRow], parent: Option<usize>) -> Vec<usize> {
        let mut indices = Vec::with_capacity(rows.len());
        for row in rows {
            if row.hidden {
                continue;
            }
            let index = self.nodes.len();
            self.nodes.push(ViewNode {
                name: row.name.clone(),
                index: row.index,
                member_name: row.member_name.clone(),
                parent,
                conflict_all: row.conflict_all.into(),
                cells: row
                    .cells
                    .iter()
                    .map(|cell| ViewCell {
                        value: cell.value.clone(),
                        summary: cell.summary.clone(),
                        path: cell.path.clone(),
                        editable: cell.editable,
                        edit_value: cell.edit_value.clone(),
                        display_name: cell.display_name.clone(),
                        name: cell.name.clone(),
                        sort_key: cell.sort_key.clone(),
                        conflict_this: cell.conflict_this.into(),
                        conflict_all: cell.conflict_all.into(),
                    })
                    .collect(),
                children: Vec::new(),
            });
            indices.push(index);
            let children = self.build_level_of(&row.children, Some(index));
            self.nodes[index].children = children;
        }
        indices
    }

    /// Drops collapse entries of nodes a rebuilt tree no longer has.
    fn prune_collapsed(&mut self) {
        let nodes = self.nodes.len();
        self.collapsed.retain(|index| *index < nodes);
    }

    /// Rebuilds the visible rows from the tree and the expansion state.
    pub fn flatten(&mut self) {
        self.rows.clear();
        let roots = self.roots.clone();
        self.flatten_level(&roots, 0);
    }

    fn flatten_level(&mut self, indices: &[usize], depth: usize) {
        for &index in indices {
            let has_children = !self.nodes[index].children.is_empty();
            let expanded = has_children && !self.collapsed.contains(&index);
            self.rows.push(ViewRow {
                node: index,
                depth,
                has_children,
                expanded,
            });
            if expanded {
                let children = self.nodes[index].children.clone();
                self.flatten_level(&children, depth + 1);
            }
        }
    }

    /// Expands or collapses the row; returns whether the visible rows
    /// changed (`vstViewDblClick` on the label column, the tree's buttons).
    pub fn toggle(&mut self, row: usize) -> bool {
        let Some(row) = self.rows.get(row).copied() else {
            return false;
        };
        if !row.has_children {
            return false;
        }
        if !self.collapsed.remove(&row.node) {
            self.collapsed.insert(row.node);
        }
        self.flatten();
        true
    }

    /// Whether the view has the filler column: more than one record
    /// (`:16885-16890`) or no record at all (`DoSetActiveRecord(nil)`).
    pub fn filler(&self) -> bool {
        match self.kind {
            ViewKind::Empty => true,
            ViewKind::Single | ViewKind::Multiple => self.columns.len() > 1,
        }
    }

    /// The number of grid columns: the label column, one per record and the
    /// filler.
    pub fn grid_column_count(&self) -> usize {
        if self.kind == ViewKind::Empty {
            return 1;
        }
        1 + self.columns.len() + usize::from(self.filler())
    }

    /// The columns the keyboard may focus: the filler is outside them
    /// (`coAllowFocus` off).
    pub fn focusable_columns(&self) -> usize {
        self.grid_column_count() - usize::from(self.filler())
    }

    /// The label column of the row (`vstViewGetText` with `Column < 1`): the
    /// display name of the focused column's element while that column has
    /// the focus, else the row's name.
    pub fn label(&self, node: usize, focused_column: usize) -> String {
        let node = &self.nodes[node];
        let focused = if self.columns.len() == 1 { 1 } else { focused_column };
        if focused > 0
            && let Some(cell) = node.cells.get(focused - 1)
            && cell.value.is_some()
            && let Some(name) = &cell.display_name
        {
            return name.clone();
        }
        node.name.clone()
    }

    /// The text of a data column (`vstViewGetText` with `Column > 0`): the
    /// value, or the summary while the row is collapsed.
    pub fn cell_text(&self, node: usize, column: usize, expanded: bool) -> String {
        let Some(cell) = self.nodes[node].cells.get(column) else {
            return String::new();
        };
        cell_text(cell, expanded)
    }

    /// The label column's text colour (`vstViewPaintText` with `Column < 1`):
    /// the highest `ConflictThis` of the row.
    pub fn row_conflict_this(&self, node: usize) -> ConflictThis {
        self.nodes[node]
            .cells
            .iter()
            .map(|cell| cell.conflict_this)
            .max()
            .unwrap_or(ConflictThis::ctUnknown)
    }

    /// The text colour of a data column (`vstViewPaintText`). A sibling
    /// comparison paints the focused column as `ctMaster` and the others by
    /// their sort key against it: equal is `ctOverride`, different is
    /// `ctConflictLoses`.
    pub fn cell_conflict_this(&self, node: usize, column: usize, focused_column: usize) -> ConflictThis {
        let Some(cell) = self.nodes[node].cells.get(column) else {
            return ConflictThis::ctUnknown;
        };
        if self.kind == ViewKind::Multiple && focused_column > 0 {
            let focused = focused_column - 1;
            if column == focused {
                return ConflictThis::ctMaster;
            }
            let focus_key = self.nodes[node]
                .cells
                .get(focused)
                .and_then(|cell| cell.sort_key.as_deref())
                .unwrap_or_default();
            let this_key = cell.sort_key.as_deref().unwrap_or_default();
            return if focus_key == this_key {
                ConflictThis::ctOverride
            } else {
                ConflictThis::ctConflictLoses
            };
        }
        cell.conflict_this
    }

    /// `mniViewSortClick` of a sibling comparison: the records ordered by
    /// the sort keys of the focused row's cells.
    pub fn sorted_records(&self, row: usize) -> Option<Vec<ViewRecord>> {
        if self.kind != ViewKind::Multiple {
            return None;
        }
        let node = &self.nodes[self.rows.get(row)?.node];
        let mut order: Vec<(String, usize)> = self
            .columns
            .iter()
            .enumerate()
            .map(|(index, _)| {
                (
                    node.cells
                        .get(index)
                        .and_then(|cell| cell.sort_key.clone())
                        .unwrap_or_default(),
                    index,
                )
            })
            .collect();
        order.sort_by(|left, right| left.0.cmp(&right.0));
        Some(
            order
                .into_iter()
                .map(|(_, index)| self.records[index].clone())
                .collect(),
        )
    }

    /// The label a dialog uses for a record (`ActiveRecords[i].Element.Name`
    /// upstream; the view knows the FormID and the file).
    pub fn record_label(record: &ViewRecord) -> String {
        format!("[{}] {}", record.form_id, record.file)
    }

    /// The cell of the row and data column, when it has an element.
    pub fn cell(&self, row: usize, column: usize) -> Option<&ViewCell> {
        let row = self.rows.get(row)?;
        self.nodes[row.node].cells.get(column)
    }
}

/// The text of a data column: the value, or the summary while the row is
/// collapsed (`vstViewGetText`).
fn cell_text(cell: &ViewCell, expanded: bool) -> String {
    match &cell.value {
        Some(value) if !value.is_empty() => value.clone(),
        Some(_) if !expanded => cell.summary.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// One edit the grid accepted and the command layer owes: the
/// `elements.set` request (`vstViewNewText` → `Element.EditValue`).
#[derive(Clone, Debug, PartialEq)]
pub struct PendingEdit {
    /// Load order FormID of the record.
    pub form_id: String,
    /// Plugin whose version of the record is edited.
    pub file: String,
    /// Path of the element inside the record.
    pub path: String,
    /// The value typed into the editor, as the edit value.
    pub value: String,
}

/// The grid source over a [`ViewModel`].
pub struct ViewSource<'a> {
    pub model: &'a mut ViewModel,
    /// The focused column of the grid (0 is the label column), which the
    /// label text and the sibling paint read.
    pub focused_column: usize,
    /// `wbEditAllowed`.
    pub edit_allowed: bool,
    /// The edit the last `set_cell` accepted.
    pub pending_edit: Option<PendingEdit>,
    columns: Vec<GridColumn>,
}

impl<'a> ViewSource<'a> {
    pub fn new(
        model: &'a mut ViewModel,
        focused_column: usize,
        edit_allowed: bool,
        mode: ColumnWidthMode,
        available_width: f32,
    ) -> Self {
        let columns = grid_columns(model, mode, available_width);
        Self {
            model,
            focused_column,
            edit_allowed,
            pending_edit: None,
            columns,
        }
    }

    /// The grid columns, for the form's own column measurement.
    pub fn columns(&self) -> &[GridColumn] {
        &self.columns
    }

    /// Replaces the widths after `AutoFitColumns` of the text mode.
    pub fn set_widths(&mut self, widths: Vec<f32>) {
        if widths.len() == self.columns.len() {
            for (column, width) in self.columns.iter_mut().zip(widths) {
                column.width = width.max(column.min_width);
            }
        }
    }
}

/// The grid columns of a view (`vstView.Header.Columns` after
/// `DoSetActiveRecord` and `UpdateColumnWidths`): the fixed unnamed label
/// column, one column per record, and the filler when more than one record
/// (or no record) is shown.
pub fn grid_columns(model: &ViewModel, mode: ColumnWidthMode, available_width: f32) -> Vec<GridColumn> {
    // `DoSetActiveRecord(nil)` leaves one filler column and no label column.
    if model.kind == ViewKind::Empty {
        let mut filler = GridColumn::new("", 0.0);
        filler.min_width = 0.0;
        return vec![filler];
    }
    let mut columns = vec![GridColumn::new("", COLUMN_WIDTH)];
    let filler = model.filler();
    let record_columns = model.columns.len();
    let width = match mode {
        ColumnWidthMode::Standard | ColumnWidthMode::FitText | ColumnWidthMode::FitSmart => COLUMN_WIDTH,
        ColumnWidthMode::FitAll if record_columns > 0 => {
            ((available_width - COLUMN_WIDTH) / record_columns as f32).max(60.0)
        }
        ColumnWidthMode::FitAll => COLUMN_WIDTH,
    };
    for column in &model.columns {
        let mut grid_column = GridColumn::new(column.title.clone(), width);
        grid_column.min_width = (width / 2.0).max(24.0);
        columns.push(grid_column);
    }
    if filler {
        let mut grid_column = GridColumn::new("", 0.0);
        grid_column.min_width = 0.0;
        columns.push(grid_column);
    }
    columns
}

impl GridSource for ViewSource<'_> {
    fn row_count(&self) -> usize {
        self.model.rows.len()
    }

    fn columns(&self) -> &[GridColumn] {
        &self.columns
    }

    fn focusable_columns(&self) -> usize {
        self.columns.len() - usize::from(self.model.filler())
    }

    fn cell(&self, row: usize, column: usize, out: &mut GridCell) {
        let Some(view_row) = self.model.rows.get(row).copied() else {
            return;
        };
        let node = &self.model.nodes[view_row.node];
        // `vstViewBeforeItemErase`: the whole row takes column 0's conflict.
        out.conflict_all = node.conflict_all;
        if column == 0 {
            out.text = self.model.label(view_row.node, self.focused_column);
            out.conflict_this = self.model.row_conflict_this(view_row.node);
            return;
        }
        let Some(cell) = node.cells.get(column - 1) else {
            // The filler column.
            return;
        };
        out.text = cell_text(cell, view_row.expanded);
        out.edit_text = cell.edit_value.clone();
        out.editable = self.edit_allowed && cell.editable && cell.path.is_some();
        out.conflict_this = self
            .model
            .cell_conflict_this(view_row.node, column - 1, self.focused_column);
        // `vstViewBeforeCellPaint`: a cell without an element, or one of the
        // active record, is filled with its own conflict colour.
        if cell.value.is_none() || self.model.active_index == Some(column - 1) {
            let mut factor = 0.85;
            if cell.value.is_none() {
                factor += 0.08;
            }
            if self.model.active_index == Some(column - 1) {
                factor -= 0.04;
                if cell.value.is_none() {
                    factor += 0.015;
                }
            }
            out.cell_fill = Some((cell.conflict_all, factor));
        }
    }

    fn set_cell(&mut self, row: usize, column: usize, text: &str) -> bool {
        if !self.edit_allowed || column == 0 {
            return false;
        }
        let Some(view_row) = self.model.rows.get(row).copied() else {
            return false;
        };
        let Some(cell) = self.model.nodes[view_row.node].cells.get(column - 1) else {
            return false;
        };
        let (true, Some(path)) = (cell.editable, cell.path.clone()) else {
            return false;
        };
        let Some(record) = self.model.records.get(column - 1) else {
            return false;
        };
        self.pending_edit = Some(PendingEdit {
            form_id: record.form_id.clone(),
            file: record.file.clone(),
            path,
            value: text.to_owned(),
        });
        true
    }

    fn row_label(&self, row: usize) -> Option<String> {
        self.model
            .rows
            .get(row)
            .map(|row| self.model.label(row.node, self.focused_column))
    }

    fn row_depth(&self, row: usize) -> usize {
        self.model.rows.get(row).map_or(0, |row| row.depth)
    }

    fn row_has_children(&self, row: usize) -> bool {
        self.model.rows.get(row).is_some_and(|row| row.has_children)
    }

    fn row_expanded(&self, row: usize) -> bool {
        self.model.rows.get(row).is_some_and(|row| row.expanded)
    }

    fn toggle_row(&mut self, row: usize) -> bool {
        self.model.toggle(row)
    }
}

/// The state of the view tab of the main form.
pub struct ViewGrid {
    pub model: ViewModel,
    pub state: GridState,
    /// The edit the grid accepted and the command layer has yet to run.
    pub pending_edit: Option<PendingEdit>,
    /// The element path of the focused cell (and its data column), kept
    /// across a rebuild (`ViewFocusedElement`/`ColumnForViewFocusedElement`).
    pub focused_element: Option<(String, usize)>,
    /// The row index to keep when the focused cell has no path.
    pub focused_row: Option<usize>,
    /// The last text of a rejected edit, for the message log.
    pub error: Option<String>,
}

impl Default for ViewGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewGrid {
    pub fn new() -> Self {
        Self {
            model: ViewModel::empty(),
            state: GridState::new(vec![LABEL_COLUMN_WIDTH, VALUES_COLUMN_WIDTH]),
            pending_edit: None,
            focused_element: None,
            focused_row: None,
            error: None,
        }
    }

    /// `DoSetActiveRecord`: replaces the model and keeps the focus on the
    /// element that had it, as `PostResetActiveTree` does with
    /// `ViewFocusedElement`.
    pub fn rebuild(&mut self, model: ViewModel) {
        self.remember_focus();
        self.model = model;
        let row = self.focused_element.as_ref().and_then(|(path, column)| {
            self.model.rows.iter().position(|row| {
                self.model.nodes[row.node]
                    .cells
                    .get(column.saturating_sub(1))
                    .and_then(|cell| cell.path.as_deref())
                    == Some(path.as_str())
            })
        });
        let row = row.or_else(|| {
            self.focused_row
                .filter(|row| *row < self.model.rows.len())
                .or_else(|| (!self.model.rows.is_empty()).then_some(0))
        });
        self.state.focused_row = row;
        self.state.selected_row = row;
        self.state.clamp(self.model.rows.len(), self.model.focusable_columns());
        if let Some(row) = row {
            self.state.scroll_to = Some(row);
        }
    }

    /// Records the focused element before a rebuild.
    pub fn remember_focus(&mut self) {
        let Some(row) = self.state.focused_row else {
            self.focused_element = None;
            self.focused_row = None;
            return;
        };
        self.focused_row = Some(row);
        let column = self.state.focused_column;
        let path = self
            .model
            .rows
            .get(row)
            .and_then(|row| self.model.nodes[row.node].cells.get(column.checked_sub(1)?))
            .and_then(|cell| cell.path.clone());
        self.focused_element = path.map(|path| (path, column));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xedit_session::conflicts::{
        CompareCell, CompareColumn, CompareRow, CompareSelectedColumn, CompareSelectedResponse,
    };

    fn cell(value: Option<&str>, path: Option<&str>, conflict_this: &str, conflict_all: &str) -> CompareCell {
        CompareCell {
            value: value.map(str::to_owned),
            conflict_this: conflict_this_name(conflict_this),
            path: path.map(str::to_owned),
            editable: path.is_some(),
            edit_value: value.map(str::to_owned),
            display_name: value.map(|value| format!("{value}_name")),
            name: value.map(|value| format!("{value}_name")),
            summary: None,
            sort_key: value.map(str::to_owned),
            modified: false,
            conflict_all: conflict_all_name(conflict_all),
        }
    }

    fn conflict_this_name(name: &str) -> xedit_session::conflicts::ConflictThisName {
        serde_json::from_value(serde_json::Value::String(name.to_owned())).unwrap()
    }

    fn conflict_all_name(name: &str) -> xedit_session::conflicts::ConflictAllName {
        serde_json::from_value(serde_json::Value::String(name.to_owned())).unwrap()
    }

    fn response() -> CompareResponse {
        CompareResponse {
            form_id: "00000801".to_owned(),
            signature: "GLOB".to_owned(),
            conflict_all: conflict_all_name("caConflict"),
            columns: vec![
                CompareColumn {
                    file: "Master.esm".to_owned(),
                    conflict_this: conflict_this_name("ctMaster"),
                },
                CompareColumn {
                    file: "Patch.esp".to_owned(),
                    conflict_this: conflict_this_name("ctConflictWins"),
                },
            ],
            rows: vec![
                CompareRow {
                    name: "EDID - Editor ID".to_owned(),
                    index: 1,
                    member_name: Some("EDID".to_owned()),
                    conflict_all: conflict_all_name("caNoConflict"),
                    hidden: false,
                    filtered: false,
                    cells: vec![
                        cell(Some("Test"), Some("EDID"), "ctMaster", "caNoConflict"),
                        cell(Some("Test"), Some("EDID"), "ctConflictBenign", "caNoConflict"),
                    ],
                    children: Vec::new(),
                },
                CompareRow {
                    name: "DATA - Data".to_owned(),
                    index: 2,
                    member_name: Some("Data".to_owned()),
                    conflict_all: conflict_all_name("caConflict"),
                    hidden: false,
                    filtered: false,
                    cells: vec![
                        cell(None, None, "ctMaster", "caConflict"),
                        cell(None, None, "ctConflictWins", "caConflict"),
                    ],
                    children: vec![CompareRow {
                        name: "FLTV - Value".to_owned(),
                        index: 0,
                        member_name: Some("Value".to_owned()),
                        conflict_all: conflict_all_name("caConflict"),
                        hidden: false,
                        filtered: false,
                        cells: vec![
                            cell(Some("1.0"), Some("DATA\\Value"), "ctMaster", "caConflict"),
                            cell(Some("2.0"), Some("DATA\\Value"), "ctConflictWins", "caConflict"),
                        ],
                        children: Vec::new(),
                    }],
                },
            ],
            view_filter: None,
            truncated: None,
            messages: Vec::new(),
        }
    }

    fn selected() -> ViewRecord {
        ViewRecord {
            form_id: "00000801".to_owned(),
            file: "Patch.esp".to_owned(),
        }
    }

    #[test]
    fn the_single_view_has_one_column_per_version_headed_by_file_name() {
        let model = ViewModel::from_compare(&response(), &selected());
        assert_eq!(model.kind, ViewKind::Single);
        assert_eq!(model.columns.len(), 2);
        assert_eq!(model.columns[0].title, "Master.esm");
        assert_eq!(model.columns[1].title, "Patch.esp");
        assert_eq!(model.active_index, Some(1));
        // Two records: the filler column follows them.
        assert!(model.filler());
        assert_eq!(model.grid_column_count(), 4);
    }

    #[test]
    fn the_rows_are_the_definition_and_its_children_expanded() {
        let model = ViewModel::from_compare(&response(), &selected());
        assert_eq!(model.rows.len(), 3);
        assert_eq!(model.rows[0].depth, 0);
        assert_eq!(model.label(model.rows[0].node, 0), "EDID - Editor ID");
        assert_eq!(model.rows[2].depth, 1);
        assert_eq!(model.label(model.rows[2].node, 0), "FLTV - Value");
    }

    #[test]
    fn a_collapse_hides_the_children_and_toggles_back() {
        let mut model = ViewModel::from_compare(&response(), &selected());
        assert!(model.toggle(1));
        assert_eq!(model.rows.len(), 2);
        assert!(!model.rows[1].expanded);
        // A collapsed row with an empty value shows its summary.
        assert!(model.toggle(1));
        assert_eq!(model.rows.len(), 3);
        assert!(!model.toggle(0));
    }

    #[test]
    fn the_focused_column_names_the_label_column() {
        let model = ViewModel::from_compare(&response(), &selected());
        let row = model.rows[0].node;
        // Without a focused data column the label is the row's own name.
        assert_eq!(model.label(row, 0), "EDID - Editor ID");
        // With the second column focused it is that element's display name.
        assert_eq!(model.label(row, 2), "Test_name");
    }

    #[test]
    fn the_label_column_takes_the_highest_conflict_this() {
        let model = ViewModel::from_compare(&response(), &selected());
        assert_eq!(
            model.row_conflict_this(model.rows[0].node),
            ConflictThis::ctConflictBenign
        );
        assert_eq!(
            model.row_conflict_this(model.rows[1].node),
            ConflictThis::ctConflictWins
        );
    }

    #[test]
    fn a_cell_edit_is_a_pending_elements_set_request() {
        let mut model = ViewModel::from_compare(&response(), &selected());
        let mut source = ViewSource::new(&mut model, 0, true, ColumnWidthMode::Standard, 900.0);
        // Row 2 is FLTV, column 2 is Patch.esp.
        assert!(source.set_cell(2, 2, "3.5"));
        assert_eq!(
            source.pending_edit,
            Some(PendingEdit {
                form_id: "00000801".to_owned(),
                file: "Patch.esp".to_owned(),
                path: "DATA\\Value".to_owned(),
                value: "3.5".to_owned(),
            })
        );
        // The label column and the filler are not editable.
        assert!(!source.set_cell(2, 0, "x"));
        assert!(!source.set_cell(2, 3, "x"));
    }

    #[test]
    fn a_read_only_view_accepts_no_edit() {
        let mut model = ViewModel::from_compare(&response(), &selected());
        let mut source = ViewSource::new(&mut model, 0, false, ColumnWidthMode::Standard, 900.0);
        assert!(!source.set_cell(2, 1, "3.5"));
    }

    #[test]
    fn the_grid_cells_carry_the_row_and_cell_conflicts() {
        let mut model = ViewModel::from_compare(&response(), &selected());
        let source = ViewSource::new(&mut model, 0, true, ColumnWidthMode::Standard, 900.0);
        let mut out = GridCell::default();
        source.cell(2, 0, &mut out);
        assert_eq!(out.text, "FLTV - Value");
        assert_eq!(out.conflict_all, ConflictAll::caConflict);
        source.cell(2, 2, &mut out);
        assert_eq!(out.text, "2.0");
        assert_eq!(out.edit_text.as_deref(), Some("2.0"));
        assert!(out.editable);
        assert_eq!(out.conflict_this, ConflictThis::ctConflictWins);
        // The second column is the active record: its cell takes the darker
        // fill of `vstViewBeforeCellPaint`.
        let (fill_conflict, factor) = out.cell_fill.expect("the active column takes a fill");
        assert_eq!(fill_conflict, ConflictAll::caConflict);
        assert!((factor - 0.81).abs() < 1e-9, "{factor}");
    }

    #[test]
    fn a_sibling_view_paints_the_focused_column_as_master() {
        let model = ViewModel::from_compare_selected(&CompareSelectedResponse {
            records: vec![
                CompareSelectedColumn {
                    file: "A.esp".to_owned(),
                    form_id: "00000801".to_owned(),
                    signature: "GLOB".to_owned(),
                    editor_id: "One".to_owned(),
                    name: "One".to_owned(),
                },
                CompareSelectedColumn {
                    file: "B.esp".to_owned(),
                    form_id: "00000802".to_owned(),
                    signature: "GLOB".to_owned(),
                    editor_id: "Two".to_owned(),
                    name: "Two".to_owned(),
                },
            ],
            rows: vec![CompareRow {
                name: "FLTV - Value".to_owned(),
                index: 1,
                member_name: Some("Value".to_owned()),
                conflict_all: conflict_all_name("caConflictBenign"),
                hidden: false,
                filtered: false,
                cells: vec![
                    cell(Some("1.0"), Some("DATA\\Value"), "ctOnlyOne", "caConflictBenign"),
                    cell(Some("1.0"), Some("DATA\\Value"), "ctOnlyOne", "caConflictBenign"),
                ],
                children: Vec::new(),
            }],
            view_filter: None,
            messages: Vec::new(),
        });
        assert_eq!(model.kind, ViewKind::Multiple);
        assert_eq!(model.columns[0].title, "One [A.esp]");
        assert_eq!(model.columns[1].title, "Two [B.esp]");
        let node = model.rows[0].node;
        assert_eq!(model.cell_conflict_this(node, 0, 1), ConflictThis::ctMaster);
        // The two sort keys are equal: the other column is an override.
        assert_eq!(model.cell_conflict_this(node, 1, 1), ConflictThis::ctOverride);
        assert_eq!(model.cell_conflict_this(node, 0, 0), ConflictThis::ctOnlyOne);
    }

    #[test]
    fn a_collapsed_row_shows_the_summary_of_its_empty_cells() {
        let mut response = response();
        response.rows[1].cells[0].value = Some(String::new());
        response.rows[1].cells[0].summary = Some("2 entries".to_owned());
        let model = ViewModel::from_compare(&response, &selected());
        assert_eq!(model.cell_text(model.rows[1].node, 0, false), "2 entries");
        assert_eq!(model.cell_text(model.rows[1].node, 0, true), "");
    }
}
