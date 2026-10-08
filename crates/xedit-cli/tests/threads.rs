// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `--threads 1` and `--threads 8` give the same dump and the same saved
//! bytes for a few corpus plugins. The plugins come from the local game
//! installs (`XEDIT_FO4_DATA`, `XEDIT_SSE_DATA`, as for the parity harness);
//! a game whose variable is not set is skipped.

use std::path::{Path, PathBuf};
use std::process::Command;

const PLUGINS: [(&str, &str, &str); 3] = [
    ("XEDIT_FO4_DATA", "fo4", "DLCworkshop01.esm"),
    ("XEDIT_SSE_DATA", "sse", "HearthFires.esm"),
    ("XEDIT_SSE_DATA", "sse", "ccBGSSSE025-AdvDSGS.esm"),
];

fn xedit(threads: usize, args: &[&str]) -> Vec<u8> {
    let output = Command::new(env!("CARGO_BIN_EXE_xedit"))
        .arg("--threads")
        .arg(threads.to_string())
        .args(args)
        .output()
        .expect("xedit runs");
    assert!(
        output.status.success(),
        "xedit {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn corpus() -> Vec<(&'static str, PathBuf)> {
    PLUGINS
        .iter()
        .filter_map(|(variable, game, plugin)| {
            let path = Path::new(&std::env::var_os(variable)?).join(plugin);
            path.is_file().then_some((*game, path))
        })
        .collect()
}

#[test]
fn one_thread_and_eight_give_the_same_dump() {
    for (game, path) in corpus() {
        let path = path.to_str().unwrap();
        let serial = xedit(1, &["dump", "--game", game, path]);
        let parallel = xedit(8, &["dump", "--game", game, path]);
        assert!(!serial.is_empty(), "{path} dumps");
        assert!(serial == parallel, "{path}: the dump depends on the thread count");
    }
}

#[test]
fn one_thread_and_eight_save_the_same_bytes() {
    let dir = std::env::temp_dir().join(format!("xedit-threads-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (game, path) in corpus() {
        let mut saved = Vec::new();
        for threads in [1, 8] {
            let output = dir.join(format!("{threads}.esm"));
            xedit(
                threads,
                &[
                    "--edit",
                    "--game",
                    game,
                    "--load",
                    path.to_str().unwrap(),
                    "save",
                    "--no-backup",
                    "--output",
                    output.to_str().unwrap(),
                ],
            );
            saved.push(std::fs::read(&output).unwrap());
        }
        assert!(
            saved[0] == saved[1],
            "{}: the save depends on the thread count",
            path.display()
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}
