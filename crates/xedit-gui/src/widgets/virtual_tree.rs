// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.dfm (vstNav: TVirtualEditTree with the
// FormID/EditorID/Name columns; vstView: the Labels/Values columns), the
// paint options of both trees (toShowButtons, toShowTreeLines,
// toFullRowSelect, toMultiSelect, toRightClickSelect, toShowHorzGridLines,
// toShowVertGridLines), xeMainForm.pas (vstNavPaintText,
// vstViewBeforeCellPaint, vstViewPaintText, vstNavKeyDown)

//! The virtualised tree primitive: rows with columns, expandable nodes and
//! the conflict colours of `wbColorConflictAll`/`wbColorConflictThis`.
//!
//! Wave-2 agents implement [`TreeSource`] over the session's data (the
//! navigation tree of step 2 and the view tree of step 3) and draw it with
//! [`TreeView::show`]. Only the rows in the visible range are asked for
//! ([`TreeSource::row`] is called for those alone), so a tree over millions
//! of records costs what the screen shows.
//!
//! The split between the model and the painting keeps the state machine
//! testable without an egui context: [`TreeState`] holds the selection,
//! focus and column widths, and [`TreeState::on_click`],
//! [`TreeState::on_key`] and [`TreeState::toggle_row`] are pure functions
//! over it.

use eframe::egui;

use xedit_core::interface::types::{ConflictAll, ConflictThis};

use super::conflict::{ConflictColors, RowColors};

/// One column of a tree: the `Header.Columns` entry of the DFM.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeColumn {
    /// The header text (`FormID`, `EditorID`, `Name`).
    pub title: String,
    /// The width in pixels.
    pub width: f32,
    /// The smallest width a resize allows.
    pub min_width: f32,
}

impl TreeColumn {
    pub fn new(title: impl Into<String>, width: f32) -> Self {
        Self {
            title: title.into(),
            width,
            min_width: 30.0,
        }
    }
}

/// One cell of a row, built when the row is drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeCell {
    pub text: String,
    pub tooltip: Option<String>,
}

impl TreeCell {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tooltip: None,
        }
    }
}

/// One visible row of a tree, built on demand.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow {
    /// A stable node identity, for the selection to survive a refresh.
    pub id: u64,
    /// The indentation level (`vstNav` nodes: file 1, group 2, record 3).
    pub depth: usize,
    /// Whether the node has children (`HasChildren` of `InitNode`).
    pub has_children: bool,
    /// Whether the node is shown expanded.
    pub expanded: bool,
    /// A one-character glyph before the first cell.
    pub icon: Option<char>,
    /// One cell per column; missing cells are empty.
    pub cells: Vec<TreeCell>,
    pub conflict_all: ConflictAll,
    pub conflict_this: ConflictThis,
}

impl Default for TreeRow {
    fn default() -> Self {
        Self {
            id: 0,
            depth: 0,
            has_children: false,
            expanded: false,
            icon: None,
            cells: Vec::new(),
            conflict_all: ConflictAll::caUnknown,
            conflict_this: ConflictThis::ctUnknown,
        }
    }
}

/// The data behind a [`TreeView`]: rows flattened in display order, asked
/// for only for the visible range.
pub trait TreeSource {
    /// The number of rows currently visible (expanded nodes only).
    fn row_count(&self) -> usize;
    /// The columns; their widths in the state win after a resize.
    fn columns(&self) -> &[TreeColumn];
    /// Fills `out` with the row at `index`. Called for visible rows only.
    fn row(&self, index: usize, out: &mut TreeRow);
    /// Expands or collapses the row at `index`; returns whether the visible
    /// row count changed.
    fn toggle(&mut self, index: usize) -> bool {
        let _ = index;
        false
    }
    /// The row's tooltip when it is not the cell's own.
    fn row_tooltip(&self, index: usize) -> Option<&str> {
        let _ = index;
        None
    }
}

/// The selection and focus of a tree, as `vstNav`'s `toMultiSelect` and
/// `toRightClickSelect` keep them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeState {
    /// The selected row indices, in the display order of the last repaint.
    pub selected: Vec<usize>,
    /// The row a Shift-click extends from.
    pub anchor: Option<usize>,
    /// The row with the focus; the keyboard moves it.
    pub focused: Option<usize>,
    /// `FocusedColumn`: the column a click focused (`vstModGroupItemsNodeClick`).
    pub focused_column: usize,
    /// The runtime column widths, from `frmMain.vstNavColumnWidth<i>`.
    pub column_widths: Vec<f32>,
    pub row_height: f32,
    pub indent: f32,
    /// A row to bring into view on the next repaint.
    pub scroll_to: Option<usize>,
}

impl TreeState {
    pub fn new(column_widths: Vec<f32>) -> Self {
        Self {
            selected: Vec::new(),
            anchor: None,
            focused: None,
            focused_column: 0,
            column_widths,
            row_height: 18.0,
            indent: 16.0,
            scroll_to: None,
        }
    }

    /// Keeps the widths in step with a source that adds or removes columns.
    pub fn set_column_widths(&mut self, widths: &[f32]) {
        if self.column_widths.len() != widths.len() {
            self.column_widths = widths.to_vec();
        }
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    /// Replaces the selection with one row.
    pub fn select_only(&mut self, index: usize) {
        self.selected.clear();
        self.selected.push(index);
        self.anchor = Some(index);
        self.focused = Some(index);
    }

    /// Ctrl-click: adds or removes one row.
    pub fn toggle_selection(&mut self, index: usize) {
        if let Some(at) = self.selected.iter().position(|&row| row == index) {
            self.selected.remove(at);
        } else {
            self.selected.push(index);
        }
        self.anchor = Some(index);
        self.focused = Some(index);
    }

    /// Shift-click: selects the range from the anchor to the row.
    pub fn extend_selection(&mut self, index: usize) {
        let anchor = self.anchor.unwrap_or(index);
        let (from, to) = if anchor <= index {
            (anchor, index)
        } else {
            (index, anchor)
        };
        self.selected = (from..=to).collect();
        self.focused = Some(index);
    }

    /// A click with the modifiers of `vstNavMouseDown`/`vstNavClick`.
    pub fn on_click(&mut self, index: usize, modifiers: egui::Modifiers) {
        if modifiers.shift {
            self.extend_selection(index);
        } else if modifiers.command {
            self.toggle_selection(index);
        } else {
            self.select_only(index);
        }
    }

    /// The clicks a row's own reaction handles: a double click toggles an
    /// expanded node (`toToggleOnDblClick`).
    pub fn toggle_row(&mut self, index: usize, source: &mut dyn TreeSource) -> bool {
        if source.toggle(index) {
            // The row count changed; keep the focus and the selection where
            // they were by index, as the tree's auto options do.
            let rows = source.row_count();
            self.clamp(rows);
            true
        } else {
            false
        }
    }

    /// Removes rows that no longer exist.
    pub fn clamp(&mut self, rows: usize) {
        self.selected.retain(|&index| index < rows);
        if self.focused.is_some_and(|index| index >= rows) {
            self.focused = rows.checked_sub(1);
        }
        if self.anchor.is_some_and(|index| index >= rows) {
            self.anchor = None;
        }
    }

    /// The keyboard of `vstNavKeyDown`: the arrows move the focus, Home and
    /// End go to the first and last row, and Left/Right collapse or expand.
    /// Returns an action the caller runs against the source.
    pub fn on_key(&mut self, key: egui::Key, rows: usize) -> Option<TreeKeyAction> {
        if rows == 0 {
            return None;
        }
        let last = rows - 1;
        match key {
            egui::Key::ArrowDown => {
                let index = self.focused.map_or(0, |index| (index + 1).min(last));
                self.select_only(index);
                self.scroll_to = Some(index);
                None
            }
            egui::Key::ArrowUp => {
                let index = self.focused.map_or(0, |index| index.saturating_sub(1));
                self.select_only(index);
                self.scroll_to = Some(index);
                None
            }
            egui::Key::Home => {
                self.select_only(0);
                self.scroll_to = Some(0);
                None
            }
            egui::Key::End => {
                self.select_only(last);
                self.scroll_to = Some(last);
                None
            }
            egui::Key::ArrowRight | egui::Key::ArrowLeft => self.focused.map(TreeKeyAction::Toggle),
            egui::Key::Enter => self.focused.map(TreeKeyAction::Activate),
            _ => None,
        }
    }
}

/// What a key wants done to a source-backed row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKeyAction {
    /// Expand or collapse (Left/Right; the source decides which applies).
    Toggle(usize),
    /// Enter: show the row's record.
    Activate(usize),
}

/// What the user did to the tree this repaint.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeOutput {
    /// The row clicked with the left button.
    pub clicked: Option<usize>,
    /// The row right-clicked (for the context menu).
    pub secondary_clicked: Option<usize>,
    /// The row double-clicked.
    pub double_clicked: Option<usize>,
    /// The row activated (Enter or a double click without children).
    pub activated: Option<usize>,
    /// The row whose check glyph was clicked (`toCheckSupport` of the
    /// selection dialogs): the click is left of the first cell's text, on
    /// the `icon` the row paints.
    pub check_clicked: Option<usize>,
    /// The column the clicked row was clicked in (`vstModGroupItemsNodeClick`
    /// toggles the flag of `HitColumn`).
    pub clicked_column: Option<usize>,
    /// The row whose expander triangle was clicked (`toToggleOnDblClick`
    /// and the tree's button).
    pub expander_clicked: Option<usize>,
    /// The column whose header was clicked (`vstModulesHeaderClick`).
    pub header_clicked: Option<usize>,
    /// A key action against the source.
    pub key: Option<TreeKeyAction>,
}

/// The `TVirtualEditTree` of the main form drawn over a [`TreeSource`].
pub struct TreeView<'a> {
    pub id: egui::Id,
    pub source: &'a mut dyn TreeSource,
    pub state: &'a mut TreeState,
    pub colors: &'a ConflictColors,
    /// Whether the header is painted (`Header.Options` of the DFM).
    pub header: bool,
}

impl<'a> TreeView<'a> {
    pub fn show(self, ui: &mut egui::Ui) -> TreeOutput {
        let TreeView {
            id,
            source,
            state,
            colors,
            header,
        } = self;
        let mut output = TreeOutput::default();
        let visual = ui.visuals().clone();
        let dark = visual.dark_mode;
        let font = ui.style().text_styles[&egui::TextStyle::Body].clone();
        state.set_column_widths(&source.columns().iter().map(|column| column.width).collect::<Vec<_>>());

        if header {
            output.header_clicked = show_header(id, ui, source, state, &font, &visual);
        }

        let row_height = state.row_height;
        let rows = source.row_count();
        let columns = state.column_widths.clone();
        let total_width = columns.iter().sum::<f32>();
        let scroll_to = state.scroll_to.take();
        let indent = state.indent;

        egui::ScrollArea::vertical()
            .id_salt(id.with("rows"))
            .auto_shrink([false, false])
            .show_rows(ui, row_height, rows, |ui, range| {
                ui.set_width(total_width.max(ui.available_width()));
                for index in range {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), egui::Sense::click());
                    let mut row = TreeRow::default();
                    source.row(index, &mut row);
                    let selected = state.is_selected(index);
                    let row_colors = colors.row_colors(row.conflict_all, row.conflict_this, visual.text_color(), dark);
                    paint_row(ui, rect, &row, selected, &row_colors, &visual, &font, &columns, indent);
                    if response.clicked() {
                        let modifiers = ui.input(|input| input.modifiers);
                        if state.focused != Some(index) {
                            response.request_focus();
                        }
                        state.on_click(index, modifiers);
                        if let Some(pos) = response.interact_pointer_pos() {
                            let mut left = rect.left();
                            for (column, width) in columns.iter().enumerate() {
                                if pos.x >= left && pos.x < left + width {
                                    state.focused_column = column;
                                    output.clicked_column = Some(column);
                                    break;
                                }
                                left += width;
                            }
                        }
                        // A click on the expander triangle toggles the node
                        // (`toToggleOnDblClick` and the tree's button click).
                        let on_expander = row.has_children
                            && response.interact_pointer_pos().is_some_and(|pos| {
                                let expander_left = rect.left() + 3.0 + row.depth as f32 * indent;
                                pos.x >= expander_left - 2.0 && pos.x <= expander_left + font.size + 2.0
                            });
                        if on_expander {
                            state.toggle_row(index, source);
                            output.expander_clicked = Some(index);
                        } else if row.icon.is_some()
                            && response.interact_pointer_pos().is_some_and(|pos| {
                                // The glyph `paint_row` draws after the
                                // indentation and the expander.
                                let glyph_left = rect.left()
                                    + 3.0
                                    + row.depth as f32 * indent
                                    + if row.has_children { indent * 0.75 } else { 0.0 };
                                pos.x >= glyph_left - 2.0 && pos.x <= glyph_left + font.size + 2.0
                            })
                        {
                            // `vstModulesChecked`: a click on the check box.
                            output.check_clicked = Some(index);
                        }
                        output.clicked = Some(index);
                    }
                    if response.secondary_clicked() {
                        state.select_only(index);
                        output.secondary_clicked = Some(index);
                    }
                    if response.double_clicked() {
                        output.double_clicked = Some(index);
                        if row.has_children {
                            state.toggle_row(index, source);
                        } else {
                            output.activated = Some(index);
                        }
                    }
                    if let Some(tooltip) = source.row_tooltip(index) {
                        let _ = response.on_hover_text(tooltip.to_owned());
                    } else if let Some(tooltip) = row.cells.first().and_then(|cell| cell.tooltip.as_ref()) {
                        let _ = response.on_hover_text(tooltip.clone());
                    }
                    if scroll_to == Some(index) {
                        ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    }
                }
            });

        // The keyboard belongs to the tree while it has the focus.
        if ui.memory(|memory| memory.has_focus(id)) {
            for key in [
                egui::Key::ArrowDown,
                egui::Key::ArrowUp,
                egui::Key::Home,
                egui::Key::End,
                egui::Key::ArrowLeft,
                egui::Key::ArrowRight,
                egui::Key::Enter,
            ] {
                if ui.input(|input| input.key_pressed(key)) {
                    output.key = state.on_key(key, rows);
                    ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key));
                }
            }
        }
        output
    }
}

/// The header row, with a drag handle between two columns.
fn show_header(
    id: egui::Id,
    ui: &mut egui::Ui,
    source: &mut dyn TreeSource,
    state: &mut TreeState,
    font: &egui::FontId,
    visual: &egui::Visuals,
) -> Option<usize> {
    let height = state.row_height + 3.0;
    let widths: Vec<f32> = state.column_widths.clone();
    let (rect, header_response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(widths.iter().sum()), height),
        egui::Sense::click(),
    );
    let mut clicked = None;
    if header_response.clicked()
        && let Some(pos) = header_response.interact_pointer_pos()
    {
        let mut left = rect.left();
        for (index, width) in widths.iter().enumerate() {
            if pos.x >= left && pos.x < left + width {
                clicked = Some(index);
                break;
            }
            left += width;
        }
    }
    let mut x = rect.left();
    for (index, width) in widths.iter().enumerate() {
        if index < source.columns().len() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, visual.widgets.inactive.bg_fill);
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
            let min = source.columns().get(index).map_or(30.0, |column| column.min_width);
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
    clicked
}

/// Paints one row: the conflict background, the selection blend, the
/// expander, the indentation and the cells.
#[allow(clippy::too_many_arguments)]
fn paint_row(
    ui: &egui::Ui,
    rect: egui::Rect,
    row: &TreeRow,
    selected: bool,
    colors: &RowColors,
    visual: &egui::Visuals,
    font: &egui::FontId,
    columns: &[f32],
    indent: f32,
) {
    let painter = ui.painter();
    if let Some(background) = colors.background {
        painter.rect_filled(rect, egui::CornerRadius::ZERO, background);
    }
    if selected {
        // `SelectionBlendFactor = 80` of the navigation tree: the conflict
        // colours still show through the selection.
        painter.rect_filled(
            rect,
            egui::CornerRadius::ZERO,
            visual.selection.bg_fill.gamma_multiply(0.55),
        );
    }
    let mut x = rect.left();
    for (column, width) in columns.iter().enumerate() {
        let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*width, rect.height()));
        let cell_painter = painter.with_clip_rect(cell.shrink2(egui::vec2(0.0, 0.0)));
        let mut text_x = cell.left() + 3.0;
        if column == 0 {
            text_x += row.depth as f32 * indent;
            if row.has_children {
                let glyph = if row.expanded { "\u{25BE}" } else { "\u{25B8}" };
                cell_painter.text(
                    egui::pos2(text_x, cell.center().y),
                    egui::Align2::LEFT_CENTER,
                    glyph,
                    font.clone(),
                    colors.text,
                );
            }
            text_x += indent * 0.75;
            if let Some(icon) = row.icon {
                cell_painter.text(
                    egui::pos2(text_x, cell.center().y),
                    egui::Align2::LEFT_CENTER,
                    icon.to_string(),
                    font.clone(),
                    colors.text,
                );
                text_x += indent * 0.75;
            }
        }
        if let Some(cell_text) = row.cells.get(column)
            && !cell_text.text.is_empty()
        {
            cell_painter.text(
                egui::pos2(text_x, cell.center().y),
                egui::Align2::LEFT_CENTER,
                &cell_text.text,
                font.clone(),
                colors.text,
            );
        }
        x += width;
    }
    // `toShowHorzGridLines`.
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0, visual.widgets.noninteractive.bg_stroke.color.gamma_multiply(0.35)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rows {
        rows: usize,
        columns: Vec<TreeColumn>,
        toggled: Vec<usize>,
    }

    impl Rows {
        fn new(rows: usize) -> Self {
            Self {
                rows,
                columns: vec![TreeColumn::new("FormID", 201.0)],
                toggled: Vec::new(),
            }
        }
    }

    impl TreeSource for Rows {
        fn row_count(&self) -> usize {
            self.rows
        }
        fn columns(&self) -> &[TreeColumn] {
            &self.columns
        }
        fn row(&self, index: usize, out: &mut TreeRow) {
            out.id = index as u64;
            out.depth = index % 3;
            out.cells = vec![TreeCell::new(format!("row {index}"))];
        }
        fn toggle(&mut self, index: usize) -> bool {
            self.toggled.push(index);
            true
        }
    }

    fn no_modifiers() -> egui::Modifiers {
        egui::Modifiers::NONE
    }

    fn ctrl() -> egui::Modifiers {
        egui::Modifiers {
            ctrl: true,
            command: true,
            ..egui::Modifiers::NONE
        }
    }

    fn shift() -> egui::Modifiers {
        egui::Modifiers {
            shift: true,
            ..egui::Modifiers::NONE
        }
    }

    #[test]
    fn a_plain_click_replaces_the_selection() {
        let mut state = TreeState::new(vec![201.0]);
        state.on_click(2, no_modifiers());
        assert_eq!(state.selected, vec![2]);
        state.on_click(5, no_modifiers());
        assert_eq!(state.selected, vec![5]);
        assert_eq!(state.focused, Some(5));
    }

    #[test]
    fn ctrl_click_toggles_and_shift_click_extends() {
        let mut state = TreeState::new(vec![201.0]);
        state.on_click(2, no_modifiers());
        state.on_click(4, ctrl());
        assert_eq!(state.selected, vec![2, 4]);
        state.on_click(4, ctrl());
        assert_eq!(state.selected, vec![2]);
        // The anchor is the row of the last click, the Ctrl-clicked 4.
        state.on_click(1, shift());
        assert_eq!(state.selected, vec![1, 2, 3, 4]);
        assert_eq!(state.anchor, Some(4));
    }

    #[test]
    fn the_keyboard_moves_the_focus_and_scrolls_to_it() {
        let mut state = TreeState::new(vec![201.0]);
        assert_eq!(state.on_key(egui::Key::ArrowDown, 10), None);
        assert_eq!(state.focused, Some(0));
        assert_eq!(state.scroll_to, Some(0));
        state.on_key(egui::Key::ArrowDown, 10);
        assert_eq!(state.focused, Some(1));
        state.on_key(egui::Key::End, 10);
        assert_eq!(state.focused, Some(9));
        state.on_key(egui::Key::ArrowDown, 10);
        assert_eq!(state.focused, Some(9));
        state.on_key(egui::Key::Home, 10);
        assert_eq!(state.focused, Some(0));
        state.on_key(egui::Key::ArrowUp, 10);
        assert_eq!(state.focused, Some(0));
        assert_eq!(state.on_key(egui::Key::Enter, 10), Some(TreeKeyAction::Activate(0)));
        assert_eq!(state.on_key(egui::Key::ArrowRight, 10), Some(TreeKeyAction::Toggle(0)));
        assert_eq!(state.on_key(egui::Key::ArrowDown, 0), None);
    }

    #[test]
    fn a_toggle_is_forwarded_to_the_source() {
        let mut state = TreeState::new(vec![201.0]);
        let mut rows = Rows::new(5);
        state.on_click(3, no_modifiers());
        assert!(state.toggle_row(3, &mut rows));
        assert_eq!(rows.toggled, vec![3]);
        assert_eq!(state.focused, Some(3));
    }

    #[test]
    fn rows_that_disappear_leave_the_selection() {
        let mut state = TreeState::new(vec![201.0]);
        state.on_click(8, no_modifiers());
        state.on_click(2, ctrl());
        state.clamp(4);
        assert_eq!(state.selected, vec![2]);
        assert_eq!(state.focused, Some(2));
        state.clamp(0);
        assert!(state.selected.is_empty());
        assert_eq!(state.focused, None);
    }

    #[test]
    fn the_column_widths_follow_the_source() {
        let mut state = TreeState::new(vec![1.0, 2.0]);
        state.set_column_widths(&[10.0, 20.0, 30.0]);
        assert_eq!(state.column_widths, vec![10.0, 20.0, 30.0]);
        // The same count keeps the user's resized widths.
        state.column_widths[0] = 99.0;
        state.set_column_widths(&[10.0, 20.0, 30.0]);
        assert_eq!(state.column_widths[0], 99.0);
    }
}
