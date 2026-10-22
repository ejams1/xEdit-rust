// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeLegendForm.pas (FormClose, FormCreate,
// FormDestroy, FormHide, FormShow, dgLegendDrawCell, the CellSizeCalc
// pass) and xEdit/xeLegendForm.dfm (frmLegend, dgLegend) and
// xEdit/xeMainForm.pas (bnLegendClick, UpdateLegend)

//! `TfrmLegend`: the grid of the conflict colours, a row per
//! `TConflictAll`, a column per `TConflictThis`.
//!
//! `dgLegendDrawCell` paints a cell with `wbLighter(ConflictAllToColor(ca),
//! 0.85)` and the text in `wbDarker(ConflictThisToColor(ct))`; the first
//! column and the first row are the names (`wbNameConflictAll` and
//! `wbNameConflictThis`). A row of `caUnknown` or `caOnlyOne` has no
//! background (`clWindow`), as upstream paints it.
//!
//! The form is a tool window the user shows with `bnLegend`; the placement
//! (`[frmLegend] Left`, `Top`, `Width`, `Height`, `WindowState`) is read by
//! `FormCreate`/`FormShow` and written by `FormHide` through
//! `settings.get`/`settings.set`.

use eframe::egui;
use xedit_core::interface::types::{ConflictAll, ConflictThis, PascalEnum};

use crate::dialogs::selection::{DialogPlacement, fit_size};
use crate::widgets::conflict::{
    CONFLICT_ALL_COLORS, CONFLICT_THIS_COLORS, ConflictColors, SYSTEM_WINDOW_TEXT, wb_darker, wb_lighter,
};

/// The section of the placement keys (`TfrmLegend.Name`).
pub const FORM_SECTION: &str = "frmLegend";

/// The cell size pass of `dgLegendDrawCell`: `0` measures, `1` fills, `2`
/// draws (upstream runs the measure pass once, from the first draw).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellSizeCalc {
    /// `0`: the measure pass.
    #[default]
    Measure,
    /// `1`: the plain fill.
    Fill,
    /// `2`: the cell draw.
    Draw,
}

/// What the legend asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegendAction {
    /// `FormClose` with `Action := caHide` and `bnLegend.Down := False`.
    Closed,
}

/// The colours of one grid cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegendCell {
    /// `Brush.Color`: `wbLighter(ConflictAllToColor(ca), 0.85)` for
    /// `caNoConflict` and above, `clWindow` (`None`) below.
    pub background: Option<egui::Color32>,
    /// `Font.Color`: `wbDarker(ConflictThisToColor(ct))`.
    pub text: egui::Color32,
    /// The name the cell shows.
    pub label: &'static str,
}

/// `dgLegendDrawCell`: the cell of row `row` (`TConflictAll`) and column
/// `column` (`TConflictThis`). Column 0 is `wbNameConflictAll[row]`, row 0
/// is `wbNameConflictThis[column]`; the body cells are the intersection
/// names with the conflict colours. `window_text` resolves `clWindowText`.
pub fn legend_cell(row: ConflictAll, column: ConflictThis, window_text: egui::Color32, dark_mode: bool) -> LegendCell {
    if column.ord() == 0 {
        // `if aCol = 0 then ... wbNameConflictAll[TConflictAll(aRow)]`.
        return LegendCell {
            background: None,
            text: window_text,
            label: row.name(),
        };
    }
    let this = ConflictThis::ALL[column.ord()];
    if row.ord() == 0 {
        // `if ARow > 0 then ...`: the header row has no colours.
        return LegendCell {
            background: None,
            text: window_text,
            label: this.name(),
        };
    }
    // `if ConflictAll >= caNoConflict then Brush.Color :=
    // wbLighter(ConflictAllToColor(ConflictAll), 0.85) else clWindow`.
    let background = if row.ord() >= ConflictAll::caNoConflict.ord() {
        CONFLICT_ALL_COLORS[row.ord()].map(|color| wb_lighter(color, 0.85, dark_mode))
    } else {
        None
    };
    // `Font.Color := wbDarker(ConflictThisToColor(ConflictThis))`.
    let color = CONFLICT_THIS_COLORS[column.ord()];
    let text = if color == SYSTEM_WINDOW_TEXT {
        window_text
    } else {
        wb_darker(color, 0.25, dark_mode)
    };
    LegendCell {
        background,
        text,
        label: this.name(),
    }
}

/// The legend with the user's colours (`wbColorConflictAll`/
/// `wbColorConflictThis` are the tables the options dialog edits).
pub fn legend_cell_with(
    colors: &ConflictColors,
    row: ConflictAll,
    column: ConflictThis,
    window_text: egui::Color32,
    dark_mode: bool,
) -> LegendCell {
    if column.ord() == 0 || row.ord() == 0 {
        return legend_cell(row, column, window_text, dark_mode);
    }
    let all = colors.all[row.ord()];
    let this = colors.this[column.ord()];
    LegendCell {
        background: all.map(|color| wb_lighter(color, 0.85, dark_mode)),
        text: if colors.this_system[column.ord()] {
            window_text
        } else {
            wb_darker(this, 0.25, dark_mode)
        },
        label: ConflictThis::ALL[column.ord()].name(),
    }
}

/// `TfrmLegend` as data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LegendState {
    /// `CellSizeCalc`, which the draw pass advances once.
    pub cell_size_calc: CellSizeCalc,
    /// The placement read by `FormCreate`/`FormShow`.
    pub placement: Option<DialogPlacement>,
    /// The rectangle the window had this frame (`FormHide` writes it).
    pub rect: Option<egui::Rect>,
}

impl LegendState {
    /// `FormCreate`: the placement of `[frmLegend]`.
    pub fn new(placement: Option<DialogPlacement>) -> Self {
        Self {
            cell_size_calc: CellSizeCalc::Measure,
            placement,
            rect: None,
        }
    }

    /// The grid's shape: `Succ(Ord(High(TConflictAll)))` rows and
    /// `Succ(Ord(High(TConflictThis)))` columns.
    pub fn rows(&self) -> usize {
        ConflictAll::ALL.len()
    }

    pub fn columns(&self) -> usize {
        ConflictThis::ALL.len()
    }
}

/// Draws the legend; `Some(LegendAction::Closed)` when its close button was
/// used (the form hides, `caHide`).
pub fn show(ctx: &egui::Context, state: &mut LegendState) -> Option<LegendAction> {
    let mut open = true;
    let mut window = egui::Window::new("Legend")
        .id(egui::Id::unique("frmLegend"))
        .collapsible(false)
        .resizable(true)
        .default_size(fit_size(ctx, egui::vec2(1296.0, 636.0)));
    window = match state.placement {
        Some(placement) => placement.apply(ctx, window),
        None => window.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)),
    };
    let response = window.open(&mut open).show(ctx, |ui| {
        // `CellSizeCalc`: the first draw measures the columns, the second
        // fills, then the cells draw. The port measures the text with the
        // current font and settles at once.
        state.cell_size_calc = CellSizeCalc::Draw;
        let window_text = ui.visuals().text_color();
        let dark_mode = ui.visuals().dark_mode;
        egui::ScrollArea::both()
            .id_salt("legend-grid")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("legend-cells").spacing([2.0, 2.0]).show(ui, |ui| {
                    for row in ConflictAll::ALL {
                        for column in ConflictThis::ALL {
                            let cell = legend_cell(*row, *column, window_text, dark_mode);
                            // `dgLegendDrawCell` fills the whole cell with
                            // `Brush.Color`; a `Frame` is the port's fill.
                            let mut frame = egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 2));
                            if let Some(background) = cell.background {
                                frame = frame.fill(background);
                            }
                            frame.show(ui, |ui| {
                                let text = egui::RichText::new(cell.label).color(cell.text);
                                let label = ui.add_sized([74.0, 16.0], egui::Label::new(text));
                                if cell.label.is_empty() {
                                    label.on_hover_text(format!("{:?} / {:?}", row, column));
                                }
                            });
                        }
                        ui.end_row();
                    }
                });
            });
    });
    state.rect = response.map(|response| response.response.rect);
    if !open {
        return Some(LegendAction::Closed);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::conflict::{darker, lighter};

    #[test]
    fn the_grid_shape_is_the_upstream_enumeration() {
        let state = LegendState::default();
        // `dgLegend.RowCount := Succ(Ord(High(TConflictAll)))` = 7.
        assert_eq!(state.rows(), 7);
        // `dgLegend.ColCount := Succ(Ord(High(TConflictThis)))` = 12.
        assert_eq!(state.columns(), 12);
    }

    #[test]
    fn the_body_cells_use_the_lighter_and_darker_roles() {
        // `wbInterface.pas:524-547`: caConflict is clRed, ctConflictWins is
        // clOrange; the cell is `wbLighter(clRed, 0.85)` on
        // `wbDarker(clOrange)`, which is the colour itself outside the dark
        // mode.
        let window_text = egui::Color32::from_rgb(1, 2, 3);
        let cell = legend_cell(
            ConflictAll::caConflict,
            ConflictThis::ctConflictWins,
            window_text,
            false,
        );
        assert_eq!(cell.background, Some(lighter(CONFLICT_ALL_COLORS[5].unwrap(), 0.85)));
        assert_eq!(cell.text, CONFLICT_THIS_COLORS[10]);
        assert_eq!(cell.label, "Conflict winner");
        // `caOverride` (yellow) and `ctOverride` (green).
        let cell = legend_cell(ConflictAll::caOverride, ConflictThis::ctOverride, window_text, false);
        assert_eq!(cell.background, Some(lighter(CONFLICT_ALL_COLORS[4].unwrap(), 0.85)));
        assert_eq!(cell.text, CONFLICT_THIS_COLORS[8]);
    }

    #[test]
    fn the_first_column_and_row_are_the_names() {
        let window_text = egui::Color32::from_rgb(1, 2, 3);
        let cell = legend_cell(ConflictAll::caConflict, ConflictThis::ctUnknown, window_text, false);
        assert_eq!(cell.label, "Conflict");
        assert_eq!(cell.background, None);
        let cell = legend_cell(ConflictAll::caUnknown, ConflictThis::ctMaster, window_text, false);
        assert_eq!(cell.label, "Master");
        assert_eq!(cell.background, None);
    }

    #[test]
    fn the_rows_below_no_conflict_have_no_background() {
        let window_text = egui::Color32::from_rgb(1, 2, 3);
        // Row 0 (`caUnknown`) is the header row: its text is the theme's.
        let cell = legend_cell(
            ConflictAll::caUnknown,
            ConflictThis::ctConflictLoses,
            window_text,
            false,
        );
        assert_eq!(cell.background, None);
        assert_eq!(cell.text, window_text);
        // `caOnlyOne` has a body cell without a background (`clWindow`) and
        // keeps the ConflictThis colour, as `wbDarker` leaves it in the
        // light mode.
        let cell = legend_cell(
            ConflictAll::caOnlyOne,
            ConflictThis::ctConflictLoses,
            window_text,
            false,
        );
        assert_eq!(cell.background, None);
        assert_eq!(cell.text, CONFLICT_THIS_COLORS[ConflictThis::ctConflictLoses.ord()]);
    }

    #[test]
    fn a_system_text_colour_follows_the_theme() {
        let window_text = egui::Color32::from_rgb(9, 9, 9);
        let cell = legend_cell(ConflictAll::caConflict, ConflictThis::ctOnlyOne, window_text, false);
        assert_eq!(cell.text, window_text);
    }

    #[test]
    fn the_dark_mode_inverts_the_brightness_roles() {
        let window_text = egui::Color32::from_rgb(9, 9, 9);
        let cell = legend_cell(ConflictAll::caConflict, ConflictThis::ctConflictWins, window_text, true);
        assert_eq!(
            cell.background,
            Some(darker(CONFLICT_ALL_COLORS[5].unwrap(), 0.85 * 0.95))
        );
        assert_eq!(cell.text, lighter(CONFLICT_THIS_COLORS[10], 0.25));
    }
}
