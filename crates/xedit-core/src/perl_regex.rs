// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `TPerlRegEx` of `System.RegularExpressionsCore`: the class upstream's
//! scripts, Sniff's processors and the filter's regex comparison run.
//!
//! The engine is [`crate::pcre`]; this module is the class around it, with
//! the RTL's own rules that were read off the 4.1.5q release build (see the
//! probe of `cargo xtask parity script --script "Regex probe"`):
//!
//! - `MatchedOffset`, `GroupOffsets` and `Start` are 1-based **character**
//!   offsets into the subject; `MatchedLength`, `GroupLengths` and `Stop`
//!   count characters. The RTL converts PCRE's UTF-8 byte offsets, so a
//!   multi-byte character counts once.
//! - `Match`/`MatchAgain` search `[Start - 1, Stop)` and `MatchAgain`
//!   refuses only when `Start` has moved past `Stop` (`Start = Stop` still
//!   searches, which is how `ReplaceAll` appends a replacement at the very
//!   end of a subject it just replaced).
//! - A zero-length match moves the next search one character on.
//! - A failed search leaves the offsets of the last match in place, and
//!   `MatchedText`/`Groups` raise `Successful match required` without a
//!   match (`SubjectLeft`/`SubjectRight` do not, as in the RTL).
//! - `NamedGroup` answers `pcre_get_stringnumber`'s `PCRE_ERROR_NOSUBSTRING`
//!   (`-7`) for a name the pattern does not have.
//! - `preNotBOL`, `preNotEOL` and `preNotEmpty` are the RTL's `State`
//!   (`pcre_exec`'s `PCRE_NOTBOL`, `PCRE_NOTEOL`, `PCRE_NOTEMPTY`); the
//!   adapter registers no `State` property, so a script cannot set them,
//!   but Sniff and the filter drive the same type.

use std::rc::Rc;

use crate::pcre::{Captures, ExecFlags, Flags, Regex};

/// `System.RegularExpressionsConsts`.
pub const E_REGEX_CLASS: &str = "ERegularExpressionError";
pub const S_REGEX_MATCH_REQUIRED: &str = "Successful match required";
pub const S_REGEX_STRINGS_REQUIRED: &str = "Strings parameter cannot be nil";
/// The text `Compile` raises when `RegEx` is empty.
pub const S_REGEX_MISSING: &str = "A regular expression specified in RegEx is required";
/// PCRE's `PCRE_ERROR_NOSUBSTRING`, which `NamedGroup` returns for a name
/// the pattern does not have (the RTL does not check the error).
pub const PCRE_ERROR_NOSUBSTRING: i64 = -7;

/// `TPerlRegExOptions`: `preCaseLess = 0` .. `preNoAutoCapture = 6`.
pub const OPTION_CASE_LESS: i32 = 0;
pub const OPTION_MULTI_LINE: i32 = 1;
pub const OPTION_SINGLE_LINE: i32 = 2;
pub const OPTION_EXTENDED: i32 = 3;
pub const OPTION_ANCHORED: i32 = 4;
pub const OPTION_UN_GREEDY: i32 = 5;
pub const OPTION_NO_AUTO_CAPTURE: i32 = 6;
/// `TPerlRegExState`: `preNotBOL = 0`, `preNotEOL = 1`, `preNotEmpty = 2`.
pub const STATE_NOT_BOL: i32 = 0;
pub const STATE_NOT_EOL: i32 = 1;
pub const STATE_NOT_EMPTY: i32 = 2;

/// What a `TPerlRegEx` operation raises, as the adapter turns it into an
/// `ERegularExpressionError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegexError {
    /// `Successful match required`.
    MatchRequired,
    /// `Index out of bounds (n)`.
    IndexOutOfBounds(i64),
    /// A `Compile` failure: `Error in regular expression at offset N: text`.
    Compile(String),
}

impl RegexError {
    pub fn message(&self) -> String {
        match self {
            RegexError::MatchRequired => S_REGEX_MATCH_REQUIRED.to_owned(),
            RegexError::IndexOutOfBounds(index) => format!("Index out of bounds ({index})"),
            RegexError::Compile(message) => message.clone(),
        }
    }
}

/// One `TPerlRegEx`. Cloning keeps the compiled expression.
#[derive(Clone)]
pub struct PerlRegEx {
    regex: String,
    replacement: String,
    subject: String,
    /// `FOptions`: the `TPerlRegExOptions` bits.
    options: i32,
    /// `FState`: the `TPerlRegExState` bits.
    state: i32,
    compiled: bool,
    studied: bool,
    /// The built expression of `RegEx` with `Options`; `None` until
    /// `Compile`.
    expression: Option<Rc<Regex>>,
    /// `FStart`: a 0-based character index into `subject` where
    /// `MatchAgain` begins (`Start` reads it 1-based).
    start: usize,
    /// `FStop`: the number of characters `Match`/`MatchAgain` search
    /// through.
    stop: usize,
    /// `FOffsets`: the byte offsets of the last match by group, `None` for
    /// a group that did not participate. A failed search leaves them in
    /// place, as upstream's `pcre_exec` does (only `OffsetCount` goes
    /// non-positive).
    offsets: Vec<Option<(usize, usize)>>,
    /// `OffsetCount > 0`: whether the offsets are a match.
    found: bool,
    /// `FStoredGroups` of `StoreGroups`.
    stored_groups: Option<Vec<String>>,
}

impl Default for PerlRegEx {
    fn default() -> Self {
        PerlRegEx::new()
    }
}

impl PerlRegEx {
    pub fn new() -> PerlRegEx {
        PerlRegEx {
            regex: String::new(),
            replacement: String::new(),
            subject: String::new(),
            options: 0,
            // `FState := []` (`Create`).
            state: 0,
            compiled: false,
            studied: false,
            expression: None,
            start: 0,
            stop: 0,
            offsets: Vec::new(),
            found: false,
            stored_groups: None,
        }
    }

    // ---------------------------------------------------------------- state

    pub fn regex(&self) -> &str {
        &self.regex
    }

    /// `RegEx := value`: a change drops the compiled expression.
    pub fn set_regex(&mut self, value: String) {
        if self.regex != value {
            self.regex = value;
            self.clean_up();
        }
    }

    pub fn replacement(&self) -> &str {
        &self.replacement
    }

    pub fn set_replacement(&mut self, value: String) {
        self.replacement = value;
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// `Subject := value`: the search window becomes the whole subject.
    pub fn set_subject(&mut self, value: String) {
        if self.subject.is_empty() || self.subject != value {
            let length = value.chars().count();
            self.subject = value;
            self.start = 0;
            self.stop = length;
            if self.stored_groups.is_none() {
                // `SetSubject` clears the match unless `StoreGroups` kept
                // the groups.
                self.found = false;
            }
        }
    }

    /// `FOptions`: `TPerlRegExOptions(Byte(value))`.
    pub fn options(&self) -> i32 {
        self.options
    }

    pub fn set_options(&mut self, value: i32) {
        let options = value & 0xFF;
        if self.options != options {
            self.options = options;
            self.clean_up();
        }
    }

    /// `FState`: the `pcre_exec` flags.
    pub fn state(&self) -> i32 {
        self.state
    }

    pub fn set_state(&mut self, value: i32) {
        self.state = value & 0xFF;
    }

    /// `Start`: a 1-based character offset (`FStart + 1`).
    pub fn start(&self) -> i64 {
        self.start as i64 + 1
    }

    pub fn set_start(&mut self, value: i64) {
        self.start = if value < 1 { 0 } else { value as usize - 1 };
    }

    /// `Stop`: the character count the search is limited to.
    pub fn stop(&self) -> i64 {
        self.stop as i64
    }

    /// `Stop := value`: clamped to the subject's length.
    pub fn set_stop(&mut self, value: i64) {
        let value = if value < 0 { 0 } else { value as usize };
        self.stop = value.min(self.subject.chars().count());
    }

    pub fn compiled(&self) -> bool {
        self.compiled
    }

    pub fn studied(&self) -> bool {
        self.studied
    }

    pub fn found_match(&self) -> bool {
        self.found
    }

    /// `OffsetCount` (`pcre_exec`'s return): one more than the highest
    /// participating group, or 0 when there is no match.
    pub fn group_count(&self) -> i64 {
        self.offsets.len() as i64 - 1
    }

    /// `TPerlRegEx.CleanUp`: the compiled expression of an option or pattern
    /// change is dropped and the stored groups cleared.
    fn clean_up(&mut self) {
        self.compiled = false;
        self.studied = false;
        self.expression = None;
        self.stored_groups = None;
        self.offsets.clear();
        self.found = false;
    }

    // -------------------------------------------------------------- compile

    /// `TPerlRegEx.Compile`: the `options` flags and the pattern; the
    /// missing-expression and compile-error texts are the RTL's.
    pub fn compile(&mut self) -> Result<(), RegexError> {
        if self.regex.is_empty() {
            return Err(RegexError::Compile(S_REGEX_MISSING.to_owned()));
        }
        self.clean_up();
        let flags = Flags::from_options(self.options);
        let expression = Regex::compile(&self.regex, flags).map_err(|error| {
            // `Error in regular expression at offset %d: %s` with PCRE's
            // own offset and message.
            RegexError::Compile(format!(
                "Error in regular expression at offset {}: {}",
                error.offset, error.message
            ))
        })?;
        self.expression = Some(Rc::new(expression));
        self.compiled = true;
        Ok(())
    }

    /// `TPerlRegEx.Study`: compiles when needed; the port's expression is
    /// built once, so studying only records the state.
    pub fn study(&mut self) -> Result<(), RegexError> {
        if !self.compiled {
            self.compile()?;
        }
        self.studied = true;
        Ok(())
    }

    /// `NamedGroup(name)`: `pcre_get_stringnumber`, `-7`
    /// (`PCRE_ERROR_NOSUBSTRING`) for a name the pattern does not have.
    pub fn named_group(&self, name: &str) -> i64 {
        let Some(expression) = &self.expression else {
            return PCRE_ERROR_NOSUBSTRING;
        };
        match expression.group_number(name) {
            Some(number) => i64::from(number),
            None => PCRE_ERROR_NOSUBSTRING,
        }
    }

    // ---------------------------------------------------------------- match

    /// The one search both `Match` and `MatchAgain` run: from `from` within
    /// the window `[0, stop)`, honoring the `State` flags and recording the
    /// offsets.
    fn search(&mut self, from: usize) -> Result<bool, RegexError> {
        if self.expression.is_none() && !self.compiled {
            self.compile()?;
        }
        self.stored_groups = None;
        if self.subject.is_empty() {
            self.found = false;
            return Ok(false);
        }
        let hay = self.window();
        // `from` is a character index; the engine searches bytes.
        if from > hay.chars().count() {
            self.found = false;
            return Ok(false);
        }
        let from_byte = hay.char_indices().nth(from).map_or(hay.len(), |(at, _)| at);
        let exec = ExecFlags {
            not_bol: self.state & (1 << STATE_NOT_BOL) != 0,
            not_eol: self.state & (1 << STATE_NOT_EOL) != 0,
            not_empty: self.state & (1 << STATE_NOT_EMPTY) != 0,
            anchored: self.options & (1 << OPTION_ANCHORED) != 0,
        };
        let search = match &self.expression {
            Some(expression) => expression.exec(hay.as_bytes(), from_byte, hay.len(), exec),
            None => None,
        };
        match search {
            Some(captures) => {
                self.record(&captures);
                Ok(true)
            }
            None => {
                self.found = false;
                Ok(false)
            }
        }
    }

    /// The subject limited to the `Stop` window.
    fn window(&self) -> &str {
        match self.subject.char_indices().nth(self.stop) {
            Some((at, _)) => &self.subject[..at],
            None => &self.subject,
        }
    }

    /// Records a match's offsets: the byte offsets of every group, with
    /// PCRE's "highest participating group" count.
    fn record(&mut self, captures: &Captures) {
        let mut offsets: Vec<Option<(usize, usize)>> = Vec::new();
        let mut highest = 0usize;
        for (index, found) in captures.iter().enumerate() {
            match found {
                Some((start, end)) => {
                    offsets.push(Some((*start, *end)));
                    highest = index;
                }
                None => offsets.push(None),
            }
        }
        // `pcre_exec` returns one more than the highest group that
        // participated; the port trims the vector to it.
        offsets.truncate(highest + 1);
        self.offsets = offsets;
        self.found = true;
        let whole = self.offsets[0].expect("group 0");
        let end = self.char_index(whole.1);
        self.start = end;
        if whole.0 == whole.1 {
            // "Make sure we don't get stuck at the same position."
            self.start = end + 1;
        }
    }

    /// `Match`: the search from the beginning of the subject (`startoffset
    /// 0`).
    pub fn match_(&mut self) -> Result<bool, RegexError> {
        self.search(0)
    }

    /// `MatchAgain`: from `FStart`; the empty subject and a start past the
    /// stop fail without a search.
    pub fn match_again(&mut self) -> Result<bool, RegexError> {
        if self.subject.is_empty() || self.start > self.stop {
            self.found = false;
            return Ok(false);
        }
        self.search(self.start)
    }

    /// `Replace`: returns the replacement text, substitutes it into the
    /// subject for the current match and moves the search position.
    pub fn replace(&mut self) -> Result<String, RegexError> {
        if !self.found_match() {
            return Err(RegexError::MatchRequired);
        }
        let replacement = self.compute_replacement()?;
        let (start, end) = self.offsets[0].expect("group 0");
        let start = self.char_index(start);
        let end = self.char_index(end);
        let length = end - start;
        let replacement_chars = replacement.chars().count();
        let mut subject = String::with_capacity(self.subject.len() + replacement.len());
        subject.push_str(&self.char_slice(0, start));
        subject.push_str(&replacement);
        subject.push_str(&self.char_slice(end, usize::MAX));
        self.subject = subject;
        // `FStart := FStart - MatchedLength + Length(Tmp); FStop := ...`.
        self.start = self.start.saturating_sub(length) + replacement_chars;
        self.stop = self.stop.saturating_sub(length) + replacement_chars;
        self.stored_groups = None;
        self.found = false;
        Ok(replacement)
    }

    /// `ReplaceAll`: `Match`, then `Replace`/`MatchAgain` until they drop.
    pub fn replace_all(&mut self) -> Result<bool, RegexError> {
        if !self.match_()? {
            return Ok(false);
        }
        loop {
            self.replace()?;
            if !self.match_again()? {
                return Ok(true);
            }
        }
    }

    // ------------------------------------------------------------ accessors

    /// `GetGroups(Index)`: empty for an index past the group count, a
    /// stored copy after `StoreGroups`, else the subject's slice.
    pub fn groups(&self, index: i64) -> Result<String, RegexError> {
        if !self.found_match() {
            return Err(RegexError::MatchRequired);
        }
        if index > self.group_count() {
            return Ok(String::new());
        }
        if let Some(stored) = &self.stored_groups {
            return Ok(stored.get(index as usize).cloned().unwrap_or_default());
        }
        match self.offsets.get(index as usize).copied().flatten() {
            Some((start, end)) => Ok(self.char_slice(self.char_index(start), self.char_index(end))),
            None => Ok(String::new()),
        }
    }

    /// `GetGroupOffsets(Index)`: a 1-based character offset; a group that
    /// did not participate reads 1.
    pub fn group_offsets(&self, index: i64) -> Result<i64, RegexError> {
        if !self.found_match() {
            return Err(RegexError::MatchRequired);
        }
        if index < 0 || index > self.group_count() {
            return Err(RegexError::IndexOutOfBounds(index));
        }
        Ok(match self.offsets.get(index as usize).copied().flatten() {
            Some((start, _)) => self.char_index(start) as i64 + 1,
            None => 1,
        })
    }

    /// `GetGroupLengths(Index)`.
    pub fn group_lengths(&self, index: i64) -> Result<i64, RegexError> {
        if !self.found_match() {
            return Err(RegexError::MatchRequired);
        }
        if index < 0 || index > self.group_count() {
            return Err(RegexError::IndexOutOfBounds(index));
        }
        Ok(match self.offsets.get(index as usize).copied().flatten() {
            Some((start, end)) => (self.char_index(end) - self.char_index(start)) as i64,
            None => 0,
        })
    }

    /// `MatchedText`.
    pub fn matched_text(&self) -> Result<String, RegexError> {
        self.groups(0)
    }

    /// `MatchedOffset`: a 1-based character offset.
    pub fn matched_offset(&self) -> Result<i64, RegexError> {
        if !self.found_match() {
            return Err(RegexError::MatchRequired);
        }
        let whole = self.offsets[0].expect("group 0");
        Ok(self.char_index(whole.0) as i64 + 1)
    }

    /// `MatchedLength`: a character count.
    pub fn matched_length(&self) -> Result<i64, RegexError> {
        if !self.found_match() {
            return Err(RegexError::MatchRequired);
        }
        let whole = self.offsets[0].expect("group 0");
        Ok((self.char_index(whole.1) - self.char_index(whole.0)) as i64)
    }

    /// `SubjectLeft`/`SubjectRight`: the parts around the last match. The
    /// RTL does not check `FoundMatch` here, so after a failed search the
    /// stale offsets of the previous match are still used (and a
    /// zero-initial offset with no match at all gives the whole subject on
    /// the right).
    pub fn subject_left(&self) -> String {
        let end = self
            .offsets
            .first()
            .and_then(|group| group.map(|(start, _)| start))
            .map_or(0, |start| self.char_index(start));
        self.char_slice(0, end)
    }

    pub fn subject_right(&self) -> String {
        let start = self
            .offsets
            .first()
            .and_then(|group| group.map(|(_, end)| end))
            .map_or(0, |end| self.char_index(end));
        self.char_slice(start, usize::MAX)
    }

    /// `StoreGroups`: a copy of every group, kept across subject changes.
    pub fn store_groups(&mut self) -> Result<(), RegexError> {
        if !self.found_match() {
            return Ok(());
        }
        let mut stored = Vec::new();
        for index in 0..=self.group_count() {
            stored.push(self.groups(index)?);
        }
        self.stored_groups = Some(stored);
        Ok(())
    }

    // ---------------------------------------------------------- replacement

    /// `ComputeReplacement`: `Replacement` with the backreferences
    /// substituted for the current match. The JGsoft flavor of the RTL:
    /// `\n`/`$n` (one or two digits, the second only when the group
    /// exists), `${n}`, `${name}`, `\g<name>`, `$&`/`\&`, `$+`/`\+`,
    /// `` $` ``, `$'`, `$_`, `\\`, `\$`, and the case modes `\l \L \u \U \f
    /// \F \i \I`.
    pub fn compute_replacement(&self) -> Result<String, RegexError> {
        if self.replacement.is_empty() {
            return Ok(String::new());
        }
        if !self.found_match() {
            return Err(RegexError::MatchRequired);
        }
        let chars: Vec<char> = self.replacement.chars().collect();
        let mut out = String::new();
        let mut index = 0usize;
        // "We let I stop one character before the end": a final `\` or `$`
        // is plain text.
        while index + 1 < chars.len() {
            let current = chars[index];
            if current != '\\' && current != '$' {
                out.push(current);
                index += 1;
                continue;
            }
            let dollar = current == '$';
            let mut cursor = index + 1;
            let mut mode = '\0';
            let mut number_only = false;
            let mut handled = false;
            // `$$` is a literal dollar sign, as `\$` is.
            if dollar && chars[cursor] == '$' {
                out.push('$');
                index = cursor + 1;
                continue;
            }
            if !dollar {
                match chars[cursor] {
                    '$' | '\\' => {
                        out.push(chars[cursor]);
                        index = cursor + 1;
                        handled = true;
                    }
                    'g' => {
                        if cursor + 2 < chars.len() && chars[cursor + 1] == '<' {
                            let name_start = cursor + 2;
                            let mut end = name_start;
                            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                                end += 1;
                            }
                            if end < chars.len() && chars[end] == '>' {
                                let name: String = chars[name_start..end].iter().collect();
                                let group = self.named_group(&name);
                                if group > 0 {
                                    out.push_str(&self.groups(group)?);
                                }
                                index = end + 1;
                            } else {
                                out.extend(&chars[index..end]);
                                index = end;
                            }
                        } else {
                            out.extend(&chars[index..index + 2]);
                            index += 2;
                        }
                        handled = true;
                    }
                    'l' | 'L' | 'u' | 'U' | 'f' | 'F' | 'i' | 'I' => {
                        mode = chars[cursor];
                        cursor += 1;
                        number_only = true;
                    }
                    _ => {}
                }
            }
            if handled {
                continue;
            }
            // ProcessBackreference.
            let mut number: i64 = -1;
            if cursor < chars.len() && chars[cursor].is_ascii_digit() {
                number = i64::from(chars[cursor] as u32 - '0' as u32);
                cursor += 1;
                if cursor < chars.len() && chars[cursor].is_ascii_digit() {
                    let two = number * 10 + i64::from(chars[cursor] as u32 - '0' as u32);
                    if two <= self.group_count() {
                        number = two;
                        cursor += 1;
                    }
                }
            } else if !number_only && cursor < chars.len() {
                if dollar && chars[cursor] == '{' {
                    cursor += 1;
                    if cursor < chars.len() && chars[cursor].is_ascii_digit() {
                        number = 0;
                        while cursor < chars.len() && chars[cursor].is_ascii_digit() {
                            number = number
                                .saturating_mul(10)
                                .saturating_add(i64::from(chars[cursor] as u32 - '0' as u32));
                            cursor += 1;
                        }
                    } else if cursor < chars.len() && (chars[cursor].is_ascii_alphabetic() || chars[cursor] == '_') {
                        let start = cursor;
                        cursor += 1;
                        while cursor < chars.len() && (chars[cursor].is_ascii_alphanumeric() || chars[cursor] == '_') {
                            cursor += 1;
                        }
                        if cursor < chars.len() && chars[cursor] == '}' {
                            let name: String = chars[start..cursor].iter().collect();
                            number = self.named_group(&name);
                        }
                    }
                    if cursor >= chars.len() || chars[cursor] != '}' {
                        number = -1;
                    } else {
                        cursor += 1;
                    }
                } else if dollar && chars[cursor] == '_' {
                    out.push_str(&self.subject);
                    index = cursor + 1;
                    continue;
                } else {
                    match chars[cursor] {
                        '&' => {
                            number = 0;
                            cursor += 1;
                        }
                        '+' => {
                            number = self.group_count();
                            cursor += 1;
                        }
                        '`' => {
                            out.push_str(&self.subject_left());
                            index = cursor + 1;
                            continue;
                        }
                        '\'' => {
                            out.push_str(&self.subject_right());
                            index = cursor + 1;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            if number >= 0 {
                let value = self.groups(number)?;
                let value = match mode {
                    'L' | 'l' => value.to_lowercase(),
                    'U' | 'u' => value.to_uppercase(),
                    'F' | 'f' => first_cap(&value),
                    'I' | 'i' => initial_caps(&value),
                    _ => value,
                };
                out.push_str(&value);
                index = cursor;
            } else {
                out.push(current);
                index += 1;
            }
        }
        if index < chars.len() {
            out.extend(&chars[index..]);
        }
        Ok(out)
    }

    // --------------------------------------------------------------- split

    /// `Split(Strings, Limit)`: the subject's non-matching pieces, the
    /// capturing groups ignored.
    pub fn split(&mut self, pieces: &mut Vec<String>, limit: i32) -> Result<(), RegexError> {
        if limit == 1 || !self.match_()? {
            pieces.push(self.subject.clone());
            return Ok(());
        }
        let mut offset = 0usize;
        let mut count = 1;
        loop {
            let (start, _) = self.offsets[0].expect("group 0");
            let start = self.char_index(start);
            pieces.push(self.char_slice(offset, start));
            count += 1;
            let (_, end) = self.offsets[0].expect("group 0");
            offset = self.char_index(end);
            if (limit > 1 && count >= limit) || !self.match_again()? {
                break;
            }
        }
        pieces.push(self.char_slice(offset, usize::MAX));
        Ok(())
    }

    /// `SplitCapture(Strings, Limit)`: the pieces plus the last capturing
    /// group of every match. The RTL's `Offset` parameter is not part of the
    /// adapter's two-argument registration and stays 1.
    pub fn split_capture(&mut self, pieces: &mut Vec<String>, limit: i32) -> Result<(), RegexError> {
        if limit == 1 || !self.match_()? {
            pieces.push(self.subject.clone());
            return Ok(());
        }
        let mut position = 0usize;
        let mut count = 1;
        loop {
            let (start, _) = self.offsets[0].expect("group 0");
            let start = self.char_index(start);
            pieces.push(self.char_slice(position, start));
            count += 1;
            if self.group_count() > 0 {
                pieces.push(self.groups(self.group_count())?);
            }
            let (_, end) = self.offsets[0].expect("group 0");
            position = self.char_index(end);
            if (limit > 1 && count >= limit) || !self.match_again()? {
                break;
            }
        }
        pieces.push(self.char_slice(position, usize::MAX));
        Ok(())
    }

    // -------------------------------------------------------------- helpers

    /// The character index of a byte offset.
    fn char_index(&self, byte: usize) -> usize {
        let byte = byte.min(self.subject.len());
        self.subject[..byte].chars().count()
    }

    /// The characters `[start, end)`, clamped to the subject.
    fn char_slice(&self, start: usize, end: usize) -> String {
        self.subject
            .chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect()
    }

    /// `EscapeRegExChars`: the RTL's 2015 list of characters that get a
    /// backslash, `#0` becoming `\0`.
    pub fn escape_regex_chars(text: &str) -> String {
        let mut out = String::new();
        for ch in text.chars() {
            match ch {
                '\\' | '[' | ']' | '^' | '$' | '.' | '|' | '?' | '*' | '+' | '-' | '(' | ')' | '{' | '}' | '&'
                | '<' | '>' => {
                    out.push('\\');
                    out.push(ch);
                }
                '\0' => out.push_str("\\0"),
                ch => out.push(ch),
            }
        }
        out
    }
}

/// `FirstCap`: the text lower case with its first character upper.
fn first_cap(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    }
}

/// `InitialCaps`: the text lower case with the first character after a
/// word break upper.
fn initial_caps(text: &str) -> String {
    let lowered: String = text.to_lowercase();
    let mut out = String::new();
    let mut upper = true;
    for ch in lowered.chars() {
        match ch {
            '\0'..='&' | '(' | '*' | '+' | ',' | '-' | '.' | '?' | '<' | '[' | '{' | '\u{B7}' => {
                upper = true;
                out.push(ch);
            }
            '\'' => out.push(ch),
            ch if upper => {
                out.extend(ch.to_uppercase());
                upper = false;
            }
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex(pattern: &str) -> PerlRegEx {
        let mut object = PerlRegEx::new();
        object.set_regex(pattern.to_owned());
        object
    }

    fn walk(pattern: &str, subject: &str, options: i32) -> Vec<String> {
        let mut object = regex(pattern);
        object.set_options(options);
        object.set_subject(subject.to_owned());
        let mut found = Vec::new();
        while object.match_again().expect("a search") {
            found.push(format!(
                "{}+{}:{}",
                object.matched_offset().expect("offset"),
                object.matched_length().expect("length"),
                object.matched_text().expect("text")
            ));
        }
        found
    }

    /// The lines the 4.1.5q probe produced for the same inputs.
    #[test]
    fn probe_walks() {
        assert_eq!(walk("a+", "xaay", 0), ["2+2:aa"]);
        assert_eq!(walk(".", "ab", 0), ["1+1:a", "2+1:b"]);
        assert_eq!(walk(".", "a\u{e9}z", 0), ["1+1:a", "2+1:\u{e9}", "3+1:z"]);
        assert_eq!(walk("\\w", "a\u{e9}z", 0), ["1+1:a", "3+1:z"]);
        assert_eq!(walk("x*", "abc", 0), ["1+0:", "2+0:", "3+0:", "4+0:"]);
        assert_eq!(walk("[a]*", "aab", 0), ["1+2:aa", "3+0:", "4+0:"]);
        assert_eq!(walk("a|", "ab", 0), ["1+1:a", "2+0:", "3+0:"]);
        assert_eq!(walk("$", "abc", 0), ["4+0:"]);
        assert_eq!(walk("$", "a\nb", 2), ["2+0:", "4+0:"]);
        assert_eq!(walk("a(?=b)", "ab ac", 0), ["1+1:a"]);
        assert_eq!(walk("(?<=a)b", "ab cb", 0), ["2+1:b"]);
        assert_eq!(walk("(a)\\1", "aa ab", 0), ["1+2:aa"]);
        assert_eq!(walk("a++b", "aab", 0), ["1+3:aab"]);
        assert_eq!(walk("a*+a", "aa", 0), Vec::<String>::new());
        assert_eq!(walk("(?>a+)a", "aa", 0), Vec::<String>::new());
        assert_eq!(walk("a.*b", "axbxb", 32), ["1+3:axb"]);
        assert_eq!(walk("a$", "a\n", 0), ["1+1:a"]);
        assert_eq!(walk("a$", "a\n\n", 0), Vec::<String>::new());
    }

    #[test]
    fn probe_replacements() {
        let replaced = |pattern: &str, subject: &str, replacement: &str| {
            let mut object = regex(pattern);
            object.set_options(1);
            object.set_subject(subject.to_owned());
            object.set_replacement(replacement.to_owned());
            let matched = object.replace_all().expect("a replacement");
            (matched, object.subject().to_owned())
        };
        assert_eq!(replaced("x*", "abc", "-"), (true, "-a-b-c-".to_owned()));
        assert_eq!(replaced("b*", "abc", "-"), (true, "-a--c-".to_owned()));
        assert_eq!(replaced("b+", "abc", "-"), (true, "a-c".to_owned()));
        assert_eq!(replaced("(a)(b)", "xaby", "\\2\\1"), (true, "xbay".to_owned()));
        assert_eq!(replaced("(a)", "aa", "<$1>"), (true, "<a><a>".to_owned()));
        assert_eq!(replaced("b", "", "x"), (false, String::new()));
        assert_eq!(replaced("b", "aBc", "x"), (true, "axc".to_owned()));
        // `$$` is a literal dollar sign.
        let mut object = regex("a");
        object.set_subject("a".to_owned());
        object.set_replacement("$$".to_owned());
        assert!(object.match_().expect("a match"));
        assert_eq!(object.compute_replacement().expect("a replacement"), "$");
        let mut object = regex("a");
        object.set_subject("a".to_owned());
        object.set_replacement("[$`$$$&]".to_owned());
        assert!(object.match_().expect("a match"));
        assert_eq!(object.compute_replacement().expect("a replacement"), "[$a]");
        // The whole-subject replacement of a multi-byte subject appends one
        // more replacement, as the release does.
        assert_eq!(replaced(".", "a\u{e9}z", "-"), (true, "---".to_owned()));
        assert_eq!(replaced(".*", "a\u{e9}z", "-"), (true, "--".to_owned()));
    }

    #[test]
    fn probe_groups_and_windows() {
        let mut object = regex("(a(.))z");
        object.set_subject("a\u{e9}z".to_owned());
        assert!(object.match_().expect("a match"));
        assert_eq!(object.group_count(), 2);
        assert_eq!(object.groups(0).expect("g0"), "a\u{e9}z");
        assert_eq!(object.groups(1).expect("g1"), "a\u{e9}");
        assert_eq!(object.groups(2).expect("g2"), "\u{e9}");
        assert_eq!(object.group_offsets(0).expect("offset"), 1);
        assert_eq!(object.group_lengths(0).expect("length"), 3);
        assert_eq!(object.group_offsets(1).expect("offset"), 1);
        assert_eq!(object.group_lengths(1).expect("length"), 2);
        assert_eq!(object.group_offsets(2).expect("offset"), 2);
        assert_eq!(object.group_lengths(2).expect("length"), 1);
        assert_eq!(object.matched_offset().expect("offset"), 1);
        assert_eq!(object.matched_length().expect("length"), 3);
        assert_eq!(object.groups(5).expect("past the count"), "");
        assert_eq!(
            object.group_offsets(5).expect_err("past the count"),
            RegexError::IndexOutOfBounds(5)
        );

        let mut object = regex("[a-z][0-9]");
        object.set_subject("a1 b2 c3".to_owned());
        assert!(object.match_().expect("a match"));
        assert_eq!(object.matched_text().expect("text"), "a1");
        assert!(object.match_again().expect("a match"));
        assert_eq!(object.matched_text().expect("text"), "b2");
        object.set_start(4);
        assert!(object.match_again().expect("a match"));
        assert_eq!(object.matched_text().expect("text"), "b2");
        object.set_start(1);
        object.set_stop(2);
        assert!(object.match_().expect("a match"));
        assert_eq!(object.matched_text().expect("text"), "a1");
        assert!(!object.match_again().expect("a search"));

        let mut object = regex("z");
        object.set_subject("a\u{e9}z".to_owned());
        assert!(object.match_().expect("a match"));
        assert_eq!(object.matched_offset().expect("offset"), 3);
        assert_eq!(object.start(), 4, "Start is a 1-based character offset");
        assert_eq!(object.stop(), 3, "Stop is the subject's character count");
    }

    #[test]
    fn probe_misc() {
        let mut object = regex("(a)(b)");
        object.set_subject("ab".to_owned());
        assert!(object.match_().expect("a match"));
        object.store_groups().expect("storing");
        object.set_subject("xy".to_owned());
        assert_eq!(object.groups(0).expect("g0"), "ab");
        assert_eq!(object.groups(1).expect("g1"), "a");
        assert_eq!(object.group_count(), 2);

        let mut object = regex("(b)(c)?");
        object.set_subject("abd".to_owned());
        assert!(object.match_().expect("a match"));
        assert_eq!(object.group_count(), 1, "only group 1 participated");
        assert_eq!(object.groups(2).expect("g2"), "");

        let mut object = regex("(a)(b)");
        object.compile().expect("a compile");
        assert_eq!(object.named_group("nope"), PCRE_ERROR_NOSUBSTRING);
        assert_eq!(
            object.named_group("a"),
            PCRE_ERROR_NOSUBSTRING,
            "no name in the pattern"
        );
        let mut empty = PerlRegEx::new();
        assert_eq!(empty.compile().expect_err("a compile").message(), S_REGEX_MISSING);

        let mut list = Vec::new();
        let mut object = regex(",");
        object.set_subject("a,b,,c".to_owned());
        object.split(&mut list, 0).expect("a split");
        assert_eq!(list, ["a", "b", "", "c"]);

        let mut list = Vec::new();
        let mut object = regex("([a-z])([0-9])");
        object.set_subject("a1b2c".to_owned());
        object.split_capture(&mut list, 0).expect("a split");
        assert_eq!(list, ["", "1", "", "2", "c"]);

        assert_eq!(
            PerlRegEx::escape_regex_chars("a.b(c)[d]{e}|f*g?h+i^j$k-l&m<n>o"),
            "a\\.b\\(c\\)\\[d\\]\\{e\\}\\|f\\*g\\?h\\+i\\^j\\$k\\-l\\&m\\<n\\>o"
        );
    }

    #[test]
    fn probe_compile_error() {
        let mut object = regex("(?<=a+)b");
        assert_eq!(
            object.compile().expect_err("a compile error").message(),
            "Error in regular expression at offset 6: lookbehind assertion is not fixed length"
        );
    }

    #[test]
    fn probe_no_auto_capture_and_state() {
        let mut object = regex("(a)(b)");
        object.set_options(1 << OPTION_NO_AUTO_CAPTURE);
        object.set_subject("ab".to_owned());
        assert!(object.match_().expect("a match"));
        assert_eq!(object.group_count(), 0);
        assert_eq!(object.matched_text().expect("text"), "ab");

        // `PCRE_NOTEMPTY` skips an empty match and finds the next one.
        let mut object = regex("b*");
        object.set_state(1 << STATE_NOT_EMPTY);
        object.set_subject("abc".to_owned());
        assert!(object.match_().expect("a match"));
        assert_eq!(object.matched_text().expect("text"), "b");

        // `PCRE_NOTBOL`: `^` does not match at the start of the subject.
        let mut object = regex("^a");
        object.set_state(1 << STATE_NOT_BOL);
        object.set_subject("ab".to_owned());
        assert!(!object.match_().expect("a search"));

        // `PCRE_NOTEOL`: `$` does not match at the end of the subject.
        let mut object = regex("b$");
        object.set_state(1 << STATE_NOT_EOL);
        object.set_subject("ab".to_owned());
        assert!(!object.match_().expect("a search"));
    }
}
