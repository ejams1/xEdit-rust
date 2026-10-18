// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: BSArch/frmMain.pas (mniLoadListClick,
// mniSaveListClick, mniAssetCompressClick, mniAssetEditClick,
// mniAssetReplaceClick, mniAssetRemoveSelectedClick,
// mniAssetRemoveUnselectedClick, mniAssetFindIdenticalClick, btnClearListClick,
// LoadList, SaveList, GetAssets, PreloadArchives, TAsset.ToJson/FromJson,
// FormCreate's defaults), BSArch/frmSearchReplace.pas (FormClose),
// Core/wbBSArchive.pas (TwbAsset.DoNotCompress, TwbSameData.Add,
// FormatSize)

//! `bsarchpro.*`: the asset list of BSArchPro (`TFormMain`). The list is
//! the `.bsarch` document the GUI's `LoadList` reads and `SaveList` writes:
//! a zip with an `assets.json` entry holding `Packing` and `Assets`, or
//! plain JSON (which `LoadList` also reads). Every command that changes the
//! list rewrites the document as `SaveList` writes it; the path has the
//! `.bsarch` extension as `mniSaveListClick` gives it. `list_load` and
//! `find_identical` only read.
//!
//! An asset is `TAsset.ToJson`'s array: the compressed flag, the asset
//! name, the source file name when it differs from the asset name, and the
//! source archive when there is one. The dialog's selection is indices into
//! that array.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as Json;
use xedit_io::archive::{Archive, do_not_compress};
use xedit_io::hash::lookup_hash;
use xedit_io::zip;

use crate::{CommandError, Registry, Session};

/// The entry `SaveList` writes with the list JSON (`TZipFile.Add(..., 'assets.json')`).
const ASSETS_JSON: &str = "assets.json";

/// `cBSArchExtension`: the extension `mniSaveListClick` and the dialogs use.
const LIST_EXTENSION: &str = ".bsarch";

/// Port of `TAsset`.
#[derive(Clone, Debug)]
pub struct Asset {
    /// `Compressed`: the asset is packed compressed.
    compressed: bool,
    /// `AssetName`: the name in the archive.
    asset_name: String,
    /// `FileName`: the source file (or the entry of `ArchiveName`).
    file_name: String,
    /// `ArchiveName`: the archive the source is in, empty for a loose file.
    archive_name: String,
}

impl Asset {
    /// `TAsset.FromJson`.
    fn from_json(value: &Json) -> Asset {
        let at = |index: usize| value.as_array().and_then(|array| array.get(index));
        let integer = |index: usize| at(index).and_then(Json::as_i64).unwrap_or(0);
        let text = |index: usize| at(index).and_then(Json::as_str).unwrap_or("").to_owned();
        let asset_name = text(1);
        let mut file_name = text(2);
        if file_name.is_empty() {
            file_name = asset_name.clone();
        }
        let archive_name = if value.as_array().is_some_and(|array| array.len() == 4) {
            text(3)
        } else {
            String::new()
        };
        Asset {
            compressed: integer(0) != 0,
            asset_name,
            file_name,
            archive_name,
        }
    }

    /// `TAsset.ToJson`.
    fn to_json(&self) -> Json {
        let mut array = vec![
            Json::from(if self.compressed { 1 } else { 0 }),
            Json::from(self.asset_name.clone()),
            Json::from(if self.file_name == self.asset_name {
                String::new()
            } else {
                self.file_name.clone()
            }),
        ];
        if !self.archive_name.is_empty() {
            array.push(Json::from(self.archive_name.clone()));
        }
        Json::Array(array)
    }

    /// `GetData`: the loose file, or the entry of the source archive.
    fn data(&self, archives: &mut HashMap<String, Archive>) -> Result<Vec<u8>, CommandError> {
        if self.archive_name.is_empty() {
            std::fs::read(&self.file_name)
                .map_err(|error| CommandError::new("io", format!("Error reading:\n{}\n\n{error}", self.file_name)))
        } else {
            if !archives.contains_key(&self.archive_name) {
                let archive = Archive::open(Path::new(&self.archive_name)).map_err(|error| {
                    CommandError::new("archive", format!("Error reading:\n{}\n\n{error}", self.file_name))
                })?;
                archives.insert(self.archive_name.clone(), archive);
            }
            archives[&self.archive_name]
                .unpack(&self.file_name)
                .map_err(|error| CommandError::new("archive", format!("Error reading:\n{}\n\n{error}", self.file_name)))
        }
    }

    /// `TAsset.FileNameDescr` as the identical-files report writes it
    /// (`ArchiveName + '\' + FileName`, or the file name).
    fn description(&self) -> String {
        if self.archive_name.is_empty() {
            self.file_name.clone()
        } else {
            format!("{}\\{}", self.archive_name, self.file_name)
        }
    }
}

impl Serialize for Asset {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Asset {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Asset::from_json(&Json::deserialize(deserializer)?))
    }
}

/// The folder of the program, with a trailing separator (`ExtractFilePath(ParamStr(0))`).
fn program_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_owned))
        .map(|dir| format!("{}\\", dir.display()))
        .unwrap_or_default()
}

fn default_archive_type() -> i64 {
    // `Integer(High(TwbBSArchiveType))`: baSFdds.
    8
}

fn default_negative_one() -> i64 {
    -1
}

fn default_archive_file_name() -> String {
    // `ExtractFilePath(ParamStr(0)) + 'New' + DefaultExtension(baSFdds)`.
    format!("{}New.ba2", program_path())
}

fn default_true() -> bool {
    true
}

/// Port of the packing settings of the form (`Packing` of the list JSON),
/// with `TFormMain.FormCreate`'s defaults for the fields a request leaves
/// out.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Packing {
    #[serde(rename = "ArchiveType", default = "default_archive_type")]
    pub archive_type: i64,
    #[serde(rename = "ArchiveTarget", default)]
    pub archive_target: i64,
    #[serde(rename = "ArchiveFileName", default = "default_archive_file_name")]
    pub archive_file_name: String,
    #[serde(rename = "ArchiveFlags", default)]
    pub archive_flags: i64,
    #[serde(rename = "FileFlags", default)]
    pub file_flags: i64,
    #[serde(rename = "AutodetectFlags", default = "default_true")]
    pub autodetect_flags: bool,
    #[serde(rename = "MultiThreaded", default = "default_true")]
    pub multi_threaded: bool,
    #[serde(rename = "SharedData", default = "default_true")]
    pub shared_data: bool,
    #[serde(rename = "Split", default = "default_negative_one")]
    pub split: i64,
    #[serde(rename = "CompressionType", default)]
    pub compression_type: String,
}

impl Default for Packing {
    fn default() -> Self {
        Packing {
            archive_type: default_archive_type(),
            archive_target: 0,
            archive_file_name: default_archive_file_name(),
            archive_flags: 0,
            file_flags: 0,
            autodetect_flags: true,
            multi_threaded: true,
            shared_data: true,
            split: -1,
            compression_type: String::new(),
        }
    }
}

impl Packing {
    /// `LoadList`'s reading of `Packing`: a key the object lacks gets the
    /// JSON default of its type (0, false, empty), except that a
    /// `baNone` archive type becomes the highest one and an absent `Split`
    /// is -1.
    fn from_json(value: &Json) -> Packing {
        let integer = |key: &str| value.get(key).and_then(Json::as_i64).unwrap_or(0);
        let boolean = |key: &str| value.get(key).and_then(Json::as_bool).unwrap_or(false);
        let text = |key: &str| value.get(key).and_then(Json::as_str).unwrap_or("").to_owned();
        let archive_type = integer("ArchiveType");
        Packing {
            archive_type: if archive_type == 0 {
                default_archive_type()
            } else {
                archive_type
            },
            archive_target: integer("ArchiveTarget"),
            archive_file_name: text("ArchiveFileName"),
            archive_flags: integer("ArchiveFlags"),
            file_flags: integer("FileFlags"),
            autodetect_flags: boolean("AutodetectFlags"),
            multi_threaded: boolean("MultiThreaded"),
            shared_data: boolean("SharedData"),
            split: if value.get("Split").is_some() {
                integer("Split")
            } else {
                -1
            },
            compression_type: text("CompressionType"),
        }
    }

    /// `SaveList`'s writing of `Packing`, in the form's order.
    fn to_json(&self) -> Json {
        serde_json::json!({
            "ArchiveType": self.archive_type,
            "ArchiveTarget": self.archive_target,
            "ArchiveFileName": self.archive_file_name,
            "ArchiveFlags": self.archive_flags,
            "FileFlags": self.file_flags,
            "AutodetectFlags": self.autodetect_flags,
            "MultiThreaded": self.multi_threaded,
            "SharedData": self.shared_data,
            "Split": self.split,
            "CompressionType": self.compression_type,
        })
    }
}

/// A loaded `.bsarch` list (`LoadList`).
struct Document {
    packing: Packing,
    assets: Vec<Asset>,
}

impl Document {
    fn to_json(&self) -> Json {
        serde_json::json!({
            "Packing": self.packing.to_json(),
            "Assets": self.assets.iter().map(Asset::to_json).collect::<Vec<_>>(),
        })
    }
}

fn change_extension(path: &str, extension: &str) -> String {
    // `TPath.ChangeExtension`: the extension of the last path element.
    let separator = path.rfind(['\\', '/']);
    match path
        .rfind('.')
        .filter(|&dot| separator.is_none_or(|separator| dot > separator))
    {
        Some(dot) => format!("{}{extension}", &path[..dot]),
        None => format!("{path}{extension}"),
    }
}

/// `mniSaveListClick`'s `SaveList(TPath.ChangeExtension(FileName, cBSArchExtension))`.
fn save_path(list: &str) -> String {
    change_extension(list, LIST_EXTENSION)
}

/// `LoadList`: the zip's `assets.json`, or the file itself when it is not a
/// zip (a plain JSON list).
fn load_list(path: &str) -> Result<Document, CommandError> {
    let data = std::fs::read(path).map_err(|error| CommandError::new("io", format!("{path}: {error}")))?;
    let bytes = match zip::read_entry(&data, ASSETS_JSON)
        .map_err(|error| CommandError::new("invalid_list", format!("{path}: {error}")))?
    {
        Some(bytes) => bytes,
        None => data,
    };
    let root: Json = serde_json::from_slice(&bytes)
        .map_err(|error| CommandError::new("invalid_list", format!("{path}: {error}")))?;
    let packing = match root.get("Packing") {
        Some(packing) => Packing::from_json(packing),
        None => Packing::default(),
    };
    let assets = root
        .get("Assets")
        .and_then(Json::as_array)
        .map(|assets| assets.iter().map(Asset::from_json).collect())
        .unwrap_or_default();
    Ok(Document { packing, assets })
}

/// `SaveList`: the zip with the `assets.json` entry.
fn write_list(path: &str, document: &Document) -> Result<(), CommandError> {
    let text = serde_json::to_vec(&document.to_json())
        .map_err(|error| CommandError::new("invalid_list", error.to_string()))?;
    zip::write_entry(Path::new(path), ASSETS_JSON, &text)
        .map_err(|error| CommandError::new("io", format!("{path}: {error}")))
}

fn checked_index(assets: &[Asset], index: usize) -> Result<(), CommandError> {
    if index >= assets.len() {
        return Err(CommandError::new(
            "invalid_params",
            format!("the assets list has no asset at index {index}"),
        ));
    }
    Ok(())
}

/// The selected indices as the tree's selection: a set, and none of them
/// outside the list.
fn checked_selection(assets: &[Asset], selected: &[usize]) -> Result<BTreeSet<usize>, CommandError> {
    for &index in selected {
        checked_index(assets, index)?;
    }
    Ok(selected.iter().copied().collect())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListRequest {
    /// The `.bsarch` list (or a plain JSON list).
    pub list: String,
}

/// `LoadList` through `mniLoadListClick`.
#[derive(Serialize, JsonSchema)]
pub struct ListLoadResponse {
    /// The path that was read.
    pub list: String,
    pub packing: Packing,
    /// The assets as `TAsset.ToJson` writes them: `[compressed, AssetName,
    /// FileName when it differs, ArchiveName when there is one]`.
    #[schemars(with = "Vec<serde_json::Value>")]
    pub assets: Vec<Asset>,
}

fn bsarchpro_list_load(_: &mut Session, request: ListRequest) -> Result<ListLoadResponse, CommandError> {
    let document = load_list(&request.list)?;
    Ok(ListLoadResponse {
        list: request.list,
        packing: document.packing,
        assets: document.assets,
    })
}

/// `mniSaveListClick`: the list saved to the path, with the `.bsarch`
/// extension.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListSaveRequest {
    /// The path to write; its extension becomes `.bsarch`.
    pub list: String,
    /// The packing settings; the form's defaults when omitted.
    pub packing: Option<Packing>,
    /// The assets as `TAsset.ToJson` writes them; none when omitted.
    #[serde(default)]
    #[schemars(with = "Option<Vec<Vec<serde_json::Value>>>")]
    pub assets: Option<Vec<Asset>>,
    /// Report the path that would be written, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct ListSaveResponse {
    /// The path written (or that a dry run would write).
    pub list: String,
    /// The number of assets written.
    pub assets: usize,
    /// Whether the file was written (a dry run does not).
    pub written: bool,
    pub dry_run: bool,
}

fn bsarchpro_list_save(_: &mut Session, request: ListSaveRequest) -> Result<ListSaveResponse, CommandError> {
    let document = Document {
        packing: request.packing.unwrap_or_default(),
        assets: request.assets.unwrap_or_default(),
    };
    let path = save_path(&request.list);
    if !request.dry_run {
        write_list(&path, &document)?;
    }
    Ok(ListSaveResponse {
        list: path,
        assets: document.assets.len(),
        written: !request.dry_run,
        dry_run: request.dry_run,
    })
}

/// `mniAssetCompressClick`: every asset's compressed flag according to
/// `TwbAsset.DoNotCompress`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompressRequest {
    /// The `.bsarch` list to change.
    pub list: String,
    /// Report what would be written, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct CompressResponse {
    /// The path written (or that a dry run would write).
    pub list: String,
    /// The assets whose compressed flag is set.
    pub compressed: usize,
    /// The sounds, voices, music and strings left uncompressed.
    pub uncompressed: usize,
    pub dry_run: bool,
}

fn bsarchpro_compress(_: &mut Session, request: CompressRequest) -> Result<CompressResponse, CommandError> {
    let mut document = load_list(&request.list)?;
    let mut compressed = 0;
    for asset in &mut document.assets {
        asset.compressed = !do_not_compress(&asset.asset_name);
        if asset.compressed {
            compressed += 1;
        }
    }
    let path = save_path(&request.list);
    if !request.dry_run {
        write_list(&path, &document)?;
    }
    Ok(CompressResponse {
        list: path,
        compressed,
        uncompressed: document.assets.len() - compressed,
        dry_run: request.dry_run,
    })
}

/// `mniAssetEditClick`: the name of one selected asset replaced by the
/// edited text, trimmed; empty text keeps the name.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditRequest {
    /// The `.bsarch` list to change.
    pub list: String,
    /// The selected asset (an index into the assets list).
    pub index: usize,
    /// The edited asset name (`InputQuery`'s text).
    pub name: String,
    /// Report what would be written, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct EditResponse {
    pub list: String,
    pub index: usize,
    /// The asset name after the edit.
    pub asset_name: String,
    /// Whether the name changed.
    pub changed: bool,
    pub dry_run: bool,
}

fn bsarchpro_edit(_: &mut Session, request: EditRequest) -> Result<EditResponse, CommandError> {
    let mut document = load_list(&request.list)?;
    checked_index(&document.assets, request.index)?;
    let (changed, asset_name) = {
        let asset = &mut document.assets[request.index];
        let name = request.name.trim();
        let changed = !name.is_empty() && name != asset.asset_name;
        if !name.is_empty() {
            asset.asset_name = name.to_owned();
        }
        (changed, asset.asset_name.clone())
    };
    let path = save_path(&request.list);
    if !request.dry_run {
        write_list(&path, &document)?;
    }
    Ok(EditResponse {
        list: path,
        index: request.index,
        asset_name,
        changed,
        dry_run: request.dry_run,
    })
}

/// The mode of `TfrmSearchReplace`'s radio buttons.
#[derive(Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReplaceMode {
    /// `rbReplace`: every occurrence of the search text, without regard to
    /// case.
    Replace,
    /// `rbPrepend`: the search text before the asset name.
    Prepend,
    /// `rbAppend`: the search text after the asset name.
    Append,
}

/// `mniAssetReplaceClick`: the search and replace dialog over the selected
/// assets.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceRequest {
    /// The `.bsarch` list to change.
    pub list: String,
    /// The dialog's mode.
    pub mode: ReplaceMode,
    /// `SearchText`: the search text of the replace mode (which can not be
    /// empty), or the text prepended or appended.
    pub search: String,
    /// `ReplaceText`: the replacement of the replace mode.
    #[serde(default)]
    pub replace_text: String,
    /// The selected assets (indices into the assets list).
    #[serde(default)]
    pub selected: Vec<usize>,
    /// Report what would be written, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct ReplaceResponse {
    pub list: String,
    /// The assets whose name changed.
    pub changed: usize,
    pub dry_run: bool,
}

/// `StringReplace(..., [rfReplaceAll, rfIgnoreCase])` with the search text
/// folded bytewise to ASCII lower case; asset names are ASCII.
fn replace_all_ignore_case(text: &str, search: &str, replacement: &str) -> String {
    let haystack = text.to_ascii_lowercase();
    let needle = search.to_ascii_lowercase();
    let mut result = String::new();
    let mut start = 0;
    let mut at = 0;
    while let Some(found) = haystack[at..].find(&needle) {
        let found = at + found;
        result.push_str(&text[start..found]);
        result.push_str(replacement);
        at = found + needle.len();
        start = at;
    }
    result.push_str(&text[start..]);
    result
}

fn bsarchpro_replace(_: &mut Session, request: ReplaceRequest) -> Result<ReplaceResponse, CommandError> {
    if request.mode == ReplaceMode::Replace && request.search.is_empty() {
        return Err(CommandError::new("invalid_params", "Search string can not be empty"));
    }
    let mut document = load_list(&request.list)?;
    let selected = checked_selection(&document.assets, &request.selected)?;
    let mut changed = 0;
    for &index in &selected {
        let asset = &mut document.assets[index];
        let name = match request.mode {
            ReplaceMode::Replace => replace_all_ignore_case(&asset.asset_name, &request.search, &request.replace_text),
            ReplaceMode::Prepend => format!("{}{}", request.search, asset.asset_name),
            ReplaceMode::Append => format!("{}{}", asset.asset_name, request.search),
        };
        if name != asset.asset_name {
            asset.asset_name = name;
            changed += 1;
        }
    }
    let path = save_path(&request.list);
    if !request.dry_run {
        write_list(&path, &document)?;
    }
    Ok(ReplaceResponse {
        list: path,
        changed,
        dry_run: request.dry_run,
    })
}

/// `mniAssetRemoveSelectedClick`: the selected assets removed, with the
/// second `btnClearList` question when the selection is the whole list.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveSelectedRequest {
    /// The `.bsarch` list to change.
    pub list: String,
    /// The selected assets (indices into the assets list).
    #[serde(default)]
    pub selected: Vec<usize>,
    /// The answer to "Remove N selected asset(s)?"; the default button
    /// (Yes) when omitted.
    pub confirm: Option<bool>,
    /// The answer to "Clear assets list?" when every asset is selected; Yes
    /// when omitted.
    pub clear_confirm: Option<bool>,
    /// Report what would be written, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct RemoveSelectedResponse {
    pub list: String,
    /// The assets removed.
    pub removed: usize,
    /// The assets left.
    pub remaining: usize,
    /// Whether the `btnClearList` question emptied the list.
    pub cleared: bool,
    pub dry_run: bool,
}

fn bsarchpro_remove_selected(
    _: &mut Session,
    request: RemoveSelectedRequest,
) -> Result<RemoveSelectedResponse, CommandError> {
    let mut document = load_list(&request.list)?;
    let selected = checked_selection(&document.assets, &request.selected)?;
    let path = save_path(&request.list);
    let count = selected.len();
    if !request.confirm.unwrap_or(true) {
        return Ok(RemoveSelectedResponse {
            list: path,
            removed: 0,
            remaining: document.assets.len(),
            cleared: false,
            dry_run: request.dry_run,
        });
    }
    if count == document.assets.len() {
        // `btnClearList.Click` and its own question.
        if !request.clear_confirm.unwrap_or(true) {
            return Ok(RemoveSelectedResponse {
                list: path,
                removed: 0,
                remaining: document.assets.len(),
                cleared: false,
                dry_run: request.dry_run,
            });
        }
        document.assets.clear();
    } else {
        document.assets = document
            .assets
            .into_iter()
            .enumerate()
            .filter(|(index, _)| !selected.contains(index))
            .map(|(_, asset)| asset)
            .collect();
    }
    if !request.dry_run {
        write_list(&path, &document)?;
    }
    Ok(RemoveSelectedResponse {
        list: path,
        removed: count,
        remaining: document.assets.len(),
        cleared: count > 0 && document.assets.is_empty(),
        dry_run: request.dry_run,
    })
}

/// `mniAssetRemoveUnselectedClick`: every asset but the selected ones
/// removed.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveUnselectedRequest {
    /// The `.bsarch` list to change.
    pub list: String,
    /// The selected assets (indices into the assets list); none keeps none.
    #[serde(default)]
    pub selected: Vec<usize>,
    /// The answer to "Remove N unselected asset(s)?"; the default button
    /// (Yes) when omitted.
    pub confirm: Option<bool>,
    /// Report what would be written, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct RemoveUnselectedResponse {
    pub list: String,
    /// The assets removed.
    pub removed: usize,
    /// The assets left (the selected ones).
    pub remaining: usize,
    pub dry_run: bool,
}

fn bsarchpro_remove_unselected(
    _: &mut Session,
    request: RemoveUnselectedRequest,
) -> Result<RemoveUnselectedResponse, CommandError> {
    let mut document = load_list(&request.list)?;
    let selected = checked_selection(&document.assets, &request.selected)?;
    let path = save_path(&request.list);
    let remaining = selected.len();
    let removed = document.assets.len() - remaining;
    if !request.confirm.unwrap_or(true) {
        return Ok(RemoveUnselectedResponse {
            list: path,
            removed: 0,
            remaining: document.assets.len(),
            dry_run: request.dry_run,
        });
    }
    document.assets = document
        .assets
        .into_iter()
        .enumerate()
        .filter(|(index, _)| selected.contains(index))
        .map(|(_, asset)| asset)
        .collect();
    if !request.dry_run {
        write_list(&path, &document)?;
    }
    Ok(RemoveUnselectedResponse {
        list: path,
        removed,
        remaining,
        dry_run: request.dry_run,
    })
}

/// One asset of an identical group.
#[derive(Serialize, JsonSchema)]
pub struct IdenticalAsset {
    /// The index in the assets list.
    pub index: usize,
    /// `ArchiveName + '\' + FileName`, or the file name.
    pub description: String,
}

/// The assets [`TwbSameData`] puts together: the same data size and
/// `TwbHash.LookupHash` (xEdit compares no bytes).
#[derive(Serialize, JsonSchema)]
pub struct IdenticalGroup {
    pub data_size: u64,
    /// `TwbHash.LookupHash` of the data, as sixteen hexadecimal digits.
    pub lookup_hash: String,
    pub assets: Vec<IdenticalAsset>,
}

/// `mniAssetFindIdenticalClick`: the groups of assets with identical data,
/// and the report the dialog shows.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindIdenticalRequest {
    /// The `.bsarch` list to read.
    pub list: String,
}

#[derive(Serialize, JsonSchema)]
pub struct FindIdenticalResponse {
    pub list: String,
    /// The groups with more than one asset.
    pub groups: Vec<IdenticalGroup>,
    /// `Duplicate Files`: the sum of the extra assets of each group.
    pub duplicate_files: u64,
    /// `Duplicate Size`: what the duplicates take, uncompressed.
    pub duplicate_size: u64,
    /// `FormatSize(duplicate_size)`.
    pub size_text: String,
    /// The dialog's title (its first two lines).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The dialog's text lines.
    pub lines: Vec<String>,
    /// `No identical assets found` when there is no group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// `FormatSize`: the size in the largest unit it fills, with at most two
/// decimals (`FormatFloat('###0.##')`).
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 9] = ["Bytes", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let mut unit = 0usize;
    while let Some(next) = 1024u64.checked_pow(unit as u32 + 1)
        && bytes >= next
        && unit + 1 < UNITS.len()
    {
        unit += 1;
    }
    let value = bytes as f64 / 1024f64.powi(unit as i32);
    let mut text = format!("{value:.2}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    format!("{text} {}", UNITS[unit])
}

fn bsarchpro_find_identical(
    _: &mut Session,
    request: FindIdenticalRequest,
) -> Result<FindIdenticalResponse, CommandError> {
    let document = load_list(&request.list)?;
    let mut archives = HashMap::new();
    let mut groups: Vec<(u64, u64, Vec<usize>)> = Vec::new();
    for (index, asset) in document.assets.iter().enumerate() {
        let data = asset.data(&mut archives)?;
        let size = data.len() as u64;
        let hash = lookup_hash(&data);
        match groups
            .iter_mut()
            .find(|(group_size, group_hash, _)| *group_size == size && *group_hash == hash)
        {
            Some((_, _, indices)) => indices.push(index),
            None => groups.push((size, hash, vec![index])),
        }
    }
    let mut duplicate_files = 0u64;
    let mut duplicate_size = 0u64;
    let mut lines = Vec::new();
    let mut response_groups = Vec::new();
    for (size, hash, indices) in groups {
        if indices.len() < 2 {
            continue;
        }
        let assets: Vec<IdenticalAsset> = indices
            .iter()
            .map(|&index| IdenticalAsset {
                index,
                description: document.assets[index].description(),
            })
            .collect();
        for asset in &assets {
            lines.push(asset.description.clone());
        }
        lines.push(String::new());
        duplicate_files += (indices.len() - 1) as u64;
        duplicate_size += size * (indices.len() - 1) as u64;
        response_groups.push(IdenticalGroup {
            data_size: size,
            lookup_hash: format!("{hash:016X}"),
            assets,
        });
    }
    if lines.is_empty() {
        return Ok(FindIdenticalResponse {
            list: request.list,
            groups: Vec::new(),
            duplicate_files: 0,
            duplicate_size: 0,
            size_text: format_size(0),
            title: None,
            lines: Vec::new(),
            message: Some("No identical assets found".to_owned()),
        });
    }
    let size_text = format_size(duplicate_size);
    let title = format!(
        "Duplicate Files: {duplicate_files}\nDuplicate Size: {size_text} (uncompressed saved space if all identical files will end up in the same archive with Shared Data option)"
    );
    lines.insert(0, String::new());
    lines.insert(0, format!("Duplicate Size: {size_text} (uncompressed saved space if all identical files will end up in the same archive with Shared Data option)"));
    lines.insert(0, format!("Duplicate Files: {duplicate_files}"));
    Ok(FindIdenticalResponse {
        list: request.list,
        groups: response_groups,
        duplicate_files,
        duplicate_size,
        size_text,
        title: Some(title),
        lines,
        message: None,
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "bsarchpro.list_load",
        "Read a BSArchPro .bsarch asset list, its packing settings and its assets (LoadList).",
        false,
        bsarchpro_list_load,
    );
    registry.register(
        "bsarchpro.list_save",
        "Write a BSArchPro asset list as the GUI saves it (SaveList, mniSaveListClick).",
        true,
        bsarchpro_list_save,
    );
    registry.register(
        "bsarchpro.compress",
        "Set every asset's compressed flag from TwbAsset.DoNotCompress (mniAssetCompressClick).",
        true,
        bsarchpro_compress,
    );
    registry.register(
        "bsarchpro.edit",
        "Replace the name of one asset, trimmed; an empty name keeps it (mniAssetEditClick).",
        true,
        bsarchpro_edit,
    );
    registry.register(
        "bsarchpro.replace",
        "Replace, prepend or append text in the names of the selected assets (mniAssetReplaceClick).",
        true,
        bsarchpro_replace,
    );
    registry.register(
        "bsarchpro.remove_selected",
        "Remove the selected assets, asking about clearing the whole list (mniAssetRemoveSelectedClick).",
        true,
        bsarchpro_remove_selected,
    );
    registry.register(
        "bsarchpro.remove_unselected",
        "Remove the unselected assets (mniAssetRemoveUnselectedClick).",
        true,
        bsarchpro_remove_unselected,
    );
    registry.register(
        "bsarchpro.find_identical",
        "Find the assets with identical data and report the duplicates as the dialog does (mniAssetFindIdenticalClick).",
        false,
        bsarchpro_find_identical,
    );
}
