// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcReplaceAssets.pas

//! `Search and replace assets`: replaces text in the texture and material
//! paths of NIF files and in the textures of BGSM and BGEM materials, as
//! text or with regular expressions, and optionally removes the absolute
//! part of a path.

use crate::data_format::{El, R, Tree};
use crate::data_format_material::MaterialFile;
use crate::data_format_nif::{NifFile, get_assets};
use crate::proc_base;
use crate::sniff::perl_regex::PerlRegEx;
use crate::sniff::proc_universal_tweaker::replace_text;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, extract_file_ext, same_text,
    string_list_lines, string_list_text_of, string_to_text, text_to_string, trim,
};

pub struct ProcReplaceAssets {
    base: ProcBase,
    /// `chkFixAbsolute`.
    fix_absolute_checked: bool,
    /// `chkRegExp`.
    reg_exp_checked: bool,
    /// `chkReport`.
    report_checked: bool,
    /// The lines of `memoPairs`.
    pairs: Vec<String>,
    fix_absolute: bool,
    reg_exp: bool,
    report_only: bool,
    search: Vec<String>,
    replace: Vec<String>,
}

impl ProcReplaceAssets {
    pub fn new() -> ProcReplaceAssets {
        ProcReplaceAssets {
            base: ProcBase::new("Search and replace assets", GameType::ALL, &["nif", "bgsm", "bgem"]),
            fix_absolute_checked: false,
            reg_exp_checked: false,
            report_checked: false,
            pairs: vec!["textures\\old\\".to_owned(), "textures\\new\\".to_owned()],
            fix_absolute: false,
            reg_exp: false,
            report_only: false,
            search: Vec::new(),
            replace: Vec::new(),
        }
    }
}

/// `LowerCase`: ASCII letters only.
fn lower_case(text: &[char]) -> Vec<char> {
    text.iter().map(char::to_ascii_lowercase).collect()
}

/// `Pos(sub, text)` on characters, 0-based.
fn pos(sub: &str, text: &[char]) -> Option<usize> {
    let sub: Vec<char> = sub.chars().collect();
    if sub.is_empty() || sub.len() > text.len() {
        return None;
    }
    (0..=text.len() - sub.len()).find(|&i| text[i..i + sub.len()] == sub[..])
}

/// `Copy(s, index, count)` on characters, `index` 0-based.
fn copy(s: &[char], index: usize, count: usize) -> String {
    s.iter().skip(index).take(count).collect()
}

impl Proc for ProcReplaceAssets {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.fix_absolute_checked = storage.get_bool("bFixAbsolute", false);
        self.reg_exp_checked = storage.get_bool("bRegExp", false);
        self.report_checked = storage.get_bool("bReportOnly", false);
        let default = text_to_string(&string_list_text_of(&self.pairs));
        self.pairs = string_list_lines(&string_to_text(&storage.get_string("sReplacements", &default)));
    }

    fn on_start(&mut self) -> R<()> {
        self.fix_absolute = self.fix_absolute_checked;
        self.report_only = self.report_checked;
        self.reg_exp = self.reg_exp_checked;
        self.base.no_output = self.report_only;
        self.search.clear();
        self.replace.clear();
        // UPSTREAM-QUIRK: the loop runs one pair past the lines; a memo
        // line out of range reads as empty, and empty pairs are skipped.
        let line = |index: usize| self.pairs.get(index).cloned().unwrap_or_default();
        for i in 0..=self.pairs.len() / 2 {
            let s = line(i * 2);
            let r = line(i * 2 + 1);
            // Skip when both lines are empty.
            if !s.is_empty() || !r.is_empty() {
                self.search.push(s);
                self.replace.push(r);
            }
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut log: Vec<String> = Vec::new();
        let mut changed = false;
        let ext = extract_file_ext(&file.file_name).to_owned();

        // The elements that hold assets.
        let mut nif = None;
        let mut material = None;
        let elements: Vec<El>;
        if same_text(&ext, ".nif") {
            let mut file_nif = NifFile::new()?;
            file_nif.load_from_data(&file.get_data()?)?;
            elements = get_assets(&mut file_nif.tree)?;
            nif = Some(file_nif);
        } else if same_text(&ext, ".bgsm") || same_text(&ext, ".bgem") {
            let mut file_material = if same_text(&ext, ".bgsm") {
                MaterialFile::new_bgsm()?
            } else {
                MaterialFile::new_bgem()?
            };
            file_material.load_from_data(&file.get_data()?)?;
            let root = file_material.root;
            let tree = &mut file_material.tree;
            let textures = tree.elements(root, "Textures")?.ok_or_else(access_violation)?;
            elements = (0..tree.count(textures))
                .map(|i| tree.item(textures, i))
                .collect::<R<Vec<El>>>()?;
            material = Some(file_material);
        } else {
            elements = Vec::new();
        }
        // Next file if nothing was found.
        if elements.is_empty() {
            return Ok(Vec::new());
        }
        let tree: &mut Tree = match (&mut nif, &mut material) {
            (Some(nif), _) => &mut nif.tree,
            (_, Some(material)) => &mut material.tree,
            _ => unreachable!(),
        };

        for el in elements {
            // The file name the element holds.
            let s = tree.edit_value(el)?;
            if s.is_empty() {
                continue;
            }
            // The replacements, on the trimmed name.
            let mut s2 = trim(&s).to_owned();
            for k in 0..self.search.len() {
                if !self.search[k].is_empty() {
                    if !self.reg_exp {
                        s2 = replace_text(&s2, &self.search[k], &self.replace[k]);
                    } else {
                        let regexp = PerlRegEx::new(&self.search[k])?;
                        s2 = regexp.replace_all(&s2, &self.replace[k]).1;
                    }
                } else {
                    // Prepend when the text to find is empty.
                    s2 = format!("{}{s2}", self.replace[k]);
                }
            }

            if self.fix_absolute {
                let chars: Vec<char> = s2.chars().collect();
                // An absolute path.
                if chars.len() > 2 && chars[1] == ':' {
                    let lower = lower_case(&chars);
                    // Remove the path up to and with Data.
                    let p = pos("\\data\\", &lower);
                    if let Some(p) = p {
                        s2 = copy(&chars, p + 6, chars.len());
                    } else if let Some(p) = pos("\\data files\\", &lower) {
                        // UPSTREAM-QUIRK: Morrowind's `Data Files` is cut
                        // from the name before the replacements.
                        let original: Vec<char> = s.chars().collect();
                        s2 = copy(&original, p + 12, chars.len());
                    }
                }
            }

            // The value has changed.
            if s != s2 {
                tree.set_edit_value(el, &s2)?;
                if !changed {
                    log.push(format!("\r\n{}", file.file_name));
                }
                let path = tree.path(el)?;
                let new = tree.edit_value(el)?;
                log.push(format!("\t{path}\r\n\t\t\"{s}\"\r\n\t\t\"{new}\""));
                changed = true;
            }
        }

        let mut result = Vec::new();
        if changed && !self.report_only {
            result = match (&mut nif, &mut material) {
                (Some(nif), _) => nif.save_to_data()?,
                (_, Some(material)) => material.save_to_data()?,
                _ => Vec::new(),
            };
        }
        ctx.add_messages(log);
        Ok(result)
    }
}
