// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcRagdollConstraintUpdate.pas

//! `Update ragdoll constraint`: sets the motor axes of the ragdoll
//! constraints to the cross product of their twist and plane axes,
//! optionally after converting the constraints to
//! `bhkMalleableConstraint`.

use crate::data_format::{El, R, Tree};
use crate::data_format_nif::{NifFile, block, block_type, blocks_count, convert_block};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, same_value_single,
};
use crate::variant::Variant;

pub struct ProcRagdollConstraintUpdate {
    base: ProcBase,
    /// `chkConvertToMalleable`.
    convert: bool,
}

impl ProcRagdollConstraintUpdate {
    pub fn new() -> ProcRagdollConstraintUpdate {
        ProcRagdollConstraintUpdate {
            base: ProcBase::new(
                "Update ragdoll constraint",
                &[GameType::Fo3, GameType::Fnv, GameType::Tes5, GameType::Sse],
                &["nif"],
            ),
            convert: false,
        }
    }
}

fn native(tree: &mut Tree, el: El, path: &str) -> R<f64> {
    tree.native_values(el, path)?.to_f64()
}

/// `CalcUpdate`: the motor axis as the cross product of the twist and the
/// plane axes. The products are doubles (the native values are) and the
/// axis is single.
fn calc_update(tree: &mut Tree, twist: Option<El>, plane: Option<El>, motor: Option<El>) -> R<bool> {
    let (Some(twist), Some(plane), Some(motor)) = (twist, plane, motor) else {
        return Ok(false);
    };
    let (tx, ty, tz) = (
        native(tree, twist, "X")?,
        native(tree, twist, "Y")?,
        native(tree, twist, "Z")?,
    );
    let (px, py, pz) = (
        native(tree, plane, "X")?,
        native(tree, plane, "Y")?,
        native(tree, plane, "Z")?,
    );
    let x = (ty * pz - tz * py) as f32;
    let y = (tz * px - tx * pz) as f32;
    let z = (tx * py - ty * px) as f32;
    let result = !same_value_single(native(tree, motor, "X")? as f32, x)
        || !same_value_single(native(tree, motor, "Y")? as f32, y)
        || !same_value_single(native(tree, motor, "Z")? as f32, z);
    if result {
        tree.set_native_values(motor, "X", Variant::Float(f64::from(x)))?;
        tree.set_native_values(motor, "Y", Variant::Float(f64::from(y)))?;
        tree.set_native_values(motor, "Z", Variant::Float(f64::from(z)))?;
    }
    Ok(result)
}

impl Proc for ProcRagdollConstraintUpdate {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.convert = storage.get_bool("bConvert", false);
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        for i in 0..blocks_count(tree)? {
            let mut constraint = block(tree, i)?;
            if self.convert
                && matches!(
                    block_type(tree, constraint),
                    "bhkBallAndSocketConstraint"
                        | "bhkHingeConstraint"
                        | "bhkLimitedHingeConstraint"
                        | "bhkPrismaticConstraint"
                        | "bhkRagdollConstraint"
                        | "bhkStiffSpringConstraint"
                )
            {
                convert_block(tree, i, "bhkMalleableConstraint")?;
                constraint = block(tree, i)?;
                changed = true;
            }
            let ragdoll = if block_type(tree, constraint) == "bhkRagdollConstraint" {
                tree.elements(constraint, "Ragdoll")?
            } else if block_type(tree, constraint) == "bhkMalleableConstraint"
                && tree.edit_values(constraint, "Hinge\\Type")? == "Ragdoll"
            {
                tree.elements(constraint, "Hinge\\Ragdoll")?
            } else {
                continue;
            };
            let ragdoll = ragdoll.ok_or_else(access_violation)?;
            for side in ["A", "B"] {
                let twist = tree.elements(ragdoll, &format!("Twist {side}"))?;
                let plane = tree.elements(ragdoll, &format!("Plane {side}"))?;
                let motor = tree.elements(ragdoll, &format!("Motor {side}"))?;
                changed = calc_update(tree, twist, plane, motor)? || changed;
            }
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
