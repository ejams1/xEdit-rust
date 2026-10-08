// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcTangents.pas

//! `Update tangents and binormals`: recalculates the tangents and
//! binormals of `BSTriShape`, `NiTriShapeData`, `NiTriStripsData` and
//! `NiSkinPartition`, optionally after the normals.

use crate::data_format::R;
use crate::data_format_nif::{NifFile, spell_add_update_tangents, spell_face_normals, spell_update_tangents};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage};

pub struct ProcTangents {
    base: ProcBase,
    /// `chkAddIfMissing`.
    add_if_missing: bool,
    /// `chkNormals`.
    face_normals: bool,
}

impl ProcTangents {
    pub fn new() -> ProcTangents {
        ProcTangents {
            base: ProcBase::new(
                "Update tangents and binormals",
                &[
                    GameType::Tes4,
                    GameType::Fo3,
                    GameType::Fnv,
                    GameType::Tes5,
                    GameType::Sse,
                    GameType::Fo4,
                ],
                &["nif"],
            ),
            add_if_missing: false,
            face_normals: false,
        }
    }
}

impl Proc for ProcTangents {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.add_if_missing = storage.get_bool("bAddIfMissing", false);
        self.face_normals = storage.get_bool("bFaceNormals", false);
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        if self.face_normals {
            changed = spell_face_normals(tree)?;
        }
        // "or" afterwards so the tangents run when the normals changed.
        changed = if self.add_if_missing {
            spell_add_update_tangents(tree)? || changed
        } else {
            spell_update_tangents(tree)? || changed
        };
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
