// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Development aid: extracts the files of an archive whose path contains
//! a filter. `cargo run -p xedit-assets --example extract <archive> <filter> <dir>`

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let archive = xedit_io::archive::Archive::open(Path::new(&args[0])).expect("archive");
    let names: Vec<String> = archive
        .files()
        .filter(|name| name.contains(&args[1]))
        .map(str::to_owned)
        .collect();
    for name in names {
        let data = archive.read(&name).unwrap().unwrap();
        let path = Path::new(&args[2]).join(name.replace('\\', "/"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }
}
