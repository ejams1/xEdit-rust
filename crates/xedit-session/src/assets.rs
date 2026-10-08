// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDataFormat.pas (LoadFromData, SaveToData,
// ToText, ToJSON, FromJSON, EditValues), Core/wbDataFormatNif.pas
// (TwbNifFile), Core/wbDataFormatMaterial.pas, Core/wbDataFormatMisc.pas

//! The asset commands: NIF and KF meshes, BGSM and BGEM materials, the LOD
//! settings and tree LOD files, FUZ voice files and DDS headers, read from
//! a loose file or from an archive. `assets.dump` prints a file as
//! `ToText` or `ToJSON` does (Sniff's JSON converter), `assets.blocks`
//! lists the blocks of a NIF, `assets.save` writes a file back as xEdit
//! saves it, `assets.set` changes values by path before saving (Sniff's
//! universal tweaker), and `assets.from-json` builds a file from its JSON.
//! None of them needs a loaded plugin.

use std::path::Path;
use std::sync::atomic::Ordering;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_assets::asset::{AssetFile, AssetKind};
use xedit_assets::data_format::{DfError, El, FLOAT_DECIMAL_DIGITS};
use xedit_assets::data_format_nif::{
    block, block_by_path, block_type, blocks_count, footer, header, wb_ni_object_list,
};
use xedit_assets::data_format_nif_types::ROTATION_EULER;
use xedit_io::archive::Archive;

use crate::save::write_atomically;
use crate::{CommandError, Registry, Session};

/// Where an asset is read from.
pub struct AssetSource {
    pub file: String,
    pub archive_path: Option<String>,
    pub kind: Option<String>,
}

macro_rules! source_of {
    ($request:expr) => {
        AssetSource {
            file: $request.file.clone(),
            archive_path: $request.archive_path.clone(),
            kind: $request.kind.clone(),
        }
    };
}

fn load_failed(error: DfError) -> CommandError {
    CommandError::new("load_failed", error.0)
}

fn edit_failed(error: DfError) -> CommandError {
    CommandError::new("edit_failed", error.0)
}

/// The bytes and the kind of the asset.
fn read_source(source: &AssetSource) -> Result<(Vec<u8>, AssetKind), CommandError> {
    let name = source.archive_path.as_deref().unwrap_or(&source.file);
    let kind = match &source.kind {
        Some(kind) => AssetKind::from_name(kind)
            .ok_or_else(|| CommandError::new("invalid_params", format!("unknown asset kind {kind}")))?,
        None => AssetKind::from_path(name).ok_or_else(|| {
            CommandError::new(
                "invalid_params",
                format!("no asset kind for {name}; give one with kind"),
            )
        })?,
    };
    let data = match &source.archive_path {
        Some(path) => {
            let archive = Archive::open(Path::new(&source.file)).map_err(|error| CommandError::new("io", error.0))?;
            archive
                .read(path)
                .map_err(|error| CommandError::new("io", error.0))?
                .ok_or_else(|| CommandError::new("io", format!("{path} is not in {}", source.file)))?
        }
        None => {
            std::fs::read(&source.file).map_err(|error| CommandError::new("io", format!("{}: {error}", source.file)))?
        }
    };
    Ok((data, kind))
}

fn load(source: &AssetSource) -> Result<(AssetFile, Vec<u8>), CommandError> {
    let (data, kind) = read_source(source)?;
    let file = AssetFile::load(kind, &data).map_err(load_failed)?;
    Ok((file, data))
}

/// `assets.dump`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetsDumpRequest {
    /// Path of the file on disk, or of the archive (BSA, BA2) that holds it.
    pub file: String,
    /// The path of the file inside the archive `file`, such as
    /// `meshes\clutter\bucket01.nif`; omitted for a loose file.
    pub archive_path: Option<String>,
    /// The format: nif (also KF), bgsm, bgem, lod, dlodsettings, lst, btt
    /// (also DTL), fuz or dds. By the extension when omitted.
    pub kind: Option<String>,
    /// `text` (`ToText`: one element per line, indented with tabs) or `json`
    /// (`ToJSON`, as Sniff's "Convert to and from JSON" writes it).
    /// Defaults to text.
    pub format: Option<String>,
    /// Decimals of the float values (`dfFloatDecimalDigits`), 6 to 16.
    /// Defaults to 6.
    pub decimals: Option<usize>,
    /// Write rotations as Euler angles in degrees instead of an angle and
    /// an axis (`wbRotationEuler`).
    #[serde(default)]
    pub euler: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct AssetsDumpResponse {
    /// The asset kind.
    pub kind: String,
    /// `text` or `json`.
    pub format: String,
    /// The dump.
    pub text: String,
}

/// Runs `body` with the float decimals and the rotation form of a dump,
/// restoring them after.
fn with_dump_settings<T>(decimals: Option<usize>, euler: bool, body: impl FnOnce() -> T) -> T {
    let old_digits = FLOAT_DECIMAL_DIGITS.load(Ordering::Relaxed);
    let old_euler = ROTATION_EULER.load(Ordering::Relaxed);
    FLOAT_DECIMAL_DIGITS.store(decimals.unwrap_or(6), Ordering::Relaxed);
    ROTATION_EULER.store(euler, Ordering::Relaxed);
    let result = body();
    FLOAT_DECIMAL_DIGITS.store(old_digits, Ordering::Relaxed);
    ROTATION_EULER.store(old_euler, Ordering::Relaxed);
    result
}

fn assets_dump(_: &mut Session, request: AssetsDumpRequest) -> Result<AssetsDumpResponse, CommandError> {
    let format = request.format.as_deref().unwrap_or("text").to_ascii_lowercase();
    if format != "text" && format != "json" {
        return Err(CommandError::new("invalid_params", "format is text or json"));
    }
    if let Some(decimals) = request.decimals
        && !(6..=16).contains(&decimals)
    {
        return Err(CommandError::new(
            "invalid_params",
            "Decimal digits can vary from 6 to 16",
        ));
    }
    with_dump_settings(request.decimals, request.euler, || {
        let (mut file, _) = load(&source_of!(request))?;
        let text = if format == "json" {
            file.to_json(false)
        } else {
            file.to_text()
        }
        .map_err(load_failed)?;
        Ok(AssetsDumpResponse {
            kind: file.kind.name().to_owned(),
            format,
            text,
        })
    })
}

/// `assets.blocks`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetsBlocksRequest {
    /// Path of the file on disk, or of the archive (BSA, BA2) that holds it.
    pub file: String,
    /// The path of the file inside the archive `file`, such as
    /// `meshes\clutter\bucket01.nif`; omitted for a loose file.
    pub archive_path: Option<String>,
    /// The format: nif (also KF), bgsm, bgem, lod, dlodsettings, lst, btt
    /// (also DTL), fuz or dds. By the extension when omitted.
    pub kind: Option<String>,
}

/// A block of a NIF file.
#[derive(Serialize, JsonSchema)]
pub struct NifBlockInfo {
    /// Index of the block; the header and the footer have none.
    pub index: Option<i32>,
    /// The block type, such as `BSFadeNode`.
    pub block_type: String,
    /// The value of its `Name` element, when it has one.
    pub name: String,
}

#[derive(Serialize, JsonSchema)]
pub struct AssetsBlocksResponse {
    /// The NIF version (`nfTES3`, `nfTES4`, `nfFO3`, `nfTES5`, `nfSSE`, `nfFO4`).
    pub nif_version: String,
    /// The header, the blocks in file order and the footer.
    pub blocks: Vec<NifBlockInfo>,
}

fn assets_blocks(_: &mut Session, request: AssetsBlocksRequest) -> Result<AssetsBlocksResponse, CommandError> {
    let (mut file, _) = load(&source_of!(request))?;
    if file.kind != AssetKind::Nif {
        return Err(CommandError::new(
            "invalid_params",
            "assets.blocks lists the blocks of a NIF file",
        ));
    }
    let tree = &mut file.tree;
    let mut blocks = Vec::new();
    let mut info = |tree: &mut xedit_assets::data_format::Tree, el: El, index: Option<i32>| {
        let name = tree.edit_values(el, "Name").map_err(load_failed)?;
        blocks.push(NifBlockInfo {
            index,
            block_type: block_type(tree, el).to_owned(),
            name,
        });
        Ok::<(), CommandError>(())
    };
    let header = header(tree).map_err(load_failed)?;
    info(tree, header, None)?;
    for index in 0..blocks_count(tree).map_err(load_failed)? {
        let el = block(tree, index).map_err(load_failed)?;
        info(tree, el, Some(index))?;
    }
    let footer = footer(tree).map_err(load_failed)?;
    info(tree, footer, None)?;
    Ok(AssetsBlocksResponse {
        nif_version: tree.nif.nif_version.name().to_owned(),
        blocks,
    })
}

/// `assets.save`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetsSaveRequest {
    /// Path of the file on disk, or of the archive (BSA, BA2) that holds it.
    pub file: String,
    /// The path of the file inside the archive `file`, such as
    /// `meshes\clutter\bucket01.nif`; omitted for a loose file.
    pub archive_path: Option<String>,
    /// The format: nif (also KF), bgsm, bgem, lod, dlodsettings, lst, btt
    /// (also DTL), fuz or dds. By the extension when omitted.
    pub kind: Option<String>,
    /// Path to write the file to.
    pub output: String,
    /// Build the file and report it, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct AssetsSaveResponse {
    /// The asset kind.
    pub kind: String,
    /// The path written to (for a dry run, the path it would write).
    pub output: String,
    /// The size of the saved file in bytes.
    pub size: usize,
    /// Whether the saved bytes equal the bytes read.
    pub equal_to_input: bool,
    /// Whether the file was written.
    pub written: bool,
    pub dry_run: bool,
}

fn finish_save(
    file: &mut AssetFile,
    input: &[u8],
    output: &str,
    dry_run: bool,
) -> Result<AssetsSaveResponse, CommandError> {
    let bytes = file.save().map_err(edit_failed)?;
    if !dry_run {
        write_atomically(Path::new(output), &bytes)?;
    }
    Ok(AssetsSaveResponse {
        kind: file.kind.name().to_owned(),
        output: output.to_owned(),
        size: bytes.len(),
        equal_to_input: bytes == input,
        written: !dry_run,
        dry_run,
    })
}

fn assets_save(_: &mut Session, request: AssetsSaveRequest) -> Result<AssetsSaveResponse, CommandError> {
    let (mut file, input) = load(&source_of!(request))?;
    finish_save(&mut file, &input, &request.output, request.dry_run)
}

/// One value to set.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetEdit {
    /// For a NIF, the block the path starts at: `header`, `footer`, a block
    /// index, or a block path of block types and names separated by `\`
    /// from a root block (`BlockByPath`). Omitted: the path starts at the
    /// root of the file.
    pub block: Option<String>,
    /// Path of the element below the block, with `\` between the names,
    /// `[n]` for an entry of an array and a flag name as the last part of a
    /// flags value, such as `Transform\Scale` or `Flags`.
    pub path: String,
    /// The new value as the dump prints it (`EditValue`).
    pub value: String,
}

/// `assets.set`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetsSetRequest {
    /// Path of the file on disk, or of the archive (BSA, BA2) that holds it.
    pub file: String,
    /// The path of the file inside the archive `file`, such as
    /// `meshes\clutter\bucket01.nif`; omitted for a loose file.
    pub archive_path: Option<String>,
    /// The format: nif (also KF), bgsm, bgem, lod, dlodsettings, lst, btt
    /// (also DTL), fuz or dds. By the extension when omitted.
    pub kind: Option<String>,
    /// The values to set, in order.
    pub edits: Vec<AssetEdit>,
    /// Path to write the changed file to.
    pub output: String,
    /// Report the values before and after, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// A value that was set.
#[derive(Serialize, JsonSchema)]
pub struct AssetChange {
    /// The block and path of the edit.
    pub block: Option<String>,
    pub path: String,
    /// The value before.
    pub old: String,
    /// The value after, as the element reads it back.
    pub new: String,
}

#[derive(Serialize, JsonSchema)]
pub struct AssetsSetResponse {
    pub changes: Vec<AssetChange>,
    #[serde(flatten)]
    pub save: AssetsSaveResponse,
}

/// The element an edit starts at.
fn edit_base(file: &mut AssetFile, block_name: Option<&str>) -> Result<El, CommandError> {
    let Some(name) = block_name else {
        return Ok(file.root);
    };
    if file.kind != AssetKind::Nif {
        return Err(CommandError::new("invalid_params", "block applies to NIF files only"));
    }
    let tree = &mut file.tree;
    let found = if name.eq_ignore_ascii_case("header") {
        Some(header(tree).map_err(edit_failed)?)
    } else if name.eq_ignore_ascii_case("footer") {
        Some(footer(tree).map_err(edit_failed)?)
    } else if let Ok(index) = name.parse::<i32>() {
        block(tree, index).ok()
    } else {
        block_by_path(tree, name).map_err(edit_failed)?
    };
    found.ok_or_else(|| CommandError::new("unknown_element", format!("no block {name}")))
}

fn assets_set(_: &mut Session, request: AssetsSetRequest) -> Result<AssetsSetResponse, CommandError> {
    let (mut file, input) = load(&source_of!(request))?;
    let mut changes = Vec::new();
    for edit in &request.edits {
        let base = edit_base(&mut file, edit.block.as_deref())?;
        let tree = &mut file.tree;
        let target = match edit.path.rfind('\\') {
            Some(index) => tree.elements(base, &edit.path[..index]).map_err(edit_failed)?,
            None => Some(base),
        };
        if target.is_none() {
            return Err(CommandError::new(
                "unknown_element",
                format!("no element {}", edit.path),
            ));
        }
        let old = tree.edit_values(base, &edit.path).map_err(edit_failed)?;
        tree.set_edit_values(base, &edit.path, &edit.value)
            .map_err(edit_failed)?;
        let new = tree.edit_values(base, &edit.path).map_err(edit_failed)?;
        changes.push(AssetChange {
            block: edit.block.clone(),
            path: edit.path.clone(),
            old,
            new,
        });
    }
    let save = finish_save(&mut file, &input, &request.output, request.dry_run)?;
    Ok(AssetsSetResponse { changes, save })
}

/// `assets.from-json`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetsFromJsonRequest {
    /// Path of the JSON file: the form `assets.dump` writes, or for a
    /// material the form of the material editor.
    pub file: String,
    /// The format to build; by the extension before `.json` when omitted
    /// (`x.nif.json` is a NIF).
    pub kind: Option<String>,
    /// Path to write the built file to.
    pub output: String,
    /// Build the file and report it, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

fn assets_from_json(_: &mut Session, request: AssetsFromJsonRequest) -> Result<AssetsSaveResponse, CommandError> {
    let kind = match &request.kind {
        Some(kind) => AssetKind::from_name(kind)
            .ok_or_else(|| CommandError::new("invalid_params", format!("unknown asset kind {kind}")))?,
        None => {
            let stem = request.file.strip_suffix(".json").unwrap_or(&request.file);
            AssetKind::from_path(stem)
                .ok_or_else(|| CommandError::new("invalid_params", "no asset kind for the file; give one with kind"))?
        }
    };
    let text =
        std::fs::read(&request.file).map_err(|error| CommandError::new("io", format!("{}: {error}", request.file)))?;
    let text = String::from_utf8_lossy(&text);
    let mut file = AssetFile::from_json(kind, &text).map_err(load_failed)?;
    finish_save(&mut file, &[], &request.output, request.dry_run)
}

/// `assets.types`: the request.
#[derive(Serialize, JsonSchema)]
pub struct AssetsTypesResponse {
    /// Every NIF block type the definitions know, in definition order
    /// (`wbNiObjectList`), abstract ones included.
    pub block_types: Vec<String>,
}

fn assets_types(_: &mut Session, _: crate::NoParams) -> Result<AssetsTypesResponse, CommandError> {
    Ok(AssetsTypesResponse {
        block_types: wb_ni_object_list(),
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "assets.dump",
        "Print a NIF, material or other data format file as text or JSON (ToText, ToJSON).",
        false,
        assets_dump,
    );
    registry.register(
        "assets.blocks",
        "List the blocks of a NIF file with their types and names (TwbNifFile.Blocks).",
        false,
        assets_blocks,
    );
    registry.register(
        "assets.types",
        "List the NIF block types the definitions know (wbNiObjectList).",
        false,
        assets_types,
    );
    registry.register(
        "assets.save",
        "Load a data format file and write it back as xEdit saves it (SaveToData).",
        true,
        assets_save,
    );
    registry.register(
        "assets.set",
        "Set values of a data format file by path and write it (EditValues, SaveToData).",
        true,
        assets_set,
    );
    registry.register(
        "assets.from-json",
        "Build a data format file from its JSON form and write it (FromJSON, SaveToData).",
        true,
        assets_from_json,
    );
}
