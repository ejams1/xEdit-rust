// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Starting an oracle GUI program without showing its window.
//!
//! `Sniff.exe` is a VCL GUI program: in its automation mode it still creates
//! and shows its main form while it works, which flashes on the desktop for
//! every run of the harness. `CREATE_NO_WINDOW` does not hide it (it only
//! concerns consoles); the first `ShowWindow` of a GUI program takes its show
//! command from `STARTUPINFO` when `STARTF_USESHOWWINDOW` is set, so the
//! program is started with `CreateProcessW` and `SW_HIDE` there. `std`'s
//! `Command` can not set that field.
//!
//! `SW_HIDE` only reaches the first window a program shows: Sniff's progress
//! form and xEdit's module selection show themselves anyway. So the programs
//! also start on a desktop of their own (`CreateDesktopW`, one per harness
//! process, `STARTUPINFO.lpDesktop`), which is never shown: nothing they
//! open reaches the user's screen. The GUI watcher finds their windows with
//! `EnumDesktopWindows` on that desktop ([`desktop`]); window messages reach
//! them there as on the user's desktop.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// The name of the harness's own desktop.
#[cfg(windows)]
fn desktop_name() -> String {
    format!("xedit-parity-{}", std::process::id())
}

/// The handle of the harness's own desktop, created at the first call and
/// kept open while the harness runs; `None` where it can not be created
/// (the programs then start on the user's desktop, hidden as far as
/// `SW_HIDE` goes).
#[cfg(windows)]
pub fn desktop() -> Option<windows_sys::Win32::System::StationsAndDesktops::HDESK> {
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::GENERIC_ALL;
    use windows_sys::Win32::System::StationsAndDesktops::CreateDesktopW;
    static DESKTOP: OnceLock<usize> = OnceLock::new();
    let handle = *DESKTOP.get_or_init(|| {
        let name: Vec<u16> = desktop_name().encode_utf16().chain([0]).collect();
        // SAFETY: the name is NUL-terminated; the other pointers may be null.
        unsafe {
            CreateDesktopW(
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                GENERIC_ALL,
                std::ptr::null(),
            ) as usize
        }
    });
    (handle != 0).then_some(handle as windows_sys::Win32::System::StationsAndDesktops::HDESK)
}

/// A program to start hidden, with its arguments and working folder.
pub struct HiddenCommand {
    program: PathBuf,
    args: Vec<String>,
    dir: Option<PathBuf>,
    env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
}

impl HiddenCommand {
    pub fn new(program: impl AsRef<Path>) -> HiddenCommand {
        HiddenCommand {
            program: program.as_ref().to_path_buf(),
            args: Vec::new(),
            dir: None,
            env: Vec::new(),
        }
    }

    pub fn arg(&mut self, arg: impl Into<String>) -> &mut HiddenCommand {
        self.args.push(arg.into());
        self
    }

    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut HiddenCommand {
        self.dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Sets an environment variable of the child over the inherited
    /// environment (`BSArchPro` asks `TPath.GetHomePath` where its settings
    /// folder goes, and the harness keeps that folder inside its work
    /// directory).
    pub fn env(
        &mut self,
        name: impl Into<std::ffi::OsString>,
        value: impl AsRef<std::ffi::OsStr>,
    ) -> &mut HiddenCommand {
        self.env.push((name.into(), value.as_ref().to_os_string()));
        self
    }

    /// The child's environment: the inherited one with [`HiddenCommand::env`]
    /// overrides, as `CreateProcessW` takes it.
    #[cfg(windows)]
    fn environment_block(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        let mut entries: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
        for (name, value) in &self.env {
            let upper = name.to_string_lossy().to_uppercase();
            entries.retain(|(key, _)| key.to_string_lossy().to_uppercase() != upper);
            entries.push((name.clone(), value.clone()));
        }
        // The block requires the names sorted case-insensitively.
        entries.sort_by_key(|(name, _)| name.to_string_lossy().to_uppercase());
        let mut block = Vec::new();
        for (name, value) in entries {
            let mut entry: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(name.as_os_str()).collect();
            entry.push('=' as u16);
            entry.extend(value.encode_wide());
            block.extend(entry);
            block.push(0);
        }
        if !block.is_empty() {
            block.push(0);
        }
        block
    }

    /// The command line as `CreateProcessW` takes it: the program and each
    /// argument quoted as the C runtime and `CommandLineToArgvW` parse it.
    pub fn command_line(&self) -> String {
        let mut line = quote(&self.program.to_string_lossy());
        for arg in &self.args {
            line.push(' ');
            line.push_str(&quote(arg));
        }
        line
    }

    #[cfg(windows)]
    pub fn spawn(&self) -> Result<HiddenChild> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::System::Threading::{
            CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

        let wide = |text: &std::ffi::OsStr| text.encode_wide().chain([0]).collect::<Vec<u16>>();
        let program = wide(self.program.as_os_str());
        let mut line = wide(std::ffi::OsStr::new(&self.command_line()));
        let dir = self.dir.as_ref().map(|dir| wide(dir.as_os_str()));
        let mut environment = self.environment_block();
        let mut desktop_name: Vec<u16> = desktop_name().encode_utf16().chain([0]).collect();
        // SAFETY: plain data structures, zero is a valid value of each field.
        let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        startup.dwFlags = STARTF_USESHOWWINDOW;
        startup.wShowWindow = SW_HIDE as u16;
        if desktop().is_some() {
            startup.lpDesktop = desktop_name.as_mut_ptr();
        }
        let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        // A Unicode environment block needs `CREATE_UNICODE_ENVIRONMENT`,
        // without which `CreateProcessW` refuses the call.
        let flags = if environment.is_empty() {
            0
        } else {
            CREATE_UNICODE_ENVIRONMENT
        };
        // SAFETY: the strings are NUL-terminated and outlive the call; the
        // command line buffer is mutable as the function requires.
        let ok = unsafe {
            CreateProcessW(
                program.as_ptr(),
                line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                flags,
                if environment.is_empty() {
                    std::ptr::null()
                } else {
                    environment.as_mut_ptr() as *const std::ffi::c_void
                },
                dir.as_ref().map_or(std::ptr::null(), |dir| dir.as_ptr()),
                &startup,
                &mut info,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: the thread handle is ours and not used.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(info.hThread) };
        Ok(HiddenChild { process: info.hProcess })
    }

    #[cfg(not(windows))]
    pub fn spawn(&self) -> Result<HiddenChild> {
        let mut command = std::process::Command::new(&self.program);
        command.args(&self.args);
        command.envs(self.env.iter().map(|(name, value)| (name, value)));
        if let Some(dir) = &self.dir {
            command.current_dir(dir);
        }
        Ok(HiddenChild {
            child: command.spawn()?,
        })
    }
}

/// `arg` quoted for a Windows command line: in double quotes when it is
/// empty or holds a space, tab or quote, with the backslashes before a quote
/// (and before the closing quote) doubled and each quote escaped.
fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    let mut result = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                result.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                result.push('"');
                backslashes = 0;
                continue;
            }
            _ => {}
        }
        if c != '\\' {
            result.extend(std::iter::repeat_n('\\', backslashes));
            backslashes = 0;
            result.push(c);
        }
    }
    result.extend(std::iter::repeat_n('\\', backslashes * 2));
    result.push('"');
    result
}

/// A started program.
pub struct HiddenChild {
    #[cfg(windows)]
    process: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(not(windows))]
    child: std::process::Child,
}

// SAFETY: a process handle may be used from any thread.
#[cfg(windows)]
unsafe impl Send for HiddenChild {}

#[cfg(windows)]
impl std::os::windows::io::AsRawHandle for HiddenChild {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.process
    }
}

impl HiddenChild {
    /// The process ID.
    #[cfg(windows)]
    pub fn id(&self) -> u32 {
        // SAFETY: the handle is open until drop.
        unsafe { windows_sys::Win32::System::Threading::GetProcessId(self.process) }
    }

    #[cfg(not(windows))]
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// The exit code once the program has ended.
    #[cfg(windows)]
    pub fn try_wait(&mut self) -> Result<Option<u32>> {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
        // SAFETY: the handle is open until drop.
        if unsafe { WaitForSingleObject(self.process, 0) } != WAIT_OBJECT_0 {
            return Ok(None);
        }
        let mut code = 0u32;
        // SAFETY: as above.
        if unsafe { GetExitCodeProcess(self.process, &mut code) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Some(code))
    }

    #[cfg(not(windows))]
    pub fn try_wait(&mut self) -> Result<Option<u32>> {
        Ok(self.child.try_wait()?.map(|status| status.code().unwrap_or(-1) as u32))
    }

    /// Ends the program and waits for it.
    #[cfg(windows)]
    pub fn kill(&mut self) {
        use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};
        // SAFETY: the handle is open until drop.
        unsafe {
            TerminateProcess(self.process, 1);
            WaitForSingleObject(self.process, 10_000);
        }
    }

    #[cfg(not(windows))]
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Waits until the program ends or `timeout` passes; `None` on timeout
    /// (the program is then killed), else the exit code.
    pub fn wait_timeout(&mut self, timeout: std::time::Duration) -> Result<Option<u32>> {
        let start = std::time::Instant::now();
        loop {
            if let Some(code) = self.try_wait()? {
                return Ok(Some(code));
            }
            if start.elapsed() > timeout {
                self.kill();
                return Ok(None);
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
}

#[cfg(windows)]
impl Drop for HiddenChild {
    fn drop(&mut self) {
        // SAFETY: the handle is ours.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.process) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn arguments_are_quoted_as_windows_parses_them() {
        assert_eq!(quote("-skip:yes"), "-skip:yes");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("-I:C:\\a b\\c.ba2"), "\"-I:C:\\a b\\c.ba2\"");
        assert_eq!(quote("C:\\a b\\"), "\"C:\\a b\\\\\"");
        assert_eq!(quote("a \"b\""), "\"a \\\"b\\\"\"");
        assert_eq!(quote("a\\\"b"), "\"a\\\\\\\"b\"");
    }

    #[cfg(windows)]
    #[test]
    fn a_hidden_program_runs_and_gives_its_exit_code() {
        let mut command = HiddenCommand::new("C:\\Windows\\System32\\cmd.exe");
        command.arg("/c").arg("exit 3");
        let mut child = command.spawn().unwrap();
        let start = std::time::Instant::now();
        let code = loop {
            if let Some(code) = child.try_wait().unwrap() {
                break code;
            }
            assert!(start.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(code, 3);
    }

    #[cfg(windows)]
    #[test]
    fn a_hidden_program_sees_the_environment_overrides() {
        let dir = std::env::temp_dir().join(format!("xedit-hidden-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("env.txt");
        let mut command = HiddenCommand::new(r"C:\Windows\System32\cmd.exe");
        command
            .arg("/c")
            .arg(format!("set > {}", out.display()))
            .env("HOME", dir.as_os_str())
            .env("USERPROFILE", dir.as_os_str());
        let mut child = command.spawn().unwrap();
        let code = child.wait_timeout(Duration::from_secs(30)).unwrap();
        assert_eq!(code, Some(0));
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains(&format!("HOME={}", dir.display())), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_hung_program_is_killed() {
        let mut command = HiddenCommand::new("C:\\Windows\\System32\\cmd.exe");
        command.arg("/c").arg("ping -n 30 127.0.0.1 > nul");
        let mut child = command.spawn().unwrap();
        assert_eq!(child.wait_timeout(Duration::from_millis(500)).unwrap(), None);
        assert!(child.try_wait().unwrap().is_some());
    }
}
