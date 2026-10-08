// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Half-float conversion of whole arrays (vertex positions, UVs and pixels),
//! with vector instructions where the machine has them.
//!
//! The scalar versions are `delphi::half_to_float` and
//! `delphi::single_to_half`, which are xEdit's `HalfToFloat` and
//! `FloatToHalf`; the vector routines give their bits for every input, so
//! the result never depends on the CPU. Where an instruction is not exactly
//! the Pascal routine (a signalling NaN is quieted by `vcvtph2ps`, and
//! `vcvtps2ph` saturates what `FloatToHalf` turns into infinity), a block of
//! eight values that holds such a value is converted by the scalar routine.

use crate::delphi::{half_to_float, single_to_half};

/// `half_to_float` of every element: `dst[i] = half_to_float(src[i])`. The
/// slices must have the same length.
pub fn half_to_float_slice(src: &[u16], dst: &mut [f32]) {
    half_to_float_slice_at(xedit_io::simd::f16c(), src, dst);
}

/// `half_to_float_slice`, with the vector instructions asked for. They are
/// used only when the machine has them.
pub fn half_to_float_slice_at(vector: bool, src: &[u16], dst: &mut [f32]) {
    assert_eq!(src.len(), dst.len(), "one float for each half");
    let done = {
        #[cfg(target_arch = "x86_64")]
        {
            if vector && xedit_io::simd::f16c() {
                // SAFETY: F16C and AVX were detected on this machine.
                unsafe { half_to_float_f16c(src, dst) }
            } else {
                0
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = vector;
            0
        }
    };
    for (half, float) in src[done..].iter().zip(&mut dst[done..]) {
        *float = half_to_float(*half);
    }
}

/// `single_to_half` of every element: `dst[i] = single_to_half(src[i])`. The
/// slices must have the same length.
pub fn single_to_half_slice(src: &[f32], dst: &mut [u16]) {
    single_to_half_slice_at(xedit_io::simd::f16c(), src, dst);
}

/// `single_to_half_slice`, with the vector instructions asked for.
pub fn single_to_half_slice_at(vector: bool, src: &[f32], dst: &mut [u16]) {
    assert_eq!(src.len(), dst.len(), "one half for each float");
    let done = {
        #[cfg(target_arch = "x86_64")]
        {
            if vector && xedit_io::simd::f16c() {
                // SAFETY: F16C and AVX were detected on this machine.
                unsafe { single_to_half_f16c(src, dst) }
            } else {
                0
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = vector;
            0
        }
    };
    for (float, half) in src[done..].iter().zip(&mut dst[done..]) {
        *half = single_to_half(*float);
    }
}

/// Converts the blocks of eight halves that need no scalar help and returns
/// the number of values done (the rest is the caller's).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx,f16c")]
unsafe fn half_to_float_f16c(src: &[u16], dst: &mut [f32]) -> usize {
    use std::arch::x86_64::{
        __m128i, _mm_and_si128, _mm_cmpgt_epi16, _mm_loadu_si128, _mm_movemask_epi8, _mm_set1_epi16, _mm256_cvtph_ps,
        _mm256_storeu_ps,
    };
    let mut index = 0;
    let magnitude = _mm_set1_epi16(0x7FFF);
    let infinity = _mm_set1_epi16(0x7C00);
    while index + 8 <= src.len() {
        // SAFETY: eight values are in both slices from `index`.
        unsafe {
            let halves = _mm_loadu_si128(src.as_ptr().add(index).cast::<__m128i>());
            // A NaN is above infinity in magnitude; `vcvtph2ps` quiets the
            // signalling ones, so the block is left to the scalar routine.
            let nan = _mm_cmpgt_epi16(_mm_and_si128(halves, magnitude), infinity);
            if _mm_movemask_epi8(nan) == 0 {
                _mm256_storeu_ps(dst.as_mut_ptr().add(index), _mm256_cvtph_ps(halves));
            } else {
                for lane in index..index + 8 {
                    dst[lane] = half_to_float(src[lane]);
                }
            }
        }
        index += 8;
    }
    index
}

/// Converts the blocks of eight floats that need no scalar help and returns
/// the number of values done.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,f16c")]
unsafe fn single_to_half_f16c(src: &[f32], dst: &mut [u16]) -> usize {
    use std::arch::x86_64::{
        __m128i, _mm_storeu_si128, _mm256_and_si256, _mm256_castps_si256, _mm256_cmpgt_epi32, _mm256_cvtps_ph,
        _mm256_loadu_ps, _mm256_movemask_epi8, _mm256_set1_epi32,
    };
    let mut index = 0;
    let magnitude = _mm256_set1_epi32(0x7FFF_FFFF);
    // The largest single that converts without overflowing to infinity,
    // 65535.99..: everything above (with infinities and NaNs) is the scalar
    // routine's, which makes infinity of it where `vcvtps2ph` saturates.
    let below_overflow = _mm256_set1_epi32(0x477F_FFFF);
    while index + 8 <= src.len() {
        // SAFETY: eight values are in both slices from `index`.
        unsafe {
            let floats = _mm256_loadu_ps(src.as_ptr().add(index));
            let large = _mm256_cmpgt_epi32(_mm256_and_si256(_mm256_castps_si256(floats), magnitude), below_overflow);
            if _mm256_movemask_epi8(large) == 0 {
                // Rounding control 3: toward zero, as the mantissa of `FloatToHalf`
                // is cut, whatever MXCSR says.
                let halves = _mm256_cvtps_ph::<0x3>(floats);
                _mm_storeu_si128(dst.as_mut_ptr().add(index).cast::<__m128i>(), halves);
            } else {
                for lane in index..index + 8 {
                    dst[lane] = single_to_half(src[lane]);
                }
            }
        }
        index += 8;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A xorshift generator, so the tests need no crate.
    struct Random(u64);

    impl Random {
        fn next(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 16) as u32
        }
    }

    fn scalar_halves(src: &[u16]) -> Vec<u32> {
        src.iter().map(|&half| half_to_float(half).to_bits()).collect()
    }

    #[test]
    fn every_half_converts_to_the_scalar_bits() {
        let src: Vec<u16> = (0..=u16::MAX).collect();
        let want = scalar_halves(&src);
        for vector in [false, true] {
            let mut got = vec![0f32; src.len()];
            half_to_float_slice_at(vector, &src, &mut got);
            let got: Vec<u32> = got.iter().map(|value| value.to_bits()).collect();
            assert_eq!(got, want, "vector: {vector}");
        }
    }

    /// Floats of every exponent and of the edges of the half range, then
    /// random ones.
    fn sample_floats() -> Vec<f32> {
        let mut values = Vec::new();
        for sign in [0u32, 0x8000_0000] {
            for exponent in 0..=255u32 {
                for mantissa in [
                    0u32, 1, 2, 0x1FFF, 0x2000, 0x2001, 0x3FFF, 0x4000, 0x1000, 0x7FE000, 0x7FFFFF, 0x7FDFFF, 0x400000,
                    0x200000, 0x000001,
                ] {
                    values.push(f32::from_bits(sign | (exponent << 23) | mantissa));
                }
            }
        }
        values.extend([
            65504.0,
            65519.9,
            65520.0,
            65535.0,
            65535.99,
            65536.0,
            1e9,
            -65536.0,
            f32::MAX,
            f32::MIN,
        ]);
        let mut random = Random(0x9E37_79B9_7F4A_7C15);
        for _ in 0..2_000_000 {
            values.push(f32::from_bits(random.next() ^ (random.next() << 16)));
        }
        values
    }

    #[test]
    fn floats_convert_to_the_scalar_halves() {
        let src = sample_floats();
        let want: Vec<u16> = src.iter().map(|&value| single_to_half(value)).collect();
        for vector in [false, true] {
            let mut got = vec![0u16; src.len()];
            single_to_half_slice_at(vector, &src, &mut got);
            assert!(got == want, "vector: {vector}");
        }
    }

    #[test]
    fn lengths_that_are_not_blocks_are_done() {
        for length in 0..40usize {
            let src: Vec<f32> = (0..length).map(|i| i as f32 * 1.37 - 9.0).collect();
            let want: Vec<u16> = src.iter().map(|&value| single_to_half(value)).collect();
            let mut got = vec![0u16; length];
            single_to_half_slice(&src, &mut got);
            assert_eq!(got, want);
            let mut back = vec![0f32; length];
            half_to_float_slice(&got, &mut back);
            let want: Vec<f32> = got.iter().map(|&half| half_to_float(half)).collect();
            assert_eq!(back, want);
        }
    }

    /// Every one of the 2^32 floats: `cargo test -p xedit-core --release --
    /// --ignored every_float`.
    #[test]
    #[ignore = "takes a few seconds in a release build"]
    fn every_float_converts_to_the_scalar_half() {
        const BLOCK: usize = 1 << 20;
        let mut src = vec![0f32; BLOCK];
        let mut got = vec![0u16; BLOCK];
        for start in (0..=u32::MAX as u64).step_by(BLOCK) {
            for (offset, value) in src.iter_mut().enumerate() {
                *value = f32::from_bits((start as u32).wrapping_add(offset as u32));
            }
            single_to_half_slice_at(true, &src, &mut got);
            for (offset, value) in src.iter().enumerate() {
                assert_eq!(got[offset], single_to_half(*value), "bits {:#010x}", value.to_bits());
            }
        }
    }
}
