// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/ImagingLib/Source/ImagingFormats.pas (the
// Vampyre Imaging Library by Marek Mauder, at the commit the release links)

//! The pixel formats of the imaging library: their descriptions, the
//! readers and writers of single pixels, the conversions between formats,
//! the DXT and ATI block codecs, the resampling filters and the stretching
//! of images.
//!
//! The arithmetic follows Delphi's Win64 rules, as the release build runs
//! it: `Single` expressions are worked out in double precision and rounded
//! where a `Single` is stored, `Round` is half to even and `Trunc` of a NaN
//! is the integer indefinite.
//!
//! The indexed format (`ifIndex8`) and the color reduction are not ported:
//! no DDS file loads as one and LOD generation never converts to it.

// The loops index the pixels and blocks as upstream's do.
#![allow(clippy::needless_range_loop)]

use super::{ImageData, ImagingError, new_image};
use crate::nif_math;

/// `TImageFormat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum ImageFormat {
    #[default]
    Unknown = 0,
    Default = 1,
    Index8 = 10,
    Gray8 = 40,
    A8Gray8 = 41,
    Gray16 = 42,
    Gray32 = 43,
    Gray64 = 44,
    A16Gray16 = 45,
    X5R1G1B1 = 80,
    R3G3B2 = 81,
    R5G6B5 = 82,
    A1R5G5B5 = 83,
    A4R4G4B4 = 84,
    X1R5G5B5 = 85,
    X4R4G4B4 = 86,
    R8G8B8 = 87,
    A8R8G8B8 = 88,
    X8R8G8B8 = 89,
    R16G16B16 = 90,
    A16R16G16B16 = 91,
    B16G16R16 = 92,
    A16B16G16R16 = 93,
    R32F = 160,
    A32R32G32B32F = 161,
    A32B32G32R32F = 162,
    R16F = 163,
    A16R16G16B16F = 164,
    A16B16G16R16F = 165,
    R32G32B32F = 166,
    B32G32R32F = 167,
    Dxt1 = 200,
    Dxt3 = 201,
    Dxt5 = 202,
    Btc = 203,
    Ati1n = 204,
    Ati2n = 205,
    Binary = 206,
}

impl ImageFormat {
    /// The format of an ordinal (`TImageFormat(i)`), `Unknown` for a value
    /// that names none.
    pub fn from_ordinal(value: i64) -> ImageFormat {
        use ImageFormat::*;
        const ALL: [ImageFormat; 38] = [
            Unknown,
            Default,
            Index8,
            Gray8,
            A8Gray8,
            Gray16,
            Gray32,
            Gray64,
            A16Gray16,
            X5R1G1B1,
            R3G3B2,
            R5G6B5,
            A1R5G5B5,
            A4R4G4B4,
            X1R5G5B5,
            X4R4G4B4,
            R8G8B8,
            A8R8G8B8,
            X8R8G8B8,
            R16G16B16,
            A16R16G16B16,
            B16G16R16,
            A16B16G16R16,
            R32F,
            A32R32G32B32F,
            A32B32G32R32F,
            R16F,
            A16R16G16B16F,
            A16B16G16R16F,
            R32G32B32F,
            B32G32R32F,
            Dxt1,
            Dxt3,
            Dxt5,
            Btc,
            Ati1n,
            Ati2n,
            Binary,
        ];
        ALL.into_iter()
            .find(|format| *format as i64 == value)
            .unwrap_or(Unknown)
    }
}

/// `ChannelBlue` and the other channel indices of a pixel.
pub const CHANNEL_BLUE: usize = 0;
pub const CHANNEL_GREEN: usize = 1;
pub const CHANNEL_RED: usize = 2;
pub const CHANNEL_ALPHA: usize = 3;

/// `TPixelFormatInfo`: the bit layout of a packed format.
#[derive(Debug, Clone, Copy, Default)]
pub struct PixelFormatInfo {
    pub a_bit_mask: u32,
    pub r_bit_mask: u32,
    pub g_bit_mask: u32,
    pub b_bit_mask: u32,
    pub a_bit_count: u32,
    pub r_bit_count: u32,
    pub g_bit_count: u32,
    pub b_bit_count: u32,
    pub a_shift: u32,
    pub r_shift: u32,
    pub g_shift: u32,
    pub b_shift: u32,
    pub a_rec_div: u32,
    pub r_rec_div: u32,
    pub g_rec_div: u32,
    pub b_rec_div: u32,
}

/// `PixelFormat`.
pub const fn pixel_format(a: u32, r: u32, g: u32, b: u32) -> PixelFormatInfo {
    const fn rec_div(bits: u32) -> u32 {
        let value = (1u32 << bits) - 1;
        if value < 1 { 1 } else { value }
    }
    PixelFormatInfo {
        a_bit_mask: ((1 << a) - 1) << (r + g + b),
        r_bit_mask: ((1 << r) - 1) << (g + b),
        g_bit_mask: ((1 << g) - 1) << b,
        b_bit_mask: (1 << b) - 1,
        a_bit_count: a,
        r_bit_count: r,
        g_bit_count: g,
        b_bit_count: b,
        a_shift: r + g + b,
        r_shift: g + b,
        g_shift: b,
        b_shift: 0,
        a_rec_div: rec_div(a),
        r_rec_div: rec_div(r),
        g_rec_div: rec_div(g),
        b_rec_div: rec_div(b),
    }
}

/// `PFSetARGB`.
fn pf_set_argb(pf: &PixelFormatInfo, a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((u32::from(a) << pf.a_bit_count >> 8) << pf.a_shift)
        | ((u32::from(r) << pf.r_bit_count >> 8) << pf.r_shift)
        | ((u32::from(g) << pf.g_bit_count >> 8) << pf.g_shift)
        | ((u32::from(b) << pf.b_bit_count >> 8) << pf.b_shift)
}

/// `PFGetARGB`.
fn pf_get_argb(pf: &PixelFormatInfo, color: u32) -> (u8, u8, u8, u8) {
    (
        ((color & pf.a_bit_mask) >> pf.a_shift)
            .wrapping_mul(255)
            .wrapping_div(pf.a_rec_div) as u8,
        ((color & pf.r_bit_mask) >> pf.r_shift)
            .wrapping_mul(255)
            .wrapping_div(pf.r_rec_div) as u8,
        ((color & pf.g_bit_mask) >> pf.g_shift)
            .wrapping_mul(255)
            .wrapping_div(pf.g_rec_div) as u8,
        ((color & pf.b_bit_mask) << pf.b_shift)
            .wrapping_mul(255)
            .wrapping_div(pf.b_rec_div) as u8,
    )
}

/// How a format reads and writes single pixels (`GetPixel32`,
/// `GetPixelFP` and the setters of `TImageFormatInfo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelAccess {
    Generic,
    A8R8G8B8,
    Channel8Bit,
    Float32,
    /// The special formats have no pixel access.
    None,
}

/// How the size of an image of the format is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeKind {
    Std,
    Dxt,
    Btc,
    Binary,
}

/// `TImageFormatInfo`.
#[derive(Debug, Clone, Copy)]
pub struct FormatInfo {
    pub format: ImageFormat,
    pub name: &'static str,
    pub bytes_per_pixel: usize,
    pub channel_count: usize,
    pub palette_entries: usize,
    pub has_gray_channel: bool,
    pub has_alpha_channel: bool,
    pub is_floating_point: bool,
    pub use_pixel_format: bool,
    pub is_rb_swapped: bool,
    pub rb_swap_format: ImageFormat,
    pub is_indexed: bool,
    pub is_special: bool,
    pub special_nearest_format: ImageFormat,
    pub pixel_format: PixelFormatInfo,
    pub size: SizeKind,
    pub access: PixelAccess,
}

const BASE: FormatInfo = FormatInfo {
    format: ImageFormat::Unknown,
    name: "",
    bytes_per_pixel: 0,
    channel_count: 0,
    palette_entries: 0,
    has_gray_channel: false,
    has_alpha_channel: false,
    is_floating_point: false,
    use_pixel_format: false,
    is_rb_swapped: false,
    rb_swap_format: ImageFormat::Unknown,
    is_indexed: false,
    is_special: false,
    special_nearest_format: ImageFormat::Unknown,
    pixel_format: pixel_format(0, 0, 0, 0),
    size: SizeKind::Std,
    access: PixelAccess::Generic,
};

macro_rules! info {
    ($format:ident, $name:expr, $bpp:expr, $channels:expr $(, $field:ident: $value:expr)*) => {
        FormatInfo {
            format: ImageFormat::$format,
            name: $name,
            bytes_per_pixel: $bpp,
            channel_count: $channels,
            $($field: $value,)*
            ..BASE
        }
    };
}

/// The description of a format (`ImageFormatInfos[Format]`); `None` for
/// `Unknown`. `Default` is `A8R8G8B8`.
pub fn format_info(format: ImageFormat) -> Option<&'static FormatInfo> {
    use ImageFormat::*;
    static INFOS: [FormatInfo; 37] = [
        info!(Index8, "Index8", 1, 1, palette_entries: 256, has_alpha_channel: true, is_indexed: true),
        info!(Gray8, "Gray8", 1, 1, has_gray_channel: true, access: PixelAccess::Channel8Bit),
        info!(A8Gray8, "A8Gray8", 2, 2, has_gray_channel: true, has_alpha_channel: true, access: PixelAccess::Channel8Bit),
        info!(Gray16, "Gray16", 2, 1, has_gray_channel: true),
        info!(Gray32, "Gray32", 4, 1, has_gray_channel: true),
        info!(Gray64, "Gray64", 8, 1, has_gray_channel: true),
        info!(A16Gray16, "A16Gray16", 4, 2, has_gray_channel: true, has_alpha_channel: true),
        info!(X5R1G1B1, "X5R1G1B1", 1, 3, use_pixel_format: true, pixel_format: pixel_format(0, 1, 1, 1)),
        info!(R3G3B2, "R3G3B2", 1, 3, use_pixel_format: true, pixel_format: pixel_format(0, 3, 3, 2)),
        info!(R5G6B5, "R5G6B5", 2, 3, use_pixel_format: true, pixel_format: pixel_format(0, 5, 6, 5)),
        info!(A1R5G5B5, "A1R5G5B5", 2, 4, has_alpha_channel: true, use_pixel_format: true, pixel_format: pixel_format(1, 5, 5, 5)),
        info!(A4R4G4B4, "A4R4G4B4", 2, 4, has_alpha_channel: true, use_pixel_format: true, pixel_format: pixel_format(4, 4, 4, 4)),
        info!(X1R5G5B5, "X1R5G5B5", 2, 3, use_pixel_format: true, pixel_format: pixel_format(0, 5, 5, 5)),
        info!(X4R4G4B4, "X4R4G4B4", 2, 3, use_pixel_format: true, pixel_format: pixel_format(0, 4, 4, 4)),
        info!(R8G8B8, "R8G8B8", 3, 3, access: PixelAccess::Channel8Bit),
        info!(A8R8G8B8, "A8R8G8B8", 4, 4, has_alpha_channel: true, access: PixelAccess::A8R8G8B8),
        info!(X8R8G8B8, "X8R8G8B8", 4, 3, access: PixelAccess::Channel8Bit),
        info!(R16G16B16, "R16G16B16", 6, 3, rb_swap_format: B16G16R16),
        info!(A16R16G16B16, "A16R16G16B16", 8, 4, has_alpha_channel: true, rb_swap_format: A16B16G16R16),
        info!(B16G16R16, "B16G16R16", 6, 3, is_rb_swapped: true, rb_swap_format: R16G16B16),
        info!(A16B16G16R16, "A16B16G16R16", 8, 4, has_alpha_channel: true, is_rb_swapped: true, rb_swap_format: A16R16G16B16),
        info!(R32F, "R32F", 4, 1, is_floating_point: true, access: PixelAccess::Float32),
        info!(A32R32G32B32F, "A32R32G32B32F", 16, 4, has_alpha_channel: true, is_floating_point: true, rb_swap_format: A32B32G32R32F, access: PixelAccess::Float32),
        info!(A32B32G32R32F, "A32B32G32R32F", 16, 4, has_alpha_channel: true, is_floating_point: true, is_rb_swapped: true, rb_swap_format: A32R32G32B32F, access: PixelAccess::Float32),
        info!(R16F, "R16F", 2, 1, is_floating_point: true),
        info!(A16R16G16B16F, "A16R16G16B16F", 8, 4, has_alpha_channel: true, is_floating_point: true, rb_swap_format: A16B16G16R16F),
        info!(A16B16G16R16F, "A16B16G16R16F", 8, 4, has_alpha_channel: true, is_floating_point: true, is_rb_swapped: true, rb_swap_format: A16R16G16B16F),
        info!(R32G32B32F, "R32G32B32F", 12, 3, is_floating_point: true, rb_swap_format: B32G32R32F, access: PixelAccess::Float32),
        info!(B32G32R32F, "B32G32R32F", 12, 3, is_floating_point: true, is_rb_swapped: true, rb_swap_format: R32G32B32F, access: PixelAccess::Float32),
        info!(Dxt1, "DXT1", 0, 4, has_alpha_channel: true, is_special: true, size: SizeKind::Dxt, special_nearest_format: A8R8G8B8, access: PixelAccess::None),
        info!(Dxt3, "DXT3", 0, 4, has_alpha_channel: true, is_special: true, size: SizeKind::Dxt, special_nearest_format: A8R8G8B8, access: PixelAccess::None),
        info!(Dxt5, "DXT5", 0, 4, has_alpha_channel: true, is_special: true, size: SizeKind::Dxt, special_nearest_format: A8R8G8B8, access: PixelAccess::None),
        info!(Btc, "BTC", 0, 1, is_special: true, size: SizeKind::Btc, special_nearest_format: Gray8, access: PixelAccess::None),
        info!(Ati1n, "ATI1N", 0, 1, is_special: true, size: SizeKind::Dxt, special_nearest_format: Gray8, access: PixelAccess::None),
        info!(Ati2n, "ATI2N", 0, 2, is_special: true, size: SizeKind::Dxt, special_nearest_format: A8R8G8B8, access: PixelAccess::None),
        info!(Binary, "Binary", 0, 1, is_special: true, size: SizeKind::Binary, special_nearest_format: Gray8, access: PixelAccess::None),
        // `Unknown` is never looked up through the table.
        BASE,
    ];
    let format = if format == Default { A8R8G8B8 } else { format };
    if format == Unknown {
        return None;
    }
    INFOS.iter().find(|info| info.format == format)
}

/// `GetPixelsSize` of a format.
pub fn pixels_size(info: &FormatInfo, width: i32, height: i32) -> usize {
    let (width, height) = (width.max(0) as usize, height.max(0) as usize);
    match info.size {
        SizeKind::Std => width * height * info.bytes_per_pixel,
        SizeKind::Dxt => {
            let (w, h) = ((width + 3) & !3, (height + 3) & !3);
            if matches!(info.format, ImageFormat::Dxt1 | ImageFormat::Ati1n) {
                w * h / 2
            } else {
                w * h
            }
        }
        SizeKind::Btc => ((width + 3) & !3) * ((height + 3) & !3) / 4,
        SizeKind::Binary => width.div_ceil(8) * height,
    }
}

/// `CheckDimensions` of a format: the dimensions a DXT format rounds up to
/// multiples of four.
pub fn check_dimensions(info: &FormatInfo, width: &mut i32, height: &mut i32) {
    if matches!(info.size, SizeKind::Dxt | SizeKind::Btc) {
        *width = (*width + 3) & !3;
        *height = (*height + 3) & !3;
    }
}

// ---- Delphi number semantics ----

/// `Round` of a double: half to even, the integer indefinite for NaN or a
/// value beyond `Int64`.
pub fn round(value: f64) -> i64 {
    if value.is_nan() || value >= 9_223_372_036_854_775_808.0 || value < -9_223_372_036_854_775_808.0 {
        i64::MIN
    } else {
        value.round_ties_even() as i64
    }
}

/// `Trunc` of a double, as `round`.
pub fn trunc(value: f64) -> i64 {
    if value.is_nan() || value >= 9_223_372_036_854_775_808.0 || value < -9_223_372_036_854_775_808.0 {
        i64::MIN
    } else {
        value.trunc() as i64
    }
}

/// `ClampToByte` (of the `LongInt` a `Round` gives).
pub fn clamp_to_byte(value: i64) -> u8 {
    (value as i32).clamp(0, 255) as u8
}

/// `ClampToWord`.
pub fn clamp_to_word(value: i64) -> u16 {
    (value as i32).clamp(0, 65535) as u16
}

/// `MulDiv` of `ImagingUtility` on words: truncating.
fn mul_div(number: u32, numerator: u32, denominator: u32) -> u16 {
    ((number & 0xFFFF) * numerator / denominator) as u16
}

/// `OneDiv8Bit` and `OneDiv16Bit`: single constants.
const ONE_DIV_8_BIT: f32 = 1.0 / 255.0;
const ONE_DIV_16_BIT: f32 = 1.0 / 65535.0;

/// `GrayConv`.
const GRAY_R: f32 = 0.299;
const GRAY_G: f32 = 0.587;
const GRAY_B: f32 = 0.114;

/// A single times a single constant, stored as a single.
fn mul_single(a: f64, b: f32) -> f32 {
    (a * f64::from(b)) as f32
}

/// `GrayConv.R * R + GrayConv.G * G + GrayConv.B * B` of singles or bytes,
/// in double.
fn gray_sum(r: f64, g: f64, b: f64) -> f64 {
    f64::from(GRAY_R) * r + f64::from(GRAY_G) * g + f64::from(GRAY_B) * b
}

// ---- colors ----

/// `TColor32Rec`: B, G, R, A bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Color32 {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

impl Color32 {
    pub fn from_bytes(bytes: &[u8]) -> Color32 {
        Color32 {
            b: bytes[0],
            g: bytes[1],
            r: bytes[2],
            a: bytes[3],
        }
    }
    pub fn to_bytes(self) -> [u8; 4] {
        [self.b, self.g, self.r, self.a]
    }
}

/// `TColor64Rec`: B, G, R, A words (`Channels[0..3]`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Color64 {
    channels: [u16; 4],
}

/// `TColorFPRec`: B, G, R, A singles (`Channels[0..3]`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ColorFP {
    pub channels: [f32; 4],
}

impl ColorFP {
    pub fn b(&self) -> f32 {
        self.channels[0]
    }
    pub fn g(&self) -> f32 {
        self.channels[1]
    }
    pub fn r(&self) -> f32 {
        self.channels[2]
    }
    pub fn a(&self) -> f32 {
        self.channels[3]
    }
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn write_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn read_f32(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn write_f32(bytes: &mut [u8], at: usize, value: f32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// `ChannelGetSrcPixel`.
fn channel_get_src_pixel(src: &[u8], info: &FormatInfo) -> Color64 {
    let mut pix = Color64::default();
    let scale = |value: u8| mul_div(u32::from(value), 65535, 255);
    match info.bytes_per_pixel {
        1 | 2 => {
            let color = if info.bytes_per_pixel == 1 {
                u32::from(src[0])
            } else {
                u32::from(read_u16(src, 0))
            };
            let (a, r, g, b) = pf_get_argb(&info.pixel_format, color);
            pix.channels = [
                u16::from(b) << 8,
                u16::from(g) << 8,
                u16::from(r) << 8,
                u16::from(a) << 8,
            ];
        }
        3 => {
            pix.channels[0] = scale(src[0]);
            pix.channels[1] = scale(src[1]);
            pix.channels[2] = scale(src[2]);
        }
        4 => {
            pix.channels[3] = scale(src[3]);
            pix.channels[2] = scale(src[2]);
            pix.channels[1] = scale(src[1]);
            pix.channels[0] = scale(src[0]);
        }
        6 => {
            for i in 0..3 {
                pix.channels[i] = read_u16(src, i * 2);
            }
        }
        8 => {
            for i in 0..4 {
                pix.channels[i] = read_u16(src, i * 2);
            }
        }
        _ => {}
    }
    if !info.has_alpha_channel {
        pix.channels[3] = 65535;
    }
    if info.is_rb_swapped {
        pix.channels.swap(0, 2);
    }
    pix
}

/// `ChannelSetDstPixel`.
fn channel_set_dst_pixel(dst: &mut [u8], info: &FormatInfo, pix: Color64) {
    let mut w = pix;
    if info.is_rb_swapped {
        w.channels.swap(0, 2);
    }
    let [b, g, r, a] = w.channels;
    let down = |value: u16| mul_div(u32::from(value), 255, 65535) as u8;
    match info.bytes_per_pixel {
        1 => {
            dst[0] = pf_set_argb(
                &info.pixel_format,
                (a >> 8) as u8,
                (r >> 8) as u8,
                (g >> 8) as u8,
                (b >> 8) as u8,
            ) as u8
        }
        2 => write_u16(
            dst,
            0,
            pf_set_argb(
                &info.pixel_format,
                (a >> 8) as u8,
                (r >> 8) as u8,
                (g >> 8) as u8,
                (b >> 8) as u8,
            ) as u16,
        ),
        3 => {
            dst[2] = down(r);
            dst[1] = down(g);
            dst[0] = down(b);
        }
        4 => {
            dst[3] = down(a);
            dst[2] = down(r);
            dst[1] = down(g);
            dst[0] = down(b);
        }
        6 => {
            for (i, value) in [b, g, r].into_iter().enumerate() {
                write_u16(dst, i * 2, value);
            }
        }
        8 => {
            for (i, value) in [b, g, r, a].into_iter().enumerate() {
                write_u16(dst, i * 2, value);
            }
        }
        _ => {}
    }
}

/// `GrayGetSrcPixel`: the gray value in `channels[3]` (`Gray.A`, the
/// whole 64 bits for an 8 byte format) and the alpha.
fn gray_get_src_pixel(src: &[u8], info: &FormatInfo) -> (Color64, u16) {
    let mut gray = Color64::default();
    let mut alpha = 0u16;
    let scale = |value: u8| mul_div(u32::from(value), 65535, 255);
    match info.bytes_per_pixel {
        1 => gray.channels[3] = scale(src[0]),
        2 => {
            if info.has_alpha_channel {
                alpha = scale(src[1]);
                gray.channels[3] = scale(src[0]);
            } else {
                gray.channels[3] = read_u16(src, 0);
            }
        }
        4 => {
            if info.has_alpha_channel {
                alpha = read_u16(src, 2);
                gray.channels[3] = read_u16(src, 0);
            } else {
                gray.channels[3] = read_u16(src, 2);
                gray.channels[2] = read_u16(src, 0);
            }
        }
        8 => {
            for i in 0..4 {
                gray.channels[i] = read_u16(src, i * 2);
            }
        }
        _ => {}
    }
    if !info.has_alpha_channel {
        alpha = 65535;
    }
    (gray, alpha)
}

/// `GraySetDstPixel`.
fn gray_set_dst_pixel(dst: &mut [u8], info: &FormatInfo, gray: Color64, alpha: u16) {
    let down = |value: u16| mul_div(u32::from(value), 255, 65535) as u8;
    match info.bytes_per_pixel {
        1 => dst[0] = down(gray.channels[3]),
        2 => {
            if info.has_alpha_channel {
                dst[1] = down(alpha);
                dst[0] = down(gray.channels[3]);
            } else {
                write_u16(dst, 0, gray.channels[3]);
            }
        }
        4 => {
            if info.has_alpha_channel {
                write_u16(dst, 2, alpha);
                write_u16(dst, 0, gray.channels[3]);
            } else {
                write_u16(dst, 2, gray.channels[3]);
                write_u16(dst, 0, gray.channels[2]);
            }
        }
        8 => {
            for i in 0..4 {
                write_u16(dst, i * 2, gray.channels[i]);
            }
        }
        _ => {}
    }
}

/// `FloatGetSrcPixel`.
fn float_get_src_pixel(src: &[u8], info: &FormatInfo) -> ColorFP {
    let mut pix = ColorFP::default();
    match info.bytes_per_pixel {
        4 => pix.channels[2] = read_f32(src, 0),
        12 => {
            for i in 0..3 {
                pix.channels[i] = read_f32(src, i * 4);
            }
        }
        16 => {
            for i in 0..4 {
                pix.channels[i] = read_f32(src, i * 4);
            }
        }
        2 => pix.channels[2] = half_to_float(read_u16(src, 0)),
        8 => {
            for i in 0..4 {
                pix.channels[i] = half_to_float(read_u16(src, i * 2));
            }
        }
        _ => {}
    }
    if !info.has_alpha_channel {
        pix.channels[3] = 1.0;
    }
    if info.is_rb_swapped {
        pix.channels.swap(0, 2);
    }
    pix
}

/// `FloatSetDstPixel`.
fn float_set_dst_pixel(dst: &mut [u8], info: &FormatInfo, pix: ColorFP) {
    let mut w = pix;
    if info.is_rb_swapped {
        w.channels.swap(0, 2);
    }
    match info.bytes_per_pixel {
        4 => write_f32(dst, 0, w.channels[2]),
        12 => {
            for i in 0..3 {
                write_f32(dst, i * 4, w.channels[i]);
            }
        }
        16 => {
            for i in 0..4 {
                write_f32(dst, i * 4, w.channels[i]);
            }
        }
        2 => write_u16(dst, 0, float_to_half(w.channels[2])),
        8 => {
            for i in 0..4 {
                write_u16(dst, i * 2, float_to_half(w.channels[i]));
            }
        }
        _ => {}
    }
}

/// `HalfToFloat`.
pub fn half_to_float(half: u16) -> f32 {
    let sign = u32::from(half >> 15);
    let mut exp = i32::from((half & 0x7C00) >> 10);
    let mut mantissa = u32::from(half & 1023);
    let dst = if exp > 0 && exp < 31 {
        exp += 127 - 15;
        (sign << 31) | ((exp as u32) << 23) | (mantissa << 13)
    } else if exp == 0 && mantissa == 0 {
        sign << 31
    } else if exp == 0 {
        while mantissa & 0x400 == 0 {
            mantissa <<= 1;
            exp -= 1;
        }
        exp += 1;
        mantissa &= !0x400;
        exp += 127 - 15;
        (sign << 31) | ((exp as u32) << 23) | (mantissa << 13)
    } else if mantissa == 0 {
        (sign << 31) | 0x7F80_0000
    } else {
        (sign << 31) | 0x7F80_0000 | (mantissa << 13)
    };
    f32::from_bits(dst)
}

/// `FloatToHalf`.
pub fn float_to_half(float: f32) -> u16 {
    let src = float.to_bits();
    let sign = (src >> 31) as i32;
    let mut exp = ((src & 0x7F80_0000) >> 23) as i32 - 127 + 15;
    let mut mantissa = (src & 0x007F_FFFF) as i32;
    if exp > 0 && exp < 30 {
        return ((sign << 15) | (exp << 10) | ((mantissa + 0x1000) >> 13)) as u16;
    }
    if src == 0 {
        return 0;
    }
    if exp <= 0 {
        if exp < -10 {
            0
        } else {
            mantissa = (mantissa | 0x0080_0000) >> (1 - exp);
            if mantissa & 0x1000 > 0 {
                mantissa += 0x2000;
            }
            ((sign << 15) | (mantissa >> 13)) as u16
        }
    } else if exp == 255 - 127 + 15 {
        if mantissa == 0 {
            ((sign << 15) | 0x7C00) as u16
        } else {
            ((sign << 15) | 0x7C00 | (mantissa >> 13)) as u16
        }
    } else {
        if mantissa & 0x1000 > 0 {
            mantissa += 0x2000;
            if mantissa & 0x0080_0000 > 0 {
                mantissa = 0;
                exp += 1;
            }
        }
        if exp > 30 {
            ((sign << 15) | 0x7C00) as u16
        } else {
            ((sign << 15) | (exp << 10) | (mantissa >> 13)) as u16
        }
    }
}

/// `ClampFloatPixel`.
fn clamp_float_pixel(pix: &mut ColorFP) {
    for channel in [3, 2, 1, 0] {
        if pix.channels[channel] > 1.0 {
            pix.channels[channel] = 1.0;
        }
    }
    for channel in [3, 2, 1, 0] {
        if pix.channels[channel] < 0.0 {
            pix.channels[channel] = 0.0;
        }
    }
}

/// `GetPixel32` of a format (`GetPixel32Generic` and the fast readers).
pub fn get_pixel32(src: &[u8], info: &FormatInfo) -> Color32 {
    match info.access {
        PixelAccess::A8R8G8B8 => Color32::from_bytes(src),
        PixelAccess::Channel8Bit => match info.format {
            ImageFormat::R8G8B8 | ImageFormat::X8R8G8B8 => Color32 {
                b: src[0],
                g: src[1],
                r: src[2],
                a: 0xFF,
            },
            _ => {
                let a = if info.has_alpha_channel { src[1] } else { 0xFF };
                Color32 {
                    b: src[0],
                    g: src[0],
                    r: src[0],
                    a,
                }
            }
        },
        _ => get_pixel32_generic(src, info),
    }
}

/// `GetPixel32Generic`.
fn get_pixel32_generic(src: &[u8], info: &FormatInfo) -> Color32 {
    if info.format == ImageFormat::A8R8G8B8 {
        Color32::from_bytes(src)
    } else if info.format == ImageFormat::R8G8B8 {
        Color32 {
            b: src[0],
            g: src[1],
            r: src[2],
            a: 0xFF,
        }
    } else if info.is_floating_point {
        let pix = float_get_src_pixel(src, info);
        let byte = |value: f32| clamp_to_byte(round(f64::from(value) * 255.0));
        Color32 {
            a: byte(pix.a()),
            r: byte(pix.r()),
            g: byte(pix.g()),
            b: byte(pix.b()),
        }
    } else if info.has_gray_channel {
        let (gray, alpha) = gray_get_src_pixel(src, info);
        let value = mul_div(u32::from(gray.channels[3]), 255, 65535) as u8;
        Color32 {
            a: mul_div(u32::from(alpha), 255, 65535) as u8,
            r: value,
            g: value,
            b: value,
        }
    } else {
        let pix = channel_get_src_pixel(src, info);
        let down = |value: u16| mul_div(u32::from(value), 255, 65535) as u8;
        Color32 {
            a: down(pix.channels[3]),
            r: down(pix.channels[2]),
            g: down(pix.channels[1]),
            b: down(pix.channels[0]),
        }
    }
}

/// `SetPixel32` of a format.
pub fn set_pixel32(dst: &mut [u8], info: &FormatInfo, color: Color32) {
    match info.access {
        PixelAccess::A8R8G8B8 => dst[..4].copy_from_slice(&color.to_bytes()),
        PixelAccess::Channel8Bit => match info.format {
            ImageFormat::R8G8B8 | ImageFormat::X8R8G8B8 => {
                dst[0] = color.b;
                dst[1] = color.g;
                dst[2] = color.r;
            }
            _ => {
                if info.has_alpha_channel {
                    dst[1] = color.a;
                }
                dst[0] = round(gray_sum(f64::from(color.r), f64::from(color.g), f64::from(color.b))) as u8;
            }
        },
        _ => set_pixel32_generic(dst, info, color),
    }
}

/// `SetPixel32Generic`.
fn set_pixel32_generic(dst: &mut [u8], info: &FormatInfo, color: Color32) {
    if info.format == ImageFormat::A8R8G8B8 {
        dst[..4].copy_from_slice(&color.to_bytes());
    } else if info.format == ImageFormat::R8G8B8 {
        dst[..3].copy_from_slice(&color.to_bytes()[..3]);
    } else if info.is_floating_point {
        let mut pix = ColorFP::default();
        for (channel, value) in [color.b, color.g, color.r, color.a].into_iter().enumerate() {
            pix.channels[channel] = mul_single(f64::from(value), ONE_DIV_8_BIT);
        }
        float_set_dst_pixel(dst, info, pix);
    } else if info.has_gray_channel {
        let alpha = mul_div(u32::from(color.a), 65535, 255);
        let mut gray = Color64::default();
        let value = round(gray_sum(f64::from(color.r), f64::from(color.g), f64::from(color.b)));
        gray.channels[3] = mul_div(value as u32, 65535, 255);
        gray_set_dst_pixel(dst, info, gray, alpha);
    } else {
        let up = |value: u8| mul_div(u32::from(value), 65535, 255);
        let pix = Color64 {
            channels: [up(color.b), up(color.g), up(color.r), up(color.a)],
        };
        channel_set_dst_pixel(dst, info, pix);
    }
}

/// `GetPixelFP` of a format.
pub fn get_pixel_fp(src: &[u8], info: &FormatInfo) -> ColorFP {
    match info.access {
        PixelAccess::A8R8G8B8 => ColorFP {
            channels: [
                mul_single(f64::from(src[0]), ONE_DIV_8_BIT),
                mul_single(f64::from(src[1]), ONE_DIV_8_BIT),
                mul_single(f64::from(src[2]), ONE_DIV_8_BIT),
                mul_single(f64::from(src[3]), ONE_DIV_8_BIT),
            ],
        },
        PixelAccess::Channel8Bit => match info.format {
            ImageFormat::R8G8B8 | ImageFormat::X8R8G8B8 => ColorFP {
                channels: [
                    mul_single(f64::from(src[0]), ONE_DIV_8_BIT),
                    mul_single(f64::from(src[1]), ONE_DIV_8_BIT),
                    mul_single(f64::from(src[2]), ONE_DIV_8_BIT),
                    1.0,
                ],
            },
            _ => {
                let a = if info.has_alpha_channel {
                    mul_single(f64::from(src[1]), ONE_DIV_8_BIT)
                } else {
                    1.0
                };
                let value = mul_single(f64::from(src[0]), ONE_DIV_8_BIT);
                ColorFP {
                    channels: [value, value, value, a],
                }
            }
        },
        PixelAccess::Float32 => {
            let mut pix = ColorFP::default();
            match info.format {
                ImageFormat::A32R32G32B32F | ImageFormat::A32B32G32R32F => {
                    for i in 0..4 {
                        pix.channels[i] = read_f32(src, i * 4);
                    }
                }
                ImageFormat::R32G32B32F | ImageFormat::B32G32R32F => {
                    pix.channels[3] = 1.0;
                    for i in 0..3 {
                        pix.channels[i] = read_f32(src, i * 4);
                    }
                }
                _ => {
                    pix.channels = [0.0, 0.0, read_f32(src, 0), 1.0];
                }
            }
            if info.is_rb_swapped {
                pix.channels.swap(0, 2);
            }
            pix
        }
        _ => get_pixel_fp_generic(src, info),
    }
}

/// `GetPixelFPGeneric`.
fn get_pixel_fp_generic(src: &[u8], info: &FormatInfo) -> ColorFP {
    if info.is_floating_point {
        float_get_src_pixel(src, info)
    } else if info.has_gray_channel {
        let (gray, alpha) = gray_get_src_pixel(src, info);
        let value = mul_single(f64::from(gray.channels[3]), ONE_DIV_16_BIT);
        ColorFP {
            channels: [value, value, value, mul_single(f64::from(alpha), ONE_DIV_16_BIT)],
        }
    } else {
        let pix = channel_get_src_pixel(src, info);
        let mut result = ColorFP::default();
        for i in 0..4 {
            result.channels[i] = mul_single(f64::from(pix.channels[i]), ONE_DIV_16_BIT);
        }
        result
    }
}

/// `SetPixelFP` of a format.
pub fn set_pixel_fp(dst: &mut [u8], info: &FormatInfo, color: ColorFP) {
    let byte = |value: f32| clamp_to_byte(round(f64::from(value) * 255.0));
    match info.access {
        PixelAccess::A8R8G8B8 => {
            dst[3] = byte(color.a());
            dst[2] = byte(color.r());
            dst[1] = byte(color.g());
            dst[0] = byte(color.b());
        }
        PixelAccess::Channel8Bit => match info.format {
            ImageFormat::R8G8B8 | ImageFormat::X8R8G8B8 => {
                dst[2] = byte(color.r());
                dst[1] = byte(color.g());
                dst[0] = byte(color.b());
            }
            _ => {
                if info.has_alpha_channel {
                    dst[1] = byte(color.a());
                }
                dst[0] = clamp_to_byte(round(
                    gray_sum(f64::from(color.r()), f64::from(color.g()), f64::from(color.b())) * 255.0,
                ));
            }
        },
        PixelAccess::Float32 => {
            let mut w = color;
            match info.format {
                ImageFormat::A32R32G32B32F | ImageFormat::A32B32G32R32F => {
                    for i in 0..4 {
                        write_f32(dst, i * 4, w.channels[i]);
                    }
                }
                ImageFormat::R32G32B32F | ImageFormat::B32G32R32F => {
                    for i in 0..3 {
                        write_f32(dst, i * 4, w.channels[i]);
                    }
                }
                _ => write_f32(dst, 0, w.channels[2]),
            }
            // UPSTREAM-QUIRK: the swap after the write swaps the stored
            // red and blue of the three-channel view, also for `R32F`
            // (which writes past its pixel; the port only swaps formats
            // with three channels or more).
            if info.is_rb_swapped {
                w.channels = [read_f32(dst, 0), 0.0, read_f32(dst, 8), 0.0];
                write_f32(dst, 0, w.channels[2]);
                write_f32(dst, 8, w.channels[0]);
            }
        }
        _ => set_pixel_fp_generic(dst, info, color),
    }
}

/// `SetPixelFPGeneric`.
fn set_pixel_fp_generic(dst: &mut [u8], info: &FormatInfo, color: ColorFP) {
    let word = |value: f32| clamp_to_word(round(f64::from(value) * 65535.0));
    if info.is_floating_point {
        float_set_dst_pixel(dst, info, color);
    } else if info.has_gray_channel {
        let alpha = word(color.a());
        let mut gray = Color64::default();
        gray.channels[3] = clamp_to_word(round(
            gray_sum(f64::from(color.r()), f64::from(color.g()), f64::from(color.b())) * 65535.0,
        ));
        gray_set_dst_pixel(dst, info, gray, alpha);
    } else {
        let pix = Color64 {
            channels: [word(color.b()), word(color.g()), word(color.r()), word(color.a())],
        };
        channel_set_dst_pixel(dst, info, pix);
    }
}

/// The swaps of `SwapChannels` for the formats without a fast path: the
/// floating point formats through their single pixels, a gray format with
/// alpha by its gray value and alpha, the others through their 64 bit
/// pixels.
pub fn swap_channels_general(bits: &mut [u8], info: &FormatInfo, num_pixels: usize, src: usize, dst: usize) {
    let bpp = info.bytes_per_pixel;
    for i in 0..num_pixels {
        let at = i * bpp;
        if info.is_floating_point {
            let mut pix = float_get_src_pixel(&bits[at..], info);
            pix.channels.swap(src, dst);
            float_set_dst_pixel(&mut bits[at..], info, pix);
        } else if info.has_gray_channel && info.has_alpha_channel && (src == CHANNEL_ALPHA || dst == CHANNEL_ALPHA) {
            let (mut gray, alpha) = gray_get_src_pixel(&bits[at..], info);
            let swapped = gray.channels[3];
            gray.channels[3] = alpha;
            gray_set_dst_pixel(&mut bits[at..], info, gray, swapped);
        } else {
            let mut pix = channel_get_src_pixel(&bits[at..], info);
            pix.channels.swap(src, dst);
            channel_set_dst_pixel(&mut bits[at..], info, pix);
        }
    }
}

// ---- conversions ----

/// `ChannelToChannel` and the other converters of `ConvertImage` for the
/// formats that are not special: `num_pixels` pixels from `src` to `dst`.
pub fn convert_pixels(num_pixels: usize, src: &[u8], dst: &mut [u8], src_info: &FormatInfo, dst_info: &FormatInfo) {
    let (sb, db) = (src_info.bytes_per_pixel, dst_info.bytes_per_pixel);
    let pixels = (0..num_pixels).map(|i| (i * sb, i * db));
    if src_info.has_gray_channel {
        if dst_info.has_gray_channel {
            // `GrayToGray`
            if src_info.format == ImageFormat::Gray8 && dst_info.format == ImageFormat::Gray16 {
                for i in 0..num_pixels {
                    write_u16(dst, i * 2, u16::from(src[i]) << 8);
                }
            } else if dst_info.format == ImageFormat::Gray8 && src_info.format == ImageFormat::Gray16 {
                for i in 0..num_pixels {
                    dst[i] = (read_u16(src, i * 2) >> 8) as u8;
                }
            } else {
                for (s, d) in pixels {
                    let (gray, alpha) = gray_get_src_pixel(&src[s..], src_info);
                    gray_set_dst_pixel(&mut dst[d..], dst_info, gray, alpha);
                }
            }
        } else if dst_info.is_floating_point {
            // `GrayToFloat`
            for (s, d) in pixels {
                let (gray, alpha) = gray_get_src_pixel(&src[s..], src_info);
                let value = mul_single(f64::from(gray.channels[3]), ONE_DIV_16_BIT);
                let pix = ColorFP {
                    channels: [value, value, value, mul_single(f64::from(alpha), ONE_DIV_16_BIT)],
                };
                float_set_dst_pixel(&mut dst[d..], dst_info, pix);
            }
        } else if matches!(db, 3 | 4) && src_info.format == ImageFormat::Gray8 {
            // `GrayToChannel`, the fast path
            for (s, d) in pixels {
                dst[d + 2] = src[s];
                dst[d + 1] = src[s];
                dst[d] = src[s];
                if dst_info.has_alpha_channel {
                    dst[d + 3] = 0xFF;
                }
            }
        } else {
            for (s, d) in pixels {
                let (gray, alpha) = gray_get_src_pixel(&src[s..], src_info);
                let value = gray.channels[3];
                let pix = Color64 {
                    channels: [value, value, value, alpha],
                };
                channel_set_dst_pixel(&mut dst[d..], dst_info, pix);
            }
        }
    } else if src_info.is_floating_point {
        if dst_info.has_gray_channel {
            // `FloatToGray`
            for (s, d) in pixels {
                let mut pix = float_get_src_pixel(&src[s..], src_info);
                clamp_float_pixel(&mut pix);
                let alpha = clamp_to_word(round(f64::from(pix.a()) * 65535.0));
                let mut gray = Color64::default();
                gray.channels[3] = clamp_to_word(round(
                    gray_sum(f64::from(pix.r()), f64::from(pix.g()), f64::from(pix.b())) * 65535.0,
                ));
                gray_set_dst_pixel(&mut dst[d..], dst_info, gray, alpha);
            }
        } else if dst_info.is_floating_point {
            for (s, d) in pixels {
                let pix = float_get_src_pixel(&src[s..], src_info);
                float_set_dst_pixel(&mut dst[d..], dst_info, pix);
            }
        } else {
            // `FloatToChannel`
            for (s, d) in pixels {
                let mut pix = float_get_src_pixel(&src[s..], src_info);
                clamp_float_pixel(&mut pix);
                let mut pix64 = Color64::default();
                for i in 0..4 {
                    pix64.channels[i] = clamp_to_word(round(f64::from(pix.channels[i]) * 65535.0));
                }
                channel_set_dst_pixel(&mut dst[d..], dst_info, pix64);
            }
        }
    } else if dst_info.has_gray_channel {
        // `ChannelToGray`
        if matches!(sb, 3 | 4) && dst_info.format == ImageFormat::Gray8 {
            for (s, d) in pixels {
                dst[d] = round(gray_sum(
                    f64::from(src[s + 2]),
                    f64::from(src[s + 1]),
                    f64::from(src[s]),
                )) as u8;
            }
        } else {
            for (s, d) in pixels {
                let mut pix = channel_get_src_pixel(&src[s..], src_info);
                let alpha = pix.channels[3];
                pix.channels[3] = round(gray_sum(
                    f64::from(pix.channels[2]),
                    f64::from(pix.channels[1]),
                    f64::from(pix.channels[0]),
                )) as u16;
                gray_set_dst_pixel(&mut dst[d..], dst_info, pix, alpha);
            }
        }
    } else if dst_info.is_floating_point {
        // `ChannelToFloat`
        for (s, d) in pixels {
            let pix = channel_get_src_pixel(&src[s..], src_info);
            let mut fp = ColorFP::default();
            for i in 0..4 {
                fp.channels[i] = mul_single(f64::from(pix.channels[i]), ONE_DIV_16_BIT);
            }
            float_set_dst_pixel(&mut dst[d..], dst_info, fp);
        }
    } else if sb == 3 && db == 4 {
        // `ChannelToChannel`, the two fast paths
        for (s, d) in pixels {
            dst[d..d + 3].copy_from_slice(&src[s..s + 3]);
            if dst_info.has_alpha_channel {
                dst[d + 3] = 255;
            }
        }
    } else if sb == 4 && db == 3 {
        for (s, d) in pixels {
            dst[d..d + 3].copy_from_slice(&src[s..s + 3]);
        }
    } else {
        for (s, d) in pixels {
            let pix = channel_get_src_pixel(&src[s..], src_info);
            channel_set_dst_pixel(&mut dst[d..], dst_info, pix);
        }
    }
}

// ---- DXT and ATI blocks ----

/// `DecodeCol`.
fn decode_col(color: u16) -> Color32 {
    Color32 {
        a: 0xFF,
        r: ((u32::from(color >> 11)) * 255 / 31) as u8,
        g: ((u32::from((color >> 5) & 0x3F)) * 255 / 63) as u8,
        b: ((u32::from(color & 0x1F)) * 255 / 31) as u8,
    }
}

/// The two interpolated colors of four color blocks.
fn interpolate4(colors: &mut [Color32; 4]) {
    let (c0, c1) = (colors[0], colors[1]);
    let third = |a: u8, b: u8| ((u32::from(a) << 1) + u32::from(b) + 1) / 3;
    colors[2].r = third(c0.r, c1.r) as u8;
    colors[2].g = third(c0.g, c1.g) as u8;
    colors[2].b = third(c0.b, c1.b) as u8;
    colors[3].r = third(c1.r, c0.r) as u8;
    colors[3].g = third(c1.g, c0.g) as u8;
    colors[3].b = third(c1.b, c0.b) as u8;
}

fn write_color(dst: &mut [u8], index: usize, color: Color32) {
    dst[index * 4..index * 4 + 4].copy_from_slice(&color.to_bytes());
}

/// `DecodeDXT1`.
pub fn decode_dxt1(src: &[u8], dst: &mut [u8], width: i32, height: i32) {
    let (w, h) = (width as usize, height as usize);
    let mut at = 0;
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            let color0 = read_u16(src, at);
            let color1 = read_u16(src, at + 2);
            let mask = u32::from_le_bytes(src[at + 4..at + 8].try_into().unwrap());
            at += 8;
            let mut colors = [Color32::default(); 4];
            colors[0] = decode_col(color0);
            colors[1] = decode_col(color1);
            if color0 > color1 {
                interpolate4(&mut colors);
                colors[2].a = 0xFF;
                colors[3].a = 0xFF;
            } else {
                let (c0, c1) = (colors[0], colors[1]);
                colors[2] = Color32 {
                    a: 0xFF,
                    r: ((u32::from(c0.r) + u32::from(c1.r)) >> 1) as u8,
                    g: ((u32::from(c0.g) + u32::from(c1.g)) >> 1) as u8,
                    b: ((u32::from(c0.b) + u32::from(c1.b)) >> 1) as u8,
                };
                let third = |a: u8, b: u8| ((u32::from(a) + (u32::from(b) << 1) + 1) / 3) as u8;
                colors[3] = Color32 {
                    a: 0,
                    r: third(c0.r, c1.r),
                    g: third(c0.g, c1.g),
                    b: third(c0.b, c1.b),
                };
            }
            let mut k = 0;
            for j in 0..4 {
                for i in 0..4 {
                    let sel = ((mask >> (k * 2)) & 3) as usize;
                    if x * 4 + i < w && y * 4 + j < h {
                        write_color(dst, (y * 4 + j) * w + x * 4 + i, colors[sel]);
                    }
                    k += 1;
                }
            }
        }
    }
}

/// `DecodeDXT3`.
pub fn decode_dxt3(src: &[u8], dst: &mut [u8], width: i32, height: i32) {
    let (w, h) = (width as usize, height as usize);
    let mut at = 0;
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            let alphas: [u16; 4] = std::array::from_fn(|i| read_u16(src, at + i * 2));
            at += 8;
            let color0 = read_u16(src, at);
            let color1 = read_u16(src, at + 2);
            let mask = u32::from_le_bytes(src[at + 4..at + 8].try_into().unwrap());
            at += 8;
            let mut colors = [Color32::default(); 4];
            colors[0] = decode_col(color0);
            colors[1] = decode_col(color1);
            interpolate4(&mut colors);
            let mut k = 0;
            for j in 0..4 {
                let mut aword = alphas[j];
                for i in 0..4 {
                    let sel = ((mask >> (k * 2)) & 3) as usize;
                    if x * 4 + i < w && y * 4 + j < h {
                        let a = (aword & 0x0F) as u8;
                        colors[sel].a = a | (a << 4);
                        write_color(dst, (y * 4 + j) * w + x * 4 + i, colors[sel]);
                    }
                    k += 1;
                    aword >>= 4;
                }
            }
        }
    }
}

/// `GetInterpolatedAlphas`.
fn interpolated_alphas(alphas: &mut [u8; 8]) {
    let (a0, a1) = (u32::from(alphas[0]), u32::from(alphas[1]));
    if a0 > a1 {
        for i in 0..6u32 {
            alphas[2 + i as usize] = (((6 - i) * a0 + (1 + i) * a1 + 3) / 7) as u8;
        }
    } else {
        for i in 0..4u32 {
            alphas[2 + i as usize] = (((4 - i) * a0 + (1 + i) * a1 + 2) / 5) as u8;
        }
        alphas[6] = 0;
        alphas[7] = 0xFF;
    }
}

/// The two 24 bit index masks of an interpolated alpha block.
fn alpha_masks(block: &[u8]) -> [u32; 2] {
    let read = |at: usize| u32::from(block[at]) | (u32::from(block[at + 1]) << 8) | (u32::from(block[at + 2]) << 16);
    [read(2), read(5)]
}

/// `DecodeDXT5`.
pub fn decode_dxt5(src: &[u8], dst: &mut [u8], width: i32, height: i32) {
    let (w, h) = (width as usize, height as usize);
    let mut at = 0;
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            let mut alphas: [u8; 8] = src[at..at + 8].try_into().unwrap();
            let mut amask = alpha_masks(&alphas);
            at += 8;
            let color0 = read_u16(src, at);
            let color1 = read_u16(src, at + 2);
            let mask = u32::from_le_bytes(src[at + 4..at + 8].try_into().unwrap());
            at += 8;
            let mut colors = [Color32::default(); 4];
            colors[0] = decode_col(color0);
            colors[1] = decode_col(color1);
            interpolate4(&mut colors);
            interpolated_alphas(&mut alphas);
            let mut k = 0;
            for j in 0..4 {
                for i in 0..4 {
                    let sel = ((mask >> (k * 2)) & 3) as usize;
                    if x * 4 + i < w && y * 4 + j < h {
                        colors[sel].a = alphas[(amask[j >> 1] & 7) as usize];
                        write_color(dst, (y * 4 + j) * w + x * 4 + i, colors[sel]);
                    }
                    k += 1;
                    amask[j >> 1] >>= 3;
                }
            }
        }
    }
}

/// `DecodeATI1N`.
pub fn decode_ati1n(src: &[u8], dst: &mut [u8], width: i32, height: i32) {
    let (w, h) = (width as usize, height as usize);
    let mut at = 0;
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            let mut alphas: [u8; 8] = src[at..at + 8].try_into().unwrap();
            let mut amask = alpha_masks(&alphas);
            at += 8;
            interpolated_alphas(&mut alphas);
            for j in 0..4 {
                for i in 0..4 {
                    dst[(y * 4 + j) * w + x * 4 + i] = alphas[(amask[j >> 1] & 7) as usize];
                    amask[j >> 1] >>= 3;
                }
            }
        }
    }
}

/// `DecodeATI2N`.
pub fn decode_ati2n(src: &[u8], dst: &mut [u8], width: i32, height: i32) {
    let (w, h) = (width as usize, height as usize);
    let mut at = 0;
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            let mut alphas1: [u8; 8] = src[at..at + 8].try_into().unwrap();
            let mut amask1 = alpha_masks(&alphas1);
            at += 8;
            let mut alphas2: [u8; 8] = src[at..at + 8].try_into().unwrap();
            let mut amask2 = alpha_masks(&alphas2);
            at += 8;
            interpolated_alphas(&mut alphas1);
            interpolated_alphas(&mut alphas2);
            for j in 0..4 {
                for i in 0..4 {
                    let color = Color32 {
                        a: 0xFF,
                        b: 0,
                        r: alphas1[(amask1[j >> 1] & 7) as usize],
                        g: alphas2[(amask2[j >> 1] & 7) as usize],
                    };
                    write_color(dst, (y * 4 + j) * w + x * 4 + i, color);
                    amask1[j >> 1] >>= 3;
                    amask2[j >> 1] >>= 3;
                }
            }
        }
    }
}

/// `DecodeBTC`.
pub fn decode_btc(src: &[u8], dst: &mut [u8], width: i32, height: i32) {
    let (w, h) = (width as usize, height as usize);
    let mut at = 0;
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            let (lower, upper) = (src[at], src[at + 1]);
            let bits = read_u16(src, at + 2);
            at += 4;
            for j in 0..4 {
                for i in 0..4 {
                    let bit = (bits >> (j * 4 + i)) & 1;
                    dst[(y * 4 + j) * w + x * 4 + i] = if bit != 0 { upper } else { lower };
                }
            }
        }
    }
}

/// `TPixelInfo` of a 4x4 block.
#[derive(Clone, Copy, Default)]
struct PixelInfo {
    color: u16,
    alpha: u8,
    orig: Color32,
}

/// `GetBlock`.
fn get_block(src: &[u8], x_pos: usize, y_pos: usize, width: usize) -> [PixelInfo; 16] {
    let mut block = [PixelInfo::default(); 16];
    let mut i = 0;
    for y in 0..4 {
        for x in 0..4 {
            let index = (y_pos * 4 + y) * width + x_pos * 4 + x;
            let pixel = Color32::from_bytes(&src[index * 4..index * 4 + 4]);
            block[i] = PixelInfo {
                color: ((u16::from(pixel.r) >> 3) << 11) | ((u16::from(pixel.g) >> 2) << 5) | (u16::from(pixel.b) >> 3),
                alpha: pixel.a,
                orig: pixel,
            };
            i += 1;
        }
    }
    block
}

/// `ColorDistance`.
fn color_distance(c1: Color32, c2: Color32) -> i32 {
    let d = |a: u8, b: u8| i32::from(a) - i32::from(b);
    d(c1.r, c2.r) * d(c1.r, c2.r) + d(c1.g, c2.g) * d(c1.g, c2.g) + d(c1.b, c2.b) * d(c1.b, c2.b)
}

/// `GetEndpoints`: the 565 colors of the two pixels farthest apart.
fn get_endpoints(block: &[PixelInfo; 16]) -> (u16, u16) {
    let mut farthest = -1;
    let (mut ep0, mut ep1) = (0, 0);
    for i in 0..16 {
        for j in i + 1..16 {
            let dist = color_distance(block[i].orig, block[j].orig);
            if dist > farthest {
                farthest = dist;
                ep0 = block[i].color;
                ep1 = block[j].color;
            }
        }
    }
    (ep0, ep1)
}

/// `GetAlphaEndpoints`: the lowest and the highest alpha.
fn get_alpha_endpoints(block: &[PixelInfo; 16]) -> (u8, u8) {
    let mut min = 255;
    let mut max = 0;
    for pixel in block {
        if pixel.alpha < min {
            min = pixel.alpha;
        }
        if pixel.alpha > max {
            max = pixel.alpha;
        }
    }
    (min, max)
}

/// `FixEndpoints`.
fn fix_endpoints(ep0: &mut u16, ep1: &mut u16, has_alpha: bool) {
    if has_alpha {
        if *ep0 > *ep1 {
            std::mem::swap(ep0, ep1);
        }
    } else if *ep0 < *ep1 {
        std::mem::swap(ep0, ep1);
    }
}

/// `GetColorMask`.
fn get_color_mask(ep0: u16, ep1: u16, num_cols: usize, block: &[PixelInfo; 16]) -> u32 {
    let mut colors = [Color32::default(); 4];
    colors[0] = decode_col(ep0);
    colors[1] = decode_col(ep1);
    if num_cols == 3 {
        let (c0, c1) = (colors[0], colors[1]);
        let half = |a: u8, b: u8| ((u32::from(a) + u32::from(b)) >> 1) as u8;
        for target in 2..4 {
            colors[target].r = half(c0.r, c1.r);
            colors[target].g = half(c0.g, c1.g);
            colors[target].b = half(c0.b, c1.b);
        }
    } else {
        interpolate4(&mut colors);
    }
    let mut mask = [0u8; 16];
    for i in 0..16 {
        if block[i].alpha < 128 && num_cols == 3 {
            mask[i] = 3;
            continue;
        }
        let mut closest = i32::MAX;
        for (j, color) in colors.iter().enumerate().take(num_cols) {
            let dist = color_distance(block[i].orig, *color);
            if dist < closest {
                closest = dist;
                mask[i] = j as u8;
            }
        }
    }
    let mut result = 0u32;
    for (i, value) in mask.iter().enumerate() {
        result |= u32::from(*value) << (i * 2);
    }
    result
}

/// `GetAlphaMask`: the six bytes of indices of an interpolated alpha block.
fn get_alpha_mask(ep0: u8, ep1: u8, block: &[PixelInfo; 16]) -> [u8; 6] {
    let mut alphas = [0u8; 8];
    alphas[0] = ep0;
    alphas[1] = ep1;
    let (a0, a1) = (u32::from(ep0), u32::from(ep1));
    for i in 0..6u32 {
        alphas[2 + i as usize] = (((6 - i) * a0 + (1 + i) * a1 + 3) / 7) as u8;
    }
    let mut m = [0u8; 16];
    for i in 0..16 {
        let mut closest = i32::MAX;
        for (j, alpha) in alphas.iter().enumerate() {
            let dist = (i32::from(*alpha) - i32::from(block[i].alpha)).abs();
            if dist < closest {
                closest = dist;
                m[i] = j as u8;
            }
        }
    }
    [
        m[0] | (m[1] << 3) | ((m[2] & 3) << 6),
        ((m[2] & 4) >> 2) | (m[3] << 1) | (m[4] << 4) | ((m[5] & 1) << 7),
        ((m[5] & 6) >> 1) | (m[6] << 2) | (m[7] << 5),
        m[8] | (m[9] << 3) | ((m[10] & 3) << 6),
        ((m[10] & 4) >> 2) | (m[11] << 1) | (m[12] << 4) | ((m[13] & 1) << 7),
        ((m[13] & 6) >> 1) | (m[14] << 2) | (m[15] << 5),
    ]
}

fn write_color_block(dst: &mut Vec<u8>, ep0: u16, ep1: u16, mask: u32) {
    dst.extend_from_slice(&ep0.to_le_bytes());
    dst.extend_from_slice(&ep1.to_le_bytes());
    dst.extend_from_slice(&mask.to_le_bytes());
}

/// `EncodeDXT1`.
pub fn encode_dxt1(src: &[u8], width: i32, height: i32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut dst = Vec::with_capacity(w * h / 2);
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            let pixels = get_block(src, x, y, w);
            let has_alpha = pixels.iter().any(|pixel| pixel.alpha < 128);
            let (mut ep0, mut ep1) = get_endpoints(&pixels);
            fix_endpoints(&mut ep0, &mut ep1, has_alpha);
            let mask = get_color_mask(ep0, ep1, if has_alpha { 3 } else { 4 }, &pixels);
            write_color_block(&mut dst, ep0, ep1, mask);
        }
    }
    dst
}

/// `EncodeDXT3`.
pub fn encode_dxt3(src: &[u8], width: i32, height: i32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut dst = Vec::with_capacity(w * h);
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            let pixels = get_block(src, x, y, w);
            for i in 0..8 {
                dst.push((pixels[i * 2].alpha >> 4) | ((pixels[i * 2 + 1].alpha >> 4) << 4));
            }
            let (mut ep0, mut ep1) = get_endpoints(&pixels);
            fix_endpoints(&mut ep0, &mut ep1, false);
            let mask = get_color_mask(ep0, ep1, 4, &pixels);
            write_color_block(&mut dst, ep0, ep1, mask);
        }
    }
    dst
}

/// `EncodeDXT5`.
pub fn encode_dxt5(src: &[u8], width: i32, height: i32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut dst = Vec::with_capacity(w * h);
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            let pixels = get_block(src, x, y, w);
            let (mut ep0, mut ep1) = get_endpoints(&pixels);
            fix_endpoints(&mut ep0, &mut ep1, false);
            let mask = get_color_mask(ep0, ep1, 4, &pixels);
            // `GetAlphaEndPoints(Pixels, Alphas[1], Alphas[0])`: the
            // highest alpha first.
            let (min, max) = get_alpha_endpoints(&pixels);
            dst.push(max);
            dst.push(min);
            dst.extend_from_slice(&get_alpha_mask(max, min, &pixels));
            write_color_block(&mut dst, ep0, ep1, mask);
        }
    }
    dst
}

/// `GetOneChannelBlock`: one channel of a 4x4 block in the alphas.
fn get_one_channel_block(
    src: &[u8],
    x_pos: usize,
    y_pos: usize,
    width: usize,
    bpp: usize,
    channel: usize,
) -> [PixelInfo; 16] {
    let mut block = [PixelInfo::default(); 16];
    let mut i = 0;
    for y in 0..4 {
        for x in 0..4 {
            block[i].alpha = src[(y_pos * 4 + y) * width * bpp + (x_pos * 4 + x) * bpp + channel];
            i += 1;
        }
    }
    block
}

fn push_alpha_block(dst: &mut Vec<u8>, pixels: &[PixelInfo; 16]) {
    let (min, max) = get_alpha_endpoints(pixels);
    dst.push(max);
    dst.push(min);
    dst.extend_from_slice(&get_alpha_mask(max, min, pixels));
}

/// `EncodeATI1N`.
pub fn encode_ati1n(src: &[u8], width: i32, height: i32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut dst = Vec::with_capacity(w * h / 2);
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            push_alpha_block(&mut dst, &get_one_channel_block(src, x, y, w, 1, 0));
        }
    }
    dst
}

/// `EncodeATI2N`.
pub fn encode_ati2n(src: &[u8], width: i32, height: i32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut dst = Vec::with_capacity(w * h);
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            push_alpha_block(&mut dst, &get_one_channel_block(src, x, y, w, 4, CHANNEL_RED));
            push_alpha_block(&mut dst, &get_one_channel_block(src, x, y, w, 4, CHANNEL_GREEN));
        }
    }
    dst
}

/// `EncodeBTC`.
pub fn encode_btc(src: &[u8], width: i32, height: i32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut dst = Vec::with_capacity(w * h / 4);
    for y in 0..h / 4 {
        for x in 0..w / 4 {
            let mut pixels = [0u8; 16];
            let mut m: i32 = 0;
            for j in 0..4 {
                for i in 0..4 {
                    pixels[j * 4 + i] = src[(y * 4 + j) * w + x * 4 + i];
                    m += i32::from(pixels[j * 4 + i]);
                }
            }
            m /= 16;
            let (mut lower, mut upper, mut k) = (0i32, 0i32, 0i32);
            let mut bits = 0u16;
            for (i, pixel) in pixels.iter().enumerate() {
                if i32::from(*pixel) > m {
                    upper += i32::from(*pixel);
                    k += 1;
                    bits |= 1 << i;
                } else {
                    lower += i32::from(*pixel);
                }
            }
            if k > 0 {
                upper /= k;
            }
            if k < 16 {
                lower /= 16 - k;
            }
            dst.push(lower as u8);
            dst.push(upper as u8);
            dst.extend_from_slice(&bits.to_le_bytes());
        }
    }
    dst
}

/// `SpecialToUnSpecial`: the pixels of a special image in its nearest
/// format, written into `dst`.
pub fn special_to_unspecial(image: &ImageData, dst: &mut [u8]) -> Result<(), ImagingError> {
    let (w, h) = (image.width, image.height);
    match image.format {
        ImageFormat::Dxt1 => decode_dxt1(&image.bits, dst, w, h),
        ImageFormat::Dxt3 => decode_dxt3(&image.bits, dst, w, h),
        ImageFormat::Dxt5 => decode_dxt5(&image.bits, dst, w, h),
        ImageFormat::Btc => decode_btc(&image.bits, dst, w, h),
        ImageFormat::Ati1n => decode_ati1n(&image.bits, dst, w, h),
        ImageFormat::Ati2n => decode_ati2n(&image.bits, dst, w, h),
        _ => return Err(ImagingError::new("Binary images are not supported")),
    }
    Ok(())
}

/// `UnSpecialToSpecial`: the pixels of `src` (in the nearest format)
/// encoded as the special format of `image`, which must have the size.
pub fn unspecial_to_special(src: &[u8], image: &mut ImageData) -> Result<(), ImagingError> {
    let (w, h) = (image.width, image.height);
    let encoded = match image.format {
        ImageFormat::Dxt1 => encode_dxt1(src, w, h),
        ImageFormat::Dxt3 => encode_dxt3(src, w, h),
        ImageFormat::Dxt5 => encode_dxt5(src, w, h),
        ImageFormat::Btc => encode_btc(src, w, h),
        ImageFormat::Ati1n => encode_ati1n(src, w, h),
        ImageFormat::Ati2n => encode_ati2n(src, w, h),
        _ => return Err(ImagingError::new("Binary images are not supported")),
    };
    let len = encoded.len().min(image.bits.len());
    image.bits[..len].copy_from_slice(&encoded[..len]);
    Ok(())
}

/// `ConvertSpecial`.
pub fn convert_special(
    image: &mut ImageData,
    src_info: &FormatInfo,
    dst_info: &FormatInfo,
) -> Result<(), ImagingError> {
    let check_size = |img: &mut ImageData| -> Result<(), ImagingError> {
        let (mut width, mut height) = (img.width, img.height);
        check_dimensions(dst_info, &mut width, &mut height);
        super::resize_image(img, width, height, super::ResizeFilter::Nearest)?;
        Ok(())
    };
    if src_info.is_special && dst_info.is_special {
        let mut work = new_image(image.width, image.height, src_info.special_nearest_format)?;
        special_to_unspecial(image, &mut work.bits)?;
        if src_info.special_nearest_format != dst_info.special_nearest_format {
            super::convert_image(&mut work, dst_info.special_nearest_format)?;
        }
        check_size(&mut work)?;
        let mut result = new_image(work.width, work.height, dst_info.format)?;
        unspecial_to_special(&work.bits, &mut result)?;
        *image = result;
    } else if src_info.is_special {
        let mut work = new_image(image.width, image.height, src_info.special_nearest_format)?;
        special_to_unspecial(image, &mut work.bits)?;
        super::convert_image(&mut work, dst_info.format)?;
        *image = work;
    } else if dst_info.is_special {
        let mut work = std::mem::take(image);
        super::convert_image(&mut work, dst_info.special_nearest_format)?;
        check_size(&mut work)?;
        let mut result = new_image(work.width, work.height, dst_info.format)?;
        unspecial_to_special(&work.bits, &mut result)?;
        *image = result;
    }
    Ok(())
}

// ---- resampling ----

/// `TSamplingFilter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingFilter {
    Nearest,
    Linear,
    Cosine,
    Hermite,
    Quadratic,
    Gaussian,
    Spline,
    Lanczos,
    Mitchell,
    CatmullRom,
}

impl SamplingFilter {
    /// `SamplingFilterRadii`.
    fn radius(self) -> f32 {
        match self {
            SamplingFilter::Nearest | SamplingFilter::Linear | SamplingFilter::Cosine | SamplingFilter::Hermite => 1.0,
            SamplingFilter::Quadratic => 1.5,
            SamplingFilter::Gaussian => 1.25,
            SamplingFilter::Spline | SamplingFilter::Mitchell | SamplingFilter::CatmullRom => 2.0,
            SamplingFilter::Lanczos => 3.0,
        }
    }

    /// `SamplingFilterFunctions`: the filter of a `Single` argument.
    fn apply(self, value: f32) -> f32 {
        let v = f64::from(value);
        match self {
            SamplingFilter::Nearest => {
                if value > -0.5 && value <= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            SamplingFilter::Linear => {
                let value = value.abs();
                if value < 1.0 {
                    (1.0 - f64::from(value)) as f32
                } else {
                    0.0
                }
            }
            SamplingFilter::Cosine => {
                if value.abs() < 1.0 {
                    ((nif_math::cos(v * std::f64::consts::PI) + 1.0) / 2.0) as f32
                } else {
                    0.0
                }
            }
            SamplingFilter::Hermite => {
                let v = f64::from(value.abs());
                if v < 1.0 {
                    ((2.0 * v - 3.0) * (v * v) + 1.0) as f32
                } else {
                    0.0
                }
            }
            SamplingFilter::Quadratic => {
                let mut v = f64::from(value.abs());
                if v < 0.5 {
                    (0.75 - v * v) as f32
                } else if v < 1.5 {
                    v = f64::from((v - 1.5) as f32);
                    (0.5 * (v * v)) as f32
                } else {
                    0.0
                }
            }
            SamplingFilter::Gaussian => ((-2.0 * (v * v)).exp() * (2.0 / std::f64::consts::PI).sqrt()) as f32,
            SamplingFilter::Spline => {
                let v = f64::from(value.abs());
                if v < 1.0 {
                    let temp = f64::from((v * v) as f32);
                    (0.5 * temp * v - temp + 2.0 / 3.0) as f32
                } else if v < 2.0 {
                    let v = f64::from((2.0 - v) as f32);
                    ((v * v) * v / 6.0) as f32
                } else {
                    0.0
                }
            }
            SamplingFilter::Lanczos => filter_lanczos(value),
            SamplingFilter::Mitchell => {
                const B: f64 = 1.0 / 3.0;
                const C: f64 = 1.0 / 3.0;
                let v = f64::from(value.abs());
                let temp = f64::from((v * v) as f32);
                if v < 1.0 {
                    let v = f64::from(
                        (((12.0 - 9.0 * B - 6.0 * C) * (v * temp))
                            + ((-18.0 + 12.0 * B + 6.0 * C) * temp)
                            + (6.0 - 2.0 * B)) as f32,
                    );
                    (v / 6.0) as f32
                } else if v < 2.0 {
                    let v = f64::from(
                        (((-B - 6.0 * C) * (v * temp))
                            + ((6.0 * B + 30.0 * C) * temp)
                            + ((-12.0 * B - 48.0 * C) * v)
                            + (8.0 * B + 24.0 * C)) as f32,
                    );
                    (v / 6.0) as f32
                } else {
                    0.0
                }
            }
            SamplingFilter::CatmullRom => {
                let v = f64::from(value.abs());
                if v < 1.0 {
                    (0.5 * (2.0 + (v * v) * (-5.0 + 3.0 * v))) as f32
                } else if v < 2.0 {
                    (0.5 * (4.0 + v * (-8.0 + v * (5.0 - v)))) as f32
                } else {
                    0.0
                }
            }
        }
    }
}

/// `FilterLanczos`.
fn filter_lanczos(value: f32) -> f32 {
    fn sinc(value: f32) -> f32 {
        if value != 0.0 {
            let value = (f64::from(value) * std::f64::consts::PI) as f32;
            (nif_math::sin(f64::from(value)) / f64::from(value)) as f32
        } else {
            1.0
        }
    }
    let value = value.abs();
    if value < 3.0 {
        (f64::from(sinc(value)) * f64::from(sinc((f64::from(value) / 3.0) as f32))) as f32
    } else {
        0.0
    }
}

/// `Floor` of `ImagingUtility`: of a `Single`.
fn floor_single(value: f32) -> i32 {
    let mut result = trunc(f64::from(value)) as i32;
    if f64::from(value) < f64::from(result) {
        result -= 1;
    }
    result
}

/// `Ceil` of `ImagingUtility`: of a `Single`.
fn ceil_single(value: f32) -> i32 {
    let mut result = trunc(f64::from(value)) as i32;
    if f64::from(value) > f64::from(result) {
        result += 1;
    }
    result
}

/// A contributor of a mapping table: the source position and its weight.
#[derive(Debug, Clone, Copy)]
struct Contributor {
    pos: i32,
    weight: f32,
}

/// `BuildMappingTable` (with `FullEdge`).
#[allow(clippy::too_many_arguments)]
fn build_mapping_table(
    dst_low: i32,
    dst_high: i32,
    src_low: i32,
    src_high: i32,
    src_image_width: i32,
    filter: SamplingFilter,
    radius: f32,
    wrap_edges: bool,
) -> Vec<Vec<Contributor>> {
    let src_width = src_high - src_low;
    let dst_width = dst_high - dst_low;
    if src_width == 1 {
        return (0..dst_width)
            .map(|_| vec![Contributor { pos: 0, weight: 1.0 }])
            .collect();
    }
    if src_width == 0 || dst_width == 0 {
        return Vec::new();
    }
    let mut scale = (f64::from(dst_width) / f64::from(src_width)) as f32;
    let mut result: Vec<Vec<Contributor>> = vec![Vec::new(); dst_width as usize];
    if scale == 0.0 {
        result[0] = vec![Contributor {
            pos: (src_low + src_high) / 2,
            weight: 1.0,
        }];
    } else if scale < 1.0 {
        // Sub-sampling - scales from bigger to smaller
        let radius = (f64::from(radius) / f64::from(scale)) as f32;
        for i in 0..dst_width {
            let center = (f64::from(src_low) - 0.5 + (f64::from(i) + 0.5) / f64::from(scale)) as f32;
            let left = floor_single((f64::from(center) - f64::from(radius)) as f32);
            let right = ceil_single((f64::from(center) + f64::from(radius)) as f32);
            let list = &mut result[i as usize];
            for j in left..=right {
                let weight = (f64::from(filter.apply(((f64::from(center) - f64::from(j)) * f64::from(scale)) as f32))
                    * f64::from(scale)) as f32;
                if weight != 0.0 {
                    list.push(Contributor {
                        pos: j.clamp(src_low, src_high - 1),
                        weight,
                    });
                }
            }
            if list.is_empty() {
                list.push(Contributor {
                    pos: floor_single(center),
                    weight: 1.0,
                });
            }
        }
    } else {
        // Super-sampling - scales from smaller to bigger
        scale = (1.0 / f64::from(scale)) as f32;
        for i in 0..dst_width {
            let center = (f64::from(src_low) - 0.5 + (f64::from(i) + 0.5) * f64::from(scale)) as f32;
            let left = floor_single((f64::from(center) - f64::from(radius)) as f32);
            let right = ceil_single((f64::from(center) + f64::from(radius)) as f32);
            let list = &mut result[i as usize];
            for j in left..=right {
                let weight = filter.apply((f64::from(center) - f64::from(j)) as f32);
                if weight != 0.0 {
                    let n = if wrap_edges {
                        if j < 0 {
                            src_image_width + j
                        } else if j >= src_image_width {
                            j - src_image_width
                        } else {
                            j.clamp(src_low, src_high - 1)
                        }
                    } else {
                        j.clamp(src_low, src_high - 1)
                    };
                    list.push(Contributor { pos: n, weight });
                }
            }
        }
    }
    result
}

/// `StretchResample` with a filter: the source rectangle resampled into
/// the destination rectangle. Both images have the same, not special,
/// format.
#[allow(clippy::too_many_arguments)]
pub fn stretch_resample(
    src: &ImageData,
    src_x: i32,
    src_y: i32,
    src_width: i32,
    src_height: i32,
    dst: &mut ImageData,
    dst_x: i32,
    dst_y: i32,
    dst_width: i32,
    dst_height: i32,
    filter: SamplingFilter,
    wrap_edges: bool,
) {
    let Some(info) = format_info(src.format) else {
        return;
    };
    let radius = filter.radius();
    let map_x = build_mapping_table(
        dst_x,
        dst_x + dst_width,
        src_x,
        src_x + src_width,
        src.width,
        filter,
        radius,
        wrap_edges,
    );
    let map_y = build_mapping_table(
        dst_y,
        dst_y + dst_height,
        src_y,
        src_y + src_height,
        src.height,
        filter,
        radius,
        wrap_edges,
    );
    if map_x.is_empty() || map_y.is_empty() {
        return;
    }
    // `FindExtremes`
    let mut x_minimum = map_x[0][0].pos;
    let mut x_maximum = x_minimum;
    for contributor in map_x.iter().flatten() {
        x_minimum = x_minimum.min(contributor.pos);
        x_maximum = x_maximum.max(contributor.pos);
    }
    let bpp = info.bytes_per_pixel;
    let mut line_buffer = vec![ColorFP::default(); (x_maximum - x_minimum + 1) as usize];
    let accumulate = |accum: &mut [f32; 4], pixel: &ColorFP, weight: f32| {
        // `AccumA := AccumA + SrcFloat.A * Weight` and so on, in the order
        // A, R, G, B.
        for channel in [3, 2, 1, 0] {
            accum[channel] =
                (f64::from(accum[channel]) + f64::from(pixel.channels[channel]) * f64::from(weight)) as f32;
        }
    };
    for j in 0..dst_height {
        let cluster_y = &map_y[j as usize];
        for x in x_minimum..=x_maximum {
            let mut accum = [0f32; 4];
            for contributor in cluster_y {
                let at = (contributor.pos as usize * src.width as usize + x as usize) * bpp;
                let pixel = get_pixel_fp(&src.bits[at..], info);
                accumulate(&mut accum, &pixel, contributor.weight);
            }
            line_buffer[(x - x_minimum) as usize] = ColorFP { channels: accum };
        }
        let mut at = (((j + dst_y) * dst.width + dst_x) as usize) * bpp;
        for i in 0..dst_width {
            let mut accum = [0f32; 4];
            for contributor in &map_x[i as usize] {
                let pixel = line_buffer[(contributor.pos - x_minimum) as usize];
                accumulate(&mut accum, &pixel, contributor.weight);
            }
            set_pixel_fp(&mut dst.bits[at..], info, ColorFP { channels: accum });
            at += bpp;
        }
    }
}

/// `StretchNearest`.
#[allow(clippy::too_many_arguments)]
pub fn stretch_nearest(
    src: &ImageData,
    src_x: i32,
    src_y: i32,
    src_width: i32,
    src_height: i32,
    dst: &mut ImageData,
    dst_x: i32,
    dst_y: i32,
    dst_width: i32,
    dst_height: i32,
) {
    let Some(info) = format_info(src.format) else {
        return;
    };
    let bpp = info.bytes_per_pixel;
    let scale_x = (src_width << 16) / dst_width;
    let scale_y = (src_height << 16) / dst_height;
    let mut yp = 0i32;
    for y in 0..dst_height {
        let mut xp = 0i32;
        let src_line = (((src_y + (yp >> 16)) * src.width + src_x) as usize) * bpp;
        let mut dst_pixel = (((dst_y + y) * dst.width + dst_x) as usize) * bpp;
        for _ in 0..dst_width {
            let from = src_line + (xp >> 16) as usize * bpp;
            // A 12 byte format is not copied (`StretchNearest` has no case
            // for it).
            if bpp != 12 {
                dst.bits[dst_pixel..dst_pixel + bpp].copy_from_slice(&src.bits[from..from + bpp]);
            }
            dst_pixel += bpp;
            xp += scale_x;
        }
        yp += scale_y;
    }
}
