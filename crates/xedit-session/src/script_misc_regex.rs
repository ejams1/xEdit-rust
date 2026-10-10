// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterMiscRegEx.pas (the
// TPerlRegEx class and its 48 registrations) over the semantics of
// System.RegularExpressionsCore.pas (TPerlRegEx by Jan Goyvaerts: Match,
// MatchAgain, Replace, ReplaceAll, ComputeReplacement, Split, SplitCapture,
// StoreGroups, Study, Compile, EscapeRegExChars, the Start/Stop window and
// the group accessors) and System.RegularExpressionsConsts.pas (the
// ERegularExpressionError texts).

//! `TPerlRegEx` for scripts, over the `regex` crate.
//!
//! UPSTREAM-QUIRK: upstream runs PCRE through `System.RegularExpressionsCore`
//! over UTF-8 bytes; the port runs the `regex` crate over characters. The
//! call shape, the option/State flags, the offset bases and the error texts
//! are the upstream ones; the divergences are:
//!
//! - `regex` has no lookaround, no backreferences inside the pattern and no
//!   possessive quantifiers: a pattern that uses one fails to compile with
//!   the crate's message where PCRE compiled it (recorded as phase 5's owed
//!   divergence for Sniff's `perl_regex.rs`, the same decision here).
//! - `\d`, `\w` and `\s` match Unicode characters where PCRE without UCP
//!   matched ASCII ones.
//! - `preNotBOL` and `preNotEOL` have no `regex` equivalent (`PCRE_NOTBOL`
//!   and `PCRE_NOTBOL`); a pattern anchored with `^`/`$` behaves as if the
//!   flags were not set. `preNotEmpty` is emulated (empty matches are
//!   skipped), and `preAnchored` by requiring the match to start at the
//!   search position.
//! - offsets and lengths count characters; upstream counts UTF-8 bytes and
//!   converts for the group accessors, which agree for ASCII subjects. The
//!   `regex` crate reports byte offsets, so the port converts them.
//! - `PCRE_UNGREEDY` is `regex`'s `swap_greed`; `PCRE_NO_AUTO_CAPTURE` has
//!   no `regex` equivalent (documented).
//! - `pcre_get_stringnumber` is the crate's capture-name table.

use std::rc::Rc;

use regex::{Captures, Regex, RegexBuilder};

use xedit_script::adapter::{Adapter, Args, PR_ARGS_NO_CHECK};
use xedit_script::values::{ClassDef, ParamType, ScriptObject, Value, ValueError, VarType};

use crate::script_classes::ScriptClasses;
use crate::script_strings::{borrow_list_mut, list_of_or_error, receiver, text_arg};

/// `System.RegularExpressionsConsts` texts.
pub const E_REGEX_CLASS: &str = "ERegularExpressionError";
pub const S_REGEX_MATCH_REQUIRED: &str = "Successful match required";
pub const S_REGEX_STRINGS_REQUIRED: &str = "Strings parameter cannot be nil";

fn index_out_of_bounds(index: i64) -> ValueError {
    ValueError::external_class(E_REGEX_CLASS, format!("Index out of bounds ({index})"))
}

fn match_required() -> ValueError {
    ValueError::external_class(E_REGEX_CLASS, S_REGEX_MATCH_REQUIRED.to_owned())
}

/// `TPerlRegExOptions` (`System.RegularExpressionsCore.pas:25`):
/// `preCaseLess = 0` .. `preNoAutoCapture = 6`.
pub const OPTION_CASE_LESS: i32 = 0;
pub const OPTION_MULTI_LINE: i32 = 1;
pub const OPTION_SINGLE_LINE: i32 = 2;
pub const OPTION_EXTENDED: i32 = 3;
pub const OPTION_ANCHORED: i32 = 4;
pub const OPTION_UN_GREEDY: i32 = 5;
pub const OPTION_NO_AUTO_CAPTURE: i32 = 6;
/// `TPerlRegExState` (`:36`): `preNotBOL = 0`, `preNotEOL = 1`,
/// `preNotEmpty = 2`. The State property is not registered for scripts; the
/// port keeps it for the option values' shared ordinals and documents the
/// emulation.
pub const STATE_NOT_BOL: i32 = 0;
pub const STATE_NOT_EOL: i32 = 1;
pub const STATE_NOT_EMPTY: i32 = 2;

/// One `TPerlRegEx`.
pub struct PerlRegExObject {
    class: Rc<ClassDef>,
    regex: String,
    replacement: String,
    subject: String,
    options: i32,
    state: i32,
    compiled: bool,
    studied: bool,
    /// The built expression of `RegEx` with `Options`; `None` until
    /// `Compile`.
    expression: Option<Regex>,
    /// `FStart`, 0-based character index into `subject` where `MatchAgain`
    /// begins (upstream: a 0-based byte index).
    start: usize,
    /// `FStop`: the number of characters `Match`/`MatchAgain` search
    /// through (upstream: bytes).
    stop: usize,
    /// `Offsets` of the last match, 0-based character pairs by group;
    /// `None` inside a pair for a group that did not participate. A failed
    /// search leaves the last match's offsets in place, as upstream's
    /// `pcre_exec` does (only `OffsetCount` goes non-positive).
    offsets: Vec<Option<(usize, usize)>>,
    /// `OffsetCount > 0`: whether the offsets are a match.
    found: bool,
    /// `FStoredGroups` of `StoreGroups`.
    stored_groups: Option<Vec<String>>,
}

impl PerlRegExObject {
    pub fn new(class: Rc<ClassDef>) -> PerlRegExObject {
        PerlRegExObject {
            class,
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

    fn found_match(&self) -> bool {
        self.found
    }

    /// `OffsetCount` (`pcre_exec`'s return): one more than the highest
    /// participating group, or 0 when there is no match.
    fn group_count(&self) -> i64 {
        self.offsets.len() as i64 - 1
    }

    /// `TPerlRegEx.CleanUp` (`:405` of the RTL): the compiled expression of
    /// an option or pattern change is dropped and the stored groups cleared.
    fn clean_up(&mut self) {
        self.compiled = false;
        self.studied = false;
        self.expression = None;
        self.stored_groups = None;
        self.offsets.clear();
        self.found = false;
    }

    /// `TPerlRegEx.Compile`: the `options` flags and the pattern; the
    /// missing-expression and compile-error texts are the RTL's.
    fn compile(&mut self) -> Result<(), ValueError> {
        if self.regex.is_empty() {
            return Err(ValueError::external_class(
                E_REGEX_CLASS,
                "A regular expression specified in RegEx is required".to_owned(),
            ));
        }
        self.clean_up();
        let mut builder = RegexBuilder::new(&self.regex);
        builder
            .case_insensitive(self.options & (1 << OPTION_CASE_LESS) != 0)
            .multi_line(self.options & (1 << OPTION_MULTI_LINE) != 0)
            .dot_matches_new_line(self.options & (1 << OPTION_SINGLE_LINE) != 0)
            .ignore_whitespace(self.options & (1 << OPTION_EXTENDED) != 0)
            .swap_greed(self.options & (1 << OPTION_UN_GREEDY) != 0)
            .unicode(true);
        let expression = builder.build().map_err(|error| {
            // The RTL formats PCRE's byte offset into the pattern; the
            // `regex` crate reports the position inside its own message, so
            // the port leaves the offset 0 (documented divergence).
            ValueError::external_class(
                E_REGEX_CLASS,
                format!("Error in regular expression at offset 0: {error}"),
            )
        })?;
        self.expression = Some(expression);
        self.compiled = true;
        Ok(())
    }

    /// `TPerlRegEx.Study`: compiles when needed; the port's expression is
    /// built once, so studying only records the state.
    fn study(&mut self) -> Result<(), ValueError> {
        if !self.compiled {
            self.compile()?;
        }
        self.studied = true;
        Ok(())
    }

    /// The one search both `Match` and `MatchAgain` run: from `from` within
    /// the search window `[0, stop)`, honoring `preAnchored` and
    /// `preNotEmpty`, and recording the offsets.
    fn search(&mut self, from: usize) -> Result<bool, ValueError> {
        if self.expression.is_none() && !self.compiled {
            self.compile()?;
        }
        let expression = self.expression.clone().expect("compiled");
        self.stored_groups = None;
        if self.subject.is_empty() || from >= self.stop {
            self.found = false;
            return Ok(false);
        }
        let hay_end = char_boundary(&self.subject, self.stop);
        // The subject is copied so the captures below do not hold `self`
        // borrowed while `record` writes the offsets.
        let subject = self.subject.clone();
        let hay = &subject[..hay_end];
        let anchored = self.options & (1 << OPTION_ANCHORED) != 0;
        let not_empty = self.state & (1 << STATE_NOT_EMPTY) != 0;
        let mut position = from;
        loop {
            let Some(captures) = expression.captures_at(hay, position) else {
                self.found = false;
                return Ok(false);
            };
            let whole = captures.get(0).expect("group 0");
            let empty = whole.start() == whole.end();
            let is_anchored = whole.start() == from;
            if (!anchored || is_anchored) && (!not_empty || !empty) {
                self.record(&captures);
                return Ok(true);
            }
            if whole.start() >= hay.len() {
                self.found = false;
                return Ok(false);
            }
            // Advance one character and search again. ADVANCE-BY-CHAR: the
            // next candidate boundary.
            let next = whole.start() + hay[whole.start()..].chars().next().map_or(1, char::len_utf8);
            if next <= position {
                position += 1;
            } else {
                position = next;
            }
            if position > hay.len() {
                self.found = false;
                return Ok(false);
            }
        }
    }

    /// Records a match's offsets: the character offsets of every group,
    /// with PCRE's "highest participating group" count.
    fn record(&mut self, captures: &Captures) {
        let mut offsets: Vec<Option<(usize, usize)>> = Vec::new();
        let mut highest = 0usize;
        for index in 0..captures.len() {
            match captures.get(index) {
                Some(found) => {
                    let start = char_index(&self.subject, found.start());
                    let end = char_index(&self.subject, found.end());
                    offsets.push(Some((start, end)));
                    highest = index;
                }
                None => offsets.push(None),
            }
        }
        // pcre_exec returns one more than the highest group that
        // participated; the port trims the vector to it.
        offsets.truncate(highest + 1);
        self.offsets = offsets;
        self.found = true;
        if let Some(Some((_, end))) = self.offsets.first().copied() {
            let whole = self.offsets[0].expect("group 0");
            self.start = end;
            if whole.0 == whole.1 {
                // "Make sure we don't get stuck at the same position."
                self.start += 1;
            }
        }
    }

    /// `Match`: the search from the beginning of the subject (`startoffset
    /// 0`).
    fn match_(&mut self) -> Result<bool, ValueError> {
        self.search(0)
    }

    /// `MatchAgain`: from `FStart`; the empty subject and a start at or
    /// past the stop fail without a search.
    fn match_again(&mut self) -> Result<bool, ValueError> {
        if self.subject.is_empty() || self.start >= self.stop {
            self.found = false;
            return Ok(false);
        }
        self.search(self.start)
    }

    /// `GetGroups(Index)`: empty for an index past the group count, a
    /// stored copy after `StoreGroups`, else the subject's slice.
    fn groups(&self, index: i64) -> Result<String, ValueError> {
        if !self.found_match() {
            return Err(match_required());
        }
        if index > self.group_count() {
            return Ok(String::new());
        }
        if let Some(stored) = &self.stored_groups {
            return Ok(stored.get(index as usize).cloned().unwrap_or_default());
        }
        match self.offsets.get(index as usize).copied().flatten() {
            Some((start, end)) => Ok(self.slice_chars(start, end)),
            None => Ok(String::new()),
        }
    }

    /// `GetGroupOffsets(Index)`: a 1-based character offset; a group that
    /// did not participate reads 1 (`UTF8IndexToUnicode(..., -1) + 1`).
    fn group_offsets(&self, index: i64) -> Result<i64, ValueError> {
        if !self.found_match() {
            return Err(match_required());
        }
        if index < 0 || index > self.group_count() {
            return Err(index_out_of_bounds(index));
        }
        Ok(match self.offsets.get(index as usize).copied().flatten() {
            Some((start, _)) => start as i64 + 1,
            None => 1,
        })
    }

    /// `GetGroupLengths(Index)`.
    fn group_lengths(&self, index: i64) -> Result<i64, ValueError> {
        if !self.found_match() {
            return Err(match_required());
        }
        if index < 0 || index > self.group_count() {
            return Err(index_out_of_bounds(index));
        }
        Ok(match self.offsets.get(index as usize).copied().flatten() {
            Some((start, end)) => (end - start) as i64,
            None => 0,
        })
    }

    fn slice_chars(&self, start: usize, end: usize) -> String {
        self.subject
            .chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect()
    }

    /// `SubjectLeft`/`SubjectRight`: the parts around the last match. The
    /// RTL does not check `FoundMatch` here, so after a failed search the
    /// stale offsets of the previous match are still used (and a zero-initial
    /// offset with no match at all gives the whole subject on the right).
    fn subject_left(&self) -> String {
        let end = self
            .offsets
            .first()
            .and_then(|group| group.map(|(start, _)| start))
            .unwrap_or(0);
        self.slice_chars(0, end)
    }

    fn subject_right(&self) -> String {
        let start = self
            .offsets
            .first()
            .and_then(|group| group.map(|(_, end)| end))
            .unwrap_or(0);
        self.slice_chars(start, usize::MAX)
    }

    /// `NamedGroup`: the index of a named group, -1 when the name is not
    /// there (`pcre_get_stringnumber`).
    fn named_group(&self, name: &str) -> i64 {
        let Some(expression) = &self.expression else {
            return -1;
        };
        expression
            .capture_names()
            .position(|candidate| candidate == Some(name))
            .map_or(-1, |index| index as i64)
    }

    /// `StoreGroups`: a copy of every group, kept across subject changes.
    fn store_groups(&mut self) -> Result<(), ValueError> {
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

    /// `ComputeReplacement`: `Replacement` with the backreferences
    /// substituted for the current match. The JGsoft flavor of the RTL:
    /// `\n`/`$n` (one or two digits, the second only when the group
    /// exists), `${n}`, `${name}`, `\g<name>`, `$&`/`\&`, `$+`/`\+`,
    /// `` $` ``, `$'`, `$_`, `\\`, `\$`, and the case modes `\l \L \u \U \f
    /// \F \i \I`.
    fn compute_replacement(&self) -> Result<String, ValueError> {
        if self.replacement.is_empty() {
            return Ok(String::new());
        }
        if !self.found_match() {
            return Err(match_required());
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
                            handled = true;
                        } else {
                            out.extend(&chars[index..index + 2]);
                            index += 2;
                            handled = true;
                        }
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
                            number = number.saturating_mul(10) + i64::from(chars[cursor] as u32 - '0' as u32);
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

    /// `Replace`: returns the replacement text, substitutes it into the
    /// subject for the current match and moves the search position.
    fn replace(&mut self) -> Result<String, ValueError> {
        if !self.found_match() {
            return Err(match_required());
        }
        let replacement = self.compute_replacement()?;
        let (start, end) = self.offsets[0].expect("group 0");
        let length = end - start;
        let replacement_chars = replacement.chars().count();
        let mut subject = self.slice_chars(0, start);
        subject.push_str(&replacement);
        subject.push_str(&self.slice_chars(end, usize::MAX));
        self.subject = subject;
        // `FStart := FStart - MatchedLength + Length(Tmp); FStop := ...`.
        self.start = self.start.saturating_sub(length) + replacement_chars;
        self.stop = self.stop.saturating_sub(length) + replacement_chars;
        self.stored_groups = None;
        self.found = false;
        Ok(replacement)
    }

    /// `ReplaceAll`: `Match`, then `Replace`/`MatchAgain` until they drop.
    fn replace_all(&mut self) -> Result<bool, ValueError> {
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

    /// `Split(Strings, Limit)`: the subject's non-matching pieces, the
    /// capturing groups ignored.
    fn split(&mut self, list: &mut crate::script_strings::StringListObject, limit: i32) -> Result<(), ValueError> {
        if limit == 1 || !self.match_()? {
            return list.append(self.subject.clone());
        }
        let mut offset = 0usize;
        let mut count = 1;
        loop {
            let (start, _) = self.offsets[0].expect("group 0");
            list.append(self.slice_chars(offset, start))?;
            count += 1;
            let (_, end) = self.offsets[0].expect("group 0");
            offset = end;
            if (limit > 1 && count >= limit) || !self.match_again()? {
                break;
            }
        }
        list.append(self.slice_chars(offset, usize::MAX))
    }

    /// `SplitCapture(Strings, Limit, Offset)`: the pieces plus the last
    /// capturing group of every match.
    fn split_capture(
        &mut self,
        list: &mut crate::script_strings::StringListObject,
        mut limit: i32,
        offset: i32,
    ) -> Result<(), ValueError> {
        if limit == 1 || !self.match_()? {
            return list.append(self.subject.clone());
        }
        let mut use_offset = offset != 1;
        if offset != 1 {
            limit -= 1;
        }
        let offset = offset.max(1) as usize - 1;
        let mut position = 0usize;
        let mut count = 1;
        loop {
            let (start, end) = self.offsets[0].expect("group 0");
            if use_offset {
                if start >= offset {
                    use_offset = false;
                    list.append(self.slice_chars(0, start))?;
                    if self.group_count() > 0 {
                        list.append(self.groups(self.group_count())?)?;
                    }
                }
            } else {
                list.append(self.slice_chars(position, start))?;
                count += 1;
                if self.group_count() > 0 {
                    list.append(self.groups(self.group_count())?)?;
                }
            }
            position = end;
            if (limit > 1 && count >= limit) || !self.match_again()? {
                break;
            }
        }
        list.append(self.slice_chars(position, usize::MAX))
    }

    /// `EscapeRegExChars`: the RTL's 2015 list of characters that get a
    /// backslash, `#0` becoming `\0`.
    fn escape_regex_chars(text: &str) -> String {
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

/// The byte offset of the character boundary at `index` (a character
/// index), clamped to the string.
fn char_boundary(text: &str, index: usize) -> usize {
    text.char_indices().nth(index).map_or(text.len(), |(at, _)| at)
}

/// The character index of a byte offset.
fn char_index(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

impl ScriptObject for PerlRegExObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The `TPerlRegEx` of an `Args.Obj` receiver.
fn regex_of(value: &Value) -> Option<Rc<std::cell::RefCell<dyn ScriptObject>>> {
    let object = value.as_object()?;
    if object.borrow().as_any().is::<PerlRegExObject>() {
        Some(object.clone())
    } else {
        None
    }
}

fn regex_obj(value: &Value) -> Result<Rc<std::cell::RefCell<dyn ScriptObject>>, ValueError> {
    regex_of(value).ok_or_else(|| {
        ValueError::external_class("EInvalidCast", "Invalid cast: the value is not a TPerlRegEx".to_owned())
    })
}

fn borrow_regex(object: &Rc<std::cell::RefCell<dyn ScriptObject>>) -> std::cell::Ref<'_, PerlRegExObject> {
    std::cell::Ref::map(object.borrow(), |object| {
        object
            .as_any()
            .downcast_ref::<PerlRegExObject>()
            .expect("a TPerlRegEx object")
    })
}

fn borrow_regex_mut(object: &Rc<std::cell::RefCell<dyn ScriptObject>>) -> std::cell::RefMut<'_, PerlRegExObject> {
    std::cell::RefMut::map(object.borrow_mut(), |object| {
        object
            .as_any_mut()
            .downcast_mut::<PerlRegExObject>()
            .expect("a TPerlRegEx object")
    })
}

fn receiver_regex(args: &Args) -> Result<Rc<std::cell::RefCell<dyn ScriptObject>>, ValueError> {
    match &args.obj {
        Some(object) => regex_obj(object),
        None => Err(ValueError::external_class(
            "EInvalidCast",
            "Invalid cast: the value is not a TPerlRegEx".to_owned(),
        )),
    }
}

fn value_param() -> ParamType {
    ParamType::value(VarType::Empty)
}

/// Registers `xejviScriptAdapterMiscRegEx.pas`.
pub fn register(adapter: &mut Adapter, classes: &ScriptClasses) {
    //=== The constants =======================================================

    for (name, value) in [
        ("preCaseLess", OPTION_CASE_LESS),
        ("preMultiLine", OPTION_MULTI_LINE),
        ("preSingleLine", OPTION_SINGLE_LINE),
        ("preExtended", OPTION_EXTENDED),
        ("preAnchored", OPTION_ANCHORED),
        ("preUnGreedy", OPTION_UN_GREEDY),
        ("preNoAutoCapture", OPTION_NO_AUTO_CAPTURE),
        ("preNotBOL", STATE_NOT_BOL),
        ("preNotEOL", STATE_NOT_EOL),
        ("preNotEmpty", STATE_NOT_EMPTY),
    ] {
        adapter.add_const("RegularExpressionsCore", name, Value::Integer(value));
    }

    let class = classes.t_perl_regex.clone();

    // `TPerlRegEx_Create`: called on the class value as upstream.
    {
        let class = class.clone();
        adapter.add_get(
            classes.t_perl_regex.clone(),
            "Create",
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                *value = Value::Object(Rc::new(std::cell::RefCell::new(PerlRegExObject::new(class.clone()))));
                Ok(())
            }),
            0,
            Vec::new(),
            VarType::Empty,
        );
    }

    adapter.add_get(
        class.clone(),
        "Compile",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).compile()
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "Compiled",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Bool(borrow_regex(&object).compiled);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "Study",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).study()
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "Studied",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Bool(borrow_regex(&object).studied);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "Match",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            let matched = borrow_regex_mut(&object).match_()?;
            *value = Value::Bool(matched);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "MatchAgain",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            let matched = borrow_regex_mut(&object).match_again()?;
            *value = Value::Bool(matched);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "FoundMatch",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Bool(borrow_regex(&object).found_match());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "GroupCount",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            let regex = borrow_regex(&object);
            if !regex.found_match() {
                return Err(match_required());
            }
            *value = Value::Integer(regex.group_count() as i32);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_iget(
        class.clone(),
        "Groups",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let index = xedit_script::values::to_i32(&args.values[0])?;
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).groups(i64::from(index))?.into_bytes());
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_iget(
        class.clone(),
        "GroupOffsets",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let index = xedit_script::values::to_i32(&args.values[0])?;
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).group_offsets(i64::from(index))? as i32);
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_iget(
        class.clone(),
        "GroupLengths",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let index = xedit_script::values::to_i32(&args.values[0])?;
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).group_lengths(i64::from(index))? as i32);
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "MatchedText",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).groups(0)?.into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "MatchedOffset",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).group_offsets(0)? as i32);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "MatchedLength",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).group_lengths(0)? as i32);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "NamedGroup",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let name = text_arg(&args.values[0]);
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).named_group(&name) as i32);
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "SubjectLeft",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).subject_left().into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "SubjectRight",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).subject_right().into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "EscapeRegExChars",
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Str(PerlRegExObject::escape_regex_chars(&text_arg(&args.values[0])).into_bytes());
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "StoreGroups",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).store_groups()
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "ComputeReplacement",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).compute_replacement()?.into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "Replace",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex_mut(&object).replace()?.into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "ReplaceAll",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Bool(borrow_regex_mut(&object).replace_all()?);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );

    // `Split(Strings, Limit)`/`SplitCapture(Strings, Limit)` -- the
    // registration allows the 1-argument form PCRE-style call shape
    // (`Args.Count` 2), so `PR_ARGS_NO_CHECK` is not used: the port follows
    // the upstream `AddGet(..., 2, [varEmpty, varEmpty])`.
    adapter.add_get(
        class.clone(),
        "Split",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let list = list_of_or_error(&args.values[0])?;
            let limit = xedit_script::values::to_i32(&args.values[1])?;
            let object = receiver_regex(args)?;
            let mut regex = borrow_regex_mut(&object);
            let mut list = borrow_list_mut(&list);
            regex.split(&mut list, limit)
        }),
        2,
        vec![value_param(), value_param()],
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "SplitCapture",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let list = list_of_or_error(&args.values[0])?;
            let limit = xedit_script::values::to_i32(&args.values[1])?;
            let object = receiver_regex(args)?;
            let mut regex = borrow_regex_mut(&object);
            let mut list = borrow_list_mut(&list);
            regex.split_capture(&mut list, limit, 1)
        }),
        2,
        vec![value_param(), value_param()],
        VarType::Empty,
    );

    // The properties.
    adapter.add_get(
        class.clone(),
        "RegEx",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).regex.clone().into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "RegEx",
        Rc::new(|value: &Value, args: &mut Args| {
            let text = text_arg(value);
            let object = receiver_regex(args)?;
            let mut regex = borrow_regex_mut(&object);
            if regex.regex != text {
                regex.regex = text;
                regex.clean_up();
            }
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_get(
        class.clone(),
        "Replacement",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).replacement.clone().into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "Replacement",
        Rc::new(|value: &Value, args: &mut Args| {
            let text = text_arg(value);
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).replacement = text;
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_get(
        class.clone(),
        "Subject",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).subject.clone().into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "Subject",
        Rc::new(|value: &Value, args: &mut Args| {
            let text = text_arg(value);
            let object = receiver_regex(args)?;
            let mut regex = borrow_regex_mut(&object);
            // `if (Length(FSubject) = 0) or (Value <> Subject)`.
            if regex.subject.is_empty() || regex.subject != text {
                let length = text.chars().count();
                regex.subject = text;
                regex.start = 0;
                regex.stop = length;
                if regex.stored_groups.is_none() {
                    regex.found = false;
                }
            }
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_get(
        class.clone(),
        "Start",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).start as i32 + 1);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "Start",
        Rc::new(|value: &Value, args: &mut Args| {
            let start = xedit_script::values::to_i32(value)?;
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).start = if start < 1 { 0 } else { start as usize - 1 };
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_get(
        class.clone(),
        "Stop",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).stop as i32);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "Stop",
        Rc::new(|value: &Value, args: &mut Args| {
            let stop = xedit_script::values::to_i32(value)?;
            let object = receiver_regex(args)?;
            let mut regex = borrow_regex_mut(&object);
            let length = regex.subject.chars().count() as i32;
            regex.stop = if stop > length {
                length.max(0) as usize
            } else {
                stop.max(0) as usize
            };
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_get(
        class.clone(),
        "Options",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            // `S2V(Byte(Options))`.
            *value = Value::Integer(borrow_regex(&object).options & 0xFF);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "Options",
        Rc::new(|value: &Value, args: &mut Args| {
            // `Options := TPerlRegExOptions(Byte(V2S(Value)))`.
            let options = xedit_script::values::to_i32(value)? & 0xFF;
            let object = receiver_regex(args)?;
            let mut regex = borrow_regex_mut(&object);
            if regex.options != options {
                regex.options = options;
                regex.clean_up();
            }
            Ok(())
        }),
        0,
        vec![value_param()],
    );

    let _ = PR_ARGS_NO_CHECK;
    let _ = receiver;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex(pattern: &str) -> PerlRegExObject {
        let mut object = PerlRegExObject::new(Rc::new(ClassDef {
            unit: String::new(),
            name: "TPerlRegEx".to_owned(),
            parent: None,
        }));
        object.regex = pattern.to_owned();
        object
    }

    #[test]
    fn matches_with_offsets_and_groups() {
        let mut object = regex("(b)(c)?");
        object.subject = "abd".to_owned();
        object.stop = 3;
        assert!(object.match_().unwrap());
        assert_eq!(object.group_count(), 1, "only group 1 participated");
        assert_eq!(object.groups(0).unwrap(), "b");
        assert_eq!(object.groups(1).unwrap(), "b");
        assert_eq!(object.groups(2).unwrap(), "", "past the group count");
        assert_eq!(object.group_offsets(0).unwrap(), 2, "1-based character offset");
        assert_eq!(object.group_lengths(0).unwrap(), 1);
        assert!(!object.match_again().unwrap());
        assert_eq!(object.subject_left(), "a");
        assert_eq!(object.subject_right(), "d");
    }

    #[test]
    fn match_again_walks_the_subject() {
        let mut object = regex("\\d+");
        object.subject = "a12 b345".to_owned();
        object.stop = object.subject.chars().count();
        let mut found = Vec::new();
        while object.match_again().unwrap() {
            found.push(object.groups(0).unwrap());
        }
        assert_eq!(found, ["12", "345"]);
    }

    #[test]
    fn replace_all_substitutes_every_match() {
        let mut object = regex("a");
        object.subject = "banana".to_owned();
        object.stop = 6;
        object.replacement = "o".to_owned();
        assert!(object.replace_all().unwrap());
        assert_eq!(object.subject, "bonono");
        assert!(!object.found_match());
    }

    #[test]
    fn replacement_backreferences() {
        let mut object = regex("(\\w+)=(\\w+)");
        object.subject = "key=value".to_owned();
        object.stop = 9;
        assert!(object.match_().unwrap());
        object.replacement = "$2:$1".to_owned();
        assert_eq!(object.compute_replacement().unwrap(), "value:key");
        object.replacement = "${1}\\U2".to_owned();
        assert_eq!(object.compute_replacement().unwrap(), "keyVALUE");
        object.replacement = "$0 $& $+".to_owned();
        assert_eq!(object.compute_replacement().unwrap(), "key=value key=value value");
    }

    #[test]
    fn split_appends_pieces() {
        let class = Rc::new(ClassDef {
            unit: String::new(),
            name: "TStringList".to_owned(),
            parent: None,
        });
        let mut object = regex(",");
        object.subject = "a,b,,c".to_owned();
        object.stop = 6;
        let mut list = crate::script_strings::StringListObject::new(class);
        object.split(&mut list, 0).unwrap();
        assert_eq!(list.string_at(0), "a");
        assert_eq!(list.string_at(1), "b");
        assert_eq!(list.string_at(2), "");
        assert_eq!(list.string_at(3), "c");
    }

    #[test]
    fn escape_and_named_groups() {
        assert_eq!(PerlRegExObject::escape_regex_chars("a.b(c)"), "a\\.b\\(c\\)");
        let mut object = regex("(?P<first>a)(b)");
        object.subject = "ab".to_owned();
        object.stop = 2;
        assert!(object.match_().unwrap());
        assert_eq!(object.named_group("first"), 1);
        assert_eq!(object.named_group("nope"), -1);
    }

    #[test]
    fn anchored_and_not_empty_options() {
        let mut object = regex("a");
        object.options = 1 << OPTION_ANCHORED;
        object.subject = "ba".to_owned();
        object.stop = 2;
        assert!(!object.match_().unwrap(), "an anchored match must start at 0");
        // Without the option the same pattern matches at 1.
        object.options = 0;
        assert!(object.match_().unwrap());
        assert_eq!(object.group_offsets(0).unwrap(), 2);
        // `preNotEmpty` skips the empty matches `a*` finds.
        object.regex = "a*".to_owned();
        object.clean_up();
        object.subject = "ba".to_owned();
        object.stop = 2;
        object.state = 1 << STATE_NOT_EMPTY;
        assert!(object.match_().unwrap());
        assert_eq!(object.groups(0).unwrap(), "a");
    }
}
