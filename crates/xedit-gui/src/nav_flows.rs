// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavCompareToClick,
// mniNavCreateDeltaPatchClick, mniNavCopyIdleClick,
// mniNavCreateMergedPatchClick, mniNavCreateModGroupClick,
// mniNavEditModGroupClick, mniNavDeleteModGroupsClick,
// mniNavAddMastersClick, mniNavApplyScriptClick, mniNavBuildRefClick,
// mniNavBatchChangeReferencingRecordsClick, mniNavChangeFormIDClick,
// mniNavGenerateLODClick, mniNavSetVWDAutoIntoClick,
// mniNavUpdateCRCModGroupsClick, mniNavRenumberFormIDsFromClick,
// mniCreateNewFileClick, AddNewFileWithDialog, AddNewFile and CopyInto)

//! The navigation items that open a dialog before they run their command.
//!
//! Upstream's navigation popup opens a modal dialog for most of its
//! editing items: `TfrmModuleSelect` over the modules, `TfrmFileSelect`
//! over a list of files, the `InputQuery` of the VCL for a single line of
//! text, and the standard open dialog for a file of the disk. The port
//! keeps the same order: the item opens its dialog, the answer decides
//! what the command runs with.
//!
//! This module is the pure model of that order: a [`NavFlow`] names the
//! item, [`NavAsk`] names the dialog one step opens, and [`NavAnswer`]
//! carries what the dialog answered. The shell of `crate::main_form` opens
//! the dialogs, collects the answers and runs the command; nothing here
//! touches the session.

use xedit_session::module_select::ModuleEntry;
use xedit_session::refs::ReferencePair;

/// A navigation item that opens a dialog before its command runs.
#[derive(Clone, Debug, PartialEq)]
pub enum NavFlow {
    /// `mniNavCompareToClick`: the open dialog of the file to compare to,
    /// then the compare load.
    CompareTo {
        /// The file the item acts on.
        file: String,
    },
    /// `mniNavCreateDeltaPatchClick`: the open dialog of the newer
    /// version, then the name of the delta patch, then
    /// `files.create_delta_patch`.
    DeltaPatch {
        /// The file the item acts on.
        file: String,
    },
    /// `mniNavBatchChangeReferencingRecordsClick`: the open dialog of the
    /// CSV file, then `references.change_batch`.
    BatchChange {
        /// The file whose referencing records change.
        file: String,
    },
    /// `mniNavAddMastersClick`: `TfrmModuleSelect` over the modules that
    /// can become masters of the file, then `masters.add`.
    AddMasters {
        /// The file the item acts on.
        file: String,
    },
    /// `mniNavBuildRefClick`: `TfrmModuleSelect` over the files without
    /// reference information, then `refs.build`.
    BuildRef,
    /// `mniNavChangeFormIDClick`: the "New FormID" box, then
    /// `formids.change`.
    ChangeFormId {
        /// The FormID of the record, in hex.
        form_id: String,
        /// The file the record is in.
        file: String,
    },
    /// `mniNavChangeFormIDClick` with a target file: `TfrmModuleSelect`
    /// over the masters of the file and the file itself, then
    /// `formids.change`.
    ChangeFormIdInto {
        /// The FormID of the record, in hex.
        form_id: String,
        /// The file the record is in.
        file: String,
    },
    /// `mniNavChangeReferencingRecordsClick`: the "New FormID" box, then
    /// `references.change`.
    ChangeReferencingRecords {
        /// The FormID of the record, in hex.
        form_id: String,
        /// The file the record is in.
        file: String,
    },
    /// `mniNavRenumberFormIDsFromClick`: the "Start from..." box, then
    /// `formids.renumber`.
    RenumberFrom {
        /// The file whose records are renumbered.
        file: String,
    },
    /// `mniNavRenumberFormIDsInjectClick`: `TfrmModuleSelect` over the
    /// master the records are injected into, then the "Start from..." box,
    /// then `formids.renumber`.
    RenumberInject {
        /// The file whose records are renumbered.
        file: String,
    },
    /// `mniNavSetVWDAutoIntoClick`: `TfrmModuleSelect` over the files the
    /// records are copied into, then `references.copy_vwd_auto`.
    SetVwdInto {
        /// The records that get the flag, with the file of each.
        records: Vec<(String, String)>,
    },
    /// `mniNavUpdateCRCModGroupsClick`: `TfrmModuleSelect` over the
    /// modules whose mod groups need the current CRC, then
    /// `modgroups.update_crcs`.
    UpdateCrc,
    /// `mniNavCreateModGroupClick`: the mod group form, then
    /// `TfrmModuleSelect` over the module the group is stored in, then
    /// `modgroups.create`.
    CreateModGroup {
        /// The name of the new group.
        name: String,
        /// The modules of the group, in the order of the tree.
        modules: Vec<String>,
    },
    /// `mniNavDeleteModGroupsClick`: `TfrmModGroupSelect`, then
    /// `modgroups.delete`.
    DeleteModGroups,
    /// `mniNavCopyIdleClick`: `TfrmFileSelect` over the model prefixes of
    /// the idle records, then the "Model Prefix" box, then
    /// `records.copy_idles`.
    CopyIdle {
        /// The file the idles are read from.
        file: String,
    },
    /// `mniNavCreateMergedPatchClick`: the "New Module File" box, then
    /// `patch.merged`.
    MergedPatch {
        /// The file that is merged.
        file: String,
    },
    /// `mniCreateNewFileClick`: `TfrmModuleSelect` over the templates,
    /// then the "New Module File" box, then `files.new`.
    NewFile,
    /// `mniNavCopyIntoClick` (`CopyInto`): `TfrmModuleSelect` over the
    /// files the records are copied into, then the "EditorID" box, then
    /// `records.copy`.
    CopyInto {
        /// Which copy item was chosen.
        kind: CopyIntoKind,
        /// The records that are copied, with the file of each.
        records: Vec<(String, String)>,
    },
    /// `mniNavFilterApplySelected` and `mniNavFilterConflictsSelected`:
    /// the file picker of the files the filter runs over, then
    /// `filter.apply`.
    FilterApply {
        /// The files the filter runs over; every loaded file when empty.
        files: Vec<String>,
        /// Whether the conflict flags are preset, as
        /// `mniNavFilterConflictsClick` does.
        conflicts: bool,
    },
}

/// The copy items of the navigation popup (`mniNavCopyIntoClick`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyIntoKind {
    /// `mniNavCopyAsOverride`.
    Override,
    /// `mniNavCopyAsOverrideWithOverwrite`.
    OverrideOverwrite,
    /// `mniNavDeepCopyAsOverride`.
    DeepOverride,
    /// `mniNavDeepCopyAsOverrideWithOverwriting`.
    DeepOverrideOverwrite,
    /// `mniNavCopyAsNewRecord`.
    AsNew,
    /// `mniNavCopyAsSpawnRateOverride`.
    AsSpawnRate,
    /// `mniNavCopyAsWrapper`.
    AsWrapper,
}

impl CopyIntoKind {
    /// The `records.copy` request fields this item sets.
    pub fn request(self) -> (bool, bool, bool, bool, bool) {
        // (as_new, deep, overwrite, as_wrapper, as_spawn_rate)
        match self {
            CopyIntoKind::Override => (false, false, false, false, false),
            CopyIntoKind::OverrideOverwrite => (false, false, true, false, false),
            CopyIntoKind::DeepOverride => (false, true, false, false, false),
            CopyIntoKind::DeepOverrideOverwrite => (false, true, true, false, false),
            CopyIntoKind::AsNew => (true, false, false, false, false),
            CopyIntoKind::AsSpawnRate => (false, false, false, false, true),
            CopyIntoKind::AsWrapper => (false, false, false, true, false),
        }
    }

    /// The caption of the module picker upstream opens for this item
    /// (`CopyInto`'s `TfrmModuleSelect`).
    pub fn caption(self, single: bool) -> String {
        match self {
            _ if single => "Which files do you want to add this record to?".to_owned(),
            _ => "Which files do you want to add these records to?".to_owned(),
        }
    }

    /// The menu item of this copy kind.
    pub fn item_id(self) -> &'static str {
        match self {
            CopyIntoKind::Override => "mniNavCopyAsOverride",
            CopyIntoKind::OverrideOverwrite => "mniNavCopyAsOverrideWithOverwrite",
            CopyIntoKind::DeepOverride => "mniNavDeepCopyAsOverride",
            CopyIntoKind::DeepOverrideOverwrite => "mniNavDeepCopyAsOverrideWithOverwriting",
            CopyIntoKind::AsNew => "mniNavCopyAsNewRecord",
            CopyIntoKind::AsSpawnRate => "mniNavCopyAsSpawnRateOverride",
            CopyIntoKind::AsWrapper => "mniNavCopyAsWrapper",
        }
    }
}

/// What the copy flow does when the EditorID answers do not satisfy
/// upstream's rule (`CopyInto`, xeMainForm.pas:3462-3506).
#[derive(Clone, Debug, PartialEq)]
pub enum CopyIntoRepeat {
    /// The warning of an unchanged EditorID is asked (`MessageDlg`).
    Question(String),
    /// A `ShowMessage` of the flow.
    Message(String),
}

/// The text of upstream's warning of an unchanged EditorID
/// (`xeMainForm.pas:3499-3506`).
pub const EDITOR_ID_UNCHANGED_WARNING: &str = "Are you sure you don't want to change the EditorID? \
EditorID conflicts will cause error messages in CS when loading.";

/// The rule of the copy flow: the EditorID box of one record repeats
/// until the answer differs from the original, and the four prefix and
/// suffix boxes of more than one record repeat until a prefix or a suffix
/// to add is given. The answer that keeps the EditorID unchanged is
/// confirmed with a warning; the wrapper copy is told to give a different
/// EditorID or a prefix or a suffix.
///
/// `answers` holds the answers of the EditorID boxes so far.
pub fn copy_into_repeat(
    kind: CopyIntoKind,
    single: bool,
    original_editor_id: &str,
    answers: &[NavAnswer],
) -> Option<CopyIntoRepeat> {
    let text = |position: usize| match answers.get(position) {
        Some(NavAnswer::Text(text)) => text.clone(),
        _ => String::new(),
    };
    match kind {
        CopyIntoKind::AsNew | CopyIntoKind::AsWrapper if single => {
            // The EditorID box (`EditorID`, xeMainForm.pas:3462-3483).
            let Some(NavAnswer::Text(editor_id)) = answers.first() else {
                return None;
            };
            // `if EditorID = '' then Break`: the copy keeps the original
            // EditorID.
            if editor_id.is_empty() {
                return None;
            }
            // `if not SameText(EditorID, MainRecord.EditorID) then Break`.
            if !editor_id.eq_ignore_ascii_case(original_editor_id) {
                return None;
            }
            if kind == CopyIntoKind::AsWrapper {
                Some(CopyIntoRepeat::Message(
                    "You need to specify a different EditorID for the wrapped copy.".to_owned(),
                ))
            } else {
                Some(CopyIntoRepeat::Question(EDITOR_ID_UNCHANGED_WARNING.to_owned()))
            }
        }
        CopyIntoKind::AsNew | CopyIntoKind::AsWrapper => {
            // The four boxes (`xeMainForm.pas:3486-3506`).
            let prefix = text(2);
            let suffix = text(3);
            // `if (EditorIDPrefix <> '') or (EditorIDSuffix <> '') then
            // Break`.
            if !prefix.is_empty() || !suffix.is_empty() {
                return None;
            }
            if kind == CopyIntoKind::AsWrapper {
                Some(CopyIntoRepeat::Message(
                    "You need to specify a prefix or suffix.".to_owned(),
                ))
            } else {
                Some(CopyIntoRepeat::Question(EDITOR_ID_UNCHANGED_WARNING.to_owned()))
            }
        }
        _ => None,
    }
}

/// The dialog one step of a flow opens.
#[derive(Clone, Debug, PartialEq)]
pub enum NavAsk {
    /// The `InputQuery` of the VCL: a caption, a message and one line of
    /// text.
    Text {
        /// The caption of the box.
        caption: String,
        /// The message of the box.
        message: String,
        /// The text the box opens with.
        value: String,
        /// Whether an empty answer is refused.
        require_text: bool,
    },
    /// The standard open dialog of the VCL: a file of the disk.
    Path {
        /// The title of the dialog.
        caption: String,
        /// The message under the title.
        message: String,
        /// The folder the dialog starts in.
        initial_dir: String,
        /// The extension the list offers.
        extension: String,
    },
    /// A list of names the user checks (`TfrmFileSelect`, and the module
    /// picker of the port).
    Files {
        /// The caption of the list.
        caption: String,
        /// The names the list offers.
        items: Vec<String>,
        /// The names that are checked when the list opens.
        checked: Vec<String>,
        /// Whether only one name may be checked.
        single: bool,
    },
    /// The mod group list (`TfrmModGroupSelect`).
    ModGroups {
        /// The caption of the list.
        caption: String,
        /// Whether only one group may be checked.
        single: bool,
    },
    /// A yes/no question (`MessageDlg`).
    Question {
        /// The text of the question.
        message: String,
        /// Whether the answer that is taken when the question is closed is
        /// yes.
        default_yes: bool,
    },
}

/// What a dialog of a flow answered.
#[derive(Clone, Debug, PartialEq)]
pub enum NavAnswer {
    /// The text of the `InputQuery`.
    Text(String),
    /// The path the open dialog chose.
    Path(String),
    /// The checked names of the list.
    Files(Vec<String>),
    /// The checked mod groups.
    ModGroups(Vec<String>),
    /// The answer of a question.
    Yes(bool),
}

/// A navigation item that opens a dialog of the port and no dialog of its
/// own before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavDialog {
    /// `TfrmFilterOptions` (`mniNavFilterApply`).
    FilterOptions,
    /// `TfrmScript` (`mniNavApplyScript`).
    Script,
    /// `TfrmLODGen` (`mniNavGenerateLOD`).
    Lodgen,
    /// `TfrmLogAnalyzerForm` (`mniNavLogAnalyzer`).
    LogAnalyzer,
    /// `TfrmLocalizePlugin` (`mniNavLocalizationSwitch`).
    LocalizePlugin,
    /// `TfrmOptions` (`mniNavOptions`, `mniMainOptions`).
    Options,
    /// `TfrmModGroupSelect` and `TfrmModGroupEdit` (`mniNavEditModGroup`).
    EditModGroup,
}

/// The modules `mniNavAddMastersClick` offers: the valid modules with a
/// file whose load order is below the file's and whose name is not a
/// master of it yet.
pub fn add_master_candidates(modules: &[ModuleEntry], file: &str) -> Vec<String> {
    let Some(target) = modules.iter().find(|module| module.name.eq_ignore_ascii_case(file)) else {
        return Vec::new();
    };
    let target_order = target.load_order.unwrap_or(i32::MAX);
    let masters: Vec<String> = target.masters.iter().map(|name| name.to_lowercase()).collect();
    modules
        .iter()
        .filter(|module| module.valid && module.loaded)
        .filter(|module| module.load_order.unwrap_or(i32::MAX) < target_order)
        .filter(|module| !masters.contains(&module.name.to_lowercase()))
        .map(|module| module.name.clone())
        .collect()
}

/// The modules `mniNavChangeFormIDClick` offers as the target file: the
/// masters of the file and the file itself.
pub fn change_form_id_candidates(modules: &[ModuleEntry], file: &str) -> Vec<String> {
    let Some(target) = modules.iter().find(|module| module.name.eq_ignore_ascii_case(file)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = target.masters.clone();
    names.push(target.name.clone());
    names
}

/// The modules `mniNavBuildRefClick` offers: the loaded files.
pub fn build_ref_candidates(modules: &[ModuleEntry]) -> Vec<String> {
    modules
        .iter()
        .filter(|module| module.valid && module.loaded)
        .map(|module| module.name.clone())
        .collect()
}

/// The modules `mniNavUpdateCRCModGroupsClick` offers: the valid modules.
pub fn update_crc_candidates(modules: &[ModuleEntry]) -> Vec<String> {
    modules
        .iter()
        .filter(|module| module.valid)
        .map(|module| module.name.clone())
        .collect()
}

/// The files `CopyInto` and `mniNavSetVWDAutoIntoClick` offer: the loaded
/// files but the one the records are in.
pub fn copy_target_candidates(modules: &[ModuleEntry], source: &str) -> Vec<String> {
    modules
        .iter()
        .filter(|module| module.valid && module.loaded)
        .filter(|module| !module.name.eq_ignore_ascii_case(source))
        .map(|module| module.name.clone())
        .collect()
}

/// The templates `mniCreateNewFileClick` offers: the kinds of file the
/// port's `files.new` can create.
pub fn file_templates() -> Vec<String> {
    vec![
        "Esp".to_owned(),
        "Esp ESM".to_owned(),
        "Esp Light".to_owned(),
        "Esp ESM Light".to_owned(),
        "Esp Medium".to_owned(),
        "Esp ESM Medium".to_owned(),
        "Esp Update".to_owned(),
        "Esp ESM Update".to_owned(),
        "Esm".to_owned(),
        "Esm Light".to_owned(),
        "Esm Medium".to_owned(),
        "Esm Update".to_owned(),
    ]
}

/// The template of a name of [`file_templates`], in the spelling of the
/// `FileTemplate` enum of `files.new`.
pub fn file_template(name: &str) -> Option<xedit_session::new_file::FileTemplate> {
    use xedit_session::new_file::FileTemplate;
    Some(match name {
        "Esp" => FileTemplate::Esp,
        "Esp ESM" => FileTemplate::EspEsm,
        "Esp Light" => FileTemplate::EspLight,
        "Esp ESM Light" => FileTemplate::EspEsmLight,
        "Esp Medium" => FileTemplate::EspMedium,
        "Esp ESM Medium" => FileTemplate::EspEsmMedium,
        "Esp Update" => FileTemplate::EspUpdate,
        "Esp ESM Update" => FileTemplate::EspEsmUpdate,
        "Esm" => FileTemplate::Esm,
        "Esm Light" => FileTemplate::EsmLight,
        "Esm Medium" => FileTemplate::EsmMedium,
        "Esm Update" => FileTemplate::EsmUpdate,
        _ => return None,
    })
}

/// One line of the CSV of `mniNavBatchChangeReferencingRecordsClick`.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchLine {
    /// The line of the file, counted from the first line of the data.
    pub line: usize,
    /// The signature the CSV names.
    pub signature: String,
    /// The old master file.
    pub old_file: String,
    /// The old FormID, in hex.
    pub old_form_id: String,
    /// The old editor ID.
    pub old_editor_id: String,
    /// The new editor ID.
    pub new_editor_id: String,
    /// The new master file.
    pub new_file: String,
    /// The new FormID, in hex.
    pub new_form_id: String,
}

/// The CSV of the batch reference change: the first line is the header and
/// is ignored, every other line carries seven values.
///
/// The values are the signature, the old master file name, the old FormID,
/// the old editor ID, the new editor ID, the new master file name and the
/// new FormID. A line that does not carry seven values is skipped with
/// the message upstream writes.
pub fn parse_batch_csv(text: &str) -> (Vec<BatchLine>, Vec<String>) {
    let mut lines = Vec::new();
    let mut messages = Vec::new();
    for (index, row) in text.lines().enumerate() {
        // The first line is the header.
        if index == 0 {
            continue;
        }
        if row.trim().is_empty() {
            continue;
        }
        let values: Vec<String> = row.split(',').map(|value| value.trim().to_owned()).collect();
        if values.len() != 7 {
            messages.push(format!(
                "Skipping line {}: 7 values expected but {} found.",
                index + 1,
                values.len()
            ));
            continue;
        }
        lines.push(BatchLine {
            line: index + 1,
            signature: values[0].clone(),
            old_file: values[1].clone(),
            old_form_id: values[2].clone(),
            old_editor_id: values[3].clone(),
            new_editor_id: values[4].clone(),
            new_file: values[5].clone(),
            new_form_id: values[6].clone(),
        });
    }
    (lines, messages)
}

/// The pairs of the batch reference change (`references.change_batch`).
pub fn batch_pairs(lines: &[BatchLine]) -> Vec<ReferencePair> {
    lines
        .iter()
        .map(|line| ReferencePair {
            old_form_id: line.old_form_id.clone(),
            old_file: Some(line.old_file.clone()),
            new_form_id: line.new_form_id.clone(),
            new_file: Some(line.new_file.clone()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_csv_of_the_batch_skips_the_header_line() {
        let text = "Signature,Old Master,Old FormID,Old EditorID,New EditorID,New Master,New FormID\r\n\
                    ALCH,Test.esp,00001234,OldName,NewName,Test.esp,00005678\r\n";
        let (lines, messages) = parse_batch_csv(text);
        assert!(messages.is_empty());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].signature, "ALCH");
        assert_eq!(lines[0].old_file, "Test.esp");
        assert_eq!(lines[0].old_form_id, "00001234");
        assert_eq!(lines[0].new_form_id, "00005678");
        assert_eq!(lines[0].new_editor_id, "NewName");
    }

    #[test]
    fn a_line_with_the_wrong_number_of_values_is_skipped() {
        let text = "header\r\nALCH,Test.esp,00001234\r\n";
        let (lines, messages) = parse_batch_csv(text);
        assert!(lines.is_empty());
        assert_eq!(
            messages,
            vec!["Skipping line 2: 7 values expected but 3 found.".to_owned()]
        );
    }

    #[test]
    fn the_pairs_carry_the_masters_of_the_csv() {
        let text = "header\r\nALCH,Old.esp,00001234,Old,New,New.esp,00005678\r\n";
        let (lines, _) = parse_batch_csv(text);
        let pairs = batch_pairs(&lines);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].old_file.as_deref(), Some("Old.esp"));
        assert_eq!(pairs[0].new_file.as_deref(), Some("New.esp"));
        assert_eq!(pairs[0].new_form_id, "00005678");
    }

    #[test]
    fn the_templates_of_the_new_file_are_the_ones_of_the_command() {
        let templates = file_templates();
        assert_eq!(templates.len(), 12);
        assert!(file_template("Esp ESM").is_some());
        assert!(file_template("Nothing").is_none());
    }

    #[test]
    fn the_copy_kinds_name_their_menu_item() {
        assert_eq!(CopyIntoKind::AsWrapper.item_id(), "mniNavCopyAsWrapper");
        assert_eq!(CopyIntoKind::DeepOverride.item_id(), "mniNavDeepCopyAsOverride");
    }

    #[test]
    fn a_different_editor_id_ends_the_repeat_of_one_record() {
        let answers = [NavAnswer::Text("NewName".to_owned())];
        assert_eq!(copy_into_repeat(CopyIntoKind::AsNew, true, "OldName", &answers), None);
    }

    #[test]
    fn the_unchanged_editor_id_of_one_record_is_confirmed() {
        let answers = [NavAnswer::Text("OldName".to_owned())];
        assert_eq!(
            copy_into_repeat(CopyIntoKind::AsNew, true, "OldName", &answers),
            Some(CopyIntoRepeat::Question(EDITOR_ID_UNCHANGED_WARNING.to_owned()))
        );
    }

    #[test]
    fn the_wrapper_copy_needs_a_different_editor_id() {
        let answers = [NavAnswer::Text("OldName".to_owned())];
        assert_eq!(
            copy_into_repeat(CopyIntoKind::AsWrapper, true, "OldName", &answers),
            Some(CopyIntoRepeat::Message(
                "You need to specify a different EditorID for the wrapped copy.".to_owned()
            ))
        );
    }

    #[test]
    fn a_prefix_or_a_suffix_ends_the_repeat_of_more_records() {
        let answers = [
            NavAnswer::Text(String::new()),
            NavAnswer::Text(String::new()),
            NavAnswer::Text("Pre".to_owned()),
            NavAnswer::Text(String::new()),
        ];
        assert_eq!(copy_into_repeat(CopyIntoKind::AsNew, false, "OldName", &answers), None);
    }

    #[test]
    fn without_a_prefix_or_a_suffix_more_records_are_confirmed() {
        let answers = [
            NavAnswer::Text(String::new()),
            NavAnswer::Text(String::new()),
            NavAnswer::Text(String::new()),
            NavAnswer::Text(String::new()),
        ];
        assert_eq!(
            copy_into_repeat(CopyIntoKind::AsNew, false, "OldName", &answers),
            Some(CopyIntoRepeat::Question(EDITOR_ID_UNCHANGED_WARNING.to_owned()))
        );
    }
}
