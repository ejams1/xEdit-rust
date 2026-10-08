// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbBSArchive.pas (TwbBSArchive.CreateArchive, Save,
// Pack, PackData and TwbCustomBSArchive.DetectFlags)

//! Creating an archive: the file table is computed when the archive is
//! created, the data of the files is appended with `pack` and the tables are
//! written over the reserved start of the file by `save`.

use std::cmp::Ordering;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};

use super::asset::{AssetType, asset_type_by_extension, asset_type_by_folder, split, split_dir_name};
use super::{
    ARCHIVE_COMPRESS, ARCHIVE_DEFAULT, ARCHIVE_EMBEDNAME, ARCHIVE_RETAINNAME, ARCHIVE_STARTUPSTR, ARCHIVE_UNKNOWN10,
    ARCHIVE_XMEM, Archive, ArchiveError, ArchiveType, CHUNK_HEADER_SIZE_DX10, CHUNK_HEADER_SIZE_GNRL,
    COMPRESSION_METHOD_LZ4, COMPRESSION_METHOD_ZLIB, FILE_FLAG_NAMES, FILE_FO4_TAIL, FILE_FONTS, FILE_MENUS,
    FILE_MESHES, FILE_MISC, FILE_SHADERS, FILE_SIZE_COMPRESS, FILE_SOUNDS, FILE_TEXTURES, FILE_TREES, FILE_VOICES,
    FileChunk, FileEntry, FolderTes4, HEADER_VERSION_FO3, HEADER_VERSION_FO4_V1, HEADER_VERSION_SF_V2,
    HEADER_VERSION_SF_V3, HEADER_VERSION_SSE, HEADER_VERSION_TES4, MAGIC_BSA, MAGIC_BTDX, MAGIC_DX10, MAGIC_GNRL,
    MAGIC_TES3, SIZE_OF_FO4, SIZE_OF_SF_V2, SIZE_OF_SF_V3, SIZE_OF_TES3, SIZE_OF_TES4, Writer, os_error_text, texture,
};
use crate::compression::CompressionType;
use crate::encoding::{ansi_bytes, lower_case};
use crate::hash::{self, LookupHash, lookup_hash, lookup_hash_text};

/// One file to create in an archive: the entry of the files list that
/// `CreateArchive` takes (`aFilesList` with its `Objects` and
/// `aFilesCompression`).
#[derive(Debug, Clone)]
pub struct PackFile {
    /// The name in the archive (`aFilesList[i]`).
    pub name: String,
    /// Whatever identifies the file to the packer (`aFilesList.Objects[i]`).
    pub file_object: usize,
    /// Whether the data is stored compressed (`aFilesCompression[i]`).
    pub compress: bool,
}

/// Which chunk of a file an operation stores: the data of a general file or
/// one mipmap chunk of a texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkSlot {
    File,
    Tex(usize),
}

/// The data of one chunk, ready to be written: compressed when the file is
/// (`PackData`'s `aData`/`aSize`) with the size it has unpacked.
pub(crate) struct Prepared {
    /// What is stored; `None` for data that the caller knows is in the
    /// archive already, so that it need not be read or compressed.
    pub data: Option<Vec<u8>>,
    /// `aUncompressedSize`.
    pub uncompressed_size: usize,
    /// The lookup hash of the data (`DataHash`), when the archive shares data.
    pub hash: Option<LookupHash>,
}

impl Writer {
    fn write(&mut self, bytes: &[u8]) -> Result<(), ArchiveError> {
        self.out
            .write_all(bytes)
            .map_err(|error| ArchiveError(format!("Write error: {}", os_error_text(&error))))?;
        self.position += bytes.len() as u64;
        Ok(())
    }

    fn u8(&mut self, value: u8) -> Result<(), ArchiveError> {
        self.write(&[value])
    }

    fn u16(&mut self, value: u16) -> Result<(), ArchiveError> {
        self.write(&value.to_le_bytes())
    }

    fn u32(&mut self, value: u32) -> Result<(), ArchiveError> {
        self.write(&value.to_le_bytes())
    }

    fn u64(&mut self, value: u64) -> Result<(), ArchiveError> {
        self.write(&value.to_le_bytes())
    }

    fn i64(&mut self, value: i64) -> Result<(), ArchiveError> {
        self.write(&value.to_le_bytes())
    }

    /// `WriteStringLen`: a length byte, the text in ANSI, and a terminator
    /// that the length counts when `term`.
    fn string_len(&mut self, text: &str, term: bool) -> Result<(), ArchiveError> {
        let mut bytes = ansi_bytes(text);
        if term {
            bytes.push(0);
        }
        self.u8(bytes.len() as u8)?;
        self.write(&bytes)
    }

    /// `WriteStringLen16`: a length word and the text in ANSI.
    fn string_len16(&mut self, text: &str) -> Result<(), ArchiveError> {
        let bytes = ansi_bytes(text);
        self.u16(bytes.len() as u16)?;
        self.write(&bytes)
    }

    /// `WriteStringTerm`: the text in ANSI and a terminating zero.
    fn string_term(&mut self, text: &str) -> Result<(), ArchiveError> {
        let mut bytes = ansi_bytes(text);
        bytes.push(0);
        self.write(&bytes)
    }

    fn seek_to_start(&mut self) -> Result<(), ArchiveError> {
        self.out
            .seek(SeekFrom::Start(0))
            .map_err(|error| ArchiveError(format!("Seek error: {}", os_error_text(&error))))?;
        self.position = 0;
        Ok(())
    }
}

/// Port of `TwbCustomBSArchive.DetectFlags`: the archive flags and the file
/// flags that suit the files of an archive of the type.
pub fn detect_flags(kind: ArchiveType, files: &[PackFile]) -> (u32, u32) {
    let mut file_flags = 0u32;
    let mut archive_flags = ARCHIVE_DEFAULT;
    if kind == ArchiveType::Tes4 {
        archive_flags |= ARCHIVE_EMBEDNAME | ARCHIVE_XMEM | ARCHIVE_UNKNOWN10;
    }

    for file in files {
        let mut asset_type = asset_type_by_folder(&file.name);
        if asset_type == AssetType::None {
            asset_type = asset_type_by_extension(&file.name);
        }

        // Determine the file flags.
        match asset_type {
            AssetType::Mesh => file_flags |= FILE_MESHES,
            AssetType::Texture => file_flags |= FILE_TEXTURES,
            AssetType::Sound => file_flags |= FILE_SOUNDS,
            AssetType::Voice => file_flags |= FILE_VOICES,
            AssetType::SpeedTree => file_flags |= FILE_TREES,
            AssetType::Font => file_flags |= FILE_FONTS,
            AssetType::Menus => file_flags |= FILE_SHADERS,
            AssetType::DistantLod | AssetType::LodSettings => file_flags |= FILE_MESHES | FILE_MISC,
            _ => file_flags |= FILE_MISC,
        }

        // Oblivion only.
        if kind == ArchiveType::Tes4 && lower_case(&file.name).ends_with(".xml") {
            file_flags |= FILE_MENUS;
        }

        // Determine the archive flags. Packed scripts can't be added to
        // objects in the SSE CK if the archive was packed without the
        // "RetainNames" flag (the scripts aren't shown in the script adding
        // window).
        if asset_type == AssetType::Script {
            archive_flags |= ARCHIVE_RETAINNAME;
        }
    }

    // Final flags detection. Menus, shaders and fonts are Oblivion only.
    if kind != ArchiveType::Tes4 {
        file_flags &= !(FILE_MENUS | FILE_SHADERS | FILE_FONTS);
    }

    // The misc file flag is not in Skyrim SE.
    if kind == ArchiveType::Sse {
        file_flags &= !FILE_MISC;
    }

    // The embedded names flag in textures only archives.
    if file_flags == FILE_TEXTURES {
        archive_flags |= ARCHIVE_EMBEDNAME;
    }

    // The startup strings flag in archives with meshes.
    if file_flags & FILE_MESHES != 0 {
        archive_flags |= ARCHIVE_STARTUPSTR;
    }

    // The retain name flag in archives with sounds.
    if file_flags & FILE_SOUNDS != 0 {
        archive_flags |= ARCHIVE_RETAINNAME;
    }

    // The compressed flag if at least one file is compressed.
    if files.iter().any(|file| file.compress) {
        archive_flags |= ARCHIVE_COMPRESS;
    }
    (archive_flags, file_flags)
}

/// `HashSortTES3`: by the low dword of the hash, then the high dword.
fn hash_sort_tes3(a: &FileEntry, b: &FileEntry) -> Ordering {
    let (d1, n1) = (a.name_hash64 as u32, (a.name_hash64 >> 32) as u32);
    let (d2, n2) = (b.name_hash64 as u32, (b.name_hash64 >> 32) as u32);
    d1.cmp(&d2).then(n1.cmp(&n2))
}

/// `HashSortTES4`: by the folder hash, then the file name hash.
fn hash_sort_tes4(a: &FileEntry, b: &FileEntry) -> Ordering {
    a.dir_hash64.cmp(&b.dir_hash64).then(a.name_hash64.cmp(&b.name_hash64))
}

/// `String2Magic`: the first four characters as bytes.
fn string_to_magic(text: &str) -> [u8; 4] {
    let mut magic = [0u8; 4];
    for (slot, c) in magic.iter_mut().zip(text.chars()) {
        *slot = c as u32 as u8;
    }
    magic
}

impl Archive {
    /// Port of `TwbBSArchive.CreateArchive`: starts an archive with the
    /// files, computes the tables and reserves the space they will take at
    /// the start of the file. The data follows with `pack` and the tables
    /// are written by `save`.
    pub fn create_archive(
        &mut self,
        file_name: &str,
        kind: ArchiveType,
        files: &[PackFile],
    ) -> Result<(), ArchiveError> {
        if self.map.is_some() {
            self.close();
        }
        if self.writer.is_some() {
            return Err(ArchiveError("Archive is already being created".to_owned()));
        }
        if files.is_empty() {
            return Err(ArchiveError("Files list to pack is empty".to_owned()));
        }

        self.file_name = file_name.to_owned();
        self.kind = kind;

        if self.compression_type == CompressionType::None {
            self.compression_type = kind.default_compression();
        } else if !kind.supports_compression(self.compression_type) {
            return Err(ArchiveError("Unsupported compression type".to_owned()));
        }

        // Magic, version.
        match kind {
            ArchiveType::Tes3 => self.header.magic = MAGIC_TES3,
            ArchiveType::Tes4 => {
                self.header.magic = MAGIC_BSA;
                self.header.version = HEADER_VERSION_TES4;
            }
            ArchiveType::Fo3 => {
                self.header.magic = MAGIC_BSA;
                self.header.version = HEADER_VERSION_FO3;
            }
            ArchiveType::Sse => {
                self.header.magic = MAGIC_BSA;
                self.header.version = HEADER_VERSION_SSE;
            }
            ArchiveType::Fo4 => {
                self.header.magic = MAGIC_BTDX;
                self.header.magic2 = MAGIC_GNRL;
                self.header.version = HEADER_VERSION_FO4_V1;
            }
            ArchiveType::Fo4Dds => {
                self.header.magic = MAGIC_BTDX;
                self.header.magic2 = MAGIC_DX10;
                self.header.version = HEADER_VERSION_FO4_V1;
            }
            ArchiveType::Sf | ArchiveType::SfDds => {
                self.header.magic = MAGIC_BTDX;
                self.header.magic2 = if kind == ArchiveType::Sf {
                    MAGIC_GNRL
                } else {
                    MAGIC_DX10
                };
                // The official Archive2 tool creates v3 archives for lz4
                // compression only.
                self.header.version = if self.compression_type == CompressionType::LZ4 {
                    HEADER_VERSION_SF_V3
                } else {
                    HEADER_VERSION_SF_V2
                };
            }
            ArchiveType::None => return Err(ArchiveError("Unsupported archive type".to_owned())),
        }

        let count = files.len();
        self.files = vec![FileEntry::default(); count];
        self.header.file_count = count as u32;

        match kind {
            // Create Morrowind.
            ArchiveType::Tes3 => {
                let mut names_length = 0usize;
                // Fill in the file entries and calculate the total names length.
                for (entry, file) in self.files.iter_mut().zip(files) {
                    entry.name = lower_case(&file.name);
                    entry.file_object = file.file_object;
                    entry.name_hash64 = hash::tes3(&entry.name);
                    names_length += entry.name.chars().count() + 1; // including the terminator
                }
                // Sort the files by hashes.
                self.files.sort_by(hash_sort_tes3);

                // The offset to the hash table.
                self.data_offset = SIZE_OF_TES3
                    + 8 * count as i64 // file sizes/offsets
                    + 4 * count as i64 // archive directory/name offsets
                    + names_length as i64; // filename records

                // Stored without the header size.
                self.header.hash_offset = (self.data_offset - SIZE_OF_TES3) as u32;

                // The offset to the files data.
                self.data_offset += 8 * count as i64; // hash table
            }

            // Create Oblivion, Fallout 3, New Vegas, Skyrim, Skyrim SE.
            ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse => {
                self.header.folder_names_length = 0;
                self.header.file_names_length = 0;
                let tes4 = kind == ArchiveType::Tes4;
                // Fill in the file entries and calculate the total names length.
                for (entry, file) in self.files.iter_mut().zip(files) {
                    entry.name = lower_case(&file.name);
                    entry.file_object = file.file_object;
                    entry.compress = file.compress;
                    let (position, directory, name) = split_dir_name(&file.name);
                    if position == 0 {
                        return Err(ArchiveError(format!("File is missing the folder part: {}", file.name)));
                    }
                    entry.dir_hash64 = hash::tes4_raw(&directory, false, tes4);
                    entry.name_hash64 = hash::tes4_raw(&name, true, tes4);
                    // Calculate the file names length.
                    self.header.file_names_length += name.chars().count() as u32 + 1; // + terminator
                }
                // Sort the files by hashes.
                self.files.sort_by(hash_sort_tes4);

                // Create the folders.
                let mut previous = 0u64;
                self.folders_tes4.clear();
                for (index, entry) in self.files.iter().enumerate() {
                    if entry.dir_hash64 != previous {
                        previous = entry.dir_hash64;
                        let (_, directory, _) = split_dir_name(&entry.name);
                        // Calculate the folder names length: the terminator
                        // only, the length prefix is not counted.
                        self.header.folder_names_length += directory.chars().count() as u32 + 1;
                        self.folders_tes4.push(FolderTes4 {
                            hash: entry.dir_hash64,
                            offset: 0,
                            name: directory,
                            files: Vec::new(),
                        });
                    }
                    self.folders_tes4.last_mut().expect("a folder").files.push(index);
                }
                let folder_count = self.folders_tes4.len();
                self.header.folder_count = folder_count as u32;
                self.header.folders_offset = SIZE_OF_TES4;

                // Calculate the folders offsets. At the end the data offset
                // holds the total size of the header, the folder and the file
                // records: the start of the files data.
                let mut offset = i64::from(self.header.folders_offset) + 16 * folder_count as i64;
                // The SSE folder record has 2 additional padding fields.
                if kind == ArchiveType::Sse {
                    offset += 2 * 4 * folder_count as i64;
                }
                // Offsets are stored including this value.
                offset += i64::from(self.header.file_names_length);
                for folder in &mut self.folders_tes4 {
                    folder.offset = offset;
                    // Add the folder name length: the length prefix and the terminator.
                    offset += folder.name.chars().count() as i64 + 2;
                    // Add the file records length.
                    offset += 16 * folder.files.len() as i64;
                }
                self.data_offset = offset;

                // Flags detection.
                let (flags, file_flags) = detect_flags(kind, files);
                self.header.flags = flags;
                self.header.file_flags = file_flags;
                let textures_only = self.header.file_flags == FILE_TEXTURES;

                // Flags override.
                if self.archive_flags != 0 {
                    self.header.flags = self.archive_flags | ARCHIVE_DEFAULT;
                }
                if self.file_flags != 0 {
                    self.header.file_flags = self.file_flags;
                }

                // Fixes to avoid game crashes with wrong flags. Embedded
                // names in textures only archives, might crash on other files.
                //
                // UPSTREAM-QUIRK: the code clears ARCHIVE_EMBEDNAME (0x100) in
                // the file flags, where that bit is FILE_MISC. The misc flag is
                // lost in every archive that holds more than textures.
                if !textures_only {
                    self.header.file_flags &= !ARCHIVE_EMBEDNAME;
                } else if !self.packer_assigned
                    && kind == ArchiveType::Sse
                    && self.header.flags & ARCHIVE_EMBEDNAME != 0
                {
                    // The SSE crashing bug: textures with embedded names must be compressed.
                    for entry in &mut self.files {
                        entry.compress = true;
                    }
                }
            }

            // Create Fallout 4, Starfield.
            ArchiveType::Fo4 | ArchiveType::Fo4Dds | ArchiveType::Sf | ArchiveType::SfDds => {
                for (entry, file) in self.files.iter_mut().zip(files) {
                    let parts = split(&file.name);
                    if parts.folder_no_delimiter.is_empty() {
                        return Err(ArchiveError(format!("File is missing the folder part: {}", file.name)));
                    }
                    entry.name = file.name.clone();
                    entry.file_object = file.file_object;
                    entry.compress = file.compress;
                    entry.dir_hash32 = hash::fo4(&parts.folder_no_delimiter);
                    entry.name_hash32 = hash::fo4(&parts.file_name_no_extension);
                    entry.ext = string_to_magic(&lower_case(&parts.extension_no_dot));
                }

                // The offset to the files data.
                self.data_offset = match kind {
                    ArchiveType::Fo4 | ArchiveType::Fo4Dds => SIZE_OF_FO4,
                    _ => match self.header.version {
                        HEADER_VERSION_SF_V2 => SIZE_OF_SF_V2,
                        _ => SIZE_OF_SF_V3,
                    },
                };

                if matches!(kind, ArchiveType::Fo4 | ArchiveType::Sf) {
                    // File records have a fixed length in a general archive.
                    self.data_offset += 36 * count as i64;
                } else {
                    // A variable file record length depending on the number of DDS chunks.
                    for index in 0..count {
                        // Get the required chunks if the dds info callback is
                        // present, otherwise assume the maximum.
                        let chunks = match self.dds_info_proc {
                            Some(proc) => {
                                let info = proc(self, &self.files[index].name);
                                self.dds_mip_chunk_num(info.width, info.height, info.mip_maps)
                            }
                            None => self.max_chunk_count,
                        };
                        // 24 is the size of the file record, 24 the size of each texture chunk.
                        self.data_offset += 24 + 24 * i64::from(chunks);
                    }
                }
            }

            ArchiveType::None => unreachable!("rejected above"),
        }

        for entry in &mut self.files {
            entry.lookup_hash = lookup_hash_text(&entry.name, true);
        }
        self.by_hash.clear();
        for (index, entry) in self.files.iter().enumerate() {
            self.by_hash.entry(entry.lookup_hash).or_insert(index);
        }

        let file = File::create(&self.file_name).map_err(|error| {
            ArchiveError(format!(
                "Cannot create file \"{}\". {}",
                self.file_name,
                os_error_text(&error)
            ))
        })?;
        let mut writer = Writer {
            out: BufWriter::with_capacity(1 << 20, file),
            position: 0,
        };
        // Reserve the space for the header.
        let zeros = vec![0u8; self.data_offset.max(0) as usize];
        writer.write(&zeros)?;
        self.writer = Some(writer);
        Ok(())
    }

    /// Whether `create_archive` was called and the archive was not saved.
    pub fn is_writing(&self) -> bool {
        self.writer.is_some()
    }

    fn writer_mut(&mut self) -> Result<&mut Writer, ArchiveError> {
        self.writer
            .as_mut()
            .ok_or_else(|| ArchiveError("Archive is not in writing mode".to_owned()))
    }

    /// Port of `TwbBSArchive.Save`: writes the tables over the space that
    /// `create_archive` reserved and closes the file.
    pub fn save(&mut self) -> Result<(), ArchiveError> {
        if self.writer.is_none() {
            return Err(ArchiveError("Archive is not in writing mode".to_owned()));
        }
        let header = self.header.clone();
        let kind = self.kind;
        let compression_type = self.compression_type;
        let files = std::mem::take(&mut self.files);
        let folders = std::mem::take(&mut self.folders_tes4);
        let result = self.save_tables(&header, kind, compression_type, &files, &folders);
        self.files = files;
        self.folders_tes4 = folders;
        let size = result?;
        self.archive_size = size;
        if let Some(mut writer) = self.writer.take() {
            writer
                .out
                .flush()
                .map_err(|error| ArchiveError(format!("Write error: {}", os_error_text(&error))))?;
        }
        Ok(())
    }

    fn save_tables(
        &mut self,
        header: &super::Header,
        kind: ArchiveType,
        compression_type: CompressionType,
        files: &[FileEntry],
        folders: &[FolderTes4],
    ) -> Result<i64, ArchiveError> {
        let data_offset = self.data_offset;
        let writer = self.writer_mut()?;
        let mut header_out = header.clone();
        match kind {
            // Save Morrowind.
            ArchiveType::Tes3 => {
                if let Some(file) = files.iter().find(|file| file.chunk.offset == 0) {
                    return Err(ArchiveError(format!("Packed file has no data: {}", file.name)));
                }
                let archive_size = writer.position as i64;
                writer.seek_to_start()?;
                // Write the header.
                writer.u32(header.magic)?;
                writer.u32(header.hash_offset)?;
                writer.u32(header.file_count)?;
                // File sizes/offsets: the offsets are relative.
                for file in files {
                    writer.u32(file.chunk.size)?;
                    writer.u32((file.chunk.offset - data_offset) as u32)?;
                }
                // Archive directory/name offsets.
                let mut offset = 0u32;
                for file in files {
                    writer.u32(offset)?;
                    offset += file.name.chars().count() as u32 + 1; // including the terminator
                }
                // Filename records.
                for file in files {
                    writer.string_term(&file.name)?;
                }
                // The hash table.
                for file in files {
                    writer.u64(file.name_hash64)?;
                }
                Ok(archive_size)
            }

            // Save Oblivion, Fallout 3, New Vegas, Skyrim, Skyrim SE.
            ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse => {
                if let Some(file) = files.iter().find(|file| file.chunk.offset == 0) {
                    return Err(ArchiveError(format!("Packed file has no data: {}", file.name)));
                }
                let archive_size = writer.position as i64;
                writer.seek_to_start()?;
                // Write the header.
                writer.u32(header.magic)?;
                writer.u32(header.version)?;
                writer.u32(header.folders_offset)?;
                writer.u32(header.flags)?;
                writer.u32(header.folder_count)?;
                writer.u32(header.file_count)?;
                writer.u32(header.folder_names_length)?;
                writer.u32(header.file_names_length)?;
                writer.u32(header.file_flags)?;
                // Write the folder records.
                let sse = kind == ArchiveType::Sse;
                for folder in folders {
                    writer.u64(folder.hash)?;
                    writer.u32(folder.files.len() as u32)?;
                    if sse {
                        writer.u32(0)?; // padding
                    }
                    writer.u32(folder.offset as u32)?;
                    if sse {
                        writer.u32(0)?; // padding
                    }
                }
                // Write the file records.
                for folder in folders {
                    writer.string_len(&folder.name, true)?;
                    for &index in &folder.files {
                        let file = &files[index];
                        writer.u64(file.name_hash64)?;
                        writer.u32(file.chunk.size)?;
                        writer.u32(file.chunk.offset as u32)?;
                    }
                }
                // Write the file names: the name only, without the folder.
                for folder in folders {
                    for &index in &folder.files {
                        let (_, _, name) = split_dir_name(&files[index].name);
                        writer.string_term(&name)?;
                    }
                }
                Ok(archive_size)
            }

            // Save Fallout 4, Starfield.
            ArchiveType::Fo4 | ArchiveType::Fo4Dds | ArchiveType::Sf | ArchiveType::SfDds => {
                for file in files {
                    if header.magic2 == MAGIC_GNRL && file.chunk.offset == 0 {
                        return Err(ArchiveError(format!("Packed file has no data: {}", file.name)));
                    }
                    if header.magic2 == MAGIC_DX10 && file.dds.tex_chunks.is_empty() {
                        return Err(ArchiveError(format!("Packed file has no data: {}", file.name)));
                    }
                }

                // The file names table at the end of the file.
                header_out.file_table_offset = writer.position as i64;
                for file in files {
                    // archive2.exe uses /
                    writer.string_len16(&file.name.replace('\\', "/"))?;
                }

                let archive_size = writer.position as i64;
                writer.seek_to_start()?;
                // Write the header.
                writer.u32(header_out.magic)?;
                writer.u32(header_out.version)?;
                writer.u32(header_out.magic2)?;
                writer.u32(header_out.file_count)?;
                writer.i64(header_out.file_table_offset)?;
                // The Starfield header.
                if matches!(header.version, HEADER_VERSION_SF_V2 | HEADER_VERSION_SF_V3) {
                    writer.u64(1)?; // always set to 1, immediately discarded on load
                }
                if header.version == HEADER_VERSION_SF_V3 {
                    header_out.compression_method = if compression_type == CompressionType::LZ4 {
                        COMPRESSION_METHOD_LZ4
                    } else {
                        COMPRESSION_METHOD_ZLIB
                    };
                    writer.u32(header_out.compression_method)?;
                }

                // Write the file entries.
                for file in files {
                    writer.u32(file.name_hash32)?;
                    writer.write(&file.ext)?;
                    writer.u32(file.dir_hash32)?;
                    writer.u8(file.mod_index)?; // always 0
                    if header.magic2 == MAGIC_GNRL {
                        writer.u8(1)?; // always a single chunk
                        writer.u16(CHUNK_HEADER_SIZE_GNRL)?;
                        writer.i64(file.chunk.offset)?;
                        writer.u32(file.chunk.packed_size)?;
                        writer.u32(file.chunk.size)?;
                        writer.u32(FILE_FO4_TAIL)?;
                    } else {
                        writer.u8(file.dds.tex_chunks.len() as u8)?;
                        writer.u16(CHUNK_HEADER_SIZE_DX10)?;
                        writer.u16(file.dds.height)?;
                        writer.u16(file.dds.width)?;
                        writer.u8(file.dds.num_mips)?;
                        writer.u8(file.dds.dxgi_format)?;
                        writer.u8(file.dds.flags)?;
                        writer.u8(file.dds.tile_mode)?;
                        for chunk in &file.dds.tex_chunks {
                            writer.i64(chunk.chunk.offset)?;
                            writer.u32(chunk.chunk.packed_size)?;
                            writer.u32(chunk.chunk.size)?;
                            writer.u16(chunk.start_mip)?;
                            writer.u16(chunk.end_mip)?;
                            writer.u32(FILE_FO4_TAIL)?;
                        }
                    }
                }
                self.header.file_table_offset = header_out.file_table_offset;
                self.header.compression_method = header_out.compression_method;
                Ok(archive_size)
            }

            ArchiveType::None => Err(ArchiveError("Unsupported archive type".to_owned())),
        }
    }

    fn chunk_mut(&mut self, file: usize, slot: ChunkSlot) -> &mut FileChunk {
        match slot {
            ChunkSlot::File => &mut self.files[file].chunk,
            ChunkSlot::Tex(index) => &mut self.files[file].dds.tex_chunks[index].chunk,
        }
    }

    /// Port of `FindPackedData`: gives the chunk the place of data of the
    /// same size and hash that is written already, when there is any.
    pub(crate) fn find_packed_data(
        &mut self,
        file: usize,
        slot: ChunkSlot,
        size: u32,
        hash: Option<LookupHash>,
    ) -> bool {
        let Some(hash) = hash.filter(|_| self.share_data) else {
            return false;
        };
        let Some(&found) = self.packed_data.get(&(size, hash)) else {
            return false;
        };
        *self.chunk_mut(file, slot) = found;
        // In DDS archives count the shared files for the first 0 mipmap chunk only.
        let later_mip = match slot {
            ChunkSlot::Tex(index) => self.files[file].dds.tex_chunks[index].start_mip != 0,
            ChunkSlot::File => false,
        };
        if !later_mip {
            self.archive_shared_files += 1;
        }
        self.archive_shared_size += if found.packed_size != 0 {
            i64::from(found.packed_size)
        } else {
            i64::from(found.size)
        };
        true
    }

    /// Port of `PackData` for data that is ready to be stored: finds the
    /// data in the archive already or appends it, and fills in the chunk.
    pub(crate) fn pack_chunk(&mut self, file: usize, slot: ChunkSlot, prepared: Prepared) -> Result<(), ArchiveError> {
        let size = prepared.uncompressed_size as u32;
        if self.find_packed_data(file, slot, size, prepared.hash) {
            return Ok(());
        }
        let data = prepared
            .data
            .ok_or_else(|| ArchiveError("Shared data is missing from the archive".to_owned()))?;
        let compress = self.files[file].compress;
        let kind = self.kind;
        let flags = self.header.flags;
        let name = self.files[file].name.clone();
        let writer = self.writer_mut()?;
        let start = writer.position;

        // The embedded file name for Fallout 3/NV/Skyrim/Skyrim SE.
        if matches!(kind, ArchiveType::Fo3 | ArchiveType::Sse) && flags & ARCHIVE_EMBEDNAME != 0 {
            writer.string_len(&name, false)?;
        }

        // If compressed, the uncompressed size goes first for
        // Oblivion/Fallout 3/NV/Skyrim/Skyrim SE.
        if matches!(kind, ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse) && compress {
            writer.u32(prepared.uncompressed_size as u32)?;
        }

        // Write the file data.
        writer.write(&data)?;
        let end = writer.position;

        // Updating the file entry.
        let chunk = self.chunk_mut(file, slot);
        chunk.offset = start as i64;
        match kind {
            ArchiveType::Tes3 => chunk.size = prepared.uncompressed_size as u32,
            ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse => {
                // The size includes the embedded file name and the uncompressed size field.
                chunk.size = (end - start) as u32;
                if compress != (flags & ARCHIVE_COMPRESS != 0) {
                    chunk.size |= FILE_SIZE_COMPRESS;
                }
                // This is purely for the warnings check on newly created
                // archives, to signal that the file was compressed.
                if compress {
                    chunk.packed_size = data.len() as u32;
                }
            }
            _ => {
                chunk.size = prepared.uncompressed_size as u32;
                if compress {
                    chunk.packed_size = data.len() as u32;
                }
            }
        }
        let stored = *chunk;

        // AddPackedData.
        if self.share_data
            && let Some(hash) = prepared.hash
        {
            self.packed_data.insert((size, hash), stored);
        }
        Ok(())
    }

    /// The data of a file stored by `PackData` without a packer: the hash
    /// of the data, then the compressed data when the file is compressed.
    fn prepare_unpacked(&self, file: usize, data: &[u8], skip_if_shared: bool) -> Result<Prepared, ArchiveError> {
        let hash = self.share_data.then(|| lookup_hash(data));
        let mut prepared = Prepared {
            data: None,
            uncompressed_size: data.len(),
            hash,
        };
        if skip_if_shared
            && self
                .packed_data
                .contains_key(&(data.len() as u32, hash.unwrap_or_default()))
        {
            return Ok(prepared);
        }
        prepared.data = Some(if self.files[file].compress {
            if self.compression_type == CompressionType::None {
                return Err(ArchiveError("Undefined compression type".to_owned()));
            }
            self.compression_type.compress(data)?
        } else {
            data.to_vec()
        });
        Ok(prepared)
    }

    /// Port of `TwbBSArchive.Pack(aFile; aData)` without a packer: stores
    /// the data of the file `index`, compressing it when the file is
    /// compressed.
    pub fn pack_entry(&mut self, index: usize, data: &[u8]) -> Result<(), ArchiveError> {
        if self.writer.is_none() {
            return Err(ArchiveError("Archive is not in writing mode".to_owned()));
        }
        if self.kind.is_dds() {
            return texture::pack_dds(self, index, data);
        }
        let prepared = self.prepare_unpacked(index, data, true)?;
        self.pack_chunk(index, ChunkSlot::File, prepared)
    }

    /// Port of `TwbBSArchive.Pack(aFileName; aData)`.
    pub fn pack(&mut self, file_name: &str, data: &[u8]) -> Result<(), ArchiveError> {
        let index = self
            .by_hash
            .get(&lookup_hash_text(file_name, true))
            .copied()
            .ok_or_else(|| ArchiveError(format!("File to pack not found in archive: {file_name}")))?;
        self.pack_entry(index, data)
    }

    /// Marks that the data comes from a packer (`Packer`), which compressed
    /// it already and splits textures into chunks.
    pub(crate) fn set_packer_assigned(&mut self, assigned: bool) {
        self.packer_assigned = assigned;
    }

    /// The names of the file flags that are set, for messages.
    pub fn file_flag_names(flags: u32) -> Vec<&'static str> {
        FILE_FLAG_NAMES
            .iter()
            .enumerate()
            .filter(|(i, _)| (flags >> i) & 1 == 1)
            .map(|(_, name)| *name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xedit-archive-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn files(names: &[(&str, bool)]) -> Vec<PackFile> {
        names
            .iter()
            .enumerate()
            .map(|(index, (name, compress))| PackFile {
                name: (*name).to_owned(),
                file_object: index,
                compress: *compress,
            })
            .collect()
    }

    /// The files written and read back are the files read from the new archive.
    fn round_trip(kind: ArchiveType, compression: CompressionType, compress: bool) {
        let dir = temp_dir(&format!("{kind:?}-{compress}"));
        let path = dir.join("test.bsa").display().to_string();
        let list = files(&[
            ("meshes\\armor\\a.nif", compress),
            ("meshes\\armor\\b.nif", compress),
            ("textures\\x\\c.dds", compress),
            ("sound\\fx\\d.wav", false),
            ("meshes\\same.nif", compress),
            ("meshes\\same2.nif", compress),
        ]);
        let contents: Vec<Vec<u8>> = (0..list.len())
            .map(|i| {
                if i == 5 {
                    // The same bytes as the file before, so the data is shared.
                    vec![b'a' + 4; 3000 + 4 * 100]
                } else {
                    vec![b'a' + i as u8; 3000 + i * 100]
                }
            })
            .collect();
        let mut archive = Archive::new();
        archive.set_share_data(true);
        archive.set_compression_type(compression);
        archive.create_archive(&path, kind, &list).unwrap();
        for (file, data) in list.iter().zip(&contents) {
            archive.pack(&file.name, data).unwrap();
        }
        archive.save().unwrap();
        assert_eq!(archive.archive_shared_files(), 1, "{kind:?}");

        let read = Archive::open(std::path::Path::new(&path)).unwrap();
        assert_eq!(read.archive_type(), kind);
        assert_eq!(read.count(), list.len());
        for (file, data) in list.iter().zip(&contents) {
            assert_eq!(&read.unpack(&file.name).unwrap(), data, "{kind:?} {}", file.name);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_archive_type_reads_back_what_was_packed() {
        for kind in [
            ArchiveType::Tes3,
            ArchiveType::Tes4,
            ArchiveType::Fo3,
            ArchiveType::Sse,
            ArchiveType::Fo4,
            ArchiveType::Sf,
        ] {
            let compression = kind.default_compression();
            round_trip(kind, compression, false);
            if kind != ArchiveType::Tes3 {
                round_trip(kind, compression, true);
            }
        }
        round_trip(ArchiveType::Sf, CompressionType::LZ4, true);
    }

    #[test]
    fn a_failed_archive_is_deleted() {
        let dir = temp_dir("failed");
        let path = dir.join("failed.ba2").display().to_string();
        {
            let mut archive = Archive::new();
            archive
                .create_archive(&path, ArchiveType::Fo4, &files(&[("meshes\\a.nif", false)]))
                .unwrap();
            assert!(std::path::Path::new(&path).exists());
            // Saving without packing the file fails and the archive is dropped.
            assert_eq!(
                archive.save().err().map(|error| error.0),
                Some("Packed file has no data: meshes\\a.nif".to_owned())
            );
        }
        assert!(!std::path::Path::new(&path).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn creating_checks_its_input() {
        let mut archive = Archive::new();
        assert_eq!(
            archive
                .create_archive("x.bsa", ArchiveType::Tes4, &[])
                .err()
                .map(|error| error.0),
            Some("Files list to pack is empty".to_owned())
        );
        assert_eq!(
            archive
                .create_archive("x.bsa", ArchiveType::Tes4, &files(&[("a.nif", false)]))
                .err()
                .map(|error| error.0),
            Some("File is missing the folder part: a.nif".to_owned())
        );
        let mut archive = Archive::new();
        archive.set_compression_type(CompressionType::LZ4);
        assert_eq!(
            archive
                .create_archive("x.ba2", ArchiveType::Fo4, &files(&[("a\\b.nif", false)]))
                .err()
                .map(|error| error.0),
            Some("Unsupported compression type".to_owned())
        );
    }

    #[test]
    fn flags_follow_the_files() {
        let list = files(&[("meshes\\a.nif", true), ("sound\\fx\\b.wav", false)]);
        let (archive_flags, file_flags) = detect_flags(ArchiveType::Fo3, &list);
        assert_eq!(file_flags, FILE_MESHES | FILE_SOUNDS);
        assert_eq!(
            archive_flags,
            ARCHIVE_DEFAULT | ARCHIVE_COMPRESS | ARCHIVE_STARTUPSTR | ARCHIVE_RETAINNAME
        );
        // Textures only: embedded names.
        let (archive_flags, file_flags) = detect_flags(ArchiveType::Sse, &files(&[("textures\\a.dds", true)]));
        assert_eq!(file_flags, FILE_TEXTURES);
        assert_eq!(archive_flags, ARCHIVE_DEFAULT | ARCHIVE_COMPRESS | ARCHIVE_EMBEDNAME);
        // Oblivion always has the embedded names, xmem and the unknown flag.
        let (archive_flags, _) = detect_flags(ArchiveType::Tes4, &files(&[("meshes\\a.nif", false)]));
        assert_eq!(
            archive_flags,
            ARCHIVE_DEFAULT | ARCHIVE_EMBEDNAME | ARCHIVE_XMEM | ARCHIVE_UNKNOWN10 | ARCHIVE_STARTUPSTR
        );
    }
}
