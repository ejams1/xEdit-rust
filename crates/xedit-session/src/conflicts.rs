// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (ConflictLevelForMainRecord,
// InheritStateFromChildren, SetActiveRecord, InitConflictStatus,
// vstViewGetText)

//! `conflicts.list` and `records.compare`: the conflict status of the
//! records of the loaded plugins, as the navigation tree of the GUI colours
//! them, and the side by side comparison of the records of a FormID, as the
//! view tab shows it. The comparison itself is `xedit_analysis::conflict`.

use std::collections::BTreeMap;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_analysis::conflict::{
    self, ConflictContext, ConflictOptions, ConflictResults, ConflictStatus, ViewNode, files_in_load_order,
};
use xedit_core::implementation::{ElementImpl, FileImpl, MainRecordImpl};
use xedit_core::interface::types::{ConflictAll, ConflictThis, PascalEnum};
use xedit_core::interface::{Element, MainRecord};

use crate::modgroups::{ModGroupChoice, ModGroupHides};
use crate::{CommandError, Registry, Session};

/// Declares the JSON form of a Pascal enumeration: its upstream value names.
macro_rules! named_enum {
    ($(#[$meta:meta])* $name:ident for $core:ident { $($value:ident,)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
        #[allow(non_camel_case_types)]
        pub enum $name {
            $($value,)+
        }

        impl From<$core> for $name {
            fn from(value: $core) -> Self {
                match value {
                    $($core::$value => $name::$value,)+
                }
            }
        }

        impl From<$name> for $core {
            fn from(value: $name) -> Self {
                match value {
                    $($name::$value => $core::$value,)+
                }
            }
        }
    };
}

named_enum! {
    /// Upstream `TConflictAll`, the conflict of all the records of a FormID,
    /// from the lowest to the highest: `caUnknown`, `caOnlyOne` (a single
    /// record), `caNoConflict` (several, no conflict), `caConflictBenign`,
    /// `caOverride` (an override without a conflict), `caConflict`,
    /// `caConflictCritical`.
    ConflictAllName for ConflictAll {
        caUnknown,
        caOnlyOne,
        caNoConflict,
        caConflictBenign,
        caOverride,
        caConflict,
        caConflictCritical,
    }
}

named_enum! {
    /// Upstream `TConflictThis`, the part one record plays among the records
    /// of its FormID, from the lowest to the highest: `ctUnknown`,
    /// `ctIgnored`, `ctNotDefined`, `ctIdenticalToMaster`, `ctOnlyOne`,
    /// `ctHiddenByModGroup`, `ctMaster`, `ctConflictBenign`, `ctOverride`,
    /// `ctIdenticalToMasterWinsConflict`, `ctConflictWins`,
    /// `ctConflictLoses`.
    ConflictThisName for ConflictThis {
        ctUnknown,
        ctIgnored,
        ctNotDefined,
        ctIdenticalToMaster,
        ctOnlyOne,
        ctHiddenByModGroup,
        ctMaster,
        ctConflictBenign,
        ctOverride,
        ctIdenticalToMasterWinsConflict,
        ctConflictWins,
        ctConflictLoses,
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConflictsRequest {
    /// List only the records of these loaded files (by name); every loaded
    /// file when empty. The status does not change: each record is always
    /// compared with the records of its FormID in every loaded file.
    #[serde(default)]
    pub files: Vec<String>,
    /// List only the records with these signatures, such as `NPC_`.
    #[serde(default)]
    pub signatures: Vec<String>,
    /// List only the records whose conflict (`conflict_all`) is at least
    /// this, such as `caConflict`.
    pub min_conflict_all: Option<ConflictAllName>,
    /// List only the records whose own status (`conflict_this`) is one of
    /// these, such as `ctConflictLoses`.
    #[serde(default)]
    pub conflict_this: Vec<ConflictThisName>,
    /// Also list the records that are the only record of their FormID
    /// (`caOnlyOne` with `ctOnlyOne`); the file counts include them always.
    #[serde(default)]
    pub include_single: bool,
    /// Compare only the master and the overrides that no other override has
    /// as a master (the GUI's "Only show Master and Leafs").
    #[serde(default)]
    pub master_and_leafs: bool,
    /// Classify a FormID with exactly one override as an override without
    /// comparing the two records, as `-quickshowconflicts` does.
    #[serde(default)]
    pub quick_show_conflicts: bool,
    /// Activate the valid mod groups of these names, so the records they
    /// hide are left out of the comparison (`ctHiddenByModGroup`).
    #[serde(default)]
    pub mod_groups: Vec<String>,
    /// Activate every valid mod group, as xEdit does with `-autoload`.
    #[serde(default)]
    pub all_mod_groups: bool,
    /// Activate the valid mod groups of the selection saved in xEdit's
    /// settings file.
    #[serde(default)]
    pub saved_mod_groups: bool,
    /// Listed records to skip.
    #[serde(default)]
    pub offset: usize,
    /// Records to list at most; all when omitted.
    pub limit: Option<usize>,
}

/// The conflicts of one loaded file, as the navigation tree shows the file:
/// the highest status of its records (`InheritStateFromChildren`).
#[derive(Serialize, JsonSchema)]
pub struct FileConflicts {
    pub name: String,
    pub load_order: i32,
    /// Main records of the file.
    pub records: usize,
    /// Records that are the only record of their FormID.
    pub single: usize,
    /// The highest `conflict_all` of the records of the file.
    pub conflict_all: ConflictAllName,
    /// The highest `conflict_this` of the records of the file.
    pub conflict_this: ConflictThisName,
    /// Records by `conflict_all`.
    pub by_conflict_all: BTreeMap<String, usize>,
    /// Records by `conflict_this`.
    pub by_conflict_this: BTreeMap<String, usize>,
}

/// The conflict status of one record.
#[derive(Serialize, JsonSchema)]
pub struct RecordConflict {
    /// File that holds this version of the record.
    pub file: String,
    /// Load order FormID as eight hexadecimal digits.
    pub form_id: String,
    pub signature: String,
    pub editor_id: String,
    /// The conflict of all the records of the FormID (`ConflictAll`).
    pub conflict_all: ConflictAllName,
    /// The part this record plays (`ConflictThis`).
    pub conflict_this: ConflictThisName,
}

#[derive(Serialize, JsonSchema)]
pub struct ConflictsResponse {
    /// Every loaded file in load order, with the counts of its records.
    pub files: Vec<FileConflicts>,
    /// Records that match the filters before `offset` and `limit` apply.
    pub total: usize,
    pub records: Vec<RecordConflict>,
    /// The warnings xEdit writes to its message log while it compares.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<String>,
    /// The activated mod groups.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mod_groups: Vec<String>,
    /// What the activated mod groups hide: for each module, the modules
    /// whose records its records hide.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mod_group_hides: Vec<ModGroupHides>,
}

/// The mod groups a request activates.
fn mod_group_choice(mod_groups: &[String], all: bool, saved: bool) -> ModGroupChoice {
    ModGroupChoice {
        mod_groups: mod_groups.to_vec(),
        all_mod_groups: all,
        saved_mod_groups: saved,
    }
}

/// The loaded files in the order of upstream's `Files`.
pub(crate) fn session_files(session: &Session) -> Result<Vec<Arc<FileImpl>>, CommandError> {
    session.mode()?;
    let files = xedit_core::interface::files()
        .into_iter()
        .filter_map(|file| file.as_element_impl().and_then(ElementImpl::file_impl))
        .collect();
    Ok(files_in_load_order(files))
}

fn conflicts_list(session: &mut Session, request: ConflictsRequest) -> Result<ConflictsResponse, CommandError> {
    let files = session_files(session)?;
    for name in &request.files {
        if !files.iter().any(|file| file.get_name().eq_ignore_ascii_case(name)) {
            return Err(CommandError::new("unknown_file", format!("{name} is not loaded")));
        }
    }
    let active = crate::modgroups::activate(
        session,
        &mod_group_choice(&request.mod_groups, request.all_mod_groups, request.saved_mod_groups),
    )?;
    let options = ConflictOptions {
        only_master_and_leafs: request.master_and_leafs,
        quick_show_conflicts: request.quick_show_conflicts,
        mod_groups: active.filter,
        ..Default::default()
    };
    let results = conflict::conflict_statuses(&files, &options);
    let mut response = list(&files, &results, &request);
    response.mod_groups = active.names;
    response.mod_group_hides = active.hides;
    Ok(response)
}

fn is_single(status: ConflictStatus) -> bool {
    status.all == ConflictAll::caOnlyOne && status.this == ConflictThis::ctOnlyOne
}

fn list(files: &[Arc<FileImpl>], results: &ConflictResults, request: &ConflictsRequest) -> ConflictsResponse {
    let mut response = ConflictsResponse {
        files: Vec::new(),
        total: 0,
        records: Vec::new(),
        messages: results.messages.clone(),
        mod_groups: Vec::new(),
        mod_group_hides: Vec::new(),
    };
    let limit = request.limit.unwrap_or(usize::MAX);
    let listed_file = |file: &FileImpl| {
        request.files.is_empty()
            || request
                .files
                .iter()
                .any(|name| file.get_name().eq_ignore_ascii_case(name))
    };
    for file in files {
        let records = file.records();
        let mut summary = FileConflicts {
            name: file.get_name(),
            load_order: file.load_order(),
            records: records.len(),
            single: 0,
            conflict_all: ConflictAllName::caUnknown,
            conflict_this: ConflictThisName::ctUnknown,
            by_conflict_all: BTreeMap::new(),
            by_conflict_this: BTreeMap::new(),
        };
        let listed = listed_file(file);
        for record in &records {
            let status = results.status(record);
            if is_single(status) {
                summary.single += 1;
            }
            summary.conflict_all = summary.conflict_all.max(status.all.into());
            summary.conflict_this = summary.conflict_this.max(status.this.into());
            *summary
                .by_conflict_all
                .entry(status.all.enum_name().to_owned())
                .or_default() += 1;
            *summary
                .by_conflict_this
                .entry(status.this.enum_name().to_owned())
                .or_default() += 1;
            if !listed || !matches(record, status, request) {
                continue;
            }
            response.total += 1;
            if response.total > request.offset && response.records.len() < limit {
                response.records.push(record_conflict(record, status));
            }
        }
        response.files.push(summary);
    }
    response
}

fn matches(record: &MainRecordImpl, status: ConflictStatus, request: &ConflictsRequest) -> bool {
    if !request.include_single && is_single(status) {
        return false;
    }
    if !request.signatures.is_empty() {
        let signature = record.get_signature().to_string();
        if !request.signatures.contains(&signature) {
            return false;
        }
    }
    if let Some(min) = request.min_conflict_all
        && ConflictAllName::from(status.all) < min
    {
        return false;
    }
    request.conflict_this.is_empty() || request.conflict_this.contains(&status.this.into())
}

fn record_conflict(record: &Arc<MainRecordImpl>, status: ConflictStatus) -> RecordConflict {
    RecordConflict {
        file: record.get_file().map(|file| file.get_name()).unwrap_or_default(),
        form_id: record.get_load_order_form_id().to_string(false),
        signature: record.get_signature().to_string(),
        editor_id: record.get_editor_id(),
        conflict_all: status.all.into(),
        conflict_this: status.this.into(),
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// Compare only the master and the overrides that no other override has
    /// as a master (the GUI's "Only show Master and Leafs").
    #[serde(default)]
    pub master_and_leafs: bool,
    /// Hide the rows without a conflict, as the view's "Hide no conflict
    /// and empty rows" does.
    #[serde(default)]
    pub hide_no_conflict: bool,
    /// Also list the rows the view hides (ignored members under the default
    /// "Hide ignored", members no record has, members the definitions hide).
    #[serde(default)]
    pub include_hidden: bool,
    /// `wbCompareRawData`: compare the raw data of the elements instead of
    /// their values, so sorted and aligned arrays are compared entry by
    /// entry. The release 4.1.5q never turns this on (the flag has no
    /// switch); the port offers it over the same code paths.
    #[serde(default)]
    pub raw_data: bool,
    /// Activate the valid mod groups of these names, so the records they
    /// hide are left out of the comparison (`ctHiddenByModGroup`).
    #[serde(default)]
    pub mod_groups: Vec<String>,
    /// Activate every valid mod group, as xEdit does with `-autoload`.
    #[serde(default)]
    pub all_mod_groups: bool,
    /// Activate the valid mod groups of the selection saved in xEdit's
    /// settings file.
    #[serde(default)]
    pub saved_mod_groups: bool,
    /// `edViewFilterName`: keep the rows whose name (column 0) contains this
    /// text, without regard to case.
    pub view_filter_name: Option<String>,
    /// `edViewFilterValue`: keep the rows with this text in one of their
    /// cells.
    pub view_filter_value: Option<String>,
    /// `cobViewFilter` at 1: a row matches when the name or a value matches,
    /// instead of both.
    #[serde(default)]
    pub view_filter_or: bool,
    /// `cbViewFilterKeepChildren`: also keep the rows below a matching one.
    #[serde(default)]
    pub keep_children: bool,
    /// `cbViewFilterKeepSiblings`: also keep the rows beside a matching one.
    #[serde(default)]
    pub keep_siblings: bool,
    /// `cbViewFilterKeepParentsSiblings`: also keep the rows beside the
    /// parent of a matching one.
    #[serde(default)]
    pub keep_parents_siblings: bool,
}

/// One record compared: a column of the view.
#[derive(Serialize, JsonSchema)]
pub struct CompareColumn {
    /// File that holds this version of the record.
    pub file: String,
    /// The part this record plays among the records of its FormID.
    pub conflict_this: ConflictThisName,
}

/// One cell of the view: the element of one record in a row.
#[derive(Serialize, JsonSchema, Clone)]
pub struct CompareCell {
    /// The value as the view shows it (the summary of an element without a
    /// value); `null` when this record has no element in the row.
    pub value: Option<String>,
    pub conflict_this: ConflictThisName,
}

/// One row of the view: the same element in each record.
#[derive(Serialize, JsonSchema, Clone)]
pub struct CompareRow {
    /// The name the view shows for the row, with " (sorted)" or
    /// " (aligned)" when the entries of arrays were matched by their sort
    /// keys or aligned.
    pub name: String,
    pub conflict_all: ConflictAllName,
    /// Whether the view shows the row; only with `include_hidden` false
    /// rows are listed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Whether the row filter hides the row (`IsViewNodeFiltered`); the row
    /// is left out unless `include_hidden` lists it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub filtered: bool,
    /// One cell per column.
    pub cells: Vec<CompareCell>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<CompareRow>,
}

#[derive(Serialize, JsonSchema)]
pub struct CompareResponse {
    /// Load order FormID as eight hexadecimal digits.
    pub form_id: String,
    pub signature: String,
    /// The conflict of all the compared records.
    pub conflict_all: ConflictAllName,
    /// The compared records in load order.
    pub columns: Vec<CompareColumn>,
    pub rows: Vec<CompareRow>,
    /// The view filter of the request and what it did, when one was asked
    /// for (`ApplyViewFilter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_filter: Option<ViewFilterReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<String>,
}

/// `ApplyViewFilter`: the rows of the view tab that the filter hides.
#[derive(Serialize, JsonSchema)]
pub struct ViewFilterReport {
    /// `IsFiltered` of every row of the view.
    pub rows: usize,
    /// The rows it hides.
    pub filtered: usize,
    /// The rows left.
    pub shown: usize,
}

/// `wbCompareRawData` for the duration of one command: the option sets the
/// process-wide flag while the comparison runs and restores it after, so a
/// `serve` or `mcp` session keeps its default. The release 4.1.5q never
/// sets the flag (its Options dialog lost the check box), so this is the
/// port's own option over the upstream code paths that read it.
struct RawData(bool);

impl RawData {
    fn new(on: bool) -> Self {
        let previous = xedit_core::interface::globals::compare_raw_data();
        xedit_core::interface::globals::set_compare_raw_data(on);
        RawData(previous)
    }
}

impl Drop for RawData {
    fn drop(&mut self) {
        xedit_core::interface::globals::set_compare_raw_data(self.0);
    }
}

fn records_compare(session: &mut Session, request: CompareRequest) -> Result<CompareResponse, CommandError> {
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let record = record
        .as_element_impl()
        .and_then(ElementImpl::main_record_impl)
        .ok_or_else(|| CommandError::new("internal", "the record is not a main record of a file"))?;
    let files = session_files(session)?;
    let active = crate::modgroups::activate(
        session,
        &mod_group_choice(&request.mod_groups, request.all_mod_groups, request.saved_mod_groups),
    )?;
    let _raw_data = RawData::new(request.raw_data);
    // The record shown in the view: its sorted and aligned arrays keep the
    // sort orders the view gives them, as in xEdit after the record was
    // shown.
    let options = ConflictOptions {
        only_master_and_leafs: request.master_and_leafs,
        hide_no_conflict: request.hide_no_conflict,
        mod_groups: active.filter,
        apply_to_elements: true,
        ..Default::default()
    };
    let mut context = ConflictContext::new(&options, &files);
    let view = context.view_for_main_record(&record);
    let columns = view
        .datas
        .iter()
        .map(|data| CompareColumn {
            file: data
                .element
                .as_ref()
                .and_then(|element| element.get_file())
                .map(|file| file.get_name())
                .unwrap_or_default(),
            conflict_this: data.conflict_this.into(),
        })
        .collect();
    let mut rows: Vec<CompareRow> = view
        .children
        .iter()
        .filter_map(|child| compare_row(child, &view, request.include_hidden))
        .collect();
    let view_filter = apply_view_filter(&mut rows, &request.view_filter());
    let conflict_all = view
        .datas
        .first()
        .map_or(ConflictAllName::caUnknown, |data| data.conflict_all.into());
    let (_, messages) = context.into_statuses();
    Ok(CompareResponse {
        form_id: record.get_load_order_form_id().to_string(false),
        signature: record.get_signature().to_string(),
        conflict_all,
        columns,
        rows,
        view_filter,
        messages,
    })
}

fn compare_row(node: &ViewNode, parent: &ViewNode, include_hidden: bool) -> Option<CompareRow> {
    if !node.visible && !include_hidden {
        return None;
    }
    Some(CompareRow {
        name: node.name(Some(parent)),
        conflict_all: node
            .datas
            .first()
            .map_or(ConflictAllName::caUnknown, |data| data.conflict_all.into()),
        hidden: !node.visible,
        filtered: false,
        cells: node
            .datas
            .iter()
            .enumerate()
            .map(|(column, data)| CompareCell {
                value: data.element.as_ref().map(|_| node.cell_text(column)),
                conflict_this: data.conflict_this.into(),
            })
            .collect(),
        children: node
            .children
            .iter()
            .filter_map(|child| compare_row(child, node, include_hidden))
            .collect(),
    })
}

/// One row of the view with its place in the tree, for the filter of
/// `ApplyViewFilter` (the GUI marks the nodes of its whole tree, which the
/// rows here are; the rows the view hides keep their subtrees with them, so
/// the rows are the tree closed under parents).
struct FlatRow {
    /// The parent row, `None` for a root row.
    parent: Option<usize>,
    /// The rows of this subtree as a range of the flat list (pre-order, the
    /// row first).
    subtree: (usize, usize),
    /// The ancestors of the row, from the root down.
    ancestors: Vec<usize>,
}

/// Flattens the rows in the order the filter walks them.
fn flatten_rows(rows: &[CompareRow], parent: Option<usize>, ancestors: &mut Vec<usize>, flat: &mut Vec<FlatRow>) {
    for row in rows {
        let index = flat.len();
        flat.push(FlatRow {
            parent,
            subtree: (index, index),
            ancestors: ancestors.clone(),
        });
        ancestors.push(index);
        flatten_rows(&row.children, Some(index), ancestors, flat);
        ancestors.pop();
        flat[index].subtree.1 = flat.len();
    }
}

/// Port of `IsViewNodeFiltered`: the name filter against the name of the row
/// (column 0) and the value filter against the cells of the columns after
/// the first. UPSTREAM-QUIRK: the loop over the columns starts at 1, so a
/// view of one record has no cell to search and no name match either, and
/// the name loop reads column 0 every time.
fn is_view_node_filtered(row: &CompareRow, name_filter: &str, value_filter: &str, use_or: bool) -> bool {
    let found_name = !name_filter.is_empty() && row.cells.len() > 1 && row.name.to_lowercase().contains(name_filter);
    let found_value = !value_filter.is_empty()
        && row.cells.iter().skip(1).any(|cell| {
            cell.value
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(value_filter)
        });
    match (!name_filter.is_empty(), !value_filter.is_empty()) {
        (true, true) => {
            if use_or {
                !(found_name || found_value)
            } else {
                !(found_name && found_value)
            }
        }
        (true, false) => !found_name,
        (false, true) => !found_value,
        (false, false) => false,
    }
}

/// The view filter panel above the view tab, as `records.compare` and
/// `records.compare_selected` ask for it.
#[derive(Default)]
struct ViewFilter {
    name: Option<String>,
    value: Option<String>,
    or: bool,
    keep_children: bool,
    keep_siblings: bool,
    keep_parents_siblings: bool,
    include_hidden: bool,
}

/// Port of `ApplyViewFilter`: every row is marked by `IsViewNodeFiltered`,
/// and the rows a matching row keeps are marked too: its parents, its
/// children (`cbViewFilterKeepChildren`), the rows beside it
/// (`cbViewFilterKeepSiblings`) and the rows beside its parent
/// (`cbViewFilterKeepParentsSiblings`). The marked rows are left out of the
/// response unless `include_hidden` lists them.
fn apply_view_filter(rows: &mut Vec<CompareRow>, request: &ViewFilter) -> Option<ViewFilterReport> {
    let name_filter = request.name.as_deref().map(str::to_lowercase).unwrap_or_default();
    let value_filter = request.value.as_deref().map(str::to_lowercase).unwrap_or_default();
    if name_filter.is_empty() && value_filter.is_empty() {
        return None;
    }
    let mut flat = Vec::new();
    flatten_rows(rows, None, &mut Vec::new(), &mut flat);
    let mut filtered: Vec<bool> = Vec::with_capacity(flat.len());
    mark_rows(rows, &name_filter, &value_filter, request.or, &mut filtered);
    let total = flat.len();
    let unfiltered: Vec<usize> = (0..flat.len()).filter(|index| !filtered[*index]).collect();
    for index in &unfiltered {
        for ancestor in &flat[*index].ancestors {
            filtered[*ancestor] = false;
        }
        if request.keep_children {
            let subtree = flat[*index].subtree.0..flat[*index].subtree.1;
            filtered[subtree].fill(false);
        }
        if request.keep_siblings
            && let Some(parent) = flat[*index].parent
        {
            for node in flat[parent].subtree.0..flat[parent].subtree.1 {
                if flat[node].parent == Some(parent) {
                    filtered[node] = false;
                }
            }
        }
        if request.keep_parents_siblings
            && let Some(parent) = flat[*index].parent
            && let Some(grandparent) = flat[parent].parent
        {
            for node in flat[grandparent].subtree.0..flat[grandparent].subtree.1 {
                if flat[node].parent == Some(grandparent) {
                    filtered[node] = false;
                }
            }
        }
    }
    let filtered_count = filtered.iter().filter(|value| **value).count();
    let include_hidden = request.include_hidden;
    let mut cursor = 0;
    *rows = rebuild_rows(rows, &filtered, &mut cursor, include_hidden);
    Some(ViewFilterReport {
        rows: total,
        filtered: filtered_count,
        shown: total - filtered_count,
    })
}

/// Marks the rows in the order the filter walks them.
fn mark_rows(rows: &[CompareRow], name_filter: &str, value_filter: &str, use_or: bool, marked: &mut Vec<bool>) {
    for row in rows {
        marked.push(is_view_node_filtered(row, name_filter, value_filter, use_or));
        mark_rows(&row.children, name_filter, value_filter, use_or, marked);
    }
}

/// The rows left after the filter, in the same order.
fn rebuild_rows(rows: &[CompareRow], filtered: &[bool], cursor: &mut usize, keep_filtered: bool) -> Vec<CompareRow> {
    let mut result = Vec::new();
    for row in rows {
        let index = *cursor;
        *cursor += 1;
        let children = rebuild_rows(&row.children, filtered, cursor, keep_filtered);
        if filtered[index] && !keep_filtered {
            continue;
        }
        result.push(CompareRow {
            filtered: filtered[index],
            children,
            ..(*row).clone()
        });
    }
    result
}

/// The view filter of a `records.compare` request.
impl CompareRequest {
    fn view_filter(&self) -> ViewFilter {
        ViewFilter {
            name: self.view_filter_name.clone(),
            value: self.view_filter_value.clone(),
            or: self.view_filter_or,
            keep_children: self.keep_children,
            keep_siblings: self.keep_siblings,
            keep_parents_siblings: self.keep_parents_siblings,
            include_hidden: self.include_hidden,
        }
    }
}

/// One record of a `records.compare_selected` request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectedRecord {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareSelectedRequest {
    /// The records to compare, in the order of the view's columns; two at
    /// least. The GUI's "Compare Selected" takes the selected records of
    /// the navigation tree (two to `wbAutoCompareSelectedLimit`, five, and
    /// one record alone when more are selected).
    pub records: Vec<SelectedRecord>,
    /// Compare only the master and the overrides that no other override has
    /// as a master (the GUI's "Only show Master and Leafs"). Read by the
    /// FormID groups a selected record belongs to.
    #[serde(default)]
    pub master_and_leafs: bool,
    /// Hide the rows without a conflict, as the view's "Hide no conflict
    /// and empty rows" does: below `caConflictBenign`, as a sibling
    /// comparison does.
    #[serde(default)]
    pub hide_no_conflict: bool,
    /// Also list the rows the view hides.
    #[serde(default)]
    pub include_hidden: bool,
    /// `wbCompareRawData`: compare the raw data of the elements instead of
    /// their values.
    #[serde(default)]
    pub raw_data: bool,
    /// Activate the valid mod groups of these names.
    #[serde(default)]
    pub mod_groups: Vec<String>,
    /// Activate every valid mod group, as xEdit does with `-autoload`.
    #[serde(default)]
    pub all_mod_groups: bool,
    /// Activate the valid mod groups of the selection saved in xEdit's
    /// settings file.
    #[serde(default)]
    pub saved_mod_groups: bool,
    /// `edViewFilterName`: keep the rows whose name (column 0) contains this
    /// text, without regard to case.
    pub view_filter_name: Option<String>,
    /// `edViewFilterValue`: keep the rows with this text in one of their
    /// cells.
    pub view_filter_value: Option<String>,
    /// `cobViewFilter` at 1: a row matches when the name or a value matches,
    /// instead of both.
    #[serde(default)]
    pub view_filter_or: bool,
    /// `cbViewFilterKeepChildren`: also keep the rows below a matching one.
    #[serde(default)]
    pub keep_children: bool,
    /// `cbViewFilterKeepSiblings`: also keep the rows beside a matching one.
    #[serde(default)]
    pub keep_siblings: bool,
    /// `cbViewFilterKeepParentsSiblings`: also keep the rows beside the
    /// parent of a matching one.
    #[serde(default)]
    pub keep_parents_siblings: bool,
}

/// One column of a sibling comparison: a selected record.
#[derive(Serialize, JsonSchema)]
pub struct CompareSelectedColumn {
    /// File that holds the record.
    pub file: String,
    /// Load order FormID as eight hexadecimal digits.
    pub form_id: String,
    pub signature: String,
    pub editor_id: String,
    /// The name xEdit shows for the record.
    pub name: String,
}

#[derive(Serialize, JsonSchema)]
pub struct CompareSelectedResponse {
    /// The compared records in the order given (the columns of the view).
    pub records: Vec<CompareSelectedColumn>,
    pub rows: Vec<CompareRow>,
    /// The view filter of the request and what it did, when one was asked
    /// for (`ApplyViewFilter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_filter: Option<ViewFilterReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<String>,
}

/// Port of the sibling comparison of `DoSetActiveRecord`, the GUI's
/// "Compare Selected": the selected records of different FormIDs are the
/// columns, and their members are compared across them
/// (`ComparingSiblings`).
fn records_compare_selected(
    session: &mut Session,
    request: CompareSelectedRequest,
) -> Result<CompareSelectedResponse, CommandError> {
    if request.records.len() < 2 {
        return Err(CommandError::new(
            "invalid_params",
            "compare_selected compares two records at least",
        ));
    }
    let mut records = Vec::with_capacity(request.records.len());
    for selected in &request.records {
        let record = session.record(&selected.form_id, selected.file.as_deref())?;
        let record = record
            .as_element_impl()
            .and_then(ElementImpl::main_record_impl)
            .ok_or_else(|| CommandError::new("internal", "the record is not a main record of a file"))?;
        records.push(record);
    }
    let files = session_files(session)?;
    let active = crate::modgroups::activate(
        session,
        &mod_group_choice(&request.mod_groups, request.all_mod_groups, request.saved_mod_groups),
    )?;
    let _raw_data = RawData::new(request.raw_data);
    let options = ConflictOptions {
        only_master_and_leafs: request.master_and_leafs,
        hide_no_conflict: request.hide_no_conflict,
        comparing_siblings: true,
        mod_groups: active.filter,
        apply_to_elements: true,
        ..Default::default()
    };
    let mut context = ConflictContext::new(&options, &files);
    let view = context.view_for_sibling_records(&records);
    let mut rows: Vec<CompareRow> = view
        .children
        .iter()
        .filter_map(|child| compare_row(child, &view, request.include_hidden))
        .collect();
    let view_filter = apply_view_filter(
        &mut rows,
        &ViewFilter {
            name: request.view_filter_name.clone(),
            value: request.view_filter_value.clone(),
            or: request.view_filter_or,
            keep_children: request.keep_children,
            keep_siblings: request.keep_siblings,
            keep_parents_siblings: request.keep_parents_siblings,
            include_hidden: request.include_hidden,
        },
    );
    let (_, messages) = context.into_statuses();
    let columns = records
        .iter()
        .map(|record| CompareSelectedColumn {
            file: record.get_file().map(|file| file.get_name()).unwrap_or_default(),
            form_id: record.get_load_order_form_id().to_string(false),
            signature: record.get_signature().to_string(),
            editor_id: record.get_editor_id(),
            name: record.get_name(),
        })
        .collect();
    Ok(CompareSelectedResponse {
        records: columns,
        rows,
        view_filter,
        messages,
    })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForElementsRequest {
    /// Load order FormID of the first record as hexadecimal digits.
    pub form_id: String,
    /// Path of the element inside the first record.
    pub path: String,
    /// Plugin the first record is seen from; the last loaded plugin when
    /// omitted.
    pub file: Option<String>,
    /// Load order FormID of the other record.
    pub other_form_id: String,
    /// Path of the element inside the other record.
    pub other_path: String,
    /// Plugin the other record is seen from; the last loaded plugin when
    /// omitted.
    pub other_file: Option<String>,
    /// `aSiblingCompare` of `ConflictAllForElements`: compare the two as
    /// records of different FormIDs (the sibling comparison).
    #[serde(default)]
    pub sibling_compare: bool,
    /// `aInjected` of `ConflictAllForElements`.
    #[serde(default)]
    pub injected: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct ForElementsResponse {
    /// The `TConflictAll` of the pair (`ConflictAllForElements`).
    pub conflict_all: ConflictAllName,
    /// Path of the first element, as the response reports it.
    pub path: String,
    /// Path of the other element.
    pub other_path: String,
}

/// Port of the script function `ConflictAllForElements(a, b, aSiblingCompare,
/// aInjected)` (`xejviScriptAdapterHost.pas`): the conflict of two elements,
/// of their whole containers when both are containers
/// (`ConflictLevelForChildNodeDatas`), else of the pair
/// (`ConflictLevelForNodeDatas`).
fn conflicts_for_elements(session: &mut Session, request: ForElementsRequest) -> Result<ForElementsResponse, CommandError> {
    let files = session_files(session)?;
    let mut datas = Vec::new();
    let mut paths = Vec::new();
    for (form_id, path, file) in [
        (&request.form_id, &request.path, request.file.as_deref()),
        (&request.other_form_id, &request.other_path, request.other_file.as_deref()),
    ] {
        let record = session.record(form_id, file)?;
        let element = record.get_element_by_path(path).ok_or_else(|| {
            CommandError::new(
                "unknown_element",
                format!("{} has no element at {path}", record.get_name()),
            )
        })?;
        paths.push(element.get_full_path());
        let container = element
            .as_container()
            .filter(|container| container.get_element_count() > 0)
            .map(|_| xedit_analysis::conflict::ViewContainer::new(element.clone()));
        datas.push(xedit_analysis::conflict::ViewNodeData {
            container,
            element: Some(element),
            ..Default::default()
        });
    }
    let options = ConflictOptions::default();
    let mut context = ConflictContext::new(&options, &files);
    let conflict_all = if datas[0].container.is_some() {
        context.conflict_level_for_child_node_datas(&mut datas, request.sibling_compare, request.injected)
    } else {
        conflict::conflict_level_for_node_datas(&mut datas, request.sibling_compare, request.injected)
    };
    Ok(ForElementsResponse {
        conflict_all: conflict_all.into(),
        path: paths[0].clone(),
        other_path: paths[1].clone(),
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "conflicts.list",
        "List the conflict status of the records of the loaded plugins and of each file (ConflictLevelForMainRecord).",
        false,
        conflicts_list,
    );
    registry.register(
        "records.compare",
        "Compare the records of a FormID side by side, row by row with their conflict status, as the view tab does (InitConflictStatus).",
        false,
        records_compare,
    );
    registry.register(
        "records.compare_selected",
        "Compare records of different FormIDs side by side, as the GUI compares the selected navigation tree nodes (ComparingSiblings, DoSetActiveRecord with several records).",
        false,
        records_compare_selected,
    );
    registry.register(
        "conflicts.for_elements",
        "The conflict of two elements of two records, as the script function ConflictAllForElements reports it (ConflictLevelForNodeDatas, ConflictLevelForChildNodeDatas).",
        false,
        conflicts_for_elements,
    );
}
