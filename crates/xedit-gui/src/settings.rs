// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (DoInit's `Name` section:
// pnlNavWidth, vstNavColumnWidth<i>, WindowState, Left, Top, Width, Height;
// FormShow's ColorConflictThis, ColorConflictAll, `View` and `Nav` keys;
// FormCreate's `UI.Theme`; SaveFont and LoadFont of xeInit.pas/wbHelpers.pas
// for `UI.FontRecords` and `UI.FontMessages`)

//! The GUI keys of xEdit's settings file (`xeSettingsFileName`):
//! `TfrmMain.Name` is `frmMain`, so the window placement and the navigation
//! pane live under `[frmMain]`, the colours under `[ColorConflictThis]` and
//! `[ColorConflictAll]`, the theme and fonts under `[UI]`, and the view and
//! navigation options under `[View]` and `[Nav]`.
//!
//! The keys are read and written through the port's `TMemIniFile`
//! (`xedit_loadorder::ini_files`), the same machinery the mod group and
//! module preset commands use, so a file the Delphi build wrote is read
//! unchanged and a file the port writes is one the Delphi build reads.

use std::path::{Path, PathBuf};

use eframe::egui::Color32;
use xedit_core::interface::types::{ConflictAll, ConflictThis, PascalEnum};
use xedit_loadorder::ini_files::MemIniFile;

use crate::theme::{FontSpec, ThemeChoice};
use crate::widgets::conflict::{CONFLICT_ALL_COLORS, CONFLICT_THIS_COLORS, ConflictColors};

/// `TfrmMain.Name`, the section of the window keys.
pub const FORM_SECTION: &str = "frmMain";

/// `TWindowState` of the form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WindowState {
    #[default]
    Normal,
    Minimized,
    Maximized,
}

impl WindowState {
    fn from_setting(value: i64) -> Self {
        match value {
            1 => WindowState::Minimized,
            2 => WindowState::Maximized,
            _ => WindowState::Normal,
        }
    }

    fn as_setting(self) -> i64 {
        match self {
            WindowState::Normal => 0,
            WindowState::Minimized => 1,
            WindowState::Maximized => 2,
        }
    }
}

/// `Left`, `Top`, `Width`, `Height` and `WindowState` of the main form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowPlacement {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub state: WindowState,
}

impl Default for WindowPlacement {
    /// The form size of `xeMainForm.dfm` (ClientWidth 1370, ClientHeight
    /// 663 with the borders of a resizable window).
    fn default() -> Self {
        Self {
            left: 0,
            top: 0,
            width: 1370,
            height: 663,
            state: WindowState::Normal,
        }
    }
}

/// `mniViewColumnWidth*` of `pmuView`: the mode the view's columns are
/// sized by (`Settings.ReadInteger('View', 'ColumnWidth', 0)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColumnWidthMode {
    /// `mniViewColumnWidthStandard`.
    #[default]
    Standard,
    /// `mniViewColumnWidthFitAll`.
    FitAll,
    /// `mniViewColumnWidthFitText`.
    FitText,
    /// `mniViewColumnWidthFitSmart`.
    FitSmart,
}

impl ColumnWidthMode {
    fn from_setting(value: i64) -> Self {
        match value {
            1 => ColumnWidthMode::FitAll,
            2 => ColumnWidthMode::FitText,
            3 => ColumnWidthMode::FitSmart,
            _ => ColumnWidthMode::Standard,
        }
    }

    pub fn as_setting(self) -> i64 {
        match self {
            ColumnWidthMode::Standard => 0,
            ColumnWidthMode::FitAll => 1,
            ColumnWidthMode::FitText => 2,
            ColumnWidthMode::FitSmart => 3,
        }
    }
}

/// `mniNavHeaderFiles*`: the order the navigation tree lists the files in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FilesSort {
    /// `mniNavHeaderFilesDefault`: "as selected".
    #[default]
    AsSelected,
    /// `mniNavHeaderFilesLoadOrder`.
    LoadOrder,
    /// `mniNavHeaderFilesFileName`.
    FileName,
}

impl FilesSort {
    fn from_setting(value: i64) -> Self {
        match value {
            1 => FilesSort::LoadOrder,
            2 => FilesSort::FileName,
            _ => FilesSort::AsSelected,
        }
    }

    pub fn as_setting(self) -> i64 {
        match self {
            FilesSort::AsSelected => 0,
            FilesSort::LoadOrder => 1,
            FilesSort::FileName => 2,
        }
    }
}

/// `mniNavHeaderINFO*`: the order the dialogue topics are listed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InfoSort {
    /// `mniNavHeaderINFObyPreviousINFO`.
    #[default]
    ByPreviousInfo,
    /// `mniNavHeaderINFObyFormID`.
    ByFormId,
}

impl InfoSort {
    fn from_setting(value: i64) -> Self {
        match value {
            1 => InfoSort::ByFormId,
            _ => InfoSort::ByPreviousInfo,
        }
    }

    pub fn as_setting(self) -> i64 {
        match self {
            InfoSort::ByPreviousInfo => 0,
            InfoSort::ByFormId => 1,
        }
    }
}

/// The `[View]` and `[Nav]` keys `FormShow` reads.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewSettings {
    /// `View.HodeNoConflict` (upstream's spelling): hide the rows without a
    /// conflict (`mniViewHideNoConflict`).
    pub hide_no_conflict: bool,
    /// `View.StickViewNodeLabel`: the view row that stays at the top
    /// (`mniViewStick*`).
    pub stick_view_node_label: String,
    /// `View.ColumnWidth`.
    pub column_width: ColumnWidthMode,
    /// `Nav.FilesSort`.
    pub files_sort: FilesSort,
    /// `Nav.INFOSort`.
    pub info_sort: InfoSort,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            hide_no_conflict: false,
            stick_view_node_label: String::new(),
            column_width: ColumnWidthMode::Standard,
            files_sort: FilesSort::AsSelected,
            info_sort: InfoSort::ByPreviousInfo,
        }
    }
}

/// The GUI settings, read from the file when the game is chosen (the file
/// name depends on the game and the plugin list).
#[derive(Clone, Debug, PartialEq)]
pub struct GuiSettings {
    /// The file the settings were read from; `None` until a game is chosen.
    pub path: Option<PathBuf>,
    pub window: WindowPlacement,
    /// `frmMain.pnlNavWidth`: the width of the navigation pane (the
    /// splitter position).
    pub nav_width: f32,
    /// `frmMain.vstNavColumnWidth<i>`: the columns of the navigation tree.
    pub nav_column_widths: Vec<f32>,
    pub theme: ThemeChoice,
    pub fonts: crate::theme::Fonts,
    pub colors: ConflictColors,
    pub view: ViewSettings,
}

/// The navigation tree's columns in `xeMainForm.dfm`: FormID 201,
/// EditorID 125, Name 125.
pub const NAV_COLUMN_WIDTHS: [f32; 3] = [201.0, 125.0, 125.0];

impl Default for GuiSettings {
    fn default() -> Self {
        Self {
            path: None,
            window: WindowPlacement::default(),
            nav_width: 455.0,
            nav_column_widths: NAV_COLUMN_WIDTHS.to_vec(),
            theme: ThemeChoice::System,
            fonts: crate::theme::Fonts::default(),
            colors: ConflictColors::default(),
            view: ViewSettings::default(),
        }
    }
}

impl GuiSettings {
    /// `TMemIniFile.Create(xeSettingsFileName)` and the reads of `DoInit`
    /// and `FormShow`; a missing file is the defaults.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, std::io::Error> {
        let path = path.into();
        let ini = MemIniFile::open(&path)?;
        let mut settings = Self::from_ini(&ini);
        settings.path = Some(path);
        Ok(settings)
    }

    /// The reads without a file: the defaults `FormShow` uses.
    pub fn from_ini(ini: &MemIniFile) -> Self {
        let mut settings = Self::default();
        settings.window = WindowPlacement {
            left: read_i32(ini, FORM_SECTION, "Left", settings.window.left),
            top: read_i32(ini, FORM_SECTION, "Top", settings.window.top),
            width: read_i32(ini, FORM_SECTION, "Width", settings.window.width),
            height: read_i32(ini, FORM_SECTION, "Height", settings.window.height),
            state: WindowState::from_setting(read_i64(ini, FORM_SECTION, "WindowState", 0)),
        };
        settings.nav_width = read_i64(ini, FORM_SECTION, "pnlNavWidth", 455) as f32;
        if settings.nav_width < 50.0 {
            settings.nav_width = 50.0;
        }
        for (index, width) in settings.nav_column_widths.iter_mut().enumerate() {
            *width = read_i64(ini, FORM_SECTION, &format!("vstNavColumnWidth{index}"), *width as i64) as f32;
        }
        settings.theme = ThemeChoice::from_setting(&ini.read_string("UI", "Theme", ""));
        settings.fonts.records = read_font(ini, "FontRecords", FontSpec::default_records());
        settings.fonts.messages = read_font(ini, "FontMessages", FontSpec::default_messages());
        for conflict_all in ConflictAll::ALL {
            let name = conflict_all.enum_name();
            let fallback = CONFLICT_ALL_COLORS[conflict_all.ord()];
            let default = fallback.map_or(-1, ConflictColors::tcolor);
            let value = read_i32(ini, "ColorConflictAll", name, default);
            settings.colors.all[conflict_all.ord()] = if value < 0 {
                fallback
            } else {
                Some(ConflictColors::from_tcolor(value, fallback.unwrap_or(Color32::PLACEHOLDER)).0)
            };
        }
        for conflict_this in ConflictThis::ALL {
            let name = conflict_this.enum_name();
            let fallback = CONFLICT_THIS_COLORS[conflict_this.ord()];
            let value = read_i32(ini, "ColorConflictThis", name, ConflictColors::tcolor(fallback));
            let (color, system) = ConflictColors::from_tcolor(value, fallback);
            settings.colors.this[conflict_this.ord()] = color;
            settings.colors.this_system[conflict_this.ord()] = system;
        }
        settings.view = ViewSettings {
            hide_no_conflict: ini.read_bool("View", "HodeNoConflict", false),
            stick_view_node_label: ini.read_string("View", "StickViewNodeLabel", ""),
            column_width: ColumnWidthMode::from_setting(read_i64(ini, "View", "ColumnWidth", 0)),
            files_sort: FilesSort::from_setting(read_i64(ini, "Nav", "FilesSort", 0)),
            info_sort: InfoSort::from_setting(read_i64(ini, "Nav", "INFOSort", 0)),
        };
        settings
    }

    /// Writes the keys `FormClose` and `SaveFont` write, and leaves every
    /// other line of the file as it is.
    pub fn save(&self) -> Result<(), std::io::Error> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut ini = MemIniFile::open(path)?;
        self.write_to(&mut ini);
        ini.update_file()
    }

    /// The writes into an open ini (testable without a file).
    pub fn write_to(&self, ini: &mut MemIniFile) {
        write_i64(ini, FORM_SECTION, "pnlNavWidth", self.nav_width.round() as i64);
        for (index, width) in self.nav_column_widths.iter().enumerate() {
            write_i64(
                ini,
                FORM_SECTION,
                &format!("vstNavColumnWidth{index}"),
                width.round() as i64,
            );
        }
        write_i64(ini, FORM_SECTION, "WindowState", self.window.state.as_setting());
        write_i64(ini, FORM_SECTION, "Left", self.window.left as i64);
        write_i64(ini, FORM_SECTION, "Top", self.window.top as i64);
        write_i64(ini, FORM_SECTION, "Width", self.window.width as i64);
        write_i64(ini, FORM_SECTION, "Height", self.window.height as i64);
        ini.write_string("UI", "Theme", &self.theme.as_setting());
        write_font(ini, "FontRecords", &self.fonts.records);
        write_font(ini, "FontMessages", &self.fonts.messages);
        for conflict_all in ConflictAll::ALL {
            if let Some(color) = self.colors.all[conflict_all.ord()] {
                write_i64(
                    ini,
                    "ColorConflictAll",
                    conflict_all.enum_name(),
                    ConflictColors::tcolor(color) as i64,
                );
            }
        }
        for conflict_this in ConflictThis::ALL {
            if !self.colors.this_system[conflict_this.ord()] {
                write_i64(
                    ini,
                    "ColorConflictThis",
                    conflict_this.enum_name(),
                    ConflictColors::tcolor(self.colors.this[conflict_this.ord()]) as i64,
                );
            }
        }
        ini.write_string(
            "View",
            "HodeNoConflict",
            if self.view.hide_no_conflict { "1" } else { "0" },
        );
        ini.write_string("View", "StickViewNodeLabel", &self.view.stick_view_node_label);
        write_i64(ini, "View", "ColumnWidth", self.view.column_width.as_setting());
        write_i64(ini, "Nav", "FilesSort", self.view.files_sort.as_setting());
        write_i64(ini, "Nav", "INFOSort", self.view.info_sort.as_setting());
    }

    /// The settings file path of a game, as `DoInitPath` computes
    /// `xeSettingsFileName`: `<program path><AppName>Edit.ini` when it
    /// exists, else `Plugins.<app>viewsettings` next to the game's
    /// `Plugins.txt`.
    pub fn default_path(game: xedit_core::interface::globals::GameMode, program_path: &Path) -> PathBuf {
        let data_path = xedit_session::game_paths::discover_game_folder(game);
        let data_path = xedit_session::game_paths::data_path_of(&data_path, game);
        let plugins = xedit_session::game_paths::default_plugins_file(game, &data_path);
        let program = program_path
            .parent()
            .map(|folder| folder.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = xedit_session::game_paths::settings_file_name(
            xedit_session::game_paths::app_name(game),
            "Edit",
            &program,
            &plugins,
        );
        PathBuf::from(name)
    }
}

/// `LoadFont` of one `TFont`.
fn read_font(ini: &MemIniFile, name: &str, default: FontSpec) -> FontSpec {
    FontSpec {
        name: ini.read_string("UI", &format!("{name}Name"), &default.name),
        char_set: read_i32(ini, "UI", &format!("{name}CharSet"), default.char_set),
        color: read_i32(ini, "UI", &format!("{name}Color"), default.color),
        size: read_i32(ini, "UI", &format!("{name}Size"), default.size),
        style: read_i32(ini, "UI", &format!("{name}Style"), i32::from(default.style)) as u8,
    }
}

/// `SaveFont` of one `TFont`.
fn write_font(ini: &mut MemIniFile, name: &str, font: &FontSpec) {
    ini.write_string("UI", &format!("{name}Name"), &font.name);
    write_i64(ini, "UI", &format!("{name}CharSet"), i64::from(font.char_set));
    write_i64(ini, "UI", &format!("{name}Color"), i64::from(font.color));
    write_i64(ini, "UI", &format!("{name}Size"), i64::from(font.size));
    write_i64(ini, "UI", &format!("{name}Style"), i64::from(font.style));
}

/// `ReadInteger`, which answers the default for a value that is not a
/// number.
fn read_i64(ini: &MemIniFile, section: &str, ident: &str, default: i64) -> i64 {
    ini.read_string(section, ident, "").trim().parse().unwrap_or(default)
}

fn read_i32(ini: &MemIniFile, section: &str, ident: &str, default: i32) -> i32 {
    ini.read_string(section, ident, "").trim().parse().unwrap_or(default)
}

fn write_i64(ini: &mut MemIniFile, section: &str, ident: &str, value: i64) {
    ini.write_string(section, ident, &value.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ini_with(lines: &[&str]) -> MemIniFile {
        let mut ini = MemIniFile::open(Path::new("this-file-does-not-exist.ini")).unwrap();
        ini.set_strings(&lines.iter().map(|line| (*line).to_owned()).collect::<Vec<_>>());
        ini
    }

    #[test]
    fn missing_keys_keep_the_form_defaults() {
        let settings = GuiSettings::from_ini(&ini_with(&[]));
        assert_eq!(settings.window.width, 1370);
        assert_eq!(settings.nav_width, 455.0);
        assert_eq!(settings.nav_column_widths, vec![201.0, 125.0, 125.0]);
        assert_eq!(settings.theme, ThemeChoice::System);
        assert!(!settings.view.hide_no_conflict);
        assert_eq!(settings.view.files_sort, FilesSort::AsSelected);
    }

    #[test]
    fn the_upstream_keys_are_read() {
        let ini = ini_with(&[
            "[frmMain]",
            "Left=10",
            "Top=20",
            "Width=1280",
            "Height=720",
            "WindowState=2",
            "pnlNavWidth=300",
            "vstNavColumnWidth0=111",
            "[UI]",
            "Theme=Dark",
            "FontRecordsName=Consolas",
            "FontRecordsSize=10",
            "FontRecordsStyle=1",
            "[View]",
            "HodeNoConflict=1",
            "ColumnWidth=3",
            "[Nav]",
            "FilesSort=2",
            "INFOSort=1",
            "[ColorConflictThis]",
            "ctConflictWins=255",
        ]);
        let settings = GuiSettings::from_ini(&ini);
        assert_eq!(settings.window.left, 10);
        assert_eq!(settings.window.state, WindowState::Maximized);
        assert_eq!(settings.nav_width, 300.0);
        assert_eq!(settings.nav_column_widths[0], 111.0);
        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(settings.fonts.records.name, "Consolas");
        assert_eq!(settings.fonts.records.size, 10);
        assert!(settings.fonts.records.bold());
        assert!(settings.view.hide_no_conflict);
        assert_eq!(settings.view.column_width, ColumnWidthMode::FitSmart);
        assert_eq!(settings.view.files_sort, FilesSort::FileName);
        assert_eq!(settings.view.info_sort, InfoSort::ByFormId);
        // TColor 255 is RGB(255, 0, 0).
        assert_eq!(
            settings.colors.this[ConflictThis::ctConflictWins.ord()],
            Color32::from_rgb(255, 0, 0)
        );
        assert!(!settings.colors.this_system[ConflictThis::ctConflictWins.ord()]);
    }

    #[test]
    fn settings_round_trip_through_an_ini_file() {
        let dir = std::env::temp_dir().join(format!("xedit-gui-settings-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("Plugins.testviewsettings");
        let _ = std::fs::remove_file(&path);
        let mut settings = GuiSettings {
            path: Some(path.clone()),
            ..GuiSettings::default()
        };
        settings.window.left = 12;
        settings.window.state = WindowState::Maximized;
        settings.nav_width = 321.0;
        settings.theme = ThemeChoice::Named("Carbon".to_owned());
        settings.view.files_sort = FilesSort::LoadOrder;
        settings.save().unwrap();
        let read = GuiSettings::load(&path).unwrap();
        assert_eq!(read.window, settings.window);
        assert_eq!(read.nav_width, 321.0);
        assert_eq!(read.theme, ThemeChoice::Named("Carbon".to_owned()));
        assert_eq!(read.view.files_sort, FilesSort::LoadOrder);
        // A second save keeps the values and does not duplicate sections.
        read.save().unwrap();
        let again = GuiSettings::load(&path).unwrap();
        assert_eq!(again.window, read.window);
        assert_eq!(again.theme, read.theme);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("[frmMain]").count(), 1);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn a_small_splitter_is_lifted_to_the_minimum() {
        let ini = ini_with(&["[frmMain]", "pnlNavWidth=10"]);
        assert_eq!(GuiSettings::from_ini(&ini).nav_width, 50.0);
    }

    #[test]
    fn other_lines_survive_a_save() {
        let dir = std::env::temp_dir().join(format!("xedit-gui-settings-keep-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.ini");
        std::fs::write(&path, "[ModGroups]\nSelection=Test\n[Other]\nKey=Value\n").unwrap();
        let mut settings = GuiSettings::load(&path).unwrap();
        settings.nav_width = 200.0;
        settings.save().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("Selection=Test"));
        assert!(text.contains("Key=Value"));
        assert!(text.contains("pnlNavWidth=200"));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
