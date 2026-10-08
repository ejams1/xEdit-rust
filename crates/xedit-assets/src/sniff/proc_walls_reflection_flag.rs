// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcWallsReflectionFlag.pas

//! `Real Time Reflections - NVSE`: marks the environment mapped
//! `BSShaderPPLightingProperty` shapes for the real time reflections of
//! an NVSE plugin, with its extra data.

use crate::data_format::{DfError, El, R, Tree, df_str_to_float};
use crate::data_format_nif::{
    NifFile, block_add_extra_data, block_extra_data_by_name, block_property_by_type, blocks_by_type,
};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, same_value};
use crate::variant::Variant;

pub struct ProcWallsReflectionFlag {
    base: ProcBase,
    /// `edMapScale`.
    map_scale: String,
    /// `edNormalIntensity`.
    normal_intensity: String,
    /// `edBlendIntensity`.
    blend_intensity: String,
}

impl ProcWallsReflectionFlag {
    pub fn new() -> ProcWallsReflectionFlag {
        ProcWallsReflectionFlag {
            base: ProcBase::new(
                "Real Time Reflections - NVSE",
                &[GameType::Fo3, GameType::Fnv],
                &["nif"],
            ),
            map_scale: "0.8".to_owned(),
            normal_intensity: String::new(),
            blend_intensity: String::new(),
        }
    }
}

/// `AddFloatExtraData`.
pub(crate) fn add_float_extra_data(tree: &mut Tree, block: El, name: &str, value: &str) -> R<bool> {
    let mut result = false;
    if value.is_empty() {
        return Ok(false);
    }
    let extra_data = match block_extra_data_by_name(tree, block, name)? {
        Some(extra_data) => extra_data,
        None => {
            let extra_data = block_add_extra_data(tree, block, "NiFloatExtraData")?;
            tree.set_edit_values(extra_data, "Name", name)?;
            result = true;
            extra_data
        }
    };
    if !same_value(
        df_str_to_float(&tree.edit_values(extra_data, "Float Data")?)?,
        df_str_to_float(value)?,
    ) {
        tree.set_edit_values(extra_data, "Float Data", value)?;
        result = true;
    }
    Ok(result)
}

/// The check of `OnStart`: an empty value or a float.
fn check_float(value: &str, message: &str) -> R<()> {
    if !value.is_empty() && df_str_to_float(value).is_err() {
        return Err(DfError::new(message));
    }
    Ok(())
}

impl Proc for ProcWallsReflectionFlag {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.map_scale = storage.get_string("sMapScale", "0.8");
        self.normal_intensity = storage.get_string("sNormalIntensity", "");
        self.blend_intensity = storage.get_string("sBlendIntensity", "");
    }

    fn on_start(&mut self) -> R<()> {
        check_float(&self.map_scale, "Invalid float number for map scale")?;
        check_float(&self.normal_intensity, "Invalid float number for Normal Intensity")?;
        check_float(&self.blend_intensity, "Invalid float number for Blend Intensity")
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        let mut changed = false;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        for b in blocks_by_type(tree, "NiTriBasedGeom", true)? {
            let Some(shader) = block_property_by_type(tree, b, "BSShaderPPLightingProperty", false)? else {
                continue;
            };
            if !tree
                .native_values(shader, "Shader Flags 1\\Environment_Mapping")?
                .to_bool()?
            {
                continue;
            }
            if tree
                .native_values(shader, "Shader Flags 2\\Envmap_Light_Fade")?
                .to_bool()?
                || !tree.native_values(shader, "Shader Flags 2\\Unknown10")?.to_bool()?
            {
                tree.set_native_values(shader, "Shader Flags 2\\Envmap_Light_Fade", Variant::Int(0))?;
                tree.set_native_values(shader, "Shader Flags 2\\Unknown10", Variant::Int(1))?;
                changed = true;
            }
            if !self.map_scale.is_empty()
                && !same_value(
                    df_str_to_float(&tree.edit_values(shader, "Environment Map Scale")?)?,
                    df_str_to_float(&self.map_scale)?,
                )
            {
                tree.set_edit_values(shader, "Environment Map Scale", &self.map_scale)?;
                changed = true;
            }
            changed = add_float_extra_data(tree, b, "NormalIntensity", &self.normal_intensity)? || changed;
            changed = add_float_extra_data(tree, b, "BlendIntensity", &self.blend_intensity)? || changed;
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
