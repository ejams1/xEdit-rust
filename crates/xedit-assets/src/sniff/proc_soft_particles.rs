// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcSoftParticles.pas

//! `Vanilla Plus Particles - NVSE`: marks the `BSShaderNoLightingProperty`
//! shapes as soft particles for an NVSE plugin, with the scale as extra
//! data.

use crate::data_format::{DfError, R, df_str_to_float};
use crate::data_format_nif::{NifFile, block_is_editor_marker, block_property_by_type, blocks_by_type};
use crate::proc_base;
use crate::sniff::proc_walls_reflection_flag::add_float_extra_data;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage};
use crate::variant::Variant;

/// `cVPSoftScale`.
const VP_SOFT_SCALE: &str = "VPSoftScale";

pub struct ProcSoftParticles {
    base: ProcBase,
    /// `edSoftScale`.
    soft_scale: String,
}

impl ProcSoftParticles {
    pub fn new() -> ProcSoftParticles {
        ProcSoftParticles {
            base: ProcBase::new(
                "Vanilla Plus Particles - NVSE",
                &[GameType::Fo3, GameType::Fnv],
                &["nif"],
            ),
            soft_scale: "0.05".to_owned(),
        }
    }
}

impl Proc for ProcSoftParticles {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.soft_scale = storage.get_string("sSoftScale", "0.05");
    }

    fn on_start(&mut self) -> R<()> {
        if !self.soft_scale.is_empty() && df_str_to_float(&self.soft_scale).is_err() {
            return Err(DfError::new("Invalid float number for soft scale"));
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        let mut changed = false;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        for b in blocks_by_type(tree, "NiTriBasedGeom", true)? {
            if block_is_editor_marker(tree, b)? {
                continue;
            }
            let Some(shader) = block_property_by_type(tree, b, "BSShaderNoLightingProperty", false)? else {
                continue;
            };
            if !tree.native_values(shader, "Shader Flags 2\\Unknown9")?.to_bool()? {
                tree.set_native_values(shader, "Shader Flags 2\\Unknown9", Variant::Int(1))?;
                changed = true;
            }
            // The extra data the same as `AddFloatExtraData` of the
            // reflections.
            changed = add_float_extra_data(tree, b, VP_SOFT_SCALE, &self.soft_scale)? || changed;
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
