// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Development aid: loads every NIF of an archive (or one file), saves it
//! and reports the files whose bytes differ from the input or that fail.
//!
//! `cargo run --release -p xedit-assets --example nif_roundtrip <archive> [<path filter>] [--json <dir>]`

use std::path::Path;

use xedit_assets::data_format_nif::NifFile;
use xedit_io::archive::Archive;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let archive = Archive::open(Path::new(&args[0])).expect("archive");
    let filter = args
        .get(1)
        .filter(|arg| !arg.starts_with("--"))
        .cloned()
        .unwrap_or_default();
    let json_dir = args
        .iter()
        .position(|arg| arg == "--json")
        .and_then(|index| args.get(index + 1))
        .cloned();
    let mut names: Vec<String> = archive
        .files()
        .filter(|name| (name.ends_with(".nif") || name.ends_with(".kf")) && name.contains(&filter))
        .map(str::to_owned)
        .collect();
    names.sort();
    let (mut equal, mut different, mut failed) = (0, 0, 0);
    for name in &names {
        let data = archive.read(name).expect("read").expect("present");
        let mut nif = NifFile::new().expect("nif");
        match nif.load_from_data(&data).and_then(|()| nif.save_to_data()) {
            Ok(saved) => {
                if saved == data {
                    equal += 1;
                } else {
                    different += 1;
                    if different <= 10 {
                        let first = saved.iter().zip(&data).position(|(a, b)| a != b);
                        println!(
                            "different: {name} ({} -> {} bytes, first difference at {first:?})",
                            data.len(),
                            saved.len()
                        );
                    }
                }
                if let Some(dir) = &json_dir {
                    let mut nif = NifFile::new().expect("nif");
                    nif.load_from_data(&data).expect("load");
                    let json = nif.to_json(false).expect("json");
                    let path = Path::new(dir).join(format!("{name}.json"));
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(path, json).unwrap();
                }
            }
            Err(error) => {
                failed += 1;
                if failed <= 10 {
                    println!("failed: {name}: {error}");
                }
            }
        }
    }
    println!(
        "{} files: {equal} equal, {different} different, {failed} failed",
        names.len()
    );
}
