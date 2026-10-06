// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbLocalization.pas

//! The string tables of localized plugins, upstream `wbLocalizationHandler`.
//! The read side is ported: the `.STRINGS`, `.DLSTRINGS` and `.ILSTRINGS`
//! files, loaded through the containers on the first lookup for a plugin.
//! The editing side (adding and writing strings) is not ported.
//!
//! The encodings of the languages (upstream `wbLEncoding` and
//! `wbEncodingForLanguage` of `wbInterface.pas`) live here as well.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use xedit_io::Encoding;

use crate::container_handler::open_resource_last;
use crate::delphi::{change_file_ext, path_file_name};
use crate::interface::globals::language;
use crate::interface::misc::{LocalizationHandler, progress, set_localization_handler};
use crate::interface::{ElementArg, Signature};

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

/// Port of `TwbLocalizationFile`: the strings of one table by ID.
pub struct LocalizationFile {
    /// Upstream `fName`: the file name without the path.
    name: String,
    file_type: LStringType,
    /// Upstream `fEncoding[False]` and `fEncoding[True]`; the fallback is
    /// `None` when it is the same as the primary encoding.
    encoding: Encoding,
    fallback_encoding: Option<Encoding>,
    strings: HashMap<u32, String>,
}

impl LocalizationFile {
    /// Port of `Create(aFileName, aData)` with `Init` and `ReadDirectory`.
    pub fn new(file_name: &str, data: &[u8]) -> Self {
        let name = path_file_name(file_name).to_owned();
        // Upstream `fLanguage`: the part of the name after the last `_`.
        let stem = change_file_ext(&name, "");
        let language = stem.rsplit_once('_').map(|(_, language)| language).unwrap_or("");
        // The `.cpoverride` file next to the strings file is not ported.
        let encoding = encoding_for_language(language, false);
        let mut text = format!("[{name}] Using encoding (from language): {}", encoding.name());
        let fallback_encoding = Some(encoding_for_language(language, true)).filter(|fallback| *fallback != encoding);
        if let Some(fallback) = fallback_encoding {
            text.push_str(&format!(" with fallback (from languange) to: {}", fallback.name()));
        }
        progress(&text);
        let file_type = Self::file_string_type(&name);
        let mut file = LocalizationFile {
            name,
            file_type,
            encoding,
            fallback_encoding,
            strings: HashMap::new(),
        };
        file.read_directory(data);
        file
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

    pub fn count(&self) -> usize {
        self.strings.len()
    }

    /// Port of `Find`: the string, or the error text in its place.
    pub fn find(&self, id: u32) -> (bool, String) {
        match self.strings.get(&id) {
            Some(string) => (true, string.clone()),
            None => (false, format!("<Error: Unknown lstring ID {id:08X}>")),
        }
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
            self.strings.insert(id, string);
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
}

/// Port of `TwbLocalizationHandler`: the loaded tables by file name.
#[derive(Default)]
pub struct LocalizationHandlerImpl {
    /// Upstream `lFiles`, keyed by the lower-case file name.
    files: RwLock<HashMap<String, Arc<LocalizationFile>>>,
    /// Upstream `NoTranslate`: show the IDs instead of the strings.
    pub no_translate: bool,
}

impl LocalizationHandlerImpl {
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
    fn localization_file_name_by_element(element: ElementArg) -> String {
        let Some(file) = element.and_then(|element| element.get_file()) else {
            return String::new();
        };
        Self::localization_file_name_by_type(&file.get_name(), Self::localized_value_decider(element))
    }

    /// Port of `AddLocalization(aFileName, aData)`.
    pub fn add_localization(&self, file_name: &str, data: &[u8]) -> Arc<LocalizationFile> {
        let key = path_file_name(file_name).to_ascii_lowercase();
        let mut files = self.files.write().unwrap();
        files
            .entry(key)
            .or_insert_with(|| Arc::new(LocalizationFile::new(file_name, data)))
            .clone()
    }

    /// Port of `LoadForFile`: the three tables of the plugin from the containers.
    pub fn load_for_file(&self, plugin_file: &str) {
        for kind in LStringType::ALL {
            let path = Self::localization_file_name_by_type(plugin_file, kind);
            let key = path_file_name(&path).to_ascii_lowercase();
            if self.files.read().unwrap().contains_key(&key) {
                continue;
            }
            if let Some(data) = open_resource_last(&path) {
                self.add_localization(&path, &data);
            }
        }
    }

    /// Port of `GetValue`.
    pub fn get_value(&self, id: u32, element: ElementArg) -> (bool, String) {
        if self.no_translate {
            return (true, format!("{id:08X}"));
        }
        if id == 0 {
            return (true, String::new());
        }
        let file_name = path_file_name(&Self::localization_file_name_by_element(element)).to_ascii_lowercase();
        if file_name.is_empty() {
            return (false, String::new());
        }
        let mut file = self.files.read().unwrap().get(&file_name).cloned();
        if file.is_none() {
            if let Some(plugin) = element.and_then(|element| element.get_file()) {
                self.load_for_file(&plugin.get_name());
            }
            file = self.files.read().unwrap().get(&file_name).cloned();
        }
        match file {
            Some(file) => file.find(id),
            None => (false, format!("<Error: No strings file for lstring ID {id:08X}>")),
        }
    }
}

impl LocalizationHandler for LocalizationHandlerImpl {
    fn get_value(&self, id: u32, element: ElementArg) -> (bool, String) {
        LocalizationHandlerImpl::get_value(self, id, element)
    }
}

/// Creates the handler and registers it as `wbLocalizationHandler`.
pub fn install_localization_handler() -> Arc<LocalizationHandlerImpl> {
    let handler = Arc::new(LocalizationHandlerImpl::default());
    set_localization_handler(Some(handler.clone()));
    handler
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
        add_default_l_encodings_if_missing(false);
        add_default_l_encodings_if_missing(true);
        let data = strings_file(&[(1, b"Iron Sword"), (2, b"Caf\xe9")], false);
        let file = LocalizationFile::new("Strings\\Skyrim_English.STRINGS", &data);
        assert_eq!(file.count(), 2);
        assert_eq!(file.find(1), (true, "Iron Sword".to_owned()));
        assert_eq!(file.find(2), (true, "Café".to_owned()));
        assert_eq!(file.find(3), (false, "<Error: Unknown lstring ID 00000003>".to_owned()));
    }

    #[test]
    fn reads_length_prefixed_strings() {
        let data = strings_file(&[(7, b"A long description")], true);
        let file = LocalizationFile::new("Strings\\Skyrim_English.DLSTRINGS", &data);
        assert_eq!(file.find(7), (true, "A long description".to_owned()));
    }

    #[test]
    fn names_the_table_of_a_plugin() {
        crate::interface::globals::set_language("English");
        assert_eq!(
            LocalizationHandlerImpl::localization_file_name_by_type("Update.esm", LStringType::ILString),
            "Strings\\Update_English.ILSTRINGS"
        );
    }
}
