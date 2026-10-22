// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask gui`: the screen-check tooling of the phase-7 gate.
//!
//! A GUI claim is witnessed by a screenshot or a scripted screen interaction
//! against the 4.1.5q binary, not by the code existing. The programs under
//! test are started on a private desktop of this window station, which is
//! never shown: nothing they draw reaches the user's screen. A session is a
//! JSON file holding the program's process id, its creation time and the
//! desktop, so the later commands (`windows`, `shot`, `keys`, `click`,
//! `menus`, `close`) find the program again from a separate process.
//!
//! See [`windows`] for the commands themselves; this module only keeps the
//! non-Windows build honest, because a desktop is a Windows object.

#[cfg(windows)]
mod windows;

use anyhow::Result;

/// Runs `cargo xtask gui <command>`.
pub fn run(args: &[&str]) -> Result<()> {
    #[cfg(windows)]
    {
        windows::run(args)
    }
    #[cfg(not(windows))]
    {
        let _ = args;
        anyhow::bail!("cargo xtask gui drives Windows desktops and only runs on Windows")
    }
}
