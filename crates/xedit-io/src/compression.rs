// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbCompression.pas

//! Decompression of record data, compressed structures and archive files,
//! and the compression of record data and archive files for the write path:
//! zlib (libdeflate, and zlib itself beyond 8 MiB), LZ4 (HC) and LZ4F.

use std::io::Read;

use libdeflater::{CompressionLvl, Compressor, DecompressionError, Decompressor};
use lz4_flex::frame::FrameDecoder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    None,
    ZLib,
    LZ4,
    LZ4F,
}

/// A decompression failure. The message is the upstream exception message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CompressionError(pub String);

/// Upstream `LIBDEFLATE_MAX_DATASIZE`: libdeflate compresses data up to this
/// size, zlib beyond it.
const LIBDEFLATE_MAX_DATASIZE: usize = 8 * 1024 * 1024;
/// Upstream `LIBDEFLATE_COMPRESSION_LEVEL`: the highest libdeflate level.
const LIBDEFLATE_COMPRESSION_LEVEL: i32 = 12;
/// Upstream `ZLIB_COMPRESSION_LEVEL`: `Z_BEST_COMPRESSION`.
const ZLIB_COMPRESSION_LEVEL: i32 = 9;
/// Upstream `LZ4_COMPRESSION_LEVEL`: `LZ4HC_CLEVEL_MAX`.
const LZ4_COMPRESSION_LEVEL: i32 = 12;

const COMPRESSION_TYPE_NAME: [(CompressionType, &str); 4] = [
    (CompressionType::None, "None"),
    (CompressionType::ZLib, "ZLib"),
    (CompressionType::LZ4, "LZ4"),
    (CompressionType::LZ4F, "LZ4F"),
];

impl CompressionType {
    pub fn name(self) -> &'static str {
        COMPRESSION_TYPE_NAME
            .iter()
            .find(|(compression, _)| *compression == self)
            .map_or("", |(_, name)| name)
    }

    /// The type called `name`, ignoring case.
    pub fn type_by_name(name: &str) -> Result<Self, CompressionError> {
        COMPRESSION_TYPE_NAME
            .iter()
            .find(|(_, candidate)| candidate.eq_ignore_ascii_case(name))
            .map(|(compression, _)| *compression)
            .ok_or_else(|| CompressionError(format!("Unknown compression type: {name}")))
    }

    /// Port of `TwbCompression.Compress` at the default level of each type:
    /// zlib through libdeflate at its highest level for data up to
    /// `LIBDEFLATE_MAX_DATASIZE` and through zlib's `compress2` at level 9
    /// beyond, LZ4 block and frame compression at the highest LZ4HC level.
    /// The output equals upstream's byte for byte as long as the libraries
    /// produce the same streams, which `cargo xtask parity bsarch` checks
    /// against `BSArch.exe`.
    pub fn compress(self, src: &[u8]) -> Result<Vec<u8>, CompressionError> {
        match self {
            CompressionType::ZLib if src.len() <= LIBDEFLATE_MAX_DATASIZE => {
                lib_deflate_compress(src, LIBDEFLATE_COMPRESSION_LEVEL)
            }
            CompressionType::ZLib => zlib_compress(src, ZLIB_COMPRESSION_LEVEL),
            CompressionType::LZ4 => lz4_compress(src, LZ4_COMPRESSION_LEVEL),
            CompressionType::LZ4F => lz4f_compress(src, LZ4_COMPRESSION_LEVEL),
            CompressionType::None => Err(CompressionError("Undefined compression type".to_owned())),
        }
    }

    /// Decompresses `src` into `dst`. The decompressed data must fill `dst` exactly.
    pub fn decompress(self, src: &[u8], dst: &mut [u8]) -> Result<(), CompressionError> {
        match self {
            CompressionType::ZLib => lib_deflate_decompress(src, dst),
            CompressionType::LZ4 => lz4_decompress(src, dst),
            CompressionType::LZ4F => lz4f_decompress(src, dst),
            CompressionType::None => Err(CompressionError("Undefined decompression type".to_owned())),
        }
    }
}

/// Port of `LibDeflateCompress`.
fn lib_deflate_compress(src: &[u8], level: i32) -> Result<Vec<u8>, CompressionError> {
    let level =
        CompressionLvl::new(level).map_err(|_| CompressionError(format!("LibDeflate error: invalid level {level}")))?;
    let mut compressor = Compressor::new(level);
    let mut out = vec![0u8; compressor.zlib_compress_bound(src.len())];
    let size = compressor
        .zlib_compress(src, &mut out)
        .map_err(|_| CompressionError("LibDeflate error: Compression failed".to_owned()))?;
    out.truncate(size);
    Ok(out)
}

/// Port of `ZLibCompress`: zlib's `compress2`, the deflate stream of the C
/// library (default window and memory level).
fn zlib_compress(src: &[u8], level: i32) -> Result<Vec<u8>, CompressionError> {
    let failed = |error: &dyn std::fmt::Display| CompressionError(format!("ZLib error: {error}"));
    let level = u32::try_from(level).map_err(|error| failed(&error))?;
    let mut compressor = flate2::Compress::new(flate2::Compression::new(level), true);
    // `compressBound`.
    let bound = src.len() + (src.len() >> 12) + (src.len() >> 14) + (src.len() >> 25) + 13;
    let mut out = Vec::with_capacity(bound);
    match compressor
        .compress_vec(src, &mut out, flate2::FlushCompress::Finish)
        .map_err(|error| failed(&error))?
    {
        flate2::Status::StreamEnd => Ok(out),
        status => Err(failed(&format!("{status:?}"))),
    }
}

/// Port of `LZ4Compress`: `LZ4_compress_HC`.
fn lz4_compress(src: &[u8], level: i32) -> Result<Vec<u8>, CompressionError> {
    let failed = || CompressionError("LZ4 error: Compression failed".to_owned());
    let size = i32::try_from(src.len()).map_err(|_| failed())?;
    // SAFETY: `LZ4_compressBound` only reads its argument.
    let bound = unsafe { lz4_sys::LZ4_compressBound(size) };
    let mut out = vec![0u8; usize::try_from(bound).map_err(|_| failed())?];
    // Upstream: LZ4_compress_HC crashes on a null source of size 0, so an
    // empty source points into the allocated output.
    let source = if src.is_empty() { out.as_ptr() } else { src.as_ptr() };
    // SAFETY: `source` is valid for `size` bytes and `out` for `bound` bytes.
    let written = unsafe { lz4_sys::LZ4_compress_HC(source.cast(), out.as_mut_ptr().cast(), size, bound, level) };
    if written <= 0 {
        return Err(failed());
    }
    out.truncate(written as usize);
    Ok(out)
}

unsafe extern "C" {
    // Part of the lz4 library that `lz4-sys` builds; the crate does not declare it.
    fn LZ4F_compressFrameBound(src_size: usize, preferences: *const lz4_sys::LZ4FPreferences) -> usize;
    fn LZ4F_compressFrame(
        dst: *mut std::ffi::c_void,
        dst_capacity: usize,
        src: *const std::ffi::c_void,
        src_size: usize,
        preferences: *const lz4_sys::LZ4FPreferences,
    ) -> usize;
}

/// Port of `LZ4FCompress`: `LZ4F_compressFrame` with independent 4 MB
/// blocks, auto flush and no checksums.
fn lz4f_compress(src: &[u8], level: i32) -> Result<Vec<u8>, CompressionError> {
    use lz4_sys::{BlockChecksum, BlockMode, BlockSize, ContentChecksum, FrameType, LZ4FFrameInfo, LZ4FPreferences};
    let preferences = LZ4FPreferences {
        frame_info: LZ4FFrameInfo {
            block_size_id: BlockSize::Max4MB,
            block_mode: BlockMode::Independent,
            content_checksum_flag: ContentChecksum::NoChecksum,
            frame_type: FrameType::Frame,
            content_size: 0,
            dict_id: 0,
            block_checksum_flag: BlockChecksum::NoBlockChecksum,
        },
        compression_level: level as u32,
        auto_flush: 1,
        favor_dec_speed: 0,
        reserved: [0; 3],
    };
    let failed = |code: usize| {
        // SAFETY: the error name is a static C string.
        let name = unsafe { std::ffi::CStr::from_ptr(lz4_sys::LZ4F_getErrorName(code)) };
        CompressionError(format!("LZ4F compression error: {}", name.to_string_lossy()))
    };
    // SAFETY: `preferences` is a valid LZ4F_preferences_t.
    let bound = unsafe { LZ4F_compressFrameBound(src.len(), &preferences) };
    let mut out = vec![0u8; bound];
    // SAFETY: `src` is valid for its length, `out` for `bound` bytes.
    let written = unsafe {
        LZ4F_compressFrame(
            out.as_mut_ptr().cast(),
            out.len(),
            src.as_ptr().cast(),
            src.len(),
            &preferences,
        )
    };
    // SAFETY: `LZ4F_isError` only reads its argument.
    if unsafe { lz4_sys::LZ4F_isError(written) } != 0 {
        return Err(failed(written));
    }
    out.truncate(written);
    Ok(out)
}

fn lib_deflate_decompress(src: &[u8], dst: &mut [u8]) -> Result<(), CompressionError> {
    let expected = dst.len();
    match Decompressor::new().zlib_decompress(src, dst) {
        Ok(size) if size == expected => Ok(()),
        Ok(_) => Err(CompressionError("LibDeflate error: Short output".to_owned())),
        Err(DecompressionError::BadData) => Err(CompressionError("LibDeflate error: Bad data".to_owned())),
        Err(DecompressionError::InsufficientSpace) => {
            Err(CompressionError("LibDeflate error: Insufficient space".to_owned()))
        }
    }
}

fn lz4_decompress(src: &[u8], dst: &mut [u8]) -> Result<(), CompressionError> {
    match lz4_flex::block::decompress_into(src, dst) {
        Ok(size) if size == dst.len() => Ok(()),
        _ => Err(CompressionError("LZ4 error: Decompression failed".to_owned())),
    }
}

fn lz4f_decompress(src: &[u8], dst: &mut [u8]) -> Result<(), CompressionError> {
    let mut decoder = FrameDecoder::new(src);
    decoder
        .read_exact(dst)
        .map_err(|error| CompressionError(format!("LZ4F decompression error: {error}")))?;
    // Upstream requires that the frame ends exactly where the output is full.
    let mut rest = [0u8; 1];
    match decoder.read(&mut rest) {
        Ok(0) if decoder.into_inner().is_empty() => Ok(()),
        Ok(_) => Err(CompressionError(
            "LZ4F decompression error: processed bytes don't match".to_owned(),
        )),
        Err(error) => Err(CompressionError(format!("LZ4F decompression error: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &[u8] = b"xEdit xEdit xEdit xEdit xEdit xEdit xEdit xEdit";

    #[test]
    fn zlib_compression_round_trips() {
        let compressed = CompressionType::ZLib.compress(TEXT).unwrap();
        let mut out = vec![0u8; TEXT.len()];
        CompressionType::ZLib.decompress(&compressed, &mut out).unwrap();
        assert_eq!(out, TEXT);
    }

    /// Text-like data of `length` bytes that compresses, but not trivially.
    fn sample(length: usize) -> Vec<u8> {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                b'a' + (state % 7) as u8
            })
            .collect()
    }

    #[test]
    fn every_type_compresses_and_decompresses() {
        for length in [0, 1, 100, 70_000, 600_000] {
            let data = sample(length);
            for kind in [CompressionType::ZLib, CompressionType::LZ4, CompressionType::LZ4F] {
                let packed = kind.compress(&data).unwrap();
                let mut out = vec![0u8; data.len()];
                kind.decompress(&packed, &mut out)
                    .unwrap_or_else(|error| panic!("{} of {length} bytes: {error}", kind.name()));
                assert_eq!(out, data, "{} of {length} bytes", kind.name());
            }
        }
        assert!(CompressionType::None.compress(TEXT).is_err());
    }

    #[test]
    fn zlib_beyond_the_libdeflate_limit_is_a_zlib_stream() {
        let data = sample(LIBDEFLATE_MAX_DATASIZE + 1000);
        let packed = CompressionType::ZLib.compress(&data).unwrap();
        // The zlib header of level 9 with the default window.
        assert_eq!(&packed[..2], &[0x78, 0xDA]);
        let mut out = vec![0u8; data.len()];
        CompressionType::ZLib.decompress(&packed, &mut out).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn names_round_trip() {
        assert_eq!(CompressionType::LZ4F.name(), "LZ4F");
        assert_eq!(CompressionType::type_by_name("zlib"), Ok(CompressionType::ZLib));
        assert!(CompressionType::type_by_name("brotli").is_err());
    }

    #[test]
    fn zlib() {
        let mut compressor = libdeflater::Compressor::new(libdeflater::CompressionLvl::best());
        let mut packed = vec![0u8; compressor.zlib_compress_bound(TEXT.len())];
        let size = compressor.zlib_compress(TEXT, &mut packed).unwrap();
        let mut out = vec![0u8; TEXT.len()];
        CompressionType::ZLib.decompress(&packed[..size], &mut out).unwrap();
        assert_eq!(out, TEXT);

        let mut long = vec![0u8; TEXT.len() + 1];
        assert_eq!(
            CompressionType::ZLib.decompress(&packed[..size], &mut long),
            Err(CompressionError("LibDeflate error: Short output".to_owned()))
        );
        let mut short = vec![0u8; TEXT.len() - 1];
        assert_eq!(
            CompressionType::ZLib.decompress(&packed[..size], &mut short),
            Err(CompressionError("LibDeflate error: Insufficient space".to_owned()))
        );
        assert_eq!(
            CompressionType::ZLib.decompress(b"not zlib", &mut out),
            Err(CompressionError("LibDeflate error: Bad data".to_owned()))
        );
    }

    #[test]
    fn lz4_block() {
        let packed = lz4_flex::block::compress(TEXT);
        let mut out = vec![0u8; TEXT.len()];
        CompressionType::LZ4.decompress(&packed, &mut out).unwrap();
        assert_eq!(out, TEXT);
        let mut long = vec![0u8; TEXT.len() + 1];
        assert!(CompressionType::LZ4.decompress(&packed, &mut long).is_err());
    }

    #[test]
    fn lz4_frame() {
        use std::io::Write;
        let mut encoder = lz4_flex::frame::FrameEncoder::new(Vec::new());
        encoder.write_all(TEXT).unwrap();
        let packed = encoder.finish().unwrap();
        let mut out = vec![0u8; TEXT.len()];
        CompressionType::LZ4F.decompress(&packed, &mut out).unwrap();
        assert_eq!(out, TEXT);
        let mut short = vec![0u8; TEXT.len() - 1];
        assert!(CompressionType::LZ4F.decompress(&packed, &mut short).is_err());
    }

    #[test]
    fn none_is_not_a_decompression() {
        assert!(CompressionType::None.decompress(TEXT, &mut [0u8; 4]).is_err());
    }
}
