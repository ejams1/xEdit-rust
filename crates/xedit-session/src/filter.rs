// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeFilterOptionsForm.pas (FormCreate,
// FilterListPresets, FilterLoadPreset, FilterSavePreset,
// FilterHasPresetChanged, btnFilterDelClick, mniSelectionClick,
// cmbPresetSelect, the RecordSignatures and BaseRecordSignatures properties)
// and xEdit/xeMainForm.pas (mniNavFilterApplyClick, mniNavFilterRemoveClick,
// ReInitTree)

//! `filter.apply`, `filter.remove` and the filter presets of the options
//! dialog (`TfrmFilterOptions`).
//!
//! The dialog of the GUI becomes parameters: [`FilterRequestOptions`] holds
//! the checkboxes and texts of `TfrmFilterOptions`, and a preset of xEdit's
//! settings file (`[Filter <name>]`) holds the same values for a name
//! (`FilterLoadPreset` with `FilterSavePreset`). `filter.apply` builds the
//! navigation tree of the loaded files (or of the files the request names,
//! which the GUI's "* Selected" items pick in the file selection dialog),
//! runs the filter of `xedit_analysis::filter` and reports the counts the
//! GUI writes to its message log: the two passes, the nodes left, and per
//! file the records the filter took out.
//!
//! The checklist boxes of the dialog that hold the conflict statuses
//! (`clbConflictAll`, `clbConflictThis`) and the signature lists
//! (`clbRecordSignatures`, `clbBaseRecordSignatures`) are sets in the
//! request; the dialog's own buttons (select all, none, invert; add, delete
//! and save a preset) are the requests that carry the values, and
//! `mniSelectionClick` and `btnFilterAddClick` are presentation only.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_analysis::filter::{FilterOptions, FilterReport, NavTree, apply_filter_with};
use xedit_core::implementation::ElementImpl;
use xedit_core::interface::PascalEnum;
use xedit_core::interface::globals::{reachable_build, set_reachable_build};
use xedit_core::interface::types::{ConflictAll, ConflictThis};
use xedit_core::interface::{Element, File};
use xedit_loadorder::ini_files::{MemIniFile, same_text};

use crate::conflicts::{ConflictAllName, ConflictThisName};
use crate::{CommandError, Registry, Session};

/// The section of a filter preset (`sFilterSection`).
const FILTER_SECTION: &str = "Filter";

fn io_error(error: std::io::Error, path: &Path) -> CommandError {
    CommandError::new("io", format!("{}: {error}", path.display()))
}

/// The options of the dialog as the request gives them: `None` is a
/// checkbox that is off, `Some` one that is on with that value.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct FilterRequestOptions {
    /// `cbConflictAll` with `clbConflictAll`: keep the records whose
    /// `ConflictAll` is one of these (`caUnknown` and the rest).
    pub conflict_all: Option<Vec<ConflictAllName>>,
    /// `cbConflictThis` with `clbConflictThis`.
    pub conflict_this: Option<Vec<ConflictThisName>>,
    /// `cbByInjectionStatus` with `cbInjected`.
    pub by_inject_status: Option<bool>,
    /// `cbByNotReachableStatus` with `cbNotReachable`, which needs
    /// "Build Reachable Info" (`refs.build_reachable`).
    pub by_not_reachable_status: Option<bool>,
    /// `cbByReferencesInjectedStatus` with `cbReferencesInjected`.
    pub by_references_injected_status: Option<bool>,
    /// `cbByEditorID` with `edEditorID`.
    pub by_editor_id: Option<String>,
    /// `cbByElementValue` with `edElementValue`.
    pub by_element_value: Option<String>,
    /// `cbByName` with `edName`.
    pub by_name: Option<String>,
    /// `cbByBaseEditorID` with `edBaseEditorID`; eight or nine characters
    /// are read as the FormID of the base record.
    pub by_base_editor_id: Option<String>,
    /// `cbByBaseName` with `edBaseName`.
    pub by_base_name: Option<String>,
    /// `cbScaledActors`.
    #[serde(default)]
    pub scaled_actors: bool,
    /// `cbRecordSignature` with `RecordSignatures` (a comma separated list
    /// of four character signatures).
    pub by_signature: Option<String>,
    /// `cbBaseRecordSignature` with `BaseRecordSignatures`.
    pub by_base_signature: Option<String>,
    /// `cbByPersistent` with `cbPersistent`.
    #[serde(default)]
    pub by_persistent: bool,
    #[serde(default)]
    pub persistent: bool,
    /// `cbUnnecessaryPersistent`.
    #[serde(default)]
    pub unnecessary_persistent: bool,
    /// `cbMasterIsTemporary` with `cbIsMaster`.
    #[serde(default)]
    pub master_is_temporary: bool,
    #[serde(default)]
    pub is_master: bool,
    /// `cbPersistentPosChanged`.
    #[serde(default)]
    pub persistent_pos_changed: bool,
    /// `cbDeleted`.
    #[serde(default)]
    pub deleted: bool,
    /// `cbByVWD` with `cbVWD`.
    pub by_vwd: Option<bool>,
    /// `cbByHasVWDMesh` with `cbHasVWDMesh`.
    pub by_has_vwd_mesh: Option<bool>,
    /// `cbByHasPrecombinedMesh` with `cbHasPrecombinedMesh`.
    pub by_has_precombined_mesh: Option<bool>,
    /// `cbRegexComparison`: the texts are regular expressions.
    #[serde(default)]
    pub regex_comparison: bool,
    /// `cbFlattenBlocks`.
    #[serde(default)]
    pub flatten_blocks: bool,
    /// `cbFlattenCellChilds`.
    #[serde(default)]
    pub flatten_cell_childs: bool,
    /// `cbAssignPersWrldChild`.
    #[serde(default)]
    pub assign_pers_wrld_child: bool,
    /// `cbInherit`: "conflict status inherited by parent".
    #[serde(default)]
    pub inherit_conflict_by_parent: bool,
}

impl FilterRequestOptions {
    pub(crate) fn to_options(&self) -> FilterOptions {
        FilterOptions {
            conflict_all: self
                .conflict_all
                .as_ref()
                .map(|set| set.iter().copied().map(ConflictAllName::into).collect()),
            conflict_this: self
                .conflict_this
                .as_ref()
                .map(|set| set.iter().copied().map(ConflictThisName::into).collect()),
            by_inject_status: self.by_inject_status,
            by_not_reachable_status: self.by_not_reachable_status,
            by_references_injected_status: self.by_references_injected_status,
            by_editor_id: self.by_editor_id.clone(),
            by_element_value: self.by_element_value.clone(),
            by_name: self.by_name.clone(),
            by_base_editor_id: self.by_base_editor_id.clone(),
            by_base_name: self.by_base_name.clone(),
            scaled_actors: self.scaled_actors,
            by_signature: self.by_signature.clone(),
            by_base_signature: self.by_base_signature.clone(),
            by_persistent: self.by_persistent,
            persistent: self.persistent,
            unnecessary_persistent: self.unnecessary_persistent,
            master_is_temporary: self.master_is_temporary,
            is_master: self.is_master,
            persistent_pos_changed: self.persistent_pos_changed,
            deleted: self.deleted,
            by_vwd: self.by_vwd,
            by_has_vwd_mesh: self.by_has_vwd_mesh,
            by_has_precombined_mesh: self.by_has_precombined_mesh,
            regex_comparison: self.regex_comparison,
            flatten_blocks: self.flatten_blocks,
            flatten_cell_childs: self.flatten_cell_childs,
            assign_pers_wrld_child: self.assign_pers_wrld_child,
            inherit_conflict_by_parent: self.inherit_conflict_by_parent,
            ..Default::default()
        }
    }

    pub(crate) fn from_options(options: &FilterOptions) -> FilterRequestOptions {
        FilterRequestOptions {
            conflict_all: options
                .conflict_all
                .as_ref()
                .map(|set| set.iter().copied().map(ConflictAllName::from).collect()),
            conflict_this: options
                .conflict_this
                .as_ref()
                .map(|set| set.iter().copied().map(ConflictThisName::from).collect()),
            by_inject_status: options.by_inject_status,
            by_not_reachable_status: options.by_not_reachable_status,
            by_references_injected_status: options.by_references_injected_status,
            by_editor_id: options.by_editor_id.clone(),
            by_element_value: options.by_element_value.clone(),
            by_name: options.by_name.clone(),
            by_base_editor_id: options.by_base_editor_id.clone(),
            by_base_name: options.by_base_name.clone(),
            scaled_actors: options.scaled_actors,
            by_signature: options.by_signature.clone(),
            by_base_signature: options.by_base_signature.clone(),
            by_persistent: options.by_persistent,
            persistent: options.persistent,
            unnecessary_persistent: options.unnecessary_persistent,
            master_is_temporary: options.master_is_temporary,
            is_master: options.is_master,
            persistent_pos_changed: options.persistent_pos_changed,
            deleted: options.deleted,
            by_vwd: options.by_vwd,
            by_has_vwd_mesh: options.by_has_vwd_mesh,
            by_has_precombined_mesh: options.by_has_precombined_mesh,
            regex_comparison: options.regex_comparison,
            flatten_blocks: options.flatten_blocks,
            flatten_cell_childs: options.flatten_cell_childs,
            assign_pers_wrld_child: options.assign_pers_wrld_child,
            inherit_conflict_by_parent: options.inherit_conflict_by_parent,
        }
    }
}

/// `filter.apply`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterApplyRequest {
    /// The files of the tree in load order; every loaded file when omitted
    /// (the file selection dialog of the "* Selected" menu items).
    #[serde(default)]
    pub files: Vec<String>,
    /// The options of the filter dialog, which the request gives as the
    /// dialog's checkboxes hold them. When it is given it is the whole
    /// filter; `preset` is read only without it.
    pub options: Option<FilterRequestOptions>,
    /// The name of the preset to read the options from (`FilterLoadPreset`
    /// with a `[Filter <name>]` section of xEdit's settings file).
    pub preset: Option<String>,
    /// Activate the valid mod groups of these names, so the records they
    /// hide are left out of the comparison (`ModGroupsEnabled`).
    #[serde(default)]
    pub mod_groups: Vec<String>,
    /// Activate every valid mod group.
    #[serde(default)]
    pub all_mod_groups: bool,
    /// Activate the valid mod groups of the selection saved in xEdit's
    /// settings file.
    #[serde(default)]
    pub saved_mod_groups: bool,
    /// List the records left per file instead of the counts only.
    #[serde(default)]
    pub list_records: bool,
    /// `FilterConflictOnly`: the mode of `mniNavFilterConflictsClick` with
    /// `xeVeryQuickShowConflicts`, which the dialog has no checkbox for.
    #[serde(default)]
    pub conflict_only: bool,
    /// `FilterOnlyOne`: `mniNavFilterForOnlyOneClick`.
    #[serde(default)]
    pub only_one: bool,
    /// `FilterNoGameMaster` of `ReInitTree`, which the `xeVeryQuickShowConflicts`
    /// mode sets.
    #[serde(default)]
    pub no_game_master: bool,
}

/// One file of the response.
#[derive(Serialize, JsonSchema)]
pub struct FilterFile {
    pub name: String,
    pub load_order: i32,
    /// `RecordCount` of the file.
    pub records: i64,
    /// The records the filter took out of the file (`FileFiltered`).
    pub filtered: i64,
    /// Whether the GUI writes its `[name] Filtered n of m records` line for
    /// the file.
    pub logged: bool,
}

/// `filter.apply`: the response.
#[derive(Serialize, JsonSchema)]
pub struct FilterApplyResponse {
    /// The options the filter ran with, after the sets and the signatures
    /// were resolved (`FilterPreset` after the dialog).
    pub options: FilterRequestOptions,
    /// The global `ReachableBuild`: without it `by_not_reachable_status`
    /// has no effect, as in the GUI, whose checkbox the state disables.
    pub reachable_build: bool,
    /// `[Pass 1] Processed Records`.
    pub pass1: u64,
    /// `[Pass 2] Processed Records`.
    pub pass2: u64,
    /// `Remaining unfiltered nodes`.
    pub unfiltered: u64,
    /// The files of the tree, in load order, with the records the filter
    /// took out.
    pub files: Vec<FilterFile>,
    /// The lines xEdit writes to its message log, without the times: the
    /// warnings of the comparison and the `[file] Filtered n of m records`
    /// lines.
    pub messages: Vec<String>,
    /// The records the tree still shows, per file, when `list_records` was
    /// asked for: the record names in the order of the tree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub records: Option<Vec<String>>,
    /// The activated mod groups.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mod_groups: Vec<String>,
}

fn default_options() -> FilterRequestOptions {
    // `TfrmFilterOptions` starts from `FilterLoadPreset`'s defaults, which
    // is why "conflict status inherited by parent" is on for a filter
    // without options.
    FilterRequestOptions {
        inherit_conflict_by_parent: true,
        ..Default::default()
    }
}

/// The files of the request in load order: the names must be loaded.
fn request_files(
    session: &Session,
    names: &[String],
) -> Result<Vec<Arc<xedit_core::implementation::FileImpl>>, CommandError> {
    let files = crate::conflicts::session_files(session)?;
    if names.is_empty() {
        return Ok(files);
    }
    let mut selected = Vec::new();
    for name in names {
        let file = files
            .iter()
            .find(|file| file.get_name().eq_ignore_ascii_case(name))
            .ok_or_else(|| CommandError::new("unknown_file", format!("{name} is not loaded")))?;
        selected.push(file.clone());
    }
    Ok(selected)
}

/// The name of every loaded file in the order of the tree.
fn tree_files(tree: &NavTree) -> Vec<(String, i32, i64)> {
    tree.roots()
        .into_iter()
        .filter_map(|node| {
            let file = tree
                .data(node)
                .element
                .as_ref()
                .and_then(|element| element.as_element_impl()?.file_impl())?;
            Some((file.get_name(), file.load_order(), i64::from(file.get_record_count())))
        })
        .collect()
}

fn filter_apply(session: &mut Session, request: FilterApplyRequest) -> Result<FilterApplyResponse, CommandError> {
    session.mode()?;
    let files = request_files(session, &request.files)?;
    // `FilterPreset`: the dialog does not open, the options are the request's
    // (or the preset's, as `FilterLoadPreset` fills the dialog).
    let mut options = match (&request.options, &request.preset) {
        (Some(options), _) => options.to_options(),
        (None, Some(name)) => load_preset(name)?,
        (None, None) => default_options().to_options(),
    };
    options.reachable_build = reachable_build();
    // `if FilterByPersistent and FilterPersistent and FilterUnnecessaryPersistent
    // then BuildAllRef`: the reference information the unnecessary-persistent
    // and the references-injected options read, which the GUI's load built.
    if (options.by_persistent && options.persistent && options.unnecessary_persistent)
        || options.by_references_injected_status.is_some()
    {
        session.ensure_refs()?;
    }
    options.conflict_only = request.conflict_only;
    options.only_one = request.only_one;
    // `ReInitTree(FilterNoGameMaster or wbTranslationMode, ...)`: the
    // translate mode leaves the game master out of the tree too.
    options.no_game_master = request.no_game_master || xedit_core::interface::globals::translation_mode();
    let active = crate::modgroups::activate(
        session,
        &crate::modgroups::ModGroupChoice {
            mod_groups: request.mod_groups.clone(),
            all_mod_groups: request.all_mod_groups,
            saved_mod_groups: request.saved_mod_groups,
        },
    )?;
    options.mod_groups_enabled = active.filter.is_some();

    // `ReInitTree(FilterNoGameMaster or wbTranslationMode, FilterFiles)`:
    // the tree holds the files the request names, without the game master
    // when the very quick conflicts mode asked for it.
    let tree_files: Vec<Arc<xedit_core::implementation::FileImpl>> = if options.no_game_master {
        files
            .iter()
            .filter(|file| {
                !file
                    .get_file_states()
                    .contains(xedit_core::interface::FileState::fsIsGameMaster)
            })
            .cloned()
            .collect()
    } else {
        files.clone()
    };
    let mut tree = NavTree::new(&tree_files);
    let mut report = apply_filter_with(&mut tree, &options, &files, active.filter.clone()).map_err(edit_failed)?;
    report.files.retain(|file| {
        tree_files
            .iter()
            .any(|kept| kept.get_name().eq_ignore_ascii_case(&file.name))
    });
    Ok(response_of(&tree, &options, request.list_records, report, &active))
}

fn response_of(
    tree: &NavTree,
    options: &FilterOptions,
    list_records: bool,
    report: FilterReport,
    active: &crate::modgroups::ActiveModGroups,
) -> FilterApplyResponse {
    let records = list_records.then(|| {
        tree.roots()
            .into_iter()
            .flat_map(|root| xedit_analysis::filter::records_below(tree, root))
            .map(|record| record.get_name())
            .collect()
    });
    let mut messages = report.messages.clone();
    messages.extend(report.file_lines());
    FilterApplyResponse {
        options: FilterRequestOptions::from_options(options),
        reachable_build: reachable_build(),
        pass1: report.pass1,
        pass2: report.pass2,
        unfiltered: report.unfiltered,
        files: report
            .files
            .iter()
            .map(|file| FilterFile {
                name: file.name.clone(),
                load_order: file.load_order,
                records: file.records,
                filtered: file.filtered,
                logged: file.is_logged(),
            })
            .collect(),
        messages,
        records,
        mod_groups: active.names.clone(),
    }
}

fn edit_failed(message: String) -> CommandError {
    CommandError::new("edit_failed", message)
}

/// `filter.remove`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterRemoveRequest {}

/// `filter.remove`: the response.
#[derive(Serialize, JsonSchema)]
pub struct FilterRemoveResponse {
    /// The tree without a filter: one node per loaded file (`ReInitTree`).
    pub files: Vec<FilterFile>,
    /// `FilterApplied` after the call.
    pub applied: bool,
    /// `ReachableBuild`, which the not-reachable filter option reads.
    pub reachable_build: bool,
}

/// Port of `mniNavFilterRemoveClick`: `ReInitTree(False, nil)` and no
/// filter. The port's tree is built on demand, so what is left is the list
/// of the loaded files, which is what the GUI's tree shows after the call.
fn filter_remove(session: &mut Session, _: FilterRemoveRequest) -> Result<FilterRemoveResponse, CommandError> {
    session.mode()?;
    let files = crate::conflicts::session_files(session)?;
    let tree = NavTree::new(&files);
    Ok(FilterRemoveResponse {
        files: tree_files(&tree)
            .into_iter()
            .map(|(name, load_order, records)| FilterFile {
                name,
                load_order,
                records,
                filtered: 0,
                logged: false,
            })
            .collect(),
        applied: false,
        reachable_build: reachable_build(),
    })
}

/// `filter.presets`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterPresetsRequest {}

/// `filter.presets`: the response.
#[derive(Serialize, JsonSchema)]
pub struct FilterPresetsResponse {
    /// The names of the `[Filter <name>]` sections, sorted, with the empty
    /// name first (`FilterListPresets`).
    pub presets: Vec<String>,
    /// The preset `View|LastUsedFilter` names (the dialog's first entry).
    pub last_used: String,
    /// The settings file the names come from (`xeSettingsFileName`).
    pub settings: String,
}

fn filter_presets(session: &mut Session, _: FilterPresetsRequest) -> Result<FilterPresetsResponse, CommandError> {
    session.mode()?;
    let path = crate::modgroups::settings_file();
    let ini = MemIniFile::open(&path).map_err(|error| io_error(error, &path))?;
    Ok(FilterPresetsResponse {
        presets: list_presets(&ini),
        last_used: ini.read_string("View", "LastUsedFilter", ""),
        settings: path.display().to_string(),
    })
}

/// Port of `FilterListPresets`: the names of the `[Filter <name>]` sections
/// of the settings file, sorted, the empty name first.
fn list_presets(ini: &MemIniFile) -> Vec<String> {
    let mut names: Vec<String> = ini
        .read_sections()
        .into_iter()
        .filter(|section| {
            section
                .to_ascii_lowercase()
                .starts_with(&FILTER_SECTION.to_ascii_lowercase())
        })
        .map(|section| section[FILTER_SECTION.len()..].trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect();
    names.sort_by_key(|name| name.to_ascii_uppercase());
    names.dedup();
    names.insert(0, String::new());
    names
}

/// Port of `FilterLoadPreset`.
fn load_preset(name: &str) -> Result<FilterOptions, CommandError> {
    let path = crate::modgroups::settings_file();
    let ini = MemIniFile::open(&path).map_err(|error| io_error(error, &path))?;
    Ok(read_preset(&ini, name))
}

/// Port of `FilterLoadPreset` over an open settings file.
fn read_preset(ini: &MemIniFile, name: &str) -> FilterOptions {
    let section = format!("{} {}", FILTER_SECTION, name).trim().to_owned();
    let read_bool = |key: &str, default: bool| ini.read_bool(&section, key, default);
    let read_string = |key: &str, default: &str| ini.read_string(&section, key, default);
    let conflict_all = xedit_core::interface::types::ConflictAll::ALL
        .iter()
        .copied()
        .filter(|value| read_bool(&pascal_enum_name(*value), true))
        .collect::<Vec<ConflictAll>>();
    let conflict_this = xedit_core::interface::types::ConflictThis::ALL
        .iter()
        .copied()
        .filter(|value| read_bool(&pascal_enum_name(*value), true))
        .collect::<Vec<ConflictThis>>();
    FilterOptions {
        conflict_all: read_bool("ConflictAll", true).then_some(conflict_all),
        conflict_this: read_bool("ConflictThis", true).then_some(conflict_this),
        by_inject_status: read_bool("byInjectStatus", false).then(|| read_bool("InjectStatus", true)),
        by_not_reachable_status: read_bool("byNotReachableStatus", false)
            .then(|| read_bool("NotReachableStatus", true)),
        by_references_injected_status: read_bool("byReferencesInjectedStatus", false)
            .then(|| read_bool("ReferencesInjectedStatus", true)),
        by_editor_id: read_bool("ByEditorID", false).then(|| read_string("EditorID", "")),
        by_name: read_bool("ByName", false).then(|| read_string("Name", "")),
        by_base_editor_id: read_bool("ByBaseEditorID", false).then(|| read_string("BaseEditorID", "")),
        by_base_name: read_bool("ByBaseName", false).then(|| read_string("BaseName", "")),
        by_element_value: read_bool("ByElementValue", false).then(|| read_string("ElementValue", "")),
        regex_comparison: read_bool("RegexComparison", false),
        scaled_actors: read_bool("ScaledActors", false),
        by_signature: read_bool("BySignature", false).then(|| read_string("Signatures", "")),
        by_base_signature: read_bool("ByBaseSignature", false).then(|| read_string("BaseSignatures", "")),
        by_persistent: read_bool("ByPersistent", false),
        persistent: read_bool("Persistent", true),
        unnecessary_persistent: read_bool("UnnecessaryPersistent", false),
        master_is_temporary: read_bool("MasterIsTemporary", false),
        is_master: read_bool("IsMaster", false),
        persistent_pos_changed: read_bool("PersistentPosChanged", false),
        deleted: read_bool("Deleted", false),
        by_vwd: read_bool("ByVWD", false).then(|| read_bool("VWD", true)),
        by_has_vwd_mesh: read_bool("ByHasVWDMesh", false).then(|| read_bool("HasVWDMesh", true)),
        by_has_precombined_mesh: read_bool("ByHasPrecombinedMesh", false)
            .then(|| read_bool("HasPrecombinedMesh", true)),
        flatten_blocks: read_bool("FlattenBlocks", false),
        flatten_cell_childs: read_bool("FlattenCellChilds", false),
        assign_pers_wrld_child: read_bool("AssignPersWrldChild", false),
        inherit_conflict_by_parent: read_bool("InheritConflictByParent", true),
        ..Default::default()
    }
}

/// The name `GetEnumName(TypeInfo(T), Ord(T))` gives a Pascal enum value:
/// the value of the enum, as the settings file holds it.
fn pascal_enum_name<T: PascalEnum + std::fmt::Debug>(value: T) -> String {
    format!("{value:?}")
}

/// `filter.preset.save`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterPresetSaveRequest {
    /// The name of the preset to write; the empty name is the defaults
    /// (`FilterSavePreset('')`).
    #[serde(default)]
    pub name: String,
    /// The values to write; the defaults when omitted.
    pub options: Option<FilterRequestOptions>,
    /// The values to write as a preset file's lines instead of the fields,
    /// as the dialog's checkboxes hold them.
    #[serde(default)]
    pub dry_run: bool,
}

/// `filter.preset.save`: the response.
#[derive(Serialize, JsonSchema)]
pub struct FilterPresetSaveResponse {
    pub name: String,
    pub settings: String,
    /// The section that was written.
    pub section: String,
    /// The values written.
    pub options: FilterRequestOptions,
    pub dry_run: bool,
    /// The preset changed the stored one (`FilterHasPresetChanged`), which
    /// the dialog asks about on close.
    pub changed: bool,
}

fn filter_preset_save(
    session: &mut Session,
    request: FilterPresetSaveRequest,
) -> Result<FilterPresetSaveResponse, CommandError> {
    session.mode()?;
    let path = crate::modgroups::settings_file();
    let options = request
        .options
        .as_ref()
        .map_or_else(|| default_options().to_options(), |options| options.to_options());
    let section = format!("{} {}", FILTER_SECTION, request.name).trim().to_owned();
    let changed = {
        let ini = MemIniFile::open(&path).map_err(|error| io_error(error, &path))?;
        read_preset(&ini, &request.name) != options
    };
    if !request.dry_run {
        let mut ini = MemIniFile::open(&path).map_err(|error| io_error(error, &path))?;
        write_preset(&mut ini, &request.name, &options);
        ini.update_file().map_err(|error| io_error(error, &path))?;
    }
    Ok(FilterPresetSaveResponse {
        name: request.name.clone(),
        settings: path.display().to_string(),
        section,
        options: FilterRequestOptions::from_options(&options),
        dry_run: request.dry_run,
        changed,
    })
}

/// Port of `FilterSavePreset`: the checkbox states and the two signature
/// lists as the settings file holds them.
fn write_preset(ini: &mut MemIniFile, name: &str, options: &FilterOptions) {
    let section = format!("{} {}", FILTER_SECTION, name).trim().to_owned();
    let write_bool =
        |ini: &mut MemIniFile, key: &str, value: bool| ini.write_string(&section, key, if value { "1" } else { "0" });
    write_bool(ini, "ConflictAll", options.conflict_all.is_some());
    write_bool(ini, "ConflictThis", options.conflict_this.is_some());
    write_bool(ini, "ByInjectStatus", options.by_inject_status.is_some());
    write_bool(ini, "InjectStatus", options.by_inject_status.unwrap_or(true));
    write_bool(ini, "ByNotReachableStatus", options.by_not_reachable_status.is_some());
    write_bool(
        ini,
        "NotReachableStatus",
        options.by_not_reachable_status.unwrap_or(true),
    );
    write_bool(
        ini,
        "ByReferencesInjectedStatus",
        options.by_references_injected_status.is_some(),
    );
    write_bool(
        ini,
        "ReferencesInjectedStatus",
        options.by_references_injected_status.unwrap_or(true),
    );
    write_bool(ini, "ByEditorID", options.by_editor_id.is_some());
    ini.write_string(&section, "EditorID", options.by_editor_id.as_deref().unwrap_or(""));
    write_bool(ini, "ByName", options.by_name.is_some());
    ini.write_string(&section, "Name", options.by_name.as_deref().unwrap_or(""));
    write_bool(ini, "ByBaseEditorID", options.by_base_editor_id.is_some());
    ini.write_string(
        &section,
        "BaseEditorID",
        options.by_base_editor_id.as_deref().unwrap_or(""),
    );
    write_bool(ini, "ByBaseName", options.by_base_name.is_some());
    ini.write_string(&section, "BaseName", options.by_base_name.as_deref().unwrap_or(""));
    write_bool(ini, "ByElementValue", options.by_element_value.is_some());
    ini.write_string(
        &section,
        "ElementValue",
        options.by_element_value.as_deref().unwrap_or(""),
    );
    write_bool(ini, "RegexComparison", options.regex_comparison);
    write_bool(ini, "ScaledActors", options.scaled_actors);
    write_bool(ini, "BySignature", options.by_signature.is_some());
    ini.write_string(&section, "Signatures", options.by_signature.as_deref().unwrap_or(""));
    write_bool(ini, "ByBaseSignature", options.by_base_signature.is_some());
    ini.write_string(
        &section,
        "BaseSignatures",
        options.by_base_signature.as_deref().unwrap_or(""),
    );
    write_bool(ini, "ByPersistent", options.by_persistent);
    write_bool(ini, "Persistent", options.persistent);
    write_bool(ini, "UnnecessaryPersistent", options.unnecessary_persistent);
    write_bool(ini, "MasterIsTemporary", options.master_is_temporary);
    // `FilterSavePreset` writes no `IsMaster`, which its `FilterLoadPreset`
    // reads back with the default false; the port writes it, so a preset
    // keeps the checkbox through a save and a load.
    write_bool(ini, "IsMaster", options.is_master);
    write_bool(ini, "PersistentPosChanged", options.persistent_pos_changed);
    write_bool(ini, "Deleted", options.deleted);
    write_bool(ini, "ByVWD", options.by_vwd.is_some());
    write_bool(ini, "VWD", options.by_vwd.unwrap_or(true));
    write_bool(ini, "ByHasVWDMesh", options.by_has_vwd_mesh.is_some());
    write_bool(ini, "HasVWDMesh", options.by_has_vwd_mesh.unwrap_or(true));
    write_bool(ini, "ByHasPrecombinedMesh", options.by_has_precombined_mesh.is_some());
    write_bool(
        ini,
        "HasPrecombinedMesh",
        options.by_has_precombined_mesh.unwrap_or(true),
    );
    write_bool(ini, "FlattenBlocks", options.flatten_blocks);
    write_bool(ini, "FlattenCellChilds", options.flatten_cell_childs);
    write_bool(ini, "AssignPersWrldChild", options.assign_pers_wrld_child);
    write_bool(ini, "InheritConflictByParent", options.inherit_conflict_by_parent);
    let conflict_all: BTreeSet<String> = options
        .conflict_all
        .as_ref()
        .map(|set| set.iter().map(|value| pascal_enum_name(*value)).collect())
        .unwrap_or_default();
    let conflict_this: BTreeSet<String> = options
        .conflict_this
        .as_ref()
        .map(|set| set.iter().map(|value| pascal_enum_name(*value)).collect())
        .unwrap_or_default();
    for value in xedit_core::interface::types::ConflictAll::ALL {
        write_bool(
            ini,
            &pascal_enum_name(*value),
            conflict_all.contains(&pascal_enum_name(*value)),
        );
    }
    for value in xedit_core::interface::types::ConflictThis::ALL {
        write_bool(
            ini,
            &pascal_enum_name(*value),
            conflict_this.contains(&pascal_enum_name(*value)),
        );
    }
}

/// `filter.preset.delete`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterPresetDeleteRequest {
    /// The name of the preset to remove; the empty name is the defaults
    /// (`btnFilterDelClick` with the first entry selected).
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub dry_run: bool,
}

/// `filter.preset.delete`: the response.
#[derive(Serialize, JsonSchema)]
pub struct FilterPresetDeleteResponse {
    pub name: String,
    pub section: String,
    pub settings: String,
    /// The preset was in the file (`TMemIniFile.EraseSection`).
    pub erased: bool,
    pub dry_run: bool,
}

fn filter_preset_delete(
    session: &mut Session,
    request: FilterPresetDeleteRequest,
) -> Result<FilterPresetDeleteResponse, CommandError> {
    session.mode()?;
    let path = crate::modgroups::settings_file();
    let section = format!("{} {}", FILTER_SECTION, request.name).trim().to_owned();
    let mut ini = MemIniFile::open(&path).map_err(|error| io_error(error, &path))?;
    let erased = ini.read_sections().iter().any(|name| same_text(name, &section));
    ini.erase_section(&section);
    if !request.dry_run {
        ini.update_file().map_err(|error| io_error(error, &path))?;
    }
    Ok(FilterPresetDeleteResponse {
        name: request.name,
        section,
        settings: path.display().to_string(),
        erased,
        dry_run: request.dry_run,
    })
}

/// `refs.build_reachable`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildReachableRequest {
    /// Build the references of the files that have none first
    /// (`mniNavBuildReachableClick` does), which the reachable walk reads.
    #[serde(default = "default_true")]
    pub build_refs: bool,
}

fn default_true() -> bool {
    true
}

/// `refs.build_reachable`: the response.
#[derive(Serialize, JsonSchema)]
pub struct BuildReachableResponse {
    /// `ReachableBuild` of the session.
    pub reachable_build: bool,
    /// The files whose references were built before the walk.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub built_refs: Vec<String>,
    /// The files whose reachable information was built again, in load order.
    pub files: Vec<String>,
}

/// Port of `mniNavBuildReachableClick`: the references of every loaded file
/// that has none, then `ResetReachable` and `BuildReachable` for every
/// file, which sets `ReachableBuild`.
fn refs_build_reachable(
    session: &mut Session,
    request: BuildReachableRequest,
) -> Result<BuildReachableResponse, CommandError> {
    session.mode()?;
    let files = crate::conflicts::session_files(session)?;
    let mut built_refs = Vec::new();
    if request.build_refs {
        for file in &files {
            if !file.refs_built() {
                xedit_core::implementation::refs::build_or_load_refs(std::slice::from_ref(file), false)
                    .map_err(|message| CommandError::new("refs_failed", message))?;
                built_refs.push(file.get_name());
            }
        }
    }
    let reached = crate::clean::run_on_large_stack({
        let files = files.clone();
        move || {
            for file in &files {
                let element: &dyn ElementImpl = file.as_element_impl().expect("a file is an element implementation");
                xedit_core::implementation::reachable::reset_reachable(element);
            }
            xedit_core::implementation::reachable::begin_reachable_walk();
            for file in &files {
                xedit_core::implementation::reachable::build_reachable(file);
            }
        }
    });
    reached?;
    set_reachable_build(true);
    Ok(BuildReachableResponse {
        reachable_build: true,
        built_refs,
        files: files.iter().map(|file| file.get_name()).collect(),
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "filter.apply",
        "Apply a filter to the navigation tree of the loaded files (mniNavFilterApplyClick, with the options of TfrmFilterOptions in the request or in a preset of the settings file) and report the nodes it left.",
        false,
        filter_apply,
    );
    registry.register(
        "filter.remove",
        "Remove the filter from the navigation tree (mniNavFilterRemoveClick).",
        false,
        filter_remove,
    );
    registry.register(
        "filter.presets",
        "List the filter presets of xEdit's settings file (FilterListPresets).",
        false,
        filter_presets,
    );
    registry.register(
        "filter.preset.save",
        "Write the options as a filter preset of xEdit's settings file (FilterSavePreset, FilterHasPresetChanged).",
        true,
        filter_preset_save,
    );
    registry.register(
        "filter.preset.delete",
        "Remove a filter preset from xEdit's settings file (btnFilterDelClick).",
        true,
        filter_preset_delete,
    );
    registry.register(
        "refs.build_reachable",
        "Build the reachable information of the loaded files, after the reference information (mniNavBuildReachableClick).",
        false,
        refs_build_reachable,
    );
}
