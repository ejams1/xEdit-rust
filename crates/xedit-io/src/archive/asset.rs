// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbBSArchive.pas (TwbAsset)

//! `TwbAsset`: which kind of asset a path is, and how a file path on disk
//! becomes the asset name stored in an archive.

use crate::encoding::lower_case;

/// Upstream `TwbAssetType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetType {
    None,
    Mesh,
    Texture,
    Material,
    Geometry,
    Sound,
    Voice,
    Music,
    Script,
    Source,
    SourceSse,
    Strings,
    SpeedTree,
    Video,
    LodSettings,
    DistantLod,
    Interface,
    Program,
    Menus,
    Font,
    Facegen,
    LsData,
    Shaders,
    ShadersFx,
    Grass,
    PreVis,
    Seq,
    DialogueViews,
    BookArt,
    Icon,
    Splash,
}

/// Upstream `TAssetDesc`: an asset type with its root folder and extensions.
struct AssetDesc {
    kind: AssetType,
    root: &'static str,
    ext: &'static [&'static str],
}

/// Upstream `cDataFolders`.
const DATA_FOLDERS: [&str; 2] = ["data", "data files"];

const fn asset(kind: AssetType, root: &'static str, ext: &'static [&'static str]) -> AssetDesc {
    AssetDesc { kind, root, ext }
}

/// Upstream `cBSAssets`: the order matters, the first match wins.
const BS_ASSETS: [AssetDesc; 30] = [
    asset(
        AssetType::Mesh,
        "meshes",
        &[
            ".nif", ".kf", ".kfm", ".egm", ".egt", ".tri", ".psa", ".hkt", ".hkx", ".ssf", ".btr", ".bto", ".btt",
            ".dtl",
        ],
    ),
    asset(AssetType::Texture, "textures", &[".dds", ".tga", ".png"]),
    asset(AssetType::Material, "materials", &[".bgsm", ".bgem"]),
    asset(AssetType::Geometry, "geometries", &[".mesh"]),
    asset(
        AssetType::Voice,
        "sound\\voice",
        &[".lip", ".wav", ".xwm", ".mp3", ".ogg", ".fuz"],
    ),
    asset(AssetType::Sound, "sound", &[".wav", ".xwm", ".ogg"]),
    asset(AssetType::Music, "music", &[".xwm", ".mp3"]),
    asset(AssetType::Source, "scripts\\source", &[".psc"]),
    asset(AssetType::SourceSse, "source\\scripts", &[".psc"]),
    asset(AssetType::Script, "scripts", &[".pex", ".psc"]),
    asset(AssetType::Strings, "strings", &[".strings", ".ilstrings", ".dlstrings"]),
    asset(AssetType::SpeedTree, "trees", &[".spt"]),
    asset(AssetType::Video, "video", &[".bik", ".bk2"]),
    asset(
        AssetType::LodSettings,
        "lodsettings",
        &[".lodsettings", ".dlodsettings", ".lod"],
    ),
    asset(AssetType::DistantLod, "distantlod", &[".cmp", ".lod"]),
    asset(AssetType::Interface, "interface", &[".swf", ".png", ".txt"]),
    asset(AssetType::Program, "programs", &[".swf"]),
    asset(AssetType::Menus, "menus", &[".xml", ".htm", ".txt", ".scc", ".bat"]),
    asset(AssetType::Font, "fonts", &[".fnt", ".tex"]),
    asset(AssetType::Facegen, "facegen", &[".ctl"]),
    asset(AssetType::LsData, "lsdata", &[".dat"]),
    asset(AssetType::Shaders, "shaders", &[".sdp"]),
    asset(AssetType::ShadersFx, "shadersfx", &[".fxp"]),
    asset(AssetType::Grass, "grass", &[".gid"]),
    asset(AssetType::PreVis, "vis", &[".uvd"]),
    asset(AssetType::Seq, "seq", &[".seq"]),
    asset(AssetType::DialogueViews, "dialogueviews", &[".xml"]),
    asset(AssetType::BookArt, "bookart", &[".dds", ".tga"]),
    asset(AssetType::Icon, "icons", &[".dds", ".tga"]),
    asset(AssetType::Splash, "splash", &[".dds", ".tga"]),
];

/// Upstream `cSkippedExtensions`: files `AddSourceFolder` leaves out.
const SKIPPED_EXTENSIONS: [&str; 29] = [
    ".bsa",
    ".ba2",
    ".esm",
    ".esp",
    ".esl",
    ".nam",
    ".sdp",
    ".cdx",
    ".csg",
    ".override",
    ".ghost",
    ".exe",
    ".dll",
    ".pdb",
    ".bak",
    ".db",
    ".psd",
    ".jpg",
    ".jpeg",
    ".3ds",
    ".max",
    ".blend",
    ".obj",
    ".xlsx",
    ".docx",
    ".7z",
    ".zip",
    ".rar",
    ".tmp",
];

/// Upstream `TAssetParts`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetParts {
    pub folder: String,
    pub folder_no_delimiter: String,
    pub file_name: String,
    pub file_name_no_extension: String,
    pub extension: String,
    pub extension_no_dot: String,
}

/// Delphi `Copy` on a character slice: `start` is 1-based and the range is
/// clamped to the string.
fn copy(chars: &[char], start: usize, count: usize) -> String {
    let from = start.saturating_sub(1).min(chars.len());
    let to = from.saturating_add(count).min(chars.len());
    chars[from..to].iter().collect()
}

/// Port of `TwbAsset.LastCharPos`: the 1-based position of the last `c`, 0
/// when there is none.
pub fn last_char_pos(text: &str, c: char) -> usize {
    text.chars()
        .enumerate()
        .filter(|&(_, x)| x == c)
        .last()
        .map_or(0, |(at, _)| at + 1)
}

/// Port of `TwbAsset.SplitDirName`: the position of the last separator
/// (`\`, else `/`; 0 for none), the folder in front of it and the name after.
pub fn split_dir_name(file_name: &str) -> (usize, String, String) {
    let chars: Vec<char> = file_name.chars().collect();
    let mut position = last_char_pos(file_name, '\\');
    if position == 0 {
        position = last_char_pos(file_name, '/');
    }
    if position != 0 {
        (
            position,
            copy(&chars, 1, position - 1),
            copy(&chars, position + 1, chars.len() - position),
        )
    } else {
        (0, String::new(), file_name.to_owned())
    }
}

/// Port of `TwbAsset.SplitNameExt`: the position of the last dot (0 for
/// none), the name in front of it and the extension (with the dot unless
/// `no_ext_dot`).
pub fn split_name_ext(file_name: &str, no_ext_dot: bool) -> (usize, String, String) {
    let chars: Vec<char> = file_name.chars().collect();
    let mut position = last_char_pos(file_name, '.');
    if position != 0 {
        let name = copy(&chars, 1, position - 1);
        if no_ext_dot {
            position += 1;
        }
        let extension = copy(&chars, position, (chars.len() + 1).saturating_sub(position));
        (position, name, extension)
    } else {
        (0, file_name.to_owned(), String::new())
    }
}

/// Port of `TwbAsset.Split`.
pub fn split(file_name: &str) -> AssetParts {
    let (_, folder_no_delimiter, name) = split_dir_name(file_name);
    let (_, without_extension, extension_no_dot) = split_name_ext(&name, true);
    AssetParts {
        folder: format!("{folder_no_delimiter}\\"),
        folder_no_delimiter,
        file_name: name,
        file_name_no_extension: without_extension,
        extension: format!(".{extension_no_dot}"),
        extension_no_dot,
    }
}

/// Port of `TwbAsset.AssetTypeByFolder`.
pub fn asset_type_by_folder(asset_name: &str) -> AssetType {
    let lowered = lower_case(asset_name);
    for asset in &BS_ASSETS {
        if lowered == asset.root || lowered.starts_with(&format!("{}\\", asset.root)) {
            return asset.kind;
        }
    }
    AssetType::None
}

/// Port of `TwbAsset.AssetTypeByExtension`.
pub fn asset_type_by_extension(asset_name: &str) -> AssetType {
    let (_, _, extension) = split_name_ext(asset_name, false);
    let extension = lower_case(&extension);
    for asset in &BS_ASSETS {
        if asset.ext.contains(&extension.as_str()) {
            return asset.kind;
        }
    }
    AssetType::None
}

/// Port of `TwbAsset.GetAssetName`: the name a file gets inside an archive,
/// found from where the data folder or a known asset folder starts in the
/// path, else from `root`, else from the asset type of the extension.
pub fn get_asset_name(file_name: &str, root: &str, mut asset_type: AssetType) -> String {
    let chars: Vec<char> = file_name.chars().collect();
    // Skip too short, empty and beth slop.
    if chars.len() < 2 || file_name.trim().is_empty() || file_name.contains("\u{8}NOR") {
        return String::new();
    }

    let path: Vec<char> = std::iter::once('\\')
        .chain(lower_case(file_name).replace('/', "\\").chars())
        .collect();
    // The 1-based position of the first occurrence, 0 for none.
    let find = |needle: &str| -> usize {
        let needle: Vec<char> = needle.chars().collect();
        path.windows(needle.len())
            .position(|window| window == needle.as_slice())
            .map_or(0, |at| at + 1)
    };

    let result = 'found: {
        // Searching for the Data folder first.
        for folder in DATA_FOLDERS {
            let i = find(&format!("\\{folder}\\"));
            if i != 0 {
                break 'found copy(&chars, i + folder.chars().count() + 1, chars.len());
            }
        }

        // Searching for known asset folders.
        for asset in &BS_ASSETS {
            if asset_type != AssetType::None && asset.kind != asset_type {
                continue;
            }
            let i = find(&format!("\\{}\\", asset.root));
            if i != 0 {
                break 'found copy(&chars, i, chars.len());
            }
        }

        // If a root folder is provided use it.
        if !root.is_empty() {
            let prefix = include_trailing_path_delimiter(root);
            break 'found copy(&chars, prefix.chars().count() + 1, chars.len());
        }

        // Last resort: detect the asset type by the extension.
        if asset_type == AssetType::None {
            asset_type = asset_type_by_extension(&path.iter().collect::<String>());
            // Priority goes to sound over voice and music (audio uses the
            // same extensions mostly).
            if matches!(asset_type, AssetType::Voice | AssetType::Music) {
                asset_type = AssetType::Sound;
            }
            // Unknowns go into meshes by default.
            if asset_type == AssetType::None {
                asset_type = AssetType::Mesh;
            }
        }

        // Prepend with the asset type root.
        for asset in &BS_ASSETS {
            if asset.kind == asset_type {
                // Use the file name only from absolute paths.
                break 'found if is_path_rooted(file_name) {
                    format!("{}\\{}", asset.root, extract_file_name(file_name))
                } else {
                    format!("{}\\{}", asset.root, file_name)
                };
            }
        }
        String::new()
    };
    result.replace('/', "\\").replace("\\\\", "\\")
}

/// Delphi `IncludeTrailingPathDelimiter`.
pub fn include_trailing_path_delimiter(path: &str) -> String {
    if path.is_empty() || path.ends_with('\\') || path.ends_with('/') {
        path.to_owned()
    } else {
        format!("{path}{}", std::path::MAIN_SEPARATOR)
    }
}

/// `TPath.IsPathRooted`: a drive letter or a leading separator.
fn is_path_rooted(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with(['\\', '/']) || (bytes.len() >= 2 && bytes[1] == b':')
}

/// Delphi `ExtractFileName`: the part after the last `\`, `/` or `:`.
pub fn extract_file_name(path: &str) -> &str {
    path.rfind(['\\', '/', ':']).map_or(path, |at| &path[at + 1..])
}

/// Delphi `ExtractFileExt`: the last dot of the file name and what follows.
pub fn extract_file_ext(path: &str) -> &str {
    let name = extract_file_name(path);
    name.rfind('.').map_or("", |at| &name[at..])
}

/// Port of `TwbAsset.DoNotPack`: files `AddSourceFolder` leaves out.
pub fn do_not_pack(file_name: &str) -> bool {
    let extension = lower_case(extract_file_ext(file_name));
    SKIPPED_EXTENSIONS.contains(&extension.as_str())
}

/// Port of `TwbAsset.DoNotCompress`: sounds, voices, music and strings are
/// stored as they are, except `.fuz` and `.hkx` files.
pub fn do_not_compress(file_name: &str) -> bool {
    let lowered = lower_case(file_name);
    matches!(
        asset_type_by_folder(file_name),
        AssetType::Sound | AssetType::Voice | AssetType::Music | AssetType::Strings
    ) && !lowered.ends_with(".fuz")
        && !lowered.ends_with(".hkx")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_directories_and_extensions() {
        assert_eq!(
            split_dir_name("meshes\\armor\\a.nif"),
            (13, "meshes\\armor".to_owned(), "a.nif".to_owned())
        );
        assert_eq!(split_dir_name("a.nif"), (0, String::new(), "a.nif".to_owned()));
        assert_eq!(split_dir_name("meshes/a.nif").1, "meshes");
        let parts = split("meshes\\armor\\a.b.nif");
        assert_eq!(parts.folder, "meshes\\armor\\");
        assert_eq!(parts.file_name_no_extension, "a.b");
        assert_eq!(parts.extension_no_dot, "nif");
        assert_eq!(parts.extension, ".nif");
        let parts = split("meshes\\readme");
        assert_eq!(parts.extension_no_dot, "");
        assert_eq!(parts.extension, ".");
    }

    #[test]
    fn asset_types_come_from_the_folder_then_the_extension() {
        assert_eq!(asset_type_by_folder("Sound\\Voice\\x.wav"), AssetType::Voice);
        assert_eq!(asset_type_by_folder("sound\\fx\\x.wav"), AssetType::Sound);
        assert_eq!(asset_type_by_folder("sounds\\x.wav"), AssetType::None);
        assert_eq!(asset_type_by_extension("foo.NIF"), AssetType::Mesh);
        assert_eq!(asset_type_by_extension("foo"), AssetType::None);
    }

    #[test]
    fn asset_names_start_at_the_data_folder_or_a_known_folder() {
        let name = |path: &str, root: &str| get_asset_name(path, root, AssetType::None);
        assert_eq!(name("C:\\Games\\Data\\Meshes\\a.nif", ""), "Meshes\\a.nif");
        assert_eq!(name("C:\\mods\\x\\Textures\\y\\a.dds", ""), "Textures\\y\\a.dds");
        assert_eq!(name("C:\\mods\\x\\y\\a.dds", "C:\\mods\\x"), "y\\a.dds");
        // The first matching folder of the list wins: meshes before textures.
        assert_eq!(name("C:\\t\\textures\\meshes\\a.dds", ""), "meshes\\a.dds");
        // Without a folder the extension chooses the root.
        assert_eq!(name("a.dds", ""), "textures\\a.dds");
        assert_eq!(name("C:\\x\\a.wav", ""), "sound\\a.wav");
        assert_eq!(name("a", ""), "");
    }

    #[test]
    fn files_to_skip_and_to_store() {
        assert!(do_not_pack("x\\Mod.ESP"));
        assert!(!do_not_pack("x\\a.nif"));
        assert!(do_not_compress("sound\\fx\\a.wav"));
        assert!(do_not_compress("sound\\voice\\a.lip"));
        assert!(!do_not_compress("sound\\voice\\a.fuz"));
        assert!(!do_not_compress("meshes\\a.nif"));
    }
}
