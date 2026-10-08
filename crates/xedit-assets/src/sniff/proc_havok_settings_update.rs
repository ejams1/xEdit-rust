// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcHavokSettingsUpdate.pas

//! `Update Havok settings`: sets the material, radius and layer of the
//! Havok shapes and the settings of the rigid bodies; an empty setting is
//! left as it is.

use xedit_core::delphi::str_to_float;

use crate::data_format::{DfError, El, R, Tree, df_float_to_str};
use crate::data_format_nif::{NifFile, block, block_is_ni_object, block_type, blocks_count};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, trim};
use crate::variant::Variant;

/// The rows of `edSettings`: the name of the row and of its setting.
const ROWS: [(&str, &str); 14] = [
    ("Material", "sMaterial"),
    ("Radius", "sRadius"),
    ("Layer", "sLayer"),
    ("Mass", "sMass"),
    ("Linear Damping", "sLinearDamping"),
    ("Angular Damping", "sAngularDamping"),
    ("Max Linear Velocity", "sMaxLinearVelocity"),
    ("Max Angular Velocity", "sMaxAngularVelocity"),
    ("Friction", "sFriction"),
    ("Restitution", "sRestitution"),
    ("Motion System", "sMotionSystem"),
    ("Deactivator Type", "sDeactivatorType"),
    ("Solver Deactivation", "sSolverDeactivation"),
    ("Motion Quality", "sMotionQuality"),
];

pub struct ProcHavokSettingsUpdate {
    base: ProcBase,
    /// The values of `edSettings`, in the order of `ROWS`.
    values: [String; 14],
    /// The settings of the run, in the order of `ROWS`; the floats in the
    /// form of `dfFloatToStr`.
    settings: [String; 14],
}

impl ProcHavokSettingsUpdate {
    pub fn new() -> ProcHavokSettingsUpdate {
        ProcHavokSettingsUpdate {
            base: ProcBase::new(
                "Update Havok settings",
                &[
                    GameType::Tes4,
                    GameType::Fo3,
                    GameType::Fnv,
                    GameType::Tes5,
                    GameType::Sse,
                ],
                &["nif"],
            ),
            values: Default::default(),
            settings: Default::default(),
        }
    }

    fn setting(&self, row: &str) -> &str {
        let index = ROWS.iter().position(|(name, _)| *name == row).unwrap_or(0);
        &self.settings[index]
    }
}

/// `UpdateField`: counts a change when the text differs, whatever the
/// value becomes.
fn update_field(tree: &mut Tree, el: Option<El>, value: &str, changed: &mut bool) -> R<()> {
    let Some(el) = el else { return Ok(()) };
    if value.is_empty() {
        return Ok(());
    }
    if tree.edit_value(el)? != value {
        tree.set_edit_value(el, value)?;
        *changed = true;
    }
    Ok(())
}

impl Proc for ProcHavokSettingsUpdate {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        for (index, (_, key)) in ROWS.iter().enumerate() {
            self.values[index] = storage.get_string(key, "");
        }
    }

    fn on_start(&mut self) -> R<()> {
        const FLOATS: [&str; 8] = [
            "Radius",
            "Mass",
            "Linear Damping",
            "Angular Damping",
            "Max Linear Velocity",
            "Max Angular Velocity",
            "Friction",
            "Restitution",
        ];
        for (index, (name, _)) in ROWS.iter().enumerate() {
            self.settings[index] = if FLOATS.contains(name) {
                // `GetFloat`.
                let value = trim(&self.values[index]).to_owned();
                if value.is_empty() {
                    value
                } else {
                    match str_to_float(&value) {
                        Some(float) => df_float_to_str(float),
                        None => return Err(DfError::new(format!("{name} is not a float value"))),
                    }
                }
            } else {
                self.values[index].clone()
            };
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
            if block_is_ni_object(tree, b, "bhkShape", true) {
                for (path, row) in [
                    ("Material", "Material"),
                    ("Radius", "Radius"),
                    ("Radius Copy", "Radius"),
                ] {
                    let el = tree.elements(b, path)?;
                    update_field(tree, el, self.setting(row), &mut changed)?;
                }
                if let Some(filters) = tree.elements(b, "Filters")? {
                    for index in 0..tree.count(filters) {
                        let filter = tree.item(filters, index)?;
                        let el = tree.elements(filter, "Layer")?;
                        update_field(tree, el, self.setting("Layer"), &mut changed)?;
                    }
                }
            } else if matches!(
                block_type(tree, b),
                "hkPackedNiTriStripsData" | "bhkCompressedMeshShapeData"
            ) {
                let sub_shapes = match tree.elements(b, "Sub Shapes")? {
                    Some(sub_shapes) => Some(sub_shapes),
                    None => tree.elements(b, "Chunk Materials")?,
                };
                let Some(sub_shapes) = sub_shapes else { continue };
                for index in 0..tree.count(sub_shapes) {
                    let sub_shape = tree.item(sub_shapes, index)?;
                    let el = tree.elements(sub_shape, "Material")?;
                    update_field(tree, el, self.setting("Material"), &mut changed)?;
                }
            } else if block_is_ni_object(tree, b, "bhkRigidBody", true) {
                for (path, row) in [
                    ("Havok Filter\\Layer", "Layer"),
                    ("Havok Filter Copy\\Layer", "Layer"),
                    ("Mass", "Mass"),
                ] {
                    let el = tree.elements(b, path)?;
                    update_field(tree, el, self.setting(row), &mut changed)?;
                }
                // UPSTREAM-QUIRK: a change anywhere before in the file, not
                // only of the mass, resets the inertia of a massless body.
                if changed && tree.native_values(b, "Mass")?.to_f64()? == 0.0 {
                    for k in 0..12 {
                        tree.set_native_values(b, &format!("Inertia Tensor\\[{k}]"), Variant::Int(0))?;
                    }
                }
                for row in [
                    "Linear Damping",
                    "Angular Damping",
                    "Max Linear Velocity",
                    "Max Angular Velocity",
                    "Friction",
                    "Restitution",
                    "Motion System",
                    "Deactivator Type",
                    "Solver Deactivation",
                    "Motion Quality",
                ] {
                    let el = tree.elements(b, row)?;
                    update_field(tree, el, self.setting(row), &mut changed)?;
                }
            }
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
