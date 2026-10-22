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
//! A navigation menu item whose upstream handler opens a dialog opens that
//! dialog: the item of `crate::nav_flows` names the flow, and the shell
//! runs it. The `InputQuery` of the VCL is `text_prompt.rs`, and the
//! standard open dialog of the three items that ask for a file of the disk
//! is `path_prompt.rs`.
//!
//! Step 6 adds the viewers and editors: the Log Analyzer, the Apply Script
//! dialog, View Elements, the RTF viewer, the localization editor, the
//! developer message, the tip, the script host seam and the Localize plugin
//! dialog. `viewers.rs` holds the split tree and memo viewer and the
//! RTF/text viewer they share.
//!
//! Step 7 adds the settings and modal tools: the options dialog with its six
//! tabs, the LODGen form, the wait form of a long command, the conflict
//! legend, the edit warning, the worldspace cell details form and the FormID
//! prompt. The two prompts answer the core callbacks through `prompts`, and
//! the wait form is the progress seam of `crate::job_runner`.

pub mod developer_message_form;
pub mod edit_warning_form;
pub mod file_select_form;
pub mod filter_options_form;
pub mod form_id_prompt;
pub mod game_select_form;
pub mod legend_form;
pub mod localization_form;
pub mod localize_plugin_form;
pub mod lodgen_form;
pub mod log_analyzer_form;
pub mod mod_group_edit_form;
pub mod mod_group_select_form;
pub mod module_select_form;
pub mod options_form;
pub mod path_prompt;
pub mod prompts;
pub mod rich_edit_form;
pub mod script_form;
pub mod script_host;
pub mod selection;
pub mod text_prompt;
pub mod tip_form;
pub mod view_elements_form;
pub mod viewers;
pub mod wait_form;
pub mod worldspace_cell_details_form;

use serde_json::Value;

use xedit_session::settings::{SettingsGetRequest, SettingsGetResponse, SettingsSetRequest};

use crate::command::CommandBridge;

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
