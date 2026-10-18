// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The one-entry zip files BSArchPro keeps its asset lists in
//! (`TZipFile` with an `assets.json` entry). The reader serves the lists
//! the GUI writes and the writer produces a list the GUI reads back; only
//! stored and deflated entries are read, which is what `TZipFile` writes.

use std::io::{Read, Write};
use std::path::Path;

use flate2::Crc;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// The end of central directory record: the offset and size of the central
/// directory and the number of entries.
fn end_of_central_directory(data: &[u8]) -> Option<(usize, usize, usize)> {
    let lowest = data.len().saturating_sub(u16::MAX as usize + 22);
    for at in (lowest..data.len().saturating_sub(21)).rev() {
        if u32_at(data, at) == Some(END_OF_CENTRAL_DIRECTORY) {
            let entries = usize::from(u16_at(data, at + 10)?);
            let size = u32_at(data, at + 12)? as usize;
            let offset = u32_at(data, at + 16)? as usize;
            return Some((offset, size, entries));
        }
    }
    None
}

/// The bytes of the entry `name` of the zip in `data`. `Ok(None)` when
/// `data` is not a zip at all (a plain list file), an error when it is one
/// without the entry.
pub fn read_entry(data: &[u8], name: &str) -> Result<Option<Vec<u8>>, String> {
    let Some((directory, size, entries)) = end_of_central_directory(data) else {
        return Ok(None);
    };
    if directory.checked_add(size).is_none_or(|end| end > data.len()) {
        return Err("the zip central directory lies outside the file".to_owned());
    }
    let mut at = directory;
    for _ in 0..entries {
        if u32_at(data, at) != Some(CENTRAL_HEADER) {
            return Err("the zip central directory is damaged".to_owned());
        }
        let method = u16_at(data, at + 10).ok_or("the zip entry is truncated")?;
        let compressed_size = u32_at(data, at + 20).ok_or("the zip entry is truncated")? as usize;
        let name_length = usize::from(u16_at(data, at + 28).ok_or("the zip entry is truncated")?);
        let extra_length = usize::from(u16_at(data, at + 30).ok_or("the zip entry is truncated")?);
        let comment_length = usize::from(u16_at(data, at + 32).ok_or("the zip entry is truncated")?);
        let local = u32_at(data, at + 42).ok_or("the zip entry is truncated")? as usize;
        let entry_name = data
            .get(at + 46..at + 46 + name_length)
            .ok_or("the zip entry is truncated")?;
        if entry_name == name.as_bytes() {
            if u32_at(data, local) != Some(LOCAL_HEADER) {
                return Err("the zip local header is damaged".to_owned());
            }
            let local_name = usize::from(u16_at(data, local + 26).ok_or("the zip local header is truncated")?);
            let local_extra = usize::from(u16_at(data, local + 28).ok_or("the zip local header is truncated")?);
            let start = local + 30 + local_name + local_extra;
            let packed = data
                .get(start..start + compressed_size)
                .ok_or("the zip entry data lies outside the file")?;
            let unpacked = match method {
                0 => packed.to_vec(),
                8 => {
                    let mut unpacked = Vec::new();
                    DeflateDecoder::new(packed)
                        .read_to_end(&mut unpacked)
                        .map_err(|error| format!("the zip entry does not inflate: {error}"))?;
                    unpacked
                }
                method => return Err(format!("the zip entry uses compression method {method}")),
            };
            return Ok(Some(unpacked));
        }
        at += 46 + name_length + extra_length + comment_length;
    }
    Err(format!("the zip holds no {name}"))
}

/// Writes `name` with `contents` as the only entry of a deflated zip at
/// `path`, as `TZipFile.zmWrite` does.
pub fn write_entry(path: &Path, name: &str, contents: &[u8]) -> Result<(), String> {
    let mut encoder = DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(contents)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let compressed = encoder
        .finish()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let mut crc = Crc::new();
    crc.update(contents);
    let crc = crc.sum();
    let compressed_size = u32::try_from(compressed.len()).unwrap_or(u32::MAX);
    let contents_size = u32::try_from(contents.len()).unwrap_or(u32::MAX);
    let name_bytes = name.as_bytes();
    let mut zip = Vec::new();
    // Local file header: version 2.0, no flags, deflate, 1980-01-01.
    zip.extend_from_slice(&LOCAL_HEADER.to_le_bytes());
    zip.extend_from_slice(&20u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&8u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&0x0021u16.to_le_bytes());
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&compressed_size.to_le_bytes());
    zip.extend_from_slice(&contents_size.to_le_bytes());
    zip.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(name_bytes);
    zip.extend_from_slice(&compressed);
    let directory = zip.len();
    // Central directory entry.
    zip.extend_from_slice(&CENTRAL_HEADER.to_le_bytes());
    zip.extend_from_slice(&20u16.to_le_bytes());
    zip.extend_from_slice(&20u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&8u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&0x0021u16.to_le_bytes());
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&compressed_size.to_le_bytes());
    zip.extend_from_slice(&contents_size.to_le_bytes());
    zip.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&0u32.to_le_bytes());
    zip.extend_from_slice(&0u32.to_le_bytes());
    zip.extend_from_slice(name_bytes);
    let directory_size = zip.len() - directory;
    // End of central directory.
    zip.extend_from_slice(&END_OF_CENTRAL_DIRECTORY.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&1u16.to_le_bytes());
    zip.extend_from_slice(&1u16.to_le_bytes());
    zip.extend_from_slice(&u32::try_from(directory_size).unwrap_or(u32::MAX).to_le_bytes());
    zip.extend_from_slice(&u32::try_from(directory).unwrap_or(u32::MAX).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    std::fs::write(path, zip).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_entry_reads_back() {
        let path = std::env::temp_dir().join(format!("xedit-io-zip-{}.bsarch", std::process::id()));
        let contents = br#"{"Packing":{"ArchiveType":5},"Assets":[[1,"a.nif","",""]]}"#;
        write_entry(&path, "assets.json", contents).unwrap();
        let data = std::fs::read(&path).unwrap();
        assert_eq!(
            read_entry(&data, "assets.json").unwrap().as_deref(),
            Some(contents.as_slice())
        );
        assert!(read_entry(&data, "other.json").is_err());
        assert_eq!(read_entry(b"not a zip", "assets.json").unwrap(), None);
        let _ = std::fs::remove_file(path);
    }
}
