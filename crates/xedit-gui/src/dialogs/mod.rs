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
//!
//! Step 6 adds the viewers and editors: the Log Analyzer, the Apply Script
//! dialog, View Elements, the RTF viewer, the localization editor, the
//! developer message, the tip, the script host seam and the Localize plugin
//! dialog. `viewers.rs` holds the split tree and memo viewer and the
//! RTF/text viewer they share.

pub mod developer_message_form;
pub mod file_select_form;
pub mod filter_options_form;
pub mod game_select_form;
pub mod localization_form;
pub mod localize_plugin_form;
pub mod log_analyzer_form;
pub mod mod_group_edit_form;
pub mod mod_group_select_form;
pub mod module_select_form;
pub mod rich_edit_form;
pub mod script_form;
pub mod script_host;
pub mod selection;
pub mod tip_form;
pub mod view_elements_form;
pub mod viewers;

use serde_json::Value;

use xedit_session::settings::{SettingsGetRequest, SettingsGetResponse, SettingsSetRequest};

use crate::command::CommandBridge;

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

/// The dialogs `--dialog <name>` and the menu commands open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    LogAnalyzer,
    Script,
    ViewElements,
    RichEdit,
    Localization,
    DeveloperMessage,
    Tip,
    LocalizePlugin,
}

impl DialogKind {
    /// The `--dialog` name of a dialog.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "log-analyzer" => Some(DialogKind::LogAnalyzer),
            "script" => Some(DialogKind::Script),
            "view-elements" => Some(DialogKind::ViewElements),
            "rich-edit" | "whats-new" => Some(DialogKind::RichEdit),
            "localization" => Some(DialogKind::Localization),
            "developer-message" => Some(DialogKind::DeveloperMessage),
            "tip" => Some(DialogKind::Tip),
            "localize-plugin" => Some(DialogKind::LocalizePlugin),
            _ => None,
        }
    }

    /// The names `--dialog` takes, for the usage text.
    pub const NAMES: [&'static str; 8] = [
        "log-analyzer",
        "script",
        "view-elements",
        "rich-edit",
        "localization",
        "developer-message",
        "tip",
        "localize-plugin",
    ];
}

/// The open dialogs of the shell.
#[derive(Default)]
pub struct DialogHost {
    pub log_analyzer: Option<log_analyzer_form::LogAnalyzerState>,
    pub script: Option<script_form::ScriptFormState>,
    pub view_elements: Option<view_elements_form::ViewElementsState>,
    pub rich_edit: Option<rich_edit_form::RichEditState>,
    pub localization: Option<localization_form::LocalizationState>,
    pub developer_message: Option<developer_message_form::DeveloperMessageState>,
    pub tip: Option<tip_form::TipState>,
    pub localize_plugin: Option<localize_plugin_form::LocalizePluginState>,
}

impl DialogHost {
    pub fn is_open(&self, kind: DialogKind) -> bool {
        match kind {
            DialogKind::LogAnalyzer => self.log_analyzer.is_some(),
            DialogKind::Script => self.script.is_some(),
            DialogKind::ViewElements => self.view_elements.is_some(),
            DialogKind::RichEdit => self.rich_edit.is_some(),
            DialogKind::Localization => self.localization.is_some(),
            DialogKind::DeveloperMessage => self.developer_message.is_some(),
            DialogKind::Tip => self.tip.is_some(),
            DialogKind::LocalizePlugin => self.localize_plugin.is_some(),
        }
    }

    /// Whether any dialog is open.
    pub fn any_open(&self) -> bool {
        [
            DialogKind::LogAnalyzer,
            DialogKind::Script,
            DialogKind::ViewElements,
            DialogKind::RichEdit,
            DialogKind::Localization,
            DialogKind::DeveloperMessage,
            DialogKind::Tip,
            DialogKind::LocalizePlugin,
        ]
        .into_iter()
        .any(|kind| self.is_open(kind))
    }

    pub fn close(&mut self, kind: DialogKind) {
        match kind {
            DialogKind::LogAnalyzer => self.log_analyzer = None,
            DialogKind::Script => self.script = None,
            DialogKind::ViewElements => self.view_elements = None,
            DialogKind::RichEdit => self.rich_edit = None,
            DialogKind::Localization => self.localization = None,
            DialogKind::DeveloperMessage => self.developer_message = None,
            DialogKind::Tip => self.tip = None,
            DialogKind::LocalizePlugin => self.localize_plugin = None,
        }
    }
}

/// `settings.get` one value as text: `None` when the command fails (a
/// missing settings file reads as the default).
pub fn setting_get(
    bridge: &mut CommandBridge,
    section: &str,
    key: &str,
    value_type: &str,
    default: Value,
) -> Option<String> {
    let request = SettingsGetRequest {
        section: Some(section.to_owned()),
        key: Some(key.to_owned()),
        value_type: Some(value_type.to_owned()),
        default: Some(default),
    };
    let response: SettingsGetResponse = bridge.request("settings.get", &request).ok()?;
    response.values.first().map(|entry| entry.raw.clone())
}

/// `settings.set` one value with the `TMemIniFile` type the GUI writes it
/// with (`integer`, `bool`, `string`).
pub fn setting_set(bridge: &mut CommandBridge, section: &str, key: &str, value: Value, value_type: &str) -> bool {
    let request = SettingsSetRequest {
        section: section.to_owned(),
        key: key.to_owned(),
        value,
        value_type: Some(value_type.to_owned()),
        dry_run: false,
    };
    bridge.request::<_, serde_json::Value>("settings.set", &request).is_ok()
}

/// `TMemIniFile.ReadBool` of a setting that may be missing.
pub fn setting_get_bool(bridge: &mut CommandBridge, section: &str, key: &str, default: bool) -> bool {
    setting_get(bridge, section, key, "bool", Value::Bool(default))
        .map(|raw| matches!(raw.trim().to_ascii_lowercase().as_str(), "1" | "true"))
        .unwrap_or(default)
}

/// `TMemIniFile.ReadInteger` of a setting that may be missing.
pub fn setting_get_integer(bridge: &mut CommandBridge, section: &str, key: &str, default: i64) -> i64 {
    setting_get(bridge, section, key, "integer", Value::from(default))
        .and_then(|raw| raw.trim().parse().ok())
        .unwrap_or(default)
}

/// `TMemIniFile.ReadString` of a setting that may be missing.
pub fn setting_get_string(bridge: &mut CommandBridge, section: &str, key: &str, default: &str) -> String {
    setting_get(bridge, section, key, "string", Value::String(default.to_owned())).unwrap_or_else(|| default.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_names_resolve() {
        assert_eq!(DialogKind::from_name("log-analyzer"), Some(DialogKind::LogAnalyzer));
        assert_eq!(DialogKind::from_name("whats-new"), Some(DialogKind::RichEdit));
        assert_eq!(DialogKind::from_name("nope"), None);
        assert_eq!(DialogKind::NAMES.len(), 8);
    }

    #[test]
    fn the_host_opens_and_closes_each_dialog() {
        let mut host = DialogHost::default();
        assert!(!host.is_open(DialogKind::Tip));
        host.tip = Some(tip_form::TipState {
            tips: vec!["a".to_owned()],
            current: "a".to_owned(),
            show_tip: true,
            seed: 0,
            shown: 0,
        });
        assert!(host.is_open(DialogKind::Tip));
        host.close(DialogKind::Tip);
        assert!(!host.is_open(DialogKind::Tip));
    }
}
