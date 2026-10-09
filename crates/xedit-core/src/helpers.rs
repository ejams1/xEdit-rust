// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbHelpers.pas (MakeDataFileName, FindBSAs,
// HasBSAs, ExecuteCaptureConsoleOutput)

//! The archive helpers by which the GUI's loader finds the archives of the
//! game ini and of the plugins, and the run of a console program whose
//! output becomes progress messages (the LOD generator `LODGenx64.exe`).

use std::path::Path;

use crate::container_handler::container_exists;
use crate::interface::globals::{
    GameMode, game_mode, is_fallout3, is_fallout4, is_fallout76, is_oblivion, is_skyrim, is_starfield,
};
use crate::interface::misc::progress;

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

/// A value of an ini as `TIniFile.ReadString` reads it
/// (`GetPrivateProfileString`): the section and the name without case, the
/// value trimmed and without a pair of enclosing quotes.
fn ini_read_string(text: &str, section: &str, ident: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line
                .trim_start_matches('[')
                .split(']')
                .next()
                .is_some_and(|name| name.trim().eq_ignore_ascii_case(section));
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((name, value)) = line.split_once('=')
            && name.trim().eq_ignore_ascii_case(ident)
        {
            let value = value.trim();
            let value = if value.len() >= 2
                && ((value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')))
            {
                &value[1..value.len() - 1]
            } else {
                value
            };
            return Some(value.to_owned());
        }
    }
    None
}

/// `FindBSAs` of one ini: the archives its archive lists name that exist
/// below the data folder and are not loaded yet (`names`), and the ones that
/// do not exist (`missing`).
pub fn find_bsas(ini_name: &Path, data_path: &str, names: &mut Vec<String>, missing: &mut Vec<String>) -> usize {
    let before = names.len() + missing.len();
    let text = std::fs::read(ini_name)
        .map(|bytes| xedit_io::encoding::ansi_string(&bytes))
        .unwrap_or_default();
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
    // `TStrings.Text`: the lines of the list.
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
