// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Delphi runtime behaviour that xEdit's mod group files and its
//! settings file are read and written with: `TStrings.LoadFromFile` and
//! `SaveToFile` (the encoding found by its preamble, `TEncoding.Default`
//! otherwise, a CRLF after every line), `TStrings.CommaText`, and
//! `TMemIniFile` (`System.IniFiles`), which normalises a file when it
//! writes it back: comments and blank lines are dropped, the spaces around
//! the first `=` of a line go, and every section is followed by an empty
//! line. These are not upstream units; they are ported here because the
//! bytes xEdit writes depend on them.

use std::path::{Path, PathBuf};

use xedit_io::Encoding;

/// The encodings `TEncoding.GetBufferEncoding` tells apart by their
/// preamble, and the default it falls back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// `TEncoding.Default`: the ANSI code page of the system.
    Ansi,
    /// `TEncoding.UTF8`, with its preamble.
    Utf8,
    /// `TEncoding.Unicode` (UTF-16 LE).
    Unicode,
    /// `TEncoding.BigEndianUnicode`.
    BigEndianUnicode,
}

impl TextEncoding {
    /// `GetPreamble`: the bytes written before the text.
    pub fn preamble(self) -> &'static [u8] {
        match self {
            TextEncoding::Ansi => &[],
            TextEncoding::Utf8 => &[0xEF, 0xBB, 0xBF],
            TextEncoding::Unicode => &[0xFF, 0xFE],
            TextEncoding::BigEndianUnicode => &[0xFE, 0xFF],
        }
    }

    /// Port of `TEncoding.GetBufferEncoding` with `TEncoding.Default` as the
    /// default: the encoding of the preamble the bytes start with.
    pub fn detect(bytes: &[u8]) -> TextEncoding {
        [
            TextEncoding::Utf8,
            TextEncoding::Unicode,
            TextEncoding::BigEndianUnicode,
        ]
        .into_iter()
        .find(|encoding| bytes.starts_with(encoding.preamble()))
        .unwrap_or(TextEncoding::Ansi)
    }

    /// `GetString` of the bytes after the preamble.
    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            TextEncoding::Ansi => Encoding::Mbcs(0)
                .get_string(bytes)
                .unwrap_or_else(|_| bytes.iter().map(|&byte| char::from(byte)).collect()),
            TextEncoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            TextEncoding::Unicode | TextEncoding::BigEndianUnicode => {
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&pair| {
                        if self == TextEncoding::Unicode {
                            u16::from_le_bytes(pair)
                        } else {
                            u16::from_be_bytes(pair)
                        }
                    })
                    .collect();
                String::from_utf16_lossy(&units)
            }
        }
    }

    /// `GetBytes`.
    pub fn encode(self, text: &str) -> Vec<u8> {
        match self {
            TextEncoding::Ansi => Encoding::Mbcs(0).get_bytes(text),
            TextEncoding::Utf8 => text.as_bytes().to_vec(),
            TextEncoding::Unicode => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            TextEncoding::BigEndianUnicode => text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
        }
    }
}

/// Delphi `Trim`: the text without the characters up to the space (every
/// control character) at both ends.
pub fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| c <= ' ')
}

/// `SameText` and the comparisons of a `TStringList` that is not case
/// sensitive (`AnsiCompareText`): equal without regard to case.
pub fn same_text(a: &str, b: &str) -> bool {
    a == b || a.to_uppercase() == b.to_uppercase()
}

/// Port of `TStrings.SetTextStr`: the lines of a text, split at CR, LF and
/// CRLF; a line break at the end makes no empty last line.
pub fn split_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut pos = 0;
    while pos < bytes.len() {
        match bytes[pos] {
            b'\r' | b'\n' => {
                lines.push(text[start..pos].to_owned());
                if bytes[pos] == b'\r' && bytes.get(pos + 1) == Some(&b'\n') {
                    pos += 1;
                }
                pos += 1;
                start = pos;
            }
            _ => pos += 1,
        }
    }
    if start < bytes.len() {
        lines.push(text[start..].to_owned());
    }
    lines
}

/// The lines of a text file as `TStringList.LoadFromFile` reads them, with
/// the encoding it found (which `SaveToFile` writes the list back with).
pub fn load_strings(path: &Path) -> std::io::Result<(Vec<String>, TextEncoding)> {
    let bytes = std::fs::read(path)?;
    let encoding = TextEncoding::detect(&bytes);
    let text = encoding.decode(&bytes[encoding.preamble().len()..]);
    Ok((split_lines(&text), encoding))
}

/// Port of `TStrings.SaveToFile`: the preamble of the encoding, then each
/// line with CRLF after it. A list that was never loaded has no encoding
/// and writes `TEncoding.Default`.
pub fn save_strings(path: &Path, lines: &[String], encoding: Option<TextEncoding>) -> std::io::Result<()> {
    let encoding = encoding.unwrap_or(TextEncoding::Ansi);
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push_str("\r\n");
    }
    let mut bytes = encoding.preamble().to_vec();
    bytes.extend(encoding.encode(&text));
    std::fs::write(path, bytes)
}

/// Port of `TStrings.GetDelimitedText` with the defaults of `CommaText`
/// (`,` as the delimiter, `"` as the quote, not strict): an entry with a
/// quote, a comma or a character up to the space is quoted, a single empty
/// entry is `""`.
pub fn comma_text(items: &[String]) -> String {
    if items.len() == 1 && items[0].is_empty() {
        return "\"\"".to_owned();
    }
    items
        .iter()
        .map(|item| {
            if item.chars().any(|c| c == '"' || c == ',' || (c > '\0' && c <= ' ')) {
                format!("\"{}\"", item.replace('"', "\"\""))
            } else {
                item.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Port of `TStrings.SetDelimitedText` with the defaults of `CommaText`:
/// entries end at a comma or at a character up to the space, unless
/// quoted (`AnsiExtractQuotedStr`); a comma at the very end adds an empty
/// entry.
pub fn parse_comma_text(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().take_while(|&c| c != '\0').collect();
    let is_space = |c: char| c > '\0' && c <= ' ';
    let mut items = Vec::new();
    let mut pos = 0;
    while pos < chars.len() && is_space(chars[pos]) {
        pos += 1;
    }
    while pos < chars.len() {
        let mut item = String::new();
        if chars[pos] == '"' {
            // `AnsiExtractQuotedStr`: up to the closing quote, a doubled
            // quote is one quote.
            pos += 1;
            while pos < chars.len() {
                if chars[pos] == '"' {
                    if chars.get(pos + 1) == Some(&'"') {
                        item.push('"');
                        pos += 2;
                        continue;
                    }
                    pos += 1;
                    break;
                }
                item.push(chars[pos]);
                pos += 1;
            }
        } else {
            while pos < chars.len() && chars[pos] > ' ' && chars[pos] != ',' {
                item.push(chars[pos]);
                pos += 1;
            }
        }
        items.push(item);
        while pos < chars.len() && is_space(chars[pos]) {
            pos += 1;
        }
        if pos < chars.len() && chars[pos] == ',' {
            if pos + 1 == chars.len() {
                items.push(String::new());
            }
            pos += 1;
            while pos < chars.len() && is_space(chars[pos]) {
                pos += 1;
            }
        }
    }
    items
}

/// Port of `TMemIniFile`: the sections of an ini file in memory, in file
/// order, each with its lines. Section and key names compare without regard
/// to case; a section whose name the file had before is dropped with its
/// lines.
#[derive(Debug, Clone)]
pub struct MemIniFile {
    file_name: PathBuf,
    /// The encoding found when the file was read; `None` for a file that
    /// did not exist, which `UpdateFile` writes in `TEncoding.Default`.
    encoding: Option<TextEncoding>,
    sections: Vec<(String, Vec<String>)>,
}

impl MemIniFile {
    /// `TMemIniFile.Create(FileName)` with `LoadValues`: a missing file is
    /// an empty ini.
    pub fn open(file_name: &Path) -> std::io::Result<Self> {
        let mut ini = MemIniFile {
            file_name: file_name.to_owned(),
            encoding: None,
            sections: Vec::new(),
        };
        if file_name.is_file() {
            let (lines, encoding) = load_strings(file_name)?;
            ini.encoding = Some(encoding);
            ini.set_strings(&lines);
        }
        Ok(ini)
    }

    pub fn file_name(&self) -> &Path {
        &self.file_name
    }

    /// Port of `SetStrings`: every line trimmed; blank lines and lines
    /// that start with `;` dropped; `[name]` starts a section (the name
    /// trimmed); a line before the first section or in a section of a
    /// name seen before dropped; the spaces around the first `=` of a line
    /// removed.
    pub fn set_strings(&mut self, lines: &[String]) {
        self.sections.clear();
        let mut current: Option<usize> = None;
        for line in lines {
            let text = trim(line);
            if text.is_empty() || text.starts_with(';') {
                continue;
            }
            if text.len() >= 2 && text.starts_with('[') && text.ends_with(']') {
                let name = trim(&text[1..text.len() - 1]).to_owned();
                // A section whose name the file had before (in any case)
                // is dropped with its lines, as the GUI's rewrite of a
                // mod group file shows.
                current = if self.section_index(&name).is_some() {
                    None
                } else {
                    self.sections.push((name, Vec::new()));
                    Some(self.sections.len() - 1)
                };
            } else if let Some(index) = current {
                let value = match text.find('=') {
                    Some(at) => format!("{}={}", trim(&text[..at]), trim(&text[at + 1..])),
                    None => text.to_owned(),
                };
                self.sections[index].1.push(value);
            }
        }
    }

    fn section_index(&self, section: &str) -> Option<usize> {
        self.sections.iter().position(|(name, _)| same_text(name, section))
    }

    /// `ReadSections`: the section names in file order.
    pub fn read_sections(&self) -> Vec<String> {
        self.sections.iter().map(|(name, _)| name.clone()).collect()
    }

    /// `ReadSectionValues`: the lines of the first section of the name.
    pub fn read_section_values(&self, section: &str) -> Vec<String> {
        self.section_index(section)
            .map(|index| self.sections[index].1.clone())
            .unwrap_or_default()
    }

    /// `EraseSection`: removes the first section of the name.
    pub fn erase_section(&mut self, section: &str) {
        if let Some(index) = self.section_index(section) {
            self.sections.remove(index);
        }
    }

    /// `GetStrings`: each section as `[name]`, its lines and an empty line.
    pub fn get_strings(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (name, values) in &self.sections {
            lines.push(format!("[{name}]"));
            lines.extend(values.iter().cloned());
            lines.push(String::new());
        }
        lines
    }

    /// `ReadString`: the value of the first line of the section whose name
    /// (the text before its first `=`) is `ident`.
    pub fn read_string(&self, section: &str, ident: &str, default: &str) -> String {
        if let Some(index) = self.section_index(section)
            && let Some(line) = self.sections[index]
                .1
                .iter()
                .find(|line| name_of(line).is_some_and(|name| same_text(name, ident)))
        {
            return line.chars().skip(ident.chars().count() + 1).collect();
        }
        default.to_owned()
    }

    /// Port of `TMemIniFile.ReadBool` (`ReadInteger(Section, Ident,
    /// Ord(Default)) <> 0`, which reads `1`, `0` and every other integer).
    pub fn read_bool(&self, section: &str, ident: &str, default: bool) -> bool {
        let value =
            xedit_core::interface::misc::str_to_int_def(&self.read_string(section, ident, ""), i32::from(default));
        value != 0
    }

    /// `WriteString`: replaces the line of the key or adds it, adding the
    /// section at the end when it is missing.
    pub fn write_string(&mut self, section: &str, ident: &str, value: &str) {
        let index = match self.section_index(section) {
            Some(index) => index,
            None => {
                self.sections.push((section.to_owned(), Vec::new()));
                self.sections.len() - 1
            }
        };
        let line = format!("{ident}={value}");
        let values = &mut self.sections[index].1;
        match values
            .iter()
            .position(|line| name_of(line).is_some_and(|name| same_text(name, ident)))
        {
            Some(at) => values[at] = line,
            None => values.push(line),
        }
    }

    /// `UpdateFile`: [`MemIniFile::get_strings`] written in the encoding the
    /// file was read with.
    pub fn update_file(&self) -> std::io::Result<()> {
        save_strings(&self.file_name, &self.get_strings(), self.encoding)
    }
}

/// The name of a `name=value` line (`TStrings.IndexOfName`), `None` for a
/// line without `=`.
fn name_of(line: &str) -> Option<&str> {
    line.find('=').map(|at| &line[..at])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_split_at_every_line_break() {
        assert_eq!(split_lines("a\r\nb\nc\rd\r\n"), ["a", "b", "c", "d"]);
        assert_eq!(split_lines("a\r\n\r\n"), ["a", ""]);
        assert!(split_lines("").is_empty());
    }

    #[test]
    fn comma_text_quotes_as_delphi() {
        let items = ["A".to_owned(), "B C".to_owned(), "say \"x\"".to_owned(), String::new()];
        assert_eq!(comma_text(&items), "A,\"B C\",\"say \"\"x\"\"\",");
        assert_eq!(comma_text(&[String::new()]), "\"\"");
        assert_eq!(
            parse_comma_text("A,\"B C\",\"say \"\"x\"\"\","),
            ["A", "B C", "say \"x\"", ""]
        );
        assert_eq!(parse_comma_text(" B C , D"), ["B", "C", "D"]);
        assert!(parse_comma_text("").is_empty());
    }

    #[test]
    fn mem_ini_file_normalises_as_delphi() {
        let mut ini = MemIniFile {
            file_name: PathBuf::new(),
            encoding: None,
            sections: Vec::new(),
        };
        let lines: Vec<String> = [
            "loose=1",
            "; comment",
            " [ First ] ",
            "  key  =  value  ",
            "",
            "+Item.esp",
            "[Second]",
            "x=1",
            "[first]",
            "y=2",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        ini.set_strings(&lines);
        // The second `[first]` goes with its line.
        assert_eq!(ini.read_sections(), ["First", "Second"]);
        assert_eq!(ini.read_section_values("FIRST"), ["key=value", "+Item.esp"]);
        assert_eq!(ini.read_string("first", "KEY", "-"), "value");
        ini.write_string("second", "X", "2");
        ini.write_string("Third", "z", "3");
        ini.erase_section("first");
        assert_eq!(ini.get_strings(), ["[Second]", "X=2", "", "[Third]", "z=3", ""]);
    }

    #[test]
    fn encodings_are_found_by_their_preamble() {
        assert_eq!(TextEncoding::detect(b"\xEF\xBB\xBFx"), TextEncoding::Utf8);
        assert_eq!(TextEncoding::detect(b"\xFF\xFEx\0"), TextEncoding::Unicode);
        assert_eq!(TextEncoding::detect(b"x"), TextEncoding::Ansi);
        assert_eq!(TextEncoding::Unicode.decode(b"x\0y\0"), "xy");
    }
}
