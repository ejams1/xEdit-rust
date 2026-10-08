// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What Sniff's processors use of Delphi's `TPerlRegEx`
//! (`System.RegularExpressionsCore`): a case-insensitive expression,
//! `ReplaceAll` and the replacement text with its backreferences
//! (`ComputeReplacement`).
//!
//! UPSTREAM-QUIRK: upstream runs PCRE; the port runs the `regex` crate,
//! which has the same syntax for everything but backreferences inside the
//! expression and look-around, which it refuses with an error. `\d`, `\w`
//! and `\s` match Unicode characters here and ASCII ones in PCRE.

use regex::{Captures, Regex, RegexBuilder};

use crate::data_format::{DfError, R};

/// `TPerlRegEx` with `preCaseLess`.
pub struct PerlRegEx {
    regex: Regex,
}

impl PerlRegEx {
    /// `RegEx := aExpression; Study`.
    pub fn new(expression: &str) -> R<PerlRegEx> {
        if expression.is_empty() {
            return Err(DfError::new(
                "TPerlRegEx.Compile() - Please specify a regular expression in RegEx first",
            ));
        }
        let regex = RegexBuilder::new(expression)
            .case_insensitive(true)
            .build()
            .map_err(|error| DfError::new(format!("Error in regular expression: {error}")))?;
        Ok(PerlRegEx { regex })
    }

    /// `Subject := subject; Replacement := replacement; ReplaceAll`:
    /// whether the expression matched, and the subject after the
    /// replacements.
    pub fn replace_all(&self, subject: &str, replacement: &str) -> (bool, String) {
        let mut matched = false;
        let mut result = String::with_capacity(subject.len());
        let mut last = 0;
        for captures in self.regex.captures_iter(subject) {
            matched = true;
            let whole = captures.get(0).expect("group 0");
            result.push_str(&subject[last..whole.start()]);
            result.push_str(&self.compute_replacement(subject, &captures, replacement));
            last = whole.end();
        }
        result.push_str(&subject[last..]);
        (matched, result)
    }

    /// `ComputeReplacement`: `\n`, `$n` and `${n}` (one or two digits, the
    /// second only when the group exists), `${name}`, `\g<name>`, `$&`,
    /// `$+`, `` $` ``, `$'`, `$_`, `\\` and `\$`, and `\l`, `\u`, `\f` with
    /// a group number for lower, upper and first upper case.
    fn compute_replacement(&self, subject: &str, captures: &Captures, replacement: &str) -> String {
        let group_count = captures.len() as i32 - 1;
        let whole = captures.get(0).expect("group 0");
        let group = |number: i32| -> String {
            if number < 0 || number > group_count {
                return String::new();
            }
            captures
                .get(number as usize)
                .map_or_else(String::new, |found| found.as_str().to_owned())
        };
        let named = |name: &str| -> i32 {
            self.regex
                .capture_names()
                .position(|candidate| candidate == Some(name))
                .map_or(-1, |index| index as i32)
        };
        let s: Vec<char> = replacement.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        // Upstream stops one character before the end: a final `\` or `$`
        // is plain text.
        while i + 1 < s.len() {
            let c = s[i];
            if c != '\\' && c != '$' {
                out.push(c);
                i += 1;
                continue;
            }
            let dollar = c == '$';
            let mut j = i + 1;
            let mut mode = '\0';
            let mut number_only = false;
            if !dollar {
                match s[j] {
                    '$' | '\\' => {
                        out.push(s[j]);
                        i = j + 1;
                        continue;
                    }
                    'g' => {
                        if j + 2 < s.len() && s[j + 1] == '<' && (s[j + 2].is_ascii_alphabetic() || s[j + 2] == '_') {
                            let mut k = j + 3;
                            while k < s.len() && (s[k].is_ascii_alphanumeric() || s[k] == '_') {
                                k += 1;
                            }
                            if k < s.len() && s[k] == '>' {
                                let name: String = s[j + 2..k].iter().collect();
                                let n = named(&name);
                                if n > 0 {
                                    out.push_str(&group(n));
                                }
                                i = k + 1;
                            } else {
                                out.extend(&s[i..k]);
                                i = k;
                            }
                        } else {
                            out.extend(&s[i..i + 2]);
                            i += 2;
                        }
                        continue;
                    }
                    'l' | 'L' | 'u' | 'U' | 'f' | 'F' => {
                        mode = s[j];
                        j += 1;
                        number_only = true;
                    }
                    _ => {}
                }
            }
            // ProcessBackreference
            let mut number: i32 = -1;
            if j < s.len() && s[j].is_ascii_digit() {
                number = s[j] as i32 - '0' as i32;
                j += 1;
                if j < s.len() && s[j].is_ascii_digit() {
                    let two = number * 10 + (s[j] as i32 - '0' as i32);
                    if two <= group_count {
                        number = two;
                        j += 1;
                    }
                }
            } else if !number_only && j < s.len() {
                if dollar && j + 1 < s.len() && s[j] == '{' {
                    j += 1;
                    if s[j].is_ascii_digit() {
                        number = 0;
                        while j < s.len() && s[j].is_ascii_digit() {
                            number = number.saturating_mul(10).saturating_add(s[j] as i32 - '0' as i32);
                            j += 1;
                        }
                    } else if s[j].is_ascii_alphabetic() || s[j] == '_' {
                        let start = j;
                        j += 1;
                        while j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == '_') {
                            j += 1;
                        }
                        if j < s.len() && s[j] == '}' {
                            let name: String = s[start..j].iter().collect();
                            number = named(&name);
                        }
                    }
                    if j >= s.len() || s[j] != '}' {
                        number = -1;
                    } else {
                        j += 1;
                    }
                } else if dollar && s[j] == '_' {
                    out.push_str(subject);
                    i = j + 1;
                    continue;
                } else {
                    match s[j] {
                        '&' => {
                            number = 0;
                            j += 1;
                        }
                        '+' => {
                            number = group_count;
                            j += 1;
                        }
                        '`' => {
                            out.push_str(&subject[..whole.start()]);
                            i = j + 1;
                            continue;
                        }
                        '\'' => {
                            out.push_str(&subject[whole.end()..]);
                            i = j + 1;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            if number >= 0 {
                let value = group(number);
                out.push_str(&match mode {
                    'L' | 'l' => value.to_lowercase(),
                    'U' | 'u' => value.to_uppercase(),
                    'F' | 'f' => {
                        let mut chars = value.chars();
                        match chars.next() {
                            Some(first) => first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
                            None => String::new(),
                        }
                    }
                    _ => value,
                });
                i = j;
            } else {
                // Not a reference: the character stays.
                out.push(c);
                i += 1;
            }
        }
        out.extend(&s[i.min(s.len())..]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacements() {
        let re = PerlRegEx::new("(.+)\\.dds").unwrap();
        assert_eq!(re.replace_all("Tex\\A.DDS", "$1_n.dds"), (true, "Tex\\A_n.dds".to_owned()));
        let re = PerlRegEx::new("^\\s*|\\s*$").unwrap();
        assert_eq!(re.replace_all("  name ", ""), (true, "name".to_owned()));
        let re = PerlRegEx::new("b").unwrap();
        assert_eq!(re.replace_all("abc", "[$&\\$$`$']$"), (true, "a[b$ac]$c".to_owned()));
        assert_eq!(re.replace_all("xyz", "q").0, false);
        let re = PerlRegEx::new("(?P<x>b)(c)").unwrap();
        assert_eq!(re.replace_all("abcd", "${x}\\2\\u1"), (true, "abcBd".to_owned()));
    }
}
