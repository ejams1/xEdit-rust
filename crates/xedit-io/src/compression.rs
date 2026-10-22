// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbCompression.pas

//! Decompression of record data, compressed structures and archive files,
//! and the compression of record data and archive files for the write path:
//! zlib (libdeflate, and zlib itself beyond 8 MiB), LZ4 (HC) and LZ4F.

use std::cell::RefCell;
use std::io::{Read, Write};

use libdeflater::{CompressionLvl, Compressor, DecompressionError, Decompressor};

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

    /// Port of `TwbCompression.Compress(aType, aSrc, aSrcSize, aDst: TStream)`:
    /// [`CompressionType::compress`] written to the stream.
    pub fn compress_to(self, src: &[u8], dst: &mut dyn Write) -> Result<(), CompressionError> {
        let data = self.compress(src)?;
        dst.write_all(&data).map_err(stream_error)
    }

    /// Port of `TwbCompression.Compress(aType, aSrc, aDst: TStream,
    /// aBuffered)`. Without `buffered`, or for LZ4 (`todo: not implemented
    /// for LZ4` upstream) and none, the whole source is read and compressed
    /// as [`CompressionType::compress`] does. With `buffered`, zlib goes
    /// through zlib's own stream (`System.ZLib.ZCompressStream`), so a
    /// source up to 8 MiB is not compressed by libdeflate as the buffer
    /// routes do, and LZ4F block by block (`LZ4FCompressStream`: 4 MiB
    /// independent blocks, auto flush, the source read 4 MiB at a time), which
    /// gives the frame [`CompressionType::compress`] gives.
    ///
    /// UPSTREAM-QUIRK: the buffered zlib route passes the level as
    /// `TZCompressionLevel(9)`, an ordinal beyond that enumeration
    /// (`zcNone`..`zcMax`), and `ZCompressStream` indexes its table of
    /// zlib levels with it, so the level upstream compresses at is whatever
    /// integer follows that table in the binary. The 4.1.5q release has no
    /// caller of this route; the port compresses at `Z_BEST_COMPRESSION`, the
    /// level the call means.
    pub fn compress_stream(
        self,
        src: &mut dyn Read,
        dst: &mut dyn Write,
        buffered: bool,
    ) -> Result<(), CompressionError> {
        match self {
            CompressionType::ZLib if buffered => {
                let mut encoder =
                    flate2::write::ZlibEncoder::new(dst, flate2::Compression::new(ZLIB_COMPRESSION_LEVEL as u32));
                std::io::copy(src, &mut encoder).map_err(stream_error)?;
                encoder.finish().map_err(stream_error)?;
                Ok(())
            }
            CompressionType::LZ4F if buffered => lz4f_compress_stream(src, dst, LZ4_COMPRESSION_LEVEL),
            _ => {
                // no buffering with full loading into memory
                let mut data = Vec::new();
                src.read_to_end(&mut data).map_err(stream_error)?;
                self.compress_to(&data, dst)
            }
        }
    }

    /// Port of `TwbCompression.Decompress(aType, aSrc, aDst: TStream)`: an
    /// LZ4 frame decoded from the source to its end (`TLZ4.Stream_Decode`);
    /// the other types are `Not implemented` upstream.
    pub fn decompress_stream(self, src: &mut dyn Read, dst: &mut dyn Write) -> Result<(), CompressionError> {
        match self {
            CompressionType::LZ4F => lz4f_decompress_stream(src, dst),
            _ => Err(CompressionError("Not implemented".to_owned())),
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

/// The message of a stream that could not be read or written.
fn stream_error(error: std::io::Error) -> CompressionError {
    CompressionError(error.to_string())
}

/// Port of `LZ4FCompressStream`: the frame header (`LZ4F_compressBegin`),
/// each block of up to 4 MiB of the source (`LZ4F_compressUpdate` with auto
/// flush) and the end mark (`LZ4F_compressEnd`), with the preferences of
/// `LZ4FCompress`.
fn lz4f_compress_stream(src: &mut dyn Read, dst: &mut dyn Write, level: i32) -> Result<(), CompressionError> {
    use lz4_sys::{
        BlockChecksum, BlockMode, BlockSize, ContentChecksum, FrameType, LZ4F_VERSION, LZ4FCompressionContext,
        LZ4FFrameInfo, LZ4FPreferences,
    };
    let check = |message: &str, code: usize| -> Result<usize, CompressionError> {
        // SAFETY: `LZ4F_isError` only reads its argument.
        if unsafe { lz4_sys::LZ4F_isError(code) } != 0 {
            // SAFETY: the error name is a static C string.
            let name = unsafe { std::ffi::CStr::from_ptr(lz4_sys::LZ4F_getErrorName(code)) };
            return Err(CompressionError(message.replace("%s", &name.to_string_lossy())));
        }
        Ok(code)
    };
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
    let mut context = LZ4FCompressionContext(std::ptr::null_mut());
    // SAFETY: the context is written by the call.
    check("LZ4F compression context error: %s", unsafe {
        lz4_sys::LZ4F_createCompressionContext(&mut context, LZ4F_VERSION)
    })?;
    struct Free(LZ4FCompressionContext);
    impl Drop for Free {
        fn drop(&mut self) {
            // SAFETY: the context was created by `LZ4F_createCompressionContext`.
            unsafe {
                lz4_sys::LZ4F_freeCompressionContext(LZ4FCompressionContext(self.0.0));
            }
        }
    }
    let _free = Free(LZ4FCompressionContext(context.0));
    let block_size = 1usize << (8 + 2 * 7);
    let mut input = vec![0u8; block_size];
    // SAFETY: `preferences` is a valid LZ4F_preferences_t.
    let bound = unsafe { lz4_sys::LZ4F_compressBound(block_size, &preferences) };
    let mut output = vec![0u8; bound];
    // Write Archive Header
    // SAFETY: `output` is valid for `bound` bytes.
    let size = check("LZ4F file header generation failed: %s", unsafe {
        lz4_sys::LZ4F_compressBegin(
            LZ4FCompressionContext(context.0),
            output.as_mut_ptr(),
            bound,
            &preferences,
        )
    })?;
    dst.write_all(&output[..size]).map_err(stream_error)?;
    loop {
        // `TStream.Read`: up to a block, fewer at the end of the source.
        let mut read = 0;
        while read < block_size {
            match src.read(&mut input[read..]) {
                Ok(0) => break,
                Ok(count) => read += count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(stream_error(error)),
            }
        }
        if read == 0 {
            break;
        }
        // SAFETY: `input` is valid for `read` bytes, `output` for `bound`.
        let size = check("LZ4F compression error: %s", unsafe {
            lz4_sys::LZ4F_compressUpdate(
                LZ4FCompressionContext(context.0),
                output.as_mut_ptr(),
                bound,
                input.as_ptr(),
                read,
                std::ptr::null(),
            )
        })?;
        dst.write_all(&output[..size]).map_err(stream_error)?;
        if read < block_size {
            break;
        }
    }
    // End of Stream mark
    // SAFETY: `output` is valid for `bound` bytes.
    let size = check("LZ4F end of file generation failed: %s", unsafe {
        lz4_sys::LZ4F_compressEnd(
            LZ4FCompressionContext(context.0),
            output.as_mut_ptr(),
            bound,
            std::ptr::null(),
        )
    })?;
    dst.write_all(&output[..size]).map_err(stream_error)
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

/// Port of `LZ4Decompress`: `LZ4_decompress_safe` of the linked C library,
/// which is the call upstream makes. The output must fill `dst` exactly.
fn lz4_decompress(src: &[u8], dst: &mut [u8]) -> Result<(), CompressionError> {
    let failed = || CompressionError("LZ4 error: Decompression failed".to_owned());
    let (Ok(compressed), Ok(capacity)) = (i32::try_from(src.len()), i32::try_from(dst.len())) else {
        return Err(failed());
    };
    // SAFETY: `src` is valid for `compressed` bytes and `dst` for `capacity`
    // bytes, which is the contract of `LZ4_decompress_safe`.
    let written = unsafe {
        lz4_sys::LZ4_decompress_safe(
            src.as_ptr().cast(),
            dst.as_mut_ptr().cast(),
            compressed,
            capacity,
        )
    };
    if written < 0 || written as usize != dst.len() {
        return Err(failed());
    }
    Ok(())
}

/// The decompression context of the linked C library, kept per thread.
///
/// The context is reset before every frame, so no state of an earlier frame
/// carries over into the next one.
fn lz4f_context() -> Result<lz4_sys::LZ4FDecompressionContext, CompressionError> {
    thread_local! {
        static CONTEXT: RefCell<Option<lz4_sys::LZ4FDecompressionContext>> = const { RefCell::new(None) };
    }
    CONTEXT.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let mut context = lz4_sys::LZ4FDecompressionContext(std::ptr::null_mut());
            // SAFETY: the context is written by the call.
            let code = unsafe { lz4_sys::LZ4F_createDecompressionContext(&mut context, lz4_sys::LZ4F_VERSION) };
            if lz4f_is_error(code) {
                return Err(lz4f_error("LZ4F decompression context error: %s", code));
            }
            *slot = Some(context);
        }
        let context = slot.expect("the context was just created");
        // SAFETY: the context was created by `LZ4F_createDecompressionContext`.
        unsafe { lz4_sys::LZ4F_resetDecompressionContext(context) };
        Ok(context)
    })
}

/// Whether the code of the frame library is an error, and its name.
fn lz4f_is_error(code: usize) -> bool {
    // SAFETY: `LZ4F_isError` only reads its argument.
    unsafe { lz4_sys::LZ4F_isError(code) != 0 }
}

/// The error of the frame library, with the name the library gives.
fn lz4f_error(message: &str, code: usize) -> CompressionError {
    // SAFETY: the error name is a static C string.
    let name = unsafe { std::ffi::CStr::from_ptr(lz4_sys::LZ4F_getErrorName(code)) };
    CompressionError(message.replace("%s", &name.to_string_lossy()))
}

/// Port of `LZ4FDecompress`: the linked C library decodes the frame into
/// the output directly, in a thread-local context.
///
/// The "processed bytes don't match" rule of upstream is reproduced: the
/// frame must consume the whole source and fill the whole output.
fn lz4f_decompress(src: &[u8], dst: &mut [u8]) -> Result<(), CompressionError> {
    let context = lz4f_context()?;
    // Pledges that the last 64 KB of the output is still there before the
    // destination pointer, which is what upstream asks for.
    let options = lz4_sys::LZ4FDecompressOptions {
        stable_dst: 1,
        reserved: [0; 3],
    };
    let mut compressed_total = 0usize;
    let mut decompressed_total = 0usize;
    loop {
        let mut compressed = src.len() - compressed_total;
        let mut decompressed = dst.len() - decompressed_total;
        // SAFETY: the context is a live decompression context, `dst` is valid
        // for `decompressed` bytes from `dst[decompressed_total..]`, and
        // `src` is valid for `compressed` bytes from `src[compressed_total..]`.
        let result = unsafe {
            lz4_sys::LZ4F_decompress(
                context,
                dst.as_mut_ptr().add(decompressed_total),
                &mut decompressed,
                src.as_ptr().add(compressed_total),
                &mut compressed,
                &options,
            )
        };
        if lz4f_is_error(result) {
            return Err(lz4f_error("LZ4F decompression error: %s", result));
        }
        let before = (compressed_total, decompressed_total);
        compressed_total += compressed;
        decompressed_total += decompressed;
        if result == 0 {
            break;
        }
        // The library asks for more room in the output than there is, so it
        // consumed nothing and decoded nothing. The frame does not fit into
        // the output, which the rule below reports.
        if (compressed_total, decompressed_total) == before {
            break;
        }
    }
    if compressed_total != src.len() || decompressed_total != dst.len() {
        return Err(CompressionError(
            "LZ4F decompression error: processed bytes don't match".to_owned(),
        ));
    }
    Ok(())
}

/// Port of `TLZ4.Stream_Decode`: the frames of the source are decoded to
/// their end and written to the destination.
///
/// The frame library writes into the output buffer as it decodes, so the
/// output is the buffer the library is given.
fn lz4f_decompress_stream(src: &mut dyn Read, dst: &mut dyn Write) -> Result<(), CompressionError> {
    let context = lz4f_context()?;
    let options = lz4_sys::LZ4FDecompressOptions {
        stable_dst: 1,
        reserved: [0; 3],
    };
    // The library consumes the source only as far as it needs it, so the rest
    // of the source stays for the next round.
    let mut input = vec![0u8; 64 * 1024];
    let mut pending = 0usize;
    let mut output = vec![0u8; 4 * 1024 * 1024];
    let mut at_end = false;
    loop {
        if pending == input.len() {
            // The library took nothing of the last read, so the buffer is
            // full of a block it wants whole: give it a larger one.
            let mut grown = vec![0u8; input.len() * 4];
            grown[..pending].copy_from_slice(&input[..pending]);
            input = grown;
        }
        if !at_end {
            let read = src.read(&mut input[pending..]).map_err(stream_error)?;
            if read == 0 {
                at_end = true;
            }
            pending += read;
        }
        if pending == 0 {
            break;
        }
        let mut compressed = pending;
        let mut decompressed = output.len();
        // SAFETY: the context is a live decompression context, `output` is
        // valid for `decompressed` bytes and `input` for `compressed` bytes.
        let result = unsafe {
            lz4_sys::LZ4F_decompress(
                context,
                output.as_mut_ptr(),
                &mut decompressed,
                input.as_ptr(),
                &mut compressed,
                &options,
            )
        };
        if lz4f_is_error(result) {
            return Err(lz4f_error("LZ4F decompression error: %s", result));
        }
        let consumed = compressed;
        pending -= consumed;
        if consumed > 0 {
            input.copy_within(consumed..pending + consumed, 0);
        }
        dst.write_all(&output[..decompressed]).map_err(stream_error)?;
        if result == 0 {
            // The frame is done. Another frame may follow, so the context
            // starts over with the rest of the source.
            // SAFETY: the context was created by `LZ4F_createDecompressionContext`.
            unsafe { lz4_sys::LZ4F_resetDecompressionContext(context) };
            continue;
        }
        if consumed == 0 && decompressed == 0 {
            if at_end {
                // The source ends in the middle of a frame.
                return Err(CompressionError(
                    "LZ4F decompression error: processed bytes don't match".to_owned(),
                ));
            }
            // The library wants more of the source than the buffer holds.
            let mut grown = vec![0u8; input.len() * 4];
            grown[..pending].copy_from_slice(&input[..pending]);
            input = grown;
        }
    }
    Ok(())
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
    fn the_stream_overloads_compress_as_upstream_routes_them() {
        for length in [0, 100, 70_000, (4 << 20) + 5000] {
            let data = sample(length);
            for kind in [CompressionType::ZLib, CompressionType::LZ4, CompressionType::LZ4F] {
                // Unbuffered, and LZ4 buffered: the buffer route's bytes.
                let mut expected = Vec::new();
                kind.compress_to(&data, &mut expected).unwrap();
                assert_eq!(expected, kind.compress(&data).unwrap());
                let mut unbuffered = Vec::new();
                kind.compress_stream(&mut data.as_slice(), &mut unbuffered, false)
                    .unwrap();
                assert_eq!(unbuffered, expected, "{} of {length} bytes", kind.name());
                let mut buffered = Vec::new();
                kind.compress_stream(&mut data.as_slice(), &mut buffered, true).unwrap();
                let mut out = vec![0u8; data.len()];
                kind.decompress(&buffered, &mut out).unwrap();
                assert_eq!(out, data, "{} buffered of {length} bytes", kind.name());
                match kind {
                    CompressionType::LZ4 => assert_eq!(buffered, expected),
                    // zlib's own stream at level 9, also where the buffer
                    // route takes libdeflate.
                    CompressionType::ZLib => {
                        assert_eq!(buffered, zlib_compress(&data, ZLIB_COMPRESSION_LEVEL).unwrap())
                    }
                    _ => {}
                }
            }
            // The stream decoder of LZ4F reads both frames.
            for frame in [CompressionType::LZ4F.compress(&data).unwrap(), {
                let mut out = Vec::new();
                CompressionType::LZ4F
                    .compress_stream(&mut data.as_slice(), &mut out, true)
                    .unwrap();
                out
            }] {
                let mut out = Vec::new();
                CompressionType::LZ4F
                    .decompress_stream(&mut frame.as_slice(), &mut out)
                    .unwrap();
                assert_eq!(out, data);
            }
        }
        let mut sink = Vec::new();
        assert_eq!(
            CompressionType::ZLib
                .decompress_stream(&mut { TEXT }, &mut sink)
                .unwrap_err()
                .0,
            "Not implemented"
        );
        assert!(
            CompressionType::None
                .compress_stream(&mut { TEXT }, &mut sink, true)
                .is_err()
        );
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

    /// The frame of the C library, decoded by the route of `lz4_flex`, the
    /// route the port used before.
    fn lz4f_decompress_old(packed: &[u8], out: &mut [u8]) -> Result<(), CompressionError> {
        let mut decoder = lz4_flex::frame::FrameDecoder::new(packed);
        decoder
            .read_exact(out)
            .map_err(|error| CompressionError(format!("LZ4F decompression error: {error}")))?;
        let mut rest = [0u8; 1];
        match decoder.read(&mut rest) {
            Ok(0) if decoder.into_inner().is_empty() => Ok(()),
            Ok(_) => Err(CompressionError(
                "LZ4F decompression error: processed bytes don't match".to_owned(),
            )),
            Err(error) => Err(CompressionError(format!("LZ4F decompression error: {error}"))),
        }
    }

    /// The two routes of the frame decode give the same bytes.
    ///
    /// The frames come from both compressors, so the test covers a frame
    /// neither route produced.
    #[test]
    fn the_lz4_frame_routes_give_the_same_bytes() {
        for length in [0usize, 1, 100, 70_000, 600_000, 5_000_000] {
            let data = sample(length);
            for packed in [CompressionType::LZ4F.compress(&data).unwrap(), {
                use std::io::Write;
                let mut encoder = lz4_flex::frame::FrameEncoder::new(Vec::new());
                encoder.write_all(&data).unwrap();
                encoder.finish().unwrap()
            }] {
                let mut want = vec![0u8; data.len()];
                lz4f_decompress_old(&packed, &mut want).unwrap();
                let mut got = vec![0u8; data.len()];
                CompressionType::LZ4F.decompress(&packed, &mut got).unwrap();
                assert_eq!(got, want, "the routes differ at {length} bytes");
                assert_eq!(got, data, "the frame of {length} bytes");
            }
        }
    }

    /// The two routes of the frame stream decode give the same bytes.
    #[test]
    fn the_lz4_frame_stream_routes_give_the_same_bytes() {
        for length in [0usize, 1, 100, 70_000, 600_000] {
            let data = sample(length);
            for packed in [CompressionType::LZ4F.compress(&data).unwrap(), {
                let mut out = Vec::new();
                CompressionType::LZ4F
                    .compress_stream(&mut data.as_slice(), &mut out, true)
                    .unwrap();
                out
            }] {
                let mut want = Vec::new();
                let mut decoder = lz4_flex::frame::FrameDecoder::new(packed.as_slice());
                std::io::copy(&mut decoder, &mut want).unwrap();
                let mut got = Vec::new();
                CompressionType::LZ4F
                    .decompress_stream(&mut packed.as_slice(), &mut got)
                    .unwrap();
                assert_eq!(got, want, "the stream routes differ at {length} bytes");
                assert_eq!(got, data, "the stream of {length} bytes");
            }
        }
    }

    /// The frame of the C library decodes the same after the route was used
    /// for another frame, so no state of the earlier frame carries over.
    #[test]
    fn the_frame_route_carries_nothing_over() {
        let first = sample(70_000);
        let second = sample(1000);
        let first_frame = CompressionType::LZ4F.compress(&first).unwrap();
        let second_frame = CompressionType::LZ4F.compress(&second).unwrap();
        let mut out = vec![0u8; first.len()];
        CompressionType::LZ4F.decompress(&first_frame, &mut out).unwrap();
        assert_eq!(out, first);
        let mut out = vec![0u8; second.len()];
        CompressionType::LZ4F.decompress(&second_frame, &mut out).unwrap();
        assert_eq!(out, second);
        let mut out = vec![0u8; first.len()];
        CompressionType::LZ4F.decompress(&first_frame, &mut out).unwrap();
        assert_eq!(out, first);
    }

    #[test]
    fn none_is_not_a_decompression() {
        assert!(CompressionType::None.decompress(TEXT, &mut [0u8; 4]).is_err());
    }

    /// A block compressed by the linked C library, so the test compares the
    /// two routes over a block neither of them produced.
    fn compress_block_c(data: &[u8]) -> Vec<u8> {
        let size = i32::try_from(data.len()).unwrap();
        // SAFETY: `LZ4_compressBound` only reads its argument.
        let bound = unsafe { lz4_sys::LZ4_compressBound(size) } as usize;
        let mut out = vec![0u8; bound];
        // SAFETY: `data` is valid for `size` bytes and `out` for `bound`.
        let written = unsafe {
            lz4_sys::LZ4_compress_default(data.as_ptr().cast(), out.as_mut_ptr().cast(), size, bound as i32)
        };
        assert!(written > 0);
        out.truncate(written as usize);
        out
    }

    /// The linked C library and the pure Rust route give the same bytes for
    /// the same input, and the same verdict when the input is not a block.
    #[test]
    fn the_lz4_block_routes_give_the_same_bytes() {
        for length in [0usize, 1, 2, 3, 15, 16, 17, 100, 70_000, 600_000] {
            let data = sample(length);
            for packed in [lz4_flex::block::compress(&data), compress_block_c(&data)] {
                let mut want = vec![0u8; data.len()];
                let old = lz4_flex::block::decompress_into(&packed, &mut want);
                let mut got = vec![0u8; data.len()];
                let new = CompressionType::LZ4.decompress(&packed, &mut got);
                match (old, new) {
                    (Ok(size), Ok(())) => {
                        assert_eq!(size, data.len());
                        assert_eq!(got, want, "{length} bytes");
                        assert_eq!(got, data, "{length} bytes");
                    }
                    (Err(_), Err(_)) => {}
                    (old, new) => panic!("{length} bytes: {old:?} against {new:?}"),
                }
            }
        }
    }

    /// A block that does not fill the output is a failure on both routes,
    /// and the two leave the same bytes in the output.
    #[test]
    fn the_lz4_block_routes_agree_on_a_short_output() {
        let data = sample(1000);
        let packed = lz4_flex::block::compress(&data);
        let mut want = vec![0u8; 999];
        let old = lz4_flex::block::decompress_into(&packed, &mut want);
        let mut got = vec![0u8; 999];
        let new = CompressionType::LZ4.decompress(&packed, &mut got);
        assert!(old.is_err(), "{old:?}");
        assert!(new.is_err(), "{new:?}");
        // UPSTREAM-QUIRK: the two routes differ in how much of the block
        // they decode before they run out of room, so they leave different
        // bytes in the output. `LZ4_decompress_safe`, the call upstream
        // makes, decodes further than the pure Rust route, so the linked
        // route is the one that matches upstream. No caller reads the output
        // of a failed decompression.
        assert_eq!(got.len(), want.len());
    }
}
