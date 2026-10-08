// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDDS.pas

//! The DDS file header: `TwbDDS` of `wbDDS.pas`.
//!
//! Upstream reads a header through a pointer cast onto the file's bytes. Here
//! the header is parsed from, and written to, a byte slice; a slice shorter
//! than the structure reads as if zero padded (upstream would read whatever
//! follows the buffer), and every caller checks `is_dds` first, as upstream's
//! do.

use crate::archive::ArchiveError;

/// `TMagic4`.
pub type Magic4 = [u8; 4];

pub const MAGIC_DDS: Magic4 = *b"DDS ";
pub const MAGIC_DXT1: Magic4 = *b"DXT1";
pub const MAGIC_DXT3: Magic4 = *b"DXT3";
pub const MAGIC_DXT5: Magic4 = *b"DXT5";
pub const MAGIC_ATI1: Magic4 = *b"ATI1";
pub const MAGIC_ATI2: Magic4 = *b"ATI2";
pub const MAGIC_BC4S: Magic4 = *b"BC4S";
pub const MAGIC_BC4U: Magic4 = *b"BC4U";
pub const MAGIC_BC5S: Magic4 = *b"BC5S";
pub const MAGIC_BC5U: Magic4 = *b"BC5U";
pub const MAGIC_DX10: Magic4 = *b"DX10";
pub const MAGIC_XBOX: Magic4 = *b"XBOX";

pub const DDSD_CAPS: u32 = 0x0000_0001;
pub const DDSD_HEIGHT: u32 = 0x0000_0002;
pub const DDSD_WIDTH: u32 = 0x0000_0004;
pub const DDSD_PITCH: u32 = 0x0000_0008;
pub const DDSD_PIXELFORMAT: u32 = 0x0000_1000;
pub const DDSD_MIPMAPCOUNT: u32 = 0x0002_0000;
pub const DDSD_LINEARSIZE: u32 = 0x0008_0000;
pub const DDSD_DEPTH: u32 = 0x0080_0000;

pub const DDSCAPS_COMPLEX: u32 = 0x0000_0008;
pub const DDSCAPS_TEXTURE: u32 = 0x0000_1000;
pub const DDSCAPS_MIPMAP: u32 = 0x0040_0000;

pub const DDSCAPS2_CUBEMAP: u32 = 0x0000_0200;
pub const DDSCAPS2_CUBEMAP_POSITIVEX: u32 = 0x0000_0400;
pub const DDSCAPS2_CUBEMAP_NEGATIVEX: u32 = 0x0000_0800;
pub const DDSCAPS2_CUBEMAP_POSITIVEY: u32 = 0x0000_1000;
pub const DDSCAPS2_CUBEMAP_NEGATIVEY: u32 = 0x0000_2000;
pub const DDSCAPS2_CUBEMAP_POSITIVEZ: u32 = 0x0000_4000;
pub const DDSCAPS2_CUBEMAP_NEGATIVEZ: u32 = 0x0000_8000;
pub const DDSCAPS2_CUBEMAP_VOLUME: u32 = 0x0020_0000;
pub const DDSCAPS2_CUBEMAP_ALLFACES: u32 = DDSCAPS2_CUBEMAP_POSITIVEX
    | DDSCAPS2_CUBEMAP_NEGATIVEX
    | DDSCAPS2_CUBEMAP_POSITIVEY
    | DDSCAPS2_CUBEMAP_NEGATIVEY
    | DDSCAPS2_CUBEMAP_POSITIVEZ
    | DDSCAPS2_CUBEMAP_NEGATIVEZ;

pub const DDPF_ALPHAPIXELS: u32 = 0x0000_0001;
pub const DDPF_ALPHA: u32 = 0x0000_0002;
pub const DDPF_FOURCC: u32 = 0x0000_0004;
pub const DDPF_RGB: u32 = 0x0000_0040;
pub const DDPF_YUV: u32 = 0x0000_0200;
pub const DDPF_LUMINANCE: u32 = 0x0002_0000;

// DX10.
pub const DDS_DIMENSION_TEXTURE2D: u32 = 0x0000_0003;
pub const DDS_RESOURCE_MISC_TEXTURECUBE: u32 = 0x0000_0004;
pub const DDS_ALPHA_MODE_UNKNOWN: u32 = 0x0000_0000;
pub const DDS_ALPHA_MODE_STRAIGHT: u32 = 0x0000_0001;
pub const DDS_ALPHA_MODE_PREMULTIPLIED: u32 = 0x0000_0002;
pub const DDS_ALPHA_MODE_OPAQUE: u32 = 0x0000_0003;
pub const DDS_ALPHA_MODE_CUSTOM: u32 = 0x0000_0004;

/// `SizeOf(TDDSHeader)`: the magic and the 124 bytes of the header.
pub const HEADER_SIZE: usize = 128;
/// `SizeOf(TDDSHeaderDX10)`.
pub const HEADER_DX10_SIZE: usize = 20;
/// `SizeOf(TDDSHeaderXBOX)`.
pub const HEADER_XBOX_SIZE: usize = 16;
/// `TwbDDS.MaxHeaderSize`.
pub const MAX_HEADER_SIZE: usize = HEADER_SIZE + HEADER_DX10_SIZE + HEADER_XBOX_SIZE;

/// `TDXGI`: the number of a DXGI format, as upstream numbers them (the
/// enumeration is contiguous from `DXGI_FORMAT_UNKNOWN` to
/// `DXGI_FORMAT_B4G4R4A4_UNORM`, which is the numbering of DXGI itself up to
/// there). Upstream's enumeration has one byte, so a number from a file is
/// cut to a byte; a number the enumeration has no name for is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dxgi(pub u8);

/// The members of `TDXGI` without the `DXGI_FORMAT_` prefix (`420_OPAQUE`
/// is `P420_OPAQUE`).
#[allow(non_upper_case_globals)]
impl Dxgi {
    pub const UNKNOWN: Dxgi = Dxgi(0);
    pub const R32G32B32A32_TYPELESS: Dxgi = Dxgi(1);
    pub const R32G32B32A32_FLOAT: Dxgi = Dxgi(2);
    pub const R32G32B32A32_UINT: Dxgi = Dxgi(3);
    pub const R32G32B32A32_SINT: Dxgi = Dxgi(4);
    pub const R32G32B32_TYPELESS: Dxgi = Dxgi(5);
    pub const R32G32B32_FLOAT: Dxgi = Dxgi(6);
    pub const R32G32B32_UINT: Dxgi = Dxgi(7);
    pub const R32G32B32_SINT: Dxgi = Dxgi(8);
    pub const R16G16B16A16_TYPELESS: Dxgi = Dxgi(9);
    pub const R16G16B16A16_FLOAT: Dxgi = Dxgi(10);
    pub const R16G16B16A16_UNORM: Dxgi = Dxgi(11);
    pub const R16G16B16A16_UINT: Dxgi = Dxgi(12);
    pub const R16G16B16A16_SNORM: Dxgi = Dxgi(13);
    pub const R16G16B16A16_SINT: Dxgi = Dxgi(14);
    pub const R32G32_TYPELESS: Dxgi = Dxgi(15);
    pub const R32G32_FLOAT: Dxgi = Dxgi(16);
    pub const R32G32_UINT: Dxgi = Dxgi(17);
    pub const R32G32_SINT: Dxgi = Dxgi(18);
    pub const R32G8X24_TYPELESS: Dxgi = Dxgi(19);
    pub const D32_FLOAT_S8X24_UINT: Dxgi = Dxgi(20);
    pub const R32_FLOAT_X8X24_TYPELESS: Dxgi = Dxgi(21);
    pub const X32_TYPELESS_G8X24_UINT: Dxgi = Dxgi(22);
    pub const R10G10B10A2_TYPELESS: Dxgi = Dxgi(23);
    pub const R10G10B10A2_UNORM: Dxgi = Dxgi(24);
    pub const R10G10B10A2_UINT: Dxgi = Dxgi(25);
    pub const R11G11B10_FLOAT: Dxgi = Dxgi(26);
    pub const R8G8B8A8_TYPELESS: Dxgi = Dxgi(27);
    pub const R8G8B8A8_UNORM: Dxgi = Dxgi(28);
    pub const R8G8B8A8_UNORM_SRGB: Dxgi = Dxgi(29);
    pub const R8G8B8A8_UINT: Dxgi = Dxgi(30);
    pub const R8G8B8A8_SNORM: Dxgi = Dxgi(31);
    pub const R8G8B8A8_SINT: Dxgi = Dxgi(32);
    pub const R16G16_TYPELESS: Dxgi = Dxgi(33);
    pub const R16G16_FLOAT: Dxgi = Dxgi(34);
    pub const R16G16_UNORM: Dxgi = Dxgi(35);
    pub const R16G16_UINT: Dxgi = Dxgi(36);
    pub const R16G16_SNORM: Dxgi = Dxgi(37);
    pub const R16G16_SINT: Dxgi = Dxgi(38);
    pub const R32_TYPELESS: Dxgi = Dxgi(39);
    pub const D32_FLOAT: Dxgi = Dxgi(40);
    pub const R32_FLOAT: Dxgi = Dxgi(41);
    pub const R32_UINT: Dxgi = Dxgi(42);
    pub const R32_SINT: Dxgi = Dxgi(43);
    pub const R24G8_TYPELESS: Dxgi = Dxgi(44);
    pub const D24_UNORM_S8_UINT: Dxgi = Dxgi(45);
    pub const R24_UNORM_X8_TYPELESS: Dxgi = Dxgi(46);
    pub const X24_TYPELESS_G8_UINT: Dxgi = Dxgi(47);
    pub const R8G8_TYPELESS: Dxgi = Dxgi(48);
    pub const R8G8_UNORM: Dxgi = Dxgi(49);
    pub const R8G8_UINT: Dxgi = Dxgi(50);
    pub const R8G8_SNORM: Dxgi = Dxgi(51);
    pub const R8G8_SINT: Dxgi = Dxgi(52);
    pub const R16_TYPELESS: Dxgi = Dxgi(53);
    pub const R16_FLOAT: Dxgi = Dxgi(54);
    pub const D16_UNORM: Dxgi = Dxgi(55);
    pub const R16_UNORM: Dxgi = Dxgi(56);
    pub const R16_UINT: Dxgi = Dxgi(57);
    pub const R16_SNORM: Dxgi = Dxgi(58);
    pub const R16_SINT: Dxgi = Dxgi(59);
    pub const R8_TYPELESS: Dxgi = Dxgi(60);
    pub const R8_UNORM: Dxgi = Dxgi(61);
    pub const R8_UINT: Dxgi = Dxgi(62);
    pub const R8_SNORM: Dxgi = Dxgi(63);
    pub const R8_SINT: Dxgi = Dxgi(64);
    pub const A8_UNORM: Dxgi = Dxgi(65);
    pub const R1_UNORM: Dxgi = Dxgi(66);
    pub const R9G9B9E5_SHAREDEXP: Dxgi = Dxgi(67);
    pub const R8G8_B8G8_UNORM: Dxgi = Dxgi(68);
    pub const G8R8_G8B8_UNORM: Dxgi = Dxgi(69);
    pub const BC1_TYPELESS: Dxgi = Dxgi(70);
    pub const BC1_UNORM: Dxgi = Dxgi(71);
    pub const BC1_UNORM_SRGB: Dxgi = Dxgi(72);
    pub const BC2_TYPELESS: Dxgi = Dxgi(73);
    pub const BC2_UNORM: Dxgi = Dxgi(74);
    pub const BC2_UNORM_SRGB: Dxgi = Dxgi(75);
    pub const BC3_TYPELESS: Dxgi = Dxgi(76);
    pub const BC3_UNORM: Dxgi = Dxgi(77);
    pub const BC3_UNORM_SRGB: Dxgi = Dxgi(78);
    pub const BC4_TYPELESS: Dxgi = Dxgi(79);
    pub const BC4_UNORM: Dxgi = Dxgi(80);
    pub const BC4_SNORM: Dxgi = Dxgi(81);
    pub const BC5_TYPELESS: Dxgi = Dxgi(82);
    pub const BC5_UNORM: Dxgi = Dxgi(83);
    pub const BC5_SNORM: Dxgi = Dxgi(84);
    pub const B5G6R5_UNORM: Dxgi = Dxgi(85);
    pub const B5G5R5A1_UNORM: Dxgi = Dxgi(86);
    pub const B8G8R8A8_UNORM: Dxgi = Dxgi(87);
    pub const B8G8R8X8_UNORM: Dxgi = Dxgi(88);
    pub const R10G10B10_XR_BIAS_A2_UNORM: Dxgi = Dxgi(89);
    pub const B8G8R8A8_TYPELESS: Dxgi = Dxgi(90);
    pub const B8G8R8A8_UNORM_SRGB: Dxgi = Dxgi(91);
    pub const B8G8R8X8_TYPELESS: Dxgi = Dxgi(92);
    pub const B8G8R8X8_UNORM_SRGB: Dxgi = Dxgi(93);
    pub const BC6H_TYPELESS: Dxgi = Dxgi(94);
    pub const BC6H_UF16: Dxgi = Dxgi(95);
    pub const BC6H_SF16: Dxgi = Dxgi(96);
    pub const BC7_TYPELESS: Dxgi = Dxgi(97);
    pub const BC7_UNORM: Dxgi = Dxgi(98);
    pub const BC7_UNORM_SRGB: Dxgi = Dxgi(99);
    pub const AYUV: Dxgi = Dxgi(100);
    pub const Y410: Dxgi = Dxgi(101);
    pub const Y416: Dxgi = Dxgi(102);
    pub const NV12: Dxgi = Dxgi(103);
    pub const P010: Dxgi = Dxgi(104);
    pub const P016: Dxgi = Dxgi(105);
    pub const P420_OPAQUE: Dxgi = Dxgi(106);
    pub const YUY2: Dxgi = Dxgi(107);
    pub const Y210: Dxgi = Dxgi(108);
    pub const Y216: Dxgi = Dxgi(109);
    pub const NV11: Dxgi = Dxgi(110);
    pub const AI44: Dxgi = Dxgi(111);
    pub const IA44: Dxgi = Dxgi(112);
    pub const P8: Dxgi = Dxgi(113);
    pub const A8P8: Dxgi = Dxgi(114);
    pub const B4G4R4A4_UNORM: Dxgi = Dxgi(115);
}

/// `TD3DFORMAT`: the legacy Direct3D 9 format numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct D3dFormat(pub u8);

impl D3dFormat {
    pub const UNKNOWN: D3dFormat = D3dFormat(0);
    pub const R8G8B8: D3dFormat = D3dFormat(20);
    pub const A8R8G8B8: D3dFormat = D3dFormat(21);
    pub const X8R8G8B8: D3dFormat = D3dFormat(22);
    pub const R5G6B5: D3dFormat = D3dFormat(23);
    pub const X1R5G5B5: D3dFormat = D3dFormat(24);
    pub const A1R5G5B5: D3dFormat = D3dFormat(25);
    pub const A4R4G4B4: D3dFormat = D3dFormat(26);
    pub const R3G3B2: D3dFormat = D3dFormat(27);
    pub const A8: D3dFormat = D3dFormat(28);
    pub const A8R3G3B2: D3dFormat = D3dFormat(29);
    pub const X4R4G4B4: D3dFormat = D3dFormat(30);
    pub const A2B10G10R10: D3dFormat = D3dFormat(31);
    pub const A8B8G8R8: D3dFormat = D3dFormat(32);
    pub const X8B8G8R8: D3dFormat = D3dFormat(33);
    pub const G16R16: D3dFormat = D3dFormat(34);
    pub const A2R10G10B10: D3dFormat = D3dFormat(35);
    pub const A16B16G16R16: D3dFormat = D3dFormat(36);
    pub const A8P8: D3dFormat = D3dFormat(40);
    pub const P8: D3dFormat = D3dFormat(41);
    pub const L8: D3dFormat = D3dFormat(50);
    pub const A8L8: D3dFormat = D3dFormat(51);
    pub const A4L4: D3dFormat = D3dFormat(52);
    pub const V8U8: D3dFormat = D3dFormat(60);
    pub const L6V5U5: D3dFormat = D3dFormat(61);
    pub const X8L8V8U8: D3dFormat = D3dFormat(62);
    pub const Q8W8V8U8: D3dFormat = D3dFormat(63);
    pub const V16U16: D3dFormat = D3dFormat(64);
    pub const A2W10V10U10: D3dFormat = D3dFormat(67);
}

/// `TwbDDS.DXGI_DX9`: the formats `SetUpHeader` writes without the
/// additional DX10 header.
pub const DXGI_DX9: &[Dxgi] = &[
    Dxgi::BC1_UNORM,
    Dxgi::BC2_UNORM,
    Dxgi::BC3_UNORM,
    Dxgi::BC4_SNORM,
    Dxgi::BC4_UNORM,
    Dxgi::BC5_SNORM,
    Dxgi::BC5_UNORM,
    Dxgi::R8G8B8A8_UNORM,
    Dxgi::B8G8R8A8_UNORM,
    Dxgi::B8G8R8X8_UNORM,
    Dxgi::B5G6R5_UNORM,
    Dxgi::B5G5R5A1_UNORM,
    Dxgi::R8G8_UNORM,
    Dxgi::A8_UNORM,
    Dxgi::R8_UNORM,
];

/// `TwbDDS.DXGI_COMPRESSED`: the formats that use block compression.
pub const DXGI_COMPRESSED: &[Dxgi] = &[
    Dxgi::BC1_UNORM,
    Dxgi::BC1_UNORM_SRGB,
    Dxgi::BC1_TYPELESS,
    Dxgi::BC2_UNORM,
    Dxgi::BC2_UNORM_SRGB,
    Dxgi::BC2_TYPELESS,
    Dxgi::BC3_UNORM,
    Dxgi::BC3_UNORM_SRGB,
    Dxgi::BC3_TYPELESS,
    Dxgi::BC4_UNORM,
    Dxgi::BC4_SNORM,
    Dxgi::BC4_TYPELESS,
    Dxgi::BC5_UNORM,
    Dxgi::BC5_SNORM,
    Dxgi::BC5_TYPELESS,
    Dxgi::BC6H_UF16,
    Dxgi::BC6H_SF16,
    Dxgi::BC6H_TYPELESS,
    Dxgi::BC7_UNORM,
    Dxgi::BC7_UNORM_SRGB,
    Dxgi::BC7_TYPELESS,
];

/// `TwbDDS.DXGI_ALPHA`: the formats with an alpha channel.
pub const DXGI_ALPHA: &[Dxgi] = &[
    Dxgi::R32G32B32A32_TYPELESS,
    Dxgi::R32G32B32A32_FLOAT,
    Dxgi::R32G32B32A32_UINT,
    Dxgi::R32G32B32A32_SINT,
    Dxgi::R16G16B16A16_TYPELESS,
    Dxgi::R16G16B16A16_FLOAT,
    Dxgi::R16G16B16A16_UNORM,
    Dxgi::R16G16B16A16_UINT,
    Dxgi::R16G16B16A16_SNORM,
    Dxgi::R16G16B16A16_SINT,
    Dxgi::R10G10B10A2_TYPELESS,
    Dxgi::R10G10B10A2_UNORM,
    Dxgi::R10G10B10A2_UINT,
    Dxgi::R8G8B8A8_TYPELESS,
    Dxgi::R8G8B8A8_UNORM,
    Dxgi::R8G8B8A8_UNORM_SRGB,
    Dxgi::R8G8B8A8_UINT,
    Dxgi::R8G8B8A8_SNORM,
    Dxgi::R8G8B8A8_SINT,
    Dxgi::A8_UNORM,
    Dxgi::BC1_TYPELESS,
    Dxgi::BC1_UNORM,
    Dxgi::BC1_UNORM_SRGB,
    Dxgi::BC2_TYPELESS,
    Dxgi::BC2_UNORM,
    Dxgi::BC2_UNORM_SRGB,
    Dxgi::BC3_TYPELESS,
    Dxgi::BC3_UNORM,
    Dxgi::BC3_UNORM_SRGB,
    Dxgi::B5G5R5A1_UNORM,
    Dxgi::B8G8R8A8_UNORM,
    Dxgi::R10G10B10_XR_BIAS_A2_UNORM,
    Dxgi::B8G8R8A8_TYPELESS,
    Dxgi::B8G8R8A8_UNORM_SRGB,
    Dxgi::BC7_TYPELESS,
    Dxgi::BC7_UNORM,
    Dxgi::BC7_UNORM_SRGB,
    Dxgi::AYUV,
    Dxgi::Y410,
    Dxgi::Y416,
    Dxgi::AI44,
    Dxgi::IA44,
    Dxgi::A8P8,
    Dxgi::B4G4R4A4_UNORM,
];

/// `TwbDDS.D3D_NODXGI`: the Direct3D 9 formats DXGI has no counterpart for
/// (<https://learn.microsoft.com/windows/win32/direct3d10/d3d10-graphics-programming-guide-resources-legacy-formats>).
pub const D3D_NODXGI: &[D3dFormat] = &[
    D3dFormat::R8G8B8,
    D3dFormat::X1R5G5B5,
    D3dFormat::R3G3B2,
    D3dFormat::A8R3G3B2,
    D3dFormat::X4R4G4B4,
    D3dFormat::X8B8G8R8,
    D3dFormat::A2R10G10B10,
];

/// The pixel format of a `TDDSHeader` (`ddspf`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PixelFormat {
    pub size: u32,
    pub flags: u32,
    pub four_cc: Magic4,
    pub rgb_bit_count: u32,
    pub r_bit_mask: u32,
    pub g_bit_mask: u32,
    pub b_bit_mask: u32,
    pub a_bit_mask: u32,
}

/// `TDDSHeader`: the 128 bytes at the start of a DDS file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DdsHeader {
    pub magic: Magic4,
    pub size: u32,
    pub flags: u32,
    pub height: u32,
    pub width: u32,
    pub pitch_or_linear_size: u32,
    pub depth: u32,
    pub mip_map_count: u32,
    pub reserved1: [u32; 11],
    pub pixel_format: PixelFormat,
    pub caps: u32,
    pub caps2: u32,
    pub caps3: u32,
    pub caps4: u32,
    pub reserved2: u32,
}

/// `TDDSHeaderDX10`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DdsHeaderDx10 {
    pub dxgi_format: i32,
    pub resource_dimension: u32,
    pub misc_flags: u32,
    pub array_size: u32,
    pub misc_flags2: u32,
}

/// `TDDSHeaderXBOX`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DdsHeaderXbox {
    pub tile_mode: u32,
    pub base_alignment: u32,
    pub data_size: u32,
    pub xdk_ver: u32,
}

/// The little-endian word at `offset`, zero where the slice ends.
fn word(data: &[u8], offset: usize) -> u32 {
    let mut bytes = [0u8; 4];
    if let Some(tail) = data.get(offset..) {
        let count = tail.len().min(4);
        bytes[..count].copy_from_slice(&tail[..count]);
    }
    u32::from_le_bytes(bytes)
}

fn magic(data: &[u8], offset: usize) -> Magic4 {
    word(data, offset).to_le_bytes()
}

fn put(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

impl DdsHeader {
    /// The header at the start of `data`.
    pub fn read(data: &[u8]) -> DdsHeader {
        let mut reserved1 = [0u32; 11];
        for (index, value) in reserved1.iter_mut().enumerate() {
            *value = word(data, 32 + index * 4);
        }
        DdsHeader {
            magic: magic(data, 0),
            size: word(data, 4),
            flags: word(data, 8),
            height: word(data, 12),
            width: word(data, 16),
            pitch_or_linear_size: word(data, 20),
            depth: word(data, 24),
            mip_map_count: word(data, 28),
            reserved1,
            pixel_format: PixelFormat {
                size: word(data, 76),
                flags: word(data, 80),
                four_cc: magic(data, 84),
                rgb_bit_count: word(data, 88),
                r_bit_mask: word(data, 92),
                g_bit_mask: word(data, 96),
                b_bit_mask: word(data, 100),
                a_bit_mask: word(data, 104),
            },
            caps: word(data, 108),
            caps2: word(data, 112),
            caps3: word(data, 116),
            caps4: word(data, 120),
            reserved2: word(data, 124),
        }
    }

    /// Writes the header to the first `HEADER_SIZE` bytes of `data`.
    pub fn write(&self, data: &mut [u8]) {
        data[0..4].copy_from_slice(&self.magic);
        put(data, 4, self.size);
        put(data, 8, self.flags);
        put(data, 12, self.height);
        put(data, 16, self.width);
        put(data, 20, self.pitch_or_linear_size);
        put(data, 24, self.depth);
        put(data, 28, self.mip_map_count);
        for (index, value) in self.reserved1.iter().enumerate() {
            put(data, 32 + index * 4, *value);
        }
        let pixel_format = &self.pixel_format;
        put(data, 76, pixel_format.size);
        put(data, 80, pixel_format.flags);
        data[84..88].copy_from_slice(&pixel_format.four_cc);
        put(data, 88, pixel_format.rgb_bit_count);
        put(data, 92, pixel_format.r_bit_mask);
        put(data, 96, pixel_format.g_bit_mask);
        put(data, 100, pixel_format.b_bit_mask);
        put(data, 104, pixel_format.a_bit_mask);
        put(data, 108, self.caps);
        put(data, 112, self.caps2);
        put(data, 116, self.caps3);
        put(data, 120, self.caps4);
        put(data, 124, self.reserved2);
    }
}

impl DdsHeaderDx10 {
    /// Writes the header to `data[offset..]`.
    pub fn write(&self, data: &mut [u8], offset: usize) {
        put(data, offset, self.dxgi_format as u32);
        put(data, offset + 4, self.resource_dimension);
        put(data, offset + 8, self.misc_flags);
        put(data, offset + 12, self.array_size);
        put(data, offset + 16, self.misc_flags2);
    }
}

impl DdsHeaderXbox {
    /// Writes the header to `data[offset..]`.
    pub fn write(&self, data: &mut [u8], offset: usize) {
        put(data, offset, self.tile_mode);
        put(data, offset + 4, self.base_alignment);
        put(data, offset + 8, self.data_size);
        put(data, offset + 12, self.xdk_ver);
    }
}

/// Port of `TwbDDS.IsDDS`.
pub fn is_dds(data: &[u8]) -> bool {
    data.len() >= HEADER_SIZE && magic(data, 0) == MAGIC_DDS && data.len() >= header_size(data)
}

/// Port of `TwbDDS.IsXBOX`.
pub fn is_xbox(data: &[u8]) -> bool {
    magic(data, 84) == MAGIC_XBOX
}

/// Port of `TwbDDS.IsCubeMap`.
pub fn is_cube_map(data: &[u8]) -> bool {
    word(data, 112) & DDSCAPS2_CUBEMAP != 0
}

/// Port of `TwbDDS.HasAlpha`.
pub fn has_alpha(dxgi: Dxgi) -> bool {
    DXGI_ALPHA.contains(&dxgi)
}

/// Port of `TwbDDS.IsCompressed`.
pub fn is_compressed(dxgi: Dxgi) -> bool {
    DXGI_COMPRESSED.contains(&dxgi)
}

/// Port of `TwbDDS.HeaderDX10`: the DX10 header that follows the header.
pub fn header_dx10(data: &[u8]) -> DdsHeaderDx10 {
    DdsHeaderDx10 {
        dxgi_format: word(data, HEADER_SIZE) as i32,
        resource_dimension: word(data, HEADER_SIZE + 4),
        misc_flags: word(data, HEADER_SIZE + 8),
        array_size: word(data, HEADER_SIZE + 12),
        misc_flags2: word(data, HEADER_SIZE + 16),
    }
}

/// Port of `TwbDDS.HeaderXBOX`: the XBOX header that follows the DX10 header.
pub fn header_xbox(data: &[u8]) -> DdsHeaderXbox {
    let at = HEADER_SIZE + HEADER_DX10_SIZE;
    DdsHeaderXbox {
        tile_mode: word(data, at),
        base_alignment: word(data, at + 4),
        data_size: word(data, at + 8),
        xdk_ver: word(data, at + 12),
    }
}

/// Port of `TwbDDS.GetHeaderSize`: the offset to the image data.
pub fn header_size(data: &[u8]) -> usize {
    let four_cc = magic(data, 84);
    let mut result = HEADER_SIZE;
    if four_cc == MAGIC_DX10 || four_cc == MAGIC_XBOX {
        result += HEADER_DX10_SIZE;
    }
    if four_cc == MAGIC_XBOX {
        result += HEADER_XBOX_SIZE;
    }
    result
}

/// Port of `TwbDDS.GetMipSize`: the size of the first mipmap in bytes.
/// Upstream computes in 32 bits without a range check.
pub fn mip_size(data: &[u8]) -> i32 {
    let header = DdsHeader::read(data);
    (header
        .width
        .wrapping_mul(header.height)
        .wrapping_mul(u32::from(bits_per_pixel_of(data)))
        >> 3) as i32
}

/// Port of `TwbDDS.GetTileMode`.
pub fn tile_mode(data: &[u8]) -> i32 {
    if is_xbox(data) {
        header_xbox(data).tile_mode as i32
    } else {
        8
    }
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

/// Port of `TwbDDS.GetD3DFMTFormatName`.
pub fn d3dfmt_format_name(format: D3dFormat) -> &'static str {
    match format {
        D3dFormat::UNKNOWN => "UNKNOWN",
        D3dFormat::R8G8B8 => "R8G8B8",
        D3dFormat::A8R8G8B8 => "A8R8G8B8",
        D3dFormat::X8R8G8B8 => "X8R8G8B8",
        D3dFormat::R5G6B5 => "R5G6B5",
        D3dFormat::X1R5G5B5 => "X1R5G5B5",
        D3dFormat::A1R5G5B5 => "A1R5G5B5",
        D3dFormat::A4R4G4B4 => "A4R4G4B4",
        D3dFormat::R3G3B2 => "R3G3B2",
        D3dFormat::A8 => "A8",
        D3dFormat::A8R3G3B2 => "A8R3G3B2",
        D3dFormat::X4R4G4B4 => "X4R4G4B4",
        D3dFormat::A2B10G10R10 => "A2B10G10R10",
        D3dFormat::A8B8G8R8 => "A8B8G8R8",
        D3dFormat::X8B8G8R8 => "X8B8G8R8",
        D3dFormat::G16R16 => "G16R16",
        D3dFormat::A2R10G10B10 => "A2R10G10B10",
        D3dFormat::A16B16G16R16 => "A16B16G16R16",
        _ => "UNKNOWN",
    }
}

/// Port of `TwbDDS.GetDXGI`: the DXGI format of a DDS file, from its FourCC,
/// its DX10 header or its pixel format masks.
pub fn dxgi(data: &[u8]) -> Dxgi {
    let header = DdsHeader::read(data);
    let pf = &header.pixel_format;
    let four_cc = pf.four_cc;
    if four_cc == MAGIC_DXT1 {
        Dxgi::BC1_UNORM
    } else if four_cc == MAGIC_DXT3 {
        Dxgi::BC2_UNORM
    } else if four_cc == MAGIC_DXT5 {
        Dxgi::BC3_UNORM
    } else if four_cc == MAGIC_ATI1 || four_cc == MAGIC_BC4U {
        Dxgi::BC4_UNORM
    } else if four_cc == MAGIC_BC4S {
        Dxgi::BC4_SNORM
    } else if four_cc == MAGIC_ATI2 || four_cc == MAGIC_BC5U {
        Dxgi::BC5_UNORM
    } else if four_cc == MAGIC_BC5S {
        Dxgi::BC5_SNORM
    } else if four_cc == MAGIC_DX10 || four_cc == MAGIC_XBOX {
        // The enumeration has one byte.
        Dxgi(header_dx10(data).dxgi_format as u8)
    } else if pf.flags & (DDPF_RGB | DDPF_LUMINANCE) != 0 {
        match pf.rgb_bit_count {
            32 => {
                if pf.flags & DDPF_ALPHAPIXELS == 0 {
                    Dxgi::B8G8R8X8_UNORM
                } else if pf.r_bit_mask == 0x0000_00FF {
                    Dxgi::R8G8B8A8_UNORM
                } else {
                    Dxgi::B8G8R8A8_UNORM
                }
            }
            16 => {
                if pf.r_bit_mask == 0xF800 && pf.g_bit_mask == 0x07E0 && pf.b_bit_mask == 0x001F && pf.a_bit_mask == 0 {
                    Dxgi::B5G6R5_UNORM
                } else if pf.r_bit_mask == 0x7C00
                    && pf.g_bit_mask == 0x03E0
                    && pf.b_bit_mask == 0x001F
                    && pf.a_bit_mask == 0x8000
                {
                    Dxgi::B5G5R5A1_UNORM
                } else {
                    Dxgi::R8G8_UNORM
                }
            }
            8 => {
                if pf.flags & DDPF_ALPHA != 0 {
                    Dxgi::A8_UNORM
                } else {
                    Dxgi::R8_UNORM
                }
            }
            _ => Dxgi::UNKNOWN,
        }
    } else {
        Dxgi::UNKNOWN
    }
}

/// Port of `TwbDDS.GetD3DFMT`: the Direct3D 9 format of the pixel format
/// masks (<https://learn.microsoft.com/windows/win32/direct3ddds/dx-graphics-dds-pguide>).
///
/// UPSTREAM-QUIRK: the 32 bit cases test `DDPF_ALPHA` (the alpha only
/// flag), not `DDPF_ALPHAPIXELS`, and `X1R5G5B5` asks for an alpha mask.
pub fn d3dfmt(data: &[u8]) -> D3dFormat {
    let header = DdsHeader::read(data);
    let pf = &header.pixel_format;
    let (r, g, b, a) = (pf.r_bit_mask, pf.g_bit_mask, pf.b_bit_mask, pf.a_bit_mask);
    let mut result = D3dFormat::UNKNOWN;
    if pf.flags & DDPF_RGB != 0 {
        match pf.rgb_bit_count {
            32 => {
                if pf.flags & DDPF_ALPHA != 0 && r == 0xFF0000 && g == 0xFF00 && b == 0xFF && a == 0xFF00_0000 {
                    result = D3dFormat::A8R8G8B8;
                } else if pf.flags & DDPF_ALPHA == 0 && r == 0xFF0000 && g == 0xFF00 && b == 0xFF {
                    result = D3dFormat::X8R8G8B8;
                } else if pf.flags & DDPF_ALPHA == 0 && r == 0xFF && g == 0xFF00 && b == 0xFF0000 {
                    result = D3dFormat::X8B8G8R8;
                } else if pf.flags & DDPF_ALPHA != 0
                    && r == 0x3FF0_0000
                    && g == 0xFFC00
                    && b == 0x3FF
                    && a == 0xC000_0000
                {
                    result = D3dFormat::A2R10G10B10;
                }
            }
            24 => {
                if r == 0xFF0000 && g == 0xFF00 && b == 0xFF && a == 0 {
                    result = D3dFormat::R8G8B8;
                }
            }
            16 => {
                if r == 0x7C00 && g == 0x03E0 && b == 0x001F && a == 0x8000 {
                    result = D3dFormat::X1R5G5B5;
                } else if r == 0xF00 && g == 0xF0 && b == 0xF && a == 0xF000 {
                    result = D3dFormat::A4R4G4B4;
                } else if r == 0xF00 && g == 0xF0 && b == 0xF && a == 0 {
                    result = D3dFormat::X4R4G4B4;
                } else if r == 0xE0 && g == 0x1C && b == 0x3 && a == 0xFF00 {
                    result = D3dFormat::A8R3G3B2;
                }
            }
            _ => {}
        }
    }
    result
}

/// Port of `TwbDDS.SetUpHeader`: writes the header of a DDS file of the
/// format into `data`, which holds `HEADER_SIZE` bytes, then the DX10 header
/// when the format needs one or `xbox` is set, then the XBOX header when
/// `xbox` is set, and room for them is reserved by the caller (zero filled).
pub fn set_up_header(
    data: &mut [u8],
    format: Dxgi,
    width: i32,
    height: i32,
    mip_map_count: i32,
    cube_map: bool,
    xbox: bool,
) {
    let mut header = DdsHeader {
        magic: MAGIC_DDS,
        size: (HEADER_SIZE - 4) as u32,
        width: width as u32,
        height: height as u32,
        flags: DDSD_CAPS | DDSD_PIXELFORMAT | DDSD_WIDTH | DDSD_HEIGHT | DDSD_MIPMAPCOUNT,
        caps: DDSCAPS_TEXTURE,
        depth: 1,
        mip_map_count: mip_map_count as u32,
        ..DdsHeader::default()
    };
    header.pixel_format.size = 32;
    if header.mip_map_count == 0 {
        header.mip_map_count += 1;
    }
    if header.mip_map_count > 1 {
        header.caps |= DDSCAPS_MIPMAP | DDSCAPS_COMPLEX;
    }
    if cube_map {
        // Archive2.exe creates invalid textures like this
        // dwCaps := dwCaps or DDSCAPS2_CUBEMAP or DDSCAPS_COMPLEX or DDSCAPS2_CUBEMAP_ALLFACES
        // this is the correct way
        header.caps |= DDSCAPS_COMPLEX;
        header.caps2 = DDSCAPS2_CUBEMAP | DDSCAPS2_CUBEMAP_ALLFACES;
    }

    // DXGI specific settings.
    let (w, h) = (header.width, header.height);
    let linear = |header: &mut DdsHeader, four_cc: Magic4, size: u32| {
        header.flags |= DDSD_LINEARSIZE;
        header.pixel_format.flags = DDPF_FOURCC;
        header.pixel_format.four_cc = four_cc;
        header.pitch_or_linear_size = size;
    };
    let pitch = |header: &mut DdsHeader, pixel_flags: u32, four_cc: Magic4, size: u32| {
        header.flags |= DDSD_PITCH;
        header.pixel_format.flags = pixel_flags;
        header.pixel_format.four_cc = four_cc;
        header.pitch_or_linear_size = size;
    };
    let masks = |header: &mut DdsHeader, bits: u32, r: u32, g: u32, b: u32, a: u32| {
        header.pixel_format.rgb_bit_count = bits;
        header.pixel_format.r_bit_mask = r;
        header.pixel_format.g_bit_mask = g;
        header.pixel_format.b_bit_mask = b;
        header.pixel_format.a_bit_mask = a;
    };
    let none = [0u8; 4];
    match format {
        Dxgi::BC1_UNORM => linear(&mut header, MAGIC_DXT1, w.wrapping_mul(h) / 2),
        Dxgi::BC2_UNORM => linear(&mut header, MAGIC_DXT3, w.wrapping_mul(h)),
        Dxgi::BC3_UNORM => linear(&mut header, MAGIC_DXT5, w.wrapping_mul(h)),
        Dxgi::BC4_SNORM => linear(&mut header, MAGIC_BC4S, w.wrapping_mul(h) / 2),
        Dxgi::BC4_UNORM => linear(&mut header, MAGIC_BC4U, w.wrapping_mul(h) / 2),
        Dxgi::BC5_SNORM => linear(&mut header, MAGIC_BC5S, w.wrapping_mul(h)),
        Dxgi::BC5_UNORM => linear(&mut header, MAGIC_BC5U, w.wrapping_mul(h)),
        Dxgi::BC1_UNORM_SRGB => linear(&mut header, MAGIC_DX10, w.wrapping_mul(h) / 2),
        Dxgi::BC2_UNORM_SRGB
        | Dxgi::BC3_UNORM_SRGB
        | Dxgi::BC6H_UF16
        | Dxgi::BC6H_SF16
        | Dxgi::BC7_UNORM
        | Dxgi::BC7_UNORM_SRGB => linear(&mut header, MAGIC_DX10, w.wrapping_mul(h)),
        Dxgi::B8G8R8A8_UNORM_SRGB
        | Dxgi::B8G8R8X8_UNORM_SRGB
        | Dxgi::R8G8B8A8_UNORM_SRGB
        | Dxgi::R8G8B8A8_SINT
        | Dxgi::R8G8B8A8_UINT => pitch(&mut header, DDPF_FOURCC, MAGIC_DX10, w.wrapping_mul(4)),
        Dxgi::R8G8B8A8_UNORM => {
            pitch(&mut header, DDPF_RGB | DDPF_ALPHAPIXELS, none, w.wrapping_mul(4));
            masks(&mut header, 32, 0x0000_00FF, 0x0000_FF00, 0x00FF_0000, 0xFF00_0000);
        }
        Dxgi::B8G8R8A8_UNORM => {
            pitch(&mut header, DDPF_RGB | DDPF_ALPHAPIXELS, none, w.wrapping_mul(4));
            masks(&mut header, 32, 0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000);
        }
        Dxgi::B8G8R8X8_UNORM => {
            pitch(&mut header, DDPF_RGB, none, w.wrapping_mul(4));
            masks(&mut header, 32, 0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0);
        }
        Dxgi::B5G6R5_UNORM => {
            pitch(&mut header, DDPF_RGB, none, w.wrapping_mul(2));
            masks(&mut header, 16, 0x0000_F800, 0x0000_07E0, 0x0000_001F, 0);
        }
        Dxgi::B5G5R5A1_UNORM => {
            pitch(&mut header, DDPF_RGB | DDPF_ALPHAPIXELS, none, w.wrapping_mul(2));
            masks(&mut header, 16, 0x0000_7C00, 0x0000_03E0, 0x0000_001F, 0x0000_8000);
        }
        Dxgi::R8G8_UNORM => {
            pitch(&mut header, DDPF_LUMINANCE | DDPF_ALPHAPIXELS, none, w.wrapping_mul(2));
            masks(&mut header, 16, 0x0000_00FF, 0, 0, 0x0000_FF00);
        }
        Dxgi::R8G8_SINT | Dxgi::R8G8_UINT => pitch(&mut header, DDPF_FOURCC, MAGIC_DX10, w.wrapping_mul(2)),
        Dxgi::R8_SINT | Dxgi::R8_SNORM | Dxgi::R8_UINT => pitch(&mut header, DDPF_FOURCC, MAGIC_DX10, w),
        Dxgi::A8_UNORM => {
            pitch(&mut header, DDPF_ALPHA, none, w);
            masks(&mut header, 8, 0, 0, 0, 0x0000_00FF);
        }
        Dxgi::R8_UNORM => {
            pitch(&mut header, DDPF_LUMINANCE, none, w);
            masks(&mut header, 8, 0x0000_00FF, 0, 0, 0);
        }
        // The rest of DXGI formats, unsupported by Bethesda?
        _ => pitch(
            &mut header,
            DDPF_FOURCC,
            MAGIC_DX10,
            w.wrapping_mul(u32::from(bits_per_pixel(format))) >> 3,
        ),
    }
    header.write(data);

    // The additional DX10 header.
    if header.pixel_format.four_cc == MAGIC_DX10 || xbox {
        DdsHeaderDx10 {
            dxgi_format: i32::from(format.0),
            resource_dimension: DDS_DIMENSION_TEXTURE2D,
            array_size: 1,
            misc_flags: if cube_map { DDS_RESOURCE_MISC_TEXTURECUBE } else { 0 },
            ..DdsHeaderDx10::default()
        }
        .write(data, HEADER_SIZE);
    }

    // The additional XBOX header.
    if xbox {
        let mut four_cc_header = DdsHeader::read(data);
        four_cc_header.pixel_format.flags = DDPF_FOURCC;
        four_cc_header.pixel_format.four_cc = MAGIC_XBOX;
        four_cc_header.pixel_format.rgb_bit_count = 0;
        four_cc_header.pixel_format.r_bit_mask = 0;
        four_cc_header.pixel_format.g_bit_mask = 0;
        four_cc_header.pixel_format.b_bit_mask = 0;
        four_cc_header.pixel_format.a_bit_mask = 0;
        four_cc_header.write(data);
        let mut xbox_header = header_xbox(data);
        // Used by Archive2 when extracting xbox textures.
        xbox_header.xdk_ver = 10705;
        xbox_header.write(data, HEADER_SIZE + HEADER_DX10_SIZE);
    }
}

/// Port of `TwbDDS.GetBitsPerPixel` for a format.
pub fn bits_per_pixel(format: Dxgi) -> u8 {
    match format {
        Dxgi::R32G32B32A32_TYPELESS | Dxgi::R32G32B32A32_FLOAT | Dxgi::R32G32B32A32_UINT | Dxgi::R32G32B32A32_SINT => {
            128
        }

        Dxgi::R32G32B32_TYPELESS | Dxgi::R32G32B32_FLOAT | Dxgi::R32G32B32_UINT | Dxgi::R32G32B32_SINT => 96,

        Dxgi::R16G16B16A16_TYPELESS
        | Dxgi::R16G16B16A16_FLOAT
        | Dxgi::R16G16B16A16_UNORM
        | Dxgi::R16G16B16A16_UINT
        | Dxgi::R16G16B16A16_SNORM
        | Dxgi::R16G16B16A16_SINT
        | Dxgi::R32G32_TYPELESS
        | Dxgi::R32G32_FLOAT
        | Dxgi::R32G32_UINT
        | Dxgi::R32G32_SINT
        | Dxgi::R32G8X24_TYPELESS
        | Dxgi::D32_FLOAT_S8X24_UINT
        | Dxgi::R32_FLOAT_X8X24_TYPELESS
        | Dxgi::X32_TYPELESS_G8X24_UINT
        | Dxgi::Y416
        | Dxgi::Y210
        | Dxgi::Y216 => 64,

        Dxgi::R10G10B10A2_TYPELESS
        | Dxgi::R10G10B10A2_UNORM
        | Dxgi::R10G10B10A2_UINT
        | Dxgi::R11G11B10_FLOAT
        | Dxgi::R8G8B8A8_TYPELESS
        | Dxgi::R8G8B8A8_UNORM
        | Dxgi::R8G8B8A8_UNORM_SRGB
        | Dxgi::R8G8B8A8_UINT
        | Dxgi::R8G8B8A8_SNORM
        | Dxgi::R8G8B8A8_SINT
        | Dxgi::R16G16_TYPELESS
        | Dxgi::R16G16_FLOAT
        | Dxgi::R16G16_UNORM
        | Dxgi::R16G16_UINT
        | Dxgi::R16G16_SNORM
        | Dxgi::R16G16_SINT
        | Dxgi::R32_TYPELESS
        | Dxgi::D32_FLOAT
        | Dxgi::R32_FLOAT
        | Dxgi::R32_UINT
        | Dxgi::R32_SINT
        | Dxgi::R24G8_TYPELESS
        | Dxgi::D24_UNORM_S8_UINT
        | Dxgi::R24_UNORM_X8_TYPELESS
        | Dxgi::X24_TYPELESS_G8_UINT
        | Dxgi::R9G9B9E5_SHAREDEXP
        | Dxgi::R8G8_B8G8_UNORM
        | Dxgi::G8R8_G8B8_UNORM
        | Dxgi::B8G8R8A8_UNORM
        | Dxgi::B8G8R8X8_UNORM
        | Dxgi::R10G10B10_XR_BIAS_A2_UNORM
        | Dxgi::B8G8R8A8_TYPELESS
        | Dxgi::B8G8R8A8_UNORM_SRGB
        | Dxgi::B8G8R8X8_TYPELESS
        | Dxgi::B8G8R8X8_UNORM_SRGB
        | Dxgi::AYUV
        | Dxgi::Y410
        | Dxgi::YUY2 => 32,

        Dxgi::P010 | Dxgi::P016 => 24,

        Dxgi::R8G8_TYPELESS
        | Dxgi::R8G8_UNORM
        | Dxgi::R8G8_UINT
        | Dxgi::R8G8_SNORM
        | Dxgi::R8G8_SINT
        | Dxgi::R16_TYPELESS
        | Dxgi::R16_FLOAT
        | Dxgi::D16_UNORM
        | Dxgi::R16_UNORM
        | Dxgi::R16_UINT
        | Dxgi::R16_SNORM
        | Dxgi::R16_SINT
        | Dxgi::B5G6R5_UNORM
        | Dxgi::B5G5R5A1_UNORM
        | Dxgi::A8P8
        | Dxgi::B4G4R4A4_UNORM => 16,

        Dxgi::NV12 | Dxgi::P420_OPAQUE | Dxgi::NV11 => 12,

        Dxgi::R8_TYPELESS
        | Dxgi::R8_UNORM
        | Dxgi::R8_UINT
        | Dxgi::R8_SNORM
        | Dxgi::R8_SINT
        | Dxgi::A8_UNORM
        | Dxgi::BC2_TYPELESS
        | Dxgi::BC2_UNORM
        | Dxgi::BC2_UNORM_SRGB
        | Dxgi::BC3_TYPELESS
        | Dxgi::BC3_UNORM
        | Dxgi::BC3_UNORM_SRGB
        | Dxgi::BC5_TYPELESS
        | Dxgi::BC5_UNORM
        | Dxgi::BC5_SNORM
        | Dxgi::BC6H_TYPELESS
        | Dxgi::BC6H_UF16
        | Dxgi::BC6H_SF16
        | Dxgi::BC7_TYPELESS
        | Dxgi::BC7_UNORM
        | Dxgi::BC7_UNORM_SRGB
        | Dxgi::AI44
        | Dxgi::IA44
        | Dxgi::P8 => 8,

        Dxgi::R1_UNORM => 1,

        Dxgi::BC1_TYPELESS
        | Dxgi::BC1_UNORM
        | Dxgi::BC1_UNORM_SRGB
        | Dxgi::BC4_TYPELESS
        | Dxgi::BC4_UNORM
        | Dxgi::BC4_SNORM => 4,

        _ => 0,
    }
}

/// Port of `TwbDDS.GetBitsPerPixel` for a DDS file.
pub fn bits_per_pixel_of(data: &[u8]) -> u8 {
    bits_per_pixel(dxgi(data))
}

/// Port of `TwbDDS.ConvertR8G8B8toB8G8R8X8`: a 24 bit DDS file as a 32 bit
/// one with an opaque fourth byte per pixel. `data` is the whole file.
///
/// Upstream sizes the result as the file and one more byte per pixel, so a
/// file whose image data is not a multiple of three bytes keeps zeros at the
/// end.
pub fn convert_r8g8b8_to_b8g8r8x8(data: &[u8]) -> Vec<u8> {
    let header_size = header_size(data);
    let pixels = data.len().saturating_sub(header_size) / 3;
    let mut result = vec![0u8; data.len() + pixels];
    result[..header_size].copy_from_slice(&data[..header_size]);
    let mut header = DdsHeader::read(&result);
    header.pixel_format.rgb_bit_count = 32;
    header.pitch_or_linear_size = header.width.wrapping_mul(4);
    header.write(&mut result);
    crate::simd::rgb_to_bgrx(
        &data[header_size..header_size + pixels * 3],
        &mut result[header_size..header_size + pixels * 4],
    );
    result
}

/// The error of a DDS file that cannot be stored (`cExceptionInvalidDDS`).
pub(crate) fn invalid_dds() -> ArchiveError {
    ArchiveError("Not a valid DDS file".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header for the format made by `set_up_header`, with room for the
    /// DX10 and XBOX headers.
    fn header_for(format: Dxgi, width: i32, height: i32, mips: i32, cube: bool, xbox: bool) -> Vec<u8> {
        let mut data = vec![0u8; MAX_HEADER_SIZE];
        set_up_header(&mut data, format, width, height, mips, cube, xbox);
        data.truncate(header_size(&data));
        data
    }

    #[test]
    fn the_names_and_numbers_agree() {
        assert_eq!(dxgi_format_name(Dxgi::BC1_UNORM.0), "BC1_UNORM");
        assert_eq!(dxgi_format_name(Dxgi::BC7_UNORM.0), "BC7_UNORM");
        assert_eq!(dxgi_format_name(Dxgi::P420_OPAQUE.0), "420_OPAQUE");
        assert_eq!(dxgi_format_name(Dxgi::B4G4R4A4_UNORM.0), "B4G4R4A4_UNORM");
        assert_eq!(dxgi_format_name(116), "");
    }

    #[test]
    fn a_header_reads_back_the_format_it_was_made_for() {
        for number in 0..=115u8 {
            let format = Dxgi(number);
            let data = header_for(format, 64, 32, 4, false, false);
            assert!(is_dds(&data), "{}", dxgi_format_name(number));
            // UPSTREAM-QUIRK: the pixel format of A8_UNORM has only the alpha
            // flag, which `GetDXGI` does not read as a format.
            let expected = if format == Dxgi::A8_UNORM {
                Dxgi::UNKNOWN
            } else {
                format
            };
            assert_eq!(dxgi(&data), expected, "{}", dxgi_format_name(number));
            // Only the formats of the DX9 list go without the DX10 header.
            assert_eq!(
                header_size(&data),
                if DXGI_DX9.contains(&format) {
                    HEADER_SIZE
                } else {
                    HEADER_SIZE + HEADER_DX10_SIZE
                },
                "{}",
                dxgi_format_name(number)
            );
            let header = DdsHeader::read(&data);
            assert_eq!((header.width, header.height, header.mip_map_count), (64, 32, 4));
            assert_eq!(header.caps, DDSCAPS_TEXTURE | DDSCAPS_MIPMAP | DDSCAPS_COMPLEX);
        }
    }

    #[test]
    fn block_compressed_headers_carry_the_linear_size() {
        let data = header_for(Dxgi::BC1_UNORM, 256, 128, 1, false, false);
        let header = DdsHeader::read(&data);
        assert_eq!(header.pixel_format.four_cc, MAGIC_DXT1);
        assert_eq!(header.pitch_or_linear_size, 256 * 128 / 2);
        assert_eq!(header.flags & DDSD_LINEARSIZE, DDSD_LINEARSIZE);
        assert_eq!(header.caps, DDSCAPS_TEXTURE);
        let data = header_for(Dxgi::BC7_UNORM, 256, 128, 0, false, false);
        let header = DdsHeader::read(&data);
        assert_eq!(header.pixel_format.four_cc, MAGIC_DX10);
        assert_eq!(header.pitch_or_linear_size, 256 * 128);
        // A mipmap count of 0 is written as 1.
        assert_eq!(header.mip_map_count, 1);
        assert_eq!(header_dx10(&data).dxgi_format, 98);
        assert_eq!(header_dx10(&data).resource_dimension, DDS_DIMENSION_TEXTURE2D);
        assert_eq!(header_dx10(&data).array_size, 1);
    }

    #[test]
    fn cube_maps_and_xbox_files_are_marked() {
        let data = header_for(Dxgi::BC3_UNORM, 64, 64, 1, true, false);
        assert!(is_cube_map(&data));
        assert_eq!(
            DdsHeader::read(&data).caps2,
            DDSCAPS2_CUBEMAP | DDSCAPS2_CUBEMAP_ALLFACES
        );
        let data = header_for(Dxgi::BC7_UNORM, 64, 64, 1, true, false);
        assert_eq!(header_dx10(&data).misc_flags, DDS_RESOURCE_MISC_TEXTURECUBE);

        let data = header_for(Dxgi::BC1_UNORM, 64, 64, 1, false, true);
        assert!(is_xbox(&data));
        assert_eq!(header_size(&data), MAX_HEADER_SIZE);
        assert_eq!(header_xbox(&data).xdk_ver, 10705);
        assert_eq!(dxgi(&data), Dxgi::BC1_UNORM);
        assert_eq!(tile_mode(&data), 0);
        assert_eq!(tile_mode(&header_for(Dxgi::BC1_UNORM, 64, 64, 1, false, false)), 8);
    }

    #[test]
    fn a_file_is_a_dds_file_by_its_magic_and_size() {
        assert!(!is_dds(&[]));
        assert!(!is_dds(&[0u8; 200]));
        let mut data = header_for(Dxgi::BC7_UNORM, 8, 8, 1, false, false);
        assert!(is_dds(&data));
        // The DX10 header must be there.
        data.truncate(HEADER_SIZE + 4);
        assert!(!is_dds(&data));
        data.truncate(HEADER_SIZE - 1);
        assert!(!is_dds(&data));
    }

    #[test]
    fn legacy_formats_are_read_from_the_masks() {
        let mut data = vec![0u8; HEADER_SIZE];
        data[0..4].copy_from_slice(&MAGIC_DDS);
        let mut header = DdsHeader::read(&data);
        header.pixel_format.flags = DDPF_RGB;
        header.pixel_format.rgb_bit_count = 24;
        header.pixel_format.r_bit_mask = 0xFF0000;
        header.pixel_format.g_bit_mask = 0xFF00;
        header.pixel_format.b_bit_mask = 0xFF;
        header.write(&mut data);
        assert_eq!(dxgi(&data), Dxgi::UNKNOWN);
        assert_eq!(d3dfmt(&data), D3dFormat::R8G8B8);
        assert!(D3D_NODXGI.contains(&d3dfmt(&data)));
        assert_eq!(d3dfmt_format_name(d3dfmt(&data)), "R8G8B8");

        // 32 bit with and without alpha.
        header.pixel_format.rgb_bit_count = 32;
        header.pixel_format.flags = DDPF_RGB | DDPF_ALPHAPIXELS;
        header.pixel_format.a_bit_mask = 0xFF00_0000;
        header.write(&mut data);
        assert_eq!(dxgi(&data), Dxgi::B8G8R8A8_UNORM);
        header.pixel_format.r_bit_mask = 0xFF;
        header.pixel_format.b_bit_mask = 0xFF0000;
        header.write(&mut data);
        assert_eq!(dxgi(&data), Dxgi::R8G8B8A8_UNORM);
        header.pixel_format.flags = DDPF_RGB;
        header.write(&mut data);
        assert_eq!(dxgi(&data), Dxgi::B8G8R8X8_UNORM);
        // UPSTREAM-QUIRK: `GetD3DFMT` tests `DDPF_ALPHA`, not `DDPF_ALPHAPIXELS`.
        assert_eq!(d3dfmt(&data), D3dFormat::X8B8G8R8);
    }

    #[test]
    fn four_cc_codes_name_the_block_formats() {
        for (four_cc, format) in [
            (MAGIC_DXT1, Dxgi::BC1_UNORM),
            (MAGIC_DXT3, Dxgi::BC2_UNORM),
            (MAGIC_DXT5, Dxgi::BC3_UNORM),
            (MAGIC_ATI1, Dxgi::BC4_UNORM),
            (MAGIC_BC4U, Dxgi::BC4_UNORM),
            (MAGIC_BC4S, Dxgi::BC4_SNORM),
            (MAGIC_ATI2, Dxgi::BC5_UNORM),
            (MAGIC_BC5U, Dxgi::BC5_UNORM),
            (MAGIC_BC5S, Dxgi::BC5_SNORM),
        ] {
            let mut data = vec![0u8; HEADER_SIZE];
            data[84..88].copy_from_slice(&four_cc);
            assert_eq!(dxgi(&data), format);
            assert!(is_compressed(format));
        }
        assert!(!is_compressed(Dxgi::R8G8B8A8_UNORM));
        assert!(has_alpha(Dxgi::BC3_UNORM) && !has_alpha(Dxgi::BC4_UNORM));
    }

    #[test]
    fn bits_per_pixel_and_mip_size() {
        assert_eq!(bits_per_pixel(Dxgi::BC1_UNORM), 4);
        assert_eq!(bits_per_pixel(Dxgi::BC3_UNORM), 8);
        assert_eq!(bits_per_pixel(Dxgi::R8G8B8A8_UNORM), 32);
        assert_eq!(bits_per_pixel(Dxgi::R32G32B32A32_FLOAT), 128);
        assert_eq!(bits_per_pixel(Dxgi::R1_UNORM), 1);
        assert_eq!(bits_per_pixel(Dxgi::UNKNOWN), 0);
        let data = header_for(Dxgi::BC1_UNORM, 512, 256, 1, false, false);
        assert_eq!(mip_size(&data), 512 * 256 / 2);
        assert_eq!(bits_per_pixel_of(&data), 4);
    }

    #[test]
    fn a_24_bit_file_becomes_a_32_bit_one() {
        let mut data = vec![0u8; HEADER_SIZE];
        data[0..4].copy_from_slice(&MAGIC_DDS);
        let mut header = DdsHeader::read(&data);
        header.width = 3;
        header.height = 1;
        header.mip_map_count = 1;
        header.pixel_format.flags = DDPF_RGB;
        header.pixel_format.rgb_bit_count = 24;
        header.pixel_format.r_bit_mask = 0xFF0000;
        header.pixel_format.g_bit_mask = 0xFF00;
        header.pixel_format.b_bit_mask = 0xFF;
        header.pitch_or_linear_size = 9;
        header.write(&mut data);
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let converted = convert_r8g8b8_to_b8g8r8x8(&data);
        // The file and one more byte for each pixel.
        assert_eq!(converted.len(), data.len() + 3);
        let header = DdsHeader::read(&converted);
        assert_eq!(header.pixel_format.rgb_bit_count, 32);
        assert_eq!(header.pitch_or_linear_size, 12);
        assert_eq!(
            &converted[HEADER_SIZE..],
            &[1, 2, 3, 0xFF, 4, 5, 6, 0xFF, 7, 8, 9, 0xFF]
        );
        assert_eq!(dxgi(&converted), Dxgi::B8G8R8X8_UNORM);

        // Trailing bytes that do not make a pixel stay zero at the end.
        data.push(77);
        let converted = convert_r8g8b8_to_b8g8r8x8(&data);
        assert_eq!(converted.len(), data.len() + 3);
        assert_eq!(converted[converted.len() - 1], 0);
    }
}
