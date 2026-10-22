// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeFilterOptionsForm.pas (FormCreate, FormShow,
// FormClose, FormKeyDown, mniSelectionClick, clbKeyPress, cmbPresetSelect,
// btnFilterAddClick, btnFilterDelClick, btnFilterSaveClick,
// FilterListPresets, FilterLoadPreset, FilterSavePreset,
// FilterHasPresetChanged, GetRecordSignatures, SetRecordSignatures,
// GetBaseRecordSignatures, SetBaseRecordSignatures) and
// xEdit/xeFilterOptionsForm.dfm

//! `TfrmFilterOptions`: the filter options as the dialog holds them — the
//! conflict status lists, the inject/not-reachable/references checks, the
//! editor ID, name, element value, base editor ID and base name texts, the
//! signature lists, the persistence and mesh checks, the flattening
//! switches, and the preset combo with Add, Remove and Save.
//!
//! The dialog turns into [`FilterRequestOptions`] for `filter.apply` and
//! into `[Filter <name>]` values for `filter.preset.save`. The values of a
//! preset are read with `settings.get` and parsed with the defaults of
//! `FilterLoadPreset`.

use eframe::egui;

use xedit_core::interface::misc::str_to_int_def;
use xedit_core::interface::types::{ConflictAll, ConflictThis, PascalEnum};
use xedit_session::conflicts::{ConflictAllName, ConflictThisName};
use xedit_session::filter::{FilterRequestOptions, SignatureEntry};

use crate::dialogs::selection::{CheckItem, Checklist, DialogPlacement, PresetCombo, Question, show_question};

/// The section of a filter preset (`sFilterSection`).
pub const FILTER_SECTION: &str = "Filter";

/// `FilterListPresets`' empty entry: the defaults.
pub const DEFAULTS_PRESET: &str = "";

/// What the dialog asks the shell to run.
#[derive(Clone, Debug, PartialEq)]
pub enum FilterOptionsAction {
    /// `btnFilter`/`FormClose` with `mrOk`: the shell writes
    /// `View/LastUsedFilter`, saves the preset if needed, then runs
    /// `filter.apply`.
    Commit,
    /// The changed-preset question was answered Yes.
    CommitAndSavePreset(String),
    /// The changed-preset question was answered No.
    CommitWithoutSaving,
    Cancelled,
    /// `btnFilterDelClick` (a named preset) and `btnFilterSaveClick`.
    DeletePreset(String),
    SavePreset(String),
    /// A preset choice: the shell reads its `[Filter <name>]` values and
    /// calls [`FilterOptionsState::apply_preset_values`].
    LoadPreset(String),
}

/// `TfrmFilterOptions` as data.
pub struct FilterOptionsState {
    /// `cbConflictAll` with `clbConflictAll`.
    pub conflict_all: bool,
    /// The six entries `caOnlyOne` .. `caConflictCritical`.
    pub conflict_all_list: Checklist,
    /// `cbConflictThis` with `clbConflictThis`.
    pub conflict_this: bool,
    /// The ten entries `ctNotDefined` .. `ctConflictLoses`.
    pub conflict_this_list: Checklist,

    pub by_injection_status: bool,
    pub injected: bool,
    pub by_not_reachable_status: bool,
    pub not_reachable: bool,
    /// `cbByNotReachableStatus.Enabled := ReachableBuild`.
    pub reachable_build: bool,
    pub by_references_injected_status: bool,
    pub references_injected: bool,

    pub by_editor_id: bool,
    pub editor_id: String,
    pub by_name: bool,
    pub name: String,
    pub by_base_editor_id: bool,
    pub base_editor_id: String,
    pub by_base_name: bool,
    pub base_name: String,
    pub by_element_value: bool,
    pub element_value: String,
    pub regex_comparison: bool,
    pub scaled_actors: bool,

    pub record_signature: bool,
    pub record_signatures: Checklist,
    pub base_record_signature: bool,
    pub base_record_signatures: Checklist,

    pub by_persistent: bool,
    pub persistent: bool,
    pub unnecessary_persistent: bool,
    pub master_is_temporary: bool,
    pub is_master: bool,
    pub persistent_pos_changed: bool,
    pub deleted: bool,
    pub by_vwd: bool,
    pub vwd: bool,
    pub by_has_vwd_mesh: bool,
    pub has_vwd_mesh: bool,
    pub by_has_precombined_mesh: bool,
    pub has_precombined_mesh: bool,
    pub flatten_blocks: bool,
    pub flatten_cell_childs: bool,
    pub assign_pers_wrld_child: bool,
    pub inherit: bool,

    /// `cbScriptedFilter` has no control in the dialog; the request keeps
    /// the value the dialog was opened with.
    pub scripted: bool,

    /// `cmbPreset` with `FilterListPresets`.
    pub preset: PresetCombo,
    /// `View|LastUsedFilter` of the settings file.
    pub last_used: String,
    /// A session exists: the preset commands and `settings.*` can run.
    pub commands_available: bool,
    /// The `FilterHasPresetChanged` question, when the close asked it.
    pub question: Option<FilterQuestion>,
    /// The last command error (a preset name the command refused).
    pub error: Option<String>,
    pub placement: Option<DialogPlacement>,
    pub rect: Option<egui::Rect>,
}

/// The question `FormClose` asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterQuestion {
    pub preset: String,
}

impl FilterOptionsState {
    /// The dialog `FormCreate` + `FormShow`: the two conflict lists, the
    /// signatures from `signatures.list`, the presets from
    /// `filter.presets`, and the empty preset's values as `FilterLoadPreset`
    /// reads them.
    pub fn new(
        records: &[SignatureEntry],
        base_records: &[SignatureEntry],
        presets: Vec<String>,
        last_used: String,
        reachable_build: bool,
    ) -> Self {
        let mut state = Self {
            conflict_all: true,
            conflict_all_list: Checklist::new(
                ConflictAll::ALL
                    .iter()
                    .skip(1)
                    .map(|value| CheckItem::new(value.name(), true)),
            ),
            conflict_this: true,
            conflict_this_list: Checklist::new(
                ConflictThis::ALL
                    .iter()
                    .skip(2)
                    .map(|value| CheckItem::new(value.name(), true)),
            ),
            by_injection_status: false,
            injected: true,
            by_not_reachable_status: false,
            not_reachable: true,
            reachable_build,
            by_references_injected_status: false,
            references_injected: true,
            by_editor_id: false,
            editor_id: String::new(),
            by_name: false,
            name: String::new(),
            by_base_editor_id: false,
            base_editor_id: String::new(),
            by_base_name: false,
            base_name: String::new(),
            by_element_value: false,
            element_value: String::new(),
            regex_comparison: false,
            scaled_actors: false,
            record_signature: false,
            record_signatures: Checklist::new(records.iter().map(|entry| CheckItem::new(&entry.text, false))),
            base_record_signature: false,
            base_record_signatures: Checklist::new(base_records.iter().map(|entry| CheckItem::new(&entry.text, false))),
            by_persistent: false,
            persistent: true,
            unnecessary_persistent: false,
            master_is_temporary: false,
            is_master: false,
            persistent_pos_changed: false,
            deleted: false,
            by_vwd: false,
            vwd: true,
            by_has_vwd_mesh: false,
            has_vwd_mesh: true,
            by_has_precombined_mesh: false,
            has_precombined_mesh: true,
            flatten_blocks: false,
            flatten_cell_childs: false,
            assign_pers_wrld_child: false,
            inherit: true,
            scripted: false,
            preset: PresetCombo::new(presets, Vec::new()),
            last_used: last_used.clone(),
            commands_available: true,
            question: None,
            error: None,
            placement: None,
            rect: None,
        };
        state.preset.text = last_used;
        state
    }

    /// The dialog of the tests and of a shell without a bridge.
    pub fn bare() -> Self {
        let mut state = Self::new(&[], &[], vec![DEFAULTS_PRESET.to_owned()], String::new(), false);
        state.commands_available = false;
        state
    }

    /// `GetRecordSignatures`.
    pub fn record_signatures(&self) -> String {
        self.record_signatures.signatures()
    }

    /// `GetBaseRecordSignatures`.
    pub fn base_record_signatures(&self) -> String {
        self.base_record_signatures.signatures()
    }

    /// The request `filter.apply` and `filter.preset.save` take.
    pub fn to_options(&self) -> FilterRequestOptions {
        let conflict_all = self.conflict_all.then(|| {
            ConflictAll::ALL
                .iter()
                .skip(1)
                .zip(self.conflict_all_list.items.iter())
                .filter(|(_, item)| item.checked)
                .map(|(value, _)| ConflictAllName::from(*value))
                .collect::<Vec<_>>()
        });
        let conflict_this = self.conflict_this.then(|| {
            ConflictThis::ALL
                .iter()
                .skip(2)
                .zip(self.conflict_this_list.items.iter())
                .filter(|(_, item)| item.checked)
                .map(|(value, _)| ConflictThisName::from(*value))
                .collect::<Vec<_>>()
        });
        FilterRequestOptions {
            conflict_all,
            conflict_this,
            by_inject_status: self.by_injection_status.then_some(self.injected),
            by_not_reachable_status: (self.by_not_reachable_status && self.reachable_build)
                .then_some(self.not_reachable),
            by_references_injected_status: self.by_references_injected_status.then_some(self.references_injected),
            by_editor_id: self.by_editor_id.then(|| self.editor_id.clone()),
            by_element_value: self.by_element_value.then(|| self.element_value.clone()),
            by_name: self.by_name.then(|| self.name.clone()),
            by_base_editor_id: self.by_base_editor_id.then(|| self.base_editor_id.clone()),
            by_base_name: self.by_base_name.then(|| self.base_name.clone()),
            scaled_actors: self.scaled_actors,
            by_signature: self.record_signature.then(|| self.record_signatures()),
            by_base_signature: self.base_record_signature.then(|| self.base_record_signatures()),
            by_persistent: self.by_persistent,
            persistent: self.persistent,
            unnecessary_persistent: self.unnecessary_persistent,
            master_is_temporary: self.master_is_temporary,
            is_master: self.is_master,
            persistent_pos_changed: self.persistent_pos_changed,
            deleted: self.deleted,
            by_vwd: self.by_vwd.then_some(self.vwd),
            by_has_vwd_mesh: self.by_has_vwd_mesh.then_some(self.has_vwd_mesh),
            by_has_precombined_mesh: self.by_has_precombined_mesh.then_some(self.has_precombined_mesh),
            regex_comparison: self.regex_comparison,
            flatten_blocks: self.flatten_blocks,
            flatten_cell_childs: self.flatten_cell_childs,
            assign_pers_wrld_child: self.assign_pers_wrld_child,
            inherit_conflict_by_parent: self.inherit,
            scripted: self.scripted,
        }
    }

    /// `FilterLoadPreset`: the section's values with the defaults of an
    /// empty section. `values` is the `settings.get` response of
    /// `[Filter <name>]`.
    pub fn apply_preset_values(&mut self, name: &str, values: &[(String, String)]) {
        let read_bool = |key: &str, default: bool| {
            values
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| str_to_int_def(value, i32::from(default)) != 0)
                .unwrap_or(default)
        };
        let read_string = |key: &str, default: &str| {
            values
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| default.to_owned())
        };
        self.conflict_all = read_bool("ConflictAll", true);
        self.conflict_this = read_bool("ConflictThis", true);
        for (index, value) in ConflictAll::ALL.iter().skip(1).enumerate() {
            self.conflict_all_list.items[index].checked = read_bool(value.enum_name(), true);
        }
        for (index, value) in ConflictThis::ALL.iter().skip(2).enumerate() {
            self.conflict_this_list.items[index].checked = read_bool(value.enum_name(), true);
        }
        self.by_injection_status = read_bool("ByInjectStatus", false);
        self.injected = read_bool("InjectStatus", true);
        self.by_not_reachable_status = self.reachable_build && read_bool("ByNotReachableStatus", false);
        self.not_reachable = read_bool("NotReachableStatus", true);
        self.by_references_injected_status = read_bool("ByReferencesInjectedStatus", false);
        self.references_injected = read_bool("ReferencesInjectedStatus", true);
        self.by_editor_id = read_bool("ByEditorID", false);
        self.editor_id = read_string("EditorID", "");
        self.by_name = read_bool("ByName", false);
        self.name = read_string("Name", "");
        self.by_base_editor_id = read_bool("ByBaseEditorID", false);
        self.base_editor_id = read_string("BaseEditorID", "");
        self.by_base_name = read_bool("ByBaseName", false);
        self.base_name = read_string("BaseName", "");
        self.by_element_value = read_bool("ByElementValue", false);
        self.element_value = read_string("ElementValue", "");
        self.regex_comparison = read_bool("RegexComparison", false);
        self.scaled_actors = read_bool("ScaledActors", false);
        self.record_signature = read_bool("BySignature", false);
        self.record_signatures.set_signatures(&read_string("Signatures", ""));
        self.base_record_signature = read_bool("ByBaseSignature", false);
        self.base_record_signatures
            .set_signatures(&read_string("BaseSignatures", ""));
        self.by_persistent = read_bool("ByPersistent", false);
        self.persistent = read_bool("Persistent", true);
        self.unnecessary_persistent = read_bool("UnnecessaryPersistent", false);
        self.master_is_temporary = read_bool("MasterIsTemporary", false);
        self.is_master = read_bool("IsMaster", false);
        self.persistent_pos_changed = read_bool("PersistentPosChanged", false);
        self.deleted = read_bool("Deleted", false);
        self.by_vwd = read_bool("ByVWD", false);
        self.vwd = read_bool("VWD", true);
        self.by_has_vwd_mesh = read_bool("ByHasVWDMesh", false);
        self.has_vwd_mesh = read_bool("HasVWDMesh", true);
        self.by_has_precombined_mesh = read_bool("ByHasPrecombinedMesh", false);
        self.has_precombined_mesh = read_bool("HasPrecombinedMesh", true);
        self.flatten_blocks = read_bool("FlattenBlocks", false);
        self.flatten_cell_childs = read_bool("FlattenCellChilds", false);
        self.assign_pers_wrld_child = read_bool("AssignPersWrldChild", false);
        self.inherit = read_bool("InheritConflictByParent", true);
        self.preset.text = name.to_owned();
        self.error = None;
    }

    /// `FilterListPresets` over the names of `filter.presets`: the empty
    /// name first.
    pub fn set_presets(&mut self, presets: Vec<String>) {
        self.preset.items = presets;
        if self.preset.items.is_empty() {
            self.preset.items.push(DEFAULTS_PRESET.to_owned());
        }
    }

    /// `btnFilterAddClick`: a new name is added to the combo box (the file
    /// is written only by a save).
    pub fn add_preset(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() || self.preset.items.iter().any(|item| item == name) {
            return;
        }
        self.preset.items.push(name.to_owned());
        self.preset.text = name.to_owned();
    }

    /// The reason a save can not run (a disabled button's tooltip).
    pub fn save_reason(&self) -> Option<&'static str> {
        if !self.commands_available {
            Some("the settings file is not open")
        } else {
            None
        }
    }

    /// The section of the current preset (`Trim(sFilterSection + ' ' + name)`).
    pub fn preset_section(&self) -> String {
        format!("{} {}", FILTER_SECTION, self.preset.text).trim().to_owned()
    }
}

/// Draws a check list with its bulk-action buttons (`mniSelectionClick`).
fn checklist_rows(ui: &mut egui::Ui, id: &str, list: &mut Checklist, height: f32) {
    ui.horizontal(|ui| {
        ui.label(id);
        if ui.small_button("+").on_hover_text("Select All").clicked() {
            list.select_all();
        }
        if ui.small_button("-").on_hover_text("Select None").clicked() {
            list.select_none();
        }
        if ui.small_button("*").on_hover_text("Invert Selection").clicked() {
            list.invert_selection();
        }
    });
    egui::ScrollArea::vertical()
        .id_salt(("checklist", id))
        .max_height(height)
        .show(ui, |ui| {
            for item in &mut list.items {
                ui.checkbox(&mut item.checked, &item.text);
            }
        });
}

/// Draws the dialog.
pub fn show(ctx: &egui::Context, state: &mut FilterOptionsState) -> Option<FilterOptionsAction> {
    let mut action = None;
    let mut open = true;
    let mut window = egui::Window::new("Filter...")
        .id(egui::Id::unique("frmFilterOptions"))
        .collapsible(false)
        .resizable(true)
        .default_size(crate::dialogs::selection::fit_size(ctx, egui::vec2(935.0, 660.0)))
        .open(&mut open);
    window = match state.placement {
        Some(placement) => placement.apply(ctx, window),
        None => window.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)),
    };
    let response = window.show(ctx, |ui| {
        egui::ScrollArea::vertical()
            .id_salt("filter-options")
            .max_height(560.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Preset");
                    let mut combo = state.preset.text.clone();
                    let selected = if combo.is_empty() {
                        "(defaults)".to_owned()
                    } else {
                        combo.clone()
                    };
                    egui::ComboBox::from_id_salt("filter-preset")
                        .width(260.0)
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for item in state.preset.items.clone() {
                                let label = if item.is_empty() {
                                    "(defaults)".to_owned()
                                } else {
                                    item.clone()
                                };
                                ui.selectable_value(&mut combo, item, label);
                            }
                        });
                    if combo != state.preset.text {
                        state.preset.text = combo.clone();
                        action = Some(FilterOptionsAction::LoadPreset(combo));
                    }
                    let mut name = state.preset.text.clone();
                    if ui
                        .add(egui::TextEdit::singleline(&mut name).desired_width(160.0))
                        .changed()
                    {
                        state.preset.text = name;
                    }
                    if ui.button("Add").clicked() {
                        // `InputQuery('Filter', 'Preset name', s)`: the
                        // text box beside the combo is the query's answer.
                        state.add_preset(&state.preset.text.clone());
                    }
                    let can_use = state.commands_available;
                    if ui
                        .add_enabled(can_use, egui::Button::new("Remove"))
                        .on_disabled_hover_text(state.save_reason().unwrap_or_default())
                        .clicked()
                    {
                        if state.preset.text.is_empty() {
                            // `ItemIndex = 0`: the fictitious section
                            // restores the defaults.
                            state.apply_preset_values(DEFAULTS_PRESET, &[]);
                        } else {
                            action = Some(FilterOptionsAction::DeletePreset(state.preset.text.clone()));
                        }
                    }
                    if ui
                        .add_enabled(can_use, egui::Button::new("Save"))
                        .on_disabled_hover_text(state.save_reason().unwrap_or_default())
                        .clicked()
                    {
                        action = Some(FilterOptionsAction::SavePreset(state.preset.text.clone()));
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.checkbox(
                            &mut state.conflict_all,
                            "by conflict status overall (that's the background color)",
                        );
                        checklist_rows(ui, "Conflict status overall", &mut state.conflict_all_list, 130.0);
                    });
                    ui.vertical(|ui| {
                        ui.checkbox(&mut state.conflict_this, "by conflict status for this record");
                        checklist_rows(ui, "Conflict status this", &mut state.conflict_this_list, 130.0);
                    });
                });
                ui.checkbox(&mut state.inherit, "Conflict status inherited by parent");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.by_injection_status, "by injection status");
                    ui.checkbox(&mut state.injected, "only injected");
                });
                ui.horizontal(|ui| {
                    ui.add_enabled(
                        state.reachable_build,
                        egui::Checkbox::new(&mut state.by_not_reachable_status, "by not reachable status"),
                    )
                    .on_disabled_hover_text("Build Reachable Info has not run");
                    ui.checkbox(&mut state.not_reachable, "only not reachable");
                });
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut state.by_references_injected_status,
                        "by references injected status",
                    );
                    ui.checkbox(&mut state.references_injected, "only references injected");
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.by_editor_id, "EditorID contains");
                    ui.add(egui::TextEdit::singleline(&mut state.editor_id).desired_width(160.0));
                    ui.checkbox(&mut state.by_name, "Name contains");
                    ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(160.0));
                });
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut state.by_base_editor_id,
                        "Base Record EditorID contains / or FormID",
                    );
                    ui.add(egui::TextEdit::singleline(&mut state.base_editor_id).desired_width(160.0));
                    ui.checkbox(&mut state.by_base_name, "Base Record Name contains");
                    ui.add(egui::TextEdit::singleline(&mut state.base_name).desired_width(160.0));
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.by_element_value, "Element value contains");
                    ui.add(egui::TextEdit::singleline(&mut state.element_value).desired_width(160.0));
                    ui.checkbox(&mut state.regex_comparison, "Use Regex for Contains Comparison");
                    ui.checkbox(&mut state.scaled_actors, "Scaled Actors");
                });
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.checkbox(&mut state.record_signature, "by Record Signature");
                        checklist_rows(ui, "Record signatures", &mut state.record_signatures, 120.0);
                    });
                    ui.vertical(|ui| {
                        ui.checkbox(&mut state.base_record_signature, "by Base Record Signature");
                        checklist_rows(ui, "Base record signatures", &mut state.base_record_signatures, 120.0);
                    });
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.by_persistent, "by persistence");
                    ui.checkbox(&mut state.persistent, "only persistent");
                    ui.checkbox(
                        &mut state.unnecessary_persistent,
                        "only possible unnecessary persistent",
                    );
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.master_is_temporary, "only if master is not persistent");
                    ui.checkbox(&mut state.is_master, "or if it's the master");
                    ui.checkbox(
                        &mut state.persistent_pos_changed,
                        "only if position/rotation was changed",
                    );
                });
                ui.checkbox(&mut state.deleted, "deleted");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.by_vwd, "by Visible When Distant");
                    ui.checkbox(&mut state.vwd, "only Visible When Distant");
                    ui.checkbox(&mut state.by_has_vwd_mesh, "by Has Visible When Distant Mesh");
                    ui.checkbox(&mut state.has_vwd_mesh, "only Has VWD Mesh");
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.by_has_precombined_mesh, "by Has Precombined Mesh");
                    ui.checkbox(&mut state.has_precombined_mesh, "only Has Precombined Mesh");
                });
                ui.checkbox(&mut state.flatten_blocks, "Flatten Blocks/Sub-Blocks");
                ui.checkbox(&mut state.flatten_cell_childs, "Flatten Cell Children");
                ui.checkbox(
                    &mut state.assign_pers_wrld_child,
                    "Assign Persistent Worldspace Children to Cells",
                );
                if let Some(error) = &state.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Filter").clicked() {
                action = Some(FilterOptionsAction::Commit);
            }
            if ui.button("Cancel").clicked() {
                action = Some(FilterOptionsAction::Cancelled);
            }
        });
    });
    if let Some(response) = response {
        state.rect = Some(response.response.rect);
    }

    // `FormKeyDown`: Escape cancels.
    if action.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        action = Some(FilterOptionsAction::Cancelled);
    }
    // `FormClose`'s question ("The Filter has changed from the preset, do
    // you want to save the changes over the existing preset ...?").
    if let Some(question) = state.question.clone() {
        let text = format!(
            "The Filter has changed from the preset, do you want to save the changes over the existing preset \"{}\"?",
            question.preset
        );
        match show_question(ctx, "filter-preset-changed", &Question::new(text, true)) {
            Some(true) => {
                state.question = None;
                action = Some(FilterOptionsAction::CommitAndSavePreset(question.preset));
            }
            Some(false) => {
                state.question = None;
                action = Some(FilterOptionsAction::CommitWithoutSaving);
            }
            None => return None,
        }
    }
    if !open && action.is_none() {
        action = Some(FilterOptionsAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> SignatureEntry {
        SignatureEntry {
            signature: text.chars().take(4).collect(),
            name: text.chars().skip(7).collect(),
            text: text.to_owned(),
            reference_signature_count: 1,
        }
    }

    fn state() -> FilterOptionsState {
        FilterOptionsState::new(
            &[entry("WEAP - Weapon"), entry("ARMO - Armor")],
            &[entry("NPC_ - Non-Player Character")],
            vec![String::new(), "MyFilter".to_owned()],
            String::new(),
            false,
        )
    }

    #[test]
    fn the_two_conflict_lists_are_the_upstream_entries() {
        let state = state();
        assert_eq!(state.conflict_all_list.items.len(), 6);
        assert_eq!(state.conflict_all_list.items[0].text, "Single Record");
        assert_eq!(state.conflict_all_list.items[5].text, "Critical Conflict");
        assert_eq!(state.conflict_this_list.items.len(), 10);
        assert_eq!(state.conflict_this_list.items[0].text, "Not Defined");
        assert_eq!(state.conflict_this_list.items[9].text, "Conflict loser");
        assert!(state.conflict_all_list.items.iter().all(|item| item.checked));
        assert!(state.conflict_this_list.items.iter().all(|item| item.checked));
    }

    #[test]
    fn the_request_carries_the_checked_values() {
        let mut state = state();
        state.by_editor_id = true;
        state.editor_id = "Player".to_owned();
        state.record_signature = true;
        state.record_signatures.items[1].checked = true;
        state.unnecessary_persistent = true;
        state.conflict_all_list.items[5].checked = false;
        let options = state.to_options();
        assert_eq!(options.by_editor_id.as_deref(), Some("Player"));
        assert_eq!(options.by_signature.as_deref(), Some("ARMO"));
        assert!(options.unnecessary_persistent);
        let all = options.conflict_all.unwrap();
        assert!(!all.contains(&ConflictAllName::caConflictCritical));
        assert!(all.contains(&ConflictAllName::caOnlyOne));
        assert_eq!(options.by_not_reachable_status, None);
        assert!(!options.scripted);
    }

    #[test]
    fn the_not_reachable_checkbox_needs_the_reachable_build() {
        let mut state = state();
        state.by_not_reachable_status = true;
        assert_eq!(state.to_options().by_not_reachable_status, None);
        state.reachable_build = true;
        state.not_reachable = false;
        assert_eq!(state.to_options().by_not_reachable_status, Some(false));
        // `FilterLoadPreset` keeps the box off while the build is missing.
        state.reachable_build = false;
        state.apply_preset_values("X", &[("ByNotReachableStatus".to_owned(), "1".to_owned())]);
        assert!(!state.by_not_reachable_status);
        state.reachable_build = true;
        state.apply_preset_values("X", &[("ByNotReachableStatus".to_owned(), "1".to_owned())]);
        assert!(state.by_not_reachable_status);
    }

    #[test]
    fn a_preset_load_uses_the_defaults_of_an_empty_section() {
        let mut state = state();
        state.by_editor_id = true;
        state.editor_id = "Old".to_owned();
        state.conflict_all_list.items[0].checked = false;
        state.apply_preset_values(DEFAULTS_PRESET, &[]);
        assert!(!state.by_editor_id);
        assert_eq!(state.editor_id, "");
        assert!(state.conflict_all);
        assert!(state.conflict_all_list.items.iter().all(|item| item.checked));
        assert!(state.persistent);
        assert!(state.inherit);
        assert_eq!(state.preset.text, DEFAULTS_PRESET);
    }

    #[test]
    fn a_preset_load_reads_every_written_value() {
        let mut state = state();
        let values: Vec<(String, String)> = [
            ("ConflictAll", "0"),
            ("ctConflictLoses", "0"),
            ("ByEditorID", "1"),
            ("EditorID", "Weapon"),
            ("RegexComparison", "1"),
            ("Signatures", "WEAP,ARMO"),
            ("BySignature", "1"),
            ("BaseSignatures", "NPC_"),
            ("Persistent", "0"),
            ("InheritConflictByParent", "0"),
            ("Deleted", "1"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
        state.apply_preset_values("Loaded", &values);
        assert!(!state.conflict_all);
        // The set stays all-true when `ConflictAll` is off, as upstream
        // reads every enum key with its default.
        assert!(state.conflict_all_list.items.iter().all(|item| item.checked));
        assert!(!state.conflict_this_list.items[9].checked);
        assert!(state.by_editor_id);
        assert_eq!(state.editor_id, "Weapon");
        assert!(state.regex_comparison);
        assert!(state.record_signature);
        assert_eq!(state.record_signatures(), "WEAP,ARMO");
        assert_eq!(state.base_record_signatures(), "NPC_");
        assert!(!state.persistent);
        assert!(!state.inherit);
        assert!(state.deleted);
        assert_eq!(state.preset.text, "Loaded");
    }

    #[test]
    fn the_add_button_appends_a_name_once() {
        let mut state = state();
        state.preset.text = "New".to_owned();
        state.add_preset("New");
        assert_eq!(state.preset.items, vec!["", "MyFilter", "New"]);
        state.add_preset("New");
        assert_eq!(state.preset.items.len(), 3);
        state.add_preset("  ");
        assert_eq!(state.preset.items.len(), 3);
    }

    #[test]
    fn the_preset_section_is_trimmed() {
        let mut state = state();
        state.preset.text = "Mine".to_owned();
        assert_eq!(state.preset_section(), "Filter Mine");
        state.preset.text = String::new();
        assert_eq!(state.preset_section(), "Filter");
    }
}
