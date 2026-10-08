// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/SniffProcessor.pas

//! The processor framework of Sniff: `TProcBase`, the base of every
//! operation, `TProcManager`, which runs a processor on one file and writes
//! what it returns, `TProcFileObject`, a file of the input folder or
//! archive, and the settings each processor keeps in its section of the
//! settings ini (`TMemIniFile`).
//!
//! A processor is a type implementing [`Proc`]. Upstream each one is a
//! class with a frame of controls: `OnShow` loads the controls from the
//! settings (the frame's `.dfm` gives the defaults), `OnStart` reads and
//! checks them before the run, `ProcessFile` runs on each file on several
//! threads, and `OnStop` ends the run. The port has no frames: the controls
//! are fields of the processor, [`Proc::on_show`] sets them from a
//! [`Storage`] with the `.dfm` defaults, and the rest keeps upstream's
//! names. `ProcessFile` takes `&self`: what a processor collects across
//! files goes to the [`ProcContext`] of the file, which the manager commits
//! in file order.

use std::cell::RefCell;
use std::path::Path;

use xedit_io::archive::{Archive, FileEntry};
use xedit_io::encoding::{ansi_bytes, string_list_text};

use crate::data_format::{DfError, R};
use crate::variant::str_to_int;

/// `TGameType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GameType {
    Tes3,
    Tes4,
    Fo3,
    Fnv,
    Tes5,
    Sse,
    Fo4,
}

impl GameType {
    /// Every game, in the order of `TGameType`.
    pub const ALL: &'static [GameType] = &[
        GameType::Tes3,
        GameType::Tes4,
        GameType::Fo3,
        GameType::Fnv,
        GameType::Tes5,
        GameType::Sse,
        GameType::Fo4,
    ];

    /// `TProcManager.GameTypeName`.
    pub fn name(self) -> &'static str {
        match self {
            GameType::Tes3 => "Morrowind",
            GameType::Tes4 => "Oblivion",
            GameType::Fo3 => "Fallout 3",
            GameType::Fnv => "New Vegas",
            GameType::Tes5 => "Skyrim LE",
            GameType::Sse => "Skyrim SE",
            GameType::Fo4 => "Fallout 4",
        }
    }

    /// The enumeration name without its `gt` prefix, as the game filter of
    /// the main form lists it.
    pub fn short_name(self) -> &'static str {
        match self {
            GameType::Tes3 => "TES3",
            GameType::Tes4 => "TES4",
            GameType::Fo3 => "FO3",
            GameType::Fnv => "FNV",
            GameType::Tes5 => "TES5",
            GameType::Sse => "SSE",
            GameType::Fo4 => "FO4",
        }
    }
}

/// `SelectFolder` and `SelectArchive` are dialogs of the GUI and are not
/// ported.
///
/// `TextToString`: line breaks as the text `#13` and `#10`, to keep a
/// memo's text on one line of the settings ini.
pub fn text_to_string(text: &str) -> String {
    text.replace('\r', "#13").replace('\n', "#10")
}

/// `StringToText`.
pub fn string_to_text(text: &str) -> String {
    text.replace("#13", "\r").replace("#10", "\n")
}

/// `IsPowerOf2`: 0 and 1 are not.
pub fn is_power_of_2(x: u32) -> bool {
    x != 0 && x != 1 && (x & (x - 1)) == 0
}

// ---- System.IniFiles ----

/// Delphi `TMemIniFile`: the settings ini of Sniff, read once. Sections and
/// names ignore case and the first of a repeated one wins; names and values
/// are trimmed, lines before the first section and lines starting with `;`
/// are dropped.
#[derive(Debug, Clone, Default)]
pub struct MemIniFile {
    sections: Vec<(String, Vec<(String, String)>)>,
}

impl MemIniFile {
    /// `TMemIniFile.Create(FileName)`: a missing file gives empty settings.
    pub fn load(path: &Path) -> MemIniFile {
        match std::fs::read(path) {
            Ok(bytes) => MemIniFile::from_text(&string_list_text(&bytes)),
            Err(_) => MemIniFile::default(),
        }
    }

    /// `TMemIniFile.SetStrings`.
    pub fn from_text(text: &str) -> MemIniFile {
        let mut ini = MemIniFile::default();
        let mut current: Option<usize> = None;
        for line in string_list_lines(text) {
            let line = trim(&line);
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
                let name = trim(&line[1..line.len() - 1]).to_owned();
                ini.sections.push((name, Vec::new()));
                current = Some(ini.sections.len() - 1);
            } else if let Some(section) = current {
                let entry = match line.find('=') {
                    Some(index) => (trim(&line[..index]).to_owned(), trim(&line[index + 1..]).to_owned()),
                    None => (line.to_owned(), String::new()),
                };
                ini.sections[section].1.push(entry);
            }
        }
        ini
    }

    fn section(&self, section: &str) -> Option<&Vec<(String, String)>> {
        self.sections
            .iter()
            .find(|(name, _)| ansi_same_text(name, section))
            .map(|(_, values)| values)
    }

    /// `ValueExists`.
    pub fn value_exists(&self, section: &str, ident: &str) -> bool {
        self.section(section)
            .is_some_and(|values| values.iter().any(|(name, _)| ansi_same_text(name, ident)))
    }

    /// `ReadString`.
    pub fn read_string(&self, section: &str, ident: &str, default: &str) -> String {
        self.section(section)
            .and_then(|values| values.iter().find(|(name, _)| ansi_same_text(name, ident)))
            .map_or_else(|| default.to_owned(), |(_, value)| value.clone())
    }

    /// `ReadInteger`: `0x` is read as hexadecimal; a value that is not a
    /// number gives the default.
    pub fn read_integer(&self, section: &str, ident: &str, default: i32) -> i32 {
        let mut text = self.read_string(section, ident, "");
        if text.len() > 2 && text[..2].eq_ignore_ascii_case("0x") {
            text = format!("${}", &text[2..]);
        }
        str_to_int(&text).unwrap_or(default)
    }

    /// `ReadBool`: an integer that is not 0.
    pub fn read_bool(&self, section: &str, ident: &str, default: bool) -> bool {
        self.read_integer(section, ident, i32::from(default)) != 0
    }

    /// `WriteString`: replaces the value or adds it, and the section.
    pub fn write_string(&mut self, section: &str, ident: &str, value: &str) {
        let index = match self.sections.iter().position(|(name, _)| ansi_same_text(name, section)) {
            Some(index) => index,
            None => {
                self.sections.push((section.to_owned(), Vec::new()));
                self.sections.len() - 1
            }
        };
        let values = &mut self.sections[index].1;
        match values.iter_mut().find(|(name, _)| ansi_same_text(name, ident)) {
            Some(entry) => entry.1 = value.to_owned(),
            None => values.push((ident.to_owned(), value.to_owned())),
        }
    }
}

/// Delphi `Trim`: removes the characters up to the space at both ends.
pub fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| c <= ' ')
}

/// The lines of a text as `TStrings.SetTextStr` splits it: at CR, LF or
/// CRLF, with no empty line after a final break.
pub fn string_list_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find(['\r', '\n']) {
            Some(index) => {
                lines.push(rest[..index].to_owned());
                let skip = if rest[index..].starts_with("\r\n") { 2 } else { 1 };
                rest = &rest[index + skip..];
            }
            None => {
                lines.push(rest.to_owned());
                rest = "";
            }
        }
    }
    lines
}

/// `TStrings.Text`: each line followed by CRLF.
pub fn string_list_text_of(lines: &[String]) -> String {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push_str("\r\n");
    }
    text
}

/// `TStringList.SaveToFile`: the lines with CRLF, in the ANSI code page.
pub fn string_list_file_bytes(lines: &[String]) -> Vec<u8> {
    ansi_bytes(&string_list_text_of(lines))
}

/// `TStrings.DelimitedText := Value` with `StrictDelimiter` and the quote
/// character `"`: only the delimiter splits, a field that starts with a
/// quote is read up to its closing quote (`AnsiExtractQuotedStr`), and a
/// delimiter at the end adds an empty field.
pub fn delimited_text(value: &str, delimiter: char) -> Vec<String> {
    set_delimited_text(value, delimiter, true)
}

/// `TStrings.CommaText := Value`: as `delimited_text` with a comma, but
/// without `StrictDelimiter`, so the characters up to the space split too
/// and are skipped around the fields.
pub fn comma_text(value: &str) -> Vec<String> {
    set_delimited_text(value, ',', false)
}

/// `TStrings.CommaText` (the getter): an empty field and a field with a
/// character up to the space, a comma or a quote are quoted.
pub fn comma_text_of(fields: &[String]) -> String {
    let mut text = String::new();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        if field.is_empty() || field.chars().any(|c| c <= ' ' || c == ',' || c == '"') {
            text.push('"');
            text.push_str(&field.replace('"', "\"\""));
            text.push('"');
        } else {
            text.push_str(field);
        }
    }
    text
}

/// `TStrings.SetDelimitedText`.
fn set_delimited_text(value: &str, delimiter: char, strict: bool) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut result = Vec::new();
    let blank = |c: char| ('\u{1}'..=' ').contains(&c);
    let mut p = 0;
    // `PChar` stops at the first #0.
    let end = chars.iter().position(|&c| c == '\0').unwrap_or(chars.len());
    if !strict {
        while p < end && blank(chars[p]) {
            p += 1;
        }
    }
    while p < end {
        let field: String;
        if chars[p] == '"' {
            // AnsiExtractQuotedStr
            let mut text = String::new();
            p += 1;
            while p < end {
                if chars[p] == '"' {
                    if p + 1 < end && chars[p + 1] == '"' {
                        text.push('"');
                        p += 2;
                        continue;
                    }
                    p += 1;
                    break;
                }
                text.push(chars[p]);
                p += 1;
            }
            field = text;
        } else {
            let start = p;
            while p < end && (strict || chars[p] > ' ') && chars[p] != delimiter {
                p += 1;
            }
            field = chars[start..p].iter().collect();
        }
        result.push(field);
        if !strict {
            while p < end && blank(chars[p]) {
                p += 1;
            }
        }
        if p < end && chars[p] == delimiter {
            if p + 1 >= end {
                result.push(String::new());
            }
            p += 1;
            if !strict {
                while p < end && blank(chars[p]) {
                    p += 1;
                }
            }
        }
    }
    result
}

/// `TStrings.DelimitedText` (the getter) with `StrictDelimiter` and the
/// quote character `"`: a field holding the delimiter or the quote is
/// quoted.
pub fn delimited_text_of(fields: &[String], delimiter: char) -> String {
    let mut text = String::new();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            text.push(delimiter);
        }
        if field.contains(delimiter) || field.contains('"') || field.contains('\0') {
            text.push('"');
            text.push_str(&field.replace('"', "\"\""));
            text.push('"');
        } else {
            text.push_str(field);
        }
    }
    text
}

/// `SameText`: case-insensitive for ASCII only.
pub fn same_text(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// `AnsiSameText` (and the case of `TStringList` lookups): Unicode case
/// folding.
pub fn ansi_same_text(a: &str, b: &str) -> bool {
    a == b || a.to_lowercase() == b.to_lowercase()
}

/// `ContainsText`: whether `text` holds `sub`, ignoring case.
pub fn contains_text(text: &str, sub: &str) -> bool {
    text.to_uppercase().contains(&sub.to_uppercase())
}

/// `ExtractFileExt`: from the last `.` after the last path delimiter.
pub fn extract_file_ext(file_name: &str) -> &str {
    match file_name.rfind(['.', '\\', '/', ':']) {
        Some(index) if file_name[index..].starts_with('.') => &file_name[index..],
        _ => "",
    }
}

/// `ExtractFileName`.
pub fn extract_file_name(file_name: &str) -> &str {
    match file_name.rfind(['\\', '/', ':']) {
        Some(index) => &file_name[index + 1..],
        None => file_name,
    }
}

/// `ExtractFilePath`: up to and including the last path delimiter.
pub fn extract_file_path(file_name: &str) -> &str {
    match file_name.rfind(['\\', '/', ':']) {
        Some(index) => &file_name[..=index],
        None => "",
    }
}

/// `ChangeFileExt`.
pub fn change_file_ext(file_name: &str, extension: &str) -> String {
    let ext = extract_file_ext(file_name);
    format!("{}{extension}", &file_name[..file_name.len() - ext.len()])
}

/// `SameValue` of two doubles with the default epsilon: the resolution of
/// `System.Math` (`DoubleResolution`, 1E-15 times the fuzz factor 1000)
/// relative to the smaller value.
pub fn same_value(a: f64, b: f64) -> bool {
    const RESOLUTION: f64 = 1e-12;
    let epsilon = (a.abs().min(b.abs()) * RESOLUTION).max(RESOLUTION);
    if a > b { a - b <= epsilon } else { b - a <= epsilon }
}

/// `SameValue` of two singles with the default epsilon
/// (`SingleResolution`, 1E-7 times 1000): the epsilon is worked out in
/// double precision and the difference in single, as the machine code of
/// the RTL does.
pub fn same_value_single(a: f32, b: f32) -> bool {
    const RESOLUTION: f64 = 1e-4;
    let smaller = if f64::from(b).abs() > f64::from(a).abs() {
        f64::from(a).abs()
    } else {
        f64::from(b).abs()
    };
    let scaled = smaller * RESOLUTION;
    let epsilon = (if scaled > RESOLUTION { scaled } else { RESOLUTION }) as f32;
    if a > b { epsilon >= a - b } else { epsilon >= b - a }
}

/// An exception of a processor where upstream reads through a nil
/// reference: Delphi raises an access violation, whose message names
/// addresses the port does not have. The parity harness takes two access
/// violations as the same error.
pub fn access_violation() -> DfError {
    DfError::new("Access violation: the element does not exist")
}

// ---- TProcFileObject ----

/// Where the files of a run come from: the archive or the folder of the
/// input (`InputArchive`, `InputDirectory`).
pub struct ProcInput {
    /// The archive, when the input is one.
    pub archive: Option<Archive>,
    /// The folder of the input with a final path delimiter, or the path of
    /// the archive.
    pub input_directory: String,
}

/// `TProcFileObject`: a file of the input.
pub struct ProcFileObject<'a> {
    pub input: &'a ProcInput,
    /// The path relative to the input folder, or the name in the archive.
    /// A processor may change it: the output is written under this name.
    pub file_name: String,
    /// The entry of the archive, when the input is one.
    pub file_entry: Option<&'a FileEntry>,
}

impl ProcFileObject<'_> {
    /// `GetData`.
    pub fn get_data(&self) -> R<Vec<u8>> {
        match (self.file_entry, &self.input.archive) {
            (Some(entry), Some(archive)) => archive.unpack_entry(entry).map_err(|error| DfError::new(error.0)),
            _ => {
                let path = format!("{}{}", self.input.input_directory, self.file_name);
                std::fs::read(&path).map_err(|error| DfError::new(format!("Cannot open file \"{path}\". {error}")))
            }
        }
    }

    /// Whether the file comes from an archive (`Assigned(FileEntry)`).
    pub fn in_archive(&self) -> bool {
        self.file_entry.is_some()
    }
}

/// What a processor reports while it processes one file. Upstream adds the
/// messages to the manager at once (`AddMessage`, under a lock) and a
/// processor that collects text over the files adds it to a list of its
/// own; the port commits both in file order, as one thread does upstream.
#[derive(Debug, Default)]
pub struct ProcContext {
    /// `fManager.AddMessage` and `AddMessages`.
    pub messages: Vec<String>,
    /// Lines the processor collects across the files (the log file of the
    /// universal fixer), read back in [`Proc::on_stop`].
    pub proc_log: Vec<String>,
}

impl ProcContext {
    /// `AddMessage`.
    pub fn add_message(&mut self, text: impl Into<String>) {
        self.messages.push(text.into());
    }

    /// `AddMessages`.
    pub fn add_messages<S: Into<String>>(&mut self, lines: impl IntoIterator<Item = S>) {
        self.messages.extend(lines.into_iter().map(Into::into));
    }
}

/// What [`Proc::on_stop`] sees: the collected lines and the manager's
/// messages.
pub struct StopContext<'a> {
    pub messages: &'a mut Vec<String>,
    /// [`ProcContext::proc_log`] of every file, in file order.
    pub proc_log: &'a [String],
    /// A dry run writes no file.
    pub dry_run: bool,
}

// ---- TProcBase ----

/// The fields of `TProcBase`.
#[derive(Debug, Clone)]
pub struct ProcBase {
    /// `GroupID`: the group of the list in the main form.
    pub group: &'static str,
    /// `Title`: the name of the operation (`-OP:`).
    pub title: &'static str,
    /// `SupportedGames`.
    pub supported_games: &'static [GameType],
    /// The extensions without a dot, in lower case (`fExtensions`).
    pub extensions: Vec<String>,
    /// `NoOutput`: the processor only reports; no file is written.
    pub no_output: bool,
    /// `Threads`: 0 for the count the user gives, else the count the
    /// processor needs.
    pub threads: i32,
}

impl ProcBase {
    /// `TProcBase.Create` with the fields every constructor sets.
    pub fn new(title: &'static str, supported_games: &'static [GameType], extensions: &[&str]) -> ProcBase {
        ProcBase {
            group: "",
            title,
            supported_games,
            extensions: extensions.iter().map(|ext| (*ext).to_owned()).collect(),
            no_output: false,
            threads: 0,
        }
    }

    /// `GetSupportedGameNames`.
    pub fn supported_game_names(&self) -> String {
        self.supported_games
            .iter()
            .map(|game| game.name())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// `GetExtensionNames`.
    pub fn extension_names(&self) -> String {
        self.extensions
            .iter()
            .map(|ext| format!("*.{ext}"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// `SetExtensionNames`: the extensions of a comma separated list of
    /// masks such as `*.nif, *.kf`.
    pub fn set_extension_names(&mut self, extensions: &str) {
        let mut new = Vec::new();
        for mask in extensions.split(',') {
            let mut ext = xedit_io::encoding::lower_case(extract_file_ext(trim(mask)));
            if ext.is_empty() {
                continue;
            }
            if ext.starts_with('.') {
                ext.remove(0);
            }
            new.push(ext);
        }
        self.extensions = new;
    }

    /// `IsAcceptedFile`.
    pub fn is_accepted_file(&self, file_name: &str) -> bool {
        let mut ext = xedit_io::encoding::lower_case(extract_file_ext(file_name));
        if !ext.is_empty() {
            ext.remove(0);
        }
        self.extensions.iter().any(|known| *known == ext)
    }

    /// `GetStorageSection`: the title without spaces.
    pub fn storage_section(&self) -> String {
        self.title.replace(' ', "")
    }
}

/// The kind of a stored setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionKind {
    Bool,
    Integer,
    String,
}

/// A setting a processor reads in [`Proc::on_show`], with the default of
/// its control.
#[derive(Debug, Clone)]
pub struct ProcOptionInfo {
    pub name: String,
    pub kind: OptionKind,
    pub default: String,
}

/// The storage section of a processor in the settings
/// (`StorageGetBool` and the rest). It can record what is read, which is
/// how the list of a processor's settings is made.
pub struct Storage<'a> {
    section: String,
    settings: Option<&'a MemIniFile>,
    record: Option<RefCell<Vec<ProcOptionInfo>>>,
}

impl<'a> Storage<'a> {
    pub fn new(section: String, settings: Option<&'a MemIniFile>) -> Storage<'a> {
        Storage {
            section,
            settings,
            record: None,
        }
    }

    /// A storage without settings that records each read.
    pub fn recording(section: String) -> Storage<'a> {
        Storage {
            section,
            settings: None,
            record: Some(RefCell::new(Vec::new())),
        }
    }

    /// The settings read so far by a recording storage.
    pub fn recorded(self) -> Vec<ProcOptionInfo> {
        self.record.map(RefCell::into_inner).unwrap_or_default()
    }

    fn note(&self, name: &str, kind: OptionKind, default: String) {
        if let Some(record) = &self.record {
            let mut record = record.borrow_mut();
            if !record.iter().any(|option| option.name == name) {
                record.push(ProcOptionInfo {
                    name: name.to_owned(),
                    kind,
                    default,
                });
            }
        }
    }

    /// `StorageGetBool`.
    pub fn get_bool(&self, name: &str, default: bool) -> bool {
        self.note(name, OptionKind::Bool, i32::from(default).to_string());
        match self.settings {
            Some(settings) => settings.read_bool(&self.section, name, default),
            None => default,
        }
    }

    /// `StorageGetInteger`.
    pub fn get_integer(&self, name: &str, default: i32) -> i32 {
        self.note(name, OptionKind::Integer, default.to_string());
        match self.settings {
            Some(settings) => settings.read_integer(&self.section, name, default),
            None => default,
        }
    }

    /// `StorageGetString`.
    pub fn get_string(&self, name: &str, default: &str) -> String {
        self.note(name, OptionKind::String, default.to_owned());
        match self.settings {
            Some(settings) => settings.read_string(&self.section, name, default),
            None => default.to_owned(),
        }
    }
}

/// A Sniff operation: `TProcBase` and its virtual methods.
pub trait Proc: Send + Sync {
    fn base(&self) -> &ProcBase;
    fn base_mut(&mut self) -> &mut ProcBase;

    /// `OnShow`: sets the controls of the frame from the storage, with the
    /// defaults of the `.dfm`.
    fn on_show(&mut self, _storage: &Storage) {}

    /// `OnStart`: reads and checks the controls before the run. An error
    /// stops the run before any file.
    fn on_start(&mut self) -> R<()> {
        Ok(())
    }

    /// `ProcessFile`: the new contents of the file, or nothing when it is
    /// unchanged.
    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>>;

    /// `OnStop`: after the last file. Upstream ignores its exceptions.
    fn on_stop(&mut self, _ctx: &mut StopContext) -> R<()> {
        Ok(())
    }

    /// `OnHide`: restores what [`Proc::on_start`] changed for the process
    /// (the float format of the JSON converter).
    fn on_hide(&mut self) {}
}

/// Implements [`Proc::base`] and [`Proc::base_mut`] on a `base` field.
#[macro_export]
macro_rules! proc_base {
    () => {
        fn base(&self) -> &$crate::sniff::processor::ProcBase {
            &self.base
        }
        fn base_mut(&mut self) -> &mut $crate::sniff::processor::ProcBase {
            &mut self.base
        }
    };
}

// ---- TProcManager ----

/// What happened to one file (the line `Process` adds to the messages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// The processor changed it and it was written.
    Updated,
    /// Unchanged, and written because every file is copied.
    Unchanged,
    /// Unchanged and not written, or the processor only reports.
    Untouched,
    /// The processor failed on it and skipping on errors is on.
    Skipped(String),
}

/// `TProcManager`: the state of a run and the step that follows
/// `ProcessFile` for each file.
pub struct ProcManager {
    /// `Settings`.
    pub settings: Option<MemIniFile>,
    /// `Messages`.
    pub messages: Vec<String>,
    /// The folders made for the outputs (`fDirectories`).
    directories: std::collections::HashSet<String>,
    /// `OutputDirectory`, with a final path delimiter.
    pub output_directory: String,
    /// `CopyAll`: write the unchanged files too.
    pub copy_all: bool,
    /// `SkipOnErrors`.
    pub skip_on_errors: bool,
    /// `ModifiedCount`.
    pub modified_count: usize,
    /// `ProcessedCount`.
    pub processed_count: usize,
    /// Write nothing: report what would be written.
    pub dry_run: bool,
    /// The lines of [`ProcContext::proc_log`], in file order.
    pub proc_log: Vec<String>,
    /// Receives the outputs instead of the files being written.
    pub sink: Option<crate::sniff::main_form::OutputSink>,
}

impl ProcManager {
    /// `TProcManager.Create` and `SetIniFile`.
    pub fn new(settings: Option<MemIniFile>) -> ProcManager {
        ProcManager {
            settings,
            messages: Vec::new(),
            directories: std::collections::HashSet::new(),
            output_directory: String::new(),
            copy_all: false,
            skip_on_errors: false,
            modified_count: 0,
            processed_count: 0,
            dry_run: false,
            proc_log: Vec::new(),
            sink: None,
        }
    }

    /// `InitializeProcessing`.
    pub fn initialize_processing(&mut self) {
        self.messages.clear();
        self.directories.clear();
        self.modified_count = 0;
        self.processed_count = 0;
        self.proc_log.clear();
    }

    /// `AddMessage`.
    pub fn add_message(&mut self, text: impl Into<String>) {
        self.messages.push(text.into());
    }

    /// `CreateDirectory`.
    fn create_directory(&mut self, path: &str) -> R<()> {
        if self.directories.contains(path) {
            return Ok(());
        }
        std::fs::create_dir_all(path).map_err(|_| DfError::new(format!("Can not create directory: {path}")))?;
        self.directories.insert(path.to_owned());
        Ok(())
    }

    /// `Process` after `ProcessFile`: counts the file, writes its output
    /// and adds its line to the messages. `result` is what `ProcessFile`
    /// returned for `file`; an error that is not skipped is returned and
    /// stops the run.
    pub fn process(
        &mut self,
        proc: &dyn Proc,
        file: &ProcFileObject,
        ctx: ProcContext,
        result: R<Vec<u8>>,
    ) -> R<FileStatus> {
        self.messages.extend(ctx.messages);
        self.proc_log.extend(ctx.proc_log);
        let mut data = match result {
            Ok(data) => data,
            Err(error) => {
                if !self.skip_on_errors {
                    return Err(error);
                }
                self.processed_count += 1;
                let message = error.0;
                self.add_message(format!("Skipped: {}: {message}", file.file_name));
                return Ok(FileStatus::Skipped(message));
            }
        };
        self.processed_count += 1;

        if proc.base().no_output {
            return Ok(FileStatus::Untouched);
        }

        // The file has been changed.
        let updated = !data.is_empty();

        // If not changed but every file is copied, the original.
        if !updated && self.copy_all {
            data = file.get_data()?;
        }

        // Nothing to save.
        if data.is_empty() {
            return Ok(FileStatus::Untouched);
        }

        // Saving the output file.
        let out_file = format!("{}{}", self.output_directory, file.file_name);
        if let Some(sink) = &self.sink {
            (sink.0)(&file.file_name, &data);
        } else if !self.dry_run {
            let dir = extract_file_path(&out_file).to_owned();
            if dir != self.output_directory {
                self.create_directory(&dir)?;
            }
            std::fs::write(&out_file, &data)
                .map_err(|error| DfError::new(format!("Cannot create file \"{out_file}\". {error}")))?;
        }

        if updated {
            self.add_message(format!("Updated: {}", file.file_name));
            self.modified_count += 1;
            Ok(FileStatus::Updated)
        } else {
            self.add_message(format!("Unchanged: {}", file.file_name));
            Ok(FileStatus::Unchanged)
        }
    }

    /// `Messages.SaveToFile`.
    pub fn messages_bytes(&self) -> Vec<u8> {
        string_list_file_bytes(&self.messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_reads_as_tmeminifile() {
        let ini = MemIniFile::from_text(
            "skipped=1\r\n[Main]\r\n  PopupWarning = 0 \r\n;x=1\r\n[Universal tweaker]\r\nsValue=a=b\r\niMode=0x10\r\nbOn=yes\r\n[main]\r\nPopupWarning=1\r\n",
        );
        assert_eq!(ini.read_string("MAIN", "popupwarning", "x"), "0");
        assert!(!ini.read_bool("Main", "PopupWarning", true));
        assert_eq!(ini.read_string("Universal tweaker", "sValue", ""), "a=b");
        assert_eq!(ini.read_integer("Universal tweaker", "iMode", 0), 16);
        // Not a number: the default.
        assert!(!ini.read_bool("Universal tweaker", "bOn", false));
        assert_eq!(ini.read_string("Main", "skipped", "-"), "-");
        assert!(ini.value_exists("Main", "PopupWarning"));
    }

    #[test]
    fn delimited_text_splits_as_tstrings() {
        let split = |text: &str| delimited_text(text, ',');
        assert_eq!(split(""), Vec::<String>::new());
        assert_eq!(split("a, b"), vec!["a", " b"]);
        assert_eq!(split("a,"), vec!["a", ""]);
        assert_eq!(split("\"a,b\",c"), vec!["a,b", "c"]);
        assert_eq!(split("\"a\"\"b\"x,c"), vec!["a\"b", "x", "c"]);
        assert_eq!(delimited_text("1 2  3", ' '), vec!["1", "2", "", "3"]);
        assert_eq!(delimited_text_of(&["1".into(), "a b".into()], ' '), "1 \"a b\"");
        let fields = comma_text(" \"0 Other=\", \"1 Head=2\",3 x=4");
        assert_eq!(fields, vec!["0 Other=", "1 Head=2", "3", "x=4"]);
        assert_eq!(comma_text_of(&fields[..2]), "\"0 Other=\",\"1 Head=2\"");
    }

    #[test]
    fn extensions_and_names() {
        let mut base = ProcBase::new("Universal tweaker", GameType::ALL, &["nif", "kf"]);
        assert_eq!(base.extension_names(), "*.nif, *.kf");
        assert_eq!(base.storage_section(), "Universaltweaker");
        assert!(base.is_accepted_file("meshes\\a.NIF"));
        assert!(!base.is_accepted_file("meshes\\a.nif.json"));
        base.set_extension_names(" *.BGSM, , *.dds");
        assert_eq!(base.extensions, vec!["bgsm", "dds"]);
        assert_eq!(change_file_ext("a\\b.nif.json", ""), "a\\b.nif");
        assert_eq!(extract_file_ext("a.b\\c"), "");
        assert_eq!(text_to_string("a\r\nb"), "a#13#10b");
        assert_eq!(string_to_text("a#13#10b"), "a\r\nb");
        assert_eq!(string_list_lines("a\r\nb\nc\r\n"), vec!["a", "b", "c"]);
        assert!(is_power_of_2(4) && !is_power_of_2(1) && !is_power_of_2(6));
    }
}
