// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `--keys` sequence of `cargo xtask gui keys`.
//!
//! A sequence is comma separated events: `ctrl+s,enter,text:FO4`. Modifiers
//! are `ctrl`, `shift` and `alt` in front of a key name joined by `+`; a key
//! is a letter, a digit or one of the names below. `text:` types literal
//! characters, and a backslash lets a comma or a backslash itself through
//! (`text:a\,b`). A lone `alt+<letter>` (or `menu:<letter>`) opens the window's
//! menu for that mnemonic with `WM_SYSCOMMAND`/`SC_KEYMENU` instead of sending
//! a key: that is how a menu on a private desktop is opened, because no key
//! actually reaches the desktop.

use serde::{Deserialize, Serialize};

/// One event of a sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyEvent {
    /// A key press with `ctrl`, `shift` and `alt` held (the virtual key code
    /// and the character the key would produce, if any).
    Key {
        vk: u16,
        character: Option<u16>,
        ctrl: bool,
        shift: bool,
        alt: bool,
    },
    /// Text typed character by character (`WM_CHAR`).
    Text(String),
    /// The window's menu opened for the mnemonic `char`.
    Menu(char),
}

/// Virtual key codes of the named keys (not all are re-exported here, only
/// the ones the parser and the poster use).
pub mod vk {
    pub const BACK: u16 = 0x08;
    pub const TAB: u16 = 0x09;
    pub const RETURN: u16 = 0x0D;
    pub const SHIFT: u16 = 0x10;
    pub const CONTROL: u16 = 0x11;
    pub const MENU: u16 = 0x12;
    pub const ESCAPE: u16 = 0x1B;
    pub const SPACE: u16 = 0x20;
    pub const PRIOR: u16 = 0x21;
    pub const NEXT: u16 = 0x22;
    pub const END: u16 = 0x23;
    pub const HOME: u16 = 0x24;
    pub const LEFT: u16 = 0x25;
    pub const UP: u16 = 0x26;
    pub const RIGHT: u16 = 0x27;
    pub const DOWN: u16 = 0x28;
    pub const INSERT: u16 = 0x2D;
    pub const DELETE: u16 = 0x2E;
    pub const F1: u16 = 0x70;
    pub const LSHIFT: u16 = 0xA0;
    pub const LCONTROL: u16 = 0xA2;
    pub const LMENU: u16 = 0xA4;
}

/// Parses `text` into its events.
pub fn parse(text: &str) -> Result<Vec<KeyEvent>, String> {
    let mut events = Vec::new();
    for token in split_events(text) {
        if token.is_empty() {
            return Err("empty event in the key sequence".to_owned());
        }
        events.push(parse_event(&token)?);
    }
    if events.is_empty() {
        return Err("the key sequence is empty".to_owned());
    }
    Ok(events)
}

/// Splits on commas that are not escaped by a backslash; the escape itself is
/// kept for [`parse_event`] to remove (so `text:a\,b` stays one token).
fn split_events(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for c in text.chars() {
        if escaped {
            current.push('\\');
            current.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            ',' => {
                tokens.push(std::mem::take(&mut current));
                continue;
            }
            _ => current.push(c),
        }
    }
    if escaped {
        current.push('\\');
    }
    tokens.push(current);
    tokens
}

/// Removes the escapes of a `text:` value.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            } else {
                out.push('\\');
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_event(token: &str) -> Result<KeyEvent, String> {
    if let Some(text) = token.strip_prefix("text:") {
        return Ok(KeyEvent::Text(unescape(text)));
    }
    if let Some(name) = token.strip_prefix("menu:") {
        let mut chars = name.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return Err(format!("menu:{name}: expected one mnemonic character"));
        };
        return Ok(KeyEvent::Menu(c));
    }
    let parts: Vec<&str> = token.split('+').collect();
    let (key, modifiers) = parts.split_last().ok_or_else(|| format!("{token}: empty event"))?;
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    for modifier in modifiers {
        match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" => alt = true,
            other => return Err(format!("{token}: unknown modifier {other:?}")),
        }
    }
    let (vk, character) = key_code(key).ok_or_else(|| format!("{token}: unknown key {key:?}"))?;
    // Alt plus one letter is the window's menu accelerator, which Windows
    // would not see from a posted key on a desktop nobody types on.
    if alt
        && !ctrl
        && !shift
        && key.chars().count() == 1
        && let Some(c) = key.chars().next().filter(|c| c.is_ascii_alphanumeric())
    {
        return Ok(KeyEvent::Menu(c.to_ascii_lowercase()));
    }
    let character = match character {
        Some(c) if ctrl => {
            // The control character the key produces (Ctrl+S is 0x13), which
            // is what a VCL edit control inserts and what its `OnKeyPress`
            // sees.
            let upper = (c as u8).to_ascii_uppercase() as u16;
            (0x40..0x60).contains(&upper).then_some(upper & 0x1F)
        }
        Some(c) if shift => Some((c as u8).to_ascii_uppercase() as u16),
        other => other,
    };
    Ok(KeyEvent::Key {
        vk,
        character,
        ctrl,
        shift,
        alt,
    })
}

/// The virtual key code and the character of a key name.
fn key_code(key: &str) -> Option<(u16, Option<u16>)> {
    let lower = key.to_ascii_lowercase();
    let named = match lower.as_str() {
        "enter" | "return" => Some((vk::RETURN, Some(0x0D))),
        "tab" => Some((vk::TAB, Some(0x09))),
        "space" => Some((vk::SPACE, Some(u16::from(b' ')))),
        "esc" | "escape" => Some((vk::ESCAPE, Some(0x1B))),
        "backspace" | "back" => Some((vk::BACK, Some(0x08))),
        "delete" | "del" => Some((vk::DELETE, None)),
        "insert" | "ins" => Some((vk::INSERT, None)),
        "home" => Some((vk::HOME, None)),
        "end" => Some((vk::END, None)),
        "pageup" | "pgup" => Some((vk::PRIOR, None)),
        "pagedown" | "pgdn" | "pagedn" => Some((vk::NEXT, None)),
        "up" => Some((vk::UP, None)),
        "down" => Some((vk::DOWN, None)),
        "left" => Some((vk::LEFT, None)),
        "right" => Some((vk::RIGHT, None)),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    if let Some(number) = lower.strip_prefix('f')
        && let Ok(number) = number.parse::<u16>()
        && (1..=24).contains(&number)
    {
        return Some((vk::F1 + number - 1, None));
    }
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    if c.is_ascii_alphabetic() {
        let upper = c.to_ascii_uppercase();
        return Some((upper as u16, Some(c.to_ascii_lowercase() as u16)));
    }
    if c.is_ascii_digit() {
        return Some((c as u16, Some(c as u16)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sequence_splits_on_commas_and_keeps_escaped_ones_in_text() {
        let events = parse("ctrl+s,enter,text:FO4").unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0],
            KeyEvent::Key {
                vk: u16::from(b'S'),
                character: Some(0x13),
                ctrl: true,
                shift: false,
                alt: false
            }
        );
        assert_eq!(
            events[1],
            KeyEvent::Key {
                vk: vk::RETURN,
                character: Some(0x0D),
                ctrl: false,
                shift: false,
                alt: false
            }
        );
        assert_eq!(events[2], KeyEvent::Text("FO4".to_owned()));

        let events = parse(r"text:a\,b\, c").unwrap();
        assert_eq!(events, vec![KeyEvent::Text("a,b, c".to_owned())]);
    }

    #[test]
    fn modifiers_shift_case_and_control_characters() {
        let events = parse("shift+a,ctrl+shift+A,alt+f,menu:x").unwrap();
        assert_eq!(
            events[0],
            KeyEvent::Key {
                vk: u16::from(b'A'),
                character: Some(u16::from(b'A')),
                ctrl: false,
                shift: true,
                alt: false
            }
        );
        assert_eq!(
            events[1],
            KeyEvent::Key {
                vk: u16::from(b'A'),
                character: Some(0x01),
                ctrl: true,
                shift: true,
                alt: false
            }
        );
        assert_eq!(events[2], KeyEvent::Menu('f'));
        assert_eq!(events[3], KeyEvent::Menu('x'));
    }

    #[test]
    fn named_function_and_navigation_keys() {
        let events = parse("f12,pageup,pgdn,del,esc,space").unwrap();
        assert_eq!(
            events[0],
            KeyEvent::Key {
                vk: vk::F1 + 11,
                character: None,
                ctrl: false,
                shift: false,
                alt: false
            }
        );
        assert_eq!(
            events[1],
            KeyEvent::Key {
                vk: vk::PRIOR,
                character: None,
                ctrl: false,
                shift: false,
                alt: false
            }
        );
        assert_eq!(
            events[2],
            KeyEvent::Key {
                vk: vk::NEXT,
                character: None,
                ctrl: false,
                shift: false,
                alt: false
            }
        );
        assert_eq!(
            events[3],
            KeyEvent::Key {
                vk: vk::DELETE,
                character: None,
                ctrl: false,
                shift: false,
                alt: false
            }
        );
        assert_eq!(
            events[4],
            KeyEvent::Key {
                vk: vk::ESCAPE,
                character: Some(0x1B),
                ctrl: false,
                shift: false,
                alt: false
            }
        );
    }

    #[test]
    fn a_bad_event_names_what_is_wrong() {
        assert!(parse("ctrl+").unwrap_err().contains("unknown key"));
        assert!(parse("nope+a").unwrap_err().contains("unknown modifier"));
        assert!(parse("").unwrap_err().contains("empty"));
        assert!(parse("menu:ab").unwrap_err().contains("one mnemonic"));
    }
}
