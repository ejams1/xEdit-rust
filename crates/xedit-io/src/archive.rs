// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbBSA.pas and Core/wbBSArchive.pas

//! Reading the files of a BSA (Morrowind excluded) or a general BA2
//! archive: the parts the localization and the resource lookup need.
//!
//! Not ported: Morrowind archives, the DX10 texture archives and writing.

use std::collections::HashMap;
use std::path::Path;

use crate::compression::{CompressionError, CompressionType};
use crate::mapped_file::MappedFile;

/// A failure to read an archive. The message names the archive and what is wrong.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ArchiveError(pub String);

impl From<CompressionError> for ArchiveError {
    fn from(error: CompressionError) -> Self {
        ArchiveError(error.0)
    }
}

/// Where a file lives in the archive and how it is stored.
#[derive(Debug, Clone, Copy)]
struct Entry {
    offset: u64,
    size: u32,
    /// `None` for a file stored as it is; the compression otherwise.
    compression: Option<CompressionType>,
    /// The BSA archive flag that puts the file name in front of the data.
    embedded_name: bool,
    /// The unpacked size of a BA2 file, known from the record.
    unpacked_size: Option<u32>,
}

/// An opened archive with its file table.
pub struct Archive {
    path: String,
    map: MappedFile,
    /// Lower-case paths with `\` separators to the entries.
    entries: HashMap<String, Entry>,
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?))
}

/// The path as the archive table keys it: lower case with backslashes.
pub fn normalize_path(path: &str) -> String {
    path.replace('/', "\\").trim_start_matches('\\').to_ascii_lowercase()
}

impl Archive {
    /// Opens the archive and reads its file table.
    pub fn open(path: &Path) -> Result<Self, ArchiveError> {
        let name = path.display().to_string();
        let map = MappedFile::open(path).map_err(|error| ArchiveError(format!("[{name}] {error}")))?;
        let entries = {
            let bytes: &[u8] = &map;
            match bytes.get(..4) {
                Some(b"BSA\0") => read_bsa(bytes, &name)?,
                Some(b"BTDX") => read_ba2(bytes, &name)?,
                _ => return Err(ArchiveError(format!("[{name}] Unknown archive format"))),
            }
        };
        Ok(Archive {
            path: name,
            map,
            entries,
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Whether the archive holds the file. `path` is relative to the data
    /// folder and in any case or separator.
    pub fn contains(&self, path: &str) -> bool {
        self.entries.contains_key(&normalize_path(path))
    }

    /// The contents of the file, decompressed.
    pub fn read(&self, path: &str) -> Result<Option<Vec<u8>>, ArchiveError> {
        let Some(entry) = self.entries.get(&normalize_path(path)) else {
            return Ok(None);
        };
        let bytes: &[u8] = &self.map;
        let mut offset = usize::try_from(entry.offset).map_err(|_| self.truncated(path))?;
        let mut size = entry.size as usize;
        if entry.embedded_name {
            let length = *bytes.get(offset).ok_or_else(|| self.truncated(path))? as usize;
            offset += 1 + length;
            size = size.saturating_sub(1 + length);
        }
        let data = bytes.get(offset..offset + size).ok_or_else(|| self.truncated(path))?;
        match entry.compression {
            None => Ok(Some(data.to_vec())),
            Some(compression) => {
                let (unpacked, packed) = match entry.unpacked_size {
                    Some(unpacked) => (unpacked as usize, data),
                    None => (
                        u32_at(data, 0).ok_or_else(|| self.truncated(path))? as usize,
                        data.get(4..).ok_or_else(|| self.truncated(path))?,
                    ),
                };
                let mut out = vec![0u8; unpacked];
                compression.decompress(packed, &mut out)?;
                Ok(Some(out))
            }
        }
    }

    fn truncated(&self, path: &str) -> ArchiveError {
        ArchiveError(format!("[{}] File {path} is outside of the archive", self.path))
    }
}

/// The table of a BSA of Oblivion through Skyrim Special Edition.
fn read_bsa(bytes: &[u8], name: &str) -> Result<HashMap<String, Entry>, ArchiveError> {
    let bad = || ArchiveError(format!("[{name}] Archive header is truncated"));
    let version = u32_at(bytes, 4).ok_or_else(bad)?;
    let folder_record_offset = u32_at(bytes, 8).ok_or_else(bad)? as usize;
    let archive_flags = u32_at(bytes, 12).ok_or_else(bad)?;
    let folder_count = u32_at(bytes, 16).ok_or_else(bad)? as usize;
    let file_count = u32_at(bytes, 20).ok_or_else(bad)? as usize;
    let total_file_name_length = u32_at(bytes, 28).ok_or_else(bad)? as usize;
    if !(103..=105).contains(&version) {
        return Err(ArchiveError(format!("[{name}] Unsupported BSA version {version}")));
    }
    let has_folder_names = archive_flags & 0x1 != 0;
    let has_file_names = archive_flags & 0x2 != 0;
    let compressed_by_default = archive_flags & 0x4 != 0;
    let embedded_names = version >= 104 && archive_flags & 0x100 != 0;
    let compression = if version >= 105 {
        CompressionType::LZ4F
    } else {
        CompressionType::ZLib
    };
    let folder_record_size = if version >= 105 { 24 } else { 16 };

    // The folder records: the file record block of each folder.
    let mut folders = Vec::with_capacity(folder_count);
    for index in 0..folder_count {
        let record = folder_record_offset + index * folder_record_size;
        let count = u32_at(bytes, record + 8).ok_or_else(bad)? as usize;
        let offset = if version >= 105 {
            u64_at(bytes, record + 16).ok_or_else(bad)? as usize
        } else {
            u32_at(bytes, record + 12).ok_or_else(bad)? as usize
        };
        // UPSTREAM-QUIRK: the offset counts the file name block as if it came first.
        folders.push((count, offset - total_file_name_length));
    }

    // The file records, with the folder names in front of each block. The
    // file names follow the last block.
    let mut files: Vec<(String, Entry)> = Vec::with_capacity(file_count);
    let mut names_start = folder_record_offset + folder_count * folder_record_size;
    for (count, block) in folders {
        let mut position = block;
        let folder_name = if has_folder_names {
            let length = *bytes.get(position).ok_or_else(bad)? as usize;
            let text = bytes.get(position + 1..position + length).ok_or_else(bad)?;
            position += 1 + length;
            String::from_utf8_lossy(text.strip_suffix(b"\0").unwrap_or(text)).into_owned()
        } else {
            String::new()
        };
        for _ in 0..count {
            let size = u32_at(bytes, position + 8).ok_or_else(bad)?;
            let offset = u32_at(bytes, position + 12).ok_or_else(bad)?;
            let compressed = compressed_by_default != (size & 0x4000_0000 != 0);
            files.push((
                folder_name.clone(),
                Entry {
                    offset: u64::from(offset),
                    size: size & 0x3FFF_FFFF,
                    compression: compressed.then_some(compression),
                    embedded_name: embedded_names,
                    unpacked_size: None,
                },
            ));
            position += 16;
        }
        names_start = names_start.max(position);
    }

    // The file names, one after the other.
    let mut entries = HashMap::with_capacity(file_count);
    if has_file_names {
        let names = bytes
            .get(names_start..names_start + total_file_name_length)
            .ok_or_else(bad)?;
        let mut file_names = names.split(|&byte| byte == 0);
        for (folder, entry) in files {
            let file_name = file_names.next().ok_or_else(bad)?;
            let file_name = String::from_utf8_lossy(file_name);
            let path = if folder.is_empty() {
                file_name.into_owned()
            } else {
                format!("{folder}\\{file_name}")
            };
            entries.insert(normalize_path(&path), entry);
        }
    }
    Ok(entries)
}

/// The table of a general BA2 of Fallout 4.
fn read_ba2(bytes: &[u8], name: &str) -> Result<HashMap<String, Entry>, ArchiveError> {
    let bad = || ArchiveError(format!("[{name}] Archive header is truncated"));
    let kind = bytes.get(8..12).ok_or_else(bad)?;
    if kind != b"GNRL" {
        return Err(ArchiveError(format!(
            "[{name}] Unsupported BA2 type {}",
            String::from_utf8_lossy(kind)
        )));
    }
    let version = u32_at(bytes, 4).ok_or_else(bad)?;
    let file_count = u32_at(bytes, 12).ok_or_else(bad)? as usize;
    let name_table_offset = u64_at(bytes, 16).ok_or_else(bad)? as usize;
    // Starfield archives (versions 2 and 3) add a value that is always 1,
    // and version 3 the compression method, where 3 is LZ4.
    let mut header_size = 24;
    let mut compression = CompressionType::ZLib;
    if matches!(version, 2 | 3) {
        header_size += 8;
    }
    if version == 3 {
        if u32_at(bytes, header_size).ok_or_else(bad)? == 3 {
            compression = CompressionType::LZ4;
        }
        header_size += 4;
    }
    let mut records = Vec::with_capacity(file_count);
    for index in 0..file_count {
        let record = header_size + index * 36;
        let offset = u64_at(bytes, record + 16).ok_or_else(bad)?;
        let packed_size = u32_at(bytes, record + 24).ok_or_else(bad)?;
        let unpacked_size = u32_at(bytes, record + 28).ok_or_else(bad)?;
        records.push(if packed_size == 0 {
            Entry {
                offset,
                size: unpacked_size,
                compression: None,
                embedded_name: false,
                unpacked_size: None,
            }
        } else {
            Entry {
                offset,
                size: packed_size,
                compression: Some(compression),
                embedded_name: false,
                unpacked_size: Some(unpacked_size),
            }
        });
    }
    let mut entries = HashMap::with_capacity(file_count);
    let mut position = name_table_offset;
    for entry in records {
        let length = u16_at(bytes, position).ok_or_else(bad)? as usize;
        let text = bytes.get(position + 2..position + 2 + length).ok_or_else(bad)?;
        position += 2 + length;
        entries.insert(normalize_path(&String::from_utf8_lossy(text)), entry);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plugin of the game, or `None` when the game is not installed here.
    fn data_file(data_env: &str, name: &str) -> Option<String> {
        let data = std::env::var(data_env).ok()?;
        let path = format!("{data}/{name}");
        Path::new(&path).exists().then_some(path)
    }

    #[test]
    fn reads_the_strings_of_skyrim_se() {
        let Some(path) = data_file("XEDIT_SSE_DATA", "Skyrim - Interface.bsa") else {
            return;
        };
        let archive = Archive::open(Path::new(&path)).unwrap();
        assert!(archive.contains("Strings\\Skyrim_English.STRINGS"));
        let strings = archive.read("strings/skyrim_english.strings").unwrap().unwrap();
        assert!(strings.len() > 100_000, "{} bytes", strings.len());
        let count = u32::from_le_bytes(strings[..4].try_into().unwrap());
        assert!(count > 10_000, "{count} strings");
    }

    #[test]
    fn reads_the_strings_of_fallout_4() {
        let Some(path) = data_file("XEDIT_FO4_DATA", "Fallout4 - Interface.ba2") else {
            return;
        };
        let archive = Archive::open(Path::new(&path)).unwrap();
        let strings = archive.read("strings\\fallout4_en.strings").unwrap().unwrap();
        assert!(strings.len() > 100_000, "{} bytes", strings.len());
    }
}
