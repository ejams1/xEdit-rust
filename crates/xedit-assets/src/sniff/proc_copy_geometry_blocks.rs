// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcCopyGeometryBlocks.pas

//! `Copy geometry blocks`: copies the transformation, the shader data and
//! the geometry of the blocks with the same name and type from the file of
//! the same path in a source folder, or from one source file.

use std::path::Path;
use std::sync::Mutex;

use crate::data_format::{El, R, Tree};
use crate::data_format_nif::{
    NifFile, NifOptions, NifVersion, TES4_TANGENTS_EXTRA_DATA_NAME, block_add_extra_data, block_by_name,
    block_extra_data_by_name, block_is_ni_object, block_property_by_type, block_remove_branch, block_type,
    blocks_by_type,
};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation};

pub struct ProcCopyGeometryBlocks {
    base: ProcBase,
    /// `rbMatchingFiles` (`rbSingleFile` is its negation).
    matching_files_checked: bool,
    /// `edSourceDirectory`.
    source_text: String,
    copy_geom_checked: bool,
    copy_transform_checked: bool,
    copy_shader_checked: bool,
    copy_texture_set_checked: bool,
    copy_geom: bool,
    copy_transform: bool,
    copy_shader: bool,
    copy_texture_set: bool,
    /// `fSourceDirectory`.
    source_directory: String,
    /// `fSourceFile`: the single source file, loaded in `OnStart` and read
    /// by every file; `None` in matching files mode.
    source_file: Mutex<Option<NifFile>>,
}

impl ProcCopyGeometryBlocks {
    pub fn new() -> ProcCopyGeometryBlocks {
        ProcCopyGeometryBlocks {
            base: ProcBase::new("Copy geometry blocks", GameType::ALL, &["nif"]),
            matching_files_checked: true,
            source_text: String::new(),
            copy_geom_checked: true,
            copy_transform_checked: false,
            copy_shader_checked: false,
            copy_texture_set_checked: false,
            copy_geom: false,
            copy_transform: false,
            copy_shader: false,
            copy_texture_set: false,
            source_directory: String::new(),
            source_file: Mutex::new(None),
        }
    }
}

/// `Elements[aPath].LinksTo` of a missing element reads through nil
/// upstream.
fn link(tree: &mut Tree, element: El, path: &str) -> R<Option<El>> {
    let element = tree.elements(element, path)?.ok_or_else(access_violation)?;
    tree.links_to(element)
}

impl Proc for ProcCopyGeometryBlocks {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.matching_files_checked = storage.get_bool("bMatchingFiles", self.matching_files_checked);
        self.source_text = storage.get_string("sSourceDirectory", "");
        self.copy_geom_checked = storage.get_bool("bCopyGeom", self.copy_geom_checked);
        self.copy_transform_checked = storage.get_bool("bCopyTransform", self.copy_transform_checked);
        self.copy_shader_checked = storage.get_bool("bCopyShader", self.copy_shader_checked);
        self.copy_texture_set_checked = storage.get_bool("bCopyTextureSet", self.copy_texture_set_checked);
    }

    fn on_start(&mut self) -> R<()> {
        self.copy_geom = self.copy_geom_checked;
        self.copy_transform = self.copy_transform_checked;
        self.copy_shader = self.copy_shader_checked;
        self.copy_texture_set = self.copy_texture_set_checked;

        if !self.copy_geom && !self.copy_transform && !self.copy_shader {
            return Err(crate::data_format::DfError::new("Nothing to copy"));
        }

        self.source_directory = self.source_text.clone();
        if self.matching_files_checked
            && (self.source_directory.is_empty() || !Path::new(&self.source_directory).is_dir())
        {
            return Err(crate::data_format::DfError::new("Source directory not found"));
        }
        if !self.matching_files_checked
            && (self.source_directory.is_empty() || !Path::new(&self.source_directory).is_file())
        {
            return Err(crate::data_format::DfError::new("Source file not found"));
        }

        if self.matching_files_checked {
            if !self.source_directory.ends_with('\\') {
                self.source_directory.push('\\');
            }
        } else {
            let mut nif = NifFile::new()?;
            nif.load_from_file(Path::new(&self.source_directory))?;
            *self.source_file.lock().unwrap() = Some(nif);
        }
        Ok(())
    }

    fn on_hide(&mut self) {
        // `FreeAndNil(fSourceFile)`.
        *self.source_file.lock().unwrap() = None;
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let guard = self.source_file.lock().unwrap();
        let mut source_path = String::new();
        if guard.is_none() {
            source_path = format!("{}{}", self.source_directory, file.file_name);
            if !Path::new(&source_path).is_file() {
                return Ok(Vec::new());
            }
        }
        drop(guard);

        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;

        let mut guard = self.source_file.lock().unwrap();
        let mut loaded = None;
        let src_nif: &mut NifFile = match guard.as_mut() {
            Some(source) => source,
            None => {
                loaded = Some(NifFile::new()?);
                let nif = loaded.as_mut().unwrap();
                nif.load_from_file(Path::new(&source_path))?;
                nif
            }
        };
        let src_tree = &mut src_nif.tree;
        let tree = &mut nif.tree;
        let tes4 = tree.nif.nif_version == NifVersion::Tes4;

        for block in blocks_by_type(tree, "NiAVObject", true)? {
            let name = tree.edit_values(block, "Name")?;
            if name.is_empty() {
                continue;
            }

            // find the same block to copy from
            let Some(src_block) = block_by_name(src_tree, &name, "")? else {
                continue;
            };
            if block_type(tree, block) != block_type(src_tree, src_block) {
                continue;
            }

            // copy Transform
            if self.copy_transform {
                let destination = tree.elements(block, "Transform")?.ok_or_else(access_violation)?;
                let source = src_tree
                    .elements(src_block, "Transform")?
                    .ok_or_else(access_violation)?;
                tree.assign_from(destination, src_tree, Some(source))?;
                // always force copy
                changed = true;
            }

            // copy Shader and Texture Set
            if self.copy_shader && block_is_ni_object(tree, block, "NiGeometry", true) {
                let src_shader = block_property_by_type(src_tree, src_block, "BSShaderProperty", true)?;
                let dst_shader = block_property_by_type(tree, block, "BSShaderProperty", true)?;
                if let (Some(src_shader), Some(dst_shader)) = (src_shader, dst_shader)
                    && block_type(tree, dst_shader) == block_type(src_tree, src_shader)
                {
                    let mut index = 0;
                    while index < tree.count(dst_shader) {
                        let el = tree.item(dst_shader, index)?;
                        index += 1;
                        let el_name = tree.name(el)?;
                        if el_name.starts_with("Extra Data")
                            || el_name == "Name"
                            || el_name == "Controller"
                            || el_name == "Texture Set"
                        {
                            continue;
                        }
                        let source = src_tree.element_by_name(src_shader, &el_name, true)?;
                        tree.assign_from(el, src_tree, source)?;
                    }

                    if self.copy_texture_set {
                        // The elements may be missing or link to nothing,
                        // which upstream checks.
                        let src_set = match src_tree.elements(src_shader, "Texture Set")? {
                            Some(element) => src_tree.links_to(element)?,
                            None => None,
                        };
                        let dst_set = match tree.elements(dst_shader, "Texture Set")? {
                            Some(element) => tree.links_to(element)?,
                            None => None,
                        };
                        if let (Some(src_set), Some(dst_set)) = (src_set, dst_set) {
                            tree.assign_from(dst_set, src_tree, Some(src_set))?;
                        }
                    }

                    // always force copy
                    changed = true;
                }
            }

            // copy geometry
            if self.copy_geom {
                if block_is_ni_object(tree, block, "NiTriBasedGeom", true) {
                    let Some(src_data) = link(src_tree, src_block, "Data")? else {
                        continue;
                    };
                    let Some(dst_data) = link(tree, block, "Data")? else {
                        continue;
                    };

                    let additional = tree.native_values(dst_data, "Additional Data")?;
                    tree.assign_from(dst_data, src_tree, Some(src_data))?;
                    tree.set_native_values(dst_data, "Additional Data", additional)?;

                    // copy Oblivion tangents in extra data block
                    if tes4 {
                        let src_tangents =
                            block_extra_data_by_name(src_tree, src_block, TES4_TANGENTS_EXTRA_DATA_NAME)?;
                        let mut dst_tangents = block_extra_data_by_name(tree, block, TES4_TANGENTS_EXTRA_DATA_NAME)?;
                        if let Some(src_tangents) = src_tangents {
                            if dst_tangents.is_none() {
                                let added = block_add_extra_data(tree, block, "NiBinaryExtraData")?;
                                tree.set_edit_values(added, "Name", TES4_TANGENTS_EXTRA_DATA_NAME)?;
                                dst_tangents = Some(added);
                            }
                            tree.assign_from(dst_tangents.unwrap(), src_tree, Some(src_tangents))?;
                        } else if let Some(dst_tangents) = dst_tangents {
                            block_remove_branch(tree, dst_tangents, true)?;
                            tree.nif.options = NifOptions {
                                collapse_link_arrays: true,
                                remove_unused_strings: false,
                            };
                        }
                    }

                    changed = true;
                } else if block_is_ni_object(tree, block, "BSTriShape", true) {
                    let links = [
                        tree.native_values(block, "Controller")?,
                        tree.native_values(block, "Collision Object")?,
                        tree.native_values(block, "Skin")?,
                        tree.native_values(block, "Shader Property")?,
                        tree.native_values(block, "Alpha Property")?,
                    ];
                    let list = tree.elements(block, "Extra Data List")?.ok_or_else(access_violation)?;
                    let mut extras = Vec::new();
                    for index in 0..tree.count(list) {
                        let item = tree.item(list, index)?;
                        extras.push(tree.native_value(item)?);
                    }

                    tree.assign_from(block, src_tree, Some(src_block))?;

                    for (field, value) in [
                        "Controller",
                        "Collision Object",
                        "Skin",
                        "Shader Property",
                        "Alpha Property",
                    ]
                    .into_iter()
                    .zip(links)
                    {
                        tree.set_native_values(block, field, value)?;
                    }
                    let list = tree.elements(block, "Extra Data List")?.ok_or_else(access_violation)?;
                    tree.set_count(list, extras.len() as i32)?;
                    for (index, value) in extras.into_iter().enumerate() {
                        let item = tree.item(list, index as i32)?;
                        tree.set_native_value(item, value)?;
                    }

                    changed = true;
                }
            }
        }

        // `SrcNif` is dropped with `loaded` here.
        drop(loaded);

        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A NIF with `Scene Root` and a `Child` node at `translation`: the
    /// block names and types a copy matches on.
    fn mesh(translation: &str) -> Vec<u8> {
        let mut nif = NifFile::new().unwrap();
        let tree = &mut nif.tree;
        crate::data_format_nif::set_nif_version(tree, NifVersion::Fo3).unwrap();
        let root = crate::data_format_nif::add_block(tree, "NiNode").unwrap();
        tree.set_edit_values(root, "Name", "Scene Root").unwrap();
        let child = crate::data_format_nif::block_add_child(tree, root, "NiNode").unwrap();
        tree.set_edit_values(child, "Name", "Child").unwrap();
        tree.set_edit_values(child, "Transform\\Translation", translation)
            .unwrap();
        nif.save_to_data().unwrap()
    }

    /// The single file mode: `OnStart` loads it once and every file is
    /// copied from it (`bMatchingFiles=0`, `bCopyTransform=1`). The oracle
    /// hangs in this mode, so a unit test stands in for the parity case.
    #[test]
    fn the_single_file_mode_copies_the_transform() {
        let dir = std::env::temp_dir().join(format!("xedit-copy-geometry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.nif");
        std::fs::write(&source, mesh("1 2 3")).unwrap();
        std::fs::write(dir.join("target.nif"), mesh("0 0 0")).unwrap();

        let mut proc = ProcCopyGeometryBlocks::new();
        proc.copy_geom_checked = false;
        proc.copy_transform_checked = true;
        proc.matching_files_checked = false;
        proc.source_text = source.display().to_string();
        proc.on_start().unwrap();

        let input = crate::sniff::processor::ProcInput {
            archive: None,
            input_directory: format!("{}\\", dir.display()),
        };
        let mut file = ProcFileObject {
            input: &input,
            file_name: "target.nif".to_owned(),
            file_entry: None,
        };
        let data = proc
            .process_file(&mut file, &mut crate::sniff::processor::ProcContext::default())
            .unwrap();
        assert!(!data.is_empty(), "the target was copied into");

        let mut nif = NifFile::new().unwrap();
        nif.load_from_data(&data).unwrap();
        let tree = &mut nif.tree;
        let child = crate::data_format_nif::block_by_name(tree, "Child", "NiNode")
            .unwrap()
            .unwrap();
        assert_eq!(
            tree.edit_values(child, "Transform\\Translation").unwrap(),
            "1.000000 2.000000 3.000000"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn on_start_checks_the_settings() {
        // The defaults copy the geometry from matching files.
        let mut proc = ProcCopyGeometryBlocks::new();
        proc.on_show(&Storage::new("Copygeometryblocks".to_owned(), None));
        assert_eq!(proc.on_start().unwrap_err().0, "Source directory not found");
        assert!(proc.copy_geom && !proc.copy_transform && !proc.copy_shader && proc.matching_files_checked);
        proc.source_text = "M:".to_owned();
        proc.on_start().unwrap();
        assert!(proc.source_directory.ends_with('\\'));

        // A single file that does not exist, and nothing to copy.
        proc.matching_files_checked = false;
        assert_eq!(proc.on_start().unwrap_err().0, "Source file not found");
        proc.copy_geom_checked = false;
        proc.copy_transform = false;
        proc.copy_shader_checked = false;
        proc.copy_transform_checked = false;
        assert_eq!(proc.on_start().unwrap_err().0, "Nothing to copy");
    }
}
