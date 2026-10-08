// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcAdjustTransform.pas

//! `Adjust transformation`: adds to, multiplies or sets the translation,
//! rotation and scale of the root node or of the nodes of the names.

use crate::data_format::{DfError, El, R, Tree, df_float_to_str, df_str_to_float};
use crate::data_format_nif::{NifFile, block, blocks_count};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, contains_text, delimited_text, delimited_text_of,
    same_text, trim,
};
use crate::variant::Variant;

pub struct ProcAdjustTransform {
    base: ProcBase,
    names_text: String,
    exact_match_checked: bool,
    mode_checked: i32,
    pos_text: [String; 3],
    rot_text: [String; 3],
    scale_text: String,
    names: Vec<String>,
    exact_match: bool,
    mode: i32,
    pos_x: String,
    pos_y: String,
    pos_z: String,
    rot_y: String,
    rot_p: String,
    rot_r: String,
    scale: String,
}

impl ProcAdjustTransform {
    pub fn new() -> ProcAdjustTransform {
        ProcAdjustTransform {
            base: ProcBase::new("Adjust transformation", GameType::ALL, &["nif", "kf"]),
            names_text: String::new(),
            exact_match_checked: true,
            mode_checked: 1,
            pos_text: Default::default(),
            rot_text: Default::default(),
            scale_text: String::new(),
            names: Vec::new(),
            exact_match: true,
            mode: 1,
            pos_x: String::new(),
            pos_y: String::new(),
            pos_z: String::new(),
            rot_y: String::new(),
            rot_p: String::new(),
            rot_r: String::new(),
            scale: String::new(),
        }
    }

    /// `Transform`.
    fn transform(&self, tree: &mut Tree, t: Option<El>) -> R<bool> {
        let mut result = false;
        let Some(t) = t else { return Ok(false) };
        result = adjust_value(tree, t, "Translation\\X", self.mode, &self.pos_x)? || result;
        result = adjust_value(tree, t, "Translation\\Y", self.mode, &self.pos_y)? || result;
        result = adjust_value(tree, t, "Translation\\Z", self.mode, &self.pos_z)? || result;
        result = adjust_value(tree, t, "Scale", self.mode, &self.scale)? || result;
        if !self.rot_y.is_empty() || !self.rot_p.is_empty() || !self.rot_r.is_empty() {
            let s = tree.edit_values(t, "Rotation")?;
            let mut parts = delimited_text(&s, ' ');
            let get = |parts: &[String], index: usize| {
                parts
                    .get(index)
                    .cloned()
                    .ok_or_else(|| DfError::new(format!("List index out of bounds ({index})")))
            };
            let y = adjust_float(df_str_to_float(&get(&parts, 0)?)?, self.mode, &self.rot_y)?;
            let p = adjust_float(df_str_to_float(&get(&parts, 1)?)?, self.mode, &self.rot_p)?;
            let r = adjust_float(df_str_to_float(&get(&parts, 2)?)?, self.mode, &self.rot_r)?;
            parts[0] = df_float_to_str(y);
            parts[1] = df_float_to_str(p);
            parts[2] = df_float_to_str(r);
            tree.set_edit_values(t, "Rotation", &delimited_text_of(&parts, ' '))?;
            result = s != tree.edit_values(t, "Rotation")? || result;
        }
        Ok(result)
    }
}

/// `AdjustFloat`.
fn adjust_float(value: f64, mode: i32, v: &str) -> R<f64> {
    if v.is_empty() {
        return Ok(value);
    }
    Ok(match mode {
        1 => value + df_str_to_float(v)?,
        2 => value * df_str_to_float(v)?,
        _ => df_str_to_float(v)?,
    })
}

/// `AdjustValue`.
fn adjust_value(tree: &mut Tree, el: El, path: &str, mode: i32, v: &str) -> R<bool> {
    if v.is_empty() {
        return Ok(false);
    }
    let s = tree.edit_values(el, path)?;
    if s.is_empty() {
        return Ok(false);
    }
    let value = tree.native_values(el, path)?.to_f64()?;
    tree.set_native_values(el, path, Variant::Float(adjust_float(value, mode, v)?))?;
    Ok(s != tree.edit_values(el, path)?)
}

/// `GetVerifyFloat`.
fn get_verify_float(s: &str) -> R<String> {
    let result = trim(s).to_owned();
    if !result.is_empty() {
        df_str_to_float(&result)?;
    }
    Ok(result)
}

impl Proc for ProcAdjustTransform {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.names_text = storage.get_string("sNames", "");
        self.exact_match_checked = storage.get_bool("bExactMatch", true);
        self.mode_checked = match storage.get_integer("iMode", 1) {
            2 => 2,
            3 => 3,
            _ => 1,
        };
        for (index, name) in ["sPosX", "sPosY", "sPosZ"].iter().enumerate() {
            self.pos_text[index] = storage.get_string(name, "");
        }
        for (index, name) in ["sRotY", "sRotP", "sRotR"].iter().enumerate() {
            self.rot_text[index] = storage.get_string(name, "");
        }
        self.scale_text = storage.get_string("sScale", "");
    }

    fn on_start(&mut self) -> R<()> {
        self.names = delimited_text(&self.names_text, ',')
            .iter()
            .map(|name| trim(name).to_owned())
            .collect();
        self.exact_match = self.exact_match_checked;
        self.mode = self.mode_checked;
        self.pos_x = get_verify_float(&self.pos_text[0])?;
        self.pos_y = get_verify_float(&self.pos_text[1])?;
        self.pos_z = get_verify_float(&self.pos_text[2])?;
        self.rot_y = get_verify_float(&self.rot_text[0])?;
        self.rot_p = get_verify_float(&self.rot_text[1])?;
        self.rot_r = get_verify_float(&self.rot_text[2])?;
        self.scale = get_verify_float(&self.scale_text)?;
        if [
            &self.pos_x,
            &self.pos_y,
            &self.pos_z,
            &self.rot_y,
            &self.rot_p,
            &self.rot_r,
            &self.scale,
        ]
        .iter()
        .all(|value| value.is_empty())
        {
            return Err(DfError::new("No adjustment values set"));
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        for i in 0..blocks_count(tree)? {
            let b = block(tree, i)?;
            // The root block when no names are given.
            if self.names.is_empty() {
                let t = tree.elements(b, "Transform")?;
                changed = self.transform(tree, t)?;
                break;
            }
            let name = tree.edit_values(b, "Name")?;
            if self
                .names
                .iter()
                .any(|s| (self.exact_match && same_text(&name, s)) || (!self.exact_match && contains_text(&name, s)))
            {
                let t = tree.elements(b, "Transform")?;
                changed = self.transform(tree, t)? || changed;
            }
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
