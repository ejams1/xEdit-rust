// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Writing a captured window as a PNG.
//!
//! The only formats the screen checks produce are screenshots, so this is a
//! minimal encoder: 8-bit RGBA, no interlacing, every row with filter 0,
//! compressed with zlib. `flate2` is already a dependency of `xedit-io`, so
//! nothing new enters the build for it.

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use flate2::Crc;

/// Writes `rgba` (4 bytes per pixel, row major) as a PNG at `path`.
pub fn write(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let expected = width as usize * height as usize * 4;
    anyhow::ensure!(
        rgba.len() == expected,
        "the image buffer holds {} bytes, not {expected}",
        rgba.len()
    );
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut out = Vec::with_capacity(expected / 4);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &compressed(width, height, rgba)?);
    chunk(&mut out, b"IEND", &[]);
    fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The zlib stream of the filtered rows.
fn compressed(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let stride = width as usize * 4;
    let mut rows = Vec::with_capacity((stride + 1) * height as usize);
    for row in rgba.chunks_exact(stride) {
        rows.push(0);
        rows.extend_from_slice(row);
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    encoder.write_all(&rows)?;
    Ok(encoder.finish()?)
}

/// One PNG chunk: length, type, data and the CRC of type and data.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = Crc::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.sum().to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tiny_image_is_a_png_with_the_declared_size() {
        // Red, green; blue, white.
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
        let dir = xedit_io::temp::TempDir::new(format!("xedit-gui-png-{}", std::process::id()));
        let path = dir.join("two.png");
        write(&path, 2, 2, &rgba).unwrap();
        let read = fs::read(&path).unwrap();
        assert_eq!(&read[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let header = read
            .windows(4)
            .position(|window| window == b"IHDR")
            .expect("an IHDR chunk");
        assert_eq!(&read[header + 4..header + 12], &[0, 0, 0, 2, 0, 0, 0, 2]);
        assert!(read.windows(4).any(|window| window == b"IDAT"));
        assert!(read.windows(4).any(|window| window == b"IEND"));
    }
}
