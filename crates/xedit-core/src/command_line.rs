// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbCommandLine.pas

//! The command line of xEdit (`wbCommandLine.pas`): the parameters a program
//! was started with, found by the name of a switch or read one by one.
//!
//! The parameters are the ones after the program name, as Delphi's
//! `ParamStr(1)` to `ParamStr(ParamCount)` are (Delphi's `ParamStr(0)` is
//! the program itself and is not one of them). `SwitchChars` (from
//! `System.SysUtils`, the unit `wbCommandLine` uses) says which characters
//! start a switch: `-` and `/`. A switch written without a value gives an
//! empty value, so the result of a find is `Some(String::new())` then.
//!
//! Every index the `var aStartIndex` overloads carry is 1-based like
//! Delphi's `ParamStr`, so the upstream arithmetic maps onto it line by
//! line.

/// Upstream `SwitchChars` (`System.SysUtils`): the characters a switch
/// starts with. A Windows path never starts with `/` in the places xEdit
/// reads one, so both are used.
pub const SWITCH_CHARS: [char; 2] = ['-', '/'];

/// The first character of a parameter, and its length in bytes.
fn first_char(text: &str) -> Option<(char, usize)> {
    text.chars().next().map(|c| (c, c.len_utf8()))
}

/// Port of `wbFindCmdLineParam(aSwitch, aChars, aIgnoreCase, out aValue)`:
/// the value of the first parameter that is the switch `aSwitch` (with a
/// colon and a value) or the switch alone, or `None` when no parameter is
/// it. A parameter that does not start with one of `aChars` is not looked
/// at; an empty `aChars` means any parameter is.
pub fn find_cmd_line_param(params: &[String], a_switch: &str, a_chars: &[char], a_ignore_case: bool) -> Option<String> {
    for param in params {
        let Some((first, length)) = first_char(param) else {
            // Delphi's `s[1]` of an empty string is #0, which is never a
            // switch character.
            continue;
        };
        if !(a_chars.is_empty() || a_chars.contains(&first)) {
            continue;
        }
        // `Delete(s, 1, 1)`.
        let s = &param[length..];
        let with_value = format!("{a_switch}:");
        if starts_with(s, &with_value, a_ignore_case) {
            return Some(s[with_value.len()..].to_owned());
        }
        if if a_ignore_case {
            s.eq_ignore_ascii_case(a_switch)
        } else {
            s == a_switch
        } {
            return Some(String::new());
        }
    }
    None
}

/// `s.StartsWith(aPrefix, aIgnoreCase)` for ASCII switch names.
fn starts_with(s: &str, a_prefix: &str, a_ignore_case: bool) -> bool {
    let head = s.get(..a_prefix.len());
    match (head, a_ignore_case) {
        (Some(head), true) => head.eq_ignore_ascii_case(a_prefix),
        (Some(head), false) => head == a_prefix,
        (None, _) => false,
    }
}

/// Port of `wbFindCmdLineParam(var aStartIndex, aChars, out aValue)`: reads
/// the parameters one by one from `a_start_index` (1-based, as Delphi's
/// `ParamStr`), skipping the ones that start with a switch character.
/// `a_start_index` moves past what was read, as upstream changes it.
pub fn find_cmd_line_param_at(params: &[String], a_start_index: &mut usize, a_chars: &[char]) -> Option<String> {
    // `for i := aStartIndex to ParamCount do`
    let mut i = *a_start_index;
    while i >= 1 && i <= params.len() {
        let param = &params[i - 1];
        let is_switch = first_char(param).is_some_and(|(first, _)| a_chars.contains(&first));
        if a_chars.is_empty() || is_switch {
            // skipped
            *a_start_index += 1;
        } else {
            *a_start_index = i + 1;
            return Some(param.clone());
        }
        i += 1;
    }
    None
}

/// Port of `wbFindCmdLineParam(aSwitch, out aValue)`: [`find_cmd_line_param`]
/// with `SwitchChars` and without case.
pub fn find_cmd_line_param_switch(params: &[String], a_switch: &str) -> Option<String> {
    find_cmd_line_param(params, a_switch, &SWITCH_CHARS, true)
}

/// Port of `wbFindCmdLineParam(var aStartIndex, out aValue)`: the next
/// parameter that is not a switch, from `a_start_index` (1-based).
pub fn find_cmd_line_param_next(params: &[String], a_start_index: &mut usize) -> Option<String> {
    find_cmd_line_param_at(params, a_start_index, &SWITCH_CHARS)
}

/// Port of `System.SysUtils.FindCmdLineSwitch(aSwitch)`, which `xeInit.pas`
/// asks for a switch of its own name: a parameter that is a switch
/// character followed by exactly `aSwitch`, compared without case. A value
/// after a colon is a different parameter (`-name:value` is not the switch
/// `name`), which is why `xeInit.pas` asks `wbFindCmdLineParam` too.
pub fn find_cmd_line_switch(params: &[String], a_switch: &str) -> bool {
    for param in params {
        if let Some((first, length)) = first_char(param)
            && SWITCH_CHARS.contains(&first)
            && param[length..].eq_ignore_ascii_case(a_switch)
        {
            return true;
        }
    }
    false
}

/// Whether the command line holds the switch `a_switch` as
/// `xeInit.pas` asks for a tool mode: `FindCmdLineSwitch(s) or
/// wbFindCmdLineParam(s, p)`, so a bare switch and one with a value are
/// both the switch.
pub fn has_cmd_line_switch(params: &[String], a_switch: &str) -> bool {
    find_cmd_line_switch(params, a_switch) || find_cmd_line_param_switch(params, a_switch).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn a_switch_with_a_value_is_found() {
        let p = params(&["-game", "-D:M:\\Data", "-l:English"]);
        assert_eq!(find_cmd_line_param_switch(&p, "D"), Some("M:\\Data".to_owned()));
        assert_eq!(find_cmd_line_param_switch(&p, "l"), Some("English".to_owned()));
        assert_eq!(find_cmd_line_param_switch(&p, "game"), Some(String::new()));
        assert_eq!(find_cmd_line_param_switch(&p, "G"), None);
    }

    #[test]
    fn a_slash_is_a_switch_character_too() {
        let p = params(&["/SSE", "/quickautoclean"]);
        assert_eq!(find_cmd_line_param_switch(&p, "SSE"), Some(String::new()));
        assert!(find_cmd_line_switch(&p, "quickautoclean"));
    }

    #[test]
    fn a_double_dash_is_not_a_switch_name() {
        // `--game` strips one character and is the switch `-game`, which is
        // not `game`.
        let p = params(&["--game", "sse"]);
        assert_eq!(find_cmd_line_param_switch(&p, "game"), None);
        assert!(!find_cmd_line_switch(&p, "game"));
    }

    #[test]
    fn a_value_after_a_colon_is_not_the_switch_itself() {
        // `FindCmdLineSwitch` compares the whole parameter after the switch
        // character; `wbFindCmdLineParam` matches it as a value.
        let p = params(&["-quickclean:1"]);
        assert!(!find_cmd_line_switch(&p, "quickclean"));
        assert_eq!(find_cmd_line_param_switch(&p, "quickclean"), Some("1".to_owned()));
        assert!(has_cmd_line_switch(&p, "quickclean"));
    }

    #[test]
    fn case_does_not_matter_for_switch_names() {
        let p = params(&["-QUICKAUTOCLEAN", "-iknowwhatimdoing"]);
        assert!(find_cmd_line_switch(&p, "quickautoclean"));
        assert!(find_cmd_line_switch(&p, "IKnowWhatImDoing"));
    }

    #[test]
    fn the_next_parameter_skips_the_switches() {
        let p = params(&["-SSE", "-nobuildrefs", "M:\\Data\\Update.esm", "-autoexit", "Next.esp"]);
        let mut index = 1;
        assert_eq!(
            find_cmd_line_param_next(&p, &mut index),
            Some("M:\\Data\\Update.esm".to_owned())
        );
        assert_eq!(index, 4);
        assert_eq!(find_cmd_line_param_next(&p, &mut index), Some("Next.esp".to_owned()));
        assert_eq!(index, 6);
        assert_eq!(find_cmd_line_param_next(&p, &mut index), None);
    }

    #[test]
    fn the_position_of_a_skipped_parameter_is_kept() {
        // `Inc(aStartIndex)` happens for every switch read, so a run of
        // switches leaves the index after the last of them.
        let p = params(&["-a", "-b", "-c"]);
        let mut index = 1;
        assert_eq!(find_cmd_line_param_next(&p, &mut index), None);
        assert_eq!(index, 4);
    }

    #[test]
    fn an_empty_character_set_takes_every_parameter() {
        // Delphi deletes the first character of every parameter when the
        // character set is empty, so the name is compared with the rest.
        let p = params(&["plain", "-switch"]);
        assert_eq!(find_cmd_line_param(&p, "lain", &[], true), Some(String::new()));
        assert_eq!(find_cmd_line_param(&p, "switch", &[], true), Some(String::new()));
        assert_eq!(find_cmd_line_param(&p, "plain", &[], true), None);
    }
}
