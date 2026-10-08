// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcConvertRootNode.pas

//! `Convert block type`: converts the blocks of one type, or the root
//! node only, to a related type.

use crate::data_format::R;
use crate::data_format_nif::{NifFile, block, block_type, blocks_by_type, convert_block, root_nodes};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation};

/// The items of `cmbNodeFrom`.
const NODE_FROM: &[&str] = &[
    "NiNode",
    "BSFadeNode",
    "BSLeafAnimNode",
    "bhkConvexListShape",
    "BSShaderPPLightingProperty",
    "NiSkinInstance",
];

/// `cmbNodeFromSelect`: the items of `cmbNodeTo` for a type to convert.
fn node_to_items(node_from: &str) -> &'static [&'static str] {
    match node_from {
        "NiNode" => &["BSFadeNode", "BSLeafAnimNode"],
        "BSFadeNode" => &["NiNode", "BSLeafAnimNode"],
        "BSLeafAnimNode" => &["NiNode", "BSFadeNode"],
        "bhkConvexListShape" => &["bhkListShape"],
        "BSShaderPPLightingProperty" => &["Lighting30ShaderProperty"],
        "NiSkinInstance" => &["BSDismemberSkinInstance"],
        _ => &[],
    }
}

/// `TStrings.IndexOf`: ignores case.
fn index_of(items: &[&str], text: &str) -> Option<usize> {
    items
        .iter()
        .position(|item| crate::sniff::processor::ansi_same_text(item, text))
}

pub struct ProcConvertRootNode {
    base: ProcBase,
    /// `cmbNodeFrom`.
    node_from: String,
    /// `cmbNodeTo`.
    node_to: String,
    /// `chkRoot`.
    root: bool,
}

impl ProcConvertRootNode {
    pub fn new() -> ProcConvertRootNode {
        ProcConvertRootNode {
            base: ProcBase::new(
                "Convert block type",
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
            node_from: String::new(),
            node_to: String::new(),
            root: false,
        }
    }
}

impl Proc for ProcConvertRootNode {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        // The combo boxes start empty: their text is '' until an item is
        // chosen.
        let from = index_of(NODE_FROM, &storage.get_string("sNodeFrom", "")).unwrap_or(0);
        self.node_from = NODE_FROM[from].to_owned();
        let items = node_to_items(&self.node_from);
        let to = index_of(items, &storage.get_string("sNodeTo", items[0])).unwrap_or(0);
        self.node_to = items[to].to_owned();
        self.root = storage.get_bool("bRoot", false);
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        let blocks = if !self.root {
            blocks_by_type(tree, &self.node_from, false)?
        } else {
            // `RootNodes[0]` of a file without blocks reads past an empty
            // array.
            let root = *root_nodes(tree)?.first().ok_or_else(access_violation)?;
            if block_type(tree, root) == self.node_from {
                vec![root]
            } else {
                Vec::new()
            }
        };
        for b in blocks {
            let i = tree.index(b)?;
            convert_block(tree, i, &self.node_to)?;
            // The updates after the conversion.
            let converted = block(tree, i)?;
            match block_type(tree, converted) {
                "bhkListShape" => {
                    let ints = tree.elements(converted, "Unknown Ints")?.ok_or_else(access_violation)?;
                    tree.set_count(ints, 2)?;
                }
                "Lighting30ShaderProperty" => {
                    tree.set_edit_values(converted, "Shader Type", "SHADER_LIGHTING30")?;
                }
                "BSDismemberSkinInstance" => {
                    let mut parts = 1;
                    let link = tree
                        .elements(converted, "Skin Partition")?
                        .ok_or_else(access_violation)?;
                    if let Some(skin_partition) = tree.links_to(link)? {
                        parts = tree.native_values(skin_partition, "Num Partitions")?.to_i32()?;
                    }
                    let partitions = tree.elements(converted, "Partitions")?.ok_or_else(access_violation)?;
                    tree.set_count(partitions, parts)?;
                }
                _ => {}
            }
            changed = true;
        }
        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}
