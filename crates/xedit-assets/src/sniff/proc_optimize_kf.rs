// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcOptimizeKF.pas

//! `Optimize Animations`: removes the keys of the interpolators' data that
//! have the same value as the keys before and after them.

use crate::data_format::{El, R, Tree, df_float_to_str};
use crate::data_format_nif::{NifFile, blocks_by_type};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject};
use crate::variant::Variant;

pub struct ProcOptimizeKF {
    base: ProcBase,
}

impl ProcOptimizeKF {
    pub fn new() -> ProcOptimizeKF {
        ProcOptimizeKF {
            base: ProcBase::new(
                "Optimize Animations",
                &[
                    GameType::Tes4,
                    GameType::Fo3,
                    GameType::Fnv,
                    GameType::Tes5,
                    GameType::Sse,
                    GameType::Fo4,
                ],
                &["kf", "nif"],
            ),
        }
    }
}

/// `KeyValue`. UPSTREAM-QUIRK: the tangents and the TBC replace the value
/// instead of being added to it.
fn key_value(tree: &mut Tree, key: El) -> R<String> {
    let mut result = tree.edit_values(key, "Value")?;
    if tree.elements(key, "Forward")?.is_some() {
        result = format!(
            " {} {}",
            tree.edit_values(key, "Forward")?,
            tree.edit_values(key, "Backward")?
        );
    }
    if tree.elements(key, "TBC")?.is_some() {
        result = format!(
            " {} {} {}",
            tree.edit_values(key, "TBC\\T")?,
            tree.edit_values(key, "TBC\\B")?,
            tree.edit_values(key, "TBC\\C")?
        );
    }
    Ok(result.replace(&df_float_to_str(-0.0), &df_float_to_str(0.0)))
}

/// `Optimize`.
fn optimize(tree: &mut Tree, keys: Option<El>, keys_count: &str) -> R<bool> {
    let mut result = false;
    let Some(keys) = keys else { return Ok(false) };
    let count = tree.count(keys);
    if count < 3 {
        return Ok(false);
    }
    let last = tree.item(keys, count - 1)?;
    let mut next = key_value(tree, last)?;
    let before_last = tree.item(keys, count - 2)?;
    let mut current = key_value(tree, before_last)?;
    for j in (1..=count - 2).rev() {
        let previous_key = tree.item(keys, j - 1)?;
        let prev = key_value(tree, previous_key)?;
        if current == prev && current == next {
            tree.delete(keys, j)?;
            result = true;
        }
        next = current;
        current = prev;
    }
    if result && !keys_count.is_empty() {
        let count = tree.count(keys);
        tree.set_native_values(keys, keys_count, Variant::Int(i64::from(count)))?;
    }
    Ok(result)
}

impl Proc for ProcOptimizeKF {
    proc_base!();

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        for b in blocks_by_type(tree, "NiKeyBasedInterpolator", true)? {
            let Some(data_link) = tree.elements(b, "Data")? else {
                continue;
            };
            let Some(data) = tree.links_to(data_link)? else {
                continue;
            };
            let entries = tree.elements(data, "Quaternion Keys")?;
            if entries.is_some() && optimize(tree, entries, "..\\Num Rotation Keys")? {
                changed = true;
            }
            for path in ["Translations\\Keys", "Scales\\Keys"] {
                let entries = tree.elements(data, path)?;
                if entries.is_some() && optimize(tree, entries, "..\\Num Keys")? {
                    changed = true;
                }
            }
            if let Some(rotations) = tree.elements(data, "XYZ Rotations")? {
                for j in 0..tree.count(rotations) {
                    let rotation = tree.item(rotations, j)?;
                    let keys = tree.elements(rotation, "Keys")?;
                    if optimize(tree, keys, "..\\Num Keys")? {
                        changed = true;
                    }
                }
            }
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
