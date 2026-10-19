// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeModuleSelectForm.pas (edFilterChange,
// cbRegExFilterClick, GetSelectedOrAll, CheckStateForModule,
// acPresetLoadUpdate, acPresetSaveUpdate, acPresetDeleteUpdate,
// PresetSave's name check), xEdit/xeModGroupSelectForm.pas (the same
// handlers over mod groups), xEdit/xeFilterOptionsForm.pas
// (mniSelectionClick, clbKeyPress, GetRecordSignatures,
// SetRecordSignatures, FilterLoadPreset's reads), xEdit/xeFileSelectForm.pas
// (SelectAll1Click, SelectNone1Click, InvertSelection1Click,
// edSearchChange, TCheckListBox.DrawItem), xEdit/xeGameSelectForm.pas
// (edSearchChange, TListBox.DrawItem)

//! The parts the six selection dialogs share: the name filter with its
//! regular expression mode, the preset combo box and its action rules, the
//! check-list state and the selected-or-all rule of the bulk actions, the
//! case-insensitive search of the owner-drawn lists, the confirmation
//! questions, and the placement values the dialogs keep in xEdit's settings
//! file.
//!
//! The helpers are pure state machines: they are tested without an egui
//! context. Drawing lives in the dialog modules and in
//! [`show_question`].

use eframe::egui;
use xedit_core::perl_regex::{OPTION_CASE_LESS, PerlRegEx};

/// [`NameFilter::matches`]: the text is not a valid regular expression, so
/// the last expression that compiled keeps filtering (upstream leaves the
/// nodes' `IsFiltered` as they were and colours `edFilter` red).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterError {
    Compile,
}

/// `edFilter` with `cbRegExFilter` (`xeModuleSelectForm.edFilterChange`).
#[derive(Default)]
pub struct NameFilter {
    /// `edFilter.Text`.
    pub text: String,
    /// `cbRegExFilter.Checked`.
    pub regex: bool,
    /// The expression of the text did not compile: `edFilter.Color :=
    /// wbLighter(clRed, 0.85)`.
    pub invalid: bool,
    program: Program,
}

/// The compiled form of [`NameFilter::text`].
#[derive(Default)]
enum Program {
    /// An empty filter keeps every node.
    #[default]
    Empty,
    /// `not NodeData.mndName.ToLowerInvariant.Contains(SearchText)`.
    Contains(String),
    /// `TPerlRegEx` with `preCaseLess`.
    Regex(Box<PerlRegEx>),
}

impl NameFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `edFilterChange` and `cbRegExFilterClick`: the program is rebuilt
    /// from the text.
    pub fn recompile(&mut self) {
        self.invalid = false;
        if self.text.is_empty() {
            self.program = Program::Empty;
            return;
        }
        if !self.regex {
            self.program = Program::Contains(self.text.to_lowercase());
            return;
        }
        let mut compiled = PerlRegEx::new();
        compiled.set_options(1 << OPTION_CASE_LESS);
        compiled.set_regex(self.text.clone());
        match compiled.compile() {
            Ok(()) => self.program = Program::Regex(Box::new(compiled)),
            Err(_) => self.invalid = true,
        }
    }

    /// `edFilter.Text := value` followed by `edFilterChange`.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.recompile();
    }

    /// `cbRegExFilter.Checked := value` followed by `cbRegExFilterClick`.
    pub fn set_regex(&mut self, regex: bool) {
        self.regex = regex;
        self.recompile();
    }

    /// `SearchText <> ''`.
    pub fn is_active(&self) -> bool {
        !self.text.is_empty()
    }

    /// One node: `not FilterRegex.Match` (`true` keeps the node).
    pub fn matches(&mut self, name: &str) -> bool {
        match &mut self.program {
            Program::Empty => true,
            Program::Contains(needle) => name.to_lowercase().contains(&**needle),
            Program::Regex(regex) => {
                // `FilterRegex.Subject := NodeData.mndName` and `Match`.
                regex.set_subject(name.to_owned());
                matches!(regex.match_(), Ok(true))
            }
        }
    }

    /// The disabled reason of the regular expression check box: upstream
    /// offers it always; the port says why a red box is red.
    pub fn invalid_reason(&self) -> Option<&'static str> {
        self.invalid
            .then_some("the filter is not a valid regular expression (the last valid one still applies)")
    }
}

/// `TCheckState` of a tree node with a check box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    Checked,
    Mixed,
    Unchecked,
}

impl CheckState {
    /// The glyph the tree paints for the check box.
    pub fn glyph(self) -> char {
        match self {
            CheckState::Checked => '\u{2611}',
            CheckState::Mixed => '\u{25A3}',
            CheckState::Unchecked => '\u{2610}',
        }
    }
}

/// The selected-or-all rule of `GetSelectedOrAll` and `mniSelectionClick`:
/// two or more selected rows restrict the action to those rows, otherwise
/// it applies to every enabled row.
pub fn affected_rows(selected: &[usize], enabled: &[bool]) -> Vec<usize> {
    let scope: Vec<usize> = if selected.len() >= 2 {
        let mut scope = selected.to_vec();
        scope.sort_unstable();
        scope.dedup();
        scope
    } else {
        (0..enabled.len()).collect()
    };
    scope
        .into_iter()
        .filter(|&row| enabled.get(row).copied().unwrap_or(false))
        .collect()
}

/// One item of a check list or of the owner-drawn list of a dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckItem {
    pub text: String,
    pub checked: bool,
    /// `ItemEnabled[i]`: a disabled row is never changed by a bulk action.
    pub enabled: bool,
    /// The row's list selection (`Selected[i]`); the bulk actions of the
    /// check lists use it when two or more rows are selected.
    pub selected: bool,
}

impl CheckItem {
    pub fn new(text: impl Into<String>, checked: bool) -> Self {
        Self {
            text: text.into(),
            checked,
            enabled: true,
            selected: false,
        }
    }
}

/// `TCheckListBox` with `mniSelectAll`, `mniSelectNone` and
/// `mniInvertSelection` (`mniSelectionClick`, `SelectAll1Click`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Checklist {
    pub items: Vec<CheckItem>,
}

impl Checklist {
    pub fn new(items: impl IntoIterator<Item = CheckItem>) -> Self {
        Self {
            items: items.into_iter().collect(),
        }
    }

    /// The rows a bulk action changes: the selected rows when at least two
    /// are selected and enabled, every enabled row otherwise.
    fn scope(&self) -> Vec<usize> {
        let selected: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.selected)
            .map(|(index, _)| index)
            .collect();
        let enabled: Vec<bool> = self.items.iter().map(|item| item.enabled).collect();
        if selected.len() >= 2 {
            affected_rows(&selected, &enabled)
        } else {
            affected_rows(&[], &enabled)
        }
    }

    /// `mniSelectAllClick`.
    pub fn select_all(&mut self) {
        for index in self.scope() {
            self.items[index].checked = true;
        }
    }

    /// `mniSelectNoneClick`.
    pub fn select_none(&mut self) {
        for index in self.scope() {
            self.items[index].checked = false;
        }
    }

    /// `mniInvertSelectionClick`.
    pub fn invert_selection(&mut self) {
        for index in self.scope() {
            self.items[index].checked = !self.items[index].checked;
        }
    }

    /// The checked item texts, in list order.
    pub fn checked_texts(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.checked)
            .map(|item| item.text.clone())
            .collect()
    }

    /// `GetRecordSignatures`: the checked entries as the first four
    /// characters each, joined with commas in list order.
    pub fn signatures(&self) -> String {
        self.items
            .iter()
            .filter(|item| item.checked)
            .map(|item| item.text.chars().take(4).collect::<String>())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// `SetRecordSignatures`: the names of the comma text check the items
    /// whose first four characters they are (`TStringList.Find` without
    /// regard to case).
    pub fn set_signatures(&mut self, value: &str) {
        let mut names: Vec<String> = xedit_loadorder::ini_files::parse_comma_text(value)
            .into_iter()
            .map(|name| name.trim().to_ascii_uppercase())
            .collect();
        names.sort();
        for item in &mut self.items {
            let signature: String = item.text.chars().take(4).collect();
            item.checked = names.binary_search(&signature.to_ascii_uppercase()).is_ok();
        }
    }
}

/// `cbPreset` of the selection dialogs with the `acPreset*Update`
/// enablement rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresetCombo {
    /// `cbPreset.Items`, including the pseudo entries the dialog inserts
    /// first (`<plugins.txt>`, `<Last>`).
    pub items: Vec<String>,
    /// `cbPreset.Text`.
    pub text: String,
    /// The pseudo presets the dialog offers (`csPluginsTxt`, `csLast`).
    pub pseudo: Vec<String>,
}

impl PresetCombo {
    pub fn new(items: Vec<String>, pseudo: Vec<String>) -> Self {
        Self {
            items,
            text: String::new(),
            pseudo,
        }
    }

    /// The preset a save writes: `cbPreset.Text.Replace('=', '').Trim`
    /// with the name rules of `acPresetSaveUpdate`.
    pub fn clean_name(&self) -> Option<String> {
        let cleaned = self.text.replace('=', "");
        let cleaned = cleaned.trim();
        (!cleaned.is_empty() && !cleaned.starts_with('<') && cleaned == self.text).then(|| cleaned.to_owned())
    }

    /// `acPresetLoadUpdate`.
    pub fn can_load(&self) -> bool {
        self.pseudo.contains(&self.text) || self.items.contains(&self.text)
    }

    /// `acPresetDeleteUpdate`.
    pub fn can_delete(&self) -> bool {
        !self.text.starts_with('<') && self.items.contains(&self.text)
    }

    /// `acPresetSaveUpdate`: the name rules and a non-empty selection.
    pub fn can_save(&self, selected: usize) -> bool {
        self.clean_name().is_some() && selected > 0
    }

    /// The reason a load is not possible, for the tooltip of a disabled
    /// button.
    pub fn load_reason(&self) -> Option<&'static str> {
        (!self.can_load()).then_some("no preset of that name is selected")
    }

    /// The reason a save is not possible.
    pub fn save_reason(&self, selected: usize) -> Option<&'static str> {
        if selected == 0 {
            Some("the selection is empty")
        } else if self.text.is_empty() {
            Some("enter a preset name")
        } else if self.text.starts_with('<') {
            Some("the built-in entries can not be overwritten")
        } else if self.clean_name().is_none() {
            Some("a preset name has no '=', surrounding spaces or leading '<'")
        } else {
            None
        }
    }

    /// The reason a delete is not possible.
    pub fn delete_reason(&self) -> Option<&'static str> {
        if self.text.is_empty() {
            Some("no preset is selected")
        } else if self.text.starts_with('<') {
            Some("the built-in entries can not be deleted")
        } else if !self.can_delete() {
            Some("the preset does not exist")
        } else {
            None
        }
    }
}

/// `ContainsText(Items[Index], s)` over a list, for the owner-drawn lists of
/// `TfrmFileSelect` and `TfrmGameSelect`: the first entry that contains the
/// search text without regard to case, its index in the list.
pub fn first_match(items: &[String], search: &str) -> Option<usize> {
    let search = search.trim().to_lowercase();
    if search.is_empty() {
        return None;
    }
    items.iter().position(|item| item.to_lowercase().contains(&search))
}

/// Whether a row is highlighted by the owner-drawn search of
/// `TListBox.DrawItem`/`TCheckListBox.DrawItem`.
pub fn row_matches(text: &str, search: &str) -> bool {
    let search = search.trim();
    !search.is_empty() && text.to_lowercase().contains(&search.to_lowercase())
}

/// The size a dialog asks for, cut to the desktop's logical size so a small
/// or high-DPI screen still shows the whole window (the caller scrolls its
/// content).
pub fn fit_size(ctx: &egui::Context, desired: egui::Vec2) -> egui::Vec2 {
    let screen = ctx.input(|input| input.content_rect()).size();
    egui::vec2(
        desired.x.min((screen.x - 20.0).max(320.0)),
        desired.y.min((screen.y - 40.0).max(240.0)),
    )
}

/// Whether a character arrived this frame (a shortcut of a dialog, such as
/// the `*` of `VK_MULTIPLY` or the space that toggles a mod group item).
pub fn char_pressed(ctx: &egui::Context, wanted: char) -> bool {
    ctx.input(|input| {
        input.events.iter().any(|event| match event {
            egui::Event::Text(text) => text.starts_with(wanted) && text.chars().count() == 1,
            _ => false,
        })
    })
}

/// A `MessageDlg` question with its default button (`mbYes` or `mbNo`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub text: String,
    pub default_yes: bool,
}

impl Question {
    pub fn new(text: impl Into<String>, default_yes: bool) -> Self {
        Self {
            text: text.into(),
            default_yes,
        }
    }
}

/// Draws a confirmation question; `Some(true)` is the Yes button and
/// `Some(false)` the No button. `None` keeps the question open.
pub fn show_question(ctx: &egui::Context, id: &str, question: &Question) -> Option<bool> {
    let mut answer = None;
    egui::Window::new("xEdit")
        .id(egui::Id::unique(("question", id)))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.label(&question.text);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                // `mbYesNo` in the order of the default button.
                let buttons: [(&str, bool); 2] = if question.default_yes {
                    [("Yes", true), ("No", false)]
                } else {
                    [("No", false), ("Yes", true)]
                };
                for (caption, value) in buttons {
                    if ui.button(caption).clicked() {
                        answer = Some(value);
                    }
                }
            });
        });
    // `MessageDlg` answers Enter with the default button and Escape with
    // the other one.
    if answer.is_none() {
        if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
            answer = Some(question.default_yes);
        } else if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            answer = Some(!question.default_yes);
        }
    }
    answer
}

/// A dialog's placement: `[frmX] Left`, `Top`, `Width`, `Height`.
///
/// PORT-EXTENSION: the six selection forms of 4.1.5q keep no placement in
/// the settings file (unlike `frmLegend`, `frmViewElements` and
/// `frmRichEdit`); the coordinator's step-5 brief asks the port to persist
/// it, so the dialogs read and write the same four keys the forms that do
/// keep placement use.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DialogPlacement {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl DialogPlacement {
    /// Reads `Left`, `Top`, `Width` and `Height` from a `settings.get`
    /// response; a zero width or height is upstream's "no placement".
    pub fn from_values(values: &[(String, String)]) -> Option<Self> {
        let read = |key: &str| {
            values
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .and_then(|(_, value)| value.trim().parse::<f32>().ok())
        };
        let placement = Self {
            left: read("Left")?,
            top: read("Top")?,
            width: read("Width")?,
            height: read("Height")?,
        };
        (placement.width > 0.0 && placement.height > 0.0).then_some(placement)
    }

    /// The values `settings.set` writes, in the order `frmLegend` writes
    /// them.
    pub fn values(self) -> [(&'static str, i64); 5] {
        [
            ("Left", self.left.round() as i64),
            ("Top", self.top.round() as i64),
            ("Width", self.width.round() as i64),
            ("Height", self.height.round() as i64),
            ("WindowState", 0),
        ]
    }

    /// A rectangle of an egui window.
    pub fn from_rect(rect: egui::Rect) -> Self {
        Self {
            left: rect.left(),
            top: rect.top(),
            width: rect.width(),
            height: rect.height(),
        }
    }

    /// The position and size an `egui::Window` starts with, cut to the
    /// desktop so a placement written on a larger monitor still opens on
    /// screen.
    pub fn apply<'a>(self, ctx: &egui::Context, window: egui::Window<'a>) -> egui::Window<'a> {
        let screen = ctx.input(|input| input.content_rect());
        let width = self.width.min((screen.width() - 20.0).max(320.0));
        let height = self.height.min((screen.height() - 40.0).max(240.0));
        let left = self
            .left
            .clamp(screen.left(), (screen.right() - width).max(screen.left()));
        let top = self
            .top
            .clamp(screen.top(), (screen.bottom() - height).max(screen.top()));
        window
            .default_pos(egui::pos2(left, top))
            .default_size(egui::vec2(width, height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_substring_filter_ignores_case_and_leaves_empty_alone() {
        let mut filter = NameFilter::new();
        assert!(filter.matches("Fallout4.esm"));
        filter.set_text("fall");
        assert!(filter.matches("Fallout4.esm"));
        assert!(!filter.matches("Update.esm"));
        // The text is not trimmed upstream.
        filter.set_text(" fall");
        assert!(!filter.matches("Fallout4.esm"));
        filter.set_text("");
        assert!(filter.matches("Update.esm"));
    }

    #[test]
    fn the_regex_filter_matches_case_lessly_and_reports_a_bad_expression() {
        let mut filter = NameFilter::new();
        filter.set_regex(true);
        filter.set_text("^FO4.*\\.esm$");
        assert!(filter.matches("FO4Something.esm"));
        assert!(!filter.matches("Update.esm"));
        filter.set_text("(unclosed");
        assert!(filter.invalid);
        // The last valid expression keeps filtering, as upstream's nodes
        // keep their `IsFiltered` values.
        assert!(filter.matches("FO4Something.esm"));
        assert!(!filter.matches("Update.esm"));
        filter.set_text("update");
        assert!(!filter.invalid);
        assert!(filter.matches("Update.esm"));
    }

    #[test]
    fn two_selected_rows_restrict_a_bulk_action() {
        let enabled = [true, true, false, true];
        assert_eq!(affected_rows(&[], &enabled), vec![0, 1, 3]);
        assert_eq!(affected_rows(&[1], &enabled), vec![0, 1, 3]);
        assert_eq!(affected_rows(&[1, 3], &enabled), vec![1, 3]);
        assert_eq!(affected_rows(&[1, 2, 3], &enabled), vec![1, 3]);
        assert_eq!(affected_rows(&[2, 2], &enabled), Vec::<usize>::new());
    }

    #[test]
    fn the_checklist_bulk_actions_follow_the_selection() {
        let mut list = Checklist::new([
            CheckItem::new("a", false),
            CheckItem::new("b", false),
            CheckItem {
                text: "c".to_owned(),
                checked: false,
                enabled: false,
                selected: false,
            },
        ]);
        list.select_all();
        assert_eq!(
            list.items.iter().map(|item| item.checked).collect::<Vec<_>>(),
            vec![true, true, false]
        );
        list.invert_selection();
        assert_eq!(
            list.items.iter().map(|item| item.checked).collect::<Vec<_>>(),
            vec![false, false, false]
        );
        list.items[0].selected = true;
        list.items[1].selected = true;
        list.items[2].selected = true;
        list.select_all();
        assert_eq!(
            list.items.iter().map(|item| item.checked).collect::<Vec<_>>(),
            vec![true, true, false]
        );
        list.select_none();
        assert_eq!(
            list.items.iter().map(|item| item.checked).collect::<Vec<_>>(),
            vec![false, false, false]
        );
    }

    #[test]
    fn signatures_round_trip_as_the_first_four_characters() {
        let mut list = Checklist::new([
            CheckItem::new("WEAP - Weapon", true),
            CheckItem::new("ARMO - Armor", false),
            CheckItem::new("NPC_ - Non-Player Character", true),
        ]);
        assert_eq!(list.signatures(), "WEAP,NPC_");
        list.set_signatures("armo,npc_");
        assert_eq!(
            list.items.iter().map(|item| item.checked).collect::<Vec<_>>(),
            vec![false, true, true]
        );
        list.set_signatures("");
        assert!(list.items.iter().all(|item| !item.checked));
    }

    #[test]
    fn the_preset_rules_are_the_upstream_action_updates() {
        let mut combo = PresetCombo::new(
            vec!["<plugins.txt>".to_owned(), "All".to_owned()],
            vec!["<plugins.txt>".to_owned()],
        );
        combo.text = "<plugins.txt>".to_owned();
        assert!(combo.can_load());
        assert!(!combo.can_delete());
        assert!(!combo.can_save(1));
        combo.text = "All".to_owned();
        assert!(combo.can_load());
        assert!(combo.can_delete());
        assert!(combo.can_save(3));
        assert!(!combo.can_save(0));
        combo.text = "a=b".to_owned();
        assert!(!combo.can_save(3));
        assert!(combo.save_reason(3).is_some());
        combo.text = "  name".to_owned();
        assert!(!combo.can_save(3));
        combo.text = "New".to_owned();
        assert!(combo.can_save(1));
        assert!(!combo.can_delete());
        assert!(combo.delete_reason().is_some());
        assert!(!combo.can_load());
        assert!(combo.load_reason().is_some());
    }

    #[test]
    fn the_owner_drawn_search_selects_the_first_containing_row() {
        let items = [
            "Fallout4.esm".to_owned(),
            "DLCRobot.esm".to_owned(),
            "Unofficial Fallout 4 Patch.esp".to_owned(),
        ];
        assert_eq!(first_match(&items, "fallout"), Some(0));
        assert_eq!(first_match(&items, "patch"), Some(2));
        assert_eq!(first_match(&items, "  "), None);
        assert_eq!(first_match(&items, "nothing"), None);
        assert!(row_matches("DLCRobot.esm", "robot"));
        assert!(!row_matches("DLCRobot.esm", " "));
    }

    #[test]
    fn placement_round_trips_through_its_values() {
        let placement = DialogPlacement {
            left: 10.4,
            top: 20.6,
            width: 800.0,
            height: 600.0,
        };
        let values: Vec<(String, String)> = placement
            .values()
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_string()))
            .collect();
        let read = DialogPlacement::from_values(&values).unwrap();
        assert_eq!(read.left, 10.0);
        assert_eq!(read.top, 21.0);
        assert_eq!(read.width, 800.0);
        assert_eq!(read.height, 600.0);
        assert_eq!(DialogPlacement::from_values(&[]), None);
        assert_eq!(
            DialogPlacement::from_values(&[("Width".to_owned(), "0".to_owned())]),
            None
        );
    }
}
