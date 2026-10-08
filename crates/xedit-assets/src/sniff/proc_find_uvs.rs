// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcFindUVs.pas

//! `Find UVs`: reports the shapes with texture coordinates outside of the
//! limits given.

use xedit_core::delphi::str_to_float;

use crate::data_format::{DfError, R, df_float_to_str};
use crate::data_format_nif::{NifFile, block, block_get_tex_coord, blocks_count};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, trim};

pub struct ProcFindUVs {
    base: ProcBase,
    texts: [String; 4],
    u_min: String,
    u_max: String,
    v_min: String,
    v_max: String,
}

impl ProcFindUVs {
    pub fn new() -> ProcFindUVs {
        let mut base = ProcBase::new("Find UVs", GameType::ALL, &["nif"]);
        base.no_output = true;
        ProcFindUVs {
            base,
            texts: ["0".to_owned(), String::new(), "0".to_owned(), String::new()],
            u_min: String::new(),
            u_max: String::new(),
            v_min: String::new(),
            v_max: String::new(),
        }
    }
}

impl Proc for ProcFindUVs {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.texts = [
            storage.get_string("sUMin", "0"),
            storage.get_string("sUMax", ""),
            storage.get_string("sVMin", "0"),
            storage.get_string("sVMax", ""),
        ];
    }

    fn on_start(&mut self) -> R<()> {
        self.u_min = trim(&self.texts[0]).to_owned();
        self.u_max = trim(&self.texts[1]).to_owned();
        self.v_min = trim(&self.texts[2]).to_owned();
        self.v_max = trim(&self.texts[3]).to_owned();
        let values = [&self.u_min, &self.u_max, &self.v_min, &self.v_max];
        if values.iter().all(|value| value.is_empty()) {
            return Err(DfError::new("Fill in at least one value"));
        }
        if values
            .iter()
            .any(|value| !value.is_empty() && str_to_float(value).is_none())
        {
            return Err(DfError::new("Invalid float value"));
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let def = |text: &str| str_to_float(text).unwrap_or(0.0);
        let (u_min, u_max, v_min, v_max) = (def(&self.u_min), def(&self.u_max), def(&self.v_min), def(&self.v_max));
        let mut log: Vec<String> = Vec::new();
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        for i in 0..blocks_count(tree)? {
            let b = block(tree, i)?;
            let uvs = block_get_tex_coord(tree, b, None)?;
            if uvs.is_empty() {
                continue;
            }
            let (mut x_min, mut x_max, mut y_min, mut y_max) = (u_min, u_max, v_min, v_max);
            for uv in uvs {
                let (x, y) = (uv.v[0], uv.v[1]);
                if !self.u_min.is_empty() && x < x_min {
                    x_min = x;
                }
                if !self.u_max.is_empty() && x > x_max {
                    x_max = x;
                }
                if !self.v_min.is_empty() && y < y_min {
                    y_min = y;
                }
                if !self.v_max.is_empty() && y > y_max {
                    y_max = y;
                }
            }
            let mut s = String::new();
            if !self.u_min.is_empty() && x_min < u_min {
                s.push_str(&format!("Umin: {}\t", df_float_to_str(x_min)));
            }
            if !self.v_min.is_empty() && y_min < v_min {
                s.push_str(&format!("Vmin: {}\t", df_float_to_str(y_min)));
            }
            if !self.u_max.is_empty() && x_max > u_max {
                s.push_str(&format!("Umax: {}\t", df_float_to_str(x_max)));
            }
            if !self.v_max.is_empty() && y_max > v_max {
                s.push_str(&format!("Vmax: {}\t", df_float_to_str(y_max)));
            }
            if !s.is_empty() {
                log.push(format!("\t{}: {}", tree.name(b)?, trim(&s)));
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
