// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcTransformInfo.pas

//! `Transform information`: reports the translation, rotation (as Euler
//! angles) and scale of the nodes and of `bhkRigidBodyT` that are not the
//! identity.

use std::sync::atomic::Ordering;

use crate::data_format::{DfError, R};
use crate::data_format_nif::{
    NifFile, block, block_children_by_type, block_get_collision, block_get_transform, block_is_ni_object, blocks_count,
};
use crate::data_format_nif_types::ROTATION_EULER;
use crate::nif_math::{Transform, round_to, same_value};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage};

pub struct ProcTransformInfo {
    base: ProcBase,
    translation_checked: bool,
    rotation_checked: bool,
    scale_checked: bool,
    skip_empty_checked: bool,
    translation: bool,
    rotation: bool,
    scale: bool,
    /// `fSkipEmpty`: never set upstream (see `on_start`).
    skip_empty: bool,
    rotation_euler_old: bool,
}

impl ProcTransformInfo {
    pub fn new() -> ProcTransformInfo {
        let mut base = ProcBase::new("Transform information", GameType::ALL, &["nif"]);
        base.no_output = true;
        ProcTransformInfo {
            base,
            translation_checked: true,
            rotation_checked: true,
            scale_checked: true,
            skip_empty_checked: true,
            translation: false,
            rotation: false,
            scale: false,
            skip_empty: false,
            rotation_euler_old: false,
        }
    }
}

impl Proc for ProcTransformInfo {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.translation_checked = storage.get_bool("bTranslation", true);
        self.rotation_checked = storage.get_bool("bRotation", true);
        self.scale_checked = storage.get_bool("bScale", true);
        self.skip_empty_checked = storage.get_bool("bSkipEmpty", true);
        self.rotation_euler_old = ROTATION_EULER.load(Ordering::Relaxed);
    }

    fn on_hide(&mut self) {
        ROTATION_EULER.store(self.rotation_euler_old, Ordering::Relaxed);
    }

    fn on_start(&mut self) -> R<()> {
        self.translation = self.translation_checked;
        self.rotation = self.rotation_checked;
        self.scale = self.scale_checked;
        if !(self.translation || self.rotation || self.scale) {
            return Err(DfError::new("Select options to report"));
        }
        // UPSTREAM-QUIRK: the skip setting goes to the scale, and nothing is
        // skipped.
        self.scale = self.skip_empty_checked;
        ROTATION_EULER.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut log: Vec<String> = Vec::new();
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        for i in 0..blocks_count(tree)? {
            let b = block(tree, i)?;
            let node = block_is_ni_object(tree, b, "NiAVObject", true);
            if !(node || block_is_ni_object(tree, b, "bhkRigidBodyT", true)) {
                continue;
            }
            let mut path = "";
            if node {
                if self.skip_empty
                    && block_get_collision(tree, b)?.is_none()
                    && block_children_by_type(tree, b, "NiAVObject", true)?.is_empty()
                {
                    continue;
                }
                path = "Transform\\";
            }
            let mut t = Transform::default();
            if !block_get_transform(tree, b, &mut t)? {
                continue;
            }
            let mut infos: Vec<String> = Vec::new();
            if self.translation && !t.translation.is_zero() {
                // A vector of four in rigid bodies: XYZ only.
                infos.push(format!(
                    "Translation: {} {} {}",
                    tree.edit_values(b, &format!("{path}Translation\\X"))?,
                    tree.edit_values(b, &format!("{path}Translation\\Y"))?,
                    tree.edit_values(b, &format!("{path}Translation\\Z"))?
                ));
            }
            if self.rotation && !t.rotation.is_identity() {
                infos.push(format!(
                    "Rotation: {}",
                    tree.edit_values(b, &format!("{path}Rotation"))?
                ));
            }
            if self.scale && !same_value(round_to(f64::from(t.scale), -3), 1.0) {
                infos.push(format!("Scale: {}", tree.edit_values(b, &format!("{path}Scale"))?));
            }
            if !infos.is_empty() {
                log.push(format!("\t{}: {}", tree.name(b)?, infos.join("\t")));
            }
        }
        if !log.is_empty() {
            log.insert(0, file.file_name.clone());
            log.push(String::new());
            ctx.add_messages(log);
        }
        Ok(Vec::new())
    }
}
