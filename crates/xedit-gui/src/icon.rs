// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit.dpr (LoadIcon and Application.Icon.Handle of
// `xeIconResource`), xEdit/xeInit.pas (the icon resource of each game
// family), xEdit/xeIcons.rc (xTESICON, xFOICON, xSFICON)

//! The per-game window icon: the Delphi build loads `xTESICON`,
//! `xFOICON` or `xSFICON` from its resources depending on the game family,
//! and these are the same three `.ico` files the resource script names
//! (`xEdit/xeIcons.rc`).
//!
//! The icon files are decoded here rather than through an image crate:
//! each one holds uncompressed 32-bit DIB entries, which is a header, the
//! BGRA rows bottom-up and an AND mask.

use eframe::egui::IconData;
use xedit_core::interface::globals::GameMode;

/// `xEdit_TES_Icon.ico` (`xTESICON`).
pub const TES_ICON: &[u8] = include_bytes!("../assets/icons/xEdit_TES_Icon.ico");
/// `xEdit_FO_Icon.ico` (`xFOICON`).
pub const FO_ICON: &[u8] = include_bytes!("../assets/icons/xEdit_FO_Icon.ico");
/// `xEdit_SF_Icon.ico` (`xSFICON`).
pub const SF_ICON: &[u8] = include_bytes!("../assets/icons/xEdit_SF_Icon.ico");

/// The icon resource of a game mode, as `xeInit.pas` picks `xeIconResource`.
pub fn icon_resource(mode: GameMode) -> &'static [u8] {
    match mode {
        GameMode::gmTES3
        | GameMode::gmTES4
        | GameMode::gmTES4R
        | GameMode::gmTES5
        | GameMode::gmEnderal
        | GameMode::gmSSE
        | GameMode::gmTES5VR
        | GameMode::gmEnderalSE => TES_ICON,
        GameMode::gmFO3 | GameMode::gmFNV | GameMode::gmFO4 | GameMode::gmFO4VR | GameMode::gmFO76 => FO_ICON,
        GameMode::gmSF1 => SF_ICON,
    }
}

/// The largest DIB entry of an `.ico` file the window can show, decoded to
/// `IconData`. `None` when the file holds only PNG entries (the 256×256
/// entries of the upstream icons are PNG, which eframe's `IconData` cannot
/// take directly without an image decoder; the 64×64 DIB is what a window
/// uses anyway).
pub fn icon_for_game(mode: GameMode) -> Option<IconData> {
    decode_icon(icon_resource(mode), 64)
}

/// Decodes the best 32-bit DIB entry of an ICO file up to `max_size`.
pub fn decode_icon(bytes: &[u8], max_size: u32) -> Option<IconData> {
    if bytes.len() < 6 {
        return None;
    }
    let reserved = u16::from_le_bytes([bytes[0], bytes[1]]);
    let kind = u16::from_le_bytes([bytes[2], bytes[3]]);
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    if reserved != 0 || kind != 1 {
        return None;
    }
    let mut best: Option<(u32, usize)> = None;
    for index in 0..count {
        let entry = 6 + index * 16;
        if bytes.len() < entry + 16 {
            return None;
        }
        let width = if bytes[entry] == 0 {
            256
        } else {
            u32::from(bytes[entry])
        };
        let bit_count = u16::from_le_bytes([bytes[entry + 6], bytes[entry + 7]]);
        let offset = u32::from_le_bytes([
            bytes[entry + 12],
            bytes[entry + 13],
            bytes[entry + 14],
            bytes[entry + 15],
        ]);
        if bit_count != 32 || width > max_size {
            continue;
        }
        // A PNG entry starts with the PNG signature: skip it.
        let start = offset as usize;
        if bytes
            .get(start..start + 8)
            .is_none_or(|head| head.starts_with(b"\x89PNG\r\n\x1a\n"))
        {
            continue;
        }
        if best.is_none_or(|(best_width, _)| width > best_width) {
            best = Some((width, index));
        }
    }
    let (_, index) = best?;
    let entry = 6 + index * 16;
    let size = u32::from_le_bytes([bytes[entry + 8], bytes[entry + 9], bytes[entry + 10], bytes[entry + 11]]) as usize;
    let offset = u32::from_le_bytes([
        bytes[entry + 12],
        bytes[entry + 13],
        bytes[entry + 14],
        bytes[entry + 15],
    ]) as usize;
    decode_dib(bytes.get(offset..offset + size)?)
}

/// A 32-bit `BITMAPINFOHEADER` DIB as an `.ico` stores it: BGRA rows from
/// the bottom up, then the AND mask (which a 32-bit icon with alpha does
/// not need).
fn decode_dib(dib: &[u8]) -> Option<IconData> {
    if dib.len() < 40 {
        return None;
    }
    let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    if header_size < 40 {
        return None;
    }
    let width = i32::from_le_bytes([dib[4], dib[5], dib[6], dib[7]]);
    let height = i32::from_le_bytes([dib[8], dib[9], dib[10], dib[11]]);
    let bit_count = u16::from_le_bytes([dib[14], dib[15]]);
    if width <= 0 || height == 0 || bit_count != 32 {
        return None;
    }
    // The height covers the colour rows and the AND mask.
    let width = width as u32;
    let height = (height / 2).unsigned_abs();
    let stride = width as usize * 4;
    let pixels = width as usize * height as usize;
    let start = header_size;
    let colour_bytes = dib.get(start..start + stride * height as usize)?;
    let mut rgba = Vec::with_capacity(pixels * 4);
    for y in (0..height as usize).rev() {
        let row = &colour_bytes[y * stride..(y + 1) * stride];
        for x in 0..width as usize {
            let [b, g, r, a] = [row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]];
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    // A 32-bit icon whose alpha channel is all zero uses the AND mask.
    if rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0) {
        let mask_stride = stride.div_ceil(32) * 4;
        if let Some(mask) = dib.get(start + stride * height as usize..) {
            for y in 0..height as usize {
                let row = mask.get(y * mask_stride..(y + 1) * mask_stride);
                for x in 0..width as usize {
                    let transparent = row.is_some_and(|row| row[x / 8] & (0x80 >> (x % 8)) != 0);
                    let at = ((height as usize - 1 - y) * width as usize + x) * 4 + 3;
                    rgba[at] = if transparent { 0 } else { 255 };
                }
            }
        }
    }
    Some(IconData { rgba, width, height })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_game_family_has_an_icon() {
        for mode in GameMode::ALL {
            let icon = icon_for_game(mode).unwrap_or_else(|| panic!("{mode:?} has no icon"));
            assert!(icon.width >= 16 && icon.width <= 64);
            assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
        }
    }

    #[test]
    fn the_families_use_the_upstream_resources() {
        assert_eq!(icon_resource(GameMode::gmSSE), TES_ICON);
        assert_eq!(icon_resource(GameMode::gmFO4), FO_ICON);
        assert_eq!(icon_resource(GameMode::gmSF1), SF_ICON);
    }

    #[test]
    fn a_foreign_file_is_refused() {
        assert!(decode_icon(b"not an icon", 64).is_none());
    }
}
