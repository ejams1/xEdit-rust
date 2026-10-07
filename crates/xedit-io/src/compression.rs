// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbCompression.pas

//! Decompression of record data, compressed structures and archive files,
//! and the zlib compression of record data for the write path.
//!
//! LZ4 and LZ4F compression (archives) are not ported yet.

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

    /// Port of `TwbCompression.Compress` for record data: zlib through
    /// libdeflate at its highest level for data up to
    /// `LIBDEFLATE_MAX_DATASIZE`, zlib level 9 beyond.
    ///
    /// UPSTREAM-QUIRK: above 8 MiB upstream switches to System.ZLib's
    /// `compress2` at level 9, whose deflate stream differs from libdeflate's.
    /// The port stays with libdeflate at level 9 there; a record that large is
    /// not known in any game file, so the difference has not been observed.
    pub fn compress(self, src: &[u8]) -> Result<Vec<u8>, CompressionError> {
        match self {
            CompressionType::ZLib => {
                let level = if src.len() <= LIBDEFLATE_MAX_DATASIZE {
                    LIBDEFLATE_COMPRESSION_LEVEL
                } else {
                    ZLIB_COMPRESSION_LEVEL
                };
                let level = CompressionLvl::new(level)
                    .map_err(|_| CompressionError(format!("LibDeflate error: invalid level {level}")))?;
                let mut compressor = Compressor::new(level);
                let mut out = vec![0u8; compressor.zlib_compress_bound(src.len())];
                let size = compressor
                    .zlib_compress(src, &mut out)
                    .map_err(|error| CompressionError(format!("LibDeflate error: {error:?}")))?;
                out.truncate(size);
                Ok(out)
            }
            _ => Err(CompressionError(format!(
                "Compression type {} is not ported yet",
                self.name()
            ))),
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
