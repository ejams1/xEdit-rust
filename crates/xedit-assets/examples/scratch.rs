// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Development aid for one-off checks.

use xedit_assets::data_format::df_calc_hash;
use xedit_assets::data_format_material::MaterialFile;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data = std::fs::read(&args[0]).unwrap();
    let mut file = MaterialFile::new_bgsm().unwrap();
    file.load_from_data(&data).unwrap();
    let path = &args[1];
    println!("hash {:08x}", df_calc_hash(path));
    let before = file.tree.edit_values(file.root, path).unwrap();
    file.tree.set_edit_values(file.root, path, &args[2]).unwrap();
    let after = file.tree.edit_values(file.root, path).unwrap();
    println!("before {before:?} after {after:?}");
    let saved = file.save_to_data().unwrap();
    println!("saved equal: {}", saved == data);
    for name in &file.tree.raw_def(file.root).defs {
        if df_calc_hash(&name.name) == df_calc_hash(path) {
            println!("collides with {}", name.name);
        }
    }
}
