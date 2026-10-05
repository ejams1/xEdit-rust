// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbCompression.pas

//! Decompression of record data, compressed structures and archive files.
//!
//! Only the decompression half of the upstream unit is ported. Compression
//! comes with the write path, because its output must be byte-identical to
//! libdeflate, zlib and LZ4 HC.

use std::io::Read;

use libdeflater::{DecompressionError, Decompressor};
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
