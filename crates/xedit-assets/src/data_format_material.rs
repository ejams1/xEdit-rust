// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDataFormatMaterial.pas

//! Fallout 4 material files: shader materials (`*.bgsm`) and effect
//! materials (`*.bgem`), binary or in the JSON form of the material editor.

use std::sync::OnceLock;

use crate::data_format::{
    Class, DataType, Def, DfError, El, Event, R, StructClass, Tree, df_bool, df_chars, df_enum, df_flags, df_float,
    df_integer, df_struct,
};
use crate::data_format_nif_types::{wb_color3, wb_color3_name};
use crate::json::Json;
use crate::variant::Variant;

const BASE_MAP: &[(&str, &str)] = &[
    ("fUOffset", "UOffset"),
    ("fVOffset", "VOffset"),
    ("fUScale", "UScale"),
    ("fVScale", "VScale"),
    ("fAlpha", "Alpha"),
    ("eAlphaBlendMode", "AlphaBlendMode"),
    ("fAlphaTestRef", "AlphaTestRef"),
    ("bAlphaTest", "AlphaTest"),
    ("bZBufferWrite", "ZBufferWrite"),
    ("bZBufferTest", "ZBufferTest"),
    ("bScreenSpaceReflections", "ScreenSpaceReflections"),
    (
        "bWetnessControl_ScreenSpaceReflections",
        "WetnessControlScreenSpaceReflections",
    ),
    ("bDecal", "Decal"),
    ("bTwoSided", "TwoSided"),
    ("bDecalNoFade", "DecalNoFade"),
    ("bNonOccluder", "NonOccluder"),
    ("bRefraction", "Refraction"),
    ("bRefractionFalloff", "RefractionFalloff"),
    ("fRefractionPower", "RefractionPower"),
    ("bEnvironmentMapping", "EnvironmentMapping"),
    ("fEnvironmentMappingMaskScale", "EnvironmentMappingMaskScale"),
    ("bGrayscaleToPaletteColor", "GrayscaleToPaletteColor"),
];

const SHADER_MAP: &[(&str, &str)] = &[
    ("sDiffuseTexture", "Textures\\Diffuse"),
    ("sNormalTexture", "Textures\\Normal"),
    ("sSmoothSpecTexture", "Textures\\SmoothSpec"),
    // Yes, "grEyscale".
    ("sGreyscaleTexture", "Textures\\Grayscale"),
    ("sEnvmapTexture", "Textures\\Envmap"),
    ("sGlowTexture", "Textures\\Glow"),
    ("sInnerLayerTexture", "Textures\\InnerLayer"),
    ("sWrinklesTexture", "Textures\\Wrinkles"),
    ("sDisplacementTexture", "Textures\\Displacement"),
    ("bEnableEditorAlphaRef", "EnableEditorAlphaRef"),
    ("bRimLighting", "RimLighting"),
    ("fRimPower", "RimPower"),
    ("fBackLightPower", "BackLightPower"),
    ("bSubsurfaceLighting", "SubsurfaceLighting"),
    ("fSubsurfaceLightingRolloff", "SubsurfaceLightingRolloff"),
    ("bSpecularEnabled", "SpecularEnabled"),
    ("cSpecularColor", "SpecularColor"),
    ("fSpecularMult", "SpecularMult"),
    ("fSmoothness", "Smoothness"),
    ("fFresnelPower", "FresnelPower"),
    ("fWetnessControl_SpecScale", "WetnessControlSpecScale"),
    ("fWetnessControl_SpecPowerScale", "WetnessControlSpecPowerScale"),
    ("fWetnessControl_SpecMinvar", "WetnessControlSpecMinvar"),
    ("fWetnessControl_EnvMapScale", "WetnessControlEnvMapScale"),
    ("fWetnessControl_FresnelPower", "WetnessControlFresnelPower"),
    ("fWetnessControl_Metalness", "WetnessControlMetalness"),
    ("sRootMaterialPath", "RootMaterialPath"),
    ("bAnisoLighting", "AnisoLighting"),
    ("bEmitEnabled", "EmitEnabled"),
    ("cEmittanceColor", "EmittanceColor"),
    ("fEmittanceMult", "EmittanceMult"),
    ("bModelSpaceNormals", "ModelSpaceNormals"),
    ("bExternalEmittance", "ExternalEmittance"),
    ("bBackLighting", "BackLighting"),
    ("bReceiveShadows", "ReceiveShadows"),
    ("bHideSecret", "HideSecret"),
    ("bCastShadows", "CastShadows"),
    ("bDissolveFade", "DissolveFade"),
    ("bAssumeShadowmask", "AssumeShadowmask"),
    ("bGlowmap", "Glowmap"),
    ("bEnvironmentMappingWindow", "EnvironmentMappingWindow"),
    ("bEnvironmentMappingEye", "EnvironmentMappingEye"),
    ("bHair", "Hair"),
    ("cHairTintColor", "HairTintColor"),
    ("bTree", "Tree"),
    ("bFacegen", "Facegen"),
    ("bSkinTint", "SkinTint"),
    ("bTessellate", "Tessellate"),
    ("fDisplacementTextureBias", "DisplacementTextureBias"),
    ("fDisplacementTextureScale", "DisplacementTextureScale"),
    ("fTessellationPnScale", "TessellationPnScale"),
    ("fTessellationBaseFactor", "TessellationBaseFactor"),
    ("fTessellationFadeDistance", "TessellationFadeDistance"),
    ("fGrayscaleToPaletteScale", "GrayscaleToPaletteScale"),
    ("bSkewSpecularAlpha", "SkewSpecularAlpha"),
];

const EFFECT_MAP: &[(&str, &str)] = &[
    ("sBaseTexture", "Textures\\Base"),
    ("sGrayscaleTexture", "Textures\\Grayscale"),
    ("sEnvmapTexture", "Textures\\Envmap"),
    ("sNormalTexture", "Textures\\Normal"),
    ("sEnvmapMaskTexture", "Textures\\EnvmapMask"),
    ("bBloodEnabled", "BloodEnabled"),
    ("bEffectLightingEnabled", "EffectLightingEnabled"),
    ("bFalloffEnabled", "FalloffEnabled"),
    ("bFalloffColorEnabled", "FalloffColorEnabled"),
    ("bGrayscaleToPaletteAlpha", "GrayscaleToPaletteAlpha"),
    ("bSoftEnabled", "SoftEnabled"),
    ("cBaseColor", "BaseColor"),
    ("fBaseColorScale", "BaseColorScale"),
    ("fFalloffStartAngle", "FalloffStartAngle"),
    ("fFalloffStopAngle", "FalloffStopAngle"),
    ("fFalloffStartOpacity", "FalloffStartOpacity"),
    ("fFalloffStopOpacity", "FalloffStopOpacity"),
    ("fLightingInfluence", "LightingInfluence"),
    ("iEnvmapMinLOD", "EnvmapMinLOD"),
    ("fSoftDepth", "SoftDepth"),
];

/// `LenStringZ_SetText`: file names use `/` and do not start with
/// `materials/`, as the vanilla materials do.
fn len_string_z_set_text(_t: &mut Tree, _e: El, text: &mut String) -> R<()> {
    *text = text.replace('\\', "/");
    if text.len() >= 10 && text.is_char_boundary(10) && text[..10].eq_ignore_ascii_case("materials/") {
        *text = text[10..].to_owned();
    }
    Ok(())
}

/// `wbLenStringZ`.
fn wb_len_string_z(name: &str) -> Def {
    df_chars(name, -4, "", 0, true, &[]).set_on_set_text(len_string_z_set_text)
}

/// `BGSMGetAlphaBlendValue`: the blend mode from its three values.
fn bgsm_get_alpha_blend_value(t: &mut Tree, e: El, value: &mut Variant) -> R<()> {
    let a = t.native_values(e, "..\\AlphaBlendMode0")?.to_i32()?;
    let b = t.native_values(e, "..\\AlphaBlendMode1")?.to_i32()?;
    let c = t.native_values(e, "..\\AlphaBlendMode2")?.to_i32()?;
    match (a, b, c) {
        (0, 6, 7) => *value = Variant::Int(0),
        (0, 0, 0) => *value = Variant::Int(1),
        (1, 6, 7) => *value = Variant::Int(2),
        (1, 6, 0) => *value = Variant::Int(3),
        (1, 4, 1) => *value = Variant::Int(4),
        _ => {}
    }
    Ok(())
}

/// `BGSMSetAlphaBlendValue`.
fn bgsm_set_alpha_blend_value(t: &mut Tree, e: El, value: &mut Variant) -> R<()> {
    let (a, b, c) = match value.to_i32()? {
        0 => (0, 6, 7),
        1 => (0, 0, 0),
        2 => (1, 6, 7),
        3 => (1, 6, 0),
        4 => (1, 4, 1),
        _ => (0, 0, 0),
    };
    t.set_native_values(e, "..\\AlphaBlendMode0", Variant::Int(a))?;
    t.set_native_values(e, "..\\AlphaBlendMode1", Variant::Int(b))?;
    t.set_native_values(e, "..\\AlphaBlendMode2", Variant::Int(c))
}

/// `BGSMEmittanceEnabled`.
fn bgsm_emittance_enabled(t: &mut Tree, e: El) -> R<bool> {
    Ok(t.native_values(e, "..\\EmitEnabled")? != 0)
}

/// `BGSMSkewAlphaEnabled`.
fn bgsm_skew_alpha_enabled(t: &mut Tree, e: El) -> R<bool> {
    Ok(t.native_values(e, ".\\Version")? >= 1)
}

pub struct MaterialDefs {
    pub bgsm: Def,
    pub bgem: Def,
}

fn float(name: &str) -> Def {
    df_float(name, DataType::Float32, "", &[])
}

fn float_default(name: &str, default: &str) -> Def {
    df_float(name, DataType::Float32, default, &[])
}

fn boolean(name: &str) -> Def {
    df_bool(name, DataType::U8, "", &[])
}

/// `wbDefineMaterial`.
fn define_material() -> R<MaterialDefs> {
    let base_material = df_struct(
        "Base Material",
        vec![
            df_flags("TileFlags", DataType::U32, &[(0, "V"), (1, "U")], "", &[]),
            float("UOffset"),
            float("VOffset"),
            float_default("UScale", "1.0"),
            float_default("VScale", "1.0"),
            float_default("Alpha", "1.0"),
            df_enum(
                "AlphaBlendMode",
                DataType::None,
                &[
                    (0, "Unknown"),
                    (1, "None"),
                    (2, "Standard"),
                    (3, "Additive"),
                    (4, "Multiplicative"),
                ],
                "Unknown",
                &[
                    Event::GetValue(bgsm_get_alpha_blend_value),
                    Event::SetValue(bgsm_set_alpha_blend_value),
                ],
            ),
            df_integer("AlphaBlendMode0", DataType::U8, "", &[]),
            df_integer("AlphaBlendMode1", DataType::U32, "", &[]),
            df_integer("AlphaBlendMode2", DataType::U32, "", &[]),
            df_integer("AlphaTestRef", DataType::U8, "", &[]),
            boolean("AlphaTest"),
            df_bool("ZBufferWrite", DataType::U8, "yes", &[]),
            df_bool("ZBufferTest", DataType::U8, "yes", &[]),
            boolean("ScreenSpaceReflections"),
            boolean("WetnessControlScreenSpaceReflections"),
            boolean("Decal"),
            boolean("TwoSided"),
            boolean("DecalNoFade"),
            boolean("NonOccluder"),
            boolean("Refraction"),
            boolean("RefractionFalloff"),
            float("RefractionPower"),
            boolean("EnvironmentMapping"),
            float_default("EnvironmentMappingMaskScale", "1.0"),
            boolean("GrayscaleToPaletteColor"),
        ],
        &[],
    );

    let mut bgsm = df_struct(
        "BGSM",
        vec![
            df_chars("Magic", 4, "BGSM", 0, false, &[]),
            df_integer("Version", DataType::U32, "2", &[]),
            // The base material goes here.
            df_struct(
                "Textures",
                vec![
                    wb_len_string_z("Diffuse"),
                    wb_len_string_z("Normal"),
                    wb_len_string_z("SmoothSpec"),
                    wb_len_string_z("Grayscale"),
                    wb_len_string_z("Envmap"),
                    wb_len_string_z("Glow"),
                    wb_len_string_z("Inner Layer"),
                    wb_len_string_z("Wrinkes"),
                    wb_len_string_z("Displacement"),
                ],
                &[],
            ),
            boolean("EnableEditorAlphaRef"),
            boolean("RimLighting"),
            float("RimPower"),
            float("BackLightPower"),
            boolean("SurfaceLighting"),
            float("SubsurfaceLightingRolloff"),
            boolean("SpecularEnabled"),
            wb_color3_name("SpecularColor"),
            float("SpecularMult"),
            float_default("Smoothness", "1.0"),
            float_default("FresnelPower", "5.0"),
            float_default("WetnessControlSpecScale", "-1.0"),
            float_default("WetnessControlSpecPowerScale", "-1.0"),
            float_default("WetnessControlSpecMinvar", "-1.0"),
            float_default("WetnessControlEnvMapScale", "-1.0"),
            float_default("WetnessControlFresnelPower", "-1.0"),
            float_default("WetnessControlMetalness", "-1.0"),
            wb_len_string_z("RootMaterialPath"),
            boolean("AnisoLighting"),
            boolean("EmitEnabled"),
            crate::data_format_nif_types::wb_color3_name_events(
                "EmittanceColor",
                &[Event::GetEnabled(bgsm_emittance_enabled)],
            ),
            float_default("EmittanceMult", "1.0"),
            boolean("ModelSpaceNormals"),
            boolean("ExternalEmittance"),
            boolean("BackLighting"),
            boolean("ReceiveShadows"),
            boolean("HideSecret"),
            boolean("CastShadows"),
            boolean("DissolveFade"),
            boolean("AssumeShadowmask"),
            boolean("Glowmap"),
            boolean("EnvironmentMappingWindow"),
            boolean("EnvironmentMappingEye"),
            boolean("Hair"),
            wb_color3("HairTintColor", "#808080", &[]),
            boolean("Tree"),
            boolean("Facegen"),
            boolean("SkinTint"),
            boolean("Tessellate"),
            float("DisplacementTextureBias"),
            float("DisplacementTextureScale"),
            float("TessellationPNScale"),
            float("TessellationBaseFactor"),
            float("TessellationFadeDistance"),
            float_default("GrayscaleToPaletteScale", "1.0"),
            // UPSTREAM-QUIRK: `dfBool` with events passes none on, so the
            // field is always present.
            boolean("SkewSpecularAlpha"),
        ],
        &[],
    );
    let _ = bgsm_skew_alpha_enabled;

    let mut bgem = df_struct(
        "BGEM",
        vec![
            df_chars("Magic", 4, "BGEM", 0, false, &[]),
            df_integer("Version", DataType::U32, "2", &[]),
            // The base material goes here.
            df_struct(
                "Textures",
                vec![
                    wb_len_string_z("Base"),
                    wb_len_string_z("Grayscale"),
                    wb_len_string_z("Envmap"),
                    wb_len_string_z("Normal"),
                    wb_len_string_z("EnvmapMask"),
                ],
                &[],
            ),
            boolean("BloodEnabled"),
            boolean("EffectLightingEnabled"),
            boolean("FalloffEnabled"),
            boolean("FalloffColorEnabled"),
            boolean("GrayscaleToPaletteAlpha"),
            boolean("SoftEnabled"),
            wb_color3_name("BaseColor"),
            float_default("BaseColorScale", "1.0"),
            float("FalloffStartAngle"),
            float("FalloffStopAngle"),
            float("FalloffStartOpacity"),
            float("FalloffStopOpacity"),
            float_default("LightingInfluence", "1.0"),
            df_integer("EnvmapMinLOD", DataType::U8, "", &[]),
            float_default("SoftDepth", "100.0"),
        ],
        &[],
    );

    bgsm.insert_defs_from(&base_material, 2)?;
    bgem.insert_defs_from(&base_material, 2)?;
    Ok(MaterialDefs { bgsm, bgem })
}

/// The definitions, defined on first use.
pub fn material_defs() -> &'static MaterialDefs {
    static DEFS: OnceLock<MaterialDefs> = OnceLock::new();
    DEFS.get_or_init(|| define_material().unwrap_or_else(|error| panic!("material definitions: {error}")))
}

/// A material file: a BGSM or BGEM tree.
pub struct MaterialFile {
    pub tree: Tree,
    pub root: El,
}

impl MaterialFile {
    /// `TwbBGSMFile.Create`.
    pub fn new_bgsm() -> R<MaterialFile> {
        let mut tree = Tree::new();
        let root = tree.create_root(&material_defs().bgsm, Class::Struct(StructClass::Bgsm))?;
        Ok(MaterialFile { tree, root })
    }

    /// `TwbBGEMFile.Create`.
    pub fn new_bgem() -> R<MaterialFile> {
        let mut tree = Tree::new();
        let root = tree.create_root(&material_defs().bgem, Class::Struct(StructClass::Bgem))?;
        Ok(MaterialFile { tree, root })
    }

    pub fn load_from_data(&mut self, data: &[u8]) -> R<()> {
        self.tree.load_from_data(self.root, data)
    }

    pub fn save_to_data(&mut self) -> R<Vec<u8>> {
        self.tree.save_to_data(self.root)
    }

    pub fn to_text(&mut self) -> R<String> {
        self.tree.to_text(self.root, 0)
    }

    /// `FromJSON`: the material editor's JSON form.
    pub fn from_json(&mut self, text: &str) -> R<()> {
        let json = Json::parse(text)?;
        material_from_json(&mut self.tree, self.root, &json)
    }

    /// `ToJSON`. Upstream does not write materials as JSON.
    pub fn to_json(&mut self, _compact: bool) -> R<String> {
        Err(DfError::new("Not implemented"))
    }
}

/// `ReadJSONValue`.
fn read_json_value(tree: &mut Tree, el: El, json: &Json, js: &str, path: &str) -> R<()> {
    if !json.contains(js) {
        return Ok(());
    }
    match js.as_bytes().first() {
        Some(b'b') => tree.set_native_values(el, path, Variant::Int(i64::from(json.b(js)?))),
        Some(b'f') => tree.set_native_values(el, path, Variant::Float(json.f(js)?)),
        Some(b'i') => tree.set_native_values(el, path, Variant::Int(i64::from(json.i(js)?))),
        _ => tree.set_edit_values(el, path, &json.s(js)?),
    }
}

fn material_from_json(tree: &mut Tree, el: El, json: &Json) -> R<()> {
    tree.set_to_default(el)?;
    if !json.is_object() {
        return Ok(());
    }
    // `BaseMaterialFromJSON`.
    if json.contains("bTileU") {
        let flags = tree
            .elements(el, "TileFlags")?
            .ok_or_else(|| DfError::new("no TileFlags"))?;
        tree.set_native_values(flags, "U", Variant::Int(i64::from(json.b("bTileU")?)))?;
    }
    if json.contains("bTileV") {
        let flags = tree
            .elements(el, "TileFlags")?
            .ok_or_else(|| DfError::new("no TileFlags"))?;
        tree.set_native_values(flags, "V", Variant::Int(i64::from(json.b("bTileV")?)))?;
    }
    for (js, path) in BASE_MAP {
        read_json_value(tree, el, json, js, path)?;
    }
    let map = if tree.class(el) == Class::Struct(StructClass::Bgsm) {
        SHADER_MAP
    } else {
        EFFECT_MAP
    };
    for (js, path) in map {
        read_json_value(tree, el, json, js, path)?;
    }
    Ok(())
}

/// `TwbBGSMFile.UnSerialize` and `TwbBGEMFile.UnSerialize`: binary data
/// with the magic, else JSON.
pub(crate) fn material_unserialize(tree: &mut Tree, el: El, data: Option<&[u8]>, data_size: i32) -> R<i32> {
    let magic: &[u8] = if tree.class(el) == Class::Struct(StructClass::Bgsm) {
        b"BGSM"
    } else {
        b"BGEM"
    };
    let Some(bytes) = data else {
        return tree.struct_unserialize(el, None, data_size);
    };
    let result = bytes.len() as i32;
    if bytes.len() > 4 && &bytes[..4] == magic {
        return tree.struct_unserialize(el, data, data_size);
    }
    let text = String::from_utf8_lossy(bytes);
    let json = Json::parse(&text)?;
    material_from_json(tree, el, &json)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bgsm_bytes() -> Vec<u8> {
        let mut file = MaterialFile::new_bgsm().unwrap();
        file.tree.set_to_default(file.root).unwrap();
        file.tree
            .set_edit_values(file.root, "Textures\\Diffuse", "Materials\\Test\\a_d.dds")
            .unwrap();
        file.save_to_data().unwrap()
    }

    #[test]
    fn bgsm_round_trip() {
        let data = bgsm_bytes();
        let mut file = MaterialFile::new_bgsm().unwrap();
        file.load_from_data(&data).unwrap();
        assert_eq!(
            file.tree.edit_values(file.root, "Textures\\Diffuse").unwrap(),
            "Test/a_d.dds"
        );
        assert_eq!(file.tree.edit_values(file.root, "AlphaBlendMode").unwrap(), "None");
        assert_eq!(file.tree.edit_values(file.root, "HairTintColor").unwrap(), "#808080");
        assert_eq!(file.save_to_data().unwrap(), data);
    }

    #[test]
    fn bgsm_from_json() {
        let mut file = MaterialFile::new_bgsm().unwrap();
        file.load_from_data(br#"{"fAlpha": 0.5, "bTwoSided": true, "sDiffuseTexture": "x/y.dds"}"#)
            .unwrap();
        assert_eq!(file.tree.edit_values(file.root, "Alpha").unwrap(), "0.500000");
        assert_eq!(file.tree.edit_values(file.root, "TwoSided").unwrap(), "yes");
        assert_eq!(
            file.tree.edit_values(file.root, "Textures\\Diffuse").unwrap(),
            "x/y.dds"
        );
    }
}
