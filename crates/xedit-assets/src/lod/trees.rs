// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbLOD.pas (TwbLodSettings, TwbLodTES5Tree,
// TwbLodTES5TreeList, TwbLodTES5TreeBlock)

//! The LOD settings of a worldspace and the tree LOD of Skyrim and the
//! Fallouts: the billboards of the trees, their atlas and list file
//! (`.lst`), and the blocks of tree references (`.btt`, `.dtl`).

use xedit_core::interface::form_id::{FileID, FormID};
use xedit_core::interface::globals::{is_fallout3, is_skyrim};

use super::atlas::{BinBlock, BinPacker};
use super::{
    LodEnv, LodError, LodResult, change_file_ext, extract_file_name, load_image_from_memory, lod_tree_block_file_ext,
};
use crate::imaging::{
    ImageData, ImageFormat, SamplingFilter, canvases, convert_image, copy_rect, generate_mip_maps,
    load_image_from_memory as imaging_load, new_image, save_multi_image_to_dds,
};

/// `TwbLodSettings`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LodSettings {
    pub sw_cell: (i32, i32),
    pub ne_cell: (i32, i32),
    pub stride: i32,
    pub lod_level_min: i32,
    pub lod_level_max: i32,
    pub object_level: i32,
}

impl LodSettings {
    /// `Init`.
    pub fn init() -> LodSettings {
        LodSettings {
            lod_level_min: 4,
            lod_level_max: 32,
            ..LodSettings::default()
        }
    }

    /// `GetSize`.
    pub fn size(&self) -> i32 {
        (f64::from(self.stride) / 2f64.sqrt()).ceil() as i32
    }

    /// `BlockForCell`.
    pub fn block_for_cell(&self, cell: (i32, i32), lod_level: i32) -> (i32, i32) {
        (
            self.sw_cell.0 + ((cell.0 - self.sw_cell.0) / lod_level) * lod_level,
            self.sw_cell.1 + ((cell.1 - self.sw_cell.1) / lod_level) * lod_level,
        )
    }

    /// `LoadFromData`.
    pub fn load_from_data(data: &[u8]) -> LodResult<LodSettings> {
        let i32_at = |at: usize| i32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        let i16_at = |at: usize| i32::from(i16::from_le_bytes(data[at..at + 2].try_into().unwrap()));
        let mut settings = LodSettings::default();
        if is_fallout3() {
            if data.len() != 24 {
                return Err(LodError::new("Invalid lodsettings file"));
            }
            settings.lod_level_min = i32_at(0);
            settings.lod_level_max = i32_at(4);
            settings.stride = i32_at(8);
            settings.sw_cell = (i16_at(12), i16_at(14));
            settings.ne_cell = (i16_at(16), i16_at(18));
            settings.object_level = i32_at(20);
        } else {
            if data.len() != 16 {
                return Err(LodError::new("Invalid lodsettings file"));
            }
            settings.sw_cell = (i16_at(0), i16_at(2));
            settings.stride = i32_at(4);
            settings.lod_level_min = i32_at(8);
            settings.lod_level_max = i32_at(12);
        }
        Ok(settings)
    }
}

/// `TwbLodTES5TreeType`: an entry of the list file.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TreeType {
    pub index: i32,
    pub width: f32,
    pub height: f32,
    pub uv_min_x: f32,
    pub uv_min_y: f32,
    pub uv_max_x: f32,
    pub uv_max_y: f32,
    pub unknown: i32,
}

/// `TwbLodTES5TreeRef`: an entry of a block file.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TreeRef {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation: f32,
    pub scale: f32,
    pub ref_form_id: u32,
    pub unknown1: i32,
    pub unknown2: i32,
}

impl TreeRef {
    fn write(&self, out: &mut Vec<u8>) {
        for value in [self.x, self.y, self.z, self.rotation, self.scale] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&self.ref_form_id.to_le_bytes());
        out.extend_from_slice(&self.unknown1.to_le_bytes());
        out.extend_from_slice(&self.unknown2.to_le_bytes());
    }

    fn read(data: &[u8]) -> TreeRef {
        let f = |at: usize| f32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        let i = |at: usize| i32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        TreeRef {
            x: f(0),
            y: f(4),
            z: f(8),
            rotation: f(12),
            scale: f(16),
            ref_form_id: i(20) as u32,
            unknown1: i(24),
            unknown2: i(28),
        }
    }
}

/// `TwbLodTES5Tree`: a tree while LOD is built.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub index: i32,
    pub form_id: FormID,
    pub billboard: String,
    pub crc32: i32,
    pub width: f32,
    pub height: f32,
    pub shift_x: f32,
    pub shift_y: f32,
    pub shift_z: f32,
    pub scale_factor: f32,
    pub image: ImageData,
}

/// `TwbLodTES5TreeList`: the trees of a worldspace, their atlas and list.
#[derive(Debug, Clone)]
pub struct TreeList {
    pub worldspace_id: String,
    pub trees_list: Vec<TreeType>,
    pub trees: Vec<Tree>,
    pub atlas: ImageData,
    pub ref_form_ids: Vec<u32>,
    pub ref_allow_duplicates: bool,
}

impl TreeList {
    /// `Create`.
    pub fn new(worldspace_id: &str) -> TreeList {
        TreeList {
            worldspace_id: if worldspace_id.is_empty() {
                "Tamriel".to_owned()
            } else {
                worldspace_id.to_owned()
            },
            trees_list: Vec::new(),
            trees: Vec::new(),
            atlas: ImageData::default(),
            ref_form_ids: Vec::new(),
            ref_allow_duplicates: false,
        }
    }

    /// `GetListFileName`.
    pub fn list_file_name(&self) -> String {
        let id = &self.worldspace_id;
        if is_skyrim() {
            format!("Meshes\\Terrain\\{id}\\Trees\\{id}.lst")
        } else if is_fallout3() {
            format!("Meshes\\Landscape\\LOD\\{id}\\Trees\\TreeTypes.lst")
        } else {
            String::new()
        }
    }

    /// `GetAtlasFileName`.
    pub fn atlas_file_name(&self) -> String {
        let id = &self.worldspace_id;
        if is_skyrim() {
            format!("Textures\\Terrain\\{id}\\Trees\\{id}TreeLod.dds")
        } else if is_fallout3() {
            if id.len() >= 4 && id[..4].eq_ignore_ascii_case("DLC4") {
                "Textures\\Landscape\\Trees\\TreeSwampLod.dds".to_owned()
            } else {
                "Textures\\Landscape\\Trees\\TreeDeadLod.dds".to_owned()
            }
        } else {
            String::new()
        }
    }

    /// `GetAtlasRect`.
    pub fn atlas_rect(&self, index: usize) -> (i32, i32, i32, i32) {
        let t = &self.trees_list[index];
        let r = |value: f64| crate::imaging::formats::round(value) as i32;
        let (w, h) = (f64::from(self.atlas.width), f64::from(self.atlas.height));
        (
            r(w * f64::from(t.uv_min_x)),
            r(h * f64::from(t.uv_min_y)),
            r(w * (f64::from(t.uv_max_x) - f64::from(t.uv_min_x))),
            r(h * (f64::from(t.uv_max_y) - f64::from(t.uv_min_y))),
        )
    }

    /// `GetTreeByFormID`.
    pub fn tree_by_form_id(&self, form_id: FormID) -> Option<usize> {
        self.trees.iter().position(|tree| tree.form_id == form_id)
    }

    /// `BillboardFileName`.
    pub fn billboard_file_name(file_name: &str, model_name: &str, form_id: FormID) -> String {
        format!(
            "Textures\\Terrain\\LODGen\\{file_name}\\{}_{}.dds",
            change_file_ext(extract_file_name(model_name), ""),
            form_id.change_file_id(FileID::null()).to_string(false)
        )
    }

    /// `AddTree`.
    pub fn add_tree(&mut self, file_name: &str, model_name: &str, form_id: FormID, width: f32, height: f32) -> usize {
        let idx = self.trees.iter().map(|tree| tree.index).fold(-1, i32::max);
        self.trees.push(Tree {
            index: idx + 1,
            form_id,
            billboard: Self::billboard_file_name(file_name, model_name, form_id),
            width,
            height,
            shift_x: 0.0,
            shift_y: 0.0,
            shift_z: 0.0,
            scale_factor: 1.0,
            ..Tree::default()
        });
        self.trees.len() - 1
    }

    /// `LoadFromData` of a list file.
    pub fn load_from_data(&mut self, data: &[u8]) -> LodResult<()> {
        if data.len() < 4 {
            self.trees_list.clear();
            return Ok(());
        }
        let count = i32::from_le_bytes(data[0..4].try_into().unwrap());
        if (data.len() as i64 - 4) < 32 * i64::from(count) {
            return Err(LodError::new("Invalid LST file"));
        }
        let f = |at: usize| f32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        let i = |at: usize| i32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        self.trees_list = (0..count.max(0) as usize)
            .map(|n| {
                let at = 4 + n * 32;
                TreeType {
                    index: i(at),
                    width: f(at + 4),
                    height: f(at + 8),
                    uv_min_x: f(at + 12),
                    uv_min_y: f(at + 16),
                    uv_max_x: f(at + 20),
                    uv_max_y: f(at + 24),
                    unknown: i(at + 28),
                }
            })
            .collect();
        Ok(())
    }

    /// `SaveToFile` of the list: nothing for an empty list.
    pub fn save_to_file(&self, file_name: &str) -> LodResult<()> {
        if self.trees_list.is_empty() {
            return Ok(());
        }
        let mut out = Vec::with_capacity(4 + 32 * self.trees_list.len());
        out.extend_from_slice(&(self.trees_list.len() as i32).to_le_bytes());
        for t in &self.trees_list {
            out.extend_from_slice(&t.index.to_le_bytes());
            for value in [t.width, t.height, t.uv_min_x, t.uv_min_y, t.uv_max_x, t.uv_max_y] {
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.extend_from_slice(&t.unknown.to_le_bytes());
        }
        std::fs::write(file_name, out)?;
        Ok(())
    }

    /// `LoadAtlas`.
    pub fn load_atlas(&mut self, data: &[u8]) {
        self.atlas = imaging_load(data).ok().flatten().unwrap_or_default();
    }

    /// `SaveAtlas`: as DXT3 with mipmaps.
    pub fn save_atlas(&mut self, file_name: &str) -> LodResult<()> {
        if !convert_image(&mut self.atlas, ImageFormat::Dxt3)? {
            return Err(LodError::new("Can't compress atlas"));
        }
        let mipmaps = generate_mip_maps(&self.atlas, 0, SamplingFilter::Lanczos)?;
        if mipmaps.is_empty() {
            return Err(LodError::new("Can't generate mipmaps"));
        }
        std::fs::write(file_name, save_multi_image_to_dds(&mipmaps)?)?;
        Ok(())
    }

    /// `ChangeAtlasBrightness`.
    pub fn change_atlas_brightness(&mut self, brightness: i32) -> LodResult<()> {
        if brightness == 0 {
            return Ok(());
        }
        canvases::modify_contrast_brightness(
            &mut self.atlas,
            (f64::from(brightness) / 10.0) as f32,
            brightness as f32,
        )?;
        Ok(())
    }

    /// `SaveFromAtlas`: one billboard as a DXT3 file.
    pub fn save_from_atlas(&self, index: usize, file_name: &str) -> LodResult<()> {
        if self.atlas.format == ImageFormat::Unknown {
            return Ok(());
        }
        let (x, y, w, h) = self.atlas_rect(index);
        let mut img = new_image(w, h, ImageFormat::Dxt3)?;
        copy_rect(&self.atlas, x, y, w, h, &mut img, 0, 0)?;
        std::fs::write(file_name, save_multi_image_to_dds(std::slice::from_ref(&img))?)?;
        Ok(())
    }

    /// `BuildAtlas`: the distinct billboards (by checksum) packed on an
    /// atlas from 512 pixels on, doubled until they fit, and the list.
    pub fn build_atlas(&mut self, max_atlas_size: i32) -> LodResult<bool> {
        let mut blocks: Vec<BinBlock> = Vec::new();
        for i in 0..self.trees.len() {
            // skip trees with missing/invalid textures
            if self.trees[i].index == -1 {
                continue;
            }
            // exclude duplicate textures by checksum
            if self.trees[..i].iter().any(|tree| tree.crc32 == self.trees[i].crc32) {
                continue;
            }
            blocks.push(BinBlock {
                index: self.trees[i].index,
                w: self.trees[i].image.width,
                h: self.trees[i].image.height,
                data: self.trees[i].crc32,
                ..BinBlock::default()
            });
        }
        if blocks.is_empty() {
            return Ok(false);
        }
        let mut packer = BinPacker {
            width: 512.min(max_atlas_size),
            height: 512.min(max_atlas_size),
            padding_x: 2,
            padding_y: 2,
        };
        while !packer.fit(&mut blocks) {
            if packer.width <= packer.height {
                packer.width *= 2;
            } else {
                packer.height *= 2;
            }
            if packer.width > max_atlas_size || packer.height > max_atlas_size {
                return Err(LodError::new("Can't fit billboards on atlas, not enough space"));
            }
        }
        self.atlas = new_image(packer.width, packer.height, ImageFormat::Default)?;
        for i in 0..self.trees.len() {
            if self.trees[i].index == -1 {
                continue;
            }
            let block = *blocks
                .iter()
                .find(|block| block.data == self.trees[i].crc32)
                .ok_or_else(|| LodError::new("Error when drawing atlas"))?;
            if !copy_rect(
                &self.trees[i].image,
                0,
                0,
                block.w,
                block.h,
                &mut self.atlas,
                block.x,
                block.y,
            )? {
                return Err(LodError::new("Error when drawing atlas"));
            }
            let (aw, ah) = (f64::from(self.atlas.width), f64::from(self.atlas.height));
            self.trees_list.push(TreeType {
                index: self.trees[i].index,
                width: self.trees[i].width,
                height: self.trees[i].height,
                uv_min_x: (f64::from(block.x) / aw) as f32,
                uv_max_x: (f64::from(block.x + block.w) / aw) as f32,
                uv_min_y: (f64::from(block.y) / ah) as f32,
                uv_max_y: (f64::from(block.y + block.h) / ah) as f32,
                unknown: 0,
            });
        }
        Ok(true)
    }
}

/// `TwbLodTES5Tree.LoadFromData`.
pub fn load_tree_image(env: &LodEnv, tree: &mut Tree, data: &[u8]) -> bool {
    match load_image_from_memory(env, data) {
        Some(image) => {
            tree.image = image;
            true
        }
        None => false,
    }
}

/// `TwbLodTES5TreeBlock`: the tree references of a block.
#[derive(Debug, Clone, Default)]
pub struct TreeBlock {
    pub cell: (i32, i32),
    pub lod_level: i32,
    /// The tree types with their counts (`Types`).
    pub types: Vec<(i32, i32)>,
    /// The references of each type, in steps of 64 (`Refs`).
    pub refs: Vec<Vec<TreeRef>>,
}

impl TreeBlock {
    /// `Init`.
    pub fn new(cell: (i32, i32), lod_level: i32) -> TreeBlock {
        TreeBlock {
            cell,
            lod_level,
            ..TreeBlock::default()
        }
    }

    /// `GetBlockFileName`.
    pub fn file_name(&self, list: &TreeList) -> String {
        let id = &list.worldspace_id;
        let (level, x, y) = (self.lod_level, self.cell.0, self.cell.1);
        if is_skyrim() {
            format!(
                "meshes\\terrain\\{id}\\trees\\{id}.{level}.{x}.{y}.{}",
                lod_tree_block_file_ext()
            )
        } else if is_fallout3() {
            format!(
                "meshes\\landscape\\lod\\{id}\\trees\\{id}.level{level}.x{x}.y{y}.{}",
                lod_tree_block_file_ext()
            )
        } else {
            String::new()
        }
    }

    /// `LoadFromData`.
    pub fn load_from_data(&mut self, data: &[u8]) -> LodResult<()> {
        self.types.clear();
        self.refs.clear();
        if data.len() < 4 {
            return Ok(());
        }
        let err = || LodError::new("Invalid tree LOD block file");
        let i32_at = |at: usize| -> LodResult<i32> {
            data.get(at..at + 4)
                .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()))
                .ok_or_else(err)
        };
        let mut p = 0;
        let types = i32_at(p)?;
        p += 4;
        for _ in 0..types.max(0) {
            if p >= data.len() {
                return Err(err());
            }
            let index = i32_at(p)?;
            p += 4;
            if p >= data.len() {
                return Err(err());
            }
            let count = i32_at(p)?;
            p += 4;
            if p >= data.len() {
                return Err(err());
            }
            let mut refs = Vec::with_capacity(count.max(0) as usize);
            for _ in 0..count.max(0) {
                let bytes = data.get(p..p + 32).ok_or_else(err)?;
                refs.push(TreeRef::read(bytes));
                p += 32;
            }
            self.types.push((index, count));
            self.refs.push(refs);
        }
        Ok(())
    }

    /// `SaveToFile`.
    pub fn save_to_file(&self, file_name: &str) -> LodResult<()> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.types.len() as i32).to_le_bytes());
        for (i, (index, count)) in self.types.iter().enumerate() {
            out.extend_from_slice(&index.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            for tree_ref in self.refs[i].iter().take(*count as usize) {
                tree_ref.write(&mut out);
            }
        }
        std::fs::write(file_name, out)?;
        Ok(())
    }

    /// `AddReference`: false for a FormID number the list has already
    /// (unless duplicates are allowed). The rotation is random.
    pub fn add_reference(
        &mut self,
        list: &mut TreeList,
        form_id: FormID,
        tree_index: i32,
        pos: (f32, f32, f32),
        scale: f32,
    ) -> bool {
        if !list.ref_allow_duplicates && list.ref_form_ids.contains(&form_id.object_id()) {
            return false;
        }
        let j = match self.types.iter().position(|(index, _)| *index == tree_index) {
            Some(j) => j,
            None => {
                self.types.push((tree_index, 0));
                self.refs.push(Vec::new());
                self.types.len() - 1
            }
        };
        self.types[j].1 += 1;
        let count = self.types[j].1 as usize;
        if count > self.refs[j].len() {
            let len = self.refs[j].len() + 64;
            self.refs[j].resize(len, TreeRef::default());
        }
        let entry = &mut self.refs[j][count - 1];
        entry.ref_form_id = form_id.to_cardinal();
        entry.x = pos.0;
        entry.y = pos.1;
        entry.z = pos.2;
        entry.scale = scale;
        entry.rotation = (2.0 * std::f64::consts::PI * super::random()) as f32;
        list.ref_form_ids.push(form_id.object_id());
        true
    }
}
