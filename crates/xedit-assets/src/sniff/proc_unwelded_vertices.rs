// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcUnweldedVertices.pas

//! `Find unwelded vertices`: reports the pairs of vertices of a shape that
//! are the same or closer than a distance.

use xedit_core::delphi::str_to_float;

use crate::data_format::{DfError, R, df_float_to_str};
use crate::data_format_nif::{NifFile, block, block_is_ni_object, blocks_count};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage};

/// `TVertex`.
type Vertex = [f32; 3];

pub struct ProcUnweldedVertices {
    base: ProcBase,
    distance_text: String,
    skip_same_checked: bool,
    report_vertices_checked: bool,
    inv_distance: f32,
    skip_same: bool,
    report_vertices: bool,
}

impl ProcUnweldedVertices {
    pub fn new() -> ProcUnweldedVertices {
        let mut base = ProcBase::new("Find unwelded vertices", GameType::ALL, &["nif"]);
        base.no_output = true;
        ProcUnweldedVertices {
            base,
            distance_text: "0.1000".to_owned(),
            skip_same_checked: false,
            report_vertices_checked: false,
            inv_distance: 10.0,
            skip_same: false,
            report_vertices: false,
        }
    }

    /// `UnweldedVerticesCount`.
    fn unwelded_vertices_count(&self, vertices: &[Vertex], log: &mut Vec<String>) -> usize {
        let mut result = 0;
        for i in 0..vertices.len().saturating_sub(1) {
            for j in i + 1..vertices.len() {
                // `CompareMem` of the twelve bytes.
                let same = vertices[i]
                    .iter()
                    .zip(&vertices[j])
                    .all(|(a, b)| a.to_bits() == b.to_bits());
                if self.skip_same && same {
                    continue;
                }
                if same || inv_distance(&vertices[i], &vertices[j]) >= self.inv_distance {
                    if self.report_vertices {
                        let f = |v: f32| df_float_to_str(f64::from(v));
                        log.push(format!(
                            "\t[{i}] ({}, {}, {}) <-> [{j}] ({}, {}, {})",
                            f(vertices[i][0]),
                            f(vertices[i][1]),
                            f(vertices[i][2]),
                            f(vertices[j][0]),
                            f(vertices[j][1]),
                            f(vertices[j][2])
                        ));
                    }
                    result += 1;
                }
            }
        }
        result
    }
}

/// `FastInvSqrt`: one Newton step from the bits of the single; the
/// expression is worked out in double precision and stored as a single.
fn fast_inv_sqrt(value: f32) -> f32 {
    let bits = 0xBE6E_B50Cu32.wrapping_sub(value.to_bits()) >> 1;
    let r = f64::from(f32::from_bits(bits));
    let v = f64::from(value);
    (0.5 * r * (3.0 - v * r * r)) as f32
}

/// `InvDistance`: the squared distance is a single argument.
fn inv_distance(v1: &Vertex, v2: &Vertex) -> f32 {
    let d = |k: usize| f64::from(v2[k]) - f64::from(v1[k]);
    let squared = (d(0) * d(0) + d(1) * d(1) + d(2) * d(2)) as f32;
    fast_inv_sqrt(squared)
}

impl Proc for ProcUnweldedVertices {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.distance_text = storage.get_string("sDistance", "0.1000");
        self.skip_same_checked = storage.get_bool("bSkipSame", false);
        self.report_vertices_checked = storage.get_bool("bReportVertices", false);
    }

    fn on_start(&mut self) -> R<()> {
        self.skip_same = self.skip_same_checked;
        self.report_vertices = self.report_vertices_checked;
        // `1 / StrToFloat`: a division by zero raises.
        match str_to_float(&self.distance_text) {
            Some(distance) if distance != 0.0 => {
                self.inv_distance = (1.0 / distance) as f32;
                Ok(())
            }
            _ => Err(DfError::new("Distance is not a float value or zero")),
        }
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut log: Vec<String> = Vec::new();
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        for i in 0..blocks_count(tree)? {
            let b = block(tree, i)?;
            let mut verts: Vec<Vertex>;
            if block_is_ni_object(tree, b, "NiTriBasedGeomData", true) {
                let Some(entries) = tree.elements(b, "Vertices")? else {
                    continue;
                };
                verts = vec![[0.0; 3]; tree.count(entries) as usize];
                // The bytes of the array as singles.
                let data = tree.save_to_data(entries)?;
                for (index, chunk) in data.chunks_exact(4).enumerate() {
                    if let Some(vertex) = verts.get_mut(index / 3) {
                        vertex[index % 3] = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    }
                }
            } else if block_is_ni_object(tree, b, "BSTriShape", true) {
                let entries = match tree.elements(b, "Vertex Data")? {
                    Some(entries) => Some(entries),
                    None => tree.elements(b, "Vertices")?,
                };
                let Some(entries) = entries else { continue };
                verts = vec![[0.0; 3]; tree.count(entries) as usize];
                for (j, vertex) in verts.iter_mut().enumerate() {
                    let item = tree.item(entries, j as i32)?;
                    let Some(entry) = tree.elements(item, "Vertex")? else {
                        break;
                    };
                    for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
                        vertex[k] = tree.native_values(entry, axis)?.to_f64()? as f32;
                    }
                }
            } else {
                continue;
            }
            let count = self.unwelded_vertices_count(&verts, &mut log);
            if count > 0 {
                let percent = (count as f64 / verts.len() as f64 * 100.0).round_ties_even() as i64;
                log.push(format!("\t{count} unwelded vertices [{percent}%] in {}", tree.name(b)?));
                if self.report_vertices {
                    log.push(String::new());
                }
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
