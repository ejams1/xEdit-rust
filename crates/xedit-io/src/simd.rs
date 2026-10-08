// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Vector kernels for the pixel and number conversions of the texture code,
//! each with a scalar version that gives the same bytes.
//!
//! The rule of the port is that no output depends on the CPU or the thread
//! count: a kernel here is only an alternative route to what its scalar
//! version computes, the scalar version is the definition, and the tests
//! compare the two on every input shape. The environment variable
//! `XEDIT_SIMD=off` makes every kernel take the scalar route, which is how a
//! run on a machine without the vector instructions is reproduced.

use std::sync::OnceLock;

/// The vector instruction set the kernels run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// No vector instructions: the scalar versions.
    Scalar,
    /// x86-64 SSSE3 (byte shuffles).
    Ssse3,
    /// x86-64 AVX2 (byte shuffles on 32 bytes).
    Avx2,
}

/// What the machine offers, without the `XEDIT_SIMD` setting.
pub fn detect() -> Level {
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            return Level::Avx2;
        }
        if std::is_x86_feature_detected!("ssse3") {
            return Level::Ssse3;
        }
    }
    Level::Scalar
}

/// The level the kernels use: `detect`, or `Scalar` when the environment
/// variable `XEDIT_SIMD` is `off`, `0`, `no` or `scalar`. It is read once.
pub fn level() -> Level {
    static LEVEL: OnceLock<Level> = OnceLock::new();
    *LEVEL.get_or_init(|| {
        let off = std::env::var("XEDIT_SIMD").is_ok_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "off" | "0" | "no" | "false" | "scalar"
            )
        });
        if off { Level::Scalar } else { detect() }
    })
}

/// Whether the half-float conversion instructions (F16C with AVX) can be
/// used: the machine has them and `XEDIT_SIMD` is not off.
pub fn f16c() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        level() != Level::Scalar && std::is_x86_feature_detected!("f16c") && std::is_x86_feature_detected!("avx")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Expands 24 bit pixels to 32 bit ones with an opaque fourth byte
/// (`TwbDDS.ConvertR8G8B8toB8G8R8X8`): `dst` holds four bytes for each three
/// bytes of `src`, which must be a multiple of three bytes.
pub fn rgb_to_bgrx(src: &[u8], dst: &mut [u8]) {
    rgb_to_bgrx_at(level(), src, dst);
}

/// `rgb_to_bgrx` on the given level, which is lowered to what the machine
/// can run.
pub fn rgb_to_bgrx_at(level: Level, src: &[u8], dst: &mut [u8]) {
    assert_eq!(src.len() % 3, 0, "whole pixels");
    assert_eq!(dst.len(), src.len() / 3 * 4, "four bytes for each pixel");
    #[cfg(target_arch = "x86_64")]
    {
        let level = level.min(detect());
        // SAFETY: the instruction set was detected on this machine.
        unsafe {
            match level {
                Level::Avx2 => return rgb_to_bgrx_avx2(src, dst),
                Level::Ssse3 => return rgb_to_bgrx_ssse3(src, dst),
                Level::Scalar => {}
            }
        }
    }
    let _ = level;
    rgb_to_bgrx_scalar(src, dst);
}

/// The definition of `rgb_to_bgrx`.
pub fn rgb_to_bgrx_scalar(src: &[u8], dst: &mut [u8]) {
    let (pixels, _) = src.as_chunks::<3>();
    let (outputs, _) = dst.as_chunks_mut::<4>();
    for (pixel, out) in pixels.iter().zip(outputs) {
        out[..3].copy_from_slice(pixel);
        out[3] = 0xFF;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "ssse3")]
unsafe fn rgb_to_bgrx_ssse3(src: &[u8], dst: &mut [u8]) {
    use std::arch::x86_64::{
        __m128i, _mm_loadu_si128, _mm_or_si128, _mm_set1_epi32, _mm_setr_epi8, _mm_shuffle_epi8, _mm_storeu_si128,
    };
    let shuffle = _mm_setr_epi8(0, 1, 2, -1, 3, 4, 5, -1, 6, 7, 8, -1, 9, 10, 11, -1);
    let alpha = _mm_set1_epi32(0xFF00_0000u32 as i32);
    // Four pixels (12 bytes) per step; the load reads 16 bytes, so the last
    // pixels are left to the scalar loop.
    let mut pixels = 0;
    let count = src.len() / 3;
    while pixels + 6 <= count {
        // SAFETY: 3 * pixels + 16 <= src.len() because pixels + 6 <= count,
        // and 4 * pixels + 16 <= dst.len().
        unsafe {
            let input = _mm_loadu_si128(src.as_ptr().add(pixels * 3).cast::<__m128i>());
            let output = _mm_or_si128(_mm_shuffle_epi8(input, shuffle), alpha);
            _mm_storeu_si128(dst.as_mut_ptr().add(pixels * 4).cast::<__m128i>(), output);
        }
        pixels += 4;
    }
    rgb_to_bgrx_scalar(&src[pixels * 3..], &mut dst[pixels * 4..]);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn rgb_to_bgrx_avx2(src: &[u8], dst: &mut [u8]) {
    use std::arch::x86_64::{
        __m128i, __m256i, _mm256_loadu2_m128i, _mm256_or_si256, _mm256_set1_epi32, _mm256_setr_epi8,
        _mm256_shuffle_epi8, _mm256_storeu_si256,
    };
    let shuffle = _mm256_setr_epi8(
        0, 1, 2, -1, 3, 4, 5, -1, 6, 7, 8, -1, 9, 10, 11, -1, 0, 1, 2, -1, 3, 4, 5, -1, 6, 7, 8, -1, 9, 10, 11, -1,
    );
    let alpha = _mm256_set1_epi32(0xFF00_0000u32 as i32);
    // Eight pixels (24 bytes) per step: two 12 byte groups, one in each lane.
    // The second load reads 16 bytes from 12 bytes on.
    let mut pixels = 0;
    let count = src.len() / 3;
    while pixels + 10 <= count {
        // SAFETY: 3 * pixels + 28 <= src.len() because pixels + 10 <= count
        // (the second load starts at 3 * pixels + 12), and
        // 4 * pixels + 32 <= dst.len().
        unsafe {
            let low = src.as_ptr().add(pixels * 3).cast::<__m128i>();
            let high = src.as_ptr().add(pixels * 3 + 12).cast::<__m128i>();
            let input = _mm256_loadu2_m128i(high, low);
            let output = _mm256_or_si256(_mm256_shuffle_epi8(input, shuffle), alpha);
            _mm256_storeu_si256(dst.as_mut_ptr().add(pixels * 4).cast::<__m256i>(), output);
        }
        pixels += 8;
    }
    // SAFETY: AVX2 implies SSSE3, which this function enables too.
    unsafe { rgb_to_bgrx_ssse3(&src[pixels * 3..], &mut dst[pixels * 4..]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pixels in every count from 0 to 70, of bytes that tell the lanes apart.
    #[test]
    fn every_level_gives_the_scalar_bytes() {
        for count in 0..=70usize {
            let src: Vec<u8> = (0..count * 3).map(|i| (i * 37 + 11) as u8).collect();
            let mut want = vec![0u8; count * 4];
            rgb_to_bgrx_scalar(&src, &mut want);
            for level in [Level::Scalar, Level::Ssse3, Level::Avx2] {
                // Stale bytes in the output must not survive.
                let mut got = vec![0x55u8; count * 4];
                rgb_to_bgrx_at(level, &src, &mut got);
                assert_eq!(got, want, "{level:?} with {count} pixels");
            }
        }
    }

    #[test]
    fn the_fourth_byte_is_opaque() {
        let mut out = [0u8; 8];
        rgb_to_bgrx_scalar(&[1, 2, 3, 4, 5, 6], &mut out);
        assert_eq!(out, [1, 2, 3, 0xFF, 4, 5, 6, 0xFF]);
    }
}
