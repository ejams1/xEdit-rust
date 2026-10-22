// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The regular expression engine behind `TPerlRegEx`
//! (`System.RegularExpressionsCore` of the Delphi run-time library, which
//! upstream runs through the script adapter, Sniff's processors and the
//! filter's regex comparison).
//!
//! Delphi's `TPerlRegEx` compiles the pattern and matches it with **PCRE 8**
//! (the C library the run-time links): the pattern is a Perl-compatible one
//! with look-around, backreferences and possessive quantifiers, the subject
//! is passed as UTF-8 (`PCRE_UTF8`) and the group delimiters are UTF-8 byte
//! offsets. The Rust `regex` crate this port used before has none of those:
//! it refuses look-around, in-pattern backreferences and possessive
//! quantifiers, matches `\d`/`\w`/`\s` over Unicode rather than ASCII and
//! treats `$` differently before a trailing newline. This module is the
//! engine that replaces it; the behaviour it reproduces was read off the
//! 4.1.5q release build with the probe of `cargo xtask parity script
//! --script "Regex probe"` (`crates/xtask/oracle/scripts/probes`), which is
//! also the check.
//!
//! What it implements:
//!
//! - literals, `.`, character classes (ranges, negation, POSIX classes,
//!   `\d`/`\w`/`\s`/`\h`/`\v`/`\R`, `\p{...}` properties, ASCII semantics
//!   without `PCRE_UCP`), `\Q...\E`, `\x`/`\o`/`\c`/octal escapes;
//! - anchors `^` `$` `\A` `\Z` `\z` `\b` `\B` `\G`, with PCRE's `$` before
//!   a trailing newline and its multi-line/inline-flag rules;
//! - alternation, capturing, non-capturing, named (`(?<n>`, `(?'n'`,
//!   `(?P<n>`), atomic (`(?>`) groups, conditionals `(?(1)a|b)`, comments
//!   `(?#..)`, callouts `(?C..)` (no-ops) and the inline flags
//!   `(?imsxUn)` / `(?imsx-Un:...)`;
//! - greedy, lazy and possessive quantifiers, bounded `{n,m}`;
//! - look-ahead and look-behind (including alternatives of different fixed
//!   lengths, and PCRE's refusal of variable-length ones);
//! - backreferences `\1`, `\g1`, `\g{-1}`, `\k<name>`, `(?P=name)`.
//!
//! What it does not implement, and how it answers for it: the pattern
//! constructs of PCRE that neither upstream's scripts nor its processors can
//! reach (recursion `(?R)`/`(?1)`, `\N{name}`, `(*VERB)` control verbs,
//! `\K`, subroutine calls) are compile errors with PCRE's own text where it
//! has one. `\p{...}` covers the properties a script is likely to use
//! (letters, numbers, upper/lower case, whitespace, word) through Rust's
//! character classification, which is not always the same Unicode version as
//! the release's tables.
//!
//! UTF-8: the pattern and the subject are characters; a match never splits a
//! character, and the offsets this module reports are **byte** offsets into
//! the subject, as PCRE reports them (`perl_regex.rs` converts them for the
//! `TPerlRegEx` properties).

use std::fmt;

/// The compile flags of `TPerlRegExOptions`: `preCaseLess` is bit 0 and
/// `preNoAutoCapture` bit 6, as the adapter registers them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    pub caseless: bool,
    pub multi_line: bool,
    pub single_line: bool,
    pub extended: bool,
    pub anchored: bool,
    pub ungreedy: bool,
    pub no_auto_capture: bool,
}

impl Flags {
    /// `preCaseLess .. preNoAutoCapture`, the bit values the adapter's
    /// constants use.
    pub fn from_options(options: i32) -> Flags {
        Flags {
            caseless: options & 1 != 0,
            multi_line: options & 2 != 0,
            single_line: options & 4 != 0,
            extended: options & 8 != 0,
            anchored: options & 16 != 0,
            ungreedy: options & 32 != 0,
            no_auto_capture: options & 64 != 0,
        }
    }
}

/// `TPerlRegExState`: `preNotBOL`, `preNotEOL` and `preNotEmpty`, the
/// `pcre_exec` flags of the RTL. The `State` property is not registered for
/// scripts, so no script can set them; the class keeps them because Sniff
/// and the filter drive the same type.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExecFlags {
    /// `PCRE_NOTBOL`: `^` does not match at the start of the subject.
    pub not_bol: bool,
    /// `PCRE_NOTEOL`: `$` and `\Z` do not match at the end of the subject.
    pub not_eol: bool,
    /// `PCRE_NOTEMPTY`: an empty match fails.
    pub not_empty: bool,
    /// `PCRE_ANCHORED`: the match has to start where the search starts.
    pub anchored: bool,
}

impl ExecFlags {
    pub fn from_state(state: i32) -> ExecFlags {
        ExecFlags {
            not_bol: state & 1 != 0,
            not_eol: state & 2 != 0,
            not_empty: state & 4 != 0,
            anchored: false,
        }
    }
}

/// A pattern PCRE refuses: its message and the offset PCRE reports, both of
/// which the RTL turns into `Error in regular expression at offset N: text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileError {
    /// `pcre_compile`'s `erroroffset`.
    pub offset: usize,
    /// `pcre_error_text`'s text for the error code.
    pub message: String,
}

impl CompileError {
    fn new(offset: usize, message: &str) -> CompileError {
        CompileError {
            offset,
            message: message.to_owned(),
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at offset {}", self.message, self.offset)
    }
}

/// The groups of a match: `None` for a group that did not participate, byte
/// offsets into the subject otherwise. Index 0 is the whole match.
pub type Captures = Vec<Option<(usize, usize)>>;

// ---------------------------------------------------------------------------
// Character sets
// ---------------------------------------------------------------------------

/// One item of a character class.
#[derive(Clone, Debug)]
enum ClassItem {
    Char(u32),
    Range(u32, u32),
    /// `\d`, `\D`, `\w`, `\W`, `\s`, `\S`, `\h`, `\H`, `\v`, `\V`.
    Shorthand(Shorthand),
    /// `[[:alpha:]]` and its negation.
    Posix(Posix, bool),
    /// `\p{...}` and `\P{...}`.
    Property(Property, bool),
    /// A POSIX class written inside a class as a shorthand `\b` (backspace)
    /// is handled as [`ClassItem::Char`].
    Bs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shorthand {
    Digit,
    NotDigit,
    Word,
    NotWord,
    Space,
    NotSpace,
    Horiz,
    NotHoriz,
    Vert,
    NotVert,
    /// `\R`: any newline sequence.
    Newline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Posix {
    Alnum,
    Alpha,
    Ascii,
    Blank,
    Cntrl,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    Word,
    Xdigit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Property {
    Letter,
    Lower,
    Upper,
    Number,
    Decimal,
    Alnum,
    Word,
    Space,
    /// Any character with a value above ASCII (PCRE's `\p{^..}` and the
    /// high half of the tables).
    XpsxDigit,
    Any,
}

/// A compiled character class: the items of `[...]`, or the single item of a
/// shorthand such as `\d`.
#[derive(Clone, Debug)]
struct ClassSet {
    negated: bool,
    items: Vec<ClassItem>,
}

impl ClassSet {
    /// Whether `c` is in the set, with `caseless` folding.
    fn matches(&self, c: u32, caseless: bool) -> bool {
        let mut found = self.contains(c);
        if !found && caseless {
            // PCRE's default tables fold ASCII and Latin-1 only.
            found = self.contains(fold_case(c));
        }
        found != self.negated
    }

    fn contains(&self, c: u32) -> bool {
        self.items.iter().any(|item| item_matches(item, c))
    }
}

fn item_matches(item: &ClassItem, c: u32) -> bool {
    match item {
        ClassItem::Char(ch) => *ch == c,
        ClassItem::Range(lo, hi) => *lo <= c && c <= *hi,
        ClassItem::Shorthand(sh) => shorthand_matches(*sh, c),
        ClassItem::Posix(posix, negated) => posix_matches(*posix, c) != *negated,
        ClassItem::Property(property, negated) => property_matches(*property, c) != *negated,
        ClassItem::Bs => c == 0x08,
    }
}

/// PCRE's `\d` and friends are ASCII outside `PCRE_UCP` (`\h` and `\v`
/// include their Unicode members in UTF-8 mode).
fn shorthand_matches(sh: Shorthand, c: u32) -> bool {
    match sh {
        Shorthand::Digit => is_digit(c),
        Shorthand::NotDigit => !is_digit(c),
        Shorthand::Word => is_word(c),
        Shorthand::NotWord => !is_word(c),
        Shorthand::Space => is_space(c),
        Shorthand::NotSpace => !is_space(c),
        Shorthand::Horiz => is_horiz(c),
        Shorthand::NotHoriz => !is_horiz(c),
        Shorthand::Vert => is_vert(c),
        Shorthand::NotVert => !is_vert(c),
        Shorthand::Newline => is_newline(c),
    }
}

fn is_digit(c: u32) -> bool {
    (0x30..=0x39).contains(&c)
}

fn is_alpha(c: u32) -> bool {
    (0x41..=0x5A).contains(&c) || (0x61..=0x7A).contains(&c)
}

fn is_hex_digit(c: u32) -> bool {
    is_digit(c) || (0x41..=0x46).contains(&c) || (0x61..=0x66).contains(&c)
}

fn is_word(c: u32) -> bool {
    is_alpha(c) || is_digit(c) || c == u32::from(b'_')
}

/// PCRE's `\s`: HT, LF, FF, CR and space (no vertical tab).
fn is_space(c: u32) -> bool {
    matches!(c, 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

fn is_horiz(c: u32) -> bool {
    matches!(
        c,
        0x09 | 0x20 | 0xA0 | 0x1680 | 0x180E | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000
    )
}

fn is_vert(c: u32) -> bool {
    matches!(c, 0x0A | 0x0B | 0x0C | 0x0D | 0x85 | 0x2028 | 0x2029)
}

fn is_newline(c: u32) -> bool {
    matches!(c, 0x0A | 0x0B | 0x0C | 0x0D | 0x85 | 0x2028 | 0x2029)
}

fn posix_matches(posix: Posix, c: u32) -> bool {
    match posix {
        Posix::Alnum => is_alpha(c) || is_digit(c),
        Posix::Alpha => is_alpha(c),
        Posix::Ascii => c < 0x80,
        Posix::Blank => c == 0x20 || c == 0x09,
        Posix::Cntrl => c < 0x20 || c == 0x7F,
        Posix::Digit => is_digit(c),
        Posix::Graph => (0x21..=0x7E).contains(&c),
        Posix::Lower => (0x61..=0x7A).contains(&c),
        Posix::Print => (0x20..=0x7E).contains(&c),
        Posix::Punct => (0x21..=0x7E).contains(&c) && !is_alpha(c) && !is_digit(c),
        Posix::Space => is_space(c) || c == 0x0B,
        Posix::Upper => (0x41..=0x5A).contains(&c),
        Posix::Word => is_word(c),
        Posix::Xdigit => is_hex_digit(c),
    }
}

fn property_matches(property: Property, c: u32) -> bool {
    match property {
        Property::Letter => char::from_u32(c).is_some_and(char::is_alphabetic),
        Property::Lower => char::from_u32(c).is_some_and(char::is_lowercase),
        Property::Upper => char::from_u32(c).is_some_and(char::is_uppercase),
        Property::Number => char::from_u32(c).is_some_and(char::is_numeric),
        Property::Decimal => is_digit(c),
        Property::Alnum => char::from_u32(c).is_some_and(|ch| ch.is_alphanumeric()),
        Property::Word => char::from_u32(c).is_some_and(|ch| ch.is_alphanumeric()) || c == u32::from(b'_'),
        Property::Space => is_space(c) || c == 0x0B || c == 0x85 || (0x2000..=0x200A).contains(&c),
        Property::XpsxDigit => c >= 0x80,
        Property::Any => true,
    }
}

/// PCRE's case folding without `PCRE_UCP`: ASCII and Latin-1.
fn fold_case(c: u32) -> u32 {
    match c {
        0x41..=0x5A => c + 0x20,
        0x61..=0x7A => c - 0x20,
        0xC0..=0xDE if c != 0xD7 => c + 0x20,
        0xE0..=0xFE if c != 0xF7 => c - 0x20,
        _ => c,
    }
}

/// Comparand equality with PCRE's folding rules.
fn chars_equal(a: u32, b: u32, caseless: bool) -> bool {
    a == b || (caseless && (fold_case(a) == b || a == fold_case(b)))
}

// ---------------------------------------------------------------------------
// The parsed pattern
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Greed {
    Greedy,
    Lazy,
    Possessive,
}

#[derive(Clone, Debug)]
enum Node {
    Empty,
    Char(u32, bool),
    Class(usize, bool),
    Any(bool),
    Anchor(Anchor),
    Backref(BackrefRef, bool),
    Group {
        index: Option<u32>,
        body: Box<Node>,
        atomic: bool,
    },
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Repeat {
        body: Box<Node>,
        min: u32,
        max: Option<u32>,
        greed: Greed,
    },
    Assert {
        behind: bool,
        negative: bool,
        body: Box<Node>,
    },
    /// `(?(1)yes|no)`: `yes` when the group participated.
    Conditional {
        group: u32,
        yes: Box<Node>,
        no: Box<Node>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    /// `^` with `multi_line`.
    Bol(bool),
    Eol(bool),
    /// `\A`
    Start,
    /// `\z`
    End,
    /// `\Z`
    EndZ,
    /// `\b` (true) and `\B` (false)
    WordBoundary(bool),
    /// `\G`: the match has to start where the search started.
    StartOfMatch,
    /// A zero-width pattern that always matches (`(?#..)`, callouts).
    Nothing,
}

#[derive(Clone, Debug)]
enum BackrefRef {
    Number(u32),
    Name(String),
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// A quantifier: `min`, `max` (`None` for `*` and `+`), how greedy it is and
/// the index of its last character in the pattern.
type Quantifier = (u32, Option<u32>, Greed, usize);

struct Parser<'a> {
    pattern: &'a [u8],
    at: usize,
    flags: Flags,
    /// The classes built so far, in the order the compiler numbers them.
    classes: Vec<ClassSet>,
    group_count: u32,
    names: Vec<(String, u32)>,
    /// The group the parser is inside (for `\g{-1}`).
    current_group: u32,
    /// The backreferences seen, checked once the whole pattern is parsed.
    references: Vec<(BackrefRef, usize)>,
}

type PResult<T> = Result<T, CompileError>;

impl<'a> Parser<'a> {
    fn new(pattern: &'a str, flags: Flags) -> Parser<'a> {
        Parser {
            pattern: pattern.as_bytes(),
            at: 0,
            flags,
            classes: Vec::new(),
            group_count: 0,
            names: Vec::new(),
            current_group: 0,
            references: Vec::new(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.pattern.get(self.at).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.pattern.get(self.at + offset).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek();
        if byte.is_some() {
            self.at += 1;
        }
        byte
    }

    fn eof(&self) -> bool {
        self.at >= self.pattern.len()
    }

    /// The character at the cursor, decoded as UTF-8, and its length.
    fn next_char(&mut self) -> Option<(u32, usize)> {
        let rest = &self.pattern[self.at..];
        let text = std::str::from_utf8(rest).ok()?;
        let ch = text.chars().next()?;
        self.at += ch.len_utf8();
        Some((ch as u32, ch.len_utf8()))
    }

    fn parse(mut self) -> PResult<Regex> {
        let node = self.parse_alt(false)?;
        if !self.eof() {
            // A `)` with no `(`: PCRE reports it where it stands.
            if self.peek() == Some(b')') {
                return Err(CompileError::new(self.at, "unmatched parentheses"));
            }
            return Err(CompileError::new(self.at, "unmatched parentheses"));
        }
        for (reference, _offset) in &self.references {
            let group = match reference {
                BackrefRef::Number(number) => *number,
                BackrefRef::Name(name) => self
                    .names
                    .iter()
                    .find(|(candidate, _)| candidate == name)
                    .map_or(0, |(_, number)| *number),
            };
            if group == 0 || group > self.group_count {
                // PCRE checks the references when the pattern has been read
                // and reports the end of the pattern.
                return Err(CompileError::new(
                    self.pattern.len(),
                    "reference to non-existent subpattern",
                ));
            }
        }
        let mut node = node;
        resolve_references(&mut node, &self.names);
        let compiled = Compiler::new().compile(&node);
        Ok(Regex {
            program: compiled.program,
            registers: compiled.registers,
            classes: self.classes,
            names: self.names,
            group_count: self.group_count,
            flags: self.flags,
        })
    }

    /// `alt := concat ('|' concat)*`
    fn parse_alt(&mut self, in_group: bool) -> PResult<Node> {
        let mut branches = Vec::new();
        loop {
            let branch = self.parse_concat(in_group)?;
            branches.push(branch);
            if self.peek() == Some(b'|') {
                self.at += 1;
                continue;
            }
            break;
        }
        if branches.len() == 1 {
            Ok(branches.pop().expect("one branch"))
        } else {
            Ok(Node::Alt(branches))
        }
    }

    /// `concat := repeat*`
    fn parse_concat(&mut self, in_group: bool) -> PResult<Node> {
        let mut items = Vec::new();
        loop {
            if self.eof() {
                break;
            }
            match self.peek() {
                Some(b'|') => break,
                Some(b')') if in_group => break,
                Some(b')') => break,
                _ => {}
            }
            // Extended mode: whitespace and `#` comments are not part of the
            // pattern.
            if self.flags.extended && self.skip_extended() {
                continue;
            }
            let item = self.parse_repeat(in_group)?;
            items.push(item);
        }
        match items.len() {
            0 => Ok(Node::Empty),
            1 => Ok(items.pop().expect("one item")),
            _ => Ok(Node::Concat(items)),
        }
    }

    /// Skips one run of whitespace or a comment in extended mode;
    /// `true` when something was skipped.
    fn skip_extended(&mut self) -> bool {
        let mut skipped = false;
        loop {
            match self.peek() {
                Some(b) if b.is_ascii_whitespace() => {
                    self.at += 1;
                    skipped = true;
                }
                Some(b'#') => {
                    while let Some(byte) = self.peek() {
                        self.at += 1;
                        if byte == b'\n' {
                            break;
                        }
                    }
                    skipped = true;
                }
                _ => return skipped,
            }
        }
    }

    fn parse_repeat(&mut self, in_group: bool) -> PResult<Node> {
        let atom = self.parse_atom(in_group)?;
        let quantifier = self.parse_quantifier()?;
        match quantifier {
            None => Ok(atom),
            Some((min, max, mut greed, end)) => {
                if self.flags.ungreedy {
                    greed = match greed {
                        Greed::Greedy => Greed::Lazy,
                        Greed::Lazy => Greed::Greedy,
                        Greed::Possessive => Greed::Possessive,
                    };
                }
                // A quantifier on nothing repeatable, and a second
                // quantifier, are both `nothing to repeat`.
                match &atom {
                    Node::Anchor(_) | Node::Empty => {
                        return Err(CompileError::new(end, "nothing to repeat"));
                    }
                    _ => {}
                }
                let node = Node::Repeat {
                    body: Box::new(atom),
                    min,
                    max,
                    greed,
                };
                // A second quantifier is refused where it stands.
                if let Some((_, _, _, second_end)) = self.parse_quantifier()? {
                    return Err(CompileError::new(second_end, "nothing to repeat"));
                }
                Ok(node)
            }
        }
    }

    /// A quantifier at the cursor.
    fn parse_quantifier(&mut self) -> PResult<Option<Quantifier>> {
        let Some(byte) = self.peek() else {
            return Ok(None);
        };
        let (min, max, last) = match byte {
            b'*' => {
                self.at += 1;
                (0, None, self.at - 1)
            }
            b'+' => {
                self.at += 1;
                (1, None, self.at - 1)
            }
            b'?' => {
                self.at += 1;
                (0, Some(1), self.at - 1)
            }
            b'{' => {
                let start = self.at;
                match self.parse_braces()? {
                    Some((min, max)) => (min, max, self.at - 1),
                    None => {
                        self.at = start;
                        return Ok(None);
                    }
                }
            }
            _ => return Ok(None),
        };
        let mut greed = Greed::Greedy;
        match self.peek() {
            Some(b'?') => {
                self.at += 1;
                greed = Greed::Lazy;
            }
            Some(b'+') => {
                self.at += 1;
                greed = Greed::Possessive;
            }
            _ => {}
        }
        Ok(Some((min, max, greed, last)))
    }

    /// `{n}`, `{n,}`, `{n,m}`; `None` when the braces are literal text
    /// (`a{`, `a{,2}`, `a{x}` are literal in PCRE 8).
    fn parse_braces(&mut self) -> PResult<Option<(u32, Option<u32>)>> {
        // self.peek() == b'{'
        let start = self.at;
        self.at += 1;
        let mut min: Option<u32> = None;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit() {
                let digit = u32::from(byte - b'0');
                min = Some(min.unwrap_or(0).saturating_mul(10).saturating_add(digit));
                self.at += 1;
            } else {
                break;
            }
        }
        let mut max = min;
        let mut comma = false;
        if self.peek() == Some(b',') {
            comma = true;
            self.at += 1;
            let mut value: Option<u32> = None;
            while let Some(byte) = self.peek() {
                if byte.is_ascii_digit() {
                    let digit = u32::from(byte - b'0');
                    value = Some(value.unwrap_or(0).saturating_mul(10).saturating_add(digit));
                    self.at += 1;
                } else {
                    break;
                }
            }
            max = value;
        }
        if self.peek() != Some(b'}') || (min.is_none() && !comma) {
            // Literal braces: PCRE only treats `{` as a quantifier when it
            // holds a number (and stays a quantifier when it holds a comma
            // and no numbers at all: `{,2}` is literal, `{2,}` is not).
            self.at = start;
            return Ok(None);
        }
        if min.is_none() {
            self.at = start;
            return Ok(None);
        }
        self.at += 1;
        let min = min.expect("a number");
        match max {
            Some(max) if max < min => {
                // PCRE reports the closing brace.
                return Err(CompileError::new(self.at - 1, "numbers out of order in {} quantifier"));
            }
            _ => {}
        }
        Ok(Some((min, max)))
    }

    fn parse_atom(&mut self, in_group: bool) -> PResult<Node> {
        let byte = self.peek().expect("an atom");
        match byte {
            b'(' => self.parse_group(in_group),
            b'[' => self.parse_class(),
            b'.' => {
                self.at += 1;
                Ok(Node::Any(self.flags.single_line))
            }
            b'^' => {
                self.at += 1;
                Ok(Node::Anchor(Anchor::Bol(self.flags.multi_line)))
            }
            b'$' => {
                self.at += 1;
                Ok(Node::Anchor(Anchor::Eol(self.flags.multi_line)))
            }
            b'\\' => self.parse_escape(in_group),
            b'*' | b'+' | b'?' => Err(CompileError::new(self.at, "nothing to repeat")),
            _ => {
                let (ch, _) = self
                    .next_char()
                    .ok_or_else(|| CompileError::new(self.at, "invalid UTF-8 string"))?;
                Ok(Node::Char(ch, self.flags.caseless))
            }
        }
    }

    fn parse_group(&mut self, in_group: bool) -> PResult<Node> {
        let _ = in_group;
        self.at += 1; // '('
        if self.peek() != Some(b'?') {
            let index = if self.flags.no_auto_capture {
                None
            } else {
                self.group_count += 1;
                Some(self.group_count)
            };
            let outer = self.current_group;
            if let Some(index) = index {
                self.current_group = index;
            }
            let body = self.parse_alt(true)?;
            self.current_group = outer;
            if self.peek() != Some(b')') {
                return Err(CompileError::new(self.pattern.len(), "missing )"));
            }
            self.at += 1;
            return Ok(Node::Group {
                index,
                body: Box::new(body),
                atomic: false,
            });
        }
        // `(?...` forms.
        self.at += 1; // '?'
        match self.peek() {
            Some(b':') => {
                self.at += 1;
                let body = self.parse_alt(true)?;
                self.expect_close()?;
                Ok(Node::Group {
                    index: None,
                    body: Box::new(body),
                    atomic: false,
                })
            }
            Some(b'>') => {
                self.at += 1;
                let body = self.parse_alt(true)?;
                self.expect_close()?;
                Ok(Node::Group {
                    index: None,
                    body: Box::new(body),
                    atomic: true,
                })
            }
            Some(b'=') => {
                self.at += 1;
                let body = self.parse_alt(true)?;
                self.expect_close()?;
                Ok(Node::Assert {
                    behind: false,
                    negative: false,
                    body: Box::new(body),
                })
            }
            Some(b'!') => {
                self.at += 1;
                let body = self.parse_alt(true)?;
                self.expect_close()?;
                Ok(Node::Assert {
                    behind: false,
                    negative: true,
                    body: Box::new(body),
                })
            }
            Some(b'#') => {
                self.at += 1;
                loop {
                    match self.next() {
                        None => {
                            return Err(CompileError::new(self.pattern.len(), "missing ) after comment"));
                        }
                        Some(b')') => break,
                        Some(_) => {}
                    }
                }
                Ok(Node::Anchor(Anchor::Nothing))
            }
            Some(b'C') => {
                // A callout: parsed and ignored (`(?C1)`, `(?C"text")`).
                self.at += 1;
                if self.peek() == Some(b'"') {
                    self.at += 1;
                    while let Some(byte) = self.next() {
                        if byte == b'"' {
                            break;
                        }
                    }
                } else {
                    while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                        self.at += 1;
                    }
                }
                if self.peek() != Some(b')') {
                    return Err(CompileError::new(self.at, "closing ) for (?C expected"));
                }
                self.at += 1;
                Ok(Node::Anchor(Anchor::Nothing))
            }
            Some(b'<') => {
                self.at += 1;
                match self.peek() {
                    Some(b'=') => {
                        self.at += 1;
                        let body = self.parse_alt(true)?;
                        self.expect_close()?;
                        self.check_lookbehind(&body)?;
                        Ok(Node::Assert {
                            behind: true,
                            negative: false,
                            body: Box::new(body),
                        })
                    }
                    Some(b'!') => {
                        self.at += 1;
                        let body = self.parse_alt(true)?;
                        self.expect_close()?;
                        self.check_lookbehind(&body)?;
                        Ok(Node::Assert {
                            behind: true,
                            negative: true,
                            body: Box::new(body),
                        })
                    }
                    Some(byte) if byte.is_ascii_alphabetic() || byte == b'_' => {
                        let name = self.parse_name()?;
                        if self.peek() != Some(b'>') {
                            return Err(CompileError::new(
                                self.at,
                                "syntax error in subpattern name (missing terminator)",
                            ));
                        }
                        self.at += 1;
                        self.parse_named_group(name)
                    }
                    _ => Err(CompileError::new(self.at, "unrecognized character after (?<")),
                }
            }
            Some(b'\'') => {
                self.at += 1;
                let name = self.parse_name()?;
                if self.peek() != Some(b'\'') {
                    return Err(CompileError::new(
                        self.at,
                        "syntax error in subpattern name (missing terminator)",
                    ));
                }
                self.at += 1;
                self.parse_named_group(name)
            }
            Some(b'P') => {
                self.at += 1;
                match self.peek() {
                    Some(b'<') => {
                        self.at += 1;
                        let name = self.parse_python_name()?;
                        if self.peek() != Some(b'>') {
                            return Err(CompileError::new(
                                self.at,
                                "syntax error in subpattern name (missing terminator)",
                            ));
                        }
                        self.at += 1;
                        self.parse_named_group(name)
                    }
                    Some(b'=') => {
                        self.at += 1;
                        let name = self.parse_python_name()?;
                        if self.peek() != Some(b')') {
                            return Err(CompileError::new(
                                self.at,
                                "syntax error in subpattern name (missing terminator)",
                            ));
                        }
                        self.at += 1;
                        let offset = self.at;
                        self.references.push((BackrefRef::Name(name.clone()), offset));
                        Ok(Node::Backref(BackrefRef::Name(name), self.flags.caseless))
                    }
                    _ => Err(CompileError::new(self.at, "unrecognized character after (?P")),
                }
            }
            Some(b'(') => {
                // A conditional: `(?(1)yes|no)`, `(?(<name>)yes|no)`.
                self.at += 1;
                let group = if self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    let mut number = 0u32;
                    while let Some(byte) = self.peek() {
                        if byte.is_ascii_digit() {
                            number = number * 10 + u32::from(byte - b'0');
                            self.at += 1;
                        } else {
                            break;
                        }
                    }
                    number
                } else if self.peek() == Some(b'<') {
                    self.at += 1;
                    let name = self.parse_name()?;
                    self.require(b'>')?;
                    self.named_index(&name)
                } else if self.peek() == Some(b'\'') {
                    self.at += 1;
                    let name = self.parse_name()?;
                    self.require(b'\'')?;
                    self.named_index(&name)
                } else {
                    return Err(CompileError::new(self.at, "unrecognized character after (?"));
                };
                self.require(b')')?;
                let yes = self.parse_concat(true)?;
                let no = if self.peek() == Some(b'|') {
                    self.at += 1;
                    self.parse_concat(true)?
                } else {
                    Node::Empty
                };
                self.expect_close()?;
                Ok(Node::Conditional {
                    group,
                    yes: Box::new(yes),
                    no: Box::new(no),
                })
            }
            Some(b'R') | Some(b'&') => Err(CompileError::new(self.at, "recursion is not supported by this engine")),
            _ => {
                // Inline flags: `(?imsxUn)`, `(?i-s:...)`, `(?i:...)`.
                let start = self.at;
                let mut on = Flags::default();
                let mut off = Flags::default();
                let mut enable = true;
                while let Some(byte) = self.peek() {
                    match byte {
                        b'-' if enable => {
                            enable = false;
                            self.at += 1;
                        }
                        b'i' | b'm' | b's' | b'x' | b'U' | b'n' | b'J' | b'X' => {
                            let target = if enable { &mut on } else { &mut off };
                            match byte {
                                b'i' => target.caseless = true,
                                b'm' => target.multi_line = true,
                                b's' => target.single_line = true,
                                b'x' => target.extended = true,
                                b'U' => target.ungreedy = true,
                                b'n' => target.no_auto_capture = true,
                                _ => {}
                            }
                            self.at += 1;
                        }
                        _ => break,
                    }
                }
                if self.at == start || !matches!(self.peek(), Some(b')') | Some(b':')) {
                    return Err(CompileError::new(
                        self.pattern.len(),
                        "unrecognized character after (? or (?-",
                    ));
                }
                let scoped = self.peek() == Some(b':');
                self.at += 1;
                let saved = self.flags;
                let apply = |flags: &mut Flags, on: &Flags, off: &Flags| {
                    flags.caseless = (flags.caseless || on.caseless) && !off.caseless;
                    flags.multi_line = (flags.multi_line || on.multi_line) && !off.multi_line;
                    flags.single_line = (flags.single_line || on.single_line) && !off.single_line;
                    flags.extended = (flags.extended || on.extended) && !off.extended;
                    flags.ungreedy = (flags.ungreedy || on.ungreedy) && !off.ungreedy;
                    flags.no_auto_capture = (flags.no_auto_capture || on.no_auto_capture) && !off.no_auto_capture;
                };
                apply(&mut self.flags, &on, &off);
                if scoped {
                    let body = self.parse_alt(true)?;
                    self.flags = saved;
                    self.expect_close()?;
                    Ok(Node::Group {
                        index: None,
                        body: Box::new(body),
                        atomic: false,
                    })
                } else {
                    Ok(Node::Anchor(Anchor::Nothing))
                }
            }
        }
    }

    fn named_index(&self, name: &str) -> u32 {
        self.names
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map_or(0, |(_, index)| *index)
    }

    fn parse_named_group(&mut self, name: String) -> PResult<Node> {
        let index = if self.flags.no_auto_capture {
            None
        } else {
            self.group_count += 1;
            if self.names.iter().any(|(candidate, _)| *candidate == name) {
                return Err(CompileError::new(
                    self.at - 1,
                    "two named subpatterns have the same name",
                ));
            }
            self.names.push((name, self.group_count));
            Some(self.group_count)
        };
        let outer = self.current_group;
        if let Some(index) = index {
            self.current_group = index;
        }
        let body = self.parse_alt(true)?;
        self.current_group = outer;
        self.expect_close()?;
        Ok(Node::Group {
            index,
            body: Box::new(body),
            atomic: false,
        })
    }

    /// A subpattern name: letters, digits and `_`, not starting with a digit.
    fn parse_name(&mut self) -> PResult<String> {
        let start = self.at;
        if self.peek().is_some_and(|b| b.is_ascii_digit()) {
            return Err(CompileError::new(self.at, "group name must start with a non-digit"));
        }
        while self.peek().is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_') {
            self.at += 1;
        }
        if self.at == start {
            return Err(CompileError::new(self.at, "group name must start with a non-digit"));
        }
        Ok(String::from_utf8_lossy(&self.pattern[start..self.at]).into_owned())
    }

    /// `(?P<name>` and `(?P=name)`: PCRE reports a digit first.
    fn parse_python_name(&mut self) -> PResult<String> {
        if self.peek().is_some_and(|b| b.is_ascii_digit()) {
            return Err(CompileError::new(self.at, "group name must start with a non-digit"));
        }
        self.parse_name()
    }

    fn require(&mut self, byte: u8) -> PResult<()> {
        if self.peek() != Some(byte) {
            return Err(CompileError::new(
                self.at,
                "syntax error in subpattern name (missing terminator)",
            ));
        }
        self.at += 1;
        Ok(())
    }

    fn expect_close(&mut self) -> PResult<()> {
        if self.peek() != Some(b')') {
            let message = if self.eof() {
                "missing )"
            } else {
                "unmatched parentheses"
            };
            let offset = if self.eof() { self.pattern.len() } else { self.at };
            return Err(CompileError::new(offset, message));
        }
        self.at += 1;
        Ok(())
    }

    /// An escape outside a class.
    fn parse_escape(&mut self, _in_group: bool) -> PResult<Node> {
        self.at += 1; // '\'
        let Some(byte) = self.peek() else {
            return Err(CompileError::new(self.pattern.len(), "\\ at end of pattern"));
        };
        match byte {
            b'A' => {
                self.at += 1;
                Ok(Node::Anchor(Anchor::Start))
            }
            b'z' => {
                self.at += 1;
                Ok(Node::Anchor(Anchor::End))
            }
            b'Z' => {
                self.at += 1;
                Ok(Node::Anchor(Anchor::EndZ))
            }
            b'b' => {
                self.at += 1;
                Ok(Node::Anchor(Anchor::WordBoundary(true)))
            }
            b'B' => {
                self.at += 1;
                Ok(Node::Anchor(Anchor::WordBoundary(false)))
            }
            b'G' => {
                self.at += 1;
                Ok(Node::Anchor(Anchor::StartOfMatch))
            }
            b'Q' => {
                // `\Q...\E`: everything to `\E` is literal.
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    if self.eof() {
                        break;
                    }
                    if self.pattern[self.at] == b'\\' && self.peek_at(1) == Some(b'E') {
                        self.at += 2;
                        break;
                    }
                    let (ch, _) = self
                        .next_char()
                        .ok_or_else(|| CompileError::new(self.at, "invalid UTF-8 string"))?;
                    items.push(Node::Char(ch, false));
                }
                match items.len() {
                    0 => Ok(Node::Empty),
                    1 => Ok(items.pop().expect("one item")),
                    _ => Ok(Node::Concat(items)),
                }
            }
            b'E' => {
                self.at += 1;
                Ok(Node::Empty)
            }
            b'g' => {
                let at = self.at;
                self.at += 1;
                self.parse_g_reference(at)
            }
            b'k' => {
                self.at += 1;
                self.parse_k_reference()
            }
            b'd' | b'D' | b'w' | b'W' | b's' | b'S' | b'h' | b'H' | b'v' | b'V' | b'R' => {
                let shorthand = self.shorthand(byte)?;
                if shorthand == Shorthand::Newline {
                    // `\R` is CRLF as one unit, or any single newline.
                    let pair = self.push_class(ClassSet {
                        negated: false,
                        items: vec![
                            ClassItem::Char(0x0A),
                            ClassItem::Char(0x0B),
                            ClassItem::Char(0x0C),
                            ClassItem::Char(0x0D),
                            ClassItem::Char(0x85),
                            ClassItem::Char(0x2028),
                            ClassItem::Char(0x2029),
                        ],
                    });
                    let crlf = self.push_class(ClassSet {
                        negated: false,
                        items: vec![ClassItem::Char(0x0D), ClassItem::Char(0x0A)],
                    });
                    return Ok(Node::Alt(vec![
                        Node::Concat(vec![Node::Class(crlf, false), Node::Class(pair, false)]),
                        Node::Class(pair, false),
                    ]));
                }
                let class = self.push_class(ClassSet {
                    negated: false,
                    items: vec![ClassItem::Shorthand(shorthand)],
                });
                Ok(Node::Class(class, self.flags.caseless))
            }
            b'p' | b'P' => {
                let negated = byte == b'P';
                self.at += 1;
                if self.peek() != Some(b'{') {
                    return Err(CompileError::new(self.at, "malformed \\P or \\p sequence"));
                }
                self.at += 1;
                let start = self.at;
                while self.peek().is_some_and(|b| b != b'}') {
                    self.at += 1;
                }
                if self.peek() != Some(b'}') {
                    return Err(CompileError::new(self.pattern.len(), "malformed \\P or \\p sequence"));
                }
                let name = String::from_utf8_lossy(&self.pattern[start..self.at]).into_owned();
                self.at += 1;
                let (property, inner_negated) = parse_property(&name)
                    .ok_or_else(|| CompileError::new(self.at - 1, "unknown property name after \\P or \\p"))?;
                let class = self.push_class(ClassSet {
                    negated: false,
                    items: vec![ClassItem::Property(property, inner_negated != negated)],
                });
                Ok(Node::Class(class, self.flags.caseless))
            }
            b'1'..=b'9' => {
                let digits_at = self.at;
                let mut number = 0u32;
                while let Some(byte) = self.peek() {
                    if byte.is_ascii_digit() {
                        number = number * 10 + u32::from(byte - b'0');
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                // `\ddd` is a backreference while the number can be one;
                // above nine with too few groups PCRE re-reads it as an
                // octal escape (`\101` is `A` with no groups at all).
                if number > 9 && number > self.group_count {
                    let digits = &self.pattern[digits_at..self.at];
                    let octal = digits.iter().take(3).take_while(|b| (b'0'..=b'7').contains(b)).count();
                    if octal > 0 {
                        self.at = digits_at;
                        let (value, _) = self.parse_octal(0)?;
                        return Ok(Node::Char(value, self.flags.caseless));
                    }
                }
                let offset = self.at;
                self.references.push((BackrefRef::Number(number), offset));
                Ok(Node::Backref(BackrefRef::Number(number), self.flags.caseless))
            }
            b'0' => {
                // `\0` starts an octal escape.
                self.at += 1;
                let (value, _) = self.parse_octal(0)?;
                Ok(Node::Char(value, self.flags.caseless))
            }
            _ => Ok(Node::Char(self.parse_escape_value()?.0, self.flags.caseless)),
        }
    }

    fn shorthand(&mut self, byte: u8) -> PResult<Shorthand> {
        self.at += 1;
        Ok(match byte {
            b'd' => Shorthand::Digit,
            b'D' => Shorthand::NotDigit,
            b'w' => Shorthand::Word,
            b'W' => Shorthand::NotWord,
            b's' => Shorthand::Space,
            b'S' => Shorthand::NotSpace,
            b'h' => Shorthand::Horiz,
            b'H' => Shorthand::NotHoriz,
            b'v' => Shorthand::Vert,
            b'V' => Shorthand::NotVert,
            _ => Shorthand::Newline,
        })
    }

    fn parse_g_reference(&mut self, letter_at: usize) -> PResult<Node> {
        let start = self.at;
        if self.peek() == Some(b'{') {
            self.at += 1;
            let negative = self.peek() == Some(b'-');
            if negative {
                self.at += 1;
            }
            let mut number = 0u32;
            let mut digits = false;
            while let Some(byte) = self.peek() {
                if byte.is_ascii_digit() {
                    number = number * 10 + u32::from(byte - b'0');
                    self.at += 1;
                    digits = true;
                } else {
                    break;
                }
            }
            if !digits {
                self.at = start;
                return Err(CompileError::new(letter_at, "a numbered reference must not be zero"));
            }
            if self.peek() != Some(b'}') {
                return Err(CompileError::new(self.at, "syntax error in number after (?P or \\g"));
            }
            self.at += 1;
            if number == 0 {
                return Err(CompileError::new(self.at - 1, "a numbered reference must not be zero"));
            }
            let reference = if negative {
                let base = i64::from(self.current_group) + i64::from(number);
                if base <= 0 {
                    return Err(CompileError::new(self.at - 1, "reference to non-existent subpattern"));
                }
                BackrefRef::Number(base as u32)
            } else {
                BackrefRef::Number(number)
            };
            self.references.push((reference.clone(), self.at));
            return Ok(Node::Backref(reference, self.flags.caseless));
        }
        let mut number = 0u32;
        let mut digits = false;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit() {
                number = number * 10 + u32::from(byte - b'0');
                self.at += 1;
                digits = true;
            } else {
                break;
            }
        }
        if !digits || number == 0 {
            // `\g` with no number: PCRE reads the NUL that ends the pattern
            // as the number zero and reports the missing digit's position.
            return Err(CompileError::new(letter_at, "a numbered reference must not be zero"));
        }
        Ok(Node::Backref(BackrefRef::Number(number), self.flags.caseless))
    }

    fn parse_k_reference(&mut self) -> PResult<Node> {
        let close = match self.peek() {
            Some(b'<') => b'>',
            Some(b'\'') => b'\'',
            Some(b'{') => b'}',
            _ => {
                return Err(CompileError::new(
                    self.at,
                    "\\k is not followed by a braced, angle-bracketed, or quoted name",
                ));
            }
        };
        self.at += 1;
        let name = self.parse_name()?;
        if self.peek() != Some(close) {
            return Err(CompileError::new(
                self.at,
                "syntax error in subpattern name (missing terminator)",
            ));
        }
        self.at += 1;
        if name.chars().all(|c| c.is_ascii_digit()) {
            let number: u32 = name.parse().unwrap_or(0);
            self.references.push((BackrefRef::Number(number), self.at));
            return Ok(Node::Backref(BackrefRef::Number(number), self.flags.caseless));
        }
        self.references.push((BackrefRef::Name(name.clone()), self.at));
        Ok(Node::Backref(BackrefRef::Name(name), self.flags.caseless))
    }

    /// PCRE refuses a look-behind whose branches are not of a fixed length;
    /// it reports the assertion's closing parenthesis.
    fn check_lookbehind(&self, body: &Node) -> PResult<()> {
        if !is_fixed_length(body) {
            return Err(CompileError::new(
                self.at - 1,
                "lookbehind assertion is not fixed length",
            ));
        }
        Ok(())
    }

    /// `\x41`, `\x{41}`, `\o{101}`, `\101`, `\cA`, `\n` and the rest.
    fn parse_escape_value(&mut self) -> PResult<EscapeValue> {
        let Some(byte) = self.peek() else {
            return Err(CompileError::new(self.pattern.len(), "\\ at end of pattern"));
        };
        match byte {
            b'x' | b'o' => {
                self.at += 1;
                let braced = self.peek() == Some(b'{');
                if braced {
                    self.at += 1;
                }
                let mut value: u32 = 0;
                let mut digits = 0;
                let limit = if byte == b'x' { 2 } else { 3 };
                while let Some(next) = self.peek() {
                    let digit = match next {
                        b'0'..=b'9' => u32::from(next - b'0'),
                        b'a'..=b'f' => u32::from(next - b'a') + 10,
                        b'A'..=b'F' => u32::from(next - b'A') + 10,
                        _ => break,
                    };
                    value = value.saturating_mul(16).saturating_add(digit);
                    self.at += 1;
                    digits += 1;
                    if !braced && digits == limit {
                        break;
                    }
                }
                if braced {
                    if digits == 0 {
                        return Err(CompileError::new(self.at, "digits missing in \\x{} or \\o{}"));
                    }
                    if self.peek() != Some(b'}') {
                        return Err(CompileError::new(self.at, "syntax error in \\x{} or \\o{}"));
                    }
                    self.at += 1;
                    if value > 0x10FFFF {
                        return Err(CompileError::new(
                            self.at - 1,
                            "character value in \\x{} or \\o{} is too large",
                        ));
                    }
                }
                if digits == 0 {
                    return Ok(EscapeValue(0));
                }
                Ok(EscapeValue(value))
            }
            b'c' => {
                self.at += 1;
                let Some(next) = self.peek() else {
                    return Err(CompileError::new(self.pattern.len(), "\\ at end of pattern"));
                };
                self.at += 1;
                Ok(EscapeValue(u32::from(next.to_ascii_uppercase() ^ 0x40)))
            }
            b'0'..=b'7' => {
                let (value, _) = self.parse_octal(0)?;
                Ok(EscapeValue(value))
            }
            b'n' => {
                self.at += 1;
                Ok(EscapeValue(0x0A))
            }
            b'r' => {
                self.at += 1;
                Ok(EscapeValue(0x0D))
            }
            b't' => {
                self.at += 1;
                Ok(EscapeValue(0x09))
            }
            b'f' => {
                self.at += 1;
                Ok(EscapeValue(0x0C))
            }
            b'v' => {
                self.at += 1;
                Ok(EscapeValue(0x0B))
            }
            b'a' => {
                self.at += 1;
                Ok(EscapeValue(0x07))
            }
            b'e' => {
                self.at += 1;
                Ok(EscapeValue(0x1B))
            }
            _ => {
                let (ch, _) = self
                    .next_char()
                    .ok_or_else(|| CompileError::new(self.at, "invalid UTF-8 string"))?;
                if is_alpha(ch) || is_digit(ch) {
                    return Err(CompileError::new(self.at - 1, "unrecognized character follows \\"));
                }
                Ok(EscapeValue(ch))
            }
        }
    }

    fn parse_octal(&mut self, mut value: u32) -> PResult<(u32, usize)> {
        let mut digits = 0;
        while digits < 3 {
            let Some(byte) = self.peek() else { break };
            if !(b'0'..=b'7').contains(&byte) {
                break;
            }
            value = value * 8 + u32::from(byte - b'0');
            self.at += 1;
            digits += 1;
        }
        Ok((value, digits))
    }

    fn parse_class(&mut self) -> PResult<Node> {
        // self.peek() == b'['
        self.at += 1;
        let mut negated = false;
        if self.peek() == Some(b'^') {
            negated = true;
            self.at += 1;
        }
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let Some(byte) = self.peek() else {
                return Err(CompileError::new(
                    self.pattern.len(),
                    "missing terminating ] for character class",
                ));
            };
            if byte == b']' && !first {
                self.at += 1;
                break;
            }
            first = false;
            // POSIX class
            if byte == b'[' && self.peek_at(1) == Some(b':') {
                let start = self.at + 2;
                let mut at = start;
                let negated_posix = self.pattern.get(at) == Some(&b'^');
                if negated_posix {
                    at += 1;
                }
                let name_start = at;
                while self.pattern.get(at).is_some_and(|b| *b != b':') {
                    at += 1;
                }
                if self.pattern.get(at) == Some(&b':') && self.pattern.get(at + 1) == Some(&b']') {
                    let name = String::from_utf8_lossy(&self.pattern[name_start..at]).into_owned();
                    let Some(posix) = parse_posix(&name) else {
                        return Err(CompileError::new(name_start, "unknown POSIX class name"));
                    };
                    items.push(ClassItem::Posix(posix, negated_posix));
                    self.at = at + 2;
                    continue;
                }
                let _ = start;
            }
            let low = self.parse_class_char()?;
            // A range?
            if self.peek() == Some(b'-') && self.peek_at(1).is_some_and(|b| b != b']') {
                self.at += 1;
                let high = self.parse_class_char()?;
                match (&low, &high) {
                    (ClassChar::Char(lo), ClassChar::Char(hi)) => {
                        if hi < lo {
                            return Err(CompileError::new(
                                self.at - utf8_len(*hi),
                                "range out of order in character class",
                            ));
                        }
                        items.push(ClassItem::Range(*lo, *hi));
                    }
                    (ClassChar::Char(lo), _) => {
                        items.push(ClassItem::Char(*lo));
                        items.push(ClassItem::Char(u32::from(b'-')));
                        push_class_char(&mut items, high);
                    }
                    (_, ClassChar::Char(hi)) => {
                        push_class_char(&mut items, low);
                        items.push(ClassItem::Char(u32::from(b'-')));
                        items.push(ClassItem::Char(*hi));
                    }
                    _ => {
                        push_class_char(&mut items, low);
                        items.push(ClassItem::Char(u32::from(b'-')));
                        push_class_char(&mut items, high);
                    }
                }
                continue;
            }
            push_class_char(&mut items, low);
        }
        let index = self.push_class(ClassSet { negated, items });
        Ok(Node::Class(index, self.flags.caseless))
    }

    fn parse_class_char(&mut self) -> PResult<ClassChar> {
        let Some(byte) = self.peek() else {
            return Err(CompileError::new(
                self.pattern.len(),
                "missing terminating ] for character class",
            ));
        };
        if byte == b'\\' {
            self.at += 1;
            let Some(next) = self.peek() else {
                return Err(CompileError::new(self.pattern.len(), "\\ at end of pattern"));
            };
            match next {
                b'd' | b'D' | b'w' | b'W' | b's' | b'S' | b'h' | b'H' | b'v' | b'V' => {
                    return Ok(ClassChar::Shorthand(self.shorthand(next)?));
                }
                b'b' => {
                    self.at += 1;
                    return Ok(ClassChar::Bs);
                }
                b'p' | b'P' => {
                    let negated = next == b'P';
                    self.at += 1;
                    if self.peek() != Some(b'{') {
                        return Err(CompileError::new(self.at, "malformed \\P or \\p sequence"));
                    }
                    self.at += 1;
                    let start = self.at;
                    while self.peek().is_some_and(|b| b != b'}') {
                        self.at += 1;
                    }
                    if self.peek() != Some(b'}') {
                        return Err(CompileError::new(self.pattern.len(), "malformed \\P or \\p sequence"));
                    }
                    let name = String::from_utf8_lossy(&self.pattern[start..self.at]).into_owned();
                    self.at += 1;
                    let (property, inner) = parse_property(&name)
                        .ok_or_else(|| CompileError::new(self.at - 1, "unknown property name after \\P or \\p"))?;
                    return Ok(ClassChar::Property(property, inner != negated));
                }
                _ => {
                    if (b'0'..=b'7').contains(&next) {
                        let (value, _) = self.parse_octal(0)?;
                        return Ok(ClassChar::Char(value));
                    }
                    return Ok(ClassChar::Char(self.parse_escape_value()?.0));
                }
            }
        }
        let (ch, _) = self
            .next_char()
            .ok_or_else(|| CompileError::new(self.at, "invalid UTF-8 string"))?;
        Ok(ClassChar::Char(ch))
    }

    fn push_class(&mut self, class: ClassSet) -> usize {
        self.classes.push(class);
        self.classes.len() - 1
    }
}

fn utf8_len(ch: u32) -> usize {
    char::from_u32(ch).map_or(1, char::len_utf8)
}

/// A `\x41`, `\n` or similar escape: the character it stands for.
struct EscapeValue(u32);

#[derive(Clone, Debug)]
enum ClassChar {
    Char(u32),
    Shorthand(Shorthand),
    Bs,
    Property(Property, bool),
}

fn push_class_char(items: &mut Vec<ClassItem>, value: ClassChar) {
    match value {
        ClassChar::Char(ch) => items.push(ClassItem::Char(ch)),
        ClassChar::Shorthand(sh) => items.push(ClassItem::Shorthand(sh)),
        ClassChar::Bs => items.push(ClassItem::Bs),
        ClassChar::Property(property, negated) => items.push(ClassItem::Property(property, negated)),
    }
}

fn parse_posix(name: &str) -> Option<Posix> {
    Some(match name.to_ascii_lowercase().as_str() {
        "alnum" => Posix::Alnum,
        "alpha" => Posix::Alpha,
        "ascii" => Posix::Ascii,
        "blank" => Posix::Blank,
        "cntrl" => Posix::Cntrl,
        "digit" => Posix::Digit,
        "graph" => Posix::Graph,
        "lower" => Posix::Lower,
        "print" => Posix::Print,
        "punct" => Posix::Punct,
        "space" => Posix::Space,
        "upper" => Posix::Upper,
        "word" => Posix::Word,
        "xdigit" => Posix::Xdigit,
        _ => return None,
    })
}

/// `\p{L}` and its friends: PCRE's single-letter property names, with `^`
/// negating them.
fn parse_property(name: &str) -> Option<(Property, bool)> {
    let (negated, name) = match name.strip_prefix('^') {
        Some(rest) => (true, rest),
        None => (false, name),
    };
    let upper = name.to_ascii_uppercase();
    let property = match upper.as_str() {
        "L" | "LETTER" => Property::Letter,
        "LL" => Property::Lower,
        "LU" => Property::Upper,
        "N" | "NUMBER" => Property::Number,
        "ND" => Property::Decimal,
        "ALNUM" | "XAN" => Property::Alnum,
        "WORD" | "XWD" => Property::Word,
        "Z" | "SPACE" | "XPS" => Property::Space,
        "XPSXDIGIT" => Property::XpsxDigit,
        "ANY" => Property::Any,
        _ => return None,
    };
    Some((property, negated))
}

// ---------------------------------------------------------------------------
// Lengths (look-behind)
// ---------------------------------------------------------------------------

/// The number of characters a node consumes: `(min, max)` with `None` for an
/// unbounded maximum.
fn char_length(node: &Node) -> (u32, Option<u32>) {
    match node {
        Node::Empty | Node::Anchor(_) => (0, Some(0)),
        Node::Char(_, _) | Node::Class(_, _) | Node::Any(_) => (1, Some(1)),
        Node::Backref(_, _) => (0, None),
        Node::Group { body, .. } => char_length(body),
        Node::Concat(items) => {
            let mut min = 0;
            let mut max = Some(0);
            for item in items {
                let (item_min, item_max) = char_length(item);
                min += item_min;
                max = match (max, item_max) {
                    (Some(a), Some(b)) => Some(a + b),
                    _ => None,
                };
            }
            (min, max)
        }
        Node::Alt(branches) => {
            let mut min = u32::MAX;
            let mut max = Some(0);
            for branch in branches {
                let (branch_min, branch_max) = char_length(branch);
                min = min.min(branch_min);
                max = match (max, branch_max) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    _ => None,
                };
            }
            (min, max)
        }
        Node::Repeat {
            body,
            min: repeat_min,
            max: repeat_max,
            ..
        } => {
            let (body_min, body_max) = char_length(body);
            let lower = body_min * repeat_min;
            let upper = match (body_max, repeat_max) {
                (Some(body_max), Some(repeat_max)) => Some(body_max * repeat_max),
                _ => None,
            };
            (lower, upper)
        }
        Node::Assert { .. } => (0, Some(0)),
        Node::Conditional { yes, no, .. } => {
            let (yes_min, yes_max) = char_length(yes);
            let (no_min, no_max) = char_length(no);
            let min = yes_min.min(no_min);
            let max = match (yes_max, no_max) {
                (Some(a), Some(b)) => Some(a.max(b)),
                _ => None,
            };
            (min, max)
        }
    }
}

/// Replaces a named backreference with the group it names: the parser has
/// every name by the time the whole pattern is read.
fn resolve_references(node: &mut Node, names: &[(String, u32)]) {
    match node {
        Node::Empty | Node::Char(_, _) | Node::Class(_, _) | Node::Any(_) | Node::Anchor(_) => {}
        Node::Backref(reference, _) => {
            if let BackrefRef::Name(name) = reference {
                let number = names
                    .iter()
                    .find(|(candidate, _)| candidate == name)
                    .map_or(0, |(_, number)| *number);
                *reference = BackrefRef::Number(number);
            }
        }
        Node::Group { body, .. } => resolve_references(body, names),
        Node::Concat(items) => {
            for item in items {
                resolve_references(item, names);
            }
        }
        Node::Alt(branches) => {
            for branch in branches {
                resolve_references(branch, names);
            }
        }
        Node::Repeat { body, .. } => resolve_references(body, names),
        Node::Assert { body, .. } => resolve_references(body, names),
        Node::Conditional { yes, no, .. } => {
            resolve_references(yes, names);
            resolve_references(no, names);
        }
    }
}

/// Whether a node always consumes the same number of characters: PCRE's
/// rule for the branches of a look-behind.
fn is_fixed_length(node: &Node) -> bool {
    match node {
        Node::Empty | Node::Anchor(_) => true,
        Node::Char(_, _) | Node::Class(_, _) | Node::Any(_) => true,
        Node::Backref(_, _) => false,
        Node::Group { body, .. } => is_fixed_length(body),
        Node::Concat(items) => items.iter().all(is_fixed_length),
        Node::Alt(branches) => branches.iter().all(is_fixed_length),
        Node::Assert { .. } => true,
        Node::Conditional { yes, no, .. } => is_fixed_length(yes) && is_fixed_length(no),
        Node::Repeat { body, min, max, .. } => is_fixed_length(body) && max.is_some_and(|max| max == *min),
    }
}

// ---------------------------------------------------------------------------
// Compiler
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Inst {
    Char(u32, bool),
    Class(u32, bool),
    Any(bool),
    Bol(bool),
    Eol(bool),
    Start,
    End,
    EndZ,
    WordBoundary(bool),
    StartOfMatch,
    Save(u32),
    Split(u32, u32),
    Jmp(u32),
    Backref(u32, bool),
    Assert {
        behind: bool,
        negative: bool,
        /// The assertion body.
        target: u32,
        /// Where the main flow continues when the assertion held.
        rest: u32,
        min_len: u32,
        max_len: u32,
    },
    AssertEnd,
    /// A conditional `(?(1)yes|no)`: continues when the group participated,
    /// jumps to its target otherwise.
    Cond(u32, u32),
    /// Repeat bookkeeping: `Loop` writes the position and the undo log
    /// length into the register before the body; `LoopEnd` leaves the
    /// repetition (at its target) when the body matched the empty string.
    Loop(u32),
    LoopEnd(u32, u32),
    /// Atomic regions: `Mark` records the backtrack stack length, `Cut`
    /// truncates to it.
    Mark(u32),
    Cut(u32),
    Match,
}

struct Compiler {
    program: Vec<Inst>,
    /// Loop and mark registers, in one numbering space: a register holds two
    /// slots.
    registers: u32,
}

impl Compiler {
    fn new() -> Compiler {
        Compiler {
            program: Vec::new(),
            registers: 0,
        }
    }

    fn compile(mut self, node: &Node) -> Compiled {
        self.emit(Inst::Save(0));
        self.emit_node(node);
        self.emit(Inst::Save(1));
        self.emit(Inst::Match);
        Compiled {
            program: self.program,
            registers: self.registers,
        }
    }

    fn emit(&mut self, inst: Inst) -> u32 {
        self.program.push(inst);
        (self.program.len() - 1) as u32
    }

    fn here(&self) -> u32 {
        self.program.len() as u32
    }

    fn patch(&mut self, at: u32, target: u32) {
        let index = at as usize;
        match &mut self.program[index] {
            Inst::Split(a, b) => {
                if *a == u32::MAX {
                    *a = target;
                } else {
                    *b = target;
                }
            }
            Inst::Jmp(t) => *t = target,
            Inst::LoopEnd(_, t) => *t = target,
            Inst::Cond(_, t) => *t = target,
            other => panic!("cannot patch {other:?}"),
        }
    }

    fn emit_node(&mut self, node: &Node) {
        match node {
            Node::Empty => {}
            Node::Char(ch, caseless) => {
                self.emit(Inst::Char(*ch, *caseless));
            }
            Node::Class(index, caseless) => {
                self.emit(Inst::Class(*index as u32, *caseless));
            }
            Node::Any(single_line) => {
                self.emit(Inst::Any(*single_line));
            }
            Node::Anchor(anchor) => self.emit_anchor(*anchor),
            Node::Backref(reference, caseless) => {
                let number = match reference {
                    BackrefRef::Number(number) => *number,
                    // The parser resolves names before the compiler runs.
                    BackrefRef::Name(_) => 0,
                };
                self.emit(Inst::Backref(number, *caseless));
            }
            Node::Group { index, body, atomic } => {
                if *atomic {
                    let register = self.allocate();
                    self.emit(Inst::Mark(register));
                    self.emit_node(body);
                    self.emit(Inst::Cut(register));
                } else if let Some(index) = index {
                    self.emit(Inst::Save(index * 2));
                    self.emit_node(body);
                    self.emit(Inst::Save(index * 2 + 1));
                } else {
                    self.emit_node(body);
                }
            }
            Node::Concat(items) => {
                for item in items {
                    self.emit_node(item);
                }
            }
            Node::Alt(branches) => {
                let mut jumps = Vec::new();
                for (index, branch) in branches.iter().enumerate() {
                    if index + 1 == branches.len() {
                        self.emit_node(branch);
                        break;
                    }
                    let split = self.emit(Inst::Split(u32::MAX, u32::MAX));
                    let body = self.here();
                    self.patch(split, body);
                    self.emit_node(branch);
                    let jump = self.emit(Inst::Jmp(u32::MAX));
                    jumps.push(jump);
                    let next = self.here();
                    self.patch(split, next);
                }
                let end = self.here();
                for jump in jumps {
                    self.patch(jump, end);
                }
            }
            Node::Conditional { group, yes, no } => {
                let jump = self.emit(Inst::Cond(*group, u32::MAX));
                self.emit_node(yes);
                let over = self.emit(Inst::Jmp(u32::MAX));
                let alternative = self.here();
                self.patch(jump, alternative);
                self.emit_node(no);
                let end = self.here();
                self.patch(over, end);
            }
            Node::Repeat { body, min, max, greed } => self.emit_repeat(body, *min, *max, *greed),
            Node::Assert { behind, negative, body } => {
                let (min_len, max_len) = char_length(body);
                let jump = self.emit(Inst::Assert {
                    behind: *behind,
                    negative: *negative,
                    target: u32::MAX,
                    rest: u32::MAX,
                    min_len,
                    max_len: max_len.unwrap_or(min_len),
                });
                let target = self.here();
                self.emit_node(body);
                self.emit(Inst::AssertEnd);
                let rest = self.here();
                if let Inst::Assert {
                    target: body_at,
                    rest: continuation,
                    ..
                } = &mut self.program[jump as usize]
                {
                    *body_at = target;
                    *continuation = rest;
                }
            }
        }
    }

    fn emit_anchor(&mut self, anchor: Anchor) {
        match anchor {
            Anchor::Bol(multi) => {
                self.emit(Inst::Bol(multi));
            }
            Anchor::Eol(multi) => {
                self.emit(Inst::Eol(multi));
            }
            Anchor::Start => {
                self.emit(Inst::Start);
            }
            Anchor::End => {
                self.emit(Inst::End);
            }
            Anchor::EndZ => {
                self.emit(Inst::EndZ);
            }
            Anchor::WordBoundary(positive) => {
                self.emit(Inst::WordBoundary(positive));
            }
            Anchor::StartOfMatch => {
                self.emit(Inst::StartOfMatch);
            }
            Anchor::Nothing => {}
        }
    }

    fn allocate(&mut self) -> u32 {
        self.registers += 1;
        self.registers - 1
    }

    fn emit_repeat(&mut self, body: &Node, min: u32, max: Option<u32>, greed: Greed) {
        for _ in 0..min {
            self.emit_node(body);
        }
        match max {
            Some(max) if max <= min => return,
            _ => {}
        }
        let mark = match greed {
            Greed::Possessive => {
                let mark = self.allocate();
                self.emit(Inst::Mark(mark));
                Some(mark)
            }
            _ => None,
        };
        let loop_register = self.allocate();
        let start = self.here();
        self.emit(Inst::Loop(loop_register));
        let split = self.emit(Inst::Split(u32::MAX, u32::MAX));
        let body_at = self.here();
        self.emit_node(body);
        let loop_end = self.emit(Inst::LoopEnd(loop_register, u32::MAX));
        self.emit(Inst::Jmp(start));
        let end = self.here();
        match greed {
            Greed::Greedy | Greed::Possessive => {
                self.patch(split, body_at);
                self.patch(split, end);
            }
            Greed::Lazy => {
                self.patch(split, end);
                self.patch(split, body_at);
            }
        }
        self.patch(loop_end, end);
        if let Some(mark) = mark {
            self.emit(Inst::Cut(mark));
        }
    }
}

// ---------------------------------------------------------------------------
// The engine
// ---------------------------------------------------------------------------

/// A compiled program and the number of registers it uses.
struct Compiled {
    program: Vec<Inst>,
    registers: u32,
}

/// A compiled pattern.
pub struct Regex {
    program: Vec<Inst>,
    registers: u32,
    classes: Vec<ClassSet>,
    names: Vec<(String, u32)>,
    group_count: u32,
    #[allow(dead_code, reason = "the flags a pattern was compiled with")]
    flags: Flags,
}

impl Regex {
    /// `pcre_compile` with `PCRE_UTF8`.
    pub fn compile(pattern: &str, flags: Flags) -> Result<Regex, CompileError> {
        Parser::new(pattern, flags).parse()
    }

    /// The capturing groups of the pattern (`pcre_fullinfo`'s
    /// `PCRE_INFO_CAPTURECOUNT`).
    pub fn group_count(&self) -> u32 {
        self.group_count
    }

    /// `pcre_get_stringnumber`: the group of a name, `None` when the name is
    /// not there.
    pub fn group_number(&self, name: &str) -> Option<u32> {
        self.names
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, number)| *number)
    }

    /// `pcre_exec`: the leftmost match in `subject[start..end]`, with the
    /// byte offsets of every group.
    pub fn exec(&self, subject: &[u8], start: usize, end: usize, exec: ExecFlags) -> Option<Captures> {
        let end = end.min(subject.len());
        let mut pos = start.min(end);
        // PCRE matches at character boundaries in UTF-8 mode: a start inside
        // a character moves to the next one.
        while pos < end && is_continuation(subject, pos) {
            pos += 1;
        }
        loop {
            let mut vm = Vm::new(self, subject, end, exec, pos);
            if let Some(found) = vm.run(0, pos, false, None)
                && (!exec.not_empty || found != pos)
            {
                return Some(vm.captures());
            }
            if pos >= end {
                return None;
            }
            // The next character boundary.
            let mut next = pos + 1;
            while next < end && is_continuation(subject, next) {
                next += 1;
            }
            pos = next;
            if exec.anchored {
                return None;
            }
        }
    }
}

fn is_continuation(subject: &[u8], at: usize) -> bool {
    subject.get(at).is_some_and(|byte| byte & 0xC0 == 0x80)
}

/// One entry of the backtrack stack.
#[derive(Clone, Copy)]
struct Frame {
    pc: usize,
    pos: usize,
    log_len: usize,
}

struct Vm<'a> {
    re: &'a Regex,
    subject: &'a [u8],
    end: usize,
    exec: ExecFlags,
    saves: Vec<usize>,
    log: Vec<(usize, usize)>,
    /// Where the search started (`\G`).
    search_start: usize,
    /// The steps this search may take before it is given up (PCRE's
    /// `match_limit`, `pcre_exec`'s default of 10,000,000 backtracking
    /// steps). A pathological pattern (`(a+)+$` over a long subject) would
    /// otherwise run for hours; where the release raises at its limit, the
    /// port answers no match (documented, not probed: no oracle run has
    /// reached a limit).
    budget: u64,
}

/// An unset capture slot.
const UNSET: usize = usize::MAX;

/// `pcre_exec`'s default `match_limit`: the backtracking steps one search
/// may take.
const MATCH_LIMIT: u64 = 10_000_000;

impl<'a> Vm<'a> {
    fn new(re: &'a Regex, subject: &'a [u8], end: usize, exec: ExecFlags, search_start: usize) -> Vm<'a> {
        let groups = re.group_count as usize + 1;
        Vm {
            re,
            subject,
            end,
            exec,
            saves: vec![UNSET; groups * 2 + re.registers as usize * 2],
            log: Vec::new(),
            search_start,
            budget: MATCH_LIMIT,
        }
    }

    fn captures(&self) -> Captures {
        let groups = self.re.group_count as usize + 1;
        (0..groups)
            .map(|group| {
                let start = self.saves[group * 2];
                let end = self.saves[group * 2 + 1];
                if start == UNSET || end == UNSET {
                    None
                } else {
                    Some((start, end))
                }
            })
            .collect()
    }

    fn save(&mut self, slot: usize, value: usize) {
        if self.saves[slot] != value {
            self.log.push((slot, self.saves[slot]));
            self.saves[slot] = value;
        }
    }

    fn restore(&mut self, log_len: usize) {
        while self.log.len() > log_len {
            let (slot, value) = self.log.pop().expect("a log entry");
            self.saves[slot] = value;
        }
    }

    /// Decodes one character at `pos`, with its byte length.
    ///
    /// Only the bytes of the character are read: the subject is a slice of
    /// a `String` (the wrapper takes a `&str`), so it is valid UTF-8 and
    /// the whole rest of it does not have to be validated on every step.
    fn char_at(&self, pos: usize) -> Option<(u32, usize)> {
        if pos >= self.end {
            return None;
        }
        decode(&self.subject[pos..self.end])
    }

    /// The byte offset `count` characters before `pos`.
    fn back_char(&self, pos: usize, count: u32) -> Option<usize> {
        let mut at = pos;
        for _ in 0..count {
            if at == 0 {
                return None;
            }
            at -= 1;
            while at > 0 && is_continuation(self.subject, at) {
                at -= 1;
            }
        }
        Some(at)
    }

    /// Runs the program from `pc` at `pos`; `Some(end position)` when it
    /// matched. `assert_end` is the position a look-behind body has to end
    /// at, and `base` is the stack depth the run may not backtrack below.
    fn run(
        &mut self,
        mut pc: usize,
        mut pos: usize,
        stop_at_assert_end: bool,
        assert_end: Option<usize>,
    ) -> Option<usize> {
        let mut stack: Vec<Frame> = Vec::new();
        loop {
            if self.budget == 0 {
                return None;
            }
            self.budget -= 1;
            let inst = self.re.program[pc];
            match inst {
                Inst::Match => return Some(pos),
                Inst::AssertEnd => {
                    if stop_at_assert_end && assert_end.is_none_or(|want| want == pos) {
                        return Some(pos);
                    }
                }
                Inst::Char(ch, caseless) => {
                    if let Some((found, len)) = self.char_at(pos)
                        && chars_equal(found, ch, caseless)
                    {
                        pos += len;
                        pc += 1;
                        continue;
                    }
                }
                Inst::Class(index, caseless) => {
                    if let Some((found, len)) = self.char_at(pos)
                        && self.re.classes[index as usize].matches(found, caseless)
                    {
                        pos += len;
                        pc += 1;
                        continue;
                    }
                }
                Inst::Any(single_line) => {
                    if let Some((found, len)) = self.char_at(pos)
                        && (single_line || found != 0x0A)
                    {
                        pos += len;
                        pc += 1;
                        continue;
                    }
                }
                Inst::Bol(multi) => {
                    let start_of_line = if multi {
                        pos == 0 || self.subject.get(pos.wrapping_sub(1)) == Some(&b'\n')
                    } else {
                        pos == 0
                    };
                    if start_of_line && !(pos == 0 && self.exec.not_bol) {
                        pc += 1;
                        continue;
                    }
                }
                Inst::Eol(multi) => {
                    let matches = if pos == self.end {
                        !self.exec.not_eol
                    } else if self.subject.get(pos) == Some(&b'\n') {
                        multi || (!self.exec.not_eol && pos + 1 == self.end)
                    } else {
                        false
                    };
                    if matches {
                        pc += 1;
                        continue;
                    }
                }
                Inst::Start => {
                    if pos == 0 {
                        pc += 1;
                        continue;
                    }
                }
                Inst::End => {
                    if pos == self.end {
                        pc += 1;
                        continue;
                    }
                }
                Inst::EndZ => {
                    let matches = pos == self.end
                        || (!self.exec.not_eol && pos + 1 == self.end && self.subject.get(pos) == Some(&b'\n'));
                    if matches {
                        pc += 1;
                        continue;
                    }
                }
                Inst::WordBoundary(positive) => {
                    let before = pos > 0 && self.word_at(pos - 1);
                    let after = pos < self.end && self.word_at(pos);
                    if (before != after) == positive {
                        pc += 1;
                        continue;
                    }
                }
                Inst::StartOfMatch => {
                    if pos == self.search_start {
                        pc += 1;
                        continue;
                    }
                }
                Inst::Save(slot) => {
                    self.save(slot as usize, pos);
                    pc += 1;
                    continue;
                }
                Inst::Split(a, b) => {
                    stack.push(Frame {
                        pc: b as usize,
                        pos,
                        log_len: self.log.len(),
                    });
                    pc = a as usize;
                    continue;
                }
                Inst::Jmp(target) => {
                    pc = target as usize;
                    continue;
                }
                Inst::Backref(group, caseless) => {
                    let start = self.saves[group as usize * 2];
                    let end = self.saves[group as usize * 2 + 1];
                    if start != UNSET && end != UNSET {
                        let text = &self.subject[start..end];
                        let mut at = pos;
                        let mut offset = 0;
                        let mut ok = true;
                        while offset < text.len() {
                            let Some((expected, expected_len)) = decode(&text[offset..]) else {
                                ok = false;
                                break;
                            };
                            let Some((found, found_len)) = self.char_at(at) else {
                                ok = false;
                                break;
                            };
                            if !chars_equal(found, expected, caseless) {
                                ok = false;
                                break;
                            }
                            at += found_len;
                            offset += expected_len;
                        }
                        if ok {
                            pos = at;
                            pc += 1;
                            continue;
                        }
                    }
                }
                Inst::Assert {
                    behind,
                    negative,
                    target,
                    rest,
                    min_len,
                    max_len,
                } => {
                    let log_len = self.log.len();
                    let mut matched = false;
                    if !behind {
                        matched = self.run(target as usize, pos, true, None).is_some();
                    } else {
                        for length in min_len..=max_len {
                            let Some(start) = self.back_char(pos, length) else {
                                continue;
                            };
                            if self.run(target as usize, start, true, Some(pos)).is_some() {
                                matched = true;
                                break;
                            }
                        }
                    }
                    if matched != negative {
                        if negative {
                            self.restore(log_len);
                        }
                        pc = rest as usize;
                        continue;
                    }
                    self.restore(log_len);
                }
                Inst::Cond(group, target) => {
                    let start = self.saves[group as usize * 2];
                    let end = self.saves[group as usize * 2 + 1];
                    if start != UNSET && end != UNSET {
                        pc += 1;
                    } else {
                        pc = target as usize;
                    }
                    continue;
                }
                Inst::Loop(register) => {
                    let slot = self.register_slot(register);
                    self.save(slot + 1, self.log.len());
                    self.save(slot, pos);
                    pc += 1;
                    continue;
                }
                Inst::LoopEnd(register, exit) => {
                    let slot = self.register_slot(register);
                    if self.saves[slot] == pos {
                        // The body matched the empty string: the repetition
                        // ends here and the empty iteration's captures are
                        // dropped.
                        let log_len = self.saves[slot + 1];
                        self.restore(log_len);
                        pc = exit as usize;
                        continue;
                    }
                    pc += 1;
                    continue;
                }
                Inst::Mark(register) => {
                    let slot = self.register_slot(register);
                    self.save(slot, stack.len());
                    pc += 1;
                    continue;
                }
                Inst::Cut(register) => {
                    let slot = self.register_slot(register);
                    let depth = self.saves[slot];
                    if depth != UNSET && depth <= stack.len() {
                        stack.truncate(depth);
                    }
                    pc += 1;
                    continue;
                }
            }
            // The instruction failed: backtrack.
            let frame = stack.pop()?;
            self.restore(frame.log_len);
            pc = frame.pc;
            pos = frame.pos;
        }
    }

    fn register_slot(&self, register: u32) -> usize {
        let groups = self.re.group_count as usize + 1;
        groups * 2 + register as usize * 2
    }

    fn word_at(&self, at: usize) -> bool {
        match self.char_at(at) {
            Some((ch, _)) => is_word(ch),
            None => false,
        }
    }
}

/// Decodes the first character of `bytes`, with its byte length.
///
/// Only the bytes of the character are read: `bytes` is a slice of a
/// `String` (the wrapper takes a `&str`), so it is valid UTF-8 and the
/// whole of it does not have to be validated to read its first character.
fn decode(bytes: &[u8]) -> Option<(u32, usize)> {
    let first = *bytes.first()?;
    // The length of the character from its first byte. A byte that starts
    // no character (a continuation byte, or one of the bytes of the UTF-8
    // that is not valid) is not a character: the slice is then invalid and
    // the character is none, as it was when the whole slice was validated.
    let len = match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    };
    let len = len.min(bytes.len());
    let text = std::str::from_utf8(&bytes[..len]).ok()?;
    let ch = text.chars().next()?;
    Some((ch as u32, ch.len_utf8()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(pattern: &str, subject: &str, options: i32, cap: usize) -> Vec<String> {
        let regex = Regex::compile(pattern, Flags::from_options(options)).expect("a pattern");
        let bytes = subject.as_bytes();
        let mut at = 0;
        let mut found = Vec::new();
        while let Some(captures) = regex.exec(bytes, at, bytes.len(), ExecFlags::default()) {
            let (start, end) = captures[0].expect("the whole match");
            // `MatchedOffset`/`MatchedLength` count UTF-8 characters, not
            // bytes (the RTL converts them).
            let start_chars = subject[..start].chars().count();
            let length = subject[start..end].chars().count();
            found.push(format!("{}+{}", start_chars + 1, length));
            if found.len() >= cap {
                found.push("cap".to_owned());
                break;
            }
            at = end + usize::from(start == end);
            if at > bytes.len() {
                break;
            }
        }
        found
    }

    /// The search of a long subject does not take a time that grows with
    /// the square of its length.
    ///
    /// The engine read the whole rest of the subject on every step of the
    /// match, so a match that walks over the whole of a subject took a time
    /// that grew with the square of its length; the character at a position
    /// is now read from its own bytes.
    ///
    /// The test is a guard, not a measurement of a speed: the time of a
    /// match over a subject of a tenth of the length is the reference, and
    /// the quadratic search could not hold a tenth of the work for a tenth
    /// of the subject.
    #[test]
    fn a_long_subject_is_not_searched_quadratically() {
        // A subject of a few kilobytes of ASCII. The pattern walks over the
        // whole of it and matches at its end, so every character of the
        // subject is read once.
        let long = "x".repeat(200_000) + "needle";
        let short = "x".repeat(20_000) + "needle";
        let pattern = ".*needle";
        let measure = |subject: &str| {
            let regex = Regex::compile(pattern, Flags::default()).expect("a pattern");
            let bytes = subject.as_bytes();
            let exec = ExecFlags {
                anchored: true,
                ..ExecFlags::default()
            };
            let start = std::time::Instant::now();
            let found = regex.exec(bytes, 0, bytes.len(), exec);
            (
                start.elapsed(),
                found.map(|captures| captures[0].expect("the whole match")),
            )
        };
        let (long_time, long_found) = measure(&long);
        let (short_time, short_found) = measure(&short);
        // The match is the same, at the end of the subject.
        assert_eq!(long_found, Some((0, 200_006)));
        assert_eq!(short_found, Some((0, 20_006)));
        // The search of the long subject may not take the hundred times
        // the work of the short one: the quadratic search would.
        assert!(
            long_time < short_time * 50,
            "the search of {} bytes took {long_time:?}, the search of {} bytes took {short_time:?}",
            long.len(),
            short.len()
        );
    }

    /// The offsets and lengths the 4.1.5q release reports for one of the
    /// probe's patterns (1-based, as `MatchedOffset`/`MatchedLength`).
    #[test]
    fn probe_offsets() {
        assert_eq!(walk(".", "ab", 0, 10), ["1+1", "2+1"]);
        assert_eq!(walk("a(?=b)", "ab ac", 0, 10), ["1+1"]);
        assert_eq!(walk("(?<=a)b", "ab cb", 0, 10), ["2+1"]);
        assert_eq!(walk("(a)\\1", "aa ab", 0, 10), ["1+2"]);
        assert_eq!(walk("a++b", "aab", 0, 10), ["1+3"]);
        assert_eq!(walk("a*+a", "aa", 0, 10), Vec::<String>::new());
        assert_eq!(walk("(?>a+)a", "aa", 0, 10), Vec::<String>::new());
        assert_eq!(walk("a|ab", "ab", 0, 10), ["1+1"]);
        assert_eq!(walk("(a|ab)*b", "abb", 0, 10), ["1+2", "3+1"]);
        assert_eq!(walk("a$", "a\n", 0, 10), ["1+1"]);
        assert_eq!(walk("a$", "a\n\n", 0, 10), Vec::<String>::new());
        assert_eq!(walk("a\\Z", "a\n", 0, 10), ["1+1"]);
        assert_eq!(walk("a\\z", "a\n", 0, 10), Vec::<String>::new());
        assert_eq!(walk("^b", "a\nb", 2, 10), ["3+1"]);
        assert_eq!(walk("a.*b", "axbxb", 32, 10), ["1+3"]);
        assert_eq!(walk("x*", "abc", 0, 10), ["1+0", "2+0", "3+0", "4+0"]);
        assert_eq!(walk("[a]*", "aab", 0, 10), ["1+2", "3+0", "4+0"]);
        assert_eq!(walk("a|", "ab", 0, 10), ["1+1", "2+0", "3+0"]);
        assert_eq!(walk("$", "abc", 0, 10), ["4+0"]);
        assert_eq!(walk("$", "a\nb", 2, 10), ["2+0", "4+0"]);
        assert_eq!(walk("(a)?(?(1)b|c)", "ab c", 0, 10), ["1+2", "4+1"]);
        assert_eq!(walk("\\Qa.b\\E", "a.b axb", 0, 10), ["1+3"]);
        assert_eq!(walk("(a*)(a*)", "aaa", 0, 10), ["1+3", "4+0"]);
        assert_eq!(walk("(a?)*", "aa", 0, 10), ["1+2", "3+0"]);
        assert_eq!(walk("a$|b", "ab", 0, 10), ["2+1"]);
        assert_eq!(walk("a^b", "ab", 0, 10), Vec::<String>::new());
        assert_eq!(walk("a\\Bb", "axb abb", 0, 10), ["5+2"]);
        // A class with `preCaseLess`: the release folds Latin-1.
        assert_eq!(walk("[a-z]", "AZ", 1, 10), ["1+1", "2+1"]);
        // `\ddd` above nine with too few groups is an octal escape.
        assert_eq!(walk("\\101", "A", 0, 10), ["1+1"]);
        assert_eq!(walk("\\12", "\n", 0, 10), ["1+1"]);
    }

    #[test]
    fn probe_multibyte() {
        let subject = "a\u{e9}z";
        assert_eq!(walk(".", subject, 0, 10), ["1+1", "2+1", "3+1"]);
        assert_eq!(walk("\u{e9}", subject, 0, 10), ["2+1"]);
        assert_eq!(walk("[^a]", subject, 0, 10), ["2+1", "3+1"]);
        assert_eq!(walk("\\w", subject, 0, 10), ["1+1", "3+1"]);
        assert_eq!(walk("\\W", subject, 0, 10), ["2+1"]);
        assert_eq!(walk("\\b", subject, 0, 10), ["1+0", "2+0", "3+0", "4+0"]);
        assert_eq!(walk("[[:alpha:]]", subject, 0, 10), ["1+1", "3+1"]);
        // Latin-1 case folding, ASCII classes.
        assert_eq!(walk("\u{e9}", "\u{c9}\u{e9}", 1, 10), ["1+1", "2+1"]);
        assert_eq!(walk("[a-z]", "\u{c9}\u{e9}", 1, 10), Vec::<String>::new());
    }

    #[test]
    fn probe_errors() {
        let error = |pattern: &str| {
            Regex::compile(pattern, Flags::default())
                .err()
                .map(|error| format!("{} at {}", error.message, error.offset))
        };
        assert_eq!(error("a("), Some("missing ) at 2".to_owned()));
        assert_eq!(
            error("a["),
            Some("missing terminating ] for character class at 2".to_owned())
        );
        assert_eq!(error("*a"), Some("nothing to repeat at 0".to_owned()));
        assert_eq!(
            error("a{2,1}"),
            Some("numbers out of order in {} quantifier at 5".to_owned())
        );
        assert_eq!(error("a**"), Some("nothing to repeat at 2".to_owned()));
        assert_eq!(error("\\"), Some("\\ at end of pattern at 1".to_owned()));
        assert_eq!(
            error("[z-a]"),
            Some("range out of order in character class at 3".to_owned())
        );
        assert_eq!(error("(?<>)"), Some("unrecognized character after (?< at 3".to_owned()));
        assert_eq!(
            error("(?P<a>x)(?P<a>y)"),
            Some("two named subpatterns have the same name at 13".to_owned())
        );
        assert_eq!(
            error("(a)\\2"),
            Some("reference to non-existent subpattern at 5".to_owned())
        );
        assert_eq!(
            error("\\1"),
            Some("reference to non-existent subpattern at 2".to_owned())
        );
        assert_eq!(error("(?#"), Some("missing ) after comment at 3".to_owned()));
        assert_eq!(error("a{1,2}{3}"), Some("nothing to repeat at 8".to_owned()));
        assert_eq!(error("]"), None);
        assert_eq!(error("a)b"), Some("unmatched parentheses at 1".to_owned()));
        assert_eq!(
            error("(?i"),
            Some("unrecognized character after (? or (?- at 3".to_owned())
        );
        assert_eq!(error("x{2"), None);
        assert_eq!(
            error("(?P<1a>x)"),
            Some("group name must start with a non-digit at 4".to_owned())
        );
        assert_eq!(
            error("(?<=a+)b"),
            Some("lookbehind assertion is not fixed length at 6".to_owned())
        );
        assert_eq!(
            error("(?P=1)"),
            Some("group name must start with a non-digit at 4".to_owned())
        );
        assert_eq!(error("[[:nope:]]"), Some("unknown POSIX class name at 3".to_owned()));
        assert_eq!(
            error("\\g"),
            Some("a numbered reference must not be zero at 1".to_owned())
        );
        assert_eq!(error("\\x{}"), Some("digits missing in \\x{} or \\o{} at 3".to_owned()));
        assert_eq!(error("a{,2}"), None);
        assert_eq!(
            error("(?<a)"),
            Some("syntax error in subpattern name (missing terminator) at 4".to_owned())
        );
        assert_eq!(error("(?Cx)"), Some("closing ) for (?C expected at 3".to_owned()));
    }

    #[test]
    fn groups_and_offsets() {
        let regex = Regex::compile("(a(.))z", Flags::default()).expect("a pattern");
        let subject = "a\u{e9}z".as_bytes();
        let captures = regex
            .exec(subject, 0, subject.len(), ExecFlags::default())
            .expect("a match");
        assert_eq!(captures[0], Some((0, 4)));
        assert_eq!(captures[1], Some((0, 3)));
        assert_eq!(captures[2], Some((1, 3)));
        assert_eq!(regex.group_count(), 2);
        let regex = Regex::compile("(?P<n>a)(b)", Flags::default()).expect("a pattern");
        assert_eq!(regex.group_number("n"), Some(1));
        assert_eq!(regex.group_number("nope"), None);
    }
}
