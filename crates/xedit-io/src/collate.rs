// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Delphi's locale-aware text comparison, which `TStringList` uses for its
//! sort and `Find` unless `UseLocale` is off.

use std::cmp::Ordering;

/// Delphi `AnsiCompareText`: `CompareString(LOCALE_USER_DEFAULT,
/// NORM_IGNORECASE, ...)`, the word sort of the user's locale without
/// regard to case. In it punctuation sorts before digits and digits before
/// letters, unlike the code units (`'|'` sorts before `'0'`), and the
/// hyphen and the apostrophe weigh less than any other character.
pub fn ansi_compare_text(a: &str, b: &str) -> Ordering {
    platform::compare(a, b)
}

#[cfg(windows)]
mod platform {
    use std::cmp::Ordering;

    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CSTR_LESS_THAN, CompareStringW, NORM_IGNORECASE};

    /// `LOCALE_USER_DEFAULT`.
    const LOCALE_USER_DEFAULT: u32 = 0x0400;

    pub fn compare(a: &str, b: &str) -> Ordering {
        let a: Vec<u16> = a.encode_utf16().collect();
        let b: Vec<u16> = b.encode_utf16().collect();
        let (Ok(a_len), Ok(b_len)) = (i32::try_from(a.len()), i32::try_from(b.len())) else {
            return super::fallback(&String::from_utf16_lossy(&a), &String::from_utf16_lossy(&b));
        };
        // SAFETY: both buffers are valid for the lengths given.
        let result = unsafe {
            CompareStringW(
                LOCALE_USER_DEFAULT,
                NORM_IGNORECASE,
                a.as_ptr(),
                a_len,
                b.as_ptr(),
                b_len,
            )
        };
        match result {
            CSTR_LESS_THAN => Ordering::Less,
            CSTR_EQUAL => Ordering::Equal,
            0 => super::fallback(&String::from_utf16_lossy(&a), &String::from_utf16_lossy(&b)),
            _ => Ordering::Greater,
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn compare(a: &str, b: &str) -> std::cmp::Ordering {
        super::fallback(a, b)
    }
}

/// The word sort of the invariant locale for ASCII text, where no system
/// collation is at hand: the hyphen and the apostrophe count only when the
/// texts are equal without them, punctuation before digits before letters,
/// letters without regard to case.
fn fallback(a: &str, b: &str) -> Ordering {
    fn weight(c: char) -> (u8, u32) {
        match c {
            '0'..='9' => (1, c as u32),
            'a'..='z' | 'A'..='Z' => (2, c.to_ascii_uppercase() as u32),
            c if c.is_ascii() => (0, c as u32),
            c => (3, c as u32),
        }
    }
    let primary =
        |text: &str| -> Vec<(u8, u32)> { text.chars().filter(|&c| c != '-' && c != '\'').map(weight).collect() };
    primary(a)
        .cmp(&primary(b))
        .then_with(|| a.chars().map(weight).cmp(b.chars().map(weight)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punctuation_sorts_before_digits_and_case_is_ignored() {
        assert_eq!(ansi_compare_text("A|B", "A0B"), Ordering::Less);
        assert_eq!(ansi_compare_text("abc", "ABC"), Ordering::Equal);
        assert_eq!(ansi_compare_text("0009", "000A"), Ordering::Less);
        assert_eq!(ansi_compare_text("AB#0000", "AB#0001"), Ordering::Less);
        assert_eq!(fallback("A|B", "A0B"), Ordering::Less);
        assert_eq!(fallback("abc", "ABC"), Ordering::Equal);
    }
}
