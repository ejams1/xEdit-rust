// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbLOD.pas (TwbBinPacker, wbPrepareImageAlpha,
// wbGetUVRangeTexturesList, wbBuildAtlas, wbBuildAtlasFromTexturesList,
// wbBuildAtlasFromAtlasMap)

//! The texture atlases of LOD: the bin packer, the textures of the LOD
//! meshes that fit the UV range, and the atlases of their diffuse, normal
//! and specular maps with the map of where each texture went.

use xedit_core::interface::globals::{app_name, is_fallout3, is_fallout4};
use xedit_io::archive::{AssetType, get_asset_name};

use super::{
    BILLBOARD_FLAG, LodEnv, LodError, LodResult, StringList, change_file_ext, default_normal_texture,
    default_specular_texture, delimited_text, extract_file_ext, extract_file_path, force_directories,
    load_image_from_memory, message, open_resource, resource_exists,
};
use crate::data_format_material::MaterialFile;
use crate::data_format_nif::{NifFile, block, block_is_ni_object, block_property_by_type, blocks_count};
use crate::imaging::{
    ImageData, ImageFormat, ResizeFilter, SamplingFilter, canvases, convert_image, copy_rect, format_info,
    generate_mip_maps, get_pixel32, new_image, resize_image, save_multi_image_to_dds, set_pixel32,
};
use crate::sniff::processor::access_violation;

/// `TBinBlock`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BinBlock {
    pub index: i32,
    pub fit: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub data: i32,
}

/// `TBinNode`.
#[derive(Debug, Default)]
struct BinNode {
    used: bool,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    down: Option<Box<BinNode>>,
    right: Option<Box<BinNode>>,
}

/// `TwbBinPacker`: the binary tree packer of
/// http://codeincomplete.com/posts/2011/5/7/bin_packing/.
#[derive(Debug, Clone, Copy)]
pub struct BinPacker {
    pub width: i32,
    pub height: i32,
    pub padding_x: i32,
    pub padding_y: i32,
}

impl Default for BinPacker {
    fn default() -> Self {
        BinPacker {
            width: 1024,
            height: 1024,
            padding_x: 0,
            padding_y: 0,
        }
    }
}

impl BinPacker {
    /// `FindNode`: the free node a block fits in, right before down.
    fn find_node(root: &mut BinNode, w: i32, h: i32) -> Option<&mut BinNode> {
        if root.used {
            // Two passes so that the borrow of the right branch ends first.
            let in_right = root
                .right
                .as_deref_mut()
                .is_some_and(|right| Self::find_node(right, w, h).is_some());
            if in_right {
                return Self::find_node(root.right.as_deref_mut().unwrap(), w, h);
            }
            root.down.as_deref_mut().and_then(|down| Self::find_node(down, w, h))
        } else if w <= root.w && h <= root.h {
            Some(root)
        } else {
            None
        }
    }

    /// `SplitNode`.
    fn split_node(&self, node: &mut BinNode, w: i32, h: i32) {
        node.used = true;
        node.down = Some(Box::new(BinNode {
            x: node.x,
            y: node.y + h + self.padding_y,
            w: node.w,
            h: node.h - h - self.padding_y,
            ..BinNode::default()
        }));
        node.right = Some(Box::new(BinNode {
            x: node.x + w + self.padding_x,
            y: node.y,
            w: node.w - w - self.padding_x,
            h,
            ..BinNode::default()
        }));
    }

    /// `Fit`: places the blocks, sorted by their larger side first (a
    /// bubble sort); false when one does not fit.
    pub fn fit(&self, blocks: &mut [BinBlock]) -> bool {
        // `MaxSideSort`
        let mut changed = true;
        while changed {
            changed = false;
            for i in 0..blocks.len().saturating_sub(1) {
                if blocks[i].w.max(blocks[i].h) < blocks[i + 1].w.max(blocks[i + 1].h) {
                    blocks.swap(i, i + 1);
                    changed = true;
                }
            }
        }
        let mut result = true;
        let mut root = BinNode {
            w: self.width,
            h: self.height,
            ..BinNode::default()
        };
        for block in blocks.iter_mut() {
            match Self::find_node(&mut root, block.w, block.h) {
                Some(node) => {
                    self.split_node(node, block.w, block.h);
                    block.x = node.x;
                    block.y = node.y;
                    block.fit = true;
                }
                None => result = false,
            }
        }
        result
    }
}

/// `wbPrepareImageAlpha`: the alpha of a format that keeps none set to
/// full, and with a threshold the alpha made 0 or 255 for DXT1.
pub fn prepare_image_alpha(image: &mut ImageData, format: ImageFormat, threshold: i32) {
    let Some(info) = format_info(format) else {
        return;
    };
    // do not convert formats that support alpha (DXT1 supports 0/1 transparency)
    if info.has_alpha_channel && format != ImageFormat::Dxt1 {
        return;
    }
    let threshold = if info.has_alpha_channel { threshold } else { 0 };
    for x in 0..image.width {
        for y in 0..image.height {
            let mut c = get_pixel32(image, x, y);
            c.a = if i32::from(c.a) >= threshold { 255 } else { 0 };
            set_pixel32(image, x, y, c);
        }
    }
}

/// `sMeshForceUntiled`.
const MESH_FORCE_UNTILED: &str = "meshes\\lod\\whiterun\\wrplainsdistrictterrainlod_lod.nif\
,meshes\\lod\\whiterun\\wrplainsdistrictterrainlod_lod_2.nif\
,meshes\\lod\\landscape\\roads\\bridge\\roadstrbridgemidend02_lod_0.nif\
,meshes\\lod\\neighborhoods\\esplanade\\esplanade_bld20lod.nif";

/// `wbGetUVRangeTexturesList`: the diffuse textures of the shapes of the
/// LOD meshes whose UVs stay in the range (a mesh named in
/// `sMeshForceUntiled` counts as untiled), and the billboards (`.dds`
/// entries) flagged as such.
pub fn get_uv_range_textures_list(meshes: &StringList, textures: &mut StringList, uv_range: f32) -> LodResult<()> {
    // UV values outside of this +/- range are errors in meshes and ignored
    const CHECK_RANGE: f64 = 100.0;
    let add_texture = |textures: &mut StringList, s: &str| {
        if !s.is_empty() {
            let t = get_asset_name(s, "", AssetType::Texture);
            if textures.index_of(&t).is_none() {
                textures.add(&t);
            }
        }
    };
    let uv_range = f64::from(uv_range);
    let tiled = |u: f32, v: f32| -> bool {
        let (u, v) = (f64::from(u), f64::from(v));
        !(u.is_nan() || v.is_nan())
            && u > -CHECK_RANGE
            && u < CHECK_RANGE
            && v > -CHECK_RANGE
            && v < CHECK_RANGE
            && (u < -uv_range || u > uv_range || v < -uv_range || v > uv_range)
    };
    let err = |e: crate::data_format::DfError| LodError::new(e.to_string());
    for mesh in meshes.strings() {
        // if dds is passed then use it as is (billboard for 3D Trees LOD)
        if extract_file_ext(mesh).eq_ignore_ascii_case(".dds") {
            textures.add_object(mesh, BILLBOARD_FLAG);
            continue;
        }
        let nifname = get_asset_name(mesh, "", AssetType::Mesh);
        if extract_file_ext(&nifname) != ".nif" {
            continue;
        }
        if !resource_exists(&nifname) {
            message(&format!("<Warning: LOD mesh not found \"{nifname}\">"));
            continue;
        }
        let mut nif = NifFile::new().map_err(err)?;
        let loaded = match open_resource_data(&nifname) {
            Some(data) => nif.load_from_data(&data),
            None => Err(crate::data_format::DfError::new(format!(
                "Resource not found (File \"{nifname}\")"
            ))),
        };
        if let Err(error) = loaded {
            message(&format!("<Warning: Error when loading \"{nifname}\": {error}>"));
            continue;
        }
        let untiled = MESH_FORCE_UNTILED.contains(nifname.as_str());
        let tree = &mut nif.tree;
        for b in 0..blocks_count(tree).map_err(err)? {
            let shape = block(tree, b).map_err(err)?;
            if block_is_ni_object(tree, shape, "BSTriShape", true) {
                // skip if no shader
                let Some(shader) =
                    block_property_by_type(tree, shape, "BSLightingShaderProperty", false).map_err(err)?
                else {
                    continue;
                };
                // skip if no UVs
                if !tree
                    .native_values(shape, "VertexDesc\\VF\\VF_UV")
                    .map_err(err)?
                    .to_bool()
                    .map_err(err)?
                {
                    continue;
                }
                let Some(entries) = tree.elements(shape, "Vertex Data").map_err(err)? else {
                    continue;
                };
                let mut is_tiled = tree.count(entries) == 0;
                if !untiled {
                    for j in 0..tree.count(entries) {
                        let item = tree.item(entries, j).map_err(err)?;
                        let entry = tree
                            .elements(item, "UV")
                            .map_err(err)?
                            .ok_or_else(access_violation)
                            .map_err(err)?;
                        let u = tree.native_values(entry, "U").map_err(err)?.to_f64().map_err(err)? as f32;
                        let v = tree.native_values(entry, "V").map_err(err)?.to_f64().map_err(err)? as f32;
                        is_tiled = tiled(u, v);
                        if is_tiled {
                            break;
                        }
                    }
                }
                if !is_tiled {
                    let name = tree.edit_values(shader, "Name").map_err(err)?;
                    let s = get_asset_name(&name, "", AssetType::Material);
                    // getting textures from material first, it has priority over textureset
                    if extract_file_ext(&s) == ".bgsm" && resource_exists(&s) {
                        match load_material(&s) {
                            Ok(diffuse) => add_texture(textures, &diffuse),
                            Err(error) => message(&format!("<Warning: Error when loading \"{s}\": {error}>")),
                        }
                    } else {
                        let set_ref = tree
                            .elements(shader, "Texture Set")
                            .map_err(err)?
                            .ok_or_else(access_violation)
                            .map_err(err)?;
                        let Some(texture_set) = tree.links_to(set_ref).map_err(err)? else {
                            continue;
                        };
                        let entries = tree
                            .elements(texture_set, "Textures")
                            .map_err(err)?
                            .ok_or_else(access_violation)
                            .map_err(err)?;
                        if tree.count(entries) != 0 {
                            let first = tree.item(entries, 0).map_err(err)?;
                            let texture = tree.edit_value(first).map_err(err)?;
                            add_texture(textures, &texture);
                        }
                    }
                }
            } else if block_is_ni_object(tree, shape, "NiTriBasedGeom", true) {
                // skip if no shader (the Fallout 3 shader after Skyrim's)
                let shader = match block_property_by_type(tree, shape, "BSLightingShaderProperty", false)
                    .map_err(err)?
                {
                    Some(shader) => Some(shader),
                    None => block_property_by_type(tree, shape, "BSShaderPPLightingProperty", false).map_err(err)?,
                };
                let Some(shader) = shader else {
                    continue;
                };
                // skip if no textureset
                let set_ref = tree
                    .elements(shader, "Texture Set")
                    .map_err(err)?
                    .ok_or_else(access_violation)
                    .map_err(err)?;
                let Some(texture_set) = tree.links_to(set_ref).map_err(err)? else {
                    continue;
                };
                // skip if no Data
                let data_ref = tree
                    .elements(shape, "Data")
                    .map_err(err)?
                    .ok_or_else(access_violation)
                    .map_err(err)?;
                let Some(shape_data) = tree.links_to(data_ref).map_err(err)? else {
                    continue;
                };
                // skip if no UVs
                let Some(sets) = tree.elements(shape_data, "UV Sets").map_err(err)? else {
                    continue;
                };
                if tree.count(sets) == 0 {
                    continue;
                }
                let entries = tree.item(sets, 0).map_err(err)?;
                let mut is_tiled = tree.count(entries) == 0;
                if !untiled {
                    for j in 0..tree.count(entries) {
                        let entry = tree.item(entries, j).map_err(err)?;
                        let u = tree.native_values(entry, "U").map_err(err)?.to_f64().map_err(err)? as f32;
                        let v = tree.native_values(entry, "V").map_err(err)?.to_f64().map_err(err)? as f32;
                        is_tiled = tiled(u, v);
                        if is_tiled {
                            break;
                        }
                    }
                }
                if !is_tiled {
                    let entries = tree
                        .elements(texture_set, "Textures")
                        .map_err(err)?
                        .ok_or_else(access_violation)
                        .map_err(err)?;
                    if tree.count(entries) != 0 {
                        let first = tree.item(entries, 0).map_err(err)?;
                        let texture = tree.edit_value(first).map_err(err)?;
                        add_texture(textures, &texture);
                    }
                }
            }
        }
    }
    Ok(())
}

/// `LoadFromResource` (`dfResourceOpenData`): the data of the last
/// container that has the file.
pub fn open_resource_data(name: &str) -> Option<Vec<u8>> {
    let data = xedit_core::container_handler::open_resource_data("", name);
    (!data.is_empty()).then_some(data)
}

/// The diffuse texture of a material (`TwbBGSMFile.LoadFromResource` and
/// `EditValues['Textures\Diffuse']`).
pub fn load_material(name: &str) -> Result<String, crate::data_format::DfError> {
    let mut bgsm = MaterialFile::new_bgsm()?;
    let data = open_resource_data(name)
        .ok_or_else(|| crate::data_format::DfError::new(format!("Resource not found (File \"{name}\")")))?;
    bgsm.load_from_data(&data)?;
    bgsm.tree.edit_values(bgsm.root, "Textures\\Diffuse")
}

/// `TSourceAtlasTexture`.
#[derive(Debug, Clone, Default)]
pub struct SourceAtlasTexture {
    pub name: String,
    pub name_n: String,
    pub name_s: String,
    pub image: ImageData,
    pub image_n: ImageData,
    pub image_s: ImageData,
    pub atlas_name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Writes the mipmaps of an image to a DDS file.
fn save_dds(name: &str, image: &ImageData) -> LodResult<()> {
    let mipmaps = generate_mip_maps(image, 0, SamplingFilter::Lanczos)?;
    std::fs::write(name, save_multi_image_to_dds(&mipmaps)?)?;
    Ok(())
}

/// One atlas of `wbBuildAtlas`: the images of the blocks copied in, cropped
/// to the used size, the alpha prepared, converted and saved with mipmaps.
#[allow(clippy::too_many_arguments)]
fn write_atlas(
    blocks: &[BinBlock],
    images: &[SourceAtlasTexture],
    pick: fn(&SourceAtlasTexture) -> &ImageData,
    width: i32,
    height: i32,
    crop: (i32, i32),
    format: ImageFormat,
    threshold: i32,
    file_name: &str,
) -> LodResult<()> {
    let mut atlas = new_image(width, height, ImageFormat::Default)?;
    for block in blocks {
        copy_rect(
            pick(&images[block.index as usize]),
            0,
            0,
            block.w,
            block.h,
            &mut atlas,
            block.x,
            block.y,
        )?;
    }
    let (maxw, maxh) = crop;
    if maxw < width || maxh < height {
        let mut cropped = new_image(maxw, maxh, ImageFormat::Default)?;
        copy_rect(&atlas, 0, 0, maxw, maxh, &mut cropped, 0, 0)?;
        atlas = cropped;
    }
    prepare_image_alpha(&mut atlas, format, threshold);
    if !convert_image(&mut atlas, format)? {
        return Err(LodError::new("Image conversion error"));
    }
    save_dds(file_name, &atlas)
}

/// `wbBuildAtlas`: the textures packed into as many atlases as they need
/// (at least two textures each), each written as the diffuse, normal and,
/// for Fallout 4, specular atlas; every texture learns its atlas, place and
/// the atlas size.
#[allow(clippy::too_many_arguments)]
pub fn build_atlas(
    images: &mut [SourceAtlasTexture],
    width: i32,
    height: i32,
    name: &str,
    fmt_diffuse: ImageFormat,
    fmt_normal: ImageFormat,
    fmt_specular: ImageFormat,
    alpha_threshold: i32,
) -> LodResult<()> {
    if images.is_empty() {
        return Ok(());
    }
    let mut blocks: Vec<BinBlock> = images
        .iter()
        .enumerate()
        .map(|(i, image)| BinBlock {
            index: i as i32,
            w: image.image.width,
            h: image.image.height,
            ..BinBlock::default()
        })
        .collect();
    let mut blocks2: Vec<BinBlock> = Vec::new();
    let mut num = 0;
    let name = change_file_ext(name, "");
    let packer = BinPacker {
        width,
        height,
        ..BinPacker::default()
    };
    loop {
        // if not fit, remove images one by one from the end and try again;
        // at least 2 textures per atlas, useless otherwise
        while !packer.fit(&mut blocks) {
            if blocks.len() > 2 {
                blocks2.push(blocks.pop().unwrap());
            } else {
                return Ok(());
            }
        }
        let numbered = !blocks2.is_empty() || num != 0;
        let file_name = |suffix: &str| {
            if numbered {
                format!("{name}{num:02}{suffix}.dds")
            } else {
                format!("{name}{suffix}.dds")
            }
        };
        // the actual width and height of the atlas tiles
        let (mut maxw, mut maxh) = (0, 0);
        let diffuse_name = file_name("");
        for block in &blocks {
            let image = &mut images[block.index as usize];
            image.atlas_name = diffuse_name.clone();
            image.x = block.x;
            image.y = block.y;
            maxw = maxw.max(block.x + block.w);
            maxh = maxh.max(block.y + block.h);
        }
        // round to the larger power of 2 value
        let pow2 = |value: i32| {
            let mut i = 1;
            while i < value {
                i *= 2;
            }
            i
        };
        maxw = pow2(maxw);
        maxh = pow2(maxh);
        for block in &blocks {
            images[block.index as usize].w = maxw;
            images[block.index as usize].h = maxh;
        }
        write_atlas(
            &blocks,
            images,
            |image| &image.image,
            width,
            height,
            (maxw, maxh),
            fmt_diffuse,
            alpha_threshold,
            &diffuse_name,
        )?;
        write_atlas(
            &blocks,
            images,
            |image| &image.image_n,
            width,
            height,
            (maxw, maxh),
            fmt_normal,
            0,
            &file_name("_n"),
        )?;
        if is_fallout4() {
            write_atlas(
                &blocks,
                images,
                |image| &image.image_s,
                width,
                height,
                (maxw, maxh),
                fmt_specular,
                0,
                &file_name("_s"),
            )?;
        }
        // copy remaining blocks back
        blocks = std::mem::take(&mut blocks2);
        num += 1;
        if blocks.is_empty() {
            break;
        }
    }
    Ok(())
}

/// The image of a texture of the data, if it loads.
fn load_texture(env: &LodEnv, name: &str) -> Option<ImageData> {
    let data = open_resource(name)?;
    load_image_from_memory(env, &data)
}

/// `StringReplace(s, old, new, [rfIgnoreCase])`: the first match only.
fn replace_first_ignore_case(s: &str, old: &str, new: &str) -> String {
    match s.to_lowercase().find(&old.to_lowercase()) {
        Some(index) => format!("{}{new}{}", &s[..index], &s[index + old.len()..]),
        None => s.to_owned(),
    }
}

/// `wbBuildAtlasFromTexturesList`: the textures with their normal maps
/// (and specular maps for Fallout 4) loaded, too large ones skipped, the
/// rest resized to the tile size, the billboards brightened; the atlases
/// built and the map written (`<texture> <w> <h> <x> <y> <atlas> <atlas w>
/// <atlas h>`, tab separated).
#[allow(clippy::too_many_arguments)]
pub fn build_atlas_from_textures_list(
    env: &LodEnv,
    textures: &StringList,
    max_texture_size: i32,
    max_tile_size: i32,
    width: i32,
    height: i32,
    name: &str,
    map_name: &str,
) -> LodResult<()> {
    let section = env.section();
    let mut images: Vec<SourceAtlasTexture> = Vec::new();
    for (texture, flag) in &textures.items {
        let mut s = texture.clone();
        if !resource_exists(&s) && is_fallout3() {
            // default diffuse texture to use, only for fallouts since they can't use loose textures in LOD
            message(&format!("<Note: {s} diffuse texture not found, using replacement>"));
            s = "textures\\shared\\shadefade01.dds".to_owned();
        }
        let Some(data) = open_resource(&s) else {
            continue;
        };
        let Some(mut image) = load_image_from_memory(env, &data) else {
            continue;
        };
        // texture is too large
        if image.width > max_texture_size || image.height > max_texture_size {
            continue;
        }
        // resize tile if over the limit
        if image.width > max_tile_size || image.height > max_tile_size {
            let scl = (f64::from(max_tile_size) / f64::from(image.width))
                .min(f64::from(max_tile_size) / f64::from(image.height));
            let (w, h) = (
                crate::imaging::formats::round(f64::from(image.width) * scl) as i32,
                crate::imaging::formats::round(f64::from(image.height) * scl) as i32,
            );
            resize_image(&mut image, w, h, ResizeFilter::Lanczos)?;
        }
        let mut entry = SourceAtlasTexture {
            name: texture.clone(),
            image,
            ..SourceAtlasTexture::default()
        };
        // change brightness if it is a billboard
        if *flag == BILLBOARD_FLAG {
            let j = env.read_integer(&format!("{} LOD Options", app_name()), "TreesBrightness", 0);
            if j != 0 {
                convert_image(&mut entry.image, ImageFormat::A8R8G8B8)?;
                canvases::modify_contrast_brightness(&mut entry.image, (f64::from(j) / 10.0) as f32, j as f32)?;
            }
        }
        // load normal
        let mut s = texture.clone();
        if s.contains("_d.dds") {
            s = replace_first_ignore_case(&s, "_d.dds", "_n.dds");
            // fallback to simple _n addition if not found
            if !resource_exists(&s) {
                s = format!("{}_n.dds", change_file_ext(texture, ""));
            }
        } else {
            s = format!("{}_n.dds", change_file_ext(texture, ""));
        }
        if !resource_exists(&s) {
            message(&format!("<Note: {s} normal map not found, using flat replacement>"));
            s = default_normal_texture().to_owned();
        }
        match load_texture(env, &s) {
            Some(mut normal) => {
                // resize normals to match diffuse
                if entry.image.width != normal.width || entry.image.height != normal.height {
                    resize_image(
                        &mut normal,
                        entry.image.width,
                        entry.image.height,
                        ResizeFilter::Lanczos,
                    )?;
                }
                entry.image_n = normal;
            }
            None => continue,
        }
        entry.name_n = s;
        // load specular
        if is_fallout4() {
            let mut s = if texture.contains("_d.dds") {
                replace_first_ignore_case(texture, "_d.dds", "_s.dds")
            } else {
                format!("{}_s.dds", change_file_ext(texture, ""))
            };
            if !resource_exists(&s) {
                message(&format!("<Note: {s} specular map not found, using flat replacement>"));
                s = default_specular_texture().to_owned();
            }
            match load_texture(env, &s) {
                Some(mut specular) => {
                    if entry.image.width != specular.width || entry.image.height != specular.height {
                        resize_image(
                            &mut specular,
                            entry.image.width,
                            entry.image.height,
                            ResizeFilter::Lanczos,
                        )?;
                    }
                    entry.image_s = specular;
                }
                None => continue,
            }
            entry.name_s = s;
        }
        images.push(entry);
    }
    if images.is_empty() {
        return Ok(());
    }
    let format_of = |ident: &str, default: ImageFormat| {
        ImageFormat::from_ordinal(i64::from(env.read_integer(&section, ident, default as i32)))
    };
    let fmt_diffuse = format_of("AtlasDiffuseFormat", env.defaults.atlas_diffuse_format);
    let fmt_normal = format_of("AtlasNormalFormat", env.defaults.atlas_normal_format);
    let fmt_specular = format_of("AtlasSpecularFormat", env.defaults.atlas_specular_format);
    let alpha_threshold = env.read_integer(&section, "DefaultAlphaThreshold", env.defaults.alpha_threshold);
    build_atlas(
        &mut images,
        width,
        height,
        name,
        fmt_diffuse,
        fmt_normal,
        fmt_specular,
        alpha_threshold,
    )?;
    let mut map = StringList::new();
    for image in &images {
        if image.atlas_name.is_empty() {
            continue;
        }
        // atlas name in map file must be relative to data folder
        let atlas_name = match image.atlas_name.to_lowercase().find("textures\\") {
            Some(index) => image.atlas_name[index..].to_owned(),
            None => image.atlas_name.clone(),
        };
        map.add(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            image.name, image.image.width, image.image.height, image.x, image.y, atlas_name, image.w, image.h
        ));
    }
    if !map.is_empty() {
        map.save_to_file(map_name)?;
    }
    Ok(())
}

/// `wbBuildAtlasFromAtlasMap`: atlases built again from a map file, with
/// brightness and gamma.
pub fn build_atlas_from_atlas_map(
    env: &LodEnv,
    map: &StringList,
    brightness: i32,
    gamma: (f32, f32, f32),
) -> LodResult<()> {
    let section = env.section();
    let mut names: Vec<String> = Vec::new();
    let mut atlases: Vec<ImageData> = Vec::new();
    let mut atlases_n: Vec<ImageData> = Vec::new();
    let int = |text: &str| -> LodResult<i32> {
        crate::variant::str_to_int(text).ok_or_else(|| LodError::new(format!("'{text}' is not a valid integer value")))
    };
    for line in map.strings() {
        let sl = delimited_text(line, '\t');
        if sl.len() != 8 {
            continue;
        }
        let data = open_resource(&sl[0]).ok_or_else(|| LodError::new(format!("Source tile not found {}", sl[0])))?;
        let mut img =
            load_image_from_memory(env, &data).ok_or_else(|| LodError::new(format!("Error loading tile {}", sl[0])))?;
        let mut fname = format!("{}_n.dds", change_file_ext(&sl[0], ""));
        let data_n = match open_resource(&fname) {
            Some(data) => data,
            None => {
                message(&format!("<Note: {fname} normal map not found, using flat replacement>"));
                fname = default_normal_texture().to_owned();
                open_resource(&fname)
                    .ok_or_else(|| LodError::new(format!("Source tile normal map not found for {}", sl[0])))?
            }
        };
        let mut img_n = load_image_from_memory(env, &data_n)
            .ok_or_else(|| LodError::new(format!("Error loading tile normal map for {}", sl[0])))?;
        // resize diffuse as set in atlas map
        if img.width != int(&sl[1])? || img.height != int(&sl[2])? {
            resize_image(&mut img, int(&sl[1])?, int(&sl[2])?, ResizeFilter::Lanczos)?;
        }
        if img.width != img_n.width || img.height != img_n.height {
            resize_image(&mut img_n, img.width, img.height, ResizeFilter::Lanczos)?;
        }
        let i = match names
            .iter()
            .position(|name| xedit_io::encoding::ansi_compare_text(name, &sl[5]).is_eq())
        {
            Some(i) => i,
            None => {
                names.push(sl[5].clone());
                atlases.push(new_image(int(&sl[6])?, int(&sl[7])?, ImageFormat::Default)?);
                atlases_n.push(new_image(int(&sl[6])?, int(&sl[7])?, ImageFormat::Default)?);
                names.len() - 1
            }
        };
        let (w, h) = (img.width, img.height);
        copy_rect(&img, 0, 0, w, h, &mut atlases[i], int(&sl[3])?, int(&sl[4])?)?;
        let (w, h) = (img_n.width, img_n.height);
        copy_rect(&img_n, 0, 0, w, h, &mut atlases_n[i], int(&sl[3])?, int(&sl[4])?)?;
    }
    let format_of = |ident: &str, default: ImageFormat| {
        ImageFormat::from_ordinal(i64::from(env.read_integer(&section, ident, default as i32)))
    };
    let fmt_diffuse = format_of("AtlasDiffuseFormat", env.defaults.atlas_diffuse_format);
    let fmt_normal = format_of("AtlasNormalFormat", env.defaults.atlas_normal_format);
    for (i, name) in names.iter().enumerate() {
        if brightness != 0 {
            canvases::modify_contrast_brightness(
                &mut atlases[i],
                (f64::from(brightness) / 10.0) as f32,
                brightness as f32,
            )?;
        }
        let one = |v: f32| crate::nif_math::same_value(f64::from(v), 1.0);
        if !one(gamma.0) || !one(gamma.1) || !one(gamma.2) {
            canvases::gamma_correction(&mut atlases[i], gamma.0, gamma.1, gamma.2)?;
        }
        prepare_image_alpha(&mut atlases[i], fmt_diffuse, 0);
        if !convert_image(&mut atlases[i], fmt_diffuse)? {
            return Err(LodError::new("Image conversion error"));
        }
        let fname = if name.len() >= 9 && name[..9].eq_ignore_ascii_case("textures\\") {
            format!("{}{name}", env.output_path)
        } else {
            name.clone()
        };
        let folder = extract_file_path(&fname);
        if !std::path::Path::new(folder).is_dir() && !force_directories(folder) {
            return Err(LodError::new("Error creating atlas folder"));
        }
        save_dds(&fname, &atlases[i])?;
        prepare_image_alpha(&mut atlases_n[i], fmt_normal, 0);
        if !convert_image(&mut atlases_n[i], fmt_normal)? {
            return Err(LodError::new("Image conversion error"));
        }
        save_dds(&format!("{}_n.dds", change_file_ext(&fname, "")), &atlases_n[i])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packer_places_the_largest_first() {
        let mut blocks = vec![
            BinBlock {
                index: 0,
                w: 64,
                h: 64,
                ..BinBlock::default()
            },
            BinBlock {
                index: 1,
                w: 128,
                h: 32,
                ..BinBlock::default()
            },
            BinBlock {
                index: 2,
                w: 32,
                h: 32,
                ..BinBlock::default()
            },
        ];
        let packer = BinPacker {
            width: 128,
            height: 128,
            ..BinPacker::default()
        };
        assert!(packer.fit(&mut blocks));
        let places: Vec<(i32, i32, i32)> = blocks.iter().map(|b| (b.index, b.x, b.y)).collect();
        assert_eq!(places, vec![(1, 0, 0), (0, 0, 32), (2, 64, 32)]);
    }
}
