// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbBSArchive.pas (TwbBSArchive, TwbCustomBSArchive)

//! Bethesda archives: reading, listing and extracting the BSA of Morrowind,
//! Oblivion, Fallout 3, New Vegas and Skyrim (LE and SE) and the BA2 of
//! Fallout 4, Fallout 76 and Starfield, and writing them (`write`), with the
//! parallel packer behind `BSArch` (`packer`).
//!
//! The DX10 texture archives (`Fallout 4 DDS`, `Starfield DDS`) read and
//! list here; what needs the DDS code (packing a texture, extracting one)
//! is the extension point in `texture` that phase 5 step 2 fills in.

mod asset;
pub mod packer;
mod texture;
mod write;

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub use asset::{
    AssetParts, AssetType, asset_type_by_extension, asset_type_by_folder, do_not_compress, do_not_pack,
    extract_file_ext, extract_file_name, get_asset_name, include_trailing_path_delimiter, last_char_pos, split,
    split_dir_name, split_name_ext,
};
pub use packer::{MultiSourcePacker, PackerError};
pub use texture::dxgi_format_name;
pub use write::{PackFile, detect_flags};

use crate::compression::{CompressionError, CompressionType};
use crate::encoding::{ansi_string, lower_case};
use crate::hash::{LookupHash, lookup_hash_text};
use crate::mapped_file::MappedFile;

/// A failure to read or write an archive. The message is the upstream
/// exception message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ArchiveError(pub String);

impl From<CompressionError> for ArchiveError {
    fn from(error: CompressionError) -> Self {
        ArchiveError(error.0)
    }
}

/// Upstream `TwbBSArchiveType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArchiveType {
    None,
    Tes3,
    Tes4,
    Fo3,
    Sse,
    Fo4,
    Fo4Dds,
    Sf,
    SfDds,
}

/// Upstream `BSA_MAX_OFFSET`: the highest file offset the games of the
/// BSA formats read.
pub const BSA_MAX_OFFSET: i64 = i32::MAX as i64;

impl ArchiveType {
    /// Upstream `cArchiveFormatNames`.
    pub fn format_name(self) -> &'static str {
        match self {
            ArchiveType::None => "None",
            ArchiveType::Tes3 => "Morrowind",
            ArchiveType::Tes4 => "Oblivion",
            ArchiveType::Fo3 => "Skyrim LE, New Vegas, Fallout 3",
            ArchiveType::Sse => "Skyrim AE, Skyrim SE",
            ArchiveType::Fo4 => "Fallout 4",
            ArchiveType::Fo4Dds => "Fallout 4 DDS",
            ArchiveType::Sf => "Starfield",
            ArchiveType::SfDds => "Starfield DDS",
        }
    }

    /// Upstream `cArchiveCompressionTypes`: the supported compression types,
    /// the default first.
    pub fn compression_types(self) -> &'static [CompressionType] {
        match self {
            ArchiveType::None | ArchiveType::Tes3 => &[CompressionType::None],
            ArchiveType::Tes4 | ArchiveType::Fo3 => &[CompressionType::ZLib],
            ArchiveType::Sse => &[CompressionType::LZ4F],
            ArchiveType::Fo4 | ArchiveType::Fo4Dds => &[CompressionType::ZLib],
            ArchiveType::Sf => &[CompressionType::ZLib, CompressionType::LZ4],
            ArchiveType::SfDds => &[CompressionType::LZ4, CompressionType::ZLib],
        }
    }

    /// Port of `DefaultCompression`.
    pub fn default_compression(self) -> CompressionType {
        self.compression_types()[0]
    }

    /// Port of `SupportsCompression`.
    pub fn supports_compression(self, compression: CompressionType) -> bool {
        self.compression_types().contains(&compression)
    }

    /// Port of `DefaultExtension`.
    pub fn default_extension(self) -> &'static str {
        if self >= ArchiveType::Fo4 { ".ba2" } else { ".bsa" }
    }

    /// Port of `DefaultSplitSize`.
    pub fn default_split_size(self) -> i64 {
        if matches!(
            self,
            ArchiveType::Tes3 | ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse
        ) {
            BSA_MAX_OFFSET
        } else {
            0
        }
    }

    /// Port of `IsDDSArchive`: the DX10 archives with chunked textures.
    pub fn is_dds(self) -> bool {
        matches!(self, ArchiveType::Fo4Dds | ArchiveType::SfDds)
    }

    /// The BA2 types (Fallout 4 and Starfield, general and DX10).
    pub fn is_ba2(self) -> bool {
        self >= ArchiveType::Fo4
    }
}

/// Port of `IsArchive`: whether the file name has an archive extension.
pub fn is_archive(file_name: &str) -> bool {
    let extension = lower_case(extract_file_ext(file_name));
    extension == ".bsa" || extension == ".ba2"
}

/// Upstream `TwbBSArchiveTarget`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Pc,
    XBox,
}

/// Upstream `cArchiveFlagNames`.
pub const ARCHIVE_FLAG_NAMES: [&str; 10] = [
    "Include Directory Names",
    "Include File Names",
    "Compressed",
    "Retain Directory Names",
    "Retain File Names",
    "Retain File Name Offsets",
    "XBox 360 Archive",
    "Retain Strings During Startup",
    "Embedded File Names",
    "XMem Codec",
];

/// Upstream `cFileFlagNames`.
pub const FILE_FLAG_NAMES: [&str; 9] = [
    "Meshes", "Textures", "Menus", "Sounds", "Voices", "Shaders", "Trees", "Fonts", "Misc",
];

// Header magic numbers and versions.
const MAGIC_TES3: u32 = 0x0000_0100;
const MAGIC_BSA: u32 = u32::from_le_bytes(*b"BSA\0");
const MAGIC_BTDX: u32 = u32::from_le_bytes(*b"BTDX");
const MAGIC_GNRL: u32 = u32::from_le_bytes(*b"GNRL");
const MAGIC_DX10: u32 = u32::from_le_bytes(*b"DX10");

const HEADER_VERSION_TES4: u32 = 0x67;
const HEADER_VERSION_FO3: u32 = 0x68;
const HEADER_VERSION_SSE: u32 = 0x69;
const HEADER_VERSION_FO4_V1: u32 = 0x01;
const HEADER_VERSION_SF_V2: u32 = 0x02;
const HEADER_VERSION_SF_V3: u32 = 0x03;
const HEADER_VERSION_FO4_V7: u32 = 0x07;
const HEADER_VERSION_FO4_V8: u32 = 0x08;

const COMPRESSION_METHOD_ZLIB: u32 = 0;
const COMPRESSION_METHOD_LZ4: u32 = 3;

/// Sizes of the headers (`TwbBSHeader.SizeOf*`).
const SIZE_OF_TES3: i64 = 12;
const SIZE_OF_TES4: u32 = 36;
const SIZE_OF_FO4: i64 = 24;
const SIZE_OF_SF_V2: i64 = SIZE_OF_FO4 + 8;
const SIZE_OF_SF_V3: i64 = SIZE_OF_SF_V2 + 4;

/// Upstream `CHUNK_COUNT_MAX`: the chunks of a DX10 texture the engine reads.
const CHUNK_COUNT_MAX: i32 = 4;
const CHUNK_HEADER_SIZE_GNRL: u16 = 16;
const CHUNK_HEADER_SIZE_DX10: u16 = 24;
const FILE_FO4_TAIL: u32 = 0xBAAD_F00D;
const DDS_FLAG_CUBEMAP: u8 = 0x01;

// Archive flags.
pub const ARCHIVE_PATHNAMES: u32 = 0x0001;
pub const ARCHIVE_FILENAMES: u32 = 0x0002;
pub const ARCHIVE_COMPRESS: u32 = 0x0004;
pub const ARCHIVE_RETAINDIR: u32 = 0x0008;
pub const ARCHIVE_RETAINNAME: u32 = 0x0010;
pub const ARCHIVE_RETAINFOFF: u32 = 0x0020;
pub const ARCHIVE_XBOX360: u32 = 0x0040;
pub const ARCHIVE_STARTUPSTR: u32 = 0x0080;
pub const ARCHIVE_EMBEDNAME: u32 = 0x0100;
pub const ARCHIVE_XMEM: u32 = 0x0200;
pub const ARCHIVE_UNKNOWN10: u32 = 0x0400;
/// The flags that are always set.
pub const ARCHIVE_DEFAULT: u32 = ARCHIVE_PATHNAMES | ARCHIVE_FILENAMES;

// File flags.
pub const FILE_MESHES: u32 = 0x0001;
pub const FILE_TEXTURES: u32 = 0x0002;
pub const FILE_MENUS: u32 = 0x0004;
pub const FILE_SOUNDS: u32 = 0x0008;
pub const FILE_VOICES: u32 = 0x0010;
pub const FILE_SHADERS: u32 = 0x0020;
pub const FILE_TREES: u32 = 0x0040;
pub const FILE_FONTS: u32 = 0x0080;
pub const FILE_MISC: u32 = 0x0100;

/// The BSA size flag that marks a file as compressed (or, in an archive
/// with the compressed flag, as not compressed).
const FILE_SIZE_COMPRESS: u32 = 0x4000_0000;

/// Upstream `TwbBSHeader`.
#[derive(Debug, Clone, Default)]
pub struct Header {
    pub magic: u32,
    pub version: u32,
    pub file_count: u32,
    // Morrowind.
    pub hash_offset: u32,
    // Oblivion through Skyrim Special Edition.
    pub folders_offset: u32,
    pub flags: u32,
    pub folder_count: u32,
    pub folder_names_length: u32,
    pub file_names_length: u32,
    pub file_flags: u32,
    // Fallout 4 and Starfield.
    pub magic2: u32,
    pub file_table_offset: i64,
    // Starfield.
    pub compression_method: u32,
}

/// Upstream `TwbBSFileChunk`: where data lives in the archive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileChunk {
    pub offset: i64,
    pub size: u32,
    pub packed_size: u32,
}

impl FileChunk {
    /// Upstream `GetCompressed` of a chunk.
    pub fn compressed(&self) -> bool {
        self.packed_size != 0
    }
}

/// Upstream `TwbBSFileChunkTex`: a chunk of mipmaps of a DX10 texture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TexChunk {
    pub chunk: FileChunk,
    pub start_mip: u16,
    pub end_mip: u16,
}

/// The `DDS` record of `TwbBSFileEntry`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DdsInfo {
    pub height: u16,
    pub width: u16,
    pub num_mips: u8,
    pub dxgi_format: u8,
    pub flags: u8,
    pub tile_mode: u8,
    pub tex_chunks: Vec<TexChunk>,
}

/// Upstream `TwbBSFileEntry`: a file of an archive.
#[derive(Debug, Clone, Default)]
pub struct FileEntry {
    pub chunk: FileChunk,
    pub name: String,
    pub lookup_hash: LookupHash,
    pub compress: bool,
    /// The identity the packer gave the file (`FileObject`).
    pub file_object: usize,
    pub dir_hash32: u32,
    pub name_hash32: u32,
    pub dir_hash64: u64,
    pub name_hash64: u64,
    pub ext: [u8; 4],
    pub mod_index: u8,
    pub dds: DdsInfo,
}

impl FileEntry {
    /// Port of `TwbBSFileEntry.IsCubeMap`.
    pub fn is_cube_map(&self) -> bool {
        self.dds.flags & DDS_FLAG_CUBEMAP != 0
    }

    /// Port of `TwbBSFileEntry.DXGIFormatName`.
    pub fn dxgi_format_name(&self) -> &'static str {
        dxgi_format_name(self.dds.dxgi_format)
    }
}

/// A folder of a BSA of Oblivion through Skyrim Special Edition
/// (`TwbBSFolderTES4`).
#[derive(Debug, Clone, Default)]
pub(crate) struct FolderTes4 {
    pub hash: u64,
    pub offset: i64,
    pub name: String,
    /// Indices into `Archive::files`.
    pub files: Vec<usize>,
}

/// The state of an archive being written.
pub(crate) struct Writer {
    pub out: BufWriter<File>,
    /// The position of the next byte written.
    pub position: u64,
}

/// Upstream `TwbBSArchive`: an archive being read or written. An opened
/// archive is read only (`&self`) and may be read from several threads.
pub struct Archive {
    pub(crate) file_name: String,
    pub(crate) kind: ArchiveType,
    pub(crate) target: Target,
    pub(crate) compression_type: CompressionType,
    pub(crate) share_data: bool,
    pub(crate) archive_flags: u32,
    pub(crate) file_flags: u32,
    pub(crate) max_chunk_count: i32,
    pub(crate) single_mip_chunk_x: i32,
    pub(crate) single_mip_chunk_y: i32,
    pub(crate) archive_size: i64,
    pub(crate) archive_shared_files: i32,
    pub(crate) archive_shared_size: i64,
    pub(crate) header: Header,
    pub(crate) files: Vec<FileEntry>,
    pub(crate) folders_tes4: Vec<FolderTes4>,
    pub(crate) data_offset: i64,
    /// The mapped archive when it was read.
    pub(crate) map: Option<MappedFile>,
    /// The output when the archive is created.
    pub(crate) writer: Option<Writer>,
    /// Whether the data comes from a packer that compressed it already.
    pub(crate) packer_assigned: bool,
    /// The data written so far, by uncompressed size and hash (`fPackedData`).
    pub(crate) packed_data: HashMap<(u32, LookupHash), FileChunk>,
    /// Lookup hash of the lower-case name to the first file with it.
    pub(crate) by_hash: HashMap<LookupHash, usize>,
    /// The DX10 callback that tells the mipmap chunks a file needs (`DDSInfoProc`).
    pub(crate) dds_info_proc: Option<DdsInfoProc>,
}

/// Upstream `TwbDDSInfo`: width, height and mipmaps of a DDS file.
#[derive(Debug, Clone, Copy, Default)]
pub struct DdsInfoValues {
    pub width: i32,
    pub height: i32,
    pub mip_maps: i32,
}

/// Upstream `TwbDDSInfoProc`.
pub type DdsInfoProc = fn(&Archive, &str) -> DdsInfoValues;

/// A cursor over the bytes of an archive. Reading past the end is upstream's
/// `EReadError`.
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn seek(&mut self, position: u64) {
        self.position = usize::try_from(position).unwrap_or(usize::MAX);
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ArchiveError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ArchiveError("Stream read error".to_owned()))?;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| ArchiveError("Stream read error".to_owned()))?;
        self.position = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, ArchiveError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ArchiveError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("two bytes")))
    }

    fn u32(&mut self) -> Result<u32, ArchiveError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes")))
    }

    fn u64(&mut self) -> Result<u64, ArchiveError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("eight bytes")))
    }

    fn i64(&mut self) -> Result<i64, ArchiveError> {
        Ok(self.u64()? as i64)
    }

    /// `ReadStringLen`: a string with a byte for the length, with the
    /// terminator the length counts removed when `term`.
    fn string_len(&mut self, term: bool) -> Result<String, ArchiveError> {
        let length = usize::from(self.u8()?);
        let mut text = self.take(length)?;
        if term && length > 0 {
            text = &text[..length - 1];
        }
        Ok(ansi_string(text))
    }

    /// `ReadStringLen16`: a string with a word for the length.
    fn string_len16(&mut self) -> Result<String, ArchiveError> {
        let length = usize::from(self.u16()?);
        Ok(ansi_string(self.take(length)?))
    }

    /// `ReadStringTerm`: a string up to its terminating zero.
    fn string_term(&mut self) -> Result<String, ArchiveError> {
        let rest = self
            .bytes
            .get(self.position..)
            .ok_or_else(|| ArchiveError("Stream read error".to_owned()))?;
        let length = rest
            .iter()
            .position(|&byte| byte == 0)
            .ok_or_else(|| ArchiveError("Stream read error".to_owned()))?;
        self.position += length + 1;
        Ok(ansi_string(&rest[..length]))
    }
}

/// The text of an OS error without the Rust suffix.
pub(crate) fn os_error_text(error: &std::io::Error) -> String {
    let text = error.to_string();
    match text.rfind(" (os error ") {
        Some(at) => text[..at].to_owned(),
        None => text,
    }
}

/// The exception text of Delphi's `EFOpenError`.
pub(crate) fn cannot_open(path: &str, error: &std::io::Error) -> ArchiveError {
    // Delphi names the expanded path (`ExpandFileName`).
    let expanded = std::path::absolute(path).map_or_else(|_| path.to_owned(), |full| full.display().to_string());
    ArchiveError(format!("Cannot open file \"{expanded}\". {}", os_error_text(error)))
}

/// Port of `Magic2Int`'s inverse for messages: the four characters of a magic.
fn magic_text(magic: u32) -> String {
    let bytes = magic.to_le_bytes();
    let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(4);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

impl Default for Archive {
    fn default() -> Self {
        Self::new()
    }
}

impl Archive {
    /// Port of `TwbBSArchive.Create`.
    pub fn new() -> Self {
        Self {
            file_name: String::new(),
            kind: ArchiveType::None,
            target: Target::Pc,
            compression_type: CompressionType::None,
            share_data: false,
            archive_flags: 0,
            file_flags: 0,
            max_chunk_count: CHUNK_COUNT_MAX,
            single_mip_chunk_x: 512,
            single_mip_chunk_y: 512,
            archive_size: 0,
            archive_shared_files: 0,
            archive_shared_size: 0,
            header: Header::default(),
            files: Vec::new(),
            folders_tes4: Vec::new(),
            data_offset: 0,
            map: None,
            writer: None,
            packer_assigned: false,
            packed_data: HashMap::new(),
            by_hash: HashMap::new(),
            dds_info_proc: None,
        }
    }

    /// Opens the archive and reads its file table.
    pub fn open(path: &Path) -> Result<Self, ArchiveError> {
        let mut archive = Self::new();
        archive.load_from_file(&path.display().to_string())?;
        Ok(archive)
    }

    /// Port of `TwbBSArchive.Close`.
    pub fn close(&mut self) {
        if let Some(writer) = self.writer.take() {
            drop(writer);
            let _ = std::fs::remove_file(&self.file_name);
        }
        self.map = None;
        self.data_offset = 0;
        self.header = Header::default();
        self.files.clear();
        self.folders_tes4.clear();
        self.by_hash.clear();
        if self.share_data {
            self.packed_data.clear();
        }
        self.kind = ArchiveType::None;
        self.file_name.clear();
        self.compression_type = CompressionType::None;
        self.archive_flags = 0;
        self.file_flags = 0;
        self.archive_size = 0;
        self.archive_shared_size = 0;
        self.archive_shared_files = 0;
    }

    // Properties.

    /// Upstream `FileName`.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// The path of the archive, as `Archive::path` of the previous reader.
    pub fn path(&self) -> &str {
        &self.file_name
    }

    pub fn archive_type(&self) -> ArchiveType {
        self.kind
    }

    pub fn version(&self) -> u32 {
        self.header.version
    }

    /// Upstream `Count`.
    pub fn count(&self) -> usize {
        self.files.len()
    }

    /// The files in the order of the archive (`Items`).
    pub fn files(&self) -> &[FileEntry] {
        &self.files
    }

    pub fn compression_type(&self) -> CompressionType {
        self.compression_type
    }

    pub fn set_compression_type(&mut self, compression: CompressionType) {
        self.compression_type = compression;
    }

    pub fn archive_flags(&self) -> u32 {
        self.archive_flags
    }

    pub fn set_archive_flags(&mut self, flags: u32) {
        self.archive_flags = flags;
    }

    pub fn file_flags(&self) -> u32 {
        self.file_flags
    }

    pub fn set_file_flags(&mut self, flags: u32) {
        self.file_flags = flags;
    }

    pub fn share_data(&self) -> bool {
        self.share_data
    }

    pub fn set_share_data(&mut self, share: bool) {
        self.share_data = share;
    }

    pub fn set_target(&mut self, target: Target) {
        self.target = target;
    }

    pub fn set_max_chunk_count(&mut self, count: i32) {
        self.max_chunk_count = count;
    }

    pub fn set_single_mip_chunk(&mut self, x: i32, y: i32) {
        self.single_mip_chunk_x = x;
        self.single_mip_chunk_y = y;
    }

    pub fn set_dds_info_proc(&mut self, proc: Option<DdsInfoProc>) {
        self.dds_info_proc = proc;
    }

    pub fn archive_size(&self) -> i64 {
        self.archive_size
    }

    pub fn archive_shared_size(&self) -> i64 {
        self.archive_shared_size
    }

    pub fn archive_shared_files(&self) -> i32 {
        self.archive_shared_files
    }

    /// Port of `GetDDSMipChunkNum`: how many chunks a DX10 texture is stored in.
    pub(crate) fn dds_mip_chunk_num(&self, mut width: i32, mut height: i32, mut mip_maps: i32) -> i32 {
        let mut result = 1;
        if mip_maps == 0 {
            mip_maps += 1;
        }
        while result < mip_maps
            && result < self.max_chunk_count
            && width >= self.single_mip_chunk_x
            && height >= self.single_mip_chunk_y
        {
            result += 1;
            width /= 2;
            height /= 2;
        }
        result
    }

    // Reading.

    /// Port of `TwbBSArchive.LoadFromFile`.
    pub fn load_from_file(&mut self, file_name: &str) -> Result<(), ArchiveError> {
        self.close();
        self.file_name = file_name.to_owned();
        let map = MappedFile::open(Path::new(file_name)).map_err(|error| cannot_open(file_name, &error))?;
        let result = self.read_tables(&map);
        self.map = Some(map);
        if let Err(error) = result {
            self.map = None;
            return Err(error);
        }
        for index in 0..self.files.len() {
            self.files[index].lookup_hash = lookup_hash_text(&self.files[index].name, true);
            self.by_hash.entry(self.files[index].lookup_hash).or_insert(index);
        }
        Ok(())
    }

    /// The file tables of the archive, read as `LoadFromFile` does.
    fn read_tables(&mut self, bytes: &[u8]) -> Result<(), ArchiveError> {
        let mut stream = Reader::new(bytes);

        // Magic.
        self.header.magic = stream.u32()?;
        self.kind = match self.header.magic {
            MAGIC_TES3 => ArchiveType::Tes3,
            MAGIC_BSA => ArchiveType::Tes4,
            MAGIC_BTDX => ArchiveType::Fo4,
            _ => return Err(ArchiveError("Unknown archive format".to_owned())),
        };

        // Archive version except Morrowind.
        if self.kind != ArchiveType::Tes3 {
            self.header.version = stream.u32()?;
            self.kind = match self.header.version {
                HEADER_VERSION_TES4 => ArchiveType::Tes4,
                HEADER_VERSION_FO3 => ArchiveType::Fo3,
                HEADER_VERSION_SSE => ArchiveType::Sse,
                HEADER_VERSION_FO4_V1 | HEADER_VERSION_FO4_V7 | HEADER_VERSION_FO4_V8 => ArchiveType::Fo4,
                HEADER_VERSION_SF_V2 | HEADER_VERSION_SF_V3 => ArchiveType::Sf,
                version => return Err(ArchiveError(format!("Unknown archive version 0x{version:08X}"))),
            };
        }

        match self.kind {
            // Load Morrowind.
            ArchiveType::Tes3 => {
                self.header.hash_offset = stream.u32()?;
                self.header.file_count = stream.u32()?;
                self.files = vec![FileEntry::default(); self.header.file_count as usize];
                // Read file records.
                for file in &mut self.files {
                    file.chunk.size = stream.u32()?;
                    file.chunk.offset = i64::from(stream.u32()?);
                }
                // Skip name offsets.
                stream.seek((stream.position as u64).saturating_add(4 * u64::from(self.header.file_count)));
                // Read names.
                for file in &mut self.files {
                    file.name = stream.string_term()?;
                }
                // Read hashes.
                for file in &mut self.files {
                    file.name_hash64 = stream.u64()?;
                }
                // Remember the binary data offset since stored file offsets are relative.
                self.data_offset = stream.position as i64;
            }

            // Load Oblivion, Fallout 3, New Vegas, Skyrim, Skyrim SE.
            ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse => {
                let header = &mut self.header;
                header.folders_offset = stream.u32()?;
                header.flags = stream.u32()?;
                header.folder_count = stream.u32()?;
                header.file_count = stream.u32()?;
                header.folder_names_length = stream.u32()?;
                header.file_names_length = stream.u32()?;
                header.file_flags = stream.u32()?;
                self.archive_flags = header.flags;
                self.file_flags = header.file_flags;

                // Read folder records.
                stream.seek(u64::from(header.folders_offset));
                let sse = self.kind == ArchiveType::Sse;
                let mut folders = Vec::new();
                let mut counts = Vec::new();
                for _ in 0..header.folder_count {
                    let hash = stream.u64()?;
                    let count = stream.u32()? as usize;
                    if sse {
                        stream.u32()?; // padding
                    }
                    let offset = i64::from(stream.u32()?);
                    if sse {
                        stream.u32()?; // padding
                    }
                    folders.push(FolderTes4 {
                        hash,
                        offset,
                        name: String::new(),
                        files: Vec::new(),
                    });
                    counts.push(count);
                }

                // Read folder names and file records.
                let compress_all = header.flags & ARCHIVE_COMPRESS != 0;
                self.files = vec![FileEntry::default(); header.file_count as usize];
                let mut index = 0usize;
                for (folder, count) in folders.iter_mut().zip(counts) {
                    folder.name = stream.string_len(true)?;
                    for _ in 0..count {
                        let file = self
                            .files
                            .get_mut(index)
                            .ok_or_else(|| ArchiveError("Stream read error".to_owned()))?;
                        folder.files.push(index);
                        file.name = format!("{}\\", folder.name);
                        file.dir_hash64 = folder.hash;
                        file.name_hash64 = stream.u64()?;
                        let size = stream.u32()?;
                        file.chunk.offset = i64::from(stream.u32()?);
                        // Compressed when either of the flags is present.
                        if compress_all != (size & FILE_SIZE_COMPRESS != 0) {
                            file.chunk.packed_size = size;
                        }
                        file.chunk.size = size & !FILE_SIZE_COMPRESS;
                        file.chunk.packed_size &= !FILE_SIZE_COMPRESS;
                        index += 1;
                    }
                }
                self.folders_tes4 = folders;

                // Read file names.
                for file in &mut self.files {
                    file.name.push_str(&stream.string_term()?);
                }
            }

            // Load Fallout 4, Starfield.
            ArchiveType::Fo4 | ArchiveType::Sf => {
                self.compression_type = CompressionType::ZLib;
                self.header.magic2 = stream.u32()?;
                if self.header.magic2 != MAGIC_GNRL && self.header.magic2 != MAGIC_DX10 {
                    return Err(ArchiveError(format!(
                        "Unsupported BA2 archive type: {}",
                        magic_text(self.header.magic2)
                    )));
                }
                if self.header.magic2 == MAGIC_DX10 {
                    self.kind = match self.kind {
                        ArchiveType::Fo4 => ArchiveType::Fo4Dds,
                        _ => ArchiveType::SfDds,
                    };
                }
                self.header.file_count = stream.u32()?;
                self.header.file_table_offset = stream.i64()?;
                // The Starfield header.
                if matches!(self.kind, ArchiveType::Sf | ArchiveType::SfDds) {
                    if self.header.version >= HEADER_VERSION_SF_V2 {
                        stream.u64()?; // always set to 1, immediately discarded on load
                    }
                    if self.header.version >= HEADER_VERSION_SF_V3 {
                        self.header.compression_method = stream.u32()?;
                        if self.header.compression_method == COMPRESSION_METHOD_LZ4 {
                            self.compression_type = CompressionType::LZ4;
                        }
                    }
                }

                // Read files.
                self.files = Vec::with_capacity((self.header.file_count as usize).min(1 << 20));
                for _ in 0..self.header.file_count {
                    let mut file = FileEntry {
                        name_hash32: stream.u32()?,
                        ..FileEntry::default()
                    };
                    file.ext = stream.u32()?.to_le_bytes();
                    file.dir_hash32 = stream.u32()?;
                    file.mod_index = stream.u8()?; // always 0
                    let chunks_count = stream.u8()?;
                    let _header_size = stream.u16()?;
                    if self.header.magic2 == MAGIC_GNRL {
                        // GNRL must always have 1 chunk.
                        if chunks_count != 1 {
                            return Err(ArchiveError(format!("Invalid chunks count {chunks_count} for: ")));
                        }
                        file.chunk.offset = stream.i64()?;
                        file.chunk.packed_size = stream.u32()?;
                        file.chunk.size = stream.u32()?;
                        stream.u32()?; // BAADF00D
                    } else {
                        file.dds.height = stream.u16()?;
                        file.dds.width = stream.u16()?;
                        file.dds.num_mips = stream.u8()?;
                        file.dds.dxgi_format = stream.u8()?;
                        file.dds.flags = stream.u8()?;
                        file.dds.tile_mode = stream.u8()?;
                        for _ in 0..chunks_count {
                            let mut chunk = TexChunk::default();
                            chunk.chunk.offset = stream.i64()?;
                            chunk.chunk.packed_size = stream.u32()?;
                            chunk.chunk.size = stream.u32()?;
                            chunk.start_mip = stream.u16()?;
                            chunk.end_mip = stream.u16()?;
                            stream.u32()?; // BAADF00D
                            file.dds.tex_chunks.push(chunk);
                        }
                    }
                    self.files.push(file);
                }

                // Read file names if present.
                if self.header.file_table_offset != 0 && (self.header.file_table_offset as u64) < bytes.len() as u64 {
                    stream.seek(self.header.file_table_offset as u64);
                    for file in &mut self.files {
                        // archive2.exe uses /
                        file.name = stream.string_len16()?.replace('/', "\\");
                    }
                } else {
                    for file in &mut self.files {
                        file.name = format!(
                            "{:08X}\\{:08X}.{}",
                            file.dir_hash32,
                            file.name_hash32,
                            magic_text(u32::from_le_bytes(file.ext))
                        );
                    }
                }
            }

            _ => unreachable!("the type comes from the magic"),
        }

        // Default compression type if not decided during load.
        if self.compression_type == CompressionType::None {
            self.compression_type = self.kind.default_compression();
        }
        Ok(())
    }

    // Lookups.

    /// Port of `FileByName`: the file whose lower-cased name has the same
    /// lookup hash as `file_name`.
    pub fn file_by_name(&self, file_name: &str) -> Option<&FileEntry> {
        self.by_hash
            .get(&lookup_hash_text(file_name, true))
            .map(|&index| &self.files[index])
    }

    /// Port of `FileExists`.
    pub fn file_exists(&self, file_name: &str) -> bool {
        self.file_by_name(file_name).is_some()
    }

    /// Port of `FilesByFolder`: the files whose name starts with the folder,
    /// compared without case, all of them for an empty folder.
    pub fn files_by_folder(&self, folder: &str) -> Vec<&FileEntry> {
        // `ExcludeTrailingPathDelimiter` takes off one separator.
        let folder = folder.strip_suffix(['\\', '/']).unwrap_or(folder);
        let folder = lower_case(folder);
        self.files
            .iter()
            .filter(|file| folder.is_empty() || lower_case(&file.name).starts_with(&folder))
            .collect()
    }

    /// Whether the archive holds the file. `path` is relative to the data
    /// folder and in any case or separator.
    pub fn contains(&self, path: &str) -> bool {
        self.file_exists(&normalize_path(path))
    }

    /// The contents of the file, decompressed, or `None` when the archive
    /// does not hold it. `path` is relative to the data folder and in any
    /// case or separator.
    pub fn read(&self, path: &str) -> Result<Option<Vec<u8>>, ArchiveError> {
        match self.file_by_name(&normalize_path(path)) {
            Some(file) => self.unpack_entry(file).map(Some),
            None => Ok(None),
        }
    }

    /// Whether the entry is stored compressed (`TwbBSFileEntry.GetCompressed`).
    pub fn is_compressed(&self, file: &FileEntry) -> bool {
        if self.kind.is_dds() {
            file.dds
                .tex_chunks
                .first()
                .is_some_and(|chunk| chunk.chunk.compressed())
        } else {
            file.chunk.compressed()
        }
    }

    fn mapped(&self) -> Result<&[u8], ArchiveError> {
        self.map
            .as_deref()
            .ok_or_else(|| ArchiveError("Archive is not loaded".to_owned()))
    }

    /// Reads `size` bytes at `offset` of the archive.
    fn slice_at(&self, offset: i64, size: usize) -> Result<&[u8], ArchiveError> {
        let bytes = self.mapped()?;
        let start = usize::try_from(offset).map_err(|_| ArchiveError("Stream read error".to_owned()))?;
        let end = start
            .checked_add(size)
            .ok_or_else(|| ArchiveError("Stream read error".to_owned()))?;
        bytes
            .get(start..end)
            .ok_or_else(|| ArchiveError("Stream read error".to_owned()))
    }

    /// Port of `DecompressBuf`.
    fn decompress_buf(&self, src: &[u8], dst: &mut [u8]) -> Result<(), ArchiveError> {
        if self.compression_type == CompressionType::None {
            return Err(ArchiveError("Undefined compression type".to_owned()));
        }
        self.compression_type.decompress(src, dst).map_err(ArchiveError::from)
    }

    /// Port of `TwbBSArchive.Unpack(aFile)`: the contents of a file.
    pub fn unpack_entry(&self, file: &FileEntry) -> Result<Vec<u8>, ArchiveError> {
        match self.kind {
            ArchiveType::Tes3 => Ok(self
                .slice_at(self.data_offset + file.chunk.offset, file.chunk.size as usize)?
                .to_vec()),

            ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse => {
                let bytes = self.mapped()?;
                let mut stream = Reader::new(bytes);
                stream.seek(u64::try_from(file.chunk.offset).unwrap_or(u64::MAX));
                let mut size = i64::from(file.chunk.size);

                // Skip the embedded file name and its length prefix.
                if matches!(self.kind, ArchiveType::Fo3 | ArchiveType::Sse)
                    && self.header.flags & ARCHIVE_EMBEDNAME != 0
                {
                    // Upstream counts the characters of the name, which are its
                    // bytes in a single byte code page.
                    let length = usize::from(stream.u8()?);
                    stream.take(length)?;
                    size -= length as i64 + 1;
                }

                if self.is_compressed(file) {
                    // Allocate the uncompressed size space.
                    let unpacked = stream.u32()? as usize;
                    size -= 4;
                    let mut result = vec![0u8; unpacked];
                    if unpacked > 0 && size > 0 {
                        let packed = stream.take(size as usize)?;
                        self.decompress_buf(packed, &mut result)?;
                    }
                    Ok(result)
                } else {
                    Ok(stream.take(usize::try_from(size).unwrap_or(0))?.to_vec())
                }
            }

            ArchiveType::Fo4 | ArchiveType::Sf => {
                if self.is_compressed(file) {
                    let packed = self.slice_at(file.chunk.offset, file.chunk.packed_size as usize)?;
                    let mut result = vec![0u8; file.chunk.size as usize];
                    self.decompress_buf(packed, &mut result)?;
                    Ok(result)
                } else {
                    Ok(self.slice_at(file.chunk.offset, file.chunk.size as usize)?.to_vec())
                }
            }

            ArchiveType::Fo4Dds | ArchiveType::SfDds => texture::unpack_dds(self, file),

            ArchiveType::None => Err(ArchiveError("Archive is not loaded".to_owned())),
        }
    }

    /// Port of `TwbBSArchive.Unpack(aFileName)`.
    pub fn unpack(&self, file_name: &str) -> Result<Vec<u8>, ArchiveError> {
        let file = self
            .file_by_name(file_name)
            .ok_or_else(|| ArchiveError(format!("File not found in archive: {file_name}")))?;
        self.unpack_entry(file)
    }

    // Text.

    /// Port of `TwbBSArchive.Info`: the header facts of the archive, one
    /// per line, lines separated by CR LF.
    pub fn info(&self) -> String {
        let mut result = String::new();
        result.push_str(&format!("{:>14}: {}", "Archive Name", self.file_name));
        result.push_str(&format!("\r\n{:>14}: {}", "Format", self.kind.format_name()));
        if self.kind != ArchiveType::Tes3 {
            result.push_str(&format!("\r\n{:>14}: 0x{:02X}", "Version", self.header.version));
        }
        result.push_str(&format!("\r\n{:>14}: {}", "Files", self.count()));
        let compressed = self.files.iter().filter(|file| self.is_compressed(file)).count();
        result.push_str(&format!(
            "\r\n{:>14}: {} ({})",
            "Compressed",
            compressed,
            self.compression_type.name()
        ));
        if matches!(self.kind, ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse) {
            result.push_str(&format!(
                "\r\n{:>14}: 0x{:04X}{:>24}: 0x{:04X}",
                "Archive Flags", self.archive_flags, "File Flags", self.file_flags
            ));
            for (i, flag_name) in ARCHIVE_FLAG_NAMES.iter().enumerate() {
                let mut left = if (self.archive_flags >> i) & 1 == 1 { "*" } else { " " }.to_owned();
                left.push_str(flag_name);
                let left = format!("{:16}{left}", " ");
                let mut right = String::new();
                if i < FILE_FLAG_NAMES.len() {
                    right.push(if (self.file_flags >> i) & 1 == 1 { '*' } else { ' ' });
                    right.push_str(FILE_FLAG_NAMES[i]);
                }
                let padding = 48usize.saturating_sub(left.chars().count());
                result.push_str(&format!("\r\n{left}{}{right}", " ".repeat(padding)));
            }
        }
        result
    }

    /// Port of `TwbBSArchive.Warnings`: facts about the archive that make
    /// the game fail.
    pub fn warnings(&self) -> Vec<String> {
        let mut result = Vec::new();
        if self.kind == ArchiveType::Tes3
            && self.files.iter().any(|file| {
                matches!(
                    asset_type_by_folder(&file.name),
                    AssetType::Sound | AssetType::Music | AssetType::Video | AssetType::Font | AssetType::Splash
                )
            })
        {
            result.push(
                "Sound, Music, Video, Fonts and Splash folders don't work when packed in Morrowind archives".to_owned(),
            );
        }

        if matches!(
            self.kind,
            ArchiveType::Tes3 | ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse
        ) {
            let bad = self
                .files
                .iter()
                .filter(|file| file.chunk.offset > BSA_MAX_OFFSET)
                .count();
            if bad != 0 {
                result.push(format!(
                    "{bad} file(s) start above 2 GB max allowed BSA size, they won't work or crash the game"
                ));
            }
        }

        // Embedded names are used for textures, are not used for loose textures.
        // The engine grabs file entries for textures in the texture list on form
        // load (MODT, TXST), which speeds up their loading. A texture is loaded
        // as a cubemap if it contains "_e.dd" in the embedded name. If a texture
        // is not loose and the embedded names are missing then cubemaps are not
        // loaded properly. Seems to crash on anything but textures.
        let ends_with = |file: &FileEntry, suffix: &str| lower_case(&file.name).ends_with(suffix);
        if matches!(self.kind, ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse)
            && self.header.flags & ARCHIVE_EMBEDNAME == 0
            && self.files.iter().any(|file| ends_with(file, "_e.dds"))
        {
            result.push(
                "Contains cubemap _e.dds textures which aren't loaded properly in the game without Embedded File Names flag"
                    .to_owned(),
            );
        }

        if matches!(self.kind, ArchiveType::Fo3 | ArchiveType::Sse)
            && self.header.flags & ARCHIVE_EMBEDNAME != 0
            && self.files.iter().any(|file| !ends_with(file, ".dds"))
        {
            result.push(
                "Contains non texture files which may crash the game when packed with Embedded File Names flag"
                    .to_owned(),
            );
        }

        if (self.kind.is_dds() || self.kind == ArchiveType::Sse)
            && self.files.iter().any(|file| !self.is_compressed(file))
        {
            if self.kind == ArchiveType::Sse && self.header.flags & ARCHIVE_EMBEDNAME != 0 {
                result.push(
                    "Contains uncompressed files with Embedded File Names flag, such combination crashes Skyrim SE/AE"
                        .to_owned(),
                );
            } else if self.kind.is_dds() {
                result.push("DDS archive contains uncompressed textures which crash the game".to_owned());
            }
        }
        result
    }

    /// Port of `TwbBSFileEntry.Info`: the facts of one file, for a dump.
    pub fn file_info(&self, file: &FileEntry) -> String {
        match self.kind {
            ArchiveType::Tes3 => format!(
                "  Hash: {:016X}  Size: {}  Offset: {}",
                file.name_hash64, file.chunk.size, file.chunk.offset
            ),
            ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse => format!(
                "  DirHash: {:016X}  NameHash: {:016X}  {}Size: {}  Offset: {}",
                file.dir_hash64,
                file.name_hash64,
                if self.is_compressed(file) { "Packed" } else { "" },
                file.chunk.size,
                file.chunk.offset
            ),
            ArchiveType::Fo4 | ArchiveType::Fo4Dds | ArchiveType::Sf | ArchiveType::SfDds => {
                let ext_end = file.ext.iter().position(|&byte| byte == 0).unwrap_or(4);
                let mut result = format!(
                    "  DirHash: {:08X}  NameHash: {:08X}  Ext: {}",
                    file.dir_hash32,
                    file.name_hash32,
                    ansi_string(&file.ext[..ext_end])
                );
                if matches!(self.kind, ArchiveType::Fo4 | ArchiveType::Sf) {
                    result.push_str(&format!(
                        "  Size: {}  PackedSize: {}  Offset: {}",
                        file.chunk.size, file.chunk.packed_size, file.chunk.offset
                    ));
                } else {
                    result.push_str(&format!(
                        "\r\n  Width: {:4}  Height: {:4}  CubeMap: {}  Format: {}",
                        file.dds.width,
                        file.dds.height,
                        if file.is_cube_map() { "Yes" } else { "No" },
                        file.dxgi_format_name()
                    ));
                    for chunk in &file.dds.tex_chunks {
                        result.push_str(&format!(
                            "\r\n    MipMaps {:02}-{:02}  Size: {:8}  PackedSize: {:8}  Offset: {}",
                            chunk.start_mip,
                            chunk.end_mip,
                            chunk.chunk.size,
                            chunk.chunk.packed_size,
                            chunk.chunk.offset
                        ));
                    }
                }
                result
            }
            ArchiveType::None => String::new(),
        }
    }
}

impl Drop for Archive {
    fn drop(&mut self) {
        // An archive that is still being created is deleted, as `Close` does.
        self.close();
    }
}

/// The path as the archive table keys it: lower case with backslashes.
pub fn normalize_path(path: &str) -> String {
    lower_case(path.replace('/', "\\").trim_start_matches('\\'))
}

/// Port of `FormatSize`: a size as `1.5 MB`, with the units Bytes to YB.
pub fn format_size(bytes: i64) -> String {
    const DESCRIPTION: [&str; 9] = ["Bytes", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let value = bytes as f64;
    let mut i = 0usize;
    while i + 1 < DESCRIPTION.len() && value >= 1024f64.powi(i as i32 + 1) {
        i += 1;
    }
    format!(
        "{} {}",
        format_float_hash_2(value / 1024f64.powi(i as i32)),
        DESCRIPTION[i]
    )
}

/// Delphi `FormatFloat('###0.##', value)`: at most two decimals, rounded
/// half away from zero on the 15 significant digits of the value, trailing
/// zeros removed.
pub fn format_float_hash_2(value: f64) -> String {
    let negative = value < 0.0;
    let text = format!("{:.14e}", value.abs());
    let (mantissa, exponent) = text.split_once('e').expect("exponent");
    let exponent: i32 = exponent.parse().expect("exponent number");
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|d| d - b'0').collect();
    // The value is 0.d1d2d3... * 10^(exponent + 1).
    let point = exponent + 1;
    let mut integer: Vec<u8> = Vec::new();
    let mut fraction: Vec<u8> = Vec::new();
    for (i, &digit) in digits.iter().enumerate() {
        let position = i as i32 - point;
        if position < 0 {
            integer.push(digit);
        } else {
            while (fraction.len() as i32) < position {
                fraction.push(0);
            }
            fraction.push(digit);
        }
    }
    if point <= 0 {
        // Leading zeros of the fraction.
        let mut padded = vec![0u8; (-point) as usize];
        padded.extend(&digits);
        fraction = padded;
        integer.clear();
    } else if (digits.len() as i32) < point {
        integer.extend(std::iter::repeat_n(0, (point - digits.len() as i32) as usize));
    }
    // Round to two decimals.
    let round_up = fraction.get(2).is_some_and(|&digit| digit >= 5);
    fraction.truncate(2);
    while fraction.len() < 2 {
        fraction.push(0);
    }
    let mut number: Vec<u8> = integer.iter().chain(fraction.iter()).copied().collect();
    if round_up {
        let mut at = number.len();
        loop {
            if at == 0 {
                number.insert(0, 1);
                break;
            }
            at -= 1;
            if number[at] == 9 {
                number[at] = 0;
            } else {
                number[at] += 1;
                break;
            }
        }
    }
    let split = number.len() - 2;
    let mut int_text: String = number[..split].iter().map(|d| char::from(b'0' + d)).collect();
    if int_text.is_empty() {
        int_text.push('0');
    }
    let int_text = int_text.trim_start_matches('0');
    let int_text = if int_text.is_empty() { "0" } else { int_text };
    let fraction_text: String = number[split..].iter().map(|d| char::from(b'0' + d)).collect();
    let fraction_text = fraction_text.trim_end_matches('0');
    let sign = if negative && (int_text != "0" || !fraction_text.is_empty()) {
        "-"
    } else {
        ""
    };
    if fraction_text.is_empty() {
        format!("{sign}{int_text}")
    } else {
        format!("{sign}{int_text}.{fraction_text}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of the game, or `None` when the game is not installed here.
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

    /// The hash fields of the file tables, which the names must give again.
    #[test]
    fn name_hashes_equal_the_tables_of_the_game_archives() {
        use crate::hash;
        for (env, name) in [
            ("XEDIT_TES3_DATA", "Tribunal.bsa"),
            ("XEDIT_TES4_DATA", "DLCHorseArmor.bsa"),
            ("XEDIT_FNV_DATA", "MercenaryPack - Main.bsa"),
            ("XEDIT_SSE_DATA", "ccQDRSSE001-SurvivalMode.bsa"),
            ("XEDIT_FO4_DATA", "Fallout4 - Materials.ba2"),
            ("XEDIT_SF1_DATA", "SFBGS004 - Main.ba2"),
        ] {
            let Some(path) = data_file(env, name) else {
                continue;
            };
            let archive = Archive::open(Path::new(&path)).unwrap();
            for file in archive.files() {
                let (_, directory, file_name) = split_dir_name(&file.name);
                match archive.archive_type() {
                    ArchiveType::Tes3 => assert_eq!(hash::tes3(&file.name), file.name_hash64, "{}", file.name),
                    ArchiveType::Tes4 => {
                        assert_eq!(hash::tes4(&directory, false), file.dir_hash64, "{}", file.name);
                        assert_eq!(hash::tes4(&file_name, true), file.name_hash64, "{}", file.name);
                    }
                    ArchiveType::Fo3 | ArchiveType::Sse => {
                        assert_eq!(hash::tes5(&directory, false), file.dir_hash64, "{}", file.name);
                        assert_eq!(hash::tes5(&file_name, true), file.name_hash64, "{}", file.name);
                    }
                    _ => {
                        let (_, stem, _) = split_name_ext(&file_name, true);
                        assert_eq!(hash::fo4(&directory), file.dir_hash32, "{}", file.name);
                        assert_eq!(hash::fo4(&stem), file.name_hash32, "{}", file.name);
                    }
                }
            }
        }
    }

    #[test]
    fn archive_types_know_their_formats() {
        assert_eq!(ArchiveType::Sse.default_compression(), CompressionType::LZ4F);
        assert_eq!(ArchiveType::Sf.default_compression(), CompressionType::ZLib);
        assert_eq!(ArchiveType::SfDds.default_compression(), CompressionType::LZ4);
        assert!(ArchiveType::Sf.supports_compression(CompressionType::LZ4));
        assert!(!ArchiveType::Fo4.supports_compression(CompressionType::LZ4));
        assert_eq!(ArchiveType::Tes3.default_split_size(), BSA_MAX_OFFSET);
        assert_eq!(ArchiveType::Fo4.default_split_size(), 0);
        assert_eq!(ArchiveType::Fo4Dds.default_extension(), ".ba2");
        assert!(is_archive("x\\Mod - Main.BA2"));
        assert!(!is_archive("x\\Mod.esp"));
    }

    #[test]
    fn sizes_format_like_delphi() {
        assert_eq!(format_size(0), "0 Bytes");
        assert_eq!(format_size(761), "761 Bytes");
        assert_eq!(format_size(1024), "1 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(3_812_133), "3.64 MB");
        assert_eq!(format_size(13 * 1024 * 1024 * 1024), "13 GB");
        assert_eq!(format_float_hash_2(1.005), "1.01");
        assert_eq!(format_float_hash_2(0.004), "0");
        assert_eq!(format_float_hash_2(99.999), "100");
        assert_eq!(format_float_hash_2(0.5), "0.5");
    }

    #[test]
    fn rejects_text_that_is_not_an_archive() {
        let dir = std::env::temp_dir().join(format!("xedit-archive-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.bsa");
        std::fs::write(&path, b"NOPE1234").unwrap();
        assert_eq!(
            Archive::open(&path).err().map(|error| error.0),
            Some("Unknown archive format".to_owned())
        );
        std::fs::write(&path, b"BSA\0\x66\0\0\0").unwrap();
        assert_eq!(
            Archive::open(&path).err().map(|error| error.0),
            Some("Unknown archive version 0x00000066".to_owned())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
