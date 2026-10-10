// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreterParser.pas (the
// tokenizer: TTokenKind, PaTokenizeTag with its hash tables, TokenTyp, Prior,
// TypToken, TJvInterpreterParser.Token) and External/jvcl/jvcl/run/JvInterpreterConst.pas
// (the St* character sets).

//! The tokenizer and token model of `JvInterpreterParser.pas`, as-is.
//!
//! The source is processed byte for byte: the Delphi unit reads a `string`
//! (UTF-16 in the build xEdit ships), but every token the interpreter cares
//! about is ASCII, `TTokenTag` truncates every character to a byte for its
//! hash (`Byte(TokenStr[I])`, `:404`), and the corpus is pure ASCII, so the
//! byte port is exact for it. A byte >= 128 in an identifier follows
//! `IsCharAlpha` of the Windows API, which classifies by the machine's ANSI
//! code page; [`is_char_alpha`] implements the CP1252 classification of this
//! machine.
//!
//! Upstream quirks kept on purpose, with their line numbers:
//!
//! * the hash tables `AssoIndices`/`AssoValues`/`WordList` are ported verbatim;
//!   the tables hold duplicate entries (e.g. `AssoValues[8]` and `[232]` are
//!   both 50) and the keyword compare uses `Cmp`, the locale-aware
//!   case-insensitive compare (`CompareString` with `LOCALE_USER_DEFAULT`),
//!   which [`cmp`] reduces to ASCII-case-insensitive equality because every
//!   keyword is ASCII;
//! * `TokenTyp('+')` and `TokenTyp('-')` are `ttInteger` when a `Token` could
//!   produce them outside `Asso1Values` -- they cannot, the single-character
//!   table catches them first (`:451`), but the multi-character path applies
//!   the number test to them (`:509`);
//! * `IsScientificNotation` needs a digit before the `E` (`:515`);
//! * a `#nnn` character constant is converted to the quoted form
//!   `'<char>'` (`:754`), so `#13#10` becomes two adjacent string tokens the
//!   expression parser concatenates;
//! * an unterminated string constant swallows the line end or the `#0`
//!   (`Inc(P)` after the loop, `:735`);
//! * the doubled-quote cleanup of a string token skips the character after a
//!   deletion (`while I < Length(Result) - 1`, `:737`);
//! * `PrevPoint` in `Token` is read at the start of the token region *before*
//!   whitespace is skipped (`:656`), so a national-symbol continuation
//!   depends on the character before the whitespace.

use crate::error::{Error, IE_BAD_REMARK};

/// The token kind (`TTokenKind`, `JvInterpreterParser.pas:44`).
pub type TTokenKind = i32;

pub const TT_UNKNOWN: TTokenKind = -1;
pub const TT_EMPTY: TTokenKind = 0;
pub const TT_IDENTIFIER: TTokenKind = 10;
pub const TT_INTEGER: TTokenKind = 11;
pub const TT_DOUBLE: TTokenKind = 12;
pub const TT_STRING: TTokenKind = 13;
pub const TT_BOOLEAN: TTokenKind = 14;

pub const TT_LB: TTokenKind = 40;
pub const TT_RB: TTokenKind = 41;
pub const TT_COL: TTokenKind = 42;
pub const TT_POINT: TTokenKind = 43;
pub const TT_COLON: TTokenKind = 44;
pub const TT_SEMICOLON: TTokenKind = 45;
pub const TT_LS: TTokenKind = 46;
pub const TT_RS: TTokenKind = 47;
pub const TT_DOUBLE_POINT: TTokenKind = 48;
pub const TT_DOUBLE_QUOTE: TTokenKind = 49;

pub const TT_FALSE: TTokenKind = 63;
pub const TT_TRUE: TTokenKind = 65;

pub const TT_BEGIN: TTokenKind = 66;
pub const TT_END: TTokenKind = 67;
pub const TT_IF: TTokenKind = 68;
pub const TT_THEN: TTokenKind = 69;
pub const TT_ELSE: TTokenKind = 70;
pub const TT_WHILE: TTokenKind = 71;
pub const TT_DO: TTokenKind = 72;
pub const TT_REPEAT: TTokenKind = 73;
pub const TT_UNTIL: TTokenKind = 74;
pub const TT_PROCEDURE: TTokenKind = 75;
pub const TT_FUNCTION: TTokenKind = 76;
pub const TT_FOR: TTokenKind = 77;
pub const TT_TO: TTokenKind = 78;
pub const TT_BREAK: TTokenKind = 79;
pub const TT_CONTINUE: TTokenKind = 80;
pub const TT_VAR: TTokenKind = 81;
pub const TT_TRY: TTokenKind = 82;
pub const TT_FINALLY: TTokenKind = 83;
pub const TT_EXCEPT: TTokenKind = 84;
pub const TT_ON: TTokenKind = 85;
pub const TT_RAISE: TTokenKind = 86;
pub const TT_EXTERNAL: TTokenKind = 87;
pub const TT_UNIT: TTokenKind = 88;
pub const TT_USES: TTokenKind = 89;
pub const TT_CONST: TTokenKind = 90;
pub const TT_PUBLIC: TTokenKind = 91;
pub const TT_PRIVATE: TTokenKind = 92;
pub const TT_PROTECTED: TTokenKind = 93;
pub const TT_PUBLISHED: TTokenKind = 94;
pub const TT_PROPERTY: TTokenKind = 95;
pub const TT_CLASS: TTokenKind = 96;
pub const TT_TYPE: TTokenKind = 97;
pub const TT_INTERFACE: TTokenKind = 98;
pub const TT_IMPLEMENTATION: TTokenKind = 99;
pub const TT_EXIT: TTokenKind = 100;
pub const TT_ARRAY: TTokenKind = 101;
pub const TT_OF: TTokenKind = 102;
pub const TT_CASE: TTokenKind = 103;
pub const TT_PROGRAM: TTokenKind = 104;
pub const TT_IN: TTokenKind = 105;
pub const TT_RECORD: TTokenKind = 106;
pub const TT_DOWNTO: TTokenKind = 107;

pub const TT_NOT: TTokenKind = 21;
pub const TT_MUL: TTokenKind = 22;
pub const TT_DIV: TTokenKind = 23;
pub const TT_INT_DIV: TTokenKind = 24;
pub const TT_MOD: TTokenKind = 25;
pub const TT_AND: TTokenKind = 26;
pub const TT_PLUS: TTokenKind = 27;
pub const TT_MINUS: TTokenKind = 28;
pub const TT_OR: TTokenKind = 29;
pub const TT_EQU: TTokenKind = 30;
pub const TT_GREATER: TTokenKind = 31;
pub const TT_LESS: TTokenKind = 32;
pub const TT_NOT_EQU: TTokenKind = 33;
pub const TT_EQU_GREATER: TTokenKind = 34;
pub const TT_EQU_LESS: TTokenKind = 35;
pub const TT_SHL: TTokenKind = 36;
pub const TT_SHR: TTokenKind = 37;
pub const TT_XOR: TTokenKind = 38;

pub const TT_FIRST_EXPRESSION: TTokenKind = 10;
pub const TT_LAST_EXPRESSION: TTokenKind = 59;

// The priority levels (`JvInterpreterParser.pas:180`).
pub const PRIOR_NOT: i32 = 8;
pub const PRIOR_MUL: i32 = 6;
pub const PRIOR_DIV: i32 = 6;
pub const PRIOR_INT_DIV: i32 = 6;
pub const PRIOR_MOD: i32 = 6;
pub const PRIOR_AND: i32 = 5;
pub const PRIOR_PLUS: i32 = 4;
pub const PRIOR_MINUS: i32 = 4;
pub const PRIOR_OR: i32 = 4;
pub const PRIOR_EQU: i32 = 3;
pub const PRIOR_GREATER: i32 = 3;
pub const PRIOR_LESS: i32 = 3;
pub const PRIOR_NOT_EQU: i32 = 3;
pub const PRIOR_EQU_GREATER: i32 = 2;
pub const PRIOR_EQU_LESS: i32 = 2;
pub const PRIOR_SHL: i32 = 6;
pub const PRIOR_SHR: i32 = 6;
pub const PRIOR_XOR: i32 = 3;

/// `Prior` (`JvInterpreterParser.pas:571`): the priority of an operator token;
/// 0 for everything outside `ttNot..ttXor` (21..38).
pub fn prior(kind: TTokenKind) -> i32 {
    const PRIORS: [i32; 18] = [
        PRIOR_NOT,
        PRIOR_MUL,
        PRIOR_DIV,
        PRIOR_INT_DIV,
        PRIOR_MOD,
        PRIOR_AND,
        PRIOR_PLUS,
        PRIOR_MINUS,
        PRIOR_OR,
        PRIOR_EQU,
        PRIOR_GREATER,
        PRIOR_LESS,
        PRIOR_NOT_EQU,
        PRIOR_EQU_GREATER,
        PRIOR_EQU_LESS,
        PRIOR_SHL,
        PRIOR_SHR,
        PRIOR_XOR,
    ];
    if (TT_NOT..=TT_XOR).contains(&kind) {
        PRIORS[(kind - TT_NOT) as usize]
    } else {
        0
    }
}

/// `TypToken` (`JvInterpreterParser.pas:566`): not implemented upstream.
pub fn type_token(_kind: TTokenKind) -> &'static str {
    "?? not implemented !!"
}

// The character sets of `JvInterpreterConst.pas:42-48`.
pub fn is_id_symbol(c: u8) -> bool {
    c == b'_' || c.is_ascii_digit() || c.is_ascii_uppercase() || c.is_ascii_lowercase()
}

pub fn is_id_first_symbol(c: u8) -> bool {
    c == b'_' || c.is_ascii_uppercase() || c.is_ascii_lowercase()
}

/// `StConstSymbols` -- a hexadecimal digit.
pub fn is_const_symbol(c: u8) -> bool {
    c.is_ascii_digit() || (b'A'..=b'F').contains(&c) || (b'a'..=b'f').contains(&c)
}

pub fn is_const_symbol_10(c: u8) -> bool {
    c.is_ascii_digit()
}

pub fn is_e(c: u8) -> bool {
    c == b'E' || c == b'e'
}

pub fn is_plus_sub(c: u8) -> bool {
    c == b'+' || c == b'-'
}

pub fn is_const_symbol_10_e(c: u8) -> bool {
    c.is_ascii_digit() || is_e(c) || is_plus_sub(c) || c == b'.'
}

/// `IsCharAlpha` of the Windows API as the ANSI build sees it: ASCII letters,
/// and the letters of the CP1252 code page for bytes >= 128 (the ANSI code
/// page of this machine; the corpus uses ASCII only).
pub fn is_char_alpha(c: u8) -> bool {
    if c < 128 {
        return c.is_ascii_alphabetic();
    }
    matches!(c,
        0x83 | 0x8A | 0x8C | 0x8E | 0x9A | 0x9C | 0x9E | 0x9F
        | 0xAA | 0xB5 | 0xBA
        | 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0xFF)
}

/// `Cmp` (`JvInterpreter.pas:1733`): upstream compares with
/// `CompareString(LOCALE_USER_DEFAULT, NORM_IGNORECASE, ...) = 2`. Keywords are
/// ASCII, where that is case-insensitive equality.
pub fn cmp(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.eq_ignore_ascii_case(b)
}

// [peter schraut: added on 2005/08/14] -- the hash tables, verbatim
// (`JvInterpreterParser.pas:295-329`).
#[rustfmt::skip]
const ASSO_INDICES: [i32; 32] = [
    50, 80, 25, 13, 92, 71, 87, 61, 91, 99, 73, 95, 27, 7, 16, 1,
    96, 41, 91, 99, 19, 15, 72, 1, 50, 30, 9, 6, 45, 27, 79, 61,
];

#[rustfmt::skip]
const ASSO_VALUES: [i32; 256] = [
    -1, -1, -1, -1, -1, -1, 44, 10, -1, -1,
    37, -1, -1, -1, -1, 7, -1, -1, -1, -1,
    -1, -1, -1, 27, -1, -1, -1, -1, -1, -1,
    -1, 41, 26, -1, -1, 20, -1, -1, -1, 28,
    -1, 30, 39, -1, -1, -1, -1, 13, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, 1, -1, -1,
    -1, -1, -1, -1, -1, 12, -1, -1, -1, -1,
    -1, -1, 6, -1, -1, -1, -1, -1, -1, -1,
    34, -1, -1, -1, -1, -1, 3, -1, -1, 49,
    -1, -1, 45, -1, -1, -1, -1, -1, -1, -1,
    2, -1, 51, -1, -1, -1, -1, 46, -1, -1,
    -1, -1, 17, -1, -1, -1, 36, -1, 11, -1,
    -1, -1, 35, 48, -1, -1, -1, -1, 8, -1,
    -1, 32, -1, 19, -1, -1, -1, 5, -1, -1,
    40, -1, -1, -1, -1, -1, -1, -1, 21, -1,
    22, -1, 31, -1, -1, -1, -1, -1, -1, 16,
    43, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, 18, -1, -1, -1, -1, 47, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, 42, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    29, -1, -1, 25, 4, 15, 24, -1, -1, -1,
    -1, -1, 33, -1, -1, 9, -1, 50, -1, 14,
    -1, -1, -1, 23, -1, -1, 38, -1, -1, -1,
    -1, -1, -1, -1, -1, 0,
];

const WORD_LIST: [(&[u8], TTokenKind); 52] = [
    (b"true", TT_TRUE),
    (b"false", TT_FALSE),
    (b"or", TT_OR),
    (b"and", TT_AND),
    (b"not", TT_NOT),
    (b"div", TT_INT_DIV),
    (b"mod", TT_MOD),
    (b"begin", TT_BEGIN),
    (b"end", TT_END),
    (b"if", TT_IF),
    (b"then", TT_THEN),
    (b"else", TT_ELSE),
    (b"while", TT_WHILE),
    (b"do", TT_DO),
    (b"repeat", TT_REPEAT),
    (b"until", TT_UNTIL),
    (b"procedure", TT_PROCEDURE),
    (b"function", TT_FUNCTION),
    (b"for", TT_FOR),
    (b"to", TT_TO),
    (b"break", TT_BREAK),
    (b"continue", TT_CONTINUE),
    (b"var", TT_VAR),
    (b"try", TT_TRY),
    (b"finally", TT_FINALLY),
    (b"except", TT_EXCEPT),
    (b"on", TT_ON),
    (b"raise", TT_RAISE),
    (b"external", TT_EXTERNAL),
    (b"unit", TT_UNIT),
    (b"uses", TT_USES),
    (b"const", TT_CONST),
    (b"public", TT_PUBLIC),
    (b"private", TT_PRIVATE),
    (b"protected", TT_PROTECTED),
    (b"published", TT_PUBLISHED),
    (b"property", TT_PROPERTY),
    (b"class", TT_CLASS),
    (b"type", TT_TYPE),
    (b"interface", TT_INTERFACE),
    (b"implementation", TT_IMPLEMENTATION),
    (b"exit", TT_EXIT),
    (b"array", TT_ARRAY),
    (b"of", TT_OF),
    (b"case", TT_CASE),
    (b"program", TT_PROGRAM),
    (b"in", TT_IN),
    (b"record", TT_RECORD),
    (b"downto", TT_DOWNTO),
    (b"shl", TT_SHL),
    (b"shr", TT_SHR),
    (b"xor", TT_XOR),
];

const MIN_WORD_LENGTH: i32 = 2;
const MAX_WORD_LENGTH: i32 = 14;

/// `PaTokenizeTag` (`JvInterpreterParser.pas:390`): the keyword hash; a
/// non-keyword gives -1. The hash arithmetic is `Integer` arithmetic upstream
/// (`Byte(Ch) - Byte('a')` can be negative; the `$1F` mask is applied to the
/// two's complement), so it is `i32` arithmetic here.
fn pa_tokenize_tag(token: &[u8]) -> TTokenKind {
    let len = token.len() as i32;
    if !((MIN_WORD_LENGTH..=MAX_WORD_LENGTH).contains(&len)) {
        return -1;
    }
    let index_of = |c: u8| -> i32 {
        let i = ((c as i32) - (b'a' as i32)) & 0x1f;
        ASSO_INDICES[i as usize]
    };
    let mut h = len;
    for i in 0..len {
        h += index_of(token[i as usize]);
        if i + 1 == 3 {
            break;
        }
    }
    h += index_of(token[len as usize - 1]);
    h &= 255;
    let slot = ASSO_VALUES[h as usize];
    if slot != -1 && cmp(WORD_LIST[slot as usize].0, token) {
        WORD_LIST[slot as usize].1
    } else {
        -1
    }
}

// `!"#$%&'()*+,-./0123456789:;<=>?` (`:420`).
#[rustfmt::skip]
const ASSO1_VALUES: [TTokenKind; 32] = [
    -1, -1, -1, -1, -1, -1, -1, -1,
    TT_LB, TT_RB, TT_MUL, TT_PLUS, TT_COL, TT_MINUS, TT_POINT, TT_DIV,
    TT_INTEGER, TT_INTEGER, TT_INTEGER, TT_INTEGER, TT_INTEGER,
    TT_INTEGER, TT_INTEGER, TT_INTEGER, TT_INTEGER, TT_INTEGER,
    TT_COLON, TT_SEMICOLON, TT_LESS, TT_EQU, TT_GREATER, -1,
];

/// `TokenTyp` (`JvInterpreterParser.pas:429`): the kind of a token's text.
pub fn token_kind(text: &[u8]) -> TTokenKind {
    if text.is_empty() {
        return TT_EMPTY;
    }
    let t1 = text[0];
    if text.len() == 1 {
        // `if CharInSet(T1, ['('..'>']) then Result := Asso1Values[T1]`
        if (b'('..=b'>').contains(&t1) {
            return ASSO1_VALUES[(t1 - b' ') as usize];
        }
        if t1 == b'[' {
            return TT_LS;
        }
        if t1 == b']' {
            return TT_RS;
        }
        if t1 == b'"' {
            return TT_DOUBLE_QUOTE;
        }
        // else: goto Any
    } else {
        match t1 {
            b'.' => {
                if text[1] == b'.' {
                    return TT_DOUBLE_POINT;
                }
            }
            b'$' => {
                if text[1..].iter().all(|&c| is_const_symbol(c)) {
                    return TT_INTEGER;
                }
            }
            b'<' => {
                if text.len() == 2 {
                    match text[1] {
                        b'=' => return TT_EQU_LESS,
                        b'>' => return TT_NOT_EQU,
                        _ => {}
                    }
                }
            }
            b'>' if text.len() == 2 && text[1] == b'=' => {
                return TT_EQU_GREATER;
            }
            _ => {}
        }
        // else: goto Any
    }
    'any: {
        let mut point = false;
        let mut is_scientific_notation = false;
        for (i, &ci) in text.iter().enumerate() {
            if i == 0 && !(is_const_symbol_10(ci) || is_plus_sub(ci)) {
                break 'any; // goto NotNumber
            }
            // Scientific notation requires a digit before the E/e (`:515`).
            if i > 0 && is_e(ci) && is_const_symbol_10(text[i - 1]) {
                is_scientific_notation = true;
            }
            if ci == b'.' {
                if point {
                    break 'any; // two points in the lexem
                }
                point = true;
            } else if !is_const_symbol_10_e(ci) {
                break 'any; // not a number
            }
        }
        return if point || is_scientific_notation {
            TT_DOUBLE
        } else {
            TT_INTEGER
        };
    }
    // NotNumber:
    if text.len() >= 2 && text[0] == b'\'' && text[text.len() - 1] == b'\'' {
        return TT_STRING;
    }
    let keyword = pa_tokenize_tag(text);
    if keyword != -1 {
        return keyword;
    }
    if !(is_id_first_symbol(t1) || is_char_alpha(t1)) {
        return TT_UNKNOWN;
    }
    for &c in &text[1..] {
        if !(is_id_symbol(c) || is_char_alpha(c)) {
            return TT_UNKNOWN;
        }
    }
    TT_IDENTIFIER
}

/// One token of the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawToken {
    /// The token text, exactly as `TJvInterpreterParser.Token` returns it.
    pub text: Vec<u8>,
    /// Byte offset of the first character.
    pub start: usize,
    /// Byte offset after the last character (the parser's `CurPos`).
    pub end: usize,
}

/// `TJvInterpreterParser` (`JvInterpreterParser.pas:46`): the hand-written
/// tokenizer. [`Tokenizer::token`] returns the next token and advances; an
/// end of source gives the empty token (`ttEmpty`).
pub struct Tokenizer<'a> {
    source: &'a [u8],
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self { source, pos: 0 }
    }

    /// `TJvInterpreterParser.Pos` (`:800`), the current parse position.
    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    fn at(&self, i: usize) -> u8 {
        self.source.get(i).copied().unwrap_or(0)
    }

    /// `Skip` (`:607`): spaces, remarks and the three remark errors.
    fn skip(&mut self) -> Result<(), Error> {
        let start = self.pos;
        match self.at(self.pos) {
            b'{' => {
                let mut q = self.pos + 1;
                loop {
                    if q >= self.source.len() {
                        // `F = nil` when StrScan finds no '}'.
                        return Err(Error::new(IE_BAD_REMARK, start as i64, "", ""));
                    }
                    if self.source[q] == b'}' {
                        break;
                    }
                    q += 1;
                }
                self.pos = q + 1;
            }
            b'(' if self.at(self.pos + 1) == b'*' => {
                let mut q = self.pos + 2;
                loop {
                    let star = (q..self.source.len()).find(|&i| self.source[i] == b'*');
                    let Some(star) = star else {
                        return Err(Error::new(IE_BAD_REMARK, start as i64, "", ""));
                    };
                    if self.at(star + 1) == b')' {
                        self.pos = star + 2;
                        break;
                    }
                    q = star + 1;
                }
            }
            b'}' => return Err(Error::new(IE_BAD_REMARK, start as i64, "", "")),
            b'*' if self.at(self.pos + 1) == b')' => {
                return Err(Error::new(IE_BAD_REMARK, start as i64, "", ""));
            }
            b'/' if self.at(self.pos + 1) == b'/' => {
                while !matches!(self.at(self.pos), b'\n' | b'\r' | 0) {
                    self.pos += 1;
                }
            }
            _ => {}
        }
        while matches!(self.at(self.pos), b' ' | b'\n' | b'\r' | b'\t') {
            self.pos += 1;
        }
        Ok(())
    }

    /// `TJvInterpreterParser.Token` (`:599`).
    pub fn token(&mut self) -> Result<RawToken, Error> {
        let mut point_occurred = false;
        let mut exponent_occurred = false;

        let prev_point = self.pos > 0 && self.source[self.pos - 1] == b'.';

        // Firstly skip spaces and remarks.
        loop {
            let before = self.pos;
            self.skip()?;
            if self.pos == before {
                break;
            }
        }

        let f = self.pos;
        let make = |text: Vec<u8>, end: usize| -> Result<RawToken, Error> { Ok(RawToken { text, start: f, end }) };

        if is_id_first_symbol(self.at(f)) || (prev_point && is_char_alpha(self.at(f))) {
            // identifier
            let mut q = f;
            while is_id_symbol(self.at(q)) || (prev_point && is_char_alpha(self.at(q))) {
                q += 1;
            }
            self.pos = q;
            make(self.source[f..q].to_vec(), q)
        } else if is_const_symbol_10(self.at(f)) {
            // number
            let mut q = f;
            while is_const_symbol_10_e(self.at(q)) || self.at(q) == b'.' {
                let c = self.at(q);
                if c == b'.' {
                    if point_occurred || !is_const_symbol_10(self.at(q - 1)) || self.at(q + 1) == b'.' {
                        break;
                    }
                    point_occurred = true;
                } else if is_e(c) {
                    if exponent_occurred || !is_const_symbol_10(self.at(q - 1)) {
                        break;
                    }
                    exponent_occurred = true;
                } else if is_plus_sub(c) && !is_e(self.at(q - 1)) {
                    break;
                }
                q += 1;
            }
            self.pos = q;
            make(self.source[f..q].to_vec(), q)
        } else if self.at(f) == b'$' && is_const_symbol(self.at(f + 1)) {
            // hex number
            let mut q = f + 1;
            while is_const_symbol(self.at(q)) {
                q += 1;
            }
            self.pos = q;
            make(self.source[f..q].to_vec(), q)
        } else if self.at(f) == b'\'' {
            // string constant
            let mut q = f + 1;
            loop {
                let c = self.at(q);
                if matches!(c, b'\n' | b'\r' | 0) {
                    break;
                }
                if c == b'\'' {
                    if self.at(q + 1) == b'\'' {
                        q += 1;
                    } else {
                        break;
                    }
                }
                q += 1;
            }
            // `Inc(P)` after the loop; at the end of the source the virtual
            // `#0` is consumed, not a source byte (`:735`).
            q += 1;
            let end = q.min(self.source.len());
            self.pos = end;
            let mut text = self.source[f..end].to_vec();
            // Delete the doubled quotes, skipping the character after each
            // deletion (`while I < Length(Result) - 1`, `:737`).
            let mut i = 1usize;
            while (i as i64) < (text.len() as i64 - 1) {
                if text[i] == b'\'' {
                    text.remove(i);
                }
                i += 1;
            }
            make(text, end)
        } else if self.at(f) == b'#' && is_const_symbol_10(self.at(f + 1)) {
            // Char constant: `#nnn` becomes the quoted form `'<char>'` (`:746`).
            let mut q = f + 1;
            while is_const_symbol_10(self.at(q)) {
                q += 1;
            }
            let digits = &self.source[f + 1..q];
            self.pos = q;
            let value: i64 = std::str::from_utf8(digits)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let mut text = vec![b'\''];
            if (0..=255).contains(&value) {
                text.push(value as u8);
            } else if let Some(c) = char::from_u32(value as u32) {
                // `Chr` of a value out of the ANSI range is the wide character.
                let mut buf = [0u8; 4];
                text.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
            text.push(b'\'');
            make(text, q)
        } else if matches!(self.at(f), b'>' | b'=' | b'<' | b'.') {
            let two = |a: u8, b: u8| vec![a, b];
            if self.at(f) == b'.' && self.at(f + 1) == b'.' {
                self.pos = f + 2;
                make(two(b'.', b'.'), f + 2)
            } else if self.at(f) == b'>' && self.at(f + 1) == b'=' {
                self.pos = f + 2;
                make(two(b'>', b'='), f + 2)
            } else if self.at(f) == b'<' && self.at(f + 1) == b'=' {
                self.pos = f + 2;
                make(two(b'<', b'='), f + 2)
            } else if self.at(f) == b'<' && self.at(f + 1) == b'>' {
                self.pos = f + 2;
                make(two(b'<', b'>'), f + 2)
            } else {
                self.pos = f + 1;
                make(vec![self.at(f)], f + 1)
            }
        } else if self.at(f) == 0 {
            make(Vec::new(), f)
        } else {
            self.pos = f + 1;
            make(vec![self.at(f)], f + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<(TTokenKind, String)> {
        let mut tokenizer = Tokenizer::new(source.as_bytes());
        let mut out = Vec::new();
        loop {
            let token = tokenizer.token().unwrap();
            let kind = token_kind(&token.text);
            if kind == TT_EMPTY {
                break;
            }
            out.push((kind, String::from_utf8_lossy(&token.text).into_owned()));
        }
        out
    }

    #[test]
    fn keywords_use_the_hash_table() {
        for (text, kind) in [
            (&b"begin"[..], TT_BEGIN),
            (b"end", TT_END),
            (b"div", TT_INT_DIV),
            (b"implementation", TT_IMPLEMENTATION),
            (b"downto", TT_DOWNTO),
            (b"xor", TT_XOR),
            (b"true", TT_TRUE),
            (b"record", TT_RECORD),
            (b"shl", TT_SHL),
        ] {
            assert_eq!(token_kind(text), kind, "{}", String::from_utf8_lossy(text));
        }
        // The keyword table is case-insensitive (Cmp).
        assert_eq!(token_kind(b"BEGIN"), TT_BEGIN);
        assert_eq!(token_kind(b"Begin"), TT_BEGIN);
        // `implementation` is 14 characters, the longest keyword.
        assert_eq!(token_kind(b"implementations"), TT_IDENTIFIER);
        assert_eq!(token_kind(b"i"), TT_IDENTIFIER);
    }

    #[test]
    fn numbers() {
        assert_eq!(token_kind(b"123"), TT_INTEGER);
        assert_eq!(token_kind(b"$FF"), TT_INTEGER);
        assert_eq!(token_kind(b"$ff"), TT_INTEGER);
        assert_eq!(token_kind(b"$G1"), TT_UNKNOWN); // '$' then 'G1'
        assert_eq!(token_kind(b"1.5"), TT_DOUBLE);
        assert_eq!(token_kind(b"1."), TT_DOUBLE);
        assert_eq!(token_kind(b"1e5"), TT_DOUBLE);
        assert_eq!(token_kind(b"1E+5"), TT_DOUBLE);
        assert_eq!(token_kind(b"1e-5"), TT_DOUBLE);
        assert_eq!(token_kind(b"1E5"), TT_DOUBLE);
        assert_eq!(token_kind(b"12"), TT_INTEGER);
        // The multi-character path admits +/- as number characters (`:509`).
        assert_eq!(token_kind(b"+1"), TT_INTEGER);
        assert_eq!(token_kind(b"-1"), TT_INTEGER);
        assert_eq!(token_kind(b"."), TT_POINT);
        assert_eq!(token_kind(b".."), TT_DOUBLE_POINT);
        assert_eq!(token_kind(b">="), TT_EQU_GREATER);
        assert_eq!(token_kind(b"<="), TT_EQU_LESS);
        assert_eq!(token_kind(b"<>"), TT_NOT_EQU);
        assert_eq!(token_kind(b"<"), TT_LESS);
        assert_eq!(token_kind(b">"), TT_GREATER);
        assert_eq!(token_kind(b"="), TT_EQU);
        assert_eq!(token_kind(b"("), TT_LB);
        assert_eq!(token_kind(b")"), TT_RB);
        assert_eq!(token_kind(b"["), TT_LS);
        assert_eq!(token_kind(b"]"), TT_RS);
        assert_eq!(token_kind(b","), TT_COL);
        assert_eq!(token_kind(b":"), TT_COLON);
        assert_eq!(token_kind(b";"), TT_SEMICOLON);
        assert_eq!(token_kind(b"+"), TT_PLUS);
        assert_eq!(token_kind(b"-"), TT_MINUS);
        assert_eq!(token_kind(b"*"), TT_MUL);
        assert_eq!(token_kind(b"/"), TT_DIV);
        assert_eq!(token_kind(b"!"), TT_UNKNOWN);
        assert_eq!(token_kind(b"@"), TT_UNKNOWN);
        assert_eq!(token_kind(b""), TT_EMPTY);
    }

    #[test]
    fn tokenizer_splits_numbers_by_radix_rules() {
        // The radix point must follow a digit and not start a '..'.
        assert_eq!(
            kinds("1..5"),
            vec![
                (TT_INTEGER, "1".to_owned()),
                (TT_DOUBLE_POINT, "..".to_owned()),
                (TT_INTEGER, "5".to_owned())
            ]
        );
        // At most one radix point and one exponent.
        assert_eq!(
            kinds("1.2.3"),
            vec![
                (TT_DOUBLE, "1.2".to_owned()),
                (TT_POINT, ".".to_owned()),
                (TT_INTEGER, "3".to_owned())
            ]
        );
        assert_eq!(
            kinds("1e5e5"),
            vec![(TT_DOUBLE, "1e5".to_owned()), (TT_IDENTIFIER, "e5".to_owned())]
        );
        // '+' only continues behind an E.
        assert_eq!(
            kinds("1+2"),
            vec![
                (TT_INTEGER, "1".to_owned()),
                (TT_PLUS, "+".to_owned()),
                (TT_INTEGER, "2".to_owned())
            ]
        );
    }

    #[test]
    fn string_escapes() {
        let mut tokenizer = Tokenizer::new(b"'It''s'");
        let token = tokenizer.token().unwrap();
        assert_eq!(token_kind(&token.text), TT_STRING);
        // The doubled quote is collapsed by Token itself.
        assert_eq!(token.text, b"'It's'");

        // Two quotes: the empty string.
        let mut tokenizer = Tokenizer::new(b"''");
        let token = tokenizer.token().unwrap();
        assert_eq!(token_kind(&token.text), TT_STRING);
        assert_eq!(token.text, b"''");

        // Four quotes: one escaped quote.
        let mut tokenizer = Tokenizer::new(b"''''");
        let token = tokenizer.token().unwrap();
        assert_eq!(token_kind(&token.text), TT_STRING);
        assert_eq!(token.text, b"'''");

        // A doubled quote pair in the middle.
        let mut tokenizer = Tokenizer::new(b"'a''b'");
        let token = tokenizer.token().unwrap();
        assert_eq!(token.text, b"'a'b'");
    }

    #[test]
    fn character_constants_become_quoted_strings() {
        let mut tokenizer = Tokenizer::new(b"#65");
        let token = tokenizer.token().unwrap();
        assert_eq!(token.text, b"'A'");
        assert_eq!(token_kind(&token.text), TT_STRING);

        let mut tokenizer = Tokenizer::new(b"#13#10");
        let first = tokenizer.token().unwrap();
        assert_eq!(first.text, b"'\r'");
        let second = tokenizer.token().unwrap();
        assert_eq!(second.text, b"'\n'");
    }

    #[test]
    fn comments_and_bad_remarks() {
        let mut tokenizer = Tokenizer::new(b"{ a } (* b *) // c\n x");
        let token = tokenizer.token().unwrap();
        assert_eq!(token.text, b"x");

        // A bad remark is ieBadRemark at the remark's position.
        let mut tokenizer = Tokenizer::new(b"{ unterminated");
        assert_eq!(tokenizer.token().unwrap_err().code, IE_BAD_REMARK);
        let mut tokenizer = Tokenizer::new(b"1 } 2");
        let token = tokenizer.token().unwrap();
        assert_eq!(token.text, b"1");
        let error = tokenizer.token().unwrap_err();
        assert_eq!(error.code, IE_BAD_REMARK);
        assert_eq!(error.pos, 2);
        let mut tokenizer = Tokenizer::new(b"1 *) 2");
        let token = tokenizer.token().unwrap();
        assert_eq!(token.text, b"1");
        assert_eq!(tokenizer.token().unwrap_err().code, IE_BAD_REMARK);
        let mut tokenizer = Tokenizer::new(b"(* open");
        assert_eq!(tokenizer.token().unwrap_err().code, IE_BAD_REMARK);
    }

    #[test]
    fn national_symbols_after_a_point() {
        // IsCharAlpha over CP1252: 0xE9 (e acute) is a letter.
        let mut tokenizer = Tokenizer::new(b"var.\xE9");
        let first = tokenizer.token().unwrap();
        assert_eq!(first.text, b"var");
        let second = tokenizer.token().unwrap();
        assert_eq!(second.text, b".");
        let third = tokenizer.token().unwrap();
        assert_eq!(third.text, b"\xE9");
        assert_eq!(token_kind(&third.text), TT_IDENTIFIER);
    }

    #[test]
    fn prior_table() {
        assert_eq!(prior(TT_NOT), 8);
        assert_eq!(prior(TT_MUL), 6);
        assert_eq!(prior(TT_SHL), 6);
        assert_eq!(prior(TT_AND), 5);
        assert_eq!(prior(TT_PLUS), 4);
        assert_eq!(prior(TT_EQU), 3);
        assert_eq!(prior(TT_XOR), 3);
        assert_eq!(prior(TT_EQU_GREATER), 2);
        assert_eq!(prior(TT_EQU_LESS), 2);
        assert_eq!(prior(TT_SEMICOLON), 0);
        assert_eq!(prior(TT_IDENTIFIER), 0);
    }
}
