// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/ImagingLib/Source/Imaging.pas (the Vampyre
// Imaging Library by Marek Mauder, at the commit the release links)

//! The part of the Vampyre Imaging Library that LOD generation uses: images
//! in memory (`TImageData`), their creation, copies, conversions between
//! pixel formats (`formats`), resizing, copying of rectangles, mipmap
//! generation, single pixel access, the DDS file format (`dds`) and the
//! point transforms of the canvas (`canvases`).
//!
//! An image is only ever loaded from a DDS file here: xEdit gives the
//! library the textures of the game, and the other formats the release
//! links (PNG, Targa, bitmap) never come up. The library's options are the
//! defaults with the mipmap filter set by the caller.

pub mod canvases;
pub mod dds;
pub mod formats;

pub use formats::{Color32, ColorFP, ImageFormat, SamplingFilter, format_info};

use formats::{FormatInfo, check_dimensions, convert_pixels, convert_special, pixels_size};

/// An error of the library (`EImagingError`), with its message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ImagingError(pub String);

impl ImagingError {
    pub fn new(message: impl Into<String>) -> ImagingError {
        ImagingError(message.into())
    }
}

/// `TImageData`: an image in memory. The palette of the indexed format is
/// not kept (no DDS file loads as one).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageData {
    pub width: i32,
    pub height: i32,
    pub format: ImageFormat,
    pub bits: Vec<u8>,
}

impl ImageData {
    /// `TestImage`: a format with a description and bits of its size.
    pub fn is_valid(&self) -> bool {
        format_info(self.format).is_some_and(|info| pixels_size(info, self.width, self.height) == self.bits.len())
            && self.format != ImageFormat::Unknown
    }

    fn info(&self) -> Result<&'static FormatInfo, ImagingError> {
        format_info(self.format).ok_or_else(|| ImagingError::new("Invalid image"))
    }
}

/// `DefaultImageFormat`.
pub const DEFAULT_IMAGE_FORMAT: ImageFormat = ImageFormat::A8R8G8B8;

/// `NewImage`: an image of zero bits; a DXT format rounds the dimensions up
/// to multiples of four.
pub fn new_image(width: i32, height: i32, format: ImageFormat) -> Result<ImageData, ImagingError> {
    let format = if format == ImageFormat::Default {
        DEFAULT_IMAGE_FORMAT
    } else {
        format
    };
    let info = format_info(format).ok_or_else(|| ImagingError::new("Invalid image format"))?;
    let (mut width, mut height) = (width, height);
    check_dimensions(info, &mut width, &mut height);
    let size = pixels_size(info, width, height);
    if size == 0 {
        return Ok(ImageData::default());
    }
    Ok(ImageData {
        width,
        height,
        format,
        bits: vec![0; size],
    })
}

/// `ConvertImage`.
pub fn convert_image(image: &mut ImageData, dest_format: ImageFormat) -> Result<bool, ImagingError> {
    if !image.is_valid() {
        return Ok(false);
    }
    let dest_format = if dest_format == ImageFormat::Default {
        DEFAULT_IMAGE_FORMAT
    } else {
        dest_format
    };
    let src_info = image.info()?;
    let Some(dst_info) = format_info(dest_format) else {
        return Ok(false);
    };
    if src_info.format == dst_info.format {
        return Ok(true);
    }
    // If dest format is just src with swapped channels we call SwapChannels
    if src_info.rb_swap_format == dest_format && dst_info.rb_swap_format == src_info.format {
        let result = swap_channels(image, formats::CHANNEL_RED, formats::CHANNEL_BLUE)?;
        image.format = src_info.rb_swap_format;
        return Ok(result);
    }
    if src_info.is_indexed || dst_info.is_indexed {
        return Err(ImagingError::new("Indexed images are not supported"));
    }
    if !src_info.is_special && !dst_info.is_special {
        let num_pixels = (image.width * image.height) as usize;
        let mut new_data = vec![0u8; num_pixels * dst_info.bytes_per_pixel];
        convert_pixels(num_pixels, &image.bits, &mut new_data, src_info, dst_info);
        image.format = dest_format;
        image.bits = new_data;
    } else {
        convert_special(image, src_info, dst_info)?;
    }
    Ok(true)
}

/// `SwapChannels`.
pub fn swap_channels(image: &mut ImageData, src_channel: usize, dst_channel: usize) -> Result<bool, ImagingError> {
    if !image.is_valid() || src_channel == dst_channel {
        return Ok(false);
    }
    let info = image.info()?;
    let num_pixels = (image.width * image.height) as usize;
    let bpp = info.bytes_per_pixel;
    if info.format == ImageFormat::R8G8B8
        || (info.format == ImageFormat::A8R8G8B8
            && src_channel != formats::CHANNEL_ALPHA
            && dst_channel != formats::CHANNEL_ALPHA)
    {
        for i in 0..num_pixels {
            image.bits.swap(i * bpp + src_channel, i * bpp + dst_channel);
        }
    } else if info.is_special {
        let format = info.format;
        convert_image(image, ImageFormat::Default)?;
        swap_channels(image, src_channel, dst_channel)?;
        convert_image(image, format)?;
    } else {
        formats::swap_channels_general(&mut image.bits, info, num_pixels, src_channel, dst_channel);
    }
    Ok(true)
}

/// `TResizeFilter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeFilter {
    Nearest,
    Bilinear,
    Bicubic,
    Lanczos,
}

/// `ResizeImage`.
pub fn resize_image(
    image: &mut ImageData,
    new_width: i32,
    new_height: i32,
    filter: ResizeFilter,
) -> Result<bool, ImagingError> {
    if !image.is_valid() || (image.width == new_width && image.height == new_height) {
        return Ok(false);
    }
    let mut work = new_image(new_width, new_height, image.format)?;
    let (w, h) = (work.width, work.height);
    stretch_rect(image, 0, 0, image.width, image.height, &mut work, 0, 0, w, h, filter)?;
    *image = work;
    Ok(true)
}

/// `ClipCopyBounds`.
#[allow(clippy::too_many_arguments)]
fn clip_copy_bounds(
    src_x: &mut i32,
    src_y: &mut i32,
    width: &mut i32,
    height: &mut i32,
    dst_x: &mut i32,
    dst_y: &mut i32,
    src_image_width: i32,
    src_image_height: i32,
    clip: (i32, i32, i32, i32),
) {
    fn clip_dim(src_pos: &mut i32, dst_pos: &mut i32, size: &mut i32, src_clip_max: i32, dst_min: i32, dst_max: i32) {
        let old_dst_pos = if *dst_pos < 0 { *dst_pos } else { 0 };
        if *dst_pos < dst_min {
            let diff = dst_min - *dst_pos;
            *size -= diff;
            *src_pos += diff;
            *dst_pos = dst_min;
        }
        if *src_pos < 0 {
            *size = *size + *src_pos - old_dst_pos;
            *dst_pos = *dst_pos - *src_pos + old_dst_pos;
            *src_pos = 0;
        }
        if *src_pos + *size > src_clip_max {
            *size = src_clip_max - *src_pos;
        }
        if *dst_pos + *size > dst_max {
            *size = dst_max - *dst_pos;
        }
    }
    clip_dim(src_x, dst_x, width, src_image_width, clip.0, clip.2);
    clip_dim(src_y, dst_y, height, src_image_height, clip.1, clip.3);
}

/// `ClipStretchBounds`.
#[allow(clippy::too_many_arguments)]
fn clip_stretch_bounds(
    src_x: &mut i32,
    src_y: &mut i32,
    src_width: &mut i32,
    src_height: &mut i32,
    dst_x: &mut i32,
    dst_y: &mut i32,
    dst_width: &mut i32,
    dst_height: &mut i32,
    src_image_width: i32,
    src_image_height: i32,
    clip: (i32, i32, i32, i32),
) {
    #[allow(clippy::too_many_arguments)]
    fn clip_dim(
        src_pos: &mut i32,
        dst_pos: &mut i32,
        src_size: &mut i32,
        dst_size: &mut i32,
        src_clip_max: i32,
        dst_min: i32,
        dst_max: i32,
    ) {
        let r = |value: f64| formats::round(value) as i32;
        let scale = (f64::from(*dst_size) / f64::from(*src_size)) as f32;
        if *dst_pos < dst_min {
            let diff = dst_min - *dst_pos;
            *dst_size -= diff;
            *src_pos += r(f64::from(diff) / f64::from(scale));
            *src_size -= r(f64::from(diff) / f64::from(scale));
            *dst_pos = dst_min;
        }
        if *src_pos < 0 {
            *src_size += *src_pos;
            *dst_pos -= r(f64::from(*src_pos) * f64::from(scale));
            *dst_size += r(f64::from(*src_pos) * f64::from(scale));
            *src_pos = 0;
        }
        if *src_pos + *src_size > src_clip_max {
            let old_size = *src_size;
            *src_size = src_clip_max - *src_pos;
            *dst_size = r(f64::from(*dst_size) * (f64::from(*src_size) / f64::from(old_size)));
        }
        if *dst_pos + *dst_size > dst_max {
            let old_size = *dst_size;
            *dst_size = dst_max - *dst_pos;
            *src_size = r(f64::from(*src_size) * (f64::from(*dst_size) / f64::from(old_size)));
        }
    }
    clip_dim(src_x, dst_x, src_width, dst_width, src_image_width, clip.0, clip.2);
    clip_dim(src_y, dst_y, src_height, dst_height, src_image_height, clip.1, clip.3);
}

/// `CopyRect`.
#[allow(clippy::too_many_arguments)]
pub fn copy_rect(
    src: &ImageData,
    mut src_x: i32,
    mut src_y: i32,
    mut width: i32,
    mut height: i32,
    dst: &mut ImageData,
    mut dst_x: i32,
    mut dst_y: i32,
) -> Result<bool, ImagingError> {
    if !src.is_valid() || !dst.is_valid() {
        return Ok(false);
    }
    clip_copy_bounds(
        &mut src_x,
        &mut src_y,
        &mut width,
        &mut height,
        &mut dst_x,
        &mut dst_y,
        src.width,
        src.height,
        (0, 0, dst.width, dst.height),
    );
    if width <= 0 || height <= 0 {
        return Ok(false);
    }
    let mut old_format = ImageFormat::Unknown;
    let mut info = dst.info()?;
    if info.is_special {
        old_format = info.format;
        convert_image(dst, ImageFormat::Default)?;
        info = dst.info()?;
    }
    let converted;
    let work = if src.format != dst.format {
        let mut copy = src.clone();
        convert_image(&mut copy, dst.format)?;
        converted = copy;
        &converted
    } else {
        src
    };
    let bpp = info.bytes_per_pixel;
    let move_bytes = width as usize * bpp;
    let dst_width_bytes = dst.width as usize * bpp;
    let src_width_bytes = work.width as usize * bpp;
    let mut dst_at = dst_y as usize * dst_width_bytes + dst_x as usize * bpp;
    let mut src_at = src_y as usize * src_width_bytes + src_x as usize * bpp;
    for _ in 0..height {
        dst.bits[dst_at..dst_at + move_bytes].copy_from_slice(&work.bits[src_at..src_at + move_bytes]);
        src_at += src_width_bytes;
        dst_at += dst_width_bytes;
    }
    if old_format != ImageFormat::Unknown {
        convert_image(dst, old_format)?;
    }
    Ok(true)
}

/// `StretchRect`.
#[allow(clippy::too_many_arguments)]
pub fn stretch_rect(
    src: &ImageData,
    mut src_x: i32,
    mut src_y: i32,
    mut src_width: i32,
    mut src_height: i32,
    dst: &mut ImageData,
    mut dst_x: i32,
    mut dst_y: i32,
    mut dst_width: i32,
    mut dst_height: i32,
    filter: ResizeFilter,
) -> Result<bool, ImagingError> {
    if !src.is_valid() || !dst.is_valid() {
        return Ok(false);
    }
    let clip = (0, 0, dst.width, dst.height);
    clip_stretch_bounds(
        &mut src_x,
        &mut src_y,
        &mut src_width,
        &mut src_height,
        &mut dst_x,
        &mut dst_y,
        &mut dst_width,
        &mut dst_height,
        src.width,
        src.height,
        clip,
    );
    if src_width == dst_width && src_height == dst_height {
        return copy_rect(src, src_x, src_y, src_width, src_height, dst, dst_x, dst_y);
    }
    if src_width <= 0 || src_height <= 0 || dst_width <= 0 || dst_height <= 0 {
        return Ok(false);
    }
    let mut old_format = ImageFormat::Unknown;
    let info = dst.info()?;
    if info.is_special {
        old_format = info.format;
        convert_image(dst, ImageFormat::Default)?;
    }
    let converted;
    let work = if src.format != dst.format {
        let mut copy = src.clone();
        convert_image(&mut copy, dst.format)?;
        converted = copy;
        &converted
    } else {
        src
    };
    if filter == ResizeFilter::Nearest {
        formats::stretch_nearest(
            work, src_x, src_y, src_width, src_height, dst, dst_x, dst_y, dst_width, dst_height,
        );
    } else {
        let resampling = match filter {
            ResizeFilter::Bilinear => SamplingFilter::Linear,
            ResizeFilter::Bicubic => SamplingFilter::CatmullRom,
            ResizeFilter::Lanczos => SamplingFilter::Lanczos,
            ResizeFilter::Nearest => SamplingFilter::Nearest,
        };
        formats::stretch_resample(
            work, src_x, src_y, src_width, src_height, dst, dst_x, dst_y, dst_width, dst_height, resampling, false,
        );
    }
    if old_format != ImageFormat::Unknown {
        convert_image(dst, old_format)?;
    }
    Ok(true)
}

/// `GetNumMipMapLevels`.
pub fn num_mip_map_levels(mut width: i32, mut height: i32) -> i32 {
    if width <= 0 || height <= 0 {
        return 0;
    }
    let mut result = 1;
    while width != 1 || height != 1 {
        width = (width / 2).max(1);
        height = (height / 2).max(1);
        result += 1;
    }
    result
}

/// `FillMipMapLevel`: a smaller level resampled from the bigger image with
/// the mipmap filter.
pub fn fill_mip_map_level(
    bigger: &ImageData,
    width: i32,
    height: i32,
    filter: SamplingFilter,
) -> Result<ImageData, ImagingError> {
    let info = bigger.info()?;
    let compatible_copy;
    let compatible = if info.is_special {
        let mut copy = bigger.clone();
        convert_image(&mut copy, ImageFormat::Default)?;
        compatible_copy = copy;
        &compatible_copy
    } else {
        bigger
    };
    let mut smaller = new_image(width, height, compatible.format)?;
    if filter == SamplingFilter::Nearest {
        formats::stretch_nearest(
            compatible,
            0,
            0,
            compatible.width,
            compatible.height,
            &mut smaller,
            0,
            0,
            width,
            height,
        );
    } else {
        formats::stretch_resample(
            compatible,
            0,
            0,
            compatible.width,
            compatible.height,
            &mut smaller,
            0,
            0,
            width,
            height,
            filter,
            false,
        );
    }
    if compatible.format != bigger.format {
        convert_image(&mut smaller, bigger.format)?;
    }
    Ok(smaller)
}

/// `GenerateMipMaps`: the image and its smaller levels, each resampled from
/// the whole image with `filter` (the `ImagingMipMapFilter` option).
pub fn generate_mip_maps(
    image: &ImageData,
    levels: i32,
    filter: SamplingFilter,
) -> Result<Vec<ImageData>, ImagingError> {
    if !image.is_valid() {
        return Ok(Vec::new());
    }
    let (mut width, mut height) = (image.width, image.height);
    let count = num_mip_map_levels(width, height);
    let levels = if levels <= 0 || levels > count { count } else { levels };
    let info = image.info()?;
    let compatible_copy;
    let compatible = if info.is_special {
        let mut copy = image.clone();
        convert_image(&mut copy, ImageFormat::Default)?;
        compatible_copy = copy;
        &compatible_copy
    } else {
        image
    };
    let mut mipmaps = Vec::with_capacity(levels as usize);
    mipmaps.push(image.clone());
    for _ in 1..levels {
        width = (width >> 1).max(1);
        height = (height >> 1).max(1);
        mipmaps.push(fill_mip_map_level(compatible, width, height, filter)?);
    }
    if compatible.format != image.format {
        for level in mipmaps.iter_mut().skip(1) {
            convert_image(level, image.format)?;
        }
    }
    Ok(mipmaps)
}

/// `GetPixel32`.
pub fn get_pixel32(image: &ImageData, x: i32, y: i32) -> Color32 {
    let info = format_info(image.format).expect("pixel of an image without a format");
    let at = ((y * image.width + x) as usize) * info.bytes_per_pixel;
    formats::get_pixel32(&image.bits[at..], info)
}

/// `SetPixel32`.
pub fn set_pixel32(image: &mut ImageData, x: i32, y: i32, color: Color32) {
    let info = format_info(image.format).expect("pixel of an image without a format");
    let at = ((y * image.width + x) as usize) * info.bytes_per_pixel;
    formats::set_pixel32(&mut image.bits[at..], info, color);
}

/// `LoadImageFromMemory`: the first image of a DDS file (the only format
/// read here), `None` when the data is not one the library loads.
pub fn load_image_from_memory(data: &[u8]) -> Result<Option<ImageData>, ImagingError> {
    if !dds::test_format(data) {
        return Ok(None);
    }
    let images = dds::load_data(data)?;
    Ok(images.and_then(|images| images.into_iter().next()))
}

/// `SaveMultiImageToFile` of a DDS file: the images as the main image and
/// its mipmaps.
pub fn save_multi_image_to_dds(images: &[ImageData]) -> Result<Vec<u8>, ImagingError> {
    dds::save_data(images)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: i32, height: i32) -> ImageData {
        let mut image = new_image(width, height, ImageFormat::Default).unwrap();
        for y in 0..height {
            for x in 0..width {
                set_pixel32(
                    &mut image,
                    x,
                    y,
                    Color32 {
                        b: (x * 255 / width) as u8,
                        g: (y * 255 / height) as u8,
                        r: ((x + y) * 7 % 256) as u8,
                        a: if (x + y) % 5 == 0 { 64 } else { 255 },
                    },
                );
            }
        }
        image
    }

    #[test]
    fn dxt_dimensions_round_up() {
        let image = new_image(2, 6, ImageFormat::Dxt5).unwrap();
        assert_eq!((image.width, image.height, image.bits.len()), (4, 8, 32));
        let image = new_image(5, 4, ImageFormat::Dxt1).unwrap();
        assert_eq!((image.width, image.bits.len()), (8, 16));
    }

    #[test]
    fn dxt_round_trip_keeps_flat_blocks() {
        let mut image = new_image(8, 8, ImageFormat::Default).unwrap();
        for pixel in image.bits.chunks_mut(4) {
            pixel.copy_from_slice(&[0, 0, 255, 255]);
        }
        for format in [ImageFormat::Dxt1, ImageFormat::Dxt3, ImageFormat::Dxt5] {
            let mut copy = image.clone();
            convert_image(&mut copy, format).unwrap();
            convert_image(&mut copy, ImageFormat::Default).unwrap();
            assert_eq!(copy.bits, image.bits, "{format:?}");
        }
    }

    #[test]
    fn mipmaps_reach_one_pixel() {
        let image = gradient(16, 8);
        let mipmaps = generate_mip_maps(&image, 0, SamplingFilter::Lanczos).unwrap();
        let sizes: Vec<(i32, i32)> = mipmaps.iter().map(|m| (m.width, m.height)).collect();
        assert_eq!(sizes, vec![(16, 8), (8, 4), (4, 2), (2, 1), (1, 1)]);
        let mut dxt = image.clone();
        convert_image(&mut dxt, ImageFormat::Dxt5).unwrap();
        let mipmaps = generate_mip_maps(&dxt, 0, SamplingFilter::Lanczos).unwrap();
        // The small levels of a DXT image are 4x4 blocks.
        assert_eq!((mipmaps[3].width, mipmaps[3].height), (4, 4));
    }

    #[test]
    fn copy_rect_converts_the_source() {
        let image = gradient(8, 8);
        let mut dxt = image.clone();
        convert_image(&mut dxt, ImageFormat::Dxt1).unwrap();
        let mut atlas = new_image(16, 16, ImageFormat::Default).unwrap();
        assert!(copy_rect(&dxt, 0, 0, 8, 8, &mut atlas, 8, 8).unwrap());
        assert_eq!(atlas.bits[0..4], [0, 0, 0, 0]);
        assert_eq!(atlas.bits[(8 * 16 + 9) * 4 + 3], 255);
    }

    #[test]
    fn resize_with_lanczos_nearly_keeps_a_flat_color() {
        let mut image = new_image(8, 8, ImageFormat::Default).unwrap();
        for pixel in image.bits.chunks_mut(4) {
            pixel.copy_from_slice(&[10, 20, 30, 255]);
        }
        resize_image(&mut image, 4, 2, ResizeFilter::Lanczos).unwrap();
        // The weights of the filter are not normalized: a flat color loses
        // a little of its value.
        for pixel in image.bits.chunks(4) {
            assert_eq!(pixel, [10, 20, 30, 253]);
        }
    }

    #[test]
    fn half_floats_convert() {
        for half in [0u16, 0x3C00, 0xBC00, 0x7BFF, 0x0001, 0x0400, 0x7C00] {
            assert_eq!(formats::float_to_half(formats::half_to_float(half)), half);
        }
    }
}
