// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcUniversalTweaker.pas

//! `Universal tweaker`: changes a value by its path in the blocks of a
//! type, or in a block by its path, of NIF, KF and material files,
//! optionally only where another value matches.

use xedit_core::delphi::{round, same_value, str_to_float};

use crate::data_format::{DfError, El, R, Tree, df_float_to_str, df_str_to_float};
use crate::data_format_material::MaterialFile;
use crate::data_format_nif::{
    NifFile, NifVersion, block_add_extra_data, block_by_path, block_by_type, block_is_ni_object, blocks_count,
    root_node,
};
use crate::proc_base;
use crate::sniff::perl_regex::PerlRegEx;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, contains_text, delimited_text, extract_file_ext,
    same_text, trim,
};

/// `TTweakOldValueMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OldValueMode {
    Equal = 0,
    NotEqual,
    Greater,
    Lesser,
    Contains,
    DoesntContain,
    StartsWith,
    EndsWith,
    And,
    AndNot,
    RegExp,
}

impl OldValueMode {
    fn from_int(value: i32) -> Option<OldValueMode> {
        use OldValueMode::*;
        [Equal, NotEqual, Greater, Lesser, Contains, DoesntContain, StartsWith, EndsWith, And, AndNot, RegExp]
            .get(usize::try_from(value).ok()?)
            .copied()
    }

    /// `MathOld`.
    fn is_math(self) -> bool {
        matches!(self, Self::Greater | Self::Lesser | Self::And | Self::AndNot)
    }
}

/// `TTweakNewValueMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewValueMode {
    Set = 0,
    Add,
    Mul,
    Replace,
    Prepend,
    Append,
    And,
    AndNot,
    Or,
    Remove,
    Round,
    MulRound,
}

impl NewValueMode {
    fn from_int(value: i32) -> Option<NewValueMode> {
        use NewValueMode::*;
        [Set, Add, Mul, Replace, Prepend, Append, And, AndNot, Or, Remove, Round, MulRound]
            .get(usize::try_from(value).ok()?)
            .copied()
    }

    /// `MathNew`.
    fn is_math(self) -> bool {
        matches!(
            self,
            Self::Add | Self::Mul | Self::And | Self::AndNot | Self::Or | Self::Round | Self::MulRound
        )
    }
}

pub struct ProcUniversalTweaker {
    base: ProcBase,
    // The controls of the frame.
    report_checked: bool,
    blocks_text: String,
    inherited_checked: bool,
    path_text: String,
    value_mode_index: i32,
    value_text: String,
    old_path_text: String,
    old_value_check_checked: bool,
    old_value_mode_index: i32,
    old_value_text: String,
    // The settings of the run.
    blocks: Vec<String>,
    inherited: bool,
    path: String,
    value: String,
    value_mode: NewValueMode,
    old_value_check: bool,
    old_path: String,
    old_value_mode: OldValueMode,
    old_value: String,
    report_only: bool,
}

impl ProcUniversalTweaker {
    pub fn new() -> ProcUniversalTweaker {
        ProcUniversalTweaker {
            base: ProcBase::new("Universal tweaker", GameType::ALL, &["nif", "kf", "bgsm", "bgem"]),
            report_checked: false,
            blocks_text: "NiMaterialProperty".to_owned(),
            inherited_checked: false,
            path_text: "Alpha".to_owned(),
            value_mode_index: 0,
            value_text: "0.8".to_owned(),
            old_path_text: String::new(),
            old_value_check_checked: false,
            old_value_mode_index: 0,
            old_value_text: String::new(),
            blocks: Vec::new(),
            inherited: false,
            path: String::new(),
            value: String::new(),
            value_mode: NewValueMode::Set,
            old_value_check: false,
            old_path: String::new(),
            old_value_mode: OldValueMode::Equal,
            old_value: String::new(),
            report_only: false,
        }
    }
}

/// What `ModifyElement` needs besides the element.
struct Modify<'a> {
    path: &'a str,
    value: &'a str,
    old_path: &'a str,
    old_value: &'a str,
    value_mode: NewValueMode,
    old_value_check: bool,
    old_value_mode: OldValueMode,
    regexp: Option<&'a PerlRegEx>,
}

/// `NativeValue(p)` of `ModifyElement` as a float: a missing element is
/// `Unassigned`, which converts to 0.
fn native_float(tree: &mut Tree, block: El, path: &str) -> R<f64> {
    let value = if path.is_empty() {
        tree.native_value(block)?
    } else {
        tree.native_values(block, path)?
    };
    value.to_f64()
}

/// `EditValue(p)` of `ModifyElement`: the result is left unassigned for a
/// missing element, so the variable keeps its value.
fn edit_value_into(tree: &mut Tree, block: El, path: &str, var: &mut String) -> R<()> {
    let value = if path.is_empty() {
        Some(tree.edit_value(block)?)
    } else {
        tree.edit_values_assigned(block, path)?
    };
    if let Some(value) = value {
        *var = value;
    }
    Ok(())
}

/// Delphi `Trunc` of an `Extended`.
fn trunc(value: f64) -> R<i64> {
    if value.is_finite() && value.abs() < 9.2e18 {
        Ok(value.trunc() as i64)
    } else {
        Err(DfError::new("Invalid floating point operation"))
    }
}

/// `ModifyElement`.
fn modify_element(tree: &mut Tree, block: El, m: &Modify, log: &mut Option<Vec<String>>) -> R<bool> {
    let mut result = false;
    let mut old_value_string = String::new();
    let mut new_value_string = String::new();
    let mut old_value_float: f64 = 0.0;
    let mut new_value_float: f64 = 0.0;

    // Every element of an array.
    if let Some(i) = m.path.find("[*]") {
        // `Copy(aPath, 1, i - 2)`: the path before `\[*]`.
        let prefix = &m.path[..i.saturating_sub(1)];
        let Some(array) = tree.elements(block, prefix)? else {
            return Ok(false);
        };
        let rest = m.path.get(i + 4..).unwrap_or("");
        let sub = Modify { path: rest, ..*m };
        for index in 0..tree.count(array) {
            let item = tree.item(array, index)?;
            result = modify_element(tree, item, &sub, log)? || result;
        }
        return Ok(result);
    }

    let read = if m.value_mode.is_math() {
        native_float(tree, block, m.path).map(|value| new_value_float = value)
    } else {
        edit_value_into(tree, block, m.path, &mut new_value_string)
    };
    if read.is_err() {
        return Ok(false);
    }

    let mut regexp_subject = String::new();
    if m.old_value_check {
        let p = if m.old_path.is_empty() { m.path } else { m.old_path };
        let read = if m.old_value_mode.is_math() {
            native_float(tree, block, p).map(|value| old_value_float = value)
        } else {
            edit_value_into(tree, block, p, &mut old_value_string)
        };
        if read.is_err() {
            return Ok(false);
        }

        // Equal and NotEqual compare as numbers when the value given is one.
        let mut eq_number = false;
        if matches!(m.old_value_mode, OldValueMode::Equal | OldValueMode::NotEqual) {
            eq_number = str_to_float(m.old_value).is_some();
            if eq_number {
                match native_float(tree, block, p) {
                    Ok(value) => old_value_float = value,
                    Err(_) => eq_number = false,
                }
            }
        }

        let matched = match m.old_value_mode {
            OldValueMode::Equal => {
                (!eq_number && same_text(&old_value_string, m.old_value))
                    || (eq_number && same_value(old_value_float, df_str_to_float(m.old_value)?))
            }
            OldValueMode::NotEqual => {
                (!eq_number && !same_text(&old_value_string, m.old_value))
                    || (eq_number && !same_value(old_value_float, df_str_to_float(m.old_value)?))
            }
            OldValueMode::Greater => old_value_float > df_str_to_float(m.old_value)?,
            OldValueMode::Lesser => old_value_float < df_str_to_float(m.old_value)?,
            OldValueMode::Contains => contains_text(&old_value_string, m.old_value),
            OldValueMode::DoesntContain => !contains_text(&old_value_string, m.old_value),
            OldValueMode::StartsWith => starts_with_text(&old_value_string, m.old_value),
            OldValueMode::EndsWith => ends_with_text(&old_value_string, m.old_value),
            OldValueMode::And => trunc(old_value_float)? & trunc(df_str_to_float(m.old_value)?)? != 0,
            OldValueMode::AndNot => trunc(old_value_float)? & trunc(df_str_to_float(m.old_value)?)? == 0,
            OldValueMode::RegExp => {
                let regexp = m.regexp.ok_or_else(|| DfError::new("Access violation"))?;
                let (matched, subject) = regexp.replace_all(&old_value_string, m.value);
                regexp_subject = subject;
                matched
            }
        };
        if !matched {
            return Ok(false);
        }
    }

    let mut new_value = match m.value_mode {
        NewValueMode::Set => m.value.to_owned(),
        NewValueMode::Add => df_float_to_str(new_value_float + df_str_to_float(m.value)?),
        NewValueMode::Mul => df_float_to_str(new_value_float * df_str_to_float(m.value)?),
        NewValueMode::Round => {
            let step = df_str_to_float(m.value)?;
            df_float_to_str(delphi_round(new_value_float / step)? as f64 * step)
        }
        NewValueMode::MulRound => df_float_to_str(delphi_round(new_value_float * df_str_to_float(m.value)?)? as f64),
        NewValueMode::And => df_float_to_str((trunc(new_value_float)? & trunc(df_str_to_float(m.value)?)?) as f64),
        NewValueMode::AndNot => df_float_to_str((trunc(new_value_float)? & !trunc(df_str_to_float(m.value)?)?) as f64),
        NewValueMode::Or => df_float_to_str((trunc(new_value_float)? | trunc(df_str_to_float(m.value)?)?) as f64),
        NewValueMode::Replace => match m.old_value_mode {
            OldValueMode::Contains => replace_text(&new_value_string, m.old_value, m.value),
            OldValueMode::StartsWith => {
                let skip = m.old_value.chars().count();
                format!("{}{}", m.value, new_value_string.chars().skip(skip).collect::<String>())
            }
            OldValueMode::EndsWith => {
                let keep = new_value_string.chars().count().saturating_sub(m.old_value.chars().count());
                format!("{}{}", new_value_string.chars().take(keep).collect::<String>(), m.value)
            }
            OldValueMode::RegExp => regexp_subject,
            // `NewValue` keeps its initial value.
            _ => String::new(),
        },
        NewValueMode::Prepend => format!("{}{new_value_string}", m.value),
        NewValueMode::Append => format!("{new_value_string}{}", m.value),
        NewValueMode::Remove => replace_text(&new_value_string, m.value, ""),
    };

    // A fractional part of zeroes is removed, for integer fields.
    if m.value_mode.is_math() {
        let one = df_float_to_str(1.0);
        let z = &one[1..];
        if let Some(stripped) = new_value.strip_suffix(z) {
            new_value = stripped.to_owned();
        }
    }

    // `OldValue := EditValues[aPath]` is left unassigned for a missing
    // element: OldValue stays '', and the reading of the new value keeps
    // the new text, so a missing element counts as changed.
    let mut old_value = String::new();
    if !m.path.is_empty() {
        edit_value_into(tree, block, m.path, &mut old_value)?;
        tree.set_edit_values(block, m.path, &new_value)?;
        edit_value_into(tree, block, m.path, &mut new_value)?;
    } else {
        old_value = tree.edit_value(block)?;
        tree.set_edit_value(block, &new_value)?;
        new_value = tree.edit_value(block)?;
    }

    result = old_value != new_value;

    if let Some(log) = log
        && result
    {
        let mut p = tree.path(block)?;
        if !m.path.is_empty() {
            p = format!("{p}\\{}", m.path);
        }
        log.push(format!("\t{p}: Changed from \"{old_value}\" to \"{new_value}\""));
    }
    Ok(result)
}

/// Delphi `Round` of an `Extended` to an `Int64`.
fn delphi_round(value: f64) -> R<i64> {
    if value.is_finite() && value.abs() < 9.2e18 {
        Ok(round(value))
    } else {
        Err(DfError::new("Invalid floating point operation"))
    }
}

/// `string.StartsWith(Value, True)`.
fn starts_with_text(text: &str, prefix: &str) -> bool {
    let text: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let prefix: Vec<char> = prefix.chars().flat_map(char::to_lowercase).collect();
    text.starts_with(&prefix)
}

/// `string.EndsWith(Value, True)`.
fn ends_with_text(text: &str, suffix: &str) -> bool {
    let text: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let suffix: Vec<char> = suffix.chars().flat_map(char::to_lowercase).collect();
    text.ends_with(&suffix)
}

/// `StringReplace(S, Old, New, [rfReplaceAll, rfIgnoreCase])`.
pub fn replace_text(text: &str, old: &str, new: &str) -> String {
    if old.is_empty() {
        return text.to_owned();
    }
    let upper_text = text.to_uppercase();
    let upper_old = old.to_uppercase();
    // Upper casing that changes lengths would misalign; compare per char.
    if upper_text.len() != text.len() || upper_old.len() != old.len() {
        let chars: Vec<char> = text.chars().collect();
        let pattern: Vec<char> = old.chars().collect();
        let fold = |c: char| c.to_uppercase().next().unwrap_or(c);
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if i + pattern.len() <= chars.len()
                && chars[i..i + pattern.len()]
                    .iter()
                    .zip(&pattern)
                    .all(|(a, b)| fold(*a) == fold(*b))
            {
                out.push_str(new);
                i += pattern.len();
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        return out;
    }
    let mut out = String::new();
    let mut last = 0;
    for (index, _) in upper_text.match_indices(&upper_old) {
        if index < last {
            continue;
        }
        out.push_str(&text[last..index]);
        out.push_str(new);
        last = index + old.len();
    }
    out.push_str(&text[last..]);
    out
}

impl Proc for ProcUniversalTweaker {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.report_checked = storage.get_bool("bReportOnly", false);
        self.blocks_text = storage.get_string("sBlocks", "NiMaterialProperty");
        self.inherited_checked = storage.get_bool("bDescendants", false);
        self.path_text = storage.get_string("sPath", "Alpha");
        // The combo boxes hold the modes as objects; an unknown one is the
        // first item.
        let mode = storage.get_integer("iValueMode", 0);
        self.value_mode_index = if NewValueMode::from_int(mode).is_some() { mode } else { 0 };
        self.value_text = storage.get_string("sValue", "0.8");
        self.old_path_text = storage.get_string("sOldPath", "");
        self.old_value_check_checked = storage.get_bool("bOldValueCheck", false);
        let mode = storage.get_integer("iOldValueMode", 0);
        self.old_value_mode_index = if OldValueMode::from_int(mode).is_some() { mode } else { 0 };
        self.old_value_text = storage.get_string("sOldValue", "");
        // The presets (`sPresets`) only fill the controls in the GUI.
    }

    fn on_start(&mut self) -> R<()> {
        self.blocks = delimited_text(&self.blocks_text, ',')
            .iter()
            .map(|block| trim(block).to_owned())
            .collect();
        self.inherited = self.inherited_checked;

        self.path = self.path_text.clone();
        if self.path.is_empty() {
            return Err(DfError::new("Field path can not be empty"));
        }

        self.value_mode = NewValueMode::from_int(self.value_mode_index).unwrap_or(NewValueMode::Set);
        self.value = self.value_text.clone();
        if self.value_mode == NewValueMode::Round && self.value.is_empty() {
            self.value = "1".to_owned();
        }

        self.old_value_check = self.old_value_check_checked;
        if self.old_value_check {
            self.old_path = self.old_path_text.clone();
            self.old_value = self.old_value_text.clone();
        } else {
            self.old_path.clear();
            self.old_value.clear();
        }
        self.old_value_mode = OldValueMode::from_int(self.old_value_mode_index).unwrap_or(OldValueMode::Equal);

        if matches!(
            self.value_mode,
            NewValueMode::Add | NewValueMode::Mul | NewValueMode::And | NewValueMode::AndNot | NewValueMode::Or | NewValueMode::Round
        ) && df_str_to_float(&self.value).is_err()
        {
            return Err(DfError::new("Value must be a number"));
        }

        if self.value_mode == NewValueMode::Replace
            && !(self.old_value_check
                && matches!(
                    self.old_value_mode,
                    OldValueMode::Contains | OldValueMode::StartsWith | OldValueMode::EndsWith | OldValueMode::RegExp
                )
                && !self.old_value.is_empty())
        {
            return Err(DfError::new(
                "When replacing, if field must be checked using \"Contains\", \"Starts with\", \"Ends with\" or \"Regular Expr\" with non-empty value",
            ));
        }

        if self.old_value_check && self.old_value_mode.is_math() && df_str_to_float(&self.old_value).is_err() {
            return Err(DfError::new("Another field's value must be a number"));
        }

        self.report_only = self.report_checked;
        self.base.no_output = self.report_only;
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let regexp = if self.old_value_check && self.old_value_mode == OldValueMode::RegExp {
            Some(PerlRegEx::new(&self.old_value)?)
        } else {
            None
        };
        let mut log = if self.report_only { Some(Vec::new()) } else { None };
        let m = Modify {
            path: &self.path,
            value: &self.value,
            old_path: &self.old_path,
            old_value: &self.old_value,
            value_mode: self.value_mode,
            old_value_check: self.old_value_check,
            old_value_mode: self.old_value_mode,
            regexp: regexp.as_ref(),
        };

        let ext = extract_file_ext(&file.file_name).to_owned();
        let mut result = Vec::new();
        if same_text(&ext, ".nif") || same_text(&ext, ".kf") {
            let mut nif = NifFile::new()?;
            nif.load_from_data(&file.get_data()?)?;
            let root = nif.root;
            let tree = &mut nif.tree;
            if self.blocks.len() == 1 && self.blocks[0].contains('\\') {
                // A block by its path.
                let Some(block) = block_by_path(tree, &self.blocks[0])? else {
                    return Ok(Vec::new());
                };
                changed = modify_element(tree, block, &m, &mut log)?;
            } else {
                // Add BSXFlags when it is the block to change and missing.
                if tree.nif.nif_version >= NifVersion::Tes4
                    && !self.blocks.is_empty()
                    && self.blocks[0] == "BSXFlags"
                    && block_by_type(tree, "BSXFlags", false)?.is_none()
                    && blocks_count(tree)? != 0
                {
                    let root_block = root_node(tree)?;
                    if block_is_ni_object(tree, root_block, "NiNode", true) {
                        let bsx = block_add_extra_data(tree, root_block, "BSXFlags")?;
                        tree.set_edit_values(bsx, "Name", "BSX")?;
                    }
                }
                // The blocks by type, the header and the footer included.
                let count = tree.count(root);
                for i in 0..count {
                    let block = tree.item(root, i)?;
                    let matched = self
                        .blocks
                        .iter()
                        .any(|s| block_is_ni_object(tree, block, s, self.inherited));
                    if !matched && !self.blocks.is_empty() {
                        continue;
                    }
                    changed = modify_element(tree, block, &m, &mut log)? || changed;
                }
            }
            if changed && !self.report_only {
                result = nif.save_to_data()?;
            }
        } else if same_text(&ext, ".bgsm") || same_text(&ext, ".bgem") {
            let mut material = if same_text(&ext, ".bgsm") {
                MaterialFile::new_bgsm()?
            } else {
                MaterialFile::new_bgem()?
            };
            material.load_from_data(&file.get_data()?)?;
            let root = material.root;
            changed = modify_element(&mut material.tree, root, &m, &mut log)?;
            if changed && !self.report_only {
                result = material.save_to_data()?;
            }
        }

        if changed && let Some(mut log) = log {
            log.insert(0, file.file_name.clone());
            log.push(String::new());
            ctx.add_messages(log);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_replace_ignores_case() {
        assert_eq!(replace_text("Textures\\Old\\a.dds", "textures\\old\\", "textures\\new\\"), "textures\\new\\a.dds");
        assert_eq!(replace_text("aAa", "a", "b"), "bbb");
        assert!(starts_with_text("ABC", "ab") && ends_with_text("ABC", "bc"));
    }
}
