// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcFindDrawCalls.pas

//! `Find excessive draw calls`: estimates the draw calls of the shapes of
//! Fallout 3 and New Vegas meshes (strips and shader passes) and reports
//! the meshes above a count.

use crate::data_format::{DfError, El, R, Tree};
use crate::data_format_nif::{
    NifFile, block, block_children_by_type, block_is_ni_object, block_property_by_type, block_type, blocks_count,
};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation};
use crate::variant::str_to_int;

pub struct ProcFindDrawCalls {
    base: ProcBase,
    calls_num_text: String,
    calls_num: i64,
}

impl ProcFindDrawCalls {
    pub fn new() -> ProcFindDrawCalls {
        let mut base = ProcBase::new("Find excessive draw calls", &[GameType::Fo3, GameType::Fnv], &["nif"]);
        base.no_output = true;
        ProcFindDrawCalls {
            base,
            calls_num_text: "10".to_owned(),
            calls_num: 10,
        }
    }
}

/// `GetShaderPasses`.
fn get_shader_passes(tree: &mut Tree, shader: Option<El>) -> R<i64> {
    let mut result = 1;
    let Some(shader) = shader else { return Ok(result) };
    let mut flag = |path: &str| -> R<bool> { tree.native_values(shader, path)?.to_bool() };
    if flag("Shader Flags 1\\Environment_Mapping")?
        || flag("Shader Flags 1\\Eye_Environment_Mapping")?
        || flag("Shader Flags 1\\Window_Environment_Mapping")?
    {
        result += 1;
    }
    if flag("Shader Flags 1\\FaceGen")? {
        result += 1;
    }
    Ok(result)
}

impl Proc for ProcFindDrawCalls {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.calls_num_text = storage.get_string("sCallsNum", "10");
    }

    fn on_start(&mut self) -> R<()> {
        self.calls_num = i64::from(
            str_to_int(&self.calls_num_text)
                .ok_or_else(|| DfError::new("Draw calls threshold is not an integer value"))?,
        );
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut log: Vec<String> = Vec::new();
        let mut total_calls: i64 = 0;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;

        // The shapes of the editor markers.
        let mut markers: Vec<El> = Vec::new();
        for i in 0..blocks_count(tree)? {
            let b = block(tree, i)?;
            if block_is_ni_object(tree, b, "NiNode", true)
                && tree.edit_values(b, "Name")?.to_lowercase().starts_with("editormarker")
            {
                markers.extend(block_children_by_type(tree, b, "NiTriBasedGeom", true)?);
            }
        }

        for i in 0..blocks_count(tree)? {
            let b = block(tree, i)?;
            if markers.contains(&b) {
                continue;
            }
            let (shapes, passes) = match block_type(tree, b) {
                "NiTriShape" => {
                    let shader = block_property_by_type(tree, b, "BSShaderProperty", true)?;
                    (1, get_shader_passes(tree, shader)?)
                }
                "NiTriStrips" => {
                    let link = tree.elements(b, "Data")?.ok_or_else(access_violation)?;
                    let shapes = match tree.links_to(link)? {
                        Some(data) => tree.native_values(data, "Num Strips")?.to_i64()?,
                        None => 1,
                    };
                    let shader = block_property_by_type(tree, b, "BSShaderProperty", true)?;
                    (shapes, get_shader_passes(tree, shader)?)
                }
                _ => continue,
            };
            let calls = shapes * passes;
            total_calls += calls;
            log.push(format!("\t{calls} draw calls estimated for {}", tree.name(b)?));
        }

        if total_calls > self.calls_num {
            log.insert(0, file.file_name.clone());
            log.push(format!("\t{total_calls} draw calls estimated in total"));
            log.push(String::new());
            ctx.add_messages(log);
        }
        Ok(Vec::new())
    }
}
