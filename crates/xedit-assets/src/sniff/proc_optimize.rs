// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcOptimize.pas

//! `Optimize mesh`: the shapes of a mesh run through `SpellOptimize` with
//! the optimizations of the frame (vertex cache, overdraw, vertex fetch,
//! triangulate or stripify).

use crate::data_format::{DfError, R};
use crate::data_format_nif::{MeshOptimizeOption, MeshOptimizeOptions, NifFile, spell_optimize};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage};

pub struct ProcOptimize {
    base: ProcBase,
    triangulate_checked: bool,
    stripify_checked: bool,
    vertex_cache_checked: bool,
    overdraw_checked: bool,
    vertex_fetch_checked: bool,
    triangulate: bool,
    stripify: bool,
    vertex_cache: bool,
    overdraw: bool,
    vertex_fetch: bool,
}

impl ProcOptimize {
    pub fn new() -> ProcOptimize {
        ProcOptimize {
            base: ProcBase::new("Optimize mesh", GameType::ALL, &["nif"]),
            triangulate_checked: false,
            stripify_checked: false,
            vertex_cache_checked: true,
            overdraw_checked: true,
            vertex_fetch_checked: true,
            triangulate: false,
            stripify: false,
            vertex_cache: false,
            overdraw: false,
            vertex_fetch: false,
        }
    }
}

impl Default for ProcOptimize {
    fn default() -> Self {
        ProcOptimize::new()
    }
}

impl Proc for ProcOptimize {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        // Checking one of the two boxes unchecks the other
        // (`chkTriangulateClick`, `chkStripifyClick`), and setting `Checked`
        // fires the click.
        self.triangulate_checked = storage.get_bool("bTriangulate", self.triangulate_checked);
        if self.triangulate_checked {
            self.stripify_checked = false;
        }
        self.stripify_checked = storage.get_bool("bStripify", self.stripify_checked);
        if self.stripify_checked {
            self.triangulate_checked = false;
        }
        self.vertex_cache_checked = storage.get_bool("bVertexCache", self.vertex_cache_checked);
        self.overdraw_checked = storage.get_bool("bOverdraw", self.overdraw_checked);
        self.vertex_fetch_checked = storage.get_bool("bVertexFetch", self.vertex_fetch_checked);
    }

    fn on_start(&mut self) -> R<()> {
        self.triangulate = self.triangulate_checked;
        self.stripify = self.stripify_checked;
        self.vertex_cache = self.vertex_cache_checked;
        self.overdraw = self.overdraw_checked;
        self.vertex_fetch = self.vertex_fetch_checked;
        if !(self.triangulate || self.stripify || self.vertex_cache || self.overdraw || self.vertex_fetch) {
            return Err(DfError::new("Select at least one optimization"));
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        // UPSTREAM-QUIRK: `Options` is a local set that is never cleared
        // before the selected options are added; the port starts it empty.
        let mut options = MeshOptimizeOptions::default();
        for (selected, option) in [
            (self.triangulate, MeshOptimizeOption::Triangulate),
            (self.stripify, MeshOptimizeOption::Stripify),
            (self.vertex_cache, MeshOptimizeOption::VertexCache),
            (self.overdraw, MeshOptimizeOption::Overdraw),
            (self.vertex_fetch, MeshOptimizeOption::VertexFetch),
        ] {
            if selected {
                options.include(option);
            }
        }
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        if spell_optimize(&mut nif.tree, options)? {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
