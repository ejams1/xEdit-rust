// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcAnalyzeMesh.pas

//! `Analyze mesh`: the vertex cache and vertex fetch statistics of the
//! shapes of a mesh (`meshopt_analyzeVertexCache`,
//! `meshopt_analyzeVertexFetch`), per shape and for the whole mesh, with
//! the values under the thresholds left out.

use xedit_core::delphi::float_to_str_f_fixed;

use crate::data_format::{DfError, R, df_str_to_float};
use crate::data_format_nif::{
    NifFile, NifVersion, block, block_get_skin, block_get_triangles, block_is_ni_object, block_type, blocks_count,
};
use crate::mesh_optimize::{analyze_vertex_cache, analyze_vertex_fetch};
use crate::nif_math::{Triangle, round_to, tris2_indices};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation};
use crate::variant::str_to_int;

pub struct ProcAnalyzeMesh {
    base: ProcBase,
    cache_size_text: String,
    per_shape_checked: bool,
    threshold_checked: bool,
    acmr_text: String,
    atvr_text: String,
    vertices_text: String,
    cache_size: i32,
    per_shape: bool,
    threshold: bool,
    acmr: f64,
    atvr: f64,
    vertices: i32,
}

impl ProcAnalyzeMesh {
    pub fn new() -> ProcAnalyzeMesh {
        let mut base = ProcBase::new("Analyze mesh", GameType::ALL, &["nif"]);
        base.no_output = true;
        ProcAnalyzeMesh {
            base,
            cache_size_text: "16".to_owned(),
            per_shape_checked: false,
            threshold_checked: true,
            acmr_text: "1.5".to_owned(),
            atvr_text: "1.5".to_owned(),
            vertices_text: String::new(),
            cache_size: 0,
            per_shape: false,
            threshold: false,
            acmr: 0.0,
            atvr: 0.0,
            vertices: 0,
        }
    }
}

impl Default for ProcAnalyzeMesh {
    fn default() -> Self {
        ProcAnalyzeMesh::new()
    }
}

/// The statistics of one shape.
struct Stat {
    verts: i32,
    tris: i32,
    acmr: f64,
    atvr: f64,
    overfetch: f64,
}

/// `Format('%.1f', ...)`.
fn f1(value: f64) -> String {
    float_to_str_f_fixed(value, 1)
}

impl Proc for ProcAnalyzeMesh {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.cache_size_text = storage.get_string("sCacheSize", &self.cache_size_text);
        self.per_shape_checked = storage.get_bool("bPerShape", self.per_shape_checked);
        self.threshold_checked = storage.get_bool("bThreshold", self.threshold_checked);
        self.acmr_text = storage.get_string("sACMR", &self.acmr_text);
        self.atvr_text = storage.get_string("sATVR", &self.atvr_text);
        self.vertices_text = storage.get_string("sVertices", &self.vertices_text);
    }

    fn on_start(&mut self) -> R<()> {
        self.cache_size = str_to_int(&self.cache_size_text).unwrap_or(0);
        if self.cache_size == 0 || self.cache_size > 128 {
            return Err(DfError::new("Invalid cache size. Default is 16, max 128."));
        }
        self.per_shape = self.per_shape_checked;
        self.threshold = self.threshold_checked;
        self.acmr = if self.acmr_text.is_empty() {
            0.0
        } else {
            df_str_to_float(&self.acmr_text)?
        };
        self.atvr = if self.atvr_text.is_empty() {
            0.0
        } else {
            df_str_to_float(&self.atvr_text)?
        };
        self.vertices = if self.vertices_text.is_empty() {
            0
        } else {
            str_to_int(&self.vertices_text).unwrap_or(0)
        };
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        let mut log: Vec<String> = Vec::new();
        let mut stats: Vec<Stat> = Vec::new();
        let mut analyze = |log: &mut Vec<String>, name: String, tris: &[Triangle], numverts: i32| {
            if tris.is_empty() {
                return;
            }
            let numtris = tris.len() as i32;
            let indices = tris2_indices(tris);
            // A cache size above zero: `OnStart` checks it.
            let vc = analyze_vertex_cache(&indices, self.cache_size as u32, 0, 0);
            let vf = analyze_vertex_fetch(&indices, 12);
            if self.per_shape {
                let acmr = round_to(vc.acmr, -1);
                let atvr = round_to(vc.atvr, -1);
                let overfetch = round_to(vf.overfetch, -1);
                if !self.threshold || acmr > self.acmr || atvr > self.atvr || numverts > self.vertices {
                    log.push(format!(
                        "\t{name}: Vertices: {numverts}    Triangles: {numtris}    ACMR: {}    ATVR: {}    Overfetch: {}",
                        f1(acmr),
                        f1(atvr),
                        f1(overfetch)
                    ));
                }
            }
            stats.push(Stat {
                verts: numverts,
                tris: numtris,
                acmr: vc.acmr,
                atvr: vc.atvr,
                overfetch: vf.overfetch,
            });
        };
        let version = tree.nif.nif_version;
        for i in 0..blocks_count(tree)? {
            let b = block(tree, i)?;
            let this_type = block_type(tree, b);
            if this_type == "NiTriShape" || this_type == "NiTriStrips" {
                let data_ref = tree.elements(b, "Data")?.ok_or_else(access_violation)?;
                let data = tree.links_to(data_ref)?;
                // Skinned shapes are not rendered, they only store vertices. Rendered tris are in skin partitions.
                if let Some(data) = data
                    && block_get_skin(tree, b)?.is_none()
                {
                    let name = tree.name(data)?;
                    let tris = block_get_triangles(tree, data, None)?;
                    let verts = tree.native_values(data, "Num Vertices")?.to_i64()? as i32;
                    analyze(&mut log, name, &tris, verts);
                }
            } else if block_is_ni_object(tree, b, "BSTriShape", true) {
                // FO4 skins don't have partitions
                if version >= NifVersion::Fo4 || block_get_skin(tree, b)?.is_none() {
                    let name = tree.name(b)?;
                    let tris = block_get_triangles(tree, b, None)?;
                    let verts = tree.native_values(b, "Num Vertices")?.to_i64()? as i32;
                    analyze(&mut log, name, &tris, verts);
                }
            } else if this_type == "NiSkinPartition" {
                let parts = tree.elements(b, "Partitions")?.ok_or_else(access_violation)?;
                for p in 0..tree.count(parts) {
                    let part = tree.item(parts, p)?;
                    let name = tree.path(part)?;
                    let tris = block_get_triangles(tree, b, Some(part))?;
                    let verts = tree.native_values(part, "Num Vertices")?.to_i64()? as i32;
                    analyze(&mut log, name, &tris, verts);
                }
            }
        }
        // summary for the mesh
        let verts: i32 = stats.iter().map(|s| s.verts).fold(0, i32::wrapping_add);
        let tris: i32 = stats.iter().map(|s| s.tris).fold(0, i32::wrapping_add);
        let (mut acmr, mut atvr, mut overfetch) = (0f64, 0f64, 0f64);
        for s in &stats {
            let weight = f64::from(s.tris) / f64::from(tris);
            acmr += weight * s.acmr;
            atvr += weight * s.atvr;
            overfetch += weight * s.overfetch;
        }
        let acmr = round_to(acmr, -1);
        let atvr = round_to(atvr, -1);
        let overfetch = round_to(overfetch, -1);
        if !self.threshold || acmr > self.acmr || atvr > self.atvr || verts > self.vertices {
            log.push(format!(
                "\tVertices: {verts}    Triangles: {tris}    ACMR: {}    ATVR: {}    Overfetch: {}",
                f1(acmr),
                f1(atvr),
                f1(overfetch)
            ));
        }
        if !log.is_empty() {
            log.insert(0, file.file_name.clone());
            log.push(String::new());
            ctx.add_messages(log);
        }
        Ok(Vec::new())
    }
}
