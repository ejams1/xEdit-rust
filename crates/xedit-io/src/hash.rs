// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbHash.pas

//! Port of `wbHash.pas`: the CRC32 of a file as xEdit computes it before and
//! after a save, the name hashes of the Bethesda archives (Morrowind,
//! Oblivion through Skyrim Special Edition, and the BA2 `BSCRC32` of Fallout 4
//! and Starfield), and the xxHash functions behind the lookup hash that the
//! archive packer uses to find identical data.

use crate::encoding::{ansi_bytes, lower_case};

/// Port of `TwbHash.CRC32`: the standard CRC-32 (IEEE 802.3, reflected, as
/// zlib's `crc32`), which upstream computes with its own table and compares
/// against `libdeflate_crc32`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = libdeflater::Crc::new();
    crc.update(data);
    crc.sum()
}

const XXH32_PRIME1: u32 = 0x9E37_79B1;
const XXH32_PRIME2: u32 = 0x85EB_CA77;
const XXH32_PRIME3: u32 = 0xC2B2_AE3D;
const XXH32_PRIME4: u32 = 0x27D4_EB2F;
const XXH32_PRIME5: u32 = 0x1656_67B1;

fn read_u32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

fn read_u64(data: &[u8], at: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[at..at + 8]);
    u64::from_le_bytes(bytes)
}

/// Port of `TwbHash.XXH32`.
pub fn xxh32(data: &[u8], seed: u32) -> u32 {
    let round = |acc: u32, input: u32| {
        acc.wrapping_add(input.wrapping_mul(XXH32_PRIME2))
            .rotate_left(13)
            .wrapping_mul(XXH32_PRIME1)
    };
    let mut at = 0;
    let mut hash = if data.len() >= 16 {
        let mut v1 = seed.wrapping_add(XXH32_PRIME1).wrapping_add(XXH32_PRIME2);
        let mut v2 = seed.wrapping_add(XXH32_PRIME2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(XXH32_PRIME1);
        while at + 16 <= data.len() {
            v1 = round(v1, read_u32(data, at));
            v2 = round(v2, read_u32(data, at + 4));
            v3 = round(v3, read_u32(data, at + 8));
            v4 = round(v4, read_u32(data, at + 12));
            at += 16;
        }
        v1.rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18))
    } else {
        seed.wrapping_add(XXH32_PRIME5)
    };
    hash = hash.wrapping_add(data.len() as u32);
    while at + 4 <= data.len() {
        hash = hash
            .wrapping_add(read_u32(data, at).wrapping_mul(XXH32_PRIME3))
            .rotate_left(17)
            .wrapping_mul(XXH32_PRIME4);
        at += 4;
    }
    while at < data.len() {
        hash = hash
            .wrapping_add(u32::from(data[at]).wrapping_mul(XXH32_PRIME5))
            .rotate_left(11)
            .wrapping_mul(XXH32_PRIME1);
        at += 1;
    }
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(XXH32_PRIME2);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(XXH32_PRIME3);
    hash ^ (hash >> 16)
}

const XXH64_PRIME1: u64 = 0x9E37_79B1_85EB_CA87;
const XXH64_PRIME2: u64 = 0xC2B2_AE3D_27D4_EB4F;
const XXH64_PRIME3: u64 = 0x1656_67B1_9E37_79F9;
const XXH64_PRIME4: u64 = 0x85EB_CA77_C2B2_AE63;
const XXH64_PRIME5: u64 = 0x27D4_EB2F_1656_67C5;

fn xxh64_round(acc: u64, input: u64) -> u64 {
    acc.wrapping_add(input.wrapping_mul(XXH64_PRIME2))
        .rotate_left(31)
        .wrapping_mul(XXH64_PRIME1)
}

fn xxh64_merge(acc: u64, value: u64) -> u64 {
    (acc ^ xxh64_round(0, value))
        .wrapping_mul(XXH64_PRIME1)
        .wrapping_add(XXH64_PRIME4)
}

/// Port of `TwbHash.XXH64`.
pub fn xxh64(data: &[u8], seed: u64) -> u64 {
    let mut at = 0;
    let mut hash = if data.len() >= 32 {
        let mut v1 = seed.wrapping_add(XXH64_PRIME1).wrapping_add(XXH64_PRIME2);
        let mut v2 = seed.wrapping_add(XXH64_PRIME2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(XXH64_PRIME1);
        while at + 32 <= data.len() {
            v1 = xxh64_round(v1, read_u64(data, at));
            v2 = xxh64_round(v2, read_u64(data, at + 8));
            v3 = xxh64_round(v3, read_u64(data, at + 16));
            v4 = xxh64_round(v4, read_u64(data, at + 24));
            at += 32;
        }
        let mut hash = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
        hash = xxh64_merge(hash, v1);
        hash = xxh64_merge(hash, v2);
        hash = xxh64_merge(hash, v3);
        xxh64_merge(hash, v4)
    } else {
        seed.wrapping_add(XXH64_PRIME5)
    };
    hash = hash.wrapping_add(data.len() as u64);
    while at + 8 <= data.len() {
        hash ^= xxh64_round(0, read_u64(data, at));
        hash = hash
            .rotate_left(27)
            .wrapping_mul(XXH64_PRIME1)
            .wrapping_add(XXH64_PRIME4);
        at += 8;
    }
    if at + 4 <= data.len() {
        hash ^= u64::from(read_u32(data, at)).wrapping_mul(XXH64_PRIME1);
        hash = hash
            .rotate_left(23)
            .wrapping_mul(XXH64_PRIME2)
            .wrapping_add(XXH64_PRIME3);
        at += 4;
    }
    while at < data.len() {
        hash ^= u64::from(data[at]).wrapping_mul(XXH64_PRIME5);
        hash = hash.rotate_left(11).wrapping_mul(XXH64_PRIME1);
        at += 1;
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(XXH64_PRIME2);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(XXH64_PRIME3);
    hash ^ (hash >> 32)
}

/// Upstream `TwbLookupHash`: the xxHash64 of data, which the archive code
/// compares to tell identical files and names apart.
pub type LookupHash = u64;

/// Port of `TwbHash.LookupHash` for data.
pub fn lookup_hash(data: &[u8]) -> LookupHash {
    xxh64(data, 0)
}

/// Port of `TwbHash.LookupHash` for text: the hash of the UTF-16 bytes of
/// the (lower-cased) text, 0 for the empty text.
pub fn lookup_hash_text(text: &str, ignore_case: bool) -> LookupHash {
    if text.is_empty() {
        return 0;
    }
    let text = if ignore_case { lower_case(text) } else { text.to_owned() };
    let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    lookup_hash(&bytes)
}

/// The path with `/` replaced by `\`, as `StringReplace(s, '/', '\', [rfReplaceAll])`.
fn backslashes(text: &str) -> String {
    text.replace('/', "\\")
}

/// Port of `TwbHash.TES3`: the hash of a Morrowind archive path.
pub fn tes3(text: &str) -> u64 {
    let s = ansi_bytes(&backslashes(&lower_case(text)));
    let half = s.len() >> 1;
    let mut sum = 0u32;
    let mut off = 0u32;
    for &byte in &s[..half] {
        sum ^= u32::from(byte) << (off & 0x1F);
        off = off.wrapping_add(8);
    }
    let low = sum;
    sum = 0;
    off = 0;
    for &byte in &s[half..] {
        let temp = u32::from(byte) << (off & 0x1F);
        sum ^= temp;
        sum = sum.rotate_right(temp & 0x1F);
        off = off.wrapping_add(8);
    }
    u64::from(low) | (u64::from(sum) << 32)
}

/// Port of `TwbHash._TES4`: the hash of a folder or file name in an
/// Oblivion through Skyrim Special Edition archive. `signed` reads bytes
/// above 127 as negative, as Oblivion's archives do.
pub fn tes4_raw(text: &str, has_extension: bool, signed: bool) -> u64 {
    let lowered = lower_case(text);
    let (name, extension) = match lowered.rfind('.') {
        Some(at) if has_extension => (&lowered[..at], &lowered[at..]),
        _ => (lowered.as_str(), ""),
    };
    let s = ansi_bytes(name);
    let e = ansi_bytes(extension);

    let l = s.len();
    let mut result = 0u64;
    if l > 0 {
        result |= u64::from(s[l - 1]);
    }
    if l > 2 {
        result |= u64::from(s[l - 2]) << 8;
    }
    result |= u64::from(l as u8) << 16;
    if l > 0 {
        result |= u64::from(s[0]) << 24;
    }

    match e.as_slice() {
        b".kf" => result |= 0x80,
        b".nif" => result |= 0x8000,
        b".dds" => result |= 0x8080,
        b".wav" => result |= 0x8000_0000,
        _ => {}
    }

    let step = |hash: u32, byte: u8| {
        let mut hash = u32::from(byte)
            .wrapping_add(hash << 6)
            .wrapping_add(hash << 16)
            .wrapping_sub(hash);
        if signed && byte > 127 {
            hash = hash.wrapping_sub(256);
        }
        hash
    };
    let mut hash = 0u32;
    if l >= 4 {
        for &byte in &s[1..l - 2] {
            hash = step(hash, byte);
        }
    }
    result = result.wrapping_add(u64::from(hash) << 32);

    hash = 0;
    for &byte in &e {
        hash = step(hash, byte);
    }
    result.wrapping_add(u64::from(hash) << 32)
}

/// Port of `TwbHash.TES4`: the hash of Oblivion archives.
pub fn tes4(text: &str, has_extension: bool) -> u64 {
    tes4_raw(text, has_extension, true)
}

/// Port of `TwbHash.TES5`: the hash of Fallout 3 through Skyrim Special
/// Edition archives.
pub fn tes5(text: &str, has_extension: bool) -> u64 {
    tes4_raw(text, has_extension, false)
}

/// Port of `TwbHash.FO4`: the folder and file name hash of BA2 archives.
pub fn fo4(text: &str) -> u32 {
    bscrc32(&backslashes(&lower_case(text)))
}

/// The table of `TwbHash.cCRC32Table`: the reflected CRC-32 table.
const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC32_TABLE: [u32; 256] = crc_table();

/// Port of `TwbHash.BSCRC32`: the CRC-32 table loop without the initial and
/// final inversion of the standard CRC-32.
pub fn bscrc32(text: &str) -> u32 {
    ansi_bytes(text).iter().fold(0u32, |crc, &byte| {
        (crc >> 8) ^ CRC32_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_reference_check_value() {
        // The check value of CRC-32/ISO-HDLC.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn xxhash_matches_the_reference_values() {
        assert_eq!(xxh32(b"", 0), 0x02CC_5D05);
        assert_eq!(xxh64(b"", 0), 0xEF46_DB37_51D8_E999);
        assert_eq!(xxh32(b"a", 0), 0x550D_7456);
        assert_eq!(xxh64(b"a", 0), 0xD24E_C4F1_A98C_6E5B);
        assert_eq!(xxh32(b"abc", 0), 0x32D1_53FF);
        assert_eq!(xxh64(b"abc", 0), 0x44BC_2CF5_AD77_0999);
    }

    #[test]
    fn xxhash_agrees_with_the_reference_implementation_on_every_length() {
        let data: Vec<u8> = (0..300u32).map(|i| (i * 7 + 3) as u8).collect();
        for length in 0..data.len() {
            let slice = &data[..length];
            assert_eq!(
                xxh32(slice, 0),
                twox_hash::XxHash32::oneshot(0, slice),
                "xxh32 of {length} bytes"
            );
            assert_eq!(
                xxh64(slice, 0),
                twox_hash::XxHash64::oneshot(0, slice),
                "xxh64 of {length} bytes"
            );
            assert_eq!(
                xxh64(slice, 99),
                twox_hash::XxHash64::oneshot(99, slice),
                "seeded xxh64 of {length} bytes"
            );
        }
    }

    #[test]
    fn bscrc32_is_the_crc_without_inversion() {
        // A CRC-32 with a zero start value and no final inversion.
        assert_eq!(bscrc32(""), 0);
        let standard = crc32(b"abc");
        let mut crc = 0u32;
        for &byte in b"abc" {
            crc = (crc >> 8) ^ CRC32_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize];
        }
        assert_ne!(crc, standard);
        assert_eq!(bscrc32("abc"), crc);
    }

    #[test]
    fn the_name_hashes_follow_the_documented_layout() {
        // The low dword of a Skyrim name hash: last character, the one before,
        // the length and the first character.
        let hash = tes5("meshes", false);
        assert_eq!(hash & 0xFF, u64::from(b's'));
        assert_eq!((hash >> 8) & 0xFF, u64::from(b'e'));
        assert_eq!((hash >> 16) & 0xFF, 6);
        assert_eq!((hash >> 24) & 0xFF, u64::from(b'm'));
        // The extension flags.
        assert_eq!(tes5("a.nif", true) & 0xFFFF_FFFF, 0x6101_8061);
        assert_eq!(tes5("abc.kf", true) & 0x80, 0x80);
        assert_eq!(tes5("abc.dds", true) & 0x8080, 0x8080);
        assert_eq!(tes5("abc.wav", true) & 0x8000_0000, 0x8000_0000);
        // The folder hash ignores the extension, the file hash takes it apart.
        assert_ne!(tes5("a.nif", true), tes5("a.nif", false));
        // The path case and separators do not matter for the BA2 hash.
        assert_eq!(fo4("Meshes/Armor"), fo4("meshes\\armor"));
    }
}
