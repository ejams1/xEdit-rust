// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeInit.pas (the `[Options]` reads of `Settings`
// and xeSettingsFileName) and xEdit/xeMainForm.pas (the settings loads of
// FormCreate and the saves of the options dialog and FormClose)

//! `settings.get` and `settings.set`: xEdit's settings file (`xEdit.ini`,
//! `TMemIniFile`) as the GUI reads and writes it.
//!
//! The file holds the keys the GUI persists: the main window's placement
//! and splitter widths (`[frmMain] Left`, `Top`, `Width`, `Height`,
//! `WindowState`, `pnlNavWidth`, `vstNavColumnWidth0..n`), a dialog's
//! placement under its own form name (`[frmLegend]`, `[frmViewElements]`,
//! the `TOCWidth` and `Zoom` of `[frmRichEdit]`), the conflict colours
//! (`[ColorConflictThis]` and `[ColorConflictAll]`, a `TColor` per enum
//! name), the tree sorts (`[Nav] FilesSort` and `INFOSort`) and the view
//! keys (`[View] HodeNoConflict`, `StickViewNodeLabel`, `ColumnWidth`,
//! `LastUsedFilter`, `LastUsedScript`, `IncludeScriptsFromSubDir`), next to
//! the `[Options]` switches `xeInit.pas` reads before anything loads. The
//! commands are generic: any section and key of the file, read and written
//! with the type of the `TMemIniFile` method the GUI uses for it
//! (`ReadString`, `ReadInteger`, `ReadBool`, `ReadFloat`, `ReadDouble` and
//! their `Write` pairs).

use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use xedit_core::delphi::float_to_str;
use xedit_core::interface::misc::str_to_int_def;
use xedit_loadorder::ini_files::MemIniFile;

use crate::{CommandError, Registry, Session};

fn io_error(error: std::io::Error, path: &Path) -> CommandError {
    CommandError::new("io", format!("{}: {error}", path.display()))
}

/// The type a setting is read or written with: one of the `TMemIniFile`
/// methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueType {
    String,
    Integer,
    Bool,
    /// `ReadFloat`/`ReadDouble` and `WriteFloat`/`WriteDouble`, which both
    /// format and parse through `FloatToStr`/`StrToFloat` (a `Single` is
    /// widened exactly, so the two give the same text).
    Float,
}

impl ValueType {
    /// `ReadString` for no type given.
    fn parse(name: Option<&str>) -> Result<ValueType, CommandError> {
        match name.unwrap_or("string").to_ascii_lowercase().as_str() {
            "string" => Ok(ValueType::String),
            "integer" | "int" => Ok(ValueType::Integer),
            "bool" | "boolean" => Ok(ValueType::Bool),
            "float" | "double" => Ok(ValueType::Float),
            other => Err(CommandError::new(
                "invalid_params",
                format!("{other} is not a setting type: use string, integer, bool, float or double"),
            )),
        }
    }

    /// `ReadString`, `ReadInteger`, `ReadBool`, `ReadFloat` and
    /// `ReadDouble` over the value the file holds. A missing key, and a
    /// value the typed read can not convert, becomes the default, as
    /// `StrToIntDef`, `StrToFloatDef` and `ReadBool`'s `<> 0` do.
    fn read(self, ini: &MemIniFile, section: &str, key: &str, default: Option<&Value>) -> Value {
        if !ini.value_exists(section, key) {
            return match self {
                ValueType::String => Value::String(match default {
                    Some(Value::String(text)) => text.clone(),
                    Some(other) => other.to_string(),
                    None => String::new(),
                }),
                ValueType::Integer => Value::from(default_i64(default)),
                ValueType::Bool => Value::from(default_bool(default)),
                ValueType::Float => Value::from(default_f64(default)),
            };
        }
        let text = ini.read_string(section, key, "");
        match self {
            ValueType::String => Value::String(text),
            ValueType::Integer => Value::from(str_to_int_def(&text, default_i64(default) as i32)),
            ValueType::Bool => Value::from(ini.read_bool(section, key, default_bool(default))),
            ValueType::Float => Value::from(xedit_core::delphi::str_to_float(&text).unwrap_or(default_f64(default))),
        }
    }

    /// `WriteString`, `WriteInteger`, `WriteBool`, `WriteFloat` and
    /// `WriteDouble`: the text the file gets.
    fn write(self, value: &Value) -> Result<String, CommandError> {
        let invalid = || {
            CommandError::new(
                "invalid_params",
                "the value does not have the type the setting is written with",
            )
        };
        match self {
            ValueType::String => match value {
                Value::String(text) => Ok(text.clone()),
                Value::Number(number) => Ok(number.to_string()),
                Value::Bool(value) => Ok(if *value { "1" } else { "0" }.to_owned()),
                _ => Err(invalid()),
            },
            // `WriteInteger`: `IntToStr` of the value.
            ValueType::Integer => number_of(value)
                .and_then(|number| number.as_i64())
                .map(|number| number.to_string())
                .ok_or_else(invalid),
            // `WriteBool`: `WriteInteger(Ord(Value))`.
            ValueType::Bool => value
                .as_bool()
                .map(|value| if value { "1" } else { "0" }.to_owned())
                .ok_or_else(invalid),
            // `WriteFloat`/`WriteDouble`: `FloatToStr` (15 significant
            // digits) of the value.
            ValueType::Float => number_of(value)
                .and_then(|number| number.as_f64())
                .map(float_to_str)
                .ok_or_else(invalid),
        }
    }
}

/// The default of the typed reads as an integer: the number or the number
/// of a string, 0 otherwise.
fn default_i64(default: Option<&Value>) -> i64 {
    default
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.trim().parse().ok()))
        .unwrap_or(0)
}

/// The default of the typed reads as a double, 0.0 otherwise.
fn default_f64(default: Option<&Value>) -> f64 {
    default
        .and_then(|value| value.as_f64().or_else(|| value.as_str()?.trim().parse().ok()))
        .unwrap_or(0.0)
}

/// The default of the typed reads as a boolean: the boolean or `true`/`1`
/// and `false`/`0` as text, `false` otherwise.
fn default_bool(default: Option<&Value>) -> bool {
    default
        .and_then(|value| {
            value.as_bool().or_else(|| {
                Some(matches!(
                    value.as_str()?.trim().to_ascii_lowercase().as_str(),
                    "true" | "1"
                ))
            })
        })
        .unwrap_or(false)
}

/// A number as given, or the number of a string that reads as one.
fn number_of(value: &Value) -> Option<serde_json::Number> {
    match value {
        Value::Number(number) => Some(number.clone()),
        Value::String(text) => text.parse::<serde_json::Number>().ok(),
        _ => None,
    }
}

/// `settings.get`: the request.
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsGetRequest {
    /// The section, such as `Options`, `View`, `Nav`, `ColorConflictThis`,
    /// `ColorConflictAll`, or a form's name (`frmMain`, `frmLegend`,
    /// `frmViewElements`, `frmRichEdit`, ...). Omit it for the names of
    /// every section of the file.
    pub section: Option<String>,
    /// The key of the section. Omit it for every key of the section.
    pub key: Option<String>,
    /// The `TMemIniFile` type the values are read with: `string` (the
    /// default, `ReadString`), `integer`, `bool`, `float` or `double`. The
    /// raw text is reported either way.
    pub value_type: Option<String>,
    /// The value of a missing section or key: the default argument of the
    /// `Read` method, as a string (the raw value) or a value of the type.
    pub default: Option<Value>,
}

/// One setting of the file.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct SettingEntry {
    pub section: String,
    pub key: String,
    /// The value as the file holds it (`ReadString`).
    pub raw: String,
    /// The value read with `value_type`, when one was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

/// `settings.get`: the response.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct SettingsGetResponse {
    /// The settings file (`xeSettingsFileName`).
    pub settings: String,
    /// Whether the file exists; a missing file reads as an empty ini.
    pub exists: bool,
    /// The names of every section, in file order, when the request named
    /// none (empty otherwise).
    pub sections: Vec<String>,
    /// The values read: the one key, or every key of the section.
    pub values: Vec<SettingEntry>,
}

fn settings_get(_session: &mut Session, request: SettingsGetRequest) -> Result<SettingsGetResponse, CommandError> {
    let value_type = ValueType::parse(request.value_type.as_deref())?;
    let typed = request.value_type.is_some();
    let path = crate::modgroups::settings_file();
    let exists = path.is_file();
    let ini = MemIniFile::open(&path).map_err(|error| io_error(error, &path))?;
    let Some(section) = request.section else {
        return Ok(SettingsGetResponse {
            settings: path.display().to_string(),
            exists,
            sections: ini.read_sections(),
            values: Vec::new(),
        });
    };
    let keys = match request.key {
        Some(key) => vec![key],
        None => ini.read_section(&section),
    };
    let values = keys
        .into_iter()
        .map(|key| {
            let value = typed.then(|| value_type.read(&ini, &section, &key, request.default.as_ref()));
            SettingEntry {
                raw: ini.read_string(&section, &key, ""),
                value,
                section: section.clone(),
                key,
            }
        })
        .collect();
    Ok(SettingsGetResponse {
        settings: path.display().to_string(),
        exists,
        sections: Vec::new(),
        values,
    })
}

/// `settings.set`: the request.
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsSetRequest {
    /// The section, such as `frmMain`, `ColorConflictThis` or `View`; a
    /// missing section is added at the end, as `TMemIniFile.WriteString`
    /// does.
    pub section: String,
    /// The key to write.
    pub key: String,
    /// The value: a string, a number or a boolean. An integer or a number
    /// may also be given as a string, as the GUI's edit boxes hold them.
    pub value: Value,
    /// The `TMemIniFile` type it is written with: `string` (the default,
    /// `WriteString`), `integer`, `bool` (`1`/`0`), `float` or `double`
    /// (`FloatToStr`).
    pub value_type: Option<String>,
    /// Report the change, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `settings.set`: the response.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct SettingsSetResponse {
    pub section: String,
    pub key: String,
    /// The value the file held before, `None` for a missing key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    /// The text the file gets.
    pub new: String,
    /// Whether the value differs from the one the file held.
    pub changed: bool,
    /// Whether the file was written (false for a dry run).
    pub written: bool,
    /// The settings file (`xeSettingsFileName`).
    pub settings: String,
    pub dry_run: bool,
}

fn settings_set(_session: &mut Session, request: SettingsSetRequest) -> Result<SettingsSetResponse, CommandError> {
    let value_type = ValueType::parse(request.value_type.as_deref())?;
    let new = value_type.write(&request.value)?;
    let path = crate::modgroups::settings_file();
    let mut ini = MemIniFile::open(&path).map_err(|error| io_error(error, &path))?;
    let old = ini
        .value_exists(&request.section, &request.key)
        .then(|| ini.read_string(&request.section, &request.key, ""));
    let changed = old.as_deref() != Some(new.as_str());
    if !request.dry_run {
        // `TfrmMain.FormCreate`: `ForceDirectories(ExtractFilePath(
        // xeSettingsFileName))` before the file is created or written.
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|error| io_error(error, folder))?;
        }
        ini.write_string(&request.section, &request.key, &new);
        ini.update_file().map_err(|error| io_error(error, &path))?;
    }
    Ok(SettingsSetResponse {
        section: request.section,
        key: request.key,
        old,
        new,
        changed,
        written: !request.dry_run,
        settings: path.display().to_string(),
        dry_run: request.dry_run,
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "settings.get",
        "Read xEdit's settings file (xEdit.ini): the section names, a section's keys and their values, typed as TMemIniFile reads them.",
        false,
        settings_get,
    );
    registry.register(
        "settings.set",
        "Write one value of xEdit's settings file as TMemIniFile writes it (the window placement, splitter, colour and view keys of the GUI).",
        true,
        settings_set,
    );
}
