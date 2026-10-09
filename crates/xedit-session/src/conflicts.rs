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
    let options = ConflictOptions {
        only_master_and_leafs: request.master_and_leafs,
        quick_show_conflicts: request.quick_show_conflicts,
        ..Default::default()
    };
    let results = conflict::conflict_statuses(&files, &options);
    Ok(list(&files, &results, &request))
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
#[derive(Serialize, JsonSchema)]
pub struct CompareCell {
    /// The value as the view shows it (the summary of an element without a
    /// value); `null` when this record has no element in the row.
    pub value: Option<String>,
    pub conflict_this: ConflictThisName,
}

/// One row of the view: the same element in each record.
#[derive(Serialize, JsonSchema)]
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<String>,
}

fn records_compare(session: &mut Session, request: CompareRequest) -> Result<CompareResponse, CommandError> {
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let record = record
        .as_element_impl()
        .and_then(ElementImpl::main_record_impl)
        .ok_or_else(|| CommandError::new("internal", "the record is not a main record of a file"))?;
    let files = session_files(session)?;
    let options = ConflictOptions {
        only_master_and_leafs: request.master_and_leafs,
        hide_no_conflict: request.hide_no_conflict,
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
    let rows = view
        .children
        .iter()
        .filter_map(|child| compare_row(child, &view, request.include_hidden))
        .collect();
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
}
