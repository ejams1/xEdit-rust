// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas (wbColorConflictAll,
// wbColorConflictThis, ConflictAllToColor, ConflictThisToColor, Lighter,
// Darker, wbLighter, wbDarker, RGBtoHSL, HSLtoRGB, wbNameConflictAll,
// wbNameConflictThis), xEdit/xeMainForm.pas (FormShow: the ColorConflictAll
// and ColorConflictThis settings, vstViewBeforeCellPaint, vstViewPaintText)

//! The conflict colour roles of the tree and the grid: the background a row
//! gets from its `TConflictAll` and the text colour of each cell from its
//! `TConflictThis`, with the user's colours from the settings file.
//!
//! Upstream paints a node with `wbLighter(ConflictAllToColor(ca), 0.85)` and
//! the text with `wbDarker(ConflictThisToColor(ct))` (`vstViewBeforeCellPaint`
//! and `vstViewPaintText`); in the navigation tree the same pair colours the
//! node label (`vstNavPaintText`). The colour tables are indexed by the
//! Pascal enumeration order, so a value of
//! [`xedit_core::interface::types::ConflictAll`] indexes them directly.

use eframe::egui::Color32;
use xedit_core::interface::types::{ConflictAll, ConflictThis, PascalEnum};

/// `clWindowText` of Delphi: a system colour, resolved from the egui theme.
pub const SYSTEM_WINDOW_TEXT: Color32 = Color32::PLACEHOLDER;

/// `clLime`: `caNoConflict`.
const LIME: Color32 = Color32::from_rgb(0, 255, 0);
/// `TColors.GreenYellow`.
const GREEN_YELLOW: Color32 = Color32::from_rgb(173, 255, 47);
/// `clYellow`.
const YELLOW: Color32 = Color32::from_rgb(255, 255, 0);
/// `clRed`.
const RED: Color32 = Color32::from_rgb(255, 0, 0);
/// `clFuchsia`.
const FUCHSIA: Color32 = Color32::from_rgb(255, 0, 255);
/// `clMedGray`.
const MED_GRAY: Color32 = Color32::from_rgb(160, 160, 160);
/// `clDkGray`.
const DK_GRAY: Color32 = Color32::from_rgb(64, 64, 64);
/// `clLtGray`.
const LT_GRAY: Color32 = Color32::from_rgb(192, 192, 192);
/// `clPurple`.
const PURPLE: Color32 = Color32::from_rgb(128, 0, 128);
/// `clGreen`.
const GREEN: Color32 = Color32::from_rgb(0, 128, 0);
/// `clOlive`.
const OLIVE: Color32 = Color32::from_rgb(128, 128, 0);
/// `clOrange`: RGB(255, 128, 0).
const ORANGE: Color32 = Color32::from_rgb(255, 128, 0);

/// The colour of `TConflictAll`; `None` is `clDefault`, which paints no
/// background of its own.
pub const CONFLICT_ALL_COLORS: [Option<Color32>; 7] = [
    None,               // caUnknown: clDefault
    None,               // caOnlyOne: clDefault
    Some(LIME),         // caNoConflict
    Some(GREEN_YELLOW), // caConflictBenign
    Some(YELLOW),       // caOverride
    Some(RED),          // caConflict
    Some(FUCHSIA),      // caConflictCritical
];

/// The colour of `TConflictThis`. [`SYSTEM_WINDOW_TEXT`] is `clWindowText`,
/// which the painter resolves to the theme's text colour.
pub const CONFLICT_THIS_COLORS: [Color32; 12] = [
    SYSTEM_WINDOW_TEXT, // ctUnknown
    SYSTEM_WINDOW_TEXT, // ctIgnored
    MED_GRAY,           // ctNotDefined
    DK_GRAY,            // ctIdenticalToMaster
    SYSTEM_WINDOW_TEXT, // ctOnlyOne
    LT_GRAY,            // ctHiddenByModGroup
    PURPLE,             // ctMaster
    SYSTEM_WINDOW_TEXT, // ctConflictBenign
    GREEN,              // ctOverride
    OLIVE,              // ctIdenticalToMasterWinsConflict
    ORANGE,             // ctConflictWins
    RED,                // ctConflictLoses
];

/// The colours a row is painted with, as `vstViewBeforeCellPaint` and
/// `vstViewPaintText` compute them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowColors {
    /// The row background (`wbLighter(ConflictAllToColor(ca), 0.85)`).
    pub background: Option<Color32>,
    /// The cell text (`wbDarker(ConflictThisToColor(ct))`).
    pub text: Color32,
}

/// Port of `wbLighter`: brightens, or darkens in the dark mode.
pub fn wb_lighter(color: Color32, amount: f64, dark_mode: bool) -> Color32 {
    if dark_mode {
        darker(color, amount * 0.95)
    } else {
        lighter(color, amount)
    }
}

/// Port of `wbDarker`: the colour itself, unless the dark mode brightens it.
pub fn wb_darker(color: Color32, amount: f64, dark_mode: bool) -> Color32 {
    if dark_mode { lighter(color, amount) } else { color }
}

/// Port of `Lighter` (Vcl.GraphUtil): the HSL lightness moved towards white.
pub fn lighter(color: Color32, amount: f64) -> Color32 {
    let (h, s, l) = rgb_to_hsl(color);
    let l = (l + (1.0 - l) * amount).min(1.0);
    hsl_to_rgb(h, s, l)
}

/// Port of `Darker` (Vcl.GraphUtil): the HSL lightness moved towards black.
pub fn darker(color: Color32, amount: f64) -> Color32 {
    let (h, s, l) = rgb_to_hsl(color);
    let l = (l - l * amount).max(0.0);
    hsl_to_rgb(h, s, l)
}

/// Port of `RGBtoHSL` of `wbInterface.pas` (`H`, `S` and `L` in 0..1).
fn rgb_to_hsl(color: Color32) -> (f64, f64, f64) {
    let r = f64::from(color.r()) / 255.0;
    let g = f64::from(color.g()) / 255.0;
    let b = f64::from(color.b()) / 255.0;
    let cmax = if r > b && r > g {
        r
    } else if g > b {
        g
    } else {
        b
    };
    let cmin = if r < b && r < g {
        r
    } else if g < b {
        g
    } else {
        b
    };
    let l = (cmax + cmin) / 2.0;
    if cmax == cmin {
        return (0.0, 0.0, l);
    }
    let delta = cmax - cmin;
    let s = if l <= 0.5 {
        delta / (cmax + cmin)
    } else {
        delta / (2.0 - cmax - cmin)
    };
    let mut h = if r == cmax {
        (g - b) / delta
    } else if g == cmax {
        2.0 + (b - r) / delta
    } else {
        4.0 + (r - g) / delta
    };
    h /= 6.0;
    if h < 0.0 {
        h += 1.0;
    }
    (h, s, l)
}

/// `HuetoRGB` of `HSLtoRGB`.
fn hue_to_rgb(m1: f64, m2: f64, mut h: f64) -> f64 {
    if h < 0.0 {
        h += 1.0;
    } else if h > 1.0 {
        h -= 1.0;
    }
    if 6.0 * h < 1.0 {
        m1 + (m2 - m1) * h * 6.0
    } else if 2.0 * h < 1.0 {
        m2
    } else if 3.0 * h < 2.0 {
        m1 + (m2 - m1) * ((2.0 / 3.0) - h) * 6.0
    } else {
        m1
    }
}

/// Port of `HSLtoRGB`; the components are rounded as Delphi's `Round`.
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> Color32 {
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let m2 = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let m1 = 2.0 * l - m2;
        (
            hue_to_rgb(m1, m2, h + 1.0 / 3.0),
            hue_to_rgb(m1, m2, h),
            hue_to_rgb(m1, m2, h - 1.0 / 3.0),
        )
    };
    let component = |value: f64| {
        let scaled = value * 255.0;
        // Delphi's `Round` is half to even.
        let rounded = if (scaled - scaled.floor() - 0.5).abs() < f64::EPSILON {
            let floor = scaled.floor();
            if floor as i64 % 2 == 0 { floor } else { floor + 1.0 }
        } else {
            scaled.round()
        };
        rounded.clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(component(r), component(g), component(b))
}

/// The colours of a row, with `clWindowText` resolved to `window_text` and
/// the brightness role of the dark mode applied.
pub fn row_colors(
    conflict_all: ConflictAll,
    conflict_this: ConflictThis,
    window_text: Color32,
    dark_mode: bool,
) -> RowColors {
    let all = CONFLICT_ALL_COLORS[conflict_all.ord()];
    let this = CONFLICT_THIS_COLORS[conflict_this.ord()];
    RowColors {
        background: all.map(|color| wb_lighter(color, 0.85, dark_mode)),
        text: if this == SYSTEM_WINDOW_TEXT {
            window_text
        } else {
            wb_darker(this, 0.25, dark_mode)
        },
    }
}

/// The conflict colours the user's settings file overrides
/// (`wbColorConflictAll` and `wbColorConflictThis` are plain arrays upstream,
/// read and written by FormShow and SaveSettings).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictColors {
    pub all: [Option<Color32>; 7],
    pub this: [Color32; 12],
    /// Entries that are a Delphi system colour (`clWindowText` and the like)
    /// keep the role of the default table rather than a literal colour.
    pub this_system: [bool; 12],
}

impl Default for ConflictColors {
    fn default() -> Self {
        Self {
            all: CONFLICT_ALL_COLORS,
            this: CONFLICT_THIS_COLORS,
            this_system: [
                true, true, false, false, true, false, false, true, false, false, false, false,
            ],
        }
    }
}

impl ConflictColors {
    /// The row colours with this table.
    pub fn row_colors(
        &self,
        conflict_all: ConflictAll,
        conflict_this: ConflictThis,
        window_text: Color32,
        dark_mode: bool,
    ) -> RowColors {
        let all = self.all[conflict_all.ord()];
        let this = self.this[conflict_this.ord()];
        RowColors {
            background: all.map(|color| wb_lighter(color, 0.85, dark_mode)),
            text: if self.this_system[conflict_this.ord()] {
                window_text
            } else {
                wb_darker(this, 0.25, dark_mode)
            },
        }
    }

    /// `TColor` of a colour as Delphi stores it in the settings file:
    /// `$00BBGGRR` (the null byte first).
    pub fn tcolor(color: Color32) -> i32 {
        (i32::from(color.b()) << 16) | (i32::from(color.g()) << 8) | i32::from(color.r())
    }

    /// A `TColor` read from the settings file. A system colour
    /// (`$02xxxxxx`) keeps `fallback`'s role: the GUI has no VCL system
    /// palette to map it to.
    pub fn from_tcolor(value: i32, fallback: Color32) -> (Color32, bool) {
        if value & 0xFF00_0000u32 as i32 != 0 {
            (fallback, true)
        } else {
            (
                Color32::from_rgb(
                    (value & 0xFF) as u8,
                    ((value >> 8) & 0xFF) as u8,
                    ((value >> 16) & 0xFF) as u8,
                ),
                false,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lighter_and_darker_follow_the_delphi_helpers() {
        // `Lighter(clRed, 0.5)` moves the lightness to 0.75: RGB(255, 128, 128).
        assert_eq!(lighter(RED, 0.5), Color32::from_rgb(255, 128, 128));
        // `Darker(clRed, 0.5)` halves it: RGB(128, 0, 0).
        assert_eq!(darker(RED, 0.5), Color32::from_rgb(128, 0, 0));
        // Saturation 0 stays gray.
        let gray = Color32::from_rgb(128, 128, 128);
        assert_eq!(lighter(gray, 0.25), Color32::from_rgb(160, 160, 160));
    }

    #[test]
    fn wb_lighter_inverts_the_role_in_the_dark_mode() {
        assert_eq!(wb_lighter(RED, 0.85, true), darker(RED, 0.85 * 0.95));
        assert_eq!(wb_darker(RED, 0.25, false), RED);
        assert_eq!(wb_darker(RED, 0.25, true), lighter(RED, 0.25));
    }

    #[test]
    fn row_colors_use_the_upstream_tables() {
        let window_text = Color32::from_rgb(10, 20, 30);
        let row = row_colors(
            ConflictAll::caConflict,
            ConflictThis::ctConflictWins,
            window_text,
            false,
        );
        assert_eq!(row.background, Some(lighter(RED, 0.85)));
        assert_eq!(row.text, ORANGE);
        // `clWindowText` resolves to the theme text.
        let row = row_colors(ConflictAll::caOnlyOne, ConflictThis::ctOnlyOne, window_text, false);
        assert_eq!(row.background, None);
        assert_eq!(row.text, window_text);
    }

    #[test]
    fn tcolor_round_trips() {
        let color = Color32::from_rgb(255, 128, 0);
        let value = ConflictColors::tcolor(color);
        assert_eq!(value, 0x000080FF);
        assert_eq!(ConflictColors::from_tcolor(value, Color32::BLACK), (color, false));
        // A system colour keeps the fallback.
        let (fallback, system) = ConflictColors::from_tcolor(0x2000_0007, Color32::WHITE);
        assert!(system);
        assert_eq!(fallback, Color32::WHITE);
    }

    #[test]
    fn every_conflict_value_has_a_colour() {
        assert_eq!(CONFLICT_ALL_COLORS.len(), ConflictAll::ALL.len());
        assert_eq!(CONFLICT_THIS_COLORS.len(), ConflictThis::ALL.len());
    }
}
