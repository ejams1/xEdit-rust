// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! String encodings with the behaviour of Delphi `TEncoding`.
//!
//! xEdit converts every string in a plugin through a `TEncoding`: a
//! `TMBCSEncoding` for a Windows code page or `TEncoding.UTF8`. On Windows this
//! module calls the same system functions as Delphi, so that best-fit mapping
//! and undefined bytes convert exactly as in xEdit. Other platforms use
//! `encoding_rs`, which differs for characters without an exact mapping.

/// A text encoding of plugin data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoding {
    /// Delphi `TMBCSEncoding` for a Windows code page.
    Mbcs(u32),
    /// Delphi `TEncoding.UTF8`. Decoding fails on invalid input.
    Utf8,
}

/// Delphi `LowerCase` of a Unicode string: every character is mapped on its
/// own to its lower case, and a character that maps to several stays as it
/// is, so the string keeps its length in characters.
pub fn lower_case(text: &str) -> String {
    text.chars()
        .map(|c| {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(single), None) => single,
                _ => c,
            }
        })
        .collect()
}

/// Delphi `AnsiString(text)`: the bytes of the string in the system ANSI code
/// page (code page 1252 where the system has none), characters without a
/// mapping converted as the system does.
pub fn ansi_bytes(text: &str) -> Vec<u8> {
    Encoding::Mbcs(ANSI_CODE_PAGE).get_bytes(text)
}

/// Delphi `string(ansi)`: the string of bytes in the system ANSI code page.
/// Bytes the code page leaves undefined convert as the system does; a
/// failure gives the lossy UTF-8 reading instead of an error.
pub fn ansi_string(bytes: &[u8]) -> String {
    Encoding::Mbcs(ANSI_CODE_PAGE)
        .get_string(bytes)
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
}

/// The text of a file as `TStringList.LoadFromFile` reads it (`LoadFromJSONFile`,
/// the settings of Sniff): by its byte order mark, else in the ANSI code page.
pub fn string_list_text(bytes: &[u8]) -> String {
    let utf16 = |rest: &[u8], big_endian: bool| {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                if big_endian {
                    u16::from_be_bytes([pair[0], pair[1]])
                } else {
                    u16::from_le_bytes([pair[0], pair[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        utf16(rest, false)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        utf16(rest, true)
    } else {
        ansi_string(bytes)
    }
}

/// `CP_ACP` on Windows, which the system resolves to its ANSI code page.
#[cfg(windows)]
const ANSI_CODE_PAGE: u32 = 0;
#[cfg(not(windows))]
const ANSI_CODE_PAGE: u32 = 1252;

/// Bytes that are not valid in the encoding.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("No mapping for the Unicode character exists in the target multi-byte code page")]
pub struct EncodingError;

impl Encoding {
    /// Delphi `EncodingName`: `UTF-8` or the Windows name of the code page.
    pub fn name(self) -> String {
        match self {
            Encoding::Utf8 => "UTF-8".to_owned(),
            Encoding::Mbcs(code_page) => format!("Windows-{code_page}"),
        }
    }

    /// Converts bytes to a string. Port of `TEncoding.GetString`.
    pub fn get_string(self, bytes: &[u8]) -> Result<String, EncodingError> {
        if bytes.is_empty() {
            return Ok(String::new());
        }
        match self {
            Encoding::Utf8 => std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| EncodingError),
            Encoding::Mbcs(code_page) => platform::get_string(code_page, bytes),
        }
    }

    /// Converts a string to bytes. Port of `TEncoding.GetBytes`.
    pub fn get_bytes(self, text: &str) -> Vec<u8> {
        match self {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Mbcs(code_page) => platform::get_bytes(code_page, text),
        }
    }
}

/// Delphi `AnsiCompareText` (and the order of `TStringList.Sort`): the
/// comparison of the user's locale, ignoring case (`CompareString` with
/// `NORM_IGNORECASE`); elsewhere the upper case strings by code unit.
pub fn ansi_compare_text(a: &str, b: &str) -> std::cmp::Ordering {
    platform::compare_text(a, b)
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Globalization::{
        CompareStringW, LOCALE_USER_DEFAULT, MultiByteToWideChar, NORM_IGNORECASE, WideCharToMultiByte,
    };

    use super::EncodingError;

    pub fn compare_text(a: &str, b: &str) -> std::cmp::Ordering {
        let a: Vec<u16> = a.encode_utf16().collect();
        let b: Vec<u16> = b.encode_utf16().collect();
        let (Ok(a_length), Ok(b_length)) = (i32::try_from(a.len()), i32::try_from(b.len())) else {
            return a.cmp(&b);
        };
        // SAFETY: both buffers are valid for the lengths passed.
        let result = unsafe {
            CompareStringW(
                LOCALE_USER_DEFAULT,
                NORM_IGNORECASE,
                a.as_ptr(),
                a_length,
                b.as_ptr(),
                b_length,
            )
        };
        match result {
            1 => std::cmp::Ordering::Less,
            3 => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }
    }

    pub fn get_string(code_page: u32, bytes: &[u8]) -> Result<String, EncodingError> {
        let length = i32::try_from(bytes.len()).map_err(|_| EncodingError)?;
        // SAFETY: `bytes` is valid for `length` bytes. A null output buffer with size 0
        // makes the function return the required size without writing.
        let size = unsafe { MultiByteToWideChar(code_page, 0, bytes.as_ptr(), length, std::ptr::null_mut(), 0) };
        if size <= 0 {
            return Err(EncodingError);
        }
        let mut wide = vec![0u16; size as usize];
        // SAFETY: `wide` is valid for `size` UTF-16 units and `bytes` for `length` bytes.
        let written = unsafe { MultiByteToWideChar(code_page, 0, bytes.as_ptr(), length, wide.as_mut_ptr(), size) };
        if written != size {
            return Err(EncodingError);
        }
        Ok(String::from_utf16_lossy(&wide))
    }

    pub fn get_bytes(code_page: u32, text: &str) -> Vec<u8> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let Ok(length) = i32::try_from(wide.len()) else {
            return Vec::new();
        };
        if length == 0 {
            return Vec::new();
        }
        // SAFETY: `wide` is valid for `length` UTF-16 units. A null output buffer with
        // size 0 makes the function return the required size without writing.
        let size = unsafe {
            WideCharToMultiByte(
                code_page,
                0,
                wide.as_ptr(),
                length,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        if size <= 0 {
            return Vec::new();
        }
        let mut bytes = vec![0u8; size as usize];
        // SAFETY: `bytes` is valid for `size` bytes and `wide` for `length` UTF-16 units.
        let written = unsafe {
            WideCharToMultiByte(
                code_page,
                0,
                wide.as_ptr(),
                length,
                bytes.as_mut_ptr(),
                size,
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        bytes.truncate(written.max(0) as usize);
        bytes
    }
}

#[cfg(not(windows))]
mod platform {
    use super::EncodingError;

    pub fn compare_text(a: &str, b: &str) -> std::cmp::Ordering {
        let a: Vec<u16> = a.to_uppercase().encode_utf16().collect();
        let b: Vec<u16> = b.to_uppercase().encode_utf16().collect();
        a.cmp(&b)
    }

    fn encoding(code_page: u32) -> Option<&'static encoding_rs::Encoding> {
        let label = match code_page {
            932 => "shift_jis".to_owned(),
            936 => "gbk".to_owned(),
            949 => "euc-kr".to_owned(),
            950 => "big5".to_owned(),
            866 => "ibm866".to_owned(),
            874 => "windows-874".to_owned(),
            1250..=1258 => format!("windows-{code_page}"),
            65001 => "utf-8".to_owned(),
            _ => return None,
        };
        encoding_rs::Encoding::for_label(label.as_bytes())
    }

    pub fn get_string(code_page: u32, bytes: &[u8]) -> Result<String, EncodingError> {
        let encoding = encoding(code_page).ok_or(EncodingError)?;
        Ok(encoding.decode_without_bom_handling(bytes).0.into_owned())
    }

    pub fn get_bytes(code_page: u32, text: &str) -> Vec<u8> {
        let Some(encoding) = encoding(code_page) else {
            return Vec::new();
        };
        let mut encoder = encoding.new_encoder();
        let mut bytes = Vec::with_capacity(text.len());
        let mut rest = text;
        loop {
            let needed = encoder
                .max_buffer_length_from_utf8_without_replacement(rest.len())
                .unwrap_or(rest.len() * 4);
            bytes.reserve(needed);
            let (result, read) = encoder.encode_from_utf8_to_vec_without_replacement(rest, &mut bytes, true);
            rest = &rest[read..];
            match result {
                encoding_rs::EncoderResult::InputEmpty => return bytes,
                encoding_rs::EncoderResult::OutputFull => {}
                // The Windows default character for the code pages xEdit uses.
                encoding_rs::EncoderResult::Unmappable(_) => bytes.push(b'?'),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_page_1252_round_trip() {
        let encoding = Encoding::Mbcs(1252);
        assert_eq!(encoding.get_string(b"Caf\xE9 \x80").unwrap(), "Café €");
        assert_eq!(encoding.get_bytes("Café €"), b"Caf\xE9 \x80");
        assert_eq!(encoding.get_string(b"").unwrap(), "");
        assert_eq!(encoding.get_bytes(""), b"");
    }

    #[test]
    fn code_page_1252_undefined_byte_is_a_c1_control() {
        assert_eq!(Encoding::Mbcs(1252).get_string(b"\x81").unwrap(), "\u{81}");
    }

    #[test]
    fn unmappable_character_does_not_fail() {
        assert_eq!(Encoding::Mbcs(1252).get_bytes("a\u{4e2d}b"), b"a?b");
    }

    #[test]
    fn code_page_1251() {
        assert_eq!(Encoding::Mbcs(1251).get_string(b"\xCF\xF0\xE8").unwrap(), "При");
    }

    #[test]
    fn utf8_is_strict() {
        assert_eq!(Encoding::Utf8.get_string("Café".as_bytes()).unwrap(), "Café");
        assert_eq!(Encoding::Utf8.get_string(b"Caf\xE9"), Err(EncodingError));
        assert_eq!(Encoding::Utf8.get_bytes("Café"), "Café".as_bytes());
    }
}
