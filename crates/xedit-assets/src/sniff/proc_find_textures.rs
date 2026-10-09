// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcFindTextures.pas

//! `Find textures`: reports the properties of the DDS files that match
//! the filters of the value list editor (format, resolution, mipmaps,
//! alpha, cube map, bits per pixel, compression, DX10 and XBOX), with the
//! DDS header dump as the option; without the report it copies the files.

use std::collections::BTreeSet;

use xedit_io::archive::{ArchiveType, format_size};
use xedit_io::dds::{self, Dxgi, MAX_HEADER_SIZE};

use crate::data_format::{DfError, El, R, Tree};
use crate::data_format_misc::{MiscFile, create_misc_file};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, is_power_of_2};
use crate::variant::str_to_int;

/// The properties of the texture the filter tests (`prop` of `ProcessFile`).
struct Properties {
    dxgi_format: i32,
    format_name: String,
    width: i32,
    height: i32,
    resolution: i32,
    size: i64,
    bits_per_pixel: i32,
    mip_maps: bool,
    compressed: bool,
    alpha: bool,
    cube_map: bool,
    xbox: bool,
}

pub struct ProcFindTextures {
    base: ProcBase,
    report_only_checked: bool,
    header_dump_checked: bool,
    format_filter_text: String,
    formats_text: String,
    resolution_text: String,
    mip_maps_text: String,
    has_alpha_text: String,
    cube_map_text: String,
    bits_per_pixel_text: String,
    compressed_text: String,
    dx10_text: String,
    xbox_text: String,

    report_only: bool,
    header_dump: bool,
    formats: BTreeSet<i32>,
    resolution: i32,
    bits_per_pixel: i32,
    compressed: String,
    mip_maps: String,
    has_alpha: String,
    cube_map: String,
    dx10: String,
    xbox: String,
}

impl ProcFindTextures {
    pub fn new() -> ProcFindTextures {
        ProcFindTextures {
            base: ProcBase::new("Find textures", GameType::ALL, &["dds"]),
            report_only_checked: true,
            header_dump_checked: false,
            format_filter_text: String::new(),
            formats_text: String::new(),
            resolution_text: String::new(),
            mip_maps_text: String::new(),
            has_alpha_text: String::new(),
            cube_map_text: String::new(),
            bits_per_pixel_text: String::new(),
            compressed_text: String::new(),
            dx10_text: String::new(),
            xbox_text: String::new(),
            report_only: true,
            header_dump: false,
            formats: BTreeSet::new(),
            resolution: 0,
            bits_per_pixel: 0,
            compressed: String::new(),
            mip_maps: String::new(),
            has_alpha: String::new(),
            cube_map: String::new(),
            dx10: String::new(),
            xbox: String::new(),
        }
    }

    /// `CheckBool`.
    fn check_bool(value: &str, state: bool) -> bool {
        value.is_empty() || (value == "Yes" && state) || (value == "No" && !state)
    }

    /// `IfThen`.
    fn if_then(state: bool, text: &'static str) -> &'static str {
        if state { text } else { "" }
    }
}

/// The `Format` line of the report: `\tWidth: %04d  Height: %04d  Size: %s
/// %d Bit  %s  %s%s%s%s`.
fn report_line(prop: &Properties) -> String {
    format!(
        "\tWidth: {:04}  Height: {:04}  Size: {}    {} Bit  {}  {}{}{}{}",
        prop.width,
        prop.height,
        format_size(prop.size),
        prop.bits_per_pixel,
        prop.format_name,
        ProcFindTextures::if_then(prop.mip_maps, "  MipMaps"),
        ProcFindTextures::if_then(prop.alpha, "  Alpha"),
        ProcFindTextures::if_then(prop.cube_map, "  CubeMap"),
        ProcFindTextures::if_then(prop.xbox, "  XBOX"),
    )
}

/// `TwbDDSFile` `UnSerialize` and the header dump of the report: the JSON
/// of every enabled item but `Magic`, without the fields the upstream
/// report drops.
fn header_dump(tree: &mut Tree, root: El) -> R<Vec<String>> {
    let mut js = String::new();
    for index in 1..tree.count(root) {
        let item = tree.item(root, index)?;
        if !tree.enabled(item)? {
            continue;
        }
        for child in 0..tree.count(item) {
            let child = tree.item(item, child)?;
            js.push_str(&tree.to_json(child, false)?);
            js.push('\r');
        }
    }
    let mut log = Vec::new();
    for s in js.split(['\n', '\r']) {
        if s.trim_matches(|c| c <= ' ').is_empty()
            || s == "{"
            || s == "}"
            || s.contains("dwSize")
            || s.contains("dwCaps3")
            || s.contains("dwCaps4")
            || s.contains("Reserved")
        {
            continue;
        }
        log.push(s.to_owned());
    }
    Ok(log)
}

impl Proc for ProcFindTextures {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.report_only_checked = storage.get_bool("bReportOnly", self.report_only_checked);
        self.header_dump_checked = storage.get_bool("bHeaderDump", self.header_dump_checked);
        self.format_filter_text = storage.get_string("sFormatFilter", "");
        self.formats_text = storage.get_string("sFormats", "");
        // `Init` fills the value list editor in this order; each row reads
        // its value from the storage as `s<Name>`.
        self.resolution_text = storage.get_string("sResolution", "");
        self.mip_maps_text = storage.get_string("sMipMaps", "");
        self.has_alpha_text = storage.get_string("sHas Alpha", "");
        self.cube_map_text = storage.get_string("sCubeMap", "");
        self.bits_per_pixel_text = storage.get_string("sBitsPerPixel", "");
        self.compressed_text = storage.get_string("sBlock Compressed", "");
        self.dx10_text = storage.get_string("sDX10+ Supported", "");
        self.xbox_text = storage.get_string("sXBOX Texture", "");
    }

    fn on_start(&mut self) -> R<()> {
        self.report_only = self.report_only_checked;
        self.header_dump = self.header_dump_checked;
        self.formats = BTreeSet::new();
        // `TDXGI(StrToInt(f))` in a `try`, which drops the rest of the
        // list at the first value that is not an integer.
        if !self.formats_text.is_empty() {
            for part in self.formats_text.split(',') {
                match str_to_int(part) {
                    Some(value) => {
                        self.formats.insert(value);
                    }
                    None => break,
                }
            }
        }

        let s = &self.resolution_text;
        if !s.is_empty() {
            if s == "Not power of 2" {
                self.resolution = 1;
            } else {
                let mut sign = 1;
                if s.starts_with('<') {
                    sign = -1;
                }
                let rest = match s.find(' ') {
                    Some(index) => &s[index + 1..],
                    None => &s[..],
                };
                self.resolution = sign * str_to_int(rest).unwrap_or(0);
            }
        } else {
            self.resolution = 0;
        }

        self.mip_maps = self.mip_maps_text.clone();
        self.compressed = self.compressed_text.clone();
        self.has_alpha = self.has_alpha_text.clone();
        self.cube_map = self.cube_map_text.clone();
        self.bits_per_pixel = str_to_int(&self.bits_per_pixel_text).unwrap_or(0);
        self.dx10 = self.dx10_text.clone();
        self.xbox = self.xbox_text.clone();
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let in_archive = file.in_archive();
        let archive_type = file.input.archive.as_ref().map(|archive| archive.archive_type());
        let ba2_dds = in_archive && archive_type.is_some_and(ArchiveType::is_dds);

        let mut buf: Vec<u8> = Vec::new();
        let mut len = 0usize;
        let mut loose_size = 0i64;
        let mut prop;

        // BA2 DDS texture, get all props from the archive entry
        if ba2_dds {
            let entry = file.file_entry.ok_or_else(|| DfError::new("no archive entry"))?;
            let mut size = dds::HEADER_SIZE as i64;
            for chunk in &entry.dds.tex_chunks {
                size += i64::from(chunk.chunk.size);
            }
            prop = Properties {
                dxgi_format: i32::from(entry.dds.dxgi_format),
                format_name: entry.dxgi_format_name().to_owned(),
                width: i32::from(entry.dds.width),
                height: i32::from(entry.dds.height),
                resolution: 0,
                size,
                bits_per_pixel: i32::from(dds::bits_per_pixel(Dxgi(entry.dds.dxgi_format))),
                mip_maps: entry.dds.num_mips > 1,
                compressed: false,
                alpha: false,
                cube_map: entry.is_cube_map(),
                xbox: false,
            };
        } else {
            // texture in ordinary archive, unpack entirely
            if in_archive {
                buf = file.get_data()?;
                len = buf.len();
            }
            // texture in loose file, read dds header only
            else {
                let path = format!("{}{}", file.input.input_directory, file.file_name);
                buf.resize(MAX_HEADER_SIZE, 0);
                let data = std::fs::read(&path)
                    .map_err(|error| DfError::new(format!("Cannot open file \"{path}\". {error}")))?;
                len = data.len().min(MAX_HEADER_SIZE);
                buf[..len].copy_from_slice(&data[..len]);
                loose_size = data.len() as i64;
            }

            if !dds::is_dds(&buf[..len]) {
                return Err(DfError::new("Not a valid DDS file"));
            }

            let header = dds::DdsHeader::read(&buf[..len]);
            let dxgi_format = i32::from(dds::dxgi(&buf[..len]).0);
            let mut format_name = dds::dxgi_format_name(dds::dxgi(&buf[..len]).0).to_owned();
            let mut bits_per_pixel = i32::from(dds::bits_per_pixel(Dxgi(dxgi_format as u8)));
            let size = if in_archive { len as i64 } else { loose_size };
            // no valid DXGI type, try D3DFMT
            if dxgi_format == 0 {
                let d3d = dds::d3dfmt(&buf[..len]);
                format_name = dds::d3dfmt_format_name(d3d).to_owned();
                bits_per_pixel = header.pixel_format.rgb_bit_count as i32;
            }
            prop = Properties {
                dxgi_format,
                format_name,
                width: header.width as i32,
                height: header.height as i32,
                resolution: 0,
                size,
                bits_per_pixel,
                mip_maps: header.mip_map_count > 1,
                compressed: false,
                alpha: false,
                cube_map: dds::is_cube_map(&buf[..len]),
                xbox: dds::is_xbox(&buf[..len]),
            };
        }

        prop.resolution = prop.width.max(prop.height);
        prop.compressed = dds::is_compressed(Dxgi(prop.dxgi_format as u8));
        prop.alpha = dds::has_alpha(Dxgi(prop.dxgi_format as u8));

        if !((self.formats.is_empty() || self.formats.contains(&prop.dxgi_format))
            && (self.resolution == 0
                || (self.resolution == 1 && (!is_power_of_2(prop.width as u32) || !is_power_of_2(prop.height as u32)))
                || (self.resolution > 1 && prop.resolution >= self.resolution)
                || (self.resolution < 0 && prop.resolution < -self.resolution))
            && (self.bits_per_pixel == 0 || prop.bits_per_pixel == self.bits_per_pixel)
            && ProcFindTextures::check_bool(&self.mip_maps, prop.mip_maps)
            && ProcFindTextures::check_bool(&self.compressed, prop.compressed)
            && ProcFindTextures::check_bool(&self.has_alpha, prop.alpha)
            && ProcFindTextures::check_bool(&self.cube_map, prop.cube_map)
            && ProcFindTextures::check_bool(&self.dx10, prop.dxgi_format != 0)
            && ProcFindTextures::check_bool(&self.xbox, prop.xbox))
        {
            return Ok(Vec::new());
        }

        if self.report_only {
            let mut log = vec![file.file_name.clone(), report_line(&prop)];
            if self.header_dump && !ba2_dds {
                let (mut tree, root) = create_misc_file(MiscFile::Dds)?;
                tree.load_from_data(root, &buf[..len])?;
                log.extend(header_dump(&mut tree, root)?);
            }
            log.push(String::new());
            ctx.add_messages(log);
            return Ok(Vec::new());
        }

        if ba2_dds {
            buf = file.get_data()?;
        } else if !in_archive {
            let path = format!("{}{}", file.input.input_directory, file.file_name);
            let data =
                std::fs::read(&path).map_err(|error| DfError::new(format!("Cannot open file \"{path}\". {error}")))?;
            buf = data[..(prop.size as usize).min(data.len())].to_vec();
        }
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties() -> Properties {
        Properties {
            dxgi_format: 71,
            format_name: "BC1_UNORM".to_owned(),
            width: 256,
            height: 128,
            resolution: 256,
            size: 32768,
            bits_per_pixel: 4,
            mip_maps: true,
            compressed: true,
            alpha: false,
            cube_map: false,
            xbox: false,
        }
    }

    #[test]
    fn the_report_line_is_formatted() {
        let mut prop = properties();
        assert_eq!(
            report_line(&prop),
            "\tWidth: 0256  Height: 0128  Size: 32 KB    4 Bit  BC1_UNORM    MipMaps"
        );
        prop.width = 1024;
        prop.height = 1024;
        prop.size = 1_048_576;
        prop.mip_maps = false;
        prop.alpha = true;
        prop.cube_map = true;
        assert_eq!(
            report_line(&prop),
            "\tWidth: 1024  Height: 1024  Size: 1 MB    4 Bit  BC1_UNORM    Alpha  CubeMap"
        );
    }

    #[test]
    fn check_bool_reads_yes_no_and_empty() {
        assert!(ProcFindTextures::check_bool("", false));
        assert!(ProcFindTextures::check_bool("Yes", true));
        assert!(!ProcFindTextures::check_bool("Yes", false));
        assert!(ProcFindTextures::check_bool("No", false));
        assert!(!ProcFindTextures::check_bool("Nope", true));
    }

    #[test]
    fn the_settings_take_their_branches() {
        let mut proc = ProcFindTextures::new();
        proc.on_show(&Storage::new("Findtextures".to_owned(), None));
        proc.on_start().unwrap();
        assert!(proc.report_only);
        assert!(proc.formats.is_empty());
        assert_eq!(proc.resolution, 0);
        assert_eq!(proc.bits_per_pixel, 0);

        // `>= 256`, `Not power of 2` and a broken list of formats.
        proc.resolution_text = ">= 256".to_owned();
        proc.on_start().unwrap();
        assert_eq!(proc.resolution, 256);
        proc.resolution_text = "< 512".to_owned();
        proc.on_start().unwrap();
        assert_eq!(proc.resolution, -512);
        proc.resolution_text = "Not power of 2".to_owned();
        proc.on_start().unwrap();
        assert_eq!(proc.resolution, 1);
        proc.resolution_text = String::new();

        proc.formats_text = "71,77".to_owned();
        proc.bits_per_pixel_text = "8".to_owned();
        proc.on_start().unwrap();
        assert_eq!(proc.formats.iter().copied().collect::<Vec<i32>>(), vec![71, 77]);
        assert_eq!(proc.bits_per_pixel, 8);
        // A value that is not a number drops the rest of the list.
        proc.formats_text = "71,x,77".to_owned();
        proc.on_start().unwrap();
        assert_eq!(proc.formats.iter().copied().collect::<Vec<i32>>(), vec![71]);
    }
}
