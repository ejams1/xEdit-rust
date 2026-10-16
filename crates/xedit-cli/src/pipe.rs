// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Windows named pipe transport of `xedit serve --pipe NAME`. Platform
//! code stays in this module (design decision 4); the protocol is the same
//! lines of JSON-RPC as on stdio.
//!
//! The pipe is `\\.\pipe\NAME` unless NAME already starts with `\\`. The
//! daemon serves one client at a time, because there is one session, and
//! accepts the next client when one disconnects. Only the user that runs
//! the daemon (and SYSTEM) may open the pipe, and remote clients are
//! refused.
//!
//! # Concurrency
//!
//! Two clients, or two daemons, never share one session:
//!
//! - **One client at a time.** While a client is connected the daemon is
//!   inside the request loop and holds the one instance of the pipe, so a
//!   client that connects then fails with `ERROR_PIPE_BUSY` (231) until the
//!   first one disconnects. A client that wants to wait can ask Windows to
//!   wait with `WaitNamedPipe`, or simply retry; nothing of a client that
//!   never got the pipe reaches the session.
//! - **One daemon per name.** The instance is created with
//!   `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a second `xedit serve --pipe NAME`
//!   fails at startup with `ERROR_ACCESS_DENIED` (5), which
//!   [`listen`](windows::listen) reports as `NAME is already served by
//!   another xedit serve`. Two daemons would otherwise load the same
//!   plugins and both write them at their own `files.save`, which is how
//!   competing sessions corrupt a plugin.
//! - **The loaded files are the session's.** The named pipe is the only
//!   shared state the daemon owns: a client of the daemon reaches the
//!   session of its operator, while two daemons on two names are two
//!   processes whose edits reach disk only at their own `files.save`.

#[cfg(not(windows))]
use std::io;

/// The full path of a pipe from the name given on the command line.
pub fn pipe_path(name: &str) -> String {
    if name.starts_with(r"\\") {
        name.to_owned()
    } else {
        format!(r"\\.\pipe\{name}")
    }
}

#[cfg(windows)]
pub use windows::listen;

/// Accepts clients on the pipe `name` and calls `serve` for each with the
/// connected pipe; `serve` returns whether to accept another client.
#[cfg(not(windows))]
pub fn listen(_name: &str, _serve: impl FnMut(std::fs::File) -> io::Result<bool>) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "named pipes are Windows only: use stdio",
    ))
}

#[cfg(windows)]
mod windows {
    use std::fs::File;
    use std::io;
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    use super::pipe_path;

    /// Full access for the owner and SYSTEM, nothing for anyone else.
    const SDDL: &str = "D:P(A;;GA;;;OW)(A;;GA;;;SY)";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Creates the one instance of the pipe and waits for a client.
    fn accept(path: &[u16]) -> io::Result<File> {
        let mut descriptor = null_mut();
        // SAFETY: the SDDL string is NUL terminated; `descriptor` receives a
        // buffer that is freed with `LocalFree` below.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(SDDL).as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        // SAFETY: `path` is NUL terminated and `attributes` outlives the call.
        let handle = unsafe {
            CreateNamedPipeW(
                path.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                1 << 16,
                1 << 16,
                0,
                &attributes,
            )
        };
        let created = io::Error::last_os_error();
        // SAFETY: the descriptor came from the conversion above.
        unsafe { LocalFree(descriptor) };
        if handle == INVALID_HANDLE_VALUE {
            // `FILE_FLAG_FIRST_PIPE_INSTANCE` makes the create of a name
            // that is served already fail with `ERROR_ACCESS_DENIED`: a
            // second daemon on one name would run a second session over the
            // same plugins.
            if created.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!(
                        "{} is already served by another xedit serve: one daemon per pipe name, because one session owns the loaded files",
                        String::from_utf16_lossy(
                            &path[..path.iter().position(|unit| *unit == 0).unwrap_or(path.len())]
                        )
                    ),
                ));
            }
            return Err(created);
        }
        // The File owns the handle from here on and closes it on every path.
        // SAFETY: `handle` is a valid pipe handle that nothing else owns.
        let pipe = unsafe { File::from_raw_handle(handle as RawHandle) };
        // SAFETY: a valid pipe handle; no overlapped I/O.
        let connected = unsafe { ConnectNamedPipe(handle, null_mut()) };
        // A client that connected between the create and the connect makes
        // the call fail with ERROR_PIPE_CONNECTED, which is a success.
        let error = io::Error::last_os_error();
        if connected == 0 && error.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
            return Err(error);
        }
        Ok(pipe)
    }

    pub fn listen(name: &str, mut serve: impl FnMut(File) -> io::Result<bool>) -> io::Result<()> {
        let path = wide(&pipe_path(name));
        loop {
            let pipe = accept(&path)?;
            // A client that vanishes mid-request is not the end of the daemon.
            match serve(pipe) {
                Ok(true) => {}
                Ok(false) => return Ok(()),
                Err(error) => eprintln!("connection closed: {error}"),
            }
        }
    }
}
