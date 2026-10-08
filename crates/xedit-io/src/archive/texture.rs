// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbBSArchive.pas (the DX10 parts: TwbSplitPacker.LoadFile,
// TwbBSArchive.Pack and Unpack for DDS archives), Core/wbDDS.pas

//! Texture archives (Fallout 4 and Starfield DX10 `.ba2`): cutting a DDS file
//! into chunks, storing them, and building the DDS file again.
//!
//! A texture is stored as chunks: the header of the DDS file is not stored
//! at all (the archive's file table keeps the dimensions, the format and the
//! flags, and `SetUpHeader` makes the header again), and the mipmaps are cut
//! into at most `max_chunk_count` chunks, the big mipmaps one by one and the
//! rest together in the last one. Each chunk is compressed on its own.
//!
//! * `chunk_dds` is `TwbSplitPacker.LoadFile` for a DDS: split a DDS file
//!   into the uncompressed header chunk and the compressed mipmap chunks.
//! * `pack_dds_chunks` is `TwbBSArchive.Pack` for a DDS archive with a
//!   packer, `prepare_dds` and `store_dds` (called together by `pack_dds`)
//!   are the same without one.
//! * `unpack_dds` is the `baFO4dds`/`baSFdds` case of `TwbBSArchive.Unpack`:
//!   rebuild the DDS file of an entry from its chunks.

use super::write::{ChunkSlot, Prepared};
use super::{Archive, ArchiveError, FileEntry, Target, TexChunk};
use crate::compression::CompressionType;
use crate::dds::{self, D3dFormat, DdsHeader, Dxgi};
use crate::hash::{LookupHash, lookup_hash};

pub use crate::dds::dxgi_format_name;

/// `DDS_FLAG_CUBEMAP`.
const DDS_FLAG_CUBEMAP: u8 = 0x01;

/// `cExceptionUnsupportedDDS`.
fn unsupported_dds() -> ArchiveError {
    ArchiveError("Unsupported DDS format".to_owned())
}

/// The settings that decide how a texture is cut into chunks
/// (`fMaxChunkCount`, `fSingleMipChunkX`, `fSingleMipChunkY`, `fTarget`)
/// and compressed (`fCompressionType`), those of the packer and of the
/// archive.
#[derive(Debug, Clone, Copy)]
pub struct TextureConfig {
    pub max_chunk_count: i32,
    pub single_mip_chunk_x: i32,
    pub single_mip_chunk_y: i32,
    pub target: Target,
    pub compression: CompressionType,
}

impl TextureConfig {
    /// The settings of an archive.
    pub(super) fn of(archive: &Archive) -> TextureConfig {
        TextureConfig {
            max_chunk_count: archive.max_chunk_count,
            single_mip_chunk_x: archive.single_mip_chunk_x,
            single_mip_chunk_y: archive.single_mip_chunk_y,
            target: archive.target,
            compression: archive.compression_type,
        }
    }

    /// Port of `GetDDSMipChunkNum`: how many chunks a DX10 texture is stored in.
    pub(super) fn mip_chunk_num(&self, mut width: i32, mut height: i32, mut mip_maps: i32) -> i32 {
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

    /// The checks of a DDS file the archive is given (`cExceptionInvalidDDS`,
    /// `cExceptionIsXboxDDS`, `cExceptionIsNotXboxDDS`).
    fn check(&self, data: &[u8]) -> Result<(), ArchiveError> {
        if !dds::is_dds(data) {
            return Err(dds::invalid_dds());
        }
        if self.target == Target::Pc && dds::is_xbox(data) {
            return Err(ArchiveError("DDS is in XBox format".to_owned()));
        }
        if self.target == Target::XBox && !dds::is_xbox(data) {
            return Err(ArchiveError("DDS is not in XBox format".to_owned()));
        }
        Ok(())
    }
}

/// `Copy(aBytes, aIndex, aCount)` of a dynamic array: the part of the array
/// that exists, empty when the range is outside of it.
fn copy_range(data: &[u8], index: i64, count: i64) -> &[u8] {
    let (mut index, mut count) = (index, count);
    if index < 0 {
        count += index;
        index = 0;
    }
    let length = data.len() as i64;
    if index > length {
        return &[];
    }
    count = count.min(length - index);
    if count <= 0 {
        return &[];
    }
    &data[index as usize..(index + count) as usize]
}

/// The DDS file with the 24 bit RGB format converted (`aData` of upstream's
/// `Pack` after `ConvertR8G8B8toB8G8R8X8`), checked for a format DXGI has.
fn convert_unsupported(data: Vec<u8>) -> Result<Vec<u8>, ArchiveError> {
    let data = if dds::d3dfmt(&data) == D3dFormat::R8G8B8 {
        dds::convert_r8g8b8_to_b8g8r8x8(&data)
    } else {
        data
    };
    if dds::dxgi(&data) == Dxgi::UNKNOWN {
        return Err(unsupported_dds());
    }
    Ok(data)
}

/// Port of `TwbSplitPacker.LoadFile` for a DDS archive: the data of the
/// chunks of one DDS file in order (chunk 0 is the uncompressed DDS header,
/// the others are the mipmap chunks, compressed when `compress`), each with
/// its uncompressed size, and the size and lookup hash of the DDS data after
/// the conversion of 24 bit formats, when `share`. `buf` is the whole DDS file.
#[allow(clippy::type_complexity)]
pub(super) fn chunk_dds(
    config: &TextureConfig,
    buf: Vec<u8>,
    compress: bool,
    share: bool,
) -> Result<(Vec<(Vec<u8>, i32)>, Option<(i32, LookupHash)>), ArchiveError> {
    config.check(&buf)?;
    let buf = convert_unsupported(buf)?;
    let header = DdsHeader::read(&buf);
    let mut offset = dds::header_size(&buf) as i64;
    let mut mip_size = i64::from(dds::mip_size(&buf));

    let add_chunk = |data: &[u8], compress: bool| -> Result<(Vec<u8>, i32), ArchiveError> {
        let stored = if compress {
            if config.compression == CompressionType::None {
                return Err(ArchiveError("Undefined compression type".to_owned()));
            }
            config.compression.compress(data)?
        } else {
            data.to_vec()
        };
        Ok((stored, data.len() as i32))
    };

    let mut chunks = Vec::new();
    // Chunk 0 is the uncompressed DDS header.
    chunks.push(add_chunk(copy_range(&buf, 0, offset), false)?);
    let mut count = config.mip_chunk_num(header.width as i32, header.height as i32, header.mip_map_count as i32);
    if dds::is_cube_map(&buf) {
        // Cubemaps are not chunked.
        count = 1;
    }
    for i in 1..=count {
        if i == count {
            mip_size = buf.len() as i64 - offset;
        }
        chunks.push(add_chunk(copy_range(&buf, offset, mip_size), compress)?);
        offset += mip_size;
        mip_size /= 4;
    }
    let share_of = share.then(|| (buf.len() as i32, lookup_hash(&buf)));
    Ok((chunks, share_of))
}

/// The description of a texture that `Pack` stores in the file entry.
struct Description {
    height: u16,
    width: u16,
    num_mips: u8,
    dxgi_format: u8,
    flags: u8,
    tile_mode: u8,
    /// The number of chunks.
    chunks: usize,
}

/// The part of `TwbBSArchive.Pack` for a DDS file that fills the entry's
/// description, from the header of the DDS file.
fn describe(config: &TextureConfig, header_bytes: &[u8]) -> Result<Description, ArchiveError> {
    let header = DdsHeader::read(header_bytes);
    let format = dds::dxgi(header_bytes);
    if format == Dxgi::UNKNOWN {
        return Err(unsupported_dds());
    }
    let mut description = Description {
        height: header.height as u16,
        width: header.width as u16,
        num_mips: header.mip_map_count as u8,
        dxgi_format: format.0,
        flags: 0,
        tile_mode: dds::tile_mode(header_bytes) as u8,
        chunks: 0,
    };
    // DirectXTexDDS.cpp, in DecodeDDSHeader, if dwMipMapCount is 0, it is forced to 1.
    if description.num_mips == 0 {
        description.num_mips += 1;
    }
    let count = if dds::is_cube_map(header_bytes) {
        description.flags |= DDS_FLAG_CUBEMAP;
        // Cubemaps are not chunked.
        1
    } else {
        config.mip_chunk_num(
            i32::from(description.width),
            i32::from(description.height),
            i32::from(description.num_mips),
        )
    };
    description.chunks = count.max(0) as usize;
    Ok(description)
}

impl Description {
    /// The first and last mipmap of chunk `index`: the last chunk stores
    /// all the remaining mipmaps.
    fn mips_of(&self, index: usize) -> (u16, u16) {
        let end = if index + 1 < self.chunks {
            index as u16
        } else {
            u16::from(self.num_mips).wrapping_sub(1)
        };
        (index as u16, end)
    }

    /// Stores the description and the (empty) chunks in the entry.
    fn set(&self, entry: &mut FileEntry) {
        entry.dds.dxgi_format = self.dxgi_format;
        entry.dds.width = self.width;
        entry.dds.height = self.height;
        entry.dds.num_mips = self.num_mips;
        entry.dds.tile_mode = self.tile_mode;
        entry.dds.flags = self.flags;
        entry.dds.tex_chunks = (0..self.chunks)
            .map(|index| {
                let (start_mip, end_mip) = self.mips_of(index);
                TexChunk {
                    start_mip,
                    end_mip,
                    ..TexChunk::default()
                }
            })
            .collect();
    }
}

/// Port of the DDS branch of `TwbBSArchive.Pack` with a packer: stores the
/// chunks that `chunk_dds` made, with the lookup hash of each, through
/// `Archive::pack_chunk`, and fills in the `dds` description of the entry.
/// `chunks[0]` is the header.
pub(super) fn pack_dds_chunks(
    archive: &mut Archive,
    entry: usize,
    chunks: Vec<(Vec<u8>, i32, Option<LookupHash>)>,
) -> Result<(), ArchiveError> {
    let config = TextureConfig::of(archive);
    let header = chunks
        .first()
        .map(|(data, _, _)| data.as_slice())
        .ok_or_else(|| ArchiveError("Chunk index 0 not found".to_owned()))?;
    config.check(header)?;
    let description = describe(&config, header)?;
    description.set(&mut archive.files[entry]);
    let name = archive.files[entry].name.clone();

    let mut chunks = chunks.into_iter().skip(1);
    for index in 0..description.chunks {
        let (data, uncompressed_size, hash) = chunks
            .next()
            .ok_or_else(|| ArchiveError(format!("Chunk index {} not found for {name}", index + 1)))?;
        archive.pack_chunk(
            entry,
            ChunkSlot::Tex(index),
            Prepared {
                data: Some(data),
                uncompressed_size: uncompressed_size as usize,
                hash,
            },
        )?;
    }
    Ok(())
}

/// A texture that `prepare_dds` cut and compressed, to be stored by
/// `store_dds`.
pub(super) struct DdsPrepared {
    description: Description,
    chunks: Vec<Prepared>,
}

/// The first half of the DDS branch of `TwbBSArchive.Pack` without a packer:
/// checks the DDS file, converts it when its format is 24 bit RGB, describes
/// it and cuts and compresses its chunks (`PackData` without the writing).
/// `shared` tells whether the data of a chunk (its index, uncompressed size
/// and hash) is in the archive already, so that it need not be compressed;
/// the hashes are computed when `share`.
pub(super) fn prepare_dds(
    config: &TextureConfig,
    data: Vec<u8>,
    compress: bool,
    share: bool,
    shared: &dyn Fn(usize, u32, LookupHash) -> bool,
) -> Result<DdsPrepared, ArchiveError> {
    config.check(&data)?;
    // Convert unsupported uncompressed 24 bit RGB to 32 bit BGRX.
    let data = convert_unsupported(data)?;
    let description = describe(config, &data)?;

    let mut offset = dds::header_size(&data) as i64;
    let mut mip_size = (u32::from(description.width)
        .wrapping_mul(u32::from(description.height))
        .wrapping_mul(u32::from(dds::bits_per_pixel(Dxgi(description.dxgi_format))))
        >> 3) as i32 as i64;
    let mut chunks = Vec::with_capacity(description.chunks);
    for index in 0..description.chunks {
        if index + 1 == description.chunks {
            // The last chunk stores all the remaining mipmaps.
            mip_size = data.len() as i64 - offset;
        }
        let part = copy_range(&data, offset, mip_size);
        let hash = share.then(|| lookup_hash(part));
        let size = part.len();
        let is_shared = hash.is_some_and(|hash| shared(index, size as u32, hash));
        let stored = if is_shared {
            None
        } else if compress {
            if config.compression == CompressionType::None {
                return Err(ArchiveError("Undefined compression type".to_owned()));
            }
            Some(config.compression.compress(part)?)
        } else {
            Some(part.to_vec())
        };
        chunks.push(Prepared {
            data: stored,
            uncompressed_size: size,
            hash,
        });
        offset += mip_size;
        mip_size /= 4;
    }
    Ok(DdsPrepared { description, chunks })
}

impl DdsPrepared {
    /// The bytes of the data to store.
    pub(super) fn size(&self) -> usize {
        self.chunks
            .iter()
            .map(|chunk| chunk.data.as_ref().map_or(0, Vec::len))
            .sum()
    }
}

/// The second half of the DDS branch of `TwbBSArchive.Pack` without a
/// packer: fills in the description of the entry and stores the chunks.
pub(super) fn store_dds(archive: &mut Archive, entry: usize, prepared: DdsPrepared) -> Result<(), ArchiveError> {
    prepared.description.set(&mut archive.files[entry]);
    for (index, chunk) in prepared.chunks.into_iter().enumerate() {
        archive.pack_chunk(entry, ChunkSlot::Tex(index), chunk)?;
    }
    Ok(())
}

/// Port of the DDS branch of `TwbBSArchive.Pack` without a packer: fills
/// the `dds` description and the chunk list of `entry` from the DDS file
/// `data` and stores the chunks through `Archive::pack_chunk`.
pub(super) fn pack_dds(archive: &mut Archive, entry: usize, data: &[u8]) -> Result<(), ArchiveError> {
    let config = TextureConfig::of(archive);
    let compress = archive.files[entry].compress;
    let share = archive.share_data;
    let prepared = {
        let packed = &archive.packed_data;
        prepare_dds(&config, data.to_vec(), compress, share, &|_, size, hash| {
            packed.contains_key(&(size, hash))
        })?
    };
    store_dds(archive, entry, prepared)
}

/// Port of the `baFO4dds`/`baSFdds` branch of `TwbBSArchive.Unpack`: the
/// DDS file of an entry, with the header made by `SetUpHeader` and the
/// mipmap chunks appended.
pub(super) fn unpack_dds(archive: &Archive, entry: &FileEntry) -> Result<Vec<u8>, ArchiveError> {
    let xbox = archive.file_name.to_lowercase().contains("_xbox.");
    let format = Dxgi(entry.dds.dxgi_format);
    // Allocate space for the total DDS size.
    let mut size = dds::HEADER_SIZE;
    if xbox || !dds::DXGI_DX9.contains(&format) {
        size += dds::HEADER_DX10_SIZE;
        if xbox {
            size += dds::HEADER_XBOX_SIZE;
        }
    }
    // The offset to the image data is the total size of the DDS header.
    let mut mip_offset = size;
    for chunk in &entry.dds.tex_chunks {
        size = size.wrapping_add(chunk.chunk.size as usize);
    }
    let mut result = vec![0u8; size];

    // Set up the DDS header.
    dds::set_up_header(
        &mut result,
        format,
        i32::from(entry.dds.width),
        i32::from(entry.dds.height),
        i32::from(entry.dds.num_mips),
        entry.is_cube_map(),
        xbox,
    );
    if xbox {
        let at = dds::HEADER_SIZE + dds::HEADER_DX10_SIZE;
        result[at..at + 4].copy_from_slice(&u32::from(entry.dds.tile_mode).to_le_bytes());
    }

    // Append the mipmap chunks.
    for chunk in &entry.dds.tex_chunks {
        let chunk = &chunk.chunk;
        let end = mip_offset + chunk.size as usize;
        if chunk.compressed() {
            let packed = archive.slice_at(chunk.offset, chunk.packed_size as usize)?;
            archive.decompress_buf(packed, &mut result[mip_offset..end])?;
        } else {
            result[mip_offset..end].copy_from_slice(archive.slice_at(chunk.offset, chunk.size as usize)?);
        }
        mip_offset = end;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::ArchiveType;
    use crate::archive::write::PackFile;
    use crate::dds::{DdsHeader, HEADER_DX10_SIZE, HEADER_SIZE, MAX_HEADER_SIZE};

    #[test]
    fn format_names_follow_the_dxgi_numbers() {
        assert_eq!(dxgi_format_name(0), "UNKNOWN");
        assert_eq!(dxgi_format_name(71), "BC1_UNORM");
        assert_eq!(dxgi_format_name(77), "BC3_UNORM");
        assert_eq!(dxgi_format_name(80), "BC4_UNORM");
        assert_eq!(dxgi_format_name(83), "BC5_UNORM");
        assert_eq!(dxgi_format_name(98), "BC7_UNORM");
        assert_eq!(dxgi_format_name(87), "B8G8R8A8_UNORM");
        assert_eq!(dxgi_format_name(115), "B4G4R4A4_UNORM");
        assert_eq!(dxgi_format_name(200), "");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xedit-texture-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A DDS file with generated mipmaps, the whole chain.
    fn texture(format: Dxgi, size: i32, mips: i32, seed: u32) -> Vec<u8> {
        let mut file = vec![0u8; MAX_HEADER_SIZE];
        dds::set_up_header(&mut file, format, size, size, mips, false, false);
        file.truncate(dds::header_size(&file));
        let bits = usize::from(dds::bits_per_pixel(format));
        let mut state = seed;
        let mut level = size as usize;
        for _ in 0..mips {
            for index in 0..(level * level * bits / 8).max(8) {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                file.push(if index % 4 == 0 {
                    (state >> 24) as u8
                } else {
                    (index / 32) as u8
                });
            }
            level = (level / 2).max(1);
        }
        file
    }

    fn list(files: &[(&str, bool)]) -> Vec<PackFile> {
        files
            .iter()
            .enumerate()
            .map(|(index, (name, compress))| PackFile {
                name: (*name).to_owned(),
                file_object: index,
                compress: *compress,
            })
            .collect()
    }

    /// Packs the textures into an archive without a packer and reads them back.
    fn round_trip(kind: ArchiveType, compress: bool, share: bool) {
        let dir = temp_dir(&format!("{kind:?}-{compress}-{share}"));
        let path = dir.join("t.ba2").display().to_string();
        let files = [
            ("textures\\a\\big.dds", texture(Dxgi::BC1_UNORM, 1024, 11, 1)),
            ("textures\\a\\seven.dds", texture(Dxgi::BC7_UNORM, 512, 10, 2)),
            ("textures\\a\\small.dds", texture(Dxgi::BC3_UNORM, 32, 6, 3)),
            ("textures\\a\\same.dds", texture(Dxgi::BC3_UNORM, 32, 6, 3)),
            ("textures\\a\\raw.dds", texture(Dxgi::R8G8B8A8_UNORM, 64, 1, 4)),
            ("textures\\a\\no_mips.dds", texture(Dxgi::BC1_UNORM, 256, 1, 5)),
        ];
        let names: Vec<(&str, bool)> = files.iter().map(|(name, _)| (*name, compress)).collect();
        let mut archive = Archive::new();
        archive.set_share_data(share);
        archive.create_archive(&path, kind, &list(&names)).unwrap();
        for (name, data) in &files {
            archive.pack(name, data).unwrap();
        }
        archive.save().unwrap();
        assert_eq!(archive.archive_shared_files(), i32::from(share), "{kind:?}");
        drop(archive);

        let read = Archive::open(std::path::Path::new(&path)).unwrap();
        assert_eq!(read.archive_type(), kind);
        for (name, data) in &files {
            assert!(&read.unpack(name).unwrap() == data, "{kind:?} {name}");
        }
        // The chunks: a 1024 texture with 11 mipmaps is cut in three (the mipmaps from
        // 256 on go together), one with
        // a single mipmap is whole.
        let big = read.file_by_name("textures\\a\\big.dds").unwrap();
        assert_eq!(
            big.dds
                .tex_chunks
                .iter()
                .map(|chunk| (chunk.start_mip, chunk.end_mip))
                .collect::<Vec<_>>(),
            [(0, 0), (1, 1), (2, 10)]
        );
        assert_eq!(
            read.file_by_name("textures\\a\\no_mips.dds")
                .unwrap()
                .dds
                .tex_chunks
                .len(),
            1
        );
        drop(read);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn textures_read_back_what_was_packed() {
        for kind in [ArchiveType::Fo4Dds, ArchiveType::SfDds] {
            for compress in [false, true] {
                for share in [false, true] {
                    round_trip(kind, compress, share);
                }
            }
        }
    }

    /// A packer hands the archive the chunks `chunk_dds` made; the archive
    /// written is the one `pack` writes from the files.
    #[test]
    fn the_packer_route_stores_what_the_direct_route_does() {
        let dir = temp_dir("packer");
        let files = [
            ("textures\\a\\big.dds", texture(Dxgi::BC1_UNORM, 1024, 11, 1)),
            ("textures\\a\\small.dds", texture(Dxgi::BC3_UNORM, 32, 6, 3)),
            ("textures\\a\\same.dds", texture(Dxgi::BC3_UNORM, 32, 6, 3)),
            ("textures\\a\\cube.dds", {
                let mut data = vec![0u8; MAX_HEADER_SIZE];
                dds::set_up_header(&mut data, Dxgi::BC1_UNORM, 64, 64, 7, true, false);
                data.truncate(HEADER_SIZE);
                data.extend((0..6 * 2800).map(|i| (i / 7) as u8));
                data
            }),
        ];
        let names: Vec<(&str, bool)> = files.iter().map(|(name, _)| (*name, true)).collect();
        let write = |packer: bool| -> Vec<u8> {
            let path = dir.join(format!("{packer}.ba2")).display().to_string();
            let mut archive = Archive::new();
            archive.set_share_data(true);
            archive
                .create_archive(&path, ArchiveType::Fo4Dds, &list(&names))
                .unwrap();
            if packer {
                archive.set_packer_assigned(true);
                let config = TextureConfig::of(&archive);
                for (name, data) in &files {
                    let (chunks, share) = chunk_dds(&config, data.clone(), true, true).unwrap();
                    assert_eq!(share.unwrap().0 as usize, data.len());
                    let entry = archive.by_hash[&crate::hash::lookup_hash_text(name, true)];
                    let chunks = chunks
                        .into_iter()
                        .map(|(chunk, size)| {
                            let hash = lookup_hash(&chunk);
                            (chunk, size, Some(hash))
                        })
                        .collect();
                    pack_dds_chunks(&mut archive, entry, chunks).unwrap();
                }
            } else {
                for (name, data) in &files {
                    archive.pack(name, data).unwrap();
                }
            }
            archive.save().unwrap();
            drop(archive);
            let bytes = std::fs::read(&path).unwrap();
            // A cube map is one chunk.
            let read = Archive::open(std::path::Path::new(&path)).unwrap();
            let cube = read.file_by_name("textures\\a\\cube.dds").unwrap();
            assert!(cube.is_cube_map());
            assert_eq!(cube.dds.tex_chunks.len(), 1);
            for (name, data) in &files {
                assert!(&read.unpack(name).unwrap() == data, "{packer} {name}");
            }
            bytes
        };
        // The identical texture is found by its chunks in both routes.
        assert!(write(true) == write(false));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_24_bit_texture_is_stored_as_32_bit() {
        let dir = temp_dir("rgb24");
        let path = dir.join("t.ba2").display().to_string();
        let mut data = vec![0u8; HEADER_SIZE];
        let mut header = DdsHeader::read(&data);
        header.magic = dds::MAGIC_DDS;
        header.width = 4;
        header.height = 4;
        header.mip_map_count = 1;
        header.pixel_format.flags = dds::DDPF_RGB;
        header.pixel_format.rgb_bit_count = 24;
        header.pixel_format.r_bit_mask = 0xFF0000;
        header.pixel_format.g_bit_mask = 0xFF00;
        header.pixel_format.b_bit_mask = 0xFF;
        header.write(&mut data);
        data.extend((0..48).map(|i| i as u8));
        let mut archive = Archive::new();
        archive
            .create_archive(&path, ArchiveType::Fo4Dds, &list(&[("textures\\x\\rgb.dds", true)]))
            .unwrap();
        archive.pack("textures\\x\\rgb.dds", &data).unwrap();
        archive.save().unwrap();
        let read = Archive::open(std::path::Path::new(&path)).unwrap();
        let entry = read.file_by_name("textures\\x\\rgb.dds").unwrap();
        assert_eq!(entry.dxgi_format_name(), "B8G8R8X8_UNORM");
        let unpacked = read.unpack("textures\\x\\rgb.dds").unwrap();
        assert_eq!(unpacked.len(), HEADER_SIZE + 64);
        assert_eq!(&unpacked[HEADER_SIZE..HEADER_SIZE + 8], &[0, 1, 2, 0xFF, 3, 4, 5, 0xFF]);
        drop(read);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn files_that_are_not_stored_say_why() {
        let dir = temp_dir("errors");
        let path = dir.join("t.ba2").display().to_string();
        let mut archive = Archive::new();
        archive
            .create_archive(&path, ArchiveType::Fo4Dds, &list(&[("textures\\x\\a.dds", false)]))
            .unwrap();
        let error = |archive: &mut Archive, data: &[u8]| archive.pack("textures\\x\\a.dds", data).unwrap_err().0;
        assert_eq!(error(&mut archive, b"not a texture"), "Not a valid DDS file");
        // A texture of the Xbox needs an Xbox archive.
        let xbox = {
            let mut data = vec![0u8; MAX_HEADER_SIZE];
            dds::set_up_header(&mut data, Dxgi::BC1_UNORM, 8, 8, 1, false, true);
            data
        };
        assert_eq!(error(&mut archive, &xbox), "DDS is in XBox format");
        // A DX10 header of format 0.
        let unknown = {
            let mut data = vec![0u8; MAX_HEADER_SIZE];
            dds::set_up_header(&mut data, Dxgi::UNKNOWN, 8, 8, 1, false, false);
            data.truncate(HEADER_SIZE + HEADER_DX10_SIZE);
            data
        };
        assert_eq!(error(&mut archive, &unknown), "Unsupported DDS format");
        archive.set_target(Target::XBox);
        assert_eq!(
            error(&mut archive, &texture(Dxgi::BC1_UNORM, 8, 1, 1)),
            "DDS is not in XBox format"
        );
        drop(archive);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file whose mipmaps are cut short keeps the part there is.
    #[test]
    fn a_truncated_texture_is_cut_where_it_ends() {
        let config = TextureConfig {
            max_chunk_count: 4,
            single_mip_chunk_x: 512,
            single_mip_chunk_y: 512,
            target: Target::Pc,
            compression: CompressionType::ZLib,
        };
        let mut data = texture(Dxgi::BC1_UNORM, 1024, 11, 1);
        let first = 1024 * 1024 / 2;
        data.truncate(HEADER_SIZE + first + 100);
        let (chunks, _) = chunk_dds(&config, data, false, false).unwrap();
        // The header, the whole first mipmap, 100 bytes of the second, then
        // nothing for the last chunk.
        let sizes: Vec<usize> = chunks.iter().map(|(chunk, _)| chunk.len()).collect();
        assert_eq!(sizes, [HEADER_SIZE, first, 100, 0]);
    }
}
