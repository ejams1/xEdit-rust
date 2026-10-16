// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What Sniff's processors use of Delphi's `TPerlRegEx`
//! (`System.RegularExpressionsCore`): a case-insensitive expression,
//! `ReplaceAll` and the replacement text with its backreferences
//! (`ComputeReplacement`).
//!
//! The class is [`xedit_core::perl_regex::PerlRegEx`], the same one the
//! script adapter and the filter run, over the port's PCRE engine
//! (`xedit_core::pcre`): upstream runs PCRE 8 through the RTL, so
//! look-around, in-pattern backreferences, possessive quantifiers, ASCII
//! `\d`/`\w`/`\s` and the byte offsets of a multi-byte subject behave as
//! they do in the release.

use xedit_core::perl_regex::{OPTION_CASE_LESS, PerlRegEx as CorePerlRegEx};

use crate::data_format::{DfError, R};

/// `TPerlRegEx` with `preCaseLess`: `ProcReplaceAssets` and
/// `ProcUniversalTweaker` both set that one option.
pub struct PerlRegEx {
    core: CorePerlRegEx,
}

impl PerlRegEx {
    /// `RegEx := aExpression; Study`: the expression is compiled here, so a
    /// pattern the engine refuses fails the operation that builds it.
    pub fn new(expression: &str) -> R<PerlRegEx> {
        let mut core = CorePerlRegEx::new();
        core.set_regex(expression.to_owned());
        core.set_options(1 << OPTION_CASE_LESS);
        core.compile().map_err(|error| DfError::new(error.message()))?;
        Ok(PerlRegEx { core })
    }

    /// `Subject := subject; Replacement := replacement; ReplaceAll`:
    /// whether the expression matched, and the subject after the
    /// replacements.
    pub fn replace_all(&self, subject: &str, replacement: &str) -> (bool, String) {
        // The class is cheap to clone: the compiled expression is shared.
        let mut core = self.core.clone();
        core.set_subject(subject.to_owned());
        core.set_replacement(replacement.to_owned());
        match core.replace_all() {
            Ok(matched) => (matched, core.subject().to_owned()),
            // `ProcReplaceAssets` and `ProcUniversalTweaker` never let a
            // `TPerlRegEx` raise here: the expression is compiled when the
            // object is built.
            Err(_) => (false, subject.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacements() {
        let re = PerlRegEx::new("(.+)\\.dds").unwrap();
        assert_eq!(
            re.replace_all("Tex\\A.DDS", "$1_n.dds"),
            (true, "Tex\\A_n.dds".to_owned())
        );
        let re = PerlRegEx::new("^\\s*|\\s*$").unwrap();
        assert_eq!(re.replace_all("  name ", ""), (true, "name".to_owned()));
        let re = PerlRegEx::new("b").unwrap();
        assert_eq!(re.replace_all("abc", "[$&\\$$`$']$"), (true, "a[b$ac]$c".to_owned()));
        assert!(!re.replace_all("xyz", "q").0);
        let re = PerlRegEx::new("(?P<x>b)(c)").unwrap();
        assert_eq!(re.replace_all("abcd", "${x}\\2\\u1"), (true, "abcBd".to_owned()));
    }

    /// The PCRE syntax the `regex` crate refused before this change.
    #[test]
    fn pcre_syntax() {
        let re = PerlRegEx::new("(?<=a)b(?=c)").unwrap();
        assert_eq!(re.replace_all("abc", "!"), (true, "a!c".to_owned()));
        let re = PerlRegEx::new("(a)\\1").unwrap();
        assert_eq!(re.replace_all("aa ab", "!"), (true, "! ab".to_owned()));
        let re = PerlRegEx::new("a++b").unwrap();
        assert_eq!(re.replace_all("aab", "!"), (true, "!".to_owned()));
    }

    #[test]
    fn compile_errors_are_the_rtl_ones() {
        let error = PerlRegEx::new("(?<=a+)b").err().expect("a compile error");
        assert_eq!(
            error.to_string(),
            "Error in regular expression at offset 6: lookbehind assertion is not fixed length"
        );
        let error = PerlRegEx::new("").err().expect("an empty expression");
        assert_eq!(
            error.to_string(),
            "A regular expression specified in RegEx is required"
        );
    }
}
