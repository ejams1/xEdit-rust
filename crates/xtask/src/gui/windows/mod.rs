// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `cargo xtask gui` commands.
//!
//! A **session** is a JSON file: the process id of the program started on a
//! private desktop, the process's creation time (so a reused id is never
//! taken for it) and the desktop's name. `launch` writes one; every other
//! command takes its path.
//!
//! Windows belong to the desktop that created them: `IsWindow` and
//! `EnumDesktopWindows` see them from a thread on that desktop, and a
//! process that opens the desktop by name sees an empty one. So the session
//! is served by a **helper process** (`gui session`, started by `launch`):
//! it creates the desktop, starts the program, holds the desktop open for
//! the session, and performs every list, capture, key, click, menu and close
//! request the other commands send it through two files beside the session
//! (`<session>.req` and `<session>.resp`).
//!
//! Windows are addressed by `--title <substr>` (a case-insensitive substring
//! of the window title, which may match several windows: `shot` captures all
//! of them) or by `--handle <hwnd>`. The single-window commands fail with
//! the list of matching windows when a title is ambiguous.

mod keys;
mod png;
mod win32;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::parity::hidden::{self, HiddenCommand};

/// The desktop a session starts on when `--desktop` is not given. It is
/// never shown, so a window on it cannot reach the user's screen.
const DEFAULT_DESKTOP: &str = "xedit-gui";
/// How long `close` waits for a `WM_CLOSE` to end the program.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// The pause between posted key events and between the text of `text:`.
const KEY_DELAY: Duration = Duration::from_millis(80);
/// How long a request to the helper may take beyond its own work.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// How often the helper and the clients look at the request files.
const REQUEST_POLL: Duration = Duration::from_millis(25);
/// How long the helper holds a desktop when no `close` ever comes: a
/// forgotten session releases its desktop on its own after this.
const KEEPER_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

/// A running screen-check session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// The program under test.
    pub pid: u32,
    /// Creation time of the program in 100 ns units since 1601.
    pub creation_time: u64,
    /// The private desktop the program runs on.
    pub desktop: String,
    /// The program, as it was resolved to an absolute path.
    pub exe: String,
    /// The arguments it was started with.
    pub args: Vec<String>,
    /// The helper process that owns the desktop and serves the requests.
    pub keeper_pid: u32,
    pub keeper_creation_time: u64,
    /// The copy of `xtask` the helper runs from (the build output itself
    /// stays unlocked for a rebuild).
    pub keeper_exe: String,
    /// The file the helper writes once it holds the desktop.
    pub ready: String,
    /// The file whose appearance ends the helper.
    pub stop: String,
    /// The first visible window the program showed, as `launch` reports it.
    #[serde(default)]
    pub first_window: Option<String>,
    pub started_unix: u64,
    /// The path of this file.
    pub session: String,
}

/// Runs `cargo xtask gui <command>`.
pub fn run(args: &[&str]) -> Result<()> {
    match args {
        [] | ["help"] | ["--help"] | ["-h"] => {
            print_help();
            Ok(())
        }
        ["launch", rest @ ..] => launch(rest),
        ["windows", rest @ ..] => list_windows(rest),
        ["children", rest @ ..] => list_children(rest),
        ["shot", rest @ ..] => shot(rest),
        ["keys", rest @ ..] => send_keys(rest),
        ["click", rest @ ..] => click(rest),
        ["resize", rest @ ..] => resize(rest),
        ["menus", rest @ ..] => menus(rest),
        ["menupopup", rest @ ..] => menus_popup(rest),
        ["close", rest @ ..] => close(rest),
        // The helper `launch` starts; not meant to be typed by hand.
        ["session", rest @ ..] => session(rest),
        [other, ..] => bail!("unknown gui command {other:?}; run `cargo xtask gui help`"),
    }
}

fn print_help() {
    println!(
        "\
cargo xtask gui launch <exe> [--args <argument>]... [--desktop <name>] [--session <file>] [--wait <seconds>] [--json]
    Start a GUI program on a private desktop and write its session file.
cargo xtask gui windows <session> [--json]
    List the top-level windows of the desktop (handle, pid, class, title, rect, visible).
cargo xtask gui children <session> (--title <substr>|--handle <hwnd>) [--json]
    List the child windows of one window (class, text, rect), the layout of
    a program that draws its own controls.
cargo xtask gui shot <session> (--title <substr>|--handle <hwnd>)... --out <png> [--json]
    Capture the matching windows with PrintWindow (BitBlt as the fallback).
cargo xtask gui keys <session> (--title <substr>|--handle <hwnd>) --keys <sequence> [--control <class>] [--delay <ms>] [--json]
    Post keys: ctrl+s, enter, text:FO4, alt+f (a menu mnemonic), menu:f. --control
    names the class of the control that receives them (TEdit, TVirtualStringTree):
    a dialog on a private desktop never has a focused control.
cargo xtask gui click <session> (--title <substr>|--handle <hwnd>) (--at <x>,<y> | --click-button <caption>) [--button left|right|middle] [--json]
    Post a mouse click at a client point of the window, or click a button by caption.
cargo xtask gui resize <session> (--title <substr>|--handle <hwnd>) (--size <w>x<h> | --at <x>,<y>) [--json]
    Move and resize a window, so a control the window's own size clips
    (the 4.1.5q form is cut off at the bottom of the screen) can be reached.
cargo xtask gui menus <session> (--title <substr>|--handle <hwnd>) [--json]
    Enumerate the window's menu tree (text, id, enabled, checked).
cargo xtask gui menupopup <session> --handle <hwnd> [--json]
    Enumerate an open popup menu (a menu window of the class #32768).
cargo xtask gui close <session> [--timeout <seconds>] [--json]
    WM_CLOSE the program's windows, then terminate exactly that process if needed.

The session is run by a helper process (`gui session`, internal) that
creates the private desktop, starts the program and keeps the desktop alive;
`close` ends it too. The helper runs from a copy of xtask, so a rebuild while
a session runs is safe.

Session files default to $XEDIT_GUI_SESSIONS, else
$XEDIT_PARITY_SCRATCH/gui, else $XEDIT_PARITY_CACHE/gui, else
<current directory>/.xtask-gui/session.json. No window of any program
started this way reaches the user's desktop."
    );
}

// ---------------------------------------------------------------------------
// Common option handling
// ---------------------------------------------------------------------------

/// The window selection of a command.
#[derive(Default, Clone)]
struct Target {
    titles: Vec<String>,
    handles: Vec<isize>,
}

impl Target {
    fn is_empty(&self) -> bool {
        self.titles.is_empty() && self.handles.is_empty()
    }

    fn to_json(&self) -> Value {
        json!({ "titles": self.titles, "handles": self.handles })
    }

    fn from_json(value: &Value) -> Target {
        Target {
            titles: value["titles"]
                .as_array()
                .map(|titles| {
                    titles
                        .iter()
                        .filter_map(|title| title.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            handles: value["handles"]
                .as_array()
                .map(|handles| {
                    handles
                        .iter()
                        .filter_map(Value::as_i64)
                        .map(|handle| handle as isize)
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// The value of `flag`, consuming the next argument.
fn value<'a>(args: &'a [&'a str], at: &mut usize, flag: &str) -> Result<&'a str> {
    *at += 1;
    args.get(*at).copied().with_context(|| format!("{flag} needs a value"))
}

/// Whether the argument is a command option (and not the session path).
fn is_option(argument: &str) -> bool {
    argument.starts_with("--")
}

/// Parses `--handle 0x10a2c` and `--handle 68268`.
fn parse_handle(text: &str) -> Result<isize> {
    let text = text.trim();
    let number = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => isize::from_str_radix(hex, 16),
        None => text.parse::<isize>(),
    };
    number.with_context(|| format!("{text:?} is not a window handle (use 0x... or a decimal number)"))
}

/// Prints `value` as JSON when `json` is set, else `text` when it is.
fn print_result(json_output: bool, value: &impl Serialize, text: impl FnOnce()) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        text();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Session files and the helper protocol
// ---------------------------------------------------------------------------

/// The session file a command uses: `--session` for `launch`, the positional
/// argument for the others, else the default location.
fn default_session_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XEDIT_GUI_SESSIONS") {
        return PathBuf::from(dir).join("session.json");
    }
    if let Some(dir) = std::env::var_os("XEDIT_PARITY_SCRATCH") {
        return PathBuf::from(dir).join("gui").join("session.json");
    }
    if let Some(dir) = std::env::var_os("XEDIT_PARITY_CACHE") {
        return PathBuf::from(dir).join("gui").join("session.json");
    }
    // The current directory, never a temporary directory: the user's machine
    // keeps its scratch off the system drive.
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".xtask-gui")
        .join("session.json")
}

/// Reads a session file.
fn load_session(path: &str) -> Result<Session> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading the session file {path} (start one with `cargo xtask gui launch`)"))?;
    serde_json::from_str(&text).with_context(|| format!("parsing the session file {path}"))
}

/// Fails when the session's program has ended (or its process id was
/// reused by another program).
fn ensure_alive(session: &Session) -> Result<()> {
    match win32::process_creation_time(session.pid) {
        Some(creation) if creation == session.creation_time => Ok(()),
        Some(_) => bail!(
            "process {} is no longer {} (the process id was reused); nothing will be done to it",
            session.pid,
            session.exe
        ),
        None => bail!(
            "the program of the session (pid {}, {}) has exited",
            session.pid,
            session.exe
        ),
    }
}

/// The helper's liveness, with a message when it has gone.
fn ensure_helper(session: &Session) -> Result<()> {
    if win32::process_creation_time(session.keeper_pid) == Some(session.keeper_creation_time) {
        Ok(())
    } else {
        bail!(
            "the session helper (pid {}) has ended; the session file {path} is stale, remove it or launch a new session",
            session.keeper_pid,
            path = session.session
        )
    }
}

/// The request and response files of a session.
fn request_paths(session_path: &Path) -> (PathBuf, PathBuf) {
    (beside(session_path, ".req"), beside(session_path, ".resp"))
}

/// Writes a file through a temporary one, so that a reader never sees half
/// of it.
fn write_atomically(path: &Path, text: &str) -> Result<()> {
    let temporary = beside(path, ".tmp");
    fs::write(&temporary, text).with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("renaming {}", temporary.display()))?;
    Ok(())
}

/// Sends `command` with `params` to the session's helper and returns its
/// result, or the helper's error.
fn request(session_path: &Path, command: &str, params: Value, timeout: Duration) -> Result<Value> {
    let (request_path, response_path) = request_paths(session_path);
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
        .to_string();
    write_atomically(
        &request_path,
        &json!({ "id": id, "command": command, "params": params }).to_string(),
    )?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(text) = fs::read_to_string(&response_path)
            && let Ok(response) = serde_json::from_str::<Value>(&text)
            && response["id"].as_str() == Some(id.as_str())
        {
            let _ = fs::remove_file(&response_path);
            if response["ok"].as_bool() == Some(true) {
                return Ok(response["result"].clone());
            }
            bail!(
                "{}",
                response["error"]
                    .as_str()
                    .unwrap_or("the session helper failed without a message")
            );
        }
        if Instant::now() > deadline {
            bail!(
                "the session helper did not answer the {command} request within {} s",
                timeout.as_secs()
            );
        }
        std::thread::sleep(REQUEST_POLL);
    }
}

// ---------------------------------------------------------------------------
// launch
// ---------------------------------------------------------------------------

fn launch(args: &[&str]) -> Result<()> {
    let mut exe = None;
    let mut program_args = Vec::new();
    let mut desktop = DEFAULT_DESKTOP.to_owned();
    let mut session_path = None;
    let mut wait = Duration::from_secs(30);
    let mut json_output = false;
    let mut at = 0;
    while at < args.len() {
        match args[at] {
            "--args" | "--arg" => program_args.push(value(args, &mut at, "--args")?.to_owned()),
            "--desktop" => desktop = value(args, &mut at, "--desktop")?.to_owned(),
            "--session" => session_path = Some(PathBuf::from(value(args, &mut at, "--session")?)),
            "--wait" => {
                wait = Duration::from_secs(
                    value(args, &mut at, "--wait")?
                        .parse()
                        .context("--wait takes seconds")?,
                )
            }
            "--json" => json_output = true,
            option if is_option(option) => bail!("unknown option {option}"),
            argument => {
                ensure!(
                    exe.is_none(),
                    "unexpected argument {argument}; the program was already named"
                );
                exe = Some(argument.to_owned());
            }
        }
        at += 1;
    }
    let exe = resolve_program(
        exe.as_deref()
            .context("usage: cargo xtask gui launch <exe> [--args ...]")?,
    )?;
    let session_path = session_path.unwrap_or_else(default_session_path);
    if let Some(parent) = session_path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    // The session is run by a helper process: it creates the private
    // desktop, starts the program on it and keeps a handle to the desktop
    // for the session's life.
    let mut keeper = Keeper::start(&session_path, &exe, &program_args, &desktop, wait)?;
    let session = keeper.wait_ready(&session_path, wait + Duration::from_secs(15))?;
    keeper.disarm();
    let report = json!({
        "pid": session.pid,
        "desktop": session.desktop,
        "session": session.session,
        "exe": session.exe,
        "args": session.args,
        "keeper_pid": session.keeper_pid,
        "first_window": session.first_window,
    });
    print_result(json_output, &report, || {
        println!("launched pid {} on desktop {:?}", session.pid, session.desktop);
        if let Some(first_window) = &session.first_window {
            println!("{first_window}");
        } else {
            eprintln!(
                "warning: the program showed no visible window within {} s",
                wait.as_secs()
            );
        }
        println!("session {}", session.session);
        println!("exe {}", session.exe);
        if !session.args.is_empty() {
            println!("args {}", session.args.join(" "));
        }
    })
}

/// What an exit code during startup means, when it is a known status.
fn exit_explanation(code: u32) -> &'static str {
    match code {
        0xC000_0142 => {
            "(STATUS_DLL_INIT_FAILED: the program could not initialise; a packaged app started outside its package shows this)"
        }
        0xC000_0135 => "(STATUS_DLL_NOT_FOUND)",
        0xC000_0005 => "(STATUS_ACCESS_VIOLATION)",
        _ => "",
    }
}

/// Resolves a program to an absolute path, searching `PATH` for a bare name.
fn resolve_program(program: &str) -> Result<PathBuf> {
    let direct = Path::new(program);
    if direct.is_file() {
        return Ok(plain_path(
            fs::canonicalize(direct).with_context(|| format!("opening {program}"))?,
        ));
    }
    if !program.contains(['\\', '/'])
        && let Some(path) = std::env::var_os("PATH")
    {
        for dir in std::env::split_paths(&path) {
            for name in [program.to_owned(), format!("{program}.exe")] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Ok(plain_path(
                        fs::canonicalize(&candidate).with_context(|| format!("opening {}", candidate.display()))?,
                    ));
                }
            }
        }
    }
    bail!("{program} is not an executable file and PATH holds no {program}")
}

/// `path` without the verbatim prefix `fs::canonicalize` adds: a child that
/// is given a `\\?\` working folder (`cmd.exe`) refuses it as a UNC path, and
/// a session file that names the program reads better without it.
fn plain_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

/// The absolute form of a path, for the session record.
fn absolute(path: &Path) -> String {
    plain_path(fs::canonicalize(path).unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }))
    .display()
    .to_string()
}

// ---------------------------------------------------------------------------
// The session helper (started by `launch`)
// ---------------------------------------------------------------------------

/// The helper process that runs one screen-check session.
///
/// Windows destroys a desktop when its last handle closes, and the windows
/// on it go with it, so the helper creates the private desktop, starts the
/// program on it, writes the session file, and keeps a handle to the
/// desktop while it serves requests until the stop file appears, the
/// program ends, or the timeout. It is dropped (and so stopped) on any
/// failure of `launch`.
struct Keeper {
    pid: u32,
    creation_time: u64,
    ready: PathBuf,
    stop: PathBuf,
    exe: PathBuf,
    child: hidden::HiddenChild,
    armed: bool,
}

impl Keeper {
    /// Starts the helper that runs the session.
    fn start(
        session_path: &Path,
        exe: &Path,
        program_args: &[String],
        desktop: &str,
        wait: Duration,
    ) -> Result<Keeper> {
        let current = std::env::current_exe().context("locating the xtask binary for the session helper")?;
        // The helper runs from a copy: a running image locks its file, and
        // the build output must stay replaceable by the next `cargo build`.
        let helper = beside(session_path, ".keeper.exe");
        fs::copy(&current, &helper).with_context(|| format!("copying the xtask binary to {}", helper.display()))?;
        let ready = beside(session_path, ".ready");
        let stop = beside(session_path, ".stop");
        let (request_path, response_path) = request_paths(session_path);
        for stale in [
            &ready,
            &stop,
            &request_path,
            &response_path,
            &beside(session_path, ".error"),
            session_path,
        ] {
            let _ = fs::remove_file(stale);
        }
        // Started through `HiddenCommand`: it inherits no handle (a pipe a
        // caller captures the command's output with would stay open for the
        // helper's whole life) and it is detached from the caller's console,
        // so it outlives the `launch` command.
        let mut command = HiddenCommand::new(&helper);
        command
            .arg("gui")
            .arg("session")
            .arg(session_path.display().to_string())
            .arg("--exe")
            .arg(exe.display().to_string())
            .arg("--desktop")
            .arg(desktop)
            .arg("--ready")
            .arg(ready.display().to_string())
            .arg("--stop")
            .arg(stop.display().to_string())
            .arg("--wait")
            .arg(wait.as_secs().to_string())
            .arg("--timeout")
            .arg(KEEPER_TIMEOUT.as_secs().to_string());
        for argument in program_args {
            command.arg("--args").arg(argument.clone());
        }
        if let Some(parent) = session_path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            command.current_dir(parent);
        }
        let mut child = command
            .detached()
            .spawn()
            .with_context(|| format!("starting the session helper for {desktop:?}"))?;
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            if let Some(status) = child.try_wait().context("watching the session helper")? {
                bail!("the session helper exited ({status}) before opening desktop {desktop:?}");
            }
            if Instant::now() > deadline {
                child.kill();
                bail!("the session helper did not open desktop {desktop:?} within 10 s");
            }
            std::thread::sleep(REQUEST_POLL);
        }
        let creation_time =
            win32::process_creation_time(pid).context("reading the creation time of the session helper")?;
        Ok(Keeper {
            pid,
            creation_time,
            ready,
            stop,
            exe: helper,
            child,
            armed: true,
        })
    }

    /// Waits for the session file the helper writes, or reports its failure.
    fn wait_ready(&mut self, session_path: &Path, timeout: Duration) -> Result<Session> {
        let error_path = beside(session_path, ".error");
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(text) = fs::read_to_string(session_path)
                && let Ok(session) = serde_json::from_str::<Session>(&text)
            {
                return Ok(session);
            }
            if let Some(status) = self.child.try_wait().context("watching the session helper")? {
                let reason = fs::read_to_string(&error_path).unwrap_or_default();
                let reason = reason.trim();
                if reason.is_empty() {
                    bail!("the session helper exited ({status}) without a reason");
                }
                bail!("the session helper exited ({status}): {reason}");
            }
            if Instant::now() > deadline {
                self.child.kill();
                bail!(
                    "the session helper wrote no session file within {} s",
                    timeout.as_secs()
                );
            }
            std::thread::sleep(REQUEST_POLL);
        }
    }

    /// The session is written: the helper stays for its lifetime.
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for Keeper {
    fn drop(&mut self) {
        if self.armed {
            stop_keeper(self.pid, self.creation_time, &self.exe, &self.ready, &self.stop);
        }
    }
}

/// Ends a helper: the stop file first, and the exact process only when it
/// does not go by itself.
fn stop_keeper(pid: u32, creation: u64, exe: &Path, ready: &Path, stop: &Path) {
    let _ = fs::write(stop, b"stop");
    if !win32::wait_for_exit(pid, creation, Duration::from_secs(5))
        && let Err(error) = win32::terminate(pid, creation)
    {
        eprintln!("warning: the session helper was not stopped: {error}");
    }
    let _ = win32::wait_for_exit(pid, creation, Duration::from_secs(2));
    let _ = fs::remove_file(ready);
    let _ = fs::remove_file(stop);
    // The image unlocks when its process has ended; a copy that stays
    // behind is harmless and the next launch overwrites it.
    let _ = fs::remove_file(exe);
}

/// `path` with `suffix` appended to its file name.
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|name| name.to_os_string()).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// `gui session`: the helper. It creates the private desktop, starts the
/// program on it, writes the session file once the program shows a window,
/// serves requests while holding the desktop, and ends on `close`, the end
/// of the program or the timeout.
fn session(args: &[&str]) -> Result<()> {
    let mut session_path = None;
    let mut exe = None;
    let mut desktop = None;
    let mut ready = None;
    let mut stop = None;
    let mut wait = Duration::from_secs(30);
    let mut timeout = KEEPER_TIMEOUT;
    let mut program_args = Vec::new();
    let mut at = 0;
    while at < args.len() {
        match args[at] {
            "--exe" => exe = Some(value(args, &mut at, "--exe")?.to_owned()),
            "--desktop" => desktop = Some(value(args, &mut at, "--desktop")?.to_owned()),
            "--ready" => ready = Some(PathBuf::from(value(args, &mut at, "--ready")?)),
            "--stop" => stop = Some(PathBuf::from(value(args, &mut at, "--stop")?)),
            "--args" | "--arg" => program_args.push(value(args, &mut at, "--args")?.to_owned()),
            "--wait" => {
                wait = Duration::from_secs(
                    value(args, &mut at, "--wait")?
                        .parse()
                        .context("--wait takes seconds")?,
                )
            }
            "--timeout" => {
                timeout = Duration::from_secs(
                    value(args, &mut at, "--timeout")?
                        .parse()
                        .context("--timeout takes seconds")?,
                )
            }
            option if is_option(option) => bail!("unknown option {option}"),
            argument => session_path = Some(PathBuf::from(argument)),
        }
        at += 1;
    }
    let session_path = session_path.context("usage: cargo xtask gui session <file> --exe <program> ...")?;
    let stop_path = stop.as_deref().context("--stop names the stop file")?.to_path_buf();
    let ready_path = ready.as_deref().context("--ready names the ready file")?.to_path_buf();
    // The helper outlives the `launch` command that started it, and it has
    // no stdio of its own: the caller's pipe is closed as soon as `launch`
    // returns. The default panic hook would then fail writing the message
    // to the closed stderr and abort the helper, which closes the desktop
    // and leaves the program's windows gone. Record panics in the session's
    // `.error` file instead, and keep the desktop (and so the program's
    // windows) open while the process lives after one.
    {
        let panic_path = beside(&session_path, ".error");
        std::panic::set_hook(Box::new(move |info| {
            let _ = fs::write(&panic_path, format!("the session helper panicked: {info}\n"));
        }));
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_session(
            &session_path,
            Path::new(exe.as_deref().context("--exe names the program to start")?),
            &program_args,
            desktop.as_deref().context("--desktop names the private desktop")?,
            &ready_path,
            &stop_path,
            wait,
            timeout,
        )
    }));
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(_) => {
            // The desktop handle of the unwound frame is a raw handle, so
            // it is still open: the process only loses the desktop when it
            // exits. Hold it until `close`, the stop file or the timeout,
            // so a later `shot` still finds the program's windows.
            if ready_path.exists() {
                let desktop = desktop.as_deref().unwrap_or(DEFAULT_DESKTOP);
                hold_desktop(desktop, &stop_path, timeout);
            }
            Err(anyhow::anyhow!(
                "the session helper panicked; see {}",
                beside(&session_path, ".error").display()
            ))
        }
    };
    if let Err(error) = &outcome {
        let _ = fs::write(beside(&session_path, ".error"), format!("{error:#}"));
    }
    outcome
}

/// Holds a session's desktop open after a panic until `close` or the
/// timeout: Windows destroys a desktop when its last handle closes, and the
/// program on it keeps running with no windows once that happens.
fn hold_desktop(desktop: &str, stop: &Path, timeout: Duration) {
    let _handle = hidden::open_or_create_desktop(desktop).ok();
    let end = Instant::now() + timeout;
    while Instant::now() < end && !stop.exists() {
        std::thread::sleep(REQUEST_POLL);
    }
}

/// The text of a caught panic.
fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = panic.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic without a message".to_owned()
    }
}

/// The helper's work: the desktop, the program, the session file and the
/// request loop.
#[allow(clippy::too_many_arguments)]
fn run_session(
    session_path: &Path,
    exe: &Path,
    program_args: &[String],
    desktop: &str,
    ready: &Path,
    stop: &Path,
    wait: Duration,
    timeout: Duration,
) -> Result<()> {
    let desktop_handle = hidden::open_or_create_desktop(desktop)?;
    fs::write(ready, b"ready").with_context(|| format!("writing {}", ready.display()))?;
    let mut command = HiddenCommand::new(exe);
    for argument in program_args {
        command.arg(argument.clone());
    }
    if let Some(parent) = exe.parent() {
        command.current_dir(parent);
    }
    let mut child = command
        .on_desktop(desktop)
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;
    let pid = child.id();
    // The first visible top-level window is what the later commands find,
    // and a program that failed to start is reported with its exit code
    // instead of a stale session.
    let deadline = Instant::now() + wait;
    let mut first_window = None;
    loop {
        if let Some(code) = child
            .try_wait()
            .with_context(|| format!("watching the started program (pid {pid})"))?
        {
            hidden::close_desktop(desktop_handle);
            bail!(
                "the program exited during startup (pid {pid}); its exit code {code:#010x} {}",
                exit_explanation(code)
            );
        }
        if let Some(window) = win32::desktop_windows(desktop)?
            .into_iter()
            .find(|window| window.pid == pid && window.visible)
        {
            first_window = Some(format!(
                "first window 0x{:x} [{}] {:?}",
                window.handle, window.class, window.title
            ));
            break;
        }
        if Instant::now() > deadline {
            break;
        }
        std::thread::sleep(REQUEST_POLL);
    }
    let creation_time = win32::process_creation_time(pid)
        .with_context(|| format!("reading the creation time of the started program (pid {pid})"))?;
    let me = std::process::id();
    let session = Session {
        pid,
        creation_time,
        desktop: desktop.to_owned(),
        exe: exe.display().to_string(),
        args: program_args.to_vec(),
        keeper_pid: me,
        keeper_creation_time: win32::process_creation_time(me).context("reading the helper's own creation time")?,
        keeper_exe: absolute(&std::env::current_exe()?),
        ready: absolute(ready),
        stop: absolute(stop),
        first_window,
        started_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
        session: absolute(session_path),
    };
    write_atomically(session_path, &serde_json::to_string_pretty(&session)?)?;
    // Serve requests while holding the desktop, until `close` asks, the
    // program ends, or the timeout.
    let (request_path, response_path) = request_paths(session_path);
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if let Some(request) = take_request(&request_path) {
            // One bad request must not end the session: a panic in a
            // handler is answered as an error, and the desktop, the
            // program and the next request stay.
            let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle_request(&session, &request)));
            let (result, done) = match handled {
                Ok(Ok(result)) => (result, request.command == "close"),
                Ok(Err(error)) => (json!({ "error": error }), false),
                Err(panic) => (
                    json!({
                        "error": format!(
                            "the session helper panicked on {:?}: {}",
                            request.command,
                            panic_text(panic.as_ref())
                        )
                    }),
                    false,
                ),
            };
            let ok = result.get("error").is_none();
            let response = if ok {
                json!({ "id": request.id, "ok": true, "result": result })
            } else {
                json!({ "id": request.id, "ok": false, "error": result["error"] })
            };
            let _ = write_atomically(&response_path, &response.to_string());
            if done {
                break;
            }
        }
        if stop.exists() || child.try_wait().context("watching the started program")?.is_some() {
            break;
        }
        std::thread::sleep(REQUEST_POLL);
    }
    hidden::close_desktop(desktop_handle);
    Ok(())
}

/// One request of a client.
struct Request {
    id: String,
    command: String,
    params: Value,
}

/// Takes the pending request, if one is written.
fn take_request(path: &Path) -> Option<Request> {
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let _ = fs::remove_file(path);
    Some(Request {
        id: value["id"].as_str().unwrap_or_default().to_owned(),
        command: value["command"].as_str().unwrap_or_default().to_owned(),
        params: value["params"].clone(),
    })
}

/// Runs one request against the desktop and windows the helper owns.
fn handle_request(session: &Session, request: &Request) -> Result<Value, String> {
    let outcome = match request.command.as_str() {
        "windows" => helper_windows(session).map(|windows| json!(windows)),
        "children" => helper_children(session, &request.params),
        "shot" => helper_shot(session, &request.params),
        "keys" => helper_keys(session, &request.params),
        "click" => helper_click(session, &request.params),
        "resize" => helper_resize(session, &request.params),
        "menus" => helper_menus(session, &request.params),
        "menupopup" => helper_menus_popup(session, &request.params),
        "close" => helper_close(session, &request.params),
        other => Err(anyhow::anyhow!("unknown session request {other:?}")),
    };
    outcome.map_err(|error| format!("{error:#}"))
}

// ---------------------------------------------------------------------------
// The requests the helper serves
// ---------------------------------------------------------------------------

/// All top-level windows of the desktop.
fn helper_windows(session: &Session) -> Result<Vec<win32::WindowInfo>> {
    win32::desktop_windows(&session.desktop)
}

/// The child windows of one window of the session's program.
fn helper_children(session: &Session, params: &Value) -> Result<Value> {
    let target = Target::from_json(&params["target"]);
    let window = select_one(session, &target)?;
    let children = win32::child_windows(window.handle)?;
    Ok(json!({
        "window": format!("0x{:x}", window.handle),
        "class": window.class,
        "title": window.title,
        "children": children,
    }))
}

/// The session program's windows, matched against a target.
fn select_windows(session: &Session, target: &Target) -> Result<Vec<win32::WindowInfo>> {
    ensure!(
        !target.is_empty(),
        "give --title <substring> or --handle <hwnd> to choose the window"
    );
    let windows: Vec<win32::WindowInfo> = win32::desktop_windows(&session.desktop)?
        .into_iter()
        .filter(|window| window.pid == session.pid)
        .collect();
    let mut matched = Vec::new();
    for handle in &target.handles {
        matched.push(
            windows
                .iter()
                .find(|window| window.handle == *handle)
                .cloned()
                .with_context(|| {
                    format!(
                        "0x{handle:x} is not a window of the session's program; its windows: {}",
                        describe(&windows)
                    )
                })?,
        );
    }
    for window in &windows {
        if target.titles.is_empty() || !target.titles.iter().any(|title| contains_title(&window.title, title)) {
            continue;
        }
        if !matched.iter().any(|matched| matched.handle == window.handle) {
            matched.push(window.clone());
        }
    }
    ensure!(
        !matched.is_empty(),
        "no window matches {}; the windows of the session's program: {}",
        describe_target(target),
        describe(&windows)
    );
    Ok(matched)
}

/// The one window a command acts on: an ambiguous title is an error that
/// lists the candidates, so a key sequence never goes to the wrong window.
fn select_one(session: &Session, target: &Target) -> Result<win32::WindowInfo> {
    let matched = select_windows(session, target)?;
    if matched.len() == 1 {
        return Ok(matched.into_iter().next().expect("one window"));
    }
    bail!(
        "{} windows match {}; give --handle to name one: {}",
        matched.len(),
        describe_target(target),
        describe(&matched)
    )
}

fn contains_title(title: &str, needle: &str) -> bool {
    title.to_lowercase().contains(&needle.to_lowercase())
}

fn describe(windows: &[win32::WindowInfo]) -> String {
    if windows.is_empty() {
        return "none".to_owned();
    }
    windows
        .iter()
        .map(|window| format!("0x{:x} [{}] {:?}", window.handle, window.class, window.title))
        .collect::<Vec<String>>()
        .join(", ")
}

fn describe_target(target: &Target) -> String {
    let mut parts = Vec::new();
    parts.extend(target.titles.iter().map(|title| format!("title containing {title:?}")));
    parts.extend(target.handles.iter().map(|handle| format!("handle 0x{handle:x}")));
    parts.join(" or ")
}

fn helper_shot(session: &Session, params: &Value) -> Result<Value> {
    let target = Target::from_json(&params["target"]);
    let out = PathBuf::from(params["out"].as_str().context("shot without an output path")?);
    let windows = select_windows(session, &target)?;
    let mut captures = Vec::new();
    for window in &windows {
        let capture = win32::capture(window.handle)
            .with_context(|| format!("capturing 0x{:x} [{}] {:?}", window.handle, window.class, window.title))?;
        let path = capture_path(&out, window, windows.len() > 1);
        png::write(&path, capture.width, capture.height, &capture.rgba)?;
        captures.push(json!({
            "handle": format!("0x{:x}", window.handle),
            "class": window.class,
            "title": window.title,
            "path": path.display().to_string(),
            "width": capture.width,
            "height": capture.height,
            "method": capture.method,
        }));
    }
    Ok(json!({ "captures": captures }))
}

fn helper_keys(session: &Session, params: &Value) -> Result<Value> {
    let target = Target::from_json(&params["target"]);
    let events: Vec<keys::KeyEvent> =
        serde_json::from_value(params["events"].clone()).context("the key events of the request")?;
    let delay = Duration::from_millis(params["delay_ms"].as_u64().unwrap_or(KEY_DELAY.as_millis() as u64));
    let window = select_one(session, &target)?;
    let posted = win32::post_keys(window.handle, params["control"].as_str(), &events, delay)?;
    Ok(json!({
        "handle": format!("0x{:x}", window.handle),
        "class": window.class,
        "title": window.title,
        "events": events,
        "posted": posted,
    }))
}

fn helper_click(session: &Session, params: &Value) -> Result<Value> {
    let target = Target::from_json(&params["target"]);
    let window = select_one(session, &target)?;
    if let Some(caption) = params["caption"].as_str() {
        let button = win32::click_button(window.handle, caption)
            .with_context(|| format!("0x{:x} has no {caption:?} button", window.handle))?;
        return Ok(json!({
            "window": format!("0x{:x}", window.handle),
            "caption": caption,
            "button_handle": format!("0x{button:x}"),
        }));
    }
    let x = params["x"].as_i64().context("click without x")? as i32;
    let y = params["y"].as_i64().context("click without y")? as i32;
    let button = params["button"].as_str().unwrap_or("left");
    let clicked = win32::click(window.handle, x, y, button)?;
    Ok(json!({
        "window": format!("0x{:x}", window.handle),
        "button": button,
        "clicked": clicked,
    }))
}

/// Moves and resizes a window, so a screen check can reach a control that
/// the window's own size clips.
fn helper_resize(session: &Session, params: &Value) -> Result<Value> {
    let target = Target::from_json(&params["target"]);
    let window = select_one(session, &target)?;
    let rect = window.rect;
    let left = params["x"].as_i64().map(|value| value as i32).unwrap_or(rect.left);
    let top = params["y"].as_i64().map(|value| value as i32).unwrap_or(rect.top);
    let width = params["width"].as_i64().map(|value| value as i32).unwrap_or(rect.width);
    let height = params["height"]
        .as_i64()
        .map(|value| value as i32)
        .unwrap_or(rect.height);
    win32::set_window_rect(window.handle, left, top, width, height)?;
    Ok(json!({
        "window": format!("0x{:x}", window.handle),
        "left": left,
        "top": top,
        "width": width,
        "height": height,
    }))
}

fn helper_menus(session: &Session, params: &Value) -> Result<Value> {
    let target = Target::from_json(&params["target"]);
    let window = select_one(session, &target)?;
    let items = win32::menus(window.handle)?;
    Ok(json!({
        "handle": format!("0x{:x}", window.handle),
        "class": window.class,
        "title": window.title,
        "menus": items,
    }))
}

fn helper_menus_popup(session: &Session, params: &Value) -> Result<Value> {
    let target = Target::from_json(&params["target"]);
    let window = select_one(session, &target)?;
    let items = win32::popup_menu(window.handle)?;
    Ok(json!({
        "handle": format!("0x{:x}", window.handle),
        "class": window.class,
        "title": window.title,
        "menus": items,
    }))
}

fn helper_close(session: &Session, params: &Value) -> Result<Value> {
    let timeout = Duration::from_secs(params["timeout"].as_u64().unwrap_or(CLOSE_TIMEOUT.as_secs()));
    let already_exited = win32::process_creation_time(session.pid) != Some(session.creation_time);
    let mut terminated = false;
    if !already_exited {
        win32::close_windows(session.pid, &session.desktop)?;
        if !win32::wait_for_exit(session.pid, session.creation_time, timeout) {
            win32::terminate(session.pid, session.creation_time)?;
            terminated = true;
            let _ = win32::wait_for_exit(session.pid, session.creation_time, Duration::from_secs(10));
        }
    }
    Ok(json!({
        "pid": session.pid,
        "desktop": session.desktop,
        "terminated": terminated,
        "already_exited": already_exited,
    }))
}

/// The file of one capture: `--out` itself for a single window, and
/// `<stem>-<handle>.png` for each when several matched.
fn capture_path(out: &Path, window: &win32::WindowInfo, multiple: bool) -> PathBuf {
    if !multiple {
        return out.to_path_buf();
    }
    let stem = out
        .file_stem()
        .map_or_else(|| "shot".to_owned(), |stem| stem.to_string_lossy().into_owned());
    let extension = out.extension().map_or_else(
        || "png".to_owned(),
        |extension| extension.to_string_lossy().into_owned(),
    );
    out.with_file_name(format!("{stem}-{:x}.{extension}", window.handle))
}

// ---------------------------------------------------------------------------
// windows
// ---------------------------------------------------------------------------

/// The child windows of one window of the session's program.
fn list_children(args: &[&str]) -> Result<()> {
    let mut session_path = None;
    let mut target = Target::default();
    let mut json_output = false;
    let mut at = 0;
    while at < args.len() {
        match args[at] {
            "--title" => target.titles.push(value(args, &mut at, "--title")?.to_owned()),
            "--handle" => target.handles.push(parse_handle(value(args, &mut at, "--handle")?)?),
            "--json" => json_output = true,
            option if is_option(option) => bail!("unknown option {option}"),
            argument => session_path = Some(argument.to_owned()),
        }
        at += 1;
    }
    let session = load_session(&session_path.context("usage: cargo xtask gui children <session> (--title|--handle)")?)?;
    ensure_alive(&session)?;
    ensure_helper(&session)?;
    let response = request(
        Path::new(&session.session),
        "children",
        json!({ "target": target.to_json() }),
        REQUEST_TIMEOUT,
    )?;
    let children: Vec<win32::WindowInfo> =
        serde_json::from_value(response["children"].clone()).context("the child windows of the helper")?;
    print_result(json_output, &children, || {
        println!("{:<12} {:<24} {:<40} {:<22}", "handle", "class", "text", "rect");
        for child in &children {
            println!(
                "0x{:<10x} {:<24} {:<40} {:<22}",
                child.handle,
                child.class,
                child.title,
                format!(
                    "{}x{}+{}+{}",
                    child.rect.width, child.rect.height, child.rect.left, child.rect.top
                ),
            );
        }
    })
}

fn list_windows(args: &[&str]) -> Result<()> {
    let mut session_path = None;
    let mut json_output = false;
    let mut at = 0;
    while at < args.len() {
        match args[at] {
            "--json" => json_output = true,
            option if is_option(option) => bail!("unknown option {option}"),
            argument => session_path = Some(argument.to_owned()),
        }
        at += 1;
    }
    let session = load_session(&session_path.context("usage: cargo xtask gui windows <session>")?)?;
    ensure_alive(&session)?;
    ensure_helper(&session)?;
    let windows: Vec<win32::WindowInfo> = serde_json::from_value(request(
        Path::new(&session.session),
        "windows",
        json!({}),
        REQUEST_TIMEOUT,
    )?)
    .context("the window list of the helper")?;
    print_result(json_output, &windows, || {
        println!(
            "{:<12} {:>7}  {:<24} {:<40} {:<18} {:<7} session",
            "handle", "pid", "class", "title", "rect", "visible"
        );
        for window in &windows {
            println!(
                "0x{:<10x} {:>7}  {:<24} {:<40} {:<18} {:<7} {}",
                window.handle,
                window.pid,
                window.class,
                window.title,
                format!(
                    "{}x{}+{}+{}",
                    window.rect.width, window.rect.height, window.rect.left, window.rect.top
                ),
                if window.visible { "yes" } else { "no" },
                if window.pid == session.pid { "yes" } else { "no" }
            );
        }
        println!(
            "{} window(s) on desktop {:?} (session pid {})",
            windows.len(),
            session.desktop,
            session.pid
        );
    })
}

// ---------------------------------------------------------------------------
// shot
// ---------------------------------------------------------------------------

fn shot(args: &[&str]) -> Result<()> {
    let mut session_path = None;
    let mut target = Target::default();
    let mut out = None;
    let mut json_output = false;
    let mut at = 0;
    while at < args.len() {
        match args[at] {
            "--title" => target.titles.push(value(args, &mut at, "--title")?.to_owned()),
            "--handle" => target.handles.push(parse_handle(value(args, &mut at, "--handle")?)?),
            "--out" => out = Some(PathBuf::from(value(args, &mut at, "--out")?)),
            "--json" => json_output = true,
            option if is_option(option) => bail!("unknown option {option}"),
            argument => session_path = Some(argument.to_owned()),
        }
        at += 1;
    }
    let session = load_session(&session_path.context("usage: cargo xtask gui shot <session> ...")?)?;
    let out = out.context("shot needs --out <png>")?;
    ensure_alive(&session)?;
    ensure_helper(&session)?;
    let response = request(
        Path::new(&session.session),
        "shot",
        json!({ "target": target.to_json(), "out": out.display().to_string() }),
        REQUEST_TIMEOUT,
    )?;
    print_result(json_output, &response, || {
        for capture in response["captures"].as_array().into_iter().flatten() {
            println!(
                "{} [{}] {:?}: {}x{} -> {} ({})",
                capture["handle"].as_str().unwrap_or(""),
                capture["class"].as_str().unwrap_or(""),
                capture["title"].as_str().unwrap_or(""),
                capture["width"].as_u64().unwrap_or(0),
                capture["height"].as_u64().unwrap_or(0),
                capture["path"].as_str().unwrap_or(""),
                capture["method"].as_str().unwrap_or("")
            );
        }
    })
}

// ---------------------------------------------------------------------------
// keys
// ---------------------------------------------------------------------------

fn send_keys(args: &[&str]) -> Result<()> {
    let mut session_path = None;
    let mut target = Target::default();
    let mut sequence = None;
    let mut control = None;
    let mut delay = KEY_DELAY;
    let mut json_output = false;
    let mut at = 0;
    while at < args.len() {
        match args[at] {
            "--title" => target.titles.push(value(args, &mut at, "--title")?.to_owned()),
            "--handle" => target.handles.push(parse_handle(value(args, &mut at, "--handle")?)?),
            "--keys" => sequence = Some(value(args, &mut at, "--keys")?.to_owned()),
            "--control" => control = Some(value(args, &mut at, "--control")?.to_owned()),
            "--delay" => {
                delay = Duration::from_millis(
                    value(args, &mut at, "--delay")?
                        .parse()
                        .context("--delay takes milliseconds")?,
                );
            }
            "--json" => json_output = true,
            option if is_option(option) => bail!("unknown option {option}"),
            argument => session_path = Some(argument.to_owned()),
        }
        at += 1;
    }
    let session = load_session(&session_path.context("usage: cargo xtask gui keys <session> --keys ...")?)?;
    let sequence = sequence.context("keys needs --keys <sequence>")?;
    let events = keys::parse(&sequence).map_err(anyhow::Error::msg)?;
    ensure_alive(&session)?;
    ensure_helper(&session)?;
    let response = request(
        Path::new(&session.session),
        "keys",
        json!({
            "target": target.to_json(),
            "events": events,
            "control": control,
            "delay_ms": delay.as_millis() as u64,
        }),
        REQUEST_TIMEOUT,
    )?;
    print_result(json_output, &response, || {
        println!(
            "posted {} event(s) to {} [{}] {:?}",
            response["posted"].as_u64().unwrap_or(0),
            response["handle"].as_str().unwrap_or(""),
            response["class"].as_str().unwrap_or(""),
            response["title"].as_str().unwrap_or("")
        );
    })
}

// ---------------------------------------------------------------------------
// click
// ---------------------------------------------------------------------------

fn click(args: &[&str]) -> Result<()> {
    let mut session_path = None;
    let mut target = Target::default();
    let mut at_point = None;
    let mut caption = None;
    let mut button = "left".to_owned();
    let mut json_output = false;
    let mut at = 0;
    while at < args.len() {
        match args[at] {
            "--title" => target.titles.push(value(args, &mut at, "--title")?.to_owned()),
            "--handle" => target.handles.push(parse_handle(value(args, &mut at, "--handle")?)?),
            "--at" => at_point = Some(parse_point(value(args, &mut at, "--at")?)?),
            "--click-button" => caption = Some(value(args, &mut at, "--click-button")?.to_owned()),
            "--button" => button = value(args, &mut at, "--button")?.to_owned(),
            "--json" => json_output = true,
            option if is_option(option) => bail!("unknown option {option}"),
            argument => session_path = Some(argument.to_owned()),
        }
        at += 1;
    }
    let session = load_session(&session_path.context("usage: cargo xtask gui click <session> --at x,y")?)?;
    ensure!(
        at_point.is_some() || caption.is_some(),
        "click needs --at <x>,<y> or --click-button <caption>"
    );
    ensure_alive(&session)?;
    ensure_helper(&session)?;
    let mut params = json!({ "target": target.to_json(), "button": button });
    if let Some((x, y)) = at_point {
        params["x"] = json!(x);
        params["y"] = json!(y);
    }
    if let Some(caption) = &caption {
        params["caption"] = json!(caption);
    }
    let response = request(Path::new(&session.session), "click", params, REQUEST_TIMEOUT)?;
    print_result(json_output, &response, || {
        if let Some(caption) = &caption {
            println!(
                "clicked the {caption:?} button ({}) of {}",
                response["button_handle"].as_str().unwrap_or(""),
                response["window"].as_str().unwrap_or("")
            );
        } else if let Some((x, y)) = at_point {
            let handle = response["clicked"]["handle"].as_i64().unwrap_or(0);
            println!(
                "{button} click at {x},{y} of {} -> 0x{handle:x} [{}] at {},{}",
                response["window"].as_str().unwrap_or(""),
                response["clicked"]["class"].as_str().unwrap_or(""),
                response["clicked"]["x"].as_i64().unwrap_or(0),
                response["clicked"]["y"].as_i64().unwrap_or(0)
            );
        }
    })
}

/// Moves and resizes a window of the session's program.
fn resize(args: &[&str]) -> Result<()> {
    let mut session_path = None;
    let mut target = Target::default();
    let mut at_point = None;
    let mut size = None;
    let mut json_output = false;
    let mut at = 0;
    while at < args.len() {
        match args[at] {
            "--title" => target.titles.push(value(args, &mut at, "--title")?.to_owned()),
            "--handle" => target.handles.push(parse_handle(value(args, &mut at, "--handle")?)?),
            "--at" => at_point = Some(parse_point(value(args, &mut at, "--at")?)?),
            "--size" => size = Some(parse_size(value(args, &mut at, "--size")?)?),
            "--json" => json_output = true,
            option if is_option(option) => bail!("unknown option {option}"),
            argument => session_path = Some(argument.to_owned()),
        }
        at += 1;
    }
    let session =
        load_session(&session_path.context("usage: cargo xtask gui resize <session> (--size <w>x<h>|--at <x>,<y>)")?)?;
    ensure!(
        at_point.is_some() || size.is_some(),
        "resize needs --size <width>x<height> or --at <x>,<y>"
    );
    ensure_alive(&session)?;
    ensure_helper(&session)?;
    let mut params = json!({ "target": target.to_json() });
    if let Some((x, y)) = at_point {
        params["x"] = json!(x);
        params["y"] = json!(y);
    }
    if let Some((width, height)) = size {
        params["width"] = json!(width);
        params["height"] = json!(height);
    }
    let response = request(Path::new(&session.session), "resize", params, REQUEST_TIMEOUT)?;
    print_result(json_output, &response, || {
        println!(
            "resized {} to {}x{} at {},{}",
            response["window"].as_str().unwrap_or(""),
            response["width"].as_i64().unwrap_or(0),
            response["height"].as_i64().unwrap_or(0),
            response["left"].as_i64().unwrap_or(0),
            response["top"].as_i64().unwrap_or(0),
        );
    })
}

/// Parses `--size 1557x1200`.
fn parse_size(text: &str) -> Result<(i32, i32)> {
    let (width, height) = text
        .split_once(['x', 'X'])
        .with_context(|| format!("{text:?} is not a size; use <width>x<height>"))?;
    Ok((
        width
            .trim()
            .parse()
            .with_context(|| format!("{width:?} is not a width"))?,
        height
            .trim()
            .parse()
            .with_context(|| format!("{height:?} is not a height"))?,
    ))
}

fn parse_point(text: &str) -> Result<(i32, i32)> {
    let (x, y) = text
        .split_once(',')
        .with_context(|| format!("{text:?} is not a point; use <x>,<y>"))?;
    Ok((
        x.trim().parse().with_context(|| format!("{x:?} is not a number"))?,
        y.trim().parse().with_context(|| format!("{y:?} is not a number"))?,
    ))
}

// ---------------------------------------------------------------------------
// menus
// ---------------------------------------------------------------------------

fn menus(args: &[&str]) -> Result<()> {
    let mut session_path = None;
    let mut target = Target::default();
    let mut json_output = false;
    let mut at = 0;
    while at < args.len() {
        match args[at] {
            "--title" => target.titles.push(value(args, &mut at, "--title")?.to_owned()),
            "--handle" => target.handles.push(parse_handle(value(args, &mut at, "--handle")?)?),
            "--json" => json_output = true,
            option if is_option(option) => bail!("unknown option {option}"),
            argument => session_path = Some(argument.to_owned()),
        }
        at += 1;
    }
    let session = load_session(&session_path.context("usage: cargo xtask gui menus <session> --title ...")?)?;
    ensure_alive(&session)?;
    ensure_helper(&session)?;
    let response = request(
        Path::new(&session.session),
        "menus",
        json!({ "target": target.to_json() }),
        REQUEST_TIMEOUT,
    )?;
    let items: Vec<win32::MenuItem> = serde_json::from_value(response["menus"].clone()).context("the menu tree")?;
    print_result(json_output, &response, || {
        println!(
            "menu of {} [{}] {:?}",
            response["handle"].as_str().unwrap_or(""),
            response["class"].as_str().unwrap_or(""),
            response["title"].as_str().unwrap_or("")
        );
        print_menu(&items, 0);
    })
}

/// The items of an open popup menu (a menu window of the class `#32768`).
fn menus_popup(args: &[&str]) -> Result<()> {
    let mut session_path = None;
    let mut target = Target::default();
    let mut json_output = false;
    let mut at = 0;
    while at < args.len() {
        match args[at] {
            "--title" => target.titles.push(value(args, &mut at, "--title")?.to_owned()),
            "--handle" => target.handles.push(parse_handle(value(args, &mut at, "--handle")?)?),
            "--json" => json_output = true,
            option if is_option(option) => bail!("unknown option {option}"),
            argument => session_path = Some(argument.to_owned()),
        }
        at += 1;
    }
    let session = load_session(&session_path.context("usage: cargo xtask gui menupopup <session> --handle <hwnd>")?)?;
    ensure_alive(&session)?;
    ensure_helper(&session)?;
    let response = request(
        Path::new(&session.session),
        "menupopup",
        json!({ "target": target.to_json() }),
        REQUEST_TIMEOUT,
    )?;
    let items: Vec<win32::MenuItem> = serde_json::from_value(response["menus"].clone()).context("the popup menu")?;
    print_result(json_output, &response, || {
        println!(
            "popup menu {} [{}] {:?}",
            response["handle"].as_str().unwrap_or(""),
            response["class"].as_str().unwrap_or(""),
            response["title"].as_str().unwrap_or("")
        );
        print_menu(&items, 0);
    })
}

fn print_menu(items: &[win32::MenuItem], depth: usize) {
    for item in items {
        let state = match (item.enabled, item.checked) {
            (true, true) => " [checked]",
            (false, _) => " [disabled]",
            _ => "",
        };
        if item.separator {
            println!("{}-", "  ".repeat(depth));
            continue;
        }
        println!(
            "{}{} (id {}, position {}){}",
            "  ".repeat(depth),
            if item.text.is_empty() {
                "<no text>"
            } else {
                item.text.as_str()
            },
            item.id,
            item.position,
            state
        );
        print_menu(&item.submenu, depth + 1);
    }
}

// ---------------------------------------------------------------------------
// close
// ---------------------------------------------------------------------------

fn close(args: &[&str]) -> Result<()> {
    let mut session_path = None;
    let mut timeout = CLOSE_TIMEOUT;
    let mut json_output = false;
    let mut at = 0;
    while at < args.len() {
        match args[at] {
            "--timeout" => {
                timeout = Duration::from_secs(
                    value(args, &mut at, "--timeout")?
                        .parse()
                        .context("--timeout takes seconds")?,
                )
            }
            "--json" => json_output = true,
            option if is_option(option) => bail!("unknown option {option}"),
            argument => session_path = Some(argument.to_owned()),
        }
        at += 1;
    }
    let path = session_path.context("usage: cargo xtask gui close <session>")?;
    let session = load_session(&path)?;
    let session_path = PathBuf::from(&session.session);
    let helper_alive = win32::process_creation_time(session.keeper_pid) == Some(session.keeper_creation_time);
    let response = if helper_alive {
        match request(
            &session_path,
            "close",
            json!({ "timeout": timeout.as_secs() }),
            timeout + REQUEST_TIMEOUT,
        ) {
            Ok(response) => response,
            Err(error) => {
                eprintln!("warning: {error}; stopping the helper directly");
                json!({ "pid": session.pid, "terminated": false, "already_exited": false })
            }
        }
    } else {
        json!({ "pid": session.pid, "terminated": false, "already_exited": true })
    };
    // The helper leaves on its own once it answered; a helper that did not
    // is stopped, and with it the desktop.
    if helper_alive {
        stop_keeper(
            session.keeper_pid,
            session.keeper_creation_time,
            Path::new(&session.keeper_exe),
            Path::new(&session.ready),
            Path::new(&session.stop),
        );
    } else {
        for stale in [
            Path::new(&session.keeper_exe),
            Path::new(&session.ready),
            Path::new(&session.stop),
        ] {
            let _ = fs::remove_file(stale);
        }
    }
    let (request_path, response_path) = request_paths(&session_path);
    let _ = fs::remove_file(request_path);
    let _ = fs::remove_file(response_path);
    let _ = fs::remove_file(beside(&session_path, ".error"));
    fs::remove_file(&session_path).with_context(|| format!("removing the session file {}", session.session))?;
    print_result(json_output, &response, || {
        println!(
            "pid {}: {}",
            response["pid"].as_u64().unwrap_or(0),
            if response["terminated"].as_bool() == Some(true) {
                "did not exit after WM_CLOSE and was terminated"
            } else if response["already_exited"].as_bool() == Some(true) {
                "was no longer running"
            } else {
                "closed"
            }
        );
        println!("removed {}", session.session);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_parse_in_hexadecimal_and_decimal() {
        assert_eq!(parse_handle("0x10A2C").unwrap(), 0x10A2C);
        assert_eq!(parse_handle("68268").unwrap(), 68268);
        assert!(parse_handle("nope").is_err());
    }

    #[test]
    fn points_parse_with_a_comma() {
        assert_eq!(parse_point("12,34").unwrap(), (12, 34));
        assert_eq!(parse_point(" 12 , 34 ").unwrap(), (12, 34));
        assert!(parse_point("12").is_err());
        assert!(parse_point("x,3").is_err());
    }

    #[test]
    fn titles_match_case_insensitively() {
        assert!(contains_title("FO4Edit 4.1.5q", "fo4edit"));
        assert!(contains_title("Module Selection", "select"));
        assert!(!contains_title("Module Selection", "main"));
    }

    #[test]
    fn targets_round_trip_through_the_request_json() {
        let target = Target {
            titles: vec!["Module".to_owned()],
            handles: vec![0x10a2c],
        };
        let restored = Target::from_json(&target.to_json());
        assert_eq!(restored.titles, target.titles);
        assert_eq!(restored.handles, target.handles);
    }

    #[test]
    fn several_captures_are_named_after_their_handle() {
        let window = win32::WindowInfo {
            handle: 0x10a2c,
            pid: 1,
            class: "TfrmMain".to_owned(),
            title: "x".to_owned(),
            rect: win32::Rect {
                left: 0,
                top: 0,
                width: 1,
                height: 1,
            },
            visible: true,
        };
        assert_eq!(capture_path(Path::new("a.png"), &window, false), PathBuf::from("a.png"));
        assert_eq!(
            capture_path(Path::new("a.png"), &window, true),
            PathBuf::from("a-10a2c.png")
        );
    }
}
