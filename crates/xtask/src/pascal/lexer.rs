// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tokens of Object Pascal source, with conditional compilation applied.

use std::collections::BTreeSet;

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// An identifier or a reserved word. Pascal identifiers ignore case; use
    /// [`Token::is_word`] to compare.
    Ident(String),
    /// An integer literal as written, without a leading `$` for hexadecimal.
    Int {
        digits: String,
        hex: bool,
    },
    Float(String),
    /// A string literal with quotes and `#nn` character codes resolved.
    Str(String),
    /// An operator or punctuation: `:=`, `<>`, `<=`, `>=`, `..` or one character.
    Symbol(&'static str),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// 1-based line of the first character.
    pub line: u32,
}

impl Token {
    /// Whether the token is the identifier or reserved word `word`, ignoring case.
    pub fn is_word(&self, word: &str) -> bool {
        matches!(&self.kind, TokenKind::Ident(name) if name.eq_ignore_ascii_case(word))
    }

    pub fn is_symbol(&self, symbol: &str) -> bool {
        matches!(&self.kind, TokenKind::Symbol(s) if *s == symbol)
    }

    pub fn ident(&self) -> Option<&str> {
        match &self.kind {
            TokenKind::Ident(name) => Some(name),
            _ => None,
        }
    }
}

const SYMBOLS: &[&str] = &[
    ":=", "<>", "<=", ">=", "..", "(", ")", "[", "]", ",", ";", ":", ".", "=", "<", ">", "+", "-", "*", "/", "@", "^",
];

/// One level of `{$IFDEF}` nesting.
struct Condition {
    /// The enclosing levels are all active.
    parent_active: bool,
    /// A branch of this level was active already.
    taken: bool,
    active: bool,
}

/// Splits `source` into tokens. `defines` are the conditional symbols that are
/// set; text in inactive `{$IFDEF}` branches is dropped. A `{$IF}` expression
/// counts as false.
pub fn tokenize(source: &str, defines: &[&str]) -> Result<Vec<Token>> {
    let defines: BTreeSet<String> = defines.iter().map(|d| d.to_ascii_uppercase()).collect();
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut conditions: Vec<Condition> = Vec::new();
    let mut i = 0;
    let mut line: u32 = 1;
    let active = |conditions: &[Condition]| conditions.last().is_none_or(|c| c.active);

    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() || c == '\u{feff}' {
            i += 1;
            continue;
        }
        // Comments and compiler directives.
        let comment_end = if c == '{' {
            Some(("}", 1))
        } else if c == '(' && chars.get(i + 1) == Some(&'*') {
            Some(("*)", 2))
        } else {
            None
        };
        if let Some((close, open_len)) = comment_end {
            let start = i + open_len;
            let close: Vec<char> = close.chars().collect();
            let mut end = start;
            while end < chars.len() && !chars[end..].starts_with(&close) {
                end += 1;
            }
            if end >= chars.len() {
                bail!("line {line}: comment is not closed");
            }
            let body: String = chars[start..end].iter().collect();
            if let Some(directive) = body.strip_prefix('$') {
                apply_directive(directive, &defines, &mut conditions, line)?;
            }
            line += body.matches('\n').count() as u32;
            i = end + close.len();
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if !active(&conditions) {
            // Skip one character of inactive text. A quote does not start a string
            // here, because inactive branches hold assembler and other dialects.
            i += 1;
            continue;
        }

        let token_line = line;
        let kind = if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            TokenKind::Ident(chars[start..i].iter().collect())
        } else if c == '&' && chars.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic()) {
            // An escaped identifier such as &type. It keeps its `&`, so that it does
            // not compare equal to the reserved word.
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            TokenKind::Ident(chars[start..i].iter().collect())
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let mut float = false;
            if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
                float = true;
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if matches!(chars.get(i), Some('e' | 'E')) {
                let mut j = i + 1;
                if matches!(chars.get(j), Some('+' | '-')) {
                    j += 1;
                }
                if chars.get(j).is_some_and(char::is_ascii_digit) {
                    float = true;
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = chars[start..i].iter().collect();
            if float {
                TokenKind::Float(text)
            } else {
                TokenKind::Int {
                    digits: text,
                    hex: false,
                }
            }
        } else if c == '$' {
            i += 1;
            let start = i;
            while i < chars.len() && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
            TokenKind::Int {
                digits: chars[start..i].iter().collect(),
                hex: true,
            }
        } else if c == '\'' || c == '#' {
            // A string: quoted parts and #nn character codes that follow each other.
            let mut text = String::new();
            loop {
                match chars.get(i) {
                    Some('\'') => {
                        i += 1;
                        loop {
                            match chars.get(i) {
                                Some('\'') if chars.get(i + 1) == Some(&'\'') => {
                                    text.push('\'');
                                    i += 2;
                                }
                                Some('\'') => {
                                    i += 1;
                                    break;
                                }
                                Some('\n') | None => bail!("line {line}: string is not closed"),
                                Some(&other) => {
                                    text.push(other);
                                    i += 1;
                                }
                            }
                        }
                    }
                    Some('#') => {
                        i += 1;
                        let hex = chars.get(i) == Some(&'$');
                        if hex {
                            i += 1;
                        }
                        let start = i;
                        while i < chars.len() && chars[i].is_ascii_hexdigit() && (hex || chars[i].is_ascii_digit()) {
                            i += 1;
                        }
                        let digits: String = chars[start..i].iter().collect();
                        let code = u32::from_str_radix(&digits, if hex { 16 } else { 10 })
                            .ok()
                            .and_then(char::from_u32);
                        match code {
                            Some(code) => text.push(code),
                            None => bail!("line {line}: invalid character code #{digits}"),
                        }
                    }
                    _ => break,
                }
            }
            TokenKind::Str(text)
        } else {
            let rest = &chars[i..];
            let symbol = SYMBOLS
                .iter()
                .find(|symbol| rest.starts_with(&symbol.chars().collect::<Vec<_>>()));
            match symbol {
                Some(symbol) => {
                    i += symbol.len();
                    TokenKind::Symbol(symbol)
                }
                None => bail!("line {line}: unexpected character {c:?}"),
            }
        };
        tokens.push(Token { kind, line: token_line });
    }
    if !conditions.is_empty() {
        bail!("conditional compilation is not closed at the end of the file");
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        line,
    });
    Ok(tokens)
}

fn apply_directive(
    directive: &str,
    defines: &BTreeSet<String>,
    conditions: &mut Vec<Condition>,
    line: u32,
) -> Result<()> {
    let mut words = directive.split_whitespace();
    let name = words.next().unwrap_or_default().to_ascii_uppercase();
    let symbol = words.next().unwrap_or_default().to_ascii_uppercase();
    let parent_active = conditions.last().is_none_or(|c| c.active);
    let mut open = |condition: bool| {
        let active = parent_active && condition;
        conditions.push(Condition {
            parent_active,
            taken: active,
            active,
        });
    };
    match name.as_str() {
        "IFDEF" => open(defines.contains(&symbol)),
        "IFNDEF" => open(!defines.contains(&symbol)),
        "IFOPT" | "IF" => open(false),
        "ELSE" | "ELSEIF" => {
            let Some(condition) = conditions.last_mut() else {
                bail!("line {line}: {{$ELSE}} without {{$IFDEF}}");
            };
            // An ELSEIF expression counts as false, like IF.
            condition.active = name == "ELSE" && condition.parent_active && !condition.taken;
            condition.taken |= condition.active;
        }
        "ENDIF" | "IFEND" if conditions.pop().is_none() => {
            bail!("line {line}: {{$ENDIF}} without {{$IFDEF}}");
        }
        "ENDIF" | "IFEND" => {}
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        tokenize(source, &["WIN64"])
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    fn ident(name: &str) -> TokenKind {
        TokenKind::Ident(name.to_owned())
    }

    #[test]
    fn literals_and_symbols() {
        assert_eq!(
            kinds("x := $1F + 10 * 2.5e3;"),
            vec![
                ident("x"),
                TokenKind::Symbol(":="),
                TokenKind::Int {
                    digits: "1F".to_owned(),
                    hex: true
                },
                TokenKind::Symbol("+"),
                TokenKind::Int {
                    digits: "10".to_owned(),
                    hex: false
                },
                TokenKind::Symbol("*"),
                TokenKind::Float("2.5e3".to_owned()),
                TokenKind::Symbol(";"),
                TokenKind::Eof,
            ]
        );
        // A range is not a float.
        assert_eq!(kinds("0..3")[1], TokenKind::Symbol(".."));
    }

    #[test]
    fn strings() {
        assert_eq!(kinds("'it''s'")[0], TokenKind::Str("it's".to_owned()));
        assert_eq!(kinds("'a'#13#10'b'")[0], TokenKind::Str("a\r\nb".to_owned()));
        assert_eq!(kinds("#$41")[0], TokenKind::Str("A".to_owned()));
        assert_eq!(kinds("''")[0], TokenKind::Str(String::new()));
        let escaped = tokenize("&type", &[]).unwrap();
        assert!(!escaped[0].is_word("type") && escaped[0].ident() == Some("&type"));
        assert!(tokenize("'open", &[]).is_err());
    }

    #[test]
    fn comments_and_lines() {
        let tokens = tokenize("a { one\n two } b // rest\n(* x *) c", &[]).unwrap();
        let lines: Vec<u32> = tokens.iter().map(|token| token.line).collect();
        assert_eq!(tokens.len(), 4);
        assert_eq!(lines, [1, 2, 3, 3]);
        assert!(tokens[2].is_word("C") && tokens[1].ident() == Some("b"));
    }

    #[test]
    fn conditional_compilation() {
        let source = "{$IFDEF WIN32} a {$ELSE} b {$ENDIF} {$IFNDEF WIN64} c {$ENDIF} {$IFDEF WIN64} d {$IFDEF X} e \
                      {$ELSE} f {$ENDIF} {$ENDIF} {$IF CompilerVersion >= 24} g {$ELSE} h {$IFEND}";
        assert_eq!(
            kinds(source),
            vec![ident("b"), ident("d"), ident("f"), ident("h"), TokenKind::Eof]
        );
        assert!(tokenize("{$IFDEF A} x", &[]).is_err());
        assert!(tokenize("{$ENDIF}", &[]).is_err());
        // Inactive text is not tokenized.
        assert_eq!(
            kinds("{$IFDEF NO} don't ` {$ENDIF} ok"),
            vec![ident("ok"), TokenKind::Eof]
        );
    }
}
