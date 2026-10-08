// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDataFormatMisc.pas

//! The small formats on the data format framework: the LOD settings files
//! (`*.lod`, `*.dlodsettings`), the tree LOD index and reference files
//! (`*.lst`, `*.btt`, `*.dtl`), FUZ voice files and the DDS header.

use std::sync::OnceLock;

use crate::data_format::{
    Class, DataType, Def, DfError, El, Event, R, StructClass, Tree, df_array, df_bytes, df_chars, df_enum, df_flags,
    df_float, df_hex_integer, df_integer, df_struct,
};
use crate::variant::Variant;

/// `FUZ_GetLIPSize`.
fn fuz_get_lip_size(t: &mut Tree, e: El, count: &mut i32) -> R<()> {
    *count = t.native_values(e, "..\\LIP Size")?.to_i32()?;
    Ok(())
}

/// `FUZ_BeforeSaveLIPSize`.
fn fuz_before_save_lip_size(t: &mut Tree, e: El) -> R<()> {
    let size = match t.elements(e, "..\\LIP Data")? {
        Some(data) => t.data_size(data)?,
        None => return Err(DfError::new("Access violation: no LIP Data")),
    };
    t.set_native_value(e, Variant::Int(i64::from(size)))
}

/// `DDS_EnDX10`.
fn dds_en_dx10(t: &mut Tree, e: El) -> R<bool> {
    let four_cc = t.edit_values(e, "..\\HEADER\\ddspf\\dwFourCC")?;
    Ok(four_cc == "DX10" || four_cc == "XBOX")
}

/// `DDS_EnXBOX`.
fn dds_en_xbox(t: &mut Tree, e: El) -> R<bool> {
    Ok(t.edit_values(e, "..\\HEADER\\ddspf\\dwFourCC")? == "XBOX")
}

/// `GetTextFourCC`.
fn get_text_four_cc(_t: &mut Tree, _e: El, text: &mut String) -> R<()> {
    if text == "\0\0\0\0" {
        text.clear();
    }
    Ok(())
}

/// `SetTextFourCC`.
fn set_text_four_cc(_t: &mut Tree, _e: El, text: &mut String) -> R<()> {
    if text.is_empty() {
        *text = "\0\0\0\0".to_owned();
    }
    Ok(())
}

/// The definitions of `wbDefineMisc`.
pub struct MiscDefs {
    pub lod_settings_tes5: Def,
    pub lod_settings_fo3: Def,
    pub lod_tree_lst: Def,
    pub lod_tree_btt: Def,
    pub fuz: Def,
    pub dds: Def,
}

const DXGI_FORMATS: &[(i64, &str)] = &[
    (0, "UNKNOWN"),
    (1, "R32G32B32A32_TYPELESS"),
    (2, "R32G32B32A32_FLOAT"),
    (3, "R32G32B32A32_UINT"),
    (4, "R32G32B32A32_SINT"),
    (5, "R32G32B32_TYPELESS"),
    (6, "R32G32B32_FLOAT"),
    (7, "R32G32B32_UINT"),
    (8, "R32G32B32_SINT"),
    (9, "R16G16B16A16_TYPELESS"),
    (10, "R16G16B16A16_FLOAT"),
    (11, "R16G16B16A16_UNORM"),
    (12, "R16G16B16A16_UINT"),
    (13, "R16G16B16A16_SNORM"),
    (14, "R16G16B16A16_SINT"),
    (15, "R32G32_TYPELESS"),
    (16, "R32G32_FLOAT"),
    (17, "R32G32_UINT"),
    (18, "R32G32_SINT"),
    (19, "R32G8X24_TYPELESS"),
    (20, "D32_FLOAT_S8X24_UINT"),
    (21, "R32_FLOAT_X8X24_TYPELESS"),
    (22, "X32_TYPELESS_G8X24_UINT"),
    (23, "R10G10B10A2_TYPELESS"),
    (24, "R10G10B10A2_UNORM"),
    (25, "R10G10B10A2_UINT"),
    (26, "R11G11B10_FLOAT"),
    (27, "R8G8B8A8_TYPELESS"),
    (28, "R8G8B8A8_UNORM"),
    (29, "R8G8B8A8_UNORM_SRGB"),
    (30, "R8G8B8A8_UINT"),
    (31, "R8G8B8A8_SNORM"),
    (32, "R8G8B8A8_SINT"),
    (33, "R16G16_TYPELESS"),
    (34, "R16G16_FLOAT"),
    (35, "R16G16_UNORM"),
    (36, "R16G16_UINT"),
    (37, "R16G16_SNORM"),
    (38, "R16G16_SINT"),
    (39, "R32_TYPELESS"),
    (40, "D32_FLOAT"),
    (41, "R32_FLOAT"),
    (42, "R32_UINT"),
    (43, "R32_SINT"),
    (44, "R24G8_TYPELESS"),
    (45, "D24_UNORM_S8_UINT"),
    (46, "R24_UNORM_X8_TYPELESS"),
    (47, "X24_TYPELESS_G8_UINT"),
    (48, "R8G8_TYPELESS"),
    (49, "R8G8_UNORM"),
    (50, "R8G8_UINT"),
    (51, "R8G8_SNORM"),
    (52, "R8G8_SINT"),
    (53, "R16_TYPELESS"),
    (54, "R16_FLOAT"),
    (55, "D16_UNORM"),
    (56, "R16_UNORM"),
    (57, "R16_UINT"),
    (58, "R16_SNORM"),
    (59, "R16_SINT"),
    (60, "R8_TYPELESS"),
    (61, "R8_UNORM"),
    (62, "R8_UINT"),
    (63, "R8_SNORM"),
    (64, "R8_SINT"),
    (65, "A8_UNORM"),
    (66, "R1_UNORM"),
    (67, "R9G9B9E5_SHAREDEXP"),
    (68, "R8G8_B8G8_UNORM"),
    (69, "G8R8_G8B8_UNORM"),
    (70, "BC1_TYPELESS"),
    (71, "BC1_UNORM"),
    (72, "BC1_UNORM_SRGB"),
    (73, "BC2_TYPELESS"),
    (74, "BC2_UNORM"),
    (75, "BC2_UNORM_SRGB"),
    (76, "BC3_TYPELESS"),
    (77, "BC3_UNORM"),
    (78, "BC3_UNORM_SRGB"),
    (79, "BC4_TYPELESS"),
    (80, "BC4_UNORM"),
    (81, "BC4_SNORM"),
    (82, "BC5_TYPELESS"),
    (83, "BC5_UNORM"),
    (84, "BC5_SNORM"),
    (85, "B5G6R5_UNORM"),
    (86, "B5G5R5A1_UNORM"),
    (87, "B8G8R8A8_UNORM"),
    (88, "B8G8R8X8_UNORM"),
    (89, "R10G10B10_XR_BIAS_A2_UNORM"),
    (90, "B8G8R8A8_TYPELESS"),
    (91, "B8G8R8A8_UNORM_SRGB"),
    (92, "B8G8R8X8_TYPELESS"),
    (93, "B8G8R8X8_UNORM_SRGB"),
    (94, "BC6H_TYPELESS"),
    (95, "BC6H_UF16"),
    (96, "BC6H_SF16"),
    (97, "BC7_TYPELESS"),
    (98, "BC7_UNORM"),
    (99, "BC7_UNORM_SRGB"),
    (100, "AYUV"),
    (101, "Y410"),
    (102, "Y416"),
    (103, "NV12"),
    (104, "P010"),
    (105, "P016"),
    (106, "420_OPAQUE"),
    (107, "YUY2"),
    (108, "Y210"),
    (109, "Y216"),
    (110, "NV11"),
    (111, "AI44"),
    (112, "IA44"),
    (113, "P8"),
    (114, "A8P8"),
    (115, "B4G4R4A4_UNORM"),
    (130, "P208"),
    (131, "V208"),
    (132, "V408"),
    (-1, "FORCE_UINT"),
];

/// `wbDefineMisc`.
fn define_misc() -> MiscDefs {
    let lod_settings_tes5 = df_struct(
        "LOD",
        vec![
            df_integer("Min X", DataType::S16, "", &[]),
            df_integer("Min Y", DataType::S16, "", &[]),
            df_integer("Stride", DataType::U16, "", &[]),
            df_integer("Min Level", DataType::U16, "", &[]),
            df_integer("Max Level", DataType::U16, "", &[]),
        ],
        &[],
    );
    let lod_settings_fo3 = df_struct(
        "LOD",
        vec![
            df_integer("Min Terrain Level", DataType::U32, "", &[]),
            df_integer("Max Terrain Level", DataType::U32, "", &[]),
            df_integer("Stride", DataType::U32, "", &[]),
            df_integer("Min X", DataType::S16, "", &[]),
            df_integer("Min Y", DataType::S16, "", &[]),
            df_integer("Max X", DataType::S16, "", &[]),
            df_integer("Max Y", DataType::S16, "", &[]),
            df_integer("Object Level", DataType::U32, "", &[]),
        ],
        &[],
    );
    let float = |name: &str| df_float(name, DataType::Float32, "", &[]);
    let lod_tree_lst = df_array(
        "Trees",
        df_struct(
            "Tree",
            vec![
                df_integer("Type", DataType::U32, "", &[]),
                float("Width"),
                float("Height"),
                df_struct(
                    "Atlas Position",
                    vec![
                        df_struct("Min", vec![float("U"), float("V")], &[]),
                        df_struct("Max", vec![float("U"), float("V")], &[]),
                    ],
                    &[],
                ),
                df_integer("Unknown", DataType::U32, "", &[]),
            ],
            &[],
        ),
        -4,
        "",
        &[],
    );
    let lod_tree_btt = df_array(
        "Trees",
        df_struct(
            "Tree",
            vec![
                df_integer("Type", DataType::U32, "", &[]),
                df_array(
                    "References",
                    df_struct(
                        "Reference",
                        vec![
                            float("X"),
                            float("Y"),
                            float("Z"),
                            float("Rotation"),
                            df_float("Scale", DataType::Float32, "1.0", &[]),
                            df_hex_integer("FormID", DataType::U32),
                            df_integer("Unknown 1", DataType::U32, "", &[]),
                            df_integer("Unknown 2", DataType::U32, "", &[]),
                        ],
                        &[],
                    ),
                    -4,
                    "",
                    &[],
                ),
            ],
            &[],
        ),
        -4,
        "",
        &[],
    );
    // The LIP size is kept apart from the data so that the data loads and
    // saves on its own.
    let fuz = df_struct(
        "FUZ",
        vec![
            df_chars("Magic", 4, "FUZE", 0, false, &[]),
            df_integer("Version", DataType::U32, "1", &[]),
            df_integer(
                "LIP Size",
                DataType::U32,
                "",
                &[Event::BeforeSave(fuz_before_save_lip_size)],
            ),
            df_bytes("LIP Data", 0, &[Event::GetCount(fuz_get_lip_size)]),
            df_bytes("XWM Data", 0, &[]),
        ],
        &[],
    );
    let u32_field = |name: &str| df_integer(name, DataType::U32, "", &[]);
    let dds = df_struct(
        "DDS",
        vec![
            df_chars("Magic", 4, "DDS", 0, false, &[]),
            df_struct(
                "HEADER",
                vec![
                    u32_field("dwSize"),
                    df_flags(
                        "dwFlags",
                        DataType::U32,
                        &[
                            (0, "DDSD_CAPS"),
                            (1, "DDSD_HEIGHT"),
                            (2, "DDSD_WIDTH"),
                            (3, "DDSD_PITCH"),
                            (12, "DDSD_PIXELFORMAT"),
                            (17, "DDSD_MIPMAPCOUNT"),
                            (19, "DDSD_LINEARSIZE"),
                            (23, "DDSD_DEPTH"),
                        ],
                        "",
                        &[],
                    ),
                    u32_field("dwHeight"),
                    u32_field("dwWidth"),
                    u32_field("dwPitchOrLinearSize"),
                    u32_field("dwDepth"),
                    u32_field("dwMipMapCount"),
                    df_bytes("dwReserved1", 11 * 4, &[]),
                    df_struct(
                        "ddspf",
                        vec![
                            u32_field("dwSize"),
                            df_flags(
                                "dwFlags",
                                DataType::U32,
                                &[
                                    (0, "DDPF_ALPHAPIXELS"),
                                    (1, "DDPF_ALPHA"),
                                    (2, "DDPF_FOURCC"),
                                    (6, "DDPF_RGB"),
                                    (9, "DDPF_YUV"),
                                    (17, "DDPF_LUMINANCE"),
                                ],
                                "",
                                &[],
                            ),
                            df_chars("dwFourCC", 4, "", 0, false, &[])
                                .set_on_get_text(get_text_four_cc)
                                .set_on_set_text(set_text_four_cc),
                            u32_field("dwRGBBitCount"),
                            df_hex_integer("dwRBitMask", DataType::U32),
                            df_hex_integer("dwGBitMask", DataType::U32),
                            df_hex_integer("dwBBitMask", DataType::U32),
                            df_hex_integer("dwABitMask", DataType::U32),
                        ],
                        &[],
                    ),
                    df_flags(
                        "dwCaps",
                        DataType::U32,
                        &[(3, "DDSCAPS_COMPLEX"), (12, "DDSCAPS_TEXTURE"), (22, "DDSCAPS_MIPMAP")],
                        "",
                        &[],
                    ),
                    df_flags(
                        "dwCaps2",
                        DataType::U32,
                        &[
                            (9, "DDSCAPS2_CUBEMAP"),
                            (10, "DDSCAPS2_CUBEMAP_POSITIVEX"),
                            (11, "DDSCAPS2_CUBEMAP_NEGATIVEX"),
                            (12, "DDSCAPS2_CUBEMAP_POSITIVEY"),
                            (13, "DDSCAPS2_CUBEMAP_NEGATIVEY"),
                            (14, "DDSCAPS2_CUBEMAP_POSITIVEZ"),
                            (15, "DDSCAPS2_CUBEMAP_NEGATIVEZ"),
                            (21, "DDSCAPS2_VOLUME"),
                        ],
                        "",
                        &[],
                    ),
                    u32_field("dwCaps3"),
                    u32_field("dwCaps4"),
                    u32_field("dwReserved2"),
                ],
                &[],
            ),
            df_struct(
                "HEADER_DXT10",
                vec![
                    df_enum("dxgiFormat", DataType::S32, DXGI_FORMATS, "", &[]),
                    df_enum(
                        "resourceDimension",
                        DataType::U32,
                        &[
                            (2, "DDS_DIMENSION_TEXTURE1D"),
                            (3, "DDS_DIMENSION_TEXTURE2D"),
                            (4, "DDS_DIMENSION_TEXTURE3D"),
                        ],
                        "",
                        &[],
                    ),
                    df_flags(
                        "miscFlags",
                        DataType::U32,
                        &[(2, "DDS_RESOURCE_MISC_TEXTURECUBE")],
                        "",
                        &[],
                    ),
                    u32_field("arraySize"),
                    df_enum(
                        "miscFlags2",
                        DataType::U32,
                        &[
                            (0, "DDS_ALPHA_MODE_UNKNOWN"),
                            (1, "DDS_ALPHA_MODE_STRAIGHT"),
                            (2, "DDS_ALPHA_MODE_PREMULTIPLIED"),
                            (3, "DDS_ALPHA_MODE_OPAQUE"),
                            (4, "DDS_ALPHA_MODE_CUSTOM"),
                        ],
                        "",
                        &[],
                    ),
                ],
                &[],
            )
            .set_on_enabled(dds_en_dx10),
            df_struct(
                "HEADER_XBOX",
                vec![
                    u32_field("tileMode"),
                    u32_field("baseAlignment"),
                    u32_field("dataSize"),
                    u32_field("xdkVer"),
                ],
                &[],
            )
            .set_on_enabled(dds_en_xbox),
        ],
        &[],
    );
    MiscDefs {
        lod_settings_tes5,
        lod_settings_fo3,
        lod_tree_lst,
        lod_tree_btt,
        fuz,
        dds,
    }
}

/// The definitions, defined on first use.
pub fn misc_defs() -> &'static MiscDefs {
    static DEFS: OnceLock<MiscDefs> = OnceLock::new();
    DEFS.get_or_init(define_misc)
}

/// The kinds of files of this unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiscFile {
    /// `TwbLODSettingsTES5File`: `*.lod` of Skyrim and Fallout 4.
    LodSettingsTes5,
    /// `TwbLODSettingsFO3File`: `*.dlodsettings` of Fallout 3 and New Vegas.
    LodSettingsFo3,
    /// `TwbLODTreeLSTFile`.
    LodTreeLst,
    /// `TwbLODTreeBTTFile`.
    LodTreeBtt,
    /// `TwbFUZFile`.
    Fuz,
    /// `TwbDDSFile`.
    Dds,
}

/// A new, empty file of the kind: its tree and root.
pub fn create_misc_file(kind: MiscFile) -> R<(Tree, El)> {
    let defs = misc_defs();
    let (def, class) = match kind {
        MiscFile::LodSettingsTes5 => (&defs.lod_settings_tes5, Class::Struct(StructClass::Plain)),
        MiscFile::LodSettingsFo3 => (&defs.lod_settings_fo3, Class::Struct(StructClass::Plain)),
        MiscFile::LodTreeLst => (&defs.lod_tree_lst, Class::Array),
        MiscFile::LodTreeBtt => (&defs.lod_tree_btt, Class::Array),
        MiscFile::Fuz => (&defs.fuz, Class::Struct(StructClass::Fuz)),
        MiscFile::Dds => (&defs.dds, Class::Struct(StructClass::Plain)),
    };
    let mut tree = Tree::new();
    let root = tree.create_root(def, class)?;
    Ok((tree, root))
}

/// `TwbFUZFile.UnSerialize`: checks the magic first.
pub(crate) fn fuz_unserialize(tree: &mut Tree, el: El, data: Option<&[u8]>, data_size: i32) -> R<i32> {
    if let Some(data) = data
        && !(data.len() > 4 && &data[..4] == b"FUZE")
    {
        return Err(DfError::new("Not a FUZ file"));
    }
    tree.struct_unserialize(el, data, data_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuz_round_trip() {
        let mut data = b"FUZE".to_vec();
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(b"lipxwmdata");
        let (mut tree, root) = create_misc_file(MiscFile::Fuz).unwrap();
        tree.load_from_data(root, &data).unwrap();
        assert_eq!(tree.edit_values(root, "LIP Data").unwrap(), "6C 69 70");
        assert_eq!(tree.save_to_data(root).unwrap(), data);
    }

    #[test]
    fn dds_header_reads() {
        let (mut tree, root) = create_misc_file(MiscFile::Dds).unwrap();
        tree.set_to_default(root).unwrap();
        assert_eq!(tree.edit_values(root, "Magic").unwrap(), "DDS\0");
        assert!(tree.elements(root, "HEADER_DXT10").unwrap().is_none());
    }
}
