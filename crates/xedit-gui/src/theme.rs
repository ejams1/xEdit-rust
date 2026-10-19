// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeInit.pas (xeInitStyles, xeApplyFontAndScale,
// xeLoadAndApplyFontAndScale), Core/wbHelpers.pas (LoadFont, SaveFont),
// xEdit/xeMainForm.pas (FormCreate: TStyleManager.TrySetStyle of
// `UI.Theme`, the fonts `UI.FontRecords` and `UI.FontMessages`)

//! The theme and the fonts of the GUI: the `[UI]` section of the settings
//! file, which upstream reads into the VCL style and the fonts of the
//! records tree and the messages memo.
//!
//! The VCL styles xEdit offers (`Themes\*.vsf`) have no egui equivalent, so
//! the port maps the three theme names it understands (`System`, `Light`,
//! `Dark`) and keeps any other value in the settings file for a later run.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;

/// The `UI.Theme` value: the port's themes and the raw VCL style name.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
    /// A VCL style name the port has no equivalent for; it is written back
    /// unchanged so the Delphi build still finds it.
    Named(String),
}

impl ThemeChoice {
    /// Reads the `UI.Theme` value. The default is the active VCL style,
    /// which the port takes as the system theme.
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "system" => ThemeChoice::System,
            "light" => ThemeChoice::Light,
            "dark" => ThemeChoice::Dark,
            _ => ThemeChoice::Named(value.to_owned()),
        }
    }

    /// The value to write back: a named style is preserved as it was read.
    pub fn as_setting(&self) -> String {
        match self {
            ThemeChoice::System => "System".to_owned(),
            ThemeChoice::Light => "Light".to_owned(),
            ThemeChoice::Dark => "Dark".to_owned(),
            ThemeChoice::Named(name) => name.clone(),
        }
    }

    pub fn preference(&self) -> egui::ThemePreference {
        match self {
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
            ThemeChoice::System | ThemeChoice::Named(_) => egui::ThemePreference::System,
        }
    }

    /// Sets the theme on the context; named styles follow the system.
    pub fn apply(&self, ctx: &egui::Context) {
        ctx.set_theme(self.preference());
    }
}

/// One of the `[UI] FontRecords`, `FontMessages` and `FontViewer` entries:
/// `LoadFont` stores a `TFont` as five keys, `<name>Name`, `<name>CharSet`,
/// `<name>Color`, `<name>Size` and `<name>Style`.
#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    pub name: String,
    /// `TFont.CharSet`; kept for the round trip, not acted on.
    pub char_set: i32,
    /// `TFont.Color`, `clWindowText` by default.
    pub color: Color32Setting,
    /// `TFont.Size` in points.
    pub size: i32,
    /// `TFontStyles` as the byte `SaveFont` writes: bit 0 bold, 1 italic,
    /// 2 underline, 3 strikeout.
    pub style: u8,
}

/// A colour stored as a Delphi `TColor`.
pub type Color32Setting = i32;

impl FontSpec {
    /// The form's font in `xeMainForm.dfm`: Tahoma 8pt, `clWindowText`.
    pub fn default_records() -> Self {
        Self {
            name: "Tahoma".to_owned(),
            char_set: 1,        // DEFAULT_CHARSET
            color: 0x2000_0007, // clWindowText (a system colour)
            size: 8,
            style: 0,
        }
    }

    /// `vstView` and `vstNav` use the same font as the form.
    pub fn default_messages() -> Self {
        Self::default_records()
    }

    /// `TFont.Size` in points to the pixels egui measures in (96 DPI).
    pub fn size_px(&self) -> f32 {
        (self.size as f32) * 4.0 / 3.0
    }

    pub fn bold(&self) -> bool {
        self.style & 1 != 0
    }

    pub fn italic(&self) -> bool {
        self.style & 2 != 0
    }

    /// The font file of a font name on this machine, if there is one.
    /// Windows keeps the system fonts in `%WINDIR%\Fonts`; the names are
    /// the file names of the usual Delphi fonts.
    pub fn font_file(name: &str) -> Option<PathBuf> {
        let file_name = match name.to_ascii_lowercase().as_str() {
            "tahoma" => "tahoma.ttf",
            "segoe ui" => "segoeui.ttf",
            "consolas" => "consola.ttf",
            "courier new" => "cour.ttf",
            "arial" => "arial.ttf",
            "verdana" => "verdana.ttf",
            "calibri" => "calibri.ttf",
            "ms sans serif" | "microsoft sans serif" => "micross.ttf",
            other => return font_file_named(other),
        };
        font_file_named(file_name)
    }
}

/// A font file in the Windows font folder, by its file name.
fn font_file_named(file_name: &str) -> Option<PathBuf> {
    let windir = std::env::var_os("WINDIR")?;
    let path = Path::new(&windir).join("Fonts").join(file_name);
    path.is_file().then_some(path)
}

/// What the two egui font families are built from: the records font and the
/// messages font, each with the family that reads best when its file is
/// missing.
#[derive(Clone, Debug, PartialEq)]
pub struct Fonts {
    pub records: FontSpec,
    pub messages: FontSpec,
}

impl Default for Fonts {
    fn default() -> Self {
        Self {
            records: FontSpec::default_records(),
            messages: FontSpec::default_messages(),
        }
    }
}

/// Applies the fonts to the context. The records font becomes the
/// proportional family's first entry; the messages font is used by the
/// messages page through [`messages_font_id`]. A font whose file is not
/// found keeps egui's default family.
pub fn apply_fonts(ctx: &egui::Context, fonts: &Fonts) {
    let mut definitions = egui::FontDefinitions::default();
    let mut loaded = false;
    if let Some(path) = FontSpec::font_file(&fonts.records.name)
        && let Ok(bytes) = std::fs::read(&path)
    {
        definitions
            .font_data
            .insert("xedit-records".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        definitions
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "xedit-records".to_owned());
        loaded = true;
    }
    if let Some(path) = FontSpec::font_file(&fonts.messages.name)
        && let Ok(bytes) = std::fs::read(&path)
    {
        definitions
            .font_data
            .insert("xedit-messages".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        definitions
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "xedit-messages".to_owned());
    }
    if loaded {
        ctx.set_fonts(definitions);
    }
    let mut style = (*ctx.global_style()).clone();
    let size = fonts.records.size_px();
    style.text_styles = [
        (egui::TextStyle::Body, egui::FontId::proportional(size)),
        (egui::TextStyle::Button, egui::FontId::proportional(size)),
        (
            egui::TextStyle::Small,
            egui::FontId::proportional((size - 1.0).max(6.0)),
        ),
        (egui::TextStyle::Heading, egui::FontId::proportional(size + 3.0)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(size)),
    ]
    .into();
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style_of(egui::Theme::Dark, style);
}

/// The `FontId` of the messages page: the messages font in the monospace
/// family, which [`apply_fonts`] puts its file into.
pub fn messages_font_id(fonts: &Fonts) -> egui::FontId {
    egui::FontId::new(fonts.messages.size_px(), egui::FontFamily::Monospace)
}

/// The `Color32` of a `TFont.Color`, with `clWindowText` resolved to the
/// theme's text colour.
pub fn font_color(spec: &FontSpec, visuals: &egui::Visuals) -> egui::Color32 {
    let value = spec.color;
    if value < 0 || (value as u32) & 0xFF00_0000 != 0 {
        visuals.text_color()
    } else {
        egui::Color32::from_rgb(
            (value & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            ((value >> 16) & 0xFF) as u8,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_names_round_trip() {
        assert_eq!(ThemeChoice::from_setting(""), ThemeChoice::System);
        assert_eq!(ThemeChoice::from_setting("Dark"), ThemeChoice::Dark);
        assert_eq!(ThemeChoice::from_setting("light"), ThemeChoice::Light);
        let named = ThemeChoice::from_setting("Carbon");
        assert_eq!(named, ThemeChoice::Named("Carbon".to_owned()));
        assert_eq!(named.as_setting(), "Carbon");
        assert_eq!(ThemeChoice::Dark.as_setting(), "Dark");
    }

    #[test]
    fn font_sizes_convert_points_to_pixels() {
        assert_eq!(FontSpec::default_records().size_px(), 32.0 / 3.0);
    }

    #[test]
    fn font_styles_read_the_delphi_bits() {
        let spec = FontSpec {
            style: 3,
            ..FontSpec::default_records()
        };
        assert!(spec.bold());
        assert!(spec.italic());
        let spec = FontSpec {
            style: 0,
            ..FontSpec::default_records()
        };
        assert!(!spec.bold() && !spec.italic());
    }
}
