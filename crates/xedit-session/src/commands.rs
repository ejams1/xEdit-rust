// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The read-only inspection commands: the session, its files, records and
//! elements as JSON.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use xedit_core::implementation::{FileImpl, MainRecordImpl, wb_file};
use xedit_core::interface::globals::{GameMode, data_path, game_name};
use xedit_core::interface::misc::Variant;
use xedit_core::interface::{
    Element, ElementRef, FileRef, FileStates, FormID, MainRecordRef, clear_files, record_by_load_order_form_id,
};

use crate::dump::{load_hardcoded, load_resources, setup_game};
use crate::{CommandError, NoParams, Registry, Session};

impl Session {
    /// Loads the plugins of a game in the order given, with their masters.
    pub fn load(game: &str, plugins: &[String]) -> Result<Self, String> {
        let mode = setup_game(game)?;
        clear_files();
        let mut files = Vec::new();
        for path in plugins {
            // Each plugin takes the next free slot after its masters.
            let file = wb_file(path, i32::MAX, FileStates::empty()).map_err(|error| error.to_string())?;
            files.push(file);
        }
        if let (Some(file), Some(path)) = (files.last(), plugins.last()) {
            load_resources(file, path, mode);
            load_hardcoded(mode)?;
        }
        Ok(Self {
            game: Some(mode),
            files,
        })
    }

    fn mode(&self) -> Result<GameMode, CommandError> {
        self.game
            .ok_or_else(|| CommandError::new("no_session", "no game loaded: pass --game and --load"))
    }

    /// The loaded file with the name, or the only loaded file when no name
    /// is given.
    fn file(&self, name: Option<&str>) -> Result<Arc<FileImpl>, CommandError> {
        self.mode()?;
        match name {
            Some(name) => self
                .files
                .iter()
                .find(|file| file.get_name().eq_ignore_ascii_case(name))
                .cloned()
                .ok_or_else(|| CommandError::new("unknown_file", format!("{name} is not loaded"))),
            None => match self.files.as_slice() {
                [file] => Ok(file.clone()),
                [] => Err(CommandError::new("no_session", "no plugin loaded: pass --load")),
                _ => Err(CommandError::new(
                    "ambiguous_file",
                    "several plugins are loaded: pass file",
                )),
            },
        }
    }

    fn record(&self, form_id: &str, file: Option<&str>) -> Result<MainRecordRef, CommandError> {
        let form_id = parse_form_id(form_id)?;
        let seen_from: Option<FileRef> = match file {
            Some(name) => Some(self.file(Some(name))? as FileRef),
            None => {
                self.mode()?;
                self.files.last().map(|file| file.clone() as FileRef)
            }
        };
        record_by_load_order_form_id(form_id, seen_from.as_ref()).ok_or_else(|| {
            CommandError::new(
                "unknown_record",
                format!("no record with FormID {}", form_id.to_string(false)),
            )
        })
    }
}

/// A load order FormID as eight hexadecimal digits.
fn parse_form_id(text: &str) -> Result<FormID, CommandError> {
    let digits = text.trim().trim_start_matches("0x");
    u32::from_str_radix(digits, 16)
        .map(FormID::from_cardinal)
        .map_err(|_| CommandError::new("invalid_params", format!("{text} is not a hexadecimal FormID")))
}

fn game_tag(mode: GameMode) -> &'static str {
    match mode {
        GameMode::gmFO4 => "fo4",
        GameMode::gmSSE => "sse",
        GameMode::gmTES5 => "tes5",
        _ => "unknown",
    }
}

/// `session.info`.
#[derive(Serialize, JsonSchema)]
pub struct SessionInfo {
    /// Game tag as given to `--game`.
    pub game: String,
    /// Upstream game name, such as `Skyrim` or `Fallout4`.
    pub game_name: String,
    /// Data folder the plugins load from.
    pub data_path: String,
    /// Names of the loaded plugins in load order.
    pub files: Vec<String>,
}

fn session_info(session: &mut Session, _: NoParams) -> Result<SessionInfo, CommandError> {
    let mode = session.mode()?;
    Ok(SessionInfo {
        game: game_tag(mode).to_owned(),
        game_name: game_name(),
        data_path: data_path(),
        files: all_files(session).iter().map(|file| file.get_name()).collect(),
    })
}

/// Every file of the session, including the masters that loaded with the
/// plugins and the hardcoded records, in load order.
fn all_files(session: &Session) -> Vec<FileRef> {
    let _ = session;
    let mut files = xedit_core::interface::files();
    files.sort_by_key(|file| file.get_load_order());
    files
}

/// One entry of `files.list`.
#[derive(Serialize, JsonSchema)]
pub struct FileInfo {
    pub name: String,
    pub load_order: i32,
    /// Master files in the order of the file header.
    pub masters: Vec<String>,
    pub record_count: usize,
    pub is_esm: bool,
    pub is_localized: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct FileList {
    pub files: Vec<FileInfo>,
}

fn files_list(session: &mut Session, _: NoParams) -> Result<FileList, CommandError> {
    session.mode()?;
    let files = all_files(session)
        .iter()
        .map(|file| FileInfo {
            name: file.get_name(),
            load_order: file.get_load_order(),
            masters: (0..file.get_master_count(false))
                .filter_map(|index| file.get_master(index, false))
                .map(|master| master.get_name())
                .collect(),
            record_count: file.get_record_count() as usize,
            is_esm: file.get_is_esm(),
            is_localized: file.get_is_localized(),
        })
        .collect();
    Ok(FileList { files })
}

/// A record in a list.
#[derive(Serialize, JsonSchema)]
pub struct RecordSummary {
    /// Load order FormID as eight hexadecimal digits.
    pub form_id: String,
    pub signature: String,
    pub editor_id: String,
    /// The name xEdit shows for the record.
    pub name: String,
    /// File that holds this version of the record.
    pub file: String,
}

fn summary_of(record: &MainRecordRef) -> RecordSummary {
    RecordSummary {
        form_id: record.get_load_order_form_id().to_string(false),
        signature: record.get_signature().to_string(),
        editor_id: record.get_editor_id(),
        name: record.get_name(),
        file: record.get_file().map(|file| file.get_name()).unwrap_or_default(),
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsListRequest {
    /// Name of the loaded plugin; the only loaded plugin when omitted.
    pub file: Option<String>,
    /// Keep only records with this signature, such as `NPC_`.
    pub signature: Option<String>,
    /// Records to skip.
    #[serde(default)]
    pub offset: usize,
    /// Records to return at most; 100 when omitted.
    pub limit: Option<usize>,
}

#[derive(Serialize, JsonSchema)]
pub struct RecordList {
    pub records: Vec<RecordSummary>,
    /// Records that match before `offset` and `limit` apply.
    pub total: usize,
}

fn records_of(file: &Arc<FileImpl>, signature: Option<&str>) -> Vec<Arc<MainRecordImpl>> {
    file.records()
        .into_iter()
        .filter(|record| signature.is_none_or(|signature| record.get_signature().to_string() == signature))
        .collect()
}

fn records_list(session: &mut Session, request: RecordsListRequest) -> Result<RecordList, CommandError> {
    let file = session.file(request.file.as_deref())?;
    let records = records_of(&file, request.signature.as_deref());
    let total = records.len();
    let records = records
        .into_iter()
        .skip(request.offset)
        .take(request.limit.unwrap_or(100))
        .map(|record| summary_of(&(record as MainRecordRef)))
        .collect();
    Ok(RecordList { records, total })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsFindRequest {
    /// Name of the loaded plugin; the only loaded plugin when omitted.
    pub file: Option<String>,
    /// Keep only records with this signature.
    pub signature: Option<String>,
    /// Text the editor ID contains, compared without case.
    pub editor_id: Option<String>,
    /// Text the name contains, compared without case.
    pub name: Option<String>,
    /// Records to return at most; 100 when omitted.
    pub limit: Option<usize>,
}

fn records_find(session: &mut Session, request: RecordsFindRequest) -> Result<RecordList, CommandError> {
    let file = session.file(request.file.as_deref())?;
    let editor_id = request.editor_id.as_deref().map(str::to_ascii_lowercase);
    let name = request.name.as_deref().map(str::to_ascii_lowercase);
    let matches: Vec<RecordSummary> = records_of(&file, request.signature.as_deref())
        .into_iter()
        .map(|record| summary_of(&(record as MainRecordRef)))
        .filter(|record| {
            editor_id
                .as_deref()
                .is_none_or(|text| record.editor_id.to_ascii_lowercase().contains(text))
                && name
                    .as_deref()
                    .is_none_or(|text| record.name.to_ascii_lowercase().contains(text))
        })
        .collect();
    let total = matches.len();
    let records = matches.into_iter().take(request.limit.unwrap_or(100)).collect();
    Ok(RecordList { records, total })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordsGetRequest {
    /// Load order FormID as hexadecimal digits.
    pub form_id: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// Levels of child elements to include; every level when omitted.
    pub depth: Option<usize>,
}

/// An element of the tree.
#[derive(Serialize, JsonSchema)]
pub struct ElementNode {
    pub name: String,
    /// Name as xEdit displays it, when it differs from `name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Value as xEdit displays it; empty for a container.
    pub value: String,
    /// Summary of a container, as the `[S]:` text of a dump.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Native value: number, boolean, string or bytes as hexadecimal.
    #[serde(skip_serializing_if = "Value::is_null")]
    pub native: Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ElementNode>,
}

fn native_json(variant: Variant) -> Value {
    match variant {
        Variant::Empty => Value::Null,
        Variant::Bool(value) => Value::from(value),
        Variant::Int(value) => Value::from(value),
        Variant::UInt(value) => Value::from(value),
        Variant::Float(value) => Value::from(value),
        Variant::Str(value) => Value::from(value),
        Variant::Bytes(bytes) => Value::from(
            bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    }
}

fn node_of(element: &ElementRef, depth: Option<usize>) -> ElementNode {
    let name = element.get_name();
    let display_name = element.get_display_name(true);
    let value = element.get_value();
    let summary = if value.is_empty() {
        element.get_summary()
    } else {
        String::new()
    };
    let children = match (element.as_container(), depth) {
        (Some(_), Some(0)) => Vec::new(),
        (Some(container), depth) => (0..container.get_element_count())
            .filter_map(|index| container.get_element(index))
            .map(|child| node_of(&child, depth.map(|depth| depth - 1)))
            .collect(),
        (None, _) => Vec::new(),
    };
    ElementNode {
        display_name: (display_name != name).then_some(display_name),
        name,
        value,
        summary,
        native: native_json(element.get_native_value()),
        children,
    }
}

#[derive(Serialize, JsonSchema)]
pub struct RecordDetail {
    #[serde(flatten)]
    pub summary: RecordSummary,
    /// Elements of the record, starting with the record header.
    pub elements: Vec<ElementNode>,
}

fn records_get(session: &mut Session, request: RecordsGetRequest) -> Result<RecordDetail, CommandError> {
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let elements = (0..record.get_element_count())
        .filter_map(|index| record.get_element(index))
        .map(|element| node_of(&element, request.depth))
        .collect();
    Ok(RecordDetail {
        summary: summary_of(&record),
        elements,
    })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsGetRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Path of the element inside the record, with `\` between the names,
    /// such as `DATA\Health` or `ACBS\Flags\Female`.
    pub path: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// Levels of child elements to include; every level when omitted.
    pub depth: Option<usize>,
}

#[derive(Serialize, JsonSchema)]
pub struct ElementDetail {
    /// Full path of the element.
    pub path: String,
    #[serde(flatten)]
    pub node: ElementNode,
}

fn elements_get(session: &mut Session, request: ElementsGetRequest) -> Result<ElementDetail, CommandError> {
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let element = record.get_element_by_path(&request.path).ok_or_else(|| {
        CommandError::new(
            "unknown_element",
            format!("{} has no element at {}", record.get_name(), request.path),
        )
    })?;
    Ok(ElementDetail {
        path: element.get_full_path(),
        node: node_of(&element, request.depth),
    })
}

/// Adds the inspection commands to the registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "session.info",
        "Report the game and the loaded plugins.",
        false,
        session_info,
    );
    registry.register(
        "files.list",
        "List the loaded files with their masters.",
        false,
        files_list,
    );
    registry.register("records.list", "List the records of a plugin.", false, records_list);
    registry.register(
        "records.find",
        "Find records by editor ID or name.",
        false,
        records_find,
    );
    registry.register("records.get", "Read a record with its elements.", false, records_get);
    registry.register(
        "elements.get",
        "Read an element of a record by path.",
        false,
        elements_get,
    );
}
