// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcInertiaUpdate.pas

//! `Update Havok inertia`: sets the inertia tensor, the center of mass and
//! the penetration depth of the rigid bodies from their shapes (box,
//! sphere, capsule, convex or MOPP shape), with a multiplier per body part.
//!
//! The variables of upstream are singles; Delphi on Win64 works out an
//! expression of singles in double precision and rounds it when it is
//! stored (`{$EXCESSPRECISION ON}`), and the arithmetic of variants is in
//! double precision too, which the port follows with `f64` expressions
//! cast to `f32` at each assignment.

use crate::data_format::{DfError, El, R, Tree};
use crate::data_format_nif::{
    NifFile, block_get_vertices, block_is_dynamic_rigid_body, block_is_ni_object, block_type, blocks_by_type, hk2gu,
};
use crate::nif_math::{Quaternion, Transform, Vector3, calculate_center_radius, calculate_min_max};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, comma_text, comma_text_of,
    same_value_single,
};
use crate::variant::{Variant, str_to_int};

/// The rows of `edMult`: the body parts and their default multipliers.
const DEFAULT_MULT: [&str; 29] = [
    "0 Other=",
    "1 Head=2",
    "2 Body=3",
    "3 Spine1=3",
    "4 Spine2=3",
    "5 LUpperArm=2",
    "6 LForeArm=",
    "7 LHand=",
    "8 LThigh=2",
    "9 LCalf=",
    "10 LFoot=",
    "11 RUpperArm=2",
    "12 RForeArm=",
    "13 RHand=",
    "14 RThigh=2",
    "15 RCalf=",
    "16 RFoot=",
    "17 Tail=",
    "18 Shield=",
    "19 Quiver=",
    "20 Weapon=",
    "21 PonyTail=",
    "22 Wing=",
    "23 Pack=",
    "24 Chain=",
    "25 AddonHead=",
    "26 AddonChest=",
    "27 AddonArm=",
    "28 AddonLeg=",
];

pub struct ProcInertiaUpdate {
    base: ProcBase,
    inertia_checked: bool,
    center_checked: bool,
    penetration_checked: bool,
    penetration_statics_checked: bool,
    depth_mult_text: String,
    /// The lines of `edMult`.
    mult_lines: Vec<String>,
    inertia: bool,
    center: bool,
    penetration: bool,
    penetration_statics: bool,
    depth_mult: f32,
    /// `fMult`: the body part and its multiplier.
    mult: Vec<(i32, f32)>,
}

impl ProcInertiaUpdate {
    pub fn new() -> ProcInertiaUpdate {
        ProcInertiaUpdate {
            base: ProcBase::new(
                "Update Havok inertia",
                &[
                    GameType::Tes4,
                    GameType::Fo3,
                    GameType::Fnv,
                    GameType::Tes5,
                    GameType::Sse,
                ],
                &["nif"],
            ),
            inertia_checked: true,
            center_checked: true,
            penetration_checked: false,
            penetration_statics_checked: false,
            depth_mult_text: "0.2".to_owned(),
            mult_lines: DEFAULT_MULT.iter().map(|line| (*line).to_owned()).collect(),
            inertia: true,
            center: true,
            penetration: false,
            penetration_statics: false,
            depth_mult: 0.2,
            mult: Vec::new(),
        }
    }
}

/// `StrToFloatDef`.
fn str_to_float_def(text: &str, default: f64) -> f64 {
    xedit_core::delphi::str_to_float(text).unwrap_or(default)
}

fn native(tree: &mut Tree, el: El, path: &str) -> R<f64> {
    tree.native_values(el, path)?.to_f64()
}

/// The state of one rigid body for `SetInertia` and the others.
struct Body {
    dynamic: bool,
    penetration_statics: bool,
}

impl Body {
    /// `SetInertia`.
    fn set_inertia(&self, tree: &mut Tree, inertia: Option<El>, m11: f32, m22: f32, m33: f32) -> R<bool> {
        let Some(inertia) = inertia.filter(|_| self.dynamic) else {
            return Ok(false);
        };
        set_three(tree, inertia, ["m11", "m22", "m33"], [m11, m22, m33])
    }

    /// `SetCenter`.
    fn set_center(&self, tree: &mut Tree, center: Option<El>, x: f32, y: f32, z: f32) -> R<bool> {
        let Some(center) = center.filter(|_| self.dynamic) else {
            return Ok(false);
        };
        set_three(tree, center, ["X", "Y", "Z"], [x, y, z])
    }

    /// `SetPenetration`.
    fn set_penetration(&self, tree: &mut Tree, depth: Option<El>, mut d: f32) -> R<bool> {
        if !self.dynamic && !self.penetration_statics {
            return Ok(false);
        }
        let Some(depth) = depth else { return Ok(false) };
        if same_value_single(d, 0.0) {
            d = 0.04;
        }
        let result = !same_value_single(tree.native_value(depth)?.to_f64()? as f32, d);
        if result {
            tree.set_native_value(depth, Variant::Float(f64::from(d)))?;
        }
        Ok(result)
    }
}

/// Three values that change together when one differs.
fn set_three(tree: &mut Tree, el: El, names: [&str; 3], values: [f32; 3]) -> R<bool> {
    let mut result = false;
    for (name, value) in names.iter().zip(values) {
        if !same_value_single(native(tree, el, name)? as f32, value) {
            result = true;
            break;
        }
    }
    if result {
        for (name, value) in names.iter().zip(values) {
            tree.set_native_values(el, name, Variant::Float(f64::from(value)))?;
        }
    }
    Ok(result)
}

/// The inertia of a box of the size: `m * (b² + c²) / 12` for each axis,
/// times the multiplier.
fn box_inertia(m: f32, x: f32, y: f32, z: f32, mult: f32) -> [f32; 3] {
    let (m, x, y, z) = (f64::from(m), f64::from(x), f64::from(y), f64::from(z));
    let m11 = (m * (y * y + z * z) / 12.0) as f32;
    let m22 = (m * (x * x + z * z) / 12.0) as f32;
    let m33 = (m * (x * x + y * y) / 12.0) as f32;
    let mult = f64::from(mult);
    [
        (f64::from(m11) * mult) as f32,
        (f64::from(m22) * mult) as f32,
        (f64::from(m33) * mult) as f32,
    ]
}

impl Proc for ProcInertiaUpdate {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.inertia_checked = storage.get_bool("bInertiaUpdate", true);
        self.center_checked = storage.get_bool("bCenterUpdate", true);
        self.penetration_checked = storage.get_bool("bPenetrationUpdate", false);
        self.penetration_statics_checked = storage.get_bool("bPenetrationStaticsUpdate", false);
        self.depth_mult_text = storage.get_string("sDepthMult", "0.2");
        let default: Vec<String> = DEFAULT_MULT.iter().map(|line| (*line).to_owned()).collect();
        self.mult_lines = comma_text(&storage.get_string("sMult", &comma_text_of(&default)));
    }

    fn on_start(&mut self) -> R<()> {
        self.inertia = self.inertia_checked;
        self.center = self.center_checked;
        self.penetration = self.penetration_checked;
        self.penetration_statics = self.penetration_statics_checked;
        if !(self.inertia || self.center || self.penetration) {
            return Err(DfError::new("No update options selected"));
        }
        self.depth_mult = str_to_float_def(&self.depth_mult_text, 0.2) as f32;
        if same_value_single(self.depth_mult, 0.0) {
            self.depth_mult = 0.2;
        }
        self.mult.clear();
        for line in &self.mult_lines {
            // `KeyNames` and `ValueFromIndex`.
            let (mut key, value) = match line.find('=') {
                Some(index) => (line[..index].to_owned(), line[index + 1..].to_owned()),
                None => (String::new(), line.chars().skip(1).collect()),
            };
            if let Some(space) = key.find(' ') {
                key.truncate(space);
            }
            let Some(part) = str_to_int(&key).filter(|&part| part != -1) else {
                continue;
            };
            let v = str_to_float_def(&value, -1.0);
            if v < 0.0 {
                continue;
            }
            self.mult.push((part, v as f32));
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        let nif_version = tree.nif.nif_version;

        for rigid in blocks_by_type(tree, "bhkRigidBody", true)? {
            let link = tree.elements(rigid, "Shape")?.ok_or_else(access_violation)?;
            let Some(mut shape) = tree.links_to(link)? else {
                continue;
            };
            let body = Body {
                dynamic: block_is_dynamic_rigid_body(tree, rigid)?,
                penetration_statics: self.penetration_statics,
            };

            // The transform shapes on the way.
            let (mut tx, mut ty, mut tz) = (0f32, 0f32, 0f32);
            let mut found = Some(shape);
            while let Some(current) = found {
                if !block_is_ni_object(tree, current, "bhkTransformShape", true) {
                    break;
                }
                tx = (f64::from(tx) + native(tree, current, "Transform\\m14")?) as f32;
                ty = (f64::from(ty) + native(tree, current, "Transform\\m24")?) as f32;
                tz = (f64::from(tz) + native(tree, current, "Transform\\m34")?) as f32;
                let link = tree.elements(current, "Shape")?.ok_or_else(access_violation)?;
                found = tree.links_to(link)?;
            }
            match found {
                Some(current) => shape = current,
                None => continue,
            }

            let inertia = tree.elements(rigid, "Inertia Tensor")?;
            let center = tree.elements(rigid, "Center")?;
            let depth = tree.elements(rigid, "Penetration Depth")?;
            let m = native(tree, rigid, "Mass")? as f32;
            let body_part = tree
                .native_values(rigid, "Havok Filter\\Flags and Part Number")?
                .to_i32()?;
            let mult = self
                .mult
                .iter()
                .find(|(part, _)| *part == body_part)
                .map_or(1.0, |(_, mult)| *mult);
            let (md, dm_depth) = (f64::from(m), f64::from(self.depth_mult));

            match block_type(tree, shape) {
                "bhkBoxShape" => {
                    let x = native(tree, shape, "Dimensions\\X")? as f32;
                    let y = native(tree, shape, "Dimensions\\Y")? as f32;
                    let z = native(tree, shape, "Dimensions\\Z")? as f32;
                    let [m11, m22, m33] = box_inertia(m, x, y, z, mult);
                    if self.inertia {
                        changed = body.set_inertia(tree, inertia, m11, m22, m33)? || changed;
                    }
                    if self.center {
                        changed = body.set_center(tree, center, tx, ty, tz)? || changed;
                    }
                    if self.penetration {
                        // `MinValue` of singles.
                        let mut min = x;
                        if min > y {
                            min = y;
                        }
                        if min > z {
                            min = z;
                        }
                        let d = (f64::from(min) * dm_depth) as f32;
                        changed = body.set_penetration(tree, depth, d)? || changed;
                    }
                }
                "bhkSphereShape" => {
                    let r = f64::from(native(tree, shape, "Radius")? as f32);
                    let m11 = ((2.0 * md) * r * r / 5.0) as f32;
                    let m11 = (f64::from(m11) * f64::from(mult)) as f32;
                    if self.inertia {
                        changed = body.set_inertia(tree, inertia, m11, m11, m11)? || changed;
                    }
                    if self.center {
                        changed = body.set_center(tree, center, tx, ty, tz)? || changed;
                    }
                    if self.penetration {
                        changed = body.set_penetration(tree, depth, (2.0 * r * dm_depth) as f32)? || changed;
                    }
                }
                "bhkCapsuleShape" => {
                    let r = f64::from(native(tree, shape, "Radius")? as f32);
                    let delta = |tree: &mut Tree, axis: &str| -> R<f32> {
                        let first = native(tree, shape, &format!("First Point\\{axis}"))?;
                        let second = native(tree, shape, &format!("Second Point\\{axis}"))?;
                        Ok((first - second).abs() as f32)
                    };
                    let lx = delta(tree, "X")?;
                    let ly = delta(tree, "Y")?;
                    let lz = delta(tree, "Z")?;
                    let along = |l: f32| {
                        // `Sqr`.
                        let t = f64::from(l) + 2.0 * r;
                        (md * r * r / 4.0 + md * (t * t) / 12.0) as f32
                    };
                    let across = (md * r * r / 2.0) as f32;
                    let (m11, m22, m33) = if lx >= ly && lx >= lz {
                        let m22 = along(lx);
                        (across, m22, m22)
                    } else if ly >= lx && ly >= lz {
                        let m11 = along(ly);
                        (m11, across, m11)
                    } else {
                        let m11 = along(lz);
                        (m11, m11, across)
                    };
                    let mult = f64::from(mult);
                    let (m11, m22, m33) = (
                        (f64::from(m11) * mult) as f32,
                        (f64::from(m22) * mult) as f32,
                        (f64::from(m33) * mult) as f32,
                    );
                    if self.inertia {
                        changed = body.set_inertia(tree, inertia, m11, m22, m33)? || changed;
                    }
                    if self.center {
                        let middle = |tree: &mut Tree, axis: &str| -> R<f32> {
                            let first = native(tree, shape, &format!("First Point\\{axis}"))?;
                            let second = native(tree, shape, &format!("Second Point\\{axis}"))?;
                            Ok(((first + second) / 2.0) as f32)
                        };
                        let x = middle(tree, "X")?;
                        let y = middle(tree, "Y")?;
                        let z = middle(tree, "Z")?;
                        let add = |a: f32, b: f32| (f64::from(a) + f64::from(b)) as f32;
                        changed = body.set_center(tree, center, add(x, tx), add(y, ty), add(z, tz))? || changed;
                    }
                    if self.penetration {
                        changed = body.set_penetration(tree, depth, (2.0 * r * dm_depth) as f32)? || changed;
                    }
                }
                "bhkConvexVerticesShape" | "bhkMoppBvTreeShape" => {
                    // `var dm := fDepthMult` is a Double: the inline
                    // variable takes the type of the expression, which
                    // excess precision makes a Double (found by parity
                    // sniff: the penetration depth of Oblivion's MOPP
                    // shapes differs by an ulp when it is a Single).
                    let mut dm = f64::from(self.depth_mult);
                    let mut verts: Vec<Vector3> = Vec::new();
                    if block_type(tree, shape) == "bhkConvexVerticesShape" {
                        verts = block_get_vertices(tree, shape, None)?;
                    } else {
                        let link = tree.elements(shape, "Shape")?.ok_or_else(access_violation)?;
                        let Some(sub_shape) = tree.links_to(link)? else {
                            continue;
                        };
                        let Some(data_link) = tree.elements(sub_shape, "Data")? else {
                            continue;
                        };
                        let Some(data) = tree.links_to(data_link)? else {
                            continue;
                        };
                        if block_type(tree, data) == "hkPackedNiTriStripsData" {
                            verts = block_get_vertices(tree, data, None)?;
                            // The vertices of strips data are in game units.
                            dm /= f64::from(hk2gu(nif_version));
                        } else if block_type(tree, data) == "bhkCompressedMeshShapeData" {
                            let big_verts = tree.elements(data, "Big Verts")?.ok_or_else(access_violation)?;
                            for i in 0..tree.count(big_verts) {
                                let v = tree.item(big_verts, i)?;
                                verts.push(Vector3::new(
                                    native(tree, v, "X")?,
                                    native(tree, v, "Y")?,
                                    native(tree, v, "Z")?,
                                ));
                            }
                            let transforms = tree.elements(data, "Chunk Transforms")?.ok_or_else(access_violation)?;
                            let chunks = tree.elements(data, "Chunks")?.ok_or_else(access_violation)?;
                            for i in 0..tree.count(chunks) {
                                let chunk = tree.item(chunks, i)?;
                                let el = tree.elements(chunk, "Vertices")?.ok_or_else(access_violation)?;
                                if tree.count(el) < 3 {
                                    continue;
                                }
                                let tindex = tree.native_values(chunk, "Transform Index")?.to_i64()? as i8;
                                let mut t = Transform::default();
                                if tindex != -1 {
                                    let chunk_t = tree.item(transforms, i32::from(tindex))?;
                                    let mut translation = [0.0; 3];
                                    for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
                                        translation[k] = native(tree, chunk_t, &format!("Translation\\{axis}"))?
                                            + native(tree, chunk, &format!("Offset\\{axis}"))?;
                                    }
                                    t.translation = Vector3::new(translation[0], translation[1], translation[2]);
                                    t.rotation = Quaternion {
                                        q: [
                                            native(tree, chunk_t, "Rotation\\W")?,
                                            native(tree, chunk_t, "Rotation\\X")?,
                                            native(tree, chunk_t, "Rotation\\Y")?,
                                            native(tree, chunk_t, "Rotation\\Z")?,
                                        ],
                                    };
                                    t.scale = 1.0;
                                }
                                for j in 0..tree.count(el) / 3 {
                                    let mut c = [0.0; 3];
                                    for (k, value) in c.iter_mut().enumerate() {
                                        let item = tree.item(el, 3 * j + k as i32)?;
                                        *value = tree.native_value(item)?.to_f64()?;
                                    }
                                    let mut v = Vector3::new(c[0], c[1], c[2]) / 1000.0;
                                    if tindex != -1 {
                                        v = v * t;
                                    }
                                    verts.push(v);
                                }
                            }
                        }
                    }
                    if verts.is_empty() {
                        continue;
                    }
                    // A box for the inertia.
                    let (v1, v2) = calculate_min_max(&verts);
                    let x = (v2.x() - v1.x()) as f32;
                    let y = (v2.y() - v1.y()) as f32;
                    let z = (v2.z() - v1.z()) as f32;
                    let [m11, m22, m33] = box_inertia(m, x, y, z, mult);
                    let (c, r_min) = calculate_center_radius(&verts, true, false);
                    if self.inertia {
                        changed = body.set_inertia(tree, inertia, m11, m22, m33)? || changed;
                    }
                    if self.center {
                        let add = |a: f32, b: f64| (f64::from(a) + b) as f32;
                        changed =
                            body.set_center(tree, center, add(tx, c.x()), add(ty, c.y()), add(tz, c.z()))? || changed;
                    }
                    if self.penetration {
                        let d = (2.0 * r_min * dm) as f32;
                        changed = body.set_penetration(tree, depth, d)? || changed;
                    }
                }
                _ => {}
            }
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
