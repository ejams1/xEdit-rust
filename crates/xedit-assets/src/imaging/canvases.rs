// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/ImagingLib/Source/ImagingCanvases.pas (the
// Vampyre Imaging Library by Marek Mauder, at the commit the release links)

//! The point transforms of `TImagingCanvas` that xEdit uses on its atlases:
//! contrast and brightness, and the gamma correction. A canvas works on
//! the whole image through the floating point pixels of its format.

use super::formats::{ColorFP, format_info, get_pixel_fp, set_pixel_fp};
use super::{ImageData, ImagingError};

/// `PointTransform` with a transform of a pixel.
fn point_transform(image: &mut ImageData, transform: impl Fn(&ColorFP) -> ColorFP) -> Result<(), ImagingError> {
    let info = format_info(image.format).ok_or_else(|| ImagingError::new("Invalid image"))?;
    if info.is_special || info.is_indexed {
        return Err(ImagingError::new("Unsupported image format for canvas"));
    }
    let bpp = info.bytes_per_pixel;
    for at in (0..(image.width * image.height) as usize).map(|i| i * bpp) {
        let pixel = get_pixel_fp(&image.bits[at..], info);
        set_pixel_fp(&mut image.bits[at..], info, transform(&pixel));
    }
    Ok(())
}

/// `ModifyContrastBrightness` (`TransformContrastBrightness` with
/// `1 + Contrast / 100` and `Brightness / 100`).
pub fn modify_contrast_brightness(image: &mut ImageData, contrast: f32, brightness: f32) -> Result<(), ImagingError> {
    let c = (1.0 + f64::from(contrast) / 100.0) as f32;
    let b = (f64::from(brightness) / 100.0) as f32;
    point_transform(image, |pixel| {
        let mut result = *pixel;
        for channel in [2, 1, 0] {
            result.channels[channel] = (f64::from(pixel.channels[channel]) * f64::from(c) + f64::from(b)) as f32;
        }
        result
    })
}

/// `Power` of `ImagingUtility`: of singles.
fn power(base: f32, exponent: f32) -> f32 {
    if exponent == 0.0 {
        1.0
    } else if base == 0.0 && exponent > 0.0 {
        0.0
    } else {
        (f64::from(exponent) * f64::from(base).ln()).exp() as f32
    }
}

/// `GammaCorrection` (`TransformGamma`).
pub fn gamma_correction(image: &mut ImageData, red: f32, green: f32, blue: f32) -> Result<(), ImagingError> {
    point_transform(image, |pixel| {
        let mut result = *pixel;
        result.channels[2] = power(pixel.r(), (1.0 / f64::from(red)) as f32);
        result.channels[1] = power(pixel.g(), (1.0 / f64::from(green)) as f32);
        result.channels[0] = power(pixel.b(), (1.0 / f64::from(blue)) as f32);
        result
    })
}
