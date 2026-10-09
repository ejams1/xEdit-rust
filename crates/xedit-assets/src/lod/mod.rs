// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbLOD.pas

//! LOD generation as xEdit's LODGen mode does it: the distant LOD files of
//! Oblivion (`.lod` and `.cmp`), the tree LOD of Skyrim and the Fallouts
//! (the billboard atlas, the `.lst` list and the `.btt` or `.dtl` blocks),
//! and the objects LOD of Skyrim, the Fallouts and Fallout 4: the export
//! file for `LODGenx64.exe` with every reference that gets LOD, the atlas of
//! the LOD textures with its map, and the run of `LODGenx64.exe`, which
//! writes the meshes. The external programs (`LODGenx64.exe`, and
//! `Texconvx64.exe` for the textures the imaging library can not read) are
//! the ones of xEdit's `Edit Scripts` folder, as upstream runs them.
//!
//! The textures go through the port of the Vampyre Imaging Library
//! (`crate::imaging`), as upstream's do.

mod atlas;
mod generate;
mod trees;

pub use atlas::{
    BinBlock, BinPacker, SourceAtlasTexture, build_atlas, build_atlas_from_atlas_map, build_atlas_from_textures_list,
    get_uv_range_textures_list, prepare_image_alpha,
};
pub use generate::{generate_lod_fo4, generate_lod_tes4, generate_lod_tes5, split_tree_lod, worldspaces_for_lod};
pub use trees::{LodSettings, TreeBlock, TreeList, TreeRef, TreeType};

use std::cmp::Ordering;

use xedit_core::interface::globals::{app_name, is_fallout3, is_fallout4, is_oblivion, is_skyrim};
use xedit_core::interface::misc::progress;

use crate::imaging::{ImageData, ImageFormat};
use crate::sniff::processor::MemIniFile;

/// `sLODGenName`.
pub const LODGEN_NAME: &str = "LODGenx64.exe";
/// `sTexconv`.
pub const TEXCONV: &str = "Texconvx64.exe";
/// `iBillboardFlag`: marks a texture of the list as a tree billboard.
pub const BILLBOARD_FLAG: u64 = 4096;

/// `BooleanText`.
pub fn boolean_text(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

/// The defaults of the atlas options (`iDefaultAtlasWidth` and the rest),
/// which the LODGen form sets for Fallout 4 and Starfield.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodDefaults {
    pub atlas_width: i32,
    pub atlas_height: i32,
    pub uv_range: f32,
    pub atlas_diffuse_format: ImageFormat,
    pub atlas_normal_format: ImageFormat,
    pub atlas_specular_format: ImageFormat,
    pub alpha_threshold: i32,
}

impl LodDefaults {
    /// The unit's initial values.
    pub const fn initial() -> LodDefaults {
        LodDefaults {
            atlas_width: 2048,
            atlas_height: 2048,
            uv_range: 1.5,
            atlas_diffuse_format: ImageFormat::Dxt3,
            atlas_normal_format: ImageFormat::Dxt1,
            atlas_specular_format: ImageFormat::Ati2n,
            alpha_threshold: 128,
        }
    }
}

/// What the generator works with that upstream keeps in globals: the
/// folders (`wbDataPath`, `wbOutputPath`, `wbScriptsPath`, `wbTempPath`, each
/// with its trailing backslash), the settings file of the LODGen options
/// (`Settings`), and the defaults of the options.
pub struct LodEnv {
    pub data_path: String,
    pub output_path: String,
    pub scripts_path: String,
    pub temp_path: String,
    pub settings: MemIniFile,
    pub defaults: LodDefaults,
}

impl LodEnv {
    /// The section of the LODGen options, `<APP> LOD Options`.
    pub fn section(&self) -> String {
        format!("{} LOD Options", app_name())
    }

    pub fn read_string(&self, section: &str, ident: &str, default: &str) -> String {
        self.settings.read_string(section, ident, default)
    }

    pub fn read_integer(&self, section: &str, ident: &str, default: i32) -> i32 {
        self.settings.read_integer(section, ident, default)
    }

    pub fn read_bool(&self, section: &str, ident: &str, default: bool) -> bool {
        self.settings.read_bool(section, ident, default)
    }
}

/// A failure of the generator: an exception and its message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct LodError(pub String);

impl LodError {
    pub fn new(message: impl Into<String>) -> LodError {
        LodError(message.into())
    }
}

impl From<crate::imaging::ImagingError> for LodError {
    fn from(error: crate::imaging::ImagingError) -> Self {
        LodError(error.0)
    }
}

impl From<std::io::Error> for LodError {
    fn from(error: std::io::Error) -> Self {
        LodError(error.to_string())
    }
}

pub type LodResult<T> = Result<T, LodError>;

/// `wbProgressCallback`. The GUI's callback (`GeneralProgress`) drops an
/// empty message, which the generator sends at times.
pub fn message(text: &str) {
    if !text.is_empty() {
        progress(text);
    }
}

/// `wbLODExtraOptionsFileName`.
pub fn lod_extra_options_file_name(plugin_name: &str, worldspace_id: &str) -> String {
    format!("{}LODGen_{plugin_name}_{worldspace_id}_Options.txt", app_name())
}

/// `wbLODSettingsFileName`.
pub fn lod_settings_file_name(worldspace_id: &str) -> String {
    if is_oblivion() {
        String::new()
    } else if is_fallout3() {
        format!("lodsettings\\{worldspace_id}.dlodsettings")
    } else {
        format!("lodsettings\\{worldspace_id}.lod")
    }
}

/// `wbLODTreeBlockFileExt`.
pub fn lod_tree_block_file_ext() -> &'static str {
    if is_skyrim() {
        "btt"
    } else if is_fallout3() {
        "dtl"
    } else {
        ""
    }
}

/// `wbDefaultNormalTexture`.
pub fn default_normal_texture() -> &'static str {
    if is_fallout4() {
        "textures\\shared\\flatflat_n.dds"
    } else if is_skyrim() {
        "textures\\default_n.dds"
    } else if is_fallout3() {
        "textures\\shared\\shadefade01_n.dds"
    } else {
        ""
    }
}

/// `wbDefaultSpecularTexture`.
pub fn default_specular_texture() -> &'static str {
    if is_fallout4() {
        "textures\\shared\\white01_s.dds"
    } else {
        ""
    }
}

/// `wbLoadImageFromMemory`: the image of a texture, or of a DDS file the
/// library can not read converted to `R32G32B32A32_FLOAT` by
/// `Texconvx64.exe` in the temporary folder.
pub fn load_image_from_memory(env: &LodEnv, data: &[u8]) -> Option<ImageData> {
    if let Ok(Some(image)) = crate::imaging::load_image_from_memory(data) {
        return Some(image);
    }
    if !(data.len() > 4 && &data[..4] == b"DDS ") {
        return None;
    }
    let name = format!("{}{}.dds", env.temp_path, guid_file_name());
    if std::fs::create_dir_all(&env.temp_path).is_err() {
        return None;
    }
    std::fs::write(&name, data).ok()?;
    let command = format!(
        "\"{}{TEXCONV}\" -nologo -y -f R32G32B32A32_FLOAT -o \"{}\" \"{name}\"",
        env.scripts_path,
        env.temp_path.trim_end_matches('\\')
    );
    let result = match xedit_core::helpers::execute_capture_console_output(&command) {
        Ok(0) if std::path::Path::new(&name).is_file() => std::fs::read(&name)
            .ok()
            .and_then(|bytes| load_image_from_memory(env, &bytes)),
        _ => None,
    };
    let _ = std::fs::remove_file(&name);
    result
}

/// `TPath.GetGUIDFileName`: a new GUID as 32 hexadecimal digits.
fn guid_file_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos() as u64);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{time:016X}{:08X}{count:08X}", std::process::id())
}

/// The comparison of a `TStringList`: the user's locale without case
/// (`AnsiCompareText`), or `CompareText` (ASCII without case) for a
/// `TwbFastStringList` (`UseLocale` off).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListCompare {
    Locale,
    Ascii,
}

/// Delphi `CompareText`: ordinal after upper-casing the ASCII letters.
pub fn compare_text(a: &str, b: &str) -> Ordering {
    let up = |s: &str| -> Vec<u16> {
        s.encode_utf16()
            .map(|unit| {
                if (b'a' as u16..=b'z' as u16).contains(&unit) {
                    unit - 32
                } else {
                    unit
                }
            })
            .collect()
    };
    up(a).cmp(&up(b))
}

/// A `TStringList` with what the unit uses of it: sorted or not, duplicates
/// ignored or kept, and an object per string.
#[derive(Debug, Clone)]
pub struct StringList {
    pub items: Vec<(String, u64)>,
    pub sorted: bool,
    pub ignore_duplicates: bool,
    pub compare: ListCompare,
}

impl StringList {
    pub fn new() -> StringList {
        StringList {
            items: Vec::new(),
            sorted: false,
            ignore_duplicates: false,
            compare: ListCompare::Locale,
        }
    }

    /// A sorted list that ignores duplicates.
    pub fn sorted() -> StringList {
        StringList {
            sorted: true,
            ignore_duplicates: true,
            ..StringList::new()
        }
    }

    fn cmp(&self, a: &str, b: &str) -> Ordering {
        match self.compare {
            ListCompare::Locale => xedit_io::encoding::ansi_compare_text(a, b),
            ListCompare::Ascii => compare_text(a, b),
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, index: usize) -> &str {
        &self.items[index].0
    }

    pub fn strings(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|(s, _)| s.as_str())
    }

    /// `Find`: whether the string is there and where it is or would go.
    fn find(&self, s: &str) -> (bool, usize) {
        let mut found = false;
        let (mut l, mut h) = (0i64, self.items.len() as i64 - 1);
        while l <= h {
            let i = (l + h) >> 1;
            let c = self.cmp(&self.items[i as usize].0, s);
            if c == Ordering::Less {
                l = i + 1;
            } else {
                h = i - 1;
                if c == Ordering::Equal {
                    found = true;
                    if self.ignore_duplicates {
                        l = i;
                    }
                }
            }
        }
        (found, l as usize)
    }

    /// `Add` and `AddObject`: the index of the string.
    pub fn add_object(&mut self, s: &str, object: u64) -> Option<usize> {
        if self.sorted {
            let (found, index) = self.find(s);
            if found && self.ignore_duplicates {
                return None;
            }
            self.items.insert(index, (s.to_owned(), object));
            Some(index)
        } else {
            self.items.push((s.to_owned(), object));
            Some(self.items.len() - 1)
        }
    }

    pub fn add(&mut self, s: &str) -> Option<usize> {
        self.add_object(s, 0)
    }

    /// `IndexOf`.
    pub fn index_of(&self, s: &str) -> Option<usize> {
        if self.sorted {
            let (found, index) = self.find(s);
            found.then_some(index)
        } else {
            self.items
                .iter()
                .position(|(item, _)| self.cmp(item, s) == Ordering::Equal)
        }
    }

    /// `IndexOfObject`.
    pub fn index_of_object(&self, object: u64) -> Option<usize> {
        self.items.iter().position(|(_, item)| *item == object)
    }

    pub fn delete(&mut self, index: usize) {
        self.items.remove(index);
    }

    /// `Sorted := True` of an unsorted list: `TStringList.QuickSort`.
    pub fn sort(&mut self) {
        if self.items.len() > 1 {
            let last = self.items.len() - 1;
            self.quick_sort(0, last);
        }
        self.sorted = true;
    }

    fn quick_sort(&mut self, mut l: usize, r: usize) {
        loop {
            let mut i = l as i64;
            let mut j = r as i64;
            let mut p = ((l + r) >> 1) as i64;
            loop {
                while self.cmp(&self.items[i as usize].0, &self.items[p as usize].0) == Ordering::Less {
                    i += 1;
                }
                while self.cmp(&self.items[j as usize].0, &self.items[p as usize].0) == Ordering::Greater {
                    j -= 1;
                }
                if i <= j {
                    if i != j {
                        self.items.swap(i as usize, j as usize);
                    }
                    if p == i {
                        p = j;
                    } else if p == j {
                        p = i;
                    }
                    i += 1;
                    j -= 1;
                }
                if i > j {
                    break;
                }
            }
            if (l as i64) < j {
                self.quick_sort(l, j as usize);
            }
            l = i as usize;
            if i >= r as i64 {
                break;
            }
        }
    }

    /// `Text`: each string with CRLF.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for (s, _) in &self.items {
            text.push_str(s);
            text.push_str("\r\n");
        }
        text
    }

    /// `SaveToFile`: the text in the ANSI code page.
    pub fn save_to_file(&self, path: &str) -> std::io::Result<()> {
        std::fs::write(path, xedit_io::encoding::ansi_bytes(&self.text()))
    }

    /// `LoadFromFile`: the lines of a file (with a byte order mark, else
    /// ANSI).
    pub fn load_from_file(path: &str) -> std::io::Result<StringList> {
        let bytes = std::fs::read(path)?;
        let text = xedit_io::encoding::string_list_text(&bytes);
        let mut list = StringList::new();
        for line in crate::sniff::processor::string_list_lines(&text) {
            list.add(&line);
        }
        Ok(list)
    }
}

impl Default for StringList {
    fn default() -> Self {
        StringList::new()
    }
}

/// `TStringList.DelimitedText` with `StrictDelimiter`: the parts between
/// the delimiters, a quoted part unquoted.
pub fn delimited_text(text: &str, delimiter: char) -> Vec<String> {
    crate::sniff::processor::delimited_text(text, delimiter)
}

/// Delphi `Trim`.
pub fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| c <= ' ')
}

/// Delphi `ExtractFileName`: after the last backslash or colon.
pub fn extract_file_name(path: &str) -> &str {
    match path.rfind(['\\', ':']) {
        Some(index) => &path[index + 1..],
        None => path,
    }
}

/// Delphi `ExtractFilePath`.
pub fn extract_file_path(path: &str) -> &str {
    match path.rfind(['\\', ':']) {
        Some(index) => &path[..index + 1],
        None => "",
    }
}

/// Delphi `ExtractFileExt`.
pub fn extract_file_ext(path: &str) -> &str {
    match path.rfind(['.', '\\', ':']) {
        Some(index) if path[index..].starts_with('.') => &path[index..],
        _ => "",
    }
}

/// Delphi `ChangeFileExt`.
pub fn change_file_ext(path: &str, extension: &str) -> String {
    match path.rfind(['.', '\\', ':']) {
        Some(index) if path[index..].starts_with('.') => format!("{}{extension}", &path[..index]),
        _ => format!("{path}{extension}"),
    }
}

/// Delphi `ForceDirectories` of a path (the folder part of a file name
/// with its trailing backslash).
pub fn force_directories(path: &str) -> bool {
    let path = path.trim_end_matches('\\');
    path.is_empty() || std::fs::create_dir_all(path).is_ok()
}

/// Delphi `MatchesMask` without case: `*`, `?` and `[set]`.
pub fn matches_mask(name: &str, mask: &str) -> bool {
    fn matches(name: &[char], mask: &[char]) -> bool {
        match mask.first() {
            None => name.is_empty(),
            Some('*') => (0..=name.len()).any(|skip| matches(&name[skip..], &mask[1..])),
            Some('?') => !name.is_empty() && matches(&name[1..], &mask[1..]),
            Some('[') => {
                let Some(end) = mask.iter().position(|&c| c == ']') else {
                    return !name.is_empty() && name[0] == '[' && matches(&name[1..], &mask[1..]);
                };
                let Some(&c) = name.first() else {
                    return false;
                };
                let set = &mask[1..end];
                let (negate, set) = match set.first() {
                    Some('!') => (true, &set[1..]),
                    _ => (false, set),
                };
                let mut hit = false;
                let mut i = 0;
                while i < set.len() {
                    if i + 2 < set.len() && set[i + 1] == '-' {
                        hit |= (set[i]..=set[i + 2]).contains(&c);
                        i += 3;
                    } else {
                        hit |= set[i] == c;
                        i += 1;
                    }
                }
                hit != negate && matches(&name[1..], &mask[end + 1..])
            }
            Some(&m) => !name.is_empty() && name[0] == m && matches(&name[1..], &mask[1..]),
        }
    }
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let mask: Vec<char> = mask.to_lowercase().chars().collect();
    matches(&name, &mask)
}

/// Delphi's `RandSeed`: process-wide. Upstream seeds it from the clock at
/// startup (`Randomize` in the initialization of `xeTipForm`), so the tree
/// rotations differ from run to run.
static RAND_SEED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Sets `RandSeed`.
pub fn set_rand_seed(seed: u32) {
    RAND_SEED.store(seed, std::sync::atomic::Ordering::Relaxed);
}

/// `Randomize`: a `RandSeed` from the clock (Delphi takes the low 32 bits
/// of the performance counter); returns it.
pub fn randomize() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let seed = nanos as u32;
    set_rand_seed(seed);
    seed
}

/// Delphi `Random`: a double in [0, 1) of the next `RandSeed`.
pub fn random() -> f64 {
    let seed = RAND_SEED
        .load(std::sync::atomic::Ordering::Relaxed)
        .wrapping_mul(0x0808_8405)
        .wrapping_add(1);
    RAND_SEED.store(seed, std::sync::atomic::Ordering::Relaxed);
    f64::from(seed) * (1.0 / 4_294_967_296.0)
}

/// The resources of a file: from the last container that has it
/// (`OpenResource` and `High(res)`).
pub fn open_resource(name: &str) -> Option<Vec<u8>> {
    xedit_core::container_handler::open_resource_last(name)
}

/// `ResourceExists`.
pub fn resource_exists(name: &str) -> bool {
    xedit_core::container_handler::resource_exists(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delphi_random_sequence() {
        set_rand_seed(0);
        assert_eq!(random(), f64::from(1u32) / 4_294_967_296.0);
        assert_eq!(random(), f64::from(0x0808_8406u32) / 4_294_967_296.0);
        set_rand_seed(0);
    }

    #[test]
    fn masks_match_without_case() {
        assert!(matches_mask("materials\\lod\\a_lod.bgsm", "materials\\LOD\\*.bgsm"));
        assert!(matches_mask("abc", "a?c"));
        assert!(!matches_mask("abd", "a[bc]c"));
        assert!(matches_mask("abc", "a[a-c]c"));
    }

    #[test]
    fn sorted_lists_ignore_duplicates() {
        let mut list = StringList::sorted();
        list.compare = ListCompare::Ascii;
        list.add("b");
        list.add("A");
        list.add("a");
        list.add("C");
        assert_eq!(list.strings().collect::<Vec<_>>(), ["A", "b", "C"]);
        assert_eq!(list.index_of("B"), Some(1));
        let mut unsorted = StringList::new();
        unsorted.compare = ListCompare::Ascii;
        for s in ["d", "B", "a", "c"] {
            unsorted.add(s);
        }
        unsorted.sort();
        assert_eq!(unsorted.strings().collect::<Vec<_>>(), ["a", "B", "c", "d"]);
    }

    #[test]
    fn file_names() {
        assert_eq!(change_file_ext("meshes\\a.b\\c.nif", ""), "meshes\\a.b\\c");
        assert_eq!(change_file_ext("meshes\\a.b\\c", "_lod.nif"), "meshes\\a.b\\c_lod.nif");
        assert_eq!(extract_file_name("Landscape\\Trees\\Palm.NIF"), "Palm.NIF");
        assert_eq!(extract_file_ext("x\\y.dds"), ".dds");
    }
}
