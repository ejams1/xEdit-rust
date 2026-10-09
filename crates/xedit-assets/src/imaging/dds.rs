// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/ImagingLib/Source/ImagingDds.pas (the
// Vampyre Imaging Library by Marek Mauder, at the commit the release links)

//! DirectDraw Surface files as the imaging library reads and writes them
//! (`TDDSFileFormat`): the formats it knows from the pixel format, the
//! FourCC or the DX10 header, mipmaps, cube maps and volumes on load, and
//! 2D textures with their mipmaps on save. The xEdit side has its own DDS
//! header code (`xedit_io::dds`); this one is the library's, whose choices
//! the atlases xEdit writes depend on.

use super::formats::{FormatInfo, ImageFormat, format_info, pixels_size};
use super::{
    ImageData, ImagingError, ResizeFilter, convert_image, new_image, num_mip_map_levels, resize_image, swap_channels,
};

const DDS_MAGIC: u32 = u32::from_le_bytes(*b"DDS ");
const FOURCC_DXT1: u32 = u32::from_le_bytes(*b"DXT1");
const FOURCC_DXT3: u32 = u32::from_le_bytes(*b"DXT3");
const FOURCC_DXT5: u32 = u32::from_le_bytes(*b"DXT5");
const FOURCC_ATI1: u32 = u32::from_le_bytes(*b"ATI1");
const FOURCC_ATI2: u32 = u32::from_le_bytes(*b"ATI2");
const FOURCC_DX10: u32 = u32::from_le_bytes(*b"DX10");
const D3DFMT_A16B16G16R16: u32 = 36;
const D3DFMT_R32F: u32 = 114;
const D3DFMT_A32B32G32R32F: u32 = 116;
const D3DFMT_R16F: u32 = 111;
const D3DFMT_A16B16G16R16F: u32 = 113;

const DDSD_CAPS: u32 = 0x0000_0001;
const DDSD_HEIGHT: u32 = 0x0000_0002;
const DDSD_WIDTH: u32 = 0x0000_0004;
const DDSD_PITCH: u32 = 0x0000_0008;
const DDSD_PIXELFORMAT: u32 = 0x0000_1000;
const DDSD_MIPMAPCOUNT: u32 = 0x0002_0000;
const DDSD_LINEARSIZE: u32 = 0x0008_0000;
const DDSD_DEPTH: u32 = 0x0080_0000;
const DDPF_ALPHAPIXELS: u32 = 0x0000_0001;
const DDPF_FOURCC: u32 = 0x0000_0004;
const DDPF_RGB: u32 = 0x0000_0040;
const DDPF_LUMINANCE: u32 = 0x0002_0000;
const DDPF_BUMPLUMINANCE: u32 = 0x0004_0000;
const DDPF_BUMPDUDV: u32 = 0x0008_0000;
const DDSCAPS_COMPLEX: u32 = 0x0000_0008;
const DDSCAPS_TEXTURE: u32 = 0x0000_1000;
const DDSCAPS_MIPMAP: u32 = 0x0040_0000;
const DDSCAPS2_CUBEMAP: u32 = 0x0000_0200;
const DDSCAPS2_POSITIVEX: u32 = 0x0000_0400;
const DDSCAPS2_NEGATIVEX: u32 = 0x0000_0800;
const DDSCAPS2_POSITIVEY: u32 = 0x0000_1000;
const DDSCAPS2_NEGATIVEY: u32 = 0x0000_2000;
const DDSCAPS2_POSITIVEZ: u32 = 0x0000_4000;
const DDSCAPS2_NEGATIVEZ: u32 = 0x0000_8000;
const DDSCAPS2_VOLUME: u32 = 0x0020_0000;
const DDS_SAVE_FLAGS: u32 = DDSD_CAPS | DDSD_PIXELFORMAT | DDSD_WIDTH | DDSD_HEIGHT | DDSD_LINEARSIZE;

/// The size of `TDDSFileHeader`: the magic and the 124 bytes of the
/// surface description.
const HEADER_SIZE: usize = 128;

/// `DDSSupportedFormats`.
const SUPPORTED_FORMATS: [ImageFormat; 22] = [
    ImageFormat::R8G8B8,
    ImageFormat::A8R8G8B8,
    ImageFormat::X8R8G8B8,
    ImageFormat::A1R5G5B5,
    ImageFormat::A4R4G4B4,
    ImageFormat::X1R5G5B5,
    ImageFormat::X4R4G4B4,
    ImageFormat::R5G6B5,
    ImageFormat::A16B16G16R16,
    ImageFormat::R32F,
    ImageFormat::A32B32G32R32F,
    ImageFormat::R16F,
    ImageFormat::A16B16G16R16F,
    ImageFormat::R3G3B2,
    ImageFormat::Gray8,
    ImageFormat::A8Gray8,
    ImageFormat::Gray16,
    ImageFormat::Dxt1,
    ImageFormat::Dxt3,
    ImageFormat::Dxt5,
    ImageFormat::Ati1n,
    ImageFormat::Ati2n,
];

fn u32_at(data: &[u8], at: usize) -> u32 {
    data.get(at..at + 4)
        .map_or(0, |bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
}

/// `TDDSFileFormat.TestFormat`: a whole header with the magic and the
/// texture capability.
pub fn test_format(data: &[u8]) -> bool {
    data.len() >= HEADER_SIZE && u32_at(data, 0) == DDS_MAGIC && u32_at(data, 108) & DDSCAPS_TEXTURE != 0
}

/// `TDDPixelFormat` and the fields of the surface description the loader
/// reads.
struct Header {
    flags: u32,
    height: u32,
    width: u32,
    pitch_or_linear_size: u32,
    depth: u32,
    mip_maps: i32,
    pf_flags: u32,
    four_cc: u32,
    bit_count: u32,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
    alpha_mask: u32,
    caps2: u32,
}

impl Header {
    fn read(data: &[u8]) -> Header {
        Header {
            flags: u32_at(data, 8),
            height: u32_at(data, 12),
            width: u32_at(data, 16),
            pitch_or_linear_size: u32_at(data, 20),
            depth: u32_at(data, 24),
            mip_maps: u32_at(data, 28) as i32,
            pf_flags: u32_at(data, 80),
            four_cc: u32_at(data, 84),
            bit_count: u32_at(data, 88),
            red_mask: u32_at(data, 92),
            green_mask: u32_at(data, 96),
            blue_mask: u32_at(data, 100),
            alpha_mask: u32_at(data, 104),
            caps2: u32_at(data, 112),
        }
    }

    /// `MasksEqual`.
    fn masks_equal(&self, format: ImageFormat) -> bool {
        let pf = format_info(format).unwrap().pixel_format;
        self.alpha_mask == pf.a_bit_mask
            && self.red_mask == pf.r_bit_mask
            && self.green_mask == pf.g_bit_mask
            && self.blue_mask == pf.b_bit_mask
    }
}

/// `FindFourCCFormat`.
fn find_four_cc_format(four_cc: u32) -> ImageFormat {
    match four_cc {
        D3DFMT_A16B16G16R16 => ImageFormat::A16B16G16R16,
        D3DFMT_R32F => ImageFormat::R32F,
        D3DFMT_A32B32G32R32F => ImageFormat::A32B32G32R32F,
        D3DFMT_R16F => ImageFormat::R16F,
        D3DFMT_A16B16G16R16F => ImageFormat::A16B16G16R16F,
        FOURCC_DXT1 => ImageFormat::Dxt1,
        FOURCC_DXT3 => ImageFormat::Dxt3,
        FOURCC_DXT5 => ImageFormat::Dxt5,
        FOURCC_ATI1 => ImageFormat::Ati1n,
        FOURCC_ATI2 => ImageFormat::Ati2n,
        _ => ImageFormat::Unknown,
    }
}

/// `FindDX10Format`: the format of a DXGI format number and whether its red
/// and blue are swapped.
fn find_dx10_format(dxgi: u32) -> (ImageFormat, bool) {
    use ImageFormat::*;
    match dxgi {
        1 | 2 => (A32B32G32R32F, false),
        5 | 6 => (B32G32R32F, false),
        10 => (A16B16G16R16F, false),
        9 | 11..=14 => (A16B16G16R16, false),
        27..=32 => (A8R8G8B8, true),
        39 | 42 | 43 => (Gray32, false),
        40 | 41 => (R32F, false),
        48..=52 => (A8Gray8, false),
        53 | 55..=59 => (Gray16, false),
        54 => (R16F, false),
        60..=65 => (Gray8, false),
        70..=72 => (Dxt1, false),
        73..=75 => (Dxt3, false),
        76..=78 => (Dxt5, false),
        79..=81 => (Ati1n, false),
        82..=84 => (Ati2n, false),
        85 => (R5G6B5, false),
        86 => (A1R5G5B5, false),
        87 | 90 => (A8R8G8B8, false),
        88 | 92 => (X8R8G8B8, false),
        115 => (A4R4G4B4, false),
        _ => (Unknown, false),
    }
}

/// `GetVolumeLevelCount`.
fn volume_level_count(depth: i32, mip_maps: i32) -> i32 {
    let mut result = depth;
    for i in 1..mip_maps {
        result += (depth >> i).clamp(1, depth.max(1));
    }
    result
}

/// `ComputeSubDimensions`.
#[allow(clippy::too_many_arguments)]
fn compute_sub_dimensions(
    mut idx: i32,
    width: i32,
    height: i32,
    mip_maps: i32,
    depth: i32,
    is_cube_map: bool,
    is_volume: bool,
) -> (i32, i32) {
    let (mut w, mut h) = (width, height);
    let half = |value: i32| (value >> 1).clamp(1, value.max(1));
    if mip_maps > 1 {
        if !is_volume {
            if is_cube_map {
                idx -= (idx / mip_maps) * mip_maps;
            }
            for _ in 0..idx {
                w = half(w);
                h = half(h);
            }
        } else {
            let mut shift = 0;
            let mut last = depth;
            while idx > last - 1 {
                w = half(w);
                h = half(h);
                if w == 1 && h == 1 {
                    break;
                }
                shift += 1;
                last += (depth >> shift).clamp(1, depth.max(1));
            }
        }
    }
    (w, h)
}

/// `TDDSFileFormat.LoadData`: every image of the file (mipmaps, faces,
/// slices), `None` for a format the library does not know. Data that ends
/// early leaves the rest of an image zero, as the memory reader reads what
/// is there.
pub fn load_data(data: &[u8]) -> Result<Option<Vec<ImageData>>, ImagingError> {
    let hdr = Header::read(data);
    let mut pos = HEADER_SIZE;
    let read = |out: &mut [u8], pos: &mut usize| {
        let available = data.len().saturating_sub(*pos).min(out.len());
        out[..available].copy_from_slice(&data[*pos..*pos + available]);
        *pos += available;
    };
    let mut src_format = ImageFormat::Unknown;
    let mut needs_swap_channels = false;
    if hdr.pf_flags & DDPF_FOURCC == DDPF_FOURCC {
        if hdr.four_cc == FOURCC_DX10 {
            // UPSTREAM-QUIRK: `TDX10Header` is a packed record whose two
            // enumerations are one byte each (`$MINENUMSIZE 1` of
            // `ImagingOptions.inc`), so the loader reads 14 of the 20 bytes
            // of the DX10 header and the image data six bytes early.
            let mut dx10 = [0u8; 14];
            read(&mut dx10, &mut pos);
            (src_format, needs_swap_channels) = find_dx10_format(u32::from(dx10[0]));
        } else {
            src_format = find_four_cc_format(hdr.four_cc);
        }
    } else if hdr.pf_flags & DDPF_RGB == DDPF_RGB {
        if hdr.pf_flags & DDPF_ALPHAPIXELS == DDPF_ALPHAPIXELS {
            match hdr.bit_count {
                16 => {
                    if hdr.masks_equal(ImageFormat::A4R4G4B4) {
                        src_format = ImageFormat::A4R4G4B4;
                    }
                    if hdr.masks_equal(ImageFormat::A1R5G5B5) {
                        src_format = ImageFormat::A1R5G5B5;
                    }
                }
                32 => {
                    src_format = ImageFormat::A8R8G8B8;
                    if hdr.blue_mask == 0x00FF_0000 {
                        needs_swap_channels = true;
                    }
                }
                _ => {}
            }
        } else {
            match hdr.bit_count {
                8 => {
                    if hdr.masks_equal(ImageFormat::R3G3B2) {
                        src_format = ImageFormat::R3G3B2;
                    }
                }
                16 => {
                    if hdr.masks_equal(ImageFormat::X4R4G4B4) {
                        src_format = ImageFormat::X4R4G4B4;
                    }
                    if hdr.masks_equal(ImageFormat::X1R5G5B5) {
                        src_format = ImageFormat::X1R5G5B5;
                    }
                    if hdr.masks_equal(ImageFormat::R5G6B5) {
                        src_format = ImageFormat::R5G6B5;
                    }
                }
                24 => src_format = ImageFormat::R8G8B8,
                32 => {
                    src_format = ImageFormat::X8R8G8B8;
                    if hdr.blue_mask == 0x00FF_0000 {
                        needs_swap_channels = true;
                    }
                }
                _ => {}
            }
        }
    } else if hdr.pf_flags & DDPF_LUMINANCE == DDPF_LUMINANCE {
        if hdr.pf_flags & DDPF_ALPHAPIXELS == DDPF_ALPHAPIXELS {
            if hdr.bit_count == 16 {
                src_format = ImageFormat::A8Gray8;
            }
        } else {
            match hdr.bit_count {
                8 => src_format = ImageFormat::Gray8,
                16 => src_format = ImageFormat::Gray16,
                _ => {}
            }
        }
    } else if hdr.pf_flags & DDPF_BUMPLUMINANCE == DDPF_BUMPLUMINANCE {
        if hdr.bit_count == 32 && hdr.blue_mask == 0x00FF_0000 {
            src_format = ImageFormat::X8R8G8B8;
            needs_swap_channels = true;
        }
    } else if hdr.pf_flags & DDPF_BUMPDUDV == DDPF_BUMPDUDV {
        match hdr.bit_count {
            16 => src_format = ImageFormat::A8Gray8,
            32 => {
                if hdr.alpha_mask == 0xFF00_0000 {
                    src_format = ImageFormat::A8R8G8B8;
                    needs_swap_channels = true;
                }
            }
            64 => src_format = ImageFormat::A16B16G16R16,
            _ => {}
        }
    }
    if src_format == ImageFormat::Unknown {
        return Ok(None);
    }
    let mut image_count = 1;
    if hdr.mip_maps > 1 {
        image_count = hdr.mip_maps;
    }
    let mut loaded_volume = false;
    let mut loaded_cube_map = false;
    if hdr.caps2 & DDSCAPS2_VOLUME == DDSCAPS2_VOLUME && hdr.flags & DDSD_DEPTH == DDSD_DEPTH {
        loaded_volume = true;
        image_count = volume_level_count(hdr.depth as i32, image_count);
    }
    if hdr.caps2 & DDSCAPS2_CUBEMAP == DDSCAPS2_CUBEMAP {
        loaded_cube_map = true;
        let faces = [
            DDSCAPS2_POSITIVEX,
            DDSCAPS2_POSITIVEY,
            DDSCAPS2_POSITIVEZ,
            DDSCAPS2_NEGATIVEX,
            DDSCAPS2_NEGATIVEY,
            DDSCAPS2_NEGATIVEZ,
        ]
        .iter()
        .filter(|&&face| hdr.caps2 & face == face)
        .count() as i32;
        image_count *= faces;
    }
    let info: &FormatInfo = format_info(src_format).unwrap();
    let use_as_pitch = hdr.flags & DDSD_PITCH == DDSD_PITCH;
    let mut use_as_linear = hdr.flags & DDSD_LINEARSIZE == DDSD_LINEARSIZE;
    if !use_as_pitch && !use_as_linear {
        use_as_linear = true;
    }
    let mut pitch_or_linear = hdr.pitch_or_linear_size as i32;
    let main_image_linear_size = pixels_size(info, hdr.width as i32, hdr.height as i32) as i32;
    if use_as_linear
        && (pitch_or_linear < main_image_linear_size
            || pitch_or_linear.wrapping_mul(hdr.height as i32) == main_image_linear_size)
    {
        pitch_or_linear = main_image_linear_size;
    }
    let mut images = Vec::with_capacity(image_count.max(0) as usize);
    for i in 0..image_count {
        let (width, height) = compute_sub_dimensions(
            i,
            hdr.width as i32,
            hdr.height as i32,
            hdr.mip_maps,
            hdr.depth as i32,
            loaded_cube_map,
            loaded_volume,
        );
        let mut image = new_image(width, height, src_format)?;
        if i > 0 || pitch_or_linear == 0 {
            pitch_or_linear = if use_as_linear {
                pixels_size(info, width, height) as i32
            } else {
                (width * info.bytes_per_pixel as i32 + 3) / 4 * 4
            };
        }
        let load_size = if use_as_linear {
            pitch_or_linear
        } else {
            height * pitch_or_linear
        };
        if use_as_linear || load_size as usize == image.bits.len() {
            let len = (load_size.max(0) as usize).min(image.bits.len());
            read(&mut image.bits[..len], &mut pos);
            // A load size beyond the image reads past its bits upstream;
            // the reader moves on by the whole size.
            pos += (load_size.max(0) as usize).saturating_sub(len);
        } else {
            let mut buffer = vec![0u8; load_size.max(0) as usize];
            read(&mut buffer, &mut pos);
            // `RemovePadBytes`
            let row = width as usize * info.bytes_per_pixel;
            for y in 0..height as usize {
                let from = y * pitch_or_linear as usize;
                if from + row <= buffer.len() && (y + 1) * row <= image.bits.len() {
                    image.bits[y * row..(y + 1) * row].copy_from_slice(&buffer[from..from + row]);
                }
            }
        }
        if needs_swap_channels {
            swap_channels(&mut image, super::formats::CHANNEL_RED, super::formats::CHANNEL_BLUE)?;
        }
        images.push(image);
    }
    Ok(Some(images))
}

/// `TDDSFileFormat.SaveData` of a 2D texture: the first image as the main
/// image and the others as its mipmaps, each made the size and format the
/// mipmap chain of the main image needs.
pub fn save_data(images: &[ImageData]) -> Result<Vec<u8>, ImagingError> {
    let Some(first) = images.first() else {
        return Err(ImagingError::new("No images to save"));
    };
    let mip_map_count = (images.len() as i32).min(num_mip_map_levels(first.width, first.height));
    // `MakeCompatible`: a format the file does not support is converted.
    let main = if SUPPORTED_FORMATS.contains(&first.format) {
        first.clone()
    } else {
        let mut copy = first.clone();
        convert_to_supported(&mut copy)?;
        copy
    };
    let info = format_info(main.format).ok_or_else(|| ImagingError::new("Invalid image format"))?;
    let mut flags = DDS_SAVE_FLAGS;
    let mut caps1 = DDSCAPS_TEXTURE;
    let mut mip_maps = 0;
    if mip_map_count > 1 {
        flags |= DDSD_MIPMAPCOUNT;
        caps1 |= DDSCAPS_MIPMAP | DDSCAPS_COMPLEX;
        mip_maps = mip_map_count;
    }
    let mut pf_flags;
    let (mut four_cc, mut bit_count) = (0u32, 0u32);
    let (mut red, mut green, mut blue, mut alpha) = (0u32, 0u32, 0u32, 0u32);
    if info.is_special || info.is_floating_point || info.bytes_per_pixel > 4 {
        pf_flags = DDPF_FOURCC;
        four_cc = match main.format {
            ImageFormat::A16B16G16R16 => D3DFMT_A16B16G16R16,
            ImageFormat::R32F => D3DFMT_R32F,
            ImageFormat::A32B32G32R32F => D3DFMT_A32B32G32R32F,
            ImageFormat::R16F => D3DFMT_R16F,
            ImageFormat::A16B16G16R16F => D3DFMT_A16B16G16R16F,
            ImageFormat::Dxt1 => FOURCC_DXT1,
            ImageFormat::Dxt3 => FOURCC_DXT3,
            ImageFormat::Dxt5 => FOURCC_DXT5,
            ImageFormat::Ati1n => FOURCC_ATI1,
            ImageFormat::Ati2n => FOURCC_ATI2,
            _ => 0,
        };
    } else if info.has_gray_channel {
        pf_flags = DDPF_LUMINANCE;
        bit_count = info.bytes_per_pixel as u32 * 8;
        match main.format {
            ImageFormat::Gray8 => red = 255,
            ImageFormat::Gray16 => red = 65535,
            ImageFormat::A8Gray8 => {
                pf_flags |= DDPF_ALPHAPIXELS;
                red = 255;
                alpha = 65280;
            }
            _ => {}
        }
    } else {
        pf_flags = DDPF_RGB;
        bit_count = info.bytes_per_pixel as u32 * 8;
        if info.has_alpha_channel {
            pf_flags |= DDPF_ALPHAPIXELS;
            alpha = 0xFF00_0000;
        }
        if info.bytes_per_pixel > 2 {
            red = 0x00FF_0000;
            green = 0x0000_FF00;
            blue = 0x0000_00FF;
        } else {
            alpha = info.pixel_format.a_bit_mask;
            red = info.pixel_format.r_bit_mask;
            green = info.pixel_format.g_bit_mask;
            blue = info.pixel_format.b_bit_mask;
        }
    }
    let mut out = Vec::with_capacity(HEADER_SIZE + main.bits.len() * 2);
    let mut push = |value: u32| out.extend_from_slice(&value.to_le_bytes());
    push(DDS_MAGIC);
    push(124);
    push(flags);
    push(main.height as u32);
    push(main.width as u32);
    push(main.bits.len() as u32);
    push(0); // depth
    push(mip_maps as u32);
    for _ in 0..11 {
        push(0);
    }
    push(32);
    push(pf_flags);
    push(four_cc);
    push(bit_count);
    push(red);
    push(green);
    push(blue);
    push(alpha);
    push(caps1);
    push(0); // caps2
    push(0);
    push(0);
    push(0); // reserved2
    out.extend_from_slice(&main.bits);
    for (i, image) in images.iter().enumerate().take(mip_map_count as usize).skip(1) {
        let (width, height) = compute_sub_dimensions(i as i32, main.width, main.height, mip_maps, 0, false, false);
        let needs_resize = !(image.width == width && image.height == height);
        let needs_convert = image.format != main.format;
        if needs_resize || needs_convert {
            let mut copy = image.clone();
            if needs_convert {
                convert_image(&mut copy, main.format)?;
            }
            if needs_resize {
                resize_image(&mut copy, width, height, ResizeFilter::Bilinear)?;
            }
            out.extend_from_slice(&copy.bits);
        } else {
            out.extend_from_slice(&image.bits);
        }
    }
    Ok(out)
}

/// `TDDSFileFormat.ConvertToSupported`.
fn convert_to_supported(image: &mut ImageData) -> Result<(), ImagingError> {
    let info = format_info(image.format).ok_or_else(|| ImagingError::new("Invalid image format"))?;
    let format = if info.is_indexed || info.is_special {
        ImageFormat::A8R8G8B8
    } else if info.is_floating_point {
        if info.format == ImageFormat::A16R16G16B16F {
            ImageFormat::A16B16G16R16F
        } else {
            ImageFormat::A32B32G32R32F
        }
    } else if info.has_gray_channel {
        if info.has_alpha_channel {
            ImageFormat::A8Gray8
        } else if info.bytes_per_pixel == 1 {
            ImageFormat::Gray8
        } else {
            ImageFormat::Gray16
        }
    } else if info.bytes_per_pixel > 4 {
        ImageFormat::A16B16G16R16
    } else if info.has_alpha_channel {
        ImageFormat::A8R8G8B8
    } else {
        ImageFormat::X8R8G8B8
    };
    convert_image(image, format)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_dxt5_loads_back() {
        let mut image = new_image(8, 8, ImageFormat::Default).unwrap();
        for (i, pixel) in image.bits.chunks_mut(4).enumerate() {
            pixel.copy_from_slice(&[i as u8, (i * 3) as u8, 200, 255]);
        }
        convert_image(&mut image, ImageFormat::Dxt5).unwrap();
        let mipmaps = super::super::generate_mip_maps(&image, 0, super::super::SamplingFilter::Lanczos).unwrap();
        let bytes = save_data(&mipmaps).unwrap();
        assert!(test_format(&bytes));
        assert_eq!(&bytes[84..88], b"DXT5");
        // 8x8, 4x4, then the 2x2 and 1x1 levels as one block each.
        assert_eq!(bytes.len(), 128 + 64 + 16 + 16 + 16);
        let loaded = load_data(&bytes).unwrap().unwrap();
        assert_eq!(loaded.len(), 4);
        assert_eq!(loaded[0], image);
    }
}
