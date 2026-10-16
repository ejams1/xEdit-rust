// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A stand-in for `xtexconv.exe` for `cargo xtask parity bsarchpro`: the
//! GUI's `DDS2XBOX` (`BSArchPro`) runs the tool as
//! `xtexconv.exe -xbox "<file>"` and reads the file back, and the port
//! does the same when the tool is next to the program. The real
//! `xtexconv.exe` is Microsoft's Xenon tool and is not in the release
//! archive, so this stub takes its place on both sides: it rewrites the
//! DDS header of the file as `SetUpHeader` writes an XBox one (the form
//! the console tool produces) and keeps the image data, so both sides
//! pack the same bytes. The harness copies it under the name
//! `xtexconv.exe`; it is a test fixture and is never shipped.

use std::path::Path;

use xedit_io::dds;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("-xbox") || args.len() != 2 {
        eprintln!("usage: xtexconv-stub -xbox <file.dds>");
        std::process::exit(2);
    }
    let path = Path::new(&args[1]);
    let Ok(mut file) = std::fs::read(path) else {
        eprintln!("cannot read {}", path.display());
        std::process::exit(1);
    };
    if !dds::is_dds(&file) || dds::is_xbox(&file) {
        eprintln!("{} is not a PC DDS file", path.display());
        std::process::exit(1);
    }
    let header = dds::DdsHeader::read(&file);
    let format = dds::dxgi(&file);
    let cube = dds::is_cube_map(&file);
    let data = file.split_off(dds::header_size(&file));
    let mut result = vec![0u8; dds::MAX_HEADER_SIZE];
    dds::set_up_header(
        &mut result,
        format,
        header.width as i32,
        header.height as i32,
        header.mip_map_count as i32,
        cube,
        true,
    );
    result.truncate(dds::header_size(&result));
    result.extend(data);
    if let Err(error) = std::fs::write(path, &result) {
        eprintln!("cannot write {}: {error}", path.display());
        std::process::exit(1);
    }
}
