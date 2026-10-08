// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDataFormatNif.pas

//! NIF files: the registry of NiObject definitions (`NiObjectInfos`),
//! `TwbNifFile` and `TwbNifBlock`, and the callbacks that many block
//! definitions share. The block definitions themselves are generated
//! from the upstream unit into `data_format_nif/defs.rs` by
//! `cargo xtask port-defs emit-df`; the callbacks the transpiler cannot
//! translate are in `data_format_nif/callbacks.rs`.

mod callbacks;
pub mod defs;
mod stubs;

use std::sync::OnceLock;

#[allow(unused_imports)]
use crate::data_format::{
    Class, DataType, Def, DefKind, DfError, El, Event, OnDecide, R, Tree, df_array, df_bool, df_bytes, df_calc_hash,
    df_chars, df_enum, df_flags, df_float, df_hex_integer, df_integer, df_merge, df_struct, df_union, df_value_union,
    req, same_text,
};
#[allow(unused_imports)]
use crate::data_format_nif_types::*;
use crate::json::Json;
use crate::variant::Variant;

pub use callbacks::*;
pub use defs::*;
#[allow(unused_imports)]
pub use stubs::*;

/// `TwbNifVersion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub enum NifVersion {
    #[default]
    Unknown,
    Tes3,
    Tes4,
    Fo3,
    Tes5,
    Sse,
    Fo4,
}

impl NifVersion {
    /// The upstream name (`nfTES3`...).
    pub fn name(self) -> &'static str {
        match self {
            NifVersion::Unknown => "nfUnknown",
            NifVersion::Tes3 => "nfTES3",
            NifVersion::Tes4 => "nfTES4",
            NifVersion::Fo3 => "nfFO3",
            NifVersion::Tes5 => "nfTES5",
            NifVersion::Sse => "nfSSE",
            NifVersion::Fo4 => "nfFO4",
        }
    }
}

/// `TwbNifOptions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NifOptions {
    pub collapse_link_arrays: bool,
    pub remove_unused_strings: bool,
}

/// `sTES4TangentsExtraDataName`.
pub const TES4_TANGENTS_EXTRA_DATA_NAME: &str = "Tangent space (binormal & tangent vectors)";

/// `HK2GU`: Havok units to game units.
pub fn hk2gu(version: NifVersion) -> f32 {
    match version {
        NifVersion::Unknown | NifVersion::Tes3 => 1.0,
        NifVersion::Tes4 | NifVersion::Fo3 => 6.999125,
        NifVersion::Tes5 | NifVersion::Sse | NifVersion::Fo4 => 69.99125,
    }
}

/// The fields of a `TwbNifFile`, kept on its tree.
#[derive(Debug, Clone)]
pub struct NifState {
    pub version: u32,
    pub user_version: u32,
    pub user_version2: u32,
    pub nif_version: NifVersion,
    pub options: NifOptions,
    pub internal_updates: bool,
    pub stop_at_index: i32,
    pub file_name: String,
}

impl Default for NifState {
    fn default() -> Self {
        NifState {
            version: 0,
            user_version: 0,
            user_version2: 0,
            nif_version: NifVersion::Unknown,
            options: NifOptions {
                collapse_link_arrays: false,
                remove_unused_strings: true,
            },
            internal_updates: true,
            stop_at_index: -1,
            file_name: String::new(),
        }
    }
}

const NIF_MAGIC_GAMEBRYO: &str = "Gamebryo File Format, Version ";
const NIF_MAGIC_NETIMMERSE: &str = "NetImmerse File Format, Version ";

/// `wbNifVersionToInt`: `a.b.c.d` as `a << 24 | b << 16 | c << 8 | d`.
pub fn wb_nif_version_to_int(version: &str) -> u32 {
    let parts: Vec<&str> = version.split('.').collect();
    let mut result: u32 = 0;
    for index in 0..4 {
        let part = parts
            .get(index)
            .and_then(|part| crate::variant::str_to_int(part))
            .unwrap_or(0) as u32;
        result |= part.wrapping_shl(24 - index as u32 * 8);
    }
    result
}

/// `wbIntToNifVersion`.
pub fn wb_int_to_nif_version(version: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        version >> 24 & 0xff,
        version >> 16 & 0xff,
        version >> 8 & 0xff,
        version & 0xff
    )
}

const fn version(a: u32, b: u32, c: u32, d: u32) -> i64 {
    ((a << 24) | (b << 16) | (c << 8) | d) as i64
}

// The version constants that `wbDefineNif` sets.
pub const V4002: i64 = version(4, 0, 0, 2);
pub const V4101: i64 = version(4, 1, 0, 1);
pub const V41012: i64 = version(4, 1, 0, 12);
pub const V4202: i64 = version(4, 2, 0, 2);
pub const V4210: i64 = version(4, 2, 1, 0);
pub const V4220: i64 = version(4, 2, 2, 0);
pub const V5001: i64 = version(5, 0, 0, 1);
pub const V10010: i64 = version(10, 0, 1, 0);
pub const V10012: i64 = version(10, 0, 1, 2);
pub const V10013: i64 = version(10, 0, 1, 3);
pub const V10100: i64 = version(10, 1, 0, 0);
pub const V10018: i64 = version(10, 0, 1, 8);
pub const V1010101: i64 = version(10, 1, 0, 101);
pub const V1010106: i64 = version(10, 1, 0, 106);
pub const V10200: i64 = version(10, 2, 0, 0);
pub const V10401: i64 = version(10, 4, 0, 1);
pub const V20103: i64 = version(20, 1, 0, 3);
pub const V20004: i64 = version(20, 0, 0, 4);
pub const V20005: i64 = version(20, 0, 0, 5);
pub const V20007: i64 = version(20, 0, 0, 7);
pub const V20101: i64 = version(20, 1, 0, 1);
pub const V20102: i64 = version(20, 1, 0, 2);
pub const V20205: i64 = version(20, 2, 0, 5);
pub const V20207: i64 = version(20, 2, 0, 7);

/// `nif(e).Version`.
pub fn nif_version(tree: &Tree) -> i64 {
    i64::from(tree.nif.version)
}

/// `nif(e).UserVersion`.
pub fn nif_user_version(tree: &Tree) -> i64 {
    i64::from(tree.nif.user_version)
}

/// `nif(e).UserVersion2`.
pub fn nif_user_version2(tree: &Tree) -> i64 {
    i64::from(tree.nif.user_version2)
}

/// `TNiObjectInfo`.
pub struct NiObjectInfo {
    pub def: Def,
    pub name: String,
    pub inherit: String,
    pub inherit_index: Option<usize>,
    pub name_hash: u32,
    pub is_abstract: bool,
}

/// `TNiObjectInfos`: every NiObject definition, in definition order.
#[derive(Default)]
pub struct NiObjectInfos {
    pub ni_objects: Vec<NiObjectInfo>,
}

impl NiObjectInfos {
    /// `Add`: registers `def`, after copying in the members of the
    /// definition it inherits from at `insert_index`.
    pub fn add(&mut self, inherit: &str, is_abstract: bool, mut def: Def, insert_index: usize) -> R<()> {
        if self.index_of(&def.name).is_some() {
            return Err(DfError::new(format!("Definition of {} already exists", def.name)));
        }
        let inherit_index = if inherit.is_empty() {
            None
        } else {
            let index = self
                .index_of(inherit)
                .ok_or_else(|| DfError::new(format!("Unknown NiObject to inherit from: {inherit}")))?;
            def.insert_defs_from(&self.ni_objects[index].def, insert_index)?;
            Some(index)
        };
        self.ni_objects.push(NiObjectInfo {
            name: def.name.clone(),
            name_hash: df_calc_hash(&def.name),
            def,
            inherit: inherit.to_owned(),
            inherit_index,
            is_abstract,
        });
        Ok(())
    }

    /// `IndexOf`: by name hash.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        let hash = df_calc_hash(name);
        self.ni_objects.iter().position(|info| info.name_hash == hash)
    }
}

/// `wbNiObject`.
pub fn wb_ni_object(
    infos: &mut NiObjectInfos,
    def: Def,
    inherit: &str,
    is_abstract: bool,
    insert_index: usize,
) -> R<()> {
    infos.add(inherit, is_abstract, def, insert_index)
}

static NIF_DEFS: OnceLock<NiObjectInfos> = OnceLock::new();

/// The NiObject definitions, defined on first use (`wbDefineNif`).
pub fn ni_object_infos() -> &'static NiObjectInfos {
    NIF_DEFS.get_or_init(|| {
        let mut infos = NiObjectInfos::default();
        defs::wb_define_nif(&mut infos).unwrap_or_else(|error| panic!("NIF definitions: {error}"));
        infos
    })
}

/// `wbNiObjectList`.
pub fn wb_ni_object_list() -> Vec<String> {
    ni_object_infos()
        .ni_objects
        .iter()
        .map(|info| info.def.name.clone())
        .collect()
}

/// `wbNiObjectDef`.
pub fn wb_ni_object_def(name: &str) -> R<&'static Def> {
    let infos = ni_object_infos();
    let index = infos
        .index_of(name)
        .ok_or_else(|| DfError::new(format!("Unknown NiObject: {name}")))?;
    let info = &infos.ni_objects[index];
    if info.is_abstract {
        return Err(DfError::new(format!(
            "Can not initialize from abstract NiObject: {name}"
        )));
    }
    Ok(&info.def)
}

/// `wbIsNiObject(aNiObject, aTemplate)`: whether the block type is the
/// template or inherits from it.
pub fn wb_is_ni_object(ni_object: &str, template: &str) -> bool {
    let infos = ni_object_infos();
    let hash = df_calc_hash(template);
    // UPSTREAM-QUIRK: upstream reads before the array for an unknown type.
    let mut index = infos.index_of(ni_object);
    while let Some(current) = index {
        let info = &infos.ni_objects[current];
        if info.name_hash == hash {
            return true;
        }
        index = info.inherit_index;
    }
    false
}

/// `wbIsNiObject(aElement, aTemplate)`: of the block that contains the
/// element.
pub fn wb_is_ni_object_el(tree: &Tree, el: El, template: &str) -> bool {
    let mut element = Some(el);
    while let Some(current) = element {
        if tree.class(current).is_nif_block() {
            return wb_is_ni_object(&tree.raw_def(current).name, template);
        }
        element = tree.parent(current);
    }
    false
}

/// `nifblk`: the block that contains the element, from its parent up.
pub fn nifblk(tree: &Tree, el: El) -> Option<El> {
    let mut element = tree.parent(el);
    while let Some(current) = element {
        if tree.class(current).is_nif_block() {
            return Some(current);
        }
        element = tree.parent(current);
    }
    None
}

/// `nifblk(e)` where upstream dereferences the result.
pub fn nifblk_r(tree: &Tree, el: El) -> R<El> {
    nifblk(tree, el).ok_or_else(|| DfError::new("Access violation: element outside of a NIF block"))
}

/// `GetControlledBlockName`.
pub fn get_controlled_block_name(tree: &mut Tree, controlled_block: El, field: &str) -> R<String> {
    if let Some(palette) = tree.elements(controlled_block, "String Palette")? {
        // Oblivion meshes store names in the string palette.
        if let Some(palette) = tree.links_to(palette)? {
            let offset = tree
                .native_values(controlled_block, &format!("{field} Offset"))?
                .to_i32()?;
            return block_get_string_palette_string(tree, palette, offset);
        }
        return Ok(String::new());
    }
    tree.edit_values(controlled_block, field)
}

/// A NIF file: a tree with a `TwbNifFile` root.
pub struct NifFile {
    pub tree: Tree,
    pub root: El,
}

impl NifFile {
    /// `TwbNifFile.Create`.
    pub fn new() -> R<NifFile> {
        let mut tree = Tree::new();
        let def = wb_ni_object_def("NIF")?;
        let root = tree.create_root(def, Class::NifFile)?;
        Ok(NifFile { tree, root })
    }

    /// `LoadFromData`.
    pub fn load_from_data(&mut self, data: &[u8]) -> R<()> {
        self.tree.load_from_data(self.root, data)
    }

    /// `LoadFromFile`.
    pub fn load_from_file(&mut self, path: &std::path::Path) -> R<()> {
        let data = std::fs::read(path).map_err(|error| DfError::new(error.to_string()))?;
        self.load_from_data(&data)?;
        self.tree.nif.file_name = path.display().to_string();
        Ok(())
    }

    /// `SaveToData`.
    pub fn save_to_data(&mut self) -> R<Vec<u8>> {
        self.tree.save_to_data(self.root)
    }

    /// `ToJSON`.
    pub fn to_json(&mut self, compact: bool) -> R<String> {
        self.tree.to_json(self.root, compact)
    }

    /// `FromJSON`.
    pub fn from_json(&mut self, text: &str) -> R<()> {
        self.tree.from_json(self.root, text)
    }

    /// `ToText`.
    pub fn to_text(&mut self) -> R<String> {
        self.tree.to_text(self.root, 0)
    }
}

// ---- TwbNifFile ----

/// `TwbNifFile.UnSerialize`.
pub(crate) fn nif_unserialize(tree: &mut Tree, el: El, data: Option<&[u8]>, data_size: i32) -> R<i32> {
    if let Some(data) = data {
        let magic_error = |need: usize| {
            DfError::new(format!(
                "Error in \"Magic\": Unexpected end of stream: need {need} bytes, available {}",
                data.len()
            ))
        };
        if data.len() < NIF_MAGIC_GAMEBRYO.len() {
            return Err(magic_error(NIF_MAGIC_GAMEBRYO.len()));
        }
        if &data[..NIF_MAGIC_GAMEBRYO.len()] != NIF_MAGIC_GAMEBRYO.as_bytes() {
            if data.len() < NIF_MAGIC_NETIMMERSE.len() {
                return Err(magic_error(NIF_MAGIC_NETIMMERSE.len()));
            }
            if &data[..NIF_MAGIC_NETIMMERSE.len()] != NIF_MAGIC_NETIMMERSE.as_bytes() {
                return Err(DfError::new("Not a valid NIF file"));
            }
        }
    }

    tree.clear(el)?;
    let mut result: i32 = 0;
    let mut num_blocks: i32 = 2;
    let mut cur_block: i32 = 0;
    let mut block_types: Option<El> = None;
    let mut block_type = String::new();
    // Older meshes have the block type in a sized string before each block.
    let block_type_def = block_type_def();
    let mut block_type_tree = Tree::new();
    let block_type_el =
        block_type_tree.create_root(block_type_def, Class::Value(crate::data_format::ValueClass::Chars))?;

    let at = |result: i32| data.map(|data| data.get(result.max(0) as usize..).unwrap_or_default());
    let outcome: R<bool> = (|| {
        while cur_block < num_blocks {
            if cur_block == 0 {
                block_type = "NiHeader".to_owned();
            } else if cur_block == num_blocks - 1 {
                block_type = "NiFooter".to_owned();
            } else if let Some(types) = block_types {
                let item = tree.item(types, cur_block - 1)?;
                block_type = tree.edit_value(item)?;
            } else {
                result += block_type_tree.unserialize(block_type_el, at(result), 0)?;
                block_type = block_type_tree.edit_value(block_type_el)?;
            }

            // 10.1.0.106 meshes have an empty block type index before each
            // block except NiFooter.
            if i64::from(tree.nif.version) == V1010106 && cur_block != num_blocks - 1 {
                result += 4;
            }

            result += read_block(tree, el, &block_type, at(result))?;

            if block_type == "NiHeader" {
                let header = header(tree)?;
                num_blocks += tree.native_values(header, "Num Blocks")?.to_i32()?;
                block_types = tree.elements(header, "Block Type Index")?;
            }

            if cur_block == tree.nif.stop_at_index {
                return Ok(true);
            }
            cur_block += 1;
        }
        Ok(false)
    })();
    match outcome {
        Ok(true) => return Ok(result),
        Ok(false) => {}
        Err(error) => {
            return Err(if cur_block == 0 || cur_block == num_blocks - 1 {
                DfError::new(format!("Error reading {block_type}: {error}"))
            } else {
                DfError::new(format!(
                    "Error reading NIF block {} {block_type}: {error}",
                    cur_block - 1
                ))
            });
        }
    }
    let _ = data_size;
    mark_loaded(tree, el, data.is_some(), data_size)?;
    Ok(result)
}

/// `TdfElement.UnSerialize` of the root, which `inherited` reaches.
fn mark_loaded(tree: &mut Tree, el: El, has_data: bool, data_size: i32) -> R<()> {
    tree.element_unserialize(el, has_data, data_size)
}

/// The definition of the block type string before each Morrowind block.
fn block_type_def() -> &'static Def {
    static DEF: OnceLock<Def> = OnceLock::new();
    DEF.get_or_init(|| wb_sized_string("Block Type", "", &[]))
}

/// `ReadBlock`.
fn read_block(tree: &mut Tree, el: El, block_type: &str, data: Option<&[u8]>) -> R<i32> {
    let def = wb_ni_object_def(block_type)?;
    let block = tree.create_element(def, Some(el))?;
    let count = tree.count(el) as usize;
    tree.put(el, count, block);
    tree.unserialize(block, data, 0)
}

/// `TwbNifFile.Serialize`.
pub(crate) fn nif_serialize(tree: &mut Tree, el: El, out: &mut Vec<u8>) -> R<i32> {
    let mut result = 0;
    let count = tree.count(el);
    for index in 0..count {
        let item = tree.item(el, index)?;
        // The data before each block except the header and the footer.
        if index != 0 && index != count - 1 {
            if tree.nif.nif_version == NifVersion::Tes3 {
                let name = tree.raw_def(item).name.clone();
                out.extend_from_slice(&(name.len() as u32).to_le_bytes());
                out.extend_from_slice(name.as_bytes());
                result += 4 + name.len() as i32;
            } else if i64::from(tree.nif.version) == V1010106 {
                out.extend_from_slice(&[0; 4]);
                result += 4;
            }
        }
        result += tree.serialize(item, out)?;
    }
    Ok(result)
}

/// `TwbNifFile.DataSize`: updates the header first when `InternalUpdates`.
pub(crate) fn nif_data_size(tree: &mut Tree, el: El) -> R<i32> {
    let mut result = 0;
    let block_size = if tree.nif.internal_updates {
        update_header(tree)?;
        let header = header(tree)?;
        tree.elements(header, "Block Size")?
    } else {
        None
    };
    if let Some(block_size) = block_size {
        let count = blocks_count(tree)?;
        tree.set_count(block_size, count)?;
    }
    let count = tree.count(el);
    for index in 0..count {
        let item = tree.item(el, index)?;
        let size = tree.data_size(item)?;
        result += size;
        // The block type string of Morrowind meshes and the block type
        // index of 10.1.0.106 meshes before each block.
        if index != 0 && index != count - 1 {
            if let Some(block_size) = block_size {
                let entry = tree.item(block_size, index - 1)?;
                tree.set_native_value(entry, Variant::Int(i64::from(size)))?;
            }
            if tree.nif.nif_version == NifVersion::Tes3 {
                let block = block(tree, index - 1)?;
                result += 4 + tree.raw_def(block).name.encode_utf16().count() as i32;
            } else if i64::from(tree.nif.version) == V1010106 {
                result += 4;
            }
        }
    }
    Ok(result)
}

/// `TwbNifFile.UnSerializeFromJSON`.
pub(crate) fn nif_unserialize_from_json(tree: &mut Tree, el: El, json: &Json) -> R<()> {
    tree.clear(el)?;
    let Json::Obj(entries) = json else {
        return Ok(());
    };
    let total = entries.len();
    for (index, (name, _)) in entries.iter().enumerate() {
        let parts: Vec<&str> = name.split(' ').collect();
        let block_type = if parts.len() > 1 { parts[1] } else { parts[0] };
        if index == 0 && block_type != "NiHeader" {
            return Err(tree.exception(el, "First block must be NiHeader"));
        }
        if index == total - 1 && block_type != "NiFooter" {
            return Err(tree.exception(el, "Last block must be NiFooter"));
        }
        read_block(tree, el, block_type, None)?;
        let block = tree.item(el, tree.count(el) - 1)?;
        tree.unserialize_from_json(block, json)?;
    }
    Ok(())
}

/// `RemapBlocks`: rewrites the references of every block and the footer.
fn remap_blocks(tree: &mut Tree, map: &[i32]) -> R<()> {
    let count = blocks_count(tree)?;
    for index in 0..=count {
        let block = if index < count {
            block(tree, index)?
        } else {
            footer(tree)?
        };
        let refs = tree
            .block_lists(block)
            .map(|lists| lists.refs.clone())
            .unwrap_or_default();
        for reference in refs {
            let target = tree.native_value(reference)?.to_i32()?;
            if target < 0 || target as usize >= map.len() {
                continue;
            }
            if target != map[target as usize] {
                tree.set_native_value(reference, Variant::Int(i64::from(map[target as usize])))?;
            }
        }
    }
    Ok(())
}

/// `TwbNifFile.Delete`: removes the block and points its references to None.
pub(crate) fn nif_delete(tree: &mut Tree, el: El, index: i32) -> R<()> {
    block(tree, index)?;
    let count = blocks_count(tree)?;
    let map: Vec<i32> = (0..count)
        .map(|i| match i.cmp(&index) {
            std::cmp::Ordering::Less => i,
            std::cmp::Ordering::Equal => -1,
            std::cmp::Ordering::Greater => i - 1,
        })
        .collect();
    remap_blocks(tree, &map)?;
    tree.container_delete(el, index + 1)
}

/// `TwbNifFile.Move`: moves the block and rewrites the references.
pub(crate) fn nif_move(tree: &mut Tree, el: El, cur_index: i32, new_index: i32) -> R<()> {
    block(tree, cur_index)?;
    block(tree, new_index)?;
    let count = blocks_count(tree)?;
    let mut map: Vec<i32> = (0..count).collect();
    if cur_index > new_index {
        for (i, entry) in map
            .iter_mut()
            .enumerate()
            .take(cur_index as usize)
            .skip(new_index as usize)
        {
            *entry = i as i32 + 1;
        }
    } else {
        for (i, entry) in map
            .iter_mut()
            .enumerate()
            .take(new_index as usize + 1)
            .skip(cur_index as usize + 1)
        {
            *entry = i as i32 - 1;
        }
    }
    map[cur_index as usize] = new_index;
    remap_blocks(tree, &map)?;
    tree.container_move(el, cur_index + 1, new_index + 1)
}

/// `Header`.
pub fn header(tree: &mut Tree) -> R<El> {
    let root = tree.root_el();
    if tree.count(root) == 0 {
        tree.set_to_default(root)?;
    }
    tree.item(root, 0)
}

/// `Footer`.
pub fn footer(tree: &mut Tree) -> R<El> {
    let root = tree.root_el();
    if tree.count(root) == 0 {
        tree.set_to_default(root)?;
    }
    let count = tree.count(root);
    tree.item(root, count - 1)
}

/// `BlocksCount`.
pub fn blocks_count(tree: &mut Tree) -> R<i32> {
    let root = tree.root_el();
    if tree.count(root) == 0 {
        tree.set_to_default(root)?;
    }
    Ok(tree.count(root) - 2)
}

/// `Blocks[Index]`.
pub fn block(tree: &mut Tree, index: i32) -> R<El> {
    if index >= 0 && index < blocks_count(tree)? {
        let root = tree.root_el();
        tree.item(root, index + 1)
    } else {
        Err(DfError::new(format!("Invalid block index {index}")))
    }
}

/// `RootNodes`: the blocks the footer lists, else the first block.
pub fn root_nodes(tree: &mut Tree) -> R<Vec<El>> {
    let footer = footer(tree)?;
    let mut result = Vec::new();
    if let Some(roots) = tree.elements(footer, "Roots")? {
        for index in 0..tree.count(roots) {
            let item = tree.item(roots, index)?;
            if let Some(root) = tree.links_to(item)? {
                result.push(root);
            }
        }
    }
    if result.is_empty() && blocks_count(tree)? != 0 {
        result.push(block(tree, 0)?);
    }
    Ok(result)
}

/// `RootNode`.
pub fn root_node(tree: &mut Tree) -> R<El> {
    root_nodes(tree)?
        .first()
        .copied()
        .ok_or_else(|| DfError::new("List index (0) is out of bounds"))
}

/// The block type of a block (`BlockType`).
pub fn block_type(tree: &Tree, block: El) -> &'static str {
    &tree.raw_def(block).name
}

/// `UpdateHeader`: the block count, block types and their indices, and the
/// string table without the strings no block uses.
pub fn update_header(tree: &mut Tree) -> R<()> {
    let header = header(tree)?;
    let count = blocks_count(tree)?;
    tree.set_native_values(header, "Num Blocks", Variant::Int(i64::from(count)))?;

    if let Some(block_types) = tree.elements(header, "Block Types")? {
        // A unique list of the block types; `TStringList.IndexOf` ignores case.
        let mut types: Vec<String> = Vec::new();
        for index in 0..count {
            let block = block(tree, index)?;
            let name = block_type(tree, block);
            if !types.iter().any(|known| same_text(known, name)) {
                types.push(name.to_owned());
            }
        }
        tree.set_count(block_types, types.len() as i32)?;
        for (index, name) in types.iter().enumerate() {
            let item = tree.item(block_types, index as i32)?;
            tree.set_edit_value(item, name)?;
        }
        if let Some(block_type_index) = tree.elements(header, "Block Type Index")? {
            tree.set_count(block_type_index, count)?;
            for index in 0..count {
                let block = block(tree, index)?;
                let name = block_type(tree, block);
                let position = types
                    .iter()
                    .position(|known| same_text(known, name))
                    .map_or(-1, |p| p as i64);
                let item = tree.item(block_type_index, index)?;
                tree.set_native_value(item, Variant::Int(position))?;
            }
        }
    }

    if let Some(strings) = tree.elements(header, "Strings")? {
        if tree.nif.options.remove_unused_strings {
            let string_count = tree.count(strings) as usize;
            let mut map: Vec<i32> = vec![-1; string_count];
            // Map the used strings; the unused stay -1.
            let mut used_by: Vec<El> = Vec::new();
            for index in 0..count {
                let block = block(tree, index)?;
                if let Some(lists) = tree.block_lists(block) {
                    used_by.extend_from_slice(&lists.strings);
                }
            }
            for &string in &used_by {
                let l = tree.native_value(string)?.to_i32()?;
                if l >= 0 && (l as usize) < string_count {
                    map[l as usize] = l;
                }
            }
            // Reindex.
            let mut shift = 0;
            for entry in map.iter_mut() {
                if *entry == -1 {
                    shift += 1;
                } else if shift != 0 {
                    *entry -= shift;
                }
            }
            for &string in &used_by {
                let l = tree.native_value(string)?.to_i32()?;
                let new = if l >= 0 && (l as usize) < string_count {
                    map[l as usize]
                } else {
                    // An out of bounds string index is fixed.
                    -1
                };
                tree.set_native_value(string, Variant::Int(i64::from(new)))?;
            }
            for index in (0..string_count).rev() {
                if map[index] == -1 {
                    tree.delete(strings, index as i32)?;
                }
            }
        }
        let string_count = tree.count(strings);
        tree.set_native_values(header, "Num Strings", Variant::Int(i64::from(string_count)))?;
        let mut longest = 0;
        for index in 0..string_count {
            let item = tree.item(strings, index)?;
            let length = tree.edit_value(item)?.encode_utf16().count();
            longest = longest.max(length);
        }
        tree.set_native_values(header, "Max String Length", Variant::Int(longest as i64))?;
    }
    Ok(())
}

/// `UpdateNifVersion`: the version fields from the header.
pub fn update_nif_version(tree: &mut Tree) -> R<()> {
    let header = header(tree)?;
    let version = wb_nif_version_to_int(&tree.edit_values(header, "Version")?);
    let user_version = tree.native_values(header, "User Version")?.to_i64()? as u32;
    let user_version2 = tree.native_values(header, "User Version 2")?.to_i64()? as u32;
    tree.nif.version = version;
    tree.nif.user_version = user_version;
    tree.nif.user_version2 = user_version2;
    let v = i64::from(version);
    let nif_version = if v == V4002 {
        NifVersion::Tes3
    } else if v == V20005 && matches!(user_version, 0 | 11) && matches!(user_version2, 0 | 11) {
        // Some third party Oblivion meshes have user version 0.
        NifVersion::Tes4
    } else if v == V20004 && matches!(user_version, 10 | 11) && user_version2 == 11 {
        NifVersion::Tes4
    } else if v == V1010106 && user_version == 10 && user_version2 == 5 {
        NifVersion::Tes4
    } else if v == V10200 && user_version == 10 && matches!(user_version2, 6..=9 | 11) {
        NifVersion::Tes4
    } else if v == V20207 && user_version == 11 {
        NifVersion::Fo3
    } else if v == V20207 && user_version == 12 && user_version2 == 83 {
        NifVersion::Tes5
    } else if v == V20207 && user_version == 12 && user_version2 == 100 {
        NifVersion::Sse
    } else if v == V20207 && user_version == 12 && matches!(user_version2, 130 | 132) {
        NifVersion::Fo4
    } else {
        NifVersion::Unknown
    };
    tree.nif.nif_version = nif_version;
    if nif_version == NifVersion::Unknown {
        return Err(DfError::new(format!(
            "Unknown NIF version \"{}\" (\"User Version\"={user_version}, \"User Version 2\"={user_version2})",
            wb_int_to_nif_version(version)
        )));
    }
    Ok(())
}

/// `NifVersion := aVersion`.
pub fn set_nif_version(tree: &mut Tree, nif_version: NifVersion) -> R<()> {
    let header = header(tree)?;
    let (version, user_version, user_version2) = match nif_version {
        NifVersion::Tes3 => ("4.0.0.2", 0, 0),
        NifVersion::Tes4 => ("20.0.0.5", 11, 11),
        NifVersion::Fo3 => ("20.2.0.7", 11, 34),
        NifVersion::Tes5 => ("20.2.0.7", 12, 83),
        NifVersion::Sse => ("20.2.0.7", 12, 100),
        NifVersion::Fo4 => ("20.2.0.7", 12, 130),
        NifVersion::Unknown => return Ok(()),
    };
    tree.nif.user_version = user_version;
    tree.nif.user_version2 = user_version2;
    tree.nif.version = wb_nif_version_to_int(version);
    tree.set_native_values(header, "Version", Variant::Int(i64::from(tree.nif.version)))?;
    tree.set_native_values(header, "User Version", Variant::Int(i64::from(user_version)))?;
    tree.set_native_values(header, "User Version 2", Variant::Int(i64::from(user_version2)))?;
    let magic = if nif_version == NifVersion::Tes3 {
        format!("{NIF_MAGIC_NETIMMERSE}{version}")
    } else {
        format!("{NIF_MAGIC_GAMEBRYO}{version}")
    };
    tree.set_edit_values(header, "Magic", &magic)?;
    tree.nif.nif_version = nif_version;
    Ok(())
}

/// `AddBlock`: a new block before the footer; the first node block becomes
/// the root.
pub fn add_block(tree: &mut Tree, block_type: &str) -> R<El> {
    let root = tree.root_el();
    let def = wb_ni_object_def(block_type)?;
    let block = tree.create_element(def, Some(root))?;
    tree.set_to_default(block)?;
    let footer = footer(tree)?;
    let count = tree.count(root) as usize;
    tree.put(root, count - 1, block);
    tree.put(root, count, footer);
    if blocks_count(tree)? == 1
        && (wb_is_ni_object(block_type, "NiNode")
            || wb_is_ni_object(block_type, "NiSequence")
            || wb_is_ni_object(block_type, "NiSequenceStreamHelper"))
        && let Some(roots) = tree.elements(footer, "Roots")?
    {
        let entry = tree.add(roots)?;
        tree.set_native_value(entry, Variant::Int(0))?;
    }
    Ok(block)
}

/// `InsertBlock`.
pub fn insert_block(tree: &mut Tree, index: i32, block_type: &str) -> R<El> {
    block(tree, index)?;
    let result = add_block(tree, block_type)?;
    let current = tree.index(result)?;
    let root = tree.root_el();
    tree.move_item(root, current, index)?;
    Ok(result)
}

/// `CopyBlock`.
pub fn copy_block(tree: &mut Tree, index: i32) -> R<El> {
    let source = block(tree, index)?;
    let block_type = block_type(tree, source).to_owned();
    let result = add_block(tree, &block_type)?;
    tree.assign(result, Some(source))?;
    Ok(result)
}

/// `BlockByType`.
pub fn block_by_type(tree: &mut Tree, block_type_name: &str, inherited: bool) -> R<Option<El>> {
    for index in 0..blocks_count(tree)? {
        let block = block(tree, index)?;
        if block_is_ni_object(tree, block, block_type_name, inherited) {
            return Ok(Some(block));
        }
    }
    Ok(None)
}

/// `BlocksByType`.
pub fn blocks_by_type(tree: &mut Tree, block_type_name: &str, inherited: bool) -> R<Vec<El>> {
    let mut result = Vec::new();
    for index in 0..blocks_count(tree)? {
        let block = block(tree, index)?;
        if block_is_ni_object(tree, block, block_type_name, inherited) {
            result.push(block);
        }
    }
    Ok(result)
}

/// `BlockByName`.
pub fn block_by_name(tree: &mut Tree, block_name: &str, block_type_name: &str) -> R<Option<El>> {
    if block_name.is_empty() {
        return Err(DfError::new("Can not find block by an empty name"));
    }
    for index in 0..blocks_count(tree)? {
        let block = block(tree, index)?;
        if (block_type_name.is_empty() || block_is_ni_object(tree, block, block_type_name, true))
            && tree.edit_values(block, "Name")? == block_name
        {
            return Ok(Some(block));
        }
    }
    Ok(None)
}

/// `TwbNifFile.BlockByPath`: the first part is a block type or name.
pub fn block_by_path(tree: &mut Tree, block_path: &str) -> R<Option<El>> {
    let path = block_path.trim_start_matches('\\');
    if path.is_empty() {
        return Ok(None);
    }
    let (current, rest) = match path.find('\\') {
        Some(index) => (&path[..index], &path[index + 1..]),
        None => (path, ""),
    };
    for index in 0..blocks_count(tree)? {
        let block = block(tree, index)?;
        if same_text(block_type(tree, block), current) || tree.edit_values(block, "Name")? == current {
            return if rest.is_empty() {
                Ok(Some(block))
            } else {
                block_block_by_path(tree, block, rest)
            };
        }
    }
    Ok(None)
}

/// `GetUniqueName`. UPSTREAM-QUIRK: the counters accumulate
/// (`name:0:1`).
pub fn get_unique_name(tree: &mut Tree, name: &str) -> R<String> {
    let mut result = name.to_owned();
    let mut index = 0;
    while block_by_name(tree, &result, "")?.is_some() {
        result = format!("{result}:{index}");
        index += 1;
    }
    Ok(result)
}

// ---- TwbNifBlock ----

/// `AddRef`.
pub fn block_add_ref(tree: &mut Tree, block: El, element: El) {
    if let Some(lists) = tree.block_lists_mut(block) {
        lists.refs.push(element);
    }
}

/// `RemoveRef`.
pub fn block_remove_ref(tree: &mut Tree, block: El, element: El) {
    if tree.is_destroying(block) {
        return;
    }
    if let Some(lists) = tree.block_lists_mut(block)
        && let Some(position) = lists.refs.iter().position(|&item| item == element)
    {
        lists.refs.remove(position);
    }
}

/// `AddString`.
pub fn block_add_string(tree: &mut Tree, block: El, element: El) {
    if let Some(lists) = tree.block_lists_mut(block) {
        lists.strings.push(element);
    }
}

/// `RemoveString`.
pub fn block_remove_string(tree: &mut Tree, block: El, element: El) {
    if tree.is_destroying(block) {
        return;
    }
    if let Some(lists) = tree.block_lists_mut(block)
        && let Some(position) = lists.strings.iter().position(|&item| item == element)
    {
        lists.strings.remove(position);
    }
}

/// `Refs`: the NiRef and NiPtr elements of the block.
pub fn block_refs(tree: &Tree, block: El) -> Vec<El> {
    tree.block_lists(block)
        .map(|lists| lists.refs.clone())
        .unwrap_or_default()
}

/// `Strings`: the indexed string elements of the block.
pub fn block_strings(tree: &Tree, block: El) -> Vec<El> {
    tree.block_lists(block)
        .map(|lists| lists.strings.clone())
        .unwrap_or_default()
}

/// `IsNiObject`.
pub fn block_is_ni_object(tree: &Tree, block: El, template: &str, inherited: bool) -> bool {
    let block_type = block_type(tree, block);
    (!inherited && block_type == template) || (inherited && wb_is_ni_object(block_type, template))
}

/// `ReferencedBy`: the references of other blocks that point to the block.
pub fn block_referenced_by(tree: &mut Tree, block_el: El) -> R<Vec<El>> {
    let mut result = Vec::new();
    for index in 0..blocks_count(tree)? {
        let other = block(tree, index)?;
        if other == block_el {
            continue;
        }
        for reference in block_refs(tree, other) {
            if tree.links_to(reference)? == Some(block_el) {
                result.push(reference);
            }
        }
    }
    Ok(result)
}

/// `IsReferenced`.
pub fn block_is_referenced(tree: &mut Tree, block: El) -> R<bool> {
    Ok(!block_referenced_by(tree, block)?.is_empty())
}

/// `Hidden`.
pub fn block_is_hidden(tree: &mut Tree, block: El) -> R<bool> {
    if block_is_ni_object(tree, block, "NiAVObject", true) {
        Ok(tree.native_values(block, "Flags")?.to_i64()? & 1 != 0)
    } else {
        Ok(false)
    }
}

/// `IsBone`.
pub fn block_is_bone(tree: &mut Tree, block: El) -> R<bool> {
    if !block_is_ni_object(tree, block, "NiNode", true) {
        return Ok(false);
    }
    for reference in block_referenced_by(tree, block)? {
        let name = &tree.def(reference)?.name;
        if name.len() >= 5 && name[..5].eq_ignore_ascii_case("Bones") {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `IsDynamicRigidBody`.
pub fn block_is_dynamic_rigid_body(tree: &mut Tree, block: El) -> R<bool> {
    if !block_is_ni_object(tree, block, "bhkRigidBody", true) {
        return Ok(false);
    }
    let layer = tree.native_values(block, "Havok Filter\\Layer")?;
    let ms = tree.edit_values(block, "Motion System")?;
    let mq = tree.edit_values(block, "Motion Quality")?;
    // A keyframed biped layer is dynamic.
    let mut result = ms != "MO_SYS_INVALID" && ms != "MO_SYS_FIXED" && (ms != "MO_SYS_KEYFRAMED" || layer == 8);
    // In Skyrim not only the motion system defines static.
    if tree.nif.nif_version >= NifVersion::Tes5 {
        result = result && layer > 2 && mq != "MO_QUAL_INVALID" && mq != "MO_QUAL_FIXED";
    }
    Ok(result)
}

/// `IsEditorMarker`.
pub fn block_is_editor_marker(tree: &mut Tree, block: El) -> R<bool> {
    Ok(block_is_ni_object(tree, block, "NiObjectNET", true)
        && tree.edit_values(block, "Name")?.to_lowercase().contains("editormarker"))
}

fn first_none_or_add(tree: &mut Tree, list: El) -> R<El> {
    for index in 0..tree.count(list) {
        let item = tree.item(list, index)?;
        if tree.native_value(item)? == -1 {
            return Ok(item);
        }
    }
    tree.add(list)
}

/// `AddChild`.
pub fn block_add_child(tree: &mut Tree, block: El, block_type: &str) -> R<El> {
    let Some(children) = tree.elements(block, "Children")? else {
        return Err(tree.exception(block, "Can not have children"));
    };
    let child = first_none_or_add(tree, children)?;
    let result = add_block(tree, block_type)?;
    let index = tree.index(result)?;
    tree.set_native_value(child, Variant::Int(i64::from(index)))?;
    Ok(result)
}

/// `AddExtraData`.
pub fn block_add_extra_data(tree: &mut Tree, block: El, block_type: &str) -> R<El> {
    let datas = match tree.elements(block, "Extra Data List")? {
        Some(datas) => Some(datas),
        None => tree.elements(block, "Extra Data")?,
    };
    let Some(datas) = datas else {
        return Err(tree.exception(block, "Can not have extra data"));
    };
    // A single value in Morrowind meshes.
    let data = if tree.class(datas).is_value() {
        datas
    } else {
        first_none_or_add(tree, datas)?
    };
    if tree.native_value(data)? != -1 {
        return Err(tree.exception(block, "Can not add more extra data"));
    }
    let result = add_block(tree, block_type)?;
    let index = tree.index(result)?;
    tree.set_native_value(data, Variant::Int(i64::from(index)))?;
    Ok(result)
}

/// `AddProperty`.
pub fn block_add_property(tree: &mut Tree, block: El, block_type: &str) -> R<El> {
    for (field, template, message) in [
        (
            "Shader Property",
            "BSShaderProperty",
            "Block already has a shader property, can not add a new one",
        ),
        (
            "Alpha Property",
            "NiAlphaProperty",
            "Block already has an alpha property, can not add a new one",
        ),
    ] {
        if let Some(property) = tree.elements(block, field)?
            && wb_is_ni_object(block_type, template)
        {
            if tree.links_to(property)?.is_some() {
                return Err(DfError::new(message));
            }
            let result = add_block(tree, block_type)?;
            let index = tree.index(result)?;
            tree.set_native_value(property, Variant::Int(i64::from(index)))?;
            return Ok(result);
        }
    }
    let Some(properties) = tree.elements(block, "Properties")? else {
        return Err(tree.exception(block, "Block can not have properties"));
    };
    let property = first_none_or_add(tree, properties)?;
    let result = add_block(tree, block_type)?;
    let index = tree.index(result)?;
    tree.set_native_value(property, Variant::Int(i64::from(index)))?;
    Ok(result)
}

fn linked_named(tree: &mut Tree, reference: El, name: &str) -> R<Option<El>> {
    match tree.links_to(reference)? {
        Some(target) if tree.edit_values(target, "Name")? == name => Ok(Some(target)),
        _ => Ok(None),
    }
}

/// `PropertyByName`.
pub fn block_property_by_name(tree: &mut Tree, block: El, name: &str) -> R<Option<El>> {
    for field in ["Shader Property", "Alpha Property"] {
        if let Some(property) = tree.elements(block, field)?
            && let Some(found) = linked_named(tree, property, name)?
        {
            return Ok(Some(found));
        }
    }
    if let Some(properties) = tree.elements(block, "Properties")? {
        for index in 0..tree.count(properties) {
            let item = tree.item(properties, index)?;
            if let Some(found) = linked_named(tree, item, name)? {
                return Ok(Some(found));
            }
        }
    }
    Ok(None)
}

/// `ExtraDataByName`.
pub fn block_extra_data_by_name(tree: &mut Tree, block: El, name: &str) -> R<Option<El>> {
    if let Some(datas) = tree.elements(block, "Extra Data List")? {
        for index in 0..tree.count(datas) {
            let item = tree.item(datas, index)?;
            if let Some(found) = linked_named(tree, item, name)? {
                return Ok(Some(found));
            }
        }
    } else if let Some(data) = tree.elements(block, "Extra Data")?
        && let Some(found) = linked_named(tree, data, name)?
    {
        return Ok(Some(found));
    }
    Ok(None)
}

fn push_if_matches(tree: &mut Tree, reference: El, block_type: &str, inherited: bool, out: &mut Vec<El>) -> R<()> {
    if let Some(target) = tree.links_to(reference)?
        && block_is_ni_object(tree, target, block_type, inherited)
    {
        out.push(target);
    }
    Ok(())
}

/// `ChildrenByType`.
pub fn block_children_by_type(tree: &mut Tree, block: El, block_type: &str, inherited: bool) -> R<Vec<El>> {
    let mut result = Vec::new();
    if let Some(children) = tree.elements(block, "Children")? {
        for index in 0..tree.count(children) {
            let item = tree.item(children, index)?;
            push_if_matches(tree, item, block_type, inherited, &mut result)?;
        }
    }
    Ok(result)
}

/// `ChildByType`.
pub fn block_child_by_type(tree: &mut Tree, block: El, block_type: &str, inherited: bool) -> R<Option<El>> {
    Ok(block_children_by_type(tree, block, block_type, inherited)?
        .first()
        .copied())
}

/// `PropertiesByType`.
pub fn block_properties_by_type(tree: &mut Tree, block: El, block_type: &str, inherited: bool) -> R<Vec<El>> {
    let mut result = Vec::new();
    for field in ["Shader Property", "Alpha Property"] {
        if let Some(property) = tree.elements(block, field)? {
            push_if_matches(tree, property, block_type, inherited, &mut result)?;
        }
    }
    if let Some(properties) = tree.elements(block, "Properties")? {
        for index in 0..tree.count(properties) {
            let item = tree.item(properties, index)?;
            push_if_matches(tree, item, block_type, inherited, &mut result)?;
        }
    }
    Ok(result)
}

/// `PropertyByType`.
pub fn block_property_by_type(tree: &mut Tree, block: El, block_type: &str, inherited: bool) -> R<Option<El>> {
    Ok(block_properties_by_type(tree, block, block_type, inherited)?
        .first()
        .copied())
}

/// `ExtraDatasByType`.
pub fn block_extra_datas_by_type(tree: &mut Tree, block: El, block_type: &str, inherited: bool) -> R<Vec<El>> {
    let mut result = Vec::new();
    if let Some(datas) = tree.elements(block, "Extra Data List")? {
        for index in 0..tree.count(datas) {
            let item = tree.item(datas, index)?;
            push_if_matches(tree, item, block_type, inherited, &mut result)?;
        }
    } else if let Some(data) = tree.elements(block, "Extra Data")? {
        push_if_matches(tree, data, block_type, inherited, &mut result)?;
    }
    Ok(result)
}

/// `ExtraDataByType`.
pub fn block_extra_data_by_type(tree: &mut Tree, block: El, block_type: &str, inherited: bool) -> R<Option<El>> {
    Ok(block_extra_datas_by_type(tree, block, block_type, inherited)?
        .first()
        .copied())
}

/// `TwbNifBlock.BlockByPath`: follows the references by block type or name.
pub fn block_block_by_path(tree: &mut Tree, block: El, block_path: &str) -> R<Option<El>> {
    let path = block_path.trim_start_matches('\\');
    if path.is_empty() {
        return Ok(None);
    }
    let (current, rest) = match path.find('\\') {
        Some(index) => (&path[..index], &path[index + 1..]),
        None => (path, ""),
    };
    for reference in block_refs(tree, block) {
        let Some(target) = tree.links_to(reference)? else {
            continue;
        };
        if same_text(block_type(tree, target), current) || tree.edit_values(target, "Name")? == current {
            return if rest.is_empty() {
                Ok(Some(target))
            } else {
                block_block_by_path(tree, target, rest)
            };
        }
    }
    Ok(None)
}

/// `GetStringPaletteString`: the string at the offset `index` of the
/// palette.
pub fn block_get_string_palette_string(tree: &mut Tree, block: El, index: i32) -> R<String> {
    if block_type(tree, block) != "NiStringPalette" {
        return Ok(String::new());
    }
    let palette = tree.native_values(block, "Palette")?.to_str()?;
    let length = palette.encode_utf16().count() as i32;
    if index < 0 || index > length {
        return Ok(String::new());
    }
    let mut offset = 0;
    for part in palette.split('\0') {
        if index == offset {
            return Ok(part.to_owned());
        }
        offset += part.encode_utf16().count() as i32 + 1;
    }
    Ok(String::new())
}
