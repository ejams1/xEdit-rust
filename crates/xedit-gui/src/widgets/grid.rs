// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.dfm (vstView: TVirtualEditTree with
// the Labels/Values columns, toEditable and toEditOnClick), xeMainForm.pas
// (vstViewBeforeCellPaint, vstViewPaintText, vstViewGetEditText,
// vstViewNewText, vstViewEditing, vstViewKeyDown, vstViewClick,
// vstViewDblClick, vstViewHeaderClick)

//! The virtualised grid primitive: rows × columns with per-cell conflict
//! colours, tree-shaped rows and in-place editing.
//!
//! The view tab of step 3 implements [`GridSource`] over a record's
//! elements (one row per element, one column per record plus the unnamed
//! label column and the filler) and routes [`GridSource::set_cell`] to
//! `elements.set`. Rows can carry the depth, expander and fold state of
//! `vstView`'s tree, and a cell or the header can open its context menu
//! (`pmuView`, `pmuViewHeader`). Only the visible rows are asked for.
//!
//! As with the tree, the state machine ([`GridState`]) is independent of the
//! egui context and is tested headlessly.

use eframe::egui;
use xedit_core::interface::types::{ConflictAll, ConflictThis, PascalEnum};

use super::conflict::{ConflictColors, RowColors};

/// One column of the grid: `vstView.Header.Columns`.
#[derive(Clone, Debug, PartialEq)]
pub struct GridColumn {
    pub title: String,
    pub width: f32,
    pub min_width: f32,
    /// Right-align numeric columns (`Alignment = taRightJustify`).
    pub right_aligned: bool,
}

impl GridColumn {
    pub fn new(title: impl Into<String>, width: f32) -> Self {
        Self {
            title: title.into(),
            width,
            min_width: 24.0,
            right_aligned: false,
        }
    }
}

/// One cell of the grid, built when it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct GridCell {
    pub text: String,
    pub tooltip: Option<String>,
    /// Whether a double click edits the cell (`IsEditable` of the element).
    pub editable: bool,
    /// The text the editor starts with (`Element.EditValue`), when it differs
    /// from `text`; `text` when absent.
    pub edit_text: Option<String>,
    pub conflict_all: ConflictAll,
    pub conflict_this: ConflictThis,
    /// A fill over the row background for this cell alone
    /// (`vstViewBeforeCellPaint`): the cell's own conflict and the factor of
    /// its colour. The view paints a cell that has no element or belongs to
    /// the active record with its own conflict colour.
    pub cell_fill: Option<(ConflictAll, f64)>,
}

impl Default for GridCell {
    fn default() -> Self {
        Self {
            text: String::new(),
            tooltip: None,
            editable: false,
            edit_text: None,
            conflict_all: ConflictAll::caUnknown,
            conflict_this: ConflictThis::ctUnknown,
            cell_fill: None,
        }
    }
}

/// The data behind a [`Grid`].
pub trait GridSource {
    fn row_count(&self) -> usize;
    fn columns(&self) -> &[GridColumn];
    /// Fills `out` with the cell at `row`, `column`. Called for the visible
    /// range only.
    fn cell(&self, row: usize, column: usize, out: &mut GridCell);
    /// Commits an edit (`vstViewNewText` → `Element.EditValue`); returns
    /// whether the source accepted it.
    fn set_cell(&mut self, row: usize, column: usize, text: &str) -> bool {
        let _ = (row, column, text);
        false
    }
    /// The row label used by the view filter and the stick row (the first
    /// column's text), for the form's own presentation.
    fn row_label(&self, row: usize) -> Option<String> {
        let _ = row;
        None
    }
    /// The indentation of the row in the first column (`vstView`'s tree
    /// rows); 0 for a flat grid.
    fn row_depth(&self, row: usize) -> usize {
        let _ = row;
        0
    }
    /// Whether the row has rows below it (`vstViewInitNode`'s
    /// `ivsHasChildren`).
    fn row_has_children(&self, row: usize) -> bool {
        let _ = row;
        false
    }
    /// Whether the row shows its children.
    fn row_expanded(&self, row: usize) -> bool {
        let _ = row;
        false
    }
    /// Expands or collapses a row; returns whether the visible rows changed.
    fn toggle_row(&mut self, row: usize) -> bool {
        let _ = row;
        false
    }
    /// The columns the keyboard may focus: the filler column of the view is
    /// outside them (`coAllowFocus` off).
    fn focusable_columns(&self) -> usize {
        self.columns().len()
    }
}

/// An edit in progress: the cell and the text typed into it.
#[derive(Clone, Debug, PartialEq)]
pub struct GridEdit {
    pub row: usize,
    pub column: usize,
    pub original: String,
    pub text: String,
}

/// The focus, selection and edit state of a grid.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridState {
    pub focused_row: Option<usize>,
    pub focused_column: usize,
    /// The last row clicked with the left button.
    pub selected_row: Option<usize>,
    pub editing: Option<GridEdit>,
    pub column_widths: Vec<f32>,
    pub row_height: f32,
    /// The indentation of one tree level in the first column.
    pub indent: f32,
    /// A row to bring into view on the next repaint.
    pub scroll_to: Option<usize>,
}

impl GridState {
    pub fn new(column_widths: Vec<f32>) -> Self {
        Self {
            focused_row: None,
            focused_column: 0,
            selected_row: None,
            editing: None,
            column_widths,
            row_height: 18.0,
            indent: 16.0,
            scroll_to: None,
        }
    }

    pub fn set_column_widths(&mut self, widths: &[f32]) {
        if self.column_widths.len() != widths.len() {
            self.column_widths = widths.to_vec();
        }
    }

    /// A left click on a cell: moves the focus and the selection, and ends
    /// an edit by committing it if the click landed elsewhere.
    pub fn on_click(&mut self, row: usize, column: usize, source: &mut dyn GridSource) -> Option<GridEditResult> {
        let result = self.finish_edit(source);
        self.focused_row = Some(row);
        self.focused_column = column;
        self.selected_row = Some(row);
        result
    }

    /// A double click (`toEditOnClick`): starts an edit when the cell is
    /// editable.
    pub fn on_double_click(&mut self, row: usize, column: usize, cell: &GridCell) {
        if !cell.editable {
            return;
        }
        if let Some(edit) = &self.editing
            && edit.row == row
            && edit.column == column
        {
            return;
        }
        let text = cell.edit_text.clone().unwrap_or_else(|| cell.text.clone());
        self.editing = Some(GridEdit {
            row,
            column,
            original: text.clone(),
            text,
        });
    }

    /// Enter starts an edit on the focused cell; Escape cancels one.
    pub fn on_edit_key(&mut self, key: egui::Key, source: &mut dyn GridSource) -> Option<GridEditResult> {
        match key {
            egui::Key::Enter => {
                if self.editing.is_some() {
                    self.finish_edit(source)
                } else {
                    None
                }
            }
            egui::Key::Escape => {
                if self.editing.is_some() {
                    self.editing = None;
                    Some(GridEditResult::Cancelled)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Commits the edit in progress through the source.
    pub fn finish_edit(&mut self, source: &mut dyn GridSource) -> Option<GridEditResult> {
        let edit = self.editing.take()?;
        if edit.text == edit.original {
            return Some(GridEditResult::Unchanged);
        }
        let accepted = source.set_cell(edit.row, edit.column, &edit.text);
        Some(if accepted {
            GridEditResult::Committed {
                row: edit.row,
                column: edit.column,
                text: edit.text,
            }
        } else {
            GridEditResult::Rejected
        })
    }

    /// Arrow keys move the focus (`vstViewKeyDown`).
    pub fn on_key(&mut self, key: egui::Key, rows: usize, columns: usize) -> bool {
        if rows == 0 || columns == 0 || self.editing.is_some() {
            return false;
        }
        let Some(row) = self.focused_row else {
            // The first arrow focuses the first row (`vstViewKeyDown`).
            self.focused_row = Some(0);
            self.selected_row = Some(0);
            self.scroll_to = Some(0);
            return true;
        };
        let (row, column) = match key {
            egui::Key::ArrowDown => ((row + 1).min(rows - 1), self.focused_column),
            egui::Key::ArrowUp => (row.saturating_sub(1), self.focused_column),
            egui::Key::ArrowLeft => (row, self.focused_column.saturating_sub(1)),
            egui::Key::ArrowRight => (row, (self.focused_column + 1).min(columns - 1)),
            egui::Key::Home => (row, 0),
            egui::Key::End => (row, columns - 1),
            _ => return false,
        };
        self.focused_row = Some(row);
        self.focused_column = column;
        self.selected_row = Some(row);
        self.scroll_to = Some(row);
        true
    }

    /// Keeps the focus inside a grid whose row count changed.
    pub fn clamp(&mut self, rows: usize, columns: usize) {
        if self.focused_row.is_some_and(|row| row >= rows) {
            self.focused_row = rows.checked_sub(1);
        }
        if self.selected_row.is_some_and(|row| row >= rows) {
            self.selected_row = rows.checked_sub(1);
        }
        if self.focused_column >= columns {
            self.focused_column = columns.saturating_sub(1);
        }
        if self
            .editing
            .as_ref()
            .is_some_and(|edit| edit.row >= rows || edit.column >= columns)
        {
            self.editing = None;
        }
    }
}

/// What happened to an edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridEditResult {
    Committed { row: usize, column: usize, text: String },
    Rejected,
    Cancelled,
    Unchanged,
}

/// What the user did to the grid this repaint.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridOutput {
    pub clicked: Option<(usize, usize)>,
    pub secondary_clicked: Option<(usize, usize)>,
    pub double_clicked: Option<(usize, usize)>,
    /// The header column clicked (left) and whether Shift was held
    /// (`vstViewHeaderClick`).
    pub header_clicked: Option<(usize, bool)>,
    /// The header column right-clicked: the `PopupIndex` of `pmuViewHeader`.
    pub header_secondary_clicked: Option<usize>,
    /// The row whose expander was clicked or double-clicked.
    pub toggled: Option<usize>,
    /// The edit that ended this repaint.
    pub edit: Option<GridEditResult>,
}

/// The column a context-menu callback passes for the header row
/// (`pmuViewHeader`'s `PopupIndex`).
pub const HEADER_MENU: usize = usize::MAX;

/// The context menu of a grid: called with `(is_header, column)`.
pub type GridContextMenu<'a> = dyn FnMut(&mut egui::Ui, bool, usize) + 'a;

/// The `vstView` grid drawn over a [`GridSource`].
pub struct Grid<'a> {
    pub id: egui::Id,
    pub source: &'a mut dyn GridSource,
    pub state: &'a mut GridState,
    pub colors: &'a ConflictColors,
    pub header: bool,
    /// The context menu of a cell or of the header: called with
    /// `(is_header, column)`, where a cell passes its own column and the
    /// header passes [`HEADER_MENU`].
    pub context_menu: Option<&'a mut GridContextMenu<'a>>,
    /// Whether the grid answers a double click itself: the label column
    /// toggles the row and a data cell starts the in-place editor. The view
    /// turns it off, because `vstViewDblClick` chooses between the editor
    /// and the View Elements dialog.
    pub own_double_click: bool,
}

impl<'a> Grid<'a> {
    pub fn show(self, ui: &mut egui::Ui) -> GridOutput {
        let Grid {
            id,
            source,
            state,
            colors,
            header,
            mut context_menu,
            own_double_click,
        } = self;
        let mut output = GridOutput::default();
        if std::env::var_os("XEDIT_GUI_DEBUG_GRID").is_some() {
            crate::log_screen(
                "grid-show",
                &[format!(
                    "id={id:?} rows={} columns={}",
                    source.row_count(),
                    source.columns().len()
                )],
            );
        }
        let visual = ui.visuals().clone();
        let dark = visual.dark_mode;
        let font = ui.style().text_styles[&egui::TextStyle::Body].clone();
        state.set_column_widths(&source.columns().iter().map(|column| column.width).collect::<Vec<_>>());
        let rows = source.row_count();
        let column_count = source.columns().len();
        state.clamp(rows, source.focusable_columns());
        if header {
            let header_state = show_header(id, ui, source, state, &font, &visual, context_menu.as_deref_mut());
            output.header_clicked = header_state.clicked;
            output.header_secondary_clicked = header_state.secondary;
        }
        let row_height = state.row_height;
        let columns = state.column_widths.clone();
        let total_width = columns.iter().sum::<f32>();
        let scroll_to = state.scroll_to.take();
        let indent = state.indent;

        egui::ScrollArea::vertical()
            .id_salt(id.with("rows"))
            .auto_shrink([false, false])
            .show_rows(ui, row_height, rows, |ui, range| {
                ui.set_width(total_width.max(ui.available_width()));
                for row in range {
                    let depth = source.row_depth(row);
                    let has_children = source.row_has_children(row);
                    let expanded = source.row_expanded(row);
                    let mut x = ui.max_rect().left();
                    let row_top = ui.cursor().top();
                    for column in 0..column_count {
                        let width = columns.get(column).copied().unwrap_or(80.0);
                        let rect = egui::Rect::from_min_size(egui::pos2(x, row_top), egui::vec2(width, row_height));
                        let mut cell = GridCell::default();
                        source.cell(row, column, &mut cell);
                        let row_colors =
                            colors.row_colors(cell.conflict_all, cell.conflict_this, visual.text_color(), dark);
                        let focused = state.focused_row == Some(row) && state.focused_column == column;
                        let editing = state
                            .editing
                            .as_ref()
                            .is_some_and(|edit| edit.row == row && edit.column == column);
                        if editing {
                            let mut text = state.editing.as_ref().map(|edit| edit.text.clone()).unwrap_or_default();
                            let response = ui.put(
                                rect,
                                egui::TextEdit::singleline(&mut text)
                                    .id(id.with(("editor", row, column)))
                                    .desired_width(width),
                            );
                            if !response.has_focus() {
                                response.request_focus();
                            }
                            if let Some(edit) = &mut state.editing {
                                edit.text = text;
                            }
                            let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                            let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                            if escape {
                                state.editing = None;
                                output.edit = Some(GridEditResult::Cancelled);
                            } else if enter || response.lost_focus() {
                                output.edit = state.finish_edit(source);
                            }
                        } else {
                            let response = ui.interact(rect, id.with((row, column)), egui::Sense::click());
                            let tree_cell =
                                (column == 0 && (depth > 0 || has_children)).then_some((depth, has_children, expanded));
                            paint_cell(
                                ui,
                                rect,
                                &cell,
                                &row_colors,
                                colors,
                                &visual,
                                &font,
                                focused,
                                tree_cell,
                                indent,
                            );
                            // The expander and the label column toggle the row
                            // (`vstViewDblClick` with column 0, and the tree's
                            // button click).
                            let on_expander = column == 0
                                && has_children
                                && response
                                    .interact_pointer_pos()
                                    .is_some_and(|pos| pos.x <= rect.left() + (depth as f32 + 1.0) * indent);
                            if response.clicked() {
                                output.clicked = Some((row, column));
                                output.edit = state.on_click(row, column, source);
                                if on_expander {
                                    toggled_row(source, state, row, &mut output);
                                }
                            }
                            if response.secondary_clicked() {
                                state.selected_row = Some(row);
                                state.focused_row = Some(row);
                                state.focused_column = column;
                                output.secondary_clicked = Some((row, column));
                            }
                            if response.double_clicked() {
                                output.double_clicked = Some((row, column));
                                if own_double_click {
                                    if column == 0 && has_children {
                                        toggled_row(source, state, row, &mut output);
                                    } else {
                                        state.on_double_click(row, column, &cell);
                                    }
                                }
                            }
                            if let Some(menu) = context_menu.as_deref_mut() {
                                crate::menu_model::context_menu(&response, |ui| menu(ui, false, column));
                            }
                            if std::env::var_os("XEDIT_GUI_DEBUG_GRID").is_some()
                                && (response.hovered() || response.secondary_clicked() || response.clicked())
                            {
                                crate::log_screen(
                                    "grid-cell",
                                    &[format!(
                                        "row={row} column={column} rect={rect:?} secondary={} clicked={} hovered={} interact_pos={:?}",
                                        response.secondary_clicked(),
                                        response.clicked(),
                                        response.hovered(),
                                        response.interact_pointer_pos()
                                    )],
                                );
                            }
                            if let Some(tooltip) = &cell.tooltip {
                                let _ = response.on_hover_text(tooltip.clone());
                            }
                        }
                        x += width;
                    }
                    if scroll_to == Some(row) {
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(ui.max_rect().left(), row_top),
                            egui::vec2(total_width, row_height),
                        );
                        ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    }
                }
            });

        if ui.memory(|memory| memory.has_focus(id)) {
            for key in [
                egui::Key::ArrowDown,
                egui::Key::ArrowUp,
                egui::Key::ArrowLeft,
                egui::Key::ArrowRight,
                egui::Key::Home,
                egui::Key::End,
            ] {
                if ui.input(|input| input.key_pressed(key)) {
                    state.on_key(key, rows, source.focusable_columns());
                    ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key));
                }
            }
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) && state.editing.is_some() {
                state.editing = None;
                output.edit = Some(GridEditResult::Cancelled);
            }
        }
        output
    }
}

/// Expands or collapses a row and keeps the state inside the new row count.
fn toggled_row(source: &mut dyn GridSource, state: &mut GridState, row: usize, output: &mut GridOutput) {
    if source.toggle_row(row) {
        output.toggled = Some(row);
        state.clamp(source.row_count(), source.focusable_columns());
    }
}

/// What the header row reported this repaint.
struct HeaderOutput {
    /// `vstViewHeaderClick`: the column clicked and whether Shift was held.
    clicked: Option<(usize, bool)>,
    /// `pmuViewHeader`'s `PopupIndex`: the column right-clicked.
    secondary: Option<usize>,
}

/// The header row of the grid.
fn show_header(
    id: egui::Id,
    ui: &mut egui::Ui,
    source: &mut dyn GridSource,
    state: &mut GridState,
    font: &egui::FontId,
    visual: &egui::Visuals,
    context_menu: Option<&mut GridContextMenu<'_>>,
) -> HeaderOutput {
    let height = state.row_height + 3.0;
    let widths: Vec<f32> = state.column_widths.clone();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(widths.iter().sum()), height),
        egui::Sense::click(),
    );
    let mut x = rect.left();
    for (index, width) in widths.iter().enumerate() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, visual.widgets.inactive.bg_fill);
        if index < source.columns().len() {
            ui.painter().text(
                egui::pos2(x + 4.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                &source.columns()[index].title,
                font.clone(),
                visual.text_color(),
            );
        }
        let boundary = egui::Rect::from_min_max(
            egui::pos2(x + width - 3.0, rect.top()),
            egui::pos2(x + width + 1.0, rect.bottom()),
        );
        let response = ui.interact(boundary, id.with(("column", index)), egui::Sense::click_and_drag());
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if response.dragged() {
            let min = source.columns().get(index).map_or(24.0, |column| column.min_width);
            let delta = response.drag_delta().x;
            state.column_widths[index] = (state.column_widths[index] + delta).max(min);
        }
        ui.painter().line_segment(
            [egui::pos2(x + width, rect.top()), egui::pos2(x + width, rect.bottom())],
            egui::Stroke::new(1.0, visual.widgets.noninteractive.bg_stroke.color),
        );
        x += width;
    }
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0, visual.widgets.noninteractive.bg_stroke.color),
    );
    // `vstViewHeaderClick` and `pmuViewHeader` take the column under the
    // pointer.
    let column_at = |x: f32| {
        let mut left = rect.left();
        for (index, width) in widths.iter().enumerate() {
            if x < left + width {
                return Some(index);
            }
            left += width;
        }
        (!widths.is_empty()).then(|| widths.len() - 1)
    };
    let shift = ui.input(|input| input.modifiers.shift);
    let clicked = response
        .interact_pointer_pos()
        .filter(|_| response.clicked())
        .and_then(|pos| column_at(pos.x))
        .map(|column| (column, shift));
    let secondary = response
        .interact_pointer_pos()
        .filter(|_| response.secondary_clicked())
        .and_then(|pos| column_at(pos.x));
    if let (Some(column), Some(menu)) = (secondary, context_menu) {
        crate::menu_model::context_menu(&response, |ui| menu(ui, true, column));
    }
    HeaderOutput { clicked, secondary }
}

/// Paints one cell: the conflict background, the focus ring and the text.
#[allow(clippy::too_many_arguments)]
fn paint_cell(
    ui: &egui::Ui,
    rect: egui::Rect,
    cell: &GridCell,
    colors: &RowColors,
    palette: &ConflictColors,
    visual: &egui::Visuals,
    font: &egui::FontId,
    focused: bool,
    tree_cell: Option<(usize, bool, bool)>,
    indent: f32,
) {
    if let Some(background) = colors.background {
        ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, background);
    }
    // `vstViewBeforeCellPaint`: a cell without an element, or one of the
    // active record, is filled with its own conflict colour.
    if let Some((conflict_all, factor)) = cell.cell_fill
        && conflict_all >= ConflictAll::caNoConflict
        && let Some(color) = palette.all[conflict_all.ord()]
    {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::ZERO,
            crate::widgets::conflict::wb_lighter(color, factor, visual.dark_mode),
        );
    }
    if focused {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            egui::CornerRadius::ZERO,
            egui::Stroke::new(1.0, visual.selection.stroke.color),
            egui::StrokeKind::Inside,
        );
    }
    let mut text_x = rect.left() + 4.0;
    if let Some((depth, has_children, expanded)) = tree_cell {
        text_x += depth as f32 * indent;
        if has_children {
            let glyph = if expanded { "\u{25BE}" } else { "\u{25B8}" };
            ui.painter().text(
                egui::pos2(text_x, rect.center().y),
                egui::Align2::LEFT_CENTER,
                glyph,
                font.clone(),
                colors.text,
            );
        }
        text_x += indent * 0.75;
    }
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(text_x, rect.top()),
        egui::pos2(rect.right() - 4.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(text_rect).text(
        egui::pos2(text_rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        &cell.text,
        font.clone(),
        colors.text,
    );
    ui.painter().line_segment(
        [rect.right_top(), rect.right_bottom()],
        egui::Stroke::new(1.0, visual.widgets.noninteractive.bg_stroke.color.gamma_multiply(0.35)),
    );
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0, visual.widgets.noninteractive.bg_stroke.color.gamma_multiply(0.35)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Cells {
        values: Vec<Vec<String>>,
        columns: Vec<GridColumn>,
        reject: bool,
    }

    impl Cells {
        fn new(rows: usize, columns: usize) -> Self {
            Self {
                values: (0..rows)
                    .map(|row| (0..columns).map(|column| format!("{row}:{column}")).collect())
                    .collect(),
                columns: (0..columns)
                    .map(|column| GridColumn::new(format!("C{column}"), 80.0))
                    .collect(),
                reject: false,
            }
        }
    }

    impl GridSource for Cells {
        fn row_count(&self) -> usize {
            self.values.len()
        }
        fn columns(&self) -> &[GridColumn] {
            &self.columns
        }
        fn cell(&self, row: usize, column: usize, out: &mut GridCell) {
            out.text = self.values[row][column].clone();
            out.editable = true;
        }
        fn set_cell(&mut self, row: usize, column: usize, text: &str) -> bool {
            if self.reject {
                return false;
            }
            self.values[row][column] = text.to_owned();
            true
        }
    }

    #[test]
    fn a_click_moves_the_focus_and_the_selection() {
        let mut source = Cells::new(3, 2);
        let mut state = GridState::new(vec![80.0, 80.0]);
        assert_eq!(state.on_click(1, 1, &mut source), None);
        assert_eq!(state.focused_row, Some(1));
        assert_eq!(state.focused_column, 1);
        assert_eq!(state.selected_row, Some(1));
    }

    #[test]
    fn a_double_click_starts_an_edit_and_enter_commits_it() {
        let mut source = Cells::new(2, 2);
        let mut state = GridState::new(vec![80.0, 80.0]);
        let mut cell = GridCell::default();
        source.cell(0, 1, &mut cell);
        state.on_double_click(0, 1, &cell);
        assert_eq!(state.editing.as_ref().unwrap().original, "0:1");
        state.editing.as_mut().unwrap().text = "edited".to_owned();
        let result = state.on_edit_key(egui::Key::Enter, &mut source);
        assert_eq!(
            result,
            Some(GridEditResult::Committed {
                row: 0,
                column: 1,
                text: "edited".to_owned()
            })
        );
        assert_eq!(source.values[0][1], "edited");
        assert!(state.editing.is_none());
    }

    #[test]
    fn escape_cancels_and_an_unchanged_edit_writes_nothing() {
        let mut source = Cells::new(1, 1);
        let mut state = GridState::new(vec![80.0]);
        let cell = GridCell {
            text: "a".to_owned(),
            editable: true,
            ..GridCell::default()
        };
        state.on_double_click(0, 0, &cell);
        assert_eq!(
            state.on_edit_key(egui::Key::Escape, &mut source),
            Some(GridEditResult::Cancelled)
        );
        assert!(state.editing.is_none());
        state.on_double_click(0, 0, &cell);
        assert_eq!(
            state.on_edit_key(egui::Key::Enter, &mut source),
            Some(GridEditResult::Unchanged)
        );
    }

    #[test]
    fn a_rejected_edit_reports_it() {
        let mut source = Cells::new(1, 1);
        source.reject = true;
        let mut state = GridState::new(vec![80.0]);
        let cell = GridCell {
            text: "a".to_owned(),
            editable: true,
            ..GridCell::default()
        };
        state.on_double_click(0, 0, &cell);
        state.editing.as_mut().unwrap().text = "b".to_owned();
        assert_eq!(
            state.on_edit_key(egui::Key::Enter, &mut source),
            Some(GridEditResult::Rejected)
        );
        assert_eq!(source.values[0][0], "0:0");
    }

    #[test]
    fn the_editor_starts_with_the_edit_value() {
        let cell = GridCell {
            text: "Dawnguard \"Dawnguard\" [QUST:0200C97A]".to_owned(),
            edit_text: Some("0200C97A".to_owned()),
            editable: true,
            ..GridCell::default()
        };
        let mut state = GridState::new(vec![80.0]);
        state.on_double_click(0, 1, &cell);
        let edit = state.editing.expect("the editor opens");
        assert_eq!(edit.text, "0200C97A");
        assert_eq!(edit.original, "0200C97A");
    }

    #[test]
    fn a_cell_that_cannot_be_edited_does_not_start_one() {
        let mut state = GridState::new(vec![80.0]);
        state.on_double_click(0, 0, &GridCell::default());
        assert!(state.editing.is_none());
    }

    #[test]
    fn arrow_keys_stay_inside_the_grid() {
        let mut state = GridState::new(vec![80.0, 80.0]);
        assert!(state.on_key(egui::Key::ArrowDown, 3, 2));
        assert_eq!((state.focused_row, state.focused_column), (Some(0), 0));
        state.on_key(egui::Key::ArrowDown, 3, 2);
        state.on_key(egui::Key::ArrowDown, 3, 2);
        state.on_key(egui::Key::ArrowDown, 3, 2);
        assert_eq!(state.focused_row, Some(2));
        state.on_key(egui::Key::End, 3, 2);
        assert_eq!(state.focused_column, 1);
        state.on_key(egui::Key::ArrowRight, 3, 2);
        assert_eq!(state.focused_column, 1);
        assert_eq!(state.scroll_to, Some(state.focused_row.unwrap()));
        assert!(!state.on_key(egui::Key::ArrowDown, 0, 2));
    }

    #[test]
    fn a_toggle_reclamps_the_state_to_the_new_rows() {
        struct Toggling {
            rows: usize,
            columns: Vec<GridColumn>,
        }
        impl GridSource for Toggling {
            fn row_count(&self) -> usize {
                self.rows
            }
            fn columns(&self) -> &[GridColumn] {
                &self.columns
            }
            fn cell(&self, _row: usize, _column: usize, _out: &mut GridCell) {}
            fn toggle_row(&mut self, _row: usize) -> bool {
                self.rows = self.rows.saturating_sub(2);
                true
            }
        }
        let mut source = Toggling {
            rows: 5,
            columns: vec![GridColumn::new("C", 80.0)],
        };
        let mut state = GridState::new(vec![80.0]);
        for _ in 0..5 {
            state.on_key(egui::Key::ArrowDown, 5, 1);
        }
        assert_eq!(state.focused_row, Some(4));
        let mut output = GridOutput::default();
        toggled_row(&mut source, &mut state, 1, &mut output);
        assert_eq!(output.toggled, Some(1));
        assert_eq!(source.row_count(), 3);
        assert_eq!(state.focused_row, Some(2));
    }

    #[test]
    fn the_state_clamps_to_a_smaller_source() {
        let mut state = GridState::new(vec![80.0, 80.0, 80.0]);
        for _ in 0..5 {
            state.on_key(egui::Key::ArrowDown, 5, 3);
        }
        assert_eq!(state.focused_row, Some(4));
        state.focused_column = 2;
        state.selected_row = Some(4);
        state.clamp(2, 1);
        assert_eq!(state.focused_row, Some(1));
        assert_eq!(state.selected_row, Some(1));
        assert_eq!(state.focused_column, 0);
    }
}
