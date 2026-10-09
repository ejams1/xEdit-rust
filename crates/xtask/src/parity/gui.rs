// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Driving the GUI build of xEdit (`xTESEdit.exe`, `xFOEdit.exe`,
//! `xSFEdit.exe`) as an oracle without a user.
//!
//! The GUI is started in its script tool mode (`-script:<file>`), which
//! loads the plugins and runs the script once they are loaded
//! (`TfrmMain.tmrGeneratorTimer`, `DoRunScript`). Upstream honours
//! `-autoload` and `-autoexit` only in the edit tool mode (`xeInit.pas`,
//! `_DoInit`), so the script mode still shows the module selection and
//! stays open after the script: the watchdog here presses the OK button of
//! the module selection (`TfrmModuleSelect`, the plugins of the private
//! plugin list are checked), waits for the marker file the script writes
//! last, and ends the process. The script mode itself shows no other
//! prompt: the "What's New", the developer message and the first 64-bit
//! start question are only shown in the edit, view and translate modes
//! (`TfrmMain.DoInit`), and the save dialog only on close.
//!
//! Nothing the GUI writes may reach a game install or the user's settings,
//! so every run gets a private folder: a copy of the executable (the
//! program path is where the GUI writes its log and its `.ini`), a `Data`
//! folder with copies of the plugins (`-D:`), a plugin list (`-P:`, which
//! also moves the `.<game>viewsettings` file next to it), a "My Games"
//! folder (`-M:`) with an empty game ini (`-I:`, so no archive of the
//! user's ini loads, and the GUI does not stop with "Fatal: Could not find
//! ini" for a game that was never started) and a save folder (`-G:`), and
//! the temporary (`-T:`), reference cache (`-C:`) and backup (`-B:`)
//! folders. The folder is removed after the run. Any other dialog the GUI shows (an error
//! message, a question) ends the run with the dialog's text, a "Fatal:"
//! line in the message log of the main form (an initialisation the GUI
//! gave up without a dialog) or the script mode's closing line without the
//! marker (a script that did not compile or raised outside its handler)
//! ends it with the end of the log, and the run is killed after the
//! timeout or when the process stops using the CPU for the hang timeout,
//! so a modal window can never block the harness.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};

use super::hidden::{HiddenChild, HiddenCommand};
use crate::memory::{Budget, Limit};

/// One run of the GUI oracle.
pub struct GuiRun<'a> {
    /// The oracle executable in `XEDIT_ORACLE_DIR`.
    pub exe: &'a Path,
    /// Game mode switch without the dash (`FO4`, `SSE`).
    pub mode: &'a str,
    /// The plugin list marks active plugins with `*` (`wbSimplePluginsTxt`
    /// lists the games without).
    pub star_plugins_txt: bool,
    /// The plugins copied into the private data folder, masters first.
    pub plugins: Vec<PathBuf>,
    /// The script. `{{WORK}}` is replaced by the run folder with a
    /// trailing backslash; the script writes its outputs below
    /// `{{WORK}}out\`, its log to `{{WORK}}status.txt` and then the
    /// marker `{{WORK}}done.txt`.
    pub script: String,
    /// Build the reference information after loading (the script mode
    /// does by default); the plain save does not need it.
    pub build_refs: bool,
    /// The private run folder; it must not exist yet.
    pub work: PathBuf,
    pub timeout: Duration,
    /// The run is ended when the process used no CPU time for this long.
    pub hang_timeout: Duration,
    pub budget: &'a Budget,
    pub expected_peak: u64,
    pub max_memory: u64,
}

/// What the script left after a run.
pub struct GuiResult {
    /// The lines of `status.txt`; the last one is `done` or `error: ...`.
    pub status: Vec<String>,
    /// The folder with the script's outputs (inside the run folder, which
    /// the caller removes with [`remove_work`]).
    pub out: PathBuf,
    pub peak: Option<u64>,
}

/// The GUI executable of a game mode.
pub fn exe_name(mode: &str) -> &'static str {
    match mode.to_ascii_uppercase().as_str() {
        "FO3" | "FNV" | "FO4" | "FO4VR" | "FO76" => "xFOEdit.exe",
        "SF1" => "xSFEdit.exe",
        _ => "xTESEdit.exe",
    }
}

/// Whether the plugin list of a game names only the active plugins, without
/// the `*` (`wbSimplePluginsTxt`).
pub fn simple_plugins_txt(mode: &str) -> bool {
    matches!(
        mode.to_ascii_uppercase().as_str(),
        "TES3" | "TES4" | "FO3" | "FNV" | "TES5" | "ENDERAL"
    )
}

/// Removes a run folder, retrying while Windows still holds the files of
/// the process that was just killed. `XEDIT_ORACLE_KEEP_WORK` keeps it, to
/// read the generated script or run the GUI on it by hand.
pub fn remove_work(work: &Path) {
    if std::env::var_os("XEDIT_ORACLE_KEEP_WORK").is_some() {
        return;
    }
    for _ in 0..50 {
        if fs::remove_dir_all(work).is_ok() || !work.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Windows path text with backslashes, as the GUI needs it for `-D:`.
pub(super) fn windows_path(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\")
}

impl GuiRun<'_> {
    /// Starts the GUI, answers the module selection and waits for the
    /// script. The run folder is left for the caller to read and remove.
    pub fn run(&self) -> Result<GuiResult> {
        if self.work.exists() {
            fs::remove_dir_all(&self.work).with_context(|| format!("removing {}", self.work.display()))?;
        }
        let data = self.work.join("Data");
        let bin = self.work.join("bin");
        let out = self.work.join("out");
        let my_games = self.work.join("mygames");
        let saves = my_games.join("Saves");
        for dir in [&data, &bin, &out, &saves] {
            fs::create_dir_all(dir)?;
        }
        let ini = my_games.join("game.ini");
        fs::write(&ini, "")?;
        let exe = bin.join(self.exe.file_name().context("oracle executable without a name")?);
        fs::copy(self.exe, &exe).with_context(|| format!("copying {}", self.exe.display()))?;
        let mut plugins_txt = String::new();
        for plugin in &self.plugins {
            let name = plugin.file_name().context("plugin without a name")?;
            // `fs::copy` keeps the modification time, by which the older
            // games order their plugins.
            fs::copy(plugin, data.join(name)).with_context(|| format!("copying {}", plugin.display()))?;
            if self.star_plugins_txt {
                plugins_txt.push('*');
            }
            plugins_txt.push_str(&name.to_string_lossy());
            plugins_txt.push_str("\r\n");
        }
        let plugins = self.work.join("plugins.txt");
        fs::write(&plugins, plugins_txt)?;
        let work_text = format!("{}\\", windows_path(&self.work));
        let script = self.work.join("oracle.pas");
        fs::write(&script, self.script.replace("{{WORK}}", &work_text))?;

        // Started hidden: the main form and the module selection are
        // driven through their window handles, which work hidden too.
        let mut command = HiddenCommand::new(&exe);
        command
            .arg(format!("-{}", self.mode))
            .arg(format!("-script:{}", windows_path(&script)))
            .arg(format!("-D:{}\\", windows_path(&data)))
            .arg(format!("-P:{}", windows_path(&plugins)))
            .arg(format!("-T:{}temp\\", work_text))
            .arg(format!("-C:{}cache\\", work_text))
            .arg(format!("-B:{}backup\\", work_text))
            .arg(format!("-S:{work_text}"))
            .arg(format!("-M:{}\\", windows_path(&my_games)))
            .arg(format!("-I:{}", windows_path(&ini)))
            .arg(format!("-G:{}\\", windows_path(&saves)))
            .arg("-IKnowWhatImDoing")
            .current_dir(&self.work);
        if !self.build_refs {
            command.arg("-nobuildrefs");
        }
        let _reservation = self.budget.reserve(self.expected_peak.min(self.max_memory));
        let mut child = command.spawn().with_context(|| format!("starting {}", exe.display()))?;
        // The job object kills the GUI when the harness ends.
        let limit = match Limit::apply(&child, self.max_memory) {
            Ok(limit) => limit,
            Err(error) => {
                child.kill();
                return Err(error);
            }
        };
        let watched = watch(&mut child, &self.work, self.timeout, self.hang_timeout);
        child.kill();
        let peak = limit.peak();
        if limit.reached() {
            bail!(
                "the oracle reached the memory cap ({:.1} GiB)",
                self.max_memory as f64 / crate::memory::GIB as f64
            );
        }
        watched?;
        let status = fs::read_to_string(self.work.join("status.txt"))
            .context("the script wrote no status")?
            .lines()
            .map(str::to_owned)
            .collect();
        Ok(GuiResult { status, out, peak })
    }
}

/// Waits for the script's marker, answering the module selection and
/// failing on any other dialog, a timeout or a hang.
fn watch(child: &mut HiddenChild, work: &Path, timeout: Duration, hang_timeout: Duration) -> Result<()> {
    let start = Instant::now();
    let marker = work.join("done.txt");
    let mut clicked = Vec::new();
    let mut dialogs: Vec<(isize, Instant)> = Vec::new();
    let mut last_cpu = (0u64, Instant::now());
    loop {
        if marker.exists() {
            return Ok(());
        }
        if let Some(code) = child.try_wait()? {
            bail!("the oracle exited (exit code: {code}) before the script finished");
        }
        let elapsed = start.elapsed();
        if elapsed > timeout {
            bail!("the oracle did not finish within {} minutes", timeout.as_secs() / 60);
        }
        let cpu = cpu_time(child);
        if cpu != last_cpu.0 {
            last_cpu = (cpu, Instant::now());
        } else if last_cpu.1.elapsed() > hang_timeout {
            bail!(
                "the oracle used no CPU time for {} seconds (hang)",
                hang_timeout.as_secs()
            );
        }
        let mut waiting = false;
        for window in visible_windows(child.id()) {
            match window.class.as_str() {
                // The main form and the application window.
                "TfrmMain" => {
                    let log = main_form_log(window.handle);
                    let fatal = log.lines().find(|line| line.starts_with("Fatal:"));
                    let closing = log.contains("You can close this application now.");
                    if let Some(line) = fatal {
                        bail!("the oracle stopped: {line}");
                    }
                    // A script that does not compile, or raises outside its
                    // own handler, is aborted without the closing line.
                    if log.contains("Aborted: Applying script") {
                        bail!(
                            "the oracle aborted the script; the end of the log:
{}",
                            tail(&log, 15)
                        );
                    }
                    if closing {
                        // The marker is written before the script returns.
                        std::thread::sleep(Duration::from_secs(2));
                        if !marker.exists() {
                            bail!(
                                "the script ended without its marker; the end of the log:
{}",
                                tail(&log, 15)
                            );
                        }
                    }
                }
                "TApplication" => {}
                "TfrmModuleSelect" => {
                    // The GUI runs hidden, but the modal module selection
                    // shows itself: it is found hidden while it is built,
                    // and answered as soon as it shows (`ShowModal` resets
                    // a result set before), polling fast meanwhile so that
                    // it shows for a moment only.
                    if !clicked.contains(&window.handle) {
                        if window.visible {
                            ensure!(click_button(&window, "OK"), "the module selection has no OK button");
                            clicked.push(window.handle);
                        } else {
                            waiting = true;
                        }
                    }
                }
                _ => {
                    // A dialog may flash up while the GUI works (a progress
                    // window); one that stays is a prompt nobody answers.
                    let seen = match dialogs.iter().find(|(handle, _)| *handle == window.handle) {
                        Some((_, seen)) => *seen,
                        None => {
                            dialogs.push((window.handle, Instant::now()));
                            Instant::now()
                        }
                    };
                    if seen.elapsed() > Duration::from_secs(10) {
                        bail!(
                            "the oracle shows a dialog [{}] \"{}\": {}",
                            window.class,
                            window.title,
                            window.texts.join(" | ")
                        );
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(if waiting { 10 } else { 250 }));
    }
}

/// The last `lines` lines of a text.
pub(super) fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join(
        "
",
    )
}

/// A visible top level window of a process.
pub(super) struct Window {
    pub(super) handle: isize,
    pub(super) class: String,
    pub(super) title: String,
    /// Whether it shows.
    pub(super) visible: bool,
    /// The texts of its visible child windows (labels, buttons).
    pub(super) texts: Vec<String>,
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    pub fn class(handle: HWND) -> String {
        let mut buffer = [0u16; 256];
        // SAFETY: the buffer is valid for its length.
        let len = unsafe { GetClassNameW(handle, buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..len.max(0) as usize])
    }

    pub fn text(handle: HWND) -> String {
        let mut buffer = [0u16; 1024];
        // SAFETY: the buffer is valid for its length.
        let len = unsafe { GetWindowTextW(handle, buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..len.max(0) as usize])
    }

    /// The text of a control of another process (`WM_GETTEXT`, which
    /// `GetWindowTextW` does not send across processes), with a timeout.
    pub fn control_text(handle: HWND) -> String {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETTEXT};
        let mut buffer = [0u16; 2048];
        let mut copied = 0usize;
        // SAFETY: the buffer holds as many characters as WM_GETTEXT is told.
        let sent = unsafe {
            SendMessageTimeoutW(
                handle,
                WM_GETTEXT,
                buffer.len(),
                buffer.as_mut_ptr() as isize,
                SMTO_ABORTIFHUNG,
                1000,
                &raw mut copied,
            )
        };
        if sent == 0 {
            return text(handle);
        }
        String::from_utf16_lossy(&buffer[..copied.min(buffer.len())])
    }

    pub fn visible(handle: HWND) -> bool {
        // SAFETY: a window handle from an enumeration; a stale one fails.
        unsafe { IsWindowVisible(handle) != 0 }
    }

    pub fn process(handle: HWND) -> u32 {
        let mut pid = 0u32;
        // SAFETY: `pid` is a valid out pointer.
        unsafe { GetWindowThreadProcessId(handle, &raw mut pid) };
        pid
    }

    /// The handles an enumeration callback collects.
    pub unsafe extern "system" fn collect(handle: HWND, out: LPARAM) -> i32 {
        // SAFETY: `out` is the `Vec` the caller passed for this call.
        let handles = unsafe { &mut *(out as *mut Vec<HWND>) };
        handles.push(handle);
        1
    }
}

#[cfg(windows)]
pub(super) fn visible_windows(pid: u32) -> Vec<Window> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::StationsAndDesktops::EnumDesktopWindows;
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumChildWindows;
    let mut tops: Vec<HWND> = Vec::new();
    // The GUI runs on the harness's own desktop (`hidden::desktop`), or on
    // the current one (a null desktop) where that could not be created.
    let desktop = super::hidden::desktop().unwrap_or(std::ptr::null_mut());
    // SAFETY: the callback only pushes into the vector passed as `LPARAM`,
    // which outlives the call.
    unsafe { EnumDesktopWindows(desktop, Some(win::collect), (&raw mut tops) as isize) };
    tops.into_iter()
        .filter(|&handle| {
            // The GUI runs hidden: its main form and the module selection
            // are read and answered hidden; any other window counts once it
            // shows.
            win::process(handle) == pid
                && (win::visible(handle) || matches!(win::class(handle).as_str(), "TfrmMain" | "TfrmModuleSelect"))
        })
        .map(|handle| {
            let mut children: Vec<HWND> = Vec::new();
            // SAFETY: as above.
            unsafe { EnumChildWindows(handle, Some(win::collect), (&raw mut children) as isize) };
            Window {
                handle: handle as isize,
                class: win::class(handle),
                title: win::text(handle),
                visible: win::visible(handle),
                texts: children
                    .into_iter()
                    .filter(|&child| win::visible(child))
                    .map(win::control_text)
                    .filter(|text| !text.is_empty())
                    .collect(),
            }
        })
        .collect()
}

#[cfg(not(windows))]
pub(super) fn visible_windows(_pid: u32) -> Vec<Window> {
    Vec::new()
}

/// Clicks the button of a window whose caption is `caption`.
#[cfg(windows)]
pub(super) fn click_button(window: &Window, caption: &str) -> bool {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{BM_CLICK, EnumChildWindows, SendMessageW};
    let mut children: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into the vector passed as `LPARAM`.
    unsafe { EnumChildWindows(window.handle as HWND, Some(win::collect), (&raw mut children) as isize) };
    let Some(button) = children
        .into_iter()
        .find(|&child| win::class(child) == "TButton" && win::text(child) == caption)
    else {
        return false;
    };
    // SAFETY: a button handle of the window; BM_CLICK takes no pointers.
    unsafe { SendMessageW(button, BM_CLICK, 0, 0) };
    true
}

/// The items of the first check list box of a window (`TCheckListBox`),
/// with their handle.
#[cfg(windows)]
pub(super) fn check_list_items(window: &Window) -> Option<(isize, Vec<String>)> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, LB_GETCOUNT, LB_GETTEXT, LB_GETTEXTLEN, SMTO_ABORTIFHUNG, SendMessageTimeoutW,
    };
    let mut children: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into the vector passed as `LPARAM`.
    unsafe { EnumChildWindows(window.handle as HWND, Some(win::collect), (&raw mut children) as isize) };
    let list = children
        .into_iter()
        .find(|&child| win::class(child) == "TCheckListBox")?;
    let send = |message: u32, wparam: usize, lparam: isize| -> Option<usize> {
        let mut result = 0usize;
        // SAFETY: the messages sent here take no pointers but LB_GETTEXT's,
        // whose buffer the caller sizes from LB_GETTEXTLEN.
        let sent =
            unsafe { SendMessageTimeoutW(list, message, wparam, lparam, SMTO_ABORTIFHUNG, 2000, &raw mut result) };
        (sent != 0).then_some(result)
    };
    let count = send(LB_GETCOUNT, 0, 0)?;
    let mut items = Vec::new();
    for index in 0..count {
        let len = send(LB_GETTEXTLEN, index, 0)?;
        let mut buffer = vec![0u16; len + 1];
        let copied = send(LB_GETTEXT, index, buffer.as_mut_ptr() as isize)?;
        items.push(String::from_utf16_lossy(&buffer[..copied.min(len)]));
    }
    Some((list as isize, items))
}

#[cfg(not(windows))]
pub(super) fn check_list_items(_window: &Window) -> Option<(isize, Vec<String>)> {
    None
}

/// Toggles the check box of an item of a `TCheckListBox`: the item is
/// selected and a space typed, which `TCheckListBox.KeyPress` takes as a
/// click on the check box of the selected item.
#[cfg(windows)]
pub(super) fn toggle_check_list_item(list: isize, index: usize) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{LB_SETCURSEL, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_CHAR};
    let mut result = 0usize;
    // SAFETY: LB_SETCURSEL and WM_CHAR take no pointers.
    unsafe {
        SendMessageTimeoutW(
            list as HWND,
            LB_SETCURSEL,
            index,
            0,
            SMTO_ABORTIFHUNG,
            2000,
            &raw mut result,
        );
        SendMessageTimeoutW(
            list as HWND,
            WM_CHAR,
            0x20,
            0x0039_0001,
            SMTO_ABORTIFHUNG,
            2000,
            &raw mut result,
        );
    }
}

#[cfg(not(windows))]
pub(super) fn toggle_check_list_item(_list: isize, _index: usize) {}

/// The text of the message log of the main form: the first memo below
/// it. Read with a timeout, as the GUI thread may be busy loading.
#[cfg(windows)]
pub(super) fn main_form_log(handle: isize) -> String {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETTEXT, WM_GETTEXTLENGTH,
    };
    let mut children: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into the vector passed as `LPARAM`.
    unsafe { EnumChildWindows(handle as HWND, Some(win::collect), (&raw mut children) as isize) };
    let Some(memo) = children.into_iter().find(|&child| win::class(child) == "TMemo") else {
        return String::new();
    };
    let mut len = 0usize;
    // SAFETY: WM_GETTEXTLENGTH takes no pointers; `len` is a valid out pointer.
    let sent = unsafe { SendMessageTimeoutW(memo, WM_GETTEXTLENGTH, 0, 0, SMTO_ABORTIFHUNG, 1000, &raw mut len) };
    if sent == 0 || len == 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; len + 1];
    let mut copied = 0usize;
    // SAFETY: the buffer holds `len + 1` characters, as WM_GETTEXT is told.
    let sent = unsafe {
        SendMessageTimeoutW(
            memo,
            WM_GETTEXT,
            buffer.len(),
            buffer.as_mut_ptr() as isize,
            SMTO_ABORTIFHUNG,
            1000,
            &raw mut copied,
        )
    };
    if sent == 0 {
        return String::new();
    }
    decode_window_text(&buffer[..copied.min(len)])
}

/// The text of a window message: UTF-16, where a run of ANSI bytes read
/// as UTF-16 (the header the LODGen mode puts at the top of its log comes
/// that way) is taken as the bytes it is.
pub(super) fn decode_window_text(units: &[u16]) -> String {
    let mut text = String::new();
    let mut at = 0;
    while at < units.len() {
        let run = units[at..].iter().take_while(|&&unit| unit >= 0x100).count();
        if run >= 8 {
            let bytes: Vec<u8> = units[at..at + run].iter().flat_map(|unit| unit.to_le_bytes()).collect();
            text.push_str(&String::from_utf8_lossy(&bytes));
            at += run;
        } else {
            let end = at + run.max(1);
            text.push_str(&String::from_utf16_lossy(&units[at..end]));
            at = end;
        }
    }
    text
}

#[cfg(not(windows))]
pub(super) fn main_form_log(_handle: isize) -> String {
    String::new()
}

#[cfg(not(windows))]
pub(super) fn click_button(_window: &Window, _caption: &str) -> bool {
    false
}

/// The kernel and user CPU time of a process in 100 ns units.
#[cfg(windows)]
fn cpu_time(child: &HiddenChild) -> u64 {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetProcessTimes;
    let mut times = [FILETIME::default(); 4];
    let [creation, exit, kernel, user] = &mut times;
    // SAFETY: the process handle is open and the out pointers are valid.
    let ok = unsafe { GetProcessTimes(child.as_raw_handle(), creation, exit, kernel, user) };
    if ok == 0 {
        return 0;
    }
    let value = |time: &FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    value(kernel) + value(user)
}

#[cfg(not(windows))]
fn cpu_time(_child: &HiddenChild) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_map_to_their_gui_build() {
        assert_eq!(exe_name("FO4"), "xFOEdit.exe");
        assert_eq!(exe_name("fnv"), "xFOEdit.exe");
        assert_eq!(exe_name("SF1"), "xSFEdit.exe");
        assert_eq!(exe_name("SSE"), "xTESEdit.exe");
        assert_eq!(exe_name("TES3"), "xTESEdit.exe");
        assert!(simple_plugins_txt("TES5"));
        assert!(!simple_plugins_txt("SSE"));
        assert!(!simple_plugins_txt("FO4"));
    }
}

#[cfg(test)]
mod decode_tests {
    #[test]
    fn ansi_bytes_read_as_wide_text_decode() {
        let units: Vec<u16> = "但䰴䑏敇⁮⸴⸱焵砠㐶".encode_utf16().collect();
        assert_eq!(super::decode_window_text(&units), "FO4LODGen 4.1.5q x64");
    }
}
