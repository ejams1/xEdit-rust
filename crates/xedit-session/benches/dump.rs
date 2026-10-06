// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Load and dump time of a vanilla plugin, to compare with the oracle:
//! `xDump.exe -SSE -q` loads `Skyrim.esm` and `Update.esm` and dumps
//! `Update.esm` (938 MB of text) in about 150 seconds on the reference
//! machine. The benchmarks do nothing when `XEDIT_SSE_DATA` is not set.

use std::path::Path;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use xedit_core::implementation::{clear_files_map, reset_load_order_slots};
use xedit_core::interface::globals::GameMode;
use xedit_session::dump::{dump_file, setup_game};

fn plugin(name: &str) -> Option<String> {
    let data = std::env::var("XEDIT_SSE_DATA").ok()?;
    let path = format!("{data}/{name}");
    Path::new(&path).exists().then_some(path)
}

/// Loads `Update.esm` with its master and writes the dump to a sink.
fn dump_update(path: &str) {
    clear_files_map();
    reset_load_order_slots();
    let mode = setup_game("sse").unwrap();
    assert_eq!(mode, GameMode::gmSSE);
    let mut sink = std::io::sink();
    dump_file(path, mode, &mut sink).unwrap();
}

fn benches(c: &mut Criterion) {
    let Some(path) = plugin("Update.esm") else {
        return;
    };
    let mut group = c.benchmark_group("sse");
    group.sample_size(10).measurement_time(Duration::from_secs(120));
    group.bench_function("dump Update.esm", |b| {
        b.iter(|| {
            // The dump needs a deep stack for the element tree.
            let path = path.clone();
            std::thread::Builder::new()
                .stack_size(1 << 30)
                .spawn(move || dump_update(&path))
                .unwrap()
                .join()
                .unwrap();
        });
    });
    group.finish();
}

criterion_group!(dump, benches);
criterion_main!(dump);
