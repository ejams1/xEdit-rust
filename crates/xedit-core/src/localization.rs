// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbLocalization.pas

//! The string tables of localized plugins, upstream `wbLocalizationHandler`:
//! the `.STRINGS`, `.DLSTRINGS` and `.ILSTRINGS` files, loaded through the
//! containers on the first lookup for a plugin, the strings an edit adds or
//! changes (`SetValue`, `AddValue`), the tables written on save
//! (`WriteToStream`), their export (`ExportToFile`), the languages and
//! tables the containers offer, and the language switch (`Clear` with its
//! generation).
//!
//! The encodings of the languages (upstream `wbLEncoding` and
//! `wbEncodingForLanguage` of `wbInterface.pas`) live here as well.

use std::cmp::Ordering as CmpOrdering;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use xedit_io::Encoding;

use crate::container_handler::{container_resource_list, has_containers, open_resource_last};
use crate::delphi::{change_file_ext, path_file_name};
use crate::interface::globals::{data_path, language};
use crate::interface::misc::{LocalizationHandler, progress, set_localization_handler};
use crate::interface::{ElementArg, Signature};

/// Upstream `sStringID`: the prefix of a string ID given as text.
pub use crate::interface::string::STRING_ID_PREFIX;

/// Upstream `TwbLStringType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LStringType {
    DLString,
    ILString,
    String,
}

impl LStringType {
    pub const ALL: [LStringType; 3] = [LStringType::DLString, LStringType::ILString, LStringType::String];

    /// Upstream `wbLocalizationExtension`.
    pub fn extension(self) -> &'static str {
        match self {
            LStringType::DLString => ".DLSTRINGS",
            LStringType::ILString => ".ILSTRINGS",
            LStringType::String => ".STRINGS",
        }
    }
}

/// Upstream `wbLEncoding[aFallback]` and `wbLEncodingDefault[aFallback]`:
/// the encoding of the strings of each language, and the one of the other
/// languages. Index 0 is the primary encoding, 1 the fallback.
struct LanguageEncodings {
    by_language: [Vec<(String, Encoding)>; 2],
    default: [Encoding; 2],
}

static L_ENCODINGS: Mutex<LanguageEncodings> = Mutex::new(LanguageEncodings {
    by_language: [Vec::new(), Vec::new()],
    default: [Encoding::Utf8, Encoding::Mbcs(1252)],
});

/// Port of `wbAddLEncodingIfMissing`. The language is matched ignoring case.
pub fn add_l_encoding_if_missing(language: &str, encoding: Encoding, fallback: bool) {
    if language.is_empty() {
        return;
    }
    let mut encodings = L_ENCODINGS.lock().unwrap();
    let list = &mut encodings.by_language[fallback as usize];
    if !list.iter().any(|(known, _)| known.eq_ignore_ascii_case(language)) {
        list.push((language.to_owned(), encoding));
    }
}

/// Port of `wbAddDefaultLEncodingsIfMissing`.
pub fn add_default_l_encodings_if_missing(fallback: bool) {
    for (language, code_page) in [
        ("english", 1252),
        ("french", 1252),
        ("polish", 1250),
        ("czech", 1250),
        ("danish", 1252),
        ("finnish", 1252),
        ("german", 1252),
        ("greek", 1253),
        ("italian", 1252),
        ("norwegian", 1252),
        ("spanish", 1252),
        ("swedish", 1252),
        ("turkish", 1254),
        ("russian", 1251),
        ("chinese", 936),
        ("hungarian", 1250),
        ("arabic", 1256),
        ("japanese", 65001),
    ] {
        let encoding = if code_page == 65001 {
            Encoding::Utf8
        } else {
            Encoding::Mbcs(code_page)
        };
        add_l_encoding_if_missing(language, encoding, fallback);
    }
}

/// Port of `wbLEncodingDefault[aFallback] := ...`.
pub fn set_l_encoding_default(encoding: Encoding, fallback: bool) {
    L_ENCODINGS.lock().unwrap().default[fallback as usize] = encoding;
}

/// Port of `wbEncodingForLanguage`.
pub fn encoding_for_language(language: &str, fallback: bool) -> Encoding {
    let encodings = L_ENCODINGS.lock().unwrap();
    encodings.by_language[fallback as usize]
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(language))
        .map(|(_, encoding)| *encoding)
        .unwrap_or(encodings.default[fallback as usize])
}

/// Port of `wbMBCSEncoding(s)`: `utf-8`, `utf8`, a code page or
/// `windows-<code page>`; `None` where upstream's `StrToInt` raises.
pub fn mbcs_encoding(name: &str) -> Option<Encoding> {
    if name.eq_ignore_ascii_case("utf-8") || name.eq_ignore_ascii_case("utf8") {
        return Some(Encoding::Utf8);
    }
    let code_page = name.strip_prefix("windows-").unwrap_or(name);
    // `StrToInt`: decimal, or hexadecimal after `$` or `0x`.
    let code_page = match code_page
        .strip_prefix('$')
        .or_else(|| code_page.strip_prefix("0x"))
        .or_else(|| code_page.strip_prefix("0X"))
    {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => code_page.parse::<i64>().ok()? as u32,
    };
    Some(if code_page == 65001 {
        Encoding::Utf8
    } else {
        Encoding::Mbcs(code_page)
    })
}

/// Delphi `CompareText` without the locale (`TwbFastStringList` sets
/// `UseLocale := False`): the letters `a` to `z` compare as upper case, the
/// rest by their UTF-16 code unit.
pub fn compare_text(a: &str, b: &str) -> CmpOrdering {
    let fold = |unit: u16| {
        if (u16::from(b'a')..=u16::from(b'z')).contains(&unit) {
            unit - 32
        } else {
            unit
        }
    };
    a.encode_utf16().map(fold).cmp(b.encode_utf16().map(fold))
}

/// Port of `TwbLocalizationFile`: the strings of one table, in the order of
/// the file, with their IDs.
pub struct LocalizationFile {
    /// Upstream `fName`: the file name without the path.
    name: String,
    /// Upstream `fFileName`: where the table is read from and saved to,
    /// `<data>\Strings\<name>` for the tables of a plugin.
    file_name: String,
    file_type: LStringType,
    /// Upstream `fEncoding[False]` and `fEncoding[True]`; the fallback is
    /// `None` when it is the same as the primary encoding.
    encoding: Encoding,
    fallback_encoding: Option<Encoding>,
    /// Upstream `fStrings`: the text with the ID as its object.
    strings: Vec<(u32, String)>,
    /// The position of the first entry of each ID (`IndexOfObject`).
    index: HashMap<u32, usize>,
    modified: bool,
    next_id: u32,
}

impl LocalizationFile {
    /// Port of `Create(aFileName, aData)` with `Init` and `ReadDirectory`.
    pub fn new(file_name: &str, data: &[u8]) -> Self {
        let mut file = Self::init(file_name);
        file.read_directory(data);
        file
    }

    /// Port of `Create(aFileName)`: the table read from the file on disk.
    pub fn open(file_name: &str) -> std::io::Result<Self> {
        let data = std::fs::read(file_name)?;
        Ok(Self::new(file_name, &data))
    }

    /// Port of `Init`: the name, the language after the last `_` of the
    /// name, the encoding (from a `.cpoverride` file next to the table, else
    /// from the language), the table type and the first free ID.
    fn init(file_name: &str) -> Self {
        let name = path_file_name(file_name).to_owned();
        let stem = change_file_ext(&name, "");
        let language = stem
            .rsplit_once('_')
            .map(|(_, language)| language)
            .unwrap_or("")
            .to_owned();
        let override_encoding = Self::code_page_override(file_name);
        let (encoding, mut text) = match override_encoding {
            Some(encoding) => (
                encoding,
                format!("[{name}] Using encoding (from override): {}", encoding.name()),
            ),
            None => {
                let encoding = encoding_for_language(&language, false);
                (
                    encoding,
                    format!("[{name}] Using encoding (from language): {}", encoding.name()),
                )
            }
        };
        let fallback_encoding = Some(encoding_for_language(&language, true)).filter(|fallback| *fallback != encoding);
        if let Some(fallback) = fallback_encoding {
            text.push_str(&format!(" with fallback (from languange) to: {}", fallback.name()));
        }
        progress(&text);
        LocalizationFile {
            file_type: Self::file_string_type(&name),
            name,
            file_name: file_name.to_owned(),
            encoding,
            fallback_encoding,
            strings: Vec::new(),
            index: HashMap::new(),
            modified: false,
            next_id: 1,
        }
    }

    /// The first line of `<table>.cpoverride` as an encoding; upstream
    /// ignores the file when it can not be read or names no code page.
    fn code_page_override(file_name: &str) -> Option<Encoding> {
        let path = change_file_ext(file_name, ".cpoverride");
        let bytes = std::fs::read(&path).ok()?;
        let text = String::from_utf8_lossy(&bytes);
        let first = text.trim_start_matches('\u{feff}').lines().next()?.trim();
        if first.is_empty() {
            return None;
        }
        mbcs_encoding(first)
    }

    /// Port of `FileStringType`.
    fn file_string_type(file_name: &str) -> LStringType {
        let extension = match file_name.rfind('.') {
            Some(dot) => &file_name[dot..],
            None => "",
        };
        LStringType::ALL
            .into_iter()
            .find(|kind| kind.extension().eq_ignore_ascii_case(extension))
            .unwrap_or(LStringType::String)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Upstream `FileName`.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn file_type(&self) -> LStringType {
        self.file_type
    }

    pub fn modified(&self) -> bool {
        self.modified
    }

    pub fn set_modified(&mut self, value: bool) {
        self.modified = value;
    }

    /// Upstream `NextID`.
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    pub fn count(&self) -> usize {
        self.strings.len()
    }

    /// Upstream `Items`: the strings with their IDs, in the order of the table.
    pub fn items(&self) -> &[(u32, String)] {
        &self.strings
    }

    /// Port of `Find`: the string, or the error text in its place.
    pub fn find(&self, id: u32) -> (bool, String) {
        match self.index.get(&id) {
            Some(&position) => (true, self.strings[position].1.clone()),
            None => (false, format!("<Error: Unknown lstring ID {id:08X}>")),
        }
    }

    /// Port of `Get` (`Strings[ID]`): the string, or the error text.
    pub fn get(&self, id: u32) -> String {
        self.find(id).1
    }

    /// Port of `Put` (`Strings[ID] := S`): an existing string changes, and
    /// the table is modified when the text differs.
    pub fn put(&mut self, id: u32, text: &str) {
        if let Some(&position) = self.index.get(&id)
            && self.strings[position].1 != text
        {
            self.strings[position].1 = text.to_owned();
            self.modified = true;
        }
    }

    /// Port of `IndexToID`.
    pub fn index_to_id(&self, index: usize) -> u32 {
        self.strings.get(index).map_or(0, |(id, _)| *id)
    }

    /// Port of `IDExists`.
    pub fn id_exists(&self, id: u32) -> bool {
        self.index.contains_key(&id)
    }

    /// Port of `AddString`: a string with a new ID at the end of the table;
    /// refused for an ID below `NextID`.
    pub fn add_string(&mut self, id: u32, text: &str) -> bool {
        if id < self.next_id {
            return false;
        }
        self.push(id, text.to_owned());
        self.next_id = id.wrapping_add(1);
        self.modified = true;
        true
    }

    fn push(&mut self, id: u32, text: String) {
        self.index.entry(id).or_insert(self.strings.len());
        self.strings.push((id, text));
    }

    /// The position of the first string with this text, compared as
    /// `TStringList.IndexOf` of an unsorted `TwbFastStringList` does
    /// (`CompareText`, so the letters `a` to `z` ignoring case).
    fn index_of_text(&self, text: &str) -> Option<usize> {
        self.strings
            .iter()
            .position(|(_, string)| compare_text(string, text) == CmpOrdering::Equal)
    }

    /// Port of `ReadDirectory`: the count, the data size, the directory of
    /// (ID, offset) pairs and the data block. Entries that point outside the
    /// file read as empty strings, as the unchecked upstream reads would.
    fn read_directory(&mut self, data: &[u8]) {
        if data.len() < 8 {
            return;
        }
        let count = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
        let data_start = 8 + count * 8;
        for index in 0..count {
            let entry = 8 + index * 8;
            let Some(bytes) = data.get(entry..entry + 8) else {
                break;
            };
            let id = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            let offset = u32::from_le_bytes(bytes[4..].try_into().unwrap()) as usize;
            let string = match self.file_type {
                LStringType::String => self.read_z_string(data, data_start + offset),
                LStringType::DLString | LStringType::ILString => self.read_len_z_string(data, data_start + offset),
            };
            self.push(id, string);
            if id.wrapping_add(1) > self.next_id {
                self.next_id = id.wrapping_add(1);
            }
        }
    }

    /// Port of `ReadZString`: the bytes up to the NUL or the end of the data.
    fn read_z_string(&self, data: &[u8], position: usize) -> String {
        let bytes = data.get(position..).unwrap_or_default();
        let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len());
        self.decode(&bytes[..end])
    }

    /// Port of `ReadLenZString`: a length (including the NUL) in front of the bytes.
    fn read_len_z_string(&self, data: &[u8], position: usize) -> String {
        let Some(length) = data.get(position..position + 4) else {
            return String::new();
        };
        let length = i32::from_le_bytes(length.try_into().unwrap()) - 1;
        if length <= 0 {
            return String::new();
        }
        let start = position + 4;
        let bytes = data.get(start..start + length as usize).unwrap_or_default();
        self.decode(bytes)
    }

    /// The decoding with the fallback encoding when the primary one fails.
    fn decode(&self, bytes: &[u8]) -> String {
        if bytes.is_empty() {
            return String::new();
        }
        match self.encoding.get_string(bytes) {
            Ok(string) => string,
            Err(error) => match self.fallback_encoding {
                Some(fallback) => fallback.get_string(bytes).unwrap_or_default(),
                // UPSTREAM-QUIRK: upstream raises; the value is left empty here.
                None => {
                    progress(&format!("[{}] {error}", self.name));
                    String::new()
                }
            },
        }
    }

    /// Port of `WriteToStream`: the count, the size of the data, the
    /// directory of (ID, offset) pairs in the order of the table and the
    /// data, each string written again in the primary encoding (no string is
    /// shared between IDs, as the game's own tables may share them).
    pub fn write_to_bytes(&self) -> Vec<u8> {
        let mut directory = Vec::with_capacity(8 + self.strings.len() * 8);
        let mut data = Vec::new();
        directory.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        directory.extend_from_slice(&0u32.to_le_bytes());
        for (id, text) in &self.strings {
            directory.extend_from_slice(&id.to_le_bytes());
            directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
            let bytes = self.encoding.get_bytes(text);
            if self.file_type == LStringType::String {
                // `WriteZString`.
                data.extend_from_slice(&bytes);
            } else {
                // `WriteLenZString`: the length counts the NUL.
                data.extend_from_slice(&(bytes.len() as u32 + 1).to_le_bytes());
                data.extend_from_slice(&bytes);
            }
            data.push(0);
        }
        directory[4..8].copy_from_slice(&(data.len() as u32).to_le_bytes());
        directory.extend_from_slice(&data);
        directory
    }

    /// Port of `ExportToFile`: each string as a line `[<ID>]` and its text,
    /// written as `TStringList.SaveToFile` writes them: every line ends with
    /// CR LF, in the system's ANSI code page (`TEncoding.Default`), without
    /// a byte order mark.
    pub fn export_text(&self) -> Vec<u8> {
        let mut text = String::new();
        for (id, string) in &self.strings {
            text.push_str(&format!("[{id:08X}]\r\n"));
            text.push_str(string);
            text.push_str("\r\n");
        }
        // Code page 0 is `CP_ACP`.
        Encoding::Mbcs(0).get_bytes(&text)
    }
}

/// Port of `TwbLocalizationHandler`: the loaded tables by file name.
#[derive(Default)]
pub struct LocalizationHandlerImpl {
    /// Upstream `lFiles`, a `TwbFastStringListIC` sorted by file name
    /// ignoring case.
    files: RwLock<Vec<LocalizationFile>>,
    /// Held while the tables of a plugin load, so that two threads that look
    /// up a string of the same plugin load its tables once.
    loading: Mutex<()>,
    /// Upstream `NoTranslate`: show the IDs instead of the strings.
    no_translate: AtomicBool,
    /// Upstream `ReuseDup`: a new string with the text of an existing one
    /// of its table takes that string's ID. Off, as upstream never sets it.
    reuse_dup: AtomicBool,
}

impl LocalizationHandlerImpl {
    /// Upstream `Generation`.
    pub fn generation(&self) -> i32 {
        GENERATION.load(Ordering::Relaxed)
    }

    pub fn no_translate(&self) -> bool {
        self.no_translate.load(Ordering::Relaxed)
    }

    pub fn set_no_translate(&self, value: bool) {
        self.no_translate.store(value, Ordering::Relaxed);
    }

    pub fn reuse_dup(&self) -> bool {
        self.reuse_dup.load(Ordering::Relaxed)
    }

    pub fn set_reuse_dup(&self, value: bool) {
        self.reuse_dup.store(value, Ordering::Relaxed);
    }

    /// Port of `Clear`: every table is dropped and the generation counts up.
    pub fn clear(&self) {
        self.files.write().unwrap().clear();
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }

    /// Port of `Count`.
    pub fn count(&self) -> usize {
        self.files.read().unwrap().len()
    }

    /// Reads the table at `index` (upstream `_Files[Index]`).
    pub fn with_file<T>(&self, index: usize, f: impl FnOnce(&LocalizationFile) -> T) -> Option<T> {
        self.files.read().unwrap().get(index).map(f)
    }

    /// Changes the table at `index`.
    pub fn with_file_mut<T>(&self, index: usize, f: impl FnOnce(&mut LocalizationFile) -> T) -> Option<T> {
        self.files.write().unwrap().get_mut(index).map(f)
    }

    /// Upstream `lFiles.Find` (and `IndexOf`, which a sorted list answers
    /// with `Find`): the position of the table with this file name.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        Self::find_in(&self.files.read().unwrap(), name).ok()
    }

    fn find_in(files: &[LocalizationFile], name: &str) -> Result<usize, usize> {
        files.binary_search_by(|file| compare_text(&file.name, name))
    }

    /// Port of `LocalizedValueDecider`.
    pub fn localized_value_decider(element: ElementArg) -> LStringType {
        let sig_element = element.and_then(|element| element.get_record_signature());
        let sig_record = element
            .and_then(|element| element.get_containing_main_record())
            .map(|record| record.get_signature());
        let is = |signature: Option<Signature>, text: &[u8; 4]| signature == Some(Signature::new(text));
        // DESC always from dlstrings except LSCR; the quest log entry and the
        // book description too.
        if (!is(sig_record, b"LSCR") && is(sig_element, b"DESC"))
            || (is(sig_record, b"QUST") && is(sig_element, b"CNAM"))
            || (is(sig_record, b"BOOK") && is(sig_element, b"CNAM"))
        {
            LStringType::DLString
        } else if is(sig_record, b"INFO") && !is(sig_element, b"RNAM") {
            LStringType::ILString
        } else {
            LStringType::String
        }
    }

    /// Port of `GetStringsPath`.
    pub fn strings_path() -> String {
        format!("{}Strings\\", data_path())
    }

    /// Port of `AvailableLanguages`: the part after the last `_` of the name
    /// of every table the containers hold, with its first letter upper case,
    /// once each.
    pub fn available_languages() -> Vec<String> {
        let mut languages: Vec<String> = Vec::new();
        let mut parse = |name: &str| {
            let Some(position) = name.rfind('_') else {
                return;
            };
            let language = &name[position + 1..];
            let mut chars = language.chars();
            let Some(first) = chars.next() else {
                return;
            };
            let language: String = first.to_uppercase().chain(chars).collect();
            // `TStringList.IndexOf` ignoring case.
            if !languages
                .iter()
                .any(|known| compare_text(known, &language) == CmpOrdering::Equal)
            {
                languages.push(language);
            }
        };
        if has_containers() {
            for name in container_resource_list("strings") {
                if name.to_ascii_lowercase().ends_with("strings") {
                    parse(&change_file_ext(&name, "").to_lowercase());
                }
            }
        } else {
            for name in Self::strings_folder_files() {
                parse(&change_file_ext(&name, "").to_lowercase());
            }
        }
        languages
    }

    /// Port of `AvailableLocalizationFiles`: the names of the tables the
    /// containers hold (a table in several containers is listed for each).
    pub fn available_localization_files() -> Vec<String> {
        if has_containers() {
            container_resource_list("strings")
                .into_iter()
                .filter(|name| name.to_ascii_lowercase().ends_with("strings"))
                .map(|name| path_file_name(&name).to_owned())
                .collect()
        } else {
            Self::strings_folder_files()
        }
    }

    /// `FindFirst(StringsPath + '*.*STRINGS')`.
    fn strings_folder_files() -> Vec<String> {
        std::fs::read_dir(Self::strings_path())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.to_ascii_uppercase().ends_with("STRINGS") && name.contains('.'))
            .collect()
    }

    /// Port of `GetLocalizationFileNameByType`: the path relative to the data folder.
    pub fn localization_file_name_by_type(plugin_file: &str, kind: LStringType) -> String {
        format!(
            "Strings\\{}_{}{}",
            change_file_ext(plugin_file, ""),
            language(),
            kind.extension()
        )
    }

    /// Port of `GetLocalizationFileNameByElement`.
    pub fn localization_file_name_by_element(element: ElementArg) -> String {
        let Some(file) = element.and_then(|element| element.get_file()) else {
            return String::new();
        };
        Self::localization_file_name_by_type(&file.get_name(), Self::localized_value_decider(element))
    }

    /// Port of `AddLocalization(aFileName, aData)`: the table of that name,
    /// read from `data` when it is not loaded yet; its position.
    pub fn add_localization(&self, file_name: &str, data: &[u8]) -> usize {
        let mut files = self.files.write().unwrap();
        Self::add_to(&mut files, file_name, data)
    }

    fn add_to(files: &mut Vec<LocalizationFile>, file_name: &str, data: &[u8]) -> usize {
        let name = path_file_name(file_name);
        match Self::find_in(files, name) {
            Ok(index) => index,
            Err(index) => {
                files.insert(index, LocalizationFile::new(file_name, data));
                index
            }
        }
    }

    /// Port of `LoadForFile`: the three tables of the plugin from the
    /// containers. Upstream loads nothing while there are no containers.
    pub fn load_for_file(&self, plugin_file: &str) {
        let _loading = self.loading.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for kind in LStringType::ALL {
            let path = Self::localization_file_name_by_type(plugin_file, kind);
            if self.index_of(path_file_name(&path)).is_some() {
                continue;
            }
            if let Some(data) = open_resource_last(&path) {
                self.add_localization(&format!("{}{path}", data_path()), &data);
            }
        }
    }

    /// Port of `GetValue`.
    pub fn get_value(&self, id: u32, element: ElementArg) -> (bool, String) {
        if self.no_translate() {
            return (true, format!("{id:08X}"));
        }
        if id == 0 {
            return (true, String::new());
        }
        let path = Self::localization_file_name_by_element(element);
        let file_name = path_file_name(&path);
        if file_name.is_empty() {
            return (false, String::new());
        }
        {
            let files = self.files.read().unwrap();
            if let Ok(index) = Self::find_in(&files, file_name) {
                return files[index].find(id);
            }
        }
        // Load the strings files when they are absent.
        if let Some(plugin) = element.and_then(|element| element.get_file()) {
            self.load_for_file(&plugin.get_name());
        }
        let files = self.files.read().unwrap();
        match Self::find_in(&files, file_name) {
            Ok(index) => files[index].find(id),
            Err(_) => (false, format!("<Error: No strings file for lstring ID {id:08X}>")),
        }
    }

    /// Port of `AddValue`: a new string for the element in the table of its
    /// type. The three tables of its plugin are made, empty and modified,
    /// when they are not loaded (also when the containers hold them: upstream
    /// does not load them here); the ID is the highest `NextID` of the
    /// three, so that an ID is never in two tables of a plugin. An empty
    /// text is no string, ID 0.
    pub fn add_value(&self, value: &str, element: ElementArg) -> u32 {
        let Some(file) = element.and_then(|element| element.get_file()) else {
            return 0;
        };
        if value.is_empty() {
            return 0;
        }
        let plugin = file.get_name();
        let mut files = self.files.write().unwrap();
        let mut id = 1u32;
        for kind in LStringType::ALL {
            let path = Self::localization_file_name_by_type(&plugin, kind);
            let index = match Self::find_in(&files, path_file_name(&path)) {
                Ok(index) => index,
                Err(_) => {
                    let index = Self::add_to(&mut files, &format!("{}{path}", data_path()), &[]);
                    files[index].modified = true;
                    index
                }
            };
            id = id.max(files[index].next_id);
        }
        let path = Self::localization_file_name_by_type(&plugin, Self::localized_value_decider(element));
        let index = Self::find_in(&files, path_file_name(&path)).expect("made above");
        let table = &mut files[index];
        // Detect a duplicate string.
        if self.reuse_dup()
            && let Some(position) = table.index_of_text(value)
        {
            id = table.strings[position].0;
        } else {
            table.add_string(id, value);
        }
        id
    }

    /// Port of `SetValue`: the string `id` of the element's table takes the
    /// text, or a new string is added (`AddValue`) when the table is not
    /// loaded, `id` is 0 or the table has no such ID; the ID the element is
    /// to hold.
    pub fn set_value(&self, id: u32, element: ElementArg, value: &str) -> u32 {
        if element.is_none() {
            return id;
        }
        let path = Self::localization_file_name_by_element(element);
        let index = self.index_of(path_file_name(&path));
        let Some(index) = index.filter(|_| id != 0) else {
            // New string.
            return self.add_value(value, element);
        };
        let exists = self.with_file(index, |file| file.id_exists(id)).unwrap_or(false);
        if !exists {
            // The string does not exist: a new one.
            return self.add_value(value, element);
        }
        // Modify the existing one.
        self.with_file_mut(index, |file| file.put(id, value));
        id
    }

    /// Port of `GetStringsFromFile`: the strings of the loaded table of that
    /// name, with their IDs.
    pub fn get_strings_from_file(&self, file_name: &str) -> Option<Vec<(u32, String)>> {
        let files = self.files.read().unwrap();
        files
            .iter()
            .find(|file| compare_text(&file.name, file_name) == CmpOrdering::Equal)
            .map(|file| file.strings.clone())
    }
}

impl LocalizationHandler for LocalizationHandlerImpl {
    fn get_value(&self, id: u32, element: ElementArg) -> (bool, String) {
        LocalizationHandlerImpl::get_value(self, id, element)
    }

    fn set_value(&self, id: u32, element: ElementArg, value: &str) -> u32 {
        LocalizationHandlerImpl::set_value(self, id, element, value)
    }

    fn no_translate(&self) -> bool {
        LocalizationHandlerImpl::no_translate(self)
    }
}

static HANDLER: RwLock<Option<Arc<LocalizationHandlerImpl>>> = RwLock::new(None);

/// Upstream `Generation` of the one `wbLocalizationHandler` of the
/// process: counts the `Clear`s, so that the names cached from strings are
/// read again after a language change. It outlives a handler made again
/// for a new session.
static GENERATION: AtomicI32 = AtomicI32::new(0);

/// Creates the handler and registers it as `wbLocalizationHandler`.
pub fn install_localization_handler() -> Arc<LocalizationHandlerImpl> {
    let handler = Arc::new(LocalizationHandlerImpl::default());
    set_localization_handler(Some(handler.clone()));
    *HANDLER.write().unwrap() = Some(handler.clone());
    handler
}

/// The handler `install_localization_handler` made last (`wbLocalizationHandler`).
pub fn localization_handler() -> Option<Arc<LocalizationHandlerImpl>> {
    HANDLER.read().unwrap().clone()
}

/// `wbLocalizationHandler.Generation`.
pub fn localization_generation() -> i32 {
    GENERATION.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings_file(entries: &[(u32, &[u8])], length_prefixed: bool) -> Vec<u8> {
        let mut directory = Vec::new();
        let mut data = Vec::new();
        for (id, bytes) in entries {
            directory.extend_from_slice(&id.to_le_bytes());
            directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
            if length_prefixed {
                data.extend_from_slice(&(bytes.len() as u32 + 1).to_le_bytes());
            }
            data.extend_from_slice(bytes);
            data.push(0);
        }
        let mut file = Vec::new();
        file.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        file.extend_from_slice(&(data.len() as u32).to_le_bytes());
        file.extend_from_slice(&directory);
        file.extend_from_slice(&data);
        file
    }

    #[test]
    fn reads_z_strings_with_the_language_encoding() {
        let _guard = crate::interface::globals::test_lock();
        add_default_l_encodings_if_missing(false);
        add_default_l_encodings_if_missing(true);
        let data = strings_file(&[(1, b"Iron Sword"), (2, b"Caf\xe9")], false);
        let file = LocalizationFile::new("Strings\\Skyrim_English.STRINGS", &data);
        assert_eq!(file.count(), 2);
        assert_eq!(file.find(1), (true, "Iron Sword".to_owned()));
        assert_eq!(file.find(2), (true, "Café".to_owned()));
        assert_eq!(file.find(3), (false, "<Error: Unknown lstring ID 00000003>".to_owned()));
        assert_eq!(file.next_id(), 3);
    }

    #[test]
    fn reads_length_prefixed_strings() {
        let data = strings_file(&[(7, b"A long description")], true);
        let file = LocalizationFile::new("Strings\\Skyrim_English.DLSTRINGS", &data);
        assert_eq!(file.find(7), (true, "A long description".to_owned()));
    }

    #[test]
    fn writes_the_tables_back_in_their_order() {
        let _guard = crate::interface::globals::test_lock();
        add_default_l_encodings_if_missing(false);
        add_default_l_encodings_if_missing(true);
        for length_prefixed in [false, true] {
            let data = strings_file(&[(9, b"Nine"), (2, b""), (5, b"Caf\xe9")], length_prefixed);
            let name = if length_prefixed {
                "Test_English.ILSTRINGS"
            } else {
                "Test_English.STRINGS"
            };
            let mut file = LocalizationFile::new(name, &data);
            assert_eq!(file.write_to_bytes(), data);
            assert_eq!(file.next_id(), 10);
            // `AddString` refuses an ID below `NextID`.
            assert!(!file.add_string(4, "Four"));
            assert!(!file.modified());
            assert!(file.add_string(10, "Ten"));
            file.put(9, "Nine");
            file.put(2, "Two");
            assert!(file.modified());
            let expected = strings_file(
                &[(9, b"Nine"), (2, b"Two"), (5, b"Caf\xe9"), (10, b"Ten")],
                length_prefixed,
            );
            assert_eq!(file.write_to_bytes(), expected);
            assert_eq!(file.index_to_id(3), 10);
            assert_eq!(file.index_to_id(4), 0);
        }
    }

    #[test]
    fn a_shared_string_is_written_once_per_id() {
        let _guard = crate::interface::globals::test_lock();
        add_default_l_encodings_if_missing(false);
        // Two IDs pointing at the same text, as the game's tables do.
        let mut data = Vec::new();
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        for id in [1u32, 2] {
            data.extend_from_slice(&id.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());
        }
        data.extend_from_slice(b"Abc\0");
        let file = LocalizationFile::new("Shared_English.STRINGS", &data);
        assert_eq!(file.write_to_bytes(), strings_file(&[(1, b"Abc"), (2, b"Abc")], false));
    }

    #[test]
    fn exports_ids_and_lines() {
        let data = strings_file(&[(0x1A, b"One"), (0xFFFF_FFFF, b"Two\r\nLines")], false);
        let file = LocalizationFile::new("Export_English.STRINGS", &data);
        assert_eq!(
            file.export_text(),
            b"[0000001A]\r\nOne\r\n[FFFFFFFF]\r\nTwo\r\nLines\r\n"
        );
    }

    #[test]
    fn names_the_table_of_a_plugin() {
        // The language is a process-wide setting that other tests reset.
        let _guard = crate::interface::globals::test_lock();
        crate::interface::globals::set_language("English");
        assert_eq!(
            LocalizationHandlerImpl::localization_file_name_by_type("Update.esm", LStringType::ILString),
            "Strings\\Update_English.ILSTRINGS"
        );
    }

    #[test]
    fn the_tables_stay_sorted_by_name_ignoring_case() {
        let handler = LocalizationHandlerImpl::default();
        for name in ["b_en.STRINGS", "A_en.STRINGS", "c_en.STRINGS", "a_EN.strings"] {
            handler.add_localization(name, &[]);
        }
        let names: Vec<String> = (0..handler.count())
            .map(|index| handler.with_file(index, |file| file.name().to_owned()).unwrap())
            .collect();
        assert_eq!(names, ["A_en.STRINGS", "b_en.STRINGS", "c_en.STRINGS"]);
        assert_eq!(handler.index_of("C_EN.STRINGS"), Some(2));
        let generation = handler.generation();
        handler.clear();
        assert_eq!(handler.count(), 0);
        assert_eq!(handler.generation(), generation + 1);
    }

    #[test]
    fn code_pages_are_named_as_upstream() {
        assert_eq!(mbcs_encoding("utf8"), Some(Encoding::Utf8));
        assert_eq!(mbcs_encoding("windows-1251"), Some(Encoding::Mbcs(1251)));
        assert_eq!(mbcs_encoding("65001"), Some(Encoding::Utf8));
        assert_eq!(mbcs_encoding("latin"), None);
    }
}
