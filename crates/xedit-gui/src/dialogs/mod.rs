// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The dialogs of the GUI, one module per upstream form (`xeGameSelectForm`
//! → `game_select_form.rs`, `xeModuleSelectForm` → `module_select_form.rs`).
//!
//! Step 1 of phase 7 brought the two dialogs the startup flow needs. Step 5
//! finishes them and adds the six selection dialogs, which share
//! [`selection`]: the name filter with its regular expression mode, the
//! preset combo box, the check-list and selected-or-all rules, the
//! owner-drawn search, the confirmation questions and the placement values.
//! A dialog module draws through the widgets of `crate::widgets` and
//! returns an action; it never calls a session command itself.
//!
//! A navigation menu item whose upstream handler opens a dialog of a later
//! step calls one of the named placeholders below instead of doing nothing:
//! the item stays reachable, its tooltip says which dialog it would open,
//! and running it logs the same sentence to the message log.

pub mod file_select_form;
pub mod filter_options_form;
pub mod game_select_form;
pub mod mod_group_edit_form;
pub mod mod_group_select_form;
pub mod module_select_form;
pub mod selection;

/// A dialog another phase-7 step owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placeholder {
    /// The dialog, as this module names it.
    pub name: &'static str,
    /// The step that builds the dialog.
    pub step: u8,
    /// The upstream form and what it asks (the tooltip).
    pub what: &'static str,
}

impl Placeholder {
    pub fn tooltip(&self) -> String {
        format!("phase 7 step {}: {}", self.step, self.what)
    }

    pub fn message(&self) -> String {
        format!(
            "{} is not built yet: it arrives in phase 7 step {} ({})",
            self.what, self.step, self.name
        )
    }
}

fn placeholder(name: &'static str, step: u8, what: &'static str) -> Placeholder {
    Placeholder { name, step, what }
}

/// `TfrmFilterOptions` (`xeFilterOptionsForm`), the filter dialog of
/// `mniNavFilterApplyClick`.
pub fn show_filter_options() -> Placeholder {
    placeholder("filter_options", 5, "the filter options dialog (xeFilterOptionsForm)")
}

/// `TfrmFileSelect` (`xeFileSelectForm`), the check list the `* Selected`
/// filter items, Compare to, Copy Idle Animations and the batch reference
/// change open.
pub fn show_file_select(what: &'static str) -> Placeholder {
    placeholder("file_select", 5, what)
}

/// `TfrmModuleSelect` (`xeModuleSelectForm`) as a picker: the copy targets,
/// Add Masters, Build Reference Info, the renumber and inject targets.
pub fn show_module_select(_what: &'static str) -> Placeholder {
    placeholder(
        "module_select",
        5,
        "the module selection dialog (xeModuleSelectForm) used to pick a target",
    )
}

/// The `Copy as ... into....` target selection (`CopyInto` with
/// `TfrmModuleSelect`).
pub fn select_copy_target_file(_item: &'static str) -> Placeholder {
    placeholder(
        "copy_target",
        5,
        "the target file selection dialog (xeModuleSelectForm) of Copy Into",
    )
}

/// The merged patch target (`mniNavCreateMergedPatchClick`).
pub fn select_patch_target_file() -> Placeholder {
    placeholder(
        "patch_target",
        5,
        "the target file selection dialog (xeModuleSelectForm) of Create Merged Patch",
    )
}

/// `InputQuery('New FormID', ...)` of `mniNavChangeFormIDClick` and
/// `mniNavChangeReferencingRecordsClick`.
pub fn prompt_new_form_id() -> Placeholder {
    placeholder("new_form_id", 7, "the New FormID prompt (InputQuery) of Change FormID")
}

/// `TfrmScript` (`xeScriptForm`), the script picker of Apply Script.
pub fn show_script_select() -> Placeholder {
    placeholder("script_select", 6, "the script selection dialog (xeScriptForm)")
}

/// `AddNewFileWithDialog` of Create New File (`mniCreateNewFileClick`).
pub fn show_new_file_dialog() -> Placeholder {
    placeholder("new_file", 7, "the Create New File dialog (AddNewFileWithDialog)")
}

/// `TfrmModGroupEdit` (`xeModGroupEditForm`), Create/Edit ModGroup.
pub fn show_modgroup_editor(_item: &'static str) -> Placeholder {
    placeholder("modgroup_editor", 5, "the mod group editor (xeModGroupEditForm)")
}

/// `TfrmModGroupSelect` (`xeModGroupSelectForm`), Delete ModGroups and the
/// CRC update.
pub fn show_modgroup_select(what: &'static str) -> Placeholder {
    placeholder("modgroup_select", 5, what)
}

/// `TfrmLODGen` (`xeLODGenForm`), Generate LOD.
pub fn show_lodgen() -> Placeholder {
    placeholder("lodgen", 7, "the LOD generation dialog (xeLODGenForm)")
}

/// `TfrmLocalizePlugin` (`xeLocalizePluginForm`), the Localization switch.
pub fn show_localizer() -> Placeholder {
    placeholder(
        "localize_plugin",
        6,
        "the plugin localization dialog (xeLocalizePluginForm)",
    )
}

/// `TfrmLogAnalyzer` (`xeLogAnalyzerForm`).
pub fn show_log_analyzer() -> Placeholder {
    placeholder("log_analyzer", 6, "the log analyzer (xeLogAnalyzerForm)")
}

/// `TfrmOptions` (`xeOptionsForm`), the Options item of `pmuMain` and
/// `pmuNav`.
pub fn show_options() -> Placeholder {
    placeholder("options", 7, "the options dialog (xeOptionsForm)")
}
