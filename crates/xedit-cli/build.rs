// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Links the `xedit` binary with a larger main thread stack on Windows.
//!
//! Windows gives a process a 1 MiB main thread stack by default. clap's
//! command construction (the whole subcommand tree of `Cli::parse`) needs
//! more than that in a debug build once the CLI surface is as large as this
//! one: the process overflowed its stack before it parsed `--version`, and
//! the daemon tests of `crates/xedit-cli/tests/daemon.rs`, which run the
//! debug binary, failed with `has overflowed its stack`. Release builds stay
//! under 1 MiB, which is why the parity harness never saw it. 16 MiB is
//! twice the usual Linux main thread stack and leaves room for the CLI to
//! grow.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg-bins=/STACK:16777216");
    }
}
