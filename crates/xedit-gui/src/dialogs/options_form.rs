// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeOptionsForm.pas (FormCreate, FormClose,
// FormKeyDown, cbConflictAllChange, cbConflictThisChange,
// clbConflictAllChange, clbConflictThisChange, rbThemeClick,
// cbThemeSystemSelect, UpdateThemePreview, GetSelectedTheme,
// pnlFontRecordsClick) and xEdit/xeOptionsForm.dfm (pcOptions with
// tsGeneral, tsViewSettings, tsCleaning, tsUISettings, tbsUITheme and
// tsExperts, btnOK, btnCancel) and xEdit/xeMainForm.pas (mniNavOptionsClick:
// the values read from the globals and the `[Options]` and `[UI]` keys, and
// the writes back)

//! `TfrmOptions`: every option of the editor on six tabs.
//!
//! The dialog holds no globals of its own. The shell reads the values
//! (through `settings.get`) and the colours, builds the state, and on OK
//! writes every key back (through `settings.set`), applies the globals with
//! [`OptionsState::apply_globals`] and hands the theme, the fonts and the
//! colours to the main form, as `mniNavOptionsClick` does.
//!
//! The port cannot render the VCL styles, so the UI Theme tab offers its
//! three themes (`System`, `Light`, `Dark`) split the way upstream splits
//! the style names; a named style the settings file carries is shown in the
//! dark group and written back unchanged. The font panels show the font the
//! settings file holds; the font chooser dialog is not offered.

use eframe::egui;
use xedit_core::interface::globals;
use xedit_core::interface::misc::str_to_int_def;
use xedit_core::interface::types::{ConflictAll, ConflictThis, PascalEnum};

use crate::theme::{FontSpec, Fonts, ThemeChoice};
use crate::widgets::conflict::ConflictColors;

/// The tabs of `pcOptions`, in DFM order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OptionsTab {
    #[default]
    General,
    ViewSettings,
    Cleaning,
    UISettings,
    UITheme,
    Experts,
}

impl OptionsTab {
    pub const ALL: [OptionsTab; 6] = [
        OptionsTab::General,
        OptionsTab::ViewSettings,
        OptionsTab::Cleaning,
        OptionsTab::UISettings,
        OptionsTab::UITheme,
        OptionsTab::Experts,
    ];

    pub fn caption(self) -> &'static str {
        match self {
            OptionsTab::General => "General",
            OptionsTab::ViewSettings => "View",
            OptionsTab::Cleaning => "Cleaning",
            OptionsTab::UISettings => "UI Settings",
            OptionsTab::UITheme => "UI Theme",
            OptionsTab::Experts => "Experts",
        }
    }
}

/// What the dialog asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsAction {
    /// `btnOK`: write the keys, apply the globals and the theme.
    Ok,
    /// `btnCancel` (or `VK_ESCAPE`): restore the colours the dialog was
    /// opened with (`FormClose` with `ModalResult <> mrOk`).
    Cancelled,
}

/// A check box of the dialog: its settings key and its caption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyedBool {
    pub key: &'static str,
    pub caption: &'static str,
    pub value: bool,
}

/// The check boxes of the General tab (`tsGeneral`), in the DFM order.
pub const GENERAL_TOGGLES: [(&str, &str); 18] = [
    ("HideUnused", "Hide unused"),
    ("HideIgnored", "Hide ignored"),
    ("HideNeverShow", "Hide \"never shown\""),
    ("LoadBSAs", "Load BSAs (requires restart)"),
    (
        "SimpleRecords",
        "Simple records LAND, NAVI, NAVM, CELL, WRLD (requires restart)",
    ),
    ("ShowUnsavedHint", "Show Unsaved Hint"),
    ("TrackAllEditorID", "Track all EditorID"),
    ("ActorTemplateHide", "Hide templated fields on actors"),
    ("ShowGroupRecordCount", "Show elements count for group records"),
    ("ShowTip", "Show tip on start"),
    ("ShowFileFlags", "Show file header flags"),
    ("AlignArrayElements", "Align alignable array elements"),
    ("Patron", "I am a patron or donor"),
    ("NoGitHubCheck", "Don't check for GitHub update"),
    ("NoNexusModsCheck", "Don't check for NexusMods update"),
    ("ResetModifiedOnSave", "Reset Modified (Bold) on Save"),
    ("AlwaysSaveOnam", "Always save ONAM"),
    ("ManualCleaningHide", "Hide Manual Cleaning functions"),
];

/// The check boxes of the View tab (`tsViewSettings`): the fields and the
/// types to collapse by default.
pub const COLLAPSE_TOGGLES: [(&str, &str); 40] = [
    ("CollapseRecordHeader", "Record Header"),
    ("CollapseObjectBounds", "Object Bounds (except TES4)"),
    ("CollapseConditions", "Conditions"),
    ("CollapseFactionRelations", "Faction Relations"),
    (
        "CollapseModels",
        "Models / 1st Person Models / Biped Models / World Models",
    ),
    ("CollapseItems", "Items / Components"),
    ("CollapseScriptProperties", "Script Properties (TES5+)"),
    ("CollapseBenignArray", "Arrays with Benign Conflict Priority"),
    ("CollapseLeveledItems", "Leveled List Entries (except FO76)"),
    ("CollapseObjectProperties", "Actor Value Properties (FO4 and FO76)"),
    ("CollapseEquipSlots", "Race Equip Slots (FO4 and FO76)"),
    ("CollapseFactions", "Factions"),
    ("CollapseRGBA", "Colors (RGB/A)"),
    ("CollapseVec3", "Vector3 (XYZ)"),
    ("CollapsePosRot", "PosRot Vec (XYZ,XYZ)"),
    ("CollapseNavmesh", "Navmesh"),
    ("CollapseKeywords", "Keywords"),
    ("CollapseAliases", "Aliases"),
    ("CollapseScriptData", "Script Data (TES3, TES4)"),
    ("CollapseFlags", "Flags"),
    ("CollapseSounds", "Sounds"),
    ("CollapseFragments", "Script Fragments (TES5+)"),
    ("CollapsePlacement", "Placement"),
    ("CollapseVertices", "Vertices"),
    ("CollapsePerk", "Perk"),
    ("CollapseOther", "Other"),
    ("CollapseRange", "Range"),
    ("CollapseARMABoneData", "ARMA Bone"),
    ("CollapseRACEBoneData", "RACE Bone"),
    ("CollapseLocations", "Locations"),
    ("CollapseTransforms", "Transforms"),
    ("CollapseHeadParts", "HeadParts"),
    ("CollapseBodyParts", "BodyParts"),
    ("CollapseDestruction", "Destruction"),
    ("CollapseOwnership", "Ownership"),
    ("CollapseRagdoll", "Ragdoll"),
    ("CollapseFactionRanks", "Faction Ranks"),
    ("CollapseBaseFormComponent", "BaseForm Component"),
    ("CollapseModelInfo", "Model Info"),
    ("CollapseDirectionRotation", "Direction Rotation"),
];

/// The check boxes of the Experts tab (`tsExperts`), which upstream shows
/// only with `wbIKnowWhatImDoing`.
pub const EXPERT_TOGGLES: [(&str, &str); 8] = [
    ("SortFLST2", "[FO3/FNV] Sort FLST"),
    ("SortINFO", "Sort INFOs (requires restart)"),
    ("FillPNAM", "Fill missing PNAM (requires restart)"),
    (
        "ShowFlagEnumValue",
        "Show values of flags and enumerations (requires restart, interferes with scripts)",
    ),
    ("ClampFormID", "Clamp FormIDs"),
    ("ManualCleaningAllow", "Allow Manual Cleaning functions"),
    ("DecodeTextureHashes2", "Decode Texture Hashes (requires restart)"),
    (
        "ConvertIntFormID",
        "Allow use of integer FormIDs (requires '0x' prefix on hex FormIDs)",
    ),
];

/// `TfrmOptions` as data.
#[derive(Clone, Debug, PartialEq)]
pub struct OptionsState {
    pub active_tab: OptionsTab,
    /// `tsExperts.TabVisible := wbIKnowWhatImDoing`.
    pub experts_visible: bool,
    pub general: Vec<KeyedBool>,
    pub collapse: Vec<KeyedBool>,
    pub experts: Vec<KeyedBool>,

    /// `cbUDRSetXESP`.
    pub udr_set_xesp: bool,
    /// `cbUDRSetScale` with `edUDRSetScaleValue`.
    pub udr_set_scale: bool,
    pub udr_set_scale_value: String,
    /// `cbUDRSetZ` with `edUDRSetZValue`.
    pub udr_set_z: bool,
    pub udr_set_z_value: String,
    /// `cbUDRSetMSTT` with `edUDRSetMSTTValue` (hex text).
    pub udr_set_mstt: bool,
    pub udr_set_mstt_value: String,

    /// `cbConflictThis` with `clbConflictThis`: the entry the colour box
    /// edits, and the colours (`wbColorConflictThis`).
    pub conflict_this_index: usize,
    pub conflict_all_index: usize,
    pub colors: ConflictColors,

    /// `pnlFontRecords`, `pnlFontMessages` and `pnlFontViewer`.
    pub font_records: FontSpec,
    pub font_messages: FontSpec,
    pub font_viewer: FontSpec,

    /// `edColumnWidth` and `edRowHeight` (`[Options] ColumnWidth`,
    /// `RowHeight`).
    pub column_width: String,
    pub row_height: String,
    /// `sedAutoCompareSelectedLimit`.
    pub auto_compare_selected_limit: String,
    /// `sedNavChangeDelay` (`tmrPendingSetActive.Interval`).
    pub nav_change_delay: String,
    pub require_ctrl_for_dbl_click: bool,
    pub focus_added_element: bool,
    pub shrink_buttons: bool,

    /// The theme tab: the radios and their combos, and `GetSelectedTheme`.
    pub theme: ThemeChoice,

    /// A refused value (`StrToIntDef` never refuses, but a dialog field can
    /// be empty).
    pub error: Option<String>,
}

impl Default for OptionsState {
    fn default() -> Self {
        let toggles = |items: &[(&'static str, &'static str)]| {
            items
                .iter()
                .map(|(key, caption)| KeyedBool {
                    key,
                    caption,
                    value: false,
                })
                .collect::<Vec<_>>()
        };
        Self {
            active_tab: OptionsTab::General,
            experts_visible: false,
            general: toggles(&GENERAL_TOGGLES),
            collapse: toggles(&COLLAPSE_TOGGLES),
            experts: toggles(&EXPERT_TOGGLES),
            udr_set_xesp: true,
            udr_set_scale: false,
            udr_set_scale_value: "0".to_owned(),
            udr_set_z: true,
            udr_set_z_value: "-30000".to_owned(),
            udr_set_mstt: true,
            udr_set_mstt_value: "0000001B".to_owned(),
            conflict_this_index: 0,
            conflict_all_index: 0,
            colors: ConflictColors::default(),
            font_records: FontSpec::default_records(),
            font_messages: FontSpec::default_messages(),
            font_viewer: FontSpec::default_records(),
            column_width: "200".to_owned(),
            row_height: "19".to_owned(),
            auto_compare_selected_limit: "5".to_owned(),
            nav_change_delay: "100".to_owned(),
            require_ctrl_for_dbl_click: false,
            focus_added_element: true,
            shrink_buttons: false,
            theme: ThemeChoice::System,
            error: None,
        }
    }
}

/// The `[Options]` values the dialog reads (`mniNavOptionsClick`), keyed the
/// way the settings file holds them; a missing key keeps the default.
pub fn state_from_values(
    values: &[(String, String)],
    fonts: &Fonts,
    colors: &ConflictColors,
    theme: ThemeChoice,
    experts_visible: bool,
) -> OptionsState {
    let mut state = OptionsState {
        font_records: fonts.records.clone(),
        font_messages: fonts.messages.clone(),
        font_viewer: fonts.records.clone(),
        colors: colors.clone(),
        theme,
        experts_visible,
        ..OptionsState::default()
    };
    let read = |key: &str| {
        values
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.clone())
    };
    for toggle in state
        .general
        .iter_mut()
        .chain(state.collapse.iter_mut())
        .chain(state.experts.iter_mut())
    {
        if let Some(value) = read(toggle.key) {
            // `ReadBool`: `<> 0`.
            toggle.value = value.trim() != "0" && !value.trim().is_empty();
        }
    }
    let read_bool =
        |key: &str, default: bool| read(key).map_or(default, |value| value.trim() != "0" && !value.trim().is_empty());
    let read_text = |key: &str, default: &str| read(key).unwrap_or_else(|| default.to_owned());
    state.udr_set_xesp = read_bool("UDRSetXESP", true);
    state.udr_set_scale = read_bool("UDRSetScale", false);
    state.udr_set_scale_value = read_text("UDRSetScaleValue", "0");
    state.udr_set_z = read_bool("UDRSetZ", true);
    state.udr_set_z_value = read_text("UDRSetZValue", "-30000");
    state.udr_set_mstt = read_bool("UDRSetMSTT", true);
    // `IntToHex(wbUDRSetMSTTValue, 8)` of the value the `[Options]` key
    // holds (`WriteInteger` writes it in decimal).
    state.udr_set_mstt_value = format!("{:08X}", str_to_int_def(&read_text("UDRSetMSTTValue", "27"), 0x1B));
    state.column_width = read_text("ColumnWidth", "200");
    state.row_height = read_text("RowHeight", "19");
    state.auto_compare_selected_limit = read_text("AutoCompareSelectedLimit", "5");
    state.nav_change_delay = read_text("NavChangeDelay", "100");
    state.require_ctrl_for_dbl_click = read_bool("RequireCtrlForDblClick", false);
    state.focus_added_element = read_bool("FocusAddedElement", true);
    state.shrink_buttons = read_bool("ShrinkButtons", false);
    state
}

impl OptionsState {
    /// The tab a `--dialog options --dialog-arg <tab>` route opens
    /// (`general`, `view`, `cleaning`, `ui`, `theme`, `experts`).
    pub fn set_tab_by_name(&mut self, name: &str) {
        let name = name.to_ascii_lowercase();
        self.active_tab = match name.as_str() {
            "view" | "viewsettings" => OptionsTab::ViewSettings,
            "cleaning" => OptionsTab::Cleaning,
            "ui" | "uisettings" => OptionsTab::UISettings,
            "theme" | "uitheme" => OptionsTab::UITheme,
            "experts" => OptionsTab::Experts,
            _ => OptionsTab::General,
        };
    }

    /// The value of a check box by its key.
    pub fn toggle(&self, key: &str) -> bool {
        self.general
            .iter()
            .chain(self.collapse.iter())
            .chain(self.experts.iter())
            .find(|toggle| toggle.key.eq_ignore_ascii_case(key))
            .map(|toggle| toggle.value)
            .unwrap_or(false)
    }

    /// Sets a check box by its key.
    pub fn set_toggle(&mut self, key: &str, value: bool) {
        if let Some(toggle) = self
            .general
            .iter_mut()
            .chain(self.collapse.iter_mut())
            .chain(self.experts.iter_mut())
            .find(|toggle| toggle.key.eq_ignore_ascii_case(key))
        {
            toggle.value = value;
        }
    }

    /// `GetSelectedTheme`: the text of the radio group's combo box.
    pub fn selected_theme(&self) -> String {
        match &self.theme {
            ThemeChoice::System => "System".to_owned(),
            ThemeChoice::Light => "Light".to_owned(),
            ThemeChoice::Dark => "Dark".to_owned(),
            ThemeChoice::Named(name) => name.clone(),
        }
    }

    /// The styles of the three radio groups: the port's three themes, with a
    /// named style in the dark group.
    pub fn theme_group(&self, group: ThemeChoice) -> Vec<String> {
        match group {
            ThemeChoice::System => vec!["System".to_owned()],
            ThemeChoice::Light => vec!["Light".to_owned()],
            ThemeChoice::Dark => {
                let mut items = vec!["Dark".to_owned()];
                if let ThemeChoice::Named(name) = &self.theme {
                    items.push(name.clone());
                }
                items
            }
            ThemeChoice::Named(_) => Vec::new(),
        }
    }

    /// The theme a radio click selects (`rbThemeClick` keeps one group
    /// checked).
    pub fn choose_theme(&mut self, choice: ThemeChoice) {
        self.theme = choice;
    }

    /// `clbConflictThisChange`: the colour box writes the entry the combo
    /// selects.
    pub fn set_conflict_this(&mut self, color: egui::Color32) {
        // The combo lists `ctNotDefined` to `ctConflictLoses`.
        let value = ConflictThis::ALL
            .iter()
            .skip(2)
            .nth(self.conflict_this_index)
            .copied()
            .unwrap_or(ConflictThis::ctConflictLoses);
        self.colors.this[value.ord()] = color;
        self.colors.this_system[value.ord()] = color == crate::widgets::conflict::SYSTEM_WINDOW_TEXT;
    }

    /// The entries of the `cbConflictThis` combo: the names from
    /// `ctNotDefined` on.
    pub fn conflict_this_items(&self) -> Vec<&'static str> {
        ConflictThis::ALL.iter().skip(2).map(|value| value.name()).collect()
    }

    /// The entries of `cbConflictAll`: the names from `caNoConflict` on.
    pub fn conflict_all_items(&self) -> Vec<&'static str> {
        ConflictAll::ALL.iter().skip(2).map(|value| value.name()).collect()
    }

    /// The colour of the currently selected `ConflictThis` entry.
    pub fn selected_conflict_this(&self) -> egui::Color32 {
        let value = ConflictThis::ALL
            .iter()
            .skip(2)
            .nth(self.conflict_this_index)
            .copied()
            .unwrap_or(ConflictThis::ctConflictLoses);
        self.colors.this[value.ord()]
    }

    /// The colour of the currently selected `ConflictAll` entry.
    pub fn selected_conflict_all(&self) -> Option<egui::Color32> {
        let value = ConflictAll::ALL
            .iter()
            .skip(2)
            .nth(self.conflict_all_index)
            .copied()
            .unwrap_or(ConflictAll::caConflictCritical);
        self.colors.all[value.ord()]
    }

    /// `clbConflictAllChange`.
    pub fn set_conflict_all(&mut self, color: egui::Color32) {
        let value = ConflictAll::ALL
            .iter()
            .skip(2)
            .nth(self.conflict_all_index)
            .copied()
            .unwrap_or(ConflictAll::caConflictCritical);
        self.colors.all[value.ord()] = Some(color);
    }

    /// Every `[Options]` key the OK writes (`mniNavOptionsClick`), with the
    /// type it is written with.
    pub fn option_values(&self) -> Vec<(&'static str, String, &'static str)> {
        let mut values = Vec::new();
        for toggle in self
            .general
            .iter()
            .chain(self.collapse.iter())
            .chain(self.experts.iter())
        {
            values.push((toggle.key, if toggle.value { "1" } else { "0" }.to_owned(), "bool"));
        }
        values.push((
            "UDRSetXESP",
            if self.udr_set_xesp { "1" } else { "0" }.to_owned(),
            "bool",
        ));
        values.push((
            "UDRSetScale",
            if self.udr_set_scale { "1" } else { "0" }.to_owned(),
            "bool",
        ));
        values.push(("UDRSetScaleValue", self.udr_set_scale_value.clone(), "float"));
        values.push(("UDRSetZ", if self.udr_set_z { "1" } else { "0" }.to_owned(), "bool"));
        values.push(("UDRSetZValue", self.udr_set_z_value.clone(), "float"));
        values.push((
            "UDRSetMSTT",
            if self.udr_set_mstt { "1" } else { "0" }.to_owned(),
            "bool",
        ));
        values.push((
            "UDRSetMSTTValue",
            str_to_int_def(
                &format!("${}", self.udr_set_mstt_value.trim().trim_start_matches('$')),
                0x1B,
            )
            .to_string(),
            "integer",
        ));
        values.push((
            "RequireCtrlForDblClick",
            bool_text(self.require_ctrl_for_dbl_click),
            "bool",
        ));
        values.push(("FocusAddedElement", bool_text(self.focus_added_element), "bool"));
        values.push(("ShrinkButtons", bool_text(self.shrink_buttons), "bool"));
        values.push(("ColumnWidth", self.column_width.clone(), "integer"));
        values.push(("RowHeight", self.row_height.clone(), "integer"));
        values.push((
            "AutoCompareSelectedLimit",
            self.auto_compare_selected_limit.clone(),
            "integer",
        ));
        values.push(("NavChangeDelay", self.nav_change_delay.clone(), "integer"));
        values
    }

    /// The `[ColorConflictThis]` and `[ColorConflictAll]` entries the OK
    /// writes (`GetEnumName` keys, `Integer(wbColorConflictThis[ct])`).
    pub fn color_values(&self) -> Vec<(&'static str, String, i64)> {
        let mut values = Vec::new();
        for value in ConflictThis::ALL {
            values.push((
                "ColorConflictThis",
                value.enum_name().to_owned(),
                i64::from(ConflictColors::tcolor(self.colors.this[value.ord()])),
            ));
        }
        for value in ConflictAll::ALL {
            let color = self.colors.all[value.ord()].unwrap_or(egui::Color32::BLACK);
            values.push((
                "ColorConflictAll",
                value.enum_name().to_owned(),
                i64::from(ConflictColors::tcolor(color)),
            ));
        }
        values
    }

    /// `[UI] Theme` and the font keys (`SaveFont` of the three panels), all
    /// written to the `UI` section.
    pub fn ui_values(&self) -> Vec<(String, String, &'static str)> {
        let mut values = vec![("Theme".to_owned(), self.theme.as_setting(), "string")];
        for (name, font) in [
            ("FontRecords", &self.font_records),
            ("FontMessages", &self.font_messages),
            ("FontViewer", &self.font_viewer),
        ] {
            values.push((font_key(name, "Name"), font.name.clone(), "string"));
            values.push((font_key(name, "CharSet"), font.char_set.to_string(), "integer"));
            values.push((font_key(name, "Color"), font.color.to_string(), "integer"));
            values.push((font_key(name, "Size"), font.size.to_string(), "integer"));
            values.push((font_key(name, "Style"), font.style.to_string(), "integer"));
        }
        values
    }

    /// Applies the `wb*` globals of the dialog to the process
    /// (`mniNavOptionsClick` after `ShowModal`). The port's translations
    /// mode leaves the three hide switches alone, as upstream does.
    pub fn apply_globals(&self) {
        if !globals::translation_mode() {
            globals::set_hide_unused(self.toggle("HideUnused"));
            globals::set_hide_ignored(self.toggle("HideIgnored"));
            globals::set_hide_never_show(self.toggle("HideNeverShow"));
        }
        globals::set_actor_template_hide(self.toggle("ActorTemplateHide"));
        globals::set_load_bsas(self.toggle("LoadBSAs"));
        globals::set_sort_flst(self.toggle("SortFLST2"));
        globals::set_sort_info(self.toggle("SortINFO"));
        globals::set_fill_pnam(self.toggle("FillPNAM"));
        globals::set_focus_added_element(self.focus_added_element);
        globals::set_require_ctrl_for_dbl_click(self.require_ctrl_for_dbl_click);
        globals::set_show_flag_enum_value(self.toggle("ShowFlagEnumValue"));
        globals::set_show_group_record_count(self.toggle("ShowGroupRecordCount"));
        globals::set_show_file_flags(self.toggle("ShowFileFlags"));
        globals::set_auto_compare_selected_limit(str_to_int_def(&self.auto_compare_selected_limit, 5));
        globals::set_simple_records(self.toggle("SimpleRecords"));
        globals::set_decode_texture_hashes(self.toggle("DecodeTextureHashes2"));
        globals::set_clamp_form_id(self.toggle("ClampFormID"));
        globals::set_reset_modified_on_save(self.toggle("ResetModifiedOnSave"));
        globals::set_always_save_onam(self.toggle("AlwaysSaveOnam") || globals::always_save_onam_force());
        globals::set_align_array_elements(self.toggle("AlignArrayElements"));
        globals::set_manual_cleaning_hide(self.toggle("ManualCleaningHide"));
        globals::set_manual_cleaning_allow(self.toggle("ManualCleaningAllow"));
        globals::set_convert_int_form_id(self.toggle("ConvertIntFormID"));
        globals::set_collapse_record_header(self.toggle("CollapseRecordHeader"));
        globals::set_collapse_object_bounds(self.toggle("CollapseObjectBounds"));
        globals::set_collapse_models(self.toggle("CollapseModels"));
        globals::set_collapse_factions(self.toggle("CollapseFactions"));
        globals::set_collapse_faction_relations(self.toggle("CollapseFactionRelations"));
        globals::set_collapse_items(self.toggle("CollapseItems"));
        globals::set_collapse_leveled_items(self.toggle("CollapseLeveledItems"));
        globals::set_collapse_equip_slots(self.toggle("CollapseEquipSlots"));
        globals::set_collapse_object_properties(self.toggle("CollapseObjectProperties"));
        globals::set_collapse_script_properties(self.toggle("CollapseScriptProperties"));
        globals::set_collapse_conditions(self.toggle("CollapseConditions"));
        globals::set_collapse_benign_array(self.toggle("CollapseBenignArray"));
        globals::set_collapse_rgba(self.toggle("CollapseRGBA"));
        globals::set_collapse_vec3(self.toggle("CollapseVec3"));
        globals::set_collapse_pos_rot(self.toggle("CollapsePosRot"));
        globals::set_collapse_fragments(self.toggle("CollapseFragments"));
        globals::set_collapse_script_data(self.toggle("CollapseScriptData"));
        globals::set_collapse_flags(self.toggle("CollapseFlags"));
        globals::set_collapse_sounds(self.toggle("CollapseSounds"));
        globals::set_collapse_navmesh(self.toggle("CollapseNavmesh"));
        globals::set_collapse_keywords(self.toggle("CollapseKeywords"));
        globals::set_collapse_aliases(self.toggle("CollapseAliases"));
        globals::set_collapse_range(self.toggle("CollapseRange"));
        globals::set_collapse_arma_bone_data(self.toggle("CollapseARMABoneData"));
        globals::set_collapse_race_bone_data(self.toggle("CollapseRACEBoneData"));
        globals::set_collapse_head_parts(self.toggle("CollapseHeadParts"));
        globals::set_collapse_body_parts(self.toggle("CollapseBodyParts"));
        globals::set_collapse_model_info(self.toggle("CollapseModelInfo"));
        globals::set_collapse_placement(self.toggle("CollapsePlacement"));
        globals::set_collapse_vertices(self.toggle("CollapseVertices"));
        globals::set_collapse_transforms(self.toggle("CollapseTransforms"));
        globals::set_collapse_destruction(self.toggle("CollapseDestruction"));
        globals::set_collapse_locations(self.toggle("CollapseLocations"));
        globals::set_collapse_other(self.toggle("CollapseOther"));
        globals::set_collapse_perk(self.toggle("CollapsePerk"));
        globals::set_collapse_faction_ranks(self.toggle("CollapseFactionRanks"));
        globals::set_collapse_ownership(self.toggle("CollapseOwnership"));
        globals::set_collapse_base_form_component(self.toggle("CollapseBaseFormComponent"));
        globals::set_collapse_ragdoll(self.toggle("CollapseRagdoll"));
        globals::set_collapse_direction_rotation(self.toggle("CollapseDirectionRotation"));
        // `wbShrinkButtons` follows the dialog; the main form's project
        // buttons shrink from it.
        globals::set_shrink_buttons(self.shrink_buttons);
        globals::set_show_tip(self.toggle("ShowTip"));
        globals::set_patron(self.toggle("Patron"));
        globals::set_no_git_hub_check(self.toggle("NoGitHubCheck"));
        globals::set_no_nexus_mods_check(self.toggle("NoNexusModsCheck"));
        globals::set_track_all_editor_id(self.toggle("TrackAllEditorID"));
        globals::set_udr_set_xesp(self.udr_set_xesp);
        globals::set_udr_set_scale(self.udr_set_scale);
        globals::set_udr_set_z(self.udr_set_z);
        globals::set_udr_set_mstt(self.udr_set_mstt);
    }
}

fn bool_text(value: bool) -> String {
    if value { "1" } else { "0" }.to_owned()
}

/// The `[UI]` key of a font panel (`<name>Name`, `<name>Size`, ...).
fn font_key(name: &str, suffix: &str) -> String {
    format!("{name}{suffix}")
}

/// Draws the dialog; `Some(OptionsAction::Ok)` or
/// `Some(OptionsAction::Cancelled)` end it (`btnOK`, `btnCancel`,
/// `VK_ESCAPE`).
pub fn show(ctx: &egui::Context, state: &mut OptionsState) -> Option<OptionsAction> {
    let mut action = None;
    let mut open = true;
    egui::Window::new("Options")
        .id(egui::Id::unique("frmOptions"))
        .collapsible(false)
        .resizable(true)
        .default_size(crate::dialogs::selection::fit_size(ctx, egui::vec2(470.0, 460.0)))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for tab in OptionsTab::ALL {
                    // `tsExperts.TabVisible := wbIKnowWhatImDoing`.
                    if tab == OptionsTab::Experts && !state.experts_visible {
                        continue;
                    }
                    if ui.selectable_label(state.active_tab == tab, tab.caption()).clicked() {
                        state.active_tab = tab;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("options-page")
                .max_height(330.0)
                .show(ui, |ui| match state.active_tab {
                    OptionsTab::General => toggle_rows(ui, &mut state.general),
                    OptionsTab::ViewSettings => {
                        ui.label("Fields to Collapse by Default (requires restart)");
                        toggle_rows(ui, &mut state.collapse);
                    }
                    OptionsTab::Cleaning => {
                        ui.label("Undelete and Disable Reference");
                        ui.checkbox(
                            &mut state.udr_set_xesp,
                            "Set enabled state opposite of player (deactivate at your own risk, this is a critical setting)",
                        );
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut state.udr_set_scale, "Set scale to");
                            ui.add(egui::TextEdit::singleline(&mut state.udr_set_scale_value).desired_width(84.0));
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut state.udr_set_z, "Set Z position to");
                            ui.add(egui::TextEdit::singleline(&mut state.udr_set_z_value).desired_width(84.0));
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut state.udr_set_mstt, "[FO3/FNV] Replace MSTT FormID with");
                            ui.add(egui::TextEdit::singleline(&mut state.udr_set_mstt_value).desired_width(82.0));
                        });
                    }
                    OptionsTab::UISettings => {
                        ui.horizontal(|ui| {
                            ui.label("Conflict Color Font");
                            let items = state.conflict_this_items();
                            combo(ui, "options-conflict-this", &mut state.conflict_this_index, &items);
                            let mut color = state.selected_conflict_this();
                            if color_picker(ui, "options-color-this", &mut color) {
                                state.set_conflict_this(color);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Conflict Color Background");
                            let items = state.conflict_all_items();
                            combo(ui, "options-conflict-all", &mut state.conflict_all_index, &items);
                            let mut color = state.selected_conflict_all().unwrap_or(egui::Color32::BLACK);
                            if color_picker(ui, "options-color-all", &mut color) {
                                state.set_conflict_all(color);
                            }
                        });
                        ui.separator();
                        for (label, font) in [
                            ("Records font", &mut state.font_records),
                            ("Messages font", &mut state.font_messages),
                            ("Viewer font", &mut state.font_viewer),
                        ] {
                            ui.label(format!("{label}: {} {} pt", font.name, font.size));
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Column Width");
                            ui.add(egui::TextEdit::singleline(&mut state.column_width).desired_width(60.0));
                            ui.label("Row Height");
                            ui.add(egui::TextEdit::singleline(&mut state.row_height).desired_width(60.0));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Auto \"Compare Selected\" when multi-selecting up to");
                            ui.add(
                                egui::TextEdit::singleline(&mut state.auto_compare_selected_limit).desired_width(50.0),
                            );
                            ui.label("records");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Delay updating View when navigation treeview selection changes by");
                            ui.add(egui::TextEdit::singleline(&mut state.nav_change_delay).desired_width(60.0));
                            ui.label("msec");
                        });
                        ui.checkbox(
                            &mut state.require_ctrl_for_dbl_click,
                            "Selection forms require holding CTRL for double click",
                        );
                        ui.checkbox(
                            &mut state.focus_added_element,
                            "Focus newly added elements in View (can be inverted by holding CTRL)",
                        );
                        ui.checkbox(
                            &mut state.shrink_buttons,
                            "Shrink SpeedButtons in the top right corner of the main form",
                        );
                    }
                    OptionsTab::UITheme => theme_tab(ui, state),
                    OptionsTab::Experts => toggle_rows(ui, &mut state.experts),
                });
            if let Some(error) = &state.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    action = Some(OptionsAction::Ok);
                }
                if ui.button("Cancel").clicked() {
                    action = Some(OptionsAction::Cancelled);
                }
            });
        });
    if action.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        action = Some(OptionsAction::Cancelled);
    }
    if !open && action.is_none() {
        action = Some(OptionsAction::Cancelled);
    }
    action
}

fn toggle_rows(ui: &mut egui::Ui, toggles: &mut [KeyedBool]) {
    for toggle in toggles {
        ui.checkbox(&mut toggle.value, toggle.caption);
    }
}

fn combo(ui: &mut egui::Ui, id: &str, index: &mut usize, items: &[&'static str]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(items.get(*index).copied().unwrap_or_default())
        .show_ui(ui, |ui| {
            for (position, item) in items.iter().enumerate() {
                ui.selectable_value(index, position, *item);
            }
        });
}

fn color_picker(ui: &mut egui::Ui, _id: &str, color: &mut egui::Color32) -> bool {
    ui.color_edit_button_srgba(color).changed()
}

/// `tbsUITheme`: the three radio groups, their combo boxes and the preview.
fn theme_tab(ui: &mut egui::Ui, state: &mut OptionsState) {
    ui.horizontal(|ui| {
        for (choice, label) in [
            (ThemeChoice::System, "System"),
            (ThemeChoice::Light, "Light"),
            (ThemeChoice::Dark, "Dark"),
        ] {
            // `rbThemeClick`: the checked group enables its combo.
            let checked = state.theme == choice;
            if ui.radio(checked, label).clicked() {
                state.choose_theme(choice.clone());
            }
            let items = state.theme_group(choice.clone());
            // Only the checked group's combo is enabled; the others show
            // their own first style.
            let selected = if checked {
                state.selected_theme()
            } else {
                items.first().cloned().unwrap_or_default()
            };
            ui.add_enabled_ui(checked, |ui| {
                egui::ComboBox::from_id_salt(("options-theme", label))
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for item in items {
                            let value = ThemeChoice::from_setting(&item);
                            ui.selectable_value(&mut state.theme, value, item);
                        }
                    });
            });
        }
    });
    ui.separator();
    // `UpdateThemePreview`: a system style shows no preview.
    match state.theme {
        ThemeChoice::System => {
            ui.label("The system style has no preview.");
        }
        ThemeChoice::Named(ref name) => {
            ui.label(format!("{name}: a VCL style the port cannot render."));
        }
        ref choice => {
            // `TVisualStylePreview` (ptTabs) of the chosen style.
            let visuals = match choice {
                ThemeChoice::Light => egui::Visuals::light(),
                _ => egui::Visuals::dark(),
            };
            egui::Frame::new()
                .fill(visuals.panel_fill)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for tab in ["Tab 1", "Tab 2", "Tab 3"] {
                            let _ = ui.selectable_label(false, tab);
                        }
                    });
                    ui.label(egui::RichText::new("Preview").color(visuals.text_color()));
                    let _ = ui.add(egui::Button::new("OK"));
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_upstream_keys_are_read_into_the_tabs() {
        let values = vec![
            ("HideUnused".to_owned(), "0".to_owned()),
            ("LoadBSAs".to_owned(), "0".to_owned()),
            ("CollapseRGBA".to_owned(), "0".to_owned()),
            ("UDRSetScale".to_owned(), "1".to_owned()),
            ("UDRSetScaleValue".to_owned(), "0.5".to_owned()),
            ("UDRSetMSTTValue".to_owned(), "27".to_owned()),
            ("ColumnWidth".to_owned(), "250".to_owned()),
            ("RowHeight".to_owned(), "24".to_owned()),
            ("AutoCompareSelectedLimit".to_owned(), "8".to_owned()),
            ("NavChangeDelay".to_owned(), "250".to_owned()),
            ("ShrinkButtons".to_owned(), "1".to_owned()),
            ("SortINFO".to_owned(), "1".to_owned()),
        ];
        let state = state_from_values(
            &values,
            &Fonts::default(),
            &ConflictColors::default(),
            ThemeChoice::Dark,
            true,
        );
        assert!(!state.toggle("HideUnused"));
        assert!(!state.toggle("LoadBSAs"));
        assert!(!state.toggle("CollapseRGBA"));
        assert!(state.toggle("SortINFO"));
        assert!(state.udr_set_scale);
        assert_eq!(state.udr_set_scale_value, "0.5");
        // `IntToHex(wbUDRSetMSTTValue, 8)` of the `[Options]` value.
        assert_eq!(state.udr_set_mstt_value, "0000001B");
        assert_eq!(state.column_width, "250");
        assert_eq!(state.row_height, "24");
        assert_eq!(state.auto_compare_selected_limit, "8");
        assert_eq!(state.nav_change_delay, "250");
        assert!(state.shrink_buttons);
        assert!(state.experts_visible);
        assert_eq!(state.theme, ThemeChoice::Dark);
    }

    #[test]
    fn the_option_values_round_trip_through_the_settings_keys() {
        let values = vec![
            ("HideIgnored".to_owned(), "0".to_owned()),
            ("CollapseModels".to_owned(), "0".to_owned()),
            ("ShowFlagEnumValue".to_owned(), "1".to_owned()),
            ("UDRSetZValue".to_owned(), "-15000".to_owned()),
            ("UDRSetMSTTValue".to_owned(), "27".to_owned()),
            ("ColumnWidth".to_owned(), "300".to_owned()),
        ];
        let state = state_from_values(
            &values,
            &Fonts::default(),
            &ConflictColors::default(),
            ThemeChoice::System,
            false,
        );
        let written: Vec<(String, String)> = state
            .option_values()
            .into_iter()
            .map(|(key, value, _)| (key.to_owned(), value))
            .collect();
        assert!(written.contains(&("HideIgnored".to_owned(), "0".to_owned())));
        assert!(written.contains(&("CollapseModels".to_owned(), "0".to_owned())));
        assert!(written.contains(&("ShowFlagEnumValue".to_owned(), "1".to_owned())));
        assert!(written.contains(&("UDRSetZValue".to_owned(), "-15000".to_owned())));
        // `WriteInteger('UDRSetMSTTValue', wbUDRSetMSTTValue)` of the hex
        // text the edit box holds.
        assert!(written.contains(&("UDRSetMSTTValue".to_owned(), "27".to_owned())));
        assert!(written.contains(&("ColumnWidth".to_owned(), "300".to_owned())));
        // Every key the dialog writes is one it reads.
        let read_back = state_from_values(
            &written,
            &Fonts::default(),
            &ConflictColors::default(),
            ThemeChoice::System,
            false,
        );
        assert_eq!(read_back.option_values(), state.option_values());
    }

    #[test]
    fn the_colour_entries_are_written_per_enum_name() {
        let state = OptionsState::default();
        let values = state.color_values();
        // 12 `TConflictThis` and 7 `TConflictAll` entries, keyed by enum
        // name (`GetEnumName`).
        assert_eq!(values.len(), 19);
        assert!(values.contains(&("ColorConflictThis", "ctConflictWins".to_owned(), 0x0000_80FF_i64)));
        // `clRed` of `caConflict` is $000000FF.
        assert!(values.contains(&("ColorConflictAll", "caConflict".to_owned(), 0x0000_00FF_i64)));
        assert!(
            values
                .iter()
                .any(|(section, key, _)| *section == "ColorConflictThis" && key == "ctUnknown")
        );
    }

    #[test]
    fn the_theme_radios_offer_the_three_styles() {
        let mut state = OptionsState::default();
        assert_eq!(state.theme_group(ThemeChoice::System), vec!["System".to_owned()]);
        assert_eq!(state.theme_group(ThemeChoice::Light), vec!["Light".to_owned()]);
        assert_eq!(state.theme_group(ThemeChoice::Dark), vec!["Dark".to_owned()]);
        state.theme = ThemeChoice::Named("Carbon".to_owned());
        // A named style is offered as the dark group's second entry.
        assert_eq!(
            state.theme_group(ThemeChoice::Dark),
            vec!["Dark".to_owned(), "Carbon".to_owned()]
        );
        assert_eq!(state.selected_theme(), "Carbon");
        state.choose_theme(ThemeChoice::Light);
        assert_eq!(state.selected_theme(), "Light");
    }

    #[test]
    fn the_conflict_colour_boxes_edit_the_selected_entry() {
        // Both combos start at their first entry: `ctNotDefined` and
        // `caNoConflict`.
        let mut state = OptionsState {
            conflict_this_index: 0,
            conflict_all_index: 0,
            ..OptionsState::default()
        };
        state.set_conflict_this(egui::Color32::from_rgb(1, 2, 3));
        assert_eq!(
            state.colors.this[ConflictThis::ctNotDefined.ord()],
            egui::Color32::from_rgb(1, 2, 3)
        );
        state.set_conflict_all(egui::Color32::from_rgb(4, 5, 6));
        assert_eq!(
            state.colors.all[ConflictAll::caNoConflict.ord()],
            Some(egui::Color32::from_rgb(4, 5, 6))
        );
    }

    #[test]
    fn the_ui_values_carry_the_theme_and_the_font_keys() {
        let state = OptionsState::default();
        let values = state.ui_values();
        assert!(values.contains(&("Theme".to_owned(), "System".to_owned(), "string")));
        assert!(
            values
                .iter()
                .any(|(key, value, _)| key == "FontRecordsName" && value == "Tahoma")
        );
        assert!(
            values
                .iter()
                .any(|(key, value, _)| key == "FontMessagesSize" && value == "8")
        );
    }

    #[test]
    fn apply_globals_sets_the_options_the_main_form_reads() {
        let _guard = globals::test_lock();
        let mut state = OptionsState::default();
        state.set_toggle("HideUnused", false);
        state.set_toggle("CollapseRGBA", false);
        state.udr_set_scale = true;
        state.auto_compare_selected_limit = "9".to_owned();
        state.apply_globals();
        assert!(!globals::hide_unused());
        assert!(!globals::collapse_rgba());
        assert!(globals::udr_set_scale());
        assert_eq!(globals::auto_compare_selected_limit(), 9);
        // The defaults are restored for the rest of the suite.
        globals::reset_globals();
    }
}
