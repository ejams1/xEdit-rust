// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbHash.pas

//! Port of the `TwbHash.CRC32` part of `wbHash.pas`: the CRC32 of a file as
//! xEdit computes it before and after a save. The other hashes of the unit
//! (the BSA hashes, xxHash) come with the archive code.

/// Port of `TwbHash.CRC32`: the standard CRC-32 (IEEE 802.3, reflected, as
/// zlib's `crc32`), which upstream computes with its own table and compares
/// against `libdeflate_crc32`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = libdeflater::Crc::new();
    crc.update(data);
    crc.sum()
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn matches_the_reference_check_value() {
        // The check value of CRC-32/ISO-HDLC.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }
}
