// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcVertexPaint.pas

//! `Vertex color painting`: fills the vertex colors of the geometry with
//! one color, adjusts them in HSL, removes them, or replaces one color
//! with another.

use xedit_core::delphi::round;
use xedit_io::encoding::lower_case;

use crate::data_format::{DfError, El, R, Tree, df_str_to_float};
use crate::data_format_nif::{NifFile, NifVersion, block_is_ni_object, block_property_by_type, blocks_by_type};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, trim};
use crate::variant::{Variant, str_to_int64};

pub struct ProcVertexPaint {
    base: ProcBase,
    /// `iMode`: 0 set, 1 adjust, 2 remove, 3 replace.
    mode_checked: i32,
    name_text: String,
    skip_color_checked: bool,
    skip_color_text: String,
    color_text: String,
    color2_text: String,
    add_if_missing_checked: bool,
    all_white_checked: bool,
    adjust_mod_index: i32,
    adjust_h_text: String,
    adjust_s_text: String,
    adjust_l_text: String,
    adjust_a_text: String,

    name: String,
    mode: i32,
    color: u32,
    color2: u32,
    skip_color: u32,
    skip: bool,
    all_white: bool,
    add_if_missing: bool,
    adjust_mod: i32,
    adjust_h: f64,
    adjust_s: f64,
    adjust_l: f64,
    adjust_a: f64,
}

impl ProcVertexPaint {
    pub fn new() -> ProcVertexPaint {
        ProcVertexPaint {
            base: ProcBase::new("Vertex color painting", GameType::ALL, &["nif"]),
            mode_checked: 0,
            name_text: String::new(),
            skip_color_checked: false,
            skip_color_text: "FFFFFFFF".to_owned(),
            color_text: "FFFFFFFF".to_owned(),
            color2_text: "FFFFFFFF".to_owned(),
            add_if_missing_checked: false,
            all_white_checked: false,
            adjust_mod_index: 0,
            adjust_h_text: String::new(),
            adjust_s_text: String::new(),
            adjust_l_text: String::new(),
            adjust_a_text: String::new(),
            name: String::new(),
            mode: 0,
            color: 0,
            color2: 0,
            skip_color: 0,
            skip: false,
            all_white: false,
            add_if_missing: false,
            adjust_mod: 0,
            adjust_h: 0.0,
            adjust_s: 0.0,
            adjust_l: 0.0,
            adjust_a: 0.0,
        }
    }

    /// `'#' + IntToHex(fColor, 8)`.
    fn color_text(color: u32) -> String {
        format!("#{color:08X}")
    }
}

/// `StrToInt64('$' + aText)`.
fn hex_value(text: &str) -> Option<u32> {
    if text.is_empty() {
        return None;
    }
    str_to_int64(&format!("${text}")).map(|value| value as u32)
}

/// `rgb2hsl`.
fn rgb2hsl(red: u8, green: u8, blue: u8) -> (f64, f64, f64) {
    let cmin = f64::from(red.min(green).min(blue)) / 255.0;
    let cmax = f64::from(red.max(green).max(blue)) / 255.0;
    let r = f64::from(red) / 255.0;
    let g = f64::from(green) / 255.0;
    let b = f64::from(blue) / 255.0;
    let mut h = (cmax + cmin) / 2.0;
    let s: f64;
    let l = (cmax + cmin) / 2.0;
    if cmax == cmin {
        h = 0.0;
        s = 0.0;
    } else {
        let d = cmax - cmin;
        if l > 0.5 {
            s = d / (2.0 - cmax - cmin);
        } else {
            s = d / (cmax + cmin);
        }
        let dd = if g < b { 6.0 } else { 0.0 };
        if cmax == r {
            h = (g - b) / d + dd;
        } else if cmax == g {
            h = (b - r) / d + 2.0;
        } else if cmax == b {
            h = (r - g) / d + 4.0;
        }
        h /= 6.0;
    }
    (h, s, l)
}

/// `hue2rgb`.
fn hue2rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

/// `hsl2rgb`.
fn hsl2rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        (
            hue2rgb(p, q, h + 1.0 / 3.0),
            hue2rgb(p, q, h),
            hue2rgb(p, q, h - 1.0 / 3.0),
        )
    };
    (round(r * 255.0) as u8, round(g * 255.0) as u8, round(b * 255.0) as u8)
}

/// `FloatColorToByte`.
fn float_color_to_byte(value: f64) -> u8 {
    let c = round(value * 255.0).clamp(0, 255);
    c as u8
}

/// `ByteColorToFloat`.
fn byte_color_to_float(value: u8) -> f64 {
    (f64::from(value) / 255.0).clamp(0.0, 255.0)
}

/// `AdjustColor`.
#[allow(clippy::too_many_arguments)]
fn adjust_color(
    r: &mut u8,
    g: &mut u8,
    b: &mut u8,
    a: &mut f64,
    adjust_mod: i32,
    h_mult: f64,
    s_mult: f64,
    l_mult: f64,
    a_mult: f64,
) {
    let (mut h, mut s, mut l) = rgb2hsl(*r, *g, *b);

    if adjust_mod == 0 {
        h *= h_mult;
        s *= s_mult;
        l *= l_mult;
        *a *= a_mult;
    } else {
        h += h_mult;
        s += s_mult;
        l += l_mult;
        *a += a_mult;
    }
    // The clamps of `AdjustColor`, each channel to 0 .. 1.
    h = h.clamp(0.0, 1.0);
    s = s.clamp(0.0, 1.0);
    l = l.clamp(0.0, 1.0);
    *a = a.clamp(0.0, 1.0);

    let (red, green, blue) = hsl2rgb(h, s, l);
    *r = red;
    *g = green;
    *b = blue;
}

/// `Elements[aPath].LinksTo` of a missing element reads through nil
/// upstream.
fn link(tree: &mut Tree, element: El, path: &str) -> R<Option<El>> {
    let element = tree.elements(element, path)?.ok_or_else(access_violation)?;
    tree.links_to(element)
}

impl Proc for ProcVertexPaint {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.mode_checked = storage.get_integer("iMode", self.mode_checked);
        self.name_text = storage.get_string("sName", "");
        self.skip_color_checked = storage.get_bool("bSkipColor", self.skip_color_checked);
        self.skip_color_text = storage.get_string("sSkipColor", &self.skip_color_text);
        self.color_text = storage.get_string("sColor", &self.color_text);
        self.color2_text = storage.get_string("sColor2", &self.color2_text);
        self.add_if_missing_checked = storage.get_bool("bAddIfMissing", self.add_if_missing_checked);
        self.all_white_checked = storage.get_bool("bAllWhite", self.all_white_checked);
        // `cbAdjustMod.ItemIndex := StorageGetInteger(...)` raises for an
        // out of range index, which the frame catches and resets to 0.
        let adjust_mod = storage.get_integer("iAdjustMod", self.adjust_mod_index);
        self.adjust_mod_index = match adjust_mod {
            0 | 1 => adjust_mod,
            _ => 0,
        };
        self.adjust_h_text = storage.get_string("sAdjustH", "");
        self.adjust_s_text = storage.get_string("sAdjustS", "");
        self.adjust_l_text = storage.get_string("sAdjustL", "");
        self.adjust_a_text = storage.get_string("sAdjustA", "");
    }

    fn on_start(&mut self) -> R<()> {
        self.mode = self.mode_checked;
        self.name = lower_case(trim(&self.name_text));
        self.skip = self.skip_color_checked;
        self.all_white = self.all_white_checked;
        self.add_if_missing = self.add_if_missing_checked;
        self.adjust_mod = self.adjust_mod_index;
        let adjust_default: f64 = if self.adjust_mod == 0 { 1.0 } else { 0.0 };

        if self.skip {
            self.skip_color =
                hex_value(&self.skip_color_text).ok_or_else(|| DfError::new("Skip color is not a valid hex number"))?;
        }

        if matches!(self.mode, 0 | 2 | 3) {
            self.color = hex_value(&self.color_text).ok_or_else(|| DfError::new("Color is not a valid hex number"))?;
        }

        if self.mode == 3 {
            self.color2 = hex_value(&self.color2_text)
                .ok_or_else(|| DfError::new("Replacement color is not a valid hex number"))?;
        }

        if self.mode == 1 {
            if self.adjust_h_text.is_empty()
                && self.adjust_s_text.is_empty()
                && self.adjust_l_text.is_empty()
                && self.adjust_a_text.is_empty()
            {
                return Err(DfError::new("Empty adjust values"));
            }

            self.adjust_h = if self.adjust_h_text.is_empty() {
                adjust_default
            } else {
                df_str_to_float(&self.adjust_h_text).map_err(|_| DfError::new("Adjust H is not a float value"))?
            };
            self.adjust_s = if self.adjust_s_text.is_empty() {
                adjust_default
            } else {
                df_str_to_float(&self.adjust_s_text).map_err(|_| DfError::new("Adjust S is not a float value"))?
            };
            self.adjust_l = if self.adjust_l_text.is_empty() {
                adjust_default
            } else {
                df_str_to_float(&self.adjust_l_text).map_err(|_| DfError::new("Adjust L is not a float value"))?
            };
            self.adjust_a = if self.adjust_a_text.is_empty() {
                adjust_default
            } else {
                df_str_to_float(&self.adjust_a_text).map_err(|_| DfError::new("Adjust A is not a float value"))?
            };
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let c = ProcVertexPaint::color_text(self.color);
        let c2 = ProcVertexPaint::color_text(self.color2);
        let cskip = ProcVertexPaint::color_text(self.skip_color);

        let mut changed = false;
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let version = nif.tree.nif.nif_version;
        let tree = &mut nif.tree;

        for block in blocks_by_type(tree, "NiAVObject", true)? {
            if !self.name.is_empty() && !lower_case(&tree.edit_values(block, "Name")?).contains(&self.name) {
                continue;
            }

            if block_is_ni_object(tree, block, "NiTriBasedGeom", true) {
                let Some(data) = link(tree, block, "Data")? else {
                    continue;
                };

                // NiTriBasedGeomData: set vertex colors
                if self.mode == 0 {
                    if self.add_if_missing && tree.native_values(data, "Has Vertex Colors")?.to_i64()? == 0 {
                        tree.set_native_values(data, "Has Vertex Colors", Variant::Int(1))?;
                        changed = true;
                    }

                    let Some(entries) = tree.elements(data, "Vertex Colors")? else {
                        continue;
                    };

                    if tree.count(entries) == 0
                        && let Some(vertices) = tree.elements(data, "Vertices")?
                    {
                        let count = tree.count(vertices);
                        tree.set_count(entries, count)?;
                    }

                    for j in 0..tree.count(entries) {
                        let item = tree.item(entries, j)?;
                        if tree.edit_value(item)? != c {
                            if self.skip && tree.edit_value(item)? == cskip {
                                continue;
                            }
                            tree.set_edit_value(item, &c)?;
                            changed = true;
                        }
                    }
                }
                // NiTriBasedGeomData: adjust vertex colors
                else if self.mode == 1 {
                    let Some(entries) = tree.elements(data, "Vertex Colors")? else {
                        continue;
                    };

                    for j in 0..tree.count(entries) {
                        let item = tree.item(entries, j)?;
                        if self.skip && tree.edit_value(item)? == cskip {
                            continue;
                        }

                        let mut r = float_color_to_byte(tree.native_values(item, "R")?.to_f64()?);
                        let mut g = float_color_to_byte(tree.native_values(item, "G")?.to_f64()?);
                        let mut b = float_color_to_byte(tree.native_values(item, "B")?.to_f64()?);
                        let mut a = tree.native_values(item, "A")?.to_f64()?;

                        adjust_color(
                            &mut r,
                            &mut g,
                            &mut b,
                            &mut a,
                            self.adjust_mod,
                            self.adjust_h,
                            self.adjust_s,
                            self.adjust_l,
                            self.adjust_a,
                        );

                        tree.set_native_values(item, "R", Variant::Float(f64::from(r) / 255.0))?;
                        tree.set_native_values(item, "G", Variant::Float(f64::from(g) / 255.0))?;
                        tree.set_native_values(item, "B", Variant::Float(f64::from(b) / 255.0))?;
                        tree.set_native_values(item, "A", Variant::Float(a))?;

                        changed = true;
                    }
                }
                // NiTriBasedGeomData: remove vertex colors
                else if self.mode == 2 {
                    if matches!(version, NifVersion::Tes5 | NifVersion::Sse) {
                        let Some(shader) = block_property_by_type(tree, block, "BSShaderProperty", true)? else {
                            continue;
                        };

                        if crate::data_format_nif::block_type(tree, shader) == "BSLightingShaderProperty" {
                            if tree.edit_values(shader, "Shader Type")? == "Parallax" {
                                continue;
                            }

                            if tree.native_values(shader, "Shader Flags 2\\Tree_Anim")?.to_bool()? {
                                continue;
                            }
                        }
                    }

                    let Some(entries) = tree.elements(data, "Vertex Colors")? else {
                        continue;
                    };

                    let mut white = true;
                    // check existing colors if we want to remove the white ones only
                    if self.all_white {
                        for j in 0..tree.count(entries) {
                            let item = tree.item(entries, j)?;
                            if tree.edit_value(item)? != c {
                                white = false;
                                break;
                            }
                        }
                    }

                    if white {
                        tree.set_native_values(data, "Has Vertex Colors", Variant::Int(0))?;
                        changed = true;
                    }
                }
                // NiTriBasedGeomData: replace vertex color
                else if self.mode == 3 {
                    let Some(entries) = tree.elements(data, "Vertex Colors")? else {
                        continue;
                    };

                    for j in 0..tree.count(entries) {
                        let item = tree.item(entries, j)?;
                        if tree.edit_value(item)? == c {
                            if self.skip && tree.edit_value(item)? == cskip {
                                continue;
                            }
                            tree.set_edit_value(item, &c2)?;
                            changed = true;
                        }
                    }
                }
            } else if block_is_ni_object(tree, block, "BSTriShape", true)
                || (version == NifVersion::Sse && block_is_ni_object(tree, block, "NiSkinPartition", true))
            {
                // BSTriShape: set vertex colors
                if self.mode == 0 {
                    if !tree.native_values(block, "VertexDesc\\VF\\VF_COLORS")?.to_bool()? {
                        if self.add_if_missing {
                            tree.set_native_values(block, "VertexDesc\\VF\\VF_COLORS", Variant::Int(1))?;
                            changed = true;
                        } else {
                            continue;
                        }
                    }

                    let Some(entries) = tree.elements(block, "Vertex Data")? else {
                        continue;
                    };

                    for j in 0..tree.count(entries) {
                        let item = tree.item(entries, j)?;
                        let clr = tree.edit_values(item, "Vertex Colors")?;
                        if self.skip && clr == cskip {
                            continue;
                        }
                        if clr != c {
                            tree.set_edit_values(item, "Vertex Colors", &c)?;
                            changed = true;
                        }
                    }
                }
                // BSTriShape: adjust vertex colors
                else if self.mode == 1 {
                    if !tree.native_values(block, "VertexDesc\\VF\\VF_COLORS")?.to_bool()? {
                        continue;
                    }

                    let Some(entries) = tree.elements(block, "Vertex Data")? else {
                        continue;
                    };

                    for j in 0..tree.count(entries) {
                        let item = tree.item(entries, j)?;
                        let entry = tree.elements(item, "Vertex Colors")?.ok_or_else(access_violation)?;
                        if self.skip && tree.edit_value(entry)? == cskip {
                            continue;
                        }

                        let mut r = tree.native_values(entry, "R")?.to_f64()? as u8;
                        let mut g = tree.native_values(entry, "G")?.to_f64()? as u8;
                        let mut b = tree.native_values(entry, "B")?.to_f64()? as u8;
                        let mut a = byte_color_to_float(tree.native_values(entry, "A")?.to_f64()? as u8);

                        adjust_color(
                            &mut r,
                            &mut g,
                            &mut b,
                            &mut a,
                            self.adjust_mod,
                            self.adjust_h,
                            self.adjust_s,
                            self.adjust_l,
                            self.adjust_a,
                        );

                        tree.set_native_values(entry, "R", Variant::Int(i64::from(r)))?;
                        tree.set_native_values(entry, "G", Variant::Int(i64::from(g)))?;
                        tree.set_native_values(entry, "B", Variant::Int(i64::from(b)))?;
                        tree.set_native_values(entry, "A", Variant::Int(i64::from(float_color_to_byte(a))))?;

                        changed = true;
                    }
                }
                // BSTriShape: remove vertex colors
                else if self.mode == 2 {
                    // don't remove from NiSkinPartition, causes issues?
                    if !block_is_ni_object(tree, block, "BSTriShape", true) {
                        continue;
                    }

                    // and also skip skinned shapes for the same reason
                    if link(tree, block, "Skin")?.is_some() {
                        continue;
                    }

                    if !tree.native_values(block, "VertexDesc\\VF\\VF_COLORS")?.to_bool()? {
                        continue;
                    }

                    if version == NifVersion::Sse {
                        let Some(shader) = block_property_by_type(tree, block, "BSShaderProperty", true)? else {
                            continue;
                        };

                        if crate::data_format_nif::block_type(tree, shader) == "BSLightingShaderProperty" {
                            if tree.edit_values(shader, "Shader Type")? == "Parallax" {
                                continue;
                            }

                            if tree.native_values(shader, "Shader Flags 2\\Tree_Anim")?.to_bool()? {
                                continue;
                            }
                        }
                    }

                    let Some(entries) = tree.elements(block, "Vertex Data")? else {
                        continue;
                    };

                    let mut white = true;
                    // check existing colors if we want to remove the white ones only
                    if self.all_white {
                        for j in 0..tree.count(entries) {
                            let item = tree.item(entries, j)?;
                            if tree.edit_values(item, "Vertex Colors")? != c {
                                white = false;
                                break;
                            }
                        }
                    }

                    if white {
                        tree.set_native_values(block, "VertexDesc\\VF\\VF_COLORS", Variant::Int(0))?;
                        changed = true;
                    }
                }
                // BSTriShape: replace vertex color
                else if self.mode == 3 {
                    if !tree.native_values(block, "VertexDesc\\VF\\VF_COLORS")?.to_bool()? {
                        continue;
                    }

                    let Some(entries) = tree.elements(block, "Vertex Data")? else {
                        continue;
                    };

                    for j in 0..tree.count(entries) {
                        let item = tree.item(entries, j)?;
                        let clr = tree.edit_values(item, "Vertex Colors")?;
                        if self.skip && clr == cskip {
                            continue;
                        }
                        if clr != c {
                            tree.set_edit_values(item, "Vertex Colors", &c2)?;
                            changed = true;
                        }
                    }
                }
            }
        }

        if changed {
            return nif.save_to_data();
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors_read_as_strtoint64() {
        assert_eq!(hex_value("FFFFFFFF"), Some(u32::MAX));
        assert_eq!(hex_value("FF00FF00"), Some(0xFF00_FF00));
        assert_eq!(hex_value(""), None);
        assert_eq!(hex_value("xyz"), None);
        assert_eq!(ProcVertexPaint::color_text(0xFFFF_FFFF), "#FFFFFFFF");
    }

    #[test]
    fn hsl_round_trip_and_clamping() {
        // A grey has no hue or saturation.
        assert_eq!(rgb2hsl(128, 128, 128).0, 0.0);
        let (r, g, b) = hsl2rgb(0.0, 1.0, 0.5);
        assert_eq!((r, g, b), (255, 0, 0));
        let (r, g, b) = hsl2rgb(1.0 / 3.0, 1.0, 0.5);
        assert_eq!((r, g, b), (0, 255, 0));

        // FloatColorToByte rounds half to even and clamps.
        assert_eq!(float_color_to_byte(0.5), 128);
        assert_eq!(float_color_to_byte(-1.0), 0);
        assert_eq!(float_color_to_byte(2.0), 255);
        assert_eq!(byte_color_to_float(255), 1.0);

        // The multiply mode scales each channel of the HSL colour, the add
        // mode shifts it, and the result is clamped.
        let mut r = 255;
        let mut g = 0;
        let mut b = 0;
        let mut a = 1.0;
        adjust_color(&mut r, &mut g, &mut b, &mut a, 0, 0.5, 1.0, 1.0, 1.0);
        assert_eq!((r, g, b, a), (255, 0, 0, 1.0));
        adjust_color(&mut r, &mut g, &mut b, &mut a, 1, 0.5, 0.0, 0.0, 0.5);
        // A hue of 0.5 is cyan; the alpha is clamped to one.
        assert_eq!((r, g, b, a), (0, 255, 255, 1.0));
    }

    #[test]
    fn the_adjust_mode_comes_from_the_settings() {
        let settings = |text: &str| crate::sniff::processor::MemIniFile::from_text(text);
        let ini = settings("[Vertexcolorpainting]\r\niMode=1\r\niAdjustMod=1\r\nsAdjustH=0.1\r\n");
        let mut proc = ProcVertexPaint::new();
        proc.on_show(&Storage::new("Vertexcolorpainting".to_owned(), Some(&ini)));
        proc.on_start().unwrap();
        assert_eq!(proc.adjust_mod, 1);
        // An index the combo box does not have gives the first item.
        let ini = settings("[Vertexcolorpainting]\r\niMode=1\r\niAdjustMod=7\r\nsAdjustH=0.1\r\n");
        let mut proc = ProcVertexPaint::new();
        proc.on_show(&Storage::new("Vertexcolorpainting".to_owned(), Some(&ini)));
        proc.on_start().unwrap();
        assert_eq!(proc.adjust_mod, 0);
    }

    #[test]
    fn settings_defaults_and_the_adjust_branch() {
        let mut proc = ProcVertexPaint::new();
        let storage = Storage::new("Vertexcolorpainting".to_owned(), None);
        proc.on_show(&storage);
        proc.on_start().unwrap();
        assert_eq!(proc.mode, 0);
        assert_eq!(proc.color, u32::MAX);
        assert!(!proc.skip);
        assert!(!proc.add_if_missing);

        // The empty adjust values are refused.
        let mut proc = ProcVertexPaint::new();
        proc.mode_checked = 1;
        assert_eq!(proc.on_start().unwrap_err().0, "Empty adjust values");

        // A mode the settings give takes its colour as hex; anything else
        // is refused.
        let mut proc = ProcVertexPaint::new();
        proc.color_text = "nope".to_owned();
        assert_eq!(proc.on_start().unwrap_err().0, "Color is not a valid hex number");

        let mut proc = ProcVertexPaint::new();
        proc.mode_checked = 1;
        proc.adjust_h_text = "0.5".to_owned();
        proc.adjust_s_text = "x".to_owned();
        assert_eq!(proc.on_start().unwrap_err().0, "Adjust S is not a float value");
        proc.adjust_s_text = String::new();
        proc.on_start().unwrap();
        // An empty value takes the default of the mode: 1 for multiply, 0
        // for add.
        assert_eq!(
            (proc.adjust_h, proc.adjust_s, proc.adjust_l, proc.adjust_a),
            (0.5, 1.0, 1.0, 1.0)
        );
        proc.adjust_mod_index = 1;
        proc.adjust_s_text = String::new();
        proc.on_start().unwrap();
        assert_eq!(
            (proc.adjust_h, proc.adjust_s, proc.adjust_l, proc.adjust_a),
            (0.5, 0.0, 0.0, 0.0)
        );
    }
}
