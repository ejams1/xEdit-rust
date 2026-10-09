// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcAddHeadtrackingAnim.pas

//! `Add headtracking anim`: adds the `Bip01 Head` `HeadTrack`
//! `NiFloatExtraDataController` controlled block with four linearly
//! interpolated keys to the sequences that have a `Bip01 Head` block.

use xedit_core::delphi::round;

use crate::data_format::{DfError, R, df_str_to_float};
use crate::data_format_nif::{NifFile, add_block, block_type, blocks_count, root_nodes};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation};
use crate::variant::{Variant, str_to_int};

const BIP01_HEAD: &str = "Bip01 Head";

pub struct ProcAddHeadtrackingAnim {
    base: ProcBase,
    cycle_clamp_only_checked: bool,
    key_value14_text: String,
    key_time2_text: String,
    key_value23_text: String,
    key_time3_text: String,
    cycle_clamp_only: bool,
    key_value14: f64,
    key_value23: f64,
    key_time2: i32,
    key_time3: i32,
}

impl ProcAddHeadtrackingAnim {
    pub fn new() -> ProcAddHeadtrackingAnim {
        ProcAddHeadtrackingAnim {
            base: ProcBase::new("Add headtracking anim", &[GameType::Fo3, GameType::Fnv], &["kf"]),
            cycle_clamp_only_checked: false,
            key_value14_text: "0".to_owned(),
            key_time2_text: "20".to_owned(),
            key_value23_text: "100".to_owned(),
            key_time3_text: "80".to_owned(),
            cycle_clamp_only: false,
            key_value14: 0.0,
            key_value23: 0.0,
            key_time2: 0,
            key_time3: 0,
        }
    }
}

/// `RoundTime`.
fn round_time(length: f64, percent: i32) -> f64 {
    let round_to = 1.0 / 30.0;
    (round(length * (f64::from(percent) / 100.0) / round_to) as f64) * round_to
}

impl Proc for ProcAddHeadtrackingAnim {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.cycle_clamp_only_checked = storage.get_bool("bCycleClampOnly", self.cycle_clamp_only_checked);
        self.key_value14_text = storage.get_string("sKeyValue14", &self.key_value14_text);
        self.key_value23_text = storage.get_string("sKeyValue23", &self.key_value23_text);
        self.key_time2_text = storage.get_string("sKeyTime2", &self.key_time2_text);
        self.key_time3_text = storage.get_string("sKeyTime3", &self.key_time3_text);
    }

    fn on_start(&mut self) -> R<()> {
        self.cycle_clamp_only = self.cycle_clamp_only_checked;
        self.key_value14 = df_str_to_float(&self.key_value14_text)?;
        self.key_value23 = df_str_to_float(&self.key_value23_text)?;
        self.key_time2 = str_to_int(&self.key_time2_text)
            .ok_or_else(|| DfError::new(format!("'{}' is not a valid integer value", self.key_time2_text)))?;
        self.key_time3 = str_to_int(&self.key_time3_text)
            .ok_or_else(|| DfError::new(format!("'{}' is not a valid integer value", self.key_time3_text)))?;
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

        if self.cycle_clamp_only && tree.edit_values(seq, "Cycle Type")? != "CYCLE_CLAMP" {
            return Ok(Vec::new());
        }

        let Some(entries) = tree.elements(seq, "Controlled Blocks")? else {
            return Ok(Vec::new());
        };

        let mut has_head = false;
        let mut priority = 0i64;
        // checking for Bip01 Head blocks
        for i in 0..tree.count(entries) {
            let entry = tree.item(entries, i)?;
            if tree.edit_values(entry, "Node Name")? == BIP01_HEAD {
                // such controller already present, skip this mesh
                if tree.edit_values(entry, "Controller Type")? == "NiFloatExtraDataController" {
                    return Ok(Vec::new());
                }
                priority = tree.native_values(entry, "Priority")?.to_i64()?;
                has_head = true;
            }
        }

        // no head blocks found, skip this mesh
        if !has_head {
            return Ok(Vec::new());
        }

        // adding new controlled block
        let entry = tree.add(entries)?;
        let count = tree.count(entries);
        tree.set_native_values(seq, "Num Controlled Blocks", Variant::Int(i64::from(count)))?;
        tree.set_edit_values(entry, "Node Name", BIP01_HEAD)?;
        tree.set_native_values(entry, "Priority", Variant::Int(priority))?;
        tree.set_edit_values(entry, "Controller Type", "NiFloatExtraDataController")?;
        tree.set_edit_values(entry, "Controller ID", "HeadTrack")?;

        // adding new interpolator
        let interpolator = add_block(tree, "NiFloatInterpolator")?;
        let index = tree.index(interpolator)?;
        tree.set_native_values(entry, "Interpolator", Variant::Int(i64::from(index)))?;

        // adding extra data
        let idata = add_block(tree, "NiFloatData")?;
        let index = tree.index(idata)?;
        tree.set_native_values(interpolator, "Data", Variant::Int(i64::from(index)))?;
        let keys = tree.elements(idata, "Data\\Keys")?.ok_or_else(access_violation)?;
        tree.set_count(keys, 4)?;
        tree.set_native_values(idata, "Data\\Num Keys", Variant::Int(4))?;
        tree.set_edit_values(idata, "Data\\Interpolation", "LINEAR_KEY")?;

        // assuming always 0
        let length = tree.native_values(seq, "Stop Time")?.to_f64()?;

        for (index, time, value) in [
            (0, 0.0, self.key_value14),
            (1, round_time(length, self.key_time2), self.key_value23),
            (2, round_time(length, self.key_time3), self.key_value23),
            (3, length, self.key_value14),
        ] {
            let key = tree.item(keys, index)?;
            tree.set_native_values(key, "Time", Variant::Float(time))?;
            tree.set_native_values(key, "Value", Variant::Float(value))?;
        }

        nif.save_to_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_times_round_to_thirtieths() {
        // `Round(aLen * (aPercent / 100) / (1/30)) * (1/30)`.
        assert_eq!(round_time(1.0, 20), 6.0 / 30.0);
        assert_eq!(round_time(1.0, 100), 1.0);
        assert_eq!(round_time(0.0, 80), 0.0);
        // Round is half to even: 0.05 * 0.5 / (1/30) = 0.75 -> 1.
        assert_eq!(round_time(0.05, 50), 1.0 / 30.0);
    }

    #[test]
    fn settings_defaults_and_invalid_numbers() {
        let mut proc = ProcAddHeadtrackingAnim::new();
        proc.on_show(&Storage::new("Addheadtrackinganim".to_owned(), None));
        proc.on_start().unwrap();
        assert!(!proc.cycle_clamp_only);
        assert_eq!((proc.key_value14, proc.key_value23), (0.0, 100.0));
        assert_eq!((proc.key_time2, proc.key_time3), (20, 80));

        proc.key_time3_text = "x".to_owned();
        assert_eq!(proc.on_start().unwrap_err().0, "'x' is not a valid integer value");
    }
}
