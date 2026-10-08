// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbBSArchive.pas (the DX10 parts), Core/wbDDS.pas
// (GetDXGIFormatName)

//! The extension points of the texture archives (Fallout 4 and Starfield
//! DX10 `.ba2`), which phase 5 step 2 fills in with `wbDDS`.
//!
//! What step 1 does with a DX10 archive: it reads and lists one (the file
//! table with the dimensions, format and mipmap chunks of each texture),
//! writes the table of one whose chunks are set (`Archive::save`), and
//! reserves the table space in `create_archive`. What it leaves to step 2 is
//! everything that looks inside a DDS file or builds one: the three
//! functions below, each of which fails with `unsupported()` for now.
//!
//! * `chunk_dds` is `TwbSplitPacker.LoadFile` for a DDS: split a DDS file
//!   into the uncompressed header chunk and the compressed mipmap chunks.
//! * `pack_dds_chunks` and `pack_dds` are `TwbBSArchive.Pack` for a DDS
//!   archive, with and without a packer: fill the `dds` description and
//!   store the chunks of an entry.
//! * `unpack_dds` is the `baFO4dds`/`baSFdds` case of `TwbBSArchive.Unpack`:
//!   rebuild the DDS file of an entry from its chunks.

use super::{Archive, ArchiveError, FileEntry, Target};
use crate::hash::LookupHash;

/// The error of everything step 2 provides.
pub(super) fn unsupported() -> ArchiveError {
    ArchiveError("Texture (DX10) archives are not supported yet".to_owned())
}

/// The settings that decide how a texture is cut into chunks
/// (`fMaxChunkCount`, `fSingleMipChunkX`, `fSingleMipChunkY`, `fTarget`),
/// those of the packer and of the archive.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // read by the DDS code of step 2
pub struct TextureConfig {
    pub max_chunk_count: i32,
    pub single_mip_chunk_x: i32,
    pub single_mip_chunk_y: i32,
    pub target: Target,
}

/// Port of `TwbSplitPacker.LoadFile` for a DDS archive: the data of the
/// chunks of one DDS file in order (chunk 0 is the uncompressed DDS header,
/// the others are the mipmap chunks, compressed when `compress`), each with
/// its uncompressed size, and the size and lookup hash of the DDS data after
/// the conversion of 24 bit formats, when `share`. `buf` is the whole DDS file.
#[allow(clippy::type_complexity)]
pub(super) fn chunk_dds(
    _config: &TextureConfig,
    _buf: Vec<u8>,
    _compress: bool,
    _share: bool,
) -> Result<(Vec<(Vec<u8>, i32)>, Option<(i32, LookupHash)>), ArchiveError> {
    Err(unsupported())
}

/// Port of the DDS branch of `TwbBSArchive.Pack` with a packer: stores the
/// chunks that `chunk_dds` made, with the lookup hash of each, through
/// `Archive::pack_chunk`, and fills in the `dds` description of the entry.
pub(super) fn pack_dds_chunks(
    _archive: &mut Archive,
    _entry: usize,
    _chunks: Vec<(Vec<u8>, i32, Option<LookupHash>)>,
) -> Result<(), ArchiveError> {
    Err(unsupported())
}

/// Port of the DDS branch of `TwbBSArchive.Pack` without a packer: fills
/// the `dds` description and the chunk list of `entry` from the DDS file
/// `data` and stores the chunks through `Archive::pack_chunk`.
pub(super) fn pack_dds(_archive: &mut Archive, _entry: usize, _data: &[u8]) -> Result<(), ArchiveError> {
    Err(unsupported())
}

/// Port of the `baFO4dds`/`baSFdds` branch of `TwbBSArchive.Unpack`.
pub(super) fn unpack_dds(_archive: &Archive, _entry: &FileEntry) -> Result<Vec<u8>, ArchiveError> {
    Err(unsupported())
}

/// The names of `TDXGI` without the `DXGI_FORMAT_` prefix, indexed by the
/// format number.
const DXGI_FORMAT_NAMES: [&str; 116] = [
    "UNKNOWN",
    "R32G32B32A32_TYPELESS",
    "R32G32B32A32_FLOAT",
    "R32G32B32A32_UINT",
    "R32G32B32A32_SINT",
    "R32G32B32_TYPELESS",
    "R32G32B32_FLOAT",
    "R32G32B32_UINT",
    "R32G32B32_SINT",
    "R16G16B16A16_TYPELESS",
    "R16G16B16A16_FLOAT",
    "R16G16B16A16_UNORM",
    "R16G16B16A16_UINT",
    "R16G16B16A16_SNORM",
    "R16G16B16A16_SINT",
    "R32G32_TYPELESS",
    "R32G32_FLOAT",
    "R32G32_UINT",
    "R32G32_SINT",
    "R32G8X24_TYPELESS",
    "D32_FLOAT_S8X24_UINT",
    "R32_FLOAT_X8X24_TYPELESS",
    "X32_TYPELESS_G8X24_UINT",
    "R10G10B10A2_TYPELESS",
    "R10G10B10A2_UNORM",
    "R10G10B10A2_UINT",
    "R11G11B10_FLOAT",
    "R8G8B8A8_TYPELESS",
    "R8G8B8A8_UNORM",
    "R8G8B8A8_UNORM_SRGB",
    "R8G8B8A8_UINT",
    "R8G8B8A8_SNORM",
    "R8G8B8A8_SINT",
    "R16G16_TYPELESS",
    "R16G16_FLOAT",
    "R16G16_UNORM",
    "R16G16_UINT",
    "R16G16_SNORM",
    "R16G16_SINT",
    "R32_TYPELESS",
    "D32_FLOAT",
    "R32_FLOAT",
    "R32_UINT",
    "R32_SINT",
    "R24G8_TYPELESS",
    "D24_UNORM_S8_UINT",
    "R24_UNORM_X8_TYPELESS",
    "X24_TYPELESS_G8_UINT",
    "R8G8_TYPELESS",
    "R8G8_UNORM",
    "R8G8_UINT",
    "R8G8_SNORM",
    "R8G8_SINT",
    "R16_TYPELESS",
    "R16_FLOAT",
    "D16_UNORM",
    "R16_UNORM",
    "R16_UINT",
    "R16_SNORM",
    "R16_SINT",
    "R8_TYPELESS",
    "R8_UNORM",
    "R8_UINT",
    "R8_SNORM",
    "R8_SINT",
    "A8_UNORM",
    "R1_UNORM",
    "R9G9B9E5_SHAREDEXP",
    "R8G8_B8G8_UNORM",
    "G8R8_G8B8_UNORM",
    "BC1_TYPELESS",
    "BC1_UNORM",
    "BC1_UNORM_SRGB",
    "BC2_TYPELESS",
    "BC2_UNORM",
    "BC2_UNORM_SRGB",
    "BC3_TYPELESS",
    "BC3_UNORM",
    "BC3_UNORM_SRGB",
    "BC4_TYPELESS",
    "BC4_UNORM",
    "BC4_SNORM",
    "BC5_TYPELESS",
    "BC5_UNORM",
    "BC5_SNORM",
    "B5G6R5_UNORM",
    "B5G5R5A1_UNORM",
    "B8G8R8A8_UNORM",
    "B8G8R8X8_UNORM",
    "R10G10B10_XR_BIAS_A2_UNORM",
    "B8G8R8A8_TYPELESS",
    "B8G8R8A8_UNORM_SRGB",
    "B8G8R8X8_TYPELESS",
    "B8G8R8X8_UNORM_SRGB",
    "BC6H_TYPELESS",
    "BC6H_UF16",
    "BC6H_SF16",
    "BC7_TYPELESS",
    "BC7_UNORM",
    "BC7_UNORM_SRGB",
    "AYUV",
    "Y410",
    "Y416",
    "NV12",
    "P010",
    "P016",
    "420_OPAQUE",
    "YUY2",
    "Y210",
    "Y216",
    "NV11",
    "AI44",
    "IA44",
    "P8",
    "A8P8",
    "B4G4R4A4_UNORM",
];

/// Port of `TwbDDS.GetDXGIFormatName`: the name of a DXGI format, empty for
/// a number that is not one.
pub fn dxgi_format_name(format: u8) -> &'static str {
    DXGI_FORMAT_NAMES.get(usize::from(format)).copied().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
