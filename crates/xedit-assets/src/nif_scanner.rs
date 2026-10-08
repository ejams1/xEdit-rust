// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbNifScanner.pas

//! The light NIF reader behind the script functions `NifBlockList`,
//! `NifTextureList` and `NifTextureListUVRange`: it reads the header of a
//! NIF from 20.0.0.0 on and the few blocks those functions look at, by
//! their offsets, without the data format definitions.

use crate::data_format::{DfError, R};

/// `TNifCheck`: the checks the warning callback reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NifCheck {
    Bsx,
    StringIndex,
    MultiName,
    Shader,
    VertexColor,
}

/// `TBinaryReader` over the data, with its stream position.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

fn read_error() -> DfError {
    DfError::new("Stream read error")
}

impl Reader<'_> {
    fn bytes(&mut self, count: usize) -> R<&[u8]> {
        let end = self.pos.checked_add(count).filter(|&end| end <= self.data.len());
        let Some(end) = end else {
            // `ReadBytes` returns what there is.
            let rest = &self.data[self.pos.min(self.data.len())..];
            self.pos = self.data.len();
            return Ok(rest);
        };
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn fixed<const N: usize>(&mut self) -> R<[u8; N]> {
        let bytes = self.data.get(self.pos..self.pos + N).ok_or_else(read_error)?;
        self.pos += N;
        Ok(bytes.try_into().expect("length checked"))
    }

    fn u8(&mut self) -> R<u8> {
        Ok(self.fixed::<1>()?[0])
    }

    fn u16(&mut self) -> R<u16> {
        Ok(u16::from_le_bytes(self.fixed()?))
    }

    fn u32(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.fixed()?))
    }

    fn i32(&mut self) -> R<i32> {
        Ok(i32::from_le_bytes(self.fixed()?))
    }

    fn f32(&mut self) -> R<f32> {
        Ok(f32::from_le_bytes(self.fixed()?))
    }

    /// `TStream.Read` of a record: what there is, without an error.
    fn skip(&mut self, count: usize) {
        self.pos = self.pos.saturating_add(count).min(self.data.len());
    }
}

/// `TEncoding.ASCII.GetString`: bytes above 127 become `?`.
fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| if byte < 0x80 { char::from(byte) } else { '?' })
        .collect()
}

/// `TNiHeader`.
#[derive(Debug, Default)]
struct Header {
    version: u32,
    user_version: u32,
    user_version2: u32,
    node_count: i32,
    node_types: Vec<String>,
    node_type_index: Vec<u16>,
    node_sizes: Vec<u32>,
    node_strings: Vec<String>,
    node_offsets: Vec<usize>,
}

/// The blocks the scanner reads, by type.
#[derive(Debug, Default)]
enum NodeData {
    #[default]
    Base,
    BsxFlags,
    TriShape {
        shape_data: i32,
        bs_properties: Vec<i32>,
    },
    TriShapeData {
        num_vertices: u16,
        uv_sets: Vec<Vec<(f32, f32)>>,
    },
    ShaderTextureSet {
        textures: Vec<String>,
    },
    LightingShaderProperty {
        texture_set: i32,
    },
    EffectShaderProperty {
        source_texture: String,
        grey_scale_texture: String,
    },
}

#[derive(Debug, Default)]
struct Node {
    index: i32,
    name: String,
    data: NodeData,
}

/// `TNiFile`.
struct NiFile {
    header: Header,
    nodes: Vec<Node>,
}

fn read_line_string(reader: &mut Reader) -> R<String> {
    let mut result = String::new();
    loop {
        let byte = reader.u8()?;
        if byte == 0x0A {
            return Ok(result);
        }
        result.push(char::from(byte));
    }
}

fn read_short_string(reader: &mut Reader) -> R<String> {
    let length = reader.u8()?;
    if length == 0 {
        return Ok(String::new());
    }
    let text = ascii(reader.bytes(usize::from(length) - 1)?);
    // The terminating null.
    reader.u8()?;
    Ok(text)
}

fn read_sized_string(reader: &mut Reader) -> R<String> {
    let length = reader.i32()?;
    if length > 4000 {
        return Err(DfError::new("Probably invalid Nif file, SizedString length > 4000"));
    }
    Ok(ascii(reader.bytes(length.max(0) as usize)?))
}

/// `ReadString`: an index into the header's strings from 20.1.0.3 on.
fn read_string(
    reader: &mut Reader,
    header: &Header,
    index: i32,
    warning: &mut dyn FnMut(NifCheck, i32, &str),
) -> R<String> {
    if header.version >= 0x1401_0003 {
        let string = reader.i32()?;
        if string >= 0 && (string as usize) < header.node_strings.len() {
            Ok(header.node_strings[string as usize].clone())
        } else {
            if string != -1 {
                warning(
                    NifCheck::StringIndex,
                    index,
                    &format!("String index {string} out of range in NiHeader strings table"),
                );
            }
            Ok(String::new())
        }
    } else {
        read_sized_string(reader)
    }
}

/// `TNiHeader.Load`.
fn read_header(reader: &mut Reader) -> R<Header> {
    const NIF_MAGIC: &str = "Gamebryo File Format";
    if ascii(reader.bytes(NIF_MAGIC.len())?) != NIF_MAGIC {
        return Err(DfError::new("Invalid nif file signature"));
    }
    // The rest of the magic line.
    read_line_string(reader)?;
    let mut header = Header {
        version: reader.u32()?,
        ..Header::default()
    };
    if header.version < 0x1400_0000 {
        return Err(DfError::new("Unsupported Nif file version"));
    }
    if header.version >= 0x1400_0004 {
        // Endianness.
        reader.u8()?;
    }
    header.user_version = reader.u32()?;
    header.node_count = reader.i32()?;
    if header.node_count > 5000 {
        return Err(DfError::new("Probably invalid Nif file, NifNumBlocks > 5000"));
    }
    header.user_version2 = reader.u32()?;
    // Export info: creator, export info 1 and 2.
    for _ in 0..3 {
        read_short_string(reader)?;
    }
    let type_count = reader.u16()?;
    if type_count > 1000 {
        return Err(DfError::new("Probably invalid Nif file, NifNumBlockTypes > 1000"));
    }
    for _ in 0..type_count {
        header.node_types.push(read_sized_string(reader)?);
    }
    let node_count = header.node_count.max(0) as usize;
    for _ in 0..node_count {
        header.node_type_index.push(reader.u16()?);
    }
    if header.version >= 0x1402_0007 {
        for _ in 0..node_count {
            header.node_sizes.push(reader.u32()?);
        }
    }
    // Oblivion meshes have no block sizes; their blocks are not read.
    if header.node_sizes.is_empty() {
        return Ok(header);
    }
    if header.version >= 0x1401_0003 {
        let count = reader.u32()?;
        // The maximum string length.
        reader.u32()?;
        for _ in 0..count {
            header.node_strings.push(read_sized_string(reader)?);
        }
    }
    // Unknown Int 2.
    reader.u32()?;
    let mut offset = reader.pos;
    for &size in &header.node_sizes {
        header.node_offsets.push(offset);
        offset += size as usize;
    }
    Ok(header)
}

/// `TNiFile.Load`.
fn load(data: &[u8], warning: &mut dyn FnMut(NifCheck, i32, &str)) -> R<NiFile> {
    let mut reader = Reader { data, pos: 0 };
    let header = read_header(&mut reader)?;
    let mut nodes = Vec::new();
    if header.node_sizes.is_empty() {
        return Ok(NiFile { header, nodes });
    }
    for index in 0..header.node_count.max(0) as usize {
        let node_type = header
            .node_type_index
            .get(index)
            .and_then(|&type_index| header.node_types.get(usize::from(type_index)))
            .ok_or_else(|| DfError::new(format!("List index ({index}) out of bounds")))?
            .clone();
        reader.pos = header.node_offsets[index];
        let mut node = Node {
            index: index as i32,
            ..Node::default()
        };
        let r = &mut reader;
        let i = index as i32;
        match node_type.as_str() {
            "NiNode" | "BSFadeNode" | "NiAlphaProperty" => node.name = read_string(r, &header, i, warning)?,
            "BSXFlags" => {
                node.name = read_string(r, &header, i, warning)?;
                r.u32()?;
                node.data = NodeData::BsxFlags;
            }
            "NiTriShape" | "BSLODTriShape" | "NiTriStrips" => {
                node.name = read_string(r, &header, i, warning)?;
                let count = r.u32()?;
                for _ in 0..count {
                    r.i32()?;
                }
                // Controller, flags.
                r.i32()?;
                r.u16()?;
                if header.version >= 0x1402_0007 && header.user_version >= 11 && header.user_version2 >= 26 {
                    r.u16()?;
                }
                // Translation, rotation.
                r.skip(12);
                r.skip(36);
                r.f32()?;
                if header.version < 0x1402_0007 || header.user_version <= 11 {
                    let count = r.u32()?;
                    for _ in 0..count {
                        r.i32()?;
                    }
                }
                // Collision object.
                r.i32()?;
                let shape_data = r.i32()?;
                // Skin instance.
                r.i32()?;
                let mut bs_properties = Vec::new();
                if header.version >= 0x1402_0007 {
                    let count = r.u32()?;
                    if count > 0 {
                        for _ in 0..count {
                            read_string(r, &header, i, warning)?;
                        }
                        for _ in 0..count {
                            r.i32()?;
                        }
                    }
                    // Active material, dirty flag.
                    r.i32()?;
                    r.u8()?;
                    for _ in 0..2 {
                        bs_properties.push(r.i32()?);
                    }
                }
                node.data = NodeData::TriShape {
                    shape_data,
                    bs_properties,
                };
            }
            "NiTriShapeData" => {
                r.i32()?;
                let num_vertices = r.u16()?;
                // Keep and compress flags.
                r.u8()?;
                r.u8()?;
                let vertex_size = 12 * usize::from(num_vertices);
                if r.u8()? != 0 {
                    r.skip(vertex_size);
                }
                let mut num_uv_sets = 0u16;
                if header.version >= 0x1402_0007 && header.user_version >= 11 {
                    num_uv_sets = r.u16()?;
                }
                if header.version >= 0x1402_0007 && header.user_version == 12 {
                    r.u32()?;
                }
                if r.u8()? != 0 {
                    r.skip(vertex_size);
                    if num_uv_sets & 61440 != 0 {
                        r.skip(vertex_size);
                        r.skip(vertex_size);
                    }
                }
                // Center and radius.
                r.skip(12);
                r.f32()?;
                if r.u8()? != 0 {
                    r.skip(16 * usize::from(num_vertices));
                }
                let mut uv_sets = Vec::new();
                if num_uv_sets & 1 != 0 {
                    let mut set = Vec::with_capacity(usize::from(num_vertices));
                    for _ in 0..num_vertices {
                        let u = r.fixed::<4>().map(f32::from_le_bytes).unwrap_or(0.0);
                        let v = r.fixed::<4>().map(f32::from_le_bytes).unwrap_or(0.0);
                        set.push((u, v));
                    }
                    uv_sets.push(set);
                }
                node.data = NodeData::TriShapeData { num_vertices, uv_sets };
            }
            "BSShaderTextureSet" => {
                let count = r.u32()?;
                let mut textures = Vec::new();
                for _ in 0..count {
                    textures.push(read_sized_string(r)?);
                }
                node.data = NodeData::ShaderTextureSet { textures };
            }
            "BSLightingShaderProperty" => {
                // Skyrim shader type.
                r.u32()?;
                node.name = read_string(r, &header, i, warning)?;
                let count = r.u32()?;
                for _ in 0..count {
                    r.i32()?;
                }
                // Controller.
                r.i32()?;
                if header.user_version == 12 {
                    r.i32()?;
                    r.i32()?;
                }
                // UV offset and scale.
                r.skip(8);
                r.skip(8);
                let texture_set = r.i32()?;
                node.data = NodeData::LightingShaderProperty { texture_set };
            }
            "BSEffectShaderProperty" => {
                node.name = read_string(r, &header, i, warning)?;
                let count = r.u32()?;
                r.bytes(4 * count as usize)?;
                // Controller, shader flags 1 and 2.
                r.i32()?;
                r.i32()?;
                r.i32()?;
                // UV offset and scale.
                r.bytes(8)?;
                r.bytes(8)?;
                let source_texture = read_sized_string(r)?;
                // Texture clamp mode, falloff and emissive data.
                r.i32()?;
                r.bytes(40)?;
                let grey_scale_texture = read_sized_string(r)?;
                node.data = NodeData::EffectShaderProperty {
                    source_texture,
                    grey_scale_texture,
                };
            }
            _ => {}
        }
        nodes.push(node);
    }
    Ok(NiFile { header, nodes })
}

impl NiFile {
    /// `GetNode`.
    fn node(&self, reference: i32) -> Option<&Node> {
        self.nodes.iter().find(|node| node.index == reference)
    }
}

/// `StringsRemoveEmptyAndTrim`: Delphi's `Trim` drops the characters up to
/// the space at both ends.
fn remove_empty_and_trim(list: &mut Vec<(String, i32)>) {
    for entry in list.iter_mut() {
        entry.0 = entry.0.trim_matches(|ch: char| ch <= ' ').to_owned();
    }
    list.retain(|(text, _)| !text.is_empty());
}

/// `NifBlockList`: every block as `Name=Type` with its index.
pub fn nif_block_list(data: &[u8]) -> R<Vec<(String, i32)>> {
    let nif = load(data, &mut |_, _, _| {})?;
    let mut result = Vec::new();
    for (index, node) in nif.nodes.iter().enumerate() {
        let type_index = usize::from(nif.header.node_type_index[index]);
        result.push((
            format!("{}={}", node.name, nif.header.node_types[type_index]),
            node.index,
        ));
    }
    remove_empty_and_trim(&mut result);
    Ok(result)
}

/// `NifTextures`: the texture file names of the texture sets (with their
/// slots) and the effect shaders. `None` for no data.
pub fn nif_textures(data: &[u8]) -> R<Option<Vec<(String, i32)>>> {
    if data.is_empty() {
        return Ok(None);
    }
    let nif = load(data, &mut |_, _, _| {})?;
    let mut result = Vec::new();
    for node in &nif.nodes {
        match &node.data {
            NodeData::ShaderTextureSet { textures } => {
                for (slot, texture) in textures.iter().enumerate() {
                    result.push((texture.clone(), slot as i32));
                }
            }
            NodeData::EffectShaderProperty {
                source_texture,
                grey_scale_texture,
            } => {
                result.push((source_texture.clone(), 0));
                result.push((grey_scale_texture.clone(), 0));
            }
            _ => {}
        }
    }
    remove_empty_and_trim(&mut result);
    Ok(Some(result))
}

/// `NifTexturesUVRange`: the textures of the shapes whose texture
/// coordinates stay within `uv_range`. `None` for no data.
pub fn nif_textures_uv_range(data: &[u8], uv_range: f32) -> R<Option<Vec<(String, i32)>>> {
    if data.is_empty() {
        return Ok(None);
    }
    let nif = load(data, &mut |_, _, _| {})?;
    let mut result = Vec::new();
    for node in &nif.nodes {
        let NodeData::TriShape {
            shape_data,
            bs_properties,
        } = &node.data
        else {
            continue;
        };
        // UPSTREAM-QUIRK: upstream casts any block to the shape data; a
        // block of another type has no vertices here.
        let Some(NodeData::TriShapeData { num_vertices, uv_sets }) = nif.node(*shape_data).map(|node| &node.data)
        else {
            continue;
        };
        if bs_properties.is_empty() || *num_vertices == 0 || uv_sets.is_empty() {
            continue;
        }
        // Tiled textures are skipped.
        let tiled = uv_sets.iter().any(|set| {
            set.iter()
                .any(|&(u, v)| u < -uv_range || u > uv_range || v < -uv_range || v > uv_range)
        });
        if tiled {
            continue;
        }
        let shader = bs_properties.iter().find_map(|&reference| match nif.node(reference) {
            Some(Node {
                data: NodeData::LightingShaderProperty { texture_set },
                ..
            }) => Some(*texture_set),
            _ => None,
        });
        let Some(texture_set) = shader else { continue };
        if let Some(Node {
            data: NodeData::ShaderTextureSet { textures },
            ..
        }) = nif.node(texture_set)
        {
            for (slot, texture) in textures.iter().enumerate() {
                result.push((texture.clone(), slot as i32));
            }
        }
    }
    remove_empty_and_trim(&mut result);
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_nif_data() {
        assert_eq!(
            nif_textures(b"not a nif file at all").unwrap_err().0,
            "Invalid nif file signature"
        );
        assert!(nif_textures(b"").unwrap().is_none());
    }
}
