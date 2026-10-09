// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (DoGenerateLOD,
// mniNavGenerateLODClick, TLoaderThread.Execute: the archives), the
// tmLODgen settings of xEdit/xeInit.pas, xEdit/xeLODGenForm.pas (the
// options and their lists)

//! `lodgen.generate`: the LODGen tool mode. The archives load as the GUI's
//! loader adds them (the game ini's lists, the archives of each plugin,
//! the data folder), the worldspaces with LOD are listed as the LODGen
//! form lists them, the options are the form's (from the settings file,
//! then the request), and the LOD of the chosen worldspaces is generated
//! (`wbGenerateLODTES5`, `wbGenerateLODFO4`; every worldspace for Oblivion,
//! `wbGenerateLODTES4`).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_assets::imaging::ImageFormat;
use xedit_assets::lod::{
    LodDefaults, LodEnv, generate_lod_fo4, generate_lod_tes4, generate_lod_tes5, randomize, set_rand_seed,
    worldspaces_for_lod,
};
use xedit_assets::sniff::processor::MemIniFile;
use xedit_core::container_handler::{add_archive, add_folder, clear_containers};
use xedit_core::helpers::{archive_extension, find_bsas, has_bsas, make_data_file_name};
use xedit_core::implementation::FileImpl;
use xedit_core::interface::element::MainRecordRef;
use xedit_core::interface::globals::{
    GameMode, ToolMode, app_name, data_path, game_mode, is_fallout3, is_fallout4, is_fallout76, is_skyrim,
    is_starfield, set_allow_internal_edit, set_show_internal_edit, set_tool_mode,
};
use xedit_core::interface::misc::{progress, set_progress_callback};
use xedit_core::interface::types::FileState;
use xedit_core::interface::{Element, File};

use crate::{CommandError, Registry, Session};

/// `lodgen.generate`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LodgenRequest {
    /// The editor IDs of the worldspaces to generate LOD for, as the LODGen
    /// form lists them (any case). Empty: the ones the form checks itself,
    /// the default worldspace (`0000003C`, `000DA726` in New Vegas) or the
    /// only one. Oblivion generates every worldspace, as its LODGen mode
    /// does; a list there limits it.
    #[serde(default)]
    pub worldspaces: Vec<String>,
    /// The options of the LODGen form by their name in the settings file
    /// (`[<APP> LOD Options]`): `ObjectsLOD`, `TreesLOD`, `Trees3D`,
    /// `BuildAtlas`, `AtlasWidth`, `AtlasHeight`, `AtlasTextureSize`,
    /// `AtlasTextureUVRange`, `AtlasDiffuseFormat`, `AtlasNormalFormat`,
    /// `AtlasSpecularFormat` (as the form names them: `888`, `8888`, `565`,
    /// `DXT1`, `DXT3`, `DXT5`, `BC4`, `BC5`), `DefaultAlphaThreshold`,
    /// `ObjectsNoTangents`, `ObjectsNoVertexColors`,
    /// `ObjectsUseAlphaThreshold`, `ObjectsUseBacklightPower`, `Chunk`,
    /// `LODLevel`, `LODX`, `LODY`, `LODX2`, `LODY2`, `TreesBrightness`;
    /// booleans as `1`/`0` or `true`/`false`. They override the settings
    /// file; the response lists every option with its value.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    /// The settings file of the options (upstream `<APP>LODGen.ini` next to
    /// the executable). It is read first and, unless a dry run, written
    /// back with the options used, as the form does.
    pub settings: Option<String>,
    /// The folder the LOD files go to (`-O:`); the data folder by default.
    pub output: Option<String>,
    /// The folder with `LODGenx64.exe`, `Texconvx64.exe`, `LODGen_flat_lod.nif`
    /// and the atlas maps (`-S:`), where the export files are written too;
    /// `Edit Scripts` next to the executable by default.
    pub scripts: Option<String>,
    /// The temporary folder for converted textures (`-T:`).
    pub temp: Option<String>,
    /// The data folder (`-D:`); the folder of the loaded plugins by default.
    pub data: Option<String>,
    /// The game ini whose archive lists load first (`-I:`, the game's
    /// `<Game>.ini`); without it only the archives named after the plugins
    /// load.
    pub game_ini: Option<String>,
    /// The `RandSeed` the tree rotations come from; from the clock by
    /// default, as upstream's `Randomize` at startup.
    pub seed: Option<u32>,
    /// List the worldspaces and the options, but generate nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// A worldspace of the LODGen form's list.
#[derive(Serialize, JsonSchema)]
pub struct LodgenWorldspace {
    pub editor_id: String,
    /// The name as the list shows it.
    pub name: String,
    /// Whether LOD is generated for it.
    pub checked: bool,
}

/// `lodgen.generate`: the response.
#[derive(Serialize, JsonSchema)]
pub struct LodgenResponse {
    /// The worldspaces of the list, in its order.
    pub worldspaces: Vec<LodgenWorldspace>,
    /// Every option of `[<APP> LOD Options]` as the generator reads it.
    pub options: BTreeMap<String, String>,
    /// The archives and folders the resources come from, in order.
    pub resources: Vec<String>,
    /// The messages of the generator, as the message log shows them.
    pub messages: Vec<String>,
    /// The `RandSeed` the tree rotations came from (none for a dry run).
    pub seed: Option<u32>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new("invalid_params", message)
}

/// A folder with its trailing backslash.
fn folder(path: &str) -> String {
    let path = path.replace('/', "\\");
    if path.ends_with('\\') {
        path
    } else {
        format!("{path}\\")
    }
}

/// `TLoaderThread.Execute`: the archives of the game ini, then those of
/// each plugin in load order, then the data folder.
fn load_resources(data: &str, game_ini: Option<&str>, plugins: &[String]) -> Vec<String> {
    clear_containers();
    let add = |name: &str| {
        let path = make_data_file_name(name, data);
        progress(&format!("[{name}] Loading Resources."));
        if let Err(error) = add_archive(Path::new(&path)) {
            progress(&format!("[{name}] Could not be loaded. <Error: {error}>"));
        }
    };
    if let Some(ini) = game_ini
        && Path::new(ini).is_file()
    {
        let (mut found, mut missing) = (Vec::new(), Vec::new());
        if find_bsas(Path::new(ini), data, &mut found, &mut missing) > 0 {
            for name in &found {
                add(name);
            }
            for name in &missing {
                progress(&format!("Warning: <Can't find {name}>"));
            }
        }
    }
    let exact = matches!(game_mode(), GameMode::gmTES5 | GameMode::gmEnderal);
    for plugin in plugins {
        let (mut found, mut missing) = (Vec::new(), Vec::new());
        let stem = xedit_core::delphi::change_file_ext(plugin, "");
        if has_bsas(&stem, data, exact, is_skyrim(), &mut found, &mut missing) > 0 {
            for name in &found {
                add(name);
            }
            for name in &missing {
                progress(&format!("Warning: <Can't find {name}>"));
            }
        }
    }
    progress(&format!("[{data}] Setting Resource Path."));
    add_folder(Path::new(data));
    let _ = archive_extension;
    xedit_core::container_handler::container_list()
}

/// A combo box of the form: its items and how a setting finds one
/// (`Max(Items.IndexOf(...), 0)`).
fn combo(items: &[String], value: &str) -> String {
    items
        .iter()
        .find(|item| xedit_io::encoding::ansi_compare_text(item, value).is_eq())
        .unwrap_or(&items[0])
        .clone()
}

/// The form's lists (`FormCreate`).
struct Lists {
    atlas_size: Vec<String>,
    texture_size: Vec<String>,
    uv_range: Vec<String>,
    brightness: Vec<String>,
    lod_level: Vec<String>,
    compression: Vec<String>,
    alpha_threshold: Vec<String>,
}

fn lists() -> Lists {
    let mut uv_range = Vec::new();
    let mut v = 1.0f64;
    while v <= 10.0 {
        uv_range.push(xedit_core::delphi::float_to_str_f_fixed(v, 1));
        v += 0.1;
    }
    Lists {
        atlas_size: [1024, 2048, 4096, 8192].iter().map(i32::to_string).collect(),
        texture_size: [256, 512, 1024].iter().map(i32::to_string).collect(),
        uv_range,
        brightness: (-30..=30).map(|i: i32| i.to_string()).collect(),
        lod_level: ["", "4", "8", "16"].iter().map(|s| s.to_string()).collect(),
        compression: ["888", "8888", "565", "DXT1", "DXT3", "DXT5", "BC4", "BC5"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        alpha_threshold: (0..=255).map(|i: i32| i.to_string()).collect(),
    }
}

/// `ImageFormatToStr`.
fn image_format_to_str(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::R8G8B8 => "888",
        ImageFormat::A8R8G8B8 => "8888",
        ImageFormat::R5G6B5 => "565",
        ImageFormat::Dxt1 => "DXT1",
        ImageFormat::Dxt3 => "DXT3",
        ImageFormat::Dxt5 => "DXT5",
        ImageFormat::Ati1n => "BC4",
        ImageFormat::Ati2n => "BC5",
        _ => "DXT5",
    }
}

/// `StrToImageFormat`.
fn str_to_image_format(name: &str) -> ImageFormat {
    match name {
        "888" => ImageFormat::R8G8B8,
        "8888" => ImageFormat::A8R8G8B8,
        "565" => ImageFormat::R5G6B5,
        "DXT1" => ImageFormat::Dxt1,
        "DXT3" => ImageFormat::Dxt3,
        "DXT5" => ImageFormat::Dxt5,
        "BC4" => ImageFormat::Ati1n,
        "BC5" => ImageFormat::Ati2n,
        _ => ImageFormat::Dxt5,
    }
}

/// The option names the request takes.
const OPTION_NAMES: &[&str] = &[
    "ObjectsLOD",
    "TreesLOD",
    "Trees3D",
    "BuildAtlas",
    "AtlasWidth",
    "AtlasHeight",
    "AtlasTextureSize",
    "AtlasTextureUVRange",
    "AtlasDiffuseFormat",
    "AtlasNormalFormat",
    "AtlasSpecularFormat",
    "DefaultAlphaThreshold",
    "ObjectsNoTangents",
    "ObjectsNoVertexColors",
    "ObjectsUseAlphaThreshold",
    "ObjectsUseBacklightPower",
    "Chunk",
    "LODLevel",
    "LODX",
    "LODY",
    "LODX2",
    "LODY2",
    "TreesBrightness",
];

fn parse_bool(name: &str, value: &str) -> Result<bool, CommandError> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(invalid(format!("option {name}: {value} is not a boolean"))),
    }
}

/// A value of a combo box set by the request: one of its items.
fn pick(name: &str, items: &[String], value: &str) -> Result<String, CommandError> {
    items
        .iter()
        .find(|item| xedit_io::encoding::ansi_compare_text(item, value).is_eq())
        .cloned()
        .ok_or_else(|| invalid(format!("option {name}: {value} is not one of {}", items.join(", "))))
}

/// The options of the form, read from the settings and the request
/// (`mniNavGenerateLODClick` up to `ShowModal`), and written back to the
/// settings as the form does after `Generate`; the LOD types.
fn form_options(
    settings: &mut MemIniFile,
    request: &BTreeMap<String, String>,
    defaults: &LodDefaults,
) -> Result<(bool, bool), CommandError> {
    for name in request.keys() {
        if !OPTION_NAMES.iter().any(|known| known.eq_ignore_ascii_case(name)) {
            return Err(invalid(format!(
                "unknown option {name}; the options are {}",
                OPTION_NAMES.join(", ")
            )));
        }
    }
    let given = |name: &str| {
        request
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };
    let section = format!("{} LOD Options", app_name());
    let lists = lists();
    let read_string = |settings: &MemIniFile, name: &str, default: &str| settings.read_string(&section, name, default);
    let bool_option = |settings: &MemIniFile, name: &str, default: bool| -> Result<bool, CommandError> {
        match given(name) {
            Some(value) => parse_bool(name, value),
            None => Ok(settings.read_bool(&section, name, default)),
        }
    };
    let combo_option =
        |settings: &MemIniFile, name: &str, items: &[String], default: &str| -> Result<String, CommandError> {
            match given(name) {
                Some(value) => pick(name, items, value),
                None => Ok(combo(items, &read_string(settings, name, default))),
            }
        };
    let format_option = |settings: &MemIniFile, name: &str, default: ImageFormat| -> Result<String, CommandError> {
        match given(name) {
            Some(value) => pick(name, &lists.compression, value),
            None => {
                let format =
                    ImageFormat::from_ordinal(i64::from(settings.read_integer(&section, name, default as i32)));
                Ok(combo(&lists.compression, image_format_to_str(format)))
            }
        }
    };
    let text_option = |settings: &MemIniFile, name: &str| -> String {
        given(name).map_or_else(|| read_string(settings, name, ""), str::to_owned)
    };
    let objects_lod = bool_option(settings, "ObjectsLOD", true)?;
    let build_atlas = bool_option(settings, "BuildAtlas", true)?;
    let atlas_width = combo_option(
        settings,
        "AtlasWidth",
        &lists.atlas_size,
        &defaults.atlas_width.to_string(),
    )?;
    let atlas_height = combo_option(
        settings,
        "AtlasHeight",
        &lists.atlas_size,
        &defaults.atlas_height.to_string(),
    )?;
    let texture_size = combo_option(settings, "AtlasTextureSize", &lists.texture_size, "512")?;
    let uv_range = combo_option(
        settings,
        "AtlasTextureUVRange",
        &lists.uv_range,
        &xedit_core::delphi::float_to_str_f_fixed(f64::from(defaults.uv_range), 1),
    )?;
    let diffuse = format_option(settings, "AtlasDiffuseFormat", defaults.atlas_diffuse_format)?;
    let normal = format_option(settings, "AtlasNormalFormat", defaults.atlas_normal_format)?;
    let specular = format_option(settings, "AtlasSpecularFormat", defaults.atlas_specular_format)?;
    let alpha_threshold = combo_option(
        settings,
        "DefaultAlphaThreshold",
        &lists.alpha_threshold,
        &defaults.alpha_threshold.to_string(),
    )?;
    let no_tangents = bool_option(settings, "ObjectsNoTangents", false)?;
    let no_vertex_colors = bool_option(settings, "ObjectsNoVertexColors", false)?;
    let use_alpha_threshold = bool_option(settings, "ObjectsUseAlphaThreshold", false)?;
    let use_backlight_power = bool_option(settings, "ObjectsUseBacklightPower", false)?;
    let chunk = bool_option(settings, "Chunk", false)?;
    let lod_level = combo_option(settings, "LODLevel", &lists.lod_level, "")?;
    let (lod_x, lod_y) = (text_option(settings, "LODX"), text_option(settings, "LODY"));
    let (lod_x2, lod_y2) = (text_option(settings, "LODX2"), text_option(settings, "LODY2"));
    let mut trees_lod = bool_option(settings, "TreesLOD", true)?;
    let trees_3d = bool_option(settings, "Trees3D", false)?;
    let brightness = combo_option(settings, "TreesBrightness", &lists.brightness, "0")?;
    if is_fallout4() || is_starfield() {
        // the form unchecks and disables the trees LOD
        trees_lod = false;
    }
    let mut write = |name: &str, value: &str| settings.write_string(&section, name, value);
    let b = |value: bool| if value { "1" } else { "0" };
    write("ObjectsLOD", b(objects_lod));
    write("BuildAtlas", b(build_atlas));
    write("AtlasWidth", &atlas_width);
    write("AtlasHeight", &atlas_height);
    write("AtlasTextureSize", &texture_size);
    write("AtlasTextureUVRange", &uv_range);
    write(
        "AtlasDiffuseFormat",
        &(str_to_image_format(&diffuse) as i32).to_string(),
    );
    write("AtlasNormalFormat", &(str_to_image_format(&normal) as i32).to_string());
    write(
        "AtlasSpecularFormat",
        &(str_to_image_format(&specular) as i32).to_string(),
    );
    write("DefaultAlphaThreshold", &alpha_threshold);
    write("ObjectsNoTangents", b(no_tangents));
    write("ObjectsNoVertexColors", b(no_vertex_colors));
    write("ObjectsUseAlphaThreshold", b(use_alpha_threshold));
    write("ObjectsUseBacklightPower", b(use_backlight_power));
    write("Chunk", b(chunk));
    write("LODLevel", &lod_level);
    write("LODX", &lod_x);
    write("LODY", &lod_y);
    // Fallouts can have only a single atlas, so no options here
    if is_fallout3() {
        write("BuildAtlas", "1");
        write("AtlasTextureSize", "1024");
        write("AtlasTextureUVRange", "10000");
        write("ObjectsNoTangents", "0");
        write("ObjectsNoVertexColors", "1");
        // area settings are for FO3/FNV only
        write("LODX2", &lod_x2);
        write("LODY2", &lod_y2);
    }
    write("TreesLOD", b(trees_lod));
    write("Trees3D", b(trees_lod && trees_3d));
    write("TreesBrightness", &brightness);
    Ok((objects_lod, trees_lod))
}

/// The line break of the messages.
const CRLF: &str = "\r\n";

/// The progress messages of the run, kept as the message log shows them.
fn capture_messages() -> Arc<Mutex<Vec<String>>> {
    let messages = Arc::new(Mutex::new(Vec::new()));
    let sink = messages.clone();
    set_progress_callback(Some(Arc::new(move |status: &str| {
        eprintln!("{status}");
        // `AddMessage`: a message of several lines is several lines of the log
        let mut sink = sink.lock().unwrap();
        for line in status.split(CRLF) {
            sink.push(line.to_owned());
        }
    })));
    messages
}

/// `FormatDateTime('nn:ss', ...)` of the time since the start (with the
/// hours in front, `wbFormatElapsedTime`).
fn elapsed_since(start: std::time::Instant) -> String {
    let seconds = start.elapsed().as_secs();
    let hours = seconds / 3600;
    let text = format!("{:02}:{:02}", (seconds / 60) % 60, seconds % 60);
    if hours > 0 { format!("{hours}:{text}") } else { text }
}

fn lodgen_generate(session: &mut Session, request: LodgenRequest) -> Result<LodgenResponse, CommandError> {
    session.mode()?;
    if is_fallout76() || is_starfield() {
        return Err(CommandError::new("unsupported", "LOD generation not supported."));
    }
    // the tmLODgen overrides of `xeInit`
    set_tool_mode(ToolMode::tmLODgen);
    set_allow_internal_edit(false);
    set_show_internal_edit(false);
    // the GUI names records with the shorter names
    let shorter_names = xedit_core::interface::globals::display_shorter_names();
    xedit_core::interface::globals::set_display_shorter_names(true);
    let data = folder(&request.data.clone().unwrap_or_else(data_path));
    let exe_folder = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| folder(&parent.display().to_string())))
        .unwrap_or_default();
    let scripts = request
        .scripts
        .as_deref()
        .map(folder)
        .unwrap_or_else(|| format!("{exe_folder}Edit Scripts\\"));
    let output = request.output.as_deref().map(folder).unwrap_or_else(|| data.clone());
    let temp = request.temp.as_deref().map(folder).unwrap_or_else(|| {
        folder(
            &std::env::temp_dir()
                .join(format!("{}Edit", app_name()))
                .display()
                .to_string(),
        )
    });
    let mut settings = match &request.settings {
        Some(path) => MemIniFile::load(Path::new(path)),
        None => MemIniFile::default(),
    };
    // the defaults the form sets for Fallout 4 and Starfield
    let mut defaults = LodDefaults::initial();
    if is_fallout4() || is_starfield() {
        defaults.atlas_width = 4096;
        defaults.atlas_height = 4096;
        defaults.uv_range = 1.1;
        defaults.atlas_diffuse_format = ImageFormat::Dxt5;
        defaults.atlas_normal_format = ImageFormat::Ati2n;
    }

    let previous = xedit_core::interface::misc::progress_callback();
    let messages = capture_messages();
    let used_seed = std::cell::Cell::new(None);
    let result = (|| -> Result<(Vec<LodgenWorldspace>, Vec<String>, LodEnv), CommandError> {
        let files: Vec<Arc<FileImpl>> = xedit_core::interface::element::files()
            .iter()
            .filter_map(|file| file.as_element_impl().and_then(|element| element.file_impl()))
            .collect();
        let plugins: Vec<String> = files
            .iter()
            .filter(|file| !file.get_file_states().contains(FileState::fsIsHardcoded))
            .map(|file| file.get_name())
            .collect();
        let resources = load_resources(&data, request.game_ini.as_deref(), &plugins);
        let mut list: Vec<MainRecordRef> = worldspaces_for_lod(&files);
        let oblivion = game_mode() == GameMode::gmTES4;
        let mut checked: Vec<bool> = vec![false; list.len()];
        if !oblivion {
            // default selected worldspace at the top
            if let Some(j) = list.iter().position(|ws| {
                let id = ws.get_load_order_form_id().to_cardinal();
                id == 0x3C || (game_mode() == GameMode::gmFNV && id == 0x000D_A726)
            }) {
                let ws = list.remove(j);
                list.insert(0, ws);
                checked[0] = true;
            }
            if list.len() == 1 {
                checked[0] = true;
            }
        }
        if !request.worldspaces.is_empty() {
            checked = vec![false; list.len()];
            for wanted in &request.worldspaces {
                let index = list
                    .iter()
                    .position(|ws| ws.get_editor_id().eq_ignore_ascii_case(wanted))
                    .ok_or_else(|| {
                        invalid(format!(
                            "{wanted} is not a worldspace of the LODGen list; the list is {}",
                            list.iter().map(|ws| ws.get_editor_id()).collect::<Vec<_>>().join(", ")
                        ))
                    })?;
                checked[index] = true;
            }
        } else if oblivion {
            checked = vec![true; list.len()];
        }
        let worldspaces = list
            .iter()
            .zip(&checked)
            .map(|(ws, checked)| LodgenWorldspace {
                editor_id: ws.get_editor_id(),
                name: ws.get_name(),
                checked: *checked,
            })
            .collect();
        let (objects_lod, trees_lod) = if oblivion {
            (false, false)
        } else {
            if !checked.iter().any(|c| *c) {
                return Err(invalid("Select worldspace(s) for LOD generation"));
            }
            form_options(&mut settings, &request.options, &defaults)?
        };
        let env = LodEnv {
            data_path: data.clone(),
            output_path: output.clone(),
            scripts_path: scripts.clone(),
            temp_path: temp.clone(),
            settings: settings.clone(),
            defaults,
        };
        if request.dry_run {
            return Ok((worldspaces, resources, env));
        }
        if let Some(path) = &request.settings {
            crate::save::write_atomically(Path::new(path), &xedit_io::encoding::ansi_bytes(&settings.to_text()))?;
        }
        let seed = match request.seed {
            Some(seed) => {
                set_rand_seed(seed);
                seed
            }
            None => randomize(),
        };
        used_seed.set(Some(seed));
        let start = std::time::Instant::now();
        let failed = |error: xedit_assets::lod::LodError| CommandError::new("edit_failed", error.0);
        if oblivion {
            progress(&format!("[{}] LOD Generator: starting", elapsed_since(start)));
            for (ws, checked) in list.iter().zip(&checked) {
                if *checked && let Err(error) = generate_lod_tes4(&env, ws, &|| elapsed_since(start)) {
                    progress(&format!("[{}] LOD Generator: <Error: {}>", elapsed_since(start), error));
                    return Err(failed(error));
                }
            }
            progress(&format!(
                "[{}] LOD Generator: finished (you can close this application now)",
                elapsed_since(start)
            ));
        } else {
            for (ws, checked) in list.iter().zip(&checked) {
                if !*checked {
                    continue;
                }
                if is_skyrim() || is_fallout3() {
                    generate_lod_tes5(&env, ws, trees_lod, objects_lod, &files).map_err(failed)?;
                } else {
                    generate_lod_fo4(&env, ws).map_err(failed)?;
                }
            }
            progress("LOD Generator: finished (you can close this application now)");
        }
        Ok((worldspaces, resources, env))
    })();
    set_progress_callback(previous);
    xedit_core::interface::globals::set_display_shorter_names(shorter_names);
    let messages = std::mem::take(&mut *messages.lock().unwrap());
    let (worldspaces, resources, env) = result?;
    let section = format!("{} LOD Options", app_name());
    let options = OPTION_NAMES
        .iter()
        .map(|name| (name.to_string(), env.settings.read_string(&section, name, "")))
        .collect();
    Ok(LodgenResponse {
        worldspaces,
        options,
        resources,
        messages,
        seed: used_seed.get(),
        dry_run: request.dry_run,
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "lodgen.generate",
        "Generate the LOD of worldspaces as xEdit's LODGen mode does (wbGenerateLODTES5, wbGenerateLODFO4, wbGenerateLODTES4), with the options of the LODGen form; writes the LOD files below the output folder.",
        true,
        lodgen_generate,
    );
}
