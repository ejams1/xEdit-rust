// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A one-off generator of the atlas probe's texture fixtures. Run with
//! `cargo test -p xedit-assets --test gen_atlas_fixtures -- --ignored` to
//! write them under `tests/fixtures/parity/atlas/`; the files are committed
//! and this test is kept as the record of how they were made.

use xedit_io::dds::{self, Dxgi};

fn bc1(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let mut file = vec![0u8; dds::MAX_HEADER_SIZE];
    dds::set_up_header(&mut file, Dxgi::BC1_UNORM, width as i32, height as i32, 1, false, false);
    file.truncate(dds::header_size(&file));
    let mut state = u32::from(seed) | 1;
    for _ in 0..(width * height / 2) {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        file.push((state >> 24) as u8);
    }
    file
}

#[test]
#[ignore = "generates the committed fixtures once"]
fn generate() {
    let root = std::path::Path::new("../../tests/fixtures/parity/atlas/textures/t");
    std::fs::create_dir_all(root).unwrap();
    for (name, width, height, seed) in [
        ("one.dds", 64, 64, 7u8),
        ("one_n.dds", 64, 64, 11),
        ("two.dds", 32, 32, 13),
        ("two_n.dds", 32, 32, 17),
    ] {
        std::fs::write(root.join(name), bc1(width, height, seed)).unwrap();
        println!("wrote {name}");
    }
}
