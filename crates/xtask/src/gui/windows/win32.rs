// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Win32 half of `cargo xtask gui`: enumerating the windows of a
//! private desktop, capturing one, posting input to it, reading its menus and
//! ending its program. Everything here runs against windows on a desktop the
//! user never sees; `EnumDesktopWindows` and message posting work there as
//! they do on the visible desktop.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{FILETIME, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDC, GetWindowDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
};
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, EnumDesktopWindows};
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardState, MAPVK_VK_TO_VSC, MapVirtualKeyW, SetKeyboardState,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_CLICK, CWP_SKIPDISABLED, CWP_SKIPINVISIBLE, CWP_SKIPTRANSPARENT, ChildWindowFromPointEx, EnumChildWindows,
    GUITHREADINFO, GetClassNameW, GetClientRect, GetGUIThreadInfo, GetMenu, GetMenuItemCount, GetMenuItemInfoW,
    GetMenuStringW, GetSubMenu, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, MENUITEMINFOW, MF_BYPOSITION, MFS_CHECKED, MFS_DISABLED, MFT_SEPARATOR, MIIM_FTYPE, MIIM_ID,
    MIIM_STATE, MIIM_STRING, MIIM_SUBMENU, PostMessageW, SC_KEYMENU, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_CHAR,
    WM_CLOSE, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::keys::{KeyEvent, vk};
use crate::parity::hidden;

/// A top-level window of a desktop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    /// The window handle as a number; the commands print it as `0x...`.
    pub handle: isize,
    /// The process that owns the window.
    pub pid: u32,
    pub class: String,
    pub title: String,
    /// `left`, `top`, `width` and `height` in the desktop's pixels.
    pub rect: Rect,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

/// What a mouse click did: the deepest child the point landed in and the
/// point in that child's client coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clicked {
    pub handle: isize,
    pub class: String,
    pub x: i32,
    pub y: i32,
}

/// A captured window.
pub struct Capture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// `printwindow` or `bitblt`.
    pub method: &'static str,
}

/// The menu of a window, item by item, with the submenus of the items that
/// have one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuItem {
    pub position: u32,
    pub id: u32,
    pub text: String,
    pub enabled: bool,
    pub checked: bool,
    pub separator: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub submenu: Vec<MenuItem>,
}

/// The top-level windows of `desktop` (`EnumDesktopWindows`).
pub fn desktop_windows(desktop: &str) -> Result<Vec<WindowInfo>> {
    let handle = hidden::open_or_create_desktop(desktop)?;
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into the vector passed as `LPARAM`,
    // which outlives the call.
    let ok = unsafe { EnumDesktopWindows(handle, Some(collect), (&raw mut handles) as isize) };
    if ok == 0 {
        // `EnumDesktopWindows` answers zero for an empty desktop as well as
        // for a failure. A valid handle (its object still has its name) with
        // no windows is an empty desktop, which is a normal intermediate
        // state before a program shows its first window.
        let error = std::io::Error::last_os_error();
        let valid = !hidden::desktop_object_name(handle).is_empty();
        // SAFETY: the handle was opened above and is closed here.
        unsafe { CloseDesktop(handle) };
        if !valid {
            return Err(error).with_context(|| format!("enumerating the windows of desktop {desktop:?}"));
        }
        return Ok(Vec::new());
    }
    // SAFETY: the handle was opened above and is not used afterwards.
    unsafe { CloseDesktop(handle) };
    let mut windows = Vec::with_capacity(handles.len());
    for window in handles {
        if let Some(info) = window_info(window) {
            windows.push(info);
        }
    }
    Ok(windows)
}

/// The details of one window; `None` when it disappeared during the read.
pub fn window_info(window: HWND) -> Option<WindowInfo> {
    // SAFETY: the handle comes from an enumeration; a stale one fails.
    if unsafe { IsWindow(window) } == 0 {
        return None;
    }
    let mut rect = RECT::default();
    // SAFETY: `rect` is a valid out pointer.
    let rect_ok = unsafe { GetWindowRect(window, &raw mut rect) } != 0;
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer.
    unsafe { GetWindowThreadProcessId(window, &raw mut pid) };
    Some(WindowInfo {
        handle: window as isize,
        pid,
        class: class_of(window),
        title: title_of(window),
        rect: Rect {
            left: rect.left,
            top: rect.top,
            width: if rect_ok { rect.right - rect.left } else { 0 },
            height: if rect_ok { rect.bottom - rect.top } else { 0 },
        },
        // SAFETY: a window handle from an enumeration; a stale one gives 0.
        visible: unsafe { IsWindowVisible(window) } != 0,
    })
}

/// The creation time of a process in 100 ns units since 1601, as the session
/// records it, so that a reused process id is never taken for the session's.
pub fn process_creation_time(pid: u32) -> Option<u64> {
    // SAFETY: the process handle is opened and closed here; the out pointers
    // are valid.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut times = [FILETIME::default(); 4];
        let [creation, exit, kernel, user] = &mut times;
        let ok = GetProcessTimes(process, creation, exit, kernel, user);
        windows_sys::Win32::Foundation::CloseHandle(process);
        if ok == 0 {
            return None;
        }
        Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
    }
}

/// Waits until the process `pid` with creation time `creation` has ended.
pub fn wait_for_exit(pid: u32, creation: u64, timeout: Duration) -> bool {
    let start = Instant::now();
    while process_creation_time(pid) == Some(creation) {
        if start.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

/// Ends the process of a session with `TerminateProcess`. The creation time
/// has to match the session's, so a process that reused the id is never
/// killed.
pub fn terminate(pid: u32, creation: u64) -> Result<()> {
    if process_creation_time(pid) != Some(creation) {
        bail!("process {pid} is not the session's process any more; nothing was terminated");
    }
    // SAFETY: the handle is opened and closed here; the process was checked
    // by its creation time immediately before.
    unsafe {
        let process = OpenProcess(
            PROCESS_TERMINATE | PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        );
        if process.is_null() {
            return Err(std::io::Error::last_os_error()).context(format!("opening process {pid}"));
        }
        let killed = TerminateProcess(process, 1);
        WaitForSingleObject(process, 10_000);
        windows_sys::Win32::Foundation::CloseHandle(process);
        if killed == 0 {
            return Err(std::io::Error::last_os_error()).context(format!("terminating process {pid}"));
        }
    }
    Ok(())
}

/// Asks every top-level window of the process to close (`WM_CLOSE`), as the
/// close button does, and gives the number of windows asked.
pub fn close_windows(pid: u32, desktop: &str) -> Result<usize> {
    let windows = desktop_windows(desktop)?;
    let mut asked = 0;
    for window in windows.iter().filter(|window| window.pid == pid) {
        // SAFETY: a window handle of the session's process; WM_CLOSE takes no
        // pointers.
        unsafe { PostMessageW(window.handle as HWND, WM_CLOSE, 0, 0) };
        asked += 1;
    }
    Ok(asked)
}

/// Captures a window with `PrintWindow`, or with a `BitBlt` of its window
/// device context when the window does not answer `WM_PRINT` (or paints a
/// single colour through it).
pub fn capture(handle: isize) -> Result<Capture> {
    let window = handle as HWND;
    // SAFETY: the handle is checked by the calls themselves.
    unsafe {
        ensure!(IsWindow(window) != 0, "0x{handle:x} is not a window");
        ensure!(
            IsIconic(window) == 0,
            "0x{handle:x} is minimized; restore it before a capture"
        );
        let mut rect = RECT::default();
        ensure!(
            GetWindowRect(window, &raw mut rect) != 0,
            "0x{handle:x} has no rectangle"
        );
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        ensure!(
            width > 0 && height > 0,
            "0x{handle:x} has an empty rectangle ({width}x{height})"
        );

        let screen = GetDC(std::ptr::null_mut());
        ensure!(!screen.is_null(), "the desktop has no device context");
        let memory = CreateCompatibleDC(screen);
        ensure!(!memory.is_null(), "no memory device context");
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        // A negative height makes the rows run downwards, as the PNG does.
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &raw mut bits, std::ptr::null_mut(), 0);
        ensure!(!bitmap.is_null() && !bits.is_null(), "no bitmap holds the capture");
        let previous = SelectObject(memory, bitmap as HGDIOBJ);

        let mut method = "printwindow";
        let mut printed = PrintWindow(
            window,
            memory,
            windows_sys::Win32::UI::WindowsAndMessaging::PW_RENDERFULLCONTENT,
        );
        if printed == 0 {
            printed = PrintWindow(window, memory, 0);
        }
        let mut image = pixels(bits, width, height);
        if printed == 0 || is_uniform(&image) {
            let window_dc = GetWindowDC(window);
            if !window_dc.is_null() {
                let copied = BitBlt(memory, 0, 0, width, height, window_dc, 0, 0, SRCCOPY);
                ReleaseDC(window, window_dc);
                if copied != 0 {
                    let blitted = pixels(bits, width, height);
                    // A uniform `PrintWindow` result is what an app that
                    // draws in its own paint does; the blit is better then.
                    if printed == 0 || !is_uniform(&blitted) {
                        image = blitted;
                        method = "bitblt";
                    }
                }
            }
        }

        SelectObject(memory, previous);
        DeleteObject(bitmap as HGDIOBJ);
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);
        Ok(Capture {
            width: width as u32,
            height: height as u32,
            rgba: image,
            method,
        })
    }
}

/// The pixels of a DIB section, converted from BGRA to RGBA with an opaque
/// alpha (GDI leaves the alpha channel at zero).
///
/// SAFETY: `bits` points at `width * height * 4` readable bytes.
unsafe fn pixels(bits: *mut std::ffi::c_void, width: i32, height: i32) -> Vec<u8> {
    let count = width as usize * height as usize * 4;
    // SAFETY: the caller guarantees the buffer; the read is within it.
    let source = unsafe { std::slice::from_raw_parts(bits as *const u8, count) };
    let mut rgba = Vec::with_capacity(count);
    for pixel in source.as_chunks::<4>().0 {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    rgba
}

/// Whether every pixel has the same colour (a capture that drew nothing).
fn is_uniform(rgba: &[u8]) -> bool {
    rgba.as_chunks::<4>().0.iter().all(|pixel| pixel == &rgba[..4])
}

/// Posts a sequence of keys to a window, one event at a time with `delay`
/// between them, and gives the number of events posted. `control` names the
/// class of the control that receives the keys (`TEdit` for a dialog's edit
/// box): a dialog on a private desktop never has a focused control, so the
/// focused control can not be the target.
pub fn post_keys(window: isize, control: Option<&str>, events: &[KeyEvent], delay: Duration) -> Result<usize> {
    let handle = window as HWND;
    // SAFETY: the handle is checked by the calls themselves.
    unsafe {
        ensure!(IsWindow(handle) != 0, "0x{window:x} is not a window");
        let named = control.and_then(|class| child_of_class(handle, class));
        let mut posted = 0;
        for event in events {
            match event {
                KeyEvent::Text(text) => {
                    let target = named.unwrap_or_else(|| focus_target(handle));
                    for unit in text.encode_utf16() {
                        send(target, WM_CHAR, usize::from(unit), 1);
                        posted += 1;
                        std::thread::sleep(delay);
                    }
                }
                KeyEvent::Menu(character) => {
                    // `SC_KEYMENU` starts the menu tracking, so it is posted:
                    // the call returns while the menu is open, and a send
                    // would wait for the item to be chosen.
                    PostMessageW(
                        handle,
                        WM_SYSCOMMAND,
                        SC_KEYMENU as usize,
                        isize::from(*character as u8),
                    );
                    posted += 1;
                    std::thread::sleep(delay);
                }
                KeyEvent::Key {
                    vk,
                    character,
                    ctrl,
                    shift,
                    alt,
                } => {
                    post_key(
                        named.unwrap_or_else(|| focus_target(handle)),
                        *vk,
                        *character,
                        *ctrl,
                        *shift,
                        *alt,
                    );
                    posted += 1;
                    std::thread::sleep(delay);
                }
            }
        }
        Ok(posted)
    }
}

/// The first child of `window` of the class `class`.
fn child_of_class(window: HWND, class: &str) -> Option<HWND> {
    let mut children: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into the vector passed as `LPARAM`.
    unsafe { EnumChildWindows(window, Some(collect), (&raw mut children) as isize) };
    children.into_iter().find(|&child| class_of(child) == class)
}

/// One key press with its modifiers: the modifiers are set in the keyboard
/// state of the thread (`AttachThreadInput` and `SetKeyboardState`, as
/// `gui::hold_shift` of the harness does for Shift) so that `GetKeyState`
/// sees them, the messages are sent while it is set, and it is restored.
///
/// SAFETY: the handle is a window.
unsafe fn post_key(target_window: HWND, key: u16, character: Option<u16>, ctrl: bool, shift: bool, alt: bool) {
    // SAFETY: the thread id is read for the window; attaching and detaching
    // are paired below.
    unsafe {
        let thread = GetWindowThreadProcessId(target_window, std::ptr::null_mut());
        let me = GetCurrentThreadId();
        let attached = thread != 0 && AttachThreadInput(me, thread, 1) != 0;
        let mut previous = [0u8; 256];
        let had_state = GetKeyboardState(previous.as_mut_ptr()) != 0;
        let mut state = previous;
        let mut pressed: Vec<u16> = Vec::new();
        for (down, keys) in [
            (ctrl, [vk::CONTROL, vk::LCONTROL]),
            (shift, [vk::SHIFT, vk::LSHIFT]),
            (alt, [vk::MENU, vk::LMENU]),
        ] {
            if down {
                for code in keys {
                    state[code as usize] = 0x80;
                    pressed.push(code);
                }
            }
        }
        if had_state {
            SetKeyboardState(state.as_ptr());
        }
        let target = target_window;
        let system = alt;
        for code in &pressed {
            send(target, WM_KEYDOWN, usize::from(*code), lparam(*code, false, system));
        }
        send(
            target,
            if system { WM_SYSKEYDOWN } else { WM_KEYDOWN },
            usize::from(key),
            lparam(key, false, system),
        );
        if let Some(character) = character {
            send(target, WM_CHAR, usize::from(character), 1);
        }
        send(
            target,
            if system { WM_SYSKEYUP } else { WM_KEYUP },
            usize::from(key),
            lparam(key, true, system),
        );
        for code in pressed.iter().rev() {
            send(target, WM_KEYUP, usize::from(*code), lparam(*code, true, system));
        }
        if had_state {
            SetKeyboardState(previous.as_ptr());
        }
        if attached {
            AttachThreadInput(me, thread, 0);
        }
    }
}

/// Posts a mouse click at `x`, `y` in the client area of a window. The point
/// is passed down to the deepest child under it (a VCL button or edit is a
/// child window of the form) and the messages go to that child in its own
/// coordinates, as the mouse would deliver them.
pub fn click(window: isize, x: i32, y: i32, button: &str) -> Result<Clicked> {
    let handle = window as HWND;
    // SAFETY: the handle is checked by the calls themselves.
    unsafe {
        ensure!(IsWindow(handle) != 0, "0x{window:x} is not a window");
        let mut client = RECT::default();
        ensure!(
            GetClientRect(handle, &raw mut client) != 0,
            "0x{window:x} has no client area"
        );
        ensure!(
            x >= 0 && y >= 0 && x < client.right && y < client.bottom,
            "the point {x},{y} is outside the client area ({}x{})",
            client.right,
            client.bottom
        );
        let mut point = windows_sys::Win32::Foundation::POINT { x, y };
        let mut target = handle;
        loop {
            let child = ChildWindowFromPointEx(
                target,
                point,
                CWP_SKIPINVISIBLE | CWP_SKIPDISABLED | CWP_SKIPTRANSPARENT,
            );
            if child.is_null() || child == target {
                break;
            }
            let mut local = point;
            windows_sys::Win32::Graphics::Gdi::MapWindowPoints(target, child, &raw mut local, 1);
            target = child;
            point = local;
        }
        let packed = ((point.y as u32) << 16 | (point.x as u32 & 0xFFFF)) as isize;
        let (down, up, flags) = match button {
            "left" => (WM_LBUTTONDOWN, WM_LBUTTONUP, 1usize), // MK_LBUTTON
            "right" => (WM_RBUTTONDOWN, WM_RBUTTONUP, 2),     // MK_RBUTTON
            "middle" => (WM_MBUTTONDOWN, WM_MBUTTONUP, 0x10), // MK_MBUTTON
            other => bail!("unknown mouse button {other:?}; use left, right or middle"),
        };
        PostMessageW(target, WM_MOUSEMOVE, 0, packed);
        PostMessageW(target, down, flags, packed);
        std::thread::sleep(Duration::from_millis(40));
        PostMessageW(target, up, 0, packed);
        Ok(Clicked {
            handle: target as isize,
            class: class_of(target),
            x: point.x,
            y: point.y,
        })
    }
}

/// The menu of a window (`GetMenu`), item by item.
pub fn menus(window: isize) -> Result<Vec<MenuItem>> {
    let handle = window as HWND;
    // SAFETY: the handle is checked by the calls themselves.
    unsafe {
        ensure!(IsWindow(handle) != 0, "0x{window:x} is not a window");
        let menu = GetMenu(handle);
        if menu.is_null() {
            bail!("0x{window:x} has no menu");
        }
        Ok(menu_items(menu, 0))
    }
}

/// The items of a menu and their submenus, `depth` levels down at most.
///
/// SAFETY: `menu` is a menu handle.
unsafe fn menu_items(menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU, depth: u32) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if depth > 8 {
        return items;
    }
    // SAFETY: the menu handle is valid; the count is read from it.
    let count = unsafe { GetMenuItemCount(menu) };
    for position in 0..count.max(0) as u32 {
        let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
        info.fMask = MIIM_STRING | MIIM_ID | MIIM_STATE | MIIM_SUBMENU | MIIM_FTYPE;
        let mut buffer = [0u16; 512];
        info.dwTypeData = buffer.as_mut_ptr();
        info.cch = buffer.len() as u32;
        // SAFETY: the structure and its text buffer are valid for the call.
        let ok = unsafe { GetMenuItemInfoW(menu, position, 1, &raw mut info) };
        let mut text = if ok != 0 {
            let len = (info.cch as usize).min(buffer.len());
            String::from_utf16_lossy(&buffer[..len])
        } else {
            String::new()
        };
        // A menu item the VCL draws itself stores no text in the menu; the
        // owner-draw item's text is asked for directly, and stays empty when
        // the app keeps it only in its own menu object.
        if text.is_empty() {
            // SAFETY: a buffer of `buffer.len()` characters is given.
            let len =
                unsafe { GetMenuStringW(menu, position, buffer.as_mut_ptr(), buffer.len() as i32, MF_BYPOSITION) };
            if len > 0 {
                text = String::from_utf16_lossy(&buffer[..(len as usize).min(buffer.len())]);
            }
        }
        let submenu = if info.hSubMenu.is_null() {
            // SAFETY: the menu handle and position are valid.
            unsafe { GetSubMenu(menu, position as i32) }
        } else {
            info.hSubMenu
        };
        items.push(MenuItem {
            position,
            id: info.wID,
            text,
            enabled: info.fState & MFS_DISABLED == 0,
            checked: info.fState & MFS_CHECKED != 0,
            separator: info.fType & MFT_SEPARATOR != 0,
            submenu: if submenu.is_null() {
                Vec::new()
            } else {
                // SAFETY: the submenu handle came from the parent item.
                unsafe { menu_items(submenu, depth + 1) }
            },
        });
    }
    items
}

/// The window that receives typed input for `window`: the focused control of
/// its thread when one has the focus (an edit control, the list of a module
/// selection), else the window itself.
///
/// SAFETY: `window` is a window handle.
unsafe fn focus_target(window: HWND) -> HWND {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a valid out pointer with its size set.
    let thread = unsafe { GetWindowThreadProcessId(window, std::ptr::null_mut()) };
    let ok = unsafe { GetGUIThreadInfo(thread, &raw mut info) };
    if ok != 0 && !info.hwndFocus.is_null() {
        info.hwndFocus
    } else {
        window
    }
}

/// A key message with its timeout: a modal dialog the key opens (Enter on a
/// form) keeps the handler busy past the timeout, which is reported as the
/// event having been delivered.
///
/// SAFETY: `window` is a window handle; the messages take no pointers.
unsafe fn send(window: HWND, message: u32, wparam: usize, lparam: isize) {
    let mut result = 0usize;
    // SAFETY: as above.
    unsafe {
        SendMessageTimeoutW(
            window,
            message,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG,
            2_000,
            &raw mut result,
        );
    }
}

/// The `lParam` of a key message: repeat count 1, the scan code, the
/// extended flag of the navigation keys, the context code of an Alt key and
/// the transition bit of a release.
fn lparam(key: u16, up: bool, system: bool) -> isize {
    // SAFETY: a virtual key code is what the call takes.
    let scan = unsafe { MapVirtualKeyW(u32::from(key), MAPVK_VK_TO_VSC) } & 0xFF;
    let mut value: u32 = 1 | (scan << 16);
    if matches!(
        key,
        vk::INSERT | vk::DELETE | vk::HOME | vk::END | vk::PRIOR | vk::NEXT | vk::LEFT | vk::UP | vk::RIGHT | vk::DOWN
    ) {
        value |= 1 << 24;
    }
    if system {
        value |= 1 << 29;
    }
    if up {
        value |= 1 << 31;
    }
    value as isize
}

/// The class name of a window.
fn class_of(window: HWND) -> String {
    let mut buffer = [0u16; 256];
    // SAFETY: the buffer is valid for its length.
    let len = unsafe { GetClassNameW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

/// The window text of a window.
fn title_of(window: HWND) -> String {
    let mut buffer = [0u16; 1024];
    // SAFETY: the buffer is valid for its length.
    let len = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

/// Clicks the button of a window whose caption is `caption` (`BM_CLICK`),
/// which reaches a button a dialog's layout has pushed outside the visible
/// client area (the 4.1.5q dialogs are clipped at 150 % scaling). The
/// caption is read with `WM_GETTEXT`, which a VCL button answers while
/// `GetWindowText` across processes does not always give it. `None` when
/// the window has no such button.
pub fn click_button(window: isize, caption: &str) -> Option<isize> {
    let handle = window as HWND;
    let mut children: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into the vector passed as `LPARAM`.
    unsafe { EnumChildWindows(handle, Some(collect), (&raw mut children) as isize) };
    let button = children.into_iter().find(|&child| {
        // A caption's `&` marks its accelerator and is not part of the name.
        let text = control_text(child).replace('&', "");
        matches!(class_of(child).as_str(), "TButton" | "Button" | "TSpeedButton")
            && text.eq_ignore_ascii_case(&caption.replace('&', ""))
    })?;
    let mut result = 0usize;
    // SAFETY: a button handle of the window; BM_CLICK takes no pointers. A
    // timeout keeps a hung handler (one that opens a dialog) from blocking
    // the helper.
    unsafe {
        SendMessageTimeoutW(button, BM_CLICK, 0, 0, SMTO_ABORTIFHUNG, 5_000, &raw mut result);
    }
    Some(button as isize)
}

/// The text of a control of another process (`WM_GETTEXT`), with a timeout.
fn control_text(handle: HWND) -> String {
    const WM_GETTEXT: u32 = 0x000D;
    let mut buffer = [0u16; 1024];
    let mut copied = 0usize;
    // SAFETY: the buffer holds as many characters as WM_GETTEXT is told.
    let sent = unsafe {
        SendMessageTimeoutW(
            handle,
            WM_GETTEXT,
            buffer.len(),
            buffer.as_mut_ptr() as isize,
            SMTO_ABORTIFHUNG,
            1_000,
            &raw mut copied,
        )
    };
    if sent == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..copied.min(buffer.len())])
}

/// The callback of `EnumDesktopWindows` and the child enumerations: pushes
/// the handle into the vector passed as `LPARAM`.
///
/// SAFETY: `out` is the `Vec` the caller passed for this call.
unsafe extern "system" fn collect(window: HWND, out: LPARAM) -> i32 {
    // SAFETY: as above.
    let handles = unsafe { &mut *(out as *mut Vec<HWND>) };
    handles.push(window);
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CW_USEDEFAULT, CreateMenu, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
        DestroyWindow, MF_GRAYED, MF_POPUP, MF_STRING, RegisterClassW, SetMenu, WNDCLASSW, WS_OVERLAPPED,
    };

    /// A window with a menu of its own, never shown, proves the menu tree is
    /// read item by item (`GetMenu`, `GetMenuItemInfoW`): text, id, enabled
    /// state and submenu.
    #[test]
    fn a_window_menu_is_read_item_by_item() {
        let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let class_name = wide("xeditGuiMenuTest");
        let window_name = wide("menu test");
        // SAFETY: the class and window are created, used and destroyed in
        // this thread; the menu handles are owned here.
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let mut class: WNDCLASSW = std::mem::zeroed();
            class.lpfnWndProc = Some(DefWindowProcW);
            class.hInstance = module;
            class.lpszClassName = class_name.as_ptr();
            assert_ne!(RegisterClassW(&class), 0);
            let window = CreateWindowExW(
                0,
                class_name.as_ptr(),
                window_name.as_ptr(),
                WS_OVERLAPPED,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                200,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                module,
                std::ptr::null(),
            );
            assert!(!window.is_null());
            let menu = CreateMenu();
            let file = CreatePopupMenu();
            let file_label = wide("File");
            assert_ne!(AppendMenuW(menu, MF_POPUP, file as usize, file_label.as_ptr()), 0);
            let open_label = wide("Open");
            let greyed_label = wide("Greyed out");
            assert_ne!(AppendMenuW(file, MF_STRING, 101, open_label.as_ptr()), 0);
            assert_ne!(AppendMenuW(file, MF_STRING | MF_GRAYED, 102, greyed_label.as_ptr()), 0);
            assert_ne!(SetMenu(window, menu), 0);

            let items = menus(window as isize).unwrap();
            assert_eq!(items.len(), 1, "{items:?}");
            assert_eq!(items[0].text, "File");
            assert_eq!(items[0].submenu.len(), 2, "{items:?}");
            assert_eq!(items[0].submenu[0].text, "Open");
            assert_eq!(items[0].submenu[0].id, 101);
            assert!(items[0].submenu[0].enabled);
            assert!(!items[0].submenu[1].enabled, "a greyed item is disabled");

            // The assertions above are the test; the cleanup only has to be
            // attempted (a window of a test thread has no message loop).
            let _ = DestroyWindow(window);
            let _ = DestroyMenu(menu);
        }
    }
}
