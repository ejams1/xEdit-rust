// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcAddFacialAnim.pas

//! `Add facial anim`: adds the `HeadAnims` and `HeadAnims:0` controlled
//! blocks of the facial animations listed in the memo, with the keys of
//! each modifier.

use crate::data_format::{DfError, R, df_str_to_float, split_string};
use crate::data_format_nif::{NifFile, add_block, block_type, blocks_count, root_nodes};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, string_list_lines, string_to_text,
};
use crate::variant::{Variant, str_to_int};

const HEAD_ANIMS: &str = "HeadAnims";
const HEAD_ANIMS_0: &str = "HeadAnims:0";

/// The IDs of `OnStart`: an `Anger` to `HeadYaw` list.
const IDS: &[&str] = &[
    "Anger",
    "Fear",
    "Happy",
    "Sad",
    "Surprise",
    "MoodNeutral",
    "MoodAfraid",
    "MoodAnnoyed",
    "MoodCocky",
    "MoodDrugged",
    "MoodPleasant",
    "MoodAngry",
    "MoodSad",
    "Pained",
    "CombatAnger",
    "Aah",
    "BigAah",
    "BMP",
    "ChjSh",
    "DST",
    "Eee",
    "Eh",
    "FV",
    "i",
    "k",
    "N",
    "Oh",
    "OohQ",
    "R",
    "Th",
    "W",
    "BlinkLeft",
    "BlinkRight",
    "BrowDownLeft",
    "BrowDownRight",
    "BrowInLeft",
    "BrowInRight",
    "BrowUpLeft",
    "BrowUpRight",
    "LookDown",
    "LookLeft",
    "LookRight",
    "LookUp",
    "SquintLeft",
    "SquintRight",
    "HeadPitch",
    "HeadRoll",
    "HeadYaw",
];

/// `TAnimKey`.
#[derive(Debug)]
struct AnimKey {
    time: f64,
    value: f64,
}

/// `TAnimMod`.
#[derive(Debug)]
struct AnimMod {
    priority: i32,
    modifier: String,
    keys: Vec<AnimKey>,
}

/// The default of the memo (`Lines.Strings` joined with CRLF).
const MODS_DEFAULT: &str =
    "99 Aah 0.466667 0 1.499999 1\r\n99 Eh 1.500000 1\r\n99 BigAah 1.5 1 1.833333 0.1 3.333333 0.5 4.033333 1";

pub struct ProcAddFacialAnim {
    base: ProcBase,
    remove_checked: bool,
    mods_text: String,
    remove: bool,
    mods: Vec<AnimMod>,
}

impl ProcAddFacialAnim {
    pub fn new() -> ProcAddFacialAnim {
        ProcAddFacialAnim {
            base: ProcBase::new("Add facial anim", &[GameType::Fo3, GameType::Fnv], &["kf"]),
            remove_checked: false,
            mods_text: MODS_DEFAULT.to_owned(),
            remove: false,
            mods: Vec::new(),
        }
    }
}

impl Proc for ProcAddFacialAnim {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.remove_checked = storage.get_bool("bRemoveExisting", self.remove_checked);
        self.mods_text = string_to_text(&storage.get_string("sMods", &self.mods_text));
    }

    fn on_start(&mut self) -> R<()> {
        self.remove = self.remove_checked;
        self.mods = Vec::new();

        for (i, line) in string_list_lines(&self.mods_text).into_iter().enumerate() {
            if line.is_empty() {
                continue;
            }

            let vals: Vec<String> = split_string(&line, " ")
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect();
            if vals.len() < 4 {
                return Err(DfError::new(format!(
                    "Line {} has less than 4 space separated values",
                    i + 1
                )));
            }

            let priority = str_to_int(&vals[0]).unwrap_or(-1);
            if priority == -1 {
                return Err(DfError::new(format!(
                    "Invalid priority \"{}\" on line {}",
                    vals[0],
                    i + 1
                )));
            }

            let modifier = vals[1].clone();
            if !IDS.iter().any(|id| *id == modifier) {
                return Err(DfError::new(format!(
                    "Invalid expression/phoneme/modifier \"{modifier}\" on line {}",
                    i + 1
                )));
            }

            let mut keys = Vec::new();
            let mut j = 2;
            while vals.len() >= j + 2 {
                let time = df_str_to_float(&vals[j])
                    .map_err(|_| DfError::new(format!("Invalid time \"{}\" on line {}", vals[j], i + 1)))?;
                let value = df_str_to_float(&vals[j + 1])
                    .map_err(|_| DfError::new(format!("Invalid intensity \"{}\" on line {}", vals[j + 1], i + 1)))?;
                keys.push(AnimKey { time, value });
                j += 2;
            }

            if keys.is_empty() {
                // UPSTREAM-QUIRK: this message alone names the 0 based line.
                return Err(DfError::new(format!(
                    "Modifier \"{modifier}\" has no Time/Intensity values on line {i}"
                )));
            }

            self.mods.push(AnimMod {
                priority,
                modifier,
                keys,
            });
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;

        if blocks_count(tree)? == 0 {
            return Ok(Vec::new());
        }

        let seq = root_nodes(tree)?.first().copied().ok_or_else(access_violation)?;
        if block_type(tree, seq) != "NiControllerSequence" {
            return Ok(Vec::new());
        }

        let Some(entries) = tree.elements(seq, "Controlled Blocks")? else {
            return Ok(Vec::new());
        };

        let mut has_head_anim = false;
        for i in (0..tree.count(entries)).rev() {
            let entry = tree.item(entries, i)?;
            // checking for HeadAnim block
            if tree.edit_values(entry, "Node Name")? == HEAD_ANIMS {
                has_head_anim = true;
            }
            // remove existing HeadAnims
            if self.remove && tree.edit_values(entry, "Node Name")? == HEAD_ANIMS_0 {
                let interpolator = tree.elements(entry, "Interpolator")?.ok_or_else(access_violation)?;
                if let Some(interpolator) = tree.links_to(interpolator)? {
                    tree.set_native_values(entry, "Interpolator", Variant::Int(-1))?;
                    crate::data_format_nif::block_remove_branch(tree, interpolator, false)?;
                }
                tree.delete(entries, i)?;
            }
        }

        // add HeadAnims block if missing
        if !has_head_anim {
            let entry = tree.add(entries)?;
            tree.set_edit_values(entry, "Node Name", HEAD_ANIMS)?;
            tree.set_edit_values(entry, "Controller Type", "NiVisController")?;
            let interpolator = add_block(tree, "NiBoolInterpolator")?;
            tree.set_edit_values(interpolator, "Value", "yes")?;
            let index = tree.index(interpolator)?;
            tree.set_native_values(entry, "Interpolator", Variant::Int(i64::from(index)))?;
            if !self.mods.is_empty() {
                tree.set_native_values(entry, "Priority", Variant::Int(i64::from(self.mods[0].priority)))?;
            }
        }

        // add facial anim blocks
        for anim in &self.mods {
            let entry = tree.add(entries)?;
            tree.set_edit_values(entry, "Node Name", HEAD_ANIMS_0)?;
            tree.set_native_values(entry, "Priority", Variant::Int(i64::from(anim.priority)))?;
            tree.set_edit_values(entry, "Controller Type", "NiGeomMorpherController")?;
            tree.set_edit_values(entry, "Interpolator ID", &anim.modifier)?;

            let interpolator = add_block(tree, "NiFloatInterpolator")?;
            tree.set_native_values(interpolator, "Value", Variant::Int(0))?;
            let index = tree.index(interpolator)?;
            tree.set_native_values(entry, "Interpolator", Variant::Int(i64::from(index)))?;

            let idata = add_block(tree, "NiFloatData")?;
            let index = tree.index(idata)?;
            tree.set_native_values(interpolator, "Data", Variant::Int(i64::from(index)))?;
            // setting num keys first because Interpolation field is disabled when keys = 0
            tree.set_native_values(idata, "Data\\Num Keys", Variant::Int(anim.keys.len() as i64))?;
            tree.set_edit_values(idata, "Data\\Interpolation", "QUADRATIC_KEY")?;
            let datakeys = tree.elements(idata, "Data\\Keys")?.ok_or_else(access_violation)?;
            for key in &anim.keys {
                let datakey = tree.add(datakeys)?;
                tree.set_native_values(datakey, "Time", Variant::Float(key.time))?;
                tree.set_native_values(datakey, "Value", Variant::Float(key.value))?;
            }
        }

        let count = tree.count(entries);
        tree.set_native_values(seq, "Num Controlled Blocks", Variant::Int(i64::from(count)))?;

        nif.save_to_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(text: &str) -> R<Vec<AnimMod>> {
        let mut proc = ProcAddFacialAnim::new();
        proc.mods_text = text.to_owned();
        proc.on_start()?;
        Ok(proc.mods)
    }

    #[test]
    fn mods_read_as_lines_of_pairs() {
        let mods = start("99 Aah 0.466667 0 1.499999 1\r\n\r\n99 HeadYaw 0.5 1").unwrap();
        assert_eq!(mods.len(), 2);
        assert_eq!(mods[0].priority, 99);
        assert_eq!(mods[0].modifier, "Aah");
        assert_eq!(mods[0].keys.len(), 2);
        assert_eq!((mods[0].keys[1].time, mods[0].keys[1].value), (1.499999, 1.0));
        // A single leftover value is ignored.
        assert_eq!(mods[1].keys.len(), 1);
    }

    #[test]
    fn bad_lines_are_refused() {
        assert_eq!(
            start("99 Aah").unwrap_err().0,
            "Line 1 has less than 4 space separated values"
        );
        assert_eq!(start("x Aah 0 1").unwrap_err().0, "Invalid priority \"x\" on line 1");
        assert_eq!(
            start("99 Nope 0 1").unwrap_err().0,
            "Invalid expression/phoneme/modifier \"Nope\" on line 1"
        );
        assert_eq!(start("99 Aah x 1").unwrap_err().0, "Invalid time \"x\" on line 1");
        assert_eq!(start("99 Aah 0 x").unwrap_err().0, "Invalid intensity \"x\" on line 1");
        // UPSTREAM-QUIRK: the `has no Time/Intensity values` message (which
        // alone names the 0 based line) can not be reached: a line with less
        // than 4 values is refused first, and 4 values hold one pair.
        assert_eq!(start("99 Aah 0 1").unwrap().len(), 1);
    }

    #[test]
    fn the_default_memo_has_three_mods() {
        let mods = start(MODS_DEFAULT).unwrap();
        assert_eq!(mods.len(), 3);
        assert_eq!(mods[2].modifier, "BigAah");
        assert_eq!(mods[2].keys.len(), 4);
    }
}
