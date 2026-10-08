// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcCopyPriorities.pas

//! `Copy anim priorities`: copies the priorities of the controlled blocks
//! from the animation of the same path in a source folder.

use std::path::Path;

use xedit_io::encoding::ansi_compare_text;

use crate::data_format::{DfError, R};
use crate::data_format_nif::{NifFile, blocks_count, root_node};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage};
use crate::variant::Variant;

pub struct ProcCopyPriorities {
    base: ProcBase,
    /// `edSourceDirectory`.
    source_text: String,
    source_directory: String,
}

impl ProcCopyPriorities {
    pub fn new() -> ProcCopyPriorities {
        ProcCopyPriorities {
            base: ProcBase::new(
                "Copy anim priorities",
                &[GameType::Tes4, GameType::Fo3, GameType::Fnv],
                &["kf"],
            ),
            source_text: String::new(),
            source_directory: String::new(),
        }
    }
}

impl Proc for ProcCopyPriorities {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.source_text = storage.get_string("sSourceDirectory", "");
    }

    fn on_start(&mut self) -> R<()> {
        if self.source_text.is_empty() || !Path::new(&self.source_text).is_dir() {
            return Err(DfError::new("Source directory not found"));
        }
        self.source_directory = if self.source_text.ends_with('\\') {
            self.source_text.clone()
        } else {
            format!("{}\\", self.source_text)
        };
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let source = format!("{}{}", self.source_directory, file.file_name);
        if !Path::new(&source).is_file() {
            return Ok(Vec::new());
        }
        let mut changed = false;
        let mut nif = NifFile::new()?;
        let mut src_nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        src_nif.load_from_file(Path::new(&source))?;
        if blocks_count(&mut src_nif.tree)? == 0 || blocks_count(&mut nif.tree)? == 0 {
            return Ok(Vec::new());
        }
        let src_root = root_node(&mut src_nif.tree)?;
        let root = root_node(&mut nif.tree)?;
        let (Some(src_blocks), Some(dst_blocks)) = (
            src_nif.tree.elements(src_root, "Controlled Blocks")?,
            nif.tree.elements(root, "Controlled Blocks")?,
        ) else {
            return Ok(Vec::new());
        };

        // The names and priorities of the source, a sorted list.
        let mut src_list: Vec<(String, i64)> = Vec::new();
        for i in 0..src_nif.tree.count(src_blocks) {
            let entry = src_nif.tree.item(src_blocks, i)?;
            let name = src_nif.tree.edit_values(entry, "Node Name")?;
            let priority = src_nif.tree.native_values(entry, "Priority")?.to_i64()? as i32;
            src_list.push((name, i64::from(priority)));
        }
        // `Sorted := True` sorts with `AnsiCompareText` (a quick sort).
        src_list.sort_by(|a, b| ansi_compare_text(&a.0, &b.0));

        // The priorities of the source.
        let tree = &mut nif.tree;
        for i in 0..tree.count(dst_blocks) {
            let entry = tree.item(dst_blocks, i)?;
            let name = tree.edit_values(entry, "Node Name")?;
            // `IndexOf` of a sorted list: a binary search for the first of
            // the equal names.
            let index = src_list.partition_point(|(known, _)| ansi_compare_text(known, &name).is_lt());
            if index >= src_list.len() || !ansi_compare_text(&src_list[index].0, &name).is_eq() {
                continue;
            }
            let p = src_list[index].1;
            if tree.native_values(entry, "Priority")?.to_i64()? == p {
                continue;
            }
            tree.set_native_values(entry, "Priority", Variant::Int(p))?;
            changed = true;
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
