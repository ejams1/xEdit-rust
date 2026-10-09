// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavLocalizationSwitchClick,
// mniMainLocalizationLanguageClick, mniMainLocalizationEditorClick, the
// localization editor of vstViewEditing), xEdit/xeLocalizationForm.pas,
// xEdit/xeLocalizePluginForm.pas

//! The localization commands: the string tables of the loaded plugins as
//! the localization editor shows them (`localization.files`,
//! `localization.strings`, `localization.get`), its edit of a string
//! (`localization.set`, `btnSaveClick`) and its export
//! (`localization.export`, `mniFileExportClick`), the language of the
//! session (`localization.languages`, `localization.language`), and the
//! localization and delocalization of a plugin
//! (`localization.localize`, `localization.delocalize`,
//! `mniNavLocalizationSwitchClick` with the translation of
//! `TfrmLocalizePlugin`). The tables an edit changes are written by
//! `files.save` with their plugin.

use std::collections::HashMap;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::FileImpl;
use xedit_core::implementation::file_flags::ModuleFlag;
use xedit_core::interface::globals::{edit_allowed, language, set_language};
use xedit_core::interface::types::DefType;
use xedit_core::interface::{Element, ElementRef, File};
use xedit_core::localization::{
    LocalizationFile, LocalizationHandlerImpl, STRING_ID_PREFIX, compare_text, localization_handler,
};

use crate::{CommandError, Registry, Session};

fn handler() -> Result<Arc<LocalizationHandlerImpl>, CommandError> {
    localization_handler().ok_or_else(|| CommandError::new("no_session", "no game loaded: pass --game and --load"))
}

fn parse_id(text: &str) -> Result<u32, CommandError> {
    let digits = text.trim().trim_start_matches("0x").trim_start_matches('$');
    u32::from_str_radix(digits, 16)
        .map_err(|_| CommandError::new("invalid_params", format!("{text} is not a hexadecimal string ID")))
}

/// Whether the table belongs to the plugin: its name starts with the name
/// of the plugin without the extension and `_`, as the localization editor
/// finds the table of a value (`EditValue`).
fn table_of_plugin(table: &str, plugin: &str) -> bool {
    let prefix = format!("{}_", xedit_core::delphi::change_file_ext(plugin, ""));
    table.len() >= prefix.len() && compare_text(&table[..prefix.len()], &prefix).is_eq()
}

/// Upstream `LoadForFile` of every loaded localized plugin, as the
/// language switch reloads them.
fn load_tables(session: &Session) {
    let _ = session;
    if let Some(handler) = localization_handler() {
        let mut files = xedit_core::interface::files();
        files.sort_by_key(|file| file.get_load_order());
        for file in &files {
            if file.get_is_localized() {
                handler.load_for_file(&file.get_name());
            }
        }
    }
}

/// A string table as the localization editor lists it.
#[derive(Serialize, JsonSchema)]
pub struct TableInfo {
    /// Position in the editor's list (sorted by name ignoring case).
    pub index: usize,
    /// File name of the table, such as `Skyrim_English.STRINGS`.
    pub name: String,
    /// Where the table is read from and saved to.
    pub path: String,
    /// Number of strings.
    pub strings: usize,
    /// Whether the table has changes that are not saved (shown in bold).
    pub modified: bool,
    /// The ID the next new string of the table gets at least, hexadecimal.
    pub next_id: String,
}

fn table_info(index: usize, file: &LocalizationFile) -> TableInfo {
    TableInfo {
        index,
        name: file.name().to_owned(),
        path: file.file_name().to_owned(),
        strings: file.count(),
        modified: file.modified(),
        next_id: format!("{:08X}", file.next_id()),
    }
}

/// `localization.files`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalizationFilesRequest {
    /// List only the tables of this plugin.
    pub file: Option<String>,
    /// Load the tables of every localized plugin first, as the GUI has them
    /// once their records are shown. Defaults to true; false lists only the
    /// tables loaded so far.
    #[serde(default = "default_true")]
    pub load: bool,
}

fn default_true() -> bool {
    true
}

/// `localization.files`: the response.
#[derive(Serialize, JsonSchema)]
pub struct LocalizationFilesResponse {
    /// The language of the tables (`wbLanguage`).
    pub language: String,
    pub tables: Vec<TableInfo>,
}

fn localization_files(
    session: &mut Session,
    request: LocalizationFilesRequest,
) -> Result<LocalizationFilesResponse, CommandError> {
    session.mode()?;
    if let Some(name) = &request.file {
        session.file(Some(name))?;
    }
    let handler = handler()?;
    if request.load {
        load_tables(session);
    }
    let tables = (0..handler.count())
        .filter_map(|index| handler.with_file(index, |file| table_info(index, file)))
        .filter(|table| {
            request
                .file
                .as_deref()
                .is_none_or(|plugin| table_of_plugin(&table.name, plugin))
        })
        .collect();
    Ok(LocalizationFilesResponse {
        language: language(),
        tables,
    })
}

/// The position of a loaded table, by its file name.
fn table_index(handler: &LocalizationHandlerImpl, name: &str) -> Result<usize, CommandError> {
    handler
        .index_of(xedit_core::delphi::path_file_name(name))
        .ok_or_else(|| {
            CommandError::new(
                "unknown_table",
                format!("{name} is not a loaded string table (see localization.files)"),
            )
        })
}

/// `localization.strings`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalizationStringsRequest {
    /// File name of the table, such as `Dawnguard_English.STRINGS`.
    pub table: String,
    /// Skip this many strings.
    #[serde(default)]
    pub offset: usize,
    /// Return at most this many strings (default 100).
    pub limit: Option<usize>,
}

/// A string of a table.
#[derive(Serialize, JsonSchema)]
pub struct StringEntry {
    /// The string ID, hexadecimal.
    pub id: String,
    pub text: String,
}

/// `localization.strings`: the response.
#[derive(Serialize, JsonSchema)]
pub struct LocalizationStringsResponse {
    pub table: TableInfo,
    /// The strings in the order of the table.
    pub strings: Vec<StringEntry>,
}

fn localization_strings(
    session: &mut Session,
    request: LocalizationStringsRequest,
) -> Result<LocalizationStringsResponse, CommandError> {
    session.mode()?;
    let handler = handler()?;
    load_tables(session);
    let index = table_index(&handler, &request.table)?;
    let limit = request.limit.unwrap_or(100);
    handler
        .with_file(index, |file| LocalizationStringsResponse {
            table: table_info(index, file),
            strings: file
                .items()
                .iter()
                .skip(request.offset)
                .take(limit)
                .map(|(id, text)| StringEntry {
                    id: format!("{id:08X}"),
                    text: text.clone(),
                })
                .collect(),
        })
        .ok_or_else(|| CommandError::new("unknown_table", request.table))
}

/// Where a string is: a table and an ID, or a localized string element.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StringTarget {
    /// File name of the table. With `id`; instead of `form_id` and `path`.
    pub table: Option<String>,
    /// The string ID, hexadecimal.
    pub id: Option<String>,
    /// Load order FormID of a record whose element holds the string.
    pub form_id: Option<String>,
    /// Path of the localized string element in the record, such as `FULL`.
    pub path: Option<String>,
    /// The plugin whose version of the record is meant.
    pub file: Option<String>,
}

/// The table and ID a request names. For an element, the ID it holds (read
/// with `NoTranslate`, as the main form opens the localization editor on
/// it) and the first table of its plugin that has the ID (`EditValue`).
fn resolve_target(
    session: &Session,
    handler: &LocalizationHandlerImpl,
    target: &StringTarget,
) -> Result<(usize, u32), CommandError> {
    match (&target.table, &target.id, &target.form_id, &target.path) {
        (Some(table), Some(id), None, None) => Ok((table_index(handler, table)?, parse_id(id)?)),
        (None, None, Some(form_id), Some(path)) => {
            let record = session.record(form_id, target.file.as_deref())?;
            let element = record.get_element_by_path(path).ok_or_else(|| {
                CommandError::new(
                    "unknown_element",
                    format!("{} has no element at {path}", record.get_name()),
                )
            })?;
            let plugin = element.get_file().map(|file| file.get_name()).unwrap_or_default();
            let localized = element.get_file().is_some_and(|file| file.get_is_localized());
            if !localized
                || element
                    .get_value_def()
                    .is_none_or(|def| def.get_def_type() != DefType::dtLString)
            {
                return Err(CommandError::new(
                    "not_localized",
                    format!(
                        "{} is not a localized string of a localized plugin",
                        element.get_full_path()
                    ),
                ));
            }
            handler.set_no_translate(true);
            let value = element.get_value();
            handler.set_no_translate(false);
            let id = u32::from_str_radix(value.trim(), 16).unwrap_or(0);
            let index = (0..handler.count())
                .find(|&index| {
                    handler
                        .with_file(index, |file| {
                            table_of_plugin(file.name(), &plugin) && file.id_exists(id)
                        })
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    CommandError::new(
                        "unknown_string",
                        format!("no table of {plugin} has the string ID {id:08X}"),
                    )
                })?;
            Ok((index, id))
        }
        _ => Err(CommandError::new(
            "invalid_params",
            "name the string with table and id, or with form_id and path",
        )),
    }
}

/// `localization.get`: the response.
#[derive(Serialize, JsonSchema)]
pub struct LocalizationGetResponse {
    pub table: String,
    /// The string ID, hexadecimal.
    pub id: String,
    /// Whether the table has the ID.
    pub found: bool,
    /// The text, or the error text upstream shows for an unknown ID.
    pub text: String,
}

fn localization_get(session: &mut Session, request: StringTarget) -> Result<LocalizationGetResponse, CommandError> {
    session.mode()?;
    let handler = handler()?;
    load_tables(session);
    let (index, id) = resolve_target(session, &handler, &request)?;
    handler
        .with_file(index, |file| {
            let (found, text) = file.find(id);
            LocalizationGetResponse {
                table: file.name().to_owned(),
                id: format!("{id:08X}"),
                found,
                text,
            }
        })
        .ok_or_else(|| CommandError::new("unknown_table", "the table is gone"))
}

/// `localization.set`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalizationSetRequest {
    /// File name of the table. With `id`; instead of `form_id` and `path`.
    pub table: Option<String>,
    /// The string ID, hexadecimal.
    pub id: Option<String>,
    /// Load order FormID of a record whose element holds the string.
    pub form_id: Option<String>,
    /// Path of the localized string element in the record, such as `FULL`.
    pub path: Option<String>,
    /// The plugin whose version of the record is meant.
    pub file: Option<String>,
    /// The new text of the string.
    pub text: String,
    /// Store the text as the editor's memo gives it (`memoText.Lines.Text`):
    /// every line break becomes CR LF and the text ends with one.
    #[serde(default)]
    pub editor_text: bool,
    /// Report the change, but make none.
    #[serde(default)]
    pub dry_run: bool,
}

/// `localization.set`: the response.
#[derive(Serialize, JsonSchema)]
pub struct LocalizationSetResponse {
    pub table: String,
    /// The string ID, hexadecimal.
    pub id: String,
    pub old: String,
    pub new: String,
    /// Whether the text changes; the table is then modified and is written
    /// when its plugin is saved.
    pub changed: bool,
    pub dry_run: bool,
}

/// The text of a `TMemo`'s `Lines.Text`: the lines (split at CR LF, CR or
/// LF) each followed by CR LF.
fn memo_text(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
        result.push_str(&rest[..end]);
        result.push_str("\r\n");
        rest = &rest[end..];
        if let Some(stripped) = rest.strip_prefix("\r\n") {
            rest = stripped;
        } else if !rest.is_empty() {
            rest = &rest[1..];
        }
    }
    result
}

fn localization_set(
    session: &mut Session,
    request: LocalizationSetRequest,
) -> Result<LocalizationSetResponse, CommandError> {
    session.mode()?;
    let handler = handler()?;
    load_tables(session);
    let target = StringTarget {
        table: request.table.clone(),
        id: request.id.clone(),
        form_id: request.form_id.clone(),
        path: request.path.clone(),
        file: request.file.clone(),
    };
    let (index, id) = resolve_target(session, &handler, &target)?;
    let new = if request.editor_text {
        memo_text(&request.text)
    } else {
        request.text.clone()
    };
    let (table, exists, old) = handler
        .with_file(index, |file| (file.name().to_owned(), file.id_exists(id), file.get(id)))
        .ok_or_else(|| CommandError::new("unknown_table", "the table is gone"))?;
    if !exists {
        return Err(CommandError::new(
            "unknown_string",
            format!("{table} has no string ID {id:08X}"),
        ));
    }
    let changed = old != new;
    if !request.dry_run {
        // `btnSaveClick`: `Data.lFile[Data.ID] := memoText.Lines.Text`.
        handler.with_file_mut(index, |file| file.put(id, &new));
    }
    Ok(LocalizationSetResponse {
        table,
        id: format!("{id:08X}"),
        old,
        new,
        changed,
        dry_run: request.dry_run,
    })
}

/// `localization.export`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalizationExportRequest {
    /// File name of the table.
    pub table: String,
    /// Path of the text file; `<table>.txt` in the current folder when
    /// omitted, as the export dialog proposes.
    pub output: Option<String>,
    /// Report the size, write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `localization.export`: the response.
#[derive(Serialize, JsonSchema)]
pub struct LocalizationExportResponse {
    pub table: String,
    pub output: String,
    /// Number of strings exported.
    pub strings: usize,
    /// Size of the text file in bytes.
    pub bytes: u64,
    pub written: bool,
}

fn localization_export(
    session: &mut Session,
    request: LocalizationExportRequest,
) -> Result<LocalizationExportResponse, CommandError> {
    session.mode()?;
    let handler = handler()?;
    load_tables(session);
    let index = table_index(&handler, &request.table)?;
    let (table, strings, text) = handler
        .with_file(index, |file| (file.name().to_owned(), file.count(), file.export_text()))
        .ok_or_else(|| CommandError::new("unknown_table", request.table.clone()))?;
    let output = request.output.unwrap_or_else(|| format!("{table}.txt"));
    if !request.dry_run {
        std::fs::write(&output, &text).map_err(|error| CommandError::new("io", format!("{output}: {error}")))?;
    }
    Ok(LocalizationExportResponse {
        table,
        output,
        strings,
        bytes: text.len() as u64,
        written: !request.dry_run,
    })
}

/// `localization.languages`: the response.
#[derive(Serialize, JsonSchema)]
pub struct LocalizationLanguagesResponse {
    /// The language of the session (`wbLanguage`).
    pub language: String,
    /// The languages of the tables the data folder and the archives hold
    /// (`AvailableLanguages`), as the Language menu lists them.
    pub available: Vec<String>,
    /// The file names of those tables (`AvailableLocalizationFiles`), as the
    /// translation lists of "Localize plugin" show them.
    pub tables: Vec<String>,
}

fn localization_languages(
    session: &mut Session,
    _request: crate::NoParams,
) -> Result<LocalizationLanguagesResponse, CommandError> {
    session.mode()?;
    Ok(LocalizationLanguagesResponse {
        language: language(),
        available: LocalizationHandlerImpl::available_languages(),
        tables: LocalizationHandlerImpl::available_localization_files(),
    })
}

/// `localization.language`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalizationLanguageRequest {
    /// The language to switch to, as `localization.languages` lists it.
    pub language: String,
    /// Switch even when string tables have unsaved changes, which the
    /// switch drops (xEdit drops them without asking).
    #[serde(default)]
    pub discard: bool,
}

/// `localization.language`: the response.
#[derive(Serialize, JsonSchema)]
pub struct LocalizationLanguageResponse {
    pub previous: String,
    pub language: String,
    pub changed: bool,
    /// The tables with unsaved changes the switch dropped.
    pub discarded: Vec<String>,
    /// The tables loaded in the new language.
    pub tables: Vec<String>,
}

/// Port of `mniMainLocalizationLanguageClick`: the language changes, every
/// table is dropped (`Clear`, which makes the records read their names
/// again) and the tables of the localized plugins load in the new language.
fn localization_language(
    session: &mut Session,
    request: LocalizationLanguageRequest,
) -> Result<LocalizationLanguageResponse, CommandError> {
    session.mode()?;
    let handler = handler()?;
    let previous = language();
    let mut response = LocalizationLanguageResponse {
        previous: previous.clone(),
        language: previous.clone(),
        changed: false,
        discarded: Vec::new(),
        tables: Vec::new(),
    };
    if previous == request.language {
        return Ok(response);
    }
    let discarded: Vec<String> = (0..handler.count())
        .filter_map(|index| handler.with_file(index, |file| file.modified().then(|| file.name().to_owned())))
        .flatten()
        .collect();
    if !discarded.is_empty() && !request.discard {
        return Err(CommandError::new(
            "unsaved_strings",
            format!(
                "the string tables {} have unsaved changes, which the switch drops: save them or pass discard",
                discarded.join(", ")
            ),
        ));
    }
    set_language(&request.language);
    handler.clear();
    load_tables(session);
    response.language = request.language;
    response.changed = true;
    response.discarded = discarded;
    response.tables = (0..handler.count())
        .filter_map(|index| handler.with_file(index, |file| file.name().to_owned()))
        .collect();
    Ok(response)
}

/// `localization.localize` and `localization.delocalize`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalizationSwitchRequest {
    /// Name of the loaded plugin; the only loaded plugin when omitted.
    pub file: Option<String>,
    /// Localize only: the tables to translate from ("Translation" of
    /// "Localize plugin"), file names as `localization.languages` lists
    /// them, read from the `Strings` folder of the data folder. A text found
    /// in them (ignoring case) is replaced by the string at the same
    /// position of the `translate_to` tables.
    #[serde(default)]
    pub translate_from: Vec<String>,
    /// Localize only: the tables to translate to, as many as
    /// `translate_from`.
    #[serde(default)]
    pub translate_to: Vec<String>,
    /// Count the localizable strings (and the translated ones), change
    /// nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `localization.localize` and `localization.delocalize`: the response.
#[derive(Serialize, JsonSchema)]
pub struct LocalizationSwitchResponse {
    pub file: String,
    /// Whether the plugin is localized afterwards (for a dry run, before).
    pub localized: bool,
    /// The localizable strings of the plugin (`Localizable Strings`).
    pub localizable: usize,
    /// The strings found in the translation tables, and the empty ones
    /// (`Translated`).
    pub translated: usize,
    /// The string tables the localization made or changed; they are written
    /// when the plugin is saved.
    pub tables: Vec<String>,
    pub dry_run: bool,
}

/// Port of `GatherLStrings` of `mniNavLocalizationSwitchClick`: every
/// element whose value is a localized string, the element before its
/// children and the children from the last one to the first.
pub fn gather_lstrings(element: &ElementRef, list: &mut Vec<ElementRef>) {
    if element
        .get_value_def()
        .is_some_and(|def| def.get_def_type() == DefType::dtLString)
    {
        list.push(element.clone());
    }
    if let Some(container) = element.as_container() {
        for index in (0..container.get_element_count()).rev() {
            if let Some(child) = container.get_element(index) {
                gather_lstrings(&child, list);
            }
        }
    }
}

/// The vocabulary of a translation (`lFrom` with the texts in lower case,
/// `lTo`): the strings of the tables of each list, in the order of
/// `AvailableLocalizationFiles`, read from `<data>\Strings`.
fn translation_vocabulary(
    from: &[String],
    to: &[String],
) -> Result<(HashMap<String, usize>, Vec<String>), CommandError> {
    let available = LocalizationHandlerImpl::available_localization_files();
    for name in from.iter().chain(to) {
        if !available.iter().any(|known| compare_text(known, name).is_eq()) {
            return Err(CommandError::new(
                "invalid_params",
                format!("{name} is not a string table of the data folder or its archives"),
            ));
        }
    }
    // `TfrmLocalizePlugin.FormClose`.
    if from
        .iter()
        .any(|name| to.iter().any(|other| compare_text(name, other).is_eq()))
    {
        return Err(CommandError::new(
            "invalid_params",
            "Translation files should not match",
        ));
    }
    if from.len() != to.len() {
        return Err(CommandError::new(
            "invalid_params",
            "Translation files should come in pairs",
        ));
    }
    let mut seen: Vec<String> = Vec::new();
    let mut lower_from: HashMap<String, usize> = HashMap::new();
    let mut from_count = 0;
    let mut to_strings = Vec::new();
    for name in &available {
        if seen.iter().any(|known| compare_text(known, name).is_eq()) {
            continue;
        }
        seen.push(name.clone());
        let in_from = from.iter().any(|other| compare_text(name, other).is_eq());
        let in_to = to.iter().any(|other| compare_text(name, other).is_eq());
        if !in_from && !in_to {
            continue;
        }
        let path = format!("{}{name}", LocalizationHandlerImpl::strings_path());
        let table = LocalizationFile::open(&path)
            .map_err(|error| CommandError::new("io", format!("Cannot open file \"{path}\". {error}")))?;
        if in_from {
            for (_, text) in table.items() {
                lower_from.entry(text.to_lowercase()).or_insert(from_count);
                from_count += 1;
            }
        }
        if in_to {
            to_strings.extend(table.items().iter().map(|(_, text)| text.clone()));
        }
    }
    if from_count != to_strings.len() {
        return Err(CommandError::new(
            "edit_failed",
            "[Error] Number of strings in vocabulary does not match. Check parameters and run again.",
        ));
    }
    Ok((lower_from, to_strings))
}

/// Port of `mniNavLocalizationSwitchClick`: a plugin that is not localized
/// gets each localized string's text as a new string of its tables
/// (`AddValue`, translated first when a vocabulary is given) and holds the
/// IDs; a localized plugin gets the texts back (`NoTranslate`); then the
/// localized flag of the file header flips. Upstream closes the
/// application after it ("a very dirty hack"), so the plugin should be
/// saved and loaded again before further edits.
pub fn switch_localization(
    file: &Arc<FileImpl>,
    translate_from: &[String],
    translate_to: &[String],
    dry_run: bool,
) -> Result<LocalizationSwitchResponse, CommandError> {
    let handler = handler()?;
    let localize = !file.get_is_localized();
    if file.get_load_order() <= 0 {
        return Err(CommandError::new(
            "not_editable",
            format!(
                "{} is the game master, which xEdit does not (de)localize",
                file.get_name()
            ),
        ));
    }
    if !localize && !(translate_from.is_empty() && translate_to.is_empty()) {
        return Err(CommandError::new(
            "invalid_params",
            "a translation applies to the localization only",
        ));
    }
    if !dry_run && !edit_allowed() {
        return Err(CommandError::new("edit_required", "pass --edit to change plugin data"));
    }
    let vocabulary = if localize && !(translate_from.is_empty() && translate_to.is_empty()) {
        Some(translation_vocabulary(translate_from, translate_to)?)
    } else {
        None
    };
    let mut lstrings = Vec::new();
    gather_lstrings(&(file.clone() as ElementRef), &mut lstrings);
    let mut translated = 0;
    let edit = |error: String| CommandError::new("edit_failed", error);
    for element in &lstrings {
        if localize {
            let mut text = element.get_edit_value();
            if let Some((from, to)) = &vocabulary {
                if let Some(&position) = from.get(&text.to_lowercase()) {
                    text = to[position].clone();
                    translated += 1;
                } else if text.is_empty() {
                    // Count empty strings as translated too.
                    translated += 1;
                }
            }
            if dry_run {
                continue;
            }
            let id = handler.add_value(&text, Some(element));
            element
                .set_edit_value(&format!("{STRING_ID_PREFIX}{id:08X}"))
                .map_err(edit)?;
        } else {
            if dry_run {
                continue;
            }
            let text = element.get_edit_value();
            handler.set_no_translate(true);
            let result = element.set_edit_value(&text);
            handler.set_no_translate(false);
            result.map_err(edit)?;
        }
    }
    let plugin = file.get_name();
    if !dry_run {
        file.set_module_flag(ModuleFlag::Localized, localize).map_err(edit)?;
    }
    let tables = if localize && !dry_run {
        (0..handler.count())
            .filter_map(|index| {
                handler
                    .with_file(index, |table| {
                        (table.modified() && table_of_plugin(table.name(), &plugin)).then(|| table.name().to_owned())
                    })
                    .flatten()
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(LocalizationSwitchResponse {
        file: plugin,
        localized: file.get_is_localized(),
        localizable: lstrings.len(),
        translated,
        tables,
        dry_run,
    })
}

fn localization_localize(
    session: &mut Session,
    request: LocalizationSwitchRequest,
) -> Result<LocalizationSwitchResponse, CommandError> {
    let file = session.file(request.file.as_deref())?;
    if file.get_is_localized() {
        return Err(CommandError::new(
            "invalid_state",
            format!("{} is localized already", file.get_name()),
        ));
    }
    switch_localization(&file, &request.translate_from, &request.translate_to, request.dry_run)
}

fn localization_delocalize(
    session: &mut Session,
    request: LocalizationSwitchRequest,
) -> Result<LocalizationSwitchResponse, CommandError> {
    let file = session.file(request.file.as_deref())?;
    if !file.get_is_localized() {
        return Err(CommandError::new(
            "invalid_state",
            format!("{} is not localized", file.get_name()),
        ));
    }
    switch_localization(&file, &request.translate_from, &request.translate_to, request.dry_run)
}

/// The modified tables of a plugin, which `files.save` writes with it.
pub(crate) fn modified_tables_of(plugin: &str) -> Vec<usize> {
    let Some(handler) = localization_handler() else {
        return Vec::new();
    };
    (0..handler.count())
        .filter(|&index| {
            handler
                .with_file(index, |file| file.modified() && table_of_plugin(file.name(), plugin))
                .unwrap_or(false)
        })
        .collect()
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "localization.files",
        "List the loaded string tables as the localization editor does (TfrmLocalization).",
        false,
        localization_files,
    );
    registry.register(
        "localization.strings",
        "List the strings of a string table with their IDs (the localization editor's tree, LocalizationGetStringsFromFile).",
        false,
        localization_strings,
    );
    registry.register(
        "localization.get",
        "Read a string of a string table by ID, or the string a localized element holds.",
        false,
        localization_get,
    );
    registry.register(
        "localization.set",
        "Change the text of a string of a string table (the localization editor's Save, btnSaveClick).",
        true,
        localization_set,
    );
    registry.register(
        "localization.export",
        "Write a string table as text, an ID line and the text per string (ExportToFile).",
        true,
        localization_export,
    );
    registry.register(
        "localization.languages",
        "The language of the session and the languages and tables available (AvailableLanguages).",
        false,
        localization_languages,
    );
    registry.register(
        "localization.language",
        "Switch the language of the string tables (mniMainLocalizationLanguageClick).",
        false,
        localization_language,
    );
    registry.register(
        "localization.localize",
        "Move the strings of a plugin into new string tables and set its localized flag (Localize plugin, mniNavLocalizationSwitchClick).",
        true,
        localization_localize,
    );
    registry.register(
        "localization.delocalize",
        "Put the strings of a localized plugin into the plugin and clear its localized flag (Delocalize plugin, mniNavLocalizationSwitchClick).",
        true,
        localization_delocalize,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_memo_ends_every_line_with_cr_lf() {
        assert_eq!(memo_text("One"), "One\r\n");
        assert_eq!(memo_text("One\nTwo\r\nThree\r"), "One\r\nTwo\r\nThree\r\n");
        assert_eq!(memo_text(""), "");
    }

    #[test]
    fn tables_belong_to_the_plugin_of_their_prefix() {
        assert!(table_of_plugin("Dawnguard_English.STRINGS", "Dawnguard.esm"));
        assert!(table_of_plugin("dawnguard_en.ilstrings", "Dawnguard.esm"));
        assert!(!table_of_plugin("Dawnguard2_English.STRINGS", "Dawnguard.esm"));
        assert!(!table_of_plugin("Skyrim_English.STRINGS", "Dawnguard.esm"));
    }
}
