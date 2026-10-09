// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcJamAnim.pas

//! `Add NiTransformData`: adds the transform data the transform
//! interpolators of a KF lack, with two rotation and translation keys.

use crate::data_format::{DfError, R};
use crate::data_format_nif::{NifFile, add_block, blocks_by_type, root_nodes};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation};
use crate::variant::Variant;

pub struct ProcJamAnim {
    base: ProcBase,
    add_rotation_checked: bool,
    add_translation_checked: bool,
    add_rotation: bool,
    add_translation: bool,
}

impl ProcJamAnim {
    pub fn new() -> ProcJamAnim {
        ProcJamAnim {
            base: ProcBase::new(
                "Add NiTransformData",
                &[GameType::Tes4, GameType::Fo3, GameType::Fnv],
                &["kf"],
            ),
            add_rotation_checked: true,
            add_translation_checked: true,
            add_rotation: true,
            add_translation: true,
        }
    }
}

impl Proc for ProcJamAnim {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.add_rotation_checked = storage.get_bool("bAddRotation", self.add_rotation_checked);
        self.add_translation_checked = storage.get_bool("bAddTranslation", self.add_translation_checked);
    }

    fn on_start(&mut self) -> R<()> {
        self.add_rotation = self.add_rotation_checked;
        self.add_translation = self.add_translation_checked;

        if !self.add_rotation && !self.add_translation {
            return Err(DfError::new("Need to select at least one option"));
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;

        for interpolator in blocks_by_type(tree, "NiTransformInterpolator", false)? {
            let data = tree.elements(interpolator, "Data")?.ok_or_else(access_violation)?;
            if tree.links_to(data)?.is_some() {
                continue;
            }

            let transfdata = add_block(tree, "NiTransformData")?;
            let index = tree.index(transfdata)?;
            tree.set_native_values(interpolator, "Data", Variant::Int(i64::from(index)))?;

            if self.add_rotation {
                tree.set_native_values(transfdata, "Num Rotation Keys", Variant::Int(2))?;
                tree.set_edit_values(transfdata, "Rotation Type", "LINEAR_KEY")?;

                let rotation = tree.edit_values(interpolator, "Transform\\Rotation")?;
                let keys = tree
                    .elements(transfdata, "Quaternion Keys")?
                    .ok_or_else(access_violation)?;
                let key = tree.add(keys)?;
                tree.set_edit_values(key, "Value", &rotation)?;

                let root = root_nodes(tree)?.first().copied().ok_or_else(access_violation)?;
                let stop_time = tree.native_values(root, "Stop Time")?;
                let key = tree.add(keys)?;
                tree.set_native_values(key, "Time", stop_time)?;
                tree.set_edit_values(key, "Value", &rotation)?;
            }

            if self.add_translation {
                let t = tree.edit_values(interpolator, "Transform\\Translation")?;
                if !(t.contains("Min") || t.contains("Max") || t.contains("Inf") || t.contains("NaN")) {
                    tree.set_native_values(transfdata, "Translations\\Num Keys", Variant::Int(2))?;
                    tree.set_edit_values(transfdata, "Translations\\Interpolation", "LINEAR_KEY")?;

                    let keys = tree
                        .elements(transfdata, "Translations\\Keys")?
                        .ok_or_else(access_violation)?;
                    let key = tree.add(keys)?;
                    tree.set_edit_values(key, "Value", &t)?;

                    let root = root_nodes(tree)?.first().copied().ok_or_else(access_violation)?;
                    let stop_time = tree.native_values(root, "Stop Time")?;
                    let key = tree.add(keys)?;
                    tree.set_native_values(key, "Time", stop_time)?;
                    tree.set_edit_values(key, "Value", &t)?;
                }
            }

            changed = true;
        }

        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
