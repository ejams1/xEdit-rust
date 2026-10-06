// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Load and dump time of a vanilla plugin, to compare with the oracle.
//! `xDump.exe -SSE -q` reports its own times in the log on stderr: with
//! `Update.esm` as input it loads `Skyrim.esm`, `Update.esm` and the
//! hardcoded records in 2.4 seconds (`Finished loading record. Starting
//! Dump.`) and writes the dump (938 MB of text) in 98 seconds on the
//! reference machine. The benchmarks do nothing when `XEDIT_SSE_DATA` is
//! not set.
//!
//! The load of the port is the same work as the oracle's: the file, its
//! masters, the string tables and the hardcoded records, with every group
//! and record header read. The elements of the records are only built
//! during the dump, as in the oracle.

use std::path::Path;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use xedit_core::implementation::{clear_files_map, reset_load_order_slots};
use xedit_core::interface::globals::GameMode;
use xedit_session::dump::{dump_file, load_file, setup_game};

fn plugin(name: &str) -> Option<String> {
    let data = std::env::var("XEDIT_SSE_DATA").ok()?;
    let path = format!("{data}/{name}");
    Path::new(&path).exists().then_some(path)
}

/// Starts from no loaded files, as a fresh `xDump.exe` process does.
fn fresh_session() -> GameMode {
    clear_files_map();
    reset_load_order_slots();
    let mode = setup_game("sse").unwrap();
    assert_eq!(mode, GameMode::gmSSE);
    mode
}

/// Loads `Update.esm` with its master and the hardcoded records.
fn load_update(path: &str) {
    let mode = fresh_session();
    load_file(path, mode).unwrap();
}

/// Loads `Update.esm` with its master and writes the dump to a sink.
fn dump_update(path: &str) {
    let mode = fresh_session();
    let mut sink = std::io::sink();
    dump_file(path, mode, &mut sink).unwrap();
}

/// Runs `work` on a thread with a stack deep enough for the element tree.
fn on_deep_stack(path: &str, work: fn(&str)) {
    let path = path.to_owned();
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || work(&path))
        .unwrap()
        .join()
        .unwrap();
}

fn benches(c: &mut Criterion) {
    let Some(path) = plugin("Update.esm") else {
        return;
    };
    let mut group = c.benchmark_group("sse");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));
    group.bench_function("load Update.esm", |b| b.iter(|| on_deep_stack(&path, load_update)));
    group.measurement_time(Duration::from_secs(120));
    group.bench_function("dump Update.esm", |b| b.iter(|| on_deep_stack(&path, dump_update)));
    group.finish();
}

criterion_group!(dump, benches);
criterion_main!(dump);
