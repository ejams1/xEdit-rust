// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbHelpers.pas (MakeDataFileName,
// CheckAddFilesToString, FindBSAs, HasBSAs, ExecuteCaptureConsoleOutput)

//! The archive helpers by which the GUI's loader finds the archives of the
//! game ini and of the plugins, and the run of a console program whose
//! output becomes progress messages (the LOD generator `LODGenx64.exe`).

use std::path::Path;

use crate::container_handler::container_exists;
use crate::interface::element::Element;
use crate::interface::globals::{
    GameMode, game_mode, is_fallout3, is_fallout4, is_fallout76, is_oblivion, is_skyrim, is_starfield,
};
use crate::interface::misc::{Variant, progress};
use crate::interface::types::ElementType;
use crate::interface::{ElementRef, MainRecordRef, Signature};
use crate::json::Json;

/// `MakeDataFileName`: a name below the data folder, or the name itself
/// when it is a path; nothing for names under three characters (the
/// aliases of Mod Organizer).
pub fn make_data_file_name(file_name: &str, data_path: &str) -> String {
    let chars: Vec<char> = file_name.chars().collect();
    if chars.len() < 3 {
        String::new()
    } else if !(chars[0] == '\\' || chars[1] == ':') {
        format!("{data_path}{file_name}")
    } else {
        file_name.to_owned()
    }
}

/// `wbArchiveExtension`.
pub fn archive_extension() -> &'static str {
    if is_fallout4() || is_fallout76() || is_starfield() {
        ".ba2"
    } else {
        ".bsa"
    }
}

/// The lines of a section of an ini as `GetPrivateProfileString` reads
/// them (the section without case, the first section of the name): the
/// `name=value` pairs, the name and the value trimmed.
fn ini_section<'a>(text: &'a str, section: &str) -> Vec<(&'a str, &'a str)> {
    let mut lines = Vec::new();
    let mut in_section = false;
    let mut seen = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            if in_section {
                break;
            }
            in_section = !seen
                && line
                    .trim_start_matches('[')
                    .split(']')
                    .next()
                    .is_some_and(|name| name.trim().eq_ignore_ascii_case(section));
            seen |= in_section;
            continue;
        }
        if !in_section || line.starts_with(';') {
            continue;
        }
        if let Some((name, value)) = line.split_once('=') {
            lines.push((name.trim(), value.trim()));
        }
    }
    lines
}

/// `TIniFile.ReadString` reads into a buffer of 2048 characters, so a value
/// keeps at most 2047 of them.
const INI_READ_STRING_BUFFER: usize = 2048;

/// A value of an ini as `TIniFile.ReadString` reads it
/// (`GetPrivateProfileString`): the section and the name without case, the
/// value trimmed and without a pair of enclosing quotes.
fn ini_read_string(text: &str, section: &str, ident: &str) -> Option<String> {
    let (_, value) = ini_section(text, section)
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(ident))?;
    let value = if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"')) || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    };
    // UPSTREAM-QUIRK: the buffer of `TIniFile.ReadString` cuts a longer
    // archive list.
    Some(value.chars().take(INI_READ_STRING_BUFFER - 1).collect())
}

/// `TIniFile.SectionExists`: the section has a key (`ReadSection` lists one).
fn ini_section_exists(text: &str, section: &str) -> bool {
    !ini_section(text, section).is_empty()
}

/// `TIniFile.ValueExists`: the section has the key.
fn ini_value_exists(text: &str, section: &str, ident: &str) -> bool {
    ini_section(text, section)
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(ident))
}

/// The text of an ini as `GetPrivateProfileString` reads it: in the ANSI
/// code page; nothing for a file that can not be read.
fn ini_text(path: &Path) -> String {
    std::fs::read(path)
        .map(|bytes| xedit_io::encoding::ansi_string(&bytes))
        .unwrap_or_default()
}

/// The part of `FindBSAs` both overloads share: the lines of an archive
/// list (`TStrings.Text`), each trimmed, the ones that exist below the data
/// folder and are not loaded yet into `names` as the ini spells them, the
/// others into `missing`.
fn add_archive_names(list: &str, data_path: &str, names: &mut Vec<String>, missing: &mut Vec<String>) {
    for entry in list.split('\n') {
        let s = entry.trim_matches(|c: char| c <= ' ');
        let t = make_data_file_name(s, data_path);
        if t.is_empty() {
            continue;
        }
        if Path::new(&t).is_file() {
            if container_exists(&t) {
                continue;
            }
            names.push(s.to_owned());
        } else {
            missing.push(s.to_owned());
        }
    }
}

/// `FindBSAs` of one ini: the archives its archive lists name that exist
/// below the data folder and are not loaded yet (`names`), and the ones that
/// do not exist (`missing`).
pub fn find_bsas(ini_name: &Path, data_path: &str, names: &mut Vec<String>, missing: &mut Vec<String>) -> usize {
    let before = names.len() + missing.len();
    let text = ini_text(ini_name);
    let read = |ident: &str| ini_read_string(&text, "Archive", ident).unwrap_or_default();
    let list = if is_oblivion() || is_fallout3() {
        let mut s = read("sArchiveList").replace(',', "\n");
        // Update.bsa is hardcoded to load in FNV
        if game_mode() == GameMode::gmFNV {
            if !s.is_empty() {
                s.push('\n');
            }
            s.push_str("Update.bsa");
        }
        s
    } else if is_skyrim() {
        format!("{},{}", read("sResourceArchiveList"), read("sResourceArchiveList2")).replace(',', "\n")
    } else if is_fallout4() || is_fallout76() || is_starfield() {
        format!(
            "{},{},{},{}",
            read("sResourceIndexFileList"),
            read("sResourceStartUpArchiveList"),
            read("sResourceArchiveList"),
            read("sResourceArchiveList2")
        )
        .replace(',', "\n")
    } else {
        String::new()
    };
    add_archive_names(&list, data_path, names, missing);
    names.len() + missing.len() - before
}

/// `FindBSAs` of the game ini and the custom ini: without an `[Archive]`
/// section in the custom ini the game ini's alone; else each archive list
/// from the custom ini where it has the key (`CheckAddFilesToString`) and
/// from the game ini where it does not, the lists joined by line breaks.
pub fn find_bsas_custom(
    ini_name: &Path,
    custom_ini_name: &Path,
    data_path: &str,
    names: &mut Vec<String>,
    missing: &mut Vec<String>,
) -> usize {
    let custom = ini_text(custom_ini_name);
    if !ini_section_exists(&custom, "Archive") {
        return find_bsas(ini_name, data_path, names, missing);
    }
    let before = names.len() + missing.len();
    let main = ini_text(ini_name);
    let check = |ident: &str| -> String {
        let text = if ini_value_exists(&custom, "Archive", ident) {
            &custom
        } else {
            &main
        };
        ini_read_string(text, "Archive", ident)
            .unwrap_or_default()
            .replace(',', "\n")
    };
    let join = |idents: &[&str]| -> String {
        let mut s = String::new();
        for (index, ident) in idents.iter().enumerate() {
            if index > 0 && !s.is_empty() {
                s.push('\n');
            }
            s.push_str(&check(ident));
        }
        s
    };
    let list = if is_oblivion() || is_fallout3() {
        let mut s = check("sArchiveList");
        // Update.bsa is hardcoded to load in FNV
        if game_mode() == GameMode::gmFNV {
            if !s.is_empty() {
                s.push('\n');
            }
            s.push_str("Update.bsa");
        }
        s
    } else if is_skyrim() {
        join(&["sResourceArchiveList", "sResourceArchiveList2"])
    } else if is_fallout4() || is_fallout76() || is_starfield() {
        join(&[
            "sResourceIndexFileList",
            "sResourceStartUpArchiveList",
            "sResourceArchiveList",
            "sResourceArchiveList2",
        ])
    } else {
        String::new()
    };
    add_archive_names(&list, data_path, names, missing);
    names.len() + missing.len() - before
}

/// Whether a file name matches `prefix*<extension>` as `FindFirst` with
/// that mask does: without case.
fn matches_archive_mask(name: &str, prefix: &str, exact: bool, extension: &str) -> bool {
    let lower = name.to_lowercase();
    let (prefix, extension) = (prefix.to_lowercase(), extension.to_lowercase());
    if exact {
        lower == format!("{prefix}{extension}")
    } else {
        lower.starts_with(&prefix) && lower.ends_with(&extension) && lower.len() >= prefix.len() + extension.len()
    }
}

/// `HasBSAs`: the archives of a plugin, `<plugin>*<extension>` (exactly
/// `<plugin><extension>` with `exact`), in the order the folder lists them,
/// after those of the plugin's own ini with `mod_ini`.
pub fn has_bsas(
    mod_name: &str,
    data_path: &str,
    exact: bool,
    mod_ini: bool,
    names: &mut Vec<String>,
    missing: &mut Vec<String>,
) -> usize {
    let mut result = 0;
    if mod_ini {
        let ini = format!("{data_path}{}", crate::delphi::change_file_ext(mod_name, ".ini"));
        result += find_bsas(Path::new(&ini), data_path, names, missing);
    }
    let before = names.len() + missing.len();
    let Ok(entries) = std::fs::read_dir(data_path) else {
        return result;
    };
    let mut found = false;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !matches_archive_mask(&name, mod_name, exact, archive_extension()) {
            continue;
        }
        found = true;
        if container_exists(&format!("{data_path}{name}")) {
            continue;
        }
        let t = make_data_file_name(&name, data_path);
        if !t.is_empty() && Path::new(&t).is_file() {
            if !container_exists(&t) {
                names.push(name);
            }
        } else {
            missing.push(name);
        }
    }
    if found {
        result = names.len() + missing.len() - before;
    }
    result
}

/// `ExecuteCaptureConsoleOutput`: runs a command line with its standard
/// output and error in a pipe and its window hidden (`STARTF_USESHOWWINDOW`
/// with `SW_HIDE`: a console program gets a hidden console of its own when
/// this process has none, as the GUI's child does), each line of the
/// output a progress message (the OEM text trimmed, the empty ones
/// dropped); the exit code.
#[cfg(windows)]
pub fn execute_capture_console_output(command_line: &str) -> std::io::Result<u32> {
    use std::io::{BufRead, BufReader};
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, GetExitCodeProcess, INFINITE, NORMAL_PRIORITY_CLASS, PROCESS_INFORMATION, STARTF_USESHOWWINDOW,
        STARTF_USESTDHANDLES, STARTUPINFOW, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let mut read: HANDLE = std::ptr::null_mut();
    let mut write: HANDLE = std::ptr::null_mut();
    // SAFETY: the out pointers are valid; the security attributes live for
    // the call.
    if unsafe { CreatePipe(&raw mut read, &raw mut write, &raw const security, 0) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // The child gets the writing end only.
    // SAFETY: `read` is a handle just created.
    unsafe { SetHandleInformation(read, HANDLE_FLAG_INHERIT, 0) };
    let mut startup: STARTUPINFOW = unsafe {
        // SAFETY: a plain structure for which zero is a valid value.
        std::mem::zeroed()
    };
    startup.cb = size_of::<STARTUPINFOW>() as u32;
    startup.hStdInput = std::ptr::null_mut();
    startup.hStdOutput = write;
    startup.hStdError = write;
    startup.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
    startup.wShowWindow = SW_HIDE as u16;
    let mut process: PROCESS_INFORMATION = unsafe {
        // SAFETY: as above.
        std::mem::zeroed()
    };
    let mut line: Vec<u16> = command_line.encode_utf16().chain([0]).collect();
    // SAFETY: the command line is a writable NUL-terminated buffer and the
    // structures live for the call.
    let created = unsafe {
        CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            NORMAL_PRIORITY_CLASS,
            std::ptr::null(),
            std::ptr::null(),
            &raw const startup,
            &raw mut process,
        )
    };
    let error = std::io::Error::last_os_error();
    // SAFETY: the writing end is ours to close; the child holds its copy.
    unsafe { CloseHandle(write) };
    if created == 0 {
        // SAFETY: as above.
        unsafe { CloseHandle(read) };
        return Err(error);
    }
    // SAFETY: `read` is an open handle this function owns from here.
    let reader = unsafe { std::fs::File::from_raw_handle(read as _) };
    let mut lines = BufReader::new(reader);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match lines.read_until(b'\n', &mut bytes) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = xedit_io::encoding::oem_string(&bytes);
        let text = text.trim_matches(|c: char| c <= ' ');
        if !text.is_empty() {
            progress(text);
        }
    }
    let mut code = 0u32;
    // SAFETY: the process handles are open until closed below.
    unsafe {
        if WaitForSingleObject(process.hProcess, INFINITE) == WAIT_OBJECT_0 {
            GetExitCodeProcess(process.hProcess, &raw mut code);
        }
        CloseHandle(process.hProcess);
        CloseHandle(process.hThread);
    }
    Ok(code)
}

/// `ExecuteCaptureConsoleOutput` elsewhere: the program of the command line
/// with its output read line by line.
#[cfg(not(windows))]
pub fn execute_capture_console_output(command_line: &str) -> std::io::Result<u32> {
    use std::io::{BufRead, BufReader};
    let (reader, writer) = std::io::pipe()?;
    let mut command = command_from_line(command_line);
    command
        .stdin(std::process::Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer);
    let mut child = command.spawn()?;
    drop(command);
    let mut lines = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        if lines.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_matches(|c: char| c <= ' ');
        if !text.is_empty() {
            progress(text);
        }
    }
    let status = child.wait()?;
    Ok(status.code().map_or(1, |code| code as u32))
}

/// A `std::process::Command` of a command line whose program is quoted:
/// the program, then the rest of the line split at the spaces.
#[cfg(not(windows))]
fn command_from_line(command_line: &str) -> std::process::Command {
    let line = command_line.trim_start();
    let (program, rest) = if let Some(stripped) = line.strip_prefix('"') {
        match stripped.find('"') {
            Some(end) => (&stripped[..end], &stripped[end + 1..]),
            None => (stripped, ""),
        }
    } else {
        match line.find(' ') {
            Some(end) => (&line[..end], &line[end..]),
            None => (line, ""),
        }
    };
    let mut command = std::process::Command::new(program);
    command.args(rest.split_whitespace().map(|arg| arg.trim_matches('"')));
    command
}

/// `wbStringToSignatures`: the four-character signatures of a string, split
/// on `,` when it holds one and on spaces otherwise (a `TStringList` with
/// `StrictDelimiter`), each entry trimmed; an entry shorter than four
/// characters is skipped.
pub fn wb_string_to_signatures(signatures: &str) -> Vec<Signature> {
    let delimiter = if signatures.contains(',') { ',' } else { ' ' };
    signatures
        .split(delimiter)
        .filter_map(|entry| {
            let entry = entry.trim();
            let bytes = entry.as_bytes();
            if bytes.len() < 4 {
                return None;
            }
            Some(Signature::new(&[bytes[0], bytes[1], bytes[2], bytes[3]]))
        })
        .collect()
}

/// Port of `wbGetSiblingRecords`: the records of a signature below the
/// element — for a main record the records of its child group, every
/// container walked depth first — and, with `overrides`, those of the child
/// groups of the overrides in files that load later, without duplicates.
pub fn wb_get_sibling_records(element: &ElementRef, signatures: &[Signature], overrides: bool) -> Vec<MainRecordRef> {
    let mut records = Vec::new();
    match element.as_main_record() {
        Some(record) => {
            let self_ref = element.clone();
            if let Some(group) = record
                .as_element_impl()
                .and_then(|element| element.main_record_impl())
                .and_then(|record| record.child_group())
            {
                find_records(&(group as ElementRef), signatures, &mut records);
            }
            if overrides {
                if let Some(record) = self_ref.into_main_record() {
                    let master = record.get_master_or_self();
                    let main_load_order = element.get_file().map(|file| file.get_load_order()).unwrap_or(i32::MAX);
                    if let Some(master) = master.as_element_impl().and_then(|element| element.main_record_impl()) {
                        for override_record in master.overrides() {
                            let later = override_record
                                .get_file()
                                .map(|file| file.get_load_order() > main_load_order)
                                .unwrap_or(false);
                            if later && let Some(group) = override_record.child_group() {
                                find_records(&(group as ElementRef), signatures, &mut records);
                            }
                        }
                    }
                }
                // Removing duplicates (overridden records): ordered by
                // `CompareElementsFormIDAndLoadOrder`, the first record of a
                // load order FormID stays.
                records.sort_by_key(|record| {
                    let load_order = record.get_file().map(|file| file.get_load_order()).unwrap_or(i32::MAX);
                    (record.get_load_order_form_id().to_cardinal(), load_order)
                });
                let mut kept: Vec<MainRecordRef> = Vec::new();
                for record in records {
                    if kept
                        .last()
                        .is_some_and(|last| last.get_load_order_form_id() == record.get_load_order_form_id())
                    {
                        continue;
                    }
                    kept.push(record);
                }
                return kept;
            }
        }
        None => find_records(element, signatures, &mut records),
    }
    records
}

/// `FindRecords`: the element itself when its signature is one of the list,
/// every element of a container otherwise.
fn find_records(element: &ElementRef, signatures: &[Signature], records: &mut Vec<MainRecordRef>) {
    if let Some(record) = element.clone().into_main_record() {
        if signatures.contains(&record.get_signature()) {
            records.push(record);
        }
        return;
    }
    if let Some(container) = element.as_container() {
        for index in 0..container.get_element_count() {
            if let Some(child) = container.get_element(index) {
                find_records(&child, signatures, records);
            }
        }
    }
}

// **************** PERK Challenge JSON serialization support

/// `IntToStr` of a variant argument: the RTL converts the variant to an
/// integer first. Only the ordinal native values of the virtual perk
/// activities reach this.
fn variant_int_str(value: &Variant) -> String {
    let number = match value {
        Variant::Bool(value) => i64::from(u8::from(*value)),
        Variant::Int(value) => *value,
        Variant::UInt(value) => *value as i64,
        Variant::Float(value) => crate::delphi::round(*value),
        Variant::Str(text) => crate::interface::misc::str_to_int64(text).unwrap_or(0),
        Variant::Empty | Variant::Bytes(_) => 0,
    };
    number.to_string()
}

/// Port of `SerializeStreamArray`: the `Stream` member of a virtual perk
/// activity as its JSON array, the text of every value and a null for
/// anything else.
pub fn serialize_stream_array(a_element: &ElementRef, a_json_array: &mut Json) {
    let Some(container) = a_element.as_container() else {
        return;
    };
    *a_json_array = Json::array();
    for index in 0..container.get_element_count() {
        match container.get_element(index) {
            Some(child) if child.get_element_type() == ElementType::etValue => {
                a_json_array.push(Json::Str(variant_int_str(&child.get_native_value())));
            }
            _ => a_json_array.push(Json::Null),
        }
    }
}

/// Port of `SerializeArray`.
pub fn serialize_array(a_element: &ElementRef, a_json_array: &mut Json) {
    let Some(container) = a_element.as_container() else {
        return;
    };
    for index in 0..container.get_element_count() {
        let Some(child) = container.get_element(index) else {
            a_json_array.push(Json::Null);
            continue;
        };
        match child.get_element_type() {
            ElementType::etValue => {
                let value = child.get_edit_value();
                if value.is_empty() || value == "<null>" {
                    a_json_array.push(Json::Null);
                } else {
                    a_json_array.push(Json::Str(value));
                }
            }
            ElementType::etStruct | ElementType::etArray => {
                a_json_array.push(serialize_element_to_json(&child));
            }
            _ => a_json_array.push(Json::Str(child.get_edit_value())),
        }
    }
}

/// Port of `SerializeElement`.
pub fn serialize_element(a_element: &ElementRef, a_json_obj: &mut Json) {
    let Some(container) = a_element.as_container() else {
        a_json_obj.set(&a_element.get_name(), Json::Str(a_element.get_edit_value()));
        return;
    };
    for index in 0..container.get_element_count() {
        let Some(child) = container.get_element(index) else {
            continue;
        };
        let name = child.get_name();

        // ==================== SPECIAL HANDLING ====================
        if name.eq_ignore_ascii_case("Type") {
            a_json_obj.set("Type", Json::Str(child.get_edit_value()));
            continue;
        }
        if name.eq_ignore_ascii_case("Stream") {
            if let Ok(Some(array)) = a_json_obj.a_mut("Stream") {
                serialize_stream_array(&child, array);
            }
            continue;
        }
        // ========================================================

        match child.get_element_type() {
            ElementType::etValue => {
                let value = child.get_edit_value();
                if value.is_empty() || value == "<null>" || value == "NULL" {
                    a_json_obj.set(&name, Json::Null);
                } else {
                    a_json_obj.set(&name, Json::Str(value));
                }
            }
            ElementType::etStruct => {
                a_json_obj.set(&name, serialize_element_to_json(&child));
            }
            ElementType::etArray => {
                if !name.eq_ignore_ascii_case("Stream") {
                    if let Ok(Some(array)) = a_json_obj.a_mut(&name) {
                        serialize_array(&child, array);
                    }
                }
            }
            _ => a_json_obj.set(&name, Json::Str(child.get_edit_value())),
        }
    }
}

/// Port of `SerializeElementToJson`.
pub fn serialize_element_to_json(a_element: &ElementRef) -> Json {
    let mut result = Json::object();
    serialize_element(a_element, &mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_file_names() {
        assert_eq!(make_data_file_name("a.bsa", "C:\\Data\\"), "C:\\Data\\a.bsa");
        assert_eq!(make_data_file_name("ab", "C:\\Data\\"), "");
        assert_eq!(make_data_file_name("D:\\x.bsa", "C:\\Data\\"), "D:\\x.bsa");
    }

    #[test]
    fn archive_masks() {
        assert!(matches_archive_mask("Fallout4 - Meshes.ba2", "Fallout4", false, ".ba2"));
        assert!(!matches_archive_mask("Fallout4 - Meshes.ba2", "Fallout4", true, ".ba2"));
        assert!(matches_archive_mask("skyrim.bsa", "Skyrim", true, ".bsa"));
    }

    #[test]
    fn ini_values() {
        let text = "[General]\r\nx=1\r\n[Archive]\r\nsArchiveList = \"a.bsa, b.bsa\"\r\n";
        assert_eq!(
            ini_read_string(text, "archive", "SARCHIVELIST").unwrap(),
            "a.bsa, b.bsa"
        );
        assert!(ini_read_string(text, "Archive", "x").is_none());
    }
}
